//! Sensory feedback triggered by simulation events: sounds, sparks,
//! camera shake, boss flash, ground warnings for area attacks. And ambient
//! particles: embers and ash from braziers, items and loot on the ground, fountain water.

use bevy::prelude::*;

use crate::render::AppState;
use crate::render::camera::CameraRig;
use crate::render::models::TintFlash;
use crate::render::ps1::Ps1Material;
use crate::sim::boss::{Boss, aoe_telegraph};
use crate::sim::data::{Element, SpellKind, Tuning};
use crate::sim::spell::{SPLASH_LIFE, Spell, splash_radius, telegraph};
use crate::sim::enemy::Enemy;
use crate::sim::fighter::{Action, Body};
use crate::sim::items::Item;
use crate::sim::{SimEvent, SimEvents, encounter, world};

#[derive(Resource)]
/// Loaded sounds, and effects volume (copied from the settings every frame).
pub struct Sounds(std::collections::HashMap<&'static str, Handle<AudioSource>>, f32);

#[derive(Resource)]
pub struct SparkAssets {
    mesh: Handle<Mesh>,
    perfect: Handle<Ps1Material>,
    guard: Handle<Ps1Material>,
    hit: Handle<Ps1Material>,
    fury: Handle<Ps1Material>,
    heal: Handle<Ps1Material>,
    /// Embers flying off (enemy defeated, checkpoint discovered).
    ember: Handle<Ps1Material>,
    /// Ambient embers: they shine through the fog.
    glow: Handle<Ps1Material>,
    ash: Handle<Ps1Material>,
    water: Handle<Ps1Material>,
    /// Pale light of items on the ground: it shines through the fog.
    wisp: Handle<Ps1Material>,
    /// Ring of radius 1 lying on the ground (shockwave).
    ring: Handle<Mesh>,
    /// Disc of radius 1 lying on the ground (eruption warning).
    disc: Handle<Mesh>,
    /// Projectile (sphere of radius 1), eruption column (cylinder of radius 1, height 1).
    orb: Handle<Mesh>,
    column: Handle<Mesh>,
    /// White crescent in the vertical plane (x = 0), 1 m tall and 1 m deep, scaled to the cut.
    crescent: Handle<Mesh>,
    /// Spell materials, per element: bright core, sparks, ground warning.
    spell: std::collections::HashMap<Element, SpellMats>,
    /// The same in each boss's colour (index of its definition): its spells, its
    /// shockwaves and its warnings are all in its colour.
    boss: Vec<SpellMats>,
    /// Embers of each boss's torch, in its colour (`Tuning::encounter_color`).
    torch: Vec<Handle<Ps1Material>>,
}

impl SparkAssets {
    /// Materials of a spell: its caster's colour, except iron (cleaver) which stays iron.
    fn spell_mats(&self, element: Element, boss: u8) -> Option<&SpellMats> {
        if element == Element::Iron {
            return self.spell.get(&element);
        }
        self.boss.get(boss as usize).or_else(|| self.spell.get(&element))
    }
}

#[derive(Clone)]
struct SpellMats {
    /// Bright core, translucent halo around it.
    core: Handle<Ps1Material>,
    halo: Handle<Ps1Material>,
    spark: Handle<Ps1Material>,
    /// Shockwave (opaque ring).
    shock: Handle<Ps1Material>,
}

/// Rendering of a spell (put on the simulation entity).
#[derive(Component)]
struct SpellVisual {
    /// Eruption: warning outline and disc, column (halo, core). Projectile: sphere (core),
    /// halo or blade (column), and the burning puddle where it crashes (disc).
    ring: Option<Entity>,
    fill: Option<Entity>,
    column: Option<Entity>,
    inner: Option<Entity>,
    /// Emitted sparks (projectiles): remainder between two frames.
    trail: f32,
}

/// Ground warning of an area attack: circle outline (`fill: false`) and disc that
/// grows until the impact (`fill: true`).
#[derive(Component)]
struct AoeMarker {
    fill: bool,
    /// Height above the ground (the ring over the disc).
    lift: f32,
}

/// Shockwave ring, which widens and fades after the impact.
#[derive(Component)]
struct Shock {
    radius: f32,
    life: f32,
}

const SHOCK_LIFE: f32 = 0.35;

/// White slash left in the air by a cut (`SimEvent::Slash`), fading out.
#[derive(Component)]
struct SlashTrail {
    life: f32,
    /// Scale when it appears (it widens slightly as it fades).
    scale: Vec3,
}

const SLASH_LIFE: f32 = 0.35;

#[derive(Component)]
struct Particle {
    vel: Vec3,
    life: f32,
    max: f32,
    gravity: f32,
    /// Ground height under the emission point (sparks bounce there).
    floor: f32,
    /// Horizontal oscillation (m/s): ash and embers dancing as they rise.
    sway: f32,
    phase: f32,
    /// Disappears when touching the ground (water drops) instead of bouncing.
    splash: bool,
}

impl Particle {
    fn new(vel: Vec3, life: f32, gravity: f32, floor: f32) -> Self {
        Self { vel, life, max: life, gravity, floor, sway: 0.0, phase: 0.0, splash: false }
    }
}

/// Recent events, readable by the HUD (banner, flash…).
#[derive(Resource, Default)]
pub struct FxState {
    pub perfect_flash: f32,
    pub fury_flash: f32,
    /// Stamina bar blinking when an action is refused.
    pub no_stamina: f32,
    pub last: Vec<SimEvent>,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FxState>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (consume_events, update_particles, aoe_markers, update_shocks, update_slashes, ambient, spell_visuals).run_if(in_state(AppState::Playing)),
            );
    }
}

const SOUNDS: [&str; 19] = [
    "heal",
    "perfect_guard", "guard", "guard_break", "hit", "hit_heavy", "slam", "swing", "swing_heavy",
    "dodge", "fury", "fatal", "roar", "switch",
    "bark", "creak", "pickup", "kindle", "fall",
];

fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    tuning: Res<Tuning>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<Ps1Material>>,
) {
    commands.insert_resource(Sounds(
        SOUNDS.iter().map(|s| (*s, server.load(format!("audio/{s}.wav")))).collect(),
        1.0,
    ));
    // Discs and rings of radius 1, lying on the ground (scaled to the area's radius).
    let flat = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    let ring = meshes.add(Annulus::new(0.93, 1.0).mesh().resolution(40));
    let disc = meshes.add(Circle::new(1.0).mesh().resolution(40));
    commands.insert_resource(SparkAssets {
        mesh: meshes.add(Cuboid::new(0.05, 0.05, 0.05)),
        perfect: mats.add(Ps1Material::unlit(Color::srgb(1.0, 0.95, 0.6))),
        guard: mats.add(Ps1Material::unlit(Color::srgb(1.0, 0.6, 0.2))),
        hit: mats.add(Ps1Material::unlit(Color::srgb(0.45, 0.05, 0.04))),
        fury: mats.add(Ps1Material::unlit(Color::srgb(1.0, 0.1, 0.05))),
        heal: mats.add(Ps1Material::unlit(Color::srgb(0.45, 1.0, 0.55))),
        ember: mats.add(Ps1Material::unlit(Color::srgb(1.0, 0.62, 0.2))),
        glow: mats.add({
            let mut m = Ps1Material::unlit(Color::srgb(1.0, 0.55, 0.16));
            m.params.emissive = Vec4::new(0.6, 0.25, 0.05, 0.0);
            m
        }),
        ash: mats.add(Ps1Material::unlit(Color::srgb(0.5, 0.48, 0.46))),
        water: mats.add(Ps1Material::unlit(Color::srgb(0.55, 0.72, 0.85))),
        wisp: mats.add({
            let mut m = Ps1Material::unlit(Color::srgb(1.0, 0.96, 0.8));
            m.params.emissive = Vec4::new(0.6, 0.55, 0.4, 0.0);
            m
        }),
        ring: ring.clone(),
        disc: disc.clone(),
        orb: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
        column: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(10)),
        crescent: meshes.add(crescent_mesh()),
        spell: [
            (Element::Fire, (1.0, 0.82, 0.4), (1.0, 0.35, 0.05), Color::srgba(1.0, 0.3, 0.05, 0.55)),
            (Element::Light, (1.0, 0.97, 0.85), (0.8, 0.75, 0.45), Color::srgba(0.95, 0.85, 0.5, 0.45)),
            (Element::Iron, (0.45, 0.42, 0.4), (0.0, 0.0, 0.0), Color::srgba(0.3, 0.28, 0.27, 0.0)),
            (Element::Ice, (0.85, 0.96, 1.0), (0.3, 0.62, 0.95), Color::srgba(0.55, 0.82, 1.0, 0.5)),
        ]
        .into_iter()
        .map(|(el, c, glow, halo)| (el, spell_mats(&mut mats, c, glow, halo)))
        .collect(),
        // Core paled towards white, halo and glow in the boss's colour.
        boss: tuning
            .bosses
            .iter()
            .map(|b| {
                let [r, g, bl] = b.color;
                let pale = |x: f32| x + (1.0 - x) * 0.55;
                spell_mats(&mut mats, (pale(r), pale(g), pale(bl)), (r, g, bl), Color::srgba(r, g, bl, 0.5))
            })
            .collect(),
        torch: (0..tuning.arenas.len())
            .map(|i| {
                let [r, g, b] = tuning.encounter_color(i);
                let mut m = Ps1Material::unlit(Color::srgb(r, g, b).mix(&Color::WHITE, 0.3));
                m.params.emissive = Vec4::new(r, g, b, 0.0) * 0.7;
                mats.add(m)
            })
            .collect(),
    });
    let blend = |c: Color| {
        let mut m = Ps1Material::unlit(c);
        m.alpha_mode = AlphaMode::Blend;
        m
    };
    for (fill, mesh, color, y) in [
        (false, ring, Color::srgba(1.0, 0.3, 0.08, 0.9), 0.05),
        (true, disc, Color::srgba(0.9, 0.12, 0.04, 0.35), 0.04),
    ] {
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mats.add(blend(color))),
            flat.with_translation(Vec3::Y * y),
            Visibility::Hidden,
            AoeMarker { fill, lift: y },
        ));
    }
}

/// Crescent of a vertical cut, seen from the side: an arc of ellipse from above the head
/// (slightly behind) down to the ground ahead, thick in the middle and tapering at both ends.
/// Local frame: z = forward, y = up, at most 1 m tall and 1 m ahead. Both faces are drawn.
fn crescent_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::mesh::{Indices, PrimitiveTopology};
    const N: u32 = 24;
    let (cy, cz, ry, rz) = (0.18, 0.08, 0.82, 0.92);
    let (from, to) = (100f32.to_radians(), (-45f32).to_radians());
    let mut pos = Vec::new();
    for i in 0..=N {
        let f = i as f32 / N as f32;
        let a = from + (to - from) * f;
        let (s, c) = a.sin_cos();
        let thick = 1.0 - 0.3 * (f * std::f32::consts::PI).sin();
        pos.push([0.0, cy + ry * s, cz + rz * c]);
        pos.push([0.0, cy + ry * thick * s, cz + rz * thick * c]);
    }
    let mut idx = Vec::new();
    for i in 0..N {
        let (o, i0, o1, i1) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        idx.extend([o, i0, o1, o1, i0, i1, o, o1, i0, o1, i1, i0]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[1.0, 0.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n])
        .with_inserted_indices(Indices::U32(idx))
}

/// Materials of a spell: core `c` glowing with `glow`, translucent halo, sparks, shockwave.
fn spell_mats(mats: &mut Assets<Ps1Material>, c: (f32, f32, f32), glow: (f32, f32, f32), halo: Color) -> SpellMats {
    let core = mats.add({
        let mut m = Ps1Material::unlit(Color::srgb(c.0, c.1, c.2));
        m.params.emissive = Vec4::new(glow.0, glow.1, glow.2, 0.0);
        m
    });
    let halo = mats.add({
        let mut m = Ps1Material::unlit(halo);
        m.alpha_mode = AlphaMode::Blend;
        m.params.emissive = Vec4::new(glow.0 * 0.5, glow.1 * 0.5, glow.2 * 0.5, 0.0);
        m
    });
    let spark = mats.add({
        let mut m = Ps1Material::unlit(Color::srgb(c.0, c.1 * 0.8, c.2 * 0.6));
        m.params.emissive = Vec4::new(glow.0 * 0.7, glow.1 * 0.7, glow.2 * 0.7, 0.0);
        m
    });
    let shock = mats.add({
        let mut m = Ps1Material::unlit(Color::srgb(glow.0, glow.1, glow.2));
        m.params.emissive = Vec4::new(glow.0 * 0.6, glow.1 * 0.6, glow.2 * 0.6, 0.0);
        m
    });
    SpellMats { core, halo, spark, shock }
}

pub fn play(commands: &mut Commands, sounds: &Sounds, name: &str, volume: f32) {
    let volume = volume * sounds.1;
    if let Some(h) = sounds.0.get(name) {
        commands.spawn((
            AudioPlayer::new(h.clone()),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(volume)),
        ));
    }
}

/// Burst of sparks. `rise`: embers that rise gently instead of falling back.
#[allow(clippy::too_many_arguments)]
fn burst(commands: &mut Commands, sp: &SparkAssets, mat: &Handle<Ps1Material>, pos: Vec3, n: usize, speed: f32, seed: u32, floor: f32) {
    spray(commands, sp, mat, pos, n, speed, seed, floor, false);
}

#[allow(clippy::too_many_arguments)]
fn spray(commands: &mut Commands, sp: &SparkAssets, mat: &Handle<Ps1Material>, pos: Vec3, n: usize, speed: f32, seed: u32, floor: f32, rise: bool) {
    for i in 0..n {
        // Local pseudo-random (purely visual, outside the simulation).
        let h = |k: u32| {
            let x = seed.wrapping_mul(747796405).wrapping_add((i as u32).wrapping_mul(2891336453)).wrapping_add(k.wrapping_mul(1013904223)) >> 9;
            (x & 0xffff) as f32 / 65535.0 * 2.0 - 1.0
        };
        let dir = Vec3::new(h(1), h(2).abs() * 0.8 + 0.2, h(3)).normalize_or_zero();
        let (life, gravity, start) = if rise {
            (0.9 + h(4).abs() * 1.0, -1.5, pos + Vec3::new(h(7), h(8).abs() * 1.2, h(9)) * 0.5)
        } else {
            (0.25 + h(4).abs() * 0.3, 9.0, pos)
        };
        commands.spawn((
            Mesh3d(sp.mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(start).with_scale(Vec3::splat(1.0 + h(5).abs())),
            Particle::new(dir * speed * (0.5 + h(6).abs()), life, gravity, floor),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn consume_events(
    mut commands: Commands,
    mut events: ResMut<SimEvents>,
    mut sounds: ResMut<Sounds>,
    settings: Res<crate::settings::Settings>,
    sparks: Res<SparkAssets>,
    mut rig: ResMut<CameraRig>,
    mut fx: ResMut<FxState>,
    mut flashes: Query<&mut TintFlash>,
    mut mats: ResMut<Assets<Ps1Material>>,
    transforms: Query<&GlobalTransform>,
    enemies: Query<&Enemy>,
    local: Query<(), With<crate::render::LocalPlayer>>,
    tuning: Res<Tuning>,
    time: Res<Time>,
    mut heard: Local<Vec<u32>>,
) {
    let floor = |p: Vec3| world::floor_at(&tuning, p.x, p.z, p.y - 1.0).unwrap_or(-1000.0);
    // A single sound per attack, whatever its number of spells.
    let mut first = |key: u32| {
        let new = !heard.contains(&key);
        if new {
            heard.push(key);
            if heard.len() > 16 {
                heard.remove(0);
            }
        }
        new
    };
    sounds.1 = settings.effects_volume;
    let dt = time.delta_secs();
    fx.perfect_flash = (fx.perfect_flash - dt * 4.0).max(0.0);
    fx.fury_flash = (fx.fury_flash - dt * 2.5).max(0.0);
    fx.no_stamina = (fx.no_stamina - dt * 3.0).max(0.0);
    fx.last.clear();
    let seed = (time.elapsed_secs() * 1000.0) as u32;
    for (k, e) in events.0.drain(..).enumerate() {
        let seed = seed.wrapping_add(k as u32 * 7919);
        fx.last.push(e);
        match e {
            SimEvent::PerfectGuard { pos } => {
                play(&mut commands, &sounds, "perfect_guard", 1.0);
                burst(&mut commands, &sparks, &sparks.perfect, pos, 22, 6.0, seed, floor(pos));
                fx.perfect_flash = 1.0;
                rig.shake = rig.shake.max(0.5);
            }
            SimEvent::Guard { pos } => {
                play(&mut commands, &sounds, "guard", 0.8);
                burst(&mut commands, &sparks, &sparks.guard, pos, 8, 3.5, seed, floor(pos));
                rig.shake = rig.shake.max(0.35);
            }
            SimEvent::GuardBreak { pos } => {
                play(&mut commands, &sounds, "guard_break", 1.0);
                burst(&mut commands, &sparks, &sparks.guard, pos, 14, 4.0, seed, floor(pos));
                rig.shake = rig.shake.max(0.7);
            }
            SimEvent::Hit { pos, heavy, foe } => {
                let on_player = foe.is_none();
                play(&mut commands, &sounds, if heavy || on_player { "hit_heavy" } else { "hit" }, 0.9);
                burst(&mut commands, &sparks, &sparks.hit, pos, if heavy { 16 } else { 8 }, 3.0, seed, floor(pos));
                rig.shake = rig.shake.max(if on_player { 0.8 } else if heavy { 0.5 } else { 0.2 });
                // White flash on the opponent hit (not on a corpse lying near the impact point).
                if let Some(mut f) = foe.and_then(|e| flashes.get_mut(e).ok()) {
                    f.white = 1.0;
                }
            }
            SimEvent::Swing { heavy, .. } => {
                play(&mut commands, &sounds, if heavy { "swing_heavy" } else { "swing" }, 0.5);
            }
            SimEvent::Slash { pos, yaw, reach, top } => {
                // Its own material: it fades out on its own.
                let mut m = Ps1Material::unlit(Color::srgba(1.0, 1.0, 1.0, 0.9));
                m.alpha_mode = AlphaMode::Blend;
                m.params.emissive = Vec4::new(0.8, 0.8, 0.8, 0.0);
                let scale = Vec3::new(1.0, top, reach);
                commands.spawn((
                    Mesh3d(sparks.crescent.clone()),
                    MeshMaterial3d(mats.add(m)),
                    Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)).with_scale(scale),
                    SlashTrail { life: SLASH_LIFE, scale },
                ));
            }
            SimEvent::Dodge { .. } => play(&mut commands, &sounds, "dodge", 0.5),
            SimEvent::Jumped { .. } => play(&mut commands, &sounds, "dodge", 0.3),
            SimEvent::Landed { entity } => {
                play(&mut commands, &sounds, "slam", 0.12);
                if let Ok(t) = transforms.get(entity) {
                    let at = t.translation() + Vec3::Y * 0.05;
                    burst(&mut commands, &sparks, &sparks.ash, at, 8, 1.2, seed, floor(at + Vec3::Y));
                }
            }
            SimEvent::FuryWarn { .. } => {
                play(&mut commands, &sounds, "fury", 0.9);
                fx.fury_flash = 1.0;
            }
            SimEvent::Groggy { .. } => {
                play(&mut commands, &sounds, "guard_break", 0.8);
                rig.shake = rig.shake.max(0.4);
            }
            SimEvent::Fatal { pos } => {
                play(&mut commands, &sounds, "fatal", 1.0);
                burst(&mut commands, &sparks, &sparks.fury, pos, 30, 5.0, seed, floor(pos));
                rig.shake = rig.shake.max(0.9);
            }
            SimEvent::BossPhase2 => {
                play(&mut commands, &sounds, "roar", 1.0);
                rig.shake = rig.shake.max(1.0);
            }
            SimEvent::BossDied => play(&mut commands, &sounds, "slam", 1.0),
            SimEvent::PlayerDied { .. } => play(&mut commands, &sounds, "slam", 0.8),
            SimEvent::Revived { entity } => {
                play(&mut commands, &sounds, "heal", 0.9);
                if local.contains(entity) {
                    rig.initialized = false;
                }
            }
            SimEvent::WeaponSwitched { .. } => play(&mut commands, &sounds, "switch", 0.6),
            SimEvent::Heal { entity } => {
                play(&mut commands, &sounds, "heal", 0.8);
                if let Ok(t) = transforms.get(entity) {
                    burst(&mut commands, &sparks, &sparks.heal, t.translation() + Vec3::Y * 1.2, 14, 1.6, seed, floor(t.translation() + Vec3::Y * 1.2));
                }
            }
            SimEvent::NoStamina { .. } => fx.no_stamina = 1.0,
            SimEvent::ItemCycled { .. } => play(&mut commands, &sounds, "switch", 0.35),
            SimEvent::BossAwake => {
                play(&mut commands, &sounds, "roar", 1.0);
                rig.shake = rig.shake.max(0.8);
            }
            SimEvent::BossRevived { arena } => {
                // The torch flares up again, in the boss's colour.
                play(&mut commands, &sounds, "kindle", 1.0);
                play(&mut commands, &sounds, "roar", 0.35);
                let at = encounter::torch_pos(&tuning, arena as usize) + Vec3::Y * TORCH_FLAME;
                let m = sparks.torch.get(arena as usize).unwrap_or(&sparks.ember).clone();
                spray(&mut commands, &sparks, &m, at, 40, 2.5, seed, floor(at), true);
            }
            SimEvent::Passage { entity, .. } => {
                play(&mut commands, &sounds, "dodge", 0.7);
                if local.contains(entity) {
                    rig.initialized = false;
                }
            }
            SimEvent::DoorOpened => play(&mut commands, &sounds, "guard_break", 0.9),
            SimEvent::TorchTouched { .. } => {}
            SimEvent::BossDefeated => {}
            SimEvent::EnemyAlert { entity } => {
                let hound = enemies.get(entity).is_ok_and(|e| tuning.enemies[e.kind as usize].model == "hound");
                play(&mut commands, &sounds, if hound { "bark" } else { "creak" }, 0.8);
            }
            SimEvent::EnemyDied { pos, .. } => {
                play(&mut commands, &sounds, "slam", 0.45);
                spray(&mut commands, &sparks, &sparks.ember, pos + Vec3::Y * 0.6, 14, 1.2, seed, floor(pos), true);
            }
            SimEvent::EnemyVanished { pos, .. } => {
                spray(&mut commands, &sparks, &sparks.ember, pos + Vec3::Y * 0.3, 20, 0.8, seed, floor(pos), true);
            }
            SimEvent::PickedUp { .. } | SimEvent::LootPicked { .. } => play(&mut commands, &sounds, "pickup", 0.8),
            SimEvent::ItemUsed { entity, item } => {
                let at = transforms.get(entity).map_or(Vec3::ZERO, |t| t.translation()) + Vec3::Y * 1.1;
                match item {
                    Item::GoldenMoss | Item::VigorRoot => {
                        play(&mut commands, &sounds, "heal", 0.6);
                        burst(&mut commands, &sparks, &sparks.heal, at, 10, 1.4, seed, floor(at));
                    }
                    _ => {
                        play(&mut commands, &sounds, "fury", 0.35);
                        burst(&mut commands, &sparks, &sparks.ember, at, 14, 2.0, seed, floor(at));
                    }
                }
            }
            SimEvent::Kindled { checkpoint, .. } => {
                play(&mut commands, &sounds, "kindle", 1.0);
                let at = encounter::checkpoint_pos(&tuning, checkpoint as usize) + Vec3::Y * 1.9;
                spray(&mut commands, &sparks, &sparks.ember, at, 40, 2.5, seed, floor(at), true);
            }
            SimEvent::Fell { .. } => play(&mut commands, &sounds, "fall", 0.9),
            SimEvent::Rested { entity } => {
                play(&mut commands, &sounds, "heal", 0.9);
                if let Ok(t) = transforms.get(entity) {
                    burst(&mut commands, &sparks, &sparks.heal, t.translation() + Vec3::Y * 1.0, 24, 2.0, seed, floor(t.translation() + Vec3::Y * 1.0));
                }
            }
            SimEvent::Shockwave { pos, radius, boss } => {
                play(&mut commands, &sounds, "slam", 1.0);
                rig.shake = rig.shake.max(1.0);
                // In the boss's colour, like its warning circle.
                let m = sparks.boss.get(boss as usize);
                let (ring, debris) = m.map_or((&sparks.fury, &sparks.guard), |m| (&m.shock, &m.spark));
                commands.spawn((
                    Mesh3d(sparks.ring.clone()),
                    MeshMaterial3d(ring.clone()),
                    Transform::from_translation(pos + Vec3::Y * 0.08)
                        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::splat(0.3)),
                    Shock { radius, life: SHOCK_LIFE },
                ));
                // Burst of debris all around the circle.
                let n = (radius * 8.0) as usize;
                for i in 0..n {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    let p = pos + Vec3::new(a.cos(), 0.1, a.sin()) * radius * 0.85;
                    burst(&mut commands, &sparks, debris, p, 2, 3.0, seed.wrapping_add(i as u32 * 31), floor(p));
                }
            }
            SimEvent::SpellCast { pos, element, boss, volley } => {
                if first(volley) {
                    play(&mut commands, &sounds, if element == Element::Iron { "swing_heavy" } else { "kindle" }, 0.6);
                }
                if let Some(m) = sparks.spell_mats(element, boss) {
                    spray(&mut commands, &sparks, &m.spark, pos, 10, 2.0, seed, floor(pos), true);
                }
            }
            SimEvent::SpellFizzle { pos, element, boss } => {
                if let Some(m) = sparks.spell_mats(element, boss) {
                    burst(&mut commands, &sparks, &m.spark, pos, 12, 3.5, seed, floor(pos));
                }
            }
            SimEvent::SpellSplash { pos, radius, element, boss } => {
                play(&mut commands, &sounds, if element == Element::Iron { "guard" } else { "slam" }, 0.35);
                if let Some(m) = sparks.spell_mats(element, boss) {
                    let n = (radius * 8.0) as usize + 6;
                    burst(&mut commands, &sparks, &m.spark, pos + Vec3::Y * 0.15, n, 4.0, seed, floor(pos + Vec3::Y));
                }
            }
            SimEvent::Eruption { pos, radius, element, boss, volley } => {
                if first(volley ^ 0x8000_0000) {
                    play(&mut commands, &sounds, "slam", 0.55);
                }
                rig.shake = rig.shake.max(0.35);
                if let Some(m) = sparks.spell_mats(element, boss) {
                    let n = (radius * 6.0) as usize + 4;
                    spray(&mut commands, &sparks, &m.spark, pos + Vec3::Y * 0.3, n, 3.0, seed, floor(pos), true);
                    burst(&mut commands, &sparks, &m.spark, pos + Vec3::Y * 0.2, n, 5.0, seed ^ 0x5bd1, floor(pos));
                }
            }
            // New fighters: the camera snaps back behind the player.
            SimEvent::Respawned => rig.initialized = false,
        }
    }
}

/// Places the ground warning of the boss's next area attack (fixed outline, disc that
/// fills up until the impact, blinking as it approaches).
fn aoe_markers(
    tuning: Res<Tuning>,
    time: Res<Time>,
    bosses: Query<(Entity, &Boss, &Body, &Action)>,
    mut markers: Query<(&AoeMarker, &mut Transform, &mut Visibility, &MeshMaterial3d<Ps1Material>)>,
    mut mats: ResMut<Assets<Ps1Material>>,
    mut locked: Local<Option<(Entity, u32, Vec3)>>,
) {
    let tele = bosses.iter().find_map(|(e, boss, b, a)| {
        let (mut pos, r, k) = aoe_telegraph(b, a, &tuning)?;
        // Climbing jump: the circle is where it lands (on the gallery, or down in the yard).
        pos.y = boss.flight.map_or(pos.y, |(_, to)| to);
        Some((e, a.seq, boss.def(&tuning).color, (pos, r, k)))
    });
    // The spot is frozen as soon as it's shown: the circle no longer moves until the impact.
    let tele = tele.map(|(e, seq, color, (pos, r, k))| {
        let pos = match *locked {
            Some((le, ls, lp)) if le == e && ls == seq => lp,
            _ => {
                *locked = Some((e, seq, pos));
                pos
            }
        };
        (pos, r, k, color)
    });
    if tele.is_none() {
        *locked = None;
    }
    for (m, mut tf, mut vis, mat) in &mut markers {
        let want = if tele.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let Some((pos, r, k, color)) = tele else { continue };
        tf.translation.x = pos.x;
        tf.translation.y = pos.y + m.lift;
        tf.translation.z = pos.z;
        let s = if m.fill { r * k } else { r };
        tf.scale = Vec3::new(s, s, 1.0);
        if let Some(mut mat) = mats.get_mut(&mat.0) {
            let blink = if k > 0.7 { 0.5 + 0.5 * (time.elapsed_secs() * 18.0).sin() } else { 1.0 };
            let shade = if m.fill { 0.75 } else { 1.0 };
            let c = Color::srgb(color[0] * shade, color[1] * shade, color[2] * shade).to_linear();
            mat.params.base_color = Vec4::new(c.red, c.green, c.blue, if m.fill { 0.2 + 0.25 * k } else { 0.5 + 0.45 * blink });
        }
    }
}

fn update_shocks(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Shock, &mut Transform)>) {
    for (e, mut s, mut t) in &mut q {
        s.life -= time.delta_secs();
        if s.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = 1.0 - s.life / SHOCK_LIFE;
        let r = s.radius * (0.3 + 0.75 * k.sqrt());
        t.scale = Vec3::new(r, r, 1.0);
    }
}

fn update_slashes(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut SlashTrail, &mut Transform, &MeshMaterial3d<Ps1Material>)>,
    mut mats: ResMut<Assets<Ps1Material>>,
) {
    for (e, mut s, mut t, mat) in &mut q {
        s.life -= time.delta_secs();
        if s.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = 1.0 - s.life / SLASH_LIFE;
        t.scale = s.scale * (1.0 + 0.08 * k);
        if let Some(mut m) = mats.get_mut(&mat.0) {
            m.params.base_color.w = 0.9 * (1.0 - k * k);
        }
    }
}

fn update_particles(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut p, mut t) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.y -= p.gravity * dt;
        t.translation += p.vel * dt;
        if p.sway > 0.0 {
            let a = p.life * 2.3 + p.phase;
            t.translation += Vec3::new(a.sin(), 0.0, (a * 0.8).cos()) * p.sway * dt;
        }
        if t.translation.y < p.floor + 0.02 {
            if p.splash {
                commands.entity(e).despawn();
                continue;
            }
            t.translation.y = p.floor + 0.02;
            p.vel *= 0.4;
        }
        let k = p.life / p.max;
        t.scale = Vec3::splat(k.max(0.2));
    }
}

/// Height of a checkpoint brazier's embers above the ground (`tools/blender/arena.py`).
const COALS: f32 = 1.0;
/// Height of a boss torch's flame (`tools/blender/arena.py`).
pub const TORCH_FLAME: f32 = 1.7;
/// Fountain (`tools/blender/arena.py`): basin surface, upper bowl (radius, water
/// height) and spout.
const FOUNTAIN_WATER: f32 = 0.42;
const FOUNTAIN_BOWL: (f32, f32) = (0.78, 1.93);
const FOUNTAIN_SPOUT: f32 = 2.2;
/// Beyond this, the fog hides everything: no particles.
const AMBIENT_RANGE: f32 = 50.0;

#[derive(Clone, Copy)]
enum Ambient {
    /// Embers rising and dancing.
    Ember,
    /// Ash flakes, higher and slower.
    Ash,
    /// Fountain jet: it rises then falls back into the bowl.
    Jet,
    /// Water overflowing the bowl and raining down into the basin.
    Spill,
    /// Pale sparks swirling above an item to pick up (or dropped by an enemy).
    Wisp,
    /// Embers rising from a boss's lit torch, in its colour.
    TorchEmber(u8),
    /// Sparks swirling slowly above an extinguished torch (it can be rekindled), in its colour.
    TorchWisp(u8),
}

/// Ambient particles, emitted continuously (rate per second) near the camera: checkpoint
/// braziers (a trickle of embers until they're rekindled, then a column of embers
/// and ash visible from afar), items to pick up and loot, fountain.
#[allow(clippy::too_many_arguments)]
fn ambient(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<Tuning>,
    sparks: Res<SparkAssets>,
    rig: Res<CameraRig>,
    players: Query<&crate::sim::player::Player, With<crate::render::LocalPlayer>>,
    encounter: Res<crate::sim::encounter::Encounter>,
    loot: Query<&crate::sim::items::Loot>,
    mut acc: Local<Vec<f32>>,
    mut seed: Local<u32>,
) {
    use crate::sim::data::Prop;
    let t = &*tuning;
    let dt = time.delta_secs().min(0.1);
    let player = players.single().ok();
    let found = player.map_or(0, |p| p.found);
    let mut emitters: Vec<(Vec3, Ambient, f32)> = Vec::new();
    for i in 0..t.level.checkpoints.len() {
        let at = encounter::checkpoint_pos(t, i) + Vec3::Y * COALS;
        let lit = found & (1 << i) != 0;
        emitters.push((at, Ambient::Ember, if lit { 16.0 } else { 2.5 }));
        emitters.push((at, Ambient::Ash, if lit { 7.0 } else { 1.5 }));
    }
    let enc = encounter.into_inner();
    for i in 0..t.arenas.len() {
        let at = encounter::torch_pos(t, i) + Vec3::Y * TORCH_FLAME;
        if enc.is_defeated(i as u8) {
            emitters.push((at - Vec3::Y * 0.1, Ambient::TorchWisp(i as u8), 9.0));
        } else {
            emitters.push((at, Ambient::TorchEmber(i as u8), 14.0));
        }
    }
    let picked = player.map_or(u64::MAX, |p| p.picked);
    for i in (0..t.level.pickups.len()).filter(|i| picked & (1u64 << i) == 0) {
        emitters.push((encounter::pickup_pos(t, i) + Vec3::Y * 0.3, Ambient::Wisp, 7.0));
    }
    for l in &loot {
        emitters.push((l.pos + Vec3::Y * 0.3, Ambient::Wisp, 7.0));
    }
    for p in t.level.props.iter().filter(|p| p.kind == Prop::Fountain) {
        let y = world::floor_at(t, p.pos[0], p.pos[1], 0.0).unwrap_or(0.0);
        let at = Vec3::new(p.pos[0], y, p.pos[1]);
        emitters.push((at, Ambient::Jet, 28.0));
        emitters.push((at, Ambient::Spill, 45.0));
    }
    acc.resize(emitters.len(), 0.0);
    for (k, (pos, kind, rate)) in emitters.into_iter().enumerate() {
        if pos.distance(rig.focus) > AMBIENT_RANGE {
            acc[k] = 0.0;
            continue;
        }
        acc[k] += rate * dt;
        while acc[k] >= 1.0 {
            acc[k] -= 1.0;
            *seed = seed.wrapping_add(1);
            let s = *seed;
            // Local pseudo-random (purely visual), in [-1, 1].
            let h = |n: u32| {
                let x = s.wrapping_mul(747796405).wrapping_add(n.wrapping_mul(2891336453)) ^ (k as u32).wrapping_mul(1013904223);
                let x = (x ^ (x >> 15)).wrapping_mul(2246822519);
                ((x >> 9) & 0xffff) as f32 / 65535.0 * 2.0 - 1.0
            };
            let torch = |i: u8| sparks.torch.get(i as usize).unwrap_or(&sparks.glow);
            let (mat, start, mut p, scale) = match kind {
                Ambient::TorchEmber(i) => {
                    let start = pos + Vec3::new(h(1) * 0.12, 0.1, h(2) * 0.12);
                    let vel = Vec3::new(h(3) * 0.2, 0.9 + h(4).abs() * 0.8, h(5) * 0.2);
                    (torch(i), start, Particle::new(vel, 0.9 + h(6).abs() * 0.9, -0.3, -1000.0), 0.6 + h(7).abs() * 0.5)
                }
                Ambient::TorchWisp(i) => {
                    // Around the cold cup, on a small circle, they rise slowly, swirling.
                    let a = h(1) * std::f32::consts::PI;
                    let start = pos + Vec3::new(a.cos() * 0.22, h(2) * 0.08, a.sin() * 0.22);
                    let vel = Vec3::new(-a.sin() * 0.3, 0.3 + h(3).abs() * 0.35, a.cos() * 0.3);
                    (torch(i), start, Particle::new(vel, 1.2 + h(4).abs() * 0.9, -0.05, -1000.0), 0.5 + h(5).abs() * 0.5)
                }
                Ambient::Ember => {
                    let start = pos + Vec3::new(h(1) * 0.3, 0.05, h(2) * 0.3);
                    let vel = Vec3::new(h(3) * 0.25, 0.9 + h(4).abs() * 0.9, h(5) * 0.25);
                    (&sparks.glow, start, Particle::new(vel, 1.6 + h(6).abs() * 1.6, -0.35, -1000.0), 0.6 + h(7).abs() * 0.6)
                }
                Ambient::Ash => {
                    let start = pos + Vec3::new(h(1) * 0.4, 0.4 + h(2).abs() * 0.8, h(3) * 0.4);
                    let vel = Vec3::new(h(4) * 0.2 + 0.12, 0.45 + h(5).abs() * 0.4, h(6) * 0.2);
                    (&sparks.ash, start, Particle::new(vel, 3.0 + h(7).abs() * 2.5, -0.05, -1000.0), 0.8 + h(8).abs() * 0.7)
                }
                Ambient::Jet => {
                    let start = pos + Vec3::Y * FOUNTAIN_SPOUT;
                    let vel = Vec3::new(h(1) * 0.35, 2.6 + h(2).abs() * 0.6, h(3) * 0.35);
                    (&sparks.water, start, Particle::new(vel, 1.2, 9.0, pos.y + FOUNTAIN_BOWL.1), 0.8 + h(4).abs() * 0.5)
                }
                Ambient::Wisp => {
                    // Around the glow, on a small circle, they rise swirling.
                    let a = h(1) * std::f32::consts::PI;
                    let start = pos + Vec3::new(a.cos() * 0.18, h(2) * 0.1, a.sin() * 0.18);
                    let vel = Vec3::new(-a.sin() * 0.25, 0.45 + h(3).abs() * 0.4, a.cos() * 0.25);
                    (&sparks.wisp, start, Particle::new(vel, 1.0 + h(4).abs() * 0.8, -0.1, -1000.0), 0.5 + h(5).abs() * 0.5)
                }
                Ambient::Spill => {
                    let a = h(1) * std::f32::consts::PI;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let start = pos + out * FOUNTAIN_BOWL.0 + Vec3::Y * (FOUNTAIN_BOWL.1 - 0.02);
                    let vel = out * (0.35 + h(2).abs() * 0.35) + Vec3::Y * h(3).abs() * 0.15;
                    (&sparks.water, start, Particle::new(vel, 1.2, 9.0, pos.y + FOUNTAIN_WATER), 0.6 + h(4).abs() * 0.5)
                }
            };
            match kind {
                Ambient::Ember | Ambient::Wisp | Ambient::TorchEmber(_) | Ambient::TorchWisp(_) => (p.sway, p.phase) = (0.35, h(9) * 3.0),
                Ambient::Ash => (p.sway, p.phase) = (0.5, h(9) * 3.0),
                Ambient::Jet | Ambient::Spill => p.splash = true,
            }
            commands.spawn((
                Mesh3d(sparks.mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(start).with_scale(Vec3::splat(scale)),
                p,
            ));
        }
    }
}

/// Height of an eruption column, according to its radius.
const COLUMN_HEIGHT: f32 = 3.2;

/// Spells: glowing projectiles and their trail; eruptions announced by a ground circle that
/// fills up, then a column that bursts out and falls back.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn spell_visuals(
    mut commands: Commands,
    tuning: Res<Tuning>,
    time: Res<Time>,
    clock: Res<crate::render::AnimClock>,
    sparks: Res<SparkAssets>,
    mut mats: ResMut<Assets<Ps1Material>>,
    mut spells: Query<(Entity, &Spell, Option<&mut SpellVisual>)>,
    mut parts: Query<(&mut Transform, &mut Visibility, Option<&MeshMaterial3d<Ps1Material>>), Without<Spell>>,
    mut roots: Query<&mut Transform, With<Spell>>,
) {
    let t = &*tuning;
    // Spells stop when paused (their sparks don't).
    let over = clock.over;
    let dt = time.delta_secs();
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    for (e, s, vis) in &mut spells {
        let sd = s.def(t);
        let Some(m) = sparks.spell_mats(sd.element, s.boss) else { continue };
        let pos = s.prev.lerp(s.pos, over);
        let Some(mut vis) = vis else {
            // New spell: its rendering.
            let mut ec = commands.entity(e);
            ec.insert((Transform::from_translation(pos), Visibility::default()));
            let mut v = SpellVisual { ring: None, fill: None, column: None, inner: None, trail: 0.0 };
            match sd.kind {
                SpellKind::Bolt => {
                    let core = commands
                        .spawn((Mesh3d(sparks.orb.clone()), MeshMaterial3d(m.core.clone()), Transform::from_scale(Vec3::splat(sd.radius * 0.55)), ChildOf(e)))
                        .id();
                    let outer = if sd.element != Element::Iron {
                        commands
                            .spawn((Mesh3d(sparks.orb.clone()), MeshMaterial3d(m.halo.clone()), Transform::from_scale(Vec3::splat(sd.radius * 1.05)), ChildOf(e)))
                            .id()
                    } else {
                        // The cleaver: a flat spinning blade.
                        commands
                            .spawn((
                                Mesh3d(sparks.mesh.clone()),
                                MeshMaterial3d(m.core.clone()),
                                Transform::from_scale(Vec3::new(sd.radius * 30.0, 2.0, sd.radius * 14.0)),
                                ChildOf(e),
                            ))
                            .id()
                    };
                    // The puddle burning where it crashes (hidden in flight).
                    let pool = commands
                        .spawn((
                            Mesh3d(sparks.disc.clone()),
                            MeshMaterial3d(m.halo.clone()),
                            Transform::from_translation(Vec3::Y * 0.05).with_rotation(flat).with_scale(Vec3::new(0.01, 0.01, 1.0)),
                            Visibility::Hidden,
                            ChildOf(e),
                        ))
                        .id();
                    (v.inner, v.column, v.fill) = (Some(core), Some(outer), Some(pool));
                }
                SpellKind::Beam => {
                    let mut col = |mat: &Handle<Ps1Material>| commands.spawn((Mesh3d(sparks.column.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::ZERO), ChildOf(e))).id();
                    (v.column, v.inner) = (Some(col(&m.halo)), Some(col(&m.core)));
                }
                SpellKind::Eruption => {
                    // The ground warning takes the casting boss's colour (like the column).
                    let [r, g, b] = t.bosses[s.boss as usize].color;
                    let warn = Color::srgba(r, g, b, 0.9);
                    let blend = |c: Color| {
                        let mut mat = Ps1Material::unlit(c);
                        mat.alpha_mode = AlphaMode::Blend;
                        mat
                    };
                    let ring = commands
                        .spawn((
                            Mesh3d(sparks.ring.clone()),
                            MeshMaterial3d(mats.add(blend(warn))),
                            Transform::from_translation(Vec3::Y * 0.06).with_rotation(flat).with_scale(Vec3::new(sd.radius, sd.radius, 1.0)),
                            ChildOf(e),
                        ))
                        .id();
                    let fill = commands
                        .spawn((
                            Mesh3d(sparks.disc.clone()),
                            MeshMaterial3d(mats.add(blend(warn.with_alpha(0.3)))),
                            Transform::from_translation(Vec3::Y * 0.05).with_rotation(flat).with_scale(Vec3::new(0.01, 0.01, 1.0)),
                            ChildOf(e),
                        ))
                        .id();
                    let mut col = |mat: &Handle<Ps1Material>| {
                        commands
                            .spawn((
                                Mesh3d(sparks.column.clone()),
                                MeshMaterial3d(mat.clone()),
                                Transform::from_scale(Vec3::ZERO),
                                Visibility::Hidden,
                                ChildOf(e),
                            ))
                            .id()
                    };
                    let (column, inner) = (col(&m.halo), col(&m.core));
                    (v.ring, v.fill, v.column, v.inner) = (Some(ring), Some(fill), Some(column), Some(inner));
                }
            }
            commands.entity(e).insert(v);
            continue;
        };
        if let Ok(mut tf) = roots.get_mut(e) {
            tf.translation = pos;
            if sd.kind == SpellKind::Bolt && sd.delay > 0 {
                // Projectile hanging before launching: it grows on the spot.
                let grow = ((s.age as f32 + over) / (sd.delay as f32 * 0.6)).clamp(0.05, 1.0);
                tf.scale = Vec3::splat(grow);
            }
            if sd.kind == SpellKind::Bolt && s.landed.is_none() {
                // The cleaver spins, the fireballs roll.
                tf.rotate_y(clock.dt * if sd.element == Element::Iron { 22.0 } else { 6.0 });
            }
        }
        match sd.kind {
            SpellKind::Bolt if s.landed.is_some() => {
                // Crashed on the ground: the projectile disappears, a puddle burns then dies out.
                for id in [vis.inner, vis.column].into_iter().flatten() {
                    if let Ok((_, mut v, _)) = parts.get_mut(id) {
                        *v = Visibility::Hidden;
                    }
                }
                let since = s.age as f32 + over - s.landed.unwrap_or(0) as f32;
                let k = (since / SPLASH_LIFE as f32).clamp(0.0, 1.0);
                if let Some(Ok((mut tf, mut v, _))) = vis.fill.map(|id| parts.get_mut(id)) {
                    *v = Visibility::Inherited;
                    let flicker = 1.0 + 0.06 * (time.elapsed_secs() * 23.0 + e.index_u32() as f32).sin();
                    let r = splash_radius(sd) * (since / 4.0).min(1.0) * (1.0 - 0.35 * k * k) * flicker;
                    tf.scale = Vec3::new(r.max(0.01), r.max(0.01), 1.0);
                }
                vis.trail += dt * 30.0 * (1.0 - k);
                let n = vis.trail as usize;
                vis.trail -= n as f32;
                let seed = (time.elapsed_secs() * 977.0) as u32 ^ e.index_u32();
                let r = splash_radius(sd);
                for i in 0..n {
                    let h = |j: u32| ((seed.wrapping_mul(2654435761).wrapping_add(i as u32 * 40503 + j * 7919) >> 8) % 1000) as f32 / 1000.0;
                    let a = h(1) * std::f32::consts::TAU;
                    let at = pos + Vec3::new(a.cos(), 0.1, a.sin()) * r * h(2).sqrt();
                    spray(&mut commands, &sparks, &m.spark, at, 1, 1.2, seed.wrapping_add(i as u32 * 31), -1000.0, true);
                }
            }
            SpellKind::Bolt => {
                vis.trail += dt * 40.0;
                let n = vis.trail as usize;
                vis.trail -= n as f32;
                let seed = (time.elapsed_secs() * 977.0) as u32 ^ e.index_u32();
                if n > 0 && sd.element != Element::Iron {
                    spray(&mut commands, &sparks, &m.spark, pos, n, 0.6, seed, -1000.0, true);
                }
            }
            SpellKind::Beam => {
                // A thick stream from the mouth to the ground, spreading there in bursts.
                let d = s.end - pos;
                let len = d.length().max(0.01);
                let rot = Quat::from_rotation_arc(Vec3::Y, d / len);
                let grow = ((s.age as f32 + over) / 6.0).min(1.0);
                let fade = ((sd.life as f32 - s.age as f32) / 8.0).clamp(0.0, 1.0);
                let flicker = 1.0 + 0.12 * (time.elapsed_secs() * 31.0 + e.index_u32() as f32).sin();
                for (id, k) in [(vis.column, 1.0), (vis.inner, 0.45)] {
                    let Some(Ok((mut tf, ..))) = id.map(|c| parts.get_mut(c)) else { continue };
                    let r = sd.radius * k * flicker * grow * fade;
                    let l = len * grow;
                    *tf = Transform { translation: d / len * l * 0.5, rotation: rot, scale: Vec3::new(r, l, r) };
                }
                vis.trail += dt * 90.0;
                let n = vis.trail as usize;
                vis.trail -= n as f32;
                let seed = (time.elapsed_secs() * 977.0) as u32 ^ e.index_u32();
                for i in 0..n {
                    let k = 0.35 + 0.65 * ((seed.wrapping_mul(2654435761).wrapping_add(i as u32 * 40503) >> 8) % 1000) as f32 / 1000.0;
                    let at = pos + d * k * grow;
                    spray(&mut commands, &sparks, &m.spark, at, 1, 2.5, seed.wrapping_add(i as u32 * 7919), -1000.0, true);
                }
            }
            SpellKind::Eruption => {
                let warn = telegraph(s, t);
                let k = warn.map_or(1.0, |w| (w.2 + over / s.burst_at(t).max(1) as f32).min(1.0));
                let blink = if k > 0.75 { 0.5 + 0.5 * (time.elapsed_secs() * 20.0).sin() } else { 1.0 };
                for (id, fill) in [(vis.ring, false), (vis.fill, true)] {
                    let Some(id) = id else { continue };
                    let Ok((mut tf, mut v, mat)) = parts.get_mut(id) else { continue };
                    let want = if warn.is_some() { Visibility::Inherited } else { Visibility::Hidden };
                    if *v != want {
                        *v = want;
                    }
                    if fill {
                        let r = sd.radius * k;
                        tf.scale = Vec3::new(r.max(0.01), r.max(0.01), 1.0);
                    }
                    if let Some(mut mm) = mat.and_then(|h| mats.get_mut(&h.0)) {
                        mm.params.base_color.w = if fill { 0.15 + 0.25 * k } else { 0.45 + 0.5 * blink };
                    }
                }
                let since = s.age as f32 + over - s.burst_at(t) as f32;
                if since < 0.0 {
                    continue;
                }
                // Bursts out at once, then thins and falls back; the core, thinner, rises higher.
                let life = sd.life.max(1) as f32;
                let f = (since / life).clamp(0.0, 1.0);
                let h = COLUMN_HEIGHT * (sd.radius * 0.6 + 0.5) * (1.0 - f * f) * (since / 3.0).min(1.0);
                let r = sd.radius * 0.8 * (1.0 - 0.6 * f);
                for (id, k) in [(vis.column, 1.0), (vis.inner, 0.45)] {
                    let Some(Ok((mut tf, mut v, _))) = id.map(|c| parts.get_mut(c)) else { continue };
                    if *v != Visibility::Inherited {
                        *v = Visibility::Inherited;
                    }
                    let hh = h * (1.0 + (1.0 - k) * 0.4);
                    tf.scale = Vec3::new(r * k, hh.max(0.01), r * k);
                    tf.translation = Vec3::Y * hh * 0.5;
                }
            }
        }
    }
}
