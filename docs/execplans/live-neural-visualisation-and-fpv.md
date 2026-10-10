# Deliver faithful live neural rendering and FPV exports

This ExecPlan is a living document maintained under `.agent/PLANS.md`. Current status: read-only display consistency and verified empty-contact coverage are implemented; authoritative physical initialization and import migration remain open. Update Progress, Surprises & Discoveries, Decision Log and Outcomes during each session.

## Purpose and observable outcome

Deliver the end-to-end actual-network experience specified by `docs/specifications/live-neural-visualisation-and-fpv-v1.md`: select high detail, navigate in 3D, inspect anatomy/activity, capture actual state over time, plan/preview a flight and retrieve a verified animation after disconnect. Keep neural execution independent and responsive.

## Specification authority and traceability

Read `AGENTS.md`, `.agent/PLANS.md`, the distributed emulator specification, the complete morphology/responsive-simulation specification and the existing morphology and FPV ExecPlans before production changes. NVR-001..039 supplement them; NVR-037..039 capture the user's default/import physical-build and automatic calibration requirements.

Particularly relevant invariants are INV-002..004 (logical time), INV-007..010 (no event loss, deterministic replay, atomic topology and independent components), INV-011 (authorisation), INV-012 (immutable checkpoint), INV-014 (authoritative route ownership), INV-015 (capture provenance) and INV-016..017 (external effects and peripheral consent). All other invariants remain in force. Map the geometry work to existing VIS-09..12, CORE and UI/QA requirements in the active traceability table.

A display observer can be developed behind a disabled feature using validated fixtures while upstream phase gates are incomplete. That does not authorise presenting provisional geometry as committed or declaring the actual-runtime acceptance gate complete.

## Prerequisites and phase boundary

1. Verify current branch/HEAD, instructions and dirty work; compare against reviewed `0c561abe210238e3dfabeb81a667f541d7d0b38f`.
2. Discover current physical-geometry publication, structural activation, stable IDs, route telemetry and committed capture/checkpoint/log capabilities for each runtime profile. Do not assume those gates have passed because a reference data type exists.
3. Identify normative generated contracts and supported target/toolchain profiles. Establish native GPU, browser and headless CI environments; record unavailable mobile SDK/device lanes.
4. Work on an isolated feature branch/worktree and preserve user changes. Do not deploy, connect to a production brain or change stored model semantics as part of an unreviewed renderer refactor.
5. Continue unaffected slices when a capability is absent. Record the precise upstream dependency, add capability reporting/tests, and do not claim the gated slice as complete.

## Scope and explicit non-goals

Scope: read-only scene/capture contracts, truthful high-detail surfaces, time-resolved activity presentation, interactive controls, persistent camera/render plans, durable headless export, compatibility and QA. Desktop/web complete interaction first, native/Android job compatibility, explicit iOS capability status.

Non-goals: a new neural engine, changing biological delays, replacing product shells, fabricated morphology, a marketing-only film, unrestricted sensor capture or live actuation. Physical metadata additions require the morphology plan's migration/admission work; avoid widening this plan into a whole-workspace rewrite.

## Repository orientation

Verified canonical locations:

- `src/morphology_contract.rs`: IDs, physical/display DTOs, coverage and paths.
- `src/engine.rs`: display projection and legacy live morphology publisher.
- `src/visualization.rs` and `web_ui/visualization-policy.js`: nine-stage policy.
- `src/ui/anatomy.rs`, `src/ui.rs`: native geometry and planner surfaces.
- `web_ui/app.js`, `web_ui/index.html`, `web_ui/style.css`: browser dashboard, FPV route and jobs.
- `src/fpv_render_jobs.rs`: requests, frame workers, replay and encoding.
- `src/bin/web_ui.rs`: authentication-scoped job and download handlers.
- `apps/android/app/src/main/java/com/neuralmimicry/aarnn/{MainActivity,RemoteAarnnClient,VisualizationPolicy}.kt` and `apps/ios/VisualizationPolicy.swift`: current client boundaries.
- `tools/xtask/src/main.rs`; `qa/scenarios/MORPH-VIS-001.toml`, `MORPH-VIS-002.toml`, `MORPH-VIS-003.toml`; `qa/fixtures/morphology/anatomical-stability.json`; `qa/fixtures/visualization/complexity-policy-v1.json`; existing scripts under `scripts/qa/`.

New module names/paths must follow dependency discovery. Prefer cohesive scene, capture, camera and renderer boundaries rather than adding another large block to `app.js`, `ui.rs` or `fpv_render_jobs.rs`. Do not create a second Rust workspace. Use the existing generated-management contract workflow; discover its source and regeneration command before editing generated files.

## Architecture and safety constraints

Data direction is authoritative runtime → immutable observation/capture publication → derived geometry/timeline → interactive or headless rendering → encoding/artifact storage. A renderer never updates the authoritative runtime. Management commands, capture permissions and job ownership continue through the existing authenticated service.

Frame time, geometry sampling, queue pressure and GPU scheduling are presentation concerns. Biological tags/route epochs and state commits remain authoritative. Display drops may be counted; committed neural events may never be dropped. Complete recording either succeeds within admitted resources or exposes an incomplete/failed interval. No blocking global capture barrier.

Prefer one renderer-neutral Rust scene/camera evaluator with portable bindings, and assess reuse of native wgpu and a browser backend in an ADR. Shared semantics and fixtures are mandatory; identical backend code is not. Retain a pinned reference/fallback renderer. Discover and justify any new dependencies before integration under the repository's existing approval rules.

## Milestones

### M0 — Baseline, capability audit and reference fixtures

Requirements: NVR-001..008,027,036. Run existing applicable baseline lanes. Produce a producer/client capability table, identify missing radii/IDs/route telemetry and committed recording hooks, and add F1–F3 fixtures. Record build profiles, renderer ADR and migration design. Gate: capability denial is truthful; no source semantics changed. Evidence: NVR-QA-001/003 initial contract cases, baseline logs and source audit update. No GPU/actual-runtime completion claim yet.

### M1 — Immutable scene and physical-data publication

Requirements: NVR-004..010,016,020..021,026,037..039. Extend contracts for per-sample radius, attachment, stable identity, coherent capture roots and tile coverage; add index/cursor/delta interfaces. Use reader-first schema upgrades. Resolve physical producer gaps through a separately mapped morphology milestone with authoritative admission; never bypass the existing witness. A fresh default/empty model must commit radius-bearing, clearance-verified anatomy before advertising stages 7–9. Incomplete imports must retain their source and transform into the same growable physical model with deterministic admission/rejection records, then use that model for later executable growth and display. Automatic calibration uses measured inputs only when declared and otherwise records a modelled policy and original coordinates. Add a no-connection stage-9 capability case without inventing contacts. Gate: NVR-QA-002..004 and negative-data cases pass, exact canonical sample preservation, old snapshots readable, plus default creation, incomplete import, checkpoint/restart and post-import growth parity. Until physical producer evidence passes, high-detail remains disabled for the affected source.

Mobile reader slice (Phase 8 interface only, no standalone runtime claim): iOS Swift and Android Kotlin decode the optional contact-set witness and apply the same stage gate as Rust and web. Owning requirements are NVR-001/003/004/037..039; scenarios are NVR-QA-001/021/022. Android compilation/unit tests require Java 21 and the Android SDK; iOS compilation/device validation requires Xcode/iOS SDK and is not available on this Linux host. Rollback ignores the optional field and retains lower-stage viewing; no mobile persisted or biological state changes in this slice.

### M2 — True 3D viewer vertical slice

Requirements: NVR-001..003,009..015,027..029. Implement mesh preparation, correct depth/clipping/materials, 3D controls and ID picking through the current desktop/web shells. Maintain lower modes and old policy. Add scientific and restrained cinematic presets. Gate: NVR-QA-005/013/019 on actual GPU/browser plus F1 turntable and near-field views; capability fallback tested. Do not use a concept image as the scene.

### M3 — Actual activity timeline and live recording

Requirements: NVR-016..021,027..030,035. Add compatible route telemetry and bounded immutable recording chunks, revision-aligned playback, gaps and source age. Implement stop-recording without stopping the brain. Reuse committed cuts/frontiers instead of adding global synchronisation. Gate: NVR-QA-006..008/012, including the controlled actual-runtime source and no-effect replay. A soma-only source gets truthful soma highlighting until route telemetry exists.

### M4 — FPV camera plans and route coverage

Requirements: NVR-022..026,029,036. Implement world/anchored 3D camera tracks, deterministic interpolation/time map, preview/scrub, undo and route preflight. Resolve/pin corridor tiles and activity focus. Retain legacy job evaluator. Gate: NVR-QA-009..011/014 and old-camera fixtures. Record camera and stage tracks so offline workers never derive detail from their instantaneous load.

### M5 — Production export and resumable download

Requirements: NVR-030..036,027..028. Extend existing worker/jobs, capabilities, bounded segment rendering, encoder verification, leases, retries, artifact publication, range downloads and manifests. Gate: NVR-QA-015..018, 120-second and 10-minute exports under budgets. Validate 1080p baseline before enabling optional 4K profiles. Rollback must preserve queued legacy jobs and retained captures.

### M6 — Integrated performance and release evidence

Requirements: all. Run matched neural/view/capture/export benchmarks, long-session recovery and cross-product matrices. Refine effects only within the fidelity/performance gates. Produce NVR-QA-020 using an actual controlled AARNN run, retain scientific/cinematic downloads and final requirement-to-evidence matrix. Gate: no unlabelled missing data, no production safety-gate bypass, complete user journey and explicit not-run platform list.

## Progress

- [~] 2026-10-02 08:29Z — Investigating the default-network native Visual detail mismatch with `message.wav` (PCM 8 kHz mono, 24.02 s). Verified the checkout root with `cargo metadata --no-deps --format-version 1`; current branch is `codex/webots-api-ingress-20260929`. User changes in `Cargo.lock`, `src/runner.rs` and untracked visualisation documents/QA are preserved. Next: correct the stage-5/6 graph projection and the capability explanation, then run focused native evidence.
- [x] 2026-10-02 09:32Z — Restored schematic graph links to the live anatomical snapshot when physical synapse routes have not grown. `cargo test --locked --features ui,morpho,growth3d --lib live_anatomical_view_keeps_weighted_graph_links_before_synapses_grow` passed (1/1). Native label now states requested/effective stage and the physical data limitation; visual frame capture remains pending.
- [~] 2026-10-02 09:32Z — User clarified the completion condition: a new/default or empty model must build sufficient authoritative anatomy for all nine stages at startup. Audit shows `Morphology::Soma` and axon/dendrite segments use normalised coordinates and persist no physical radii; live snapshot has no clearance witness. The point-only importer can reconstruct physical routes, but its result is a separate derived artefact, not the live Runner's route authority. Next: map a versioned physical initialisation and structural-activation path before enabling stages 7–9 for the default run.
- [~] 2026-10-02 09:32Z — User extended the requirement to incomplete imports: they must transform into growable physical anatomy. Added NVR-037/038 to the feature specification and mapped them to M1. Repository evidence now shows `Runner::rebuild_default_topology` uses normalised `[-1,1]` coordinates, while `brain_region_space_scale` heuristically scales region presets and the reconstruction contract labels its coordinates millimetres. Explicit unit mapping and physical-size policy remain required before state-affecting activation.
- [x] 2026-10-02 09:43Z — Drafted `docs/architecture/decisions/ADR-0008-initial-physical-anatomy.md` with the shared new/import physical-build boundary, contact-coverage semantics, versioned checkpoint migration and two physical-parameter choices. It is proposed only; no state-affecting builder has been activated.
- [x] 2026-10-02 09:49Z — Added backward-compatible `DisplayCoverage.contact_set_verified` and aligned Rust, browser, iOS and Android stage resolution. Complete read-only reconstructions report the witness only when every committed synapse marker is present; old snapshots default to false. A verified empty contact set, including a zero-node scene, can resolve stage 9 without invented markers. `cargo xtask qa run --suite staged-visualisation` passed.
- [x] 2026-10-02 09:54Z — Marked bounded snapshots incomplete when source paths, edges or markers are omitted, so a clipped empty projection cannot claim complete contact coverage. The final staged-visualisation suite passed (Node policy, 7 Rust stage tests, 11 FPV tests, live display test); 18 `morphology_contract::tests` passed, including zero-connection and clipped-contact cases. `JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 ANDROID_HOME=/home/pbisaacs/Android/Sdk ./gradlew --offline :app:testDebugUnitTest --quiet` passed (7 Android unit tests). `cargo fmt --all --check` and `git diff --check` passed before this documentation update. iOS/Xcode validation remains unavailable here.
- [x] 2026-10-02 — User selected automatic heuristic physical calibration for defaults and incomplete imports. ADR-0008 now records a versioned, explicitly modelled local-spacing estimator using the existing reconstruction size prior; the fixed 55 mm/unit suggestion is withdrawn. This resolves the numerical-policy question, not the structural activation or persistence gate.
- [x] 2026-10-02 10:20Z — Added `HeuristicPhysicalCalibration` to the existing read-only reconstruction contract. A stable coordinate-hash sample of at most 512 source positions estimates a modelled source-to-mm scale from the existing radius prior; the importer retains original positions, unit absence, policy version, seed, size assumptions and admitted/rejected routes. Persisted records are validated; a tampered record is discarded and reconstructed from preserved topology. `cargo test --locked --features ui,engine_runtime,morpho,growth3d --lib morphology_contract::tests:: --quiet` passed (19 tests); the shipped `config.json` 3×70 topology calibration and import/reimport/tamper tests passed individually. This is derived reference data, not the live route owner.
- [x] 2026-10-02 10:22Z — Registered `MORPH-CAL-001` and `cargo xtask qa run --suite morphology-physical-calibration` for the bounded reference policy; the suite passed all 3 calibration tests. This scenario deliberately does not satisfy NVR-QA-021/022 live activation.
- [x] 2026-10-02 — Validated a persisted heuristic reconstruction against the imported soma positions as well as its own calibration record. Changed positions or a tampered scale now cause derived anatomy to be rebuilt from current source; `cargo xtask qa run --suite morphology-physical-calibration` passed 3/3 after the stale-source case was added. Existing uncalibrated legacy reconstructions remain readable until their reader-first migration. Persisted graph-link parity after weight topology changes remains an open migration check.
- [!] 2026-10-02 10:20Z — Live default and growable-import stage-7..9 activation remains gated by the morphology plan's uncompleted structural commit and checkpoint migration. The Runner's `Morphology` still stores normalised positions and mutable branches without physical radii or a swept-volume admission witness. Do not publish the read-only reconstruction as its live anatomy. Next slice: versioned physical fields on the owning Morphology, exact source/checkpoint migration and committed route/growth admission parity, then default WAV/native/browser acceptance.
- [x] 2026-10-02 09:32Z — `cargo xtask qa run --suite staged-visualisation` passed: Node policy fixture, 5 Rust policy tests, 11 FPV policy tests and the live morphology display test. `cargo fmt --all --check` and `git diff --check` passed after formatting the focused engine change. Native `ui_screenshot` build passed with `--locked --features ui_screenshot,morpho,growth3d`; `AARNN_AUDIO_FILE=/home/pbisaacs/Downloads/message.wav` logged 8 kHz/192160 samples, 8 EQ bands and 64 sensory neurons. Stage-9 request captured a stage-6 schematic graph; stage-1 capture first revealed an incorrect anatomical membrane, then `/tmp/aarnn-visual-detail-stage1-fixed.png` confirmed that the synthetic columns no longer draw it. `/tmp/aarnn-visual-detail-after.png` retains the graph-link capture. The physical-stage gate is still open.
- [x] 2026-09-30 — Read-only baseline source reviewed at the pinned commit; audit retained in `docs/reviews/visualisation-source-audit-2026-09-30.md`. This initial hand-off uses date-only discovery records; implementation entries must add actual UTC times under `.agent/PLANS.md`.
- [x] 2026-09-30 — Direct Node policy and anatomical fixture checks passed; this is not the full xtask/native/browser/device matrix.
- [x] 2026-09-30 — Requirements, proposed QA and hand-off prepared. No implementation or new-feature acceptance claimed.
- [ ] M0 — Refresh baseline and establish capability/dependency evidence.
- [ ] M1 — Contract/publication and morphology dependency gates.
- [ ] M2 — Desktop/web 3D viewer and GPU evidence.
- [ ] M3 — Live capture/activity and non-interference evidence.
- [ ] M4 — Camera tracks, preflight and preview parity.
- [ ] M5 — Durable exports, recovery and verified downloads.
- [ ] M6 — Actual-runtime integrated demonstration and final traceability.

### Progress update — 2026-10-10 14:14Z

- [x] The supplied FPV screenshot's `Projection request failed (503)` is from
  `loadFpvProjection()`'s authenticated workspace snapshot GET; the browser
  reports render-job POST failures with different wording. Ingress evidence
  for the incident had no ready web-ui upstream and no matching FPV jobs POST.
- [x] Read-only Spirit/Kubernetes measurements at 14:13Z found the single
  saved `system/neuralmimicry-shared-snn` snapshot is 271,858,802 bytes. The
  web-ui process had 9,933,120 kB RSS; its cgroup used 12,211,912,704 bytes of
  12,884,901,888 (12 GiB) and had ten restarts. It was not OOM-killed during
  this measurement. These observations show little container headroom, not a
  controlled peak-memory benchmark.
- [x] Source trace at `src/bin/web_ui.rs::runtime_workspace_snapshot` and
  `src/runtime.rs::workspace_saved_snapshot` found the display request loads
  or retains the full workspace engine, reads the full persisted JSON, builds
  a temporary imported engine for display snapshots, then imports the same
  JSON again in `display_projection_for_snapshot_json`. The browser FPV caller
  consumes only `display_snapshots`; this duplicate work is avoidable.
- [x] Implement a read-only display-projection path that imports the persisted
  snapshot once outside the live engine mutex, returns bounded display DTOs and
  dashboard metadata, and admits one projection import per web-ui process.
  Preserve the full snapshot path and route-level owner resolution. Formatting
  and runner tests are still pending; this is not production-validated.
- [ ] Resolve a request-scoped memory/load budget from measurements, keep the
  saved workspace intact, and validate authenticated projection, an actual
  FPV job POST, worker completion and result retrieval. Current pod readiness
  is not application or end-to-end acceptance evidence.

### Review update — 2026-10-10 14:22Z

- [x] Rejected the first implementation approach after checking NVR-027: it
  ran geometry generation under the live `WorkspaceHandle.engine` mutex. The
  current implementation instead uses one isolated import of the persisted
  snapshot and a one-slot projection semaphore. It still has a temporary
  memory peak from that import; the existing runner and deployment measurement
  must establish whether this bounded single-request path fits the cgroup.
- [ ] Run the focused runtime-manager regression and exact-head CI on the
  existing runner fleet; then measure projection lock impact and cgroup peak.
- [ ] Deploy through the existing Ansible path and validate the authenticated
  browser, a real FPV POST, worker completion and result download.

### Continuum-first resource observation — 2026-10-10 15:18Z

- [x] Through `ssh pbisaacs@192.168.1.2`, `nmc 0.0.0197` reported the
  Continuum server healthy and its AARNN runtime-status, workspace-list and
  endpoint-discovery operations returned HTTP 200. The shared workspace was
  running at step 3,722,473 with 2,101 neurons across five reported nodes;
  web-ui, control API and orchestrator each reported one ready replica. This
  is point-in-time API evidence only, not an FPV browser or render test.
- [x] The deployed Continuum runtime response currently reports
  `local_memory_usage_pct: null`, and `nmc 0.0.0197` has no structured
  workload-resource/cgroup query. This leaves the requested live cgroup
  measurement unavailable through the current Continuum surface. Preserve the
  Continuum-first operations rule: add a bounded resource-observation function
  to Continuum rather than using direct Kubernetes/package CLI reads.
- [x] Source review confirms the current FPV branch bounds returned node/edge
  counts and admits one projection at a time, but still reads the complete
  saved snapshot and imports it into a temporary `RunnerEngine`. The output
  cap therefore does not bound the temporary engine's memory. The branch has
  not passed exact-head CI, been measured or been deployed.
- [ ] Wait for AARNN PR #36 run `38059605193` (ARM64 passed; X64 still building
  release binaries) to finish before rebasing the FPV branch. Its contract run
  `38059605195` passed. Then profile the projection with safe headroom and
  provide the Continuum resource observation needed to measure the actual
  container high-water mark.
- [ ] Complete authenticated browser projection, actual `POST /api/fpv/jobs`,
  worker completion and rendered-result retrieval. Do not call the incident a
  failed render submission unless a browser click correlates to that POST.
- [~] 2026-10-10 15:27Z — Add an authenticated, read-only process/cgroup resource
  sample in AARNN, then a typed `nmc aarnn runtime resources` operation in
  Continuum. Expose current/high-water/limit, process RSS and OOM counters;
  use it to measure before and after projection without direct Kubernetes or
  package-CLI access. The AARNN implementation is in progress; the Continuum
  source change waits for the current exact-head NMC PR checks.

### Runner regression — 2026-10-10 19:06Z

- [x] Exact-head X64 check in AARNN PR #37 run `38076487416` reached the lint
  step and failed while compiling `web_ui`; the job log reports two E0433
  unresolved imports at `src/bin/web_ui.rs:4924` and `:4926`. Both refer to
  `crate::morphology_contract::DisplayMode`, but `web_ui` is a binary crate
  and already imports `DisplayMode` from `aarnn_rust::morphology_contract`.
  This is a source compile failure, not evidence that FPV logic tests failed.
- [x] The same log confirms the failure is not the repository's 1,347
  pre-existing Clippy warnings; compilation terminates on two unresolved
  imports before subsequent verification can proceed. ARM64 later failed at
  `Check build` with the same two E0433 errors; neither architecture reached
  FPV tests or release builds.
- [x] Corrected both references to the existing imported `DisplayMode` in
  `src/bin/web_ui.rs`. `cargo fmt --all --check` and `git diff --check` pass;
  this correction changes no projection or neural-runtime behaviour.
- [ ] Commit and push the correction, then wait for refreshed exact-head X64
  and ARM64 runner checks. Do not merge or deploy this FPV fix until all
  required checks pass.

## Validation and acceptance

Use the companion acceptance matrix. Existing confirmed entry points include:

```sh
cargo xtask qa run --suite anatomical-render
cargo xtask qa run --suite anatomical-render-browser
cargo xtask qa run --suite anatomical-growth
cargo xtask qa run --suite staged-visualisation
cargo xtask qa run --suite morphology-growth-cone
```

Choose relevant lanes and establish prerequisites; a new dependency/target may require different discovered feature profiles. Do not run an indiscriminate all-features build. Add each new lane to xtask and scenario manifests before documenting it as executable. Record exact commands, not proposed commands reported as passed. Use `cargo fmt --all --check` and applicable lint/test/doc/generated-contract checks at green milestones.

Configure shareable RustRover run configurations under `.run/` that invoke the exact discovered/added xtask commands: baseline visualisation, high-detail fixture, live capture, FPV recovery and performance. Do not place local absolute paths, tokens or signing identities in them. The root Cargo workspace stays the IDE project. CLI/CI must reproduce every IDE verification step.

## Rollout, compatibility and rollback

Deploy compatible readers first, then new optional display/capture publication, then viewer/planner flags, then capable workers and export profiles. Keep legacy schema/policy fixtures and the old renderer/evaluator until the stated migration window closes. New render/capture settings never mutate neural state. Disable the new view to roll back UI behaviour; continue retrieving already published artifacts. Preserve immutable job/capture versions. If physical-model persistence changes, the morphology plan owns forward migration, checkpoint compatibility and the point of no return.

## Risks and mitigations

- Missing physical metadata or uncompleted structural-activation gates: capability reporting and dependency evidence; no fake radii/witness.
- Visually attractive but false activity: typed route/time provenance; soma-only fallback and explicit gaps.
- Incoherent tiles or ID reuse: revision-pinned manifests, stable IDs and conflicting-duplicate tests.
- GPU contention affecting neural work: admission, bounded worker budgets, measured non-interference and resource separation when required.
- GPU pixel variance: exact scene/timing oracle plus reference backend and bounded perceptual tolerances.
- Long-export disk growth: streaming segments, scratch/retention quotas, resume and fault injection.
- Monolithic code growth: separate contracts/evaluators/adapters with small vertical changes.

## Surprises & Discoveries

- 2026-10-02: The screenshot requests stage 9 but resolves stage 6. `config.json` starts with three hidden layers and no sensory/output neurons; the audio startup path provisions 64 sensory neurons. Legacy live morphology publishes no soma or neurite radii and no clearance witness, so stage 7–9 denial is required by VIS-12/NVR-003. The native UI currently explains that denial only for cluster views.
- 2026-10-02: The live morphology display snapshot draws stage-5/6 links solely from `morphology.synapses`; its graph matrix may already contain live weights while that list is empty. The renderer suppresses cached matrix overlays whenever an anatomical contract is present. Thus the stage resolver can advertise stage 6 from cached graph edges while drawing no connections. Region labels are projected from configured centres in a separate legacy frame and can overlap at the top left of an anatomical frame.
- 2026-10-02: The existing `reconstruct_point_only_import` path translates the runner's matrix/point topology into a finite, radius-bearing reconstruction with explicit rejected connections. It runs only on snapshot import; displaying that artefact as live authoritative anatomy would give a false claim about the Runner's electrical routes and later growth. The initialisation path instead builds legacy `Morphology::from_weights`, which has no radius fields or verified swept-volume clearance.
- 2026-10-02: Default topology construction is in `src/runner.rs::rebuild_default_topology`; import transformation is `Runner::reconstruct_point_only_import`; live stage publication is `src/engine.rs::live_morphology_display_snapshot`; rendering and capability resolution are `src/ui.rs` and `src/visualization.rs`. `config.json` has 3x70 hidden neurons, zero initial sensory/output neurons and `use_morphology=true`; `AARNN_AUDIO_FILE` provisions 64 sensory neurons and autoplays. The current reconstruction defaults to 0.04 mm soma and 0.006 mm neurite radii, but it receives normalised topology coordinates without an explicit physical scale. Those numbers cannot be silently attached to the live model.
- Existing jobs are already durable and owner-scoped; this work extends them.
- Legacy live physical radii are missing, so the high-stage gate is correctly restrictive.
- 2026-10-02: Marker count alone did not distinguish an actually empty contact set from omitted contact data. An optional false-by-default coverage witness now carries that distinction. The legacy Runner never sets the witness or the physical-clearance flag, so this reader change cannot unlock its unsupported stages.
- 2026-10-02: JSON export/reimport of an existing `f64` reconstruction coordinate differed by one ULP in the focused import test. The reference calibration preserves original topology as `f32` and validates restored modelled geometry within a tight declared tolerance; it does not claim bit-identical persisted route geometry or deterministic-reference replay from this JSON path. Exact checkpoint encoding belongs to the authoritative physical migration gate.
- `DisplayPath` loses per-sample radius richness present in `PhysicalPath`.
- Browser route merging needs explicit revision agreement and corridor coverage.
- Current export activity is normally a submitted active-ID sample; isolated replay is not a captured time-varying geometry stream.
- Current 8 GiB RGB-intermediate estimate limits 1080p30 to roughly 46 seconds.
- 2026-10-10: The workspace FPV display route layered a resident workspace
  engine, a saved 271.8 MB JSON string and repeated temporary engine imports.
  This is a source-confirmed extra-work path and a plausible contributor to
  the observed OOM, but only the unavailable-upstream/OOM sequence is directly
  confirmed by production logs; per-allocation peak attribution still needs a
  controlled measurement. The browser display caller does not read the
  returned `snapshot_json`. A direct projection from the resident engine was
  rejected because it performs geometry work under the engine mutex (NVR-027).
- 2026-10-10: The deployed Continuum AARNN runtime status returns HTTP 200 and
  workspace counters, but its local memory percentage is `null`; the installed
  client offers no workload cgroup resource observation. The bounded FPV
  projection still constructs a temporary engine from the whole saved JSON,
  so its node/edge output cap cannot be treated as a memory bound.
- 2026-10-10: The refreshed PR #37 X64 lint job failed because two workspace
  display-mode references in the `web_ui` binary use the library's `crate::`
  namespace instead of its existing `aarnn_rust` import. The exact diagnostic
  is E0433 at `src/bin/web_ui.rs:4924` and `:4926`; the unrelated Clippy
  warnings are not the cause. Correcting the import path does not change
  projection or neural-runtime semantics.

Update these as the current checkout differs. Distinguish source observation, measured behaviour and inference.

## Decision Log

- D05, 2026-10-02: Keep physical stages gated until the morphology owner publishes measured or modelled radii and a verified clearance witness. For stages 5/6, project existing weighted graph links into the same anatomical display contract, retaining their schematic identity and leaving biological state unchanged. Show the precise capability reason on standalone views. Authority: VIS-12 and NVR-001/003/004; rollback is limited to read-only display publication and UI labels.
- D06, 2026-10-02: Treat the user's default/empty-model all-stage requirement as the release target for new local model creation, not as permission to set a display-only clearance flag or reuse importer geometry as live anatomy. The initial physical build must share ownership, revisions and route activation with execution, carry explicit units/radii/provenance, and verify clearance before publication. The existing stage-6 fallback remains a compatibility path for older checkpoints and imported networks lacking that evidence.
- D07, 2026-10-02: Incomplete imports take the same growable physical-build path as new models; their original source and rejected graph links remain inspectable. A read-only reconstruction is preparatory evidence only. Before activating transformed geometry, define an explicit normalised-to-physical coordinate mapping and modelled radius policy in the morphology authority, with checkpoint migration, collision admission and route-timing parity. The user has specified the required outcome but these physical parameters have no normative value in the current live model.
- D08, 2026-10-02: Add a false-by-default contact-set witness to the display coverage contract. Stage 9 requires a complete view, verified 3D clearance, physical radii for all present structures and this witness; zero contacts are a valid result. The field is additive and older snapshots remain readable but cannot claim verified empty contact coverage. This is read-only presentation policy; it does not activate physical morphology or change neural state.
- D09, 2026-10-02: User authorised automatic heuristic physical calibration. Use a versioned modelled size prior with deterministic local source-spacing estimation where units are missing; retain source coordinates and provenance, validate the persisted record and let occupancy admission reject unsafe geometry. Do not call the result an empirical measurement. A declared valid physical measurement takes precedence in future import contracts. The current reference import may use this policy without changing neural matrices or live morphology; state-affecting cutover still requires morphology ownership, timing and durability evidence.
- D01, 2026-09-30: Extend existing stage policy, morphology DTOs and FPV service. Evidence: source audit. Consequence: backwards compatibility is a release gate.
- D02, 2026-09-30: Prioritise network truth over decorative biological detail. Authority: existing morphology VIS-09..12 plus this user's actual-live-network request. Missing geometry remains unavailable until authoritative publication supports it.
- D03, 2026-09-30: Separate frozen scene, recorded-live capture and isolated replay. Consequence: metadata, UI and acceptance identify the time source explicitly.
- D04, 2026-09-30: Require semantic parity, not cross-GPU byte-identical pixels. Consequence: canonical scene/camera/time oracles and pinned reference outputs.
- Pending: backend/dependency ADR, final schema versions, actual hardware profiles and budget measurements. Resolve from the checkout and bounded prototypes; do not invent a measurement.
- D10, 2026-10-10: Candidate approach—derive the bounded display result from
  the resident `WorkspaceHandle` under its engine lock. Rejected at 14:22Z
  after review because geometry generation under that lock violates NVR-027.
- D11, 2026-10-10: Use one bounded, isolated import of the persisted snapshot
  for owner-authorised workspace `projection=display` requests. Admit one
  import at a time per process; do not hold the live engine lock while
  decoding or generating geometry. Keep owner resolution and the full snapshot
  API unchanged, cap output, and return a retryable 503 when the projection
  slot is busy. Evidence: the prior route imported the same 271.8 MB snapshot
  twice sequentially while the browser used only the bounded DTOs. This avoids
  duplicate route work but keeps one temporary Engine peak; the measured
  cgroup gate remains open. Roll back by restoring the previous handler; no
  persisted format or workspace data changes.
- D12, 2026-10-10: Use Continuum as the first source for production resource
  observations. The deployed AARNN status query succeeds but returns no
  cgroup/process sample, and the installed CLI has no workload-resource
  operation. Add a bounded, authenticated Continuum resource observation
  before measuring production cgroup headroom; do not use direct Kubernetes or
  package CLI reads as a substitute. This decision does not authorise a
  rollout or a guessed memory limit.
- D13, 2026-10-10: Implement a read-only authenticated AARNN resource endpoint
  for cgroup and process memory, then expose it through a typed Continuum
  runtime-resources function. Prefer kernel cgroup current/high-water/limit
  and OOM counters over host-wide percentage estimates; retain `null` for
  unsupported counters instead of presenting host memory as container memory.
  This supports measurement only and does not weaken projection memory gates.

## Outcomes & Retrospective

The first native consistency repair is implemented and validated: schematic stage-5/6 graph links remain visible before physical synapses grow; stage 1 no longer draws a legacy anatomical membrane; requested/effective stage and the physical capability reason are explicit. The display contract can distinguish a verified empty contact set from missing data across Rust, browser and mobile readers. A versioned heuristic conversion now records modelled scale, source positions and provenance for derived point-only imports, and the shipped default topology passes the same estimator. The default/import all-stage requirement NVR-037..039 is not complete: the live Runner still lacks authoritative physical radii, unit mapping and a clearance witness, and the importer reconstruction is not its growable route authority. GPU/browser/device validation, physical-build migration, actual high-stage native captures and performance gates remain open. Do not mark M0–M6 complete from this reference work.

The 2026-10-10 FPV incident diagnosis distinguishes projection loading from job
submission: the observed 503 occurred on the projection GET while the pod had
no ready upstream, and no FPV POST was retained in the outage sample. The
current display endpoint repeated large workspace materialisation despite the
browser using only its bounded display DTOs. A single-import projection path
is implemented but is not yet runner-tested, measured or deployed;
authenticated render submission, returned diagnostics and measured peak
memory remain unverified. A Continuum-first status recheck on Spirit is healthy
at the API level, but current Continuum response fields do not measure AARNN
cgroup high-water; a structured resource operation remains required.

The latest FPV exact-head run supplies a concrete repair: both X64 and ARM64
fail to compile the `web_ui` binary on two `DisplayMode` import paths. The
source correction is prepared and formatted; refreshed exact-head CI remains
pending. No FPV job POST, result retrieval, deployed memory measurement or
end-to-end acceptance has been completed in this session.
