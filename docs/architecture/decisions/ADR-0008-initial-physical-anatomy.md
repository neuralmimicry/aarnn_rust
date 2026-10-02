# ADR-0008 — Physical anatomy for new and incomplete imported models

Status: calibration policy accepted by user on 2026-10-02; authoritative live cutover remains proposed pending structural-activation and checkpoint evidence.

## Context

The shipped default network constructs `Topology3D` positions in a normalised `[-1, 1]` space. `Morphology::from_weights` builds live somas, branches and synapses from those positions, but persists no soma or neurite radii and publishes no swept-volume clearance witness. The live display contract therefore correctly stops at visual stage 6. The point-only importer can create a radius-bearing `PointOnlyReconstruction` with admitted and rejected routes, but that result is a separate derived artefact; the legacy Runner continues to execute and grow its own morphology. Treating that artefact as committed live geometry would misstate route ownership and timing.

The requested outcome is that new default/empty models support all nine visual stages from their first committed generation, and that imports missing physical parameters become growable physical models. A model without connections has no contacts to render, but a verified empty contact set must still be inspectable at stage 9. This decision implements NVR-037/038 with VIS-09..12 and preserves INV-002/004/008/009/014.

## Proposed boundary

1. Preserve imported source bytes, source unit status, original positions and graph connectivity. A conversion is a versioned model operation with an explicit seed, physical coordinate frame, scale, radius/clearance policy and provenance. A declared measured quantity remains distinct from a modelled one.
2. Use one physical builder for new and incomplete imported models. It places or repairs soma centres, proposes paired axonal/dendritic branches, validates swept volumes and synaptic gaps, and records each admitted or rejected connection. Rejected graph links remain schematic and eligible for later growth; they do not acquire a physical path or synapse marker.
3. Commit the resulting morphology and electrical route mapping together at the agreed logical boundary under stable owner IDs and a topology/route generation. The live executor and display publisher read this same committed state. Subsequent growth uses the same admission and ownership path; no display-only builder advances a separate anatomy.
4. Publish contact-set coverage explicitly, including a verified empty set, rather than inferring contact capability from the presence of markers. Stages 7–9 require radius-bearing paths and clearance evidence; stage 9 additionally requires contact coverage. An empty model may render an empty stage-9 scene without fabricating neurons or synapses.
5. Keep old checkpoint readers and the stage-1..6 compatibility view. Physical migration creates a new immutable checkpoint/generation with a source-to-new-ID map and rollback to the old checkpoint. Reader-first schema rollout precedes writer activation.

## Automatic calibration decision

The user selected automatic intelligent/heuristic physical calibration for defaults and incomplete imports. Supplied physical measurements with declared units take precedence when valid; a source without such measurements uses a versioned **modelled** conversion. A heuristic can calibrate internal geometry to its declared size prior, but cannot recover a specimen's empirical millimetres from dimensionless coordinates. The product must label these quantities as modelled and retain the original positions.

The v1 reference estimator uses the existing `ReconstructionConfig` prior of 0.04 mm soma radius, 0.006 mm neurite radius, 0.002 mm clearance and 0.02 mm synaptic gap. It selects up to 512 positions by a stable coordinate hash, takes the tenth-percentile nearest positive source spacing and maps that spacing to two modelled soma diameters plus clearance. Empty or wholly coincident positions use a recorded 1 mm/source-unit fallback and require deterministic soma repair. Every proposal still undergoes 3D occupancy admission; the scale estimate is not a clearance witness. This is a deterministic procedural reference, not biological validation. The old suggestion of a fixed 55 mm/normalised-unit value is withdrawn because the region preset does not declare its physical unit. The live builder must use the same recorded policy, test route-timing effects and fail explicitly if occupancy cannot be verified.

No runtime path may infer millimetres from field names such as `position_mm`, assign a tiny radius solely to pass clearance, set a clearance flag without an occupancy proof, or use the imported reconstruction as a second live route authority. The profile choice and route-delay conversion must be settled and validated before enabling the new writer.

## Verification and rollback

Add deterministic create/import/restart/growth fixtures and NVR-QA-021/022. Compare source and post-build graph links, accepted/rejected physical routes, stable IDs, logical route tags, delays, checkpoints and all nine render stages. Run the provided WAV through the normal sensory provider to check activity presentation independently from anatomical capability. Benchmark initial build, live growth and UI sampling separately. The new writer remains disabled until the owning morphology and distributed-activation gates pass; rollback retains existing checkpoint bytes and stage-1..6 publication.
