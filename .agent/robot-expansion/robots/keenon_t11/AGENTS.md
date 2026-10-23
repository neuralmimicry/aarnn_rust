# Implement DINERBOT T11

Task profile ID: `keenon_t11`. Category: narrow aisle delivery advertising. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Narrow-aisle tray robot with advertising/display behaviour.
- Preserve the T11 body and tray layout; do not alias it to T8 or T10.



## Grounded parameters and gaps

- `body_dimensions_wdh`: [0.463, 0.384, 1.123] m; published; source `keenon-t11`. 
- `mass`: 38 kg; published; source `keenon-t11`. 
- `total_payload`: 20 kg; published; source `keenon-t11`. Manufacturer says 20 kg total while parenthetical tray figures mention 5 kg middle + 10 kg bottom. Resolve remaining payload allocation before claiming tray-specific ratings.

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `blocked_path`
- `obstacle_and_person_avoidance`
- `low_energy_return_to_dock`
- `connection_loss_neutral_output`
- `two_instance_brain_isolation`
- `loaded_delivery_and_pickup`
- `payload_does_not_teleport_or_fall_through_tray`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [keenon-t11](https://www.keenon.com/en/product/T11)
- [lec-rentals](https://www.lecrobotics.ai/rentals)
