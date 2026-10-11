"""Generates the other bosses (assets/config/bosses.ron): models and animations.

blender -b --factory-startup -P tools/blender/trial_bosses.py [-- name ...]

Without arguments, all models are generated. Animations are timed on the attack timings
(tools/blender/timings.json, `bosses`): "h0" start of hit 0, "c0" tick of spell 0…
The empty objects `lock_*` are the lockable points (the bosses' `parts`) followed by the reticle.
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common  # noqa: E402
from common import *  # noqa: E402,F403

R, L = -1, 1
SIDES = (("R", R), ("L", L))


def start(name, cell=None):
    reset_scene()
    common._materials.clear()
    common.MeshBuilder.cell = cell
    return Rig(name)


TIMINGS = load_timings().get("bosses", {})


def finish(rig, name, idle_keys, clips=()):
    """Exports the model: the looping idle, then the `clips` (name, keys[, loop])."""
    T = TIMINGS.get(name, {})
    markers = {}
    end = add_animation(rig, "idle", idle_keys)
    markers["idle"] = {"frames": end, "marks": [0, end], "loop": True}
    for clip in clips:
        cname, keys = clip[0], clip[1]
        loop = len(clip) > 2 and clip[2]
        info = T.get(cname, {"total": 0, "hits": [], "casts": []})
        end = add_animation(rig, cname, keys, info)
        markers[cname] = {"frames": end, "marks": anim_markers(info) if cname in T else [0, end], "loop": loop}
    missing = sorted(set(T) - set(markers))
    if missing:
        raise SystemExit(f"{name}: missing animations: {missing}")
    rest_pose(rig)
    export(name, markers)


def lock(rig, name, at, parent):
    """Lockable point (empty object) followed by the game's reticle."""
    rig.part(name, at, parent)


def tween(a, b, k):
    """Intermediate pose between two poses (interpolated rotations and translations)."""
    out = {}
    for key in set(a) | set(b):
        va, vb = a.get(key, (0, 0, 0)), b.get(key, (0, 0, 0))
        da = va if isinstance(va, dict) else {"r": va, "t": (0, 0, 0)}
        db = vb if isinstance(vb, dict) else {"r": vb, "t": (0, 0, 0)}
        r = tuple(x + (y - x) * k for x, y in zip(da["r"], db["r"]))
        t = tuple(x + (y - x) * k for x, y in zip(da.get("t", (0, 0, 0)), db.get("t", (0, 0, 0))))
        out[key] = {"r": r, "t": t}
    return out


def biped_walk(base, frames=80, stride=22, arms=6):
    """Biped walk: alternating legs, slight roll, swinging arms."""
    def fn(p, ph):
        a = s(ph)
        out = add(p, thigh_R=(-stride * a, 0, 0), shin_R=(14 * max(0.0, s(ph, 1, 0.25)), 0, 0),
                  thigh_L=(stride * a, 0, 0), shin_L=(14 * max(0.0, s(ph, 1, 0.75)), 0, 0),
                  foot_R=(stride * a * 0.6, 0, 0), foot_L=(-stride * a * 0.6, 0, 0),
                  upper_arm_R=(arms * a, 0, 0), upper_arm_L=(-arms * a, 0, 0), chest=(0, 0, 3 * a))
        return lift(out, "hips", (0, 0, -0.04 * abs(s(ph, 2))))
    return cycle(base, 8, frames, fn)


def biped_states(base, hip, look_arms=None):
    """Stagger, fatal blow received, roar and death of a biped whose hips are at `hip` m."""
    k = hip / 1.65
    kneel = merge(base, legs(-80, 85, 75, 100, -0.75 * k), {"chest": (38, 0, 8), "head": (35, 0, 0)},
                  arm("R", (-40, 15, 0), (-30, 0, 0), (40, 0, 0)), arm("L", (-10, -10, 0), (-20, 0, 0)))
    rise = merge(base, legs(-40, 60, 30, 60, -0.35 * k))
    roar = merge(base, legs(-20, 25, 20, 25, -0.1 * k), {"chest": (-25, 0, 0), "head": (-40, 0, 0)},
                 arm("R", (-100, 70, 0), (-10, 0, 0), (40, 0, 0)), arm("L", (-100, -70, 0), (-10, 0, 0)))
    if look_arms:
        roar = merge(roar, look_arms)
    fallen = merge(base, legs(-90, 0, -90, 10, 0), {"hips": {"r": (85, 0, 15), "t": (0, -0.9 * k, -1.45 * k)},
                                                    "chest": (5, 0, 0), "head": (10, 0, 30)},
                   arm("R", (-160, 30, 0)), arm("L", (-150, -40, 0)))
    return [
        ("groggy", [(0, base), (14, kneel), (100, add(kneel, head=(10, 0, 10))), ("T-40", kneel), ("T-24", rise), ("T", base)]),
        ("fatal_received", [(0, kneel), (40, kneel), (50, add(kneel, chest=(-58, 0, 0), head=(-75, 0, 0))),
                            (80, add(kneel, chest=(12, 0, 0), head=(15, 0, 0))), ("T-16", rise), ("T", base)]),
        ("roar", [(0, base), (20, add(base, chest=(20, 0, 0))), (34, roar), ("T-30", add(roar, head=(10, 0, 10))), ("T", base)]),
        ("death", [(0, base), (30, add(kneel, head=(-65, 0, 0), chest=(-48, 0, 0))), (70, kneel), (110, fallen), ("T", fallen)]),
    ]


def sheet(mb, pts, mat):
    """Flat membrane visible from both sides (convex polygon, fan-triangulated)."""
    for order in (pts, list(reversed(pts))):
        for i in range(1, len(order) - 1):
            tri = [order[0], order[i], order[i + 1]]
            mb._face([mb.bm.verts.new(p) for p in tri], mat)


def mx(p, s):
    """Point (x, y, z) defined for the left side, mirrored to side `s`."""
    return (p[0] * s, p[1], p[2])


def arm(side, upper, fore=(0, 0, 0), hand=(0, 0, 0)):
    return {f"upper_arm_{side}": upper, f"forearm_{side}": fore, f"hand_{side}": hand}


def legs(tr=0, sr=0, tl=0, sl=0, drop=0.0, twist=0, fwd=0.0, sway=0):
    return {
        "thigh_R": (tr, 0, 0), "shin_R": (sr, 0, 0), "foot_R": (-(tr + sr), 0, 0),
        "thigh_L": (tl, 0, 0), "shin_L": (sl, 0, 0), "foot_L": (-(tl + sl), 0, 0),
        "hips": {"r": (0, sway, twist), "t": (0, -fwd, drop)},
    }


def add(pose, **parts):
    """Adds rotations (in degrees) to some pieces of a pose."""
    out = dict(pose)
    for k, d in parts.items():
        v = out.get(k, (0, 0, 0))
        if isinstance(v, dict):
            out[k] = {"r": tuple(a + b for a, b in zip(v["r"], d)), "t": v["t"]}
        else:
            out[k] = tuple(a + b for a, b in zip(v, d))
    return out


def lift(pose, part, dt):
    """Offsets a piece (translation, in metres)."""
    out = dict(pose)
    v = out.get(part, (0, 0, 0))
    r, t = (v["r"], v["t"]) if isinstance(v, dict) else (v, (0, 0, 0))
    out[part] = {"r": r, "t": tuple(a + b for a, b in zip(t, dt))}
    return out


def cycle(base, n, frames, fn):
    """Loop of `n` keys over `frames` frames: `fn(pose, phase 0→1)`; the last one = the first."""
    keys = [(round(frames * i / n), fn(base, i / n)) for i in range(n)]
    keys.append((frames, keys[0][1]))
    return keys


def s(ph, k=1, off=0.0):
    return math.sin(2 * math.pi * (ph * k + off))


# ============================================================================= ash wyvern

def dragon():
    # Huge: its faces are split, otherwise the texture stretches and distorts on them.
    rig = start("dragon", cell=0.6)
    SCALE = material("d_scale", tex=tex_noise((0.24, 0.22, 0.22), 0.5, seed=101))
    SCALE_DARK = material("d_scale_dark", tex=tex_noise((0.12, 0.11, 0.11), 0.4, seed=102))
    BELLY = material("d_belly", tex=tex_noise((0.36, 0.28, 0.22), 0.45, seed=103))
    MEMBRANE = material("d_membrane", tex=tex_noise((0.34, 0.12, 0.08), 0.45, seed=104))
    HORN = material("d_horn", (0.74, 0.68, 0.58))
    EYES = material("d_eyes", (1.0, 0.6, 0.2), emissive=(1.0, 0.5, 0.1))
    # Orange, like its fire and its torch (assets/config/bosses.ron).
    EMBER = material("d_ember", (1.0, 0.48, 0.1), emissive=(1.0, 0.4, 0.05))

    def body(mb):
        mb.box((0, -0.2, 2.05), (1.8, 3.4, 1.5), SCALE, taper=(0.8, 0.85))
        mb.box((0, -1.4, 2.15), (2.0, 1.4, 1.7), SCALE, taper=(0.75, 0.8))
        mb.box((0, 1.3, 2.0), (1.6, 1.3, 1.4), SCALE, taper=(0.8, 0.8))
        mb.box((0, -0.4, 1.32), (1.2, 3.4, 0.3), BELLY)
        for i in range(8):
            y = -1.8 + i * 0.5
            h = 0.5 - abs(i - 2) * 0.04
            mb.box((0, y, 2.95 + h / 2), (0.12, 0.3, h), HORN, taper=(0.2, 0.3), shift_top=(0, 0.12))
        # Ember cracks along the flanks.
        for sd in (R, L):
            for y, z, ln in ((-1.2, 2.3, 0.6), (-0.2, 1.9, 0.8), (0.9, 2.2, 0.5)):
                mb.box((sd * 0.93, y, z), (0.04, ln, 0.06), EMBER)

    def neck1(mb):
        mb.seg((0, -1.9, 2.5), (0, -2.95, 3.3), 0.9, 0.95, SCALE, taper=0.85)
        mb.box((0, -2.4, 3.4), (0.1, 0.25, 0.35), HORN, taper=(0.2, 0.3), shift_top=(0, 0.1))

    def neck2(mb):
        # Clearly thinner than the end of neck1: at equal thickness, their sides
        # overlap at the joint and the texture flickers there.
        mb.seg((0, -2.85, 3.25), (0, -3.65, 3.9), 0.66, 0.7, SCALE, taper=0.85)
        mb.box((0, -3.2, 4.0), (0.08, 0.2, 0.3), HORN, taper=(0.2, 0.3), shift_top=(0, 0.1))

    def head(mb):
        mb.box((0, -3.95, 3.95), (0.75, 0.85, 0.62), SCALE, taper=(0.9, 0.85))
        mb.box((0, -4.75, 3.9), (0.55, 0.95, 0.4), SCALE_DARK, taper=(0.8, 0.85))
        for sd in (R, L):
            mb.box((sd * 0.24, -4.15, 4.27), (0.2, 0.45, 0.12), SCALE_DARK)
            # Protruding eyes (not in the plane of the cheek: they would flicker there).
            mb.box((sd * 0.41, -4.22, 4.06), (0.08, 0.18, 0.08), EYES)
            mb.seg((sd * 0.22, -3.75, 4.2), (sd * 0.5, -2.9, 4.8), 0.17, 0.17, HORN, taper=0.15)
            mb.seg((sd * 0.34, -3.85, 4.0), (sd * 0.75, -3.35, 4.1), 0.1, 0.1, HORN, taper=0.2)
            mb.box((sd * 0.12, -5.24, 3.98), (0.07, 0.03, 0.05), EMBER)
            for y in (-4.45, -4.75, -5.05):
                mb.box((sd * 0.2, y, 3.66), (0.05, 0.05, 0.1), HORN)
        mb.box((0, -4.6, 3.68), (0.4, 0.8, 0.02), EMBER)

    def jaw(mb):
        mb.box((0, -4.55, 3.56), (0.5, 1.25, 0.2), SCALE_DARK, taper=(0.85, 0.9))
        for sd in (R, L):
            for y in (-4.3, -4.65, -5.0):
                mb.box((sd * 0.18, y, 3.7), (0.05, 0.05, 0.09), HORN)

    def tail(a, b, w, spike=True, end=False):
        def geo(mb):
            mb.seg(a, b, w, w * 0.92, SCALE, taper=0.7)
            if spike:
                m = tuple((p + q) / 2 for p, q in zip(a, b))
                mb.box((0, m[1], m[2] + w * 0.5), (0.08, 0.22, 0.3), HORN, taper=(0.2, 0.3), shift_top=(0, 0.1))
            if end:
                mb.seg(b, (0, b[1] + 0.7, b[2] - 0.05), 0.7, 0.08, HORN, taper=0.05)
        return geo

    rig.part("body", (0, 0, 2.0), build=body)
    rig.part("neck1", (0, -1.95, 2.55), "body", build=neck1)
    rig.part("neck2", (0, -2.9, 3.28), "neck1", build=neck2)
    rig.part("head", (0, -3.6, 3.88), "neck2", build=head)
    rig.part("jaw", (0, -3.8, 3.68), "head", build=jaw)
    rig.part("tail1", (0, 1.8, 2.2), "body", build=tail((0, 1.7, 2.2), (0, 3.4, 1.75), 1.0))
    rig.part("tail2", (0, 3.4, 1.75), "tail1", build=tail((0, 3.35, 1.75), (0, 5.0, 1.2), 0.72))
    rig.part("tail3", (0, 5.0, 1.2), "tail2", build=tail((0, 4.95, 1.2), (0, 6.5, 0.75), 0.5, spike=False, end=True))

    def foot(mb, x, y, fwd):
        mb.box((x, y, 0.1), (0.48, 0.62, 0.2), SCALE_DARK)
        for dx in (-0.15, 0, 0.15):
            mb.seg((x + dx, y - 0.28 * fwd, 0.14), (x + dx * 1.3, y - 0.52 * fwd, 0.02), 0.08, 0.08, HORN, taper=0.2)

    for sd_name, sd in SIDES:
        x = 0.95 * sd

        def fup(mb, x=x):
            mb.seg((x, -1.3, 2.3), (x * 1.1, -1.45, 1.0), 0.6, 0.62, SCALE, taper=0.75)

        def flow(mb, x=x):
            mb.seg((x * 1.1, -1.45, 1.05), (x * 1.1, -1.35, 0.2), 0.38, 0.4, SCALE_DARK, taper=0.9)
            foot(mb, x * 1.1, -1.55, 1)

        def hup(mb, x=x):
            mb.box((x * 1.05, 1.25, 1.65), (0.62, 1.05, 1.15), SCALE, taper=(0.8, 0.7))

        def hlow(mb, x=x):
            mb.seg((x * 1.1, 1.55, 1.1), (x * 1.1, 1.3, 0.2), 0.4, 0.42, SCALE_DARK, taper=0.9)
            foot(mb, x * 1.1, 1.15, 1)

        rig.part(f"fleg_{sd_name}", (x, -1.3, 2.1), "body", build=fup)
        rig.part(f"fleg_low_{sd_name}", (x * 1.1, -1.45, 1.0), f"fleg_{sd_name}", build=flow)
        rig.part(f"hleg_{sd_name}", (x, 1.3, 2.1), "body", build=hup)
        rig.part(f"hleg_low_{sd_name}", (x * 1.1, 1.5, 1.05), f"hleg_{sd_name}", build=hlow)

        shoulder, elbow, wrist = mx((0.6, -1.0, 2.85), sd), mx((2.3, -0.6, 4.0), sd), mx((3.8, 0.3, 4.5), sd)
        back, drop = mx((0.6, 1.5, 2.75), sd), mx((2.2, 1.1, 2.9), sd)
        tips = [mx(p, sd) for p in ((5.7, 0.9, 3.3), (5.0, 2.1, 2.5), (3.5, 2.5, 2.3))]

        def wing(mb, shoulder=shoulder, elbow=elbow, back=back, drop=drop):
            mb.seg(shoulder, elbow, 0.32, 0.32, SCALE, taper=0.8)
            sheet(mb, [shoulder, elbow, drop, back], MEMBRANE)

        def wing_fore(mb, elbow=elbow, wrist=wrist, tips=tips, drop=drop, sd=sd):
            mb.seg(elbow, wrist, 0.24, 0.24, SCALE, taper=0.8)
            mb.seg(wrist, mx((4.0, -0.15, 4.85), sd), 0.1, 0.1, HORN, taper=0.15)
            for t in tips:
                mb.seg(wrist, t, 0.1, 0.1, SCALE_DARK, taper=0.4)
            sheet(mb, [wrist, tips[0], tips[1]], MEMBRANE)
            sheet(mb, [wrist, tips[1], tips[2]], MEMBRANE)
            sheet(mb, [wrist, tips[2], drop, elbow], MEMBRANE)

        rig.part(f"wing_{sd_name}", shoulder, "body", build=wing)
        rig.part(f"wing_fore_{sd_name}", elbow, f"wing_{sd_name}", build=wing_fore)

    lock(rig, "lock_head", (0, -4.4, 3.95), "head")
    for sd_name, sd in SIDES:
        lock(rig, f"lock_fleg_{sd_name}", (sd * 1.05, -1.45, 1.0), f"fleg_low_{sd_name}")
        lock(rig, f"lock_hleg_{sd_name}", (sd * 1.05, 1.4, 1.1), f"hleg_low_{sd_name}")
    lock(rig, "lock_tail", (0, 4.3, 1.45), "tail2")

    # Head high, wings half spread: it looms.
    BASE = {
        "neck1": (-22, 0, 0), "neck2": (-12, 0, 0), "head": (30, 0, 0), "jaw": (6, 0, 0),
        "wing_R": (0, 16, 8), "wing_L": (0, -16, -8), "wing_fore_R": (0, 10, 0), "wing_fore_L": (0, -10, 0),
        "fleg_R": (-6, 0, 0), "fleg_low_R": (6, 0, 0), "fleg_L": (-6, 0, 0), "fleg_low_L": (6, 0, 0),
        "tail1": (6, 0, 0), "tail2": (-4, 0, 0),
    }

    def idle(p, ph):
        b = s(ph)
        look = s(ph, 1, 0.1)
        out = lift(p, "body", (0, 0, 0.05 * b))
        out = add(out, body=(1.5 * b, 0, 0), neck1=(-3 * b, 0, 8 * look), neck2=(0, 0, 10 * look),
                  head=(4 * s(ph, 2), -6 * look, 8 * look), jaw=(10 * max(0.0, s(ph, 2, 0.25)), 0, 0),
                  tail1=(2 * b, 0, 6 * s(ph, 1, 0.3)), tail2=(3 * b, 0, 10 * s(ph, 1, 0.45)),
                  tail3=(4 * b, 0, 16 * s(ph, 1, 0.6)),
                  wing_R=(0, 5 * b, 4 * b), wing_L=(0, -5 * b, -4 * b),
                  wing_fore_R=(0, 6 * b, 0), wing_fore_L=(0, -6 * b, 0))
        return out

    def walk(p, ph):
        a = s(ph)
        out = add(p, fleg_R=(-20 * a, 0, 0), fleg_low_R=(22 * max(0.0, s(ph, 1, 0.75)), 0, 0),
                  fleg_L=(20 * a, 0, 0), fleg_low_L=(22 * max(0.0, s(ph, 1, 0.25)), 0, 0),
                  hleg_R=(20 * a, 0, 0), hleg_low_R=(-18 * max(0.0, s(ph, 1, 0.25)), 0, 0),
                  hleg_L=(-20 * a, 0, 0), hleg_low_L=(-18 * max(0.0, s(ph, 1, 0.75)), 0, 0),
                  body=(0, 2 * a, 3 * a), neck1=(0, 0, -5 * a), head=(0, 0, -3 * a),
                  tail1=(0, 0, 8 * a), tail2=(0, 0, 10 * s(ph, 1, 0.15)), tail3=(0, 0, 14 * s(ph, 1, 0.3)),
                  wing_R=(0, 3 * a, 0), wing_L=(0, 3 * a, 0))
        return lift(out, "body", (0, 0, -0.08 * abs(s(ph, 2))))

    def body_lift(p, z):
        return lift(p, "body", (0, 0, z))

    # Headbutt: it rears its head, then slams it down to the ground in front of it.
    REAR = add(BASE, neck1=(-20, 0, 0), neck2=(-12, 0, 0), head=(10, 0, 0), jaw=(8, 0, 0), body=(-6, 0, 0))
    BUTT = body_lift(add(BASE, body=(10, 0, 0), neck1=(56, 0, 0), neck2=(26, 0, 0), head=(-30, 0, 0), jaw=(10, 0, 0),
                         fleg_R=(-16, 0, 0), fleg_L=(-16, 0, 0), fleg_low_R=(10, 0, 0), fleg_low_L=(10, 0, 0),
                         wing_R=(0, 12, 0), wing_L=(0, -12, 0)), -0.25)
    headbutt = [(0, BASE), (30, REAR), ("h0-8", add(REAR, neck1=(-8, 0, 0), head=(6, 0, 0))),
                ("h0-2", tween(REAR, BUTT, 0.7)), ("h0", BUTT), ("h0e+20", add(BUTT, head=(4, 0, 0))), ("T", BASE)]

    # Pivot: the target is on its flank (`sd` = 1 left, -1 right). The game turns it
    # a quarter turn the other way; the tail, cocked on the other side, whips the targeted flank.
    def pivot(sd):
        def pose(z1, z2, z3, look, lean=0):
            return add(BASE, body=(0, lean * sd, 0), tail1=(0, 0, z1 * sd), tail2=(0, 0, z2 * sd), tail3=(0, 0, z3 * sd),
                       neck1=(0, 0, look * sd), head=(0, 0, look * 0.6 * sd))
        return [(0, BASE), ("h0-22", pose(30, 15, 10, 25)), ("h0-2", pose(38, 22, 14, 30, 6)),
                ("h0+12", pose(-10, -20, -12, 12)), ("h0e", pose(-45, -32, -26, -8, -6)),
                ("h0e+16", pose(-30, -25, -20, 0)), ("T", BASE)]

    # Tail swipe: it starts from its right and sweeps the whole rear.
    def tail_pose(z1, z2, z3, body_z=0, look=0):
        return add(BASE, body=(0, 0, body_z), tail1=(0, 0, z1), tail2=(0, 0, z2), tail3=(0, 0, z3),
                   neck1=(0, 0, look), head=(0, 0, look * 0.6))
    tail_sweep = [(0, BASE), ("h0-18", tail_pose(40, 20, 10, 12, -22)), ("h0", tail_pose(60, 26, 16, 16, -26)),
                  ("h0+11", tail_pose(0, 20, 30, 0, -10)), ("h0e", tail_pose(-60, -26, -12, -16, 18)),
                  ("h0e+16", tail_pose(-45, -35, -30, -10, 12)), ("T", BASE)]

    # Breath: it inhales, head high, then spits a stream of fire that sweeps slowly.
    INHALE = body_lift(add(BASE, neck1=(-26, 0, 0), neck2=(-18, 0, 0), head=(-16, 0, 0), body=(-6, 0, 0),
                           wing_R=(0, 14, 0), wing_L=(0, -14, 0)), 0.15)
    AIM = add(BASE, neck1=(16, 0, 0), neck2=(8, 0, 0), head=(-12, 0, 0), jaw=(48, 0, 0), body=(4, 0, 0))
    breath = [(0, BASE), (34, INHALE), ("c0-6", add(INHALE, neck1=(-6, 0, 0))), ("c0", AIM),
              ("c0+30", add(AIM, neck2=(0, 0, -6), head=(0, 0, -6))), ("c0+60", add(AIM, neck2=(0, 0, 6), head=(0, 0, 6))),
              ("c0+78", add(AIM, jaw=(-44, 0, 0))), ("T", BASE)]

    # Rear up: it rises on its hind legs and comes down with all its weight.
    REAR_UP = body_lift(add(BASE, body=(-28, 0, 0), fleg_R=(-50, 0, 0), fleg_L=(-40, 0, 0), fleg_low_R=(50, 0, 0),
                            fleg_low_L=(40, 0, 0), hleg_R=(28, 0, 0), hleg_L=(28, 0, 0), neck1=(-14, 0, 0), head=(20, 0, 0),
                            jaw=(30, 0, 0), wing_R=(0, 34, 10), wing_L=(0, -34, -10), tail1=(-16, 0, 0)), 0.7)
    SLAM = body_lift(add(BASE, body=(8, 0, 0), fleg_R=(-22, 0, 0), fleg_L=(-22, 0, 0), neck1=(18, 0, 0), head=(-12, 0, 0),
                         jaw=(30, 0, 0), wing_R=(0, -12, 0), wing_L=(0, 12, 0), hleg_R=(-6, 0, 0), hleg_L=(-6, 0, 0)), -0.2)
    # Body slam: rearing up, it lets itself fall full length in front of it.
    FLOP = body_lift(add(BASE, body=(12, 0, 0), fleg_R=(-70, 0, 0), fleg_L=(-70, 0, 0), fleg_low_R=(20, 0, 0),
                         fleg_low_L=(20, 0, 0), hleg_R=(30, 0, 0), hleg_L=(30, 0, 0), hleg_low_R=(-30, 0, 0),
                         hleg_low_L=(-30, 0, 0), neck1=(40, 0, 0), neck2=(10, 0, 0), head=(-20, 0, 0), jaw=(24, 0, 0),
                         wing_R=(0, -30, 0), wing_L=(0, 30, 0), tail1=(-10, 0, 0)), -0.9)
    belly_flop = [(0, BASE), (30, tween(BASE, REAR_UP, 0.5)), ("h0-34", REAR_UP),
                  ("h0-14", body_lift(add(REAR_UP, body=(-4, 0, 0)), 0.2)), ("h0", FLOP), ("h0e+30", FLOP),
                  ("T-24", tween(FLOP, BASE, 0.5)), ("T", BASE)]

    # Take-off: crouching, wing beats, dive and impact.
    CROUCH = body_lift(add(BASE, fleg_low_R=(30, 0, 0), fleg_low_L=(30, 0, 0), hleg_low_R=(-30, 0, 0),
                           hleg_low_L=(-30, 0, 0), neck1=(10, 0, 0), wing_R=(0, 40, 0), wing_L=(0, -40, 0)), -0.5)
    AIR = add(BASE, fleg_R=(-30, 0, 0), fleg_low_R=(60, 0, 0), fleg_L=(-30, 0, 0), fleg_low_L=(60, 0, 0),
              hleg_R=(40, 0, 0), hleg_low_R=(-40, 0, 0), hleg_L=(40, 0, 0), hleg_low_L=(-40, 0, 0), tail1=(-10, 0, 0))
    UP = {"wing_R": (0, 70, 10), "wing_L": (0, -70, -10), "wing_fore_R": (0, 30, 0), "wing_fore_L": (0, -30, 0)}
    DOWN = {"wing_R": (0, -20, -10), "wing_L": (0, 20, 10), "wing_fore_R": (0, -25, 0), "wing_fore_L": (0, 25, 0)}
    DIVE = add(AIR, body=(16, 0, 0), neck1=(20, 0, 0), head=(-10, 0, 0), jaw=(30, 0, 0))
    LAND = body_lift(add(SLAM, wing_R=(0, 40, 0), wing_L=(0, -40, 0)), -0.3)
    fly_slam = [(0, BASE), (36, CROUCH), (54, body_lift(merge(CROUCH, UP), -0.7)),
                (64, body_lift(merge(AIR, DOWN), 2.5)), (74, body_lift(merge(AIR, UP), 5.0)),
                (84, body_lift(merge(AIR, DOWN), 6.5)), (94, body_lift(merge(AIR, UP), 6.8)),
                (104, body_lift(merge(DIVE, DOWN), 4.0)), ("h0-4", body_lift(merge(DIVE, UP), 0.8)),
                ("h0", LAND), ("h0e+30", LAND), ("T", BASE)]

    # Fire flight: it rises, hovers out of reach flapping its wings and spits three
    # fireballs at its target, then lands.
    HIGH = 6.5
    SPIT = add(AIR, neck1=(34, 0, 0), neck2=(16, 0, 0), head=(-6, 0, 0), jaw=(46, 0, 0), body=(10, 0, 0))
    casts = TIMINGS.get("dragon", {}).get("fly_fire", {}).get("casts", [])
    fly_fire = [(0, BASE), (30, CROUCH), (46, body_lift(merge(CROUCH, UP), -0.7)), (56, body_lift(merge(AIR, DOWN), 2.5))]
    for k, f in enumerate(range(66, 190, 10)):
        if all(abs(f - c) > 10 for c in casts):
            fly_fire.append((f, body_lift(merge(AIR, UP if k % 2 else DOWN), min(HIGH, 2.5 + (f - 56) * 0.12))))
    for c in range(len(casts)):
        fly_fire += [(f"c{c}-8", body_lift(merge(add(AIR, neck1=(-14, 0, 0), jaw=(10, 0, 0)), UP), HIGH)),
                     (f"c{c}", body_lift(merge(SPIT, DOWN), HIGH)), (f"c{c}+8", body_lift(merge(SPIT, UP), HIGH))]
    fly_fire += [(196, body_lift(merge(AIR, DOWN), HIGH - 0.5)), (206, body_lift(merge(AIR, UP), 3.0)),
                 (214, body_lift(merge(AIR, DOWN), 0.6)), (220, LAND), ("T", BASE)]

    # Ash rain: it roars at the sky.
    SKY = body_lift(add(BASE, neck1=(-40, 0, 0), neck2=(-30, 0, 0), head=(-30, 0, 0), jaw=(52, 0, 0), body=(-10, 0, 0),
                        wing_R=(0, 44, 0), wing_L=(0, -44, 0), wing_fore_R=(0, 24, 0), wing_fore_L=(0, -24, 0)), 0.3)
    fire_rain = [(0, BASE), (30, tween(BASE, SKY, 0.6)), ("c0-6", SKY), ("c0+40", add(SKY, head=(0, 0, 12))), ("T", BASE)]
    ROAR = add(BASE, neck1=(-6, 0, 0), neck2=(-8, 0, 0), head=(-24, 0, 0), jaw=(56, 0, 0),
               wing_R=(0, 40, 14), wing_L=(0, -40, -14), wing_fore_R=(0, 24, 0), wing_fore_L=(0, -24, 0))
    roar = [(0, BASE), (24, add(BASE, neck1=(-24, 0, 0), head=(-10, 0, 0))), (40, ROAR),
            ("T-30", add(ROAR, neck2=(0, 0, 14), head=(0, 0, 10))), ("T", BASE)]

    # Stagger: it collapses on its belly.
    FLOOR = body_lift(add(BASE, body=(4, 0, 0), fleg_R=(-50, 0, 0), fleg_low_R=(80, 0, 0), fleg_L=(-50, 0, 0),
                          fleg_low_L=(80, 0, 0), hleg_R=(40, 0, 0), hleg_low_R=(-60, 0, 0), hleg_L=(40, 0, 0),
                          hleg_low_L=(-60, 0, 0), neck1=(50, 0, 0), neck2=(18, 0, 0), head=(-30, 0, 0), jaw=(20, 0, 0),
                          wing_R=(0, -36, 0), wing_L=(0, 36, 0), tail1=(14, 0, 0)), -1.0)
    groggy = [(0, BASE), (20, FLOOR), (110, add(FLOOR, head=(4, 0, 10))), ("T-40", FLOOR),
              ("T-20", tween(FLOOR, BASE, 0.6)), ("T", BASE)]
    fatal = [(0, FLOOR), (40, FLOOR), (50, add(FLOOR, neck1=(-40, 0, 0), head=(-20, 0, 0), jaw=(30, 0, 0))),
             (80, add(FLOOR, neck1=(10, 0, 0))), ("T-20", tween(FLOOR, BASE, 0.6)), ("T", BASE)]
    DEAD = lift(add(FLOOR, body=(0, 70, 0), neck1=(-10, 0, 30), head=(0, 0, 20), jaw=(30, 0, 0),
                    wing_R=(0, -20, 0), wing_L=(0, 60, 0), tail1=(0, 0, 20), tail2=(0, 0, 20)), "body", (0.6, 0, -0.3))
    death = [(0, BASE), (30, FLOOR), (90, add(FLOOR, neck1=(-50, 0, 0), head=(-10, 0, 0), jaw=(40, 0, 0))),
             (140, DEAD), ("T", DEAD)]

    finish(rig, "dragon", cycle(BASE, 8, 280, idle), [
        ("walk", cycle(BASE, 8, 96, walk), True), ("headbutt", headbutt), ("pivot_L", pivot(1)), ("pivot_R", pivot(-1)),
        ("tail_sweep", tail_sweep), ("breath", breath), ("belly_flop", belly_flop), ("fly_slam", fly_slam), ("fly_fire", fly_fire), ("fire_rain", fire_rain), ("roar", roar),
        ("groggy", groggy), ("fatal_received", fatal), ("death", death),
    ])


# ============================================================================= horned butcher

def horned_butcher():
    rig = start("horned_butcher")
    SKIN = material("c_skin", tex=tex_noise((0.36, 0.17, 0.13), 0.45, seed=121))
    SKIN_DARK = material("c_skin_dark", tex=tex_noise((0.22, 0.1, 0.08), 0.4, seed=122))
    FUR = material("c_fur", tex=tex_noise((0.2, 0.15, 0.11), 0.6, seed=123))
    FUR_DARK = material("c_fur_dark", tex=tex_noise((0.1, 0.08, 0.06), 0.5, seed=128))
    HIDE = material("c_hide", tex=tex_noise((0.3, 0.22, 0.14), 0.5, seed=124))
    SKULL = material("c_skull", (0.85, 0.8, 0.68))
    HORN = material("c_horn", tex=tex_stripes((0.32, 0.27, 0.22), (0.22, 0.18, 0.15), n=8, seed=125))
    BLADE = material("c_blade", tex=tex_noise((0.42, 0.38, 0.35), 0.55, seed=126))
    BLOOD = material("c_blood", (0.33, 0.04, 0.03))
    WOOD = material("c_wood", tex=tex_planks((0.3, 0.2, 0.12), seed=127))
    # Bright red, like his torch and his warnings (assets/config/bosses.ron).
    EYES = material("c_eyes", (1.0, 0.2, 0.15), emissive=(1.0, 0.08, 0.06))
    MOUTH = material("c_mouth", (0.12, 0.04, 0.04))

    def hips(mb):
        mb.box((0, 0, 1.8), (0.8, 0.5, 0.4), SKIN)
        mb.box((0, -0.03, 1.45), (0.86, 0.6, 0.62), HIDE, taper=(0.86, 0.9))
        mb.box((0, -0.32, 1.22), (0.42, 0.04, 0.7), HIDE, taper=(1.2, 1))
        mb.box((0, 0, 1.78), (0.84, 0.54, 0.1), FUR)

    def chest(mb):
        mb.box((0, 0, 2.5), (1.1, 0.7, 1.0), SKIN, taper=(1.3, 1.15))
        mb.box((0, 0.06, 3.02), (1.3, 0.6, 0.25), SKIN, taper=(0.6, 0.8))
        mb.box((0, -0.33, 2.25), (0.6, 0.12, 0.42), SKIN_DARK)
        mb.seg((-0.62, -0.42, 2.92), (0.52, -0.42, 2.08), 0.12, 0.05, HIDE)
        # Trophy: a small skull hanging from the strap.
        mb.box((0.36, -0.47, 2.15), (0.14, 0.12, 0.16), SKULL)

    def head(mb):
        mb.box((0, -0.1, 3.18), (0.36, 0.36, 0.26), SKIN)
        mb.box((0, -0.3, 3.42), (0.42, 0.46, 0.42), SKULL, taper=(0.8, 0.9))
        # Skull muzzle: jaw, dark recessed maw, fangs.
        mb.box((0, -0.68, 3.3), (0.3, 0.46, 0.26), SKULL, taper=(0.8, 0.85))
        mb.box((0, -0.64, 3.1), (0.26, 0.42, 0.12), SKULL, taper=(0.9, 0.9))
        mb.box((0, -0.63, 3.165), (0.2, 0.36, 0.06), MOUTH)
        for sd in (R, L):
            mb.box((sd * 0.06, -0.92, 3.36), (0.05, 0.03, 0.05), MOUTH)
            mb.box((sd * 0.09, -0.81, 3.16), (0.04, 0.04, 0.08), SKULL)
            mb.box((sd * 0.12, -0.545, 3.5), (0.09, 0.04, 0.06), EYES)
            pts = [(0.15, -0.2, 3.6), (0.45, -0.02, 3.98), (0.78, 0.24, 3.9), (0.9, 0.2, 3.52), (0.78, -0.06, 3.32)]
            w = 0.2
            for a, b in zip(pts, pts[1:]):
                mb.seg(mx(a, sd), mx(b, sd), w, w, HORN, taper=0.78)
                w *= 0.78
            mb.seg(mx((0.1, -0.15, 3.62), sd), mx((0.18, 0.15, 4.0), sd), 0.09, 0.09, HORN, taper=0.2)

    rig.part("hips", (0, 0, 1.75), build=hips)
    rig.part("chest", (0, 0, 2.0), "hips", build=chest)
    rig.part("head", (0, -0.1, 3.1), "chest", build=head)
    for sd_name, sd in SIDES:
        x = 0.75 * sd

        def up(mb, x=x):
            mb.box((x, 0.02, 3.0), (0.46, 0.46, 0.3), SKIN)
            mb.seg((x, 0, 3.0), (x * 1.03, 0, 2.25), 0.4, 0.4, SKIN, taper=0.85)

        def fore(mb, x=x):
            mb.seg((x * 1.03, 0, 2.25), (x * 1.03, 0, 1.6), 0.32, 0.32, SKIN, taper=0.85)
            mb.box((x * 1.03, 0, 1.8), (0.37, 0.37, 0.3), HIDE)

        def hand(mb, x=x):
            mb.box((x * 1.03, -0.03, 1.47), (0.25, 0.27, 0.26), SKIN_DARK)

        def cleaver(mb, x=x):
            hx = x * 1.03
            mb.seg((hx, 0.15, 1.45), (hx, -0.35, 1.45), 0.09, 0.09, WOOD)
            mb.box((hx, -1.0, 1.38), (0.06, 1.35, 0.48), BLADE, taper=(1, 1.2))
            mb.box((hx, -1.0, 1.15), (0.065, 1.25, 0.05), BLOOD)
            mb.box((hx, -0.34, 1.45), (0.14, 0.06, 0.2), BLADE)

        rig.part(f"upper_arm_{sd_name}", (x, 0, 2.98), "chest", build=up)
        rig.part(f"forearm_{sd_name}", (x * 1.03, 0, 2.25), f"upper_arm_{sd_name}", build=fore)
        rig.part(f"hand_{sd_name}", (x * 1.03, 0, 1.6), f"forearm_{sd_name}", build=hand)
        rig.part(f"cleaver_{sd_name}", (x * 1.03, 0, 1.45), f"hand_{sd_name}", build=cleaver)

        lx = 0.3 * sd

        def thigh(mb, x=lx):
            mb.box((x, -0.04, 1.3), (0.44, 0.47, 0.86), FUR, taper=(0.75, 0.8))

        def shin(mb, x=lx):
            mb.seg((x, 0, 0.92), (x, 0.26, 0.26), 0.27, 0.29, FUR, taper=0.75)

        def foot(mb, x=lx):
            mb.seg((x, 0.26, 0.28), (x, 0.02, 0.08), 0.2, 0.22, FUR_DARK, taper=0.9)
            mb.box((x, -0.06, 0.06), (0.25, 0.3, 0.12), HORN)

        rig.part(f"thigh_{sd_name}", (lx, 0, 1.72), "hips", build=thigh)
        rig.part(f"shin_{sd_name}", (lx, 0, 0.9), f"thigh_{sd_name}", build=shin)
        rig.part(f"foot_{sd_name}", (lx, 0.26, 0.26), f"shin_{sd_name}", build=foot)

    BASE = merge(legs(-22, 32, -8, 22, -0.1), {"chest": (22, 0, 4), "head": (-18, 0, -4)},
                 arm("R", (-18, 14, 0), (-42, 0, 0), (100, 0, 0)), arm("L", (-8, -16, 0), (-36, 0, 0), (96, 0, 0)))

    def idle(p, ph):
        b = s(ph)
        out = add(p, chest=(3 * b, 0, 3 * s(ph, 1, 0.3)), head=(-3 * b, 4 * s(ph, 2), 12 * s(ph, 1, 0.1)),
                  upper_arm_R=(-4 * b, 0, 0), upper_arm_L=(-3 * b, 0, 0), forearm_R=(-6 * s(ph, 1, 0.2), 0, 0),
                  forearm_L=(-5 * s(ph, 1, 0.6), 0, 0))
        # Twitch: the head twists briefly.
        if 0.55 < ph < 0.7:
            out = add(out, head=(8, 18, -10))
        return lift(out, "hips", (0, 0, 0.04 * b))

    lock(rig, "lock_head", (0, -0.35, 3.4), "head")

    # Diagonal cleavers: cocked above the shoulder, pulled outwards, he brings it down
    # across his body to the ground in front of him, on the opposite side.
    W_R = merge(BASE, legs(-14, 24, -12, 22, -0.06, twist=-10), {"chest": (0, 0, -36), "head": (-14, 0, 18)},
                arm("R", (-150, -48, 0), (-24, 0, 0), (40, 0, 0)))
    C_R = merge(BASE, legs(-34, 40, 0, 20, -0.16, fwd=0.15, twist=10), {"chest": (36, 0, 30), "head": (-26, 0, -16)},
                arm("R", (-58, -38, 0), (-6, 0, 0), (95, 0, 0)), arm("L", (-20, -16, 0), (-40, 0, 0), (96, 0, 0)))
    W_L = merge(C_R, legs(-12, 22, -14, 24, -0.06, twist=10), {"chest": (0, 0, 36), "head": (-14, 0, -18)},
                arm("L", (-150, 48, 0), (-24, 0, 0), (40, 0, 0)), arm("R", (-30, 14, 0), (-30, 0, 0), (100, 0, 0)))
    C_L = merge(BASE, legs(-6, 22, -30, 40, -0.16, fwd=0.15, twist=-10), {"chest": (36, 0, -30), "head": (-26, 0, 16)},
                arm("L", (-58, 38, 0), (-6, 0, 0), (95, 0, 0)), arm("R", (-30, 14, 0), (-30, 0, 0), (100, 0, 0)))
    W_R2 = merge(C_L, legs(-14, 24, -12, 22, -0.06, twist=-10), {"chest": (0, 0, -36)}, arm("R", (-150, -48, 0), (-24, 0, 0), (40, 0, 0)))
    double_chop = [(0, BASE), ("h0-16", W_R), ("h0-3", add(W_R, upper_arm_R=(-8, 0, 0))), ("h0", C_R), ("h0e+6", C_R),
                   ("h1-14", W_L), ("h1", C_L), ("h1e+14", C_L), ("T", BASE)]

    # Whirlwind: crouched and leaning, cleavers held low, at head height
    # (arms raised horizontally, they would pass well above his target).
    SPIN = merge(BASE, legs(-45, 70, 40, 70, -0.65), {"chest": (38, 0, 0), "head": (-30, 0, 0)},
                 arm("R", (-30, 55, 0), (-10, 0, 0), (40, 0, 0)), arm("L", (-30, -55, 0), (-10, 0, 0), (40, 0, 0)))

    def spun(p, z):
        return add(p, hips=(0, 0, z))
    whirl = [(0, BASE), ("h0-14", spun(SPIN, -50)), ("h0", spun(SPIN, -20)), ("h0+14", spun(SPIN, 180)),
             ("h0e", spun(SPIN, 380)), ("h0e+14", spun(SPIN, 400)), ("T", spun(BASE, 360))]

    HEAD_DOWN = {"chest": (40, 0, 0), "head": (50, 0, 0)}
    GORE_ARMS = merge(arm("R", (30, 20, 0), (-30, 0, 0), (80, 0, 0)), arm("L", (30, -20, 0), (-30, 0, 0), (80, 0, 0)))
    gore = [(0, BASE), (20, merge(BASE, legs(-20, 40, 25, 40, -0.25), HEAD_DOWN, GORE_ARMS)),
            (34, merge(BASE, legs(-30, 50, 30, 45, -0.3), HEAD_DOWN, GORE_ARMS))]
    for i, t in enumerate(range(40, 72, 8)):
        lg = legs(-45, 30, 35, 70, -0.12) if i % 2 == 0 else legs(35, 70, -45, 30, -0.12)
        gore.append((t, merge(BASE, lg, HEAD_DOWN, GORE_ARMS)))
    gore += [(80, merge(BASE, legs(-30, 40, 25, 30, -0.2), HEAD_DOWN, GORE_ARMS)), ("T", BASE)]

    BOTH_UP = merge(BASE, legs(-10, 20, 10, 24, -0.04), {"chest": (-20, 0, 0), "head": (-14, 0, 0)},
                    arm("R", (-170, 10, 0), (-14, 0, 0), (40, 0, 0)), arm("L", (-170, -10, 0), (-14, 0, 0), (40, 0, 0)))
    BOTH_DOWN = merge(BASE, legs(-46, 50, 30, 16, -0.32, fwd=0.25), {"chest": (46, 0, 0), "head": (-22, 0, 0)},
                      arm("R", (-56, 6, 0), (0, 0, 0), (100, 0, 0)), arm("L", (-56, -6, 0), (0, 0, 0), (100, 0, 0)))
    cleave = [(0, BASE), (30, BOTH_UP), ("h0-8", lift(BOTH_UP, "hips", (0, 0, 0.06))), ("h0", BOTH_DOWN),
              ("h0e+40", BOTH_DOWN), ("T", BASE)]

    THROW_UP = merge(BASE, {"chest": (-6, 0, -30), "head": (-10, 0, 20)}, arm("R", (-160, 30, 0), (-40, 0, 0), (30, 0, 0)))
    THROWN = merge(BASE, legs(-30, 30, 10, 20, -0.12, fwd=0.1), {"chest": (30, 0, 20), "head": (-20, 0, -10)},
                   arm("R", (-50, 0, 0), (-10, 0, 0), (60, 0, 0)))
    throw = [(0, BASE), ("c0-22", THROW_UP), ("c0-6", add(THROW_UP, upper_arm_R=(-10, 0, 0), chest=(0, 0, -6))),
             ("c0+2", THROWN), ("c0+30", THROWN), ("T", BASE)]

    SW_A = merge(BASE, legs(-20, 30, 10, 30, -0.2), {"chest": (12, 0, -55)},
                 arm("R", (-80, 20, 0), (-10, 0, 0), (60, 0, 0)), arm("L", (-80, -20, 0), (-10, 0, 0), (60, 0, 0)))
    SW_B = add(SW_A, chest=(0, 0, 110))
    frenzy = [(0, BASE), ("h0-10", W_R), ("h0", C_R), ("h1-10", W_L), ("h1", C_L), ("h2-10", W_R2), ("h2", C_R),
              ("h3-16", SW_A), ("h3", tween(SW_A, SW_B, 0.2)), ("h3e", SW_B), ("h3e+16", SW_B), ("T", BASE)]

    # Back leap: he crouches, jumps far back and lands heavily.
    CROUCH = merge(BASE, legs(-50, 75, -40, 70, -0.5), {"chest": (30, 0, 0), "head": (-20, 0, 0)})
    LEAP = lift(merge(BASE, legs(-20, 60, 10, 50, 0.0), {"chest": (-10, 0, 0), "head": (6, 0, 0)},
                      arm("R", (-40, 40, 0), (-20, 0, 0), (90, 0, 0)), arm("L", (-40, -40, 0), (-20, 0, 0), (90, 0, 0))),
                "hips", (0, 0, 1.0))
    leap_back = [(0, BASE), (12, CROUCH), (16, lift(CROUCH, "hips", (0, 0, -0.08))), (26, LEAP),
                 (36, lift(LEAP, "hips", (0, 0, -0.6))), (42, CROUCH), (54, CROUCH), ("T", BASE)]

    # Leap onto the gallery (or back down from it): the game raises him from one floor to the
    # other along the jump; the hips add the arc. Cleavers overhead, he crashes down with both.
    SOAR = merge(BASE, legs(-40, 80, -10, 60, 0.0), {"chest": (-16, 0, 0), "head": (-10, 0, 0)},
                 arm("R", (-170, 10, 0), (-14, 0, 0), (40, 0, 0)), arm("L", (-170, -10, 0), (-14, 0, 0), (40, 0, 0)))
    leap_slam = [(0, BASE), (28, CROUCH), (40, lift(CROUCH, "hips", (0, 0, -0.1))), (48, lift(SOAR, "hips", (0, 0, 1.2))),
                 (58, lift(SOAR, "hips", (0, 0, 1.8))), (68, lift(SOAR, "hips", (0, 0, 1.0))),
                 ("h0-2", lift(tween(SOAR, BOTH_DOWN, 0.5), "hips", (0, 0, 0.2))), ("h0", BOTH_DOWN), ("h0e+30", BOTH_DOWN),
                 ("T", BASE)]

    finish(rig, "horned_butcher", cycle(BASE, 10, 200, idle), [
        ("walk", biped_walk(BASE, 84, 22), True), ("double_chop", double_chop), ("whirl", whirl), ("gore", gore),
        ("leap_back", leap_back), ("leap_slam", leap_slam),
        ("cleave", cleave), ("throw", throw), ("frenzy", frenzy),
    ] + biped_states(BASE, 1.75))


# ============================================================================= the lamplighter

def lamplighter():
    rig = start("lamplighter")
    COAT = material("l_coat", tex=tex_noise((0.16, 0.18, 0.26), 0.4, seed=131))
    COAT_DARK = material("l_coat_dark", tex=tex_noise((0.09, 0.1, 0.14), 0.35, seed=132))
    PORCELAIN = material("l_mask", (0.9, 0.87, 0.8))
    HAT = material("l_hat", (0.08, 0.07, 0.08))
    BRASS = material("l_brass", (0.8, 0.62, 0.28))
    GLOVE = material("l_glove", (0.2, 0.15, 0.12))
    WOOD = material("l_wood", tex=tex_planks((0.35, 0.24, 0.15), seed=133))
    # Pale green, like its glows and its torch (assets/config/bosses.ron).
    LIGHT = material("l_light", (0.7, 1.0, 0.6), emissive=(0.4, 1.0, 0.35))
    EYES = material("l_eyes", (0.75, 1.0, 0.65), emissive=(0.45, 1.0, 0.4))

    def hips(mb):
        mb.box((0, 0, 1.55), (0.4, 0.26, 0.2), COAT)
        mb.box((0, 0.02, 1.05), (0.62, 0.44, 1.0), COAT, taper=(0.68, 0.62))
        mb.box((0, 0, 1.6), (0.42, 0.28, 0.05), BRASS)

    def chest(mb):
        mb.box((0, 0, 2.1), (0.46, 0.3, 0.75), COAT, taper=(1.25, 1.1))
        mb.box((0, 0.02, 2.56), (0.44, 0.36, 0.22), COAT_DARK, taper=(1.15, 1.0))
        for z in (1.85, 2.0, 2.15, 2.3):
            mb.box((0.07, -0.165, z), (0.035, 0.02, 0.035), BRASS)

    def head(mb):
        mb.cylinder((0, 0, 2.66), 0.06, 0.12, COAT_DARK, sides=6)
        mb.box((0, -0.02, 2.82), (0.22, 0.24, 0.3), PORCELAIN, taper=(0.85, 0.9))
        mb.seg((0, -0.13, 2.8), (0, -0.4, 2.73), 0.055, 0.055, PORCELAIN, taper=0.3)
        for sd in (R, L):
            mb.box((sd * 0.05, -0.142, 2.87), (0.05, 0.016, 0.014), EYES)
        mb.cylinder((0, 0, 2.97), 0.3, 0.03, HAT, sides=10)
        mb.cylinder((0, 0, 3.18), 0.14, 0.4, HAT, sides=8, radius_top=0.17)
        mb.cylinder((0, 0, 3.02), 0.145, 0.05, BRASS, sides=8)

    rig.part("hips", (0, 0, 1.5), build=hips)
    rig.part("chest", (0, 0, 1.72), "hips", build=chest)
    rig.part("head", (0, 0, 2.6), "chest", build=head)
    for sd_name, sd in SIDES:
        x = 0.31 * sd

        def up(mb, x=x):
            mb.seg((x, 0, 2.5), (x, 0, 2.0), 0.14, 0.15, COAT, taper=0.9)

        def fore(mb, x=x):
            mb.seg((x, 0, 2.0), (x, 0, 1.55), 0.12, 0.13, COAT, taper=0.9)
            mb.box((x, 0, 1.6), (0.15, 0.16, 0.1), COAT_DARK)

        def hand(mb, x=x):
            mb.box((x, -0.02, 1.46), (0.1, 0.12, 0.17), GLOVE)

        rig.part(f"upper_arm_{sd_name}", (x, 0, 2.5), "chest", build=up)
        rig.part(f"forearm_{sd_name}", (x, 0, 2.0), f"upper_arm_{sd_name}", build=fore)
        rig.part(f"hand_{sd_name}", (x, 0, 1.55), f"forearm_{sd_name}", build=hand)

        lx = 0.12 * sd

        def thigh(mb, x=lx):
            mb.box((x, 0, 1.15), (0.15, 0.17, 0.72), COAT_DARK, taper=(0.85, 0.85))

        def shin(mb, x=lx):
            mb.seg((x, 0, 0.8), (x, 0, 0.14), 0.12, 0.13, COAT_DARK, taper=0.9)

        def foot(mb, x=lx):
            mb.box((x, -0.07, 0.07), (0.15, 0.34, 0.14), HAT)

        rig.part(f"thigh_{sd_name}", (lx, 0, 1.5), "hips", build=thigh)
        rig.part(f"shin_{sd_name}", (lx, 0, 0.8), f"thigh_{sd_name}", build=shin)
        rig.part(f"foot_{sd_name}", (lx, 0, 0.14), f"shin_{sd_name}", build=foot)

    hx = 0.31 * R

    def pole(mb):
        # Lamplighter's pole: resting on the ground, hook and small lantern at the top.
        mb.cylinder((hx, -0.03, 1.85), 0.032, 3.7, WOOD, sides=6)
        mb.box((hx, -0.03, 3.72), (0.07, 0.07, 0.06), BRASS)
        mb.seg((hx, -0.03, 3.7), (hx, -0.3, 3.98), 0.04, 0.04, BRASS)
        mb.seg((hx, -0.3, 3.98), (hx, -0.32, 3.82), 0.04, 0.04, BRASS, taper=0.4)
        mb.box((hx, -0.03, 3.86), (0.13, 0.13, 0.18), LIGHT)
        mb.cylinder((hx, -0.03, 3.98), 0.1, 0.06, BRASS, sides=6, radius_top=0.02)

    lx = 0.31 * L

    def lantern(mb):
        mb.seg((lx, -0.02, 1.4), (lx, -0.02, 1.26), 0.02, 0.02, BRASS)
        mb.cylinder((lx, -0.02, 1.25), 0.1, 0.03, BRASS, sides=6)
        mb.box((lx, -0.02, 1.13), (0.15, 0.15, 0.2), LIGHT)
        mb.cylinder((lx, -0.02, 1.01), 0.1, 0.04, BRASS, sides=6)

    rig.part("pole", (hx, 0, 1.46), "hand_R", build=pole)
    rig.part("lantern", (lx, -0.02, 1.42), "hand_L", build=lantern)

    BASE = merge(legs(-4, 6, 2, 4, -0.02), {"chest": (4, 0, 0), "head": (-2, 0, 0)},
                 arm("R", (-28, 6, 0), (-34, 0, 0), (62, 0, 0)), arm("L", (-14, -8, 0), (-40, 0, 0), (54, 0, 0)))

    def idle(p, ph):
        b = s(ph)
        out = add(p, chest=(1.5 * b, 0, 0), head=(4 * s(ph, 1, 0.25), 14 * s(ph, 1, 0.1), 6 * s(ph, 1, 0.4)),
                  lantern=(10 * s(ph, 2), 0, 0), hand_L=(4 * s(ph, 2, 0.1), 0, 0))
        return lift(out, "hips", (0, 0, 0.015 * b))

    lock(rig, "lock_head", (0, -0.05, 2.8), "head")

    # Pole pointed at the target (the top of the pole tilts forward).
    def point(p, z=0, up=-75):
        return merge(p, {"chest": (6, 0, z * 0.6), "head": (-4, 0, z * 0.3)}, arm("R", (up, 0, z * 0.4), (-10, 0, 0), (150, 0, 0)))
    RAISE = merge(BASE, {"chest": (-10, 0, -10), "head": (-8, 0, 0)}, arm("R", (-150, 10, 0), (-10, 0, 0), (100, 0, 0)))
    bolt = [(0, BASE), ("c0-20", RAISE), ("c0-4", add(RAISE, upper_arm_R=(-10, 0, 0))),
            ("c0", point(merge(BASE, legs(-20, 20, 10, 10, -0.06, fwd=0.08)))), ("c0+26", point(BASE)), ("T", BASE)]
    volley = [(0, BASE), ("c0-22", RAISE), ("c0-6", add(RAISE, upper_arm_R=(-10, 0, 0))), ("c0", point(BASE)),
              ("c0+6", point(BASE, 0, -95)), ("c1", point(BASE)), ("c1+6", point(BASE, 0, -95)), ("c2", point(BASE)),
              ("c2+28", point(BASE)), ("T", BASE)]
    PLANT_UP = merge(BASE, legs(-6, 10, 4, 8, 0.0), {"chest": (-12, 0, 0), "head": (-16, 0, 0)},
                     arm("R", (-150, 0, 0), (-20, 0, 0), (170, 0, 0)))
    PLANTED = merge(BASE, legs(-30, 50, 20, 40, -0.2), {"chest": (24, 0, 0), "head": (-10, 0, 0)},
                    arm("R", (-50, 0, 0), (-10, 0, 0), (60, 0, 0)), arm("L", (-40, -30, 0), (-30, 0, 0), (40, 0, 0)))
    flares = [(0, BASE), ("c0-18", PLANT_UP), ("c0-4", lift(PLANT_UP, "hips", (0, 0, 0.05))), ("c0", PLANTED),
              ("c0+40", PLANTED), ("T", BASE)]

    def sweep_pose(z):
        return merge(BASE, legs(-24, 30, 14, 20, -0.12), {"chest": (12, 0, z), "head": (-6, 0, -z * 0.3)},
                     arm("R", (-80, 10, z * 0.3), (-6, 0, 0), (170, 0, 0)))
    pole_sweep = [(0, BASE), ("h0-16", sweep_pose(-60)), ("h0", sweep_pose(-40)), ("h0e", sweep_pose(55)),
                  ("h0e+12", sweep_pose(60)), ("T", BASE)]
    CROUCH = merge(BASE, legs(-60, 90, -45, 85, -0.5), {"chest": (30, 0, 0)})
    AIRBORNE = lift(merge(BASE, legs(-50, 90, -30, 80, 0.0), {"chest": (24, 0, 0), "head": (-10, 0, 0)},
                          arm("R", (-60, 10, 0), (-20, 0, 0), (62, 0, 0)), arm("L", (-60, -20, 0), (-20, 0, 0), (54, 0, 0))),
                    "hips", (0, 0, 0.7))
    blink = [(0, BASE), (8, CROUCH), (16, AIRBORNE), (26, lift(AIRBORNE, "hips", (0, 0, 0.2))), (34, CROUCH),
             (46, BASE), ("T", BASE)]
    # Glide: he slides sideways without walking, leaning, the lantern trailing behind him.
    # `sd` = 1 to his left, -1 to his right.
    def glide(sd):
        lean = merge(BASE, legs(-8, 20, -6, 18, 0.0, sway=10 * sd), {"chest": (10, 8 * sd, 0), "head": (-6, -10 * sd, 0)},
                     arm("L", (-40, -40, 0), (-20, 0, 0), (54, 0, 0)), arm("R", (-50, 20, 0), (-30, 0, 0), (62, 0, 0)))
        crouch = merge(BASE, legs(-30, 50, -24, 46, -0.25), {"chest": (18, 0, 0)})
        return [(0, BASE), (8, crouch), (14, lift(lean, "hips", (0, 0, 0.18))), (30, lift(add(lean, chest=(0, 4 * sd, 0)), "hips", (0, 0, 0.22))),
                (38, crouch), (48, BASE), ("T", BASE)]

    LANTERN_UP = merge(BASE, legs(-10, 16, 10, 16, -0.06), {"chest": (-16, 0, 0), "head": (-26, 0, 0)},
                       arm("L", (-175, -10, 0), (-6, 0, 0), (0, 0, 0)))
    nova = [(0, BASE), (22, LANTERN_UP), ("c0", add(LANTERN_UP, chest=(-6, 0, 0))), ("c1", add(LANTERN_UP, chest=(-8, 0, 6))),
            ("c1+30", LANTERN_UP), ("T", BASE)]

    finish(rig, "lamplighter", cycle(BASE, 10, 220, idle), [
        ("walk", biped_walk(BASE, 76, 18), True), ("bolt", bolt), ("volley", volley), ("flares", flares),
        ("pole_sweep", pole_sweep), ("blink", blink), ("nova", nova), ("glide_L", glide(1)), ("glide_R", glide(-1)),
    ] + biped_states(BASE, 1.5))


# ============================================================================= the anvil

def anvil():
    rig = start("anvil")
    SKIN = material("a_skin", tex=tex_noise((0.62, 0.45, 0.36), 0.35, seed=141))
    LEATHER = material("a_leather", tex=tex_noise((0.3, 0.2, 0.12), 0.5, seed=142))
    TROUSERS = material("a_trousers", tex=tex_noise((0.2, 0.18, 0.17), 0.35, seed=143))
    IRON = material("a_iron", tex=tex_noise((0.3, 0.3, 0.32), 0.45, seed=144))
    DARK = material("a_dark", (0.06, 0.05, 0.05))
    WOOD = material("a_wood", tex=tex_planks((0.32, 0.22, 0.14), seed=145))
    EMBER = material("a_ember", (0.6, 1.0, 0.5), emissive=(0.35, 1.0, 0.3))

    def hips(mb):
        mb.box((0, 0, 1.12), (1.1, 0.8, 0.5), TROUSERS, taper=(1.05, 1.05))

    def chest(mb):
        mb.box((0, -0.1, 1.85), (1.5, 1.3, 1.1), SKIN, taper=(0.85, 0.85))
        mb.box((0, 0, 2.5), (1.4, 0.95, 0.42), SKIN, taper=(0.75, 0.85))
        mb.box((0, -0.79, 1.55), (0.95, 0.06, 1.3), LEATHER)
        mb.box((0, -0.35, 2.1), (1.55, 0.95, 0.08), LEATHER)
        mb.box((0, -0.82, 1.3), (0.3, 0.04, 0.25), EMBER)
        for sd in (R, L):
            mb.seg((sd * 0.45, -0.75, 2.2), (sd * 0.5, -0.45, 2.65), 0.1, 0.05, LEATHER)

    def head(mb):
        mb.box((0, -0.12, 2.86), (0.42, 0.44, 0.4), SKIN)
        mb.cylinder((0, -0.12, 3.02), 0.27, 0.32, IRON, sides=8, radius_top=0.2)
        mb.box((0, -0.36, 2.88), (0.3, 0.05, 0.2), DARK)
        for sd in (R, L):
            mb.box((sd * 0.07, -0.385, 2.92), (0.06, 0.02, 0.03), EMBER)

    rig.part("hips", (0, 0, 1.2), build=hips)
    rig.part("chest", (0, 0, 1.35), "hips", build=chest)
    rig.part("head", (0, -0.08, 2.7), "chest", build=head)
    for sd_name, sd in SIDES:
        x = 0.88 * sd

        def up(mb, x=x):
            mb.box((x, 0, 2.66), (0.6, 0.7, 0.35), IRON, taper=(0.7, 0.8))
            mb.seg((x, 0, 2.6), (x * 1.05, 0, 1.95), 0.44, 0.44, SKIN, taper=0.88)

        def fore(mb, x=x):
            mb.seg((x * 1.05, 0, 1.95), (x * 1.05, 0, 1.35), 0.4, 0.4, SKIN, taper=0.9)
            mb.box((x * 1.05, 0, 1.5), (0.44, 0.44, 0.3), LEATHER)

        def hand(mb, x=x):
            mb.box((x * 1.05, -0.02, 1.2), (0.34, 0.36, 0.32), LEATHER)

        rig.part(f"upper_arm_{sd_name}", (x, 0, 2.6), "chest", build=up)
        rig.part(f"forearm_{sd_name}", (x * 1.05, 0, 1.95), f"upper_arm_{sd_name}", build=fore)
        rig.part(f"hand_{sd_name}", (x * 1.05, 0, 1.35), f"forearm_{sd_name}", build=hand)

        lx = 0.34 * sd

        def thigh(mb, x=lx):
            mb.box((x, 0, 0.85), (0.5, 0.52, 0.6), TROUSERS, taper=(0.9, 0.9))

        def shin(mb, x=lx):
            mb.box((x, 0, 0.38), (0.42, 0.44, 0.5), LEATHER, taper=(1.1, 1.1))

        def foot(mb, x=lx):
            mb.box((x, -0.08, 0.08), (0.46, 0.62, 0.16), IRON)

        rig.part(f"thigh_{sd_name}", (lx, 0, 1.12), "hips", build=thigh)
        rig.part(f"shin_{sd_name}", (lx, 0, 0.6), f"thigh_{sd_name}", build=shin)
        rig.part(f"foot_{sd_name}", (lx, 0, 0.14), f"shin_{sd_name}", build=foot)

    hx = 0.88 * 1.05 * R

    def hammer(mb):
        # Hammer whose head is an anvil, held handle up, resting on the ground.
        mb.cylinder((hx, -0.02, 0.95), 0.07, 1.5, WOOD, sides=6)
        mb.box((hx, -0.02, 1.72), (0.12, 0.12, 0.1), IRON)
        mb.box((hx, -0.05, 0.4), (0.56, 1.0, 0.36), IRON, taper=(0.9, 1.15))
        mb.seg((hx, -0.6, 0.48), (hx, -1.05, 0.5), 0.34, 0.24, IRON, taper=0.15)
        mb.box((hx, 0.0, 0.12), (0.62, 0.7, 0.24), IRON, taper=(0.75, 0.75))
        mb.box((hx, -0.05, 0.6), (0.5, 0.9, 0.04), DARK)

    rig.part("hammer", (hx, 0, 1.2), "hand_R", build=hammer)

    BASE = merge(legs(-6, 10, 4, 6, -0.04), {"chest": (-4, 0, 6), "head": (8, 0, -6)},
                 arm("R", (-4, 8, 0), (-6, 0, 0), (10, 0, 0)), arm("L", (-20, -14, 0), (-50, 0, 0), (20, 0, 0)))

    def idle(p, ph):
        b = s(ph)
        out = lift(p, "chest", (0, 0, 0.04 * b))
        out = add(out, chest=(-2 * b, 0, 3 * s(ph, 1, 0.3)), head=(3 * b, 0, -14 * s(ph, 1, 0.15)),
                  upper_arm_L=(-6 * s(ph, 1, 0.2), 0, 0), forearm_L=(-8 * s(ph, 1, 0.3), 0, 0),
                  upper_arm_R=(0, 3 * b, 0))
        return lift(out, "hips", (0, 0, 0.02 * b))

    lock(rig, "lock_head", (0, -0.15, 2.9), "head")

    OVER = merge(BASE, legs(-8, 14, 12, 18, -0.02), {"chest": (-18, 0, 0), "head": (-10, 0, 0)},
                 arm("R", (-175, 10, 0), (-10, 0, 0), (0, 0, 0)), arm("L", (-160, -24, 0), (-20, 0, 0), (0, 0, 0)))
    SMASH = merge(BASE, legs(-40, 44, 26, 14, -0.22, fwd=0.2), {"chest": (32, 0, 0), "head": (-14, 0, 0)},
                  arm("R", (-55, 4, 0), (-6, 0, 0), (-12, 0, 0)), arm("L", (-50, -24, 0), (-10, 0, 0), (0, 0, 0)))
    hammer_slam = [(0, BASE), ("h0-30", OVER), ("h0-8", lift(OVER, "hips", (0, 0, 0.05))), ("h0", SMASH),
                   ("h0e+34", SMASH), ("T", BASE)]

    def swing(z, lean=6):
        return merge(BASE, legs(-24, 30, 14, 20, -0.12, twist=z * 0.3), {"chest": (lean, 0, z), "head": (0, 0, -z * 0.3)},
                     arm("R", (-80, 0, 0), (-4, 0, 0), (0, 0, 0)), arm("L", (-60, -20, z * 0.2), (-30, 0, 0), (0, 0, 0)))
    sweep = [(0, BASE), ("h0-18", swing(-75, 0)), ("h0", swing(-50)), ("h0e", swing(60)), ("h0e+16", swing(70, 12)), ("T", BASE)]

    SHOULDER = {"chest": (24, 0, -34), "head": (-14, 0, 30)}
    CH_ARMS = merge(arm("R", (-30, 30, 0), (-60, 0, 0), (0, 0, 0)), arm("L", (-50, -10, 0), (-70, 0, 0), (0, 0, 0)))
    shoulder_charge = [(0, BASE), (20, merge(BASE, legs(-20, 40, 25, 40, -0.25), SHOULDER, CH_ARMS)),
                       (32, merge(BASE, legs(-30, 50, 30, 45, -0.3), SHOULDER, CH_ARMS))]
    for i, t in enumerate(range(38, 68, 8)):
        lg = legs(-45, 30, 35, 70, -0.1) if i % 2 == 0 else legs(35, 70, -45, 30, -0.1)
        shoulder_charge.append((t, merge(BASE, lg, SHOULDER, CH_ARMS)))
    shoulder_charge += [(76, merge(BASE, legs(-30, 40, 25, 30, -0.2), SHOULDER, CH_ARMS)), ("T", BASE)]

    DEEP = merge(SMASH, legs(-60, 80, 40, 60, -0.45), {"chest": (40, 0, 0)})
    quake = [(0, BASE), (24, lift(OVER, "hips", (0, 0, 0.04))), (50, add(OVER, chest=(-10, 0, 0), head=(-10, 0, 0))),
             ("h0-8", lift(add(OVER, chest=(-14, 0, 0)), "hips", (0, 0, 0.12))), ("h0", DEEP), ("h0e+24", DEEP), ("T", BASE)]

    finish(rig, "anvil", cycle(BASE, 10, 200, idle), [
        ("walk", biped_walk(BASE, 90, 16), True), ("hammer_slam", hammer_slam), ("sweep", sweep),
        ("shoulder_charge", shoulder_charge), ("quake", quake),
    ] + biped_states(BASE, 1.2))


# ============================================================================= hollow-spined beast

def spine_beast():
    rig = start("spine_beast")
    FLESH = material("s_flesh", tex=tex_noise((0.52, 0.55, 0.58), 0.5, seed=161))
    FLESH_DARK = material("s_flesh_dark", tex=tex_noise((0.25, 0.27, 0.31), 0.5, seed=162))
    BONE = material("s_bone", tex=tex_noise((0.82, 0.8, 0.74), 0.3, seed=163))
    BONE_DARK = material("s_bone_dark", (0.5, 0.48, 0.44))
    GLOW = material("s_glow", (0.55, 0.85, 1.0), emissive=(0.4, 0.8, 1.0))
    MOUTH = material("s_mouth", (0.12, 0.04, 0.06))

    # Belly to the ground: the torso skims the floor, legs splayed like a lizard's.
    def torso(mb):
        mb.box((0, -0.7, 0.8), (1.0, 1.4, 1.0), FLESH, taper=(0.8, 0.85))
        mb.box((0, 0.6, 0.62), (0.7, 1.4, 0.7), FLESH_DARK, taper=(0.8, 0.8))
        mb.box((0, 1.4, 0.52), (0.85, 0.6, 0.7), FLESH)
        for sd in (R, L):
            for i in range(5):
                mb.box((sd * 0.48, -1.15 + i * 0.22, 0.65), (0.06, 0.09, 0.62), BONE)
            mb.box((sd * 0.36, 0.5, 0.65), (0.03, 0.9, 0.05), GLOW)
        for i in range(10):
            y = -1.35 + i * 0.33
            h = 0.35 + 0.55 * math.sin(math.pi * i / 9)
            z = 1.3 if i < 4 else 1.05
            mb.box((0, y, z + h / 2), (0.08, 0.12, h), BONE, taper=(0.25, 0.3), shift_top=(0, 0.18))

    def neck(mb):
        mb.seg((0, -1.35, 1.12), (0, -2.0, 1.22), 0.45, 0.45, FLESH, taper=0.8)
        for i in range(2):
            mb.box((0, -1.5 - i * 0.3, 1.45), (0.06, 0.1, 0.3), BONE, taper=(0.2, 0.3), shift_top=(0, 0.1))

    def head(mb):
        mb.box((0, -2.5, 1.26), (0.5, 1.0, 0.44), BONE, taper=(0.8, 0.75))
        mb.box((0, -2.25, 1.5), (0.3, 0.5, 0.18), BONE_DARK, taper=(0.5, 0.7))
        for sd in (R, L):
            # Eyes standing out of the cheek (in its plane, they flickered over the bone).
            mb.box((sd * 0.225, -2.42, 1.35), (0.06, 0.16, 0.07), GLOW)
            mb.seg(mx((0.15, -2.1, 1.5), sd), mx((0.32, -1.55, 1.8), sd), 0.08, 0.08, BONE, taper=0.15)
            for y in (-2.55, -2.75, -2.95):
                mb.box((sd * 0.16, y, 1.03), (0.04, 0.04, 0.12), BONE)
        # The mouth's slit, a little out of the snout's faces (flush, it flickered).
        mb.box((0, -2.63, 1.03), (0.32, 0.8, 0.05), MOUTH)

    def jaw(mb):
        mb.box((0, -2.6, 0.95), (0.4, 0.95, 0.15), BONE_DARK, taper=(0.8, 0.85))
        for sd in (R, L):
            for y in (-2.65, -2.85):
                mb.box((sd * 0.14, y, 1.07), (0.04, 0.04, 0.1), BONE)

    rig.part("torso", (0, 0, 0.7), build=torso)
    rig.part("neck", (0, -1.35, 1.12), "torso", build=neck)
    rig.part("head", (0, -2.0, 1.22), "neck", build=head)
    rig.part("jaw", (0, -2.15, 1.05), "head", build=jaw)

    def tail(a, b, w):
        def geo(mb):
            mb.seg(a, b, w, w, FLESH_DARK, taper=0.7)
            m = tuple((p + q) / 2 for p, q in zip(a, b))
            mb.box((0, m[1], m[2] + w * 0.6), (0.06, 0.1, 0.25), BONE, taper=(0.2, 0.3), shift_top=(0, 0.1))
        return geo

    rig.part("tail1", (0, 1.7, 0.6), "torso", build=tail((0, 1.7, 0.6), (0, 2.8, 0.45), 0.3))
    rig.part("tail2", (0, 2.8, 0.45), "tail1", build=tail((0, 2.8, 0.45), (0, 4.0, 0.3), 0.21))
    rig.part("tail3", (0, 4.0, 0.3), "tail2", build=tail((0, 4.0, 0.3), (0, 5.0, 0.18), 0.13))

    def claws(mb, p, n=3, length=0.4):
        x, y, z = p
        for i in range(n):
            dx = (i - (n - 1) / 2) * 0.1
            mb.seg((x + dx, y, z), (x + dx * 1.4, y - length, z - 0.15), 0.06, 0.06, BONE, taper=0.15)

    for sd_name, sd in SIDES:
        sh, el, wr = mx((0.6, -0.9, 1.05), sd), mx((1.35, -1.05, 0.75), sd), mx((1.45, -1.3, 0.12), sd)

        def up(mb, sh=sh, el=el):
            mb.seg(sh, el, 0.34, 0.34, FLESH, taper=0.8)

        def fore(mb, el=el, wr=wr):
            mb.seg(el, wr, 0.25, 0.25, FLESH_DARK, taper=0.85)

        def hand(mb, wr=wr):
            mb.box((wr[0], wr[1] - 0.05, 0.12), (0.3, 0.32, 0.2), FLESH_DARK)
            claws(mb, (wr[0], wr[1] - 0.2, 0.15), 4, 0.42)

        rig.part(f"upper_arm_{sd_name}", sh, "torso", build=up)
        rig.part(f"forearm_{sd_name}", el, f"upper_arm_{sd_name}", build=fore)
        rig.part(f"hand_{sd_name}", wr, f"forearm_{sd_name}", build=hand)

        # Extra arms coming out of the back and curling forward.
        bs, be, bt = mx((0.32, 0.1, 1.22), sd), mx((1.6, 0.0, 2.2), sd), mx((2.2, -0.9, 1.4), sd)

        def back_up(mb, bs=bs, be=be):
            mb.seg(bs, be, 0.17, 0.17, FLESH_DARK, taper=0.8)

        def back_fore(mb, be=be, bt=bt, sd=sd):
            mb.seg(be, bt, 0.13, 0.13, FLESH, taper=0.7)
            claws(mb, bt, 3, 0.35)

        rig.part(f"back_arm_{sd_name}", bs, "torso", build=back_up)
        rig.part(f"back_fore_{sd_name}", be, f"back_arm_{sd_name}", build=back_fore)

        hp, kn, an = mx((0.45, 1.5, 0.7), sd), mx((1.2, 1.25, 0.6), sd), mx((1.3, 1.7, 0.18), sd)

        def thigh(mb, hp=hp, kn=kn):
            mb.seg(hp, kn, 0.36, 0.38, FLESH, taper=0.8)

        def shin(mb, kn=kn, an=an):
            mb.seg(kn, an, 0.24, 0.24, FLESH_DARK, taper=0.85)

        def foot(mb, an=an):
            mb.seg(an, (an[0], an[1] - 0.25, 0.06), 0.2, 0.2, FLESH_DARK)
            claws(mb, (an[0], an[1] - 0.3, 0.1), 3, 0.25)

        rig.part(f"thigh_{sd_name}", hp, "torso", build=thigh)
        rig.part(f"shin_{sd_name}", kn, f"thigh_{sd_name}", build=shin)
        rig.part(f"foot_{sd_name}", an, f"shin_{sd_name}", build=foot)

    lock(rig, "lock_head", (0, -2.55, 1.25), "head")
    lock(rig, "lock_hips", (0, 1.4, 0.9), "torso")

    # Crouched, head low, dorsal arms raised like a mantis: it stalks its prey.
    BASE = {"torso": (-3, 0, 0), "neck": (-4, 0, 0), "head": (6, 0, 0), "jaw": (6, 0, 0),
            "upper_arm_R": (8, 0, 0), "upper_arm_L": (8, 0, 0),
            "back_arm_R": (-14, 28, 10), "back_arm_L": (-14, -28, -10), "back_fore_R": (-10, 0, 0), "back_fore_L": (-10, 0, 0),
            "tail1": (0, 0, 0), "tail2": (2, 0, 0), "tail3": (2, 0, 0)}

    def idle(p, ph):
        b = s(ph)
        sway = s(ph, 1, 0.2)
        out = lift(p, "torso", (0, 0, 0.04 * b))
        out = add(out, torso=(1.5 * b, 2 * sway, 0), neck=(-3 * b, 0, 14 * sway), head=(4 * s(ph, 2), -8 * sway, 16 * sway),
                  jaw=(18 * max(0.0, s(ph, 1, 0.4)), 0, 0),
                  back_arm_R=(8 * s(ph, 1, 0.1), -6 * s(ph, 2), 0), back_arm_L=(8 * s(ph, 1, 0.6), 6 * s(ph, 2, 0.5), 0),
                  back_fore_R=(10 * s(ph, 2, 0.2), 0, 0), back_fore_L=(10 * s(ph, 2, 0.7), 0, 0),
                  tail1=(0, 0, 10 * s(ph, 1, 0.5)), tail2=(0, 0, 14 * s(ph, 1, 0.65)), tail3=(0, 0, 20 * s(ph, 1, 0.8)))
        return out

    def walk(p, ph):
        a = s(ph)
        out = add(p, upper_arm_R=(-18 * a, 0, 0), forearm_R=(-16 * max(0.0, s(ph, 1, 0.75)), 0, 0),
                  upper_arm_L=(18 * a, 0, 0), forearm_L=(-16 * max(0.0, s(ph, 1, 0.25)), 0, 0),
                  thigh_R=(18 * a, 0, 0), shin_R=(16 * max(0.0, s(ph, 1, 0.25)), 0, 0),
                  thigh_L=(-18 * a, 0, 0), shin_L=(16 * max(0.0, s(ph, 1, 0.75)), 0, 0),
                  torso=(0, 3 * a, 6 * a), neck=(0, 0, -8 * a), tail1=(0, 0, 12 * a), tail2=(0, 0, 16 * s(ph, 1, 0.15)),
                  tail3=(0, 0, 20 * s(ph, 1, 0.3)))
        return lift(out, "torso", (0, 0, 0.05 * abs(s(ph, 2))))

    COIL = add(BASE, torso=(-6, 0, 0), neck=(-20, 0, 0), head=(10, 0, 0), jaw=(10, 0, 0))
    STRIKE = lift(add(BASE, torso=(4, 0, 0), neck=(14, 0, 0), head=(-6, 0, 0), upper_arm_R=(-14, 0, 0), upper_arm_L=(-14, 0, 0)),
                  "torso", (0, -0.25, 0.0))
    bite = [(0, BASE), (24, COIL), ("h0-6", add(COIL, jaw=(36, 0, 0))), ("h0", add(STRIKE, jaw=(40, 0, 0))),
            ("h0+4", STRIKE), ("h0e+16", STRIKE), ("T", BASE)]

    def stab(side, up):
        a, f = f"back_arm_{side}", f"back_fore_{side}"
        sg = 1 if side == "R" else -1
        if up:
            return {a: (-50, 40 * sg, 14 * sg), f: (-40, 0, 0)}
        return {a: (30, 6 * sg, -10 * sg), f: (40, 0, 0)}
    back_stabs = [(0, BASE), ("h0-16", merge(add(BASE, torso=(-6, 0, -6)), stab("R", True))),
                  ("h0", merge(add(BASE, torso=(4, 0, 4)), stab("R", False))),
                  ("h1-14", merge(add(BASE, torso=(-6, 0, 6)), stab("R", False), stab("L", True))),
                  ("h1", merge(add(BASE, torso=(4, 0, -4)), stab("L", False), stab("R", False))),
                  ("h1e+16", merge(BASE, stab("L", False))), ("T", BASE)]

    def tail_pose(z1, z2, z3, body_z=0, look=0):
        return add(BASE, torso=(0, 0, body_z), tail1=(0, 0, z1), tail2=(0, 0, z2), tail3=(0, 0, z3), neck=(0, 0, look),
                   head=(0, 0, look * 0.6))
    tail_whip = [(0, BASE), ("h0-18", tail_pose(40, 20, 10, 14, -24)), ("h0", tail_pose(60, 26, 16, 18, -28)),
                 ("h0+9", tail_pose(0, 20, 30, 0, -10)), ("h0e", tail_pose(-60, -26, -12, -18, 20)),
                 ("h0e+16", tail_pose(-45, -35, -30, -10, 12)), ("T", BASE)]

    # Pivot: the target is on its flank (`sd` = 1 left, -1 right); the game turns it
    # a quarter turn the other way, and the tail whips the targeted flank.
    def pivot(sd):
        def pose(z1, z2, z3, look):
            return add(BASE, tail1=(0, 0, z1 * sd), tail2=(0, 0, z2 * sd), tail3=(0, 0, z3 * sd),
                       neck=(0, 0, look * sd), head=(0, 0, look * 0.6 * sd),
                       upper_arm_R=(-14 * sd, 0, 0), upper_arm_L=(14 * sd, 0, 0))
        return [(0, BASE), ("h0-20", pose(34, 16, 10, 28)), ("h0-2", pose(42, 22, 14, 32)),
                ("h0+12", pose(-10, -20, -12, 12)), ("h0e", pose(-48, -34, -26, -8)),
                ("h0e+14", pose(-30, -25, -20, 0)), ("T", BASE)]

    CROUCH = lift(add(BASE, torso=(6, 0, 0), upper_arm_R=(20, 0, 0), upper_arm_L=(20, 0, 0), thigh_R=(-20, 0, 0),
                      thigh_L=(-20, 0, 0), shin_R=(30, 0, 0), shin_L=(30, 0, 0)), "torso", (0, 0, -0.15))
    LEAP = lift(add(BASE, torso=(-8, 0, 0), upper_arm_R=(-60, 0, 0), upper_arm_L=(-60, 0, 0), thigh_R=(40, 0, 0),
                    thigh_L=(40, 0, 0), neck=(-10, 0, 0), jaw=(40, 0, 0), tail1=(-20, 0, 0)), "torso", (0, 0, 1.4))
    LANDED = lift(add(BASE, torso=(8, 0, 0), upper_arm_R=(-30, 0, 0), upper_arm_L=(-30, 0, 0), neck=(10, 0, 0),
                      head=(-6, 0, 0), jaw=(30, 0, 0)), "torso", (0, 0, -0.08))
    pounce = [(0, BASE), (26, CROUCH), (38, lift(CROUCH, "torso", (0, 0, -0.05))), (48, LEAP),
              ("h0-2", lift(LEAP, "torso", (0, 0, -0.9))), ("h0", LANDED), ("h0e+20", LANDED), ("T", BASE)]

    def claws_pose(z):
        return add(BASE, torso=(4, 0, z * 0.3), upper_arm_R=(-60, 0, z), upper_arm_L=(-60, 0, z), neck=(0, 0, -z * 0.4))
    frenzy = [(0, BASE), ("h0-8", claws_pose(-50)), ("h0", claws_pose(-30)), ("h0e", claws_pose(40)), ("h1-6", claws_pose(50)),
              ("h1", claws_pose(30)), ("h1e", claws_pose(-40)), ("h2-6", claws_pose(-50)), ("h2", claws_pose(-30)),
              ("h2e", claws_pose(40)), ("h3-12", COIL), ("h3", add(STRIKE, jaw=(40, 0, 0))), ("h3e+16", STRIKE), ("T", BASE)]

    # Rearing on its hind legs (the torso pivots around the hips), then it comes back down.
    def upright(p, pitch, rise):
        return lift(add(p, torso=(-pitch, 0, 0), thigh_R=(pitch * 0.6, 0, 0), thigh_L=(pitch * 0.6, 0, 0),
                        tail1=(pitch * 0.5, 0, 0)), "torso", (0, -rise * 0.4, rise))
    REARED = upright(add(BASE, neck=(-24, 0, 0), head=(-10, 0, 0), jaw=(50, 0, 0), upper_arm_R=(-40, 30, 0),
                         upper_arm_L=(-40, -30, 0), back_arm_R=(-30, 30, 0), back_arm_L=(-30, -30, 0)), 34, 0.9)
    STOOD = upright(add(BASE, neck=(-10, 0, 0), head=(-14, 0, 0), jaw=(56, 0, 0), upper_arm_R=(-100, 30, 0),
                        upper_arm_L=(-100, -30, 0), forearm_R=(-30, 0, 0), forearm_L=(-30, 0, 0),
                        back_arm_R=(-60, 40, 0), back_arm_L=(-60, -40, 0)), 50, 1.2)
    SLAMMED = lift(add(BASE, torso=(8, 0, 0), neck=(12, 0, 0), jaw=(40, 0, 0), upper_arm_R=(-30, 0, 0), upper_arm_L=(-30, 0, 0),
                       back_arm_R=(30, 10, 0), back_arm_L=(30, -10, 0)), "torso", (0, -0.2, -0.08))
    howl = [(0, BASE), (30, tween(BASE, REARED, 0.6)), ("h0-12", REARED), ("h0", SLAMMED), ("h0e+20", SLAMMED), ("T", BASE)]
    rise_crash = [(0, BASE), (30, tween(BASE, STOOD, 0.6)), ("h0-28", STOOD), ("h0-10", add(STOOD, torso=(-6, 0, 0))),
                  ("h0", SLAMMED), ("h0e+34", SLAMMED), ("T", BASE)]
    roar = [(0, BASE), (30, REARED), ("T-40", add(REARED, head=(0, 0, 14))), ("T", BASE)]

    FLOOR = lift(add(BASE, torso=(6, 0, 0), neck=(24, 0, 0), head=(-10, 0, 0), jaw=(16, 0, 0), upper_arm_R=(-30, 20, 0),
                     upper_arm_L=(-30, -20, 0), forearm_R=(40, 0, 0), forearm_L=(40, 0, 0), thigh_R=(-20, 0, 0),
                     thigh_L=(-20, 0, 0), back_arm_R=(20, -20, 0), back_arm_L=(20, 20, 0)),
                 "torso", (0, 0, -0.25))
    groggy = [(0, BASE), (18, FLOOR), (110, add(FLOOR, head=(4, 0, 12))), ("T-40", FLOOR), ("T-20", tween(FLOOR, BASE, 0.6)), ("T", BASE)]
    fatal = [(0, FLOOR), (40, FLOOR), (50, add(FLOOR, neck=(-40, 0, 0), jaw=(30, 0, 0))), (80, add(FLOOR, neck=(10, 0, 0))),
             ("T-20", tween(FLOOR, BASE, 0.6)), ("T", BASE)]
    DEAD = lift(add(FLOOR, torso=(0, 80, 0), neck=(-10, 0, 30), jaw=(30, 0, 0), tail1=(0, 0, 30)), "torso", (0.4, 0, 0.5))
    death = [(0, BASE), (30, REARED), (70, FLOOR), (130, DEAD), ("T", DEAD)]

    finish(rig, "spine_beast", cycle(BASE, 10, 200, idle), [
        ("walk", cycle(BASE, 8, 80, walk), True), ("bite", bite), ("back_stabs", back_stabs), ("tail_whip", tail_whip),
        ("pivot_L", pivot(1)), ("pivot_R", pivot(-1)), ("pounce", pounce), ("frenzy", frenzy), ("howl", howl),
        ("rise_crash", rise_crash), ("roar", roar), ("groggy", groggy), ("fatal_received", fatal), ("death", death),
    ])


# ============================================================================= great marionette

def marionette():
    rig = start("marionette")
    # Violet and gold: her colour, that of her ground warnings.
    DRESS = material("m_dress", tex=tex_stripes((0.4, 0.12, 0.55), (0.78, 0.66, 0.42), n=10, seed=171))
    BODICE = material("m_bodice", tex=tex_noise((0.26, 0.07, 0.36), 0.4, seed=172))
    LACE = material("m_lace", (0.88, 0.85, 0.78))
    PORCELAIN = material("m_porcelain", (0.93, 0.9, 0.85))
    WOOD = material("m_wood", tex=tex_planks((0.5, 0.36, 0.22), seed=173))
    GOLD = material("m_gold", (0.82, 0.62, 0.25))
    DARK = material("m_dark", (0.05, 0.04, 0.05))
    # Lilac cheekbones, matching the dress.
    BLUSH = material("m_blush", (0.68, 0.44, 0.7))
    EYES = material("m_eyes", (0.86, 0.5, 1.0), emissive=(0.72, 0.3, 1.0))
    STRING = material("m_string", (0.85, 0.82, 0.74))
    TOP = 11.0

    def string(mb, p, top=(0, 0)):
        mb.seg(p, (top[0], top[1], TOP), 0.035, 0.035, STRING)

    def hips(mb):
        mb.cylinder((0, 0, 1.55), 0.95, 1.3, DRESS, sides=10, radius_top=0.38)
        mb.cylinder((0, 0, 0.9), 0.97, 0.06, GOLD, sides=10)
        mb.cylinder((0, 0, 2.22), 0.38, 0.06, GOLD, sides=10)

    def chest(mb):
        mb.box((0, 0, 2.72), (0.68, 0.44, 0.9), BODICE, taper=(1.3, 1.1))
        mb.cylinder((0, 0, 3.23), 0.5, 0.12, LACE, sides=12, radius_top=0.56)
        mb.box((0, -0.23, 2.7), (0.18, 0.03, 0.6), GOLD, taper=(1.6, 1))

    def head(mb):
        mb.cylinder((0, 0, 3.33), 0.09, 0.12, WOOD, sides=6)
        mb.box((0, 0, 3.62), (0.5, 0.5, 0.6), PORCELAIN, taper=(0.9, 0.9))
        for sd in (R, L):
            mb.box((sd * 0.11, -0.252, 3.7), (0.1, 0.02, 0.08), DARK)
            mb.box((sd * 0.11, -0.256, 3.7), (0.035, 0.02, 0.035), EYES)
            mb.box((sd * 0.16, -0.252, 3.56), (0.07, 0.02, 0.05), BLUSH)
            mb.box((sd * 0.11, -0.254, 3.58), (0.02, 0.02, 0.1), DARK)
        mb.box((-0.06, -0.253, 3.82), (0.02, 0.02, 0.14), DARK)
        mb.cylinder((0, 0, 3.98), 0.24, 0.16, GOLD, sides=8, radius_top=0.28)
        for i in range(8):
            a = 2 * math.pi * i / 8
            mb.box((math.cos(a) * 0.26, math.sin(a) * 0.26, 4.1), (0.05, 0.05, 0.1), GOLD, taper=(0.2, 0.2))

    def jaw(mb):
        mb.box((0, -0.06, 3.38), (0.26, 0.4, 0.12), PORCELAIN)
        mb.box((0, -0.2, 3.45), (0.16, 0.02, 0.03), DARK)

    def control(mb):
        mb.box((0, 0, TOP), (2.6, 0.14, 0.14), WOOD)
        mb.box((0, 0, TOP), (0.14, 1.6, 0.14), WOOD)
        # The strings (fixed relative to the cross): head, hands, knees.
        string(mb, (0, 0.05, 4.2), (0, 0.05))
        for sd in (R, L):
            string(mb, (sd * 0.56, 0.05, 1.5), (sd * 1.25, 0))
            string(mb, (sd * 0.26, -0.1, 0.7), (sd * 0.5, -0.75))

    # Everything hangs from the cross: she swings like a pendulum.
    rig.part("control", (0, 0, TOP), build=control)
    rig.part("hips", (0, 0, 2.2), "control", build=hips)
    rig.part("chest", (0, 0, 2.3), "hips", build=chest)
    rig.part("head", (0, 0, 3.32), "chest", build=head)
    rig.part("jaw", (0, -0.05, 3.44), "head", build=jaw)
    for sd_name, sd in SIDES:
        x = 0.48 * sd

        def up(mb, x=x):
            mb.cylinder((x, 0, 3.1), 0.12, 0.2, WOOD, sides=6)
            mb.seg((x, 0, 3.05), (x * 1.05, 0, 2.38), 0.16, 0.16, WOOD, taper=0.9)
            mb.cylinder((x, 0, 2.95), 0.18, 0.2, LACE, sides=8, radius_top=0.13)

        def fore(mb, x=x):
            mb.cylinder((x * 1.05, 0, 2.36), 0.1, 0.12, WOOD, sides=6)
            mb.seg((x * 1.05, 0, 2.3), (x * 1.05, 0, 1.65), 0.14, 0.14, WOOD, taper=0.9)

        def hand(mb, x=x):
            mb.box((x * 1.05, -0.02, 1.42), (0.1, 0.3, 0.42), PORCELAIN, taper=(1, 0.8))

        rig.part(f"upper_arm_{sd_name}", (x, 0, 3.1), "chest", build=up)
        rig.part(f"forearm_{sd_name}", (x * 1.05, 0, 2.36), f"upper_arm_{sd_name}", build=fore)
        rig.part(f"hand_{sd_name}", (x * 1.05, 0, 1.64), f"forearm_{sd_name}", build=hand)

        lx = 0.22 * sd

        def thigh(mb, x=lx):
            mb.seg((x, 0, 1.25), (x, 0, 0.7), 0.15, 0.15, WOOD, taper=0.9)

        def shin(mb, x=lx):
            mb.cylinder((x, 0, 0.68), 0.09, 0.1, WOOD, sides=6)
            mb.seg((x, 0, 0.65), (x, 0, 0.32), 0.12, 0.12, WOOD, taper=0.9)

        def foot(mb, x=lx):
            mb.box((x, -0.1, 0.24), (0.14, 0.38, 0.14), DARK, taper=(0.6, 0.7))

        rig.part(f"thigh_{sd_name}", (lx, 0, 1.25), "hips", build=thigh)
        rig.part(f"shin_{sd_name}", (lx, 0, 0.68), f"thigh_{sd_name}", build=shin)
        rig.part(f"foot_{sd_name}", (lx, 0, 0.32), f"shin_{sd_name}", build=foot)


    # Suspended: the feet don't touch the ground, the limbs dangle.
    BASE = merge(legs(-6, 14, 4, 10), {"hips": {"r": (0, 0, 0), "t": (0, 0, 0.16)}, "head": (8, 8, 0), "jaw": (8, 0, 0)},
                 arm("R", (0, 4, 0), (-8, 0, 0), (6, 0, 0)), arm("L", (2, -4, 0), (-4, 0, 0)))

    def idle(p, ph):
        b = s(ph)
        c = s(ph, 1, 0.25)
        return add(p, control=(1.2 * b, 1.2 * c, 5 * c), chest=(-2 * b, -2 * c, 0),
                   head=(4 * s(ph, 1, 0.1), 10 * s(ph, 1, 0.3), 6 * s(ph, 1, 0.5)), jaw=(6 * s(ph, 2), 0, 0),
                   upper_arm_R=(-4 * s(ph, 1, 0.15), 2 * c, 0), upper_arm_L=(-4 * s(ph, 1, 0.65), -2 * c, 0),
                   forearm_R=(-6 * s(ph, 1, 0.3), 0, 0), forearm_L=(-6 * s(ph, 1, 0.8), 0, 0),
                   thigh_R=(-5 * s(ph, 1, 0.4), 0, 0), thigh_L=(-5 * s(ph, 1, 0.9), 0, 0),
                   shin_R=(8 * s(ph, 1, 0.55), 0, 0), shin_L=(8 * s(ph, 1, 0.05), 0, 0))

    lock(rig, "lock_head", (0, 0, 3.62), "head")
    lock(rig, "lock_chest", (0, 0, 2.72), "chest")

    def hang(p, z=0.0, swing=0.0):
        """The whole marionette rises or falls with its cross, and sways on it."""
        return lift(add(p, control=(swing, 0, 0)), "control", (0, 0, z))

    def walk(p, ph):
        a = s(ph)
        out = add(p, thigh_R=(-22 * a, 0, 0), thigh_L=(22 * a, 0, 0), shin_R=(14 * max(0.0, a), 0, 0),
                  shin_L=(14 * max(0.0, -a), 0, 0), control=(1.5 * s(ph, 2), 0, 3 * a), chest=(0, 0, -4 * a),
                  upper_arm_R=(10 * a, 0, 0), upper_arm_L=(-10 * a, 0, 0))
        return lift(out, "control", (0, 0, 0.08 * abs(s(ph, 2))))

    # She leans and bends her knees: the hand sweeps at head height.
    def slap_pose(z):
        return hang(merge(add(BASE, chest=(30, 0, z), head=(-20, 0, -z * 0.3)), legs(-35, 55, -25, 50),
                          arm("R", (-45, 0, z * 0.2), (-10, 0, 0), (10, 0, 0))), -0.3)
    slap = [(0, BASE), ("h0-18", slap_pose(-70)), ("h0", slap_pose(-50)), ("h0e", slap_pose(60)), ("h0e+14", slap_pose(70)),
            ("T", BASE)]

    LIMP = add(BASE, chest=(-10, 0, 0), head=(-14, 0, 0), thigh_R=(10, 0, 0), thigh_L=(10, 0, 0),
               upper_arm_R=(0, -10, 0), upper_arm_L=(0, 10, 0))
    CRASH = hang(merge(BASE, legs(-60, 90, -45, 85, 0), {"chest": (30, 0, 0), "head": (20, 0, 0), "jaw": (30, 0, 0)},
                       arm("R", (-40, 30, 0), (-30, 0, 0)), arm("L", (-40, -30, 0), (-30, 0, 0))), -0.7)
    drop = [(0, BASE), (30, hang(LIMP, 0.8)), (50, hang(LIMP, 3.4)), (70, hang(LIMP, 4.2, 4)), ("h0-6", hang(LIMP, 2.4)),
            ("h0", CRASH), ("h0e+30", CRASH), ("T", BASE)]

    TWIRL = hang(merge(BASE, legs(-30, 50, -30, 50), {"chest": (20, 0, 0), "head": (-8, 0, 0)},
                       arm("R", (-10, 55, 0), (-6, 0, 0)), arm("L", (-10, -55, 0), (-6, 0, 0))), -0.45)
    spin = [(0, BASE), ("h0-16", add(TWIRL, hips=(0, 0, -40))), ("h0", add(TWIRL, hips=(0, 0, -20))),
            ("h0+14", add(TWIRL, hips=(0, 0, 180))), ("h0e", add(TWIRL, hips=(0, 0, 380))), ("h0e+16", add(TWIRL, hips=(0, 0, 400))),
            ("T", add(BASE, hips=(0, 0, 360)))]

    def kick(side, up):
        if up:
            return hang(add(BASE, **{f"thigh_{side}": (-80, 0, 0), f"shin_{side}": (70, 0, 0)}, chest=(-8, 0, 0)), 0.5)
        return hang(add(BASE, **{f"thigh_{side}": (-30, 0, 0)}, chest=(14, 0, 0), jaw=(20, 0, 0)), -0.2)
    stomp_dance = [(0, BASE), ("h0-16", kick("R", True)), ("h0", kick("R", False)), ("h1-16", kick("L", True)),
                   ("h1", kick("L", False)), ("h1e+16", kick("L", False)), ("T", BASE)]

    PULLED = hang(merge(LIMP, arm("R", (20, -10, 0)), arm("L", (20, 10, 0))), 0.6, 10)
    PULL = merge(BASE, {"chest": (-14, 0, 0), "head": (-24, 0, 0), "jaw": (30, 0, 0)},
                 arm("R", (-170, 10, 0), (-10, 0, 0)), arm("L", (-170, -10, 0), (-10, 0, 0)))
    TUG = merge(PULL, arm("R", (-130, 20, 0), (-60, 0, 0)), arm("L", (-130, -20, 0), (-60, 0, 0)))
    strings = [(0, BASE), (24, PULL), ("c0-4", hang(TUG, 0.3)), ("c0", hang(PULL, -0.1)), ("c0+30", hang(TUG, 0.2)),
               ("c0+60", PULL), ("T", BASE)]

    # Needles: she raises both arms above her head (they appear there, clearly
    # visible), then brings them down towards her target.
    NEEDLES_UP = merge(LIMP, {"chest": (-12, 0, 0), "head": (-22, 0, 0), "jaw": (16, 0, 0)},
                       arm("R", (-172, 14, 0), (-10, 0, 0)), arm("L", (-172, -14, 0), (-10, 0, 0)))
    NEEDLES_THROWN = merge(LIMP, {"chest": (22, 0, 0), "head": (14, 0, 0), "jaw": (24, 0, 0)},
                           arm("R", (-70, 6, 0), (-10, 0, 0)), arm("L", (-70, -6, 0), (-10, 0, 0)))

    # Ascent: the strings hoist her way up; she throws three needles from there, then falls.
    HIGH = 3.2
    ascent = [(0, BASE), (16, hang(LIMP, -0.2)), (40, hang(LIMP, HIGH * 0.8, 4)), ("c0-16", hang(NEEDLES_UP, HIGH)),
              ("c0", hang(NEEDLES_UP, HIGH, -2)), ("c0+6", hang(NEEDLES_THROWN, HIGH)), ("c1-8", hang(NEEDLES_UP, HIGH)),
              ("c1", hang(NEEDLES_UP, HIGH, 2)), ("c1+6", hang(NEEDLES_THROWN, HIGH)), ("c2-8", hang(NEEDLES_UP, HIGH)),
              ("c2", hang(NEEDLES_UP, HIGH, -2)), ("c2+6", hang(NEEDLES_THROWN, HIGH)), ("h0-18", hang(LIMP, HIGH + 0.4)),
              ("h0-4", hang(LIMP, 1.6)), ("h0", CRASH), ("h0e+30", CRASH), ("T", BASE)]

    ROAR = merge(BASE, {"chest": (-16, 0, 0), "head": (-30, 0, 0), "jaw": (40, 0, 0)},
                 arm("R", (-100, 70, 0), (-10, 0, 0)), arm("L", (-100, -70, 0), (-10, 0, 0)))
    roar = [(0, BASE), (24, add(BASE, chest=(20, 0, 0))), (40, ROAR), ("T-30", add(ROAR, head=(10, 0, 10))), ("T", BASE)]

    # Leap: the strings lift her and set her down further away; in the air, she raises her arms and throws
    # two needles.
    # Arms spread wide: one needle in each hand, at her left and right (the casts' `from`, bosses.ron).
    NEEDLES_SPREAD = merge(NEEDLES_UP, arm("R", (-160, -40, 0), (-10, 0, 0)), arm("L", (-160, 40, 0), (-10, 0, 0)))
    NEEDLES_SPREAD_THROWN = merge(NEEDLES_THROWN, arm("R", (-70, -24, 0), (-10, 0, 0)), arm("L", (-70, 24, 0), (-10, 0, 0)))
    hop = [(0, BASE), (8, hang(LIMP, -0.25)), ("c0-14", hang(NEEDLES_SPREAD, 1.4, 6)), ("c0", hang(NEEDLES_SPREAD, 1.8, -2)),
           ("c0+6", hang(NEEDLES_SPREAD_THROWN, 1.8, -4)), (48, hang(LIMP, 0.6, 2)),
           (56, hang(add(LIMP, thigh_R=(-20, 0, 0), thigh_L=(-20, 0, 0)), -0.15)), ("T", BASE)]

    # Hoisted: the strings pull her straight up, very high, then drop her on the spot.
    hoist = [(0, BASE), (20, hang(LIMP, -0.3)), (36, hang(PULLED, 2.2, 4)), (60, hang(LIMP, 5.6, -3)),
             ("h0-14", hang(add(LIMP, chest=(-12, 0, 0)), 6.2)), ("h0-4", hang(LIMP, 1.4)), ("h0", CRASH),
             ("h0e+30", CRASH), ("T", BASE)]
    SLUMP = hang(merge(BASE, legs(-80, 85, -70, 90, 0), {"chest": (36, 0, 10), "head": (40, 0, 20), "jaw": (10, 0, 0)},
                       arm("R", (6, -20, 0), (-10, 0, 0)), arm("L", (6, 20, 0), (-10, 0, 0))), -1.2)
    groggy = [(0, BASE), (16, SLUMP), (110, add(SLUMP, head=(0, 10, -10))), ("T-40", SLUMP), ("T-20", hang(LIMP, 0.2)), ("T", BASE)]
    fatal = [(0, SLUMP), (40, SLUMP), (50, add(SLUMP, chest=(-50, 0, 0), head=(-60, 0, 0))), (80, SLUMP),
             ("T-20", hang(LIMP, 0.2)), ("T", BASE)]
    CUT = lift(merge(BASE, legs(-90, 0, -80, 10, 0), {"hips": {"r": (80, 0, 20), "t": (0, -1.0, -1.85)},
                                                      "chest": (10, 0, 0), "head": (20, 0, 40), "jaw": (20, 0, 0)},
                     arm("R", (-150, 40, 0)), arm("L", (-160, -30, 0))), "control", (0, 0, 0))
    death = [(0, BASE), (30, hang(ROAR, 0.6)), (60, hang(LIMP, 1.0)), (90, CUT), ("T", CUT)]

    finish(rig, "marionette", cycle(BASE, 12, 300, idle), [
        ("walk", cycle(BASE, 8, 90, walk), True), ("slap", slap), ("drop", drop), ("spin", spin),
        ("stomp_dance", stomp_dance), ("hop", hop), ("hoist", hoist), ("strings", strings), ("ascent", ascent),
        ("roar", roar), ("groggy", groggy),
        ("fatal_received", fatal), ("death", death),
    ])


# ============================================================================= flame-bearing giant

def giant():
    rig = start("giant")
    STONE = material("t_stone", tex=tex_noise((0.42, 0.38, 0.34), 0.5, seed=181))
    STONE_DARK = material("t_stone_dark", tex=tex_noise((0.22, 0.2, 0.19), 0.45, seed=182))
    CLOTH = material("t_cloth", tex=tex_noise((0.3, 0.26, 0.2), 0.5, seed=183))
    RIB = material("t_rib", (0.62, 0.58, 0.5))
    # His fire is ash-grey (assets/config/bosses.ron: his colour, his spells, his torch).
    FLAME = material("t_flame", (0.62, 0.62, 0.66), emissive=(0.42, 0.42, 0.46))
    CORE = material("t_core", (0.9, 0.9, 0.93), emissive=(0.75, 0.75, 0.8))
    COLUMN = material("t_column", tex=tex_stone(64, 184, (0.6, 0.57, 0.52)))

    # Short belt and loincloth: the trousers are carried by the thighs (otherwise the legs
    # go through them when they move).
    def hips(mb):
        mb.box((0, 0, 2.95), (1.5, 0.9, 0.6), STONE_DARK)
        mb.box((0, -0.05, 2.62), (1.62, 1.02, 0.6), CLOTH, taper=(0.95, 0.95))
        mb.box((0, -0.56, 2.3), (0.6, 0.05, 0.5), CLOTH, taper=(1.15, 1))

    def chest(mb):
        mb.box((0, 0.1, 4.1), (1.8, 1.0, 1.9), STONE, taper=(1.3, 1.1))
        mb.box((0, 0.15, 5.1), (2.3, 1.0, 0.4), STONE_DARK, taper=(0.6, 0.8))
        # Open chest: the flame burns between the ribs.
        mb.box((0, -0.42, 4.1), (0.9, 0.2, 1.1), FLAME)
        mb.box((0, -0.5, 4.1), (0.4, 0.1, 0.5), CORE)
        for i, z in enumerate((3.6, 3.9, 4.2, 4.5)):
            w = 1.1 - abs(i - 1.5) * 0.12
            mb.box((0, -0.58, z), (w, 0.12, 0.09), RIB)
        mb.box((0, -0.58, 4.05), (0.12, 0.1, 1.2), RIB)

    def head(mb):
        mb.box((0, -0.05, 5.4), (0.5, 0.5, 0.4), STONE_DARK)
        mb.box((0, -0.2, 5.75), (0.8, 0.8, 0.85), STONE, taper=(0.85, 0.9))
        mb.box((0, -0.62, 5.85), (0.7, 0.06, 0.16), STONE_DARK)
        for sd in (R, L):
            mb.box((sd * 0.18, -0.61, 5.74), (0.14, 0.04, 0.08), FLAME)
        mb.box((0, -0.62, 5.5), (0.36, 0.05, 0.08), FLAME)

    rig.part("hips", (0, 0, 2.9), build=hips)
    rig.part("chest", (0, 0, 3.2), "hips", build=chest)
    rig.part("head", (0, -0.05, 5.3), "chest", build=head)
    for sd_name, sd in SIDES:
        x = 1.25 * sd

        def up(mb, x=x):
            mb.box((x, 0.05, 5.1), (0.95, 1.0, 0.6), STONE_DARK, taper=(0.75, 0.8))
            mb.seg((x, 0, 5.0), (x * 1.05, 0, 3.8), 0.7, 0.7, STONE, taper=0.85)

        def fore(mb, x=x):
            mb.seg((x * 1.05, 0, 3.8), (x * 1.05, 0, 2.75), 0.6, 0.6, STONE, taper=0.9)
            mb.box((x * 1.05, 0, 3.0), (0.66, 0.66, 0.3), CLOTH)

        def hand(mb, x=x):
            mb.box((x * 1.05, -0.04, 2.45), (0.55, 0.6, 0.6), STONE_DARK)

        rig.part(f"upper_arm_{sd_name}", (x, 0, 5.0), "chest", build=up)
        rig.part(f"forearm_{sd_name}", (x * 1.05, 0, 3.8), f"upper_arm_{sd_name}", build=fore)
        rig.part(f"hand_{sd_name}", (x * 1.05, 0, 2.75), f"forearm_{sd_name}", build=hand)

        lx = 0.5 * sd

        def thigh(mb, x=lx):
            mb.box((x, 0, 2.2), (0.75, 0.8, 1.5), STONE, taper=(0.85, 0.85))
            mb.box((x, 0, 2.42), (0.86, 0.9, 1.05), CLOTH, taper=(1.08, 1.08))

        def shin(mb, x=lx):
            mb.box((x, 0, 0.85), (0.6, 0.66, 1.3), STONE, taper=(1.1, 1.1))
            mb.box((x, 0, 1.3), (0.7, 0.76, 0.25), CLOTH)

        def foot(mb, x=lx):
            mb.box((x, -0.18, 0.13), (0.75, 1.1, 0.26), STONE_DARK)

        rig.part(f"thigh_{sd_name}", (lx, 0, 2.9), "hips", build=thigh)
        rig.part(f"shin_{sd_name}", (lx, 0, 1.5), f"thigh_{sd_name}", build=shin)
        rig.part(f"foot_{sd_name}", (lx, 0, 0.26), f"shin_{sd_name}", build=foot)

    hx = 1.25 * 1.05 * R

    def column(mb):
        # A broken column as a club, resting on the ground.
        mb.cylinder((hx, -0.05, 1.45), 0.22, 2.6, COLUMN, sides=8)
        mb.cylinder((hx, -0.05, 2.75), 0.24, 0.2, COLUMN, sides=8)
        mb.cylinder((hx, -0.05, 0.15), 0.5, 0.3, COLUMN, sides=8)
        mb.box((hx, -0.05, 0.38), (0.7, 0.7, 0.2), COLUMN)

    rig.part("column", (hx, 0, 2.45), "hand_R", build=column)

    BASE = merge(legs(-10, 14, 6, 8, -0.08), {"chest": (8, 0, 6), "head": (6, 0, -6)},
                 arm("R", (-6, 4, 0), (-8, 0, 0), (14, 0, 0)), arm("L", (-10, -10, 0), (-24, 0, 0), (10, 0, 0)))

    def idle(p, ph):
        b = s(ph)
        out = lift(p, "chest", (0, 0, 0.06 * b))
        out = add(out, chest=(-2.5 * b, 0, 2 * s(ph, 1, 0.3)), head=(5 * s(ph, 1, 0.15), 6 * s(ph, 1, 0.4), -10 * s(ph, 1, 0.2)),
                  upper_arm_L=(-5 * b, 0, 0), forearm_L=(-6 * s(ph, 1, 0.2), 0, 0))
        return lift(out, "hips", (0, 0, 0.04 * b))

    lock(rig, "lock_head", (0, -0.2, 5.75), "head")
    lock(rig, "lock_chest", (0, -0.1, 4.1), "chest")

    OVER = merge(BASE, legs(-8, 14, 12, 18, -0.04), {"chest": (-16, 0, 0), "head": (-10, 0, 0)},
                 arm("R", (-175, 6, 0), (-10, 0, 0), (0, 0, 0)), arm("L", (-160, -20, 0), (-20, 0, 0), (0, 0, 0)))
    SMASH = merge(BASE, legs(-40, 44, 26, 14, -0.4, fwd=0.35), {"chest": (32, 0, 0), "head": (-14, 0, 0)},
                  arm("R", (-55, 4, 0), (-6, 0, 0), (-12, 0, 0)), arm("L", (-50, -24, 0), (-10, 0, 0), (0, 0, 0)))
    column_smash = [(0, BASE), ("h0-36", OVER), ("h0-10", lift(OVER, "hips", (0, 0, 0.1))), ("h0", SMASH), ("h0e+34", SMASH),
                    ("T", BASE)]

    # Recoil: he crouches and leaps back, away from his target.
    CROUCH = merge(BASE, legs(-40, 60, -30, 55, -0.45), {"chest": (20, 0, 0), "head": (-10, 0, 0)},
                   arm("L", (-20, -30, 0), (-30, 0, 0)), arm("R", (-10, 20, 0), (-10, 0, 0)))
    AIR = lift(merge(BASE, legs(-50, 80, -20, 50, 0.0), {"chest": (-8, 0, 0), "head": (6, 0, 0)},
                     arm("L", (-60, -40, 0), (-20, 0, 0)), arm("R", (-30, 30, 0), (-10, 0, 0))), "hips", (0, 0, 1.0))
    leap_back = [(0, BASE), (18, CROUCH), (26, lift(CROUCH, "hips", (0, 0, -0.08))), (34, AIR),
                 (46, lift(AIR, "hips", (0, 0, -0.5))), (52, CROUCH), (72, CROUCH), ("T", BASE)]

    HAND_UP = merge(BASE, {"chest": (-12, 0, 6), "head": (-20, 0, 0)}, arm("L", (-175, -6, 0), (-10, 0, 0), (0, 0, 0)))
    HAND_DOWN = merge(BASE, legs(-30, 40, 20, 30, -0.3), {"chest": (26, 0, 0), "head": (-6, 0, 0)},
                      arm("L", (-60, -10, 0), (-10, 0, 0), (40, 0, 0)))
    pillars = [(0, BASE), ("c0-24", HAND_UP), ("c0-6", add(HAND_UP, upper_arm_L=(-6, 0, 0))), ("c0", HAND_DOWN),
               ("c0+40", HAND_DOWN), ("T", BASE)]

    GRAB = merge(BASE, {"chest": (-6, 0, -20), "head": (10, 0, -10)}, arm("L", (-40, -20, 0), (-100, 0, 30), (0, 0, 0)))
    WIND = merge(BASE, {"chest": (-10, 0, 30), "head": (-6, 0, -20)}, arm("L", (-120, -60, 0), (-40, 0, 0), (0, 0, 0)))
    HURL = merge(BASE, legs(-30, 30, 10, 20, -0.2, fwd=0.2), {"chest": (24, 0, -20), "head": (-10, 0, 10)},
                 arm("L", (-90, 0, -20), (-6, 0, 0), (10, 0, 0)))
    fireballs = [(0, BASE), (26, GRAB), ("c0-14", WIND), ("c0", HURL), ("c1-10", WIND), ("c1", HURL), ("c2-10", WIND),
                 ("c2", HURL), ("c2+28", HURL), ("T", BASE)]

    def raise_leg(side):
        return merge(BASE, {f"thigh_{side}": (-70, 0, 0), f"shin_{side}": (80, 0, 0), f"foot_{side}": (-10, 0, 0),
                            "chest": (-10, 0, 0)}, arm("L", (-40, -40, 0), (-20, 0, 0)), arm("R", (-20, 30, 0), (-10, 0, 0)))
    STOMPED = merge(BASE, legs(-34, 40, 30, 40, -0.4), {"chest": (20, 0, 0), "head": (-10, 0, 0)})
    stomp = [(0, BASE), ("c0-20", raise_leg("R")), ("c0", STOMPED), ("c1-10", raise_leg("L")), ("c1", STOMPED),
             ("c1+30", STOMPED), ("T", BASE)]

    # Stomp: he raises his right foot high and slams it down just in front of him.
    trample = [(0, BASE), ("h0-26", raise_leg("R")), ("h0-8", lift(add(raise_leg("R"), chest=(-6, 0, 0)), "hips", (0, 0, 0.15))),
               ("h0", STOMPED), ("h0e+30", STOMPED), ("T", BASE)]

    # Sweep: the column, held low, mows the ground from his right to his left.
    def sweep_pose(z, arm_z):
        return merge(BASE, legs(-40, 60, 30, 50, -0.5, twist=z * 0.3), {"chest": (34, 0, z), "head": (-20, 0, -z * 0.4)},
                     arm("R", (-50, 30, arm_z), (-10, 0, 0), (-40, 0, 0)), arm("L", (-30, -20, 0), (-20, 0, 0)))
    low_sweep = [(0, BASE), ("h0-20", sweep_pose(-60, -20)), ("h0", sweep_pose(-45, -10)), ("h0e", sweep_pose(55, 30)),
                 ("h0e+24", sweep_pose(60, 30)), ("T", BASE)]

    GATHER = merge(BASE, legs(-30, 50, 20, 40, -0.4), {"chest": (30, 0, 0), "head": (20, 0, 0)},
                   arm("R", (-30, 30, 0), (-80, 0, 0)), arm("L", (-30, -30, 0), (-80, 0, 0)))
    BLAZE = merge(BASE, legs(-10, 14, 10, 14, -0.04), {"chest": (-24, 0, 0), "head": (-30, 0, 0)},
                  arm("R", (-120, 80, 0), (-10, 0, 0)), arm("L", (-120, -80, 0), (-10, 0, 0)))
    nova = [(0, BASE), (40, GATHER), ("c0-4", lift(GATHER, "hips", (0, 0, -0.1))), ("c0", BLAZE), ("c2", add(BLAZE, chest=(-4, 0, 0))),
            ("c2+40", BLAZE), ("T", BASE)]

    finish(rig, "giant", cycle(BASE, 10, 320, idle), [
        ("walk", biped_walk(BASE, 110, 18), True), ("column_smash", column_smash), ("leap_back", leap_back), ("pillars", pillars),
        ("fireballs", fireballs), ("stomp", stomp), ("nova", nova), ("trample", trample), ("low_sweep", low_sweep),
    ] + biped_states(BASE, 2.9))


BUILDERS = {
    "dragon": dragon, "horned_butcher": horned_butcher, "lamplighter": lamplighter, "anvil": anvil,
    "spine_beast": spine_beast, "marionette": marionette, "giant": giant,
}

args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
for name in args or BUILDERS:
    BUILDERS[name]()
