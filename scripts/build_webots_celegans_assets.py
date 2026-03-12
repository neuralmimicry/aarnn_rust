#!/usr/bin/env python3
"""
Generate Webots assets for the C. elegans connectome snapshot.

Outputs:
  - webots_world/protos/CelegansRobot.proto
  - webots_world/worlds/celegans_neuroworld.wbt
  - webots_world/configs/config_celegans_webots.json
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any


def sanitize_def(name: str) -> str:
    out = []
    for ch in name:
        if ch.isalnum():
            out.append(ch.upper())
        else:
            out.append("_")
    s = "".join(out).strip("_")
    if not s:
        s = "NODE"
    if s[0].isdigit():
        s = f"N_{s}"
    return s


def build_motor_block(index: int, motor_name: str, total: int) -> str:
    # Spread joints along the worm body with small lateral oscillation.
    length = 0.34
    x = -length / 2.0 + (length * index / max(1, total - 1))
    z = 0.018 if index % 2 == 0 else -0.018
    def_name = sanitize_def(f"joint_{index}_{motor_name}")
    end_name = sanitize_def(f"segment_{index}_{motor_name}")
    return f"""    DEF {def_name} HingeJoint {{
      jointParameters HingeJointParameters {{
        anchor {x:.5f} 0.0 {z:.5f}
        axis 0 1 0
      }}
      device [
        RotationalMotor {{
          name "{motor_name}"
          maxVelocity 8
          minPosition -1.3
          maxPosition 1.3
        }}
      ]
      endPoint DEF {end_name} Solid {{
        name "{end_name}"
        translation {x:.5f} 0.0 {z:.5f}
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.78 0.46 0.29
              roughness 0.45
              metalness 0
            }}
            geometry Sphere {{
              radius 0.006
            }}
          }}
        ]
        boundingObject Sphere {{
          radius 0.006
        }}
        physics Physics {{
          density -1
          mass 0.001
        }}
      }}
    }}
"""


def build_sensor_block() -> str:
    distance_specs = [
        # name, tx, tz, yaw, max_range
        ("celegans_front", 0.19, 0.00, 0.0, 1.4),
        ("celegans_front_left", 0.15, 0.08, 0.55, 1.3),
        ("celegans_front_right", 0.15, -0.08, -0.55, 1.3),
        ("celegans_front_far_left", 0.10, 0.11, 1.05, 1.1),
        ("celegans_front_far_right", 0.10, -0.11, -1.05, 1.1),
        ("celegans_left", 0.00, 0.12, 1.5708, 1.0),
        ("celegans_right", 0.00, -0.12, -1.5708, 1.0),
        ("celegans_rear_left", -0.11, 0.08, 2.6, 1.1),
        ("celegans_rear_right", -0.11, -0.08, -2.6, 1.1),
        ("celegans_rear_far_left", -0.15, 0.06, 2.2, 1.2),
        ("celegans_rear_far_right", -0.15, -0.06, -2.2, 1.2),
        ("celegans_rear", -0.19, 0.00, 3.14159, 1.4),
    ]
    distance_blocks = []
    for name, tx, tz, yaw, max_range in distance_specs:
        rotation_line = ""
        if abs(yaw) > 1e-6:
            rotation_line = f"\n        rotation 0 1 0 {yaw:.5f}"
        distance_blocks.append(
            f"""      DistanceSensor {{
        name "{name}"
        type "infra-red"
        translation {tx:.5f} 0.0 {tz:.5f}{rotation_line}
        lookupTable [
          0 1000 0
          {max_range:.2f} 0 0
        ]
      }}"""
        )

    return f"""      # Inertial + contact sensing
      Accelerometer {{
        name "celegans_accel"
      }}
      Gyro {{
        name "celegans_gyro"
      }}
      TouchSensor {{
        name "celegans_bumper_front"
        type "bumper"
        translation 0.19 0.0 0.0
      }}
      TouchSensor {{
        name "celegans_bumper_rear"
        type "bumper"
        translation -0.19 0.0 0.0
      }}
      # Multi-directional proximity sensing to create rich directional gradients.
{chr(10).join(distance_blocks)}
"""


def generate_proto(
    output_nodes: list[str],
    proto_path: Path,
    default_network_file: str,
    default_config_file: str,
) -> None:
    motor_blocks = "".join(
        build_motor_block(i, name, len(output_nodes)) for i, name in enumerate(output_nodes)
    )
    sensor_block = build_sensor_block()
    proto = f"""#VRML_SIM R2025a utf8

PROTO CelegansRobot [
  field SFVec3f     translation 0 0.03 0
  field SFRotation  rotation 0 1 0 0
  field SFString    name "celegans_robot"
  field SFString    controller "nao_nn_controller_uds"
  field MFString    controllerArgs [
    "NM_BRAINS=default"
  ]
  field MFNode      extensionSlot [ ]
]
{{
  Robot {{
    translation IS translation
    rotation IS rotation
    name IS name
    controller IS controller
    controllerArgs IS controllerArgs
    customData "{{\\"network\\":\\"{default_network_file}\\",\\"config\\":\\"{default_config_file}\\"}}"
    children [
      Shape {{
        appearance PBRAppearance {{
          baseColor 0.88 0.63 0.43
          roughness 0.75
          metalness 0
        }}
        geometry Capsule {{
          radius 0.028
          height 0.34
        }}
      }}
{sensor_block}
{motor_blocks}
      Group {{
        children IS extensionSlot
      }}
    ]
    boundingObject Capsule {{
      radius 0.03
      height 0.34
    }}
    physics Physics {{
      density -1
      mass 0.25
    }}
  }}
}}
"""
    proto_path.write_text(proto, encoding="utf-8")


def generate_world(world_path: Path) -> None:
    world = """#VRML_SIM R2025a utf8

EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackground.proto"
EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackgroundLight.proto"
EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/floors/protos/RectangleArena.proto"
EXTERNPROTO "../protos/CelegansRobot.proto"

WorldInfo {
}
Viewpoint {
  orientation 0.01305170466665628 -0.9999027911309566 0.005232327632668971 4.698053854211715
  position 0.0010716134103770597 1.2199874709707863 1.9439949804831262
}
TexturedBackground {
}
TexturedBackgroundLight {
}
RectangleArena {
  floorSize 10 10
  wallHeight 0.35
}
# Static landmarks and corridors.
Solid {
  translation 0.0 0.08 0.0
  name "pillar_center"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.83 0.37 0.24
        roughness 0.5
      }
      geometry Cylinder {
        radius 0.22
        height 0.16
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.22
    height 0.16
  }
}
Solid {
  translation 0.0 0.06 0.55
  name "near_pillar_north"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.93 0.76 0.36
        roughness 0.5
      }
      geometry Cylinder {
        radius 0.08
        height 0.12
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.08
    height 0.12
  }
}
Solid {
  translation 0.0 0.06 -0.55
  name "near_pillar_south"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.93 0.76 0.36
        roughness 0.5
      }
      geometry Cylinder {
        radius 0.08
        height 0.12
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.08
    height 0.12
  }
}
Solid {
  translation 0.55 0.06 0.0
  name "near_pillar_east"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.93 0.76 0.36
        roughness 0.5
      }
      geometry Cylinder {
        radius 0.08
        height 0.12
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.08
    height 0.12
  }
}
Solid {
  translation -0.55 0.06 0.0
  name "near_pillar_west"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.93 0.76 0.36
        roughness 0.5
      }
      geometry Cylinder {
        radius 0.08
        height 0.12
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.08
    height 0.12
  }
}
Solid {
  translation 2.1 0.09 1.1
  rotation 0 1 0 0.42
  name "maze_wall_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.27 0.62 0.36
        roughness 0.6
      }
      geometry Box {
        size 2.8 0.18 0.2
      }
    }
  ]
  boundingObject Box {
    size 2.8 0.18 0.2
  }
}
Solid {
  translation -2.2 0.09 -1.3
  rotation 0 1 0 -0.62
  name "maze_wall_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.21 0.51 0.74
        roughness 0.55
      }
      geometry Box {
        size 2.9 0.18 0.2
      }
    }
  ]
  boundingObject Box {
    size 2.9 0.18 0.2
  }
}
Solid {
  translation 0.0 0.09 2.6
  rotation 0 1 0 1.12
  name "maze_wall_c"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.78 0.66 0.31
        roughness 0.6
      }
      geometry Box {
        size 2.4 0.18 0.2
      }
    }
  ]
  boundingObject Box {
    size 2.4 0.18 0.2
  }
}
Solid {
  translation -0.3 0.09 -2.4
  rotation 0 1 0 -1.05
  name "maze_wall_d"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.64 0.32 0.24
        roughness 0.56
      }
      geometry Box {
        size 2.0 0.18 0.2
      }
    }
  ]
  boundingObject Box {
    size 2.0 0.18 0.2
  }
}
Solid {
  translation 2.7 0.09 -0.3
  rotation 0 1 0 0.15
  name "maze_wall_e"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.41 0.49 0.83
        roughness 0.57
      }
      geometry Box {
        size 1.6 0.18 0.2
      }
    }
  ]
  boundingObject Box {
    size 1.6 0.18 0.2
  }
}
# Sloped platforms trigger IMU variation when climbed.
Solid {
  translation -2.7 0.11 2.2
  rotation 0 0 1 -0.25
  name "ramp_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.72 0.56 0.29
        roughness 0.6
      }
      geometry Box {
        size 1.2 0.18 0.8
      }
    }
  ]
  boundingObject Box {
    size 1.2 0.18 0.8
  }
}
Solid {
  translation 2.5 0.11 -2.1
  rotation 0 0 1 0.27
  name "ramp_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.52 0.62 0.29
        roughness 0.6
      }
      geometry Box {
        size 1.2 0.18 0.8
      }
    }
  ]
  boundingObject Box {
    size 1.2 0.18 0.8
  }
}
# Dynamic objects create continuously changing proximity/contact stimuli.
Solid {
  translation 1.4 0.06 -1.7
  linearVelocity 1.9 0 1.2
  angularVelocity 0 2.4 0
  name "dynamic_pellet_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.95 0.91 0.28
        roughness 0.3
      }
      geometry Sphere {
        radius 0.08
      }
    }
  ]
  boundingObject Sphere {
    radius 0.08
  }
  physics Physics {
    density -1
    mass 0.07
  }
}
Solid {
  translation -1.8 0.06 1.5
  linearVelocity -1.5 0 -1.9
  angularVelocity 0 1.8 0
  name "dynamic_pellet_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.88 0.42 0.18
        roughness 0.35
      }
      geometry Sphere {
        radius 0.09
      }
    }
  ]
  boundingObject Sphere {
    radius 0.09
  }
  physics Physics {
    density -1
    mass 0.08
  }
}
Solid {
  translation 2.2 0.07 0.1
  linearVelocity -1.8 0 0.9
  angularVelocity 0 1.5 0
  name "dynamic_pellet_c"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.34 0.86 0.63
        roughness 0.4
      }
      geometry Sphere {
        radius 0.1
      }
    }
  ]
  boundingObject Sphere {
    radius 0.1
  }
  physics Physics {
    density -1
    mass 0.09
  }
}
Solid {
  translation -0.9 0.06 -2.2
  linearVelocity 2.1 0 0.8
  angularVelocity 0 2.1 0
  name "dynamic_pellet_d"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.86 0.55 0.20
        roughness 0.37
      }
      geometry Sphere {
        radius 0.07
      }
    }
  ]
  boundingObject Sphere {
    radius 0.07
  }
  physics Physics {
    density -1
    mass 0.06
  }
}
Solid {
  translation 0.7 0.06 2.1
  linearVelocity -0.8 0 -2.0
  angularVelocity 0 2.6 0
  name "dynamic_pellet_e"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.28 0.79 0.86
        roughness 0.34
      }
      geometry Sphere {
        radius 0.085
      }
    }
  ]
  boundingObject Sphere {
    radius 0.085
  }
  physics Physics {
    density -1
    mass 0.07
  }
}
Solid {
  translation -2.4 0.06 -0.4
  linearVelocity 1.6 0 1.6
  angularVelocity 0 1.9 0
  name "dynamic_pellet_f"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.91 0.33 0.49
        roughness 0.35
      }
      geometry Sphere {
        radius 0.075
      }
    }
  ]
  boundingObject Sphere {
    radius 0.075
  }
  physics Physics {
    density -1
    mass 0.06
  }
}
Solid {
  translation 0.0 0.07 -1.1
  rotation 0 1 0 0.6
  linearVelocity 0.7 0 1.4
  angularVelocity 0.3 3.1 0.1
  name "dynamic_bar_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.40 0.74 0.34
        roughness 0.42
      }
      geometry Box {
        size 0.7 0.14 0.18
      }
    }
  ]
  boundingObject Box {
    size 0.7 0.14 0.18
  }
  physics Physics {
    density -1
    mass 0.12
  }
}
Solid {
  translation -0.2 0.07 1.1
  rotation 0 1 0 -0.55
  linearVelocity -0.9 0 -1.2
  angularVelocity -0.2 2.9 -0.1
  name "dynamic_bar_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.78 0.31 0.32
        roughness 0.41
      }
      geometry Box {
        size 0.75 0.14 0.18
      }
    }
  ]
  boundingObject Box {
    size 0.75 0.14 0.18
  }
  physics Physics {
    density -1
    mass 0.13
  }
}
CelegansRobot {
  translation -3.4 0.032 -3.2
  name "C_Elegans"
  controller "nao_nn_controller_uds"
  controllerArgs [
    "NM_BRAINS=default"
  ]
}
"""
    world_path.write_text(world, encoding="utf-8")


def build_webots_config(snapshot: dict[str, Any], config_path: Path) -> None:
    net = dict(snapshot.get("net", {}))
    # 12 distance sensors + accel(3) + gyro(3) + bumpers(2)
    sensory_target = 20
    net["num_sensory_neurons"] = max(sensory_target, int(net.get("num_sensory_neurons", 0)))
    net["num_output_neurons"] = max(1, int(net.get("num_output_neurons", 0)))
    net["growth_enabled"] = False
    net["morpho_growth_enabled"] = False
    net["sleep_enabled"] = False
    config_path.write_text(json.dumps(net, indent=2), encoding="utf-8")


def output_nodes_from_snapshot(snapshot: dict[str, Any]) -> list[str]:
    labels = snapshot.get("connectome_labels")
    if isinstance(labels, dict):
        names = labels.get("output_nodes")
        if isinstance(names, list) and names:
            return [str(name) for name in names]

    net = snapshot.get("net", {})
    count = int(net.get("num_output_neurons", 0) or 0)
    count = max(1, count)
    return [f"muscle_{idx:03d}" for idx in range(count)]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate Webots C. elegans robot/world/config assets.")
    parser.add_argument(
        "--network",
        default="network_celegans.json",
        help="Path to the connectome snapshot JSON (default: network_celegans.json)",
    )
    parser.add_argument(
        "--proto",
        default="webots_world/protos/CelegansRobot.proto",
        help="Output PROTO path",
    )
    parser.add_argument(
        "--world",
        default="webots_world/worlds/celegans_neuroworld.wbt",
        help="Output world path",
    )
    parser.add_argument(
        "--config",
        default="webots_world/configs/config_celegans_webots.json",
        help="Output config path",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    network_path = Path(args.network)
    proto_path = Path(args.proto)
    world_path = Path(args.world)
    config_path = Path(args.config)

    if not network_path.exists():
        raise SystemExit(f"Missing network snapshot: {network_path}")

    snapshot = json.loads(network_path.read_text(encoding="utf-8"))
    output_nodes = output_nodes_from_snapshot(snapshot)

    proto_path.parent.mkdir(parents=True, exist_ok=True)
    world_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.parent.mkdir(parents=True, exist_ok=True)

    default_network = os.path.relpath(network_path.resolve(), proto_path.parent.resolve()).replace(
        "\\", "/"
    )
    default_config = "../configs/config_celegans_webots.json"
    generate_proto(output_nodes, proto_path, default_network, default_config)
    generate_world(world_path)
    build_webots_config(snapshot, config_path)

    print(
        f"Wrote {proto_path} and {world_path} with {len(output_nodes)} motor outputs; "
        f"config at {config_path}"
    )


if __name__ == "__main__":
    main()
