//! Boss spells: projectiles that fly towards their target, and eruptions that burst from the ground
//! after a warning. They're full-fledged (deterministic) simulation entities.

use bevy::prelude::*;

use super::boss::Boss;
use super::combat::{Blow, Struck, strike_player};
use super::data::{Cast, CastAim, Reaction, SpellKind, Tuning};
use super::fighter::{Action, Body, Health, Hitstop};
use super::player::{PState, Player};
use super::rng::SimRng;
use super::{DT, SimEntity, SimEvent, SimEvents, SimIds, SimTick, math, sim_order, world};

#[derive(Component, Clone, Debug)]
pub struct Spell {
    /// Casting boss (definition) and spell (the boss's `spells`).
    pub boss: u8,
    pub spell: u8,
    /// Targeted player (projectiles home in on it slightly).
    pub target: Option<Entity>,
    pub pos: Vec3,
    /// Position on the previous tick (collision sweep, render interpolation).
    pub prev: Vec3,
    /// Direction of a projectile.
    pub dir: Vec3,
    pub age: u32,
    /// Extra delay before bursting out (wave of eruptions).
    pub wait: u32,
    /// Players already hit (an eruption only hits once).
    pub struck: Vec<Entity>,
    /// Attack that cast it: spells from the same attack only hit once.
    pub volley: u32,
    /// Stream: its caster and the instance of its attack (the stream stops if it's interrupted),
    /// the starting point (caster's local frame), the distance along the ground where it hits the ground,
    /// and the end of the stream.
    pub caster: Option<(Entity, u32)>,
    pub from: [f32; 3],
    pub reach: f32,
    pub end: Vec3,
    /// Projectile crashed on the ground: tick (`age`) of the impact. It keeps burning there for a while.
    pub landed: Option<u32>,
    /// Aimed projectile that hangs for a while (`delay`): its offset (radians) from the
    /// target's direction, to aim at it again when it launches.
    pub aim_off: Option<f32>,
}

impl Spell {
    pub fn def<'a>(&self, t: &'a Tuning) -> &'a super::data::SpellDef {
        &t.bosses[self.boss as usize].spells[self.spell as usize]
    }

    /// Eruption: tick at which it bursts out.
    pub fn burst_at(&self, t: &Tuning) -> u32 {
        self.def(t).delay + self.wait
    }
}

/// Height aimed at on a player.
const AIM_HEIGHT: f32 = 1.0;
/// Projectiles leaving the arena, or crashing into a pillar, disappear.
const ARENA_MARGIN: f32 = 2.0;
/// Height of the arenas' pillars (tools/blender/arena.py): projectiles fly over them above that.
const PILLAR_HEIGHT: f32 = 6.0;
/// The stream extends slightly beyond the point where it hits the ground (it spreads there).
const BEAM_SPLASH: f32 = 2.5;
/// Projectile crashed on the ground: it burns there for `SPLASH_LIFE` ticks, on a small circle (at least
/// `SPLASH_MIN` m, otherwise `SPLASH_SCALE` times its radius), and deals this share of its damage there.
pub const SPLASH_LIFE: u32 = 40;
const SPLASH_MIN: f32 = 1.2;
const SPLASH_SCALE: f32 = 2.5;
const SPLASH_DAMAGE: f32 = 0.5;

/// Radius of the area a projectile leaves when crashing on the ground.
pub fn splash_radius(sd: &super::data::SpellDef) -> f32 {
    (sd.radius * SPLASH_SCALE).max(SPLASH_MIN)
}

/// Ends of a stream cast from `from` (local frame of `body`), which hits the ground
/// `reach` metres in front of that point.
pub fn beam_ends(body: &Body, from: [f32; 3], reach: f32) -> (Vec3, Vec3) {
    let origin = math::local_to_world(body.pos, body.yaw, from);
    let ground = Vec3::new(origin.x, body.pos.y, origin.z) + math::forward(body.yaw) * reach;
    let dir = (ground - origin).normalize_or(math::forward(body.yaw));
    (origin, ground + dir * BEAM_SPLASH)
}

/// Casts spell `c` from `body` towards `target` (targeted player and their position).
/// `caster`: the caster and the number of its action (a stream stops with it).
#[allow(clippy::too_many_arguments)]
pub fn cast(
    commands: &mut Commands,
    t: &Tuning,
    boss: u8,
    caster: (Entity, u32),
    c: &Cast,
    body: &Body,
    target: Option<(Entity, Vec3)>,
    volley: u32,
    rng: &mut SimRng,
    events: &mut SimEvents,
) {
    let bd = &t.bosses[boss as usize];
    let Some(si) = bd.spell(&c.spell) else { return };
    let sd = &bd.spells[si as usize];
    let origin = math::local_to_world(body.pos, body.yaw, c.from);
    let n = c.count.max(1) as usize;
    let fan = |i: usize| if n > 1 { (i as f32 / (n - 1) as f32 - 0.5) * c.spread } else { 0.0 };
    let ground = body.pos.y;
    let spell = |pos: Vec3, dir: Vec3, wait: u32| Spell {
        boss,
        spell: si,
        target: target.map(|x| x.0),
        pos,
        prev: pos,
        dir,
        age: 0,
        wait,
        struck: Vec::new(),
        volley,
        caster: None,
        from: c.from,
        reach: 0.0,
        end: pos,
        landed: None,
        aim_off: None,
    };
    let mut spawn = |s: Spell| {
        commands.spawn((SimEntity, s));
    };
    match sd.kind {
        SpellKind::Bolt => {
            let base = match (c.aim, target) {
                (CastAim::Target, Some((_, tp))) => (tp + Vec3::Y * AIM_HEIGHT - origin).normalize_or(math::forward(body.yaw)),
                // Straight ahead, but diving towards the target's height (a breath from above).
                (CastAim::Forward, Some((_, tp))) => {
                    let to = tp + Vec3::Y * AIM_HEIGHT - origin;
                    let pitch = math::atan2(to.y, math::flat_len(to).max(1.0));
                    (math::forward(body.yaw) * pitch.cos() + Vec3::Y * pitch.sin()).normalize_or(math::forward(body.yaw))
                }
                _ => math::forward(body.yaw),
            };
            let yaw = math::yaw_of(base);
            let pitch = base.y.clamp(-1.0, 1.0).asin();
            for i in 0..n {
                let y = if c.aim == CastAim::Ring { yaw + std::f32::consts::TAU * i as f32 / n as f32 } else { yaw + fan(i).to_radians() };
                let dir = math::forward(y) * pitch.cos() + Vec3::Y * pitch.sin();
                let aim_off = (c.aim == CastAim::Target && sd.delay > 0).then(|| fan(i).to_radians());
                spawn(Spell { aim_off, ..spell(origin, dir, 0) });
            }
        }
        SpellKind::Beam => {
            let dist = target.map_or(sd.reach[1], |(_, tp)| math::flat_len(tp - origin));
            let reach = dist.clamp(sd.reach[0], sd.reach[1]);
            let (a, b) = beam_ends(body, c.from, reach);
            spawn(Spell { caster: Some(caster), reach, end: b, ..spell(a, Vec3::ZERO, 0) });
        }
        SpellKind::Eruption => {
            let mut placed: Vec<Vec3> = Vec::new();
            for i in 0..n {
                let flat = match (c.aim, target) {
                    (CastAim::Target, Some((_, tp))) => {
                        // The first under the feet, the others scattered around, without
                        // overlapping: keep the best of a few draws.
                        if i == 0 || c.spread <= 0.0 {
                            tp
                        } else {
                            let mut best = (f32::MIN, tp);
                            for _ in 0..6 {
                                let a = rng.next_f32() * std::f32::consts::TAU;
                                let r = c.spread * (0.35 + 0.65 * rng.next_f32());
                                let p = tp + Vec3::new(math::cos(a), 0.0, math::sin(a)) * r;
                                let gap = placed.iter().map(|q| math::flat_len(p - *q)).fold(f32::MAX, f32::min);
                                if gap > best.0 {
                                    best = (gap, p);
                                }
                                if gap > sd.radius * 1.6 {
                                    break;
                                }
                            }
                            best.1
                        }
                    }
                    (CastAim::Ring, _) => {
                        let a = body.yaw + std::f32::consts::TAU * i as f32 / n as f32;
                        body.pos + math::forward(a) * c.spread
                    }
                    _ => origin + math::forward(body.yaw) * (c.step * (i + 1) as f32),
                };
                // On the ground (or at the caster's height above the void).
                placed.push(flat);
                let y = world::floor_at(t, flat.x, flat.z, ground + 1.0).unwrap_or(ground);
                spawn(spell(Vec3::new(flat.x, y, flat.z), Vec3::ZERO, c.delay_step * i as u32));
            }
        }
    }
    events.push(SimEvent::SpellCast { pos: origin, element: sd.element, boss, volley });
}

/// Advances the spells and hits the players.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn spell_tick(
    mut commands: Commands,
    tuning: Res<Tuning>,
    tick: Res<SimTick>,
    mut events: ResMut<SimEvents>,
    mut spells: Query<(Entity, &mut Spell)>,
    mut players: Query<(Entity, &mut Player, &Body, &mut Health, &mut Action, &mut Hitstop), Without<Boss>>,
    casters: Query<(&Body, &Action, &Health), (With<Boss>, Without<Player>)>,
    ids: SimIds,
) {
    let t = &*tuning;
    let now = tick.0;
    let order = sim_order(&ids);
    let mut spell_it = spells.iter_many_mut(&order);
    while let Some((e, mut s)) = spell_it.fetch_next() {
        let sd = s.def(t).clone();
        s.age += 1;
        s.prev = s.pos;
        let blow = Blow { damage: sd.damage, reaction: sd.reaction, fury: false, aoe: sd.aoe || sd.kind != SpellKind::Bolt, hitstop: sd.hitstop };
        let targetable = |p: &Player, h: &Health| !h.dead() && !matches!(p.state, PState::Falling | PState::Dead);
        match sd.kind {
            SpellKind::Bolt => {
                if let Some(at) = s.landed {
                    // Crashed on the ground: it keeps burning there for a while (neither guard nor parry).
                    if s.age >= at + SPLASH_LIFE {
                        commands.entity(e).despawn();
                        continue;
                    }
                    let r = splash_radius(&sd);
                    let blow = Blow { damage: sd.damage * SPLASH_DAMAGE, reaction: Reaction::Light, aoe: true, ..blow };
                    let mut pit = players.iter_many_mut(&order);
                    while let Some((pe, mut p, pbody, mut php, mut pact, mut pstop)) = pit.fetch_next() {
                        if s.struck.contains(&pe) || !targetable(&p, &php) || p.volley == s.volley {
                            continue;
                        }
                        if math::flat_len(pbody.pos - s.pos) > r + pbody.radius || (pbody.pos.y - s.pos.y).abs() > 1.5 {
                            continue;
                        }
                        let pos = pbody.pos + Vec3::Y * 0.5;
                        if strike_player(t, now, s.pos, &blow, &mut p, &mut pact, &mut php, &mut pstop, pbody, pos, pe, &mut events) != Struck::Dodged {
                            p.volley = s.volley;
                            s.struck.push(pe);
                        }
                    }
                    continue;
                }
                // Hangs for a while where it appeared (you see it coming), then heads for
                // its target, wherever it is at that point.
                if s.age < sd.delay {
                    continue;
                }
                if s.age == sd.delay
                    && let Some(off) = s.aim_off
                    && let Some((_, _, tb, ..)) = s.target.and_then(|p| players.get(p).ok())
                {
                    let to = (tb.pos + Vec3::Y * AIM_HEIGHT - s.pos).normalize_or(s.dir);
                    let pitch = to.y.clamp(-1.0, 1.0).asin();
                    s.dir = math::forward(math::yaw_of(to) + off) * pitch.cos() + Vec3::Y * pitch.sin();
                }
                let out = t.arenas.iter().map(world::arena_bounds).all(|(c, r)| math::flat_len(s.pos - c) > r + ARENA_MARGIN);
                let pillar = t.arenas.iter().flat_map(|a| &a.pillars).any(|&[x, z, r]| s.pos.y < PILLAR_HEIGHT && math::flat_len(s.pos - Vec3::new(x, s.pos.y, z)) < r + sd.radius * 0.5);
                if s.age > sd.delay + sd.life || out || pillar {
                    events.push(SimEvent::SpellFizzle { pos: s.pos, element: sd.element, boss: s.boss });
                    commands.entity(e).despawn();
                    continue;
                }
                if let Some(y) = world::floor_at(t, s.pos.x, s.pos.z, s.pos.y + 0.5).filter(|&y| s.pos.y < y) {
                    // On the ground: a small damage area, where it crashed.
                    s.pos.y = y;
                    s.prev = s.pos;
                    s.landed = Some(s.age);
                    events.push(SimEvent::SpellSplash { pos: s.pos, radius: splash_radius(&sd), element: sd.element, boss: s.boss });
                    continue;
                }
                // Homes in on its target slightly (only horizontally and not behind it).
                if sd.homing > 0.0
                    && let Some((_, _, tb, ..)) = s.target.and_then(|p| players.get(p).ok())
                {
                    let to = tb.pos + Vec3::Y * AIM_HEIGHT - s.pos;
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
                let mut gone = false;
                let mut pit = players.iter_many_mut(&order);
                while let Some((pe, mut p, pbody, mut php, mut pact, mut pstop)) = pit.fetch_next() {
                    if !targetable(&p, &php) || p.volley == s.volley {
                        continue;
                    }
                    let h1 = pbody.pos + Vec3::Y * pbody.radius;
                    let h2 = pbody.pos + Vec3::Y * (pbody.height - pbody.radius);
                    if math::segment_distance(a, b, h1, h2) > sd.radius + pbody.radius {
                        continue;
                    }
                    let pos = s.pos;
                    if strike_player(t, now, s.prev, &blow, &mut p, &mut pact, &mut php, &mut pstop, pbody, pos, pe, &mut events) != Struck::Dodged {
                        p.volley = s.volley;
                        gone = true;
                        break;
                    }
                }
                if gone {
                    events.push(SimEvent::SpellFizzle { pos: s.pos, element: sd.element, boss: s.boss });
                    commands.entity(e).despawn();
                }
            }
            SpellKind::Beam => {
                // Follows the caster's mouth; stops with its attack.
                let alive = s.caster.and_then(|(c, seq)| {
                    casters.get(c).ok().filter(|(_, a, h)| !h.dead() && a.seq == seq && a.mv.is_some())
                });
                let Some((cb, ..)) = alive.filter(|_| s.age <= sd.life) else {
                    commands.entity(e).despawn();
                    continue;
                };
                let (a, b) = beam_ends(cb, s.from, s.reach);
                (s.pos, s.end) = (a, b);
                let mut pit = players.iter_many_mut(&order);
                while let Some((pe, mut p, pbody, mut php, mut pact, mut pstop)) = pit.fetch_next() {
                    if s.struck.contains(&pe) || !targetable(&p, &php) || p.volley == s.volley {
                        continue;
                    }
                    let h1 = pbody.pos + Vec3::Y * pbody.radius;
                    let h2 = pbody.pos + Vec3::Y * (pbody.height - pbody.radius);
                    if math::segment_distance(a, b, h1, h2) > sd.radius + pbody.radius {
                        continue;
                    }
                    let pos = pbody.pos + Vec3::Y * 1.0;
                    if strike_player(t, now, a, &blow, &mut p, &mut pact, &mut php, &mut pstop, pbody, pos, pe, &mut events) != Struck::Dodged {
                        p.volley = s.volley;
                        s.struck.push(pe);
                    }
                }
            }
            SpellKind::Eruption => {
                let burst = s.burst_at(t);
                if s.age == burst {
                    events.push(SimEvent::Eruption { pos: s.pos, radius: sd.radius, element: sd.element, boss: s.boss, volley: s.volley });
                }
                if s.age >= burst + sd.life {
                    commands.entity(e).despawn();
                    continue;
                }
                if s.age < burst {
                    continue;
                }
                let mut pit = players.iter_many_mut(&order);
                while let Some((pe, mut p, pbody, mut php, mut pact, mut pstop)) = pit.fetch_next() {
                    if s.struck.contains(&pe) || !targetable(&p, &php) || p.volley == s.volley {
                        continue;
                    }
                    if math::flat_len(pbody.pos - s.pos) > sd.radius + pbody.radius || (pbody.pos.y - s.pos.y).abs() > 2.5 {
                        continue;
                    }
                    let pos = pbody.pos + Vec3::Y * 1.0;
                    if strike_player(t, now, s.pos, &blow, &mut p, &mut pact, &mut php, &mut pstop, pbody, pos, pe, &mut events) != Struck::Dodged {
                        p.volley = s.volley;
                        s.struck.push(pe);
                    }
                }
            }
        }
    }
}

/// Upcoming eruption: centre, radius and warning progress (0 → 1 when it bursts).
pub fn telegraph(s: &Spell, t: &Tuning) -> Option<(Vec3, f32, f32)> {
    let sd = s.def(t);
    if sd.kind != SpellKind::Eruption {
        return None;
    }
    let burst = s.burst_at(t);
    (s.age < burst).then(|| (s.pos, sd.radius, s.age as f32 / burst.max(1) as f32))
}
