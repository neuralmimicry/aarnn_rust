//! AARNN current-domain kernels.
//!
//! These functions describe how spike-driven synaptic currents are shaped before they
//! reach the membrane integrator. The software version is intentionally explicit and
//! deterministic so it can serve as a golden model for later analog replacements.
//!
//! FPAA replacement notes:
//! - Synaptic filtering maps naturally to OTA-C or switched-capacitor low-pass stages
//!   with separate AMPA/NMDA/GABA branches and current summation at the neuron input.
//! - Gap-junction coupling corresponds to programmable transconductance/resistive links
//!   between nearby membrane nodes.
//! - Volume transmission can be approximated by diffusor meshes or slow bias fields.
//! - Active dendritic compartments match reconfigurable cable/dendrite blocks plus a
//!   comparator-like plateau trigger and a controllable current gain stage.

use ndarray::Array1;
use std::collections::HashMap;

use crate::config::{AarnnBioParams, IzhikevichParams};

/// Precomputed exponential decays and derived gains for one biological parameter set.
///
/// The runner evaluates these coefficients once per time step configuration and then
/// reuses them throughout the inner loop. This keeps the simulation stable and makes
/// it obvious which continuous-time effects could be realized by analog RC/OTA time
/// constants instead of re-evaluated exponentials.
#[derive(Clone, Copy, Debug)]
pub struct AarnnDecays {
    /// Exponential recovery factor for the STP resource pool.
    pub stp_rec_decay: f64,
    /// Exponential relaxation factor for the STP utilization state.
    pub stp_facil_decay: f64,
    /// AMPA-like synaptic decay.
    pub syn_decay_ampa: f64,
    /// NMDA-like synaptic decay.
    pub syn_decay_nmda: f64,
    /// GABA-like synaptic decay.
    pub syn_decay_gaba: f64,
    /// Adaptive-threshold relaxation.
    pub thr_decay: f64,
    /// Homeostatic firing-rate EMA decay.
    pub homeo_decay: f64,
    /// Per-step target rate used by threshold homeostasis.
    pub base_homeo_target: f64,
    /// Refractory duration converted into integer simulation steps.
    pub izh_refractory_steps: i32,
    /// Plasticity gain implied by the neuromodulator baseline ratios.
    pub neuromod_plasticity_gain: f64,
    /// Excitability gain implied by acetylcholine-like modulation.
    pub neuromod_excitability_gain: f64,
    /// Izhikevich parameters derived from the selected preset and time step.
    pub izh_params: IzhikevichParams,
}

/// Small copyable bundle of the per-site values needed by the synaptic filter.
///
/// Keeping only the directly used fields here makes the filter a clean module
/// boundary. A future FPAA implementation would typically receive the same values as
/// programmable bias currents or floating-gate memories attached to a reusable synapse
/// macrocell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SynapticDriveParams {
    /// Fraction of excitatory charge routed into the slow NMDA branch.
    pub nmda_ratio: f64,
    /// Base gain applied after AMPA/NMDA/GABA combination.
    pub synaptic_gain: f64,
    /// AMPA branch decay factor.
    pub decay_ampa: f64,
    /// NMDA branch decay factor.
    pub decay_nmda: f64,
    /// GABA branch decay factor.
    pub decay_gaba: f64,
    /// Additional excitability gain from neuromodulation.
    pub neuromod_excitability_gain: f64,
}

impl SynapticDriveParams {
    /// Build the filter parameters for one site from the configured biology and the
    /// already precomputed decays.
    pub fn from_bio(bio: &AarnnBioParams, decays: AarnnDecays) -> Self {
        Self {
            nmda_ratio: bio.nmda_ratio,
            synaptic_gain: bio.synaptic_gain,
            decay_ampa: decays.syn_decay_ampa,
            decay_nmda: decays.syn_decay_nmda,
            decay_gaba: decays.syn_decay_gaba,
            neuromod_excitability_gain: decays.neuromod_excitability_gain,
        }
    }
}

/// Minimal 3D position used by field and coupling kernels.
///
/// The full topology stores richer metadata, but these kernels only need geometry.
/// That makes the numerical part easy to replace by an FPAA floorplan-aware mapper
/// that binds logical neurons onto nearby analog tiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialPoint3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Structure-dependent side-channel used by the active dendrite model.
///
/// `local_stimulus` acts like a slow trophic or structural support signal, while
/// `branching_gain` captures the extra nonlinear leverage of richer branching. An FPAA
/// realization could source these terms from a bias DAC, floating-gate memory, or a
/// slow envelope circuit tied to a reconfigurable dendrite network.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DendriteStructureSignal {
    pub local_stimulus: f64,
    pub branching_gain: f64,
}

/// Parameters required by the active dendritic compartment update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveDendriteSpec {
    pub enabled: bool,
    pub calcium_tau_ms: f64,
    pub plateau_tau_ms: f64,
    pub calcium_influx_gain: f64,
    pub plateau_threshold: f64,
    pub plateau_gain: f64,
}

impl ActiveDendriteSpec {
    /// Extract only the dendrite-specific values from the full biological profile.
    pub fn from_bio(bio: &AarnnBioParams) -> Self {
        Self {
            enabled: bio.dendritic_active_enabled,
            calcium_tau_ms: bio.dendritic_ca_tau_ms,
            plateau_tau_ms: bio.dendritic_plateau_tau_ms,
            calcium_influx_gain: bio.dendritic_ca_influx_gain,
            plateau_threshold: bio.dendritic_plateau_threshold,
            plateau_gain: bio.dendritic_plateau_gain,
        }
    }
}

/// Precompute all exponential decays and derived gains for one AARNN site.
///
/// This is the software analogue of configuring an FPAA tile with time constants,
/// bias currents, and preset neuron parameters before streaming activity through it.
pub fn precalculate_decays(dt_ms: f64, bio: &AarnnBioParams) -> AarnnDecays {
    AarnnDecays {
        stp_rec_decay: (-(dt_ms / bio.stp_tau_rec_ms.max(1e-6))).exp(),
        stp_facil_decay: (-(dt_ms / bio.stp_tau_facil_ms.max(1e-6))).exp(),
        syn_decay_ampa: (-(dt_ms / bio.ampa_tau_ms.max(1e-6))).exp(),
        syn_decay_nmda: (-(dt_ms / bio.nmda_tau_ms.max(1e-6))).exp(),
        syn_decay_gaba: (-(dt_ms / bio.gaba_tau_ms.max(1e-6))).exp(),
        thr_decay: (-(dt_ms / bio.adaptive_threshold_tau_ms.max(1e-6))).exp(),
        homeo_decay: (-(dt_ms / bio.homeostasis_tau_ms.max(1e-6))).exp(),
        base_homeo_target: bio.homeostasis_target_rate_hz * dt_ms / 1000.0,
        izh_refractory_steps: (bio.izh_refractory_ms / dt_ms.max(1e-6)).round() as i32,
        neuromod_plasticity_gain: if bio.neuromodulation_enabled {
            (bio.dopamine_gain / bio.serotonin_gain.max(1e-6)).max(0.0)
        } else {
            1.0
        },
        neuromod_excitability_gain: if bio.neuromodulation_enabled {
            bio.acetylcholine_gain.max(0.0)
        } else {
            1.0
        },
        izh_params: IzhikevichParams::from_preset(&bio.izh_preset, dt_ms),
    }
}

/// Apply AMPA/NMDA/GABA filtering to a raw synaptic current vector.
///
/// Each index is updated independently so callers can provide either one shared site
/// configuration or a different configuration per neuron. The three state vectors are
/// updated in-place and the combined current is returned.
///
/// An FPAA implementation would typically realize this block as three parallel analog
/// filters with weighted injection into a summing node, plus a voltage-sensitive NMDA
/// gate driven by the local membrane potential.
pub fn apply_synaptic_filter<F>(
    raw: &Array1<f64>,
    ampa: &mut Array1<f64>,
    nmda: &mut Array1<f64>,
    gaba: &mut Array1<f64>,
    vmem: Option<&Array1<f64>>,
    nmda_voltage_sensitivity: f64,
    mut params_for_index: F,
) -> Array1<f64>
where
    F: FnMut(usize) -> SynapticDriveParams,
{
    let mut out = Array1::<f64>::zeros(raw.len());
    for i in 0..raw.len() {
        let params = params_for_index(i);
        let val = raw[i];
        let exc = val.max(0.0);
        let inh = (-val).max(0.0);
        let nmda_gate = if nmda_voltage_sensitivity > 0.0 {
            let vm = vmem.and_then(|v| v.get(i)).copied().unwrap_or(0.0);
            let x = (nmda_voltage_sensitivity * (vm + 40.0)).clamp(-60.0, 60.0);
            1.0 / (1.0 + (-x).exp())
        } else {
            1.0
        };
        ampa[i] = ampa[i] * params.decay_ampa + exc * (1.0 - params.nmda_ratio);
        nmda[i] = nmda[i] * params.decay_nmda + exc * params.nmda_ratio * nmda_gate;
        gaba[i] = gaba[i] * params.decay_gaba + inh;
        out[i] = (ampa[i] + nmda[i] - gaba[i])
            * params.synaptic_gain
            * params.neuromod_excitability_gain;
    }
    out
}

/// Clamp one current sample into a numerically stable range.
///
/// In the software model this avoids runaway states and NaNs. In hardware the same
/// role would be performed by supply rails, source degeneration, or explicit limiter
/// cells, so this function defines the numerical envelope the digital model expects.
#[inline]
pub fn sanitize_current_value(i: f64) -> f64 {
    const I_ABS_LIMIT: f64 = 250.0;
    if !i.is_finite() {
        0.0
    } else {
        i.clamp(-I_ABS_LIMIT, I_ABS_LIMIT)
    }
}

/// Clamp every entry of a current vector with [`sanitize_current_value`].
#[inline]
pub fn sanitize_current_array(curr: &mut Array1<f64>) {
    for v in curr.iter_mut() {
        *v = sanitize_current_value(*v);
    }
}

/// Apply a mean-field gap-junction term using the layer-average membrane potential.
///
/// This is the cheapest fallback when no explicit geometry is available. It behaves
/// like a resistive coupling from each membrane node toward the layer mean.
pub fn apply_gap_junction_mean_field(curr: &mut Array1<f64>, v: &Array1<f64>, strength: f64) {
    if strength <= 0.0 || curr.len() < 2 || v.len() != curr.len() {
        return;
    }
    let mean_v = v.iter().sum::<f64>() / (v.len() as f64);
    for j in 0..curr.len() {
        curr[j] += strength * (mean_v - v[j]);
    }
}

/// Apply a locality-aware gap-junction term using explicit 3D positions.
///
/// Returns `true` if local coupling was applied and `false` if the caller should fall
/// back to a coarser approximation. The optional mask lets the caller restrict the
/// coupling network to inhibitory or otherwise special subpopulations.
///
/// Uses a spatial hash grid (cell side = `radius`) so each neuron only examines the
/// 27 neighbouring cells instead of all N neurons, reducing the cost from O(N²) to
/// O(N × k) where k is the average number of neurons within one radius.
///
/// In an FPAA this block could be implemented by short programmable resistive links or
/// transconductance couplers between neighboring cells that share a local routing island.
pub fn apply_local_gap_junction_coupling<P>(
    curr: &mut Array1<f64>,
    v: &Array1<f64>,
    strength: f64,
    radius: f64,
    inhibitory_mask: Option<&[bool]>,
    mut position_of: P,
) -> bool
where
    P: FnMut(usize) -> SpatialPoint3,
{
    if strength <= 0.0 || radius <= 0.0 || curr.len() < 2 || v.len() != curr.len() {
        return false;
    }

    let n = curr.len();

    // Collect all positions eagerly so we can reference them without re-calling
    // the closure (which may mutate external state).
    let positions: Vec<SpatialPoint3> = (0..n).map(|i| position_of(i)).collect();

    // Build a spatial hash grid with cell side = radius so that all neurons
    // reachable from j are in j's cell or one of its 26 face/edge/corner neighbours.
    let inv_r = 1.0 / radius;
    let cell_key = |p: &SpatialPoint3| -> (i64, i64, i64) {
        (
            (p.x * inv_r).floor() as i64,
            (p.y * inv_r).floor() as i64,
            (p.z * inv_r).floor() as i64,
        )
    };

    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for i in 0..n {
        if inhibitory_mask
            .and_then(|mask| mask.get(i))
            .copied()
            .is_some_and(|flag| !flag)
        {
            continue;
        }
        grid.entry(cell_key(&positions[i])).or_default().push(i);
    }

    let mut delta = vec![0.0f64; n];
    let mut local_edges = 0usize;
    let r2 = radius * radius;

    for j in 0..n {
        if inhibitory_mask
            .and_then(|mask| mask.get(j))
            .copied()
            .is_some_and(|flag| !flag)
        {
            continue;
        }
        let pj = &positions[j];
        let (cx, cy, cz) = cell_key(pj);
        let mut sum = 0.0f64;
        let mut wsum = 0.0f64;

        // Only examine the 3×3×3 = 27 cells that can contain neurons within `radius`.
        for ddz in -1i64..=1 {
            for ddy in -1i64..=1 {
                for ddx in -1i64..=1 {
                    let Some(candidates) = grid.get(&(cx + ddx, cy + ddy, cz + ddz)) else {
                        continue;
                    };
                    for &i in candidates {
                        if i == j {
                            continue;
                        }
                        let pi = &positions[i];
                        let dx = pi.x - pj.x;
                        let dy = pi.y - pj.y;
                        let dz = pi.z - pj.z;
                        let d2 = dx * dx + dy * dy + dz * dz;
                        if d2 <= r2 && d2 > 1.0e-18 {
                            let d = d2.sqrt();
                            let w = (1.0 - d / radius).max(0.0);
                            sum += w * (v[i] - v[j]);
                            wsum += w;
                            local_edges += 1;
                        }
                    }
                }
            }
        }
        if wsum > 1.0e-9 {
            delta[j] = strength * (sum / wsum);
        }
    }

    if local_edges == 0 {
        return false;
    }
    for j in 0..n {
        curr[j] += delta[j];
    }
    true
}

/// Compute a multiplicative volume-transmission field for one layer.
///
/// `sources` should contain the positions of active neuromodulatory neurons. The
/// returned vector is initialized to 1.0 so callers can multiply it directly into the
/// synaptic current. Analog arrays can approximate this behavior with slow diffusive
/// fields, shared bias lines, or subthreshold diffusor networks.
pub fn volume_transmission_factors_for_layer<P>(
    neuron_count: usize,
    radius: f64,
    strength: f64,
    tone: f64,
    sources: &[SpatialPoint3],
    mut position_of: P,
) -> Array1<f64>
where
    P: FnMut(usize) -> SpatialPoint3,
{
    // Collect positions once so the inner loop can be parallelised.
    let positions: Vec<SpatialPoint3> = (0..neuron_count).map(|j| position_of(j)).collect();
    volume_transmission_factors_from_positions(radius, strength, tone, sources, &positions)
}

/// Parallel-friendly variant that takes pre-computed positions.
pub fn volume_transmission_factors_from_positions(
    radius: f64,
    strength: f64,
    tone: f64,
    sources: &[SpatialPoint3],
    positions: &[SpatialPoint3],
) -> Array1<f64> {
    let neuron_count = positions.len();
    let mut factors = Array1::from_elem(neuron_count, 1.0);
    if neuron_count == 0 || radius <= 0.0 || strength <= 0.0 || sources.is_empty() {
        return factors;
    }

    let tone_scale = tone.clamp(0.0, 3.0) / 3.0;
    let two_sigma2 = 2.0 * radius * radius;
    let r2 = radius * radius;

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        // Use as_slice_mut() to get a standard &mut [f64] so we can zip with rayon.
        if let Some(slice) = factors.as_slice_mut() {
            slice
                .par_iter_mut()
                .zip(positions.par_iter())
                .for_each(|(f, p)| {
                    let mut field = 0.0f64;
                    for src in sources {
                        let dx = p.x - src.x;
                        let dy = p.y - src.y;
                        let dz = p.z - src.z;
                        let d2 = dx * dx + dy * dy + dz * dz;
                        if d2 <= r2 {
                            field += (-(d2 / two_sigma2)).exp();
                        }
                    }
                    *f = (1.0 + strength * tone_scale * field).clamp(0.5, 2.5);
                });
        } else {
            // Non-contiguous layout fallback (rare).
            for (f, p) in factors.iter_mut().zip(positions.iter()) {
                let mut field = 0.0f64;
                for src in sources {
                    let dx = p.x - src.x;
                    let dy = p.y - src.y;
                    let dz = p.z - src.z;
                    let d2 = dx * dx + dy * dy + dz * dz;
                    if d2 <= r2 {
                        field += (-(d2 / two_sigma2)).exp();
                    }
                }
                *f = (1.0 + strength * tone_scale * field).clamp(0.5, 2.5);
            }
        }
    }
    #[cfg(not(feature = "parallel"))]
    for (f, p) in factors.iter_mut().zip(positions.iter()) {
        let mut field = 0.0f64;
        for src in sources {
            let dx = p.x - src.x;
            let dy = p.y - src.y;
            let dz = p.z - src.z;
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 <= r2 {
                field += (-(d2 / two_sigma2)).exp();
            }
        }
        *f = (1.0 + strength * tone_scale * field).clamp(0.5, 2.5);
    }
    factors
}

/// Update one active dendritic compartment and reshape the local current.
///
/// The caller owns the calcium and plateau state variables. This function updates them
/// in place and multiplies the current if the compartment enters a plateau-like state.
/// A future FPAA macrocell could implement the same behavior using a leaky integrator,
/// threshold element, and a programmable current-gain path placed between dendrite and
/// soma summing node.
pub fn apply_active_dendritic_compartment(
    curr: &mut f64,
    calcium_state: &mut f64,
    plateau_state: &mut f64,
    dt_ms: f64,
    spec: ActiveDendriteSpec,
    structure: DendriteStructureSignal,
) {
    if !spec.enabled {
        return;
    }
    let tau_ca = spec.calcium_tau_ms.max(1.0);
    let tau_plateau = spec.plateau_tau_ms.max(1.0);
    let ca_decay = (-dt_ms.max(0.001) / tau_ca).exp();
    let plateau_decay = (-dt_ms.max(0.001) / tau_plateau).exp();
    let ca_influx = spec.calcium_influx_gain.max(0.0);
    let plateau_threshold = spec.plateau_threshold.max(0.0);
    let plateau_gain = spec.plateau_gain.max(0.0);
    if ca_influx <= 0.0 || plateau_gain <= 0.0 {
        return;
    }

    let branch_factor = structure.branching_gain.clamp(1.0, 3.0);
    let exc = (*curr).max(0.0);
    let drive = 0.75 * exc + 0.25 * structure.local_stimulus.max(0.0) * branch_factor;
    let ca = (*calcium_state * ca_decay + ca_influx * drive).clamp(0.0, 1.0e6);
    *calcium_state = ca;

    let over = (ca - plateau_threshold).max(0.0);
    let trigger = over / (1.0 + over);
    let plateau =
        (*plateau_state * plateau_decay + trigger * (1.0 - plateau_decay)).clamp(0.0, 1.0);
    *plateau_state = plateau;

    let gain = (1.0 + plateau_gain * plateau * branch_factor).clamp(1.0, 3.0);
    if *curr >= 0.0 {
        *curr *= gain;
    } else {
        *curr *= 1.0 + 0.25 * (gain - 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_gap_coupling_prefers_neighbors() {
        let mut curr = Array1::zeros(3);
        let v = Array1::from(vec![1.0, 0.0, 0.0]);
        let applied =
            apply_local_gap_junction_coupling(&mut curr, &v, 1.0, 0.2, None, |idx| match idx {
                0 => SpatialPoint3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                1 => SpatialPoint3 {
                    x: 0.05,
                    y: 0.0,
                    z: 0.0,
                },
                _ => SpatialPoint3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            });
        assert!(applied);
        assert!(curr[1] > curr[2]);
    }

    #[test]
    fn test_active_dendrite_increases_excitation() {
        let mut curr = 1.0;
        let mut ca = 0.0;
        let mut plateau = 0.0;
        apply_active_dendritic_compartment(
            &mut curr,
            &mut ca,
            &mut plateau,
            1.0,
            ActiveDendriteSpec {
                enabled: true,
                calcium_tau_ms: 10.0,
                plateau_tau_ms: 20.0,
                calcium_influx_gain: 2.0,
                plateau_threshold: 0.1,
                plateau_gain: 1.0,
            },
            DendriteStructureSignal {
                local_stimulus: 1.0,
                branching_gain: 2.0,
            },
        );
        assert!(curr >= 1.0);
        assert!(ca > 0.0);
    }
}
