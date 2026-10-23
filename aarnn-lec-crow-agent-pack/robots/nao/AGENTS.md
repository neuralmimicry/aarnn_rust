# Port existing nao to MuJoCo and Isaac Sim

Verified snapshot projection: **250 sensory / 40 output**. Existing network `network_nao.json` and configuration `webots_world/configs/config_nao_webots.json` are source references, not files to overwrite.

Use permitted upstream/model assets and verify the exact NAO revision/licence. The source lacks an equivalent checked-in named alignment manifest; derive one explicitly before porting 250/40 projections. Do not assume motor count equals output count or fix hand/mimic joints by arbitrary indexing.

Read the existing source, applicable alignment/configuration and the shared task contracts before editing. Generate native MJCF and USD articulations/sensors/actuators and the two backend adapters with exact named IO meaning, model/unit metadata and distinct per-instance AER/brain bindings. Preserve the existing aliases, all prior engine behaviours and checkpoints. Where existing semantics conflict, record a versioned migration or unresolved gate; equal vector lengths are insufficient evidence.

Add headless/interactive launch, physics/camera/contact tests, real AARNN round-trip and control, reset/reconnect/isolation and evidence. The complete eight-target matrix must retain current-backend regression rows as well as the two new backend rows. Blocked licences, physics calibration or missing toolchains remain visible and do not become success stubs.
