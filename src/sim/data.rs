//! Tuning data loaded from `assets/config/*.ron`.
//!
//! All durations are in **ticks** (60 per second). Distances are in metres,
//! speeds in m/s, angles in degrees.
//! A fighter's local frame: x = right, y = up, z = forward.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::lang::LText;
use super::items::Item;

/// Capsule in the local frame (segment `a`–`b` of radius `r`).
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Capsule {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub r: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reaction {
    /// Small stagger.
    #[default]
    Light,
    /// Knockdown.
    Heavy,
}

/// Hit window active during `[start, end)`.
#[derive(Deserialize, Clone, Debug)]
pub struct HitWindow {
    pub start: u32,
    pub end: u32,
    pub capsule: Capsule,
    /// Rotation of the capsule around the vertical axis during the window (degrees,
    /// start → end). Positive = to the left. Used for horizontal cuts and sweeps.
    #[serde(default)]
    pub arc: Option<[f32; 2]>,
    pub damage: f32,
    /// Damage dealt to the target's stagger gauge.
    #[serde(default)]
    pub stagger: f32,
    #[serde(default)]
    pub reaction: Reaction,
    /// Rage attack: can't be blocked with a normal guard.
    #[serde(default)]
    pub fury: bool,
    /// Area effect (shockwave): neither guard nor perfect guard, only fleeing or
    /// i-frames. The capsule is shown on the ground during the wind-up.
    #[serde(default)]
    pub aoe: bool,
    /// Hitstop ticks applied to both fighters on impact.
    #[serde(default = "default_hitstop")]
    pub hitstop: u8,
    /// Player attack: a white slash is drawn in the air when the window opens, across the
    /// vertical plane in front of them, as tall and as far as the capsule.
    #[serde(default)]
    pub slash: bool,
}

fn default_hitstop() -> u8 {
    4
}

/// "Root motion" movement during `[start, end)`.
#[derive(Deserialize, Clone, Debug)]
pub struct Motion {
    pub start: u32,
    pub end: u32,
    /// Forward speed (negative = backwards).
    #[serde(default)]
    pub speed: f32,
    /// If true, the speed is computed to land at `stop_dist` from the target
    /// (distance frozen at the start of the action), capped by `speed`.
    #[serde(default)]
    pub to_target: bool,
    #[serde(default)]
    pub stop_dist: f32,
    /// With `to_target`: the distance is re-measured every tick until the start of the
    /// movement (rather than frozen at the start of the action). Used for jumps that land on the target.
    #[serde(default)]
    pub retarget: bool,
    /// Rotation in place during the segment (total degrees, positive = to the left):
    /// a large boss pivoting while delivering a tail swipe.
    #[serde(default)]
    pub turn: f32,
    /// Lateral speed (m/s, positive = to the right): a side hop.
    #[serde(default)]
    pub side: f32,
    /// Player: vertical speed (m/s) replacing gravity during the segment (0 = hanging in
    /// the air). A positive one takes off from the ground; a negative one only applies in the
    /// air, until landing.
    #[serde(default)]
    pub lift: Option<f32>,
}

/// Definition of an action (attack, dodge, reaction…).
#[derive(Deserialize, Clone, Debug)]
pub struct MoveDef {
    /// Name of the matching animation clip.
    pub anim: String,
    pub total: u32,
    #[serde(default)]
    pub hits: Vec<HitWindow>,
    #[serde(default)]
    pub motion: Vec<Motion>,
    /// The orientation follows the target / the stick until this tick.
    #[serde(default)]
    pub track_until: u32,
    /// Turning speed during tracking (degrees/s).
    #[serde(default = "default_track_rate")]
    pub track_rate: f32,
    /// Stamina cost. Absent on a weapon attack: computed from the damage
    /// (`stamina_per_damage`), so that the same total damage costs the same whatever the weapon.
    #[serde(default)]
    pub stamina: Option<f32>,
    /// Walking speed allowed during the action (heal), 0 = stationary.
    #[serde(default)]
    pub walk: f32,
    /// Tick from which the next action (combo, buffered input) can start.
    #[serde(default)]
    pub chain_from: Option<u32>,
    /// Tick from which dodge and guard can interrupt the action.
    #[serde(default)]
    pub cancel_from: Option<u32>,
    #[serde(default)]
    pub hyperarmor: Option<[u32; 2]>,
    #[serde(default)]
    pub iframes: Option<[u32; 2]>,
    /// Spells cast during the action (bosses).
    #[serde(default)]
    pub casts: Vec<Cast>,
}

fn default_track_rate() -> f32 {
    360.0
}

impl MoveDef {
    pub fn total_damage(&self) -> f32 {
        self.hits.iter().map(|h| h.damage).sum()
    }
    pub fn chain_tick(&self) -> u32 {
        self.chain_from.unwrap_or(self.total)
    }
    pub fn cancel_tick(&self) -> u32 {
        self.cancel_from.or(self.chain_from).unwrap_or(self.total)
    }
    pub fn in_window(w: Option<[u32; 2]>, tick: u32) -> bool {
        w.is_some_and(|[s, e]| tick >= s && tick < e)
    }
    /// Vertical speed imposed at `tick` (`Motion::lift`).
    pub fn lift_at(&self, tick: u32) -> Option<f32> {
        self.motion.iter().find(|m| tick >= m.start && tick < m.end && m.lift.is_some())?.lift
    }
    /// First active tick and end of the last hit, used to time the animation.
    pub fn strike_span(&self) -> Option<(u32, u32)> {
        let s = self.hits.iter().map(|h| h.start).min()?;
        let e = self.hits.iter().map(|h| h.end).max()?;
        Some((s, e))
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct GuardDef {
    /// Perfect guard window (ticks since the start of the guard).
    pub perfect_window: u32,
    /// Repeated press within `spam_window` ticks: the window shrinks by `spam_penalty`.
    pub spam_window: u32,
    pub spam_penalty: u32,
    pub min_window: u32,
    /// Share of the damage taken with a normal guard (the rest is cancelled).
    pub damage_ratio: f32,
    /// Stamina lost per point of blocked damage.
    pub stamina_ratio: f32,
    /// Stagger dealt to the attacker by a perfect guard.
    pub perfect_stagger: f32,
    /// Special gauge gained from a perfect guard.
    pub perfect_special: f32,
    pub perfect_hitstop: u8,
    /// Frontal arc protected by the guard (degrees on each side).
    pub arc: f32,
    pub walk_speed: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct JumpDef {
    /// Initial vertical speed (m/s): height = v² / (2 × gravity).
    pub speed: f32,
    pub stamina: f32,
    /// Horizontal acceleration in the air (m/s²): you can nudge your trajectory a little, no more.
    pub air_control: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PlayerDef {
    pub max_hp: f32,
    pub max_stamina: f32,
    /// Stamina consumed per point of damage of weapon attacks.
    pub stamina_per_damage: f32,
    /// Stamina can go down to this; at least 1 point is needed to act.
    pub stamina_floor: f32,
    pub stamina_regen: f32,
    pub stamina_regen_guarding: f32,
    pub stamina_delay: u32,
    pub run_speed: f32,
    pub sprint_speed: f32,
    pub sprint_stamina: f32,
    /// Holding dodge beyond this number of ticks triggers the sprint.
    pub sprint_hold: u32,
    pub accel: f32,
    pub turn_rate: f32,
    pub jump: JumpDef,
    pub radius: f32,
    pub height: f32,
    pub lock_range: f32,
    pub input_buffer: u32,
    /// Duration during which the regain can be recovered.
    pub regain_ticks: u32,
    /// HP restored per point of damage dealt.
    pub regain_ratio: f32,
    /// Full special gauge: the special attack is ready, and using it empties the gauge.
    pub special_max: f32,
    /// Gauge gained per point of damage dealt (not by the special itself or a fatal blow).
    pub special_per_damage: f32,
    /// Max distance and arc to trigger the fatal blow.
    pub fatal_range: f32,
    pub fatal_arc: f32,
    pub guard: GuardDef,
    pub dodge: MoveDef,
    pub backstep: MoveDef,
    pub guard_hit: MoveDef,
    pub perfect_guard: MoveDef,
    pub guard_break: MoveDef,
    pub hit_light: MoveDef,
    pub hit_heavy: MoveDef,
    pub switch: MoveDef,
    /// Tick at which the weapon change takes effect during `switch`.
    pub switch_at: u32,
    pub death: MoveDef,
    /// Healing charges (refilled at the checkpoint).
    pub heal_charges: u8,
    /// Share of max HP restored by a heal.
    pub heal_ratio: f32,
    pub heal: MoveDef,
    /// Tick at which the heal applies: hit before it, the charge is lost.
    pub heal_at: u32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct WeaponDef {
    pub name: LText,
    /// What it does best (equipment menu sheet).
    pub description: LText,
    pub light: Vec<MoveDef>,
    pub heavy: MoveDef,
    pub heavy_charged: MoveDef,
    /// Ticks held for a full charge: the charged attack then fires on its own.
    pub charge_ticks: u32,
    pub charge_anim: String,
    pub special: MoveDef,
    pub fatal: MoveDef,
    /// Jump attack (attack during a jump): more damage, little stamina (the jump
    /// already cost some).
    pub jump: MoveDef,
}

#[derive(Deserialize, Clone, Debug)]
pub struct WeaponsDef {
    pub weapons: Vec<WeaponDef>,
}

/// Where the spells of a cast go.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CastAim {
    /// Projectiles towards the target (in a fan of `spread` degrees if there are several);
    /// eruptions under its feet, scattered within a radius of `spread` metres.
    #[default]
    Target,
    /// Straight ahead (fan of `spread` degrees); eruptions in a line, every `step` metres.
    Forward,
    /// In a circle around the caster, at `spread` metres (eruptions) or in all directions.
    Ring,
}

/// Spell cast at tick `at` of an action.
#[derive(Deserialize, Clone, Debug)]
pub struct Cast {
    pub at: u32,
    /// Name of the spell (the boss's `spells`).
    pub spell: String,
    /// Starting point (caster's local frame, at its scale).
    #[serde(default)]
    pub from: [f32; 3],
    #[serde(default)]
    pub aim: CastAim,
    #[serde(default = "one_u8")]
    pub count: u8,
    #[serde(default)]
    pub spread: f32,
    /// Distance between two eruptions in a line (`Forward`), and to the first one.
    #[serde(default = "one")]
    pub step: f32,
    /// Extra delay between two successive eruptions (an advancing wave).
    #[serde(default)]
    pub delay_step: u32,
}

fn one_u8() -> u8 {
    1
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpellKind {
    /// Projectile that flies (and homes in on its target slightly).
    #[default]
    Bolt,
    /// Column that bursts from the ground after a warning (area marked on the ground).
    Eruption,
    /// Continuous stream (breath): from the caster's mouth to the ground in front of it, it follows its
    /// movements for `life` ticks and stops if the attack is interrupted.
    Beam,
}

/// Element of a spell (visual only).
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Element {
    #[default]
    Fire,
    /// The lamplighter's pale light.
    Light,
    /// Spinning iron (thrown cleaver).
    Iron,
    /// Ice of the hollow-spined beast.
    Ice,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SpellDef {
    pub name: String,
    #[serde(default)]
    pub kind: SpellKind,
    #[serde(default)]
    pub element: Element,
    pub damage: f32,
    pub radius: f32,
    #[serde(default)]
    pub reaction: Reaction,
    #[serde(default = "default_hitstop")]
    pub hitstop: u8,
    /// Projectile: speed (m/s) and turning towards the target (degrees/s).
    #[serde(default)]
    pub speed: f32,
    #[serde(default)]
    pub homing: f32,
    /// Eruption: warning ticks before bursting out. Projectile: ticks during which it hangs
    /// where it appears, before heading for its target.
    #[serde(default)]
    pub delay: u32,
    /// Lifetime (projectile) or column duration (eruption), in ticks.
    pub life: u32,
    /// Neither guard nor parry (eruptions always are).
    #[serde(default)]
    pub aoe: bool,
    /// Stream: distance (min, max bounds) between the mouth and the point where it hits the ground, according to
    /// the target at cast time.
    #[serde(default = "default_reach")]
    pub reach: [f32; 2],
}

fn default_reach() -> [f32; 2] {
    [4.0, 12.0]
}

/// Part of a large boss: hittable zone on top of the body, and point that can be locked on.
#[derive(Deserialize, Clone, Debug)]
pub struct PartDef {
    /// Centre (local frame, at the model's scale).
    pub at: [f32; 3],
    pub r: f32,
    /// Lockable (otherwise just a hittable zone).
    #[serde(default = "yes")]
    pub lock: bool,
    /// Model piece followed by the reticle (otherwise the fixed point `at`).
    #[serde(default)]
    pub bone: Option<String>,
}

fn yes() -> bool {
    true
}

/// Side of the target relative to the boss's front.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    #[default]
    Any,
    Left,
    Right,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BossAttack {
    pub name: String,
    pub mv: MoveDef,
    pub min_range: f32,
    pub max_range: f32,
    /// Max angle between the boss's front and the target to launch the attack.
    pub max_angle: f32,
    /// Min angle (backwards attacks: tail swipe…).
    #[serde(default)]
    pub min_angle: f32,
    /// Side where the target must be (pivots to the left or right).
    #[serde(default)]
    pub side: Side,
    pub weight: f32,
    pub cooldown: u32,
    /// Phases in which the attack is available (1 and/or 2). Enemies only have one phase.
    #[serde(default = "default_phases")]
    pub phases: Vec<u8>,
    /// Possible follow-ups: (name of the next attack, probability). A single draw:
    /// probabilities add up (≤ 1), the rest of the time it doesn't chain.
    #[serde(default)]
    pub next: Vec<(String, f32)>,
    /// Jump between levels (onto a gallery, the stairs, and back down): its `to_target` +
    /// `retarget` jump also follows the target's height. Chosen alone, whatever its cooldown and
    /// range, once the target has stood a step above the boss for this many ticks; to come back
    /// down, as soon as the target is below it. Never otherwise.
    #[serde(default)]
    pub climb: Option<u32>,
}

impl BossAttack {
    /// Chained attack (index in `attacks`) for a draw `roll` ∈ [0, 1).
    pub fn chained(&self, attacks: &[BossAttack], mut roll: f32) -> Option<usize> {
        for (name, chance) in &self.next {
            if roll < *chance {
                return attacks.iter().position(|a| &a.name == name);
            }
            roll -= chance;
        }
        None
    }
}

fn default_phases() -> Vec<u8> {
    vec![1, 2]
}

#[derive(Deserialize, Clone, Debug)]
pub struct BossDef {
    /// Key (referenced by the encounters).
    #[serde(default)]
    pub key: String,
    pub name: LText,
    /// Model (`assets/models/<model>.glb`) and its scale.
    #[serde(default = "default_boss_model")]
    pub model: String,
    #[serde(default = "one")]
    pub scale: f32,
    /// Permanent tint of the model (r, g, b, strength).
    #[serde(default)]
    pub tint: Option<[f32; 4]>,
    /// Colour (sRGB) of its spells and ground warnings and, for a main boss, of its torch and of
    /// its medallion on the final door: one per boss, to tell them apart.
    #[serde(default = "default_aoe_color")]
    pub color: [f32; 3],
    /// Supporting role (the butcher's dogs): no health bar at the bottom of the screen, and its death
    /// isn't required for victory.
    #[serde(default)]
    pub minor: bool,
    #[serde(default = "default_boss_mass")]
    pub mass: f32,
    /// Distance under which it backs away (spellcaster); 0 = never.
    #[serde(default)]
    pub keep_away: f32,
    pub max_hp: f32,
    pub phase2_at: f32,
    pub radius: f32,
    pub height: f32,
    pub walk_speed: f32,
    pub strafe_speed: f32,
    pub turn_rate: f32,
    /// Large beast (degrees): it only turns towards its target when the target leaves this
    /// cone, and aims roughly (not exactly) in its direction. Right next to it, it hardly
    /// pivots any more: it's its side attacks that make it turn. 0 = follows the target.
    #[serde(default)]
    pub heading_slack: f32,
    /// Distance the boss tries to keep from its target.
    pub preferred_range: f32,
    pub stagger_max: f32,
    pub stagger_delay: u32,
    pub stagger_decay: f32,
    /// Pause (min, max) between two attacks: that's where you punish.
    pub idle_ticks: [u32; 2],
    /// Duration during which the last attacker keeps the aggro.
    pub aggro_ticks: u32,
    pub groggy: MoveDef,
    pub fatal_received: MoveDef,
    pub roar: MoveDef,
    pub death: MoveDef,
    pub attacks: Vec<BossAttack>,
    /// Hittable zones and lock-on points of large bosses (head, legs…).
    #[serde(default)]
    pub parts: Vec<PartDef>,
    #[serde(default)]
    pub spells: Vec<SpellDef>,
}

fn default_aoe_color() -> [f32; 3] {
    [1.0, 0.3, 0.08]
}

fn default_boss_model() -> String {
    "boss".into()
}

fn default_boss_mass() -> f32 {
    8.0
}

impl BossDef {
    pub fn spell(&self, name: &str) -> Option<u8> {
        self.spells.iter().position(|s| s.name == name).map(|i| i as u8)
    }
}

/// A member of a boss encounter.
#[derive(Deserialize, Clone, Debug)]
pub struct MemberDef {
    pub boss: String,
    /// Offset (x, z) from the boss spawn point.
    #[serde(default)]
    pub offset: [f32; 2],
}

/// What waits in the arena: a boss, a duo, a boss and its dogs…
#[derive(Deserialize, Clone, Debug)]
pub struct EncounterDef {
    pub name: LText,
    pub members: Vec<MemberDef>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BossesDef {
    pub bosses: Vec<BossDef>,
    pub encounters: Vec<EncounterDef>,
}

/// A fog passage: centre of the fog on the ground (x, z), and a point beyond it (the direction
/// in which you go through).
#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct PortalDef {
    pub pos: [f32; 2],
    pub look: [f32; 2],
    /// Half-width of the opening (the fog's).
    #[serde(default = "default_portal_half_width")]
    pub half_width: f32,
}

fn default_portal_half_width() -> f32 {
    1.6
}

/// A boss arena (one per encounter, in the same order). Each one has its own place, far from the
/// level and from the others (out of sight): you get there by going through the fog at the end of
/// its corridor, in the level.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ArenaDef {
    pub name: LText,
    /// Look of the scenery (tools/blender/arena.py).
    #[serde(default)]
    pub theme: String,
    /// In the level: the fog at the end of the corridor, and the torch in front of it (x, z),
    /// lit as long as the boss lives.
    pub gate: PortalDef,
    pub torch: [f32; 2],
    /// The arena: its floors (`ledge`: the bosses don't go there), pillars (x, z, radius),
    /// the fog you come in through (and leave by after the victory), the bosses' spawn point.
    pub floors: Vec<FloorDef>,
    #[serde(default)]
    pub pillars: Vec<[f32; 3]>,
    pub door: PortalDef,
    pub boss_spawn: [f32; 2],
}

#[derive(Deserialize, Clone, Debug)]
pub struct ArenasDef {
    pub arenas: Vec<ArenaDef>,
}

/// Shape of a piece of walkable floor. Game frame: (x, z) on the ground, y = height.
#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub enum Shape {
    /// Horizontal ellipse (a disc if both radii are equal).
    Ellipse { center: [f32; 2], radii: [f32; 2], y: f32 },
    /// Straight strip from `from` to `to` (x, z, y): bridge, ramp or stairs if the heights differ.
    Strip { from: [f32; 3], to: [f32; 3], half_width: f32 },
}

/// Look of a piece of floor (decor only).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloorStyle {
    /// Paving on a rock base.
    #[default]
    Paved,
    /// Stone bridge on arches.
    Bridge,
    /// Plank walkway.
    Planks,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct FloorDef {
    pub shape: Shape,
    /// Walled edges: you bump into them. Otherwise, past the edge, it's the void (and the fall).
    #[serde(default)]
    pub walled: bool,
    /// Threshold of a boss's fog (off-limits to path enemies).
    #[serde(default)]
    pub arena: bool,
    /// In an arena: raised floor (gallery, stairs) where the bosses don't go. You can jump
    /// down from it, onto the boss with a plunging attack.
    #[serde(default)]
    pub ledge: bool,
    /// Behind the final door: walkable only once it's open (all bosses defeated).
    #[serde(default)]
    pub sealed: bool,
    /// Number of steps drawn (decor; the slope is continuous for the simulation).
    #[serde(default)]
    pub steps: u32,
    #[serde(default)]
    pub style: FloorStyle,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct CheckpointDef {
    pub name: LText,
    /// Position (x, z) of the brazier.
    pub pos: [f32; 2],
    /// Point (x, z) the player respawning at the brazier faces (and the camera,
    /// behind them): the way forward. They stand next to the fire, not in front of it, so that the
    /// brazier and its embers don't block the view.
    pub look: [f32; 2],
    /// View of the place in the travel menu: camera position then target point (x, y, z).
    pub view: [[f32; 3]; 2],
}

/// Decor placed on the ground. Collisions are given by `Prop::colliders`.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    /// Street lamp (lit).
    Lamp,
    /// Unlit, bent street lamp.
    DeadLamp,
    /// Dry fountain.
    Fountain,
    Bench,
    /// Toppled horse statue.
    Horse,
    /// Fairground ticket booth (wooden hut).
    Booth,
    /// Broken column (bandstand).
    Column,
    Crates,
}

impl Prop {
    /// Collision circles (dx, dz, radius), in the local frame (z = forward).
    pub fn colliders(self) -> &'static [[f32; 3]] {
        match self {
            Prop::Lamp | Prop::DeadLamp => &[[0.0, 0.0, 0.18]],
            Prop::Fountain => &[[0.0, 0.0, 1.75]],
            Prop::Bench => &[[-0.5, 0.0, 0.32], [0.5, 0.0, 0.32]],
            Prop::Horse => &[[0.0, -0.45, 0.4], [0.0, 0.45, 0.4]],
            Prop::Booth => &[[0.0, 0.0, 0.95]],
            Prop::Column => &[[0.0, 0.0, 0.3]],
            Prop::Crates => &[[0.0, 0.0, 0.55]],
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct PropDef {
    pub kind: Prop,
    pub pos: [f32; 2],
    /// Orientation (degrees).
    #[serde(default)]
    pub yaw: f32,
}

/// Enemy placed in the level.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct EnemySpawn {
    /// Key of the enemy type (`enemies.ron`).
    pub kind: String,
    pub pos: [f32; 2],
    #[serde(default)]
    pub yaw: f32,
    /// Asleep: you have to get closer to wake it, but it sees in all
    /// directions. Otherwise, it watches ahead.
    #[serde(default)]
    pub asleep: bool,
    /// Enemies of the same group (> 0) raise the alarm together.
    #[serde(default)]
    pub group: u8,
    /// Doesn't respawn once defeated.
    #[serde(default)]
    pub unique: bool,
}

/// Item glowing on the ground, picked up only once per game.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PickupDef {
    pub pos: [f32; 2],
    pub items: Vec<(Item, u8)>,
}

/// The level around the arena: floors, checkpoints, decor, enemies, items.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LevelDef {
    pub floors: Vec<FloorDef>,
    pub checkpoints: Vec<CheckpointDef>,
    #[serde(default)]
    pub props: Vec<PropDef>,
    #[serde(default)]
    pub enemies: Vec<EnemySpawn>,
    #[serde(default)]
    pub pickups: Vec<PickupDef>,
    /// The door that opens once all the bosses are defeated, and the painted panel that awaits
    /// beyond it, floating over the void (x, z; tools/blender/arena.py).
    pub final_door: PortalDef,
    pub finale: [f32; 2],
}

/// Enemy type (dog, puppet…).
#[derive(Deserialize, Clone, Debug)]
pub struct EnemyDef {
    pub key: String,
    /// Model (`assets/models/<model>.glb`); several types can share a model.
    pub model: String,
    /// Model scale (hit reaches must be given at this scale).
    #[serde(default = "one")]
    pub scale: f32,
    /// Permanent tint of the model (r, g, b, strength).
    #[serde(default)]
    pub tint: Option<[f32; 4]>,
    pub name: LText,
    pub max_hp: f32,
    pub radius: f32,
    pub height: f32,
    pub mass: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub turn_rate: f32,
    /// Detection distance (halved if it's asleep, and then in all directions).
    pub sight: f32,
    /// Max distance from its starting point before giving up the chase.
    pub leash: f32,
    pub preferred_range: f32,
    pub idle_ticks: [u32; 2],
    /// Damage taken (over ~1 s) before being staggered; 0 = always.
    pub poise: f32,
    /// What it may drop when its body vanishes: at most one item, each with its own chance.
    #[serde(default)]
    pub loot: Vec<LootDef>,
    /// Alert cry.
    pub alert: MoveDef,
    pub hit: MoveDef,
    pub death: MoveDef,
    pub attacks: Vec<BossAttack>,
}

/// A possible drop: `item`, with probability `chance` (0..1).
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct LootDef {
    pub item: Item,
    pub chance: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Deserialize, Clone, Debug)]
pub struct EnemiesDef {
    pub kinds: Vec<EnemyDef>,
}

/// All the tuning data used by the simulation.
#[derive(Resource, Clone, Debug)]
pub struct Tuning {
    pub player: PlayerDef,
    pub weapons: Vec<WeaponDef>,
    /// The first is the Automaton (`boss.ron`), then those of `bosses.ron`.
    pub bosses: Vec<BossDef>,
    /// Boss encounters (the first: the Automaton alone), and their arenas (same order).
    pub encounters: Vec<EncounterDef>,
    pub arenas: Vec<ArenaDef>,
    pub level: LevelDef,
    pub enemies: Vec<EnemyDef>,
}

pub const PLAYER_RON: &str = include_str!("../../assets/config/player.ron");
pub const WEAPONS_RON: &str = include_str!("../../assets/config/weapons.ron");
pub const BOSS_RON: &str = include_str!("../../assets/config/boss.ron");
pub const BOSSES_RON: &str = include_str!("../../assets/config/bosses.ron");
pub const ARENAS_RON: &str = include_str!("../../assets/config/arenas.ron");
pub const LEVEL_RON: &str = include_str!("../../assets/config/level.ron");
pub const ENEMIES_RON: &str = include_str!("../../assets/config/enemies.ron");

/// Contents of the tuning files, in the order of `Tuning::parse`.
pub struct TuningSources<'a> {
    pub player: &'a str,
    pub weapons: &'a str,
    pub boss: &'a str,
    pub bosses: &'a str,
    pub arenas: &'a str,
    pub level: &'a str,
    pub enemies: &'a str,
}

impl Tuning {
    pub fn parse(src: &TuningSources) -> Result<Self, String> {
        let opts = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let p = |name: &str, e: ron::error::SpannedError| format!("{name}: {e}");
        let mut automaton: BossDef = opts.from_str(src.boss).map_err(|e| p("boss.ron", e))?;
        if automaton.key.is_empty() {
            automaton.key = "automaton".into();
        }
        let more: BossesDef = opts.from_str(src.bosses).map_err(|e| p("bosses.ron", e))?;
        let mut encounters = vec![EncounterDef {
            name: automaton.name.clone(),
            members: vec![MemberDef { boss: automaton.key.clone(), offset: [0.0, 0.0] }],
        }];
        encounters.extend(more.encounters);
        let t = Self {
            player: opts.from_str(src.player).map_err(|e| p("player.ron", e))?,
            weapons: opts
                .from_str::<WeaponsDef>(src.weapons)
                .map_err(|e| p("weapons.ron", e))?
                .weapons,
            bosses: std::iter::once(automaton).chain(more.bosses).collect(),
            encounters,
            arenas: opts.from_str::<ArenasDef>(src.arenas).map_err(|e| p("arenas.ron", e))?.arenas,
            level: opts.from_str(src.level).map_err(|e| p("level.ron", e))?,
            enemies: opts.from_str::<EnemiesDef>(src.enemies).map_err(|e| p("enemies.ron", e))?.kinds,
        };
        for e in &t.level.enemies {
            if t.enemy_kind(&e.kind).is_none() {
                return Err(format!("level.ron: unknown enemy type '{}'", e.kind));
            }
        }
        if t.level.checkpoints.is_empty() {
            return Err("level.ron: il faut au moins un checkpoint".into());
        }
        if t.arenas.len() != t.encounters.len() {
            return Err(format!("arenas.ron: {} arenas for {} encounters", t.arenas.len(), t.encounters.len()));
        }
        if t.encounters.len() > 32 {
            return Err("bosses.ron: 32 encounters at most".into());
        }
        for e in &t.encounters {
            for m in &e.members {
                if t.boss_kind(&m.boss).is_none() {
                    return Err(format!("bosses.ron: unknown boss '{}'", m.boss));
                }
            }
        }
        for b in &t.bosses {
            for a in &b.attacks {
                for c in &a.mv.casts {
                    if b.spell(&c.spell).is_none() {
                        return Err(format!("{}/{}: unknown spell '{}'", b.key, a.name, c.spell));
                    }
                }
            }
        }
        // As soon as an area effect's circle appears, the boss stops tracking its target: the
        // hit lands where the circle announced it.
        let mut t = t;
        for b in &mut t.bosses {
            for a in &mut b.attacks {
                let mv = &a.mv;
                let lock = mv.hits.iter().filter(|h| h.aoe).map(|h| super::boss::aoe_lock_tick(mv, h)).min();
                if let Some(lock) = lock {
                    a.mv.track_until = a.mv.track_until.min(lock);
                }
            }
        }
        Ok(t)
    }

    /// Data compiled into the binary (used at startup and by the tests).
    pub fn builtin() -> Self {
        Self::parse(&TuningSources {
            player: PLAYER_RON,
            weapons: WEAPONS_RON,
            boss: BOSS_RON,
            bosses: BOSSES_RON,
            arenas: ARENAS_RON,
            level: LEVEL_RON,
            enemies: ENEMIES_RON,
        })
        .expect("invalid built-in tuning")
    }

    /// The main boss of encounter `i` (its first member that isn't a supporting role).
    pub fn encounter_boss(&self, i: usize) -> Option<u8> {
        let e = self.encounters.get(i)?;
        let kinds = e.members.iter().filter_map(|m| self.boss_kind(&m.boss));
        kinds.clone().find(|&k| !self.bosses[k as usize].minor).or(kinds.clone().next())
    }

    /// Colour of boss `i`'s torch and medallion.
    pub fn encounter_color(&self, i: usize) -> [f32; 3] {
        self.encounter_boss(i).map_or([1.0, 0.5, 0.2], |b| self.bosses[b as usize].color)
    }

    /// Index of boss `key`.
    pub fn boss_kind(&self, key: &str) -> Option<u8> {
        self.bosses.iter().position(|b| b.key == key).map(|i| i as u8)
    }

    /// Index of enemy type `key`.
    pub fn enemy_kind(&self, key: &str) -> Option<u8> {
        self.enemies.iter().position(|k| k.key == key).map(|i| i as u8)
    }

    pub fn get(&self, r: MoveRef) -> &MoveDef {
        match r {
            MoveRef::Player(m) => {
                let p = &self.player;
                match m {
                    PlayerMove::Dodge => &p.dodge,
                    PlayerMove::Backstep => &p.backstep,
                    PlayerMove::GuardHit => &p.guard_hit,
                    PlayerMove::PerfectGuard => &p.perfect_guard,
                    PlayerMove::GuardBreak => &p.guard_break,
                    PlayerMove::HitLight => &p.hit_light,
                    PlayerMove::HitHeavy => &p.hit_heavy,
                    PlayerMove::Switch => &p.switch,
                    PlayerMove::Death => &p.death,
                    PlayerMove::Heal => &p.heal,
                }
            }
            MoveRef::Weapon(w, m) => {
                let w = &self.weapons[w as usize];
                match m {
                    WeaponMove::Light(i) => &w.light[i as usize],
                    WeaponMove::Heavy => &w.heavy,
                    WeaponMove::HeavyCharged => &w.heavy_charged,
                    WeaponMove::Special => &w.special,
                    WeaponMove::Fatal => &w.fatal,
                    WeaponMove::Jump => &w.jump,
                }
            }
            MoveRef::BossAttack(b, i) => &self.bosses[b as usize].attacks[i as usize].mv,
            MoveRef::Boss(b, m) => {
                let b = &self.bosses[b as usize];
                match m {
                    BossMove::Groggy => &b.groggy,
                    BossMove::FatalReceived => &b.fatal_received,
                    BossMove::Roar => &b.roar,
                    BossMove::Death => &b.death,
                }
            }
            MoveRef::Enemy(k, m) => {
                let e = &self.enemies[k as usize];
                match m {
                    EnemyMove::Attack(i) => &e.attacks[i as usize].mv,
                    EnemyMove::Alert => &e.alert,
                    EnemyMove::Hit => &e.hit,
                    EnemyMove::Death => &e.death,
                }
            }
        }
    }
}

/// Compact (Copy) reference to a `MoveDef`: this is what's stored in the sim state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveRef {
    Player(PlayerMove),
    Weapon(u8, WeaponMove),
    /// Boss attack: (boss, attack).
    BossAttack(u8, u16),
    /// Shared boss action: (boss, action).
    Boss(u8, BossMove),
    /// Enemy action: (type, action).
    Enemy(u8, EnemyMove),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayerMove {
    Dodge,
    Backstep,
    GuardHit,
    PerfectGuard,
    GuardBreak,
    HitLight,
    HitHeavy,
    Switch,
    Death,
    Heal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeaponMove {
    Light(u8),
    Heavy,
    HeavyCharged,
    Special,
    Fatal,
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BossMove {
    Groggy,
    FatalReceived,
    Roar,
    Death,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyMove {
    Attack(u8),
    Alert,
    Hit,
    Death,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tuning_parses() {
        let t = Tuning::builtin();
        assert_eq!(t.weapons.len(), 2);
        assert!(!t.bosses[0].attacks.is_empty());
        // Boss follow-ups point to existing attacks.
        for b in &t.bosses {
            for a in &b.attacks {
                for (n, _) in &a.next {
                    assert!(b.attacks.iter().any(|x| &x.name == n), "{}: next inconnu: {n}", b.key);
                }
                for h in &a.mv.hits {
                    assert!(h.start < h.end && h.end <= a.mv.total, "{}/{}: invalid window", b.key, a.name);
                }
                for c in &a.mv.casts {
                    assert!(c.at < a.mv.total, "{}/{}: spell cast after the end", b.key, a.name);
                }
            }
        }
        for k in &t.enemies {
            for a in &k.attacks {
                for h in &a.mv.hits {
                    assert!(h.start < h.end && h.end <= a.mv.total, "{}/{}: invalid window", k.key, a.name);
                }
                for (n, _) in &a.next {
                    assert!(k.attacks.iter().any(|b| &b.name == n), "{}: next inconnu: {n}", k.key);
                }
            }
        }
    }
}
