//! Network weight container and randomized initialization.
//!
//! This module builds the initial synaptic weight matrices used by both the
//! batch simulation (`sim.rs`) and the interactive Runner (`runner.rs`).
//!
//! Shapes (rows × cols)
//! - `w_in`: (H0 × S)         — sensory → first hidden layer
//! - `w_hh_fwd[l]`: (H × H)   — hidden l → hidden l+1 (forward)
//! - `w_hh_bwd[l]`: (H × H)   — hidden l+1 → hidden l (backward)
//! - `w_out`: (O × H_last)    — last hidden → output
//!
//! Guarantees
//! - At startup, every sensory input column connects to at least one H0 neuron
//!   (a small positive weight is injected if a column would otherwise be all zero).
//! - Probabilities `p_in`, `p_hidden`, and `p_out` control sparsity.
//!
use ndarray::Array2;
use rand::{Rng, RngExt};

use crate::config::NetworkConfig;

/// Container for the initialized synaptic weight matrices of the neural network.
///
/// Synaptic weights are represented as 2D matrices where rows typically represent
/// the post-synaptic neurons and columns represent pre-synaptic neurons.
pub struct BuiltNetwork {
    /// Synaptic weights from the sensory (input) layer to the first hidden layer (H0).
    /// Shape: (Number of Neurons in H0 × Number of Sensory Neurons)
    pub w_in: Array2<f64>,
    /// Forward synaptic weights between adjacent hidden layers (L -> L+1).
    /// Each element in the vector is a matrix for one layer interface.
    /// Shape: (Neurons in L+1 × Neurons in L)
    pub w_hh_fwd: Vec<Array2<f64>>,
    /// Backward (feedback) synaptic weights between adjacent hidden layers (L+1 -> L).
    /// Shape: (Neurons in L × Neurons in L+1)
    pub w_hh_bwd: Vec<Array2<f64>>,
    /// Recurrent synaptic weights within the same hidden layer (L -> L).
    /// Shape: (Neurons in L × Neurons in L)
    pub w_hh_rec: Vec<Array2<f64>>,
    /// Synaptic weights from the last hidden layer to the output layer.
    /// Shape: (Number of Output Neurons × Number of Neurons in Last Hidden Layer)
    pub w_out: Array2<f64>,
}

/// Constructs a randomized neural network based on the provided configuration.
///
/// This function handles:
/// 1. Allocating memory for weight matrices based on layer sizes.
/// 2. Randomly initializing weights according to connection probabilities (`p_in`, `p_hidden`, `p_out`).
/// 3. Ensuring "sensory coverage" - every input neuron is guaranteed to have at least one
///    outgoing connection to the first hidden layer, preventing dead input channels.
///
/// # Arguments
/// * `cfg`: The network configuration defining layer sizes and probabilities.
/// * `rng`: A random number generator for weight and connection sampling.
pub fn build_network<R: Rng>(cfg: &NetworkConfig, rng: &mut R) -> BuiltNetwork {
    let num_hidden_layers = cfg.num_hidden_layers;
    let num_hidden_per_layer = cfg.num_hidden_per_layer_initial;
    let num_sensory_neurons = cfg.num_sensory_neurons;
    let num_output_neurons = cfg.num_output_neurons;

    // num_sensory_neurons -> H1
    let mut w_in = Array2::<f64>::zeros((num_hidden_per_layer, num_sensory_neurons));
    let mut sensory_conn_counts = vec![0; num_sensory_neurons];
    for j in 0..num_hidden_per_layer {
        for i in 0..num_sensory_neurons {
            if sensory_conn_counts[i] < 6 && rng.random::<f64>() < cfg.p_in {
                w_in[(j, i)] = rng.random::<f64>() * 0.3 + 0.1;
                sensory_conn_counts[i] += 1;
            }
        }
    }
    // Ensure every sensory input connects to at least one hidden-0 neuron at startup.
    // 1:1 is not required; we only guarantee coverage. Applies to all modes.
    if num_hidden_per_layer > 0 {
        for i in 0..num_sensory_neurons {
            if sensory_conn_counts[i] == 0 {
                let j = if num_hidden_per_layer == 1 {
                    0
                } else {
                    ((rng.random::<f64>() * (num_hidden_per_layer as f64)) as usize)
                        .min(num_hidden_per_layer - 1)
                };
                // Seed a small positive weight consistent with other initializations.
                // We don't check for 6 here because it's only called if sensory_conn_counts[i] == 0.
                w_in[(j, i)] = rng.random::<f64>() * 0.3 + 0.1;
                sensory_conn_counts[i] += 1;
            }
        }
    }

    // num_hidden_per_layer -> num_hidden_per_layer (forward/backward/recurrent)
    let mut w_hh_fwd = Vec::with_capacity(num_hidden_layers.saturating_sub(1));
    let mut w_hh_bwd = Vec::with_capacity(num_hidden_layers.saturating_sub(1));
    let mut w_hh_rec = Vec::with_capacity(num_hidden_layers);

    for _l in 0..num_hidden_layers {
        let mut wr = Array2::<f64>::zeros((num_hidden_per_layer, num_hidden_per_layer));
        for j in 0..num_hidden_per_layer {
            for i in 0..num_hidden_per_layer {
                if rng.random::<f64>() < cfg.p_hidden {
                    wr[(j, i)] = rng.random::<f64>() * 0.3 + 0.1;
                }
            }
        }
        w_hh_rec.push(wr);

        if _l < num_hidden_layers.saturating_sub(1) {
            let mut wf = Array2::<f64>::zeros((num_hidden_per_layer, num_hidden_per_layer));
            let mut wb = Array2::<f64>::zeros((num_hidden_per_layer, num_hidden_per_layer));
            for j in 0..num_hidden_per_layer {
                for i in 0..num_hidden_per_layer {
                    if rng.random::<f64>() < cfg.p_hidden {
                        wf[(j, i)] = rng.random::<f64>() * 0.3 + 0.1;
                    }
                    if rng.random::<f64>() < cfg.p_hidden {
                        wb[(j, i)] = rng.random::<f64>() * 0.3 + 0.1;
                    }
                }
            }
            w_hh_fwd.push(wf);
            w_hh_bwd.push(wb);
        }
    }

    // H_L -> num_output_neurons
    let mut w_out = Array2::<f64>::zeros((num_output_neurons, num_hidden_per_layer));
    for k in 0..num_output_neurons {
        for j in 0..num_hidden_per_layer {
            if rng.random::<f64>() < cfg.p_out {
                w_out[(k, j)] = rng.random::<f64>() * 0.3 + 0.1;
            }
        }
    }

    let n_in = w_in.iter().filter(|&&w| w > 0.0).count();
    let mut n_hh = 0;
    for m in &w_hh_fwd {
        n_hh += m.iter().filter(|&&w| w > 0.0).count();
    }
    for m in &w_hh_bwd {
        n_hh += m.iter().filter(|&&w| w > 0.0).count();
    }
    for m in &w_hh_rec {
        n_hh += m.iter().filter(|&&w| w > 0.0).count();
    }
    let n_out = w_out.iter().filter(|&&w| w > 0.0).count();
    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
        nm_log!(
            "[trace] network initialized with {} input, {} hidden, and {} output synapses",
            n_in,
            n_hh,
            n_out
        );
    }

    BuiltNetwork {
        w_in,
        w_hh_fwd,
        w_hh_bwd,
        w_hh_rec,
        w_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;
    use rand::SeedableRng;

    #[test]
    fn test_build_network_shapes() {
        let mut cfg = NetworkConfig::default();
        cfg.num_sensory_neurons = 10;
        cfg.num_hidden_layers = 2;
        cfg.num_hidden_per_layer_initial = 5;
        cfg.num_output_neurons = 3;

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let net = build_network(&cfg, &mut rng);

        assert_eq!(net.w_in.shape(), &[5, 10]);
        assert_eq!(net.w_hh_fwd.len(), 1);
        assert_eq!(net.w_hh_fwd[0].shape(), &[5, 5]);
        assert_eq!(net.w_hh_bwd.len(), 1);
        assert_eq!(net.w_hh_bwd[0].shape(), &[5, 5]);
        assert_eq!(net.w_hh_rec.len(), 2);
        assert_eq!(net.w_hh_rec[0].shape(), &[5, 5]);
        assert_eq!(net.w_out.shape(), &[3, 5]);
    }

    #[test]
    fn test_sensory_coverage() {
        let mut cfg = NetworkConfig::default();
        cfg.num_sensory_neurons = 100;
        cfg.num_hidden_per_layer_initial = 1;
        cfg.p_in = 0.0; // Force zero probability to test coverage logic

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let net = build_network(&cfg, &mut rng);

        // Every sensory neuron must have at least one connection to H0
        for i in 0..cfg.num_sensory_neurons {
            let col = net.w_in.column(i);
            assert!(
                col.iter().any(|&w| w > 0.0),
                "Sensory neuron {} has no connections",
                i
            );
        }
    }

    #[test]
    fn test_network_sparsity() {
        let mut cfg = NetworkConfig::default();
        cfg.num_sensory_neurons = 100;
        cfg.num_hidden_per_layer_initial = 100;
        cfg.p_in = 0.1;

        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let net = build_network(&cfg, &mut rng);

        let non_zero = net.w_in.iter().filter(|&&w| w > 0.0).count();
        let total = net.w_in.len();
        let actual_p = non_zero as f64 / total as f64;

        // With 10000 elements and p=0.1, it should be reasonably close to 0.1
        assert!(actual_p > 0.05 && actual_p < 0.15);
    }
}
