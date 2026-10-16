//! Presentation: everything that reads the simulation to display it, without ever modifying it.

pub mod camera;
pub mod gates;
pub mod models;
pub mod preview;
pub mod ps1;

use bevy::prelude::*;

use crate::sim::fighter::{Body, PrevBody};
use crate::sim::math;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Loading,
    /// Title screen (no fighter in the world).
    Title,
    Playing,
}

/// Position/orientation interpolated between the last two simulation ticks.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Interp {
    pub pos: Vec3,
    pub yaw: f32,
}

/// Animation clock: time step and progress between two simulation ticks. Frozen
/// while the game is paused (menu open in solo: the simulation stops running); the
/// particles keep going in real time.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct AnimClock {
    pub dt: f32,
    /// Fraction of the current tick (0 → 1); 1 when paused (last simulation state).
    pub over: f32,
    pub paused: bool,
}

/// Player controlled on this machine (the camera and the HUD follow it).
#[derive(Component)]
pub struct LocalPlayer;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ps1::Ps1Plugin, camera::CameraPlugin, models::ModelsPlugin, gates::GatesPlugin, preview::PreviewPlugin))
            .init_resource::<AnimClock>()
            // After the fixed-step loop: the tick fraction is only correct once the frame's
            // ticks have run (in PreUpdate, it lags one tick every other frame and
            // the player jitters while running).
            .add_systems(RunFixedMainLoop, tick_anim_clock.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop))
            .add_systems(Update, interpolate.run_if(in_state(AppState::Playing)));
    }
}

fn tick_anim_clock(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    state: Res<State<AppState>>,
    menu: Res<crate::menu::MenuState>,
    net: Res<crate::net::Net>,
    mut clock: ResMut<AnimClock>,
) {
    let paused = *state.get() == AppState::Playing && menu.open && !net.coop();
    *clock = if paused {
        AnimClock { dt: 0.0, over: 1.0, paused }
    } else {
        AnimClock { dt: time.delta_secs(), over: fixed.overstep_fraction().clamp(0.0, 1.0), paused }
    };
}

fn interpolate(clock: Res<AnimClock>, mut q: Query<(&Body, &PrevBody, &mut Interp, &mut Transform)>) {
    let a = clock.over;
    for (b, p, mut i, mut t) in &mut q {
        i.pos = p.pos.lerp(b.pos, a);
        i.yaw = p.yaw + math::wrap(b.yaw - p.yaw) * a;
        t.translation = i.pos;
        t.rotation = Quat::from_rotation_y(i.yaw);
    }
}
