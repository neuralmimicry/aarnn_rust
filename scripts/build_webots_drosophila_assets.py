#!/usr/bin/env python3
"""
Generate Webots assets for a Drosophila connectome snapshot.

Fourth-pass generator:
- improved Drosophila morphology retained
- articulated anatomical wing joints with controllable flap motors
- articulated leg joint chains (coxa, femur, tibia, tarsus) for all six legs
- leg physics generator updated to avoid Webots light/oblong inertia warnings
- sanitised level/horizontal world presentation retained
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
BASE_SENSOR_CHANNELS = DISTANCE_SENSOR_COUNT + TOUCH_SENSOR_CHANNELS + IMU_SENSOR_CHANNELS

DROS_SENSOR_PREFIX = "dros_s"
DROS_OUTPUT_PREFIX = "dros_o"
DROS_SENSORS_REGEX = r"^dros_s_[0-9]{2}_.*$"
DROS_ACTUATORS_REGEX = r"^dros_o_[0-9]{3}_.*$"
DROS_EYE_LEFT_NAME = f"{DROS_SENSOR_PREFIX}_34_eye_left"
DROS_EYE_RIGHT_NAME = f"{DROS_SENSOR_PREFIX}_35_eye_right"
# Hidden endpoint solids are required for motor channels, but extremely low
# masses trigger Webots mass-ratio instability warnings in shared worlds.
MOTOR_ENDPOINT_MIN_MASS = 0.0001


def parse_bool(value: str, *, default: bool = True) -> bool:
    token = (value or "").strip().lower()
    if token in {"1", "true", "yes", "on", "enable", "enabled"}:
        return True
    if token in {"0", "false", "no", "off", "disable", "disabled"}:
        return False
    return default


def positive_int(raw: int | str, field_name: str) -> int:
    try:
        value = int(raw)
    except Exception as exc:
        raise SystemExit(f"{field_name} must be an integer, got: {raw!r}") from exc
    if value <= 0:
        raise SystemExit(f"{field_name} must be > 0, got: {value}")
    return value


def index_digits(n: int) -> int:
    max_index = max(0, n - 1)
    digits = 1
    while max_index >= 10:
        max_index //= 10
        digits += 1
    return max(2, digits)


def camera_channel_name(camera_name: str, polarity: str, row: int, col: int, row_digits: int, col_digits: int) -> str:
    return f"{camera_name}.{polarity}.r{row:0{row_digits}d}c{col:0{col_digits}d}"


def append_camera_channels(ports: list[str], camera_name: str, rows: int, cols: int) -> None:
    row_digits = index_digits(rows)
    col_digits = index_digits(cols)
    for r in range(rows):
        for c in range(cols):
            ports.append(camera_channel_name(camera_name, "on", r, c, row_digits, col_digits))
            ports.append(camera_channel_name(camera_name, "off", r, c, row_digits, col_digits))


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


def sanitize_device_name(name: str) -> str:
    out = []
    for ch in str(name):
        if ch.isalnum():
            out.append(ch.lower())
        else:
            out.append("_")
    value = "".join(out).strip("_")
    return value or "chan"


def output_motor_name(index: int, node_id: str) -> str:
    return f"{DROS_OUTPUT_PREFIX}_{index:03d}_{sanitize_device_name(node_id)}"


def expected_sensory_channel_ports(
    *,
    include_compound_eyes: bool,
    eye_retina_rows: int,
    eye_retina_cols: int,
) -> list[str]:
    ports = [
        f"{DROS_SENSOR_PREFIX}_00_accel.x",
        f"{DROS_SENSOR_PREFIX}_00_accel.y",
        f"{DROS_SENSOR_PREFIX}_00_accel.z",
        f"{DROS_SENSOR_PREFIX}_03_gyro.x",
        f"{DROS_SENSOR_PREFIX}_03_gyro.y",
        f"{DROS_SENSOR_PREFIX}_03_gyro.z",
        f"{DROS_SENSOR_PREFIX}_06_touch_front",
        f"{DROS_SENSOR_PREFIX}_07_touch_rear",
        f"{DROS_SENSOR_PREFIX}_08_touch_left",
        f"{DROS_SENSOR_PREFIX}_09_touch_right",
    ]
    for i in range(DISTANCE_SENSOR_COUNT):
        ports.append(f"{DROS_SENSOR_PREFIX}_{10 + i:02d}_prox_{i:02d}")
    if include_compound_eyes:
        append_camera_channels(ports, DROS_EYE_LEFT_NAME, eye_retina_rows, eye_retina_cols)
        append_camera_channels(ports, DROS_EYE_RIGHT_NAME, eye_retina_rows, eye_retina_cols)
    return ports


def sensory_nodes_from_snapshot(snapshot: dict[str, Any], minimum_count: int) -> list[str]:
    labels = snapshot.get("connectome_labels")
    if isinstance(labels, dict):
        names = labels.get("sensory_nodes")
        if isinstance(names, list) and names:
            selected = [str(name) for name in names]
            if len(selected) >= minimum_count:
                return selected

    net = snapshot.get("net", {})
    count = int(net.get("num_sensory_neurons", 0) or 0)
    count = max(minimum_count, count)
    return [f"sensory_{idx:03d}" for idx in range(count)]


def build_io_alignment_map(
    snapshot: dict[str, Any],
    sensory_nodes: list[str],
    output_nodes: list[str],
    brain_id: str,
    *,
    include_compound_eyes: bool,
    eye_retina_rows: int,
    eye_retina_cols: int,
) -> dict[str, Any]:
    labels = snapshot.get("connectome_labels")
    dataset = labels.get("dataset") if isinstance(labels, dict) else None
    sensory_ports = expected_sensory_channel_ports(
        include_compound_eyes=include_compound_eyes,
        eye_retina_rows=eye_retina_rows,
        eye_retina_cols=eye_retina_cols,
    )
    if len(sensory_nodes) < len(sensory_ports):
        missing = len(sensory_ports) - len(sensory_nodes)
        sensory_nodes = sensory_nodes + [f"sensory_pad_{i:03d}" for i in range(missing)]
    sensory_nodes = sensory_nodes[: len(sensory_ports)]
    output_names = [output_motor_name(i, node_id) for i, node_id in enumerate(output_nodes)]

    return {
        "dataset": dataset,
        "brain_id": brain_id,
        "sensor_regex": DROS_SENSORS_REGEX,
        "actuator_regex": DROS_ACTUATORS_REGEX,
        "sensory_channels": [
            {
                "channel_index": i,
                "connectome_node_id": sensory_nodes[i],
                "device_port": sensory_ports[i],
            }
            for i in range(len(sensory_ports))
        ],
        "output_channels": [
            {
                "channel_index": i,
                "connectome_node_id": output_nodes[i],
                "actuator_name": output_names[i],
            }
            for i in range(len(output_nodes))
        ],
    }


def build_motor_block(index: int, node_id: str, total: int) -> str:
    # Keep high-dimensional motor channels physically present but tucked inside
    # the thorax so the visible morphology remains recognisably fly-like.
    motor_name = output_motor_name(index, node_id)
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
          name \"{motor_name}\"
          maxVelocity 14
          minPosition -1.6
          maxPosition 1.6
        }}
      ]
      endPoint DEF {end_name} Solid {{
        name \"{end_name}\"
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
          mass {MOTOR_ENDPOINT_MIN_MASS:.5f}
        }}
      }}
    }}
"""


def build_sensor_block(
    *,
    include_compound_eyes: bool,
    eye_camera_width: int,
    eye_camera_height: int,
) -> str:
    distance_blocks = []
    for i in range(DISTANCE_SENSOR_COUNT):
        yaw = (2.0 * math.pi * i) / float(DISTANCE_SENSOR_COUNT)
        tx = -0.004 + 0.0120 * math.cos(yaw)
        tz = 0.0095 * math.sin(yaw)
        channel_index = 10 + i
        distance_blocks.append(
            f"""      DistanceSensor {{
        name \"{DROS_SENSOR_PREFIX}_{channel_index:02d}_prox_{i:02d}\"
        type \"infra-red\"
        translation {tx:.5f} 0.0034 {tz:.5f}
        rotation 0 1 0 {yaw:.5f}
        lookupTable [
          0 1000 0
          0.18 0 0
        ]
      }}"""
        )

    eye_block = ""
    if include_compound_eyes:
        # Two wide-FOV eyes approximating Drosophila compound vision.
        eye_block = f"""
      # Compound-eye cameras (left/right) for event-retina encoding.
      Camera {{
        name \"{DROS_EYE_LEFT_NAME}\"
        width {eye_camera_width}
        height {eye_camera_height}
        fieldOfView 2.6
        translation 0.00905 0.00340 0.00230
        rotation 0 1 0 0.95
      }}
      Camera {{
        name \"{DROS_EYE_RIGHT_NAME}\"
        width {eye_camera_width}
        height {eye_camera_height}
        fieldOfView 2.6
        translation 0.00905 0.00340 -0.00230
        rotation 0 1 0 -0.95
      }}"""

    return f"""      # Inertial sensing
      Accelerometer {{
        name \"{DROS_SENSOR_PREFIX}_00_accel\"
      }}
      Gyro {{
        name \"{DROS_SENSOR_PREFIX}_03_gyro\"
      }}
      # Contact sensing around body perimeter
      TouchSensor {{
        name \"{DROS_SENSOR_PREFIX}_06_touch_front\"
        type \"bumper\"
        translation 0.010 0.0015 0.0
      }}
      TouchSensor {{
        name \"{DROS_SENSOR_PREFIX}_07_touch_rear\"
        type \"bumper\"
        translation -0.024 0.0009 0.0
      }}
      TouchSensor {{
        name \"{DROS_SENSOR_PREFIX}_08_touch_left\"
        type \"bumper\"
        translation -0.004 0.0015 0.010
      }}
      TouchSensor {{
        name \"{DROS_SENSOR_PREFIX}_09_touch_right\"
        type \"bumper\"
        translation -0.004 0.0015 -0.010
      }}
      # Directional proximity ring for odour/obstacle gradient response.
{chr(10).join(distance_blocks)}
{eye_block}
"""


def wing_mesh_geometry() -> str:
    points = [
        # Root at thorax, with a shallow cambered membrane swept posteriorly.
        (0.0000, 0.00000, 0.0000),
        (-0.0018, 0.00006, 0.00070),
        (-0.0056, 0.00012, 0.00220),
        (-0.0108, 0.00016, 0.00320),
        (-0.0162, 0.00013, 0.00290),
        (-0.0204, 0.00007, 0.00160),
        (-0.0222, 0.00002, 0.00020),
        (-0.0210, -0.00006, -0.00120),
        (-0.0162, -0.00012, -0.00290),
        (-0.0109, -0.00015, -0.00360),
        (-0.0050, -0.00011, -0.00250),
        (-0.0016, -0.00005, -0.00110),
    ]
    coord = ",\n                    ".join(f"{x:.5f} {y:.5f} {z:.5f}" for x, y, z in points)
    faces = ", ".join(f"0, {i}, {i + 1}, -1" for i in range(1, len(points) - 1))
    return f"""IndexedFaceSet {{
                  coord Coordinate {{
                    point [
                      {coord}
                    ]
                  }}
                  coordIndex [
                    {faces}
                  ]
                  creaseAngle 0.6
                }}"""


def build_wing_joint(side: str) -> str:
    sign = 1 if side == "left" else -1
    side_name = "left" if sign == 1 else "right"
    wing_geom = wing_mesh_geometry()
    # Side profile tuning: roots slightly posterior/dorsal and wings pitched up/back.
    anchor_x = -0.00170
    anchor_y = 0.00600
    anchor_z = 0.00245 * sign
    sweep = 0.40 * sign
    dihedral = 0.22
    return f"""      DEF WING_{side_name.upper()}_JOINT HingeJoint {{
        jointParameters HingeJointParameters {{
          anchor {anchor_x:.5f} {anchor_y:.5f} {anchor_z:.5f}
          axis 1 0 0
        }}
        device [
          RotationalMotor {{
            name \"wing_{side_name}_flap\"
            minPosition -0.52
            maxPosition 0.52
            maxVelocity 40
            maxTorque 0.02
          }}
          PositionSensor {{
            name \"wing_{side_name}_flap_sensor\"
          }}
        ]
        endPoint Solid {{
          name \"wing_{side_name}_solid\"
          translation {anchor_x:.5f} {anchor_y:.5f} {anchor_z:.5f}
          children [
            Transform {{
              rotation 0 1 0 {sweep:.5f}
              children [
                Transform {{
                  rotation 1 0 0 {dihedral:.5f}
                  scale 1 1 1
                  children [
                    Shape {{
                      appearance PBRAppearance {{
                        baseColor 0.96 0.96 0.93
                        roughness 0.06
                        metalness 0
                        transparency 0.55
                      }}
                      geometry {wing_geom}
                    }}
                    Transform {{
                      translation -0.0086 0.00010 0.00028
                      rotation 0 0 1 -1.34
                      children [ Shape {{ appearance PBRAppearance {{ baseColor 0.63 0.56 0.43 roughness 0.70 transparency 0.28 }} geometry Capsule {{ radius 0.000045 height 0.0186 }} }} ]
                    }}
                    Transform {{
                      translation -0.0120 0.00007 -0.00065
                      rotation 0 0 1 -0.94
                      children [ Shape {{ appearance PBRAppearance {{ baseColor 0.63 0.56 0.43 roughness 0.72 transparency 0.34 }} geometry Capsule {{ radius 0.000036 height 0.0118 }} }} ]
                    }}
                    Transform {{
                      translation -0.0135 0.00005 -0.00165
                      rotation 0 0 1 -0.58
                      children [ Shape {{ appearance PBRAppearance {{ baseColor 0.63 0.56 0.43 roughness 0.74 transparency 0.38 }} geometry Capsule {{ radius 0.000030 height 0.0086 }} }} ]
                    }}
                    Transform {{
                      translation -0.0060 0.00007 -0.00135
                      rotation 0 0 1 -0.22
                      children [ Shape {{ appearance PBRAppearance {{ baseColor 0.63 0.56 0.43 roughness 0.74 transparency 0.42 }} geometry Capsule {{ radius 0.000026 height 0.0062 }} }} ]
                    }}
                  ]
                }}
              ]
            }}
          ]
          boundingObject Group {{
            children [
              Transform {{
                translation -0.0098 0.00002 0
                rotation 0 0 1 -1.33
                children [ Box {{ size 0.00100 0.0190 0.0052 }} ]
              }}
              Transform {{
                translation -0.0142 -0.00002 -0.0012
                rotation 0 0 1 -0.92
                children [ Box {{ size 0.00085 0.0120 0.0039 }} ]
              }}
            ]
          }}
          physics Physics {{
            density -1
            mass 0.00007
          }}
        }}
      }}
"""


def oriented_leg_bounding(angle: float, length: float, thickness: float) -> str:
    return f"""Transform {{
                              rotation 0 0 1 {angle:.5f}
                              children [
                                Box {{
                                  size {thickness:.5f} {length:.5f} {thickness:.5f}
                                }}
                              ]
                            }}"""


def build_leg_chain(side: str, segment_name: str, base_x: float, base_y: float, base_z: float,
                    coxa_angle: float, femur_angle: float, tibia_angle: float, tarsus_angle: float,
                    coxa_len: float, femur_len: float, tibia_len: float, tarsus_len: float) -> str:
    sign = 1 if side == "left" else -1
    side_name = "left" if sign == 1 else "right"
    z = base_z * sign

    coxa_radius = max(0.00022, coxa_len * 0.11)
    femur_radius = max(0.00024, femur_len * 0.09)
    tibia_radius = max(0.00018, tibia_len * 0.065)
    tarsus_radius = max(0.00016, tarsus_len * 0.070)

    # Use common fore/aft leg orientation for both sides; sign only mirrors lateral offsets.
    coxa_box = oriented_leg_bounding(coxa_angle, coxa_len, max(0.00062, coxa_radius * 2.4))
    femur_box = oriented_leg_bounding(femur_angle, femur_len, max(0.00070, femur_radius * 2.4))
    tibia_box = oriented_leg_bounding(tibia_angle, tibia_len, max(0.00058, tibia_radius * 2.5))
    tarsus_box = oriented_leg_bounding(tarsus_angle, tarsus_len, max(0.00058, tarsus_radius * 2.9))

    coxa_mass = max(0.00018, coxa_len * 0.07)
    femur_mass = max(0.00024, femur_len * 0.07)
    tibia_mass = max(0.00020, tibia_len * 0.06)
    tarsus_mass = max(0.00019, tarsus_len * 0.065)

    return f"""      DEF {sanitize_def(f'{side_name}_{segment_name}_coxa_joint')} HingeJoint {{
        jointParameters HingeJointParameters {{
          anchor {base_x:.5f} {base_y:.5f} {z:.5f}
          axis 0 0 {sign}
        }}
        device [
          RotationalMotor {{
            name \"leg_{side_name}_{segment_name}_coxa\"
            minPosition {-0.9 if sign == 1 else -0.2}
            maxPosition {0.2 if sign == 1 else 0.9}
            maxVelocity 8
            maxTorque 0.03
          }}
          PositionSensor {{
            name \"leg_{side_name}_{segment_name}_coxa_sensor\"
          }}
        ]
        endPoint Solid {{
          name \"leg_{side_name}_{segment_name}_coxa_solid\"
          translation {base_x:.5f} {base_y:.5f} {z:.5f}
          children [
            Transform {{
              rotation 0 0 1 {coxa_angle:.5f}
              children [
                Shape {{ appearance PBRAppearance {{ baseColor 0.78 0.62 0.40 roughness 0.78 }} geometry Capsule {{ radius {coxa_radius:.5f} height {coxa_len:.5f} }} }}
                Transform {{
                  translation {0.00034:.5f} {-coxa_len * 0.48:.5f} {0.00030 * sign:.5f}
                  rotation 0 0 1 {0.42 * sign:.5f}
                  children [
                    Shape {{ appearance PBRAppearance {{ baseColor 0.80 0.67 0.47 roughness 0.72 }} geometry Capsule {{ radius {max(0.00012, coxa_radius * 0.55):.5f} height {max(0.00072, coxa_len * 0.38):.5f} }} }}
                  ]
                }}
              ]
            }}
            HingeJoint {{
              jointParameters HingeJointParameters {{
                anchor {0.00055:.5f} {-coxa_len * 0.40:.5f} {0.00090 * sign:.5f}
                axis 0 0 {sign}
              }}
              device [
                RotationalMotor {{
                  name \"leg_{side_name}_{segment_name}_femur\"
                  minPosition {-1.3 if sign == 1 else -0.2}
                  maxPosition {0.2 if sign == 1 else 1.3}
                  maxVelocity 8
                  maxTorque 0.03
                }}
                PositionSensor {{
                  name \"leg_{side_name}_{segment_name}_femur_sensor\"
                }}
              ]
              endPoint Solid {{
                name \"leg_{side_name}_{segment_name}_femur_solid\"
                translation {0.00055:.5f} {-coxa_len * 0.40:.5f} {0.00090 * sign:.5f}
                children [
                  Transform {{
                    rotation 0 0 1 {femur_angle:.5f}
                    children [
                      Shape {{ appearance PBRAppearance {{ baseColor 0.84 0.70 0.48 roughness 0.76 }} geometry Capsule {{ radius {femur_radius:.5f} height {femur_len:.5f} }} }}
                    ]
                  }}
                  HingeJoint {{
                    jointParameters HingeJointParameters {{
                      anchor {0.00120:.5f} {-femur_len * 0.48:.5f} {0.00100 * sign:.5f}
                      axis 0 0 {sign}
                    }}
                    device [
                      RotationalMotor {{
                        name \"leg_{side_name}_{segment_name}_tibia\"
                        minPosition {-1.4 if sign == 1 else -0.15}
                        maxPosition {0.15 if sign == 1 else 1.4}
                        maxVelocity 8
                        maxTorque 0.025
                      }}
                      PositionSensor {{
                        name \"leg_{side_name}_{segment_name}_tibia_sensor\"
                      }}
                    ]
                    endPoint Solid {{
                      name \"leg_{side_name}_{segment_name}_tibia_solid\"
                      translation {0.00120:.5f} {-femur_len * 0.48:.5f} {0.00100 * sign:.5f}
                      children [
                        Transform {{
                          rotation 0 0 1 {tibia_angle:.5f}
                          children [
                            Shape {{ appearance PBRAppearance {{ baseColor 0.86 0.73 0.50 roughness 0.78 }} geometry Capsule {{ radius {tibia_radius:.5f} height {tibia_len:.5f} }} }}
                            Transform {{
                              translation {0.00036:.5f} {-tibia_len * 0.14:.5f} {0.00022 * sign:.5f}
                              rotation 0 0 1 {0.66 * sign:.5f}
                              children [ Shape {{ appearance PBRAppearance {{ baseColor 0.69 0.56 0.39 roughness 0.70 }} geometry Capsule {{ radius 0.00005 height 0.00085 }} }} ]
                            }}
                            Transform {{
                              translation {0.00048:.5f} {-tibia_len * 0.34:.5f} {0.00020 * sign:.5f}
                              rotation 0 0 1 {0.72 * sign:.5f}
                              children [ Shape {{ appearance PBRAppearance {{ baseColor 0.69 0.56 0.39 roughness 0.70 }} geometry Capsule {{ radius 0.00005 height 0.00090 }} }} ]
                            }}
                          ]
                        }}
                        HingeJoint {{
                          jointParameters HingeJointParameters {{
                            anchor {0.00135:.5f} {-tibia_len * 0.46:.5f} {0.00080 * sign:.5f}
                            axis 0 0 {sign}
                          }}
                          device [
                            RotationalMotor {{
                              name \"leg_{side_name}_{segment_name}_tarsus\"
                              minPosition {-0.9 if sign == 1 else -0.15}
                              maxPosition {0.15 if sign == 1 else 0.9}
                              maxVelocity 10
                              maxTorque 0.02
                            }}
                            PositionSensor {{
                              name \"leg_{side_name}_{segment_name}_tarsus_sensor\"
                            }}
                          ]
                          endPoint Solid {{
                            name \"leg_{side_name}_{segment_name}_tarsus_solid\"
                            translation {0.00135:.5f} {-tibia_len * 0.46:.5f} {0.00080 * sign:.5f}
                            children [
                              Transform {{
                                rotation 0 0 1 {tarsus_angle:.5f}
                                children [
                                  Shape {{ appearance PBRAppearance {{ baseColor 0.88 0.77 0.56 roughness 0.80 }} geometry Capsule {{ radius {tarsus_radius:.5f} height {tarsus_len:.5f} }} }}
                                  Transform {{
                                    translation 0.00058 {-tarsus_len * 0.48:.5f} 0
                                    children [ Shape {{ appearance PBRAppearance {{ baseColor 0.11 0.10 0.08 roughness 0.52 }} geometry Sphere {{ radius 0.00010 }} }} ]
                                  }}
                                  Transform {{
                                    translation 0.00076 {-tarsus_len * 0.57:.5f} {0.00013 * sign:.5f}
                                    rotation 0 0 1 {0.78 * sign:.5f}
                                    children [ Shape {{ appearance PBRAppearance {{ baseColor 0.09 0.08 0.06 roughness 0.52 }} geometry Capsule {{ radius 0.00004 height 0.00060 }} }} ]
                                  }}
                                  Transform {{
                                    translation 0.00076 {-tarsus_len * 0.57:.5f} {-0.00013 * sign:.5f}
                                    rotation 0 0 1 {0.58 * sign:.5f}
                                    children [ Shape {{ appearance PBRAppearance {{ baseColor 0.09 0.08 0.06 roughness 0.52 }} geometry Capsule {{ radius 0.00004 height 0.00058 }} }} ]
                                  }}
                                ]
                              }}
                            ]
                            boundingObject {tarsus_box}
                            physics Physics {{
                              density -1
                              mass {tarsus_mass:.5f}
                            }}
                          }}
                        }}
                      ]
                      boundingObject {tibia_box}
                      physics Physics {{
                        density -1
                        mass {tibia_mass:.5f}
                      }}
                    }}
                  }}
                ]
                boundingObject {femur_box}
                physics Physics {{
                  density -1
                  mass {femur_mass:.5f}
                }}
              }}
            }}
          ]
          boundingObject {coxa_box}
          physics Physics {{
            density -1
            mass {coxa_mass:.5f}
          }}
        }}
      }}
"""


def build_visual_core_body_block() -> str:
    return """      # Side-profile tuned: compact head, tall thorax and tapered abdomen.
      Transform {
        translation 0.0087 0.00335 0
        scale 1.02 0.92 0.98
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.84 0.64 0.37
              roughness 0.33
              metalness 0
            }
            geometry Sphere {
              radius 0.00205
            }
          }
        ]
      }
      Transform {
        translation 0.0097 0.00235 0
        scale 0.86 0.78 0.82
        children [ Shape { appearance PBRAppearance { baseColor 0.78 0.58 0.34 roughness 0.42 } geometry Sphere { radius 0.00092 } } ]
      }
      Transform {
        translation 0.00010 0.00355 0
        rotation 0 0 1 1.5708
        scale 1.20 1.04 1.08
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.57 0.41 0.24
              roughness 0.36
              metalness 0
            }
            geometry Capsule {
              radius 0.00325
              height 0.0092
            }
          }
        ]
      }
      Transform {
        translation -0.0011 0.00585 0
        rotation 0 0 1 1.5708
        scale 1.22 0.64 0.94
        children [ Shape { appearance PBRAppearance { baseColor 0.45 0.31 0.19 roughness 0.48 } geometry Capsule { radius 0.0020 height 0.0049 } } ]
      }
      Transform {
        translation -0.0034 0.00535 0
        scale 1.12 0.68 0.82
        children [ Shape { appearance PBRAppearance { baseColor 0.44 0.31 0.19 roughness 0.50 } geometry Sphere { radius 0.00148 } } ]
      }
      Transform {
        translation -0.0077 0.00290 0
        rotation 0 0 1 1.5708
        scale 1.28 1.00 0.96
        children [
          Shape {
            appearance PBRAppearance {
              baseColor 0.76 0.56 0.33
              roughness 0.44
            }
            geometry Capsule {
              radius 0.00245
              height 0.0118
            }
          }
        ]
      }
      Transform {
        translation -0.0135 0.00245 0
        scale 1.32 0.90 0.78
        children [
          Shape {
            appearance PBRAppearance { baseColor 0.49 0.34 0.20 roughness 0.50 }
            geometry Sphere { radius 0.00180 }
          }
        ]
      }
      Transform {
        translation -0.0153 0.00230 0
        scale 1.06 0.86 0.68
        children [ Shape { appearance PBRAppearance { baseColor 0.24 0.18 0.12 roughness 0.54 } geometry Sphere { radius 0.00105 } } ]
      }

      # Abdomen striping and ventral taper.
      Transform { translation -0.0055 0.00300 0 children [ Shape { appearance PBRAppearance { baseColor 0.17 0.13 0.09 roughness 0.64 } geometry Box { size 0.00066 0.0055 0.0059 } } ] }
      Transform { translation -0.0084 0.00282 0 children [ Shape { appearance PBRAppearance { baseColor 0.17 0.13 0.09 roughness 0.64 } geometry Box { size 0.00064 0.0052 0.0056 } } ] }
      Transform { translation -0.0110 0.00260 0 children [ Shape { appearance PBRAppearance { baseColor 0.17 0.13 0.09 roughness 0.64 } geometry Box { size 0.00060 0.0048 0.0052 } } ] }
      Transform { translation -0.0082 0.00158 0 children [ Shape { appearance PBRAppearance { baseColor 0.62 0.46 0.29 roughness 0.56 } geometry Sphere { radius 0.00116 } } ] }

      # Compound eyes
      Transform {
        translation 0.00895 0.00340 0.00225
        scale 1.00 1.05 0.96
        children [ Shape { appearance PBRAppearance { baseColor 0.86 0.12 0.10 roughness 0.14 } geometry Sphere { radius 0.00192 } } ]
      }
      Transform {
        translation 0.00895 0.00340 -0.00225
        scale 1.00 1.05 0.96
        children [ Shape { appearance PBRAppearance { baseColor 0.86 0.12 0.10 roughness 0.14 } geometry Sphere { radius 0.00192 } } ]
      }

      # Ocelli
      Transform { translation 0.00755 0.00492 0 children [ Shape { appearance PBRAppearance { baseColor 0.10 0.08 0.06 roughness 0.30 } geometry Sphere { radius 0.00024 } } ] }
      Transform { translation 0.00705 0.00470 0.00046 children [ Shape { appearance PBRAppearance { baseColor 0.10 0.08 0.06 roughness 0.30 } geometry Sphere { radius 0.00020 } } ] }
      Transform { translation 0.00705 0.00470 -0.00046 children [ Shape { appearance PBRAppearance { baseColor 0.10 0.08 0.06 roughness 0.30 } geometry Sphere { radius 0.00020 } } ] }

      # Antennae and aristae
      Transform { translation 0.00935 0.00305 0.00092 rotation 0 0 1 0.72 children [ Shape { appearance PBRAppearance { baseColor 0.73 0.59 0.40 roughness 0.74 } geometry Capsule { radius 0.00012 height 0.0020 } } ] }
      Transform { translation 0.00935 0.00305 -0.00092 rotation 0 0 1 -0.72 children [ Shape { appearance PBRAppearance { baseColor 0.73 0.59 0.40 roughness 0.74 } geometry Capsule { radius 0.00012 height 0.0020 } } ] }
      Transform { translation 0.01030 0.00408 0.00135 rotation 0 0 1 0.40 children [ Shape { appearance PBRAppearance { baseColor 0.18 0.15 0.11 roughness 0.78 } geometry Capsule { radius 0.00004 height 0.0021 } } ] }
      Transform { translation 0.01030 0.00408 -0.00135 rotation 0 0 1 -0.40 children [ Shape { appearance PBRAppearance { baseColor 0.18 0.15 0.11 roughness 0.78 } geometry Capsule { radius 0.00004 height 0.0021 } } ] }

      # Proboscis
      Transform { translation 0.00945 0.00170 0 rotation 0 0 1 0.10 children [ Shape { appearance PBRAppearance { baseColor 0.73 0.57 0.39 roughness 0.78 } geometry Capsule { radius 0.00016 height 0.0027 } } ] }

      # Halteres behind wing bases
      Transform { translation -0.0027 0.00480 0.00222 rotation 0 0 1 0.90 children [ Shape { appearance PBRAppearance { baseColor 0.80 0.72 0.53 roughness 0.62 } geometry Capsule { radius 0.00012 height 0.0020 } } ] }
      Transform { translation -0.0027 0.00480 -0.00222 rotation 0 0 1 -0.90 children [ Shape { appearance PBRAppearance { baseColor 0.80 0.72 0.53 roughness 0.62 } geometry Capsule { radius 0.00012 height 0.0020 } } ] }
      Transform { translation -0.00355 0.00570 0.00246 children [ Shape { appearance PBRAppearance { baseColor 0.84 0.73 0.50 roughness 0.52 } geometry Sphere { radius 0.00021 } } ] }
      Transform { translation -0.00355 0.00570 -0.00246 children [ Shape { appearance PBRAppearance { baseColor 0.84 0.73 0.50 roughness 0.52 } geometry Sphere { radius 0.00021 } } ] }
    """


def sanitize_proto_name(name: str) -> str:
    out = []
    for ch in name:
        if ch.isalnum() or ch == "_":
            out.append(ch)
        else:
            out.append("_")
    value = "".join(out).strip("_")
    if not value:
        value = "DrosophilaRobot"
    if value[0].isdigit():
        value = f"R_{value}"
    return value


def generate_proto(
    output_nodes: list[str],
    proto_path: Path,
    default_network_file: str,
    default_config_file: str,
    *,
    proto_name: str,
    brain_id: str,
    default_robot_name: str,
    include_compound_eyes: bool,
    eye_camera_width: int,
    eye_camera_height: int,
) -> None:
    motor_blocks = "".join(build_motor_block(i, name, len(output_nodes)) for i, name in enumerate(output_nodes))
    sensor_block = build_sensor_block(
        include_compound_eyes=include_compound_eyes,
        eye_camera_width=eye_camera_width,
        eye_camera_height=eye_camera_height,
    )
    visual_core = build_visual_core_body_block()
    articulated_wings = build_wing_joint("left") + build_wing_joint("right")
    articulated_legs = "".join([
        build_leg_chain("left", "front", 0.00490, 0.00235, 0.00220, -1.05, -0.80, 0.44, 0.52, 0.00210, 0.00390, 0.00590, 0.00480),
        build_leg_chain("right", "front", 0.00490, 0.00235, 0.00220, -1.05, -0.80, 0.44, 0.52, 0.00210, 0.00390, 0.00590, 0.00480),
        build_leg_chain("left", "mid", 0.00035, 0.00215, 0.00275, -0.05, 0.50, 0.62, 0.66, 0.00240, 0.00450, 0.00650, 0.00520),
        build_leg_chain("right", "mid", 0.00035, 0.00215, 0.00275, -0.05, 0.50, 0.62, 0.66, 0.00240, 0.00450, 0.00650, 0.00520),
        build_leg_chain("left", "rear", -0.00460, 0.00220, 0.00255, 0.52, 0.95, 0.82, 0.74, 0.00280, 0.00560, 0.00770, 0.00550),
        build_leg_chain("right", "rear", -0.00460, 0.00220, 0.00255, 0.52, 0.95, 0.82, 0.74, 0.00280, 0.00560, 0.00770, 0.00550),
    ])

    proto = f"""#VRML_SIM R2025a utf8

PROTO {proto_name} [
  field SFVec3f     translation 0 0.008 0
  field SFRotation  rotation 0 1 0 0
  field SFString    name \"{default_robot_name}\"
  field SFString    controller \"nao_nn_controller_uds\"
  field MFString    controllerArgs [
    \"NM_BRAINS={brain_id}\"
    \"NM_SENSORS_{brain_id}={DROS_SENSORS_REGEX}\"
    \"NM_ACTUATORS_{brain_id}={DROS_ACTUATORS_REGEX}\"
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
    customData {json.dumps(json.dumps({"network": default_network_file, "config": default_config_file}))}
    children [
{visual_core}
      # Articulated wings
{articulated_wings}
      # Articulated legs
{articulated_legs}
{sensor_block}
{motor_blocks}
      Group {{
        children IS extensionSlot
      }}
    ]
    boundingObject Group {{
      children [
        Transform {{
          translation 0.0009 0.0030 0
          rotation 0 0 1 1.5708
          children [ Capsule {{ radius 0.0032 height 0.0082 }} ]
        }}
        Transform {{
          translation 0.0076 0.0031 0
          children [ Sphere {{ radius 0.0025 }} ]
        }}
        Transform {{
          translation -0.0056 0.0030 0
          rotation 0 0 1 1.5708
          children [ Capsule {{ radius 0.0030 height 0.0118 }} ]
        }}
        Transform {{
          translation -0.0141 0.0026 0
          children [ Sphere {{ radius 0.0020 }} ]
        }}
      ]
    }}
    physics Physics {{
      density -1
      mass 0.0018
    }}
  }}
}}
"""
    proto_path.write_text(proto, encoding="utf-8")


def generate_world(
    world_path: Path,
    proto_a_path: Path,
    proto_b_path: Path,
    proto_a_name: str,
    proto_b_name: str,
    brain_a: str,
    brain_b: str,
    *,
    single_instance: bool = False,
) -> None:
    world = """#VRML_SIM R2025a utf8

EXTERNPROTO \"https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackground.proto\"
EXTERNPROTO \"https://raw.githubusercontent.com/cyberbotics/webots/R2025a/projects/objects/backgrounds/protos/TexturedBackgroundLight.proto\"
EXTERNPROTO \"../protos/DrosophilaRobot.proto\"

WorldInfo {
  basicTimeStep 16
}
Viewpoint {
  fieldOfView 0.92
  # Zero-roll camera: pitched downward only, so the horizon and ground stay level.
  orientation 0.999 0.045 0 1.18
  position 0.34 0.23 0.34
}
TexturedBackground {
}
TexturedBackgroundLight {
}
# Ground and floor detail. All large surfaces are axis-aligned and level to keep the scene visually horizontal.
Solid {
  translation 0 0.005 0
  name \"ground_base\"
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
  name \"grass_layer\"
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
  name \"orchard_soil_patch\"
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
Solid {
  translation 0 0.020 0
  name \"spawn_platform\"
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
Solid {
  translation -0.10 0.050 0
  name \"spawn_rail_west\"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.31 0.19 roughness 0.72 } geometry Box { size 0.020 0.060 0.32 } } ]
  boundingObject Box { size 0.020 0.060 0.32 }
}
Solid {
  translation 0 0.050 -0.15
  name \"spawn_rail_north\"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.31 0.19 roughness 0.72 } geometry Box { size 0.24 0.060 0.020 } } ]
  boundingObject Box { size 0.24 0.060 0.020 }
}
Solid {
  translation 0 0.050 0.15
  name \"spawn_rail_south\"
  children [ Shape { appearance PBRAppearance { baseColor 0.41 0.31 0.19 roughness 0.72 } geometry Box { size 0.24 0.060 0.020 } } ]
  boundingObject Box { size 0.24 0.060 0.020 }
}
Solid {
  translation 0 0.065 -0.96
  name \"berm_north\"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 2.10 0.13 0.10 } } ]
  boundingObject Box { size 2.10 0.13 0.10 }
}
Solid {
  translation 0 0.065 0.96
  name \"berm_south\"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 2.10 0.13 0.10 } } ]
  boundingObject Box { size 2.10 0.13 0.10 }
}
Solid {
  translation -0.96 0.065 0
  name \"berm_west\"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 0.10 0.13 2.10 } } ]
  boundingObject Box { size 0.10 0.13 2.10 }
}
Solid {
  translation 0.96 0.065 0
  name \"berm_east\"
  children [ Shape { appearance PBRAppearance { baseColor 0.43 0.34 0.22 roughness 0.86 } geometry Box { size 0.10 0.13 2.10 } } ]
  boundingObject Box { size 0.10 0.13 2.10 }
}
DrosophilaRobot {
  translation 0 0.044 0
  name \"Drosophila\"
  controller \"nao_nn_controller_uds\"
  controllerArgs [
    \"NM_BRAINS=default\"
  ]
}
"""
    proto_a_ref = os.path.relpath(proto_a_path.resolve(), world_path.parent.resolve()).replace("\\", "/")
    proto_b_ref = os.path.relpath(proto_b_path.resolve(), world_path.parent.resolve()).replace("\\", "/")
    extern_lines = [f"EXTERNPROTO \"{proto_a_ref}\""]
    if (not single_instance) and proto_b_ref != proto_a_ref:
        extern_lines.append(f"EXTERNPROTO \"{proto_b_ref}\"")
    world = world.replace(
        "EXTERNPROTO \"../protos/DrosophilaRobot.proto\"",
        "\n".join(extern_lines),
    )
    world = world.replace(
        "Viewpoint {\n  fieldOfView 0.92",
        "Viewpoint {\n  fieldOfView 0.92",
    )
    old_robot_block = """DrosophilaRobot {
  translation 0 0.044 0
  name \"Drosophila\"
  controller \"nao_nn_controller_uds\"
  controllerArgs [
    \"NM_BRAINS=default\"
  ]
}
"""
    if single_instance:
        new_robot_block = f"""{proto_a_name} {{
  translation 0 0.044 0
  name \"Drosophila\"
  controller \"nao_nn_controller_uds\"
  controllerArgs [
    \"NM_BRAINS={brain_a}\"
    \"NM_SENSORS_{brain_a}={DROS_SENSORS_REGEX}\"
    \"NM_ACTUATORS_{brain_a}={DROS_ACTUATORS_REGEX}\"
  ]
}}
"""
    else:
        new_robot_block = f"""{proto_a_name} {{
  translation -0.032 0.044 0
  name \"Drosophila_BANC\"
  controller \"nao_nn_controller_uds\"
  controllerArgs [
    \"NM_BRAINS={brain_a}\"
    \"NM_SENSORS_{brain_a}={DROS_SENSORS_REGEX}\"
    \"NM_ACTUATORS_{brain_a}={DROS_ACTUATORS_REGEX}\"
  ]
}}
{proto_b_name} {{
  translation 0.032 0.044 0
  name \"Drosophila_FAFB\"
  controller \"nao_nn_controller_uds\"
  controllerArgs [
    \"NM_BRAINS={brain_b}\"
    \"NM_SENSORS_{brain_b}={DROS_SENSORS_REGEX}\"
    \"NM_ACTUATORS_{brain_b}={DROS_ACTUATORS_REGEX}\"
  ]
}}
"""
    world = world.replace(old_robot_block, new_robot_block)
    world_path.write_text(world, encoding="utf-8")


def build_webots_config(snapshot: dict[str, Any], config_path: Path, output_count: int, sensory_count: int) -> None:
    net = dict(snapshot.get("net", {}))
    net["num_sensory_neurons"] = max(1, int(sensory_count))
    net["num_output_neurons"] = max(1, int(output_count))
    net.setdefault("growth_enabled", True)
    net.setdefault("morpho_growth_enabled", True)
    net.setdefault("use_morphology", True)
    net.setdefault("use_aarnn_delays", True)
    net["aarnn_layer_depth"] = max(1, int(net.get("aarnn_layer_depth", 0) or 0))
    net.setdefault("sleep_enabled", True)
    net.setdefault("aarnn_import_topology_rewire_enabled", True)
    net.setdefault("aarnn_import_topology_rewire_keep_fraction", 0.78)
    net.setdefault("aarnn_import_topology_rewire_region_bias", 0.24)
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
        help="Path to connectome snapshot JSON (single mode default, or network A fallback)",
    )
    parser.add_argument(
        "--network-a",
        default=None,
        help="Path to connectome snapshot JSON for network A",
    )
    parser.add_argument(
        "--network-b",
        default=None,
        help="Path to connectome snapshot JSON for network B (enables dual-fly mode)",
    )
    parser.add_argument(
        "--proto",
        default="webots_world/protos/DrosophilaRobot.proto",
        help="Output PROTO path (single mode default, or proto A fallback)",
    )
    parser.add_argument(
        "--proto-a",
        default=None,
        help="Output PROTO path for fly A",
    )
    parser.add_argument(
        "--proto-b",
        default="webots_world/protos/DrosophilaFafbRobot.proto",
        help="Output PROTO path for fly B (dual mode)",
    )
    parser.add_argument(
        "--world",
        default="webots_world/worlds/drosophila_neuroworld.wbt",
        help="Output world path",
    )
    parser.add_argument(
        "--config",
        default="webots_world/configs/config_drosophila_webots.json",
        help="Output config path (single mode default, or config A fallback)",
    )
    parser.add_argument(
        "--config-a",
        default=None,
        help="Output config path for fly A",
    )
    parser.add_argument(
        "--config-b",
        default="webots_world/configs/config_drosophila_fafb_webots.json",
        help="Output config path for fly B (dual mode)",
    )
    parser.add_argument(
        "--brain-a",
        default="default",
        help="Brain ID for fly A controllerArgs (default: default)",
    )
    parser.add_argument(
        "--brain-b",
        default="default",
        help="Brain ID for fly B controllerArgs (default: default)",
    )
    parser.add_argument(
        "--compound-eyes",
        default=os.environ.get("DROSOPHILA_EYE_CAMERAS", "on"),
        help="Enable left/right compound-eye cameras (on/off, default: on).",
    )
    parser.add_argument(
        "--eye-retina-width",
        type=int,
        default=int(os.environ.get("DROSOPHILA_EYE_RETINA_WIDTH", "12")),
        help="Event-retina columns per eye used for sensory channel mapping.",
    )
    parser.add_argument(
        "--eye-retina-height",
        type=int,
        default=int(os.environ.get("DROSOPHILA_EYE_RETINA_HEIGHT", "8")),
        help="Event-retina rows per eye used for sensory channel mapping.",
    )
    parser.add_argument(
        "--eye-camera-width",
        type=int,
        default=int(os.environ.get("DROSOPHILA_EYE_CAMERA_WIDTH", "32")),
        help="Webots camera width for each eye device.",
    )
    parser.add_argument(
        "--eye-camera-height",
        type=int,
        default=int(os.environ.get("DROSOPHILA_EYE_CAMERA_HEIGHT", "24")),
        help="Webots camera height for each eye device.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    include_compound_eyes = parse_bool(args.compound_eyes, default=True)
    eye_retina_cols = positive_int(args.eye_retina_width, "eye-retina-width")
    eye_retina_rows = positive_int(args.eye_retina_height, "eye-retina-height")
    eye_camera_width = positive_int(args.eye_camera_width, "eye-camera-width")
    eye_camera_height = positive_int(args.eye_camera_height, "eye-camera-height")
    sensory_ports = expected_sensory_channel_ports(
        include_compound_eyes=include_compound_eyes,
        eye_retina_rows=eye_retina_rows,
        eye_retina_cols=eye_retina_cols,
    )
    sensory_target = len(sensory_ports)

    network_a_path = Path(args.network_a or args.network)
    network_b_path = Path(args.network_b) if args.network_b else None
    proto_a_path = Path(args.proto_a or args.proto)
    proto_b_path = Path(args.proto_b)
    world_path = Path(args.world)
    config_a_path = Path(args.config_a or args.config)
    config_b_path = Path(args.config_b)
    dual_mode = network_b_path is not None

    if not network_a_path.exists():
        raise SystemExit(f"Missing network snapshot A: {network_a_path}")
    if dual_mode and not network_b_path.exists():
        raise SystemExit(f"Missing network snapshot B: {network_b_path}")

    snapshot_a = json.loads(network_a_path.read_text(encoding="utf-8"))
    sensory_nodes_a = sensory_nodes_from_snapshot(snapshot_a, sensory_target)
    output_nodes_a = output_nodes_from_snapshot(snapshot_a)
    snapshot_b = json.loads(network_b_path.read_text(encoding="utf-8")) if dual_mode else None
    sensory_nodes_b = sensory_nodes_from_snapshot(snapshot_b, sensory_target) if snapshot_b is not None else []
    output_nodes_b = output_nodes_from_snapshot(snapshot_b) if snapshot_b is not None else []

    proto_a_path.parent.mkdir(parents=True, exist_ok=True)
    if dual_mode:
        proto_b_path.parent.mkdir(parents=True, exist_ok=True)
    world_path.parent.mkdir(parents=True, exist_ok=True)
    config_a_path.parent.mkdir(parents=True, exist_ok=True)
    if dual_mode:
        config_b_path.parent.mkdir(parents=True, exist_ok=True)

    proto_a_name = sanitize_proto_name(proto_a_path.stem)
    proto_b_name = sanitize_proto_name(proto_b_path.stem if dual_mode else proto_a_path.stem)

    default_network_a = os.path.relpath(network_a_path.resolve(), proto_a_path.parent.resolve()).replace("\\", "/")
    default_config_a = os.path.relpath(config_a_path.resolve(), proto_a_path.parent.resolve()).replace("\\", "/")
    generate_proto(
        output_nodes_a,
        proto_a_path,
        default_network_a,
        default_config_a,
        proto_name=proto_a_name,
        brain_id=args.brain_a,
        default_robot_name="drosophila_banc_robot" if dual_mode else "drosophila_robot",
        include_compound_eyes=include_compound_eyes,
        eye_camera_width=eye_camera_width,
        eye_camera_height=eye_camera_height,
    )
    if dual_mode:
        default_network_b = os.path.relpath(network_b_path.resolve(), proto_b_path.parent.resolve()).replace("\\", "/")
        default_config_b = os.path.relpath(config_b_path.resolve(), proto_b_path.parent.resolve()).replace("\\", "/")
        generate_proto(
            output_nodes_b,
            proto_b_path,
            default_network_b,
            default_config_b,
            proto_name=proto_b_name,
            brain_id=args.brain_b,
            default_robot_name="drosophila_fafb_robot",
            include_compound_eyes=include_compound_eyes,
            eye_camera_width=eye_camera_width,
            eye_camera_height=eye_camera_height,
        )

    generate_world(
        world_path,
        proto_a_path=proto_a_path,
        proto_b_path=proto_b_path if dual_mode else proto_a_path,
        proto_a_name=proto_a_name,
        proto_b_name=proto_b_name,
        brain_a=args.brain_a,
        brain_b=args.brain_b if dual_mode else args.brain_a,
        single_instance=(not dual_mode),
    )
    wbproj_path = world_path.with_name(f".{world_path.stem}.wbproj")
    if wbproj_path.exists():
        wbproj_path.unlink()
    build_webots_config(snapshot_a, config_a_path, len(output_nodes_a), sensory_target)
    io_map_a = build_io_alignment_map(
        snapshot_a,
        sensory_nodes_a,
        output_nodes_a,
        args.brain_a,
        include_compound_eyes=include_compound_eyes,
        eye_retina_rows=eye_retina_rows,
        eye_retina_cols=eye_retina_cols,
    )
    io_map_a_path = config_a_path.with_name(f"{config_a_path.stem}.io_alignment.json")
    io_map_a_path.write_text(json.dumps(io_map_a, indent=2), encoding="utf-8")
    if dual_mode and snapshot_b is not None:
        build_webots_config(snapshot_b, config_b_path, len(output_nodes_b), sensory_target)
        io_map_b = build_io_alignment_map(
            snapshot_b,
            sensory_nodes_b,
            output_nodes_b,
            args.brain_b,
            include_compound_eyes=include_compound_eyes,
            eye_retina_rows=eye_retina_rows,
            eye_retina_cols=eye_retina_cols,
        )
        io_map_b_path = config_b_path.with_name(f"{config_b_path.stem}.io_alignment.json")
        io_map_b_path.write_text(json.dumps(io_map_b, indent=2), encoding="utf-8")

    if dual_mode:
        print(
            f"Wrote {proto_a_path} ({len(output_nodes_a)} motors), {proto_b_path} ({len(output_nodes_b)} motors), "
            f"and {world_path}; sensory target={sensory_target}; "
            f"configs: {config_a_path}, {config_b_path}; "
            f"io maps: {config_a_path.with_name(f'{config_a_path.stem}.io_alignment.json')}, "
            f"{config_b_path.with_name(f'{config_b_path.stem}.io_alignment.json')}"
        )
    else:
        print(
            f"Wrote {proto_a_path} and {world_path} with {len(output_nodes_a)} motor outputs; "
            f"sensory target={sensory_target}; config at {config_a_path}; "
            f"io map at {config_a_path.with_name(f'{config_a_path.stem}.io_alignment.json')}"
        )


if __name__ == "__main__":
    main()
