# Architecture

## End-to-end flow

1. Code and model changes are committed.
2. CI builds immutable architecture-specific images.
3. A multi-architecture manifest is assembled and published.
4. Argo CD syncs Kubernetes manifests.
5. Argo Rollouts and Istio shift traffic.
6. Experiment controllers evaluate candidate variants.
7. Autonomous controllers trigger retraining and redeploy if metrics degrade.

## Main control loops

### Delivery loop

`CI build -> registry -> GitOps sync -> rollout -> traffic validation -> promotion`

### Autonomous improvement loop

`metrics -> anomaly detection -> training pipeline -> new model -> rollout`

## Core directories

- `operator/` custom resources and control logic
- `ml/` training and serving
- `experiments/` promotion logic
- `k8s/` runtime manifests
- `monitoring/` operational insight
