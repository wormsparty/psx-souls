"""Generates assets/models/player.glb: low-poly character + animations.

blender -b --factory-startup -P tools/blender/player.py
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: E402,F403

reset_scene()
T = load_timings()["player"]

# ----------------------------------------------------------------------------- materials
COAT = material("coat", tex=tex_noise((0.36, 0.11, 0.09), 0.4, seed=11))
COAT_DARK = material("coat_dark", tex=tex_noise((0.2, 0.07, 0.06), 0.35, seed=12))
SHIRT = material("shirt", (0.86, 0.83, 0.74))
SKIN = material("porcelain", (0.9, 0.86, 0.8))
LEATHER = material("leather", tex=tex_noise((0.26, 0.17, 0.1), 0.4, seed=13))
TROUSERS = material("trousers", tex=tex_noise((0.17, 0.16, 0.2), 0.3, seed=14))
HAT = material("hat", (0.11, 0.09, 0.09))
BRASS = material("brass", (0.78, 0.6, 0.28))
EYE = material("eye", (0.95, 0.8, 0.35), emissive=(0.9, 0.7, 0.2))

# ----------------------------------------------------------------------------- model
rig = Rig("player")
R, L = -1, 1  # X sign of the right / left side


def hips_geo(mb):
    mb.box((0, 0, 0.95), (0.34, 0.2, 0.16), TROUSERS)
    # Coat tails (wider at the bottom).
    mb.box((0, 0.015, 0.72), (0.46, 0.3, 0.42), COAT, taper=(0.76, 0.72))
    mb.box((0, 0, 1.04), (0.34, 0.23, 0.05), LEATHER)
    mb.box((0, -0.125, 1.04), (0.06, 0.02, 0.045), BRASS)


def chest_geo(mb):
    mb.box((0, 0, 1.29), (0.34, 0.22, 0.42), COAT, taper=(1.25, 1.08))
    # Breastplate and buttons: clearly raised (≥ 1.5 cm) off the coat, otherwise
    # vertex snapping makes the two nearly coincident faces flicker.
    mb.box((0, -0.115, 1.3), (0.1, 0.04, 0.36), SHIRT, taper=(1.4, 1))
    mb.box((0, 0.01, 1.5), (0.24, 0.2, 0.08), COAT_DARK)
    for z in (1.2, 1.3, 1.4):
        mb.box((0.085, -0.125, z), (0.025, 0.02, 0.025), BRASS)


def head_geo(mb):
    mb.box((0, 0, 1.57), (0.09, 0.09, 0.07), SKIN)
    mb.box((0, -0.005, 1.67), (0.19, 0.21, 0.2), SKIN, taper=(0.92, 0.9))
    mb.box((-0.045, -0.114, 1.68), (0.035, 0.016, 0.02), EYE)
    mb.box((0.045, -0.114, 1.68), (0.035, 0.016, 0.02), EYE)
    mb.box((0, 0.025, 1.74), (0.205, 0.19, 0.08), HAT)
    mb.cylinder((0, 0, 1.78), 0.24, 0.02, HAT, sides=8)
    mb.cylinder((0, 0, 1.86), 0.12, 0.15, HAT, sides=8, radius_top=0.105)
    mb.cylinder((0, 0, 1.805), 0.122, 0.03, COAT, sides=8)


def arm_geo(side):
    x = 0.22 * side

    def upper(mb):
        mb.box((x, 0, 1.43), (0.14, 0.15, 0.08), COAT_DARK)
        mb.box((x, 0, 1.3), (0.1, 0.11, 0.28), COAT)

    def fore(mb):
        mb.box((x, 0, 1.05), (0.085, 0.095, 0.24), COAT, taper=(1.1, 1.1))
        mb.box((x, 0, 0.95), (0.1, 0.11, 0.05), SHIRT)

    def hand(mb):
        mb.box((x, -0.01, 0.875), (0.075, 0.09, 0.1), LEATHER)

    return upper, fore, hand


def leg_geo(side):
    x = 0.1 * side

    def thigh(mb):
        mb.box((x, 0, 0.71), (0.12, 0.13, 0.42), TROUSERS, taper=(1.25, 1.2))

    def shin(mb):
        mb.box((x, 0, 0.29), (0.11, 0.12, 0.42), LEATHER, taper=(1.12, 1.1))

    def foot(mb):
        mb.box((x, -0.05, 0.04), (0.11, 0.24, 0.08), LEATHER, taper=(1, 0.9))

    return thigh, shin, foot


rig.part("hips", (0, 0, 0.95), build=hips_geo)
rig.part("chest", (0, 0, 1.08), "hips", build=chest_geo)
rig.part("head", (0, 0, 1.52), "chest", build=head_geo)
for side, s in (("R", R), ("L", L)):
    up, fo, ha = arm_geo(s)
    x = 0.22 * s
    rig.part(f"upper_arm_{side}", (x, 0, 1.45), "chest", build=up)
    rig.part(f"forearm_{side}", (x, 0, 1.17), f"upper_arm_{side}", build=fo)
    rig.part(f"hand_{side}", (x, 0, 0.92), f"forearm_{side}", build=ha)
    rig.part(f"grip_{side}", (x, -0.01, 0.86), f"hand_{side}")
    th, sh, ft = leg_geo(s)
    lx = 0.1 * s
    rig.part(f"thigh_{side}", (lx, 0, 0.92), "hips", build=th)
    rig.part(f"shin_{side}", (lx, 0, 0.5), f"thigh_{side}", build=sh)
    rig.part(f"foot_{side}", (lx, 0, 0.08), f"shin_{side}", build=ft)

# ----------------------------------------------------------------------------- poses
# See common.py for the axis conventions. Tuples are angles (X, Y, Z) in degrees.


def hips(r=(0, 0, 0), t=(0, 0, 0)):
    return {"hips": {"r": r, "t": t}}


def legs(tr=-20, sr=25, tl=10, sl=20, drop=-0.04, twist=0, fwd=0.0, side=(0, 0)):
    """Legs: thighs/shins (X); the feet stay flat. `drop` lowers the pelvis."""
    return {
        "thigh_R": (tr, side[0], 0), "shin_R": (sr, 0, 0), "foot_R": (-(tr + sr), 0, 0),
        "thigh_L": (tl, side[1], 0), "shin_L": (sl, 0, 0), "foot_L": (-(tl + sl), 0, 0),
        "hips": {"r": (0, 0, twist), "t": (0, -fwd, drop)},
    }


def arm(side, upper, fore=(0, 0, 0), hand=(0, 0, 0)):
    return {f"upper_arm_{side}": upper, f"forearm_{side}": fore, f"hand_{side}": hand}


# Rapier guard: right side forward, point aimed at the opponent's face.
STANCE_R = merge(
    legs(-22, 28, 12, 22, -0.05, twist=22),
    {"chest": (6, 0, 8), "head": (0, 0, -28)},
    arm("R", (-42, 14, 0), (-30, 0, 0), (57, 0, 0)),
    arm("L", (22, -22, 0), (-75, 0, 0), (0, 0, 0)),
)
# Longsword guard: two-handed, blade pointing forward.
STANCE_G = merge(
    legs(-24, 30, 16, 26, -0.08, twist=14),
    {"chest": (14, 0, 8), "head": (-8, 0, -18)},
    arm("R", (-26, 10, 0), (-40, 0, -10), (78, 0, 0)),
    arm("L", (-36, 30, 0), (-42, 0, 10), (62, 0, 0)),
)
GUARD = merge(
    legs(-15, 22, 12, 20, -0.06, twist=8),
    {"chest": (8, 0, 12), "head": (0, 0, -12)},
    arm("R", (-62, 22, 0), (-28, 0, 0), (0, 0, 90)),
    arm("L", (-72, -8, 0), (-22, 0, 0), (0, 0, 0)),
)

STANCES = {"rapier": STANCE_R, "greatsword": STANCE_G}

markers = {}


def anim(name, keys, loop=False):
    info = T.get(name, {"total": 0, "hits": []})
    end = add_animation(rig, name, keys, info)
    marks = anim_markers(info) if name in T else [0, end]
    markers[name] = {"frames": end, "marks": marks, "loop": loop}


# ----------------------------------------------------------------------------- locomotion
for w, st in STANCES.items():
    breathe = merge(st, {"chest": (st["chest"][0] + 3, st["chest"][1], st["chest"][2])})
    anim(f"idle_{w}", [(0, st), (60, breathe), (120, st)], loop=True)


def run_cycle(period, thigh, shin_back, lean, bob, arm_r, swing):
    q = period // 4
    a = merge(legs(-thigh, 15, thigh * 0.8, shin_back, -0.03 + bob * 0), {"chest": (lean, 0, 0)},
              arm_r, arm("L", (-swing, -8, 0), (-40, 0, 0)))
    b = merge(legs(0, 15, -10, shin_back * 1.6, 0.0 + bob), {"chest": (lean, 0, 3)},
              arm_r, arm("L", (0, -8, 0), (-40, 0, 0)))
    c = merge(legs(thigh * 0.8, shin_back, -thigh, 15, -0.03), {"chest": (lean, 0, 0)},
              arm_r, arm("L", (swing, -8, 0), (-40, 0, 0)))
    d = merge(legs(-10, shin_back * 1.6, 0, 15, 0.0 + bob), {"chest": (lean, 0, -3)},
              arm_r, arm("L", (0, -8, 0), (-40, 0, 0)))
    return [(0, a), (q, b), (2 * q, c), (3 * q, d), (period, a)]


ARM_RUN = arm("R", (-30, 12, 0), (-45, 0, 0), (60, 0, 0))
anim("walk", run_cycle(64, 22, 18, 4, 0.01, ARM_RUN, 18), loop=True)
anim("run", run_cycle(40, 38, 40, 12, 0.025, ARM_RUN, 35), loop=True)
anim("sprint", run_cycle(30, 50, 60, 24, 0.035, arm("R", (35, 15, 0), (-35, 0, 0), (40, 0, 0)), 50), loop=True)
guard_walk = []
for t, p in run_cycle(64, 20, 18, 4, 0.01, {}, 0):
    guard_walk.append((t, merge(p, {k: v for k, v in GUARD.items() if not k.startswith(("thigh", "shin", "foot", "hips"))})))
anim("guard_walk", guard_walk, loop=True)
anim("guard", [(0, GUARD), (60, merge(GUARD, {"chest": (10, 0, 12)})), (120, GUARD)], loop=True)

# ----------------------------------------------------------------------------- generic
TUCK = merge(
    {"chest": (45, 0, 0), "head": (30, 0, 0)},
    legs(-110, 130, -100, 120, -0.45),
    arm("R", (-60, 10, 0), (-70, 0, 0), (60, 0, 0)),
    arm("L", (-60, -10, 0), (-70, 0, 0)),
)
anim("dodge", [
    (0, STANCE_R),
    (3, merge(TUCK, {"hips": {"r": (20, 0, 0), "t": (0, 0, -0.3)}})),
    (10, merge(TUCK, {"hips": {"r": (180, 0, 0), "t": (0, 0, -0.5)}})),
    (17, merge(TUCK, {"hips": {"r": (340, 0, 0), "t": (0, 0, -0.35)}})),
    (21, merge(legs(-40, 70, 20, 60, -0.25), {"chest": (25, 0, 0), "hips": {"r": (360, 0, 0), "t": (0, 0, -0.25)}},
               arm("R", (-40, 15, 0), (-40, 0, 0), (60, 0, 0)))),
    ("T", merge(STANCE_R, {"hips": {"r": (360, 0, 22), "t": (0, 0, -0.05)}})),
])
# Jump: push-off, legs tucked at the apex, then stretched towards the ground (the game holds the
# last pose until landing; ~0.55 s in the air).
AIR_ARMS = merge(arm("R", (-50, 25, 0), (-40, 0, 0), (60, 0, 0)), arm("L", (-40, -35, 0), (-30, 0, 0)))
anim("jump", [
    (0, merge(STANCE_R, legs(-40, 60, -30, 55, -0.18), {"chest": (18, 0, 6)})),
    (5, merge(legs(-25, 10, 15, 5, 0.02), {"chest": (4, 0, 4), "head": (-6, 0, 0)}, AIR_ARMS)),
    (14, merge(legs(-70, 95, -45, 85, 0.0), {"chest": (10, 0, 4), "head": (0, 0, 0)}, AIR_ARMS)),
    (26, merge(legs(-30, 35, -10, 25, 0.0), {"chest": (6, 0, 4)}, AIR_ARMS)),
    (34, merge(legs(-20, 20, 0, 15, 0.0), {"chest": (4, 0, 4)}, AIR_ARMS)),
])
anim("backstep", [
    (0, STANCE_R),
    (3, merge(STANCE_R, legs(-20, 50, 10, 50, -0.15, twist=10), {"chest": (15, 0, 0)})),
    (7, merge(STANCE_R, legs(10, 30, 30, 40, 0.06, twist=10), {"chest": (-8, 0, 0)})),
    (12, merge(STANCE_R, legs(-25, 55, 15, 55, -0.16, twist=15), {"chest": (12, 0, 0)})),
    ("T", STANCE_R),
])
anim("guard_hit", [
    (0, GUARD),
    (3, merge(GUARD, {"chest": (-10, 0, 14), "head": (-10, 0, -12)}, arm("R", (-50, 30, 0), (-45, 0, 0), (0, 0, 90)),
              legs(-5, 25, 25, 30, -0.08, twist=8))),
    ("T", GUARD),
])
anim("perfect_guard", [
    (0, GUARD),
    (2, merge(GUARD, {"chest": (14, 0, 4)}, arm("R", (-80, 10, 0), (-10, 0, 0), (0, 0, 80)),
              arm("L", (-80, -5, 0), (-10, 0, 0)))),
    ("T", GUARD),
])
FLUNG = merge({"chest": (-22, 0, 0), "head": (-20, 0, 0)},
              arm("R", (-150, 40, 0), (-20, 0, 0), (30, 0, 0)), arm("L", (-130, -40, 0), (-20, 0, 0)),
              legs(10, 30, 30, 40, -0.12))
anim("guard_break", [(0, GUARD), (6, FLUNG), (40, merge(FLUNG, {"chest": (10, 0, 0)})), ("T", STANCE_R)])
HIT = merge(STANCE_R, {"chest": (-22, 0, 10), "head": (-25, 0, 0)}, arm("L", (-40, -40, 0), (-30, 0, 0)),
            legs(-5, 25, 25, 35, -0.08, twist=15))
anim("hit_light", [(0, STANCE_R), (4, HIT), (12, merge(HIT, {"chest": (-10, 0, 10)})), ("T", STANCE_R)])
DOWN = merge(legs(-50, 30, -30, 50, 0), {"hips": {"r": (-85, 0, 0), "t": (0, 0.35, -0.78)}, "chest": (-5, 0, 0), "head": (20, 0, 0)},
             arm("R", (-40, 50, 0), (-20, 0, 0), (40, 0, 0)), arm("L", (-20, -50, 0)))
KNEEL = merge(legs(-80, 90, 70, 100, -0.45), {"chest": (30, 0, 0)}, arm("R", (-20, 10, 0), (-40, 0, 0), (60, 0, 0)))
anim("hit_heavy", [
    (0, STANCE_R),
    (5, merge(HIT, {"hips": {"r": (-30, 0, 0), "t": (0, 0.15, -0.1)}})),
    (16, DOWN), (45, DOWN), (58, KNEEL), (72, merge(STANCE_R, {"chest": (20, 0, 8)})), ("T", STANCE_R),
])
SHEATHE = merge(STANCE_R, arm("R", (-10, -35, 0), (-60, 0, -20), (60, 0, 0)), {"chest": (6, 0, 25)})
anim("switch", [(0, STANCE_R), (11, SHEATHE), (13, SHEATHE), ("T", STANCE_R)])
anim("death", [
    (0, HIT), (25, KNEEL),
    (55, merge(KNEEL, legs(-90, 0, -80, 10, 0), {"hips": {"r": (80, 0, 10), "t": (0, -0.4, -0.75)}, "chest": (10, 0, 0)},
               arm("R", (-170, 20, 0)), arm("L", (-160, -30, 0)))),
    ("T", merge(KNEEL, legs(-90, 0, -80, 10, 0), {"hips": {"r": (88, 0, 10), "t": (0, -0.42, -0.8)}, "chest": (5, 0, 0)},
                arm("R", (-170, 20, 0)), arm("L", (-160, -30, 0)))),
])

# Heal: the left hand brings the flask to the chest.
DRINK = merge(STANCE_R, {"chest": (2, 0, 14), "head": (8, 0, -14)},
              arm("L", (-55, 30, 0), (-105, 0, 0), (0, 0, 0)),
              arm("R", (-20, 12, 0), (-40, 0, 0), (60, 0, 0)))
anim("heal", [
    (0, STANCE_R), (10, DRINK),
    (22, merge(DRINK, {"chest": (-6, 0, 14), "head": (-12, 0, -14)})),
    (30, merge(DRINK, {"chest": (6, 0, 14)})), ("T", STANCE_R),
])

# ----------------------------------------------------------------------------- rapier
LUNGE = legs(-50, 45, 30, 8, -0.12, twist=25, fwd=0.12)
PULL_R = merge(STANCE_R, {"chest": (2, 0, 30)}, arm("R", (-15, 18, 0), (-85, 0, 0), (85, 0, 0)))


def thrust(height=0.0, lunge=LUNGE, twist=-4):
    return merge(lunge, {"chest": (10 + height, 0, twist), "head": (0, 0, -20)},
                 arm("R", (-86 + height, 6, 0), (-4, 0, 0), (90 - height, 0, 0)),
                 arm("L", (35, -30, 0), (-60, 0, 0)))


def thrust_anim(name, height=0.0, lunge=LUNGE):
    anim(name, [
        (0, STANCE_R), ("h0-3", PULL_R), ("h0", thrust(height, lunge)), ("h0e", thrust(height, lunge)),
        ("h0e+6", merge(STANCE_R, {"chest": (8, 0, 4)})), ("T", STANCE_R),
    ])


thrust_anim("rapier_light1", 0)
thrust_anim("rapier_light2", 12)
anim("rapier_light3", [
    (0, STANCE_R),
    ("h0-3", merge(STANCE_R, {"chest": (6, 0, -35)}, arm("R", (-80, 25, 0), (-10, 0, 0), (0, 0, 80)))),
    ("h0", merge(legs(-30, 35, 20, 15, -0.08, twist=10), {"chest": (8, 0, -25)},
                 arm("R", (-85, 10, 0), (-5, 0, 0), (0, 0, 90)))),
    ("h0e", merge(legs(-30, 35, 20, 15, -0.08, twist=10), {"chest": (8, 0, 35)},
                  arm("R", (-85, 10, 0), (-5, 0, 0), (0, 0, 90)))),
    ("h0e+5", merge(STANCE_R, {"chest": (6, 0, 40)})),
    ("T", STANCE_R),
])
BIG_LUNGE = legs(-65, 55, 40, 5, -0.2, twist=28, fwd=0.25)
thrust_anim("rapier_light4", 4, BIG_LUNGE)
DEEP_PULL = merge(STANCE_R, legs(-10, 40, 25, 35, -0.12, twist=35), {"chest": (-4, 0, 40)},
                  arm("R", (0, 22, 0), (-100, 0, 0), (95, 0, 0)))
anim("rapier_charge", [(0, STANCE_R), ("T", DEEP_PULL)])
anim("rapier_heavy", [
    (0, STANCE_R), ("h0-7", DEEP_PULL), ("h0", thrust(0, BIG_LUNGE)), ("h0e+2", thrust(0, BIG_LUNGE)),
    ("h0e+10", merge(STANCE_R, {"chest": (10, 0, 0)})), ("T", STANCE_R),
])
anim("rapier_heavy_charged", [
    (0, DEEP_PULL), ("h0", thrust(-4, BIG_LUNGE)), ("h0e+3", thrust(-4, BIG_LUNGE)),
    ("h0e+12", merge(STANCE_R, {"chest": (10, 0, 0)})), ("T", STANCE_R),
])
DASH = merge(legs(-40, 60, 35, 40, -0.2, twist=15, fwd=0.1), {"chest": (35, 0, 15)},
             arm("R", (-50, 20, 0), (-60, 0, 0), (90, 0, 0)), arm("L", (40, -20, 0), (-30, 0, 0)))
special = [(0, STANCE_R), (4, merge(DASH, {"chest": (20, 0, 20)})), (12, DASH)]
for i, h in enumerate((8, -6, 14)):
    special += [(f"h{i}-2", merge(BIG_LUNGE, PULL_R)), (f"h{i}", thrust(h, BIG_LUNGE)), (f"h{i}e", thrust(h, BIG_LUNGE))]
special += [("h3-4", merge(DEEP_PULL, BIG_LUNGE)), ("h3", thrust(0, BIG_LUNGE)), ("h3e+4", thrust(0, BIG_LUNGE)),
            ("T", STANCE_R)]
anim("rapier_special", special)
HIGH_R = merge(STANCE_R, {"chest": (-10, 0, 20)}, arm("R", (-160, 20, 0), (-20, 0, 0), (60, 0, 0)))
STAB_DOWN = merge(legs(-50, 60, 30, 30, -0.25, twist=10, fwd=0.1), {"chest": (35, 0, 0)},
                  arm("R", (-70, 5, 0), (-10, 0, 0), (110, 0, 0)), arm("L", (-60, -10, 0), (-20, 0, 0)))
anim("rapier_fatal", [
    (0, STANCE_R), (24, HIGH_R), ("h0-4", HIGH_R), ("h0", STAB_DOWN), ("h0e+20", STAB_DOWN),
    ("h0e+32", merge(STANCE_R, legs(-10, 20, 30, 20, -0.05, twist=22))), ("T", STANCE_R),
])

# ----------------------------------------------------------------------------- longsword
ARMS_FWD = merge(arm("R", (-82, 6, 0), (-8, 0, 0), (90, 0, 0)), arm("L", (-80, 22, 0), (-12, 0, 0), (90, 0, 0)))
STEP = legs(-35, 30, 22, 14, -0.1, twist=0, fwd=0.08)


def sweep(z_from, z_to, name):
    wind = merge(STANCE_G, {"chest": (4, 0, z_from * 1.15), "head": (0, 0, -z_from * 0.6)},
                 arm("R", (-75, 10, z_from * 0.4), (-20, 0, 0), (90, 0, 0)),
                 arm("L", (-70, 20, z_from * 0.4), (-25, 0, 0), (90, 0, 0)))
    hit_a = merge(STEP, {"chest": (10, 0, z_from), "head": (0, 0, -z_from * 0.5)}, ARMS_FWD)
    hit_b = merge(STEP, {"chest": (10, 0, z_to), "head": (0, 0, -z_to * 0.5)}, ARMS_FWD)
    follow = merge(STEP, {"chest": (16, 0, z_to * 1.25)},
                   arm("R", (-55, 10, z_to * 0.3), (-20, 0, 0), (100, 0, 0)),
                   arm("L", (-55, 20, z_to * 0.3), (-20, 0, 0), (100, 0, 0)))
    anim(name, [(0, STANCE_G), ("h0-6", wind), ("h0", hit_a), ("h0e", hit_b), ("h0e+8", follow), ("T", STANCE_G)])


sweep(-65, 65, "greatsword_light1")
sweep(65, -65, "greatsword_light2")
OVERHEAD = merge(legs(-12, 25, 18, 25, -0.06), {"chest": (-12, 0, 0), "head": (-6, 0, 0)},
                 arm("R", (-168, 6, 0), (-12, 0, 0), (40, 0, 0)), arm("L", (-165, 18, 0), (-15, 0, 0), (40, 0, 0)))
CHOP = merge(legs(-45, 40, 30, 12, -0.16, fwd=0.12), {"chest": (28, 0, 0), "head": (-10, 0, 0)},
             arm("R", (-62, 6, 0), (-4, 0, 0), (82, 0, 0)), arm("L", (-60, 20, 0), (-6, 0, 0), (82, 0, 0)))
CHOP_LOW = merge(CHOP, {"chest": (36, 0, 0)}, arm("R", (-30, 6, 0), (-4, 0, 0), (80, 0, 0)),
                 arm("L", (-28, 20, 0), (-6, 0, 0), (80, 0, 0)))


def chop(name, wind_at="h0-8", wind=OVERHEAD, start=STANCE_G):
    anim(name, [(0, start), (wind_at, wind), ("h0", CHOP), ("h0e", CHOP_LOW), ("h0e+12", CHOP_LOW), ("T", STANCE_G)])


chop("greatsword_light3")
chop("greatsword_heavy", "h0-12")
SHOULDER = merge(legs(-10, 35, 25, 35, -0.12, twist=30), {"chest": (-6, 0, 35), "head": (0, 0, -30)},
                 arm("R", (-140, 30, 0), (-40, 0, 0), (40, 0, 0)), arm("L", (-120, 10, 0), (-40, 0, 0), (40, 0, 0)))
anim("greatsword_charge", [(0, STANCE_G), ("T", SHOULDER)])
chop("greatsword_heavy_charged", "h0-6", SHOULDER, SHOULDER)
PLUNGE_UP = merge(legs(-20, 30, 20, 30, -0.05), {"chest": (-15, 0, 0)},
                  arm("R", (-175, 6, 0), (-10, 0, 0), (180, 0, 0)), arm("L", (-175, 18, 0), (-10, 0, 0), (180, 0, 0)))
PLUNGE = merge(legs(-50, 60, 30, 30, -0.25, fwd=0.1), {"chest": (40, 0, 0)},
               arm("R", (-80, 6, 0), (-5, 0, 0), (170, 0, 0)), arm("L", (-80, 18, 0), (-5, 0, 0), (170, 0, 0)))
anim("greatsword_fatal", [
    (0, STANCE_G), (26, PLUNGE_UP), ("h0-4", PLUNGE_UP), ("h0", PLUNGE), ("h0e+22", PLUNGE),
    ("h0e+34", merge(STANCE_G, {"chest": (20, 0, 0)})), ("T", STANCE_G),
])

# ----------------------------------------------------------------------------- jump attacks
# In the air: legs tucked, weapon raised, then brought down while falling.
AIR_LEGS = legs(-60, 80, -35, 70, 0.0)
anim("rapier_jump", [
    (0, merge(HIGH_R, AIR_LEGS)), ("h0-3", merge(HIGH_R, AIR_LEGS, {"chest": (-14, 0, 20)})),
    ("h0", merge(STAB_DOWN, legs(-40, 50, -10, 40, -0.1))), ("h0e+8", STAB_DOWN), ("T", STANCE_R),
])
anim("greatsword_jump", [
    (0, merge(OVERHEAD, AIR_LEGS)), ("h0-4", merge(OVERHEAD, AIR_LEGS, {"chest": (-20, 0, 0)})),
    ("h0", merge(CHOP, {"chest": (34, 0, 0)})), ("h0e", CHOP_LOW), ("h0e+16", CHOP_LOW), ("T", STANCE_G),
])

# Greatsword special: crouch, leap with the blade raised, hang at the top, vertical cleave
# on the way down, landing crouched with the blade in the ground.
CROUCH = merge(legs(-70, 95, 45, 85, -0.32, fwd=0.08), {"chest": (30, 0, 0), "head": (-20, 0, 0)},
               arm("R", (-25, 10, 0), (-30, 0, 0), (85, 0, 0)), arm("L", (-22, 24, 0), (-32, 0, 0), (85, 0, 0)))
TAKEOFF = merge(legs(-15, 10, 25, 30, 0.0), {"chest": (-5, 0, 0), "head": (-10, 0, 0)},
                arm("R", (-130, 8, 0), (-15, 0, 0), (50, 0, 0)), arm("L", (-128, 20, 0), (-18, 0, 0), (50, 0, 0)))
APEX = merge(OVERHEAD, AIR_LEGS, {"chest": (-24, 0, 0), "head": (-4, 0, 0)},
             arm("R", (-175, 4, 0), (-20, 0, 0), (25, 0, 0)), arm("L", (-172, 16, 0), (-22, 0, 0), (25, 0, 0)))
CLEAVE = merge(CHOP, legs(-40, 30, 10, 25, -0.02), {"chest": (32, 0, 0)})
LANDED = merge(CHOP_LOW, legs(-65, 85, 40, 80, -0.3, fwd=0.12), {"chest": (42, 0, 0)})
anim("greatsword_special", [
    (0, STANCE_G), (5, CROUCH), (12, TAKEOFF), (20, APEX), ("h0-1", merge(APEX, {"chest": (-28, 0, 0)})),
    ("h0+3", CLEAVE), ("h0+6", LANDED), ("h0e+14", LANDED), ("T", STANCE_G),
])

missing = sorted(set(T) - set(markers))
if missing:
    raise SystemExit(f"missing animations: {missing}")

rest_pose(rig)
export("player", markers)
