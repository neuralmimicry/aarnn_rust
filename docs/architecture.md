# Architecture

## End-to-end flow

1. Code and model changes are committed.
2. CI builds immutable architecture-specific images.
3. A multi-architecture manifest is assembled and published.
4. Argo CD syncs Kubernetes manifests.
5. Argo Rollouts and Istio shift traffic.
6. Experiment controllers evaluate candidate variants.
7. Autonomous controllers trigger retraining and redeploy if metrics degrade.

## Hybrid distributed runtime

The distributed runtime uses worker-initiated membership. Kubernetes engine pods
and host-native workers join the same singleton orchestrator over gRPC, and the
orchestrator exposes their combined state to the web and Rust UIs. Configured
orchestrator endpoints are ordered and de-duplicated; UDP broadcast, loopback,
and optional unicast discovery provide fallback. Workers retain their candidate
set and reconnect continuously after connect, join, or heartbeat failures.

An explicitly advertised address separates the listener bind address from the
address peers must dial. This is required for wildcard listeners and protects
membership from an address being inferred incorrectly through container or k3s
NAT. Because spike batches flow directly between assigned peers, Kubernetes
workers also need LAN-routable advertised endpoints when native workers have no
route to the pod overlay. Until membership and scheduling state are externalised, multiple
orchestrator replicas are intentionally unsupported because they form independent
cluster views.

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
