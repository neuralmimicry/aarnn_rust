# Align sensory habitats and robot presentation across simulators

This ExecPlan is a living document maintained under `.agent/PLANS.md` and owned by
Phase 8 alongside `active-production-cross-review.md`.

## Purpose and observable outcome

Webots, Unity, Unreal and WebGL consume one reproducible catalogue of species
habitats and robot visual anatomy. Scenery supplies spatially meaningful sensory
cues. Content checks detect stale exports and incompatible channel counts.
Minecraft Java and native Bedrock now consume the same catalogue through the
additive [Minecraft delivery plan](minecraft-simulator-parity.md).

## Specification authority and traceability

Specification sections 2, 3.4, 16.17–16.24, 19.3, 20.9–20.10, 21.11–21.14 and
22 govern the adapter boundary; preserve INV-002, INV-007, INV-008, INV-015–017.
SIM-CONTENT-001 verifies catalogue/export identity, geometry, channel dimensions,
sensor response and browser presentation. IO-E2E production gates remain separate.

## Prerequisites and phase boundary

Phase 8 production gates remain open. This work improves existing reference
simulator adapters and visual content; it does not enable workstation_io or
replace the authoritative Rust executor, checkpoint schema or authorisation.

## Scope

Six robot profiles, five habitats, reproducible visual geometry, morphology
landmarks, world-related reference sensing, generator freshness and engine checks.

## Non-goals

No claim of biological adequacy, CFD, photorealistic parity or identical trajectories
between ODE, PhysX, Chaos and a bounded browser kinematic adapter. No mobile release,
privileged capture, live infrastructure mutation or production peripheral cutover.

## Repository orientation

Root: `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`. Cargo metadata saved to
`/tmp/aarnn-sim-cargo-metadata.json` confirms root package plus tools/xtask; no second
workspace is needed. Canonical content paths are `scripts/build_webots_*_assets.py`,
`scripts/build_webots_multi_world.py`, `webots_world/protos/`, Unity Runtime/Robots
under `sim/unity/Assets/NeuralMimicry/`, Unreal NmAerBridge under `sim/unreal/Source/`,
and `web_ui/webgl-sim.{html,js}` embedded by `src/bin/web_ui.rs`.
`scripts/robot_profiles.py` owns dimensions; Webots `.io_alignment.json` files record
ordered channels. Generated biological PROTOs must be changed through generators.
NAO uses the upstream Webots PROTO. Android/Gradle exists; iOS is absent; neither
is changed. CI uses locked Cargo checks and warning-tolerant Clippy. Native engine
commands are in sim/README.md. Webots, Chromium, Node, dotnet and UE build scripts
are installed; Unity editor availability is to be verified.

The initial tree contains user changes in runtime, launchers, simulator README,
WebGL assets, C. elegans config, Cargo.lock and the cross-review plan. Preserve them.
Work branch: codex/simulator-content-parity. No commits or deployment requested.

## Architecture and safety constraints

One versioned authored catalogue and procedural compiler produce bounded local
assets for all renderers. Geometry is expressed in explicit right-handed X-forward,
Y-left, Z-up coordinates; adapters convert units/axes. Physics hulls remain distinct
from visual detail. Anatomical labels describe idealised structures, not inferred
connectome locations. C. elegans has no eyes; zebrafish stage must not be called
larval while depicting adult pigment patterns without an explicit illustration note.
Environmental field/proximity approximations must be labelled as proxies. Existing
compatibility sensor/transport discrepancies are recorded instead of claimed equal.

## Milestones

1. Catalogue and generator: five repeatable habitats and six profile definitions;
   canonical dimensions, shared geometry/materials and meaningful cue metadata.
2. Integrate generated habitats and visual anatomy into four adapters, preserving
   articulated collision bodies; replace WebGL stick figures and fabricated sensors.
3. Run content/behaviour/freshness checks through xtask, browser screenshots and
   available native builds; record engine limitations and reproduction commands.

## Progress

- [x] `2026-09-15` Discovery: manifests, phase and cross-review plans, relevant
  specification, ADRs, deployment/mobile/CI definitions and dirty tree inspected.
- [x] `2026-09-15 10:57Z` Shared catalogue/compiler integrated into all four adapters;
  visual-only `scripts/regenerate_simulator_assets.py` regenerates six maintained
  standalone/mixed reference worlds plus the new hexapod world and biological PROTOs.
- [x] `2026-09-15 10:57Z` Six content tests plus spatial Node oracles pass through
  `cargo xtask qa run --suite simulator-content`; 9 launcher and 8 browser Rust tests
  pass. Browser Playwright lane passes all six profiles, cutaway, inference mock,
  stale-response cancellation, timeout recovery and mobile layout.
- [x] `2026-09-15 10:57Z` Unreal Editor build passes with shared content and cutaway.
  Native five-type null-RHI load constructs all habitats/robots. Webots supervisor
  report confirms 619 root nodes, 601 habitat objects and six robot profiles.
- [x] `2026-09-15 11:10Z` Final regeneration, formatting, Python/Node syntax, workflow YAML
  and scenario TOML parse checks pass. Webots and browser xtask lanes pass against
  content digest `c166b085a7d3d62bf6a7e75a86a5376f8459120239445491a170cd5bf057aae8`.
  Unreal rebuild passes after the final head-direction and 95-muscle corrections.
- [x] `2026-09-15 11:10Z` `cargo check --locked --bin web_ui`, `cargo clippy --locked --bin
  web_ui -p aarnn_rust`, and the xtask unit test pass. Clippy was installed for the
  pinned 1.92.0 toolchain; existing warning-level findings remain, with no new deny
  policy or unrelated fixes. `git diff --check` passes.
- [x] `2026-09-15 11:11Z` Final Unreal own-framebuffer capture observed the compiled
  `c166b085...` agar scene after camera framing, including the 189-part worm model.
  Screenshot, digest report and build/render logs are retained under
  `target/qa/simulator-content/unreal-final/`. This is one rendered habitat,
  not an all-profile visual/physics acceptance result. Task-owned servers and probes
  have stopped; prior user changes remain in place.
- [ ] Unity Editor build/play-mode, calibrated native sensor/motor parity and
  cross-engine dynamics/visual acceptance remain unverified; no Unity Editor was
  found in the installed toolchains. These are required before a full parity claim.
- [x] `2026-09-15 13:11Z` Superseding water revision: **602 habitat objects**,
  digest `4d0d506e80f146acf04e34ee781f67ab465c6d1f336b73c799bc3e3904a6b2d8`.
  The earlier 601-object counts and `c166b085...` digest describe the pre-water
  baseline only. Shared stream water now covers the fish in every adapter;
  Webots uses freshwater Fluid/immersion properties, Unity/Unreal/WebGL use
  transparent water, and both Minecraft editions use a glass tank with native water.
  WebGL/Java depth input derives from the authored `.625` water surface.
- [x] `2026-09-15 13:11Z` Final browser checks pass in
  `target/qa/simulator-content/browser-m1nxueil/`, and Webots constructs six
  robots/602 objects in `target/qa/simulator-content/webots-r1e_rg82/`.
  Unreal's water rebuild passes in `/tmp/aarnn-minecraft-water-unreal-build.log`;
  `/tmp/aarnn-unreal-water-final.png` records a native fish/water frame. Its bounded
  render process was terminated after capture, so this is not clean-exit acceptance.
  Minecraft evidence is indexed in `sim/minecraft/VALIDATION.md`.

## Validation and acceptance

`cargo xtask qa run --suite simulator-content` will check generator freshness and
bounded geometry/profile/sensor fixtures. Run Node syntax and browser automation,
focused Rust browser/launcher tests, `cargo fmt --all --check`, `git diff --check`,
and available native engine builds. Record required unavailable engines as not-run.
Compare objects/materials/anatomy against one catalogue digest; numerical renderer
and rigid-body differences need separate calibration, not a content hash.

Recorded commands (all from the repository root):

- `python3 scripts/regenerate_simulator_assets.py --check`: every export and maintained
  Webots PROTO/world matches its generator; no network/config/editor settings regenerated.
- `cargo xtask qa run --suite simulator-content`: six Python tests plus Node spatial,
  retinal and muscle oracles; result in `target/qa/simulator-content/contract-yxwwbxug/`.
- `NODE_PATH=/tmp/aarnn-sim-browser/node_modules NM_CHROMIUM=/opt/google/chrome/chrome
  cargo xtask qa run --suite simulator-content-browser`: Chrome 139.0.7258.138, six
  profiles, zero page errors, mock inference/disconnect/stale response/timeout/mobile
  checks; `target/qa/simulator-content/browser-i5i5h7f3/`.
- `cargo xtask qa run --suite simulator-content-webots`: Webots R2025a constructs six
  profiles and 601 habitat objects; `target/qa/simulator-content/webots-4fmd6pnz/`.
  Mass-ratio warnings remain explicit; this lane does not assert dynamics stability.
- `cargo test --locked --test web_ui_browser_compat --test run_examples_launcher`:
  8 browser and 9 launcher tests pass; `/tmp/aarnn-sim-rust-tests.log`.
- `cargo test --locked -p xtask`: one catalogue test passes.
- `cargo check --locked --bin web_ui`: passes; `/tmp/aarnn-sim-web-ui-check.log`.
- `cargo clippy --locked --bin web_ui -p aarnn_rust`: passes with existing warnings;
  `/tmp/aarnn-sim-clippy.log`. `cargo fmt --all --check` and `git diff --check` pass.
- `python3 -m py_compile` on the changed Python sources, `node --check` on browser
  renderer/client/QA sources and Python YAML/TOML parsers pass.
- `/home/pbisaacs/Developer/Engine/Build/BatchFiles/Linux/Build.sh
  NeuralMimicrySimEditor Linux Development
  -Project=/home/pbisaacs/Developer/neuralmimicry/aarnn_rust/sim/unreal/NeuralMimicrySim.uproject
  -WaitMutex`: Unreal 5.8 editor target builds; `/tmp/aarnn-sim-unreal-build.log`.
- Unreal render command uses the absolute project path, default template map with
  `?game=/Script/NmAerBridge.NmSimGameMode`, `-game -unattended -RenderOffscreen
  -nosound -nosplash -stdout -FullStdOutLogOutput`, `NM_UE_ROBOTS=celegans=1`,
  `NM_AARNN_BASE_PORT=61900` (no brain listening), and `NM_SIM_CONTENT_CAPTURE` set
  to the desired absolute PNG. A 75-second external timeout bounds this observation;
  timeout exit is not itself treated as an engine pass.

`qa/scenarios/SIM-CONTENT-001.toml` records scope, fixture procedure and bounds.
The xtask lanes emit unique machine-readable evidence bundles including schema,
Git/dirty state, toolchains, manifest/content/input hashes, capability presence,
CPU/RSS/time and pass/fail reasons. CI executes and retains the contract lane.
Browser/Webots capabilities are optional lanes; when requested, absence fails the
lane instead of claiming success. Unity was not found under installed editor/app
locations and is not reported as built or tested.

## Rollout, compatibility and rollback

Additive catalogue schema v1; regenerated exports are checked in for offline engine
use. Changes touch reference worlds, visual anatomy and explicitly identified reference
transducer/output-map corrections. Revert task-owned
catalogue/adapter changes together; no persisted brain migration or point of no return.

## Risks and mitigations

Native materials, axes and hull scale differ: explicit transforms and engine evidence.
Extra detail can overload draw calls: bounded primitive counts and reusable meshes.
Old native I/O maps differ: verify by names, do not infer equivalence from vector size.

## Surprises & Discoveries

- WebGL was line geometry over a grid with sine-wave sensors unrelated to scenery.
- Unity worm has colliders but no visible meshes and a different 24-channel order;
  Unreal worm has another order. These are compatibility defects, not alternate biology.
- Hexapod canonical dimensions are 34/18; README says 32, native camera sizing varies.
- Unreal random terrain uses global FRand and independently authored habitat content.
- Existing fish pigment comments call alternating transverse segment colours
  anatomically correct; adult zebrafish stripes are longitudinal. Current rigs and
  transducers are engineering proxies and cannot certify real animal behaviour.

- Webots repeated root object names across five habitats; the real load probe
  exposed the collisions and names are now scoped by habitat. No dummy controllers
  connect to a brain during this probe.
- Shared humanoid table height initially exceeded the normalized robot's reach;
  it was lowered and a reach envelope assertion added. Fish reference extent now
  preserves approximately the prior 0.22 m water-surface presentation.
- Native fly/fish maps still conflict with Webots, including imported fly output
  readouts lacking a validated muscle-to-joint map. This is a recorded legacy
  adapter migration in Phase 8, not authority to guess biological semantics.
- Unreal null-RHI logs show native CDO physical-material initialisation errors and
  a NAO leaving its habitat without a connected brain; Webots warns of large mixed
  mass ratios. No claim of stable cross-engine trajectories is made.
- An immediate Unreal screenshot precedes camera framing and material warmup. The
  existing delayed diagnostic callback now supports optional own-framebuffer QA
  capture through NM_SIM_CONTENT_CAPTURE; the warmed agar view verifies shared
  geometry and colour while all-profile rendering parity remains unverified.
- The user correctly identified that the original stream looked dry. Water-surface
  markers alone did not represent an underwater habitat. The superseding explicit
  fluid volumes and native fish screenshots resolve that content defect; they do
  not establish calibrated hydrodynamics. Surface/depth values now share one source.

## Decision Log

- `2026-09-15 SIM-001`: Use repository-native procedural geometry and shared authored
  content rather than external raster/asset dependencies. Reference dimensions and
  idealised anatomy are documented; no scientific measurement is invented.
- `2026-09-15 SIM-002`: Separate exact content parity from numerical/physics parity.
  Preserve production gates and document incompatible existing mappings explicitly.

- `2026-09-15 SIM-003`: Retain physical collision rigs, native gains and compatibility
  transport. Fix proven canonical worm/hexapod mappings and anatomical head direction;
  do not invent a biological fly motor assignment. Remaining mapping/stability
  evidence belongs to the Phase 8 native parity gate. Legacy inactive Webots ecology
  helper functions remain through this review/rollback window; remove them after
  native acceptance instead of reintroducing an independently authored live path.
- `2026-09-15 SIM-004`: Regenerate only visual assets through a bounded wrapper;
  preserve user-dirty network/config/editor work. The historical saved NAO world is
  replaced with a clean reference pose and a lower reachable table. Date-stamped
  world captures stay historical. The Webots-only fridge switch now fails explicitly.

## Outcomes & Retrospective

The shared-content implementation and browser/reference validation are in place.
The final shared catalogue contains 602 objects, including explicit fish water;
the earlier counts/digest above are retained historical evidence. Java and native
Bedrock delivery and real snapshot checks are recorded in the Minecraft plan.
Native validation is partial: direct rendered comparison and stability cannot be
inferred from asset digests. See `sim/content/README.md` for the evaluation and
remaining migration gates. No production phase gate is promoted.
