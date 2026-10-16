//! Third-person camera, with target lock-on and shake.

use bevy::prelude::*;

use super::ps1::WorldCamera;
use super::{Interp, LocalPlayer};
use crate::input::LookInput;
use crate::sim::data::Tuning;
use crate::sim::boss::{Boss, lock_points};
use crate::sim::fighter::{Body, Foe};
use crate::sim::player::Player;
use crate::sim::{math, world};

/// Piece of a boss model that carries a lockable point (`PartDef::bone`).
#[derive(Component, Clone, Copy)]
pub struct PartBone {
    pub owner: Entity,
    pub part: u8,
}

pub type LockFoes<'w, 's> = Query<'w, 's, (&'static Interp, &'static Body, Option<&'static Boss>), With<Foe>>;
pub type PartBones<'w, 's> = Query<'w, 's, (&'static PartBone, &'static GlobalTransform)>;

/// Displayed position of the locked point `part` of `e`: the model's animated piece if it
/// exists, otherwise the simulation point, at the interpolated position.
pub fn lock_target(t: &Tuning, e: Entity, part: u8, foes: &LockFoes, bones: &PartBones) -> Option<Vec3> {
    if let Some((_, gt)) = bones.iter().find(|(b, _)| b.owner == e && b.part == part) {
        return Some(gt.translation());
    }
    let (i, b, boss) = foes.get(e).ok()?;
    let at = Body { pos: i.pos, yaw: i.yaw, ..*b };
    let pts = lock_points(t, &at, boss);
    pts.get(part as usize).or(pts.first()).copied()
}

#[derive(Resource, Clone, Debug)]
pub struct CameraRig {
    /// Horizontal yaw of the camera (same convention as the sim: 0 = facing +Z).
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub shake: f32,
    pub focus: Vec3,
    /// Share of the distance allowed by the walls (the camera snaps closer when a wall
    /// gets in the way, and only backs off gradually).
    pub reach: f32,
    pub initialized: bool,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.3, distance: 5.8, shake: 0.0, focus: Vec3::ZERO, reach: 1.0, initialized: false }
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>()
            .add_systems(PostUpdate, update_camera.before(TransformSystems::Propagate));
    }
}

#[allow(clippy::too_many_arguments)]
fn update_camera(
    time: Res<Time>,
    tuning: Res<Tuning>,
    look: Res<LookInput>,
    mut rig: ResMut<CameraRig>,
    players: Query<(&Player, &Interp), With<LocalPlayer>>,
    others: Query<(&Player, &Interp, &crate::sim::fighter::Health), Without<LocalPlayer>>,
    foes: LockFoes,
    bones: PartBones,
    mut cam: Single<&mut Transform, With<WorldCamera>>,
    settings: Res<crate::settings::Settings>,
) {
    let dt = time.delta_secs();
    let Ok((me, mi)) = players.single() else { return };
    // Co-op: fallen during a boss fight, they watch a teammate who fights on.
    let waiting = me.state == crate::sim::player::PState::Dead && me.dead_ticks > crate::sim::encounter::RESPAWN_TICKS;
    let watched = others.iter().find(|(p, _, h)| waiting && !h.dead() && p.zone == me.zone);
    let (player, pi) = watched.map_or((me, mi), |(p, i, _)| (p, i));
    let lock = player
        .lock
        .and_then(|e| Some((lock_target(&tuning, e, player.lock_part, &foes, &bones)?, foes.get(e).ok()?.1)));

    let fresh = !rig.initialized;
    if fresh {
        rig.yaw = pi.yaw;
        rig.focus = pi.pos;
        rig.initialized = true;
    }

    if let Some((point, _)) = lock {
        // Locked on: the camera goes behind the player, facing the locked point.
        let want = math::yaw_of(point - pi.pos);
        let d = math::wrap(want - rig.yaw);
        rig.yaw = math::wrap(rig.yaw + d * (1.0 - (-8.0 * dt).exp()));
        let dist = math::flat_len(point - pi.pos).max(1.0);
        // The higher the point (a dragon's head), the more the camera dives… upwards.
        let rise = ((point.y - pi.pos.y - 1.7) / dist).atan();
        let target_pitch = (0.22 - rise * 0.6 - dist * 0.004).clamp(-0.2, 0.4);
        rig.pitch += (target_pitch - rig.pitch) * (1.0 - (-4.0 * dt).exp());
    } else {
        rig.yaw = math::wrap(rig.yaw - look.delta.x);
        rig.pitch = (rig.pitch + look.delta.y).clamp(-0.35, 1.1);
    }

    let mut target = pi.pos + Vec3::Y * 1.7;
    // Fall: the camera stays at the edge and watches the body vanish into the black.
    if player.falling {
        target.y = target.y.max(player.fall_from + 0.4);
    }
    // Jump: the camera only follows part of the height (less shaking).
    if player.airborne {
        target.y = player.air_from + 1.7 + (pi.pos.y - player.air_from) * 0.4;
    }
    rig.focus = rig.focus.lerp(target, 1.0 - (-14.0 * dt).exp());
    let back = -math::forward(rig.yaw);
    let horiz = rig.pitch.cos() * rig.distance;
    let mut pos = rig.focus + back * horiz + Vec3::Y * (rig.pitch.sin() * rig.distance + 0.25);
    let ground = if player.falling {
        player.fall_from
    } else if player.airborne {
        player.air_from.min(pi.pos.y)
    } else {
        pi.pos.y
    };
    pos.y = pos.y.max(ground + 0.4);
    // A wall (arena, stairs) between the player and the camera would hide it: it moves in front.
    // Elsewhere, nothing stops it: all around is the void.
    let offset = pos - rig.focus;
    let len = math::flat_len(offset).max(0.01);
    let allowed = if player.falling {
        1.0
    } else {
        world::wall_hit(&tuning, rig.focus, pos).map_or(1.0, |s| (s - 0.3 / len).max(0.08))
    };
    rig.reach = if allowed < rig.reach { allowed } else { rig.reach + (allowed - rig.reach) * (1.0 - (-5.0 * dt).exp()) };
    if fresh {
        rig.reach = allowed;
    }
    pos = rig.focus + Vec3::new(offset.x * rig.reach, offset.y, offset.z * rig.reach);
    let mut look_at = rig.focus;
    if let Some((point, _)) = lock {
        look_at = rig.focus.lerp(point, 0.35);
    }

    rig.shake = (rig.shake - dt * 2.5).max(0.0);
    let t = time.elapsed_secs();
    let s = if settings.camera_shake { rig.shake * rig.shake * 0.25 } else { 0.0 };
    let jitter = Vec3::new((t * 71.0).sin(), (t * 53.0).cos(), (t * 61.0).sin()) * s;

    **cam = Transform::from_translation(pos + jitter).looking_at(look_at, Vec3::Y);
}
