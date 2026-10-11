//! Body collisions, hit detection and resolution (guard, perfect guard, damage…).

use bevy::prelude::*;

use super::boss::Boss;
use super::data::{BossMove, EnemyMove, HitWindow, MoveDef, MoveRef, PartDef, PlayerMove, Reaction, Tuning, WeaponMove};
use super::encounter::{self, CHECKPOINT_RADIUS, Encounter};
use super::enemy::Enemy;
use super::fighter::{Action, Body, Foe, Health, Hitstop, PrevBody};
use super::player::{PState, Player, force_move};
use super::world::{self, Mover, Step};
use super::{DT, SimEvent, SimEvents, SimTick, math};

/// Separates overlapping bodies and keeps them on walkable ground. A player pushed
/// (or walking, rolling…) past an open edge falls.
#[allow(clippy::type_complexity)]
pub fn separate_bodies(
    tuning: Res<Tuning>,
    enc: Res<Encounter>,
    mut events: ResMut<SimEvents>,
    mut q: Query<(Entity, &mut Body, &PrevBody, &Health, Option<&mut Player>, &mut Action, Option<&Boss>)>,
) {
    let solid = |h: &Health, p: Option<&Mut<Player>>| !h.dead() && !p.is_some_and(|p| p.falling);
    let mut combos = q.iter_combinations_mut();
    while let Some([(_, mut a, _, ha, pa, ..), (_, mut b, _, hb, pb, ..)]) = combos.fetch_next() {
        if !solid(ha, pa.as_ref()) || !solid(hb, pb.as_ref()) {
            continue;
        }
        let d = Vec3::new(b.pos.x - a.pos.x, 0.0, b.pos.z - a.pos.z);
        let dist = math::flat_len(d);
        let min = a.radius + b.radius;
        if dist < min && (a.pos.y - b.pos.y).abs() < 1.5 {
            let n = if dist > 1e-4 { d / dist } else { Vec3::X };
            let overlap = min - dist;
            let total = a.mass + b.mass;
            let (wa, wb) = (b.mass / total, a.mass / total);
            a.pos -= n * overlap * wa;
            b.pos += n * overlap * wb;
        }
    }
    let t = &*tuning;
    let obstacles = world::obstacles(t, CHECKPOINT_RADIUS);
    for (entity, mut b, prev, hp, mut player, mut action, boss) in &mut q {
        if player.as_ref().is_some_and(|p| p.falling) {
            continue;
        }
        let r = b.radius;
        for [px, pz, pr] in obstacles.iter().copied() {
            let d = Vec3::new(b.pos.x - px, 0.0, b.pos.z - pz);
            let dist = math::flat_len(d);
            if dist < pr + r && dist > 1e-4 {
                b.pos += d / dist * (pr + r - dist);
            }
        }
        // Players walk in the level or in the arena they went into; the bosses never leave
        // theirs, path enemies never go into the corridors to the fogs.
        let door_open = encounter::door_open(t, enc.defeated);
        let mover = match (&player, boss) {
            (Some(p), _) => Mover::Player { zone: p.zone, door_open },
            // Climbing jump in flight: over walls and floors, it lands at the end (`boss_act`).
            (None, Some(bs)) if bs.flight.is_some() => continue,
            (None, Some(bs)) if world::perched(t, b.pos, bs.arena) => Mover::Climber(bs.arena),
            (None, Some(bs)) => Mover::Boss(bs.arena),
            (None, None) => Mover::Enemy,
        };
        if let Some(p) = player.as_mut().filter(|p| p.airborne) {
            air_step(t, &mut b, prev, p, &mut action, mover, hp.dead(), entity, &mut events);
            continue;
        }
        match world::step(t, b.pos, r, mover) {
            Step::Ground(p) => b.pos = p,
            Step::Drop(p) => {
                b.pos = p;
                // Walking off a gallery: you jump down, momentum kept.
                if let Some(mut pl) = player {
                    pl.vel = if matches!(pl.state, PState::Free | PState::Guard) {
                        Vec3::new(b.pos.x - prev.pos.x, 0.0, b.pos.z - prev.pos.z) / DT
                    } else {
                        Vec3::ZERO
                    };
                    pl.airborne = true;
                    pl.air_vy = 0.0;
                    pl.air_from = prev.pos.y;
                    pl.air_ticks = 0;
                }
            }
            Step::Fall => {
                if let Some(mut p) = player {
                    let momentum = (b.pos - prev.pos) / DT;
                    p.start_fall(&mut action, momentum, prev.pos);
                    events.push(SimEvent::Fell { entity });
                }
            }
        }
    }
}

/// Plunging attack: a jump attack started at least this high above the opponent's feet deals
/// (and staggers) this much more.
const PLUNGE_HEIGHT: f32 = 1.8;
const PLUNGE_MULT: f32 = 1.8;

/// A floor higher than this above the feet stops a jumper (they hit its edge);
/// below that, they climb onto it while coming down.
const LEDGE: f32 = 0.35;
/// Above the void, once this far below the floor left behind, the jump becomes a fall.
const AIR_FALL: f32 = 1.0;

/// Player in the air: walls stop them, the void doesn't. They land on the first floor they
/// meet while coming down; if they pass too low above the void, they fall.
#[allow(clippy::too_many_arguments)]
fn air_step(t: &Tuning, b: &mut Body, prev: &PrevBody, p: &mut Player, action: &mut Action, mover: Mover, dead: bool, entity: Entity, events: &mut SimEvents) {
    if let Step::Ground(g) | Step::Drop(g) = world::step(t, b.pos, b.radius, mover) {
        b.pos.x = g.x;
        b.pos.z = g.z;
    }
    let mut floor = world::floor_below(t, b.pos.x, b.pos.z, b.pos.y, Some(mover));
    if floor.is_some_and(|y| y > b.pos.y + LEDGE) {
        // Against the side of a higher platform: stick to it, fall back down.
        b.pos.x = prev.pos.x;
        b.pos.z = prev.pos.z;
        p.vel = Vec3::ZERO;
        floor = world::floor_below(t, b.pos.x, b.pos.z, b.pos.y, Some(mover)).filter(|y| *y <= b.pos.y + LEDGE);
    }
    match floor {
        Some(y) if p.air_vy <= 0.0 && b.pos.y <= y => {
            b.pos.y = y;
            p.airborne = false;
            p.air_vy = 0.0;
            events.push(SimEvent::Landed { entity });
        }
        None if b.pos.y < p.air_from - AIR_FALL => {
            let vy = p.air_vy;
            let momentum = p.vel;
            p.start_fall(action, momentum, Vec3::new(prev.pos.x, p.air_from, prev.pos.z));
            p.fall_vy = vy;
            if dead {
                p.state = PState::Dead;
            }
            events.push(SimEvent::Fell { entity });
        }
        _ => {}
    }
}

/// World capsule of a hit window at a given instant (`sub` ∈ [0,1] between two ticks).
pub fn hit_capsule(body: &Body, h: &HitWindow, tick: f32) -> (Vec3, Vec3, f32) {
    let (mut a, mut b) = (h.capsule.a, h.capsule.b);
    if let Some([s, e]) = h.arc {
        let span = (h.end - h.start).saturating_sub(1).max(1) as f32;
        let f = ((tick - h.start as f32) / span).clamp(0.0, 1.0);
        let deg = s + (e - s) * f;
        a = math::rotate_local(a, deg);
        b = math::rotate_local(b, deg);
    }
    (
        math::local_to_world(body.pos, body.yaw, a),
        math::local_to_world(body.pos, body.yaw, b),
        h.capsule.r,
    )
}

/// Tests an active window against a body's hurtbox (vertical capsule).
/// Arc sweeps are sub-sampled so as not to "skip" over the target.
///
/// `reach_down`: the hit lowers down to opponents smaller than it (a dog
/// under a thrust at chest height), as the animation would.
/// `parts`: hittable zones on top of the body (head, tail of a large boss).
pub fn hit_test(attacker: &Body, h: &HitWindow, tick: u32, victim: &Body, reach_down: bool, parts: &[PartDef]) -> bool {
    let h1 = victim.pos + Vec3::Y * victim.radius;
    let h2 = victim.pos + Vec3::Y * (victim.height - victim.radius).max(victim.radius);
    let top = victim.pos.y + victim.height;
    let samples = if h.arc.is_some() && tick > h.start { 4 } else { 1 };
    (0..samples).any(|i| {
        let ft = tick as f32 - (samples - 1 - i) as f32 / samples as f32;
        let (mut a, mut b, r) = hit_capsule(attacker, h, ft);
        if reach_down {
            a.y = a.y.min(top);
            b.y = b.y.min(top);
        }
        math::segment_distance(a, b, h1, h2) <= r + victim.radius
            || parts.iter().any(|p| {
                let c = math::local_to_world(victim.pos, victim.yaw, p.at);
                math::segment_distance(a, b, c, c) <= r + p.r
            })
    })
}

fn active_hits(def: &MoveDef, tick: u32) -> impl Iterator<Item = (u8, &HitWindow)> {
    def.hits
        .iter()
        .enumerate()
        .filter(move |(_, h)| tick >= h.start && tick < h.end)
        .map(|(i, h)| (i as u8, h))
}

struct Pending {
    attacker: Entity,
    victim: Entity,
    mv: MoveRef,
    window: u8,
}

/// Approximate impact point for visual effects.
fn impact_point(attacker: &Body, victim: &Body) -> Vec3 {
    let dir = (attacker.pos - victim.pos).normalize_or_zero();
    victim.pos + Vec3::Y * (victim.height * 0.55).min(1.4) + dir * victim.radius
}

/// Hit taken by an opponent (boss or enemy) that doesn't kill it: boss stagger,
/// enemy interruption. True if the boss becomes staggered.
pub fn foe_stagger(boss: Option<&mut Boss>, enemy: Option<&mut Enemy>, attacker: Entity, amount: f32, poise: f32, action: &mut Action, t: &Tuning) -> bool {
    if let Some(b) = boss {
        return b.add_stagger(amount, action, t);
    }
    if let Some(e) = enemy {
        e.take_hit(attacker, poise, action, t);
    }
    false
}

#[allow(clippy::type_complexity)]
pub fn resolve_hits(
    tuning: Res<Tuning>,
    tick: Res<SimTick>,
    mut events: ResMut<SimEvents>,
    mut players: Query<
        (Entity, &mut Player, &Body, &mut Health, &mut Action, &mut Hitstop),
        Without<Foe>,
    >,
    mut foes: Query<
        (Entity, Option<&mut Boss>, Option<&mut Enemy>, &Body, &mut Health, &mut Action, &mut Hitstop),
        (With<Foe>, Without<Player>),
    >,
) {
    let t = &*tuning;
    let now = tick.0;
    let mut pending = Vec::new();
    let targetable = |p: &Player, h: &Health| !h.dead() && !matches!(p.state, PState::Falling | PState::Dead);

    // 1) Detection (read-only), in a deterministic order: players then opponents.
    for (pe, p, pbody, phealth, pact, _) in &players {
        let (Some(mv), true) = (pact.mv, pact.executed) else { continue };
        if !targetable(p, phealth) {
            continue;
        }
        for (wi, h) in active_hits(t.get(mv), pact.tick) {
            for (fe, fboss, _, fbody, fhealth, fact, _) in &foes {
                let parts = fboss.map_or(&[][..], |b| &b.def(t).parts[..]);
                // Out of reach (the marionette hoisted into the air, the lamplighter slipping away).
                if !fhealth.dead() && !fact.iframes(t) && !pact.hits.contains(&(wi, fe)) && hit_test(pbody, h, pact.tick, fbody, true, parts) {
                    pending.push(Pending { attacker: pe, victim: fe, mv, window: wi });
                }
            }
        }
    }
    for (fe, _, _, fbody, fhealth, fact, _) in &foes {
        let (Some(mv), true) = (fact.mv, fact.executed) else { continue };
        if fhealth.dead() || matches!(mv, MoveRef::Enemy(_, EnemyMove::Death)) {
            continue;
        }
        for (wi, h) in active_hits(t.get(mv), fact.tick) {
            for (pe, p, pbody, phealth, _, _) in &players {
                if targetable(p, phealth) && !fact.hits.contains(&(wi, pe)) && hit_test(fbody, h, fact.tick, pbody, false, &[]) {
                    pending.push(Pending { attacker: fe, victim: pe, mv, window: wi });
                }
            }
        }
    }

    // 2) Resolution.
    for hit in pending {
        let h = &t.get(hit.mv).hits[hit.window as usize];
        if let Ok((pe, mut p, pbody, mut php, mut pact, mut pstop)) = players.get_mut(hit.attacker) {
            // Player → opponent.
            let Ok((fe, mut boss, mut enemy, fbody, mut fhp, mut fact, mut fstop)) = foes.get_mut(hit.victim) else {
                continue;
            };
            if fhp.dead() {
                continue;
            }
            pact.hits.push((hit.window, hit.victim));
            // Plunging attack: a jump attack from high above the opponent (from a gallery).
            let plunge = matches!(hit.mv, MoveRef::Weapon(_, WeaponMove::Jump)) && p.air_from - fbody.pos.y >= PLUNGE_HEIGHT;
            let mult = if plunge { PLUNGE_MULT } else { 1.0 };
            let dmg = h.damage * p.damage_mult() * mult;
            fhp.cur = (fhp.cur - dmg).max(0.0);
            if let Some(b) = boss.as_mut() {
                b.last_attacker = Some((pe, now));
            }
            // Regain: hitting restores part of the HP lost while guarding.
            let heal = (dmg * t.player.regain_ratio).min(p.regain);
            if heal > 0.0 {
                php.cur = (php.cur + heal).min(php.max);
                p.regain -= heal;
            }
            // Special attack and fatal blow hits don't recharge the gauge (otherwise the special,
            // which staggers, would pay for itself through the fatal blow that follows).
            if !matches!(hit.mv, MoveRef::Weapon(_, WeaponMove::Special | WeaponMove::Fatal)) {
                p.special += dmg * t.player.special_per_damage;
            }
            pstop.0 = pstop.0.max(h.hitstop);
            fstop.0 = fstop.0.max(h.hitstop);
            let pos = impact_point(pbody, fbody);
            let heavy = matches!(hit.mv, MoveRef::Weapon(_, WeaponMove::HeavyCharged | WeaponMove::Fatal | WeaponMove::Jump));
            events.push(SimEvent::Hit { pos, heavy, foe: Some(fe) });
            if fhp.dead() {
                if let Some(b) = boss.as_ref() {
                    fact.start(MoveRef::Boss(b.def, BossMove::Death), 0.0);
                    fact.executed = false;
                    events.push(SimEvent::BossDied);
                } else if let Some(e) = enemy.as_mut() {
                    fact.start(MoveRef::Enemy(e.kind, EnemyMove::Death), 0.0);
                    fact.executed = false;
                    if e.unique {
                        p.slain |= 1u64 << e.spawn;
                    }
                    events.push(SimEvent::EnemyDied { pos: fbody.pos });
                }
            } else if foe_stagger(boss.as_deref_mut(), enemy.as_deref_mut(), pe, h.stagger * mult, dmg, &mut fact, t) {
                events.push(SimEvent::Groggy { entity: fe });
            }
        } else if let Ok((fe, mut boss, mut enemy, fbody, _, mut fact, mut fstop)) = foes.get_mut(hit.attacker) {
            // Opponent → player.
            let Ok((pe, mut p, pbody, mut php, mut pact, mut pstop)) = players.get_mut(hit.victim) else {
                continue;
            };
            let pos = impact_point(fbody, pbody);
            let g = &t.player.guard;
            match strike_player(t, now, fbody.pos, &Blow::of(h), &mut p, &mut pact, &mut php, &mut pstop, pbody, pos, &mut events) {
                // I-frames don't consume the hit: it can land later in the window.
                Struck::Dodged => continue,
                Struck::Parried => {
                    // Perfect guard: boss stagger; an enemy is staggered (unless
                    // it has enough poise: it loses a lot of it).
                    fstop.0 = g.perfect_hitstop;
                    if foe_stagger(boss.as_deref_mut(), enemy.as_deref_mut(), pe, g.perfect_stagger, g.perfect_stagger * 5.0, &mut fact, t) {
                        events.push(SimEvent::Groggy { entity: fe });
                    }
                }
                Struck::Guarded | Struck::Hit => fstop.0 = h.hitstop,
            }
            fact.hits.push((hit.window, hit.victim));
        }
    }
}

/// What hits a player: a strike (attack window) or a spell.
#[derive(Clone, Copy, Debug)]
pub struct Blow {
    pub damage: f32,
    pub reaction: Reaction,
    pub fury: bool,
    pub aoe: bool,
    pub hitstop: u8,
}

impl Blow {
    pub fn of(h: &HitWindow) -> Self {
        Self { damage: h.damage, reaction: h.reaction, fury: h.fury, aoe: h.aoe, hitstop: h.hitstop }
    }
}

/// Outcome of a hit dealt to a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Struck {
    /// I-frames: the hit goes through.
    Dodged,
    /// Perfect guard.
    Parried,
    Guarded,
    Hit,
}

/// Applies a hit coming from `from` to the player: perfect guard, guard, or damage and
/// reaction. Doesn't touch the attacker (stagger, hitstop), left to the caller.
#[allow(clippy::too_many_arguments)]
pub fn strike_player(
    t: &Tuning,
    now: u32,
    from: Vec3,
    blow: &Blow,
    p: &mut Player,
    pact: &mut Action,
    php: &mut Health,
    pstop: &mut Hitstop,
    pbody: &Body,
    pos: Vec3,
    events: &mut SimEvents,
) -> Struck {
    if php.dead() || pact.iframes(t) {
        return Struck::Dodged;
    }
    let to_attacker = math::yaw_of(from - pbody.pos);
    let facing = math::wrap(to_attacker - pbody.yaw).abs() <= t.player.guard.arc.to_radians();
    let g = &t.player.guard;
    let damage = blow.damage * p.defense_mult();

    let in_guard = p.state == PState::Guard
        || pact.is(MoveRef::Player(PlayerMove::GuardHit))
        || pact.is(MoveRef::Player(PlayerMove::PerfectGuard));
    if p.guard_held && facing && in_guard && !blow.aoe {
        if now.saturating_sub(p.guard_start) <= p.perfect_window(t) {
            // Perfect guard: no damage, stagger for the attacker.
            force_move(p, pact, MoveRef::Player(PlayerMove::PerfectGuard));
            p.special += g.perfect_special;
            pstop.0 = g.perfect_hitstop;
            events.push(SimEvent::PerfectGuard { pos });
            return Struck::Parried;
        }
        if !blow.fury {
            // Normal guard: reduced damage, converted into regain.
            let dmg = damage * g.damage_ratio;
            php.cur = (php.cur - dmg).max(0.0);
            p.regain += dmg;
            p.regain_timer = t.player.regain_ticks;
            p.spend_stamina(blow.damage * g.stamina_ratio, t);
            pstop.0 = blow.hitstop;
            if php.dead() {
                force_move(p, pact, MoveRef::Player(PlayerMove::Death));
                events.push(SimEvent::PlayerDied);
            } else if p.stamina <= 0.0 {
                force_move(p, pact, MoveRef::Player(PlayerMove::GuardBreak));
                events.push(SimEvent::GuardBreak { pos });
            } else {
                force_move(p, pact, MoveRef::Player(PlayerMove::GuardHit));
                events.push(SimEvent::Guard { pos });
            }
            return Struck::Guarded;
        }
    }

    // Hit taken.
    php.cur = (php.cur - damage).max(0.0);
    p.regain = 0.0;
    p.regain_timer = 0;
    pstop.0 = blow.hitstop;
    events.push(SimEvent::Hit { pos, heavy: blow.reaction == Reaction::Heavy, foe: None });
    if php.dead() {
        force_move(p, pact, MoveRef::Player(PlayerMove::Death));
        events.push(SimEvent::PlayerDied);
    } else if !pact.hyperarmor(t) {
        let mv = match blow.reaction {
            Reaction::Light => PlayerMove::HitLight,
            Reaction::Heavy => PlayerMove::HitHeavy,
        };
        force_move(p, pact, MoveRef::Player(mv));
    }
    Struck::Hit
}
