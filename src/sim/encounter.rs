//! Game flow: checkpoints (rest, discovery, travel), the bosses' fogs (into their arena and back),
//! the torches (reviving a defeated boss), victory, the final door, death and respawn at
//! the last checkpoint, picked-up items and loot, commands coming from the menus.
//!
//! This is simulation: everything that changes the state goes through here, tick-accurate.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::boss::Boss;
use super::data::{BossMove, MoveRef, Tuning};
use super::fighter::{Action, Body, Foe, Health, Hitstop, PrevBody};
use super::input::{MAX_PLAYERS, PlayerInputs};
use super::items::{Inventory, Item, Loot};
use super::player::{PState, Player};
use super::world::{self, Zone};
use super::{ResetFight, SimEvent, SimEvents, SimIds, math, sim_order};

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Encounter {
    /// Defeated bosses (bit i: `Tuning::encounters[i]`, fought in `Tuning::arenas[i]`).
    pub defeated: u32,
    /// Arena whose bosses are there: a player went through its fog.
    pub arena: Option<u8>,
    /// Fight in progress: the bosses are awake and the fog blocks the way out.
    pub active: bool,
    /// A path enemy is chasing a player: resting is impossible.
    pub hunted: bool,
    /// Enemies must return to their post (rest at the checkpoint).
    pub respawn_enemies: bool,
}

impl Encounter {
    pub fn is_defeated(&self, i: u8) -> bool {
        1u32.checked_shl(i.into()).is_some_and(|b| self.defeated & b != 0)
    }
}

/// All the bosses are defeated: the final door is open.
pub fn door_open(t: &Tuning, defeated: u32) -> bool {
    let all = if t.encounters.len() >= 32 { u32::MAX } else { (1u32 << t.encounters.len()) - 1 };
    defeated & all == all
}

/// Delay between the end of the death animation and the respawn.
pub const RESPAWN_TICKS: u32 = 150;
/// Distance to the checkpoint to be able to rest there.
pub const REST_RANGE: f32 = 2.2;
/// Distance to pick up an item.
pub const PICKUP_RANGE: f32 = 1.4;
/// Collision radius of the checkpoint.
pub const CHECKPOINT_RADIUS: f32 = 0.6;
/// Distance to go through a fog, to rekindle a torch.
pub const FOG_RANGE: f32 = 1.8;
pub const TORCH_RANGE: f32 = 1.6;
/// A player's persistent progress: this is what the save contains.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Defeated bosses (bit i: `Tuning::encounters[i]`).
    pub defeated: u32,
    pub weapon: u8,
    pub inventory: Inventory,
    /// HP (`None`: full).
    pub hp: Option<f32>,
    /// Position (x, z) and orientation (`None`: in front of the last checkpoint).
    pub pos: Option<[f32; 3]>,
    /// Last checkpoint rested at: respawn there.
    pub checkpoint: u8,
    /// Discovered checkpoints (bit i: `level.checkpoints[i]`).
    pub found: u32,
    /// Picked-up items (bit i: `level.pickups[i]`).
    pub picked: u64,
    /// Defeated unique enemies (bit i: `level.enemies[i]`).
    pub slain: u64,
    /// Start in this arena, in front of its bosses (debug: `SOULS_BOSS`). Never saved (a
    /// player's progress never has it), but sent to the co-op guests with the host's world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arena: Option<u8>,
}

impl Progress {
    pub fn new_game(t: &Tuning) -> Self {
        Self { inventory: Inventory::new_game(t), found: 1, ..default() }
    }

    /// State to resume for this player. Dead (or mid-fall): as after the
    /// respawn (last checkpoint, items refilled). In an arena: in front of its fog, the boss
    /// will be reset.
    pub fn of_player(p: &Player, body: &Body, hp: &Health, enc: &Encounter, t: &Tuning) -> Self {
        let dead = matches!(p.state, PState::Dead | PState::Falling) || hp.dead();
        let mut inventory = p.inventory.clone();
        if dead {
            inventory.refill(t);
        }
        let outside = match p.zone {
            Zone::Arena(i) => Some(gate_outside(t, i as usize)),
            Zone::Level => None,
        };
        let pos = if dead {
            None
        } else if let Some((g, yaw)) = outside {
            Some([g.x, g.z, yaw])
        } else {
            Some([body.pos.x, body.pos.z, body.yaw])
        };
        Self {
            // The first player's world is the one in play (the host's, in co-op); a guest
            // only counts the bosses defeated with them.
            defeated: if p.id == 0 { enc.defeated } else { p.felled },
            weapon: p.weapon,
            inventory,
            hp: (!dead).then_some(hp.cur),
            pos,
            checkpoint: p.checkpoint,
            found: p.found,
            picked: p.picked,
            slain: p.slain,
            ..default()
        }
    }
}

/// Brazier of checkpoint `i`, on the ground.
pub fn checkpoint_pos(t: &Tuning, i: usize) -> Vec3 {
    let c = &t.level.checkpoints[i.min(t.level.checkpoints.len() - 1)];
    let y = world::floor_at(t, c.pos[0], c.pos[1], 0.0).unwrap_or(0.0);
    Vec3::new(c.pos[0], y, c.pos[1])
}

/// Respawn point at checkpoint `i` (position, orientation): next to the brazier (the fire on
/// its left), facing the way forward (`look`); the camera, behind, sees the fire from the side.
pub fn checkpoint_spawn(t: &Tuning, i: usize) -> (Vec3, f32) {
    let c = &t.level.checkpoints[i.min(t.level.checkpoints.len() - 1)];
    let fire = checkpoint_pos(t, i);
    let yaw = math::yaw_of(Vec3::new(c.look[0], 0.0, c.look[1]) - fire);
    // The fire on the left if there's room, otherwise on the right: never near an edge.
    let at = |side: f32| fire + math::forward(yaw) * 0.5 + math::right(yaw) * 1.4 * side;
    let p = [1.0, -1.0].into_iter().map(at).find(|p| clear(t, *p, SPAWN_CLEARANCE)).unwrap_or_else(|| at(1.0));
    let y = world::floor_at(t, p.x, p.z, p.y).unwrap_or(p.y);
    (Vec3::new(p.x, y, p.z), yaw)
}

/// Floor all around a respawn point, at least this far (m).
pub const SPAWN_CLEARANCE: f32 = 1.5;

/// Floor at about the same height everywhere within `r` of `p`.
pub fn clear(t: &Tuning, p: Vec3, r: f32) -> bool {
    let Some(y) = world::floor_at(t, p.x, p.z, p.y) else { return false };
    (0..16).all(|k| {
        let d = math::forward(std::f32::consts::TAU * k as f32 / 16.0) * r;
        world::floor_at(t, p.x + d.x, p.z + d.z, y).is_some_and(|fy| (fy - y).abs() < world::STEP_UP)
    })
}

/// Checkpoint within resting range.
pub fn near_checkpoint(t: &Tuning, pos: Vec3) -> Option<u8> {
    (0..t.level.checkpoints.len()).find_map(|i| {
        let c = checkpoint_pos(t, i);
        (math::flat_len(pos - c) <= REST_RANGE && (pos.y - c.y).abs() < 1.5).then_some(i as u8)
    })
}

/// Ground position of an item to pick up.
pub fn pickup_pos(t: &Tuning, i: usize) -> Vec3 {
    let p = &t.level.pickups[i];
    Vec3::new(p.pos[0], world::floor_at(t, p.pos[0], p.pos[1], 0.0).unwrap_or(0.0), p.pos[1])
}

/// Loot of player `owner` on the ground within range (the closest).
pub fn near_loot<'a>(loot: impl Iterator<Item = (Entity, &'a Loot)>, owner: u8, pos: Vec3) -> Option<(Entity, Loot)> {
    loot.filter(|(_, l)| l.owner == owner && within(pos, l.pos, PICKUP_RANGE))
        .min_by(|a, b| math::flat_len(pos - a.1.pos).total_cmp(&math::flat_len(pos - b.1.pos)))
        .map(|(e, l)| (e, *l))
}

/// Item not yet picked up (bits of `picked`) within range.
pub fn near_pickup(t: &Tuning, picked: u64, pos: Vec3) -> Option<u16> {
    (0..t.level.pickups.len())
        .filter(|i| picked & (1u64 << i) == 0)
        .map(|i| (i, pickup_pos(t, i)))
        .filter(|(_, p)| math::flat_len(pos - *p) <= PICKUP_RANGE && (pos.y - p.y).abs() < 1.5)
        .min_by(|a, b| math::flat_len(pos - a.1).total_cmp(&math::flat_len(pos - b.1)))
        .map(|(i, _)| i as u16)
}

fn within(pos: Vec3, p: Vec3, range: f32) -> bool {
    math::flat_len(pos - p) <= range && (pos.y - p.y).abs() < 1.5
}

/// Fog at the end of boss `i`'s corridor, in the level (ground point, direction into it).
pub fn gate(t: &Tuning, i: usize) -> (Vec3, Vec3) {
    world::portal(t, &t.arenas[i].gate)
}

/// Where you come back out of arena `i`: in front of its fog in the level, your back to it.
pub fn gate_outside(t: &Tuning, i: usize) -> (Vec3, f32) {
    let (g, dir) = gate(t, i);
    let p = g - dir * 1.4;
    let y = world::floor_at(t, p.x, p.z, g.y).unwrap_or(g.y);
    (Vec3::new(p.x, y, p.z), math::yaw_of(-dir))
}

/// Where you enter arena `i`: past its fog, facing the bosses.
pub fn door_entry(t: &Tuning, i: usize) -> (Vec3, f32) {
    let (d, dir) = world::portal(t, &t.arenas[i].door);
    let p = d + dir * 2.4;
    let y = world::floor_at(t, p.x, p.z, d.y).unwrap_or(d.y);
    (Vec3::new(p.x, y, p.z), math::yaw_of(dir))
}

/// Boss fog within range, in the level.
pub fn near_gate(t: &Tuning, pos: Vec3) -> Option<u8> {
    (0..t.arenas.len()).find(|&i| within(pos, gate(t, i).0, FOG_RANGE)).map(|i| i as u8)
}

/// In arena `i`, close to the fog you came in through.
pub fn near_door(t: &Tuning, i: usize, pos: Vec3) -> bool {
    within(pos, world::portal(t, &t.arenas[i].door).0, FOG_RANGE)
}

/// Torch of boss `i`, on the ground.
pub fn torch_pos(t: &Tuning, i: usize) -> Vec3 {
    let [x, z] = t.arenas[i].torch;
    Vec3::new(x, world::floor_at(t, x, z, 0.0).unwrap_or(0.0), z)
}

pub fn near_torch(t: &Tuning, pos: Vec3) -> Option<u8> {
    (0..t.arenas.len()).find(|&i| within(pos, torch_pos(t, i), TORCH_RANGE)).map(|i| i as u8)
}


/// Gets a fallen player back up (co-op), rested: full health, flasks refilled, at `pos`.
#[allow(clippy::too_many_arguments)]
fn revive(t: &Tuning, p: &mut Player, body: &mut Body, prev: &mut PrevBody, action: &mut Action, hp: &mut Health, stop: &mut Hitstop, (pos, yaw): (Vec3, f32), zone: Zone) {
    let mut inventory = p.inventory.clone();
    inventory.refill(t);
    *p = Player {
        weapon: p.weapon,
        inventory,
        checkpoint: p.checkpoint,
        found: p.found,
        picked: p.picked,
        slain: p.slain,
        felled: p.felled,
        cmd_seq: p.cmd_seq,
        zone,
        ..Player::new(p.id, t)
    };
    body.pos = pos;
    body.yaw = yaw;
    *prev = PrevBody { pos, yaw };
    action.stop();
    hp.cur = hp.max;
    stop.0 = 0;
}

type PlayerParts = (Entity, &'static mut Player, &'static mut Body, &'static mut PrevBody, &'static mut Health, &'static mut Action, &'static mut Hitstop);

/// Bosses waiting in the arena a player went into; the fight starts at once (the fog closes
/// behind them); victory; the arena empties once everyone has left it; respawn.
///
/// Co-op (as in the souls' co-op mods): a fallen player gets back up alone at their checkpoint
/// while the others fight on — except in a boss fight, where they wait for its outcome (revived
/// in the arena on victory). Everyone down: back to the checkpoint, the world starts over.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn encounter_tick(
    mut commands: Commands,
    tuning: Res<Tuning>,
    mut enc: ResMut<Encounter>,
    mut reset: ResMut<ResetFight>,
    mut events: ResMut<SimEvents>,
    ids: SimIds,
    mut players: Query<PlayerParts, Without<Boss>>,
    mut bosses: Query<(Entity, &Boss, &mut Action, &mut Health), Without<Player>>,
) {
    let t = &*tuning;
    let order = sim_order(&ids);
    for (_, mut p, ..) in &mut players {
        p.dead_ticks = if p.state == PState::Dead { p.dead_ticks + 1 } else { 0 };
    }

    // Everyone is dead: back to the checkpoint, the boss starts over.
    let everyone_down = players.iter().all(|(_, p, ..)| p.state == PState::Dead);
    let all_dead = players.iter().all(|(_, p, ..)| p.state == PState::Dead && p.dead_ticks >= RESPAWN_TICKS);
    if all_dead
        && !reset.requested
        && let Some((_, p, b, _, h, ..)) = players.iter().min_by_key(|(_, p, ..)| p.id)
    {
        reset.requested = true;
        reset.progress = Some(Progress::of_player(p, b, h, &enc, t));
    }
    // Co-op: fallen alone, outside a boss fight: back at their checkpoint.
    if !everyone_down {
        let mut it = players.iter_many_mut(&order);
        while let Some((e, mut p, mut b, mut prev, mut h, mut a, mut stop)) = it.fetch_next() {
            let fighting = matches!(p.zone, Zone::Arena(i) if enc.active && enc.arena == Some(i));
            if p.state == PState::Dead && p.dead_ticks >= RESPAWN_TICKS && !fighting {
                let at = checkpoint_spawn(t, p.checkpoint as usize);
                revive(t, &mut p, &mut b, &mut prev, &mut a, &mut h, &mut stop, at, Zone::Level);
                events.push(SimEvent::Revived { entity: e });
            }
        }
    }
    let alive_in = |p: &Player, h: &Health| match p.zone {
        Zone::Arena(i) if !h.dead() => Some(i),
        _ => None,
    };

    // A player went through a fog: its bosses are there (created at the next tick).
    if let Some(i) = players.iter_many(&order).find_map(|(_, p, _, _, h, ..)| alive_in(p, h))
        && enc.arena != Some(i)
        && !enc.is_defeated(i)
    {
        for (e, ..) in &bosses {
            commands.entity(e).despawn();
        }
        super::spawn_boss(&mut commands, t, i, players.iter().count());
        enc.arena = Some(i);
        enc.active = false;
        return;
    }
    let Some(arena) = enc.arena else { return };
    // Supporting roles (the butcher's dogs) don't count for victory.
    let boss_alive = bosses.iter().any(|(_, b, _, h)| !h.dead() && !b.def(t).minor);

    // The bosses wake up roaring, the fog closes again.
    if !enc.active && boss_alive && players.iter().any(|(_, p, _, _, h, ..)| alive_in(p, h) == Some(arena)) {
        enc.active = true;
        for (_, b, mut a, h) in &mut bosses {
            if !h.dead() && a.mv.is_none() {
                a.start(MoveRef::Boss(b.def, BossMove::Roar), 0.0);
            }
        }
        events.push(SimEvent::BossAwake);
    }

    if enc.active && !boss_alive {
        enc.active = false;
        enc.defeated |= 1u32 << arena;
        // With the master fallen, his dogs collapse with him.
        for (_, b, mut a, mut h) in &mut bosses {
            if !h.dead() {
                h.cur = 0.0;
                a.start(MoveRef::Boss(b.def, BossMove::Death), 0.0);
            }
        }
        events.push(SimEvent::BossDefeated);
        // The survivors catch their breath: full health and flasks refilled. The fallen
        // (co-op) get back up at the arena's door. The victory counts for everyone who was
        // in the arena, fallen or not (the others weren't part of it).
        let mut it = players.iter_many_mut(&order);
        while let Some((e, mut p, mut b, mut prev, mut h, mut a, mut stop)) = it.fetch_next() {
            if p.zone == Zone::Arena(arena) {
                p.felled |= 1u32 << arena;
            }
            if !h.dead() {
                h.cur = h.max;
                p.inventory.refill(t);
                events.push(SimEvent::Heal { entity: e });
            } else if p.zone == Zone::Arena(arena) && !p.falling {
                revive(t, &mut p, &mut b, &mut prev, &mut a, &mut h, &mut stop, door_entry(t, arena as usize), Zone::Arena(arena));
                events.push(SimEvent::Revived { entity: e });
            }
        }
        if door_open(t, enc.defeated) {
            events.push(SimEvent::DoorOpened);
        }
    }

    // Everyone has left the arena (after the victory): it empties.
    if !enc.active && !players.iter().any(|(_, p, ..)| p.zone == Zone::Arena(arena)) {
        for (e, ..) in &bosses {
            commands.entity(e).despawn();
        }
        enc.arena = None;
    }
}

/// Actions decided in the menus. Over the network, they travel with the inputs
/// (`PlayerInput::cmd`, see `encode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimCommand {
    Equip { player: u8, slot: u8, item: Option<Item> },
    EquipTalisman { player: u8, item: Option<Item> },
    /// Brings back a defeated boss (rekindling its torch): it awaits again in its arena.
    ReviveBoss(u8),
    /// Travel to a discovered checkpoint: you respawn there rested, the world is reset (in
    /// co-op, the whole party travels).
    Travel { player: u8, checkpoint: u8 },
    /// Co-op: the player left the game (or lost the connection), their character vanishes.
    Leave { player: u8 },
}

const CMD_EQUIP: u8 = 1;
const CMD_TALISMAN: u8 = 2;
const CMD_REVIVE: u8 = 3;
const CMD_TRAVEL: u8 = 4;
const CMD_LEAVE: u8 = 5;
const NO_ITEM: u8 = u8::MAX;

fn item_code(item: Option<Item>) -> u8 {
    item.and_then(|it| Item::ALL.iter().position(|x| *x == it)).map_or(NO_ITEM, |i| i as u8)
}

fn item_of(code: u8) -> Option<Item> {
    Item::ALL.get(code as usize).copied()
}

impl SimCommand {
    /// Encoding in a `PlayerInput` (`cmd`): number `seq` (never 0), kind, arguments. The player
    /// is the one whose input carries it.
    pub fn encode(self, seq: u8) -> [u8; 4] {
        let (kind, a, b) = match self {
            SimCommand::Equip { slot, item, .. } => (CMD_EQUIP, slot, item_code(item)),
            SimCommand::EquipTalisman { item, .. } => (CMD_TALISMAN, item_code(item), 0),
            SimCommand::ReviveBoss(i) => (CMD_REVIVE, i, 0),
            SimCommand::Travel { checkpoint, .. } => (CMD_TRAVEL, checkpoint, 0),
            SimCommand::Leave { .. } => (CMD_LEAVE, 0, 0),
        };
        [seq.max(1), kind, a, b]
    }

    pub fn decode(player: u8, raw: [u8; 4]) -> Option<Self> {
        let [_, kind, a, b] = raw;
        Some(match kind {
            CMD_EQUIP => SimCommand::Equip { player, slot: a, item: item_of(b) },
            CMD_TALISMAN => SimCommand::EquipTalisman { player, item: item_of(a) },
            CMD_REVIVE => SimCommand::ReviveBoss(a),
            CMD_TRAVEL => SimCommand::Travel { player, checkpoint: a },
            CMD_LEAVE => SimCommand::Leave { player },
            _ => return None,
        })
    }
}

#[derive(Resource, Default, Debug)]
pub struct SimCommands(pub Vec<SimCommand>);

/// The menu decisions carried by the inputs (co-op): each one is queued once, in player order.
pub fn input_commands(inputs: Res<PlayerInputs>, ids: SimIds, mut players: Query<&mut Player>, mut queue: ResMut<SimCommands>) {
    let order = sim_order(&ids);
    let mut it = players.iter_many_mut(&order);
    while let Some(mut p) = it.fetch_next() {
        let raw = inputs.0[p.id as usize % MAX_PLAYERS].cmd;
        if raw[0] == 0 || raw[0] == p.cmd_seq {
            continue;
        }
        p.cmd_seq = raw[0];
        if let Some(c) = SimCommand::decode(p.id, raw) {
            queue.0.push(c);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_commands(
    mut commands: Commands,
    tuning: Res<Tuning>,
    mut queue: ResMut<SimCommands>,
    mut enc: ResMut<Encounter>,
    mut events: ResMut<SimEvents>,
    mut reset: ResMut<ResetFight>,
    mut players: Query<(Entity, &mut Player, &Body, &Health), Without<Foe>>,
    mut foes: Query<(&mut Health, Option<&Boss>), With<Foe>>,
) {
    for c in std::mem::take(&mut queue.0) {
        match c {
            SimCommand::Equip { player, slot, item } => {
                for (_, mut p, ..) in &mut players {
                    if p.id == player {
                        p.inventory.equip(slot as usize, item);
                    }
                }
            }
            SimCommand::EquipTalisman { player, item } => {
                for (_, mut p, ..) in &mut players {
                    if p.id == player {
                        p.inventory.equip_talisman(item);
                    }
                }
            }
            SimCommand::Travel { player, checkpoint } => {
                let Some((_, p, ..)) = players.iter().find(|(_, p, ..)| p.id == player) else { continue };
                if enc.active
                    || p.zone != Zone::Level
                    || checkpoint as usize >= tuning.level.checkpoints.len()
                    || p.found & (1u32 << checkpoint) == 0
                {
                    continue;
                }
                // The whole party travels, rested: the world (the first player's) and each
                // one's character.
                let mut party: Vec<(u8, Progress)> = players.iter().map(|(_, p, b, h)| (p.id, Progress::of_player(p, b, h, &enc, &tuning))).collect();
                party.sort_by_key(|x| x.0);
                let mut party: Vec<Progress> = party.into_iter().map(|x| x.1).collect();
                for pr in &mut party {
                    pr.checkpoint = checkpoint;
                    pr.pos = None;
                    pr.hp = None;
                    pr.inventory.refill(&tuning);
                }
                reset.requested = true;
                reset.progress = Some(party.remove(0));
                reset.guests = party;
            }
            SimCommand::Leave { player } => {
                let Some(e) = players.iter().find(|(_, p, ..)| p.id == player).map(|x| x.0) else { continue };
                commands.entity(e).despawn();
                // One fewer: the opponents go back to the health of a smaller party.
                let n = players.iter().count();
                for (mut h, boss) in &mut foes {
                    let lead = boss.is_some_and(|b| !b.def(&tuning).minor);
                    let k = super::fighter::party_hp(n - 1, lead) / super::fighter::party_hp(n, lead);
                    h.max *= k;
                    h.cur *= k;
                }
            }
            SimCommand::ReviveBoss(i) => {
                if enc.is_defeated(i) && enc.arena != Some(i) {
                    enc.defeated &= !(1u32 << i);
                    events.push(SimEvent::BossRevived { arena: i });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_spawn_points_and_gate() {
        let t = Tuning::builtin();
        for i in 0..t.level.checkpoints.len() {
            let (p, _) = checkpoint_spawn(&t, i);
            assert_eq!(near_checkpoint(&t, p), Some(i as u8));
            assert!(world::floor_at(&t, p.x, p.z, p.y).is_some());
            assert!(clear(&t, p, SPAWN_CLEARANCE), "checkpoint {i}: spawn {p:?} too close to an edge");
        }
    }

    #[test]
    fn fogs_torches_and_sign_are_on_the_ground_of_their_zone() {
        let t = Tuning::builtin();
        for i in 0..t.arenas.len() {
            let (g, _) = gate(&t, i);
            assert_eq!(world::zone_at(&t, g), Zone::Level, "gate {i}");
            let (out, _) = gate_outside(&t, i);
            assert!(world::floor_below(&t, out.x, out.z, out.y, Some(world::Mover::Player { zone: Zone::Level, door_open: false })).is_some(), "gate {i}");
            assert_eq!(near_gate(&t, out), Some(i as u8));
            let (entry, _) = door_entry(&t, i);
            assert_eq!(world::zone_at(&t, entry), Zone::Arena(i as u8), "arena {i}");
            let (d, dir) = world::portal(&t, &t.arenas[i].door);
            assert!(near_door(&t, i, d + dir * 0.8) && !near_door(&t, i, entry), "arena {i}");
            let torch = torch_pos(&t, i);
            assert!(world::floor_at(&t, torch.x, torch.z, 0.0).is_some(), "torch {i}");
            assert_eq!(near_torch(&t, torch + Vec3::X * 0.8), Some(i as u8));
            let [x, z] = t.arenas[i].boss_spawn;
            assert_eq!(world::zone_at(&t, Vec3::new(x, 0.0, z)), Zone::Arena(i as u8), "boss {i}");
        }
        assert!(!door_open(&t, 0) && door_open(&t, u32::MAX));
    }
}
