# Build the replicated control plane and authorised management plane

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 7 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` for orchestrator authority, lifecycle operations and both management clients.

## Purpose and observable outcome

Allow authorised web and Rust workstations to manage their permitted brains through an orchestrator quorum, never by controlling workers directly. At completion, membership, leases, fencing, placement and durable operations survive leader change; concurrent clients receive explicit version conflicts; imports/exports/resets/restarts are idempotent and audited; and permissions are enforced server-side across tenant, brain, shard, federation and peripheral resources.

## Specification authority and traceability

- Primary sections: 4.1, 15.1–15.2, 15.5–15.7, 16.1–16.14, 17.3, 17.5, 19, 20.8, 21.7 and 21.10–21.12.
- Invariants: `INV-001`, `INV-003`, `INV-005`, `INV-012`, `INV-015`–`INV-017`.
- Tests: `UT-FENCE-001`, `UT-AUTH-001`, `UT-IDEMP-001`, `IT-DIST-008`, `CT-005`, `CT-008`, `CT-012`, `API-001`–`019`, plus Sections 21.11–21.12 management workflows. High-rate media/USB execution for `API-013`–`019` remains disabled until Phase 8, but resources, directional grants, local-device bindings, channel independence, leases and denial paths exist.
- Phase gate: two authorised workstations safely perform concurrent lifecycle/import/export/checkpoint/federation-resource operations through leader failover; stale/unauthorised/direct-worker actions are rejected and audited; no operation or promotion executes twice.

## Prerequisites and phase boundary

Phases 1–6 must be green. Phase 6 supplies durable operation prerequisites and recovery mechanisms but has no production promotion authority. This phase installs quorum/lease fencing at every boundary and enables production failover. Phase 8 supplies live high-rate workstation media/data streams, transducers, actuator delivery and federation event links.

## Scope

- Implement a replicated orchestrator state machine for membership, node lifecycle, brain/shard desired state, placement plans, terms, leases/fencing tokens, operation records, resource versions, grants and audit references.
- Implement node join, healthy, draining, failed, recovering and removed transitions with attested identity/capability.
- Validate authority at event, WAL, checkpoint, migration, output and worker-command boundaries.
- Implement versioned management APIs for resource discovery, create/clone/import/export, configure, start/pause/stop/restart/reset/delete, checkpoint/restore, migrate/rebalance, durability policy, federation/peripheral resource configuration—including separate USB AER input/output capabilities—and operation cancellation.
- Use asynchronous durable `Operation` resources for long work, with progress, cancellation boundary, error and resume cursor.
- Implement OIDC/OAuth 2.1 authorisation-code with PKCE for users, mTLS/workload identity for services and default-deny RBAC/ABAC policy.
- Implement idempotency keys, request hashes, optimistic concurrency (`resource_version`/ETag) and typed errors.
- Implement status/telemetry/event streams with bounded cursors, reconnect and redaction.
- Migrate `app.js`/web UI and `ui.rs`/Rust UI to one generated/versioned management client and orchestrator endpoints.
- Implement staged, sandboxed import validation and consistent, authorised, expiring export retrieval.

## Non-goals

- Do not expose workers or shard transport endpoints to end-user identities.
- Do not trust hidden UI controls as authorisation.
- Do not perform microphone/camera/display/HID capture or actuator effects before Phase 8.
- Do not permit a single orchestrator or compute node to promote itself after quorum loss.

## Repository orientation

Locate current HTTP/gRPC endpoints, `app.js`, `index.html`, `service-access.js`, `shell.js`, `swagger.html`, `ui.rs`, auth/config code, worker administration endpoints and deployment manifests. Inventory any UI call that targets a worker directly, any local-only operation, token storage/logging and duplicate API models.

The intended services/modules are `control/consensus`, `control/membership`, `control/lease`, `control/placement`, `management/api`, `management/operation`, `authn`, `authz/policy`, `audit`, `import`, `export` and a schema-generated client used by both UIs. Consensus library/protocol choice is an ADR backed by repository/licence/operational evidence; do not invent an ad-hoc consensus protocol.

## Architecture and safety constraints

The replicated control plane determines who may be active; the data plane continues already-authorised safe execution during a transient leader change. New promotions, destructive operations and membership changes require quorum. Every authority-sensitive record carries brain/shard, term, fencing token and topology generation; stale values fail at all write/output boundaries.

All authorisation is server-side, resource-scoped and default deny. Authentication mode misconfiguration cannot become allow-all. Tenant/resource existence is not leaked. Browser tokens use secure practices; secrets/tokens/cookies/media payloads are redacted from logs and diagnostics. Service identity is distinct from user delegation.

Mutations require idempotency and optimistic concurrency. Same key and same canonical body returns the same operation; same key with another body conflicts. Long work is not held in one request or UI thread. Leader recovery reconstructs and resumes an operation from durable state once.

Reset/delete/native-effect-related actions require capability-aware confirmation and policy gates. Imports are staged, size/rate limited, path-safe, schema/version checked and scanned/validated outside authoritative state. Exports are consistent cuts with short-lived principal-bound retrieval.

Peripheral/federation resource models and grants are created now to avoid an ungoverned side channel. Brain-control permission alone never grants I/O capture or actuation. USB device access, AER input and AER output are distinct capabilities, and one session may hold them concurrently with A/V/HID grants without merging their channel state. Phase 8 must use these resources; it cannot bypass them.

## Milestones

### Milestone 7.1 — Consensus-backed authority and node lifecycle

Select/document a mature consensus implementation, persist the replicated state machine and implement membership/lifecycle. Issue terms/leases/fencing tokens and validate them at worker, data, WAL, checkpoint and output adapters. Complete quorum-loss and isolated-old-active tests.

### Milestone 7.2 — Versioned management contract

Define the resource hierarchy, typed errors, operation lifecycle, idempotency and optimistic concurrency in one API schema. Generate Rust/JavaScript clients and golden compatibility fixtures; remove handwritten semantic duplication.

### Milestone 7.3 — Identity, policy and audit

Integrate user OIDC/PKCE and service mTLS/workload identity. Implement default-deny RBAC/ABAC decisions, grant lifecycle, audit integrity/redaction and fail-closed privileged mutations when audit durability is unavailable.

### Milestone 7.4 — Durable lifecycle/import/export workflows

Implement create/start/pause/stop/restart/reset/delete, checkpoint/restore, migrate/rebalance and durability changes as resumable operations. Add staged import and consistent export. Run leader failure/cancellation/retry at each operation boundary.

### Milestone 7.5 — Web management client

Replace direct/static worker assumptions with orchestrator discovery and generated client calls. Add sign-in/scope selection, permission-driven controls, operation progress/cancel, conflicts, reconnect, discontinuity/durability display and accessible destructive confirmations without blocking the browser thread.

### Milestone 7.6 — Rust management client

Add secure endpoint profiles, device/PKCE authentication, multi-orchestrator/tenant/brain switching, generated management client, operation/conflict/reconnect flows and strict separation of remote vs standalone state. Keep render/input loops non-blocking.

### Milestone 7.7 — Peripheral/federation governance and phase gate

Implement resources, grants, bindings, capability reports, actuator lease records and disabled media/USB-AER endpoints. Model per-channel direction, local device consent, optional device allow-list and concurrent multi-modality binding. Verify denied I/O requests and concurrent clients, then enable production durability promotion only after all fencing/quorum chaos tests pass.

## Progress

- [x] `2026-08-23 12:00Z` Audited management endpoints, browser/Rust UI call
  paths, authentication and deployment boundaries; direct runtime/worker
  management remains reachable and is retained for rollback.
- [x] `2026-08-23 12:00Z` Implemented default-deny policy, operation-state and
  leader-term validation reference types in `src/management.rs`; the
  idempotency/fencing case in `phase2_to_phase8_gate` passed.
- [x] `2026-08-23 12:00Z` Added versioned management DTO/reference seams and
  mobile capability/discovery documentation without granting authority from
  discovery.
- [!] `2026-08-23 12:00Z` There is no mature replicated consensus authority,
  generated management client consumed by Rust/web/native clients, complete
  OIDC/PKCE/mTLS/audit integration or live API concurrency/security suite.
  `management_v1` remains disabled.
- [x] `2026-08-23 12:07Z` Final cross-review confirms the Android shell is only
  a bounded JNI/reference client seam: it does not bypass orchestrator
  authorisation or claim a management capability. Generated Kotlin management
  clients, authenticated enrolment and live API evidence remain blockers;
  `management_v1` remains disabled.
- [x] `2026-08-23 12:17Z` Final cross-review reran all sequential Rust checks,
  Android JVM tests and emulator launch evidence. The shell still exposes no
  management authority and no generated management client is consumed by the
  native, web or Android paths. Mature quorum consensus, OIDC/PKCE/mTLS/audit
  integration, live concurrency/security evidence, authenticated enrolment and
  direct-worker closure remain blockers; `management_v1` remains disabled and
  legacy management paths remain the rollback boundary.
- [x] `2026-08-23 13:35Z` Android remote validation reached the live gateway
  and received the expected `HTTP 401 invalid_credentials` response for a
  deliberately invalid login. This confirms the Android client uses the
  ingress host and fails closed at authentication; it is not evidence of
  generated-client consumption, OIDC/PKCE/mTLS, quorum fencing, API
  concurrency or live workspace authorisation. The missing authorised
  credential is an explicit validation blocker; `management_v1` remains
  disabled and all legacy management paths remain rollback paths.
- [x] `2026-08-23 14:55Z` Final cross-review confirms the Graph Explorer
  capture is a read-only presentation path and issues no management or worker
  command. The Webots IPC round trip therefore supplies no management-plane
  acceptance evidence. Mature quorum authority, generated client consumption
  across Rust/web/native, authenticated enrolment, OIDC/PKCE/mTLS, audit and
  live concurrency/security evidence remain blockers; `management_v1` remains
  disabled.
- [x] `2026-08-23 17:13Z` Added and reviewed the authenticated, observe-only
  workspace topology route. It resolves owner scope through the existing
  gateway authorisation path and returns bounded versioned DTOs; Android uses
  this route and never addresses workers directly.
- [!] `2026-08-23 17:13Z` Generated management clients, replicated consensus,
  OIDC/PKCE/mTLS, audit/concurrency evidence, authenticated enrolment and
  direct-worker closure remain production blockers. The topology route is an
  additive reference endpoint and does not close `management_v1`.
- [x] `2026-08-23 17:39Z` Final Rust verification, protobuf freshness through
  the all-feature build and Android JBR/SDK-scoped JVM/package verification
  passed. The topology route remains observe-only, owner-scoped and bounded;
  it issues no worker or management mutation.
- [!] `2026-08-23 17:39Z` Explicit management blockers remain: generated
  versioned clients consumed by Rust/web/native/Android, quorum-backed
  consensus and fencing, OIDC/PKCE/mTLS, audit and live concurrency/security
  evidence, authenticated enrolment, and direct-worker closure. `management_v1`
  remains disabled; legacy management remains the rollback boundary.

## Validation and acceptance

- `UT-FENCE-001`: stale term/token/generation fails at event, log, checkpoint and output boundaries.
- `UT-AUTH-001`: missing/unknown permission and production auth misconfiguration never grant access.
- `UT-IDEMP-001`: same key/body returns one operation; changed body conflicts.
- `IT-DIST-008`, `CT-005` and `CT-008`: safe work survives leader change, old isolated active is fenced, and quorum loss prevents new promotion/destructive work.
- `CT-012`: privileged mutation follows fail-closed audit policy while documented biological data-plane behaviour continues safely.
- `API-001`–`012`: concurrency, retries, tenant isolation, forged/direct-worker requests, refresh, CSRF, hostile import, export expiry, leader failure, auth bypass and redaction pass.
- `API-013`–`019`: grants/bindings/lease/dedupe/security denial contracts—including directional USB AER permission and stale device-epoch rejection—pass with media/USB execution disabled; Phase 8 reruns them end to end.
- Sections 21.11–21.12 management cases pass in browser automation and Rust integration tests, including accessibility/responsiveness and no blocking render/UI loop.

## Rollout, compatibility and rollback

Use `management_v1` and a documented API compatibility window. Deploy quorum members conservatively, verify snapshot/log upgrade and prevent mixed versions from issuing unsupported writes. UIs may roll back within the API window; authority state cannot be downgraded without its tested snapshot/log migration. Disable legacy worker management access before considering the gate complete.

## Risks and mitigations

- Ad-hoc consensus can violate safety. Use a mature implementation and model/chaos test membership and recovery.
- Incomplete fencing leaves side doors. Enumerate and test every event/storage/output/command boundary.
- Generated clients can drift from server schema. Generate in CI and fail on diff/golden incompatibility.
- UI permission hiding can mask server gaps. Forge direct requests in tests and default deny.
- Leader failover can duplicate destructive work. Make operations durable, idempotent and step-resumable.

## Surprises & Discoveries

The policy/orchestrator reference fences stale operation terms, but browser and
runtime clients still use legacy endpoints. No replicated authority, generated
management client or live OIDC/PKCE/API security evidence is present.

## Decision Log

- Initial decision: only orchestrator quorum grants active authority; compute/workstation reachability is insufficient. Authority: Sections 15.1 and 15.7.
- Initial decision: both UIs share one versioned generated management contract. Authority: Sections 16.12–16.13 and 17.3.
- Initial decision: brain management and peripheral I/O/actuation are separately authorised capabilities. Authority: Sections 16.5 and 16.15–16.16.

## Outcomes & Retrospective

Reference policy and stale-term operation checks pass. Consensus, generated
client consumption, live security/accessibility evidence and direct-worker
closure remain open; Phase 8 governed I/O cannot be promoted before this gate.
