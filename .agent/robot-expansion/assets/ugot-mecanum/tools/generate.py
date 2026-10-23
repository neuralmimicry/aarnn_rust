#!/usr/bin/env python3
"""Generate MuJoCo MJCF and OpenUSD ASCII from the parameter ledger."""
from __future__ import annotations

import json
import math
from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / "config" / "ugot_mecanum.json"
MODEL = ROOT / "model"


def load_values():
    raw = json.loads(CONFIG.read_text(encoding="utf-8"))
    def unpack(section):
        return {key: item["value"] for key, item in raw[section].items()}
    return raw, unpack("dimensions"), unpack("mass"), unpack("drive"), unpack("environment")


def fmt(values):
    if not isinstance(values, (tuple, list)):
        values = [values]
    return " ".join(f"{v:.9g}" for v in values)


def add(parent, tag, **attrs):
    return ET.SubElement(parent, tag, {k.rstrip("_"): str(v) for k, v in attrs.items()})


def indent(node, level=0):
    pad = "\n" + "  " * level
    if len(node):
        if not node.text or not node.text.strip(): node.text = pad + "  "
        for child in node:
            indent(child, level + 1)
        if not child.tail or not child.tail.strip(): child.tail = pad
    if level and (not node.tail or not node.tail.strip()): node.tail = pad


def build_mjcf(dim, mass, drive, environment):
    root = ET.Element("mujoco", model="UGOT mecanum car")
    add(root, "compiler", angle="radian", coordinate="local", inertiafromgeom="true", balanceinertia="true")
    add(root, "option", timestep="0.002", integrator="implicitfast", solver="Newton", iterations="80", gravity="0 0 -9.81")
    size = add(root, "size", njmax="4000", nconmax="1000")
    default = add(root, "default")
    add(default, "joint", armature="0.00002", damping="0.0002")
    add(default, "geom", condim="4", solref="0.004 1", solimp="0.9 0.95 0.001", friction=f'{drive["sliding_friction"]} 0.01 {drive["rolling_friction"]}')
    visual = add(root, "visual")
    add(visual, "headlight", ambient="0.35 0.35 0.35", diffuse="0.8 0.8 0.8", specular="0.2 0.2 0.2")
    add(visual, "rgba", haze="0.15 0.2 0.25 1")
    asset = add(root, "asset")
    add(asset, "texture", name="ground_tex", type="2d", builtin="checker", rgb1="0.18 0.20 0.22", rgb2="0.12 0.14 0.16", width="512", height="512")
    add(asset, "material", name="ground", texture="ground_tex", texrepeat="8 8", reflectance="0.05")
    add(asset, "material", name="ugot_blue", rgba="0.055 0.42 0.72 1", specular="0.35", shininess="0.4")
    add(asset, "material", name="dark_plastic", rgba="0.025 0.035 0.045 1")
    add(asset, "material", name="roller", rgba="0.12 0.13 0.14 1")
    add(asset, "material", name="screen", rgba="0.03 0.13 0.20 1", emission="0.15")
    world = add(root, "worldbody")
    add(world, "light", pos="0 0 2", dir="0 0 -1", directional="true")
    platform_half = environment["platform_half_extent_m"]
    platform_thickness = environment["platform_thickness_m"]
    add(world, "geom", name="platform", type="box",
        pos=fmt((0, 0, -platform_thickness / 2)),
        size=fmt((platform_half, platform_half, platform_thickness / 2)),
        material="ground", contype="1", conaffinity="1")
    z0 = dim["wheel_radius_m"] + 0.006
    car = add(world, "body", name="base_link", pos=fmt((0, 0, z0)))
    add(car, "freejoint", name="base_free")
    add(car, "geom", name="chassis_collision", type="box", size=fmt((dim["chassis_length_m"]/2, dim["chassis_width_m"]/2, dim["chassis_height_m"]/2)), mass=mass["chassis_kg"], material="ugot_blue")
    controller_z = dim["chassis_height_m"]/2 + dim["controller_size_m"][2]/2
    add(car, "geom", name="controller", type="box", pos=fmt((0, 0, controller_z)), size=fmt(tuple(v/2 for v in dim["controller_size_m"])), mass=mass["controller_kg"], material="dark_plastic", contype="0", conaffinity="0")
    add(car, "geom", name="screen", type="box", pos=fmt((dim["controller_size_m"][0]/2 + .001, 0, controller_z)), size="0.002 0.031 0.021", mass="0.001", material="screen", contype="0", conaffinity="0")
    add(car, "geom", name="battery", type="box", pos=fmt((-0.02, 0, -dim["chassis_height_m"]/2 + dim["battery_size_m"][2]/2)), size=fmt(tuple(v/2 for v in dim["battery_size_m"])), mass=mass["battery_kg"], material="dark_plastic", contype="0", conaffinity="0")

    wheels = [("front_left", 1, 1), ("front_right", 1, -1), ("rear_left", -1, 1), ("rear_right", -1, -1)]
    roller_n = int(dim["roller_count"])
    alpha = math.radians(dim["roller_angle_deg"])
    roller_mass = mass["rollers_per_wheel_kg"] / roller_n
    for name, sx, sy in wheels:
        x, y = sx*dim["wheelbase_m"]/2, sy*dim["track_width_m"]/2
        wb = add(car, "body", name=f"{name}_wheel", pos=fmt((x, y, 0)))
        add(wb, "joint", name=f"{name}_joint", type="hinge", axis="0 1 0", damping=drive["joint_damping_nms_per_rad"], limited="false")
        add(wb, "geom", name=f"{name}_hub", type="cylinder", size=fmt((dim["wheel_radius_m"]*.70, dim["wheel_width_m"]*.38)), quat="0.707106781 0.707106781 0 0", mass=mass["wheel_hub_each_kg"] + mass["motor_each_kg"], material="dark_plastic", contype="0", conaffinity="0")
        # Mirrored roller handedness: FL/RR use +45 degrees; FR/RL use -45.
        handed = 1 if sx == sy else -1
        ring_r = dim["wheel_radius_m"] - dim["roller_radius_m"]*.72
        for i in range(roller_n):
            theta = 2*math.pi*i/roller_n
            px, pz = ring_r*math.cos(theta), ring_r*math.sin(theta)
            direction = (-math.sin(theta)*math.cos(alpha), handed*math.sin(alpha), math.cos(theta)*math.cos(alpha))
            half = dim["roller_length_m"]/2
            endpoints = tuple(-half*v for v in direction) + tuple(half*v for v in direction)
            rb = add(wb, "body", name=f"{name}_roller_{i:02d}", pos=fmt((px, 0, pz)))
            add(rb, "joint", name=f"{name}_roller_{i:02d}_joint", type="hinge", axis=fmt(direction), limited="false", damping="0.00004", armature="0.000001")
            add(rb, "geom", name=f"{name}_roller_{i:02d}_geom", type="capsule", fromto=fmt(endpoints), size=fmt(dim["roller_radius_m"]), mass=roller_mass, material="roller", friction=f'{drive["sliding_friction"]} 0.01 {drive["rolling_friction"]}')

    actuator = add(root, "actuator")
    for name, _, _ in wheels:
        add(actuator, "motor", name=f"{name}_motor", joint=f"{name}_joint", gear=drive["max_motor_torque_nm"], ctrllimited="true", ctrlrange="-1 1")
    sensor = add(root, "sensor")
    add(sensor, "framepos", name="base_position", objtype="body", objname="base_link")
    add(sensor, "framequat", name="base_orientation", objtype="body", objname="base_link")
    add(sensor, "gyro", name="imu_gyro", site="imu_site") if False else None
    for name, _, _ in wheels:
        add(sensor, "jointpos", name=f"{name}_position", joint=f"{name}_joint")
        add(sensor, "jointvel", name=f"{name}_velocity", joint=f"{name}_joint")
    key = add(root, "keyframe")
    add(key, "key", name="home", qpos=fmt((0, 0, z0, 1, 0, 0, 0) + (0,)*(4 + 4*roller_n)))
    indent(root)
    return '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(root, encoding="unicode") + "\n"


def build_usda(dim, mass, drive):
    # Isaac Sim consumes this as a lightweight articulation. Detailed passive
    # rollers remain in MJCF; the USD uses collision cylinders plus anisotropic
    # wheel behaviour to keep the asset small.
    z0 = dim["wheel_radius_m"] + .006
    lines = ["#usda 1.0", "(", '    defaultPrim = "UGOTMecanum"', '    metersPerUnit = 1', '    upAxis = "Z"', ")", "", 'def Xform "UGOTMecanum" (', '    prepend apiSchemas = ["PhysicsArticulationRootAPI"]', ")", "{", '    def Xform "base_link" (', '        prepend apiSchemas = ["PhysicsRigidBodyAPI", "PhysicsMassAPI"]', '    )', "    {", f"        double physics:mass = {mass['chassis_kg'] + mass['controller_kg'] + mass['battery_kg']}", f"        double3 xformOp:translate = (0, 0, {z0})", '        uniform token[] xformOpOrder = ["xformOp:translate"]', '        def Cube "chassis" (', '            prepend apiSchemas = ["PhysicsCollisionAPI"]', '        )', '        {', f"            double3 xformOp:scale = ({dim['chassis_length_m']}, {dim['chassis_width_m']}, {dim['chassis_height_m']})", '            uniform token[] xformOpOrder = ["xformOp:scale"]', '            color3f[] primvars:displayColor = [(0.055, 0.42, 0.72)]', '        }', '    }']
    for name, sx, sy in [("front_left",1,1),("front_right",1,-1),("rear_left",-1,1),("rear_right",-1,-1)]:
        x, y = sx*dim["wheelbase_m"]/2, sy*dim["track_width_m"]/2
        lines += [f'    def Xform "{name}_wheel" (', '        prepend apiSchemas = ["PhysicsRigidBodyAPI", "PhysicsMassAPI"]', '    )', '    {', f"        double physics:mass = {mass['motor_each_kg'] + mass['wheel_hub_each_kg'] + mass['rollers_per_wheel_kg']}", f"        double3 xformOp:translate = ({x}, {y}, {z0})", '        uniform token[] xformOpOrder = ["xformOp:translate"]', f'        def Cylinder "collision" (', '            prepend apiSchemas = ["PhysicsCollisionAPI"]', '        )', '        {', '            token axis = "Y"', f"            double radius = {dim['wheel_radius_m']}", f"            double height = {dim['wheel_width_m']}", '            color3f[] primvars:displayColor = [(0.04, 0.05, 0.06)]', '        }', '    }', f'    def PhysicsRevoluteJoint "{name}_joint"', '    {', '        uniform token physics:axis = "Y"', '        rel physics:body0 = </UGOTMecanum/base_link>', f'        rel physics:body1 = </UGOTMecanum/{name}_wheel>', f'        float drive:angular:physics:maxForce = {drive["max_motor_torque_nm"]}', f'        float drive:angular:physics:maxVelocity = {drive["max_motor_speed_rpm"] * 6.0}', '    }']
    lines += ["}", ""]
    return "\n".join(lines)


def main():
    _, dim, mass, drive, environment = load_values()
    MODEL.mkdir(parents=True, exist_ok=True)
    (MODEL / "ugot_mecanum.xml").write_text(build_mjcf(dim, mass, drive, environment), encoding="utf-8")
    (MODEL / "ugot_mecanum.usda").write_text(build_usda(dim, mass, drive), encoding="utf-8")
    print("Generated model/ugot_mecanum.xml and model/ugot_mecanum.usda")


if __name__ == "__main__":
    main()
