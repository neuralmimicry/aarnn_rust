//! AARNN plasticity and constraint kernels.
//!
//! The functions here cover fast presynaptic resource dynamics, stochastic release,
//! and slow structural constraints that shape the weight matrices after online
//! learning. They are grouped together because they all describe how a synapse's
//! effective strength departs from the raw stored matrix value.
//!
//! FPAA replacement notes:
//! - STP state (`u`, `x`) maps well to capacitor charge or floating-gate bias stored
//!   locally at the synapse cell.
//! - Release probability can be realized by analog mismatch, tunable comparator noise,
//!   or a small mixed-signal supervisor.
//! - Dale enforcement and synaptic scaling are slower supervisory operations; they are
//!   realistic as hybrid background calibration loops even if the fast signal path is
//!   fully analog.

use ndarray::Array2;

/// State of one short-term plasticity channel.
///
/// `utilization` corresponds to the fraction of available resources recruited by a
/// spike, while `available_resources` tracks the remaining releasable pool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShortTermPlasticityState {
    pub utilization: f64,
    pub available_resources: f64,
}

/// Decay and baseline parameters for one short-term plasticity channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShortTermPlasticityParams {
    pub baseline_utilization: f64,
    pub recovery_decay: f64,
    pub facilitation_decay: f64,
}

/// Deterministically hash a `u64` seed into `[0, 1]`.
///
/// This is used for reproducible pseudo-random release heterogeneity and inferred
/// inhibitory/excitatory role assignment. A hardware analogue would typically use
/// calibrated device mismatch or nonvolatile offsets instead of recomputing a hash.
#[inline]
pub fn hash_to_unit(mut x: u64) -> f64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    (x as f64) / (u64::MAX as f64)
}

/// Advance one Tsodyks-Markram-like STP channel by one simulation step.
///
/// The returned value is the effective release fraction for the current step. In an
/// FPAA implementation the multiplicative `u * x` term is a natural fit for current
/// multipliers or translinear loops driven by two slow analog state variables.
pub fn stp_step(
    state: &mut ShortTermPlasticityState,
    spiked: bool,
    params: ShortTermPlasticityParams,
) -> f64 {
    state.utilization = state.utilization * params.facilitation_decay
        + params.baseline_utilization * (1.0 - params.facilitation_decay);
    state.available_resources =
        state.available_resources * params.recovery_decay + (1.0 - params.recovery_decay);
    if !spiked {
        return 0.0;
    }
    let rel = (state.utilization * state.available_resources).clamp(0.0, 1.0);
    state.available_resources = (state.available_resources - rel).max(0.0);
    state.utilization = (state.utilization
        + params.baseline_utilization * (1.0 - state.utilization))
        .clamp(0.0, 1.0);
    rel
}

/// Vectorized STP update over a spike slice.
///
/// The caller supplies the per-index parameters through `params_for_index`, which lets
/// the same reference implementation cover both the batch simulator's shared biology
/// and the runner's per-neuron heterogeneous biology.
pub fn stp_update_slice<F>(
    pre_spikes: &[i8],
    utilization: &mut [f64],
    available_resources: &mut [f64],
    release: &mut [f64],
    mut params_for_index: F,
) where
    F: FnMut(usize) -> ShortTermPlasticityParams,
{
    let n = pre_spikes
        .len()
        .min(utilization.len())
        .min(available_resources.len())
        .min(release.len());
    for i in 0..n {
        let mut state = ShortTermPlasticityState {
            utilization: utilization[i],
            available_resources: available_resources[i],
        };
        release[i] = stp_step(&mut state, pre_spikes[i] != 0, params_for_index(i));
        utilization[i] = state.utilization;
        available_resources[i] = state.available_resources;
    }
}

/// Compute the deterministic release probability used by morphology-aware routing.
///
/// `heterogeneity` spreads individual synapses around a common baseline without using
/// a runtime RNG. That keeps replay stable and makes it easier to compare a future
/// analog implementation against the software reference.
pub fn release_probability(
    base: f32,
    heterogeneity: f32,
    synapse_index: Option<usize>,
    time_seed: u64,
) -> f32 {
    let base = base.clamp(0.0, 1.0);
    let heterogeneity = heterogeneity.clamp(0.0, 1.0);
    if heterogeneity <= 0.0 {
        return base;
    }
    let seed = synapse_index
        .map(|idx| (idx as u64).wrapping_mul(0x9e3779b185ebca87))
        .unwrap_or_else(|| time_seed.wrapping_mul(0xd2b74407b1ce6e93));
    let delta = ((hash_to_unit(seed) * 2.0) - 1.0) as f32 * heterogeneity;
    (base + delta).clamp(0.0, 1.0)
}

/// Infer whether a presynaptic column should be treated as inhibitory.
///
/// This keeps Dale-law enforcement reproducible even before explicit cell types are
/// assigned. On hardware, a compiled FPAA design would normally bake this role into the
/// sign of a synapse macrocell or into the routing of inhibitory versus excitatory DACs.
pub fn is_inhibitory_presyn(pre_idx: usize, inhibitory_fraction: f64, salt: u64) -> bool {
    if inhibitory_fraction <= 0.0 {
        return false;
    }
    let seed = (pre_idx as u64)
        .wrapping_mul(0x9e3779b185ebca87)
        .wrapping_add(salt);
    hash_to_unit(seed) < inhibitory_fraction
}

/// Blend each matrix column toward a Dale-law-consistent sign assignment.
///
/// The sign target is inferred from `inhibitory_fraction` and a deterministic hash. The
/// operation is soft because `strictness` interpolates between the current value and the
/// sign-constrained value rather than snapping immediately.
pub fn enforce_dale_matrix_cols(
    mat: &mut Array2<f64>,
    inhibitory_fraction: f64,
    strictness: f64,
    max_abs_w: f64,
    salt: u64,
) {
    if strictness <= 0.0 || mat.is_empty() {
        return;
    }
    for j in 0..mat.nrows() {
        for i in 0..mat.ncols() {
            let w = mat[(j, i)];
            let inhibitory = is_inhibitory_presyn(i, inhibitory_fraction, salt);
            let target = if inhibitory { -w.abs() } else { w.abs() };
            let blended = w + strictness * (target - w);
            mat[(j, i)] = blended.clamp(-max_abs_w, max_abs_w);
        }
    }
}

/// Blend each matrix column toward a Dale-law-consistent sign assignment using an
/// explicit inhibitory mask.
pub fn enforce_dale_matrix_cols_with_mask(
    mat: &mut Array2<f64>,
    inhibitory_mask: &[bool],
    strictness: f64,
    max_abs_w: f64,
) {
    if strictness <= 0.0 || mat.is_empty() {
        return;
    }
    for j in 0..mat.nrows() {
        for i in 0..mat.ncols() {
            let w = mat[(j, i)];
            let inhibitory = inhibitory_mask.get(i).copied().unwrap_or(false);
            let target = if inhibitory { -w.abs() } else { w.abs() };
            let blended = w + strictness * (target - w);
            mat[(j, i)] = blended.clamp(-max_abs_w, max_abs_w);
        }
    }
}

/// Apply row-wise synaptic scaling toward a target summed absolute input strength.
///
/// This is a slow homeostatic operation. An FPAA system would likely implement it via a
/// background calibration pass that reads aggregate current and retunes floating-gate or
/// bias values, but the numerical objective belongs with the software model.
pub fn apply_synaptic_scaling_matrix_rows(mat: &mut Array2<f64>, strength: f64, target: f64) {
    if strength <= 0.0 || target <= 0.0 || mat.is_empty() {
        return;
    }
    for mut row in mat.axis_iter_mut(ndarray::Axis(0)) {
        let sum_abs = row.iter().map(|w| w.abs()).sum::<f64>();
        if sum_abs <= 1.0e-9 {
            continue;
        }
        let desired_ratio = (target / sum_abs).clamp(0.25, 4.0);
        let scale = 1.0 + strength * (desired_ratio - 1.0);
        for w in row.iter_mut() {
            *w *= scale;
        }
    }
}

/// Convert mean trace and rate signals into the multiplicative triplet plasticity gain.
///
/// Returning the scale as a standalone function keeps the biological heuristic explicit
/// and makes it easy to replace by a future analog metaplasticity block.
pub fn triplet_eta_scale(
    pre_mean: f64,
    post_mean: f64,
    rate_mean: f64,
    ltp_gain: f64,
    ltd_gain: f64,
) -> f64 {
    let triplet_mod = (ltp_gain.max(0.0) * pre_mean * post_mean) - (ltd_gain.max(0.0) * rate_mean);
    (1.0 + triplet_mod).clamp(0.05, 5.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stp_step_releases_only_on_spike() {
        let mut state = ShortTermPlasticityState {
            utilization: 0.2,
            available_resources: 1.0,
        };
        let params = ShortTermPlasticityParams {
            baseline_utilization: 0.2,
            recovery_decay: 0.9,
            facilitation_decay: 0.9,
        };
        let rel_spike = stp_step(&mut state, true, params);
        let rel_quiet = stp_step(&mut state, false, params);
        assert!(rel_spike > 0.0);
        assert_eq!(rel_quiet, 0.0);
    }

    #[test]
    fn test_triplet_gain_clamps() {
        let scale = triplet_eta_scale(10.0, 10.0, 0.0, 1.0, 0.0);
        assert_eq!(scale, 5.0);
    }
}
