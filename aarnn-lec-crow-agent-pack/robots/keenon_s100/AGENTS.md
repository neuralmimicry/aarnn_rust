# Implement COURIER S100

Task profile ID: `keenon_s100`. Category: heavy load delivery. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Heavy-load platform/rack with explicitly placed loads; stability, emergency stop and dock.
- Keep chassis mass separate from payload and loading-platform dimensions.


Manufacturer depth 640 mm conflicts with LEC 620 mm. Default engineering surrogate may use explicitly labelled manufacturer geometry; exact LEC replication remains unresolved.

Variant data is in the manifest: `manufacturer_640mm_depth`, `lec_620mm_depth`. Select a variant explicitly; do not combine incompatible fields.


## Grounded parameters and gaps

- `body_dimensions_wdh`: [0.925, 0.64, 1.282] m; published; source `keenon-s100`. 
- `mass`: 87.5 kg; published; source `keenon-s100`. 
- `total_payload`: 100 kg; published; source `keenon-s100`. 

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
- `loaded_turn_stability`
- `payload_mass_changes_acceleration`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [keenon-s100](https://www.keenon.com/en/product/S100)
- [lec-courier-s100](https://www.lecrobotics.ai/products/courier-s100)
