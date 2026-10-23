# Add LEC robots, a biological crow and MuJoCo/Isaac Sim backends

This is a starting ExecPlan, to be adapted into `docs/execplans/` under the repository’s `.agent/PLANS.md`. This package does not itself change runtime semantics.

## Purpose and observable outcome

A RustRover Codex session can implement 22 new profiles and port all six existing profiles to native MuJoCo and Isaac Sim, with bidirectional AER and per-instance brain binding. The complete 28×8 matrix remains reviewable. The user can launch a body, observe its sensors drive a selected real AARNN network and see its neural outputs affect the physics and next observations.

## Specification authority and traceability

Root AGENTS.md and `docs/specifications/distributed-whole-brain-emulator-v1.1.md` govern. Map Sections 3.4, 16.16–16.24 and relevant Section 21 tests during discovery. Preserve INV-002/007/008/009/010/015/016/017 and ADR-0007. Reference mode and production-governed mode have separate gates.

## Prerequisites and phase boundary

Discover the active phase state, named feature sets, current instructions and toolchains. Existing production workstation/management gates cannot be promoted by simulation work. Add adapters behind compatible explicit profiles while preserving the current executor and admission boundaries.

## Scope

The 15 commercial model families, seven UGOT forms, Corvus corone, six existing profiles, eight runtime targets, licensed assets, original surrogates, canonical model/IO compilation, physical/process affordances, AER transducers, network recipes, learning/evaluation and QA evidence.

## Non-goals

No live hardware/building/payment operation, vendor contact, proprietary firmware extraction, publishing/deployment or claim of a measured complete crow brain. No invisible substitute for missing physics, mapping, licences or engine execution.

## Repository orientation

Read `research/repository-map.md`; starting snapshot `a02c9c43089b5cb488094b50124d034ae51b4ca3`. The user checkout is `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust` and was not mounted during preparation. Record actual HEAD/dirty state, verified files, ownership and command set before implementation. Candidate new adapter locations are `sim/mujoco/` and `sim/isaacsim/`; the existing Cargo workspace remains canonical.

## Architecture and safety constraints

One canonical embodiment and named IO contract per selected model variant; one physics owner and sensory producer per world/body; existing authoritative AARNN execution and committed-output rules; bounded asynchronous queues; no wall-clock biological time or global neural barrier. Keep original licensed sources immutable and generated assets reproducible. Exact OEM fidelity, functional surrogate and visual preview remain separate labels.

## Milestones

1. Current discovery, baseline, phase mapping and 28-profile/eight-target inventory.
2. Verified source assets, canonical mechanical/IO schema and official Galbot vertical slice through native MuJoCo and AER to a real AARNN network.
3. Isaac Sim native import/bridge, then Webots/Unity/Unreal integration for the same slice; preserve passive/mimic constraints and variant identity.
4. UGOT mecanum slice and reusable delivery/cleaning/kiosk/modular robot archetypes; original surrogates for missing OEM mechanics with explicit assumptions.
5. Crow articulated body, sensory/motor projections, aerodynamic/contact/energy model and mechanics falsification; then actual neural-control curriculum/evaluation.
6. All remaining profiles, including the six existing profiles in MuJoCo/Isaac; common mapping fixtures and checkpoint compatibility.
7. WebGL and Java/Bedrock functional physics-service views, mirrored interactions, headless lifecycle and per-instance isolation.
8. Complete required native/rendered, neural, fault, performance and mixed-fleet evidence; final launch/documentation/asset and licence delivery.

Each milestone needs a runnable demonstration, exact tests, affected responsibilities and rollback point. Resolve ordinary discoveries autonomously; retain target-specific blockers while completing independent work.

## Progress

- [x] 2026-10-23 — Research and instruction pack created; official Galbot and community UGOT sources identified, pinned and bundled. Source inspection only; no simulator execution is claimed.
- [ ] Local discovery and baseline recorded.
- [ ] Canonical schema/IO/compiler and first live closed loop pass.
- [ ] MuJoCo and Isaac Sim native backends pass representative and all-profile lanes.
- [ ] LEC surrogate/exact-variant profiles, all native engines and crow pass their declared gates.
- [ ] Existing six profiles pass new-backend and prior-target regressions.
- [ ] WebGL and both Minecraft editions pass functional service/interaction gates.
- [ ] All 224 rows have current outcomes/evidence and remaining blockers are reconciled.

## Validation and acceptance

Follow `spec/acceptance.md`, extend xtask/scenario orchestration and retain unique result bundles. A source hash or a unit test is not an engine load result; a load is not dynamics; dynamics is not a neural learned task. Test per-instance isolation, actual committed output, stale/duplicate/revoked effects, reset/replay, queue pressure and responsive viewing. Record exact commands after implementation rather than assuming future CLI flags exist today.

## Rollout, compatibility and rollback

Keep existing profile IDs, named IO semantics, network/checkpoint files and auth routes. Add new feature/backends and generated assets without changing default production state. Version model/mapping changes; reject mismatched persisted bindings. Use a branch/worktree preserving user changes. Roll back only task-owned additive files/commits, never reset the user tree or rewrite old networks.

## Risks and mitigations

Proprietary mechanics -> original surrogate plus open exact-replica gate. Galbot/coffee variant ambiguity -> explicit variant selection. Cross-engine solver/import differences -> calibration and task tolerances. Crow flight/learning complexity -> staged mechanics then trained-control evidence. Isaac requirements -> supported dedicated runner and explicit capability report. Browser/game previews -> authoritative physics-service mode with no duplicate stepping/sensors.

## Surprises & Discoveries

Rental page adds three offerings missing from the 12-family homepage. XBOT S Pro webpage and brochure conflict on size/mass. S100 depth differs between LEC and manufacturer. Legacy AER1 differs from governed AARNN-AER/1 and FPAA fabric. Existing browser/game paths are kinematic, and Minecraft auth prose is stale relative to inspected source. MuJoCo/Isaac integration did not exist in the inspected tree.

## Decision Log

- 2026-10-23 D-001: Carrion crow (Corvus corone) is the default bird, a reasonable UK species choice; mass/wing values are selected reference parameters.
- 2026-10-23 D-002: Author functional surrogates when exact OEM models are unavailable, while preserving explicit unresolved exact-model gates.
- 2026-10-23 D-003: Add MuJoCo and Isaac Sim for all new and existing profiles, following the user’s explicit scope addition.
- 2026-10-23 D-004: Use an authoritative physics-service mode for browser/game functional experiments rather than accepting current preview movement as physical validation. Final implementation authority/transport choice is to be confirmed from repository evidence and recorded here.

## Outcomes & Retrospective

Pending implementation. The prepared pack and its helper validation establish the hand-off, not the robot expansion. Replace this paragraph with measured outcomes, reproduction commands, calibration limits and target-specific blockers when the implementation is actually executed.
