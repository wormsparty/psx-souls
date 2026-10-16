//! Deterministic combat simulation at 60 ticks/s.
//!
//! Rules to stay compatible with network rollback (`snapshot`, `crate::net`):
//! - no reading of `Time` or hardware input here: only `PlayerInputs` and `SimTick`;
//! - maths via `math` (libm), randomness via `SimRng`;
//! - all state lives in `Clone` components/resources (saved and restored by `snapshot`);
//! - entities are visited in `SimId` order (`sim_order`), never in Bevy's storage order, which
//!   differs from one machine to the other; an `Entity` is only a reference, never a value
//!   (its bits differ between peers).

pub mod boss;
pub mod combat;
pub mod data;
pub mod encounter;
pub mod enemy;
pub mod fighter;
pub mod input;
pub mod items;
pub mod math;
pub mod player;
pub mod rng;
pub mod snapshot;
pub mod spell;
pub mod world;

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;

use data::Tuning;
use encounter::{Encounter, Progress, SimCommands};
use fighter::{Action, Body, Foe, Health, Hitstop, PrevBody, Team};
use input::PlayerInputs;

pub const TICK_HZ: f64 = 60.0;
pub const DT: f32 = 1.0 / 60.0;

/// Number of the current simulation tick.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SimTick(pub u32);

/// Schedule run once per simulation tick.
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct SimSchedule;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum SimSet {
    Begin,
    Act,
    Physics,
    Combat,
    End,
}

/// Events produced by the sim for the presentation (sounds, VFX, HUD).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimEvent {
    /// `foe`: the opponent hit (`None`: a player).
    Hit { pos: Vec3, heavy: bool, foe: Option<Entity> },
    Guard { pos: Vec3 },
    PerfectGuard { pos: Vec3 },
    GuardBreak { pos: Vec3 },
    Swing { entity: Entity, heavy: bool },
    /// White slash in the air (`HitWindow::slash`): from `pos`, facing `yaw`, up to `top` metres
    /// high and `reach` metres ahead.
    Slash { pos: Vec3, yaw: f32, reach: f32, top: f32 },
    Dodge { entity: Entity },
    FuryWarn { entity: Entity },
    /// Impact of an area attack (`aoe`): ground centre and radius. `boss`: the boss's
    /// definition (its colour).
    Shockwave { pos: Vec3, radius: f32, boss: u8 },
    /// Spell cast (starting point), projectile fading out, eruption bursting out, projectile
    /// crashing on the ground (it burns there for a while). `boss`: the caster's definition (its colour).
    /// `volley`: the attack that cast it (a single sound per attack).
    SpellCast { pos: Vec3, element: data::Element, boss: u8, volley: u32 },
    SpellFizzle { pos: Vec3, element: data::Element, boss: u8 },
    Eruption { pos: Vec3, radius: f32, element: data::Element, boss: u8, volley: u32 },
    SpellSplash { pos: Vec3, radius: f32, element: data::Element, boss: u8 },
    Groggy { entity: Entity },
    Fatal { pos: Vec3 },
    BossPhase2,
    BossDied,
    PlayerDied { entity: Entity },
    /// Co-op: a fallen player gets back up — at their checkpoint, or in the arena once the boss
    /// is defeated (the others fought on).
    Revived { entity: Entity },
    WeaponSwitched { entity: Entity },
    Heal { entity: Entity },
    /// Action refused for lack of stamina (visual feedback on the bar).
    NoStamina { entity: Entity },
    /// Next quick slot selected.
    ItemCycled { entity: Entity },
    /// The player entered the arena: the boss wakes up, the fog closes.
    BossAwake,
    BossDefeated,
    /// The last boss has fallen: the final door opens.
    DoorOpened,
    /// A defeated boss's torch was rekindled: it awaits again in its arena.
    BossRevived { arena: u8 },
    /// A player went through a fog: into an arena (`Some`), or back into the level.
    Passage { entity: Entity, arena: Option<u8> },
    /// A player wants to rekindle the torch of a defeated boss (the menu asks for confirmation).
    TorchTouched { entity: Entity, arena: u8 },
    /// Rest at the checkpoint (HP, items and stamina restored).
    Rested { entity: Entity },
    /// The fighters have just been (re)created: loading, respawn, travel.
    Respawned,
    /// An enemy spots a player and raises the alarm.
    EnemyAlert { entity: Entity },
    /// An enemy is defeated.
    EnemyDied { pos: Vec3 },
    /// Its body disappears, a few moments later (leaving what it dropped, if anything).
    EnemyVanished { pos: Vec3, loot: Option<items::Item> },
    /// Item picked up (`level.pickups[pickup]`).
    PickedUp { entity: Entity, pickup: u16 },
    /// Item dropped by an enemy picked up.
    LootPicked { entity: Entity, item: items::Item },
    /// Consumable used (other than the flask, which gives `Heal`).
    ItemUsed { entity: Entity, item: items::Item },
    /// Checkpoint discovered (first rest).
    Kindled { entity: Entity, checkpoint: u8 },
    /// The player went over the edge.
    Fell { entity: Entity },
    /// Jump, and back on the ground.
    Jumped { entity: Entity },
    Landed { entity: Entity },
}

#[derive(Resource, Default, Debug)]
pub struct SimEvents(pub Vec<SimEvent>);

impl SimEvents {
    pub fn push(&mut self, e: SimEvent) {
        // Avoids unbounded growth if nobody reads (tests, background tab).
        if self.0.len() < 256 {
            self.0.push(e);
        }
    }
}

/// Debug options that affect the sim (identical on all peers).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SimDebug {
    pub boss_passive: bool,
}

/// Request to (re)create the fighters, handled at the start of the next tick.
#[derive(Resource, Clone, Debug)]
pub struct ResetFight {
    pub requested: bool,
    pub players: u8,
    /// Starting progress: the world (bosses, checkpoint, position) and the first player's
    /// character. `None`: the current first player's (or a new game).
    pub progress: Option<Progress>,
    /// Co-op: the characters of the other players (weapon, inventory, discoveries), by id from 1.
    /// Empty: the current players' (missing ones get a copy of the first).
    pub guests: Vec<Progress>,
}

impl Default for ResetFight {
    fn default() -> Self {
        Self { requested: true, players: 1, progress: None, guests: Vec::new() }
    }
}

/// Marker of all simulation entities (for the reset). Each one gets its `SimId` on creation.
#[derive(Component, Clone, Copy, Debug)]
#[require(SimId)]
pub struct SimEntity;

/// Stable identity of a simulation entity, in creation order: the same on every peer, unlike
/// `Entity` (which also counts the presentation's entities). Assigned from `NextSimId` when the
/// entity is created; a snapshot restore (`snapshot`) brings back its own.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[component(on_add = assign_sim_id)]
pub struct SimId(pub u32);

impl SimId {
    const UNASSIGNED: SimId = SimId(u32::MAX);
}

impl Default for SimId {
    fn default() -> Self {
        Self::UNASSIGNED
    }
}

/// Next `SimId` to hand out (part of the simulation state).
#[derive(Resource, Clone, Copy, Debug, Default, Hash)]
pub struct NextSimId(pub u32);

fn assign_sim_id(mut world: DeferredWorld, ctx: HookContext) {
    if world.get::<SimId>(ctx.entity).is_some_and(|id| *id != SimId::UNASSIGNED) {
        return;
    }
    let id = {
        let mut next = world.resource_mut::<NextSimId>();
        next.0 = next.0.wrapping_add(1);
        next.0 - 1
    };
    if let Some(mut s) = world.get_mut::<SimId>(ctx.entity) {
        s.0 = id;
    }
}

/// `SimId` of every simulation entity, to visit queries in a deterministic order.
pub type SimIds<'w, 's> = Query<'w, 's, (Entity, &'static SimId)>;

/// The simulation entities in `SimId` order. Queries are visited with `iter_many(&order)` (or
/// `iter_many_mut`), which keeps this order and skips the entities they don't match.
pub fn sim_order(ids: &SimIds) -> Vec<Entity> {
    let mut v: Vec<(SimId, Entity)> = ids.iter().map(|(e, id)| (*id, e)).collect();
    v.sort_unstable_by_key(|x| x.0);
    v.into_iter().map(|(_, e)| e).collect()
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<Tuning>() {
            app.insert_resource(Tuning::builtin());
        }
        app.init_resource::<SimTick>()
            .init_resource::<SimEvents>()
            .init_resource::<PlayerInputs>()
            .init_resource::<rng::SimRng>()
            .init_resource::<SimDebug>()
            .init_resource::<ResetFight>()
            .init_resource::<NextSimId>()
            .init_resource::<Encounter>()
            .init_resource::<SimCommands>();

        let mut schedule = Schedule::new(SimSchedule);
        schedule.configure_sets(
            (SimSet::Begin, SimSet::Act, SimSet::Physics, SimSet::Combat, SimSet::End).chain(),
        );
        schedule.add_systems((
            (reset_fight, encounter::input_commands, encounter::apply_commands, begin_tick).chain().in_set(SimSet::Begin),
            (player::player_act, boss::boss_act, enemy::enemy_act).chain().in_set(SimSet::Act),
            combat::separate_bodies.in_set(SimSet::Physics),
            (combat::resolve_hits, spell::spell_tick).chain().in_set(SimSet::Combat),
            (
                player::player_end_tick,
                boss::boss_end_tick,
                encounter::encounter_tick,
                enemy::respawn_on_request,
                advance_actions,
                end_tick,
            )
                .chain()
                .in_set(SimSet::End),
        ));
        app.add_schedule(schedule);
    }
}

/// Runs one simulation tick. Call from `FixedUpdate` (solo) or from the rollback session (`net`).
pub fn run_sim_tick(world: &mut World) {
    world.run_schedule(SimSchedule);
}

#[allow(clippy::too_many_arguments)]
fn reset_fight(
    mut commands: Commands,
    mut reset: ResMut<ResetFight>,
    tuning: Res<Tuning>,
    existing: Query<Entity, With<SimEntity>>,
    players: Query<(&player::Player, &Body, &Health)>,
    mut enc: ResMut<Encounter>,
    mut events: ResMut<SimEvents>,
    mut rng: ResMut<rng::SimRng>,
    mut next_id: ResMut<NextSimId>,
) {
    if !reset.requested {
        return;
    }
    reset.requested = false;
    // The current characters, by id (the despawned entities' order doesn't matter: sorted).
    let mut current: Vec<(u8, Progress)> = players.iter().map(|(p, b, h)| (p.id, Progress::of_player(p, b, h, &enc, &tuning))).collect();
    current.sort_by_key(|c| c.0);
    let progress = reset
        .progress
        .take()
        .unwrap_or_else(|| current.first().map(|c| c.1.clone()).unwrap_or_else(|| Progress::new_game(&tuning)));
    let guests = match std::mem::take(&mut reset.guests) {
        g if g.is_empty() => current.into_iter().skip(1).map(|c| c.1).collect(),
        g => g,
    };
    for e in &existing {
        commands.entity(e).despawn();
    }
    events.0.clear();
    *rng = rng::SimRng::default();
    *next_id = NextSimId::default();
    *enc = Encounter { defeated: progress.defeated, ..default() };
    spawn_fight(&mut commands, &tuning, reset.players, &progress, &guests);
    events.push(SimEvent::Respawned);
}

/// Creates the players, the boss (if it hasn't been defeated) and the path enemies. Entities
/// are created in a fixed order (determinism). The world comes from `progress` (the first
/// player's, the host in co-op); each player brings their own character (`guests` from the
/// second, by default a copy of the first).
pub fn spawn_fight(commands: &mut Commands, t: &Tuning, players: u8, progress: &Progress, guests: &[Progress]) {
    let checkpoint = (progress.checkpoint as usize).min(t.level.checkpoints.len() - 1);
    let door_open = encounter::door_open(t, progress.defeated);
    let arena = progress.arena.filter(|&i| (i as usize) < t.arenas.len());
    let zone = arena.map_or(world::Zone::Level, world::Zone::Arena);
    for id in 0..players.max(1) {
        let (spawn, spawn_yaw) = encounter::checkpoint_spawn(t, checkpoint);
        // A saved position off the ground (modified level): at the checkpoint.
        // At the foot of a brazier (we quit while resting there): at the usual spot, facing
        // the way forward rather than the fire.
        let saved = progress.pos.and_then(|[x, z, yaw]| {
            let y = world::floor_at(t, x, z, 0.0)?;
            let pos = Vec3::new(x, y, z);
            match encounter::near_checkpoint(t, pos) {
                Some(cp) => Some(encounter::checkpoint_spawn(t, cp as usize)),
                None => Some((pos, yaw)),
            }
        });
        let (pos, yaw) = match arena {
            // In front of the door (or at the saved position, if it's in that arena: debug).
            Some(i) => saved.filter(|(p, _)| world::zone_at(t, *p) == zone).unwrap_or_else(|| encounter::door_entry(t, i as usize)),
            None => saved.filter(|(p, _)| world::zone_at(t, *p) == zone).unwrap_or((spawn, spawn_yaw)),
        };
        let pos = match world::step(t, pos + math::right(yaw) * id as f32, t.player.radius, world::Mover::Player { zone, door_open }) {
            world::Step::Ground(p) => p,
            _ => pos,
        };
        let me = id.checked_sub(1).and_then(|g| guests.get(g as usize)).unwrap_or(progress);
        let mut p = player::Player::new(id, t);
        p.weapon = me.weapon.min(t.weapons.len().saturating_sub(1) as u8);
        p.inventory = me.inventory.clone();
        // An old save's selected item may no longer exist.
        if p.inventory.active_item().is_none() {
            p.inventory.cycle();
        }
        p.checkpoint = checkpoint as u8;
        p.found = me.found | (1 << checkpoint);
        p.picked = me.picked;
        p.slain = me.slain | progress.slain;
        p.zone = zone;
        let mut hp = Health::new(t.player.max_hp);
        if let Some(cur) = me.hp {
            hp.cur = cur.clamp(1.0, hp.max);
        }
        commands.spawn((
            SimEntity,
            Team::Players,
            Body { pos, yaw, radius: t.player.radius, height: t.player.height, mass: 1.0 },
            PrevBody { pos, yaw },
            hp,
            Hitstop::default(),
            Action::default(),
            p,
        ));
    }
    enemy::spawn_all(commands, t, progress.slain, players.max(1) as usize);
}

/// Bosses of encounter `i` (`Tuning::encounters`), in their arena (`Tuning::arenas`), asleep at
/// their spawn point, facing the fog you come in through; sturdier for a party of `players`.
pub fn spawn_boss(commands: &mut Commands, t: &Tuning, i: u8, players: usize) {
    let i = (i as usize).min(t.encounters.len() - 1);
    let arena = &t.arenas[i];
    let [bx, bz] = arena.boss_spawn;
    let (door, _) = world::portal(t, &arena.door);
    for m in &t.encounters[i].members {
        let Some(def) = t.boss_kind(&m.boss) else { continue };
        let bd = &t.bosses[def as usize];
        let (x, z) = (bx + m.offset[0], bz + m.offset[1]);
        let pos = Vec3::new(x, world::floor_at(t, x, z, 0.0).unwrap_or(0.0), z);
        let yaw = math::yaw_of(door - pos);
        commands.spawn((
            SimEntity,
            Team::Enemies,
            Foe,
            Body { pos, yaw, radius: bd.radius, height: bd.height, mass: bd.mass },
            PrevBody { pos, yaw },
            Health::new(bd.max_hp * fighter::party_hp(players, !bd.minor)),
            Hitstop::default(),
            Action::default(),
            boss::Boss::new(t, def, i as u8),
        ));
    }
}

fn begin_tick(mut q: Query<(&Body, &mut PrevBody, &mut Action)>) {
    for (b, mut p, mut a) in &mut q {
        p.pos = b.pos;
        p.yaw = b.yaw;
        a.executed = false;
    }
}

/// Advances the counter of actions that ran a frame this tick.
fn advance_actions(mut q: Query<&mut Action>) {
    for mut a in &mut q {
        if a.executed && a.mv.is_some() {
            a.tick += 1;
        }
    }
}

fn end_tick(mut tick: ResMut<SimTick>) {
    tick.0 = tick.0.wrapping_add(1);
}

/// Hash of the simulation state: used by the determinism tests and, over the network, to
/// detect desyncs. Visits the entities in `SimId` order and never hashes an `Entity` (both
/// differ from one machine to the other); references are hashed as the `SimId` they point to.
pub fn state_hash(world: &mut World) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = StableHasher::default();
    world.resource::<SimTick>().0.hash(&mut h);
    world.resource::<rng::SimRng>().hash(&mut h);
    world.resource::<Encounter>().hash(&mut h);
    world.resource::<NextSimId>().hash(&mut h);
    let mut ids: Vec<(SimId, Entity)> = world.query::<(Entity, &SimId)>().iter(world).map(|(e, id)| (*id, e)).collect();
    ids.sort_unstable_by_key(|x| x.0);
    let id_of = |w: &World, e: Option<Entity>| e.and_then(|e| w.get::<SimId>(e)).map(|id| id.0);
    for &(id, e) in &ids {
        id.hash(&mut h);
        if let (Some(b), Some(hp), Some(a)) = (world.get::<Body>(e), world.get::<Health>(e), world.get::<Action>(e)) {
            for f in [b.pos.x, b.pos.y, b.pos.z, b.yaw, hp.cur] {
                f.to_bits().hash(&mut h);
            }
            a.mv.hash(&mut h);
            a.tick.hash(&mut h);
        }
        if let Some(p) = world.get::<player::Player>(e) {
            p.stamina.to_bits().hash(&mut h);
            p.regain.to_bits().hash(&mut h);
            p.special.to_bits().hash(&mut h);
            p.inventory.hash(&mut h);
            (p.picked, p.slain, p.found, p.checkpoint).hash(&mut h);
            (p.ward_ticks, p.vigor_ticks).hash(&mut h);
            (p.airborne, p.air_vy.to_bits(), p.zone).hash(&mut h);
            id_of(world, p.lock).hash(&mut h);
        }
        if let Some(b) = world.get::<boss::Boss>(e) {
            b.stagger.to_bits().hash(&mut h);
            b.phase.hash(&mut h);
            b.below_target.hash(&mut h);
            b.flight.map(|(a, b)| (a.to_bits(), b.to_bits())).hash(&mut h);
            id_of(world, b.target).hash(&mut h);
        }
        if let Some(s) = world.get::<spell::Spell>(e) {
            for f in [s.pos.x, s.pos.y, s.pos.z] {
                f.to_bits().hash(&mut h);
            }
            s.age.hash(&mut h);
        }
        if let Some(en) = world.get::<enemy::Enemy>(e) {
            (en.spawn, en.state, en.idle).hash(&mut h);
            id_of(world, en.target).hash(&mut h);
            en.poise.to_bits().hash(&mut h);
        }
        if let Some(l) = world.get::<items::Loot>(e) {
            for f in [l.pos.x, l.pos.y, l.pos.z] {
                f.to_bits().hash(&mut h);
            }
            l.item.hash(&mut h);
        }
    }
    h.finish()
}

/// A hasher that gives the same result on every machine (native 64-bit and browser 32-bit):
/// lengths and enum discriminants (`usize`, `isize`) are hashed on 8 bytes.
#[derive(Default)]
pub struct StableHasher(std::collections::hash_map::DefaultHasher);

impl std::hash::Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0.finish()
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }
    fn write_usize(&mut self, n: usize) {
        self.0.write_u64(n as u64);
    }
    fn write_isize(&mut self, n: isize) {
        self.0.write_i64(n as i64);
    }
}

/// Hash of a value, the same on every machine (see `StableHasher`).
pub fn stable_hash<T: std::hash::Hash + ?Sized>(v: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = StableHasher::default();
    v.hash(&mut h);
    h.finish()
}
