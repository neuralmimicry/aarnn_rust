# Repository source audit — 30 September 2026

Read-only review of https://github.com/neuralmimicry/aarnn_rust at commit `0c561abe210238e3dfabeb81a667f541d7d0b38f`, commit subject `test(distributed): initialize network load fingerprints`. HEAD was obtained by `git ls-remote` and a depth-one clone. `Cargo.toml` declares package 0.1.37. This report describes inspected source, not a deployed-system audit or full runtime certification. Recheck HEAD and local uncommitted changes in RustRover before implementing.

## Findings and requirement mapping

| Finding | Verified source / symbol | Consequence |
|---|---|---|
| Existing instructions require a living ExecPlan, one authoritative Rust semantic implementation and read-only display boundaries. | `AGENTS.md`; `.agent/PLANS.md`; morphology specification | Add a feature plan; do not replace repository instructions or bypass phase gates. |
| Nine shared stages and version-1 Auto policy already exist. | `src/visualization.rs`: `VisualizationStage`, `highest_supported_stage`, `automatic_target`, `update_auto_stage` | NVR-001/003/025/029 preserve stage and compatibility semantics. |
| Stages 7–9 require a producer clearance witness and positive soma/path radii. | `highest_supported_stage`; `web_ui/visualization-policy.js` | Do not force high detail for incomplete legacy data. |
| The normative morphology document already prohibits decorative neurites and distinguishes procedural from observed anatomy. | `docs/specifications/morphology-and-responsive-simulation.md`, VIS-09..12 | The cinematic reference changes rendering quality, not biological authority. |
| Display snapshot schema is version 2; ID values are decimal strings; DTOs carry geometry/topology/route revisions and coverage. | `src/morphology_contract.rs`: `DisplaySnapshot`, `AnatomicalId`, `DisplayCoverage` | Extend these contracts, preserving old readers/import paths. |
| Physical paths have per-sample radii, but display paths have one scalar radius. | `PhysicalPath`, `PathSample`, `DisplayPath` | NVR-005 preserves taper and attachments at the publication boundary. |
| Display contact markers have position/kind/owner/synapse ID but no physical shape dimensions. | `DisplayMarker` | NVR-010 distinguishes glyphs from physically dimensioned contacts. |
| Legacy live morphology emits `soma_radius_mm: None`, `radius_mm: 0.0`, and derived per-export path counter IDs. | `src/engine.rs`: `live_morphology_display_snapshot` | NVR-004/006 address source capability and identity; a renderer cannot supply missing physical evidence. |
| Display projection caps are 4,096 nodes and 32,768 edges per request; defaults are 512 and 4,096. | `RunnerEngine::display_snapshot_for_runner_in_region` | Preserve bounded queries; introduce tested spatial indexing and explicit coverage. |
| Regional filtering exists; current FPV plan identifies full source-array scanning as a scale limit. | `src/engine.rs`; `docs/execplans/fpv-render-jobs.md` | NVR-026 requires indexed intersection queries without billion-neuron claims. |
| Main web network and FPV-map canvases use `getContext("2d")`. | `web_ui/app.js`: `drawNetwork`, `drawTubePolygon`, `drawFpvMap` | These are projected 2D surfaces, not the desired true 3D neural render path. |
| Native anatomy builds egui screen meshes and projected tube polygons. | `src/ui/anatomy.rs`: `Frame`, `tube`, `project` | Existing GPU-backed UI hosting does not imply volumetric/depth-tested neural surfaces. |
| FPV requests embed one scene, neuron-ID waypoints, active IDs and stage/zoom keyframes. | `src/fpv_render_jobs.rs`: `FpvRenderRequest`, `FpvVisualizationKeyframe` | Need independent 3D camera track and recorded live capture contract. |
| FPV frame rendering uses a software RGB buffer, projected strokes, depth-sorted soma discs and fixed-size contact discs. | `render_frame_ppm`, `draw_line`, `draw_disc` | NVR-009/011 supply shared true-3D scene evaluation; retain fallback honestly. |
| Camera target follows linear interpolation; eye distance depends on scene extent and zoom. | `route_target`, `render_frame_ppm` | Current route is not a general SE(3) camera spline or independently controlled FPV camera. |
| Web route capture samples at most 16 waypoint centres, four requests at a time, then merges by ID with overview metadata. | `web_ui/app.js`: `captureFpvRouteTiles` | Inspected merge does not compare tile revisions or cover the entire swept camera corridor. NVR-007/008/026. |
| Merged nodes cap at 65,536; path/marker ownership is filtered through visible somas. | `captureFpvRouteTiles` | A nearby path with a distant owner can disappear. Aggregated omission/coverage must be explicit. |
| Owner-scoped durable jobs, frame leases, retries, cancellation and MP4 streaming already exist. | `src/fpv_render_jobs.rs`; `src/bin/web_ui.rs` FPV handlers | Extend rather than replace. Do not describe this work as introducing jobs from scratch. |
| The browser submits scene JSON; the shown job handler passes the typed request to `submit_job`. | `submitFpvJob`; `fpv_submit_job` | Server-rooted capture provenance must be distinguishable from client-supplied imported scenes. This is a trust-boundary requirement, not a claim of a verified exploit. |
| Isolated replay imports a separate engine snapshot, steps through timestamped sensory input and extracts active IDs per frame, capped at 512. | `run_isolated_replay`, `FPV_REPLAY_ACTIVE_IDS_PER_FRAME` | This does not demonstrate a complete committed route-event/morphology recording or superdense replay parity. NVR-016..021/035. |
| Frame resource limits: 9,000 frames; 65,536 nodes/paths; 131,072 edges; 2M geometry points; 8 GiB scratch. Dimensions up to 3840×2160, rate 1–60 fps. | FPV constants and `FpvRenderRequest::validate` | Resolution/rate validation alone is not proof a long 4K job fits. At 1080p30 uncompressed RGB reaches the scratch cap after approximately 46 s. |
| Downloads stream chunks with `video/mp4`, content length and attachment disposition; inspected handler returns 200. | `fpv_download_video` | Preserve attachment; add tested range/resume behaviour and verified artifact publication. |
| Existing native/Android FPV integration shares job API; Android says route-specific spatial tile rendering is available in web FPV Studio. | `src/ui.rs`; Android `MainActivity.kt`, `RemoteAarnnClient.kt` | Native/mobile parity must be measured, not assumed from shared policy labels. |
| Existing FPV plan explicitly states no iOS FPV planner and leaves worker-restart/duplicate-claim/damaged-frame/large-transfer tests open. | `docs/execplans/fpv-render-jobs.md` | Retain these unfinished gates and report platform scope accurately. |
| Native/web/Android/iOS stage labels already share fixture checks; generated management client and xtask exist. | `qa/fixtures/visualization/complexity-policy-v1.json`; `tools/xtask/src/main.rs`; `web_ui/management-client.generated.js` | Extend authoritative generation and QA routes; don't hand-maintain divergent protocols. |

## Verification performed in this review

Working directory: the isolated repository clone. Both checks passed:

```sh
node scripts/qa/test_visualization_policy.cjs
# visualisation policy fixture and stage safety checks passed
node scripts/qa/test_anatomical_render.cjs
# PASS: bounded snapshots, path/soma projection, mode switching and synthetic connectivity
```

These are Node/fixture checks. No complete Cargo build, real-browser capture, native GPU test, mobile build, distributed live capture or performance benchmark was run for this requirements task. Passing these tests is baseline evidence only; no new feature implementation is claimed. Repository production code was not changed.

## Existing canonical QA entry points found in xtask

```sh
cargo xtask qa run --suite anatomical-render
cargo xtask qa run --suite anatomical-render-browser
cargo xtask qa run --suite anatomical-growth
cargo xtask qa run --suite staged-visualisation
cargo xtask qa run --suite morphology-growth-cone
```

`staged-visualisation` currently invokes the policy fixture and Rust tests with `--locked --features ui,engine_runtime,morpho,growth3d`, including `visualization::tests`, `fpv_render_jobs::tests` and a live-morphology display test. The implementation agent must discover prerequisites and execute appropriate product lanes. Do not treat unrun GPU/mobile work as passed.

## Pinned source links

- [Repository baseline](https://github.com/neuralmimicry/aarnn_rust/tree/0c561abe210238e3dfabeb81a667f541d7d0b38f)
- [Stage policy](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/src/visualization.rs)
- [Display/morphology contracts](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/src/morphology_contract.rs)
- [Live geometry producer](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/src/engine.rs)
- [FPV worker and request](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/src/fpv_render_jobs.rs)
- [Browser renderer and planner](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/web_ui/app.js)
- [Native anatomy renderer](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/src/ui/anatomy.rs)
- [Job API and downloads](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/src/bin/web_ui.rs)
- [Existing morphology authority](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/docs/specifications/morphology-and-responsive-simulation.md)
- [Existing FPV work plan](https://github.com/neuralmimicry/aarnn_rust/blob/0c561abe210238e3dfabeb81a667f541d7d0b38f/docs/execplans/fpv-render-jobs.md)

The reference film was used only for visual ambition. No third-party video's anatomy or unsupported biological claims become AARNN requirements.
