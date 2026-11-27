# Architecture

## Package and dependency direction

```text
aarnn_rust (root library: Runner, NetworkConfig, ...)
   ^
   |  path dependency, features parallel + growth3d + morpho
crates/aarnn-explorer (library + binary `aarnn-explorer`)
```

The root crate does not depend on the explorer. No root source file was changed to install the explorer: lesions are applied to snapshot JSON, and transient silencing uses the runner's public weight matrices. The workspace gained one member (`Cargo.toml` `members`), and `default-members` is unchanged.

| Module | Responsibility |
|---|---|
| `store` | Project directory, content-addressed immutable objects, atomic publication, index, aliases, job journal |
| `config` | Validated TOML profile (unknown fields rejected, ranges, resource consistency) |
| `model` | Scientific topology from native snapshots and the sparse interchange; sparse-to-native compiler |
| `exec` | The only code that steps `aarnn_rust::runner::Runner`; deterministic encoder; event recording; silencing |
| `tasks` | Task suites, trial distributions, measures, oracles, closed-loop heading plant |
| `capture` | Trace segments and manifests, verification, bounded live observer |
| `gnat`, `motifs` | Reference + indexed GNAT construction, calibration, sensitivity, depth; analogous subthreads |
| `lesion` | Interventions on isolated copies, physical lowering, inert-cell rule, cost model |
| `worker` | Child-process protocol, environment and thread bounds, deadlines, OS enforcement |
| `pipeline` | Controller commands: import, baseline, analyse, experiment, validate, export, verify, demo |
| `optimise` | Deterministic beam search, Pareto frontier, resilience campaigns |
| `stats` | Exact one-sided Clopper-Pearson bounds, Bonferroni |
| `web` (feature `web-ui`) | Loopback UI and `/api/explorer/v1` job API |
| `slurm`, `jobs`, `doctor` | Optional cluster launcher, journal reconciliation, capability report |

## Processes

- **Controller.** The CLI or the UI server schedules work and analyses results. It never steps a network itself.
- **Workers.** Every experiment, including baselines, captures, lesions, candidate evaluations and bundle verification, runs in a child process: `aarnn-explorer worker --job job.json --out result.json`.
- **Per-worker guarantees.** Each worker:
  - builds its own `Runner` from an immutable project object and checks that object's sha256 before and after the run;
  - starts from a cleared environment, keeping only PATH, HOME, LANG, LD_LIBRARY_PATH and the XDG/DBus session needed by `systemd-run`, so it inherits no credentials or peripheral, AER, robot or federation settings;
  - is bounded by `RAYON_NUM_THREADS = NM_SIM_PAR_MAX_WORKERS = threads_per_worker`, `OPENBLAS_NUM_THREADS = OMP_NUM_THREADS = 1` and `NM_DISABLE_OPENCL = 1`;
  - runs in its own process group, which is killed at the wall deadline or on cancellation;
  - runs under `systemd-run --user --scope` with MemoryMax, CPUQuota and TasksMax when available (reported `enforced`), and otherwise under internal admission limits only (`advisory`).
- **Concurrency.** At most `max_worker_processes` workers run at once. Results are collected in submission order, so search decisions do not depend on completion order.
- **Idempotence.** A job id is the hash of its full specification, and a completed job is reused rather than re-run.

## Model reuse and execution protocol

`exec::run_with` follows the embodiment client's protocol:

1. import the network JSON into `Runner::new(.., NeuronModel::Aarnn, Learning::Aarnn)` and seed `runner.rng`;
2. take 2 ms frames, each a phase-accumulator encoding of sensor values in [0, 1];
3. call `step_sync(1.0, frame)`, then a zero frame;
4. read the outputs as graded activation, or spikes for spiking cells.

Closed-loop tasks compute each frame's sensors from the previous frame's outputs.

## Capability table

| Capability | State | Notes |
|---|---|---|
| Native single-layer snapshots (designed circuits, imported connectomes) | supported | `declared-cold-start` |
| Multi-layer snapshots | unsupported | structural import refused with a precise message |
| Sparse interchange, structural | supported | `structural-only` |
| Sparse interchange with declared dynamics | supported | compiled to a native snapshot; `declared-cold-start` |
| Exact resume of a running brain | unsupported | the legacy runner does not persist every transient (graded output phase, STP release history, neuromodulator EMAs) |
| Trace capture, legacy Runner | supported | complete; spikes, plus graded spike-equivalents when enabled |
| Trace capture, `StableShardExecutor` / accelerators | unsupported | no observer seam yet; rejected before a scientific run |
| Live observation | supported (library) | bounded queue with gap accounting |
| Closed loop, authored heading plant | supported | synthetic task environment |
| Closed loop, MuJoCo bodies | external | through `run_sim.sh` / `aarnn-mujoco` (see drosophila.md) |
| GPU / OpenCL / CUDA / FPAA | not admitted | the explorer never competes for accelerators |
| Slurm launcher | optional | render/submit/status/cancel; submission only on explicit invocation |
