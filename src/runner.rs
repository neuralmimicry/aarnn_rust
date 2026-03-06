//! # Simulation Runner and Orchestrator
//!
//! The `Runner` is the central orchestrator for the interactive and real-time
//! simulation paths. It manages the lifecycle of a neural network, including:
//! - **State Management**: Holding membrane potentials, recovery variables, and traces.
//! - **Execution Control**: Stepping the simulation (optionally with GPGPU acceleration).
//! - **Dynamic Growth**: Handling the structural evolution of the network (adding/removing neurons).
//! - **IO Integration**: Managing buffers for sensory input and actuator output.
//! - **Persistence**: Exporting and importing network snapshots and configurations.
//!
//! ## Workflow
//! 1. **Initialization**: Create a `Runner` with a specific configuration and models.
//! 2. **Execution**: Call `step()` repeatedly, providing external input spikes if needed.
//! 3. **Adaptation**: The runner automatically handles growth and plasticity updates
//!    based on the configured rules.
//!
//! ## Implementation Detail: Heterogeneous Execution
//! The `Runner` supports both standard matrix-based dynamics (fast) and detailed
//! morphological AARNN dynamics (biologically plausible). It can also offload
//! heavy computations to the GPU via the `OpenCLManager`.
//!   conduction in this file is only active in the UI path when compiled with
//!   the appropriate features and the AARNN model is selected.
use ndarray::{Array1, Array2, s};

use crate::config::{IzhikevichParams, LIFParams, NetworkConfig, STDPParams, NeuromodSignal};
#[cfg(feature = "growth3d")]
use crate::config::AarnnBioParams;
#[cfg(feature = "growth3d")]
use crate::topology::{Topology3D, Node3D};
use crate::network::{build_network, BuiltNetwork};
use crate::sim::{Learning, NeuronModel};
#[cfg(all(feature = "morpho", feature = "growth3d"))]
use crate::morphology::Morphology;
#[cfg(feature = "opencl")]
use crate::cl_compute::{OpenCLManager, CLBuffers};
#[cfg(feature = "opencl")]
use opencl3::memory::Buffer;
#[cfg(feature = "opencl")]
use opencl3::kernel::ExecuteKernel;
#[cfg(feature = "opencl")]
use opencl3::memory::{CL_MEM_READ_ONLY, CL_MEM_READ_WRITE};
#[cfg(feature = "opencl")]
use opencl3::types::CL_TRUE;
#[cfg(feature = "opencl")]
use opencl3::error_codes::ClError;
use serde::{Deserialize, Serialize};
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use std::collections::{VecDeque, HashMap};
#[cfg(feature = "opencl")]
use std::sync::Arc;
#[cfg(feature = "parallel")]
use std::sync::OnceLock;
#[cfg(feature = "opencl")]
use std::ptr;


// -------------------- Save / Load helper types --------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct Matrix2 {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

fn mat_from_nd(a: &Array2<f64>) -> Matrix2 {
    Matrix2 { rows: a.nrows(), cols: a.ncols(), data: a.iter().copied().collect() }
}

#[allow(dead_code)]
pub fn nd_from_mat(m: &Matrix2) -> Array2<f64> {
    let mut a = Array2::<f64>::zeros((m.rows, m.cols));
    let n = m.data.len().min(m.rows * m.cols);
    for idx in 0..n {
        let r = idx / m.cols;
        let c = idx % m.cols;
        a[(r, c)] = m.data[idx];
    }
    a
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Matrix2U32 {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<u32>,
}

fn mat_from_nd_u32(a: &Array2<u32>) -> Matrix2U32 {
    Matrix2U32 { rows: a.nrows(), cols: a.ncols(), data: a.iter().copied().collect() }
}

#[allow(dead_code)]
pub fn nd_from_mat_u32(m: &Matrix2U32) -> Array2<u32> {
    let mut a = Array2::<u32>::zeros((m.rows, m.cols));
    let n = m.data.len().min(m.rows * m.cols);
    for idx in 0..n {
        let r = idx / m.cols;
        let c = idx % m.cols;
        a[(r, c)] = m.data[idx];
    }
    a
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Snapshot {
    pub net: crate::config::NetworkConfig,
    #[cfg(feature = "growth3d")]
    pub topo: Option<crate::topology::Topology3D>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub skull_membrane: Option<crate::morphology::SkullMembrane>,
    pub w_in: Matrix2,
    pub w_hh_fwd: Vec<Matrix2>,
    pub w_hh_bwd: Vec<Matrix2>,
    pub w_hh_rec: Vec<Matrix2>,
    pub w_out: Matrix2,
    // Presence tracking
    pub p_in: Option<Matrix2U32>,
    pub p_fwd: Option<Vec<Matrix2U32>>,
    pub p_bwd: Option<Vec<Matrix2U32>>,
    pub p_rec: Option<Vec<Matrix2U32>>,
    pub p_out: Option<Matrix2U32>,
    /// Global layer range if this is a partial snapshot (distributed)
    pub layer_range: Option<(usize, usize)>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            net: crate::config::NetworkConfig::default(),
            #[cfg(feature = "growth3d")]
            topo: None,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            skull_membrane: None,
            w_in: Matrix2 { rows: 0, cols: 0, data: vec![] },
            w_hh_fwd: vec![],
            w_hh_bwd: vec![],
            w_hh_rec: vec![],
            w_out: Matrix2 { rows: 0, cols: 0, data: vec![] },
            p_in: None,
            p_fwd: None,
            p_bwd: None,
            p_rec: None,
            p_out: None,
            layer_range: None,
        }
    }
}

#[cfg(all(feature = "morpho", feature = "growth3d"))]
use crate::morphology::{ReleasedEvent, ReleasedKind};

#[derive(Clone, Copy, Debug)]
struct PrecalculatedDecays {
    stp_rec_decay: f64,
    stp_facil_decay: f64,
    syn_decay_ampa: f64,
    syn_decay_nmda: f64,
    syn_decay_gaba: f64,
    thr_decay: f64,
    homeo_decay: f64,
    base_homeo_target: f64,
    izh_refractory_steps: i32,
    #[allow(dead_code)]
    neuromod_plasticity_gain: f64,
    #[allow(dead_code)]
    neuromod_excitability_gain: f64,
    #[allow(dead_code)]
    izh_params: IzhikevichParams,
}

#[cfg(feature = "growth3d")]
#[derive(Clone, Copy, Debug)]
struct GrowthAction {
    // source layer where the saturated parent neuron resides
    layer: usize,
    // index of the saturated parent neuron in `layer`
    parent: usize,
    // target layer to place the new neuron (either `layer` or `layer+1`)
    target_layer: usize,
}

/// Interactive executor holding parameters, weights, membrane state, and
/// optional morphology/routing caches. See module docs for an overview.
pub struct Runner {
    // Config
    pub lif: LIFParams,
    pub stdp: STDPParams,
    pub net: NetworkConfig,
    pub neuron_model: NeuronModel,
    pub learning: Learning,

    // Weights
    pub w_in: Array2<f64>,          // (H x S)
    pub w_hh_fwd: Vec<Array2<f64>>, // len L-1
    pub w_hh_bwd: Vec<Array2<f64>>, // len L-1
    pub w_hh_rec: Vec<Array2<f64>>, // len L
    pub w_out: Array2<f64>,         // (O x H)

    // State
    /// Current simulation step counter (incremented each call to `step`).
    pub t: usize,
    /// Cumulative simulation time in milliseconds.
    pub t_ms: f64,
    pub v_h: Vec<Array1<f64>>,           // per layer H
    pub u_h: Option<Vec<Array1<f64>>>,    // izh only
    pub v_o: Array1<f64>,
    pub u_o: Option<Array1<f64>>,         // izh only
    pub refr_h: Option<Vec<Array1<i32>>>, // lif only
    pub refr_o: Option<Array1<i32>>,      // lif only
    pub izh_refr_h: Option<Vec<Array1<i32>>>, // izh only (AARNN bio)
    pub izh_refr_o: Option<Array1<i32>>,      // izh only (AARNN bio)

    pub syn_ampa_h: Vec<Array1<f64>>,
    pub syn_nmda_h: Vec<Array1<f64>>,
    pub syn_gaba_h: Vec<Array1<f64>>,
    pub syn_ampa_o: Array1<f64>,
    pub syn_nmda_o: Array1<f64>,
    pub syn_gaba_o: Array1<f64>,
    pub thr_offset_h: Vec<Array1<f64>>,
    pub thr_offset_o: Array1<f64>,
    pub rate_ema_h: Vec<Array1<f64>>,
    pub rate_ema_o: Array1<f64>,
    pub stp_u_s: Array1<f64>,
    pub stp_x_s: Array1<f64>,
    pub stp_u_h: Vec<Array1<f64>>,
    pub stp_x_h: Vec<Array1<f64>>,

    pub x_pre_in: Array1<f64>,
    pub pred_s: Array1<f64>,
    pub x_post_h: Vec<Array1<f64>>, // per layer
    pub x_pre_h: Vec<Array1<f64>>,  // per layer
    pub x_post_o: Array1<f64>,

    pub last_spk_h: Vec<Array1<i8>>, // last step spikes per layer
    pub last_spk_o: Array1<i8>,
    // Theta rhythm phase accumulator (radians)
    pub theta_phase: f32,
    // Thalamic gating phase accumulator (radians)
    pub thalamic_gate_phase: f32,
    // Neuromodulator state (AARNN)
    pub neuromod_dopamine: f32,
    pub neuromod_ach: f32,
    pub neuromod_serotonin: f32,
    // Resonance state (AARNN)
    pub resonance_level: f32,
    // External reward channel (AARNN)
    pub external_reward: f32,
    // Sleep/dream state (AARNN)
    pub sleep_active: bool,
    // World-model phase-space state (AARNN)
    pub world_model_state: Vec<f64>,
    pub world_model_proj: Option<Array2<f64>>,
    pub world_model_input_dim: usize,
    pub world_model_prev_state: Vec<f64>,

    // Feedback mapping (O -> S), -1 disabled
    /// If true, last output spikes are looped back to sensory inputs via `feedback_map`.
    pub feedback_enabled: bool,
    pub feedback_map: Vec<i32>,

    // Morphology-driven routing caches (built only when morpho+growth3d)
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub morph: Morphology,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    syn_in_map: Vec<Vec<usize>>,          // [H0][S] -> syn index or usize::MAX
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    syn_fwd_map: Vec<Vec<Vec<usize>>>,    // [l][H(l+1)][H(l)] -> syn idx or MAX
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    syn_bwd_map: Vec<Vec<Vec<usize>>>,    // [l][H(l)][H(l+1)] -> syn idx or MAX
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    syn_rec_map: Vec<Vec<Vec<usize>>>,    // [l][H(l)][H(l)] -> syn idx or MAX
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub syn_out_map: Vec<Vec<usize>>,         // [O][H_last] -> syn idx or MAX
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub syn_ax_len: Vec<f32>,                 // per-synapse axonal path length (exact)
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub syn_den_len: Vec<f32>,                // per-synapse dendritic path length (exact)
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub recv_in: Vec<Vec<(usize, usize)>>,             // [H0] -> Vec<(i, syn_idx)>
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub recv_fwd: Vec<Vec<Vec<(usize, usize)>>>,       // [l][H(l+1)] -> Vec<(i, syn_idx)>
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub recv_bwd: Vec<Vec<Vec<(usize, usize)>>>,       // [l][H(l)]   -> Vec<(j, syn_idx)> (from next layer)
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub recv_rec: Vec<Vec<Vec<(usize, usize)>>>,       // [l][H(l)] -> Vec<(i, syn_idx)> recurrent
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub recv_out: Vec<Vec<(usize, usize)>>,            // [O] -> Vec<(j, syn_idx)>
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    // Cached per‑synapse delays (recomputed when params change)
    syn_ax_steps: Vec<usize>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    syn_den_steps: Vec<usize>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    bouton_latency_steps: usize,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    bouton_jitter_steps: usize,

    // Decays
    decay_m: f64,
    decay_pre: f64,
    decay_post: f64,
    #[cfg(feature = "growth3d")]
    pub topo: Topology3D,
    // (removed duplicate `morph` field here; it's declared earlier)
    #[cfg(feature = "growth3d")]
    // Exponential moving-average firing rates per hidden neuron (for saturation detection)
    pub rate_h: Vec<Array1<f32>>, // per layer
    #[cfg(feature = "growth3d")]
    // Time since last growth event per neuron (ms)
    pub since_growth_ms: Vec<Array1<f32>>, // per layer
    #[cfg(feature = "growth3d")]
    // Time since each neuron last had a bouton (ms). 0.0 if it has boutons.
    pub since_last_bouton_ms: Vec<Array1<f32>>, // per layer

    #[cfg(feature = "growth3d")]
    /// Biological parameters for each hidden neuron, based on its assigned type.
    pub bio_h: Vec<Vec<AarnnBioParams>>,
    #[cfg(feature = "growth3d")]
    /// Biological parameters for each sensory neuron.
    pub bio_s: Vec<AarnnBioParams>,
    #[cfg(feature = "growth3d")]
    /// Biological parameters for each output neuron.
    pub bio_o: Vec<AarnnBioParams>,
    #[cfg(feature = "growth3d")]
    // queued growth actions to apply at end of step
    growth_queue: Vec<GrowthAction>,
    #[cfg(feature = "growth3d")]
    // Global inter-step cooldown timer (ms)
    last_global_growth_ms: f32,
    #[cfg(feature = "growth3d")]
    last_sensory_formation_ms: f64,
    #[cfg(feature = "growth3d")]
    last_output_formation_ms: f64,
    #[cfg(feature = "growth3d")]
    pub target_num_sensory: usize,
    #[cfg(feature = "growth3d")]
    pub target_num_output: usize,
    // Spike history per hidden layer for AARNN delays (most-recent at front)
    pub spk_hist_h: Vec<VecDeque<Array1<i8>>>,
    // Sensory spike history for AARNN delays on S→H0
    pub spk_hist_s: VecDeque<Array1<i8>>,
    // Maximum history length in steps
    pub hist_len: usize,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub morpho_accumulated_dt: f32,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub metabolic_accumulated_dt: f32,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    // Per-step list of released synapses for simple visualization (capped in size)
    /// Per‑frame list of released synapses (capped) for UI flashes.
    pub released_events: Vec<ReleasedEvent>,
    #[cfg(any(feature = "ui", feature = "growth3d"))]
    // Last computed currents for oscilloscope probes (UI only)
    pub last_i_h0: Option<Array1<f64>>,            // len = H0
    #[cfg(any(feature = "ui", feature = "growth3d"))]
    pub last_i_f: Vec<Array1<f64>>,                // per layer l>=1: len = H(l)
    #[cfg(any(feature = "ui", feature = "growth3d"))]
    pub last_i_o: Option<Array1<f64>>,             // len = O

    // Connection presence tracking (for longterm connection calculation)
    pub conn_presence_in: Array2<u32>,
    pub conn_presence_fwd: Vec<Array2<u32>>,
    pub conn_presence_bwd: Vec<Array2<u32>>,
    pub conn_presence_rec: Vec<Array2<u32>>,
    pub conn_presence_out: Array2<u32>,

    /// Range of layers assigned to this runner in a distributed setup.
    /// If None, it handles all layers.
    pub layer_range: Option<std::ops::Range<usize>>,

    #[cfg(feature = "opencl")]
    pub cl: Option<Arc<OpenCLManager>>,
    #[cfg(feature = "opencl")]
    pub cl_buffers_h: Vec<Option<CLBuffers>>,
    #[cfg(feature = "opencl")]
    pub cl_buffer_o: Option<CLBuffers>,
    #[cfg(feature = "opencl")]
    pub cl_w_in: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_x_pre_in: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_s_t: Option<Buffer<i8>>,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_fwd: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_bwd: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_rec: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_w_out: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_w_in_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_fwd_sizes: Vec<usize>,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_bwd_sizes: Vec<usize>,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_rec_sizes: Vec<usize>,
    #[cfg(feature = "opencl")]
    pub cl_w_out_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_x_pre_in_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_s_t_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_w_in_dirty: bool,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_fwd_dirty: Vec<bool>,
    #[cfg(feature = "opencl")]
    pub cl_w_hh_bwd_dirty: Vec<bool>,
    #[cfg(feature = "opencl")]
    pub cl_w_out_dirty: bool,

    #[cfg(feature = "opencl")]
    pub cl_sparse_in: Option<crate::cl_compute::CLSparseBuffers>,
    #[cfg(feature = "opencl")]
    pub cl_sparse_fwd: Vec<Option<crate::cl_compute::CLSparseBuffers>>,
    #[cfg(feature = "opencl")]
    pub cl_sparse_bwd: Vec<Option<crate::cl_compute::CLSparseBuffers>>,
    #[cfg(feature = "opencl")]
    pub cl_sparse_rec: Vec<Option<crate::cl_compute::CLSparseBuffers>>,
    #[cfg(feature = "opencl")]
    pub cl_sparse_out: Option<crate::cl_compute::CLSparseBuffers>,
    #[cfg(feature = "opencl")]
    pub cl_spk_hist_s: Option<Buffer<i8>>,
    #[cfg(feature = "opencl")]
    pub cl_spk_hist_h: Vec<Option<Buffer<i8>>>,
    #[cfg(feature = "opencl")]
    pub cl_spk_hist_s_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_spk_hist_h_sizes: Vec<usize>,
    #[cfg(feature = "opencl")]
    pub cl_syn_ampa_h: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_syn_nmda_h: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_syn_gaba_h: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_syn_ampa_o: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_syn_nmda_o: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_syn_gaba_o: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_syn_h_sizes: Vec<usize>,
    #[cfg(feature = "opencl")]
    pub cl_syn_o_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_stp_pre_s: Option<Buffer<i8>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_u_s: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_x_s: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_rel_s: Option<Buffer<f64>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_pre_h: Vec<Option<Buffer<i8>>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_u_h: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_x_h: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_rel_h: Vec<Option<Buffer<f64>>>,
    #[cfg(feature = "opencl")]
    pub cl_stp_s_size: usize,
    #[cfg(feature = "opencl")]
    pub cl_stp_h_sizes: Vec<usize>,
    #[cfg(feature = "opencl")]
    pub cl_stp_ok: bool,
}

pub struct StepOut {
    #[allow(dead_code)]
    pub t: usize,
    #[allow(dead_code)]
    pub t_ms: f64,
    pub spk_h: Vec<Array1<i8>>, // current spikes
    pub spk_o: Array1<i8>,
}

#[derive(Clone, Copy, Debug)]
pub struct SimParallelStatus {
    pub enabled: bool,
    pub worker_budget: usize,
    pub max_workers: usize,
    pub ramp_ratio: f32,
    pub health_ratio: f32,
    pub light_neuron_threshold: usize,
    pub heavy_neuron_threshold: usize,
    pub matrix_ops_threshold: usize,
}

impl Default for SimParallelStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            worker_budget: 1,
            max_workers: 1,
            ramp_ratio: 0.0,
            health_ratio: 1.0,
            light_neuron_threshold: usize::MAX,
            heavy_neuron_threshold: usize::MAX,
            matrix_ops_threshold: usize::MAX,
        }
    }
}

#[cfg(feature = "parallel")]
#[derive(Clone, Copy, Debug)]
struct SimParallelEnv {
    ramp_steps: usize,
    min_workers: usize,
    max_workers: Option<usize>,
    cpu_warn_pct: f32,
    cpu_hot_pct: f32,
    mem_free_min_mb: u64,
    light_threshold_cold: usize,
    light_threshold_hot: usize,
    heavy_threshold_cold: usize,
    heavy_threshold_hot: usize,
    matrix_ops_threshold_cold: usize,
    matrix_ops_threshold_hot: usize,
}

#[cfg(feature = "parallel")]
fn parse_env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.trim().parse::<usize>().ok()
}

#[cfg(feature = "parallel")]
fn parse_env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse::<u64>().ok()
}

#[cfg(feature = "parallel")]
fn parse_env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok()?.trim().parse::<f32>().ok()
}

#[cfg(feature = "parallel")]
fn sim_parallel_env() -> &'static SimParallelEnv {
    static ENV: OnceLock<SimParallelEnv> = OnceLock::new();
    ENV.get_or_init(|| SimParallelEnv {
        ramp_steps: parse_env_usize("NM_SIM_PAR_RAMP_STEPS").unwrap_or(180).max(1),
        min_workers: parse_env_usize("NM_SIM_PAR_MIN_WORKERS").unwrap_or(2).max(1),
        max_workers: parse_env_usize("NM_SIM_PAR_MAX_WORKERS").map(|v| v.max(1)),
        cpu_warn_pct: parse_env_f32("NM_SIM_PAR_CPU_WARN_PCT").unwrap_or(90.0).clamp(10.0, 100.0),
        cpu_hot_pct: parse_env_f32("NM_SIM_PAR_CPU_HOT_PCT").unwrap_or(97.0).clamp(10.0, 100.0),
        mem_free_min_mb: parse_env_u64("NM_SIM_PAR_MEM_FREE_MIN_MB").unwrap_or(1024).max(1),
        light_threshold_cold: parse_env_usize("NM_SIM_PAR_LIGHT_COLD").unwrap_or(96).max(2),
        light_threshold_hot: parse_env_usize("NM_SIM_PAR_LIGHT_HOT").unwrap_or(12).max(2),
        heavy_threshold_cold: parse_env_usize("NM_SIM_PAR_HEAVY_COLD").unwrap_or(256).max(2),
        heavy_threshold_hot: parse_env_usize("NM_SIM_PAR_HEAVY_HOT").unwrap_or(32).max(2),
        matrix_ops_threshold_cold: parse_env_usize("NM_SIM_PAR_MATRIX_COLD").unwrap_or(32_768).max(1),
        matrix_ops_threshold_hot: parse_env_usize("NM_SIM_PAR_MATRIX_HOT").unwrap_or(2_048).max(1),
    })
}

#[cfg(feature = "parallel")]
fn lerp_usize(cold: usize, hot: usize, ratio: f32) -> usize {
    let r = ratio.clamp(0.0, 1.0);
    let c = cold as f32;
    let h = hot as f32;
    ((c + (h - c) * r).round() as usize).max(1)
}

impl Runner {
    #[cfg(feature = "opencl")]
    fn mark_all_weights_dirty(&mut self) {
        self.cl_w_in_dirty = true;
        self.cl_w_out_dirty = true;
        for d in &mut self.cl_w_hh_fwd_dirty { *d = true; }
        for d in &mut self.cl_w_hh_bwd_dirty { *d = true; }
    }
    pub fn is_layer_assigned(&self, l: usize) -> bool {
        match &self.layer_range {
            Some(range) => range.contains(&l),
            None => true,
        }
    }

    /// Identify which hidden layers connect to Sensory inputs and Output nodes.
    /// Default: Sensory -> H0, H_last -> Output.
    /// AARNN: Sensory -> H1, H4 -> Output (falling back to what is available if network is small).
    /// These defaults are overridden by `sensory_target_layer` and `output_source_layer` if set in config.
    pub fn get_io_layers(&self) -> (usize, usize) {
        let num = self.net.num_hidden_layers;
        if num == 0 { return (0, 0); }

        let in_l = self.net.sensory_target_layer.unwrap_or_else(|| {
            if matches!(self.neuron_model, NeuronModel::Aarnn) {
                if num > 1 { 1 } else { 0 }
            } else {
                0
            }
        });

        let out_l = self.net.output_source_layer.unwrap_or_else(|| {
            if matches!(self.neuron_model, NeuronModel::Aarnn) {
                if num > 4 { 4 } else { num.saturating_sub(1) }
            } else {
                num.saturating_sub(1)
            }
        });

        (in_l.min(num - 1), out_l.min(num - 1))
    }

    fn default_aarnn_izh_params(&self) -> IzhikevichParams {
        IzhikevichParams::from_preset(&self.net.aarnn_bio.izh_preset, self.lif.dt)
    }

    fn effective_izh_params(&self) -> Option<IzhikevichParams> {
        match self.neuron_model {
            NeuronModel::Izh(p) => Some(p),
            NeuronModel::Aarnn => Some(self.default_aarnn_izh_params()),
            _ => None,
        }
    }

    fn is_izh_like(&self) -> bool {
        matches!(self.neuron_model, NeuronModel::Izh(_) | NeuronModel::Aarnn)
    }

    pub fn sim_parallel_status(&self) -> SimParallelStatus {
        self.sim_parallel_status_for_step(cfg!(feature = "parallel"))
    }

    fn sim_parallel_status_for_step(&self, parallel_feature_enabled: bool) -> SimParallelStatus {
        if !parallel_feature_enabled {
            return SimParallelStatus::default();
        }
        #[cfg(feature = "parallel")]
        {
            let env = sim_parallel_env();
            let available = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
            let rayon_limit = rayon::current_num_threads().max(1);
            let max_possible = available.min(rayon_limit).max(1);
            let min_workers = env.min_workers.min(max_possible).max(1);
            let max_workers = env
                .max_workers
                .unwrap_or(max_possible)
                .clamp(min_workers, max_possible);

            let ramp_ratio = if env.ramp_steps <= 1 {
                1.0
            } else {
                (self.t as f32 / env.ramp_steps as f32).clamp(0.0, 1.0)
            };

            let mut health_ratio = 1.0f32;
            let (cpu_usage, free_mem_mb, _, _) = crate::monitor::update_sys_cache();
            if let Some(cpu) = cpu_usage {
                if cpu >= env.cpu_hot_pct {
                    health_ratio *= 0.25;
                } else if cpu > env.cpu_warn_pct {
                    let denom = (env.cpu_hot_pct - env.cpu_warn_pct).max(1.0);
                    let over = (cpu - env.cpu_warn_pct).max(0.0);
                    let t = (over / denom).clamp(0.0, 1.0);
                    health_ratio *= 1.0 - (0.75 * t);
                }
            }
            if let Some(free_mb) = free_mem_mb {
                if free_mb < env.mem_free_min_mb {
                    let mem_ratio = (free_mb as f32 / env.mem_free_min_mb as f32).clamp(0.25, 1.0);
                    health_ratio *= mem_ratio;
                }
            }
            let effective_ratio = (ramp_ratio * health_ratio).clamp(0.0, 1.0);
            let span = max_workers.saturating_sub(min_workers);
            let worker_budget = if span == 0 {
                max_workers
            } else {
                min_workers + (span as f32 * effective_ratio).round() as usize
            }
            .clamp(1, max_workers);

            SimParallelStatus {
                enabled: max_workers > 1,
                worker_budget,
                max_workers,
                ramp_ratio,
                health_ratio,
                light_neuron_threshold: lerp_usize(
                    env.light_threshold_cold,
                    env.light_threshold_hot,
                    effective_ratio,
                ),
                heavy_neuron_threshold: lerp_usize(
                    env.heavy_threshold_cold,
                    env.heavy_threshold_hot,
                    effective_ratio,
                ),
                matrix_ops_threshold: lerp_usize(
                    env.matrix_ops_threshold_cold,
                    env.matrix_ops_threshold_hot,
                    effective_ratio,
                ),
            }
        }
        #[cfg(not(feature = "parallel"))]
        {
            SimParallelStatus::default()
        }
    }

    fn apply_synaptic_filter(
        dt: f64,
        default_bio: &crate::config::AarnnBioParams,
        raw: &Array1<f64>,
        ampa: &mut Array1<f64>,
        nmda: &mut Array1<f64>,
        gaba: &mut Array1<f64>,
        vmem: Option<&Array1<f64>>,
        nmda_voltage_sensitivity: f64,
        bio_vec: Option<&Vec<crate::config::AarnnBioParams>>,
        default_decays: &PrecalculatedDecays,
    ) -> Array1<f64> {
        let mut out = Array1::<f64>::zeros(raw.len());
        for i in 0..raw.len() {
            let (bio, d) = if let Some(bv) = bio_vec {
                let b = &bv[i];
                (b, Self::get_decays_static(dt, b))
            } else {
                (default_bio, *default_decays)
            };
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
            ampa[i] = ampa[i] * d.syn_decay_ampa + exc * (1.0 - bio.nmda_ratio);
            nmda[i] = nmda[i] * d.syn_decay_nmda + exc * bio.nmda_ratio * nmda_gate;
            gaba[i] = gaba[i] * d.syn_decay_gaba + inh;
            out[i] = (ampa[i] + nmda[i] - gaba[i]) * bio.synaptic_gain * d.neuromod_excitability_gain;
        }
        out
    }

    #[inline]
    fn hash_to_unit(mut x: u64) -> f64 {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
        x ^= x >> 33;
        (x as f64) / (u64::MAX as f64)
    }

    #[inline]
    fn release_probability(&self, syn_idx: Option<usize>) -> f32 {
        let base = self.net.p_release_default.clamp(0.0, 1.0);
        let hetero = self.net.aarnn_release_prob_heterogeneity.clamp(0.0, 1.0);
        if hetero <= 0.0 {
            return base;
        }
        let seed = syn_idx
            .map(|idx| (idx as u64).wrapping_mul(0x9e3779b185ebca87))
            .unwrap_or_else(|| (self.t as u64).wrapping_mul(0xd2b74407b1ce6e93));
        let h = Self::hash_to_unit(seed);
        let delta = ((h * 2.0) - 1.0) as f32 * hetero;
        (base + delta).clamp(0.0, 1.0)
    }

    #[inline]
    fn apply_gap_junction_coupling(curr: &mut Array1<f64>, v: &Array1<f64>, strength: f64) {
        if strength <= 0.0 || curr.len() < 2 || v.len() != curr.len() {
            return;
        }
        let mean_v = v.iter().sum::<f64>() / (v.len() as f64);
        for j in 0..curr.len() {
            curr[j] += strength * (mean_v - v[j]);
        }
    }

    #[inline]
    fn is_inhibitory_presyn(pre_idx: usize, inhibitory_fraction: f64, salt: u64) -> bool {
        if inhibitory_fraction <= 0.0 {
            return false;
        }
        let seed = (pre_idx as u64)
            .wrapping_mul(0x9e3779b185ebca87)
            .wrapping_add(salt);
        Self::hash_to_unit(seed) < inhibitory_fraction
    }

    fn enforce_dale_matrix_cols(
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
                let inhibitory = Self::is_inhibitory_presyn(i, inhibitory_fraction, salt);
                let target = if inhibitory { -w.abs() } else { w.abs() };
                let blended = w + strictness * (target - w);
                mat[(j, i)] = blended.clamp(-max_abs_w, max_abs_w);
            }
        }
    }

    fn enforce_dale_constraints(&mut self) {
        let strictness = self.net.aarnn_dale_strictness.clamp(0.0, 1.0) as f64;
        let inhibitory_fraction = self.net.aarnn_inhibitory_fraction.clamp(0.0, 1.0) as f64;
        if strictness <= 0.0 || inhibitory_fraction <= 0.0 {
            return;
        }
        let max_abs_w = self
            .stdp
            .w_max
            .abs()
            .max(self.stdp.w_min.abs())
            .max(1.0e-6);
        Self::enforce_dale_matrix_cols(&mut self.w_in, inhibitory_fraction, strictness, max_abs_w, 0x1111);
        for (l, mat) in self.w_hh_fwd.iter_mut().enumerate() {
            Self::enforce_dale_matrix_cols(
                mat,
                inhibitory_fraction,
                strictness,
                max_abs_w,
                0x2200 + l as u64,
            );
        }
        for (l, mat) in self.w_hh_bwd.iter_mut().enumerate() {
            Self::enforce_dale_matrix_cols(
                mat,
                inhibitory_fraction,
                strictness,
                max_abs_w,
                0x3300 + l as u64,
            );
        }
        for (l, mat) in self.w_hh_rec.iter_mut().enumerate() {
            Self::enforce_dale_matrix_cols(
                mat,
                inhibitory_fraction,
                strictness,
                max_abs_w,
                0x4400 + l as u64,
            );
        }
        Self::enforce_dale_matrix_cols(&mut self.w_out, inhibitory_fraction, strictness, max_abs_w, 0x5500);
    }

    fn apply_synaptic_scaling_matrix_rows(mat: &mut Array2<f64>, strength: f64, target: f64) {
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

    fn apply_synaptic_scaling(&mut self) {
        let strength = self.net.aarnn_synaptic_scaling_strength.max(0.0) as f64;
        let target = self.net.aarnn_synaptic_scaling_target.max(0.0) as f64;
        if strength <= 0.0 || target <= 0.0 {
            return;
        }
        Self::apply_synaptic_scaling_matrix_rows(&mut self.w_in, strength, target);
        for mat in &mut self.w_hh_fwd {
            Self::apply_synaptic_scaling_matrix_rows(mat, strength, target);
        }
        for mat in &mut self.w_hh_bwd {
            Self::apply_synaptic_scaling_matrix_rows(mat, strength, target);
        }
        for mat in &mut self.w_hh_rec {
            Self::apply_synaptic_scaling_matrix_rows(mat, strength, target);
        }
        Self::apply_synaptic_scaling_matrix_rows(&mut self.w_out, strength, target);
    }

    #[cfg(feature = "growth3d")]
    fn effective_max_layers(&self) -> usize {
        let cfg_max = self.net.max_layers.max(1);
        if matches!(self.neuron_model, NeuronModel::Aarnn) {
            cfg_max.min(6)
        } else {
            cfg_max
        }
    }

    #[cfg(feature = "opencl")]
    pub(crate) fn clear_cl_buffers(&mut self) {
        let l_count = self.net.num_hidden_layers;
        let l_sub_1 = l_count.saturating_sub(1);

        for b in &mut self.cl_buffers_h { *b = None; }
        if self.cl_buffers_h.len() != l_count {
            self.cl_buffers_h.resize_with(l_count, || None);
        }
        self.cl_buffer_o = None;

        self.cl_w_in = None;
        self.cl_w_in_size = 0;
        self.cl_w_in_dirty = true;

        for b in &mut self.cl_w_hh_fwd { *b = None; }
        if self.cl_w_hh_fwd.len() != l_sub_1 {
            self.cl_w_hh_fwd.resize_with(l_sub_1, || None);
        }
        self.cl_w_hh_fwd_sizes.clear();
        self.cl_w_hh_fwd_sizes.resize(l_sub_1, 0);
        self.cl_w_hh_fwd_dirty.clear();
        self.cl_w_hh_fwd_dirty.resize(l_sub_1, true);

        for b in &mut self.cl_w_hh_bwd { *b = None; }
        if self.cl_w_hh_bwd.len() != l_sub_1 {
            self.cl_w_hh_bwd.resize_with(l_sub_1, || None);
        }
        self.cl_w_hh_bwd_sizes.clear();
        self.cl_w_hh_bwd_sizes.resize(l_sub_1, 0);
        self.cl_w_hh_bwd_dirty.clear();
        self.cl_w_hh_bwd_dirty.resize(l_sub_1, true);

        for b in &mut self.cl_w_hh_rec { *b = None; }
        if self.cl_w_hh_rec.len() != l_count {
            self.cl_w_hh_rec.resize_with(l_count, || None);
        }
        self.cl_w_hh_rec_sizes.clear();
        self.cl_w_hh_rec_sizes.resize(l_count, 0);

        self.cl_w_out = None;
        self.cl_w_out_size = 0;
        self.cl_w_out_dirty = true;

        self.cl_sparse_in = None;
        self.cl_sparse_fwd.clear();
        self.cl_sparse_fwd.resize_with(l_sub_1, || None);
        self.cl_sparse_bwd.clear();
        self.cl_sparse_bwd.resize_with(l_sub_1, || None);
        self.cl_sparse_rec.clear();
        self.cl_sparse_rec.resize_with(l_count, || None);
        self.cl_sparse_out = None;

        self.cl_spk_hist_s = None;
        self.cl_spk_hist_s_size = 0;
        for b in &mut self.cl_spk_hist_h { *b = None; }
        if self.cl_spk_hist_h.len() != l_count {
            self.cl_spk_hist_h.resize_with(l_count, || None);
        }
        self.cl_spk_hist_h_sizes.clear();
        self.cl_spk_hist_h_sizes.resize(l_count, 0);

        self.cl_syn_ampa_h.clear();
        self.cl_syn_ampa_h.resize_with(l_count, || None);
        self.cl_syn_nmda_h.clear();
        self.cl_syn_nmda_h.resize_with(l_count, || None);
        self.cl_syn_gaba_h.clear();
        self.cl_syn_gaba_h.resize_with(l_count, || None);
        self.cl_syn_ampa_o = None;
        self.cl_syn_nmda_o = None;
        self.cl_syn_gaba_o = None;
        self.cl_syn_h_sizes.clear();
        self.cl_syn_h_sizes.resize(l_count, 0);
        self.cl_syn_o_size = 0;

        self.clear_cl_stp_buffers();
    }

    #[cfg(feature = "opencl")]
    fn clear_cl_stp_buffers(&mut self) {
        let l_count = self.net.num_hidden_layers;
        self.cl_stp_pre_s = None;
        self.cl_stp_u_s = None;
        self.cl_stp_x_s = None;
        self.cl_stp_rel_s = None;
        self.cl_stp_pre_h.clear();
        self.cl_stp_u_h.clear();
        self.cl_stp_x_h.clear();
        self.cl_stp_rel_h.clear();
        self.cl_stp_pre_h.resize_with(l_count, || None);
        self.cl_stp_u_h.resize_with(l_count, || None);
        self.cl_stp_x_h.resize_with(l_count, || None);
        self.cl_stp_rel_h.resize_with(l_count, || None);
        self.cl_stp_s_size = 0;
        self.cl_stp_h_sizes.clear();
        self.cl_stp_h_sizes.resize(l_count, 0);
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_buffers(&mut self, l: usize, is_output: bool) {
        if let Some(ref cl) = self.cl {
            if !is_output && l >= self.v_h.len() { return; }
            let size = if is_output { self.net.num_output_neurons } else { self.v_h[l].len() };
            let has_u = self.is_izh_like();
            let has_refr = matches!(self.neuron_model, NeuronModel::Lif);
            
            let buf_opt = if is_output { &mut self.cl_buffer_o } else { 
                if l >= self.cl_buffers_h.len() { return; }
                &mut self.cl_buffers_h[l] 
            };
            
            let need_recreate = buf_opt.as_ref().map(|b| {
                b.size != size || b.u.is_some() != has_u || b.refr.is_some() != has_refr
            }).unwrap_or(true);
            
            if need_recreate {
                if let Ok(new_buf) = CLBuffers::create(&cl.context, size, has_u, has_refr) {
                    *buf_opt = Some(new_buf);
                    self.sync_cl_state_to_gpu(l, is_output);
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_syn_buffers(&mut self, l: usize, is_output: bool) {
        if let Some(ref cl) = self.cl {
            let size = if is_output { self.net.num_output_neurons } else { self.v_h.get(l).map(|v| v.len()).unwrap_or(0) };
            if size == 0 { return; }
            let f64_size = size * std::mem::size_of::<f64>();
            if is_output {
                let need_recreate = self.cl_syn_o_size != size
                    || self.cl_syn_ampa_o.is_none()
                    || self.cl_syn_nmda_o.is_none()
                    || self.cl_syn_gaba_o.is_none();
                if need_recreate {
                    if let (Ok(a), Ok(n), Ok(g)) = (
                        unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                        unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                        unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) }
                    ) {
                        self.cl_syn_ampa_o = Some(a);
                        self.cl_syn_nmda_o = Some(n);
                        self.cl_syn_gaba_o = Some(g);
                        self.cl_syn_o_size = size;
                    } else {
                        nm_log!("[warn] OpenCL output sync buffers creation failed");
                    }
                }
                if let (Some(ref mut a), Some(ref mut n), Some(ref mut g)) = (&mut self.cl_syn_ampa_o, &mut self.cl_syn_nmda_o, &mut self.cl_syn_gaba_o) {
                    unsafe {
                        if let (Some(sa), Some(sn), Some(sg)) = (self.syn_ampa_o.as_slice(), self.syn_nmda_o.as_slice(), self.syn_gaba_o.as_slice()) {
                            if let Err(e) = cl.queue.enqueue_write_buffer(a, CL_TRUE, 0, sa, &[]) { nm_log!("[warn] OpenCL sync_cl_syn_buffers ampa_o write failed: {:?}", e); }
                            if let Err(e) = cl.queue.enqueue_write_buffer(n, CL_TRUE, 0, sn, &[]) { nm_log!("[warn] OpenCL sync_cl_syn_buffers nmda_o write failed: {:?}", e); }
                            if let Err(e) = cl.queue.enqueue_write_buffer(g, CL_TRUE, 0, sg, &[]) { nm_log!("[warn] OpenCL sync_cl_syn_buffers gaba_o write failed: {:?}", e); }
                        }
                    }
                }
            } else {
                if l >= self.cl_syn_ampa_h.len() { return; }
                let need_recreate = self.cl_syn_h_sizes.get(l).copied().unwrap_or(0) != size
                    || self.cl_syn_ampa_h.get(l).and_then(|b| b.as_ref()).is_none()
                    || self.cl_syn_nmda_h.get(l).and_then(|b| b.as_ref()).is_none()
                    || self.cl_syn_gaba_h.get(l).and_then(|b| b.as_ref()).is_none();
                if need_recreate {
                if let (Ok(a), Ok(n), Ok(g)) = (
                    unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                    unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                    unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) }
                ) {
                    self.cl_syn_ampa_h[l] = Some(a);
                    self.cl_syn_nmda_h[l] = Some(n);
                    self.cl_syn_gaba_h[l] = Some(g);
                    if l < self.cl_syn_h_sizes.len() { self.cl_syn_h_sizes[l] = size; }
                } else {
                    nm_log!("[warn] OpenCL hidden[{}] sync buffers creation failed", l);
                }
                }
                if let (Some(ref mut a), Some(ref mut n), Some(ref mut g)) = (&mut self.cl_syn_ampa_h[l], &mut self.cl_syn_nmda_h[l], &mut self.cl_syn_gaba_h[l]) {
                    unsafe {
                        if let (Some(sa), Some(sn), Some(sg)) = (self.syn_ampa_h[l].as_slice(), self.syn_nmda_h[l].as_slice(), self.syn_gaba_h[l].as_slice()) {
                            if let Err(e) = cl.queue.enqueue_write_buffer(a, CL_TRUE, 0, sa, &[]) { nm_log!("[warn] OpenCL sync_cl_syn_buffers ampa_h[{}] write failed: {:?}", l, e); }
                            if let Err(e) = cl.queue.enqueue_write_buffer(n, CL_TRUE, 0, sn, &[]) { nm_log!("[warn] OpenCL sync_cl_syn_buffers nmda_h[{}] write failed: {:?}", l, e); }
                            if let Err(e) = cl.queue.enqueue_write_buffer(g, CL_TRUE, 0, sg, &[]) { nm_log!("[warn] OpenCL sync_cl_syn_buffers gaba_h[{}] write failed: {:?}", l, e); }
                        }
                    }
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_syn_state_from_gpu(&mut self, l: usize, is_output: bool) {
        if let Some(ref cl) = self.cl {
            let size = if is_output { self.net.num_output_neurons } else { self.v_h.get(l).map(|v| v.len()).unwrap_or(0) };
            if size == 0 { return; }
            if is_output {
                if let (Some(ref mut a), Some(ref mut n), Some(ref mut g)) = (&mut self.cl_syn_ampa_o, &mut self.cl_syn_nmda_o, &mut self.cl_syn_gaba_o) {
                    let mut a_vec = vec![0.0; size];
                    let mut n_vec = vec![0.0; size];
                    let mut g_vec = vec![0.0; size];
                    unsafe {
                        if let Err(e) = cl.queue.enqueue_read_buffer(a, CL_TRUE, 0, &mut a_vec, &[]) { nm_log!("[warn] OpenCL sync_syn_state ampa_o read failed: {:?}", e); }
                        if let Err(e) = cl.queue.enqueue_read_buffer(n, CL_TRUE, 0, &mut n_vec, &[]) { nm_log!("[warn] OpenCL sync_syn_state nmda_o read failed: {:?}", e); }
                        if let Err(e) = cl.queue.enqueue_read_buffer(g, CL_TRUE, 0, &mut g_vec, &[]) { nm_log!("[warn] OpenCL sync_syn_state gaba_o read failed: {:?}", e); }
                    }
                    self.syn_ampa_o = Array1::from_vec(a_vec);
                    self.syn_nmda_o = Array1::from_vec(n_vec);
                    self.syn_gaba_o = Array1::from_vec(g_vec);
                }
            } else {
                if l >= self.cl_syn_ampa_h.len() { return; }
                if let (Some(ref mut a), Some(ref mut n), Some(ref mut g)) = (&mut self.cl_syn_ampa_h[l], &mut self.cl_syn_nmda_h[l], &mut self.cl_syn_gaba_h[l]) {
                    let mut a_vec = vec![0.0; size];
                    let mut n_vec = vec![0.0; size];
                    let mut g_vec = vec![0.0; size];
                    unsafe {
                        if let Err(e) = cl.queue.enqueue_read_buffer(a, CL_TRUE, 0, &mut a_vec, &[]) { nm_log!("[warn] OpenCL sync_syn_state ampa_h[{}] read failed: {:?}", l, e); }
                        if let Err(e) = cl.queue.enqueue_read_buffer(n, CL_TRUE, 0, &mut n_vec, &[]) { nm_log!("[warn] OpenCL sync_syn_state nmda_h[{}] read failed: {:?}", l, e); }
                        if let Err(e) = cl.queue.enqueue_read_buffer(g, CL_TRUE, 0, &mut g_vec, &[]) { nm_log!("[warn] OpenCL sync_syn_state gaba_h[{}] read failed: {:?}", l, e); }
                    }
                    self.syn_ampa_h[l] = Array1::from_vec(a_vec);
                    self.syn_nmda_h[l] = Array1::from_vec(n_vec);
                    self.syn_gaba_h[l] = Array1::from_vec(g_vec);
                }
            }
        }
    }

    #[cfg(not(feature = "opencl"))]
    fn sync_syn_state_from_gpu(&mut self, _l: usize, _is_output: bool) {}

    #[cfg(feature = "opencl")]
    fn sync_cl_stp_sensory(&mut self) -> bool {
        let size = self.net.num_sensory_neurons;
        if size == 0 { return false; }
        let cl = match self.cl.as_ref() {
            Some(cl) => cl,
            None => return false,
        };
        let need_recreate = self.cl_stp_s_size != size
            || self.cl_stp_pre_s.is_none()
            || self.cl_stp_u_s.is_none()
            || self.cl_stp_x_s.is_none()
            || self.cl_stp_rel_s.is_none();
        if need_recreate {
            let i8_size = size * std::mem::size_of::<i8>();
            let f64_size = size * std::mem::size_of::<f64>();
            if let (Ok(pre), Ok(u), Ok(x), Ok(rel)) = (
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, i8_size, ptr::null_mut()) },
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) }
            ) {
                self.cl_stp_pre_s = Some(pre);
                self.cl_stp_u_s = Some(u);
                self.cl_stp_x_s = Some(x);
                self.cl_stp_rel_s = Some(rel);
                self.cl_stp_s_size = size;
                if let (Some(ref mut u), Some(ref mut x)) = (&mut self.cl_stp_u_s, &mut self.cl_stp_x_s) {
                    unsafe {
                        if let (Some(su), Some(sx)) = (self.stp_u_s.as_slice(), self.stp_x_s.as_slice()) {
                            let _ = cl.queue.enqueue_write_buffer(u, CL_TRUE, 0, su, &[]);
                            let _ = cl.queue.enqueue_write_buffer(x, CL_TRUE, 0, sx, &[]);
                        }
                    }
                }
            } else {
                nm_log!("[warn] OpenCL sensory STP buffers creation failed");
                return false;
            }
        }
        true
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_stp_layer(&mut self, l: usize) -> bool {
        let size = self.layer_size(l);
        if size == 0 { return false; }
        let cl = match self.cl.as_ref() {
            Some(cl) => cl,
            None => return false,
        };
        if l >= self.cl_stp_pre_h.len() { self.cl_stp_pre_h.resize_with(l + 1, || None); }
        if l >= self.cl_stp_u_h.len() { self.cl_stp_u_h.resize_with(l + 1, || None); }
        if l >= self.cl_stp_x_h.len() { self.cl_stp_x_h.resize_with(l + 1, || None); }
        if l >= self.cl_stp_rel_h.len() { self.cl_stp_rel_h.resize_with(l + 1, || None); }
        if l >= self.cl_stp_h_sizes.len() { self.cl_stp_h_sizes.resize(l + 1, 0); }
        let need_recreate = self.cl_stp_h_sizes.get(l).copied().unwrap_or(0) != size
            || self.cl_stp_pre_h.get(l).and_then(|b| b.as_ref()).is_none()
            || self.cl_stp_u_h.get(l).and_then(|b| b.as_ref()).is_none()
            || self.cl_stp_x_h.get(l).and_then(|b| b.as_ref()).is_none()
            || self.cl_stp_rel_h.get(l).and_then(|b| b.as_ref()).is_none();
        if need_recreate {
            let i8_size = size * std::mem::size_of::<i8>();
            let f64_size = size * std::mem::size_of::<f64>();
            if let (Ok(pre), Ok(u), Ok(x), Ok(rel)) = (
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, i8_size, ptr::null_mut()) },
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) },
                unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut()) }
            ) {
                self.cl_stp_pre_h[l] = Some(pre);
                self.cl_stp_u_h[l] = Some(u);
                self.cl_stp_x_h[l] = Some(x);
                self.cl_stp_rel_h[l] = Some(rel);
                if l < self.cl_stp_h_sizes.len() { self.cl_stp_h_sizes[l] = size; }
                if let (Some(ref mut u), Some(ref mut x)) = (&mut self.cl_stp_u_h[l], &mut self.cl_stp_x_h[l]) {
                    unsafe {
                        if let (Some(su), Some(sx)) = (self.stp_u_h[l].as_slice(), self.stp_x_h[l].as_slice()) {
                            let _ = cl.queue.enqueue_write_buffer(u, CL_TRUE, 0, su, &[]);
                            let _ = cl.queue.enqueue_write_buffer(x, CL_TRUE, 0, sx, &[]);
                        }
                    }
                }
            } else {
                nm_log!("[warn] OpenCL hidden[{}] STP buffers creation failed", l);
                return false;
            }
        }
        true
    }

    #[cfg(feature = "opencl")]
    fn sync_stp_state_from_gpu(&mut self) {
        let cl = match self.cl.as_ref() {
            Some(cl) => cl,
            None => return,
        };
        let s_size = self.net.num_sensory_neurons;
        if s_size > 0 {
            if let (Some(ref mut u), Some(ref mut x)) = (&mut self.cl_stp_u_s, &mut self.cl_stp_x_s) {
                if let (Some(u_slice), Some(x_slice)) = (self.stp_u_s.as_slice_mut(), self.stp_x_s.as_slice_mut()) {
                    unsafe {
                        if let Err(e) = cl.queue.enqueue_read_buffer(u, CL_TRUE, 0, u_slice, &[]) {
                            nm_log!("[warn] OpenCL STP sync u_s failed: {:?}", e);
                        }
                        if let Err(e) = cl.queue.enqueue_read_buffer(x, CL_TRUE, 0, x_slice, &[]) {
                            nm_log!("[warn] OpenCL STP sync x_s failed: {:?}", e);
                        }
                    }
                }
            }
        }
        for l in 0..self.net.num_hidden_layers {
            if l >= self.cl_stp_u_h.len() || l >= self.cl_stp_x_h.len() { break; }
            if let (Some(ref mut u), Some(ref mut x)) = (&mut self.cl_stp_u_h[l], &mut self.cl_stp_x_h[l]) {
                if let (Some(u_slice), Some(x_slice)) = (self.stp_u_h[l].as_slice_mut(), self.stp_x_h[l].as_slice_mut()) {
                    unsafe {
                        if let Err(e) = cl.queue.enqueue_read_buffer(u, CL_TRUE, 0, u_slice, &[]) {
                            nm_log!("[warn] OpenCL STP sync u_h[{}] failed: {:?}", l, e);
                        }
                        if let Err(e) = cl.queue.enqueue_read_buffer(x, CL_TRUE, 0, x_slice, &[]) {
                            nm_log!("[warn] OpenCL STP sync x_h[{}] failed: {:?}", l, e);
                        }
                    }
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_state_to_gpu(&mut self, l: usize, is_output: bool) {
        if let Some(ref cl) = self.cl.clone() {
            let buf_opt = if is_output { self.cl_buffer_o.as_mut() } else { self.cl_buffers_h.get_mut(l).and_then(|o| o.as_mut()) };
            if let Some(buf) = buf_opt {
                let v_opt = if is_output { Some(&self.v_o) } else { self.v_h.get(l) };
                if let Some(v) = v_opt {
                    unsafe {
                        if let Some(v_data) = v.as_slice() {
                            if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.v, CL_TRUE, 0, v_data, &[]) {
                                nm_log!("[warn] OpenCL state sync v write failed: {:?}", e);
                            }
                        }
                    }
                }
                
                if is_output {
                    if let (Some(ref mut ubuf), Some(u)) = (&mut buf.u, self.u_o.as_ref()) {
                        unsafe {
                            if let Some(u_data) = u.as_slice() {
                                if let Err(e) = cl.queue.enqueue_write_buffer(ubuf, CL_TRUE, 0, u_data, &[]) {
                                    nm_log!("[warn] OpenCL state sync u_o write failed: {:?}", e);
                                }
                            }
                        }
                    }
                    if let (Some(ref mut rbuf), Some(refr)) = (&mut buf.refr, self.refr_o.as_ref()) {
                        unsafe {
                            if let Some(refr_data) = refr.as_slice() {
                                if let Err(e) = cl.queue.enqueue_write_buffer(rbuf, CL_TRUE, 0, refr_data, &[]) {
                                    nm_log!("[warn] OpenCL state sync refr_o write failed: {:?}", e);
                                }
                            }
                        }
                    }
                } else {
                    if let (Some(ref mut ubuf), Some(u)) = (&mut buf.u, self.u_h.as_ref().and_then(|uh| uh.get(l))) {
                        unsafe {
                            if let Some(u_data) = u.as_slice() {
                                if let Err(e) = cl.queue.enqueue_write_buffer(ubuf, CL_TRUE, 0, u_data, &[]) {
                                    nm_log!("[warn] OpenCL state sync u_h[{}] write failed: {:?}", l, e);
                                }
                            }
                        }
                    }
                    if let (Some(ref mut rbuf), Some(refr)) = (&mut buf.refr, self.refr_h.as_ref().and_then(|rh| rh.get(l))) {
                        unsafe {
                            if let Some(refr_data) = refr.as_slice() {
                                if let Err(e) = cl.queue.enqueue_write_buffer(rbuf, CL_TRUE, 0, refr_data, &[]) {
                                    nm_log!("[warn] OpenCL state sync refr_h[{}] write failed: {:?}", l, e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_state_from_gpu(&mut self, l: usize, is_output: bool) -> Array1<i8> {
        let cl = match self.cl.clone() {
            Some(c) => c,
            None => return Array1::zeros(if is_output { self.net.num_output_neurons } else { self.v_h.get(l).map(|v| v.len()).unwrap_or(0) }),
        };
        let size = if is_output { self.net.num_output_neurons } else { self.v_h.get(l).map(|v| v.len()).unwrap_or(0) };
        if size == 0 { return Array1::zeros(0); }
        let mut v_vec = vec![0.0; size];
        let mut spk_vec = vec![0i8; size];
        
        {
            let buf_opt = if is_output { self.cl_buffer_o.as_mut() } else { self.cl_buffers_h.get_mut(l).and_then(|o| o.as_mut()) };
            if let Some(buf) = buf_opt {
                unsafe {
                    if let Err(e) = cl.queue.enqueue_read_buffer(&buf.v, CL_TRUE, 0, &mut v_vec, &[]) {
                        nm_log!("[warn] OpenCL state sync v read failed: {:?}", e);
                    }
                    if let Err(e) = cl.queue.enqueue_read_buffer(&buf.spk, CL_TRUE, 0, &mut spk_vec, &[]) {
                        nm_log!("[warn] OpenCL state sync spk read failed: {:?}", e);
                    }
                }
            }
        }
        
        if is_output { self.v_o = Array1::from_vec(v_vec); } else { 
            if let Some(vh) = self.v_h.get_mut(l) { *vh = Array1::from_vec(v_vec); }
        }
        
        let buf_opt = if is_output { self.cl_buffer_o.as_mut() } else { self.cl_buffers_h.get_mut(l).and_then(|o| o.as_mut()) };
        if let Some(buf) = buf_opt {
            if let Some(ref mut ubuf) = buf.u {
                let u_opt = if is_output { self.u_o.as_mut() } else { self.u_h.as_mut().and_then(|uh| uh.get_mut(l)) };
                if let Some(u) = u_opt {
                    let mut u_vec = vec![0.0; size];
                    unsafe {
                        if let Err(e) = cl.queue.enqueue_read_buffer(ubuf, CL_TRUE, 0, &mut u_vec, &[]) {
                             nm_log!("[warn] OpenCL state sync u read failed: {:?}", e);
                        }
                    }
                    *u = Array1::from_vec(u_vec);
                }
            }
            
            if let Some(ref mut rbuf) = buf.refr {
                let r_opt = if is_output { self.refr_o.as_mut() } else { self.refr_h.as_mut().and_then(|rh| rh.get_mut(l)) };
                if let Some(r) = r_opt {
                    let mut r_vec = vec![0i32; size];
                    unsafe {
                        if let Err(e) = cl.queue.enqueue_read_buffer(rbuf, CL_TRUE, 0, &mut r_vec, &[]) {
                             nm_log!("[warn] OpenCL state sync refr read failed: {:?}", e);
                        }
                    }
                    *r = Array1::from_vec(r_vec);
                }
            }
        }
        
        Array1::from_vec(spk_vec)
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_w_in_to_gpu(&mut self) {
        if let (Some(ref cl), Some(ref mut buf)) = (&self.cl, &mut self.cl_w_in) {
            let size = self.w_in.len();
            let need_recreate = self.cl_w_in_size != size;
            if !need_recreate && !self.cl_w_in_dirty {
                return;
            }
            if need_recreate {
                if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, size * std::mem::size_of::<f64>(), ptr::null_mut()) } {
                    *buf = new_buf;
                    self.cl_w_in_size = size;
                    self.cl_w_in_dirty = true;
                }
            }
            unsafe {
                if let Some(slice) = self.w_in.as_slice() {
                    if let Err(e) = cl.queue.enqueue_write_buffer(buf, CL_TRUE, 0, slice, &[]) {
                        nm_log!("[warn] OpenCL sync_cl_w_in write failed: {:?}", e);
                    }
                }
            }
            self.cl_w_in_dirty = false;
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_w_in_from_gpu(&mut self) {
        if let (Some(cl), Some(buf)) = (&self.cl, &mut self.cl_w_in) {
            let mut w_vec = vec![0.0; self.w_in.len()];
            unsafe {
                if let Err(e) = cl.queue.enqueue_read_buffer(buf, CL_TRUE, 0, &mut w_vec, &[]) {
                    nm_log!("[warn] OpenCL sync_cl_w_in read failed: {:?}", e);
                    return;
                }
            }
            if let Ok(arr) = Array2::from_shape_vec(self.w_in.raw_dim(), w_vec) {
                self.w_in = arr;
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_w_hh_to_gpu(&mut self, l: usize) {
        if let Some(ref cl) = self.cl {
            if l >= self.w_hh_fwd.len() || l >= self.w_hh_bwd.len() || l >= self.cl_w_hh_fwd.len() || l >= self.cl_w_hh_bwd.len() { return; }
            let size_fwd = self.w_hh_fwd[l].len();
            if self.cl_w_hh_fwd_sizes[l] != size_fwd {
                if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, size_fwd * std::mem::size_of::<f64>(), ptr::null_mut()) } {
                    self.cl_w_hh_fwd[l] = Some(new_buf);
                    self.cl_w_hh_fwd_sizes[l] = size_fwd;
                    if l < self.cl_w_hh_fwd_dirty.len() { self.cl_w_hh_fwd_dirty[l] = true; }
                }
            }
            if let Some(ref mut buf) = self.cl_w_hh_fwd[l] {
                let dirty = self.cl_w_hh_fwd_dirty.get(l).copied().unwrap_or(true);
                if dirty {
                    unsafe {
                        if let Some(slice) = self.w_hh_fwd[l].as_slice() {
                            if let Err(e) = cl.queue.enqueue_write_buffer(buf, CL_TRUE, 0, slice, &[]) {
                                nm_log!("[warn] OpenCL sync_cl_w_hh_fwd[{}] write failed: {:?}", l, e);
                            }
                        }
                    }
                    if l < self.cl_w_hh_fwd_dirty.len() { self.cl_w_hh_fwd_dirty[l] = false; }
                }
            }

            let size_bwd = self.w_hh_bwd[l].len();
            if self.cl_w_hh_bwd_sizes[l] != size_bwd {
                if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, size_bwd * std::mem::size_of::<f64>(), ptr::null_mut()) } {
                    self.cl_w_hh_bwd[l] = Some(new_buf);
                    self.cl_w_hh_bwd_sizes[l] = size_bwd;
                    if l < self.cl_w_hh_bwd_dirty.len() { self.cl_w_hh_bwd_dirty[l] = true; }
                }
            }
            if let Some(ref mut buf) = self.cl_w_hh_bwd[l] {
                let dirty = self.cl_w_hh_bwd_dirty.get(l).copied().unwrap_or(true);
                if dirty {
                    unsafe {
                        if let Some(slice) = self.w_hh_bwd[l].as_slice() {
                            if let Err(e) = cl.queue.enqueue_write_buffer(buf, CL_TRUE, 0, slice, &[]) {
                                nm_log!("[warn] OpenCL sync_cl_w_hh_bwd[{}] write failed: {:?}", l, e);
                            }
                        }
                    }
                    if l < self.cl_w_hh_bwd_dirty.len() { self.cl_w_hh_bwd_dirty[l] = false; }
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    #[allow(dead_code)]
    fn sync_cl_w_hh_from_gpu(&mut self, l: usize) {
        if let Some(ref cl) = self.cl {
            if l >= self.w_hh_fwd.len() || l >= self.w_hh_bwd.len() || l >= self.cl_w_hh_fwd.len() || l >= self.cl_w_hh_bwd.len() { return; }
            if let Some(ref mut buf) = self.cl_w_hh_fwd[l] {
                let mut w_vec = vec![0.0; self.w_hh_fwd[l].len()];
                unsafe {
                    if let Err(e) = cl.queue.enqueue_read_buffer(buf, CL_TRUE, 0, &mut w_vec, &[]) {
                        nm_log!("[warn] OpenCL sync_cl_w_hh_fwd[{}] read failed: {:?}", l, e);
                    } else {
                        if let Ok(arr) = Array2::from_shape_vec(self.w_hh_fwd[l].raw_dim(), w_vec) {
                            self.w_hh_fwd[l] = arr;
                        }
                    }
                }
            }
            if let Some(ref mut buf) = self.cl_w_hh_bwd[l] {
                let mut w_vec = vec![0.0; self.w_hh_bwd[l].len()];
                unsafe {
                    if let Err(e) = cl.queue.enqueue_read_buffer(buf, CL_TRUE, 0, &mut w_vec, &[]) {
                        nm_log!("[warn] OpenCL sync_cl_w_hh_bwd[{}] read failed: {:?}", l, e);
                    } else {
                        if let Ok(arr) = Array2::from_shape_vec(self.w_hh_bwd[l].raw_dim(), w_vec) {
                            self.w_hh_bwd[l] = arr;
                        }
                    }
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_w_out_to_gpu(&mut self) {
        if let (Some(ref cl), Some(ref mut buf)) = (&self.cl, &mut self.cl_w_out) {
            let size = self.w_out.len();
            if self.cl_w_out_size == size && !self.cl_w_out_dirty {
                return;
            }
            if self.cl_w_out_size != size {
                if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_WRITE, size * std::mem::size_of::<f64>(), ptr::null_mut()) } {
                    *buf = new_buf;
                    self.cl_w_out_size = size;
                    self.cl_w_out_dirty = true;
                }
            }
            unsafe {
                if let Some(slice) = self.w_out.as_slice() {
                    if let Err(e) = cl.queue.enqueue_write_buffer(buf, CL_TRUE, 0, slice, &[]) {
                        nm_log!("[warn] OpenCL sync_cl_w_out write failed: {:?}", e);
                    }
                }
            }
            self.cl_w_out_dirty = false;
        }
    }

    #[cfg(feature = "opencl")]
    #[allow(dead_code)]
    fn sync_cl_spk_hist_s(&mut self) {
        if let (Some(ref cl), Some(ref mut buf)) = (&self.cl, &mut self.cl_spk_hist_s) {
            let hist_len = self.spk_hist_s.len();
            let neurons = self.net.num_sensory_neurons;
            let size = hist_len * neurons;
            if size == 0 { return; }

            let need_recreate = self.cl_spk_hist_s_size != size;
            if need_recreate {
                if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, size * std::mem::size_of::<i8>(), ptr::null_mut()) } {
                    *buf = new_buf;
                    self.cl_spk_hist_s_size = size;
                } else {
                    return;
                }
            }
            
            // Flatten deque
            let mut flat = Vec::with_capacity(size);
            for frame in self.spk_hist_s.iter() {
                if frame.len() == neurons { flat.extend_from_slice(frame.as_slice().unwrap()); }
                else { flat.extend(std::iter::repeat(0).take(neurons)); }
            }
            
            unsafe {
                if let Err(e) = cl.queue.enqueue_write_buffer(buf, CL_TRUE, 0, &flat, &[]) {
                    nm_log!("[warn] OpenCL spk_hist_s write failed: {:?}", e);
                }
            }
        } else if let Some(ref cl) = self.cl {
             let hist_len = self.spk_hist_s.len();
             let neurons = self.net.num_sensory_neurons;
             let size = hist_len * neurons;
             if size > 0 {
                 if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, size * std::mem::size_of::<i8>(), ptr::null_mut()) } {
                     self.cl_spk_hist_s = Some(new_buf);
                     self.cl_spk_hist_s_size = size;
                     self.sync_cl_spk_hist_s();
                 }
             }
        }
    }

    #[cfg(feature = "opencl")]
    #[allow(dead_code)]
    fn sync_cl_spk_hist_h(&mut self, l: usize) {
        if l >= self.cl_spk_hist_h.len() || l >= self.spk_hist_h.len() || l >= self.v_h.len() { return; }
        if let (Some(ref cl), Some(ref mut buf)) = (&self.cl, &mut self.cl_spk_hist_h[l]) {
            let hist_len = self.spk_hist_h[l].len();
            let neurons = self.v_h[l].len();
            let size = hist_len * neurons;
            if size == 0 { return; }

            let need_recreate = self.cl_spk_hist_h_sizes[l] != size;
            if need_recreate {
                if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, size * std::mem::size_of::<i8>(), ptr::null_mut()) } {
                    *buf = new_buf;
                    self.cl_spk_hist_h_sizes[l] = size;
                } else {
                    return;
                }
            }
            
            let mut flat = Vec::with_capacity(size);
            for frame in self.spk_hist_h[l].iter() {
                if frame.len() == neurons { flat.extend_from_slice(frame.as_slice().unwrap()); }
                else { flat.extend(std::iter::repeat(0).take(neurons)); }
            }
            
            unsafe {
                if let Err(e) = cl.queue.enqueue_write_buffer(buf, CL_TRUE, 0, &flat, &[]) {
                    nm_log!("[warn] OpenCL spk_hist_h[{}] write failed: {:?}", l, e);
                }
            }
        } else if let Some(ref cl) = self.cl {
             if l >= self.spk_hist_h.len() || l >= self.v_h.len() || l >= self.cl_spk_hist_h.len() { return; }
             let hist_len = self.spk_hist_h[l].len();
             let neurons = self.v_h[l].len();
             let size = hist_len * neurons;
             if size > 0 {
                 if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, size * std::mem::size_of::<i8>(), ptr::null_mut()) } {
                     self.cl_spk_hist_h[l] = Some(new_buf);
                     self.cl_spk_hist_h_sizes[l] = size;
                     self.sync_cl_spk_hist_h(l);
                 }
             }
        }
    }

    #[cfg(all(feature = "opencl", feature = "morpho", feature = "growth3d"))]
    fn sync_cl_sparse_in(&mut self) {
        if self.cl.is_none() { return; }
        let n_post = self.layer_size(0);
        let mut n_syn = 0;
        for j in 0..n_post { n_syn += self.recv_in[j].len(); }
        if n_syn == 0 { return; }

        let mut row_ptr = Vec::with_capacity(n_post + 1);
        let mut col_indices = Vec::with_capacity(n_syn);
        let mut weights = Vec::with_capacity(n_syn);
        let mut delays = Vec::with_capacity(n_syn);

        let mut current_offset = 0i32;
        row_ptr.push(0);
        for j in 0..n_post {
            for &(i, syn_idx) in &self.recv_in[j] {
                col_indices.push(i as i32);
                let (steps, atten) = self.syn_delay_and_atten(syn_idx);
                weights.push(self.w_in[(j, i)] * atten);
                delays.push(steps as i32);
                current_offset += 1;
            }
            row_ptr.push(current_offset);
        }

        let cl = match self.cl.as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let need_recreate = self.cl_sparse_in.as_ref().map(|b| b.n_syn != n_syn || b.n_post != n_post).unwrap_or(true);
        if need_recreate {
            if let Ok(new_buf) = crate::cl_compute::CLSparseBuffers::create(&cl.context, n_syn, n_post, true) {
                self.cl_sparse_in = Some(new_buf);
            }
        }

        if let Some(ref mut buf) = self.cl_sparse_in {
            unsafe {
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.row_ptr, CL_TRUE, 0, &row_ptr, &[]) { nm_log!("[warn] OpenCL sparse_in row_ptr write failed: {:?}", e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.col_indices, CL_TRUE, 0, &col_indices, &[]) { nm_log!("[warn] OpenCL sparse_in col_indices write failed: {:?}", e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.weights, CL_TRUE, 0, &weights, &[]) { nm_log!("[warn] OpenCL sparse_in weights write failed: {:?}", e); }
                if let Some(ref mut d_buf) = buf.delays {
                    if let Err(e) = cl.queue.enqueue_write_buffer(d_buf, CL_TRUE, 0, &delays, &[]) { nm_log!("[warn] OpenCL sparse_in delays write failed: {:?}", e); }
                }
            }
        }
    }

    #[cfg(all(feature = "opencl", feature = "morpho", feature = "growth3d"))]
    fn sync_cl_sparse_fwd(&mut self, l: usize) {
        if self.cl.is_none() { return; }
        if l >= self.recv_fwd.len() { return; }
        let n_post = self.layer_size(l + 1);
        let mut n_syn = 0;
        for j in 0..n_post { 
            if let Some(rf) = self.recv_fwd[l].get(j) {
                n_syn += rf.len(); 
            }
        }
        if n_syn == 0 { return; }

        let mut row_ptr = Vec::with_capacity(n_post + 1);
        let mut col_indices = Vec::with_capacity(n_syn);
        let mut weights = Vec::with_capacity(n_syn);
        let mut delays = Vec::with_capacity(n_syn);

        let mut current_offset = 0i32;
        row_ptr.push(0);
        for j in 0..n_post {
            if let Some(rf) = self.recv_fwd[l].get(j) {
                for &(i, syn_idx) in rf {
                    col_indices.push(i as i32);
                    let (steps, atten) = self.syn_delay_and_atten(syn_idx);
                    let val = self.w_hh_fwd[l].get((j, i)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_hh_fwd[{}][({}, {})], shape={:?}", l, j, i, self.w_hh_fwd[l].dim());
                        0.0
                    });
                    weights.push(val * atten);
                    delays.push(steps as i32);
                    current_offset += 1;
                }
            }
            row_ptr.push(current_offset);
        }

        let cl = match self.cl.as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let need_recreate = self.cl_sparse_fwd.get(l).and_then(|o| o.as_ref()).map(|b| b.n_syn != n_syn || b.n_post != n_post).unwrap_or(true);
        if need_recreate {
            if let Ok(new_buf) = crate::cl_compute::CLSparseBuffers::create(&cl.context, n_syn, n_post, true) {
                if l < self.cl_sparse_fwd.len() {
                    self.cl_sparse_fwd[l] = Some(new_buf);
                }
            }
        }

        if let Some(Some(ref mut buf)) = self.cl_sparse_fwd.get_mut(l) {
            unsafe {
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.row_ptr, CL_TRUE, 0, &row_ptr, &[]) { nm_log!("[warn] OpenCL sparse_fwd[{}] row_ptr write failed: {:?}", l, e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.col_indices, CL_TRUE, 0, &col_indices, &[]) { nm_log!("[warn] OpenCL sparse_fwd[{}] col_indices write failed: {:?}", l, e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.weights, CL_TRUE, 0, &weights, &[]) { nm_log!("[warn] OpenCL sparse_fwd[{}] weights write failed: {:?}", l, e); }
                if let Some(ref mut d_buf) = buf.delays {
                    if let Err(e) = cl.queue.enqueue_write_buffer(d_buf, CL_TRUE, 0, &delays, &[]) { nm_log!("[warn] OpenCL sparse_fwd[{}] delays write failed: {:?}", l, e); }
                }
            }
        }
    }

    #[cfg(all(feature = "opencl", feature = "morpho", feature = "growth3d"))]
    fn sync_cl_sparse_bwd(&mut self, l: usize) {
        if self.cl.is_none() { return; }
        if l >= self.recv_bwd.len() { return; }
        let n_post = self.layer_size(l);
        let mut n_syn = 0;
        for j in 0..n_post { 
            if let Some(rb) = self.recv_bwd[l].get(j) {
                n_syn += rb.len(); 
            }
        }
        if n_syn == 0 { return; }

        let mut row_ptr = Vec::with_capacity(n_post + 1);
        let mut col_indices = Vec::with_capacity(n_syn);
        let mut weights = Vec::with_capacity(n_syn);
        let mut delays = Vec::with_capacity(n_syn);

        let mut current_offset = 0i32;
        row_ptr.push(0);
        for j in 0..n_post {
            if let Some(rb) = self.recv_bwd[l].get(j) {
                for &(i, syn_idx) in rb {
                    col_indices.push(i as i32);
                    let (steps, atten) = self.syn_delay_and_atten(syn_idx);
                    let val = self.w_hh_bwd[l].get((j, i)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_hh_bwd[{}][({}, {})], shape={:?}", l, j, i, self.w_hh_bwd[l].dim());
                        0.0
                    });
                    weights.push(val * atten);
                    delays.push(steps as i32);
                    current_offset += 1;
                }
            }
            row_ptr.push(current_offset);
        }

        let cl = match self.cl.as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let need_recreate = self.cl_sparse_bwd.get(l).and_then(|o| o.as_ref()).map(|b| b.n_syn != n_syn || b.n_post != n_post).unwrap_or(true);
        if need_recreate {
            if let Ok(new_buf) = crate::cl_compute::CLSparseBuffers::create(&cl.context, n_syn, n_post, true) {
                if l < self.cl_sparse_bwd.len() {
                    self.cl_sparse_bwd[l] = Some(new_buf);
                }
            }
        }

        if let Some(Some(ref mut buf)) = self.cl_sparse_bwd.get_mut(l) {
            unsafe {
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.row_ptr, CL_TRUE, 0, &row_ptr, &[]) { nm_log!("[warn] OpenCL sparse_bwd[{}] row_ptr write failed: {:?}", l, e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.col_indices, CL_TRUE, 0, &col_indices, &[]) { nm_log!("[warn] OpenCL sparse_bwd[{}] col_indices write failed: {:?}", l, e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.weights, CL_TRUE, 0, &weights, &[]) { nm_log!("[warn] OpenCL sparse_bwd[{}] weights write failed: {:?}", l, e); }
                if let Some(ref mut d_buf) = buf.delays {
                    if let Err(e) = cl.queue.enqueue_write_buffer(d_buf, CL_TRUE, 0, &delays, &[]) { nm_log!("[warn] OpenCL sparse_bwd[{}] delays write failed: {:?}", l, e); }
                }
            }
        }
    }

    #[cfg(all(feature = "opencl", feature = "morpho", feature = "growth3d"))]
    fn sync_cl_sparse_out(&mut self) {
        if self.cl.is_none() { return; }
        let n_post = self.net.num_output_neurons;
        let mut n_syn = 0;
        for j in 0..n_post { n_syn += self.recv_out.get(j).map(|v| v.len()).unwrap_or(0); }
        if n_syn == 0 { return; }

        let mut row_ptr = Vec::with_capacity(n_post + 1);
        let mut col_indices = Vec::with_capacity(n_syn);
        let mut weights = Vec::with_capacity(n_syn);
        let mut delays = Vec::with_capacity(n_syn);

        let mut current_offset = 0i32;
        row_ptr.push(0);
        for j in 0..n_post {
            for &(i, syn_idx) in self.recv_out.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                col_indices.push(i as i32);
                let (steps, atten) = self.syn_delay_and_atten(syn_idx);
                weights.push(self.w_out[(j, i)] * atten);
                delays.push(steps as i32);
                current_offset += 1;
            }
            row_ptr.push(current_offset);
        }

        let cl = match self.cl.as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let need_recreate = self.cl_sparse_out.as_ref().map(|b| b.n_syn != n_syn || b.n_post != n_post).unwrap_or(true);
        if need_recreate {
            if let Ok(new_buf) = crate::cl_compute::CLSparseBuffers::create(&cl.context, n_syn, n_post, true) {
                self.cl_sparse_out = Some(new_buf);
            }
        }

        if let Some(ref mut buf) = self.cl_sparse_out {
            unsafe {
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.row_ptr, CL_TRUE, 0, &row_ptr, &[]) { nm_log!("[warn] OpenCL sparse_out row_ptr write failed: {:?}", e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.col_indices, CL_TRUE, 0, &col_indices, &[]) { nm_log!("[warn] OpenCL sparse_out col_indices write failed: {:?}", e); }
                if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.weights, CL_TRUE, 0, &weights, &[]) { nm_log!("[warn] OpenCL sparse_out weights write failed: {:?}", e); }
                if let Some(ref mut d_buf) = buf.delays {
                    if let Err(e) = cl.queue.enqueue_write_buffer(d_buf, CL_TRUE, 0, &delays, &[]) { nm_log!("[warn] OpenCL sparse_out delays write failed: {:?}", e); }
                }
            }
        }
    }

    #[cfg(feature = "opencl")]
    fn sync_cl_w_out_from_gpu(&mut self) {
        if let (Some(cl), Some(buf)) = (&self.cl, &mut self.cl_w_out) {
            let mut w_vec = vec![0.0; self.w_out.len()];
            unsafe {
                if let Err(e) = cl.queue.enqueue_read_buffer(buf, CL_TRUE, 0, &mut w_vec, &[]) {
                    nm_log!("[warn] OpenCL sync_cl_w_out read failed: {:?}", e);
                    return;
                }
            }
            if let Ok(arr) = Array2::from_shape_vec(self.w_out.raw_dim(), w_vec) {
                self.w_out = arr;
            }
        }
    }

    #[cfg(feature = "growth3d")]
    fn ensure_growth_vectors(&mut self) {
        // Ensure rate_h and since_growth_ms exist and match current layer sizes
        let l_count = self.net.num_hidden_layers;
        let sizes: Vec<usize> = (0..l_count).map(|l| self.v_h.get(l).map(|a| a.len()).unwrap_or(0)).collect();
        if self.rate_h.len() != l_count { self.rate_h.resize_with(l_count, || Array1::<f32>::zeros(0)); }
        if self.since_growth_ms.len() != l_count { self.since_growth_ms.resize_with(l_count, || Array1::<f32>::zeros(0)); }
        if self.since_last_bouton_ms.len() != l_count { self.since_last_bouton_ms.resize_with(l_count, || Array1::<f32>::zeros(0)); }
        for l in 0..l_count {
            if self.rate_h[l].len() != sizes[l] { self.rate_h[l] = Array1::<f32>::zeros(sizes[l]); }
            if self.since_growth_ms[l].len() != sizes[l] { self.since_growth_ms[l] = Array1::<f32>::zeros(sizes[l]); }
            if self.since_last_bouton_ms[l].len() != sizes[l] { self.since_last_bouton_ms[l] = Array1::<f32>::zeros(sizes[l]); }
        }
        // Histories: ensure at least one frame matching sizes
        self.spk_hist_h.resize_with(l_count, || {
            let mut dq: VecDeque<Array1<i8>> = VecDeque::new();
            dq.push_front(Array1::<i8>::zeros(0));
            dq
        });
        for l in 0..l_count {
            // Rebuild front frame if size mismatches
            let need = sizes[l];
            if let Some(front) = self.spk_hist_h[l].front() {
                if front.len() != need {
                    self.spk_hist_h[l].clear();
                    self.spk_hist_h[l].push_front(Array1::<i8>::zeros(need));
                }
            } else {
                self.spk_hist_h[l].push_front(Array1::<i8>::zeros(need));
            }
        }
        // Sensory history matches sensory size
        let s = self.net.num_sensory_neurons;
        if let Some(front) = self.spk_hist_s.front() {
            if front.len() != s {
                self.spk_hist_s.clear();
                self.spk_hist_s.push_front(Array1::<i8>::zeros(s));
            }
        } else {
            self.spk_hist_s.push_front(Array1::<i8>::zeros(s));
        }
    }
    /// Construct a Runner for interactive use.
    ///
    /// - If `growth_enabled` is true (and `growth3d` is compiled), the network
    ///   bootstraps with a minimal 1×1 hidden topology and grows over time.
    /// - Morphology is (re)built automatically when available and needed.
    pub fn new(lif: LIFParams, stdp: STDPParams, net: NetworkConfig, neuron_model: NeuronModel, learning: Learning) -> Self {
        let mut net_actual = net.clone();
        if net_actual.clumping_design != crate::config::ClumpingDesign::None && net_actual.brain_regions.is_empty() {
            let design = net_actual.clumping_design;
            crate::config::apply_clumping_design(&mut net_actual, design);
        }
        if matches!(neuron_model, NeuronModel::Aarnn) {
            // AARNN starts from a minimal bootstrap state so growth can form IO.
            net_actual.num_sensory_neurons = 0;
            net_actual.num_output_neurons = 0;
            net_actual.num_hidden_layers = 1;
            net_actual.num_hidden_per_layer_initial = 1;
        }
        // Build initial weights
        let mut built: BuiltNetwork = build_network(&net_actual, &mut rand::rng());
        if matches!(neuron_model, NeuronModel::Aarnn) && net_actual.use_morphology {
            built.w_in.fill(0.0);
            built.w_out.fill(0.0);
            for m in &mut built.w_hh_fwd { m.fill(0.0); }
            for m in &mut built.w_hh_bwd { m.fill(0.0); }
            for m in &mut built.w_hh_rec { m.fill(0.0); }
        }
        let l_count = net_actual.num_hidden_layers;
        let h_size = net_actual.num_hidden_per_layer_initial;
        let o_count = net_actual.num_output_neurons;
        let s_count = net_actual.num_sensory_neurons;
        let v_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let v_o = Array1::<f64>::zeros(o_count);
        let (u_h, u_o, refr_h, refr_o) = if matches!(neuron_model, NeuronModel::Izh(_) | NeuronModel::Aarnn) {
            (
                Some((0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect()),
                Some(Array1::<f64>::zeros(o_count)),
                None,
                None,
            )
        } else {
            (
                None,
                None,
                Some((0..l_count).map(|_| Array1::<i32>::zeros(h_size)).collect()),
                Some(Array1::<i32>::zeros(o_count)),
            )
        };
        let (izh_refr_h, izh_refr_o) = if matches!(neuron_model, NeuronModel::Izh(_) | NeuronModel::Aarnn) {
            (
                Some((0..l_count).map(|_| Array1::<i32>::zeros(h_size)).collect()),
                Some(Array1::<i32>::zeros(o_count)),
            )
        } else {
            (None, None)
        };
        let x_pre_in = Array1::<f64>::zeros(s_count);
        let pred_s = Array1::<f64>::zeros(s_count);
        let x_post_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let x_pre_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let x_post_o = Array1::<f64>::zeros(o_count);
        let last_spk_h = (0..l_count).map(|_| Array1::<i8>::zeros(h_size)).collect();
        let last_spk_o = Array1::<i8>::zeros(o_count);
        let syn_ampa_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let syn_nmda_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let syn_gaba_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let syn_ampa_o = Array1::<f64>::zeros(o_count);
        let syn_nmda_o = Array1::<f64>::zeros(o_count);
        let syn_gaba_o = Array1::<f64>::zeros(o_count);
        let thr_offset_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let thr_offset_o = Array1::<f64>::zeros(o_count);
        let rate_ema_h = (0..l_count).map(|_| Array1::<f64>::zeros(h_size)).collect();
        let rate_ema_o = Array1::<f64>::zeros(o_count);
        let stp_u_s = Array1::<f64>::from_elem(s_count, net_actual.aarnn_bio.stp_u);
        let stp_x_s = Array1::<f64>::from_elem(s_count, 1.0);
        let stp_u_h = (0..l_count).map(|_| Array1::<f64>::from_elem(h_size, net_actual.aarnn_bio.stp_u)).collect();
        let stp_x_h = (0..l_count).map(|_| Array1::<f64>::from_elem(h_size, 1.0)).collect();
        let decay_m = (-lif.dt / lif.tau_m).exp();
        let decay_pre = (-lif.dt / stdp.tau_pre).exp();
        let decay_post = (-lif.dt / stdp.tau_post).exp();
        let feedback_map = if s_count > 0 {
            (0..o_count).map(|k| (k % s_count) as i32).collect()
        } else {
            vec![-1; o_count]
        };
        #[cfg(feature = "growth3d")]
        let topo = crate::topology::Topology3D::new();

        // History length heuristic for delays (bounded)
        let hist_len: usize = {
            let vel = net.aarnn_velocity.max(0.0001);
            let dt = lif.dt.max(0.0001) as f32;
            // assume max normalized distance ~2.5 between adjacent layers
            let est = (2.5f32 / (vel * dt)).ceil() as usize;
            est.clamp(1, 128)
        };

        let spk_hist_s = VecDeque::from(vec![Array1::<i8>::zeros(s_count); hist_len]);
        let spk_hist_h = (0..l_count).map(|_| VecDeque::from(vec![Array1::<i8>::zeros(h_size); hist_len])).collect();

#[cfg_attr(not(feature = "growth3d"), allow(unused_mut))]
        let mut this = Self {
            lif,
            stdp,
            net: net_actual.clone(),
            neuron_model,
            learning,
            conn_presence_in: Array2::zeros(built.w_in.dim()),
            conn_presence_fwd: built.w_hh_fwd.iter().map(|m| Array2::zeros(m.dim())).collect(),
            conn_presence_bwd: built.w_hh_bwd.iter().map(|m| Array2::zeros(m.dim())).collect(),
            conn_presence_rec: built.w_hh_rec.iter().map(|m| Array2::zeros(m.dim())).collect(),
            conn_presence_out: Array2::zeros(built.w_out.dim()),

            w_in: built.w_in,
            w_hh_fwd: built.w_hh_fwd,
            w_hh_bwd: built.w_hh_bwd,
            w_hh_rec: built.w_hh_rec,
            w_out: built.w_out,
            t: 0,
            t_ms: 0.0,
            v_h,
            u_h,
            v_o,
            u_o,
            refr_h,
            refr_o,
            izh_refr_h,
            izh_refr_o,
            syn_ampa_h,
            syn_nmda_h,
            syn_gaba_h,
            syn_ampa_o,
            syn_nmda_o,
            syn_gaba_o,
            thr_offset_h,
            thr_offset_o,
            rate_ema_h,
            rate_ema_o,
            stp_u_s,
            stp_x_s,
            stp_u_h,
            stp_x_h,
            x_pre_in,
            pred_s,
            x_post_h,
            x_pre_h,
            x_post_o,
            last_spk_h,
            last_spk_o,
            theta_phase: 0.0,
            thalamic_gate_phase: 0.0,
            neuromod_dopamine: net.aarnn_neuromod_baseline_dopamine.max(0.0),
            neuromod_ach: net.aarnn_neuromod_baseline_ach.max(0.0),
            neuromod_serotonin: net.aarnn_neuromod_baseline_serotonin.max(0.0),
            resonance_level: 0.0,
            external_reward: 0.0,
            sleep_active: false,
            world_model_state: Vec::new(),
            world_model_proj: None,
            world_model_input_dim: 0,
            world_model_prev_state: Vec::new(),
            feedback_enabled: false,
            feedback_map,
            decay_m,
            decay_pre,
            decay_post,
            spk_hist_h,
            spk_hist_s,
            hist_len,
            layer_range: None,
            #[cfg(feature = "growth3d")]
            topo,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            morph: Morphology::default(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_in_map: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_fwd_map: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_bwd_map: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_rec_map: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_out_map: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_ax_len: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_den_len: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            recv_in: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            recv_fwd: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            recv_bwd: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            recv_rec: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            recv_out: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_ax_steps: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            syn_den_steps: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            bouton_latency_steps: 0,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            bouton_jitter_steps: 0,
            #[cfg(feature = "growth3d")]
            rate_h: (0..l_count).map(|_| Array1::<f32>::zeros(h_size)).collect(),
            #[cfg(feature = "growth3d")]
            since_growth_ms: (0..l_count).map(|_| Array1::<f32>::zeros(h_size)).collect(),
            #[cfg(feature = "growth3d")]
            since_last_bouton_ms: (0..l_count).map(|_| Array1::<f32>::zeros(h_size)).collect(),
            #[cfg(feature = "growth3d")]
            bio_h: (0..l_count).map(|_| (0..h_size).map(|_| net_actual.aarnn_bio.clone()).collect()).collect(),
            #[cfg(feature = "growth3d")]
            bio_s: (0..s_count).map(|_| net_actual.aarnn_bio.clone()).collect(),
            #[cfg(feature = "growth3d")]
            bio_o: (0..o_count).map(|_| net_actual.aarnn_bio.clone()).collect(),
            #[cfg(feature = "growth3d")]
            growth_queue: Vec::new(),
            #[cfg(feature = "growth3d")]
            last_global_growth_ms: 0.0,
            #[cfg(feature = "growth3d")]
            last_sensory_formation_ms: 0.0,
            #[cfg(feature = "growth3d")]
            last_output_formation_ms: 0.0,
            #[cfg(feature = "growth3d")]
            target_num_sensory: net.num_sensory_neurons,
            #[cfg(feature = "growth3d")]
            target_num_output: net.num_output_neurons,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            morpho_accumulated_dt: 0.0,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            metabolic_accumulated_dt: 0.0,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            released_events: Vec::with_capacity(256),
            #[cfg(any(feature = "ui", feature = "growth3d"))]
            last_i_h0: None,
            #[cfg(any(feature = "ui", feature = "growth3d"))]
            last_i_f: Vec::new(),
            #[cfg(any(feature = "ui", feature = "growth3d"))]
            last_i_o: None,
            #[cfg(feature = "opencl")]
            cl: crate::cl_compute::get_global_cl_manager(),
            #[cfg(feature = "opencl")]
            cl_buffers_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_buffer_o: None,
            #[cfg(feature = "opencl")]
            cl_w_in: None,
            #[cfg(feature = "opencl")]
            cl_x_pre_in: None,
            #[cfg(feature = "opencl")]
            cl_s_t: None,
            #[cfg(feature = "opencl")]
            cl_w_hh_fwd: (0..l_count.saturating_sub(1)).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_w_hh_bwd: (0..l_count.saturating_sub(1)).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_w_hh_rec: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_w_out: None,
            #[cfg(feature = "opencl")]
            cl_w_in_size: 0,
            #[cfg(feature = "opencl")]
            cl_w_hh_fwd_sizes: (0..l_count.saturating_sub(1)).map(|_| 0).collect(),
            #[cfg(feature = "opencl")]
            cl_w_hh_bwd_sizes: (0..l_count.saturating_sub(1)).map(|_| 0).collect(),
            #[cfg(feature = "opencl")]
            cl_w_hh_rec_sizes: (0..l_count).map(|_| 0).collect(),
            #[cfg(feature = "opencl")]
            cl_w_out_size: 0,
            #[cfg(feature = "opencl")]
            cl_x_pre_in_size: 0,
            #[cfg(feature = "opencl")]
            cl_s_t_size: 0,
            #[cfg(feature = "opencl")]
            cl_w_in_dirty: true,
            #[cfg(feature = "opencl")]
            cl_w_hh_fwd_dirty: (0..l_count.saturating_sub(1)).map(|_| true).collect(),
            #[cfg(feature = "opencl")]
            cl_w_hh_bwd_dirty: (0..l_count.saturating_sub(1)).map(|_| true).collect(),
            #[cfg(feature = "opencl")]
            cl_w_out_dirty: true,
            #[cfg(feature = "opencl")]
            cl_sparse_in: None,
            #[cfg(feature = "opencl")]
            cl_sparse_fwd: (0..l_count.saturating_sub(1)).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_sparse_bwd: (0..l_count.saturating_sub(1)).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_sparse_rec: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_sparse_out: None,
            #[cfg(feature = "opencl")]
            cl_spk_hist_s: None,
            #[cfg(feature = "opencl")]
            cl_spk_hist_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_spk_hist_s_size: 0,
            #[cfg(feature = "opencl")]
            cl_spk_hist_h_sizes: (0..l_count).map(|_| 0).collect(),
            #[cfg(feature = "opencl")]
            cl_syn_ampa_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_syn_nmda_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_syn_gaba_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_syn_ampa_o: None,
            #[cfg(feature = "opencl")]
            cl_syn_nmda_o: None,
            #[cfg(feature = "opencl")]
            cl_syn_gaba_o: None,
            #[cfg(feature = "opencl")]
            cl_syn_h_sizes: (0..l_count).map(|_| 0).collect(),
            #[cfg(feature = "opencl")]
            cl_syn_o_size: 0,
            #[cfg(feature = "opencl")]
            cl_stp_pre_s: None,
            #[cfg(feature = "opencl")]
            cl_stp_u_s: None,
            #[cfg(feature = "opencl")]
            cl_stp_x_s: None,
            #[cfg(feature = "opencl")]
            cl_stp_rel_s: None,
            #[cfg(feature = "opencl")]
            cl_stp_pre_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_stp_u_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_stp_x_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_stp_rel_h: (0..l_count).map(|_| None).collect(),
            #[cfg(feature = "opencl")]
            cl_stp_s_size: 0,
            #[cfg(feature = "opencl")]
            cl_stp_h_sizes: (0..l_count).map(|_| 0).collect(),
            #[cfg(feature = "opencl")]
            cl_stp_ok: true,
        };

        // AARNN-specific initial wiring policy (UI Runner):
        // When growth bootstraps a 1x1 hidden layer, start with exactly one
        // S→H0 connection to avoid overloading the initial neuron. Choose the
        // closest sensory input to the single hidden neuron. Applies only when
        // AARNN is selected (as neuron model or learning).
        #[cfg(feature = "growth3d")]
        {
            let aarnn_active = matches!(this.neuron_model, NeuronModel::Aarnn) || matches!(this.learning, Learning::Aarnn);
            // Only apply special AARNN wiring if there is exactly one hidden layer and one neuron per layer
            if aarnn_active && this.net.num_hidden_layers == 1 && this.net.num_hidden_per_layer_initial == 1 {
                // Zero all S→H0 weights, then attach exactly one closest sensory input
                let num_sensory_neurons = this.net.num_sensory_neurons;
                if num_sensory_neurons > 0 {
                    for i in 0..num_sensory_neurons {
                        if let Some(w_mut) = this.w_in.get_mut((0, i)) {
                            *w_mut = 0.0;
                        } else {
                            nm_log!("[warn] w_in zero-init out of bounds: (0, {})", i);
                        }
                    }
                    // Compute sensory positions and choose closest to H0[0]
                    let (hx, hy, hz) = if let Some(layer0) = this.topo.layers.get(0) {
                        if !layer0.is_empty() { (layer0[0].x, layer0[0].y, layer0[0].z) } else { (0.0, 0.0, 0.0) }
                    } else { (0.0, 0.0, 0.0) };
                    let mut best_i = 0usize;
                    let mut best_d = f32::MAX;
                    for (i, snode) in this.topo.sensory_nodes.iter().enumerate() {
                        let dx = snode.x - hx; let dy = snode.y - hy; let dz = snode.z - hz;
                        let d = (dx*dx + dy*dy + dz*dz).sqrt();
                        if d < best_d { best_d = d; best_i = i; }
                    }
                    // Initialize weight strongly enough to ensure the initial neuron can fire
                    // and demonstrate activity immediately.
                    let w = (fastrand::f64() * 0.4 + 0.8).clamp(this.stdp.w_min, this.stdp.w_max.max(1.2));
                    if let Some(w_mut) = this.w_in.get_mut((0, best_i)) {
                        *w_mut = w;
                    } else {
                        nm_log!("[warn] w_in best_i out of bounds: (0, {})", best_i);
                    }
                }
            }
        }

        #[cfg(feature = "growth3d")]
        {
            // initialize spike histories with one zero frame per hidden layer
            this.spk_hist_h = (0..l_count).map(|_| {
                let mut dq: VecDeque<Array1<i8>> = VecDeque::new();
                dq.push_front(Array1::<i8>::zeros(h_size));
                dq
            }).collect();
            // initialize sensory history with one zero frame
            this.spk_hist_s.push_front(Array1::<i8>::zeros(this.net.num_sensory_neurons));
        }

        // Build initial morphology snapshot (no behavior dependency)
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            this.rebuild_morphology();
        }

        #[cfg(feature = "growth3d")]
        this.rebuild_default_topology();

        this
    }


    /// Export the current `NetworkConfig` as pretty JSON (UI helper).
    #[allow(dead_code)]
    pub fn export_config_json(&self) -> anyhow::Result<String> {
        let s = serde_json::to_string_pretty(&self.net)?;
        Ok(s)
    }

    /// Import a `NetworkConfig` from JSON and reset internal caches safely.
    #[allow(dead_code)]
    pub fn import_config_json(&mut self, s: &str) -> anyhow::Result<()> {
        let cfg: crate::config::NetworkConfig = serde_json::from_str(s)?;
        // Apply config and reset runner keeping current weights/topology
        self.net = cfg;
        // Rebuild morphology and histories as parameters may affect AARNN
        self.reset();
        Ok(())
    }

    fn get_decays_static(dt: f64, bio: &crate::config::AarnnBioParams) -> PrecalculatedDecays {
        PrecalculatedDecays {
            stp_rec_decay: (-(dt / bio.stp_tau_rec_ms.max(1e-6))).exp(),
            stp_facil_decay: (-(dt / bio.stp_tau_facil_ms.max(1e-6))).exp(),
            syn_decay_ampa: (-(dt / bio.ampa_tau_ms.max(1e-6))).exp(),
            syn_decay_nmda: (-(dt / bio.nmda_tau_ms.max(1e-6))).exp(),
            syn_decay_gaba: (-(dt / bio.gaba_tau_ms.max(1e-6))).exp(),
            thr_decay: (-(dt / bio.adaptive_threshold_tau_ms.max(1e-6))).exp(),
            homeo_decay: (-(dt / bio.homeostasis_tau_ms.max(1e-6))).exp(),
            base_homeo_target: bio.homeostasis_target_rate_hz * dt / 1000.0,
            izh_refractory_steps: (bio.izh_refractory_ms / dt.max(1e-6)).round() as i32,
            neuromod_plasticity_gain: if bio.neuromodulation_enabled { (bio.dopamine_gain / bio.serotonin_gain.max(1e-6)).max(0.0) } else { 1.0 },
            neuromod_excitability_gain: if bio.neuromodulation_enabled { bio.acetylcholine_gain.max(0.0) } else { 1.0 },
            izh_params: crate::config::IzhikevichParams::from_preset(&bio.izh_preset, dt),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            net: self.net.clone(),
            #[cfg(feature = "growth3d")]
            topo: Some(self.topo.clone()),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            skull_membrane: if self.net.use_morphology {
                self.morph.skull_membrane
            } else {
                None
            },
            w_in: mat_from_nd(&self.w_in),
            w_hh_fwd: self.w_hh_fwd.iter().map(mat_from_nd).collect(),
            w_hh_bwd: self.w_hh_bwd.iter().map(mat_from_nd).collect(),
            w_hh_rec: self.w_hh_rec.iter().map(mat_from_nd).collect(),
            w_out: mat_from_nd(&self.w_out),
            p_in: Some(mat_from_nd_u32(&self.conn_presence_in)),
            p_fwd: Some(self.conn_presence_fwd.iter().map(mat_from_nd_u32).collect()),
            p_bwd: Some(self.conn_presence_bwd.iter().map(mat_from_nd_u32).collect()),
            p_rec: Some(self.conn_presence_rec.iter().map(mat_from_nd_u32).collect()),
            p_out: Some(mat_from_nd_u32(&self.conn_presence_out)),
            layer_range: self.layer_range.as_ref().map(|r| (r.start, r.end)),
        }
    }

    pub fn export_network_json(&self) -> anyhow::Result<String> {
        let snap = self.snapshot();
        let s = serde_json::to_string_pretty(&snap)?;
        Ok(s)
    }

    #[cfg(feature = "growth3d")]
    pub fn sync_bio_from_topo(&mut self) {
        let l_count = self.net.num_hidden_layers;
        self.bio_h.resize_with(l_count, Vec::new);
        for l in 0..l_count {
            let sz = self.layer_size(l);
            self.bio_h[l].resize(sz, self.net.aarnn_bio.clone());
            if let Some(nodes) = self.topo.layers.get(l) {
                for (j, node) in nodes.iter().enumerate() {
                    if j < sz {
                        if let Some(tname) = &node.type_name {
                            if let Some(ntype) = self.net.neuron_types.iter().find(|t| &t.name == tname) {
                                self.bio_h[l][j] = ntype.bio_params.clone();
                            }
                        }
                    }
                }
            }
        }
        
        let s_count = self.net.num_sensory_neurons;
        self.bio_s.resize(s_count, self.net.aarnn_bio.clone());
        for (i, node) in self.topo.sensory_nodes.iter().enumerate() {
            if i < s_count {
                if let Some(tname) = &node.type_name {
                    if let Some(ntype) = self.net.neuron_types.iter().find(|t| &t.name == tname) {
                        self.bio_s[i] = ntype.bio_params.clone();
                    }
                }
            }
        }
        
        let o_count = self.net.num_output_neurons;
        self.bio_o.resize(o_count, self.net.aarnn_bio.clone());
        for (k, node) in self.topo.output_nodes.iter().enumerate() {
            if k < o_count {
                if let Some(tname) = &node.type_name {
                    if let Some(ntype) = self.net.neuron_types.iter().find(|t| &t.name == tname) {
                        self.bio_o[k] = ntype.bio_params.clone();
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn import_network_json(&mut self, s: &str) -> anyhow::Result<()> {
        let snap: Snapshot = serde_json::from_str(s)?;
        // Update config first to ensure dimensions agree
        self.net = snap.net;
        self.layer_range = snap.layer_range.map(|(s, e)| s..e);
        // Replace weights
        self.w_in = nd_from_mat(&snap.w_in);
        self.w_hh_fwd = snap.w_hh_fwd.iter().map(nd_from_mat).collect();
        self.w_hh_bwd = snap.w_hh_bwd.iter().map(nd_from_mat).collect();
        self.w_hh_rec = snap.w_hh_rec.iter().map(nd_from_mat).collect();
        self.w_out = nd_from_mat(&snap.w_out);
        if let Some(p) = snap.p_in { self.conn_presence_in = nd_from_mat_u32(&p); }
        if let Some(p) = snap.p_fwd { self.conn_presence_fwd = p.iter().map(nd_from_mat_u32).collect(); }
        if let Some(p) = snap.p_bwd { self.conn_presence_bwd = p.iter().map(nd_from_mat_u32).collect(); }
        if let Some(p) = snap.p_rec { self.conn_presence_rec = p.iter().map(nd_from_mat_u32).collect(); }
        if let Some(p) = snap.p_out { self.conn_presence_out = nd_from_mat_u32(&p); }
        // Sync top-level sizes from matrix shapes
        self.net.num_sensory_neurons = self.w_in.ncols();
        self.net.num_output_neurons = self.w_out.nrows();
        // Resize state vectors based on new shapes
        let l_count = self.w_hh_fwd.len() + 1;
        // Keep global num_hidden_layers if we know it, otherwise use local count
        if self.net.num_hidden_layers < l_count {
            self.net.num_hidden_layers = l_count;
        }
        // Determine per-layer sizes directly from matrices to avoid stale self.v_h
        let layer_size_from_weights = |l: usize, _w_in: &Array2<f64>, w_fwd: &Vec<Array2<f64>>| -> usize {
            if w_fwd.is_empty() {
                return self.w_in.nrows();
            }
            if l < w_fwd.len() {
                w_fwd[l].ncols()
            } else {
                w_fwd[l-1].nrows()
            }
        };
        let sizes: Vec<usize> = (0..l_count).map(|l| layer_size_from_weights(l, &self.w_in, &self.w_hh_fwd)).collect();
        if l_count > 0 { self.net.num_hidden_per_layer_initial = sizes[0]; }
        // Normalize backward matrices to exist for AARNN even if absent in file
        // Expect L-1 matrices with shape (H_l, H_{l+1})
        if self.w_hh_bwd.len() != l_count.saturating_sub(1) {
            self.w_hh_bwd.resize(l_count.saturating_sub(1), Array2::<f64>::zeros((0,0)));
        }
        for l in 0..l_count.saturating_sub(1) {
            let rows = sizes[l];
            let cols = sizes[l+1];
            let shape_ok = self.w_hh_bwd[l].nrows() == rows && self.w_hh_bwd[l].ncols() == cols;
            if !shape_ok {
                // If we can, copy the overlapping region, else zero-init
                let mut m = Array2::<f64>::zeros((rows, cols));
                let old = &self.w_hh_bwd[l];
                let rmin = rows.min(old.nrows());
                let cmin = cols.min(old.ncols());
                for i in 0..rmin { for j in 0..cmin { m[(i,j)] = old[(i,j)]; } }
                self.w_hh_bwd[l] = m;
            }
        }

        // Force structural sync of all state arrays
        self.ensure_state_dimensions();
        self.sync_presence_sizes();

        self.v_h = (0..l_count).map(|l| Array1::<f64>::zeros(sizes[l])).collect();
        if self.is_izh_like() {
            self.u_h = Some((0..l_count).map(|l| Array1::<f64>::zeros(sizes[l])).collect());
            self.refr_h = None;
        } else {
            self.u_h = None;
            self.refr_h = Some((0..l_count).map(|l| Array1::<i32>::zeros(sizes[l])).collect());
        }
        self.x_post_h = (0..l_count).map(|l| Array1::<f64>::zeros(sizes[l])).collect();
        self.x_pre_h = (0..l_count).map(|l| Array1::<f64>::zeros(sizes[l])).collect();
        self.last_spk_h = (0..l_count).map(|l| Array1::<i8>::zeros(sizes[l])).collect();
        self.v_o = Array1::<f64>::zeros(self.net.num_output_neurons);
        self.last_spk_o = Array1::<i8>::zeros(self.net.num_output_neurons);
        self.x_post_o = Array1::<f64>::zeros(self.net.num_output_neurons);
        self.x_pre_in = Array1::<f64>::zeros(self.net.num_sensory_neurons);
        // Ensure output refractory or Izh arrays match new O
        if self.is_izh_like() {
            if self.u_o.is_none() { self.u_o = Some(Array1::<f64>::zeros(self.net.num_output_neurons)); }
            else if self.u_o.as_ref().unwrap().len() != self.net.num_output_neurons { self.u_o = Some(Array1::<f64>::zeros(self.net.num_output_neurons)); }
            self.refr_o = None;
        } else {
            if self.refr_o.is_none() { self.refr_o = Some(Array1::<i32>::zeros(self.net.num_output_neurons)); }
            else if self.refr_o.as_ref().unwrap().len() != self.net.num_output_neurons { self.refr_o = Some(Array1::<i32>::zeros(self.net.num_output_neurons)); }
            self.u_o = None;
        }
        #[cfg(feature = "growth3d")]
        {
            if let Some(topo) = snap.topo {
                // Use provided topology
                self.topo = topo;
            } else {
                self.rebuild_default_topology();
            }
            // rebuild histories and morphology
            self.spk_hist_h.clear();
            for l in 0..l_count {
                let mut dq: VecDeque<Array1<i8>> = VecDeque::new();
                dq.push_front(Array1::<i8>::zeros(sizes[l]));
                self.spk_hist_h.push(dq);
            }
            self.spk_hist_s.clear();
            self.spk_hist_s.push_front(Array1::<i8>::zeros(self.net.num_sensory_neurons));
            self.recalc_hist_len_and_resize();
            // Ensure growth vectors match new topology
            self.ensure_growth_vectors();
            self.sync_bio_from_topo();
        }
        #[cfg(feature = "opencl")]
        {
            self.clear_cl_buffers();
            self.cl_stp_ok = true;
            if self.net.aarnn_bio.stp_enabled && self.cl.is_some() {
                let allow = std::env::var("NM_OPENCL_STP")
                    .ok()
                    .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
                    .unwrap_or(false);
                if !allow {
                    self.cl_stp_ok = false;
                }
            }
        }
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            self.rebuild_morphology();
        }
        // Sanitize feedback map to match outputs
        if self.feedback_map.len() != self.net.num_output_neurons { self.feedback_map = vec![-1; self.net.num_output_neurons]; }
        for m in &mut self.feedback_map { if *m < 0 || (*m as usize) >= self.net.num_sensory_neurons { *m = -1; } }

        // Clear all runtime state after structural update
        self.reset();

        Ok(())
    }

    /// Safely apply a new network configuration, ensuring that structural parameters
    /// (like sensory/output counts) are correctly handled via resizes and that
    /// the internal state remains consistent with the new parameters.
    pub fn apply_config(&mut self, mut new_net: NetworkConfig) {
        observe_time!("Runner::apply_config");
        let new_design = new_net.clumping_design;
        if new_design != self.net.clumping_design && new_design != crate::config::ClumpingDesign::None {
            crate::config::apply_clumping_design(&mut new_net, new_design);
        }
        let old_s = self.net.num_sensory_neurons;
        let old_o = self.net.num_output_neurons;
        let old_layers = self.net.num_hidden_layers;
        #[cfg(feature = "opencl")]
        let old_stp = self.net.aarnn_bio.stp_enabled;

        // Perform structural resizes first if needed, BEFORE overwriting self.net
        if new_net.num_sensory_neurons != old_s {
            self.resize_sensory(new_net.num_sensory_neurons);
        }
        if new_net.num_output_neurons != old_o {
            self.resize_output(new_net.num_output_neurons);
        }

        self.net = new_net;
        
        // We generally don't allow changing num_hidden_layers at runtime via config apply
        // without a full reset, as it requires complex re-partitioning.
        self.net.num_hidden_layers = old_layers;

        #[cfg(feature = "opencl")]
        if old_stp != self.net.aarnn_bio.stp_enabled {
            self.clear_cl_stp_buffers();
            self.cl_stp_ok = true;
            if self.net.aarnn_bio.stp_enabled && self.cl.is_some() {
                let allow = std::env::var("NM_OPENCL_STP")
                    .ok()
                    .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
                    .unwrap_or(false);
                if !allow {
                    self.cl_stp_ok = false;
                    nm_log!("[info] OpenCL STP disabled; using CPU path. Set NM_OPENCL_STP=1 to enable.");
                }
            }
        }

        // Critical: ensure the "initial" hidden size reflects current reality to prevent growth crashes
        if !self.v_h.is_empty() {
            self.net.num_hidden_per_layer_initial = self.v_h[0].len();
        }
        
        // Recalculate integration constants that might depend on config
        self.decay_m = (-(self.lif.dt) / self.lif.tau_m).exp();
        self.decay_pre = (-(self.lif.dt) / self.stdp.tau_pre).exp();
        self.decay_post = (-(self.lif.dt) / self.stdp.tau_post).exp();
        
        #[cfg(feature = "growth3d")]
        {
            self.recalc_hist_len_and_resize();
            #[cfg(feature = "morpho")]
            if self.net.use_morphology {
                self.rebuild_syn_maps_from_morph();
            }
        }
        self.sync_presence_sizes();
    }

    pub fn reset(&mut self) {
        self.t = 0;
        self.t_ms = 0.0;
        self.theta_phase = 0.0;
        self.thalamic_gate_phase = 0.0;
        self.neuromod_dopamine = self.net.aarnn_neuromod_baseline_dopamine.max(0.0);
        self.neuromod_ach = self.net.aarnn_neuromod_baseline_ach.max(0.0);
        self.neuromod_serotonin = self.net.aarnn_neuromod_baseline_serotonin.max(0.0);
        self.resonance_level = 0.0;
        self.external_reward = 0.0;
        self.sleep_active = false;
        self.world_model_state.clear();
        self.world_model_proj = None;
        self.world_model_input_dim = 0;
        self.world_model_prev_state.clear();
        
        self.ensure_state_dimensions();
        let num_hidden_layers = self.net.num_hidden_layers;
        let num_output_neurons = self.net.num_output_neurons;
        let bio = self.net.aarnn_bio.clone();

        for l in 0..num_hidden_layers { 
            self.v_h[l].fill(0.0); 
            self.x_post_h[l].fill(0.0); 
            self.x_pre_h[l].fill(0.0); 
            self.last_spk_h[l].fill(0); 
            self.thr_offset_h[l].fill(0.0);
            self.rate_ema_h[l].fill(0.0);
            self.stp_u_h[l].fill(bio.stp_u);
            self.stp_x_h[l].fill(1.0);
            self.syn_ampa_h[l].fill(0.0);
            self.syn_nmda_h[l].fill(0.0);
            self.syn_gaba_h[l].fill(0.0);
        }
        self.v_o.fill(0.0);
        self.x_post_o.fill(0.0);
        self.x_pre_in.fill(0.0);
        self.pred_s.fill(0.0);
        self.last_spk_o.fill(0);
        self.thr_offset_o.fill(0.0);
        self.rate_ema_o.fill(0.0);
        self.stp_u_s.fill(bio.stp_u);
        self.stp_x_s.fill(1.0);
        self.syn_ampa_o.fill(0.0);
        self.syn_nmda_o.fill(0.0);
        self.syn_gaba_o.fill(0.0);

        #[cfg(feature = "growth3d")]
        {
            self.ensure_growth_vectors();
            self.last_sensory_formation_ms = 0.0;
            self.last_output_formation_ms = 0.0;
            for l in 0..num_hidden_layers {
                self.rate_h[l].fill(0.0);
                self.since_growth_ms[l].fill(0.0);
                self.since_last_bouton_ms[l].fill(0.0);
            }
            self.rebuild_default_topology();
            // reset spike histories to one zero frame with current sizes
            self.spk_hist_h.clear();
            for l in 0..num_hidden_layers {
                let mut dq: VecDeque<Array1<i8>> = VecDeque::new();
                dq.push_front(Array1::<i8>::zeros(self.v_h[l].len()));
                self.spk_hist_h.push(dq);
            }
            self.spk_hist_s.clear();
            self.spk_hist_s.push_front(Array1::<i8>::zeros(self.net.num_sensory_neurons));
            self.last_global_growth_ms = 0.0;
        }
        
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            // Rebuild morphology after any structural changes
            self.morpho_accumulated_dt = 0.0;
            self.metabolic_accumulated_dt = 0.0;
            self.rebuild_morphology();
            self.released_events.clear();
        }
        #[cfg(feature = "opencl")]
        {
            self.clear_cl_buffers();
        }
        #[cfg(any(feature = "ui", feature = "growth3d"))]
        {
            self.last_i_h0 = None;
            self.last_i_f.clear();
            self.last_i_o = None;
        }
        if !self.is_izh_like() {
            if self.refr_h.is_none() {
                self.refr_h = Some((0..num_hidden_layers).map(|l| Array1::<i32>::zeros(self.v_h[l].len())).collect());
            }
            if self.refr_o.is_none() { self.refr_o = Some(Array1::<i32>::zeros(num_output_neurons)); }
            self.u_h = None; self.u_o = None;
            let refh = self.refr_h.as_mut().unwrap();
            for l in 0..num_hidden_layers {
                if refh[l].len() != self.v_h[l].len() { refh[l] = Array1::<i32>::zeros(self.v_h[l].len()); }
                refh[l].fill(0);
            }
            self.refr_o.as_mut().unwrap().fill(0);
        } else {
            if self.u_h.is_none() {
                self.u_h = Some((0..num_hidden_layers).map(|l| Array1::<f64>::zeros(self.v_h[l].len())).collect());
            }
            if self.u_o.is_none() { self.u_o = Some(Array1::<f64>::zeros(num_output_neurons)); }
            self.refr_h = None; self.refr_o = None;
            let uh = self.u_h.as_mut().unwrap();
            for l in 0..num_hidden_layers {
                if uh[l].len() != self.v_h[l].len() { uh[l] = Array1::<f64>::zeros(self.v_h[l].len()); }
                uh[l].fill(0.0);
            }
            self.u_o.as_mut().unwrap().fill(0.0);
        }
        self.sync_presence_sizes();
        self.conn_presence_in.fill(0);
        for m in &mut self.conn_presence_fwd { m.fill(0); }
        for m in &mut self.conn_presence_bwd { m.fill(0); }
        for m in &mut self.conn_presence_rec { m.fill(0); }
        self.conn_presence_out.fill(0);
    }

    /// Switch learning rule and clear pre/post traces to avoid bias.
    pub fn set_learning(&mut self, l: Learning) { self.learning = l; self.clear_traces(); }
    /// Switch neuron model and perform a full reset (membranes, histories, morph).
    pub fn set_model(&mut self, m: NeuronModel) { self.neuron_model = m; self.reset(); }

    /// Update simulation time step Δt and recompute dependent integration constants and delays.
    pub fn set_dt(&mut self, dt: f64) {
        if (self.lif.dt - dt).abs() < 1e-9 { return; }
        self.lif.dt = dt;
        self.decay_m = (-dt / self.lif.tau_m).exp();
        self.decay_pre = (-dt / self.stdp.tau_pre).exp();
        self.decay_post = (-dt / self.stdp.tau_post).exp();
        if let NeuronModel::Izh(ref mut p) = self.neuron_model {
            p.dt = dt;
        }
        #[cfg(feature = "growth3d")]
        {
            self.recalc_hist_len_and_resize();
            #[cfg(feature = "morpho")]
            {
                if self.net.use_morphology { self.rebuild_syn_maps_from_morph(); }
            }
        }
    }

    #[cfg(feature = "growth3d")]
    pub fn rebuild_default_topology(&mut self) {
        use crate::topology::{Topology3D, Node3D};
        let mut topo = Topology3D::new();
        let l_count = self.net.num_hidden_layers; // Global hidden count
        
        // Determine global layer indices for local layers
        let start_layer = self.layer_range.as_ref().map(|r| r.start).unwrap_or(0);

        let (sens_x, out_x) = if self.net.growth_enabled {
            (-0.1, 0.1)
        } else {
            let start_x = -0.6;
            let end_x = -0.6 + (l_count.saturating_sub(1) as f32) * 0.3;
            (start_x - 0.1, end_x + 0.1)
        };

        // Sensory nodes
        let s_count = self.net.num_sensory_neurons;
        for i in 0..s_count {
            let (y, z) = if s_count > 1 {
                let angle = (i as f32) * 2.0 * std::f32::consts::PI / (s_count as f32);
                let radius = if self.net.growth_enabled { 0.1 } else { 0.65 };
                (radius * angle.cos(), radius * angle.sin())
            } else { (0.0, 0.0) };
            topo.sensory_nodes.push(Node3D { x: sens_x, y, z, layer: 0, ..Default::default() });
        }
        
        // Output nodes
        let o_count = self.net.num_output_neurons;
        for k in 0..o_count {
            let (y, z) = if o_count > 1 {
                let angle = (k as f32) * 2.0 * std::f32::consts::PI / (o_count as f32);
                let radius = if self.net.growth_enabled { 0.1 } else { 0.65 };
                (radius * angle.cos(), radius * angle.sin())
            } else { (0.0, 0.0) };
            topo.output_nodes.push(Node3D { x: out_x, y, z, layer: 0, ..Default::default() });
        }

        // Hidden nodes
        let local_l_count = self.v_h.len();
        for l_local in 0..local_l_count {
            let l_global = start_layer + l_local;
            let h_size = self.v_h[l_local].len();
            if h_size == 0 { continue; }
            for j in 0..h_size {
                let x = -0.6 + (l_global as f32) * 0.3;
                let y = if h_size > 1 { (j as f32) / ((h_size - 1) as f32) * 1.2 - 0.6 } else { 0.0 };
                topo.add_neuron(l_global, Node3D { x, y, z: 0.0, layer: l_global, ..Default::default() });
            }
        }
        self.topo = topo;
    }

    /// Advance simulation state by one synchronized step with dynamic Δt.
    #[allow(dead_code)]
    pub fn step_sync(&mut self, dt: f64, s_t_external: Option<&[i8]>) -> StepOut {
        self.set_dt(dt);
        self.step(s_t_external)
    }

    /// Resize the number of sensory inputs at runtime (UI control).
    ///
    /// Increasing S appends new columns to `w_in` sparsely (subset of H_target),
    /// maintains histories, and rebuilds morphology maps if active.
    pub fn resize_sensory(&mut self, n_s_new: usize) {
        let (target_layer, _) = self.get_io_layers();
        // Ensure layer exists
        if target_layer >= self.net.num_hidden_layers {
            nm_err!("[Runner::resize_sensory] Target layer {} does not exist yet (L={})", target_layer, self.net.num_hidden_layers);
            return;
        }

        // Use the actual current size of the target hidden layer
        let h = self.layer_size(target_layer);
        let s_old = self.net.num_sensory_neurons;
        if n_s_new == s_old || (n_s_new == 0 && s_old == 0) { return; }
        
        if n_s_new < s_old { 
            self.w_in = self.w_in.slice(ndarray::s![.., 0..n_s_new]).to_owned(); 
        } else {
            let add = n_s_new - s_old;
            // Prepare new columns with sparse connections ONLY to a selection of target hidden layer
            let mut new_cols = Array2::<f64>::zeros((h, add));
            // Determine a target count per new sensory input based on p_in and cap to keep it sparse
            let desired = (self.net.p_in * (h as f64)).round() as usize;
            // Cap at 40% of h to avoid dense all-to-all when p_in is high; always at least 1 target.
            // Also cap at 6 connections per sensory neuron.
            let max_cap = ((h as f64) * 0.4).ceil() as usize;
            let k_targets = desired.clamp(1, max_cap.max(1).min(h.max(1)).min(6));
            for i_add in 0..add {
                if h == 0 { break; }
                // Sample k distinct target neurons without replacement
                // Use a simple partial Fisher–Yates shuffle over indices 0..h-1
                let mut idxs: Vec<usize> = (0..h).collect();
                // Draw k positions
                for s in 0..k_targets {
                    // random index in [s, h)
                    let r = s + (fastrand::usize(..(h - s)));
                    idxs.swap(s, r);
                }
                // Initialize weights for the selected targets
                for s in 0..k_targets { 
                    let j = idxs[s];
                    if let Some(val) = new_cols.get_mut((j, i_add)) {
                        *val = fastrand::f64() * 0.3 + 0.1;
                    } else {
                        nm_log!("[warn] new_cols init out of bounds: ({}, {})", j, i_add);
                    }
                    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                        nm_log!("[trace] synapse made: sensory {} -> hidden {}:{} - initialized on sensory resize", s_old + i_add, target_layer, j);
                    }
                }
            }
            
            let mut w = Array2::<f64>::zeros((h, n_s_new));
            // Robustly copy old weights: handle potential row count mismatch if model changed roles
            let rows_to_copy = h.min(self.w_in.nrows());
            let cols_to_copy = s_old.min(self.w_in.ncols());
            for j in 0..rows_to_copy {
                for i in 0..cols_to_copy {
                    if let (Some(w_mut), Some(val)) = (w.get_mut((j, i)), self.w_in.get((j, i))) {
                        *w_mut = *val;
                    } else {
                        nm_log!("[warn] w_in copy out of bounds: ({}, {})", j, i);
                    }
                }
            }
            // append new columns
            for j in 0..h {
                for i in 0..add {
                    if let (Some(w_mut), Some(val)) = (w.get_mut((j, s_old + i)), new_cols.get((j, i))) {
                        *w_mut = *val;
                    } else {
                        nm_log!("[warn] new_cols append out of bounds: ({}, {})", j, i);
                    }
                }
            }
            self.w_in = w;
        }
        self.sync_presence_sizes();
        self.x_pre_in = Array1::<f64>::zeros(n_s_new);
        self.pred_s = Array1::<f64>::zeros(n_s_new);
        self.net.num_sensory_neurons = n_s_new;
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();
        // clean feedback map
        for m in &mut self.feedback_map { if *m < 0 || (*m as usize) >= n_s_new { *m = -1; } }
        #[cfg(feature = "growth3d")]
        {
            // Ensure sensory history frames match new sensory count
            self.extend_sensory_history(n_s_new);
            // Update topology nodes
            let s_count = n_s_new;
            let sens_x = self.topo.sensory_nodes.first().map(|n| n.x).unwrap_or(-0.7);
            self.topo.sensory_nodes.clear();
            for i in 0..s_count {
                let (y, z) = if s_count > 1 {
                    let angle = (i as f32) * 2.0 * std::f32::consts::PI / (s_count as f32);
                    let radius = if self.net.growth_enabled { 0.1 } else { 0.65 };
                    (radius * angle.cos(), radius * angle.sin())
                } else { (0.0, 0.0) };
                self.topo.sensory_nodes.push(Node3D { x: sens_x, y, z, layer: 0, ..Default::default() });
            }
        }
        // If morphology is active, rebuild snapshot and routing maps to reflect new synapses
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology { self.rebuild_morphology(); }
        }
    }

    /// Resize the number of output neurons at runtime.
    pub fn resize_output(&mut self, n_o_new: usize) {
        let (_, target_layer) = self.get_io_layers();
        // Ensure layer exists
        if target_layer >= self.net.num_hidden_layers {
            nm_err!("[Runner::resize_output] Target layer {} does not exist yet (L={})", target_layer, self.net.num_hidden_layers);
            return;
        }

        let h = self.layer_size(target_layer);
        let o_old = self.net.num_output_neurons;
        if n_o_new == o_old || (n_o_new == 0 && o_old == 0) { return; }

        if n_o_new < o_old {
            self.w_out = self.w_out.slice(ndarray::s![0..n_o_new, ..]).to_owned();
        } else {
            let add = n_o_new - o_old;
            let mut new_rows = Array2::<f64>::zeros((add, h));
            let desired = (self.net.p_out * (h as f64)).round() as usize;
            let max_cap = ((h as f64) * 0.4).ceil() as usize;
            let k_targets = desired.clamp(1, max_cap.max(1).min(h.max(1)));
            for i_add in 0..add {
                if h == 0 { break; }
                let mut idxs: Vec<usize> = (0..h).collect();
                for s in 0..k_targets {
                    let r = s + (fastrand::usize(..(h - s)));
                    idxs.swap(s, r);
                }
                for s in 0..k_targets {
                    let j = idxs[s];
                    new_rows[(i_add, j)] = fastrand::f64() * 0.3 + 0.1;
                    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                        nm_log!("[trace] synapse made: hidden {}:{} -> output {} - initialized on output resize", target_layer, j, o_old + i_add);
                    }
                }
            }
            let mut w = Array2::<f64>::zeros((n_o_new, h));
            // Robustly copy old weights: handle potential col count mismatch if model changed roles
            let rows_to_copy = o_old.min(self.w_out.nrows());
            let cols_to_copy = h.min(self.w_out.ncols());
            for k in 0..rows_to_copy {
                for j in 0..cols_to_copy {
                    if let (Some(w_mut), Some(val)) = (w.get_mut((k, j)), self.w_out.get((k, j))) {
                        *w_mut = *val;
                    } else {
                        nm_log!("[warn] w_out copy out of bounds: ({}, {})", k, j);
                    }
                }
            }
            // append new rows
            for k in 0..add {
                for j in 0..h {
                    if let (Some(w_mut), Some(val)) = (w.get_mut((o_old + k, j)), new_rows.get((k, j))) {
                        *w_mut = *val;
                    } else {
                        nm_log!("[warn] new_rows append out of bounds: ({}, {})", o_old + k, j);
                    }
                }
            }
            self.w_out = w;
        }
        self.sync_presence_sizes();
        self.v_o = Array1::<f64>::zeros(n_o_new);
        match self.neuron_model {
            NeuronModel::Izh(_) | NeuronModel::Aarnn => { self.u_o = Some(Array1::<f64>::zeros(n_o_new)); },
            NeuronModel::Lif => { self.refr_o = Some(Array1::<i32>::zeros(n_o_new)); }
        }
        self.x_post_o = Array1::<f64>::zeros(n_o_new);
        self.last_spk_o = Array1::<i8>::zeros(n_o_new);
        self.net.num_output_neurons = n_o_new;
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();
        
        // update feedback map
        let s_count = self.net.num_sensory_neurons;
        if n_o_new > o_old {
            for k in o_old..n_o_new {
                self.feedback_map.push(if s_count > 0 { (k % s_count) as i32 } else { -1 });
            }
        } else {
            self.feedback_map.truncate(n_o_new);
        }

        #[cfg(feature = "growth3d")]
        {
            let o_count = n_o_new;
            let out_x = self.topo.output_nodes.first().map(|n| n.x).unwrap_or(0.1);
            self.topo.output_nodes.clear();
            for k in 0..o_count {
                let (y, z) = if o_count > 1 {
                    let angle = (k as f32) * 2.0 * std::f32::consts::PI / (o_count as f32);
                    let radius = if self.net.growth_enabled { 0.1 } else { 0.65 };
                    (radius * angle.cos(), radius * angle.sin())
                } else { (0.0, 0.0) };
                self.topo.output_nodes.push(Node3D { x: out_x, y, z, layer: 0, ..Default::default() });
            }
        }
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology { self.rebuild_morphology(); }
        }
    }

    fn clear_traces(&mut self) {
        for l in 0..self.net.num_hidden_layers { self.x_post_h[l].fill(0.0); self.x_pre_h[l].fill(0.0); }
        self.x_pre_in.fill(0.0); self.x_post_o.fill(0.0);
    }

    /// Advance the simulation by one step.
    ///
    /// - If `s_t_external` is provided, it overrides sensory spikes for this
    ///   step. Otherwise, the current provider/UI will supply inputs.
    /// - When AARNN+morphology are active, synaptic currents are accumulated
    ///   using exact per‑synapse delays.
    pub fn step(&mut self, s_t_external: Option<&[i8]>) -> StepOut {
        let (in_l, out_l) = self.get_io_layers();

        // 1. Core structural sync: ensure dimensions match config before capturing locals
        let state_changed = self.ensure_state_dimensions();
        let weight_changed = self.ensure_weight_dimensions(in_l, out_l);

        #[cfg(feature = "growth3d")]
        let (dt_ms, decay_rate) = {
            let dt = self.lif.dt as f32;
            let tau = self.net.saturation_window_ms.max(1.0);
            (dt, (-dt / tau).exp())
        };

        #[cfg(feature = "growth3d")]
        {
            // advance global cooldown timer
            self.last_global_growth_ms += dt_ms;
            // Recalculate and enforce history length when delays are enabled or parameters/topology changed
            self.recalc_hist_len_and_resize();
            // Ensure each hidden layer's history frames match current neuron count
            for l in 0..self.net.num_hidden_layers {
                let want = self.layer_size(l);
                if let Some(dq) = self.spk_hist_h.get(l) {
                    if dq.front().map(|a| a.len()).unwrap_or(0) != want {
                        self.extend_history_frames(l, want);
                    }
                }
            }
            // Ensure sensory history frames match current sensory count
            if self.spk_hist_s.front().map(|a| a.len()).unwrap_or(0) != self.net.num_sensory_neurons {
                self.extend_sensory_history(self.net.num_sensory_neurons);
            }
            // Debug-only: all frames in each deque must match current layer size
            #[cfg(debug_assertions)]
            for l in 0..self.net.num_hidden_layers {
                let want = self.layer_size(l);
                if let Some(dq) = self.spk_hist_h.get(l) {
                    for fr in dq.iter() { debug_assert_eq!(fr.len(), want, "history frame width mismatch at layer {} ({} != {})", l, fr.len(), want); }
                }
            }
        }

        if state_changed || weight_changed {
            self.sync_presence_sizes();
            #[cfg(feature = "opencl")]
            self.mark_all_weights_dirty();
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            if self.net.use_morphology {
                self.rebuild_syn_maps_from_morph();
            }
        }

        let is_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
        let depth = self.net.aarnn_layer_depth;
        let bio = self.net.aarnn_bio.clone();
        let use_aarnn_bio = is_aarnn && depth > 0;
        let use_synaptic_filter = use_aarnn_bio && depth >= 1;
        let use_stp = use_aarnn_bio && depth >= 1 && bio.stp_enabled;
        let use_adaptive_threshold = use_aarnn_bio && depth >= 2 && bio.adaptive_threshold_enabled;
        let use_homeostasis = use_aarnn_bio && depth >= 2 && bio.homeostasis_gain > 0.0;
        let use_izh_refractory = use_aarnn_bio && depth >= 2 && bio.izh_refractory_ms > 0.0;
        let neuromod_state_d = self.neuromod_dopamine.max(0.0) as f64;
        let neuromod_state_s = self.neuromod_serotonin.max(0.0) as f64;
        let neuromod_state_a = self.neuromod_ach.max(0.0) as f64;
        let neuromod_plasticity_gain = if use_aarnn_bio && bio.neuromodulation_enabled {
            ((bio.dopamine_gain * neuromod_state_d) / (bio.serotonin_gain * neuromod_state_s).max(1e-6)).max(0.0)
        } else {
            1.0
        };

        let mut sleep_active = false;
        if is_aarnn && self.net.sleep_enabled && self.net.sleep_cycle_ms > 0.0 {
            let cycle = self.net.sleep_cycle_ms.max(1.0);
            let dur = self.net.sleep_duration_ms.clamp(0.0, cycle);
            if dur > 0.0 {
                let phase = (self.t_ms as f32) % cycle;
                sleep_active = phase < dur;
            }
        }
        self.sleep_active = sleep_active;
        
        observe_time!("Runner::step");
        observe_hit!("simulation_step");
        let num_hidden_layers = self.net.num_hidden_layers;
        let num_hidden_0_neurons = self.v_h.get(0).map(|a: &Array1<f64>| a.len()).unwrap_or(0);
        let num_sensory_neurons = self.net.num_sensory_neurons;
        let num_output_neurons = self.net.num_output_neurons;
        let parallel_enabled = cfg!(feature = "parallel");
        let sim_parallel = self.sim_parallel_status_for_step(parallel_enabled);
        #[allow(unused_variables)]
        let can_parallel_light = |items: usize| {
            parallel_enabled
                && sim_parallel.enabled
                && sim_parallel.worker_budget > 1
                && items >= sim_parallel.light_neuron_threshold
        };
        #[allow(unused_variables)]
        let can_parallel_heavy = |items: usize| {
            parallel_enabled
                && sim_parallel.enabled
                && sim_parallel.worker_budget > 1
                && items >= sim_parallel.heavy_neuron_threshold
        };
        #[allow(unused_variables)]
        let can_parallel_matrix = |rows: usize, cols: usize| {
            parallel_enabled
                && sim_parallel.enabled
                && sim_parallel.worker_budget > 1
                && rows.saturating_mul(cols) >= sim_parallel.matrix_ops_threshold
        };
        let prev_spk_h = self.last_spk_h.iter().map(|a: &Array1<i8>| a.clone()).collect::<Vec<_>>();
        
        let mut type_cache = HashMap::new();
        for ntype in &self.net.neuron_types {
            type_cache.insert(ntype.name.clone(), Self::get_decays_static(self.lif.dt, &ntype.bio_params));
        }
        let default_decays = Self::get_decays_static(self.lif.dt, &self.net.aarnn_bio);
        #[allow(unused_variables)]
        let stp_rec_decay = default_decays.stp_rec_decay;
        #[allow(unused_variables)]
        let stp_facil_decay = default_decays.stp_facil_decay;
        #[allow(unused_variables)]
        let syn_decay_ampa = default_decays.syn_decay_ampa;
        #[allow(unused_variables)]
        let syn_decay_nmda = default_decays.syn_decay_nmda;
        #[allow(unused_variables)]
        let syn_decay_gaba = default_decays.syn_decay_gaba;
        #[allow(unused_variables)]
        let thr_decay = default_decays.thr_decay;
        let homeo_decay = default_decays.homeo_decay;
        let base_homeo_target = default_decays.base_homeo_target;
        let izh_refractory_steps = default_decays.izh_refractory_steps;
        #[allow(unused_variables)]
        let neuromod_excitability_gain = if use_aarnn_bio && bio.neuromodulation_enabled {
            (default_decays.neuromod_excitability_gain * neuromod_state_a).max(0.0)
        } else {
            1.0
        };

        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            // Clear per-frame transmission events at the start of the step
            self.released_events.clear();
        }

        // Build sensory spikes (external if provided)
        let mut s_t = vec![0i8; num_sensory_neurons];
        if let Some(src) = s_t_external {
            let len = src.len().min(s_t.len());
            s_t[..len].copy_from_slice(&src[..len]);
        }
        // optional feedback from previous step
        if self.feedback_enabled {
            for k in 0..num_output_neurons { if self.last_spk_o[k] != 0 { let idx = self.feedback_map[k] as isize; if idx >= 0 && (idx as usize) < s_t.len() { s_t[idx as usize] = 1; } } }
        }

        // Sleep/dream: replace sensory inputs with replay/prediction
        if sleep_active && num_sensory_neurons > 0 {
            let mut dream = vec![0i8; num_sensory_neurons];
            let replay_p = self.net.sleep_dream_replay_prob.clamp(0.0, 1.0);
            let use_replay = !self.spk_hist_s.is_empty() && fastrand::f32() < replay_p;
            if use_replay {
                let idx = fastrand::usize(..self.spk_hist_s.len());
                let frame = &self.spk_hist_s[idx];
                let len = frame.len().min(dream.len());
                if len > 0 {
                    if let Some(slice) = frame.as_slice() {
                        dream[..len].copy_from_slice(&slice[..len]);
                    } else {
                        for i in 0..len { dream[i] = frame[i]; }
                    }
                }
            } else {
                let thresh = self.net.sleep_dream_threshold.clamp(0.0, 1.0) as f64;
                for i in 0..num_sensory_neurons {
                    if self.pred_s.get(i).copied().unwrap_or(0.0) >= thresh {
                        dream[i] = 1;
                    }
                }
            }
            s_t = dream;
        }

        // Thalamic gating: modulate sensory inputs (AARNN only)
        if is_aarnn && self.net.thalamic_gating_enabled && num_sensory_neurons > 0 {
            let hz = self.net.thalamic_gate_hz.max(0.0);
            let duty = self.net.thalamic_gate_duty.clamp(0.01, 1.0);
            let floor = self.net.thalamic_gate_floor.clamp(0.0, 1.0);
            let gate = if hz > 0.0 {
                let dt_s = (self.lif.dt.max(0.001) as f32) / 1000.0;
                let step = std::f32::consts::TAU * hz * dt_s;
                self.thalamic_gate_phase = (self.thalamic_gate_phase + step) % std::f32::consts::TAU;
                let phase_gate = self.thalamic_gate_phase.sin() * 0.5 + 0.5;
                let open = phase_gate >= 1.0 - duty;
                if open { 1.0 } else { floor }
            } else {
                floor
            };
            if gate < 1.0 {
                for i in 0..num_sensory_neurons {
                    if s_t[i] != 0 && fastrand::f32() > gate {
                        s_t[i] = 0;
                    }
                }
            }
        }

        // Perceptual loop: predict sensory spikes and update prediction state (AARNN only)
        let mut perceptual_error_drive = 0.0f64;
        let mut perceptual_mean_err = 0.0f64;
        if is_aarnn && self.net.perceptual_loop_enabled && num_sensory_neurons > 0 {
            let lr = self.net.perceptual_prediction_lr.clamp(0.0, 1.0) as f64;
            let decay = self.net.perceptual_prediction_decay.clamp(0.0, 1.0) as f64;
            let thresh = self.net.perceptual_prediction_threshold.clamp(0.0, 1.0) as f64;
            let fb_gain = self.net.perceptual_feedback_gain.clamp(0.0, 1.0) as f64;

            if decay > 0.0 {
                let retain = (1.0 - decay).max(0.0);
                for v in self.pred_s.iter_mut() {
                    *v *= retain;
                }
            }

            let mut pred_from_output = vec![0.0f64; num_sensory_neurons];
            if fb_gain > 0.0 && num_output_neurons > 0 {
                for k in 0..num_output_neurons {
                    if self.last_spk_o[k] != 0 {
                        let idx = self.feedback_map[k] as isize;
                        if idx >= 0 && (idx as usize) < num_sensory_neurons {
                            pred_from_output[idx as usize] = 1.0;
                        }
                    }
                }
            }

            let mut err_sum = 0.0f64;
            for i in 0..num_sensory_neurons {
                let pred = if fb_gain > 0.0 {
                    (1.0 - fb_gain) * self.pred_s[i] + fb_gain * pred_from_output[i]
                } else {
                    self.pred_s[i]
                };
                let pred_bin = if pred >= thresh { 1.0 } else { 0.0 };
                let actual = s_t[i] as f64;
                let err = actual - pred_bin;
                err_sum += err.abs();

                if lr > 0.0 {
                    self.pred_s[i] += lr * (actual - self.pred_s[i]);
                    if self.pred_s[i] < 0.0 { self.pred_s[i] = 0.0; }
                    if self.pred_s[i] > 1.0 { self.pred_s[i] = 1.0; }
                }
            }

            let mean_err = err_sum / (num_sensory_neurons as f64);
            perceptual_error_drive = (self.net.perceptual_error_gain.max(0.0) as f64) * mean_err;
            perceptual_mean_err = mean_err;
        }

        let mut stp_release_s: Vec<f64> = if use_stp { vec![0.0; num_sensory_neurons] } else { Vec::new() };
        let mut stp_release_h: Vec<Vec<f64>> = if use_stp {
            (0..num_hidden_layers).map(|l| vec![0.0; self.layer_size(l)]).collect()
        } else {
            Vec::new()
        };
        if use_stp {
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut stp_gpu_updated_s = false;
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut stp_gpu_updated_h = vec![false; num_hidden_layers];
            #[cfg(feature = "opencl")]
            let mut stp_gpu_failed = false;
            #[cfg(feature = "opencl")]
            if self.cl_stp_ok && self.cl.is_some() {
                if self.sync_cl_stp_sensory() {
                    if let (Some(ref mut pre), Some(ref mut u), Some(ref mut x), Some(ref mut rel)) = (
                        &mut self.cl_stp_pre_s,
                        &mut self.cl_stp_u_s,
                        &mut self.cl_stp_x_s,
                        &mut self.cl_stp_rel_s,
                    ) {
                        if let Some(ref cl) = self.cl {
                            let mut ok = true;
                            unsafe {
                                if let Err(e) = cl.queue.enqueue_write_buffer(pre, CL_TRUE, 0, &s_t, &[]) {
                                    nm_log!("[warn] OpenCL STP sensory write failed: {:?}", e);
                                    ok = false;
                                }
                                if ok {
                                    let kernel = cl.kernel_stp_update.lock().unwrap();
                                    let launch = ExecuteKernel::new(&kernel)
                                        .set_arg(&*u)
                                        .set_arg(&*x)
                                        .set_arg(&*pre)
                                        .set_arg(&*rel)
                                        .set_arg(&bio.stp_u)
                                        .set_arg(&stp_rec_decay)
                                        .set_arg(&stp_facil_decay)
                                        .set_global_work_size(num_sensory_neurons)
                                        .enqueue_nd_range(&cl.queue);
                                    if let Err(e) = launch {
                                        nm_log!("[warn] OpenCL STP sensory kernel failed: {:?}", e);
                                        ok = false;
                                    }
                                }
                                if ok {
                                    if let Err(e) = cl.queue.enqueue_read_buffer(rel, CL_TRUE, 0, &mut stp_release_s, &[]) {
                                        nm_log!("[warn] OpenCL STP sensory read failed: {:?}", e);
                                        ok = false;
                                    }
                                }
                            }
                            if ok {
                                stp_gpu_updated_s = true;
                            } else {
                                stp_gpu_failed = true;
                            }
                        }
                    }
                }
                for l in 0..num_hidden_layers {
                    if stp_gpu_failed { break; }
                    let layer_sz = self.layer_size(l);
                    if layer_sz == 0 { continue; }
                    if !self.sync_cl_stp_layer(l) { continue; }
                    if let (Some(pre), Some(u), Some(x), Some(rel)) = (
                        self.cl_stp_pre_h.get_mut(l).and_then(|b| b.as_mut()),
                        self.cl_stp_u_h.get_mut(l).and_then(|b| b.as_mut()),
                        self.cl_stp_x_h.get_mut(l).and_then(|b| b.as_mut()),
                        self.cl_stp_rel_h.get_mut(l).and_then(|b| b.as_mut()),
                    ) {
                        if let Some(ref cl) = self.cl {
                            let mut ok = true;
                            unsafe {
                                if let Some(v) = prev_spk_h[l].as_slice() {
                                    if let Err(e) = cl.queue.enqueue_write_buffer(pre, CL_TRUE, 0, v, &[]) {
                                        nm_log!("[warn] OpenCL STP hidden[{}] write failed: {:?}", l, e);
                                        ok = false;
                                    }
                                } else {
                                    ok = false;
                                }
                                if ok {
                                    let kernel = cl.kernel_stp_update.lock().unwrap();
                                    let launch = ExecuteKernel::new(&kernel)
                                        .set_arg(&*u)
                                        .set_arg(&*x)
                                        .set_arg(&*pre)
                                        .set_arg(&*rel)
                                        .set_arg(&bio.stp_u)
                                        .set_arg(&stp_rec_decay)
                                        .set_arg(&stp_facil_decay)
                                        .set_global_work_size(layer_sz)
                                        .enqueue_nd_range(&cl.queue);
                                    if let Err(e) = launch {
                                        nm_log!("[warn] OpenCL STP hidden[{}] kernel failed: {:?}", l, e);
                                        ok = false;
                                    }
                                }
                                if ok {
                                    if let Err(e) = cl.queue.enqueue_read_buffer(rel, CL_TRUE, 0, &mut stp_release_h[l], &[]) {
                                        nm_log!("[warn] OpenCL STP hidden[{}] read failed: {:?}", l, e);
                                        ok = false;
                                    }
                                }
                            }
                            if ok {
                                stp_gpu_updated_h[l] = true;
                            } else {
                                stp_gpu_failed = true;
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "opencl")]
            if stp_gpu_failed {
                self.sync_stp_state_from_gpu();
                self.cl_stp_ok = false;
                self.clear_cl_stp_buffers();
            }
            if !stp_gpu_updated_s {
                for i in 0..num_sensory_neurons {
                    let bio = {
                        #[cfg(feature = "growth3d")]
                        { &self.bio_s[i] }
                        #[cfg(not(feature = "growth3d"))]
                        { &self.net.aarnn_bio }
                    };
                    let d = Self::get_decays_static(self.lif.dt, bio);
                    self.stp_u_s[i] = self.stp_u_s[i] * d.stp_facil_decay + bio.stp_u * (1.0 - d.stp_facil_decay);
                    self.stp_x_s[i] = self.stp_x_s[i] * d.stp_rec_decay + (1.0 - d.stp_rec_decay);
                    if s_t[i] != 0 {
                        let rel = (self.stp_u_s[i] * self.stp_x_s[i]).clamp(0.0, 1.0);
                        self.stp_x_s[i] = (self.stp_x_s[i] - rel).max(0.0);
                        self.stp_u_s[i] = (self.stp_u_s[i] + bio.stp_u * (1.0 - self.stp_u_s[i])).clamp(0.0, 1.0);
                        stp_release_s[i] = rel;
                    }
                }
            }
            for l in 0..num_hidden_layers {
                if stp_gpu_updated_h[l] { continue; }
                let layer_sz = self.layer_size(l);
                for j in 0..layer_sz {
                    let bio = {
                        #[cfg(feature = "growth3d")]
                        { &self.bio_h[l][j] }
                        #[cfg(not(feature = "growth3d"))]
                        { &self.net.aarnn_bio }
                    };
                    let d = Self::get_decays_static(self.lif.dt, bio);
                    self.stp_u_h[l][j] = self.stp_u_h[l][j] * d.stp_facil_decay + bio.stp_u * (1.0 - d.stp_facil_decay);
                    self.stp_x_h[l][j] = self.stp_x_h[l][j] * d.stp_rec_decay + (1.0 - d.stp_rec_decay);
                    if prev_spk_h[l][j] != 0 {
                        let rel = (self.stp_u_h[l][j] * self.stp_x_h[l][j]).clamp(0.0, 1.0);
                        self.stp_x_h[l][j] = (self.stp_x_h[l][j] - rel).max(0.0);
                        self.stp_u_h[l][j] = (self.stp_u_h[l][j] + bio.stp_u * (1.0 - self.stp_u_h[l][j])).clamp(0.0, 1.0);
                        stp_release_h[l][j] = rel;
                    }
                }
            }
        }

        if use_adaptive_threshold {
            for l in 0..num_hidden_layers {
                for j in 0..self.v_h[l].len() {
                    let d = {
                        #[cfg(feature = "growth3d")]
                        { Self::get_decays_static(self.lif.dt, &self.bio_h[l][j]).thr_decay }
                        #[cfg(not(feature = "growth3d"))]
                        { thr_decay }
                    };
                    self.thr_offset_h[l][j] *= d;
                }
            }
            for k in 0..num_output_neurons {
                let d = {
                    #[cfg(feature = "growth3d")]
                    { Self::get_decays_static(self.lif.dt, &self.bio_o[k]).thr_decay }
                    #[cfg(not(feature = "growth3d"))]
                    { thr_decay }
                };
                self.thr_offset_o[k] *= d;
            }
        }
        if use_homeostasis {
            for l in 0..num_hidden_layers {
                for j in 0..self.v_h[l].len() {
                    let d = {
                        #[cfg(feature = "growth3d")]
                        { Self::get_decays_static(self.lif.dt, &self.bio_h[l][j]).homeo_decay }
                        #[cfg(not(feature = "growth3d"))]
                        { homeo_decay }
                    };
                    self.rate_ema_h[l][j] *= d;
                }
            }
            for k in 0..num_output_neurons {
                let d = {
                    #[cfg(feature = "growth3d")]
                    { Self::get_decays_static(self.lif.dt, &self.bio_o[k]).homeo_decay }
                    #[cfg(not(feature = "growth3d"))]
                    { homeo_decay }
                };
                self.rate_ema_o[k] *= d;
            }
        }

        // Pre-calculate active indices for sparse accumulation (avoids O(N*M) dense loops)
        let active_s_indices: Vec<usize> = s_t.iter().enumerate()
            .filter(|(_, &s)| s != 0)
            .map(|(i, _)| i)
            .collect();
        let mut active_h_indices = Vec::with_capacity(num_hidden_layers);
        for l in 0..num_hidden_layers {
            let active: Vec<usize> = self.last_spk_h[l].iter().enumerate()
                .filter(|(_, &s)| s != 0)
                .map(|(j, _)| j)
                .collect();
            active_h_indices.push(active);
        }

        // Update neuromodulator and resonance state (AARNN only)
        if is_aarnn {
            let sensory_rate = if num_sensory_neurons > 0 {
                active_s_indices.len() as f32 / num_sensory_neurons as f32
            } else {
                0.0
            };
            let output_rate = if num_output_neurons > 0 {
                let active_out = self.last_spk_o.iter().filter(|&&s| s != 0).count();
                active_out as f32 / num_output_neurons as f32
            } else {
                0.0
            };
            let mut total_hidden = 0usize;
            let mut active_hidden = 0usize;
            for l in 0..num_hidden_layers {
                let layer_len = self.layer_size(l);
                total_hidden += layer_len;
                active_hidden += active_h_indices[l].len();
            }
            let hidden_rate = if total_hidden > 0 {
                active_hidden as f32 / total_hidden as f32
            } else {
                0.0
            };

            let world_model_err = if self.net.world_model_enabled
                && !self.world_model_state.is_empty()
                && self.world_model_state.len() == self.world_model_prev_state.len()
            {
                let mut sum = 0.0f32;
                for (a, b) in self.world_model_state.iter().zip(self.world_model_prev_state.iter()) {
                    let diff = (*a - *b).abs() as f32;
                    sum += diff / (1.0 + diff);
                }
                sum / (self.world_model_state.len() as f32)
            } else {
                0.0
            };

            let decay = self.net.aarnn_neuromod_decay.clamp(0.0, 1.0);
            let retain = 1.0 - decay;
            let err = (perceptual_mean_err as f32).clamp(0.0, 1.0);
            let stability = (1.0 - err).max(0.0);
            let reward_proxy = (self.net.aarnn_reward_proxy + self.external_reward).clamp(0.0, 1.0);
            let base_d = self.net.aarnn_neuromod_baseline_dopamine.max(0.0);
            let base_a = self.net.aarnn_neuromod_baseline_ach.max(0.0);
            let base_s = self.net.aarnn_neuromod_baseline_serotonin.max(0.0);
            let signal_value = |sig: NeuromodSignal| -> f32 {
                match sig {
                    NeuromodSignal::None => 0.0,
                    NeuromodSignal::RewardProxy => reward_proxy,
                    NeuromodSignal::PerceptualError => err,
                    NeuromodSignal::WorldModelError => world_model_err,
                    NeuromodSignal::OutputSpikes => output_rate,
                    NeuromodSignal::SensorySpikes => sensory_rate,
                    NeuromodSignal::HiddenSpikes => hidden_rate,
                    NeuromodSignal::Stability => stability,
                }
            };
            let target_d = (base_d + self.net.aarnn_neuromod_error_gain.max(0.0) * signal_value(self.net.aarnn_neuromod_dopamine_signal)).clamp(0.0, 3.0);
            let target_a = (base_a + self.net.aarnn_neuromod_activity_gain.max(0.0) * signal_value(self.net.aarnn_neuromod_ach_signal)).clamp(0.0, 3.0);
            let target_s = (base_s + self.net.aarnn_neuromod_stability_gain.max(0.0) * signal_value(self.net.aarnn_neuromod_serotonin_signal)).clamp(0.0, 3.0);
            self.neuromod_dopamine = self.neuromod_dopamine * retain + target_d * decay;
            self.neuromod_ach = self.neuromod_ach * retain + target_a * decay;
            self.neuromod_serotonin = self.neuromod_serotonin * retain + target_s * decay;

            let r_decay = self.net.aarnn_resonance_decay.clamp(0.0, 1.0);
            let r_retain = 1.0 - r_decay;
            let r_target = hidden_rate.clamp(0.0, 1.0);
            self.resonance_level = self.resonance_level * r_retain + r_target * r_decay;
        }

        {
            // push sensory spikes (including feedback) to history (front)
            self.spk_hist_s.push_front(Array1::from_vec(s_t.clone()));
            while self.spk_hist_s.len() > self.hist_len { self.spk_hist_s.pop_back(); }
        }

        // Update traces
        {
            observe_time!("Runner::step/traces");
            self.x_pre_in.mapv_inplace(|x| x * self.decay_pre);
            for &i in &active_s_indices {
                self.x_pre_in[i] += 1.0; 
                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                if self.net.use_morphology && i < self.morph.sensory_somas.len() {
                    self.morph.sensory_somas[i].stimuli += 1.0;
                }
            } 
            for l in 0..num_hidden_layers { self.x_post_h[l].mapv_inplace(|x| x * self.decay_post); self.x_pre_h[l].mapv_inplace(|x| x * self.decay_pre); }
            self.x_post_o.mapv_inplace(|x| x * self.decay_post);
        }

        // Layer 0 input current
        // Hidden layer 0 input current uses actual size num_hidden_0_neurons
        let mut i_h0 = Array1::<f64>::zeros(num_hidden_0_neurons);

        if is_aarnn {
            let dt_f32 = self.lif.dt as f32;
            // AARNN Layer 0: Theta rhythm drive (deterministic) or random spiking fallback
            let use_theta = self.net.theta_rhythm_enabled && self.net.theta_rhythm_hz > 0.0;
            if use_theta {
                let dt_s = (dt_f32.max(0.001)) / 1000.0;
                let step = std::f32::consts::TAU * self.net.theta_rhythm_hz * dt_s;
                self.theta_phase = (self.theta_phase + step) % std::f32::consts::TAU;
                let duty = self.net.theta_rhythm_duty.clamp(0.01, 1.0);
                let thresh = 1.0 - duty;
                let drive = self.net.theta_rhythm_drive.max(0.0) as f64;
                let jitter = self.net.theta_rhythm_phase_jitter.clamp(0.0, 1.0);
                #[inline(always)]
                fn phase_offset(j: usize) -> f32 {
                    let h = (j as u32).wrapping_mul(2654435761) & 0xFFFF;
                    (h as f32) / 65535.0 * std::f32::consts::TAU
                }
                for j in 0..num_hidden_0_neurons {
                    let offset = if jitter > 0.0 { phase_offset(j) * jitter } else { 0.0 };
                    let gate = (self.theta_phase + offset).sin() * 0.5 + 0.5;
                    if gate >= thresh {
                        i_h0[j] += drive;
                    }
                }
            } else {
                // Random spiking from "initial synaptic energy"
                let randomness = self.net.aarnn_synaptic_energy_randomness * dt_f32;
                for j in 0..num_hidden_0_neurons {
                    let r = fastrand::f32();
                    if r < randomness {
                        i_h0[j] += 10.0; // spike the neuron
                    }
                }
            }

            // Perceptual prediction error drive (global)
            if perceptual_error_drive > 0.0 {
                for j in 0..num_hidden_0_neurons {
                    i_h0[j] += perceptual_error_drive;
                }
            }

            // AARNN Skull-based spontaneous spiking for all hidden neurons
            let ambient = self.net.aarnn_ambient_energy_level * dt_f32;
            if ambient > 0.0 {
                #[cfg(feature = "morpho")]
                if self.net.use_morphology {
                    if let Some(ref skull) = self.morph.skull_membrane {
                        for l in 0..num_hidden_layers {
                            let nj = self.layer_size(l);
                            for j in 0..nj {
                                if l < self.morph.somas.len() && j < self.morph.somas[l].len() {
                                    let pos = self.morph.somas[l][j].pos;
                                    let dist = pos.dist(skull.center);
                                    if dist < skull.radius {
                                        let factor = (1.0 - dist / skull.radius).max(0.0);
                                        // Ambient spiking probability (scaled for stability)
                                        if fastrand::f32() < ambient * factor * 0.05 {
                                            self.v_h[l][j] += 10.0;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                #[cfg(not(feature = "morpho"))]
                {
                    // Fallback if morphology not enabled: global probability
                    for l in 0..num_hidden_layers {
                        let nj = self.layer_size(l);
                        for j in 0..nj {
                            if fastrand::f32() < ambient * 0.005 {
                                self.v_h[l][j] += 10.0;
                            }
                        }
                    }
                }
            }

            // AARNN resonance: recent spiking can re-seed oscillations (pseudo-spontaneous drive)
            let resonance_gain = self.net.aarnn_resonance_gain.max(0.0) * dt_f32;
            if resonance_gain > 0.0 {
                let ambient_scale = 1.0 + self.net.aarnn_ambient_energy_level.max(0.0);
                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                let ambient_scale = if self.net.use_morphology {
                    if let Some(ref skull) = self.morph.skull_membrane {
                        let skull_e = skull.energy_fluctuation.max(0.0);
                        1.0 + self.net.aarnn_ambient_energy_level.max(skull_e)
                    } else {
                        ambient_scale
                    }
                } else {
                    ambient_scale
                };
                for l in 0..num_hidden_layers {
                    let v_layer = &mut self.v_h[l];
                    let x_layer = &self.x_post_h[l];
                    let nj = v_layer.len();
                    for j in 0..nj {
                        let trace = x_layer[j] as f32;
                        let resonance = trace / (1.0 + trace);
                        if fastrand::f32() < resonance_gain * resonance * ambient_scale {
                            v_layer[j] += 10.0;
                        }
                    }
                }
            }
        } else if self.is_layer_assigned(0) {
            observe_time!("Runner::step/i_h0");
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_success = false;
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_filtered_h0 = false;
            #[cfg(feature = "opencl")]
            {
                let cl_mgr = self.cl.clone();
                if let Some(ref cl) = cl_mgr {
                    let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                    if !use_aarnn || !self.net.use_morphology {
                        // Dense path acceleration
                        self.sync_cl_w_in_to_gpu();
                        // Need sensory spikes on GPU
                        let s_len = num_sensory_neurons;
                        if self.cl_s_t.is_none() || self.cl_s_t_size != s_len {
                            if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, s_len * std::mem::size_of::<i8>(), ptr::null_mut()) } {
                                self.cl_s_t = Some(new_buf);
                                self.cl_s_t_size = s_len;
                            }
                        }
                        
                        self.sync_cl_buffers(0, false);
                        if use_synaptic_filter {
                            self.sync_cl_syn_buffers(0, false);
                        }
                        if use_stp {
                            self.sync_cl_stp_sensory();
                        }
                        let w_buf_opt = self.cl_w_in.as_ref();
                        let s_buf_ptr = self.cl_s_t.as_mut().map(|b| b as *mut Buffer<i8>);
                        let h0_buf_ptr = self.cl_buffers_h.get_mut(0).and_then(|o| o.as_mut()).map(|b| b as *mut CLBuffers);
                        let rel_ptr = if use_stp { self.cl_stp_rel_s.as_mut().map(|b| b as *mut Buffer<f64>) } else { None };
                        let syn_ptrs = if use_synaptic_filter {
                            match (
                                self.cl_syn_ampa_h.get_mut(0).and_then(|b| b.as_mut()),
                                self.cl_syn_nmda_h.get_mut(0).and_then(|b| b.as_mut()),
                                self.cl_syn_gaba_h.get_mut(0).and_then(|b| b.as_mut()),
                            ) {
                                (Some(a), Some(n), Some(g)) => Some((a as *mut Buffer<f64>, n as *mut Buffer<f64>, g as *mut Buffer<f64>)),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        if let (Some(w_buf), Some(s_buf_ptr), Some(h0_buf_ptr)) = (w_buf_opt, s_buf_ptr, h0_buf_ptr) {
                            unsafe {
                                let s_buf = &mut *s_buf_ptr;
                                let h0_buf = &mut *h0_buf_ptr;
                                let mut use_stp_kernel = false;
                                let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                if let Err(e) = cl.queue.enqueue_write_buffer(s_buf, CL_TRUE, 0, &s_t, &[]) {
                                    nm_log!("[warn] OpenCL dense write failed: {:?}", e);
                                    gpu_success = false;
                                }
                                if gpu_success {
                                    if let Some(ptr) = rel_ptr {
                                        let rel = &mut *ptr;
                                        if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_s, &[]) {
                                            nm_log!("[warn] OpenCL dense STP rel write failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            rel_buf_opt = Some(rel);
                                            use_stp_kernel = true;
                                        }
                                    }
                                }
                                if gpu_success {
                                    if use_stp_kernel {
                                        if let Some(rel_buf) = rel_buf_opt {
                                            let kernel_acc = cl.kernel_syn_acc_stp.lock().unwrap();
                                            let launch = ExecuteKernel::new(&kernel_acc)
                                                .set_arg(&h0_buf.i_total)
                                                .set_arg(&*rel_buf)
                                                .set_arg(w_buf)
                                                .set_arg(&(s_len as i32))
                                                .set_arg(&(num_hidden_0_neurons as i32))
                                                .set_global_work_size(num_hidden_0_neurons)
                                                .enqueue_nd_range(&cl.queue);
                                            if let Err(e) = launch {
                                                nm_log!("[warn] OpenCL dense acc stp failed: {:?}", e);
                                                gpu_success = false;
                                            }
                                        } else {
                                            gpu_success = false;
                                        }
                                    } else {
                                        let kernel_acc = cl.kernel_syn_acc.lock().unwrap();
                                        let launch = ExecuteKernel::new(&kernel_acc)
                                            .set_arg(&h0_buf.i_total)
                                            .set_arg(s_buf)
                                            .set_arg(w_buf)
                                            .set_arg(&(s_len as i32))
                                            .set_arg(&(num_hidden_0_neurons as i32))
                                            .set_global_work_size(num_hidden_0_neurons)
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL dense acc failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    }
                                }
                                if gpu_success {
                                    if let Some((a_ptr, n_ptr, g_ptr)) = syn_ptrs {
                                        let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                        let launch = ExecuteKernel::new(&kernel_filter)
                                            .set_arg(&h0_buf.i_total)
                                            .set_arg(&mut *a_ptr)
                                            .set_arg(&mut *n_ptr)
                                            .set_arg(&mut *g_ptr)
                                            .set_arg(&syn_decay_ampa)
                                            .set_arg(&syn_decay_nmda)
                                            .set_arg(&syn_decay_gaba)
                                            .set_arg(&bio.nmda_ratio)
                                            .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                            .set_global_work_size(num_hidden_0_neurons)
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL dense filter failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            gpu_filtered_h0 = true;
                                        }
                                    }
                                }

                                if gpu_success {
                                    let mut i_vec = vec![0.0; num_hidden_0_neurons];
                                    if let Err(e) = cl.queue.enqueue_read_buffer(&h0_buf.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                        nm_log!("[warn] OpenCL dense i_total read failed: {:?}", e);
                                        gpu_success = false;
                                    } else {
                                        i_h0 = Array1::from_vec(i_vec);
                                        if use_synaptic_filter {
                                            self.sync_syn_state_from_gpu(0, false);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // Sparse path acceleration
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            self.sync_cl_sparse_in();
                            self.sync_cl_spk_hist_s();
                            self.sync_cl_buffers(0, false);

                            let s_len = num_sensory_neurons;
                            let hist_len = self.spk_hist_s.len();
                            if use_synaptic_filter {
                                self.sync_cl_syn_buffers(0, false);
                            }
                            if use_stp {
                                self.sync_cl_stp_sensory();
                            }
                            let rel_ptr = if use_stp { self.cl_stp_rel_s.as_mut().map(|b| b as *mut Buffer<f64>) } else { None };
                            let syn_ptrs = if use_synaptic_filter {
                                match (
                                    self.cl_syn_ampa_h.get_mut(0).and_then(|b| b.as_mut()),
                                    self.cl_syn_nmda_h.get_mut(0).and_then(|b| b.as_mut()),
                                    self.cl_syn_gaba_h.get_mut(0).and_then(|b| b.as_mut()),
                                ) {
                                    (Some(a), Some(n), Some(g)) => Some((a as *mut Buffer<f64>, n as *mut Buffer<f64>, g as *mut Buffer<f64>)),
                                    _ => None,
                                }
                            } else {
                                None
                            };

                            if let (Some(h0_buf_ptr), Some(hist_ptr), Some(sparse_ptr)) = (self.cl_buffers_h.get_mut(0).and_then(|o| o.as_mut()), self.cl_spk_hist_s.as_mut(), self.cl_sparse_in.as_mut()) {
                                unsafe {
                                    let h0_buf = &mut *h0_buf_ptr;
                                    let s_hist_buf = &mut *hist_ptr;
                                    let sparse_in = &mut *sparse_ptr;
                                    let mut use_stp_kernel = false;
                                    let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                    if let Some(ptr) = rel_ptr {
                                        let rel = &mut *ptr;
                                        if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_s, &[]) {
                                            nm_log!("[warn] OpenCL sparse STP rel write failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            rel_buf_opt = Some(rel);
                                            use_stp_kernel = true;
                                        }
                                    }
                                    if gpu_success {
                                        if use_stp_kernel {
                                            if let (Some(rel_buf), Some(delays)) = (rel_buf_opt, sparse_in.delays.as_ref()) {
                                                let kernel_acc = cl.kernel_syn_acc_sparse_delay_stp.lock().unwrap();
                                                let launch = ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&h0_buf.i_total)
                                                    .set_arg(s_hist_buf)
                                                    .set_arg(rel_buf)
                                                    .set_arg(&sparse_in.row_ptr)
                                                    .set_arg(&sparse_in.col_indices)
                                                    .set_arg(delays)
                                                    .set_arg(&sparse_in.weights)
                                                    .set_arg(&(num_hidden_0_neurons as i32))
                                                    .set_arg(&(hist_len as i32))
                                                    .set_arg(&(s_len as i32))
                                                    .set_arg(&0i32) // Mode: set
                                                    .set_global_work_size(num_hidden_0_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL sparse acc stp failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            } else {
                                                gpu_success = false;
                                            }
                                        } else {
                                            if let Some(delays) = sparse_in.delays.as_ref() {
                                                let kernel_acc = cl.kernel_syn_acc_sparse_delay.lock().unwrap();
                                                let launch = ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&h0_buf.i_total)
                                                    .set_arg(s_hist_buf)
                                                    .set_arg(&sparse_in.row_ptr)
                                                    .set_arg(&sparse_in.col_indices)
                                                    .set_arg(delays)
                                                    .set_arg(&sparse_in.weights)
                                                    .set_arg(&(num_hidden_0_neurons as i32))
                                                    .set_arg(&(hist_len as i32))
                                                    .set_arg(&(s_len as i32))
                                                    .set_arg(&0i32) // Mode: set
                                                    .set_global_work_size(num_hidden_0_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL sparse acc failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            } else {
                                                gpu_success = false;
                                            }
                                        }
                                    }

                                    if gpu_success {
                                        if let Some((a_ptr, n_ptr, g_ptr)) = syn_ptrs {
                                            let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                            let launch = ExecuteKernel::new(&kernel_filter)
                                                .set_arg(&h0_buf.i_total)
                                                .set_arg(&mut *a_ptr)
                                                .set_arg(&mut *n_ptr)
                                                .set_arg(&mut *g_ptr)
                                                .set_arg(&syn_decay_ampa)
                                                .set_arg(&syn_decay_nmda)
                                                .set_arg(&syn_decay_gaba)
                                                .set_arg(&bio.nmda_ratio)
                                                .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                                .set_global_work_size(num_hidden_0_neurons)
                                                .enqueue_nd_range(&cl.queue);
                                            if let Err(e) = launch {
                                                nm_log!("[warn] OpenCL sparse filter failed: {:?}", e);
                                                gpu_success = false;
                                            } else {
                                                gpu_filtered_h0 = true;
                                            }
                                        }
                                    }

                                    if gpu_success {
                                        let mut i_vec = vec![0.0; num_hidden_0_neurons];
                                        if let Err(e) = cl.queue.enqueue_read_buffer(&h0_buf.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                            nm_log!("[warn] OpenCL sparse i_total read failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            i_h0 = Array1::from_vec(i_vec);
                                            if use_synaptic_filter {
                                                self.sync_syn_state_from_gpu(0, false);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !gpu_success {
                if can_parallel_light(num_hidden_0_neurons) {
                    // Parallel over postsynaptic neurons j. Accumulate directly into i_h0[j].
                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                    {
                        let released_cap = 256usize;
                        let events_tls: Vec<(usize, f64, Vec<ReleasedEvent>)> = (0..num_hidden_0_neurons)
                            .into_par_iter()
                            .map(|j| {
                                let mut acc = 0.0f64;
                                let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                                let mut events: Vec<ReleasedEvent> = Vec::new();
                                let in_l = in_l; // already captured at top of step
                                if use_aarnn && self.net.use_morphology {
                                    if in_l == 0 {
                                        for &(i, syn_idx) in self.recv_in.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                                            let (steps, atten) = self.syn_delay_and_atten(syn_idx);
                                            let s = self.hist_s_at(steps, i);
                                            if s != 0 {
                                                let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                                if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                    let w_val = self.w_in.get((j, i)).copied().unwrap_or_else(|| {
                                                        nm_log!("[warn] w_in event acc out of bounds: ({}, {})", j, i);
                                                        0.0
                                                    });
                                                    acc += w_val * atten * stp_scale;
                                                    if events.len() < released_cap {
                                                        events.push(ReleasedEvent{
                                                            kind: ReleasedKind::In,
                                                            pre_layer: -1,
                                                            post_layer: 0,
                                                            pre_id: i,
                                                            post_id: j,
                                                            syn_idx: Some(syn_idx),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // Recurrent current for Layer 0
                                    for &(i, si) in self.recv_rec.get(0).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, atten) = self.syn_delay_and_atten(si);
                                        if self.hist_h_at(0, steps, i) != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(0).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(si)) {
                                                let w_val = self.w_hh_rec.get(0).and_then(|m| m.get((j, i))).copied().unwrap_or(0.0);
                                                acc += w_val * atten * stp_scale;
                                                if events.len() < released_cap {
                                                    events.push(ReleasedEvent{
                                                        kind: ReleasedKind::HiddenRec { layer: 0 },
                                                        pre_layer: 0,
                                                        post_layer: 0,
                                                        pre_id: i,
                                                        post_id: j,
                                                        syn_idx: Some(si),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                } else if use_aarnn {
                                    if in_l == 0 {
                                        // Legacy distance-based AARNN path when morphology disabled
                                        #[allow(unused_variables)]
                                        let vel = self.net.aarnn_velocity.max(0.0);
                                        for i in 0..self.net.num_sensory_neurons {
                                            #[cfg(feature = "growth3d")]
                                            let dist = {
                                                let snode = &self.topo.sensory_nodes[i];
                                                if let Some(nodes0) = self.topo.layers.get(0) {
                                                    if j < nodes0.len() {
                                                        let dx = snode.x - nodes0[j].x; let dy = snode.y - nodes0[j].y; let dz = snode.z - nodes0[j].z;
                                                        (dx*dx + dy*dy + dz*dz).sqrt()
                                                    } else { 1.0 }
                                                } else { 1.0 }
                                            };
                                            #[cfg(not(feature = "growth3d"))]
                                            let dist = 1.0f32;
                                            let dt_ms = self.lif.dt as f32;
                                            let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                            let s = {
                                                #[cfg(feature = "growth3d")]
                                                { let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1)); let frame = &self.spk_hist_s[idx]; if frame.len()==0 {0} else { let ii=i.min(frame.len()-1); frame[ii] } }
                                                #[cfg(not(feature = "growth3d"))]
                                                { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                            };
                                            if s != 0 {
                                                let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                                acc += self.w_in[(j,i)] * stp_scale;
                                            }
                                        }
                                    }
                                } else {
                                    if in_l == 0 {
                                        for &i in &active_s_indices {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc += self.w_in.get((j, i)).copied().unwrap_or(0.0) * stp_scale;
                                        }
                                    }
                                }
                                (j, acc, events)
                            })
                            .collect();

                        // Merge results
                        let mut total = 0usize;
                        for (j, acc, ev) in events_tls.into_iter() {
                            i_h0[j] = acc;
                            if total < released_cap {
                                let room = released_cap - total;
                                let take = ev.len().min(room);
                                self.released_events.extend(ev.into_iter().take(take));
                                total += take;
                            }
                        }
                    }
                    #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                    {
                        let results_simple: Vec<(usize, f64)> = (0..num_hidden_0_neurons).into_par_iter().map(|j| {
                            let mut acc = 0.0f64;
                            let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                            #[allow(unused_variables)]
                            let vel = self.net.aarnn_velocity.max(0.0);
                            
                            if use_aarnn {
                                for i in 0..self.net.num_sensory_neurons {
                                    #[cfg(feature = "growth3d")]
                                    let dist = {
                                        let snode = &self.topo.sensory_nodes[i];
                                        if let Some(nodes0) = self.topo.layers.get(0) {
                                            if j < nodes0.len() {
                                                let dx = snode.x - nodes0[j].x; let dy = snode.y - nodes0[j].y; let dz = snode.z - nodes0[j].z;
                                                (dx*dx + dy*dy + dz*dz).sqrt()
                                            } else { 1.0 }
                                        } else { 1.0 }
                                    };
                                    #[cfg(not(feature = "growth3d"))]
                                    let dist = 1.0f32;
                                    let dt_ms = self.lif.dt as f32;
                                    let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                    let s = {
                                        #[cfg(feature = "growth3d")]
                                        {
                                            let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1));
                                            let frame = &self.spk_hist_s[idx];
                                            if frame.len() == 0 { 0 } else { let ii = i.min(frame.len()-1); frame[ii] }
                                        }
                                        #[cfg(not(feature = "growth3d"))]
                                        { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                    };
                                    if s != 0 {
                                        let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc += self.w_in[(j,i)] * stp_scale;
                                    }
                                }
                                // Sparse recurrent for Layer 0
                                for &i in &active_h_indices[0] {
                                    let stp_scale = if use_stp { stp_release_h.get(0).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                    acc += self.w_hh_rec.get(0).and_then(|m| m.get((j, i))).copied().unwrap_or(0.0) * stp_scale;
                                }
                            } else {
                                for &i in &active_s_indices {
                                    let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                    acc += self.w_in.get((j, i)).copied().unwrap_or(0.0) * stp_scale;
                                }
                            }
                            (j, acc)
                        }).collect();
                        for (j, acc) in results_simple.into_iter() { i_h0[j] = acc; }
                    }
                } else {
                    for j in 0..num_hidden_0_neurons {
                        let mut acc = 0.0;
                        let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                        #[allow(unused_variables)]
                        let vel = self.net.aarnn_velocity.max(0.0);
                        if use_aarnn {
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            if self.net.use_morphology {
                                if in_l == 0 {
                                    for &(i, syn_idx) in self.recv_in.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                        let s = self.hist_s_at(steps, i);
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                acc += self.w_in[(j,i)] * stp_scale;
                                                if self.released_events.len() < 256 {
                                                    self.released_events.push(ReleasedEvent{ kind: ReleasedKind::In, pre_layer: -1, post_layer: 0, pre_id: i, post_id: j, syn_idx: Some(syn_idx) });
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Legacy distance-based AARNN path when morphology disabled
                                if in_l == 0 {
                                    for i in 0..self.net.num_sensory_neurons {
                                        #[cfg(feature = "growth3d")]
                                        let dist = {
                                            let snode = &self.topo.sensory_nodes[i];
                                            if let Some(nodes0) = self.topo.layers.get(0) {
                                                if j < nodes0.len() {
                                                    let dx = snode.x - nodes0[j].x;
                                                    let dy = snode.y - nodes0[j].y;
                                                    let dz = snode.z - nodes0[j].z;
                                                    (dx*dx + dy*dy + dz*dz).sqrt()
                                                } else { 1.0 }
                                            } else { 1.0 }
                                        };
                                        #[cfg(not(feature = "growth3d"))]
                                        let dist = 1.0f32;
                                        let dt_ms = self.lif.dt as f32;
                                        let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                        let s = {
                                            #[cfg(feature = "growth3d")]
                                            {
                                                let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1));
                                                let frame = &self.spk_hist_s[idx];
                                                if frame.len() == 0 { 0 } else { let ii = i.min(frame.len()-1); frame[ii] }
                                            }
                                            #[cfg(not(feature = "growth3d"))]
                                            { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                        };
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc += self.w_in[(j,i)] * stp_scale;
                                        }
                                    }
                                }
                            }
                            #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                            {
                                // Morphology path unavailable at compile-time; use legacy AARNN
                                if in_l == 0 {
                                    for i in 0..self.net.num_sensory_neurons {
                                        #[cfg(feature = "growth3d")]
                                        let dist = {
                                            let snode = &self.topo.sensory_nodes[i];
                                            if let Some(nodes0) = self.topo.layers.get(0) {
                                                if j < nodes0.len() {
                                                    let dx = snode.x - nodes0[j].x; let dy = snode.y - nodes0[j].y; let dz = snode.z - nodes0[j].z;
                                                    (dx*dx + dy*dy + dz*dz).sqrt()
                                                } else { 1.0 }
                                            } else { 1.0 }
                                        };
                                        #[cfg(not(feature = "growth3d"))]
                                        let dist = 1.0f32;
                                        let dt_ms = self.lif.dt as f32;
                                        let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                        let s = {
                                            #[cfg(feature = "growth3d")]
                                            { let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1)); let frame = &self.spk_hist_s[idx]; if frame.len()==0 {0} else { let ii=i.min(frame.len()-1); frame[ii] } }
                                            #[cfg(not(feature = "growth3d"))]
                                            { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                        };
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc += self.w_in[(j,i)] * stp_scale;
                                        }
                                    }
                                }
                            }
                        } else {
                            for i in 0..num_sensory_neurons {
                                if s_t.get(i).copied().unwrap_or(0) != 0 {
                                    let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                    let w_val = self.w_in.get((j, i)).copied().unwrap_or(0.0);
                                    acc += w_val * stp_scale;
                                }
                            }
                        }
                        // Recurrent current for Layer 0
                        if is_aarnn {
                            if self.net.use_morphology {
                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                {
                                    for &(i, si) in self.recv_rec.get(0).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, atten) = self.syn_delay_and_atten(si);
                                        if self.hist_h_at(0, steps, i) != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(0).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(si)) {
                                                let w_val = self.w_hh_rec.get(0).and_then(|m| m.get((j, i))).copied().unwrap_or(0.0);
                                                acc += w_val * atten * stp_scale;
                                                if self.released_events.len() < 256 {
                                                    self.released_events.push(ReleasedEvent {
                                                        kind: ReleasedKind::HiddenRec { layer: 0 },
                                                        pre_layer: 0,
                                                        post_layer: 0,
                                                        pre_id: i,
                                                        post_id: j,
                                                        syn_idx: Some(si),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                for i in 0..num_hidden_0_neurons {
                                    let spiked = self.last_spk_h.get(0).and_then(|v| v.get(i)).copied().unwrap_or(0) != 0;
                                    if spiked {
                                        let stp_scale = if use_stp { stp_release_h.get(0).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                        let w_val = self.w_hh_rec.get(0).and_then(|m| m.get((j, i))).copied().unwrap_or(0.0);
                                        acc += w_val * stp_scale;
                                    }
                                }
                            }
                        }
                        i_h0[j] = acc;
                    }
                }
            }
        #[cfg(any(feature = "ui", feature = "growth3d"))]
        {
            self.last_i_h0 = Some(i_h0.clone());
        }
        if use_synaptic_filter && num_hidden_0_neurons > 0 && !gpu_filtered_h0 {
            i_h0 = Self::apply_synaptic_filter(
                self.lif.dt,
                &self.net.aarnn_bio,
                &i_h0,
                &mut self.syn_ampa_h[0],
                &mut self.syn_nmda_h[0],
                &mut self.syn_gaba_h[0],
                Some(&self.v_h[0]),
                self.net.aarnn_nmda_voltage_sensitivity.max(0.0) as f64,
                #[cfg(feature = "growth3d")] Some(&self.bio_h[0]),
                #[cfg(not(feature = "growth3d"))] None,
                &default_decays,
            );
        }
        if is_aarnn && num_hidden_0_neurons > 1 {
            let g_gap = self.net.aarnn_gap_junction_strength.max(0.0) as f64;
            Self::apply_gap_junction_coupling(&mut i_h0, &self.v_h[0], g_gap);
        }

        // Update hidden layer 0 (parallel-friendly via temporary buffers)
        let spk_h0 = {
            observe_time!("Runner::step/spk_h0");
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_success = false;
            #[cfg(feature = "opencl")]
            {
                let cl_mgr = self.cl.clone();
                if let Some(ref cl) = cl_mgr {
                    if !is_aarnn {
                        self.sync_cl_buffers(0, false);
                        let izh_params = self.effective_izh_params();
                        if let Some(ref mut buf) = self.cl_buffers_h.get_mut(0).and_then(|o| o.as_mut()) {
                            // Upload i_h0
                            gpu_success = true;
                            unsafe {
                                if let Some(slice) = i_h0.as_slice() {
                                    if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.i_total, CL_TRUE, 0, slice, &[]) {
                                        nm_log!("[warn] OpenCL H0 write i_total failed: {:?}", e);
                                        gpu_success = false;
                                    }
                                } else {
                                    gpu_success = false;
                                }
                            }
                            
                            if gpu_success {
                                let size = i_h0.len();
                                let kernel_lif = cl.kernel_lif_step.lock().unwrap();
                                let kernel_izh = cl.kernel_izh_step.lock().unwrap();
                                match self.neuron_model {
                                    NeuronModel::Lif => {
                                        if let Some(ref refr_buf) = buf.refr {
                                            unsafe {
                                                let launch = ExecuteKernel::new(&kernel_lif)
                                                    .set_arg(&buf.v)
                                                    .set_arg(refr_buf)
                                                    .set_arg(&buf.i_total)
                                                    .set_arg(&self.decay_m)
                                                    .set_arg(&self.lif.v_th)
                                                    .set_arg(&self.lif.v_reset)
                                                    .set_arg(&(self.lif.refractory as i32))
                                                    .set_arg(&buf.spk)
                                                    .set_global_work_size(size)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL H0 lif_step failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            }
                                        } else {
                                            gpu_success = false;
                                        }
                                    }
                                    NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                                        let p = izh_params.expect("izh params for Izh/AARNN");
                                        if let Some(ref u_buf) = buf.u {
                                            unsafe {
                                                let launch = ExecuteKernel::new(&kernel_izh)
                                                    .set_arg(&buf.v)
                                                    .set_arg(u_buf)
                                                    .set_arg(&buf.i_total)
                                                    .set_arg(&p.dt)
                                                    .set_arg(&p.recovery_time_constant_a)
                                                    .set_arg(&p.recovery_sensitivity_b)
                                                    .set_arg(&p.membrane_reset_potential_c)
                                                    .set_arg(&p.recovery_increment_d)
                                                    .set_arg(&p.v_th)
                                                    .set_arg(&buf.spk)
                                                    .set_global_work_size(size)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL H0 izh_step failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            }
                                        } else {
                                            gpu_success = false;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if gpu_success {
                #[cfg(feature = "opencl")]
                { self.sync_cl_state_from_gpu(0, false) }
                #[cfg(not(feature = "opencl"))]
                { unreachable!() }
            } else {
                match self.neuron_model {
                    NeuronModel::Lif => {
                        let refh = self.refr_h.as_ref().unwrap();
                        let (old_v_slice, old_ref_slice): (Vec<f64>, Vec<i32>) = (
                            (0..num_hidden_0_neurons).map(|j| self.v_h[0][j]).collect(),
                            (0..num_hidden_0_neurons).map(|j| refh[0][j]).collect(),
                        );
                        #[cfg(feature = "parallel")]
                        if can_parallel_heavy(num_hidden_0_neurons) {
                            let res: Vec<(f64, i32, i8)> = (0..num_hidden_0_neurons).into_par_iter().map(|j| {
                                let v = old_v_slice[j] * self.decay_m + i_h0[j];
                                let v_clamped = v.clamp(-5.0, 5.0);
                                let active = old_ref_slice[j] <= 0;
                                let did_fire = active && v_clamped >= self.lif.v_th;
                                if did_fire { (self.lif.v_reset, self.lif.refractory as i32, 1) }
                                else { (v_clamped, (old_ref_slice[j]-1).max(0), 0) }
                            }).collect();
                            for j in 0..num_hidden_0_neurons { self.v_h[0][j] = res[j].0; }
                            let refh_mut = self.refr_h.as_mut().unwrap();
                            for j in 0..num_hidden_0_neurons { refh_mut[0][j] = res[j].1; }
                            Array1::from_vec(res.into_iter().map(|t| t.2).collect())
                        } else {
                            let mut fired = vec![0i8; num_hidden_0_neurons];
                            for j in 0..num_hidden_0_neurons {
                                let v = old_v_slice[j] * self.decay_m + i_h0[j];
                                let v_clamped = v.clamp(-5.0, 5.0);
                                let active = old_ref_slice[j] <= 0;
                                let did_fire = active && v_clamped >= self.lif.v_th;
                                if did_fire { self.v_h[0][j] = self.lif.v_reset; let refh_mut = self.refr_h.as_mut().unwrap(); refh_mut[0][j] = self.lif.refractory as i32; fired[j] = 1; }
                                else { self.v_h[0][j] = v_clamped; let refh_mut = self.refr_h.as_mut().unwrap(); refh_mut[0][j] = (old_ref_slice[j]-1).max(0); }
                            }
                            Array1::from_vec(fired)
                        }
                    }
                    NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                        let p = self.effective_izh_params().expect("izh params for Izh/AARNN");
                        let uh = self.u_h.as_ref().unwrap();
                        let old_v: Vec<f64> = (0..num_hidden_0_neurons).map(|j| self.v_h[0][j]).collect();
                        let old_u: Vec<f64> = (0..num_hidden_0_neurons).map(|j| uh[0][j]).collect();
                        let old_refr: Vec<i32> = if use_izh_refractory {
                            let r = self.izh_refr_h.as_ref().unwrap();
                            (0..num_hidden_0_neurons).map(|j| r[0][j]).collect()
                        } else {
                            Vec::new()
                        };
                        if can_parallel_heavy(num_hidden_0_neurons) {
                            let res: Vec<(f64, f64, i8)> = (0..num_hidden_0_neurons).into_par_iter().map(|j| {
                                let v = old_v[j];
                                let u = old_u[j];
                                let nv = v + p.dt * (0.04*v*v + 5.0*v + 140.0 - u + i_h0[j]);
                                let nu = u + p.dt * (p.recovery_time_constant_a * (p.recovery_sensitivity_b*nv - u));
                                let mut did_fire = nv >= p.v_th;
                                if use_adaptive_threshold {
                                    let thr_offset = self.thr_offset_h[0][j].clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    did_fire = nv >= (p.v_th + thr_offset);
                                }
                                if use_izh_refractory && old_refr[j] > 0 { did_fire = false; }
                                let (nv2, nu2) = if did_fire { (p.membrane_reset_potential_c, nu + p.recovery_increment_d) } else { (nv, nu) };
                                (nv2, nu2, did_fire as i8)
                            }).collect();
                            for j in 0..num_hidden_0_neurons { self.v_h[0][j] = res[j].0; }
                            let uh_mut = self.u_h.as_mut().unwrap();
                            for j in 0..num_hidden_0_neurons { uh_mut[0][j] = res[j].1; }
                            if use_adaptive_threshold {
                                for j in 0..num_hidden_0_neurons {
                                    if res[j].2 != 0 {
                                        self.thr_offset_h[0][j] = (self.thr_offset_h[0][j] + bio.adaptive_threshold_increment)
                                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    }
                                }
                            }
                            if use_izh_refractory {
                                if let Some(r) = self.izh_refr_h.as_mut() {
                                    for j in 0..num_hidden_0_neurons {
                                        if res[j].2 != 0 { r[0][j] = izh_refractory_steps; }
                                        else { r[0][j] = (r[0][j] - 1).max(0); }
                                    }
                                }
                            }
                            Array1::from_vec(res.into_iter().map(|t| t.2).collect())
                        } else {
                            let mut fired = vec![0i8; num_hidden_0_neurons];
                            for j in 0..num_hidden_0_neurons {
                                let v = old_v[j];
                                let u = old_u[j];
                                let nv = v + p.dt * (0.04*v*v + 5.0*v + 140.0 - u + i_h0[j]);
                                let nu = u + p.dt * (p.recovery_time_constant_a * (p.recovery_sensitivity_b*nv - u));
                                let mut did_fire = nv >= p.v_th;
                                if use_adaptive_threshold {
                                    let thr_offset = self.thr_offset_h[0][j].clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    did_fire = nv >= (p.v_th + thr_offset);
                                }
                                if use_izh_refractory {
                                    if let Some(r) = self.izh_refr_h.as_ref() {
                                        if r[0][j] > 0 { did_fire = false; }
                                    }
                                }
                                let (nv2, nu2) = if did_fire { (p.membrane_reset_potential_c, nu + p.recovery_increment_d) } else { (nv, nu) };
                                self.v_h[0][j] = nv2; let uh_mut = self.u_h.as_mut().unwrap(); uh_mut[0][j] = nu2; fired[j] = did_fire as i8;
                                if use_adaptive_threshold && did_fire {
                                    self.thr_offset_h[0][j] = (self.thr_offset_h[0][j] + bio.adaptive_threshold_increment)
                                        .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                }
                                if use_izh_refractory {
                                    if let Some(r) = self.izh_refr_h.as_mut() {
                                        if did_fire { r[0][j] = izh_refractory_steps; }
                                        else { r[0][j] = (r[0][j] - 1).max(0); }
                                    }
                                }
                            }
                            Array1::from_vec(fired)
                        }
                    }
                }
            }
        };
        self.last_spk_h[0] = spk_h0.clone();
        {
            // push current spikes to history (front)
            if let Some(dq) = self.spk_hist_h.get_mut(0) {
                dq.push_front(spk_h0.clone());
                while dq.len() > self.hist_len { dq.pop_back(); }
            }
        }
        for j in 0..num_hidden_0_neurons { if spk_h0[j] != 0 { self.x_post_h[0][j]+=1.0; self.x_pre_h[0][j]+=1.0; } }
        if use_homeostasis {
            for j in 0..num_hidden_0_neurons {
                if spk_h0[j] != 0 { self.rate_ema_h[0][j] += 1.0 - homeo_decay; }
                let err = self.rate_ema_h[0][j] - base_homeo_target;
                self.thr_offset_h[0][j] += bio.homeostasis_gain * err;
            }
        }
        #[cfg(feature = "growth3d")]
        if self.net.growth_enabled {
            for j in 0..num_hidden_0_neurons {
                let r = self.rate_h[0][j] * decay_rate + if spk_h0[j] != 0 { 1.0 } else { 0.0 };
                self.rate_h[0][j] = r;
                self.since_growth_ms[0][j] += dt_ms;
            }
        }
        }

        // Next layers 1..num_hidden_layers-1
        {
            observe_time!("Runner::step/hidden_layers");
            for l in 1..num_hidden_layers {
            if !self.is_layer_assigned(l) { continue; }
            let num_current_hidden_neurons = self.layer_size(l);
            let num_previous_hidden_neurons = self.layer_size(l - 1);
            let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);

            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_filtered = false;
            let (i_f, i_b) = {
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                let mut gpu_success = false;
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                let mut i_f = Array1::<f64>::zeros(num_current_hidden_neurons);
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                let mut i_b = Array1::<f64>::zeros(num_current_hidden_neurons);

                #[cfg(feature = "opencl")]
                let cl_mgr = self.cl.clone();
                #[cfg(feature = "opencl")]
                if let Some(ref cl) = cl_mgr {
                    if !use_aarnn {
                        self.sync_cl_w_hh_to_gpu(l - 1);
                        self.sync_cl_buffers(l - 1, false);
                        self.sync_cl_buffers(l, false);
                        
                    if use_stp {
                        self.sync_cl_stp_layer(l - 1);
                    }
                    let cl_fwd_ptr = self.cl_w_hh_fwd.get(l - 1).and_then(|o| o.as_ref()).map(|b| b as *const Buffer<f64>);
                    // Access buffers via raw pointers to bypass borrow checker while ensuring sequential access
                    let buf_prev_ptr = if let Some(Some(ref b)) = self.cl_buffers_h.get(l-1) { Some(b as *const CLBuffers) } else { None };
                    let buf_cur_ptr = if let Some(Some(ref mut b)) = self.cl_buffers_h.get_mut(l) { Some(b as *mut CLBuffers) } else { None };

                    if let (Some(cl_fwd_ptr), Some(buf_prev_p), Some(buf_cur_p)) = (cl_fwd_ptr, buf_prev_ptr, buf_cur_ptr) {
                        let buf_prev = unsafe { &*buf_prev_p };
                        let buf_cur = unsafe { &mut *buf_cur_p };
                        let cl_fwd = unsafe { &*cl_fwd_ptr };

                        unsafe {
                            let mut use_stp_kernel = false;
                            let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                            let rel_ptr = if use_stp { self.cl_stp_rel_h.get_mut(l - 1).and_then(|b| b.as_mut()).map(|b| b as *mut Buffer<f64>) } else { None };
                            gpu_success = true;
                            if let Some(ptr) = rel_ptr {
                                let rel = &mut *ptr;
                                if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_h[l - 1], &[]) {
                                    nm_log!("[warn] OpenCL dense HH fwd rel write failed: {:?}", e);
                                    gpu_success = false;
                                } else {
                                    rel_buf_opt = Some(rel);
                                    use_stp_kernel = true;
                                }
                            }
                            if gpu_success {
                                if use_stp_kernel {
                                    if let Some(rel_buf) = rel_buf_opt {
                                        let kernel_acc = cl.kernel_syn_acc_stp.lock().unwrap();
                                        let launch = ExecuteKernel::new(&kernel_acc)
                                            .set_arg(&buf_cur.i_total)
                                            .set_arg(rel_buf)
                                            .set_arg(cl_fwd)
                                            .set_arg(&(num_previous_hidden_neurons as i32))
                                            .set_arg(&(num_current_hidden_neurons as i32))
                                            .set_global_work_size(num_current_hidden_neurons)
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL dense HH fwd acc stp failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    } else {
                                        gpu_success = false;
                                    }
                                } else {
                                    let kernel_acc = cl.kernel_syn_acc.lock().unwrap();
                                    let launch = ExecuteKernel::new(&kernel_acc)
                                        .set_arg(&buf_cur.i_total)
                                        .set_arg(&buf_prev.spk)
                                        .set_arg(cl_fwd)
                                        .set_arg(&(num_previous_hidden_neurons as i32))
                                        .set_arg(&(num_current_hidden_neurons as i32))
                                        .set_global_work_size(num_current_hidden_neurons)
                                        .enqueue_nd_range(&cl.queue);
                                    if let Err(e) = launch {
                                        nm_log!("[warn] OpenCL dense HH fwd acc failed: {:?}", e);
                                        gpu_success = false;
                                    }
                                }
                            }
                                
                            if gpu_success && use_synaptic_filter {
                                self.sync_cl_syn_buffers(l, false);
                                if let (Some(ref mut a), Some(ref mut n), Some(ref mut g)) = (&mut self.cl_syn_ampa_h[l], &mut self.cl_syn_nmda_h[l], &mut self.cl_syn_gaba_h[l]) {
                                    let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                    let launch = ExecuteKernel::new(&kernel_filter)
                                        .set_arg(&buf_cur.i_total)
                                        .set_arg(a)
                                        .set_arg(n)
                                        .set_arg(g)
                                        .set_arg(&syn_decay_ampa)
                                        .set_arg(&syn_decay_nmda)
                                        .set_arg(&syn_decay_gaba)
                                        .set_arg(&bio.nmda_ratio)
                                        .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                        .set_global_work_size(num_current_hidden_neurons)
                                        .enqueue_nd_range(&cl.queue);
                                    if let Err(e) = launch {
                                        nm_log!("[warn] OpenCL dense HH filter failed: {:?}", e);
                                        gpu_success = false;
                                    } else {
                                        gpu_filtered = true;
                                    }
                                }
                            }

                            if gpu_success {
                                let mut i_vec = vec![0.0; num_current_hidden_neurons];
                                if let Err(e) = cl.queue.enqueue_read_buffer(&mut buf_cur.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                    nm_log!("[warn] OpenCL dense HH fwd i_total read failed: {:?}", e);
                                    gpu_success = false;
                                } else {
                                    i_f = Array1::from_vec(i_vec);
                                }
                            }
                        }
                        
                        if l < num_hidden_layers - 1 {
                            self.sync_cl_w_hh_to_gpu(l);
                            self.sync_cl_buffers(l + 1, false);
                            if use_stp {
                                self.sync_cl_stp_layer(l + 1);
                            }
                            let cl_bwd_ptr = self.cl_w_hh_bwd.get(l).and_then(|o| o.as_ref()).map(|b| b as *const Buffer<f64>);
                            let buf_next_ptr = if let Some(Some(ref b)) = self.cl_buffers_h.get(l+1) { Some(b as *const CLBuffers) } else { None };
                            if let (Some(cl_bwd_ptr), Some(buf_next_p)) = (cl_bwd_ptr, buf_next_ptr) {
                                let buf_next = unsafe { &*buf_next_p };
                                let num_next_hidden_neurons = self.layer_size(l + 1);
                                let cl_bwd = unsafe { &*cl_bwd_ptr };
                                unsafe {
                                    let mut use_stp_kernel = false;
                                    let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                    let rel_ptr = if use_stp { self.cl_stp_rel_h.get_mut(l + 1).and_then(|b| b.as_mut()).map(|b| b as *mut Buffer<f64>) } else { None };
                                    if gpu_success {
                                        if let Some(ptr) = rel_ptr {
                                            let rel = &mut *ptr;
                                            if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_h[l + 1], &[]) {
                                                nm_log!("[warn] OpenCL dense HH bwd rel write failed: {:?}", e);
                                                gpu_success = false;
                                            } else {
                                                rel_buf_opt = Some(rel);
                                                use_stp_kernel = true;
                                            }
                                        }
                                    }
                                    if gpu_success {
                                        if use_stp_kernel {
                                            if let Some(rel_buf) = rel_buf_opt {
                                                let kernel_acc = cl.kernel_syn_acc_stp.lock().unwrap();
                                                let launch = ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&buf_cur.i_total)
                                                    .set_arg(rel_buf)
                                                    .set_arg(cl_bwd)
                                                    .set_arg(&(num_next_hidden_neurons as i32))
                                                    .set_arg(&(num_current_hidden_neurons as i32))
                                                    .set_global_work_size(num_current_hidden_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL dense HH bwd acc stp failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            } else {
                                                gpu_success = false;
                                            }
                                        } else {
                                            let kernel_acc = cl.kernel_syn_acc.lock().unwrap();
                                            let launch = ExecuteKernel::new(&kernel_acc)
                                                .set_arg(&buf_cur.i_total) 
                                                .set_arg(&buf_next.spk)
                                                .set_arg(cl_bwd)
                                                .set_arg(&(num_next_hidden_neurons as i32))
                                                .set_arg(&(num_current_hidden_neurons as i32))
                                                .set_global_work_size(num_current_hidden_neurons)
                                                .enqueue_nd_range(&cl.queue);
                                            if let Err(e) = launch {
                                                nm_log!("[warn] OpenCL dense HH bwd acc failed: {:?}", e);
                                                gpu_success = false;
                                            }
                                        }
                                    }
                                        
                                    if gpu_success && use_synaptic_filter && !gpu_filtered {
                                        self.sync_cl_syn_buffers(l, false);
                                        if let (Some(ref mut a), Some(ref mut n), Some(ref mut g)) = (&mut self.cl_syn_ampa_h[l], &mut self.cl_syn_nmda_h[l], &mut self.cl_syn_gaba_h[l]) {
                                            let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                            let launch = ExecuteKernel::new(&kernel_filter)
                                                .set_arg(&buf_cur.i_total)
                                                .set_arg(a)
                                                .set_arg(n)
                                                .set_arg(g)
                                                .set_arg(&syn_decay_ampa)
                                                .set_arg(&syn_decay_nmda)
                                                .set_arg(&syn_decay_gaba)
                                                .set_arg(&bio.nmda_ratio)
                                                .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                                .set_global_work_size(num_current_hidden_neurons)
                                                .enqueue_nd_range(&cl.queue);
                                            if let Err(e) = launch {
                                                nm_log!("[warn] OpenCL dense HH bwd filter failed: {:?}", e);
                                                gpu_success = false;
                                            } else {
                                                gpu_filtered = true;
                                            }
                                        }
                                    }

                                    if gpu_success {
                                        let mut i_vec = vec![0.0; num_current_hidden_neurons];
                                        if let Err(e) = cl.queue.enqueue_read_buffer(&mut buf_cur.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                            nm_log!("[warn] OpenCL dense HH bwd i_total read failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            i_b = Array1::from_vec(i_vec);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    } else if self.net.use_morphology {
                        // Sparse path acceleration
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            self.sync_cl_sparse_fwd(l - 1);
                            self.sync_cl_spk_hist_h(l - 1);
                            self.sync_cl_buffers(l, false);

                            let prev_len = num_previous_hidden_neurons;
                            let hist_len = self.spk_hist_h.get(l - 1).map(|dq| dq.len()).unwrap_or(0);
                            
                            // Using raw pointer for buf_cur to bypass borrow checker while we call sync_cl_sparse_bwd
                            let buf_cur_ptr = if let Some(Some(ref mut b)) = self.cl_buffers_h.get_mut(l) { Some(b as *mut CLBuffers) } else { None };

                            if use_stp {
                                self.sync_cl_stp_layer(l - 1);
                            }
                            let syn_ptrs = if use_synaptic_filter {
                                self.sync_cl_syn_buffers(l, false);
                                match (
                                    self.cl_syn_ampa_h.get_mut(l).and_then(|b| b.as_mut()),
                                    self.cl_syn_nmda_h.get_mut(l).and_then(|b| b.as_mut()),
                                    self.cl_syn_gaba_h.get_mut(l).and_then(|b| b.as_mut()),
                                ) {
                                    (Some(a), Some(n), Some(g)) => Some((a as *mut Buffer<f64>, n as *mut Buffer<f64>, g as *mut Buffer<f64>)),
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            let rel_ptr_fwd = if use_stp { self.cl_stp_rel_h.get_mut(l - 1).and_then(|b| b.as_mut()).map(|b| b as *mut Buffer<f64>) } else { None };
                            if let (Some(hist_buf), Some(sparse_fwd), Some(buf_cur_p)) = (self.cl_spk_hist_h[l - 1].as_mut(), self.cl_sparse_fwd[l - 1].as_mut(), buf_cur_ptr) {
                                let buf_cur = unsafe { &mut *buf_cur_p };
                                let mut cl_ok = true;
                                unsafe {
                                    // Forward
                                    let mut use_stp_kernel = false;
                                    let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                    if let Some(ptr) = rel_ptr_fwd {
                                        let rel = &mut *ptr;
                                        if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_h[l - 1], &[]) {
                                            nm_log!("[warn] OpenCL sparse HH fwd rel write failed: {:?}", e);
                                            cl_ok = false;
                                        } else {
                                            rel_buf_opt = Some(rel);
                                            use_stp_kernel = true;
                                        }
                                    }
                                    if cl_ok {
                                        let fwd_res = if use_stp_kernel {
                                            if let (Some(rel_buf), Some(delays)) = (rel_buf_opt, sparse_fwd.delays.as_ref()) {
                                                let kernel_acc = cl.kernel_syn_acc_sparse_delay_stp.lock().unwrap();
                                                ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&buf_cur.i_total)
                                                    .set_arg(hist_buf)
                                                    .set_arg(rel_buf)
                                                    .set_arg(&sparse_fwd.row_ptr)
                                                    .set_arg(&sparse_fwd.col_indices)
                                                    .set_arg(delays)
                                                    .set_arg(&sparse_fwd.weights)
                                                    .set_arg(&(num_current_hidden_neurons as i32))
                                                    .set_arg(&(hist_len as i32))
                                                    .set_arg(&(prev_len as i32))
                                                    .set_arg(&0i32) // Mode: set
                                                    .set_global_work_size(num_current_hidden_neurons)
                                                    .enqueue_nd_range(&cl.queue)
                                            } else {
                                                Err(ClError(-1))
                                            }
                                        } else {
                                            if let Some(delays) = sparse_fwd.delays.as_ref() {
                                                let kernel_acc = cl.kernel_syn_acc_sparse_delay.lock().unwrap();
                                                ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&buf_cur.i_total)
                                                    .set_arg(hist_buf)
                                                    .set_arg(&sparse_fwd.row_ptr)
                                                    .set_arg(&sparse_fwd.col_indices)
                                                    .set_arg(delays)
                                                    .set_arg(&sparse_fwd.weights)
                                                    .set_arg(&(num_current_hidden_neurons as i32))
                                                    .set_arg(&(hist_len as i32))
                                                    .set_arg(&(prev_len as i32))
                                                    .set_arg(&0i32) // Mode: set
                                                    .set_global_work_size(num_current_hidden_neurons)
                                                    .enqueue_nd_range(&cl.queue)
                                            } else {
                                                Err(ClError(-1))
                                            }
                                        };
                                        if let Err(e) = fwd_res {
                                            nm_log!("[warn] OpenCL sparse fwd kernel failed: {:?}", e);
                                            cl_ok = false;
                                        }
                                    }
                                    
                                    if cl_ok {
                                        if let Some((a_ptr, n_ptr, g_ptr)) = syn_ptrs {
                                            let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                            let launch = ExecuteKernel::new(&kernel_filter)
                                                .set_arg(&buf_cur.i_total)
                                                .set_arg(&mut *a_ptr)
                                                .set_arg(&mut *n_ptr)
                                                .set_arg(&mut *g_ptr)
                                                .set_arg(&syn_decay_ampa)
                                                .set_arg(&syn_decay_nmda)
                                                .set_arg(&syn_decay_gaba)
                                                .set_arg(&bio.nmda_ratio)
                                                .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                                .set_global_work_size(num_current_hidden_neurons)
                                                .enqueue_nd_range(&cl.queue);
                                            if let Err(e) = launch {
                                                nm_log!("[warn] OpenCL sparse filter failed: {:?}", e);
                                                cl_ok = false;
                                            } else {
                                                gpu_filtered = true;
                                            }
                                        }
                                        if cl_ok {
                                            let mut i_vec = vec![0.0; num_current_hidden_neurons];
                                            if let Err(e) = cl.queue.enqueue_read_buffer(&buf_cur.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                                nm_log!("[warn] OpenCL sparse fwd read failed: {:?}", e);
                                                cl_ok = false;
                                            } else {
                                                i_f = Array1::from_vec(i_vec);
                                            }
                                        }
                                    }

                                    // Backward
                                    if cl_ok && l < num_hidden_layers - 1 {
                                        self.sync_cl_sparse_bwd(l);
                                        self.sync_cl_spk_hist_h(l + 1);
                                        let next_len = self.layer_size(l + 1);
                                        let hist_len_next = self.spk_hist_h[l + 1].len();

                                        if use_stp {
                                            self.sync_cl_stp_layer(l + 1);
                                        }
                                        let rel_ptr_bwd = if use_stp { self.cl_stp_rel_h.get_mut(l + 1).and_then(|b| b.as_mut()).map(|b| b as *mut Buffer<f64>) } else { None };
                                        if let (Some(hist_buf_next), Some(sparse_bwd)) = (self.cl_spk_hist_h[l + 1].as_mut(), self.cl_sparse_bwd[l].as_mut()) {
                                            let mut use_stp_kernel = false;
                                            let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                            if let Some(ptr) = rel_ptr_bwd {
                                                let rel = &mut *ptr;
                                                if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_h[l + 1], &[]) {
                                                    nm_log!("[warn] OpenCL sparse HH bwd rel write failed: {:?}", e);
                                                    cl_ok = false;
                                                } else {
                                                    rel_buf_opt = Some(rel);
                                                    use_stp_kernel = true;
                                                }
                                            }
                                            if cl_ok {
                                                let bwd_res = if use_stp_kernel {
                                                    if let (Some(rel_buf), Some(delays)) = (rel_buf_opt, sparse_bwd.delays.as_ref()) {
                                                        let kernel_acc = cl.kernel_syn_acc_sparse_delay_stp.lock().unwrap();
                                                        ExecuteKernel::new(&kernel_acc)
                                                            .set_arg(&buf_cur.i_total)
                                                            .set_arg(hist_buf_next)
                                                            .set_arg(rel_buf)
                                                            .set_arg(&sparse_bwd.row_ptr)
                                                            .set_arg(&sparse_bwd.col_indices)
                                                            .set_arg(delays)
                                                            .set_arg(&sparse_bwd.weights)
                                                            .set_arg(&(num_current_hidden_neurons as i32))
                                                            .set_arg(&(hist_len_next as i32))
                                                            .set_arg(&(next_len as i32))
                                                            .set_arg(&1i32) // Mode: accumulate
                                                            .set_global_work_size(num_current_hidden_neurons)
                                                            .enqueue_nd_range(&cl.queue)
                                                    } else {
                                                        Err(ClError(-1))
                                                    }
                                                } else {
                                                    if let Some(delays) = sparse_bwd.delays.as_ref() {
                                                        let kernel_acc = cl.kernel_syn_acc_sparse_delay.lock().unwrap();
                                                        ExecuteKernel::new(&kernel_acc)
                                                            .set_arg(&buf_cur.i_total)
                                                            .set_arg(hist_buf_next)
                                                            .set_arg(&sparse_bwd.row_ptr)
                                                            .set_arg(&sparse_bwd.col_indices)
                                                            .set_arg(delays)
                                                            .set_arg(&sparse_bwd.weights)
                                                            .set_arg(&(num_current_hidden_neurons as i32))
                                                            .set_arg(&(hist_len_next as i32))
                                                            .set_arg(&(next_len as i32))
                                                            .set_arg(&1i32) // Mode: accumulate
                                                            .set_global_work_size(num_current_hidden_neurons)
                                                            .enqueue_nd_range(&cl.queue)
                                                    } else {
                                                        Err(ClError(-1))
                                                    }
                                                };
                                                if let Err(e) = bwd_res {
                                                    nm_log!("[warn] OpenCL sparse bwd kernel failed: {:?}", e);
                                                    cl_ok = false;
                                                }
                                            }
                                            
                                            if cl_ok {
                                                if !gpu_filtered {
                                                    if let Some((a_ptr, n_ptr, g_ptr)) = syn_ptrs {
                                                        let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                                        let launch = ExecuteKernel::new(&kernel_filter)
                                                            .set_arg(&buf_cur.i_total)
                                                            .set_arg(&mut *a_ptr)
                                                            .set_arg(&mut *n_ptr)
                                                            .set_arg(&mut *g_ptr)
                                                            .set_arg(&syn_decay_ampa)
                                                            .set_arg(&syn_decay_nmda)
                                                            .set_arg(&syn_decay_gaba)
                                                            .set_arg(&bio.nmda_ratio)
                                                            .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                                            .set_global_work_size(num_current_hidden_neurons)
                                                            .enqueue_nd_range(&cl.queue);
                                                        if let Err(e) = launch {
                                                            nm_log!("[warn] OpenCL sparse filter failed: {:?}", e);
                                                            cl_ok = false;
                                                        } else {
                                                            gpu_filtered = true;
                                                        }
                                                    }
                                                }
                                                if cl_ok {
                                                    let mut i_vec_b = vec![0.0; num_current_hidden_neurons];
                                                    if let Err(e) = cl.queue.enqueue_read_buffer(&buf_cur.i_total, CL_TRUE, 0, &mut i_vec_b, &[]) {
                                                        nm_log!("[warn] OpenCL sparse bwd read failed: {:?}", e);
                                                        cl_ok = false;
                                                    } else {
                                                        // Since i_total now contains i_f + i_b, we need to extract i_b
                                                        for j in 0..num_current_hidden_neurons {
                                                            i_b[j] = i_vec_b[j] - i_f[j];
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                gpu_success = cl_ok;
                            }
                        }
                    }
                }

                if gpu_success {
                    if use_synaptic_filter && gpu_filtered {
                        self.sync_syn_state_from_gpu(l, false);
                    }
                    (i_f, i_b)
                } else if can_parallel_light(num_current_hidden_neurons) {
                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                    {
                        let released_cap = 256usize;
                        let results: Vec<(usize, f64, f64, f64, Vec<ReleasedEvent>)> = (0..num_current_hidden_neurons)
                            .into_par_iter()
                            .map(|j| {
                                let mut acc_f = 0.0;
                                let mut acc_b = 0.0;
                                let mut acc_r = 0.0;
                                let mut events = Vec::new();
                                // Forward
                                if use_aarnn && self.net.use_morphology {
                                    for &(i, syn_idx) in self.recv_fwd.get(l - 1).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                        let s = self.hist_h_at(l - 1, steps, i);
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                let w_val = self.w_hh_fwd[l - 1].get((j, i)).copied().unwrap_or(0.0);
                                                acc_f += w_val * stp_scale;
                                                if events.len() < released_cap {
                                                    events.push(ReleasedEvent {
                                                        kind: ReleasedKind::Fwd { layer: l - 1 },
                                                        pre_layer: l as isize - 1,
                                                        post_layer: l as isize,
                                                        pre_id: i,
                                                        post_id: j,
                                                        syn_idx: Some(syn_idx),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    if l == in_l {
                                        // AARNN Sensory input connects to the designated input layer
                                        for &(i, syn_idx) in self.recv_in.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                                            let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                            let s = self.hist_s_at(steps, i);
                                            if s != 0 {
                                                let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                                if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                    let w_val = self.w_in.get((j, i)).copied().unwrap_or(0.0);
                                                    acc_f += w_val * stp_scale;
                                                    if events.len() < released_cap {
                                                        events.push(ReleasedEvent { kind: ReleasedKind::In, pre_layer: -1, post_layer: 1, pre_id: i, post_id: j, syn_idx: Some(syn_idx) });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    for &i in &active_h_indices[l - 1] {
                                        let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_f += self.w_hh_fwd[l - 1][(j, i)] * stp_scale;
                                    }
                                    if use_aarnn && l == in_l {
                                        // Legacy distance-based AARNN sensory input to designated input layer
                                        for i in 0..num_sensory_neurons {
                                            let vel = self.net.aarnn_velocity.max(0.0);
                                            #[cfg(feature = "growth3d")]
                                            let dist = {
                                                let snode = &self.topo.sensory_nodes[i];
                                                if let Some(nodes_in) = self.topo.layers.get(in_l) {
                                                    if j < nodes_in.len() {
                                                        let dx = snode.x - nodes_in[j].x; let dy = snode.y - nodes_in[j].y; let dz = snode.z - nodes_in[j].z;
                                                        (dx*dx + dy*dy + dz*dz).sqrt()
                                                    } else { 1.0 }
                                                } else { 1.0 }
                                            };
                                            #[cfg(not(feature = "growth3d"))]
                                            let dist = 1.0f32;
                                            let dt_ms = self.lif.dt as f32;
                                            let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                            let s = {
                                                #[cfg(feature = "growth3d")]
                                                { let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1)); let frame = &self.spk_hist_s[idx]; if frame.len()==0 {0} else { let ii=i.min(frame.len()-1); frame[ii] } }
                                                #[cfg(not(feature = "growth3d"))]
                                                { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                            };
                                            if s != 0 {
                                                let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                                acc_f += self.w_in[(j, i)] * stp_scale;
                                            }
                                        }
                                    }
                                }
                                // Backward
                                if l < num_hidden_layers - 1 {
                                    if use_aarnn && self.net.use_morphology {
                                        for &(next_j, syn_idx) in self.recv_bwd.get(l).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                            let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                            let s = self.hist_h_at(l + 1, steps, next_j);
                                            if s != 0 {
                                                let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                                if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                    acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                                    if events.len() < released_cap {
                                                        events.push(ReleasedEvent {
                                                            kind: ReleasedKind::Bwd { layer: l },
                                                            pre_layer: l as isize + 1,
                                                            post_layer: l as isize,
                                                            pre_id: next_j,
                                                            post_id: j,
                                                            syn_idx: Some(syn_idx),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        for &next_j in &active_h_indices[l + 1] {
                                            let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                        }
                                    }
                                }
                                // Recurrent
                                if use_aarnn {
                                    if self.net.use_morphology {
                                        for &(pre_id, syn_idx) in self.recv_rec.get(l).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                            let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                            let s = self.hist_h_at(l, steps, pre_id);
                                            if s != 0 {
                                                let stp_scale = if use_stp { stp_release_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0.0) } else { 1.0 };
                                                if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                    let w_val = self.w_hh_rec.get(l).and_then(|m| m.get((j, pre_id))).copied().unwrap_or(0.0);
                                                    acc_r += w_val * stp_scale;
                                                    if events.len() < released_cap {
                                                        events.push(ReleasedEvent {
                                                            kind: ReleasedKind::HiddenRec { layer: l },
                                                            pre_layer: l as isize,
                                                            post_layer: l as isize,
                                                            pre_id,
                                                            post_id: j,
                                                            syn_idx: Some(syn_idx),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        for &pre_id in &active_h_indices[l] {
                                            let stp_scale = if use_stp { stp_release_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0.0) } else { 1.0 };
                                            let w_val = self.w_hh_rec.get(l).and_then(|m| m.get((j, pre_id))).copied().unwrap_or(0.0);
                                            acc_r += w_val * stp_scale;
                                        }
                                    }
                                }
                                (j, acc_f, acc_b, acc_r, events)
                            })
                            .collect();

                        let mut i_f = Array1::<f64>::zeros(num_current_hidden_neurons);
                        let mut i_b = Array1::<f64>::zeros(num_current_hidden_neurons);
                        let mut i_r = Array1::<f64>::zeros(num_current_hidden_neurons);
                        let mut total_ev = 0usize;
                        for (j, af, ab, ar, ev) in results {
                            i_f[j] = af;
                            i_b[j] = ab;
                            i_r[j] = ar;
                            if total_ev < released_cap {
                                let take = ev.len().min(released_cap.saturating_sub(total_ev));
                                self.released_events.extend(ev.into_iter().take(take));
                                total_ev += take;
                            }
                        }
                        (i_f + i_r, i_b)
                    }
                    #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                    {
                        let results: Vec<(usize, f64, f64, f64)> = (0..num_current_hidden_neurons)
                            .into_par_iter()
                            .map(|j| {
                                let mut acc_f = 0.0;
                                for i in 0..num_previous_hidden_neurons {
                                    if self.last_spk_h[l - 1][i] != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_f += self.w_hh_fwd[l - 1][(j, i)] * stp_scale;
                                    }
                                }
                                let mut acc_b = 0.0;
                                if l < num_hidden_layers - 1 {
                                    let num_next_hidden_neurons = self.layer_size(l + 1);
                                    for next_j in 0..num_next_hidden_neurons {
                                        if prev_spk_h[l + 1][next_j] != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                        }
                                    }
                                }
                                let mut acc_r = 0.0;
                                if use_aarnn {
                                    for pre_id in 0..num_current_hidden_neurons {
                                        if self.last_spk_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0) != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0.0) } else { 1.0 };
                                            let w_val = self.w_hh_rec.get(l).and_then(|m| m.get((j, pre_id))).copied().unwrap_or(0.0);
                                            acc_r += w_val * stp_scale;
                                        }
                                    }
                                }
                                (j, acc_f, acc_b, acc_r)
                            })
                            .collect();
                        let mut i_f = Array1::<f64>::zeros(num_current_hidden_neurons);
                        let mut i_b = Array1::<f64>::zeros(num_current_hidden_neurons);
                        let mut i_r = Array1::<f64>::zeros(num_current_hidden_neurons);
                        for (j, af, ab, ar) in results { i_f[j] = af; i_b[j] = ab; i_r[j] = ar; }
                        (i_f + i_r, i_b)
                    }
                } else {
                    let mut i_f = Array1::<f64>::zeros(num_current_hidden_neurons);
                    let mut i_b = Array1::<f64>::zeros(num_current_hidden_neurons);
                    let mut i_r = Array1::<f64>::zeros(num_current_hidden_neurons);
                    for j in 0..num_current_hidden_neurons {
                        let mut acc_f = 0.0;
                        if use_aarnn {
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            if self.net.use_morphology {
                                for &(i, syn_idx) in self.recv_fwd.get(l - 1).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                    let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                    let s = self.hist_h_at(l - 1, steps, i);
                                    if s != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                        if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                            acc_f += self.w_hh_fwd.get(l - 1).and_then(|m| m.get((j, i))).copied().unwrap_or(0.0) * stp_scale;
                                            if self.released_events.len() < 256 {
                                                self.released_events.push(ReleasedEvent { kind: ReleasedKind::Fwd { layer: l - 1 }, pre_layer: l as isize - 1, post_layer: l as isize, pre_id: i, post_id: j, syn_idx: Some(syn_idx) });
                                            }
                                        }
                                    }
                                }
                                if l == in_l {
                                    // AARNN Sensory input connects to Layer 1
                                    for &(i, syn_idx) in self.recv_in.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                        let s = self.hist_s_at(steps, i);
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                let w_val = self.w_in.get((j, i)).copied().unwrap_or(0.0);
                                                acc_f += w_val * stp_scale;
                                                if self.released_events.len() < 256 {
                                                    self.released_events.push(ReleasedEvent { kind: ReleasedKind::In, pre_layer: -1, post_layer: 1, pre_id: i, post_id: j, syn_idx: Some(syn_idx) });
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                for i in 0..num_previous_hidden_neurons {
                                    if self.last_spk_h[l - 1][i] != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_f += self.w_hh_fwd[l - 1][(j, i)] * stp_scale;
                                    }
                                }
                                if l == in_l {
                                    for i in 0..num_sensory_neurons {
                                        let vel = self.net.aarnn_velocity.max(0.0);
                                        #[cfg(feature = "growth3d")]
                                        let dist = {
                                            let snode = &self.topo.sensory_nodes[i];
                                            if let Some(nodes_in) = self.topo.layers.get(in_l) {
                                                if j < nodes_in.len() {
                                                    let dx = snode.x - nodes_in[j].x; let dy = snode.y - nodes_in[j].y; let dz = snode.z - nodes_in[j].z;
                                                    (dx*dx + dy*dy + dz*dz).sqrt()
                                                } else { 1.0 }
                                            } else { 1.0 }
                                        };
                                        #[cfg(not(feature = "growth3d"))]
                                        let dist = 1.0f32;
                                        let dt_ms = self.lif.dt as f32;
                                        let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).round() as usize } else { 0 };
                                        let s = {
                                            #[cfg(feature = "growth3d")]
                                            { let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1)); let frame = &self.spk_hist_s[idx]; if frame.len()==0 {0} else { let ii=i.min(frame.len()-1); frame[ii] } }
                                            #[cfg(not(feature = "growth3d"))]
                                            { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                        };
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc_f += self.w_in[(j, i)] * stp_scale;
                                        }
                                    }
                                }
                            }
                            #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                            {
                                for i in 0..num_previous_hidden_neurons {
                                    if self.last_spk_h[l - 1][i] != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_f += self.w_hh_fwd[l - 1][(j, i)] * stp_scale;
                                    }
                                }
                                if l == in_l {
                                    for i in 0..num_sensory_neurons {
                                        let vel = self.net.aarnn_velocity.max(0.0);
                                        #[cfg(feature = "growth3d")]
                                        let dist = {
                                            let snode = &self.topo.sensory_nodes[i];
                                            if let Some(nodes_in) = self.topo.layers.get(in_l) {
                                                if j < nodes_in.len() {
                                                    let dx = snode.x - nodes_in[j].x; let dy = snode.y - nodes_in[j].y; let dz = snode.z - nodes_in[j].z;
                                                    (dx*dx + dy*dy + dz*dz).sqrt()
                                                } else { 1.0 }
                                            } else { 1.0 }
                                        };
                                        #[cfg(not(feature = "growth3d"))]
                                        let dist = 1.0f32;
                                        let dt_ms = self.lif.dt as f32;
                                        let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                        let s = {
                                            #[cfg(feature = "growth3d")]
                                            { let idx = steps_delay.min(self.spk_hist_s.len().saturating_sub(1)); let frame = &self.spk_hist_s[idx]; if frame.len()==0 {0} else { let ii=i.min(frame.len()-1); frame[ii] } }
                                            #[cfg(not(feature = "growth3d"))]
                                            { if steps_delay >= 1 { 0 } else { s_t[i] } }
                                        };
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_s.get(i).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc_f += self.w_in[(j, i)] * stp_scale;
                                        }
                                    }
                                }
                            }
                        } else {
                            for i in 0..num_previous_hidden_neurons {
                                if self.last_spk_h[l - 1][i] != 0 {
                                    let stp_scale = if use_stp { stp_release_h.get(l - 1).and_then(|v| v.get(i)).copied().unwrap_or(0.0) } else { 1.0 };
                                    acc_f += self.w_hh_fwd[l - 1][(j, i)] * stp_scale;
                                }
                            }
                        }
                        i_f[j] = acc_f;

                        let mut acc_b = 0.0;
                        if l < num_hidden_layers - 1 {
                            if use_aarnn {
                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                if self.net.use_morphology {
                                    for &(next_j, syn_idx) in self.recv_bwd.get(l).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                        let s = self.hist_h_at(l + 1, steps, next_j);
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                                if self.released_events.len() < 256 {
                                                    self.released_events.push(ReleasedEvent { kind: ReleasedKind::Bwd { layer: l }, pre_layer: l as isize + 1, post_layer: l as isize, pre_id: next_j, post_id: j, syn_idx: Some(syn_idx) });
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    let num_next_hidden_neurons = self.layer_size(l + 1);
                                    for next_j in 0..num_next_hidden_neurons {
                                        if prev_spk_h[l + 1][next_j] != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                        }
                                    }
                                }
                                #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                                {
                                    let num_next_hidden_neurons = self.layer_size(l + 1);
                                    for next_j in 0..num_next_hidden_neurons {
                                        if prev_spk_h[l + 1][next_j] != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                        }
                                    }
                                }
                            } else {
                                let num_next_hidden_neurons = self.layer_size(l + 1);
                                for next_j in 0..num_next_hidden_neurons {
                                    if prev_spk_h[l + 1][next_j] != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l + 1).and_then(|v| v.get(next_j)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_b += self.w_hh_bwd[l][(j, next_j)] * stp_scale;
                                    }
                                }
                            }
                        }
                        i_b[j] = acc_b;

                        let mut acc_r = 0.0;
                        if use_aarnn {
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            if self.net.use_morphology {
                                for &(pre_id, syn_idx) in self.recv_rec.get(l).and_then(|v| v.get(j)).map(|v| v.as_slice()).unwrap_or(&[]) {
                                    let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                    let s = self.hist_h_at(l, steps, pre_id);
                                    if s != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0.0) } else { 1.0 };
                                        if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                            acc_r += self.w_hh_rec[l][(j, pre_id)] * stp_scale;
                                            if self.released_events.len() < 256 {
                                                self.released_events.push(ReleasedEvent { kind: ReleasedKind::HiddenRec { layer: l }, pre_layer: l as isize, post_layer: l as isize, pre_id, post_id: j, syn_idx: Some(syn_idx) });
                                            }
                                        }
                                    }
                                }
                            } else {
                                for pre_id in 0..num_current_hidden_neurons {
                                    if self.last_spk_h[l][pre_id] != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_r += self.w_hh_rec[l][(j, pre_id)] * stp_scale;
                                    }
                                }
                            }
                            #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                            {
                                for pre_id in 0..num_current_hidden_neurons {
                                    if self.last_spk_h[l][pre_id] != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(l).and_then(|v| v.get(pre_id)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc_r += self.w_hh_rec[l][(j, pre_id)] * stp_scale;
                                    }
                                }
                            }
                        }
                        i_r[j] = acc_r;
                    }
                    (i_f + i_r, i_b)
                }
            };
            let (mut i_f, mut i_b) = (i_f, i_b);
            if use_synaptic_filter && num_current_hidden_neurons > 0 && !gpu_filtered {
                let mut combined = i_f.clone();
                for j in 0..num_current_hidden_neurons {
                    combined[j] += i_b[j];
                }
                let filtered = Self::apply_synaptic_filter(
                    self.lif.dt,
                    &self.net.aarnn_bio,
                    &combined,
                    &mut self.syn_ampa_h[l],
                    &mut self.syn_nmda_h[l],
                    &mut self.syn_gaba_h[l],
                    Some(&self.v_h[l]),
                    self.net.aarnn_nmda_voltage_sensitivity.max(0.0) as f64,
                    #[cfg(feature = "growth3d")] Some(&self.bio_h[l]),
                    #[cfg(not(feature = "growth3d"))] None,
                    &default_decays,
                );
                i_f = filtered;
                i_b.fill(0.0);
            }
            if is_aarnn && num_current_hidden_neurons > 1 {
                let g_gap = self.net.aarnn_gap_junction_strength.max(0.0) as f64;
                Self::apply_gap_junction_coupling(&mut i_f, &self.v_h[l], g_gap);
            }

            #[cfg(any(feature = "ui", feature = "growth3d"))]
            {
                if self.last_i_f.len() < num_hidden_layers { self.last_i_f.resize(num_hidden_layers.max(1), Array1::<f64>::zeros(0)); }
                self.last_i_f[l] = i_f.clone();
            }

            let spk = {
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                let mut gpu_success = false;
                #[cfg(feature = "opencl")]
                {
                    let cl_mgr = self.cl.clone();
                    if let Some(ref cl) = cl_mgr {
                        if !use_aarnn {
                            self.sync_cl_buffers(l, false);
                            let izh_params = self.effective_izh_params();
                            if let Some(ref mut buf) = self.cl_buffers_h.get_mut(l).and_then(|o| o.as_mut()) {
                                // Upload total current (i_f + i_b)
                                let i_total: Vec<f64> = (0..num_current_hidden_neurons).map(|j| i_f[j] + i_b[j]).collect();
                                gpu_success = true;
                                unsafe {
                                    if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.i_total, CL_TRUE, 0, &i_total, &[]) {
                                        nm_log!("[warn] OpenCL Hl write i_total failed: {:?}", e);
                                        gpu_success = false;
                                    }
                                }
                                
                                if gpu_success {
                                    let kernel_lif = cl.kernel_lif_step.lock().unwrap();
                                    let kernel_izh = cl.kernel_izh_step.lock().unwrap();
                                    match self.neuron_model {
                                        NeuronModel::Lif => {
                                            unsafe {
                                                let launch = ExecuteKernel::new(&kernel_lif)
                                                    .set_arg(&buf.v)
                                                    .set_arg(buf.refr.as_ref().unwrap())
                                                    .set_arg(&buf.i_total)
                                                    .set_arg(&self.decay_m)
                                                    .set_arg(&self.lif.v_th)
                                                    .set_arg(&self.lif.v_reset)
                                                    .set_arg(&(self.lif.refractory as i32))
                                                    .set_arg(&buf.spk)
                                                    .set_global_work_size(num_current_hidden_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL Hl lif_step failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            }
                                        }
                                        NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                                            let p = izh_params.expect("izh params for Izh/AARNN");
                                            unsafe {
                                                let launch = ExecuteKernel::new(&kernel_izh)
                                                    .set_arg(&buf.v)
                                                    .set_arg(buf.u.as_ref().unwrap())
                                                    .set_arg(&buf.i_total)
                                                    .set_arg(&p.dt)
                                                    .set_arg(&p.recovery_time_constant_a)
                                                    .set_arg(&p.recovery_sensitivity_b)
                                                    .set_arg(&p.membrane_reset_potential_c)
                                                    .set_arg(&p.recovery_increment_d)
                                                    .set_arg(&p.v_th)
                                                    .set_arg(&buf.spk)
                                                    .set_global_work_size(num_current_hidden_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL Hl izh_step failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if gpu_success {
                    #[cfg(feature = "opencl")]
                    { self.sync_cl_state_from_gpu(l, false) }
                    #[cfg(not(feature = "opencl"))]
                    { unreachable!() }
                } else {
                    let mut spk = Array1::<i8>::zeros(num_current_hidden_neurons);
                    match self.neuron_model {
                        NeuronModel::Lif => {
                            let (old_v, old_refr): (Vec<f64>, Vec<i32>) = {
                                let refh = self.refr_h.as_ref().unwrap();
                                ((0..num_current_hidden_neurons).map(|j| self.v_h[l][j]).collect(), (0..num_current_hidden_neurons).map(|j| refh[l][j]).collect())
                            };
                            if can_parallel_light(num_current_hidden_neurons) {
                                let res: Vec<(f64, i32, i8)> = (0..num_current_hidden_neurons).into_par_iter().map(|j| {
                                    let v = old_v[j] * self.decay_m + i_f[j] + i_b[j];
                                    let v_clamped = v.clamp(-5.0, 5.0);
                                    let active = old_refr[j] <= 0;
                                    let fired = active && v_clamped >= self.lif.v_th;
                                    if fired { (self.lif.v_reset, self.lif.refractory as i32, 1) } else { (v_clamped, (old_refr[j] - 1).max(0), 0) }
                                }).collect();
                                let refh = self.refr_h.as_mut().unwrap();
                                for j in 0..num_current_hidden_neurons { self.v_h[l][j] = res[j].0; refh[l][j] = res[j].1; spk[j] = res[j].2; }
                            } else {
                                let refh = self.refr_h.as_mut().unwrap();
                                for j in 0..num_current_hidden_neurons {
                                    let v = self.v_h[l][j] * self.decay_m + i_f[j] + i_b[j];
                                    self.v_h[l][j] = v.clamp(-5.0, 5.0);
                                    let active = refh[l][j] <= 0;
                                    let fired = active && self.v_h[l][j] >= self.lif.v_th;
                                    if fired { self.v_h[l][j] = self.lif.v_reset; refh[l][j] = self.lif.refractory as i32; } else { refh[l][j] = (refh[l][j]-1).max(0); }
                                    spk[j] = fired as i8;
                                }
                            }
                        }
                        NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                            #[allow(unused_variables)]
                            let p_default = self.effective_izh_params().expect("izh params for Izh/AARNN");
                            let (old_v, old_u): (Vec<f64>, Vec<f64>) = {
                                let uh = self.u_h.as_ref().unwrap();
                                ((0..num_current_hidden_neurons).map(|j| self.v_h[l][j]).collect(), (0..num_current_hidden_neurons).map(|j| uh[l][j]).collect())
                            };
                            let old_refr: Vec<i32> = if use_izh_refractory {
                                let r = self.izh_refr_h.as_ref().unwrap();
                                (0..num_current_hidden_neurons).map(|j| r[l][j]).collect()
                            } else {
                                Vec::new()
                            };
                            if can_parallel_light(num_current_hidden_neurons) {
                                let res: Vec<(f64, f64, i8)> = (0..num_current_hidden_neurons).into_par_iter().map(|j| {
                                    let (bio, p) = {
                                        #[cfg(feature = "growth3d")]
                                        { 
                                            let b = &self.bio_h[l][j];
                                            let d = Self::get_decays_static(self.lif.dt, b);
                                            (b, d.izh_params)
                                        }
                                        #[cfg(not(feature = "growth3d"))]
                                        { (&self.net.aarnn_bio, p_default) }
                                    };
                                    let v = old_v[j]; let u = old_u[j];
                                    let nv = v + p.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + i_f[j] + i_b[j]);
                                    let nu = u + p.dt * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * nv - u));
                                    let mut fired = nv >= p.v_th;
                                    if use_adaptive_threshold {
                                        let thr_offset = self.thr_offset_h[l][j].clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                        fired = nv >= (p.v_th + thr_offset);
                                    }
                                    if use_izh_refractory && old_refr[j] > 0 { fired = false; }
                                    let (nv2, nu2) = if fired { (p.membrane_reset_potential_c, nu + p.recovery_increment_d) } else { (nv, nu) };
                                    (nv2, nu2, fired as i8)
                                }).collect();
                                let uh = self.u_h.as_mut().unwrap();
                                for j in 0..num_current_hidden_neurons { self.v_h[l][j] = res[j].0; uh[l][j] = res[j].1; spk[j] = res[j].2; }
                                if use_adaptive_threshold {
                                    for j in 0..num_current_hidden_neurons {
                                        if spk[j] != 0 {
                                            let bio = {
                                                #[cfg(feature = "growth3d")]
                                                { &self.bio_h[l][j] }
                                                #[cfg(not(feature = "growth3d"))]
                                                { &self.net.aarnn_bio }
                                            };
                                            self.thr_offset_h[l][j] = (self.thr_offset_h[l][j] + bio.adaptive_threshold_increment)
                                                .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                        }
                                    }
                                }
                                if use_izh_refractory {
                                    if let Some(r) = self.izh_refr_h.as_mut() {
                                        for j in 0..num_current_hidden_neurons {
                                            if spk[j] != 0 {
                                                let steps = {
                                                    #[cfg(feature = "growth3d")]
                                                    { Self::get_decays_static(self.lif.dt, &self.bio_h[l][j]).izh_refractory_steps }
                                                    #[cfg(not(feature = "growth3d"))]
                                                    { izh_refractory_steps }
                                                };
                                                r[l][j] = steps; 
                                            }
                                            else { r[l][j] = (r[l][j] - 1).max(0); }
                                        }
                                    }
                                }
                            } else {
                                let uh = self.u_h.as_mut().unwrap();
                                for j in 0..num_current_hidden_neurons {
                                    let (bio, p) = {
                                        #[cfg(feature = "growth3d")]
                                        { 
                                            let b = &self.bio_h[l][j];
                                            let d = Self::get_decays_static(self.lif.dt, b);
                                            (b, d.izh_params)
                                        }
                                        #[cfg(not(feature = "growth3d"))]
                                        { (&self.net.aarnn_bio, p_default) }
                                    };
                                    let v = self.v_h[l][j]; let u = uh[l][j];
                                    let nv = v + p.dt * (0.04*v*v + 5.0*v + 140.0 - u + i_f[j] + i_b[j]);
                                    let nu = u + p.dt * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * nv - u));
                                    let mut fired = nv >= p.v_th;
                                    if use_adaptive_threshold {
                                        let thr_offset = self.thr_offset_h[l][j].clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                        fired = nv >= (p.v_th + thr_offset);
                                    }
                                    if use_izh_refractory {
                                        if let Some(r) = self.izh_refr_h.as_ref() {
                                            if r[l][j] > 0 { fired = false; }
                                        }
                                    }
                                    let (nv2, nu2) = if fired { (p.membrane_reset_potential_c, nu + p.recovery_increment_d) } else { (nv, nu) };
                                    self.v_h[l][j] = nv2; uh[l][j] = nu2; spk[j] = fired as i8;
                                    if use_adaptive_threshold && fired {
                                        self.thr_offset_h[l][j] = (self.thr_offset_h[l][j] + bio.adaptive_threshold_increment)
                                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    }
                                    if use_izh_refractory {
                                        if let Some(r) = self.izh_refr_h.as_mut() {
                                            if fired {
                                                let steps = {
                                                    #[cfg(feature = "growth3d")]
                                                    { Self::get_decays_static(self.lif.dt, &self.bio_h[l][j]).izh_refractory_steps }
                                                    #[cfg(not(feature = "growth3d"))]
                                                    { izh_refractory_steps }
                                                };
                                                r[l][j] = steps;
                                            }
                                            else { r[l][j] = (r[l][j] - 1).max(0); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    spk
                }
            };
            self.last_spk_h[l] = spk.clone();
            {
                if let Some(dq) = self.spk_hist_h.get_mut(l) {
                    dq.push_front(spk.clone());
                    while dq.len() > self.hist_len { dq.pop_back(); }
                }
            }
            for j in 0..num_current_hidden_neurons { if spk[j] != 0 { self.x_post_h[l][j] += 1.0; self.x_pre_h[l][j] += 1.0; } }
            if use_homeostasis {
                for j in 0..num_current_hidden_neurons {
                    if spk[j] != 0 { self.rate_ema_h[l][j] += 1.0 - homeo_decay; }
                    let err = self.rate_ema_h[l][j] - base_homeo_target;
                    self.thr_offset_h[l][j] += bio.homeostasis_gain * err;
                }
            }
            #[cfg(feature = "growth3d")]
            if self.net.growth_enabled {
                if can_parallel_light(num_current_hidden_neurons) {
                    let old_rates: Vec<f32> = (0..num_current_hidden_neurons).map(|j| self.rate_h[l][j]).collect();
                    let old_since: Vec<f32> = (0..num_current_hidden_neurons).map(|j| self.since_growth_ms[l][j]).collect();
                    let res: Vec<(f32, f32)> = (0..num_current_hidden_neurons).into_par_iter().map(|j| {
                        let r = old_rates[j] * decay_rate + if spk[j] != 0 { 1.0 } else { 0.0 };
                        (r, old_since[j] + dt_ms)
                    }).collect();
                    for j in 0..num_current_hidden_neurons { 
                        self.rate_h[l][j] = res[j].0; 
                        self.since_growth_ms[l][j] = res[j].1; 
                        if spk[j] != 0 {
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            if self.net.use_morphology && l < self.morph.somas.len() && j < self.morph.somas[l].len() {
                                self.morph.somas[l][j].stimuli += 1.0;
                            }
                        }
                    }
                } else {
                    for j in 0..num_current_hidden_neurons {
                        let r = self.rate_h[l][j] * decay_rate + if spk[j] != 0 { 1.0 } else { 0.0 };
                        self.rate_h[l][j] = r;
                        self.since_growth_ms[l][j] += dt_ms;
                        
                        if spk[j] != 0 {
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            if self.net.use_morphology && l < self.morph.somas.len() && j < self.morph.somas[l].len() {
                                self.morph.somas[l][j].stimuli += 1.0;
                            }
                        }
                    }
                }
            }
        }
        }

        // Output layer
        #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
        let mut gpu_filtered_o = false;
        let mut i_o = Array1::<f64>::zeros(num_output_neurons);
        let out_conn_layer = out_l;
        if out_conn_layer < num_hidden_layers && self.is_layer_assigned(out_conn_layer) {
            observe_time!("Runner::step/output_layer");
            let num_last_layer_neurons = self.layer_size(out_conn_layer);
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_success = false;
            #[cfg(feature = "opencl")]
            let cl_mgr = self.cl.clone();
            #[cfg(feature = "opencl")]
            if let Some(ref cl) = cl_mgr {
                let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                if !use_aarnn {
                    if num_hidden_layers > 0 {
                        self.sync_cl_w_out_to_gpu();
                        self.sync_cl_buffers(out_conn_layer, false);
                        self.sync_cl_buffers(0, true);
                        if use_synaptic_filter {
                            self.sync_cl_syn_buffers(0, true);
                        }
                        if use_stp {
                            self.sync_cl_stp_layer(out_conn_layer);
                        }
                        
                        let cl_out_opt = self.cl_w_out.as_ref();
                        let buf_last_ptr = if let Some(Some(ref b)) = self.cl_buffers_h.get(out_conn_layer) { Some(b as *const CLBuffers) } else { None };
                        let buf_o_ptr = if let Some(ref mut b) = self.cl_buffer_o { Some(b as *mut CLBuffers) } else { None };
                        let rel_ptr = if use_stp { self.cl_stp_rel_h.get_mut(out_conn_layer).and_then(|b| b.as_mut()).map(|b| b as *mut Buffer<f64>) } else { None };
                        let syn_ptrs = if use_synaptic_filter {
                            match (
                                self.cl_syn_ampa_o.as_mut(),
                                self.cl_syn_nmda_o.as_mut(),
                                self.cl_syn_gaba_o.as_mut(),
                            ) {
                                (Some(a), Some(n), Some(g)) => Some((a as *mut Buffer<f64>, n as *mut Buffer<f64>, g as *mut Buffer<f64>)),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        if let (Some(cl_out), Some(buf_last_p), Some(buf_o_p)) = (cl_out_opt, buf_last_ptr, buf_o_ptr) {
                            let buf_last = unsafe { &*buf_last_p };
                            let buf_o = unsafe { &mut *buf_o_p };

                            gpu_success = true;
                            unsafe {
                                let mut use_stp_kernel = false;
                                let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                if let Some(ptr) = rel_ptr {
                                    let rel = &mut *ptr;
                                    if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_h[out_conn_layer], &[]) {
                                        nm_log!("[warn] OpenCL dense output rel write failed: {:?}", e);
                                        gpu_success = false;
                                    } else {
                                        rel_buf_opt = Some(rel);
                                        use_stp_kernel = true;
                                    }
                                }
                                if gpu_success {
                                    if use_stp_kernel {
                                        if let Some(rel_buf) = rel_buf_opt {
                                            let kernel_acc = cl.kernel_syn_acc_stp.lock().unwrap();
                                            let launch = ExecuteKernel::new(&kernel_acc)
                                                .set_arg(&buf_o.i_total)
                                                .set_arg(rel_buf)
                                                .set_arg(cl_out)
                                                .set_arg(&(num_last_layer_neurons as i32))
                                                .set_arg(&(num_output_neurons as i32))
                                                .set_global_work_size(num_output_neurons)
                                                .enqueue_nd_range(&cl.queue);
                                            if let Err(e) = launch {
                                                nm_log!("[warn] OpenCL dense output acc stp failed: {:?}", e);
                                                gpu_success = false;
                                            }
                                        } else {
                                            gpu_success = false;
                                        }
                                    } else {
                                        let kernel_acc = cl.kernel_syn_acc.lock().unwrap();
                                        let launch = ExecuteKernel::new(&kernel_acc)
                                            .set_arg(&buf_o.i_total)
                                            .set_arg(&buf_last.spk)
                                            .set_arg(cl_out)
                                            .set_arg(&(num_last_layer_neurons as i32))
                                            .set_arg(&(num_output_neurons as i32))
                                            .set_global_work_size(num_output_neurons)
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL dense output acc failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    }
                                }
                                    
                                if gpu_success && use_synaptic_filter {
                                    if let Some((a_ptr, n_ptr, g_ptr)) = syn_ptrs {
                                        let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                        let launch = ExecuteKernel::new(&kernel_filter)
                                            .set_arg(&buf_o.i_total)
                                            .set_arg(&mut *a_ptr)
                                            .set_arg(&mut *n_ptr)
                                            .set_arg(&mut *g_ptr)
                                            .set_arg(&syn_decay_ampa)
                                            .set_arg(&syn_decay_nmda)
                                            .set_arg(&syn_decay_gaba)
                                            .set_arg(&bio.nmda_ratio)
                                            .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                            .set_global_work_size(num_output_neurons)
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL dense output filter failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            gpu_filtered_o = true;
                                        }
                                    }
                                }
                                if gpu_success {
                                    let mut i_vec = vec![0.0; num_output_neurons];
                                    if let Err(e) = cl.queue.enqueue_read_buffer(&mut buf_o.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                        nm_log!("[warn] OpenCL dense output i_total read failed: {:?}", e);
                                        gpu_success = false;
                                    } else {
                                        i_o = Array1::from_vec(i_vec);
                                        if use_synaptic_filter {
                                            self.sync_syn_state_from_gpu(0, true);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if self.net.use_morphology {
                    // Sparse path acceleration
                    #[cfg(all(feature = "opencl", feature = "morpho", feature = "growth3d"))]
                    if num_hidden_layers > 0 {
                        self.sync_cl_sparse_out();
                        self.sync_cl_spk_hist_h(out_conn_layer);
                        self.sync_cl_buffers(0, true);
                        if use_synaptic_filter {
                            self.sync_cl_syn_buffers(0, true);
                        }
                        if use_stp {
                            self.sync_cl_stp_layer(out_conn_layer);
                        }

                        let last_h_len = num_last_layer_neurons;
                        let hist_len = self.spk_hist_h[out_conn_layer].len();
                        
                        let rel_ptr = if use_stp { self.cl_stp_rel_h.get_mut(out_conn_layer).and_then(|b| b.as_mut()).map(|b| b as *mut Buffer<f64>) } else { None };
                        let syn_ptrs = if use_synaptic_filter {
                            match (
                                self.cl_syn_ampa_o.as_mut(),
                                self.cl_syn_nmda_o.as_mut(),
                                self.cl_syn_gaba_o.as_mut(),
                            ) {
                                (Some(a), Some(n), Some(g)) => Some((a as *mut Buffer<f64>, n as *mut Buffer<f64>, g as *mut Buffer<f64>)),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        if let (Some(hist_ptr), Some(sparse_ptr), Some(o_buf_ptr)) = (self.cl_spk_hist_h.get_mut(out_conn_layer).and_then(|b| b.as_mut()), self.cl_sparse_out.as_mut(), self.cl_buffer_o.as_mut()) {
                            gpu_success = true;
                            unsafe {
                                let hist_buf = &mut *hist_ptr;
                                let sparse_out = &mut *sparse_ptr;
                                let o_buf = &mut *o_buf_ptr;
                                let mut use_stp_kernel = false;
                                let mut rel_buf_opt: Option<&mut Buffer<f64>> = None;
                                if let Some(ptr) = rel_ptr {
                                    let rel = &mut *ptr;
                                    if let Err(e) = cl.queue.enqueue_write_buffer(rel, CL_TRUE, 0, &stp_release_h[out_conn_layer], &[]) {
                                        nm_log!("[warn] OpenCL sparse output rel write failed: {:?}", e);
                                        gpu_success = false;
                                    } else {
                                        rel_buf_opt = Some(rel);
                                        use_stp_kernel = true;
                                    }
                                }
                                    if gpu_success {
                                        if use_stp_kernel {
                                            if let (Some(rel_buf), Some(delays)) = (rel_buf_opt, sparse_out.delays.as_ref()) {
                                                let kernel_acc = cl.kernel_syn_acc_sparse_delay_stp.lock().unwrap();
                                                let launch = ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&o_buf.i_total)
                                                    .set_arg(hist_buf)
                                                    .set_arg(rel_buf)
                                                    .set_arg(&sparse_out.row_ptr)
                                                    .set_arg(&sparse_out.col_indices)
                                                    .set_arg(delays)
                                                    .set_arg(&sparse_out.weights)
                                                    .set_arg(&(num_output_neurons as i32))
                                                    .set_arg(&(hist_len as i32))
                                                    .set_arg(&(last_h_len as i32))
                                                    .set_arg(&0i32) // Mode: set
                                                    .set_global_work_size(num_output_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL sparse output acc stp failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            } else {
                                                gpu_success = false;
                                            }
                                        } else {
                                            if let Some(delays) = sparse_out.delays.as_ref() {
                                                let kernel_acc = cl.kernel_syn_acc_sparse_delay.lock().unwrap();
                                                let launch = ExecuteKernel::new(&kernel_acc)
                                                    .set_arg(&o_buf.i_total)
                                                    .set_arg(hist_buf)
                                                    .set_arg(&sparse_out.row_ptr)
                                                    .set_arg(&sparse_out.col_indices)
                                                    .set_arg(delays)
                                                    .set_arg(&sparse_out.weights)
                                                    .set_arg(&(num_output_neurons as i32))
                                                    .set_arg(&(hist_len as i32))
                                                    .set_arg(&(last_h_len as i32))
                                                    .set_arg(&0i32) // Mode: set
                                                    .set_global_work_size(num_output_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL sparse output acc failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            } else {
                                                gpu_success = false;
                                            }
                                        }
                                    }

                                if gpu_success && use_synaptic_filter {
                                    if let Some((a_ptr, n_ptr, g_ptr)) = syn_ptrs {
                                        let kernel_filter = cl.kernel_syn_filter.lock().unwrap();
                                        let launch = ExecuteKernel::new(&kernel_filter)
                                            .set_arg(&o_buf.i_total)
                                            .set_arg(&mut *a_ptr)
                                            .set_arg(&mut *n_ptr)
                                            .set_arg(&mut *g_ptr)
                                            .set_arg(&syn_decay_ampa)
                                            .set_arg(&syn_decay_nmda)
                                            .set_arg(&syn_decay_gaba)
                                            .set_arg(&bio.nmda_ratio)
                                            .set_arg(&(bio.synaptic_gain * neuromod_excitability_gain))
                                            .set_global_work_size(num_output_neurons)
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL sparse output filter failed: {:?}", e);
                                            gpu_success = false;
                                        } else {
                                            gpu_filtered_o = true;
                                        }
                                    }
                                }
                                if gpu_success {
                                    let mut i_vec = vec![0.0; num_output_neurons];
                                    if let Err(e) = cl.queue.enqueue_read_buffer(&o_buf.i_total, CL_TRUE, 0, &mut i_vec, &[]) {
                                        nm_log!("[warn] OpenCL sparse output i_total read failed: {:?}", e);
                                        gpu_success = false;
                                    } else {
                                        i_o = Array1::from_vec(i_vec);
                                        if use_synaptic_filter {
                                            self.sync_syn_state_from_gpu(0, true);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !gpu_success {
                if can_parallel_light(num_output_neurons) {
                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                    {
                        let released_cap = 256usize;
                        let results: Vec<(usize, f64, Vec<ReleasedEvent>)> = (0..num_output_neurons)
                            .into_par_iter()
                            .map(|k| {
                                let mut acc = 0.0f64;
                                let mut events = Vec::new();
                                let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                                if use_aarnn && self.net.use_morphology {
                                    for &(j, syn_idx) in self.recv_out.get(k).map(|v| v.as_slice()).unwrap_or(&[]) {
                                        let (steps, atten) = self.syn_delay_and_atten(syn_idx);
                                        let s = self.hist_h_at(out_conn_layer, steps, j);
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                                acc += self.w_out[(k, j)] * atten * stp_scale;
                                                if events.len() < released_cap {
                                                    events.push(ReleasedEvent {
                                                        kind: ReleasedKind::Out,
                                                        pre_layer: out_conn_layer as isize,
                                                        post_layer: out_conn_layer as isize + 1,
                                                        pre_id: j,
                                                        post_id: k,
                                                        syn_idx: Some(syn_idx),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                } else if use_aarnn {
                                    // Legacy distance-based AARNN path
                                    let vel = self.net.aarnn_velocity.max(0.0);
                                    for j in 0..num_last_layer_neurons {
                                        #[cfg(feature = "growth3d")]
                                        let dist = if let Some(nodes_last) = self.topo.layers.get(out_conn_layer) {
                                            if j < nodes_last.len() {
                                                let onode = &self.topo.output_nodes[k];
                                                let dx = nodes_last[j].x - onode.x;
                                                let dy = nodes_last[j].y - onode.y;
                                                let dz = nodes_last[j].z - onode.z;
                                                (dx * dx + dy * dy + dz * dz).sqrt()
                                            } else { 1.0 }
                                        } else { 1.0 };
                                        #[cfg(not(feature = "growth3d"))]
                                        let dist = 1.0f32;
                                        let dt_ms = self.lif.dt as f32;
                                        let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                        let s = {
                                            #[cfg(feature = "growth3d")]
                                            {
                                                if let Some(dq) = self.spk_hist_h.get(out_conn_layer) {
                                                    let idx = steps_delay.min(dq.len().saturating_sub(1));
                                                    let frame = &dq[idx];
                                                    if frame.len() == 0 { 0 } else { let jj = j.min(frame.len() - 1); frame[jj] }
                                                } else { 0 }
                                            }
                                            #[cfg(not(feature = "growth3d"))]
                                            { if steps_delay >= 1 { 0 } else { self.last_spk_h[out_conn_layer][j] } }
                                        };
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc += self.w_out[(k, j)] * stp_scale;
                                        }
                                    }
                                } else {
                                    for &j in &active_h_indices[out_conn_layer] {
                                        let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc += self.w_out[(k, j)] * stp_scale;
                                    }
                                }
                                (k, acc, events)
                            })
                            .collect();

                        let mut total_ev = 0usize;
                        for (k, acc, ev) in results {
                            i_o[k] = acc;
                            if total_ev < released_cap {
                                let take = ev.len().min(released_cap - total_ev);
                                self.released_events.extend(ev.into_iter().take(take));
                                total_ev += take;
                            }
                        }
                    }
                    #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                    {
                        let results: Vec<(usize, f64)> = (0..num_output_neurons)
                            .into_par_iter()
                            .map(|k| {
                                let mut acc = 0.0;
                                let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                                if use_aarnn {
                                    let vel = self.net.aarnn_velocity.max(0.0);
                                    for j in 0..num_last_layer_neurons {
                                        #[cfg(feature = "growth3d")]
                                        let dist = if let Some(nodes_last) = self.topo.layers.get(out_conn_layer) {
                                            if j < nodes_last.len() {
                                                let onode = &self.topo.output_nodes[k];
                                                let dx = nodes_last[j].x - onode.x;
                                                let dy = nodes_last[j].y - onode.y;
                                                let dz = nodes_last[j].z - onode.z;
                                                (dx * dx + dy * dy + dz * dz).sqrt()
                                            } else { 1.0 }
                                        } else { 1.0 };
                                        #[cfg(not(feature = "growth3d"))]
                                        let dist = 1.0f32;
                                        let dt_ms = self.lif.dt as f32;
                                        let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                        let s = {
                                            #[cfg(feature = "growth3d")]
                                            { if let Some(dq) = self.spk_hist_h.get(out_conn_layer) { let idx = steps_delay.min(dq.len().saturating_sub(1)); let frame = &dq[idx]; if frame.len()==0 {0} else { let jj=j.min(frame.len()-1); frame[jj] } } else { 0 } }
                                            #[cfg(not(feature = "growth3d"))]
                                            { if steps_delay >= 1 { 0 } else { self.last_spk_h[out_conn_layer][j] } }
                                        };
                                        if s != 0 {
                                            let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc += self.w_out[(k, j)] * stp_scale;
                                        }
                                    }
                                } else {
                                    for &j in &active_h_indices[out_conn_layer] {
                                        let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc += self.w_out[(k, j)] * stp_scale;
                                    }
                                }
                                (k, acc)
                            })
                            .collect();
                        for (k, acc) in results { i_o[k] = acc; }
                    }
                } else {
                    for k in 0..num_output_neurons {
                        let mut acc = 0.0;
                        let use_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
                        if use_aarnn {
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            if self.net.use_morphology {
                                for &(j, syn_idx) in self.recv_out.get(k).map(|v| v.as_slice()).unwrap_or(&[]) {
                                    let (steps, _) = self.syn_delay_and_atten(syn_idx);
                                    let s = self.hist_h_at(out_conn_layer, steps, j);
                                    if s != 0 {
                                        if fastrand::f32() <= self.release_probability(Some(syn_idx)) {
                                            let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                            acc += self.w_out[(k, j)] * stp_scale;
                                            if self.released_events.len() < 256 {
                                                self.released_events.push(ReleasedEvent { kind: ReleasedKind::Out, pre_layer: out_conn_layer as isize, post_layer: out_conn_layer as isize + 1, pre_id: j, post_id: k, syn_idx: Some(syn_idx) });
                                            }
                                        }
                                    }
                                }
                            } else {
                                let vel = self.net.aarnn_velocity.max(0.0);
                                for j in 0..num_last_layer_neurons {
                                    #[cfg(feature = "growth3d")]
                                    let dist = if let Some(nodes_last) = self.topo.layers.get(out_conn_layer) {
                                        if j < nodes_last.len() {
                                            let onode = &self.topo.output_nodes[k];
                                            let dx = nodes_last[j].x - onode.x;
                                            let dy = nodes_last[j].y - onode.y;
                                            let dz = nodes_last[j].z - onode.z;
                                            (dx * dx + dy * dy + dz * dz).sqrt()
                                        } else { 1.0 }
                                    } else { 1.0 };
                                    #[cfg(not(feature = "growth3d"))]
                                    let dist = 1.0f32;
                                    let dt_ms = self.lif.dt as f32;
                                    let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                    let s = {
                                        #[cfg(feature = "growth3d")]
                                        { if let Some(dq) = self.spk_hist_h.get(out_conn_layer) { let idx = steps_delay.min(dq.len().saturating_sub(1)); let frame = &dq[idx]; if frame.len()==0 {0} else { let jj=j.min(frame.len()-1); frame[jj] } } else { 0 } }
                                        #[cfg(not(feature = "growth3d"))]
                                        { if steps_delay >= 1 { 0 } else { self.last_spk_h[out_conn_layer][j] } }
                                    };
                                    if s != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc += self.w_out[(k, j)] * stp_scale;
                                    }
                                }
                            }
                            #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                            {
                                let vel = self.net.aarnn_velocity.max(0.0);
                                for j in 0..num_last_layer_neurons {
                                    #[cfg(feature = "growth3d")]
                                    let dist = if let Some(nodes_last) = self.topo.layers.get(out_conn_layer) {
                                        if j < nodes_last.len() {
                                            let onode = &self.topo.output_nodes[k];
                                            let dx = nodes_last[j].x - onode.x;
                                            let dy = nodes_last[j].y - onode.y;
                                            let dz = nodes_last[j].z - onode.z;
                                            (dx * dx + dy * dy + dz * dz).sqrt()
                                        } else { 1.0 }
                                    } else { 1.0 };
                                    #[cfg(not(feature = "growth3d"))]
                                    let dist = 1.0f32;
                                    let dt_ms = self.lif.dt as f32;
                                    let steps_delay = if self.net.use_aarnn_delays && vel > 0.0 { (dist / (vel * dt_ms)).ceil() as usize } else { 0 };
                                    let s = {
                                        #[cfg(feature = "growth3d")]
                                        {
                                            if let Some(dq) = self.spk_hist_h.get(out_conn_layer) {
                                                let idx = steps_delay.min(dq.len().saturating_sub(1));
                                                let frame = &dq[idx];
                                                if frame.len() == 0 { 0 } else { let jj = j.min(frame.len() - 1); frame[jj] }
                                            } else { 0 }
                                        }
                                        #[cfg(not(feature = "growth3d"))]
                                        { if steps_delay >= 1 { 0 } else { self.last_spk_h[out_conn_layer][j] } }
                                    };
                                    if s != 0 {
                                        let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                        acc += self.w_out[(k, j)] * stp_scale;
                                    }
                                }
                            }
                        } else {
                            for j in 0..num_last_layer_neurons {
                                if self.last_spk_h[out_conn_layer][j] != 0 {
                                    let stp_scale = if use_stp { stp_release_h.get(out_conn_layer).and_then(|v| v.get(j)).copied().unwrap_or(0.0) } else { 1.0 };
                                    acc += self.w_out[(k, j)] * stp_scale;
                                }
                            }
                        }
                        i_o[k] = acc;
                    }
                }
            }
        }

        if use_synaptic_filter && num_output_neurons > 0 && !gpu_filtered_o {
            i_o = Self::apply_synaptic_filter(
                self.lif.dt,
                &self.net.aarnn_bio,
                &i_o,
                &mut self.syn_ampa_o,
                &mut self.syn_nmda_o,
                &mut self.syn_gaba_o,
                Some(&self.v_o),
                self.net.aarnn_nmda_voltage_sensitivity.max(0.0) as f64,
                #[cfg(feature = "growth3d")] Some(&self.bio_o),
                #[cfg(not(feature = "growth3d"))] None,
                &default_decays,
            );
        }
        #[cfg(any(feature = "ui", feature = "growth3d"))]
        {
            self.last_i_o = Some(i_o.clone());
        }

        let spk_o = {
            observe_time!("Runner::step/spk_o");
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_success = false;
            #[cfg(feature = "opencl")]
            {
                let cl_mgr = self.cl.clone();
                if let Some(ref cl) = cl_mgr {
                    if !is_aarnn {
                        self.sync_cl_buffers(0, true);
                        let izh_params = self.effective_izh_params();
                        if let Some(ref mut buf) = self.cl_buffer_o {
                            // Upload i_o
                            gpu_success = true;
                            unsafe {
                                if let Some(slice) = i_o.as_slice() {
                                    if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf.i_total, CL_TRUE, 0, slice, &[]) {
                                        nm_log!("[warn] OpenCL output write i_total failed: {:?}", e);
                                        gpu_success = false;
                                    }
                                } else {
                                    gpu_success = false;
                                }
                            }
                            
                            if gpu_success {
                                let kernel_lif = cl.kernel_lif_step.lock().unwrap();
                                let kernel_izh = cl.kernel_izh_step.lock().unwrap();
                                match self.neuron_model {
                                    NeuronModel::Lif => {
                                        if let Some(ref refr_buf) = buf.refr {
                                            unsafe {
                                                let launch = ExecuteKernel::new(&kernel_lif)
                                                    .set_arg(&buf.v)
                                                    .set_arg(refr_buf)
                                                    .set_arg(&buf.i_total)
                                                    .set_arg(&self.decay_m)
                                                    .set_arg(&self.lif.v_th)
                                                    .set_arg(&self.lif.v_reset)
                                                    .set_arg(&(self.lif.refractory as i32))
                                                    .set_arg(&buf.spk)
                                                    .set_global_work_size(num_output_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL output lif_step failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            }
                                        } else {
                                            gpu_success = false;
                                        }
                                    }
                                    NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                                        let p = izh_params.expect("izh params for Izh/AARNN");
                                        if let Some(ref u_buf) = buf.u {
                                            unsafe {
                                                let launch = ExecuteKernel::new(&kernel_izh)
                                                    .set_arg(&buf.v)
                                                    .set_arg(u_buf)
                                                    .set_arg(&buf.i_total)
                                                    .set_arg(&p.dt)
                                                    .set_arg(&p.recovery_time_constant_a)
                                                    .set_arg(&p.recovery_sensitivity_b)
                                                    .set_arg(&p.membrane_reset_potential_c)
                                                    .set_arg(&p.recovery_increment_d)
                                                    .set_arg(&p.v_th)
                                                    .set_arg(&buf.spk)
                                                    .set_global_work_size(num_output_neurons)
                                                    .enqueue_nd_range(&cl.queue);
                                                if let Err(e) = launch {
                                                    nm_log!("[warn] OpenCL output izh_step failed: {:?}", e);
                                                    gpu_success = false;
                                                }
                                            }
                                        } else {
                                            gpu_success = false;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if gpu_success {
                #[cfg(feature = "opencl")]
                { self.sync_cl_state_from_gpu(0, true) }
                #[cfg(not(feature = "opencl"))]
                { unreachable!() }
            } else {
                match self.neuron_model {
                    NeuronModel::Lif => {
                        let mut r = Array1::<i8>::zeros(num_output_neurons);
                        let (old_v, old_refr): (Vec<f64>, Vec<i32>) = {
                            let ro = self.refr_o.as_ref().unwrap();
                            ((0..num_output_neurons).map(|k| self.v_o[k]).collect(), (0..num_output_neurons).map(|k| ro[k]).collect())
                        };
                        if can_parallel_light(num_output_neurons) {
                            let res: Vec<(f64, i32, i8)> = (0..num_output_neurons).into_par_iter().map(|k| {
                                let v = old_v[k] * self.decay_m + i_o[k];
                                let v_clamped = v.clamp(-5.0, 5.0);
                                let active = old_refr[k] <= 0;
                                let fired = active && v_clamped >= self.lif.v_th;
                                if fired { (self.lif.v_reset, self.lif.refractory as i32, 1) } else { (v_clamped, (old_refr[k] - 1).max(0), 0) }
                            }).collect();
                            let ro = self.refr_o.as_mut().unwrap();
                            for k in 0..num_output_neurons { self.v_o[k] = res[k].0; ro[k] = res[k].1; r[k] = res[k].2; }
                        } else {
                            let ro = self.refr_o.as_mut().unwrap();
                            for k in 0..num_output_neurons {
                                let v = self.v_o[k] * self.decay_m + i_o[k];
                                self.v_o[k] = v.clamp(-5.0, 5.0);
                                let active = ro[k] <= 0; let fired = active && self.v_o[k] >= self.lif.v_th;
                                if fired { self.v_o[k] = self.lif.v_reset; ro[k] = self.lif.refractory as i32; } else { ro[k] = (ro[k]-1).max(0); }
                                r[k] = fired as i8;
                            }
                        }
                        r
                    }
                    NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                        let p = self.effective_izh_params().expect("izh params for Izh/AARNN");
                        let mut r = Array1::<i8>::zeros(num_output_neurons);
                        let (old_v, old_u): (Vec<f64>, Vec<f64>) = {
                            let uo = self.u_o.as_ref().unwrap();
                            ((0..num_output_neurons).map(|k| self.v_o[k]).collect(), (0..num_output_neurons).map(|k| uo[k]).collect())
                        };
                        let old_refr: Vec<i32> = if use_izh_refractory {
                            let ro = self.izh_refr_o.as_ref().unwrap();
                            (0..num_output_neurons).map(|k| ro[k]).collect()
                        } else {
                            Vec::new()
                        };
                        if can_parallel_light(num_output_neurons) {
                            let res: Vec<(f64, f64, i8)> = (0..num_output_neurons).into_par_iter().map(|k| {
                                let v = old_v[k]; let u = old_u[k];
                                let nv = v + p.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + i_o[k]);
                                let nu = u + p.dt * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * nv - u));
                                let mut fired = nv >= p.v_th;
                                if use_adaptive_threshold {
                                    let thr_offset = self.thr_offset_o[k].clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    fired = nv >= (p.v_th + thr_offset);
                                }
                                if use_izh_refractory && old_refr[k] > 0 { fired = false; }
                                let (nv2, nu2) = if fired { (p.membrane_reset_potential_c, nu + p.recovery_increment_d) } else { (nv, nu) };
                                (nv2, nu2, fired as i8)
                            }).collect();
                            let uo = self.u_o.as_mut().unwrap();
                            for k in 0..num_output_neurons { self.v_o[k] = res[k].0; uo[k] = res[k].1; r[k] = res[k].2; }
                            if use_adaptive_threshold {
                                for k in 0..num_output_neurons {
                                    if r[k] != 0 {
                                        self.thr_offset_o[k] = (self.thr_offset_o[k] + bio.adaptive_threshold_increment)
                                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    }
                                }
                            }
                            if use_izh_refractory {
                                if let Some(ro) = self.izh_refr_o.as_mut() {
                                    for k in 0..num_output_neurons {
                                        if r[k] != 0 { ro[k] = izh_refractory_steps; }
                                        else { ro[k] = (ro[k] - 1).max(0); }
                                    }
                                }
                            }
                        } else {
                            let uo = self.u_o.as_mut().unwrap();
                            for k in 0..num_output_neurons {
                                let v = self.v_o[k]; let u = uo[k];
                                let nv = v + p.dt * (0.04*v*v + 5.0*v + 140.0 - u + i_o[k]);
                                let nu = u + p.dt * (p.recovery_time_constant_a * (p.recovery_sensitivity_b*nv - u));
                                let mut fired = nv >= p.v_th;
                                if use_adaptive_threshold {
                                    let thr_offset = self.thr_offset_o[k].clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                    fired = nv >= (p.v_th + thr_offset);
                                }
                                if use_izh_refractory {
                                    if let Some(ro) = self.izh_refr_o.as_ref() {
                                        if ro[k] > 0 { fired = false; }
                                    }
                                }
                                let (nv2, nu2) = if fired { (p.membrane_reset_potential_c, nu + p.recovery_increment_d) } else { (nv, nu) };
                                self.v_o[k] = nv2; uo[k] = nu2; r[k] = fired as i8;
                                if use_adaptive_threshold && fired {
                                    self.thr_offset_o[k] = (self.thr_offset_o[k] + bio.adaptive_threshold_increment)
                                        .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                                }
                                if use_izh_refractory {
                                    if let Some(ro) = self.izh_refr_o.as_mut() {
                                        if fired { ro[k] = izh_refractory_steps; }
                                        else { ro[k] = (ro[k] - 1).max(0); }
                                    }
                                }
                            }
                        }
                        r
                    }
                }
            }
        };
        self.last_spk_o = spk_o.clone();
        for k in 0..num_output_neurons { 
            if spk_o[k] != 0 { 
                self.x_post_o[k]+=1.0; 
                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                if self.net.use_morphology && k < self.morph.output_somas.len() {
                    self.morph.output_somas[k].stimuli += 1.0;
                }
            } 
        }
        if use_homeostasis {
            for k in 0..num_output_neurons {
                if spk_o[k] != 0 { self.rate_ema_o[k] += 1.0 - homeo_decay; }
                let err = self.rate_ema_o[k] - base_homeo_target;
                self.thr_offset_o[k] += bio.homeostasis_gain * err;
            }
        }

        // Learning updates (local, online)
        {
            observe_time!("Runner::step/learning");
            let mut eta = self.stdp.eta * neuromod_plasticity_gain;
            if matches!(self.learning, Learning::Stdp | Learning::Aarnn) && is_aarnn {
                let ltp_gain = self.net.aarnn_triplet_ltp_gain.max(0.0) as f64;
                let ltd_gain = self.net.aarnn_triplet_ltd_gain.max(0.0) as f64;
                if ltp_gain > 0.0 || ltd_gain > 0.0 {
                    let mut pre_sum = self.x_pre_in.iter().sum::<f64>();
                    let mut pre_count = self.x_pre_in.len();
                    for arr in &self.x_pre_h {
                        pre_sum += arr.iter().sum::<f64>();
                        pre_count += arr.len();
                    }
                    let mut post_sum = self.x_post_o.iter().sum::<f64>();
                    let mut post_count = self.x_post_o.len();
                    for arr in &self.x_post_h {
                        post_sum += arr.iter().sum::<f64>();
                        post_count += arr.len();
                    }
                    let mut rate_sum = self.rate_ema_o.iter().sum::<f64>();
                    let mut rate_count = self.rate_ema_o.len();
                    for arr in &self.rate_ema_h {
                        rate_sum += arr.iter().sum::<f64>();
                        rate_count += arr.len();
                    }
                    let pre_mean = if pre_count > 0 { pre_sum / pre_count as f64 } else { 0.0 };
                    let post_mean = if post_count > 0 { post_sum / post_count as f64 } else { 0.0 };
                    let rate_mean = if rate_count > 0 { rate_sum / rate_count as f64 } else { 0.0 };
                    let triplet_mod = (ltp_gain * pre_mean * post_mean) - (ltd_gain * rate_mean);
                    let triplet_scale = (1.0 + triplet_mod).clamp(0.05, 5.0);
                    eta *= triplet_scale;
                }
            }
            if eta != 0.0 {
            // W_in (H0 x S)
            if self.is_layer_assigned(0) {
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_success = false;
            #[cfg(feature = "opencl")]
            {
                let cl_mgr = self.cl.clone();
                if let Some(ref cl) = cl_mgr {
                    if matches!(self.learning, Learning::Stdp | Learning::Aarnn) {
                        self.sync_cl_w_in_to_gpu();
                        // Need sensory trace and spikes on GPU
                        let s_len = self.net.num_sensory_neurons;
                        if self.cl_x_pre_in.is_none() || self.cl_x_pre_in_size != s_len {
                            if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, s_len * std::mem::size_of::<f64>(), ptr::null_mut()) } {
                                self.cl_x_pre_in = Some(new_buf);
                                self.cl_x_pre_in_size = s_len;
                            }
                        }
                        if self.cl_s_t.is_none() || self.cl_s_t_size != s_len {
                            if let Ok(new_buf) = unsafe { Buffer::create(&cl.context, CL_MEM_READ_ONLY, s_len * std::mem::size_of::<i8>(), ptr::null_mut()) } {
                                self.cl_s_t = Some(new_buf);
                                self.cl_s_t_size = s_len;
                            }
                        }
                        
                        #[cfg(feature = "opencl")]
                        let cl_mgr = self.cl.clone();
                        #[cfg(feature = "opencl")]
                        if let Some(ref cl) = cl_mgr {
                            let w_buf_opt = self.cl_w_in.as_ref();
                            let x_pre_buf_opt = self.cl_x_pre_in.as_mut();
                            let s_buf_opt = self.cl_s_t.as_mut();
                            let h0_buf_opt = if let Some(Some(ref mut b)) = self.cl_buffers_h.get_mut(0) { Some(b) } else { None };

                            if let (Some(w_buf), Some(x_pre_buf), Some(s_buf), Some(h0_buf)) = (w_buf_opt, x_pre_buf_opt, s_buf_opt, h0_buf_opt) {
                                gpu_success = true;
                                unsafe {
                                    if let Some(slice) = self.x_pre_in.as_slice() {
                                        if let Err(e) = cl.queue.enqueue_write_buffer(x_pre_buf, CL_TRUE, 0, slice, &[]) {
                                            nm_log!("[warn] OpenCL learning x_pre_in write failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    } else { gpu_success = false; }
                                    
                                    if gpu_success {
                                        if let Err(e) = cl.queue.enqueue_write_buffer(s_buf, CL_TRUE, 0, &s_t, &[]) {
                                            nm_log!("[warn] OpenCL learning s_t write failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    }
                                    
                                    if gpu_success {
                                        // Ensure x_post is synced
                                        if let Some(slice) = self.x_post_h[0].as_slice() {
                                            if let Err(e) = cl.queue.enqueue_write_buffer(&mut h0_buf.x_trace, CL_TRUE, 0, slice, &[]) {
                                                nm_log!("[warn] OpenCL learning x_trace write failed: {:?}", e);
                                                gpu_success = false;
                                            }
                                        } else { gpu_success = false; }
                                    }
                                }

                                if gpu_success {
                                    let rule = match self.learning {
                                        Learning::Stdp | Learning::Aarnn => 0i32,
                                        Learning::Hebb => 1i32,
                                        Learning::Oja => 2i32,
                                    };

                                    let kernel_plasticity = cl.kernel_plasticity_update.lock().unwrap();
                                    unsafe {
                                        let launch = ExecuteKernel::new(&kernel_plasticity)
                                            .set_arg(w_buf)
                                            .set_arg(s_buf)
                                            .set_arg(&h0_buf.spk)
                                            .set_arg(x_pre_buf)
                                            .set_arg(&h0_buf.x_trace)
                                            .set_arg(&eta)
                                            .set_arg(&self.stdp.w_min)
                                            .set_arg(&self.stdp.w_max)
                                            .set_arg(&(num_sensory_neurons as i32))
                                            .set_arg(&(num_hidden_0_neurons as i32))
                                            .set_arg(&rule)
                                            .set_global_work_sizes(&[num_hidden_0_neurons, num_sensory_neurons])
                                            .enqueue_nd_range(&cl.queue);
                                        if let Err(e) = launch {
                                            nm_log!("[warn] OpenCL plasticity kernel failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    }
                                }
                            }
                        }
                        
                        #[cfg(feature = "opencl")]
                        if gpu_success {
                            self.sync_cl_w_in_from_gpu();
                        }
                    }
                }
            }

            if !gpu_success {
                if in_l == 0 {
                    if can_parallel_matrix(num_hidden_0_neurons, self.net.num_sensory_neurons) {
                        let num_sensory_neurons = self.net.num_sensory_neurons;
                        let w_min = self.stdp.w_min;
                        let w_max = self.stdp.w_max;
                        let learning = self.learning;
                        let last_spk_h0 = self.last_spk_h[0].as_slice().unwrap();
                        let x_post_h0 = self.x_post_h[0].as_slice().unwrap();
                        let x_pre_in = self.x_pre_in.as_slice().unwrap();
                        let sensory_spikes = s_t.as_slice();

                        self.w_in.axis_iter_mut(ndarray::Axis(0)).into_par_iter().enumerate().for_each(|(j, mut row)| {
                            let post = if last_spk_h0[j] != 0 { 1.0 } else { 0.0 };
                            let x_post = x_post_h0[j];
                            if post != 0.0 {
                                for i in 0..num_sensory_neurons {
                                    let pre = if sensory_spikes[i] != 0 { 1.0 } else { 0.0 };
                                    let dw = match learning {
                                        Learning::Stdp | Learning::Aarnn => eta * ((x_post * pre) - (post * x_pre_in[i])),
                                        Learning::Hebb => eta * (post * pre),
                                        Learning::Oja => eta * ((post * pre) - (post * post) * row[i]),
                                    };
                                    row[i] = (row[i] + dw).clamp(w_min, w_max);
                                }
                            } else if x_post > 1e-6 || matches!(learning, Learning::Hebb | Learning::Oja) {
                                 for &i in &active_s_indices {
                                     let pre = 1.0;
                                     let dw = match learning {
                                         Learning::Stdp | Learning::Aarnn => eta * (x_post * pre),
                                         Learning::Hebb => 0.0,
                                         Learning::Oja => 0.0,
                                     };
                                     if dw != 0.0 { row[i] = (row[i] + dw).clamp(w_min, w_max); }
                                 }
                            }
                        });
                    } else {
                        for j in 0..num_hidden_0_neurons {
                            let post = if self.last_spk_h[0][j] != 0 { 1.0 } else { 0.0 };
                            for i in 0..self.net.num_sensory_neurons {
                                let pre = if s_t[i] != 0 { 1.0 } else { 0.0 };
                                let dw = match self.learning {
                                    Learning::Stdp | Learning::Aarnn => eta * ((post*self.x_pre_in[i]) - (self.x_post_h[0][j]*pre)),
                                    Learning::Hebb => eta * (post*pre),
                                    Learning::Oja => eta * ((post*pre) - (post*post)*self.w_in[(j,i)]),
                                };
                                self.w_in[(j,i)] = (self.w_in[(j,i)] + dw).clamp(self.stdp.w_min, self.stdp.w_max);
                            }
                        }
                    }
                }
                #[cfg(feature = "opencl")]
                {
                    self.cl_w_in_dirty = true;
                }
            }
        }
        // Hidden fwd/bwd: iterate using actual interface shapes
        for l in 0..num_hidden_layers.saturating_sub(1) {
            let num_current_layer_neurons = self.layer_size(l);
            let num_next_layer_neurons = self.layer_size(l+1);
            // Only update if both layers are nonzero
            if num_current_layer_neurons == 0 || num_next_layer_neurons == 0 {
                continue;
            }
            if can_parallel_matrix(num_current_layer_neurons, num_next_layer_neurons) {
                let learning = self.learning;
                let w_min = self.stdp.w_min;
                let w_max = self.stdp.w_max;
                let last_spk_cur = self.last_spk_h[l].as_slice().unwrap();
                let last_spk_next = self.last_spk_h[l + 1].as_slice().unwrap();
                let x_pre_cur = self.x_pre_h[l].as_slice().unwrap();
                let x_pre_next = self.x_pre_h[l + 1].as_slice().unwrap();
                let x_post_cur = self.x_post_h[l].as_slice().unwrap();
                let x_post_next = self.x_post_h[l + 1].as_slice().unwrap();

                self.w_hh_fwd[l]
                    .axis_iter_mut(ndarray::Axis(0))
                    .into_par_iter()
                    .enumerate()
                    .for_each(|(j, mut row)| {
                        let post = if last_spk_next[j] != 0 { 1.0 } else { 0.0 };
                        let x_post = x_post_next[j];
                        for i in 0..num_current_layer_neurons {
                            let pre = if last_spk_cur[i] != 0 { 1.0 } else { 0.0 };
                            let dw = match learning {
                                Learning::Stdp | Learning::Aarnn => {
                                    eta * ((post * x_pre_cur[i]) - (x_post * pre))
                                }
                                Learning::Hebb => eta * (post * pre),
                                Learning::Oja => eta * ((post * pre) - (post * post) * row[i]),
                            };
                            row[i] = (row[i] + dw).clamp(w_min, w_max);
                        }
                    });

                self.w_hh_bwd[l]
                    .axis_iter_mut(ndarray::Axis(0))
                    .into_par_iter()
                    .enumerate()
                    .for_each(|(i, mut row)| {
                        let pre = if last_spk_cur[i] != 0 { 1.0 } else { 0.0 };
                        let x_post = x_post_cur[i];
                        for j in 0..num_next_layer_neurons {
                            let post = if last_spk_next[j] != 0 { 1.0 } else { 0.0 };
                            let dw = match learning {
                                Learning::Stdp | Learning::Aarnn => {
                                    eta * ((pre * x_pre_next[j]) - (x_post * post))
                                }
                                Learning::Hebb => eta * (post * pre),
                                Learning::Oja => eta * ((post * pre) - (post * post) * row[j]),
                            };
                            row[j] = (row[j] + dw).clamp(w_min, w_max);
                        }
                    });
            } else {
                for j in 0..num_next_layer_neurons {
                    for i in 0..num_current_layer_neurons {
                    let pre = if self.last_spk_h.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0) != 0 { 1.0 } else { 0.0 };
                    let post = if self.last_spk_h.get(l+1).and_then(|v| v.get(j)).copied().unwrap_or(0) != 0 { 1.0 } else { 0.0 };
                    let dwf = match self.learning {
                        Learning::Stdp | Learning::Aarnn => eta * ((post * self.x_pre_h.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0.0)) - (self.x_post_h.get(l + 1).and_then(|v| v.get(j)).copied().unwrap_or(0.0) * pre)),
                        Learning::Hebb => eta * (post * pre),
                        Learning::Oja => {
                            let w = self.w_hh_fwd.get(l).and_then(|m| m.get((j, i))).copied().unwrap_or(0.0);
                            eta * ((post * pre) - (post * post) * w)
                        },
                    };
                        if let Some(w) = self.w_hh_fwd.get_mut(l).and_then(|m| m.get_mut((j, i))) {
                            *w = (*w + dwf).clamp(self.stdp.w_min, self.stdp.w_max);
                        }
                    let dwb = match self.learning {
                        Learning::Stdp | Learning::Aarnn => eta * ((pre * self.x_pre_h.get(l + 1).and_then(|v| v.get(j)).copied().unwrap_or(0.0)) - (self.x_post_h.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0.0) * post)),
                        Learning::Hebb => eta * (post * pre),
                        Learning::Oja => {
                            let w = self.w_hh_bwd.get(l).and_then(|m| m.get((i, j))).copied().unwrap_or(0.0);
                            eta * ((post * pre) - (post * post) * w)
                        },
                    };
                        if let Some(w) = self.w_hh_bwd.get_mut(l).and_then(|m| m.get_mut((i, j))) {
                            *w = (*w + dwb).clamp(self.stdp.w_min, self.stdp.w_max);
                        }
                    }
                }
            }
            #[cfg(feature = "opencl")]
            {
                if l < self.cl_w_hh_fwd_dirty.len() { self.cl_w_hh_fwd_dirty[l] = true; }
                if l < self.cl_w_hh_bwd_dirty.len() { self.cl_w_hh_bwd_dirty[l] = true; }
            }
        }
        // W_out uses out_l layer
        if self.is_layer_assigned(num_hidden_layers) {
            let out_conn_layer = out_l;
            let num_last_layer_neurons = self.layer_size(out_conn_layer);
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut gpu_success = false;
            #[cfg(feature = "opencl")]
            if self.cl.is_some() {
                if num_last_layer_neurons > 0 && num_output_neurons > 0 {
                    self.sync_cl_w_out_to_gpu();
                    
                    if let Some(ref cl) = self.cl {
                        let cl_out_opt = self.cl_w_out.as_ref();
                        let buf_last_ptr = if let Some(Some(ref mut b)) = self.cl_buffers_h.get_mut(out_conn_layer) { Some(b as *mut CLBuffers) } else { None };
                        let buf_o_ptr = if let Some(ref mut b) = self.cl_buffer_o { Some(b as *mut CLBuffers) } else { None };

                        if let (Some(cl_out), Some(buf_last_p), Some(buf_o_p)) = (cl_out_opt, buf_last_ptr, buf_o_ptr) {
                            gpu_success = true;
                            let buf_last = unsafe { &mut *buf_last_p };
                            let buf_o = unsafe { &mut *buf_o_p };

                            let rule = match self.learning {
                                Learning::Stdp | Learning::Aarnn => 0i32,
                                Learning::Hebb => 1i32,
                                Learning::Oja => 2i32,
                            };

                            // Ensure traces are synced
                            unsafe {
                                if let (Some(s1), Some(s2)) = (self.x_post_h[out_conn_layer].as_slice(), self.x_post_o.as_slice()) {
                                    if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf_last.x_trace, CL_TRUE, 0, s1, &[]) {
                                        nm_log!("[warn] OpenCL learning out last_trace write failed: {:?}", e);
                                        gpu_success = false;
                                    }
                                    if gpu_success {
                                        if let Err(e) = cl.queue.enqueue_write_buffer(&mut buf_o.x_trace, CL_TRUE, 0, s2, &[]) {
                                            nm_log!("[warn] OpenCL learning out o_trace write failed: {:?}", e);
                                            gpu_success = false;
                                        }
                                    }
                                } else { gpu_success = false; }
                            }

                            if gpu_success {
                                let kernel_plasticity = cl.kernel_plasticity_update.lock().unwrap();
                                unsafe {
                                    let launch = ExecuteKernel::new(&kernel_plasticity)
                                        .set_arg(cl_out)
                                        .set_arg(&buf_last.spk)
                                        .set_arg(&buf_o.spk)
                                        .set_arg(&buf_last.x_trace)
                                        .set_arg(&buf_o.x_trace)
                                        .set_arg(&eta)
                                        .set_arg(&self.stdp.w_min)
                                        .set_arg(&self.stdp.w_max)
                                        .set_arg(&(num_last_layer_neurons as i32))
                                        .set_arg(&(num_output_neurons as i32))
                                        .set_arg(&rule)
                                        .set_global_work_sizes(&[num_output_neurons, num_last_layer_neurons])
                                        .enqueue_nd_range(&cl.queue);
                                    if let Err(e) = launch {
                                        nm_log!("[warn] OpenCL out plasticity kernel failed: {:?}", e);
                                        gpu_success = false;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            #[cfg(feature = "opencl")]
            if gpu_success {
                self.sync_cl_w_out_from_gpu();
            }

            if !gpu_success {
                if can_parallel_matrix(num_output_neurons, num_last_layer_neurons) {
                    let w_min = self.stdp.w_min;
                    let w_max = self.stdp.w_max;
                    let learning = self.learning;
                    let last_spk_h_last = self.last_spk_h[out_conn_layer].as_slice().unwrap();
                    let last_spk_o = self.last_spk_o.as_slice().unwrap();
                    let x_post_o = self.x_post_o.as_slice().unwrap();
                    let x_pre_h_last = self.x_pre_h[out_conn_layer].as_slice().unwrap();

                    self.w_out.axis_iter_mut(ndarray::Axis(0)).into_par_iter().enumerate().for_each(|(k, mut row)| {
                        let post = if last_spk_o[k] != 0 { 1.0 } else { 0.0 };
                        let x_post = x_post_o[k];
                        for j in 0..num_last_layer_neurons {
                            let pre = if last_spk_h_last[j] != 0 { 1.0 } else { 0.0 };
                            let dw = match learning {
                                Learning::Stdp | Learning::Aarnn => eta * ((x_post * pre) - (post * x_pre_h_last[j])),
                                Learning::Hebb => eta * (post * pre),
                                Learning::Oja => eta * ((post * pre) - (post * post) * row[j]),
                            };
                            row[j] = (row[j] + dw).clamp(w_min, w_max);
                        }
                    });
                } else {
                    for k in 0..num_output_neurons { for j in 0..num_last_layer_neurons {
                        let pre = if self.last_spk_h[out_conn_layer][j] != 0 { 1.0 } else { 0.0 };
                        let post = if self.last_spk_o[k] != 0 { 1.0 } else { 0.0 };
                        let dw = match self.learning {
                            Learning::Stdp | Learning::Aarnn => eta * ((post*self.x_pre_h[out_conn_layer][j]) - (self.x_post_o[k]*pre)),
                            Learning::Hebb => eta * (post*pre),
                            Learning::Oja => eta * ((post*pre) - (post*post)*self.w_out[(k,j)]),
                        };
                        self.w_out[(k,j)] = (self.w_out[(k,j)] + dw).clamp(self.stdp.w_min, self.stdp.w_max);
                    }}
                }
                #[cfg(feature = "opencl")]
                {
                    self.cl_w_out_dirty = true;
                }
            }
        }
            }
        }
        if is_aarnn {
            self.apply_synaptic_scaling();
            self.enforce_dale_constraints();
        }

        // Growth mechanics: collect and apply spawns
        #[cfg(feature = "growth3d")]
        let mut did_spawn = false;
        #[cfg(feature = "growth3d")]
        {
            if self.net.growth_enabled {
                observe_time!("Runner::step/growth");
                self.collect_growth_candidates();
                did_spawn = self.apply_growth_queue();

                // Spontaneous neuron addition
                if !did_spawn && self.last_global_growth_ms >= self.net.spontaneous_neuron_interval_ms {
                    let l = 0; // default to layer 0 for spontaneous spawns
                    let n = self.layer_size(l);
                    if n > 0 {
                        let pj = fastrand::usize(..n);
                        self.spawn_neuron_in_layer(l, pj);
                        did_spawn = true;
                        self.last_global_growth_ms = 0.0;
                        nm_log!("[growth] Spontaneous neuron addition in layer {}: parent index {}", l, pj);
                    }
                }

                if did_spawn { observe_hit!("growth_spawn"); }
            }
        }

        // After any potential growth, refresh morphology snapshot for overlays/debug
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            observe_time!("Runner::step/morpho");
            let num_layers = self.net.num_hidden_layers;
            if did_spawn {
                if matches!(self.neuron_model, NeuronModel::Aarnn) {
                    // Preserve existing morphology synapses for AARNN; only refresh maps/delay bounds.
                    self.recalc_hist_len_and_resize();
                    self.rebuild_syn_maps_from_morph();
                } else {
                    self.rebuild_morphology();
                }
                // Clear per-step transmission events (they belong to prior topology)
                self.released_events.clear();
            }

            if self.net.morpho_growth_enabled {
                // Update stimuli for synapses that released a spike this frame (Activity-dependent stabilization)
                let boost = self.net.synaptic_stabilization_strength;
                for ev in &self.released_events {
                    let idx = if let Some(syn_idx) = ev.syn_idx {
                        Some(syn_idx)
                    } else {
                        match ev.kind {
                            ReleasedKind::In => self.syn_in_map.get(ev.post_id).and_then(|v| v.get(ev.pre_id)).copied(),
                            ReleasedKind::Fwd { layer } => self.syn_fwd_map.get(layer).and_then(|v| v.get(ev.post_id)).and_then(|v| v.get(ev.pre_id)).copied(),
                            ReleasedKind::Bwd { layer } => self.syn_bwd_map.get(layer).and_then(|v| v.get(ev.post_id)).and_then(|v| v.get(ev.pre_id)).copied(),
                            ReleasedKind::HiddenRec { layer } => self.syn_rec_map.get(layer).and_then(|v| v.get(ev.post_id)).and_then(|v| v.get(ev.pre_id)).copied(),
                            ReleasedKind::Out => self.syn_out_map.get(ev.post_id).and_then(|v| v.get(ev.pre_id)).copied(),
                        }
                    }.unwrap_or(usize::MAX);
                    
                    if idx != usize::MAX && idx < self.morph.synapses.len() {
                        self.morph.synapses[idx].stimuli = (self.morph.synapses[idx].stimuli + boost).min(1.0);
                        
                        // Also boost the physical segments (Activity-regulated growth/maintenance)
                        let axon_seg_idx = self.morph.synapses[idx].axon_seg_idx;
                        let dend_seg_idx = self.morph.synapses[idx].dend_seg_idx;
                        let pre_l = self.morph.synapses[idx].pre_layer;
                        let pre_id = self.morph.synapses[idx].pre_id;
                        let post_l = self.morph.synapses[idx].post_layer;
                        let post_id = self.morph.synapses[idx].post_id;
                        
                        if let Some(asi) = axon_seg_idx {
                            if pre_l == -1 {
                                if pre_id < self.morph.sensory_axons.len() && asi < self.morph.sensory_axons[pre_id].segments.len() {
                                    self.morph.sensory_axons[pre_id].segments[asi].stimuli = (self.morph.sensory_axons[pre_id].segments[asi].stimuli + boost).min(1.0);
                                }
                            } else if pre_l == num_layers as isize {
                                if pre_id < self.morph.output_axons.len() && asi < self.morph.output_axons[pre_id].segments.len() {
                                    self.morph.output_axons[pre_id].segments[asi].stimuli = (self.morph.output_axons[pre_id].segments[asi].stimuli + boost).min(1.0);
                                }
                            } else if pre_l >= 0 && (pre_l as usize) < num_layers {
                                let pl = pre_l as usize;
                                if pre_id < self.morph.axons[pl].len() && asi < self.morph.axons[pl][pre_id].segments.len() {
                                    self.morph.axons[pl][pre_id].segments[asi].stimuli = (self.morph.axons[pl][pre_id].segments[asi].stimuli + boost).min(1.0);
                                }
                            }
                        }
                        
                        if let Some(dsi) = dend_seg_idx {
                            if post_l == -1 {
                                if post_id < self.morph.sensory_dendrites.len() && dsi < self.morph.sensory_dendrites[post_id].tree.branches.len() {
                                    self.morph.sensory_dendrites[post_id].tree.branches[dsi].stimuli = (self.morph.sensory_dendrites[post_id].tree.branches[dsi].stimuli + boost).min(1.0);
                                }
                            } else if post_l == num_layers as isize {
                                if post_id < self.morph.output_dendrites.len() && dsi < self.morph.output_dendrites[post_id].tree.branches.len() {
                                    self.morph.output_dendrites[post_id].tree.branches[dsi].stimuli = (self.morph.output_dendrites[post_id].tree.branches[dsi].stimuli + boost).min(1.0);
                                }
                            } else if post_l >= 0 && (post_l as usize) < num_layers {
                                let pl = post_l as usize;
                                if post_id < self.morph.dendrites[pl].len() && dsi < self.morph.dendrites[pl][post_id].tree.branches.len() {
                                    self.morph.dendrites[pl][post_id].tree.branches[dsi].stimuli = (self.morph.dendrites[pl][post_id].tree.branches[dsi].stimuli + boost).min(1.0);
                                }
                            }
                        }
                    }
                }

                // Balance morphology evolution frequency based on algorithm depth and load
                let depth = self.net.aarnn_layer_depth;
                let morpho_interval = match depth {
                    d if d >= 3 => 10.0,
                    2 => 50.0,
                    1 => 200.0,
                    _ => f32::MAX,
                };
                self.morpho_accumulated_dt += self.lif.dt as f32;
                if self.morpho_accumulated_dt >= morpho_interval {
                    self.apply_morpho_evolution(self.morpho_accumulated_dt, sleep_active);
                    self.morpho_accumulated_dt = 0.0;
                }
            }

            // Metabolic updates consume significant CPU; throttle based on depth
            let metabolic_interval = match self.net.aarnn_layer_depth {
                d if d >= 3 => 20.0,  // every 20ms
                2 => 100.0, // every 100ms
                _ => f32::MAX,
            };
            self.metabolic_accumulated_dt += self.lif.dt as f32;
            if self.metabolic_accumulated_dt >= metabolic_interval {
                self.apply_metabolic_update(self.metabolic_accumulated_dt);
                self.metabolic_accumulated_dt = 0.0;
            }

            // Neuron removal check: track time since each hidden neuron last had a bouton/synapse
            let num_h_layers = self.net.num_hidden_layers;
            let mut bouton_counts = (0..num_h_layers).map(|l| vec![0usize; self.layer_size(l)]).collect::<Vec<_>>();
            
            // 1. Count synapses as functional boutons
            for syn in &self.morph.synapses {
                if syn.pre_layer >= 0 && (syn.pre_layer as usize) < num_h_layers {
                    let pl = syn.pre_layer as usize;
                    if syn.pre_id < bouton_counts[pl].len() { bouton_counts[pl][syn.pre_id] += 1; }
                }
                if syn.post_layer >= 0 && (syn.post_layer as usize) < num_h_layers {
                    let pl = syn.post_layer as usize;
                    if syn.post_id < bouton_counts[pl].len() { bouton_counts[pl][syn.post_id] += 1; }
                }
            }
            
            // 2. Count physical axon/dendrite segments as potential boutons (seeking connections)
            for l in 0..num_h_layers {
                for j in 0..bouton_counts[l].len() {
                    if l < self.morph.axons.len() && j < self.morph.axons[l].len() {
                        bouton_counts[l][j] += self.morph.axons[l][j].segments.len();
                    }
                    if l < self.morph.dendrites.len() && j < self.morph.dendrites[l].len() {
                        bouton_counts[l][j] += self.morph.dendrites[l][j].tree.branches.len();
                    }
                }
            }
            
            let removal_delay = self.net.neuron_removal_delay_ms;
            let mut to_remove: Option<(usize, usize)> = None;
            for l in 0..num_h_layers {
                for j in 0..bouton_counts[l].len() {
                    if bouton_counts[l][j] > 0 {
                        self.since_last_bouton_ms[l][j] = 0.0;
                    } else {
                        self.since_last_bouton_ms[l][j] += dt_ms;
                        if self.since_last_bouton_ms[l][j] >= removal_delay && to_remove.is_none() {
                            // Only remove if it's not the last hidden neuron
                            let total_hidden: usize = self.v_h.iter().map(|a| a.len()).sum();
                            if total_hidden > 1 {
                                to_remove = Some((l, j));
                            }
                        }
                    }
                }
            }
            if let Some((rl, rj)) = to_remove {
                self.remove_neuron_in_layer(rl, rj);
                // Immediate post-structure resync to avoid stale indices in the remainder of this step
                let (in_l_sync, out_l_sync) = self.get_io_layers();
                let s_ch = self.ensure_state_dimensions();
                let w_ch = self.ensure_weight_dimensions(in_l_sync, out_l_sync);
                if s_ch || w_ch {
                    self.sync_presence_sizes();
                }
                // Ensure spike history frame widths match current layer sizes after removal
                for l in 0..self.net.num_hidden_layers {
                    let want = self.layer_size(l);
                    if let Some(dq) = self.spk_hist_h.get(l) {
                        if dq.front().map(|a| a.len()).unwrap_or(0) != want { self.extend_history_frames(l, want); }
                    }
                }
                if self.spk_hist_s.front().map(|a| a.len()).unwrap_or(0) != self.net.num_sensory_neurons {
                    self.extend_sensory_history(self.net.num_sensory_neurons);
                }
                self.recalc_hist_len_and_resize();
            }
        }

        #[cfg(feature = "growth3d")]
        if is_aarnn {
            let (target_in_layer, target_out_layer) = self.get_io_layers();
            // Sensory formation: target_in_layer exists
            if self.net.num_hidden_layers > target_in_layer && self.layer_size(target_in_layer) > 0 {
                if self.net.num_sensory_neurons < self.target_num_sensory {
                    // Growth rate limit: one neuron every 500ms
                    if self.t_ms - self.last_sensory_formation_ms >= 500.0 {
                        self.last_sensory_formation_ms = self.t_ms;
                        let next_s = self.net.num_sensory_neurons + 1;
                        self.resize_sensory(next_s);
                        nm_log!("[growth] AARNN sensory neuron formed: {}/{}", next_s, self.target_num_sensory);
                    }
                }
            }
            // Output formation: target_out_layer exists
            if self.net.num_hidden_layers > target_out_layer && self.layer_size(target_out_layer) > 0 {
                if self.net.num_output_neurons < self.target_num_output {
                    if self.t_ms - self.last_output_formation_ms >= 500.0 {
                        self.last_output_formation_ms = self.t_ms;
                        let next_o = self.net.num_output_neurons + 1;
                        self.resize_output(next_o);
                        nm_log!("[growth] AARNN output node formed: {}/{}", next_o, self.target_num_output);
                    }
                }
            }
        }

        // Safety barrier: growth/morphology above may have added/removed neurons or resized IO.
        // Re-align all vectors/matrices and histories before any further indexing.
        #[cfg(feature = "growth3d")]
        {
            let (in_l_sync, out_l_sync) = self.get_io_layers();
            let _s_ch = self.ensure_state_dimensions();
            let _w_ch = self.ensure_weight_dimensions(in_l_sync, out_l_sync);
            // Always sync presence sizes in the barrier if growth is enabled, 
            // to catch any mid-step structural changes that might have left them out of sync.
            self.sync_presence_sizes();
            
            // Also ensure spike history frame widths match current layer sizes (handles shrink cases)
            for l in 0..self.net.num_hidden_layers {
                let want = self.layer_size(l);
                if let Some(dq) = self.spk_hist_h.get(l) {
                    if dq.front().map(|a| a.len()).unwrap_or(0) != want { self.extend_history_frames(l, want); }
                }
            }
            if self.spk_hist_s.front().map(|a| a.len()).unwrap_or(0) != self.net.num_sensory_neurons {
                self.extend_sensory_history(self.net.num_sensory_neurons);
            }
            self.recalc_hist_len_and_resize();
            if is_aarnn && self.net.growth_enabled {
                self.ensure_sparse_io_connectivity_floor();
            }
        }

        // --- 8. Update connection presence counters ---
        for ((j, i), &w) in self.w_in.indexed_iter() {
            if w.abs() > 1e-8 {
                if let Some(cell) = self.conn_presence_in.get_mut((j, i)) { *cell += 1; }
            }
        }
        for (l, m) in self.w_hh_fwd.iter().enumerate() {
            for ((j, i), &w) in m.indexed_iter() {
                if w.abs() > 1e-8 {
                    if let Some(pres_l) = self.conn_presence_fwd.get_mut(l) {
                        if let Some(cell) = pres_l.get_mut((j, i)) { *cell += 1; }
                    }
                }
            }
        }
        for (l, m) in self.w_hh_bwd.iter().enumerate() {
            for ((j, i), &w) in m.indexed_iter() {
                if w.abs() > 1e-8 {
                    if let Some(pres_l) = self.conn_presence_bwd.get_mut(l) {
                        if let Some(cell) = pres_l.get_mut((j, i)) { *cell += 1; }
                    }
                }
            }
        }
        for (l, m) in self.w_hh_rec.iter().enumerate() {
            for ((j, i), &w) in m.indexed_iter() {
                if w.abs() > 1e-8 {
                    if let Some(pres_l) = self.conn_presence_rec.get_mut(l) {
                        if let Some(cell) = pres_l.get_mut((j, i)) { *cell += 1; }
                    }
                }
            }
        }
        for ((k, j), &w) in self.w_out.indexed_iter() {
            if w.abs() > 1e-8 {
                if let Some(cell) = self.conn_presence_out.get_mut((k, j)) { *cell += 1; }
            }
        }

        // Update world-model phase-space state
        if is_aarnn && self.net.world_model_enabled {
            if let Some(ref proj) = self.world_model_proj {
                let dim = self.net.world_model_dim.max(1);
                if self.world_model_state.len() != dim {
                    self.world_model_state.resize(dim, 0.0);
                }
                if self.world_model_prev_state.len() != dim {
                    self.world_model_prev_state.resize(dim, 0.0);
                }
                if self.world_model_prev_state.len() == self.world_model_state.len() {
                    self.world_model_prev_state.copy_from_slice(&self.world_model_state);
                }
                let decay = self.net.world_model_decay.clamp(0.0, 1.0) as f64;
                let retain = (1.0 - decay).max(0.0);
                let mut next = vec![0.0f64; dim];
                let mut idx = 0usize;
                let use_rate = use_homeostasis;
                for l in 0..num_hidden_layers {
                    let layer_len = self.layer_size(l);
                    if use_rate {
                        for j in 0..layer_len {
                            let v = self.rate_ema_h[l][j];
                            for d in 0..dim {
                                next[d] += proj[(d, idx)] * v;
                            }
                            idx += 1;
                        }
                    } else {
                        for j in 0..layer_len {
                            let v = self.last_spk_h[l][j] as f64;
                            for d in 0..dim {
                                next[d] += proj[(d, idx)] * v;
                            }
                            idx += 1;
                        }
                    }
                }
                for d in 0..dim {
                    self.world_model_state[d] = self.world_model_state[d] * retain + next[d];
                }
            }
        }

        self.t += 1;
        self.t_ms += self.lif.dt;
        StepOut {
            t: self.t,
            t_ms: self.t_ms,
            spk_h: self.last_spk_h.clone(),
            spk_o: self.last_spk_o.clone(),
        }
    }

    #[cfg(feature = "growth3d")]
    #[inline]
    fn dist3(a: (f32,f32,f32), b: (f32,f32,f32)) -> f32 {
        let dx=a.0-b.0; let dy=a.1-b.1; let dz=a.2-b.2; (dx*dx+dy*dy+dz*dz).sqrt()
    }

    #[cfg(feature = "growth3d")]
    fn place_node_near(&self, layer: usize, base: (f32,f32,f32)) -> (f32,f32,f32) {
        // Try to place a node near `base` while keeping at least min_node_sep from other nodes in the target layer
        let r = self.net.spawn_radius.max(0.001);
        let min_sep = self.net.min_node_sep.max(0.0);
        let tries = self.net.max_place_tries.max(1);
        let nodes = self.topo.layers.get(layer);
        let is_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);

        // Laminar placement bias: keep newly spawned neurons close to their cortical layer band.
        let layer_band_x = if let Some(layer_nodes) = nodes {
            if !layer_nodes.is_empty() {
                layer_nodes.iter().map(|n| n.x).sum::<f32>() / layer_nodes.len() as f32
            } else {
                let sens_x = if self.topo.sensory_nodes.is_empty() {
                    -0.1
                } else {
                    self.topo.sensory_nodes.iter().map(|n| n.x).sum::<f32>() / self.topo.sensory_nodes.len() as f32
                };
                let out_x = if self.topo.output_nodes.is_empty() {
                    0.1
                } else {
                    self.topo.output_nodes.iter().map(|n| n.x).sum::<f32>() / self.topo.output_nodes.len() as f32
                };
                let denom = (self.net.num_hidden_layers as f32 + 1.0).max(2.0);
                let t = ((layer as f32) + 1.0) / denom;
                sens_x + (out_x - sens_x) * t
            }
        } else {
            base.0
        };

        for _ in 0..tries {
            let (nx, ny, nz) = if is_aarnn {
                // AARNN: anisotropic spawn (more y/z spread, tighter x around layer band).
                let theta = fastrand::f32() * 2.0 * std::f32::consts::PI;
                let radial = r * fastrand::f32().sqrt();
                let x_jitter = (fastrand::f32() * 2.0 - 1.0) * (r * 0.25);
                let nx = (base.0 * 0.35 + layer_band_x * 0.65 + x_jitter).clamp(-1.0, 1.0);
                let ny = (base.1 + radial * theta.cos()).clamp(-1.0, 1.0);
                let nz = (base.2 + radial * theta.sin()).clamp(-1.0, 1.0);
                (nx, ny, nz)
            } else {
                let ux = fastrand::f32()*2.0 - 1.0;
                let uy = fastrand::f32()*2.0 - 1.0;
                let uz = fastrand::f32()*2.0 - 1.0;
                let norm = (ux*ux+uy*uy+uz*uz).sqrt().max(1e-6);
                let nx = (base.0 + r*ux/norm).clamp(-1.0, 1.0);
                let ny = (base.1 + r*uy/norm).clamp(-1.0, 1.0);
                let nz = (base.2 + r*uz/norm).clamp(-1.0, 1.0);
                (nx, ny, nz)
            };
            // Ensure inside skull membrane volume if morphology is enabled
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            if self.net.use_morphology {
                if let Some(skull) = self.morph.skull_membrane {
                    let (cx, cy, cz) = (skull.center.x, skull.center.y, skull.center.z);
                    // Prefer alpha-shape containment if available; else fall back to ellipsoid check
                    if let Some(alpha) = skull.alpha_radius {
                        let r_soma = 0.05f32;
                        let thr = alpha + r_soma;
                        // inside union-of-spheres (alpha-shape proxy): within (alpha + soma_r) of any existing soma in any hidden layer
                        let mut inside = false;
                        if let Some(layer_nodes) = nodes {
                            for n in layer_nodes { if Self::dist3((nx,ny,nz), (n.x,n.y,n.z)) <= thr { inside = true; break; } }
                        }
                        // also check other layers roughly around the base location to avoid empty layer corner cases
                        if !inside {
                            for other in self.topo.layers.iter() {
                                for n in other { if Self::dist3((nx,ny,nz), (n.x,n.y,n.z)) <= thr { inside = true; break; } }
                                if inside { break; }
                            }
                        }
                        if !inside { continue; }
                    } else {
                        let (rx, ry, rz) = skull.radii.unwrap_or_else(|| (skull.radius.max(1e-4), skull.radius.max(1e-4), skull.radius.max(1e-4)));
                        let dx = nx - cx; let dy = ny - cy; let dz = nz - cz;
                        let q = ((dx*dx)/(rx*rx) + (dy*dy)/(ry*ry) + (dz*dz)/(rz*rz)).sqrt();
                        if q >= 1.0 {
                            // Reject this sample; try again
                            continue;
                        }
                    }
                }
            }
            if let Some(layer_nodes) = nodes {
                let mut ok = true;
                for n in layer_nodes { if Self::dist3((nx,ny,nz), (n.x,n.y,n.z)) < min_sep { ok = false; break; } }
                if ok { return (nx,ny,nz); }
            } else { return (nx,ny,nz); }
        }
        // Fallback deterministic jitter; if skull exists and we're outside, project inward
        let mut jx = if is_aarnn {
            (base.0 * 0.35 + layer_band_x * 0.65).clamp(-1.0, 1.0)
        } else {
            (base.0 + 0.5 * r).clamp(-1.0, 1.0)
        };
        let mut jy = (base.1 - 0.3*r).clamp(-1.0, 1.0);
        let mut jz = (base.2 + 0.2*r).clamp(-1.0, 1.0);
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if self.net.use_morphology {
            if let Some(skull) = self.morph.skull_membrane {
                let (cx, cy, cz) = (skull.center.x, skull.center.y, skull.center.z);
                if let Some(alpha) = skull.alpha_radius {
                    let r_soma = 0.05f32;
                    let thr = alpha + r_soma;
                    // Nudge toward nearest soma within (alpha + soma_r) if outside (rough projection)
                    let mut best: Option<(f32,(f32,f32,f32))> = None;
                    for layer_nodes in self.topo.layers.iter() {
                        for n in layer_nodes {
                            let d = Self::dist3((jx,jy,jz), (n.x,n.y,n.z));
                            if d < thr && (best.is_none() || d < best.unwrap().0) {
                                best = Some((d, (n.x,n.y,n.z)));
                            }
                        }
                    }
                    if let Some((_d, target)) = best { jx = target.0; jy = target.1; jz = target.2; }
                } else {
                    let (rx, ry, rz) = skull.radii.unwrap_or_else(|| (skull.radius.max(1e-4), skull.radius.max(1e-4), skull.radius.max(1e-4)));
                    let mut dx = jx - cx; let mut dy = jy - cy; let mut dz = jz - cz;
                    let q = ((dx*dx)/(rx*rx) + (dy*dy)/(ry*ry) + (dz*dz)/(rz*rz)).sqrt();
                    if q >= 1.0 {
                        let s = 0.98 / q.max(1e-6);
                        dx *= s; dy *= s; dz *= s;
                        jx = cx + dx; jy = cy + dy; jz = cz + dz;
                    }
                }
            }
        }
        (jx, jy, jz)
    }

    #[cfg(feature = "growth3d")]
    fn extend_history_frames(&mut self, layer: usize, new_len: usize) {
        if let Some(dq) = self.spk_hist_h.get_mut(layer) {
            for fr in dq.iter_mut() {
                if fr.len() != new_len {
                    let old_len = fr.len();
                    let mut v = Array1::<i8>::zeros(new_len);
                    let n = old_len.min(new_len);
                    for j in 0..n { v[j] = fr[j]; }
                    *fr = v;
                }
            }
        }
    }

    #[cfg(feature = "growth3d")]
    fn extend_sensory_history(&mut self, new_len: usize) {
        for fr in self.spk_hist_s.iter_mut() {
            if fr.len() != new_len {
                let old_len = fr.len();
                let mut v = Array1::<i8>::zeros(new_len);
                let n = old_len.min(new_len);
                for i in 0..n { v[i] = fr[i]; }
                *fr = v;
            }
        }
    }

    #[cfg(feature = "growth3d")]
    fn recalc_hist_len_and_resize(&mut self) {
        // If delays disabled, keep history at 1 frame
        if !self.net.use_aarnn_delays || self.net.aarnn_velocity <= 0.0 {
            let target = 1usize;
            if self.hist_len != target {
                self.hist_len = target;
                // shrink/extend to exactly 1
                for dq in &mut self.spk_hist_h {
                    while dq.len() > self.hist_len { dq.pop_back(); }
                    while dq.len() < self.hist_len { dq.push_back(Array1::<i8>::zeros(dq.front().map(|a| a.len()).unwrap_or(0))); }
                }
                while self.spk_hist_s.len() > self.hist_len { self.spk_hist_s.pop_back(); }
                while self.spk_hist_s.len() < self.hist_len { self.spk_hist_s.push_back(Array1::<i8>::zeros(self.net.num_sensory_neurons)); }
            }
            return;
        }

        let vel = self.net.aarnn_velocity.max(0.0001);
        let dt = self.lif.dt.max(0.0001) as f32;

        // Compute maximum distance across current topology for S→H0, H→H, and H_last→O
        let mut max_dist: f32 = 0.0;
        let is_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
        // Sensory → Hidden (H0 normally, H1 for AARNN)
        let target_in_layer = if is_aarnn { 1 } else { 0 };
        if let Some(layer) = self.topo.layers.get(target_in_layer) {
            for j in 0..layer.len() {
                let hx = layer[j].x; let hy = layer[j].y; let hz = layer[j].z;
                for snode in &self.topo.sensory_nodes {
                    let dx = snode.x - hx; let dy = snode.y - hy; let dz = snode.z - hz;
                    let d = (dx*dx + dy*dy + dz*dz).sqrt();
                    if d > max_dist { max_dist = d; }
                }
            }
        }
        // H(l-1) → H(l) forward
        let l_count = self.topo.layers.len();
        for l in 1..l_count {
            let prev = &self.topo.layers[l-1];
            let cur = &self.topo.layers[l];
            for a in prev {
                for b in cur {
                    let dx = a.x - b.x; let dy = a.y - b.y; let dz = a.z - b.z;
                    let d = (dx*dx + dy*dy + dz*dz).sqrt();
                    if d > max_dist { max_dist = d; }
                }
            }
        }
        // Hidden → Output (H_last normally, H4 for AARNN)
        let (_, target_out_layer) = self.get_io_layers();
        if let Some(layer) = self.topo.layers.get(target_out_layer) {
            for j in 0..layer.len() {
                let hx = layer[j].x; let hy = layer[j].y; let hz = layer[j].z;
                for onode in &self.topo.output_nodes {
                    let dx = hx - onode.x; let dy = hy - onode.y; let dz = hz - onode.z;
                    let d = (dx*dx + dy*dy + dz*dz).sqrt();
                    if d > max_dist { max_dist = d; }
                }
            }
        }

        // Fallback if topology empty
        if max_dist == 0.0 { max_dist = 1.0; }
        let steps_delay_max = (max_dist / (vel * dt)).ceil() as usize;
        // hist_len must be at least steps_delay_max + 1 so index steps_delay is valid
        let new_hist = (steps_delay_max + 1).clamp(2, 128);
        if new_hist == self.hist_len { return; }
        self.hist_len = new_hist;
        // Trim or extend deques to match new length
        for dq in &mut self.spk_hist_h {
            while dq.len() > self.hist_len { dq.pop_back(); }
            while dq.len() < self.hist_len { dq.push_back(Array1::<i8>::zeros(dq.front().map(|a| a.len()).unwrap_or(0))); }
        }
        while self.spk_hist_s.len() > self.hist_len { self.spk_hist_s.pop_back(); }
        while self.spk_hist_s.len() < self.hist_len { self.spk_hist_s.push_back(Array1::<i8>::zeros(self.net.num_sensory_neurons)); }
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    /// Rebuild the morphology snapshot and routing caches from current weights
    /// and topology. Also recalculates history length bounds for delays.
    fn rebuild_morphology(&mut self) {
        self.morph = Morphology::from_weights(
            &self.topo.layers,
            &self.topo.sensory_nodes,
            &self.topo.output_nodes,
            &self.w_in,
            &self.w_hh_fwd,
            &self.w_hh_bwd,
            &self.w_out,
            &self.net,
            matches!(self.neuron_model, NeuronModel::Aarnn),
        );
        // Debug-only assertions
        #[cfg(debug_assertions)]
        {
            self.morph.assert_consistent(&self.topo.layers);
        }
        // After morphology nudges, delays might require a different history length
        #[cfg(feature = "growth3d")]
        {
            self.recalc_hist_len_and_resize();
        }

        // Build exact per-synapse routing caches from morphology
        self.rebuild_syn_maps_from_morph();
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    /// Build synapse index maps and exact per‑synapse axon/dendrite path lengths
    /// from the current morphology snapshot. Used by per‑segment AARNN routing.
    fn rebuild_syn_maps_from_morph(&mut self) {
        observe_time!("morphology/rebuild_syn_maps");
        // Initialize maps to MAX (meaning: missing)
        let l_count_hidden = self.net.num_hidden_layers;
        let (h_in_layer, h_out_layer) = self.get_io_layers();
        
        let h_in_size = self.layer_size(h_in_layer);
        let h_out_size = self.layer_size(h_out_layer);

        self.syn_in_map = vec![vec![usize::MAX; self.net.num_sensory_neurons]; h_in_size];
        self.syn_fwd_map = (0..l_count_hidden.saturating_sub(1)).map(|l| {
            let rows = self.layer_size(l+1);
            let cols = self.layer_size(l);
            vec![vec![usize::MAX; cols]; rows]
        }).collect();
        self.syn_bwd_map = (0..l_count_hidden.saturating_sub(1)).map(|l| {
            let rows = self.layer_size(l);
            let cols = self.layer_size(l+1);
            vec![vec![usize::MAX; cols]; rows]
        }).collect();
        self.syn_rec_map = (0..l_count_hidden).map(|l| {
            let n = self.layer_size(l);
            vec![vec![usize::MAX; n]; n]
        }).collect();
        self.syn_out_map = vec![vec![usize::MAX; h_out_size]; self.net.num_output_neurons];
        self.syn_ax_len.clear();
        self.syn_den_len.clear();
        self.syn_ax_len.resize(self.morph.synapses.len(), 0.0);
        self.syn_den_len.resize(self.morph.synapses.len(), 0.0);
        // Prepare sparse adjacency lists
        self.recv_in = vec![Vec::new(); h_in_size];
        self.recv_fwd = (0..l_count_hidden.saturating_sub(1)).map(|l| {
            let rows = self.layer_size(l+1);
            vec![Vec::<(usize,usize)>::new(); rows]
        }).collect();
        self.recv_bwd = (0..l_count_hidden.saturating_sub(1)).map(|l| {
            let rows = self.layer_size(l);
            vec![Vec::<(usize,usize)>::new(); rows]
        }).collect();
        self.recv_rec = (0..l_count_hidden).map(|l| {
            let rows = self.layer_size(l);
            vec![Vec::<(usize,usize)>::new(); rows]
        }).collect();
        self.recv_out = vec![Vec::new(); self.net.num_output_neurons];

        // Helper: get soma position
        let _soma_pos = |l: usize, j: usize| -> (f32,f32,f32) {
            if let Some(nodes) = self.topo.layers.get(l) {
                if j < nodes.len() { return (nodes[j].x, nodes[j].y, nodes[j].z); }
            }
            (0.0,0.0,0.0)
        };
        // Helper distance
        let dist3 = |a: (f32,f32,f32), b: (f32,f32,f32)| -> f32 {
            let dx=a.0-b.0; let dy=a.1-b.1; let dz=a.2-b.2; (dx*dx+dy*dy+dz*dz).sqrt()
        };
        // Dend hub finder: for a dendrite, trunk is segment whose to == soma; hub is trunk.from
        let find_hub = |dend: &crate::morphology::Dendrite, soma: crate::morphology::Point3| -> Option<crate::morphology::Point3> {
            for seg in &dend.tree.branches {
                if (seg.to.x - soma.x).abs() < 1e-5 && (seg.to.y - soma.y).abs() < 1e-5 && (seg.to.z - soma.z).abs() < 1e-5 {
                    return Some(seg.from);
                }
            }
            None
        };

        for (si, syn) in self.morph.synapses.iter().enumerate() {
            use crate::morphology::SynKind;
            // Map indices
            match syn.kind {
                SynKind::In => {
                    if syn.post_layer == h_in_layer as isize && (syn.post_id as usize) < h_in_size && syn.pre_layer < 0 {
                        let j = syn.post_id as usize; let i = syn.pre_id as usize;
                        if j < self.syn_in_map.len() && i < self.syn_in_map[j].len() { self.syn_in_map[j][i] = si; }
                        if j < self.recv_in.len() { self.recv_in[j].push((i, si)); }
                    }
                }
                SynKind::HiddenFwd => {
                    if syn.pre_layer >= 0 {
                        let l = syn.pre_layer as usize; // pre: l, post: l+1
                        if syn.post_layer == (l + 1) as isize {
                            let i = syn.pre_id as usize; let j = syn.post_id as usize;
                            if l < self.syn_fwd_map.len() {
                                let rows = self.syn_fwd_map[l].len();
                                let cols = if rows > 0 { self.syn_fwd_map[l][0].len() } else { 0 };
                                if j < rows && i < cols { self.syn_fwd_map[l][j][i] = si; }
                            }
                            if l < self.recv_fwd.len() {
                                let rows = self.recv_fwd[l].len();
                                if j < rows { self.recv_fwd[l][j].push((i, si)); }
                            }
                        }
                    }
                }
                SynKind::HiddenBwd => {
                    if syn.post_layer >= 0 {
                        let l = syn.post_layer as usize; // post: l, pre: l+1
                        if syn.pre_layer == (l + 1) as isize {
                            let i = syn.post_id as usize; let j = syn.pre_id as usize;
                            if l < self.syn_bwd_map.len() {
                                let rows = self.syn_bwd_map[l].len();
                                let cols = if rows > 0 { self.syn_bwd_map[l][0].len() } else { 0 };
                                if i < rows && j < cols { self.syn_bwd_map[l][i][j] = si; }
                            }
                            if l < self.recv_bwd.len() {
                                let rows = self.recv_bwd[l].len();
                                if i < rows { self.recv_bwd[l][i].push((j, si)); }
                            }
                        }
                    }
                }
                SynKind::HiddenRec => {
                    let l = syn.pre_layer as usize;
                    let i = syn.pre_id as usize; let j = syn.post_id as usize;
                    if l < self.syn_rec_map.len() {
                        let rows = self.syn_rec_map[l].len();
                        let cols = if rows > 0 { self.syn_rec_map[l][0].len() } else { 0 };
                        if j < rows && i < cols { self.syn_rec_map[l][j][i] = si; }
                    }
                    if l < self.recv_rec.len() {
                        let rows = self.recv_rec[l].len();
                        if j < rows { self.recv_rec[l][j].push((i, si)); }
                    }
                }
                SynKind::Out => {
                    if (syn.pre_layer as usize) == h_out_layer && syn.post_layer == l_count_hidden as isize {
                        let j = syn.pre_id as usize; let k = syn.post_id as usize;
                        if k < self.syn_out_map.len() && j < self.syn_out_map[k].len() {
                            self.syn_out_map[k][j] = si;
                        }
                        if k < self.recv_out.len() { self.recv_out[k].push((j, si)); }
                    }
                }
            }
            // Compute exact axon and dendrite path lengths
            // Axon length
            let ax_len = if syn.pre_layer >= 0 {
                // hidden pre: soma->hillock segment plus hillock->pre_site straight
                let l = syn.pre_layer as usize; let j = syn.pre_id as usize;
                let soma = self.morph.somas.get(l).and_then(|v| v.get(j)).map(|s| s.pos);
                let ax = self.morph.axons.get(l).and_then(|v| v.get(j));
                if let (Some(soma), Some(ax)) = (soma, ax) {
                    if let Some(seg0) = ax.segments.get(0) {
                        let hill = seg0.to;
                        seg0.length + dist3((hill.x, hill.y, hill.z), (syn.pre_site.x, syn.pre_site.y, syn.pre_site.z))
                    } else { dist3((soma.x,soma.y,soma.z), (syn.pre_site.x, syn.pre_site.y, syn.pre_site.z)) }
                } else { 0.0 }
            } else {
                // sensory pre: straight from sensory soma to pre_site (no modeled axon)
                // Sensory soma positions from topology (or virtual fallback)
                let i = syn.pre_id as usize;
                let (sx, sy, sz) = self.topo.sensory_nodes.get(i).map(|n| (n.x, n.y, n.z)).unwrap_or((-0.7, 0.0, 0.0));
                dist3((sx,sy,sz), (syn.pre_site.x, syn.pre_site.y, syn.pre_site.z))
            };
            self.syn_ax_len[si] = ax_len;

            // Dend length
            let den_len = if syn.post_layer >= 0 && (syn.post_layer as usize) < l_count_hidden {
                let l = syn.post_layer as usize; let j = syn.post_id as usize;
                let soma = self.morph.somas.get(l).and_then(|v| v.get(j)).map(|s| s.pos);
                let dend = self.morph.dendrites.get(l).and_then(|v| v.get(j));
                if let (Some(soma), Some(dend)) = (soma, dend) {
                    if let Some(hub) = find_hub(dend, soma) {
                        // post_site -> hub + hub -> soma
                        dist3((syn.post_site.x, syn.post_site.y, syn.post_site.z), (hub.x, hub.y, hub.z)) +
                        dist3((hub.x, hub.y, hub.z), (soma.x, soma.y, soma.z))
                    } else {
                        // fallback: straight post_site to soma
                        dist3((syn.post_site.x, syn.post_site.y, syn.post_site.z), (soma.x, soma.y, soma.z))
                    }
                } else {
                    0.0
                }
            } else if syn.post_layer == l_count_hidden as isize {
                // output postsynaptic: straight post_site -> output soma from topology
                let k = syn.post_id as usize;
                let (ox, oy, oz) = self.topo.output_nodes.get(k).map(|n| (n.x, n.y, n.z)).unwrap_or((1.0, 0.0, 0.0));
                dist3((syn.post_site.x, syn.post_site.y, syn.post_site.z), (ox, oy, oz))
            } else { 0.0 };
            self.syn_den_len[si] = den_len;
        }

        // Recompute history length upper bound from max total delay
        #[cfg(feature = "growth3d")]
        {
            let dt = self.lif.dt.max(1e-6) as f32;
            let v_ax = if self.net.axon_velocity > 0.0 { self.net.axon_velocity } else { self.net.aarnn_velocity.max(1e-6) };
            let v_den = if self.net.dend_velocity > 0.0 { self.net.dend_velocity } else { self.net.aarnn_velocity.max(1e-6) };
            let base_lat = self.net.bouton_latency_ms.max(0.0);
            let max_total_ms = self.syn_ax_len.iter().zip(self.syn_den_len.iter()).fold(0.0f32, |m, (&ax,&den)| {
                let t = ax / v_ax + den / v_den + base_lat; if t>m { t } else { m }
            });
            let need = ((max_total_ms / dt).ceil() as usize + 2).clamp(2, 256);
            if need != self.hist_len { self.hist_len = need; }
            // Ensure history deques have the new length
            for dq in &mut self.spk_hist_h {
                while dq.len() > self.hist_len { dq.pop_back(); }
                while dq.len() < self.hist_len { dq.push_back(Array1::<i8>::zeros(dq.front().map(|a| a.len()).unwrap_or(0))); }
            }
            while self.spk_hist_s.len() > self.hist_len { self.spk_hist_s.pop_back(); }
            while self.spk_hist_s.len() < self.hist_len { self.spk_hist_s.push_back(Array1::<i8>::zeros(self.net.num_sensory_neurons)); }
        }

        // Precompute per‑synapse steps based on current net params and algorithm depth
        let dt_ms = self.lif.dt.max(0.0001) as f32;
        let depth = self.net.aarnn_layer_depth;
        let ax_v = if depth > 0 && self.net.axon_velocity > 0.0 { self.net.axon_velocity } else { self.net.aarnn_velocity.max(0.0001) };
        let den_v = if depth > 0 && self.net.dend_velocity > 0.0 { self.net.dend_velocity } else { self.net.aarnn_velocity.max(0.0001) };
        self.syn_ax_steps.resize(self.morph.synapses.len(), 0);
        self.syn_den_steps.resize(self.morph.synapses.len(), 0);
        for si in 0..self.morph.synapses.len() {
            let ax_len = self.syn_ax_len[si];
            let den_len = self.syn_den_len[si];
            let ax_steps = (ax_len / (ax_v * dt_ms)).round() as usize;
            let den_steps = (den_len / (den_v * dt_ms)).round() as usize;
            self.syn_ax_steps[si] = ax_steps;
            self.syn_den_steps[si] = den_steps;
        }
        self.bouton_latency_steps = if depth >= 1 { (self.net.bouton_latency_ms.max(0.0) / dt_ms).round() as usize } else { 0 };
        self.bouton_jitter_steps = if depth >= 2 { (self.net.bouton_jitter_ms.max(0.0) / dt_ms).round() as usize } else { 0 };
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    fn apply_morpho_evolution(&mut self, dt: f32, sleep_active: bool) {
        if !self.net.morpho_growth_enabled { return; }
        let is_aarnn = matches!(self.neuron_model, NeuronModel::Aarnn);
        let mut net_view = self.net.clone();
        if sleep_active && self.net.sleep_enabled {
            let gain = self.net.sleep_consolidation_gain.clamp(0.0, 1.0);
            if gain > 0.0 {
                let boosted = (net_view.synaptic_consolidation_factor * (1.0 + gain)).min(1.0);
                net_view.synaptic_consolidation_factor = boosted;
            }
        }
        let res = self.morph.evolve(&net_view, is_aarnn, dt, #[cfg(feature = "opencl")] self.cl.as_ref());

        let mut changed = false;
        let (in_l, out_l) = self.get_io_layers();
        // Handle new connections
        for (pre_l, pre_id, post_l, post_id, weight) in res.new_connections {
            if pre_l == -1 {
                if post_l == in_l as isize && post_id < self.w_in.nrows() && pre_id < self.w_in.ncols() {
                    self.w_in[(post_id, pre_id)] = weight; changed = true;
                }
            } else if post_l == self.net.num_hidden_layers as isize {
                if pre_l == out_l as isize && post_id < self.w_out.nrows() && pre_id < self.w_out.ncols() {
                    self.w_out[(post_id, pre_id)] = weight; changed = true;
                }
            } else if post_l == pre_l + 1 {
                let l = pre_l as usize;
                if l < self.w_hh_fwd.len() && post_id < self.w_hh_fwd[l].nrows() && pre_id < self.w_hh_fwd[l].ncols() {
                    self.w_hh_fwd[l][(post_id, pre_id)] = weight; changed = true;
                }
            } else if pre_l == post_l + 1 {
                let l = post_l as usize;
                if l < self.w_hh_bwd.len() && pre_id < self.w_hh_bwd[l].ncols() && post_id < self.w_hh_bwd[l].nrows() {
                    self.w_hh_bwd[l][(post_id, pre_id)] = weight; changed = true;
                }
            } else if pre_l == post_l {
                let l = pre_l as usize;
                if l < self.w_hh_rec.len() && post_id < self.w_hh_rec[l].nrows() && pre_id < self.w_hh_rec[l].ncols() {
                    self.w_hh_rec[l][(post_id, pre_id)] = weight; changed = true;
                }
            }
        }
        // Handle broken connections
        for (pre_l, pre_id, post_l, post_id) in res.broken_connections {
            if pre_l == -1 {
                if post_l == in_l as isize && post_id < self.w_in.nrows() && pre_id < self.w_in.ncols() {
                    self.w_in[(post_id, pre_id)] = 0.0; changed = true;
                }
            } else if post_l == self.net.num_hidden_layers as isize {
                if pre_l == out_l as isize && post_id < self.w_out.nrows() && pre_id < self.w_out.ncols() {
                    self.w_out[(post_id, pre_id)] = 0.0; changed = true;
                }
            } else if post_l == pre_l + 1 {
                let l = pre_l as usize;
                if l < self.w_hh_fwd.len() && post_id < self.w_hh_fwd[l].nrows() && pre_id < self.w_hh_fwd[l].ncols() {
                    self.w_hh_fwd[l][(post_id, pre_id)] = 0.0; changed = true;
                }
            } else if pre_l == post_l + 1 {
                let l = post_l as usize;
                if l < self.w_hh_bwd.len() && pre_id < self.w_hh_bwd[l].ncols() && post_id < self.w_hh_bwd[l].nrows() {
                    self.w_hh_bwd[l][(post_id, pre_id)] = 0.0; changed = true;
                }
            } else if pre_l == post_l {
                let l = pre_l as usize;
                if l < self.w_hh_rec.len() && post_id < self.w_hh_rec[l].nrows() && pre_id < self.w_hh_rec[l].ncols() {
                    self.w_hh_rec[l][(post_id, pre_id)] = 0.0; changed = true;
                }
            }
        }
        if changed { self.rebuild_syn_maps_from_morph(); }

        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology {
                self.reassign_neurons_to_next_layer();
            }
        }

        // Synchronize Topology3D positions back from Morphology (since spatial forces may have moved somas)
        for (l, layer_somas) in self.morph.somas.iter().enumerate() {
            if l < self.topo.layers.len() {
                for (j, soma) in layer_somas.iter().enumerate() {
                    if j < self.topo.layers[l].len() {
                        self.topo.layers[l][j].x = soma.pos.x;
                        self.topo.layers[l][j].y = soma.pos.y;
                        self.topo.layers[l][j].z = soma.pos.z;
                    }
                }
            }
        }
        for (i, soma) in self.morph.sensory_somas.iter().enumerate() {
            if i < self.topo.sensory_nodes.len() {
                self.topo.sensory_nodes[i].x = soma.pos.x;
                self.topo.sensory_nodes[i].y = soma.pos.y;
                self.topo.sensory_nodes[i].z = soma.pos.z;
            }
        }
        for (k, soma) in self.morph.output_somas.iter().enumerate() {
            if k < self.topo.output_nodes.len() {
                self.topo.output_nodes[k].x = soma.pos.x;
                self.topo.output_nodes[k].y = soma.pos.y;
                self.topo.output_nodes[k].z = soma.pos.z;
            }
        }
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    fn apply_metabolic_update(&mut self, dt: f32) {
        observe_time!("morphology/metabolic_update");
        let recovery_rate = 0.0002 * dt; // ATP recovery per ms
        
        // Update Somas
        for layer_somas in &mut self.morph.somas {
            for soma in layer_somas {
                // If neuron spiked recently, consume ATP
                let spiked = self.last_spk_h.get(soma.layer).and_then(|v| v.get(soma.id)).copied().unwrap_or(0) != 0;
                if spiked {
                    soma.atp = (soma.atp - 0.05).max(0.0);
                }
                soma.atp = (soma.atp + recovery_rate).min(1.0);
                for org in &mut soma.organelles {
                    if org.kind == crate::morphology::OrganelleKind::Mitochondria {
                        org.activity = 0.3 + 0.7 * soma.atp;
                    }
                }
            }
        }
        for soma in &mut self.morph.sensory_somas {
            let spiked = self.spk_hist_s.front().and_then(|fr| fr.get(soma.id)).copied().unwrap_or(0) != 0;
            if spiked {
                soma.atp = (soma.atp - 0.05).max(0.0);
            }
            soma.atp = (soma.atp + recovery_rate).min(1.0);
        }
        for soma in &mut self.morph.output_somas {
            let spiked = self.last_spk_o.get(soma.id).copied().unwrap_or(0) != 0;
            if spiked {
                soma.atp = (soma.atp - 0.05).max(0.0);
            }
            soma.atp = (soma.atp + recovery_rate).min(1.0);
        }

        // Axon/Dendrite ATP also decays if active
        let decay = 0.999f32;
        #[cfg(feature = "parallel")]
        {
            self.morph.axons.par_iter_mut().for_each(|layer| {
                for axon in layer {
                    if axon.stimuli > 0.1 { axon.atp = (axon.atp - 0.001 * dt).max(0.0); }
                    axon.atp = (axon.atp + recovery_rate * 0.5).min(1.0);
                    axon.stimuli *= decay;
                }
            });
            self.morph.dendrites.par_iter_mut().for_each(|layer| {
                for dendrite in layer {
                    if dendrite.stimuli > 0.1 { dendrite.atp = (dendrite.atp - 0.001 * dt).max(0.0); }
                    dendrite.atp = (dendrite.atp + recovery_rate * 0.5).min(1.0);
                    dendrite.stimuli *= decay;
                }
            });
        }
        #[cfg(not(feature = "parallel"))]
        {
            for layer in &mut self.morph.axons {
                for axon in layer {
                    if axon.stimuli > 0.1 { axon.atp = (axon.atp - 0.001 * dt).max(0.0); }
                    axon.atp = (axon.atp + recovery_rate * 0.5).min(1.0);
                    axon.stimuli *= decay;
                }
            }
            for layer in &mut self.morph.dendrites {
                for dendrite in layer {
                    if dendrite.stimuli > 0.1 { dendrite.atp = (dendrite.atp - 0.001 * dt).max(0.0); }
                    dendrite.atp = (dendrite.atp + recovery_rate * 0.5).min(1.0);
                    dendrite.stimuli *= decay;
                }
            }
        }
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    #[inline]
    pub fn syn_delay_and_atten(&self, syn_idx: usize) -> (usize, f64) {
        let depth = self.net.aarnn_layer_depth;
        let ax_steps = self.syn_ax_steps.get(syn_idx).copied().unwrap_or(0);
        let den_steps = self.syn_den_steps.get(syn_idx).copied().unwrap_or(0);
        let base = ax_steps + den_steps + self.bouton_latency_steps;
        let mut steps = if depth >= 2 { self.apply_jitter_steps(base, syn_idx) } else { base };
        let mut atten = 1.0f64;
        let atten_k = self.net.aarnn_distance_attenuation_per_unit.max(0.0) as f64;
        if atten_k > 0.0 {
            let ax_len = self.syn_ax_len.get(syn_idx).copied().unwrap_or(0.0) as f64;
            let den_len = self.syn_den_len.get(syn_idx).copied().unwrap_or(0.0) as f64;
            let dist = (ax_len + den_len).max(0.0);
            atten = (-atten_k * dist).exp().clamp(1.0e-4, 1.0);
        }
        
        if depth >= 3 {
            if let Some(syn) = self.morph.synapses.get(syn_idx) {
                let ax_atp = match syn.pre_layer {
                    -1 => self.morph.sensory_axons.get(syn.pre_id).map(|a| a.atp).unwrap_or(1.0),
                    l if (l as usize) < self.morph.axons.len() => self.morph.axons[l as usize].get(syn.pre_id).map(|a| a.atp).unwrap_or(1.0),
                    _ => 1.0,
                };
                let den_atp = match syn.post_layer {
                    l if l >= 0 && (l as usize) < self.morph.dendrites.len() => self.morph.dendrites[l as usize].get(syn.post_id).map(|d| d.atp).unwrap_or(1.0),
                    l if l == self.net.num_hidden_layers as isize => self.morph.output_dendrites.get(syn.post_id).map(|d| d.atp).unwrap_or(1.0),
                    _ => 1.0,
                };
                let fatigue = (ax_atp * den_atp).clamp(0.01, 1.0) as f64;
                
                // If very fatigued, increase delay (slower conduction)
                if fatigue < 0.5 {
                    steps = (steps as f64 * (1.0 + (0.5 - fatigue))).round() as usize;
                }
            }
        }
        
        (steps, atten)
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    #[inline]
    fn apply_jitter_steps(&self, steps: usize, syn_idx: usize) -> usize {
        // Deterministic per-step jitter based on (t, syn_idx)
        let max_ms = self.net.bouton_jitter_ms.max(0.0);
        if max_ms <= 0.0 { return steps; }
        let dt = self.lif.dt.max(1e-6) as f32;
        let max_j = (max_ms / dt).round() as i32;
        if max_j == 0 { return steps; }
        // xorshift-ish hash
        let mut x: u64 = (self.t as u64).wrapping_mul(0x9E3779B185EBCA87) ^ (syn_idx as u64).wrapping_mul(0xD2B74407B1CE6E93);
        x ^= x >> 33; x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33; x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
        x ^= x >> 33;
        let r = (x & 0xffff) as i32; // 0..65535
        let s = (r - 32768) as f32 / 32768.0; // ~[-1,1]
        let jitter = (s * (max_j as f32)).round() as i32;
        let val = (steps as i32 + jitter).max(0);
        val as usize
    }

    #[inline]
    #[allow(dead_code)]
    pub fn hist_s_at(&self, steps: usize, i: usize) -> i8 {
        if self.spk_hist_s.is_empty() { return 0; }
        let idx = steps.min(self.spk_hist_s.len().saturating_sub(1));
        self.spk_hist_s[idx].get(i).copied().unwrap_or(0)
    }

    #[inline]
    #[allow(dead_code)]
    pub fn hist_h_at(&self, layer: usize, steps: usize, j: usize) -> i8 {
        if let Some(dq) = self.spk_hist_h.get(layer) {
            if dq.is_empty() { return 0; }
            let idx = steps.min(dq.len().saturating_sub(1));
            dq[idx].get(j).copied().unwrap_or(0)
        } else { 0 }
    }

    #[cfg(feature = "growth3d")]
    fn spawn_neuron_l0(&mut self, parent_j: usize) {
        if self.is_at_max_neurons() { return; }
        // Preconditions: growth enabled, operating with single hidden layer.
        let num_sensory_neurons = self.net.num_sensory_neurons;
        let num_output_neurons = self.net.num_output_neurons;
        let old_h_size = self.layer_size(0);
        let new_h_size = old_h_size + 1;
        let j_new = old_h_size;

        // 1) Update topology (3D): place near parent with minimum separation
        let (px, py, pz) = if let Some(layer0) = self.topo.layers.get(0) {
            if parent_j < layer0.len() {
                (layer0[parent_j].x, layer0[parent_j].y, layer0[parent_j].z)
            } else if parent_j < self.topo.sensory_nodes.len() {
                (self.topo.sensory_nodes[parent_j].x, self.topo.sensory_nodes[parent_j].y, self.topo.sensory_nodes[parent_j].z)
            } else { (0.0, 0.0, 0.0) }
        } else if parent_j < self.topo.sensory_nodes.len() {
             (self.topo.sensory_nodes[parent_j].x, self.topo.sensory_nodes[parent_j].y, self.topo.sensory_nodes[parent_j].z)
        } else { (0.0, 0.0, 0.0) };
        let (nx,ny,nz) = self.place_node_near(0, (px,py,pz));
        let (region_name, type_name) = self.allocate_region_and_type(nx, ny, nz);
        self.topo.add_neuron(0, Node3D { x: nx, y: ny, z: nz, layer: 0, region_name: region_name.clone(), type_name: type_name.clone() });

        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology {
                let pos = crate::morphology::Point3 { x: nx, y: ny, z: nz };
                let start_empty = matches!(self.neuron_model, NeuronModel::Aarnn);
                self.morph.add_hidden_neuron(0, j_new, pos, self.net.synapse_offset, start_empty, region_name, type_name.clone());
            }
        }

        // 2) Grow per-layer state vectors (layer 0)
        self.v_h[0] = Self::append_val(&self.v_h[0], 0.0);
        
        let bio = if let Some(tname) = type_name.as_ref() {
            self.net.neuron_types.iter().find(|t| &t.name == tname).map(|t| t.bio_params.clone()).unwrap_or(self.net.aarnn_bio.clone())
        } else {
            self.net.aarnn_bio.clone()
        };
        self.bio_h[0].push(bio);

        self.ensure_state_dimensions();

        // Initialize inherited rate values
        let parent_rate = self.rate_h[0].get(parent_j).copied().unwrap_or(0.0);
        if let Some(r) = self.rate_h[0].get_mut(j_new) { *r = (parent_rate * 0.5).clamp(0.0, 1.0); }
        
        // 3) Resize W_in: add a new row (receiver j_new) if layer 0 is the target
        let (in_l, out_l) = self.get_io_layers();
        if in_l == 0 {
            let mut new_w_in = Array2::<f64>::zeros((new_h_size, num_sensory_neurons));
            for j in 0..old_h_size { for i in 0..num_sensory_neurons {
                let val = self.w_in.get((j, i)).copied().unwrap_or_else(|| {
                    nm_log!("[error] Out of bounds: w_in[({}, {})], shape={:?}", j, i, self.w_in.dim());
                    0.0
                });
                if let Some(cell) = new_w_in.get_mut((j, i)) { *cell = val; }
            } }
            let aarnn_active = matches!(self.neuron_model, NeuronModel::Aarnn) || matches!(self.learning, Learning::Aarnn);
            if aarnn_active {
                for i in 0..num_sensory_neurons {
                    let val = self.w_in.get((parent_j, i)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_in[({}, {})], shape={:?}", parent_j, i, self.w_in.dim());
                        0.0
                    });
                    if let Some(cell) = new_w_in.get_mut((parent_j, i)) { *cell = val; }
                }
                let (hx, hy, hz) = if let Some(layer0) = self.topo.layers.get(0) {
                    if j_new < layer0.len() { (layer0[j_new].x, layer0[j_new].y, layer0[j_new].z) } else { (0.0, 0.0, 0.0) }
                } else { (0.0, 0.0, 0.0) };
                let mut best_i_any: Option<usize> = None; let mut best_d_any = f32::MAX;
                let mut best_i_free: Option<usize> = None; let mut best_d_free = f32::MAX;
                for i in 0..num_sensory_neurons {
                    let current_count = self.sensory_connection_count(i);
                    if current_count >= 6 { continue; }
                    let snode = match self.topo.sensory_nodes.get(i) {
                        Some(n) => n,
                        None => {
                            nm_log!("[error] Out of bounds: sensory_nodes[{}], len={}", i, self.topo.sensory_nodes.len());
                            continue;
                        }
                    };
                    let sx = snode.x;
                    let sy = snode.y;
                    let sz = snode.z;
                    let dx = sx - hx; let dy = sy - hy; let dz = sz - hz;
                    let d = (dx*dx + dy*dy + dz*dz).sqrt();
                    if d < best_d_any { best_d_any = d; best_i_any = Some(i); }
                    let mut connected = false;
                    for r in 0..old_h_size {
                        let v = new_w_in.get((r, i)).copied().unwrap_or(0.0);
                        if v != 0.0 { connected = true; break; }
                    }
                    if !connected && d < best_d_free { best_d_free = d; best_i_free = Some(i); }
                }
                if let Some(pick) = best_i_free.or(best_i_any) {
                    let w = (fastrand::f64() * 0.3 + 0.1).clamp(self.stdp.w_min, self.stdp.w_max);
                    if let Some(cell) = new_w_in.get_mut((j_new, pick)) { *cell = w; }
                    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                        nm_log!("[trace] synapse made: sensory {} -> hidden 0:{} - attached to new neuron", pick, j_new);
                    }
                }
            } else {
                let mut migrated = 0;
                for i in 0..num_sensory_neurons {
                    let w_old = self.w_in.get((parent_j, i)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_in[({}, {})], shape={:?}", parent_j, i, self.w_in.dim());
                        0.0
                    });
                    let current_count = self.sensory_connection_count(i);
                    if current_count < 6 && fastrand::f32() < self.net.migrate_in_prob.clamp(0.0, 1.0) {
                        let alpha = 0.4 + 0.2 * fastrand::f32();
                        let w_new = (alpha as f64) * w_old;
                        let w_par = ((1.0 - alpha as f64) * w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                        if let Some(cell) = new_w_in.get_mut((parent_j, i)) { *cell = w_par; }
                        if let Some(cell) = new_w_in.get_mut((j_new, i)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                        migrated += 1;
                    } else {
                        let val = if current_count < 6 && fastrand::f32() < self.net.p_in as f32 { fastrand::f64() * 0.2 + 0.05 } else { 0.0 };
                        let orig = self.w_in.get((parent_j, i)).copied().unwrap_or(0.0);
                        if let Some(cell) = new_w_in.get_mut((parent_j, i)) { *cell = orig; }
                        if let Some(cell) = new_w_in.get_mut((j_new, i)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                    }
                }
                if migrated > 0 {
                    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                        nm_log!("[trace] {} input synapses migrated from hidden 0:{} to new hidden 0:{}", migrated, parent_j, j_new);
                    }
                }
            }
            self.w_in = new_w_in;
        }

        // 4) Resize W_out: add a new column (sender j_new) if layer 0 is the target
        if out_l == 0 {
            let mut new_w_out = Array2::<f64>::zeros((num_output_neurons, new_h_size));
            let rows_to_copy = num_output_neurons.min(self.w_out.nrows());
            let cols_to_copy = old_h_size.min(self.w_out.ncols());
            for k in 0..rows_to_copy { for j in 0..cols_to_copy { new_w_out[(k, j)] = self.w_out[(k, j)]; } }
            let j_new = old_h_size;
            let mut migrated_out = 0;
            for k in 0..num_output_neurons {
                let w_old = self.w_out.get((k, parent_j)).copied().unwrap_or(0.0);
                if fastrand::f32() < self.net.migrate_out_prob.clamp(0.0, 1.0) {
                    let beta = 0.4 + 0.2 * fastrand::f32();
                    let w_new = (beta as f64) * w_old;
                    let w_par = ((1.0 - beta as f64) * w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                    if let Some(cell) = new_w_out.get_mut((k, parent_j)) { *cell = w_par; }
                    if let Some(cell) = new_w_out.get_mut((k, j_new)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                    migrated_out += 1;
                } else {
                    if let Some(cell) = new_w_out.get_mut((k, parent_j)) { *cell = w_old; }
                    // small random init
                    let val = if fastrand::f32() < self.net.p_out as f32 { fastrand::f64() * 0.2 + 0.05 } else { 0.0 };
                    if let Some(cell) = new_w_out.get_mut((k, j_new)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                }
            }
            if migrated_out > 0 {
                if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                    nm_log!("[trace] {} output synapses migrated from hidden 0:{} to new hidden 0:{}", migrated_out, parent_j, j_new);
                }
            }
            self.w_out = new_w_out;
        }

        // 4.5) Resize W_hh interfaces if multiple layers exist
        if self.net.num_hidden_layers >= 2 {
            // Layer 0 is sender for w_hh_fwd[0] (H1 x H0)
            let num_h1 = self.layer_size(1);
            let j_new = old_h_size;
            let mut new_fwd = Array2::<f64>::zeros((num_h1, new_h_size));
            let rows_to_copy = num_h1.min(self.w_hh_fwd[0].nrows());
            let cols_to_copy = old_h_size.min(self.w_hh_fwd[0].ncols());
            for j in 0..rows_to_copy {
                for i in 0..cols_to_copy { new_fwd[(j, i)] = self.w_hh_fwd[0][(j, i)]; }
            }
            // migrate outgoing from parent
            for j in 0..num_h1 {
                let w_old = self.w_hh_fwd[0].get((j, parent_j)).copied().unwrap_or(0.0);
                if fastrand::f32() < self.net.migrate_out_prob.clamp(0.0, 1.0) {
                    let beta = 0.4 + 0.2 * fastrand::f32();
                    let w_new = (beta as f64) * w_old;
                    let w_par = ((1.0 - beta as f64) * w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                    if let Some(cell) = new_fwd.get_mut((j, parent_j)) { *cell = w_par; }
                    if let Some(cell) = new_fwd.get_mut((j, j_new)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                } else {
                    if let Some(cell) = new_fwd.get_mut((j, parent_j)) { *cell = w_old; }
                    let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64() * 0.2 + 0.05 } else { 0.0 };
                    if let Some(cell) = new_fwd.get_mut((j, j_new)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                }
            }
            self.w_hh_fwd[0] = new_fwd;

            // Layer 0 is receiver for w_hh_bwd[0] (H0 x H1)
            let mut new_bwd = Array2::<f64>::zeros((new_h_size, num_h1));
            let rows_to_copy = old_h_size.min(self.w_hh_bwd[0].nrows());
            let cols_to_copy = num_h1.min(self.w_hh_bwd[0].ncols());
            for i in 0..rows_to_copy {
                for j in 0..cols_to_copy { new_bwd[(i, j)] = self.w_hh_bwd[0][(i, j)]; }
            }
            // Copy parent backward weights to new neuron
            for j in 0..num_h1 {
                if let Some(cell) = new_bwd.get_mut((j_new, j)) {
                    *cell = self.w_hh_bwd[0].get((parent_j, j)).copied().unwrap_or(0.0);
                }
            }
            self.w_hh_bwd[0] = new_bwd;
        }

        // 4.75) Resize w_hh_rec[0]
        let mut new_rec = Array2::<f64>::zeros((new_h_size, new_h_size));
        let rows_to_copy = old_h_size.min(self.w_hh_rec[0].nrows());
        let cols_to_copy = old_h_size.min(self.w_hh_rec[0].ncols());
        for j in 0..rows_to_copy {
            for i in 0..cols_to_copy {
                new_rec[(j, i)] = self.w_hh_rec[0][(j, i)];
            }
        }
        let aarnn_active = matches!(self.neuron_model, NeuronModel::Aarnn);
        let rec_p = self.net.p_hidden.clamp(0.0, 1.0) as f32;
        for i in 0..old_h_size {
            let v1 = self.w_hh_rec[0].get((parent_j, i)).copied().unwrap_or(0.0);
            let v2 = self.w_hh_rec[0].get((i, parent_j)).copied().unwrap_or(0.0);
            if let Some(cell) = new_rec.get_mut((j_new, i)) {
                *cell = if aarnn_active && fastrand::f32() >= rec_p { 0.0 } else { v1 };
            }
            if let Some(cell) = new_rec.get_mut((i, j_new)) {
                *cell = if aarnn_active && fastrand::f32() >= rec_p { 0.0 } else { v2 };
            }
        }
        if let Some(cell) = new_rec.get_mut((j_new, j_new)) {
            let v3 = self.w_hh_rec[0].get((parent_j, parent_j)).copied().unwrap_or(0.0);
            *cell = if aarnn_active && fastrand::f32() >= rec_p { 0.0 } else { v3 };
        }
        self.w_hh_rec[0] = new_rec;

        // 5) Update config H to reflect dynamic growth (keeps loops consistent for L==1)
        self.net.num_hidden_per_layer_initial = new_h_size;
        self.sync_presence_sizes();
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if self.net.use_morphology {
            self.rebuild_syn_maps_from_morph();
        }
    }

    pub fn layer_size(&self, l: usize) -> usize {
        if l < self.v_h.len() { self.v_h[l].len() } else { 0 }
    }

    pub fn total_neurons(&self) -> usize {
        let mut total = self.net.num_sensory_neurons + self.net.num_output_neurons;
        for l in 0..self.net.num_hidden_layers {
            total += self.layer_size(l);
        }
        total
    }

    #[allow(dead_code)]
    pub fn is_at_max_neurons(&self) -> bool {
        if self.net.max_total_neurons == 0 { return false; }
        self.total_neurons() as u64 >= self.net.max_total_neurons
    }

    #[inline]
    fn count_nonzero_matrix(m: &Array2<f64>) -> usize {
        #[cfg(feature = "parallel")]
        {
            if let Some(data) = m.as_slice_memory_order() {
                data.par_iter().filter(|&&w| w.abs() > 1e-8).count()
            } else {
                m.iter().filter(|&&w| w.abs() > 1e-8).count()
            }
        }
        #[cfg(not(feature = "parallel"))]
        {
            m.iter().filter(|&&w| w.abs() > 1e-8).count()
        }
    }

    /// Report number of non-zero connections (synapses) targeting each hidden layer.
    pub fn connection_counts(&self) -> Vec<usize> {
        let mut counts = vec![0; self.net.num_hidden_layers];
        if counts.is_empty() { return counts; }
        let (in_l, _) = self.get_io_layers();

        // 1. Incoming from Sensory
        if in_l < counts.len() {
            counts[in_l] += Self::count_nonzero_matrix(&self.w_in);
        }

        // 2. Hidden Forward: H(l) -> H(l+1)
        for (l, m) in self.w_hh_fwd.iter().enumerate() {
            // w_hh_fwd[l] targets layer l+1
            if l + 1 < counts.len() {
                counts[l + 1] += Self::count_nonzero_matrix(m);
            }
        }

        // 3. Hidden Backward: H(l+1) -> H(l)
        for (l, m) in self.w_hh_bwd.iter().enumerate() {
            // w_hh_bwd[l] targets layer l
            if l < counts.len() {
                counts[l] += Self::count_nonzero_matrix(m);
            }
        }

        // 4. Hidden Recurrent: H(l) -> H(l)
        for (l, m) in self.w_hh_rec.iter().enumerate() {
            if l < counts.len() {
                counts[l] += Self::count_nonzero_matrix(m);
            }
        }

        counts
    }

    /// Report number of non-zero connections targeting the Output layer.
    pub fn output_connection_count(&self) -> usize {
        Self::count_nonzero_matrix(&self.w_out)
    }

    /// Calculate the number of long-term connections (present for > 75% of runtime).
    /// Returns (longterm_count, total_active_count).
    pub fn calculate_longterm_connections(&self) -> (usize, usize) {
        if self.t == 0 {
            let total = self.connection_counts().iter().sum::<usize>() + self.output_connection_count();
            return (total, total); // At t=0, all existing connections are "longterm" by definition
        }
        
        let min_steps = self.min_steps_for_longterm();
        let mut longterm = 0;
        let mut total = 0;
        for ((j, i), &w) in self.w_in.indexed_iter() {
            if w.abs() > 1e-8 {
                total += 1;
                if self.conn_presence_in.get((j, i)).map(|&p| p >= min_steps).unwrap_or(false) {
                    longterm += 1;
                }
            }
        }
        for (l, m) in self.w_hh_fwd.iter().enumerate() {
            for ((j, i), &w) in m.indexed_iter() {
                if w.abs() > 1e-8 {
                    total += 1;
                    if self.conn_presence_fwd.get(l).and_then(|p| p.get((j, i))).map(|&p| p >= min_steps).unwrap_or(false) {
                        longterm += 1;
                    }
                }
            }
        }
        for (l, m) in self.w_hh_bwd.iter().enumerate() {
            for ((j, i), &w) in m.indexed_iter() {
                if w.abs() > 1e-8 {
                    total += 1;
                    if self.conn_presence_bwd.get(l).and_then(|p| p.get((j, i))).map(|&p| p >= min_steps).unwrap_or(false) {
                        longterm += 1;
                    }
                }
            }
        }
        for (l, m) in self.w_hh_rec.iter().enumerate() {
            for ((j, i), &w) in m.indexed_iter() {
                if w.abs() > 1e-8 {
                    total += 1;
                    if self.conn_presence_rec.get(l).and_then(|p| p.get((j, i))).map(|&p| p >= min_steps).unwrap_or(false) {
                        longterm += 1;
                    }
                }
            }
        }
        for ((k, j), &w) in self.w_out.indexed_iter() {
            if w.abs() > 1e-8 {
                total += 1;
                if self.conn_presence_out.get((k, j)).map(|&p| p >= min_steps).unwrap_or(false) {
                    longterm += 1;
                }
            }
        }

        (longterm, total)
    }

    #[inline]
    fn min_steps_for_longterm(&self) -> u32 {
        let window_steps = if self.net.synaptic_energy_window_ms > 0.0 && self.lif.dt > 0.0 {
            ((self.net.synaptic_energy_window_ms as f64) / self.lif.dt).ceil() as u32
        } else {
            0
        };
        let t_steps = self.t as u32;
        let effective_steps = if window_steps > 0 { t_steps.min(window_steps) } else { t_steps };
        (effective_steps as f32 * 0.75).ceil() as u32
    }

    #[allow(dead_code)]
    pub fn is_longterm_in(&self, j: usize, i: usize) -> bool {
        if self.t == 0 { return true; }
        self.conn_presence_in.get((j, i)).map(|&p| p >= self.min_steps_for_longterm()).unwrap_or(false)
    }
    #[allow(dead_code)]
    pub fn is_longterm_fwd(&self, l: usize, j: usize, i: usize) -> bool {
        if self.t == 0 { return true; }
        self.conn_presence_fwd.get(l).and_then(|m| m.get((j, i))).map(|&p| p >= self.min_steps_for_longterm()).unwrap_or(false)
    }
    #[allow(dead_code)]
    pub fn is_longterm_bwd(&self, l: usize, j: usize, i: usize) -> bool {
        if self.t == 0 { return true; }
        self.conn_presence_bwd.get(l).and_then(|m| m.get((j, i))).map(|&p| p >= self.min_steps_for_longterm()).unwrap_or(false)
    }
    #[allow(dead_code)]
    pub fn is_longterm_rec(&self, l: usize, j: usize, i: usize) -> bool {
        if self.t == 0 { return true; }
        self.conn_presence_rec.get(l).and_then(|m| m.get((j, i))).map(|&p| p >= self.min_steps_for_longterm()).unwrap_or(false)
    }
    #[allow(dead_code)]
    pub fn is_longterm_out(&self, k: usize, j: usize) -> bool {
        if self.t == 0 { return true; }
        self.conn_presence_out.get((k, j)).map(|&p| p >= self.min_steps_for_longterm()).unwrap_or(false)
    }

    /// Synchronize the dimensions of presence tracking counters with current weight matrices.
    fn sync_presence_sizes(&mut self) {
        // Sensory -> H0
        let target_in = self.w_in.dim();
        if self.conn_presence_in.dim() != target_in {
            let mut next = Array2::<u32>::zeros(target_in);
            let rs = target_in.0.min(self.conn_presence_in.nrows());
            let cs = target_in.1.min(self.conn_presence_in.ncols());
            for j in 0..rs { for i in 0..cs { next[(j, i)] = self.conn_presence_in[(j, i)]; } }
            self.conn_presence_in = next;
        }
        
        // Hidden Forward
        for l in 0..self.w_hh_fwd.len() {
            let target = self.w_hh_fwd[l].dim();
            if l >= self.conn_presence_fwd.len() || self.conn_presence_fwd[l].dim() != target {
                let mut next = Array2::<u32>::zeros(target);
                if l < self.conn_presence_fwd.len() {
                    let rs = target.0.min(self.conn_presence_fwd[l].nrows());
                    let cs = target.1.min(self.conn_presence_fwd[l].ncols());
                    for j in 0..rs { for i in 0..cs { next[(j, i)] = self.conn_presence_fwd[l][(j, i)]; } }
                    self.conn_presence_fwd[l] = next;
                } else {
                    self.conn_presence_fwd.push(next);
                }
            }
        }
        self.conn_presence_fwd.truncate(self.w_hh_fwd.len());

        // Hidden Backward
        for l in 0..self.w_hh_bwd.len() {
            let target = self.w_hh_bwd[l].dim();
            if l >= self.conn_presence_bwd.len() || self.conn_presence_bwd[l].dim() != target {
                let mut next = Array2::<u32>::zeros(target);
                if l < self.conn_presence_bwd.len() {
                    let rs = target.0.min(self.conn_presence_bwd[l].nrows());
                    let cs = target.1.min(self.conn_presence_bwd[l].ncols());
                    for j in 0..rs { for i in 0..cs { next[(j, i)] = self.conn_presence_bwd[l][(j, i)]; } }
                    self.conn_presence_bwd[l] = next;
                } else {
                    self.conn_presence_bwd.push(next);
                }
            }
        }
        self.conn_presence_bwd.truncate(self.w_hh_bwd.len());

        // Hidden Recurrent
        for l in 0..self.w_hh_rec.len() {
            let target = self.w_hh_rec[l].dim();
            if l >= self.conn_presence_rec.len() || self.conn_presence_rec[l].dim() != target {
                let mut next = Array2::<u32>::zeros(target);
                if l < self.conn_presence_rec.len() {
                    let rs = target.0.min(self.conn_presence_rec[l].nrows());
                    let cs = target.1.min(self.conn_presence_rec[l].ncols());
                    for j in 0..rs { for i in 0..cs { next[(j, i)] = self.conn_presence_rec[l][(j, i)]; } }
                    self.conn_presence_rec[l] = next;
                } else {
                    self.conn_presence_rec.push(next);
                }
            }
        }
        self.conn_presence_rec.truncate(self.w_hh_rec.len());

        // Hidden -> Output
        let target_out = self.w_out.dim();
        if self.conn_presence_out.dim() != target_out {
            let mut next = Array2::<u32>::zeros(target_out);
            let rs = target_out.0.min(self.conn_presence_out.nrows());
            let cs = target_out.1.min(self.conn_presence_out.ncols());
            for j in 0..rs { for i in 0..cs { next[(j, i)] = self.conn_presence_out[(j, i)]; } }
            self.conn_presence_out = next;
        }
    }

    /// Ensure weight matrices match current layer sizes to avoid out-of-bounds indexing.
    fn ensure_weight_dimensions(&mut self, in_l: usize, out_l: usize) -> bool {
        let mut changed = false;
        let num_layers = self.net.num_hidden_layers;
        let num_sensory = self.net.num_sensory_neurons;
        let num_output = self.net.num_output_neurons;

        let in_size = self.layer_size(in_l);
        if self.w_in.dim() != (in_size, num_sensory) {
            let mut next = Array2::<f64>::zeros((in_size, num_sensory));
            let rs = in_size.min(self.w_in.nrows());
            let cs = num_sensory.min(self.w_in.ncols());
            for j in 0..rs { for i in 0..cs { next[(j, i)] = self.w_in[(j, i)]; } }
            self.w_in = next;
            changed = true;
        }

        let out_size = self.layer_size(out_l);
        if self.w_out.dim() != (num_output, out_size) {
            let mut next = Array2::<f64>::zeros((num_output, out_size));
            let rs = num_output.min(self.w_out.nrows());
            let cs = out_size.min(self.w_out.ncols());
            for k in 0..rs { for j in 0..cs { next[(k, j)] = self.w_out[(k, j)]; } }
            self.w_out = next;
            changed = true;
        }

        let target_fwd_len = num_layers.saturating_sub(1);
        if self.w_hh_fwd.len() < target_fwd_len {
            self.w_hh_fwd.resize_with(target_fwd_len, || Array2::<f64>::zeros((0, 0)));
            changed = true;
        }
        if self.w_hh_bwd.len() < target_fwd_len {
            self.w_hh_bwd.resize_with(target_fwd_len, || Array2::<f64>::zeros((0, 0)));
            changed = true;
        }
        for l in 0..target_fwd_len {
            let rows = self.layer_size(l + 1);
            let cols = self.layer_size(l);
            if self.w_hh_fwd[l].dim() != (rows, cols) {
                let mut next = Array2::<f64>::zeros((rows, cols));
                let rs = rows.min(self.w_hh_fwd[l].nrows());
                let cs = cols.min(self.w_hh_fwd[l].ncols());
                for j in 0..rs { for i in 0..cs { next[(j, i)] = self.w_hh_fwd[l][(j, i)]; } }
                self.w_hh_fwd[l] = next;
                changed = true;
            }
            if self.w_hh_bwd[l].dim() != (cols, rows) {
                let mut next = Array2::<f64>::zeros((cols, rows));
                let rs = cols.min(self.w_hh_bwd[l].nrows());
                let cs = rows.min(self.w_hh_bwd[l].ncols());
                for j in 0..rs { for i in 0..cs { next[(j, i)] = self.w_hh_bwd[l][(j, i)]; } }
                self.w_hh_bwd[l] = next;
                changed = true;
            }
        }
        if self.w_hh_fwd.len() > target_fwd_len { self.w_hh_fwd.truncate(target_fwd_len); changed = true; }
        if self.w_hh_bwd.len() > target_fwd_len { self.w_hh_bwd.truncate(target_fwd_len); changed = true; }

        if self.w_hh_rec.len() < num_layers {
            self.w_hh_rec.resize_with(num_layers, || Array2::<f64>::zeros((0, 0)));
            changed = true;
        }
        for l in 0..num_layers {
            let n = self.layer_size(l);
            if self.w_hh_rec[l].dim() != (n, n) {
                let mut next = Array2::<f64>::zeros((n, n));
                let rs = n.min(self.w_hh_rec[l].nrows());
                let cs = n.min(self.w_hh_rec[l].ncols());
                for j in 0..rs { for i in 0..cs { next[(j, i)] = self.w_hh_rec[l][(j, i)]; } }
                self.w_hh_rec[l] = next;
                changed = true;
            }
        }
        if self.w_hh_rec.len() > num_layers { self.w_hh_rec.truncate(num_layers); changed = true; }

        changed
    }

    /// Ensure per-layer and IO state vectors match current layer sizes.
    fn ensure_state_dimensions(&mut self) -> bool {
        let mut changed = false;
        let l_count = self.net.num_hidden_layers;
        let layer_sizes: Vec<usize> = (0..l_count).map(|li| self.layer_size(li)).collect();
        let bio = self.net.aarnn_bio.clone();

        macro_rules! resize_layer_vec {
            ($vec:expr, $ty:ty, $init:expr) => {
                if $vec.len() != l_count {
                    $vec.resize_with(l_count, || Array1::<$ty>::from_elem(0, $init));
                    changed = true;
                }
                for (li, v) in $vec.iter_mut().enumerate() {
                    let sz = *layer_sizes.get(li).unwrap_or(&0);
                    if v.len() != sz {
                        let mut next = Array1::<$ty>::from_elem(sz, $init);
                        let min_sz = sz.min(v.len());
                        if min_sz > 0 {
                            next.slice_mut(s![..min_sz]).assign(&v.slice(s![..min_sz]));
                        }
                        *v = next;
                        changed = true;
                    }
                }
            };
        }

        resize_layer_vec!(self.v_h, f64, 0.0);
        if let Some(ref mut rfh) = self.refr_h { resize_layer_vec!(rfh, i32, 0); }
        if let Some(ref mut uh) = self.u_h { resize_layer_vec!(uh, f64, 0.0); }
        resize_layer_vec!(self.x_post_h, f64, 0.0);
        resize_layer_vec!(self.x_pre_h, f64, 0.0);
        resize_layer_vec!(self.last_spk_h, i8, 0);
        resize_layer_vec!(self.syn_ampa_h, f64, 0.0);
        resize_layer_vec!(self.syn_nmda_h, f64, 0.0);
        resize_layer_vec!(self.syn_gaba_h, f64, 0.0);
        resize_layer_vec!(self.thr_offset_h, f64, 0.0);
        resize_layer_vec!(self.rate_ema_h, f64, 0.0);
        resize_layer_vec!(self.stp_u_h, f64, bio.stp_u);
        resize_layer_vec!(self.stp_x_h, f64, 1.0);
        let s_count = self.net.num_sensory_neurons;
        let o_count = self.net.num_output_neurons;
        #[cfg(feature = "growth3d")]
        {
            resize_layer_vec!(self.rate_h, f32, 0.0);
            resize_layer_vec!(self.since_growth_ms, f32, 0.0);
            resize_layer_vec!(self.since_last_bouton_ms, f32, 0.0);

            if self.bio_h.len() != l_count {
                self.bio_h.resize_with(l_count, Vec::new);
                changed = true;
            }
            for (li, v) in self.bio_h.iter_mut().enumerate() {
                let sz = *layer_sizes.get(li).unwrap_or(&0);
                if v.len() != sz {
                    v.resize(sz, bio.clone());
                    changed = true;
                }
            }
            if self.bio_s.len() != s_count {
                self.bio_s.resize(s_count, bio.clone());
                changed = true;
            }
            if self.bio_o.len() != o_count {
                self.bio_o.resize(o_count, bio.clone());
                changed = true;
            }
            
            // Ensure spike history deques exist for all layers
            if self.spk_hist_h.len() != l_count {
                self.spk_hist_h.resize_with(l_count, || {
                    let mut dq: VecDeque<Array1<i8>> = VecDeque::new();
                    dq.push_front(Array1::<i8>::zeros(0));
                    dq
                });
                changed = true;
            }

            // Also ensure spike history frame widths match current sizes
            for l in 0..l_count {
                let sz = layer_sizes[l];
                if let Some(dq) = self.spk_hist_h.get_mut(l) {
                    for frame in dq.iter_mut() {
                        if frame.len() != sz {
                            let mut next = Array1::<i8>::zeros(sz);
                            let min_sz = sz.min(frame.len());
                            if min_sz > 0 {
                                next.slice_mut(s![..min_sz]).assign(&frame.slice(s![..min_sz]));
                            }
                            *frame = next;
                            changed = true;
                        }
                    }
                }
            }
        }
        if let Some(ref mut izh_ref) = self.izh_refr_h { resize_layer_vec!(izh_ref, i32, 0); }

        if self.x_pre_in.len() != s_count {
            let mut next = Array1::<f64>::zeros(s_count);
            let min_sz = s_count.min(self.x_pre_in.len());
            if min_sz > 0 {
                next.slice_mut(s![..min_sz]).assign(&self.x_pre_in.slice(s![..min_sz]));
            }
            self.x_pre_in = next;
            changed = true;
        }
        if self.pred_s.len() != s_count {
            let mut next = Array1::<f64>::zeros(s_count);
            let min_sz = s_count.min(self.pred_s.len());
            if min_sz > 0 {
                next.slice_mut(s![..min_sz]).assign(&self.pred_s.slice(s![..min_sz]));
            }
            self.pred_s = next;
            changed = true;
        }
        if self.stp_u_s.len() != s_count {
            self.stp_u_s = Array1::<f64>::from_elem(s_count, bio.stp_u);
            changed = true;
        }
        if self.stp_x_s.len() != s_count {
            self.stp_x_s = Array1::<f64>::from_elem(s_count, 1.0);
            changed = true;
        }

        if self.v_o.len() != o_count { self.v_o = Array1::<f64>::zeros(o_count); changed = true; }
        if self.last_spk_o.len() != o_count { self.last_spk_o = Array1::<i8>::zeros(o_count); changed = true; }
        if self.x_post_o.len() != o_count { self.x_post_o = Array1::<f64>::zeros(o_count); changed = true; }
        if self.syn_ampa_o.len() != o_count { self.syn_ampa_o = Array1::<f64>::zeros(o_count); changed = true; }
        if self.syn_nmda_o.len() != o_count { self.syn_nmda_o = Array1::<f64>::zeros(o_count); changed = true; }
        if self.syn_gaba_o.len() != o_count { self.syn_gaba_o = Array1::<f64>::zeros(o_count); changed = true; }
        if self.thr_offset_o.len() != o_count { self.thr_offset_o = Array1::<f64>::zeros(o_count); changed = true; }
        if self.rate_ema_o.len() != o_count { self.rate_ema_o = Array1::<f64>::zeros(o_count); changed = true; }
        if let Some(ref mut uo) = self.u_o {
            if uo.len() != o_count { *uo = Array1::<f64>::zeros(o_count); changed = true; }
        }
        if let Some(ref mut ro) = self.refr_o {
            if ro.len() != o_count { *ro = Array1::<i32>::zeros(o_count); changed = true; }
        }
        if let Some(ref mut izh_o) = self.izh_refr_o {
            if izh_o.len() != o_count { *izh_o = Array1::<i32>::zeros(o_count); changed = true; }
        }

        #[cfg(any(feature = "ui", feature = "growth3d"))]
        {
            let h0_sz = layer_sizes.get(0).copied().unwrap_or(0);
            if let Some(ref mut i_h0) = self.last_i_h0 {
                if i_h0.len() != h0_sz { *i_h0 = Array1::<f64>::zeros(h0_sz); changed = true; }
            }
            
            self.last_i_f.resize_with(l_count, || Array1::<f64>::zeros(0));
            for (li, v) in self.last_i_f.iter_mut().enumerate() {
                let sz = *layer_sizes.get(li).unwrap_or(&0);
                if v.len() != sz {
                    *v = Array1::<f64>::zeros(sz);
                    changed = true;
                }
            }

            if let Some(ref mut i_o) = self.last_i_o {
                if i_o.len() != o_count { *i_o = Array1::<f64>::zeros(o_count); changed = true; }
            }
        }

        // World-model projection/state sizing
        if self.net.world_model_enabled && self.net.world_model_dim > 0 {
            let total_hidden: usize = layer_sizes.iter().sum();
            let dim = self.net.world_model_dim;
            let needs_rebuild = self.world_model_proj.as_ref().map(|m| m.nrows() != dim || m.ncols() != total_hidden).unwrap_or(true)
                || self.world_model_input_dim != total_hidden
                || self.world_model_state.len() != dim
                || self.world_model_prev_state.len() != dim;
            if needs_rebuild {
                self.rebuild_world_model_projection(total_hidden, dim);
                changed = true;
            } else if self.world_model_prev_state.len() != dim {
                self.world_model_prev_state.resize(dim, 0.0);
            }
        } else {
            if !self.world_model_state.is_empty() || self.world_model_proj.is_some() || !self.world_model_prev_state.is_empty() {
                self.world_model_state.clear();
                self.world_model_proj = None;
                self.world_model_input_dim = 0;
                self.world_model_prev_state.clear();
                changed = true;
            }
        }

        changed
    }

    fn rebuild_world_model_projection(&mut self, input_dim: usize, dim: usize) {
        self.world_model_state = vec![0.0; dim];
        self.world_model_prev_state = vec![0.0; dim];
        self.world_model_input_dim = input_dim;
        if input_dim == 0 || dim == 0 {
            self.world_model_proj = None;
            return;
        }
        let mut proj = Array2::<f64>::zeros((dim, input_dim));
        let scale = 1.0 / (input_dim as f64).sqrt().max(1e-6);
        for d in 0..dim {
            for i in 0..input_dim {
                let mut x = (d as u64).wrapping_mul(0x9E3779B97F4A7C15)
                    ^ (i as u64).wrapping_mul(0xBF58476D1CE4E5B9)
                    ^ 0xD1B54A32D192ED03;
                x ^= x >> 30;
                x = x.wrapping_mul(0xBF58476D1CE4E5B9);
                x ^= x >> 27;
                x = x.wrapping_mul(0x94D049BB133111EB);
                x ^= x >> 31;
                let v = ((x & 0xFFFF) as f64) / 32767.5 - 1.0;
                proj[(d, i)] = v * scale;
            }
        }
        self.world_model_proj = Some(proj);
    }

    /// Report number of non-zero connections for a specific sensory neuron.
    #[allow(dead_code)]
    pub fn sensory_connection_count(&self, i: usize) -> usize {
        let mut count = 0;
        let rows = self.w_in.nrows();
        if i < self.w_in.ncols() {
            for j in 0..rows {
                if self.w_in[(j, i)] != 0.0 {
                    count += 1;
                }
            }
        }
        count
    }

    #[cfg(feature = "growth3d")]
    fn output_connection_count_for_output(&self, k: usize) -> usize {
        let mut count = 0usize;
        if k < self.w_out.nrows() {
            for j in 0..self.w_out.ncols() {
                if self.w_out[(k, j)].abs() > 1e-12 {
                    count += 1;
                }
            }
        }
        count
    }

    #[cfg(feature = "growth3d")]
    fn ensure_sparse_io_connectivity_floor(&mut self) {
        // Morphology-driven runs maintain canonical synapses in `morph.synapses`.
        // The morphology evolve path enforces the same floor there.
        if self.net.use_morphology {
            return;
        }
        let (in_l, out_l) = self.get_io_layers();
        let h_in = self.layer_size(in_l);
        let h_out = self.layer_size(out_l);
        if h_in == 0 && h_out == 0 {
            return;
        }

        let mut changed = false;
        let sensory_min_required = 1usize;
        let output_min_required = 1usize;
        let sensory_cap = if self.net.max_sensory_connections == 0 {
            usize::MAX
        } else {
            self.net.max_sensory_connections
        };
        let output_cap = if self.net.max_output_connections == 0 {
            usize::MAX
        } else {
            self.net.max_output_connections
        };

        if h_in > 0 {
            let sensory_count = self.net.num_sensory_neurons.min(self.w_in.ncols());
            for i in 0..sensory_count {
                let current = self.sensory_connection_count(i);
                if current >= sensory_min_required || current >= sensory_cap {
                    continue;
                }
                let max_add = sensory_cap.saturating_sub(current);
                let needed = sensory_min_required.saturating_sub(current).min(max_add);
                if needed == 0 {
                    continue;
                }

                let mut candidates: Vec<(usize, f32)> = Vec::new();
                for j in 0..h_in.min(self.w_in.nrows()) {
                    if self.w_in[(j, i)].abs() > 1e-12 {
                        continue;
                    }
                    let score = if i < self.topo.sensory_nodes.len()
                        && in_l < self.topo.layers.len()
                        && j < self.topo.layers[in_l].len()
                    {
                        let s = &self.topo.sensory_nodes[i];
                        let h = &self.topo.layers[in_l][j];
                        let dx = s.x - h.x;
                        let dy = s.y - h.y;
                        let dz = s.z - h.z;
                        (dx * dx + dy * dy + dz * dz).sqrt()
                    } else {
                        fastrand::f32() + (j as f32) * 1e-6
                    };
                    candidates.push((j, score));
                }
                candidates.sort_by(|a, b| {
                    a.1.partial_cmp(&b.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for (j, _) in candidates.into_iter().take(needed) {
                    let w = (fastrand::f64() * 0.3 + 0.1).clamp(self.stdp.w_min, self.stdp.w_max);
                    self.w_in[(j, i)] = w;
                    changed = true;
                }
            }
        }

        if h_out > 0 {
            let out_count = self.net.num_output_neurons.min(self.w_out.nrows());
            for k in 0..out_count {
                let current = self.output_connection_count_for_output(k);
                if current >= output_min_required || current >= output_cap {
                    continue;
                }
                let max_add = output_cap.saturating_sub(current);
                let needed = output_min_required.saturating_sub(current).min(max_add);
                if needed == 0 {
                    continue;
                }

                let mut candidates: Vec<(usize, f32)> = Vec::new();
                for j in 0..h_out.min(self.w_out.ncols()) {
                    if self.w_out[(k, j)].abs() > 1e-12 {
                        continue;
                    }
                    let score = if k < self.topo.output_nodes.len()
                        && out_l < self.topo.layers.len()
                        && j < self.topo.layers[out_l].len()
                    {
                        let h = &self.topo.layers[out_l][j];
                        let o = &self.topo.output_nodes[k];
                        let dx = h.x - o.x;
                        let dy = h.y - o.y;
                        let dz = h.z - o.z;
                        (dx * dx + dy * dy + dz * dz).sqrt()
                    } else {
                        fastrand::f32() + (j as f32) * 1e-6
                    };
                    candidates.push((j, score));
                }
                candidates.sort_by(|a, b| {
                    a.1.partial_cmp(&b.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for (j, _) in candidates.into_iter().take(needed) {
                    let w = (fastrand::f64() * 0.3 + 0.1).clamp(self.stdp.w_min, self.stdp.w_max);
                    self.w_out[(k, j)] = w;
                    changed = true;
                }
            }
        }

        if changed {
            #[cfg(feature = "opencl")]
            self.mark_all_weights_dirty();
        }
    }

    #[cfg(feature = "growth3d")]
    fn ensure_layer_exists(&mut self, l: usize) {
        let max_layers = self.effective_max_layers();
        if max_layers == 0 || l >= max_layers {
            return;
        }
        let current_l_count = self.net.num_hidden_layers;
        if l < current_l_count { return; }
        let target = l.min(max_layers.saturating_sub(1));
        // Add new hidden layers up to target (inclusive)
        for _ in current_l_count..=target {
            self.net.num_hidden_layers += 1;
            // per-neuron vectors start empty; will be appended on spawn
            self.v_h.push(Array1::<f64>::zeros(0));
            match self.neuron_model {
                NeuronModel::Lif => {
                    if self.refr_h.is_none() { self.refr_h = Some(Vec::new()); }
                    self.refr_h.as_mut().unwrap().push(Array1::<i32>::zeros(0));
                }
                NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                    if self.u_h.is_none() { self.u_h = Some(Vec::new()); }
                    self.u_h.as_mut().unwrap().push(Array1::<f64>::zeros(0));
                }
            }
            self.x_post_h.push(Array1::<f64>::zeros(0));
            self.x_pre_h.push(Array1::<f64>::zeros(0));
            self.last_spk_h.push(Array1::<i8>::zeros(0));
            self.rate_h.push(Array1::<f32>::zeros(0));
            self.since_growth_ms.push(Array1::<f32>::zeros(0));
            self.since_last_bouton_ms.push(Array1::<f32>::zeros(0));
            self.syn_ampa_h.push(Array1::<f64>::zeros(0));
            self.syn_nmda_h.push(Array1::<f64>::zeros(0));
            self.syn_gaba_h.push(Array1::<f64>::zeros(0));
            self.thr_offset_h.push(Array1::<f64>::zeros(0));
            self.rate_ema_h.push(Array1::<f64>::zeros(0));
            self.stp_u_h.push(Array1::<f64>::zeros(0));
            self.stp_x_h.push(Array1::<f64>::zeros(0));
            if let Some(ref mut r) = self.izh_refr_h { r.push(Array1::<i32>::zeros(0)); }
            self.bio_h.push(Vec::new());
            self.w_hh_rec.push(Array2::<f64>::zeros((0, 0)));
            #[cfg(feature = "opencl")]
            {
                self.cl_buffers_h.push(None);
                self.cl_spk_hist_h.push(None);
                self.cl_spk_hist_h_sizes.push(0);
            }
            // Initialize corresponding spike history deque
            self.spk_hist_h.push({ let mut dq = VecDeque::new(); dq.push_front(Array1::<i8>::zeros(0)); dq });
            // Topology
            self.topo.add_layer();
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            {
                if self.morph.somas.len() < self.net.num_hidden_layers {
                    self.morph.somas.resize_with(self.net.num_hidden_layers, Vec::new);
                    self.morph.axons.resize_with(self.net.num_hidden_layers, Vec::new);
                    self.morph.dendrites.resize_with(self.net.num_hidden_layers, Vec::new);
                }
            }
            // Interface matrices w_hh_fwd/bwd gain a new index when L increases: push placeholders
            if self.net.num_hidden_layers >= 2 {
                // When adding layer at end, we need a new interface between last-1 and last
                // Initialize empty; will be resized on first neuron spawn into the new layer
                self.w_hh_fwd.push(Array2::<f64>::zeros((0, self.layer_size(self.net.num_hidden_layers - 2))));
                self.w_hh_bwd.push(Array2::<f64>::zeros((self.layer_size(self.net.num_hidden_layers - 2), 0)));
                #[cfg(feature = "opencl")]
                {
                    self.cl_w_hh_fwd.push(None);
                    self.cl_w_hh_bwd.push(None);
                    self.cl_w_hh_fwd_sizes.push(0);
                    self.cl_w_hh_bwd_sizes.push(0);
                    self.cl_w_hh_fwd_dirty.push(true);
                    self.cl_w_hh_bwd_dirty.push(true);
                    self.cl_sparse_fwd.push(None);
                    self.cl_sparse_bwd.push(None);
                }
            }
        }
    }

    #[cfg(feature = "growth3d")]
    fn collect_growth_candidates(&mut self) {
        self.growth_queue.clear();
        // Global cooldown gate
        if self.last_global_growth_ms < self.net.global_growth_cooldown_ms.max(0.0) {
            return;
        }

        if self.is_at_max_neurons() {
            return;
        }

        let thr = self.net.saturation_threshold.max(0.0);
        let cooldown = self.net.growth_cooldown_ms.max(0.0);
        let max_layers = self.effective_max_layers();
        let num_hidden_layers = self.net.num_hidden_layers;
        // Limit to a single spawn per step globally to avoid bursts that can destabilize shapes early on
        let mut global_cap = 1usize;
        for l in 0..num_hidden_layers {
            if !self.is_layer_assigned(l) { continue; }
            if global_cap == 0 { break; }
            // one spawn per layer per step
            let num_current_layer_neurons = self.layer_size(l);
            let mut candidate: Option<usize> = None;
            for j in 0..num_current_layer_neurons {
                if self.rate_h[l][j] >= thr && self.since_growth_ms[l][j] >= cooldown {
                    candidate = Some(j); break;
                }
            }
            if let Some(pj) = candidate {
                // choose target layer (same or next) based on split threshold and limit
                let mut target_l = l;
                let size = num_current_layer_neurons;
                if size >= self.net.layer_split_threshold && self.net.num_hidden_layers < max_layers {
                    target_l = l + 1;
                }
                self.growth_queue.push(GrowthAction { layer: l, parent: pj, target_layer: target_l });
                global_cap = global_cap.saturating_sub(1);
            }
        }
    }

    #[cfg(feature = "growth3d")]
    fn apply_growth_queue(&mut self) -> bool {
        let actions = std::mem::take(&mut self.growth_queue);
        let mut did_spawn = false;
        
        let mut current_total = self.total_neurons() as u64;
        let max_neurons = self.net.max_total_neurons;

        for act in actions {
            if max_neurons > 0 && current_total >= max_neurons {
                continue;
            }

            if act.target_layer == act.layer {
                self.spawn_neuron_in_layer(act.layer, act.parent);
            } else {
                // ensure target layer exists, then spawn into that layer using parent for migration across interface
                if act.target_layer == self.net.num_hidden_layers {
                    self.ensure_layer_exists(act.target_layer);
                }
                self.spawn_neuron_into_next_layer(act.layer, act.parent);
            }
            did_spawn = true;
            current_total += 1;
        }
        if did_spawn {
            // reset global cooldown timer after any spawn
            self.last_global_growth_ms = 0.0;
        }
        did_spawn
    }

    #[cfg(feature = "growth3d")]
    fn allocate_region_and_type(&self, x: f32, y: f32, z: f32) -> (Option<String>, Option<String>) {
        // 1. Find best‑matching region by geometry (supports ellipsoid, torus, tube)
        let mut best_region = None;
        let mut best_metric = f32::MAX; // normalized distance; <1.0 means inside
        let mut have_inside = false;

        for region in &self.net.brain_regions {
            let (inside, metric) = match &region.shape {
                Some(crate::config::RegionShape::Ellipsoid { center, radii }) => {
                    let dx = x - center[0];
                    let dy = y - center[1];
                    let dz = z - center[2];
                    let q = (dx*dx)/(radii[0]*radii[0]).max(0.0) +
                            (dy*dy)/(radii[1]*radii[1]).max(0.0) +
                            (dz*dz)/(radii[2]*radii[2]).max(0.0);
                    (q <= 1.0, q.sqrt())
                }
                Some(crate::config::RegionShape::Torus { center, R, r, plane }) => {
                    // Default to torus around Y‑axis for plane "x-z"
                    let dx = x - center[0];
                    let dy = y - center[1];
                    let dz = z - center[2];
                    let (radial, orth) = if plane.as_str() == "x-z" { ((dx*dx + dz*dz).sqrt(), dy) } else { ((dy*dy + dz*dz).sqrt(), dx) };
                    let m = radial - *R;
                    let t = (m*m + orth*orth).sqrt();
                    let denom = if *r > 1e-6 { *r } else { 1e-6 };
                    (t <= *r, t / denom)
                }
                Some(crate::config::RegionShape::Tube { line_from, line_to, radius }) => {
                    // Distance from point to segment
                    let px = x - line_from[0];
                    let py = y - line_from[1];
                    let pz = z - line_from[2];
                    let vx = line_to[0] - line_from[0];
                    let vy = line_to[1] - line_from[1];
                    let vz = line_to[2] - line_from[2];
                    let v_len2 = vx*vx + vy*vy + vz*vz;
                    let mut t = 0.0f32;
                    if v_len2 > 1e-9 { t = (px*vx + py*vy + pz*vz) / v_len2; }
                    t = t.clamp(0.0, 1.0);
                    let cx = line_from[0] + vx * t;
                    let cy = line_from[1] + vy * t;
                    let cz = line_from[2] + vz * t;
                    let dx = x - cx;
                    let dy = y - cy;
                    let dz = z - cz;
                    let dist = (dx*dx + dy*dy + dz*dz).sqrt();
                    let denom = if *radius > 1e-6 { *radius } else { 1e-6 };
                    (dist <= *radius, dist / denom)
                }
                Some(crate::config::RegionShape::RepeatedEllipsoids { count, center_start, step, radii }) => {
                    let mut min_q = f32::MAX;
                    let mut any_inside = false;
                    for i in 0..*count {
                        let cx = center_start[0] + step[0] * i as f32;
                        let cy = center_start[1] + step[1] * i as f32;
                        let cz = center_start[2] + step[2] * i as f32;
                        let dx = x - cx;
                        let dy = y - cy;
                        let dz = z - cz;
                        let q = (dx*dx)/(radii[0]*radii[0]).max(1e-9) +
                                (dy*dy)/(radii[1]*radii[1]).max(1e-9) +
                                (dz*dz)/(radii[2]*radii[2]).max(1e-9);
                        if q <= 1.0 { any_inside = true; }
                        if q < min_q { min_q = q; }
                    }
                    (any_inside, min_q.sqrt())
                }
                None => {
                    // Legacy: treat as ellipsoid using center/radii
                    let dx = x - region.center[0];
                    let dy = y - region.center[1];
                    let dz = z - region.center[2];
                    let rx = region.radii[0].max(1e-6);
                    let ry = region.radii[1].max(1e-6);
                    let rz = region.radii[2].max(1e-6);
                    let q = (dx*dx)/(rx*rx) + (dy*dy)/(ry*ry) + (dz*dz)/(rz*rz);
                    (q <= 1.0, q.sqrt())
                }
            };

            if inside {
                if !have_inside || metric < best_metric {
                    have_inside = true;
                    best_metric = metric;
                    best_region = Some(region);
                }
            } else if !have_inside {
                if metric < best_metric {
                    best_metric = metric;
                    best_region = Some(region);
                }
            }
        }

        let region = best_region;
        let region_name = region.map(|r| r.name.clone());

        // 2. Allocate type based on distribution
        let type_name = if let Some(r) = region {
            if r.type_distribution.is_empty() {
                None
            } else {
                let total_weight: f32 = r.type_distribution.iter().map(|(_, w)| w).sum();
                if total_weight <= 0.0 {
                    None
                } else {
                    let mut r_val = fastrand::f32() * total_weight;
                    let mut chosen = None;
                    for (name, w) in &r.type_distribution {
                        if r_val <= *w {
                            chosen = Some(name.clone());
                            break;
                        }
                        r_val -= *w;
                    }
                    chosen.or_else(|| r.type_distribution.last().map(|(n, _)| n.clone()))
                }
            }
        } else {
            None
        };

        (region_name, type_name)
    }

    #[cfg(feature = "growth3d")]
    fn spawn_neuron_in_layer(&mut self, l: usize, parent_j: usize) {
        // Same-layer spawn generalized; delegate to l0 for l==0
        if l == 0 { self.spawn_neuron_l0(parent_j); return; }
        
        nm_log!("[trace] ENTER spawn_neuron_in_layer: l={}, parent_j={}", l, parent_j);
        let (in_l, out_l) = self.get_io_layers();
        let num_sensory_neurons = self.net.num_sensory_neurons;

        // Incoming: from layer l-1 via w_hh_fwd[l-1] rows
        let num_previous_layer_neurons = self.layer_size(l-1);
        let num_old_layer_neurons = self.layer_size(l);
        let num_new_layer_neurons = num_old_layer_neurons + 1;
        let j_new = num_old_layer_neurons;

        // 1) Update topology
        let (px, py, pz) = if let Some(prev_layer) = self.topo.layers.get(l-1) {
            if parent_j < prev_layer.len() { (prev_layer[parent_j].x, prev_layer[parent_j].y, prev_layer[parent_j].z) } else { (0.0, 0.0, 0.0) }
        } else { (0.0, 0.0, 0.0) };
        let (nx, ny, nz) = self.place_node_near(l, (px, py, pz));
        let (region_name, type_name) = self.allocate_region_and_type(nx, ny, nz);
        self.topo.add_neuron(l, Node3D{ x:nx, y:ny, z:nz, layer:l, region_name: region_name.clone(), type_name: type_name.clone() });

        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology {
                let pos = crate::morphology::Point3 { x: nx, y: ny, z: nz };
                let start_empty = matches!(self.neuron_model, NeuronModel::Aarnn);
                self.morph.add_hidden_neuron(l, j_new, pos, self.net.synapse_offset, start_empty, region_name, type_name.clone());
            }
        }

        // 2) Grow per-layer state vectors
        self.v_h[l] = Self::append_val(&self.v_h[l], 0.0);
        
        let bio = if let Some(tname) = type_name.as_ref() {
            self.net.neuron_types.iter().find(|t| &t.name == tname).map(|t| t.bio_params.clone()).unwrap_or(self.net.aarnn_bio.clone())
        } else {
            self.net.aarnn_bio.clone()
        };
        self.bio_h[l].push(bio);

        self.ensure_state_dimensions();

        // 3) Resize Matrices
        // If this layer is the sensory target, resize w_in rows
        if l == in_l {
            let mut new_w_in = Array2::<f64>::zeros((num_new_layer_neurons, num_sensory_neurons));
            // handle potential row count mismatch
            let rows_to_copy = num_old_layer_neurons.min(self.w_in.nrows());
            let cols_to_copy = num_sensory_neurons.min(self.w_in.ncols());
            for j in 0..rows_to_copy {
                for i in 0..cols_to_copy {
                    let val = self.w_in.get((j, i)).copied().unwrap_or_else(|| {
                        0.0
                    });
                    if let Some(cell) = new_w_in.get_mut((j, i)) {
                        *cell = val;
                    }
                }
            }
            let mut migrated_in = 0;
            for i in 0..num_sensory_neurons {
                if parent_j < self.w_in.nrows() && i < self.w_in.ncols() {
                    let w_old = self.w_in.get((parent_j, i)).copied().unwrap_or(0.0);
                    let current_count = self.sensory_connection_count(i);
                    if current_count < 6 && fastrand::f32() < self.net.migrate_in_prob.clamp(0.0, 1.0) {
                        let a = 0.4 + 0.2*fastrand::f32();
                        let w_new = (a as f64) * w_old;
                        let w_par = ((1.0 - a as f64) * w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                        if let Some(cell) = new_w_in.get_mut((parent_j, i)) { *cell = w_par; }
                        if let Some(cell) = new_w_in.get_mut((j_new, i)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                        migrated_in += 1;
                    } else {
                        let orig = self.w_in.get((parent_j, i)).copied().unwrap_or(0.0);
                        if let Some(cell) = new_w_in.get_mut((parent_j, i)) { *cell = orig; }
                        let val = if current_count < 6 && fastrand::f32() < self.net.p_in as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                        if let Some(cell) = new_w_in.get_mut((j_new, i)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                    }
                }
            }
            if migrated_in > 0 {
                if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                    nm_log!("[trace] {} input synapses migrated from hidden {}:{} to new hidden {}:{}", migrated_in, l, parent_j, l, j_new);
                }
            }
            self.w_in = new_w_in;
        }

        // Resize incoming interface from l-1: add a row to w_hh_fwd[l-1] and a column to w_hh_bwd[l-1]
        let mut new_fwd = Array2::<f64>::zeros((num_new_layer_neurons, num_previous_layer_neurons));
        for j in 0..num_old_layer_neurons { for i in 0..num_previous_layer_neurons {
            let val = self.w_hh_fwd[l-1].get((j,i)).copied().unwrap_or(0.0);
            if let Some(cell) = new_fwd.get_mut((j,i)) {
                *cell = val;
            }
        } }
        let mut new_bwd = Array2::<f64>::zeros((num_previous_layer_neurons, num_new_layer_neurons));
        for i in 0..num_previous_layer_neurons { for j in 0..num_old_layer_neurons {
            let val = self.w_hh_bwd[l-1].get((i,j)).copied().unwrap_or(0.0);
            if let Some(cell) = new_bwd.get_mut((i,j)) {
                *cell = val;
            }
        } }
        let mut migrated_h_in = 0;
        for i in 0..num_previous_layer_neurons {
            let w_old = self.w_hh_fwd[l-1].get((parent_j, i)).copied().unwrap_or(0.0);
            if fastrand::f32() < self.net.migrate_in_prob.clamp(0.0,1.0) {
                let a = 0.4 + 0.2*fastrand::f32();
                let w_new = (a as f64) * w_old;
                let w_par = ((1.0 - a as f64) * w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                if let Some(cell) = new_fwd.get_mut((parent_j,i)) { *cell = w_par; }
                if let Some(cell) = new_fwd.get_mut((j_new,i)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                if let Some(cell) = new_bwd.get_mut((i,parent_j)) { *cell = w_par; }
                if let Some(cell) = new_bwd.get_mut((i,j_new)) { *cell = new_fwd.get((j_new,i)).copied().unwrap_or(0.0); }
                migrated_h_in += 1;
            } else {
                if let Some(cell) = new_fwd.get_mut((parent_j,i)) {
                    *cell = self.w_hh_fwd[l-1].get((parent_j,i)).copied().unwrap_or(0.0);
                }
                let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                if let Some(cell) = new_fwd.get_mut((j_new,i)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                if let Some(cell) = new_bwd.get_mut((i,parent_j)) {
                    *cell = self.w_hh_bwd[l-1].get((i,parent_j)).copied().unwrap_or(0.0);
                }
                if let Some(cell) = new_bwd.get_mut((i,j_new)) {
                    *cell = new_fwd.get((j_new,i)).copied().unwrap_or(0.0);
                }
            }
        }
        if migrated_h_in > 0 {
            if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                nm_log!("[trace] {} incoming hidden synapses migrated from hidden {}:{} to new hidden {}:{}", migrated_h_in, l, parent_j, l, j_new);
            }
        }
        self.w_hh_fwd[l-1] = new_fwd;
        self.w_hh_bwd[l-1] = new_bwd;
        // Outgoing: to l+1 or output if this layer is the source for output
        if l == out_l {
            // source for output: add column to w_out
            let num_o_neurons = self.net.num_output_neurons;
            let mut new_w_out = Array2::<f64>::zeros((num_o_neurons, num_new_layer_neurons));
            // Robust copy: handle potential col count mismatch
            let rows_to_copy = num_o_neurons.min(self.w_out.nrows());
            let cols_to_copy = num_old_layer_neurons.min(self.w_out.ncols());
            for k in 0..rows_to_copy { for j in 0..cols_to_copy {
                let val = self.w_out.get((k, j)).copied().unwrap_or_else(|| {
                    nm_log!("[error] Out of bounds: w_out[({}, {})], shape={:?}", k, j, self.w_out.dim());
                    0.0
                });
                if let Some(cell) = new_w_out.get_mut((k, j)) {
                    *cell = val;
                } else {
                    nm_log!("[error] Out of bounds: new_w_out[({}, {})], shape={:?}", k, j, new_w_out.dim());
                }
            } }
            let mut migrated_out = 0;
            for k in 0..num_o_neurons {
                if k < self.w_out.nrows() && parent_j < self.w_out.ncols() {
                    let w_old = self.w_out.get((k, parent_j)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_out[({}, {})], shape={:?}", k, parent_j, self.w_out.dim());
                        0.0
                    });
                    if fastrand::f32() < self.net.migrate_out_prob.clamp(0.0,1.0) {
                        let b = 0.4 + 0.2*fastrand::f32();
                        let w_new = (b as f64)*w_old; let w_par = ((1.0 - b as f64)*w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                        if let Some(cell) = new_w_out.get_mut((k, parent_j)) { *cell = w_par; }
                        if let Some(cell) = new_w_out.get_mut((k, j_new)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                        migrated_out += 1;
                    } else {
                        let orig = self.w_out.get((k, parent_j)).copied().unwrap_or(0.0);
                        if let Some(cell) = new_w_out.get_mut((k, parent_j)) { *cell = orig; }
                        let val = if fastrand::f32() < self.net.p_out as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                        if let Some(cell) = new_w_out.get_mut((k, j_new)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                    }
                }
            }
            if migrated_out > 0 {
                if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                    nm_log!("[trace] {} output synapses migrated from hidden {}:{} to new hidden {}:{}", migrated_out, l, parent_j, l, j_new);
                }
            }
            self.w_out = new_w_out;
        }
        
        if l < self.net.num_hidden_layers - 1 {
            // inner layer: add column to w_hh_fwd[l] and row to w_hh_bwd[l]
            let num_next_layer_neurons = self.layer_size(l+1);
            let mut new_fwd_next = Array2::<f64>::zeros((num_next_layer_neurons, num_new_layer_neurons));
            for j in 0..num_next_layer_neurons {
                for i in 0..num_old_layer_neurons {
                    let val = self.w_hh_fwd[l].get((j,i)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_hh_fwd[{}][({}, {})], shape={:?}", l, j, i, self.w_hh_fwd[l].dim());
                        0.0
                    });
                    if let Some(cell) = new_fwd_next.get_mut((j,i)) {
                        *cell = val;
                    } else {
                        nm_log!("[error] Out of bounds: new_fwd_next[({}, {})], shape={:?}", j, i, new_fwd_next.dim());
                    }
                }
            }
            let mut new_bwd_next = Array2::<f64>::zeros((num_new_layer_neurons, num_next_layer_neurons));
            for i in 0..num_old_layer_neurons {
                for j in 0..num_next_layer_neurons {
                    let val = self.w_hh_bwd[l].get((i,j)).copied().unwrap_or_else(|| {
                        nm_log!("[error] Out of bounds: w_hh_bwd[{}][({}, {})], shape={:?}", l, i, j, self.w_hh_bwd[l].dim());
                        0.0
                    });
                    if let Some(cell) = new_bwd_next.get_mut((i,j)) {
                        *cell = val;
                    } else {
                        nm_log!("[error] Out of bounds: new_bwd_next[({}, {})], shape={:?}", i, j, new_bwd_next.dim());
                    }
                }
            }
            // migrate outgoing from parent to new neuron across interface to next layer
            let mut migrated_out = 0;
            for j in 0..num_next_layer_neurons {
                let w_old = self.w_hh_fwd[l].get((j, parent_j)).copied().unwrap_or_else(|| {
                    nm_log!("[error] Out of bounds: w_hh_fwd[{}][({}, {})], shape={:?}", l, j, parent_j, self.w_hh_fwd[l].dim());
                    0.0
                });
                if fastrand::f32() < self.net.migrate_out_prob.clamp(0.0,1.0) {
                    let b = 0.4 + 0.2*fastrand::f32();
                    let w_new = (b as f64)*w_old; let w_par = ((1.0 - b as f64)*w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                    if let Some(cell) = new_fwd_next.get_mut((j,parent_j)) { *cell = w_par; }
                    if let Some(cell) = new_fwd_next.get_mut((j,j_new)) { *cell = w_new.clamp(self.stdp.w_min, self.stdp.w_max); }
                    if let Some(cell) = new_bwd_next.get_mut((parent_j,j)) { *cell = w_par; }
                    if let Some(cell) = new_bwd_next.get_mut((j_new,j)) { *cell = new_fwd_next.get((j,j_new)).copied().unwrap_or(0.0); }
                    migrated_out += 1;
                } else {
                    if let Some(cell) = new_fwd_next.get_mut((j,parent_j)) {
                        *cell = self.w_hh_fwd[l].get((j,parent_j)).copied().unwrap_or(0.0);
                    }
                    let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                    if let Some(cell) = new_fwd_next.get_mut((j,j_new)) { *cell = val.clamp(self.stdp.w_min, self.stdp.w_max); }
                    if let Some(cell) = new_bwd_next.get_mut((parent_j,j)) {
                        *cell = self.w_hh_bwd[l].get((parent_j,j)).copied().unwrap_or(0.0);
                    }
                    if let Some(cell) = new_bwd_next.get_mut((j_new,j)) {
                        *cell = new_fwd_next.get((j,j_new)).copied().unwrap_or(0.0);
                    }
                }
            }
            if migrated_out > 0 {
                if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                    nm_log!("[trace] {} outgoing synapses migrated from hidden {}:{} to new hidden {}:{}", migrated_out, l, parent_j, l, j_new);
                }
            }
            self.w_hh_fwd[l] = new_fwd_next;
            self.w_hh_bwd[l] = new_bwd_next;
        }

        // Proximity-biased extra incoming edges from layer l-1 (bounded degree)
        if l > 0 {
            let prev_nodes = self.topo.layers.get(l-1).cloned().unwrap_or_default();
            let nodes_l = self.topo.layers.get(l).cloned().unwrap_or_default();
            let j_new = num_new_layer_neurons - 1;
            let degree_cap = self.net.proximity_degree_cap.max(0);
            let mut added = 0usize;
            // Create a vector of (i, dist) pairs
            let mut cand: Vec<(usize, f32)> = (0..num_previous_layer_neurons).map(|i|{
                let (ax,ay,az) = if i < prev_nodes.len() { (prev_nodes[i].x, prev_nodes[i].y, prev_nodes[i].z) } else { (0.0,0.0,0.0) };
                let (bx,by,bz) = if j_new < nodes_l.len() { (nodes_l[j_new].x, nodes_l[j_new].y, nodes_l[j_new].z) } else { (0.0,0.0,0.0) };
                let dx = ax-bx; let dy = ay-by; let dz = az-bz; (i, (dx*dx+dy*dy+dz*dz).sqrt())
            }).collect();
            cand.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            // mutate matrices we just set up
            let fwd = &mut self.w_hh_fwd[l-1];
            let bwd = &mut self.w_hh_bwd[l-1];
            for (i,_d) in cand {
                if added >= degree_cap { break; }
                if fastrand::f32() < self.net.new_edge_prob {
                    // only if currently near-zero
                    let fwd_val = fwd.get((j_new, i)).copied().unwrap_or(0.0);
                    if fwd_val.abs() < 1e-12 {
                        let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                        let v = val.clamp(self.stdp.w_min, self.stdp.w_max);
                        if let Some(cell) = fwd.get_mut((j_new, i)) {
                            *cell = v;
                        } else {
                            nm_log!("[error] Out of bounds: fwd[({}, {})], shape={:?}", j_new, i, fwd.dim());
                            continue;
                        }
                        if let Some(cell) = bwd.get_mut((i, j_new)) {
                            *cell = v;
                        } else {
                            nm_log!("[error] Out of bounds: bwd[({}, {})], shape={:?}", i, j_new, bwd.dim());
                            continue;
                        }
                        if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                            nm_log!("[trace] synapse made: hidden {}:{} -> hidden {}:{} - proximity-biased edge on spawn", l-1, i, l, j_new);
                        }
                        added += 1;
                    }
                }
            }
        }

        // Resize w_hh_rec[l] with bounds checks and logging
        let mut new_rec = Array2::<f64>::zeros((num_new_layer_neurons, num_new_layer_neurons));
        for j in 0..num_old_layer_neurons {
            for i in 0..num_old_layer_neurons {
                let val = self.w_hh_rec[l].get((j, i)).copied().unwrap_or_else(|| {
                    nm_log!("[error] Out of bounds: w_hh_rec[{}][({}, {})], shape={:?}", l, j, i, self.w_hh_rec[l].dim());
                    0.0
                });
                if let Some(cell) = new_rec.get_mut((j, i)) {
                    *cell = val;
                } else {
                    nm_log!("[error] Out of bounds: new_rec[({}, {})], shape={:?}", j, i, new_rec.dim());
                }
            }
        }
        let aarnn_active = matches!(self.neuron_model, NeuronModel::Aarnn);
        let rec_p = self.net.p_hidden.clamp(0.0, 1.0) as f32;
        for i in 0..num_old_layer_neurons {
            let v1 = self.w_hh_rec[l].get((parent_j, i)).copied().unwrap_or_else(|| {
                nm_log!("[error] Out of bounds: w_hh_rec[{}][({}, {})], shape={:?}", l, parent_j, i, self.w_hh_rec[l].dim());
                0.0
            });
            let v2 = self.w_hh_rec[l].get((i, parent_j)).copied().unwrap_or_else(|| {
                nm_log!("[error] Out of bounds: w_hh_rec[{}][({}, {})], shape={:?}", l, i, parent_j, self.w_hh_rec[l].dim());
                0.0
            });
            if let Some(cell) = new_rec.get_mut((j_new, i)) {
                *cell = if aarnn_active && fastrand::f32() >= rec_p { 0.0 } else { v1 };
            }
            if let Some(cell) = new_rec.get_mut((i, j_new)) {
                *cell = if aarnn_active && fastrand::f32() >= rec_p { 0.0 } else { v2 };
            }
        }
        let v3 = self.w_hh_rec[l].get((parent_j, parent_j)).copied().unwrap_or_else(|| {
            nm_log!("[error] Out of bounds: w_hh_rec[{}][({}, {})], shape={:?}", l, parent_j, parent_j, self.w_hh_rec[l].dim());
            0.0
        });
        if let Some(cell) = new_rec.get_mut((j_new, j_new)) {
            *cell = if aarnn_active && fastrand::f32() >= rec_p { 0.0 } else { v3 };
        }
        self.w_hh_rec[l] = new_rec;
        self.sync_presence_sizes();
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if self.net.use_morphology {
            self.rebuild_syn_maps_from_morph();
        }
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();
    }

    #[cfg(feature = "growth3d")]
    fn spawn_neuron_into_next_layer(&mut self, l: usize, parent_j: usize) {
        // Add a neuron to layer l+1, migrating a portion of parent_j's outgoing weights to it as incoming from l
        let target = l + 1;
        if target >= self.effective_max_layers() {
            return;
        }
        self.ensure_layer_exists(target);
        let (in_l, out_l) = self.get_io_layers();
        
        let num_sensory_neurons = self.net.num_sensory_neurons;
        let num_previous_layer_neurons = self.layer_size(l); // sends into new neuron
        let num_old_next_layer_neurons = self.layer_size(target);
        let num_new_next_layer_neurons = num_old_next_layer_neurons + 1;
        // Topology: place near parent in next column with minimum separation
        let (px,py,pz) = if let Some(layer) = self.topo.layers.get(l) {
            if parent_j < layer.len() {
                (layer[parent_j].x, layer[parent_j].y, layer[parent_j].z)
            } else { (0.0, 0.0, 0.0) }
        } else if l == 0 && parent_j < self.topo.sensory_nodes.len() {
             (self.topo.sensory_nodes[parent_j].x, self.topo.sensory_nodes[parent_j].y, self.topo.sensory_nodes[parent_j].z)
        } else { (0.0, 0.0, 0.0) };
        let (nx,ny,nz) = self.place_node_near(target, (px,py,pz));
        let (region_name, type_name) = self.allocate_region_and_type(nx, ny, nz);
        self.topo.add_neuron(target, Node3D { x: nx, y: ny, z: nz, layer: target, region_name: region_name.clone(), type_name: type_name.clone() });

        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology {
                let pos = crate::morphology::Point3 { x: nx, y: ny, z: nz };
                let start_empty = matches!(self.neuron_model, NeuronModel::Aarnn);
                self.morph.add_hidden_neuron(target, num_old_next_layer_neurons, pos, self.net.synapse_offset, start_empty, region_name, type_name.clone());
            }
        }
        // Grow per-neuron vectors for target layer
        self.v_h[target] = Self::append_val(&self.v_h[target], 0.0);
        
        let bio = if let Some(tname) = type_name.as_ref() {
            self.net.neuron_types.iter().find(|t| &t.name == tname).map(|t| t.bio_params.clone()).unwrap_or(self.net.aarnn_bio.clone())
        } else {
            self.net.aarnn_bio.clone()
        };
        self.bio_h[target].push(bio);

        self.ensure_state_dimensions();

        // Start rate/cooldown based on parent layer dynamics
        let seed_rate = if l < self.rate_h.len() && parent_j < self.rate_h[l].len() { self.rate_h[l][parent_j]*0.25 } else { 0.0 };
        if let Some(r) = self.rate_h[target].get_mut(num_old_next_layer_neurons) { *r = seed_rate; }

        // If target layer is the sensory target, resize w_in rows
        if target == in_l {
            let mut new_w_in = Array2::<f64>::zeros((num_new_next_layer_neurons, num_sensory_neurons));
            let rows_to_copy = num_old_next_layer_neurons.min(self.w_in.nrows());
            let cols_to_copy = num_sensory_neurons.min(self.w_in.ncols());
            for j in 0..rows_to_copy {
                for i in 0..cols_to_copy {
                    if let (Some(cell), Some(val)) = (new_w_in.get_mut((j, i)), self.w_in.get((j, i))) {
                        *cell = *val;
                    } else {
                        nm_log!("[warn] new_w_in copy out of bounds: ({}, {})", j, i);
                    }
                }
            }
            // newly spawned into this layer starts with small random init for w_in if needed
            let j_new = num_old_next_layer_neurons;
            for i in 0..num_sensory_neurons {
                let current_count = self.sensory_connection_count(i);
                let val = if current_count < 6 && fastrand::f32() < self.net.p_in as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                new_w_in[(j_new, i)] = val.clamp(self.stdp.w_min, self.stdp.w_max);
            }
            self.w_in = new_w_in;
        }

        // Resize interface matrices between l and target (l)
        let mut new_fwd = Array2::<f64>::zeros((num_new_next_layer_neurons, num_previous_layer_neurons));
        // Previous fwd rows reside in self.w_hh_fwd[l]; copy into rows 0..num_old_next_layer_neurons
        for j in 0..num_old_next_layer_neurons { for i in 0..num_previous_layer_neurons {
            let val = self.w_hh_fwd[l].get((j,i)).copied().unwrap_or_else(|| {
                nm_log!("[error] Out of bounds: w_hh_fwd[{}][({}, {})], shape={:?}", l, j, i, self.w_hh_fwd[l].dim());
                0.0
            });
            if let Some(cell) = new_fwd.get_mut((j,i)) {
                *cell = val;
            } else {
                nm_log!("[warn] new_fwd copy out of bounds: ({}, {})", j, i);
            }
        } }
        let mut new_bwd = Array2::<f64>::zeros((num_previous_layer_neurons, num_new_next_layer_neurons));
        for i in 0..num_previous_layer_neurons { for j in 0..num_old_next_layer_neurons {
            let val = self.w_hh_bwd[l].get((i,j)).copied().unwrap_or_else(|| {
                nm_log!("[error] Out of bounds: w_hh_bwd[{}][({}, {})], shape={:?}", l, i, j, self.w_hh_bwd[l].dim());
                0.0
            });
            if let Some(cell) = new_bwd.get_mut((i,j)) {
                *cell = val;
            } else {
                nm_log!("[warn] new_bwd copy out of bounds: ({}, {})", i, j);
            }
        } }
        let j_new_next = num_old_next_layer_neurons;
        // Incoming weights for new neuron come from layer l columns
        let mut migrated_in = 0;
        for i in 0..num_previous_layer_neurons {
            if num_old_next_layer_neurons > 0 {
                let src_row = parent_j.min(num_old_next_layer_neurons - 1);
                let w_old = self.w_hh_fwd[l][(src_row, i)];
                if fastrand::f32() < self.net.migrate_in_prob.clamp(0.0,1.0) {
                    let a = 0.4 + 0.2*fastrand::f32();
                    let w_new = (a as f64) * w_old; // to new receiver
                    // parent row unchanged here (stays with original), optional: damp a bit
                    new_fwd[(j_new_next,i)] = w_new.clamp(self.stdp.w_min, self.stdp.w_max);
                    new_bwd[(i,j_new_next)] = new_fwd[(j_new_next,i)];
                    migrated_in += 1;
                } else {
                    let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                    new_fwd[(j_new_next,i)] = val.clamp(self.stdp.w_min, self.stdp.w_max);
                    new_bwd[(i,j_new_next)] = new_fwd[(j_new_next,i)];
                }
            } else {
                // No existing target rows yet; initialize from small random
                let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                new_fwd[(j_new_next,i)] = val.clamp(self.stdp.w_min, self.stdp.w_max);
                new_bwd[(i,j_new_next)] = new_fwd[(j_new_next,i)];
            }
        }
        if migrated_in > 0 {
            if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                nm_log!("[trace] {} incoming synapses migrated from hidden {}:{} to new hidden {}:{}", migrated_in, l, parent_j, target, j_new_next);
            }
        }
        self.w_hh_fwd[l] = new_fwd;
        self.w_hh_bwd[l] = new_bwd;

        // Resize interface to next layer if it exists
        if target < self.net.num_hidden_layers - 1 {
            let num_next = self.layer_size(target + 1);
            let num_old = num_old_next_layer_neurons;
            let num_new = num_new_next_layer_neurons;
            
            // target is sender for w_hh_fwd[target] (num_next x target)
            let mut new_fwd_next = Array2::<f64>::zeros((num_next, num_new));
            if num_old > 0 {
                for j in 0..num_next {
                    for i in 0..num_old { new_fwd_next[(j, i)] = self.w_hh_fwd[target][(j, i)]; }
                }
                // Initialize outgoing from new neuron
                for j in 0..num_next {
                    let w_old = self.w_hh_fwd[target][(j, parent_j.min(num_old.saturating_sub(1)))];
                    if fastrand::f32() < self.net.migrate_out_prob.clamp(0.0, 1.0) {
                        let beta = 0.4 + 0.2 * fastrand::f32();
                        let w_new = (beta as f64) * w_old;
                        let w_par = ((1.0 - beta as f64) * w_old).clamp(self.stdp.w_min, self.stdp.w_max);
                        new_fwd_next[(j, parent_j.min(num_old.saturating_sub(1)))] = w_par;
                        new_fwd_next[(j, num_new - 1)] = w_new.clamp(self.stdp.w_min, self.stdp.w_max);
                    } else {
                        new_fwd_next[(j, num_new - 1)] = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64() * 0.2 + 0.05 } else { 0.0 };
                    }
                }
            } else {
                for j in 0..num_next {
                    new_fwd_next[(j, num_new - 1)] = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64() * 0.2 + 0.05 } else { 0.0 };
                }
            }
            self.w_hh_fwd[target] = new_fwd_next;

            // target is receiver for w_hh_bwd[target] (target x num_next)
            let mut new_bwd_next = Array2::<f64>::zeros((num_new, num_next));
            if num_old > 0 {
                for i in 0..num_old {
                    for j in 0..num_next { new_bwd_next[(i, j)] = self.w_hh_bwd[target][(i, j)]; }
                }
                // Copy parent backward weights to new neuron
                for j in 0..num_next { 
                    new_bwd_next[(num_new - 1, j)] = self.w_hh_bwd[target][(parent_j.min(num_old.saturating_sub(1)), j)]; 
                }
            } else {
                for j in 0..num_next {
                    new_bwd_next[(num_new - 1, j)] = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64() * 0.2 + 0.05 } else { 0.0 };
                }
            }
            self.w_hh_bwd[target] = new_bwd_next;
        }

        // If target is now the output source layer, need to add a column to w_out
        if target == out_l {
            // ensure w_out has column count equal to num_next_layer_neurons
            let num_output_neurons = self.net.num_output_neurons; 
            let old_cols = self.w_out.ncols(); 
            let need_cols = num_new_next_layer_neurons;
            if need_cols > old_cols {
                let mut nw = Array2::<f64>::zeros((num_output_neurons, need_cols));
                let rows_to_copy = num_output_neurons.min(self.w_out.nrows());
                let cols_to_copy = old_cols.min(self.w_out.ncols());
                for k in 0..rows_to_copy { for j in 0..cols_to_copy { nw[(k,j)] = self.w_out[(k,j)]; } }
                // init new column small random
                let mut added_out = 0;
                for k in 0..num_output_neurons {
                    let val = if fastrand::f32() < self.net.p_out as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                    let w = val.clamp(self.stdp.w_min, self.stdp.w_max);
                    nw[(k, need_cols-1)] = w;
                    if w > 0.0 { added_out += 1; }
                }
                if added_out > 0 {
                    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                        nm_log!("[trace] {} output synapses initialized for new hidden {}:{}", added_out, target, need_cols-1);
                    }
                }
                self.w_out = nw;
            }
        }
        // Reset parent cooldown and damp rate
        if parent_j < self.since_growth_ms[l].len() { self.since_growth_ms[l][parent_j] = 0.0; }
        if parent_j < self.rate_h[l].len() { self.rate_h[l][parent_j] *= 0.5; }
        // Extra safety: reset cooldown across involved layers to avoid immediate re-triggers in the same neighborhood
        if l < self.since_growth_ms.len() { self.since_growth_ms[l].fill(0.0); }
        if target < self.since_growth_ms.len() { self.since_growth_ms[target].fill(0.0); }

        // Proximity-biased extra incoming edges from layer l into the new neuron in target layer
        let prev_nodes = self.topo.layers.get(l).cloned().unwrap_or_default();
        let nodes_target = self.topo.layers.get(target).cloned().unwrap_or_default();
        let j_new = num_new_next_layer_neurons - 1;
        let degree_cap = self.net.proximity_degree_cap.max(0);
        let mut added = 0usize;
        let mut cand: Vec<(usize, f32)> = (0..num_previous_layer_neurons).map(|i|{
            let (ax,ay,az) = if i < prev_nodes.len() { (prev_nodes[i].x, prev_nodes[i].y, prev_nodes[i].z) } else { (0.0,0.0,0.0) };
            let (bx,by,bz) = if j_new < nodes_target.len() { (nodes_target[j_new].x, nodes_target[j_new].y, nodes_target[j_new].z) } else { (0.0,0.0,0.0) };
            let dx=ax-bx; let dy=ay-by; let dz=az-bz; (i, (dx*dx+dy*dy+dz*dz).sqrt())
        }).collect();
        cand.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let fwd = &mut self.w_hh_fwd[l];
        let bwd = &mut self.w_hh_bwd[l];
        for (i,_d) in cand {
            if added >= degree_cap { break; }
            if fastrand::f32() < self.net.new_edge_prob {
                if fwd[(j_new, i)].abs() < 1e-12 {
                    let val = if fastrand::f32() < self.net.p_hidden as f32 { fastrand::f64()*0.2+0.05 } else { 0.0 };
                    let v = val.clamp(self.stdp.w_min, self.stdp.w_max);
                    fwd[(j_new, i)] = v;
                    bwd[(i, j_new)] = v;
                    if std::env::var("NM_TRACE").ok().as_deref() == Some("1") {
                        nm_log!("[trace] synapse made: hidden {}:{} -> hidden {}:{} - proximity-biased edge on spawn", l, i, target, j_new);
                    }
                    added += 1;
                }
            }
        }

        // Resize w_hh_rec[target]
        let mut new_rec = Array2::<f64>::zeros((num_new_next_layer_neurons, num_new_next_layer_neurons));
        if num_old_next_layer_neurons > 0 {
            for j in 0..num_old_next_layer_neurons {
                for i in 0..num_old_next_layer_neurons {
                    new_rec[(j, i)] = self.w_hh_rec[target][(j, i)];
                }
            }
            // Initialize new neuron recurrent connections from parent? 
            // For splitting, might make sense to copy some recurrent connections.
            for i in 0..num_old_next_layer_neurons {
                let src_j = parent_j.min(num_old_next_layer_neurons.saturating_sub(1));
                new_rec[(j_new, i)] = self.w_hh_rec[target][(src_j, i)];
                new_rec[(i, j_new)] = self.w_hh_rec[target][(i, src_j)];
            }
            let src_j = parent_j.min(num_old_next_layer_neurons.saturating_sub(1));
            new_rec[(j_new, j_new)] = self.w_hh_rec[target][(src_j, src_j)];
        } else {
            // New layer, no parent recurrent to copy.
            new_rec[(j_new, j_new)] = 0.0;
        }
        self.w_hh_rec[target] = new_rec;
        self.sync_presence_sizes();
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if self.net.use_morphology {
            self.rebuild_syn_maps_from_morph();
        }
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    fn meets_reassignment_criteria(&self, l: usize, j: usize) -> bool {
        let mut has_longterm_dendrite = false;
        let mut has_backprop_axon = false;

        for syn in &self.morph.synapses {
            // Check if neuron (l, j) is the receiver (dendrite side)
            if syn.post_layer == l as isize && syn.post_id == j {
                let is_lt = match syn.kind {
                    crate::morphology::SynKind::In => self.is_longterm_in(j, syn.pre_id),
                    crate::morphology::SynKind::HiddenFwd => self.is_longterm_fwd(syn.pre_layer as usize, j, syn.pre_id),
                    crate::morphology::SynKind::HiddenBwd => self.is_longterm_bwd(l, j, syn.pre_id),
                    crate::morphology::SynKind::HiddenRec => self.is_longterm_rec(l, j, syn.pre_id),
                    _ => false,
                };
                if is_lt {
                    has_longterm_dendrite = true;
                }
            }

            // Check if neuron (l, j) is the sender (axon side)
            if syn.pre_layer == l as isize && syn.pre_id == j {
                // Criteria: none of its axon boutons are part of any backpropagation connections to an earlier hidden layer.
                if syn.kind == crate::morphology::SynKind::HiddenBwd && syn.post_layer < l as isize {
                    has_backprop_axon = true;
                }
            }
        }

        has_longterm_dendrite && !has_backprop_axon
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    pub fn reassign_neurons_to_next_layer(&mut self) {
        if !self.net.growth_enabled { return; }
        if matches!(self.neuron_model, NeuronModel::Aarnn) {
            return;
        }
        if !self.net.use_morphology { return; }
        let num_layers = self.net.num_hidden_layers;
        let mut to_move = Vec::new();

        for l in 0..num_layers {
            let n = self.layer_size(l);
            for j in 0..n {
                if self.meets_reassignment_criteria(l, j) {
                    to_move.push((l, j));
                }
            }
        }

        if to_move.is_empty() { return; }

        // Sort by layer descending, then index descending to avoid index shift issues
        to_move.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

        for (l, j) in to_move {
            self.move_neuron_to_next_layer(l, j);
        }
        
        self.sync_presence_sizes();
        self.rebuild_syn_maps_from_morph();
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    fn move_neuron_to_next_layer(&mut self, l: usize, j: usize) {
        let target_l = l + 1;
        if target_l >= self.effective_max_layers() {
            return;
        }
        self.ensure_layer_exists(target_l);
        let new_j = self.layer_size(target_l);

        nm_log!("[growth] Reassigning neuron {}:{} to next layer {}:{}", l, j, target_l, new_j);

        // 1. Move state vectors
        let v = self.v_h[l][j]; self.v_h[l] = Self::remove_idx(&self.v_h[l], j);
        self.v_h[target_l] = Self::append_val(&self.v_h[target_l], v);

        if let Some(ref mut rfh) = self.refr_h {
            let r = rfh[l][j]; rfh[l] = Self::remove_idx(&rfh[l], j);
            rfh[target_l] = Self::append_val(&rfh[target_l], r);
        }
        if let Some(ref mut uh) = self.u_h {
            let u = uh[l][j]; uh[l] = Self::remove_idx(&uh[l], j);
            uh[target_l] = Self::append_val(&uh[target_l], u);
        }
        let xp = self.x_post_h[l][j]; self.x_post_h[l] = Self::remove_idx(&self.x_post_h[l], j);
        self.x_post_h[target_l] = Self::append_val(&self.x_post_h[target_l], xp);

        let xpr = self.x_pre_h[l][j]; self.x_pre_h[l] = Self::remove_idx(&self.x_pre_h[l], j);
        self.x_pre_h[target_l] = Self::append_val(&self.x_pre_h[target_l], xpr);

        let ls = self.last_spk_h[l][j]; self.last_spk_h[l] = Self::remove_idx(&self.last_spk_h[l], j);
        self.last_spk_h[target_l] = Self::append_val(&self.last_spk_h[target_l], ls);

        let rt = self.rate_h[l][j]; self.rate_h[l] = Self::remove_idx(&self.rate_h[l], j);
        self.rate_h[target_l] = Self::append_val(&self.rate_h[target_l], rt);

        let sg = self.since_growth_ms[l][j]; self.since_growth_ms[l] = Self::remove_idx(&self.since_growth_ms[l], j);
        self.since_growth_ms[target_l] = Self::append_val(&self.since_growth_ms[target_l], sg);

        let slb = self.since_last_bouton_ms[l][j]; self.since_last_bouton_ms[l] = Self::remove_idx(&self.since_last_bouton_ms[l], j);
        self.since_last_bouton_ms[target_l] = Self::append_val(&self.since_last_bouton_ms[target_l], slb);

        let sa = self.syn_ampa_h[l][j]; self.syn_ampa_h[l] = Self::remove_idx(&self.syn_ampa_h[l], j);
        self.syn_ampa_h[target_l] = Self::append_val(&self.syn_ampa_h[target_l], sa);

        let sn = self.syn_nmda_h[l][j]; self.syn_nmda_h[l] = Self::remove_idx(&self.syn_nmda_h[l], j);
        self.syn_nmda_h[target_l] = Self::append_val(&self.syn_nmda_h[target_l], sn);

        let sg_syn = self.syn_gaba_h[l][j]; self.syn_gaba_h[l] = Self::remove_idx(&self.syn_gaba_h[l], j);
        self.syn_gaba_h[target_l] = Self::append_val(&self.syn_gaba_h[target_l], sg_syn);

        let to = self.thr_offset_h[l][j]; self.thr_offset_h[l] = Self::remove_idx(&self.thr_offset_h[l], j);
        self.thr_offset_h[target_l] = Self::append_val(&self.thr_offset_h[target_l], to);

        let re = self.rate_ema_h[l][j]; self.rate_ema_h[l] = Self::remove_idx(&self.rate_ema_h[l], j);
        self.rate_ema_h[target_l] = Self::append_val(&self.rate_ema_h[target_l], re);

        let su = self.stp_u_h[l][j]; self.stp_u_h[l] = Self::remove_idx(&self.stp_u_h[l], j);
        self.stp_u_h[target_l] = Self::append_val(&self.stp_u_h[target_l], su);

        let sx = self.stp_x_h[l][j]; self.stp_x_h[l] = Self::remove_idx(&self.stp_x_h[l], j);
        self.stp_x_h[target_l] = Self::append_val(&self.stp_x_h[target_l], sx);

        if let Some(ref mut r) = self.izh_refr_h {
            let rv = r[l][j]; r[l] = Self::remove_idx(&r[l], j);
            r[target_l] = Self::append_val(&r[target_l], rv);
        }

        let bio = self.bio_h[l].remove(j);
        self.bio_h[target_l].push(bio);

        // Spike history
        if let Some(dq) = self.spk_hist_h.get_mut(l) {
            for frame in dq.iter_mut() {
                *frame = Self::remove_idx(frame, j);
            }
        }
        self.extend_history_frames(target_l, new_j + 1);

        // 2. Topology
        let mut node = self.topo.layers[l].remove(j);
        node.layer = target_l;
        self.topo.layers[target_l].push(node);

        // 3. Morphology
        let mut soma = self.morph.somas[l].remove(j);
        soma.layer = target_l;
        soma.id = new_j;
        self.morph.somas[target_l].push(soma);
        for (idx, s) in self.morph.somas[l].iter_mut().enumerate().skip(j) { s.id = idx; }

        let mut axon = self.morph.axons[l].remove(j);
        axon.neuron_layer = target_l;
        axon.neuron_id = new_j;
        self.morph.axons[target_l].push(axon);
        for (idx, a) in self.morph.axons[l].iter_mut().enumerate().skip(j) { a.neuron_id = idx; }

        let mut dend = self.morph.dendrites[l].remove(j);
        dend.neuron_layer = target_l;
        dend.neuron_id = new_j;
        self.morph.dendrites[target_l].push(dend);
        for (idx, d) in self.morph.dendrites[l].iter_mut().enumerate().skip(j) { d.neuron_id = idx; }

        // 4. Update Synapses and Weight Matrices
        // We will perform a full matrix sync after all movements, but we must update the synapse metadata now
        for syn in &mut self.morph.synapses {
            if syn.pre_layer == l as isize {
                if syn.pre_id == j {
                    syn.pre_layer = target_l as isize;
                    syn.pre_id = new_j;
                } else if syn.pre_id > j {
                    syn.pre_id -= 1;
                }
            }

            if syn.post_layer == l as isize {
                if syn.post_id == j {
                    syn.post_layer = target_l as isize;
                    syn.post_id = new_j;
                } else if syn.post_id > j {
                    syn.post_id -= 1;
                }
            }
        }
        
        // Rebuild matrices and sync presence tracking
        self.repopulate_matrices_from_synapses();
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    fn repopulate_matrices_from_synapses(&mut self) {
        let (in_l, out_l) = self.get_io_layers();
        let num_layers = self.net.num_hidden_layers;
        let num_sensory = self.net.num_sensory_neurons;
        let num_output = self.net.num_output_neurons;

        // 1. Resize all weight matrices to current dimensions based on layer_size()
        let h_in_size = self.layer_size(in_l);
        if self.w_in.nrows() != h_in_size || self.w_in.ncols() != num_sensory {
            self.w_in = Array2::zeros((h_in_size, num_sensory));
        } else {
            self.w_in.fill(0.0);
        }

        for l in 0..num_layers.saturating_sub(1) {
            let rows = self.layer_size(l + 1);
            let cols = self.layer_size(l);
            if l >= self.w_hh_fwd.len() {
                self.w_hh_fwd.push(Array2::zeros((rows, cols)));
            } else if self.w_hh_fwd[l].nrows() != rows || self.w_hh_fwd[l].ncols() != cols {
                self.w_hh_fwd[l] = Array2::zeros((rows, cols));
            } else {
                self.w_hh_fwd[l].fill(0.0);
            }

            if l >= self.w_hh_bwd.len() {
                self.w_hh_bwd.push(Array2::zeros((cols, rows)));
            } else if self.w_hh_bwd[l].nrows() != cols || self.w_hh_bwd[l].ncols() != rows {
                self.w_hh_bwd[l] = Array2::zeros((cols, rows));
            } else {
                self.w_hh_bwd[l].fill(0.0);
            }
        }
        self.w_hh_fwd.truncate(num_layers.saturating_sub(1));
        self.w_hh_bwd.truncate(num_layers.saturating_sub(1));

        for l in 0..num_layers {
            let n = self.layer_size(l);
            if l >= self.w_hh_rec.len() {
                self.w_hh_rec.push(Array2::zeros((n, n)));
            } else if self.w_hh_rec[l].nrows() != n || self.w_hh_rec[l].ncols() != n {
                self.w_hh_rec[l] = Array2::zeros((n, n));
            } else {
                self.w_hh_rec[l].fill(0.0);
            }
        }
        self.w_hh_rec.truncate(num_layers);

        let h_out_size = self.layer_size(out_l);
        if self.w_out.nrows() != num_output || self.w_out.ncols() != h_out_size {
            self.w_out = Array2::zeros((num_output, h_out_size));
        } else {
            self.w_out.fill(0.0);
        }

        // 2. Populate from current morphology synapses
        for syn in &self.morph.synapses {
            let pre_l = syn.pre_layer;
            let post_l = syn.post_layer;
            let i = syn.pre_id;
            let j = syn.post_id;
            let w = syn.weight;

            if pre_l == -1 {
                if post_l == in_l as isize && j < self.w_in.nrows() && i < self.w_in.ncols() {
                    self.w_in[(j, i)] = w;
                }
            } else if post_l == num_layers as isize {
                if pre_l == out_l as isize && j < self.w_out.nrows() && i < self.w_out.ncols() {
                    self.w_out[(j, i)] = w;
                }
            } else if post_l == pre_l + 1 {
                let l = pre_l as usize;
                if l < self.w_hh_fwd.len() && j < self.w_hh_fwd[l].nrows() && i < self.w_hh_fwd[l].ncols() {
                    self.w_hh_fwd[l][(j, i)] = w;
                }
            } else if pre_l == post_l + 1 {
                let l = post_l as usize;
                if l < self.w_hh_bwd.len() && j < self.w_hh_bwd[l].nrows() && i < self.w_hh_bwd[l].ncols() {
                    self.w_hh_bwd[l][(j, i)] = w;
                }
            } else if pre_l == post_l {
                let l = pre_l as usize;
                if l < self.w_hh_rec.len() && j < self.w_hh_rec[l].nrows() && i < self.w_hh_rec[l].ncols() {
                    self.w_hh_rec[l][(j, i)] = w;
                }
            }
        }

        // 3. Finally sync presence sizes to match new weight matrix dimensions
        self.sync_presence_sizes();
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if self.net.use_morphology {
            self.rebuild_syn_maps_from_morph();
        }
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();
    }

    #[cfg(feature = "growth3d")]
    fn remove_neuron_in_layer(&mut self, l: usize, j: usize) {
        if l >= self.net.num_hidden_layers { return; }
        let num_neurons = self.layer_size(l);
        if j >= num_neurons { return; }
        
        nm_log!("[growth] Removing neuron {}:{} from network", l, j);

        // 1. Basic state vectors
        self.v_h[l] = Self::remove_idx(&self.v_h[l], j);
        if let Some(ref mut rfh) = self.refr_h { rfh[l] = Self::remove_idx(&rfh[l], j); }
        if let Some(ref mut uh) = self.u_h { uh[l] = Self::remove_idx(&uh[l], j); }
        self.x_post_h[l] = Self::remove_idx(&self.x_post_h[l], j);
        self.x_pre_h[l] = Self::remove_idx(&self.x_pre_h[l], j);
        self.last_spk_h[l] = Self::remove_idx(&self.last_spk_h[l], j);
        self.syn_ampa_h[l] = Self::remove_idx(&self.syn_ampa_h[l], j);
        self.syn_nmda_h[l] = Self::remove_idx(&self.syn_nmda_h[l], j);
        self.syn_gaba_h[l] = Self::remove_idx(&self.syn_gaba_h[l], j);
        self.thr_offset_h[l] = Self::remove_idx(&self.thr_offset_h[l], j);
        self.rate_ema_h[l] = Self::remove_idx(&self.rate_ema_h[l], j);
        self.stp_u_h[l] = Self::remove_idx(&self.stp_u_h[l], j);
        self.stp_x_h[l] = Self::remove_idx(&self.stp_x_h[l], j);
        self.rate_h[l] = Self::remove_idx(&self.rate_h[l], j);
        self.since_growth_ms[l] = Self::remove_idx(&self.since_growth_ms[l], j);
        self.since_last_bouton_ms[l] = Self::remove_idx(&self.since_last_bouton_ms[l], j);
        self.bio_h[l].remove(j);
        if let Some(ref mut izh_ref) = self.izh_refr_h { izh_ref[l] = Self::remove_idx(&izh_ref[l], j); }
        #[cfg(any(feature = "ui", feature = "growth3d"))]
        {
            if l < self.last_i_f.len() { self.last_i_f[l] = Self::remove_idx(&self.last_i_f[l], j); }
            if l == 0 {
                if let Some(ref mut i_h0) = self.last_i_h0 { *i_h0 = Self::remove_idx(i_h0, j); }
            }
        }

        // 2. Spike history
        if let Some(dq) = self.spk_hist_h.get_mut(l) {
            for frame in dq.iter_mut() {
                *frame = Self::remove_idx(frame, j);
            }
        }

        // 3. Weight matrices
        let (in_l, out_l) = self.get_io_layers();
        if l == in_l {
            self.w_in = Self::remove_row(&self.w_in, j);
            self.conn_presence_in = Self::remove_row(&self.conn_presence_in, j);
        }
        if l == out_l {
            self.w_out = Self::remove_col(&self.w_out, j);
            self.conn_presence_out = Self::remove_col(&self.conn_presence_out, j);
        }
        if l > 0 {
            // neuron j in layer l is a receiver for layer l-1
            self.w_hh_fwd[l - 1] = Self::remove_row(&self.w_hh_fwd[l - 1], j);
            self.w_hh_bwd[l - 1] = Self::remove_col(&self.w_hh_bwd[l - 1], j);
            
            self.conn_presence_fwd[l - 1] = Self::remove_row(&self.conn_presence_fwd[l - 1], j);
            self.conn_presence_bwd[l - 1] = Self::remove_col(&self.conn_presence_bwd[l - 1], j);
        }
        if l < self.net.num_hidden_layers - 1 {
            // neuron j in layer l is a sender for layer l+1
            self.w_hh_fwd[l] = Self::remove_col(&self.w_hh_fwd[l], j);
            self.w_hh_bwd[l] = Self::remove_row(&self.w_hh_bwd[l], j);

            self.conn_presence_fwd[l] = Self::remove_col(&self.conn_presence_fwd[l], j);
            self.conn_presence_bwd[l] = Self::remove_row(&self.conn_presence_bwd[l], j);
        }
        // Recurrent
        self.w_hh_rec[l] = Self::remove_row(&self.w_hh_rec[l], j);
        self.w_hh_rec[l] = Self::remove_col(&self.w_hh_rec[l], j);

        self.conn_presence_rec[l] = Self::remove_row(&self.conn_presence_rec[l], j);
        self.conn_presence_rec[l] = Self::remove_col(&self.conn_presence_rec[l], j);

        // 4. Topology
        if let Some(layer) = self.topo.layers.get_mut(l) {
            if j < layer.len() {
                layer.remove(j);
            }
        }

        self.ensure_state_dimensions(); // Final sync
        self.sync_presence_sizes();
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();

        // 5. Morphology (if active)
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            if self.net.use_morphology {
                if l < self.morph.somas.len() && j < self.morph.somas[l].len() {
                    self.morph.somas[l].remove(j);
                    // Update IDs for remaining somas in the same layer
                    for idx in j..self.morph.somas[l].len() {
                        self.morph.somas[l][idx].id = idx;
                    }
                }
                if l < self.morph.axons.len() && j < self.morph.axons[l].len() {
                    self.morph.axons[l].remove(j);
                    for idx in j..self.morph.axons[l].len() {
                        self.morph.axons[l][idx].neuron_id = idx;
                    }
                }
                if l < self.morph.dendrites.len() && j < self.morph.dendrites[l].len() {
                    self.morph.dendrites[l].remove(j);
                    for idx in j..self.morph.dendrites[l].len() {
                        self.morph.dendrites[l][idx].neuron_id = idx;
                    }
                }
                
                // Remove synapses connected to this neuron and shift others
                let mut old_syn_to_new: std::collections::HashMap<usize, Option<usize>> = std::collections::HashMap::new();
                
                let mut new_synapses = Vec::new();
                for (si, syn) in self.morph.synapses.iter().enumerate() {
                    let mut keep = true;
                    if syn.pre_layer == l as isize && syn.pre_id == j { keep = false; }
                    if syn.post_layer == l as isize && syn.post_id == j { keep = false; }
                    
                    if keep {
                        let mut syn_new = syn.clone();
                        if syn_new.pre_layer == l as isize && syn_new.pre_id > j { syn_new.pre_id -= 1; }
                        if syn_new.post_layer == l as isize && syn_new.post_id > j { syn_new.post_id -= 1; }
                        old_syn_to_new.insert(si, Some(new_synapses.len()));
                        new_synapses.push(syn_new);
                    } else {
                        old_syn_to_new.insert(si, None);
                    }
                }
                self.morph.synapses = new_synapses;
                
                // Update syn_index in all segments of all neurons
                let mut all_axons = vec![&mut self.morph.sensory_axons, &mut self.morph.output_axons];
                for al in &mut self.morph.axons { all_axons.push(al); }
                for al in all_axons {
                    for ax in al {
                        for seg in &mut ax.segments {
                            if let Some(idx) = seg.syn_index {
                                seg.syn_index = old_syn_to_new.get(&idx).and_then(|&opt| opt);
                            }
                        }
                    }
                }
                let mut all_dends = vec![&mut self.morph.sensory_dendrites, &mut self.morph.output_dendrites];
                for dl in &mut self.morph.dendrites { all_dends.push(dl); }
                for dl in all_dends {
                    for den in dl {
                        for seg in &mut den.tree.branches {
                            if let Some(idx) = seg.syn_index {
                                seg.syn_index = old_syn_to_new.get(&idx).and_then(|&opt| opt);
                            }
                        }
                    }
                }
                
                // Rebuild routing maps because indices changed
                self.rebuild_syn_maps_from_morph();
            }
        }

        // Update NetworkConfig initial size if it was reflecting the uniform size (legacy)
        if l == 0 && self.net.num_hidden_layers == 1 {
            self.net.num_hidden_per_layer_initial = self.v_h[0].len();
        }

        self.sync_presence_sizes();
        #[cfg(feature = "opencl")]
        self.mark_all_weights_dirty();
    }

    #[cfg(feature = "growth3d")]
    fn append_val<T: Clone>(arr: &Array1<T>, val: T) -> Array1<T> {
        let old = arr.len();
        // Build via Vec to avoid indexing arr[0] when old == 0
        let mut v: Vec<T> = Vec::with_capacity(old + 1);
        for i in 0..old {
            match arr.get(i) {
                Some(item) => v.push(item.clone()),
                None => {
                    nm_log!("[error] append_val: out of bounds arr[{}], arr.len()={}", i, old);
                    // skip or break; here we skip
                }
            }
        }
        v.push(val);
        Array1::from_vec(v)
    }

    #[cfg(feature = "growth3d")]
    fn remove_row<T: Clone + Default>(arr: &Array2<T>, row_idx: usize) -> Array2<T> {
        let (rows, cols) = arr.dim();
        if rows == 0 || row_idx >= rows { return arr.clone(); }
        let mut new_arr = Array2::from_elem((rows - 1, cols), T::default());
        for j in 0..row_idx {
            for i in 0..cols { new_arr[(j, i)] = arr[(j, i)].clone(); }
        }
        for j in (row_idx + 1)..rows {
            for i in 0..cols { new_arr[(j - 1, i)] = arr[(j, i)].clone(); }
        }
        new_arr
    }

    #[cfg(feature = "growth3d")]
    fn remove_col<T: Clone + Default>(arr: &Array2<T>, col_idx: usize) -> Array2<T> {
        let (rows, cols) = arr.dim();
        if cols == 0 || col_idx >= cols { return arr.clone(); }
        let mut new_arr = Array2::from_elem((rows, cols - 1), T::default());
        for i in 0..col_idx {
            for j in 0..rows { new_arr[(j, i)] = arr[(j, i)].clone(); }
        }
        for i in (col_idx + 1)..cols {
            for j in 0..rows { new_arr[(j, i - 1)] = arr[(j, i)].clone(); }
        }
        new_arr
    }

    #[cfg(feature = "growth3d")]
    fn remove_idx<T: Clone + Default>(arr: &Array1<T>, idx: usize) -> Array1<T> {
        let n = arr.len();
        if n == 0 || idx >= n { return arr.clone(); }
        let mut new_arr = Array1::from_elem(n - 1, T::default());
        for i in 0..idx { new_arr[i] = arr[i].clone(); }
        for i in (idx + 1)..n { new_arr[i - 1] = arr[i].clone(); }
        new_arr
    }
}

#[cfg(all(test, feature = "growth3d"))]
mod tests {
    use super::*;

    fn mk_runner() -> Runner {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        // make growth easy in tests
        net.saturation_threshold = 0.01;
        net.saturation_window_ms = 10.0;
        net.growth_cooldown_ms = 0.0;
        net.global_growth_cooldown_ms = 0.0;
        Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp)
    }

    #[test]
    fn same_layer_spawn_updates_shapes() {
        let mut r = mk_runner();
        assert_eq!(r.net.num_hidden_layers, 1);
        assert_eq!(r.net.num_hidden_per_layer_initial, 1);
        let s = r.net.num_sensory_neurons;
        let o = r.net.num_output_neurons;
        // Force saturation of neuron 0 in layer 0
        r.rate_h[0][0] = 1.0;
        r.since_growth_ms[0][0] = r.net.growth_cooldown_ms + 1.0;
        r.last_global_growth_ms = r.net.global_growth_cooldown_ms + 1.0;
        r.collect_growth_candidates();
        assert_eq!(r.growth_queue.len(), 1);
        r.apply_growth_queue();
        // H increased to 2
        assert_eq!(r.net.num_hidden_per_layer_initial, 2);
        // w_in rows increased
        assert_eq!(r.w_in.nrows(), 2);
        assert_eq!(r.w_in.ncols(), s);
        // w_out cols increased
        assert_eq!(r.w_out.ncols(), 2);
        assert_eq!(r.w_out.nrows(), o);
        // state vectors grew
        assert_eq!(r.v_h[0].len(), 2);
        assert_eq!(r.last_spk_h[0].len(), 2);
    }

    #[test]
    fn global_cooldown_blocks_growth() {
        let mut r = mk_runner();
        r.net.global_growth_cooldown_ms = 1000.0;
        r.last_global_growth_ms = 0.0; // just reset
        r.rate_h[0][0] = 1.0;
        r.since_growth_ms[0][0] = r.net.growth_cooldown_ms + 1.0;
        r.collect_growth_candidates();
        // Should be blocked by global cooldown
        assert_eq!(r.growth_queue.len(), 0);
        // Advance timer and try again
        r.last_global_growth_ms = 2000.0;
        r.collect_growth_candidates();
        assert_eq!(r.growth_queue.len(), 1);
    }

    #[test]
    fn dynamic_step_no_panic_with_growth() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        net.saturation_threshold = 0.05; // easy to trigger
        net.saturation_window_ms = 50.0;
        net.growth_cooldown_ms = 50.0;
        net.global_growth_cooldown_ms = 50.0;
        net.layer_split_threshold = 2; // split early
        net.max_layers = 3;
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        // run several steps with random spikes of correct length
        for _ in 0..200 {
            let s = vec![if fastrand::f32() < 0.05 { 1 } else { 0 }; r.net.num_sensory_neurons];
            let _ = r.step(Some(&s));
        }
        // After steps, ensure shapes are consistent with per-layer sizes
        let l_count_res = r.net.num_hidden_layers;
        // Interfaces count equals L-1
        assert_eq!(r.w_hh_fwd.len(), l_count_res.saturating_sub(1));
        assert_eq!(r.w_hh_bwd.len(), l_count_res.saturating_sub(1));
        for l in 0..l_count_res.saturating_sub(1) {
            let h_l = r.v_h[l].len();
            let h_lp1 = r.v_h[l+1].len();
            assert_eq!(r.w_hh_fwd[l].nrows(), h_lp1);
            assert_eq!(r.w_hh_fwd[l].ncols(), h_l);
            assert_eq!(r.w_hh_bwd[l].nrows(), h_l);
            assert_eq!(r.w_hh_bwd[l].ncols(), h_lp1);
        }
        if l_count_res > 0 {
            let h_last = r.v_h[l_count_res-1].len();
            assert_eq!(r.w_out.ncols(), h_last);
        }
        // And w_in rows must equal H0
        if l_count_res > 0 { assert_eq!(r.w_in.nrows(), r.v_h[0].len()); }
    }

    #[test]
    fn test_sensory_connection_limit() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.num_sensory_neurons = 10;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 100; // many neurons to ensure p_in has many chances
        net.p_in = 1.0; // Force full connectivity (should be capped at 6)
        
        let r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        
        for i in 0..10 {
            let count = r.sensory_connection_count(i);
            assert!(count <= 6, "Sensory neuron {} has {} connections (max 6)", i, count);
        }
    }

    #[test]
    fn test_sensory_connection_limit_on_resize() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.num_sensory_neurons = 0;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 100;
        net.p_in = 1.0; 
        
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        r.resize_sensory(10);
        
        for i in 0..10 {
            let count = r.sensory_connection_count(i);
            assert!(count <= 6, "Resized sensory neuron {} has {} connections (max 6)", i, count);
        }
    }

    #[test]
    fn test_aarnn_io_connectivity_floor_non_morph_growth() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        net.use_morphology = false;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 1;
        net.max_sensory_connections = 3;
        net.max_output_connections = 3;
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Aarnn, Learning::Aarnn);

        for l in 0..4 {
            r.spawn_neuron_into_next_layer(l, 0);
        }
        assert_eq!(r.net.num_hidden_layers, 5);
        r.resize_sensory(3);
        r.resize_output(2);
        r.w_in.fill(0.0);
        r.w_out.fill(0.0);

        let _ = r.step(None);

        for i in 0..r.net.num_sensory_neurons {
            let c = r.sensory_connection_count(i);
            assert!(c >= 1, "Sensory neuron {} lost all targets after growth", i);
            assert!(
                c <= r.net.max_sensory_connections.max(1),
                "Sensory neuron {} exceeded cap: {} > {}",
                i,
                c,
                r.net.max_sensory_connections.max(1)
            );
        }
        for k in 0..r.net.num_output_neurons {
            let mut c = 0usize;
            for j in 0..r.w_out.ncols() {
                if r.w_out[(k, j)].abs() > 1e-12 {
                    c += 1;
                }
            }
            assert!(c >= 1, "Output neuron {} lost all sources after growth", k);
            assert!(
                c <= r.net.max_output_connections.max(1),
                "Output neuron {} exceeded cap: {} > {}",
                k,
                c,
                r.net.max_output_connections.max(1)
            );
        }
    }

    #[test]
    #[cfg(all(feature = "growth3d", feature = "morpho"))]
    fn test_aarnn_io_connectivity_floor_with_morph_growth() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        net.use_morphology = true;
        net.morpho_growth_enabled = true;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 1;
        net.max_sensory_connections = 3;
        net.max_output_connections = 3;
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Aarnn, Learning::Aarnn);

        for l in 0..4 {
            r.spawn_neuron_into_next_layer(l, 0);
        }
        assert_eq!(r.net.num_hidden_layers, 5);
        r.resize_sensory(2);
        r.resize_output(2);
        r.w_in.fill(0.0);
        r.w_out.fill(0.0);
        r.rebuild_morphology();
        r.apply_morpho_evolution(10.0, false);

        for i in 0..r.net.num_sensory_neurons {
            let c = r.sensory_connection_count(i);
            assert!(c >= 1, "Sensory neuron {} should reconnect via morphology floor", i);
            assert!(
                c <= r.net.max_sensory_connections.max(1),
                "Sensory neuron {} exceeded cap: {} > {}",
                i,
                c,
                r.net.max_sensory_connections.max(1)
            );
        }
        for k in 0..r.net.num_output_neurons {
            let mut c = 0usize;
            for j in 0..r.w_out.ncols() {
                if r.w_out[(k, j)].abs() > 1e-12 {
                    c += 1;
                }
            }
            assert!(c >= 1, "Output neuron {} should reconnect via morphology floor", k);
            assert!(
                c <= r.net.max_output_connections.max(1),
                "Output neuron {} exceeded cap: {} > {}",
                k,
                c,
                r.net.max_output_connections.max(1)
            );
        }
    }

    #[test]
    #[cfg(all(feature = "growth3d", feature = "morpho"))]
    fn test_sensory_connection_limit_morpho_evolve() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.num_sensory_neurons = 1;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 10;
        net.p_in = 0.0; // Start with no connections
        net.morpho_growth_enabled = true;
        net.axon_contact_dist = 2.0; // large distance to ensure contact
        
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        
        // Run evolve
        let res = r.morph.evolve(&r.net, false, 1.0, #[cfg(feature = "opencl")] None);
        
        // Apply new connections to w_in
        for (pre_l, pre_id, _post_l, post_id, w) in res.new_connections {
            if pre_l == -1 {
                r.w_in[(post_id as usize, pre_id as usize)] = w;
            }
        }
        
        let count = r.sensory_connection_count(0);
        assert!(count <= 6, "Morpho evolve sensory neuron 0 has {} connections (max 6)", count);
    }

    #[test]
    fn aarnn_s_to_h1_delay_delivery() {
        // Compare AARNN (low velocity) vs classic: AARNN should not arrive earlier
        // than classic; at very high velocity, AARNN should match classic.
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        // Case A: low velocity → spike time >= classic
        let mut net = NetworkConfig::default();
        net.growth_enabled = true; // minimal 1×1
        net.use_aarnn_delays = true;
        net.aarnn_velocity = 0.3; // fairly slow → multi-step delay
        net.axon_velocity = 0.0;
        net.dend_velocity = 0.0;
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Aarnn, Learning::Stdp);
        r.net.use_morphology = false;
        
        // Manually grow to layer 1 so we can add sensory
        r.spawn_neuron_into_next_layer(0, 0);
        r.target_num_sensory = 1;
        r.resize_sensory(1);

        r.stdp.eta = 0.0; // freeze learning
        r.net.p_release_default = 1.0;
        for j in 0..r.w_in.nrows() { for i in 0..r.w_in.ncols() { r.w_in[(j,i)] = 0.0; } }
        r.w_in[(0,0)] = 2.0;
        
        // r.reset(); // DO NOT CALL RESET HERE
        
        r.net.use_morphology = false; // reset() might have flipped it back if enabled in config
        let mut fired_steps: Vec<usize> = Vec::new();
        for step in 0..15 {
            let s = if step == 0 { vec![1i8] } else { vec![0i8] };
            let out = r.step(Some(&s));
            if out.spk_h[1][0] != 0 { 
                fired_steps.push(out.t); 
            }
        }
        assert!(!fired_steps.is_empty(), "Hidden Layer 1 did not spike with delayed input");
        let aarnn_first = fired_steps[0];
        // Classic reference
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net_ref = NetworkConfig::default();
        net_ref.num_sensory_neurons = 1;
        net_ref.growth_enabled = true;
        let mut r_ref = Runner::new(lif, stdp, net_ref, NeuronModel::Lif, Learning::Stdp);
        r_ref.stdp.eta = 0.0;
        for j in 0..r_ref.w_in.nrows() { for i in 0..r_ref.w_in.ncols() { r_ref.w_in[(j,i)] = 0.0; } }
        r_ref.w_in[(0,0)] = 2.0;
        let mut lif_first: Option<usize> = None;
        for step in 0..5 {
            let s = if step == 0 { vec![1i8; r_ref.net.num_sensory_neurons] } else { vec![0i8; r_ref.net.num_sensory_neurons] };
            let out = r_ref.step(Some(&s));
            if lif_first.is_none() && out.spk_h[0][0] != 0 { lif_first = Some(out.t); }
        }
        let lif_first = lif_first.unwrap_or(usize::MAX);
        assert!(aarnn_first >= lif_first, "AARNN (slow) should not be earlier than classic");

        // Case B: very high velocity → near-immediate matching classic
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net2 = NetworkConfig::default();
        net2.growth_enabled = true;
        net2.use_aarnn_delays = true;
        net2.aarnn_velocity = 1_000.0; // effectively zero delay
        net2.axon_velocity = 0.0;
        net2.dend_velocity = 0.0;
        net2.bouton_latency_ms = 0.0;
        let mut r2 = Runner::new(lif, stdp, net2, NeuronModel::Aarnn, Learning::Stdp);
        r2.net.use_morphology = false;
        
        r2.spawn_neuron_into_next_layer(0, 0);
        r2.target_num_sensory = 1;
        r2.resize_sensory(1);

        r2.stdp.eta = 0.0;
        r2.net.p_release_default = 1.0;
        for j in 0..r2.w_in.nrows() { for i in 0..r2.w_in.ncols() { r2.w_in[(j,i)] = 0.0; } }
        r2.w_in[(0,0)] = 2.0;
        r2.reset();
        r2.net.use_morphology = false;
        let mut a_first: Option<usize> = None;
        for step in 0..3 {
            let s = if step == 0 { vec![1i8] } else { vec![0i8] };
            let out = r2.step(Some(&s));
            if a_first.is_none() && out.spk_h[1][0] != 0 { a_first = Some(out.t); }
        }
        let a_first = a_first.unwrap_or(usize::MAX);
        assert_eq!(a_first, lif_first, "AARNN fast should match classic timing");
    }

    // #[test]
    // fn aarnn_h4_to_o_delay_delivery() {
    //     // Ensure hiddenLast→output delays are respected when enabled.
    //     let lif = LIFParams::default();
    //     let stdp = STDPParams::default();
    //     let mut net = NetworkConfig::default();
    //     net.growth_enabled = true; // minimal 1×1
    //     net.use_aarnn_delays = true;
    //     net.aarnn_velocity = 0.5;
    //     net.axon_velocity = 0.0;
    //     net.dend_velocity = 0.0;
    //     net.bouton_latency_ms = 0.0;
    //     let mut r = Runner::new(lif, stdp, net, NeuronModel::Aarnn, Learning::Stdp);
    //     r.net.use_morphology = false;
    //     
    //     // Manually grow to layer 4 (5th layer) so we can add output
    //     for l in 0..4 {
    //         r.spawn_neuron_into_next_layer(l, 0);
    //     }
    //     r.target_num_sensory = 1;
    //     r.resize_sensory(1);
    //     r.target_num_output = 1;
    //     r.resize_output(1);
    // 
    //     r.stdp.eta = 0.0; // freeze learning
    //     r.net.p_release_default = 1.0;
    //     
    //     // H(l) -> H(l+1)
    //     for l in 0..r.w_hh_fwd.len() {
    //         r.w_hh_fwd[l].fill(2.0);
    //     }
    // 
    //     // H4 -> O
    //     for k in 0..r.w_out.nrows() { for j in 0..r.w_out.ncols() { r.w_out[(k,j)] = 0.0; } }
    //     r.w_out[(0,0)] = 2.0;
    //     
    //     r.net.use_morphology = false;
    //     r.net.aarnn_synaptic_energy_randomness = 1.0; // force H0 to spike
    //     
    //     let mut h0_fire_t: Option<usize> = None;
    //     let mut o_fire_t: Option<usize> = None;
    //     for _step in 0..50 {
    //         let out = r.step(None);
    //         if h0_fire_t.is_none() && !out.spk_h.is_empty() && !out.spk_h[0].is_empty() && out.spk_h[0][0] != 0 { 
    //             h0_fire_t = Some(out.t); 
    //         }
    //         if o_fire_t.is_none() && !out.spk_o.is_empty() && out.spk_o[0] != 0 { 
    //             o_fire_t = Some(out.t); 
    //         }
    //     }
    //     
    //     // If layer 0 random spiking is too unreliable in tests, just force it
    //     if h0_fire_t.is_none() {
    //         r.v_h[0][0] = 10.0;
    //         for _step in 0..50 {
    //             let out = r.step(None);
    //             if h0_fire_t.is_none() && !out.spk_h.is_empty() && out.spk_h[0][0] != 0 { h0_fire_t = Some(out.t); }
    //             if o_fire_t.is_none() && !out.spk_o.is_empty() && out.spk_o[0] != 0 { o_fire_t = Some(out.t); }
    //         }
    //     }
    // 
    //     let ht = h0_fire_t.expect("Hidden H0 failed to spike");
    //     let ot = o_fire_t.expect("Output failed to spike");
    //     assert!(ot > ht, "Output spike should occur after hidden spike");
    // }
    #[test]
    fn spawn_into_next_layer_shapes() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        // Force a queue that targets next layer
        r.rate_h[0][0] = 1.0;
        r.since_growth_ms[0][0] = r.net.growth_cooldown_ms + 1.0;
        r.last_global_growth_ms = r.net.global_growth_cooldown_ms + 1.0;
        r.collect_growth_candidates();
        // With default split threshold, target should be layer 0 (same) if H0 is small
        // We set split threshold to 1 to force it
        r.net.layer_split_threshold = 1;
        r.collect_growth_candidates();
        assert_eq!(r.growth_queue.len(), 1);
        let act = r.growth_queue[0];
        assert_eq!(act.layer, 0);
        assert_eq!(act.target_layer, 1);
        r.apply_growth_queue();
        // Shapes
        assert!(r.net.num_hidden_layers >= 2);
        let h0 = r.v_h[0].len();
        let h1 = r.v_h[1].len();
        assert_eq!(r.w_hh_fwd[0].ncols(), h0);
        assert_eq!(r.w_hh_fwd[0].nrows(), h1);
        assert_eq!(r.w_hh_bwd[0].nrows(), h0);
        assert_eq!(r.w_hh_bwd[0].ncols(), h1);
    }

    #[test]
    fn test_aarnn_growth_sequence_no_panic() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        net.num_sensory_neurons = 10;
        net.num_output_neurons = 5;
        
        // 1. Create a non-AARNN runner (e.g. LIF)
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        
        // 2. Run a few steps
        for _ in 0..10 { r.step(None); }
        
        // 3. Switch to AARNN (Simulation of UI switch)
        // We recreate the runner as the fix suggested
        let net_for_aarnn = r.net;
        let mut r = Runner::new(lif, stdp, net_for_aarnn, NeuronModel::Aarnn, Learning::Aarnn);
        
        // AARNN should start with 0 sensory/output and 1 layer
        assert_eq!(r.net.num_sensory_neurons, 0);
        assert_eq!(r.net.num_output_neurons, 0);
        assert_eq!(r.net.num_hidden_layers, 1);
        
        // 4. Manually grow it to trigger the logic
        // Grow to 2 layers (so sensory can start forming)
        r.spawn_neuron_into_next_layer(0, 0);
        assert_eq!(r.net.num_hidden_layers, 2);
        
        // Grow Layer 1 (this was where the panic happened in resize_sensory)
        r.spawn_neuron_in_layer(1, 0);
        
        // Now trigger resize_sensory
        r.resize_sensory(1); // Should not panic now!
        r.resize_sensory(2); 
        
        // Grow Layer 1 further
        for _ in 0..10 {
            r.spawn_neuron_in_layer(1, 0);
        }
        
        // Resize sensory again
        r.resize_sensory(5);
        
        // Grow to 5 layers (so output can start forming)
        r.spawn_neuron_into_next_layer(1, 0); // L=3
        r.spawn_neuron_into_next_layer(2, 0); // L=4
        r.spawn_neuron_into_next_layer(3, 0); // L=5
        
        assert_eq!(r.net.num_hidden_layers, 5);
        
        // Trigger resize_output (connects to Layer 4)
        r.resize_output(1); // Should not panic!
        
        // Grow Layer 4
        r.spawn_neuron_in_layer(4, 0);
        
        // Resize output again
        r.resize_output(3);
        
        // 5. Run steps
        for _ in 0..100 { r.step(None); }
    }

    #[test]
    fn test_switch_models_direct_robustness() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        net.num_sensory_neurons = 50;
        
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        
        // Grow a bit
        for _ in 0..5 { r.spawn_neuron_in_layer(0, 0); }
        
        // Force switch without recreation (testing robustness of resize methods)
        r.neuron_model = NeuronModel::Aarnn;
        
        // This used to panic if layer 1 didn't exist, but resize_sensory now has a check
        r.resize_sensory(20);
    }

    #[test]
    fn test_config_apply_robustness() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.growth_enabled = true;
        net.num_hidden_per_layer_initial = 1;
        net.num_sensory_neurons = 10;
        
        let mut r = Runner::new(lif, stdp, net.clone(), NeuronModel::Lif, Learning::Stdp);
        
        // 1. Grow the network to 10 neurons in L0
        for _ in 0..9 {
            r.spawn_neuron_l0(0);
        }
        assert_eq!(r.layer_size(0), 10);
        assert_eq!(r.net.num_hidden_per_layer_initial, 10);
        
        // 2. Simulate applying a "best" config from GA that has initial values
        let mut best_cfg = net; // num_hidden_per_layer_initial = 1
        best_cfg.saturation_threshold = 0.1;
        
        // Use the NEW safe apply_config
        r.apply_config(best_cfg);
        
        // Should have preserved current size in net.num_hidden_per_layer_initial
        assert_eq!(r.net.num_hidden_per_layer_initial, 10);
        assert_eq!(r.layer_size(0), 10);
        
        // 3. Trigger further growth. 
        // This used to panic if spawn_neuron_l0 used net.num_hidden_per_layer_initial (1) 
        // while the matrices actually had 10 rows.
        r.spawn_neuron_l0(0);
        
        assert_eq!(r.layer_size(0), 11);
        assert_eq!(r.net.num_hidden_per_layer_initial, 11);
    }

    #[test]
    #[cfg(all(feature = "growth3d", feature = "morpho"))]
    fn test_aarnn_multilayer_growth_panic() {
        let mut config = NetworkConfig::default();
        config.num_hidden_per_layer_initial = 1;
        config.num_hidden_layers = 6;
        config.use_morphology = true;
        config.morpho_growth_enabled = true;
        config.use_aarnn_delays = true;
        config.aarnn_layer_depth = 5;
        config.sensory_target_layer = Some(1);
        config.output_source_layer = Some(4);
        
        let mut runner = Runner::new(LIFParams::default(), STDPParams::default(), config.clone(), NeuronModel::Aarnn, Learning::Aarnn);
        
        // Step a few times
        for _ in 0..10 {
            runner.step(Some(&vec![0i8; 10]));
        }
        
        // Force a spawn in layer 0
        runner.spawn_neuron_in_layer(0, 0);
        
        // Next step should NOT panic
        runner.step(Some(&vec![1i8; 10]));
    }

    #[test]
    #[cfg(all(feature = "growth3d", feature = "morpho"))]
    fn test_sensory_migration() {
        use crate::morphology::{Synapse, SynKind, Point3};

        let mut net = NetworkConfig::default();
        net.use_morphology = true;
        net.morpho_growth_enabled = true;
        net.growth_enabled = false; // Disable layer splitting growth
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 2; // Two neurons in layer 0
        net.num_sensory_neurons = 1;
        net.num_output_neurons = 1;
        net.max_sensory_connections = 1; // Strict limit
        net.initial_synaptic_weight = 0.5;
        net.p_in = 0.0; // Don't form synapses automatically
        net.p_hidden = 0.0;
        net.p_out = 0.0;
        
        let mut r = Runner::new(LIFParams::default(), STDPParams::default(), net, NeuronModel::Aarnn, Learning::Aarnn);
        r.net.num_sensory_neurons = 1;
        r.net.num_output_neurons = 1;
        r.spawn_neuron_in_layer(0, 0); // Add second hidden neuron
        r.rebuild_default_topology();
        r.rebuild_morphology();
        
        println!("DEBUG: r.net.num_sensory_neurons: {}", r.net.num_sensory_neurons);
        println!("DEBUG: sensory_nodes len: {}", r.topo.sensory_nodes.len());
        println!("DEBUG: sensory_somas len: {}", r.morph.sensory_somas.len());

        // Initial state: Sensory 0 is connected to someone.
        // Let's force positions to control the scenario.
        {
            // Clear existing axons/dendrites to ensure we only have our manual ones for better determinism
            for layer in &mut r.morph.axons { for ax in layer { ax.segments.clear(); } }
            for ax in &mut r.morph.sensory_axons { ax.segments.clear(); ax.segments.push(crate::morphology::AxonSeg::default()); }
            for layer in &mut r.morph.dendrites { for dend in layer { dend.tree.branches.clear(); dend.tree.branches.push(crate::morphology::DendSeg::default()); } }

            // Sensory soma at (0, 0, 0)
            r.morph.sensory_somas[0].pos = Point3 { x: 0.0, y: 0.0, z: 0.0 };
            // Sensory axon tip at (0.1, 0, 0)
            r.morph.sensory_axons[0].segments[0].from = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.sensory_axons[0].segments[0].to = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.sensory_axons[0].segments[0].stimuli = 1.0;

            // Neuron 0 at (0.11, 0, 0) - Very close to axon tip
            r.morph.somas[0][0].pos = Point3 { x: 0.11, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][0].tree.branches[0].from = Point3 { x: 0.11, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][0].tree.branches[0].to = Point3 { x: 0.11, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][0].tree.branches[0].stimuli = 1.0;

            // Neuron 1 at (0.2, 0, 0) - Further away
            r.morph.somas[0][1].pos = Point3 { x: 0.2, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][1].tree.branches[0].from = Point3 { x: 0.2, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][1].tree.branches[0].to = Point3 { x: 0.2, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][1].tree.branches[0].stimuli = 1.0;
            
            // Clear all synapses and force one from Sensory 0 to Neuron 0
            r.morph.synapses.clear();
            
            // Manually create synapse Sensory 0 -> Hidden 0
            r.morph.synapses.push(Synapse {
                kind: SynKind::In,
                pre_layer: -1, pre_id: 0,
                post_layer: 0, post_id: 0,
                pre_site: Point3 { x: 0.1, y: 0.0, z: 0.0 },
                post_site: Point3 { x: 0.11, y: 0.0, z: 0.0 },
                axon_seg_idx: Some(0),
                dend_seg_idx: Some(0),
                bend: None, weight: 0.5, p_release: 1.0, delay_ms: 1.0, stimuli: 1.0,
            });
            r.morph.sensory_axons[0].segments[0].syn_index = Some(0);
            r.morph.dendrites[0][0].tree.branches[0].syn_index = Some(0);
            
            r.rebuild_syn_maps_from_morph();
        }
        
        // Verify initial connection
        assert_eq!(r.morph.synapses.len(), 1);
        assert_eq!(r.morph.synapses[0].post_id, 0);
        
        // Now move Neuron 1 to be EVEN CLOSER than Neuron 0
        // And move Neuron 0 further away.
        {
            // Clear segments again just in case
            for layer in &mut r.morph.axons { for ax in layer { ax.segments.clear(); ax.segments.push(crate::morphology::AxonSeg::default()); } }
            for ax in &mut r.morph.sensory_axons { ax.segments.clear(); ax.segments.push(crate::morphology::AxonSeg::default()); }
            for layer in &mut r.morph.dendrites { for dend in layer { dend.tree.branches.clear(); dend.tree.branches.push(crate::morphology::DendSeg::default()); } }

            // Sensory axon tip at (0.1, 0, 0)
            r.morph.sensory_axons[0].segments[0].from = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.sensory_axons[0].segments[0].to = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.sensory_axons[0].segments[0].stimuli = 1.0;

            // Neuron 1 now at (0.101, 0, 0) - EXTREMELY CLOSE to axon tip (0.1, 0, 0)
            r.morph.somas[0][1].pos = Point3 { x: 0.101, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][1].tree.branches[0].from = Point3 { x: 0.101, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][1].tree.branches[0].to = Point3 { x: 0.101, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][1].tree.branches[0].stimuli = 1.0;

            // Neuron 0 moved to (0.15, 0, 0)
            r.morph.somas[0][0].pos = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][0].tree.branches[0].from = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][0].tree.branches[0].to = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.dendrites[0][0].tree.branches[0].stimuli = 1.0;

            // Update synapse post_site and pre_site
            r.morph.synapses[0].pre_site = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.synapses[0].post_site = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.synapses[0].axon_seg_idx = Some(0);
            r.morph.synapses[0].dend_seg_idx = Some(0);
            
            println!("DEBUG: Sensory Axon Tip: {:?}", r.morph.sensory_axons[0].segments[0].to);
            println!("DEBUG: Neuron 0 Tip: {:?}", r.morph.dendrites[0][0].tree.branches[0].to);
            println!("DEBUG: Neuron 1 Tip: {:?}", r.morph.dendrites[0][1].tree.branches[0].to);
            println!("DEBUG: Synapse 0 Post Site: {:?}", r.morph.synapses[0].post_site);
        }
        
        // Set stimuli high to make it "energetic"
        r.morph.synapses[0].stimuli = 2.0; 
        
        // Run evolve.
        r.apply_morpho_evolution(10.0, false); 
        
        // Check if synapse 0 now points to Neuron 1
        assert_eq!(r.morph.synapses[0].post_id, 1, "Sensory connection should have migrated to the closer neuron (Neuron 1)");
    }

    #[test]
    #[cfg(all(feature = "growth3d", feature = "morpho"))]
    fn test_output_migration() {
        use crate::morphology::{Synapse, SynKind, Point3};

        let mut net = NetworkConfig::default();
        net.use_morphology = true;
        net.morpho_growth_enabled = true;
        net.growth_enabled = false;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 2; // Two hidden neurons
        net.num_sensory_neurons = 1;
        net.num_output_neurons = 1;
        net.max_output_connections = 1; // Strict limit
        net.initial_synaptic_weight = 0.5;
        net.p_in = 0.0;
        net.p_hidden = 0.0;
        net.p_out = 0.0;
        
        let mut r = Runner::new(LIFParams::default(), STDPParams::default(), net, NeuronModel::Aarnn, Learning::Aarnn);
        r.net.num_sensory_neurons = 1;
        r.net.num_output_neurons = 1;
        r.spawn_neuron_in_layer(0, 0); // Add second hidden neuron
        r.rebuild_default_topology();
        r.rebuild_morphology();
        
        // Initial state: Output 0 is connected to Hidden 0.
        {
            // Clear existing axons/dendrites for determinism
            for layer in &mut r.morph.axons { for ax in layer { ax.segments.clear(); ax.segments.push(crate::morphology::AxonSeg::default()); } }
            for dend in &mut r.morph.output_dendrites { dend.tree.branches.clear(); dend.tree.branches.push(crate::morphology::DendSeg::default()); }

            // Output soma at (0, 0, 0)
            r.morph.output_somas[0].pos = Point3 { x: 0.0, y: 0.0, z: 0.0 };
            // Output dendrite tip at (0.1, 0, 0)
            r.morph.output_dendrites[0].tree.branches[0].from = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.output_dendrites[0].tree.branches[0].to = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.output_dendrites[0].tree.branches[0].stimuli = 1.0;

            // Hidden 0 axon tip at (0.11, 0, 0) - Close to output dendrite
            r.morph.axons[0][0].segments[0].from = Point3 { x: 0.11, y: 0.0, z: 0.0 };
            r.morph.axons[0][0].segments[0].to = Point3 { x: 0.11, y: 0.0, z: 0.0 };
            r.morph.axons[0][0].segments[0].stimuli = 1.0;
            
            // Hidden 1 axon tip at (0.2, 0, 0) - Further away
            r.morph.axons[0][1].segments[0].from = Point3 { x: 0.2, y: 0.0, z: 0.0 };
            r.morph.axons[0][1].segments[0].to = Point3 { x: 0.2, y: 0.0, z: 0.0 };
            r.morph.axons[0][1].segments[0].stimuli = 1.0;
            
            // Clear all synapses and force one from Hidden 0 -> Output 0
            r.morph.synapses.clear();
            r.morph.synapses.push(Synapse {
                kind: SynKind::Out,
                pre_layer: 0, pre_id: 0,
                post_layer: 1, post_id: 0, // Layer 1 is output if num_hidden_layers = 1
                pre_site: Point3 { x: 0.11, y: 0.0, z: 0.0 },
                post_site: Point3 { x: 0.1, y: 0.0, z: 0.0 },
                axon_seg_idx: Some(0),
                dend_seg_idx: Some(0),
                bend: None, weight: 0.5, p_release: 1.0, delay_ms: 1.0, stimuli: 1.0,
            });
            r.morph.axons[0][0].segments[0].syn_index = Some(0);
            r.morph.output_dendrites[0].tree.branches[0].syn_index = Some(0);
            
            r.rebuild_syn_maps_from_morph();
        }
        
        // Verify initial connection
        assert_eq!(r.morph.synapses.len(), 1);
        assert_eq!(r.morph.synapses[0].pre_id, 0);
        
        // Now move Hidden 1 axon tip to be EVEN CLOSER than Hidden 0
        // And move Hidden 0 further away.
        {
            // Clear segments for determinism
            for layer in &mut r.morph.axons { for ax in layer { ax.segments.clear(); ax.segments.push(crate::morphology::AxonSeg::default()); } }
            for dend in &mut r.morph.output_dendrites { dend.tree.branches.clear(); dend.tree.branches.push(crate::morphology::DendSeg::default()); }

            // Output dendrite tip at (0.1, 0, 0)
            r.morph.output_dendrites[0].tree.branches[0].from = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.output_dendrites[0].tree.branches[0].to = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.output_dendrites[0].tree.branches[0].stimuli = 1.0;

            // Hidden 1 axon now at (0.101, 0, 0) - EXTREMELY CLOSE to output dendrite (0.1, 0, 0)
            r.morph.axons[0][1].segments[0].from = Point3 { x: 0.101, y: 0.0, z: 0.0 };
            r.morph.axons[0][1].segments[0].to = Point3 { x: 0.101, y: 0.0, z: 0.0 };
            r.morph.axons[0][1].segments[0].stimuli = 1.0;

            // Hidden 0 moved to (0.15, 0, 0)
            r.morph.axons[0][0].segments[0].from = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.axons[0][0].segments[0].to = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.axons[0][0].segments[0].stimuli = 1.0;

            // Update synapse pre_site and post_site
            r.morph.synapses[0].pre_site = Point3 { x: 0.15, y: 0.0, z: 0.0 };
            r.morph.synapses[0].post_site = Point3 { x: 0.1, y: 0.0, z: 0.0 };
            r.morph.synapses[0].axon_seg_idx = Some(0);
            r.morph.synapses[0].dend_seg_idx = Some(0);

            println!("DEBUG: Output Dendrite Tip: {:?}", r.morph.output_dendrites[0].tree.branches[0].to);
            println!("DEBUG: Hidden 0 Axon Tip: {:?}", r.morph.axons[0][0].segments[0].to);
            println!("DEBUG: Hidden 1 Axon Tip: {:?}", r.morph.axons[0][1].segments[0].to);
            println!("DEBUG: Synapse 0 Pre Site: {:?}", r.morph.synapses[0].pre_site);
        }
        
        // Set stimuli high to make it "energetic"
        r.morph.synapses[0].stimuli = 2.0; 
        
        // Run evolve.
        r.apply_morpho_evolution(10.0, false); 
        
        // Check if synapse 0 now points to Hidden 1
        assert_eq!(r.morph.synapses[0].pre_id, 1, "Output connection should have migrated to the closer neuron (Hidden 1)");
    }

    #[test]
    fn test_max_total_neurons_limit() {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut net = NetworkConfig::default();
        net.num_sensory_neurons = 5;
        net.num_output_neurons = 5;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 1;
        net.max_total_neurons = 12; // 5 + 5 + 1 = 11 initial. Can only grow 1 more.
        net.growth_enabled = true;
        net.growth_cooldown_ms = 0.0;
        net.global_growth_cooldown_ms = 0.0;
        net.saturation_threshold = 0.01;
        
        let mut r = Runner::new(lif, stdp, net, NeuronModel::Lif, Learning::Stdp);
        assert_eq!(r.total_neurons(), 11);
        assert!(!r.is_at_max_neurons());

        // Trigger growth
        r.rate_h[0][0] = 1.0;
        r.since_growth_ms[0][0] = 1000.0;
        r.last_global_growth_ms = 1000.0;
        r.collect_growth_candidates();
        assert_eq!(r.growth_queue.len(), 1);
        r.apply_growth_queue();
        assert_eq!(r.total_neurons(), 12);
        assert!(r.is_at_max_neurons());

        // Try triggering growth again
        r.rate_h[0][0] = 1.0;
        r.since_growth_ms[0][0] = 1000.0;
        r.last_global_growth_ms = 1000.0;
        r.collect_growth_candidates();
        assert_eq!(r.growth_queue.len(), 0);
        let spawned = r.apply_growth_queue();
        assert!(!spawned);
        assert_eq!(r.total_neurons(), 12);
    }
}
