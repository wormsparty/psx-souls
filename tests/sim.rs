//! Simulation integration tests (no rendering).

use bevy::prelude::*;
use psx_souls::sim::boss::Boss;
use psx_souls::sim::data::{BossMove, MoveRef, PlayerMove, Shape, Tuning, WeaponMove};
use psx_souls::sim::encounter::{self, Encounter, Progress, SimCommand, SimCommands, checkpoint_pos, checkpoint_spawn};
use psx_souls::sim::items::{Item, Loot};
use psx_souls::sim::fighter::{Action, Body, Health};
use psx_souls::sim::input::{PlayerInput, PlayerInputs, btn};
use psx_souls::sim::player::{PState, Player};
use psx_souls::sim::world::Zone;
use psx_souls::sim::{ResetFight, SimDebug, SimEvent, SimEvents, SimPlugin, SimSchedule, math, state_hash};

/// Fight in progress against the Automaton (passive): the player went through the fog of the
/// theatre, facing the boss (which wakes up).
fn new_app() -> App {
    let mut app = encounter_app(0);
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    app
}

/// The theatre's arena, moved so that it's centred on the origin.
fn theatre_at_origin(t: &Tuning) -> psx_souls::sim::data::ArenaDef {
    let mut a = t.arenas[0].clone();
    let (c, _) = psx_souls::sim::world::arena_bounds(&a);
    let [dx, dz] = [-c.x, -c.z];
    for f in &mut a.floors {
        match &mut f.shape {
            Shape::Ellipse { center, .. } => *center = [center[0] + dx, center[1] + dz],
            Shape::Strip { from, to, .. } => {
                (from[0], from[1], to[0], to[1]) = (from[0] + dx, from[1] + dz, to[0] + dx, to[1] + dz);
            }
        }
    }
    for p in &mut a.pillars {
        (p[0], p[1]) = (p[0] + dx, p[1] + dz);
    }
    a.door.pos = [a.door.pos[0] + dx, a.door.pos[1] + dz];
    a.door.look = [a.door.look[0] + dx, a.door.look[1] + dz];
    a.boss_spawn = [a.boss_spawn[0] + dx, a.boss_spawn[1] + dz];
    a
}

/// Game with encounter `choice`, the player having gone through its fog (fight started). For the
/// tests of the bosses' mechanics, its arena is replaced by the theatre, centred on the origin.
fn encounter_app(choice: u8) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    let t = {
        let mut t = app.world_mut().resource_mut::<Tuning>();
        let theatre = theatre_at_origin(&t);
        let a = &mut t.arenas[choice as usize];
        (a.floors, a.pillars, a.door, a.boss_spawn) = (theatre.floors, theatre.pillars, theatre.door, theatre.boss_spawn);
        t.clone()
    };
    app.world_mut().resource_mut::<ResetFight>().progress = Some(Progress { arena: Some(choice), ..Progress::new_game(&t) });
    steps(&mut app, 3, IDLE); // spawn, the bosses, they wake up
    assert!(app.world().resource::<Encounter>().active);
    app
}

/// Game starting, as in play: in front of the checkpoint, boss asleep.
fn fresh_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    step(&mut app, PlayerInput::default());
    app
}

fn step(app: &mut App, input: PlayerInput) {
    app.world_mut().resource_mut::<PlayerInputs>().0[0] = input;
    app.world_mut().run_schedule(SimSchedule);
}

fn steps(app: &mut App, n: u32, input: PlayerInput) {
    for _ in 0..n {
        step(app, input);
    }
}

fn player(app: &mut App) -> Entity {
    app.world_mut().query_filtered::<Entity, With<Player>>().single(app.world()).unwrap()
}

fn boss(app: &mut App) -> Entity {
    app.world_mut().query_filtered::<Entity, With<Boss>>().single(app.world()).unwrap()
}

fn hp(app: &mut App, e: Entity) -> f32 {
    app.world().get::<Health>(e).unwrap().cur
}

fn tuning(app: &App) -> Tuning {
    app.world().resource::<Tuning>().clone()
}

/// Places the player facing the boss at `dist` metres and launches the boss's attack `name`.
/// Returns the number of ticks before the attack's first active tick.
fn boss_attack(app: &mut App, name: &str, dist: f32) -> u32 {
    let t = tuning(app);
    let (p, b) = (player(app), boss(app));
    let idx = t.bosses[0].attacks.iter().position(|a| a.name == name).expect(name);
    {
        let mut bb = app.world_mut().get_mut::<Body>(b).unwrap();
        bb.pos = Vec3::ZERO;
        bb.yaw = 0.0;
    }
    {
        let mut pb = app.world_mut().get_mut::<Body>(p).unwrap();
        pb.pos = Vec3::new(0.0, 0.0, dist);
        pb.yaw = std::f32::consts::PI; // facing the boss
    }
    app.world_mut().get_mut::<Action>(b).unwrap().start(MoveRef::BossAttack(0, idx as u16), dist);
    app.world_mut().resource_mut::<SimEvents>().0.clear();
    t.bosses[0].attacks[idx].mv.hits[0].start
}

fn events(app: &mut App) -> Vec<SimEvent> {
    std::mem::take(&mut app.world_mut().resource_mut::<SimEvents>().0)
}

const GUARD: PlayerInput = PlayerInput { buttons: btn::GUARD, move_x: 0, move_y: 0, cam_yaw: 0, cmd: [0; 4] };
const IDLE: PlayerInput = PlayerInput { buttons: 0, move_x: 0, move_y: 0, cam_yaw: 0, cmd: [0; 4] };

/// Presses guard `lead` ticks before the impact and returns (HP lost, perfect guard?).
fn guard_with_lead(lead: u32) -> (f32, bool) {
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    let p = player(&mut app);
    let before = hp(&mut app, p);
    // The first active tick runs on the (hit_start + 1)-th sim tick.
    let press_at = hit_start + 1 - lead;
    steps(&mut app, press_at - 1, IDLE);
    steps(&mut app, lead + 20, GUARD);
    let ev = events(&mut app);
    let perfect = ev.iter().any(|e| matches!(e, SimEvent::PerfectGuard { .. }));
    (before - hp(&mut app, p), perfect)
}

#[test]
fn perfect_guard_window_is_tick_exact() {
    let t = Tuning::builtin();
    let w = t.player.guard.perfect_window;
    for lead in 1..=w + 1 {
        let (lost, perfect) = guard_with_lead(lead);
        // Press `lead` ticks before the impact tick.
        if lead <= w {
            assert!(perfect, "lead {lead}: should be perfect");
            assert_eq!(lost, 0.0);
        } else {
            assert!(!perfect, "lead {lead}: should not be perfect");
        }
    }
    // Guard held for a long time: normal guard, reduced damage converted into regain.
    let (lost, perfect) = guard_with_lead(40);
    let full = t.bosses[0].attacks.iter().find(|a| a.name == "ecrasement").unwrap().mv.hits[0].damage;
    assert!(!perfect);
    assert!((lost - full * t.player.guard.damage_ratio).abs() < 1e-3, "lost {lost}");
}

/// Keeping guard held through a guard reaction (cancelled back into guard) doesn't reopen the
/// perfect window: a second blow right after is a normal guard.
#[test]
fn held_guard_after_a_block_does_not_reopen_perfect_window() {
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    let p = player(&mut app);
    steps(&mut app, hit_start - 20, GUARD);
    let start = app.world().get::<Player>(p).unwrap().guard_start;
    steps(&mut app, 40, GUARD);
    assert!(events(&mut app).iter().all(|e| !matches!(e, SimEvent::PerfectGuard { .. })));
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!(pl.state, PState::Guard);
    assert_eq!(pl.guard_start, start, "guard window restarted while held");
}

#[test]
fn guard_spam_shrinks_window() {
    let mut app = new_app();
    let t = tuning(&app);
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    let p = player(&mut app);
    // Spam: repeated presses until the impact, last press `perfect_window` ticks before.
    let last = hit_start + 1 - t.player.guard.perfect_window - 1;
    let mut tick = 0;
    while tick + 4 < last {
        step(&mut app, GUARD);
        step(&mut app, IDLE);
        tick += 2;
    }
    steps(&mut app, last - tick, IDLE);
    steps(&mut app, 30, GUARD);
    let pl = app.world().get::<Player>(p).unwrap();
    assert!(pl.guard_spam > 0);
    assert!(!events(&mut app).iter().any(|e| matches!(e, SimEvent::PerfectGuard { .. })));
}

#[test]
fn fury_breaks_normal_guard_but_not_perfect() {
    let t = Tuning::builtin();
    let fury = t.bosses[0].attacks.iter().find(|a| a.name == "furie_estoc").unwrap();
    let dmg = fury.mv.hits[0].damage;

    // Guard held: full damage.
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "furie_estoc", 4.0);
    let p = player(&mut app);
    steps(&mut app, hit_start + 10, GUARD);
    assert_eq!(hp(&mut app, p), t.player.max_hp - dmg);

    // Perfect guard: no damage.
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "furie_estoc", 4.0);
    let p = player(&mut app);
    steps(&mut app, hit_start - 2, IDLE);
    steps(&mut app, 20, GUARD);
    assert_eq!(hp(&mut app, p), t.player.max_hp);
}

#[test]
fn dodge_iframes_avoid_damage() {
    let mut app = new_app();
    let t = tuning(&app);
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    let p = player(&mut app);
    // Backwards dodge (backstep) started just before the impact.
    steps(&mut app, hit_start - 3, IDLE);
    step(&mut app, PlayerInput { buttons: btn::DODGE, ..IDLE });
    steps(&mut app, 40, IDLE);
    assert_eq!(hp(&mut app, p), t.player.max_hp);
}

#[test]
fn regain_heals_when_hitting_back() {
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    let p = player(&mut app);
    steps(&mut app, hit_start + 20, GUARD);
    let after_guard = hp(&mut app, p);
    let regain = app.world().get::<Player>(p).unwrap().regain;
    assert!(regain > 0.0);
    // Counter-attack: the player is in range, facing the boss.
    let b = boss(&mut app);
    app.world_mut().get_mut::<Action>(b).unwrap().stop();
    for _ in 0..4 {
        step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
        steps(&mut app, 25, IDLE);
    }
    assert!(hp(&mut app, p) > after_guard, "regain must restore HP");
}

#[test]
fn stagger_leads_to_groggy_and_fatal() {
    let mut app = new_app();
    let t = tuning(&app);
    let (p, b) = (player(&mut app), boss(&mut app));
    {
        let mut pb = app.world_mut().get_mut::<Body>(p).unwrap();
        pb.pos = Vec3::new(0.0, 0.0, 6.0 - 2.0);
    }
    app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::new(0.0, 0.0, 6.0);
    app.world_mut().get_mut::<Body>(b).unwrap().yaw = std::f32::consts::PI;
    app.world_mut().get_mut::<Body>(p).unwrap().yaw = 0.0;
    {
        let mut boss = app.world_mut().get_mut::<Boss>(b).unwrap();
        boss.stagger = t.bosses[0].stagger_max - 1.0;
    }
    // A light attack is enough to fill the gauge.
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    steps(&mut app, 30, IDLE);
    assert!(app.world().get::<Action>(b).unwrap().is(MoveRef::Boss(0, BossMove::Groggy)));
    let boss_hp = hp(&mut app, b);
    let special = app.world().get::<Player>(p).unwrap().special;
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    assert!(app.world().get::<Action>(b).unwrap().is(MoveRef::Boss(0, BossMove::FatalReceived)));
    steps(&mut app, 120, IDLE);
    assert!(hp(&mut app, b) < boss_hp - 200.0);
    // The fatal blow doesn't recharge the special gauge.
    assert_eq!(app.world().get::<Player>(p).unwrap().special, special);
}

#[test]
fn weapon_switch_and_lock_on() {
    let mut app = new_app();
    let p = player(&mut app);
    step(&mut app, PlayerInput { buttons: btn::SWITCH, ..IDLE });
    steps(&mut app, 30, IDLE);
    assert_eq!(app.world().get::<Player>(p).unwrap().weapon, 1);
    step(&mut app, PlayerInput { buttons: btn::LOCK, ..IDLE });
    assert!(app.world().get::<Player>(p).unwrap().lock.is_some());
}

#[test]
fn hit_reaction_and_death() {
    let mut app = new_app();
    let t = tuning(&app);
    let p = player(&mut app);
    app.world_mut().get_mut::<Health>(p).unwrap().cur = 10.0;
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    steps(&mut app, hit_start + 30 + t.player.death.total, IDLE);
    assert_eq!(app.world().get::<Player>(p).unwrap().state, PState::Dead);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::PlayerDied { .. })) || hp(&mut app, p) == 0.0);
    let _ = PlayerMove::Death;
}

/// Pseudo-random but reproducible input sequence.
fn scripted_input(i: u32) -> PlayerInput {
    let mut x = i.wrapping_mul(2654435761) ^ 0x5bd1e995;
    x ^= x >> 15;
    let buttons = match (i / 7) % 9 {
        0 => btn::LIGHT,
        1 => btn::HEAVY,
        2 => btn::GUARD,
        3 => btn::DODGE,
        4 => 0,
        5 => btn::LIGHT,
        6 => btn::GUARD,
        7 => if i % 300 < 7 { btn::SPECIAL } else { 0 },
        _ => if i % 500 < 7 { btn::SWITCH } else { 0 },
    };
    PlayerInput {
        buttons: if i == 5 { btn::LOCK } else { buttons },
        move_x: ((x & 0xff) as i32 - 128).clamp(-127, 127) as i8,
        move_y: (((x >> 8) & 0xff) as i32 - 128).clamp(-127, 127) as i8,
        cam_yaw: PlayerInput::quantize_yaw(i as f32 * 0.002),
        cmd: [0; 4],
    }
}

fn run_scripted(n: u32) -> (u64, f32, f32) {
    // Straight into the theatre (after a death, back at the checkpoint: no more boss).
    let mut app = encounter_app(0);
    let mut hashes = 0u64;
    let mut min_hp = (f32::MAX, f32::MAX);
    for i in 0..n {
        step(&mut app, scripted_input(i));
        hashes = hashes.rotate_left(5) ^ state_hash(app.world_mut());
        // Lowest HP reached.
        let p = player(&mut app);
        min_hp.0 = min_hp.0.min(hp(&mut app, p));
        let b = app.world_mut().query_filtered::<Entity, With<Boss>>().iter(app.world()).next();
        if let Some(b) = b {
            min_hp.1 = min_hp.1.min(hp(&mut app, b));
        }
    }
    (hashes, min_hp.0, min_hp.1)
}

#[test]
fn simulation_is_deterministic() {
    let a = run_scripted(4000);
    let b = run_scripted(4000);
    assert_eq!(a.0, b.0, "the simulation must be deterministic");
    let t = Tuning::builtin();
    // Something really happened.
    assert!(a.1 < t.player.max_hp || a.2 < t.bosses[0].max_hp, "{a:?}");
    let _ = math::wrap(0.0);
}

#[test]
fn stamina_goes_negative_and_blocks_actions() {
    let mut app = new_app();
    let t = tuning(&app);
    let p = player(&mut app);
    // A hit with 1.5 stamina: it goes off, and stamina goes negative.
    app.world_mut().get_mut::<Player>(p).unwrap().stamina = 1.5;
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    let st = app.world().get::<Player>(p).unwrap().stamina;
    assert!(st < 0.0, "stamina {st}");
    assert!(st >= t.player.stamina_floor);
    steps(&mut app, 30, IDLE);
    // Below 1 point: neither attack nor dodge.
    app.world_mut().get_mut::<Player>(p).unwrap().stamina = 0.5;
    app.world_mut().get_mut::<Player>(p).unwrap().stamina_delay = 100;
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    assert_ne!(app.world().get::<Player>(p).unwrap().state, PState::Acting);
    step(&mut app, PlayerInput { buttons: btn::DODGE, ..IDLE });
    assert_ne!(app.world().get::<Player>(p).unwrap().state, PState::Acting);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::NoStamina { .. })));
}

#[test]
fn stamina_cost_is_proportional_to_damage_for_all_weapons() {
    use psx_souls::sim::player::stamina_cost;
    let t = Tuning::builtin();
    for (w, wd) in t.weapons.iter().enumerate() {
        for (i, l) in wd.light.iter().enumerate() {
            let c = stamina_cost(MoveRef::Weapon(w as u8, WeaponMove::Light(i as u8)), &t);
            let ratio = c / l.total_damage();
            assert!((ratio - t.player.stamina_per_damage).abs() < 1e-5, "{}: {ratio}", l.anim);
        }
        assert_eq!(stamina_cost(MoveRef::Weapon(w as u8, WeaponMove::Fatal), &t), 0.0);
    }
}

#[test]
fn heal_restores_health_and_is_lost_if_interrupted() {
    let t = Tuning::builtin();
    // Normal heal.
    let mut app = new_app();
    let p = player(&mut app);
    app.world_mut().get_mut::<Health>(p).unwrap().cur = 100.0;
    step(&mut app, PlayerInput { buttons: btn::ITEM, ..IDLE });
    steps(&mut app, t.player.heal.total + 2, IDLE);
    let expected = 100.0 + t.player.max_hp * t.player.heal_ratio;
    assert!((hp(&mut app, p) - expected).abs() < 1e-3);
    assert_eq!(app.world().get::<Player>(p).unwrap().inventory.count(Item::HealFlask), t.player.heal_charges - 1);

    // Hit before it applies: charge lost, no heal.
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    let p = player(&mut app);
    steps(&mut app, hit_start + 1 - 5, IDLE);
    step(&mut app, PlayerInput { buttons: btn::ITEM, ..IDLE });
    steps(&mut app, 60, IDLE);
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!(pl.inventory.count(Item::HealFlask), t.player.heal_charges - 1);
    assert!(hp(&mut app, p) < t.player.max_hp);
    // No more charges: nothing happens.
    while app.world_mut().get_mut::<Player>(p).unwrap().inventory.consume(Item::HealFlask) {}
    steps(&mut app, 120, IDLE);
    step(&mut app, PlayerInput { buttons: btn::ITEM, ..IDLE });
    assert_ne!(app.world().get::<Player>(p).unwrap().state, PState::Acting);
}

#[test]
fn charged_heavy_releases_automatically_at_full_charge() {
    use psx_souls::sim::data::WeaponMove;
    let t = Tuning::builtin();
    for w in 0..t.weapons.len() as u8 {
        let mut app = new_app();
        let p = player(&mut app);
        app.world_mut().get_mut::<Player>(p).unwrap().weapon = w;
        let full = t.weapons[w as usize].charge_ticks;
        // Heavy button held indefinitely.
        steps(&mut app, full + 2, PlayerInput { buttons: btn::HEAVY, ..IDLE });
        let a = app.world().get::<Action>(p).unwrap();
        assert!(a.is(MoveRef::Weapon(w, WeaponMove::HeavyCharged)), "weapon {w}: {:?}", a.mv);
        // Released early: normal heavy.
        let mut app = new_app();
        let p = player(&mut app);
        app.world_mut().get_mut::<Player>(p).unwrap().weapon = w;
        steps(&mut app, 5, PlayerInput { buttons: btn::HEAVY, ..IDLE });
        step(&mut app, IDLE);
        assert!(app.world().get::<Action>(p).unwrap().is(MoveRef::Weapon(w, WeaponMove::Heavy)));
    }
}

fn body(app: &mut App, e: Entity) -> Body {
    *app.world().get::<Body>(e).unwrap()
}

#[test]
fn the_theatre_fog_leads_into_the_arena_where_the_boss_awaits() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    // Start in front of the first checkpoint, on the square below the theatre: no boss yet.
    let start = body(&mut app, p).pos;
    assert!(start.distance(checkpoint_pos(&t, 0)) < 2.5);
    assert!(start.y < -1.0);
    assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0);
    // Climb the stairs (the camera faces +z): the fog stops you at the top.
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, -2.4, -25.0);
    steps(&mut app, 300, PlayerInput { move_y: 127, ..IDLE });
    let top = body(&mut app, p).pos;
    assert!(top.y.abs() < 1e-3 && top.z <= -16.2 + 1e-3 && top.z > -17.5, "{top:?}");
    assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Level);
    // Through the fog: in the theatre, the boss wakes up roaring.
    events(&mut app);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Passage { arena: Some(0), .. })));
    assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Arena(0));
    steps(&mut app, 2, IDLE);
    assert!(app.world().resource::<Encounter>().active);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::BossAwake)));
    let b = boss(&mut app);
    assert!(app.world().get::<Action>(b).unwrap().is(MoveRef::Boss(0, BossMove::Roar)));
    // No way back out during the fight: the fog is a wall, and doesn't let you through.
    steps(&mut app, 240, PlayerInput { move_y: -127, ..IDLE });
    step(&mut app, PlayerInput { buttons: btn::INTERACT, move_y: -127, ..IDLE });
    let pos = body(&mut app, p).pos;
    let (centre, _) = psx_souls::sim::world::arena_bounds(&t.arenas[0]);
    assert!(math::flat_len(pos - centre) <= 16.0, "{pos:?}");
    assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Arena(0));
}

#[test]
fn defeated_boss_bars_its_fog_until_its_torch_is_rekindled() {
    let mut app = new_app();
    let t = tuning(&app);
    let (p, b) = (player(&mut app), boss(&mut app));
    // Boss at one HP, right in front of the player.
    app.world_mut().get_mut::<Health>(b).unwrap().cur = 1.0;
    app.world_mut().get_mut::<Action>(b).unwrap().stop();
    app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::new(0.0, 0.0, -8.0);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, -10.0);
    app.world_mut().get_mut::<Body>(p).unwrap().yaw = 0.0;
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    steps(&mut app, 40, IDLE);
    let enc = *app.world().resource::<Encounter>();
    assert!(enc.is_defeated(0) && !enc.active);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::BossDefeated)));

    // Back out through the fog: in front of it, in the level; the arena empties.
    let (door, dir) = psx_souls::sim::world::portal(&t, &t.arenas[0].door);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = door + dir * 0.8;
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Level);
    assert!(body(&mut app, p).pos.distance(encounter::gate_outside(&t, 0).0) < 0.1);
    steps(&mut app, 2, IDLE);
    assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0);
    assert_eq!(app.world().resource::<Encounter>().arena, None);
    // The fog of a defeated boss no longer lets you through.
    steps(&mut app, 20, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    steps(&mut app, 40, IDLE);
    assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Level);

    // Reload: still defeated.
    app.world_mut().resource_mut::<ResetFight>().requested = true;
    step(&mut app, IDLE);
    let p = player(&mut app);
    assert!(app.world().resource::<Encounter>().is_defeated(0));

    // Its extinguished torch: rekindling it asks the menu, which revives the boss.
    app.world_mut().get_mut::<Body>(p).unwrap().pos = encounter::torch_pos(&t, 0) + Vec3::X * 0.8;
    events(&mut app);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::TorchTouched { arena: 0, .. })));
    app.world_mut().resource_mut::<SimCommands>().0.push(SimCommand::ReviveBoss(0));
    step(&mut app, IDLE);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::BossRevived { arena: 0 })));
    assert!(!app.world().resource::<Encounter>().is_defeated(0));
    // The fog lets you through again, the boss awaits at full strength.
    let (out, _) = encounter::gate_outside(&t, 0);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = out;
    steps(&mut app, 20, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    steps(&mut app, 2, IDLE);
    assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Arena(0));
    let b = boss(&mut app);
    assert_eq!(hp(&mut app, b), t.bosses[0].max_hp);
}

#[test]
fn every_fog_leads_to_its_arena_and_back() {
    let t = Tuning::builtin();
    for i in 0..t.arenas.len() {
        let mut app = fresh_app();
        let p = player(&mut app);
        let (out, yaw) = encounter::gate_outside(&t, i);
        {
            let mut b = app.world_mut().get_mut::<Body>(p).unwrap();
            b.pos = out;
            b.yaw = yaw;
        }
        steps(&mut app, 20, IDLE);
        step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
        steps(&mut app, 3, IDLE);
        assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Arena(i as u8), "arena {i}");
        assert!(app.world().resource::<Encounter>().active, "arena {i}");
        let n = app.world_mut().query::<&Boss>().iter(app.world()).count();
        assert_eq!(n, t.encounters[i].members.len(), "arena {i}");
        // Everyone stands on the arena's floor.
        for (b, _) in app.world_mut().query::<(&Body, &Boss)>().iter(app.world()) {
            assert_eq!(psx_souls::sim::world::zone_at(&t, b.pos), Zone::Arena(i as u8), "arena {i}: {:?}", b.pos);
        }
        // Defeated: back out through the door.
        let ids: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<Boss>>().iter(app.world()).collect();
        for e in ids {
            app.world_mut().get_mut::<Health>(e).unwrap().cur = 0.0;
        }
        steps(&mut app, 3, IDLE);
        assert!(app.world().resource::<Encounter>().is_defeated(i as u8), "arena {i}");
        let (door, dir) = psx_souls::sim::world::portal(&t, &t.arenas[i].door);
        app.world_mut().get_mut::<Body>(p).unwrap().pos = door + dir * 0.8;
        step(&mut app, IDLE);
        step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
        assert_eq!(app.world().get::<Player>(p).unwrap().zone, Zone::Level, "arena {i}");
    }
}

#[test]
fn the_final_door_opens_once_every_boss_is_defeated() {
    let t = Tuning::builtin();
    let (door, dir) = psx_souls::sim::world::portal(&t, &t.level.final_door);
    let cam_yaw = PlayerInput::quantize_yaw(math::yaw_of(dir));
    let walk = PlayerInput { move_y: 127, cam_yaw, ..IDLE };
    for defeated in [0b011_1111u32, 0b111_1111] {
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        app.world_mut().resource_mut::<ResetFight>().progress = Some(Progress { defeated, ..Progress::new_game(&t) });
        step(&mut app, IDLE);
        let p = player(&mut app);
        app.world_mut().get_mut::<Body>(p).unwrap().pos = door - dir * 2.0;
        steps(&mut app, 100, walk);
        let past = (body(&mut app, p).pos - door).dot(dir);
        let open = defeated == 0b111_1111;
        assert_eq!(past > 3.0, open, "{past}");
    }
}

#[test]
fn a_plunging_attack_from_the_gallery_hits_harder() {
    let t = Tuning::builtin();
    let butcher = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == "butcher")).unwrap();
    let a = t.arenas[butcher].clone();
    let Some(Shape::Ellipse { center: [gx, gz], y: top, .. }) = a.floors.iter().find(|f| f.ledge && matches!(f.shape, Shape::Ellipse { .. })).map(|f| f.shape) else {
        panic!("no gallery");
    };
    let damage = |plunge: bool| {
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
        app.world_mut().resource_mut::<ResetFight>().progress = Some(Progress { arena: Some(butcher as u8), ..Progress::new_game(&t) });
        steps(&mut app, 3, IDLE);
        let p = player(&mut app);
        let main = app.world_mut().query::<(Entity, &Boss)>().iter(app.world()).find(|(_, b)| !b.def(&t).minor).unwrap().0;
        // The player runs off the gallery towards the butcher, and strikes while dropping onto
        // him; or, from the yard, jumps at him and strikes in the air.
        let centre = Vec3::new(gx, 0.0, gz);
        let dir = (Vec3::new(a.boss_spawn[0], 0.0, a.boss_spawn[1]) - centre).normalize();
        let target = centre + dir * 4.3;
        {
            let mut b = app.world_mut().get_mut::<Body>(main).unwrap();
            b.pos = target;
            b.yaw = math::yaw_of(-dir);
        }
        *app.world_mut().get_mut::<Action>(main).unwrap() = Action::default();
        let from = if plunge { centre.with_y(top) } else { target - dir * 3.2 };
        let cam_yaw = PlayerInput::quantize_yaw(math::yaw_of(dir));
        {
            let mut b = app.world_mut().get_mut::<Body>(p).unwrap();
            b.pos = from;
            b.yaw = math::yaw_of(dir);
        }
        let run = PlayerInput { move_y: 127, cam_yaw, ..IDLE };
        let before = hp(&mut app, main);
        if plunge {
            for _ in 0..120 {
                step(&mut app, run);
                if app.world().get::<Player>(p).unwrap().airborne {
                    break;
                }
            }
            assert!(app.world().get::<Player>(p).unwrap().airborne, "dropped from the gallery");
        } else {
            steps(&mut app, 10, run);
            step(&mut app, PlayerInput { buttons: btn::INTERACT, ..run });
            steps(&mut app, 6, run);
        }
        step(&mut app, PlayerInput { buttons: btn::LIGHT, ..run });
        steps(&mut app, 80, IDLE);
        assert!(!app.world().get::<Player>(p).unwrap().airborne);
        before - hp(&mut app, main)
    };
    let (plunge, ground) = (damage(true), damage(false));
    assert!(ground > 0.0 && plunge > ground * 1.5, "plunge {plunge}, from the ground {ground}");
}

#[test]
fn the_butcher_leaps_onto_the_gallery_after_a_while_and_back_down() {
    let t = Tuning::builtin();
    let butcher = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == "butcher")).unwrap();
    let a = t.arenas[butcher].clone();
    let Some(Shape::Ellipse { center: [gx, gz], radii: [rx, rz], y: top }) = a.floors.iter().find(|f| f.ledge && matches!(f.shape, Shape::Ellipse { .. })).map(|f| f.shape) else {
        panic!("no gallery");
    };
    let patience = t.bosses.iter().flat_map(|b| &b.attacks).find_map(|a| a.climb).expect("a climbing jump");
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.world_mut().resource_mut::<ResetFight>().progress = Some(Progress { arena: Some(butcher as u8), ..Progress::new_game(&t) });
    steps(&mut app, 3, IDLE);
    let p = player(&mut app);
    let main = app.world_mut().query::<(Entity, &Boss)>().iter(app.world()).find(|(_, b)| !b.def(&t).minor).unwrap().0;
    // The player stays put (unhurt) at `at`; returns the tick at which the butcher stands on its
    // ground, at height `y`.
    let wait_for = |app: &mut App, at: Vec3, y: f32, max: u32| {
        for i in 0..max {
            app.world_mut().get_mut::<Body>(p).unwrap().pos = at;
            let mut h = app.world_mut().get_mut::<Health>(p).unwrap();
            h.cur = h.max;
            step(app, IDLE);
            let b = body(app, main);
            if (b.pos.y - y).abs() < 0.05 && app.world().get::<Boss>(main).unwrap().flight.is_none() {
                return Some((i, b.pos));
            }
        }
        None
    };
    let (up, pos) = wait_for(&mut app, Vec3::new(gx, top, gz), top, 900).expect("leapt onto the gallery");
    assert!(up >= patience, "only after a while ({up} ticks)");
    let (dx, dz) = ((pos.x - gx) / rx, (pos.z - gz) / rz);
    assert!(dx * dx + dz * dz <= 1.0, "on the gallery ({pos:?})");
    // The player drops back into the yard: it jumps down after them.
    let yard = Vec3::new(a.boss_spawn[0], 0.0, a.boss_spawn[1]);
    let (down, _) = wait_for(&mut app, yard, 0.0, 600).expect("jumped back down");
    assert!(down < 240, "without delay ({down} ticks)");
}

#[test]
fn death_respawns_at_checkpoint_with_items_refilled() {
    let mut app = new_app();
    let t = tuning(&app);
    let p = player(&mut app);
    app.world_mut().get_mut::<Player>(p).unwrap().inventory.consume(Item::HealFlask);
    let b = boss(&mut app);
    app.world_mut().get_mut::<Health>(b).unwrap().cur = 500.0;
    app.world_mut().get_mut::<Health>(p).unwrap().cur = 10.0;
    let hit_start = boss_attack(&mut app, "ecrasement", 2.6);
    steps(&mut app, hit_start + 30 + t.player.death.total + psx_souls::sim::encounter::RESPAWN_TICKS + 2, IDLE);
    let p = player(&mut app);
    let pl = app.world().get::<Player>(p).unwrap().clone();
    assert_eq!(pl.state, PState::Free);
    assert_eq!(pl.inventory.count(Item::HealFlask), t.player.heal_charges);
    assert_eq!(hp(&mut app, p), t.player.max_hp);
    assert!(body(&mut app, p).pos.distance(checkpoint_pos(&t, 0)) < 2.5);
    // The boss starts over: it will await beyond its fog.
    assert_eq!(app.world_mut().query::<&Boss>().iter(app.world()).count(), 0);
    let enc = *app.world().resource::<Encounter>();
    assert!(!enc.active && enc.arena.is_none() && !enc.is_defeated(0));
}

#[test]
fn resting_at_checkpoint_restores_everything() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    app.world_mut().get_mut::<Health>(p).unwrap().cur = 50.0;
    while app.world_mut().get_mut::<Player>(p).unwrap().inventory.consume(Item::HealFlask) {}
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Rested { .. })));
    assert_eq!(hp(&mut app, p), t.player.max_hp);
    assert_eq!(app.world().get::<Player>(p).unwrap().inventory.count(Item::HealFlask), t.player.heal_charges);
    // Far from the checkpoint: nothing.
    app.world_mut().get_mut::<Body>(p).unwrap().pos.x += 5.0;
    steps(&mut app, 10, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(!events(&mut app).iter().any(|e| matches!(e, SimEvent::Rested { .. })));
}

#[test]
fn quick_slots_cycle_and_empty_slot_does_nothing() {
    let mut app = fresh_app();
    let p = player(&mut app);
    // Move the flask to slot 2: the selection follows.
    app.world_mut().resource_mut::<SimCommands>().0.push(SimCommand::Equip { player: 0, slot: 1, item: Some(Item::HealFlask) });
    step(&mut app, IDLE);
    assert_eq!(app.world().get::<Player>(p).unwrap().inventory.active, 1);
    // Remove the item: using does nothing.
    app.world_mut().resource_mut::<SimCommands>().0.push(SimCommand::Equip { player: 0, slot: 1, item: None });
    step(&mut app, IDLE);
    step(&mut app, PlayerInput { buttons: btn::ITEM, ..IDLE });
    assert_ne!(app.world().get::<Player>(p).unwrap().state, PState::Acting);
    step(&mut app, PlayerInput { buttons: btn::NEXT_ITEM, ..IDLE });
}

#[test]
fn l3_click_starts_sprint_until_stick_released() {
    let mut app = fresh_app();
    let p = player(&mut app);
    let fwd = PlayerInput { move_y: 127, ..default() };
    steps(&mut app, 10, fwd);
    assert!(!app.world().get::<Player>(p).unwrap().sprinting);
    // A single click is enough: the sprint continues without holding the button.
    step(&mut app, PlayerInput { buttons: btn::SPRINT, ..fwd });
    steps(&mut app, 10, fwd);
    assert!(app.world().get::<Player>(p).unwrap().sprinting);
    // Releasing the stick stops the sprint; moving again doesn't restart it.
    steps(&mut app, 2, PlayerInput::default());
    steps(&mut app, 5, fwd);
    assert!(!app.world().get::<Player>(p).unwrap().sprinting);
}

#[test]
fn special_hits_do_not_refill_special_gauge() {
    let mut app = new_app();
    let t = tuning(&app);
    let (p, b) = (player(&mut app), boss(&mut app));
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, 4.0);
    app.world_mut().get_mut::<Body>(p).unwrap().yaw = 0.0;
    app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::new(0.0, 0.0, 6.0);
    app.world_mut().get_mut::<Player>(p).unwrap().special = t.player.special_max;
    let before = hp(&mut app, b);
    step(&mut app, PlayerInput { buttons: btn::SPECIAL, ..IDLE });
    steps(&mut app, 80, IDLE);
    let dealt = before - hp(&mut app, b);
    assert!(dealt >= 200.0, "the special must hurt: {dealt}");
    assert_eq!(app.world().get::<Player>(p).unwrap().special, 0.0);
}

#[test]
fn greatsword_special_leaps_and_cleaves_far_ahead() {
    let mut app = new_app();
    let t = tuning(&app);
    let (p, b) = (player(&mut app), boss(&mut app));
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, 0.0);
    app.world_mut().get_mut::<Body>(p).unwrap().yaw = 0.0;
    // Well beyond a normal swing's reach.
    app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::new(0.0, 0.0, 5.0);
    {
        let mut pl = app.world_mut().get_mut::<Player>(p).unwrap();
        pl.weapon = 1;
        pl.special = t.player.special_max;
    }
    let before = hp(&mut app, b);
    step(&mut app, PlayerInput { buttons: btn::SPECIAL, ..IDLE });
    let mut peak: f32 = 0.0;
    for _ in 0..30 {
        step(&mut app, IDLE);
        peak = peak.max(body(&mut app, p).pos.y);
    }
    assert!(peak > 1.2, "the player leaps: {peak}");
    steps(&mut app, 60, IDLE);
    assert!(!app.world().get::<Player>(p).unwrap().airborne);
    assert!(body(&mut app, p).pos.y.abs() < 0.01, "back on the ground");
    assert!(before - hp(&mut app, b) >= 300.0, "the cleave reaches far ahead");
}

#[test]
fn special_needs_a_full_gauge() {
    let mut app = new_app();
    let t = tuning(&app);
    let p = player(&mut app);
    app.world_mut().get_mut::<Player>(p).unwrap().special = t.player.special_max - 1.0;
    step(&mut app, PlayerInput { buttons: btn::SPECIAL, ..IDLE });
    steps(&mut app, 5, IDLE);
    let w = app.world().get::<Player>(p).unwrap().weapon;
    assert!(!app.world().get::<Action>(p).unwrap().is(MoveRef::Weapon(w, WeaponMove::Special)));
    assert_eq!(app.world().get::<Player>(p).unwrap().special, t.player.special_max - 1.0);
}

#[test]
fn dodge_roll_covers_ground() {
    let mut app = new_app();
    let p = player(&mut app);
    let start = body(&mut app, p).pos;
    let fwd = PlayerInput { move_y: 127, ..IDLE };
    step(&mut app, PlayerInput { buttons: btn::DODGE, ..fwd });
    steps(&mut app, 29, IDLE);
    let d = math::flat_len(body(&mut app, p).pos - start);
    assert!(d >= 3.2, "roll too short: {d}");
}

#[test]
fn shockwave_ignores_guard_but_can_be_outrun() {
    let t = Tuning::builtin();
    let dmg = t.bosses[0].attacks.iter().find(|a| a.name == "onde_de_choc").unwrap().mv.hits[0].damage;

    // Guard held (even perfect): full damage.
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "onde_de_choc", 2.5);
    let p = player(&mut app);
    steps(&mut app, hit_start - 3, IDLE);
    steps(&mut app, 20, GUARD);
    assert_eq!(hp(&mut app, p), t.player.max_hp - dmg);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Shockwave { .. })));

    // Fleeing out of the circle during the wind-up: no damage.
    let mut app = new_app();
    let hit_start = boss_attack(&mut app, "onde_de_choc", 2.5);
    let p = player(&mut app);
    steps(&mut app, hit_start + 30, PlayerInput { move_y: 127, ..IDLE });
    assert_eq!(hp(&mut app, p), t.player.max_hp);
}

#[test]
fn leap_slam_lands_on_the_marked_spot() {
    use psx_souls::sim::boss::aoe_telegraph;
    let mut app = new_app();
    let t = tuning(&app);
    let hit_start = boss_attack(&mut app, "saut_ecrasant", 8.0);
    let (p, b) = (player(&mut app), boss(&mut app));
    // No circle while the jump tracks its target: it only settles at take-off.
    steps(&mut app, 10, IDLE);
    assert!(aoe_telegraph(app.world().get::<Body>(b).unwrap(), app.world().get::<Action>(b).unwrap(), &t).is_none());
    steps(&mut app, 50, IDLE);
    let (center, r, _) = {
        let w = app.world();
        aoe_telegraph(w.get::<Body>(b).unwrap(), w.get::<Action>(b).unwrap(), &t).expect("ground warning")
    };
    assert!(math::flat_len(center - body(&mut app, p).pos) < 0.5, "centre {center}");
    // Staying in the circle: hit.
    steps(&mut app, hit_start + 4 - 60, IDLE);
    assert!(hp(&mut app, p) < t.player.max_hp);
    assert!(r > 2.0);
}

// ----------------------------------------------------------------------------- level

use psx_souls::sim::enemy::{EState, Enemy};
use psx_souls::sim::world;

/// Places the player on the ground at (x, z), facing `yaw`.
fn put_player(app: &mut App, x: f32, z: f32, yaw: f32) {
    let t = tuning(app);
    let y = world::floor_at(&t, x, z, 0.0).expect("ground");
    let p = player(app);
    let mut b = app.world_mut().get_mut::<Body>(p).unwrap();
    b.pos = Vec3::new(x, y, z);
    b.yaw = yaw;
}

fn enemies(app: &mut App) -> Vec<(Entity, Enemy, f32)> {
    let mut q = app.world_mut().query::<(Entity, &Enemy, &Health)>();
    q.iter(app.world()).map(|(e, en, h)| (e, en.clone(), h.cur)).collect()
}

/// Advances `n` ticks in world direction `dir` (camera facing it).
fn walk(app: &mut App, n: u32, dir: Vec3) {
    let cam_yaw = PlayerInput::quantize_yaw(math::yaw_of(dir));
    steps(app, n, PlayerInput { move_y: 127, cam_yaw, ..IDLE });
}

#[test]
fn walking_off_the_edge_is_a_fall_to_death_then_back_to_the_lantern() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    // The square is an ellipse above the void: walk straight east.
    put_player(&mut app, 6.0, -31.5, 0.0);
    walk(&mut app, 120, Vec3::X);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Fell { .. })));
    assert_eq!(app.world().get::<Player>(p).unwrap().state, PState::Dead);
    assert!(body(&mut app, p).pos.y < -6.0, "the body falls");
    steps(&mut app, psx_souls::sim::encounter::RESPAWN_TICKS + 2, IDLE);
    let p = player(&mut app);
    assert_eq!(app.world().get::<Player>(p).unwrap().state, PState::Free);
    assert!(body(&mut app, p).pos.distance(checkpoint_pos(&t, 0)) < 2.5);
}

#[test]
fn walls_hold_on_the_stairs() {
    let mut app = fresh_app();
    let p = player(&mut app);
    // Halfway up the stairs, pushing against the balustrade: you bump, you don't fall.
    put_player(&mut app, 0.0, -21.0, 0.0);
    walk(&mut app, 90, Vec3::X);
    assert!(!events(&mut app).iter().any(|e| matches!(e, SimEvent::Fell { .. })));
    let b = body(&mut app, p);
    assert!(b.pos.x < 1.6 && b.pos.y < -0.5, "{:?}", b.pos);
}

#[test]
fn hounds_wake_together_bite_and_drop_loot() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    let kennel: Vec<Entity> = enemies(&mut app).iter().filter(|(_, e, _)| e.group == 1).map(|(e, ..)| *e).collect();
    assert_eq!(kennel.len(), 2);
    // They're asleep: you can get a little closer.
    put_player(&mut app, -7.4, -46.0, std::f32::consts::PI);
    steps(&mut app, 30, IDLE);
    assert!(enemies(&mut app).iter().filter(|(_, e, _)| e.group == 1).all(|(_, e, _)| e.state == EState::Asleep));
    // Too close: one wakes up and wakes the other.
    put_player(&mut app, -9.8, -51.0, std::f32::consts::PI);
    step(&mut app, IDLE);
    assert!(enemies(&mut app).iter().filter(|(_, e, _)| e.group == 1).all(|(_, e, _)| e.state == EState::Chase));
    assert!(app.world().resource::<Encounter>().hunted);
    // They bite (without killing: a stationary player doesn't last long).
    let before = hp(&mut app, p);
    steps(&mut app, 120, IDLE);
    assert!(hp(&mut app, p) < before, "the dogs attack");

    // Finish off a dog (alone, the other is moved away): it disappears, leaving its loot (here,
    // always the ash).
    app.world_mut().despawn(kennel[1]);
    {
        let mut pl = app.world_mut().get_mut::<Player>(p).unwrap();
        pl.state = PState::Free;
        pl.stamina = t.player.max_stamina;
    }
    app.world_mut().get_mut::<Action>(p).unwrap().stop();
    let dog = kennel[0];
    app.world_mut().get_mut::<Health>(dog).unwrap().cur = 1.0;
    let dpos = body(&mut app, dog).pos;
    let pos = body(&mut app, p).pos;
    app.world_mut().get_mut::<Body>(p).unwrap().yaw = math::yaw_of(dpos - pos);
    app.world_mut().get_mut::<Health>(p).unwrap().cur = t.player.max_hp;
    let hound = t.enemy_kind("hound").unwrap() as usize;
    app.world_mut().resource_mut::<Tuning>().enemies[hound].loot =
        vec![psx_souls::sim::data::LootDef { item: Item::WardingAsh, chance: 1.0 }];
    // Hit at contact range (the dog is moved closer).
    app.world_mut().get_mut::<Body>(dog).unwrap().pos = pos + math::forward(math::yaw_of(dpos - pos)) * 1.2;
    app.world_mut().get_mut::<Action>(dog).unwrap().stop();
    app.world_mut().get_mut::<psx_souls::sim::fighter::Hitstop>(dog).unwrap().0 = 30;
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    steps(&mut app, 20, IDLE);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::EnemyDied { .. })));
    steps(&mut app, t.enemies[hound].death.total + psx_souls::sim::enemy::VANISH_TICKS + 40, IDLE);
    assert!(app.world().get_entity(dog).is_err(), "the body has disappeared");
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::EnemyVanished { .. })));
    let loot: Vec<Loot> = app.world_mut().query::<&Loot>().iter(app.world()).copied().collect();
    assert_eq!(loot.len(), 1);
    assert_eq!((loot[0].item, loot[0].owner), (Item::WardingAsh, 0));

    // Picked up: into the inventory, gone from the ground.
    app.world_mut().get_mut::<Body>(p).unwrap().pos = loot[0].pos + Vec3::new(0.6, 0.0, 0.0);
    steps(&mut app, 15, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::LootPicked { item: Item::WardingAsh, .. })));
    assert_eq!(app.world().get::<Player>(p).unwrap().inventory.count(Item::WardingAsh), 1);
    step(&mut app, IDLE);
    assert_eq!(app.world_mut().query::<&Loot>().iter(app.world()).count(), 0);
}

#[test]
fn loot_tables_are_valid() {
    let t = Tuning::builtin();
    for e in &t.enemies {
        let total: f32 = e.loot.iter().map(|l| l.chance).sum();
        assert!(total <= 1.0 + 1e-4, "{}: {total}", e.key);
        assert!(e.loot.iter().all(|l| l.chance > 0.0 && l.item.kind() == psx_souls::sim::items::Kind::Consumable && l.item != Item::HealFlask), "{}", e.key);
    }
}

#[test]
fn enemies_give_up_far_from_home_and_heal() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let (e, en, _) = enemies(&mut app).into_iter().find(|(_, e, _)| e.group == 1).unwrap();
    // Woken and wounded, then the player runs far away (back to the square).
    put_player(&mut app, -9.8, -51.0, std::f32::consts::PI);
    step(&mut app, IDLE);
    app.world_mut().get_mut::<Health>(e).unwrap().cur = 10.0;
    put_player(&mut app, 0.0, -30.0, 0.0);
    app.world_mut().get_mut::<Body>(e).unwrap().pos = Vec3::new(-4.0, -2.4, -37.0);
    let hound = t.enemy_kind("hound").unwrap() as usize;
    let mut back = false;
    for _ in 0..1200 {
        step(&mut app, IDLE);
        let cur = enemies(&mut app).into_iter().find(|(x, ..)| *x == e).unwrap();
        if cur.1.state != EState::Chase && cur.2 >= t.enemies[hound].max_hp {
            back = true;
            break;
        }
    }
    assert!(back, "it returns to its post and heals");
    assert!(body(&mut app, e).pos.distance(en.home) < 0.5);
}

#[test]
fn pickups_are_taken_once_and_kept_after_death() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    let [x, z] = t.level.pickups[0].pos;
    put_player(&mut app, x - 0.6, z, 0.0);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::PickedUp { pickup: 0, .. })));
    let (item, n) = t.level.pickups[0].items[0];
    assert_eq!(app.world().get::<Player>(p).unwrap().inventory.count(item), n);
    // A second time: nothing.
    steps(&mut app, 15, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(!events(&mut app).iter().any(|e| matches!(e, SimEvent::PickedUp { .. })));
    // Dying doesn't bring it back.
    app.world_mut().get_mut::<Health>(p).unwrap().cur = 0.0;
    app.world_mut().get_mut::<Player>(p).unwrap().state = PState::Dead;
    steps(&mut app, psx_souls::sim::encounter::RESPAWN_TICKS + 2, IDLE);
    let p = player(&mut app);
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!(pl.picked & 1, 1);
    assert_eq!(pl.inventory.count(item), n);
}

#[test]
fn boosts_are_temporary() {
    use psx_souls::sim::items::{ASH_DAMAGE, ASH_TICKS, ROOT_TICKS};
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    {
        let mut pl = app.world_mut().get_mut::<Player>(p).unwrap();
        pl.inventory.add(Item::WardingAsh, 1);
        pl.inventory.add(Item::VigorRoot, 1);
        pl.inventory.active = 1;
    }
    step(&mut app, PlayerInput { buttons: btn::ITEM, ..IDLE });
    steps(&mut app, t.player.heal.total + 2, IDLE);
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!(pl.inventory.count(Item::WardingAsh), 0);
    assert!(pl.ward_ticks > 0 && pl.ward_ticks <= ASH_TICKS);
    assert_eq!(pl.defense_mult(), ASH_DAMAGE);
    app.world_mut().get_mut::<Player>(p).unwrap().inventory.active = 2;
    step(&mut app, PlayerInput { buttons: btn::ITEM, ..IDLE });
    steps(&mut app, t.player.heal.total + 2, IDLE);
    assert!(app.world().get::<Player>(p).unwrap().vigor_ticks > 0);
    // It wears off.
    steps(&mut app, ASH_TICKS.max(ROOT_TICKS), IDLE);
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!((pl.ward_ticks, pl.vigor_ticks), (0, 0));
    assert_eq!(pl.defense_mult(), 1.0);
}

#[test]
fn cannot_rest_while_hunted_and_resting_brings_enemies_back() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let n = enemies(&mut app).len();
    // One dog killed, another on your heels: no rest.
    let (dog, ..) = enemies(&mut app).into_iter().find(|(_, e, _)| e.group == 1).unwrap();
    app.world_mut().despawn(dog);
    let (e, ..) = enemies(&mut app).into_iter().find(|(_, e, _)| e.group == 1).unwrap();
    {
        let mut en = app.world_mut().get_mut::<Enemy>(e).unwrap();
        en.state = EState::Chase;
    }
    app.world_mut().get_mut::<Body>(e).unwrap().pos = Vec3::new(-6.0, -2.4, -36.0);
    let (spawn, yaw) = checkpoint_spawn(&t, 0);
    put_player(&mut app, spawn.x, spawn.z, yaw);
    step(&mut app, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    assert!(!events(&mut app).iter().any(|e| matches!(e, SimEvent::Rested { .. })));
    // (The button, with nothing else to do, made them jump: wait for them to land.)
    // Once rid of it: rest, and everyone returns to their post.
    app.world_mut().despawn(e);
    steps(&mut app, 60, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    steps(&mut app, 2, IDLE);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Rested { .. })));
    let now = enemies(&mut app);
    assert_eq!(now.len(), n);
    assert!(now.iter().all(|(_, e, _)| e.state != EState::Chase));
}

#[test]
fn kindling_a_lantern_sets_respawn_and_allows_travel() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    // The colossus, unique, is already defeated (otherwise it guards the track just before).
    let (spawn, yaw) = checkpoint_spawn(&t, 1);
    put_player(&mut app, spawn.x, spawn.z, yaw);
    steps(&mut app, 2, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    let ev = events(&mut app);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Kindled { checkpoint: 1, .. })));
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!((pl.checkpoint, pl.found), (1, 0b11));
    // Travel to the first lantern.
    app.world_mut().resource_mut::<SimCommands>().0.push(SimCommand::Travel { player: 0, checkpoint: 0 });
    steps(&mut app, 2, IDLE);
    let p = player(&mut app);
    assert!(body(&mut app, p).pos.distance(checkpoint_pos(&t, 0)) < 2.5);
    let pl = app.world().get::<Player>(p).unwrap();
    assert_eq!((pl.checkpoint, pl.found), (0, 0b11));
}

#[test]
fn unique_enemy_stays_dead() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    let (boss_e, ..) = enemies(&mut app).into_iter().find(|(_, e, _)| e.unique).unwrap();
    let bpos = body(&mut app, boss_e).pos;
    // At contact range, facing it, one HP left.
    put_player(&mut app, bpos.x, bpos.z - 1.3, 0.0);
    app.world_mut().get_mut::<Health>(boss_e).unwrap().cur = 1.0;
    app.world_mut().get_mut::<psx_souls::sim::fighter::Hitstop>(boss_e).unwrap().0 = 30;
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    steps(&mut app, 20, IDLE);
    assert!(app.world().get::<Player>(p).unwrap().slain != 0);
    // Rest: the others come back, not it.
    app.world_mut().resource_mut::<Encounter>().respawn_enemies = true;
    steps(&mut app, 2, IDLE);
    assert!(!enemies(&mut app).iter().any(|(_, e, _)| e.unique));
    assert_eq!(enemies(&mut app).len(), t.level.enemies.len() - 1);
}

#[test]
fn fights_with_enemies_are_deterministic() {
    let run = || {
        let mut app = fresh_app();
        put_player(&mut app, -3.7, -64.0, std::f32::consts::PI);
        for i in 0..900u32 {
            let mut inp = scripted_input(i);
            inp.cam_yaw = PlayerInput::quantize_yaw(std::f32::consts::PI);
            step(&mut app, inp);
        }
        state_hash(app.world_mut())
    };
    assert_eq!(run(), run());
}

/// Adds a platform to the level west of the square, separated from it by 2.5 m of void.
fn add_islet(app: &mut App) {
    use psx_souls::sim::data::{FloorDef, FloorStyle, Shape};
    let mut t = app.world_mut().resource_mut::<Tuning>();
    t.level.floors.push(FloorDef {
        shape: Shape::Ellipse { center: [-15.0, -31.5], radii: [2.5, 2.5], y: -2.4 },
        walled: false,
        arena: false,
        ledge: false,
        sealed: false,
        steps: 0,
        style: FloorStyle::default(),
    });
}

#[test]
fn jumping_in_place_lands_back_and_costs_stamina() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    put_player(&mut app, 0.0, -31.5, 0.0);
    steps(&mut app, 2, IDLE);
    events(&mut app);
    let y = body(&mut app, p).pos.y;
    step(&mut app, PlayerInput { buttons: btn::JUMP, ..IDLE });
    steps(&mut app, 15, IDLE);
    assert!(body(&mut app, p).pos.y > y + 0.5, "in the air");
    assert!(app.world().get::<Player>(p).unwrap().stamina < t.player.max_stamina);
    steps(&mut app, 30, IDLE);
    let ev = events(&mut app);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Jumped { .. })));
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Landed { .. })));
    assert!(!app.world().get::<Player>(p).unwrap().airborne);
    assert!((body(&mut app, p).pos.y - y).abs() < 1e-4);
}

#[test]
fn a_running_jump_clears_a_gap_that_walking_falls_into() {
    let west = Vec3::NEG_X;
    let cam_yaw = PlayerInput::quantize_yaw(math::yaw_of(west));
    let run = PlayerInput { move_y: 127, cam_yaw, ..IDLE };
    // Without jumping: the fall.
    let mut app = fresh_app();
    add_islet(&mut app);
    put_player(&mut app, -6.0, -31.5, 0.0);
    walk(&mut app, 90, west);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Fell { .. })));
    // Running, jump at the edge: you end up on the islet.
    let mut app = fresh_app();
    add_islet(&mut app);
    let p = player(&mut app);
    put_player(&mut app, -4.0, -31.5, 0.0);
    step(&mut app, PlayerInput { buttons: btn::SPRINT, ..run });
    for _ in 0..120 {
        if body(&mut app, p).pos.x < -9.4 {
            break;
        }
        step(&mut app, run);
    }
    step(&mut app, PlayerInput { buttons: btn::JUMP, ..run });
    steps(&mut app, 45, IDLE);
    let ev = events(&mut app);
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::Fell { .. })), "no fall");
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Landed { .. })));
    assert!(body(&mut app, p).pos.x < -12.5, "on the islet ({:?})", body(&mut app, p).pos);
    // A jump over the void, too short: the fall.
    put_player(&mut app, -13.0, -31.5, 0.0);
    steps(&mut app, 2, IDLE);
    step(&mut app, PlayerInput { buttons: btn::JUMP, ..IDLE });
    walk(&mut app, 80, Vec3::X);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Fell { .. })));
}

#[test]
fn interact_picks_up_instead_of_jumping() {
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    let at = t.level.pickups[0].pos;
    put_player(&mut app, at[0] + 0.5, at[1], 0.0);
    steps(&mut app, 2, IDLE);
    step(&mut app, PlayerInput { buttons: btn::INTERACT | btn::JUMP, ..IDLE });
    steps(&mut app, 2, IDLE);
    let ev = events(&mut app);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PickedUp { pickup: 0, .. })));
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::Jumped { .. })));
    assert!(!app.world().get::<Player>(p).unwrap().airborne);
}

#[test]
fn interact_alone_never_jumps() {
    // Keyboard: interact (E) and jump (F) are separate keys.
    let mut app = fresh_app();
    let p = player(&mut app);
    put_player(&mut app, 0.0, -31.5, 0.0);
    steps(&mut app, 2, IDLE);
    events(&mut app);
    step(&mut app, PlayerInput { buttons: btn::INTERACT, ..IDLE });
    steps(&mut app, 2, IDLE);
    assert!(!events(&mut app).iter().any(|e| matches!(e, SimEvent::Jumped { .. })));
    assert!(!app.world().get::<Player>(p).unwrap().airborne);
}

#[test]
fn reloading_at_a_brazier_faces_the_way_on() {
    use psx_souls::sim::encounter::Progress;
    let mut app = fresh_app();
    let t = tuning(&app);
    for cp in 0..t.level.checkpoints.len() {
        // Save made right against the brazier, facing the fire.
        let fire = checkpoint_pos(&t, cp);
        let at = fire + Vec3::new(0.0, 0.0, -1.2);
        let mut progress = Progress::new_game(&t);
        progress.pos = Some([at.x, at.z, 0.0]);
        let mut reset = app.world_mut().resource_mut::<psx_souls::sim::ResetFight>();
        reset.requested = true;
        reset.progress = Some(progress);
        steps(&mut app, 2, IDLE);
        let p = player(&mut app);
        let b = body(&mut app, p);
        let (spawn, yaw) = checkpoint_spawn(&t, cp);
        assert!(b.pos.distance(spawn) < 0.05, "at the usual spot");
        assert!(math::wrap(b.yaw - yaw).abs() < 1e-3);
        // Facing `look`, and the fire isn't behind (the camera doesn't go through it).
        let c = &t.level.checkpoints[cp];
        let to_look = Vec3::new(c.look[0], 0.0, c.look[1]) - fire;
        assert!(math::wrap(yaw - math::yaw_of(to_look)).abs() < 1e-3);
        let behind = -math::forward(yaw);
        let to_fire = (fire - b.pos).normalize();
        assert!(Vec3::new(to_fire.x, 0.0, to_fire.z).normalize().dot(behind) < 0.5);
    }
}

fn bosses(app: &mut App) -> Vec<(Entity, u8)> {
    let mut v: Vec<_> = app.world_mut().query::<(Entity, &Boss)>().iter(app.world()).map(|(e, b)| (e, b.def)).collect();
    v.sort_by_key(|x| x.1);
    v
}

#[test]
fn every_encounter_spawns_its_members_and_fights() {
    let t = Tuning::builtin();
    for (i, enc) in t.encounters.iter().enumerate() {
        let mut app = encounter_app(i as u8);
        assert_eq!(bosses(&mut app).len(), enc.members.len(), "{}", enc.name.get());
        let p = player(&mut app);
        let mut used = std::collections::HashSet::new();
        let mut hurt = false;
        // The player circles the arena (taking hits, but not dying).
        for tick in 0..4000u32 {
            let a = tick as f32 * 0.004;
            let want = Vec3::new(a.cos() * 7.0, 0.0, a.sin() * 7.0);
            {
                let mut b = app.world_mut().get_mut::<Body>(p).unwrap();
                if tick % 400 < 200 {
                    b.pos = want;
                }
            }
            let mut h = app.world_mut().get_mut::<Health>(p).unwrap();
            hurt |= h.cur < h.max;
            h.cur = h.max;
            step(&mut app, IDLE);
            for (_, a) in app.world_mut().query::<(&Boss, &Action)>().iter(app.world()) {
                if let Some(MoveRef::BossAttack(d, k)) = a.mv {
                    used.insert((d, k));
                }
            }
        }
        assert!(hurt, "{}: the player was never hit", enc.name.get());
        assert!(used.len() >= 3, "{}: too few attacks used ({used:?})", enc.name.get());
    }
}

#[test]
fn every_boss_attack_runs_and_casts_its_spells() {
    let t = Tuning::builtin();
    for (i, enc) in t.encounters.iter().enumerate().skip(1) {
        let mut app = encounter_app(i as u8);
        app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
        for (b, def) in bosses(&mut app) {
            let bd = &t.bosses[def as usize];
            for (k, a) in bd.attacks.iter().enumerate() {
                let p = player(&mut app);
                app.world_mut().get_mut::<Health>(p).unwrap().cur = 1.0e6;
                app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::ZERO;
                app.world_mut().get_mut::<Body>(b).unwrap().yaw = 0.0;
                app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, 6.0);
                app.world_mut().get_mut::<Action>(b).unwrap().start(MoveRef::BossAttack(def, k as u16), 6.0);
                events(&mut app);
                let mut casts = 0;
                for _ in 0..a.mv.total {
                    step(&mut app, IDLE);
                    casts += events(&mut app).iter().filter(|e| matches!(e, SimEvent::SpellCast { .. })).count();
                }
                assert_eq!(casts, a.mv.casts.len(), "{}/{}/{}", enc.name.get(), bd.key, a.name);
                // The spells eventually disappear.
                steps(&mut app, 400, IDLE);
                let left = app.world_mut().query::<&psx_souls::sim::spell::Spell>().iter(app.world()).count();
                assert_eq!(left, 0, "{}/{}: spells left in play", bd.key, a.name);
            }
        }
    }
}

#[test]
fn spells_of_one_attack_hit_only_once() {
    let t = Tuning::builtin();
    let giant = t.boss_kind("giant").unwrap();
    let choice = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == "giant")).unwrap() as u8;
    let mut app = encounter_app(choice);
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    let (b, p) = (bosses(&mut app)[0].0, player(&mut app));
    let idx = t.bosses[giant as usize].attacks.iter().position(|a| a.name == "brasier").unwrap();
    app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::ZERO;
    // The player stands where the three rings of flames overlap.
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, 4.0);
    app.world_mut().get_mut::<Action>(b).unwrap().start(MoveRef::BossAttack(giant, idx as u16), 4.0);
    app.world_mut().get::<Health>(p).unwrap();
    let before = hp(&mut app, p);
    let mut hits = 0;
    for _ in 0..200 {
        app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, 4.0);
        step(&mut app, IDLE);
        hits += events(&mut app).iter().filter(|e| matches!(e, SimEvent::Hit { foe: None, .. })).count();
    }
    assert_eq!(hits, 1);
    assert!(hp(&mut app, p) < before);
}

#[test]
fn lock_on_switches_between_parts_and_members() {
    let t = Tuning::builtin();
    let choice = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == "dragon")).unwrap() as u8;
    let mut app = encounter_app(choice);
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    let p = player(&mut app);
    let b = bosses(&mut app)[0].0;
    // Facing the dragon: locking on targets the point closest to the camera axis.
    app.world_mut().get_mut::<Body>(b).unwrap().pos = Vec3::new(0.0, 0.0, 6.0);
    app.world_mut().get_mut::<Body>(b).unwrap().yaw = std::f32::consts::PI;
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, -6.0);
    let cam = PlayerInput::quantize_yaw(0.0);
    step(&mut app, PlayerInput { buttons: btn::LOCK, cam_yaw: cam, ..IDLE });
    let first = app.world().get::<Player>(p).unwrap().lock_part;
    assert_eq!(app.world().get::<Player>(p).unwrap().lock, Some(b));
    step(&mut app, PlayerInput { cam_yaw: cam, ..IDLE });
    step(&mut app, PlayerInput { buttons: btn::TARGET_RIGHT, cam_yaw: cam, ..IDLE });
    let right = app.world().get::<Player>(p).unwrap().lock_part;
    assert_ne!(first, right, "switch target to the right");
    step(&mut app, PlayerInput { cam_yaw: cam, ..IDLE });
    step(&mut app, PlayerInput { buttons: btn::TARGET_LEFT, cam_yaw: cam, ..IDLE });
    step(&mut app, PlayerInput { cam_yaw: cam, ..IDLE });
    step(&mut app, PlayerInput { buttons: btn::TARGET_LEFT, cam_yaw: cam, ..IDLE });
    let left = app.world().get::<Player>(p).unwrap().lock_part;
    assert_ne!(left, right);

    // Duo: switch from one to the other.
    let duo = t.encounters.iter().position(|e| e.members.len() == 2).unwrap() as u8;
    let mut app = encounter_app(duo);
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    let p = player(&mut app);
    let pair = bosses(&mut app);
    app.world_mut().get_mut::<Body>(pair[0].0).unwrap().pos = Vec3::new(-3.0, 0.0, 0.0);
    app.world_mut().get_mut::<Body>(pair[1].0).unwrap().pos = Vec3::new(3.0, 0.0, 0.0);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, -6.0);
    step(&mut app, PlayerInput { buttons: btn::LOCK, cam_yaw: cam, ..IDLE });
    let a = app.world().get::<Player>(p).unwrap().lock;
    step(&mut app, PlayerInput { cam_yaw: cam, ..IDLE });
    // Seen from the south, the first one (x = -3) is on the right.
    let dir = if a == Some(pair[0].0) { btn::TARGET_LEFT } else { btn::TARGET_RIGHT };
    step(&mut app, PlayerInput { buttons: dir, cam_yaw: cam, ..IDLE });
    let b2 = app.world().get::<Player>(p).unwrap().lock;
    assert!(a.is_some() && b2.is_some() && a != b2, "{a:?} → {b2:?}");
}

#[test]
fn duo_partner_enrages_and_minions_do_not_block_victory() {
    let t = Tuning::builtin();
    let duo = t.encounters.iter().position(|e| e.members.len() == 2).unwrap() as u8;
    let mut app = encounter_app(duo);
    let pair = bosses(&mut app);
    app.world_mut().get_mut::<Health>(pair[0].0).unwrap().cur = 0.0;
    // (once its current attack is over)
    steps(&mut app, 240, IDLE);
    assert_eq!(app.world().get::<Boss>(pair[1].0).unwrap().phase, 2, "the survivor enters phase 2");
    app.world_mut().get_mut::<Health>(pair[1].0).unwrap().cur = 0.0;
    steps(&mut app, 3, IDLE);
    let enc = *app.world().resource::<Encounter>();
    assert!(enc.defeated != 0 && !enc.active);

    // The butcher: his dogs don't prevent victory, and fall with him.
    let butcher = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == "butcher")).unwrap() as u8;
    let mut app = encounter_app(butcher);
    let all = bosses(&mut app);
    let main = all.iter().find(|(_, d)| !t.bosses[*d as usize].minor).unwrap().0;
    app.world_mut().get_mut::<Health>(main).unwrap().cur = 0.0;
    steps(&mut app, 3, IDLE);
    assert!(app.world().resource::<Encounter>().is_defeated(butcher));
    for (e, _) in all {
        assert!(app.world().get::<Health>(e).unwrap().dead());
    }
}

/// Encounter with boss `key` alone, passive, placed in the centre facing +z; the player at `at`.
fn lone_boss(key: &str, at: Vec3) -> (App, Entity, u8, Entity) {
    let t = Tuning::builtin();
    let choice = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == key)).unwrap() as u8;
    let mut app = encounter_app(choice);
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    let (b, def) = bosses(&mut app).into_iter().find(|&(_, d)| t.bosses[d as usize].key == key).unwrap();
    let p = player(&mut app);
    {
        let mut bb = app.world_mut().get_mut::<Body>(b).unwrap();
        bb.pos = Vec3::ZERO;
        bb.yaw = 0.0;
    }
    app.world_mut().get_mut::<Body>(p).unwrap().pos = at;
    *app.world_mut().get_mut::<Action>(b).unwrap() = Action::default();
    (app, b, def, p)
}

fn start_boss_attack(app: &mut App, b: Entity, def: u8, name: &str, dist: f32) {
    let t = Tuning::builtin();
    let idx = t.bosses[def as usize].attacks.iter().position(|a| a.name == name).expect(name);
    app.world_mut().get_mut::<Action>(b).unwrap().start(MoveRef::BossAttack(def, idx as u16), dist);
    events(app);
}

#[test]
fn dragon_breath_is_one_beam_that_hits_once_and_stops_with_its_attack() {
    let (mut app, b, def, p) = lone_boss("dragon", Vec3::new(0.0, 0.0, 9.0));
    start_boss_attack(&mut app, b, def, "souffle", 9.0);
    let (mut hits, mut casts) = (0, 0);
    for _ in 0..176 {
        app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(0.0, 0.0, 9.0);
        step(&mut app, IDLE);
        for e in events(&mut app) {
            hits += matches!(e, SimEvent::Hit { foe: None, .. }) as u32;
            casts += matches!(e, SimEvent::SpellCast { .. }) as u32;
        }
    }
    assert_eq!((casts, hits), (1, 1), "a single stream, which only hits once");

    // Interrupted (staggered), the stream dies out at once.
    let (mut app, b, def, _) = lone_boss("dragon", Vec3::new(0.0, 0.0, -9.0));
    start_boss_attack(&mut app, b, def, "souffle", 9.0);
    steps(&mut app, 70, IDLE);
    let beams = |app: &mut App| app.world_mut().query::<&psx_souls::sim::spell::Spell>().iter(app.world()).count();
    assert_eq!(beams(&mut app), 1);
    app.world_mut().get_mut::<Action>(b).unwrap().start(MoveRef::Boss(def, BossMove::Groggy), 0.0);
    steps(&mut app, 2, IDLE);
    assert_eq!(beams(&mut app), 0);
}

#[test]
fn big_beast_pivots_a_quarter_turn_to_tail_whip_its_flank() {
    // The target on its left flank (+x is its left when it faces +z).
    let (mut app, b, def, p) = lone_boss("dragon", Vec3::new(4.5, 0.0, -1.0));
    start_boss_attack(&mut app, b, def, "pivot_gauche", 4.6);
    let before = hp(&mut app, p);
    let total = Tuning::builtin().bosses[def as usize].attacks.iter().find(|a| a.name == "pivot_gauche").unwrap().mv.total;
    steps(&mut app, total - 1, IDLE);
    let yaw = body(&mut app, b).yaw;
    assert!((math::wrap(yaw + std::f32::consts::FRAC_PI_2)).abs() < 0.05, "quarter turn to the right: {yaw}");
    assert!(hp(&mut app, p) < before, "the tail whips the left flank");
}

#[test]
fn big_beast_does_not_track_its_target_exactly() {
    let (mut app, b, _, p) = lone_boss("dragon", Vec3::new(0.0, 0.0, 12.0));
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    // The target weaves in front of it, within its cone: it doesn't turn.
    for tick in 0..240u32 {
        let x = (tick as f32 * 0.05).sin() * 3.0;
        app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(x, 0.0, 12.0 + body(&mut app, b).pos.z);
        step(&mut app, IDLE);
        assert!(body(&mut app, b).yaw.abs() < 0.01, "tick {tick}: it turned");
    }
    // Out of the cone (90° to its left, far away): it turns roughly towards it.
    let at = body(&mut app, b).pos + Vec3::new(12.0, 0.0, 0.0);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = at;
    steps(&mut app, 180, IDLE);
    let to = math::yaw_of(at - body(&mut app, b).pos);
    let err = math::wrap(to - body(&mut app, b).yaw).abs().to_degrees();
    assert!(err < 35.0, "offset {err}°");
}

#[test]
fn attacking_in_the_air_is_a_jump_attack_that_lands() {
    use psx_souls::sim::data::{MoveRef, WeaponMove};
    let mut app = fresh_app();
    let p = player(&mut app);
    put_player(&mut app, 0.0, -31.5, 0.0);
    steps(&mut app, 2, IDLE);
    let y = body(&mut app, p).pos.y;
    step(&mut app, PlayerInput { buttons: btn::JUMP, ..IDLE });
    steps(&mut app, 8, IDLE);
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    let a = app.world().get::<psx_souls::sim::fighter::Action>(p).unwrap().mv;
    assert_eq!(a, Some(MoveRef::Weapon(0, WeaponMove::Jump)));
    steps(&mut app, 60, IDLE);
    let pl = app.world().get::<Player>(p).unwrap();
    assert!(!pl.airborne && pl.state == PState::Free);
    assert!((body(&mut app, p).pos.y - y).abs() < 1e-4);
}

#[test]
fn up_and_down_switch_between_high_and_low_lock_points() {
    let mut app = new_app();
    let t = tuning(&app);
    // The wyvern: its head is much higher than its legs.
    let dragon = t.boss_kind("dragon").unwrap();
    let b = boss(&mut app);
    *app.world_mut().get_mut::<psx_souls::sim::boss::Boss>(b).unwrap() = psx_souls::sim::boss::Boss::new(&t, dragon, 0);
    {
        let mut bb = app.world_mut().get_mut::<Body>(b).unwrap();
        bb.pos = Vec3::ZERO;
        bb.yaw = 0.0;
    }
    let p = player(&mut app);
    {
        let mut pb = app.world_mut().get_mut::<Body>(p).unwrap();
        pb.pos = Vec3::new(0.0, 0.0, 12.0);
        pb.yaw = std::f32::consts::PI;
    }
    let cam_yaw = PlayerInput::quantize_yaw(std::f32::consts::PI);
    step(&mut app, PlayerInput { buttons: btn::LOCK, cam_yaw, ..IDLE });
    step(&mut app, PlayerInput { cam_yaw, ..IDLE });
    let part = |app: &mut App| app.world().get::<Player>(p).unwrap().lock_part;
    assert!(app.world().get::<Player>(p).unwrap().lock.is_some());
    step(&mut app, PlayerInput { buttons: btn::TARGET_UP, cam_yaw, ..IDLE });
    step(&mut app, PlayerInput { cam_yaw, ..IDLE });
    assert_eq!(part(&mut app), 0, "the head");
    step(&mut app, PlayerInput { buttons: btn::TARGET_DOWN, cam_yaw, ..IDLE });
    assert_ne!(part(&mut app), 0, "a leg");
}

#[test]
fn the_brooch_islet_by_the_belvedere_needs_a_running_jump() {
    let east = Vec3::X;
    let cam_yaw = PlayerInput::quantize_yaw(math::yaw_of(east));
    let run = PlayerInput { move_y: 127, cam_yaw, ..IDLE };
    // Walking: the fall.
    let mut app = fresh_app();
    let p = player(&mut app);
    put_player(&mut app, 12.0, -115.2, 0.0);
    walk(&mut app, 120, east);
    assert!(events(&mut app).iter().any(|e| matches!(e, SimEvent::Fell { .. })));
    // Running (sprint), jump at the edge: the islet, and the talisman in range.
    let mut app = fresh_app();
    let t = tuning(&app);
    let p2 = player(&mut app);
    put_player(&mut app, 9.0, -115.2, 0.0);
    steps(&mut app, 2, IDLE);
    events(&mut app);
    step(&mut app, PlayerInput { buttons: btn::SPRINT, ..run });
    for _ in 0..200 {
        if body(&mut app, p2).pos.x > 13.7 {
            break;
        }
        step(&mut app, run);
    }
    step(&mut app, PlayerInput { buttons: btn::JUMP, ..run });
    steps(&mut app, 40, run);
    steps(&mut app, 20, IDLE);
    let ev = events(&mut app);
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::Fell { .. })), "no fall ({:?})", body(&mut app, p2).pos);
    let brooch = t.level.pickups.iter().find(|k| k.items.iter().any(|(i, _)| *i == Item::IronBrooch)).unwrap().pos;
    let at = body(&mut app, p2).pos;
    assert!(math::flat_len(at - Vec3::new(brooch[0], at.y, brooch[1])) < 1.8, "on the islet ({at:?})");
    let _ = p;
}

#[test]
fn dragon_headbutt_turns_toward_a_target_off_its_axis() {
    // It aims badly (heading_slack): the target is 30° off its axis when it attacks.
    let a = 30f32.to_radians();
    let at = Vec3::new(a.sin(), 0.0, a.cos()) * 6.5;
    let (mut app, b, def, p) = lone_boss("dragon", at);
    start_boss_attack(&mut app, b, def, "coup_de_tete", 6.5);
    let before = hp(&mut app, p);
    for _ in 0..112 {
        app.world_mut().get_mut::<Body>(p).unwrap().pos = at;
        step(&mut app, IDLE);
    }
    assert!(hp(&mut app, p) < before, "the headbutt reaches them");
}

#[test]
fn aoe_circle_shows_early_stays_put_and_marks_the_impact() {
    use psx_souls::sim::boss::{MIN_WARNING, aoe_telegraph};
    let (mut app, b, def, p) = lone_boss("butcher", Vec3::new(0.0, 0.0, 3.0));
    start_boss_attack(&mut app, b, def, "fendoir", 3.0);
    let t = Tuning::builtin();
    let mut first: Option<(u32, Vec3)> = None;
    let mut impact = None;
    for tick in 0..140u32 {
        // The target circles him: neither the circle nor the hit follow it once the warning is shown.
        let a = tick as f32 * 0.03;
        app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(a.sin(), 0.0, a.cos()) * 3.0;
        step(&mut app, IDLE);
        let w = app.world();
        if let Some((pos, ..)) = aoe_telegraph(w.get::<Body>(b).unwrap(), w.get::<Action>(b).unwrap(), &t) {
            match first {
                None => first = Some((tick, pos)),
                Some((_, p0)) => assert!(p0.distance(pos) < 0.05, "tick {tick}: the circle moved"),
            }
        }
        for e in events(&mut app) {
            if let SimEvent::Shockwave { pos, .. } = e {
                impact = Some((tick, pos));
            }
        }
    }
    let ((shown, marked), (hit, pos)) = (first.expect("warning circle"), impact.expect("shockwave"));
    assert!(hit - shown >= MIN_WARNING, "warning of only {} ticks", hit - shown);
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    assert!(flat(marked).distance(flat(pos)) < 0.1, "the impact lands in the circle");
}

#[test]
fn a_bolt_that_misses_burns_on_the_ground_for_a_moment() {
    use psx_souls::sim::spell::{SPLASH_LIFE, Spell};
    let (mut app, b, def, p) = lone_boss("marionette", Vec3::new(0.0, 0.0, 7.0));
    start_boss_attack(&mut app, b, def, "bond_arriere", 7.0);
    // The needles start from well above her.
    let mut cast_y = None;
    for _ in 0..40 {
        step(&mut app, IDLE);
        for e in events(&mut app) {
            if let SimEvent::SpellCast { pos, .. } = e {
                cast_y = Some(pos.y);
            }
        }
    }
    assert!(cast_y.expect("needles thrown") > 8.0);
    // They hang for a moment, then head for their target.
    let at = |app: &mut App| app.world_mut().query::<&Spell>().iter(app.world()).map(|s| s.pos).collect::<Vec<_>>();
    let hung = at(&mut app);
    steps(&mut app, 10, IDLE);
    assert_eq!(at(&mut app), hung, "hanging");
    steps(&mut app, 20, IDLE);
    assert_ne!(at(&mut app), hung, "launched");
    // Dodged (the target moved aside), they crash on the ground...
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(14.0, 0.0, -10.0);
    let landed = |app: &mut App| {
        app.world_mut().query::<&Spell>().iter(app.world()).filter(|s| s.landed.is_some()).map(|s| s.pos).next()
    };
    let mut spot = None;
    for _ in 0..120 {
        step(&mut app, IDLE);
        if let Some(s) = landed(&mut app) {
            spot = Some(s);
            break;
        }
    }
    let spot = spot.expect("a needle on the ground");
    // ...and burn there: whoever walks in is hit, then the puddle dies out.
    let before = hp(&mut app, p);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(spot.x, 0.0, spot.z);
    steps(&mut app, 3, IDLE);
    assert!(hp(&mut app, p) < before, "the puddle burns");
    app.world_mut().get_mut::<Body>(p).unwrap().pos = Vec3::new(14.0, 0.0, -10.0);
    steps(&mut app, SPLASH_LIFE + 120, IDLE);
    assert!(landed(&mut app).is_none());
}

#[test]
fn dead_hounds_do_not_shield_the_butcher() {
    let t = Tuning::builtin();
    let butcher = t.encounters.iter().position(|e| e.members.iter().any(|m| m.boss == "butcher")).unwrap() as u8;
    let mut app = encounter_app(butcher);
    app.world_mut().resource_mut::<SimDebug>().boss_passive = true;
    let t = tuning(&app);
    let bosses: Vec<(Entity, bool)> =
        app.world_mut().query::<(Entity, &Boss)>().iter(app.world()).map(|(e, b)| (e, b.def(&t).minor)).collect();
    let main = bosses.iter().find(|(_, m)| !m).unwrap().0;
    let p = player(&mut app);
    // The butcher at the origin, the player 2.4 m in front of it, the dogs dead between them.
    for (e, minor) in &bosses {
        app.world_mut().get_mut::<Action>(*e).unwrap().stop();
        let mut b = app.world_mut().get_mut::<Body>(*e).unwrap();
        b.pos = if *minor { Vec3::new(0.0, 0.0, 1.2) } else { Vec3::ZERO };
        b.yaw = 0.0;
    }
    for (e, minor) in &bosses {
        if *minor {
            app.world_mut().get_mut::<Health>(*e).unwrap().cur = 0.0;
            let def = app.world().get::<Boss>(*e).unwrap().def;
            app.world_mut().get_mut::<Action>(*e).unwrap().start(MoveRef::Boss(def, BossMove::Death), 0.0);
        }
    }
    {
        let mut pb = app.world_mut().get_mut::<Body>(p).unwrap();
        pb.pos = Vec3::new(0.0, 0.0, 2.4);
        pb.yaw = std::f32::consts::PI;
    }
    app.world_mut().get_mut::<Player>(p).unwrap().weapon = 0;
    let before = hp(&mut app, main);
    events(&mut app);
    step(&mut app, PlayerInput { buttons: btn::LIGHT, ..IDLE });
    steps(&mut app, 30, IDLE);
    assert!(hp(&mut app, main) < before, "the rapier reaches the butcher through the corpses");
    // The impact (white flash) is on the butcher, not on a corpse lying near the impact point.
    let hits: Vec<_> = events(&mut app).into_iter().filter_map(|e| match e {
        SimEvent::Hit { foe, .. } => Some(foe),
        _ => None,
    }).collect();
    assert_eq!(hits, vec![Some(main)]);
}

#[test]
fn stuck_chasers_give_up_and_let_the_player_rest() {
    // Everyone alerted, the player up at the Belvedere: those who can't reach them (the colossus
    // at the foot of its stairs, a dog on its ledge…) end up going back to their post.
    let mut app = fresh_app();
    let t = tuning(&app);
    let p = player(&mut app);
    app.world_mut().get_mut::<Body>(p).unwrap().pos = checkpoint_spawn(&t, 1).0;
    let mut q = app.world_mut().query::<&mut psx_souls::sim::enemy::Enemy>();
    for mut e in q.iter_mut(app.world_mut()) {
        e.state = psx_souls::sim::enemy::EState::Chase;
        e.target = Some(p);
    }
    steps(&mut app, 2, IDLE);
    assert!(app.world().resource::<Encounter>().hunted);
    steps(&mut app, 600, IDLE);
    assert!(!app.world().resource::<Encounter>().hunted);
}
