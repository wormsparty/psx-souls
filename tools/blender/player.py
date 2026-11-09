"""Generates assets/models/player.glb: low-poly character + animations.

blender -b --factory-startup -P tools/blender/player.py
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: E402,F403
from humanoid import bake, foot, hand, hilt, pose, tweak  # noqa: E402

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


def coat_geo(side):
    """One half of the coat's skirt (wider at the bottom): split down the middle, each half is its
    own piece swinging with its thigh, so a lunge opens the coat instead of going through it."""
    def tail(mb):
        mb.box((0.117 * side, 0.015, 0.72), (0.23, 0.3, 0.42), COAT, taper=(0.76, 0.72),
               shift_top=(-0.0275 * side, 0))

    return tail


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
    rig.part(f"coat_{side}", (lx, 0, 0.93), "hips", build=coat_geo(s))

# ----------------------------------------------------------------------------- poses
# Poses say where the pelvis, the feet and the hands are; the limbs are solved (see humanoid.py).
# Coordinates are (right, forwards, up) in metres, angles in degrees (yaw positive to the left).
# Durations and hit frames come from the tuning (timings.json): the animations are written
# around them and never change them.


def torso(drop=0.0, fwd=0.0, side=0.0, yaw=0.0, lean=0.0, roll=0.0, twist=0.0, bend=0.0, tilt=0.0, nod=0.0, look=0.9):
    """Pelvis (`drop`, `fwd`, `side`, `yaw`, `lean`, `roll`) and chest on top of it (`twist`,
    `bend`, `tilt`). The head undoes most of it to keep facing the opponent."""
    return {"hips": (side, fwd, -drop), "hips_rot": (lean, roll, yaw), "chest": (bend, tilt, twist),
            "head": (nod - 0.5 * (lean + bend), 0, -(yaw + twist) * look)}


def plant(p, *sides):
    """The same pose with these feet planted in the world: they stay where they are on the
    ground while the move's root motion carries the body forwards."""
    return pose(p, **{f"foot_{s}": tweak(p[f"foot_{s}"], space="world") for s in sides or ("R", "L")})


# Rapier guard: right side leading, feet a good stride apart on one line, rear foot turned out,
# point aimed at the opponent's face, off hand held back for balance.
STANCE_R = pose(
    torso(drop=0.08, yaw=32, twist=14, bend=5),
    foot_R=foot(0.10, 0.27), foot_L=foot(-0.13, -0.25, yaw=65),
    hand_R=hand(0.17, 0.50, 1.13, aim=(4, 14, 0)),
    hand_L=hand(-0.27, -0.20, 1.02),
)
# Greatsword guard: left foot leading, both hands on the grip at the right hip, blade up ahead.
STANCE_G = pose(
    torso(drop=0.10, yaw=-24, twist=-8, bend=8),
    foot_L=foot(-0.12, 0.26), foot_R=foot(0.16, -0.25, yaw=-55),
    hand_R=hand(0.15, 0.26, 0.98, aim=(10, 34, 0)),
    hand_L=hilt(),
)
# Block: square on the feet, blade held across the body, off hand bracing behind it.
GUARD = pose(
    torso(drop=0.11, yaw=10, twist=6, bend=10),
    foot_R=foot(0.15, 0.19), foot_L=foot(-0.17, -0.18, yaw=35),
    hand_R=hand(0.27, 0.42, 1.40, aim=(88, 8, 0)),
    hand_L=hand(-0.12, 0.38, 1.33, elbow=(-0.8, -0.2, -0.6)),
)

STANCES = {"rapier": STANCE_R, "greatsword": STANCE_G}

markers = {}


def anim(name, keys, loop=False):
    info = T.get(name, {"total": 0, "hits": []})
    end = bake(rig, name, keys, info, loop)
    marks = anim_markers(info) if name in T else [0, end]
    markers[name] = {"frames": end, "marks": marks, "loop": loop}


# ----------------------------------------------------------------------------- locomotion
for w, st in STANCES.items():
    hr = st["hand_R"]
    breathe = pose(st, hips=(0, 0, st["hips"][2] - 0.012), chest=(st["chest"][0] + 2.5, 0, st["chest"][2]),
                   hand_R=tweak(hr, d=(0, -0.01, 0.012), aim=(hr["aim"][0], hr["aim"][1] + 2, 0)))
    anim(f"idle_{w}", [(0, st), (60, breathe), (120, st)], loop=True)


def gait(period, stride, duty, drop, bob, lean, lift, width, turn, sway, arms, kick=0.0, heel=35.0, toe=12.0):
    """One key per frame of a walk/run cycle. `stride`: metres a foot spends on the ground,
    `duty`: share of the cycle it spends there (under 0.5: flight phases, a run), `bob` > 0: the
    pelvis is highest over the standing foot (walk), < 0: lowest (run). `arms(phase)` → hands."""
    keys = []
    for f in range(period + 1):
        ph = f / period
        feet = {}
        for side, off, r in (("R", 0.0, width), ("L", 0.5, -width)):
            p = (ph + off) % 1.0
            if p < duty:
                q = p / duty
                at, u = stride * (0.5 - q), 0.0
                h = heel * max(0.0, (q - 0.5) / 0.5) ** 1.5 - toe * max(0.0, 1 - q / 0.2)
            else:
                q = (p - duty) / (1 - duty)
                s = q * q * (3 - 2 * q)
                at = stride * (s - 0.5)
                u = lift * math.sin(math.pi * q) + kick * math.sin(math.pi * min(q * 1.8, 1.0))
                h = heel * (1 - q) ** 2 + (kick * 120) * math.sin(math.pi * min(q * 1.6, 1.0)) - toe * q ** 3
            feet[f"foot_{side}"] = foot(r, at, u, heel=h)
        a = 2 * math.pi * (ph - duty / 2)
        c = math.cos(2 * math.pi * ph)
        keys.append((f, pose(
            torso(drop=drop - bob * math.cos(2 * a), side=sway * math.cos(a), yaw=turn * c, twist=-1.9 * turn * c,
                  lean=lean * 0.4, bend=lean * 0.6, roll=-2.5 * math.cos(a) * (1 if bob > 0 else -1), look=1.0),
            feet, arms(ph)), "lin"))
    return keys


def carry(swing, low=0.0):
    """Arms while moving: the weapon hand rides ahead of the hip, the off arm swings."""
    def arms(ph):
        c, s = math.cos(2 * math.pi * ph), math.sin(2 * math.pi * ph)
        return {
            "hand_R": hand(0.27, 0.22 - 0.3 * swing * c, 0.0 - low + 0.02 * s, aim=(-6, -10 - 30 * low, 0), space="chest"),
            "hand_L": hand(-0.27, 0.06 + swing * c, -0.07 + 0.25 * swing * abs(c), space="chest"),
        }
    return arms


def trail(ph):
    """Sprint: the blade trails behind, the off arm pumps."""
    c = math.cos(2 * math.pi * ph)
    return {
        "hand_R": hand(0.31, -0.12 - 0.06 * c, -0.14, aim=(-172, -34, 0), space="chest"),
        "hand_L": hand(-0.26, 0.10 + 0.24 * c, -0.02 + 0.08 * abs(c), space="chest"),
    }


def guard_arms(ph):
    return {"hand_R": GUARD["hand_R"], "hand_L": GUARD["hand_L"]}


# The game plays `walk` at 1.6 m/s, `run` at 4.2, `sprint` at 6.4 and `guard_walk` at 1.8 (and
# scales from there): strides match those speeds, the feet don't skate.
anim("walk", gait(52, 0.80, 0.58, 0.05, 0.012, 5, 0.07, 0.09, 5, 0.02, carry(0.12)), loop=True)
anim("run", gait(36, 0.82, 0.33, 0.09, -0.03, 16, 0.10, 0.08, 9, 0.0, carry(0.22, 0.04), kick=0.14), loop=True)
anim("sprint", gait(28, 0.80, 0.27, 0.12, -0.035, 30, 0.12, 0.07, 11, 0.0, trail, kick=0.2), loop=True)
guard_walk = []
for f, p, e in gait(52, 0.62, 0.6, 0.12, 0.008, 0, 0.05, 0.15, 3, 0.02, guard_arms):
    guard_walk.append((f, pose(p, chest=(GUARD["chest"][0], 0, GUARD["chest"][2] + p["chest"][2] * 0.4),
                               hips_rot=(0, p["hips_rot"][1], GUARD["hips_rot"][2] + p["hips_rot"][2]),
                               head=GUARD["head"]), e))
anim("guard_walk", guard_walk, loop=True)
anim("guard", [(0, GUARD), (60, pose(GUARD, chest=(12.5, 0, 6), hips=(0, 0, -0.122))), (120, GUARD)], loop=True)

# ----------------------------------------------------------------------------- generic
# Roll: dive onto the hands, go round tucked into a ball, come up on one knee and rise.
TUCK_HANDS = {"hand_R": hand(0.2, 0.32, 0.08, aim=(-25, 35, 0), space="chest"),
              "hand_L": hand(-0.2, 0.34, 0.06, space="chest")}
TUCK_FEET = {"foot_R": foot(0.1, 0.26, -0.36, space="hips"), "foot_L": foot(-0.1, 0.22, -0.4, space="hips")}
anim("dodge", [
    (0, STANCE_R),
    (3, pose({"hips": (0, 0.14, -0.22), "hips_rot": (62, 0, 0), "chest": (42, 0, 0), "head": (30, 0, 0)},
             foot_R=foot(0.1, -0.16, -0.76, space="hips", heel=50), foot_L=foot(-0.1, -0.02, -0.6, space="hips", heel=30),
             hand_R=hand(0.24, 0.46, 0.0, aim=(-40, 50, 0), space="chest"),
             hand_L=hand(-0.2, 0.5, -0.04, space="chest")), "out"),
    (8, pose({"hips": (0, 0, -0.46), "hips_rot": (170, 0, 0), "chest": (58, 0, 0), "head": (38, 0, 0)},
             TUCK_FEET, TUCK_HANDS), "lin"),
    (13, pose({"hips": (0, 0, -0.84), "hips_rot": (270, 0, 0), "chest": (52, 0, 0), "head": (30, 0, 0)},
              TUCK_FEET, TUCK_HANDS), "lin"),
    (17, pose({"hips": (0, 0, -0.52), "hips_rot": (335, 0, 6), "chest": (44, 0, 4), "head": (0, 0, -8)},
              foot_R=foot(0.13, 0.26, heel=-10), foot_L=foot(-0.13, -0.16, yaw=40, heel=50),
              hand_R=hand(0.26, 0.3, 0.1, aim=(-14, 30, 0), space="chest"), hand_L=hand(-0.24, 0.3, 0.0, space="chest")), "lin"),
    (21, pose({"hips": (0, 0.04, -0.36), "hips_rot": (360, 0, 16), "chest": (30, 0, 8), "head": (-14, 0, -20)},
              foot_R=foot(0.13, 0.26), foot_L=foot(-0.13, -0.2, yaw=40, heel=45),
              hand_R=hand(0.26, 0.4, 0.8, aim=(-6, 14, 0)), hand_L=hand(-0.3, 0.1, 0.66)), "out"),
    ("T", pose(STANCE_R, hips_rot=(360, 0, 32))),
])
# Jump: push-off, legs tucked at the apex, then stretched towards the ground (the game holds the
# last pose until landing; ~0.55 s in the air).
AIR_ARMS = {"hand_R": hand(0.36, 0.2, 0.2, aim=(-12, 25, 0), space="chest"),
            "hand_L": hand(-0.42, 0.12, 0.12, space="chest", elbow=(-1, -0.3, -0.3))}
anim("jump", [
    (0, pose(torso(drop=0.22, bend=20, yaw=10), foot_R=foot(0.11, 0.1), foot_L=foot(-0.11, -0.1, heel=20),
             hand_R=hand(0.3, 0.05, -0.05, aim=(-10, -10, 0), space="chest"), hand_L=hand(-0.3, -0.12, -0.05, space="chest"))),
    (5, pose(torso(drop=-0.02, bend=-4, nod=-8), AIR_ARMS,
             foot_R=foot(0.1, -0.06, -0.8, space="hips", heel=50), foot_L=foot(-0.1, -0.16, -0.76, space="hips", heel=55)), "out"),
    (15, pose(torso(bend=10), foot_R=foot(0.11, 0.28, -0.46, space="hips", heel=10),
              foot_L=foot(-0.11, -0.12, -0.5, space="hips", heel=40),
              hand_R=hand(0.4, 0.14, 0.3, aim=(-14, 40, 0), space="chest"),
              hand_L=hand(-0.46, 0.08, 0.26, space="chest", elbow=(-1, -0.3, -0.3)))),
    (27, pose(torso(bend=6), AIR_ARMS, foot_R=foot(0.11, 0.2, -0.66, space="hips"),
              foot_L=foot(-0.11, -0.05, -0.7, space="hips", heel=25))),
    (34, pose(torso(bend=8), foot_R=foot(0.11, 0.16, -0.76, space="hips", heel=-8),
              foot_L=foot(-0.11, 0.0, -0.78, space="hips", heel=15),
              hand_R=hand(0.38, 0.22, 0.1, aim=(-10, 15, 0), space="chest"),
              hand_L=hand(-0.44, 0.1, 0.02, space="chest", elbow=(-1, -0.3, -0.3)))),
])
# Backstep: a hop off the front foot, landing on the rear one with the body folding over it.
anim("backstep", [
    (0, STANCE_R),
    (2, pose(torso(drop=0.17, fwd=0.03, yaw=30, twist=10, bend=16), foot_R=tweak(STANCE_R["foot_R"], heel=10)), "out"),
    (6, pose(torso(drop=0.02, fwd=-0.05, yaw=28, twist=8, bend=-6),
             foot_R=foot(0.1, 0.34, -0.68, space="hips", heel=45), foot_L=foot(-0.1, -0.16, -0.7, space="hips", heel=20),
             hand_R=hand(0.22, 0.52, 1.2, aim=(2, 22, 0)), hand_L=hand(-0.36, -0.1, 1.12)), "out"),
    (12, pose(torso(drop=0.2, fwd=-0.06, yaw=34, twist=12, bend=18),
              foot_R=foot(0.1, 0.3), foot_L=foot(-0.14, -0.3, yaw=65),
              hand_R=hand(0.18, 0.5, 1.02, aim=(4, 18, 0)), hand_L=hand(-0.3, -0.12, 0.9)), "in"),
    ("T", STANCE_R),
])
# Blocked hit: the blow drives the blade in and the body back onto the rear leg.
anim("guard_hit", [
    (0, GUARD),
    (3, pose(torso(drop=0.17, fwd=-0.08, yaw=14, twist=12, bend=-4, nod=6),
             foot_R=tweak(GUARD["foot_R"], heel=-10), foot_L=tweak(GUARD["foot_L"], d=(0, -0.06, 0)),
             hand_R=hand(0.24, 0.28, 1.36, aim=(84, 16, 0)),
             hand_L=hand(-0.12, 0.24, 1.3, elbow=(-0.8, -0.2, -0.6))), "snap"),
    (9, pose(torso(drop=0.14, fwd=-0.03, yaw=11, twist=8, bend=8), hand_R=tweak(GUARD["hand_R"], d=(0, -0.04, 0)))),
    ("T", GUARD),
])
# Perfect guard: the blade snaps out to meet the blow and throws it aside.
anim("perfect_guard", [
    (0, GUARD),
    (2, pose(torso(drop=0.13, fwd=0.07, yaw=2, twist=-10, bend=14),
             foot_R=tweak(GUARD["foot_R"], d=(0, 0.05, 0)),
             hand_R=hand(0.42, 0.56, 1.5, aim=(64, 30, 0)),
             hand_L=hand(-0.24, 0.3, 1.2, elbow=(-0.8, -0.2, -0.6))), "snap"),
    (6, pose(torso(drop=0.12, fwd=0.04, yaw=6, twist=-2, bend=12), hand_R=hand(0.4, 0.52, 1.46, aim=(70, 22, 0)))),
    ("T", GUARD),
])
# Guard break: the arms are thrown open, two stumbling steps back before the guard comes back.
FLUNG = pose(
    torso(drop=0.1, fwd=-0.1, yaw=-6, twist=-10, bend=-24, nod=-14),
    hand_R=hand(0.62, 0.02, 1.52, aim=(-70, 50, 0), elbow=(1, -0.3, -0.2)),
    hand_L=hand(-0.58, 0.0, 1.36, elbow=(-1, -0.3, -0.2)),
)
anim("guard_break", [
    (0, GUARD),
    (6, pose(FLUNG, foot_R=foot(0.15, 0.24, heel=-14), foot_L=foot(-0.2, -0.3, yaw=35, lift=0.05)), "snap"),
    (20, pose(torso(drop=0.16, fwd=-0.04, yaw=4, twist=-4, bend=-12, nod=-8),
              foot_R=foot(0.17, -0.02, lift=0.05), foot_L=foot(-0.2, -0.3, yaw=35),
              hand_R=hand(0.56, 0.1, 1.2, aim=(-50, 10, 0), elbow=(1, -0.3, -0.4)), hand_L=hand(-0.5, 0.05, 1.1))),
    (38, pose(torso(drop=0.2, yaw=14, twist=6, bend=22, nod=10),
              foot_R=foot(0.17, 0.0), foot_L=foot(-0.2, -0.3, yaw=35),
              hand_R=hand(0.34, 0.3, 0.8, aim=(-10, -18, 0)), hand_L=hand(-0.3, 0.14, 0.78))),
    ("T", pose(STANCE_R, foot_R=tweak(STANCE_R["foot_R"], lift=0.04))),
])
# Hit: the chest is knocked back and round, the rear foot catches the weight.
HIT = pose(
    torso(drop=0.13, fwd=-0.1, yaw=18, twist=-12, bend=-22, tilt=8, nod=-16),
    foot_R=foot(0.1, 0.27, heel=-12), foot_L=foot(-0.16, -0.36, yaw=60, lift=0.04),
    hand_R=hand(0.36, 0.3, 1.2, aim=(-26, 36, 0)),
    hand_L=hand(-0.5, -0.06, 1.22, elbow=(-1, -0.3, -0.3)),
)
anim("hit_light", [
    (0, STANCE_R), (4, HIT, "snap"),
    (12, pose(torso(drop=0.17, fwd=-0.05, yaw=24, twist=4, bend=10, nod=8),
              foot_R=foot(0.1, 0.2, lift=0.03), foot_L=foot(-0.16, -0.36, yaw=60),
              hand_R=hand(0.28, 0.36, 1.0, aim=(-8, 6, 0)), hand_L=hand(-0.36, -0.1, 0.96))),
    ("T", STANCE_R),
])
# Heavy hit: thrown off the feet onto the back, a moment on the ground, then up through one knee.
# (The game also freezes this clip around frame 7 while the character falls into the void.)
DOWN = pose(
    {"hips": (0, -0.3, -0.8), "hips_rot": (-86, 0, 6), "chest": (-4, 0, 0), "head": (18, 0, 10)},
    foot_R=foot(0.14, 0.2, -0.7, space="hips", heel=30), foot_L=foot(-0.18, -0.12, -0.6, space="hips", heel=20, knee=25),
    hand_R=hand(0.62, 0.0, 0.2, aim=(-80, 0, 0), space="chest", elbow=(1, 0, -0.2)),
    hand_L=hand(-0.55, 0.05, 0.05, space="chest", elbow=(-1, 0, -0.2)),
)
KNEEL = pose(
    torso(drop=0.46, yaw=20, twist=8, bend=26, nod=14),
    foot_R=foot(0.13, 0.26), foot_L=foot(-0.14, -0.3, 0.06, heel=75),
    hand_R=hand(0.3, 0.36, 0.5, aim=(-10, -20, 0)), hand_L=hand(-0.3, 0.1, 0.36),
)
anim("hit_heavy", [
    (0, STANCE_R),
    (6, pose({"hips": (0, -0.12, 0.02), "hips_rot": (-34, 0, 10), "chest": (-20, 0, -6), "head": (-22, 0, 0)},
             foot_R=foot(0.12, 0.3, -0.62, space="hips", heel=40), foot_L=foot(-0.14, 0.05, -0.74, space="hips", heel=40),
             hand_R=hand(0.56, 0.2, 0.42, aim=(-40, 50, 0), space="chest", elbow=(1, -0.3, -0.2)),
             hand_L=hand(-0.56, 0.18, 0.36, space="chest", elbow=(-1, -0.3, -0.2))), "snap"),
    (16, DOWN, "in"),
    (20, pose(DOWN, hips=(0, -0.3, -0.76), chest=(4, 0, 0), head=(26, 0, 10))),
    (26, DOWN), (45, DOWN),
    (58, pose(KNEEL, hand_L=hand(-0.34, 0.0, 0.3))),
    (72, pose(STANCE_R, torso(drop=0.16, yaw=30, twist=10, bend=20))),
    ("T", STANCE_R),
])
# Weapon switch: the hand goes to the back of the left hip, the weapons swap there, and returns.
SHEATHE = pose(
    torso(drop=0.1, yaw=18, twist=34, bend=8, look=0.7),
    hand_R=hand(-0.2, -0.04, 0.96, aim=(150, -30, 0), elbow=(0.7, 0.1, -0.7)),
    hand_L=hand(-0.3, -0.12, 0.98),
)
anim("switch", [(0, STANCE_R), (10, SHEATHE, "out"), (13, SHEATHE), ("T", STANCE_R)])
# Death: the knees give way, then the body pitches forward onto the ground.
PRONE = pose(
    {"hips": (0, 0.38, -0.8), "hips_rot": (86, 0, 10), "chest": (4, 0, 0), "head": (-10, 0, 30)},
    foot_R=foot(0.14, -0.1, -0.74, space="hips", heel=60), foot_L=foot(-0.16, 0.0, -0.66, space="hips", heel=60, knee=20),
    hand_R=hand(0.4, 0.16, 0.5, aim=(-20, 70, 0), space="chest", elbow=(1, 0, 0)),
    hand_L=hand(-0.36, 0.14, 0.42, space="chest", elbow=(-1, 0, 0)),
)
anim("death", [
    (0, HIT),
    (24, pose(KNEEL, torso(drop=0.5, yaw=10, bend=14, nod=24), foot_R=foot(0.14, 0.1, 0.06, heel=75),
              hand_R=hand(0.32, 0.2, 0.42, aim=(-30, -10, 0)), hand_L=hand(-0.32, 0.06, 0.4)), "in"),
    (36, pose(torso(drop=0.52, yaw=10, bend=24, nod=34))),
    (56, PRONE, "in"),
    (62, pose(PRONE, hips=(0, 0.38, -0.78), chest=(-3, 0, 0))),
    ("T", PRONE),
])
# Heal: the left hand brings the flask to the lips, the head tips back to drink.
DRINK = pose(
    torso(drop=0.06, yaw=22, twist=4, bend=-4, nod=-6, look=0.6),
    hand_R=hand(0.3, 0.3, 0.92, aim=(-8, -14, 0)),
    hand_L=hand(-0.05, 0.3, 1.52, elbow=(-0.9, 0.1, -0.5)),
)
anim("heal", [
    (0, STANCE_R), (11, DRINK, "out"),
    (22, pose(torso(drop=0.05, yaw=22, twist=4, bend=-12, nod=-20, look=0.6),
              hand_L=hand(-0.05, 0.28, 1.62, elbow=(-0.9, 0.1, -0.2)))),
    (30, pose(torso(drop=0.07, yaw=24, twist=6, bend=2, look=0.6), hand_L=hand(-0.12, 0.3, 1.4, elbow=(-0.9, 0.1, -0.5)))),
    ("T", STANCE_R),
])
# Summoning (online co-op search, at the brazier): both arms raised to the sky, swaying slowly.
SUMMON = pose(
    torso(drop=0.02, bend=-10, nod=-24),
    foot_R=foot(0.12, 0.02, yaw=-8), foot_L=foot(-0.12, -0.02, yaw=8),
    hand_R=hand(0.42, 0.14, 1.94, aim=(-20, 80, 0), elbow=(1, 0.2, 0.2)),
    hand_L=hand(-0.42, 0.14, 1.9, elbow=(-1, 0.2, 0.2)),
)
anim("summon", [
    (0, SUMMON),
    (60, pose(SUMMON, torso(drop=0.0, bend=-15, nod=-30, side=0.02),
              hand_R=hand(0.5, 0.08, 1.98, aim=(-28, 76, 0), elbow=(1, 0.2, 0.2)),
              hand_L=hand(-0.5, 0.08, 1.95, elbow=(-1, 0.2, 0.2)))),
    (120, SUMMON),
], loop=True)

# ----------------------------------------------------------------------------- rapier
# A thrust is a lunge: the weight sinks back as the hand comes in, then the front foot kicks out
# and lands as the arm locks, rear leg straight and rear foot flat, off arm thrown back in line
# with the blade. The recovery brings the rear foot up under the body again.
R_FEET = plant(STANCE_R)


def coil(depth=1.0, height=1.16):
    """Weight on the rear leg, hand drawn back to the chest, shoulders wound up."""
    return pose(
        torso(drop=0.08 + 0.07 * depth, fwd=-0.05 * depth, yaw=32 - 6 * depth, twist=14 - 22 * depth, bend=3),
        hand_R=hand(0.24, 0.5 - 0.24 * depth, height + 0.03 * depth, aim=(6, 8, 0), elbow=(0.6, -0.8, -0.4)),
        hand_L=hand(-0.2, -0.06 + 0.12 * depth, 1.08),
        foot_R=tweak(R_FEET["foot_R"], heel=14 * depth),
    )


def lunge(step, travel, height=1.25, depth=1.0, reach=0.86, lift=0.07):
    """Fully extended. `step`: how far the front foot kicks out, `travel`: root motion of the move
    (the front foot lands that much further in the world, the rear one hasn't moved)."""
    return pose(
        torso(drop=0.13 + 0.07 * depth, fwd=0.03 + 0.05 * depth, yaw=44 + 6 * depth, twist=16, bend=8 + 5 * depth),
        foot_R=foot(0.10, 0.27 + travel + step, space="world", lift=lift),
        # Past a certain depth the rear foot can't stay behind: it is dragged along on its edge.
        foot_L=tweak(R_FEET["foot_L"], yaw=78) if travel < 0.3 else foot(-0.14, -0.54, yaw=78, heel=12),
        hand_R=hand(0.09, reach, height, aim=(0, (height - 1.22) * 30, 0)),
        hand_L=hand(-0.22, -0.4 - 0.05 * depth, 1.0 - 0.06 * depth, elbow=(-0.3, -0.3, -1)),
    )


def recover(p, travel, sink=0.02):
    """Settling into the lunge after the hit, then the rear foot coming up: what the body does
    during the frames an attack can already be chained from."""
    return [
        ("h0e", pose(p, hips=(p["hips"][0], p["hips"][1] + 0.02, p["hips"][2] - sink))),
        ("h0e+5", pose(torso(drop=0.14, fwd=0.03, yaw=38, twist=10, bend=10),
                       foot_L=foot(-0.13, -0.3, yaw=70, heel=30), hand_R=hand(0.14, 0.62, 1.16, aim=(2, 8, 0)),
                       hand_L=hand(-0.27, -0.26, 1.0))),
        ("T", pose(STANCE_R, foot_L=tweak(STANCE_R["foot_L"], lift=0.03))),
    ]


def thrust_anim(name, step, travel, height, depth, wind="h0-4"):
    hit = lunge(step, travel, height, depth)
    anim(name, [(0, R_FEET), (wind, coil(depth, height - 0.06), "out"), ("h0", hit, "snap")] + recover(hit, travel))


thrust_anim("rapier_light1", 0.22, 0.22, 1.25, 0.8, "h0-5")
thrust_anim("rapier_light2", 0.2, 0.22, 1.13, 1.0)
# Third: a flat cut from right to left, the hips turning through it.
CUT = (0.0, 0.12)
anim("rapier_light3", [
    (0, R_FEET),
    ("h0-4", pose(torso(drop=0.13, fwd=-0.03, yaw=18, twist=-22, bend=4),
                  hand_R=hand(0.5, 0.2, 1.3, aim=(-70, 14, 0), elbow=(0.9, -0.4, -0.3), arc=CUT),
                  hand_L=hand(-0.16, 0.1, 1.1), foot_R=tweak(R_FEET["foot_R"], heel=12)), "out"),
    ("h0", pose(torso(drop=0.16, fwd=0.06, yaw=34, twist=2, bend=8),
                foot_R=foot(0.12, 0.27 + 0.3 + 0.12, space="world", lift=0.05), foot_L=tweak(R_FEET["foot_L"], yaw=75),
                hand_R=hand(0.42, 0.62, 1.24, aim=(-38, 0, 0), arc=CUT), hand_L=hand(-0.3, -0.2, 1.05)), "wind"),
    ("h0e", pose(torso(drop=0.17, fwd=0.08, yaw=52, twist=30, bend=10),
                 hand_R=hand(-0.2, 0.62, 1.2, aim=(42, -2, 0), arc=CUT),
                 hand_L=hand(-0.34, -0.36, 0.98, elbow=(-0.3, -0.3, -1))), "lin"),
    ("h0e+3", pose(torso(drop=0.16, fwd=0.07, yaw=56, twist=40, bend=10),
                   hand_R=hand(-0.36, 0.4, 1.14, aim=(80, -8, 0), arc=CUT)), "out"),
    ("h0e+9", pose(torso(drop=0.12, fwd=0.03, yaw=40, twist=20, bend=8), foot_L=foot(-0.13, -0.28, yaw=70, heel=25),
                   hand_R=hand(0.02, 0.5, 1.14, aim=(30, 8, 0), arc=CUT), hand_L=hand(-0.27, -0.22, 1.0))),
    ("T", pose(STANCE_R, foot_L=tweak(STANCE_R["foot_L"], lift=0.03))),
])
# Fourth: the finisher, wound up deep and thrown from the rear leg.
thrust_anim("rapier_light4", 0.2, 0.52, 1.22, 1.6, "h0-5")

# Charged thrust. Drawing it back: sunk onto the rear leg, hand at the ear, off hand pointing.
DEEP = pose(
    torso(drop=0.2, fwd=-0.1, yaw=22, twist=-18, bend=0),
    foot_R=foot(0.10, 0.3, heel=-10), foot_L=foot(-0.13, -0.25, yaw=70),
    hand_R=hand(0.3, 0.06, 1.3, aim=(8, 2, 0), elbow=(0.7, -0.8, 0.0)),
    hand_L=hand(-0.06, 0.42, 1.2, elbow=(-0.6, -0.2, -0.8)),
)
anim("rapier_charge", [(0, STANCE_R), (14, pose(coil(1.0), hand_L=hand(-0.14, 0.2, 1.12)), "out"), ("T", DEEP)])
# Heavy: the same, thrown with a passing step that carries the whole body behind the point.
HEAVY = lunge(0.16, 0.8, 1.22, 1.9, reach=0.88, lift=0.1)
anim("rapier_heavy", [
    (0, R_FEET), ("h0-7", plant(DEEP), "out"),
    ("h0-3", {"foot_L": tweak(R_FEET["foot_L"], yaw=75, heel=40)}),
    ("h0", pose(HEAVY, foot_L=foot(-0.14, -0.56, yaw=75, heel=35)), "snap"),
    ("h0e+2", pose(HEAVY, torso(drop=0.28, fwd=0.14, yaw=56, twist=16, bend=18), foot_L=foot(-0.14, -0.56, yaw=75, heel=10),
                   foot_R=foot(0.10, 0.53))),
    ("h0e+10", pose(torso(drop=0.16, fwd=0.04, yaw=40, twist=10, bend=12),
                    foot_R=foot(0.10, 0.4), foot_L=foot(-0.13, -0.34, yaw=70, heel=30, lift=0.04),
                    hand_R=hand(0.14, 0.6, 1.14, aim=(2, 6, 0)), hand_L=hand(-0.27, -0.26, 1.0))),
    ("T", STANCE_R),
])
# Released charge: a flèche. The body flies flat behind the blade, legs trailing, and lands in a
# deep lunge that skids to a stop.
FLECHE = pose(
    {"hips": (0, 0.2, -0.1), "hips_rot": (38, 0, 46), "chest": (16, 0, 16), "head": (-40, 0, -52)},
    foot_R=foot(0.08, 0.3, -0.6, space="hips", heel=20), foot_L=foot(-0.1, -0.5, -0.56, space="hips", heel=60),
    hand_R=hand(0.08, 1.02, 1.2, aim=(0, 0, 0)),
    hand_L=hand(-0.3, -0.36, 0.2, space="chest", elbow=(-0.3, -0.3, -1)),
)
SKID = pose(lunge(0.3, 0.0, 1.2, 2.2, reach=0.9), foot_R=foot(0.10, 0.6), foot_L=foot(-0.14, -0.56, yaw=78, heel=15))
anim("rapier_heavy_charged", [
    (0, DEEP),
    (2, pose(DEEP, torso(drop=0.24, fwd=-0.06, yaw=24, twist=-20, bend=6), foot_L=foot(-0.13, -0.25, yaw=70, heel=30))),
    ("h0-1", FLECHE, "snap"), ("h0e-2", pose(FLECHE, hips=(0, 0.22, -0.16))),
    ("h0e+2", SKID, "out"),
    ("h0e+9", pose(SKID, torso(drop=0.3, fwd=0.14, yaw=54, twist=14, bend=20))),
    ("h0e+19", pose(torso(drop=0.16, fwd=0.04, yaw=40, twist=10, bend=12),
                    foot_R=foot(0.10, 0.42), foot_L=foot(-0.13, -0.34, yaw=70, heel=30, lift=0.04),
                    hand_R=hand(0.14, 0.6, 1.14, aim=(2, 6, 0)), hand_L=hand(-0.27, -0.26, 1.0))),
    ("T", STANCE_R),
])
# Special: a dash that lands in a low lunge, three stabs pumped from the shoulder at different
# heights, then the hand drawn right back for the last one.
LOW = pose(torso(drop=0.24, fwd=0.08, yaw=46, twist=8, bend=12), foot_R=foot(0.10, 0.56), foot_L=foot(-0.14, -0.5, yaw=78))
special = [
    (0, STANCE_R),
    (4, pose(torso(drop=0.26, fwd=0.0, yaw=24, twist=-14, bend=24),
             foot_R=foot(0.10, 0.2, heel=10), foot_L=foot(-0.13, -0.3, yaw=50, heel=40),
             hand_R=hand(0.3, 0.1, 0.96, aim=(4, 4, 0), elbow=(0.7, -0.8, 0.0)), hand_L=hand(-0.1, 0.34, 0.96)), "out"),
    (8, pose({"hips": (0, 0.14, -0.2), "hips_rot": (26, 0, 30), "chest": (14, 0, -6), "head": (-30, 0, -24)},
             foot_R=foot(0.09, 0.5, -0.5, space="hips"), foot_L=foot(-0.1, -0.52, -0.5, space="hips", heel=60),
             hand_R=hand(0.26, 0.2, 1.0, aim=(4, 6, 0), elbow=(0.7, -0.8, 0.0)), hand_L=hand(-0.2, 0.3, 0.9))),
    (12, pose(LOW, hand_L=hand(-0.24, -0.4, 0.94, elbow=(-0.3, -0.3, -1))), "out"),
]
for i, h in enumerate((1.3, 1.0, 1.4)):
    stab = {"hand_R": hand(0.08, 0.9, h, aim=(0, (h - 1.2) * 30, 0)), "chest": (14, 0, 22), "hips": (0, 0.11, -0.25)}
    special += [(f"h{i}", stab, "snap"), (f"h{i}e", stab)]
    nxt = (1.0, 1.4, 1.2)[i]
    special.append((f"h{i}e+3", {"hand_R": hand(0.22, 0.34, nxt + 0.02, aim=(4, (nxt - 1.2) * 20, 0), elbow=(0.6, -0.8, -0.3)),
                                 "chest": (9, 0, -6), "hips": (0, 0.05, -0.23)}, "out"))
FINAL = pose(lunge(0.0, 0.0, 1.2, 2.4, reach=0.94), foot_R=foot(0.10, 0.74, lift=0.08), foot_L=foot(-0.14, -0.5, yaw=80))
special += [
    ("h3-5", pose(torso(drop=0.26, fwd=-0.04, yaw=26, twist=-20, bend=4),
                  hand_R=hand(0.3, 0.04, 1.26, aim=(6, 0, 0), elbow=(0.7, -0.8, 0.0)),
                  hand_L=hand(-0.06, 0.42, 1.16, elbow=(-0.6, -0.2, -0.8)), foot_R=foot(0.10, 0.5, heel=-12))),
    ("h3", FINAL, "snap"), ("h3e+4", pose(FINAL, torso(drop=0.32, fwd=0.16, yaw=56, twist=16, bend=22))),
    ("h3e+14", pose(torso(drop=0.16, fwd=0.04, yaw=40, twist=10, bend=12),
                    foot_R=foot(0.10, 0.44), foot_L=foot(-0.13, -0.3, yaw=70, heel=30, lift=0.04),
                    hand_R=hand(0.14, 0.6, 1.14, aim=(2, 6, 0)), hand_L=hand(-0.27, -0.26, 1.0))),
    ("T", STANCE_R),
]
anim("rapier_special", special)
# Fatal blow: the blade is flourished up overhead, point down, and driven in with the whole body.
HIGH_R = pose(
    torso(drop=0.04, fwd=-0.04, yaw=40, twist=10, bend=-10, nod=10),
    foot_R=foot(0.10, 0.24), foot_L=foot(-0.13, -0.26, yaw=65),
    hand_R=hand(0.26, 0.2, 1.86, aim=(0, -50, 0), elbow=(0.9, 0.3, 0.2)),
    hand_L=hand(-0.12, 0.28, 1.28, elbow=(-0.6, -0.2, -0.8)),
)
STAB_DOWN = pose(
    torso(drop=0.3, fwd=0.14, yaw=40, twist=14, bend=34, nod=10),
    foot_R=foot(0.10, 0.62, lift=0.08), foot_L=foot(-0.14, -0.4, yaw=75, heel=20),
    hand_R=hand(0.1, 0.78, 0.92, aim=(0, -34, 0), elbow=(0.9, 0.0, 0.3)),
    hand_L=hand(-0.3, -0.36, 1.0, elbow=(-0.3, -0.3, -1)),
)
anim("rapier_fatal", [
    (0, STANCE_R),
    (12, pose(torso(drop=0.12, yaw=30, twist=24, bend=6), hand_R=hand(0.0, 0.4, 1.2, aim=(60, 30, 0)),
              hand_L=hand(-0.24, -0.1, 1.06))),
    (24, HIGH_R, "out"), ("h0-5", pose(HIGH_R, torso(drop=0.02, fwd=-0.07, yaw=40, twist=6, bend=-14, nod=12),
                                       hand_R=hand(0.27, 0.16, 1.9, aim=(0, -46, 0), elbow=(0.9, 0.3, 0.2)))),
    ("h0", STAB_DOWN, "wind"),
    ("h0e+18", pose(STAB_DOWN, torso(drop=0.33, fwd=0.17, yaw=42, twist=14, bend=38, nod=10),
                    hand_R=hand(0.1, 0.8, 0.86, aim=(0, -36, 0), elbow=(0.9, 0.0, 0.3)))),
    ("h0e+30", pose(torso(drop=0.14, fwd=0.02, yaw=36, twist=4, bend=6),
                    foot_R=foot(0.10, 0.4), foot_L=foot(-0.13, -0.3, yaw=70, heel=20),
                    hand_R=hand(0.3, 0.3, 1.2, aim=(-20, 30, 0)), hand_L=hand(-0.27, -0.2, 1.02))),
    ("T", STANCE_R),
])

# ----------------------------------------------------------------------------- greatsword
# The sword is heavy: every blow starts from the feet. The hips wind up and unwind first, the
# shoulders follow, the blade comes last and drags the body on after the hit.
G_FEET = plant(STANCE_G)
SWING = (0.0, 0.0)


def sweep(name, d, travel):
    """Flat sweep, `d` = +1 from right to left (a passing step with the right foot), -1 the other
    way (the left foot is already leading: it just reaches further)."""
    def blade(angle, r, u, yaw, pitch, **kw):
        a = math.radians(angle * d)
        return hand(-math.sin(a) * r, math.cos(a) * r, u, aim=(yaw * d, pitch, 0), arc=SWING, **kw)

    if d > 0:
        step = {"foot_R": foot(0.2, travel + 0.3, yaw=-10, space="world", lift=0.09),
                "foot_L": tweak(G_FEET["foot_L"], yaw=40, heel=20)}
        load = {"foot_L": tweak(G_FEET["foot_L"], yaw=10)}
    else:
        step = {"foot_L": foot(-0.16, 0.26 + travel + 0.1, yaw=20, space="world", lift=0.08),
                "foot_R": foot(0.16, -0.5, yaw=-55, heel=30)}
        load = {"foot_L": tweak(G_FEET["foot_L"], heel=16)}
    back = {"foot_R": tweak(STANCE_G["foot_R"], lift=0.06), "foot_L": STANCE_G["foot_L"]}
    anim(name, [
        (0, G_FEET),
        ("h0-9", pose(torso(drop=0.17, fwd=-0.07, side=0.05 * d, yaw=-52 * d, twist=-34 * d, bend=6), load,
                      hand_R=blade(-128, 0.42, 1.2, -196, 24, elbow=(0.9 * d, -0.5, -0.1))), "out"),
        ("h0-3", pose(torso(drop=0.2, fwd=-0.02, yaw=-34 * d, twist=-40 * d, bend=8),
                      hand_R=blade(-112, 0.46, 1.18, -176, 16))),
        ("h0", pose(torso(drop=0.22, fwd=0.06, yaw=-4 * d, twist=-22 * d, bend=10), step,
                    hand_R=blade(-52, 0.5, 1.12, -84, 4)), "wind"),
        ("h0e", pose(torso(drop=0.22, fwd=0.09, yaw=30 * d, twist=24 * d, bend=12),
                     hand_R=blade(46, 0.5, 1.06, 74, -4)), "lin"),
        ("h0e+5", pose(torso(drop=0.2, fwd=0.09, yaw=44 * d, twist=40 * d, bend=14),
                       hand_R=blade(104, 0.44, 1.0, 150, -12)), "out"),
        ("h0e+12", pose(torso(drop=0.18, fwd=0.07, yaw=44 * d, twist=38 * d, bend=14),
                        hand_R=blade(110, 0.42, 0.98, 158, -16))),
        ("T", pose(STANCE_G, back, hand_R=tweak(STANCE_G["hand_R"], arc=SWING))),
    ])


sweep("greatsword_light1", 1, 0.44)
sweep("greatsword_light2", -1, 0.44)

# Overhead blows: the edge is turned down (roll), the sword goes up over the right shoulder and
# comes down through the middle, the knees giving as it lands.
OVERHEAD = pose(
    torso(drop=0.05, fwd=-0.06, yaw=-12, twist=-6, bend=-12),
    foot_L=tweak(G_FEET["foot_L"], heel=-8), foot_R=foot(0.16, -0.32, yaw=-55, heel=30, lift=0.05),
    hand_R=hand(0.12, 0.02, 1.9, aim=(0, 128, 90), elbow=(0.9, 0.5, 0.1)),
    hand_L=hilt(elbow=(-0.9, 0.5, 0.1)),
)


def chop(name, travel, pre, raised=OVERHEAD, start=G_FEET, power=1.0, hold=10):
    """`pre`: the keys that bring the sword from `start` up to `raised`."""
    low = 0.24 + 0.06 * power
    land = pose(
        torso(drop=low, fwd=0.12, yaw=-10, twist=4, bend=24 + 8 * power),
        foot_L=foot(-0.13, 0.26 + travel + 0.16, space="world", lift=0.08), foot_R=foot(0.16, -0.4, yaw=-55, heel=25),
        hand_R=hand(0.05, 0.56, 0.78 - 0.06 * power, aim=(0, -12, 90), elbow=(0.6, -0.4, -0.7)),
        hand_L=hilt(),
    )
    anim(name, [(0, start)] + pre + [
        ("h0-2", pose(torso(drop=0.07, fwd=-0.02, yaw=-12, twist=-6, bend=-16),
                      hand_R=tweak(raised["hand_R"], d=(0, 0.04, 0.02)))),
        ("h0", pose(land, hips=(0, 0.07, -0.14), chest=(10, 0, 0),
                    hand_R=hand(0.06, 0.52, 1.34, aim=(0, 44, 90), elbow=(0.7, -0.2, -0.6))), "wind"),
        ("h0e", land, "lin"),
        ("h0e+3", pose(land, torso(drop=low + 0.03, fwd=0.13, yaw=-10, twist=4, bend=30 + 8 * power),
                       hand_R=hand(0.05, 0.56, 0.72 - 0.06 * power, aim=(0, -16, 90), elbow=(0.6, -0.4, -0.7))), "out"),
        (f"h0e+{hold}", land),
        (f"h0e+{hold + 12}", pose(torso(drop=0.16, fwd=0.04, yaw=-20, twist=-6, bend=14),
                                  foot_L=foot(-0.12, 0.3), foot_R=foot(0.16, -0.25, yaw=-55, heel=20, lift=0.05),
                                  hand_R=hand(0.18, 0.34, 0.9, aim=(8, 10, 40)))),
        ("T", STANCE_G),
    ])


chop("greatsword_light3", 0.31, [("h0-9", OVERHEAD, "out")])
chop("greatsword_heavy", 0.56, [("h0-20", pose(OVERHEAD, torso(drop=0.14, fwd=-0.04, yaw=-30, twist=-16, bend=8),
                                               hand_R=hand(0.3, 0.1, 1.2, aim=(-60, 60, 60))), "out"),
                                ("h0-9", OVERHEAD)], power=1.5, hold=12)
# Charging: the sword laid back on the right shoulder, the body sinking under it.
SHOULDER = pose(
    torso(drop=0.2, fwd=-0.08, yaw=-40, twist=-26, bend=4),
    foot_L=foot(-0.12, 0.28, heel=-8), foot_R=foot(0.16, -0.25, yaw=-60),
    hand_R=hand(0.34, -0.06, 1.44, aim=(-150, 28, 60), elbow=(0.9, -0.2, 0.3)),
    hand_L=hilt(elbow=(-0.5, 0.6, -0.4)),
)
anim("greatsword_charge", [(0, STANCE_G), (20, pose(SHOULDER, torso(drop=0.12, fwd=-0.03, yaw=-34, twist=-18, bend=6)), "out"),
                           ("T", SHOULDER)])
# Released: a bounding step, the sword torn off the shoulder, up and over.
RAISED = pose(OVERHEAD, torso(drop=0.02, fwd=0.02, yaw=-18, twist=-8, bend=-16),
              foot_R=foot(0.16, -0.36, yaw=-55, heel=40, lift=0.06),
              hand_R=hand(0.14, 0.02, 1.9, aim=(0, 134, 90), elbow=(0.9, 0.5, 0.1)))
chop("greatsword_heavy_charged", 0.89, [("h0-6", RAISED, "wind")], raised=RAISED, start=plant(SHOULDER), power=2.0, hold=16)
# Fatal blow: the sword reversed overhead, point down, and driven in with both arms.
PLUNGE_UP = pose(
    torso(drop=0.02, fwd=-0.05, yaw=-8, bend=-14, nod=14),
    foot_L=foot(-0.13, 0.22), foot_R=foot(0.16, -0.22, yaw=-45),
    hand_R=hand(0.06, 0.2, 1.9, aim=(0, -74, 0), elbow=(0.9, 0.4, 0.2)),
    hand_L=hilt(elbow=(-0.9, 0.4, 0.2)),
)
PLUNGE = pose(
    torso(drop=0.34, fwd=0.14, yaw=-6, bend=36, nod=12),
    foot_L=foot(-0.14, 0.52, lift=0.08), foot_R=foot(0.16, -0.4, yaw=-50, heel=30),
    hand_R=hand(0.04, 0.66, 1.06, aim=(0, -62, 0), elbow=(0.8, -0.2, -0.3)),
    hand_L=hilt(elbow=(-0.8, -0.2, -0.3)),
)
anim("greatsword_fatal", [
    (0, STANCE_G),
    (14, pose(torso(drop=0.14, yaw=-30, twist=-14, bend=6), hand_R=hand(0.3, 0.2, 1.2, aim=(-30, 70, 0)))),
    (28, PLUNGE_UP, "out"),
    ("h0-5", pose(PLUNGE_UP, torso(drop=0.0, fwd=-0.08, yaw=-8, bend=-18, nod=16),
                  hand_R=hand(0.06, 0.16, 1.92, aim=(0, -70, 0), elbow=(0.9, 0.4, 0.2)))),
    ("h0", PLUNGE, "wind"),
    ("h0e+20", pose(PLUNGE, torso(drop=0.37, fwd=0.16, yaw=-6, bend=40, nod=12),
                    hand_R=hand(0.04, 0.68, 1.0, aim=(0, -64, 0), elbow=(0.8, -0.2, -0.3)))),
    ("h0e+34", pose(torso(drop=0.16, fwd=0.04, yaw=-18, twist=-4, bend=16),
                    foot_L=foot(-0.12, 0.34), foot_R=foot(0.16, -0.28, yaw=-55, heel=20),
                    hand_R=hand(0.16, 0.36, 1.0, aim=(6, 4, 30)))),
    ("T", STANCE_G),
])

# ----------------------------------------------------------------------------- jump attacks
# The clip starts in the air (the jump already happened) and the hit lands with the feet.
AIR_FEET = {"foot_R": foot(0.11, 0.24, -0.5, space="hips", heel=10), "foot_L": foot(-0.11, -0.2, -0.52, space="hips", heel=50)}
anim("rapier_jump", [
    (0, pose(torso(bend=-12, yaw=30, twist=10, nod=14), AIR_FEET,
             hand_R=hand(0.3, 0.1, 1.84, aim=(0, -40, 0), elbow=(0.9, 0.3, 0.2)), hand_L=hand(-0.2, 0.36, 1.3))),
    ("h0-2", pose(torso(bend=-18, yaw=30, twist=4, nod=16),
                  hand_R=hand(0.32, 0.04, 1.9, aim=(0, -36, 0), elbow=(0.9, 0.3, 0.2)))),
    ("h0+2", pose(torso(drop=0.1, fwd=0.08, yaw=40, twist=14, bend=24, nod=8),
                  foot_R=foot(0.11, 0.4, -0.6, space="hips"), foot_L=foot(-0.12, -0.3, -0.6, space="hips", heel=40),
                  hand_R=hand(0.1, 0.72, 1.16, aim=(0, -34, 0), elbow=(0.9, 0.0, 0.3)),
                  hand_L=hand(-0.3, -0.36, 1.1, elbow=(-0.3, -0.3, -1))), "wind"),
    ("h0e", pose(STAB_DOWN, torso(drop=0.36, fwd=0.14, yaw=40, twist=14, bend=36, nod=10), foot_R=foot(0.10, 0.56),
                 foot_L=foot(-0.14, -0.36, yaw=75, heel=30)), "out"),
    ("h0e+8", STAB_DOWN),
    ("T", STANCE_R),
])
anim("greatsword_jump", [
    (0, pose(OVERHEAD, torso(bend=-14, yaw=-10, nod=6), AIR_FEET)),
    ("h0-3", pose(torso(bend=-22, yaw=-10, nod=10), hand_R=hand(0.12, -0.02, 1.94, aim=(0, 136, 90), elbow=(0.9, 0.5, 0.1)))),
    ("h0+1", pose(torso(drop=0.08, fwd=0.06, yaw=-10, bend=12),
                  foot_L=foot(-0.12, 0.34, -0.6, space="hips"), foot_R=foot(0.14, -0.26, -0.6, space="hips", heel=40),
                  hand_R=hand(0.06, 0.5, 1.4, aim=(0, 50, 90), elbow=(0.7, -0.2, -0.6))), "wind"),
    ("h0e", pose(torso(drop=0.4, fwd=0.14, yaw=-10, twist=4, bend=40),
                 foot_L=foot(-0.13, 0.5), foot_R=foot(0.16, -0.36, yaw=-55, heel=35),
                 hand_R=hand(0.05, 0.56, 0.62, aim=(0, -16, 90), elbow=(0.6, -0.4, -0.7))), "lin"),
    ("h0e+4", pose(torso(drop=0.44, fwd=0.15, yaw=-10, twist=4, bend=46),
                   hand_R=hand(0.05, 0.56, 0.56, aim=(0, -18, 90), elbow=(0.6, -0.4, -0.7))), "out"),
    ("h0e+16", pose(torso(drop=0.4, fwd=0.14, yaw=-10, twist=4, bend=40))),
    ("h0e+28", pose(torso(drop=0.16, fwd=0.04, yaw=-20, twist=-6, bend=14),
                    foot_L=foot(-0.12, 0.3), foot_R=foot(0.16, -0.25, yaw=-55, heel=20, lift=0.05),
                    hand_R=hand(0.18, 0.34, 0.9, aim=(8, 10, 40)))),
    ("T", STANCE_G),
])

# Greatsword special: crouch, leap with the blade raised (the game lifts the body: `lift`), hang
# at the top arched back, vertical cleave on the way down, landing crouched, blade in the ground.
APEX = pose(
    torso(bend=-26, yaw=-8, nod=10),
    foot_L=foot(-0.11, 0.2, -0.5, space="hips", heel=20), foot_R=foot(0.12, -0.3, -0.42, space="hips", heel=60),
    hand_R=hand(0.1, -0.08, 1.96, aim=(0, 150, 90), elbow=(0.9, 0.5, 0.1)), hand_L=hilt(elbow=(-0.9, 0.5, 0.1)),
)
LANDED = pose(
    torso(drop=0.42, fwd=0.14, yaw=-10, twist=4, bend=44),
    foot_L=foot(-0.14, 0.5), foot_R=foot(0.16, -0.36, yaw=-55, heel=40),
    hand_R=hand(0.05, 0.56, 0.56, aim=(0, -18, 90), elbow=(0.6, -0.4, -0.7)), hand_L=hilt(),
)
anim("greatsword_special", [
    (0, STANCE_G),
    (6, pose(torso(drop=0.36, fwd=0.04, yaw=-16, bend=30, nod=-14),
             foot_L=foot(-0.13, 0.26), foot_R=foot(0.16, -0.22, yaw=-45, heel=30),
             hand_R=hand(0.24, 0.2, 0.62, aim=(10, -6, 60)), hand_L=hilt()), "out"),
    (12, pose(torso(drop=-0.02, bend=-6, yaw=-10),
              foot_L=foot(-0.11, 0.0, -0.8, space="hips", heel=50), foot_R=foot(0.12, -0.14, -0.76, space="hips", heel=55),
              hand_R=hand(0.16, 0.2, 1.6, aim=(0, 80, 90), elbow=(0.9, 0.3, -0.2))), "out"),
    (21, APEX),
    ("h0-1", pose(APEX, torso(bend=-32, yaw=-8, nod=12))),
    ("h0+4", pose(torso(drop=0.04, bend=18, yaw=-10),
                  foot_L=foot(-0.12, 0.36, -0.62, space="hips"), foot_R=foot(0.14, -0.2, -0.62, space="hips", heel=40),
                  hand_R=hand(0.06, 0.52, 1.36, aim=(0, 30, 90), elbow=(0.7, -0.2, -0.6))), "wind"),
    ("h0e+3", LANDED, "lin"),
    ("h0e+7", pose(LANDED, torso(drop=0.46, fwd=0.15, yaw=-10, twist=4, bend=50)), "out"),
    ("h0e+16", LANDED),
    ("h0e+30", pose(torso(drop=0.16, fwd=0.04, yaw=-20, twist=-6, bend=14),
                    foot_L=foot(-0.12, 0.3), foot_R=foot(0.16, -0.25, yaw=-55, heel=20, lift=0.05),
                    hand_R=hand(0.18, 0.34, 0.9, aim=(8, 10, 40)))),
    ("T", STANCE_G),
])

missing = sorted(set(T) - set(markers))
if missing:
    raise SystemExit(f"missing animations: {missing}")

rest_pose(rig)
export("player", markers)
