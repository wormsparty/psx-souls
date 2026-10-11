//! Walkable geometry: the level (floors hanging above the void: stairs, square, paths, bridges)
//! and, apart, the boss arenas (walled). Each piece of floor has a height (ramps and stairs on a
//! continuous slope). Past a walled edge you bump; past an open edge, you fall. A floor higher
//! than a step is a wall (the base of a gallery, the side of stairs); from it, you jump down.
//!
//! Everything uses deterministic maths (`math`): this is simulation.

use bevy::prelude::*;

use super::data::{ArenaDef, FloorDef, PortalDef, Shape, Tuning};
use super::math;

/// Where floors are: the level, or a boss arena (index in `Tuning::arenas`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Zone {
    #[default]
    Level,
    Arena(u8),
}

/// Who is moving: determines the reachable floors and what open edges mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mover {
    /// Player, in the level or in an arena; an open edge makes them fall. `door_open`: the final
    /// door is open (all bosses defeated), the floors behind it are reachable.
    Player { zone: Zone, door_open: bool },
    /// The bosses of an arena (never on its ledges).
    Boss(u8),
    /// A boss that jumped onto one of its arena's ledges: it walks on them too, but never off
    /// their edge (it jumps back down).
    Climber(u8),
    /// Path enemies: not in the corridors to the bosses, and they never jump into the void.
    Enemy,
}

impl Mover {
    fn is_player(self) -> bool {
        matches!(self, Mover::Player { .. })
    }
}

/// All the floors, with their zone.
fn pieces(t: &Tuning) -> impl Iterator<Item = (Zone, &FloorDef)> + '_ {
    t.level
        .floors
        .iter()
        .map(|f| (Zone::Level, f))
        .chain(t.arenas.iter().enumerate().flat_map(|(i, a)| a.floors.iter().map(move |f| (Zone::Arena(i as u8), f))))
}

fn allowed(zone: Zone, f: &FloorDef, m: Mover) -> bool {
    match m {
        Mover::Player { zone: z, door_open } => zone == z && (door_open || !f.sealed),
        Mover::Boss(i) => zone == Zone::Arena(i) && !f.ledge,
        Mover::Climber(i) => zone == Zone::Arena(i),
        Mover::Enemy => zone == Zone::Level && !f.arena && !f.sealed,
    }
}

/// Margin to keep from the edge: a body of radius `r` bumps against a wall, but
/// can step out over the void up to its centre. Enemies stay well on the floor.
fn margin(f: &FloorDef, m: Mover, r: f32) -> f32 {
    if f.walled || !m.is_player() { r } else { 0.0 }
}

/// Wall thickness, outside the walled floor (stair balustrades, tools/blender/arena.py).
const WALL_THICKNESS: f32 = 0.4;
/// Highest step you climb walking; a floor higher than that is a wall.
pub const STEP_UP: f32 = 0.6;
/// A floor this much lower: the player jumps down onto it (they don't teleport down).
const DROP: f32 = 0.6;
/// A floor this much higher is a ceiling: you go under it.
const CEILING: f32 = 5.0;

/// Tolerance of the "inside" tests: the arenas are far from the origin (f32 precision).
const EPS: f32 = 1e-3;

/// Result of an "inside the piece" test: point brought back onto the piece (with the margin),
/// distance to that point (0 if inside), floor height at that spot.
struct Fit {
    point: Vec3,
    dist: f32,
    y: f32,
}

fn fit(shape: &Shape, pos: Vec3, m: f32) -> Fit {
    fit_ends(shape, pos, m, [false; 2])
}

/// Same, with the margin also at the ends of a strip that are `closed` (start, end): dead ends.
fn fit_ends(shape: &Shape, pos: Vec3, m: f32, closed: [bool; 2]) -> Fit {
    match *shape {
        Shape::Ellipse { center: [cx, cz], radii: [rx, rz], y } => {
            let (rx, rz) = ((rx - m).max(0.05), (rz - m).max(0.05));
            let (dx, dz) = (pos.x - cx, pos.z - cz);
            let k = math::sqrt((dx / rx) * (dx / rx) + (dz / rz) * (dz / rz));
            if k <= 1.0 {
                return Fit { point: pos, dist: 0.0, y };
            }
            // Radial projection (approximate, but continuous) onto the edge of the ellipse.
            let p = Vec3::new(cx + dx / k, pos.y, cz + dz / k);
            Fit { point: p, dist: math::flat_len(pos - p), y }
        }
        Shape::Strip { from: [x0, z0, y0], to: [x1, z1, y1], half_width } => {
            let a = Vec3::new(x0, 0.0, z0);
            let ab = Vec3::new(x1 - x0, 0.0, z1 - z0);
            let len = math::flat_len(ab).max(1e-4);
            let dir = ab / len;
            let side = Vec3::new(dir.z, 0.0, -dir.x);
            let rel = Vec3::new(pos.x, 0.0, pos.z) - a;
            // No margin at the ends (they connect to other pieces), except at a dead end.
            let lo = if closed[0] { m.max(0.0).min(len / 2.0) } else { 0.0 };
            let hi = if closed[1] { len - m.max(0.0).min(len / 2.0) } else { len };
            let along = rel.dot(dir).clamp(lo, hi);
            let hw = (half_width - m).max(0.0);
            let across = rel.dot(side).clamp(-hw, hw);
            let p = a + dir * along + side * across;
            let y = y0 + (y1 - y0) * (along / len);
            let p = Vec3::new(p.x, pos.y, p.z);
            Fit { point: p, dist: math::flat_len(pos - p), y }
        }
    }
}

/// A body of radius `r` overlapping the solid base of a floor that's too high to step onto is
/// pushed out of it: radially for a platform, sideways for stairs (their ends connect).
fn push_out(shape: &Shape, pos: Vec3, r: f32) -> Option<Vec3> {
    let f = fit(shape, pos, -r);
    if f.dist > EPS || !(pos.y + STEP_UP..pos.y + CEILING).contains(&f.y) {
        return None;
    }
    match *shape {
        Shape::Ellipse { center: [cx, cz], radii: [rx, rz], .. } => {
            let (rx, rz) = (rx + r, rz + r);
            let (dx, dz) = if (pos.x - cx).abs() + (pos.z - cz).abs() < 1e-4 { (rx, 0.0) } else { (pos.x - cx, pos.z - cz) };
            let k = math::sqrt((dx / rx) * (dx / rx) + (dz / rz) * (dz / rz)).max(1e-4);
            Some(Vec3::new(cx + dx / k * 1.001, pos.y, cz + dz / k * 1.001))
        }
        Shape::Strip { from: [x0, z0, _], to: [x1, z1, _], half_width } => {
            let dir = Vec3::new(x1 - x0, 0.0, z1 - z0).normalize_or(Vec3::Z);
            let side = Vec3::new(dir.z, 0.0, -dir.x);
            let across = (Vec3::new(pos.x - x0, 0.0, pos.z - z0)).dot(side);
            let out = if across >= 0.0 { half_width + r } else { -(half_width + r) };
            Some(pos + side * (out - across) * 1.001)
        }
    }
}

/// Where a body that wants to go to `pos` ends up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// On the floor (adjusted position, including height).
    Ground(Vec3),
    /// Above a lower floor (a player walking off a gallery): they jump down, from this position.
    Drop(Vec3),
    /// Above the void: it falls.
    Fall,
}

/// Brings `pos` (body of radius `r`) back onto a floor reachable by `m`. Among the pieces that
/// contain the point, keep the highest one that's at most a step above the feet (stairs over a
/// yard). A body touching the wall of a walled floor bumps, even if the nearest floor is
/// open (at the foot of the stairs, along the balustrades, next to the landing).
pub fn step(t: &Tuning, pos: Vec3, r: f32, m: Mover) -> Step {
    let mut pos = pos;
    for (_, f) in pieces(t) {
        if let Some(p) = push_out(&f.shape, pos, r) {
            pos = p;
        }
    }
    let mut inside: Option<f32> = None;
    let mut nearest: Option<(Fit, bool)> = None;
    let mut against_wall = false;
    for (_, f) in pieces(t).filter(|(z, f)| allowed(*z, f, m)) {
        if f.walled && fit(&f.shape, pos, -(WALL_THICKNESS + r)).dist <= EPS {
            against_wall = true;
        }
        let closed = if f.walled && m.is_player() { dead_ends(t, f, m) } else { [false; 2] };
        let fp = fit_ends(&f.shape, pos, margin(f, m, r), closed);
        if fp.y > pos.y + STEP_UP || (matches!(m, Mover::Climber(_)) && fp.y < pos.y - DROP) {
            continue;
        }
        if fp.dist <= EPS {
            if inside.is_none_or(|y| fp.y > y) {
                inside = Some(fp.y);
            }
        } else if nearest.as_ref().is_none_or(|(n, _)| fp.dist < n.dist) {
            nearest = Some((fp, f.walled));
        }
    }
    if let Some(y) = inside {
        if m.is_player() && y < pos.y - DROP {
            return Step::Drop(pos);
        }
        return Step::Ground(Vec3::new(pos.x, y, pos.z));
    }
    match nearest {
        // Open edge, for the player: the void.
        Some((_, false)) if m.is_player() && !against_wall => Step::Fall,
        Some((f, _)) => Step::Ground(Vec3::new(f.point.x, f.y, f.point.z)),
        None => Step::Ground(pos),
    }
}

/// The ends (start, end) of a walled strip that lead nowhere for `m`: the end of a corridor at
/// a boss's fog or portcullis, the porch of the closed final door. You bump into them with your
/// whole body (and not half into the bars).
fn dead_ends(t: &Tuning, f: &FloorDef, m: Mover) -> [bool; 2] {
    let Shape::Strip { from: [x0, z0, y0], to: [x1, z1, y1], .. } = f.shape else { return [false; 2] };
    let leads = |x: f32, z: f32, y: f32| {
        let p = Vec3::new(x, y, z);
        pieces(t)
            .filter(|(zone, g)| !std::ptr::eq(*g, f) && allowed(*zone, g, m))
            .any(|(_, g)| {
                let q = fit(&g.shape, p, 0.0);
                q.dist <= 0.05 && (q.y - y).abs() <= STEP_UP
            })
    };
    [!leads(x0, z0, y0), !leads(x1, z1, y1)]
}

/// Floor height under (x, z), if any (the one closest to `near_y`), whatever the zone.
pub fn floor_at(t: &Tuning, x: f32, z: f32, near_y: f32) -> Option<f32> {
    floor_below(t, x, z, near_y, None)
}

/// Same, among the floors reachable by `m` (`None`: all of them).
pub fn floor_below(t: &Tuning, x: f32, z: f32, near_y: f32, m: Option<Mover>) -> Option<f32> {
    let pos = Vec3::new(x, near_y, z);
    let mut best: Option<f32> = None;
    for (_, f) in pieces(t).filter(|(z, f)| m.is_none_or(|m| allowed(*z, f, m))) {
        let fp = fit(&f.shape, pos, 0.0);
        if fp.dist <= EPS && best.is_none_or(|y| (fp.y - near_y).abs() < (y - near_y).abs()) {
            best = Some(fp.y);
        }
    }
    best
}

/// A boss standing a step above its arena's ground: on a gallery or the stairs, where only a
/// climbing jump takes it.
pub fn perched(t: &Tuning, pos: Vec3, arena: u8) -> bool {
    floor_below(t, pos.x, pos.z, pos.y, Some(Mover::Boss(arena))).is_some_and(|g| pos.y > g + STEP_UP)
}

/// Where a boss of radius `r` jumping to `pos` sets foot: on the floor of its arena (ledges
/// included) nearest to it, the difference in height counting as much as the distance — on the
/// gallery it aimed for, and not at the foot of its wall.
pub fn landing(t: &Tuning, pos: Vec3, r: f32, arena: u8) -> Vec3 {
    let m = Mover::Climber(arena);
    pieces(t)
        .filter(|(z, f)| allowed(*z, f, m))
        .map(|(_, f)| fit(&f.shape, pos, margin(f, m, r)))
        .min_by(|a, b| (a.dist + (a.y - pos.y).abs()).total_cmp(&(b.dist + (b.y - pos.y).abs())))
        .map_or(pos, |f| Vec3::new(f.point.x, f.y, f.point.z))
}

/// Zone of a position: the arena whose floors contain it, otherwise the level.
pub fn zone_at(t: &Tuning, pos: Vec3) -> Zone {
    pieces(t)
        .find(|(z, f)| *z != Zone::Level && fit(&f.shape, pos, 0.0).dist <= EPS)
        .map_or(Zone::Level, |(z, _)| z)
}

/// Centre and radius of the circle enclosing the arena's floors.
pub fn arena_bounds(a: &ArenaDef) -> (Vec3, f32) {
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for f in &a.floors {
        let (min, max) = match f.shape {
            Shape::Ellipse { center: [cx, cz], radii: [rx, rz], .. } => (Vec2::new(cx - rx, cz - rz), Vec2::new(cx + rx, cz + rz)),
            Shape::Strip { from: [x0, z0, _], to: [x1, z1, _], half_width: h } => {
                (Vec2::new(x0.min(x1) - h, z0.min(z1) - h), Vec2::new(x0.max(x1) + h, z0.max(z1) + h))
            }
        };
        lo = lo.min(min);
        hi = hi.max(max);
    }
    let c = (lo + hi) * 0.5;
    (Vec3::new(c.x, 0.0, c.y), (hi - lo).length() * 0.5)
}

/// Height of the balustrade around a walled platform of the level (tools/blender/arena.py).
const BALUSTRADE: f32 = 0.95;

/// First wall crossed by the segment `from` → `to` (seen from above): fraction of the segment where it
/// is hit (`None`: nothing in the way). Walls: the edges of walled platforms (the arenas) and the
/// sides of walled strips (stairs, corridors). Used by the camera, which must not go behind them.
pub fn wall_hit(t: &Tuning, from: Vec3, to: Vec3) -> Option<f32> {
    let (a, d) = (Vec2::new(from.x, from.z), Vec2::new(to.x - from.x, to.z - from.z));
    let mut best: Option<f32> = None;
    let mut hit = |s: f32| {
        if (0.0..=1.0).contains(&s) && best.is_none_or(|b| s < b) {
            best = Some(s);
        }
    };
    let walled: Vec<(Zone, &FloorDef)> = pieces(t).filter(|(_, f)| f.walled).collect();
    // Inside a walled platform (the threshold of a strip that enters it doesn't count).
    let in_room = |p: Vec2| {
        walled.iter().any(|(_, f)| matches!(f.shape, Shape::Ellipse { .. }) && fit(&f.shape, Vec3::new(p.x, 0.0, p.y), 0.0).dist <= EPS)
    };
    for &(zone, f) in &walled {
        match f.shape {
            Shape::Ellipse { center: [cx, cz], radii: [rx, rz], y } => {
                // In the frame where the ellipse is the unit circle: |a' + s·d'| = 1.
                let a2 = Vec2::new((a.x - cx) / rx, (a.y - cz) / rz);
                let d2 = Vec2::new(d.x / rx, d.y / rz);
                let (qa, qb, qc) = (d2.dot(d2), 2.0 * a2.dot(d2), a2.dot(a2) - 1.0);
                let disc = qb * qb - 4.0 * qa * qc;
                // In the level, only a low balustrade (the last terrace): you see over it.
                let over = |s: f32| zone == Zone::Level && from.y + (to.y - from.y) * s > y + BALUSTRADE;
                if qa > 1e-8 && disc >= 0.0 {
                    let sq = math::sqrt(disc);
                    for s in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
                        if !over(s) {
                            hit(s);
                        }
                    }
                }
            }
            Shape::Strip { from: [x0, z0, _], to: [x1, z1, _], half_width } => {
                let (p0, p1) = (Vec2::new(x0, z0), Vec2::new(x1, z1));
                let side = (p1 - p0).perp().normalize_or_zero() * half_width;
                for o in [side, -side] {
                    let (w0, w) = (p0 + o, p1 - p0);
                    let den = d.perp_dot(w);
                    if den.abs() < 1e-8 {
                        continue;
                    }
                    let s = (w0 - a).perp_dot(w) / den;
                    let u = (w0 - a).perp_dot(d) / den;
                    if (0.0..=1.0).contains(&u) && !in_room(a + d * s) {
                        hit(s);
                    }
                }
            }
        }
    }
    best
}

/// Circular obstacles (x, z, radius): arena pillars, braziers, torches, decor.
pub fn obstacles(t: &Tuning, checkpoint_radius: f32) -> Vec<[f32; 3]> {
    let mut v: Vec<[f32; 3]> = t.arenas.iter().flat_map(|a| a.pillars.iter().copied()).collect();
    for c in &t.level.checkpoints {
        v.push([c.pos[0], c.pos[1], checkpoint_radius]);
    }
    for a in &t.arenas {
        v.push([a.torch[0], a.torch[1], TORCH_RADIUS]);
    }
    for p in &t.level.props {
        let yaw = p.yaw.to_radians();
        for [dx, dz, r] in p.kind.colliders() {
            let w = math::local_to_world(Vec3::new(p.pos[0], 0.0, p.pos[1]), yaw, [*dx, 0.0, *dz]);
            v.push([w.x, w.z, *r]);
        }
    }
    v
}

/// Collision radius of a boss torch.
pub const TORCH_RADIUS: f32 = 0.25;

/// Ground point of a fog passage (centre of the fog), and the direction in which you go through it.
pub fn portal(t: &Tuning, p: &PortalDef) -> (Vec3, Vec3) {
    let base = Vec3::new(p.pos[0], 0.0, p.pos[1]);
    let dir = (Vec3::new(p.look[0], 0.0, p.look[1]) - base).normalize_or(Vec3::Z);
    // The fog is at the end of its floor: its height is that of the floor just before it.
    let probe = base - dir * 0.5;
    let y = floor_at(t, probe.x, probe.z, 0.0).unwrap_or(0.0);
    (Vec3::new(base.x, y, base.z), dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: Mover = Mover::Player { zone: Zone::Level, door_open: false };

    fn ground(s: Step) -> Vec3 {
        match s {
            Step::Ground(p) => p,
            s => panic!("unexpected {s:?}"),
        }
    }

    #[test]
    fn arenas_are_walled_and_apart_from_the_level() {
        let t = Tuning::builtin();
        let r = 0.4;
        let theatre = Mover::Player { zone: Zone::Arena(0), door_open: false };
        let (c, _) = arena_bounds(&t.arenas[0]);
        // Against the theatre wall: bump.
        let p = ground(step(&t, c + Vec3::new(20.0, 0.0, 0.0), r, theatre));
        assert!((math::flat_len(p - c) - (16.0 - r)).abs() < 1e-3);
        // Through its door: neither the player inside nor the bosses go out.
        let (door, dir) = portal(&t, &t.arenas[0].door);
        let out = door - dir * 1.3;
        for m in [theatre, Mover::Boss(0)] {
            let q = ground(step(&t, out, r, m));
            assert!(math::flat_len(q - c) <= 16.0 - r + 1e-3);
        }
        // From the level, the landing at the top of the stairs ends at the fog, with your whole
        // body (and later the portcullis) in front of it.
        let q = ground(step(&t, Vec3::new(0.0, 0.0, -14.0), r, LEVEL));
        assert!(q.z <= -16.2 - r + 1e-3, "{q:?}");
        // Down the stairs to the square.
        let mut pos = Vec3::new(0.0, 0.0, -17.0);
        let mut low = 0.0f32;
        for _ in 0..200 {
            pos = ground(step(&t, pos - Vec3::Z * 0.1, r, LEVEL));
            low = low.min(pos.y);
        }
        assert!(low < -1.0, "going down ({low})");
        // Enemies don't go into the corridors.
        let q = ground(step(&t, Vec3::new(0.0, 0.0, -17.0), r, Mover::Enemy));
        assert!(q.z <= -18.6 + 1e-3, "{q:?}");
        // The arenas are out of sight of each other and of the level.
        for (i, a) in t.arenas.iter().enumerate() {
            let (c, rad) = arena_bounds(a);
            assert!(math::flat_len(c) - rad > 150.0, "arena {i}");
            for (j, b) in t.arenas.iter().enumerate().skip(i + 1) {
                let (d, rb) = arena_bounds(b);
                assert!(math::flat_len(c - d) - rad - rb > 90.0, "arenas {i} and {j}");
            }
        }
    }

    #[test]
    fn the_castle_moat_is_a_fall_and_its_drawbridge_has_rails() {
        let t = Tuning::builtin();
        let (i, a) = t.arenas.iter().enumerate().find(|(_, a)| a.floors.iter().any(|f| !f.walled)).expect("an arena with a moat");
        let me = Mover::Player { zone: Zone::Arena(i as u8), door_open: false };
        let court = a.floors.iter().find(|f| !f.walled).unwrap();
        let Shape::Ellipse { center: [cx, cz], radii: [rx, _], .. } = court.shape else { unreachable!() };
        // Off the side of the courtyard: the moat, the void.
        let off = Vec3::new(cx + rx + 0.5, 0.0, cz);
        assert_eq!(step(&t, off, 0.4, me), Step::Fall);
        // The bosses stay on the courtyard.
        let p = ground(step(&t, off, 2.0, Mover::Boss(i as u8)));
        assert!(p.x <= cx + rx - 2.0 + 1e-3, "{p:?}");
        // The drawbridge: its rails on either side, and at its end, the door (no fall).
        let (d, dir) = portal(&t, &a.door);
        let side = Vec3::new(-dir.z, 0.0, dir.x);
        for s in [-2.0, 2.0] {
            let p = ground(step(&t, d + dir + side * s, 0.4, me));
            assert!((p - d).dot(side).abs() <= 1.6 - 0.4 + 1e-3, "{p:?}");
        }
        let p = ground(step(&t, d - dir * 0.5, 0.4, me));
        assert!((p - d).dot(dir) >= -1e-3, "{p:?}");
    }

    #[test]
    fn galleries_are_walls_from_below_and_you_jump_down_from_them() {
        let t = Tuning::builtin();
        let (i, a) = t.arenas.iter().enumerate().find(|(_, a)| a.floors.iter().any(|f| f.ledge)).expect("an arena with a gallery");
        let me = Mover::Player { zone: Zone::Arena(i as u8), door_open: false };
        let gallery = a.floors.iter().find(|f| f.ledge && matches!(f.shape, Shape::Ellipse { .. })).unwrap();
        let Shape::Ellipse { center: [cx, cz], y: top, .. } = gallery.shape else { unreachable!() };
        // From the yard, walking into the gallery's base: pushed back out of it, on the ground.
        let p = ground(step(&t, Vec3::new(cx, 0.0, cz), 0.4, me));
        assert!(p.y.abs() < 1e-3 && math::flat_len(p - Vec3::new(cx, 0.0, cz)) > 1.0, "{p:?}");
        // The bosses neither.
        let p = ground(step(&t, Vec3::new(cx, 0.0, cz), 0.4, Mover::Boss(i as u8)));
        assert!(p.y.abs() < 1e-3 && math::flat_len(p - Vec3::new(cx, 0.0, cz)) > 1.0, "{p:?}");
        // Up the stairs to it.
        let stairs = a.floors.iter().find(|f| f.ledge && matches!(f.shape, Shape::Strip { .. })).unwrap();
        let Shape::Strip { from: [x0, z0, _], to: [x1, z1, _], .. } = stairs.shape else { unreachable!() };
        let dir = Vec3::new(x1 - x0, 0.0, z1 - z0).normalize();
        let mut pos = Vec3::new(x0, 0.0, z0) - dir * 0.5;
        let len = Vec3::new(x1 - x0, 0.0, z1 - z0).length();
        for _ in 0..((len + 1.0) / 0.1) as usize {
            pos = ground(step(&t, pos + dir * 0.1, 0.4, me));
        }
        assert!((pos.y - top).abs() < 0.05, "on the gallery ({pos:?})");
        // Walking off it towards the yard: a jump down, not a fall into the void.
        let mut q = Vec3::new(cx, top, cz);
        let to_yard = (Vec3::new(a.boss_spawn[0], 0.0, a.boss_spawn[1]) - q).with_y(0.0).normalize();
        let s = loop {
            q += to_yard * 0.1;
            match step(&t, q, 0.4, me) {
                Step::Ground(g) => q = g,
                s => break s,
            }
        };
        assert!(matches!(s, Step::Drop(_)), "{s:?}");
    }

    #[test]
    fn walls_block_the_camera_but_not_the_open_end_of_the_stairs() {
        let t = Tuning::builtin();
        // In the theatre, towards the outside: the wall.
        let (c, _) = arena_bounds(&t.arenas[0]);
        let s = wall_hit(&t, c + Vec3::new(10.0, 0.0, 0.0), c + Vec3::new(20.0, 0.0, 0.0)).expect("wall");
        assert!((s - 0.6).abs() < 1e-3);
        // Along the stairs: nothing.
        assert_eq!(wall_hit(&t, Vec3::new(0.0, -0.5, -19.0), Vec3::new(0.0, -2.0, -22.0)), None);
        // At the foot of the stairs, camera behind you on the square: nothing either.
        assert_eq!(wall_hit(&t, Vec3::new(0.0, -2.0, -23.0), Vec3::new(0.5, -1.0, -28.5)), None);
        // On the stairs, camera from the side: the stair wall.
        assert!(wall_hit(&t, Vec3::new(0.0, -1.0, -20.0), Vec3::new(5.0, 0.0, -21.0)).is_some());
        // On the last terrace, camera beyond its balustrade: over it, nothing; below it, the balustrade.
        let [sx, sz] = terrace(&t);
        let y = floor_at(&t, sx, sz, 0.0).unwrap();
        let (eye, out) = (Vec3::new(sx, y + 1.6, sz), Vec3::new(sx + 8.0, y + 2.5, sz));
        assert_eq!(wall_hit(&t, eye, out), None);
        assert!(wall_hit(&t, eye, Vec3::new(sx + 8.0, y - 0.5, sz)).is_some());
    }

    #[test]
    fn open_edges_make_the_player_fall_but_not_enemies() {
        let t = Tuning::builtin();
        let cp = t.level.checkpoints[0].pos;
        let y = floor_at(&t, cp[0], cp[1], 0.0).expect("checkpoint on the ground");
        // Off the south edge of the square, away from the walled corridors.
        let far = Vec3::new(5.0, y, -40.5);
        assert_eq!(step(&t, far, 0.4, LEVEL), Step::Fall);
        assert!(matches!(step(&t, far, 0.4, Mover::Enemy), Step::Ground(_)));
        assert!(floor_at(&t, far.x, far.z, y).is_none());
        // At the foot of the stairs, along the balustrades: you bump, you never fall.
        for i in 0..=48 {
            let x = -2.4 + 0.1 * i as f32;
            let mut pos = Vec3::new(x, -2.4, -24.5);
            for _ in 0..40 {
                pos = ground(step(&t, pos + Vec3::Z * 0.1, 0.4, LEVEL));
            }
        }
        // The landing's open edge, though, always makes you fall.
        assert_eq!(step(&t, Vec3::new(3.0, -2.4, -23.5), 0.4, LEVEL), Step::Fall);
    }

    #[test]
    fn the_final_door_bars_the_way_until_it_opens() {
        let t = Tuning::builtin();
        let (door, dir) = portal(&t, &t.level.final_door);
        let walk = |open: bool| {
            let m = Mover::Player { zone: Zone::Level, door_open: open };
            let mut pos = door - dir * 2.0;
            for _ in 0..80 {
                pos = ground(step(&t, pos + dir * 0.1, 0.4, m));
            }
            (pos - door).dot(dir)
        };
        assert!(walk(false) < 0.01);
        assert!(walk(true) > 4.0);
    }

    /// Centre of the last terrace, behind the final door.
    fn terrace(t: &Tuning) -> [f32; 2] {
        t.level.floors.iter().find_map(|f| match f.shape {
            Shape::Ellipse { center, .. } if f.sealed => Some(center),
            _ => None,
        }).expect("a terrace behind the final door")
    }

    #[test]
    fn the_open_final_door_leads_to_the_terrace() {
        let t = Tuning::builtin();
        let (door, _) = portal(&t, &t.level.final_door);
        let [sx, sz] = terrace(&t);
        let goal = Vec3::new(sx, door.y, sz - 1.5);
        let m = Mover::Player { zone: Zone::Level, door_open: true };
        // Not only down the middle: anywhere across the porch.
        for dx in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let target = goal + Vec3::X * dx;
            let mut pos = door + Vec3::X * dx;
            for _ in 0..200 {
                let to = target - pos;
                if math::flat_len(to) < 0.1 {
                    break;
                }
                pos = ground(step(&t, pos + to.normalize_or_zero() * 0.1, 0.4, m));
            }
            assert!(math::flat_len(target - pos) < 1.0, "stuck at {pos:?}, aiming at {goal:?}");
        }
    }

    #[test]
    fn level_content_is_on_the_ground() {
        let t = Tuning::builtin();
        for c in &t.level.checkpoints {
            assert!(floor_at(&t, c.pos[0], c.pos[1], 0.0).is_some(), "checkpoint in the void: {c:?}");
        }
        for e in &t.level.enemies {
            assert!(floor_at(&t, e.pos[0], e.pos[1], 0.0).is_some(), "enemy in the void: {e:?}");
        }
        for p in &t.level.pickups {
            assert!(floor_at(&t, p.pos[0], p.pos[1], 0.0).is_some(), "item in the void: {p:?}");
        }
        for p in &t.level.props {
            assert!(floor_at(&t, p.pos[0], p.pos[1], 0.0).is_some(), "decor in the void: {p:?}");
        }
        for a in &t.arenas {
            for &[x, z, _] in &a.pillars {
                assert_eq!(zone_at(&t, Vec3::new(x, 0.0, z)), zone_at(&t, Vec3::new(a.boss_spawn[0], 0.0, a.boss_spawn[1])), "pillar out of its arena");
            }
        }
    }
}
