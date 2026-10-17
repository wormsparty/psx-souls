//! Dark Souls-style autosave: a single slot, rewritten as soon as something important
//! happens (rest, entering the arena, victory, enemy defeated, item picked up
//! or used, fall, death, menu opened/closed), every 5 seconds,
//! and when quitting. The file is only
//! rewritten if it has changed. You resume where you left off — except mid boss fight:
//! you then come back in front of the fog, and the boss starts over.
//!
//! Co-op: the host saves as in solo (it's its world). A guest keeps their own save: their
//! character comes back from the host's world (weapon, inventory, discoveries, items picked up)
//! and the bosses defeated there count, but their position and checkpoint stay those of their
//! own world.
//!
//! Location: `save.ron` next to the settings (see `storage`).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::fx::FxState;
use crate::menu::MenuState;
use crate::render::AppState;
use crate::sim::SimEvent;
use crate::sim::data::Tuning;
use crate::sim::encounter::{Encounter, Progress};
use crate::sim::fighter::{Body, Health};
use crate::sim::player::Player;

pub const VERSION: u32 = 1;
const FILE: &str = "save";
const AUTOSAVE_SECS: f32 = 5.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SaveData {
    pub version: u32,
    /// Play time, in seconds.
    pub play_time: f64,
    pub progress: Progress,
}

#[derive(Resource, Default)]
pub struct SaveSlot {
    /// Last save read or written.
    pub data: Option<SaveData>,
    /// Writes nothing to disk (automatic screenshots).
    pub disabled: bool,
    pub play_time: f64,
    pending: bool,
    timer: f32,
    last_text: Option<String>,
}

impl SaveSlot {
    fn load() -> Self {
        let text = crate::storage::read(FILE);
        let data = text.as_deref().and_then(|t| match ron::from_str::<SaveData>(t) {
            Ok(d) if d.version == VERSION => Some(d),
            Ok(d) => {
                warn!("save version {} ignored (expected {VERSION})", d.version);
                None
            }
            Err(e) => {
                warn!("unreadable save, ignored: {e}");
                None
            }
        });
        Self { last_text: data.is_some().then_some(text).flatten(), data, ..default() }
    }

    /// Requests a write at the end of the frame.
    pub fn request(&mut self) {
        self.pending = true;
    }

    pub fn store(&mut self, data: SaveData) {
        self.pending = false;
        self.timer = 0.0;
        let text = match ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default()) {
            Ok(t) => t,
            Err(e) => {
                warn!("cannot serialise the save: {e}");
                return;
            }
        };
        self.data = Some(data);
        if self.disabled || self.last_text.as_deref() == Some(text.as_str()) {
            return;
        }
        crate::storage::write(FILE, &text);
        self.last_text = Some(text);
    }
}

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SaveSlot::load())
            .add_systems(Last, (autosave_triggers, flush).chain().run_if(in_state(AppState::Playing)));
    }
}

fn autosave_triggers(
    time: Res<Time>,
    fx: Res<FxState>,
    menu: Res<MenuState>,
    mut exit: MessageReader<AppExit>,
    mut slot: ResMut<SaveSlot>,
    mut was_open: Local<bool>,
) {
    let dt = time.delta_secs();
    slot.play_time += dt as f64;
    slot.timer += dt;
    let important = fx.last.iter().any(|e| {
        matches!(
            e,
            SimEvent::Rested { .. }
                | SimEvent::BossAwake
                | SimEvent::BossDefeated
                | SimEvent::BossRevived { .. }
                | SimEvent::Passage { .. }
                | SimEvent::PlayerDied { .. }
                | SimEvent::Revived { .. }
                | SimEvent::Respawned
                | SimEvent::Heal { .. }
                | SimEvent::WeaponSwitched { .. }
                | SimEvent::ItemCycled { .. }
                | SimEvent::ItemUsed { .. }
                | SimEvent::PickedUp { .. }
                | SimEvent::LootPicked { .. }
                | SimEvent::Kindled { .. }
                | SimEvent::EnemyDied { .. }
                | SimEvent::Fell { .. }
        )
    });
    let menu_toggled = menu.open != *was_open;
    *was_open = menu.open;
    if important || menu_toggled || slot.timer >= AUTOSAVE_SECS || exit.read().next().is_some() {
        slot.request();
    }
}

fn flush(world: &mut World) {
    if world.resource::<SaveSlot>().pending {
        save_now(world);
    }
}

/// Current progress of the local player (`None` outside a game). A co-op guest's: their own
/// save, with their character back from the host's world.
pub fn capture(world: &mut World) -> Option<Progress> {
    let enc = *world.resource::<Encounter>();
    let tuning = world.resource::<Tuning>().clone();
    let local = world.resource::<crate::net::LocalId>().0;
    let mut q = world.query::<(&Player, &Body, &Health)>();
    let progress = q.iter(world).find(|(p, ..)| p.id == local).map(|(p, b, h)| Progress::of_player(p, b, h, &enc, &tuning))?;
    let net = world.resource::<crate::net::Net>();
    Some(match (&net.guest_base, net.role) {
        (Some(base), crate::net::Role::Guest) => guest_progress(base, &progress),
        _ => progress,
    })
}

/// A guest's save after (or during) a co-op game: `base`, their own save when they joined,
/// with the character they play in the host's world (`coop`).
pub fn guest_progress(base: &Progress, coop: &Progress) -> Progress {
    Progress {
        defeated: base.defeated | coop.defeated,
        weapon: coop.weapon,
        inventory: coop.inventory.clone(),
        found: base.found | coop.found,
        picked: base.picked | coop.picked,
        slain: base.slain | coop.slain,
        ..base.clone()
    }
}

/// Writes the save right away (if a game is in progress).
pub fn save_now(world: &mut World) {
    let Some(progress) = capture(world) else { return };
    let mut slot = world.resource_mut::<SaveSlot>();
    let data = SaveData { version: VERSION, play_time: slot.play_time, progress };
    slot.store(data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::items::Item;

    #[test]
    fn save_roundtrip() {
        let t = Tuning::builtin();
        let mut progress = Progress::new_game(&t);
        progress.pos = Some([0.5, -20.0, 3.0]);
        let data = SaveData { version: VERSION, play_time: 75.0, progress };
        let txt = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<SaveData>(&txt).unwrap(), data);
    }

    /// A save from before the embers were removed still loads: the currency, the corpse and the
    /// embers to crush are dropped.
    #[test]
    fn old_save_with_embers_loads() {
        let txt = r#"(
            version: 1,
            play_time: 10.0,
            progress: (
                embers: 400,
                defeated: 1,
                weapon: 0,
                inventory: (
                    items: [(HealFlask, 3), (FadedEmber, 3), (EmberResin, 1)],
                    slots: (Some(HealFlask), Some(FadedEmber), None, Some(EmberResin)),
                    active: 1,
                    talisman: None,
                    flask_bonus: 0,
                ),
                hp: None,
                pos: None,
                checkpoint: 0,
                found: 1,
                picked: 3,
                slain: 0,
                dropped: Some((at: (1.0, 0.0, 2.0), embers: 100)),
            ),
        )"#;
        let data = ron::from_str::<SaveData>(txt).unwrap();
        let inv = &data.progress.inventory;
        assert_eq!(inv.items, vec![(Item::HealFlask, 3), (Item::EmberResin, 1)]);
        assert_eq!(inv.slots, [Some(Item::HealFlask), None, None, Some(Item::EmberResin)]);
        assert_eq!((data.progress.defeated, data.progress.picked), (1, 3));
    }
}
