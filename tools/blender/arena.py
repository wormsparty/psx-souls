"""Generates assets/models/arena.glb: the whole scenery.

- ruins hanging above the darkness: the stairs, the Lamplighters' square, the bridges,
  platforms and walkways of the path, their decor, and far off in the void a few street lamps
  lost on floating rocks;
- at the end of a corridor per boss, a gateway (its fog and portcullis are made by the game)
  and a torch in front of it; at the end of the path, the final door and the painted panel beyond it;
- far away, out of sight, the bosses' arenas, each in its own style: first the circular
  courtyard of a ruined fairground theatre (the Automaton's), set on a base of rock that sinks
  into the void. Their objects are named `arena_<i>_*`: the game only draws the arena the player
  is in (and the level only when they're in it).

Everything is read from tools/blender/timings.json (exported from assets/config/arenas.ron and
level.ron by `cargo run --bin export_timings`): same positions as the collisions.
The "light_*" empties tell the game where to put the lights ("light_checkpoint_<i>":
lanterns, "light_lamp_<i>": street lamps, "light_ash_<i>": the grey lava, "light_tint_<arena>_*":
fires in the arena boss's colour, the others: braziers).

Frame: the game's (x, y, z) maps to (x, -z, y) in Blender.
"""

import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: E402,F403

reset_scene()
DATA = load_timings()
ARENAS, LEVEL, COLORS = DATA["arenas"], DATA["level"], DATA["colors"]
THEATRE = ARENAS[0]
# The theatre is drawn around the origin, then moved to its place (TX, TZ), far away.
(TX, TZ), (RADIUS, _) = THEATRE["floors"][0]["shape"]["Ellipse"]["center"], THEATRE["floors"][0]["shape"]["Ellipse"]["radii"]
PILLARS = [(x - TX, z - TZ, r) for x, z, r in THEATRE["pillars"]]
rng = random.Random(1234)

FLOOR = material("a_floor", tex=tex_stone(seed=41))
BRICK = material("a_brick", tex=tex_brick(seed=42))
STONE = material("a_stone", tex=tex_stone(seed=43, base=(0.5, 0.48, 0.44)))
DARKSTONE = material("a_darkstone", tex=tex_noise((0.25, 0.24, 0.24), 0.4, seed=44))
ROCK = material("a_rock", tex=tex_noise((0.2, 0.18, 0.17), 0.5, seed=47, cells=(16, 8, 4)))
WOOD = material("a_wood", tex=tex_planks((0.4, 0.28, 0.17), seed=45))
PLANKS = material("a_planks", tex=tex_planks((0.33, 0.24, 0.16), seed=48))
# The theatre's awnings: teal stripes, the Automaton's livery (assets/config/boss.ron).
CANOPY = material("a_canopy", tex=tex_stripes((0.06, 0.42, 0.38), (0.72, 0.8, 0.74), n=8, seed=46))
PEWTER = material("a_pewter", (0.6, 0.64, 0.63))  # its trims
PUPPET = material("a_puppet", tex=tex_stripes((0.55, 0.12, 0.1), (0.8, 0.72, 0.55), n=8, seed=46))
BOOTH = material("a_booth", tex=tex_stripes((0.2, 0.3, 0.45), (0.78, 0.72, 0.58), n=6, seed=49))
GOLD = material("a_gold", (0.7, 0.52, 0.22))
RED = material("a_red", (0.55, 0.1, 0.08))
CREAM = material("a_cream", (0.78, 0.72, 0.58))
FIRE = material("a_fire", (1.0, 0.6, 0.2), emissive=(1.0, 0.55, 0.15))
GLASS = material("a_lampglass", (1.0, 0.92, 0.65), emissive=(0.95, 0.85, 0.55))
IRON = material("a_iron", (0.2, 0.19, 0.2))
WATER = material("a_water", (0.16, 0.26, 0.32), emissive=(0.02, 0.04, 0.05))

sc = bpy.context.scene


def B(x, z, y=0.0):
    """Game point (x, z) at height y → Blender coordinates."""
    return (x, -z, y)


def obj(name, build, loc=(0, 0, 0), yaw=0.0):
    mb = MeshBuilder()
    build(mb)
    o = bpy.data.objects.new(name, mb.finish(name + "_mesh"))
    o.location = loc
    o.rotation_euler = (0, 0, yaw)
    sc.collection.objects.link(o)
    return o


LIGHTS = []


def light(name, p):
    LIGHTS.append((name, p))


# ----------------------------------------------------------------------------- floors (same logic as src/sim/world.rs)

def strip_geom(f):
    s = f["shape"]["Strip"]
    (x0, z0, y0), (x1, z1, y1) = s["from"], s["to"]
    return x0, z0, y0, x1, z1, y1, s["half_width"]


def floor_at(x, z, near=0.0):
    best = None
    pieces = LEVEL["floors"] + [f for a in ARENAS for f in a["floors"]]
    for f in pieces:
        sh = f["shape"]
        if "Ellipse" in sh:
            e = sh["Ellipse"]
            (cx, cz), (rx, rz) = e["center"], e["radii"]
            if ((x - cx) / rx) ** 2 + ((z - cz) / rz) ** 2 <= 1.0:
                y = e["y"]
            else:
                continue
        else:
            x0, z0, y0, x1, z1, y1, hw = strip_geom(f)
            dx, dz = x1 - x0, z1 - z0
            ln = math.hypot(dx, dz)
            ux, uz = dx / ln, dz / ln
            rx, rz = x - x0, z - z0
            a = rx * ux + rz * uz
            c = rx * uz - rz * ux
            if not (0 <= a <= ln and abs(c) <= hw):
                continue
            y = y0 + (y1 - y0) * a / ln
        if best is None or abs(y - near) < abs(best - near):
            best = y
    return best


def ellipses():
    for f in LEVEL["floors"]:
        if "Ellipse" in f["shape"]:
            e = f["shape"]["Ellipse"]
            yield e["center"], e["radii"], e["y"]


def inside_ellipse(x, z, margin=0.0):
    for (cx, cz), (rx, rz), y in ellipses():
        if ((x - cx) / (rx - margin)) ** 2 + ((z - cz) / (rz - margin)) ** 2 <= 1.0:
            return y
    return None


# ----------------------------------------------------------------------------- arena

def ring_quads(mb, r0, r1, z, mat, sides, uv=(1, 1), cx=0.0, cy=0.0, ry=None):
    k = 1.0 if ry is None else ry
    for i in range(sides):
        a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
        v = [mb.bm.verts.new((cx + r * math.cos(a), cy + r * k * math.sin(a), z)) for r, a in ((r0, a0), (r1, a0), (r1, a1), (r0, a1))]
        mb._face(v, mat, uv_scale=uv)


def arena_floor(mb):
    # Floor in concentric rings (flagstones), slightly subdivided to limit the affine distortion.
    rings = [0, 1.5, 3, 4.5, 6, 7.5, 9, 10.5, 12, 13.5, 15, RADIUS + 1.5]
    for r0, r1 in zip(rings, rings[1:]):
        sides = 48  # same split everywhere: no crack between rings
        if r0 == 0:
            for i in range(sides):
                a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
                v = [mb.bm.verts.new((0, 0, 0)), mb.bm.verts.new((r1 * math.cos(a0), r1 * math.sin(a0), 0)),
                     mb.bm.verts.new((r1 * math.cos(a1), r1 * math.sin(a1), 0))]
                mb._face(v, FLOOR, uvs=[(0.5, 0), (0, 1), (1, 1)])
        else:
            ring_quads(mb, r0, r1, 0, FLOOR, sides)
    # Central rosette.
    ring_quads(mb, 2.2, 2.6, 0.04, DARKSTONE, 24)


GATE_SEGMENT = 7  # wall segment centred on +y (opening towards the stairs)


def wall(mb):
    sides = 32
    r_in, r_out, h = RADIUS + 0.5, RADIUS + 1.3, 5.0
    for i in range(sides):
        # Offset by half a segment so that one segment is centred on the stairs' axis.
        a0, a1 = 2 * math.pi * (i + 0.5) / sides, 2 * math.pi * (i + 1.5) / sides
        if i == GATE_SEGMENT:
            continue
        c0, s0, c1, s1 = math.cos(a0), math.sin(a0), math.cos(a1), math.sin(a1)
        hh = h - (1.6 if i % 5 == 2 else 0) - (0.7 if i % 3 == 0 else 0)  # ruined battlements
        vi = [mb.bm.verts.new(p) for p in ((r_in * c1, r_in * s1, 0), (r_in * c0, r_in * s0, 0),
                                           (r_in * c0, r_in * s0, hh), (r_in * c1, r_in * s1, hh))]
        mb._face(vi, BRICK, uv_scale=(1, 2))
        # The outer facing goes down below the floor (it's visible from the square).
        vo = [mb.bm.verts.new(p) for p in ((r_out * c0, r_out * s0, -1.5), (r_out * c1, r_out * s1, -1.5),
                                           (r_out * c1, r_out * s1, hh), (r_out * c0, r_out * s0, hh))]
        mb._face(vo, BRICK, uv_scale=(1, 2.6))
        vt = [mb.bm.verts.new(p) for p in ((r_in * c0, r_in * s0, hh), (r_out * c0, r_out * s0, hh),
                                           (r_out * c1, r_out * s1, hh), (r_in * c1, r_in * s1, hh))]
        mb._face(vt, STONE)
        # Buttresses (not next to the opening).
        if i % 4 == 0 and i not in (GATE_SEGMENT, GATE_SEGMENT + 1):
            mb.box((r_in * c0 - 0.3 * c0, r_in * s0 - 0.3 * s0, h / 2 - 0.4), (0.7, 0.7, h - 0.8), STONE)


def rock_cone(mb, cx, cy, top, rx, ry, depth, seed, sides=14, rings=4):
    """Irregular rock base under a platform: it narrows and fades into the void."""
    r = random.Random(seed)
    jag = [[0.82 + 0.3 * r.random() for _ in range(sides)] for _ in range(rings + 1)]
    levels = []
    for k in range(rings + 1):
        t = k / rings
        z = top - depth * t ** 0.8
        s = (1.0 - t) ** 1.3 * 0.95 + 0.04
        ring = []
        for i in range(sides):
            a = 2 * math.pi * (i + 0.5 * (k % 2)) / sides
            j = 1.0 if k == 0 else jag[k][i]
            ring.append(mb.bm.verts.new((cx + rx * s * j * math.cos(a), cy + ry * s * j * math.sin(a), z)))
        levels.append(ring)
    for k in range(rings):
        a, b = levels[k], levels[k + 1]
        for i in range(sides):
            j = (i + 1) % sides
            mb._face((a[j], a[i], b[i], b[j]), ROCK, uv_scale=(1.5, 2))
    mb._face(list(reversed(levels[-1])), ROCK)


def arena_base(mb):
    rock_cone(mb, 0, 0, -1.5, RADIUS + 1.3, RADIUS + 1.3, 26, seed=7, sides=24, rings=5)


def pillars(mb):
    for x, z, r in PILLARS:
        x, y, _ = B(x, z)
        mb.cylinder((x, y, 0.25), r * 1.25, 0.5, STONE, sides=8)
        mb.cylinder((x, y, 3.6), r, 6.2, STONE, sides=8, uv_scale=(2, 4))
        mb.cylinder((x, y, 6.85), r * 1.3, 0.3, STONE, sides=8)


def braziers(mb):
    for i, (x, z, r) in enumerate(PILLARS[:4]):
        d = math.hypot(x, z)
        bx, by, _ = B(x - x / d * (r + 0.9), z - z / d * (r + 0.9))
        mb.cylinder((bx, by, 0.5), 0.08, 1.0, IRON, sides=6)
        mb.cylinder((bx, by, 1.1), 0.35, 0.25, IRON, sides=8, radius_top=0.45)
        mb.box((bx, by, 1.32), (0.35, 0.35, 0.25), FIRE, taper=(0.4, 0.4))
        ox, oy, _ = B(TX, TZ)
        light(f"light_a0_{i}", (ox + bx, oy + by, 1.6))


def carousel(mb):
    # Ruined carousel beyond the wall, behind the boss (to the north), on its own rock.
    cx, cy = 0.0, -(RADIUS + 9.0)
    rock_cone(mb, cx, cy, 0.0, 7.0, 7.0, 16, seed=8)
    mb.cylinder((cx, cy, 0.3), 6.0, 0.6, WOOD, sides=12)
    mb.cylinder((cx, cy, 4.5), 0.4, 9.0, PEWTER, sides=8)
    for i in range(12):
        a = 2 * math.pi * i / 12
        if i in (3, 4):
            continue  # broken posts
        mb.cylinder((cx + 5.2 * math.cos(a), cy + 5.2 * math.sin(a), 3.2), 0.1, 5.6, PEWTER, sides=6)
    mb.cylinder((cx, cy, 6.3), 6.4, 0.5, PEWTER, sides=12)
    mb.cylinder((cx, cy, 8.0), 6.6, 3.0, CANOPY, sides=12, radius_top=0.3, uv_scale=(3, 1))
    mb.box((cx + 2.5, cy + 1.0, 1.1), (0.5, 1.4, 1.0), WOOD)  # toppled wooden horse
    mb.box((cx - 3.0, cy - 0.5, 1.3), (0.4, 1.2, 0.9), WOOD)


def skyline(mb):
    """Building silhouettes, to the north only (to the south there is only the void), each on
    a rock spire."""
    for i in range(20):
        a = 2 * math.pi * (i + 0.3) / 20
        d = RADIUS + 16 + (i * 7 % 5) * 2.5
        x, y = d * math.cos(a), d * math.sin(a)
        if y > -6 or (abs(x) < 9 and y < 0):
            continue  # not to the south, and leaves the carousel visible
        h = 8 + (i * 13 % 7) * 2.0
        rock_cone(mb, x, y, -0.2, 4.2, 3.6, 18 + (i % 3) * 6, seed=100 + i, sides=8, rings=3)
        mb.box((x, y, h / 2), (6, 5, h), DARKSTONE, taper=(0.9, 0.9), uv_scale=(2, 3))
        mb.box((x, y, h + 1.2), (6.2, 5.2, 2.4), DARKSTONE, taper=(0.1, 0.9))


# ----------------------------------------------------------------------------- paths and platforms

def platform(mb, f, seed):
    e = f["shape"]["Ellipse"]
    (cx, cz), (rx, rz), y = e["center"], e["radii"], e["y"]
    bx, by, _ = B(cx, cz)
    k = rz / rx
    # Top in rings (no face too large: the affine texture would get distorted). The walled
    # terrace is small: fewer sides (its balustrade follows them).
    sides = 16 if f.get("walled") else 40
    step = 1.6
    rings = [0.0]
    while rings[-1] + step < rx - 0.3:
        rings.append(rings[-1] + step)
    rings.append(rx)
    for r0, r1 in zip(rings, rings[1:]):
        if r0 == 0:
            for i in range(sides):
                a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
                v = [mb.bm.verts.new((bx, by, y)),
                     mb.bm.verts.new((bx + r1 * math.cos(a0), by + r1 * k * math.sin(a0), y)),
                     mb.bm.verts.new((bx + r1 * math.cos(a1), by + r1 * k * math.sin(a1), y))]
                mb._face(v, FLOOR, uvs=[(0.5, 0), (0, 1), (1, 1)])
        else:
            ring_quads(mb, r0, r1, y, FLOOR, sides, cx=bx, cy=by, ry=k)
    # Edge of the paving, then the rock underneath.
    for i in range(sides):
        a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
        p = [(bx + rx * math.cos(a), by + rx * k * math.sin(a)) for a in (a0, a1)]
        v = [mb.bm.verts.new((p[1][0], p[1][1], y)), mb.bm.verts.new((p[0][0], p[0][1], y)),
             mb.bm.verts.new((p[0][0], p[0][1], y - 0.6)), mb.bm.verts.new((p[1][0], p[1][1], y - 0.6))]
        mb._face(v, STONE, uv_scale=(0.5, 0.3))
    rock_cone(mb, bx, by, y - 0.6, rx, rx * k, 6 + rx * 1.6, seed=seed, sides=16)
    if f.get("walled"):
        # Walled platform (the last terrace): a balustrade all around, open where a path arrives.
        for i in range(sides):
            a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
            am = (a0 + a1) / 2
            gx, gz = bx + (rx + 0.2) * math.cos(am), -(by + (rx + 0.2) * k * math.sin(am))
            if any(near_strip_end(g, gx, gz) for g in LEVEL["floors"] if "Strip" in g["shape"]):
                continue
            p0 = (bx + (rx + 0.2) * math.cos(a0), by + (rx + 0.2) * k * math.sin(a0))
            p1 = (bx + (rx + 0.2) * math.cos(a1), by + (rx + 0.2) * k * math.sin(a1))
            mb.seg((p0[0], p0[1], y + 0.45), (p1[0], p1[1], y + 0.45), 0.4, 1.0, STONE)


def ellipse_at(x, z):
    """The level platform (centre, radii, y) that contains (x, z), or None."""
    for (cx, cz), (rx, rz), y in ellipses():
        if ((x - cx) / rx) ** 2 + ((z - cz) / rz) ** 2 <= 1.0:
            return (cx, cz), (rx, rz), y
    return None


# Length of the fillets' legs, along the path's side and along the platform's edge (m).
FILLET = 1.0


def fillets(mb, f, a, b, deck, mat_side, out=0.0):
    """Where a straight path meets a round platform, a wedge of void stays between the path's
    side and the edge curving away (all the deeper as the path arrives at an angle): a curved
    fillet of paving fills it, at the platform's height. Visual only: it stays within ~0.3 m of
    the corner, less than the player's radius (they only fall once their centre is over the void).
    `out`: from that far outside the path (walled path: from the outer face of its balustrades)."""
    x0, z0, y0, x1, z1, y1, hw = strip_geom(f)
    dx, dz = x1 - x0, z1 - z0
    ln = math.hypot(dx, dz)
    ux, uz = dx / ln, dz / ln
    sx, sz = uz, -ux

    def at(t, side):
        return x0 + ux * t + sx * side, z0 + uz * t + sz * side

    def height(t):
        return y0 + (y1 - y0) * t / ln

    # Ends in a platform: towards the platform, t decreases at `from`, increases at `to`.
    for t_end, sgn in ((a, -1), (b, 1)):
        e = ellipse_at(*at(0.0 if sgn < 0 else ln, 0.0))
        if e is None:
            continue
        (cx, cz), (rx, rz), yp = e

        def val(p):
            return ((p[0] - cx) / rx) ** 2 + ((p[1] - cz) / rz) ** 2

        for s in (-hw - out, hw + out):
            # Where the path's side leaves the platform, going away from it.
            t, inside = t_end + sgn * 3.0, None
            while (t - t_end) * sgn > -3.0:
                if val(at(t, s)) <= 1.0:
                    inside = t
                elif inside is not None:
                    break
                t -= sgn * 0.01
            if inside is None or val(at(inside - sgn * FILLET, s)) <= 1.0:
                continue
            ex, ez = at(inside, s)
            p1 = at(inside - sgn * FILLET, s)
            # Along the edge, away from the path, over the same length.
            th = math.atan2((ez - cz) / rz, (ex - cx) / rx)
            away = 1 if s > 0 else -1
            step = 0.01
            probe = (cx + rx * math.cos(th + step), cz + rz * math.sin(th + step))
            if ((probe[0] - ex) * sx + (probe[1] - ez) * sz) * away < 0:
                step = -step
            q, walked = (ex, ez), 0.0
            while walked < FILLET:
                th += step
                nq = (cx + rx * math.cos(th), cz + rz * math.sin(th))
                walked += math.hypot(nq[0] - q[0], nq[1] - q[1])
                q = nq
            # Quadratic curve from the path's side to the edge, its control point in the corner.
            n = 6
            curve = []
            for k in range(n + 1):
                u = k / n
                w0, w1, w2 = (1 - u) ** 2, 2 * u * (1 - u), u * u
                gx = w0 * p1[0] + w1 * ex + w2 * q[0]
                gz = w0 * p1[1] + w1 * ez + w2 * q[1]
                gy = (1 - u) * (height(inside - sgn * FILLET) - 0.02) + u * (yp - 0.01)
                curve.append((gx, gz, gy))
            corner = B(ex, ez, yp - 0.02)
            top = [B(*c) for c in curve]
            bot = [B(c[0], c[1], c[2] - deck) for c in curve]
            corner_bot = (corner[0], corner[1], corner[2] - deck)
            for k in range(n):
                tri = [corner, top[k], top[k + 1]]
                vs = [mb.bm.verts.new(p) for p in tri]
                if not is_ccw(tri):
                    vs.reverse()
                mb._face(vs, FLOOR, uvs=[(v.co.x / 2, v.co.y / 2) for v in vs])
                tri = [corner_bot, bot[k], bot[k + 1]]
                vs = [mb.bm.verts.new(p) for p in tri]
                if is_ccw(tri):
                    vs.reverse()
                mb._face(vs, mat_side)
                # Skirt along the curve, facing away from the corner.
                quad = [top[k], top[k + 1], bot[k + 1], bot[k]]
                mx, my = (top[k][0] + top[k + 1][0]) / 2 - corner[0], (top[k][1] + top[k + 1][1]) / 2 - corner[1]
                ex_, ey_ = top[k + 1][0] - top[k][0], top[k + 1][1] - top[k][1]
                vs = [mb.bm.verts.new(p) for p in quad]
                # Normal of (top k → top k+1, downwards) seen from above: (−ey, ex) rotated; flip if inwards.
                if (ey_ * mx - ex_ * my) > 0:
                    vs.reverse()
                mb._face(vs, mat_side, uv_scale=(1, 0.4))


def near_strip_end(f, x, z):
    x0, z0, _, x1, z1, _, hw = strip_geom(f)
    return min(math.hypot(x - x0, z - z0), math.hypot(x - x1, z - z1)) < hw + 1.2


def strip(mb, f, idx):
    x0, z0, y0, x1, z1, y1, hw = strip_geom(f)
    style, steps, walled = f.get("style", "Paved"), f.get("steps", 0), f.get("walled", False)
    dx, dz = x1 - x0, z1 - z0
    ln = math.hypot(dx, dz)
    ux, uz = dx / ln, dz / ln
    # Ends that go into a platform (or the arena floor) are trimmed: we don't
    # stack two floors (a 2 cm gap flickers from afar), only a seam remains.
    sx, sz = uz, -ux  # side (right when looking towards `to`)

    def trim(side):
        """Ends of the line `side` metres from the axis, trimmed by the platforms."""
        def inside(t):
            return inside_ellipse(x0 + ux * t + sx * side, z0 + uz * t + sz * side) is not None
        a, b = 0.0, ln
        while a < ln and inside(a + 0.3):
            a += 0.1
        while b > 0 and inside(b - 0.3):
            b -= 0.1
        return a, b
    a, b = trim(0.0)
    # Strip entirely on a platform: nothing to draw (except its balustrades).
    covered = b - a < 0.2 and not steps
    if b - a < 0.2:
        a, b = 0.0, ln
    deck = 0.12 if style == "Planks" else 0.5
    mat_top = PLANKS if style == "Planks" else FLOOR
    mat_side = WOOD if style == "Planks" else STONE

    def P(t, side, dy=0.0):
        y = y0 + (y1 - y0) * t / ln
        return B(x0 + ux * t + sx * side, z0 + uz * t + sz * side, y + dy)

    if steps:
        # Steps: the top of each step is at the height of the slope at its middle.
        n = steps
        for k in range(n):
            ta, tb = a + (b - a) * k / n, a + (b - a) * (k + 1) / n
            tm = (ta + tb) / 2
            ytop = y0 + (y1 - y0) * tm / ln
            lo = min(y0, y1) - 0.4
            corners = [B(x0 + ux * t + sx * s, z0 + uz * t + sz * s) for t, s in ((ta, -hw), (tb, -hw), (tb, hw), (ta, hw))]
            v_top = [mb.bm.verts.new((c[0], c[1], ytop)) for c in corners]
            v_bot = [mb.bm.verts.new((c[0], c[1], lo)) for c in corners]
            mb._face(v_top if is_ccw(corners) else list(reversed(v_top)), FLOOR, uv_scale=(1, 0.5))
            for i in range(4):
                j = (i + 1) % 4
                q = (v_bot[i], v_bot[j], v_top[j], v_top[i])
                mb._face(q if is_ccw(corners) else tuple(reversed(q)), STONE, uv_scale=(1, 0.3))
    elif not covered:
        # Deck slabs in strips of about 1 m (affine texture kept in check). Each column of
        # vertices is trimmed on its own: a path arriving at an angle goes into the platform on
        # both sides (otherwise a corner of void remains between its end and the edge).
        n = max(1, math.ceil((b - a) / 1.0))
        cols = [-hw + 2 * hw * i / 3 for i in range(4)]
        ends = [trim(c) for c in cols]
        if any(eb - ea < 0.2 for ea, eb in ends):
            ends = [(a, b)] * 4
        rows = [[mb.bm.verts.new(P(ea + (eb - ea) * j / n, c, -0.02)) for c, (ea, eb) in zip(cols, ends)] for j in range(n + 1)]
        for j in range(n):
            for i in range(3):
                q = [rows[j][i], rows[j + 1][i], rows[j + 1][i + 1], rows[j][i + 1]]
                if not is_ccw([v.co for v in q]):
                    q.reverse()
                mb._face(q, mat_top, uvs=[(v.co.x / 2, v.co.y / 2) for v in q])
        # Deck edges.
        for side, (ea, eb) in ((-hw, ends[0]), (hw, ends[3])):
            for j in range(n):
                ta, tb = ea + (eb - ea) * j / n, ea + (eb - ea) * (j + 1) / n
                q = [mb.bm.verts.new(P(ta, side, -0.02)), mb.bm.verts.new(P(tb, side, -0.02)),
                     mb.bm.verts.new(P(tb, side, -deck)), mb.bm.verts.new(P(ta, side, -deck))]
                if side < 0:
                    q.reverse()
                mb._face(q, mat_side, uv_scale=(1, 0.4))
        # Underside.
        q = [mb.bm.verts.new(P(ends[0][0], -hw, -deck)), mb.bm.verts.new(P(ends[3][0], hw, -deck)),
             mb.bm.verts.new(P(ends[3][1], hw, -deck)), mb.bm.verts.new(P(ends[0][1], -hw, -deck))]
        if is_ccw([v.co for v in q]):
            q.reverse()
        mb._face(q, mat_side)
        if style != "Planks":
            fillets(mb, f, a, b, deck, mat_side, out=0.4 if walled else 0.0)
        if style == "Planks":
            # Badly joined cross planks, and a few beams hanging underneath.
            for k in range(int((b - a) / 0.9)):
                t = a + 0.45 + k * 0.9
                mb.seg(P(t, -hw - 0.1, -0.05), P(t, hw + 0.1, -0.05), 0.18, 0.06, WOOD)
            for t in (a + 0.6, b - 0.6):
                for side in (-hw, hw):
                    mb.seg(P(t, side, -0.1), P(t, side * 1.4, -3.0), 0.12, 0.12, WOOD)
        else:
            # Piers plunging into the void.
            k = max(1, int((b - a) / 6.0))
            for i in range(k):
                t = a + (b - a) * (i + 0.5) / k
                c = P(t, 0, -deck)
                mb.box((c[0], c[1], c[2] - 7.0), (2 * hw * 0.7, 1.2, 14.0), ROCK if style != "Bridge" else STONE,
                       taper=(0.5, 0.6))
                # Corbelling under the deck.
                mb.box((c[0], c[1], c[2] - 0.5), (2 * hw + 0.2, 1.6, 1.0), STONE, taper=(1.15, 1.4))
    if walled:
        # Balustrades on either side (arena stairs).
        for side in (-hw - 0.2, hw + 0.2):
            n = max(1, steps or int(ln))
            # Each one trimmed by the platforms along its own line (path arriving at an angle).
            ea, eb = (a, b) if steps or covered else trim(side)
            if eb - ea < 0.2:
                ea, eb = a, b
            for k in range(n):
                ta, tb = ea + (eb - ea) * k / n, ea + (eb - ea) * (k + 1) / n
                c0, c1 = P(ta, side), P(tb, side)
                bot, top = min(c0[2], c1[2]) - 0.1, max(c0[2], c1[2]) + 0.9
                rail(mb, c0, c1, 0.4, bot, top, STONE)
        # Underside of the stairs: piers under the steps (their top stays under the lowest
        # one, otherwise it would poke through the bottom of the stairs).
        lo = min(y0, y1) - 0.45
        for t in (a + (b - a) * 0.3, a + (b - a) * 0.8):
            c = P(t, 0)
            mb.box((c[0], c[1], lo - 4.0), (2 * hw + 0.6, 1.4, 8.0), ROCK, taper=(0.6, 0.8))


def rail(mb, p0, p1, w, bot, top, mat):
    """Wall `w` thick along the segment p0 → p1 (Blender x, y), from `bot` to `top`: a slab
    turned in that direction, in the same mesh as its floor (no object per piece)."""
    import mathutils
    d = mathutils.Vector((p1[0] - p0[0], p1[1] - p0[1], 0.0))
    d = d.normalized() * (d.length + 0.02)
    a = mathutils.Vector((p0[0], p0[1], bot)) - d.normalized() * 0.01
    s = mathutils.Vector((-d.y, d.x, 0.0)).normalized() * (w / 2)
    h = mathutils.Vector((0.0, 0.0, top - bot))
    for o, du, dv in ((a - s, d, h), (a + s + d, -d, h), (a - s + h, d, 2 * s), (a + s, -2 * s, h), (a - s + d, 2 * s, h)):
        mb.panel(o, du, dv, mat)


def is_ccw(pts):
    """True if the polygon (seen from above, Z axis) winds counter-clockwise."""
    s = 0.0
    for i in range(len(pts)):
        x0, y0 = pts[i][0], pts[i][1]
        x1, y1 = pts[(i + 1) % len(pts)][0], pts[(i + 1) % len(pts)][1]
        s += x0 * y1 - x1 * y0
    return s > 0


# ----------------------------------------------------------------------------- decor
# Local frame of a decor piece: front = -Y, right = -X (like the characters), origin on the ground.

def lamp(mb):
    mb.box((0, 0, 0.15), (0.32, 0.32, 0.3), STONE, taper=(0.8, 0.8))
    mb.cylinder((0, 0, 1.8), 0.06, 3.0, IRON, sides=6)
    mb.cylinder((0, 0, 3.32), 0.1, 0.06, IRON, sides=6)
    mb.box((0, 0, 3.52), (0.26, 0.26, 0.34), GLASS, taper=(1.15, 1.15))
    for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
        mb.box((sx * 0.14, sy * 0.14, 3.52), (0.03, 0.03, 0.38), IRON)
    mb.box((0, 0, 3.76), (0.4, 0.4, 0.14), IRON, taper=(0.2, 0.2))


def dead_lamp(mb):
    mb.box((0, 0, 0.15), (0.32, 0.32, 0.3), STONE, taper=(0.8, 0.8))
    mb.seg((0, 0, 0.3), (0.0, -0.5, 2.2), 0.1, 0.1, IRON)
    mb.seg((0.0, -0.5, 2.2), (0.3, -1.1, 2.6), 0.09, 0.09, IRON)
    mb.box((0.35, -1.2, 2.45), (0.24, 0.24, 0.3), IRON, taper=(1.1, 1.1))


def tube_inside(mb, r, z0, z1, mat, sides):
    """Inner wall of a cylinder (faces turned towards the axis)."""
    for i in range(sides):
        a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
        p = lambda a, z: mb.bm.verts.new((r * math.cos(a), r * math.sin(a), z))
        mb._face([p(a1, z0), p(a0, z0), p(a0, z1), p(a1, z1)], mat, uv_scale=(1, 0.3))


def disc(mb, r, z, mat, sides):
    """Horizontal disc facing up, without an edge (water surface: it goes slightly into the
    wall around it, so no rim doubles it)."""
    v = [mb.bm.verts.new((r * math.cos(2 * math.pi * i / sides), r * math.sin(2 * math.pi * i / sides), z)) for i in range(sides)]
    mb._face(v, mat)


# Fountain heights (the game makes water spurt there: see FOUNTAIN_* in src/fx.rs).
FOUNTAIN_WATER = 0.42
FOUNTAIN_BOWL = 1.93
FOUNTAIN_SPOUT = 2.2


def fountain(mb):
    # Hollow basin: outer wall, rim, inner wall, and the water below the edge (two
    # surfaces at the same height flickered).
    sides = 16
    mb.cylinder((0, 0, 0.28), 1.7, 0.56, STONE, sides=sides, caps=False)
    ring_quads(mb, 1.45, 1.7, 0.56, STONE, sides)
    tube_inside(mb, 1.45, 0.2, 0.56, STONE, sides)
    disc(mb, 1.5, FOUNTAIN_WATER, WATER, sides)
    # Column and upper bowl, hollow too: the water is well below the rim and the top of
    # the column well below the water (faces 1-2 cm apart overlapped once
    # the vertices were snapped to the screen grid, PS1-style). A spout in the middle.
    mb.cylinder((0, 0, 1.1), 0.28, 1.3, STONE, sides=8, radius_top=0.22)
    mb.cylinder((0, 0, FOUNTAIN_BOWL - 0.13), 0.38, 0.26, STONE, sides=12, radius_top=0.8, caps=False)
    mb.cylinder((0, 0, FOUNTAIN_BOWL - 0.27), 0.38, 0.02, STONE, sides=12)  # underside
    ring_quads(mb, 0.64, 0.8, FOUNTAIN_BOWL, STONE, 12)
    tube_inside(mb, 0.64, FOUNTAIN_BOWL - 0.2, FOUNTAIN_BOWL, STONE, 12)
    disc(mb, 0.68, FOUNTAIN_BOWL - 0.09, WATER, 12)
    mb.cylinder((0, 0, (FOUNTAIN_BOWL - 0.1 + FOUNTAIN_SPOUT) / 2), 0.09, FOUNTAIN_SPOUT - FOUNTAIN_BOWL + 0.1, STONE, sides=6, radius_top=0.06)
    mb.box((0.0, -0.9, FOUNTAIN_WATER + 0.06), (0.7, 0.35, 0.22), DARKSTONE)  # shard fallen into the basin


def bench(mb):
    for sx in (-0.6, 0.6):
        mb.box((sx, 0, 0.2), (0.08, 0.4, 0.4), IRON)
    mb.box((0, 0, 0.44), (1.5, 0.42, 0.06), WOOD)
    mb.box((0, 0.2, 0.78), (1.5, 0.05, 0.36), WOOD)


def horse(mb):
    # Carousel horse fallen on its side.
    mb.box((0, 0, 0.3), (0.42, 1.25, 0.5), WOOD)
    mb.box((0, -0.75, 0.5), (0.3, 0.45, 0.32), WOOD, taper=(0.9, 0.8))
    mb.box((0, -1.0, 0.42), (0.24, 0.3, 0.22), WOOD)
    for sy in (-0.4, 0.4):
        mb.seg((0.12, sy, 0.2), (0.75, sy - 0.1, 0.12), 0.1, 0.1, WOOD)
        mb.seg((-0.12, sy, 0.2), (0.6, sy + 0.15, 0.05), 0.1, 0.1, WOOD)
    mb.box((0, 0, 0.58), (0.3, 0.5, 0.08), CANOPY)
    mb.seg((0.0, 0.3, 0.3), (-1.4, 0.9, 0.08), 0.07, 0.07, GOLD)


def booth(mb):
    mb.box((0, 0.1, 1.0), (1.5, 1.3, 2.0), WOOD)
    mb.box((0, -0.56, 1.15), (1.1, 0.06, 0.55), DARKSTONE)  # ticket booth
    mb.box((0, -0.62, 0.88), (1.2, 0.25, 0.06), WOOD)
    mb.box((0, -0.05, 2.25), (1.8, 1.8, 0.5), BOOTH, taper=(0.15, 0.15))
    mb.box((0, -0.86, 2.1), (1.7, 0.08, 0.25), BOOTH)


def column(h):
    def build(mb):
        mb.cylinder((0, 0, 0.12), 0.38, 0.24, STONE, sides=8)
        mb.cylinder((0, 0, 0.24 + h / 2), 0.26, h, STONE, sides=8, uv_scale=(1, 2))
    return build


def crates(mb):
    mb.box((0, 0, 0.3), (0.6, 0.6, 0.6), WOOD)
    mb.box((0.55, 0.25, 0.25), (0.5, 0.5, 0.5), WOOD)
    mb.box((0.1, 0.1, 0.82), (0.45, 0.45, 0.45), WOOD)


PROPS = {"Lamp": lamp, "DeadLamp": dead_lamp, "Fountain": fountain, "Bench": bench, "Horse": horse,
         "Booth": booth, "Crates": crates}


def props():
    lamp_i = 0
    for i, p in enumerate(LEVEL["props"]):
        x, z = p["pos"]
        y = floor_at(x, z) or 0.0
        yaw = math.radians(p.get("yaw", 0.0))
        kind = p["kind"]
        build = column(1.0 + (i * 7 % 5) * 0.55) if kind == "Column" else PROPS[kind]
        obj(f"prop_{i}_{kind}", build, loc=B(x, z, y), yaw=yaw)
        if kind == "Lamp":
            light(f"light_lamp_{lamp_i}", B(x, z, y + 3.5))
            lamp_i += 1


# Height of the brazier's embers (the game makes embers and ash rise from it: see src/fx.rs).
COALS = 1.0


def checkpoint(mb):
    """Resting-point brazier: stone base, clawed iron bowl, and an old sword planted in the
    embers (in local coordinates; the embers are separate)."""
    mb.cylinder((0, 0, 0.12), 0.62, 0.24, STONE, sides=8)
    mb.cylinder((0, 0, 0.36), 0.46, 0.24, DARKSTONE, sides=8, radius_top=0.36)
    mb.cylinder((0, 0, 0.56), 0.16, 0.16, IRON, sides=6)
    mb.cylinder((0, 0, 0.79), 0.3, 0.3, IRON, sides=8, radius_top=0.62)
    for k in range(6):
        a = 2 * math.pi * (k + 0.5) / 6
        c, s = math.cos(a), math.sin(a)
        mb.seg((0.56 * c, 0.56 * s, 0.9), (0.74 * c, 0.74 * s, 1.22), 0.06, 0.06, IRON, taper=0.4)
    # Planted sword, slightly tilted: blade, guard, grip, pommel.
    hilt = (0.13, 0.05, 1.72)
    mb.seg((0.03, 0.0, 0.85), hilt, 0.11, 0.03, IRON, taper=1.3)
    mb.seg((hilt[0], hilt[1] - 0.2, hilt[2]), (hilt[0], hilt[1] + 0.2, hilt[2]), 0.06, 0.06, IRON)
    top = (hilt[0] + 0.03, hilt[1] + 0.01, hilt[2] + 0.28)
    mb.seg(hilt, top, 0.05, 0.05, WOOD)
    mb.box(top, (0.09, 0.09, 0.09), IRON)


def checkpoint_coals(mb):
    """Heap of embers in the bowl (separate object: the game darkens it until the brazier
    is rekindled)."""
    mb.cylinder((0, 0, COALS - 0.06), 0.56, 0.12, FIRE, sides=8, radius_top=0.32)
    for k in range(5):
        a = 2 * math.pi * k / 5 + 0.4
        mb.box((0.22 * math.cos(a), 0.22 * math.sin(a), COALS + 0.02), (0.14, 0.12, 0.1), FIRE, taper=(0.6, 0.6))


def checkpoints():
    for i, c in enumerate(LEVEL["checkpoints"]):
        x, z = c["pos"]
        y = floor_at(x, z) or 0.0
        obj(f"checkpoint_{i}", checkpoint, loc=B(x, z, y))
        obj(f"checkpoint_{i}_coals", checkpoint_coals, loc=B(x, z, y))
        light(f"light_checkpoint_{i}", B(x, z, y + COALS + 0.35))


def ring_marks(mb):
    """The colossus's track: a red and white circle painted on the flagstones."""
    for (cx, cz), (rx, rz), y in ellipses():
        if rx >= 6.0 and abs(rx - rz) < 1e-3:
            bx, by, _ = B(cx, cz)
            sides = 32
            for i in range(sides):
                a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
                v = [mb.bm.verts.new((bx + r * math.cos(a), by + r * math.sin(a), y + 0.025))
                     for r, a in ((rx - 1.2, a0), (rx - 0.8, a0), (rx - 0.8, a1), (rx - 1.2, a1))]
                mb._face(v, RED if i % 2 else CREAM)


def far_lamps():
    """Far off in the void: street lamps on floating rocks, the only points of light."""
    zs = [f["shape"]["Ellipse"]["center"][1] for f in LEVEL["floors"] if "Ellipse" in f["shape"]]
    cz = (min(zs) + max(zs)) / 2
    for i in range(18):
        a = 2 * math.pi * (i + rng.random() * 0.6) / 18
        d = 40 + rng.random() * 60
        x, z = 5 + d * math.cos(a), cz + d * 1.3 * math.sin(a)
        if abs(z) < 30 and abs(x) < 30:
            continue  # not in the arena
        y = -14 - rng.random() * 30
        s = 1.4 + rng.random() * 1.2  # larger than life: visible from afar

        def build(mb, s=s, seed=i):
            rock_cone(mb, 0, 0, 0, 1.6 * s, 1.3 * s, 4 * s, seed=500 + seed, sides=7, rings=2)
            mb.cylinder((0, 0, 1.5 * s), 0.07 * s, 3 * s, IRON, sides=5)
            mb.box((0, 0, 3.2 * s), (0.4 * s, 0.4 * s, 0.5 * s), GLASS)
        obj(f"far_lamp_{i}", build, loc=B(x, z, y))


# ----------------------------------------------------------------------------- the bosses' passages
# Local frame of a passage: origin on the ground at the centre of the fog, +Y: the direction in
# which you go through it, X across.

VOID = material("a_void", (0.015, 0.012, 0.018))
CLOTH = material("a_cloth", tex=tex_noise((0.3, 0.06, 0.05), 0.3, seed=60))
ICE = material("a_ice", tex=tex_noise((0.55, 0.7, 0.8), 0.25, seed=61), emissive=(0.04, 0.07, 0.09))
BURNT = material("a_burnt", tex=tex_noise((0.17, 0.14, 0.12), 0.5, seed=63))
# The foundry's floor: packed sand and soot. A large texture (8 m at the floor's scale) with
# broad, soft variations, so that its repeats don't draw a grid in the round room.
SAND = material("a_sand", tex=tex_noise((0.24, 0.22, 0.21), 0.35, seed=64, size=256, cells=(64, 16, 16, 8)), size=256)
DARKBRICK = material("a_darkbrick", tex=tex_brick(seed=65, base=(0.26, 0.22, 0.22)))
BLOOD = material("a_blood", (0.2, 0.02, 0.025))
CURTAIN = material("a_curtain", tex=tex_stripes((0.45, 0.05, 0.06), (0.3, 0.03, 0.04), n=6, seed=66))
EMBER = material("a_ember", (0.9, 0.35, 0.08), emissive=(0.9, 0.3, 0.05))
# The Giant's dead fire: cold, dark ash in the ditch, barely glowing (a light ring around the
# arena caught the eye more than the fight).
ASHLAVA = material("a_ashlava", tex=tex_noise((0.13, 0.12, 0.12), 0.5, seed=67, cells=(8, 8, 4)), emissive=(0.025, 0.025, 0.03))
# The Wyvern's castle: grey stone like its courtyard's flagstones, darker below the courtyard.
CASTLE = material("a_castle", tex=tex_brick(seed=68, base=(0.4, 0.39, 0.37)))
CASTLE_DARK = material("a_castle_dark", tex=tex_brick(seed=69, base=(0.22, 0.21, 0.21)))
BOSS_MATS = [material(f"a_boss_{i}", tuple(0.75 * c for c in col)) for i, col in enumerate(COLORS)]
# Fire in a boss's colour (its arena's braziers, molten metal, lanterns): their lights are
# "light_tint_<i>_*", tinted the same way by the game.
BOSS_FIRE = [material(f"a_bossfire_{i}", tuple(0.45 + 0.55 * c for c in col), emissive=tuple(col)) for i, col in enumerate(COLORS)]

# Height of the fog's opening (the game's fog is 4.2 m high), of the final door, and the place of
# its medallions (see src/render/gates.rs).
OPENING = 4.4
DOOR_HEIGHT = 4.0
MEDALLION_HEIGHT = 4.75
MEDALLION_SPACING = 0.62


def portal(p):
    """Ground point (x, z, y) and unit direction (dx, dz) of a fog passage."""
    (x, z), (lx, lz) = p["pos"], p["look"]
    d = math.hypot(lx - x, lz - z)
    dx, dz = (lx - x) / d, (lz - z) / d
    y = floor_at(x - dx * 0.5, z - dz * 0.5) or 0.0
    return (x, z, y), (dx, dz)


def yaw_of(dx, dz):
    """Blender rotation that turns the local +Y towards the game direction (dx, dz)."""
    return math.atan2(-dx, -dz)


def gateway(mb, hw, width=9.0, height=7.0, depth=10.0, mat=BRICK, seed=0, recess=True, mass=True):
    """A ruined building front with an arched opening (the fog), a dark passage behind it, the
    building's mass and its rock base."""
    t0, t1 = -0.3, 0.9  # thickness of the front
    side = width / 2
    for sx in (-1, 1):
        x0, x1 = hw + 0.15, side
        mb.slab((sx * (x0 + x1) / 2, (t0 + t1) / 2, height / 2), (x1 - x0, t1 - t0, height), mat)
        # Jamb and pilaster.
        mb.box((sx * (hw + 0.35), t0 - 0.1, OPENING / 2), (0.45, 0.3, OPENING), STONE)
        mb.box((sx * (side - 0.3), t0 - 0.15, height / 2 - 0.3), (0.7, 0.45, height - 0.6), STONE)
    mb.slab((0, (t0 + t1) / 2, (OPENING + height) / 2), (2 * hw + 0.3, t1 - t0, height - OPENING), mat)
    mb.box((0, t0 - 0.12, OPENING + 0.25), (2 * hw + 1.2, 0.35, 0.5), STONE)
    mb.box((0, t0 - 0.2, OPENING + 0.45), (0.5, 0.4, 0.8), STONE, taper=(1.4, 1.0))  # keystone
    # Ruined top: a few teeth.
    r = random.Random(seed)
    for k in range(7):
        x = -side + 0.6 + k * (width - 1.2) / 6
        h = 0.4 + r.random() * 1.1
        mb.box((x, (t0 + t1) / 2, height + h / 2), (0.7, t1 - t0, h), mat)
    if recess:
        # Dark passage behind the fog: the light doesn't go far in.
        d0, d1 = t1, t1 + 3.0
        mb.box((0, (d0 + d1) / 2, -0.1), (2 * hw + 0.3, d1 - d0, 0.2), FLOOR)
        for sx in (-1, 1):
            mb.box((sx * (hw + 0.35), (d0 + d1) / 2, OPENING / 2), (0.4, d1 - d0, OPENING), DARKSTONE)
        mb.box((0, (d0 + d1) / 2, OPENING + 0.15), (2 * hw + 1.0, d1 - d0, 0.3), DARKSTONE)
        mb.box((0, d1 + 0.1, OPENING / 2), (2 * hw + 1.0, 0.2, OPENING), VOID)
    if mass:
        mb.box((0, t1 + depth / 2, (height - 0.6) / 2), (width - 0.4, depth, height - 0.6), DARKSTONE, taper=(0.95, 0.9), uv_scale=(3, 2))
        mb.box((0, t1 + depth / 2, height - 0.6 + 1.4), (width - 0.2, depth + 0.2, 2.8), DARKSTONE, taper=(0.1, 0.85))
        rock_cone(mb, 0, depth / 2, -0.4, width / 2 + 0.6, depth / 2 + 1.2, 16, seed=700 + seed, sides=12)
    else:
        rock_cone(mb, 0, (t0 + t1) / 2, -0.4, width / 2 + 0.3, 1.6, 9, seed=700 + seed, sides=10)


def banner(mb, mat, z, w=1.1, h=1.6):
    """Hanging cloth in the boss's colour, in front of a facade, with a pointed tail."""
    y = -0.48
    mb.box((0, y, z), (w + 0.2, 0.08, 0.08), IRON)
    top, bot = z - 0.05, z - h
    v = [mb.bm.verts.new(p) for p in ((-w / 2, y, top), (w / 2, y, top), (w / 2, y, bot), (0, y, bot - 0.35), (-w / 2, y, bot))]
    mb._face(list(reversed(v)), mat)
    v2 = [mb.bm.verts.new((p.co.x, p.co.y + 0.02, p.co.z)) for p in v]
    mb._face(v2, mat)


def boss_gates():
    for i, a in enumerate(ARENAS):
        (x, z, y), (dx, dz) = portal(a["gate"])
        hw = a["gate"]["half_width"]

        def build(mb, hw=hw, i=i):
            if i == 0:
                # The theatre, at the top of the square's stairs: a taller front, its
                # fairground canopy above the opening.
                gateway(mb, hw, width=12.0, height=8.5, depth=12.0, seed=i)
                mb.box((0, -0.75, OPENING + 1.0), (2 * hw + 2.4, 1.0, 0.25), PEWTER)
                mb.box((0, -0.75, OPENING + 1.5), (2 * hw + 2.6, 1.1, 0.9), CANOPY, taper=(0.75, 0.3), uv_scale=(2, 1))
                banner(mb, BOSS_MATS[i], OPENING + 3.6, w=1.4, h=1.4)
            else:
                gateway(mb, hw, seed=i)
                banner(mb, BOSS_MATS[i], OPENING + 2.3)
        obj(f"gate_{i}", build, loc=B(x, z, y), yaw=yaw_of(dx, dz))


def torch_post(mb):
    """Standing torch: stone base, iron pole, a cup with claws (the flame is the game's)."""
    mb.box((0, 0, 0.12), (0.5, 0.5, 0.24), STONE, taper=(0.8, 0.8))
    mb.cylinder((0, 0, 0.24 + 0.7), 0.06, 1.4, IRON, sides=6)
    mb.cylinder((0, 0, 1.62), 0.1, 0.2, IRON, sides=8, radius_top=0.22)
    for k in range(4):
        a = math.pi / 2 * k + math.pi / 4
        mb.seg((0.18 * math.cos(a), 0.18 * math.sin(a), 1.68), (0.3 * math.cos(a), 0.3 * math.sin(a), 1.9), 0.05, 0.05, IRON)
    mb.cylinder((0, 0, 1.69), 0.16, 0.05, DARKSTONE, sides=8)


def torches():
    for i, a in enumerate(ARENAS):
        x, z = a["torch"]
        obj(f"torch_{i}", torch_post, loc=B(x, z, floor_at(x, z) or 0.0))


WALL_FOOT = 4.0  # how far the final wall goes down below its door


def final_door():
    """The door that opens once every boss is defeated: a wall with a double door, and above it a
    medallion socket per boss (the game sets the medallions in them). The leaves are separate
    objects, their origin on the hinge (the game swings them open)."""
    (x, z, y), (dx, dz) = portal(LEVEL["final_door"])
    hw = LEVEL["final_door"]["half_width"]
    yaw = yaw_of(dx, dz)
    # Wide enough to hide the painted panel beyond it from anywhere on the parvis (camera included).
    width, height = 21.0, 8.0

    def wall(mb):
        t0, t1 = -0.4, 1.0
        side = width / 2
        # The wings go down into the rock below (they'd float over the void otherwise).
        low = -WALL_FOOT
        for sx in (-1, 1):
            x0 = hw + 0.15
            mb.slab((sx * (x0 + side) / 2, (t0 + t1) / 2, (height + low) / 2), (side - x0, t1 - t0, height - low), BRICK)
            mb.box((sx * (hw + 0.4), t0 - 0.12, DOOR_HEIGHT / 2), (0.5, 0.35, DOOR_HEIGHT), STONE)
            # Buttresses: framing the door, and at the ends of the wings.
            for bx in (5.1, side - 0.4):
                mb.box((sx * bx, t0 - 0.4, (height + low) / 2), (0.9, 1.0, height - low), STONE, taper=(0.8, 0.6))
        mb.slab((0, (t0 + t1) / 2, (DOOR_HEIGHT + height) / 2), (2 * hw + 0.3, t1 - t0, height - DOOR_HEIGHT), BRICK)
        mb.box((0, t0 - 0.15, DOOR_HEIGHT + 0.2), (2 * hw + 1.4, 0.4, 0.4), STONE)
        # Band of the medallions, and their sockets.
        n = len(ARENAS)
        mb.box((0, t0 - 0.02, MEDALLION_HEIGHT), (n * MEDALLION_SPACING + 0.5, 0.06, 0.8), DARKSTONE)
        for k in range(n):
            mx = (k - (n - 1) / 2) * MEDALLION_SPACING
            mb.cylinder((mx, t0 - 0.06, MEDALLION_HEIGHT), 0.25, 0.1, STONE, sides=10, axis="Y")
        # Pediment.
        mb.box((0, (t0 + t1) / 2, height + 0.6), (10.0, t1 - t0 + 0.2, 1.2), STONE, taper=(0.15, 1.0))
        rock_cone(mb, 0, 0.3, low + 0.3, width / 2 + 0.6, 2.4, 12, seed=777, sides=18)

    obj("final_wall", wall, loc=B(x, z, y), yaw=yaw)

    def leaf(sign):
        def build(mb):
            w = hw - 0.03
            cx = sign * w / 2
            mb.box((cx, 0, DOOR_HEIGHT / 2), (w, 0.18, DOOR_HEIGHT - 0.04), WOOD, uv_scale=(1, 2))
            for zz in (0.7, 2.0, 3.3):
                mb.box((cx, -0.1, zz), (w, 0.04, 0.14), IRON)
            mb.box((sign * (w - 0.25), -0.14, 1.9), (0.1, 0.06, 0.4), IRON)  # handle
        return build

    # Leaf 0 on the left (its hinge at -X), it swings +; leaf 1 the other way (src/render/gates.rs).
    sx, sz = -dz, dx  # local +X, in the game frame
    for k, sign in ((0, 1), (1, -1)):
        hx, hz = x - sign * sx * hw, z - sign * sz * hw
        obj(f"final_door_{k}", leaf(sign), loc=B(hx + dx * 0.3, hz + dz * 0.3, y), yaw=yaw)


FINALE_WIDTH = 16.0  # height from the image's proportions; big: the game renders at 240 lines
FINALE_BOTTOM = 2.0  # above the floor of the last terrace


def finale_panel():
    """Beyond the last terrace (level.ron `finale`), floating over the void and facing the door: a painted panel, thanks
    (tools/blender/finale_art.py), unlit so that it reads in the dark, in a stone frame."""
    import finale_art
    x, z = LEVEL["finale"]
    (_, _, y), (dx, dz) = portal(LEVEL["final_door"])
    px = finale_art.image([tuple(c) for c in COLORS])
    w, h = finale_art.W, finale_art.H
    img = bpy.data.images.new("finale_tex", w, h, alpha=False)
    flat = []
    for row in reversed(px):  # Blender: bottom row first
        for c in row:
            flat.extend((*c, 1.0))
    img.pixels.foreach_set(flat)
    img.pack()
    # Unlit for the glTF export (KHR_materials_unlit): the texture straight into the output.
    m = bpy.data.materials.new("a_finale")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.remove(nt.nodes.get("Principled BSDF"))
    node = nt.nodes.new("ShaderNodeTexImage")
    node.image = img
    node.interpolation = "Closest"
    nt.links.new(node.outputs["Color"], nt.nodes.get("Material Output").inputs["Surface"])

    pw, ph = FINALE_WIDTH, FINALE_WIDTH * h / w

    def build(mb):
        # Its front faces local -Y (towards the door); cells of 1 m (affine texturing).
        mb.split_face([(-pw / 2, 0, 0), (pw / 2, 0, 0), (pw / 2, 0, ph), (-pw / 2, 0, ph)], m,
                      [(0, 0), (1, 0), (1, 1), (0, 1)], size=1.0)
        # Nothing right behind it: with the vertex snapping, a surface a few centimetres back
        # pokes through the image (flickering). The back plate stands well behind; the frame's
        # bars stand in front of its edges (only stars there).
        back = 0.8
        mb.box((0, back, ph / 2), (pw + 0.7, 0.3, ph + 0.7), DARKSTONE)
        f, d = 0.35, back + 0.2  # bar width, depth (from in front of the image to the plate)
        for cx, cz, sx, sz in ((0, -f / 2 + 0.1, pw + 0.7, f), (0, ph + f / 2 - 0.1, pw + 0.7, f),
                               (-pw / 2 - f / 2 + 0.1, ph / 2, f, ph), (pw / 2 + f / 2 - 0.1, ph / 2, f, ph)):
            mb.box((cx, d / 2 - 0.15, cz), (sx, d, sz), DARKSTONE)
        rock_cone(mb, 0, back / 2, -0.25, pw / 2 + 0.3, 0.6, 5, seed=1989, sides=10, rings=3)
    obj("finale_panel", build, loc=B(x, z, y + FINALE_BOTTOM), yaw=yaw_of(dx, dz))


# ----------------------------------------------------------------------------- the arenas, far away

def ellipse_points(cx, cz, rx, rz, sides, phase=0.0):
    return [(cx + rx * math.cos(2 * math.pi * (k + phase) / sides), cz + rz * math.sin(2 * math.pi * (k + phase) / sides)) for k in range(sides)]


def room_floor(mb, cx, cz, rx, rz, y, mat, out=1.0, tile=None):
    """Floor of an elliptical room, `out` beyond its edge (under the wall). `tile`: UVs in world
    coordinates, one texture repeat every `tile` metres, instead of one per ring segment (which
    squeezes the texture towards the centre)."""
    bx, by, _ = B(cx, cz)
    k = rz / rx
    rings = [0.0]
    while rings[-1] + 1.6 < rx - 0.3:
        rings.append(rings[-1] + 1.6)
    rings.append(rx + out)
    sides = 48
    for r0, r1 in zip(rings, rings[1:]):
        if r0 == 0:
            for i in range(sides):
                a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
                v = [mb.bm.verts.new((bx, by, y)), mb.bm.verts.new((bx + r1 * math.cos(a0), by + r1 * k * math.sin(a0), y)),
                     mb.bm.verts.new((bx + r1 * math.cos(a1), by + r1 * k * math.sin(a1), y))]
                mb._face(v, mat, uvs=[(0.5, 0), (0, 1), (1, 1)])
        else:
            ring_quads(mb, r0, r1, y, mat, sides, cx=bx, cy=by, ry=k)
    if tile:
        for f in mb.bm.faces:
            for loop in f.loops:
                loop[mb.uv].uv = ((loop.vert.co.x - bx) / tile, (loop.vert.co.y - by) / tile)


def room_wall(mb, cx, cz, rx, rz, door, hw, height, mat, seed, ruin=0.0, top=STONE, stage=False, bottom=0.0, merlons=False, low=None):
    """Wall around an elliptical floor, open at the door; `ruin`: how much its top is broken.
    `stage`: to the north, only the low front of a stage (the theatre's). `bottom`: where its
    inner face starts (below the floor: a moat, a ditch), `low`: its material below the floor.
    `merlons`: crenellated top."""
    (gx, gz, _), _ = portal(door)
    r = random.Random(seed)
    sides = 40
    for i in range(sides):
        a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
        am = (a0 + a1) / 2
        mx, mz = cx + (rx + 0.5) * math.cos(am), cz + (rz + 0.5) * math.sin(am)
        if math.hypot(mx - gx, mz - gz) < hw + 0.9:
            continue
        h = height - ruin * r.random() * height * 0.6
        m = mat
        if stage and math.sin(am) > 0.5:
            h, m = STAGE_FRONT, WOOD
        p = []
        for a in (a0, a1):
            for off in (0.5, 1.3):
                p.append(B(cx + (rx + off) * math.cos(a), cz + (rz + off) * math.sin(a)))
        (i0, o0), (i1, o1) = (p[0], p[1]), (p[2], p[3])
        v = lambda q, zz: mb.bm.verts.new((q[0], q[1], zz))

        # Its faces are tall (down into a moat, a ditch): split into small cells, or the
        # affine texturing goes haywire up close or at a grazing angle.
        def side(a, b, z0, z1, mat):
            mb.split_face([(a[0], a[1], z0), (b[0], b[1], z0), (b[0], b[1], z1), (a[0], a[1], z1)], mat,
                          [(0, 0), (1, 0), (1, (z1 - z0) / 2.5), (0, (z1 - z0) / 2.5)])
        if low is not None and bottom < 0:
            side(i1, i0, bottom, 0, low)
            side(i1, i0, 0, h, m)
        else:
            side(i1, i0, bottom, h, m)
        side(o0, o1, min(bottom, -1.5), h, m)
        mb._face([v(i0, h), v(o0, h), v(o1, h), v(i1, h)], top)
        if merlons and i % 2 == 0:
            c = B(cx + (rx + 0.9) * math.cos(am), cz + (rz + 0.9) * math.sin(am))
            mb.box((c[0], c[1], h + 0.45), (0.8, 0.8, 0.9), m)


STAGE_FRONT = 1.3


def room_base(mb, cx, cz, rx, rz, seed):
    bx, by, _ = B(cx, cz)
    rock_cone(mb, bx, by, -1.5, rx + 1.4, rz + 1.4, 26, seed=seed, sides=20, rings=5)


def room_braziers(i, cx, cz, rx, rz, door, count=4, high=False, tinted=False, skip=lambda a: False):
    """Braziers along the wall (their light), avoiding the door. `tinted`: their fire in the
    boss's colour. `skip(angle)`: no brazier there (no wall to hang it on)."""
    (gx, gz, _), _ = portal(door)
    fire = BOSS_FIRE[i] if tinted else FIRE
    lname = f"light_tint_{i}" if tinted else f"light_a{i}"
    k = 0
    for j in range(count):
        a = 2 * math.pi * (j + 0.5) / count
        x, z = cx + (rx - 1.0) * math.cos(a), cz + (rz - 1.0) * math.sin(a)
        if math.hypot(x - gx, z - gz) < 4.0 or skip(a):
            continue

        def build(mb):
            mb.cylinder((0, 0, 0.5), 0.08, 1.0, IRON, sides=6)
            mb.cylinder((0, 0, 1.1), 0.35, 0.25, IRON, sides=8, radius_top=0.45)
            mb.box((0, 0, 1.32), (0.35, 0.35, 0.25), fire, taper=(0.4, 0.4))
        bx, by, _ = B(x, z)
        if high:
            # Fixed high on the wall.
            wx, wz = cx + (rx + 0.45) * math.cos(a), cz + (rz + 0.45) * math.sin(a)
            obj(f"arena_{i}_brazier_{k}", build, loc=B(wx, wz, 2.6))
            light(f"{lname}_{k}", B(wx, wz, 4.2))
        else:
            obj(f"arena_{i}_brazier_{k}", build, loc=B(x, z, 0.0))
            light(f"{lname}_{k}", B(x, z, 1.6))
        k += 1


def room_porch(i, a, mat):
    """The arena's door seen from inside: a gateway in the wall, its dark passage outside."""
    (x, z, y), (dx, dz) = portal(a["door"])
    hw = a["door"]["half_width"]
    obj(f"arena_{i}_porch", lambda mb: gateway(mb, hw, width=6.5, height=6.0, depth=4.0, mat=mat, seed=40 + i, mass=False),
        loc=B(x, z, y), yaw=yaw_of(-dx, -dz))


def room_skyline(i, cx, cz, rx, mat, seed, n=14, tall=1.0):
    """Silhouettes of ruins all around, on rock spires, beyond the wall."""
    r = random.Random(seed)

    def build(mb):
        for k in range(n):
            ang = 2 * math.pi * (k + r.random() * 0.5) / n
            d = rx + 12 + r.random() * 18
            x, y = d * math.cos(ang), d * math.sin(ang)
            h = (6 + r.random() * 12) * tall
            rock_cone(mb, x, y, -0.5, 4.0, 3.4, 16 + r.random() * 10, seed=seed * 10 + k, sides=8, rings=3)
            mb.box((x, y, h / 2), (5 + r.random() * 2, 4 + r.random() * 2, h), mat, taper=(0.85, 0.85), uv_scale=(2, 3))
            if r.random() < 0.6:
                mb.box((x, y, h + 1.2), (5.4, 4.4, 2.4), mat, taper=(0.1, 0.9))
    bx, by, _ = B(cx, cz)
    obj(f"arena_{i}_skyline", build, loc=(bx, by, 0))


def ledges(i, a, mat):
    """Galleries and their stairs: solid down to the ground."""
    for k, f in enumerate(a["floors"]):
        if not f.get("ledge"):
            continue
        if "Strip" in f["shape"]:
            obj(f"arena_{i}_stairs_{k}", lambda mb, f=f, k=k: strip(mb, f, 1000 + k))
        else:
            e = f["shape"]["Ellipse"]
            (cx, cz), (rx, rz), y = e["center"], e["radii"], e["y"]

            def build(mb, cx=cx, cz=cz, rx=rx, rz=rz, y=y):
                room_floor(mb, cx, cz, rx - 1.0, rz * (rx - 1.0) / rx, y, FLOOR)
                bx, by, _ = B(cx, cz)
                sides = 24
                for s_ in range(sides):
                    a0, a1 = 2 * math.pi * s_ / sides, 2 * math.pi * (s_ + 1) / sides
                    p = [(bx + rx * math.cos(q), by + rz * math.sin(q)) for q in (a0, a1)]
                    v = [mb.bm.verts.new((p[1][0], p[1][1], y)), mb.bm.verts.new((p[0][0], p[0][1], y)),
                         mb.bm.verts.new((p[0][0], p[0][1], 0)), mb.bm.verts.new((p[1][0], p[1][1], 0))]
                    mb._face(v, mat, uv_scale=(0.6, y / 2))
                # Corbels under the edge.
                for s_ in range(0, sides, 3):
                    q = 2 * math.pi * s_ / sides
                    mb.box((bx + (rx - 0.1) * math.cos(q), by + (rz - 0.1) * math.sin(q), y - 0.5), (0.4, 0.4, 1.0), STONE, taper=(1.4, 1.4))
            obj(f"arena_{i}_gallery_{k}", build)


def broken_pillar(mb, x, y, r, h, mat, seed):
    rr = random.Random(seed)
    mb.cylinder((x, y, 0.25), r * 1.25, 0.5, STONE, sides=8)
    mb.cylinder((x, y, h / 2), r, h, mat, sides=8, uv_scale=(2, h / 2))
    for k in range(3):
        a = rr.random() * 2 * math.pi
        mb.box((x + r * 0.4 * math.cos(a), y + r * 0.4 * math.sin(a), h + 0.2), (r * 0.9, r * 0.8, 0.5 + rr.random() * 0.6), mat, taper=(0.4, 0.5))


def arena_room(i, a):
    main = next(f for f in a["floors"] if not f.get("ledge"))
    e = main["shape"]["Ellipse"]
    (cx, cz), (rx, rz) = e["center"], e["radii"]
    hw = a["door"]["half_width"]
    theme = a.get("theme", "")
    pil = [tuple(p) for p in a["pillars"]]
    seed = 900 + i * 37
    # `custom`: the theme draws its own floor, wall and base (moat, lava ditch).
    style = {
        "summit": dict(floor=FLOOR, wall=CASTLE, height=8.0, ruin=0.0, sky=DARKSTONE, tall=1.6, custom=True),
        "slaughter": dict(floor=DARKSTONE, wall=DARKBRICK, height=5.5, ruin=0.15, sky=DARKBRICK, tall=0.8),
        "foundry": dict(floor=SAND, wall=DARKBRICK, height=6.0, ruin=0.1, sky=DARKBRICK, tall=1.2, tile=8.0),
        "guignol": dict(floor=PLANKS, wall=DARKBRICK, height=7.0, ruin=0.05, sky=DARKSTONE, tall=1.0),
        "cistern": dict(floor=ICE, wall=DARKSTONE, height=6.0, ruin=0.1, sky=DARKSTONE, tall=0.9),
        "hearth": dict(floor=BURNT, wall=BURNT, height=4.5, ruin=0.5, sky=BURNT, tall=1.3, custom=True),
    }.get(theme, dict(floor=FLOOR, wall=BRICK, height=5.0, ruin=0.2, sky=DARKSTONE, tall=1.0))
    if not style.get("custom"):
        obj(f"arena_{i}_floor", lambda mb: room_floor(mb, cx, cz, rx, rz, 0.0, style["floor"], tile=style.get("tile")))
        obj(f"arena_{i}_wall", lambda mb: room_wall(mb, cx, cz, rx, rz, a["door"], hw, style["height"], style["wall"], seed, style["ruin"],
                                                    stage=theme == "guignol"))
        obj(f"arena_{i}_base", lambda mb: room_base(mb, cx, cz, rx, rz, seed))
    room_porch(i, a, style["wall"])
    room_skyline(i, cx, cz, max(rx, rz), style["sky"], seed + 1, tall=style["tall"])
    ledges(i, a, style["wall"])
    THEMES.get(theme, lambda *_: None)(i, a, cx, cz, rx, rz, pil, seed, style)


def wall_torch(i, k, x, z, ang, h=3.2):
    """Torch on an iron bracket, fixed to a wall that faces the angle `ang` (towards the room)."""
    c, sn = math.cos(ang), math.sin(ang)

    def build(mb):
        nx, ny = -c, sn  # towards the room, in Blender coordinates
        mb.box((0, 0, 0), (0.4, 0.4, 0.7), IRON)
        mb.seg((0, 0, -0.2), (nx * 0.8, ny * 0.8, 0.35), 0.1, 0.1, IRON)
        mb.cylinder((nx * 0.85, ny * 0.85, 0.5), 0.14, 0.3, IRON, sides=6, radius_top=0.3)
        mb.box((nx * 0.85, ny * 0.85, 0.85), (0.4, 0.4, 0.5), FIRE, taper=(0.3, 0.3))
    obj(f"arena_{i}_torch_{k}", build, loc=B(x, z, h))
    light(f"light_a{i}_{k}", B(x - c * 0.85, z - sn * 0.85, h + 1.2))


def theme_summit(i, a, cx, cz, rx, rz, pil, seed, style):
    """The Soot Wyvern: the courtyard of a ruined castle. A moat (the void) all around it, then the
    crenellated curtain wall and its towers; the drawbridge crosses the moat to the gatehouse.
    Torches on the walls, a few broken pillars to shelter from the fire."""
    (gx, gz, _), _ = portal(a["door"])
    door_ang = math.atan2(gz - cz, gx - cx)
    wall_r = math.hypot(gx - cx, gz - cz) - 0.5  # the wall's inner face, at the gate
    moat = -14.0  # the walls go down this far into the void, then nothing

    def courtyard(mb):
        room_floor(mb, cx, cz, rx, rz, 0.0, FLOOR, out=0.0)
        bx, by, _ = B(cx, cz)
        sides = 48
        # Edge of the courtyard: a retaining wall plunging into the moat, a rock spur under it.
        for k in range(sides):
            a0, a1 = 2 * math.pi * k / sides, 2 * math.pi * (k + 1) / sides
            p = [(bx + rx * math.cos(q), by - rz * math.sin(q)) for q in (a0, a1)]
            mb.split_face([(p[0][0], p[0][1], 0), (p[1][0], p[1][1], 0), (p[1][0], p[1][1], moat * 0.6),
                           (p[0][0], p[0][1], moat * 0.6)], CASTLE_DARK, [(0, 0), (1, 0), (1, 3), (0, 3)])
        rock_cone(mb, bx, by, moat * 0.6, rx, rz, 20, seed=seed, sides=20, rings=4)
        # Pale coping stones along the edge (low, with gaps): you see where the void starts.
        r = random.Random(seed)
        for k in range(72):
            q = 2 * math.pi * (k + 0.5) / 72
            if r.random() < 0.25 or abs(math.remainder(q - door_ang, 2 * math.pi)) < 0.1:
                continue
            x, z = cx + (rx - 0.25) * math.cos(q), cz + (rz - 0.25) * math.sin(q)
            c = B(x, z)
            mb.box((c[0], c[1], 0.07), (0.5, 0.5, 0.14), CREAM, taper=(0.85, 0.85))
    obj(f"arena_{i}_floor", courtyard)

    def curtain(mb):
        room_wall(mb, cx, cz, wall_r - 0.5, wall_r - 0.5, a["door"], a["door"]["half_width"] + 1.0, style["height"], CASTLE, seed,
                  bottom=moat, merlons=True, low=CASTLE_DARK)
        # Towers on the wall (not at the gate: its own two towers).
        for k in range(6):
            q = door_ang + 2 * math.pi * (k + 1) / 7
            x, z = cx + (wall_r + 0.6) * math.cos(q), cz + (wall_r + 0.6) * math.sin(q)
            b = B(x, z)
            tower(mb, b[0], b[1], 2.4, style["height"] + 3.5, moat, CASTLE)
        for sx in (-1, 1):
            q = door_ang + sx * 0.19
            x, z = cx + (wall_r + 0.9) * math.cos(q), cz + (wall_r + 0.9) * math.sin(q)
            b = B(x, z)
            tower(mb, b[0], b[1], 2.0, style["height"] + 5.0, moat, CASTLE)
    obj(f"arena_{i}_wall", curtain)

    def drawbridge(mb):
        f = next(f for f in a["floors"] if "Strip" in f["shape"])
        x0, z0, _, x1, z1, _, hw = strip_geom(f)
        ln = math.hypot(x1 - x0, z1 - z0)
        ux, uz = (x1 - x0) / ln, (z1 - z0) / ln
        # It stops at the edge of the courtyard (no deck over the paving).
        t1 = ln
        while t1 > 0 and ((x0 + ux * t1 - cx) / rx) ** 2 + ((z0 + uz * t1 - cz) / rz) ** 2 < 1.0:
            t1 -= 0.05
        sx, sz = uz, -ux
        P = lambda t, side, y: B(x0 + ux * t + sx * side, z0 + uz * t + sz * side, y)
        n = max(1, round(t1 / 0.6))
        for k in range(n):
            ta, tb = t1 * k / n, t1 * (k + 1) / n
            c0, c1 = P((ta + tb) / 2, -hw, -0.15), P((ta + tb) / 2, hw, -0.15)
            mb.seg(c0, c1, (tb - ta) - 0.04, 0.3, PLANKS)
        # Beams under it, rails and chains up to the gatehouse.
        for side in (-hw + 0.25, hw - 0.25):
            mb.seg(P(0, side, -0.45), P(t1, side, -0.45), 0.25, 0.3, WOOD)
        for side in (-hw - 0.1, hw + 0.1):
            for t in (0.3, t1 / 2, t1 - 0.2):
                mb.seg(P(t, side, 0), P(t, side, 1.0), 0.14, 0.14, WOOD)
            mb.seg(P(0.3, side, 0.95), P(t1 - 0.2, side, 0.95), 0.1, 0.12, WOOD)
            mb.seg(P(t1 - 0.2, side, 1.0), P(-0.6, side, 5.5), 0.06, 0.06, IRON)
    obj(f"arena_{i}_drawbridge", drawbridge)

    def pillars_(mb):
        for k, (x, z, r) in enumerate(pil):
            bx, by, _ = B(x, z)
            broken_pillar(mb, bx, by, r, 3.0 + (k % 3) * 1.6, STONE, seed + k)
    obj(f"arena_{i}_decor", pillars_)
    # Torches on the curtain wall, all around (not at the gate).
    for k in range(10):
        q = door_ang + 2 * math.pi * (k + 0.5) / 10
        if abs(math.remainder(q - door_ang, 2 * math.pi)) < 0.5:
            continue
        x, z = cx + (wall_r + 0.05) * math.cos(q), cz + (wall_r + 0.05) * math.sin(q)
        wall_torch(i, k, x, z, q)

    def spires(mb):
        rr = random.Random(seed + 5)
        for k in range(5):
            ang = 2 * math.pi * (k + 0.3) / 5 + 0.4
            d = wall_r + 8 + rr.random() * 6
            x, y = d * math.cos(ang), d * math.sin(ang)
            rock_cone(mb, x, y, 0.0, 3.0, 3.0, 20, seed=seed + 50 + k, sides=8)
            mb.box((x, y, 6), (3.0, 3.0, 12 + rr.random() * 8), STONE, taper=(0.6, 0.6), uv_scale=(1, 4))
            mb.box((x, y, 14), (2.0, 2.0, 2.5), VOID)  # empty belfry
    bx, by, _ = B(cx, cz)
    obj(f"arena_{i}_spires", spires, loc=(bx, by, 0))


def tower(mb, x, y, r, h, bottom, mat):
    """Round castle tower (Blender coordinates), from `bottom` up, crenellated, a slate roof."""
    mb.cylinder((x, y, (h + bottom) / 2), r, h - bottom, mat, sides=10, uv_scale=(3, (h - bottom) / 3), split=2.0)
    mb.cylinder((x, y, h + 0.2), r + 0.3, 0.4, STONE, sides=10)
    for k in range(10):
        if k % 2:
            continue
        q = 2 * math.pi * (k + 0.5) / 10
        mb.box((x + (r + 0.05) * math.cos(q), y + (r + 0.05) * math.sin(q), h + 0.85), (0.6, 0.6, 0.9), mat)
    mb.cylinder((x, y, h + 2.4), r * 0.95, 3.6, DARKSTONE, sides=10, radius_top=0.05)
    mb.box((x, y, h - 2.0), (0.25, 2 * r + 0.02, 0.9), VOID)  # arrow slits
    mb.box((x, y, h - 2.0), (2 * r + 0.02, 0.25, 0.9), VOID)


def theme_slaughter(i, a, cx, cz, rx, rz, pil, seed, style):
    """The Knacker: a cramped slaughter yard, butcher blocks, carcasses on hooks."""
    def build(mb):
        rr = random.Random(seed)
        for k, (x, z, r) in enumerate(pil):
            bx, by, _ = B(x, z)
            mb.box((bx, by, 0.45), (r * 1.8, r * 1.4, 0.9), WOOD)
            mb.seg((bx + 0.1, by, 0.9), (bx + 0.3, by + 0.1, 1.3), 0.4, 0.04, IRON)  # cleaver
        # Pools of blood: one flat polygon each, well above the paving, never on top of each other
        # (overlapping faces flicker).
        spots = []
        while len(spots) < 10:
            x, z = cx + rr.uniform(-rx + 2, rx - 2) * 0.8, cz + rr.uniform(-rz + 2, rz - 2) * 0.8
            if all(math.hypot(x - u, z - v) > 1.6 for u, v in spots) and all(math.hypot(x - px, z - pz) > pr + 0.8 for px, pz, pr in pil):
                spots.append((x, z))
        for x, z in spots:
            bx, by, _ = B(x, z)
            n = 7
            rad = [0.35 + rr.random() * 0.35 for _ in range(n)]
            v = [mb.bm.verts.new((bx + rad[j] * math.cos(2 * math.pi * j / n), by + rad[j] * 0.8 * math.sin(2 * math.pi * j / n), 0.04))
                 for j in range(n)]
            mb._face(v, BLOOD)
        # Two beams across the yard, high up, with chains, hooks and carcasses: north-south, away
        # from the gallery (nothing to bump into when jumping from it).
        for k, dx_ in enumerate((4.0, 9.0)):
            half = rz * math.sqrt(1 - (dx_ / rx) ** 2) - 0.1
            z0, z1 = cz - half, cz + half
            b0, b1 = B(cx + dx_, z0), B(cx + dx_, z1)
            mb.seg((b0[0], b0[1], 5.0), (b1[0], b1[1], 5.0), 0.3, 0.3, WOOD)
            for j in range(4):
                hz = z0 + (z1 - z0) * (j + 0.6) / 4
                bx, by, _ = B(cx + dx_, hz)
                # High enough to pass under (nobody bumps into them).
                low = 4.1 + rr.random() * 0.4
                mb.seg((bx, by, 5.0), (bx, by, low), 0.04, 0.04, IRON)
                if rr.random() < 0.7:
                    mb.box((bx, by, low - 0.45), (0.45, 0.3, 0.9), BLOOD, taper=(0.6, 0.8))
    obj(f"arena_{i}_decor", build)
    room_braziers(i, cx, cz, rx, rz, a["door"], count=4, high=True)


def theme_foundry(i, a, cx, cz, rx, rz, pil, seed, style):
    """The Wick-Trimmer and the Anvil: a lantern foundry; its columns carry lanterns. Near the
    wall, crucibles of molten metal and anvils (the pillars after the six columns). Every fire
    burns in the duo's green (their lanterns' glow), none orange."""
    def build(mb):
        for k, (x, z, r) in enumerate(pil[:6]):
            bx, by, _ = B(x, z)
            mb.box((bx, by, 0.3), (r * 2.2, r * 2.2, 0.6), STONE)
            mb.cylinder((bx, by, 2.6), r * 0.6, 4.2, IRON, sides=6)
            mb.box((bx, by, 4.95), (0.5, 0.5, 0.6), BOSS_FIRE[i], taper=(1.15, 1.15))
            mb.box((bx, by, 5.35), (0.7, 0.7, 0.2), IRON, taper=(0.2, 0.2))
            light(f"light_tint_{i}_lamp{k}", (bx, by, 4.95))
    obj(f"arena_{i}_decor", build)
    for k, (x, z, r) in enumerate(pil[6:]):
        yaw = math.atan2(-(x - cx), -(z - cz))  # facing the centre
        obj(f"arena_{i}_{'anvil' if k % 2 else 'crucible'}_{k}", anvil(r) if k % 2 else crucible(r, BOSS_FIRE[i]), loc=B(x, z), yaw=yaw)
    room_braziers(i, cx, cz, rx, rz, a["door"], count=2, tinted=True)

    def chimneys(mb):
        for k in range(4):
            ang = 2 * math.pi * (k + 0.2) / 4
            x, y = (rx + 4) * math.cos(ang), (rz + 4) * math.sin(ang)
            mb.box((x, y, 8), (2.2, 2.2, 16), DARKBRICK, taper=(0.75, 0.75), uv_scale=(1, 6))
    bx, by, _ = B(cx, cz)
    obj(f"arena_{i}_chimneys", chimneys, loc=(bx, by, 0))


def crucible(r, metal=EMBER):
    """Crucible of molten metal (`r`: its collision radius), hollow: the metal well below the rim
    (faces a few centimetres apart flicker)."""
    def build(mb):
        sides = 10
        mb.cylinder((0, 0, 0.45), r * 0.8, 0.9, IRON, sides=sides, radius_top=r, caps=False)
        ring_quads(mb, r - 0.12, r, 0.9, IRON, sides)
        tube_inside(mb, r - 0.12, 0.5, 0.9, IRON, sides)
        disc(mb, r - 0.1, 0.72, metal, sides)
        for k in range(3):
            q = 2 * math.pi * k / 3
            mb.box((r * 0.75 * math.cos(q), r * 0.75 * math.sin(q), 0.1), (0.25, 0.25, 0.2), STONE)
    return build


def anvil(r):
    """Anvil on its stump (`r`: its collision radius), a hammer resting on it."""
    def build(mb):
        mb.cylinder((0, 0, 0.3), r * 0.7, 0.6, WOOD, sides=8)
        mb.box((0, 0, 0.72), (0.35, 0.3, 0.25), IRON, taper=(1.3, 1.3))
        mb.box((0, 0, 0.95), (r * 1.4, 0.36, 0.22), IRON)
        mb.box((r * 0.85, 0, 0.98), (0.3, 0.2, 0.14), IRON, taper=(0.2, 0.6))  # horn
        mb.seg((-0.2, -0.05, 1.1), (0.25, 0.25, 1.08), 0.06, 0.06, WOOD)
        mb.box((-0.2, -0.05, 1.12), (0.12, 0.26, 0.12), IRON)
    return build


def theme_guignol(i, a, cx, cz, rx, rz, pil, seed, style):
    """The Great Marionette: a puppet theatre; the stage beyond the north wall, fallen chandeliers."""
    def build(mb):
        # Curtains along the wall (not around the door, nor in front of the stage).
        for k in range(14):
            ang = 2 * math.pi * (k + 0.5) / 14
            x, z = cx + (rx + 0.42) * math.cos(ang), cz + (rz + 0.42) * math.sin(ang)
            if z < cz - rz + 2 or math.sin(ang) > 0.5:
                continue
            bx, by, _ = B(x, z)
            mb.box((bx, by, 3.6), (1.4, 0.12, 6.4), CURTAIN, taper=(0.7, 1.0))
        # The stage, beyond the low north wall: its floor follows the wall, a proscenium frames
        # it, puppets hang from their strings in front of the back curtain.
        ro, zo = rx + 1.3, rz + 1.3
        W = 0.866 * ro
        back = cz + rz + 10.0
        front = lambda x: cz + zo * math.sqrt(max(0.0, 1 - (x / ro) ** 2))
        n = 16
        for k in range(n):
            x0, x1 = -W + 2 * W * k / n, -W + 2 * W * (k + 1) / n
            q = [B(x0 + cx, front(x0)), B(x1 + cx, front(x1)), B(x1 + cx, back), B(x0 + cx, back)]
            v = [mb.bm.verts.new((p_[0], p_[1], STAGE_FRONT)) for p_ in q]
            mb._face(v if is_ccw(q) else list(reversed(v)), PLANKS, uv_scale=(2, 3))
        for sx in (-1, 1):
            x = cx + sx * W
            mb.box(B(x, (front(sx * W) + back) / 2, -0.1), (0.3, back - front(sx * W), 2.8), WOOD)
            # Proscenium: a gilded column on each side, the beam high above.
            mb.box(B(x, front(sx * W) + 0.8, 6.0), (1.4, 1.4, 12), GOLD, uv_scale=(1, 4))
            mb.box(B(x - sx * 1.6, front(sx * W) + 0.8, 8.5), (2.6, 0.3, 7.0), CURTAIN, taper=(0.5, 1.0))
        bx, by, _ = B(cx, front(W) + 0.8)
        mb.box((bx, by, 12.6), (2 * W + 2.0, 1.6, 1.4), GOLD)
        bx, by, _ = B(cx, back)
        mb.box((bx, by, 7.0), (2 * W, 0.3, 11.5), CURTAIN)
        rr = random.Random(seed)
        for k in range(5):
            px = cx - 6 + k * 3
            pz = cz + rz + 5 + rr.random() * 2
            bx, by, _ = B(px, pz)
            h = STAGE_FRONT + 1.2 + rr.random() * 1.5
            mb.seg((bx, by, 12), (bx, by, h + 1.2), 0.02, 0.02, CREAM)
            mb.box((bx, by, h + 0.6), (0.5, 0.3, 1.2), PUPPET)
            mb.box((bx, by, h + 1.35), (0.3, 0.3, 0.3), CREAM)
    obj(f"arena_{i}_decor", build)
    # Braziers on the wall, but not over the stage (its front is low: they would float in the
    # air); torches on the proscenium's columns light it instead.
    room_braziers(i, cx, cz, rx, rz, a["door"], count=4, high=True, skip=lambda q: math.sin(q) > 0.45)
    ro, zo = rx + 1.3, rz + 1.3
    W = 0.866 * ro
    for k, sx in ((10, -1), (11, 1)):
        px, pz = cx + sx * W, cz + 0.5 * zo + 0.8  # the column's centre (see build)
        d = math.hypot(px - cx, pz - cz)
        x, z = px - (px - cx) / d * 0.72, pz - (pz - cz) / d * 0.72  # on its face, towards the hall
        wall_torch(i, k, x, z, math.atan2(z - cz, x - cx))


def theme_cistern(i, a, cx, cz, rx, rz, pil, seed, style):
    """The Rimeback: a frozen cistern, broken columns crusted with ice, icicles. Blue fires."""
    def build(mb):
        rr = random.Random(seed)
        for k, (x, z, r) in enumerate(pil):
            bx, by, _ = B(x, z)
            broken_pillar(mb, bx, by, r, 2.2 + rr.random() * 3.0, STONE, seed + k)
            mb.cylinder((bx, by, 0.3), r * 1.5, 0.6, ICE, sides=8, radius_top=r * 1.05)
        # Icicles hanging from the top of the wall.
        for k in range(40):
            ang = 2 * math.pi * (k + rr.random() * 0.5) / 40
            x, z = cx + (rx + 0.6) * math.cos(ang), cz + (rz + 0.6) * math.sin(ang)
            bx, by, _ = B(x, z)
            ln = 0.6 + rr.random() * 1.6
            mb.box((bx, by, 6.0 - ln / 2), (0.25, 0.25, ln), ICE, taper=(1.0, 1.0), shift_top=(0, 0))
        # Blocks of ice against the wall (half in it: nothing to walk through).
        for k in range(8):
            ang = rr.random() * 2 * math.pi
            d = rx + 0.35
            (gx, gz, _), _ = portal(a["door"])
            if math.hypot(cx + d * math.cos(ang) - gx, cz + d * math.sin(ang) - gz) < 3.5:
                continue
            bx2, by2, _ = B(cx + d * math.cos(ang), cz + d * math.sin(ang))
            mb.box((bx2, by2, 0.35), (0.9, 0.8, 0.7), ICE, taper=(0.6, 0.7))
    obj(f"arena_{i}_decor", build)
    # Its fires burn in the Rimeback's icy blue, none orange.
    room_braziers(i, cx, cz, rx, rz, a["door"], count=4, high=True, tinted=True)


def theme_hearth(i, a, cx, cz, rx, rz, pil, seed, style):
    """The Dead-Hearth Giant: the hearth of a burnt city. Great pillars; a low parapet around the
    floor, and beyond it, at the foot of the ruined wall, a ditch of dead lava: dark ash, like his
    dead fire (assets/config/bosses.ron)."""
    hw = a["door"]["half_width"]
    (gx, gz, _), _ = portal(a["door"])
    door_ang = math.atan2(gz - cz, gx - cx)
    ditch, lava = 4.0, 0.12  # width of the ditch, height of the lava (seen over the parapet)
    outer = rx + 0.8 + ditch  # inner face of the outer wall

    def build(mb):
        room_floor(mb, cx, cz, rx, rz, 0.0, BURNT, out=0.5)
        # The parapet: the walkable floor's edge (the collision).
        room_wall(mb, cx, cz, rx, rz, a["door"], hw, 0.5, STONE, seed, top=DARKSTONE, bottom=0.0)
        bx, by, _ = B(cx, cz)
        # The lava, out to the wall, and crusts drifting on it.
        ring_quads(mb, rx + 1.0, outer + 0.3, lava, ASHLAVA, 48, uv=(2, 1), cx=bx, cy=by, ry=rz / rx)
        rr = random.Random(seed)
        for k in range(26):
            q = 2 * math.pi * (k + rr.random() * 0.6) / 26
            if abs(math.remainder(q - door_ang, 2 * math.pi)) < 0.25:
                continue
            d = rx + 1.6 + rr.random() * (ditch - 1.6)
            c = B(cx + d * math.cos(q), cz + d * rz / rx * math.sin(q))
            w = 0.6 + rr.random() * 1.2
            mb.box((c[0], c[1], lava + 0.06), (w, w * (0.6 + rr.random() * 0.5), 0.35), BURNT, taper=(0.6, 0.7))
        for k, (x, z, r) in enumerate(pil):
            px, py, _ = B(x, z)
            mb.cylinder((px, py, 0.3), r * 1.3, 0.6, STONE, sides=8)
            mb.cylinder((px, py, 3.8), r, 7.0, BURNT, sides=8, uv_scale=(2, 4))
            mb.cylinder((px, py, 7.45), r * 1.35, 0.3, STONE, sides=8)
    obj(f"arena_{i}_decor", build)
    # The outer wall has no opening: the gateway's dark passage ends against it.
    obj(f"arena_{i}_wall", lambda mb: room_wall(mb, cx, cz, outer - 0.5, (outer - 0.5) * rz / rx, a["door"], -1.0,
                                                style["height"], BURNT, seed + 1, style["ruin"], bottom=lava - 0.3))
    obj(f"arena_{i}_base", lambda mb: room_base(mb, cx, cz, outer, outer * rz / rx, seed))
    # The ashes' faint glow, all along the ditch (a cold grey light).
    for k in range(8):
        q = door_ang + 2 * math.pi * (k + 0.5) / 8
        d = rx + 0.8 + ditch / 2
        light(f"light_ash_a{i}_{k}", B(cx + d * math.cos(q), cz + d * rz / rx * math.sin(q), 0.8))


THEMES = {"summit": theme_summit, "slaughter": theme_slaughter, "foundry": theme_foundry,
          "guignol": theme_guignol, "cistern": theme_cistern, "hearth": theme_hearth}

# ----------------------------------------------------------------------------- assembly

for name, build in (("floor", arena_floor), ("wall", wall), ("base", arena_base), ("pillars", pillars),
                    ("braziers", braziers), ("carousel", carousel), ("skyline", skyline)):
    obj(f"arena_0_{name}", build, loc=B(TX, TZ))
room_porch(0, THEATRE, BRICK)
for i, f in enumerate(LEVEL["floors"]):
    if "Ellipse" in f["shape"]:
        obj(f"platform_{i}", lambda mb, f=f, i=i: platform(mb, f, seed=200 + i))
    else:
        obj(f"strip_{i}", lambda mb, f=f, i=i: strip(mb, f, i))
obj("ring_marks", ring_marks)
props()
checkpoints()
boss_gates()
torches()
final_door()
finale_panel()
for i, a in enumerate(ARENAS):
    if i > 0:
        arena_room(i, a)
far_lamps()
for name, p in LIGHTS:
    e = bpy.data.objects.new(name, None)
    e.location = p
    sc.collection.objects.link(e)
export("arena")
