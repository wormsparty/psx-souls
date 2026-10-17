//! Co-op tests: rollback (a replayed tick gives exactly the same state), the same simulation
//! whatever Bevy's storage order, and the party's rules (respawn, menu decisions in the inputs,
//! travel, leaving).

use bevy::prelude::*;
use ggrs::SessionBuilder;
use psx_souls::net::{Coop, run_requests};
use psx_souls::sim::boss::Boss;
use psx_souls::sim::enemy::Enemy;
use psx_souls::sim::data::Tuning;
use psx_souls::sim::encounter::{self, Encounter, Progress, RESPAWN_TICKS, SimCommand};
use psx_souls::sim::fighter::{Body, Health};
use psx_souls::sim::input::{MAX_PLAYERS, PlayerInput, PlayerInputs, btn};
use psx_souls::sim::player::{PState, Player};
use psx_souls::sim::world::Zone;
use psx_souls::sim::{ResetFight, SimEntity, SimEvent, SimEvents, SimId, SimPlugin, SimSchedule, snapshot, stable_hash, state_hash};

/// A game for `players`, starting from `progress` (the guests: copies of the first).
fn party(players: u8, progress: Progress) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    *app.world_mut().resource_mut::<ResetFight>() = ResetFight { requested: true, players, progress: Some(progress), guests: Vec::new() };
    app
}

fn step(app: &mut App, inputs: &[PlayerInput]) {
    let mut all = PlayerInputs::default();
    all.0[..inputs.len()].copy_from_slice(inputs);
    *app.world_mut().resource_mut::<PlayerInputs>() = all;
    app.world_mut().run_schedule(SimSchedule);
}

fn idle(app: &mut App, n: u32) {
    for _ in 0..n {
        step(app, &[PlayerInput::default(); MAX_PLAYERS]);
    }
}

/// A busy player: moves, attacks, dodges, guards, locks on, switches weapon, heals.
fn scripted(i: u32, who: usize) -> PlayerInput {
    let x = (i / 20).wrapping_mul(2654435761).wrapping_add(who as u32 * 0x9E37_79B9) ^ 0x5bd1_e995;
    let buttons = match (x >> 16) % 10 {
        0 | 1 => btn::LIGHT,
        2 => btn::HEAVY,
        3 => btn::DODGE,
        4 => btn::GUARD,
        5 if i % 20 < 2 => btn::LOCK,
        6 if i % 400 < 5 => btn::SWITCH,
        7 if i % 300 < 5 => btn::ITEM,
        8 => btn::SPECIAL | btn::JUMP,
        _ => 0,
    };
    // Pressed then released, so that the presses follow one another.
    let buttons = if i % 20 < 10 { buttons } else { 0 };
    PlayerInput {
        buttons,
        move_x: ((x & 0xff) as i32 - 128).clamp(-127, 127) as i8,
        move_y: (((x >> 8) & 0xff) as i32 - 128).clamp(-127, 127) as i8,
        cam_yaw: PlayerInput::quantize_yaw(i as f32 * 0.003 + who as f32),
        cmd: [0; 4],
    }
}

fn players(app: &mut App) -> Vec<(Entity, Player, Body, Health)> {
    let w = app.world_mut();
    let mut v: Vec<_> = w.query::<(Entity, &Player, &Body, &Health)>().iter(w).map(|(e, p, b, h)| (e, p.clone(), *b, *h)).collect();
    v.sort_by_key(|x| x.1.id);
    v
}

fn player(app: &mut App, id: u8) -> (Entity, Player, Body, Health) {
    players(app).into_iter().find(|p| p.1.id == id).expect("player")
}

fn kill(app: &mut App, id: u8) {
    let e = player(app, id).0;
    let mut em = app.world_mut().entity_mut(e);
    em.get_mut::<Health>().unwrap().cur = 0.0;
    em.get_mut::<Player>().unwrap().state = PState::Dead;
}

fn events(app: &mut App) -> Vec<SimEvent> {
    std::mem::take(&mut app.world_mut().resource_mut::<SimEvents>().0)
}

/// GGRS's own check: every tick is replayed (back up to 7 ticks), the state must come out
/// identical. Fights against every boss encounter, then the path with its enemies.
#[test]
fn replayed_ticks_give_the_same_state() {
    let t = Tuning::builtin();
    let starts = (0..t.encounters.len() as u8).map(Some).chain([None]);
    for arena in starts {
        let mut app = party(2, Progress { arena, ..Progress::new_game(&t) });
        let mut session = SessionBuilder::<Coop>::new()
            .with_num_players(2)
            .unwrap()
            .with_check_distance(7)
            .start_synctest_session()
            .unwrap();
        let mut presented = 0;
        for f in 0..360 {
            for h in 0..2 {
                session.add_local_input(h, scripted(f, h)).unwrap();
            }
            match session.advance_frame() {
                Ok(requests) => run_requests(app.world_mut(), requests, &mut presented),
                Err(e) => panic!("arena {arena:?}, frame {f}: {e}"),
            }
        }
    }
}

/// Going back to a saved state and replaying the same inputs leads to the same place, even
/// after entities came and went (spells, deaths, respawn).
#[test]
fn a_restored_snapshot_replays_identically() {
    let t = Tuning::builtin();
    let mut app = party(2, Progress { arena: Some(1), ..Progress::new_game(&t) });
    for i in 0..120 {
        step(&mut app, &[scripted(i, 0), scripted(i, 1)]);
    }
    let snap = snapshot::save(app.world_mut());
    let run = |app: &mut App| {
        let mut hashes = Vec::new();
        for i in 120..900 {
            step(app, &[scripted(i, 0), scripted(i, 1)]);
            hashes.push(state_hash(app.world_mut()));
        }
        hashes
    };
    let first = run(&mut app);
    snapshot::load(app.world_mut(), &snap);
    app.world_mut().flush();
    let second = run(&mut app);
    assert_eq!(first, second);
}

/// Marker the second game adds and removes on the fly: it moves the entities around Bevy's
/// tables, as the presentation does (models, `LocalPlayer`), differently on each machine.
#[derive(Component)]
struct Shuffled;

/// Two machines don't store the entities in the same order (nor give them the same `Entity`):
/// the simulation must not depend on it.
#[test]
fn storage_order_does_not_change_the_simulation() {
    let t = Tuning::builtin();
    for arena in [Some(2u8), Some(5), None] {
        let mut a = party(2, Progress { arena, ..Progress::new_game(&t) });
        let mut b = App::new();
        // Other entities first: other `Entity` values.
        for _ in 0..37 {
            b.world_mut().spawn_empty();
        }
        b.add_plugins(SimPlugin);
        *b.world_mut().resource_mut::<ResetFight>() = ResetFight { requested: true, players: 2, progress: Some(Progress { arena, ..Progress::new_game(&t) }), guests: Vec::new() };
        for i in 0..1200 {
            let inputs = [scripted(i, 0), scripted(i, 1)];
            step(&mut a, &inputs);
            step(&mut b, &inputs);
            let w = b.world_mut();
            let sim: Vec<(Entity, SimId, bool)> = w.query_filtered::<(Entity, &SimId, Has<Shuffled>), With<SimEntity>>().iter(w).map(|(e, id, s)| (e, *id, s)).collect();
            for (e, id, shuffled) in sim {
                if (id.0 + i) % 7 == 0 {
                    if shuffled {
                        w.entity_mut(e).remove::<Shuffled>();
                    } else {
                        w.entity_mut(e).insert(Shuffled);
                    }
                }
            }
            assert_eq!(state_hash(a.world_mut()), state_hash(b.world_mut()), "arena {arena:?}, tick {i}");
        }
    }
}

#[test]
fn guests_bring_their_own_character() {
    let t = Tuning::builtin();
    let host = Progress::new_game(&t);
    let mut guest = Progress::new_game(&t);
    guest.weapon = 1;
    guest.inventory.slots = [None; 4];
    guest.picked = 0b101;
    let mut app = party(2, host.clone());
    app.world_mut().resource_mut::<ResetFight>().guests = vec![guest.clone()];
    idle(&mut app, 1);
    let (p0, p1) = (player(&mut app, 0).1, player(&mut app, 1).1);
    assert_eq!((p0.weapon, p1.weapon), (0, 1));
    assert_eq!(p1.inventory.slots, [None; 4]);
    assert_eq!((p0.picked, p1.picked), (0, 0b101));
    // Side by side at the host's checkpoint.
    let (b0, b1) = (player(&mut app, 0).2, player(&mut app, 1).2);
    assert!(b0.pos.distance(b1.pos) < 2.0);
}

/// Fallen alone in the level: back at their checkpoint, the others and the world untouched.
#[test]
fn a_fallen_player_gets_back_up_alone_at_the_checkpoint() {
    let t = Tuning::builtin();
    let mut app = party(2, Progress::new_game(&t));
    idle(&mut app, 2);
    // The host waits a little way off; the guest wanders off, and falls.
    let (e0, e1) = (player(&mut app, 0).0, player(&mut app, 1).0);
    app.world_mut().get_mut::<Body>(e0).unwrap().pos += Vec3::new(-2.0, 0.0, 0.0);
    app.world_mut().get_mut::<Body>(e1).unwrap().pos += Vec3::new(3.0, 0.0, 3.0);
    idle(&mut app, 1);
    let host_before = player(&mut app, 0).2.pos;
    kill(&mut app, 1);
    events(&mut app);
    idle(&mut app, RESPAWN_TICKS + 2);
    let ev = events(&mut app);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Revived { entity } if *entity == e1)));
    assert!(!ev.contains(&SimEvent::Respawned), "the world must not start over");
    let (_, p1, b1, h1) = player(&mut app, 1);
    assert_eq!(p1.state, PState::Free);
    assert_eq!(h1.cur, h1.max);
    let (spawn, _) = encounter::checkpoint_spawn(&t, p1.checkpoint as usize);
    assert!(b1.pos.distance(spawn) < 0.5);
    assert_eq!(player(&mut app, 0).2.pos, host_before);
}

/// Fallen during a boss fight: they wait for its outcome, then get back up in the arena.
#[test]
fn a_player_fallen_in_a_boss_fight_waits_then_gets_up_on_victory() {
    let t = Tuning::builtin();
    let mut app = party(2, Progress { arena: Some(0), ..Progress::new_game(&t) });
    idle(&mut app, 3);
    assert!(app.world().resource::<Encounter>().active);
    kill(&mut app, 1);
    idle(&mut app, RESPAWN_TICKS + 30);
    assert_eq!(player(&mut app, 1).1.state, PState::Dead, "still watching the fight");
    // The host fells the boss.
    let w = app.world_mut();
    let bosses: Vec<Entity> = w.query_filtered::<Entity, With<Boss>>().iter(w).collect();
    for b in bosses {
        w.get_mut::<Health>(b).unwrap().cur = 0.0;
    }
    idle(&mut app, 2);
    let (_, p1, _, h1) = player(&mut app, 1);
    assert_eq!((p1.state, p1.zone), (PState::Free, Zone::Arena(0)));
    assert_eq!(h1.cur, h1.max);
}

#[test]
fn everyone_down_starts_the_world_over() {
    let t = Tuning::builtin();
    let mut app = party(2, Progress { arena: Some(0), ..Progress::new_game(&t) });
    idle(&mut app, 3);
    kill(&mut app, 0);
    kill(&mut app, 1);
    events(&mut app);
    idle(&mut app, RESPAWN_TICKS + 2);
    assert!(events(&mut app).contains(&SimEvent::Respawned));
    assert!(players(&mut app).iter().all(|p| p.1.zone == Zone::Level && p.1.state == PState::Free));
}

/// A menu decision travels in the input: applied once, even if the input is repeated (GGRS
/// predicts a late input by repeating the last one).
#[test]
fn menu_decisions_in_the_inputs_apply_once() {
    let t = Tuning::builtin();
    let mut app = party(2, Progress::new_game(&t));
    idle(&mut app, 1);
    let flask = player(&mut app, 1).1.inventory.slots[0];
    assert!(flask.is_some());
    let cmd = SimCommand::Equip { player: 1, slot: 0, item: None }.encode(7);
    let input = PlayerInput { cmd, ..default() };
    step(&mut app, &[PlayerInput::default(), input]);
    assert_eq!(player(&mut app, 1).1.inventory.slots[0], None);
    assert_eq!(player(&mut app, 0).1.inventory.slots[0], flask, "the other player keeps theirs");
    // Put back by hand: the same decision repeated doesn't apply again.
    let e1 = player(&mut app, 1).0;
    app.world_mut().get_mut::<Player>(e1).unwrap().inventory.slots[0] = flask;
    for _ in 0..5 {
        step(&mut app, &[PlayerInput::default(), input]);
    }
    assert_eq!(player(&mut app, 1).1.inventory.slots[0], flask);
}

#[test]
fn commands_survive_their_encoding() {
    use psx_souls::sim::items::Item;
    for c in [
        SimCommand::Equip { player: 2, slot: 3, item: Some(Item::VigorRoot) },
        SimCommand::Equip { player: 2, slot: 1, item: None },
        SimCommand::EquipTalisman { player: 2, item: Some(Item::CrestPlume) },
        SimCommand::EquipTalisman { player: 2, item: None },
        SimCommand::ReviveBoss(4),
        SimCommand::Travel { player: 2, checkpoint: 1 },
        SimCommand::Leave { player: 2 },
    ] {
        let raw = c.encode(9);
        assert_ne!(raw[0], 0);
        assert_eq!(SimCommand::decode(2, raw), Some(c));
    }
    assert_eq!(SimCommand::Leave { player: 0 }.encode(0)[0], 1, "never number 0 (= nothing)");
}

/// A player gone (left, connection lost): their character vanishes, the game goes on.
#[test]
fn a_player_who_leaves_vanishes() {
    let t = Tuning::builtin();
    let mut app = party(2, Progress::new_game(&t));
    idle(&mut app, 1);
    let leave = PlayerInput { cmd: SimCommand::Leave { player: 1 }.encode(u8::MAX), ..default() };
    step(&mut app, &[PlayerInput::default(), leave]);
    idle(&mut app, 1);
    let ids: Vec<u8> = players(&mut app).iter().map(|p| p.1.id).collect();
    assert_eq!(ids, vec![0]);
}

/// Travel: the whole party goes, each with their own character.
#[test]
fn the_whole_party_travels_together() {
    let t = Tuning::builtin();
    let host = Progress { found: 0b11, ..Progress::new_game(&t) };
    let guest = Progress { weapon: 1, found: 0b11, ..Progress::new_game(&t) };
    let mut app = party(2, host);
    app.world_mut().resource_mut::<ResetFight>().guests = vec![guest];
    idle(&mut app, 1);
    let travel = PlayerInput { cmd: SimCommand::Travel { player: 1, checkpoint: 1 }.encode(1), ..default() };
    step(&mut app, &[PlayerInput::default(), travel]);
    idle(&mut app, 1);
    let (spawn, _) = encounter::checkpoint_spawn(&t, 1);
    for (_, p, b, _) in players(&mut app) {
        assert_eq!(p.checkpoint, 1);
        assert!(b.pos.distance(spawn) < 2.0, "player {} at {:?}", p.id, b.pos);
    }
    assert_eq!(player(&mut app, 1).1.weapon, 1);
}

#[test]
fn the_state_hash_is_the_same_on_32_and_64_bits() {
    assert_eq!(stable_hash(&5usize), stable_hash(&5u64));
    assert_eq!(stable_hash(&(-3isize)), stable_hash(&(-3i64)));
}

/// Max and current health of the path enemies (by spawn) and of the bosses (by definition).
#[allow(clippy::type_complexity)]
fn foe_health(app: &mut App) -> (Vec<(u16, f32, f32)>, Vec<(u8, f32, f32)>) {
    let w = app.world_mut();
    let mut enemies: Vec<_> = w.query::<(&Enemy, &Health)>().iter(w).map(|(e, h)| (e.spawn, h.max, h.cur)).collect();
    enemies.sort_by_key(|e| e.0);
    let mut bosses: Vec<_> = w.query::<(&Boss, &Health)>().iter(w).map(|(b, h)| (b.def, h.max, h.cur)).collect();
    bosses.sort_by_key(|b| b.0);
    (enemies, bosses)
}

/// For two: path enemies +30% health, bosses +100% (their supporting roles, the dogs: +30%);
/// back to normal when the partner leaves.
#[test]
fn opponents_are_sturdier_for_two() {
    let t = Tuning::builtin();
    let butcher = t.encounters.iter().position(|e| e.members.len() > 1 && e.members.iter().any(|m| t.boss_kind(&m.boss).is_some_and(|d| t.bosses[d as usize].minor))).expect("an encounter with minor members") as u8;
    let start = Progress { arena: Some(butcher), ..Progress::new_game(&t) };
    let mut solo = party(1, start.clone());
    let mut duo = party(2, start);
    idle(&mut solo, 3);
    idle(&mut duo, 3);
    let ((e1, b1), (e2, b2)) = (foe_health(&mut solo), foe_health(&mut duo));
    assert!(!e1.is_empty() && !b1.is_empty());
    for (a, b) in e1.iter().zip(&e2) {
        assert!((b.1 / a.1 - 1.3).abs() < 1e-4, "enemy {}: {} → {}", a.0, a.1, b.1);
    }
    for (a, b) in b1.iter().zip(&b2) {
        let want = if t.bosses[a.0 as usize].minor { 1.3 } else { 2.0 };
        assert!((b.1 / a.1 - want).abs() < 1e-4, "boss {}: {} → {}", a.0, a.1, b.1);
        assert_eq!(b.1, b.2, "starts at full health");
    }
    // The partner leaves: back to the solo values.
    let leave = PlayerInput { cmd: SimCommand::Leave { player: 1 }.encode(9), ..default() };
    step(&mut duo, &[PlayerInput::default(), leave]);
    let (e3, b3) = foe_health(&mut duo);
    for (a, b) in e1.iter().zip(&e3) {
        assert!((b.1 - a.1).abs() < 1e-3);
    }
    for (a, b) in b1.iter().zip(&b3) {
        assert!((b.1 - a.1).abs() < 1e-3);
    }
}

#[test]
fn the_password_picks_the_room() {
    use psx_souls::net::room_name;
    assert_eq!(room_name("psx-souls", ""), "psx-souls");
    assert_eq!(room_name("psx-souls", "  "), "psx-souls");
    assert_eq!(room_name("psx-souls", "lantern"), room_name("psx-souls", "lantern"));
    assert_ne!(room_name("psx-souls", "lantern"), room_name("psx-souls", "Lantern"));
    assert!(!room_name("psx-souls", "lantern").contains("lantern"), "not in clear");
}
