// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
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
use serde::{Deserialize, Serialize};

/// One pure long-term-plasticity proposal for a matrix-owned synapse.
///
/// The proposal captures the weight observed while the biological transition was
/// evaluated. Commit validates that observation before mutating the authoritative
/// matrix, so a later shard-owned implementation cannot accidentally apply a stale
/// delta or let two writers update the same coordinate silently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightProposal {
    pub row: usize,
    pub col: usize,
    pub base_weight: f64,
    pub delta: f64,
}

impl WeightProposal {
    #[inline]
    pub const fn new(row: usize, col: usize, base_weight: f64, delta: f64) -> Self {
        Self {
            row,
            col,
            base_weight,
            delta,
        }
    }
}

/// Errors raised while committing a deterministic proposal batch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeightCommitError {
    InvalidNumericalValue {
        row: usize,
        col: usize,
    },
    OutOfBounds {
        row: usize,
        col: usize,
    },
    DuplicateCoordinate {
        row: usize,
        col: usize,
    },
    StaleBaseWeight {
        row: usize,
        col: usize,
        expected: f64,
        actual: f64,
    },
}

/// Commit a validated, canonically ordered proposal batch atomically.
///
/// Validation completes before the first matrix write. Proposals are sorted by
/// coordinate for deterministic commit order, duplicate coordinates are rejected,
/// and the captured base weight must still match the authoritative matrix. This is
/// the reference commit boundary for CPU and later shard-owned plasticity paths.
pub fn commit_weight_proposals(
    matrix: &mut Array2<f64>,
    proposals: &[WeightProposal],
    w_min: f64,
    w_max: f64,
) -> Result<usize, WeightCommitError> {
    let mut ordered = proposals.to_vec();
    ordered.sort_by_key(|proposal| (proposal.row, proposal.col));

    let mut previous = None;
    for proposal in &ordered {
        if !proposal.base_weight.is_finite() || !proposal.delta.is_finite() {
            return Err(WeightCommitError::InvalidNumericalValue {
                row: proposal.row,
                col: proposal.col,
            });
        }
        if proposal.row >= matrix.nrows() || proposal.col >= matrix.ncols() {
            return Err(WeightCommitError::OutOfBounds {
                row: proposal.row,
                col: proposal.col,
            });
        }
        let coordinate = (proposal.row, proposal.col);
        if previous == Some(coordinate) {
            return Err(WeightCommitError::DuplicateCoordinate {
                row: proposal.row,
                col: proposal.col,
            });
        }
        previous = Some(coordinate);
        let actual = matrix[coordinate];
        if actual.to_bits() != proposal.base_weight.to_bits() {
            return Err(WeightCommitError::StaleBaseWeight {
                row: proposal.row,
                col: proposal.col,
                expected: proposal.base_weight,
                actual,
            });
        }
    }

    for proposal in &ordered {
        let coordinate = (proposal.row, proposal.col);
        matrix[coordinate] = apply_weight_delta(proposal.base_weight, proposal.delta, w_min, w_max);
    }
    Ok(ordered.len())
}

/// Long-term synaptic rules supported by the reference biological kernels.
///
/// The arguments to [`weight_delta`] are expressed from the perspective of
/// one directed synapse: `pre_*` belongs to its source and `post_*` belongs to
/// its target. Keeping this calculation independent of matrices makes serial,
/// Rayon and later shard-owned implementations share one authoritative rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlasticityRule {
    Stdp,
    Hebb,
    Oja,
}

/// Calculate one bounded-step weight change without mutating model state.
///
/// STDP uses the conventional causal term `post * pre_trace` minus the
/// anti-causal term `pre * post_trace`. Clamping is deliberately left to
/// [`apply_weight_delta`] so proposal generation and commit remain separate.
#[inline]
pub fn weight_delta(
    rule: PlasticityRule,
    eta: f64,
    pre_spike: f64,
    post_spike: f64,
    pre_trace: f64,
    post_trace: f64,
    current_weight: f64,
) -> f64 {
    match rule {
        PlasticityRule::Stdp => eta * (post_spike * pre_trace - pre_spike * post_trace),
        PlasticityRule::Hebb => eta * post_spike * pre_spike,
        PlasticityRule::Oja => {
            eta * (post_spike * pre_spike - post_spike * post_spike * current_weight)
        }
    }
}

/// Smallest efficacy long-term depression leaves on an existing synapse.
///
/// LTD silences a synapse; it does not delete it. Removal is structural
/// (morphology pruning), so a depressed synapse keeps its place and its sign
/// and can potentiate again (de-depression).
pub const SILENT_SYNAPSE_FLOOR: f64 = 1.0e-6;

/// Apply one proposal to a weight using the configured finite bounds.
///
/// Long-term plasticity changes the efficacy of synapses that exist; it
/// never creates one. A weight of exactly zero is an absent synapse (the
/// connectome or growth did not make it) and stays zero: creating synapses
/// is structural plasticity, owned by growth. Letting a rate rule
/// potentiate empty matrix entries wired every co-active pair together
/// (C. elegans: 246 -> 12,381 of 14,496 input entries within 8 s), which
/// gave every cell the same drive and locked the network into runaway
/// synchrony.
///
/// `w_min..=w_max` bounds the synaptic *efficacy*. An inhibitory synapse is
/// stored with a negative sign, so its magnitude is bounded instead and a
/// positive (potentiating) proposal makes it more inhibitory. With
/// non-negative bounds an existing synapse is held at least
/// [`SILENT_SYNAPSE_FLOOR`] from zero, so it can neither vanish nor cross
/// zero and change sign. An imported efficacy above `w_max` is kept (it can
/// only be depressed until it is inside the bound) instead of being
/// truncated the first time a zero delta touches it.
#[inline]
pub fn apply_weight_delta(current_weight: f64, delta: f64, w_min: f64, w_max: f64) -> f64 {
    if current_weight == 0.0 {
        return current_weight;
    }
    let lo = w_min.min(w_max);
    let hi = w_max.max(w_min);
    if w_min >= 0.0 {
        let magnitude = current_weight.abs();
        let hi_eff = hi.max(magnitude);
        let lo_eff = lo.max(SILENT_SYNAPSE_FLOOR).min(hi_eff);
        return (magnitude + delta)
            .clamp(lo_eff, hi_eff)
            .copysign(current_weight);
    }
    // Explicitly signed bounds: the rule may move a synapse across zero.
    (current_weight + delta).clamp(lo.min(current_weight), hi.max(current_weight))
}

/// Per-cell multiplicative synaptic scaling toward a firing-rate set point.
///
/// `rate_error[j] = (set_point_j - rate_j) / set_point_j`, clamped to
/// `[-1, 1]`. Row `j` of `mat` holds the synapses onto cell `j`. Excitatory
/// synapses are scaled by `exp(k * e)` and inhibitory magnitudes by
/// `exp(-k * e)`: an over-active cell weakens its excitation and strengthens
/// its inhibition, an under-active cell the reverse (Turrigiano et al. 1998;
/// Kilman, van Rossum & Turrigiano 2002). Scaling is multiplicative - each
/// synapse in proportion to its strength (Turrigiano et al. 1998) - so it
/// keeps the relative weights that Hebbian/STDP learning wrote into the row,
/// keeps absent synapses absent and never changes a sign. Magnitudes stay within `[SILENT_SYNAPSE_FLOOR, max(w_max, |w|)]`.
pub fn homeostatic_scale_rows(mat: &mut Array2<f64>, rate_error: &[f64], k: f64, w_max: f64) {
    if k <= 0.0 || mat.is_empty() {
        return;
    }
    for (j, mut row) in mat.axis_iter_mut(ndarray::Axis(0)).enumerate() {
        let e = match rate_error.get(j) {
            Some(e) if e.is_finite() && *e != 0.0 => e.clamp(-1.0, 1.0),
            _ => continue,
        };
        let up = (k * e).exp();
        let down = (-k * e).exp();
        for w in row.iter_mut() {
            if *w == 0.0 {
                continue;
            }
            let magnitude = w.abs();
            let scaled = magnitude * if *w > 0.0 { up } else { down };
            let hi = w_max.abs().max(magnitude);
            *w = scaled.clamp(SILENT_SYNAPSE_FLOOR.min(hi), hi).copysign(*w);
        }
    }
}

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

/// Return the deterministic uniform variate used for a synaptic release
/// decision.  Release is part of the biological event stream, so it must not
/// depend on thread scheduling, GPU work-group order, or the process-global
/// `fastrand` state.  The two coordinates are deliberately explicit: a
/// synapse may release on one logical step and fail on the next while replaying
/// the same trace produces the same result.
#[inline]
pub fn release_draw(synapse_index: usize, time_step: u64) -> f32 {
    let seed = (synapse_index as u64)
        .wrapping_mul(0x9e3779b185ebca87)
        .wrapping_add(time_step.wrapping_mul(0xd2b74407b1ce6e93));
    hash_to_unit(seed) as f32
}

/// Evaluate one deterministic probabilistic release decision.
#[inline]
pub fn should_release(base: f32, heterogeneity: f32, synapse_index: usize, time_step: u64) -> bool {
    release_draw(synapse_index, time_step)
        <= release_probability(base, heterogeneity, Some(synapse_index), time_step)
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

/// Lowest set point a cell defends (Hz): a cell silent over the warm-up still
/// resists being driven into runaway firing by learning.
pub const MIN_SET_POINT_HZ: f64 = 1.0;

/// Firing-rate state of the homeostatic counterweight, one population per
/// hidden layer followed by the output population.
///
/// Each cell's rate is an exponential average of its activity (spikes, or a
/// graded cell's expected spikes). After the warm-up the cell's set point is
/// fixed - its own average rate, or a common target - and the scaling
/// error is `(set_point - rate) / set_point`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RateHomeostat {
    pub rate_hz: Vec<Vec<f64>>,
    /// Negative until the cell's set point is taken.
    pub set_point_hz: Vec<Vec<f64>>,
    /// Averaging time per population (ms).
    pub age_ms: Vec<f64>,
    /// Time since the last scaling pass (ms).
    pub since_scale_ms: f64,
}

impl RateHomeostat {
    /// Match the population sizes; a population whose size changed (growth,
    /// pruning) restarts its warm-up so set points never belong to the wrong
    /// cell.
    pub fn fit(&mut self, sizes: &[usize]) {
        self.rate_hz.resize_with(sizes.len(), Vec::new);
        self.set_point_hz.resize_with(sizes.len(), Vec::new);
        self.age_ms.resize(sizes.len(), 0.0);
        for (p, &n) in sizes.iter().enumerate() {
            if self.rate_hz[p].len() != n || self.set_point_hz[p].len() != n {
                self.rate_hz[p] = vec![0.0; n];
                self.set_point_hz[p] = vec![-1.0; n];
                self.age_ms[p] = 0.0;
            }
        }
    }

    /// Advance population `pop` by one step of `dt_ms`; `activity[j]` is
    /// cell `j`'s spikes this step (or expected spikes, in [0, 1]).
    pub fn observe(
        &mut self,
        pop: usize,
        activity: &[f64],
        dt_ms: f64,
        tau_ms: f64,
        warmup_ms: f64,
        target_hz: f64,
    ) {
        let (Some(rates), Some(sets)) = (self.rate_hz.get_mut(pop), self.set_point_hz.get_mut(pop))
        else {
            return;
        };
        if dt_ms <= 0.0 {
            return;
        }
        let alpha = 1.0 - (-dt_ms / tau_ms.max(dt_ms)).exp();
        for (r, a) in rates.iter_mut().zip(activity) {
            *r += alpha * (a.clamp(0.0, 1.0) * 1000.0 / dt_ms - *r);
        }
        let age = &mut self.age_ms[pop];
        *age += dt_ms;
        if *age >= warmup_ms {
            for (s, r) in sets.iter_mut().zip(rates.iter()) {
                if *s < 0.0 {
                    *s = if target_hz > 0.0 {
                        target_hz
                    } else {
                        r.max(MIN_SET_POINT_HZ)
                    };
                }
            }
        }
    }

    /// Per-cell scaling error of population `pop`, or `None` before its
    /// set points exist.
    pub fn rate_error(&self, pop: usize) -> Option<Vec<f64>> {
        let sets = self.set_point_hz.get(pop)?;
        if sets.is_empty() || sets.iter().any(|s| *s < 0.0) {
            return None;
        }
        Some(
            sets.iter()
                .zip(&self.rate_hz[pop])
                .map(|(s, r)| ((s - r) / s).clamp(-1.0, 1.0))
                .collect(),
        )
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
    fn weight_delta_preserves_inhibitory_sign_and_bounds_magnitude() {
        // Zero delta leaves an inhibitory synapse untouched (was clamped to 0).
        assert_eq!(apply_weight_delta(-0.4, 0.0, 0.0, 1.0), -0.4);
        // Potentiation strengthens inhibition; depression weakens it.
        assert!((apply_weight_delta(-0.4, 0.1, 0.0, 1.0) + 0.5).abs() < 1e-12);
        assert!((apply_weight_delta(-0.4, -0.1, 0.0, 1.0) + 0.3).abs() < 1e-12);
        // Magnitude bounds apply to inhibitory synapses too; depression
        // silences a synapse but keeps it (and its sign).
        assert_eq!(apply_weight_delta(-0.95, 0.2, 0.0, 1.0), -1.0);
        assert_eq!(
            apply_weight_delta(-0.05, -0.2, 0.0, 1.0),
            -SILENT_SYNAPSE_FLOOR
        );
        assert_eq!(apply_weight_delta(0.95, 0.2, 0.0, 1.0), 1.0);
        assert_eq!(
            apply_weight_delta(0.05, -0.2, 0.0, 1.0),
            SILENT_SYNAPSE_FLOOR
        );
        // A silenced inhibitory synapse stays inhibitory when it recovers.
        let silent = apply_weight_delta(-0.05, -0.2, 0.0, 1.0);
        assert!(apply_weight_delta(silent, 0.1, 0.0, 1.0) < 0.0);
        // Explicit signed bounds are honoured as before.
        assert_eq!(apply_weight_delta(-0.5, -0.7, -1.0, 1.0), -1.0);
    }

    #[test]
    fn plasticity_never_creates_a_synapse() {
        // An absent synapse (exactly zero) stays absent under any proposal,
        // with non-negative or signed bounds; creation belongs to growth.
        assert_eq!(apply_weight_delta(0.0, 0.3, 0.0, 1.0), 0.0);
        assert_eq!(apply_weight_delta(0.0, -0.3, 0.0, 1.0), 0.0);
        assert_eq!(apply_weight_delta(0.0, 0.3, -1.0, 1.0), 0.0);
        let mut matrix = Array2::from_shape_vec((1, 3), vec![0.0, 0.2, -0.2]).unwrap();
        let proposals = [
            WeightProposal::new(0, 0, 0.0, 0.5),
            WeightProposal::new(0, 1, 0.2, 0.5),
            WeightProposal::new(0, 2, -0.2, 0.5),
        ];
        commit_weight_proposals(&mut matrix, &proposals, 0.0, 1.0).unwrap();
        assert_eq!(matrix[(0, 0)], 0.0);
        assert!((matrix[(0, 1)] - 0.7).abs() < 1e-12);
        assert!((matrix[(0, 2)] + 0.7).abs() < 1e-12);
    }

    #[test]
    fn imported_efficacy_above_the_bound_is_not_truncated() {
        // Connectome weights may exceed w_max; a zero delta keeps them and
        // potentiation cannot push them further.
        assert_eq!(apply_weight_delta(1.4, 0.0, 0.0, 1.0), 1.4);
        assert_eq!(apply_weight_delta(1.4, 0.2, 0.0, 1.0), 1.4);
        assert!((apply_weight_delta(1.4, -0.1, 0.0, 1.0) - 1.3).abs() < 1e-12);
        assert_eq!(apply_weight_delta(-1.4, 0.2, 0.0, 1.0), -1.4);
    }

    #[test]
    fn homeostatic_scaling_opposes_the_rate_error_and_keeps_structure() {
        // Row 0 fires above its set point (error -1), row 1 below (+0.5),
        // row 2 at its set point.
        let mut mat =
            Array2::from_shape_vec((3, 3), vec![0.4, -0.2, 0.0, 0.4, -0.2, 0.0, 0.4, -0.2, 0.0])
                .unwrap();
        homeostatic_scale_rows(&mut mat, &[-1.0, 0.5, 0.0], 0.1, 1.0);
        // Over-active: excitation down, inhibition up.
        assert!(mat[(0, 0)] < 0.4 && mat[(0, 0)] > 0.0);
        assert!(mat[(0, 1)] < -0.2);
        // Under-active: excitation up, inhibition down.
        assert!(mat[(1, 0)] > 0.4);
        assert!(mat[(1, 1)] > -0.2 && mat[(1, 1)] < 0.0);
        // At the set point nothing changes; absent synapses stay absent.
        assert_eq!(mat[(2, 0)], 0.4);
        assert_eq!(mat[(2, 1)], -0.2);
        for j in 0..3 {
            assert_eq!(mat[(j, 2)], 0.0);
        }
        // Multiplicative: the ratio between a row's excitatory synapses is kept.
        let mut row = Array2::from_shape_vec((1, 2), vec![0.2, 0.6]).unwrap();
        homeostatic_scale_rows(&mut row, &[-0.5], 0.2, 1.0);
        assert!((row[(0, 1)] / row[(0, 0)] - 3.0).abs() < 1e-12);
        // Errors are clamped to [-1, 1] and the bound holds.
        let mut big = Array2::from_shape_vec((1, 1), vec![0.9]).unwrap();
        homeostatic_scale_rows(&mut big, &[50.0], 5.0, 1.0);
        assert_eq!(big[(0, 0)], 1.0);
    }

    #[test]
    fn rate_homeostat_takes_individual_set_points_after_warm_up() {
        let mut h = RateHomeostat::default();
        h.fit(&[2]);
        // Cell 0 fires every 10th 1 ms step (100 Hz), cell 1 is silent.
        for step in 0..3000 {
            let a = [if step % 10 == 0 { 1.0 } else { 0.0 }, 0.0];
            h.observe(0, &a, 1.0, 200.0, 2000.0, 0.0);
            if step < 1990 {
                assert!(h.rate_error(0).is_none(), "no scaling during the warm-up");
            }
        }
        let e = h.rate_error(0).expect("set points after the warm-up");
        assert!((h.set_point_hz[0][0] - 100.0).abs() < 15.0);
        assert_eq!(h.set_point_hz[0][1], MIN_SET_POINT_HZ);
        assert!(e[0].abs() < 0.2, "a cell at its own rate has ~no error");
        // Learning drives cell 1 to 50 Hz: error saturates at -1.
        for _ in 0..2000 {
            h.observe(0, &[0.1, 0.05], 1.0, 200.0, 2000.0, 0.0);
        }
        assert_eq!(h.rate_error(0).unwrap()[1], -1.0);
        // A resized population restarts its warm-up.
        h.fit(&[3]);
        assert!(h.rate_error(0).is_none());
    }

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

    #[test]
    fn release_decision_is_replayable_and_index_sensitive() {
        let first = should_release(0.5, 0.2, 7, 11);
        assert_eq!(first, should_release(0.5, 0.2, 7, 11));
        assert_ne!(release_draw(7, 11).to_bits(), release_draw(7, 12).to_bits());
    }

    #[test]
    fn weight_delta_is_directional_and_bounded_only_at_commit() {
        let delta = weight_delta(PlasticityRule::Stdp, 0.5, 1.0, 0.0, 0.25, 0.75, 0.4);
        assert_eq!(delta, -0.375);
        assert_eq!(
            apply_weight_delta(0.1, delta, 0.0, 1.0),
            SILENT_SYNAPSE_FLOOR,
            "the rule emits a proposal; the commit boundary owns clamping"
        );
    }

    #[test]
    fn weight_delta_matches_the_reference_rules() {
        assert_eq!(
            weight_delta(PlasticityRule::Hebb, 0.2, 1.0, 1.0, 9.0, 9.0, 0.5),
            0.2
        );
        assert_eq!(
            weight_delta(PlasticityRule::Oja, 0.2, 1.0, 1.0, 9.0, 9.0, 0.5),
            0.1
        );
    }

    #[test]
    fn proposal_commit_is_canonical_and_rejects_stale_writers_without_mutation() {
        let mut matrix = Array2::from_shape_vec((1, 2), vec![0.2, 0.4]).unwrap();
        let proposals = [
            WeightProposal::new(0, 1, 0.4, 0.3),
            WeightProposal::new(0, 0, 0.2, -0.5),
        ];
        assert_eq!(
            commit_weight_proposals(&mut matrix, &proposals, 0.0, 1.0),
            Ok(2)
        );
        assert_eq!(matrix[(0, 0)], SILENT_SYNAPSE_FLOOR);
        assert_eq!(matrix[(0, 1)], 0.7);

        let before = matrix.clone();
        let stale = [WeightProposal::new(0, 0, 0.2, 0.1)];
        assert!(matches!(
            commit_weight_proposals(&mut matrix, &stale, 0.0, 1.0),
            Err(WeightCommitError::StaleBaseWeight { .. })
        ));
        assert_eq!(matrix, before);
    }

    #[test]
    fn proposal_commit_rejects_duplicate_coordinates_before_any_write() {
        let mut matrix = Array2::from_shape_vec((1, 1), vec![0.2]).unwrap();
        let before = matrix.clone();
        let proposals = [
            WeightProposal::new(0, 0, 0.2, 0.1),
            WeightProposal::new(0, 0, 0.2, 0.2),
        ];
        assert_eq!(
            commit_weight_proposals(&mut matrix, &proposals, 0.0, 1.0),
            Err(WeightCommitError::DuplicateCoordinate { row: 0, col: 0 })
        );
        assert_eq!(matrix, before);
    }
}
