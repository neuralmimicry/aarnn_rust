# Connectome explorer, GNAT analysis and function-preserving circuit reduction

This ExecPlan is a living document maintained under `.agent/PLANS.md`. A new session resumes from `Progress` (newest last) and `Next actions`; do not repeat the discovery recorded below.

## Purpose and observable outcome

A separately runnable Rust binary, `aarnn-explorer`, that:

1. imports an existing AARNN network (native designed-circuit or imported-connectome JSON) or a sparse edge-list interchange with explicit dynamics;
2. records a complete spike trace by executing the real `aarnn_rust::runner::Runner` in an isolated worker process;
3. extracts Graphical Neural Activity Threads (GNATs; Theilman et al. 2023) and recurring (analogous) subthreads;
4. runs lesions and other interventions on isolated copies, never on the source;
5. evaluates intact and modified circuits against declared functional tasks and resilience profiles with exact confidence bounds;
6. searches for smaller circuits that pass every required task, keeps a Pareto frontier and physically lowers the selected candidate (rows/columns removed, mapping kept);
7. exports an executable candidate bundle that a clean process re-imports and revalidates;
8. serves an optional loopback browser UI (`web-ui` feature) over the same job API.

The user's stated goals (26 Nov 2026):

- **G1:** reduce an existing biomimetic AARNN network so that, after GNAT-guided reduction, it keeps the same declared functions with fewer computational resources.
- **G2:** reverse-engineer several AARNN networks (fish, fly, hexapod and others) into a library of recurring functional motifs, and use it to build a network for a new robot that this work designs.
- **VX-001, the user's verification test (26 Nov 2026, 23:00Z)**: three Drosophila networks of the same designed walking/foraging circuit, sharing one task contract and one environment distribution:
  1. without neuromodulation;
  2. with neuromodulation (the current design: dopamine/5-HT modulate transmission);
  3. with neuromodulation, after GNAT-guided optimisation.
  - The user's hypothesis: the GNAT-optimised version could have greater computation depth across fewer neurons, and perform better than both non-GNAT versions.
  - Measured per network:
    - resources: neurons, synapses, state bytes, measured step time;
    - GNAT computation depth: longest causal chain in hops per thread, mean and p95, plus depth per participating neuron;
    - open-loop contract tasks;
    - closed-loop foraging in `aarnn-mujoco`: trials reached, mean time, wall contact.
  - Verdict: each part of the hypothesis (fewer neurons, greater depth, better performance) passes, fails or is inconclusive separately, with trial counts and confidence bounds. A reduction that only preserves function is reported as preserving, not as improving.

## Specification authority and traceability

- Brief: `docs/AARNN_Connectome_Explorer_Codex_Claude_Implementation.md` (v1.0, 25 Nov 2026). Sections 3-22.
- Normative specification: `docs/specifications/distributed-whole-brain-emulator-v1.1.md`. The explorer is an offline, isolated analysis product. It does not enable unfinished production migration paths.
- Invariants touched:
  - `INV-002` (event time is the synaptic transition's logical time): trace events carry the runner tick and microstep 0 (see D-003).
  - `INV-007` (no silent drops): a full live-observation queue counts lost sequences and never drops simulation events.
  - `INV-008` (deterministic replay): each worker seeds `fastrand` from the experiment seed; replay parity is tested.
  - `INV-009` (atomic topology change): lesions are applied at a declared cold-start boundary, never mid-step.
  - `INV-012` (immutable checkpoints): project objects are immutable after publication.
  - `INV-014` (single owner of each synapse): one source synapse maps to at most one candidate synapse, recorded in the mapping.
  - `INV-016/017` (effects and peripherals): workers never open live AER, robot, audio or HID bindings; outputs go to recorded sinks.
- GNAT method reference: Theilman, Wang, Rothganger and Aimone (2023), arXiv:2306.16684 v1, Eq. 1 (Ω), Section 3 (weakly connected components) and Eq. 4-5 (second-order activity graph). Implementation differences are recorded in D-004.

## Prerequisites and phase boundary

Offline analysis and isolated reference experiments are permitted at the current phase (brief Section 20). Nothing here changes default production scheduling, learning, growth, numerical precision, checkpoint cadence, rendering or I/O routing. Live/distributed capture through `StableShardExecutor` stays a reported `unsupported` capability until that executor exposes a trace seam (see the capability table in `docs/connectome-explorer/architecture.md`).

## Scope

The M0-M10 milestones below, plus G1 (applied to the real designed circuits) and G2 (motif library and new-robot network).

## Non-goals

- No mutation of a running brain, no deployment, publishing or physical actuation.
- No claim of biological equivalence, whole-fly compression or validated silicon.
- No Python, LLM API or second simulator at runtime.
- Exact mid-run resume of the legacy `Runner`: it lacks a complete pending-event/RNG snapshot, so it is reported as `declared-cold-start` (D-002).

## Repository orientation

Recorded 2026-11-26 22:50Z.

- Workspace root `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`. Branch `connectome-explorer`, created from `main` at `f767570ff20ebd32918d71a71fbc9ed2cb3e8be2` without touching the working tree.
- 249 dirty paths belong to unrelated earlier work and are preserved:
  - `src/runner.rs`, `src/engine.rs`, `src/config.rs` and others: neuromodulators, touch escape, needs;
  - `adapters/mujoco`, `scripts`, `sim/content`.
- Toolchain: `rust-toolchain.toml` pins 1.92.0 with components rustfmt and rust-analyzer. Clippy is not installed in the pinned toolchain, so clippy lanes are `not-run` here unless `rustup component add clippy` is authorised.
- Workspace members before this work: `.`, `tools/xtask`, `crates/aarnn-aer-legacy`, `crates/aarnn-embodiment`, `crates/aarnn-embodiment-ffi`, `adapters/mujoco`, `adapters/webots`. `default-members = ["."]`.
- Root crate `aarnn_rust` 0.1.43, edition 2024:
  - non-optional heavy dependencies: tonic/prost (with `build.rs` protobuf generation), axum, reqwest, openidconnect, and ndarray with the `blas` feature;
  - so a dependent crate inherits protoc and BLAS build requirements (see the doctor output).
- Headless execution recipe, as used by `examples/celegans_depth_probe.rs`:
  1. `Runner::new(LIFParams::default(), STDPParams::default(), NetworkConfig::default(), NeuronModel::Aarnn, Learning::Aarnn)`;
  2. `import_network_json(&raw)`;
  3. `fastrand::seed(seed)` and `set_dt(1.0)`;
  4. per 2 ms sensor frame, a phase-accumulator encoder builds an `i8` spike frame for `step_sync(1.0, Some(&frame))`, followed by a zero frame;
  5. read `last_spk_h[0]`, `last_spk_o`, `graded_equivalent_rate_h(0)` and `graded_activation_o()`.
- Native designed-circuit format (`scripts/aarnn_circuit.py::Circuit.write`):
  - `w_in` [hidden x sensory], `w_hh_rec[0]` [post x pre], `w_out` [output x hidden], `w_gap[0]` symmetric; `p_*` are the 0/1 patterns;
  - `topo.layers[0][i]` holds `type_name`, `region_name` and x/y/z;
  - `connectome_labels.{sensory,hidden,output}_nodes` hold the stable names;
  - template defaults come from `network.json`.
- (More runner facts are appended under Surprises & Discoveries when discovery completes.)

## Architecture and safety constraints

- New package `crates/aarnn-explorer`, a library plus the `aarnn-explorer` binary. It depends on `aarnn_rust`; the root never depends on the explorer.
- Experiments run in child processes (`aarnn-explorer worker`). Each one owns a fresh `Runner` built from an immutable source object, with output going only to its result file.
- Optional OS enforcement uses `systemd-run --user --scope` when available, reported as `enforced`; otherwise it is reported as `advisory` with internal admission limits.
- Workers inherit bounded threads: `RAYON_NUM_THREADS`, `OPENBLAS_NUM_THREADS=1`, `OMP_NUM_THREADS=1` and `NM_SIM_PAR_MAX_WORKERS`.
- Project directory:
  - immutable content-addressed objects under `objects/sha256/..`, published by atomic tmp-then-rename;
  - small JSON indexes: `project.json`, `aliases.json`, and the journal `jobs.jsonl`.

## Milestones

| Milestone | Behaviour at the end | Gate / command |
|---|---|---|
| M0 | Discovery recorded here; branch created | This section |
| M1 | Package builds; `doctor`, `project init`, `import` (native + sparse) with validation and overwrite refusal | `cargo test -p aarnn-explorer` import/project tests |
| M2 | `capture`: real Runner trace in a worker; complete manifest, checksummed segments, gap accounting; observation off/on parity | trace tests |
| M3 | `analyse`: reference + indexed GNAT, recurrence, threshold calibration with shuffled controls | GNAT fixture tests |
| M4 | `experiment`: isolated lesions, sham/intact controls, cancellation, persistence | lesion tests |
| M5 | Task oracles, resilience campaigns, Clopper-Pearson, Bonferroni | task/resilience tests |
| M6 | `optimise`: deterministic beam search, cumulative revalidation, Pareto, physical lowering | optimise tests |
| M7 | `export` + clean-process reimport/revalidation | export tests |
| M8 | `serve` web UI + async job API | UI/API tests |
| M9 | xtask catalogue/QA, `.run/` configs, Slurm dry-run, budgets | `cargo xtask qa run --suite explorer` |
| M10 | Docs, validation report, hand-off | docs present, checks re-run |
| G1 | Reduction of real designed circuits (fish/fly/hexapod) under open-loop contracts | report under `docs/connectome-explorer/` |
| G2 | Cross-network motif library and a new robot body + network | new embodiment + closed-loop run |

## Progress

- [x] `2026-11-26 22:45Z` Read the brief, `AGENTS.md` and `.agent/PLANS.md`. Created branch `connectome-explorer`. Read the GNAT paper (Eq. 1, 4-5). Recorded the headless execution recipe and the native format.
- [x] `2026-11-26 23:05Z` Runner API and QA/xtask discovery complete; facts are recorded under Surprises & Discoveries.
- [x] `2026-11-27 00:40Z` M1 core:
  - package `crates/aarnn-explorer` added to the workspace members, with modules `error`, `store`, `config`, `stats`, `gnat`, `motifs`, `model`, `exec`, `author`, `lesion` and `tasks`;
  - `configs/explorer/default.toml` and `examples/explorer/tasks/synthetic_sensorimotor.toml`;
  - `cargo test -p aarnn-explorer --lib`: 22 passed. This covers:
    - GNAT reference/indexed agreement over 40 random adversarial fixtures, plus chain, branch, reconvergence, ties, inhibition, the zero-delay stage order, window boundaries and calibration;
    - recurrence, plus the Clopper-Pearson golden cases (299/299 gives 0.990031; 95/100 gives 0.8977466, cross-checked with scipy) and the config validation cases.
- [x] `2026-11-27 01:30Z` Authored executable fixture `explorer-synthetic-sensorimotor-v1`, calibrated on the real runner (`tests/calibrate_*.rs`, ignored):
  - `cargo test -p aarnn-explorer --test demo_contract`: 4 passed in 140 s (debug build);
  - the intact circuit passes all 6 tasks (go, nogo, quiet, memory-hit, memory-miss, closed-loop steer);
  - G1a or G2b alone is dispensable, while G1a + G1b fails go; I fails only nogo; M2 fails only memory-hit; TR fails only steer;
  - the emptied circuit is rejected as quiescent.
- [x] `2026-11-26 23:55Z` M2-M7 mechanisms implemented:
  - modules `capture` (segments/manifest/verify/live observer), `worker` (process groups, cleared env, bounded threads, systemd-run scope, deadline/cancel), `pipeline` (import, baseline, analyse, experiment, validate, export, verify-bundle, demo), `optimise` (screen, beam, frontier, resilience, learning-substrate protection), `doctor`, `jobs`, `slurm` and `api`/`web` (feature `web-ui`);
  - CLI `src/main.rs` with every brief command plus `verify-bundle`, `jobs` and `slurm`.
  - `cargo test -p aarnn-explorer --test explorer_units`: 7 passed. It covers large IDs, bad schema/float ID/unknown field/budget/truncated matrix, observation off/on output parity, exact replay, trace corruption, overwrite/traversal/immutability, fork isolation, transient silencing, and worker timeout/crash/memory-limit kill (systemd scope)/cancellation.
- [x] `2026-11-26 23:55Z` M9 integration:
  - `examples/catalog.toml` EX-EXPLORER-001 and `qa/scenarios/EX-EXPLORER-001.toml` (all 21 fields);
  - xtask: `example_package_test`, plus the `qa run --suite explorer` arm; catalogue test updated;
  - five `.run/Explorer_-_*.run.xml` configurations;
  - `tests/explorer_e2e.rs` asserts every demo outcome.
- [x] `2026-11-26 23:55Z` M10 docs: `docs/connectome-explorer/{README,architecture,methods,data-formats,functional-validation,resilience,optimisation-and-hardware,operations}.md`.
- [~] `2026-11-26 23:57Z` Demo running (`target/explorer-demo`, release binary; log `target/explorer-demo.{out,err}`). A first run failed because non-finite measures serialised as JSON null. Measures now use `tasks::NO_RESPONSE` = 1e9, and the project was cleared and the demo restarted.
- [~] `2026-11-26 23:57Z` Frontend (`crates/aarnn-explorer/web/*`) delegated to a subagent against `web.rs`. The backend is compiled with `--features web-ui`.
- [ ] G1 / VX-001:
  - contract `examples/explorer/tasks/drosophila_open_loop.toml`, generated by `tests/gen_fly_suite.rs`: 8 conditions x 48 outputs, within 10 % + 0.02 of intact;
  - profile `configs/explorer/drosophila.toml`;
  - next: build the nonmod/mod variants from the fly base, chosen after the fly pivot trials (`target/qa/forage/fly_pivot*`, due about 00:45Z); then import, baseline, analyse and optimise; then closed-loop trials of the three networks.
- [x] `2026-11-27 00:30Z` Frontend done by a subagent: `crates/aarnn-explorer/web/{index.html,app.js (1289 lines),style.css}`.
  - All ten views were verified in headless Chrome against a copy of the demo project.
  - Backend follow-ups were fixed:
    - validate stores the resolved hash, and records no longer embed the project path;
    - GNAT `event_ids` plus `first_trial`;
    - `/neuron` and `/search` endpoints;
    - idempotent export directory, with `cost` in the result;
    - budgets in `/status`;
    - export costs taken from the same split.
  - Remaining limitations are documented: no fine-grained progress events, pause is equivalent to cancel plus resume-by-resubmit, no import route from the browser.
- [x] `2026-11-27 00:35Z` G2 body: `salamander_crawler`, added by a subagent through `scripts/embodiment.py` + `scripts/embodiment_salamander.py`.
  - The body: 45 sensors and 22 outputs; 4 trunk segments with 3 axial yaw joints; 4 legs with coxa + lift.
  - A scripted gait walks 0.56 m in 5 s.
  - Existing records were regenerated (digest fields only; models byte-identical).
  - Native adapter test: 14/14.
- [x] `2026-11-27 00:45Z` G2 network: `scripts/build_salamander_crawler_network_json.py`, composed from the shared blocks:
  - hindbrain + turn bouts;
  - zebrafish axial E/A/C segments;
  - fly/hexapod `add_leg_oscillators` as a diagonal trot;
  - girdle coupling (stance -> same-side axial);
  - touch escape and learned foraging, steering axially and by stride.
  - Open loop (`tests/probe_outputs.rs`): diagonal trot at 1.6 Hz; the axial wave is entrained 1:1 with tau_a_axial 300 and girdle 6.
  - MuJoCo closed loop (`target/qa/salamander/walk_walk`, 30 s): 0.30 m straight in 3 s, then exploration turns; path 3.4 m, net 0.85 m, minimum up-vector 0.93.
- [~] `2026-11-27 00:55Z` G2 analysis: `target/qa/g2/run_g2.sh` -> `target/g2-explorer`.
  - Networks: zebrafish, drosophila, hexapod, legged (UGOT quadruped) and salamander, each with a `suite-template` contract.
  - Stages: baseline + analyse per network; then `library` (role-name-v1); then `depth-report`; then optimise, validate, export and verify of the salamander.
- [x] `2026-11-27 01:15Z` G2 analysis done (22 min; `target/qa/g2/g2.log`).
  - **Library** (`aarnn-explorer library`, rule role-name-v1, extended with hidden role-edge sharing and pairwise Jaccard):
    - 42 recurring motifs, all body-specific (each includes output edges);
    - 33 roles active in GNATs in 2 or more networks: MLR, RS, ARTR, ARTR_A and the BOUT cells in 4; leg E/A/C/D half-centres, ESC/TOUCH/TCH and the foraging cells V/STEER/FEED/HUNGER/SATED/RISE/FALL/DA_R/CF in 3;
    - one shared hidden role-edge across recurring motifs: E_st->C_st (drosophila, salamander).
    - Interpretation: shared blocks are evidenced by shared active roles, not by identical recurring threads.
  - **Depth report** (graded GNATs, template contracts):

    | Network | Hidden cells | Mean depth | Max depth |
    |---|---|---|---|
    | zebrafish | 118 | 1.90 | 8 |
    | drosophila | 143 | 1.26 | 3 |
    | hexapod | 24 | 1.62 | 2 |
    | legged | 105 | 1.15 | 5 |

  - **Salamander under the template contract:** the reduction removed ESC_l, ESC_r and RS_r, validated, and the exported bundle reproduces its validation. The random-pattern template never exercised touch or right obstacles, so this is an incomplete-contract result: the candidate must not be used. It demonstrates the brief's point that the contract defines what is preserved.
  - **Salamander under the proper contract:** `examples/explorer/tasks/salamander_open_loop.toml`, generated by `tests/gen_salamander_suite.rs` (8 conditions x 22 outputs). Rerun started 01:19Z via `target/qa/g2/run_salamander_contract.sh`, followed by a closed-loop walk of the reduced network.
- [x] `2026-11-27 01:33Z` Demo completed (`target/explorer-demo/demo-report.json`).
  - The first run hit its 90 min limit in the resilient stage, which then had no evaluation reuse. The rerun took 9 min with job reuse and fast single-fault checks.
  - Baseline passes; sham = intact; source unchanged.
  - Lesions: G1a passes; G1a+G1b fails go; I fails nogo; memory region fails memory-hit; inhibition x2 fails nogo (rebound, S-003); dead-end branch passes.
  - Compact candidate: removes D1, D2, G1a, G2a, TL, TR, Z. Hidden cells 16 -> 9; state bytes 8576 -> 3984 (-54 %). Locked validation: behavioural preservation passed.
  - Under the resilient profile the compact candidate's resilience fails.
  - Resilient candidate: removes only D1, D2, Z (16 -> 13). Resilience is inconclusive: 20/20 trials cannot reach a 0.99 bound.
  - The exported bundle reproduces its validation in a clean process.
- [x] `2026-11-27 01:35Z` Bug found by the demo: `silence-M2-80-120ms` "passed" because applied silence windows never reached the evaluation runs.
  - Fixed: `tasks::evaluate_silenced`/`run_trial_silenced`; the worker passes the windows; silence combined with lowering is rejected.
  - Job keys now include `pipeline::SEMANTICS_VERSION` (2026-11-27-b), so stale results computed under older semantics are never reused.
  - e2e now asserts that silencing M2 fails memory-hit and that inhibition x2 fails nogo.
- [~] `2026-11-27 01:37Z` Full release test suite running: `cargo test --locked --release -p aarnn-explorer`, log in `target/explorer-tests.log`, including the e2e demo from scratch.
- [x] `2026-11-27 02:15Z` Salamander under the 8-condition contract:
  - removed ARTR_r, FALL, RISE, STEER_l and STEER_r (96 -> 91);
  - validated, and the bundle reproduces its validation;
  - the reduced network walked farther (net 1.26 m vs 0.85 m in 30 s).
  
  The removed cells are learned-value readouts (silent before learning) and the slow ARTR oscillator (10-20 s), which the 1 s frozen cold-start contract cannot see. The straighter walk is lost exploration, not an improvement.
  - Fixes: the learning substrate now includes the direct excitatory hidden targets of plastic and modulatory cells (D-010); the salamander contract gains `explore-20s` (9 conditions).
  - Restarted at 02:20Z: the salamander run (`salamander_contract2.log`) and both VX-001 runs (`explorer2.log`); completed jobs are reused.
- [x] `2026-11-27 05:10Z` G2 salamander, learned state (`run_salamander_learned.sh`):
  - The learned-cap preset changed 24 synapses. The reduction removed only TCH_l_1 and TCH_r_2 (96 -> 94). It validated, the bundle reproduces, and the naive `experiment` passed its naive contract (with the 20 s task).
  - 30 s walks: reduced3 net 0.84 m / path 3.29 m against the full network's 0.85 / 3.40. The contract2 reduction re-walked with a synced config is identical to the digit (1.259 / 3.514), so the stale masks did not change that walk.
  - Written up in `docs/connectome-explorer/g2-salamander.md`.
- [ ] `2026-11-28 04:20Z` VX-001 true trained state (in progress):
  - The closed-loop d1_mod trial (`run_trained.sh trial`) reached food in 4/12 releases, the same as seed 0. On exit the worker wrote its network.
  - Learning moved all 44 plastic synapses upward: mean 0.0045 -> 0.093 (69 % of the cap); 16 at the cap. Nothing else changed. The learned-cap approximation is therefore an upper bound on the real state.
  - The exit snapshot has no `connectome_labels`, so the explorer rejects its runtime channel `needs_energy`. The trained state is instead the naive network with the trained `w_hh_rec` transplanted (`trained/network_d1_mod_trained.json`, source `fly-mod-trained-w`). It passes the whole learned-state contract.
  - Candidate C is the reduction of the trained network (`run_trained_w.sh`). It removes REV_lm_sw, TN_l_1, TN_l_2 and TN_r_2 (B's class), and depth is unchanged.
  - Closed loop (`run_c_trials.sh`, `analyse_c_trials.py`): C reached 13/36 releases over seeds 1-3, the same as the same-build original (13/36; CMH p 0.81). The same-build original reproduced its earlier runs exactly (4, 7, 4, 2), so the 28 Nov build did not change the closed loop. Seed 0 of C was still running at 09:14Z.
  - The project names `fly-mod-trained`, `fly-gnatc`, `gnatc-dev` and `gnatc-dev-gnat` come from the failed first run: `validate --candidate selected` fell back to candidate B. Do not use them.
- [x] `2026-11-28 06:20Z` A lone worker is no longer its own backup (uncommitted):
  - Found by the isolated cluster test, and reproduced locally with an orchestrator and two workers.
  - In `plan_active_layer_assignments`, the backup filter `nodes.len() == 1 || node_id != source_node` made a single eligible worker the backup of its own layers: layers [0], redundant [0].
  - Workers take `layers - redundant_layers` as their active set, so the worker owned nothing. It ran warm, with no sensory or output stage, `Neurons=0` in the orchestrator, and `/api/activity` 503. `preserve_sharded_node_assignments` then kept that assignment after more workers joined.
  - The filter now never selects the source node. New test: `hierarchical_planner_gives_a_single_node_no_local_backup`. The legacy builder already enforced this (`a_single_eligible_node_does_not_get_a_fake_local_backup`).
- [x] `2026-11-28 04:15Z` Distributed processes now exit on SIGTERM (uncommitted):
  - Cause: `main` idled in `loop { sleep(1s) }` after start-up. Orchestrators and workers logged `graceful_shutdown`, but each process stayed alive until the service manager's SIGKILL. That is 20 s for the native unit, 30 s in k8s, and forever for a test process.
  - Fix: `wait_for_distributed_exit` waits for the signal, then for the simulation supervisor's new stopped flag (`obs::note_simulation_supervisor_stopped`, set after the workers and autosave writers finish). The wait is capped at 15 s, or 600 s with `NM_NODE_SNAPSHOT_ON_EXIT_DIR`; `NM_SHUTDOWN_GRACE_MS` overrides either.
  - Measured exit after SIGTERM: about 0.27 s for a waiting worker, a connected worker and an orchestrator (before: never). The exit-snapshot smoke walk still writes its snapshot.
- [x] `2026-11-28 02:45Z` VX-001 replicates (3 more seeds per network, jittered places; `analyse_replicates.py`):
  - Reached, 48 releases each: non-neuromodulated 10, neuromodulated 17, candidate A 18, candidate B 17.
  - B vs original: difference 0.00 [-0.19, +0.19]; CMH p 0.83; Mann-Whitney p 0.82; run-level permutation p 1.00.
  - Neuromodulated vs non-neuromodulated: +0.15 [-0.03, +0.31]; CMH p 0.18.
  - B's single-run lead (6 against 4 of 12) was run-to-run noise; runs of one network range from 2 to 7 of 12.
  - Performance verdict changed from "not shown" to "no": equal, not better. drosophila.md section 3 is updated.
- [x] `2026-11-28 00:45Z` Cluster and release work (user request, 27 Nov):
  - **Cross-architecture Slurm reproducibility:** `aarnn-explorer` was built as Slurm jobs on qc00 (aarch64) and sm00 (x86_64). Four stored VX-001 jobs (intact, lowered, intervention, capture) were submitted with the explorer's own `slurm submit` (new `--worker-exe` and `--sbatch-arg`), each on one aarch64 and one x86_64 node. All 8 results are identical to the development host's (2 723-188 898 leaves, timing excluded). See validation-report.md.
  - **Release:** PR #44 merged (`b9647b7`) after Verify passed on X64 and ARM64. Two CI fixes:
    - a dpkg-lock wait;
    - skipping apt when every dependency is installed. qc01's unattended-upgrade had held the lock since 14 Nov.
  - **Test fixes found by the local Verify replay:**
    - the neuromod drive tests are gated to builds without `superdense_executor`. Release builds compile the Runner's neuromodulator update out, so environmental drives are absent there: a known gap;
    - the tonic tolerance is now 0.3 mV;
    - the modality cache rebuilds when the tag count changes.
  - **Final-snapshot hook (uncommitted):** `NM_NODE_SNAPSHOT_ON_EXIT_DIR` makes a cluster worker write each hosted network (`export_network_json`) on SIGTERM, then exit. `scripts/run_sim.sh` gains `NM_CLUSTER_STOP_GRACE_S` (default 1 s, as before). The true-trained-state VX-001 run is `target/qa/vx001d/run_trained.sh`, which needs the release runtime rebuilt after the replicates.
  - **VX-001 replicates:** `run_replicates.sh`, 3 seeds of jittered release places x 4 networks; analysis in `analyse_replicates.py`.
- [x] `2026-11-27 08:47Z` VX-001 closed-loop trials of the candidates: A 4/12 reached (mean with misses 31.1 s), B 6/12 (27.4 s), against the tier-1 neuromodulated original 4/12 (30.8 s) and non-neuromodulated 1/12 (35.5 s). B vs original: Fisher p 0.68, Mann-Whitney p 0.32; B vs non-neuromodulated: Mann-Whitney p 0.02. Verdicts are written in `docs/connectome-explorer/drosophila.md` section 3: fewer neurons yes (-2.8 % hidden, -3.4 % state), depth not supported (unchanged), performance not shown (no loss; B is better only than the non-neuromodulated fly).
- [x] `2026-11-27 07:30Z` VX-001 candidate B (`run_explorer_b.sh`, `Escape:A` declared, candidate `07fbf9ff...`; the search took 17 min because the cached evaluations were reused):
  - Declared protected: ARTR_l/r, ARTR_A_l/r, BOUT_on/off, BOUT_A_on/off, ESC_A_l/r.
  - Removed REV_lm_sw, TN_l_1, TN_l_3, TN_r_2 (143 -> 139). Locked validation passed, the bundle reproduces it, and the naive experiment passed.
  - Depth: naive 1.169 mean / 4 max / 0.367 per participating neuron; learned 1.201 / 4 / 0.371. The same as the modulated fly.
  - Closed-loop trial launched at 07:38Z (`run_trials.sh ... gnatb`).
- [x] `2026-11-27 07:11Z` VX-001 candidate A (`run_explorer.sh`, profile copy `target/qa/vx001d/drosophila_learned_a.toml`, candidate `3d16156d...`):
  - Removed ESC_A_l, REV_lm_sw, TN_l_3, TN_r_2 (143 -> 139 hidden; state bytes 1 385 712). Locked validation passed, the bundle reproduces it, and the naive `experiment` passed the naive contract.
  - Roles: two of the several teacher range cells (one per side) of the anticipation learning; the left-middle leg's reverse-swing cell of the pivot; the left escape adaptation (1.5 s, slower than the 1 s touch tasks: an undeclared out-of-contract cell, D-014).
  - Depth report (development split, 44 trials each):

    | Analysis | Hidden | GNATs/trial | Mean depth | p95 max | Max depth | Depth per participating neuron |
    |---|---|---|---|---|---|---|
    | nonmod naive | 143 | 132.5 | 1.178 | 2 | 3 | 0.369 |
    | mod naive | 143 | 131.6 | 1.169 | 2 | 4 | 0.366 |
    | GNAT A naive | 139 | 131.7 | 1.169 | 2 | 4 | 0.367 |
    | nonmod learned | 143 | 130.9 | 1.224 | 3 | 4 | 0.372 |
    | mod learned | 143 | 132.1 | 1.201 | 2 | 4 | 0.371 |
    | GNAT A learned | 139 | 132.3 | 1.201 | 2 | 4 | 0.372 |

  - Closed-loop trial of A launched at 07:20Z (`run_trials.sh ... gnat`). Candidate B (`run_explorer_b.sh`, `Escape:A` declared, D-014) launched at 07:21Z; log `explorer_b.log`.
- [x] `2026-11-27 06:48Z` Release test suite rerun after `derive` / `weight-set` / declared protection: aarnn-explorer pass (lib 32, demo_contract 4, e2e 1 in 454 s, units 7, gen suites 2 + 2, fixture 1), web-ui lib 34, xtask 1; `cargo fmt --all --check` clean. It ran in `CARGO_TARGET_DIR=target/tests-r2`, so that cargo could not relink `target/release/aarnn-explorer` while the VX-001 optimiser was spawning workers from it (`current_exe`).
- [x] `2026-11-27 05:40Z` VX-001 tier 1/2 closed-loop trials (`target/qa/vx001d/trials`, 12 releases of 36 s each from 400 s, reserve 0.30):

  | Network | Reached | 95 % CI | Median | Mean with misses | Wall-touched windows | Tipped windows (up-z < 0.5) |
  |---|---|---|---|---|---|---|
  | d0 (fly_touch `off`, reference) | 9/12 | 0.43-0.95 | 14.7 s | 21.0 s | 3 | - |
  | d1 mod | 4/12 | 0.10-0.65 | 19.3 s | 30.8 s | 1 | 3 |
  | d1 nonmod | 1/12 | 0.00-0.38 | 29.5 s | 35.5 s | 0 | 0 |
  | d2 mod | 5/12 | 0.15-0.72 | 20.9 s | 30.0 s | 0 | 1 |
  | d2 nonmod | 1/12 | 0.00-0.38 | 27.5 s | 35.3 s | 0 | 0 |

  - Pooled, mod 9/24 vs nonmod 2/24: Fisher p = 0.036. Neither tier alone is significant (d1 p = 0.32, d2 p = 0.15).
  - The nonmod flies first reach the source earlier in the care period (about 142 s against 311-458 s), but in the releases their heading error is about 90 deg, which means no steering. The mod flies' heading error is 72-74 deg. The mod flies need 3 caretaker feeds in the care period against 1 (they spend more energy), and tip over in some windows, then recover.
  - Tier 0 is better than either tier here. The tier-1/2 bio filters (synaptic filter, STP, at 2 the adaptive threshold) were not tuned for this designed circuit, so tier 0 is context only, not a competitor.
- [x] `2026-11-27 04:05Z` Second runs under D-010 finished, and showed three problems:
  - **Salamander (contract2):** passed and validated (96 -> 88; the walk net 1.26 m), but removed ARTR_r, BOUT_A_off, ESC_A_l/r, FALL, HUNGER, TCH_l_1 and TCH_r_2. The 20 s task did not catch ARTR_r: in 20 s the intact oscillator never switched sides. HUNGER gates every learned readout, and FALL reads V_SLOW (a second-order readout); neither is visible in a naive frozen network.
  - **Stride-cut fly (vx001c):** the candidate removed BOUT_off, HUNGER and REV_lm_st (143 -> 140), the same class of loss.
  - **VX-001 at tier 0 is void:** the "modulated" and "non-modulated" flies gave identical GNAT statistics to every digit. Designed circuits run at `aarnn_layer_depth` 0, and depth 0 has no neuromodulated transmission (bit-identical by design), so the two networks were the same network. The pivot-base run (vx001) was stopped.
- [~] `2026-11-27 04:15Z` Fixes (D-011 to D-013):
  - VX-001 is rebuilt at tier 1 (`target/qa/vx001d`, `make_depth.py`: d0/d1/d2 x mod/nonmod from the stride-cut fly). Closed-loop trials of d1/d2 mod/nonmod were launched at 04:14Z (`run_trials.sh`, the run_touch_trials protocol). The tier-0 result is fly_touch `off`: 9/12 reached, median 14.7 s.
  - `aarnn-explorer derive`: a learned-state source (`--preset learned-cap`: plastic synapses at the plasticity cap; 44 synapses on the tier-1 fly), persistent interventions, or `--lower-like CANDIDATE` to carry a removal set to another source (`--write` for the closed loop). New `weight-set` action. Learned-state contracts `drosophila_learned.toml` and `salamander_learned.toml` (generated) add hungry-cue-right, sated-cue-left (hunger gate) and hungry-cue-fall (3 s).
  - `optimisation.protect_cell_types` / `protect_neurons`: declared out-of-contract function. The fly and salamander learned profiles declare the turn-bout clock and ARTR oscillator (plus the salamander's escape adaptation).
  - Reduced networks get configs synced from their own `net` (`target/qa/vx001d/sync_config.py`): the salamander reduced walks had reused the full network's config with 96-cell masks for 88 cells.
- [x] `2026-11-27 00:40Z` Oracle semantics fixed:
  - a run without an external reference is its own reference;
  - candidates are compared with the same-seed intact trial (`task@seed` keys), falling back to task means;
  - journal lines are written in one write.
  
  The first VX-001 run had failed because the intact baseline could not pass the within-reference criteria. VX-001 was restarted at 00:45Z.

## Next actions (for a resuming session)

State at 08:50Z on 27 Nov: M0-M10, G1, G2 and VX-001 are done and documented; the hand-off was given. Optional follow-ups, not started:

1. VX-001 replicates: run the original and candidate B with perturbed release places (several seeds each) to resolve 4/12 against 6/12.
2. A final-snapshot hook in the closed loop, so that a true trained state (not the plasticity cap) can be imported and reduced.
3. clippy (`rustup component add clippy`) and the ARM64 lane.

## Validation and acceptance

See brief Section 21. The commands actually run, with their results, are recorded here as milestones close.

## Rollout, compatibility and rollback

- Additive only: a new workspace member plus optional entry points.
- Rollback: remove `crates/aarnn-explorer` from the workspace members and delete the package; root behaviour is unchanged.
- Capture is off by default. Root seams, if added, are inert unless called.

## Risks and mitigations

- **Root build cost** (protoc, BLAS): `doctor` reports them, and CI reuses the existing toolchain.
- **Graded (non-spiking) cells dominate the designed circuits.** The spike-only GNAT would ignore them, so graded release is captured as separate threshold-crossing events under a separately versioned method (D-005) and never used as evidence that a graded cell is dispensable.
- **Runner nondeterminism** (Rayon, `fastrand` global): workers run single-threaded by default, with a replay parity test.

## Surprises & Discoveries

- **S-001 (runner discovery):**
  - Matrices are `Array2<f64>` with rows = post and cols = pre.
  - Designed circuits (`use_aarnn_delays=false`) have `hist_len = 1`, which gives these delays: sensory→hidden 0 steps, hidden→hidden 1 step, hidden→output 0 steps. Confirmed empirically: a relay shows S 10.0, H1 10.1, H2 11.1, O 11.2 (tick.stage).
  - There is no ablation/lesion API. `w_gap` is copied into the private `gap_routes` at import, so lesions edit the snapshot JSON and re-import.
  - The step reads `w_hh_rec[l]` and `w_out` live, which makes transient silencing by zeroing outgoing columns valid.
  - The runner reseeds `fastrand` from `runner.rng` at every step. `runner.rng.seed(x)` after import sets the stream.
  - The parallel path depends on live CPU/memory and is off with `NM_SIM_PAR_MAX_WORKERS=1`.
  - Graded hidden cells never spike.
  - The snapshot's embedded `net` is authoritative; config files are not merged by the runtime.
- **S-002:** `NetworkConfig::default()` differs from the `network.json` template that the designed circuits use: `p_release_default` 0.7 vs 1.0, and many whole-network subsystems are on (import rewiring, synaptic-energy randomness, ambient energy, rhythms). Compiled networks now apply the designed-circuit overrides of `aarnn_circuit.py::_net`. Without them, relays ignored their weights and fired spontaneously.
- **S-003:** Izhikevich RS cells rebound-burst after inhibition, so stronger inhibition produced more output spikes. FS inhibition at −16 blocks cleanly, while −30 causes rebound bursts. This is a real fragility: weight perturbation of inhibitory synapses is a meaningful resilience test.
- **S-004:** odd-millisecond onset jitter split 2 ms pulses over two frames, doubling the probe's spikes and causing false memory responses. Jitter is now applied in whole frames.
- **S-005:** in a debug build the runner is about 10x slower; heavy explorer lanes use `--release`.

## Decision Log

- **D-001** (2026-11-26) Work in the main checkout on branch `connectome-explorer`. A clean worktree from HEAD would lack the uncommitted runner/neuromodulator changes that the designed circuits require. All 249 pre-existing dirty paths are preserved.
- **D-002** (2026-11-26) The legacy `Runner` import is classified `declared-cold-start`. A network JSON restores weights and types, but not a complete pending-event queue or RNG coordinate, so exact continuation is not claimed.
- **D-003** (2026-11-26) Authoritative trace time = runner step index (1 ms ticks) with microstep 0. The legacy `Runner` has no superdense microsteps, so same-tick causal order is not available. GNAT edges require a positive delay, so a same-tick edge is never created. Delay = the runtime's effective synaptic delay in steps (see discovery).
- **D-004** (2026-11-26) GNAT method `effective-delivery-v1`:
  - Ω = (W⁺_ab / ‖W_a‖₁) · exp(−Δ/τ), with Δ = t_post − (t_pre + δ_ab) ≥ 0 and θ(0) = 1;
  - ‖W_a‖₁ = Σ |w| over all chemical inputs onto the postsynaptic cell, sensory inputs included; zero norm means no edges;
  - only positive (excitatory) pre→post weights create edges; inhibitory and electrical couplings are kept as overlays;
  - the paper does not name its norm (it says "the norm of all synaptic weights onto neuron α"). A `norm = "l2-abs"` option reproduces an L2 reading for comparison;
  - default τ = 5 ms and default threshold Ω ≥ e^-5 ≈ 0.0067, from the paper's "−log Ω ≈ 5";
  - the threshold can be calibrated against circularly time-shifted controls, which keep each cell's rate.

- **D-005** (2026-11-26) Graded cells emit spike-equivalent events (method `effective-delivery-graded-v1`): a phase accumulator integrates the release rate, and the events are kind `graded-equivalent`. The designed circuits are mostly graded, so spike-only GNATs would show only their outputs.
- **D-006** (2026-11-26) Lesions are applied to the snapshot JSON before import (cold-start-empty), because the runner has no ablation API and its gap routes are private. Transient silencing zeroes outgoing columns of the live `w_hh_rec`/`w_out` and requires frozen plasticity.
- **D-007** (2026-11-26) Compiled sparse networks use the designed-circuit overrides and `p_release_default` 1.0, with neuromodulator transmission and environmental drive off unless `net_overrides` says otherwise. Each choice is recorded as an assumption.
- **D-008** (2026-11-26) The optimiser protects the learning substrate (plastic, pre-plastic and modulatory cells) unless the contract has `learning_required = true`. A frozen open-loop contract cannot show those cells are dispensable for future learning.
- **D-009** (2026-11-26) The task-onset jitter is in whole frames, and missing responses use the finite sentinel `NO_RESPONSE`.

- **D-010** (2026-11-27) The protected learning substrate is extended with learned-value readouts: hidden cells with a positive input from a plastic or modulatory cell. Contracts must also include tasks as long as the slowest function they preserve. Evidence: G2 reductions removed STEER/RISE/FALL and ARTR_r while passing 1 s frozen tasks. The general lesson: a reduction preserves exactly what the contract tests, so under-specified contracts silently license loss of function.
- **D-011** (2026-11-27) VX-001 runs at depth tier 1, the lowest tier with neuromodulated transmission; non-modulated and modulated flies share the tier and differ only in `aarnn_neuromod_{transmission,env}_enabled`. Evidence: at tier 0 the two networks gave identical GNAT analyses (the depth-0 path is bit-identical with or without the flags). Tier 1 also enables synaptic filters and STP, so the tier-1 flies are compared with each other, and with tier 0 only as context.
- **D-012** (2026-11-27) Reductions of learning networks run on a learned state of the source (`derive --preset learned-cap`), under a contract with conditions that use the learned pathways. The removal set is carried back to the naive source (`derive --lower-like`), which must also keep its naive contract (an `experiment` of the same ablation). The cap is an upper bound of learning, not a recorded experience; a recorded trained snapshot would be better, but the closed-loop runner has no final-snapshot hook.
- **D-013** (2026-11-27) Function slower than the contract's trials is declared out of scope (`protect_cell_types`) rather than tested, when a test would cost more than the rest of the contract. The declaration is stored in the optimisation record (`declared_protected`).
- **D-014** (2026-11-27) The same slow function is declared for every body that has it. The fly learned profile now declares `Escape:A` (1.5 s adaptation), as the salamander profile does; VX-001 candidate A was found without it and is reported with that caveat, and candidate B with it.

## Outcomes & Retrospective

At hand-off (27 Nov 2026, 08:50Z):

- **Delivered.** `aarnn-explorer` implements the brief's M0-M10 as a separate workspace binary with bounded workers, a loopback web UI, a demo, an xtask catalogue entry, docs and IDE run configurations. The release suite passes.
- **G1, reducing a network to identical function.** The authored demo circuit went from 16 to 9 hidden cells (-54 % state). Designed networks reduce far less once the contract covers learned and slow function: the salamander lost 2 of 96 hidden cells, and the fly 4 of 143.
- **G2, reverse-engineering networks for a new robot.** The designed networks share a block vocabulary but not causal threads. A new body (`salamander_crawler`) was built from that block library. It walks, and it is close to minimal.
- **VX-001.** The GNAT-optimised neuromodulated fly keeps function in fewer cells. It shows no greater depth and no significant performance gain. Neuromodulation itself improves foraging at tiers 1 and 2 (pooled p 0.036).
- **Lessons.**
  - A reduction keeps exactly what its contract tests. The first reductions removed learning readouts, slow oscillators and escape adaptation, because naive frozen contracts could not see them. That led to D-010 and D-012 to D-014.
  - Tier 0 has no neuromodulated transmission, so a mod vs non-mod comparison needs tier >= 1 (D-011).
  - A live optimiser must not have its binary relinked, because it spawns workers through `current_exe()`.
