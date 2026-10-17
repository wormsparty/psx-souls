//! Items, inventory and quick slots (Lies of P style): the items equipped in the
//! slots are selected in game (D-pad down / C) and used with a single button.
//!
//! Three families, as in souls-likes:
//! - consumables (quick slots): healing flask (refilled on rest), and temporary boosts
//!   (moss, resin, ash, root) that enemies drop at random (`Loot`);
//! - talismans (a dedicated slot): permanent bonuses while worn;
//! - key items, applied as soon as they're picked up (flask shard: one more charge).

use bevy::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};

use super::data::Tuning;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Item {
    HealFlask,
    /// Golden moss: regenerates HP for a few seconds.
    GoldenMoss,
    /// Ember resin: the weapon deals more damage for a minute.
    EmberResin,
    /// Warding ash: reduced damage taken for a while.
    WardingAsh,
    /// Vigor root: stamina recovers faster for a while.
    VigorRoot,
    /// Flask shard: one more healing charge, as soon as it's picked up.
    FlaskShard,
    /// Talisman: reduced damage taken.
    IronBrooch,
    /// Talisman: dodges cost less stamina.
    CrestPlume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Consumable,
    Talisman,
    Key,
}

/// Golden moss: HP restored per second, and duration (ticks).
pub const MOSS_HP_PER_SEC: f32 = 6.0;
pub const MOSS_TICKS: u32 = 25 * 60;
/// Ember resin: damage bonus and duration (ticks).
pub const RESIN_DAMAGE: f32 = 1.2;
pub const RESIN_TICKS: u32 = 60 * 60;
/// Warding ash: share of the damage taken, and duration (ticks).
pub const ASH_DAMAGE: f32 = 0.6;
pub const ASH_TICKS: u32 = 30 * 60;
/// Vigor root: stamina regeneration multiplier, and duration (ticks).
pub const ROOT_REGEN: f32 = 1.75;
pub const ROOT_TICKS: u32 = 30 * 60;
/// Iron brooch: share of the damage taken.
pub const BROOCH_DAMAGE: f32 = 0.85;
/// Crest plume: share of the stamina cost of dodges.
pub const FEATHER_DODGE: f32 = 0.7;

impl Item {
    pub const ALL: [Item; 8] = [
        Item::HealFlask,
        Item::GoldenMoss,
        Item::EmberResin,
        Item::WardingAsh,
        Item::VigorRoot,
        Item::FlaskShard,
        Item::IronBrooch,
        Item::CrestPlume,
    ];

    pub fn kind(self) -> Kind {
        match self {
            Item::IronBrooch | Item::CrestPlume => Kind::Talisman,
            Item::FlaskShard => Kind::Key,
            _ => Kind::Consumable,
        }
    }

    pub fn name(self) -> &'static str {
        use crate::lang::tr;
        match self {
            Item::HealFlask => tr("Healing Flask", "Fiole de soin"),
            Item::GoldenMoss => tr("Golden Moss", "Mousse dorée"),
            Item::EmberResin => tr("Ember Resin", "Résine ardente"),
            Item::WardingAsh => tr("Warding Ash", "Cendre protectrice"),
            Item::VigorRoot => tr("Vigor Root", "Racine de vigueur"),
            Item::FlaskShard => tr("Flask Shard", "Éclat de fiole"),
            Item::IronBrooch => tr("Iron Brooch", "Broche de fer"),
            Item::CrestPlume => tr("Crest Plume", "Plume de cimier"),
        }
    }

    /// Short description (picked-up item popup, equipment menu).
    pub fn description(self) -> &'static str {
        use crate::lang::tr;
        match self {
            Item::HealFlask => tr("Restores HP. Refilled when resting.", "Rend des PV. Remplie au repos."),
            Item::GoldenMoss => tr("Slowly restores HP for a while.", "Rend lentement des PV pendant un moment."),
            Item::EmberResin => tr("Weapon deals more damage for a minute.", "L'arme frappe plus fort pendant une minute."),
            Item::WardingAsh => tr("Damage taken reduced by 40% for 30 s.", "Dégâts subis réduits de 40 % pendant 30 s."),
            Item::VigorRoot => tr("Stamina recovers 75% faster for 30 s.", "L'endurance revient 75 % plus vite pendant 30 s."),
            Item::FlaskShard => tr("Healing Flask: one more charge.", "Fiole de soin : une charge de plus."),
            Item::IronBrooch => tr("Talisman. Damage taken reduced by 15%.", "Talisman. Dégâts subis réduits de 15 %."),
            Item::CrestPlume => tr("Talisman. Dodging costs 30% less stamina.", "Talisman. Esquiver coûte 30 % d'endurance en moins."),
        }
    }
}

pub const QUICK_SLOTS: usize = 4;

/// Item dropped by a defeated enemy (`EnemyDef::loot`), glowing on the ground where its body
/// vanished. It stays there until it's picked up or the world is reset (death, travel).
///
/// Co-op: each player rolls their own drop, which only they see and can pick up (`owner`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Loot {
    pub pos: Vec3,
    pub item: Item,
    /// The player it's for (`Player::id`).
    pub owner: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Inventory {
    /// Items owned and their quantity (one entry per item, in order of acquisition).
    #[serde(deserialize_with = "saved_items")]
    pub items: Vec<(Item, u8)>,
    #[serde(deserialize_with = "saved_slots")]
    pub slots: [Option<Item>; QUICK_SLOTS],
    /// Selected quick slot.
    pub active: u8,
    /// Worn talisman.
    pub talisman: Option<Item>,
    /// Healing charges gained from flask shards.
    pub flask_bonus: u8,
}

/// Item read from a save: `None` if it no longer exists (old saves: the embers to crush),
/// rather than making the whole save unreadable.
struct SavedItem(Option<Item>);

impl<'de> Deserialize<'de> for SavedItem {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{DeserializeSeed, EnumAccess, IntoDeserializer, VariantAccess, Visitor, value::Error};
        /// The variant's name, whatever it is.
        struct Name;
        impl<'de> Visitor<'de> for Name {
            type Value = String;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an item name")
            }
            fn visit_str<E>(self, v: &str) -> Result<String, E> {
                Ok(v.to_owned())
            }
        }
        impl<'de> DeserializeSeed<'de> for Name {
            type Value = String;
            fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
                d.deserialize_identifier(self)
            }
        }
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = SavedItem;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an item")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<SavedItem, A::Error> {
                let (name, variant) = data.variant_seed(Name)?;
                variant.unit_variant()?;
                Ok(SavedItem(Item::deserialize(IntoDeserializer::<Error>::into_deserializer(name)).ok()))
            }
        }
        d.deserialize_enum("Item", &[], V)
    }
}

impl SavedItem {
    fn get(self) -> Option<Item> {
        self.0
    }
}

fn saved_items<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(Item, u8)>, D::Error> {
    let v = Vec::<(SavedItem, u8)>::deserialize(d)?;
    Ok(v.into_iter().filter_map(|(i, n)| Some((i.get()?, n))).collect())
}

fn saved_slots<'de, D: Deserializer<'de>>(d: D) -> Result<[Option<Item>; QUICK_SLOTS], D::Error> {
    let v = <[Option<SavedItem>; QUICK_SLOTS]>::deserialize(d)?;
    Ok(v.map(|s| s.and_then(SavedItem::get)))
}

impl Default for Inventory {
    fn default() -> Self {
        Self { items: Vec::new(), slots: [None; QUICK_SLOTS], active: 0, talisman: None, flask_bonus: 0 }
    }
}

impl Inventory {
    pub fn new_game(t: &Tuning) -> Self {
        let mut inv = Self::default();
        inv.items.push((Item::HealFlask, t.player.heal_charges));
        inv.slots[0] = Some(Item::HealFlask);
        inv
    }

    pub fn count(&self, item: Item) -> u8 {
        self.items.iter().find(|(i, _)| *i == item).map_or(0, |(_, n)| *n)
    }

    pub fn owns(&self, item: Item) -> bool {
        self.items.iter().any(|(i, _)| *i == item)
    }

    pub fn active_item(&self) -> Option<Item> {
        self.slots.get(self.active as usize).copied().flatten()
    }

    /// Quantity restored on rest and on death (`None`: the item doesn't refill).
    pub fn refill_amount(&self, item: Item, t: &Tuning) -> Option<u8> {
        match item {
            Item::HealFlask => Some(t.player.heal_charges.saturating_add(self.flask_bonus)),
            _ => None,
        }
    }

    /// Adds picked-up items. Key items apply immediately; a new
    /// consumable goes into the first free quick slot, a talisman is worn if none
    /// was worn.
    pub fn add(&mut self, item: Item, n: u8) {
        match item.kind() {
            Kind::Key => {
                if item == Item::FlaskShard {
                    self.flask_bonus = self.flask_bonus.saturating_add(n);
                    // The gained charge is usable immediately.
                    if let Some((_, c)) = self.items.iter_mut().find(|(i, _)| *i == Item::HealFlask) {
                        *c = c.saturating_add(n);
                    }
                }
                return;
            }
            Kind::Talisman | Kind::Consumable => {}
        }
        let new = !self.owns(item);
        match self.items.iter_mut().find(|(i, _)| *i == item) {
            Some((_, c)) => *c = c.saturating_add(n).min(99),
            None => self.items.push((item, n.min(99))),
        }
        if item.kind() == Kind::Talisman && self.talisman.is_none() {
            self.talisman = Some(item);
        }
        if new && item.kind() == Kind::Consumable && !self.slots.contains(&Some(item)) {
            if let Some(s) = self.slots.iter_mut().find(|s| s.is_none()) {
                *s = Some(item);
            }
            if self.active_item().is_none() {
                self.cycle();
            }
        }
    }

    /// Removes one; false if there are none left.
    pub fn consume(&mut self, item: Item) -> bool {
        match self.items.iter_mut().find(|(i, _)| *i == item) {
            Some((_, n)) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        }
    }

    /// Switches to the next equipped slot (does nothing if there's no other).
    pub fn cycle(&mut self) {
        for k in 1..=QUICK_SLOTS {
            let i = (self.active as usize + k) % QUICK_SLOTS;
            if self.slots[i].is_some() {
                self.active = i as u8;
                return;
            }
        }
    }

    /// Equips a consumable in a slot (it leaves its old slot if it had one).
    pub fn equip(&mut self, slot: usize, item: Option<Item>) {
        if slot >= QUICK_SLOTS || item.is_some_and(|i| !self.owns(i) || i.kind() != Kind::Consumable) {
            return;
        }
        if let Some(it) = item {
            for s in &mut self.slots {
                if *s == Some(it) {
                    *s = None;
                }
            }
        }
        self.slots[slot] = item;
        if self.active_item().is_none() {
            self.cycle();
        }
    }

    pub fn equip_talisman(&mut self, item: Option<Item>) {
        if item.is_none_or(|i| self.owns(i) && i.kind() == Kind::Talisman) {
            self.talisman = item;
        }
    }

    pub fn wears(&self, item: Item) -> bool {
        self.talisman == Some(item)
    }

    /// Refills the refillable items (rest at the checkpoint, death).
    pub fn refill(&mut self, t: &Tuning) {
        let amounts: Vec<Option<u8>> = self.items.iter().map(|(i, _)| self.refill_amount(*i, t)).collect();
        for ((_, n), max) in self.items.iter_mut().zip(amounts) {
            if let Some(max) = max {
                *n = max;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equip_cycle_and_consume() {
        let t = Tuning::builtin();
        let mut inv = Inventory::new_game(&t);
        assert_eq!(inv.active_item(), Some(Item::HealFlask));
        // A single equipped item: switching slots does nothing.
        inv.cycle();
        assert_eq!(inv.active, 0);
        // Moving the item to slot 3 frees slot 1 and follows the selection.
        inv.equip(2, Some(Item::HealFlask));
        assert_eq!(inv.slots, [None, None, Some(Item::HealFlask), None]);
        assert_eq!(inv.active, 2);
        while inv.consume(Item::HealFlask) {}
        assert_eq!(inv.count(Item::HealFlask), 0);
        inv.refill(&t);
        assert_eq!(inv.count(Item::HealFlask), t.player.heal_charges);
    }

    #[test]
    fn pickups_fill_slots_and_shards_add_charges() {
        let t = Tuning::builtin();
        let mut inv = Inventory::new_game(&t);
        inv.add(Item::GoldenMoss, 2);
        assert_eq!(inv.slots[1], Some(Item::GoldenMoss));
        inv.add(Item::GoldenMoss, 1);
        assert_eq!(inv.count(Item::GoldenMoss), 3);
        // A talisman doesn't go into the quick slots; the first one is worn automatically.
        inv.add(Item::IronBrooch, 1);
        assert!(!inv.slots.contains(&Some(Item::IronBrooch)));
        assert!(inv.wears(Item::IronBrooch));
        inv.equip(2, Some(Item::IronBrooch));
        assert_eq!(inv.slots[2], None);
        inv.add(Item::CrestPlume, 1);
        assert!(inv.wears(Item::IronBrooch));
        inv.equip_talisman(Some(Item::CrestPlume));
        assert!(inv.wears(Item::CrestPlume));
        // Flask shard: one more charge, kept on rest; consumables don't refill.
        inv.add(Item::FlaskShard, 1);
        assert!(!inv.owns(Item::FlaskShard));
        assert_eq!(inv.count(Item::HealFlask), t.player.heal_charges + 1);
        while inv.consume(Item::HealFlask) {}
        inv.consume(Item::GoldenMoss);
        inv.refill(&t);
        assert_eq!(inv.count(Item::HealFlask), t.player.heal_charges + 1);
        assert_eq!(inv.count(Item::GoldenMoss), 2);
    }
}
