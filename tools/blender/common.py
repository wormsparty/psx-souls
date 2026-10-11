"""Shared tools for the asset generation scripts (Blender 5.x, batch mode).

Conventions (Blender frame): Z up, characters face -Y, so the character's right
is -X. "Rigs" are hierarchies of objects (rigid pieces, as on the PS1) whose rest
rotation is the identity: animation rotations are therefore expressed in world axes:

- positive X: a hanging limb swings backwards; the torso leans forward.
- positive Z: rotation to the left (seen from above).
- positive Y: the top of a piece tilts towards the character's left
  (hanging right arm: abduction; left arm: adduction).

Animations are sampled at 60 fps: 1 frame = 1 simulation tick.
"""

import json
import math
import os
import random
import re
import sys

import bmesh
import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
MODELS = os.path.join(ROOT, "assets", "models")
BLEND = os.path.join(HERE, "blend")


def reset_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.fps = 60
    sc.frame_start = 0
    return sc


def load_timings():
    with open(os.path.join(HERE, "timings.json")) as f:
        return json.load(f)


# ----------------------------------------------------------------------------- textures

def _hash(x, y, seed):
    n = (x * 374761393 + y * 668265263 + seed * 2147483647) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0


def _value_noise(x, y, cell, seed, size):
    gx, gy = x / cell, y / cell
    x0, y0 = int(math.floor(gx)), int(math.floor(gy))
    fx, fy = gx - x0, gy - y0
    period = max(1, size // cell)

    def h(i, j):
        return _hash(i % period, j % period, seed)

    sx, sy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = h(x0, y0) + (h(x0 + 1, y0) - h(x0, y0)) * sx
    b = h(x0, y0 + 1) + (h(x0 + 1, y0 + 1) - h(x0, y0 + 1)) * sx
    return a + (b - a) * sy


def fbm(x, y, size, seed, cells=(16, 8, 4)):
    v, amp, tot = 0.0, 1.0, 0.0
    for c in cells:
        v += _value_noise(x, y, c, seed, size) * amp
        tot += amp
        amp *= 0.5
    return v / tot


def make_image(name, size, fn):
    """Creates a 'size'x'size' image; fn(x, y) -> (r, g, b) in 0..1 (sRGB)."""
    img = bpy.data.images.new(name, size, size, alpha=False)
    px = []
    for y in range(size):
        for x in range(size):
            r, g, b = fn(x, y)
            px.extend((min(max(r, 0), 1), min(max(g, 0), 1), min(max(b, 0), 1), 1.0))
    img.pixels.foreach_set(px)
    img.pack()
    return img


def tex_stone(size=64, seed=1, base=(0.42, 0.40, 0.37)):
    tile = size // 4

    def fn(x, y):
        row = y // tile
        ox = (tile // 2) * (row % 2)
        mortar = (y % tile) in (0,) or ((x + ox) % tile) in (0,)
        n = fbm(x, y, size, seed)
        k = _hash((x + ox) // tile, row, seed + 7) * 0.18
        v = 0.75 + n * 0.35 + k - (0.45 if mortar else 0)
        return tuple(c * v for c in base)

    return fn


def tex_brick(size=64, seed=2, base=(0.45, 0.32, 0.26)):
    bh, bw = size // 8, size // 4

    def fn(x, y):
        row = y // bh
        ox = (bw // 2) * (row % 2)
        mortar = (y % bh) == 0 or ((x + ox) % bw) == 0
        n = fbm(x, y, size, seed)
        k = _hash((x + ox) // bw, row, seed) * 0.25
        if mortar:
            return (0.22, 0.21, 0.2)
        v = 0.7 + n * 0.3 + k
        return tuple(c * v for c in base)

    return fn


def tex_noise(base, amount=0.35, seed=3, size=64, cells=(8, 4, 2)):
    def fn(x, y):
        v = 1 - amount / 2 + fbm(x, y, size, seed, cells) * amount
        return tuple(c * v for c in base)

    return fn


def tex_planks(base=(0.38, 0.25, 0.15), seed=4, size=64):
    pw = size // 4

    def fn(x, y):
        plank = x // pw
        n = fbm(x * 0.5, y * 3, size, seed + plank, (16, 8))
        edge = (x % pw) == 0
        v = 0.7 + n * 0.45 + _hash(plank, 0, seed) * 0.15 - (0.35 if edge else 0)
        return tuple(c * v for c in base)

    return fn


def tex_stripes(a, b, n=8, size=64, seed=5):
    def fn(x, y):
        c = a if (x * n // size) % 2 == 0 else b
        v = 0.85 + fbm(x, y, size, seed, (8, 4)) * 0.2
        return tuple(ch * v for ch in c)

    return fn


# ----------------------------------------------------------------------------- materials

_materials = {}


def material(name, color=(0.8, 0.8, 0.8), tex=None, emissive=None, size=64):
    """Simple material: flat colour or texture ("closest" filtering, PS1 style), `size` pixels
    square (the `tex` function's own size)."""
    if name in _materials:
        return _materials[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes.get("Principled BSDF")
    bsdf.inputs["Roughness"].default_value = 1.0
    if tex is not None:
        img = make_image(name + "_tex", size, tex)
        node = nt.nodes.new("ShaderNodeTexImage")
        node.image = img
        node.interpolation = "Closest"
        nt.links.new(node.outputs["Color"], bsdf.inputs["Base Color"])
    else:
        bsdf.inputs["Base Color"].default_value = (*srgb_to_linear(color), 1.0)
    if emissive is not None:
        bsdf.inputs["Emission Color"].default_value = (*srgb_to_linear(emissive), 1.0)
        bsdf.inputs["Emission Strength"].default_value = 1.0
    _materials[name] = m
    return m


def srgb_to_linear(c):
    return tuple(x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c)


# ----------------------------------------------------------------------------- geometry

class MeshBuilder:
    """Accumulates low-poly primitives into a single mesh (flat faces, per-face UVs).

    `MeshBuilder.cell` (metres): if set, the faces of boxes and segments are split into
    cells of at most that size, with the texture repeated on each: on a large model, a single
    texture stretched over a face several metres long gets distorted (PS1 affine mapping)."""

    cell = None

    def __init__(self):
        self.bm = bmesh.new()
        self.uv = self.bm.loops.layers.uv.new("UVMap")
        self.mats = []

    def _mat_index(self, mat):
        if mat not in self.mats:
            self.mats.append(mat)
        return self.mats.index(mat)

    def _face(self, verts, mat, uvs=None, uv_scale=(1, 1)):
        f = self.bm.faces.new(verts)
        f.material_index = self._mat_index(mat)
        f.smooth = False
        n = len(verts)
        default = [(0, 0), (1, 0), (1, 1), (0, 1)] if n == 4 else [(0, 0), (1, 0), (0.5, 1)]
        for i, loop in enumerate(f.loops):
            u, v = (uvs[i] if uvs else default[i % len(default)])
            loop[self.uv].uv = (u * uv_scale[0], v * uv_scale[1])
        return f

    def split_face(self, corners, mat, uvs, size=2.0):
        """Quad given by its 4 corners (coordinates) and their UVs, split into a grid of cells of
        at most `size` metres, positions and UVs interpolated: the texture looks the same, but
        the affine distortion (PS1) and the per-vertex lighting stay confined to each cell."""
        import mathutils
        p = [mathutils.Vector(c) for c in corners]
        q = [mathutils.Vector(t) for t in uvs]
        nu = max(1, math.ceil(max((p[1] - p[0]).length, (p[2] - p[3]).length) / size - 1e-6))
        nv = max(1, math.ceil(max((p[3] - p[0]).length, (p[2] - p[1]).length) / size - 1e-6))
        at = lambda c, u, v: c[0].lerp(c[1], u).lerp(c[3].lerp(c[2], u), v)
        grid = [[self.bm.verts.new(at(p, i / nu, j / nv)) for j in range(nv + 1)] for i in range(nu + 1)]
        for i in range(nu):
            for j in range(nv):
                cell = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
                self._face([grid[a][b] for a, b in cell], mat, uvs=[tuple(at(q, a / nu, b / nv)) for a, b in cell])

    def _quad(self, verts, mat, uv_scale=(1, 1)):
        """Four-vertex face, split according to `cell` (see the class)."""
        if not self.cell or len(verts) != 4:
            return self._face(verts, mat, uv_scale=uv_scale)
        p0, p1, p2, p3 = (v.co.copy() for v in verts)
        lu, lv = max((p1 - p0).length, (p2 - p3).length), max((p3 - p0).length, (p2 - p1).length)
        nu, nv = max(1, math.ceil(lu / self.cell - 1e-6)), max(1, math.ceil(lv / self.cell - 1e-6))
        if nu == 1 and nv == 1:
            return self._face(verts, mat, uv_scale=uv_scale)
        at = lambda u, v: (p0.lerp(p1, u)).lerp(p3.lerp(p2, u), v)
        grid = [[self.bm.verts.new(at(i / nu, j / nv)) for j in range(nv + 1)] for i in range(nu + 1)]
        # One texture repeat per cell (roughly square), joined from one cell to the next.
        ku, kv = lu / nu / self.cell, lv / nv / self.cell
        for i in range(nu):
            for j in range(nv):
                q = (grid[i][j], grid[i + 1][j], grid[i + 1][j + 1], grid[i][j + 1])
                uvs = [(i * ku, j * kv), ((i + 1) * ku, j * kv), ((i + 1) * ku, (j + 1) * kv), (i * ku, (j + 1) * kv)]
                self._face(q, mat, uvs=uvs, uv_scale=uv_scale)

    def box(self, center, size, mat, taper=(1.0, 1.0), shift_top=(0.0, 0.0), uv_scale=(1, 1)):
        """Centred box; `taper` scales the top face (x, y), `shift_top` offsets it."""
        cx, cy, cz = center
        sx, sy, sz = size[0] / 2, size[1] / 2, size[2] / 2
        tx, ty = taper
        bot = [(-sx, -sy), (sx, -sy), (sx, sy), (-sx, sy)]
        v = [self.bm.verts.new((cx + x, cy + y, cz - sz)) for x, y in bot]
        v += [
            self.bm.verts.new((cx + x * tx + shift_top[0], cy + y * ty + shift_top[1], cz + sz))
            for x, y in bot
        ]
        faces = [
            (v[0], v[3], v[2], v[1]),  # bottom
            (v[4], v[5], v[6], v[7]),  # top
            (v[0], v[1], v[5], v[4]),  # front (-Y)
            (v[1], v[2], v[6], v[5]),
            (v[2], v[3], v[7], v[6]),
            (v[3], v[0], v[4], v[7]),
        ]
        for f in faces:
            self._quad(f, mat, uv_scale=uv_scale)

    def panel(self, origin, du, dv, mat, cell=1.0, tile=2.0):
        """Quad (origin, origin+du, origin+du+dv, origin+dv) split into cells of at most
        `cell` metres, UVs in world coordinates (one texture repeat every `tile`
        metres): neighbouring panels line up, and the affine texture distortion
        (PS1) stays confined to each small cell."""
        import mathutils
        o, du, dv = mathutils.Vector(origin), mathutils.Vector(du), mathutils.Vector(dv)
        nu, nv = max(1, math.ceil(du.length / cell - 1e-6)), max(1, math.ceil(dv.length / cell - 1e-6))
        eu, ev = du.normalized(), dv.normalized()
        grid = [[self.bm.verts.new(o + du * (i / nu) + dv * (j / nv)) for j in range(nv + 1)] for i in range(nu + 1)]
        uv = lambda p: (p.dot(eu) / tile, p.dot(ev) / tile)
        for i in range(nu):
            for j in range(nv):
                q = (grid[i][j], grid[i + 1][j], grid[i + 1][j + 1], grid[i][j + 1])
                self._face(q, mat, uvs=[uv(v.co) for v in q])

    def slab(self, center, size, mat, cell=1.0, tile=2.0, skip=("bottom",)):
        """Box whose every face is a `panel` (subdivided, world UVs). `skip`: faces left out
        among bottom, top, -x, +x, -y, +y."""
        cx, cy, cz = center
        sx, sy, sz = size
        x0, y0, z0 = cx - sx / 2, cy - sy / 2, cz - sz / 2
        faces = {
            "bottom": ((x0, y0, z0), (0, sy, 0), (sx, 0, 0)),
            "top": ((x0, y0, z0 + sz), (sx, 0, 0), (0, sy, 0)),
            "-y": ((x0, y0, z0), (sx, 0, 0), (0, 0, sz)),
            "+x": ((x0 + sx, y0, z0), (0, sy, 0), (0, 0, sz)),
            "+y": ((x0 + sx, y0 + sy, z0), (-sx, 0, 0), (0, 0, sz)),
            "-x": ((x0, y0 + sy, z0), (0, -sy, 0), (0, 0, sz)),
        }
        for name, (o, du, dv) in faces.items():
            if name not in skip:
                self.panel(o, du, dv, mat, cell, tile)

    def seg(self, a, b, w, d, mat, taper=1.0):
        """Elongated box between two points (aligned with Z if a and b are vertically aligned)."""
        ax, ay, az = a
        bx, by, bz = b
        if abs(ax - bx) < 1e-6 and abs(ay - by) < 1e-6:
            lo, hi = min(az, bz), max(az, bz)
            t = (taper, taper) if az > bz else (1.0, 1.0)
            if az > bz:
                # Wider at the top: build it upside down.
                self.box(((ax), ay, (lo + hi) / 2), (w * taper, d * taper, hi - lo), mat,
                         taper=(1 / taper, 1 / taper))
            else:
                self.box((ax, ay, (lo + hi) / 2), (w, d, hi - lo), mat, taper=(taper, taper))
            return
        # Arbitrary segment: oriented prism.
        import mathutils
        va, vb = mathutils.Vector(a), mathutils.Vector(b)
        axis = (vb - va)
        length = axis.length
        axis.normalize()
        up = mathutils.Vector((0, 0, 1)) if abs(axis.z) < 0.9 else mathutils.Vector((1, 0, 0))
        side = axis.cross(up).normalized()
        up2 = side.cross(axis).normalized()
        hw, hd = w / 2, d / 2
        ring = lambda base, s: [
            self.bm.verts.new(base + side * (x * s) + up2 * (y * s))
            for x, y in ((-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd))
        ]
        r0 = ring(va, 1.0)
        r1 = ring(va + axis * length, taper)
        # (side, up2, axis) is a left-handed frame: rings run clockwise seen
        # from +axis, hence this order so that normals point outwards.
        self._quad((r0[0], r0[1], r0[2], r0[3]), mat)
        self._quad((r1[3], r1[2], r1[1], r1[0]), mat)
        for i in range(4):
            j = (i + 1) % 4
            self._quad((r0[j], r0[i], r1[i], r1[j]), mat)

    def cylinder(self, center, radius, height, mat, sides=8, radius_top=None, uv_scale=(1, 1),
                 caps=True, axis="Z", split=None):
        """`split` (metres, Z axis only): its sides are split into cells of at most that size
        (see `split_face`), for a tall cylinder seen up close."""
        cx, cy, cz = center
        rt = radius if radius_top is None else radius_top
        bot, top = [], []
        for i in range(sides):
            a = 2 * math.pi * i / sides
            ca, sa = math.cos(a), math.sin(a)
            if axis == "Z":
                bot.append(self.bm.verts.new((cx + ca * radius, cy + sa * radius, cz - height / 2)))
                top.append(self.bm.verts.new((cx + ca * rt, cy + sa * rt, cz + height / 2)))
            else:  # Y axis
                bot.append(self.bm.verts.new((cx + ca * radius, cy - height / 2, cz + sa * radius)))
                top.append(self.bm.verts.new((cx + ca * rt, cy + height / 2, cz + sa * rt)))
        for i in range(sides):
            j = (i + 1) % sides
            uvs = [(i / sides, 0), (j / sides, 0), (j / sides, 1), (i / sides, 1)]
            if axis == "Z" and split:
                self.split_face([v.co for v in (bot[i], bot[j], top[j], top[i])], mat,
                                [(u * uv_scale[0], v * uv_scale[1]) for u, v in uvs], split)
            elif axis == "Z":
                self._face((bot[i], bot[j], top[j], top[i]), mat, uvs, uv_scale)
            else:
                self._face((bot[j], bot[i], top[i], top[j]), mat, uvs, uv_scale)
        if caps:
            if axis == "Z":
                self._face(list(reversed(bot)), mat)
                if rt > 1e-4:
                    self._face(top, mat)
            else:
                self._face(bot, mat)
                if rt > 1e-4:
                    self._face(list(reversed(top)), mat)

    def finish(self, name):
        # Vertices of the split faces replaced by their grid.
        loose = [v for v in self.bm.verts if not v.link_faces]
        if loose:
            bmesh.ops.delete(self.bm, geom=loose, context="VERTS")
        me = bpy.data.meshes.new(name)
        self.bm.normal_update()
        self.bm.to_mesh(me)
        self.bm.free()
        for m in self.mats:
            me.materials.append(m)
        return me


# ----------------------------------------------------------------------------- rig

class Rig:
    """Hierarchy of rigid pieces. Each piece has its origin on the joint."""

    def __init__(self, name):
        self.sc = bpy.context.scene
        self.root = bpy.data.objects.new(name, None)
        self.sc.collection.objects.link(self.root)
        self.parts = {}
        self.pivots = {name: (0.0, 0.0, 0.0)}
        self.root_name = name

    def part(self, name, pivot, parent=None, build=None):
        """`build(mb, px, py, pz)` adds geometry in world coordinates; it is
        recentred on the pivot. Without `build`, the piece is an empty object (attachment point)."""
        px, py, pz = pivot
        if build is not None:
            mb = MeshBuilder()
            build(mb)
            me = mb.finish(name + "_mesh")
            me.transform(__import__("mathutils").Matrix.Translation((-px, -py, -pz)))
            obj = bpy.data.objects.new(name, me)
        else:
            obj = bpy.data.objects.new(name, None)
            obj.empty_display_size = 0.05
        self.sc.collection.objects.link(obj)
        parent_name = parent or self.root_name
        parent_obj = self.parts.get(parent_name, self.root)
        obj.parent = parent_obj
        ppx, ppy, ppz = self.pivots[parent_name]
        obj.location = (px - ppx, py - ppy, pz - ppz)
        obj.rotation_mode = "XYZ"
        self.parts[name] = obj
        self.pivots[name] = pivot
        return obj


# ----------------------------------------------------------------------------- animation

_TIME_RE = re.compile(r"^(T|h\d+e?|c\d+|\d+)([+-]\d+)?$")


def resolve_time(spec, info):
    """Symbolic time: integer, "T" (duration), "h0" (start of hit 0), "h0e" (end), "c0" (tick of
    spell 0), with ±n."""
    if isinstance(spec, (int, float)):
        return float(spec)
    m = _TIME_RE.match(spec.replace(" ", ""))
    if not m:
        raise ValueError(f"invalid time: {spec}")
    base, off = m.group(1), int(m.group(2) or 0)
    if base == "T":
        v = info["total"]
    elif base.startswith("h"):
        idx = int(base[1:].rstrip("e"))
        v = info["hits"][idx][1 if base.endswith("e") else 0]
    elif base.startswith("c"):
        v = info["casts"][int(base[1:])]
    else:
        v = int(base)
    return float(v + off)


def merge(*poses):
    """Combines poses (later ones override earlier ones, piece by piece)."""
    out = {}
    for p in poses:
        for k, v in p.items():
            out[k] = v
    return out


def mirror(pose):
    """Swaps left/right (Y and Z rotations inverted)."""
    out = {}
    for k, v in pose.items():
        nk = k.replace("_L", "_TMP").replace("_R", "_L").replace("_TMP", "_R")
        if isinstance(v, dict):
            r = v.get("r", (0, 0, 0))
            t = v.get("t", (0, 0, 0))
            out[nk] = {"r": (r[0], -r[1], -r[2]), "t": (-t[0], t[1], t[2])}
        else:
            out[nk] = (v[0], -v[1], -v[2])
    return out


def _apply_pose(rig, pose, frame):
    for name, obj in rig.parts.items():
        v = pose.get(name, (0, 0, 0))
        if isinstance(v, dict):
            r = v.get("r", (0, 0, 0))
            t = v.get("t", (0, 0, 0))
        else:
            r, t = v, (0, 0, 0)
        obj.rotation_euler = tuple(math.radians(a) for a in r)
        ppx, ppy, ppz = rig.pivots[rig_parent_name(rig, name)]
        px, py, pz = rig.pivots[name]
        obj.location = (px - ppx + t[0], py - ppy + t[1], pz - ppz + t[2])
        obj.keyframe_insert("rotation_euler", frame=frame)
        obj.keyframe_insert("location", frame=frame)


def rig_parent_name(rig, name):
    p = rig.parts[name].parent
    return rig.root_name if p is rig.root else p.name


def add_animation(rig, name, keys, info=None, interp="BEZIER"):
    """Creates an action `name` animating all the pieces, and stores it in an NLA track.

    keys: list of (time, pose). Returns the duration in frames.
    """
    info = info or {"total": 0, "hits": []}
    act = bpy.data.actions.new(name)
    act.use_fake_user = True
    resolved = sorted(((resolve_time(t, info), p) for t, p in keys), key=lambda k: k[0])
    for obj in rig.parts.values():
        obj.animation_data_create()
        obj.animation_data.action = act
    for frame, pose in resolved:
        _apply_pose(rig, pose, frame)
    for obj in rig.parts.values():
        ad = obj.animation_data
        slot = ad.action_slot
        try:
            for fc in _fcurves(act, slot):
                for kp in fc.keyframe_points:
                    kp.interpolation = interp
        except Exception:
            pass
        tr = ad.nla_tracks.new()
        tr.name = name
        st = tr.strips.new(name, 0, act)
        st.action_slot = slot
        ad.action = None
    return resolved[-1][0]


def _fcurves(act, slot):
    # "Slotted actions" API (Blender 4.4+).
    for layer in act.layers:
        for strip in layer.strips:
            cb = strip.channelbag(slot)
            if cb:
                yield from cb.fcurves


def rest_pose(rig):
    for name, obj in rig.parts.items():
        ppx, ppy, ppz = rig.pivots[rig_parent_name(rig, name)]
        px, py, pz = rig.pivots[name]
        obj.location = (px - ppx, py - ppy, pz - ppz)
        obj.rotation_euler = (0, 0, 0)


# ----------------------------------------------------------------------------- export

def export(name, markers=None, selection=False, save_blend=True):
    os.makedirs(MODELS, exist_ok=True)
    os.makedirs(BLEND, exist_ok=True)
    path = os.path.join(MODELS, name + ".glb")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_animation_mode="ACTIONS",
        export_merge_animation="ACTION",
        export_force_sampling=True,
        export_optimize_animation_size=False,
        export_anim_slide_to_zero=False,
        export_image_format="AUTO",
        export_yup=True,
        export_apply=False,
        use_selection=selection,
    )
    if save_blend:
        bpy.ops.wm.save_as_mainfile(filepath=os.path.join(BLEND, name + ".blend"))
    if markers is not None:
        with open(os.path.join(MODELS, name + ".anim.json"), "w") as f:
            json.dump(markers, f, indent=1, sort_keys=True)
    print(f"[assets] {path}")


def anim_markers(info):
    """Markers (in frames) used at runtime to retime the animation if the tuning changes."""
    marks = [0]
    for s, e in info.get("hits", []):
        marks += [s, e]
    marks.append(info["total"])
    return marks
