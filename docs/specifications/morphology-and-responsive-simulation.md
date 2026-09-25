# Morphology and responsive simulation requirements

This repository document records the implementation authority supplied as the
24 September 2026 morphology brief. It supplements, and cannot weaken,
`distributed-whole-brain-emulator-v1.1.md`.

The implementation must keep causal stimulus processing independent from growth
and rendering; maintain authoritative morphology, simulation state and display
representations as separate versioned data; use constrained finite 3D growth;
derive propagation from committed physical routes; activate structural changes
at safe logical boundaries; and expose the same anatomical and synthetic-column
semantics to Rust and web clients.

The stable requirement identifiers are `CORE-01..05`, `AUD-01..06`,
`DATA-01..07`, `ENV-01..07`, `GROW-01..09`, `SIG-01..07`, `COM-01..07`,
`RUN-01..05`, `DIST-01..03`, `VIS-01..11`, `UI-01..04`, `ENG-01..04`,
`QA-01..06` and `DONE-01..03`. The working implementation matrix and evidence
are maintained in
[`docs/execplans/morphology-responsive-simulation.md`](../execplans/morphology-responsive-simulation.md).

The initial electrical model is a reduced event-based path model. A route uses
piecewise-linear centreline arc length, configured conduction velocity,
synaptic delay and receiving-cell response. Delays are quantised upward to the
configured biological quantum, so a route cannot deliver earlier than its
declared physical model. This is not a compartment cable solver and must not be
described as full biophysical fidelity.

Procedural geometry is labelled procedural. Imported point-only data retains
its source and reconstruction status; display meshes, screen positions and
synthetic columns never authorise growth or alter route timing.

The schema-2 JSON contract represents anatomical ID `value` fields as unsigned
decimal strings. This preserves all 64 bits in browser and mobile clients.
Rust readers accept schema-1 numeric IDs for import compatibility; a browser
must reject already rounded unsafe numbers and request a fresh schema-2 view.
ID generation remains an unsigned 32-bit number. This encoding changes no
biological identity, ownership or logical-time semantics. Upgrade display
clients before schema-2 publication and retain the original checkpoint for
rollback to an older executable.

## GROW-09 — Point-only connectome reconstruction

When an imported connectome contains neuron positions and connectivity but no
anatomical routing, the importer must generate procedural anatomy before it is
presented as anatomical geometry. It must canonicalise neurons from the
innermost position outward, ordering equal-position bands by measured local
density, supplied formation order and stable neuron identity. For each
connection it must generate the axonal and receiving dendritic branches as one
paired proposal, including their bends and terminal sites, and reserve their
swept volumes before either branch is committed. Existing soma volumes,
neurites and other reservations are part of the same three dimensional
occupancy test; a crossing in a two dimensional projection is not a junction.

The reconstruction seed, ordering, parameter provenance, environment revision,
source status and every accepted or rejected connection must be retained. A
rejected route must remain an explicit connectome edge with a reason; it must
not be replaced by a decorative line or silently removed. Generated geometry
is procedural anatomy and must not be described as observed biological
anatomy. The result is a derived import artefact until it is published through
the structural commit protocol, and it must not change the imported neural
weights or causal event ordering.

## VIS-09 — Volumetric anatomical detail

At the highest anatomical detail, stored axon and dendrite centre-lines must
be rendered as filled swept polygons or equivalent volumetric tubes using
their committed radii. Combined route records remain available for route
inspection, but anatomical presentation must not replace neurites with
straight point-to-point connection lines. The bounding membrane calculation
is retained as a soft cloudy fill without an outer white contour.

The display projection must use the committed membrane coverage region as its
stable frame. It must not recompute its origin or scale from changing route
points, animated synthetic topology positions or the current visible subset.
The published `coverage.membrane`, when present, describes the actual
ellipsoidal tissue boundary by centre and radii. `coverage.region` alone is a
coverage box and must not be interpreted as an ellipsoid. All rendered faces
and contact/soma footprints must be clipped to the published boundary; clients
must not drag individual centreline samples onto its edges, which fabricates
long boundary-following pipes. Invalid or
stale geometry is omitted while the last complete display snapshot remains
visible. Stored bend samples must be preserved in the projected path so a
curved volumetric neurite does not judder between straight chords as growth or
streaming updates arrive. These presentation operations must not modify the
authoritative route or its electrical delay.

## VIS-10 — Stable visual identity

Each display snapshot must assign deterministic adjacency-aware colour slots
to visible neuron identities. Adjacent neurons must use different slots, and
the soma colour must remain the base colour for that neuron while its axon and
dendrites use related, visibly offset hues. Switching detail, camera position,
UI client or frame timing must not recolour an unchanged identity.

## VIS-11 — Stimulus presentation

When the display has a compatible activity projection, a traversing spike must
produce a bounded brightness change on the active soma-owned dendrite or axon
volume and on the soma while it is active. This is presentation telemetry only;
sampling, interpolation or dropped frames must not feed back into propagation.
Clients without a compatible activity projection must retain base geometry and
must not invent a biological event.

## Repeatable visual regression

Run `cargo xtask qa run --suite anatomical-render-browser` for the shared
`MORPH-VIS-001` fixture. The result bundle under `target/qa/anatomy/` includes
source/fixture digests, native CPU mesh SVG, a real Chrome canvas capture,
repeated-frame and clipping evidence. `anatomical-render` runs the CPU/Node
checks without requiring Chrome. These lanes neither connect to nor mutate a
live brain. Native GPU and mobile device tests are separate evidence gates.

Native, web, Android and iOS use the same colour-slot hue wheel with
HSL saturation 72% and lightness 55%; axon/dendrite hue offsets are +0.055 and
-0.055 turns. Anatomical Y increases upwards; camera projections and shading
may differ by platform. A bounded snapshot is valid for its loaded geometry
regardless of whether `coverage.complete` is false due to truncation.

The optional membrane field is additive and absent in older snapshots.
`MorphologyState` JSON maps use sorted `[identity, value]` entries, because
anatomical identity objects cannot be JSON property names. Empty legacy `{}`
maps remain readable. No populated old map could be exported with that broken
encoding. Preserve the original point-only export before reconstructing with
this unreleased schema; rollback to an older executable requires that export.

## Growth-cone policy reference

`morphology_contract::growth_cone::propose_step` implements a bounded persistent
walk as an additive reference policy. It combines dimensionless local
attraction, repulsion, tissue orientation, nearest-surface avoidance and seeded
variation. A candidate moves no further than the configured step and turns no
more than the configured forward angle (at most 60 degrees). Axon and dendrite
policies have distinct random domains and may use different parameter sets.
Guidance does not create a synapse or assert target eligibility.

The immutable neighbourhood supplies finite-width capsules, including the
tip's own geometry and soma volumes (zero-length capsules). Swept tests cover
the whole segment and its radius/clearance. Continuation at the declared parent
terminal permits a shared junction for the same owner, with no increase in
radius and no reversal. Arbitrary contacts and crossings are not junctions.
The current environment service is a finite box with conservative expanded-box
obstacle tests; the new policy does not implement a curved tissue field solver.

One call admits at most 64 candidates, 4096 capsules, 1024 obstacles and 256
history points. Incomplete neighbourhoods or excess work return `Deferred`;
obstruction or insufficient resources returns `Stalled`. The caller must
provide every relevant occupied capsule from the spatial revision, including
remote reservations. The policy does not infer that a truncated halo is empty.
Only acceptance permits the owner to advance a tip and deduct resource cost.
`ConeExtension::into_proposal` creates an ordinary versioned `GrowthProposal`
using identities supplied by the owner; it leaves the element `Proposed`.
Reference admission rechecks stored path segments and occupancy from earlier
batches as well as concurrent contenders. It is not distributed activation.

The shared `deterministic::CounterRng` is addressed by brain, seed, stable tip,
tip generation, growth step and axon/dendrite rule. Retrying a growth step or
changing processing order cannot consume a different stream. Reproducibility
is tested on the host; the f64 geometry uses a local comparison tolerance of
1e-12 mm and does not claim cross-platform bitwise numerical equality.

Run the fixture with:

```sh
cargo xtask qa run --suite morphology-growth-cone
```

The runnable policy configuration is
`qa/fixtures/morphology/growth-cone-v1.json`; seed 42 and bounded scenarios live
in `MORPH-GROW-001` and the associated Rust tests. The result bundle under
`target/qa/growth-cone/` records revision, source digests, configuration, command
and outcome. Parameters are procedural engineering assumptions, not calibrated
biology. The user's NeuroMaC and ECM/chemogradient references motivate local
guidance; this implementation has not been validated against either study.

The policy remains separate from known-target point-only reconstruction and
legacy live growth. Automatic daughter-tip scheduling, branching/resource
allocation across tips, retraction, spatial indexing, coarse A* guidance,
distributed occupancy, electrical activation and recovery remain open work.
Calling the kernel for a daughter tip does not by itself complete those gates.
There is no model/checkpoint cutover for this policy; rollback removes its
caller without changing legacy brain state.
