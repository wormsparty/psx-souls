"""Target-driven posing for the humanoid rigs (the player), baked one key per frame.

`common.add_animation` interpolates joint angles between a handful of poses: feet slide, hands
wander off their line and every move looks like a morph between two statues. Here a pose says
*where things are* rather than how joints are bent:

- the pelvis (offset from its rest position, and its rotation), the chest and the head;
- each foot: where it stands on the ground, its heading, how far the heel is off the floor;
- each hand: where the grip is and where the blade points.

Legs and arms are solved (two-bone IK) on every frame, so a planted foot stays planted while the
pelvis travels, and a thrusting hand moves along a line. A foot can also be planted in the
*world*: the simulation slides the character forward during an attack (`motion` in the tuning),
the foot is then held back by that same distance and doesn't skate.

Author coordinates are `(r, f, u)`: metres to the character's right, forwards and up. Angles are
degrees: `yaw` positive to the left, torso `lean` positive forwards, `roll` positive to the left,
blade `pitch` positive upwards.

Keys are sparse per channel, as in any animation package: a key only moves the channels it names
(`hips`, `hips_rot`, `chest`, `head`, `foot_R`, `foot_L`, `hand_R`, `hand_L`), the others keep
gliding between their own neighbouring keys. That is what gives overlap: the pelvis can start
before the arm, the off hand can trail behind.
"""

import math

import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

from common import resolve_time

CHANNELS = ("hips", "hips_rot", "chest", "head", "foot_R", "foot_L", "hand_R", "hand_L")
SIDES = (("R", -1.0), ("L", 1.0))  # Blender X sign of each side
ANKLE = 0.08  # height of the ankle above the sole
BALL = Vector((0.0, -0.12, -ANKLE))  # ball of the foot, from the ankle


def V(r, f, u):
    """Author coordinates → Blender."""
    return Vector((-r, -f, u))


def rot(lean=0.0, roll=0.0, yaw=0.0):
    return Euler((math.radians(lean), math.radians(roll), math.radians(yaw)), "XYZ").to_matrix()


def rz(yaw):
    return Matrix.Rotation(math.radians(yaw), 3, "Z")


def aim_matrix(aim):
    """Blade orientation: heading, elevation, then roll around the blade."""
    yaw, pitch, roll = aim
    return rz(yaw) @ Matrix.Rotation(math.radians(-pitch), 3, "X") @ Matrix.Rotation(math.radians(roll), 3, "Y")


# ----------------------------------------------------------------------------- pose helpers

def foot(r, f, u=0.0, yaw=0.0, heel=0.0, space="root", knee=0.0, lift=0.0):
    """A foot. `space`: "root" (follows the character), "world" (planted: held back by the root
    motion of the move) or "hips" (relative to the pelvis: rolls, jumps). `knee` splays the knee
    (degrees, positive to the left), `lift` arcs the foot off the ground on its way to this key."""
    return {"pos": (r, f, u), "yaw": yaw, "heel": heel, "space": space, "knee": knee, "lift": lift}


def hand(r, f, u, aim=None, space="root", elbow=None, arc=None):
    """A hand: position of the grip (of the wrist without `aim`). `aim` = (yaw, pitch, roll) of
    the blade; without it the hand follows the forearm. `space`: "root" or "chest". `elbow`:
    direction the elbow points to, in the chest's frame. `arc` = (r, f): on its way to this key
    the hand circles around that vertical axis instead of cutting straight through."""
    return {"pos": (r, f, u), "aim": aim, "space": space, "elbow": elbow, "arc": arc}


def hilt(along=0.17, elbow=None):
    """The off hand on the weapon's grip, `along` metres behind the main hand."""
    return {"pos": (0.0, -along, 0.0), "aim": None, "space": "hilt", "elbow": elbow, "arc": None}


def pose(*parts, **channels):
    """Merges poses channel by channel (later ones win)."""
    out = {}
    for p in parts:
        out.update(p)
    out.update(channels)
    return out


def tweak(limb, **changes):
    """Copy of a foot or a hand with some fields changed; `d=(r, f, u)` offsets its position."""
    out = dict(limb)
    d = changes.pop("d", None)
    if d:
        out["pos"] = tuple(a + b for a, b in zip(out["pos"], d))
    out.update(changes)
    return out


# ----------------------------------------------------------------------------- easing

def _ease(kind, t):
    if kind == "lin":
        return t
    if kind == "in":  # accelerates into the key
        return t * t
    if kind == "out":  # leaves fast, settles on the key
        return 1 - (1 - t) ** 2
    if kind == "snap":  # most of the way in the first third: strikes
        return 1 - (1 - t) ** 3
    if kind == "wind":  # slow start, whips into the key: wind-ups feeding a strike
        return t ** 3
    return t * t * (3 - 2 * t)  # "io"


def _lerp(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


# ----------------------------------------------------------------------------- IK

# Furthest a target was out of reach since the last reset (metres) and which limb it was: `bake`
# reports it, a limb that can't reach its target visibly lets go of it.
STRETCH = [0.0, "", ""]

def two_bone(start, target, l1, l2, pole, sign):
    """Rotations (world) of the two bones of a limb hanging along -Z at rest, reaching from
    `start` to `target` with the middle joint pushed towards `pole`. `sign`: +1 for a knee
    (bends forwards), -1 for an elbow (bends backwards)."""
    d = target - start
    dist = min(max(d.length, abs(l1 - l2) + 1e-3), (l1 + l2) * 0.9995)
    if d.length - (l1 + l2) > STRETCH[0]:
        STRETCH[:2] = [d.length - (l1 + l2), STRETCH[2]]
    along = d.normalized() if d.length > 1e-6 else Vector((0, 0, -1))
    perp = pole - along * pole.dot(along)
    if perp.length < 1e-4:
        perp = Vector((0, -sign, 0)) - along * (-sign * along.y)
    perp.normalize()
    a = (l1 * l1 - l2 * l2 + dist * dist) / (2 * dist)
    h = math.sqrt(max(l1 * l1 - a * a, 0.0))
    mid = start + along * a + perp * h
    end = start + along * dist
    n = along.cross(perp).normalized()

    def frame(direction):
        z = -direction.normalized()
        x = -n * sign
        y = z.cross(x)
        return Matrix((x, y, z)).transposed()

    return frame(mid - start), frame(end - mid), end


# ----------------------------------------------------------------------------- evaluation

class Clip:
    """Keys of one animation, resolved to frames, and its root motion (for planted feet)."""

    def __init__(self, keys, info):
        self.info = info
        self.tracks = {c: [] for c in CHANNELS}
        for key in keys:
            t, p = key[0], key[1]
            ease = key[2] if len(key) > 2 else "io"
            frame = resolve_time(t, info)
            for c, v in p.items():
                if c not in self.tracks:
                    raise ValueError(f"unknown channel: {c}")
                self.tracks[c].append((frame, v, ease))
        for c, tr in self.tracks.items():
            tr.sort(key=lambda k: k[0])
            if not tr or tr[0][0] > 0:
                raise ValueError(f"channel {c} has no key at frame 0")
        self.frames = int(max(tr[-1][0] for tr in self.tracks.values()))

    def travel(self, frame):
        """Metres the simulation has moved the character forwards by `frame`."""
        return sum(m["speed"] * min(max(frame - m["start"], 0), m["end"] - m["start"]) / 60.0
                   for m in self.info.get("motion", []))

    def segment(self, channel, frame):
        """(previous key, next key, eased 0..1) around `frame`."""
        tr = self.tracks[channel]
        prev = tr[0]
        for k in tr:
            if k[0] > frame:
                t = (frame - prev[0]) / (k[0] - prev[0])
                return prev[1], k[1], _ease(k[2], t)
            prev = k
        return prev[1], prev[1], 1.0


class Solver:
    def __init__(self, rig):
        self.rig = rig
        p = {k: Vector(v) for k, v in rig.pivots.items()}
        self.p = p
        self.thigh = (p["thigh_R"] - p["shin_R"]).length
        self.shin = (p["shin_R"] - p["foot_R"]).length
        self.upper = (p["upper_arm_R"] - p["forearm_R"]).length
        self.fore = (p["forearm_R"] - p["hand_R"]).length
        self.grip = {s: p[f"grip_{s}"] - p[f"hand_{s}"] for s, _ in SIDES}

    # Each `_foot` / `_hand` resolves one key of a limb under the current body: the two keys of
    # a segment may live in different spaces, they are blended once brought to the same one.

    def _foot(self, key, hips_pos, hips_rot, hips_yaw, travel):
        r, f, u = key["pos"]
        if key["space"] == "hips":
            flat = hips_rot @ rz(key["yaw"])
            ankle = hips_pos + hips_rot @ V(r, f, u)
            pole = hips_rot @ rz(key["knee"]) @ Vector((0, -1, 0))
            return ankle, flat @ Matrix.Rotation(math.radians(key["heel"]), 3, "X"), pole
        if key["space"] == "world":
            f -= travel
        flat = rz(key["yaw"])
        full = flat @ Matrix.Rotation(math.radians(key["heel"]), 3, "X")
        # The heel comes up around the ball of the foot, which stays where it was.
        ankle = V(r, f, u + ANKLE) + flat @ BALL - full @ BALL
        pole = rz(0.6 * key["yaw"] + 0.4 * hips_yaw + key["knee"]) @ Vector((0, -1, 0))
        return ankle, full, pole

    def _hand(self, key, side, sx, chest_pos, chest_rot, sword):
        """Wrist position, hand rotation (None: follows the forearm), elbow pole."""
        base = Vector((sx * 0.5, 0.75, -0.5))
        pole = chest_rot @ (V(*key["elbow"]) if key["elbow"] else base)
        if key["space"] == "hilt":
            pos, rm = sword
            grip = pos + rm @ V(*key["pos"])
            return grip - rm @ self.grip[side], rm, pole
        rm = aim_matrix(key["aim"]) if key["aim"] else None
        pos = V(*key["pos"])
        if key["space"] == "chest":
            pos = chest_pos + chest_rot @ pos
            rm = chest_rot @ rm if rm else None
        if rm:
            pos = pos - rm @ self.grip[side]
        return pos, rm, pole

    def solve(self, clip, frame, tails):
        """Local (location, rotation matrix) of every piece at `frame`."""
        p = self.p
        ch = {c: clip.segment(c, frame) for c in CHANNELS}
        val = lambda c: _lerp(*ch[c])
        travel = clip.travel(frame)

        hips_rot_v = val("hips_rot")
        hips_rot = rot(*hips_rot_v)
        hips_pos = p["hips"] + V(*val("hips"))
        chest_local, head_local = rot(*val("chest")), rot(*val("head"))
        chest_rot = hips_rot @ chest_local
        chest_pos = hips_pos + hips_rot @ (p["chest"] - p["hips"])
        out = {"hips": (hips_pos, hips_rot), "chest": (None, chest_local), "head": (None, head_local)}

        for side, sx in SIDES:
            a, b, t = ch[f"foot_{side}"]
            (pa, ra, pla), (pb, rb, plb) = (self._foot(k, hips_pos, hips_rot, hips_rot_v[2], travel) for k in (a, b))
            ankle = pa.lerp(pb, t)
            ankle.z += b["lift"] * math.sin(math.pi * t)
            foot_rot = ra.to_quaternion().slerp(rb.to_quaternion(), t).to_matrix()
            joint = hips_pos + hips_rot @ (p[f"thigh_{side}"] - p["hips"])
            STRETCH[2] = f"foot_{side} at frame {frame}"
            r1, r2, _ = two_bone(joint, ankle, self.thigh, self.shin, pla.lerp(plb, t), 1.0)
            thigh_local = hips_rot.inverted() @ r1
            out[f"thigh_{side}"] = (None, thigh_local)
            out[f"shin_{side}"] = (None, r1.inverted() @ r2)
            out[f"foot_{side}"] = (None, r2.inverted() @ foot_rot)
            # Coat tail: follows its thigh most of the way, a few frames late.
            want = Quaternion().slerp(thigh_local.to_quaternion(), 0.7)
            tails[side] = tails[side].slerp(want, 0.4) if side in tails and tails[side] else want
            out[f"coat_{side}"] = (None, tails[side].to_matrix())

        sword = None
        for side, sx in SIDES:
            a, b, t = ch[f"hand_{side}"]
            (pa, ra, pla), (pb, rb, plb) = (self._hand(k, side, sx, chest_pos, chest_rot, sword) for k in (a, b))
            numeric = a["aim"] and b["aim"] and a["space"] == b["space"] == "root"
            if numeric:
                # Same space: blend the angles themselves, so a sweep can turn more than 180°.
                hand_rot = aim_matrix(_lerp(a["aim"], b["aim"], t))
                pa, pb = V(*a["pos"]), V(*b["pos"])
            if b["arc"] and a["space"] == b["space"] == "root":
                c = V(b["arc"][0], b["arc"][1], 0)
                va, vb = pa - c, pb - c
                aa, ab = math.atan2(va.y, va.x), math.atan2(vb.y, vb.x)
                ang = aa + ((ab - aa + math.pi) % (2 * math.pi) - math.pi) * t
                rad = va.xy.length + (vb.xy.length - va.xy.length) * t
                wrist = Vector((c.x + math.cos(ang) * rad, c.y + math.sin(ang) * rad, va.z + (vb.z - va.z) * t))
            else:
                wrist = pa.lerp(pb, t)
            if numeric:
                wrist = wrist - hand_rot @ self.grip[side]
            elif ra or rb:
                # A free hand hangs in line with the forearm: stand in for it with the chest.
                qa = (ra or chest_rot).to_quaternion()
                qb = (rb or chest_rot).to_quaternion()
                hand_rot = qa.slerp(qb, t).to_matrix()
            else:
                hand_rot = None
            STRETCH[2] = f"hand_{side} at frame {frame}"
            shoulder = chest_pos + chest_rot @ (p[f"upper_arm_{side}"] - p["chest"])
            r1, r2, end = two_bone(shoulder, wrist, self.upper, self.fore, pla.lerp(plb, t), -1.0)
            out[f"upper_arm_{side}"] = (None, chest_rot.inverted() @ r1)
            out[f"forearm_{side}"] = (None, r1.inverted() @ r2)
            out[f"hand_{side}"] = (None, r2.inverted() @ hand_rot if hand_rot else Matrix.Identity(3))
            if side == "R":
                hr = hand_rot or r2
                sword = (end + hr @ self.grip[side], hr)
        self.sword = sword
        return out


def bake(rig, name, keys, info=None, loop=False):
    """Creates the action `name` (one key per frame on every piece) and stores it in an NLA
    track, like `common.add_animation`. Returns the duration in frames."""
    info = info or {"total": 0, "hits": []}
    clip = Clip(keys, info)
    solver = Solver(rig)
    act = bpy.data.actions.new(name)
    act.use_fake_user = True
    for obj in rig.parts.values():
        obj.animation_data_create()
        obj.animation_data.action = act
    tails, prev = {}, {}
    STRETCH[:2] = [0.0, ""]
    # A looping clip is run once beforehand so that whatever lags (the coat) is already in step.
    frames = list(range(clip.frames + 1))
    for i, frame in enumerate(frames * 2 if loop else frames):
        solved = solver.solve(clip, frame, tails)
        if loop and i <= clip.frames:
            continue
        for part, obj in rig.parts.items():
            loc, rm = solved.get(part, (None, Matrix.Identity(3)))
            if loc is None:
                parent = obj.parent.name if obj.parent is not rig.root else rig.root_name
                loc = Vector(rig.pivots[part]) - Vector(rig.pivots[parent])
            e = rm.to_euler("XYZ", prev[part]) if part in prev else rm.to_euler("XYZ")
            prev[part] = e
            obj.location = loc
            obj.rotation_euler = e
            obj.keyframe_insert("rotation_euler", frame=frame)
            obj.keyframe_insert("location", frame=frame)
    for obj in rig.parts.values():
        ad = obj.animation_data
        slot = ad.action_slot
        for layer in act.layers:
            for strip in layer.strips:
                cb = strip.channelbag(slot)
                for fc in (cb.fcurves if cb else ()):
                    for kp in fc.keyframe_points:
                        kp.interpolation = "LINEAR"
        tr = ad.nla_tracks.new()
        tr.name = name
        st = tr.strips.new(name, 0, act)
        st.action_slot = slot
        ad.action = None
    if STRETCH[0] > 0.03:
        print(f"[stretch] {name}: {STRETCH[1]} is {STRETCH[0] * 100:.0f} cm short of its target")
    return float(clip.frames)
