//! PS1-style souls-like combat prototype.
//!
//! - `sim`: deterministic simulation at 60 ticks/s (no dependency on rendering);
//! - `render`, `fx`, `hud`: presentation, which only reads the simulation;
//! - `input`: devices → `PlayerInput`;
//! - `net`: online co-op (peer-to-peer rollback).

pub mod config;
pub mod debug;
pub mod fx;
pub mod hud;
pub mod input;
pub mod lang;
pub mod menu;
pub mod net;
pub mod render;
pub mod save;
pub mod settings;
pub mod sim;
pub mod storage;
pub mod ui;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::window::WindowResolution;

use render::AppState;

pub fn run() {
    // Settings loaded before creating the window: start directly in the right mode.
    let settings = settings::Settings::load();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "PSX Souls".into(),
                    canvas: Some("#bevy".into()),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    resolution: {
                        let (w, h) = settings.resolution.unwrap_or((1280, 720));
                        WindowResolution::new(w, h)
                    },
                    // The exact exclusive mode is applied once the monitor is known.
                    mode: settings.window_mode(None),
                    present_mode: settings.present_mode(),
                    ..default()
                }),
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            .set(AssetPlugin {
                // No .meta files: avoids 404 requests on the web.
                meta_check: AssetMetaCheck::Never,
                ..default()
            }),
    )
    .insert_resource(settings)
    .insert_resource(Time::<Fixed>::from_hz(sim::TICK_HZ))
    .init_state::<AppState>()
    .add_plugins((
        settings::SettingsPlugin,
        lang::LangPlugin,
        ui::UiPlugin,
        menu::MenuPlugin,
        sim::SimPlugin,
        config::ConfigPlugin,
        input::InputPlugin,
        render::RenderPlugin,
        fx::FxPlugin,
        hud::HudPlugin,
        save::SavePlugin,
        net::NetPlugin,
        debug::DebugPlugin,
        debug::AutoShotPlugin,
    ))
    .add_systems(
        FixedUpdate,
        (
            input::collect_local_input,
            // Solo: the menu pauses the game.
            (input::feed_solo, sim::run_sim_tick).chain().run_if(net::solo.and_then(menu::menu_closed)),
            // Co-op: the rollback session decides which ticks to simulate.
            net::coop_tick.run_if(net::coop_running),
        )
            .chain()
            .run_if(in_state(AppState::Playing)),
    );
    app.run();
}
