# Acceptance and evidence

The deliverable is complete only at the declared fidelity tier. Functional-surrogate success does not close the exact-OEM-replica gate. A build, import, animation, mocked exchange and trained neural task are separate pieces of evidence.

## Gates for every profile/target

| Gate | Required evidence |
|---|---|
| LEC-001 Provenance | Exact source/version/hash/licence; variant and every physical assumption stated. |
| LEC-002 Import | Native model loads with resolved dependencies, valid mass/inertia, articulation/constraints and sensor/actuator names. Rendered and headless results identified separately. |
| LEC-003 Mapping | Ordered names, dimensions, units, signs, address windows and digests agree with the selected model and brain. Mismatch rejected. |
| LEC-004 Mechanics | Deterministic diagnostic impulse/joint/contact and task-affordance probes: free fall, wheel contact, grasp, loaded motion, cleaning/process conservation as applicable. |
| LEC-005 Transport | Actual AER packet round-trip, timestamps/epochs/bounds/empty payload/error paths and mapping isolation; mock tests only establish the protocol part. |
| LEC-006 Neural loop | Real AARNN sensory admission and output apply correctly to a body; task success measured with diagnostic control disabled and silence/random controls. |
| LEC-007 Lifecycle | Stop, revoke, disconnect, reset, stale effect, duplicate response and restart; no cross-instance state or output contamination. |
| LEC-008 Performance | Bounded memory/queues, render responsiveness and p50/p95/p99 end-to-end latency under a reproducible load. Budget values are configured and measured, not asserted. |
| LEC-009 Parity | Common fixture semantics and task outcomes across engines within declared tolerances. Identical neural replay does not imply identical floating-point physical trajectories. |
| LEC-010 Usability | Unified launch, model/variant/fidelity display, headless/view attach, pause/step/disarm and clear missing-capability errors. |

`verification/acceptance-matrix.json` starts with **224 not_run rows**, including all 28 profiles and both Minecraft editions. Preserve every row. A requested engine that cannot run on the host is blocked/not_run with a named runner/toolchain requirement. Do not silently mark it passed from XML/USD structure or another engine’s result.

## Minimum scenarios

Use the model-specific scenarios in `manifests/robots.json`. Additionally:

- Two same-type robots with different brains, plus different-type robots in one world; perturb one and prove sensor/output isolation. Test two worlds with independent epochs.
- Handshake/mapping mismatch; malformed/truncated/oversized AER; invalid addresses; stale and duplicate frames; wrong brain; absent grant; late input; queue pressure; disconnect during application.
- Pause/reset/reload invalidates cached joint indices and drains stale output; observe the next source step and epoch. Body-only reset cannot silently reset a shared remote brain.
- A viewer disconnect leaves an authorised headless world running; a stop/revoke operation disarms it regardless of render/network congestion.
- Crow mechanics and neural evaluation cover the falsification checks in `crow-biology.md`, then walking, takeoff, glide, banking, landing, perching, grasp/feeding, energy and gust recovery.
- Existing C. elegans, BANC/FAFB, hexapod, NAO and zebrafish preserve their ordered mappings on MuJoCo/Isaac. Hydrodynamics, muscle mapping and skin/rig differences remain explicitly calibrated or blocked.

## Evidence bundle

Each run records requirement/scenario IDs; profile/variant/model/IO/content hashes; Git revision/dirty state; dependency and engine versions; CPU/GPU/driver; exact command/config/seed; controller mode; brain/checkpoint identity; admission/committed-output trace; physics/sensor/actuator samples; wall-clock latency/queue/resource measurements; status/reason; and target-specific captures when rendered. Persist outcome metrics and failure seeds.

Framebuffers and videos demonstrate appearance or movement only. Protocol goldens use exact bytes; neural deterministic-reference fixtures use the repository oracle; physical and learned-task tests use declared numeric/statistical criteria. Record effect and input gaps without leaking credentials or raw private media.

## Current baseline commands

These commands exist in the inspected source; rediscover prerequisites and run them on the user’s current checkout before relying on historical results:

```bash
git status --short
cargo metadata --no-deps --format-version 1
python3 scripts/regenerate_simulator_assets.py --check
cargo xtask qa run --suite simulator-content
cargo test --locked --test web_ui_browser_compat --test run_examples_launcher
cargo fmt --all --check
```

Use the repository’s named product feature sets for Rust checks. Do not assume `--all-features` is valid for every platform. Extend the QA scenario catalogue and xtask with new embodiment, bridge, MuJoCo and Isaac lanes before documenting their final commands. This pack does not invent those future commands as already available.
