# Implement OMNI

Task profile ID: `botinkit_omni`. Category: automated induction wok. Source status: `no_public_design_verified`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Fixed kitchen appliance with articulated wok tilt/rotation, lid or stirring mechanism only where documented.
- Recipe-driven heating, ingredient and 13-seasoning dispensing, batch transfer, cleaning and fault states.
- Use a labelled thermal/mixing state model; food chemistry and certified hygiene are outside this surrogate.


Variant data is in the manifest: `omni_01`, `omni_02`. Select a variant explicitly; do not combine incompatible fields.


## Grounded parameters and gaps

- `body_dimensions_wdh`: [0.827, 0.92, 1.769] m; published; source `lec-botinkit-omni-cooking-robot`. 
- `mass`: 180 kg; published; source `lec-botinkit-omni-cooking-robot`. 

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `recipe_batch_cycle`
- `mass_and_energy_accounting`
- `heater_off_cools`
- `empty_reservoir_fault`
- `clean_between_batches`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [lec-botinkit-omni-cooking-robot](https://www.lecrobotics.ai/products/botinkit-omni-cooking-robot)
