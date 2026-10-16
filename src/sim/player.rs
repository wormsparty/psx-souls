//! Player state machine.

use bevy::prelude::*;

use super::boss::Boss;
use super::data::{BossMove, MoveRef, PlayerMove, Tuning, WeaponMove};
use super::encounter::{self, Encounter, near_checkpoint, near_loot, near_pickup};
use super::fighter::{Action, Body, Foe, Health, Hitstop};
use super::input::{InputBuffer, PlayerInputs, btn};
use super::items::{self, Inventory, Item, Loot};
use super::world::Zone;
use super::{DT, SimEvent, SimEvents, SimIds, SimTick, math, sim_order};

/// Gravity during a fall (m/s²).
pub const GRAVITY: f32 = 22.0;
/// A fall into the void is fatal after this time.
pub const FALL_DEATH_TICKS: u32 = 45;
/// An opponent further than this (or too high / too low) isn't an automatic target.
const AUTO_TARGET_RANGE: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PState {
    /// Free movement.
    Free,
    /// Guard held.
    Guard,
    /// Heavy attack being charged.
    Charging,
    /// An action (`Action::mv`) is in progress.
    Acting,
    /// Gone over the edge: no more control, death at the end of the fall.
    Falling,
    Dead,
}

#[derive(Component, Clone, Debug)]
pub struct Player {
    pub id: u8,
    pub state: PState,
    pub weapon: u8,
    /// Index of the next light attack in the combo.
    pub combo: u8,
    /// Until this tick, a light attack continues the combo after the previous one ended.
    pub combo_until: u32,
    pub stamina: f32,
    pub stamina_delay: u32,
    /// HP recoverable by hitting ("regain").
    pub regain: f32,
    pub regain_timer: u32,
    /// Special attack gauge.
    pub special: f32,
    pub guard_start: u32,
    pub guard_held: bool,
    pub last_guard_press: u32,
    pub guard_spam: u8,
    pub charge: u32,
    pub lock: Option<Entity>,
    /// Locked point of the target (parts of a large boss: head, legs…).
    pub lock_part: u8,
    /// Last spell volley that hit them (`Spell::volley`): it only hits them once.
    pub volley: u32,
    pub vel: Vec3,
    pub sprinting: bool,
    /// Sprint started by `btn::SPRINT`, active until the player stops.
    pub sprint_latched: bool,
    pub dodge_held: u32,
    pub buffer: InputBuffer,
    /// The weapon change of the current `Switch` action has already been applied.
    pub switched: bool,
    pub inventory: Inventory,
    /// The heal of the current `Heal` action has already been applied.
    pub healed: bool,
    /// Ticks spent in the `Dead` state (respawn after `RESPAWN_TICKS`).
    pub dead_ticks: u32,
    /// Item being used (`Heal` action, shared by all consumables).
    pub using: Option<Item>,
    /// Consumable effects, in ticks: regeneration (moss), flaming weapon (resin), protection
    /// (ash), faster stamina recovery (root).
    pub regen_ticks: u32,
    pub resin_ticks: u32,
    pub ward_ticks: u32,
    pub vigor_ticks: u32,
    /// Last checkpoint rested at, discovered checkpoints (bits).
    pub checkpoint: u8,
    pub found: u32,
    /// Picked-up items, defeated unique enemies (bits, see `Progress`).
    pub picked: u64,
    pub slain: u64,
    /// Fall in progress (the body keeps falling after death, until the respawn).
    pub falling: bool,
    pub fall_vy: f32,
    pub fall_ticks: u32,
    /// Height of the floor left behind (the camera doesn't go lower than that).
    pub fall_from: f32,
    /// In the air (jump): vertical speed, height of the floor left behind, elapsed ticks.
    pub airborne: bool,
    pub air_vy: f32,
    pub air_from: f32,
    pub air_ticks: u32,
    /// Where they are: the level or a boss arena (they went through its fog).
    pub zone: Zone,
    /// Number of the last menu decision applied (`PlayerInput::cmd`).
    pub cmd_seq: u8,
}

impl Player {
    pub fn new(id: u8, t: &Tuning) -> Self {
        Self {
            id,
            state: PState::Free,
            weapon: 0,
            combo: 0,
            combo_until: 0,
            stamina: t.player.max_stamina,
            stamina_delay: 0,
            regain: 0.0,
            regain_timer: 0,
            special: 0.0,
            guard_start: 0,
            guard_held: false,
            last_guard_press: 0,
            guard_spam: 0,
            charge: 0,
            lock: None,
            lock_part: 0,
            volley: 0,
            vel: Vec3::ZERO,
            sprinting: false,
            sprint_latched: false,
            dodge_held: 0,
            buffer: InputBuffer::default(),
            switched: false,
            inventory: Inventory::new_game(t),
            healed: false,
            dead_ticks: 0,
            using: None,
            regen_ticks: 0,
            resin_ticks: 0,
            ward_ticks: 0,
            vigor_ticks: 0,
            checkpoint: 0,
            found: 1,
            picked: 0,
            slain: 0,
            falling: false,
            fall_vy: 0.0,
            fall_ticks: 0,
            fall_from: 0.0,
            airborne: false,
            air_vy: 0.0,
            air_from: 0.0,
            air_ticks: 0,
            zone: Zone::Level,
            cmd_seq: 0,
        }
    }

    /// Weapon damage multiplier (ember resin).
    pub fn damage_mult(&self) -> f32 {
        if self.resin_ticks > 0 { items::RESIN_DAMAGE } else { 1.0 }
    }

    /// Damage taken multiplier (talisman, warding ash).
    pub fn defense_mult(&self) -> f32 {
        let brooch = if self.inventory.wears(Item::IronBrooch) { items::BROOCH_DAMAGE } else { 1.0 };
        let ash = if self.ward_ticks > 0 { items::ASH_DAMAGE } else { 1.0 };
        brooch * ash
    }

    /// Starts a fall (the body is already above the void; `from`: last position on the ground).
    pub fn start_fall(&mut self, action: &mut Action, momentum: Vec3, from: Vec3) {
        self.falling = true;
        self.airborne = false;
        self.state = PState::Falling;
        self.fall_vy = 0.0;
        self.fall_ticks = 0;
        self.fall_from = from.y;
        let m = Vec3::new(momentum.x, 0.0, momentum.z);
        let len = math::flat_len(m);
        self.vel = if len > 7.0 { m / len * 7.0 } else { m };
        self.lock = None;
        self.charge = 0;
        self.sprinting = false;
        self.sprint_latched = false;
        action.stop();
    }

    /// At least 1 stamina point is needed to attack, dodge or use the special.
    pub fn can_act(&self) -> bool {
        self.stamina >= 1.0
    }

    /// Effective perfect guard window (reduced if guard is spammed).
    pub fn perfect_window(&self, t: &Tuning) -> u32 {
        let g = &t.player.guard;
        g.perfect_window
            .saturating_sub(self.guard_spam as u32 * g.spam_penalty)
            .max(g.min_window)
    }

    pub fn spend_stamina(&mut self, amount: f32, t: &Tuning) {
        if amount > 0.0 {
            // Stamina can go negative: you'll have to wait for it to come back up.
            self.stamina = (self.stamina - amount).max(t.player.stamina_floor);
            self.stamina_delay = t.player.stamina_delay;
        }
    }

    pub fn special_max(&self, t: &Tuning) -> f32 {
        t.player.special_max
    }

    /// Full gauge: the special attack can be used.
    pub fn special_ready(&self, t: &Tuning) -> bool {
        self.special >= self.special_max(t)
    }
}

/// Info on the locked target or the nearest boss, extracted before mutating the player.
#[derive(Clone, Copy)]
struct TargetInfo {
    entity: Entity,
    pos: Vec3,
    yaw: f32,
    radius: f32,
    /// Staggered boss (its definition): the fatal blow can be dealt.
    groggy: Option<u8>,
}

/// Opponents (bosses and enemies) as seen by the player.
type Foes<'w, 's> = Query<'w, 's, (Entity, &'static Body, &'static mut Action, &'static Health, Option<&'static Boss>), (With<Foe>, Without<Player>)>;

/// Distance to an opponent, measured from the edge of its body (large bosses).
fn foe_dist(b: &Body, pos: Vec3) -> f32 {
    (b.pos.distance(pos) - b.radius).max(0.0)
}

/// Nearest living opponent in range (and at roughly the same height). `order`: the entities
/// in `SimId` order (ties go to the oldest).
fn nearest_foe(foes: &Foes, order: &[Entity], pos: Vec3, range: f32) -> Option<Entity> {
    foes.iter_many(order)
        .filter(|(_, b, _, h, _)| !h.dead() && foe_dist(b, pos) <= range && (b.pos.y - pos.y).abs() < 4.0)
        .min_by(|a, b| foe_dist(a.1, pos).total_cmp(&foe_dist(b.1, pos)))
        .map(|(e, ..)| e)
}

/// Lockable points in range: (opponent, point, position).
fn lock_candidates(t: &Tuning, foes: &Foes, order: &[Entity], pos: Vec3, range: f32) -> Vec<(Entity, u8, Vec3)> {
    let mut out = Vec::new();
    for (e, b, _, h, boss) in foes.iter_many(order) {
        if h.dead() || foe_dist(b, pos) > range || (b.pos.y - pos.y).abs() > 4.0 {
            continue;
        }
        for (i, p) in super::boss::lock_points(t, b, boss).into_iter().enumerate() {
            out.push((e, i as u8, p));
        }
    }
    out
}

/// Lock-on: the point closest to the camera axis (failing that, the nearest).
fn pick_lock(t: &Tuning, foes: &Foes, order: &[Entity], pos: Vec3, cam_yaw: f32, range: f32) -> Option<(Entity, u8)> {
    let score = |p: Vec3| {
        let ang = math::wrap(math::yaw_of(p - pos) - cam_yaw).abs();
        ang * 6.0 + math::flat_len(p - pos) * 0.15
    };
    lock_candidates(t, foes, order, pos, range)
        .into_iter()
        .min_by(|a, b| score(a.2).total_cmp(&score(b.2)))
        .map(|(e, i, _)| (e, i))
}

/// Target switch: the next lockable point to the left (`dir` > 0) or right.
#[allow(clippy::too_many_arguments)]
fn switch_lock(t: &Tuning, foes: &Foes, order: &[Entity], pos: Vec3, cur: (Entity, u8), cur_pos: Vec3, dir: f32, range: f32) -> Option<(Entity, u8)> {
    let base = math::yaw_of(cur_pos - pos);
    lock_candidates(t, foes, order, pos, range)
        .into_iter()
        .filter(|(e, i, _)| (*e, *i) != cur)
        .map(|(e, i, p)| (e, i, math::wrap(math::yaw_of(p - pos) - base) * dir))
        .filter(|(.., d)| *d > 0.01)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(e, i, _)| (e, i))
}

/// Target switch upwards (`dir` > 0) or downwards: the nearest higher (or lower) point,
/// preferably in the same direction.
#[allow(clippy::too_many_arguments)]
fn switch_lock_vertical(t: &Tuning, foes: &Foes, order: &[Entity], pos: Vec3, cur: (Entity, u8), cur_pos: Vec3, dir: f32, range: f32) -> Option<(Entity, u8)> {
    let base = math::yaw_of(cur_pos - pos);
    lock_candidates(t, foes, order, pos, range)
        .into_iter()
        .filter(|(e, i, _)| (*e, *i) != cur)
        .map(|(e, i, p)| (e, i, (p.y - cur_pos.y) * dir, math::wrap(math::yaw_of(p - pos) - base).abs()))
        .filter(|(_, _, dy, _)| *dy > 0.3)
        .min_by(|a, b| (a.2 + a.3 * 4.0).total_cmp(&(b.2 + b.3 * 4.0)))
        .map(|(e, i, ..)| (e, i))
}

/// Position of the locked point.
fn lock_pos(t: &Tuning, foes: &Foes, e: Entity, part: u8) -> Option<Vec3> {
    let (_, b, _, h, boss) = foes.get(e).ok()?;
    if h.dead() {
        return None;
    }
    let pts = super::boss::lock_points(t, b, boss);
    pts.get(part as usize).or(pts.first()).copied()
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn player_act(
    mut commands: Commands,
    tuning: Res<Tuning>,
    inputs: Res<PlayerInputs>,
    tick: Res<SimTick>,
    mut encounter: ResMut<Encounter>,
    mut events: ResMut<SimEvents>,
    mut players: Query<
        (Entity, &mut Player, &mut Body, &mut Action, &mut Hitstop, &mut Health),
        Without<Foe>,
    >,
    mut foes: Foes,
    loot: Query<(Entity, &Loot)>,
    ids: SimIds,
) {
    let t = &*tuning;
    let now = tick.0;
    let pd = &t.player;
    let order = sim_order(&ids);
    // In creation order (determinism, if two players reach for the same item).
    let mut loot: Vec<(Entity, Loot)> = loot.iter_many(&order).map(|(e, l)| (e, *l)).collect();

    let mut it = players.iter_many_mut(&order);
    while let Some((entity, mut p, mut body, mut action, mut hitstop, mut health)) = it.fetch_next() {
        let inp = inputs.0[p.id as usize % inputs.0.len()];
        let pressed = p.buffer.update(inp.buttons, now);
        p.guard_held = inp.held(btn::GUARD);
        p.dodge_held = if inp.held(btn::DODGE) { p.dodge_held + 1 } else { 0 };
        if pressed & btn::SPRINT != 0 {
            p.sprint_latched = true;
        }

        const STAMINA_BTNS: u16 = btn::LIGHT | btn::HEAVY | btn::DODGE | btn::SPECIAL;
        if pressed & STAMINA_BTNS != 0 && !p.can_act() && p.state != PState::Dead {
            events.push(SimEvent::NoStamina { entity });
        }

        if pressed & btn::GUARD != 0 {
            p.guard_spam = if now.saturating_sub(p.last_guard_press) < pd.guard.spam_window {
                (p.guard_spam + 1).min(8)
            } else {
                0
            };
            p.last_guard_press = now;
            // Pressing guard again during an impact while guarding restarts the perfect guard window.
            let in_guard_move = action.is(MoveRef::Player(PlayerMove::GuardHit))
                || action.is(MoveRef::Player(PlayerMove::PerfectGuard));
            if p.state == PState::Guard || in_guard_move {
                p.guard_start = now;
            }
        }

        // Lock-on (lost when the player or the target dies, or if it's too far away).
        let lockable = |e: Entity| {
            foes.get(e).is_ok_and(|(_, b, _, h, _)| !h.dead() && foe_dist(b, body.pos) <= pd.lock_range * 1.3)
        };
        if health.dead() || p.lock.is_some_and(|e| !lockable(e)) {
            p.lock = None;
        }
        let cam_yaw = inp.cam_yaw_rad();
        if pressed & btn::LOCK != 0 && !health.dead() && !p.falling {
            let pick = if p.lock.is_some() { None } else { pick_lock(t, &foes, &order, body.pos, cam_yaw, pd.lock_range) };
            p.lock = pick.map(|x| x.0);
            p.lock_part = pick.map_or(0, |x| x.1);
        }
        // Target switch (right stick, mouse): next point to the left, right, higher
        // or lower.
        const TARGET_BTNS: u16 = btn::TARGET_LEFT | btn::TARGET_RIGHT | btn::TARGET_UP | btn::TARGET_DOWN;
        if let Some(cur) = p.lock
            && pressed & TARGET_BTNS != 0
            && let Some(cur_pos) = lock_pos(t, &foes, cur, p.lock_part)
        {
            let next = if pressed & (btn::TARGET_UP | btn::TARGET_DOWN) != 0 {
                let dir = if pressed & btn::TARGET_UP != 0 { 1.0 } else { -1.0 };
                switch_lock_vertical(t, &foes, &order, body.pos, (cur, p.lock_part), cur_pos, dir, pd.lock_range)
            } else {
                let dir = if pressed & btn::TARGET_LEFT != 0 { 1.0 } else { -1.0 };
                switch_lock(t, &foes, &order, body.pos, (cur, p.lock_part), cur_pos, dir, pd.lock_range)
            };
            if let Some((e, i)) = next {
                p.lock = Some(e);
                p.lock_part = i;
            }
        }

        let target = p
            .lock
            .or_else(|| nearest_foe(&foes, &order, body.pos, AUTO_TARGET_RANGE))
            .and_then(|e| foes.get(e).ok())
            .map(|(e, b, a, _, boss)| TargetInfo {
                entity: e,
                pos: b.pos,
                yaw: b.yaw,
                radius: b.radius,
                groggy: boss.filter(|b| a.is(MoveRef::Boss(b.def, BossMove::Groggy))).map(|b| b.def),
            });
        // Aim at the locked point (the head, a leg…), not the centre of the body.
        let locked_pos = p.lock.and_then(|e| lock_pos(t, &foes, e, p.lock_part));

        // Movement direction in world space, relative to the camera.
        let stick = inp.stick();
        let stick_len = stick.length().min(1.0);
        let cam = inp.cam_yaw_rad();
        let move_dir = (stick_len > 0.15).then(|| {
            (math::right(cam) * stick.x + math::forward(cam) * stick.y).normalize_or_zero()
        });

        // Fall: no more control; the body keeps falling, even after death.
        if p.falling {
            p.fall_vy -= GRAVITY * DT;
            let v = p.vel + Vec3::Y * p.fall_vy;
            if body.pos.y > p.fall_from - 80.0 {
                body.pos += v * DT;
            }
            p.fall_ticks += 1;
            if p.state == PState::Falling && p.fall_ticks >= FALL_DEATH_TICKS {
                health.cur = 0.0;
                p.state = PState::Dead;
                events.push(SimEvent::PlayerDied { entity });
            }
            continue;
        }
        // Jump: gravity applies whatever the state (hit, or even killed, mid-jump).
        // Landing (or falling into the void) is decided with the collisions.
        // A leaping attack (`Motion::lift`) drives the height itself, frozen during hitstop.
        let lift = action.def(t).filter(|_| p.state == PState::Acting).and_then(|d| d.lift_at(action.tick));
        match lift {
            Some(_) if hitstop.0 > 0 => {}
            Some(vy) if vy > 0.0 || p.airborne => {
                if !p.airborne {
                    p.airborne = true;
                    p.air_from = body.pos.y;
                    p.air_ticks = 0;
                    p.vel = Vec3::ZERO;
                }
                p.air_vy = vy;
                body.pos.y += vy * DT;
                p.air_ticks += 1;
            }
            _ if p.airborne => {
                p.air_vy -= GRAVITY * DT;
                body.pos.y += p.air_vy * DT;
                p.air_ticks += 1;
            }
            _ => {}
        }
        if p.state == PState::Dead {
            continue;
        }
        if pressed & btn::NEXT_ITEM != 0 && !health.dead() {
            let before = p.inventory.active;
            p.inventory.cycle();
            if p.inventory.active != before {
                events.push(SimEvent::ItemCycled { entity });
            }
        }
        if hitstop.0 > 0 {
            hitstop.0 -= 1;
            continue;
        }
        action.executed = true;

        let mut ctx = Ctx {
            t,
            now,
            entity,
            move_dir,
            locked_pos,
            target,
            events: &mut events,
            loot: &mut loot,
            commands: &mut commands,
        };

        if p.state == PState::Acting {
            let mv = action.mv.expect("Acting without an action");
            let def = t.get(mv);
            if action.tick >= def.total {
                action.stop();
                if mv == MoveRef::Player(PlayerMove::Death) {
                    p.state = PState::Dead;
                    continue;
                }
                if matches!(mv, MoveRef::Weapon(_, WeaponMove::Light(_))) {
                    p.combo_until = now + 12;
                }
                let from_guard = matches!(
                    mv,
                    MoveRef::Player(PlayerMove::GuardHit | PlayerMove::PerfectGuard)
                );
                if p.guard_held {
                    if matches!(mv, MoveRef::Weapon(..)) {
                        // Guard held through an attack: the perfect window counts from the press,
                        // so it's long gone unless the press came at the very end.
                        p.guard_start = p.last_guard_press;
                    } else if !from_guard {
                        p.guard_start = now;
                    }
                    p.state = PState::Guard;
                } else {
                    p.state = PState::Free;
                }
            } else {
                if mv == MoveRef::Player(PlayerMove::Switch)
                    && !p.switched
                    && action.tick >= pd.switch_at
                {
                    p.switched = true;
                    p.weapon = (p.weapon + 1) % t.weapons.len() as u8;
                    p.combo = 0;
                    ctx.events.push(SimEvent::WeaponSwitched { entity });
                }
                if mv == MoveRef::Player(PlayerMove::Heal) && !p.healed && action.tick >= pd.heal_at {
                    p.healed = true;
                    use_item_effect(&mut p, &mut health, entity, &mut ctx);
                }
                let can_cancel = action.tick >= def.cancel_tick();
                let can_chain = action.tick >= def.chain_tick();
                let interrupted = (can_cancel
                    && try_defensive(&mut p, &mut body, &mut action, &mut ctx))
                    || (can_chain
                        && (try_item(&mut p, &mut body, &mut action, &mut ctx)
                            || try_offensive(&mut p, &mut body, &mut action, &mut foes, &mut ctx)));
                if !interrupted {
                    if p.airborne {
                        // Jump attack: the jump's momentum carries on.
                        body.pos += p.vel * DT;
                    }
                    run_frame(&mut body, &action, def, locked_pos, move_dir);
                    if let Some(h) = def.hits.iter().find(|h| h.slash && h.start == action.tick) {
                        let c = &h.capsule;
                        ctx.events.push(SimEvent::Slash {
                            pos: body.pos,
                            yaw: body.yaw,
                            reach: c.a[2].max(c.b[2]) + c.r,
                            top: c.a[1].max(c.b[1]) + c.r,
                        });
                    }
                    if def.walk > 0.0 {
                        // Slow walking allowed (heal).
                        if let Some(d) = move_dir {
                            body.pos += d * def.walk * stick_len * DT;
                            let want = locked_pos.map(|tp| math::yaw_of(tp - body.pos)).unwrap_or(math::yaw_of(d));
                            body.yaw = math::turn_towards(body.yaw, want, pd.turn_rate.to_radians() * 0.5 * DT);
                        }
                    }
                    continue;
                }
            }
        }

        match p.state {
            PState::Acting => {
                // A new action has just started: run its first frame.
                let def = action.def(t).expect("action");
                run_frame(&mut body, &action, def, locked_pos, move_dir);
            }
            PState::Charging => {
                p.charge += 1;
                let w = &t.weapons[p.weapon as usize];
                if p.buffer.buffered(btn::DODGE, now, pd.input_buffer) && p.can_act() {
                    p.charge = 0;
                    try_defensive(&mut p, &mut body, &mut action, &mut ctx);
                } else if !inp.held(btn::HEAVY) || p.charge >= w.charge_ticks {
                    // Released before the full charge: normal heavy; otherwise, fires on its own.
                    let wm = if p.charge >= w.charge_ticks {
                        WeaponMove::HeavyCharged
                    } else {
                        WeaponMove::Heavy
                    };
                    p.charge = 0;
                    let mv = MoveRef::Weapon(p.weapon, wm);
                    start_move(&mut p, &mut body, &mut action, mv, &mut ctx);
                    let def = action.def(t).expect("action");
                    run_frame(&mut body, &action, def, locked_pos, move_dir);
                } else {
                    let desired = locked_pos
                        .map(|tp| math::yaw_of(tp - body.pos))
                        .or(move_dir.map(math::yaw_of));
                    if let Some(y) = desired {
                        body.yaw = math::turn_towards(body.yaw, y, pd.turn_rate.to_radians() * 0.5 * DT);
                    }
                }
            }
            PState::Free | PState::Guard => {
                if !p.guard_held && p.state == PState::Guard {
                    p.state = PState::Free;
                }
                if p.airborne {
                    // In the air: only nudge the trajectory a little, or strike.
                    if try_jump_attack(&mut p, &mut body, &mut action, &mut ctx) {
                        body.pos += p.vel * DT;
                        let def = action.def(t).expect("action");
                        run_frame(&mut body, &action, def, locked_pos, move_dir);
                    } else {
                        air_control(&mut p, &mut body, stick_len, move_dir, t);
                    }
                } else if try_interact(&mut p, &mut body, &mut health, &mut encounter, &mut ctx) {
                    // Rest, item picked up, fog crossed…: nothing else this tick (nor a jump,
                    // the gamepad sends it on the same button).
                    p.buffer.consume(btn::JUMP);
                } else if try_jump(&mut p, &mut body, &mut ctx) {
                    air_control(&mut p, &mut body, stick_len, move_dir, t);
                } else if try_defensive(&mut p, &mut body, &mut action, &mut ctx)
                    && p.state == PState::Acting
                {
                    let def = action.def(t).expect("action");
                    run_frame(&mut body, &action, def, locked_pos, move_dir);
                } else if try_item(&mut p, &mut body, &mut action, &mut ctx)
                    || try_offensive(&mut p, &mut body, &mut action, &mut foes, &mut ctx)
                {
                    if p.state == PState::Acting {
                        let def = action.def(t).expect("action");
                        run_frame(&mut body, &action, def, locked_pos, move_dir);
                    }
                } else {
                    locomotion(&mut p, &mut body, stick_len, move_dir, locked_pos, t);
                }
            }
            PState::Dead | PState::Falling => {}
        }
    }
}

struct Ctx<'a, 'w, 'cw, 'cs> {
    t: &'a Tuning,
    now: u32,
    entity: Entity,
    move_dir: Option<Vec3>,
    locked_pos: Option<Vec3>,
    target: Option<TargetInfo>,
    events: &'a mut ResMut<'w, SimEvents>,
    /// Loot still on the ground (taken out as it's picked up).
    loot: &'a mut Vec<(Entity, Loot)>,
    commands: &'a mut Commands<'cw, 'cs>,
}

/// Dodge or guard. Returns true if the state changed.
fn try_defensive(p: &mut Player, body: &mut Body, action: &mut Action, ctx: &mut Ctx) -> bool {
    let pd = &ctx.t.player;
    if p.buffer.buffered(btn::DODGE, ctx.now, pd.input_buffer) && p.can_act() {
        p.buffer.consume(btn::DODGE);
        p.charge = 0;
        let mv = if let Some(d) = ctx.move_dir {
            body.yaw = math::yaw_of(d);
            PlayerMove::Dodge
        } else {
            PlayerMove::Backstep
        };
        start_move(p, body, action, MoveRef::Player(mv), ctx);
        ctx.events.push(SimEvent::Dodge { entity: ctx.entity });
        return true;
    }
    // No guard during an attack: only once it's over (dodge can still cancel it).
    let attacking = matches!(action.mv, Some(MoveRef::Weapon(..)));
    if p.guard_held && p.state != PState::Guard && !attacking {
        // Cancelling a guard reaction while still holding: same guard, no new perfect window.
        let from_guard = action.is(MoveRef::Player(PlayerMove::GuardHit))
            || action.is(MoveRef::Player(PlayerMove::PerfectGuard));
        if action.mv.is_some() {
            action.stop();
        }
        p.charge = 0;
        p.state = PState::Guard;
        if !from_guard {
            p.guard_start = ctx.now;
        }
        return true;
    }
    false
}

/// Attacks, special, weapon change. Returns true if an action started.
fn try_offensive(p: &mut Player, body: &mut Body, action: &mut Action, foes: &mut Foes, ctx: &mut Ctx) -> bool {
    let t = ctx.t;
    let pd = &t.player;
    let buf = pd.input_buffer;
    let now = ctx.now;
    if !p.can_act() {
        return false;
    }
    let w = p.weapon;
    let wd = &t.weapons[w as usize];

    if p.buffer.buffered(btn::LIGHT, now, buf) {
        p.buffer.consume(btn::LIGHT);
        // Fatal blow on a staggered boss, from the front and in range.
        if let Some((ti, def)) = ctx.target.and_then(|ti| ti.groggy.map(|d| (ti, d))) {
            let to_player = body.pos - ti.pos;
            let dist = math::flat_len(to_player);
            let ang = math::wrap(math::yaw_of(to_player) - ti.yaw).abs();
            if dist <= pd.fatal_range + ti.radius && ang <= pd.fatal_arc.to_radians() {
                if let Ok((_, _, mut bact, _, _)) = foes.get_mut(ti.entity) {
                    bact.start(MoveRef::Boss(def, BossMove::FatalReceived), 0.0);
                    bact.executed = true;
                }
                body.pos = ti.pos + math::forward(ti.yaw) * (ti.radius + body.radius + 0.35);
                body.yaw = math::yaw_of(ti.pos - body.pos);
                start_move(p, body, action, MoveRef::Weapon(w, WeaponMove::Fatal), ctx);
                ctx.events.push(SimEvent::Fatal { pos: ti.pos + Vec3::Y * 1.5 });
                return true;
            }
        }
        let idx = match action.mv {
            Some(MoveRef::Weapon(pw, WeaponMove::Light(i))) if pw == w => i + 1,
            _ if now <= p.combo_until => p.combo,
            _ => 0,
        } % wd.light.len() as u8;
        p.combo = (idx + 1) % wd.light.len() as u8;
        start_move(p, body, action, MoveRef::Weapon(w, WeaponMove::Light(idx)), ctx);
        return true;
    }
    if p.buffer.buffered(btn::HEAVY, now, buf) {
        p.buffer.consume(btn::HEAVY);
        action.stop();
        p.state = PState::Charging;
        p.charge = 0;
        p.combo = 0;
        return true;
    }
    if p.buffer.buffered(btn::SPECIAL, now, buf) && p.special_ready(t) {
        p.buffer.consume(btn::SPECIAL);
        p.special = 0.0;
        start_move(p, body, action, MoveRef::Weapon(w, WeaponMove::Special), ctx);
        return true;
    }
    if p.buffer.buffered(btn::SWITCH, now, buf) {
        p.buffer.consume(btn::SWITCH);
        p.switched = false;
        start_move(p, body, action, MoveRef::Player(PlayerMove::Switch), ctx);
        return true;
    }
    false
}

/// Uses the item in the selected quick slot. Doesn't require stamina.
fn try_item(p: &mut Player, body: &mut Body, action: &mut Action, ctx: &mut Ctx) -> bool {
    if !p.buffer.buffered(btn::ITEM, ctx.now, ctx.t.player.input_buffer) {
        return false;
    }
    p.buffer.consume(btn::ITEM);
    let Some(item) = p.inventory.active_item() else { return false };
    if !p.inventory.consume(item) {
        return false;
    }
    // All consumables go through the same action (bringing the item to the mouth,
    // crushing the ember…); the effect applies at `heal_at`, lost if hit before.
    p.healed = false;
    p.using = Some(item);
    start_move(p, body, action, MoveRef::Player(PlayerMove::Heal), ctx);
    true
}

/// Effect of the consumable being used.
fn use_item_effect(p: &mut Player, health: &mut Health, entity: Entity, ctx: &mut Ctx) {
    let Some(item) = p.using.take() else { return };
    match item {
        Item::HealFlask => {
            health.cur = (health.cur + health.max * ctx.t.player.heal_ratio).min(health.max);
            ctx.events.push(SimEvent::Heal { entity });
            return;
        }
        Item::GoldenMoss => p.regen_ticks = items::MOSS_TICKS,
        Item::EmberResin => p.resin_ticks = items::RESIN_TICKS,
        Item::WardingAsh => p.ward_ticks = items::ASH_TICKS,
        Item::VigorRoot => p.vigor_ticks = items::ROOT_TICKS,
        Item::FlaskShard | Item::IronBrooch | Item::CrestPlume => {}
    }
    ctx.events.push(SimEvent::ItemUsed { entity, item });
}

/// Interact: pick up the item (or loot) in range, otherwise rest at the checkpoint,
/// go through a boss's fog, rekindle the torch of a defeated boss, read the sign.
/// Nothing in range: on the gamepad, the button is left to the jump.
fn try_interact(p: &mut Player, body: &mut Body, health: &mut Health, enc: &mut Encounter, ctx: &mut Ctx) -> bool {
    if !p.buffer.buffered(btn::INTERACT, ctx.now, ctx.t.player.input_buffer) {
        return false;
    }
    if let Some((e, l)) = near_loot(ctx.loot.iter().map(|(e, l)| (*e, l)), body.pos) {
        p.buffer.consume(btn::INTERACT);
        ctx.loot.retain(|(le, _)| *le != e);
        ctx.commands.entity(e).despawn();
        p.inventory.add(l.item, 1);
        ctx.events.push(SimEvent::LootPicked { entity: ctx.entity, item: l.item });
        return true;
    }
    if let Some(i) = near_pickup(ctx.t, p.picked, body.pos) {
        p.buffer.consume(btn::INTERACT);
        p.picked |= 1u64 << i;
        for (item, n) in &ctx.t.level.pickups[i as usize].items {
            p.inventory.add(*item, *n);
        }
        ctx.events.push(SimEvent::PickedUp { entity: ctx.entity, pickup: i });
        return true;
    }
    if try_passage(p, body, enc, ctx) {
        return true;
    }
    // No resting during a fight, nor with enemies on your heels.
    let Some(cp) = near_checkpoint(ctx.t, body.pos).filter(|_| !enc.active && !enc.hunted) else {
        return false;
    };
    p.buffer.consume(btn::INTERACT);
    if p.found & (1u32 << cp) == 0 {
        p.found |= 1u32 << cp;
        ctx.events.push(SimEvent::Kindled { entity: ctx.entity, checkpoint: cp });
    }
    p.checkpoint = cp;
    enc.respawn_enemies = true;
    health.cur = health.max;
    p.stamina = ctx.t.player.max_stamina;
    p.stamina_delay = 0;
    p.regain = 0.0;
    p.regain_timer = 0;
    p.inventory.refill(ctx.t);
    p.lock = None;
    p.vel = Vec3::ZERO;
    p.sprinting = false;
    ctx.events.push(SimEvent::Rested { entity: ctx.entity });
    true
}

/// The bosses' fogs (into their arena, and back out of it after the victory), their torches and
/// the sign behind the final door. The button has already been checked.
fn try_passage(p: &mut Player, body: &mut Body, enc: &Encounter, ctx: &mut Ctx) -> bool {
    let t = ctx.t;
    let entity = ctx.entity;
    let to = match p.zone {
        // A living boss's fog: into its arena (unless another one is being fought).
        Zone::Level => {
            if let Some(i) = encounter::near_gate(t, body.pos).filter(|&i| !enc.is_defeated(i) && enc.arena.is_none_or(|a| a == i)) {
                Some((encounter::door_entry(t, i as usize), Zone::Arena(i)))
            } else if let Some(i) = encounter::near_torch(t, body.pos).filter(|&i| enc.is_defeated(i)) {
                p.buffer.consume(btn::INTERACT);
                ctx.events.push(SimEvent::TorchTouched { entity, arena: i });
                return true;
            } else {
                None
            }
        }
        // Once the fight is over, back out in front of the fog.
        Zone::Arena(i) if !enc.active && encounter::near_door(t, i as usize, body.pos) => {
            Some((encounter::gate_outside(t, i as usize), Zone::Level))
        }
        Zone::Arena(_) => None,
    };
    let Some(((pos, yaw), zone)) = to else { return false };
    p.buffer.consume(btn::INTERACT);
    body.pos = pos;
    body.yaw = yaw;
    p.zone = zone;
    p.vel = Vec3::ZERO;
    p.lock = None;
    p.sprinting = false;
    ctx.events.push(SimEvent::Passage { entity, arena: if let Zone::Arena(i) = zone { Some(i) } else { None } });
    true
}

/// Jump (on the gamepad, the interact button when there's nothing in range). It requires stamina.
fn try_jump(p: &mut Player, body: &mut Body, ctx: &mut Ctx) -> bool {
    let jd = &ctx.t.player.jump;
    if !p.buffer.buffered(btn::JUMP, ctx.now, ctx.t.player.input_buffer) || !p.can_act() {
        return false;
    }
    p.buffer.consume(btn::JUMP);
    p.buffer.consume(btn::INTERACT);
    p.spend_stamina(jd.stamina, ctx.t);
    p.state = PState::Free;
    p.airborne = true;
    p.air_vy = jd.speed;
    p.air_from = body.pos.y;
    p.air_ticks = 0;
    p.charge = 0;
    // Leap in the stick's direction, at the current speed.
    if let Some(d) = ctx.move_dir {
        body.yaw = math::yaw_of(d);
    }
    ctx.events.push(SimEvent::Jumped { entity: ctx.entity });
    true
}

/// Jump attack: light or heavy attack pressed in the air.
fn try_jump_attack(p: &mut Player, body: &mut Body, action: &mut Action, ctx: &mut Ctx) -> bool {
    let buf = ctx.t.player.input_buffer;
    let pressed = [btn::LIGHT, btn::HEAVY].into_iter().any(|b| p.buffer.buffered(b, ctx.now, buf));
    if !pressed || !p.can_act() {
        return false;
    }
    p.buffer.consume(btn::LIGHT);
    p.buffer.consume(btn::HEAVY);
    p.combo = 0;
    start_move(p, body, action, MoveRef::Weapon(p.weapon, WeaponMove::Jump), ctx);
    true
}

/// Stamina cost of an action: explicit, or proportional to damage for attacks.
pub fn stamina_cost(mv: MoveRef, t: &Tuning) -> f32 {
    let def = t.get(mv);
    match (def.stamina, mv) {
        (Some(c), _) => c,
        (None, MoveRef::Weapon(..)) => def.total_damage() * t.player.stamina_per_damage,
        _ => 0.0,
    }
}

/// Starts an action: stamina cost, initial orientation, events.
fn start_move(p: &mut Player, body: &mut Body, action: &mut Action, mv: MoveRef, ctx: &mut Ctx) {
    let mut cost = stamina_cost(mv, ctx.t);
    if matches!(mv, MoveRef::Player(PlayerMove::Dodge | PlayerMove::Backstep)) && p.inventory.wears(Item::CrestPlume) {
        cost *= items::FEATHER_DODGE;
    }
    p.spend_stamina(cost, ctx.t);
    if matches!(mv, MoveRef::Weapon(..)) {
        // Attacks immediately face the locked target or the stick's direction.
        if let Some(tp) = ctx.locked_pos {
            body.yaw = math::yaw_of(tp - body.pos);
        } else if let Some(d) = ctx.move_dir {
            body.yaw = math::yaw_of(d);
        }
        let heavy = matches!(mv, MoveRef::Weapon(_, WeaponMove::Heavy | WeaponMove::HeavyCharged | WeaponMove::Jump));
        ctx.events.push(SimEvent::Swing { entity: ctx.entity, heavy });
    }
    let dist = ctx.target.map(|ti| math::flat_len(ti.pos - body.pos)).unwrap_or(0.0);
    action.start(mv, dist);
    p.state = PState::Acting;
    p.sprinting = false;
    p.sprint_latched = false;
}

/// Starts an action undergone (hit reaction, guard…), outside the input loop.
pub fn force_move(p: &mut Player, action: &mut Action, mv: MoveRef) {
    p.using = None;
    action.start(mv, 0.0);
    action.executed = false;
    p.state = PState::Acting;
    p.charge = 0;
    p.sprinting = false;
    p.sprint_latched = false;
    p.buffer.clear();
}

/// Runs the current frame of an action: target tracking and root motion.
pub fn run_frame(
    body: &mut Body,
    action: &Action,
    def: &super::data::MoveDef,
    target: Option<Vec3>,
    move_dir: Option<Vec3>,
) {
    if action.tick < def.track_until {
        let desired = target
            .map(|tp| math::yaw_of(tp - body.pos))
            .or(move_dir.map(math::yaw_of));
        if let Some(y) = desired {
            body.yaw = math::turn_towards(body.yaw, y, def.track_rate.to_radians() * DT);
        }
    }
    for m in &def.motion {
        if action.tick >= m.start && action.tick < m.end {
            if m.turn != 0.0 {
                body.yaw = math::wrap(body.yaw + m.turn.to_radians() / (m.end - m.start).max(1) as f32);
            }
            body.pos += math::forward(body.yaw) * motion_speed(m, action.target_dist) * DT;
            if m.side != 0.0 {
                body.pos += math::right(body.yaw) * m.side * DT;
            }
        }
    }
}

/// Speed of a root motion segment (forwards), according to the distance to the frozen target.
pub fn motion_speed(m: &super::data::Motion, target_dist: f32) -> f32 {
    if m.to_target {
        let span = (m.end - m.start).max(1) as f32 * DT;
        ((target_dist - m.stop_dist).max(0.0) / span).min(m.speed)
    } else {
        m.speed
    }
}

fn locomotion(
    p: &mut Player,
    body: &mut Body,
    stick_len: f32,
    move_dir: Option<Vec3>,
    locked_pos: Option<Vec3>,
    t: &Tuning,
) {
    let pd = &t.player;
    let guarding = p.state == PState::Guard;
    let can_sprint = !guarding && move_dir.is_some() && stick_len > 0.5 && p.stamina > 0.0;
    p.sprint_latched &= can_sprint;
    p.sprinting = can_sprint && (p.dodge_held >= pd.sprint_hold || p.sprint_latched);
    let speed = if guarding {
        pd.guard.walk_speed
    } else if p.sprinting {
        pd.sprint_speed
    } else {
        pd.run_speed
    } * stick_len;
    if p.sprinting {
        p.spend_stamina(pd.sprint_stamina * DT, t);
    }
    let target_vel = move_dir.unwrap_or(Vec3::ZERO) * speed;
    let dv = target_vel - p.vel;
    let max_dv = pd.accel * DT;
    p.vel += if dv.length() > max_dv { dv.normalize() * max_dv } else { dv };
    body.pos += p.vel * DT;

    let rate = pd.turn_rate.to_radians() * DT;
    if let (Some(tp), false) = (locked_pos, p.sprinting) {
        body.yaw = math::turn_towards(body.yaw, math::yaw_of(tp - body.pos), rate);
    } else if let Some(d) = move_dir {
        body.yaw = math::turn_towards(body.yaw, math::yaw_of(d), rate);
    }
}

/// Air movement: momentum is preserved (even with the stick released); the stick only
/// bends it slowly, without slowing it if it pushes in the same direction.
fn air_control(p: &mut Player, body: &mut Body, stick_len: f32, move_dir: Option<Vec3>, t: &Tuning) {
    let pd = &t.player;
    if let Some(d) = move_dir {
        let want = d * math::flat_len(p.vel).max(pd.run_speed * stick_len);
        let dv = want - p.vel;
        let max_dv = pd.jump.air_control * DT;
        p.vel += if dv.length() > max_dv { dv.normalize() * max_dv } else { dv };
        body.yaw = math::turn_towards(body.yaw, math::yaw_of(d), pd.turn_rate.to_radians() * 0.4 * DT);
    }
    p.sprinting = false;
    body.pos += p.vel * DT;
}

/// End-of-tick regenerations and timers.
pub fn player_end_tick(tuning: Res<Tuning>, mut q: Query<(&mut Player, &Action, &mut Health)>) {
    let pd = &tuning.player;
    for (mut p, action, mut health) in &mut q {
        let pm = &mut *p;
        for ticks in [&mut pm.resin_ticks, &mut pm.ward_ticks, &mut pm.vigor_ticks] {
            *ticks = ticks.saturating_sub(1);
        }
        if p.regen_ticks > 0 {
            p.regen_ticks -= 1;
            if !health.dead() && !p.falling {
                health.cur = (health.cur + items::MOSS_HP_PER_SEC * DT).min(health.max);
            }
        }
        if !matches!(p.state, PState::Free | PState::Guard) && !p.airborne {
            p.vel = Vec3::ZERO;
        }
        if p.stamina_delay > 0 {
            p.stamina_delay -= 1;
        } else if !p.sprinting && p.state != PState::Dead {
            let attacking = matches!(action.mv, Some(MoveRef::Weapon(..)));
            if !attacking {
                let rate = if p.state == PState::Guard {
                    pd.stamina_regen_guarding
                } else {
                    pd.stamina_regen
                };
                let rate = if p.vigor_ticks > 0 { rate * items::ROOT_REGEN } else { rate };
                p.stamina = (p.stamina + rate * DT).min(pd.max_stamina);
            }
        }
        if p.regain_timer > 0 {
            p.regain_timer -= 1;
            if p.regain_timer == 0 {
                p.regain = 0.0;
            }
        }
        let max = p.special_max(&tuning);
        p.special = p.special.clamp(0.0, max);
    }
}
