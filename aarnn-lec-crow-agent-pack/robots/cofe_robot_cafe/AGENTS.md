# Implement COFE+ Robot Café

Task profile ID: `cofe_robot_cafe`. Category: robotic coffee kiosk. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Enclosure, actual selected arm, cup/gripper contacts, coffee stations, lids and pickup window.
- Order -> acquire cup -> brew/dispense -> lid -> handoff -> clean, with stocks, spills and interlocks.
- Arm OEM, generation, joint count/limits and station transforms remain unknown; never substitute an xArm/UR arm and label it OEM.



## Grounded parameters and gaps

- `body_dimensions_wdh`: [1.8, 1.3, 2.18] m; published; source `lec-cofe-plus-robot-cafe`. 
- `mass`: 800 kg; published; source `lec-cofe-plus-robot-cafe`. 

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `prepare_drink_and_handoff`
- `cup_missing_interlock`
- `ingredient_inventory_conservation`
- `workspace_collision`
- `cleaning_cycle`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [lec-cofe-plus-robot-cafe](https://www.lecrobotics.ai/products/cofe-plus-robot-cafe)
