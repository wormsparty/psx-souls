//! Save states of the simulation, for network rollback (`crate::net`): everything a tick reads
//! and writes, copied, then put back when a late input proves a prediction wrong.
//!
//! Restoring keeps the entities that still exist (the presentation's models stay attached to
//! them), recreates the missing ones (with their `SimId`) and removes the extra ones. A recreated
//! entity gets a new `Entity`: the references between entities (lock-on, targets, hits…) are
//! translated.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::boss::Boss;
use super::encounter::{Encounter, SimCommands};
use super::enemy::Enemy;
use super::fighter::{Action, Body, Foe, Health, Hitstop, PrevBody, Team};
use super::items::Loot;
use super::player::Player;
use super::rng::SimRng;
use super::spell::Spell;
use super::{NextSimId, ResetFight, SimEntity, SimId, SimTick};

/// The whole simulation state at the end of a tick.
#[derive(Clone)]
pub struct Snapshot {
    tick: SimTick,
    rng: SimRng,
    encounter: Encounter,
    reset: ResetFight,
    next_id: NextSimId,
    /// In `SimId` order.
    entities: Vec<Saved>,
}

impl Snapshot {
    pub fn tick(&self) -> u32 {
        self.tick.0
    }
}

/// A simulation entity and all its components (the absent ones: `None`).
#[derive(Clone)]
struct Saved {
    entity: Entity,
    id: SimId,
    team: Option<Team>,
    foe: bool,
    body: Option<Body>,
    prev: Option<PrevBody>,
    health: Option<Health>,
    hitstop: Option<Hitstop>,
    action: Option<Action>,
    player: Option<Player>,
    boss: Option<Boss>,
    enemy: Option<Enemy>,
    spell: Option<Spell>,
    loot: Option<Loot>,
}

pub fn save(world: &mut World) -> Snapshot {
    let mut ids: Vec<(SimId, Entity)> = world.query_filtered::<(Entity, &SimId), With<SimEntity>>().iter(world).map(|(e, id)| (*id, e)).collect();
    ids.sort_unstable_by_key(|x| x.0);
    let entities = ids
        .into_iter()
        .map(|(id, entity)| {
            let e = world.entity(entity);
            Saved {
                entity,
                id,
                team: e.get::<Team>().copied(),
                foe: e.contains::<Foe>(),
                body: e.get::<Body>().copied(),
                prev: e.get::<PrevBody>().copied(),
                health: e.get::<Health>().copied(),
                hitstop: e.get::<Hitstop>().copied(),
                action: e.get::<Action>().cloned(),
                player: e.get::<Player>().cloned(),
                boss: e.get::<Boss>().cloned(),
                enemy: e.get::<Enemy>().cloned(),
                spell: e.get::<Spell>().cloned(),
                loot: e.get::<Loot>().copied(),
            }
        })
        .collect();
    Snapshot {
        tick: *world.resource::<SimTick>(),
        rng: *world.resource::<SimRng>(),
        encounter: *world.resource::<Encounter>(),
        reset: world.resource::<ResetFight>().clone(),
        next_id: *world.resource::<NextSimId>(),
        entities,
    }
}

pub fn load(world: &mut World, snap: &Snapshot) {
    let current: HashMap<SimId, Entity> = world.query_filtered::<(Entity, &SimId), With<SimEntity>>().iter(world).map(|(e, id)| (*id, e)).collect();
    let wanted: HashMap<SimId, ()> = snap.entities.iter().map(|s| (s.id, ())).collect();
    for (id, e) in &current {
        if !wanted.contains_key(id) {
            world.despawn(*e);
        }
    }
    // Saved entity → live entity (the same one if it still exists).
    let mut map: HashMap<Entity, Entity> = HashMap::default();
    for s in &snap.entities {
        let e = match current.get(&s.id) {
            Some(&e) => e,
            None => world.spawn((SimEntity, s.id)).id(),
        };
        map.insert(s.entity, e);
    }
    let to = |e: Entity| map.get(&e).copied().unwrap_or(e);
    for s in &snap.entities {
        let mut e = world.entity_mut(to(s.entity));
        put(&mut e, s.team);
        put(&mut e, s.foe.then_some(Foe));
        put(&mut e, s.body);
        put(&mut e, s.prev);
        put(&mut e, s.health);
        put(&mut e, s.hitstop);
        put(
            &mut e,
            s.action.clone().map(|mut a| {
                a.hits.iter_mut().for_each(|h| h.1 = to(h.1));
                a
            }),
        );
        put(
            &mut e,
            s.player.clone().map(|mut p| {
                p.lock = p.lock.map(to);
                p
            }),
        );
        put(
            &mut e,
            s.boss.clone().map(|mut b| {
                b.target = b.target.map(to);
                b.last_attacker = b.last_attacker.map(|(a, at)| (to(a), at));
                b
            }),
        );
        put(
            &mut e,
            s.enemy.clone().map(|mut en| {
                en.target = en.target.map(to);
                en
            }),
        );
        put(
            &mut e,
            s.spell.clone().map(|mut sp| {
                sp.target = sp.target.map(to);
                sp.struck.iter_mut().for_each(|x| *x = to(*x));
                sp.caster = sp.caster.map(|(c, seq)| (to(c), seq));
                sp
            }),
        );
        put(&mut e, s.loot);
    }
    *world.resource_mut::<SimTick>() = snap.tick;
    *world.resource_mut::<SimRng>() = snap.rng;
    *world.resource_mut::<Encounter>() = snap.encounter;
    *world.resource_mut::<ResetFight>() = snap.reset.clone();
    *world.resource_mut::<NextSimId>() = snap.next_id;
    world.resource_mut::<SimCommands>().0.clear();
}

/// Sets the component (or removes it).
fn put<T: Component>(e: &mut EntityWorldMut, v: Option<T>) {
    match v {
        Some(v) => {
            e.insert(v);
        }
        None => {
            e.remove::<T>();
        }
    }
}
