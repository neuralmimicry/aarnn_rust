# Port existing hexapod to MuJoCo and Isaac Sim

Verified snapshot projection: **34 sensory / 18 output**. Existing network `network_hexapod.json` and configuration `webots_world/configs/config_hexapod_webots.json` are source references, not files to overwrite.

Preserve six legs × coxa/femur/tibia = 18 outputs and the lf/lm/lr/rf/rm/rr ordering. Preserve camera ON/OFF at inputs 30/31 and sonar 32/33 where confirmed by the selected mapping. Port actual servo limits, sensors and contacts.

Read the existing source, applicable alignment/configuration and the shared task contracts before editing. Generate native MJCF and USD articulations/sensors/actuators and the two backend adapters with exact named IO meaning, model/unit metadata and distinct per-instance AER/brain bindings. Preserve the existing aliases, all prior engine behaviours and checkpoints. Where existing semantics conflict, record a versioned migration or unresolved gate; equal vector lengths are insufficient evidence.

Add headless/interactive launch, physics/camera/contact tests, real AARNN round-trip and control, reset/reconnect/isolation and evidence. The complete eight-target matrix must retain current-backend regression rows as well as the two new backend rows. Blocked licences, physics calibration or missing toolchains remain visible and do not become success stubs.
