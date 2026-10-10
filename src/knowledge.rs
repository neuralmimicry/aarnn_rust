//! Knowledge-region support for Evelyn (transformer feed-forward layers hosted
//! as AARNN neuron meshes).
//!
//! Evelyn converts a transformer's feed-forward ("perceptron") layers into
//! populations of AARNN neurons. To stay faithful to AARNN's biology, the
//! conversion must target the dynamics AARNN actually runs, not an idealised
//! neuron. This module is the authoritative source of that information: it
//! measures each neuron model's steady-state transfer function (firing rate
//! against constant input current, the "f-I curve") by driving the crate's own
//! deterministic transition kernels ([`crate::neuron_kernels`]).
//!
//! The curves are exported as data, which Evelyn fits its population codes
//! against. AARNN keeps ownership of the dynamics and of the computational
//! detail selection (`aarnn_layer_depth`). Evelyn never re-implements them.
//!
//! Every current is measured independently, so curves are computed in parallel
//! across all available cores.

use crate::config::{AarnnBioParams, IzhikevichParams, LIFParams};
use crate::neuron_kernels::{izh_transition, lif_transition};
use serde::{Deserialize, Serialize};

/// A neuron configuration exactly as AARNN would instantiate it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KnowledgeNeuron {
    /// Leaky integrate-and-fire with the crate's LIF kernel.
    Lif(LIFParams),
    /// Izhikevich/AARNN dynamics. `detail_depth` follows `aarnn_layer_depth`:
    /// at depth 2 and above the adaptive firing threshold is active, using
    /// `bio`'s adaptive-threshold parameters, as in the batch simulator.
    Izhikevich {
        params: IzhikevichParams,
        bio: AarnnBioParams,
        detail_depth: usize,
    },
}

impl KnowledgeNeuron {
    /// Short stable label, e.g. `lif` or `izh-RS-depth2`.
    pub fn label(&self) -> String {
        match self {
            Self::Lif(_) => "lif".into(),
            Self::Izhikevich {
                bio, detail_depth, ..
            } => {
                format!("izh-{}-depth{}", bio.izh_preset, detail_depth)
            }
        }
    }
}

/// A measured transfer curve: `rates[i]` is spikes per simulation step at a
/// constant input current of `currents[i]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferCurve {
    pub neuron: String,
    pub currents: Vec<f64>,
    pub rates: Vec<f64>,
    /// Steps measured per point after `warmup` discarded steps.
    pub steps: usize,
    pub warmup: usize,
    /// Standard deviation of the Gaussian membrane-current noise applied each
    /// step (0 = noiseless). Biological membrane noise smooths the
    /// discrete-time f-I staircase (a constant drive can only produce
    /// integer firing periods) into a graded response. A knowledge region
    /// must run with the same noise its population codes were fitted to.
    #[serde(default)]
    pub noise_std: f64,
}

/// Deterministic xorshift64* generator, used so noisy measurements are
/// reproducible on every node.
struct NoiseRng(u64);

impl NoiseRng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn uniform(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Standard normal via Box-Muller.
    fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-300);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// Steady-state firing rate for one constant current.
fn rate_at(
    neuron: &KnowledgeNeuron,
    current: f64,
    steps: usize,
    warmup: usize,
    noise_std: f64,
    seed: u64,
) -> f64 {
    let mut fired_count = 0usize;
    let mut rng = NoiseRng::new(seed);
    let mut drive = |c: f64| {
        if noise_std > 0.0 {
            c + noise_std * rng.normal()
        } else {
            c
        }
    };
    match neuron {
        KnowledgeNeuron::Lif(p) => {
            let decay = (-p.dt / p.tau_m.max(1e-9)).exp();
            let (mut v, mut r) = (p.v_reset, 0i32);
            for t in 0..warmup + steps {
                let next = lif_transition(v, r, drive(current), decay, *p);
                v = next.voltage;
                r = next.refractory;
                if t >= warmup && next.fired {
                    fired_count += 1;
                }
            }
        }
        KnowledgeNeuron::Izhikevich {
            params,
            bio,
            detail_depth,
        } => {
            let adaptive = *detail_depth >= 2 && bio.adaptive_threshold_enabled;
            let refractory_steps = (bio.izh_refractory_ms / params.dt.max(1e-9)).round() as i32;
            let threshold_decay = (-params.dt / bio.adaptive_threshold_tau_ms.max(1e-9)).exp();
            let mut v = params.membrane_reset_potential_c;
            let mut u = params.recovery_sensitivity_b * v;
            let mut offset = 0.0f64;
            let mut refractory = Some(0i32);
            for t in 0..warmup + steps {
                let next = izh_transition(
                    v,
                    u,
                    drive(current),
                    *params,
                    offset,
                    adaptive,
                    bio.adaptive_threshold_increment,
                    bio.adaptive_threshold_min,
                    bio.adaptive_threshold_max,
                    refractory,
                    refractory_steps,
                );
                v = next.voltage;
                u = next.recovery;
                // The adaptive offset relaxes towards zero between spikes, as
                // in the simulator's threshold decay.
                offset = next.threshold_offset * threshold_decay;
                refractory = Some(next.refractory);
                if t >= warmup && next.fired {
                    fired_count += 1;
                }
            }
        }
    }
    fired_count as f64 / steps.max(1) as f64
}

/// Measure a noiseless transfer curve over `currents`, in parallel across cores.
pub fn transfer_curve(
    neuron: &KnowledgeNeuron,
    currents: &[f64],
    steps: usize,
    warmup: usize,
) -> TransferCurve {
    transfer_curve_noisy(neuron, currents, steps, warmup, 0.0)
}

/// Measure a transfer curve with Gaussian membrane-current noise of standard
/// deviation `noise_std`, in parallel across cores. Each current uses its own
/// deterministic noise stream, so results are reproducible.
pub fn transfer_curve_noisy(
    neuron: &KnowledgeNeuron,
    currents: &[f64],
    steps: usize,
    warmup: usize,
    noise_std: f64,
) -> TransferCurve {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = currents.len().div_ceil(threads).max(1);
    let rates = std::thread::scope(|s| {
        let handles: Vec<_> = currents
            .chunks(chunk)
            .enumerate()
            .map(|(ci, part)| {
                s.spawn(move || {
                    part.iter()
                        .enumerate()
                        .map(|(j, &c)| {
                            rate_at(
                                neuron,
                                c,
                                steps,
                                warmup,
                                noise_std,
                                0x9E37_79B9_7F4A_7C15 ^ ((ci * chunk + j) as u64 + 1),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("transfer-curve worker panicked"))
            .collect()
    });
    TransferCurve {
        neuron: neuron.label(),
        currents: currents.to_vec(),
        rates,
        steps,
        warmup,
        noise_std,
    }
}

/// Evenly spaced currents from `lo` to `hi` inclusive.
pub fn current_grid(lo: f64, hi: f64, points: usize) -> Vec<f64> {
    let n = points.max(2);
    (0..n)
        .map(|i| lo + (hi - lo) * i as f64 / (n - 1) as f64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lif_curve_is_monotone_with_rheobase_and_saturation() {
        let p = LIFParams::default();
        let curve = transfer_curve(
            &KnowledgeNeuron::Lif(p),
            &current_grid(0.0, 2.0, 81),
            4000,
            200,
        );
        // Discrete-time phase effects (refractory release against the
        // integration step) give sub-percent ripple at saturation. That is
        // genuine AARNN behaviour, so the check is monotone within 1 % of peak.
        let peak = curve.rates.iter().cloned().fold(0.0, f64::max);
        assert!(
            curve.rates.windows(2).all(|w| w[1] + 0.01 * peak >= w[0]),
            "f-I curve must be monotone within ripple"
        );
        assert_eq!(curve.rates[0], 0.0, "no firing without drive");
        let ceiling = 1.0 / (p.refractory as f64 + 1.0);
        assert!(
            *curve.rates.last().unwrap() <= ceiling + 1e-9,
            "refractory period caps the rate"
        );
        assert!(curve.rates.iter().any(|r| *r > 0.0));
    }

    #[test]
    fn izhikevich_detail_depth_changes_the_curve() {
        let params = IzhikevichParams::from_preset("RS", 1.0);
        let bio = AarnnBioParams::default();
        let grid = current_grid(0.0, 30.0, 31);
        let plain = transfer_curve(
            &KnowledgeNeuron::Izhikevich {
                params,
                bio: bio.clone(),
                detail_depth: 0,
            },
            &grid,
            3000,
            300,
        );
        let adaptive = transfer_curve(
            &KnowledgeNeuron::Izhikevich {
                params,
                bio,
                detail_depth: 2,
            },
            &grid,
            3000,
            300,
        );
        assert!(plain.rates.iter().any(|r| *r > 0.0));
        let (sp, sa): (f64, f64) = (plain.rates.iter().sum(), adaptive.rates.iter().sum());
        assert!(
            sa <= sp,
            "adaptive threshold can only reduce steady-state firing"
        );
    }

    #[test]
    fn membrane_noise_smooths_the_staircase() {
        let n = KnowledgeNeuron::Lif(LIFParams::default());
        let g = current_grid(0.0, 0.3, 121);
        let plain = transfer_curve(&n, &g, 4000, 200);
        let noisy = transfer_curve_noisy(&n, &g, 20_000, 500, 0.03);
        let distinct = |c: &TransferCurve| {
            let mut v: Vec<i64> = c.rates.iter().map(|r| (r * 1e4).round() as i64).collect();
            v.dedup();
            v.len()
        };
        assert!(
            distinct(&noisy) > 2 * distinct(&plain),
            "noise must grade the staircase"
        );
        // Just below the noiseless rheobase, noise occasionally lifts the
        // membrane over threshold; far below it (zero drive) it cannot.
        let below = plain
            .rates
            .iter()
            .rposition(|r| *r == 0.0)
            .expect("a silent region below rheobase");
        assert!(
            noisy.rates[below] > 0.0,
            "noise lets near-rheobase drive fire occasionally"
        );
        assert_eq!(noisy.rates[0], 0.0, "zero drive stays silent");
    }

    #[test]
    fn curves_are_deterministic() {
        let n = KnowledgeNeuron::Lif(LIFParams::default());
        let g = current_grid(0.0, 1.5, 17);
        assert_eq!(
            transfer_curve(&n, &g, 1000, 100).rates,
            transfer_curve(&n, &g, 1000, 100).rates
        );
    }
}

/// Steady-state firing rate (spikes per step) of one AARNN neuron under a
/// constant current with Gaussian membrane noise, simulated with the crate's
/// own kernels. This is the per-neuron primitive knowledge regions are built
/// from. `seed` selects an independent, reproducible noise stream.
pub fn steady_rate(
    neuron: &KnowledgeNeuron,
    current: f64,
    steps: usize,
    warmup: usize,
    noise_std: f64,
    seed: u64,
) -> f64 {
    rate_at(neuron, current, steps, warmup, noise_std, seed)
}
