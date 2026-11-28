# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Engine-independent port of the native MuJoCo owner's sensory projections.

Line-for-line ports of ``adapters/mujoco/src/legacy.rs`` (legacy-* kinds),
the C. elegans and simple native kinds of ``adapters/mujoco/src/world.rs``,
``crates/aarnn-embodiment/src/vision.rs`` (event cell) and the analytic
stimulus fields of ``crates/aarnn-embodiment/src/aer.rs``. A physics owner
(Isaac/PhysX, or MuJoCo in the parity tests) supplies a :class:`State`: body
frames, centre-of-mass velocities, proper accelerations and contacts.

Ray proxies (range finders, retinal cells) use :class:`Geometry`, the pinned
MJCF compiled by the ``mujoco`` package (bundled with ``isaacsim-core``) and
posed from the owner's state, so ``mj_ray`` sees exactly the geoms, groups,
alpha and plane extents the MuJoCo owner sees. No MuJoCo dynamics run here.
"""
import math
from dataclasses import dataclass, field

import numpy as np

RANGE_GROUPS = np.array([1, 1, 0, 1, 1, 1], dtype=np.uint8)
DT = 0.002


def norm(v, a, b):
    return min(max((v - a) / (b - a), 0.0), 1.0)


class Geometry:
    """Compiled MJCF used for parameters absent from USD and for ray proxies."""

    def __init__(self, xml_path):
        import mujoco
        self.mj = mujoco
        self.m = mujoco.MjModel.from_xml_path(str(xml_path))
        self.d = mujoco.MjData(self.m)
        mujoco.mj_forward(self.m, self.d)
        self._geomid = np.array([-1], dtype=np.int32)

    def body(self, name):
        i = self.mj.mj_name2id(self.m, self.mj.mjtObj.mjOBJ_BODY, name)
        if i < 0:
            raise KeyError(f'missing named model element {name}')
        return i

    def joint(self, name):
        j = self.mj.mj_name2id(self.m, self.mj.mjtObj.mjOBJ_JOINT, name)
        if j < 0:
            raise KeyError(f'missing named model element {name}')
        return int(self.m.jnt_qposadr[j]), int(self.m.jnt_dofadr[j]), j

    def body_name(self, i):
        return self.mj.mj_id2name(self.m, self.mj.mjtObj.mjOBJ_BODY, i)

    def geom_name(self, i):
        return self.mj.mj_id2name(self.m, self.mj.mjtObj.mjOBJ_GEOM, i) or ''

    def sync(self, qpos):
        """Pose every body/geom/camera from an owner's generalised position."""
        self.d.qpos[:] = qpos
        self.mj.mj_kinematics(self.m, self.d)
        self.mj.mj_camlight(self.m, self.d)

    def _ray(self, point, direction, groups, exclude):
        self._geomid[0] = -1
        p = np.asarray(point, dtype=np.float64)
        v = np.asarray(direction, dtype=np.float64)
        try:
            x = self.mj.mj_ray(self.m, self.d, p, v, groups, 1, int(exclude), self._geomid)
        except TypeError:
            x = self.mj.mj_ray(self.m, self.d, p, v, groups, 1, int(exclude), self._geomid, None)
        return x, int(self._geomid[0])

    def ray(self, point, direction, exclude, groups=RANGE_GROUPS):
        x, _ = self._ray(point, direction, groups, exclude)
        return x if x >= 0 else None

    def echo(self, point, direction, exclude, incidence_max=None):
        """``ray`` with the sonar echo rule of ``Physics::echo_beyond_self``:
        a hit at an angle of incidence above ``incidence_max`` returns nothing."""
        if incidence_max is None:
            return self.ray(point, direction, exclude)
        self._geomid[0] = -1
        p = np.asarray(point, dtype=np.float64)
        v = np.asarray(direction, dtype=np.float64)
        normal = np.zeros(3, dtype=np.float64)
        x = self.mj.mj_ray(self.m, self.d, p, v, RANGE_GROUPS, 1, int(exclude), self._geomid, normal)
        if x < 0:
            return None
        n, length = float(np.linalg.norm(normal)), float(np.linalg.norm(v))
        if n > 0 and length > 0:
            cos = abs(float(v @ normal)) / (n * length)
            if math.acos(min(max(cos, 0.0), 1.0)) > incidence_max:
                return None
        return x

    def ray_light(self, point, direction, exclude):
        x, geom = self._ray(point, direction, None, exclude)
        if x < 0 or geom < 0:
            return 0.1
        rgba = self.m.geom_rgba[geom]
        return float(sum(float(rgba[i]) * w for i, w in enumerate((0.2126, 0.7152, 0.0722))))

    def camera_state(self, name):
        c = self.mj.mj_name2id(self.m, self.mj.mjtObj.mjOBJ_CAMERA, name)
        if c < 0:
            raise KeyError(f'missing named model element {name}')
        return self.d.cam_xpos[c].copy(), self.d.cam_xmat[c].reshape(3, 3).copy(), int(self.m.cam_bodyid[c]), \
            float(self.m.cam_fovy[c])

    def irradiance_proxy(self, point, warm):
        total = 0.0
        for i in range(self.m.nlight):
            distance2 = float(((np.asarray(point) - self.d.light_xpos[i]) ** 2).sum())
            diffuse = self.m.light_diffuse[i]
            intensity = float(diffuse[0]) if warm else float(sum(float(x) for x in diffuse)) / 3.0
            total += intensity / (1.0 + distance2)
        return min(max(total, 0.0), 1.0)


@dataclass
class State:
    """Owner measurements in MuJoCo body order (index 0 is the world)."""
    qpos: np.ndarray                 # MuJoCo generalised position layout
    qvel: np.ndarray                 # MuJoCo generalised velocity layout
    body_pos: np.ndarray             # (nbody, 3) body frame origins (xpos)
    body_rot: np.ndarray             # (nbody, 3, 3) body-to-world (xmat)
    com_vel: np.ndarray              # (nbody, 6) [angular, linear] at COM, world axes
    proper_acc: np.ndarray           # (nbody, 3) COM linear acceleration minus gravity, world axes
    contact: np.ndarray              # (nbody,) any contact touching the body
    contact_force: np.ndarray = None   # (nbody, 3) net contact force on the body, world axes
    contact_points: dict = field(default_factory=dict)   # body -> [(point, other_geom_name)]


class Cell:
    """Port of vision::Cell (versioned log-luminance event cell)."""

    def __init__(self):
        self.previous = None
        self.memory = 0.0
        self.refractory = 0.0
        self.polarity = 0

    def sample(self, luminance, dt):
        if not math.isfinite(luminance) or not 0.0 <= luminance <= 1.0 or not math.isfinite(dt) or dt <= 0.0:
            raise ValueError('invalid optical sample/time')
        current = math.log1p(9.0 * luminance) / math.log(10.0)
        previous = current if self.previous is None else self.previous
        delta = current - previous
        self.memory = min(max(self.memory * math.exp(-dt / 0.045) + delta, -1.0), 1.0)
        result = [0.0, 0.0, current, min(abs(delta) / 0.08, 1.0)]
        if self.refractory > 0.0:
            self.refractory = max(self.refractory - dt, 0.0)
        elif self.memory >= 0.08 + (0.02 if self.polarity < 0 else 0.0):
            result[0] = 1.0
            self.polarity = 1
            self.refractory = 0.012
            self.memory *= 0.25
        elif self.memory <= -0.08 - (0.02 if self.polarity > 0 else 0.0):
            result[1] = 1.0
            self.polarity = -1
            self.refractory = 0.012
            self.memory *= 0.25
        elif abs(self.memory) < 0.04:
            self.polarity = 0
        self.previous = previous + 0.35 * delta
        return result


def stimulus_value(s, x, y):
    """Port of aer::Stimulus::value."""
    center = s.get('center', [0.0, 0.0])
    dx, dy = x - center[0], y - center[1]
    shape = s.get('shape')
    radii = s.get('radii', [0.0, 0.0])
    strength = s.get('strength', 0.0)
    if shape == 'gaussian':
        sx, sy = max(radii[0], 1e-6), max(radii[1], 1e-6)
        return strength * math.exp(-0.5 * ((dx / sx) ** 2 + (dy / sy) ** 2))
    if shape == 'ellipse':
        ax, ay = max(radii[0], 1e-6), max(radii[1], 1e-6)
        r = math.sqrt((dx / ax) ** 2 + (dy / ay) ** 2)
        edge = max(s.get('edge', 0.0) / (0.5 * (ax + ay)), 1e-6)
        z = (r - 1.0) / edge
        return strength / (1.0 + math.exp(z)) if z < 700 else 0.0
    if shape == 'ring':
        d = math.sqrt(dx * dx + dy * dy) - s.get('radius', 0.0)
        w = max(s.get('width', 0.0), 1e-6)
        return strength * math.exp(-0.5 * (d / (0.5 * w)) ** 2)
    if shape == 'linear':
        g = s.get('gradient', [0.0, 0.0])
        return s.get('offset', 0.0) + g[0] * x + g[1] * y
    if shape == 'constant':
        return strength
    return 0.0


def field_value(stimuli, name, x, y):
    return min(max(sum(stimulus_value(s, x, y) for s in stimuli if s.get('field') == name), 0.0), 1.0)


# ---- crates/aarnn-embodiment/src/acoustics.rs ------------------------------------------
SPEED_OF_SOUND = 343.0


def _source_on(src, t):
    if t < src.get('start_s', 0.0):
        return False
    period, on = src.get('period_s'), src.get('on_s')
    if period and on is not None and period > 0:
        return (t - src.get('start_s', 0.0)) % period < on
    return True


def _level_at_1m(src, band_hz):
    pts = sorted((f, db) for f, db in src.get('spectrum', []) if f > 0)
    if not pts or band_hz <= 0:
        return -200.0
    x = math.log(band_hz)
    if x <= math.log(pts[0][0]):
        return pts[0][1]
    for (f0, d0), (f1, d1) in zip(pts, pts[1:]):
        a, b = math.log(f0), math.log(f1)
        if x <= b:
            t = (x - a) / (b - a) if b > a else 0.0
            return d0 + t * (d1 - d0)
    return pts[-1][1]


def _source_level_db(src, rx_pos, rx_axis, directivity, band_hz):
    to = np.asarray(src['position'], dtype=float) - rx_pos
    r = max(float(np.linalg.norm(to)), 0.05)
    n = float(np.linalg.norm(rx_axis))
    cos = float(to @ rx_axis) / (r * n) if n > 0 else 1.0
    k = min(max(directivity, 0.0), 1.0) * min(max(band_hz / 1000.0, 0.0), 1.0)
    gain = max((1.0 - k) + k * min(max(cos, -1.0), 1.0), 0.05)
    return _level_at_1m(src, band_hz) - 20.0 * math.log10(r) + 20.0 * math.log10(gain)


def band_level_db(sources, rx_pos, rx_axis, directivity, band_hz, t):
    power = sum(10 ** (_source_level_db(s, rx_pos, rx_axis, directivity, band_hz) / 10.0)
                for s in sources if _source_on(s, t))
    return 10.0 * math.log10(power) if power > 0 else -200.0


def itd_normalised(sources, a, b, band_hz, t):
    """a, b: (position, axis, directivity) receivers; 0.5 when silent."""
    live = [(s, _source_level_db(s, *a, band_hz)) for s in sources if _source_on(s, t)]
    live = [x for x in live if x[1] > -100.0]
    spacing = float(np.linalg.norm(a[0] - b[0]))
    if not live or spacing <= 0:
        return 0.5
    src = max(live, key=lambda x: x[1])[0]
    p = np.asarray(src['position'], dtype=float)
    dt = (float(np.linalg.norm(p - a[0])) - float(np.linalg.norm(p - b[0]))) / SPEED_OF_SOUND
    return min(max(0.5 + 0.5 * dt / (spacing / SPEED_OF_SOUND), 0.0), 1.0)


class Senses:
    """Per-body sensory projection with the MuJoCo owner's per-step cache."""

    def __init__(self, record, geometry):
        self.record = record
        self.geometry = geometry
        # Energy-needs channels (optional group `needs`): Isaac has no energy
        # model, so they read neutral (full reserve, no refill, no cue) and are
        # spliced back into the record order after the engine projection.
        self.needs = [(i, s['kind']) for i, s in enumerate(record['io']['sensors']) if s['kind'].startswith('needs-')]
        self.sensors = [s for s in record['io']['sensors'] if not s['kind'].startswith('needs-')]
        self.profile = record['profile_id']
        self.cells = {}
        self.cache_step = None
        self.values = None
        self.held = {}
        self.dynamic_sound_sources = []   # moving sources set by the owner each step
        self.vibration_lp = np.zeros(3)
        self.vibration_step = None
        kinds = {s['kind'] for s in self.sensors}
        if self.profile == 'celegans':
            self.mode = 'celegans'
        elif all(k.startswith('legacy-') for k in kinds):
            self.mode = 'legacy'
        elif kinds <= {'angle', 'angular_velocity', 'body_gyro', 'body_gravity', 'range_ray'} and self.profile != 'crow_corvus_corone':
            self.mode = 'native'
        else:
            raise NotImplementedError(f'{self.profile}: sensory projection {sorted(kinds)} has no Isaac port yet')
        self._bodies = {}
        self._joints = {}
        for s in self.sensors:
            for name in [s.get('body')] + list(s.get('bodies') or []):
                if name:
                    self._bodies[name] = geometry.body(name)
            if s.get('joint'):
                self._joints[s['joint']] = geometry.joint(s['joint'])
        self._actuated = [(a, geometry.joint(a['joint'])) for a in record['actuators']]
        if self.mode == 'celegans':
            self._hinges = sorted(
                ((23.5 - (int(a['joint'][len('celegans_spine_'):]) - 1.0)) / 24.0, jq[0])
                for a, jq in self._actuated if a['joint'].startswith('celegans_spine_'))
            self._root = geometry.body('segment_00')
        scene = record.get('scene') or {}
        self.stimuli = scene.get('stimuli', [])

    def reset(self):
        self.cells.clear()
        self.cache_step = None
        self.held.clear()
        self.vibration_step = None

    def measure(self, st, step):
        if self.cache_step == step:
            return list(self.values)
        if self.mode == 'legacy':
            out = self._legacy(st, step)
        elif self.mode == 'celegans':
            out = self._celegans(st, step)
        else:
            out = self._native(st)
        for i, kind in self.needs:
            out.insert(i, 1.0 if kind == 'needs-energy' else 0.0)
        self.cache_step = step
        self.values = out
        return list(out)

    # ---- adapters/mujoco/src/legacy.rs -------------------------------------------------
    def _legacy(self, st, step=0):
        g = self.geometry
        cameras = {}
        out = []
        for index, s in enumerate(self.sensors):
            period = s.get('period_s')
            if period and index in self.held:
                at, value = self.held[index]
                if 0 <= step - at < max(1, round(period / DT)):
                    out.append(value)
                    continue
            body = self._bodies[s['body']]
            position, r, velocity = st.body_pos[body], st.body_rot[body], st.com_vel[body]
            axis = s.get('axis') or 0
            if axis >= 3:
                raise ValueError('sensor axis outside XYZ')
            kind = s['kind']
            if kind == 'legacy-angle':
                q, _, _ = self._joints[s['joint']]
                v = norm(float(st.qpos[q]), -3.14, 3.14)
            elif kind in ('legacy-accel', 'legacy-gyro'):
                if kind == 'legacy-accel':
                    vec, bound = r.T @ st.proper_acc[body], 20.0
                else:
                    vec, bound = r.T @ velocity[:3], 10.0
                if s.get('mode') == 'legacy-y-up':
                    vec = np.array([vec[0], vec[2], -vec[1]])
                v = norm(float(vec[axis]), -bound, bound)
            elif kind == 'legacy-contact':
                if s.get('region_m') is not None:
                    v = self._region_contact(st, body, np.asarray(s['region_m'], dtype=float))
                elif s.get('direction') is not None:
                    v = self._directional_contact(st, body, np.asarray(s['direction'], dtype=float))
                else:
                    v = 1.0 if st.contact[body] else 0.0
            elif kind == 'legacy-contact-force':
                if st.contact_force is None:
                    raise NotImplementedError('contact force measurement unavailable from this owner')
                v = norm(float((r.T @ st.contact_force[body])[axis]), -100.0, 100.0)
            elif kind == 'legacy-range':
                point = position + r @ np.asarray(s.get('position_m') or [0.0, 0.0, 0.0], dtype=float)
                reach = s['angle_rad']
                distance = reach
                for ray in s.get('rays') or [s['direction']]:
                    hit = g.echo(point, r @ np.asarray(ray, dtype=float), body, s.get('incidence_max_rad'))
                    if hit is not None:
                        distance = min(distance, hit)
                distance = max(min(distance, reach), s.get('min_m') or 0.0)
                lookup = s.get('lookup')
                if lookup:
                    raw = lookup[-1][1]
                    for a, b in zip(lookup, lookup[1:]):
                        if distance <= b[0]:
                            t = norm(distance, a[0], b[0])
                            raw = a[1] + t * (b[1] - a[1])
                            break
                    v = norm(raw, 120.0, 1000.0)
                else:
                    v = 1.0 - distance / reach
            elif kind == 'legacy-camera':
                name = s['camera']
                rows, cols = s['retina_shape']
                key = f'{name}#{rows}x{cols}'   # one eye may feed retinas of two shapes
                if key not in cameras:
                    if rows == 0 or cols == 0 or rows * cols > 192:
                        raise ValueError('camera bound')
                    point, cr, owner, fovy = g.camera_state(name)
                    vertical = math.tan(math.radians(fovy) / 2.0)
                    cells = self.cells.setdefault(key, [Cell() for _ in range(rows * cols)])
                    readings = []
                    for y in range(rows):
                        for x in range(cols):
                            ray = cr @ np.array([(2.0 * (x + 0.5) / cols - 1.0) * vertical * 4.0 / 3.0,
                                                 (1.0 - 2.0 * (y + 0.5) / rows) * vertical, -1.0])
                            ray = ray / math.sqrt(float(ray @ ray))
                            readings.append(cells[y * cols + x].sample(g.ray_light(point, ray, owner), DT))
                    cameras[key] = readings
                y, x = s['cell']
                v = cameras[key][y * cols + x][{'on': 0, 'off': 1, 'lum': 2, 'gradient': 3}[s['mode']]]
            elif kind == 'legacy-flow':
                lv = float((r.T @ velocity[3:])[axis])
                if s.get('mode') == 'left':
                    v = min(max(lv, 0.0), 1.0)
                elif s.get('mode') == 'right':
                    v = min(max(-lv, 0.0), 1.0)
                else:
                    v = min(abs(lv), 1.0)
            elif kind == 'legacy-acoustic':
                def receiver(spec):
                    bb = self._bodies[spec['body']]
                    pos, rot = st.body_pos[bb], st.body_rot[bb]
                    return (pos + rot @ np.asarray(spec.get('position_m') or [0.0, 0.0, 0.0], dtype=float),
                            rot @ np.asarray(spec.get('direction') or [1.0, 0.0, 0.0], dtype=float),
                            float(spec.get('directivity') or 0.0))
                sources = list((self.record.get('scene') or {}).get('sound_sources') or []) + list(self.dynamic_sound_sources)
                t = step * DT
                rx = receiver(s)
                if s.get('mode') == 'itd':
                    pair = next(x for x in self.sensors if x['name'] == s['pair'])
                    v = itd_normalised(sources, rx, receiver(pair), s['band_hz'], t)
                else:
                    level = band_level_db(sources, *rx, s['band_hz'], t)
                    lo, hi = s.get('floor_db', 20.0), s.get('ceil_db', 100.0)
                    v = min(max((level - lo) / (hi - lo), 0.0), 1.0) if hi > lo else 0.0
            elif kind == 'legacy-depth':
                v = min(max((0.4 - float(position[2])) / 0.4, 0.0), 1.0)
            elif kind == 'legacy-pitch':
                v = norm(math.asin(min(max(-float(r[2][0]), -1.0), 1.0)), -math.pi / 2, math.pi / 2)
            else:
                raise NotImplementedError(f"unimplemented native sensor {s['name']} {kind}")
            if not math.isfinite(v):
                raise ValueError(f"nonfinite sensor {s['name']}")
            if period:
                self.held[index] = (step, min(max(v, 0.0), 1.0))
            out.append(min(max(v, 0.0), 1.0))
        return out

    @staticmethod
    def _region_contact(st, body, region):
        position, r = st.body_pos[body], st.body_rot[body]
        for point, _ in st.contact_points.get(body, ()):
            local = r.T @ (np.asarray(point) - position)
            if np.all(local >= region[0]) and np.all(local <= region[1]):
                return 1.0
        return 0.0

    @staticmethod
    def _directional_contact(st, body, direction):
        position, r = st.body_pos[body], st.body_rot[body]
        for point, _ in st.contact_points.get(body, ()):
            local = r.T @ (np.asarray(point) - position)
            if float(local @ direction) > 0.0:
                return 1.0
        return 0.0

    # ---- adapters/mujoco/src/world.rs (common native kinds) ------------------------------
    def _native(self, st):
        values = {}
        for a, (q, v, _) in self._actuated:
            values[a['joint'] + '_angle'] = float(st.qpos[q])
            values[a['joint'] + '_angular_velocity'] = float(st.qvel[v])
        position, r, velocity = st.body_pos[1], st.body_rot[1], st.com_vel[1]
        omega = r.T @ velocity[:3]
        out = []
        for s in self.sensors:
            if s['kind'] in ('angle', 'angular_velocity'):
                raw = values[s['name']]
            elif s['kind'] == 'body_gyro':
                raw = float(omega[s['axis']])
            elif s['kind'] == 'body_gravity':
                raw = float((r.T @ np.array([0.0, 0.0, -1.0]))[s['axis']])
            else:
                angle = s['angle_rad']
                point = position + r @ np.array([0.0, 0.0, 0.1])
                hit = self.geometry.ray(point, r @ np.array([math.cos(angle), math.sin(angle), 0.0]), 1)
                raw = min(3.0 if hit is None else hit, 3.0)
            out.append(norm(raw, *s['range']))
        return out

    # ---- adapters/mujoco/src/world.rs (C. elegans v2 projection) -----------------------
    def _celegans_bend(self, st, k):
        ap = 0.06 + 0.88 * max(k - 1, 0) / 11.0
        hinges = [(f, float(st.qpos[q])) for f, q in self._hinges]
        if ap <= hinges[0][0]:
            return hinges[0][1]
        for a, b in zip(hinges, hinges[1:]):
            if ap <= b[0]:
                t = (ap - a[0]) / (b[0] - a[0])
                return a[1] + t * (b[1] - a[1])
        return hinges[-1][1]

    def _mount(self, st, s):
        body = self._bodies[s['body']]
        p, r = st.body_pos[body], st.body_rot[body]
        return body, p + r @ np.asarray(s['position_m'], dtype=float), r

    def _object_contact(self, st, body):
        for _, other in st.contact_points.get(body, ()):
            if other not in ('ground', 'floor', 'platform') and not other.startswith('segment_'):
                return 1.0
        return 0.0

    def _celegans(self, st, step):
        root = self._root
        r = st.body_rot[root]
        a = r.T @ st.proper_acc[root]
        omega = r.T @ st.com_vel[root][:3]
        accel = np.array([a[0], a[2], -a[1]])
        gyro = np.array([omega[0], omega[2], -omega[1]])
        if self.vibration_step != step:
            alpha = 1.0 - math.exp(-0.002 / 0.2)
            if self.vibration_step is None:
                self.vibration_lp = accel.copy()
            self.vibration_lp += alpha * (accel - self.vibration_lp)
            self.vibration_step = step
        out = []
        for s in self.sensors:
            kind = s['kind']
            if kind == 'celegans-inertial-v1':
                axis = s['axis']
                v = norm(accel[axis], -20, 20) if 'accel' in s['name'] else norm(gyro[axis], -10, 10)
            elif kind == 'celegans-vibration-v2':
                v = min(max(abs(accel[s['axis']] - self.vibration_lp[s['axis']]) / 20.0, 0.0), 1.0)
            elif kind == 'celegans-rotation-v2':
                v = min(max(abs(gyro[s['axis']]) / 10.0, 0.0), 1.0)
            elif kind == 'celegans-contact-v1':
                names = s.get('bodies') or [s['body']]
                v = max(self._object_contact(st, self._bodies[n]) for n in names)
            elif kind == 'celegans-field-v2':
                _, point, _ = self._mount(st, s)
                v = field_value(self.stimuli, s['mode'], point[0], point[1])
            elif kind in ('celegans-range-v2', 'celegans-range-v1'):
                body, point, rr = self._mount(st, s)
                angle = s['angle_rad']
                direction = rr @ np.array([math.cos(angle), math.sin(angle), 0.0])
                groups = np.array([1, 0, 0, 1, 1, 1], dtype=np.uint8) if kind == 'celegans-range-v2' else RANGE_GROUPS
                hit = self.geometry.ray(point, direction, body, groups)
                v = 1.0 - norm(s['range'][1] if hit is None else hit, s['range'][0], s['range'][1])
            elif kind == 'celegans-bend-v1':
                theta = self._celegans_bend(st, s['axis'])
                reach = max(s['range'][1], 1e-6)
                v = {'dorsal': min(max(-theta / reach, 0.0), 1.0),
                     'ventral': min(max(theta / reach, 0.0), 1.0)}[s['mode']]
            elif kind == 'celegans-irradiance-v1':
                _, point, _ = self._mount(st, s)
                v = self.geometry.irradiance_proxy(point, s.get('axis') == 0)
            else:
                raise NotImplementedError('C. elegans projection kind ' + kind)
            if not math.isfinite(v):
                raise ValueError('nonfinite C. elegans sensor')
            out.append(min(max(v, 0.0), 1.0))
        return out
