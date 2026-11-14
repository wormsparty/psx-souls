//! The mage's magic: a spell going off (the arcane bolt, the arcane lance), and the projectiles
//! (or eruptions) it leaves, which hit the opponents.
//!
//! They're `Spell` entities like the bosses' (same definitions, same rendering), cast by a
//! player (`Spell::ally`): `spell_tick` leaves them to `ally_spell_tick`.

use bevy::prelude::*;

use super::boss::Boss;
use super::combat::{FoeHit, Strike, damage_foe};
use super::data::{SpellKind, Tuning};
use super::enemy::Enemy;
use super::fighter::{Action, Body, Foe, Health, Hitstop};
use super::player::Player;
use super::spell::Spell;
use super::{DT, SimEntity, SimEvent, SimEvents, SimIds, math, sim_order, world};

/// `Spell::boss` of a player's spell (no boss: its element's colours).
pub const ALLY: u8 = u8::MAX;

/// What a spell aims at: an opponent, the point aimed at (projectile) and its feet (eruption).
#[derive(Clone, Copy, Debug)]
pub struct Aim {
    pub foe: Entity,
    pub point: Vec3,
    pub feet: Vec3,
}

/// Height of the arenas' pillars: a projectile below it crashes into them.
const PILLAR_HEIGHT: f32 = 6.0;
/// A projectile further than this from every arena and from the caster's zone is lost.
const ARENA_MARGIN: f32 = 2.0;

/// Spell `si` of player `caster` goes off: its projectile or eruption.
#[allow(clippy::too_many_arguments)]
pub fn release(commands: &mut Commands, t: &Tuning, body: &Body, si: u8, caster: Entity, volley: u32, aim: Option<Aim>, events: &mut SimEvents) {
    let Some(sd) = t.magic.spells.get(si as usize) else { return };
    let spell = &sd.spell;
    let origin = math::local_to_world(body.pos, body.yaw, sd.from);
    let ahead = math::forward(body.yaw);
    let (pos, dir) = match spell.kind {
        SpellKind::Bolt => (origin, aim.map_or(ahead, |a| (a.point - origin).normalize_or(ahead))),
        // Under the opponent's feet, otherwise a little ahead; on the ground.
        SpellKind::Eruption => {
            let at = aim.map_or(body.pos + ahead * (sd.range / 3.0), |a| a.feet);
            let y = world::floor_at(t, at.x, at.z, at.y + 1.0).unwrap_or(at.y);
            (Vec3::new(at.x, y, at.z), Vec3::ZERO)
        }
        // A player doesn't breathe fire.
        SpellKind::Beam => return,
    };
    commands.spawn((
        SimEntity,
        Spell {
            boss: ALLY,
            spell: si,
            target: aim.map(|a| a.foe),
            pos,
            prev: pos,
            dir,
            age: 0,
            wait: 0,
            struck: Vec::new(),
            volley,
            caster: None,
            from: sd.from,
            reach: 0.0,
            end: pos,
            landed: None,
            aim_off: None,
            ally: Some(caster),
        },
    ));
    events.push(SimEvent::SpellCast { pos: origin, element: spell.element, boss: ALLY, volley });
}

/// The capsule of a body (and the zones of a large boss) touched by a sphere of radius `r`
/// sweeping from `a` to `b`.
fn touches(a: Vec3, b: Vec3, r: f32, body: &Body, boss: Option<&Boss>, t: &Tuning) -> bool {
    let h1 = body.pos + Vec3::Y * body.radius;
    let h2 = body.pos + Vec3::Y * (body.height - body.radius).max(body.radius);
    if math::segment_distance(a, b, h1, h2) <= r + body.radius {
        return true;
    }
    boss.is_some_and(|bs| {
        bs.def(t).parts.iter().any(|p| {
            let c = math::local_to_world(body.pos, body.yaw, p.at);
            math::segment_distance(a, b, c, c) <= r + p.r
        })
    })
}

type Foes<'w, 's> = Query<
    'w,
    's,
    (Entity, Option<&'static mut Boss>, Option<&'static mut Enemy>, &'static Body, &'static mut Health, &'static mut Action, &'static mut Hitstop),
    (With<Foe>, Without<Player>),
>;

/// Advances the players' spells: the bolts fly (homing in slightly on their target) and burst
/// on the first opponent they meet, the pillars burst out of the ground after their warning.
#[allow(clippy::type_complexity)]
pub fn ally_spell_tick(
    mut commands: Commands,
    tuning: Res<Tuning>,
    mut events: ResMut<SimEvents>,
    mut spells: Query<(Entity, &mut Spell)>,
    mut players: Query<(Entity, &mut Player, &mut Health), Without<Foe>>,
    mut foes: Foes,
    ids: SimIds,
) {
    let t = &*tuning;
    let order = sim_order(&ids);
    let mut slain = 0u64;
    let mut it = spells.iter_many_mut(&order);
    while let Some((e, mut s)) = it.fetch_next() {
        let Some(caster) = s.ally else { continue };
        let sd = s.def(t).clone();
        let stagger = sd.stagger;
        s.age += 1;
        s.prev = s.pos;
        // Who it can hit, in a deterministic order: the living opponents not dodging.
        let mut struck: Option<(Entity, Vec3)> = None;
        match sd.kind {
            SpellKind::Bolt => {
                let out = !t.arenas.iter().map(world::arena_bounds).any(|(c, r)| math::flat_len(s.pos - c) <= r + ARENA_MARGIN)
                    && world::zone_at(t, s.pos) != world::Zone::Level;
                let pillar = t.arenas.iter().flat_map(|a| &a.pillars).any(|&[x, z, r]| s.pos.y < PILLAR_HEIGHT && math::flat_len(s.pos - Vec3::new(x, s.pos.y, z)) < r + sd.radius * 0.5);
                let ground = world::floor_at(t, s.pos.x, s.pos.z, s.pos.y + 0.5).is_some_and(|y| s.pos.y < y);
                if s.age > sd.delay + sd.life || out || pillar || ground {
                    events.push(SimEvent::SpellFizzle { pos: s.pos, element: sd.element, boss: ALLY });
                    commands.entity(e).despawn();
                    continue;
                }
                // Homes in on its target (only horizontally, and not behind it).
                if sd.homing > 0.0
                    && let Some((.., tb, th, _, _)) = s.target.and_then(|f| foes.get(f).ok())
                    && !th.dead()
                {
                    let to = tb.pos + Vec3::Y * (tb.height * 0.5).min(1.3) - s.pos;
                    let cur = math::yaw_of(s.dir);
                    let want = math::yaw_of(to);
                    if math::wrap(want - cur).abs() < 1.6 {
                        let y = math::turn_towards(cur, want, sd.homing.to_radians() * DT);
                        let flat = math::flat_len(s.dir);
                        s.dir = math::forward(y) * flat + Vec3::Y * s.dir.y;
                    }
                }
                let step = s.dir * sd.speed * DT;
                s.pos += step;
                let (a, b) = (s.prev, s.pos);
                for (fe, boss, _, fb, fh, fa, _) in foes.iter_many(&order) {
                    if !fh.dead() && !fa.iframes(t) && touches(a, b, sd.radius, fb, boss, t) {
                        struck = Some((fe, b));
                        break;
                    }
                }
                if struck.is_some() {
                    events.push(SimEvent::SpellFizzle { pos: s.pos, element: sd.element, boss: ALLY });
                    commands.entity(e).despawn();
                }
            }
            SpellKind::Eruption => {
                let burst = s.burst_at(t);
                if s.age == burst {
                    events.push(SimEvent::Eruption { pos: s.pos, radius: sd.radius, element: sd.element, boss: ALLY, volley: s.volley });
                }
                if s.age >= burst + sd.life {
                    commands.entity(e).despawn();
                    continue;
                }
                if s.age < burst {
                    continue;
                }
                // Several opponents can be caught in it: each one once, one per tick.
                for (fe, _, _, fb, fh, fa, _) in foes.iter_many(&order) {
                    let inside = math::flat_len(fb.pos - s.pos) <= sd.radius + fb.radius && (fb.pos.y - s.pos.y).abs() < 3.0;
                    if !fh.dead() && !fa.iframes(t) && inside && !s.struck.contains(&fe) {
                        struck = Some((fe, fb.pos + Vec3::Y * 1.0));
                        break;
                    }
                }
                if let Some((fe, _)) = struck {
                    s.struck.push(fe);
                }
            }
            SpellKind::Beam => {
                commands.entity(e).despawn();
                continue;
            }
        }
        let Some((fe, pos)) = struck else { continue };
        let Ok((_, mut boss, mut enemy, fbody, mut fhp, mut fact, mut fstop)) = foes.get_mut(fe) else { continue };
        fstop.0 = fstop.0.max(sd.hitstop);
        let strike = Strike { damage: sd.damage, stagger, gauge: true, heavy: sd.kind == SpellKind::Eruption, pos };
        let foe = FoeHit { entity: fe, boss: boss.as_deref_mut(), enemy: enemy.as_deref_mut(), body: fbody, health: &mut fhp, action: &mut fact };
        let attacker = players.get_mut(caster).ok();
        let player = attacker.map(|(_, p, h)| (p, h));
        match player {
            Some((mut p, mut h)) => damage_foe(t, caster, Some((&mut p, &mut h)), foe, &strike, &mut events, &mut slain),
            None => damage_foe(t, caster, None, foe, &strike, &mut events, &mut slain),
        }
    }
    if slain != 0 {
        for (_, mut p, _) in &mut players {
            p.slain |= slain;
        }
    }
}
