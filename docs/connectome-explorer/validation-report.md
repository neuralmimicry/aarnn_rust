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
| VX-001 Drosophila, tier 1 | GNAT candidates 143 → 139 hidden; depth unchanged; foraging 4/12 (A) and 6/12 (B) against 4/12 for the neuromodulated original (p 0.68) and 1/12 for the non-neuromodulated fly | drosophila.md, section 3 |

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
| Real Slurm submission | not-run by design; fake-runner tests only |
| Real Drosophila connectome data | external dataset; see drosophila.md. Whole-brain execution exceeds the dense legacy backend |
| FPAA / ASIC / FPGA targets | not provided; hardware-specific tools needed |
| Biological validation | not claimed |
