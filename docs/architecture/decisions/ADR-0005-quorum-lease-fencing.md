# ADR-0005: Quorum lease and writer fencing

Status: Accepted for the migration design; not implemented in Phase 0.

## Context

Current membership/heartbeat and local file leases do not provide consensus-backed writer terms or stale-message rejection for distributed shard state.

## Decision

The control plane will grant a monotonically increasing writer term and lease to exactly one active shard writer after quorum authorisation. Data-plane receivers will reject stale terms and generations. Loss of quorum freezes new ownership and destructive management; active work may continue only under the documented bounded lease/grace policy.

## Consequences

This is the authority boundary for INV-001, INV-009, INV-011, and INV-014. It introduces a control-plane dependency and rolling-upgrade compatibility constraints; no current heartbeat path may be described as satisfying the decision.

Authority: specification sections 2.3, 15, 16, and INV-001, INV-009, INV-011, INV-014.
