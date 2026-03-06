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
scripts/build_container.sh ghcr.io/neuralmimicry/neuromorphic_demo brainregions false
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
