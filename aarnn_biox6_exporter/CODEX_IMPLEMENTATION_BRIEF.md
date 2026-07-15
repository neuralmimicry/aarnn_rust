# Codex implementation brief: AARNN to BIO X6 physical photonic exporter

## Objective

Build a Rust CLI and library that freezes an AARNN network at a chosen tick,
converts its neuron/axon/dendrite/synapse semantics into a printable,
calibrated optical-hydrogel network, and produces:

- a physical network plan;
- per-material preview meshes;
- a BIO X6/DNA Studio reviewable toolpath;
- a material and printer setup manifest;
- a power, timing and threshold verification report;
- a provenance map from every printed feature to its AARNN source IDs.

The exporter must never infer that geometric light-flight time alone can
represent AARNN millisecond delays.

## Required pipeline

### 1. Extract a frozen AARNN snapshot

Add `AarnnGraphSource` in the AARNN crate and implement it for the current
database/in-memory model. Collapse the detailed schema into nodes and directed
connections while preserving component provenance:

- soma + axon hillock → logical node;
- dendrite branch/dendrite/dendrite bouton → receiving path and input port;
- axon/axon branch/axon bouton → transmitting path and splitter topology;
- synaptic gap → directed connection, polarity, weight, delay and plastic state.

Reject incomplete or ambiguous connectivity.

### 2. Choose signal encoding

Use pulsed optical intensity for activation.

Use one of these signed-value encodings:

- dual rail: separate positive and negative paths;
- separate wavelength: excitatory and inhibitory wavelengths;
- electronic subtraction at the detector/regenerator.

The first implementation should use dual rail or electronic subtraction.
A passive optical path cannot carry a negative value.

### 3. Map logical components to physical features

| AARNN component | Physical feature |
|---|---|
| soma | fluorescent or optoelectronic integration node |
| axon hillock | calibrated threshold/regenerator element |
| axon | core/cladding optical waveguide |
| axon branch | Y splitter with calibrated coupling ratio |
| axon bouton | terminal optical coupler |
| synaptic gap | attenuation/coupling/delay junction |
| dendrite bouton | receiving coupler |
| dendrite | converging waveguide |
| dendrite branch | optical combiner |
| myelination state | lower-loss material profile or larger core, subject to calibration |
| energy level | available optical power budget, not geometry |

### 4. Establish a time-scale policy

For each edge:

`target_physical_delay = aarnn_delay * physical_seconds_per_aarnn_unit`

Calculate geometric delay:

`tau_geometry = n_eff * length / c`

Calculate:

`tau_residual = target_physical_delay - tau_geometry`

Realise positive residual delay using a selected policy:

- fluorescence lifetime;
- photochromic or thermoresponsive response;
- recurrent optical loop;
- time-bin scheduling;
- external detector/comparator/LED regenerator delay.

If the residual is negative, reroute, change the global time scale or fail the
export. Never silently clamp timing errors.

### 5. Threshold mapping

Represent the AARNN activation threshold as an optical transfer curve, not only
as a material label.

Store measured pairs `(input_power, output_power)` for every node formulation.
Invert the curve to find the optical power needed to cross the logical
threshold.

Support these node realisations:

1. passive fluorescent: visualisation and summation, no hard threshold;
2. soft-threshold fluorescent: chemistry-defined nonlinear response;
3. photochromic/thermoresponsive: stateful but slower and environment-sensitive;
4. optoelectronic regenerator: photodiode, comparator and re-emitting LED/laser.

Default to the optoelectronic regenerator whenever threshold error exceeds the
configured tolerance.

### 6. 3D placement and routing

Implement a deterministic constraint solver:

- keep all geometry inside the calibrated build volume;
- enforce minimum feature size and minimum spacing;
- enforce bend radius;
- minimise path length subject to target delay;
- avoid intersections or move crossings to separate Z levels;
- create explicit splitter/combiner geometry;
- reserve perimeter ports for optical inputs, outputs and detector access;
- reserve service cavities for optoelectronic elements;
- support FRESH/support-bath mode for unsupported 3D paths.

Start with a layered layout:
sensory inputs on X-min, motor/readout nodes on X-max, internal nodes between
them, and recurrent edges on elevated Z routing layers.

### 7. Material allocation

Treat every formulation as a calibrated material ID.

Suggested roles across the six BIO X6 positions:

1. higher-index optical core;
2. lower-index cladding, preferably coaxial with the core;
3. fluorescent node material;
4. attenuating/scattering junction material;
5. opaque separator or structural material;
6. support bath/degradable support or another functional node material.

Do not hardcode commercial formulations as optically suitable. Refractive
index, loss, fluorescence response, curing shrinkage and hydration drift must
come from lab measurements.

### 8. Print planning

Generate operations in this order unless calibration proves another order:

1. support bath or base;
2. cladding lower shells/tracks;
3. optical cores or coaxial core/cladding tracks;
4. branch and coupling structures;
5. fluorescent/threshold nodes;
6. opaque separators and fixture features;
7. staged photocuring;
8. final cure and hydration protocol.

Minimise tool changes and prevent premature curing inside nozzles.

### 9. Outputs

Create:

- `physical-plan.json`
- `toolpaths.json`
- `job.gcode`
- `materials.yaml`
- `machine-setup.yaml`
- `verification.json`
- `provenance.csv`
- `preview/core.stl`
- `preview/cladding.stl`
- `preview/nodes.stl`
- `preview/support.stl`
- `README-PRINT.txt`

The G-code backend must be template-driven. Maintain separate dialect fixtures
for each tested BIO X6 firmware/DNA Studio profile.

### 10. Validation gates

Fail export when:

- a referenced AARNN component is missing;
- core index is not above cladding index;
- a path is outside build volume;
- a feature is below calibrated minimum;
- a bend violates minimum radius;
- an unplanned crossing exists;
- predicted received power falls below detector sensitivity;
- a splitter exceeds the power budget;
- a delay error exceeds tolerance;
- a threshold cannot be realised within tolerance;
- material volume exceeds loaded cartridge capacity;
- required tools exceed available BIO X6 positions;
- a machine command lacks a verified dialect mapping.

### 11. Calibration workflow

Before any full network, print coupon libraries:

- straight waveguides at multiple lengths;
- bends at multiple radii;
- crossings at multiple Z separations;
- two-way and multi-way splitters;
- core/cladding diameter combinations;
- fluorescent nodes at multiple sizes and dye concentrations;
- attenuating junctions;
- candidate threshold/regenerator nodes.

Measure:

- refractive index;
- propagation loss by wavelength;
- bend and junction losses;
- splitter ratios;
- fluorescence lifetime and transfer curve;
- curing shrinkage;
- dimensional error;
- hydration drift;
- detector response and noise.

Write the measurements into versioned calibration YAML. The exporter must
record the calibration ID in every output.

### 12. Testing

Add:

- schema tests;
- topology round-trip tests;
- deterministic layout snapshots;
- property tests for delay and power equations;
- golden G-code tests per verified machine dialect;
- mesh manifold tests;
- collision tests;
- full example network exports;
- an optical digital-twin regression test against coupon measurements.

## Definition of done for version 1

Version 1 is complete when it can export a small, frozen AARNN network of
approximately 10–50 nodes into a reviewed multi-material print plan, with every
connection assigned:

- physical geometry;
- material IDs;
- polarity encoding;
- predicted attenuation;
- target and realised delay;
- node threshold implementation;
- provenance back to AARNN components;

and the exported job passes both software validation and a dry-run review in
DNA Studio without relying on undocumented commands.
