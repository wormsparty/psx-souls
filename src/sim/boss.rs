//! Boss AI: slow, telegraphed, with pauses where you can punish. Each boss has its
//! definition (`boss.ron`, `bosses.ron`): attacks, spells, lockable parts. An encounter
//! can bring several together (a duo, a butcher and his dogs).

use bevy::prelude::*;

use super::data::{BossAttack, BossDef, BossMove, MoveDef, MoveRef, Side, Tuning};
use super::encounter::Encounter;
use super::fighter::{Action, Body, Health, Hitstop};
use super::player::{Player, motion_speed, run_frame};
use super::rng::SimRng;
use super::world::{self, STEP_UP, Zone};
use super::{DT, SimDebug, SimEvent, SimEvents, SimId, SimIds, SimTick, math, sim_order, spell};

#[derive(Component, Clone, Debug)]
pub struct Boss {
    /// Definition (index in `Tuning::bosses`).
    pub def: u8,
    /// Its arena (`Tuning::arenas`).
    pub arena: u8,
    pub phase: u8,
    pub target: Option<Entity>,
    /// Last player to hit it, and when (aggro in co-op).
    pub last_attacker: Option<(Entity, u32)>,
    pub stagger: f32,
    pub stagger_delay: u32,
    /// Tick from which each attack becomes available again.
    pub cooldowns: Vec<u32>,
    /// Remaining ticks before it can attack.
    pub idle: u32,
    pub strafe: f32,
    pub strafe_timer: u32,
    /// Large beast (`heading_slack`): it's turning towards its target.
    pub turning: bool,
    /// Offset (radians) between the aimed direction and the target, renewed from time to time.
    pub aim_offset: f32,
    pub aim_timer: u32,
    /// Ticks the target has spent standing a step above it (gallery, stairs): past the patience
    /// of its climbing jump (`BossAttack::climb`), it leaps up there.
    pub below_target: u32,
    /// Climbing jump in flight: heights at take-off and at landing. Floors and walls don't hold
    /// it meanwhile; it sets foot at the end of the jump.
    pub flight: Option<(f32, f32)>,
}

impl Boss {
    pub fn new(t: &Tuning, def: u8, arena: u8) -> Self {
        Self {
            def,
            arena,
            phase: 1,
            target: None,
            last_attacker: None,
            stagger: 0.0,
            stagger_delay: 0,
            cooldowns: vec![0; t.bosses[def as usize].attacks.len()],
            idle: 90,
            strafe: 1.0,
            strafe_timer: 0,
            turning: false,
            aim_offset: 0.0,
            aim_timer: 0,
            below_target: 0,
            flight: None,
        }
    }

    pub fn def<'a>(&self, t: &'a Tuning) -> &'a BossDef {
        &t.bosses[self.def as usize]
    }

    /// Adds stagger; returns true if the boss becomes staggered.
    pub fn add_stagger(&mut self, amount: f32, action: &mut Action, t: &Tuning) -> bool {
        let immune = matches!(
            action.mv,
            Some(MoveRef::Boss(_, BossMove::Groggy | BossMove::FatalReceived | BossMove::Death))
        );
        if immune || amount <= 0.0 {
            return false;
        }
        let bd = self.def(t);
        self.stagger += amount;
        self.stagger_delay = bd.stagger_delay;
        if self.stagger >= bd.stagger_max {
            self.stagger = bd.stagger_max;
            action.start(MoveRef::Boss(self.def, BossMove::Groggy), 0.0);
            action.executed = false;
            return true;
        }
        false
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn boss_act(
    mut commands: Commands,
    tuning: Res<Tuning>,
    tick: Res<SimTick>,
    debug: Res<SimDebug>,
    encounter: Res<Encounter>,
    mut rng: ResMut<SimRng>,
    mut events: ResMut<SimEvents>,
    mut bosses: Query<
        (Entity, &mut Boss, &mut Body, &mut Action, &mut Hitstop, &Health, &SimId),
        Without<Player>,
    >,
    players: Query<(Entity, &Body, &Health, &Player), Without<Boss>>,
    ids: SimIds,
) {
    let t = &*tuning;
    let now = tick.0;
    let order = sim_order(&ids);
    // One of the leads has fallen: its partner enters phase 2.
    let partner_fallen = bosses.iter().any(|(_, b, _, _, _, h, _)| h.dead() && !b.def(t).minor);

    let mut it = bosses.iter_many_mut(&order);
    while let Some((entity, mut boss, mut body, mut action, mut hitstop, health, &sim_id)) = it.fetch_next() {
        let def = boss.def;
        let bd = &t.bosses[def as usize];
        if hitstop.0 > 0 {
            hitstop.0 -= 1;
            continue;
        }
        action.executed = true;

        // Target choice: recent last attacker, otherwise the nearest living player (among those
        // in its arena).
        let here = Zone::Arena(boss.arena);
        let alive = |e: Entity| players.get(e).is_ok_and(|(_, _, h, p)| !h.dead() && p.zone == here);
        let recent = boss
            .last_attacker
            .filter(|&(e, at)| now.saturating_sub(at) <= bd.aggro_ticks && alive(e))
            .map(|(e, _)| e);
        boss.target = recent.or_else(|| {
            players
                .iter_many(&order)
                .filter(|(_, _, h, p)| !h.dead() && p.zone == here)
                .min_by(|a, b| a.1.pos.distance(body.pos).total_cmp(&b.1.pos.distance(body.pos)))
                .map(|(e, ..)| e)
        });
        let target_pos = boss.target.and_then(|e| players.get(e).ok()).map(|(_, b, ..)| b.pos);
        // Height of the target relative to the boss: a step above, it's out of reach.
        let rise = target_pos.map_or(0.0, |tp| tp.y - body.pos.y);
        boss.below_target = if rise > STEP_UP { boss.below_target + 1 } else { 0 };

        // Climbing jump cut short (stagger, death): it falls back onto the nearest floor.
        if boss.flight.is_some() && !action.mv.is_some_and(|mv| is_climb(t, mv)) {
            boss.flight = None;
            body.pos = world::landing(t, body.pos, body.radius, boss.arena);
        }

        if let Some(mv) = action.mv {
            let def = t.get(mv);
            if action.tick < def.total {
                // Jump that lands on the target: distance tracked until take-off.
                if let Some(tp) = target_pos
                    && def.motion.iter().any(|m| m.to_target && m.retarget && action.tick <= m.start)
                {
                    action.target_dist = math::flat_len(tp - body.pos);
                }
                run_frame(&mut body, &action, def, target_pos, None);
                if is_climb(t, mv) {
                    fly(&mut boss, &mut body, def, &action, target_pos, t);
                }
                for h in def.hits.iter().filter(|h| h.aoe && h.start == action.tick) {
                    let (a, _, r) = super::combat::hit_capsule(&body, h, action.tick as f32);
                    events.push(SimEvent::Shockwave { pos: Vec3::new(a.x, body.pos.y, a.z), radius: r, boss: boss.def });
                }
                // The same attack only hits once, whatever its number of spells.
                let volley = sim_id.0.rotate_left(16) ^ action.seq;
                for c in def.casts.iter().filter(|c| c.at == action.tick) {
                    let aim = boss.target.zip(target_pos);
                    spell::cast(&mut commands, t, boss.def, (entity, action.seq), c, &body, aim, volley, &mut rng, &mut events);
                }
                continue;
            }
            match mv {
                MoveRef::Boss(_, BossMove::Death) => {
                    // Stays on the last frame.
                    action.tick = def.total - 1;
                    action.executed = false;
                    continue;
                }
                MoveRef::Boss(_, BossMove::Groggy | BossMove::FatalReceived) => {
                    boss.stagger = 0.0;
                    action.stop();
                    boss.idle = rng.range(bd.idle_ticks[0], bd.idle_ticks[1]) / 2;
                }
                MoveRef::BossAttack(_, i) => {
                    action.stop();
                    let a = &bd.attacks[i as usize];
                    let chained = (!a.next.is_empty()).then(|| a.chained(&bd.attacks, rng.next_f32())).flatten();
                    if let (Some(j), Some(tp)) = (chained, target_pos) {
                        start_attack(&mut boss, &mut body, &mut action, j, tp, t, now, &mut events, entity);
                        continue;
                    }
                    boss.idle = rng.range(bd.idle_ticks[0], bd.idle_ticks[1]);
                }
                _ => {
                    action.stop();
                    boss.idle = bd.idle_ticks[0];
                }
            }
        }

        // Asleep as long as nobody has entered the arena.
        if health.dead() || !encounter.active {
            continue;
        }
        if boss.phase == 1 && (health.cur <= health.max * bd.phase2_at || (partner_fallen && !bd.minor)) {
            boss.phase = 2;
            action.start(MoveRef::Boss(def, BossMove::Roar), 0.0);
            events.push(SimEvent::BossPhase2);
            continue;
        }
        let Some(tp) = target_pos else {
            continue;
        };

        // Out of reach on another level: it jumps there (up, after a while; down, at once).
        // Perched above its target, it waits at the edge for its jump.
        let down = rise < -STEP_UP && world::perched(t, body.pos, boss.arena);
        let waited = boss.below_target;
        let climb = |a: &BossAttack| a.climb.is_some_and(|wait| down || (rise > STEP_UP && waited >= wait));
        let climbing = bd.attacks.iter().any(|a| a.phases.contains(&boss.phase) && climb(a));

        // Approach: turn slowly towards the target, walk or circle around.
        let to = tp - body.pos;
        let dist = math::flat_len(to);
        let want = math::yaw_of(to);
        if bd.heading_slack > 0.0 {
            lumber(&mut boss, &mut body, bd, want, dist, &mut rng);
        } else {
            body.yaw = math::turn_towards(body.yaw, want, bd.turn_rate.to_radians() * DT);
            if down {
                // Waits at the edge of the gallery: it jumps down, it doesn't walk off it.
            } else if bd.keep_away > 0.0 && dist < bd.keep_away {
                // Spellcaster: it backs away facing the target.
                let back = math::forward(body.yaw) * bd.walk_speed * DT;
                body.pos -= back;
            } else if dist > bd.preferred_range + 0.75 {
                let f = math::forward(body.yaw);
                body.pos += f * bd.walk_speed * DT;
            } else {
                if boss.strafe_timer == 0 {
                    boss.strafe = if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
                    boss.strafe_timer = rng.range(90, 180);
                }
                boss.strafe_timer -= 1;
                let r = math::right(body.yaw);
                body.pos += r * boss.strafe * bd.strafe_speed * DT;
            }
        }

        if boss.idle > 0 {
            boss.idle -= 1;
            continue;
        }
        if debug.boss_passive {
            continue;
        }
        let signed = math::wrap(want - body.yaw);
        let angle = signed.abs().to_degrees();
        let side = if signed > 0.0 { Side::Left } else { Side::Right };
        let phase = boss.phase;
        let candidates: Vec<(usize, f32)> = bd
            .attacks
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                if climbing {
                    return a.phases.contains(&phase) && climb(a) && angle <= a.max_angle;
                }
                a.phases.contains(&phase)
                    && a.climb.is_none()
                    && boss.cooldowns[*i] <= now
                    && dist >= a.min_range
                    && dist <= a.max_range
                    && angle <= a.max_angle
                    && angle >= a.min_angle
                    && (a.side == Side::Any || a.side == side)
            })
            .map(|(i, a)| (i, a.weight))
            .collect();
        let total: f32 = candidates.iter().map(|c| c.1).sum();
        if total <= 0.0 {
            continue;
        }
        let mut roll = rng.next_f32() * total;
        let mut chosen = candidates[candidates.len() - 1].0;
        for (i, w) in &candidates {
            if roll < *w {
                chosen = *i;
                break;
            }
            roll -= w;
        }
        start_attack(&mut boss, &mut body, &mut action, chosen, tp, t, now, &mut events, entity);
    }
}

fn is_climb(t: &Tuning, mv: MoveRef) -> bool {
    matches!(mv, MoveRef::BossAttack(d, i) if t.bosses[d as usize].attacks[i as usize].climb.is_some())
}

/// Climbing jump (after `run_frame`): from take-off to landing, the boss rises (or drops) from
/// its floor to its target's, then sets foot on the nearest floor at that height.
fn fly(boss: &mut Boss, body: &mut Body, def: &MoveDef, action: &Action, target: Option<Vec3>, t: &Tuning) {
    let Some(m) = def.motion.iter().find(|m| m.to_target && m.retarget) else { return };
    if action.tick == m.start {
        let to = target.map_or(body.pos.y, |tp| {
            world::floor_below(t, tp.x, tp.z, tp.y, Some(world::Mover::Climber(boss.arena))).unwrap_or(tp.y)
        });
        boss.flight = Some((body.pos.y, to));
    }
    let Some((from, to)) = boss.flight else { return };
    let k = (action.tick + 1 - m.start) as f32 / (m.end - m.start).max(1) as f32;
    body.pos.y = from + (to - from) * k.min(1.0);
    if action.tick + 1 >= m.end {
        boss.flight = None;
        body.pos = world::landing(t, body.pos, body.radius, boss.arena);
    }
}

/// Movement of a large beast: it only turns when its target leaves the
/// `heading_slack` cone, aims roughly in its direction, and only advances when facing it. Up close, it
/// pivots more slowly (its side attacks also make it turn).
fn lumber(boss: &mut Boss, body: &mut Body, bd: &BossDef, want: f32, dist: f32, rng: &mut SimRng) {
    let slack = bd.heading_slack.to_radians();
    if boss.aim_timer == 0 {
        boss.aim_offset = (rng.next_f32() - 0.5) * slack * 0.8;
        boss.aim_timer = rng.range(120, 260);
    }
    boss.aim_timer -= 1;
    let err = math::wrap(want - body.yaw);
    boss.turning |= err.abs() > slack;
    if boss.turning {
        let goal = want + boss.aim_offset;
        let near = dist < bd.radius + 2.5;
        // Up close, it pivots more slowly: the target on its flank has time to take advantage
        // (or to take a tail swipe there), but it ends up facing it.
        let rate = bd.turn_rate.to_radians() * DT * if near { 0.7 } else { 1.0 };
        body.yaw = math::turn_towards(body.yaw, goal, rate);
        boss.turning = math::wrap(goal - body.yaw).abs() > 0.05;
    }
    if dist > bd.preferred_range + 0.75 && err.abs() < 1.0 {
        body.pos += math::forward(body.yaw) * bd.walk_speed * DT;
    }
}

#[allow(clippy::too_many_arguments)]
fn start_attack(
    boss: &mut Boss,
    body: &mut Body,
    action: &mut Action,
    idx: usize,
    target_pos: Vec3,
    t: &Tuning,
    now: u32,
    events: &mut SimEvents,
    entity: Entity,
) {
    let a = &boss.def(t).attacks[idx];
    boss.cooldowns[idx] = now + a.cooldown;
    action.start(MoveRef::BossAttack(boss.def, idx as u16), math::flat_len(target_pos - body.pos));
    if a.mv.hits.iter().any(|h| h.fury) {
        events.push(SimEvent::FuryWarn { entity });
    }
    run_frame(body, action, &a.mv, Some(target_pos), None);
}

pub fn boss_end_tick(tuning: Res<Tuning>, mut q: Query<(&mut Boss, &Action)>) {
    for (mut boss, action) in &mut q {
        let bd = &tuning.bosses[boss.def as usize];
        if action.is(MoveRef::Boss(boss.def, BossMove::Groggy)) {
            continue;
        }
        if boss.stagger_delay > 0 {
            boss.stagger_delay -= 1;
        } else {
            boss.stagger = (boss.stagger - bd.stagger_decay * DT).max(0.0);
        }
    }
}

/// Minimum warning of an area effect (ticks): the circle stays fixed all that time, and the
/// boss stops tracking its target as soon as it appears (`Tuning::parse`).
pub const MIN_WARNING: u32 = 40;

/// Tick from which the spot targeted by area effect `h` no longer moves: end of target
/// tracking, last spells cast, end of a pivot — at the latest `MIN_WARNING` ticks before
/// the impact. A jump landing on its target tracks it until take-off: its flight serves as the warning.
pub fn aoe_lock_tick(def: &MoveDef, h: &super::data::HitWindow) -> u32 {
    let casts = def.casts.iter().map(|c| c.at + 1).filter(|&at| at < h.start).max().unwrap_or(0);
    let jumps = def.motion.iter().filter(|m| m.to_target && m.retarget && m.start < h.start).map(|m| m.start).max();
    // An ongoing pivot also moves the area: it's only fixed afterwards.
    let turns = def.motion.iter().filter(|m| m.turn != 0.0 && m.start < h.start).map(|m| m.end).max().unwrap_or(0);
    match jumps {
        Some(j) => j.max(casts).max(turns).min(h.start),
        None => casts.max(turns).max(def.track_until).min(h.start.saturating_sub(MIN_WARNING)),
    }
}

/// Area effect announced by the current action: ground centre at impact (extrapolating the
/// remaining movement) at the height of the boss's feet (of its landing during a climbing jump:
/// see `Boss::flight`), radius, and wind-up progress (0 → 1 at impact).
/// Used to draw the ground warning; `None` outside the wind-up or after the impact, and
/// as long as the spot isn't fixed (the circle never follows the player).
pub fn aoe_telegraph(body: &Body, action: &Action, t: &Tuning) -> Option<(Vec3, f32, f32)> {
    let def = action.def(t)?;
    let h = def.hits.iter().find(|h| h.aoe && action.tick < h.end)?;
    let from = aoe_lock_tick(def, h);
    if action.tick < from {
        return None;
    }
    let mut pos = body.pos;
    let f = math::forward(body.yaw);
    for tick in action.tick..h.start {
        for m in def.motion.iter().filter(|m| tick >= m.start && tick < m.end) {
            pos += (f * motion_speed(m, action.target_dist) + math::right(body.yaw) * m.side) * DT;
        }
    }
    let at = Body { pos, ..*body };
    let (a, _, r) = super::combat::hit_capsule(&at, h, h.start as f32);
    let progress = ((action.tick - from) as f32 / (h.start - from).max(1) as f32).min(1.0);
    Some((Vec3::new(a.x, body.pos.y, a.z), r, progress))
}

/// True if the boss's current action contains a rage attack not yet triggered.
pub fn fury_pending(action: &Action, t: &Tuning) -> bool {
    action.def(t).is_some_and(|d: &MoveDef| {
        d.hits.iter().any(|h| h.fury && action.tick < h.end)
    })
}

/// True if the current action prepares an unblockable hit that no ground circle
/// announces (rage attack, fire stream): the model glows red, you have to flee.
pub fn unblockable_pending(action: &Action, t: &Tuning, boss: Option<&BossDef>) -> bool {
    if fury_pending(action, t) {
        return true;
    }
    let (Some(d), Some(bd)) = (action.def(t), boss) else { return false };
    d.casts.iter().any(|c| {
        bd.spell(&c.spell).map(|i| &bd.spells[i as usize]).is_some_and(|sd| {
            let open = sd.kind == super::data::SpellKind::Beam || (sd.kind == super::data::SpellKind::Bolt && sd.aoe);
            open && action.tick < c.at + sd.life
        })
    })
}

/// Lockable points of an opponent (world frame, simulation position): the parts
/// of a large boss, otherwise the middle of the body.
pub fn lock_points(t: &Tuning, body: &Body, boss: Option<&Boss>) -> Vec<Vec3> {
    let parts: Vec<Vec3> = boss
        .map(|b| &b.def(t).parts)
        .into_iter()
        .flatten()
        .filter(|p| p.lock)
        .map(|p| math::local_to_world(body.pos, body.yaw, p.at))
        .collect();
    if parts.is_empty() { vec![body.pos + Vec3::Y * body.height * 0.55] } else { parts }
}

/// Index of the `n`-th lockable part in `parts` (to find its model piece).
pub fn lock_part(def: &BossDef, n: u8) -> Option<&super::data::PartDef> {
    def.parts.iter().filter(|p| p.lock).nth(n as usize)
}
