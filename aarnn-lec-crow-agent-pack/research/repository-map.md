# AARNN integration map

Inspected public `main` at `a02c9c43089b5cb488094b50124d034ae51b4ca3`, committed 2026-10-13T23:55:25Z. The recursive Git tree was complete (not truncated). The user’s `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust` checkout was not mounted here. Reconcile current local files and dirty work before implementation.

| Responsibility | Verified canonical paths | Integration consequence |
|---|---|---|
| Repository authority | `AGENTS.md`, `.agent/PLANS.md`, `docs/specifications/distributed-whole-brain-emulator-v1.1.md`, `docs/execplans/phase-08-workstation-io.md` | Add a living feature plan, preserve phase gates and INV-002/007/008/009/010/015/016/017. |
| Profile registry | `scripts/robot_profiles.py` | Currently six canonical neural profiles; add model-derived entries without renaming existing aliases. |
| Shared content | `sim/content/catalog.json`, `scripts/sim_content.py`, `scripts/regenerate_simulator_assets.py` | Extend authored schema/compiler. Do not hand-edit generated renderer exports. |
| Unified launchers | `scripts/run_sim.sh`, `scripts/run_multi_robot_webots.sh`, `scripts/run_webgl_sim.sh`, `scripts/run_minecraft_sim.sh` | Existing dispatch and some per-type logic are finite/hard-coded; registry entry alone is insufficient. Add real MuJoCo/Isaac dispatch, readiness, cleanup and network plumbing. |
| Webots assets/controller | `webots_world/protos/`, `webots_world/configs/`, `webots_world/controllers/nao_nn_controller_uds/nao_nn_controller_uds.cpp`, `scripts/build_webots_multi_world.py` | Device discovery, IO alignment, world/PROTO generation and bridge startup all need extension. The NAO controller name is historical, not proof it supports any new body. |
| Unity | `sim/unity/Assets/NeuralMimicry/Runtime/NmRobotBase.cs`, `NmAerClient.cs`, `NmSimulationManager.cs`, `Robots/` | One bounded background exchange exists; robotSpec in the manager is informational and does not by itself spawn new prefabs. Add generated construction/prefab/factory integration. |
| Unreal | `sim/unreal/Source/NmAerBridge/`, especially `Private/NmSimGameMode.cpp`, `NmAerClient.cpp`, `Robots/` | Existing class/type dispatch, spawn and scaling rules need extension; do not inherit illustrative biological scaling for real metre-sized hardware. |
| Browser | `web_ui/webgl-sim.js`, `web_ui/`, generated shared content | Current rendering/kinematic adapter and authorised `/api/aer/infer` path. Full physics mode requires an authoritative physics service and a single sensory producer. |
| Minecraft Java | `sim/minecraft/src/main/java/org/neuralmimicry/minecraft/` | Companion and mod use AER1 TCP bridge; six-profile assumptions and participant limits must be generalised. |
| Minecraft Bedrock | `sim/minecraft/bedrock/`, `scripts/build_minecraft_bedrock.py` | Separate rendering/runtime/export path; test independently from Java. |
| Legacy AER codec | `src/aer.rs`, `scripts/aer_legacy_codec.py`, `examples/nn_tcp_server.rs`, `examples/nn_uds_server.rs` | AER1 payload, binary spikes, vector/name negotiation; cannot encode negative motor drive by sending value zero. |
| TCP-to-distributed adapter | `src/tcp_aer_ipc_bridge.rs`, `src/bin/tcp_aer_ipc_bridge.rs` | Reuse or extend the canonical bridge; do not create new per-engine neural execution servers. |
| Governed AER/peripherals | `src/aer_transport.rs`, `src/peripheral.rs`, `src/bin/web_ui.rs` | AARNN-AER/1 has epochs/sequences/clock provenance. Authorised input routes and committed-output effects remain canonical. |
| Managed simulation ingress | `docs/architecture/decisions/ADR-0007-managed-simulation-ingress.md`, `src/bin/web_ui.rs` | `/api/simulation/aer/inject` is service-authorised virtual input and is not interchangeable with workstation capture. |
| FPAA experimental fabric | `aer_fabric_bridge/README.md`, `aer_fabric_bridge/src/aer/` | Separate synapse-addressed UDP protocol, not a native robot SDK or AER1-over-UDP. |
| QA | `scripts/qa/run_simulator_content.py`, `scripts/qa/test_simulator_content.py`, `qa/scenarios/SIM-CONTENT-001.toml`, `tools/xtask/` | Extend existing evidence orchestration. Content tests are not neural or dynamics proof. |
| New backends | No MuJoCo/Isaac-specific paths found in this tree | `sim/mujoco/` and `sim/isaacsim/` are proposed new adapter locations, subject to local discovery. |

## Existing semantic debt that the expansion must not conceal

`sim/content/README.md` documents native fly/fish channel differences, missing NAO alignment, nonphysical presentation scale and unfinished native calibration. Preserve existing names and checkpoints while resolving or explicitly versioning mappings. Do not conclude that matching vector lengths makes a trained network portable.

`sim/minecraft/README.md` still says authenticated `/api/aer/infer` is rejected. The inspected implementation actually calls `authorize_peripheral_aer_input` before inference. Use source and endpoint tests to establish the current contract; do not disable authentication to reproduce an outdated sentence.

Existing source claims and old ExecPlan results are historical evidence. This pack did not run an AARNN neural network, a native renderer, MuJoCo or Isaac Sim.
