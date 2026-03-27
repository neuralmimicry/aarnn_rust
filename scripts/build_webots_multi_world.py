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


def parse_csv(value: str) -> List[str]:
    out: List[str] = []
    for item in value.split(","):
        token = item.strip()
        if token:
            out.append(token)
    return out


def rel_proto_ref(world_path: Path, proto_path: Path) -> str:
    return os.path.relpath(proto_path.resolve(), world_path.parent.resolve()).replace("\\", "/")


def layout_positions(count: int, *, has_nao: bool) -> Tuple[List[Tuple[float, float]], float, float]:
    """Return robot positions plus center keep-out radius and half arena size."""
    if count <= 0:
        return [], 5.0, 17.0

    cols = max(1, int(math.ceil(math.sqrt(count))))
    rows = int(math.ceil(count / float(cols)))
    spacing = 3.4 if has_nao else 2.5

    x_offset = (cols - 1) * spacing * 0.5
    z_offset = (rows - 1) * spacing * 0.5
    out: List[Tuple[float, float]] = []
    for i in range(count):
        row = i // cols
        col = i % cols
        out.append((col * spacing - x_offset, row * spacing - z_offset))

    keepout_radius = max(x_offset, z_offset) + spacing * 0.95
    arena_half_size = max(17.0, keepout_radius + 8.0)
    return out, keepout_radius, arena_half_size


def yaw_toward_center(x: float, z: float, idx: int) -> float:
    if abs(x) < 1e-6 and abs(z) < 1e-6:
        return 0.0
    jitter = ((idx % 5) - 2) * 0.06
    return math.atan2(-x, -z) + jitter


def clamp(value: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, value))


def robot_node(
    proto_name: str,
    name: str,
    brain_id: str,
    x: float,
    y: float,
    z: float,
    yaw: float,
    controller_args: List[str],
) -> str:
    args_lines = "\n".join(f'    "{arg}"' for arg in controller_args)
    return f"""{proto_name} {{
  translation {x:.4f} {y:.4f} {z:.4f}
  rotation 0 1 0 {yaw:.5f}
  name "{name}"
  controller "nao_nn_controller_uds"
  controllerArgs [
{args_lines}
  ]
}}
"""


def build_robot_stimuli(
    entries: List[tuple[str, str, str, float, str]],
    positions: List[Tuple[float, float]],
    arena_half_size: float,
) -> str:
    """
    Build local, non-overlapping sensory stimulus geometry around each robot spawn.

    Goal: ensure proximity/contact sensors see nearby structure immediately on start,
    rather than only distant arena boundaries.
    """
    blocks: List[str] = []
    bound = max(0.8, arena_half_size - 0.8)

    for idx, ((_, robot_name, _, _, robot_kind), (x, z)) in enumerate(zip(entries, positions), 1):
        if robot_kind == "drosophila":
            ring_r = 0.085
            post_radius = 0.018
            post_height = 0.12
            center_height = 0.055
            center_radius = 0.032
        elif robot_kind == "celegans":
            ring_r = 0.24
            post_radius = 0.03
            post_height = 0.16
            center_height = 0.08
            center_radius = 0.052
        else:  # nao
            ring_r = 0.46
            post_radius = 0.055
            post_height = 0.62
            center_height = 0.24
            center_radius = 0.11

        # Deterministic rotation so clusters are varied but reproducible.
        phase = 0.38 * idx
        color_a = (0.90, 0.34, 0.17)
        color_b = (0.90, 0.66, 0.15)
        color_c = (0.72, 0.21, 0.13)
        color_d = (0.36, 0.64, 0.20)
        post_colors = [color_a, color_b, color_c, color_d]

        for k in range(4):
            ang = phase + (math.pi * 0.5 * k)
            px = clamp(x + ring_r * math.cos(ang), -bound, bound)
            pz = clamp(z + ring_r * math.sin(ang), -bound, bound)
            r, g, b = post_colors[k]
            blocks.append(
                f"""Solid {{
  translation {px:.4f} {post_height * 0.5:.4f} {pz:.4f}
  name "stim_{robot_name.lower()}_{idx:02d}_{k:02d}"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor {r:.2f} {g:.2f} {b:.2f}
        roughness 0.52
      }}
      geometry Cylinder {{
        radius {post_radius:.4f}
        height {post_height:.4f}
      }}
    }}
  ]
  boundingObject Cylinder {{
    radius {post_radius:.4f}
    height {post_height:.4f}
  }}
}}"""
            )

        # Central attractant marker to provide a strong nearby target for
        # short-range proximity rings (not touching spawn position).
        marker_dx = 0.58 * ring_r * math.cos(phase + 0.35)
        marker_dz = 0.58 * ring_r * math.sin(phase + 0.35)
        mx = clamp(x + marker_dx, -bound, bound)
        mz = clamp(z + marker_dz, -bound, bound)
        blocks.append(
            f"""Solid {{
  translation {mx:.4f} {center_height:.4f} {mz:.4f}
  name "stim_{robot_name.lower()}_{idx:02d}_core"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.97 0.82 0.31
        roughness 0.35
      }}
      geometry Sphere {{
        radius {center_radius:.4f}
      }}
    }}
  ]
  boundingObject Sphere {{
    radius {center_radius:.4f}
  }}
}}"""
        )

        if robot_kind == "celegans":
            # Localized light/heat gradient for phototaxis + thermotaxis channels.
            warm_x = clamp(x + 0.28, -bound, bound)
            warm_z = clamp(z - 0.22, -bound, bound)
            cool_x = clamp(x - 0.26, -bound, bound)
            cool_z = clamp(z + 0.24, -bound, bound)
            blocks.append(
                f"""PointLight {{
  location {warm_x:.4f} 0.32 {warm_z:.4f}
  color 1.00 0.58 0.25
  intensity 2.7
  attenuation 0 0 0.2770
  radius 1.9
  castShadows FALSE
}}
PointLight {{
  location {cool_x:.4f} 0.30 {cool_z:.4f}
  color 0.74 0.88 1.00
  intensity 2.1
  attenuation 0 0 0.3460
  radius 1.7
  castShadows FALSE
}}"""
            )

            # Taste/chemical attractants near head-level sensing arcs.
            taste_r = 0.032
            for t_idx, (dx, dz, cr, cg, cb) in enumerate(
                (
                    (0.20, 0.00, 0.92, 0.74, 0.28),
                    (0.18, 0.09, 0.82, 0.56, 0.23),
                    (0.18, -0.09, 0.78, 0.50, 0.22),
                ),
                1,
            ):
                tx = clamp(x + dx, -bound, bound)
                tz = clamp(z + dz, -bound, bound)
                blocks.append(
                    f"""Solid {{
  translation {tx:.4f} 0.022 {tz:.4f}
  name "stim_{robot_name.lower()}_taste_{t_idx:02d}"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor {cr:.2f} {cg:.2f} {cb:.2f}
        roughness 0.45
      }}
      geometry Sphere {{
        radius {taste_r:.4f}
      }}
    }}
  ]
  boundingObject Sphere {{
    radius {taste_r:.4f}
  }}
}}"""
                )

            # Moving pellet to provide changing proximity/contact/vibration cues.
            vib_x = clamp(x - 0.36, -bound, bound)
            vib_z = clamp(z + 0.16, -bound, bound)
            blocks.append(
                f"""Solid {{
  translation {vib_x:.4f} 0.058 {vib_z:.4f}
  linearVelocity 1.35 0 -0.95
  angularVelocity 0 2.2 0
  name "stim_{robot_name.lower()}_vibration_pellet"
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.90 0.84 0.33
        roughness 0.36
      }}
      geometry Sphere {{
        radius 0.058
      }}
    }}
  ]
  boundingObject Sphere {{
    radius 0.058
  }}
  physics Physics {{
    density -1
    mass 0.045
  }}
}}"""
            )

    return "\n".join(blocks) + ("\n" if blocks else "")


def build_environment_block(
    keepout_radius: float,
    arena_half_size: float,
    has_celegans: bool,
    has_drosophila: bool,
    has_nao: bool,
    include_fridge: bool,
) -> str:
    """Build a kitchen-inspired environment adapted from Webots kitchen.wbt sample."""

    # Add robot-specific environmental features
    robot_features = []

    # C. elegans: Water features (sink/bowl areas)
    if has_celegans:
        robot_features.append("""
SolidBox {
  translation 2.5 -2 0.01
  name "water_dish_celegans"
  size 0.8 0.8 0.02
  appearance PBRAppearance {
    baseColor 0.3 0.5 0.7
    roughness 0.1
    metalness 0.05
    transparency 0.6
  }
}""")

    # Drosophila: Fruit bowl and elevated perches
    if has_drosophila:
        robot_features.append("""
SolidBox {
  translation -2 2 0.5
  name "perch_platform"
  size 0.3 0.3 1.0
  appearance PBRAppearance {
    baseColor 0.4 0.3 0.2
    roughness 0.7
  }
}""")

    # NAO: Open floor space already provided by kitchen
    if has_nao:
        robot_features.append("""
SolidBox {
  translation 1 1 0.02
  name "nao_platform"
  size 1.5 1.5 0.04
  appearance PBRAppearance {
    baseColor 0.6 0.55 0.45
    roughness 0.5
  }
}""")

    fridge_block = ""
    if include_fridge:
        fridge_block = """Fridge {
  translation 3.4 2.5 0
  rotation 0 0 1 -1.5708
}
"""

    return f"""# Kitchen environment (inspired by Webots kitchen sample)
Floor {{
  translation 0 0 0
  size 8 8
  tileSize 1 1
  appearance Parquetry {{
  }}
}}
Ceiling {{
  translation 0 0 2.4
  size 8 8
}}
Wall {{
  translation 3.9 0 0
  name "wall_east"
  size 0.2 8 2.4
}}
Wall {{
  translation -3.9 0 0
  name "wall_west"
  size 0.2 8 2.4
}}
Wall {{
  translation 0 3.9 0
  rotation 0 0 1 1.5708
  name "wall_north"
  size 0.2 8 2.4
}}
Wall {{
  translation 0 -3.9 0
  rotation 0 0 1 1.5708
  name "wall_south"
  size 0.2 8 2.4
}}
CeilingLight {{
  translation 0 0 2.4
  pointLightIntensity 3
}}
Table {{
  translation -1.5 -1.5 0
  size 1.2 1.2 0.74
}}
FruitBowl {{
  translation -1.5 -1.5 0.78
}}
{fridge_block}SolidBox {{
  translation 2.5 3.2 0.4
  name "counter"
  size 2.5 0.6 0.8
  appearance PBRAppearance {{
    baseColor 0.4 0.4 0.4
    roughness 0.5
  }}
}}
{''.join(robot_features)}
"""



def main() -> None:
    parser = argparse.ArgumentParser(description="Generate mixed Webots world for multi-robot AARNN runs.")
    parser.add_argument("--world", required=True, help="Output world file")
    parser.add_argument("--celegans-proto", default="", help="Path to CelegansRobot.proto")
    parser.add_argument("--drosophila-banc-proto", default="", help="Path to DrosophilaBancRobot.proto")
    parser.add_argument("--drosophila-fafb-proto", default="", help="Path to DrosophilaFafbRobot.proto")
    parser.add_argument("--celegans-brains", default="", help="CSV brain IDs for celegans instances")
    parser.add_argument("--drosophila-banc-brains", default="", help="CSV brain IDs for BANC drosophila instances")
    parser.add_argument("--drosophila-fafb-brains", default="", help="CSV brain IDs for FAFB drosophila instances")
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
    nao_brains = parse_csv(args.nao_brains)
    has_celegans = bool(celegans_brains)
    has_drosophila = bool(banc_brains or fafb_brains)
    has_nao = bool(nao_brains)
    if args.fridge == "on":
        include_fridge = True
    elif args.fridge == "off":
        include_fridge = False
    else:
        # Avoid articulated heavy fixtures in mixed/tiny-body scenes where they
        # can trigger large mass-ratio instability warnings.
        include_fridge = has_nao and not (has_celegans or has_drosophila)

    total = len(celegans_brains) + len(banc_brains) + len(fafb_brains) + len(nao_brains)
    if total <= 0:
        raise SystemExit("At least one robot brain must be provided.")

    extern_lines = [
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackground.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackgroundLight.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/floors/protos/Floor.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/appearances/protos/Parquetry.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/apartment_structure/protos/Wall.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/apartment_structure/protos/Ceiling.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/tables/protos/Table.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/fruits/protos/FruitBowl.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/solids/protos/SolidBox.proto"',
        'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/lights/protos/CeilingLight.proto"',
    ]
    if include_fridge:
        extern_lines.append(
            'EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/kitchen/fridge/protos/Fridge.proto"'
        )

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

    for i, brain in enumerate(nao_brains, 1):
        entries.append(("Nao", f"NAO_{i:02d}", brain, 0.34, "nao"))
    if nao_brains:
        extern_lines.append(f'EXTERNPROTO "{NAO_EXTERNPROTO}"')

    positions, keepout_radius, arena_half_size = layout_positions(len(entries), has_nao=bool(nao_brains))

    robot_nodes = []
    for i, ((proto_name, robot_name, brain_id, y, robot_kind), (x, z)) in enumerate(zip(entries, positions)):
        yaw = yaw_toward_center(x, z, i)
        controller_args = [f"NM_BRAINS={brain_id}"]
        if robot_kind == "drosophila":
            controller_args.append(f"NM_SENSORS_{brain_id}={DROS_SENSORS_REGEX}")
            controller_args.append(f"NM_ACTUATORS_{brain_id}={DROS_ACTUATORS_REGEX}")
        elif robot_kind == "celegans":
            controller_args.append(f"NM_SENSORS_{brain_id}={CELEGANS_SENSORS_REGEX}")
            controller_args.append(f"NM_ACTUATORS_{brain_id}={CELEGANS_ACTUATORS_REGEX}")
        robot_nodes.append(robot_node(proto_name, robot_name, brain_id, x, y, z, yaw, controller_args))

    cam_y = max(10.5, arena_half_size * 0.90)
    cam_z = max(12.0, arena_half_size * 1.05)
    stimuli_nodes = build_robot_stimuli(entries, positions, arena_half_size)

    world = f"""#VRML_SIM R2025a utf8

{os.linesep.join(extern_lines)}

WorldInfo {{
  # Slightly larger time step to keep Webots responsive with articulated
  # multi-robot scenes and clustered controller traffic.
  basicTimeStep 32
}}
Viewpoint {{
  fieldOfView 0.85
  orientation -0.5773 0.5773 0.5773 2.0944
  position {cam_z * 0.4:.2f} {cam_y:.2f} {cam_z * 0.4:.2f}
}}
TexturedBackground {{
}}
TexturedBackgroundLight {{
}}
{build_environment_block(keepout_radius, arena_half_size, has_celegans, has_drosophila, has_nao, include_fridge)}
{stimuli_nodes}
{''.join(robot_nodes)}
"""
    world_path.write_text(world, encoding="utf-8")
    print(
        f"Wrote {world_path} with {len(entries)} robots "
        f"(celegans={len(celegans_brains)}, banc={len(banc_brains)}, "
        f"fafb={len(fafb_brains)}, nao={len(nao_brains)})"
    )


if __name__ == "__main__":
    main()
