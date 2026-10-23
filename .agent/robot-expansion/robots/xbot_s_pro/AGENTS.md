# Implement XBOT S Pro

Task profile ID: `xbot_s_pro`. Category: dual arm robotic barista. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Two articulated arms, coordinated handoff/pouring and actual physical cup/jug contact.
- Espresso, milk, pour and optional foam-printing stations with conservation and cleaning states.
- The exact arm OEM and kinematics are not established; use disclosed generic arms only for the surrogate tier.


Variant data is in the manifest: `lec_webpage`, `lec_brochure`. Select a variant explicitly; do not combine incompatible fields.


## Grounded parameters and gaps

- `body_dimensions_wdh`: null m; conflicting; source `lec-xbot-s-pro`. Webpage 2.200 W x 1.700 D x 2.170 H m; linked S PRO PDF 1.130 W x 1.280 D x 1.600 H m. Do not merge the two.
- `mass`: null kg; conflicting; source `lec-xbot-s-pro`. Webpage approx. 900 kg; linked PDF 400 kg.

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `two_arm_collision_avoidance`
- `cup_and_jug_handoff`
- `pour_into_cup`
- `drink_recipe_cycle`
- `stock_faults`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [lec-xbot-s-pro](https://www.lecrobotics.ai/products/xbot-s-pro)
