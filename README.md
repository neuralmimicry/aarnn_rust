# Neuromorphic Autonomous AI Platform

This repository integrates the existing neuromorphic build and container workflow with an operator-driven autonomous AI platform.

## Included capabilities

- Existing multi-architecture container build based on the project `Containerfile`
- Local build script that reuses CI-built architecture images for manifest completion
- GitHub Actions workflow for immutable multi-arch builds and promotion
- Kubernetes operator-style control plane for `Model`, `Experiment`, and `Dataset`
- Argo Rollouts canary delivery and Istio traffic management
- Shadow deployment, canary promotion, and blue/green targeting
- A/B testing, multi-armed bandit selection, and reinforcement-learning style routing
- Training pipeline, inference service, GitOps manifests, and observability artefacts

## Quick start

### Build locally

```bash
scripts/build_container.sh ghcr.io/neuralmimicry/aarnn_rust brainregions false
```

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

## Shared token billing via nmchain

The web UI can participate in the shared private token ledger used by the NeuralMimicry portal and Refiner.

Set these environment variables before running `src/bin/web_ui.rs`:

- `NMCHAIN_API_BASE=http://nmchain-host:9080`
- `NMCHAIN_API_TOKEN=<aarnn-app-token>`
- `NMCHAIN_APP_ID=aarnn`
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
