# Add a versioned morphology and responsive simulation boundary

This ExecPlan is a living document maintained under `.agent/PLANS.md`.

## Purpose and observable outcome

The repository will have one portable contract for constrained 3D growth,
physical route timing, versioned structural commits and display metadata. The
existing Runner, distributed executor, Rust UI and web UI remain runnable while
the new contract is adopted behind explicit adapters. The first accepted slice
has deterministic tests for swept-volume collision, competing growth proposals,
route timing and conservative time quantisation.

## Specification authority and traceability

The governing sources are `docs/specifications/distributed-whole-brain-emulator-v1.1.md`,
the morphology brief supplied on 2026-09-24, and the repository invariants in
`AGENTS.md`. This plan maps the initial slice as follows:

| Requirement | Current implementation/evidence | This slice |
| --- | --- | --- |
| DATA-01..07 | `src/deterministic.rs`, `src/topology_model.rs`, legacy `src/morphology.rs` | `src/morphology_contract.rs` stable anatomical IDs, revisioned state and explicit units |
| ENV-01..07 | legacy spatial grid/octree in `src/morphology.rs` | finite environment, swept segment admission and deterministic reservations |
| GROW-01..08 | `Runner` growth cadence and async morphology path | reference `growth_cone::propose_step`, bounded forward candidates, shared counter RNG, swept capsules, stalls/deferrals; `MORPH-GROW-001`; live policy/branch scheduling and full lifecycle incomplete |
| GROW-09 | point-only import path in `src/runner.rs` | deterministic paired axon/dendrite reconstruction in `src/morphology_contract.rs`, persisted provenance and explicit route failures |
| SIG-01..07 | `Runner::rebuild_syn_maps_from_morph`, `src/aarnn/transmission.rs` | route arc length, velocity, receiving response and conservative quantisation |
| COM-01..07 | `src/topology_model.rs::ExecutionPlanRegistry` | contract activation metadata; full Runner adoption remains staged |
| CORE-01..04, RUN-01..04 | `Runner` async morphology path and runtime worker boundary | no UI dependency in the new contract; load isolation integration remains open |
| VIS-01..11, UI-01..04 | `src/ui.rs`, `web_ui/app.js`, Android `MainActivity.kt` and iOS `AarnnConnectomeView.swift` | shared display vocabulary, adjacency-aware colour slots, volumetric neurite polygons, activity brightness cues and versioned `display_snapshots`; generated physical routes are consumed by anatomical adapters |
| DIST-01..03 | stable shard plan, causal transport and phase gate tests | physical route identity is independent of placement; distributed morphology activation remains next |
| QA-01..04 | existing morphology, UI and phase gate suites | three new deterministic contract tests |

The new contract preserves `INV-002`, `INV-004`, `INV-007`, `INV-008`,
`INV-009`, `INV-014` and `INV-013`; it does not claim the later distributed
activation gate is complete.

## Prerequisites and phase boundary

Phase 0 remains open for repository CI tiers and unavailable device evidence.
The growth-cone policy is additive, has no live production caller, and does not
replace legacy biological transitions. Earlier point-only reconstruction is
persisted as a derived import artefact, while display adapters are read-only.
The existing phase 2–8 safety gate must remain green.

## Scope

- Add `src/morphology_contract.rs` as the portable domain boundary.
- Validate coordinates, radii, velocities, route samples and environment bounds.
- Admit sorted growth proposals against an immutable base revision and finite
  swept-volume environment, with bounded retained history and explicit rejects.
- Compute route delays from centreline arc length and conservative quantum
  rounding. Preserve logical tags and route epochs.
- Make the endpoint vocabulary explicit as `Anatomical` and
  `SyntheticColumns`; update existing UI labels without changing numerical
  semantics.
- Make anatomical snapshots visibly represent the supplied reference target:
  soma bodies, continuous branching axons and dendrites, route traces, and
  distinct bouton, postsynaptic-site and synapse markers. These are derived
  from committed geometry and update as growth changes the published view.
- Record the audit, requirement matrix and repeatable commands in this plan.

## Non-goals

- No wholesale extraction of the current 8,560-line legacy morphology module.
- No automatic claim that procedural paths are observed anatomy; GROW-09 records
  procedural provenance and failed route outcomes.
- No live distributed morphology transaction, database migration, mobile
  standalone-brain work or actuator policy change in this slice. Mobile display
  clients consume read-only snapshots and do not gain execution authority.
- No renderer-driven simulation state or unbounded snapshot copying.

## Repository orientation

The Git/workspace root is `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`.
`cargo metadata --format-version 1 --no-deps` reports one primary package
(`aarnn_rust`), the `aarnn-biox6-exporter` path package and `tools/xtask`.
The canonical execution paths are `src/runner.rs`, `src/engine.rs`,
`src/runtime.rs`, `src/topology_model.rs`, `src/causal.rs` and
`src/distributed.rs`. The existing renderer paths are `src/ui.rs` and
`web_ui/app.js`, served by `src/bin/web_ui.rs`; Android consumes the same
gateway through `apps/android/app/src/main/java/com/neuralmimicry/aarnn/RemoteAarnnClient.kt`,
and the portable iOS sources are `apps/ios/AarnnRemoteSession.swift` and
`apps/ios/AarnnConnectomeView.swift`. There is no separate browser package.
Persistence is JSON snapshots through `src/runtime.rs` and
`Runner::export_network_json`/`import_network_json`. Morphology is feature gated
by `morpho`; topology and growth scheduling are feature gated by `growth3d`.

The legacy morphology model stores dense layer/neuron indices in `Soma`, `Axon`,
`Dendrite` and `Synapse`; `Runner` keeps exact per-synapse axon/dendrite length
caches and asynchronously applies evolution results. The distributed stable
path already has typed identities, topology/partition generations, route
validation and activation at logical boundaries. Rust UI has `Conventional` and
`Aarnn` layout modes; web UI has matching `conventional` and `aarnn` modes.

## Architecture and safety constraints

The new contract uses stable `(value, generation)` anatomical IDs and treats
dense indices as execution coordinates. A growth proposal carries both
morphology and environment base revisions. Proposals are sorted by ID and
validated against the full interpolated segment, including radius and
clearance. Accepted proposals publish one new immutable revision; rejected
proposals carry a reason. Physical path length is piecewise-linear over stored
samples. A route's biological delay is path length divided by velocity, plus
declared synaptic and receiving response time, quantised upward to avoid early
delivery. Network arrival/transport time is not consulted.

The contract is framework-free and does not import UI, HTTP, gRPC, database,
OS, GPU or media dependencies. It is a reference boundary, not a claim that
the old Runner has already migrated to stable anatomical IDs.

## Milestones

### Milestone 1 — Portable contract

Completed by `src/morphology_contract.rs`: units, frames, anatomical elements,
paths, route timing, finite environment, swept collision, exact segment
reservations, ownership validation, revisioned growth store and deterministic
point-only connectome reconstruction. Proved by
`cargo test --locked morphology_contract::tests --lib` (7 tests).

### Milestone 2 — Existing path compatibility

Use the contract in route and display adapters. Keep legacy `morpho` snapshots
readable, retain the current Runner path behind its feature flags, and add a
migration report before changing authoritative persisted morphology.

### Milestone 3 — Shared anatomical snapshot

Complete for the available native/web/mobile source surfaces: `DisplaySnapshot` in `src/morphology_contract.rs` is a bounded
read-only snapshot adapter consumed by `src/ui.rs`, `src/runtime.rs`,
`src/bin/web_ui.rs` and `web_ui/app.js`. It carries schema, morphology,
topology and route epochs, sequence, coverage, truncation and provenance. The
current legacy topology adapter is explicitly procedural and marks missing
physical paths; anatomical rendering falls back when geometry is unavailable.
Point-only imports now persist their generated paths and the anatomical view
consumes them. Android and the portable iOS SwiftUI source consume the same
endpoint and expose both modes. The iOS Xcode packaging and device evidence
remain unavailable in this checkout. Delta transport and distributed
structural activation remain open.

### Milestone 4 — Distributed structural activation

Bind accepted change sets to affected stable shard owners and the existing
`ExecutionPlanRegistry`; activate only at a future logical boundary after both
endpoint owners prepare. Add fault/retry/reclamation tests before enabling it.

## Progress

- [x] `2026-09-24` Diagnosed the output-raster mismatch. `Runner` was
  evaluating every configured output row, importing/startup was repairing empty
  rows with synthetic hidden-to-output weights, and morphology mode fell back
  to those weights when `recv_out` had no committed route. The raster then
  faithfully displayed those generated spike vectors, while a missing
  voltage-only fallback could also fabricate raster activity in the native UI.
  Morphology output drive now uses committed `recv_out` routes only; empty
  imported rows remain empty; all UI rasters use explicit spike frames; and the
  native diagnostic reports matrix-connected versus committed anatomical output
  routes. Evidence: `morphology_output_drive_requires_a_committed_route` and
  `fresh_runner_derives_morphology_from_default_topology` pass.

- [x] `2026-09-24 15:30Z` Sustained repair verified by the complete registered
  lane: `cargo xtask qa run --suite anatomical-growth` passed with evidence in
  `target/qa/anatomy/run-gv17oaz7`. Seed 42; initial 210 hidden cells; 1,200
  steps; 13 snapshots; two actual I/O admissions; 2,804 protected-root checks.
  Final capture: 218 cells and 4,088 untruncated paths. Native CPU meshes and
  Chrome repeated pixels/identity ownership passed. Capture took 82.52 seconds
  in the debug offline profile; this is explicitly not a real-time claim.
- [x] `2026-09-24 15:30Z` Final regression evidence: 445 library tests passed
  with `ui,engine_runtime,morpho` (the two costly tests passed separately in
  the registered lane); 19 headless morphology-contract tests; 14 UI interface
  checks; seven phase gates. `cargo clippy --locked --features
  'ui,engine_runtime,morpho' --lib --bins` passed with repository warnings.
  Formatting, JavaScript/Python syntax and diff-whitespace checks passed.
  Native GPU and mobile-device tests, full connectivity parity and stimulus
  contention/resource-isolation gates remain open.

- [x] `2026-09-24 15:15Z` Reproduced destructive reconstruction at I/O formation
  and replaced both resize callers with incremental peripheral morphology.
  Eleven focused anatomy tests pass, including preserved old geometry/contact
  references, shrink/regrow, stale-worker fencing, non-blocking polling and
  identical parallel reconstruction using one/four Rayon workers.
- [x] `2026-09-24 15:15Z` `MORPH-VIS-002` captures 1,200 shipped-network steps,
  compares old arbors at I/O boundaries and exercises native meshes and Chrome
  on the actual progression. Browser parity exposed numeric u64 ID rounding;
  schema-2 decimal-string identities and compatible native/web/mobile readers
  are implemented; host/browser evidence is above. Mobile device evidence
  remains unavailable. No live checkpoint or deployed client is modified.

- [~] `2026-09-24 14:56Z` Reopened sustained-growth visual instability from five
  native captures: short arbors at 131/321 ms become a large fan by 506 ms and
  remain distorted at 1040 ms. Static snapshot QA did not cover this trajectory.
  Trace real growth and geometry publication with the shipped 3x70 network;
  inspect blocking work and parallel safety as explicitly requested. Preserve
  all previous dirty changes and the distributed activation boundary.
- [x] `2026-09-24 14:40Z` Final verification passed: 440 feature-enabled library
  tests; 18 headless morphology/growth tests; 14 source/interface parity checks;
  7 phase gates; 7 native CPU mesh tests and real Chrome canvas checks.
  Final visual bundle: `target/qa/anatomy/run-_ujdxns2`; growth bundle:
  `target/qa/growth-cone/run-akuo8xpj`. Formatting, JavaScript/Python syntax and
  diff whitespace checks passed. Android/iOS builds, native GPU operation,
  serial feature build and sustained contention remain unverified as recorded.
- [x] `2026-09-24 14:39Z` Incorporated the user's growth-cone guidance in a
  renderer-independent reference policy under `src/morphology_contract/`.
  Reuse `deterministic::CounterRng` and existing growth proposal contracts;
  bound candidates and neighbourhood work, preserve forward heading, test
  capsule occupancy and report stalled/deferred outcomes. This is preparatory
  growth policy work under specification sections 8.4 and 13, not permission
  to bypass the distributed activation gate or silently replace the legacy
  numerical model. Known-target import reconstruction remains distinct.
  Evidence: 18 headless contract tests in `MORPH-GROW-001`, result bundle
  `target/qa/growth-cone/run-akuo8xpj`; all 440 feature-enabled library tests
  passed (`/tmp/aarnn-lib-growth-final.log`).
- [x] `2026-09-24 14:25Z` Rechecked the prior evidence: 431 feature-enabled library
  tests and 14 UI source/interface plus 7 phase-gate tests passed. Browser/native
  CPU evidence bundle: `target/qa/anatomy/run-v2kin3y4`. The serial profile
  `--no-default-features --features morpho,growth3d` cannot compile: 25 existing
  Rayon/parallel-iterator errors in `src/ga.rs`, `src/runner.rs` and
  `src/morphology.rs:3690` (`/tmp/aarnn-serial-growth.log`). Serial execution is
  unverified; repairing the wider feature matrix is outside this correction.
- [x] `2026-09-24 14:20Z` Corrected mixed native projection and completeness gating:
  `src/ui/anatomy.rs` projects somas, paths and contacts from one immutable
  frame, clips local 3D cylinder faces to the actual projected membrane and
  retains valid truncated views. No entire concave tube outline is passed to
  convex tessellation. Legacy PID/depth styling is excluded from this path.
- [x] `2026-09-24 14:20Z` Added optional `coverage.membrane` (centre and radii) to the
  shared display DTO. Native, web, Android and iOS use the published membrane
  for their cloudy fill and clipping; box-only environments remain boxes.
  Updated mobile synthetic lines, soma colours and Android activity ordering.
- [x] `2026-09-24 14:20Z` Fixed growth movement beyond the intended local step after
  adding activity terms, capped trunk interpolation to `[0,1]`, and preserved
  intermediate-parent attachment fractions. Regression exercises eight large
  accumulated intervals and a branch attached halfway along another branch.
  Both execution implementations are maintained; the serial build limitation
  is recorded above.
- [x] `2026-09-24 14:20Z` Broad regression initially failed six checkpoint/export tests
  with `key must be a string`: reconstructed `MorphologyState` maps used
  structured anatomical IDs as JSON object keys. Sorted identity/value entry
  arrays now preserve generations; empty legacy objects remain readable and
  duplicate identities are rejected. All 431 feature-enabled library tests
  subsequently passed, including all six failures.
- [x] `2026-09-24 14:20Z` Added `MORPH-VIS-001`, a shared four-neuron procedural fixture,
  native mesh checks and actual Chrome canvas pixel checks. The browser test
  verifies repeated unchanged frames, deliberate oversized geometry, bounded
  snapshots and synthetic feedback connectivity. Native fixture output uses
  the production CPU mesh builder; it is not a native GPU capture.

- [x] `2026-09-24 14:20Z` Reopened and diagnosed native instability after further user captures.
  Verified mixed PID/topology soma coordinates versus membrane-centred neurites,
  convex-only tessellation of bent tube outlines, and completeness gating that
  switches presentation for valid bounded snapshots. The previous source tests
  did not establish visual stability. Preserve the dirty worktree on
  `fix/anatomical-render-stability`; behavioural mesh/browser evidence is now
  recorded above. This does not close the native GPU/live-growth gate.

- [x] `2026-09-24 07:00Z` Read repository instructions, phase plans, normative
  specification, manifests and architecture decisions; confirmed a clean
  worktree and one primary Cargo package with mobile source directories.
- [x] `2026-09-24 07:20Z` Ran baseline evidence: Phase 0 fixture passed,
  morphology feature suite passed (6 tests), UI topology suite passed (8
  tests), and `phase2_to_phase8_gate` passed (7 tests).
- [x] `2026-09-24 07:45Z` Added stable morphology IDs, validated units and
  coordinate frames, swept-volume environment admission, deterministic growth
  batching, route timing and display contracts in
  `src/morphology_contract.rs`; added three deterministic tests.
- [x] `2026-09-24 07:50Z` Changed the morphology route cache's physical
  length-to-step conversion in `src/runner.rs` from nearest rounding to upward
  rounding so a stored route cannot be scheduled early.
- [x] `2026-09-24 08:55Z` Added bounded `DisplaySnapshot` DTOs with explicit
  provenance, coverage, epochs and stable display identities. Added runner and
  runtime adapters, workspace/API response fields, and web graph consumption;
  native UI stores both contract views on a bounded cadence. Legacy points are
  labelled procedural and unavailable anatomy does not enter the contract
  renderer.
- [x] `2026-09-24 09:20Z` Snapshot integration is green for native and web
  compilation, JavaScript syntax validation and focused DTO/engine tests. The
  native path avoids matrix extraction on its simulation thread; runtime and
  web snapshots include bounded edges. Delta resynchronisation,
  physical-path extraction and UI parity acceptance remain the next work item.
- [x] `2026-09-24` Added GROW-09 point-only import reconstruction. Dense
  topology coordinates are converted into deterministic stable identities;
  inner/dense neurons
  are processed first, paired axon/dendrite routes reserve swept 3D volumes,
  and rejected routes remain explicit. The reconstruction is persisted with
  snapshot provenance and used by the anatomical display adapter.
- [ ] Complete distributed prepare/activate/recovery evidence for structural
  changes.
- [x] `2026-09-24 10:30Z` Replaced the anatomical display fallback for live
  legacy morphology with a bounded adapter that publishes soma identities,
  axon/dendrite branch centre-lines and synaptic route polylines. Native UI
  snapshots now request anatomical geometry and render these paths; web UI
  preserves route samples and draws them as polylines. Evidence: focused live
  morphology display test, native feature check and `node --check web_ui/app.js`.
- [x] `2026-09-24 10:45Z` Added a synthetic view for point-only imports using
  the same reconstructed soma identities and all source connectome edges.
  Delayed web responses are rejected by display sequence and transient fetch
  failures retain the last committed graph. Evidence: mode identity assertion
  in the live morphology test and JavaScript syntax check.
- [x] `2026-09-24 11:15Z` Cross-checked every checked-in connectome UI source.
  Android now fetches and renders both `display_snapshots` modes, including
  stored path polylines; iOS now exposes the same decoded contract and a
  SwiftUI connectome view. WebGL remains a separate robot-body anatomy
  surface, not a connectome viewer. Evidence:
  `connectome_display_contract_is_exposed_across_supported_ui_sources`.
- [x] `2026-09-24` Fixed the missing imported-connectome neurite display. The
  point-only anatomical snapshot now emits every committed physical route as
  an owner-linked `DisplayPath`, retaining the combined route edge for
  inspection. Native and web membrane fills retain their hull calculation and
  cloudy appearance while all outer hull strokes are suppressed. Native path
  colours and widths were increased for reliable axon/dendrite visibility.
  Evidence: `point_only_anatomical_display_exposes_paired_axon_and_dendrite_paths`,
  feature-enabled `live_morphology_display_contains_paths_and_keeps_mode_ids_stable`,
  and the cross-UI parity suite.
- [x] `2026-09-24` Removed anatomical path flicker during concurrent UI and
  simulation updates. Both simulation branches now refresh the bounded display
  contract, and native rendering keeps the last complete UI topology snapshot
  when the simulation writer holds the publication lock. Evidence: native
  feature check, formatting/diff checks and the 14-test UI parity suite.
- [x] `2026-09-24` Fixed fresh default-network ordering: `Runner::new` now
  commits the default soma topology before applying initial AARNN wiring and
  rebuilding morphology. Every non-zero initial matrix edge is seeded with an
  axon route, dendrite route and committed anatomical synapse before the first
  display snapshot. Evidence: `fresh_runner_derives_morphology_from_default_topology`
  and `matrix_connectome_edges_seed_complete_aarnn_routes_deterministically`.
- [x] `2026-09-24` Added explicit anatomical contact markers to the shared
  display contract. Boutons, postsynaptic sites and synapses are emitted as
  bounded point markers alongside soma nodes, neurite paths and route edges;
  Rust UI, web UI, Android and iOS render the same marker kinds. Evidence:
  the contract and live morphology tests plus the cross-UI parity suite.
- [x] `2026-09-24` Added adjacency-aware deterministic colour slots to the
  shared display nodes. Each renderer uses the same slot-derived hue, with
  axon and dendrite hues offset from the owning soma. Added a contract test
  proving adjacent nodes receive different slots.
- [x] `2026-09-24` Changed anatomical highest-detail rendering from centreline
  strokes to filled swept polygons derived from stored path radii. Combined
  route edges remain available for inspection but are suppressed from the
  anatomical canvas so they cannot appear as artificial point-to-point
  anatomy. Active neuron presentation adds a bounded white brightness overlay
  to its soma-owned neurite polygons in native, web and Android clients; iOS
  accepts the same active-ID overlay input.
- [x] `2026-09-24` Stabilised anatomical projection and containment across
  supported clients. Live coverage now comes from the committed skull
  membrane frame; native, web, Android and iOS no longer derive the projection
  from changing neurite extents or the animated synthetic-layout pivot. Route
  points are bounded to the coverage region, the web canvas clips to its
  membrane hull, and the shared route samples retain their bends. The white
  membrane contour remains removed while the cloudy fill and bounds are kept.
  Evidence: the focused feature and parity tests below, plus source-level
  inspection of the four adapters. Device rendering remains unavailable on
  this Linux host.
- [x] `2026-09-24 13:33Z` Bounded the swept polygon vertices as well as their
  centre-lines in the native, Android and iOS anatomical adapters. This keeps
  tube radii within the committed presentation frame at the client boundary.
  Evidence: `cargo check --locked --features 'ui,engine_runtime'`,
  `cargo fmt --all --check`, `node --check web_ui/app.js` and
  `git diff --check` passed; the source parity suite remains green.
- [x] `2026-09-24` Added the fill-only committed membrane cue to the Android
  and iOS contract views. Both mobile adapters now consume the same coverage
  region as native and web, draw a cloudy translucent fill without a contour,
  and retain the same anatomical/synthetic mode semantics.
- [x] `2026-09-24` Removed a remaining source of visible route instability.
  Native anatomical mode now suppresses all legacy live morphology, matrix,
  transmission and feedback overlays even during a contract publication gap;
  web anatomical mode retains the last complete contract graph in the same
  situation. Growth endpoint search no longer uses the process-global random
  generator: its samples are derived from the endpoint state and its search
  step is capped at `0.02` model units. Membrane energy fluctuation is also
  deterministic. Evidence: the repeatability/step-bound test, 8 morphology
  tests, 9 native presentation tests, 14 cross-interface tests and JavaScript
  syntax validation.

## Validation and acceptance

The sustained-instability repair has the following focused mapping. It does
not close the broader release gates listed in Outcomes & Retrospective.

| Requirements | Implementation | Repeatable evidence |
| --- | --- | --- |
| CORE-03, DATA-03, VIS-01, QA-04 | `Morphology::resize_io`, `Runner::resize_io_morphology` and preserved peripheral topology coordinates | `anatomy_io_resize_preserves_grown_arbors_and_contact_indices`; `MORPH-VIS-002` protected-root checks across actual development |
| GROW-05, COM-01 (local legacy boundary only) | existing worker sequence fenced on peripheral resizing | `anatomy_io_resize_fences_in_flight_geometry`; distributed activation remains open |
| RUN-02/03, ENG-02 | receiver try-lock, capacity-one result channel, immutable parallel base, Arc display publication | `anatomy_worker_poll_yields_when_receiver_is_busy`, `anatomy_parallel_reconstruction_is_independent_of_worker_count`; no stimulus-latency gate claimed |
| DATA-01/07, UI-01/04, VIS-10 | schema-2 string IDs; native/web/Android/iOS readers | `anatomy_identity_json_preserves_full_unsigned_range_and_legacy_input`; actual browser owner/cardinality assertions; mobile device checks not run |
| VIS-09, UI-03, QA-04 (rendering subset) | existing production native mesh builder and browser canvas | `anatomy_captured_growth_meshes`; `test_anatomical_render.cjs --browser --sustained`; synthetic/physical connectivity discrepancy explicitly recorded |

Run the full progression with `cargo xtask qa run --suite anatomical-growth`.
It runs the focused regressions, explicit costly capture and mesh tests, then
Chrome. The two costly tests are excluded from ordinary library runs but are
mandatory in this registered suite. Captures use seed 42, shipped `config.json`,
CPU execution and `NM_MORPHO_ASYNC=0`; these are offline numerical/geometry
checks, not production latency measurements. Avoid concurrent Cargo builds
during this lane because build-lock wait counts against its wrapper deadlines.

Current evidence:

- Final growth/render correction regression: `cargo test --locked --features
  'ui,engine_runtime,morpho' --lib` — 440 passed. `cargo xtask qa run --suite
  morphology-growth-cone` — 18 passed with seed 42 and source/configuration
  digests in `target/qa/growth-cone/run-akuo8xpj/result.json`.
- `cargo test --locked --test web_ui_browser_compat --test phase2_to_phase8_gate`
  — 14 source/interface and 7 phase-gate tests passed on the final code.
- `cargo xtask qa run --suite anatomical-render-browser` — 7 native CPU mesh
  tests and Node/real Chrome tests passed; final retained bundle:
  `target/qa/anatomy/run-_ujdxns2/result.json`.
- `node scripts/qa/test_anatomical_render.cjs --browser` — real Chrome
  repeated-frame and membrane pixel checks passed after the growth additions.
- Serial feature profile remains unavailable: `cargo test --locked
  --no-default-features --features 'morpho,growth3d' morphology::tests:: --lib`
  fails on 25 existing Rayon/parallel feature-gating errors, including
  `src/ga.rs`, `src/runner.rs` and `src/morphology.rs:3690`. This is not a passed
  serial-growth test.
- `cargo metadata --format-version 1 --no-deps` — workspace inventory passed.
- `cargo test --locked --test phase0_baseline` — 1 passed.
- `cargo test --locked --features 'growth3d,morpho' morphology::tests:: --lib` — 6 passed.
- `cargo test --locked --features 'ui,engine_runtime' topology_presentation_tests --lib` — 8 passed.
- `cargo test --locked --test phase2_to_phase8_gate` — 7 passed.
- `cargo test --locked morphology_contract::tests --lib` — 7 passed, including
  canonical point-only reconstruction, paired-route occupancy and explicit
  failed-route retention.
- `cargo check --locked --features 'growth3d'` — passed.
- `cargo check --locked --features 'growth3d,ui,engine_runtime'` — passed.
- `cargo test --locked engine::tests::display_snapshot_is_versioned_bounded_and_explicit_about_provenance --lib`
  — 1 passed.
- `cargo check --locked --bin aarnn_rust --features 'ui,engine_runtime'` —
  passed.
- `cargo check --locked --bin web_ui --features web_ui_workload` — passed.
- `node --check web_ui/app.js` and `git diff --check` — passed.
- `cargo test --locked --test web_ui_browser_compat` — includes native/web and
  Android/iOS source parity assertions; passed after the mobile adapter slice.
- `cargo test --locked morphology_contract::tests --lib` — 8 passed, including
  the imported-connectome axon/dendrite display-path regression.
- `cargo test --locked --features 'morpho,growth3d' runner::tests::fresh_runner_derives_morphology_from_default_topology --lib`
  — passed; the default AARNN startup morphology contains one anatomical
  synapse and route for every non-zero initial matrix edge, with both endpoint
  attachment indices present.
- `cargo test --locked --features 'morpho,growth3d' live_morphology_display_contains_paths_and_keeps_mode_ids_stable --lib`
  — 1 passed, including live axon and dendrite path kinds.
- `cargo fmt --all --check`, `cargo check --locked --features 'ui,engine_runtime'`
  and `node --check web_ui/app.js` — passed after the renderer change.
- `cargo test --locked --test web_ui_browser_compat` — 14 passed after the
  concurrent snapshot cache change.
- `JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 ./gradlew testDebugUnitTest`
  — not run to completion: the host has no configured Android SDK. iOS Xcode
  packaging is also unavailable on this Linux host, and the repository has no
  checked-in Xcode project. These remain explicit Phase 8 packaging gates.
- `cargo test --locked morphology_contract::tests --lib` — includes the
  adjacency-aware display colour slot test.
- `cargo test --locked --features 'ui,engine_runtime' topology_presentation_tests --lib`
  — native overlay gating and snapshot retention tests remain green after the
  volumetric renderer change.
- `node --check web_ui/app.js` and `git diff --check` — passed after the web
  polygon and activity overlay change.
- `cargo fmt --all --check` — passed after stable projection changes.
- `cargo test --locked morphology_contract::tests --lib` — passed after the
  bend-sample regression and coverage contract changes.
- `cargo test --locked --features 'ui,engine_runtime'
  topology_presentation_tests --lib` — passed after native coverage-frame,
  clamping and snapshot-retention changes.
- `cargo test --locked --test web_ui_browser_compat` — passed after web,
  Android and iOS coverage-region decoding and projection changes.
- `node --check web_ui/app.js` and `git diff --check` — passed after the final
  clipping and projection changes.
- `cargo check --locked --features 'ui,engine_runtime'` — passed after the
  final native polygon-bound change; existing repository warnings remain.
- `cargo test --locked --features 'morpho,growth3d'
  morphology::tests::energy_biased_growth_search_is_repeatable_and_step_bounded
  --lib` — passed.
- `cargo test --locked --features 'morpho,growth3d' morphology::tests:: --lib`
  — 8 passed.
- `cargo test --locked --features 'ui,engine_runtime'
  topology_presentation_tests --lib` — 9 passed after legacy overlay
  suppression.

The new tests prove the 1 mm at 1 m/s plus 0.5 ms example quantises to at
least 2 ms at a 1 ms quantum, a thin obstacle blocks a long swept step, and
proposal order/revision/occupancy decisions are deterministic. These tests do
not yet prove biological calibration, distributed activation, native device
rendering or live UI parity. Source-level parity is covered for Rust UI, web
UI, Android and the portable iOS SwiftUI source.

## Rollout, compatibility and rollback

Schema 2 of the reference morphology, reconstruction and display contracts
writes anatomical `value` fields as decimal strings in JSON. Rust input still
accepts schema-1 numeric IDs; binary ID representation remains u64. Web uses
exact string keys and rejects unsafe legacy numeric IDs rather than attaching
paths to the wrong owner; iOS accepts either representation and Android parses
the full unsigned range. Upgrade clients before emitting schema 2. Old numeric
JSON snapshots remain readable without changing biological identities. For
rollback use the retained pre-upgrade checkpoint; do not feed schema-2 JSON to
an old client. No live state migration or store publication was performed.

The new module remains additive. Point-only reconstruction is activated only
when an imported snapshot has no persisted reconstruction, and it is retained
as a versioned derived snapshot field. Removing that field and the import call
returns to the legacy point-only display path; neural matrices remain
unchanged. The generated anatomy is not yet a distributed structural commit.

## Risks and mitigations

- The legacy model has dense indices and procedural geometry. Keep an explicit
  adapter and do not equate its indices with contract identity.
- Conservative collision tests may reject more growth than a future calibrated
  tissue model. Keep clearance and obstacle policy configurable and record
  parameter provenance.
- Upward delay quantisation changes a feature-gated morphology path. The change
  is limited to route-cache preparation, is covered by existing morphology
  timing tests, and can be reverted before persisted authoritative adoption.
- Full UI and distributed integration remain incomplete until the next
  milestones and must not be reported as complete from the contract tests.
- Product parity is split by evidence: Rust UI and web UI have compiled
  adapters, Android has a source and Gradle project but no emulator run in this
  session, and iOS has portable Swift sources without an Xcode project.
- The native UI previously treated a failed `try_read` of its shared snapshot
  as an unavailable topology and could fall back for one frame. It now retains
  the last complete immutable UI projection when the short read is busy; the
  simulation writer remains non-blocking and skips publication if its short
  write lock is busy.
- The current display adapter scans legacy matrices only when a bounded view is
  requested and refreshes the native UI copy every 100 simulation steps. This
  is a scheduling boundary, not performance evidence; contention benchmarks
  remain outstanding.

## Surprises & Discoveries

- The shipped 3x70 hidden-cell configuration reproduces the reported transition
  at 500 ms: development calls `Runner::resize_sensory` and `resize_output`,
  which rebuilt all morphology from the weight matrices. Seed 42's synchronous
  diagnostic changed from 2,636 displayed paths at 400 ms (longest 0.133) to the
  4,096 display cap at 500 ms (longest 0.994). Existing grown arbors were replaced
  with star routes. `/tmp/aarnn-sustained-before.log` and
  `/tmp/aarnn-growth-trace/frame-*.json` retain the diagnostic; it was stopped
  after reproduction, not reported as a passing test or real-time benchmark.
- `Morphology::from_weights` used a raw pointer to read a synapse vector during
  parallel mutation of that vector. Use an immutable base for this relaxation;
  retain parallelism and verify independence from Rayon worker count.
- The new direct resize regression also found `from_weights` assigning the
  first axon segment as its own parent when an AARNN soma had no separate
  hillock segment. Parent is now absent in that case; siblings attach to the
  soma independently. Both legacy LIF and AARNN resize fixtures validate every
  parent and reciprocal contact reference after append, shrink and regrowth.
- The real-growth crosscheck reveals a remaining connectivity discrepancy:
  at 400 ms the synthetic view reaches its 4,096-edge cap from learned matrix
  weights, while live morphology has no formed synapses (but has growing
  neurite segments). At 500 ms it has 17 physical synapses. Do not claim full
  QA-04 connectivity parity from the renderer tests. Reconciling learned
  weights with committed physical routes is still an open Runner integration
  requirement; hiding weights or drawing invented anatomy would conceal it.
  The web canvas also remains a 2D projection, without the native orbit view;
  full VIS-06 navigation parity remains open.
- The first `MORPH-VIS-002` harness run (`run-3uz13a61`) failed its boundary
  coverage assertion: it inferred growth admission from a snapshot sequence.
  The 13 captured states and native/browser mesh checks passed. Correct the
  harness to observe actual I/O population changes on every step and compare
  each layer separately, including concurrent hidden-cell admission.
- The next sustained oracle (`run-j6lwte0q`) incorrectly treated normal leaf
  pruning on the 1,000-ms step as I/O data loss. The exact direct-resize oracle
  remains unchanged. The sustained oracle now checks every model-protected
  root across each actual I/O admission, with bounded ordinary movement, while
  allowing the same step to prune/compact leaves. This follows the existing
  explicit root protection in `Morphology::evolve`, not an increased tolerance
  or a disabled growth rule. Failed bundles are retained.
- `run-uc4fido7` passed the sustained capture (two I/O admissions, 2,804
  protected roots), native mesh assertions and browser frames, but failed the
  native command's 120-second wrapper deadline while waiting for overlapping
  Cargo checks/recompilation. Keep this bundle failed. Re-run the complete lane
  without competing Cargo jobs, with the same budgets. QA now terminates the
  whole child process group on timeout and stops dependent render checks on
  an earlier failure; a timed-out Cargo parent previously left its test alive.
- Native display extraction held the publication lock and readers deep-copied
  display geometry. Extract before publication and share immutable `Arc` views.
  Runner-owned extraction, route-cache rebuilds, cloning at worker admission,
  global Rayon contention and frame-time tube construction remain synchronous
  costs; these changes do not establish CORE-01's latency/resource-isolation gate.

- `2026-09-24 14:35Z`: The reference environment's `contains_segment` is a slab
  intersection test, unsuitable for proving containment. Both endpoints now
  must lie in the box eroded by radius plus clearance. Parallel capsule
  distance also needed reclamping after selecting a closest endpoint; reversed
  and tiny collinear segments are now tested. These are physical admission
  corrections, separate from render clipping.
- `2026-09-24 14:35Z`: Reference `MorphologyStore` checked endpoint chords only
  against the current batch, ignored previous committed paths, and incremented
  the executable epoch for all geometry. It now checks stored segments against
  previous and accepted paths, accepts only `Proposed` elements, and leaves the
  executable epoch unchanged. No production caller uses this store. Terminal
  parent attachment is explicit; fractional branch admission remains open.


Follow-up audit: MORPH-014/015 overstated evidence. Rectangle clamping does not
clip an ellipsoid or a soma hull; earlier mobile code left offset polygon
vertices unclipped. The screenshots do not prove the hypothesised legacy
fallback cause. `update_skull_membrane` energy noise is an energy term, not
boundary motion. Truncated complete messages are valid bounded snapshots, not
partial publications. `Morphology::evolve` additionally extrapolates trunk
interpolation for large accumulated `dt` and rewrites non-root attachments;
this needs a growth regression independently from rendering. Pending distributed
activation and full biological occupancy gates remain incomplete.

The repository already has substantial procedural morphology and renderer
support, but those paths are feature gated and use dense indices in the legacy
model. The stable distributed topology model already provides the generation,
route and activation concepts needed for later integration. Web and Rust UI
already offer two layouts, so the first UI change is terminology and shared
contract work rather than a second renderer.
The WebGL surface is a separately scoped robot-body simulator with its own
`Anatomy cutaway` control; it does not consume connectome `DisplaySnapshot`
records and is therefore excluded from neural anatomical/synthetic parity.

## Decision Log

- `2026-09-24 MORPH-021`: I/O resizing appends/removes only peripheral ownership
  and contacts, retaining hidden arbors, soma positions and membrane state.
  Removed contacts clear/remap segment references. Asynchronous old-population
  results are fenced with the existing morphology sequence. Full rebuild stays
  available for initialisation and incompatible legacy state. Evidence: the
  500-ms reproduction and `anatomy_io_resize_*`. This is a legacy compatibility
  repair, not the distributed COM-04/06 removal/activation protocol.
- `2026-09-24 MORPH-022`: Replace raw-pointer parallel reads with an immutable
  synapse base; keep Rayon computation. Use a one-result channel and try-lock
  polling. Build native display updates outside the publication lock and share
  `Arc` snapshots; return retired handles outside that lock. Synchronous CPU
  work, single in-flight morphology cloning and global pool contention remain
  limitations, so CORE-01/RUN-01 and QA-05 are still open.
- `2026-09-24 MORPH-023`: Version JSON anatomical identities as decimal strings.
  A 210-neuron real snapshot contains role-tagged IDs above 2^53; browser
  numbers rounded many different owners to one key. Small-ID static fixtures
  missed this. Use one serde boundary, retain numeric input compatibility and
  update every UI reader; add full-u64 round trips and real-growth browser
  owner/cardinality assertions. Authority: DATA-01/07, UI-01/04 and VIS-10.

- `2026-09-24 MORPH-019`: Implement the local growth-cone proposal kernel as a
  separate submodule of the existing portable contract, reusing `CounterRng`
  rather than introducing another random stream implementation. Versioned
  parameters describe procedural assumptions. The reference neighbourhood has
  bounded capsule/obstacle sizes; incomplete occupancy defers. Known-target
  reconstruction and legacy live growth retain their declared models until a
  controlled activation migration. Automatic branching, retraction and coarse
  A* are not claimed. Authority: GROW-02/05/07, ENV-02/03, specification sections
  8.4 and 13, INV-008/009. No new dependency or persisted live-model change.
- `2026-09-24 MORPH-020`: Correct physical boundary, complete-path occupancy and
  parallel-distance tests in the reference contract. Keep morphology proposals
  electrically inactive, with unchanged topology epoch, until the separate
  topology transaction commits. Evidence: `MORPH-GROW-001` and reference
  proposal tests. These checks cannot repair legacy anatomy already outside
  its domain or replace distributed occupancy/fencing.

- `2026-09-24 MORPH-016`: Publish the actual membrane separately from coverage
  bounds and use one snapshot projection per anatomical frame. Clip local tube
  faces rather than moving centreline samples onto box edges. Accept complete
  bounded messages even when they are not whole-brain-complete. This corrects
  the presentation faults recorded in MORPH-014/015; it does not establish
  physical occupancy of legacy geometry. Native mesh, web pixel and mobile
  source evidence have different scopes and are reported separately.
- `2026-09-24 MORPH-017`: Correct the procedural compatibility growth
  implementation's final movement bound and interpolation overshoot; retain
  each existing parent attachment instead of replacing it with segment zero.
  The 0.02-model-unit step was already intended by MORPH-015. This is a bug
  correction to that bounded-step policy, not a biological calibration or a
  new distributed numerical model. Existing geometry outside the physical
  environment is not teleported; complete collision-safe legacy migration and
  distributed activation remain outstanding.
- `2026-09-24 MORPH-018`: Encode `MorphologyState` identity maps as sorted
  `[identity, value]` entries for JSON. The previous populated representation
  was never JSON-serialisable; only its empty `{}` representation could have
  been written and remains readable. The enclosing unreleased schema stays
  at version 1. No live data is migrated. Earlier builds cannot restore a
  populated reconstruction; retain original source exports for rollback.

- `2026-09-24 MORPH-001`: Add a portable contract beside the legacy morphology
  module. Evidence: `src/morphology.rs` is feature gated and dense-indexed;
  extracting it wholesale would violate the incremental migration boundary.
  Consequence: no authoritative legacy snapshot changes yet.
- `2026-09-24 MORPH-002`: Quantise route delays upward at the morphology route
  cache. Authority: SIG-01/SIG-02 and `INV-002`/`INV-004`; consequence is at
  most one quantum of execution lateness for the reference cache.
- `2026-09-24 MORPH-003`: Treat competing same-base proposals as a sorted,
  bounded reservation batch. Authority: ENV-06, GROW-05 and `INV-009`; the
  first deterministic proposal may commit and later conflicts are retained as
  explicit rejections.
- `2026-09-24 MORPH-004`: Publish display views as derived DTOs with explicit
  provenance and completeness. The legacy `Topology3D` adapter may support
  navigation but cannot claim observed anatomy or physical route geometry.
  Authority: CORE-03/04, DATA-06 and UI-02; consequence is that unavailable
  anatomical data falls back to the existing renderer rather than fabricating
  an anatomical witness.
- `2026-09-24 MORPH-005`: Reconstruct imported point-only routes in the
  portable contract, with paired branch admission and deterministic reservations
  rather than calling the legacy renderer's decorative geometry authoritative.
  Authority: GROW-09, ENV-02/03/06 and INV-008/009/014. Route failures remain
  in the report and do not delete the corresponding connectome edge.
- `2026-09-24 MORPH-006`: Make live morphology paths part of the shared display
  contract and keep point-only synthetic/anatomical node IDs identical across
  modes. Authority: CORE-03/04, VIS-01/02/05 and UI-01/02; consequences are a
  backward-compatible `paths` field and explicit procedural provenance.
- `2026-09-24 MORPH-007`: Retain the last committed client graph through
  transient polling errors and reject older responses by display sequence.
  Authority: UI-02/03/04 and INV-009; this affects presentation state only and
  cannot alter neural execution.
- `2026-09-24 MORPH-008`: Extend the read-only display contract to Android and
  portable iOS sources through the workspace snapshot endpoint. Keep WebGL as a
  separate robot anatomy surface and report missing Xcode/emulator evidence
  explicitly. Authority: UI-01/02/04, DIST-03 and product-profile rules;
  consequence is shared presentation semantics without claiming mobile
  packaging or device validation.
- `2026-09-24 MORPH-009`: Emit imported physical paths separately from route
  edges, and render the membrane as fill-only. Evidence: the point-only
  reconstruction already committed paired `PhysicalPath` values, but its
  anatomical DTO exposed only combined edges; the membrane outline obscured
  the intended cloudy boundary. Authority: CORE-03/04, VIS-01, UI-01 and the
  user's display requirement. Consequence: all supported contract consumers
  receive explicit axon/dendrite kinds, while route inspection and membrane
  bounds remain available.
- `2026-09-24 MORPH-010`: Treat the last complete display contract as the
  renderer's presentation fallback while a new simulation snapshot is being
  published. Evidence: the UI and simulation share a short-lived lock and a
  missing/partial read previously allowed geometry to disappear between
  frames. Authority: CORE-01/02, DATA-06, UI-02/03 and INV-009. Consequence:
  presentation may briefly lag one bounded snapshot, but it cannot alter
  numerical state or display a partial anatomy update.
- `2026-09-24 MORPH-011`: Build default topology before initial wiring and
  morphology derivation. Evidence: the synthetic matrix view could contain
  edges while the first anatomical snapshot had no corresponding routes when
  topology was rebuilt later in `Runner::new`. Authority: CORE-03, VIS-01/02
  and the startup consistency requirement. Consequence: the initial
  anatomical and synthetic views share the same non-zero connectome edges;
  procedural geometry remains explicitly labelled as such.
- `2026-09-24 MORPH-012`: Put adjacency-aware presentation colouring in the
  versioned display DTO rather than reimplementing graph colouring in each
  client. A deterministic greedy assignment derives a slot from stable ID and
  probes conflicts against visible neighbours. Renderers use the same hue
  wheel and fixed axon/dendrite offsets. Authority: VIS-01/04/06 and UI-01.
- `2026-09-24 MORPH-013`: Render stored neurite centre-lines as filled swept
  polygons using their physical radii in highest-detail anatomical mode.
  Suppress combined route strokes in that mode while retaining route DTOs for
  inspection and timing. Activity overlays are presentation-only and keyed by
  the current activity projection. Authority: VIS-01/08, CORE-03 and SIG-06.
- `2026-09-24 MORPH-014`: Anchor every anatomical client to the committed
  membrane coverage frame, and bound or clip projected neurite geometry to
  that frame. Preserve stored bends and retain the last complete snapshot
  during transient publication or transport gaps. Authority: VIS-09, VIS-10,
  UI-02/03 and INV-009. Consequence: presentation can remain briefly stale
  under contention, but it cannot jump with changing route extents, expose
  neurites beyond the membrane frame or replace a curved route with an
  unstable straight chord.
- `2026-09-24 MORPH-015`: Anatomical presentation is an exclusive rendering
  mode. Legacy live overlays cannot be composited when the anatomical contract
  is temporarily unavailable, and web clients retain the last complete
  contract instead of falling back to matrix geometry. Growth search uses a
  state-derived deterministic stream and a bounded local step. Evidence from
  the paired captures showed that the previous fallback and unseeded endpoint
  sampling were materially changing the apparent pipe paths between frames.
  Authority: CORE-03/04, GROW-07, UI-02/03 and INV-008/009.
- `2026-09-24 MORPH-021`: Treat a committed physical output route as a
  prerequisite for morphology-mode electrical drive. A matrix weight can
  describe an imported or pending connectome edge, but it cannot substitute for
  an active anatomical route; this preserves COM-02, SIG-01 and INV-009.
  Remove voltage-derived raster events because presentation must report
  committed spike vectors rather than infer biological events from membrane
  potential. Empty output rows are preserved through import and startup so
  reconstruction can report them as pending or absent instead of silently
  inventing topology.

## Outcomes & Retrospective

The reproduced 500-ms destructive anatomy replacement is repaired. I/O
formation preserves grown hidden arbors, and stale asynchronous results cannot
overwrite the new population. Parallel reconstruction no longer reads another
worker's mutable synapse state. Snapshot readers share immutable geometry;
publication and worker polling avoid blocking lock acquisition, while the
remaining synchronous preparation/destruction costs are explicitly recorded.
Schema-2 IDs prevent browser rounding from merging distinct neuron owners.

The final evidence bundle includes `growth-metrics.json`, `result.json`,
`diagnostic-comparison.json`, the before/after JSON captures,
`native-growth-{0400,0500,1200}.svg` and `web-growth.png`. At 500 ms the old
diagnostic showed 3,604 displayed segments longer than 0.2 fixture units and
hit the 4,096-path cap; the repaired run shows 36 such segments and 2,982 paths.
That threshold only illustrates the mass reconstruction; it is not a growth
constraint or biological calibration. Both endpoints render the supplied
snapshots, but synthetic weights still outnumber formed physical contacts.
Full anatomical/synthetic connectivity parity is therefore **incomplete**.

This is a partial delivery against the full morphology brief. The following
mandatory release work remains incomplete; the earlier requirement table maps
code, not blanket acceptance of each grouped requirement:

| Open requirement groups | Missing release evidence or implementation |
| --- | --- |
| CORE-01/02, AUD-03/05, RUN-01..05, ENG-04, QA-05/06 | Measured single/multi-node envelope, resource isolation/adaptive budgets, 60-minute contention, failure/capacity results and instrumentation overhead |
| DATA-01..07, ENV-01..07 | Full legacy-to-stable migration, local frames and immutable chunks at scale; complete soma/neurite occupancy, spatial indexes/halos, environment edits and distributed reservations |
| GROW-01..09 | Live growth-cone integration, branching/taper/retract/prune lifecycle, activity feedback, automatic reconstruction and failure handling across every import/startup path under full occupancy constraints |
| SIG-01..07 | Shared explicit dendritic electrical model, physical-route activation across all runtime profiles, route-epoch activity and full timing acceptance; current lumped dendritic timing is not cable biophysics |
| COM-01..07, DIST-01..03, QA-03 | Distributed prepare/activation/recovery, in-flight pruning/migration, bounded safe reclamation and exactly-once fault evidence |
| VIS-01..08, UI-01..04, QA-04 | Native GPU/mobile runtime parity, all intermediate views/navigation, progressive loading/deltas/reconnection, route-timestamp activity, and full headless/UI trace equivalence |
| DONE-01..03 | Release benchmark bundles, full cross-product demonstrations, complete mandatory-requirement acceptance and all supported target checks |

The cone kernel is callable headlessly and passes reference geometry admission;
it is not enabled in the live Runner. No stochastic branching scheduler or
coarse A* router is enabled by this change. Reference admission deliberately
retains proposed lifecycle and executable epoch until later electrical commit.
Its bounded kernel does not establish bounded memory for the whole reference
store, which still clones state and retains its proposal-deduplication set.

Follow-up visual stability evidence supersedes earlier source-only completion
claims. The native renderer no longer mixes PID somas with fixed neurites;
local triangulation and the actual membrane boundary pass behavioural tests.
Real Chrome canvas tests pass; Android/iOS changes are source-reviewed only.
The native GPU application, live long-duration growth/rendering, spatial
occupancy of all legacy anatomy, distributed structural activation and mobile
device parity are not release-validated by this slice. Global release gates
in the morphology brief remain open.

The first vertical slice is working and independently testable. It now carries
the shared derived display contract through the runtime and web response path,
and the native UI records the same versioned views without placing rendering
work in the stimulus loop. Point-only imports now generate and persist
procedural physical routes with deterministic occupancy admission. It still
does not implement deltas or activate distributed structural edits; generated
routes remain an import artefact until that later commit protocol is added.
