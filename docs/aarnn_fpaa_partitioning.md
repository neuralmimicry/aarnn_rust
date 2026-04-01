# AARNN FPAA Partitioning Notes

## Project intent

This repository is not only a spiking-network crate. It is a full neuromorphic platform with:

- a core Rust neural engine (`src/network.rs`, `src/sim.rs`, `src/runner.rs`)
- morphology-aware AARNN execution for the interactive/growth path
- robotics and Webots bridges
- distributed transports and orchestration
- operator, deployment, and experiment-control layers

The practical center of gravity is still the neural engine. `Runner` is the highest-fidelity execution path and is where the AARNN-specific behavior actually lives. The batch simulator in `src/sim.rs` is a lighter matrix-based reference path.

## Execution paths

### Batch path

Files:
- `src/main.rs`
- `src/network.rs`
- `src/sim.rs`

Characteristics:
- fixed hidden-layer sizes
- no runtime topology growth
- no morphology-aware path delays
- AARNN mode approximated through the same matrix machinery as STDP/Izh-style stepping

### Interactive / morphology path

Files:
- `src/runner.rs`
- `src/topology.rs`
- `src/morphology.rs`

Characteristics:
- runtime stateful stepping
- optional 3D growth
- optional morphology-aware routing and path lengths
- per-neuron biological heterogeneity
- online plasticity, homeostasis, neuromodulation, and structural effects

## AARNN-specific algorithms identified in the codebase

The AARNN path differs from the plain LIF/Izh path in these algorithm families:

1. Synaptic front-end filtering
- AMPA, NMDA, and GABA state variables
- NMDA voltage sensitivity
- per-neuron synaptic gain and neuromodulated excitability gain

2. Short-term plasticity (STP)
- utilization/resource variables (`u`, `x`)
- per-spike release scaling
- heterogeneous time constants via neuron biology

3. Active dendritic nonlinearities
- calcium-like integration
- plateau-like state
- branch-structure-dependent gain shaping

4. Gap-junction and field coupling
- mean-field electrical coupling fallback
- topology-local coupling with radius and inhibitory-only option
- volume-transmission-like modulation from neuromodulatory cells

5. Morphology-aware transmission
- axonal and dendritic path delay
- bouton latency and jitter
- distance attenuation
- dendrite-class modifiers (apical, basal, generic)
- myelination-dependent speedup
- ATP/fatigue-dependent slowdown

6. Long-timescale plasticity constraints
- triplet-like metaplastic gain applied to learning rate
- synaptic scaling
- Dale-law sign enforcement
- release-probability heterogeneity

7. Higher-level AARNN control loops
- thalamic gating
- sleep/dream replay
- perceptual prediction/error loop
- resonance / neuromodulator state updates
- growth / pruning / migration

The first six groups are the best candidates for future analog substitution because they are localized, stateful kernels with well-defined inputs and outputs.

## New module boundaries

A new namespace was introduced:

- `src/aarnn/mod.rs`
- `src/aarnn/dynamics.rs`
- `src/aarnn/plasticity.rs`
- `src/aarnn/transmission.rs`

### `src/aarnn/dynamics.rs`

Contains:
- decay precomputation
- synaptic filtering
- current sanitization limits
- gap-junction kernels
- volume-transmission field factors
- active dendritic compartment update

Recommended FPAA mapping:
- weighted synapse cells or FG-VMM for current injection
- OTA-C / switched-capacitor filters for AMPA/NMDA/GABA
- local transconductance couplers for gap junctions
- slow diffusor/bias mesh for volume transmission
- reconfigurable cable or dendrite macrocells for plateau gain

### `src/aarnn/plasticity.rs`

Contains:
- STP state update
- release-probability model
- triplet learning-rate modulation
- synaptic scaling
- Dale-law enforcement

Recommended FPAA mapping:
- local capacitor or floating-gate state for STP
- analog multiplier / translinear block for `u * x`
- slow supervisory calibration loop for synaptic scaling and Dale enforcement
- optional mixed-signal randomization for release variability

### `src/aarnn/transmission.rs`

Contains:
- deterministic jitter model
- distance- and morphology-aware delay/attenuation computation
- myelination and fatigue modifiers

Recommended FPAA mapping:
- configurable cable-delay chain or switched-capacitor delay line
- programmable gain stages for attenuation
- route-dependent conduction presets for myelination
- slow gain/delay bias adaptation for fatigue

## Why these boundaries matter

The original `Runner` carried these algorithms inline, which made the code hard to swap piecemeal. The refactor preserves orchestration in `Runner` but moves the biological kernels behind narrow data contracts.

That means future work can replace one layer at a time:

1. keep `Runner` as the orchestrator
2. replace one software kernel with an FPAA-backed implementation
3. keep the remaining kernels in software until their hardware versions are ready
4. compare both paths against the software reference using the same tests

## Suggested future substitutions

1. Replace `dynamics::apply_synaptic_filter` first
- highest leverage analog fit
- minimal graph boundary
- reusable in both batch and runner paths

2. Replace `plasticity::stp_step` next
- compact state machine
- local state and output only
- pairs naturally with the synaptic front-end

3. Replace `transmission::compute_delay_and_attenuation`
- useful once morphology-aware routing is available on hardware
- likely hybrid analog/digital in the first implementation

4. Keep synaptic scaling and Dale enforcement in software longer
- they are slower calibration functions
- they do not need to be in the nanosecond or microsecond datapath

## Verification status

The refactor keeps the original runner orchestration but routes the following behaviors through shared modules:

- batch synaptic filtering and STP
- runner synaptic filtering
- runner STP CPU path
- runner dendritic boosting
- runner local gap coupling
- runner volume transmission
- runner triplet gain calculation
- runner synaptic scaling and Dale enforcement
- runner morphology-aware delay/attenuation

The targeted morphology-enabled runner tests and the default neural-engine tests covering these kernels pass after the refactor. The only remaining failing test observed during validation is the pre-existing distributed transport regression in `src/distributed.rs`.
