#!/usr/bin/env python3
"""
Generate Webots assets for a Drosophila connectome snapshot.

Outputs:
  - webots_world/protos/DrosophilaRobot.proto
  - webots_world/worlds/drosophila_neuroworld.wbt
  - webots_world/configs/config_drosophila_webots.json
"""

from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
from typing import Any


DISTANCE_SENSOR_COUNT = 24
TOUCH_SENSOR_CHANNELS = 4
IMU_SENSOR_CHANNELS = 6  # accel(3) + gyro(3)
TOTAL_SENSOR_CHANNELS = DISTANCE_SENSOR_COUNT + TOUCH_SENSOR_CHANNELS + IMU_SENSOR_CHANNELS


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
    # Arrange effectors in concentric rings around the thorax so large output
    # vectors remain physically represented without making an ultra-long body.
    rings = max(1, min(4, (total + 15) // 16))
    per_ring = max(1, math.ceil(total / rings))
    ring_idx = min(rings - 1, index // per_ring)
    in_ring_idx = index % per_ring
    theta = (2.0 * math.pi * in_ring_idx) / float(per_ring)
    radius = 0.013 + 0.0035 * ring_idx
    x = radius * math.cos(theta)
    z = radius * math.sin(theta)
    y = -0.0015 + 0.0012 * ((index % 3) - 1)

    def_name = sanitize_def(f"joint_{index}_{motor_name}")
    end_name = sanitize_def(f"segment_{index}_{motor_name}")
    return f"""    DEF {def_name} HingeJoint {{
      jointParameters HingeJointParameters {{
        anchor {x:.5f} {y:.5f} {z:.5f}
        axis 0 1 0
      }}
      device [
        RotationalMotor {{
          name "{motor_name}"
          maxVelocity 14
          minPosition -1.6
          maxPosition 1.6
        }}
      ]
      endPoint DEF {end_name} Solid {{
        name "{end_name}"
        translation {x:.5f} {y:.5f} {z:.5f}
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.28 0.22 0.18
              roughness 0.55
              metalness 0
            }}
            geometry Capsule {{
              radius 0.0015
              height 0.008
            }}
          }}
        ]
        boundingObject Capsule {{
          radius 0.0016
          height 0.008
        }}
        physics Physics {{
          density -1
          mass 0.00002
        }}
      }}
    }}
"""


def build_sensor_block() -> str:
    distance_blocks = []
    for i in range(DISTANCE_SENSOR_COUNT):
        yaw = (2.0 * math.pi * i) / float(DISTANCE_SENSOR_COUNT)
        tx = 0.0205 * math.cos(yaw)
        tz = 0.0205 * math.sin(yaw)
        distance_blocks.append(
            f"""      DistanceSensor {{
        name "fly_prox_{i:02d}"
        type "infra-red"
        translation {tx:.5f} 0.0015 {tz:.5f}
        rotation 0 1 0 {yaw:.5f}
        lookupTable [
          0 1000 0
          0.28 0 0
        ]
      }}"""
        )

    return f"""      # Inertial sensing
      Accelerometer {{
        name "fly_accel"
      }}
      Gyro {{
        name "fly_gyro"
      }}
      # Contact sensing around body perimeter
      TouchSensor {{
        name "fly_touch_front"
        type "bumper"
        translation 0.028 0.0 0.0
      }}
      TouchSensor {{
        name "fly_touch_rear"
        type "bumper"
        translation -0.028 0.0 0.0
      }}
      TouchSensor {{
        name "fly_touch_left"
        type "bumper"
        translation 0.0 0.0 0.020
      }}
      TouchSensor {{
        name "fly_touch_right"
        type "bumper"
        translation 0.0 0.0 -0.020
      }}
      # Directional proximity ring for odor/obstacle gradient response.
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

PROTO DrosophilaRobot [
  field SFVec3f     translation 0 0.016 0
  field SFRotation  rotation 0 1 0 0
  field SFString    name "drosophila_robot"
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
      # Body plan: head + thorax + abdomen
      Shape {{
        appearance PBRAppearance {{
          baseColor 0.35 0.24 0.12
          roughness 0.5
          metalness 0
        }}
        geometry Sphere {{
          radius 0.009
        }}
      }}
      Solid {{
        name "thorax_segment"
        translation -0.010 0 0
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.42 0.28 0.14
              roughness 0.45
            }}
            geometry Sphere {{
              radius 0.012
            }}
          }}
        ]
        boundingObject Sphere {{
          radius 0.012
        }}
      }}
      Solid {{
        name "abdomen_segment"
        translation -0.024 0 0
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.53 0.36 0.18
              roughness 0.48
            }}
            geometry Capsule {{
              radius 0.007
              height 0.018
            }}
          }}
        ]
        boundingObject Capsule {{
          radius 0.007
          height 0.018
        }}
      }}
{sensor_block}
{motor_blocks}
      Group {{
        children IS extensionSlot
      }}
    ]
    boundingObject Group {{
      children [
        Sphere {{
          radius 0.0125
        }}
        Transform {{
          translation -0.020 0 0
          children [
            Capsule {{
              radius 0.008
              height 0.022
            }}
          ]
        }}
      ]
    }}
    physics Physics {{
      density -1
      mass 0.008
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
EXTERNPROTO "../protos/DrosophilaRobot.proto"

WorldInfo {
}
Viewpoint {
  orientation 0.2413571298348519 -0.9568337613679344 0.16245018235004453 4.147536566868645
  position 0.05587760533114102 0.6488540704626902 1.0361880242305197
}
TexturedBackground {
}
TexturedBackgroundLight {
}
RectangleArena {
  floorSize 1.8 1.8
  wallHeight 0.08
}
# Fruit-scale terrain and clutter for rich sensory gradients.
Solid {
  translation 0 0.028 0
  name "fermenting_fruit_core"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.91 0.62 0.24
        roughness 0.43
      }
      geometry Cylinder {
        radius 0.12
        height 0.056
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.12
    height 0.056
  }
}
Solid {
  translation 0.24 0.019 0.20
  name "fruit_chunk_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.87 0.33 0.18
        roughness 0.38
      }
      geometry Sphere {
        radius 0.036
      }
    }
  ]
  boundingObject Sphere {
    radius 0.036
  }
}
Solid {
  translation -0.30 0.018 -0.16
  name "fruit_chunk_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.79 0.20 0.17
        roughness 0.36
      }
      geometry Sphere {
        radius 0.032
      }
    }
  ]
  boundingObject Sphere {
    radius 0.032
  }
}
Solid {
  translation -0.10 0.030 0.37
  name "flower_stem_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.31 0.63 0.26
        roughness 0.58
      }
      geometry Cylinder {
        radius 0.012
        height 0.06
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.012
    height 0.06
  }
}
Solid {
  translation 0.11 0.030 -0.34
  name "flower_stem_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.28 0.56 0.30
        roughness 0.58
      }
      geometry Cylinder {
        radius 0.011
        height 0.06
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.011
    height 0.06
  }
}
# Narrow channels and corners for directional exploration.
Solid {
  translation 0.45 0.024 0.02
  rotation 0 1 0 0.45
  name "twig_wall_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.49 0.31 0.17
        roughness 0.66
      }
      geometry Box {
        size 0.44 0.048 0.03
      }
    }
  ]
  boundingObject Box {
    size 0.44 0.048 0.03
  }
}
Solid {
  translation -0.43 0.024 0.07
  rotation 0 1 0 -0.68
  name "twig_wall_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.43 0.28 0.14
        roughness 0.66
      }
      geometry Box {
        size 0.40 0.048 0.03
      }
    }
  ]
  boundingObject Box {
    size 0.40 0.048 0.03
  }
}
Solid {
  translation 0.02 0.024 -0.48
  rotation 0 1 0 1.12
  name "twig_wall_c"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.46 0.32 0.19
        roughness 0.66
      }
      geometry Box {
        size 0.34 0.048 0.03
      }
    }
  ]
  boundingObject Box {
    size 0.34 0.048 0.03
  }
}
# Uneven patches create changing inertial/contact stimuli.
Solid {
  translation -0.52 0.028 -0.35
  rotation 0 0 1 -0.35
  name "leaf_ramp_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.29 0.52 0.22
        roughness 0.49
      }
      geometry Box {
        size 0.22 0.06 0.14
      }
    }
  ]
  boundingObject Box {
    size 0.22 0.06 0.14
  }
}
Solid {
  translation 0.56 0.029 0.34
  rotation 0 0 1 0.32
  name "leaf_ramp_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.25 0.49 0.24
        roughness 0.49
      }
      geometry Box {
        size 0.22 0.06 0.14
      }
    }
  ]
  boundingObject Box {
    size 0.22 0.06 0.14
  }
}
# Dynamic particles and bars keep gradients non-static.
Solid {
  translation 0.18 0.016 -0.19
  linearVelocity -0.38 0 0.27
  angularVelocity 0 6.4 0
  name "pollen_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.96 0.88 0.21
        roughness 0.28
      }
      geometry Sphere {
        radius 0.012
      }
    }
  ]
  boundingObject Sphere {
    radius 0.012
  }
  physics Physics {
    density -1
    mass 0.003
  }
}
Solid {
  translation -0.24 0.016 0.22
  linearVelocity 0.42 0 -0.24
  angularVelocity 0 5.9 0
  name "pollen_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.93 0.72 0.16
        roughness 0.28
      }
      geometry Sphere {
        radius 0.011
      }
    }
  ]
  boundingObject Sphere {
    radius 0.011
  }
  physics Physics {
    density -1
    mass 0.0025
  }
}
Solid {
  translation 0.38 0.018 0.41
  rotation 0 1 0 0.78
  linearVelocity -0.19 0 -0.34
  angularVelocity 0 4.8 0
  name "odor_bar_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.34 0.79 0.55
        roughness 0.35
      }
      geometry Box {
        size 0.16 0.036 0.02
      }
    }
  ]
  boundingObject Box {
    size 0.16 0.036 0.02
  }
  physics Physics {
    density -1
    mass 0.004
  }
}
Solid {
  translation -0.37 0.018 -0.40
  rotation 0 1 0 -0.62
  linearVelocity 0.21 0 0.32
  angularVelocity 0 5.2 0
  name "odor_bar_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.37 0.64 0.82
        roughness 0.35
      }
      geometry Box {
        size 0.15 0.036 0.02
      }
    }
  ]
  boundingObject Box {
    size 0.15 0.036 0.02
  }
  physics Physics {
    density -1
    mass 0.004
  }
}
DrosophilaRobot {
  translation -0.62 0.016 -0.58
  name "Drosophila"
  controller "nao_nn_controller_uds"
  controllerArgs [
    "NM_BRAINS=default"
  ]
}
"""
    world_path.write_text(world, encoding="utf-8")


def build_webots_config(snapshot: dict[str, Any], config_path: Path) -> None:
    net = dict(snapshot.get("net", {}))
    net["num_sensory_neurons"] = max(TOTAL_SENSOR_CHANNELS, int(net.get("num_sensory_neurons", 0) or 0))
    net["num_output_neurons"] = max(1, int(net.get("num_output_neurons", 0) or 0))
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
    return [f"motor_{idx:03d}" for idx in range(count)]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate Webots Drosophila robot/world/config assets.")
    parser.add_argument(
        "--network",
        default="network_drosophila.json",
        help="Path to connectome snapshot JSON",
    )
    parser.add_argument(
        "--proto",
        default="webots_world/protos/DrosophilaRobot.proto",
        help="Output PROTO path",
    )
    parser.add_argument(
        "--world",
        default="webots_world/worlds/drosophila_neuroworld.wbt",
        help="Output world path",
    )
    parser.add_argument(
        "--config",
        default="webots_world/configs/config_drosophila_webots.json",
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
    default_config = "../configs/config_drosophila_webots.json"

    generate_proto(output_nodes, proto_path, default_network, default_config)
    generate_world(world_path)
    build_webots_config(snapshot, config_path)

    print(
        f"Wrote {proto_path} and {world_path} with {len(output_nodes)} motor outputs; "
        f"sensory target={TOTAL_SENSOR_CHANNELS}; config at {config_path}"
    )


if __name__ == "__main__":
    main()
