//! Deterministic combat simulation at 60 ticks/s.
//!
//! Rules to stay compatible with future network rollback:
//! - no reading of `Time` or hardware input here: only `PlayerInputs` and `SimTick`;
//! - maths via `math` (libm), randomness via `SimRng`;
//! - all state lives in `Clone` components/resources.

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
pub mod spell;
pub mod world;

use bevy::ecs::schedule::ScheduleLabel;
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
    PlayerDied,
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
    Kindled { checkpoint: u8 },
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
    /// Starting progress. `None`: the current player's (or a new game).
    pub progress: Option<Progress>,
}

impl Default for ResetFight {
    fn default() -> Self {
        Self { requested: true, players: 1, progress: None }
    }
}

/// Marker of all simulation entities (for the reset).
#[derive(Component, Clone, Copy, Debug)]
pub struct SimEntity;

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
            .init_resource::<Encounter>()
            .init_resource::<SimCommands>();

        let mut schedule = Schedule::new(SimSchedule);
        schedule.configure_sets(
            (SimSet::Begin, SimSet::Act, SimSet::Physics, SimSet::Combat, SimSet::End).chain(),
        );
        schedule.add_systems((
            (reset_fight, encounter::apply_commands, begin_tick).chain().in_set(SimSet::Begin),
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

/// Runs one simulation tick. Call from `FixedUpdate` (or from rollback later).
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
) {
    if !reset.requested {
        return;
    }
    reset.requested = false;
    let progress = reset.progress.take().unwrap_or_else(|| {
        players
            .iter()
            .min_by_key(|(p, ..)| p.id)
            .map(|(p, b, h)| Progress::of_player(p, b, h, &enc, &tuning))
            .unwrap_or_else(|| Progress::new_game(&tuning))
    });
    for e in &existing {
        commands.entity(e).despawn();
    }
    events.0.clear();
    *rng = rng::SimRng::default();
    *enc = Encounter { defeated: progress.defeated, ..default() };
    spawn_fight(&mut commands, &tuning, reset.players, &progress);
    events.push(SimEvent::Respawned);
}

/// Creates the players, the boss (if it hasn't been defeated) and the path enemies. Entities
/// are created in a fixed order (determinism).
pub fn spawn_fight(commands: &mut Commands, t: &Tuning, players: u8, progress: &Progress) {
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
        let mut p = player::Player::new(id, t);
        p.weapon = progress.weapon.min(t.weapons.len().saturating_sub(1) as u8);
        p.inventory = progress.inventory.clone();
        // An old save's selected item may no longer exist.
        if p.inventory.active_item().is_none() {
            p.inventory.cycle();
        }
        p.checkpoint = checkpoint as u8;
        p.found = progress.found | (1 << checkpoint);
        p.picked = progress.picked;
        p.slain = progress.slain;
        p.zone = zone;
        let mut hp = Health::new(t.player.max_hp);
        if let Some(cur) = progress.hp {
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
    enemy::spawn_all(commands, t, progress.slain);
}

/// Bosses of encounter `i` (`Tuning::encounters`), in their arena (`Tuning::arenas`), asleep at
/// their spawn point, facing the fog you come in through.
pub fn spawn_boss(commands: &mut Commands, t: &Tuning, i: u8) {
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
            Health::new(bd.max_hp),
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

/// Hash of the simulation state: used by the determinism test (and later for network
/// desync detection).
pub fn state_hash(world: &mut World) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    world.resource::<SimTick>().0.hash(&mut h);
    world.resource::<rng::SimRng>().hash(&mut h);
    world.resource::<Encounter>().hash(&mut h);
    let mut q = world.query::<(&Body, &Health, &Action)>();
    for (b, hp, a) in q.iter(world) {
        for f in [b.pos.x, b.pos.y, b.pos.z, b.yaw, hp.cur] {
            f.to_bits().hash(&mut h);
        }
        a.mv.hash(&mut h);
        a.tick.hash(&mut h);
    }
    let mut qp = world.query::<&player::Player>();
    for p in qp.iter(world) {
        p.stamina.to_bits().hash(&mut h);
        p.regain.to_bits().hash(&mut h);
        p.special.to_bits().hash(&mut h);
        p.inventory.hash(&mut h);
        (p.picked, p.slain, p.found, p.checkpoint).hash(&mut h);
        (p.ward_ticks, p.vigor_ticks).hash(&mut h);
        (p.airborne, p.air_vy.to_bits(), p.zone).hash(&mut h);
    }
    let mut qb = world.query::<&boss::Boss>();
    for b in qb.iter(world) {
        b.stagger.to_bits().hash(&mut h);
        b.phase.hash(&mut h);
        b.below_target.hash(&mut h);
        b.flight.map(|(a, b)| (a.to_bits(), b.to_bits())).hash(&mut h);
    }
    let mut qs = world.query::<&spell::Spell>();
    for s in qs.iter(world) {
        for f in [s.pos.x, s.pos.y, s.pos.z] {
            f.to_bits().hash(&mut h);
        }
        s.age.hash(&mut h);
    }
    let mut qe = world.query::<&enemy::Enemy>();
    for e in qe.iter(world) {
        (e.spawn, e.state, e.target, e.idle).hash(&mut h);
        e.poise.to_bits().hash(&mut h);
    }
    let mut ql = world.query::<&items::Loot>();
    for l in ql.iter(world) {
        for f in [l.pos.x, l.pos.y, l.pos.z] {
            f.to_bits().hash(&mut h);
        }
        l.item.hash(&mut h);
    }
    h.finish()
}
