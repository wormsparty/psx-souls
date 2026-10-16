//! Online co-op for two, in the manner of the souls' co-op mods: both play in the host's world
//! (defeated bosses, checkpoints, where it stands), each one with their own character (weapon,
//! inventory, discoveries). Bosses defeated together also count in the guest's save.
//!
//! It starts from a brazier: "Host" or "Join", with an optional password (only players with the
//! same one find each other). While it searches the character raises their arms and can't move;
//! after `SEARCH_SECS` without a partner, it gives up. The opponents are sturdier for two
//! (`sim::fighter::party_hp`). When the co-op ends, the host stays in its world, the guest goes
//! back to theirs.
//!
//! Network model: peer-to-peer rollback (GGRS). Every peer simulates the whole game, only the
//! inputs travel (`PlayerInput`, 10 bytes per tick). A remote input that hasn't arrived yet is
//! predicted (the last one, repeated); when it arrives and differs, the simulation goes back to
//! the last correct state (`sim::snapshot`) and replays the ticks since. Hits, guards and perfect
//! guards are thus decided the same way on every machine. A checksum of the state
//! (`sim::state_hash`) is compared regularly: a desync is reported.
//!
//! Connection: a signaling server (`matchbox_server`, see `tools/lobby.sh`) puts the peers of a
//! room in touch (the room's name comes from the password); they then talk directly (WebRTC,
//! native and browser alike). The host waits for a guest, then sends it the start: the world and
//! both characters.
//!
//! In co-op the game never pauses: an open menu only stops the local player's inputs.

pub mod socket;

use std::collections::VecDeque;
use std::time::Duration;

use bevy::prelude::*;
use ggrs::{DesyncDetection, GgrsEvent, GgrsRequest, InputStatus, P2PSession, PlayerType, SessionBuilder, SessionState};
use matchbox_socket::PeerId;
use serde::{Deserialize, Serialize};

use crate::render::AppState;
use crate::save::SaveSlot;
use crate::sim::fighter::Health;
use crate::sim::player::{PState, Player};
use crate::sim::data::Tuning;
use crate::sim::encounter::{Encounter, Progress, SimCommand, SimCommands, apply_commands};
use crate::sim::input::{MAX_PLAYERS, PlayerInput, PlayerInputs};
use crate::sim::snapshot::{self, Snapshot};
use crate::sim::{NextSimId, ResetFight, SimDebug, SimEvents, SimTick, rng::SimRng, run_sim_tick, stable_hash, state_hash};

/// Protocol version: peers of different versions refuse each other.
const VERSION: u32 = 1;
/// Signaling server by default (`tools/lobby.sh`): `SOULS_LOBBY` (native) or `?lobby=` (web)
/// to change it, `SOULS_ROOM` / `?room=` for the room.
pub const DEFAULT_LOBBY: &str = "ws://127.0.0.1:3536";
const DEFAULT_ROOM: &str = "psx-souls";
/// Local inputs are delayed by this many ticks: fewer rollbacks, for 33 ms of latency.
const INPUT_DELAY: usize = 2;
/// At most this many ticks predicted ahead of the last confirmed input (then it waits).
const MAX_PREDICTION: usize = 8;
/// Without news from a peer for this long, they're gone.
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// Without a partner after this long, the search gives up.
pub const SEARCH_SECS: f32 = 120.0;
/// How long a notice stays on screen (partner gone, nobody found…).
const NOTICE_SECS: f32 = 6.0;

/// The rollback session's types.
#[derive(Debug)]
pub struct Coop;

impl ggrs::Config for Coop {
    type Input = PlayerInput;
    type InputPredictor = ggrs::PredictRepeatLast;
    type State = Snapshot;
    type Address = PeerId;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Solo,
    Host,
    Guest,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// Reaching the signaling server.
    Connecting,
    /// In the room: the host waits for its players, a guest for the host's start.
    Lobby,
    /// Started: the peers synchronise.
    Syncing,
    Running,
}

/// Why the co-op stopped, or didn't start (shown for a few seconds).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Notice {
    /// Signaling server unreachable (or connection lost).
    Unreachable(String),
    /// The host already has a partner.
    Full,
    /// The other one runs another version (or other tuning files).
    Version,
    /// The host has left: back to our own world.
    HostLeft,
    /// The guest has left: alone again.
    GuestLeft,
    /// Nobody within `SEARCH_SECS`.
    NobodyFound,
    /// Hit (or worse) while searching.
    Interrupted,
}

/// Lobby messages (reliable channel).
#[derive(Serialize, Deserialize, Clone, Debug)]
enum Msg {
    /// Sent to each peer once connected.
    Hello { version: u32, tuning: u64, host: bool, character: Progress },
    /// From the host: the game starts. `order`: the guests, by player number (from 1), and their
    /// characters.
    Start { order: Vec<PeerId>, world: Progress, guests: Vec<Progress> },
    /// From the host: no room for you.
    Refused(Notice),
}

/// Id of the player controlled on this machine (0 in solo and for the host).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalId(pub u8);

/// Connection quality, for the HUD.
#[derive(Clone, Debug, Default)]
pub struct NetStatus {
    /// Round trip to the furthest peer.
    pub ping_ms: Option<u32>,
    /// Silent peer (it may come back).
    pub interrupted: bool,
    /// First tick on which the peers' states differed.
    pub desync: Option<i32>,
    /// Players still connected (us included).
    pub players: u8,
}

#[derive(Resource, Default)]
pub struct Net {
    pub role: Role,
    pub phase: Phase,
    /// Time spent searching (seconds).
    pub searching_for: f32,
    pub notice: Option<Notice>,
    /// Time left showing the notice.
    pub notice_for: f32,
    pub status: NetStatus,
    /// The guest's own world when the co-op started: they go back to it, with their character
    /// (`save`).
    pub guest_base: Option<Progress>,
    socket: Option<socket::Socket>,
    host: Option<PeerId>,
    /// Host: the guest who said hello.
    guest: Option<(PeerId, Progress)>,
    session: Option<P2PSession<Coop>>,
    /// Start received or sent: the world and the guest's character, applied once synchronised.
    start: Option<(Progress, Vec<Progress>)>,
    /// Our player number, and the number of players.
    local: u8,
    players: u8,
    /// Peers by player number (`None`: us).
    peers: Vec<Option<PeerId>>,
    /// Menu decisions waiting to go out (one per tick).
    outbox: VecDeque<SimCommand>,
    seq: u8,
    /// Last tick whose events went to the presentation: replaying a tick after a rollback
    /// doesn't play its sounds again.
    presented: u32,
    /// Ticks to wait (the others are behind).
    skip: u32,
}

impl Net {
    /// A co-op game is going on (the world is shared).
    pub fn coop(&self) -> bool {
        self.phase == Phase::Running
    }
    /// Looking for a partner (the character stands still, arms raised).
    pub fn searching(&self) -> bool {
        matches!(self.phase, Phase::Connecting | Phase::Lobby | Phase::Syncing)
    }
}

pub fn solo(net: Res<Net>) -> bool {
    !net.coop()
}

pub fn coop_running(net: Res<Net>) -> bool {
    net.phase == Phase::Running
}

/// Address of the room: signaling server and room name (from the password: only those who
/// have the same one meet).
fn room_url(password: &str) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    let (lobby, room) = (std::env::var("SOULS_LOBBY").ok(), std::env::var("SOULS_ROOM").ok());
    #[cfg(target_arch = "wasm32")]
    let (lobby, room) = {
        let (mut lobby, mut room) = (None, None);
        let location = web_sys::window().map(|w| w.location());
        let search = location.as_ref().and_then(|l| l.search().ok()).unwrap_or_default();
        for pair in search.trim_start_matches('?').split('&') {
            match pair.split_once('=') {
                Some(("lobby", v)) => lobby = Some(v.to_string()),
                Some(("room", v)) => room = Some(v.to_string()),
                _ => {}
            }
        }
        // By default, the machine serving the page.
        if lobby.is_none()
            && let Some(host) = location.and_then(|l| l.hostname().ok()).filter(|h| !h.is_empty())
        {
            lobby = Some(format!("ws://{host}:3536"));
        }
        (lobby, room)
    };
    let lobby = lobby.unwrap_or_else(|| DEFAULT_LOBBY.into());
    format!("{}/{}", lobby.trim_end_matches('/'), room_name(&room.unwrap_or_else(|| DEFAULT_ROOM.into()), password))
}

/// Room for a password (hashed: it doesn't travel in clear to the server).
pub fn room_name(base: &str, password: &str) -> String {
    let password = password.trim();
    if password.is_empty() { base.to_string() } else { format!("{base}-{:016x}", stable_hash(password)) }
}

/// The tuning files must be the same everywhere (the simulation depends on them).
fn tuning_hash(t: &Tuning) -> u64 {
    stable_hash(&format!("{t:?}"))
}

/// The character (and world) this machine brings: where it stands right now.
fn own_character(w: &mut World) -> Progress {
    #[allow(unused_mut)]
    let mut p = crate::save::capture(w).unwrap_or_else(|| Progress::new_game(w.resource::<Tuning>()));
    // Debug: both start in front of a boss (`SOULS_BOSS=n`, host's side).
    #[cfg(not(target_arch = "wasm32"))]
    if w.resource::<Net>().role == Role::Host
        && let Some(n) = std::env::var("SOULS_BOSS").ok().and_then(|n| n.parse().ok())
    {
        p.arena = Some(n);
    }
    p
}

/// Starts looking for a partner (from a brazier): as the host (`host`), or to join one.
pub fn search(w: &mut World, host: bool, password: &str) {
    *w.resource_mut::<Net>() = Net {
        role: if host { Role::Host } else { Role::Guest },
        phase: Phase::Connecting,
        socket: Some(socket::Socket::open(&room_url(password))),
        ..default()
    };
}

/// Stops the co-op, or the search. In a co-op game, the host stays in its world (its partner's
/// character vanishes), the guest goes back to theirs with their character.
pub fn leave(w: &mut World, notice: Option<Notice>) {
    let net = w.resource::<Net>();
    let (coop, role) = (net.coop(), net.role);
    if coop {
        match role {
            Role::Guest => {
                // Own world, own character: saved, then the world comes back.
                let progress = crate::save::capture(w);
                if let Some(p) = progress.clone() {
                    let play_time = w.resource::<SaveSlot>().play_time;
                    w.resource_mut::<SaveSlot>().store(crate::save::SaveData { version: crate::save::VERSION, play_time, progress: p });
                }
                *w.resource_mut::<ResetFight>() = ResetFight { requested: true, players: 1, progress, guests: Vec::new() };
            }
            _ => {
                w.resource_mut::<SimCommands>().0.push(SimCommand::Leave { player: 1 });
                let _ = w.run_system_cached(apply_commands);
                crate::save::save_now(w);
            }
        }
    }
    let notice_for = if notice.is_some() { NOTICE_SECS } else { 0.0 };
    *w.resource_mut::<Net>() = Net { notice, notice_for, ..default() };
    w.insert_resource(LocalId(0));
}

/// A decision taken in a menu (equip, travel…). Solo: applied at once (the game is paused).
/// Co-op: it goes out with the next inputs, so that every peer applies it on the same tick.
pub fn send_command(w: &mut World, cmd: SimCommand) {
    if w.resource::<Net>().coop() {
        w.resource_mut::<Net>().outbox.push_back(cmd);
    } else {
        w.resource_mut::<SimCommands>().0.push(cmd);
        let _ = w.run_system_cached(apply_commands);
    }
}

/// What the search is doing, and the time left (HUD).
pub fn search_text(net: &Net) -> String {
    use crate::lang::tr;
    let what = match (net.phase, net.role) {
        (Phase::Connecting, _) => tr("Connecting to the lobby…", "Connexion au salon…"),
        (Phase::Lobby, Role::Host) => tr("Waiting for a partner…", "En attente d'un partenaire…"),
        (Phase::Lobby, _) => tr("Looking for a host…", "Recherche d'un hôte…"),
        _ => tr("Joining…", "Connexion au partenaire…"),
    };
    let left = (SEARCH_SECS - net.searching_for).max(0.0) as u32;
    format!("{what} {}:{:02}", left / 60, left % 60)
}

pub fn notice_text(n: Option<&Notice>) -> String {
    use crate::lang::tr;
    match n {
        None => String::new(),
        Some(Notice::Unreachable(why)) => format!("{} ({why})", tr("Cannot reach the lobby server", "Serveur de salon injoignable")),
        Some(Notice::Full) => tr("The host already has a partner.", "L'hôte a déjà un partenaire.").into(),
        Some(Notice::Version) => tr("The other player runs a different version of the game.", "L'autre joueur a une version différente du jeu.").into(),
        Some(Notice::HostLeft) => tr("The host has left: back to your world.", "L'hôte est parti : retour dans votre monde.").into(),
        Some(Notice::GuestLeft) => tr("Your partner has left.", "Votre partenaire est parti.").into(),
        Some(Notice::NobodyFound) => tr("Nobody found.", "Personne n'a été trouvé.").into(),
        Some(Notice::Interrupted) => tr("Search interrupted.", "Recherche interrompue.").into(),
    }
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Net>()
            .init_resource::<LocalId>()
            .add_systems(PreUpdate, (lobby, search_clock).chain())
            .add_systems(OnEnter(AppState::Title), |w: &mut World| leave(w, None));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(OnEnter(AppState::Playing), auto_coop);
    }
}

/// The search's clock (gives up after `SEARCH_SECS`), the notices' display time; hit while
/// searching, the search stops.
fn search_clock(world: &mut World) {
    let dt = world.resource::<Time>().delta_secs();
    let local = world.resource::<LocalId>().0;
    let shaken = {
        let mut q = world.query::<(&Player, &Health)>();
        q.iter(world).find(|(p, _)| p.id == local).is_some_and(|(p, h)| p.state != PState::Free || h.dead())
    };
    let mut net = world.resource_mut::<Net>();
    net.notice_for = (net.notice_for - dt).max(0.0);
    if !net.searching() {
        return;
    }
    net.searching_for += dt;
    let end = if net.searching_for >= SEARCH_SECS {
        Some(Notice::NobodyFound)
    } else if shaken && net.phase != Phase::Syncing {
        Some(Notice::Interrupted)
    } else {
        None
    };
    if end.is_some() {
        leave(world, end);
    }
}

/// `SOULS_COOP=host` or `join` (`SOULS_PASSWORD=…`): the search starts as soon as the game does.
#[cfg(not(target_arch = "wasm32"))]
fn auto_coop(mut commands: Commands, mut done: Local<bool>) {
    if std::mem::replace(&mut *done, true) {
        return;
    }
    let Ok(role) = std::env::var("SOULS_COOP") else { return };
    let password = std::env::var("SOULS_PASSWORD").unwrap_or_default();
    commands.queue(move |w: &mut World| match role.as_str() {
        "host" => search(w, true, &password),
        "join" => search(w, false, &password),
        _ => warn!("SOULS_COOP: host or join"),
    });
}

/// Room life: hellos, the host's start, synchronisation, then the co-op's launch.
fn lobby(world: &mut World) {
    if world.resource::<Net>().socket.is_none() {
        return;
    }
    let mut end = None;
    let mut launch = false;
    let tuning = tuning_hash(world.resource::<Tuning>());
    // Our character (the host's: its world too), as it stands — still, arms raised.
    let character = if world.resource::<Net>().searching() { Some(own_character(world)) } else { None };
    world.resource_scope(|_, mut net: Mut<Net>| {
        let net = &mut *net;
        let role = net.role;
        let Some(sock) = net.socket.as_mut() else { return };
        if let Some(why) = sock.failure() {
            if matches!(net.phase, Phase::Connecting | Phase::Lobby) {
                end = Some(Notice::Unreachable(why));
            }
            return;
        }
        let me = sock.id();
        if net.phase == Phase::Connecting && me.is_some() {
            net.phase = Phase::Lobby;
        }
        let character = character.unwrap_or_default();
        // Session to create once the messages are read: the peers by number, our number.
        let mut session = None;
        for (peer, up) in sock.update() {
            if up {
                sock.send(peer, &Msg::Hello { version: VERSION, tuning, host: role == Role::Host, character: character.clone() });
            } else if role == Role::Host && net.phase == Phase::Lobby && net.guest.as_ref().is_some_and(|g| g.0 == peer) {
                net.guest = None;
            } else if role == Role::Guest && net.host == Some(peer) && net.phase == Phase::Lobby {
                net.host = None;
            }
        }
        for (peer, msg) in sock.receive::<Msg>() {
            match msg {
                Msg::Hello { version, tuning: theirs, host, character } => {
                    let compatible = version == VERSION && theirs == tuning;
                    match role {
                        Role::Host if host => {}
                        Role::Host if !compatible => sock.send(peer, &Msg::Refused(Notice::Version)),
                        Role::Host if net.phase == Phase::Lobby && net.guest.is_none() => net.guest = Some((peer, character)),
                        Role::Host => sock.send(peer, &Msg::Refused(Notice::Full)),
                        Role::Guest if host && !compatible => end = Some(Notice::Version),
                        Role::Guest if host => net.host = Some(peer),
                        _ => {}
                    }
                }
                Msg::Start { order, world, guests } if role == Role::Guest && net.host == Some(peer) && net.phase == Phase::Lobby => {
                    if me.is_some_and(|me| order.first() == Some(&me)) {
                        net.start = Some((world, guests));
                        session = Some((vec![Some(peer), None], 1));
                    } else {
                        end = Some(Notice::Full);
                    }
                }
                // Another guest got there first: keep looking.
                Msg::Refused(Notice::Full) if role == Role::Guest => {
                    if net.host == Some(peer) {
                        net.host = None;
                    }
                }
                Msg::Refused(n) if role == Role::Guest => end = Some(n),
                _ => {}
            }
        }
        // Host: a partner is there, let's go.
        if role == Role::Host
            && net.phase == Phase::Lobby
            && let Some((guest, theirs)) = net.guest.clone()
        {
            sock.send(guest, &Msg::Start { order: vec![guest], world: character.clone(), guests: vec![theirs.clone()] });
            net.start = Some((character, vec![theirs]));
            session = Some((vec![None, Some(guest)], 0));
        }
        if let Some((peers, local)) = session {
            start_session(net, peers, local);
        }
        if net.phase == Phase::Syncing
            && let Some(s) = net.session.as_mut()
        {
            s.poll_remote_clients();
            for e in s.events() {
                debug!("co-op: {e:?}");
            }
            if s.current_state() == SessionState::Running {
                net.phase = Phase::Running;
                launch = true;
            }
        }
    });
    if let Some(n) = end {
        leave(world, Some(n));
    } else if launch {
        launch_game(world);
    }
}

/// Creates the rollback session: players by number, us at `local`.
fn start_session(net: &mut Net, peers: Vec<Option<PeerId>>, local: u8) {
    let Some(channel) = net.socket.as_mut().and_then(|s| s.take_ggrs()) else { return };
    let n = peers.len();
    let mut b = match SessionBuilder::<Coop>::new().with_num_players(n) {
        Ok(b) => b,
        Err(e) => return warn!("co-op: {e}"),
    }
    .with_input_delay(INPUT_DELAY)
    .with_max_prediction_window(MAX_PREDICTION)
    .with_disconnect_timeout(DISCONNECT_TIMEOUT)
    .with_desync_detection_mode(DesyncDetection::On { interval: 30 });
    for (handle, peer) in peers.iter().enumerate() {
        let kind = match peer {
            Some(p) => PlayerType::Remote(*p),
            None => PlayerType::Local,
        };
        b = match b.add_player(kind, handle) {
            Ok(b) => b,
            Err(e) => return warn!("co-op: {e}"),
        };
    }
    match b.start_p2p_session(channel) {
        Ok(s) => {
            net.session = Some(s);
            net.peers = peers;
            net.local = local;
            net.players = n as u8;
            net.phase = Phase::Syncing;
            info!("co-op: {n} players, we are number {local}");
        }
        Err(e) => warn!("co-op: {e}"),
    }
}

/// Synchronised: both start from the same state, in the host's world (right where it stands).
fn launch_game(w: &mut World) {
    // The guest's own world, to go back to (and to keep in their save meanwhile).
    let base = crate::save::capture(w);
    let (local, players, start) = {
        let mut net = w.resource_mut::<Net>();
        net.presented = 0;
        net.guest_base = base.filter(|_| net.role == Role::Guest);
        (net.local, net.players, net.start.take())
    };
    let Some((world, guests)) = start else { return };
    // Nothing from the solo game may remain: the simulation must start identical on both sides.
    *w.resource_mut::<SimTick>() = SimTick::default();
    *w.resource_mut::<SimRng>() = SimRng::default();
    *w.resource_mut::<Encounter>() = Encounter::default();
    *w.resource_mut::<NextSimId>() = NextSimId::default();
    *w.resource_mut::<SimDebug>() = SimDebug::default();
    *w.resource_mut::<PlayerInputs>() = PlayerInputs::default();
    w.resource_mut::<SimEvents>().0.clear();
    w.resource_mut::<SimCommands>().0.clear();
    *w.resource_mut::<ResetFight>() = ResetFight { requested: true, players, progress: Some(world), guests };
    w.insert_resource(LocalId(local));
}

/// One co-op tick (in `FixedUpdate`): our input goes out, GGRS says what to do — save the
/// state, go back to an earlier one, simulate a tick with everyone's inputs.
pub fn coop_tick(world: &mut World) {
    let mut end = None;
    world.resource_scope(|world, mut net: Mut<Net>| {
        let net = &mut *net;
        let Some(session) = net.session.as_mut() else { return };
        session.poll_remote_clients();
        for e in session.events() {
            match e {
                GgrsEvent::WaitRecommendation { skip_frames } => net.skip = skip_frames,
                GgrsEvent::NetworkInterrupted { .. } => net.status.interrupted = true,
                GgrsEvent::NetworkResumed { .. } => net.status.interrupted = false,
                GgrsEvent::Disconnected { addr } => {
                    info!("co-op: {addr} disconnected");
                    net.status.interrupted = false;
                    end = Some(if net.role == Role::Guest { Notice::HostLeft } else { Notice::GuestLeft });
                }
                GgrsEvent::DesyncDetected { frame, local_checksum, remote_checksum, addr } => {
                    error!("co-op: desync with {addr} at frame {frame} ({local_checksum:x} ≠ {remote_checksum:x})");
                    net.status.desync.get_or_insert(frame);
                }
                _ => {}
            }
        }
        if session.current_state() != SessionState::Running {
            return;
        }
        if net.skip > 0 {
            net.skip -= 1;
            return;
        }
        let mut input = world.resource::<crate::input::LocalInput>().0;
        if let Some(c) = net.outbox.pop_front() {
            net.seq = net.seq.wrapping_add(1).max(1);
            input.cmd = c.encode(net.seq);
        }
        if let Err(e) = session.add_local_input(net.local as usize, input) {
            return warn!("co-op: {e}");
        }
        match session.advance_frame() {
            Ok(requests) => run_requests(world, requests, &mut net.presented),
            Err(e) => return warn!("co-op: {e}"),
        }
        // Connection quality, now and then.
        if world.resource::<SimTick>().0 % 30 == 0 {
            let remotes = session.remote_player_handles();
            net.status.ping_ms = remotes.iter().filter_map(|h| session.network_stats(*h).ok()).map(|s| s.ping as u32).max();
            net.status.players = 1 + remotes.iter().filter(|h| session.network_stats(**h).is_ok()).count() as u8;
        }
    });
    if let Some(n) = end {
        leave(world, Some(n));
    }
}

/// Does what GGRS asks: save the state, go back to an earlier one, simulate a tick with
/// everyone's inputs. `presented`: the last tick whose events went to the presentation (the
/// events of a replayed tick are dropped).
pub fn run_requests(world: &mut World, requests: Vec<GgrsRequest<Coop>>, presented: &mut u32) {
    for r in requests {
        match r {
            GgrsRequest::SaveGameState { cell, frame } => {
                let snap = snapshot::save(world);
                let sum = state_hash(world) as u128;
                cell.save(frame, Some(snap), Some(sum));
            }
            GgrsRequest::LoadGameState { cell, .. } => {
                if let Some(s) = cell.load() {
                    snapshot::load(world, &s);
                }
            }
            GgrsRequest::AdvanceFrame { inputs } => {
                let mut all = PlayerInputs::default();
                for (i, (inp, status)) in inputs.into_iter().enumerate().take(MAX_PLAYERS) {
                    all.0[i] = match status {
                        // Gone: their character vanishes (the same tick everywhere).
                        InputStatus::Disconnected => PlayerInput { cmd: SimCommand::Leave { player: i as u8 }.encode(u8::MAX), ..default() },
                        _ => inp,
                    };
                }
                *world.resource_mut::<PlayerInputs>() = all;
                let before = world.resource::<SimEvents>().0.len();
                run_sim_tick(world);
                let tick = world.resource::<SimTick>().0;
                if tick <= *presented {
                    world.resource_mut::<SimEvents>().0.truncate(before);
                } else {
                    *presented = tick;
                }
            }
        }
    }
}
