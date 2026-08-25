# Codex execution plans for the AARNN distributed emulator

This file defines the required format and working method for every ExecPlan under `docs/execplans/`. An ExecPlan is a self-contained, living design and implementation record that Codex can follow from repository discovery through demonstrably working behaviour. It must remain sufficient to restart the work without relying on chat history.

## Relationship to the specification

`docs/specifications/distributed-whole-brain-emulator-v1.1.md` is normative. An ExecPlan translates a bounded phase of that specification into repository-grounded work; it cannot relax an invariant or invent a conflicting semantic shortcut.

Every ExecPlan must identify:

- the specification sections and invariant IDs it implements;
- prerequisite phase gates;
- user-visible and operational outcomes;
- canonical repository-relative files after discovery;
- schemas, generated outputs, state migrations and compatibility boundaries;
- named tests and acceptance scenarios from Section 21;
- rollback limits, risks and unresolved decisions.

If a phase plan discovers that the specification cannot be implemented as written, preserve the evidence, record the conflict in `Surprises & Discoveries` and `Decision Log`, and stop before violating a safety invariant. Do not silently rewrite the normative specification from an implementation task.

## Living-document rule

The sections `Progress`, `Surprises & Discoveries`, `Decision Log` and `Outcomes & Retrospective` must be updated while work proceeds, not reconstructed at the end.

Use UTC timestamps in progress entries. Each work session must leave the plan in a state from which a new Codex session can identify:

- what is complete;
- what is currently safe to run;
- what remains;
- which commands passed or failed;
- which files are dirty and why;
- what decision or external authority, if any, is blocking progress.

## Required plan structure

Each phase ExecPlan shall use these sections in this order:

1. Title and living-plan notice.
2. Purpose and observable outcome.
3. Specification authority and traceability.
4. Prerequisites and phase boundary.
5. Scope and explicit non-goals.
6. Repository orientation.
7. Architecture and safety constraints.
8. Milestones.
9. Progress.
10. Validation and acceptance.
11. Rollout, compatibility and rollback.
12. Risks and mitigations.
13. Surprises & Discoveries.
14. Decision Log.
15. Outcomes & Retrospective.

The checked-in plan is a normal Markdown file and therefore does not wrap itself in an outer fenced block.

## Repository discovery before implementation

The initial plan may name candidate modules from the specification, but it must not pretend that an uploaded filename is a canonical repository path. Before the first production edit, inspect the repository and update `Repository orientation` with:

- Git/workspace root and relevant packages/crates;
- canonical source and test paths;
- current entry points, ownership and dependency direction;
- build, test, lint, browser, schema and documentation commands;
- generated-file sources and regeneration commands;
- feature flags and configuration precedence;
- current persistence/protocol versions;
- relevant dirty work that must be preserved.

Use `rg`/symbol search and manifests rather than broad speculative rewrites. Read affected code and tests completely enough to understand ownership and error paths.

## Milestone requirements

Milestones are independently verifiable vertical slices, not lists of files to edit. Each milestone must explain:

- the behaviour that will exist at its end;
- why that behaviour is safe at the current phase boundary;
- the implementation approach and affected responsibilities;
- the exact command or scenario that demonstrates success;
- migration, compatibility and rollback implications;
- what remains deliberately disabled.

Prefer a reference implementation and test seam before optimisation. For uncertain algorithms, numerical formats, browser capabilities, device kernels or distributed protocols, include a bounded proof of concept or reference interpreter milestone before production integration.

Do not expose partially safe behaviour. Use coarse temporary flags such as `superdense_executor`, `causal_transport`, `replicated_durability` and `management_v1`; record which path creates persisted state. Remove flags after the specified rollback window and acceptance gate.

## Progress format

Use checkboxes with timestamps and evidence references:

- [ ] `YYYY-MM-DD HH:MMZ` Pending action and observable completion condition.
- [x] `YYYY-MM-DD HH:MMZ` Completed action; command/test/digest or review evidence.
- [~] `YYYY-MM-DD HH:MMZ` In progress; current safe state and next operation.
- [!] `YYYY-MM-DD HH:MMZ` Blocked; exact evidence and required decision.

Split an entry when only part is complete. Never mark a milestone complete because code was written; its acceptance evidence must exist.

## Decision Log format

Each material decision shall record:

- Date and decision identifier.
- Decision and scope.
- Evidence and alternatives considered.
- Consequences for semantics, compatibility, performance and rollback.
- Specification authority or explicit approval.

State-affecting scheduler, numerical, topology, persistence and fidelity decisions must be versioned and replayable. Operational tuning that does not affect authoritative results must still be reproducible through configuration and benchmark evidence.

## Validation evidence

Record exact commands, working directory, relevant environment assumptions and concise results. Evidence may include:

- deterministic state/event digests;
- property-test seeds and minimal failing cases;
- protocol golden fixture versions;
- multi-node topology and fault schedule;
- p50/p95/p99 latency and throughput profiles;
- RAM/VRAM/queue/network bounds;
- recovery point/time measurements;
- browser/native capability and consent results;
- security denial and audit evidence;
- upgrade/rollback and checkpoint compatibility results.

Tests must be bounded and deterministic. Replace arbitrary sleeps with fake clocks, controlled transports or deterministic fault injection. Preserve failure seeds, event traces, scheduler decisions and checkpoint/log references.

Passing a deterministic digest validates reproducibility, not biological adequacy. Plans that touch neuron, synapse, transducer or actuator models must separately state the scientific reference, units, parameter provenance, accepted error measure and known limitations.

## Safety and stop conditions

Continue autonomously through an approved milestone, but stop before:

- destructive changes to user work or live infrastructure;
- an irreversible persistence migration without an agreed backup and rollback boundary;
- weakening an `INV-*` invariant;
- choosing a biologically meaningful event boundary, numerical range/error budget or actuator safety policy not settled by the specification/evidence;
- adding a production dependency or privileged OS integration with materially different security/operational consequences;
- exposing native/global HID actuation without its independent hazard review and safety gate;
- deploying, publishing, rotating credentials or controlling a live brain without explicit authority.

Ordinary repository discoveries are not blockers. Resolve them from code, tests and documentation, update the plan and proceed.

## Git and review discipline

Work on a phase branch or isolated worktree. Preserve unrelated changes. Commit or checkpoint at green milestone boundaries when authorised by the repository workflow. Never use destructive reset, clean or checkout operations on user work.

At each milestone hand-off, review the diff against:

- mapped requirements and invariants;
- ownership and dependency boundaries;
- error, cancellation and shutdown paths;
- bounded memory and backpressure;
- replay, failover and stale-term behaviour;
- protocol/schema compatibility;
- observability without secret or payload leakage;
- documentation and generated outputs.

## Phase completion

A phase gate is complete only when all mapped acceptance evidence passes and the ExecPlan records the result. If a test is deferred to a later phase because its infrastructure legitimately does not yet exist, mark it explicitly as deferred, name the owning later phase and ensure the current phase does not claim the unavailable behaviour.

The final phase must reconcile all deferred evidence, remove obsolete layer-sharding code and temporary flags after the rollback window, and satisfy the complete definition of done in Section 21.15.

## ExecPlan starting skeleton

Use the following headings when creating an additional plan:

    # <Action-oriented phase or feature title>

    This ExecPlan is a living document maintained under `.agent/PLANS.md`.

    ## Purpose and observable outcome

    ## Specification authority and traceability

    ## Prerequisites and phase boundary

    ## Scope

    ## Non-goals

    ## Repository orientation

    ## Architecture and safety constraints

    ## Milestones

    ## Progress

    ## Validation and acceptance

    ## Rollout, compatibility and rollback

    ## Risks and mitigations

    ## Surprises & Discoveries

    ## Decision Log

    ## Outcomes & Retrospective
