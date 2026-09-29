# ADR-0006: Separate cluster, node and brain identity

Status: Accepted for architecture and launcher configuration; the production
orchestrator hierarchy and management gates remain unimplemented.

## Context

The specification defines organisation/team/project resource scopes, isolated
brain domains, orchestrator quorum, worker membership and explicit federation
links. The compatibility runtime already carries separate node IDs and network
IDs. Several local launchers nevertheless used `node_1` and `node_2` as
`--brain-id` values and omitted stable `--node-id` values. The worker preload
then reported a network named after each node; orchestrator Join/Heartbeat
handling correctly exposed unknown hosted network IDs, so the dashboard
presented worker bootstrap state as extra brains.

The Rust UI can connect to several orchestrator endpoints, but this is client
endpoint aggregation. There is no parent/child orchestrator protocol, shared
quorum, delegated control plane or cross-cluster scheduler. `docs/architecture.md`
also records that multiple orchestrator replicas currently form independent
cluster views and are unsupported.

## Decision

1. Keep identity scopes distinct: a `NodeId` identifies a compute process; a
   `BrainId` identifies one neural-network domain; shard/topology generations
   identify execution ownership inside that brain; an orchestrator endpoint
   identifies a local control and membership scope. A node may report hosting
   several brain IDs, and a brain may place work on several nodes.
2. In a local single-cell deployment with no upstream control plane, the local
   orchestrator is also the cluster master for its registered workers and the
   workstation-facing I/O ingress for that cell. It admits input under the
   selected brain's I/O policy and hands the neural events to the designated
   sensory owner. Neural peer traffic continues directly between assigned
   workers; a remote or higher-level management endpoint is not an audio/AER
   relay.
3. A dashboard connected to multiple orchestrators shows independent scopes.
   It does not combine their workers into one cluster or transfer authority.
   A future global control hierarchy requires an explicit versioned
   management/resource contract, identity delegation, tenant policy, audit,
   fencing and failure semantics; it must not be inferred from connectivity.
4. A cluster may host multiple brains with independent IDs, clocks, state,
   seeds, quotas, checkpoints and permissions. Scheduling may share permitted
   capacity according to tenant/brain fairness. Being owned by the same
   principal does not create a causal link. Cross-owner interaction requires
   explicit federation authorisation by both owners and a versioned
   `FederationLink`; federation does not merge brain identity or authority.
5. Legacy `combined_group`, `federation_group` and related-network fields are
   placement compatibility metadata only. They do not prove common ownership,
   grant access, create an event path or constitute a live federation link.
6. A single-brain local launcher passes the same target brain ID to the
   orchestrator's hosted network and every worker, and a distinct stable node
   ID to each worker. A multi-brain launcher names each hosted brain explicitly
   while retaining unique node IDs. Launcher output and status checks
   distinguish cluster membership from brain count and placement.

## Consequences

The local run scripts can demonstrate one cluster master, stable worker
membership and the actual hosted brain without claiming replicated authority.
Cross-cluster management remains a Phase 7 deliverable and live federation
remains a Phase 8 deliverable. Existing compatibility placement is retained
until the planned scheduler and shard-owner migrations replace it.

Authority: specification Sections 11.6, 12.1–12.3, 15.1–15.2 and 16.1–16.5;
`INV-001`, `INV-010`, `INV-011`, `INV-015`–`INV-017`; Phase 5 and Phase 7
production gates.
