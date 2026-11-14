"""Generates the weapons: assets/models/rapier.glb, greatsword.glb and staff.glb (the mage's).

Origin = centre of the grip (where the hand holds the weapon), blade pointing forward (-Y Blender,
+Z in Bevy).
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *  # noqa: E402,F403

reset_scene()
STEEL = material("steel", (0.78, 0.8, 0.86))
STEEL_DARK = material("steel_dark", (0.45, 0.47, 0.52))
BRASS = material("brass", (0.8, 0.62, 0.28))
GRIP = material("grip", tex=tex_noise((0.22, 0.13, 0.08), 0.5, seed=21))
WOOD = material("staff_wood", tex=tex_noise((0.34, 0.24, 0.15), 0.45, seed=31))
CRYSTAL = material("staff_crystal", (0.72, 0.85, 1.0), emissive=(0.35, 0.55, 1.0))


def weapon(name, build):
    mb = MeshBuilder()
    build(mb)
    obj = bpy.data.objects.new(name, mb.finish(name + "_mesh"))
    bpy.context.scene.collection.objects.link(obj)
    return obj


def rapier(mb):
    mb.cylinder((0, 0.02, 0), 0.016, 0.15, GRIP, sides=6, axis="Y")
    mb.box((0, 0.11, 0), (0.035, 0.035, 0.035), BRASS)
    # Shell guard + quillons.
    mb.cylinder((0, -0.07, 0), 0.055, 0.02, BRASS, sides=8, axis="Y", radius_top=0.035)
    mb.box((0, -0.08, 0), (0.2, 0.012, 0.012), BRASS)
    mb.seg((0.0, -0.05, 0.0), (0.0, -0.02, -0.07), 0.01, 0.01, BRASS)
    # Thin blade.
    mb.seg((0, -0.08, 0), (0, -1.08, 0), 0.035, 0.02, STEEL, taper=0.3)


def greatsword(mb):
    # Greatsword: ~1.75 m blade, wide and thick, long two-handed grip.
    mb.cylinder((0, 0.14, 0), 0.024, 0.38, GRIP, sides=6, axis="Y")
    mb.box((0, 0.35, 0), (0.07, 0.07, 0.07), STEEL_DARK, taper=(0.7, 0.7))
    mb.box((0, -0.08, 0), (0.46, 0.06, 0.06), STEEL_DARK)
    mb.box((0, -0.08, 0), (0.1, 0.08, 0.08), BRASS)
    mb.box((-0.23, -0.08, 0), (0.05, 0.08, 0.08), STEEL_DARK)
    mb.box((0.23, -0.08, 0), (0.05, 0.08, 0.08), STEEL_DARK)
    # Ricasso then wide blade.
    mb.seg((0, -0.11, 0), (0, -0.3, 0), 0.085, 0.03, STEEL_DARK)
    mb.seg((0, -0.3, 0), (0, -1.62, 0), 0.13, 0.028, STEEL, taper=0.82)
    mb.seg((0, -1.62, 0), (0, -1.86, 0), 0.107, 0.024, STEEL, taper=0.08)
    mb.seg((0, -0.3, 0.0), (0, -1.45, 0.0), 0.03, 0.034, STEEL_DARK)


def staff(mb):
    # Ash staff: ~1.65 m, held a third of the way up (the grip); the head points forward (-Y),
    # the iron-shod foot backwards (+Y).
    mb.seg((0, 0.52, 0), (0, -1.0, 0), 0.042, 0.042, WOOD, taper=0.8)
    mb.box((0, 0.54, 0), (0.05, 0.06, 0.05), STEEL_DARK, taper=(0.7, 0.7))
    mb.cylinder((0, 0.0, 0), 0.028, 0.18, GRIP, sides=6, axis="Y")
    for y in (-0.11, 0.11, -0.9):
        mb.cylinder((0, y, 0), 0.031, 0.025, BRASS, sides=6, axis="Y")
    # Head: three prongs curling up around a pale crystal.
    for k in range(3):
        a = 2 * math.pi * k / 3
        x, z = math.cos(a) * 0.075, math.sin(a) * 0.075
        mb.seg((x * 0.3, -0.98, z * 0.3), (x, -1.1, z), 0.02, 0.02, WOOD)
        mb.seg((x, -1.1, z), (x * 0.45, -1.27, z * 0.45), 0.018, 0.018, WOOD, taper=0.6)
    # Bipyramid crystal (point towards the grip, then point ahead).
    mb.cylinder((0, -1.11, 0), 0.065, 0.07, CRYSTAL, sides=6, axis="Y", radius_top=0.006)
    mb.cylinder((0, -1.2, 0), 0.006, 0.11, CRYSTAL, sides=6, axis="Y", radius_top=0.065)


for name, build in (("rapier", rapier), ("greatsword", greatsword), ("staff", staff)):
    for o in list(bpy.context.scene.objects):
        o.select_set(False)
    obj = weapon(name, build)
    obj.select_set(True)
    export(name, selection=True, save_blend=False)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(BLEND, "weapons.blend"))
