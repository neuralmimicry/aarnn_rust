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
    # Keep high-dimensional motor channels physically present but tucked inside
    # the thorax so the visual profile stays recognizably fruit-fly shaped.
    layers = max(1, min(5, math.ceil(total / 20)))
    per_layer = max(1, math.ceil(total / layers))
    layer_idx = min(layers - 1, index // per_layer)
    in_layer_idx = index % per_layer
    theta = (2.0 * math.pi * in_layer_idx) / float(per_layer)
    radius = 0.0023 + 0.0010 * (layer_idx % 2)
    x = -0.003 + (0.0022 * layer_idx)
    z = radius * math.sin(theta)
    y = 0.0002 + radius * math.cos(theta)

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
              baseColor 0.2 0.2 0.2
              roughness 0.55
              metalness 0
              transparency 1
            }}
            geometry Sphere {{
              radius 0.00035
            }}
          }}
        ]
        boundingObject Sphere {{
          radius 0.0004
        }}
        physics Physics {{
          density -1
          mass 0.000001
        }}
      }}
    }}
"""


def build_sensor_block() -> str:
    distance_blocks = []
    for i in range(DISTANCE_SENSOR_COUNT):
        yaw = (2.0 * math.pi * i) / float(DISTANCE_SENSOR_COUNT)
        tx = -0.004 + 0.0120 * math.cos(yaw)
        tz = 0.0095 * math.sin(yaw)
        distance_blocks.append(
            f"""      DistanceSensor {{
        name "fly_prox_{i:02d}"
        type "infra-red"
        translation {tx:.5f} 0.0034 {tz:.5f}
        rotation 0 1 0 {yaw:.5f}
        lookupTable [
          0 1000 0
          0.18 0 0
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
        translation 0.010 0.0015 0.0
      }}
      TouchSensor {{
        name "fly_touch_rear"
        type "bumper"
        translation -0.024 0.0009 0.0
      }}
      TouchSensor {{
        name "fly_touch_left"
        type "bumper"
        translation -0.004 0.0015 0.010
      }}
      TouchSensor {{
        name "fly_touch_right"
        type "bumper"
        translation -0.004 0.0015 -0.010
      }}
      # Directional proximity ring for odor/obstacle gradient response.
{chr(10).join(distance_blocks)}
"""


def build_visual_body_block() -> str:
    # More drosophila-like shape: compact thorax, tapered abdomen, big red eyes, long legs.
    return """      # Head-thorax-abdomen body axis
      Transform {
        translation 0.0062 0.0029 0
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.78 0.59 0.34
              roughness 0.38
              metalness 0
            }
            geometry Sphere {
              radius 0.0026
            }
          }
        ]
      }
      Transform {
        translation 0.0002 0.0027 0
        rotation 0 0 1 1.5708
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.52 0.37 0.22
              roughness 0.40
              metalness 0
            }
            geometry Capsule {
              radius 0.0033
              height 0.0078
            }
          }
        ]
      }
      Transform {
        translation -0.0064 0.0026 0
        rotation 0 0 1 1.5708
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.67 0.49 0.29
              roughness 0.46
              metalness 0
            }
            geometry Capsule {
              radius 0.0030
              height 0.0102
            }
          }
        ]
      }
      Transform {
        translation -0.0122 0.0023 0
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.55 0.38 0.22
              roughness 0.52
            }
            geometry Sphere {
              radius 0.0019
            }
          }
        ]
      }
      # Abdomen striping
      Transform {
        translation -0.0093 0.0028 0
        children [ Shape { appearance PBRAppearance { baseColor 0.22 0.17 0.13 roughness 0.62 } geometry Box { size 0.0007 0.0058 0.0062 } } ]
      }
      Transform {
        translation -0.0071 0.0028 0
        children [ Shape { appearance PBRAppearance { baseColor 0.22 0.17 0.13 roughness 0.62 } geometry Box { size 0.0007 0.0062 0.0065 } } ]
      }
      Transform {
        translation -0.0049 0.0028 0
        children [ Shape { appearance PBRAppearance { baseColor 0.22 0.17 0.13 roughness 0.62 } geometry Box { size 0.0007 0.0064 0.0068 } } ]
      }
      # Compound eyes
      Transform {
        translation 0.0072 0.0031 0.0022
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.86 0.10 0.08
              roughness 0.20
              metalness 0
            }
            geometry Sphere {
              radius 0.0019
            }
          }
        ]
      }
      Transform {
        translation 0.0072 0.0031 -0.0022
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.86 0.10 0.08
              roughness 0.20
              metalness 0
            }
            geometry Sphere {
              radius 0.0019
            }
          }
        ]
      }
      # Ocelli
      Transform {
        translation 0.0068 0.0048 0
        children [ Shape { appearance PBRAppearance { baseColor 0.10 0.07 0.05 roughness 0.35 } geometry Sphere { radius 0.00024 } } ]
      }
      Transform {
        translation 0.0065 0.0046 0.00045
        children [ Shape { appearance PBRAppearance { baseColor 0.10 0.07 0.05 roughness 0.35 } geometry Sphere { radius 0.00020 } } ]
      }
      Transform {
        translation 0.0065 0.0046 -0.00045
        children [ Shape { appearance PBRAppearance { baseColor 0.10 0.07 0.05 roughness 0.35 } geometry Sphere { radius 0.00020 } } ]
      }
      # Antennae and proboscis
      Transform {
        translation 0.0081 0.0033 0.00095
        rotation 0 0 1 0.56
        children [ Shape { appearance PBRAppearance { baseColor 0.66 0.53 0.38 roughness 0.74 } geometry Capsule { radius 0.00014 height 0.0030 } } ]
      }
      Transform {
        translation 0.0081 0.0033 -0.00095
        rotation 0 0 1 -0.56
        children [ Shape { appearance PBRAppearance { baseColor 0.66 0.53 0.38 roughness 0.74 } geometry Capsule { radius 0.00014 height 0.0030 } } ]
      }
      Transform {
        translation 0.0078 0.0012 0
        rotation 0 0 1 0.12
        children [ Shape { appearance PBRAppearance { baseColor 0.70 0.56 0.40 roughness 0.77 } geometry Capsule { radius 0.00022 height 0.0028 } } ]
      }
      # Wings
      Transform {
        translation -0.0012 0.0050 0.0029
        rotation 0 1 0 0.14
        scale 0.0108 0.00008 0.0030
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.94 0.95 0.91
              roughness 0.10
              metalness 0
              transparency 0.48
            }
            geometry Sphere {
              radius 1
            }
          }
        ]
      }
      Transform {
        translation -0.0012 0.0050 -0.0029
        rotation 0 1 0 -0.14
        scale 0.0108 0.00008 0.0030
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.94 0.95 0.91
              roughness 0.10
              metalness 0
              transparency 0.48
            }
            geometry Sphere {
              radius 1
            }
          }
        ]
      }
      # Wing veins
      Transform {
        translation -0.0001 0.0052 0.0032
        rotation 0 0 1 1.30
        children [ Shape { appearance PBRAppearance { baseColor 0.58 0.51 0.39 roughness 0.74 transparency 0.34 } geometry Capsule { radius 0.00005 height 0.0070 } } ]
      }
      Transform {
        translation -0.0029 0.0053 0.0028
        rotation 0 0 1 1.74
        children [ Shape { appearance PBRAppearance { baseColor 0.58 0.51 0.39 roughness 0.74 transparency 0.36 } geometry Capsule { radius 0.000045 height 0.0044 } } ]
      }
      Transform {
        translation -0.0001 0.0052 -0.0032
        rotation 0 0 1 -1.30
        children [ Shape { appearance PBRAppearance { baseColor 0.58 0.51 0.39 roughness 0.74 transparency 0.34 } geometry Capsule { radius 0.00005 height 0.0070 } } ]
      }
      Transform {
        translation -0.0029 0.0053 -0.0028
        rotation 0 0 1 -1.74
        children [ Shape { appearance PBRAppearance { baseColor 0.58 0.51 0.39 roughness 0.74 transparency 0.36 } geometry Capsule { radius 0.000045 height 0.0044 } } ]
      }
      # Halteres
      Transform {
        translation -0.0035 0.0039 0.0018
        rotation 0 0 1 0.66
        children [ Shape { appearance PBRAppearance { baseColor 0.79 0.70 0.52 roughness 0.63 } geometry Capsule { radius 0.00016 height 0.0018 } } ]
      }
      Transform {
        translation -0.0035 0.0039 -0.0018
        rotation 0 0 1 -0.66
        children [ Shape { appearance PBRAppearance { baseColor 0.79 0.70 0.52 roughness 0.63 } geometry Capsule { radius 0.00016 height 0.0018 } } ]
      }
      # Front legs
      Transform {
        translation 0.0034 0.0011 0.0024
        rotation 0 0 1 1.03
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.76 } geometry Capsule { radius 0.00017 height 0.0048 } } ]
      }
      Transform {
        translation 0.0058 -0.0019 0.0037
        rotation 0 0 1 0.34
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.78 } geometry Capsule { radius 0.00013 height 0.0052 } } ]
      }
      Transform {
        translation 0.0075 -0.0048 0.0045
        children [ Shape { appearance PBRAppearance { baseColor 0.12 0.10 0.08 roughness 0.57 } geometry Sphere { radius 0.00016 } } ]
      }
      Transform {
        translation 0.0034 0.0011 -0.0024
        rotation 0 0 1 -1.03
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.76 } geometry Capsule { radius 0.00017 height 0.0048 } } ]
      }
      Transform {
        translation 0.0058 -0.0019 -0.0037
        rotation 0 0 1 -0.34
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.78 } geometry Capsule { radius 0.00013 height 0.0052 } } ]
      }
      Transform {
        translation 0.0075 -0.0048 -0.0045
        children [ Shape { appearance PBRAppearance { baseColor 0.12 0.10 0.08 roughness 0.57 } geometry Sphere { radius 0.00016 } } ]
      }
      # Middle legs
      Transform {
        translation -0.0003 0.0010 0.0029
        rotation 0 0 1 0.88
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.76 } geometry Capsule { radius 0.00017 height 0.0052 } } ]
      }
      Transform {
        translation 0.0020 -0.0022 0.0044
        rotation 0 0 1 0.24
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.78 } geometry Capsule { radius 0.00013 height 0.0054 } } ]
      }
      Transform {
        translation 0.0036 -0.0052 0.0053
        children [ Shape { appearance PBRAppearance { baseColor 0.12 0.10 0.08 roughness 0.57 } geometry Sphere { radius 0.00016 } } ]
      }
      Transform {
        translation -0.0003 0.0010 -0.0029
        rotation 0 0 1 -0.88
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.76 } geometry Capsule { radius 0.00017 height 0.0052 } } ]
      }
      Transform {
        translation 0.0020 -0.0022 -0.0044
        rotation 0 0 1 -0.24
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.78 } geometry Capsule { radius 0.00013 height 0.0054 } } ]
      }
      Transform {
        translation 0.0036 -0.0052 -0.0053
        children [ Shape { appearance PBRAppearance { baseColor 0.12 0.10 0.08 roughness 0.57 } geometry Sphere { radius 0.00016 } } ]
      }
      # Rear legs
      Transform {
        translation -0.0040 0.0010 0.0027
        rotation 0 0 1 0.71
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.76 } geometry Capsule { radius 0.00017 height 0.0057 } } ]
      }
      Transform {
        translation -0.0020 -0.0027 0.0042
        rotation 0 0 1 0.10
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.78 } geometry Capsule { radius 0.00013 height 0.0058 } } ]
      }
      Transform {
        translation -0.0007 -0.0058 0.0051
        children [ Shape { appearance PBRAppearance { baseColor 0.12 0.10 0.08 roughness 0.57 } geometry Sphere { radius 0.00016 } } ]
      }
      Transform {
        translation -0.0040 0.0010 -0.0027
        rotation 0 0 1 -0.71
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.76 } geometry Capsule { radius 0.00017 height 0.0057 } } ]
      }
      Transform {
        translation -0.0020 -0.0027 -0.0042
        rotation 0 0 1 -0.10
        children [ Shape { appearance PBRAppearance { baseColor 0.83 0.69 0.48 roughness 0.78 } geometry Capsule { radius 0.00013 height 0.0058 } } ]
      }
      Transform {
        translation -0.0007 -0.0058 -0.0051
        children [ Shape { appearance PBRAppearance { baseColor 0.12 0.10 0.08 roughness 0.57 } geometry Sphere { radius 0.00016 } } ]
      }
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
    visual_body_block = build_visual_body_block()
    proto = f"""#VRML_SIM R2025a utf8

PROTO DrosophilaRobot [
  field SFVec3f     translation 0 0.008 0
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
{visual_body_block}
{sensor_block}
{motor_blocks}
      Group {{
        children IS extensionSlot
      }}
    ]
    boundingObject Group {{
      children [
        Transform {{
          translation 0.0002 0.0027 0
          rotation 0 0 1 1.5708
          children [
            Capsule {{
              radius 0.0033
              height 0.0078
            }}
          ]
        }}
        Transform {{
          translation 0.0062 0.0029 0
          children [
            Sphere {{
              radius 0.0026
            }}
          ]
        }}
        Transform {{
          translation -0.0064 0.0026 0
          rotation 0 0 1 1.5708
          children [
            Capsule {{
              radius 0.0030
              height 0.0102
            }}
          ]
        }}
        Transform {{
          translation -0.0122 0.0023 0
          children [
            Sphere {{
              radius 0.0019
            }}
          ]
        }}
      ]
    }}
    physics Physics {{
      density -1
      mass 0.0008
    }}
  }}
}}
"""
    proto_path.write_text(proto, encoding="utf-8")


def generate_world(world_path: Path) -> None:
    world = """#VRML_SIM R2025a utf8

EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackground.proto"
EXTERNPROTO "https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackgroundLight.proto"
EXTERNPROTO "../protos/DrosophilaRobot.proto"

WorldInfo {
  basicTimeStep 16
}
Viewpoint {
  fieldOfView 1
  # Corrects roll to ensure the ground is always at the bottom of the screen
  orientation -0.05 0.99 0.05 1.57 
  position 0.6 0.25 0.35
}
TexturedBackground {
}
TexturedBackgroundLight {
}
# Ground and floor detail.
Solid {
  translation 0 0.005 0
  name "ground_base"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.34 0.31 0.25
        roughness 0.95
      }
      geometry Box {
        size 40 0.01 40
      }
    }
  ]
  boundingObject Box {
    size 40 0.01 40
  }
}
Solid {
  translation 0 0.011 0
  name "grass_layer"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.29 0.40 0.25
        roughness 0.90
      }
      geometry Box {
        size 3.2 0.002 3.2
      }
    }
  ]
  boundingObject Box {
    size 3.2 0.002 3.2
  }
}
Solid {
  translation 0.03 0.0035 0.02
  rotation 0 1 0 0.14
  name "orchard_soil_patch"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.39 0.31 0.20
        roughness 0.87
      }
      geometry Box {
        size 1.46 0.003 1.08
      }
    }
  ]
  boundingObject Box {
    size 1.46 0.003 1.08
  }
}
# Central platform so the fly starts on a stable object, not in mid-air.
Solid {
  translation 0 0.020 0
  name "spawn_platform"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.50 0.38 0.23
        roughness 0.62
      }
      geometry Cylinder {
        radius 0.18
        height 0.040
      }
    }
  ]
  boundingObject Cylinder {
    radius 0.18
    height 0.040
  }
}
# Spawn cradle: three low rails around platform to prevent instant fall-off.
Solid {
  translation -0.10 0.050 0
  name "spawn_rail_west"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.31 0.19 roughness 0.72 } geometry Box { size 0.020 0.060 0.32 } } ]
  boundingObject Box { size 0.020 0.060 0.32 }
}
Solid {
  translation 0 0.050 -0.15
  name "spawn_rail_north"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.31 0.19 roughness 0.72 } geometry Box { size 0.24 0.060 0.020 } } ]
  boundingObject Box { size 0.24 0.060 0.020 }
}
Solid {
  translation 0 0.050 0.15
  name "spawn_rail_south"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.31 0.19 roughness 0.72 } geometry Box { size 0.24 0.060 0.020 } } ]
  boundingObject Box { size 0.24 0.060 0.020 }
}
# Low earth berm around the active area.
Solid {
  translation 0 0.065 -0.96
  name "berm_north"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 2.10 0.13 0.10 } } ]
  boundingObject Box { size 2.10 0.13 0.10 }
}
Solid {
  translation 0 0.065 0.96
  name "berm_south"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 2.10 0.13 0.10 } } ]
  boundingObject Box { size 2.10 0.13 0.10 }
}
Solid {
  translation -0.96 0.065 0
  name "berm_west"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 0.10 0.13 2.10 } } ]
  boundingObject Box { size 0.10 0.13 2.10 }
}
Solid {
  translation 0.96 0.065 0
  name "berm_east"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 0.10 0.13 2.10 } } ]
  boundingObject Box { size 0.10 0.13 2.10 }
}
# Trees 
Solid {
  translation -0.62 0  -0.46
  name "tree_a"
  children [
    Transform {
      translation 0 0.21 0
      children [ Shape { appearance PBRAppearance { baseColor 0.36 0.24 0.15 roughness 0.72 } geometry Cylinder { radius 0.035 height 0.42 } } ]
    }
    Transform {
      translation 0 0.50 0
      children [ Shape { appearance PBRAppearance { baseColor 0.21 0.44 0.17 roughness 0.56 } geometry Sphere { radius 0.18 } } ]
    }
    Transform {
      translation 0.10 0.46 -0.04
      children [ Shape { appearance PBRAppearance { baseColor 0.22 0.49 0.20 roughness 0.56 } geometry Sphere { radius 0.12 } } ]
    }
    Transform {
      translation 0.07 0.40 0.01
      children [ Shape { appearance PBRAppearance { baseColor 0.80 0.16 0.10 roughness 0.36 } geometry Sphere { radius 0.020 } } ]
    }
    Transform {
      translation -0.08 0.38 -0.03
      children [ Shape { appearance PBRAppearance { baseColor 0.83 0.19 0.12 roughness 0.36 } geometry Sphere { radius 0.018 } } ]
    }
  ]
  boundingObject Group {
    children [
      Transform {
        translation 0 0.21 0
        children [ Cylinder { radius 0.045 height 0.42 } ]
      }
    ]
  }
}
Solid {
  translation 0.66 0 -0.34
  name "tree_b"
  children [
    Transform {
      translation 0 0.22 0
      children [ Shape { appearance PBRAppearance { baseColor 0.37 0.25 0.16 roughness 0.72 } geometry Cylinder { radius 0.037 height 0.44 } } ]
    }
    Transform {
      translation 0 0.52 0
      children [ Shape { appearance PBRAppearance { baseColor 0.22 0.45 0.18 roughness 0.56 } geometry Sphere { radius 0.19 } } ]
    }
    Transform {
      translation -0.11 0.47 0.05
      children [ Shape { appearance PBRAppearance { baseColor 0.22 0.50 0.20 roughness 0.56 } geometry Sphere { radius 0.12 } } ]
    }
    Transform {
      translation 0.08 0.42 -0.05
      children [ Shape { appearance PBRAppearance { baseColor 0.80 0.17 0.10 roughness 0.36 } geometry Sphere { radius 0.020 } } ]
    }
    Transform {
      translation -0.03 0.39 0.08
      children [ Shape { appearance PBRAppearance { baseColor 0.83 0.20 0.12 roughness 0.36 } geometry Sphere { radius 0.018 } } ]
    }
  ]
  boundingObject Group {
    children [
      Transform {
        translation 0 0.22 0
        children [ Cylinder { radius 0.047 height 0.44 } ]
      }
    ]
  }
}
Solid {
  translation 0.02 0 0.72
  name "tree_c"
  children [
    Transform {
      translation 0 0.23 0
      children [ Shape { appearance PBRAppearance { baseColor 0.35 0.24 0.15 roughness 0.72 } geometry Cylinder { radius 0.039 height 0.46 } } ]
    }
    Transform {
      translation 0 0.54 0
      children [ Shape { appearance PBRAppearance { baseColor 0.21 0.44 0.17 roughness 0.56 } geometry Sphere { radius 0.20 } } ]
    }
    Transform {
      translation 0.12 0.49 -0.01
      children [ Shape { appearance PBRAppearance { baseColor 0.23 0.50 0.21 roughness 0.56 } geometry Sphere { radius 0.13 } } ]
    }
    Transform {
      translation -0.08 0.43 0.05
      children [ Shape { appearance PBRAppearance { baseColor 0.81 0.17 0.11 roughness 0.36 } geometry Sphere { radius 0.021 } } ]
    }
  ]
  boundingObject Group {
    children [
      Transform {
        translation 0 0.23 0
        children [ Cylinder { radius 0.050 height 0.46 } ]
      }
    ]
  }
}
    # Fruit and fermenting attractants.
Solid {
  translation 0.22 0.022 0.10
  name "ferment_core_a"
  children [
    Shape {
      appearance PBRAppearance { baseColor 0.84 0.52 0.21 roughness 0.44 }
      geometry Cylinder { radius 0.08 height 0.044 }
    }
    Transform {
      translation 0.05 0.021 0.02
      children [ Shape { appearance PBRAppearance { baseColor 0.80 0.16 0.10 roughness 0.36 } geometry Sphere { radius 0.027 } } ]
    }
    Transform {
      translation -0.04 0.018 -0.03
      children [ Shape { appearance PBRAppearance { baseColor 0.88 0.68 0.25 roughness 0.40 } geometry Sphere { radius 0.024 } } ]
    }
  ]
  boundingObject Cylinder { radius 0.08 height 0.044 }
}
Solid {
  translation -0.23 0.021 -0.20
  name "ferment_core_b"
  children [
    Shape {
      appearance PBRAppearance { baseColor 0.76 0.46 0.20 roughness 0.46 }
      geometry Cylinder { radius 0.075 height 0.042 }
    }
    Transform {
      translation 0.03 0.019 -0.03
      children [ Shape { appearance PBRAppearance { baseColor 0.73 0.16 0.09 roughness 0.38 } geometry Sphere { radius 0.022 } } ]
    }
    Transform {
      translation -0.04 0.018 0.02
      children [ Shape { appearance PBRAppearance { baseColor 0.87 0.71 0.26 roughness 0.40 } geometry Sphere { radius 0.021 } } ]
    }
  ]
  boundingObject Cylinder { radius 0.075 height 0.042 }
}
Solid {
  translation 0.43 0.027 -0.18
  name "fallen_apple_a"
  children [ Shape { appearance PBRAppearance { baseColor 0.79 0.17 0.11 roughness 0.36 } geometry Sphere { radius 0.027 } } ]
  boundingObject Sphere { radius 0.027 }
}
Solid {
  translation -0.39 0.025 0.26
  name "fallen_apple_b"
  children [ Shape { appearance PBRAppearance { baseColor 0.76 0.16 0.10 roughness 0.37 } geometry Sphere { radius 0.025 } } ]
  boundingObject Sphere { radius 0.025 }
}
Solid {
  translation 0.05 0.023 0.21
  rotation 0 0 1 1.5708
  name "banana_piece"
  children [ Shape { appearance PBRAppearance { baseColor 0.89 0.77 0.30 roughness 0.36 } geometry Capsule { radius 0.008 height 0.030 } } ]
  boundingObject Capsule { radius 0.008 height 0.030 }
}
# Flowers and odor markers.
Solid {
  translation -0.12 0.046 0.36
  name "flower_cluster_a"
  children [
    Shape { appearance PBRAppearance { baseColor 0.27 0.59 0.25 roughness 0.58 } geometry Cylinder { radius 0.012 height 0.092 } }
    Transform {
      translation 0 0.060 0
      children [ Shape { appearance PBRAppearance { baseColor 0.90 0.82 0.20 roughness 0.33 } geometry Sphere { radius 0.023 } } ]
    }
  ]
  boundingObject Cylinder { radius 0.016 height 0.092 }
}
Solid {
  translation 0.17 0.048 -0.41
  name "flower_cluster_b"
  children [
    Shape { appearance PBRAppearance { baseColor 0.25 0.56 0.27 roughness 0.58 } geometry Cylinder { radius 0.012 height 0.096 } }
    Transform {
      translation 0 0.062 0
      children [ Shape { appearance PBRAppearance { baseColor 0.92 0.74 0.22 roughness 0.33 } geometry Sphere { radius 0.022 } } ]
    }
  ]
  boundingObject Cylinder { radius 0.016 height 0.096 }
}
Solid {
  translation 0.26 0.003 0.24
  name "attractant_pad_a"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.30 0.66 0.40
        emissiveColor 0.08 0.21 0.14
        roughness 0.34
      }
      geometry Cylinder {
        radius 0.038
        height 0.006
      }
    }
  ]
  boundingObject Cylinder { radius 0.038 height 0.006 }
}
Solid {
  translation -0.28 0.003 -0.28
  name "attractant_pad_b"
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.28 0.56 0.65
        emissiveColor 0.08 0.17 0.22
        roughness 0.35
      }
      geometry Cylinder {
        radius 0.036
        height 0.006
      }
    }
  ]
  boundingObject Cylinder { radius 0.036 height 0.006 }
}
Transform {
  translation 0.26 0.064 0.24
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.52 0.88 0.66
        emissiveColor 0.16 0.29 0.23
        roughness 0.18
        transparency 0.75
      }
      geometry Cylinder {
        radius 0.016
        height 0.125
      }
    }
  ]
}
Transform {
  translation -0.28 0.064 -0.28
  children [
    Shape {
      appearance PBRAppearance {
        baseColor 0.58 0.80 0.93
        emissiveColor 0.15 0.23 0.30
        roughness 0.18
        transparency 0.75
      }
      geometry Cylinder {
        radius 0.015
        height 0.125
      }
    }
  ]
}
# Twigs and stones.
Solid {
  translation 0.40 0.014 -0.03
  rotation 0 1 0 0.40
  name "twig_a"
  children [ Shape { appearance PBRAppearance { baseColor 0.44 0.30 0.19 roughness 0.69 } geometry Box { size 0.34 0.028 0.032 } } ]
  boundingObject Box { size 0.34 0.028 0.032 }
}
Solid {
  translation -0.16 0.014 0.35
  rotation 0 1 0 0.72
  name "twig_b"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.28 0.17 roughness 0.70 } geometry Box { size 0.30 0.028 0.032 } } ]
  boundingObject Box { size 0.30 0.028 0.032 }
}
Solid {
  translation 0.08 0.013 -0.43
  rotation 0 1 0 -0.66
  name "twig_c"
  children [ Shape { appearance PBRAppearance { baseColor 0.45 0.31 0.19 roughness 0.69 } geometry Box { size 0.28 0.026 0.030 } } ]
  boundingObject Box { size 0.28 0.026 0.030 }
}
Solid {
  translation 0.08 0.020 0.41
  name "stone_a"
  children [ Shape { appearance PBRAppearance { baseColor 0.46 0.46 0.48 roughness 0.78 } geometry Sphere { radius 0.020 } } ]
  boundingObject Sphere { radius 0.020 }
}
Solid {
  translation -0.53 0.017 -0.10
  name "stone_b"
  children [ Shape { appearance PBRAppearance { baseColor 0.44 0.44 0.46 roughness 0.78 } geometry Sphere { radius 0.017 } } ]
  boundingObject Sphere { radius 0.017 }
}
Solid {
  translation 0.53 0.022 -0.15
  name "stone_c"
  children [ Shape { appearance PBRAppearance { baseColor 0.48 0.48 0.50 roughness 0.77 } geometry Sphere { radius 0.022 } } ]
  boundingObject Sphere { radius 0.022 }
}
DrosophilaRobot {
  translation 0 0.044 0
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
    # Reset cached Webots view/layout state so updated Viewpoint is applied.
    wbproj_path = world_path.with_name(f".{world_path.stem}.wbproj")
    if wbproj_path.exists():
        wbproj_path.unlink()
    build_webots_config(snapshot, config_path)

    print(
        f"Wrote {proto_path} and {world_path} with {len(output_nodes)} motor outputs; "
        f"sensory target={TOTAL_SENSOR_CHANNELS}; config at {config_path}"
    )


if __name__ == "__main__":
    main()
