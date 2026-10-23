# Port existing zebrafish to MuJoCo and Isaac Sim

Verified snapshot projection: **32 sensory / 32 output**. Existing network `network_zebrafish.json` and configuration `webots_world/configs/config_zebrafish_webots.json` are source references, not files to overwrite.

Preserve the 32/32 projections after reconciling native/Webots mappings. Include articulated body/muscle readouts, declared buoyancy/drag/flow and physically consistent water interaction. A transparent water mesh is not hydrodynamic validation. Retain the existing larval-inspired anatomy label and avoid inferring adult stripes or biological dimensions from display scale.

Read the existing source, applicable alignment/configuration and the shared task contracts before editing. Generate native MJCF and USD articulations/sensors/actuators and the two backend adapters with exact named IO meaning, model/unit metadata and distinct per-instance AER/brain bindings. Preserve the existing aliases, all prior engine behaviours and checkpoints. Where existing semantics conflict, record a versioned migration or unresolved gate; equal vector lengths are insufficient evidence.

Add headless/interactive launch, physics/camera/contact tests, real AARNN round-trip and control, reset/reconnect/isolation and evidence. The complete eight-target matrix must retain current-backend regression rows as well as the two new backend rows. Blocked licences, physics calibration or missing toolchains remain visible and do not become success stubs.
