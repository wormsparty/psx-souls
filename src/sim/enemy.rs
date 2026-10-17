//! Path enemies (dogs, puppets…): a simple, readable AI.
//!
//! - at their post, they sleep (you can get close, but they sense everything around
//!   them) or watch (they see far, but only in front of them);
//! - spotting the player or being hit triggers an alert cry, which wakes the whole group;
//! - they chase and attack like the boss (weighted choice, cooldowns, pauses
//!   where you can punish);
//! - too far from their post, they give up, go back and heal;
//! - once defeated, they disappear, sometimes leaving an item behind (`EnemyDef::loot`, one roll per player). They all come back when you
//!   rest or die, except the unique ones.

use bevy::prelude::*;

use super::boss::Boss;
use super::data::{EnemyDef, EnemyMove, MoveRef, Tuning};
use super::encounter::Encounter;
use super::fighter::{Action, Body, Foe, Health, Hitstop, PrevBody, Team, party_hp};
use super::items::{Item, Loot};
use super::player::{PState, Player, run_frame};
use super::rng::SimRng;
use super::world::Zone;
use super::{DT, SimDebug, SimEntity, SimEvent, SimEvents, SimIds, SimTick, math, sim_order, world};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EState {
    /// Asleep at its post.
    Asleep,
    /// Standing at its post, watching ahead.
    Watch,
    Chase,
    /// Returning to its post (it ignores the player unless hit).
    Return,
}

#[derive(Component, Clone, Debug)]
pub struct Enemy {
    /// Type (index in `Tuning::enemies`).
    pub kind: u8,
    /// Index in `level.enemies`.
    pub spawn: u16,
    pub home: Vec3,
    pub home_yaw: f32,
    /// State at its post: `Asleep` or `Watch`.
    pub rest_state: EState,
    pub state: EState,
    pub target: Option<Entity>,
    /// Tick from which each attack becomes available again.
    pub cooldowns: Vec<u32>,
    /// Ticks before it can attack.
    pub idle: u32,
    pub strafe: f32,
    pub strafe_timer: u32,
    /// Damage taken recently (staggered beyond `poise`).
    pub poise: f32,
    pub poise_timer: u32,
    /// Raised the alarm this tick: the group wakes up.
    pub alerted: bool,
    pub group: u8,
    pub unique: bool,
    /// Ticks elapsed since the end of the death animation.
    pub dead_ticks: u32,
    /// Consecutive ticks the target has been out of reach (see `LOSE_TICKS`).
    pub lost_ticks: u32,
    /// Where it stood when it last made headway towards its target.
    pub anchor: Vec3,
}

/// Delay between the end of the death animation and disappearing.
pub const VANISH_TICKS: u32 = 30;
/// Height difference beyond which an enemy doesn't spot the player.
const SIGHT_DY: f32 = 3.0;
/// Half-angle of the field of view of a watching enemy (degrees).
const SIGHT_ANGLE: f32 = 70.0;
/// Within this range, a watching enemy hears the player even behind its back.
const HEAR_RANGE: f32 = 3.0;
/// A chasing enemy whose target stays out of reach (beyond `sight`, or on another level), or
/// that can't get any closer (stuck against a wall, at the foot of stairs it may not climb),
/// this long gives up and goes back to its post: otherwise it would chase forever, out of
/// sight, and forbid resting.
const LOSE_TICKS: u32 = 180;
/// Distance an approaching enemy must cover to count as making headway.
const HEADWAY: f32 = 1.0;

impl Enemy {
    /// Takes a hit: chases the attacker, alerts the group, and is staggered if the
    /// poise reserve is exceeded (outside hyper armour). True if staggered.
    pub fn take_hit(&mut self, attacker: Entity, amount: f32, action: &mut Action, t: &Tuning) -> bool {
        if self.state != EState::Chase {
            self.alerted = true;
        }
        self.state = EState::Chase;
        self.target = Some(attacker);
        self.lost_ticks = 0;
        self.poise += amount;
        self.poise_timer = 60;
        let def = &t.enemies[self.kind as usize];
        if self.poise >= def.poise && !action.hyperarmor(t) {
            self.poise = 0.0;
            action.start(MoveRef::Enemy(self.kind, EnemyMove::Hit), 0.0);
            action.executed = false;
            return true;
        }
        false
    }
}

/// Creates all the level's enemies (except unique ones already defeated: bits of `slain`), in
/// file order (determinism), sturdier for a party of `players` (`party_hp`).
pub fn spawn_all(commands: &mut Commands, t: &Tuning, slain: u64, players: usize) {
    for (i, s) in t.level.enemies.iter().enumerate() {
        if s.unique && slain & (1u64 << i) != 0 {
            continue;
        }
        let Some(kind) = t.enemy_kind(&s.kind) else { continue };
        let d = &t.enemies[kind as usize];
        let y = world::floor_at(t, s.pos[0], s.pos[1], 0.0).unwrap_or(0.0);
        let pos = Vec3::new(s.pos[0], y, s.pos[1]);
        let yaw = s.yaw.to_radians();
        let rest = if s.asleep { EState::Asleep } else { EState::Watch };
        commands.spawn((
            SimEntity,
            Team::Enemies,
            Foe,
            Body { pos, yaw, radius: d.radius, height: d.height, mass: d.mass },
            PrevBody { pos, yaw },
            Health::new(d.max_hp * party_hp(players, false)),
            Hitstop::default(),
            Action::default(),
            Enemy {
                kind,
                spawn: i as u16,
                home: pos,
                home_yaw: yaw,
                rest_state: rest,
                state: rest,
                target: None,
                cooldowns: vec![0; d.attacks.len()],
                idle: 0,
                strafe: 1.0,
                strafe_timer: 0,
                poise: 0.0,
                poise_timer: 0,
                alerted: false,
                group: s.group,
                unique: s.unique,
                dead_ticks: 0,
                lost_ticks: 0,
                anchor: pos,
            },
        ));
    }
}

type PlayerView<'a> = (Entity, &'a Body, &'a Health, &'a Player);

/// A player they can go after: alive, on their feet, in the level (not in a boss's arena).
fn hunts(p: &PlayerView) -> bool {
    !p.2.dead() && !matches!(p.3.state, PState::Dead | PState::Falling) && p.3.zone == Zone::Level
}

/// The nearest player this enemy spots from its post.
fn spot<'a>(e: &Enemy, body: &Body, sight: f32, players: impl Iterator<Item = PlayerView<'a>>) -> Option<Entity> {
    players
        .filter(hunts)
        .filter(|(_, pb, ..)| {
            let to = pb.pos - body.pos;
            let d = math::flat_len(to);
            if (pb.pos.y - body.pos.y).abs() > SIGHT_DY {
                return false;
            }
            match e.state {
                EState::Asleep => d <= sight * 0.5,
                _ => {
                    let ang = math::wrap(math::yaw_of(to) - body.yaw).abs().to_degrees();
                    d <= HEAR_RANGE || (d <= sight && ang <= SIGHT_ANGLE)
                }
            }
        })
        .min_by(|a, b| a.1.pos.distance(body.pos).total_cmp(&b.1.pos.distance(body.pos)))
        .map(|(e, ..)| e)
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn enemy_act(
    mut commands: Commands,
    tuning: Res<Tuning>,
    tick: Res<SimTick>,
    debug: Res<SimDebug>,
    mut enc: ResMut<Encounter>,
    mut rng: ResMut<SimRng>,
    mut events: ResMut<SimEvents>,
    mut enemies: Query<
        (Entity, &mut Enemy, &mut Body, &mut Action, &mut Hitstop, &mut Health),
        (Without<Player>, Without<Boss>),
    >,
    players: Query<PlayerView, Without<Enemy>>,
    ids: SimIds,
) {
    let t = &*tuning;
    let now = tick.0;
    let order = sim_order(&ids);
    let mut groups: Vec<u8> = Vec::new();

    let mut it = enemies.iter_many_mut(&order);
    while let Some((entity, mut e, mut body, mut action, mut hitstop, mut health)) = it.fetch_next() {
        e.alerted = false;
        if hitstop.0 > 0 {
            hitstop.0 -= 1;
            continue;
        }
        action.executed = true;
        let d = &t.enemies[e.kind as usize];
        if e.poise_timer > 0 {
            e.poise_timer -= 1;
        } else {
            e.poise = 0.0;
        }

        // Dead: end of the animation, then disappears.
        if health.dead() {
            let death = MoveRef::Enemy(e.kind, EnemyMove::Death);
            if !action.is(death) {
                action.start(death, 0.0);
            }
            if action.tick + 1 >= d.death.total {
                action.tick = d.death.total - 1;
                action.executed = false;
                e.dead_ticks += 1;
                if e.dead_ticks >= VANISH_TICKS {
                    commands.entity(entity).despawn();
                    // A roll for each player (co-op: everyone gets their own drop), by id.
                    let mut ids: Vec<u8> = players.iter().map(|(.., p)| p.id).collect();
                    ids.sort_unstable();
                    for owner in ids {
                        if let Some(item) = roll_loot(d, &mut rng) {
                            commands.spawn((SimEntity, Loot { pos: body.pos, item, owner }));
                        }
                    }
                    events.push(SimEvent::EnemyVanished { pos: body.pos });
                }
            }
            continue;
        }

        let target_pos = e.target.and_then(|p| players.get(p).ok()).filter(hunts).map(|(_, b, ..)| b.pos);

        // Current action.
        if let Some(mv) = action.mv {
            let def = t.get(mv);
            if action.tick < def.total {
                if let Some(tp) = target_pos
                    && def.motion.iter().any(|m| m.to_target && m.retarget && action.tick <= m.start)
                {
                    action.target_dist = math::flat_len(tp - body.pos);
                }
                // The alert cry: it turns towards its target.
                if matches!(mv, MoveRef::Enemy(_, EnemyMove::Alert))
                    && let Some(tp) = target_pos
                {
                    body.yaw = math::turn_towards(body.yaw, math::yaw_of(tp - body.pos), d.turn_rate.to_radians() * DT);
                }
                run_frame(&mut body, &action, def, target_pos, None);
                continue;
            }
            action.stop();
            match mv {
                MoveRef::Enemy(_, EnemyMove::Attack(i)) => {
                    let a = &d.attacks[i as usize];
                    let chained = (!a.next.is_empty()).then(|| a.chained(&d.attacks, rng.next_f32())).flatten();
                    if let (Some(j), Some(tp)) = (chained, target_pos) {
                        start_attack(&mut e, &mut body, &mut action, j, tp, t, now);
                        continue;
                    }
                    e.idle = rng.range(d.idle_ticks[0], d.idle_ticks[1]);
                }
                MoveRef::Enemy(_, EnemyMove::Hit) => e.idle = d.idle_ticks[0] / 2,
                _ => e.idle = 10,
            }
        }

        match e.state {
            EState::Asleep | EState::Watch => {
                if let Some(p) = spot(&e, &body, d.sight, players.iter_many(&order)) {
                    wake(&mut e, &mut action, p, &mut events, entity);
                    groups.push(e.group);
                } else {
                    body.yaw = math::turn_towards(body.yaw, e.home_yaw, d.turn_rate.to_radians() * 0.3 * DT);
                }
            }
            EState::Return => {
                let to = e.home - body.pos;
                let dist = math::flat_len(to);
                if dist < 0.3 {
                    e.state = e.rest_state;
                    e.target = None;
                    health.cur = health.max;
                    continue;
                }
                body.yaw = math::turn_towards(body.yaw, math::yaw_of(to), d.turn_rate.to_radians() * DT);
                let step = math::forward(body.yaw) * d.walk_speed.max(d.run_speed * 0.5).min(dist / DT) * DT;
                body.pos += step;
            }
            EState::Chase => {
                // Target: the previous one if it's still there, otherwise the nearest player.
                let tp = target_pos.or_else(|| {
                    let p = players
                        .iter_many(&order)
                        .filter(hunts)
                        .min_by(|a, b| a.1.pos.distance(body.pos).total_cmp(&b.1.pos.distance(body.pos)))?;
                    e.target = Some(p.0);
                    Some(p.1.pos)
                });
                let out_of_reach = tp.is_some_and(|tp| {
                    let dist = math::flat_len(tp - body.pos);
                    let stuck = dist > d.preferred_range + 0.6 && math::flat_len(body.pos - e.anchor) < HEADWAY;
                    stuck || dist > d.sight || (tp.y - body.pos.y).abs() > SIGHT_DY
                });
                if out_of_reach {
                    e.lost_ticks += 1;
                } else {
                    e.lost_ticks = 0;
                    e.anchor = body.pos;
                }
                let too_far = math::flat_len(body.pos - e.home) > d.leash || e.lost_ticks >= LOSE_TICKS;
                let Some(tp) = tp.filter(|_| !too_far) else {
                    e.state = EState::Return;
                    e.target = None;
                    e.lost_ticks = 0;
                    continue;
                };
                let to = tp - body.pos;
                let dist = math::flat_len(to);
                let want = math::yaw_of(to);
                body.yaw = math::turn_towards(body.yaw, want, d.turn_rate.to_radians() * DT);
                if dist > d.preferred_range + 0.6 {
                    // Jogging from afar, walking for the final approach.
                    let speed = if dist > d.preferred_range + 2.5 { d.run_speed } else { d.walk_speed * 1.4 };
                    let step = math::forward(body.yaw) * speed * DT;
                    body.pos += step;
                } else {
                    if e.strafe_timer == 0 {
                        e.strafe = if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
                        e.strafe_timer = rng.range(60, 150);
                    }
                    e.strafe_timer -= 1;
                    let step = math::right(body.yaw) * e.strafe * d.walk_speed * 0.6 * DT;
                    body.pos += step;
                }
                if e.idle > 0 {
                    e.idle -= 1;
                    continue;
                }
                if debug.boss_passive {
                    continue;
                }
                let angle = math::wrap(want - body.yaw).abs().to_degrees();
                let candidates: Vec<(usize, f32)> = d
                    .attacks
                    .iter()
                    .enumerate()
                    .filter(|(i, a)| {
                        e.cooldowns[*i] <= now && dist >= a.min_range && dist <= a.max_range && angle <= a.max_angle
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
                start_attack(&mut e, &mut body, &mut action, chosen, tp, t, now);
            }
        }
    }

    // Alert: the rest of the group wakes up (those hit this tick also raise the alarm).
    for (_, e, ..) in enemies.iter_many(&order) {
        if e.alerted && e.group > 0 && !groups.contains(&e.group) {
            groups.push(e.group);
        }
    }
    let mut hunted = false;
    let mut it = enemies.iter_many_mut(&order);
    while let Some((entity, mut e, body, mut action, _, health)) = it.fetch_next() {
        if health.dead() {
            continue;
        }
        if e.group > 0 && groups.contains(&e.group) && matches!(e.state, EState::Asleep | EState::Watch) {
            let target = players
                .iter_many(&order)
                .filter(hunts)
                .min_by(|a, b| a.1.pos.distance(body.pos).total_cmp(&b.1.pos.distance(body.pos)))
                .map(|p| p.0);
            if let Some(p) = target {
                wake(&mut e, &mut action, p, &mut events, entity);
            }
        }
        hunted |= e.state == EState::Chase;
    }
    enc.hunted = hunted;
}

fn wake(e: &mut Enemy, action: &mut Action, target: Entity, events: &mut SimEvents, entity: Entity) {
    e.state = EState::Chase;
    e.target = Some(target);
    e.alerted = true;
    e.lost_ticks = 0;
    if action.mv.is_none() {
        action.start(MoveRef::Enemy(e.kind, EnemyMove::Alert), 0.0);
        events.push(SimEvent::EnemyAlert { entity });
    }
}

fn start_attack(e: &mut Enemy, body: &mut Body, action: &mut Action, idx: usize, target_pos: Vec3, t: &Tuning, now: u32) {
    let a = &t.enemies[e.kind as usize].attacks[idx];
    e.cooldowns[idx] = now + a.cooldown;
    action.start(MoveRef::Enemy(e.kind, EnemyMove::Attack(idx as u8)), math::flat_len(target_pos - body.pos));
    run_frame(body, action, &a.mv, Some(target_pos), None);
}

/// What a defeated enemy leaves behind: a single roll over its loot table.
fn roll_loot(d: &EnemyDef, rng: &mut SimRng) -> Option<Item> {
    if d.loot.is_empty() {
        return None;
    }
    let mut r = rng.next_f32();
    for l in &d.loot {
        if r < l.chance {
            return Some(l.item);
        }
        r -= l.chance;
    }
    None
}

/// Rest, travel: all enemies return to their post (except defeated unique ones).
pub fn respawn_on_request(
    mut commands: Commands,
    tuning: Res<Tuning>,
    mut enc: ResMut<Encounter>,
    enemies: Query<Entity, With<Enemy>>,
    players: Query<&Player>,
) {
    if !enc.respawn_enemies {
        return;
    }
    enc.respawn_enemies = false;
    for e in &enemies {
        commands.entity(e).despawn();
    }
    let slain = players.iter().min_by_key(|p| p.id).map_or(0, |p| p.slain);
    spawn_all(&mut commands, &tuning, slain, players.iter().count());
}
