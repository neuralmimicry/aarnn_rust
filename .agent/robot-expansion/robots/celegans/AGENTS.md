# Port existing celegans to MuJoCo and Isaac Sim

Verified snapshot projection: **24 sensory / 96 output**. Existing network `network_celegans.json` and configuration `webots_world/configs/config_celegans_webots.json` are source references, not files to overwrite.

Preserve the 24 external inputs and 96 muscle readouts. The 302 biological neurons are not those IO channel counts. Reuse named MDL/MDR/MVL/MVR groupings and the MVULVA exception from the existing mapping; do not reinterpret them as interleaved segment motors. Port the segmented compliant body and declared contact/muscle response.

Read the existing source, applicable alignment/configuration and the shared task contracts before editing. Generate native MJCF and USD articulations/sensors/actuators and the two backend adapters with exact named IO meaning, model/unit metadata and distinct per-instance AER/brain bindings. Preserve the existing aliases, all prior engine behaviours and checkpoints. Where existing semantics conflict, record a versioned migration or unresolved gate; equal vector lengths are insufficient evidence.

Add headless/interactive launch, physics/camera/contact tests, real AARNN round-trip and control, reset/reconnect/isolation and evidence. The complete eight-target matrix must retain current-backend regression rows as well as the two new backend rows. Blocked licences, physics calibration or missing toolchains remain visible and do not become success stubs.
