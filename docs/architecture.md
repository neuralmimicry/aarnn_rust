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

## Causal migration boundary

The repository now contains additive reference contracts for the distributed
emulator: `deterministic`, `causal`, `topology_model`, `data_plane`,
`field_events`, `multi_brain`, `durability`, `management` and `peripheral`.
They provide stable IDs, superdense tags, component/SCC planning, bounded
causal and field-event streams, fenced storage, default-deny operations and
governed peripheral admission. The legacy
layer/vector runtime remains the default until a generation-boundary migration
has generated and validated the production adapters.

Mobile products use the same shared Rust semantics through the additive
`mobile_runtime` contract. Its host evidence covers explicit execution modes,
checkpoint-before-background lifecycle handling, bounded checkpoint restore,
observation-only discovery and safe-unavailable capability reporting. It does
not claim signed iOS/Android applications, production-generated native bindings, platform
permissions, live AER, federation or physical-device acceptance; those remain
tracked in `docs/mobile-platform.md` and the production blocker runbook.

The web gateway exposes `GET /api/runtime/workspaces/{workspace_id}/topology`
as an authenticated,
observe-only projection for workspace clients. It returns a bounded,
versioned snapshot of the current local runner layer metadata, visible nodes,
active state and exact non-zero weighted matrix edges. Node IDs are scoped to
the returned topology generation; this endpoint is not a cluster-global shard
snapshot and does not grant worker or management authority. Clients must show
truncation/unavailability explicitly rather than inventing connected-session
edges.

`topology_model::CompiledExecutionPlan` is the reference ownership boundary
for the opt-in partition path. It compiles stable neuron/synapse identities,
component owners and synapse-derived routes into one immutable plan;
`ExecutionPlanRegistry` admits a replacement only at microstep zero and makes
it visible atomically at its effective logical tag. The opt-in superdense
controller validates topology/partition generations and route endpoints before
local admission. It deliberately rejects unowned field scopes and does not
claim that the legacy runner's dense arrays are yet shard-owned biological
state.

The migration gates are declared in `Cargo.toml` and are intentionally not
enabled by deployment manifests: `superdense_executor`,
`virtual_partitioning`, `causal_transport`, `multi_brain_scheduler`,
`replicated_durability`, `management_v1` and `workstation_io`. The additive
`CausalEventEnvelope` and `CausalDataPlane.StreamEvents` in
`proto/distributed.proto` are generated at build time. Rust conversion
preserves logical time, stage, provenance and optional biological endpoints;
`CausalValidationService` currently validates and echoes frames only. It does
not apply shard state or publish durable receipts. Legacy `SpikeBatch` remains
available during the compatibility window.

When `superdense_executor` is enabled, `RunnerEngine` and the distributed node
step loop admit work through the bounded local causal executor before invoking
the legacy biological kernel. Explicit `FieldUpdate` events carry a version,
effective logical tag, scope, cadence, reduction metadata and bounded value;
same-tag field updates sort before spike decisions. Cadence creates a
deterministically identified future event, while Sum/Mean/Maximum and
alpha-declared EMA reductions are applied in canonical order without a
wall-clock barrier. Imports, resets and model changes reset that causal
admission state. Standalone/UI/bridge/GA callers still use the explicit legacy
compatibility facade until their biological phase extraction is complete.

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
