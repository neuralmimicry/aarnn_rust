#!/usr/bin/env python3
"""
Generate a mixed Webots world containing multiple robot types, each bound to a
distinct brain ID via controllerArgs (NM_BRAINS=<brain_id>).
"""

from __future__ import annotations

import argparse
import math
import os
from pathlib import Path
from typing import List, Tuple


NAO_EXTERNPROTO = (
    "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/robots/"
    "softbank/nao/protos/Nao.proto"
)

DROS_SENSORS_REGEX = r"^dros_s_[0-9]{2}_.*$"
DROS_ACTUATORS_REGEX = r"^dros_o_[0-9]{3}_.*$"
CELEGANS_SENSORS_REGEX = r"^celegans_s_[0-9]{2}_.*$"
CELEGANS_ACTUATORS_REGEX = r"^celegans_o_[0-9]{3}_.*$"
HEXAPOD_SENSORS_REGEX = r"^hex_s_[0-9]{2}_.*$"
HEXAPOD_ACTUATORS_REGEX = r"^hex_o_[0-9]{3}_.*$"

# Approximate per-robot scene footprint used for startup camera framing.
# These values intentionally include the nearby stimulus props placed around
# each robot, not only the body mesh.
ROBOT_VIEW_RADIUS = {
    "celegans": 0.55,
    "drosophila": 0.80,
    "hexapod": 1.25,
    "nao": 2.80,
}

# Clear region to preserve around each robot before adding room furniture.
ROBOT_ZONE_CLEAR_RADIUS = {
    "celegans": 0.78,
    "drosophila": 0.92,
    "hexapod": 1.45,
    "nao": 1.95,
}


def parse_csv(value: str) -> List[str]:
    out: List[str] = []
    for item in value.split(","):
        token = item.strip()
        if token:
            out.append(token)
    return out


def rel_proto_ref(world_path: Path, proto_path: Path) -> str:
    return os.path.relpath(proto_path.resolve(), world_path.parent.resolve()).replace("\\", "/")


def centered_grid_offsets(
    count: int,
    spacing_x: float,
    spacing_z: float,
    *,
    max_cols: int,
) -> List[Tuple[float, float]]:
    """Return centered local offsets for repeated robots inside one demo zone."""
    if count <= 0:
        return []

    cols = min(max_cols, max(1, count))
    rows = int(math.ceil(count / float(cols)))
    x_offset = (cols - 1) * spacing_x * 0.5
    z_offset = (rows - 1) * spacing_z * 0.5
    out: List[Tuple[float, float]] = []
    for i in range(count):
        row = i // cols
        col = i % cols
        out.append((col * spacing_x - x_offset, row * spacing_z - z_offset))
    return out


def kind_zone_offsets(kind: str, count: int) -> List[Tuple[float, float]]:
    """
    Return local spawn offsets for one robot class.

    The spacings reflect the demo footprint of each platform: celegans can be
    packed tightly on a lab pad, drosophila needs more room for perch/fruit props,
    and NAO requires the widest articulated service bay.
    """
    if kind == "celegans":
        return centered_grid_offsets(count, 1.05, 0.92, max_cols=3 if count >= 5 else 2)
    if kind == "drosophila":
        return centered_grid_offsets(count, 1.28, 1.10, max_cols=3 if count >= 5 else 2)
    if kind == "hexapod":
        return centered_grid_offsets(count, 2.05, 1.75, max_cols=2)
    return centered_grid_offsets(count, 2.65, 2.35, max_cols=2)


def kind_zone_extent(kind: str, offsets: List[Tuple[float, float]]) -> Tuple[float, float]:
    """Return half extents for a robot zone including the class keep-out radius."""
    clearance = ROBOT_ZONE_CLEAR_RADIUS.get(kind, 0.9)
    if not offsets:
        return clearance, clearance
    return (
        max(abs(x) for x, _ in offsets) + clearance,
        max(abs(z) for _, z in offsets) + clearance,
    )


def mixed_zone_centers(zone_extents: dict[str, Tuple[float, float]]) -> dict[str, Tuple[float, float]]:
    """
    Place robot classes into deliberate demo zones instead of a generic square grid.

    Layout policy:
    - celegans occupies the left-front observation pad.
    - drosophila occupies the right-front exploration pad.
    - NAO occupies the rear-center presentation bay.
    - single-class scenes remain centered.
    """
    if not zone_extents:
        return {}

    kinds = set(zone_extents)
    if len(kinds) == 1:
        sole_kind = next(iter(kinds))
        return {sole_kind: (0.0, 0.0)}

    has_celegans = "celegans" in kinds
    has_drosophila = "drosophila" in kinds
    has_nao = "nao" in kinds
    has_hexapod = "hexapod" in kinds
    centers: dict[str, Tuple[float, float]] = {}

    if has_nao or has_hexapod:
        small_front_depth = max(
            (zone_extents[kind][1] for kind in ("celegans", "drosophila") if kind in zone_extents),
            default=0.0,
        )
        front_z = -(0.55 + small_front_depth)
        if has_nao and has_hexapod:
            dual_gap = 1.35
            centers["hexapod"] = (
                -(dual_gap * 0.5 + zone_extents["hexapod"][0]),
                0.62 + zone_extents["hexapod"][1],
            )
            centers["nao"] = (
                dual_gap * 0.5 + zone_extents["nao"][0],
                0.62 + zone_extents["nao"][1],
            )
        elif has_nao:
            centers["nao"] = (0.0, 0.62 + zone_extents["nao"][1])
        else:
            centers["hexapod"] = (0.0, 0.62 + zone_extents["hexapod"][1])

        if has_celegans and has_drosophila:
            side_gap = 1.15
            centers["celegans"] = (-(side_gap * 0.5 + zone_extents["celegans"][0]), front_z)
            centers["drosophila"] = (side_gap * 0.5 + zone_extents["drosophila"][0], front_z)
        elif has_celegans:
            centers["celegans"] = (-(0.82 + zone_extents["celegans"][0]), front_z)
        elif has_drosophila:
            centers["drosophila"] = (0.82 + zone_extents["drosophila"][0], front_z)

        return centers

    if has_celegans and has_drosophila:
        side_gap = 0.95
        shared_front_z = -0.18
        centers["celegans"] = (-(side_gap * 0.5 + zone_extents["celegans"][0]), shared_front_z)
        centers["drosophila"] = (side_gap * 0.5 + zone_extents["drosophila"][0], shared_front_z)
        return centers

    if has_celegans:
        centers["celegans"] = (0.0, 0.0)
    if has_drosophila:
        centers["drosophila"] = (0.0, 0.0)
    return centers


def layout_positions(
    entries: List[tuple[str, str, str, float, str]],
) -> Tuple[List[Tuple[float, float]], float, float]:
    """Return type-aware robot positions plus keep-out radius and room half size."""
    if not entries:
        return [], 1.6, 4.6

    indices_by_kind: dict[str, List[int]] = {}
    for idx, (_, _, _, _, kind) in enumerate(entries):
        indices_by_kind.setdefault(kind, []).append(idx)

    offsets_by_kind = {
        kind: kind_zone_offsets(kind, len(indices))
        for kind, indices in indices_by_kind.items()
    }
    zone_extents = {
        kind: kind_zone_extent(kind, offsets)
        for kind, offsets in offsets_by_kind.items()
    }
    zone_centers = mixed_zone_centers(zone_extents)

    positions: List[Tuple[float, float]] = [(0.0, 0.0)] * len(entries)
    for kind, indices in indices_by_kind.items():
        center_x, center_z = zone_centers.get(kind, (0.0, 0.0))
        for idx, (local_x, local_z) in zip(indices, offsets_by_kind[kind]):
            positions[idx] = (center_x + local_x, center_z + local_z)

    center_x, center_z, _scene_radius, _span_x, _span_z = scene_metrics(entries, positions)
    keepout_radius = 0.0
    for (_, _, _, _, kind), (x, z) in zip(entries, positions):
        keepout_radius = max(
            keepout_radius,
            math.hypot(x - center_x, z - center_z) + ROBOT_ZONE_CLEAR_RADIUS.get(kind, 0.9),
        )

    has_large = any(kind in {"nao", "hexapod"} for (_, _, _, _, kind) in entries)
    room_margin = 3.2 if has_large else 2.25
    min_half_size = 6.8 if has_large else 4.6
    arena_half_size = max(min_half_size, keepout_radius + room_margin)
    return positions, keepout_radius, arena_half_size


def ecology_basis(
    x: float,
    z: float,
    center_x: float,
    center_z: float,
) -> Tuple[float, float, float, float]:
    """
    Return the local forward/lateral axes for one robot's ecology pocket.

    The forward axis points into the active scene cluster so mixed demos start in
    a deliberate pose: each robot looks toward nearby stimuli rather than toward
    empty wall space. For single-robot scenes we fall back to +X, which matches
    the zero-yaw forward convention already used by the custom bio meshes.
    """
    dx = center_x - x
    dz = center_z - z
    norm = math.hypot(dx, dz)
    if norm <= 1e-6:
        forward_x, forward_z = 1.0, 0.0
    else:
        forward_x, forward_z = dx / norm, dz / norm
    lateral_x, lateral_z = -forward_z, forward_x
    return forward_x, forward_z, lateral_x, lateral_z


def yaw_toward_center(x: float, z: float, center_x: float, center_z: float) -> float:
    """
    Return a spawn heading aligned with the robot's local ecology.

    This keeps the robot's "forward" sensors pointed into the intended stimulus
    corridor. A future analog-array backend could reuse the same heading policy
    while swapping rigid props for field emitters or analog sensor fixtures.
    """
    forward_x, forward_z, _lateral_x, _lateral_z = ecology_basis(x, z, center_x, center_z)
    return math.atan2(forward_z, forward_x)


def oriented_floor_point(
    origin_x: float,
    origin_z: float,
    forward_x: float,
    forward_z: float,
    lateral_x: float,
    lateral_z: float,
    *,
    forward: float,
    lateral: float,
    bound: float,
) -> Tuple[float, float]:
    """
    Resolve a local ecology offset into a clamped world-floor point.

    The generator models every local scene in robot-centric coordinates and only
    converts to Webots world coordinates at the edge. That separation is also
    useful for future FPAA work because the same ecology description could drive
    analog field placement without changing the high-level scene logic.
    """
    return (
        clamp(origin_x + forward_x * forward + lateral_x * lateral, -bound, bound),
        clamp(origin_z + forward_z * forward + lateral_z * lateral, -bound, bound),
    )


def oriented_velocity(
    forward_x: float,
    forward_z: float,
    lateral_x: float,
    lateral_z: float,
    *,
    forward: float,
    lateral: float,
    vertical: float = 0.0,
) -> Tuple[float, float, float]:
    """
    Convert robot-centric motion cues into Webots linear/angular velocity tuples.

    The same helper can later map moving visual/tactile cues onto analog drive
    waveforms or time-varying field sources when rigid-body motion is replaced.
    """
    return (
        forward_x * forward + lateral_x * lateral,
        forward_z * forward + lateral_z * lateral,
        vertical,
    )


def yaw_rotation(yaw: float) -> Tuple[float, float, float, float]:
    """Return a Z-up yaw rotation for props that only need planar alignment."""
    return 0.0, 0.0, 1.0, yaw


def ramp_rotation(
    forward_x: float,
    forward_z: float,
    lateral_x: float,
    lateral_z: float,
    pitch: float,
) -> Tuple[float, float, float, float]:
    """
    Compose floor-plane yaw with a tilt around the ecology's lateral axis.

    Ramps provide inertial stimulation through body pitch and wheel/slither load
    changes. In an analog-array equivalent, that same ecological role would be
    implemented by biasing IMU-like channels or local force gradients instead of
    a literal rigid incline.
    """
    yaw = math.atan2(forward_z, forward_x)
    rotation = matrix_mul(
        axis_angle_to_matrix(lateral_x, lateral_z, 0.0, pitch),
        axis_angle_to_matrix(0.0, 0.0, 1.0, yaw),
    )
    return rotation_matrix_to_axis_angle(rotation)


def floor_patch_box(
    x: float,
    z: float,
    center_height: float,
    *,
    size_x: float,
    size_y: float,
    size_z: float,
    color: Tuple[float, float, float],
    roughness: float = 0.86,
    metalness: float = 0.0,
    transparency: float = 0.0,
    rotation: Tuple[float, float, float, float] | None = None,
) -> str:
    """
    Emit a thin non-colliding floor patch used as a visual/terrain prior.

    These patches make the ecology legible to cameras without disturbing robot
    physics. An FPAA-oriented replacement could express the same prior through a
    printed conductivity/reflectance region or a fixed chemical/thermal source.
    """
    rotation_line = ""
    if rotation is not None:
        rx, ry, rz, angle = rotation
        rotation_line = f"\n  rotation {rx:.6f} {ry:.6f} {rz:.6f} {angle:.5f}"
    cr, cg, cb = color
    return f"""Transform {{
  translation {x:.4f} {z:.4f} {center_height:.4f}{rotation_line}
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor {cr:.2f} {cg:.2f} {cb:.2f}
        roughness {roughness:.2f}
        metalness {metalness:.2f}
        transparency {transparency:.2f}
      }}
      geometry Box {{
        size {size_x:.4f} {size_y:.4f} {size_z:.4f}
      }}
    }}
  ]
}}"""


def point_light_node(
    x: float,
    z: float,
    height: float,
    *,
    color: Tuple[float, float, float],
    intensity: float,
    attenuation: float,
    radius: float,
) -> str:
    """
    Emit a localized light source for photo/thermo-taxis style stimulation.

    The celegans sensors already model "heat" as irradiance channels, so a
    point light is the most direct scene primitive here. An analog replacement
    would likely be a compact optical or thermal emitter in the same location.
    """
    cr, cg, cb = color
    return f"""PointLight {{
  location {x:.4f} {z:.4f} {height:.4f}
  color {cr:.2f} {cg:.2f} {cb:.2f}
  intensity {intensity:.2f}
  attenuation 0 0 {attenuation:.4f}
  radius {radius:.2f}
  castShadows FALSE
}}"""


def solid_node(
    name: str,
    x: float,
    z: float,
    center_height: float,
    *,
    geometry: str,
    bounding_object: str,
    color: Tuple[float, float, float],
    roughness: float = 0.60,
    metalness: float = 0.0,
    transparency: float = 0.0,
    emissive_color: Tuple[float, float, float] | None = None,
    rotation: Tuple[float, float, float, float] | None = None,
    mass: float | None = None,
    linear_velocity: Tuple[float, float, float] | None = None,
    angular_velocity: Tuple[float, float, float] | None = None,
) -> str:
    """
    Emit a reusable collision-enabled Webots `Solid` block.

    The ecology builders stay focused on sensory intent while this helper owns
    the VRML syntax. That modularity matters for future FPAA substitutions: the
    high-level ecology can remain stable while the implementation swaps between
    rigid bodies, fixed stimulus emitters, or mixed analog/digital hybrids.
    """
    rotation_line = ""
    if rotation is not None:
        rx, ry, rz, angle = rotation
        rotation_line = f"\n  rotation {rx:.6f} {ry:.6f} {rz:.6f} {angle:.5f}"

    linear_velocity_line = ""
    if linear_velocity is not None:
        vx, vy, vz = linear_velocity
        linear_velocity_line = f"\n  linearVelocity {vx:.4f} {vy:.4f} {vz:.4f}"

    angular_velocity_line = ""
    if angular_velocity is not None:
        ax, ay, az = angular_velocity
        angular_velocity_line = f"\n  angularVelocity {ax:.4f} {ay:.4f} {az:.4f}"

    er, eg, eb = (0.0, 0.0, 0.0)
    emissive_line = ""
    if emissive_color is not None:
        er, eg, eb = emissive_color
        emissive_line = f"\n        emissiveColor {er:.2f} {eg:.2f} {eb:.2f}"

    cr, cg, cb = color
    physics_block = ""
    if mass is not None:
        physics_block = f"""
  physics Physics {{
    density -1
    mass {mass:.5f}
  }}"""

    return f"""Solid {{
  translation {x:.4f} {z:.4f} {center_height:.4f}{rotation_line}{linear_velocity_line}{angular_velocity_line}
  name "{name}"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor {cr:.2f} {cg:.2f} {cb:.2f}
        roughness {roughness:.2f}
        metalness {metalness:.2f}
        transparency {transparency:.2f}{emissive_line}
      }}
      geometry {geometry}
    }}
  ]
  boundingObject {bounding_object}{physics_block}
}}"""


def box_node(
    name: str,
    x: float,
    z: float,
    center_height: float,
    *,
    size_x: float,
    size_y: float,
    size_z: float,
    color: Tuple[float, float, float],
    roughness: float = 0.60,
    metalness: float = 0.0,
    transparency: float = 0.0,
    emissive_color: Tuple[float, float, float] | None = None,
    rotation: Tuple[float, float, float, float] | None = None,
    mass: float | None = None,
    linear_velocity: Tuple[float, float, float] | None = None,
    angular_velocity: Tuple[float, float, float] | None = None,
) -> str:
    """
    Build a box-shaped prop for rails, walls, ramps, gates, and target panels.

    Box primitives are cheap to simulate and easy to replace later with analog
    field approximations such as resistive barriers, reflectance panels, or
    local contact strips on an FPAA-driven testbed.
    """
    return solid_node(
        name,
        x,
        z,
        center_height,
        geometry=f"Box {{\n        size {size_x:.4f} {size_y:.4f} {size_z:.4f}\n      }}",
        bounding_object=f"Box {{\n    size {size_x:.4f} {size_y:.4f} {size_z:.4f}\n  }}",
        color=color,
        roughness=roughness,
        metalness=metalness,
        transparency=transparency,
        emissive_color=emissive_color,
        rotation=rotation,
        mass=mass,
        linear_velocity=linear_velocity,
        angular_velocity=angular_velocity,
    )


def sphere_node(
    name: str,
    x: float,
    z: float,
    center_height: float,
    *,
    radius: float,
    color: Tuple[float, float, float],
    roughness: float = 0.45,
    metalness: float = 0.0,
    transparency: float = 0.0,
    emissive_color: Tuple[float, float, float] | None = None,
    mass: float | None = None,
    linear_velocity: Tuple[float, float, float] | None = None,
    angular_velocity: Tuple[float, float, float] | None = None,
) -> str:
    """
    Build a spherical prop for pellets, fruit, and push targets.

    Spheres give smooth contact dynamics and moving highlights for event-camera
    sensors. In an analog equivalent they map naturally to localized isotropic
    stimulus sources such as light blobs, heat spots, or odor emitters.
    """
    return solid_node(
        name,
        x,
        z,
        center_height,
        geometry=f"Sphere {{\n        radius {radius:.4f}\n      }}",
        bounding_object=f"Sphere {{\n    radius {radius:.4f}\n  }}",
        color=color,
        roughness=roughness,
        metalness=metalness,
        transparency=transparency,
        emissive_color=emissive_color,
        mass=mass,
        linear_velocity=linear_velocity,
        angular_velocity=angular_velocity,
    )


def cylinder_node(
    name: str,
    x: float,
    z: float,
    center_height: float,
    *,
    radius: float,
    height: float,
    color: Tuple[float, float, float],
    roughness: float = 0.60,
    metalness: float = 0.0,
    transparency: float = 0.0,
    emissive_color: Tuple[float, float, float] | None = None,
    rotation: Tuple[float, float, float, float] | None = None,
    mass: float | None = None,
    linear_velocity: Tuple[float, float, float] | None = None,
    angular_velocity: Tuple[float, float, float] | None = None,
) -> str:
    """
    Build a cylindrical prop for posts, perches, and gate markers.

    Cylinders create strong contact and range signatures with less snagging than
    boxes. They also translate well to analog fixtures such as stalks, emitters,
    or tactile posts surrounding an FPAA-controlled arena.
    """
    return solid_node(
        name,
        x,
        z,
        center_height,
        geometry=f"Cylinder {{\n        radius {radius:.4f}\n        height {height:.4f}\n      }}",
        bounding_object=f"Cylinder {{\n    radius {radius:.4f}\n    height {height:.4f}\n  }}",
        color=color,
        roughness=roughness,
        metalness=metalness,
        transparency=transparency,
        emissive_color=emissive_color,
        rotation=rotation,
        mass=mass,
        linear_velocity=linear_velocity,
        angular_velocity=angular_velocity,
    )


def clamp(value: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, value))


def normalize3(x: float, y: float, z: float) -> Tuple[float, float, float]:
    magnitude = math.sqrt(x * x + y * y + z * z)
    if magnitude <= 1e-9:
        return 0.0, 0.0, 0.0
    return x / magnitude, y / magnitude, z / magnitude


def cross3(
    ax: float,
    ay: float,
    az: float,
    bx: float,
    by: float,
    bz: float,
) -> Tuple[float, float, float]:
    return (
        ay * bz - az * by,
        az * bx - ax * bz,
        ax * by - ay * bx,
    )


def rotation_matrix_to_axis_angle(
    matrix: Tuple[Tuple[float, float, float], Tuple[float, float, float], Tuple[float, float, float]],
) -> Tuple[float, float, float, float]:
    """Convert a camera basis matrix into the axis-angle form required by Webots."""
    trace = matrix[0][0] + matrix[1][1] + matrix[2][2]
    angle = math.acos(clamp((trace - 1.0) * 0.5, -1.0, 1.0))
    if angle <= 1e-7:
        return 0.0, 1.0, 0.0, 0.0

    sin_angle = math.sin(angle)
    if abs(sin_angle) > 1e-6:
        axis = (
            (matrix[2][1] - matrix[1][2]) / (2.0 * sin_angle),
            (matrix[0][2] - matrix[2][0]) / (2.0 * sin_angle),
            (matrix[1][0] - matrix[0][1]) / (2.0 * sin_angle),
        )
    else:
        xx = max(0.0, (matrix[0][0] + 1.0) * 0.5)
        yy = max(0.0, (matrix[1][1] + 1.0) * 0.5)
        zz = max(0.0, (matrix[2][2] + 1.0) * 0.5)
        axis = (
            math.sqrt(xx),
            math.copysign(math.sqrt(yy), matrix[0][1] + matrix[1][0]),
            math.copysign(math.sqrt(zz), matrix[0][2] + matrix[2][0]),
        )

    axis_x, axis_y, axis_z = normalize3(*axis)
    return axis_x, axis_y, axis_z, angle


def axis_angle_to_matrix(
    axis_x: float,
    axis_y: float,
    axis_z: float,
    angle: float,
) -> Tuple[Tuple[float, float, float], Tuple[float, float, float], Tuple[float, float, float]]:
    axis_x, axis_y, axis_z = normalize3(axis_x, axis_y, axis_z)
    if abs(axis_x) + abs(axis_y) + abs(axis_z) <= 1e-9 or abs(angle) <= 1e-9:
        return (
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, 1.0),
        )

    c = math.cos(angle)
    s = math.sin(angle)
    t = 1.0 - c
    return (
        (
            t * axis_x * axis_x + c,
            t * axis_x * axis_y - s * axis_z,
            t * axis_x * axis_z + s * axis_y,
        ),
        (
            t * axis_x * axis_y + s * axis_z,
            t * axis_y * axis_y + c,
            t * axis_y * axis_z - s * axis_x,
        ),
        (
            t * axis_x * axis_z - s * axis_y,
            t * axis_y * axis_z + s * axis_x,
            t * axis_z * axis_z + c,
        ),
    )


def matrix_mul(
    a: Tuple[Tuple[float, float, float], Tuple[float, float, float], Tuple[float, float, float]],
    b: Tuple[Tuple[float, float, float], Tuple[float, float, float], Tuple[float, float, float]],
) -> Tuple[Tuple[float, float, float], Tuple[float, float, float], Tuple[float, float, float]]:
    return tuple(
        tuple(sum(a[row][k] * b[k][col] for k in range(3)) for col in range(3))
        for row in range(3)
    )


def viewpoint_orientation(
    eye_x: float,
    eye_y: float,
    eye_z: float,
    target_x: float,
    target_y: float,
    target_z: float,
) -> Tuple[float, float, float, float]:
    """
    Build a level Webots `Viewpoint.orientation` value from an eye/target pair.

    Webots uses a `Viewpoint` basis with +X as the forward direction and +Z as
    the camera-up direction. We therefore solve a look-at rotation in that frame
    so the generated room remains level in Webots' Z-up world.
    """
    forward_x, forward_y, forward_z = normalize3(
        target_x - eye_x,
        target_y - eye_y,
        target_z - eye_z,
    )
    if abs(forward_x) + abs(forward_y) + abs(forward_z) <= 1e-9:
        return 0.0, 0.0, 1.0, 0.0

    up_x, up_y, up_z = 0.0, 0.0, 1.0
    if abs(forward_z) >= 0.999:
        up_x, up_y, up_z = 0.0, 1.0, 0.0

    left_x, left_y, left_z = normalize3(
        *cross3(up_x, up_y, up_z, forward_x, forward_y, forward_z)
    )
    up_x, up_y, up_z = cross3(forward_x, forward_y, forward_z, left_x, left_y, left_z)
    matrix = (
        (forward_x, left_x, up_x),
        (forward_y, left_y, up_y),
        (forward_z, left_z, up_z),
    )
    return rotation_matrix_to_axis_angle(matrix)


def scene_metrics(
    entries: List[tuple[str, str, str, float, str]],
    positions: List[Tuple[float, float]],
) -> Tuple[float, float, float, float, float]:
    if not entries or not positions:
        return 0.0, 0.0, 0.35, 0.0, 0.0

    xs = [x for x, _ in positions]
    zs = [z for _, z in positions]
    center_x = 0.5 * (min(xs) + max(xs))
    center_z = 0.5 * (min(zs) + max(zs))
    span_x = max(xs) - min(xs)
    span_z = max(zs) - min(zs)

    scene_radius = 0.0
    for (_, _, _, _, robot_kind), (x, z) in zip(entries, positions):
        footprint = ROBOT_VIEW_RADIUS.get(robot_kind, 1.0)
        scene_radius = max(
            scene_radius,
            math.hypot(x - center_x, z - center_z) + footprint,
        )

    return center_x, center_z, max(scene_radius, 0.35), span_x, span_z


def zone_basis(x: float, z: float, center_x: float, center_z: float) -> Tuple[float, float, float, float]:
    dx = x - center_x
    dz = z - center_z
    norm = math.hypot(dx, dz)
    if norm <= 1e-6:
        outward_x, outward_z = 0.0, -1.0
    else:
        outward_x, outward_z = dx / norm, dz / norm
    lateral_x, lateral_z = -outward_z, outward_x
    return outward_x, outward_z, lateral_x, lateral_z


def clamp_room_point(x: float, z: float, room_half_size: float, margin: float) -> Tuple[float, float]:
    bound = max(1.0, room_half_size - margin)
    return clamp(x, -bound, bound), clamp(z, -bound, bound)


def startup_camera(
    entries: List[tuple[str, str, str, float, str]],
    positions: List[Tuple[float, float]],
) -> Tuple[float, float, float, float]:
    """
    Return `(fov, x, y, z)` for a startup camera that keeps the horizon level and
    frames the active robot cluster instead of the full room shell.
    """
    if not entries or not positions:
        return 0.84, -7.0, 0.0, 6.0

    center_x, center_z, scene_radius, _span_x, _span_z = scene_metrics(entries, positions)
    robot_kinds = {robot_kind for (_, _, _, _, robot_kind) in entries}
    has_large = bool(robot_kinds.intersection({"nao", "hexapod"}))
    only_small_bio = robot_kinds.issubset({"celegans", "drosophila"})

    if len(entries) == 1 and robot_kinds == {"celegans"}:
        cam_height = max(1.20, scene_radius * 2.60)
        fov = 0.72
    elif only_small_bio and not has_large:
        cam_height = max(1.90, scene_radius * 2.25)
        fov = 0.80 if scene_radius < 2.4 else 0.86
    else:
        cam_height = max(4.60, scene_radius * 1.75)
        fov = 0.88

    # In Webots the floor lies on the X/Y plane and +Z is vertical. Place the
    # camera slightly behind the active cluster on +Y, then look back toward it.
    cam_y = center_z + max(1.50, cam_height * 1.17)
    return fov, center_x, cam_y, cam_height


def scene_target_height(entries: List[tuple[str, str, str, float, str]]) -> float:
    """Return a stable camera look-at height above the floor for the active scene."""
    robot_kinds = {robot_kind for (_, _, _, _, robot_kind) in entries}
    if "nao" in robot_kinds:
        return 0.72
    if "hexapod" in robot_kinds:
        return 0.36
    if robot_kinds == {"drosophila"}:
        return 0.14
    if robot_kinds == {"celegans"}:
        return 0.08
    return 0.18


def robot_node(
    proto_name: str,
    name: str,
    brain_id: str,
    x: float,
    height: float,
    y: float,
    robot_kind: str,
    yaw: float,
    controller_args: List[str],
) -> str:
    args_lines = "\n".join(f'    "{arg}"' for arg in controller_args)
    if robot_kind in {"celegans", "drosophila"}:
        # The custom bio robot meshes were authored in a Y-up local frame.
        # Rotate them into Webots' Z-up world, then apply planar heading.
        rotation_matrix = matrix_mul(
            axis_angle_to_matrix(0.0, 0.0, 1.0, yaw),
            axis_angle_to_matrix(1.0, 0.0, 0.0, math.pi * 0.5),
        )
        rot_x, rot_y, rot_z, rot_angle = rotation_matrix_to_axis_angle(rotation_matrix)
    else:
        rot_x, rot_y, rot_z, rot_angle = 0.0, 0.0, 1.0, yaw
    return f"""{proto_name} {{
  translation {x:.4f} {y:.4f} {height:.4f}
  rotation {rot_x:.6f} {rot_y:.6f} {rot_z:.6f} {rot_angle:.5f}
  name "{name}"
  controller "nao_nn_controller_uds"
  controllerArgs [
{args_lines}
  ]
}}
"""


def build_celegans_interactive_ecology(
    robot_name: str,
    x: float,
    z: float,
    forward_x: float,
    forward_z: float,
    lateral_x: float,
    lateral_z: float,
    bound: float,
) -> str:
    """
    Build a compact celegans micro-ecology around one spawn point.

    The prop mix targets the actual celegans channels present in the PROTO:
    front/rear touch, short-range taste/chemical arcs, longer flow sensors, and
    light/heat channels. An FPAA-style replacement could preserve this layout as
    a set of local analog emitters and tactile barriers without changing the
    rest of the mixed-world orchestration.
    """
    prefix = robot_name.lower()
    yaw = math.atan2(forward_z, forward_x)
    blocks: List[str] = [
        floor_patch_box(
            x,
            z,
            0.004,
            size_x=0.92,
            size_y=0.62,
            size_z=0.004,
            color=(0.76, 0.81, 0.78),
            roughness=0.90,
            transparency=0.34,
            rotation=yaw_rotation(yaw),
        )
    ]

    left_wall_x, left_wall_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.20,
        lateral=-0.17,
        bound=bound,
    )
    right_wall_x, right_wall_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.20,
        lateral=0.17,
        bound=bound,
    )
    rear_wall_x, rear_wall_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=-0.18,
        lateral=0.0,
        bound=bound,
    )
    blocks.extend(
        [
            box_node(
                f"eco_{prefix}_maze_left",
                left_wall_x,
                left_wall_z,
                0.046,
                size_x=0.42,
                size_y=0.040,
                size_z=0.092,
                color=(0.28, 0.57, 0.35),
                roughness=0.66,
                rotation=yaw_rotation(yaw),
            ),
            box_node(
                f"eco_{prefix}_maze_right",
                right_wall_x,
                right_wall_z,
                0.046,
                size_x=0.42,
                size_y=0.040,
                size_z=0.092,
                color=(0.26, 0.49, 0.68),
                roughness=0.62,
                rotation=yaw_rotation(yaw),
            ),
            box_node(
                f"eco_{prefix}_rear_barrier",
                rear_wall_x,
                rear_wall_z,
                0.038,
                size_x=0.28,
                size_y=0.038,
                size_z=0.076,
                color=(0.62, 0.35, 0.23),
                roughness=0.60,
                rotation=yaw_rotation(yaw + math.pi * 0.5),
            ),
        ]
    )

    ramp_x, ramp_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.02,
        lateral=0.29,
        bound=bound,
    )
    blocks.append(
        box_node(
            f"eco_{prefix}_ridge_ramp",
            ramp_x,
            ramp_z,
            0.038,
            size_x=0.24,
            size_y=0.18,
            size_z=0.055,
            color=(0.67, 0.56, 0.28),
            roughness=0.58,
            rotation=ramp_rotation(forward_x, forward_z, lateral_x, lateral_z, 0.26),
        )
    )

    for suffix, local_forward, local_lateral, radius, color in (
        ("taste_center", 0.20, 0.00, 0.030, (0.92, 0.74, 0.30)),
        ("taste_left", 0.18, 0.09, 0.028, (0.80, 0.56, 0.22)),
        ("taste_right", 0.18, -0.09, 0.028, (0.76, 0.48, 0.20)),
    ):
        taste_x, taste_z = oriented_floor_point(
            x,
            z,
            forward_x,
            forward_z,
            lateral_x,
            lateral_z,
            forward=local_forward,
            lateral=local_lateral,
            bound=bound,
        )
        blocks.append(
            sphere_node(
                f"eco_{prefix}_{suffix}",
                taste_x,
                taste_z,
                0.022,
                radius=radius,
                color=color,
                roughness=0.44,
            )
        )

    warm_x, warm_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.22,
        lateral=-0.19,
        bound=bound,
    )
    cool_x, cool_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=-0.06,
        lateral=0.21,
        bound=bound,
    )
    blocks.extend(
        [
            point_light_node(
                warm_x,
                warm_z,
                0.31,
                color=(1.00, 0.62, 0.28),
                intensity=0.55,
                attenuation=4.50,
                radius=1.80,
            ),
            point_light_node(
                cool_x,
                cool_z,
                0.29,
                color=(0.72, 0.87, 1.00),
                intensity=0.42,
                attenuation=5.20,
                radius=1.65,
            ),
        ]
    )

    pellet_x, pellet_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=-0.28,
        lateral=0.16,
        bound=bound,
    )
    pellet_velocity = oriented_velocity(
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.42,
        lateral=-0.14,
    )
    bar_x, bar_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.03,
        lateral=-0.30,
        bound=bound,
    )
    bar_velocity = oriented_velocity(
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=-0.16,
        lateral=0.28,
    )
    blocks.extend(
        [
            sphere_node(
                f"eco_{prefix}_vibration_pellet",
                pellet_x,
                pellet_z,
                0.046,
                radius=0.046,
                color=(0.91, 0.83, 0.32),
                roughness=0.34,
                mass=0.018,
                linear_velocity=pellet_velocity,
                angular_velocity=(0.0, 0.0, 2.2),
            ),
            box_node(
                f"eco_{prefix}_flow_bar",
                bar_x,
                bar_z,
                0.028,
                size_x=0.18,
                size_y=0.050,
                size_z=0.044,
                color=(0.86, 0.44, 0.18),
                roughness=0.38,
                rotation=yaw_rotation(yaw + 0.42),
                mass=0.022,
                linear_velocity=bar_velocity,
                angular_velocity=(0.0, 0.0, 2.6),
            ),
        ]
    )

    return "\n".join(blocks)


def build_drosophila_interactive_ecology(
    robot_name: str,
    x: float,
    z: float,
    forward_x: float,
    forward_z: float,
    lateral_x: float,
    lateral_z: float,
    bound: float,
) -> str:
    """
    Build a fly-scale orchard patch for drosophila robots.

    The scene combines close rails for the 24-way proximity ring, small visual
    targets for the compound-eye cameras, and perch/fruit cues that invite
    exploratory contact. In future FPAA experiments those same cues could be
    rendered as analog reflectance panels, odor sources, and tactile guides.
    """
    prefix = robot_name.lower()
    yaw = math.atan2(forward_z, forward_x)
    blocks: List[str] = [
        floor_patch_box(
            x,
            z,
            0.003,
            size_x=0.78,
            size_y=0.58,
            size_z=0.004,
            color=(0.39, 0.31, 0.20),
            roughness=0.88,
            rotation=yaw_rotation(yaw),
        )
    ]

    for suffix, local_forward, local_lateral, length, yaw_offset in (
        ("rail_left", 0.00, -0.115, 0.18, 0.0),
        ("rail_right", 0.00, 0.115, 0.18, 0.0),
        ("rail_rear", -0.11, 0.00, 0.24, math.pi * 0.5),
    ):
        rail_x, rail_z = oriented_floor_point(
            x,
            z,
            forward_x,
            forward_z,
            lateral_x,
            lateral_z,
            forward=local_forward,
            lateral=local_lateral,
            bound=bound,
        )
        blocks.append(
            box_node(
                f"eco_{prefix}_{suffix}",
                rail_x,
                rail_z,
                0.022,
                size_x=length,
                size_y=0.016,
                size_z=0.044,
                color=(0.43, 0.33, 0.20),
                roughness=0.74,
                rotation=yaw_rotation(yaw + yaw_offset),
            )
        )

    for suffix, local_forward, local_lateral, color in (
        ("panel_left", 0.15, -0.09, (0.93, 0.78, 0.34)),
        ("panel_right", 0.15, 0.09, (0.28, 0.70, 0.86)),
    ):
        panel_x, panel_z = oriented_floor_point(
            x,
            z,
            forward_x,
            forward_z,
            lateral_x,
            lateral_z,
            forward=local_forward,
            lateral=local_lateral,
            bound=bound,
        )
        blocks.append(
            box_node(
                f"eco_{prefix}_{suffix}",
                panel_x,
                panel_z,
                0.080,
                size_x=0.018,
                size_y=0.072,
                size_z=0.160,
                color=color,
                roughness=0.32,
                emissive_color=(color[0] * 0.18, color[1] * 0.18, color[2] * 0.18),
                rotation=yaw_rotation(yaw),
            )
        )

    perch_x, perch_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.17,
        lateral=0.05,
        bound=bound,
    )
    blocks.extend(
        [
            cylinder_node(
                f"eco_{prefix}_perch_post",
                perch_x,
                perch_z,
                0.080,
                radius=0.014,
                height=0.160,
                color=(0.46, 0.35, 0.22),
                roughness=0.72,
            ),
            box_node(
                f"eco_{prefix}_perch_crossbar",
                perch_x,
                perch_z,
                0.162,
                size_x=0.10,
                size_y=0.028,
                size_z=0.012,
                color=(0.48, 0.37, 0.23),
                roughness=0.70,
                rotation=yaw_rotation(yaw + math.pi * 0.5),
            ),
        ]
    )

    for suffix, local_forward, local_lateral, radius, color in (
        ("fruit_a", 0.09, 0.04, 0.018, (0.92, 0.66, 0.24)),
        ("fruit_b", 0.12, 0.08, 0.016, (0.86, 0.34, 0.18)),
        ("fruit_c", 0.07, 0.10, 0.014, (0.58, 0.71, 0.26)),
    ):
        fruit_x, fruit_z = oriented_floor_point(
            x,
            z,
            forward_x,
            forward_z,
            lateral_x,
            lateral_z,
            forward=local_forward,
            lateral=local_lateral,
            bound=bound,
        )
        blocks.append(
            sphere_node(
                f"eco_{prefix}_{suffix}",
                fruit_x,
                fruit_z,
                0.018,
                radius=radius,
                color=color,
                roughness=0.42,
            )
        )

    bead_x, bead_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.03,
        lateral=-0.10,
        bound=bound,
    )
    bead_velocity = oriented_velocity(
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.02,
        lateral=0.18,
    )
    blocks.append(
        sphere_node(
            f"eco_{prefix}_rolling_bead",
            bead_x,
            bead_z,
            0.014,
            radius=0.014,
            color=(0.96, 0.88, 0.30),
            roughness=0.30,
            mass=0.00080,
            linear_velocity=bead_velocity,
            angular_velocity=(0.0, 0.0, 8.0),
        )
    )

    return "\n".join(blocks)


def build_nao_interactive_ecology(
    robot_name: str,
    x: float,
    z: float,
    forward_x: float,
    forward_z: float,
    lateral_x: float,
    lateral_z: float,
    bound: float,
) -> str:
    """
    Build a manipulation and navigation pocket for one NAO spawn.

    The layout gives NAO immediate vision, sonar, contact, and balance cues:
    pushable objects near the feet, a reachable ramp, and a bright target gate
    deeper in the lane. The same topology can later map onto analog sonars,
    tactile pads, and field-coded landmarks around an FPAA arena.
    """
    prefix = robot_name.lower()
    yaw = math.atan2(forward_z, forward_x)
    blocks: List[str] = [
        floor_patch_box(
            x,
            z,
            0.004,
            size_x=2.30,
            size_y=1.64,
            size_z=0.006,
            color=(0.37, 0.40, 0.36),
            roughness=0.84,
            rotation=yaw_rotation(yaw),
        ),
        floor_patch_box(
            *oriented_floor_point(
                x,
                z,
                forward_x,
                forward_z,
                lateral_x,
                lateral_z,
                forward=1.10,
                lateral=0.00,
                bound=bound,
            ),
            0.006,
            size_x=0.22,
            size_y=1.06,
            size_z=0.004,
            color=(0.92, 0.76, 0.34),
            roughness=0.52,
            transparency=0.12,
            rotation=yaw_rotation(yaw),
        ),
    ]

    ball_x, ball_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.58,
        lateral=-0.42,
        bound=bound,
    )
    cube_x, cube_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.54,
        lateral=0.40,
        bound=bound,
    )
    ramp_x, ramp_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=0.96,
        lateral=0.05,
        bound=bound,
    )
    blocks.extend(
        [
            sphere_node(
                f"eco_{prefix}_push_ball",
                ball_x,
                ball_z,
                0.140,
                radius=0.140,
                color=(0.92, 0.43, 0.22),
                roughness=0.28,
                mass=0.240,
            ),
            box_node(
                f"eco_{prefix}_push_cube",
                cube_x,
                cube_z,
                0.130,
                size_x=0.26,
                size_y=0.26,
                size_z=0.26,
                color=(0.32, 0.62, 0.78),
                roughness=0.40,
                mass=0.380,
            ),
            box_node(
                f"eco_{prefix}_balance_ramp",
                ramp_x,
                ramp_z,
                0.110,
                size_x=0.82,
                size_y=0.62,
                size_z=0.140,
                color=(0.67, 0.56, 0.34),
                roughness=0.58,
                rotation=ramp_rotation(forward_x, forward_z, lateral_x, lateral_z, 0.18),
            ),
        ]
    )

    for suffix, local_lateral, panel_color in (
        ("gate_left", -0.58, (0.92, 0.76, 0.34)),
        ("gate_right", 0.58, (0.30, 0.78, 0.84)),
    ):
        post_x, post_z = oriented_floor_point(
            x,
            z,
            forward_x,
            forward_z,
            lateral_x,
            lateral_z,
            forward=1.55,
            lateral=local_lateral,
            bound=bound,
        )
        panel_x, panel_z = oriented_floor_point(
            x,
            z,
            forward_x,
            forward_z,
            lateral_x,
            lateral_z,
            forward=1.38,
            lateral=local_lateral * 1.24,
            bound=bound,
        )
        blocks.extend(
            [
                cylinder_node(
                    f"eco_{prefix}_{suffix}_post",
                    post_x,
                    post_z,
                    0.430,
                    radius=0.060,
                    height=0.860,
                    color=(0.56, 0.47, 0.32),
                    roughness=0.64,
                ),
                box_node(
                    f"eco_{prefix}_{suffix}_panel",
                    panel_x,
                    panel_z,
                    0.580,
                    size_x=0.032,
                    size_y=0.40,
                    size_z=0.70,
                    color=panel_color,
                    roughness=0.36,
                    emissive_color=(
                        panel_color[0] * 0.12,
                        panel_color[1] * 0.12,
                        panel_color[2] * 0.12,
                    ),
                    rotation=yaw_rotation(yaw),
                ),
            ]
        )

    crossbar_x, crossbar_z = oriented_floor_point(
        x,
        z,
        forward_x,
        forward_z,
        lateral_x,
        lateral_z,
        forward=1.55,
        lateral=0.00,
        bound=bound,
    )
    blocks.append(
        box_node(
            f"eco_{prefix}_gate_crossbar",
            crossbar_x,
            crossbar_z,
            0.830,
            size_x=1.26,
            size_y=0.08,
            size_z=0.08,
            color=(0.76, 0.69, 0.48),
            roughness=0.52,
            rotation=yaw_rotation(yaw + math.pi * 0.5),
        )
    )

    return "\n".join(blocks)


def build_robot_stimuli(
    entries: List[tuple[str, str, str, float, str]],
    positions: List[Tuple[float, float]],
    arena_half_size: float,
) -> str:
    """
    Build species-specific interactive ecologies around each robot spawn.

    The mixed world no longer uses a generic ring of props. Each robot now gets
    a small environment tuned to its actual Webots sensor suite so the AARNN
    receives meaningful stimuli immediately after reset.
    """
    blocks: List[str] = []
    bound = max(0.8, arena_half_size - 0.8)
    center_x, center_z, _scene_radius, _span_x, _span_z = scene_metrics(entries, positions)

    for (_, robot_name, _, _, robot_kind), (x, z) in zip(entries, positions):
        forward_x, forward_z, lateral_x, lateral_z = ecology_basis(x, z, center_x, center_z)
        if robot_kind == "celegans":
            blocks.append(
                build_celegans_interactive_ecology(
                    robot_name,
                    x,
                    z,
                    forward_x,
                    forward_z,
                    lateral_x,
                    lateral_z,
                    bound,
                )
            )
        elif robot_kind == "drosophila":
            blocks.append(
                build_drosophila_interactive_ecology(
                    robot_name,
                    x,
                    z,
                    forward_x,
                    forward_z,
                    lateral_x,
                    lateral_z,
                    bound,
                )
            )
        else:
            blocks.append(
                build_nao_interactive_ecology(
                    robot_name,
                    x,
                    z,
                    forward_x,
                    forward_z,
                    lateral_x,
                    lateral_z,
                    bound,
                )
            )

    return "\n".join(blocks) + ("\n" if blocks else "")


def build_demo_zone_furniture(
    entries: List[tuple[str, str, str, float, str]],
    positions: List[Tuple[float, float]],
    room_half_size: float,
) -> str:
    blocks: List[str] = []
    center_x, center_z, _scene_radius, _span_x, _span_z = scene_metrics(entries, positions)

    for idx, ((_, robot_name, _, _, robot_kind), (x, z)) in enumerate(zip(entries, positions), 1):
        outward_x, outward_z, lateral_x, lateral_z = zone_basis(x, z, center_x, center_z)
        clearance = ROBOT_ZONE_CLEAR_RADIUS.get(robot_kind, 0.9)
        service_x, service_z = clamp_room_point(
            x + outward_x * (clearance + 0.68) + lateral_x * 0.26,
            z + outward_z * (clearance + 0.68) + lateral_z * 0.26,
            room_half_size,
            0.95,
        )
        accent_x, accent_z = clamp_room_point(
            x - lateral_x * (clearance * 0.55),
            z - lateral_z * (clearance * 0.55),
            room_half_size,
            0.75,
        )

        if robot_kind == "celegans":
            blocks.append(
                f"""Transform {{
  translation {x:.4f} {z:.4f} 0.004
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.29 0.50 0.58
        roughness 0.74
        metalness 0.02
        transparency 0.22
      }}
      geometry Cylinder {{
        radius 0.56
        height 0.008
      }}
    }}
  ]
}}
Solid {{
  translation {service_x:.4f} {service_z:.4f} 0.060
  name "zone_{robot_name.lower()}_lab_station"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.56 0.49 0.40
        roughness 0.72
      }}
      geometry Box {{
        size 0.62 0.40 0.12
      }}
    }}
  ]
  boundingObject Box {{
    size 0.62 0.40 0.12
  }}
}}
Transform {{
  translation {service_x:.4f} {service_z:.4f} 0.126
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.31 0.55 0.73
        roughness 0.12
        metalness 0.05
        transparency 0.54
      }}
      geometry Box {{
        size 0.50 0.28 0.010
      }}
    }}
  ]
}}
Transform {{
  translation {accent_x:.4f} {accent_z:.4f} 0.010
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.72 0.79 0.61
        roughness 0.88
      }}
      geometry Box {{
        size 0.28 0.18 0.014
      }}
    }}
  ]
}}"""
            )
        elif robot_kind == "drosophila":
            perch_x, perch_z = clamp_room_point(
                x + outward_x * (clearance + 0.82) - lateral_x * 0.22,
                z + outward_z * (clearance + 0.82) - lateral_z * 0.22,
                room_half_size,
                0.90,
            )
            blocks.append(
                f"""Transform {{
  translation {x:.4f} {z:.4f} 0.004
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.46 0.37 0.24
        roughness 0.76
        transparency 0.20
      }}
      geometry Cylinder {{
        radius 0.66
        height 0.008
      }}
    }}
  ]
}}
Solid {{
  translation {service_x:.4f} {service_z:.4f} 0.210
  name "zone_{robot_name.lower()}_fruit_stand"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.49 0.37 0.24
        roughness 0.68
      }}
      geometry Cylinder {{
        radius 0.22
        height 0.42
      }}
    }}
  ]
  boundingObject Cylinder {{
    radius 0.22
    height 0.42
  }}
}}
Transform {{
  translation {service_x:.4f} {service_z:.4f} 0.442
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.92 0.79 0.36
        roughness 0.42
      }}
      geometry Sphere {{
        radius 0.060
      }}
    }}
    Transform {{
      translation 0.090 0.035 0.015
      children [
        Shape {{
          appearance PBRAppearance {{
            baseColor 0.87 0.34 0.18
            roughness 0.46
          }}
          geometry Sphere {{
            radius 0.040
          }}
        }}
      ]
    }}
    Transform {{
      translation -0.075 -0.030 0.018
      children [
        Shape {{
          appearance PBRAppearance {{
            baseColor 0.58 0.70 0.26
            roughness 0.54
          }}
          geometry Sphere {{
            radius 0.036
          }}
        }}
      ]
    }}
  ]
}}
Solid {{
  translation {perch_x:.4f} {perch_z:.4f} 0.310
  name "zone_{robot_name.lower()}_perch"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.43 0.31 0.20
        roughness 0.74
      }}
      geometry Cylinder {{
        radius 0.034
        height 0.62
      }}
    }}
  ]
  boundingObject Cylinder {{
    radius 0.034
    height 0.62
  }}
}}
Transform {{
  translation {perch_x:.4f} {perch_z:.4f} 0.635
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.48 0.35 0.23
        roughness 0.72
      }}
      geometry Box {{
        size 0.46 0.08 0.020
      }}
    }}
  ]
}}"""
            )
        else:
            gate_x, gate_z = clamp_room_point(
                x + outward_x * (clearance + 0.95),
                z + outward_z * (clearance + 0.95),
                room_half_size,
                1.10,
            )
            blocks.append(
                f"""Transform {{
  translation {x:.4f} {z:.4f} 0.005
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.43 0.39 0.34
        roughness 0.72
        transparency 0.12
      }}
      geometry Box {{
        size 2.40 2.04 0.010
      }}
    }}
  ]
}}
Solid {{
  translation {service_x:.4f} {service_z:.4f} 0.220
  name "zone_{robot_name.lower()}_service_bench"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.58 0.53 0.47
        roughness 0.62
      }}
      geometry Box {{
        size 1.45 0.62 0.44
      }}
    }}
  ]
  boundingObject Box {{
    size 1.45 0.62 0.44
  }}
}}
Solid {{
  translation {gate_x - lateral_x * 0.58:.4f} {gate_z - lateral_z * 0.58:.4f} 0.210
  name "zone_{robot_name.lower()}_marker_left"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.92 0.76 0.34
        roughness 0.48
      }}
      geometry Box {{
        size 0.12 0.12 0.42
      }}
    }}
  ]
  boundingObject Box {{
    size 0.12 0.12 0.42
  }}
}}
Solid {{
  translation {gate_x + lateral_x * 0.58:.4f} {gate_z + lateral_z * 0.58:.4f} 0.210
  name "zone_{robot_name.lower()}_marker_right"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.92 0.76 0.34
        roughness 0.48
      }}
      geometry Box {{
        size 0.12 0.12 0.42
      }}
    }}
  ]
  boundingObject Box {{
    size 0.12 0.12 0.42
  }}
}}"""
            )

    return "\n".join(blocks) + ("\n" if blocks else "")


def build_environment_block(
    entries: List[tuple[str, str, str, float, str]],
    positions: List[Tuple[float, float]],
    room_half_size: float,
    include_fridge: bool,
) -> str:
    """Build a scene-aware demo hall around the active robot cluster."""

    center_x, center_z, scene_radius, span_x, span_z = scene_metrics(entries, positions)
    robot_kinds = {robot_kind for (_, _, _, _, robot_kind) in entries}
    has_nao = bool(robot_kinds.intersection({"nao", "hexapod"}))

    floor_size = max(6.8, room_half_size * 2.0)
    arena_size = max(5.6, floor_size - 0.9)
    wall_height = 1.85 if has_nao else 1.55
    perimeter = floor_size * 0.5 - 0.42
    light_height = 2.65 if has_nao else 2.20
    showcase_x = max(2.8, min(floor_size - 1.4, span_x + 3.2))
    showcase_z = max(2.8, min(floor_size - 1.4, span_z + 3.4))
    inner_patch = max(4.6, min(floor_size - 1.8, scene_radius * 2.6 + 2.0))

    storage_block = ""
    if include_fridge:
        storage_x, storage_z = clamp_room_point(
            center_x + room_half_size * 0.62,
            center_z - room_half_size * 0.62,
            room_half_size,
            0.80,
        )
        storage_block = f"""Solid {{
  translation {storage_x:.4f} {storage_z:.4f} 0.95
  name "service_storage_wall"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.70 0.72 0.76
        roughness 0.36
        metalness 0.24
      }}
      geometry Box {{
        size 0.78 0.74 1.90
      }}
    }}
  ]
  boundingObject Box {{
    size 0.78 0.74 1.90
  }}
}}
"""

    zone_props = build_demo_zone_furniture(entries, positions, room_half_size)
    return f"""# Scene-aware demo hall with clear robot zones and low-profile staging.
RectangleArena {{
  floorSize {arena_size:.2f} {arena_size:.2f}
  wallHeight {wall_height:.2f}
  floorAppearance PBRAppearance {{
    baseColor 0.19 0.18 0.16
    roughness 0.97
    metalness 0.01
  }}
  wallAppearance PBRAppearance {{
    baseColor 0.31 0.28 0.24
    roughness 0.91
    metalness 0.01
  }}
}}
Solid {{
  translation {center_x:.4f} {center_z:.4f} 0.005
  name "ground_base"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.30 0.29 0.27
        roughness 0.94
      }}
      geometry Box {{
        size {floor_size:.2f} {floor_size:.2f} 0.010
      }}
    }}
  ]
  boundingObject Box {{
    size {floor_size:.2f} {floor_size:.2f} 0.010
  }}
}}
Transform {{
  translation {center_x:.4f} {center_z:.4f} 0.011
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.35 0.41 0.33
        roughness 0.86
      }}
      geometry Box {{
        size {inner_patch:.2f} {inner_patch:.2f} 0.002
      }}
    }}
  ]
}}
Transform {{
  translation {center_x:.4f} {center_z:.4f} 0.013
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.56 0.48 0.36
        roughness 0.72
        transparency 0.22
      }}
      geometry Box {{
        size {showcase_x:.2f} {showcase_z:.2f} 0.004
      }}
    }}
  ]
}}
Solid {{
  translation {center_x:.4f} {-perimeter:.4f} 0.065
  name "front_bench"
  children [ Shape {{ appearance PBRAppearance {{ baseColor 0.44 0.35 0.25 roughness 0.78 }} geometry Box {{ size {showcase_x * 0.88:.2f} 0.24 0.13 }} }} ]
  boundingObject Box {{ size {showcase_x * 0.88:.2f} 0.24 0.13 }}
}}
Solid {{
  translation {center_x:.4f} {perimeter:.4f} 0.065
  name "rear_bench"
  children [ Shape {{ appearance PBRAppearance {{ baseColor 0.44 0.35 0.25 roughness 0.78 }} geometry Box {{ size {showcase_x * 0.88:.2f} 0.24 0.13 }} }} ]
  boundingObject Box {{ size {showcase_x * 0.88:.2f} 0.24 0.13 }}
}}
PointLight {{
  location {center_x + max(1.4, scene_radius * 0.55):.4f} {center_z + max(1.2, scene_radius * 0.42):.4f} {light_height:.2f}
  color 1.00 0.93 0.82
  intensity 11.0
  attenuation 0 0 1.80
  radius {max(7.0, scene_radius * 2.8):.2f}
  castShadows FALSE
}}
PointLight {{
  location {center_x - max(1.4, scene_radius * 0.55):.4f} {center_z - max(1.2, scene_radius * 0.42):.4f} {light_height - 0.18:.2f}
  color 0.82 0.91 1.00
  intensity 9.0
  attenuation 0 0 2.10
  radius {max(6.5, scene_radius * 2.5):.2f}
  castShadows FALSE
}}
{storage_block}{zone_props}"""



def main() -> None:
    parser = argparse.ArgumentParser(description="Generate mixed Webots world for multi-robot AARNN runs.")
    parser.add_argument("--world", required=True, help="Output world file")
    parser.add_argument("--celegans-proto", default="", help="Path to CelegansRobot.proto")
    parser.add_argument("--drosophila-banc-proto", default="", help="Path to DrosophilaBancRobot.proto")
    parser.add_argument("--drosophila-fafb-proto", default="", help="Path to DrosophilaFafbRobot.proto")
    parser.add_argument("--hexapod-proto", default="", help="Path to HexapodRobot.proto")
    parser.add_argument("--celegans-brains", default="", help="CSV brain IDs for celegans instances")
    parser.add_argument("--drosophila-banc-brains", default="", help="CSV brain IDs for BANC drosophila instances")
    parser.add_argument("--drosophila-fafb-brains", default="", help="CSV brain IDs for FAFB drosophila instances")
    parser.add_argument("--hexapod-brains", default="", help="CSV brain IDs for hexapod instances")
    parser.add_argument("--nao-brains", default="", help="CSV brain IDs for NAO instances")
    parser.add_argument(
        "--fridge",
        choices=["auto", "on", "off"],
        default=os.environ.get("NM_WORLD_FRIDGE", "auto"),
        help=(
            "Kitchen fridge policy. 'auto' enables fridge only for NAO-only worlds; "
            "'on' always includes it; 'off' always excludes it."
        ),
    )
    args = parser.parse_args()

    world_path = Path(args.world)
    world_path.parent.mkdir(parents=True, exist_ok=True)

    celegans_brains = parse_csv(args.celegans_brains)
    banc_brains = parse_csv(args.drosophila_banc_brains)
    fafb_brains = parse_csv(args.drosophila_fafb_brains)
    hexapod_brains = parse_csv(args.hexapod_brains)
    nao_brains = parse_csv(args.nao_brains)
    has_celegans = bool(celegans_brains)
    has_drosophila = bool(banc_brains or fafb_brains)
    has_hexapod = bool(hexapod_brains)
    has_nao = bool(nao_brains)
    if args.fridge == "on":
        include_fridge = True
    elif args.fridge == "off":
        include_fridge = False
    else:
        # Avoid articulated heavy fixtures in mixed/tiny-body scenes where they
        # can trigger large mass-ratio instability warnings.
        include_fridge = has_nao and not (has_celegans or has_drosophila or has_hexapod)

    total = (
        len(celegans_brains)
        + len(banc_brains)
        + len(fafb_brains)
        + len(hexapod_brains)
        + len(nao_brains)
    )
    if total <= 0:
        raise SystemExit("At least one robot brain must be provided.")

    extern_lines = [
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackground.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackgroundLight.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/floors/protos/RectangleArena.proto"',
    ]

    entries: List[tuple[str, str, str, float, str]] = []
    for i, brain in enumerate(celegans_brains, 1):
        entries.append(("CelegansRobot", f"C_ELEGANS_{i:02d}", brain, 0.032, "celegans"))
    if celegans_brains:
        if not args.celegans_proto:
            raise SystemExit("celegans brains are set but --celegans-proto is missing.")
        celegans_ref = rel_proto_ref(world_path, Path(args.celegans_proto))
        extern_lines.append(f'EXTERNPROTO "{celegans_ref}"')

    for i, brain in enumerate(banc_brains, 1):
        entries.append(("DrosophilaBancRobot", f"DROS_BANC_{i:02d}", brain, 0.044, "drosophila"))
    if banc_brains:
        if not args.drosophila_banc_proto:
            raise SystemExit("banc brains are set but --drosophila-banc-proto is missing.")
        banc_ref = rel_proto_ref(world_path, Path(args.drosophila_banc_proto))
        extern_lines.append(f'EXTERNPROTO "{banc_ref}"')

    for i, brain in enumerate(fafb_brains, 1):
        entries.append(("DrosophilaFafbRobot", f"DROS_FAFB_{i:02d}", brain, 0.044, "drosophila"))
    if fafb_brains:
        if not args.drosophila_fafb_proto:
            raise SystemExit("fafb brains are set but --drosophila-fafb-proto is missing.")
        fafb_ref = rel_proto_ref(world_path, Path(args.drosophila_fafb_proto))
        extern_lines.append(f'EXTERNPROTO "{fafb_ref}"')

    for i, brain in enumerate(hexapod_brains, 1):
        entries.append(("HexapodRobot", f"HEXAPOD_{i:02d}", brain, 0.190, "hexapod"))
    if hexapod_brains:
        if not args.hexapod_proto:
            raise SystemExit("hexapod brains are set but --hexapod-proto is missing.")
        hexapod_ref = rel_proto_ref(world_path, Path(args.hexapod_proto))
        extern_lines.append(f'EXTERNPROTO "{hexapod_ref}"')

    for i, brain in enumerate(nao_brains, 1):
        entries.append(("Nao", f"NAO_{i:02d}", brain, 0.34, "nao"))
    if nao_brains:
        extern_lines.append(f'EXTERNPROTO "{NAO_EXTERNPROTO}"')

    positions, keepout_radius, arena_half_size = layout_positions(entries)
    center_x, center_z, _scene_radius, _span_x, _span_z = scene_metrics(entries, positions)

    robot_nodes = []
    for (proto_name, robot_name, brain_id, height, robot_kind), (x, z) in zip(entries, positions):
        yaw = yaw_toward_center(x, z, center_x, center_z)
        controller_args = [f"NM_BRAINS={brain_id}"]
        if robot_kind == "drosophila":
            controller_args.append(f"NM_SENSORS_{brain_id}={DROS_SENSORS_REGEX}")
            controller_args.append(f"NM_ACTUATORS_{brain_id}={DROS_ACTUATORS_REGEX}")
        elif robot_kind == "celegans":
            controller_args.append(f"NM_SENSORS_{brain_id}={CELEGANS_SENSORS_REGEX}")
            controller_args.append(f"NM_ACTUATORS_{brain_id}={CELEGANS_ACTUATORS_REGEX}")
        elif robot_kind == "hexapod":
            controller_args.append(f"NM_SENSORS_{brain_id}={HEXAPOD_SENSORS_REGEX}")
            controller_args.append(f"NM_ACTUATORS_{brain_id}={HEXAPOD_ACTUATORS_REGEX}")
        robot_nodes.append(robot_node(proto_name, robot_name, brain_id, x, height, z, robot_kind, yaw, controller_args))

    cam_fov, cam_x, cam_y, cam_z = startup_camera(entries, positions)
    target_height = scene_target_height(entries)
    cam_axis_x, cam_axis_y, cam_axis_z, cam_angle = viewpoint_orientation(
        cam_x,
        cam_y,
        cam_z,
        center_x,
        center_z,
        target_height,
    )
    stimuli_nodes = build_robot_stimuli(entries, positions, arena_half_size)

    world = f"""#VRML_SIM R2025a utf8

{os.linesep.join(extern_lines)}

WorldInfo {{
  # Slightly larger time step to keep Webots responsive with articulated
  # multi-robot scenes and clustered controller traffic.
  basicTimeStep 32
  # Explicit gravity magnitude keeps Webots' standard -Z downward acceleration
  # active for loose props, articulated robots, and contact responses.
  gravity 9.81
}}
Viewpoint {{
  fieldOfView {cam_fov:.2f}
  # Zero-roll camera solved from eye->scene-center with world +Z as "up".
  orientation {cam_axis_x:.6f} {cam_axis_y:.6f} {cam_axis_z:.6f} {cam_angle:.6f}
  position {cam_x:.2f} {cam_y:.2f} {cam_z:.2f}
}}
TexturedBackground {{
  texture "noon_cloudy_countryside"
  luminosity 1.0
}}
TexturedBackgroundLight {{
  texture "noon_cloudy_countryside"
  luminosity 0.62
}}
# Soft directional key light — no visible glare orb, casts crisp shadows.
DirectionalLight {{
  color 1.0 0.96 0.88
  direction -0.38 -0.28 -0.88
  intensity 1.05
  castShadows TRUE
}}
{build_environment_block(entries, positions, arena_half_size, include_fridge)}
{stimuli_nodes}
{''.join(robot_nodes)}
"""
    world_path.write_text(world, encoding="utf-8")
    wbproj_path = world_path.with_name(f".{world_path.stem}.wbproj")
    if wbproj_path.exists():
        wbproj_path.unlink()
    print(
        f"Wrote {world_path} with {len(entries)} robots "
        f"(celegans={len(celegans_brains)}, banc={len(banc_brains)}, "
        f"fafb={len(fafb_brains)}, hexapod={len(hexapod_brains)}, nao={len(nao_brains)})"
    )


if __name__ == "__main__":
    main()
