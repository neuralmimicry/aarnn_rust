# Implement Galbot G1 (official Golf description)

Task profile ID: `galbot_g1_golf`. Category: wheeled dual arm mobile manipulator. Source status: `official_description_available_variant_match_pending`.

These are explicitly loaded task instructions. Follow repository root instructions and the pack’s shared requirements, AER/physics contracts and acceptance gates. This file does not automatically govern files outside its own directory.

## Required model and behaviour

- Import the official wheeled-base model, torso/lift, head, arms and selected grippers with their original joint names.
- Preserve mimic/passive joints, transforms, sensor extrinsics, limits, masses, inertia and mesh scale.
- Match LEC G1 variant explicitly. Neither a Unitree G1 biped nor another Galbot end effector is an interchangeable model.


Use the pinned source at `assets/galbot-g1-golf` (pack-relative), revision `2d496b053f0d4e9e2688f59fac66022f447226be`. Preserve upstream originals and notices.

Variant data is in the manifest: `wheeled`, `fixed_base`, `mjcf_planar_base`. Select a variant explicitly; do not combine incompatible fields.


## Grounded parameters and gaps

- `mass`: 92.5 kg; published; source `galbot-manual`. Published body weight; exact mass of the selected description must be summed and compared, not blindly forced to this number.

All unlisted wheel geometry, joint axes/limits, link inertias, motor curves, sensor extrinsics and noise/latency remain to be derived from the selected licensed design or recorded as engineering assumptions. Published envelope dimensions alone do not determine mechanics.

## Implementation outputs

Produce canonical mechanics/visual/collision source, explicit parameter provenance, generated Webots/Unity/Unreal/MJCF/USD assets, a named ordered sensor/actuator map, network/config recipes, per-instance AER binding, headless/interactive launch presets and task fixtures. Add browser and both Minecraft functional views through the authoritative physics-service path. Do not reuse an unrelated robot’s network dimensions or create a silent profile alias.

All eight targets are required: Webots, Unity, Unreal, WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim. Use the actual scene/body sensors and AARNN outputs. A labelled diagnostic controller is permitted for mechanics tests but must be absent from neural evaluation.

## Profile acceptance scenarios

- `native_import`
- `base_forward_strafe_yaw`
- `arm_joint_sweep`
- `reachable_grasp_and_place`
- `self_collision`
- `two_instance_brain_isolation`

Also pass the shared mapping/AER/timing/reset/disconnect/isolation gates. Retain source hashes, model/IO digests and target results. An unrun target stays not_run; an unavailable exact model stays pending while the documented functional surrogate proceeds.

## Evidence sources

- [galbot-description](https://github.com/GalaxyGeneralRobotics/galbot_one_golf_description/tree/2d496b053f0d4e9e2688f59fac66022f447226be)
- [lec-rentals](https://www.lecrobotics.ai/rentals)
- [galbot-platform](https://developer.galbot.com/platform/)
- [galbot-manual](https://developer.galbot.com/docs/g1/2.2.4/en/g1/)
