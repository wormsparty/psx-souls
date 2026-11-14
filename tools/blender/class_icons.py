"""Portraits of the classes for the class choice menu (assets/ui/class_warrior.png, class_mage.png).

blender -b --factory-startup -P tools/blender/class_icons.py

The player (blend/player.blend, made by player.py) in a pose of each class, weapon in hand
(blend/weapons.blend): the warrior on guard with the greatsword, the mage gathering the arcane
lance, in the mage's blue coat (the game dyes it the same way: render/models.rs, `Dye`).
Seen three-quarter front, rendered at 128×160 on a transparent background.
"""

import math
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 128, 160
OUT = os.path.join(ROOT, "assets", "ui")

# Class: (weapon, animation, frame, coat dye: texel channel swap and gains, as in the game).
CLASSES = {
    "warrior": ("greatsword", "idle_greatsword", 0, None),
    "mage": ("staff", "cast_lance", 72, ((2, 1.1), (1, 1.5), (0, 1.35))),
}


def dye_coat(dye):
    for m in bpy.data.materials:
        if not m.name.startswith("coat") or not m.use_nodes:
            continue
        for n in m.node_tree.nodes:
            if n.type == "TEX_IMAGE" and n.image:
                px = list(n.image.pixels)
                for i in range(0, len(px), 4):
                    src = px[i:i + 3]
                    for k, (c, gain) in enumerate(dye):
                        px[i + k] = min(1.0, src[c] * gain)
                n.image.pixels.foreach_set(px)


def portrait(key, weapon, action, frame, dye):
    bpy.ops.wm.open_mainfile(filepath=os.path.join(HERE, "blend", "player.blend"))
    sc = bpy.context.scene
    objs = [o for o in sc.objects if o.animation_data]
    with bpy.data.libraries.load(os.path.join(HERE, "blend", "weapons.blend")) as (src, dst):
        dst.objects = [weapon]
    w = dst.objects[0]
    sc.collection.objects.link(w)
    w.parent = sc.objects["grip_R"]
    w.location = (0, 0, 0)
    if dye:
        dye_coat(dye)
    act = bpy.data.actions[action]
    for o in objs:
        for tr in o.animation_data.nla_tracks:
            tr.mute = True
        o.animation_data.action = act
        o.animation_data.action_slot = next((s for s in act.slots if s.identifier == "OB" + o.name), None)
    sc.frame_set(frame)
    bpy.context.view_layer.update()

    meshes = [o for o in sc.objects if o.type == "MESH"]
    pts = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
    center = sum(pts, Vector()) / len(pts)
    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam = bpy.data.objects.new("cam", cam_data)
    sc.collection.objects.link(cam)
    sc.camera = cam
    # Three-quarter front (the models face -Y), slightly from above.
    yaw, pitch = math.radians(-30), math.radians(8)
    direction = Vector((math.sin(yaw) * math.cos(pitch), -math.cos(yaw) * math.cos(pitch), math.sin(pitch)))
    right = Vector((math.cos(yaw), math.sin(yaw), 0))
    up = direction.cross(right).normalized() * -1
    xs = [(p - center).dot(right) for p in pts]
    ys = [(p - center).dot(up) for p in pts]
    span = max((max(xs) - min(xs)) * H / W, max(ys) - min(ys)) * 1.06
    cam_data.ortho_scale = span
    cam.location = center + direction * 30 + right * (max(xs) + min(xs)) / 2 + up * (max(ys) + min(ys)) / 2
    cam.rotation_euler = (-direction).to_track_quat("-Z", "Y").to_euler()

    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.light = "STUDIO"
    sc.display.shading.color_type = "TEXTURE"
    sc.display.shading.show_cavity = False
    sc.view_settings.view_transform = "Standard"
    sc.view_settings.exposure = 1.3
    sc.display.shading.show_object_outline = True
    sc.display.shading.object_outline_color = (0.02, 0.02, 0.02)
    sc.render.resolution_x = W
    sc.render.resolution_y = H
    sc.render.film_transparent = True
    sc.render.filter_size = 0.0
    sc.display.render_aa = "OFF"
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGBA"
    sc.render.filepath = os.path.join(OUT, f"class_{key}.png")
    bpy.ops.render.render(write_still=True)
    print(f"[assets] {sc.render.filepath}")


for key, (weapon, action, frame, dye) in CLASSES.items():
    portrait(key, weapon, action, frame, dye)
