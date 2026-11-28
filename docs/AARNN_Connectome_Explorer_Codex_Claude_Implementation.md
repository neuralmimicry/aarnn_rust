# AARNN Connectome Explorer: end-to-end implementation instructions for Codex or Claude

Version 1.0 · 25 November 2026 · NEURALMIMICRY

This is a software implementation brief. It specifies software to create; it does not claim that the explorer, its commands or a reduced fly brain already exist. Repository observations below were checked against `neuralmimicry/aarnn_rust` commit `f767570ff20ebd32918d71a71fbc9ed2cb3e8be2`. Reconcile every path and API with the user's current checkout before editing. Do not downgrade or reset a newer checkout to that commit.

## 1. Copy/paste instruction to the coding agent

Implement the complete solution in this document inside the existing `github.com/neuralmimicry/aarnn_rust` Cargo workspace. The likely local checkout is `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`; discover the actual workspace root instead of assuming that path exists.

Create a separately runnable Rust binary named `aarnn-explorer`. It must interactively explore existing connectomes and recorded neural activity, extract Graphical Neural Activity Threads (GNATs), run controlled interventions on isolated experiment copies, optimise circuits against explicit behavioural and resilience contracts, and export executable reduced AARNN candidates with reproducible evidence.

The motivating use case is reducing a Drosophila connectome for implementation on silicon while preserving selected sensorimotor, memory, learning and recovery functions. Generalise the implementation to other supported connectomes. The selected contract defines what must be preserved; do not claim that a finite benchmark proves equivalence to every function of an animal.

Do the implementation, tests, documentation and IDE integration. Do not stop after producing a plan, mock interface, algorithm description, disconnected demonstration or list of suggested future work. Work through the milestones in Section 20. Keep a living ExecPlan so a later context window or IDE session can resume without repeating discovery. Resolve routine implementation decisions from repository evidence and this brief. Continue independent work when an external dataset, simulator or device is unavailable; distinguish the completed software mechanism from the external scientific validation that remains unavailable.

Read the current root and applicable nested `AGENTS.md`, `.agent/PLANS.md`, active phase plans and relevant normative specifications before production edits. Preserve their biological and distributed execution invariants. This request authorises local source changes, local builds and isolated synthetic experiments. It does not authorise mutating a running production brain, publishing code, deploying services or driving physical actuators. Those actions are not needed to complete the software.

Keep new project-authored code consistent with the repository's MIT licensing and SPDX conventions. Preserve third-party and dataset licences. Keep all numerical analysis, simulation integration, orchestration and optimisation in Rust. A browser presentation layer may use the project's existing JavaScript/WebGL conventions; do not add Python, an LLM API or a second simulation implementation as a runtime requirement.

## 2. Repository evidence and integration map

The following were observed in the inspected commit. These are integration candidates, not instructions to copy their implementations or assume their production gates have passed.

| Existing location | Observed responsibility | Required use or boundary |
| --- | --- | --- |
| Root `Cargo.toml` | Existing Cargo workspace, `aarnn_rust` library, product and migration features | Add the explorer to this workspace; retain existing default members and product feature behaviour. |
| `src/deterministic.rs` | Stable identities, `LogicalTag`, canonical event keys, numerical/RNG/digest primitives | Import the canonical types. Do not invent another logical clock or stable identity implementation. |
| `src/engine.rs` | `RunnerEngine`, snapshots, topology projections and `EngineActivity` | Reuse execution/snapshot integration. Display projections alone are insufficient scientific input. |
| `src/runner.rs` | Biological execution, `Snapshot`, optional `SnapshotRuntimeState`, native connectome state | Reuse model semantics and snapshot decoding. Audit exact-resume completeness and automatic import transformations. |
| `src/runtime_api.rs` | Workspace snapshot, topology and activity contracts and client | Use authenticated read paths where appropriate. Do not use activity polling as an allegedly complete spike recorder. |
| `src/shard_executor.rs` | `StableShardExecutor`, `ShardExecutionResult`, `StableShardCheckpoint` | Integrate tracing and isolated execution at genuine biological transitions and commit boundaries. |
| `src/topology_model.rs` | Versioned topology and compiled execution plans | Use stable ownership, delay and route semantics; use sparse graph representations for large imports. |
| `src/stable_executor_store.rs` | Immutable complete checkpoint sets with plan/fabric digests | Restore a coherent set, not an arbitrary collection of the latest shard files. |
| `src/cluster_snapshot.rs`, `src/consistent_cut.rs`, `src/checkpoint_transfer.rs` | Cluster export, coherent cuts and transfer | Reuse for distributed captures instead of adding a global per-tick barrier. |
| `src/causal.rs`, `src/durability.rs`, `src/stable_executor_durable.rs` | Causal settlement, logs and durable execution | Preserve committed-event, replay and non-convergence semantics. |
| `src/scientific_validation.rs` | Numerical/timing comparison and explicitly separate behavioural validity | Extend with a separate behavioural result layer; do not reinterpret a matching digest as biological validation. |
| `src/morphology_contract.rs`, `src/visualization.rs` | Stable display contracts, provenance and presentation detail policies | Reuse presentation concepts without allowing visual decimation to alter analysis. |
| `src/ga.rs` | Existing genetic optimisation responsibility, exported by the library | Inspect before adding search infrastructure. Reuse suitable generic machinery without changing its existing callers. |
| `crates/aarnn-embodiment`, `adapters/webots`, `adapters/mujoco` | Shared embodiment and simulator adapters | Use these for closed-loop tasks; do not create another sensory/motor mapping stack. |
| `src/bin/web_ui.rs`, `web_ui/`, `src/ui.rs` | Existing browser and native interfaces | Add optional explorer entry points without putting heavy analysis in their render or simulation loops. |
| `tools/xtask`, `examples/catalog.toml`, `qa/`, `.run/` | Existing orchestration, example and IDE conventions | Integrate scenarios and commands with these rather than duplicating test runners. |

Two specific limitations are already visible: `EngineTopologySnapshot` has a `truncated` flag, and its node identities are explicitly display identities scoped to a topology generation. `EngineActivity` contains per-step neuron indices and selected history frames. Neither is a substitute for full scientific topology, stable biological identities and event provenance.

The root package also has native build dependencies, including protobuf generation and BLAS-enabled numerical dependencies. A new workspace member does not automatically become dependency-free. Discover and document the actual headless build requirements; do not promise a portable build merely because desktop UI features are disabled.

## 3. Observable outcomes and scientific claim boundaries

Deliver these working capabilities:

1. Open a native AARNN snapshot/checkpoint bundle or a validated imported connectome; inspect anatomy, types, connectivity, weights, delays and provenance.
2. Generate or import a complete activity trace, with explicit coverage and timing semantics, and explore GNATs and recurring subthreads.
3. Select neurons, synapses, cell classes, regions or activity motifs and run interventions on isolated copies.
4. Compare intact and modified circuits on declared functional tasks, including closed-loop tasks and learning where selected.
5. Search for smaller circuits under independent behavioural, resilience, evidence and hardware constraints.
6. Export a genuinely executable candidate, its complete mapping/provenance, a validation report and a measured or explicitly estimated cost report.
7. Resume interrupted campaigns and inspect results without reopening or modifying the original running brain.

Represent claims separately:

| Claim | Meaning |
| --- | --- |
| Implementation correctness | This mechanism passes its deterministic and failure-path tests. |
| Reference-model agreement | The candidate matches a declared computational reference within declared tolerances. |
| Behavioural preservation | Named tasks pass over the declared input, state, history and environment distribution. |
| Biological validity | Named experimental observations support the model; untested biology remains unvalidated. |
| Resilience | A named fault family, magnitude, adaptation policy and evidence procedure pass. |
| Hardware benefit | Measured target improvement, or a clearly labelled cost estimate. |

A static connectome may support structural exploration immediately. GNAT extraction requires activity; functional reduction requires executable dynamics and task oracles. Missing inputs must produce an explicit capability state, not a fabricated result.

## 4. Process and dependency architecture

Prefer one new workspace package, `crates/aarnn-explorer`, containing a library for testable application logic and the `aarnn-explorer` binary. Preserve the binary name even if repository discovery justifies another package layout. Do not introduce a nested workspace.

Suggested responsibilities, adapted to existing canonical equivalents:

```text
crates/aarnn-explorer/
  Cargo.toml
  src/main.rs
  src/lib.rs
  src/project.rs
  src/import.rs
  src/capture.rs
  src/gnat.rs
  src/motifs.rs
  src/intervention.rs
  src/experiment.rs
  src/tasks.rs
  src/resilience.rs
  src/optimise.rs
  src/hardware.rs
  src/export.rs
  src/worker.rs
  src/api.rs
  web/                         # presentation assets, if not shared elsewhere
  tests/
  benches/
docs/execplans/connectome-explorer.md
docs/connectome-explorer/       # operation, methods, schemas, examples, limitations
examples/explorer/              # tiny authored models and task definitions
qa/fixtures/explorer/
qa/scenarios/explorer/
```

Add only narrow observer/experiment seams to the authoritative root implementation. For example, a root `activity_trace` module can define event-observer contracts using canonical AARNN types. The explorer then depends on that root API; the root library must not depend on the explorer package. Keep dependency direction acyclic. If a canonical portable crate already exists in the newer checkout, use it. Extract a shared type only once when a measured dependency problem actually requires it; do not make wholesale workspace restructuring a prerequisite for this feature.

Run heavy analysis in the explorer process. Run simulation experiments in bounded child worker processes using the same binary's internal worker mode. Workers execute the existing AARNN biological implementation, not an explorer-specific simulator. Each worker owns its mutable state. UI, API, persistence and analysis jobs have bounded independently managed concurrency.

Provide a complete primary interactive interface as a loopback browser application served by the Rust binary, behind a `web-ui` feature. Reuse existing visualisation assets and policies where sensible. A headless build must support all capture, analysis, experiment, optimisation, validation and export workflows. Native and existing web interfaces should gain optional actions to open the explorer or export an exploration bundle; they must not acquire a mandatory explorer dependency.

The engine continues independently of explorer crashes, cancellation, saturation or UI closure. Do not hold an engine lock while serialising large exploration data, running analysis or waiting for a client. Thread separation alone is not resource isolation: implement Section 17 and measure interference.

## 5. Discovery and implementation preparation

Before editing, run the applicable read-only discovery commands and record results in the ExecPlan:

```bash
git rev-parse --show-toplevel
git status --short
git rev-parse HEAD
cargo metadata --no-deps --format-version 1
rg --files -g AGENTS.md -g AGENTS.override.md -g Cargo.toml -g rust-toolchain.toml
rg --files docs/execplans docs/specifications .github/workflows .run
rg -n 'LogicalTag|StableShardExecutor|ShardExecutionResult|StableExecutorCheckpointSet|EngineTopologySnapshot|SnapshotRuntimeState|ValidationReport' src crates tests
```

Find the current scientific model, feature matrix, importer, snapshot codecs, RNG ownership, generated contracts, UI rendering policy, simulator task contracts and existing optimisation code. Read the code and tests relevant to any change. Resolve runtime frame indices to biological IDs through an explicit generation-bound mapping.

Create the living plan under `.agent/PLANS.md` conventions. Map this work to applicable invariants, particularly `INV-002/003/004/007/008/009/011/012/014/015/016/017`, and specification sections on identity/time, clone/branch, topology transactions, durability/export, management, visualisation and scientific acceptance. Respect existing phase gates. Offline analysis and isolated reference experiments can proceed without enabling unfinished production migration paths.

Establish a task branch or isolated worktree while preserving unrelated changes. Run the relevant current baseline before attributing failures to new work. Implement additive schemas and narrowly scoped migrations. Do not change default production scheduling, learning, growth, numerical precision, checkpoint cadence, rendering policy or input/output routing as a side effect of installing the explorer.

## 6. Project, provenance and schema contracts

Define explicit versioned serialisable records with golden compatibility fixtures. Names below describe responsibilities; reuse authoritative existing types instead of defining competing ones.

| Record | Required content |
| --- | --- |
| Exploration project | Project ID/schema, source manifests, available backends, task suite, profiles, experiment index, result references and resource policy. |
| Source manifest | Dataset/specimen/version, original file digests, licence, biological scope, units, mapping, model/config/kernel versions, source commit/dirty state and import transformations. |
| Scientific topology | Complete neurons/synapses, stable ID mappings, chemical/electrical/modulatory class, signs, weights, delays, coordinates where known and uncertainty/provenance. |
| Trace manifest | Brain/run/checkpoint identity, coherent cut, topology/model/numeric profile, time units, sequence ranges, shard coverage, capture mode, gaps, filters, warm-up, segment hashes and completeness status. |
| Trace records | Spike, delivered synaptic influence, weight/delay/topology change, relevant field/input/output events, epoch changes and explicit gaps as supported by the backend. |
| GNAT analysis | Method/version, input hashes, thresholds/calibration, time window, coverage, component IDs, event edges, boundary flags and uncertainty. |
| Intervention | Source generation/hash, effective logical tag, selected stable IDs, intervention type, pending-event policy, adaptation policy, seed and proposed resource effect. |
| Experiment specification | Immutable baseline, candidate, task, initial state, interventions/fault schedule, seeds, backend, stop rule, budgets and required output artefacts. |
| Functional contract | Required tasks, metrics, accepted errors, trial distributions, operating conditions, learning/recovery requirements and protected I/O mappings. |
| Result | Status, measured metrics, evidence scope, hashes, errors, trace/checkpoint references, reproduction command and independent validity/resilience/cost verdicts. |
| Candidate export | Executable topology/state or cold-start definition, stable-to-dense mapping, removed/replaced IDs, model/numeric profile, provenance, required capabilities and validation references. |

Use integer ticks plus the canonical microstep type for authoritative time. Carry original source identifiers without loss; large FlyWire identifiers must not pass through JavaScript floating-point numbers. Represent such IDs as strings in browser DTOs. Browser display IDs must never authorise a mutation by vector index.

Support at least these ingestion routes:

- Native AARNN JSON snapshots through the canonical reader, preserving state and profile metadata.
- Verified stable-executor checkpoint bundles accompanied by the required topology/compiled-plan provenance.
- A documented sparse edge-list interchange with neuron metadata and explicit dynamical parameter assignments, suitable for Drosophila datasets and independently supplied activity.

Do not silently equate them. Distinguish `exact-resume`, `declared-cold-start` and `structural-only` capabilities. A snapshot lacking complete future events, RNG coordinates or model state cannot claim exact continuation. Provide a declared initialisation path when appropriate and preserve that distinction in every result.

Retain unknown parameters as unknown. Any default sign, delay, cell model or weight conversion must be an explicit, versioned modelling assumption. Keep dataset anatomy, model inference, simulated activity and candidate replacements distinguishable in the interface.

Use sparse adjacency/index structures and streaming readers. Never allocate an all-pairs dense matrix merely to load a whole fly connectome. Estimate memory and disk cost before allocation; if an existing legacy backend cannot execute that scale, select a supported canonical sparse backend or report the execution limitation without inventing a substitute model.

## 7. Complete activity capture without blocking the running engine

Implement opt-in tracing at genuine firing, synaptic delivery/transition and commit boundaries. `ShardExecutionResult` is an integration seam to inspect, not proof that every field required for scientific attribution is already present. Capture effective weights and delays at the model-defined use point, including their generation/version when plasticity changes them.

A spike record needs a unique stable event identity, neuron identity, canonical logical tag and provenance linking it to relevant committed transitions. A delivery record needs source spike identity, synapse/target identity, emission and arrival/effect tags when distinct, actual delivered efficacy, release outcome and generation. Preserve canonical event stages. An event being processed is not automatically a neuron firing.

Provide three modes:

1. **Off:** default for existing production workloads; avoid allocation/serialization/locking overhead from disabled capture.
2. **Live observation:** bounded, non-blocking batches to an independent consumer. If the observation queue fills, preserve simulation events and record lost observation sequences through a counter/manifest path that itself cannot disappear in the full queue. Mark affected intervals incomplete.
3. **Scientific recording/replay:** complete capture on an isolated branch, with bounded buffering and lossless persistence. Backpressure may slow that experiment, never the original brain. Use an existing sufficient authoritative log when possible; do not assume a WAL contains all biological spike/delivery information without checking.

A bounded live stream can never guarantee both unlimited complete capture and zero backpressure. Represent that trade-off explicitly. Incomplete streams support exploratory display; they cannot silently qualify a deletion or validate GNAT absence. Re-run the needed interval through complete isolated replay when its inputs/state are reproducible.

Write checksummed bounded segments plus a manifest/index, preferably with optional streaming compression. Validate sequence continuity per source/shard and verify consistent-cut provenance. Do not globally total-order unrelated brains for convenience. Finalise analysis only against declared coverage/frontiers. Keep filtered, sampled, truncated, warm-up and non-replayable input intervals visible.

Trace coverage must include the active pathway: legacy Runner, stable shard executor and any selected accelerator path. Reject an unsupported recording backend before a scientific run; never reconstruct allegedly exact spikes from decimated rendering data. Observer IDs must not consume the simulation RNG or perturb event allocation, scheduler decisions, state digests or committed effects.

## 8. GNAT construction and recurring subthreads

Implement a small transparent reference algorithm and an indexed implementation that agrees with it on bounded fixtures. For the published excitatory chemical case, estimate a presynaptic event's influence on a later postsynaptic event using the delivered weight, incoming-weight normalisation, conduction delay and exponentially decaying eligibility; retain edges exceeding a declared threshold. GNATs are weakly connected components of that directed event graph. Recurrent anatomy unfolds into a forward causal event graph. Recurrence analysis finds analogous subthreads, not necessarily exact graph isomorphisms. See Reference 1.

Make the implemented scoring definition explicit. A proposed baseline is:

```text
delta = postsynaptic_spike_tick - effective_arrival_tick
score = positive_effective_weight / incoming_weight_norm * exp(-delta / tau)
eligible only when delta >= 0 and the canonical source/effect ordering is valid
```

Document the chosen norm, units, zero-norm handling, numerical rounding and tie policy. For a fixed-weight reproduction profile, verify the chosen definition against the paper's equation and publish any differences. For dynamic release, plasticity, multiple compartments or parallel synapses, define a separately versioned effective-delivery method rather than claiming an unchanged reproduction of the original method. A score is an estimated influence, not a proof of necessary causation.

Implementation requirements:

- Use canonical synaptic arrival/effect tags; do not substitute packet arrival or a guessed Euclidean-distance delay.
- For equal biological ticks, use microsteps to establish causal order. Do not invent a physical duration for a microstep or put it into exponential decay.
- Match neuron pairs and real connections using adjacency and indexed recent spikes/deliveries. Do not compare every spike with every other spike.
- Derive a finite eligibility horizon from the threshold, decay and valid weight bound; enforce finite positive parameters. Record any additional truncation as an approximation.
- Preserve branching and reconvergence. A later edge can merge previously separate provisional components.
- Account for cross-window edges with overlap/state or external merging. Mark components open at recording boundaries; do not call an arbitrary chunk a complete GNAT.
- Use source/topology-scoped event identities and deterministic component IDs. Late/deduplicated input handling must be explicit.
- Calibrate thresholds on development data with suitable shuffled controls that preserve relevant rates/constraints. If the data do not support a clear calibration, record uncertainty instead of manufacturing a peak.
- Keep signed inhibition, electrical coupling and neuromodulatory context in the simulator and explanatory overlays. The baseline positive-edge GNAT graph must not turn their absence into evidence that they are dispensable.
- Expose tau, threshold and coverage sensitivity; keep these analysis settings independent of resilience settings.

For recurrence, implement a bounded small-data reference using the same-neuron spike-pair/edge-pair construction described in the paper, then a sparse indexed implementation. Protect against quadratic spike-pair products and unbounded motif enumeration. Budgeted large-data discovery may use fingerprints and candidate verification, but must report sampling, incompleteness and approximation. Hash equality alone is not proof of isomorphism. Do not equate similar GNAT topology with the same behavioural role.

Report motif occurrences, embedding contexts, timing variability, participating anatomical IDs and task associations. Cross-candidate motif comparison needs an explicit source-to-candidate mapping; ordinary same-neuron recurrence is not arbitrary graph matching between different circuits.

## 9. Isolated forks and intervention semantics

Fork from an immutable source into a new brain/experiment identity with independent mutable state, event streams, lease/output state, RNG policy and artefact directory. Immutable content may be shared read-only. Do not hard-link files that a worker might modify. Verify source digests before and after experiments.

Run workers in an explicit experiment mode that routes outputs to recorded or simulated effect sinks. Importing a configuration must not activate its live USB/AER, audio/HID/robot, federation, discovery or external actuator bindings. Do not give workers the source brain's management or actuator credentials. Existing simulated body adapters remain available through the declared task capability.

Restore the complete causal state where exact continuation is supported: neuron state, refractory state, synapse/release/plasticity state, fields, pending events, deduplication state, topology generation, random state and peripheral input provenance. Restoring only weights is a new initialisation, not an exact fork.

Implement these interventions with explicit backend capability checks:

| Intervention | Required semantics |
| --- | --- |
| Transient silencing | Prevent declared firing/output during a specified logical interval; document treatment of incoming integration and internal adaptation. |
| Neuron ablation | Remove or disable the neuron and its incident biological functions from an effective boundary, with an explicit pending-event policy. |
| Synapse ablation | Disable an identified synapse/class while retaining other incoming/outgoing connections. |
| Group/region ablation | Apply a stable-ID selection atomically; do not depend on current rendering selection indices. |
| Weight/delay perturbation | Apply bounded declared changes through the canonical model semantics. |
| Transmission/timing perturbation | Model biological transmission failure or jitter before the appropriate transition; preserve causal bounds. |
| Hardware-domain failure | Apply correlated loss to the model elements mapped to one selected physical failure domain. |

Keep biological transmission failure distinct from transport loss. A reliable AARNN transport must retry/fail according to its contract; a test must not violate `INV-007` by silently dropping committed causal events and call that biological resilience.

Every intervention records whether it is a transient mask, structural deletion, parameter change or physical fault model. Define the handling of events already in flight. Baseline behaviour before the intervention tag must remain unchanged. Topology-changing interventions must use a new generation and the canonical transaction/route/SCC rules. If a backend cannot perform an intervention safely mid-run, use a declared cold-start or supported safe boundary and identify the resulting scope.

Support deterministic random selections, degree/type/region selections, GNAT-informed selections and explicit user lists. Protect designated sensory/motor interfaces and known required structures through configurable constraints. Test individual and combined removals: two individually dispensable neurons can be jointly essential.

Keep **acute preservation** with frozen plasticity separate from **adaptive recovery** with a bounded permitted recovery period. Include sham interventions and intact controls. A silent reference output must not allow an empty candidate to pass through an unsuitable oracle.

For counterfactual comparisons, use paired initial conditions and environment seeds. Where canonical RNG is identity/counter based, keep corresponding surviving elements' random coordinates aligned. Do not let removal of one element shift a global RNG stream for every remaining element. Log any unavoidable stochastic divergence and use appropriately replicated comparisons.

Creating a new BrainId can legitimately change identity-bound raw digests and RNG coordinates. Preserve raw provenance and use a separately versioned identity-normalised comparison digest where appropriate. Reuse an existing supported explicit seed/replay policy to pair randomness; if that cannot provide paired draws, report distributional rather than exact replay agreement. Do not silently change canonical RNG semantics or rewrite identity fields inside an already sealed checkpoint to force a match.

## 10. Functional contracts and task execution

Implement a Rust task/oracle interface that separates simulation from evaluation. Each task declares its model inputs, initial-state distribution, logical duration/stop rule, environment, expected behavioural measurements, tolerances, seed schedule and required capabilities. Accept recorded input replay and closed-loop simulated environments as different experiment types.

Required task families:

- Stimulus discrimination and appropriate positive/negative motor response.
- Timing-dependent response, including latency and temporal ordering.
- A short memory task whose answer depends on stimulus history.
- Learning/reversal or adaptation where supported and explicitly selected.
- Closed-loop navigation or another existing embodied task through the shared embodiment/simulator adapters.
- Fault/recovery variants of the above under a declared resilience profile.

Implement working small authored tasks without external data. Provide adapter-driven task definitions for real connectomes, recorded biological measurements and existing supported simulators. Do not invent experimentally validated fly behaviour or a functioning simulator integration when the evidence is absent. A task with a required unavailable capability fails admission with a precise explanation; an optional capability may be not-run without being counted as passed.

Use per-task metrics and thresholds, not only an aggregate correlation. Include task success, false-positive/negative response, response latency, trajectory or action quality, state stability, memory retention, learning rate or recovery duration as relevant. Define behaviour when the reference score is zero or negative; support absolute task thresholds rather than dividing blindly by a reference score. Required tasks cannot trade away their minimum pass threshold through a weighted average.

Separate at least development/search, locked validation and optional biological challenge sets. Do not tune GNAT thresholds, candidate structure or training weights on the locked final validation set. If a supposedly locked set is reused for selection, reclassify it as development and require a fresh final assessment.

During closed-loop evaluation, freeze environment initialisation and exogenous randomness, not the intact fly's entire sensory sequence. The modified controller must receive the sensory consequences of its own actions. Compare policies in the same environment distribution; replaying the intact controller's observations only tests an open-loop response.

Preserving current output is distinct from preserving future learning. When learning is part of the contract, test new associations and retained performance after the intervention. Run against multiple plausible biological parameterisations when uncertainty in those parameters matters. Report agreement with the computational teacher separately from agreement with measured biology.

A physically reduced circuit is expected to have a different internal state digest from the intact circuit. Check deterministic replay within each model/profile separately, then compare task outputs through the functional oracle. Do not feed unequal internal states into the root module's exact-digest comparison and call the expected mismatch a failure of functional equivalence, or suppress that mismatch by relabelling the numerical profile.

## 11. Resilience, uncertainty and statistical evidence

Make these independent configuration dimensions:

| Dimension | Examples |
| --- | --- |
| Required function | Named tasks, operating range, error and latency thresholds |
| Fault population | Eligible neurons/synapses, cell classes, regions or hardware domains |
| Fault exposure | Random loss, targeted ablation, timing jitter, parameter variation, shared-domain failure |
| Recovery policy | Frozen/acute or permitted plasticity with a bounded recovery interval |
| Biological fidelity | Which neuron/synapse/field mechanisms the selected reference model includes |
| Analysis evidence | Trace coverage, GNAT threshold sensitivity and uncertainty |
| Validation evidence | Exhaustive cases or a declared statistical sampling/confidence procedure |
| Hardware assumptions | Numeric format, mapping, reliability model and measured operating limits |

Presets may be called `compact-silicon`, `resilient-silicon`, `biological-variability` and `adaptive-recovery`. They are editable named collections of explicit requirements. They must not silently select a different biological model, quantisation, decoder or functional tolerance. Store the expanded preset in each result, not merely its name.

Provide deterministic campaigns for random neuron loss, random synapse loss, selected high-impact ablations, cell-type/region failures, combinations and mapped hardware-domain failures. Separate biological faults from delivery/transport, process and machine faults. Quantisation and timing effects must be evaluated through a supported numerical/hardware model, not implemented by corrupting authoritative time.

The number/fraction removed, eligible population and exact selected IDs must be recorded. Define rounding for small populations, replacement policy and simultaneous versus sequential lesions. Targeted tests are required separately from random sampling; random robustness does not establish a worst-case guarantee.

Support exhaustive single-fault evaluation for bounded eligible populations and labelled random or targeted multi-fault sampling for larger cases. Exhaustive single-fault success proves only the declared single-fault scope. Do not claim a globally minimal connectome or immunity to all failures from a search heuristic.

For sampled binary task success, implement a tested one-sided confidence bound, preferably exact Clopper-Pearson, with declared independence assumptions and fixed trial budgets. A point estimate is insufficient when a profile requires a minimum success probability. At one-sided 95% confidence, zero failures in 299 independent trials is just enough to put the lower bound above 0.99 for one predeclared binomial claim; it is not evidence about every task/fault family. Include a fixture verifying the chosen convention.

Account for multiple required claims and selected candidates using a declared simultaneous-confidence procedure, for example a predeclared Bonferroni allocation. Report the family of claims, adjusted confidence, trial counts, failures and inconclusive results. Do not repeatedly stop early when an unadjusted estimate happens to pass. Infrastructure corruption or missing data makes a trial invalid and requires a recorded disposition; a valid trial with lost function is a failure, not a skip.

Treat confidence bounds, model uncertainty and trace completeness as separate kinds of evidence. If the budget cannot establish a required resilience level, return `inconclusive` or `failed` as appropriate. Do not round it into a pass.

## 12. Optimisation and physical circuit reduction

Search for candidates satisfying all hard constraints while reducing a measured cost vector. Retain a Pareto frontier rather than forcing every trade-off into one hidden score. Required cost components are active/executed neuron count, synapse count, mutable state bytes, event rate, memory footprint and latency. Include measured energy or hardware area only when a supported measurement/tool provides it; otherwise label the proxy and its uncertainty.

Implement a deterministic baseline optimiser first:

1. Revalidate the intact reference under the selected task/profile. If it fails the contract, report that fact before attempting to preserve an impossible baseline.
2. Derive candidate rankings from activity, GNAT/motif participation, connectivity, task association, prior interventions and estimated hardware cost. Treat these as hypotheses.
3. Propose bounded neuron/synapse/group removals, run fast development screening and keep provenance for every accepted/rejected proposal.
4. Test cumulative candidates, not only independent deletions against the intact graph. Retest after each accepted change.
5. Explore alternative deletion orders and targeted combinations within a declared budget so a greedy local result is not presented as a proven optimum.
6. Re-evaluate survivors on all required functional and resilience checks. Lock selection before final validation.
7. Lower selected candidates to physically smaller executable representations, then repeat validation on the actual exported/lowered model.

Inspect `src/ga.rs` and reuse suitable generic search utilities. Add bounded beam/population search only after the simple reference works. Seed proposal ordering and evaluation reproducibly; asynchronous job completion order must not silently alter a supposedly deterministic search. Record hardware, thread count and any tolerated numeric variability.

Implement both experimental masks and real reduction. A mask is useful for causal tests but does not prove reduced memory or silicon cost. Physical reduction must remove unnecessary storage and execution work, update sparse indexes and ownership/route plans, retain stable biological provenance, and preserve a source-to-candidate mapping. Measure the lowered model, not only the number of mask bits.

Make source biological identity distinct from candidate implementation identity. Multiple biological neurons may eventually map to one replacement element; preserve that many-to-one relationship rather than pretending they are still identical anatomical cells.

The required structural transformations are neuron/synapse removal and verified elimination of disconnected, inert subgraphs with no relevant field, electrical, growth or learning effects. Microcircuit replacement is a separate optional transformation. A suitable initial class, if the existing model supports it, is an isolated, fixed-weight, stateless relay chain whose dynamics satisfy an explicitly checked rule. A replacement must run the same AARNN primitives with validated total delay/efficacy semantics. If no safe rule can be established for a model, report that capability unavailable. Do not merge arbitrary co-active neurons or sum delays across nonlinear integration, inhibition, refractory or plasticity mechanisms.

Provide an extension contract for stateful motif replacement or distillation using existing training machinery where appropriate. Learning a replacement and fine-tuning after pruning must be explicitly labelled as training-assisted reduction, with separate training data, compute budget and final evaluation. General automatic distillation is not required to demonstrate the core end-to-end pruning workflow; do not ship a command claiming to perform it through a stub.

If no candidate passes, export a reproducible negative result and the intact reference frontier entry. A sound optimiser is allowed to find no acceptable reduction. It must not relax requirements to manufacture a smaller brain.

## 13. Hardware targets and executable exports

The required first target is an executable reduced AARNN CPU model. Use the reference model's existing supported numerical profile. Add a fixed-point or other silicon-oriented profile only through the project's canonical numeric implementation and an explicit conversion/validation step.

Define a hardware-target interface describing:

- Supported neuron/synapse/learning mechanisms, numeric ranges, rounding and saturation.
- Delay resolution/range, routing and fan-in/fan-out limits.
- State, model and event-storage costs, measured throughput/latency and failure domains.
- Available placement/compilation/calibration procedures and evidence sources.
- Required target-parity tests and unsupported-model rejection.

Integrate existing CPU/OpenCL/CUDA/FPAA capabilities only where genuinely available and applicable. Do not make a GPU, DGX, FPAA board, RDMA or fabrication tool a prerequisite for normal explorer operation. FPAA/ASIC/FPGA netlists, area, energy and physical reliability remain target-specific work; a hardware-neutral JSON netlist does not constitute a fabricated or validated chip.

An export bundle must contain:

1. A native loadable AARNN candidate or an explicitly supported candidate bundle with an implemented loader.
2. Its cold-start/exact-state classification and required model/numeric/backend capabilities.
3. Complete topology, parameter and state representation needed to execute it.
4. Biological-to-candidate ID mapping, retired IDs, replacements, protected interfaces and timing transformations.
5. The expanded functional/resilience contract and locked validation result references.
6. Cost measurements/proxies, measurement environment and baseline comparison.
7. Dataset/model/code/config/schema hashes, source licences and reproduction commands.

Test export followed by clean-process reimport and revalidation. Do not count an internal optimiser object as a usable deployment artefact. Never automatically promote a candidate over a live source brain. Integration with an existing deployment/import operation remains a separate user action under the current management contract.

## 14. Interactive explorer requirements

Provide a working browser application served from the Rust binary with no external CDN dependency. Reuse the current project's visual conventions and adaptive display policy where possible. Cache/bundle frontend dependencies with appropriate licences. Avoid introducing a second Node backend. JavaScript/WebGL should present data and interactions; Rust owns analysis, task execution and authoritative results.

Required views and interactions:

| View | Required interaction |
| --- | --- |
| Project/source | Open/import, inspect hashes and assumptions, show structural/executable/exact-resume capability. |
| Connectome | Pan/zoom, region/cell-type/ID search, select neurons/synapses, inspect signed weights/delays and provenance. |
| Activity | Time scrub/playback, spike raster, GNAT colouring, select a thread and inspect ancestry/descendants and recording boundaries. |
| Recurrent motifs | Inspect occurrences and contexts, timing differences, uncertainty and task associations. |
| Intervention | Build a mask or lesion, inspect affected stable IDs, choose acute/recovery policy, run against isolated controls and undo an uncommitted selection. |
| Comparison | Intact/candidate side-by-side, shared stimulus/environment definition, behavioural outputs and explicit differences. |
| Resilience | Edit independent fault, recovery, fidelity and evidence settings; inspect curves and failure cases. |
| Optimisation | Start/pause/cancel/resume, inspect Pareto candidates, see failed constraints and open corresponding traces. |
| Export/evidence | Validate a selected candidate, download the executable bundle and report, show measured versus estimated hardware costs. |
| Resource/status | Queue, CPU/memory/disk budgets, workers, trace completeness, unsupported capabilities and job errors. |

Use bounded server queries, temporal/region indexes and multiscale rendering for large graphs. Never ship the entire whole-brain activity graph to the browser for every frame. Display decimation must be labelled and must not alter underlying scientific topology or analysis. Preserve exact IDs as strings. Distinguish anatomy from procedural layout and measured geometry from inferred geometry.

Keep UI work responsive during analysis. All expensive operations are cancellable jobs with progress, not synchronous HTTP handlers. Provide keyboard operation and readable error/status messages. Build accessible tabular alternatives for selected cells, metrics and candidates.

Provide optional integration actions in existing native/web interfaces: export/open an exploration bundle or open a configured explorer session. Launch only the discovered installed binary using structured arguments. Do not embed unrestricted credentials in links, copy the explorer into another product tree, or turn a display selection into a live ablation command.

## 15. CLI, API and job lifecycle

Implement a documented CLI with these responsibilities. Exact option spelling below is the requested new contract; update the documentation and tests together if discovery requires a justified change.

| Command | Required behaviour |
| --- | --- |
| `doctor` | Check build/runtime features, data schemas, available execution backends, budgets and optional tools without mutating infrastructure. |
| `project init` | Create a new versioned local project without overwriting an existing one. |
| `import` | Import validated native/sparse data; report complete capabilities and assumptions. |
| `baseline` | Create reproducible reference runs for a task suite and source state. |
| `capture` | Record from an isolated source or explicitly configured read-only observation session. |
| `analyse` | Construct GNATs, recurrence and sensitivity results from declared complete or exploratory traces. |
| `experiment` | Run specified isolated interventions and controls. |
| `optimise` | Search under a declared profile with bounded cost and reproducible state. |
| `validate` | Run locked assessment of a fixed candidate and produce scoped verdicts. |
| `export` | Produce the verified executable/evidence bundle. |
| `serve` | Serve the optional interactive UI/API on loopback by default. |
| `demo` | Run the authored offline end-to-end scenario through real AARNN execution. |
| `worker` | Internal bounded execution protocol; not an unauthenticated remote shell. |

Support `--help`, machine-readable JSON summaries and stable exit codes. Distinguish invalid input, unsupported capability, budget exhaustion, cancellation, failed acceptance and inconclusive evidence. Diagnostic messages must explain what can be done with the available source.

Provide a versioned job API, for example `/api/explorer/v1`, for source metadata, paged topology/activity queries, trace analyses, experiment submission, optimisation, candidate comparison, validation, export and event/progress streaming. Generate clients/contracts through the repository's existing schema workflow rather than hand-maintaining incompatible copies.

Use states such as `queued`, `running`, `pausing`, `paused`, `succeeded`, `failed`, `cancelled` and `interrupted`, with monotonic event cursors, idempotent submission, bounded concurrency and a persisted transition journal. Keep job success separate from scientific validation pass: a successfully completed experiment may demonstrate failure of the candidate.

On cancellation, stop new admission, terminate or safely checkpoint child work, flush evidence, release resource allocations and leave an explicit resumable/non-resumable state. Kill the worker process group after a configured shutdown deadline. Do not leave orphan simulations, Slurm jobs or GPU allocations. A restarted controller must identify interrupted jobs and reconcile them; it must not pretend that they succeeded or automatically duplicate live work.

For authenticated runtime observation, use existing management authorisation and read permissions. Do not bypass the orchestrator or modify source workspaces. For local UI, use an appropriate short-lived local session mechanism, origin/CSRF protections, loopback binding and bounded request bodies; do not expose an unauthenticated remote experiment endpoint. Remote access must reuse supported authenticated transport rather than adding a new public listener by default.

## 16. Storage and reproducible resumption

Use a project directory containing immutable source/candidate/trace/result objects and a crash-safe small project/job index. Reuse existing atomic publication/checksum utilities where their contract fits. A lightweight SQLite index or equivalent may be appropriate; do not require an external database service for offline operation.

Write new objects to private temporary paths, validate them and atomically publish manifests. Protect against partial writes, concurrent publications, path traversal, oversized/decompression-bomb inputs and stale project versions. Keep indexes reconstructible from manifests where practical. Do not place multi-gigabyte datasets, generated traces, model binaries or experiment output in Git.

Record optimiser population/frontier, evaluated proposal hashes, deterministic ordering, RNG coordinates and source/profile/task hashes for resumption. Resume only when relevant provenance matches. Record a new run when semantics or inputs change. No shared mutable baseline arrays or inherited source leases are allowed.

Retain failure evidence: intervention, task, fault schedule, seed, last valid state, error, trace excerpts and reproduction command. Keep retention bounded by project policy. Deletion must respect referenced immutable objects and active jobs; normal cancellation is not permission to purge source data.

Expose source, analysis and result manifests in a human-readable report plus machine-readable JSON. Scientific unknowns and unsupported capabilities must remain machine-readable, not only free-text footnotes.

## 17. Resource isolation, concurrency and optional cluster execution

The default local profile must use a small bounded compute budget, no accelerator and one experiment worker. Use dedicated Rayon pools or equivalent bounded execution; do not initialise an unrestricted global pool. Bound Tokio blocking work and child-process thread counts. Account for BLAS/OpenMP and nested simulator parallelism, not only Rust worker threads.

Use one resource ledger across the controller's analysis jobs and child experiments. Reserve memory, disk and compute before admission. Include indexes, decompression, topology, trace windows, simulator state, candidate copies, worker outputs and GUI/API buffers in the estimate. Spill/stream when supported; reject work before allocation if it cannot fit. In particular, do not clone a whole-brain working state for every trial without a resource check.

On Linux, support an optional cgroup/systemd execution adapter for CPU, memory, process and I/O limits and optionally CPU affinity. Detect whether limits are actually enforceable. Report `enforced`, `advisory` or `unavailable`; CLI flags alone are not OS isolation. Do not require privileged installation or reconfigure existing production services to run a demo. Platforms lacking an enforcement capability must report that limitation and retain internal admission limits.

A worker process can crash or exceed its budget without corrupting the source, other experiment results or the UI. Test cancellation, out-of-memory disposition, timeout, partial result publication and restart. Use checkpoint-aware cancellation for supported experiments, with process termination as a bounded fallback. GPU use is opt-in with explicit device/memory admission; no default competition for the engine's accelerator.

Provide an optional Slurm launcher using the same worker manifest, binary protocol and result bundle. Include dry-run job rendering and submission/reconciliation/cancellation code, with tests using an injected command runner. Use structured arguments, explicit CPU/memory/time requests, private project directories and a native worker build matching the allocated architecture. Do not assume an x86 binary runs on Centriq ARM64 or that remote DGX machines are always available. Actual cluster submission occurs only when the user selects that launcher; generating the software must not submit a job.

Benchmark the main engine with observation off, with bounded observation, and with concurrent explorer analysis/experiments. Record throughput, p50/p95/p99 step or event latency, CPU/RSS, disk/network traffic, dropped observation sequences and engine event/state digests. Use controlled workloads, warm-up and repeated measurements. State the hardware and noise limits. Make an interference budget configurable; do not claim zero impact from separate threads/processes or declare a production performance gate passed from an uncontrolled CI timing sample.

## 18. Drosophila-specific workflow

Implement a generic connectome workflow with Drosophila source adapters/task templates as an initial biological application. Discover existing imports/assets first; do not copy the historical robot-agent pack into a second authoritative implementation.

Required stages:

1. Identify the dataset precisely: adult/larval, specimen/sex, source/version, brain-only versus brain-and-nerve-cord coverage, licence and file digests. Do not merge FAFB/FlyWire/BANC variants under one unversioned name.
2. Import neuron and connection tables with original stable IDs, neurotransmitter/type metadata, synapse counts and measured geometry where available. Record inferred signs and all weight conversions separately from direct measurements.
3. Supply a declared neuron/synapse/delay/plasticity model. Do not infer complete dynamics from the wiring diagram alone. If a parameter is unknown, run a declared assumption or ensemble and report it.
4. Bind sensory inputs, motor/descending outputs and the simulated body's interfaces through existing embodiment mappings. State which missing nervous-system components or body dynamics are supplied by the model.
5. Establish intact computational and, where available, biological reference tasks. Suitable task templates include visual motion response, approach/avoidance, feeding/grooming responses, navigation and associative learning; templates are not claims that all these behaviours are already validated.
6. Capture activity across the relevant conditions and histories; mine GNATs and formulate intervention hypotheses.
7. Search and compare compact versus resilient candidates under the same functional contract. Assess performance in changed conditions and after selected faults.
8. Export and re-run the physically reduced implementation on the selected CPU/numerical target, and report exactly which functions and conditions have been preserved.

Not all fly signalling is necessarily represented by spike events. If the reference contains graded/non-spiking cells, continuous electrical effects or relevant neuromodulation, preserve their contributions in simulation and assess them through appropriate signals/interventions. A spike-only GNAT view does not establish that those elements are irrelevant.

Develop a documented data adapter with a tiny legally redistributable authored fixture using the same schema as a larger user-provided dataset. Large real data are external inputs with explicit acquisition instructions and checksums. Do not require credentials, a cloud subscription or a large download for software acceptance. Do not invent a percentage by which the whole fly brain can be compressed.

The software may exclude biological development, metabolism or regeneration from a silicon target's contract only through an explicit scope decision. If self-repair, lifelong adaptation or resilience is required, those effects need corresponding tests; they cannot be removed merely because silicon is assumed reliable.

## 19. Configuration and runnable workflow to deliver

Implement a validated configuration schema. The following TOML is a requested example contract to create at `configs/explorer/default.toml`, not an existing configuration. Validate unknown fields, ranges, units, resource consistency and capability compatibility. Do not accept unused settings silently.

```toml
schema_version = 1
name = "synthetic-compact-silicon"

[execution]
backend = "source"
numeric_profile = "inherit"
plasticity_policy = "frozen"
seed = 20261125

[resources]
total_compute_threads = 2
analysis_threads = 1
max_worker_processes = 1
threads_per_worker = 1
memory_limit_mib = 4096
disk_budget_mib = 8192
worker_wall_timeout_seconds = 300
gpu_enabled = false
launcher = "local"

[capture]
mode = "scientific-isolated"
require_complete = true
max_segment_mib = 64
compression = "zstd"

[gnat]
method = "effective-delivery-v1"
tau_ms = 5.0
threshold_mode = "development-calibrated"
minimum_score = 0.01
require_generation_mapping = true
recurrence_mode = "bounded-verified"
max_motif_candidates = 100000

[functional]
task_suite = "examples/explorer/tasks/synthetic_sensorimotor.toml"
minimum_reference_score_ratio = 0.99
maximum_additional_latency_ms = 1.0
require_all_tasks = true
development_split = "development"
validation_split = "locked-validation"
learning_required = false

[resilience]
preset = "compact-silicon"
eligible_population = "all-internal-neurons"
random_neuron_loss_fraction = 0.0
random_synapse_loss_fraction = 0.0
targeted_single_faults_required = false
minimum_success_probability = 0.99
confidence = 0.95
confidence_scope = "simultaneous-required-claims"
trials_per_sampled_claim = 1000
recovery_policy = "none"
recovery_window_ms = 0.0

[optimisation]
method = "deterministic-beam"
proposal_budget = 200
beam_width = 4
allow_neuron_removal = true
allow_synapse_removal = true
allow_verified_relay_replacement = false
training_assisted_reduction = false
protect_input_output_mapping = true
cost_dimensions = ["state_bytes", "synapses", "neurons", "latency_ms"]

[hardware]
target = "aarnn-cpu-native"
cost_mode = "measured-when-supported"

[ui]
listen = "127.0.0.1:0"
maximum_visible_nodes = 20000
maximum_visible_edges = 50000
```

These numerical values are illustrative software defaults for an authored scenario, not biological constants or a claim of industrial reliability. The project records the expanded, task-specific settings actually used. A resilience preset with non-zero loss requires appropriately configured faults, additional trials and task-specific admissibility; merely renaming the preset must not create evidence.

Provide a second example profile retaining the same functional task contract but requesting loss of one eligible internal element and targeted/exhaustive bounded single-fault tests. Demonstrate why a compact candidate may fail that profile. If a profile requests probability/confidence that its trial budget cannot establish, show an explicit insufficient-evidence result.

Deliver the following post-implementation commands as tested documentation. They are not commands to execute before creating the package. Use the repository toolchain and record any required native development packages in `doctor` output.

```bash
# Build the new binary with its primary interactive interface.
cargo build --locked -p aarnn-explorer --features web-ui

# Run a complete authored, offline, reproducible pipeline.
cargo run --locked -p aarnn-explorer -- demo --project target/explorer-demo

# Open the saved project in the browser, using an allocated loopback port.
cargo run --locked -p aarnn-explorer --features web-ui -- serve --project target/explorer-demo --open

# Inspect available capabilities and resource enforcement.
cargo run --locked -p aarnn-explorer -- doctor --project target/explorer-demo --json

# Run the integrated scenario through the project's existing QA orchestration.
cargo xtask examples run --id EX-EXPLORER-001 --product host
cargo xtask qa run --suite explorer
```

The demo must use the real import, trace, analysis, intervention, task, search, validation, export and reimport paths. Do not use a separate toy simulator or precomputed successful result. It creates an immutable source, a complete trace, GNAT results, lesion controls, at least one genuinely reduced passing candidate, a failed damaging candidate, a resilience comparison, a native executable export and a report. Its fixtures must make those outcomes true under actual AARNN semantics.

For explicit staged operation, implement commands equivalent to:

```bash
cargo run --locked -p aarnn-explorer -- baseline --project target/explorer-demo --split development --label intact-development --reuse-compatible
cargo run --locked -p aarnn-explorer -- analyse --project target/explorer-demo --run intact-development
cargo run --locked -p aarnn-explorer -- experiment --project target/explorer-demo --plan examples/explorer/interventions/single_ablation.json
cargo run --locked -p aarnn-explorer -- optimise --project target/explorer-demo --config configs/explorer/default.toml --select min-state-bytes
cargo run --locked -p aarnn-explorer -- validate --project target/explorer-demo --candidate selected --split locked-validation
cargo run --locked -p aarnn-explorer -- export --project target/explorer-demo --candidate selected --output target/explorer-candidate
```

`selected` is a project-local alias resolving to a recorded immutable candidate hash, not a mutable graph. Every command must print the resolved hash. Selection is explicit and deterministic among eligible development candidates; validation does not silently replace the selected candidate with whichever one happened to pass. `--reuse-compatible` reuses a completed baseline only when all relevant immutable input and configuration hashes match; it must not overwrite incompatible work. Export either includes the matching valid assessment or marks the candidate exploratory and unvalidated, as requested.

Also document a real-data walkthrough using `project init`, `import --format aarnn-snapshot` and the sparse Drosophila interchange adapter. Supply input-format examples and explain the required task/parameter/body mapping. Do not put shell redirection placeholders such as `<candidate-id>` into claimed runnable examples. Provide a small smoke profile for CI and label its evidence as a software smoke test rather than an assessment at the larger profile's confidence level.

## 20. Implement in these verifiable milestones

Every milestone must leave working code, targeted tests, updated documentation and an updated ExecPlan. Continue into the next milestone once the current gate passes; do not ask for confirmation between routine local steps.

| Milestone | Implement | Gate |
| --- | --- | --- |
| M0: discovery | Current repository map, requirements traceability, feature/build baseline, active ExecPlan and preservation of unrelated work | Baseline evidence and authoritative integration paths recorded. |
| M1: runnable package | Workspace member/binary, doctor, config/schema validation, immutable project/source store and tiny import | Headless package builds; malformed input and overwrite attempts fail correctly. |
| M2: canonical capture | Root observer seams, full scientific topology mapping, isolated complete recording, bounded live observation with gap accounting | Capture does not alter committed state/events; display truncation cannot masquerade as completeness. |
| M3: activity analysis | Transparent and indexed GNAT algorithms, recurrence, coverage/sensitivity and indexed queries | Both algorithms agree on adversarial small fixtures; window boundaries and same-tick order are correct. |
| M4: isolated experiments | Exact/cold-start classification, bounded workers, lesion semantics, controls, cancellation and persistence | Source remains unchanged; sham replay matches; effective-boundary and pending-event tests pass. |
| M5: functions/resilience | Rust task oracles, acute/adaptive policies, fault campaigns, confidence evidence and independent verdicts | Known good, damaged, fragile and inconclusive cases are classified correctly. |
| M6: reduction | Reproducible search, Pareto frontier, masks plus physical sparse lowering, checked structural-removal rules | At least one authored model is genuinely smaller and preserves required behaviour; harmful reductions fail. |
| M7: export | Native executable bundle, hardware profile/cost reporting, mapping and evidence manifests | Clean-process reimport/revalidation reproduces declared results. |
| M8: interactive product | Working graph/raster/motif/lesion/comparison/resilience/frontier views and async job API | Browser flow exercises real backend work and remains responsive during cancellation/load. |
| M9: integration/scale | Existing UI entry points, QA catalogue, IDE configurations, streaming/load budgets and optional Slurm adapter | Offline end-to-end example, headless/UI lanes and bounded stress/failure tests pass. |
| M10: hand-off | Operation/method/data/hardware documentation, reproducible reports, compatibility/rollback notes and final review | Every required software capability has executable evidence; external biological/hardware evidence is separately labelled. |

Do not expose a future production capability through an unfinished earlier phase. You may complete offline exploration and reference experiments while keeping live/distributed integration behind the applicable gate. Retain explicit unsupported states, not successful stubs. New command surfaces must exercise their implementation in tests.

## 21. Acceptance tests and evidence

Add meaningful tests for the new scientific and process boundaries. Reuse the existing QA runner and deterministic reference fixtures. Avoid tests that merely repeat private implementation details.

| Test group | Required cases |
| --- | --- |
| EXPLORER-IMPORT | Native and sparse import; stable IDs larger than JavaScript's safe integer range; unsupported schema; missing units/dynamics; truncated topology; incomplete runtime state; sparse allocation bounds; source round-trip. |
| EXPLORER-TRACE | Off/on state/event digest parity; plastic weight/delay provenance; same-tick microsteps; exact sequence gaps; full observation buffer; checksum corruption; duplicate/reordered events; distributed consistent-cut mismatch; replay limitations. |
| EXPLORER-GNAT | Chain, branch, reconvergence, recurrent neuron firing, independent overlapping threads, false temporal coincidence, disconnected input, weak/zero influence, threshold ties, dynamic weights, signed context, window-crossing components and unknown prehistory. |
| EXPLORER-RECURRENCE | Reused subthreads with timing variation; unrelated same-shaped motifs; positive and negative matches; fingerprint collision verification; bounded second-order construction; approximation flags. |
| EXPLORER-FORK | Unmodified replay under the declared identity/RNG comparison policy; fresh brain identity; independent mutable state; original source checksum unchanged; cold-start distinction; missing RNG/queues; stale-generation intervention rejection. |
| EXPLORER-LESION | Effective-tag boundary; pre-existing in-flight events; transient versus permanent effects; synapse/class/group removal; individually harmless but jointly harmful removal; inhibition and protected I/O; malformed selection. |
| EXPLORER-TASK | Positive and negative behaviour; empty/quiescent candidate rejection; temporal response; memory; learning when required; frozen versus adaptive recovery; closed-loop feedback; unsupported required simulator. |
| EXPLORER-RESILIENCE | Deterministic eligible selections; small-population rounding; random versus targeted faults; exhaustive bounded single faults; correlated hardware fault; acute/adaptive distinction; exact confidence golden cases; multiple claims; insufficient samples. |
| EXPLORER-OPTIMISE | Reproducible search; cumulative revalidation; budget stop/resume; no qualifying candidate; minimum protected functions; Pareto dominance; development/validation separation; physical reduction versus mask-only reporting. |
| EXPLORER-EXPORT | Clean reimport and execution; mapping completeness; retained delay/plasticity semantics; unsupported target rejection; measured versus proxy cost; failed/inconclusive validation cannot become an unqualified deployment claim. |
| EXPLORER-JOBS | Bounded pools/queues; cancellation and child process cleanup; timeout/OOM; crash during publish; controller restart/resume; duplicate submission; advisory versus enforced limits; optional Slurm submit/status/cancel via fakes. |
| EXPLORER-UI | Real backend import/analysis/lesion/compare/export; exact IDs; visible uncertainty/gaps; paginated large graphs; responsive controls; local session/origin rejection; no physical actuation. |
| EXPLORER-REGRESSION | Existing snapshot/checkpoint compatibility; existing product feature lanes; capture disabled by default; no new workspace dependency cycle; unchanged committed outputs with observation. |

Provide at least these authored fixtures:

1. A tiny trace with hand-checkable GNAT membership, branching and recurrence.
2. A real AARNN-executed redundant sensorimotor circuit where one pathway can be removed without losing required function, but removing all alternatives fails.
3. A circuit whose inhibitory component is essential despite not being a positive-edge GNAT component.
4. A memory/delay circuit where naïve chain collapsing or pruning destroys required behaviour.
5. A fault profile where the smaller accepted functional candidate loses resilience relative to a larger candidate.

Register `EX-EXPLORER-001` and related QA IDs through the existing catalogue. Extend `tools/xtask` without changing the semantics of existing suites. Existing tests that enumerate catalogue entries will need intentional updates rather than being disabled.

Discover the actual feature matrix before fixing final commands. At minimum, provide and run the equivalents of:

```bash
cargo fmt --all --check
cargo test --locked -p aarnn-explorer --no-default-features
cargo test --locked -p aarnn-explorer --features web-ui
cargo clippy --locked -p aarnn-explorer --all-targets --features web-ui -- -D warnings
cargo test --locked -p aarnn-explorer --doc
cargo xtask examples run --id EX-EXPLORER-001 --product host
cargo xtask qa run --suite explorer
```

Run the affected root/executor/checkpoint suites and the existing relevant product lanes as well. Do not assume `--workspace --all-features` is a valid cross-platform configuration. Record exact commands, toolchain, feature sets and failures. A missing capability is `not-run` or an admission failure, never a passing result.

Add ARM64 and x86_64 compile/test lanes using existing self-hosted CI conventions where available; avoid requiring a large IDE host. Add browser automation using the project's supported tooling, with real visible-state verification and retained evidence. Run external simulator, accelerator and physical hardware lanes only on appropriate infrastructure; do not claim them from fakes.

Benchmark bounded streaming analysis and capture overhead separately from correctness. Large-scale reports must state neuron/synapse/spike counts, peak RSS, disk cost, elapsed time and approximate/complete analysis coverage. The small fixture proves software operation, not whole-fly scale or biological equivalence.

## 22. Documentation, IDE setup and final hand-off

Create:

- `docs/connectome-explorer/README.md`: installation, build, doctor, quick start and normal operation.
- `architecture.md`: dependency/process boundaries, coherent source capture and model reuse.
- `methods.md`: actual implemented GNAT scoring, recurrence, approximation and intervention semantics.
- `data-formats.md`: schemas, units, coverage, ID mapping, import/export and version compatibility.
- `functional-validation.md`: task contracts, data splits, confidence and biological claim boundaries.
- `resilience.md`: preset expansion, fault semantics, acute/adaptive recovery and evidence scope.
- `optimisation-and-hardware.md`: physical lowering, protected functions, target constraints and measured/proxy costs.
- `drosophila.md`: real-data acquisition/import/parameter/body/task workflow with honest prerequisites.
- `operations.md`: resource budgets, restart/cancel, storage retention, optional Slurm and troubleshooting.
- `validation-report.md`: exact final software checks, scale/performance evidence and explicit external not-run items.

Provide checked-in, team-safe JetBrains run configurations for `Explorer - Doctor`, `Explorer - Demo`, `Explorer - UI`, `Explorer - Tests` and `Explorer - QA`. Use workspace-relative paths and repository tooling. Keep secrets, personal absolute paths, hostnames and dataset credentials out of project configuration. Provide equivalent command-line instructions for Claude Code and other IDEs; no proprietary IDE API should be required.

Provide a short agent continuation document or the living ExecPlan containing completed milestones, current safe state, next actions, exact reproduction commands and known blockers. Do not put working status only in chat. Re-read only changed or relevant files when resuming.

Before the final hand-off, inspect the actual diff for accidental default changes, source-state mutation, duplicate semantics, unbounded data paths, stale generated schemas and fabricated validation claims. Run required checks after final changes. Explain any pre-existing failures separately with evidence, and complete all independent authorised work.

The final coding-agent response must state what is runnable, the exact demo/UI commands, changed integration points, tests actually run, how to inspect the exported candidate, measured limitations and any external data/hardware work that remains. Do not claim that the biological fly has been completely reproduced or compressed unless the corresponding scientific evidence really exists.

## 23. References and source baseline

These references explain the starting concepts. The application architecture, schemas, commands, resource controls, experiment design and acceptance gates in this brief are proposed implementation requirements.

1. Theilman, Wang, Rothganger and Aimone, *Decomposing spiking neural networks with Graphical Neural Activity Threads* (2023): https://arxiv.org/abs/2306.16684 and full text https://arxiv.org/html/2306.16684v1. Check the actual definition and extraction method before claiming reproduction; cite any implementation differences.
2. AARNN repository baseline: https://github.com/neuralmimicry/aarnn_rust/tree/f767570ff20ebd32918d71a71fbc9ed2cb3e8be2. The user's current checkout is authoritative for implementation discovery.
3. AARNN normative specification: `docs/specifications/distributed-whole-brain-emulator-v1.1.md`, particularly identity/time, clone/branch, topology transactions, immutable checkpoints/consistent exports, management and scientific acceptance. Read the current file in the checkout.
4. AARNN implementation rules: current `AGENTS.md` and `.agent/PLANS.md` in that checkout.
5. Shiu et al., *A Drosophila computational brain model reveals sensorimotor processing*, Nature (2024): https://doi.org/10.1038/s41586-024-07763-9; abstract https://pubmed.ncbi.nlm.nih.gov/39358519/. Relevant biological reference work, not evidence that this proposed reduction already works.
6. Lappalainen et al., *Connectome-constrained networks predict neural activity across the fly visual system*, Nature (2024): https://doi.org/10.1038/s41586-024-07939-3; abstract https://pubmed.ncbi.nlm.nih.gov/39261740/.
7. Pospisil et al., *The fly connectome reveals a path to the effectome*, Nature (2024): https://doi.org/10.1038/s41586-024-07982-0; abstract https://pubmed.ncbi.nlm.nih.gov/39358526/.
8. Cargo binary/workspace target behaviour: https://doc.rust-lang.org/cargo/reference/cargo-targets.html.
9. Linux cgroup resource control: https://docs.kernel.org/admin-guide/cgroup-v2.html. Actual enforcement depends on the host configuration and privileges.

Do not copy third-party implementation code without checking its licence and attribution requirements. The software's useful result is a reproducible, inspectable relationship between a source connectome, declared functions, controlled interventions, a reduced implementation and scoped validation evidence.
