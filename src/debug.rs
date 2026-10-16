//! Tuning tools: state overlay, hitboxes, slow motion, passive boss. In co-op, only those that
//! don't touch the simulation (F1, F2): the peers would no longer simulate the same game.

use bevy::prelude::*;

use crate::render::{AppState, LocalPlayer};
use crate::sim::boss::Boss;
use crate::sim::combat::hit_capsule;
use crate::sim::data::{MoveDef, Tuning};
use crate::sim::fighter::{Action, Body};
use crate::sim::player::{PState, Player};
use crate::sim::{SimDebug, SimTick, TICK_HZ};

#[derive(Resource, Default)]
struct DebugUi {
    overlay: bool,
    hitboxes: bool,
    slow: bool,
}

#[derive(Component)]
struct DebugText;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugUi>()
            .add_systems(Startup, |mut commands: Commands, font: Res<crate::ui::UiFont>| {
                commands.spawn((
                    Text::new(""),
                    font.at(1),
                    TextColor(Color::srgb(0.6, 1.0, 0.6)),
                    Node { position_type: PositionType::Absolute, left: px(28), top: px(96), ..default() },
                    DebugText,
                ));
            })
            .add_systems(Update, (keys, overlay, hitboxes).run_if(in_state(AppState::Playing)));
    }
}

fn keys(
    input: Res<ButtonInput<KeyCode>>,
    mut ui: ResMut<DebugUi>,
    mut sim: ResMut<SimDebug>,
    mut fixed: ResMut<Time<Fixed>>,
    mut reset: ResMut<crate::sim::ResetFight>,
    net: Res<crate::net::Net>,
) {
    let solo = !net.coop();
    if input.just_pressed(KeyCode::F5) && solo {
        // Respawns the fighters while keeping the progress (the boss starts over).
        reset.requested = true;
    }
    if input.just_pressed(KeyCode::F1) {
        ui.overlay = !ui.overlay;
    }
    if input.just_pressed(KeyCode::F2) {
        ui.hitboxes = !ui.hitboxes;
    }
    if input.just_pressed(KeyCode::F3) && solo {
        ui.slow = !ui.slow;
        // Slow motion: the sim runs at 15 ticks/s, timings in ticks stay the same.
        fixed.set_timestep_hz(if ui.slow { TICK_HZ / 4.0 } else { TICK_HZ });
    }
    if input.just_pressed(KeyCode::F4) && solo {
        sim.boss_passive = !sim.boss_passive;
    }
}

fn overlay(
    ui: Res<DebugUi>,
    sim: Res<SimDebug>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    players: Query<(&Player, &Action), With<LocalPlayer>>,
    bosses: Query<(&Boss, &Action)>,
    mut text: Single<&mut Text, With<DebugText>>,
) {
    if !ui.overlay {
        if !text.0.is_empty() {
            text.0.clear();
        }
        return;
    }
    let mut s = format!("tick {}{}{}\n", tick.0, if ui.slow { "  [SLOW x0.25]" } else { "" }, if sim.boss_passive { "  [PASSIVE BOSS]" } else { "" });
    if let Ok((p, a)) = players.single() {
        let window = p.perfect_window(&tuning);
        let since = tick.0.saturating_sub(p.guard_start);
        let in_perfect = matches!(p.state, PState::Guard) && since <= window;
        s += &format!(
            "player  {:?}  {:?} t{}\nstamina {:.0}  regain {:.0}  special {:.0}\nperfect guard: window {} ticks (spam {}) {}\n",
            p.state,
            a.mv,
            a.tick,
            p.stamina,
            p.regain,
            p.special,
            window,
            p.guard_spam,
            if in_perfect { "■ ACTIVE" } else { "" },
        );
    }
    for (b, a) in &bosses {
        let def: Option<&MoveDef> = a.def(&tuning);
        let next_hit = def.and_then(|d| d.hits.iter().find(|h| h.end > a.tick)).map(|h| h.start as i64 - a.tick as i64);
        s += &format!(
            "boss  phase {}  {:?} t{}/{}  hit in {:?}\nstagger {:.0}/{:.0}  pause {}\n",
            b.phase,
            a.mv,
            a.tick,
            def.map(|d| d.total).unwrap_or(0),
            next_hit,
            b.stagger,
            b.def(&tuning).stagger_max,
            b.idle,
        );
    }
    text.0 = s;
}

fn hitboxes(ui: Res<DebugUi>, tuning: Res<Tuning>, q: Query<(&Body, &Action)>, mut gizmos: Gizmos) {
    if !ui.hitboxes {
        return;
    }
    for (body, action) in &q {
        // Hurtbox.
        let iso = Isometry3d::from_translation(body.pos + Vec3::Y * body.height * 0.5);
        gizmos.primitive_3d(
            &Capsule3d::new(body.radius, (body.height - 2.0 * body.radius).max(0.01)),
            iso,
            Color::srgb(0.2, 0.8, 1.0),
        );
        let Some(def) = action.def(&tuning) else { continue };
        for h in &def.hits {
            let active = action.tick >= h.start && action.tick < h.end;
            let soon = action.tick < h.start && h.start - action.tick < 20;
            if !active && !soon {
                continue;
            }
            let (a, b, r) = hit_capsule(body, h, action.tick.max(h.start) as f32);
            let color = if !active {
                Color::srgba(1.0, 1.0, 0.2, 0.4)
            } else if h.fury {
                Color::srgb(1.0, 0.0, 0.0)
            } else {
                Color::srgb(1.0, 0.4, 0.1)
            };
            let mid = (a + b) * 0.5;
            let dir = (b - a).normalize_or_zero();
            let rot = Quat::from_rotation_arc(Vec3::Y, if dir == Vec3::ZERO { Vec3::Y } else { dir });
            gizmos.primitive_3d(&Capsule3d::new(r, a.distance(b)), Isometry3d::new(mid, rot), color);
        }
    }
}

/// Automatic screenshots and autopilot, to check the rendering without playing:
/// `SOULS_SHOTS=3,6,9 SOULS_SHOT_DIR=/tmp SOULS_AUTOPILOT=1 cargo run`
pub struct AutoShotPlugin;

#[derive(Resource)]
struct AutoShots {
    times: Vec<f32>,
    dir: String,
    done: usize,
}

impl Plugin for AutoShotPlugin {
    fn build(&self, app: &mut App) {
        // `SOULS_PERF=1`: average frame time and worst frame, every second, in the console.
        if std::env::var("SOULS_PERF").is_ok() {
            app.init_resource::<PerfClock>()
                .add_systems(First, |mut c: ResMut<PerfClock>| c.start = Some(std::time::Instant::now()))
                .add_systems(Last, perf_log);
        }
        let Ok(spec) = std::env::var("SOULS_SHOTS") else { return };
        let times = spec.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let dir = std::env::var("SOULS_SHOT_DIR").unwrap_or_else(|_| ".".into());
        app.insert_resource(AutoShots { times, dir, done: 0 })
            .add_systems(Update, autoshot.run_if(not(in_state(AppState::Loading))));
        // `SOULS_PAD=1`: gamepad labels.
        if std::env::var("SOULS_PAD").is_ok() {
            app.add_systems(PostStartup, |mut d: ResMut<crate::input::Device>| *d = crate::input::Device::Gamepad);
        }
        // No save writes during screenshots; `SOULS_TITLE` stays on the title screen.
        app.add_systems(PostStartup, |mut slot: ResMut<crate::save::SaveSlot>| slot.disabled = true);
        if std::env::var("SOULS_TITLE").is_err() {
            app.add_systems(OnEnter(AppState::Title), (|mut commands: Commands| {
                commands.queue(|w: &mut World| {
                    crate::menu::launch(w, crate::menu::Launch::New);
                    // Optional starting position: SOULS_START=x,z[,yaw in degrees].
                    let start = std::env::var("SOULS_START").ok().map(|s| {
                        s.split(',').filter_map(|p| p.trim().parse().ok()).collect::<Vec<f32>>()
                    });
                    if let (Some(v), Some(p)) = (start, w.resource_mut::<crate::sim::ResetFight>().progress.as_mut())
                        && v.len() >= 2
                    {
                        p.pos = Some([v[0], v[1], v.get(2).copied().unwrap_or(0.0).to_radians()]);
                    }
                    // Start in a boss's arena, in front of it: SOULS_BOSS=n (index in `Tuning::encounters`).
                    if let (Some(n), Some(p)) = (
                        std::env::var("SOULS_BOSS").ok().and_then(|h| h.parse().ok()),
                        w.resource_mut::<crate::sim::ResetFight>().progress.as_mut(),
                    ) {
                        p.arena = Some(n);
                    }
                    // Defeated bosses: SOULS_DEFEATED=bits (e.g. 127: all of them, the final door is open).
                    if let (Some(n), Some(p)) = (
                        std::env::var("SOULS_DEFEATED").ok().and_then(|h| h.parse().ok()),
                        w.resource_mut::<crate::sim::ResetFight>().progress.as_mut(),
                    ) {
                        p.defeated = n;
                    }
                    // Optional starting HP: SOULS_HP=1 (check death and respawn).
                    if let (Some(hp), Some(p)) = (
                        std::env::var("SOULS_HP").ok().and_then(|h| h.parse().ok()),
                        w.resource_mut::<crate::sim::ResetFight>().progress.as_mut(),
                    ) {
                        p.hp = Some(hp);
                    }
                });
            })
            .after(crate::menu::enter_title));
        }
        // `SOULS_REST=1`: presses "interact" one second after the start (rest at the checkpoint).
        if std::env::var("SOULS_REST").is_ok() {
            app.add_systems(
                FixedUpdate,
                (|tick: Res<SimTick>, mut input: ResMut<crate::input::LocalInput>| {
                    if tick.0 == 60 {
                        input.0.buttons |= crate::sim::input::btn::INTERACT;
                    }
                })
                .after(crate::input::collect_local_input)
                .before(crate::input::feed_solo)
                .before(crate::net::coop_tick)
                .run_if(in_state(AppState::Playing)),
            );
        }
        // `SOULS_ATTACK=name`: the boss that knows it chains this attack as soon as it's free.
        if let Ok(name) = std::env::var("SOULS_ATTACK") {
            app.add_systems(
                FixedUpdate,
                (move |tuning: Res<Tuning>,
                       encounter: Res<crate::sim::encounter::Encounter>,
                       mut bosses: Query<(&Boss, &Body, &mut Action)>,
                       players: Query<&Body, (With<Player>, Without<Boss>)>| {
                    let Some(target) = players.iter().next().map(|b| b.pos) else { return };
                    for (boss, body, mut action) in &mut bosses {
                        let idx = boss.def(&tuning).attacks.iter().position(|a| a.name == name);
                        if let (Some(i), None, true) = (idx, action.mv, encounter.active) {
                            let dist = crate::sim::math::flat_len(target - body.pos);
                            action.start(crate::sim::data::MoveRef::BossAttack(boss.def, i as u16), dist);
                        }
                    }
                })
                .before(crate::sim::run_sim_tick)
                .run_if(in_state(AppState::Playing)),
            );
        }
        // `SOULS_WANDER=1`: the local player moves, attacks, dodges and guards at random (co-op
        // tests: every peer must keep simulating the same game).
        if std::env::var("SOULS_WANDER").is_ok() {
            app.add_systems(
                FixedUpdate,
                wander
                    .after(crate::input::collect_local_input)
                    .before(crate::input::feed_solo)
                    .before(crate::net::coop_tick)
                    .run_if(in_state(AppState::Playing)),
            );
        }
        if std::env::var("SOULS_AUTOPILOT").is_ok() {
            app.add_systems(
                FixedUpdate,
                autopilot
                    .after(crate::input::collect_local_input)
                    .before(crate::input::feed_solo)
                    .before(crate::net::coop_tick)
                    .run_if(in_state(AppState::Playing)),
            );
        }
    }
}

/// Start of the frame (to measure the main world's CPU time).
#[derive(Resource, Default)]
struct PerfClock {
    start: Option<std::time::Instant>,
    /// Elapsed time, worst frame, cumulative CPU time of the main world, frames.
    acc: (f32, f32, f32, u32),
    /// Frames over 20 ms (stutters).
    slow: u32,
}

fn perf_log(time: Res<Time<Real>>, mut clock: ResMut<PerfClock>, entities: Query<()>) {
    let dt = time.delta_secs();
    let cpu = clock.start.map_or(0.0, |s| s.elapsed().as_secs_f32());
    let (total, worst, main, frames) = clock.acc;
    clock.acc = (total + dt, worst.max(dt), main + cpu, frames + 1);
    clock.slow += (dt > 0.020) as u32;
    let (total, worst, main, frames) = clock.acc;
    if total >= 1.0 {
        info!(
            "perf: {:.2} ms/frame on average ({:.0} fps), worst {:.2} ms ({} > 20 ms), main world {:.2} ms, {} entities",
            total / frames as f32 * 1000.0,
            frames as f32 / total,
            worst * 1000.0,
            clock.slow,
            main / frames as f32 * 1000.0,
            entities.iter().count()
        );
        clock.acc = (0.0, 0.0, 0.0, 0);
        clock.slow = 0;
    }
}

fn autoshot(
    mut menu: ResMut<crate::menu::MenuState>,
    mut commands: Commands,
    time: Res<Time>,
    mut shots: ResMut<AutoShots>,
    mut start: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let t0 = *start.get_or_insert(time.elapsed_secs());
    let t = time.elapsed_secs() - t0;
    if let Ok(page) = std::env::var("SOULS_OPEN_MENU")
        && t > 1.0
        && !menu.open
    {
        use crate::menu::Page;
        menu.open(match page.as_str() {
            "help" => Page::Help,
            "equip" => Page::Equipment,
            "status" => Page::Status,
            "pause" => Page::Pause,
            "choose" => Page::Choose,
            "options" => Page::Options,
            "system" => Page::System,
            "checkpoint" => Page::Checkpoint,
            "travel" => Page::Travel,
            "revive" => Page::Revive,
            "language" => Page::Language,
            "style" => Page::Style,
            _ => Page::Pause,
        });
    }
    if let Some(&at) = shots.times.get(shots.done) {
        if t >= at {
            let path = format!("{}/shot_{}.png", shots.dir, shots.done);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            shots.done += 1;
        }
    } else if t >= shots.times.last().copied().unwrap_or(0.0) + 1.0 {
        exit.write(AppExit::Success);
    }
}

fn wander(mut input: ResMut<crate::input::LocalInput>, mut t: Local<u32>, local: Res<crate::net::LocalId>, net: Res<crate::net::Net>) {
    use crate::sim::input::{PlayerInput, btn};
    // Looking for a partner: standing still, arms raised.
    if net.searching() {
        return;
    }
    *t += 1;
    let i = *t;
    let x = (i / 20).wrapping_mul(2654435761).wrapping_add(local.0 as u32 * 0x9E37_79B9) ^ 0x5bd1_e995;
    let buttons = match (x >> 16) % 8 {
        0 | 1 => btn::LIGHT,
        2 => btn::DODGE,
        3 => btn::GUARD,
        4 => btn::LOCK,
        _ => 0,
    };
    input.0 = PlayerInput {
        buttons: if i % 20 < 8 { buttons } else { 0 },
        move_x: ((x & 0xff) as i32 - 128).clamp(-127, 127) as i8,
        move_y: (((x >> 8) & 0xff) as i32 - 128).clamp(-60, 127) as i8,
        cam_yaw: input.0.cam_yaw,
        cmd: [0; 4],
    };
}

fn autopilot(
    tick: Res<SimTick>,
    mut input: ResMut<crate::input::LocalInput>,
    rig: Res<crate::render::camera::CameraRig>,
    players: Query<(&Player, &Body), With<LocalPlayer>>,
    bosses: Query<&Body, With<Boss>>,
) {
    use crate::sim::input::{PlayerInput, btn};
    let (Ok((p, pb)), Some(bb)) = (players.single(), bosses.iter().next()) else { return };
    let t = tick.0;
    let mut i = PlayerInput { cam_yaw: PlayerInput::quantize_yaw(rig.yaw), ..default() };
    if t == 30 {
        i.buttons |= btn::LOCK;
    }
    if t == 50 {
        i.buttons |= btn::SWITCH;
    }
    if t % 480 == 400 {
        i.buttons |= btn::ITEM;
    }
    let dist = pb.pos.distance(bb.pos);
    if t > 40 && dist > 3.2 {
        i.move_y = 127;
    }
    if dist <= 3.4 && t % 90 < 3 {
        i.buttons |= btn::LIGHT;
    }
    if dist <= 3.4 && (t / 200) % 2 == 1 {
        i.buttons |= btn::GUARD;
    }
    let _ = p;
    input.0 = i;
}
