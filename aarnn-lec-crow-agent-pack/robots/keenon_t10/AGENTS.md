# Implement DINERBOT T10

Task profile ID: `keenon_t10`. Category: tray delivery advertising. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Four tray levels, advertising screen and separately modelled head articulation if evidence permits.
- Delivery and screen interaction are separate from locomotion control.



## Grounded parameters and gaps

- `body_dimensions_wdh`: [0.486, 0.555, 1.399] m; published; source `keenon-t10`. 
- `mass`: 58 kg; published; source `keenon-t10`. 
- `total_payload`: 40 kg; published; source `keenon-t10`. 

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

- [keenon-t10](https://www.keenon.com/en/product/T10)
- [lec-dinerbot-t10](https://www.lecrobotics.ai/products/dinerbot-t10)
