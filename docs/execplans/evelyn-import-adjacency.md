# Place Evelyn sensory inputs beside an existing SNN's outputs

This ExecPlan is a living document maintained under `.agent/PLANS.md`.

## Purpose and observable outcome

When an Evelyn-converted SNN is explicitly attached to a selected existing
AARNN brain, every imported sensory neuron must be within the declared physical
contact distance of an existing output neuron. Preserve the existing neurons,
source identities, weights and internal geometry. Placement gives growth an
opportunity; it does not prove learning or authorise new synapses.

## Specification authority and traceability

The owner's 2026-10-13 requirement supplies the sensory-to-output direction.
The normative specification's Sections 2.1–2.2 and 13.1–13.4 require separate
biological/physical placement, stable identity, atomic topology generations,
safe logical boundaries and resource admission. Preserve INV-008, INV-009,
INV-011, INV-012 and INV-014. Section 21's deterministic topology and morphology
verification applies; numerical and biological validation remain separate.

## Prerequisites and phase boundary

The existing Evelyn `FfnMesh` is evaluated as a separate knowledge region.
`Runner::import_network_json` replaces a runner; it is not a merge. Neither may
be relabelled as a completed attachment. Implement a bounded portable placement
and admission proposal first, using canonical physical coordinates and stable
IDs. Live attachment still requires authorised topology/state/ownership and
capacity adapters at a committed boundary. Do not bypass incomplete phase gates.

## Scope

Use the existing point-only connectome and morphology contracts. Add a default
Evelyn input-placement policy and an explicit planning API. Verify all sensory
contacts, soma separation, unchanged source internal geometry, target identity
and generation before preparing a combined connectome proposal.

## Non-goals

Do not replace an existing brain, mutate a running brain, copy a foreign writer
term, infer federation consent, invent a biological contact radius, force
connections, alter fitted weights or claim growth/learning acceptance.

## Repository orientation

- Clean task worktree: `/var/tmp/aarnn-evelyn-adjacency-20261013`, based on
  main `359e46c00f75e070046dbcdb9b8a33eaecc9599c`; preserve the original checkout.
- `Cargo.toml` names `aarnn_rust` and `tools/xtask`; toolchain is Rust 1.92.0.
  `cargo metadata --locked --no-deps --format-version 1` passed on discovery.
- `src/knowledge_region.rs` owns FfnMesh/FfnRegion; Evelyn exports it from
  `evelyn/src/mesh.rs`. There are no live target-brain geometry arguments today.
- `src/morphology_contract.rs` owns `Vec3`, millimetre units, point-only neuron
  roles, connectome validation and spatial soma-index helpers. Reuse these.
- `src/topology_model.rs` and `src/hierarchical_sharding.rs` own topology and
  biological ownership transactions; a geometry proposal cannot bypass them.
- `src/runner.rs:6425` replaces snapshots; do not modify that restore boundary
  to merge implicitly. No generated schema or platform binding is changed.
- `.github/workflows/build-and-release.yml` verifies native source and publishes
  immutable workload images. Production uses pinned content; no live rollout
  is part of this geometry milestone.

## Architecture and safety constraints

The caller supplies calibrated millimetre contact/separation distances and work
budgets. Search deterministically over existing output IDs and bounded rigid
translations; reject an unplaceable model instead of warping it or moving the
target. No new neural or OS dependency, feature-specific biological semantics,
network I/O or state writer belongs in the planner. Reject stale/tampered plans.

## Milestones

1. Establish the missing import/attachment boundary and register this requirement.
2. Implement deterministic placement and fresh-target admission checks, preserving
   connectome edges and every source pairwise distance.
3. Require the policy when planning an Evelyn mesh attachment; keep standalone
   FFN evaluation compatible and prove serde compatibility.
4. Run focused native tests, formatting and required CI; merge qualified source
   and retire its branch. Qualify live attachment separately when its adapter,
   target and conversion route are available.

## Progress

- [x] `2026-10-13 12:48Z` Read repository/plan rules, located canonical modules,
  checked clean source and recorded the FFN-evaluation versus snapshot-replacement
  gap. The owner's intended attachment route has been requested asynchronously.
- [x] `2026-10-13 13:15Z` Implemented portable rigid placement and mesh/loaded-region
  planning APIs, calibrated work/clearance limits and legacy JSON default policy.
  Fresh admission revalidates source and translated soma spacing, relative
  floating-point geometry, all contacts, IDs, brain/generation and digests.
- [x] `2026-10-13 13:15Z` Native ARM geometry fixtures passed 14/14; final expanded
  suite passes 15/15, including loaded legacy/current numerical equivalence.
  The selected default-profile full library/doc/lint run remains in progress.
- [x] `2026-10-13 13:22Z` Native ARM final verification passed: 15 geometry/mesh
  fixtures, default-profile full library 415/415, doc-test command (zero doctests)
  and Clippy under the repository warning policy. No Clippy diagnostic targets
  the changed placement/knowledge-region files; 370 existing library warnings
  remain. Formatting and source diff checks pass.
- [ ] Final-head canonical dual-architecture CI and main-source consolidation.
- [~] `2026-10-13 13:12Z` Native ARM compile found existing unconditional Rayon
  references in GA/runner when all default features are disabled. Record the
  failed log `/var/tmp/aarnn-evelyn-native-arm64-20261013.log`; select the existing
  explicit `parallel` feature for the portable geometry suite. Do not claim that
  the existing whole workspace compiles without its required runtime feature.
- [!] Live neuronal attachment, resource/ownership commit, runtime conversion
  parity and observed cross-network growth remain unqualified.

## Validation and acceptance

The registered command is `cargo xtask qa run --suite evelyn-import-adjacency
--product server`; its canonical selection is `cargo test --locked
--no-default-features --features parallel --lib
morphology_contract::knowledge_import::tests`. Use a private task target, two
Cargo build jobs, disabled incremental/debug symbols and one OpenBLAS thread.
Native ARM logs: `/var/tmp/aarnn-evelyn-native-arm64-final-20261013.log`.
Scenario: `qa/scenarios/EVELYN-IMPORT-001.toml`; deterministic fixtures are in
`src/morphology_contract/knowledge_import/tests.rs`. The absolute proposal
caps are 1,000,000 neurons/connections, 4,096 candidates and 10,000,000 geometry
checks, with per-record metadata bounded at 256 bytes. Caller budgets may be
lower. Spatial index memory is linear in admitted records; candidate visit and
distance calculations consume the same finite work budget.

Cover translated sensory adjacency; fixed target and unchanged source/internal
edges; permutation independence; absent outputs/sensory inputs; ID collisions;
invalid distances/non-finite coordinates; overlap and bounded search rejection;
stale generation/changed target; tampered proposal and serde replay. Use existing
stable IDs and physical units. Run focused tests on native workers with a private
Cargo target, then the affected package and required CI. No inference/provider
spend or production brain input is required for these fixtures.

## Rollout, compatibility and rollback

Legacy mesh JSON defaults to the new policy when attachment is requested;
standalone evaluation remains unchanged. Placement is a proposal, not a published
checkpoint or committed writer. Removing the proposal code restores prior source
behavior. A future live import needs an immutable baseline and separately
qualified atomic ownership/state/morphology rollback; source revert is insufficient.

## Risks and mitigations

An imported sensory layer may not fit the allowed contact/clearance region.
Reject it explicitly. Unknown target geometry or units cannot establish proximity.
Proposal IDs cannot be assumed to be an authorised allocation. Pre-existing
state, single-writer and model conversion acceptance stay mandatory.

## Surprises & Discoveries

There is no existing Evelyn-to-live-brain merge adapter in the inspected path.
`ConnectomeRole` is geometry metadata, so it cannot grant runtime authority.
The canonical physical representation uses millimetres, not renderer coordinates.
The workspace's legacy GA/runner currently requires Rayon even for geometry-only
library tests. `--no-default-features --features parallel` is the bounded native
suite profile; this is dependency discovery, not a new biological semantic flag.

## Decision Log

- `2026-10-13`, EIA-001: Preserve target state and source internal geometry using
  rigid translations and explicit caller-calibrated bounds. This prepares the
  requested import behavior without inventing a model-changing warp or authority.
- `2026-10-13`, EIA-002: Reject floating-point translations whose source-relative
  component displacement changes by more than 64 f64 epsilons at the source
  displacement/separation scale. Recheck translated clearance directly. This
  arithmetic guard never grows with absolute target coordinates and cannot act
  as a biological error allowance. The bounded six-axis search may refuse a
  geometry another placement could fit; it never claims exhaustive feasibility.

## Outcomes & Retrospective

The portable geometry milestone is implemented and verified on native ARM64.
The Evelyn exporter policy is separately qualified and merged through PR #4,
`0672462699426cec71e0155888039e58980442d6`; both main workflow jobs passed.
Canonical AARNN source CI and consolidation remain next. Geometry verification
cannot establish biological binding or close the missing live attachment,
source-to-runtime conversion provenance, ownership/capacity commit, rollback or
production acceptance gates. The original user checkout and live brain are preserved.
