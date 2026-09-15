#!/usr/bin/env python3
"""
Generate Webots assets for the C. elegans connectome snapshot.

Outputs:
  - webots_world/protos/CelegansRobot.proto
  - webots_world/worlds/celegans_neuroworld.wbt
  - webots_world/configs/config_celegans_webots.json
"""

from __future__ import annotations

from sim_content import worm_segment_details, reference_world

import argparse
import json
import os
import re
from pathlib import Path
from typing import Any


CELEGANS_SENSOR_PREFIX = "celegans_s"
CELEGANS_OUTPUT_PREFIX = "celegans_o"
CELEGANS_SENSORS_REGEX = r"^celegans_s_[0-9]{2}_.*$"
CELEGANS_ACTUATORS_REGEX = r"^celegans_o_[0-9]{3}_.*$"
CELEGANS_SENSORY_CHANNELS = [
    "celegans_s_00_vibration_accel.x",
    "celegans_s_00_vibration_accel.y",
    "celegans_s_00_vibration_accel.z",
    "celegans_s_03_vibration_gyro.x",
    "celegans_s_03_vibration_gyro.y",
    "celegans_s_03_vibration_gyro.z",
    "celegans_s_06_touch_front",
    "celegans_s_07_touch_rear",
    "celegans_s_08_light_left",
    "celegans_s_09_light_right",
    "celegans_s_10_heat_left",
    "celegans_s_11_heat_right",
    "celegans_s_12_taste_front_center",
    "celegans_s_13_taste_front_left",
    "celegans_s_14_taste_front_right",
    "celegans_s_15_chem_left",
    "celegans_s_16_chem_right",
    "celegans_s_17_chem_rear_left",
    "celegans_s_18_chem_rear_right",
    "celegans_s_19_flow_front",
    "celegans_s_20_flow_rear",
    "celegans_s_21_far_front_left",
    "celegans_s_22_far_front_right",
    "celegans_s_23_far_rear",
]

SPINE_SEGMENTS = 24
SPINE_LENGTH = 0.34
SPINE_RADIUS = 0.021
SPINE_JOINT_LIMIT = 0.38
SPINE_JOINT_MAX_VELOCITY = 1.2
SPINE_JOINT_MAX_TORQUE = 0.006


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
            out.append(ch)
        else:
            out.append("_")
    value = "".join(out).strip("_")
    return value or "MUSCLE"


def output_motor_name(index: int, node_id: str) -> str:
    return f"{CELEGANS_OUTPUT_PREFIX}_{index:03d}_{sanitize_device_name(node_id).upper()}"


def segment_index_from_output_label(label: str, fallback_index: int, total: int) -> int:
    upper = label.upper()
    m = re.match(r"^M[DV][LR](\d{2})$", upper)
    if m:
        return max(1, min(SPINE_SEGMENTS, int(m.group(1))))
    if upper == "MVULVA":
        return max(1, min(SPINE_SEGMENTS, SPINE_SEGMENTS // 2))
    # Fallback: spread unknown outputs across the body.
    return 1 + int((SPINE_SEGMENTS - 1) * (fallback_index / max(1, total - 1)))


def muscle_anchor_from_label(label: str, fallback_index: int, total: int) -> tuple[float, float, float]:
    seg_idx = segment_index_from_output_label(label, fallback_index, total)
    x = -SPINE_LENGTH * 0.5 + SPINE_LENGTH * ((seg_idx - 1) / max(1, SPINE_SEGMENTS - 1))
    upper = label.upper()
    if "L" in upper:
        z = 0.030
    elif "R" in upper:
        z = -0.030
    else:
        z = 0.0
    # Dorsal sits slightly above the body centerline, ventral slightly below.
    y = 0.014 if "D" in upper else -0.010
    return x, y, z


def build_motor_block(index: int, node_id: str, total: int) -> str:
    x, y, z = muscle_anchor_from_label(node_id, index, total)
    motor_name = output_motor_name(index, node_id)
    def_name = sanitize_def(f"muscle_joint_{index}_{motor_name}")
    end_name = sanitize_def(f"muscle_segment_{index}_{motor_name}")
    return f"""    DEF {def_name} HingeJoint {{
      jointParameters HingeJointParameters {{
        anchor {x:.5f} {y:.5f} {z:.5f}
        axis 0 1 0
      }}
      device [
        RotationalMotor {{
          name "{motor_name}"
          maxVelocity 6
          minPosition -1.2
          maxPosition 1.2
          maxTorque 0.001
        }}
      ]
      # Channel-only muscle output joint: no collision body/physics.
      # It exists so celegans_o_* outputs are discoverable by DeviceMapper,
      # while bend mechanics are applied through celegans_spine_* motors.
      endPoint DEF {end_name} Solid {{
        name "{end_name}"
        translation {x:.5f} {y:.5f} {z:.5f}
        children [ ]
      }}
    }}
"""


def _spine_radius(seg_idx: int) -> float:
    # Slight taper toward tail/head; fattest around the center.
    center = (SPINE_SEGMENTS - 1) * 0.5
    d = abs(seg_idx - center) / max(1.0, center)
    return SPINE_RADIUS * (1.0 - 0.34 * d)


def _spine_segment_solid(
    seg_idx: int,
    seg_len: float,
    child_joint_block: str,
    *,
    translate_x: float,
    indent: str,
) -> str:
    r = _spine_radius(seg_idx)
    mass = max(0.0035, 0.0068 * (r / SPINE_RADIUS))
    # The rendered capsules overlap to form a continuous skin.  Do not reuse
    # them as collision geometry: their diameter is larger than the segment
    # pitch, so ODE starts with every neighbouring body deeply intersecting.
    collision_len = seg_len * 0.88
    collision_radius = r * 0.82
    ridge_offset = r * 0.80
    ridge_h = max(0.0018, r * 0.16)
    ridge_w = max(0.0022, r * 0.24)
    ridge_len = seg_len * 0.78
    return f"""{indent}Solid {{
{indent}  name "celegans_spine_seg_{seg_idx + 1:02d}"
{indent}  translation {translate_x:.5f} 0.0 0.0
{indent}  children [
{indent}    Transform {{
{indent}      rotation 0 0 1 1.5708
{indent}      children [
{indent}        Shape {{
{indent}          appearance PBRAppearance {{
{indent}            baseColor 0.76 0.64 0.42
{indent}            transparency IS skinTransparency
{indent}            roughness 0.72
{indent}            metalness 0
{indent}          }}
{indent}          geometry Capsule {{
{indent}            radius {r:.5f}
{indent}            height {seg_len * 0.92:.5f}
{indent}          }}
{indent}        }}
{indent}      ]
{indent}    }}
{indent}    # Lateral alae-like ridges improve directional traction during
{indent}    # dorsoventral undulation on flat surfaces.
{indent}    Transform {{
{indent}      translation 0.0 0.0 {ridge_offset:.5f}
{indent}      children [
{indent}        Shape {{
{indent}          appearance PBRAppearance {{
{indent}            baseColor 0.85 0.62 0.40
{indent}            roughness 0.68
{indent}            metalness 0
{indent}          }}
{indent}          geometry Box {{
{indent}            size {ridge_len:.5f} {ridge_h:.5f} {ridge_w:.5f}
{indent}          }}
{indent}        }}
{indent}      ]
{indent}    }}
{indent}    Transform {{
{indent}      translation 0.0 0.0 {-ridge_offset:.5f}
{indent}      children [
{indent}        Shape {{
{indent}          appearance PBRAppearance {{
{indent}            baseColor 0.85 0.62 0.40
{indent}            roughness 0.68
{indent}            metalness 0
{indent}          }}
{indent}          geometry Box {{
{indent}            size {ridge_len:.5f} {ridge_h:.5f} {ridge_w:.5f}
{indent}          }}
{indent}        }}
{indent}      ]
{indent}    }}
{worm_segment_details(seg_idx, SPINE_LENGTH, r)}
{child_joint_block}
{indent}  ]
{indent}  # A short hull keeps adjacent articulated bodies disjoint while the
{indent}  # overlapping visual capsules provide a coherent outer surface.
{indent}  boundingObject Box {{
{indent}    size {collision_len:.5f} {collision_radius * 2.0:.5f} {collision_radius * 2.0:.5f}
{indent}  }}
{indent}  physics Physics {{
{indent}    density -1
{indent}    mass {mass:.5f}
{indent}    damping Damping {{
{indent}      linear 0.18
{indent}      angular 0.42
{indent}    }}
{indent}  }}
{indent}}}"""


def build_spine_block() -> str:
    seg_len = SPINE_LENGTH / SPINE_SEGMENTS
    # Build nested chain from tail toward head.
    chain = _spine_segment_solid(
        SPINE_SEGMENTS - 1,
        seg_len,
        child_joint_block="",
        translate_x=seg_len,
        indent="        ",
    )

    for seg_idx in range(SPINE_SEGMENTS - 2, -1, -1):
        joint_index = seg_idx + 1
        child_joint = f"""
        HingeJoint {{
          jointParameters HingeJointParameters {{
            anchor {seg_len * 0.5:.5f} 0.0 0.0
            axis 0 1 0
            dampingConstant 0.0015
            staticFriction 0.0002
          }}
          device [
            RotationalMotor {{
              name "celegans_spine_{joint_index:02d}"
              minPosition {-SPINE_JOINT_LIMIT:.2f}
              maxPosition {SPINE_JOINT_LIMIT:.2f}
              maxVelocity {SPINE_JOINT_MAX_VELOCITY:.1f}
              maxTorque {SPINE_JOINT_MAX_TORQUE:.3f}
            }}
          ]
          endPoint
{chain}
        }}"""
        translate_x = -SPINE_LENGTH * 0.5 if seg_idx == 0 else seg_len
        chain = _spine_segment_solid(
            seg_idx,
            seg_len,
            child_joint_block=child_joint,
            translate_x=translate_x,
            indent="      " if seg_idx == 0 else "        ",
        )

    # Anchor the articulated spine to the Robot root via a locked root joint.
    # This keeps the visible spine and root collision body in one rigid assembly.
    return f"""      HingeJoint {{
        jointParameters HingeJointParameters {{
          anchor 0.0 0.0 0.0
          axis 0 1 0
          dampingConstant 0.004
          staticFriction 0.0005
        }}
        device [
          RotationalMotor {{
            name "celegans_spine_root_lock"
            minPosition 0
            maxPosition 0
            maxVelocity 1.2
            maxTorque 0.02
          }}
        ]
        endPoint
{chain}
      }}"""


def build_sensor_block() -> str:
    distance_specs = [
        # name, tx, tz, yaw, max_range
        (f"{CELEGANS_SENSOR_PREFIX}_12_taste_front_center", 0.182, 0.000, 0.00, 0.26),
        (f"{CELEGANS_SENSOR_PREFIX}_13_taste_front_left", 0.168, 0.030, 0.36, 0.23),
        (f"{CELEGANS_SENSOR_PREFIX}_14_taste_front_right", 0.168, -0.030, -0.36, 0.23),
        (f"{CELEGANS_SENSOR_PREFIX}_15_chem_left", 0.060, 0.046, 1.20, 0.38),
        (f"{CELEGANS_SENSOR_PREFIX}_16_chem_right", 0.060, -0.046, -1.20, 0.38),
        (f"{CELEGANS_SENSOR_PREFIX}_17_chem_rear_left", -0.074, 0.030, 2.46, 0.36),
        (f"{CELEGANS_SENSOR_PREFIX}_18_chem_rear_right", -0.074, -0.030, -2.46, 0.36),
        (f"{CELEGANS_SENSOR_PREFIX}_19_flow_front", 0.126, 0.000, 0.00, 0.92),
        (f"{CELEGANS_SENSOR_PREFIX}_20_flow_rear", -0.126, 0.000, 3.14159, 0.92),
        (f"{CELEGANS_SENSOR_PREFIX}_21_far_front_left", 0.110, 0.078, 0.95, 1.28),
        (f"{CELEGANS_SENSOR_PREFIX}_22_far_front_right", 0.110, -0.078, -0.95, 1.28),
        (f"{CELEGANS_SENSOR_PREFIX}_23_far_rear", -0.168, 0.000, 3.14159, 1.30),
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
        name "{CELEGANS_SENSOR_PREFIX}_00_vibration_accel"
      }}
      Gyro {{
        name "{CELEGANS_SENSOR_PREFIX}_03_vibration_gyro"
      }}
      TouchSensor {{
        name "{CELEGANS_SENSOR_PREFIX}_06_touch_front"
        type "bumper"
        translation 0.19 0.0 0.0
      }}
      TouchSensor {{
        name "{CELEGANS_SENSOR_PREFIX}_07_touch_rear"
        type "bumper"
        translation -0.19 0.0 0.0
      }}
      # Phototaxis channels.
      LightSensor {{
        name "{CELEGANS_SENSOR_PREFIX}_08_light_left"
        translation 0.172 0.012 0.030
      }}
      LightSensor {{
        name "{CELEGANS_SENSOR_PREFIX}_09_light_right"
        translation 0.172 0.012 -0.030
      }}
      # Thermotaxis equivalents (modeled as warm-spectrum irradiance sensors).
      LightSensor {{
        name "{CELEGANS_SENSOR_PREFIX}_10_heat_left"
        translation 0.095 0.016 0.040
      }}
      LightSensor {{
        name "{CELEGANS_SENSOR_PREFIX}_11_heat_right"
        translation 0.095 0.016 -0.040
      }}
      # Multi-directional taste/chemical/flow proximity sensing.
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
    spine_block = build_spine_block()
    proto = f"""#VRML_SIM R2025a utf8

PROTO CelegansRobot [
  field SFVec3f     translation 0 0.038 0
  field SFRotation  rotation 0 1 0 0
  field SFString    name "celegans_robot"
  field SFString    controller "nao_nn_controller_uds"
  field MFString    controllerArgs [
    "NM_BRAINS=default"
    "NM_SENSORS_default={CELEGANS_SENSORS_REGEX}"
    "NM_ACTUATORS_default={CELEGANS_ACTUATORS_REGEX}"
  ]
  field SFFloat     skinTransparency 0
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
      # Segmented spine (driven indirectly from output muscle channels).
{spine_block}
      # Sensory pathways mapped to celegans_s_* channels.
{sensor_block}
      # Output muscle channels mapped one-to-one to celegans_o_* motors.
{motor_blocks}
      Group {{
        children IS extensionSlot
      }}
    ]
    # Keep the robot root collision minimal; articulated spine segments carry
    # the main body collision/physics.
    boundingObject Sphere {{
      radius 0.006
    }}
    physics Physics {{
      density -1
      mass 0.002
    }}
  }}
}}
"""
    proto_path.write_text(proto, encoding="utf-8")


def generate_world(world_path: Path) -> None:
    world_path.write_text(reference_world("celegans", "CelegansRobot", "../protos/CelegansRobot.proto", [
        "NM_BRAINS=default", f"NM_SENSORS_default={CELEGANS_SENSORS_REGEX}",
        f"NM_ACTUATORS_default={CELEGANS_ACTUATORS_REGEX}"]), encoding="utf-8")


def build_webots_config(snapshot: dict[str, Any], config_path: Path) -> None:
    net = dict(snapshot.get("net", {}))
    sensory_target = len(CELEGANS_SENSORY_CHANNELS)
    net["num_sensory_neurons"] = sensory_target
    net["num_output_neurons"] = max(1, int(net.get("num_output_neurons", 0)))
    net["num_hidden_layers"] = 1
    net["num_hidden_per_layer_initial"] = int(net.get("num_hidden_per_layer_initial", 302) or 302)
    net.setdefault("growth_enabled", True)
    net.setdefault("morpho_growth_enabled", True)
    net.setdefault("use_morphology", True)
    net.setdefault("use_aarnn_delays", True)
    net["aarnn_layer_depth"] = max(1, int(net.get("aarnn_layer_depth", 0) or 0))
    net.setdefault("sleep_enabled", True)
    net.setdefault("aarnn_import_topology_rewire_enabled", True)
    net.setdefault("aarnn_import_topology_rewire_keep_fraction", 0.74)
    net.setdefault("aarnn_import_topology_rewire_region_bias", 0.30)
    config_path.write_text(json.dumps(net, indent=2), encoding="utf-8")


def build_io_alignment_map(snapshot: dict[str, Any], output_nodes: list[str]) -> dict[str, Any]:
    labels = snapshot.get("connectome_labels")
    dataset = labels.get("dataset") if isinstance(labels, dict) else None
    sensory_nodes = []
    if isinstance(labels, dict):
        raw_nodes = labels.get("sensory_nodes")
        if isinstance(raw_nodes, list):
            sensory_nodes = [str(node) for node in raw_nodes]
    if len(sensory_nodes) < len(CELEGANS_SENSORY_CHANNELS):
        sensory_nodes += [f"sensory_pad_{i:03d}" for i in range(len(CELEGANS_SENSORY_CHANNELS) - len(sensory_nodes))]
    sensory_nodes = sensory_nodes[: len(CELEGANS_SENSORY_CHANNELS)]
    output_names = [output_motor_name(i, node_id) for i, node_id in enumerate(output_nodes)]

    return {
        "dataset": dataset,
        "brain_id": "default",
        "sensor_regex": CELEGANS_SENSORS_REGEX,
        "actuator_regex": CELEGANS_ACTUATORS_REGEX,
        "sensory_channels": [
            {
                "channel_index": i,
                "connectome_node_id": sensory_nodes[i],
                "device_port": CELEGANS_SENSORY_CHANNELS[i],
            }
            for i in range(len(CELEGANS_SENSORY_CHANNELS))
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
    io_map = build_io_alignment_map(snapshot, output_nodes)
    io_map_path = config_path.with_name(f"{config_path.stem}.io_alignment.json")
    io_map_path.write_text(json.dumps(io_map, indent=2), encoding="utf-8")

    print(
        f"Wrote {proto_path} and {world_path} with {len(output_nodes)} motor outputs; "
        f"config at {config_path}; io map at {io_map_path}"
    )


if __name__ == "__main__":
    main()
