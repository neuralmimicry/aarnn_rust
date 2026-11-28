# Validation report

Host: x86_64 Linux, 16 cores, Rust 1.92.0 (pinned). Branch `connectome-explorer`, dated 2026-11-27. Software checks prove the mechanisms; they are not biological or hardware validation.

## Checks run

| Command | Result |
|---|---|
| `cargo test --locked --release -p aarnn-explorer` | **pass** (exit 0; rerun at 06:48Z on 27 Nov after the `derive`, `weight-set` and declared-protection changes, in a separate target directory; 7 min 14 s build, 8 min of tests): lib 32 (adds `weight_set_puts_plastic_synapses_at_the_cap`); demo_contract 4 (17 s); explorer_e2e 1 (454 s, the demo from scratch); explorer_units 7; gen_fly_suite 2 and gen_salamander_suite 2 (each: the naive contract and the learned-state contract are current); write_fixture 1; 4 development probes ignored |
| `cargo test --locked --release -p aarnn-explorer --features web-ui --lib` | **pass**: 34 (adds the session/CSRF/origin and loopback-only tests) |
| `cargo test --locked --release -p xtask` | **pass**: the catalogue test lists EX-EXPLORER-001 |
| `cargo xtask examples list` | EX-EXPLORER-001 listed |
| `cargo fmt --all --check` | clean |
| Browser UI | All ten views verified against the real backend, by a CDP-driven headless Chrome session on a copy of the demo project. Checks: session cookie, CSP, no console errors, CSRF/origin rejection, jobs submitted and cancelled from the UI, exact GNAT highlighting, search, inspector, export. This was a manual lane, not a catalogued automated test. |

### Coverage of the brief's test groups

| Group | Covered by |
|---|---|
| EXPLORER-IMPORT | large IDs, unsupported schema, float IDs, unknown fields, truncated matrix, budget, structural-only, round trip |
| EXPLORER-TRACE | output parity with observation on and off, exact replay, multi-segment continuity, corruption detection, full live queue with gap accounting |
| EXPLORER-GNAT | chain, branch, reconvergence, overlap, false coincidence, early arrival, weak/zero influence, ties, inhibition in the norm, stage microsteps, window boundaries, calibration, reference = indexed on 40 adversarial fixtures |
| EXPLORER-RECURRENCE | reused subthread with jitter, same shape on other neurons, budget overflow flag, fingerprint collision separated |
| EXPLORER-FORK / LESION | source unchanged; sham = intact; redundant vs jointly essential; inhibition essential; protected I/O; malformed selection; transient silencing; `weight-set` at the plasticity cap (plastic mask required, negative values refused) |
| EXPLORER-TASK | positive and negative tasks, empty/quiescent rejection, temporal latency criterion, memory, closed-loop plant |
| EXPLORER-RESILIENCE | Clopper-Pearson golden cases, verdict classes, Bonferroni, exhaustive single faults, insufficient evidence (inconclusive) |
| EXPLORER-OPTIMISE | Pareto dominance, reproducible beam (sorted selection), cumulative revalidation, physical reduction measured, compact vs resilient |
| EXPLORER-EXPORT | clean-process reimport and revalidation reproducing the validation record |
| EXPLORER-JOBS | timeout kill, crash disposition, memory-limit kill (systemd scope), cancellation, Slurm render/submit/status/cancel through a fake runner |
| EXPLORER-UI | web unit tests, plus the manual browser lane above |
| EXPLORER-REGRESSION | no root source change; capture off by default; cut-mode builds byte-identical; workspace fmt clean |

## Demonstration results (`aarnn-explorer demo`, authored circuit)

| Outcome | Result |
|---|---|
| Intact reference | passes all 6 tasks |
| Sham / source integrity | sham = intact; source hash unchanged |
| Damaging lesions | G1a+G1b fails go; I fails nogo; memory region fails memory-hit; silencing M2 for 80-120 ms fails memory-hit; inhibition x2 fails nogo (rebound) |
| Compact candidate | 16 → 9 hidden cells, state bytes 8576 → 3984 (−54 %); locked validation: behavioural preservation passed |
| Resilience comparison | compact under resilient profile: single-fault claim failed; resilient candidate (16 → 13) passes it, while its sampled claim is inconclusive (20/20 cannot reach 0.99) |
| Export | bundle `validated`; a clean process reproduces the validation's per-task means |

## Experiment results (designed networks, simulated)

| Study | Result | Details |
|---|---|---|
| G2 salamander crawler | learned-state reduction 96 → 94 hidden (two redundant touch relays); 30 s walk unchanged (0.84 m against 0.85 m net) | g2-salamander.md |
| VX-001 Drosophila, tier 1 | GNAT candidates 143 → 139 hidden; depth unchanged. Foraging over 4 runs each (48 releases): A 18, B 17, the neuromodulated original 17 (p ≥ 0.79) and the non-neuromodulated fly 10 (p ≈ 0.17 against the original). Function is preserved, not improved | drosophila.md, section 3 |

## Slurm and cross-architecture reproducibility (swarmhpc cluster, 27 Nov 2026)

Commit `3fc8b1d`. `aarnn-explorer` was built as Slurm jobs on qc00 (aarch64, Ubuntu 24.04, 7 min) and sm00 (x86_64, Ubuntu 26.04, 5 min). Four stored VX-001 worker jobs were then submitted with the explorer's own `slurm submit`, each pinned with `--sbatch-arg=--nodelist=...` and given `--worker-exe`. Each job ran once on an aarch64 node (qc02, qc03) and once on an x86_64 node (sm00, sm01), as 8 Slurm jobs, 4 CPUs each.

| Job | Leaves compared | qc (aarch64) | sm (x86_64) | Wall time on a node |
|---|---|---|---|---|
| intact evaluation, 11-task learned contract | 2 723 | identical | identical | 88-102 s |
| lowered candidate (2 cells removed) | 2 877 | identical | identical | 88-102 s |
| weight intervention | 5 619 | identical | identical | 93-103 s |
| full trace capture (touch-left, 4 seeds) | 188 898 | identical | identical | 8-9 s |

Leaves are the result JSON with the timing fields removed. Every result is also identical to the result recorded on the development host when the job first ran, and to a rerun there with the same commit. AARNN execution through the explorer is bit-reproducible across these architectures, OS releases and hosts. The jobs share no filesystem: each job directory was staged with an absolute snapshot path.

## Multi-node, multi-network runtime on the cluster (28 Nov 2026)

Harness: `target/qa/cluster/cluster_test.py`; report: `target/qa/cluster/report.json`.

**Build.** Production release `0.1.43~dev.373.b9647b7`, as deployed to the native workers the same morning.

**Isolation.** The test cluster shared the production hosts but nothing else:
- its own orchestrator on sm00:51151 (brain `cluster-test`), with the discovery beacon disabled;
- workers on port 51152, each given the orchestrator explicitly;
- a per-run management token, kept in 0600 files;
- the management endpoint off (`NM_MANAGEMENT_ENABLED=0`), as in production templates without TLS;
- workers placed only on nodes Gail training cannot use, so Gail job 4503 ran throughout.

**Topology.**
- Native workers: sm01 (x86_64) and qc02 (aarch64).
- Slurm workers: qc00 and qc04 (aarch64, `sbatch` jobs).

**Networks.** Four ran at once: the neuromodulated fly, candidate B, the reduced salamander, and a synthetic 5-layer network that asked for 4 shards.

| Phase | Result |
|---|---|
| Membership | all four workers joined in 10 s |
| Placement | every network placed at once. The synthetic network was sharded across a native and a Slurm worker (sm01: layers 0, 2, 3; qc00: layers 1, 4), with hot backups on qc04 and qc02. Each single-layer network ran on sm01 with a hot backup on qc02 |
| Failover | cancelling qc00's Slurm job moved its layers within 20 s: qc02 was promoted from backup (layers 1, 2, 4) and sm01 kept 0 and 3 |
| Rejoin | the resubmitted Slurm worker rejoined in 10 s |
| Production unchanged | the production `aarnn-node` services on all four hosts had the same PIDs before and after; nothing was left behind |

Defects found:

1. **Single-layer networks have no sensory or output stage owner.**
   - On both the active worker (sm01) and the backup (qc02), `GetNetworkActivity` for the flies and the salamander reports `output_stage_assigned: false` and no sensory target layer.
   - The networks step, but they receive no input and emit no output. `/api/activity` answers 503 "no available active worker owns output-source layer 0".
   - The multi-layer synthetic network is unaffected.
   - Cause: the hierarchical planner made a lone eligible worker the backup of its own layers (layers [0], redundant [0]). Workers take `layers - redundant` as their active set, so the worker owned nothing. The assignment was then preserved after more workers joined.
   - Reproduced locally with an orchestrator and two workers.
   - Fixed in the working tree (uncommitted), with a unit test. With a runtime built from the fixed tree, the same local reproduction gives layers [0] with redundant [], `Neurons=143`, `output_stage_assigned: true`, sensory target layer 0 and output spikes; `/api/activity` returns 200.
   - The production shared brain may be affected: Gail's comparative validation scores AARNN's output at quality 0.0, and the orchestrator showed native workers with `Neurons=0`. This could not be checked directly, because the production web UI needs credentials.
2. **Distributed processes did not exit on SIGTERM.**
   - `main` idled in a sleep loop after start-up, so each process lingered until SIGKILL.
   - Fixed in the working tree (uncommitted; see the ExecPlan). The test teardown escalates to SIGKILL for the deployed release.
3. **Harness lessons** (fixed in the harness):
   - `cd DIR && cmd &` backgrounds the `cd`, so PID files went astray;
   - the orchestrator needs `NM_MANAGEMENT_BEARER_TOKEN`, and management requires TLS unless disabled;
   - `distribution` in `/api/status` is a list of `{node_id, layers, backup_layers}`.

## Scale and performance (this host, release build)

- Authored circuit (6 sensory, 16 hidden, 4 outputs): one suite evaluation (6 tasks × 6 seeds) takes about 4 s, dominated by the 1.6 s closed-loop task. Full demo from scratch: 13.5 min, with one worker.
- Designed Drosophila circuit (428 / 143 / 48): one evaluation of the 8-condition contract × 4 seeds is tens of seconds per worker.
- Capture overhead: the trace parity test proves observation does not change outputs. A controlled throughput benchmark (observation off, bounded and concurrent) was **not run**; see below.

## Not run, with reasons

| Item | Status |
|---|---|
| `cargo clippy -p aarnn-explorer -- -D warnings` | not-run: clippy is not in the pinned toolchain's components (`rust-toolchain.toml`); needs `rustup component add clippy` |
| ARM64 compile/test lane | not-run on this host; CI runner needed |
| Engine interference benchmark (p50/p95/p99 with and without observation, concurrent explorer load) | not-run; the trace seam is on the isolated worker path, so the production engine is never observed. Measuring interference needs a controlled engine workload |
| Live capture from `StableShardExecutor` / accelerators | unsupported (no observer seam); reported by `doctor` |
| Real Drosophila connectome data | external dataset; see drosophila.md. Whole-brain execution exceeds the dense legacy backend |
| FPAA / ASIC / FPGA targets | not provided; hardware-specific tools needed |
| Biological validation | not claimed |
