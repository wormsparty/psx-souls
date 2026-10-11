//! Exports action timings (taken from the RONs) as JSON for the Blender scripts,
//! so that animations line up with the frame data, along with the arena and the level
//! (the scenery is built from the same data as the collisions).
//!
//! `cargo run --bin export_timings > tools/blender/timings.json`

use serde_json::{Map, Value, json};
use psx_souls::sim::data::{MoveDef, Tuning};

fn mv(d: &MoveDef) -> Value {
    json!({
        "total": d.total,
        "hits": d.hits.iter().map(|h| [h.start, h.end]).collect::<Vec<_>>(),
        "motion": d.motion.iter().map(|m| json!({"start": m.start, "end": m.end, "speed": m.speed})).collect::<Vec<_>>(),
        "iframes": d.iframes,
        "casts": d.casts.iter().map(|c| c.at).collect::<Vec<_>>(),
    })
}

fn main() {
    let t = Tuning::builtin();
    let mut player = Map::new();
    let p = &t.player;
    for d in [&p.dodge, &p.backstep, &p.guard_hit, &p.perfect_guard, &p.guard_break, &p.hit_light, &p.hit_heavy, &p.switch, &p.death, &p.heal] {
        player.insert(d.anim.clone(), mv(d));
    }
    for w in &t.weapons {
        let mut all: Vec<&MoveDef> = w.light.iter().collect();
        all.extend([&w.heavy, &w.heavy_charged, &w.special, &w.fatal, &w.jump]);
        for d in all {
            player.insert(d.anim.clone(), mv(d));
        }
        player.insert(w.charge_anim.clone(), json!({"total": w.charge_ticks, "hits": [], "motion": []}));
    }
    let mut boss = Map::new();
    let b = &t.bosses[0];
    for d in [&b.groggy, &b.fatal_received, &b.roar, &b.death] {
        boss.insert(d.anim.clone(), mv(d));
    }
    for a in &b.attacks {
        boss.insert(a.mv.anim.clone(), mv(&a.mv));
    }
    // Other bosses: per model (except those reusing an enemy model), the first one that
    // uses an animation provides its timings.
    let mut bosses = Map::new();
    for b in t.bosses.iter().skip(1).filter(|b| !t.enemies.iter().any(|k| k.model == b.model)) {
        let Value::Object(m) = bosses.entry(b.model.clone()).or_insert_with(|| json!({})) else { continue };
        for d in [&b.groggy, &b.fatal_received, &b.roar, &b.death].into_iter().chain(b.attacks.iter().map(|a| &a.mv)) {
            m.entry(d.anim.clone()).or_insert_with(|| mv(d));
        }
    }
    // Enemies: per model, the first type using it provides the animation timings
    // (the others are retimed to them at runtime).
    let mut enemies = Map::new();
    for k in &t.enemies {
        let Value::Object(m) = enemies.entry(k.model.clone()).or_insert_with(|| json!({})) else { continue };
        for d in [&k.alert, &k.hit, &k.death].into_iter().chain(k.attacks.iter().map(|a| &a.mv)) {
            m.entry(d.anim.clone()).or_insert_with(|| mv(d));
        }
    }
    // Encounters (menu portraits): model, scale and offset of each member.
    let encounters: Vec<Value> = t
        .encounters
        .iter()
        .map(|e| {
            Value::Array(
                e.members
                    .iter()
                    .filter_map(|m| t.bosses.iter().find(|b| b.key == m.boss))
                    .zip(&e.members)
                    .map(|(b, m)| json!({"model": b.model, "scale": b.scale, "offset": m.offset, "minor": b.minor}))
                    .collect(),
            )
        })
        .collect();
    let out = json!({
        "encounters": encounters,
        "player": player,
        "boss": boss,
        "bosses": bosses,
        "enemies": enemies,
        "switch_at": p.switch_at,
        "heal_at": p.heal_at,
        "arenas": serde_json::to_value(&t.arenas).unwrap(),
        // Colour of each encounter's main boss (torches, banners, medallions).
        "colors": (0..t.encounters.len()).map(|i| t.encounter_color(i)).collect::<Vec<_>>(),
        "level": serde_json::to_value(&t.level).unwrap(),
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
