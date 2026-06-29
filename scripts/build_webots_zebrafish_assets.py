#!/usr/bin/env python3
"""
Generate Webots assets for the zebrafish connectome snapshot.

Outputs:
  - webots_world/protos/ZebrafishRobot.proto
  - webots_world/worlds/zebrafish_neuroworld.wbt
  - webots_world/configs/config_zebrafish_webots.json
  - webots_world/configs/config_zebrafish_webots.io_alignment.json

Robot design: larval Danio rerio (~80 mm body length in Webots scale).
  - Elongated capsule body
  - 8 articulated tail segments (undulatory swimming CPG outputs)
  - Pectoral fins (pitch + roll, L/R)
  - Dorsal and caudal fins
  - Lateral line DistanceSensors (8 each side) simulating neuromast response
  - PositionSensor cameras → simple LightSensors for eyes (L/R)
  - Accelerometer + Gyro (inertial)
  - Olfactory InertialUnit equivalents (nose)

Sensory channels (32): zebrafish_s_00_* … zebrafish_s_31_*
Output channels  (32): zebrafish_o_00_* … zebrafish_o_31_*
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, List, Tuple

# ---------------------------------------------------------------------------
# Channel name lists (must match build_zebrafish_network_json.py exactly)
# ---------------------------------------------------------------------------

SENSORY_CHANNELS: List[str] = [
    "zebrafish_s_00_lateralline_l0", "zebrafish_s_01_lateralline_l1",
    "zebrafish_s_02_lateralline_l2", "zebrafish_s_03_lateralline_l3",
    "zebrafish_s_04_lateralline_l4", "zebrafish_s_05_lateralline_l5",
    "zebrafish_s_06_lateralline_l6", "zebrafish_s_07_lateralline_l7",
    "zebrafish_s_08_lateralline_r0", "zebrafish_s_09_lateralline_r1",
    "zebrafish_s_10_lateralline_r2", "zebrafish_s_11_lateralline_r3",
    "zebrafish_s_12_lateralline_r4", "zebrafish_s_13_lateralline_r5",
    "zebrafish_s_14_lateralline_r6", "zebrafish_s_15_lateralline_r7",
    "zebrafish_s_16_eye_left_lum",  "zebrafish_s_17_eye_left_grad",
    "zebrafish_s_18_eye_right_lum", "zebrafish_s_19_eye_right_grad",
    "zebrafish_s_20_olfactory_l",   "zebrafish_s_21_olfactory_r",
    "zebrafish_s_22_olfactory_front","zebrafish_s_23_olfactory_rear",
    "zebrafish_s_24_flow_anterior",  "zebrafish_s_25_flow_posterior",
    "zebrafish_s_26_pressure_depth", "zebrafish_s_27_pressure_pitch",
    # Axis-suffix convention (.x/.y) tells DeviceMapper to extract one axis only
    # → each device contributes exactly 1 sensory channel (S=32, not S=40).
    "zebrafish_s_28_accel.x",        "zebrafish_s_29_accel.y",
    "zebrafish_s_30_gyro.x",         "zebrafish_s_31_gyro.y",
]

OUTPUT_CHANNELS: List[str] = [
    "zebrafish_o_00_tail_l0", "zebrafish_o_01_tail_r0",
    "zebrafish_o_02_tail_l1", "zebrafish_o_03_tail_r1",
    "zebrafish_o_04_tail_l2", "zebrafish_o_05_tail_r2",
    "zebrafish_o_06_tail_l3", "zebrafish_o_07_tail_r3",
    "zebrafish_o_08_tail_l4", "zebrafish_o_09_tail_r4",
    "zebrafish_o_10_tail_l5", "zebrafish_o_11_tail_r5",
    "zebrafish_o_12_tail_l6", "zebrafish_o_13_tail_r6",
    "zebrafish_o_14_tail_l7", "zebrafish_o_15_tail_r7",
    "zebrafish_o_16_pec_fin_l_pitch","zebrafish_o_17_pec_fin_l_roll",
    "zebrafish_o_18_pec_fin_r_pitch","zebrafish_o_19_pec_fin_r_roll",
    "zebrafish_o_20_dorsal_fin_l",   "zebrafish_o_21_dorsal_fin_r",
    "zebrafish_o_22_caudal_fin_l",   "zebrafish_o_23_caudal_fin_r",
    "zebrafish_o_24_pelvic_fin_l",   "zebrafish_o_25_pelvic_fin_r",
    "zebrafish_o_26_pelvic_fin_l2",  "zebrafish_o_27_pelvic_fin_r2",
    "zebrafish_o_28_jaw",            "zebrafish_o_29_operculum_l",
    "zebrafish_o_30_operculum_r",    "zebrafish_o_31_trunk_stiffness",
]

NUM_SENSORY = len(SENSORY_CHANNELS)
NUM_OUTPUT  = len(OUTPUT_CHANNELS)

# Robot physical dimensions (Webots metres)
BODY_LENGTH = 0.080    # 80 mm — larval zebrafish scaled up for physics stability
BODY_RADIUS = 0.008    # 8 mm
TAIL_SEGMENTS  = 8               # logical motor segments (output channel pairs)
PHYS_SEGMENTS  = 4               # PHYSICAL joints in the tail spine (ODE cost ∝ chain depth)
TAIL_LENGTH    = BODY_LENGTH * 0.62
HEAD_LENGTH    = BODY_LENGTH - TAIL_LENGTH
SEG_LEN        = TAIL_LENGTH / TAIL_SEGMENTS   # spacing for channel-only motors
PHYS_SEG_LEN   = TAIL_LENGTH / PHYS_SEGMENTS   # spacing for physical spine joints
FIN_SPAN       = BODY_RADIUS * 2.8

# First PHYS_SEGMENTS L-channel names are mapped to physical spine joints.
# All remaining channels (R-sides + l4-l7 + fins) become channel-only joints.
PHYS_L_CHANNELS = [OUTPUT_CHANNELS[i * 2] for i in range(PHYS_SEGMENTS)]


# ---------------------------------------------------------------------------
# PROTO body
# ---------------------------------------------------------------------------

def _body_colour(i: int, total: int) -> Tuple[float, float, float]:
    """Zebra-stripe colouring along tail."""
    t = i / max(1, total - 1)
    stripe = (i % 2 == 0)
    r = 0.15 if stripe else 0.92
    g = 0.15 if stripe else 0.88
    b = 0.20 if stripe else 0.65
    return r, g, b


def _tail_segment_radius(seg: int) -> float:
    """Taper from body junction to caudal peduncle."""
    t = seg / TAIL_SEGMENTS
    return BODY_RADIUS * (1.0 - 0.60 * t)


def build_lateral_line_sensors(side: str, n: int) -> str:
    """8 DistanceSensors along each flank, sampling local water-pressure gradients."""
    lines = []
    for i in range(n):
        frac  = (i + 0.5) / n
        x_pos = -HEAD_LENGTH * 0.5 - TAIL_LENGTH * frac
        y_pos = 0.0
        z_pos = (_tail_segment_radius(int(frac * TAIL_SEGMENTS)) + 0.001) * (1 if side == "l" else -1)
        name  = f"zebrafish_s_{i:02d}_lateralline_{side}{i}" if side == "l" else \
                f"zebrafish_s_{i+8:02d}_lateralline_{side}{i}"
        lines.append(f"""      Transform {{
        translation {x_pos:.5f} {y_pos:.5f} {z_pos:.5f}
        children [
          DistanceSensor {{
            name "{name}"
            lookupTable [0 0 0, 0.05 1000 0]
            type "generic"
            aperture 0.4
            numberOfRays 3
          }}
        ]
      }}""")
    return "\n".join(lines)


def build_eye_sensors() -> str:
    """Bilateral Camera eyes for retinal event encoding via DeviceMapper.

    Each 1×1-pixel Camera is processed by the camera event encoder into 2
    sensory channels (ON + OFF polarity per pixel): left eye → channels 16–17,
    right eye → channels 18–19.  Both project to the optic tectum in the brain.

    fieldOfView ≈ 163° — wide lateral angle matching real zebrafish.
    Per-camera retina size is overridden to 1×1 via env vars exported by
    run_multi_robot_webots.sh so S=32 is preserved.
    """
    eye_specs = [
        ("zebrafish_eye_left",   HEAD_LENGTH * 0.28,  BODY_RADIUS * 0.72,  0.28),
        ("zebrafish_eye_right",  HEAD_LENGTH * 0.28, -BODY_RADIUS * 0.72, -0.28),
    ]
    lines = []
    for cam_name, ex, ez, yaw in eye_specs:
        lines.append(f"""      Transform {{
        translation {ex:.5f} {BODY_RADIUS * 0.12:.5f} {ez:.5f}
        rotation 0 1 0 {yaw:.4f}
        children [
          Camera {{
            name "{cam_name}"
            width 1
            height 1
            fieldOfView 2.85
            near 0.001
          }}
        ]
      }}""")
    return "\n".join(lines)


def build_olfactory_sensors() -> str:
    """DistanceSensors at the snout modelling chemo/olfactory gradient."""
    specs = [
        ("zebrafish_s_20_olfactory_l",      HEAD_LENGTH*0.55,  BODY_RADIUS*0.45,  0.0,  0.40),
        ("zebrafish_s_21_olfactory_r",      HEAD_LENGTH*0.55, -BODY_RADIUS*0.45,  0.0,  0.40),
        ("zebrafish_s_22_olfactory_front",  HEAD_LENGTH*0.60,  0.0,  0.0,          0.60),
        ("zebrafish_s_23_olfactory_rear",   HEAD_LENGTH*0.10,  0.0,  0.0,          0.30),
    ]
    lines = []
    for name, ex, ez, ey, aperture in specs:
        lines.append(f"""      Transform {{
        translation {ex:.5f} {ey:.5f} {ez:.5f}
        children [
          DistanceSensor {{
            name "{name}"
            lookupTable [0 0 0, 0.30 1000 0]
            type "generic"
            aperture {aperture:.2f}
          }}
        ]
      }}""")
    return "\n".join(lines)


def build_imu_sensors() -> str:
    """Accelerometer + Gyroscope modelling the otolith/semicircular-canal system.

    Axis-suffix convention in device names (.x / .y) tells DeviceMapper to
    extract only the specified axis from the multi-axis sensor, giving exactly
    1 channel per device (S=32 total, not S=40).
    """
    lines = [
        f'      Accelerometer {{ name "zebrafish_s_28_accel.x" }}',
        f'      Accelerometer {{ name "zebrafish_s_29_accel.y" }}',
        f'      Gyro {{ name "zebrafish_s_30_gyro.x" }}',
        f'      Gyro {{ name "zebrafish_s_31_gyro.y" }}',
        f'      InertialUnit {{ name "zebrafish_imu" }}',
    ]
    return "\n".join(lines)


def build_flow_pressure_sensors() -> str:
    """Simulate hydrostatic pressure (depth) and flow (velocity) sensing."""
    lines = [
        f'      DistanceSensor {{ name "zebrafish_s_24_flow_anterior"  lookupTable [0 0 0, 0.10 1000 0] type "generic" }}',
        f'      DistanceSensor {{ name "zebrafish_s_25_flow_posterior" lookupTable [0 0 0, 0.10 1000 0] type "generic" }}',
        f'      DistanceSensor {{ name "zebrafish_s_26_pressure_depth" lookupTable [0 0 0, 2.00 1000 0] type "generic" }}',
        f'      DistanceSensor {{ name "zebrafish_s_27_pressure_pitch" lookupTable [0 0 0, 0.50 1000 0] type "generic" }}',
    ]
    return "\n".join(lines)


def _channel_only_joint(ch_name: str, x: float, y: float, z: float) -> str:
    """Flat (non-chained) channel-only HingeJoint following the celegans pattern.

    The endPoint Solid has no bounding object and no physics node, so ODE
    ignores it entirely — but DeviceMapper can still discover the RotationalMotor
    and route AARNN spike outputs to it.  The joint IS real: commands are
    received and the motor position can be read back, enabling the brain to
    learn correlations between motor commands and body state.
    """
    safe = ch_name.replace(".", "_").replace("-", "_")
    return f"""      HingeJoint {{
        jointParameters HingeJointParameters {{
          anchor {x:.5f} {y:.5f} {z:.5f}
          axis 0 1 0
        }}
        device [
          RotationalMotor {{
            name "{ch_name}"
            minPosition -1.57
            maxPosition  1.57
            maxVelocity  8.0
            maxTorque    0.00005
          }}
        ]
        endPoint Solid {{
          name "chan_{safe}"
          translation {x:.5f} {y:.5f} {z:.5f}
          children []
        }}
      }}"""


def build_pectoral_fin_joint(side: str, x_pos: float) -> str:
    """Pectoral fin: visual shape only (no physics joints — channel-only motors
    for pitch/roll are registered in build_channel_only_joints)."""
    z_sign = 1 if side == "l" else -1
    fin_z = BODY_RADIUS * 1.1 * z_sign
    return f"""      Transform {{
        translation {x_pos:.5f} 0.0 {fin_z:.5f}
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.85 0.75 0.50 roughness 0.55 metalness 0 transparency 0.25
            }}
            geometry Box {{
              size {FIN_SPAN * 0.70:.5f} {BODY_RADIUS * 0.06:.5f} {FIN_SPAN:.5f}
            }}
          }}
        ]
      }}"""


def build_vertical_fin(name: str, y_pos: float, axis: str, x_from: float, x_to: float,
                        motor_name_l: str, motor_name_r: str) -> str:
    """Dorsal, caudal, or pelvic fin: visual shape only (no physics joints).
    Motors are registered as channel-only flat joints in build_channel_only_joints."""
    length = abs(x_to - x_from)
    cx = (x_from + x_to) / 2.0
    return f"""      Transform {{
        translation {cx:.5f} {y_pos + BODY_RADIUS * 0.6:.5f} 0.0
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.80 0.72 0.52 roughness 0.50 metalness 0 transparency 0.20
            }}
            geometry Box {{
              size {length:.5f} {BODY_RADIUS * 1.40:.5f} {BODY_RADIUS * 0.08:.5f}
            }}
          }}
        ]
      }}"""


def build_jaw_motors() -> str:
    """Jaw / operculum motors + trunk stiffness control joint.

    zebrafish_o_31_trunk_stiffness is placed here as a minimal HingeJoint at
    the body/tail junction — it cannot go in a Solid.children field (Webots
    only allows RotationalMotor inside a joint device field).
    """
    lines = []
    for name, x_f in [("zebrafish_o_28_jaw", HEAD_LENGTH * 0.45),
                       ("zebrafish_o_29_operculum_l", HEAD_LENGTH * 0.30),
                       ("zebrafish_o_30_operculum_r", HEAD_LENGTH * 0.30),
                       ("zebrafish_o_31_trunk_stiffness", -HEAD_LENGTH * 0.10)]:
        lines.append(f"""      HingeJoint {{
        jointParameters HingeJointParameters {{
          anchor {x_f:.5f} 0.0 0.0
          axis 0 1 0
        }}
        device [
          RotationalMotor {{
            name "{name}"
            minPosition -0.30
            maxPosition  0.30
            maxVelocity  6.0
            maxTorque    0.00005
          }}
        ]
        endPoint Solid {{
          name "{name}_solid"
          translation {x_f:.5f} 0.0 0.0
          children []
          boundingObject Sphere {{ radius 0.0001 }}
          physics Physics {{ density -1 mass 0.00001 }}
        }}
      }}""")
    return "\n".join(lines)


def build_tail_chain() -> str:
    """Physical tail spine: PHYS_SEGMENTS chained HingeJoints driving real undulation.

    Following the celegans pattern:
    - A short physical spine (4 segments) provides actual swimming mechanics.
    - Each spine joint uses ONE RotationalMotor named after the first 4 L-channels
      (o_00_tail_l0 … o_06_tail_l3) so the AARNN directly drives physical motion.
    - The remaining 28 motor channels are registered as channel-only flat joints
      (see build_channel_only_joints) — ODE ignores them but DeviceMapper finds them.

    Chain depth: PHYS_SEGMENTS (4) vs. the previous 8 Hinge2Joints (16 DOF).
    This eliminates the "physics too complex" step failures.
    """
    def spine_solid(seg_idx: int, child_block: str) -> str:
        seg_frac = seg_idx / PHYS_SEGMENTS
        r = BODY_RADIUS * (1.0 - 0.55 * seg_frac)   # taper
        mass = max(0.0008, 0.0012 * (r / BODY_RADIUS))  # 0.8–1.2 g per segment
        r1, g1, b1 = _body_colour(seg_idx, PHYS_SEGMENTS)
        return f"""Solid {{
          name "zebrafish_spine_seg_{seg_idx + 1:02d}"
          translation {PHYS_SEG_LEN:.5f} 0.0 0.0
          children [
            Transform {{
              rotation 0 0 1 1.5708
              children [
                Shape {{
                  appearance PBRAppearance {{
                    baseColor {r1:.2f} {g1:.2f} {b1:.2f}
                    roughness 0.65 metalness 0
                  }}
                  geometry Capsule {{
                    radius {r:.5f} height {PHYS_SEG_LEN * 0.90:.5f}
                  }}
                }}
              ]
            }}
{child_block}
          ]
          boundingObject Transform {{
            rotation 0 0 1 1.5708
            children [ Capsule {{ radius {r:.5f} height {PHYS_SEG_LEN * 0.92:.5f} }} ]
          }}
          physics Physics {{ density -1 mass {mass:.6f} }}
        }}"""

    # Tail tip: minimal solid closing the chain.
    chain = f"""Solid {{
          name "zebrafish_spine_tip"
          translation {PHYS_SEG_LEN:.5f} 0.0 0.0
          children []
          boundingObject Sphere {{ radius {BODY_RADIUS * 0.25:.5f} }}
          physics Physics {{ density -1 mass 0.0002 }}
        }}"""

    # Build the chain from tail outward → body (innermost segment last).
    for seg_idx in range(PHYS_SEGMENTS - 1, -1, -1):
        motor_name = PHYS_L_CHANNELS[seg_idx]  # o_00, o_02, o_04, o_06
        child_joint = f"""            HingeJoint {{
              jointParameters HingeJointParameters {{
                anchor {PHYS_SEG_LEN * 0.5:.5f} 0.0 0.0
                axis 0 1 0
              }}
              device [
                RotationalMotor {{
                  name "{motor_name}"
                  minPosition -1.20
                  maxPosition  1.20
                  maxVelocity  8.0
                  maxTorque    0.00020
                }}
              ]
              endPoint
              {chain}
            }}"""
        chain = spine_solid(seg_idx, child_joint)

    return f"""      HingeJoint {{
        jointParameters HingeJointParameters {{
          anchor 0.0 0.0 0.0
          axis 0 1 0
        }}
        device [
          RotationalMotor {{
            name "zebrafish_tail_root_lock"
            minPosition 0 maxPosition 0
            maxVelocity 2.0 maxTorque 1.0
          }}
        ]
        endPoint
        {chain}
      }}"""


def build_channel_only_joints() -> str:
    """28 flat (non-chained) channel-only joints for all non-physical motor channels.

    Channels o_00/02/04/06 (L0-L3) are attached to the physical spine above.
    All remaining 28 channels are registered here as ODE-invisible joints:
      - R0-R3 (o_01/03/05/07): R-side of physical segments
      - L4-R7 (o_08-o_15): distal tail pairs
      - Fin motors (o_16-o_27): pectoral, dorsal, caudal, pelvic
    Jaw motors o_28-31 are handled by build_jaw_motors().
    """
    SKIP = set(PHYS_L_CHANNELS)                     # already physical
    SKIP.update(OUTPUT_CHANNELS[28:])               # jaw/operculum via build_jaw_motors
    channel_only = [ch for ch in OUTPUT_CHANNELS if ch not in SKIP]

    lines = []
    n = len(channel_only)
    for idx, ch in enumerate(channel_only):
        frac = idx / max(1, n - 1)
        # Distribute along the body to match approximate muscle/fin locations.
        if "tail" in ch:
            x = -TAIL_LENGTH * 0.20 - frac * TAIL_LENGTH * 0.65
            y_off = BODY_RADIUS * 0.60 * (1 if "_l" in ch else -1)
        elif "pec_fin" in ch:
            x = -HEAD_LENGTH * 0.05
            y_off = BODY_RADIUS * 1.2 * (1 if "_l_" in ch else -1)
        elif "dorsal" in ch:
            x = -HEAD_LENGTH * 0.3 - frac * TAIL_LENGTH * 0.2
            y_off = BODY_RADIUS * 0.9
        elif "caudal" in ch:
            x = -TAIL_LENGTH * 0.90
            y_off = BODY_RADIUS * 0.5 * (1 if "_l" in ch else -1)
        elif "pelvic" in ch:
            x = -HEAD_LENGTH * 0.50
            y_off = BODY_RADIUS * 0.85 * (1 if "_l" in ch else -1)
        else:
            x = HEAD_LENGTH * 0.20 - frac * BODY_LENGTH * 0.15
            y_off = 0.0
        lines.append(_channel_only_joint(ch, x, y_off, 0.0))

    return "\n".join(lines)


def build_head_solid() -> str:
    """Main head capsule with embedded sensors (no physics — fused to Robot root)."""
    r = BODY_RADIUS
    return f"""      Transform {{
        rotation 0 0 1 1.5708
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.92 0.85 0.60
              roughness 0.55
              metalness 0
            }}
            geometry Capsule {{
              radius {r:.5f}
              height {HEAD_LENGTH:.5f}
            }}
          }}
        ]
      }}
      Transform {{
        translation {HEAD_LENGTH * 0.4:.5f} 0.0 0.0
        children [
          Shape {{
            appearance PBRAppearance {{
              baseColor 0.30 0.30 0.30
              roughness 0.30
              metalness 0.10
            }}
            geometry Sphere {{ radius {r * 0.55:.5f} }}
          }}
        ]
      }}"""


# ---------------------------------------------------------------------------
# Full PROTO
# ---------------------------------------------------------------------------

def build_proto(proto_name: str = "ZebrafishRobot") -> str:
    lateral_l = build_lateral_line_sensors("l", 8)
    lateral_r = build_lateral_line_sensors("r", 8)
    eye_block  = build_eye_sensors()
    olf_block  = build_olfactory_sensors()
    imu_block  = build_imu_sensors()
    flow_block = build_flow_pressure_sensors()
    tail_block = build_tail_chain()
    pec_l      = build_pectoral_fin_joint("l", -HEAD_LENGTH * 0.05)
    pec_r      = build_pectoral_fin_joint("r", -HEAD_LENGTH * 0.05)
    dorsal     = build_vertical_fin(
        "dorsal", BODY_RADIUS * 0.95, "x",
        x_from= HEAD_LENGTH * 0.10,
        x_to  =-HEAD_LENGTH * 0.80,
        motor_name_l="zebrafish_o_20_dorsal_fin_l",
        motor_name_r="zebrafish_o_21_dorsal_fin_r")
    caudal     = build_vertical_fin(
        "caudal", 0.0, "x",
        x_from=-TAIL_LENGTH * 0.85,
        x_to  =-TAIL_LENGTH * 1.02,
        motor_name_l="zebrafish_o_22_caudal_fin_l",
        motor_name_r="zebrafish_o_23_caudal_fin_r")
    pelvic_l   = build_vertical_fin(
        "pelvic_l", -BODY_RADIUS * 0.90, "x",
        x_from=-HEAD_LENGTH * 0.30,
        x_to  =-HEAD_LENGTH * 0.70,
        motor_name_l="zebrafish_o_24_pelvic_fin_l",
        motor_name_r="zebrafish_o_26_pelvic_fin_l2")
    pelvic_r   = build_vertical_fin(
        "pelvic_r", -BODY_RADIUS * 0.90, "x",
        x_from=-HEAD_LENGTH * 0.30,
        x_to  =-HEAD_LENGTH * 0.70,
        motor_name_l="zebrafish_o_25_pelvic_fin_r",
        motor_name_r="zebrafish_o_27_pelvic_fin_r2")
    jaw_block      = build_jaw_motors()
    chan_only_block = build_channel_only_joints()
    head_solid     = build_head_solid()

    return f"""#VRML_SIM R2025a utf8
# Zebrafish robot PROTO for AARNN neuromorphic simulation.
# Generated by scripts/build_webots_zebrafish_assets.py
# DO NOT EDIT — regenerate with build_webots_zebrafish_assets.py

PROTO {proto_name} [
  field SFVec3f    translation     0 0 0
  field SFRotation rotation        0 1 0 0
  field SFString   name            "{proto_name.lower()}"
  field SFString   controller      "<extern>"
  field MFString   controllerArgs  []
  field SFString   customData      ""
  field SFBool     supervisor      FALSE
  field SFBool     synchronization TRUE
]
{{
  Robot {{
    translation     IS translation
    rotation        IS rotation
    name            IS name
    controller      IS controller
    controllerArgs  IS controllerArgs
    customData      IS customData
    supervisor      IS supervisor
    synchronization IS synchronization

    children [
      # ---- Head body ----
{head_solid}

      # ---- Lateral line sensors (left flank) ----
{lateral_l}

      # ---- Lateral line sensors (right flank) ----
{lateral_r}

      # ---- Eyes (light sensors) ----
{eye_block}

      # ---- Olfactory sensors ----
{olf_block}

      # ---- IMU ----
{imu_block}

      # ---- Flow / pressure sensors ----
{flow_block}

      # ---- Articulated tail (8 segments, CPG outputs) ----
{tail_block}

      # ---- Pectoral fins ----
{pec_l}
{pec_r}

      # ---- Dorsal fin ----
{dorsal}

      # ---- Caudal fin ----
{caudal}

      # ---- Pelvic fins ----
{pelvic_l}
{pelvic_r}

      # ---- Jaw / operculum (physical HingeJoints) ----
{jaw_block}

      # ---- Channel-only motor joints (28 flat non-physics joints) ----
      # These allow DeviceMapper to route AARNN outputs to all 32 motor
      # channels while ODE only solves the 4-joint physical spine chain.
{chan_only_block}

    ]

    # ---- Collision body ----
    # Three distributed spheres replace the single elongated capsule: each
    # sphere has 1:1:1 aspect ratio, eliminating the eccentricity warning that
    # Webots raises when one body dimension >> others.
    boundingObject Group {{
      children [
        Transform {{
          translation {HEAD_LENGTH * 0.25:.5f} 0 0
          children [ Sphere {{ radius {BODY_RADIUS * 1.20:.5f} }} ]
        }}
        Transform {{
          translation {-TAIL_LENGTH * 0.30:.5f} 0 0
          children [ Sphere {{ radius {BODY_RADIUS * 1.00:.5f} }} ]
        }}
        Transform {{
          translation {-TAIL_LENGTH * 0.70:.5f} 0 0
          children [ Sphere {{ radius {BODY_RADIUS * 0.70:.5f} }} ]
        }}
      ]
    }}

    physics Physics {{
      density -1
      mass    0.008
      # Damping sub-node: correct mechanism in Webots R2025a for linear/angular
      # drag.  High values simulate water resistance so the fish slows quickly
      # when the tail CPG is idle and responds promptly to active tail beats.
      damping Damping {{
        linear  0.85
        angular 0.70
      }}
    }}
  }}
}}
"""


# ---------------------------------------------------------------------------
# Webots aquarium world
# ---------------------------------------------------------------------------

def build_world(proto_name: str = "ZebrafishRobot") -> str:
    return f"""#VRML_SIM R2025a utf8
# Zebrafish aquarium world for AARNN neuromorphic simulation.
# Water physics are approximated: gravity is damped and water-plane is visual only.
# Generated by scripts/build_webots_zebrafish_assets.py

WorldInfo {{
  info "Zebrafish AARNN neuromorphic simulation — aquarium world"
  title "ZebrafishNeuroworld"
  basicTimeStep 32
  gravity 2.2
  contactProperties [
    ContactProperties {{
      material1 "water_body"
      coulombFriction 0.05 0.05 0.05
      bounce 0.1
      bounceVelocity 0.02
    }}
  ]
}}

Viewpoint {{
  orientation -0.2 0.97 0.14 1.2
  position 0.0 0.12 0.30
}}

Background {{
  skyColor 0.04 0.08 0.18
}}

DirectionalLight {{
  ambientIntensity 0.45
  direction  0 -1 0.3
  intensity  0.9
  color 0.75 0.88 1.0
}}

DirectionalLight {{
  ambientIntensity 0.12
  direction  0 1 -0.4
  intensity  0.3
  color 0.60 0.80 1.0
}}

# Aquarium floor (gravel)
DEF AQUARIUM_FLOOR Solid {{
  translation 0 -0.12 0
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.46 0.40 0.30
        roughness 0.92
        metalness 0
      }}
      geometry Box {{ size 0.80 0.01 0.60 }}
    }}
  ]
  boundingObject Box {{ size 0.80 0.01 0.60 }}
  physics Physics {{ density 2600 }}
}}

# Aquarium glass walls (transparent)
DEF WALL_FRONT Solid {{
  translation 0 0.0 0.305
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.7 0.9 1.0
        roughness 0.05
        metalness 0
        transparency 0.82
      }}
      geometry Box {{ size 0.80 0.26 0.01 }}
    }}
  ]
  boundingObject Box {{ size 0.80 0.26 0.01 }}
  physics Physics {{ density 2500 }}
}}
DEF WALL_BACK Solid {{
  translation 0 0.0 -0.305
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.7 0.9 1.0
        roughness 0.05
        metalness 0
        transparency 0.82
      }}
      geometry Box {{ size 0.80 0.26 0.01 }}
    }}
  ]
  boundingObject Box {{ size 0.80 0.26 0.01 }}
  physics Physics {{ density 2500 }}
}}

# Water surface (visual plane only)
DEF WATER_SURFACE Solid {{
  translation 0 0.08 0
  children [
    Shape {{
      appearance PBRAppearance {{
        baseColor 0.35 0.62 0.90
        roughness 0.05
        metalness 0.1
        transparency 0.70
      }}
      geometry Box {{ size 0.80 0.001 0.60 }}
    }}
  ]
}}

# Zebrafish robot instance (filled in by build_webots_multi_world.py)
DEF ZEBRAFISH_0 {proto_name} {{
  translation 0 0 0
  rotation 0 1 0 0
  name "zebrafish_0"
  controller "<extern>"
}}
"""


# ---------------------------------------------------------------------------
# Config JSON
# ---------------------------------------------------------------------------

def build_config(net_path: Optional[str]) -> dict:
    cfg: dict[str, Any] = {
        "num_sensory_neurons":          NUM_SENSORY,
        "num_output_neurons":           NUM_OUTPUT,
        "num_hidden_layers":            1,
        "num_hidden_per_layer_initial": 2000,
        "growth_enabled":               True,
        "use_morphology":               True,
        "aarnn_layer_depth":            4,
        "clumping_design":              "ZebraFish",
        "spike_io": {
            "profile": "zebrafish",
        },
    }
    if net_path:
        try:
            import json as _json
            data = _json.loads(Path(net_path).read_text(encoding="utf-8"))
            net  = data.get("net", {})
            cfg["num_hidden_per_layer_initial"] = int(
                net.get("num_hidden_per_layer_initial", 2000))
        except Exception:
            pass
    return cfg


def build_io_alignment() -> dict:
    return {
        "sensory_channels": SENSORY_CHANNELS,
        "output_channels":  OUTPUT_CHANNELS,
    }


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

from typing import Optional


def main() -> None:
    here = Path(__file__).resolve().parent
    root = here.parent

    parser = argparse.ArgumentParser(
        description="Generate Webots assets for the zebrafish robot.")
    parser.add_argument("--proto-output",
        default=str(root / "webots_world" / "protos" / "ZebrafishRobot.proto"),
        help="Output PROTO file path")
    parser.add_argument("--world-output",
        default=str(root / "webots_world" / "worlds" / "zebrafish_neuroworld.wbt"),
        help="Output Webots world file path")
    parser.add_argument("--config-output",
        default=str(root / "webots_world" / "configs" / "config_zebrafish_webots.json"),
        help="Output config JSON path")
    parser.add_argument("--io-alignment-output",
        default=str(root / "webots_world" / "configs" / "config_zebrafish_webots.io_alignment.json"),
        help="Output I/O alignment JSON path")
    parser.add_argument("--network",
        default=None,
        help="Path to network_zebrafish.json (used to read hidden layer size for config)")
    args = parser.parse_args()

    proto_path     = Path(args.proto_output)
    world_path     = Path(args.world_output)
    config_path    = Path(args.config_output)
    io_align_path  = Path(args.io_alignment_output)

    for p in [proto_path, world_path, config_path, io_align_path]:
        p.parent.mkdir(parents=True, exist_ok=True)

    proto_path.write_text(build_proto(), encoding="utf-8")
    world_path.write_text(build_world(), encoding="utf-8")
    config_path.write_text(
        json.dumps(build_config(args.network), indent=2), encoding="utf-8")
    io_align_path.write_text(
        json.dumps(build_io_alignment(), indent=2), encoding="utf-8")

    print(f"Wrote {proto_path}")
    print(f"Wrote {world_path}")
    print(f"Wrote {config_path}")
    print(f"Wrote {io_align_path}")
    print(f"Sensory channels: {NUM_SENSORY}  |  Output channels: {NUM_OUTPUT}")


if __name__ == "__main__":
    main()
