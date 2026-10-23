# Implement Carrion crow (Corvus corone)

Task profile ID: `crow_corvus_corone`. Category: biological bird. Source status: `visual_assets_identified_biomechanics_requires_original_surrogate`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Articulated wings, body, neck, beak, tail, legs, anisodactyl feet and contact-based grasping/perching.
- Walk, hop, take off, powered flight, glide, bank, land, perch, feed and release objects.
- Sensory projections and a trained AARNN sensorimotor controller; no claim of a measured crow connectome or complete crow cognition.


Read `spec/crow-biology.md` and `spec/crow-io.proposed.json` fully. The proposed 492/64 projection is not a measured biological brain or an implemented adapter.


## Grounded parameters and gaps

- `length`: 0.47 m; selected_reference; source `crow-rspb`. Published species range 0.45–0.47 m; 0.47 m is the selected nominal.
- `wingspan`: 0.98 m; selected_reference; source `crow-rspb`. Published range 0.93–1.04 m; 0.98 m is a modelling choice.
- `mass`: 0.51 kg; selected_reference; source `crow-bto`. BTO summary 0.5087 kg; 0.510 kg is a rounded model baseline, not a specimen measurement.
- `species_mass_range`: [0.37, 0.65] kg; published; source `crow-rspb`. 

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `joint_sweeps`
- `unpowered_fall`
- `glide_sink_rate`
- `powered_takeoff`
- `bank_and_turn`
- `land_and_perch`
- `walking`
- `beak_pickup`
- `gust_recovery`
- `energy_fatigue`
- `neural_silence_controls`
- `multi_brain_isolation`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [crow-rspb](https://www.rspb.org.uk/birds-and-wildlife/identifying-birds/corvids-how-to-identify-the-crow-family)
- [crow-bto](https://www.bto.org/learn/about-birds/birdfacts/carrion-crow)
- [crow-visual-cc0](https://blendswap.com/blend/7340)
- [ptera](https://github.com/camUrban/PteraSoftware/tree/0fec89188372e688901473cadd7ae68a49f60d95)
