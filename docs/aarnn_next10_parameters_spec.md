# AARNN Next-10 Biological Parameter Spec (Simulation + GA)

This document defines the next 10 AARNN parameters added to both simulation dynamics and GA search space.

## Scope

- Runtime config fields: `src/config.rs`
- Simulation hooks: `src/runner.rs`
- Rust UI controls/defaults: `src/ui.rs`
- GA randomization/crossover/mutation + plausibility prior scoring: `src/ga.rs`

## Parameters

| # | Parameter | Biological intent | Simulation hook | GA search range | GA plausibility prior |
|---|---|---|---|---|---|
| 1 | `aarnn_inhibitory_fraction` | Fraction of inhibitory presynaptic neurons | Used by Dale enforcement (`enforce_dale_constraints`) to assign inhibitory columns | `0.0..0.6` | `0.15..0.30` |
| 2 | `aarnn_dale_strictness` | Degree of Dale-law sign enforcement | Blends original weights toward sign-constrained weights | `0.0..1.0` | `0.70..1.00` |
| 3 | `aarnn_gap_junction_strength` | Electrical coupling between nearby neurons | Diffusive current term via `apply_gap_junction_coupling` | `0.0..0.2` | `0.005..0.06` |
| 4 | `aarnn_nmda_voltage_sensitivity` | Voltage-dependent NMDA gating | Additional voltage-gated NMDA contribution in `apply_synaptic_filter` | `0.0..0.2` | `0.02..0.12` |
| 5 | `aarnn_triplet_ltp_gain` | Extra potentiation tendency in triplet-like plasticity | Modulates effective learning gain in plastic update block | `0.0..2.0` | `0.10..0.80` |
| 6 | `aarnn_triplet_ltd_gain` | Extra depression tendency in triplet-like plasticity | Counterbalances LTP modulation in same block | `0.0..2.0` | `0.05..0.60` |
| 7 | `aarnn_synaptic_scaling_strength` | Homeostatic synaptic scaling strength | Row-wise scaling in `apply_synaptic_scaling` after plasticity | `0.0..0.2` | `0.005..0.08` |
| 8 | `aarnn_synaptic_scaling_target` | Target total incoming synaptic magnitude | Target setpoint for scaling normalization | `0.1..5.0` | `0.6..1.8` |
| 9 | `aarnn_distance_attenuation_per_unit` | Path-length-dependent attenuation | Applied in `syn_delay_and_atten` as exponential attenuation | `0.0..2.0` | `0.05..0.6` |
| 10 | `aarnn_release_prob_heterogeneity` | Synapse-to-synapse release variability | Per-synapse release probability in `release_probability` | `0.0..1.0` | `0.05..0.40` |

## Additional coupling prior

- GA also scores `abs(aarnn_triplet_ltp_gain - aarnn_triplet_ltd_gain)` with preferred range `0.0..0.5` to keep LTP/LTD in similar order while allowing mild asymmetry.

## Notes on biological fidelity

- These are tractable approximations inside the current solver architecture, not full conductance-based multi-compartment biophysics.
- Parameter priors are intentionally soft (granular scoring) so evolution can explore outside the preferred range when task fitness requires it.
