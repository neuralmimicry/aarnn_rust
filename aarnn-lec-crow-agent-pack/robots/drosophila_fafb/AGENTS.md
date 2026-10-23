# Port existing drosophila_fafb to MuJoCo and Isaac Sim

Verified snapshot projection: **418 sensory / 48 output**. Existing network `network_drosophila_fafb.json` and configuration `webots_world/configs/config_drosophila_fafb_webots.json` are source references, not files to overwrite.

Retain FAFB-specific imported identities and network, separate from BANC. Sharing an anatomical body does not authorise sharing neuronal identities or mutable sensor/output buffers. Resolve the existing channel discrepancy before controller-transfer claims.

Read the existing source, applicable alignment/configuration and the shared task contracts before editing. Generate native MJCF and USD articulations/sensors/actuators and the two backend adapters with exact named IO meaning, model/unit metadata and distinct per-instance AER/brain bindings. Preserve the existing aliases, all prior engine behaviours and checkpoints. Where existing semantics conflict, record a versioned migration or unresolved gate; equal vector lengths are insufficient evidence.

Add headless/interactive launch, physics/camera/contact tests, real AARNN round-trip and control, reset/reconnect/isolation and evidence. The complete eight-target matrix must retain current-backend regression rows as well as the two new backend rows. Blocked licences, physics calibration or missing toolchains remain visible and do not become success stubs.
