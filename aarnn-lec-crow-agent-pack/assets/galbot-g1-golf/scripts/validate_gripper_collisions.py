#!/usr/bin/env python3
"""Check composed USD finger colliders against the authored STL files (usd-core)."""

import struct
from pathlib import Path

from pxr import Gf, Usd, UsdGeom, UsdPhysics, UsdShade


ROOT = Path(__file__).resolve().parent.parent


def stl_points(path):
    data = path.read_bytes()
    count = struct.unpack_from("<I", data, 80)[0]
    return [
        struct.unpack_from("<3f", data, 84 + 50 * i + 12 + 12 * j)
        for i in range(count)
        for j in range(3)
    ]


def rounded_points(points):
    return {tuple(round(c, 7) for c in point) for point in points}


def validate(stage, root):
    for arm in ("left", "right"):
        for side in ("l", "r"):
            link = stage.GetPrimAtPath(f"{root}/{arm}_gripper_{side}_finger_link")
            colliders = [
                prim for prim in Usd.PrimRange(link, Usd.TraverseInstanceProxies())
                if prim.HasAPI(UsdPhysics.CollisionAPI)
                and UsdPhysics.CollisionAPI(prim).GetCollisionEnabledAttr().Get()
            ]
            assert len(colliders) == 3, f"{link.GetPath()}: expected three STL colliders"
            suffix = "_right" if side == "r" else ""
            for i, prim in enumerate(colliders, 1):
                assert "/collisions/" in str(prim.GetPath()), prim.GetPath()
                assert prim.IsA(UsdGeom.Mesh), prim.GetPath()
                assert UsdPhysics.MeshCollisionAPI(prim).GetApproximationAttr().Get() == "convexHull"
                transform = UsdGeom.XformCache().ComputeRelativeTransform(prim, link)[0]
                assert transform.GetDeterminant() > 0, prim.GetPath()
                actual = rounded_points(
                    transform.Transform(Gf.Vec3d(*point))
                    for point in UsdGeom.Mesh(prim).GetPointsAttr().Get()
                )
                source = (
                    ROOT / "meshes/collision/end_effectors/galbot_gripper"
                    / f"link3{suffix}" / f"link_3{suffix}_collision_{i:02}.stl"
                )
                assert actual == rounded_points(stl_points(source)), (
                    f"{prim.GetPath()}: differs from {source}"
                )
                material, _ = UsdShade.MaterialBindingAPI(prim).ComputeBoundMaterial("physics")
                assert material and material.GetPrim().HasAPI(UsdPhysics.MaterialAPI), prim.GetPath()
                physics = UsdPhysics.MaterialAPI(material)
                assert physics.GetStaticFrictionAttr().Get() == 1.5, material.GetPath()
                assert physics.GetDynamicFrictionAttr().Get() == 1.5, material.GetPath()
                assert physics.GetRestitutionAttr().Get() == 0, material.GetPath()
    print(f"PASS: 12 authored gripper colliders and physics materials at {root}")


if __name__ == "__main__":
    asset = ROOT / "usd/galbot_one_golf.usda"
    validate(Usd.Stage.Open(str(asset)), "/galbot_one_golf")
    # Material targets must also resolve when a consumer references the robot elsewhere.
    stage = Usd.Stage.CreateInMemory()
    stage.DefinePrim("/World/Robot").GetReferences().AddReference(str(asset))
    validate(stage, "/World/Robot")
