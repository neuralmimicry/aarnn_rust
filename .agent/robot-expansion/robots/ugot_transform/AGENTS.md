# Implement UGOT Transforming car

Task profile ID: `ugot_transform`. Category: reconfigurable wheeled robot. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Model transform joints and verify clearances; changing mode does not teleport links.
- Use modular part identities, port maps, joint sensors, camera, TOF/IMU and mode-specific motor limits.
- Do not infer CAD openness from an open Python SDK or assembly blueprints.



## Grounded parameters and gaps

- Exact physical dimensions, masses and joint parameters remain unresolved; authored assumptions must be explicit.

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `drive`
- `transform`
- `collision_free_mode_change`
- `drive_after_transform`
- `missing_sensor_fault`
- `two_instance_brain_isolation`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [ugot-docs](https://docs.ubtrobot.com/ugot/)
- [lec-ugot](https://www.lecrobotics.ai/products/ugot)
