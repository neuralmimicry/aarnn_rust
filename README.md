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
