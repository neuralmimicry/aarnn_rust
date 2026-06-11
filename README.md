# Neuromorphic Autonomous AI Platform

This repository integrates the existing neuromorphic build and container workflow with an operator-driven autonomous AI platform.

## Included capabilities

- Existing multi-architecture container build based on the project `Containerfile`
- Local build script that reuses CI-built architecture images for manifest completion
- GitHub Actions workflow for immutable multi-arch builds and promotion
- Optional FPAA startup autodetection for AARNN kernels, with Pi.HAT GPIO/SPI and USB probe support
- Kubernetes operator-style control plane for `Model`, `Experiment`, and `Dataset`
- Argo Rollouts canary delivery and Istio traffic management
- Shadow deployment, canary promotion, and blue/green targeting
- A/B testing, multi-armed bandit selection, and reinforcement-learning style routing
- Training pipeline, inference service, GitOps manifests, and observability artefacts

## Quick start

### Build locally

```bash
scripts/build_container.sh ghcr.io/neuralmimicry/aarnn_rust main
```

This builds and pushes the native-architecture workload images from the same source tree. The script now prepares and reuses workload-specific Debian packages under `.container-cache/` and feeds those packages into the container build instead of compiling Rust inside the image:

- `engine-standalone`
- `engine-orchestrator`
- `engine-node`
- `engine-web-ui`
- `engine-desktop-ui`

Pass `false` as the third argument if you want a local-only build without pushing.

## Release workflow

GitHub Actions binary release automation lives in `.github/workflows/build-and-release.yml`.

- `Cargo.toml` is the release version source of truth.
- official GitHub releases require a matching `vX.Y.Z` tag.
- `scripts/package-release.sh --version <cargo-version> --output-dir ./dist` builds the release tarball and checksum manifest.
- `scripts/package-release.sh --version <cargo-version> --output-dir ./dist --platform linux-x86_64 --deb-arch amd64` also emits a Debian package for Linux.
- manual `workflow_dispatch` runs can package artifacts from any ref.
- publish steps only run from a `v*` tag ref, either automatically on tag push or manually from `workflow_dispatch`.

The binary release workflow packages `aarnn_rust`, `web_ui`, and the base runtime config. Linux CI now validates `.deb` artifacts on both `amd64` and `arm64` runners. The existing `.github/workflows/container-build.yml` pipeline remains the multi-arch container promotion path for richer runtime images.

### Train a sample model artefact

```bash
scripts/train.sh
```

### Run the local inference API

```bash
python -m uvicorn ml.inference.server:app --reload --host 0.0.0.0 --port 8000
```

### Run an experiment decision locally

```bash
python experiments/ab/ab_test.py
python experiments/bandits/bandit.py
python experiments/rl/rl_router.py
```

## FPAA offload startup support

The Rust runtime can now probe for an attached FPAA at startup and expose per-kernel
offload routing controls for the AARNN path.

What it does:

- probes for a Pi.HAT-style FPAA through GPIO/SPI, defaulting to `/dev/spidev0.0`
- probes for USB-connected FPAA endpoints through `/dev/ttyUSB*`, `/dev/ttyACM*`, and `/dev/serial/by-id`
- verifies whether expected AARNN kernel images are present by checking the Okika manifest, `.ahf` export, and `fpaa/runtime_state.json`
- runs host-side sample tests for the supported FPAA-realizable AARNN kernels
- computes requested vs effective routing so unverified kernels fall back to software automatically
- exposes the same controls in the CLI and the native egui UI

Main CLI entry points:

```bash
# Print detection and verification status, then exit
cargo run --bin aarnn_rust -- --fpaa-status-only

# Prefer Pi.HAT probing and request FPAA for two kernels
cargo run --bin aarnn_rust -- \
  --fpaa-transport pihat \
  --fpaa-route synaptic_filter=fpaa \
  --fpaa-route stp=fpaa

# Require hardware and use a USB hint while probing
cargo run --bin aarnn_rust -- \
  --fpaa-mode required \
  --fpaa-transport usb \
  --fpaa-usb-hint okika \
  --fpaa-print-status
```

In the egui application, the controls are under `AARNN -> Biological Realism -> FPAA Offload`.

Current limit:

- startup detection, verification, and routing selection are implemented
- the numerical AARNN kernels still execute in Rust unless a real hardware data-path is added
- if hardware is missing, not ready, or not verified, the effective route is forced back to software

The generated Xcos and Okika collateral lives under `fpaa/`. Start with `fpaa/README.md`.

## Shared token billing via nmchain

The web UI can participate in the shared private token ledger used by the NeuralMimicry portal and Refiner.

Set these environment variables before running `src/bin/web_ui.rs`:

- `NMCHAIN_API_BASE=http://nmchain-host:9080`
- `NMCHAIN_API_TOKEN=<aarnn-app-token>`
- `NMCHAIN_APP_ID=aarnn`
- Optional shared auth/session and billing overrides when identity and token routes are split across services:
  - `AARNN_CENTRAL_AUTH_API_BASE=http://customers-host:5010`
  - `AARNN_BILLING_API_BASE=http://billing-host:5020`
  - `AARNN_BILLING_TIMEOUT_SECS=10`
  - If `AARNN_BILLING_API_BASE` is unset, AARNN falls back to `AARNN_CENTRAL_AUTH_API_BASE` for backward compatibility.
- Optional debit schedule overrides:
  - `NM_AARNN_TOKEN_CREATE_COST` (default `25`)
  - `NM_AARNN_TOKEN_IMPORT_COST` (default `25`)
  - `NM_AARNN_TOKEN_START_COST` (default `5`)
  - `NM_AARNN_TOKEN_REPEAT_COST` (default `2`)
  - `NM_AARNN_TOKEN_STEP_COST` (default `1`)

When enabled, the web UI:

- mirrors successful local/OIDC logins to `nmchain`
- exposes `GET /api/tokens` and `GET /api/tokens/ledger`
- debits shared user tokens for workspace create/import/start/repeat/step operations

For backend-to-backend calls, prefer Customers-issued service-account bearer tokens. AARNN honours their explicit `service_access.aarnn` grants and does not apply the human authenticated fallback when the principal is a service account.

### Gail mirrored LLM bridge

The web UI now exposes `POST /api/llm/mirror` for Gail's mirrored LLM I/O bridge.

- Present the Gail Customers-issued bearer token when calling the route from Gail or another backend service.
- The route requires `service_access.aarnn: use`.
- Each accepted mirrored exchange is written under `<runtime_root>/llm_mirror/<conversation_id>/`.
- When `network_id` is supplied, AARNN translates the mirrored exchange into an AER batch and stimulates the selected network.
- Candidate replies are currently a deliberately low-confidence bootstrap echo. Keep Gail on `llm_preferred` until decoded network-output replies are ready.


## Multi-Network Deployment Modes

The runtime and distributed engine now carry an explicit deployment intent in
`NetworkConfig.deployment`. This lets the same network or network set be marked as:

- `individual`
- `distributed`
- `sharded`
- `grouped`
- `combined`
- `federated`

These modes can be selected from the CLI and are persisted through config JSON,
snapshot JSON, runtime workspaces, and orchestrator startup payloads.

Useful CLI flags:

```bash
--execution-mode individual
--execution-mode distributed,sharded
--execution-mode grouped,combined --execution-combined-group ensemble-a
--execution-mode grouped,federated --execution-federation-group tenant-a
--execution-related-network vision --execution-related-network motor
--execution-scope cluster
--execution-live-transition
--execution-autonomous-transition
--execution-transition-mode individual,sharded,combined,federated
--execution-target-step-ms 8
--execution-transition-cooldown-ms 5000
--execution-allow-multi-user
--execution-max-concurrent-networks 8
--execution-desired-shards 4
--execution-autodetect=true
--infrastructure-root /home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible
```

Scheduling behaviour:

- `individual` keeps a network on one engine node.
- `sharded` allows the existing layer-partitioning rebalance logic to split it across nodes.
- `combined` prefers co-location with related networks.
- `federated` prefers separation from related networks when capacity allows.
- `grouped` means multiple related networks can be carried and scheduled concurrently.
- `node`, `container`, and `system` scopes pin execution to one engine target even when a sharded mode is requested.
- `cluster` and `federated_cluster` scopes allow cross-node shard placement, and `--execution-desired-shards` caps shard fan-out.
- `--execution-max-concurrent-networks` limits how many active networks a target should host before the rebalancer prefers other capacity.
- `--execution-live-transition` lets the orchestrator move a running network between isolated, combined, distributed, sharded, and federated placements without stopping it.
- `--execution-autonomous-transition` lets the orchestrator switch between those permitted modes based on runtime step latency, node saturation, and related-network pressure.
- `--execution-transition-mode` constrains the autonomous controller to the mode permutations you allow for that network.
- Autonomous transitions and deployment-only manual mode changes now best-effort refresh the latest live snapshot from the current primary shard before hot reassignment, reducing stale-state drift during seamless moves.
- Manual live deployment changes are rejected unless the running deployment or the requested deployment explicitly grants `deployment.transition_policy.allow_live_transition=true`.
- Nodes that leave a live distribution now receive `UNLOAD_NETWORK`, so old shards are retired instead of lingering after a mode transition.
- Cluster status APIs and the desktop cluster dashboard now expose each network's deployment modes, scope, live/autonomous transition flags, and the last transition source/reason/timestamp.
- When a local Tracey agent is reachable, node capacity scoring and autonomous transitions also factor in external CPU, memory, network, disk, and GPU pressure from `GET /status`.

Optional Tracey integration knobs:

```bash
NM_TRACEY_STATUS_URL=http://127.0.0.1:48000/status
NM_TRACEY_STATUS_TIMEOUT_MS=80
NM_TRACEY_STATUS_CACHE_TTL_MS=1000
NM_TRACEY_STATUS_FAILURE_BACKOFF_MS=2000
```

If no explicit deployment modes are set, the distributed engine keeps its existing
backward-compatible behaviour and shards across worker nodes when orchestrated.

Infrastructure autodetection:

- `aarnn_rust` and `web_ui` will scan `NM_INFRASTRUCTURE_ROOT` or, by default,
  `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible` when it exists.
- The detector looks for SwarmHPC/Continuum tenant signals such as Kubernetes,
  DaemonSet worker mode, Continuum autoscaling, orchestrator service naming, and
  runtime root hints.
- In node mode, this can pre-resolve the orchestrator address from the tenant
  configuration instead of relying only on UDP discovery.
- In `web_ui`, the same detector can fill in the default orchestrator address and
  runtime root when they are not provided explicitly.

### Deploy manifests

```bash
scripts/deploy.sh developer
```

## Optional HPC modes

### OpenMP (native C++ helpers/controllers)

OpenMP is auto-detected for Webots/native C++ example builds.

```bash
# auto-detect OpenMP support (default)
make -C examples nao_nn_controller

# force-enable / force-disable
make -C examples nao_nn_controller ENABLE_OPENMP=1
make -C examples nao_nn_controller ENABLE_OPENMP=0
```

### OpenMPI (distributed bootstrap for `aarnn_rust`)

Build with OpenMPI support and launch with `mpirun`; rank 0 becomes orchestrator
and other ranks become worker nodes automatically when role flags are omitted.

```bash
cargo build --release --features openmpi
mpirun -np 3 target/release/aarnn_rust --brain-id cluster --grpc-addr 0.0.0.0:50051
```

Useful env overrides:
- `NM_MPI_ORCHESTRATOR_ADDR=http://host:50051` to force broadcast address.
- `NM_MPI_ADVERTISE_ADDR=<ip-or-hostname>` to control rank-0 advertised host.
- `NM_MPI_TRANSPORT=0` to disable MPI spike transport (keep gRPC transports only).
- `NM_OPENMP_AUTO=0` to disable automatic OpenMP runtime env tuning.

When `openmpi` is enabled, spike exchange can use three paths:
- persistent gRPC stream
- burst gRPC stream
- MPI point-to-point

The runtime keeps per-peer latency EWMAs and failure streaks and auto-selects the
lowest-latency healthy transport, with automatic fallback on errors.
If MPI reports only `Single`/`Funneled` threading support, MPI transport is
auto-disabled for safety and gRPC transports remain active.

## Webots C. elegans Runtime

This repo now includes generated Webots assets driven by `network_celegans.json`:

- `webots_world/protos/CelegansRobot.proto`
- `webots_world/worlds/celegans_neuroworld.wbt`
- `webots_world/configs/config_celegans_webots.json`

Regenerate assets at any time:

```bash
python3 scripts/build_webots_celegans_assets.py
```

Run end-to-end (backend import + Webots auto-connect) by interface:

```bash
# CLI backend mode (UDS server runtime)
scripts/run_celegans_cli_webots.sh

# Native Rust UI mode
scripts/run_celegans_rust_ui_webots.sh

# Web UI mode (starts backend + web_ui server)
scripts/run_celegans_web_ui_webots.sh
```

Useful env overrides:
- `NETWORK_FILE=/abs/path/network_celegans.json`
- `CONFIG_FILE=/abs/path/config_celegans_webots.json`
- `WORLD_FILE=/abs/path/celegans_neuroworld.wbt`
- `ORCHESTRATOR_PORT=50051` (used by Rust UI/Web UI wrappers)
- `WEB_UI_LISTEN=0.0.0.0:8080` (web UI wrapper only)
- IPC tuning (controller async bridge):
  `NM_UDS_RECV_TIMEOUT_MS`, `NM_IPC_TIMEOUT_GRACE_MS`,
  `NM_IPC_TIMEOUT_LOG_INTERVAL_MS`, `NM_IPC_UDS_CTRL_BUF_BYTES`,
  `NM_IPC_WINDOW_MIN`, `NM_IPC_WINDOW_INIT`, `NM_IPC_WINDOW_MAX`,
  `NM_IPC_SEND_BUDGET_MAX`.

## Webots Drosophila Runtime

This repo also includes a Drosophila (fruit fly) connectome pipeline and Webots assets:

- `scripts/build_drosophila_network_json.py`
- `network_drosophila.json`
- `webots_world/protos/DrosophilaRobot.proto`
- `webots_world/worlds/drosophila_neuroworld.wbt`
- `webots_world/configs/config_drosophila_webots.json`

Regenerate the projected Drosophila network from BANC v626:

```bash
python3 scripts/build_drosophila_network_json.py \
  --neurons "data/drosophila/BANC v626/neurons.csv.gz" \
  --connections "data/drosophila/BANC v626/connections_princeton.csv.gz" \
  --output network_drosophila.json \
  --max-sensory 34 \
  --max-hidden 1024 \
  --max-output 48
```

Regenerate Drosophila Webots assets for the current network:

```bash
python3 scripts/build_webots_drosophila_assets.py --network network_drosophila.json
```

Run end-to-end by interface:

```bash
# CLI backend mode (UDS server runtime)
scripts/run_drosophila_cli_webots.sh

# Native Rust UI mode
scripts/run_drosophila_rust_ui_webots.sh

# Web UI mode (starts backend + web_ui server)
scripts/run_drosophila_web_ui_webots.sh
```

Useful env overrides:
- `DROSOPHILA_REBUILD_NETWORK=1` to force rebuilding `network_drosophila.json`.
- `DROSOPHILA_MAX_SENSORY`, `DROSOPHILA_MAX_HIDDEN`, `DROSOPHILA_MAX_OUTPUT`.
- `DROSOPHILA_MIN_SYN_COUNT` and `DROSOPHILA_WEIGHT_TRANSFORM`.
- IPC tuning (controller async bridge):
  `NM_UDS_RECV_TIMEOUT_MS`, `NM_IPC_TIMEOUT_GRACE_MS`,
  `NM_IPC_TIMEOUT_LOG_INTERVAL_MS`, `NM_IPC_UDS_CTRL_BUF_BYTES`,
  `NM_IPC_WINDOW_MIN`, `NM_IPC_WINDOW_INIT`, `NM_IPC_WINDOW_MAX`,
  `NM_IPC_SEND_BUDGET_MAX`.

## Repository map

- `scripts/` operational entrypoints
- `operator/` CRDs and reconciliation logic
- `ml/` training and inference code
- `experiments/` decision engines for rollout selection
- `pipelines/` Kubeflow and Argo workflow definitions
- `k8s/` rollouts and traffic policy
- `gitops/` Argo CD application
- `monitoring/` Prometheus queries and Grafana dashboard
- `docs/` architecture and operations documentation

## Notes

The operator and experiment code is intentionally lightweight and dependency-minimal so it is runnable as example code. Production deployments would normally wrap the controllers in a controller-runtime framework or a long-running service inside the cluster.
