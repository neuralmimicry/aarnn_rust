#!/usr/bin/env python3
"""
Build a NAO-focused Runner snapshot JSON and matching NetworkConfig JSON.

Goal:
  - Match NAO IPC dimensions observed at runtime (default O=40, S derived from camera event grid).
  - Produce a deterministic, multi-layer AARNN snapshot for embodied control.
  - Enable 3D growth + morphology + biological dynamics by default.
  - Reverse-engineer sensor/actuator labels from the NAO proto when available.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import urllib.parse
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Dict, Iterable, List, Sequence, Tuple


NAME_RE = re.compile(r'\bname\s+"([^"]+)"')
TYPE_RE = re.compile(r'\btype\s+"([^"]+)"')
EXTERNPROTO_RE = re.compile(r'^\s*EXTERNPROTO\s+"([^"]+)"', flags=re.MULTILINE)
CAMERA_EVENT_RE = re.compile(r"^(?P<base>.+)\.(?P<polarity>on|off)\.r(?P<row>\d+)c(?P<col>\d+)$")


def iter_node_blocks(proto_text: str, node_type: str) -> Iterable[str]:
    """Yield textual blocks for `NodeType { ... }` occurrences."""
    pattern = re.compile(rf"\b{re.escape(node_type)}\b\s*\{{")
    for match in pattern.finditer(proto_text):
        start = match.end() - 1
        depth = 0
        i = start
        n = len(proto_text)
        while i < n:
            ch = proto_text[i]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    yield proto_text[start : i + 1]
                    break
            i += 1


def first_name(block: str) -> str | None:
    m = NAME_RE.search(block)
    if not m:
        return None
    name = m.group(1).strip()
    return name or None


def dedupe_keep_order(values: Sequence[str]) -> List[str]:
    seen = set()
    out: List[str] = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        out.append(value)
    return out


def is_http_url(value: str) -> bool:
    v = value.strip().lower()
    return v.startswith("http://") or v.startswith("https://")


def index_digits(n: int) -> int:
    max_index = max(0, n - 1)
    digits = 1
    while max_index >= 10:
        max_index //= 10
        digits += 1
    return max(2, digits)


def camera_event_channels(camera_name: str, rows: int, cols: int) -> List[str]:
    row_digits = index_digits(rows)
    col_digits = index_digits(cols)
    out: List[str] = []
    for r in range(rows):
        for c in range(cols):
            out.append(f"{camera_name}.on.r{r:0{row_digits}d}c{c:0{col_digits}d}")
            out.append(f"{camera_name}.off.r{r:0{row_digits}d}c{c:0{col_digits}d}")
    return out


def sort_sensor_channels_for_mapper(channels: Sequence[str]) -> List[str]:
    axis_order = {
        "x": 0,
        "y": 1,
        "z": 2,
        "mean": 3,
        "center": 4,
        "mean_gray": 3,
        "center_gray": 4,
    }

    def key(channel: str) -> Tuple[str, int, int, int, int, int, str]:
        cam = CAMERA_EVENT_RE.match(channel)
        if cam:
            base = cam.group("base")
            row = int(cam.group("row"))
            col = int(cam.group("col"))
            polarity = 0 if cam.group("polarity") == "on" else 1
            return (base, 0, 0, row, col, polarity, channel)
        if "." in channel:
            base, axis = channel.rsplit(".", 1)
            if axis in axis_order:
                return (base, 0, 1, axis_order[axis], 0, 0, channel)
        return (channel, 1, 0, 0, 0, 0, channel)

    return sorted(dedupe_keep_order(channels), key=key)


def parse_nao_channels_text(
    text: str,
    camera_retina_width: int,
    camera_retina_height: int,
) -> Tuple[List[str], List[str]]:

    sensor_devices: List[Tuple[str, List[str]]] = []
    actuator_devices: List[str] = []

    for block in iter_node_blocks(text, "Accelerometer"):
        name = first_name(block) or "accelerometer"
        sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))

    for block in iter_node_blocks(text, "Camera"):
        name = first_name(block)
        if name:
            sensor_devices.append(
                (name, camera_event_channels(name, camera_retina_height, camera_retina_width))
            )

    for block in iter_node_blocks(text, "Gyro"):
        name = first_name(block) or "gyro"
        sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))

    for block in iter_node_blocks(text, "DistanceSensor"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "LightSensor"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "TouchSensor"):
        name = first_name(block)
        if not name:
            continue
        t_match = TYPE_RE.search(block)
        t = (t_match.group(1).strip().lower() if t_match else "")
        if "force-3d" in t or "force3d" in t:
            sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))
        else:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "PositionSensor"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "RotationalMotor"):
        name = first_name(block)
        if name:
            actuator_devices.append(name)

    for block in iter_node_blocks(text, "LinearMotor"):
        name = first_name(block)
        if name:
            actuator_devices.append(name)

    sensor_channels: List[str] = []
    for _, channels in sorted(sensor_devices, key=lambda item: item[0]):
        sensor_channels.extend(channels)
    sensor_channels = sort_sensor_channels_for_mapper(sensor_channels)
    actuator_devices = sorted(dedupe_keep_order(actuator_devices))

    return sensor_channels, actuator_devices


def parse_nao_channels(
    nao_proto: Path,
    camera_retina_width: int,
    camera_retina_height: int,
) -> Tuple[List[str], List[str]]:
    text = nao_proto.read_text(encoding="utf-8")
    return parse_nao_channels_text(text, camera_retina_width, camera_retina_height)


def read_proto_source_text(source: str) -> str:
    if is_http_url(source):
        with urllib.request.urlopen(source, timeout=15.0) as response:
            data = response.read()
        return data.decode("utf-8", errors="replace")
    return Path(source).expanduser().read_text(encoding="utf-8")


def resolve_externproto_ref(base_source: str, ref: str) -> str:
    ref = ref.strip()
    if not ref:
        return ref
    if is_http_url(ref):
        return ref
    if is_http_url(base_source):
        return urllib.parse.urljoin(base_source, ref)
    base_path = Path(base_source).expanduser()
    return str((base_path.parent / ref).resolve())


def parse_nao_channels_recursive(
    proto_source: str,
    camera_retina_width: int,
    camera_retina_height: int,
    max_depth: int = 6,
) -> Tuple[List[str], List[str]]:
    all_sensory: List[str] = []
    all_output: List[str] = []
    visited: set[str] = set()
    stack: List[Tuple[str, int]] = [(proto_source, 0)]

    while stack:
        source, depth = stack.pop()
        if not source:
            continue
        key = source if is_http_url(source) else str(Path(source).expanduser().resolve())
        if key in visited:
            continue
        visited.add(key)

        text = read_proto_source_text(source)
        sensory, output = parse_nao_channels_text(text, camera_retina_width, camera_retina_height)
        all_sensory.extend(sensory)
        all_output.extend(output)

        if depth >= max_depth:
            continue

        for ref in EXTERNPROTO_RE.findall(text):
            if not ref.lower().endswith(".proto"):
                continue
            stack.append((resolve_externproto_ref(source, ref), depth + 1))

    return sort_sensor_channels_for_mapper(all_sensory), sorted(dedupe_keep_order(all_output))


def find_nao_proto(explicit: str | None) -> Path | None:
    def proto_candidates_from_root(root: Path) -> List[Path]:
        return [
            root / "projects" / "robots" / "softbank" / "nao" / "protos" / "Nao.proto",
            root / "resources" / "projects" / "robots" / "softbank" / "nao" / "protos" / "Nao.proto",
            root / "share" / "webots" / "projects" / "robots" / "softbank" / "nao" / "protos" / "Nao.proto",
            root
            / "usr"
            / "share"
            / "webots"
            / "projects"
            / "robots"
            / "softbank"
            / "nao"
            / "protos"
            / "Nao.proto",
        ]

    candidates: List[Path] = []
    if explicit:
        explicit_path = Path(explicit).expanduser()
        candidates.append(explicit_path)
        if explicit_path.is_dir():
            candidates.extend(proto_candidates_from_root(explicit_path))

    env_nao = os.environ.get("NAO_PROTO_FILE")
    if env_nao:
        env_path = Path(env_nao).expanduser()
        candidates.append(env_path)
        if env_path.is_dir():
            candidates.extend(proto_candidates_from_root(env_path))

    webots_home = os.environ.get("WEBOTS_HOME")
    if webots_home:
        candidates.extend(proto_candidates_from_root(Path(webots_home).expanduser()))

    webots_bin_env = os.environ.get("WEBOTS_EXECUTABLE") or os.environ.get("WEBOTS_BINARY")
    webots_bin = webots_bin_env or shutil.which("webots")
    if webots_bin:
        webots_bin_path = Path(webots_bin).expanduser()
        for root in {
            webots_bin_path.parent,
            webots_bin_path.parent.parent,
            webots_bin_path.resolve().parent,
            webots_bin_path.resolve().parent.parent,
        }:
            candidates.extend(proto_candidates_from_root(root))

    candidates.extend(
        [
            Path("/usr/local/webots/resources/projects/robots/softbank/nao/protos/Nao.proto"),
            Path("/usr/local/webots/projects/robots/softbank/nao/protos/Nao.proto"),
            Path("/opt/webots/resources/projects/robots/softbank/nao/protos/Nao.proto"),
            Path("/opt/webots/projects/robots/softbank/nao/protos/Nao.proto"),
            Path("/usr/share/webots/resources/projects/robots/softbank/nao/protos/Nao.proto"),
            Path("/usr/share/webots/projects/robots/softbank/nao/protos/Nao.proto"),
            Path("/snap/webots/current/usr/share/webots/projects/robots/softbank/nao/protos/Nao.proto"),
        ]
    )

    seen: set[str] = set()
    for path in candidates:
        normalized = path.expanduser().resolve()
        key = str(normalized)
        if key in seen:
            continue
        seen.add(key)
        if normalized.exists():
            return normalized
    return None


def find_nao_proto_url_from_worlds(nao_roots: Sequence[Path]) -> str | None:
    for root in nao_roots:
        worlds_dir = root / "worlds"
        if not worlds_dir.exists() or not worlds_dir.is_dir():
            continue
        for world_file in sorted(worlds_dir.glob("*.wbt")):
            try:
                text = world_file.read_text(encoding="utf-8")
            except Exception:
                continue
            for ref in EXTERNPROTO_RE.findall(text):
                if "projects/robots/softbank/nao/protos/Nao.proto" in ref and is_http_url(ref):
                    return ref
    return None


def find_nao_proto_source(explicit: str | None) -> str | None:
    explicit_value = (explicit or "").strip()
    if explicit_value:
        if is_http_url(explicit_value):
            return explicit_value
        explicit_path = Path(explicit_value).expanduser()
        if explicit_path.is_file():
            return str(explicit_path.resolve())
        local_from_explicit = find_nao_proto(explicit_value)
        if local_from_explicit is not None:
            return str(local_from_explicit)

    local = find_nao_proto(None)
    if local is not None:
        return str(local)

    world_roots: List[Path] = []
    webots_home = os.environ.get("WEBOTS_HOME")
    if webots_home:
        world_roots.append(Path(webots_home).expanduser() / "projects" / "robots" / "softbank" / "nao")

    webots_bin_env = os.environ.get("WEBOTS_EXECUTABLE") or os.environ.get("WEBOTS_BINARY")
    webots_bin = webots_bin_env or shutil.which("webots")
    if webots_bin:
        webots_bin_path = Path(webots_bin).expanduser()
        for root in {
            webots_bin_path.parent,
            webots_bin_path.parent.parent,
            webots_bin_path.resolve().parent,
            webots_bin_path.resolve().parent.parent,
        }:
            world_roots.append(root / "projects" / "robots" / "softbank" / "nao")

    world_roots.extend(
        [
            Path("/usr/local/webots/projects/robots/softbank/nao"),
            Path("/opt/webots/projects/robots/softbank/nao"),
            Path("/usr/share/webots/projects/robots/softbank/nao"),
            Path("/snap/webots/current/usr/share/webots/projects/robots/softbank/nao"),
        ]
    )

    seen: set[str] = set()
    unique_roots: List[Path] = []
    for root in world_roots:
        key = str(root.expanduser())
        if key in seen:
            continue
        seen.add(key)
        unique_roots.append(root)

    return find_nao_proto_url_from_worlds(unique_roots)


def synthetic_names(prefix: str, count: int, width: int) -> List[str]:
    return [f"{prefix}_{i:0{width}d}" for i in range(count)]


def default_expected_sensory(camera_retina_width: int, camera_retina_height: int) -> int:
    # NAO base sensors excluding both cameras = 58.
    # Two cameras, per-pixel ON/OFF event channels: 2 * (W * H * 2) = 4 * W * H.
    return 58 + 4 * camera_retina_width * camera_retina_height


def resize_channels(
    channels: Sequence[str],
    expected: int,
    pad_prefix: str,
    width: int = 2,
) -> Tuple[List[str], str]:
    cleaned = dedupe_keep_order([c.strip() for c in channels if isinstance(c, str) and c.strip()])
    if expected <= 0:
        return [], "empty_target"
    if len(cleaned) >= expected:
        mode = "exact" if len(cleaned) == expected else "trimmed"
        return cleaned[:expected], mode

    out = list(cleaned)
    seen = set(out)
    pad_idx = 0
    while len(out) < expected:
        candidate = f"{pad_prefix}_pad_{pad_idx:0{width}d}"
        pad_idx += 1
        if candidate in seen:
            continue
        seen.add(candidate)
        out.append(candidate)
    mode = "padded" if cleaned else "synthetic"
    return out, mode


def remap_index(index: int, src_size: int, dst_size: int) -> int:
    if dst_size <= 1:
        return 0
    if src_size <= 1:
        return dst_size // 2
    return int(round(index * (dst_size - 1) / float(src_size - 1)))


def aarnn_laminar_io_layers(hidden_layer_count: int) -> Tuple[int, int]:
    """Map hidden-layer count to canonical AARNN laminar IO layers."""
    if hidden_layer_count <= 0:
        return 0, 0
    sensory_target_layer = 1 if hidden_layer_count > 1 else 0  # L4
    if hidden_layer_count > 4:
        output_source_layer = 4  # L5
    else:
        output_source_layer = hidden_layer_count - 1
    return sensory_target_layer, output_source_layer


def classify_sensor_channel(name: str, index: int, total: int) -> str:
    n = name.lower()
    if any(k in n for k in ("camera", "image", "vision", "light")):
        return "vision"
    if any(k in n for k in ("gyro", "accel", "imu", "inertial")):
        return "vestibular"
    if any(k in n for k in ("sonar", "distance", "range", "ultra")):
        return "proximity"
    if any(k in n for k in ("touch", "fsr", "bumper", "force")):
        return "touch"
    if any(k in n for k in ("position", "joint", "angle", "encoder")):
        return "proprioception"

    # Count-based fallback for synthetic or unknown labels.
    if index < min(6, total):
        return "vestibular"
    if index < min(14, total):
        return "proximity_touch"
    return "proprioception"


def classify_actuator_channel(name: str, index: int, total: int) -> str:
    n = name.lower()
    if any(k in n for k in ("head", "yaw", "pitch")):
        return "head_gaze"
    if any(k in n for k in ("shoulder", "elbow", "wrist", "arm", "hand", "finger")):
        return "manipulation"
    if any(k in n for k in ("hip", "knee", "ankle", "leg", "foot")):
        return "locomotion"

    # Count-based fallback for synthetic or unknown labels.
    if index < min(2, total):
        return "head_gaze"
    if index < min(16, total):
        return "manipulation"
    return "locomotion"


def build_role_maps(
    sensory_names: Sequence[str], output_names: Sequence[str]
) -> Tuple[Dict[str, str], Dict[str, str], Dict[str, List[int]], Dict[str, List[int]]]:
    sensor_roles: Dict[str, str] = {}
    output_roles: Dict[str, str] = {}
    sensory_groups: Dict[str, List[int]] = defaultdict(list)
    output_groups: Dict[str, List[int]] = defaultdict(list)

    for i, name in enumerate(sensory_names):
        role = classify_sensor_channel(name, i, len(sensory_names))
        sensor_roles[name] = role
        sensory_groups[role].append(i)

    for i, name in enumerate(output_names):
        role = classify_actuator_channel(name, i, len(output_names))
        output_roles[name] = role
        output_groups[role].append(i)

    return sensor_roles, output_roles, dict(sensory_groups), dict(output_groups)


def modality_band(modality: str, layer_size: int) -> Tuple[int, int]:
    """Return [start,end) sub-band for modality-specific projections."""
    thirds = max(1, layer_size // 3)
    if modality in {"vision", "vestibular", "proprioception", "proximity", "touch", "proximity_touch"}:
        # Sensor-rich modalities bias early-middle hidden bands.
        if modality in {"vision", "proximity", "touch", "proximity_touch"}:
            return (0, min(layer_size, thirds))
        if modality in {"vestibular"}:
            return (thirds // 2, min(layer_size, thirds + thirds // 2))
        return (thirds, min(layer_size, 2 * thirds))

    if modality in {"head_gaze"}:
        return (0, min(layer_size, thirds))
    if modality in {"manipulation"}:
        return (thirds, min(layer_size, 2 * thirds))
    if modality in {"locomotion"}:
        return (min(layer_size, 2 * thirds), layer_size)
    return (0, layer_size)


def make_matrix(rows: int, cols: int, fill: float = 0.0) -> List[List[float]]:
    return [[fill for _ in range(cols)] for _ in range(rows)]


def make_u32_matrix(rows: int, cols: int) -> List[List[int]]:
    return [[0 for _ in range(cols)] for _ in range(rows)]


def flatten_row_major(mat: List[List[float]]) -> List[float]:
    out: List[float] = []
    for row in mat:
        out.extend(row)
    return out


def flatten_row_major_u32(mat: List[List[int]]) -> List[int]:
    out: List[int] = []
    for row in mat:
        out.extend(row)
    return out


def add_weight(
    w: List[List[float]],
    p: List[List[int]],
    row: int,
    col: int,
    weight: float,
    presence: int = 1,
) -> None:
    w[row][col] += float(weight)
    p[row][col] += int(presence)


NAO_REGION_ORDER: Tuple[str, ...] = (
    "left_cortex",
    "right_cortex",
    "thalamus",
    "hippocampus_left",
    "hippocampus_right",
    "cerebellum",
    "brainstem",
)

NAO_TOPO_REGIONS: Dict[str, dict] = {
    "left_cortex": {"shape": "ellipsoid", "center": (-0.46, 0.14, 0.18), "radii": (0.34, 0.42, 0.24)},
    "right_cortex": {"shape": "ellipsoid", "center": (0.46, 0.14, 0.18), "radii": (0.34, 0.42, 0.24)},
    "thalamus": {"shape": "ellipsoid", "center": (0.0, -0.03, 0.08), "radii": (0.14, 0.11, 0.10)},
    "hippocampus_left": {"shape": "ellipsoid", "center": (-0.18, -0.30, 0.03), "radii": (0.15, 0.09, 0.08)},
    "hippocampus_right": {"shape": "ellipsoid", "center": (0.18, -0.30, 0.03), "radii": (0.15, 0.09, 0.08)},
    "cerebellum": {"shape": "ellipsoid", "center": (0.0, -0.63, -0.08), "radii": (0.34, 0.20, 0.16)},
    "brainstem": {
        "shape": "tube",
        "line_from": (0.0, -0.22, -0.02),
        "line_to": (0.0, -0.95, -0.24),
        "radius": 0.08,
    },
}


def stable_u01(node_key: str, salt: str) -> float:
    digest = hashlib.blake2b(f"{node_key}|{salt}".encode("utf-8"), digest_size=8).digest()
    return int.from_bytes(digest, "big") / float((1 << 64) - 1)


def stable_gauss(node_key: str, salt: str) -> float:
    u1 = max(1e-12, stable_u01(node_key, f"{salt}:u1"))
    u2 = stable_u01(node_key, f"{salt}:u2")
    return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def pick_weighted_cluster(
    node_key: str,
    salt: str,
    clusters: Sequence[Tuple[Tuple[float, float, float], Tuple[float, float, float], float]],
) -> Tuple[Tuple[float, float, float], Tuple[float, float, float]]:
    if not clusters:
        return (0.0, 0.0, 0.0), (0.25, 0.25, 0.25)
    total = sum(max(0.0, w) for _, _, w in clusters)
    if total <= 0.0:
        return clusters[0][0], clusters[0][1]
    t = stable_u01(node_key, f"{salt}:cluster") * total
    acc = 0.0
    for center_n, spread_n, weight in clusters:
        acc += max(0.0, weight)
        if t <= acc:
            return center_n, spread_n
    return clusters[-1][0], clusters[-1][1]


def sample_clustered_ellipsoid(
    center: Tuple[float, float, float],
    radii: Tuple[float, float, float],
    node_key: str,
    salt: str,
    clusters: Sequence[Tuple[Tuple[float, float, float], Tuple[float, float, float], float]],
    shell_bias: float = 0.0,
) -> Tuple[float, float, float]:
    (cx, cy, cz), (sx, sy, sz) = pick_weighted_cluster(node_key, salt, list(clusters))
    nx = cx + stable_gauss(node_key, f"{salt}:x") * sx
    ny = cy + stable_gauss(node_key, f"{salt}:y") * sy
    nz = cz + stable_gauss(node_key, f"{salt}:z") * sz
    norm = math.sqrt(nx * nx + ny * ny + nz * nz)
    if norm > 1e-6:
        # Positive shell bias pushes samples toward ellipsoid surface (cortical sheet effect).
        radial = ((norm - 0.0) / max(1e-6, 1.0 - 0.0)) ** (1.0 - shell_bias)
        target = clamp(0.55 + 0.40 * radial, 0.15, 1.0)
        nx = nx / norm * target
        ny = ny / norm * target
        nz = nz / norm * target
    norm_sq = nx * nx + ny * ny + nz * nz
    if norm_sq > 1.0:
        scale = 0.98 / math.sqrt(norm_sq)
        nx *= scale
        ny *= scale
        nz *= scale
    return (
        center[0] + radii[0] * nx,
        center[1] + radii[1] * ny,
        center[2] + radii[2] * nz,
    )


def pick_weighted(node_key: str, choices: Sequence[Tuple[str, float]]) -> str:
    total = 0.0
    for _, w in choices:
        total += max(0.0, float(w))
    if total <= 0.0:
        return choices[0][0]
    t = stable_u01(node_key, "choice") * total
    acc = 0.0
    for name, w in choices:
        acc += max(0.0, float(w))
        if t <= acc:
            return name
    return choices[-1][0]


def sample_point_in_ellipsoid(
    center: Tuple[float, float, float],
    radii: Tuple[float, float, float],
    node_key: str,
    salt: str,
) -> Tuple[float, float, float]:
    u = stable_u01(node_key, f"{salt}:u")
    v = stable_u01(node_key, f"{salt}:v")
    w = stable_u01(node_key, f"{salt}:w")
    theta = 2.0 * math.pi * u
    cos_phi = max(-1.0, min(1.0, 2.0 * v - 1.0))
    phi = math.acos(cos_phi)
    rr = w ** (1.0 / 3.0)
    sin_phi = math.sin(phi)
    dx = rr * sin_phi * math.cos(theta)
    dy = rr * sin_phi * math.sin(theta)
    dz = rr * math.cos(phi)
    return (
        center[0] + radii[0] * dx,
        center[1] + radii[1] * dy,
        center[2] + radii[2] * dz,
    )


def sample_point_in_tube(
    line_from: Tuple[float, float, float],
    line_to: Tuple[float, float, float],
    radius: float,
    node_key: str,
    salt: str,
    t_override: float | None = None,
) -> Tuple[float, float, float]:
    t = t_override if t_override is not None else stable_u01(node_key, f"{salt}:t")
    angle = 2.0 * math.pi * stable_u01(node_key, f"{salt}:a")
    rr = math.sqrt(stable_u01(node_key, f"{salt}:r")) * radius
    px = line_from[0] + (line_to[0] - line_from[0]) * t
    py = line_from[1] + (line_to[1] - line_from[1]) * t
    pz = line_from[2] + (line_to[2] - line_from[2]) * t
    return (px + rr * math.cos(angle), py, pz + rr * math.sin(angle))


def infer_side_from_label(label: str, node_key: str) -> str:
    lower = label.lower()
    if "left" in lower:
        return "left"
    if "right" in lower:
        return "right"

    left_prefixes = (
        "lshoulder",
        "lelbow",
        "lwrist",
        "lhand",
        "lhip",
        "lknee",
        "lankle",
        "lfoot",
        "larm",
        "lfsr",
        "lbumper",
    )
    right_prefixes = (
        "rshoulder",
        "relbow",
        "rwrist",
        "rhand",
        "rhip",
        "rknee",
        "rankle",
        "rfoot",
        "rarm",
        "rfsr",
        "rbumper",
    )
    compact = re.sub(r"[^a-z0-9]", "", lower)
    if compact.startswith(left_prefixes):
        return "left"
    if compact.startswith(right_prefixes):
        return "right"
    return "left" if stable_u01(node_key, "side") < 0.5 else "right"


def hemisphere_regions(side: str) -> Tuple[str, str]:
    if side == "right":
        return "right_cortex", "hippocampus_right"
    return "left_cortex", "hippocampus_left"


def hidden_region_for(layer_frac: float, band_frac: float, node_key: str) -> str:
    if band_frac < 0.47:
        side = "left"
    elif band_frac > 0.53:
        side = "right"
    else:
        side = "left" if stable_u01(node_key, "band_side") < 0.5 else "right"
    cortex_hemi, hippocampus_hemi = hemisphere_regions(side)
    opposite = "right_cortex" if cortex_hemi == "left_cortex" else "left_cortex"

    if layer_frac < 0.18:
        choices = [(cortex_hemi, 0.36), ("thalamus", 0.42), ("brainstem", 0.22)]
    elif layer_frac < 0.55:
        choices = [(cortex_hemi, 0.50), ("thalamus", 0.20), (hippocampus_hemi, 0.20), (opposite, 0.10)]
    elif layer_frac < 0.82:
        choices = [(cortex_hemi, 0.42), (hippocampus_hemi, 0.20), ("thalamus", 0.16), ("cerebellum", 0.22)]
    else:
        choices = [("cerebellum", 0.38), ("brainstem", 0.32), (cortex_hemi, 0.20), ("thalamus", 0.10)]
    return pick_weighted(node_key, choices)


def role_ap_bias(role: str) -> float | None:
    if role == "head_gaze":
        return 0.18
    if role == "manipulation":
        return 0.45
    if role == "locomotion":
        return 0.78
    if role == "vestibular":
        return 0.22
    if role in {"proprioception", "proximity_touch"}:
        return 0.58
    return None


def hidden_type_for_region(region_name: str, node_key: str) -> str:
    u = stable_u01(node_key, f"type:{region_name}")
    if region_name in {"left_cortex", "right_cortex", "hippocampus_left", "hippocampus_right"}:
        if u < 0.08:
            return "Neuromodulatory"
        if u < 0.16:
            return "Sensory"
        if u < 0.36:
            return "Interneuron"
        return "Pyramidal"
    if region_name == "thalamus":
        if u < 0.12:
            return "Neuromodulatory"
        if u < 0.46:
            return "Sensory"
        if u < 0.68:
            return "Interneuron"
        return "Pyramidal"
    if region_name in {"cerebellum", "brainstem"}:
        if u < 0.10:
            return "Neuromodulatory"
        if u < 0.28:
            return "Interneuron"
        if u < 0.38:
            return "Sensory"
        if u < 0.84:
            return "Motor"
        return "Pyramidal"
    return "Interneuron"


def sample_nao_region_position(
    region_name: str,
    node_key: str,
    salt: str,
    layer_frac: float,
    ap_bias: float | None = None,
) -> Tuple[float, float, float]:
    region = NAO_TOPO_REGIONS[region_name]
    if region["shape"] == "ellipsoid":
        center = region["center"]
        radii = region["radii"]
        if region_name in {"left_cortex", "right_cortex"}:
            clusters = [
                ((-0.46, 0.24, 0.28), (0.20, 0.16, 0.14), 0.25),
                ((-0.10, -0.08, 0.36), (0.18, 0.16, 0.14), 0.23),
                ((0.24, 0.10, 0.08), (0.18, 0.16, 0.14), 0.22),
                ((0.44, -0.20, -0.16), (0.16, 0.14, 0.12), 0.15),
                ((0.08, 0.00, -0.30), (0.18, 0.16, 0.12), 0.15),
            ]
            x, y, z = sample_clustered_ellipsoid(center, radii, node_key, salt, clusters, shell_bias=0.55)
            # Folded cortical sheet effect; deterministic gyri-like corrugation.
            phase = 2.0 * math.pi * stable_u01(node_key, f"{salt}:fold_phase")
            y += 0.030 * math.sin((x - center[0]) * 12.0 + (z - center[2]) * 9.0 + phase)
            z += 0.018 * math.sin((x - center[0]) * 8.0 + phase * 0.7)
            y += (0.5 - layer_frac) * 0.12
            z += (0.5 - layer_frac) * 0.04
        elif region_name == "thalamus":
            clusters = [
                ((-0.28, 0.10, 0.15), (0.16, 0.14, 0.14), 0.28),
                ((0.28, 0.10, 0.15), (0.16, 0.14, 0.14), 0.28),
                ((0.00, -0.08, 0.02), (0.18, 0.16, 0.16), 0.24),
                ((0.00, 0.00, -0.22), (0.16, 0.14, 0.16), 0.20),
            ]
            x, y, z = sample_clustered_ellipsoid(center, radii, node_key, salt, clusters, shell_bias=0.10)
        elif region_name in {"hippocampus_left", "hippocampus_right"}:
            clusters = [
                ((-0.44, 0.10, 0.10), (0.16, 0.14, 0.12), 0.24),
                ((-0.16, -0.08, 0.24), (0.16, 0.14, 0.12), 0.26),
                ((0.16, -0.22, 0.00), (0.16, 0.14, 0.12), 0.26),
                ((0.42, -0.04, -0.20), (0.16, 0.14, 0.12), 0.24),
            ]
            x, y, z = sample_clustered_ellipsoid(center, radii, node_key, salt, clusters, shell_bias=0.30)
            y += 0.020 * math.sin((x - center[0]) * 16.0 + stable_u01(node_key, f"{salt}:hp") * 4.0)
        elif region_name == "cerebellum":
            clusters = [
                ((-0.48, 0.12, 0.10), (0.16, 0.14, 0.12), 0.28),
                ((0.48, 0.12, 0.10), (0.16, 0.14, 0.12), 0.28),
                ((0.00, -0.10, 0.22), (0.18, 0.16, 0.12), 0.22),
                ((0.00, -0.20, -0.16), (0.18, 0.16, 0.12), 0.22),
            ]
            x, y, z = sample_clustered_ellipsoid(center, radii, node_key, salt, clusters, shell_bias=0.38)
            z += 0.016 * math.sin((x - center[0]) * 10.0 + stable_u01(node_key, f"{salt}:cb") * 5.0)
        else:
            x, y, z = sample_point_in_ellipsoid(center, radii, node_key, salt)
    else:
        # Midbrain/pons/medulla-like segmented density instead of uniform tube fill.
        anchors = [0.10, 0.34, 0.58, 0.82]
        if ap_bias is None:
            t_center = stable_u01(node_key, f"{salt}:ap")
        else:
            t_center = ap_bias
        nearest = sorted(anchors, key=lambda a: abs(a - t_center))[:2]
        anchor = nearest[int(stable_u01(node_key, f"{salt}:seg_pick") * len(nearest)) % len(nearest)]
        t = clamp(anchor + stable_gauss(node_key, f"{salt}:seg_jitter") * 0.050 + (t_center - anchor) * 0.45, 0.0, 1.0)
        x, y, z = sample_point_in_tube(
            region["line_from"],
            region["line_to"],
            region["radius"],
            node_key,
            salt,
            t_override=t,
        )
        # Nucleus columns around tract core.
        x += stable_gauss(node_key, f"{salt}:bs_x") * 0.020
        z += stable_gauss(node_key, f"{salt}:bs_z") * 0.020
    return (
        max(-1.35, min(1.35, x)),
        max(-1.35, min(1.35, y)),
        max(-1.35, min(1.35, z)),
    )


def sensory_region_candidates(role: str, side: str) -> List[Tuple[str, float]]:
    cortex_hemi, hippocampus_hemi = hemisphere_regions(side)
    if role == "vision":
        return [(cortex_hemi, 0.52), ("thalamus", 0.28), (hippocampus_hemi, 0.12), ("cerebellum", 0.08)]
    if role == "vestibular":
        return [("brainstem", 0.44), ("thalamus", 0.34), ("cerebellum", 0.22)]
    if role in {"proximity", "touch", "proximity_touch"}:
        return [("thalamus", 0.44), (cortex_hemi, 0.32), ("brainstem", 0.24)]
    if role == "proprioception":
        return [("cerebellum", 0.40), ("brainstem", 0.25), ("thalamus", 0.23), (cortex_hemi, 0.12)]
    return [("thalamus", 0.45), (cortex_hemi, 0.35), ("brainstem", 0.20)]


def output_region_candidates(role: str, side: str) -> List[Tuple[str, float]]:
    cortex_hemi, _ = hemisphere_regions(side)
    if role == "head_gaze":
        return [("brainstem", 0.54), ("thalamus", 0.26), (cortex_hemi, 0.20)]
    if role == "manipulation":
        return [(cortex_hemi, 0.40), ("cerebellum", 0.25), ("brainstem", 0.25), ("thalamus", 0.10)]
    if role == "locomotion":
        return [("brainstem", 0.45), ("cerebellum", 0.40), (cortex_hemi, 0.10), ("thalamus", 0.05)]
    return [("brainstem", 0.40), ("cerebellum", 0.30), (cortex_hemi, 0.20), ("thalamus", 0.10)]


def build_nao_regions() -> List[dict]:
    regions: List[dict] = []
    for region_name in NAO_REGION_ORDER:
        geom = NAO_TOPO_REGIONS[region_name]
        if geom["shape"] == "ellipsoid":
            center = list(geom["center"])
            radii = list(geom["radii"])
            shape = {"shape": "ellipsoid", "center": center, "radii": radii}
        else:
            line_from = list(geom["line_from"])
            line_to = list(geom["line_to"])
            radius = float(geom["radius"])
            center = [
                0.5 * (line_from[0] + line_to[0]),
                0.5 * (line_from[1] + line_to[1]),
                0.5 * (line_from[2] + line_to[2]),
            ]
            radii = [radius, abs(line_to[1] - line_from[1]) * 0.5, radius]
            shape = {"shape": "tube", "line_from": line_from, "line_to": line_to, "radius": radius}

        if region_name in {"left_cortex", "right_cortex"}:
            type_distribution = [["Pyramidal", 0.66], ["Interneuron", 0.20], ["Neuromodulatory", 0.08], ["Sensory", 0.06]]
        elif region_name == "thalamus":
            type_distribution = [["Sensory", 0.34], ["Pyramidal", 0.32], ["Interneuron", 0.22], ["Neuromodulatory", 0.12]]
        elif region_name in {"hippocampus_left", "hippocampus_right"}:
            type_distribution = [["Pyramidal", 0.58], ["Interneuron", 0.24], ["Neuromodulatory", 0.12], ["Sensory", 0.06]]
        elif region_name == "cerebellum":
            type_distribution = [["Motor", 0.50], ["Pyramidal", 0.20], ["Interneuron", 0.20], ["Neuromodulatory", 0.10]]
        else:
            type_distribution = [["Motor", 0.56], ["Interneuron", 0.20], ["Neuromodulatory", 0.14], ["Sensory", 0.10]]

        regions.append(
            {
                "name": region_name,
                "shape": shape,
                "center": center,
                "radii": radii,
                "type_distribution": type_distribution,
            }
        )
    return regions


def build_nao_topology(
    sensory_names: Sequence[str],
    output_names: Sequence[str],
    sensor_roles: Dict[str, str],
    output_roles: Dict[str, str],
    hidden_layers: int,
    hidden_per_layer: int,
) -> Tuple[dict, Dict[str, int]]:
    region_counts: defaultdict[str, int] = defaultdict(int)
    topo_layers: List[List[dict]] = []

    for l_idx in range(hidden_layers):
        layer_frac = l_idx / float(max(1, hidden_layers - 1))
        layer_nodes: List[dict] = []
        for h_idx in range(hidden_per_layer):
            node_id = f"nao_h{l_idx:02d}_{h_idx:04d}"
            band_frac = h_idx / float(max(1, hidden_per_layer - 1))
            region_name = hidden_region_for(layer_frac, band_frac, node_id)
            type_name = hidden_type_for_region(region_name, node_id)
            x, y, z = sample_nao_region_position(region_name, node_id, "hidden", layer_frac)
            layer_nodes.append(
                {
                    "x": x,
                    "y": y,
                    "z": z,
                    "layer": l_idx,
                    "region_name": region_name,
                    "type_name": type_name,
                }
            )
            region_counts[f"hidden:{region_name}"] += 1
        topo_layers.append(layer_nodes)

    sensory_nodes_topo: List[dict] = []
    for s_idx, name in enumerate(sensory_names):
        role = sensor_roles.get(name, "proprioception")
        side = infer_side_from_label(name, f"sens:{s_idx}")
        region_name = pick_weighted(f"sens:{s_idx}:{name}", sensory_region_candidates(role, side))
        x, y, z = sample_nao_region_position(
            region_name,
            f"sens:{s_idx}:{name}",
            "sensory",
            layer_frac=0.0,
            ap_bias=role_ap_bias(role),
        )
        sensory_nodes_topo.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": "Sensory",
            }
        )
        region_counts[f"sensory:{region_name}"] += 1

    output_nodes_topo: List[dict] = []
    for o_idx, name in enumerate(output_names):
        role = output_roles.get(name, "locomotion")
        side = infer_side_from_label(name, f"out:{o_idx}")
        region_name = pick_weighted(f"out:{o_idx}:{name}", output_region_candidates(role, side))
        x, y, z = sample_nao_region_position(
            region_name,
            f"out:{o_idx}:{name}",
            "output",
            layer_frac=1.0,
            ap_bias=role_ap_bias(role),
        )
        output_nodes_topo.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": "Motor",
            }
        )
        region_counts[f"output:{region_name}"] += 1

    topo = {"layers": topo_layers, "sensory_nodes": sensory_nodes_topo, "output_nodes": output_nodes_topo}
    return topo, {k: int(v) for k, v in sorted(region_counts.items())}


def build_weights(
    sensory_count: int,
    output_count: int,
    hidden_layers: int,
    hidden_per_layer: int,
    sensory_groups: Dict[str, List[int]],
    output_groups: Dict[str, List[int]],
    sensory_target_layer: int,
    output_source_layer: int,
) -> Tuple[
    List[List[float]],
    List[List[int]],
    List[List[List[float]]],
    List[List[List[int]]],
    List[List[List[float]]],
    List[List[List[int]]],
    List[List[List[float]]],
    List[List[List[int]]],
    List[List[float]],
    List[List[int]],
]:
    layer_sizes = [hidden_per_layer for _ in range(hidden_layers)]

    in_rows = layer_sizes[sensory_target_layer]
    out_cols = layer_sizes[output_source_layer]

    w_in = make_matrix(in_rows, sensory_count)
    p_in = make_u32_matrix(in_rows, sensory_count)

    w_fwd: List[List[List[float]]] = []
    p_fwd: List[List[List[int]]] = []
    w_bwd: List[List[List[float]]] = []
    p_bwd: List[List[List[int]]] = []
    w_rec: List[List[List[float]]] = []
    p_rec: List[List[List[int]]] = []

    # Sensory -> first hidden layer, modality-biased and topographic.
    in_kernel = [(-9, 0.65), (-5, 0.80), (-2, 0.95), (0, 1.10), (2, 0.95), (5, 0.80), (9, 0.65)]
    sensory_role_for_idx: Dict[int, str] = {}
    for role, idxs in sensory_groups.items():
        for i in idxs:
            sensory_role_for_idx[i] = role

    for s_idx in range(sensory_count):
        role = sensory_role_for_idx.get(s_idx, "proprioception")
        start, end = modality_band(role, in_rows)
        band_len = max(1, end - start)
        center = start + remap_index(s_idx, sensory_count, band_len)
        for offset, base in in_kernel:
            h_idx = start + ((center - start + offset) % band_len)
            wobble = 0.05 * math.sin((s_idx + 1) * (offset + 13))
            add_weight(w_in, p_in, h_idx, s_idx, max(0.05, base + wobble))

        h_global = (s_idx * 7 + 3) % in_rows
        add_weight(w_in, p_in, h_global, s_idx, 0.35)

    # Hidden recurrent + inter-layer forward/backward ladders.
    rec_kernel = [(-13, 0.05), (-5, 0.09), (-1, 0.15), (0, 0.22), (1, 0.15), (5, 0.09), (13, 0.05)]
    fwd_kernel = [(-10, 0.30), (-4, 0.45), (0, 0.60), (4, 0.45), (10, 0.30)]
    bwd_kernel = [(-8, 0.10), (0, 0.16), (8, 0.10)]

    for l in range(hidden_layers):
        rows = layer_sizes[l]
        cols = layer_sizes[l]
        wr = make_matrix(rows, cols)
        pr = make_u32_matrix(rows, cols)
        for pre in range(cols):
            phase = 0.03 * math.sin((pre + 1) * 0.19 + l)
            for delta, base in rec_kernel:
                post = (pre + delta) % rows
                add_weight(wr, pr, post, pre, max(0.02, base + phase))
        w_rec.append(wr)
        p_rec.append(pr)

        if l + 1 < hidden_layers:
            rows_f = layer_sizes[l + 1]
            cols_f = layer_sizes[l]
            wf = make_matrix(rows_f, cols_f)
            pf = make_u32_matrix(rows_f, cols_f)
            wb = make_matrix(cols_f, rows_f)
            pb = make_u32_matrix(cols_f, rows_f)

            for pre in range(cols_f):
                center = remap_index(pre, cols_f, rows_f)
                for delta, base in fwd_kernel:
                    post = (center + delta) % rows_f
                    lift = 0.03 * math.cos((l + 1) * (pre + 3) * 0.01)
                    add_weight(wf, pf, post, pre, max(0.02, base + lift))

            for pre in range(rows_f):
                center = remap_index(pre, rows_f, cols_f)
                for delta, base in bwd_kernel:
                    post = (center + delta) % cols_f
                    lift = 0.02 * math.sin((l + 5) * (pre + 11) * 0.01)
                    add_weight(wb, pb, post, pre, max(0.01, base + lift))

            w_fwd.append(wf)
            p_fwd.append(pf)
            w_bwd.append(wb)
            p_bwd.append(pb)

    # Last hidden -> outputs, modality-biased.
    w_out = make_matrix(output_count, out_cols)
    p_out = make_u32_matrix(output_count, out_cols)

    out_kernel = [(-12, 0.50), (-6, 0.68), (-2, 0.88), (0, 1.05), (2, 0.88), (6, 0.68), (12, 0.50)]
    output_role_for_idx: Dict[int, str] = {}
    for role, idxs in output_groups.items():
        for i in idxs:
            output_role_for_idx[i] = role

    for o_idx in range(output_count):
        role = output_role_for_idx.get(o_idx, "locomotion")
        start, end = modality_band(role, out_cols)
        band_len = max(1, end - start)
        center = start + remap_index(o_idx, output_count, band_len)

        for offset, base in out_kernel:
            pre = start + ((center - start + offset) % band_len)
            wobble = 0.06 * math.cos((o_idx + 1) * (offset + 17) * 0.03)
            add_weight(w_out, p_out, o_idx, pre, max(0.05, base + wobble))

        pre_global = (o_idx * 11 + 5) % out_cols
        add_weight(w_out, p_out, o_idx, pre_global, 0.30)

    return w_in, p_in, w_fwd, p_fwd, w_bwd, p_bwd, w_rec, p_rec, w_out, p_out


def count_nonzero_u32(mat: List[List[int]]) -> int:
    return sum(1 for row in mat for v in row if v > 0)


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate NAO network/config artifacts for Webots runtime.")
    parser.add_argument(
        "--template",
        default="network.json",
        help="Template snapshot used for baseline net defaults (default: network.json)",
    )
    parser.add_argument(
        "--output",
        default="network_nao.json",
        help="Output snapshot JSON path (default: network_nao.json)",
    )
    parser.add_argument(
        "--config-output",
        default="webots_world/configs/config_nao_webots.json",
        help="Output NetworkConfig JSON path (default: webots_world/configs/config_nao_webots.json)",
    )
    parser.add_argument(
        "--nao-proto",
        default="",
        help="Optional path or URL to Nao.proto for channel label extraction",
    )
    parser.add_argument(
        "--expected-sensory",
        type=int,
        default=None,
        help="Expected sensory channel count from NAO mapper (default: 58 + 4*camera-retina-width*camera-retina-height)",
    )
    parser.add_argument(
        "--expected-output",
        type=int,
        default=40,
        help="Expected actuator channel count from NAO mapper (default: 40)",
    )
    parser.add_argument(
        "--camera-retina-width",
        type=int,
        default=160,
        help="Camera event encoder retina width per camera (default: 160 for full NAO camera width)",
    )
    parser.add_argument(
        "--camera-retina-height",
        type=int,
        default=120,
        help="Camera event encoder retina height per camera (default: 120 for full NAO camera height)",
    )
    parser.add_argument(
        "--hidden",
        dest="hidden_per_layer",
        type=int,
        default=96,
        help="(Deprecated alias) hidden neurons per layer",
    )
    parser.add_argument(
        "--hidden-per-layer",
        dest="hidden_per_layer",
        type=int,
        default=96,
        help="Hidden neurons per hidden layer (default: 96)",
    )
    parser.add_argument(
        "--hidden-layers",
        type=int,
        default=6,
        help="Number of hidden layers (default: 6)",
    )
    parser.add_argument(
        "--aarnn-depth",
        type=int,
        default=5,
        help="AARNN biological depth level (clamped to 1..5, default: 5)",
    )
    parser.add_argument(
        "--growth-headroom",
        type=float,
        default=1.8,
        help="Growth budget multiplier over initial neurons (>= 1.0, default: 1.8)",
    )
    args = parser.parse_args()

    template_path = Path(args.template)
    output_path = Path(args.output)
    config_output_path = Path(args.config_output)

    if args.camera_retina_width <= 0 or args.camera_retina_height <= 0:
        raise SystemExit("camera-retina-width and camera-retina-height must be > 0")
    if args.expected_sensory is None:
        args.expected_sensory = default_expected_sensory(
            args.camera_retina_width,
            args.camera_retina_height,
        )
    if args.expected_sensory <= 0 or args.expected_output <= 0:
        raise SystemExit("expected-sensory and expected-output must be > 0")
    if args.hidden_per_layer <= 0:
        raise SystemExit("hidden-per-layer must be > 0")
    if args.hidden_layers <= 0:
        raise SystemExit("hidden-layers must be > 0")
    if args.aarnn_depth <= 0:
        raise SystemExit("aarnn-depth must be > 0")
    if args.growth_headroom < 1.0:
        raise SystemExit("growth-headroom must be >= 1.0")
    if not template_path.exists():
        raise SystemExit(f"Missing template snapshot: {template_path}")

    template = json.loads(template_path.read_text(encoding="utf-8"))
    if not isinstance(template, dict):
        raise SystemExit(f"Template JSON must be an object: {template_path}")

    template_net_raw = template.get("net", template)
    if not isinstance(template_net_raw, dict):
        raise SystemExit(f"Template net config is not an object: {template_path}")
    net = dict(template_net_raw)

    proto_source = find_nao_proto_source(args.nao_proto or None)
    parsed_s: List[str] = []
    parsed_o: List[str] = []
    label_mode = "synthetic_fallback"
    label_adjustments: Dict[str, str] = {"sensory": "synthetic", "output": "synthetic"}
    parse_error = ""

    if proto_source is not None:
        try:
            parsed_s, parsed_o = parse_nao_channels_recursive(
                proto_source,
                camera_retina_width=args.camera_retina_width,
                camera_retina_height=args.camera_retina_height,
            )
        except Exception as exc:
            parse_error = str(exc)

    if parsed_s or parsed_o:
        sensory_names, sensory_adjustment = resize_channels(parsed_s, args.expected_sensory, "nao_s")
        output_names, output_adjustment = resize_channels(parsed_o, args.expected_output, "nao_o")
        label_adjustments = {"sensory": sensory_adjustment, "output": output_adjustment}
        if sensory_adjustment == "exact" and output_adjustment == "exact":
            label_mode = "proto_exact"
        else:
            label_mode = "proto_resized"
    else:
        sensory_names = synthetic_names("nao_s", args.expected_sensory, 2)
        output_names = synthetic_names("nao_o", args.expected_output, 2)
        label_mode = "synthetic_counts"

    sensory_count = len(sensory_names)
    output_count = len(output_names)
    hidden_per_layer = int(args.hidden_per_layer)
    hidden_layers = int(args.hidden_layers)
    sensory_target_layer, output_source_layer = aarnn_laminar_io_layers(hidden_layers)
    requested_depth = int(args.aarnn_depth)
    aarnn_depth = max(1, min(requested_depth, 5))
    hidden_total = hidden_per_layer * hidden_layers

    sensor_roles, output_roles, sensory_groups, output_groups = build_role_maps(
        sensory_names=sensory_names,
        output_names=output_names,
    )

    (
        w_in,
        p_in,
        w_fwd,
        p_fwd,
        w_bwd,
        p_bwd,
        w_rec,
        p_rec,
        w_out,
        p_out,
    ) = build_weights(
        sensory_count=sensory_count,
        output_count=output_count,
        hidden_layers=hidden_layers,
        hidden_per_layer=hidden_per_layer,
        sensory_groups=sensory_groups,
        output_groups=output_groups,
        sensory_target_layer=sensory_target_layer,
        output_source_layer=output_source_layer,
    )

    # Core shape
    net["num_sensory_neurons"] = sensory_count
    net["num_hidden_layers"] = hidden_layers
    net["num_hidden_per_layer_initial"] = hidden_per_layer
    net["num_output_neurons"] = output_count
    net["sensory_target_layer"] = sensory_target_layer
    net["output_source_layer"] = output_source_layer

    # Growth + morphology + bio detail
    net["growth_enabled"] = True
    net["morpho_growth_enabled"] = True
    net["use_morphology"] = True
    net["use_aarnn_delays"] = True
    net["aarnn_layer_depth"] = aarnn_depth
    net["max_layers"] = max(int(net.get("max_layers", 0) or 0), hidden_layers + 4)

    # Biological runtime systems
    net["perceptual_loop_enabled"] = True
    net["world_model_enabled"] = True
    net["sleep_enabled"] = True
    net["theta_rhythm_enabled"] = True
    net["thalamic_gating_enabled"] = True
    net["volume_transmission_enabled"] = True
    net["columnar_enabled"] = True
    net["aarnn_myelination_enabled"] = True
    net["perceptual_prediction_lr"] = 0.06
    net["perceptual_prediction_decay"] = 0.01
    net["perceptual_prediction_threshold"] = 0.45
    net["perceptual_error_gain"] = 3.5
    net["perceptual_feedback_gain"] = 0.35
    net["world_model_dim"] = 12
    net["world_model_decay"] = 0.04
    net["theta_rhythm_hz"] = 6.0
    net["theta_rhythm_duty"] = 0.25
    net["theta_rhythm_drive"] = 9.0
    net["theta_rhythm_phase_jitter"] = 0.02
    net["thalamic_gate_hz"] = 8.0
    net["thalamic_gate_duty"] = 0.35
    net["thalamic_gate_floor"] = 0.12

    # Connectivity + dynamics tuning (deterministic initialization + adaptive runtime)
    net["p_in"] = 0.28
    net["p_hidden"] = 0.16
    net["p_out"] = 0.24
    net["clumping_design"] = "HumanBrain"
    net["brain_regions"] = build_nao_regions()
    net["neuron_types"] = [
        {"name": "Pyramidal", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.05}},
        {"name": "Sensory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.0}},
        {"name": "Interneuron", "bio_params": {"izh_preset": "FS", "synaptic_gain": 0.95}},
        {"name": "Motor", "bio_params": {"izh_preset": "CH", "synaptic_gain": 1.15}},
        {
            "name": "Neuromodulatory",
            "bio_params": {"izh_preset": "IB", "neuromodulation_enabled": True, "synaptic_gain": 0.9},
        },
    ]

    # Neuromod + adaptation mapped to robot-driven signals
    net["aarnn_neuromod_dopamine_signal"] = "perceptual_error"
    net["aarnn_neuromod_ach_signal"] = "sensory_spikes"
    net["aarnn_neuromod_serotonin_signal"] = "stability"
    net["aarnn_neuromod_error_gain"] = 0.25
    net["aarnn_neuromod_activity_gain"] = 0.20
    net["aarnn_neuromod_stability_gain"] = 0.15
    net["aarnn_gap_junction_strength"] = 0.02
    net["aarnn_gap_junction_radius"] = 0.20
    net["aarnn_gap_junction_inhibitory_only"] = True
    net["aarnn_nmda_voltage_sensitivity"] = 0.06
    net["volume_transmission_radius"] = 0.35
    net["volume_transmission_strength"] = 0.12
    net["aarnn_triplet_ltp_gain"] = 0.12
    net["aarnn_triplet_ltd_gain"] = 0.06
    net["aarnn_synaptic_scaling_strength"] = 0.03
    net["aarnn_synaptic_scaling_target"] = 1.0
    net["aarnn_distance_attenuation_per_unit"] = 0.08
    net["aarnn_import_topology_rewire_enabled"] = True
    net["aarnn_import_topology_rewire_keep_fraction"] = 0.82
    net["aarnn_import_topology_rewire_region_bias"] = 0.20
    net["aarnn_release_prob_heterogeneity"] = 0.08
    net["aarnn_dale_strictness"] = 0.72
    net["aarnn_inhibitory_fraction"] = 0.22
    net["aarnn_myelination_rate"] = 0.0015
    net["aarnn_demyelination_rate"] = 0.0005
    net["aarnn_myelination_activity_target"] = 0.12
    net["aarnn_myelin_min_conduction_gain"] = 0.9
    net["aarnn_myelin_max_conduction_gain"] = 1.9
    net["aarnn_myelin_initial"] = 0.30

    # Keep sleep enabled but avoid rapid control dropout in short runs.
    net["sleep_cycle_ms"] = 300000.0
    net["sleep_duration_ms"] = 1200.0
    net["sleep_dream_replay_prob"] = 0.7
    net["sleep_dream_threshold"] = 0.55
    net["sleep_consolidation_gain"] = 0.35

    # Robot scale / growth budget
    initial_total_neurons = hidden_total + output_count + sensory_count
    requested_budget = int(math.ceil(initial_total_neurons * float(args.growth_headroom)))
    net["max_total_neurons"] = max(
        initial_total_neurons,
        requested_budget,
        int(net.get("max_total_neurons", 0) or 0),
    )

    hidden_nodes: List[str] = []
    hidden_layer_sizes: List[int] = []
    for l in range(hidden_layers):
        hidden_layer_sizes.append(hidden_per_layer)
        hidden_nodes.extend(f"nao_h{l:02d}_{i:04d}" for i in range(hidden_per_layer))
    in_rows = hidden_layer_sizes[sensory_target_layer]
    out_cols = hidden_layer_sizes[output_source_layer]
    topo, topo_region_counts = build_nao_topology(
        sensory_names=sensory_names,
        output_names=output_names,
        sensor_roles=sensor_roles,
        output_roles=output_roles,
        hidden_layers=hidden_layers,
        hidden_per_layer=hidden_per_layer,
    )

    snapshot = {
        "net": net,
        "topo": topo,
        "w_in": {"rows": in_rows, "cols": sensory_count, "data": flatten_row_major(w_in)},
        "w_hh_fwd": [
            {"rows": len(mat), "cols": (len(mat[0]) if mat else 0), "data": flatten_row_major(mat)}
            for mat in w_fwd
        ],
        "w_hh_bwd": [
            {"rows": len(mat), "cols": (len(mat[0]) if mat else 0), "data": flatten_row_major(mat)}
            for mat in w_bwd
        ],
        "w_hh_rec": [
            {"rows": len(mat), "cols": (len(mat[0]) if mat else 0), "data": flatten_row_major(mat)}
            for mat in w_rec
        ],
        "w_out": {"rows": output_count, "cols": out_cols, "data": flatten_row_major(w_out)},
        "p_in": {"rows": in_rows, "cols": sensory_count, "data": flatten_row_major_u32(p_in)},
        "p_fwd": [
            {"rows": len(mat), "cols": (len(mat[0]) if mat else 0), "data": flatten_row_major_u32(mat)}
            for mat in p_fwd
        ],
        "p_bwd": [
            {"rows": len(mat), "cols": (len(mat[0]) if mat else 0), "data": flatten_row_major_u32(mat)}
            for mat in p_bwd
        ],
        "p_rec": [
            {"rows": len(mat), "cols": (len(mat[0]) if mat else 0), "data": flatten_row_major_u32(mat)}
            for mat in p_rec
        ],
        "p_out": {"rows": output_count, "cols": out_cols, "data": flatten_row_major_u32(p_out)},
        "layer_range": None,
        "connectome_labels": {
            "dataset": "NAO reverse engineered",
            "sensory_nodes": sensory_names,
            "hidden_nodes": hidden_nodes,
            "hidden_layer_sizes": hidden_layer_sizes,
            "laminar_mapping": {
                "hidden_layer_count": hidden_layers,
                "sensory_target_layer": sensory_target_layer,
                "output_source_layer": output_source_layer,
                "sensory_layer_name": "L4" if sensory_target_layer == 1 else "fallback",
                "output_layer_name": "L5" if output_source_layer == 4 else "fallback",
            },
            "output_nodes": output_names,
            "sensor_role_map": sensor_roles,
            "output_role_map": output_roles,
            "sensory_groups": sensory_groups,
            "output_groups": output_groups,
            "label_mode": label_mode,
            "label_adjustments": label_adjustments,
            "nao_proto_path": str(proto_source) if proto_source is not None else None,
            "parsed_counts": {"sensory": len(parsed_s), "output": len(parsed_o)},
            "expected_counts": {"sensory": args.expected_sensory, "output": args.expected_output},
            "camera_event_encoder": {
                "retina_width": int(args.camera_retina_width),
                "retina_height": int(args.camera_retina_height),
                "channels_per_camera": int(2 * args.camera_retina_width * args.camera_retina_height),
                "camera_count_assumed_for_default_expected_sensory": 2,
            },
            "parse_error": parse_error or None,
            "generator": "build_nao_network_json.py",
            "growth_headroom": float(args.growth_headroom),
            "aarnn_depth_requested": requested_depth,
            "aarnn_depth_applied": aarnn_depth,
            "max_total_neurons": int(net["max_total_neurons"]),
            "topology_projection": {
                "mode": "human_brain_biomimetic",
                "topo_region_counts": topo_region_counts,
                "uses_role_guided_regioning": True,
            },
            "bio_profile": {
                "species": "homo_sapiens_approx",
                "dataset": "NAO reverse engineered",
                "growth3d": True,
                "morphology": True,
                "aarnn_layer_depth": int(net["aarnn_layer_depth"]),
                "perceptual_loop": bool(net["perceptual_loop_enabled"]),
                "world_model": bool(net["world_model_enabled"]),
                "sleep": bool(net["sleep_enabled"]),
                "theta": bool(net["theta_rhythm_enabled"]),
                "thalamic_gating": bool(net["thalamic_gating_enabled"]),
                "volume_transmission": bool(net["volume_transmission_enabled"]),
                "myelination": bool(net["aarnn_myelination_enabled"]),
                "rewire_on_import": bool(net.get("aarnn_import_topology_rewire_enabled", False)),
                "rewire_keep_fraction": float(net.get("aarnn_import_topology_rewire_keep_fraction", 1.0) or 1.0),
            },
        },
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    config_output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8")
    config_output_path.write_text(json.dumps(net, indent=2) + "\n", encoding="utf-8")

    in_edges = count_nonzero_u32(p_in)
    rec_edges = sum(count_nonzero_u32(m) for m in p_rec)
    fwd_edges = sum(count_nonzero_u32(m) for m in p_fwd)
    bwd_edges = sum(count_nonzero_u32(m) for m in p_bwd)
    out_edges = count_nonzero_u32(p_out)
    print(
        f"Wrote {output_path} and {config_output_path} | "
        f"S={sensory_count} H={hidden_layers}x{hidden_per_layer} O={output_count} "
        f"depth={aarnn_depth} "
        f"headroom={float(args.growth_headroom):.2f} "
        f"(edges in/fwd/bwd/rec/out={in_edges}/{fwd_edges}/{bwd_edges}/{rec_edges}/{out_edges}) "
        f"labels={label_mode}"
    )
    if proto_source is not None:
        print(f"NAO proto source: {proto_source} (parsed S={len(parsed_s)} O={len(parsed_o)})")
    elif args.nao_proto:
        print(f"NAO proto source not found at requested value: {args.nao_proto}")


if __name__ == "__main__":
    main()
