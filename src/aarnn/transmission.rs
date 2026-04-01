//! Morphology-aware transmission kernels for AARNN.
//!
//! This module turns morphological path information into an effective conduction delay
//! and attenuation. The software implementation is explicit about every factor that
//! changes signal travel time: axonal path, dendritic path, bouton latency, jitter,
//! compartment class, myelination, and metabolic fatigue.
//!
//! FPAA replacement notes:
//! - Base delay can map to switched-capacitor delay lines, cascaded low-pass sections,
//!   or explicit wave-propagation cable cells.
//! - Distance attenuation maps to programmable gain loss along those cells.
//! - Myelination is naturally represented as a selectable conduction-gain path.
//! - Fatigue is a slow modulation term and is a good candidate for mixed-signal control.

/// Reduced dendrite class used by the delay/attenuation model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompartmentClass {
    Generic,
    Apical,
    Basal,
}

/// Additional dendrite-dependent modifiers for one synaptic path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DendriticTransmissionProfile {
    /// Which compartment family the postsynaptic segment belongs to.
    pub compartment: CompartmentClass,
    /// Trunk distance from soma used to stretch delays.
    pub trunk_length: f64,
    /// Forward transmission gain for this compartment family.
    pub forward_gain: f64,
    /// Backpropagating action potential gain for this compartment family.
    pub backprop_gain: f64,
    /// Whether the path represents a backward / feedback projection.
    pub is_backward_path: bool,
}

/// Myelination-dependent conduction parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MyelinationProfile {
    /// Current myelin state in `[0, 1]`.
    pub level: f64,
    /// Minimum conduction gain when unmyelinated.
    pub min_gain: f64,
    /// Maximum conduction gain when strongly myelinated.
    pub max_gain: f64,
}

/// Slow metabolic state used to stretch delays under fatigue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FatigueProfile {
    pub axon_atp: f64,
    pub dendrite_atp: f64,
}

/// Fully specified input bundle for computing one synaptic delay/attenuation pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayAttenuationSpec {
    /// AARNN algorithm depth that determines which biological refinements are active.
    pub depth: usize,
    /// Simulation time step in milliseconds.
    pub dt_ms: f64,
    /// Current simulation step used for deterministic jitter.
    pub time_seed: u64,
    /// Stable synapse identifier.
    pub synapse_index: usize,
    /// Precomputed axonal transport delay in whole steps.
    pub axon_steps: usize,
    /// Precomputed dendritic transport delay in whole steps.
    pub dendrite_steps: usize,
    /// Fixed bouton latency contribution.
    pub bouton_latency_steps: usize,
    /// Symmetric jitter amplitude in milliseconds.
    pub jitter_ms: f64,
    /// Distance-based attenuation coefficient.
    pub attenuation_per_unit: f64,
    /// Physical axonal path length.
    pub axon_length: f64,
    /// Physical dendritic path length.
    pub dendrite_length: f64,
    /// Characteristic length used to normalize path distance.
    pub path_length_scale: f64,
    /// Optional dendrite-specific modifiers.
    pub dendritic_profile: Option<DendriticTransmissionProfile>,
    /// Optional myelination state.
    pub myelination: Option<MyelinationProfile>,
    /// Optional slow metabolic fatigue state.
    pub fatigue: Option<FatigueProfile>,
}

/// Result of the transmission model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayAttenuationResult {
    pub steps: usize,
    pub attenuation: f64,
}

fn jitter_source(time_seed: u64, synapse_index: usize) -> f64 {
    let mut x: u64 = time_seed.wrapping_mul(0x9E3779B185EBCA87)
        ^ (synapse_index as u64).wrapping_mul(0xD2B74407B1CE6E93);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    let r = (x & 0xffff) as i32;
    (r - 32768) as f64 / 32768.0
}

/// Apply deterministic per-step conduction jitter.
///
/// The jitter is deterministic so replay and snapshot-based debugging remain stable.
/// A future analog backend may implement the same effect with device-noise envelopes or
/// small mixed-signal perturbations, but this function defines the reference behavior.
pub fn deterministic_jitter_steps(
    base_steps: usize,
    dt_ms: f64,
    jitter_ms: f64,
    synapse_index: usize,
    time_seed: u64,
) -> usize {
    let max_ms = jitter_ms.max(0.0);
    if max_ms <= 0.0 {
        return base_steps;
    }
    let max_j = (max_ms / dt_ms.max(1e-6)).round() as i32;
    if max_j == 0 {
        return base_steps;
    }
    let jitter = (jitter_source(time_seed, synapse_index) * max_j as f64).round() as i32;
    (base_steps as i32 + jitter).max(0) as usize
}

/// Compute the delay and attenuation for one morphology-aware synaptic path.
///
/// This function preserves the exact structure of the existing AARNN transport model
/// while isolating it behind a small data contract. That makes it straightforward to
/// substitute a compiled FPAA cable/delay macrocell in the future.
pub fn compute_delay_and_attenuation(spec: DelayAttenuationSpec) -> DelayAttenuationResult {
    let base_steps = spec.axon_steps + spec.dendrite_steps + spec.bouton_latency_steps;
    let mut steps = if spec.depth >= 2 {
        deterministic_jitter_steps(
            base_steps,
            spec.dt_ms,
            spec.jitter_ms,
            spec.synapse_index,
            spec.time_seed,
        )
    } else {
        base_steps
    };

    let mut attenuation = 1.0f64;
    let atten_k = spec.attenuation_per_unit.max(0.0);
    if atten_k > 0.0 {
        let dist = (spec.axon_length.max(0.0) + spec.dendrite_length.max(0.0)).max(0.0);
        let dist_scale = spec.path_length_scale.max(1.0e-3);
        let normalized_dist = if dist > 0.0 { dist / dist_scale } else { 0.0 };
        attenuation = (-atten_k * normalized_dist).exp().clamp(1.0e-2, 1.0);
    }

    if let Some(profile) = spec.dendritic_profile {
        let trunk_norm =
            (profile.trunk_length.max(0.0) / spec.path_length_scale.max(1.0e-3)).max(0.0);
        attenuation *= profile.forward_gain.clamp(0.25, 3.0);
        if profile.is_backward_path {
            attenuation *= profile.backprop_gain.clamp(0.25, 3.0);
            steps = ((steps as f64) / profile.backprop_gain.max(1.0e-3)).round() as usize;
        }
        let trunk_delay = match profile.compartment {
            CompartmentClass::Apical => 1.0 + 0.45 * trunk_norm,
            CompartmentClass::Basal => 1.0 + 0.20 * trunk_norm,
            CompartmentClass::Generic => 1.0 + 0.30 * trunk_norm,
        };
        steps = ((steps as f64) * trunk_delay.max(0.1)).round() as usize;
    }

    if let Some(myelin) = spec.myelination {
        let level = myelin.level.clamp(0.0, 1.0);
        let min_gain = myelin.min_gain.max(0.1);
        let max_gain = myelin.max_gain.max(min_gain + 1.0e-3);
        let conduction_gain = (min_gain + (max_gain - min_gain) * level).max(1.0e-3);
        steps = ((steps as f64) / conduction_gain).round() as usize;
        attenuation *= (0.9 + 0.1 * level).clamp(0.5, 1.1);
    }

    if spec.depth >= 3 {
        if let Some(fatigue) = spec.fatigue {
            let fatigue_level = (fatigue.axon_atp * fatigue.dendrite_atp).clamp(0.01, 1.0);
            if fatigue_level < 0.5 {
                steps = (steps as f64 * (1.0 + (0.5 - fatigue_level))).round() as usize;
            }
        }
    }

    DelayAttenuationResult { steps, attenuation }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_myelination_speeds_conduction() {
        let slow = compute_delay_and_attenuation(DelayAttenuationSpec {
            depth: 3,
            dt_ms: 1.0,
            time_seed: 0,
            synapse_index: 0,
            axon_steps: 10,
            dendrite_steps: 5,
            bouton_latency_steps: 1,
            jitter_ms: 0.0,
            attenuation_per_unit: 0.0,
            axon_length: 1.0,
            dendrite_length: 1.0,
            path_length_scale: 1.0,
            dendritic_profile: None,
            myelination: Some(MyelinationProfile {
                level: 0.0,
                min_gain: 1.0,
                max_gain: 3.0,
            }),
            fatigue: None,
        });
        let fast = compute_delay_and_attenuation(DelayAttenuationSpec {
            myelination: Some(MyelinationProfile {
                level: 1.0,
                min_gain: 1.0,
                max_gain: 3.0,
            }),
            ..DelayAttenuationSpec {
                depth: 3,
                dt_ms: 1.0,
                time_seed: 0,
                synapse_index: 0,
                axon_steps: 10,
                dendrite_steps: 5,
                bouton_latency_steps: 1,
                jitter_ms: 0.0,
                attenuation_per_unit: 0.0,
                axon_length: 1.0,
                dendrite_length: 1.0,
                path_length_scale: 1.0,
                dendritic_profile: None,
                myelination: None,
                fatigue: None,
            }
        });
        assert!(fast.steps < slow.steps);
    }
}
