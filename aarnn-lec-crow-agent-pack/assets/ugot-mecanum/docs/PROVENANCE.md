# Parameter provenance and confidence

Accessed 19 October 2026.

## Published values used

| Parameter | Value used | Source |
|---|---:|---|
| Gear motor envelope | Ø40 × 50 mm | UBTECH full-kit specification mirrored by RobotShop |
| Gear motor maximum speed | ≥360 RPM | UBTECH full-kit specification mirrored by RobotShop |
| Gear motor maximum torque | ≈2.0 kgf·cm (0.196133 N·m) | UBTECH full-kit specification mirrored by RobotShop |
| Battery envelope | 54 × 62 × 82 mm | UGOT published specification reproduced by SmartRobot |
| Battery | 11.1 V, 2600 mAh | UBTECH product page |
| Vehicle advertised maximum speed | 1.6 m/s | UBTECH product page |
| Number of wheel motors | 4 | UBTECH product page |

Sources:

- https://www.ubtrobot.com/cn/ai-education/products/ugot
- https://uk.robotshop.com/products/ugot-ai-robotic-kit
- https://smartrobot.ro/en/product/kit-robot-inteligenta-artificiala-ugot/

## Explicit estimates

The chassis dimensions, wheel dimensions, roller geometry, assembled mass,
module masses, centres of mass, inertias, joint damping, and friction values are
engineering estimates. MuJoCo derives primitive inertias from the assigned
geometry and mass. These values exist to make a coherent, executable baseline;
they are not manufacturer claims.

## Acceptance state

| Gate | State |
|---|---|
| Parameter schema complete | PASS |
| Four wheel drives present | PASS |
| Passive roller bodies present | PASS |
| MJCF XML parses | PASS via dependency-free test |
| MuJoCo engine compile/dynamics | PASS with MuJoCo smoke test |
| USD articulation authored | PASS via structural test |
| Isaac Sim load/dynamics | PENDING target installation |
| Dimensional fidelity | PENDING physical measurement/CAD |

