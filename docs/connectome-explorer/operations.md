# Operations

## Resource budgets

`[resources]` in the profile controls the compute used by a project.

**Admission limits.** The default local profile allows one worker process with one thread, a 4 GiB memory budget and a 300 s wall deadline per worker. Workers get bounded threads through `RAYON_NUM_THREADS`, `NM_SIM_PAR_MAX_WORKERS`, `OPENBLAS_NUM_THREADS` and `OMP_NUM_THREADS`. `analysis_threads` bounds controller-side analysis.

**OS enforcement.**
- `doctor` reports `enforced` when `systemd-run --user --scope` works on the host. Each worker then runs in a scope with:
  - MemoryMax = budget / workers;
  - CPUQuota = 100 % × threads;
  - TasksMax 64;
  - MemorySwapMax 0.
- Otherwise enforcement is `advisory`, with internal admission limits only.
- `AARNN_EXPLORER_NO_OS_LIMITS=1` turns OS enforcement off, for example in CI containers without a user systemd.

**Accelerators.** GPUs are never used. The explorer does not compete with the engine for an accelerator.

## Cancellation and restart

- **Cancellation.** The UI cancels a job through `POST /api/explorer/v1/jobs/{id}/cancel`; on the CLI, interrupt the process. On cancellation:
  1. new workers are not admitted;
  2. running workers' process groups get SIGTERM, then SIGKILL after 2 s;
  3. completed job results are kept.
- **Resume.** Job IDs are content hashes of their full specification, so re-running a command reuses every completed job: resume by re-running.
- **Journal.** `jobs.jsonl` records every transition, and its line number is a monotonic cursor. `aarnn-explorer jobs --project P --reconcile` marks a job whose last state is `running` as `interrupted`, after a controller crash. It is never marked succeeded and never resubmitted automatically.
- **Failed workers.** A worker that times out, is killed by its memory limit (signal 9) or crashes gets a recorded disposition, plus a stderr tail (bounded to 1 MiB read, 2000 characters kept). The controller reports an infrastructure error, which is not counted as a failed trial.

## Storage

- **Objects.** Objects are immutable. They are written under `tmp/`, fsynced, verified and renamed into place. Publishing the same content twice is idempotent.
- **Index.** `index.json` is updated under an exclusive lock file; a lock older than 60 s is treated as stale.
- **Size guard.** Objects over 1 GiB are refused on read.
- **Retention.** Nothing is deleted automatically, and cancellation never purges data. Delete a project directory only when no job is running against it.
- **Git.** Generated projects (`target/explorer-*`) stay out of Git.

## Optional Slurm launcher

```bash
aarnn-explorer slurm render --job P/runs/<job>/job.json --cpus 2 --mem-mib 4096 --time 00:30:00 --partition arm64
aarnn-explorer slurm submit --job P/runs/<job>/job.json --dry-run   # prints the sbatch command only
aarnn-explorer slurm submit --job P/runs/<job>/job.json --cpus 2 --mem-mib 4096 \
  --worker-exe /opt/aarnn-explorer/bin/aarnn-explorer --sbatch-arg=--nodelist=qc02
aarnn-explorer slurm status --id 4242
aarnn-explorer slurm cancel --id 4242
```

How the launcher behaves:

- **Protocol:** it uses the same worker job and result protocol as local workers.
- **Validation:** commands are structured argument lists, never shell strings. Job IDs and times are validated, and each `--sbatch-arg` must be a single `--option[=value]`.
- **Architecture:** the worker binary must be built for the nodes' architecture; an x86_64 build does not run on ARM64 nodes. `--worker-exe` names the binary on the compute nodes, and `--sbatch-arg=--nodelist=...` (or `--exclude=...`) keeps a job on nodes of that architecture.
- **Paths:** the job file, the snapshot it names and the job directory must exist at the same paths on the compute node, on shared storage or as staged copies. The result is written next to the job file.
- **Submission:** a job is submitted only by an explicit `submit` without `--dry-run`.

## Troubleshooting

| Symptom | Cause / action |
|---|---|
| `unsupported capability: built without the web-ui feature` | rebuild with `--features web-ui` |
| `unsupported capability: N hidden layers` | multi-layer snapshots are structural-only in this version |
| `conflict: result L exists with different inputs` | choose a new label; records are immutable |
| worker outcome `timeout` | raise `worker_wall_timeout_seconds` or reduce the suite |
| `doctor` shows `advisory` | the user systemd session is unavailable; limits are internal only |
| evaluations slow | use `--release`; a debug build of the runner is about 10x slower |
