//! Batch (matrix-based) spiking simulation used by the CLI path.
//!
//! This module provides a compact simulator that operates on fixed‑shape
//! matrices and does not depend on runtime topology growth or morphology.
//! It mirrors the core ideas of the interactive Runner but omits per‑segment
//! conduction and geometry‑aware delays. AARNN learning is accepted as an
//! option and currently mirrors STDP updates here.
//!
//! Major types
//! - `NeuronModel`: LIF or Izhikevich dynamics (AARNN maps to Izhikevich here).
//! - `Learning`: STDP/Hebb/Oja (AARNN ≈ STDP in this path).
//! - `SimOut`: time series of spikes and the final weights.
//!
//! The UI Runner (`runner.rs`) implements detailed AARNN per‑segment conduction
//! using morphology when built with `growth3d+morpho` features.
use ndarray::{s, Array1, Array2};
use rand::{Rng, RngExt};

#[cfg(feature = "opencl")]
use crate::cl_compute::{
    get_global_cl_manager, Buffer, ClError, ClResult, ExecuteKernel, OpenCLManager,
    CL_INVALID_VALUE, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE, CL_TRUE,
};
use crate::config::{IzhikevichParams, LIFParams, NetworkConfig, STDPParams};
use crate::network::BuiltNetwork;
#[cfg(feature = "opencl")]
use std::ptr;
#[cfg(feature = "opencl")]
use std::sync::Arc;

/// Neuron membrane potential models supported by the batch simulator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NeuronModel {
    /// Leaky Integrate-and-Fire model.
    Lif,
    /// Izhikevich model with specific biological presets.
    Izh(IzhikevichParams),
    /// Adaptive Axonal-Relay Neural Network model.
    /// *Note: In batch mode, this uses Izhikevich-style dynamics.*
    Aarnn,
}

impl NeuronModel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "lif" => Some(Self::Lif),
            "izh" => Some(Self::Izh(IzhikevichParams::from_preset("RS", 1.0))),
            "aarnn" => Some(Self::Aarnn),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Lif => "lif",
            Self::Izh(_) => "izh",
            Self::Aarnn => "aarnn",
        }
    }
}

/// Synaptic learning rules supported by the batch simulator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Learning {
    /// Spike-Timing-Dependent Plasticity.
    Stdp,
    /// Standard Hebbian learning.
    Hebb,
    /// Oja's rule for normalized weight updates.
    Oja,
    /// AARNN-specific learning rule.
    /// *Note: In batch mode, this currently mirrors STDP updates.*
    Aarnn,
}

impl Learning {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "stdp" => Some(Self::Stdp),
            "hebb" => Some(Self::Hebb),
            "oja" => Some(Self::Oja),
            "aarnn" => Some(Self::Aarnn),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Stdp => "stdp",
            Self::Hebb => "hebb",
            Self::Oja => "oja",
            Self::Aarnn => "aarnn",
        }
    }
}

/// Final synaptic weights exported after simulation for analysis and visualization.
#[derive(Debug)]
pub struct WeightsOut {
    /// Input to first hidden layer weights.
    pub w_in: Array2<f64>,
    /// Forward weights between hidden layers.
    pub w_hh_fwd: Vec<Array2<f64>>,
    /// Backward weights between hidden layers.
    pub w_hh_bwd: Vec<Array2<f64>>,
    /// Hidden to output layer weights.
    pub w_out: Array2<f64>,
}

/// Complete results of a simulation run.
#[derive(Debug)]
pub struct SimOut {
    /// Time-series spike rasters for each hidden layer.
    /// Each entry is a (Timesteps × Number of Neurons) matrix.
    pub spikes_h: Vec<Array2<i8>>,
    /// Time-series spike raster for the output layer.
    pub spikes_o: Array2<i8>,
    /// The final state of all synaptic weights.
    pub weights: WeightsOut,
    /// Number of "long-term" connections identified at the end of simulation.
    pub longterm_conn: usize,
    /// Total number of connections in the network.
    pub total_conn: usize,
}

/// Generates synthetic Poisson-distributed spike patterns for the sensory input layer.
///
/// This creates a structured stimulus where different groups of sensory neurons
/// "burst" at higher frequencies at different points in time, following a fixed schedule.
/// This is typically used to test the network's ability to learn and differentiate
/// between different input patterns.
///
/// # Returns
/// A tuple containing:
/// 1. `spikes`: The binary spike raster (Time × Neurons).
/// 2. `pattern_id`: The ID of the active burst group at each time step.
/// 3. `groups`: Indices of neurons belonging to each pattern group.
pub fn poisson_input_patterns<TR: Rng>(
    t_ms: f64,
    num_sensory_neurons: usize,
    dt: f64,
    rng: &mut TR,
) -> (Array2<i8>, Array1<i8>, Vec<Vec<usize>>) {
    let steps = (t_ms / dt).round() as usize;
    let base_rate = 2.0_f64; // Hz
    let burst_rate = 25.0_f64; // Hz
    let base_spike_probability = base_rate * dt / 1000.0;
    let burst_spike_probability = burst_rate * dt / 1000.0;
    let mut spikes = Array2::<i8>::zeros((steps, num_sensory_neurons));
    let mut pattern_id = Array1::<i8>::zeros(steps);
    // Split into three groups
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(), Vec::new(), Vec::new()];
    for (i, idx) in (0..num_sensory_neurons).enumerate() {
        groups[i % 3].push(idx);
    }
    let chunk = (steps / 6).max(1);
    let schedule = [0usize, 1, 2, 0, 2, 1];
    for (k, &pat) in schedule.iter().enumerate() {
        let start = k * chunk;
        let end = if k < schedule.len() - 1 {
            (k + 1) * chunk
        } else {
            steps
        };
        for t in start..end {
            pattern_id[t] = pat as i8;
            for i in 0..num_sensory_neurons {
                spikes[(t, i)] = (rng.random::<f64>() < base_spike_probability) as i8;
            }
            for &i in &groups[pat] {
                spikes[(t, i)] = (rng.random::<f64>() < burst_spike_probability) as i8;
            }
        }
    }
    (spikes, pattern_id, groups)
}

/// Generates deterministic theta-rhythm spike patterns for the sensory input layer.
///
/// This produces a global oscillatory drive with optional per-neuron phase jitter.
/// The duty cycle controls the active window of each theta cycle.
pub fn theta_input_patterns(
    t_ms: f64,
    num_sensory_neurons: usize,
    dt: f64,
    freq_hz: f32,
    duty: f32,
    phase_jitter: f32,
) -> (Array2<i8>, Array1<i8>, Vec<Vec<usize>>) {
    let steps = (t_ms / dt).round() as usize;
    let mut spikes = Array2::<i8>::zeros((steps, num_sensory_neurons));
    let mut pattern_id = Array1::<i8>::zeros(steps);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    if num_sensory_neurons > 0 {
        groups.push((0..num_sensory_neurons).collect());
    }
    if steps == 0 || num_sensory_neurons == 0 {
        return (spikes, pattern_id, groups);
    }

    let freq = freq_hz.max(0.01) as f64;
    let duty = duty.clamp(0.0, 1.0) as f64;
    let phase_jitter = phase_jitter.clamp(0.0, 1.0) as f64;
    let dt_s = dt / 1000.0;
    let step = std::f64::consts::TAU * freq * dt_s;
    let thresh = 1.0 - duty;

    let mut phase = 0.0f64;
    let offsets: Vec<f64> = (0..num_sensory_neurons)
        .map(|i| {
            let h = (i as u64).wrapping_mul(6364136223846793005) & 0xffff;
            let base = (h as f64) / 65535.0;
            base * std::f64::consts::TAU * phase_jitter
        })
        .collect();

    for t in 0..steps {
        phase = (phase + step) % std::f64::consts::TAU;
        let gate = phase.sin() * 0.5 + 0.5;
        pattern_id[t] = if gate >= thresh { 1 } else { 0 };
        for i in 0..num_sensory_neurons {
            let gate_i = (phase + offsets[i]).sin() * 0.5 + 0.5;
            spikes[(t, i)] = if gate_i >= thresh { 1 } else { 0 };
        }
    }
    (spikes, pattern_id, groups)
}

fn apply_synaptic_filter(
    raw: &Array1<f64>,
    ampa: &mut Array1<f64>,
    nmda: &mut Array1<f64>,
    gaba: &mut Array1<f64>,
    decay_ampa: f64,
    decay_nmda: f64,
    decay_gaba: f64,
    nmda_ratio: f64,
    syn_gain: f64,
) -> Array1<f64> {
    let mut out = Array1::<f64>::zeros(raw.len());
    for i in 0..raw.len() {
        let val = raw[i];
        let exc = val.max(0.0);
        let inh = (-val).max(0.0);
        ampa[i] = ampa[i] * decay_ampa + exc * (1.0 - nmda_ratio);
        nmda[i] = nmda[i] * decay_nmda + exc * nmda_ratio;
        gaba[i] = gaba[i] * decay_gaba + inh;
        out[i] = (ampa[i] + nmda[i] - gaba[i]) * syn_gain;
    }
    out
}

fn stp_update_cpu(
    pre_spks: &[i8],
    u: &mut Array1<f64>,
    x: &mut Array1<f64>,
    release: &mut Array1<f64>,
    stp_u: f64,
    stp_rec_decay: f64,
    stp_facil_decay: f64,
) {
    for i in 0..pre_spks.len() {
        u[i] = u[i] * stp_facil_decay + stp_u * (1.0 - stp_facil_decay);
        x[i] = x[i] * stp_rec_decay + (1.0 - stp_rec_decay);
        if pre_spks[i] != 0 {
            let rel = (u[i] * x[i]).clamp(0.0, 1.0);
            x[i] = (x[i] - rel).max(0.0);
            u[i] = (u[i] + stp_u * (1.0 - u[i])).clamp(0.0, 1.0);
            release[i] = rel;
        } else {
            release[i] = 0.0;
        }
    }
}

#[cfg(feature = "opencl")]
struct ClStpContext {
    cl: Arc<OpenCLManager>,
    pre_spk_s: Buffer<i8>,
    u_s: Buffer<f64>,
    x_s: Buffer<f64>,
    rel_s: Buffer<f64>,
    pre_spk_h: Buffer<i8>,
    u_h: Vec<Buffer<f64>>,
    x_h: Vec<Buffer<f64>>,
    rel_h: Vec<Buffer<f64>>,
}

#[cfg(feature = "opencl")]
impl ClStpContext {
    fn new(
        cl: Arc<OpenCLManager>,
        num_sensory: usize,
        num_hidden: usize,
        num_hidden_layers: usize,
        stp_u: f64,
    ) -> ClResult<Self> {
        let pre_spk_s = unsafe {
            Buffer::create(
                &cl.context,
                CL_MEM_READ_ONLY,
                num_sensory * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )?
        };
        let mut u_s = unsafe {
            Buffer::create(
                &cl.context,
                CL_MEM_READ_WRITE,
                num_sensory * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )?
        };
        let mut x_s = unsafe {
            Buffer::create(
                &cl.context,
                CL_MEM_READ_WRITE,
                num_sensory * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )?
        };
        let rel_s = unsafe {
            Buffer::create(
                &cl.context,
                CL_MEM_READ_WRITE,
                num_sensory * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )?
        };
        let pre_spk_h = unsafe {
            Buffer::create(
                &cl.context,
                CL_MEM_READ_ONLY,
                num_hidden * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )?
        };
        let mut u_h = Vec::with_capacity(num_hidden_layers);
        let mut x_h = Vec::with_capacity(num_hidden_layers);
        let mut rel_h = Vec::with_capacity(num_hidden_layers);
        for _ in 0..num_hidden_layers {
            u_h.push(unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_WRITE,
                    num_hidden * std::mem::size_of::<f64>(),
                    ptr::null_mut(),
                )?
            });
            x_h.push(unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_WRITE,
                    num_hidden * std::mem::size_of::<f64>(),
                    ptr::null_mut(),
                )?
            });
            rel_h.push(unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_WRITE,
                    num_hidden * std::mem::size_of::<f64>(),
                    ptr::null_mut(),
                )?
            });
        }
        let u_s_init = vec![stp_u; num_sensory];
        let x_s_init = vec![1.0_f64; num_sensory];
        unsafe {
            cl.queue
                .enqueue_write_buffer(&mut u_s, CL_TRUE, 0, &u_s_init, &[])?;
            cl.queue
                .enqueue_write_buffer(&mut x_s, CL_TRUE, 0, &x_s_init, &[])?;
            for l in 0..num_hidden_layers {
                let u_h_init = vec![stp_u; num_hidden];
                let x_h_init = vec![1.0_f64; num_hidden];
                cl.queue
                    .enqueue_write_buffer(&mut u_h[l], CL_TRUE, 0, &u_h_init, &[])?;
                cl.queue
                    .enqueue_write_buffer(&mut x_h[l], CL_TRUE, 0, &x_h_init, &[])?;
            }
        }
        Ok(Self {
            cl,
            pre_spk_s,
            u_s,
            x_s,
            rel_s,
            pre_spk_h,
            u_h,
            x_h,
            rel_h,
        })
    }

    fn update_sensory(
        &mut self,
        pre_spks: &[i8],
        release_out: &mut [f64],
        stp_u: f64,
        stp_rec_decay: f64,
        stp_facil_decay: f64,
    ) -> ClResult<()> {
        cl_stp_update(
            &self.cl,
            &mut self.pre_spk_s,
            &mut self.u_s,
            &mut self.x_s,
            &mut self.rel_s,
            pre_spks,
            release_out,
            stp_u,
            stp_rec_decay,
            stp_facil_decay,
        )
    }

    fn update_hidden(
        &mut self,
        layer: usize,
        pre_spks: &[i8],
        release_out: &mut [f64],
        stp_u: f64,
        stp_rec_decay: f64,
        stp_facil_decay: f64,
    ) -> ClResult<()> {
        let u_buf = self
            .u_h
            .get_mut(layer)
            .ok_or_else(|| ClError::from(CL_INVALID_VALUE))?;
        let x_buf = self
            .x_h
            .get_mut(layer)
            .ok_or_else(|| ClError::from(CL_INVALID_VALUE))?;
        let rel_buf = self
            .rel_h
            .get_mut(layer)
            .ok_or_else(|| ClError::from(CL_INVALID_VALUE))?;
        cl_stp_update(
            &self.cl,
            &mut self.pre_spk_h,
            u_buf,
            x_buf,
            rel_buf,
            pre_spks,
            release_out,
            stp_u,
            stp_rec_decay,
            stp_facil_decay,
        )
    }

    fn sync_to_cpu(
        &mut self,
        u_s: &mut Array1<f64>,
        x_s: &mut Array1<f64>,
        u_h: &mut [Array1<f64>],
        x_h: &mut [Array1<f64>],
    ) -> ClResult<()> {
        if let (Some(u_s_slice), Some(x_s_slice)) = (u_s.as_slice_mut(), x_s.as_slice_mut()) {
            unsafe {
                self.cl
                    .queue
                    .enqueue_read_buffer(&mut self.u_s, CL_TRUE, 0, u_s_slice, &[])?;
                self.cl
                    .queue
                    .enqueue_read_buffer(&mut self.x_s, CL_TRUE, 0, x_s_slice, &[])?;
            }
        }
        for l in 0..u_h.len() {
            if let (Some(u_h_slice), Some(x_h_slice)) =
                (u_h[l].as_slice_mut(), x_h[l].as_slice_mut())
            {
                unsafe {
                    self.cl.queue.enqueue_read_buffer(
                        &mut self.u_h[l],
                        CL_TRUE,
                        0,
                        u_h_slice,
                        &[],
                    )?;
                    self.cl.queue.enqueue_read_buffer(
                        &mut self.x_h[l],
                        CL_TRUE,
                        0,
                        x_h_slice,
                        &[],
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "opencl")]
fn cl_stp_update(
    cl: &OpenCLManager,
    pre_buf: &mut Buffer<i8>,
    u_buf: &mut Buffer<f64>,
    x_buf: &mut Buffer<f64>,
    rel_buf: &mut Buffer<f64>,
    pre_spks: &[i8],
    release_out: &mut [f64],
    stp_u: f64,
    stp_rec_decay: f64,
    stp_facil_decay: f64,
) -> ClResult<()> {
    unsafe {
        cl.queue
            .enqueue_write_buffer(pre_buf, CL_TRUE, 0, pre_spks, &[])?;
        let kernel = cl.kernel_stp_update.lock().unwrap();
        ExecuteKernel::new(&kernel)
            .set_arg(u_buf)
            .set_arg(x_buf)
            .set_arg(pre_buf)
            .set_arg(&mut *rel_buf)
            .set_arg(&stp_u)
            .set_arg(&stp_rec_decay)
            .set_arg(&stp_facil_decay)
            .set_global_work_size(release_out.len())
            .enqueue_nd_range(&cl.queue)?;
        cl.queue
            .enqueue_read_buffer(rel_buf, CL_TRUE, 0, release_out, &[])?;
    }
    Ok(())
}

#[cfg(feature = "opencl")]
fn disable_cl_stp(
    cl_stp: &mut Option<ClStpContext>,
    stp_u_sensory: &mut Option<Array1<f64>>,
    stp_x_sensory: &mut Option<Array1<f64>>,
    stp_u_hidden: &mut Vec<Array1<f64>>,
    stp_x_hidden: &mut Vec<Array1<f64>>,
) {
    if let Some(mut ctx) = cl_stp.take() {
        if let (Some(u_s), Some(x_s)) = (stp_u_sensory.as_mut(), stp_x_sensory.as_mut()) {
            if let Err(e) = ctx.sync_to_cpu(u_s, x_s, stp_u_hidden, stp_x_hidden) {
                nm_log!("[warn] OpenCL STP sync failed: {:?}", e);
            }
        }
    }
}

#[inline]

/// Core simulation engine for the Spiking Neural Network (SNN).
///
/// This function executes a time-stepping simulation of the entire network,
/// including membrane potential updates, spike generation, and synaptic plasticity.
/// It operates in "batch mode" using fixed-size matrices for maximum throughput.
///
/// # Workflow
/// For each time step `dt`:
/// 1. Update pre-synaptic and post-synaptic traces for STDP/Hebbian learning.
/// 2. Update membrane potentials for all hidden and output neurons based on
///    incoming spikes from the previous step.
/// 3. Determine which neurons spike (cross threshold).
/// 4. Apply learning rules to adjust synaptic weights based on spike timing.
/// 5. Record spikes and prepare for the next time step.
///
/// # Arguments
/// * `t_ms`: Total simulation time in milliseconds.
/// * `lif`: Parameters for the LIF neuron model.
/// * `stdp`: Parameters for the STDP learning rule.
/// * `cfg`: Network topology configuration.
/// * `built`: The initial weight matrices.
/// * `sensory_spikes`: Pre-generated input spike trains for the sensory layer.
/// * `neuron_model`: The specific mathematical model to use for hidden/output neurons.
/// * `learning`: The selected synaptic plasticity rule.
pub fn run_snn(
    t_ms: f64,
    lif: &LIFParams,
    stdp: &STDPParams,
    cfg: &NetworkConfig,
    mut built: BuiltNetwork,
    sensory_spikes: &Array2<i8>,
    neuron_model: NeuronModel,
    learning: Learning,
) -> SimOut {
    let izh_params = match neuron_model {
        NeuronModel::Izh(p) => Some(p),
        NeuronModel::Aarnn => Some(IzhikevichParams::from_preset("RS", lif.dt)),
        _ => None,
    };
    let dt = lif.dt;
    let steps = (t_ms / dt).round() as usize;
    let num_hidden_layers = cfg.num_hidden_layers;
    let num_hidden_per_layer = cfg.num_hidden_per_layer_initial;
    let num_output_neurons = cfg.num_output_neurons;
    let num_sensory_neurons = cfg.num_sensory_neurons;
    let aarnn_depth = cfg.aarnn_layer_depth;
    let use_aarnn_bio = matches!(neuron_model, NeuronModel::Aarnn) && aarnn_depth > 0;
    let bio = cfg.aarnn_bio.clone();
    let use_synaptic_filter = use_aarnn_bio && aarnn_depth >= 1;
    let use_stp = use_aarnn_bio && aarnn_depth >= 1 && bio.stp_enabled;
    let use_adaptive_threshold =
        use_aarnn_bio && aarnn_depth >= 2 && bio.adaptive_threshold_enabled;
    let use_homeostasis = use_aarnn_bio && aarnn_depth >= 2 && bio.homeostasis_gain > 0.0;
    let use_izh_refractory = use_aarnn_bio && aarnn_depth >= 2 && bio.izh_refractory_ms > 0.0;

    // Identify which hidden layers connect to Sensory inputs and Output nodes.
    let in_l = cfg
        .sensory_target_layer
        .unwrap_or_else(|| {
            if matches!(neuron_model, NeuronModel::Aarnn) {
                if num_hidden_layers > 1 {
                    1
                } else {
                    0
                }
            } else {
                0
            }
        })
        .min(num_hidden_layers.saturating_sub(1));

    let out_l = cfg
        .output_source_layer
        .unwrap_or_else(|| {
            if matches!(neuron_model, NeuronModel::Aarnn) {
                if num_hidden_layers > 4 {
                    4
                } else {
                    num_hidden_layers.saturating_sub(1)
                }
            } else {
                num_hidden_layers.saturating_sub(1)
            }
        })
        .min(num_hidden_layers.saturating_sub(1));

    let mut hidden_membrane_potentials: Vec<Array1<f64>> = (0..num_hidden_layers)
        .map(|_| Array1::zeros(num_hidden_per_layer))
        .collect();
    let mut hidden_recovery_variables: Option<Vec<Array1<f64>>> = if izh_params.is_some() {
        Some(
            (0..num_hidden_layers)
                .map(|_| Array1::zeros(num_hidden_per_layer))
                .collect(),
        )
    } else {
        None
    };
    let mut output_membrane_potentials = Array1::<f64>::zeros(num_output_neurons);
    let mut output_recovery_variables: Option<Array1<f64>> = if izh_params.is_some() {
        Some(Array1::zeros(num_output_neurons))
    } else {
        None
    };
    let mut hidden_refractory_counters: Option<Vec<Array1<i32>>> = match neuron_model {
        NeuronModel::Lif => Some(
            (0..num_hidden_layers)
                .map(|_| Array1::zeros(num_hidden_per_layer))
                .collect(),
        ),
        _ => None,
    };
    let mut output_refractory_counters: Option<Array1<i32>> = match neuron_model {
        NeuronModel::Lif => Some(Array1::zeros(num_output_neurons)),
        _ => None,
    };

    // traces
    let mut sensory_pre_synaptic_traces = Array1::<f64>::zeros(num_sensory_neurons);
    let mut hidden_post_synaptic_traces: Vec<Array1<f64>> = (0..num_hidden_layers)
        .map(|_| Array1::zeros(num_hidden_per_layer))
        .collect();
    let mut hidden_pre_synaptic_traces: Vec<Array1<f64>> = (0..num_hidden_layers)
        .map(|_| Array1::zeros(num_hidden_per_layer))
        .collect();
    let mut output_post_synaptic_traces = Array1::<f64>::zeros(num_output_neurons);

    let mut syn_ampa_h: Vec<Array1<f64>> = if use_synaptic_filter {
        (0..num_hidden_layers)
            .map(|_| Array1::zeros(num_hidden_per_layer))
            .collect()
    } else {
        Vec::new()
    };
    let mut syn_nmda_h: Vec<Array1<f64>> = if use_synaptic_filter {
        (0..num_hidden_layers)
            .map(|_| Array1::zeros(num_hidden_per_layer))
            .collect()
    } else {
        Vec::new()
    };
    let mut syn_gaba_h: Vec<Array1<f64>> = if use_synaptic_filter {
        (0..num_hidden_layers)
            .map(|_| Array1::zeros(num_hidden_per_layer))
            .collect()
    } else {
        Vec::new()
    };
    let mut syn_ampa_o = if use_synaptic_filter {
        Array1::<f64>::zeros(num_output_neurons)
    } else {
        Array1::<f64>::zeros(0)
    };
    let mut syn_nmda_o = if use_synaptic_filter {
        Array1::<f64>::zeros(num_output_neurons)
    } else {
        Array1::<f64>::zeros(0)
    };
    let mut syn_gaba_o = if use_synaptic_filter {
        Array1::<f64>::zeros(num_output_neurons)
    } else {
        Array1::<f64>::zeros(0)
    };
    let mut thr_offset_h: Vec<Array1<f64>> = if use_adaptive_threshold || use_homeostasis {
        (0..num_hidden_layers)
            .map(|_| Array1::zeros(num_hidden_per_layer))
            .collect()
    } else {
        Vec::new()
    };
    let mut thr_offset_o = if use_adaptive_threshold || use_homeostasis {
        Array1::<f64>::zeros(num_output_neurons)
    } else {
        Array1::<f64>::zeros(0)
    };
    let mut rate_ema_h: Vec<Array1<f64>> = if use_homeostasis {
        (0..num_hidden_layers)
            .map(|_| Array1::zeros(num_hidden_per_layer))
            .collect()
    } else {
        Vec::new()
    };
    let mut rate_ema_o = if use_homeostasis {
        Array1::<f64>::zeros(num_output_neurons)
    } else {
        Array1::<f64>::zeros(0)
    };
    let mut stp_u_sensory = if use_stp {
        Some(Array1::<f64>::from_elem(num_sensory_neurons, bio.stp_u))
    } else {
        None
    };
    let mut stp_x_sensory = if use_stp {
        Some(Array1::<f64>::from_elem(num_sensory_neurons, 1.0))
    } else {
        None
    };
    let mut stp_u_hidden: Vec<Array1<f64>> = if use_stp {
        (0..num_hidden_layers)
            .map(|_| Array1::<f64>::from_elem(num_hidden_per_layer, bio.stp_u))
            .collect()
    } else {
        Vec::new()
    };
    let mut stp_x_hidden: Vec<Array1<f64>> = if use_stp {
        (0..num_hidden_layers)
            .map(|_| Array1::<f64>::from_elem(num_hidden_per_layer, 1.0))
            .collect()
    } else {
        Vec::new()
    };
    let mut hidden_izh_refractory: Option<Vec<Array1<i32>>> = if use_izh_refractory {
        Some(
            (0..num_hidden_layers)
                .map(|_| Array1::<i32>::zeros(num_hidden_per_layer))
                .collect(),
        )
    } else {
        None
    };
    let mut output_izh_refractory: Option<Array1<i32>> = if use_izh_refractory {
        Some(Array1::<i32>::zeros(num_output_neurons))
    } else {
        None
    };

    // recordings
    let mut hidden_spike_recordings: Vec<Array2<i8>> = (0..num_hidden_layers)
        .map(|_| Array2::<i8>::zeros((steps, num_hidden_per_layer)))
        .collect();
    let mut output_spike_recordings = Array2::<i8>::zeros((steps, num_output_neurons));

    // decays
    let membrane_decay_factor = (-dt / lif.tau_m).exp();
    let pre_synaptic_trace_decay_factor = (-dt / stdp.tau_pre).exp();
    let post_synaptic_trace_decay_factor = (-dt / stdp.tau_post).exp();
    let stp_rec_decay = if use_stp {
        (-dt / bio.stp_tau_rec_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    let stp_facil_decay = if use_stp {
        (-dt / bio.stp_tau_facil_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    #[cfg(feature = "opencl")]
    let mut cl_stp: Option<ClStpContext> = None;
    #[cfg(feature = "opencl")]
    if use_stp {
        if let Some(cl) = get_global_cl_manager() {
            match ClStpContext::new(
                cl,
                num_sensory_neurons,
                num_hidden_per_layer,
                num_hidden_layers,
                bio.stp_u,
            ) {
                Ok(ctx) => cl_stp = Some(ctx),
                Err(e) => nm_log!("[warn] OpenCL STP init failed: {:?}", e),
            }
        }
    }
    let syn_decay_ampa = if use_synaptic_filter {
        (-dt / bio.ampa_tau_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    let syn_decay_nmda = if use_synaptic_filter {
        (-dt / bio.nmda_tau_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    let syn_decay_gaba = if use_synaptic_filter {
        (-dt / bio.gaba_tau_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    let thr_decay = if use_adaptive_threshold {
        (-dt / bio.adaptive_threshold_tau_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    let homeo_decay = if use_homeostasis {
        (-dt / bio.homeostasis_tau_ms.max(1e-6)).exp()
    } else {
        0.0
    };
    let base_homeo_target = if use_homeostasis {
        bio.homeostasis_target_rate_hz * dt / 1000.0
    } else {
        0.0
    };
    let neuromod_plasticity_gain = if use_aarnn_bio && bio.neuromodulation_enabled {
        (bio.dopamine_gain / bio.serotonin_gain.max(1e-6)).max(0.0)
    } else {
        1.0
    };
    let neuromod_excitability_gain = if use_aarnn_bio && bio.neuromodulation_enabled {
        bio.acetylcholine_gain.max(0.0)
    } else {
        1.0
    };
    let izh_refractory_steps = if use_izh_refractory {
        (bio.izh_refractory_ms / dt).round() as i32
    } else {
        0
    };

    // helper for izh integration
    // (removed unused helpers)

    let progress_interval = (steps / 10).max(1);
    // --- Longterm connection tracking ---
    // For each connection, track how many steps it is present (weight > 1e-8)
    let mut conn_presence_in =
        vec![vec![0; cfg.num_sensory_neurons]; cfg.num_hidden_per_layer_initial];
    let mut conn_presence_fwd =
        vec![
            vec![vec![0; cfg.num_hidden_per_layer_initial]; cfg.num_hidden_per_layer_initial];
            cfg.num_hidden_layers.saturating_sub(1)
        ];
    let mut conn_presence_bwd =
        vec![
            vec![vec![0; cfg.num_hidden_per_layer_initial]; cfg.num_hidden_per_layer_initial];
            cfg.num_hidden_layers.saturating_sub(1)
        ];
    let mut conn_presence_out =
        vec![vec![0; cfg.num_hidden_per_layer_initial]; cfg.num_output_neurons];

    for t in 0..steps {
        if t % progress_interval == 0 {
            nm_log!(
                "[info] Simulation progress: {}% (step {}/{})",
                (t * 100) / steps,
                t,
                steps
            );
        }

        // --- 1. Current Step Inputs ---
        // Fetch input spikes from the sensory layer for the current time step.
        let sensory_spikes_at_step = sensory_spikes.slice(s![t, ..]);
        let mut sensory_release = if use_stp {
            Array1::<f64>::zeros(num_sensory_neurons)
        } else {
            Array1::<f64>::zeros(0)
        };
        let mut stp_release_hidden: Vec<Array1<f64>> = if use_stp {
            (0..num_hidden_layers)
                .map(|_| Array1::<f64>::zeros(num_hidden_per_layer))
                .collect()
        } else {
            Vec::new()
        };

        if use_stp {
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut use_cpu = true;
            #[cfg(feature = "opencl")]
            if let Some(ctx) = cl_stp.as_mut() {
                let pre_slice = sensory_spikes_at_step.as_slice().unwrap();
                let rel_slice = sensory_release.as_slice_mut().unwrap();
                if let Err(e) = ctx.update_sensory(
                    pre_slice,
                    rel_slice,
                    bio.stp_u,
                    stp_rec_decay,
                    stp_facil_decay,
                ) {
                    nm_log!("[warn] OpenCL STP sensory update failed: {:?}", e);
                    disable_cl_stp(
                        &mut cl_stp,
                        &mut stp_u_sensory,
                        &mut stp_x_sensory,
                        &mut stp_u_hidden,
                        &mut stp_x_hidden,
                    );
                } else {
                    use_cpu = false;
                }
            }
            if use_cpu {
                if let (Some(u), Some(x)) = (stp_u_sensory.as_mut(), stp_x_sensory.as_mut()) {
                    stp_update_cpu(
                        sensory_spikes_at_step.as_slice().unwrap(),
                        u,
                        x,
                        &mut sensory_release,
                        bio.stp_u,
                        stp_rec_decay,
                        stp_facil_decay,
                    );
                }
            }
        }

        if use_adaptive_threshold {
            for l in 0..num_hidden_layers {
                thr_offset_h[l].mapv_inplace(|x| x * thr_decay);
            }
            thr_offset_o.mapv_inplace(|x| x * thr_decay);
        }
        if use_homeostasis {
            for l in 0..num_hidden_layers {
                rate_ema_h[l].mapv_inplace(|x| x * homeo_decay);
            }
            rate_ema_o.mapv_inplace(|x| x * homeo_decay);
        }

        // --- 2. Trace Updates (for Learning) ---
        // Decay existing synaptic traces and increment based on new activity.
        sensory_pre_synaptic_traces.mapv_inplace(|x| x * pre_synaptic_trace_decay_factor);
        for l in 0..num_hidden_layers {
            hidden_post_synaptic_traces[l].mapv_inplace(|x| x * post_synaptic_trace_decay_factor);
            hidden_pre_synaptic_traces[l].mapv_inplace(|x| x * pre_synaptic_trace_decay_factor);
        }
        output_post_synaptic_traces.mapv_inplace(|x| x * post_synaptic_trace_decay_factor);
        for i in 0..num_sensory_neurons {
            if sensory_spikes_at_step[i] != 0 {
                sensory_pre_synaptic_traces[i] += 1.0;
            }
        }

        // --- 3. Hidden Layer 0 Update ---
        // Calculate incoming current (sensory if in_l == 0).
        let mut hidden_layer_0_currents = Array1::<f64>::zeros(num_hidden_per_layer);
        if in_l == 0 {
            for j in 0..num_hidden_per_layer {
                let mut acc = 0.0;
                for i in 0..num_sensory_neurons {
                    let spike_val = if use_stp {
                        sensory_release[i]
                    } else if sensory_spikes[(t, i)] != 0 {
                        1.0
                    } else {
                        0.0
                    };
                    if spike_val != 0.0 {
                        acc += built.w_in[(j, i)] * spike_val;
                    }
                }
                hidden_layer_0_currents[j] = acc;
            }
        }
        if use_synaptic_filter {
            hidden_layer_0_currents = apply_synaptic_filter(
                &hidden_layer_0_currents,
                &mut syn_ampa_h[0],
                &mut syn_nmda_h[0],
                &mut syn_gaba_h[0],
                syn_decay_ampa,
                syn_decay_nmda,
                syn_decay_gaba,
                bio.nmda_ratio,
                bio.synaptic_gain * neuromod_excitability_gain,
            );
        }

        // Integrate membrane dynamics for H0 neurons.
        let hidden_layer_0_spikes_at_step: Array1<i8> = match neuron_model {
            NeuronModel::Lif => {
                let mut r = Array1::<i8>::zeros(num_hidden_per_layer);
                let refh = hidden_refractory_counters.as_mut().unwrap();
                for j in 0..num_hidden_per_layer {
                    let v = hidden_membrane_potentials[0][j] * membrane_decay_factor
                        + hidden_layer_0_currents[j];
                    hidden_membrane_potentials[0][j] = v.clamp(-5.0, 5.0);
                    let active = refh[0][j] <= 0;
                    let fired = active && hidden_membrane_potentials[0][j] >= lif.v_th;
                    if fired {
                        hidden_membrane_potentials[0][j] = lif.v_reset;
                        refh[0][j] = lif.refractory as i32;
                    } else {
                        refh[0][j] = (refh[0][j] - 1).max(0);
                    }
                    r[j] = fired as i8;
                }
                r
            }
            NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                let p = izh_params.expect("izh params for Izh/AARNN");
                let mut r = Array1::<i8>::zeros(num_hidden_per_layer);
                let u0 = hidden_recovery_variables.as_mut().unwrap();
                for j in 0..num_hidden_per_layer {
                    let v = hidden_membrane_potentials[0][j];
                    let u = u0[0][j];
                    // Euler integration of Izhikevich dynamics
                    let nv = v + p.dt
                        * (0.04 * v * v + 5.0 * v + 140.0 - u + hidden_layer_0_currents[j]);
                    let nu = u + p.dt
                        * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * nv - u));
                    let mut fired = nv >= p.v_th;
                    if use_adaptive_threshold {
                        let thr_offset = thr_offset_h[0][j]
                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                        fired = nv >= (p.v_th + thr_offset);
                    }
                    if let Some(refh) = hidden_izh_refractory.as_mut() {
                        if refh[0][j] > 0 {
                            refh[0][j] -= 1;
                            fired = false;
                        }
                    }
                    let (nv2, nu2) = if fired {
                        (p.membrane_reset_potential_c, nu + p.recovery_increment_d)
                    } else {
                        (nv, nu)
                    };
                    hidden_membrane_potentials[0][j] = nv2;
                    u0[0][j] = nu2;
                    r[j] = fired as i8;
                    if fired && use_adaptive_threshold {
                        thr_offset_h[0][j] = (thr_offset_h[0][j]
                            + bio.adaptive_threshold_increment)
                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                    }
                    if fired {
                        if let Some(refh) = hidden_izh_refractory.as_mut() {
                            refh[0][j] = izh_refractory_steps;
                        }
                    }
                }
                r
            }
        };
        // Record spikes and update traces for H0.
        for j in 0..num_hidden_per_layer {
            hidden_spike_recordings[0][(t, j)] = hidden_layer_0_spikes_at_step[j];
            if hidden_layer_0_spikes_at_step[j] != 0 {
                hidden_post_synaptic_traces[0][j] += 1.0;
                hidden_pre_synaptic_traces[0][j] += 1.0;
            }
        }
        if use_stp {
            #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
            let mut use_cpu = true;
            #[cfg(feature = "opencl")]
            if let Some(ctx) = cl_stp.as_mut() {
                let pre_slice = hidden_layer_0_spikes_at_step.as_slice().unwrap();
                let rel_slice = stp_release_hidden[0].as_slice_mut().unwrap();
                if let Err(e) = ctx.update_hidden(
                    0,
                    pre_slice,
                    rel_slice,
                    bio.stp_u,
                    stp_rec_decay,
                    stp_facil_decay,
                ) {
                    nm_log!("[warn] OpenCL STP hidden[0] update failed: {:?}", e);
                    disable_cl_stp(
                        &mut cl_stp,
                        &mut stp_u_sensory,
                        &mut stp_x_sensory,
                        &mut stp_u_hidden,
                        &mut stp_x_hidden,
                    );
                } else {
                    use_cpu = false;
                }
            }
            if use_cpu {
                stp_update_cpu(
                    hidden_layer_0_spikes_at_step.as_slice().unwrap(),
                    &mut stp_u_hidden[0],
                    &mut stp_x_hidden[0],
                    &mut stp_release_hidden[0],
                    bio.stp_u,
                    stp_rec_decay,
                    stp_facil_decay,
                );
            }
        }
        if use_homeostasis {
            for j in 0..num_hidden_per_layer {
                if hidden_layer_0_spikes_at_step[j] != 0 {
                    rate_ema_h[0][j] += 1.0 - homeo_decay;
                }
                let err = rate_ema_h[0][j] - base_homeo_target;
                thr_offset_h[0][j] += bio.homeostasis_gain * err;
            }
        }

        // --- 4. Subsequent Hidden Layers Update ---
        for l in 1..num_hidden_layers {
            // forward current from current spikes of l-1
            let mut forward_currents_at_step = Array1::<f64>::zeros(num_hidden_per_layer);

            // Add sensory if this is the target layer
            if l == in_l {
                for j in 0..num_hidden_per_layer {
                    let mut acc = 0.0;
                    for i in 0..num_sensory_neurons {
                        let spike_val = if use_stp {
                            sensory_release[i]
                        } else if sensory_spikes[(t, i)] != 0 {
                            1.0
                        } else {
                            0.0
                        };
                        if spike_val != 0.0 {
                            acc += built.w_in[(j, i)] * spike_val;
                        }
                    }
                    forward_currents_at_step[j] = acc;
                }
            }

            for j in 0..num_hidden_per_layer {
                let mut acc = forward_currents_at_step[j];
                for i in 0..num_hidden_per_layer {
                    let spike_val = if use_stp {
                        stp_release_hidden[l - 1][i]
                    } else if hidden_spike_recordings[l - 1][(t, i)] != 0 {
                        1.0
                    } else {
                        0.0
                    };
                    if spike_val != 0.0 {
                        acc += built.w_hh_fwd[l - 1][(j, i)] * spike_val;
                    }
                }
                forward_currents_at_step[j] = acc;
            }
            // backward from prev step of l+1
            let mut backward_currents_at_step = Array1::<f64>::zeros(num_hidden_per_layer);
            if l < num_hidden_layers - 1 {
                for j in 0..num_hidden_per_layer {
                    let mut acc = 0.0;
                    for k in 0..num_hidden_per_layer {
                        let prv = if t > 0 {
                            hidden_spike_recordings[l + 1][(t - 1, k)] != 0
                        } else {
                            false
                        };
                        if prv {
                            acc += built.w_hh_bwd[l][(j, k)];
                        }
                    }
                    backward_currents_at_step[j] = acc;
                }
            }
            if use_synaptic_filter {
                let mut combined = forward_currents_at_step.clone();
                for j in 0..num_hidden_per_layer {
                    combined[j] += backward_currents_at_step[j];
                }
                let filtered = apply_synaptic_filter(
                    &combined,
                    &mut syn_ampa_h[l],
                    &mut syn_nmda_h[l],
                    &mut syn_gaba_h[l],
                    syn_decay_ampa,
                    syn_decay_nmda,
                    syn_decay_gaba,
                    bio.nmda_ratio,
                    bio.synaptic_gain * neuromod_excitability_gain,
                );
                for j in 0..num_hidden_per_layer {
                    forward_currents_at_step[j] = filtered[j];
                    backward_currents_at_step[j] = 0.0;
                }
            }
            let mut spk = Array1::<i8>::zeros(num_hidden_per_layer);
            match neuron_model {
                NeuronModel::Lif => {
                    let refh = hidden_refractory_counters.as_mut().unwrap();
                    for j in 0..num_hidden_per_layer {
                        let v = hidden_membrane_potentials[l][j] * membrane_decay_factor
                            + forward_currents_at_step[j]
                            + backward_currents_at_step[j];
                        hidden_membrane_potentials[l][j] = v.clamp(-5.0, 5.0);
                        let active = refh[l][j] <= 0;
                        let fired = active && hidden_membrane_potentials[l][j] >= lif.v_th;
                        if fired {
                            hidden_membrane_potentials[l][j] = lif.v_reset;
                            refh[l][j] = lif.refractory as i32;
                        } else {
                            refh[l][j] = (refh[l][j] - 1).max(0);
                        }
                        spk[j] = fired as i8;
                    }
                }
                NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                    let p = izh_params.expect("izh params for Izh/AARNN");
                    let uh = hidden_recovery_variables.as_mut().unwrap();
                    for j in 0..num_hidden_per_layer {
                        let v = hidden_membrane_potentials[l][j];
                        let u = uh[l][j];
                        let nv = v + p.dt
                            * (0.04 * v * v + 5.0 * v + 140.0 - u
                                + forward_currents_at_step[j]
                                + backward_currents_at_step[j]);
                        let nu = u + p.dt
                            * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * nv - u));
                        let mut fired = nv >= p.v_th;
                        if use_adaptive_threshold {
                            let thr_offset = thr_offset_h[l][j]
                                .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                            fired = nv >= (p.v_th + thr_offset);
                        }
                        if let Some(refh) = hidden_izh_refractory.as_mut() {
                            if refh[l][j] > 0 {
                                refh[l][j] -= 1;
                                fired = false;
                            }
                        }
                        let (nv2, nu2) = if fired {
                            (p.membrane_reset_potential_c, nu + p.recovery_increment_d)
                        } else {
                            (nv, nu)
                        };
                        hidden_membrane_potentials[l][j] = nv2;
                        uh[l][j] = nu2;
                        spk[j] = fired as i8;
                        if fired && use_adaptive_threshold {
                            thr_offset_h[l][j] = (thr_offset_h[l][j]
                                + bio.adaptive_threshold_increment)
                                .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                        }
                        if fired {
                            if let Some(refh) = hidden_izh_refractory.as_mut() {
                                refh[l][j] = izh_refractory_steps;
                            }
                        }
                    }
                }
            }
            for j in 0..num_hidden_per_layer {
                hidden_spike_recordings[l][(t, j)] = spk[j];
                if spk[j] != 0 {
                    hidden_post_synaptic_traces[l][j] += 1.0;
                    hidden_pre_synaptic_traces[l][j] += 1.0;
                }
            }
            if use_stp {
                #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
                let mut use_cpu = true;
                #[cfg(feature = "opencl")]
                if let Some(ctx) = cl_stp.as_mut() {
                    let pre_slice = spk.as_slice().unwrap();
                    let rel_slice = stp_release_hidden[l].as_slice_mut().unwrap();
                    if let Err(e) = ctx.update_hidden(
                        l,
                        pre_slice,
                        rel_slice,
                        bio.stp_u,
                        stp_rec_decay,
                        stp_facil_decay,
                    ) {
                        nm_log!("[warn] OpenCL STP hidden[{}] update failed: {:?}", l, e);
                        disable_cl_stp(
                            &mut cl_stp,
                            &mut stp_u_sensory,
                            &mut stp_x_sensory,
                            &mut stp_u_hidden,
                            &mut stp_x_hidden,
                        );
                    } else {
                        use_cpu = false;
                    }
                }
                if use_cpu {
                    stp_update_cpu(
                        spk.as_slice().unwrap(),
                        &mut stp_u_hidden[l],
                        &mut stp_x_hidden[l],
                        &mut stp_release_hidden[l],
                        bio.stp_u,
                        stp_rec_decay,
                        stp_facil_decay,
                    );
                }
            }
            if use_homeostasis {
                for j in 0..num_hidden_per_layer {
                    if spk[j] != 0 {
                        rate_ema_h[l][j] += 1.0 - homeo_decay;
                    }
                    let err = rate_ema_h[l][j] - base_homeo_target;
                    thr_offset_h[l][j] += bio.homeostasis_gain * err;
                }
            }
        }

        // output
        let mut output_layer_currents_at_step = Array1::<f64>::zeros(num_output_neurons);
        for k in 0..num_output_neurons {
            let mut acc = 0.0;
            for j in 0..num_hidden_per_layer {
                let spike_val = if use_stp {
                    stp_release_hidden[out_l][j]
                } else if hidden_spike_recordings[out_l][(t, j)] != 0 {
                    1.0
                } else {
                    0.0
                };
                if spike_val != 0.0 {
                    acc += built.w_out[(k, j)] * spike_val;
                }
            }
            output_layer_currents_at_step[k] = acc;
        }
        if use_synaptic_filter {
            output_layer_currents_at_step = apply_synaptic_filter(
                &output_layer_currents_at_step,
                &mut syn_ampa_o,
                &mut syn_nmda_o,
                &mut syn_gaba_o,
                syn_decay_ampa,
                syn_decay_nmda,
                syn_decay_gaba,
                bio.nmda_ratio,
                bio.synaptic_gain * neuromod_excitability_gain,
            );
        }
        let output_layer_spikes_at_step: Array1<i8> = match neuron_model {
            NeuronModel::Lif => {
                let mut r = Array1::<i8>::zeros(num_output_neurons);
                let ro = output_refractory_counters.as_mut().unwrap();
                for k in 0..num_output_neurons {
                    let v = output_membrane_potentials[k] * membrane_decay_factor
                        + output_layer_currents_at_step[k];
                    output_membrane_potentials[k] = v.clamp(-5.0, 5.0);
                    let active = ro[k] <= 0;
                    let fired = active && output_membrane_potentials[k] >= lif.v_th;
                    if fired {
                        output_membrane_potentials[k] = lif.v_reset;
                        ro[k] = lif.refractory as i32;
                    } else {
                        ro[k] = (ro[k] - 1).max(0);
                    }
                    r[k] = fired as i8;
                }
                r
            }
            NeuronModel::Izh(_) | NeuronModel::Aarnn => {
                let p = izh_params.expect("izh params for Izh/AARNN");
                let mut r = Array1::<i8>::zeros(num_output_neurons);
                let u = output_recovery_variables.as_mut().unwrap();
                for k in 0..num_output_neurons {
                    let v = output_membrane_potentials[k];
                    let uu = u[k];
                    let nv = v + p.dt
                        * (0.04 * v * v + 5.0 * v + 140.0 - uu + output_layer_currents_at_step[k]);
                    let nu = uu
                        + p.dt
                            * (p.recovery_time_constant_a * (p.recovery_sensitivity_b * nv - uu));
                    let mut fired = nv >= p.v_th;
                    if use_adaptive_threshold {
                        let thr_offset = thr_offset_o[k]
                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                        fired = nv >= (p.v_th + thr_offset);
                    }
                    if let Some(ro) = output_izh_refractory.as_mut() {
                        if ro[k] > 0 {
                            ro[k] -= 1;
                            fired = false;
                        }
                    }
                    let (nv2, nu2) = if fired {
                        (p.membrane_reset_potential_c, nu + p.recovery_increment_d)
                    } else {
                        (nv, nu)
                    };
                    output_membrane_potentials[k] = nv2;
                    u[k] = nu2;
                    r[k] = fired as i8;
                    if fired && use_adaptive_threshold {
                        thr_offset_o[k] = (thr_offset_o[k] + bio.adaptive_threshold_increment)
                            .clamp(bio.adaptive_threshold_min, bio.adaptive_threshold_max);
                    }
                    if fired {
                        if let Some(ro) = output_izh_refractory.as_mut() {
                            ro[k] = izh_refractory_steps;
                        }
                    }
                }
                r
            }
        };
        for k in 0..num_output_neurons {
            output_spike_recordings[(t, k)] = output_layer_spikes_at_step[k];
            if output_layer_spikes_at_step[k] != 0 {
                output_post_synaptic_traces[k] += 1.0;
            }
        }
        if use_homeostasis {
            for k in 0..num_output_neurons {
                if output_layer_spikes_at_step[k] != 0 {
                    rate_ema_o[k] += 1.0 - homeo_decay;
                }
                let err = rate_ema_o[k] - base_homeo_target;
                thr_offset_o[k] += bio.homeostasis_gain * err;
            }
        }

        // learning
        let eta = stdp.eta * neuromod_plasticity_gain;
        // W_in (targets in_l)
        for j in 0..num_hidden_per_layer {
            for i in 0..num_sensory_neurons {
                let pre = if sensory_spikes[(t, i)] != 0 {
                    1.0
                } else {
                    0.0
                };
                let post = if hidden_spike_recordings[in_l][(t, j)] != 0 {
                    1.0
                } else {
                    0.0
                };
                let dw = match learning {
                    Learning::Stdp | Learning::Aarnn => {
                        eta * ((post * sensory_pre_synaptic_traces[i])
                            - (pre * hidden_post_synaptic_traces[in_l][j]))
                    }
                    Learning::Hebb => eta * (post * pre),
                    Learning::Oja => eta * ((post * pre) - (post * post) * built.w_in[(j, i)]),
                };
                built.w_in[(j, i)] = (built.w_in[(j, i)] + dw).clamp(stdp.w_min, stdp.w_max);
                if built.w_in[(j, i)].abs() > 1e-8 {
                    conn_presence_in[j][i] += 1;
                }
            }
        }
        // Hidden forward/backward
        for l in 0..num_hidden_layers - 1 {
            for j in 0..num_hidden_per_layer {
                for i in 0..num_hidden_per_layer {
                    let pre = if hidden_spike_recordings[l][(t, i)] != 0 {
                        1.0
                    } else {
                        0.0
                    };
                    let post = if hidden_spike_recordings[l + 1][(t, j)] != 0 {
                        1.0
                    } else {
                        0.0
                    };
                    let dwf = match learning {
                        Learning::Stdp | Learning::Aarnn => {
                            eta * ((post * hidden_pre_synaptic_traces[l][i])
                                - (pre * hidden_post_synaptic_traces[l + 1][j]))
                        }
                        Learning::Hebb => eta * (post * pre),
                        Learning::Oja => {
                            eta * ((post * pre) - (post * post) * built.w_hh_fwd[l][(j, i)])
                        }
                    };
                    built.w_hh_fwd[l][(j, i)] =
                        (built.w_hh_fwd[l][(j, i)] + dwf).clamp(stdp.w_min, stdp.w_max);
                    if built.w_hh_fwd[l][(j, i)].abs() > 1e-8 {
                        conn_presence_fwd[l][j][i] += 1;
                    }
                    let dwb = match learning {
                        Learning::Stdp | Learning::Aarnn => {
                            eta * ((pre * hidden_pre_synaptic_traces[l + 1][j])
                                - (post * hidden_post_synaptic_traces[l][i]))
                        }
                        Learning::Hebb => eta * (post * pre),
                        Learning::Oja => {
                            eta * ((post * pre) - (post * post) * built.w_hh_bwd[l][(i, j)])
                        }
                    };
                    built.w_hh_bwd[l][(i, j)] =
                        (built.w_hh_bwd[l][(i, j)] + dwb).clamp(stdp.w_min, stdp.w_max);
                    if built.w_hh_bwd[l][(i, j)].abs() > 1e-8 {
                        conn_presence_bwd[l][i][j] += 1;
                    }
                }
            }
        }
        // W_out (sourced from out_l)
        for k in 0..num_output_neurons {
            for j in 0..num_hidden_per_layer {
                let pre = if hidden_spike_recordings[out_l][(t, j)] != 0 {
                    1.0
                } else {
                    0.0
                };
                let post = if output_layer_spikes_at_step[k] != 0 {
                    1.0
                } else {
                    0.0
                };
                let dw = match learning {
                    Learning::Stdp | Learning::Aarnn => {
                        eta * ((post * hidden_pre_synaptic_traces[out_l][j])
                            - (pre * output_post_synaptic_traces[k]))
                    }
                    Learning::Hebb => eta * (post * pre),
                    Learning::Oja => eta * ((post * pre) - (post * post) * built.w_out[(k, j)]),
                };
                built.w_out[(k, j)] = (built.w_out[(k, j)] + dw).clamp(stdp.w_min, stdp.w_max);
                if built.w_out[(k, j)].abs() > 1e-8 {
                    conn_presence_out[k][j] += 1;
                }
            }
        }
    }

    // --- Compute longterm connections ---
    let min_steps = (steps as f32 * 0.75).ceil() as usize;
    let mut longterm = 0;
    let mut total = 0;
    for j in 0..num_hidden_per_layer {
        for i in 0..num_sensory_neurons {
            if conn_presence_in[j][i] > 0 {
                total += 1;
                if conn_presence_in[j][i] >= min_steps {
                    longterm += 1;
                }
            }
        }
    }
    for l in 0..num_hidden_layers.saturating_sub(1) {
        for j in 0..num_hidden_per_layer {
            for i in 0..num_hidden_per_layer {
                if conn_presence_fwd[l][j][i] > 0 {
                    total += 1;
                    if conn_presence_fwd[l][j][i] >= min_steps {
                        longterm += 1;
                    }
                }
                if conn_presence_bwd[l][j][i] > 0 {
                    total += 1;
                    if conn_presence_bwd[l][j][i] >= min_steps {
                        longterm += 1;
                    }
                }
            }
        }
    }
    for k in 0..num_output_neurons {
        for j in 0..num_hidden_per_layer {
            if conn_presence_out[k][j] > 0 {
                total += 1;
                if conn_presence_out[k][j] >= min_steps {
                    longterm += 1;
                }
            }
        }
    }

    SimOut {
        spikes_h: hidden_spike_recordings,
        spikes_o: output_spike_recordings,
        weights: WeightsOut {
            w_in: built.w_in,
            w_hh_fwd: built.w_hh_fwd,
            w_hh_bwd: built.w_hh_bwd,
            w_out: built.w_out,
        },
        longterm_conn: longterm,
        total_conn: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LIFParams, NetworkConfig, STDPParams};
    use crate::network::build_network;
    use rand::SeedableRng;

    #[test]
    fn test_poisson_input_patterns() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let (spikes, pattern_id, groups) = poisson_input_patterns(100.0, 30, 1.0, &mut rng);
        assert_eq!(spikes.shape(), &[100, 30]);
        assert_eq!(pattern_id.len(), 100);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].len(), 10);
    }

    #[test]
    fn test_theta_input_patterns() {
        let (spikes, pattern_id, groups) = theta_input_patterns(100.0, 16, 1.0, 6.0, 0.2, 0.1);
        assert_eq!(spikes.shape(), &[100, 16]);
        assert_eq!(pattern_id.len(), 100);
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn test_apply_synaptic_filter() {
        let raw = Array1::from(vec![1.0, -1.0]);
        let mut ampa = Array1::zeros(2);
        let mut nmda = Array1::zeros(2);
        let mut gaba = Array1::zeros(2);
        let out = apply_synaptic_filter(
            &raw, &mut ampa, &mut nmda, &mut gaba, 0.9, 0.9, 0.9, 0.25, 1.0,
        );

        // Index 0: raw=1.0 (excitatory)
        // ampa[0] = 0*0.9 + 1.0*(1-0.25) = 0.75
        // nmda[0] = 0*0.9 + 1.0*0.25 = 0.25
        // gaba[0] = 0*0.9 + 0 = 0
        // out[0] = 0.75 + 0.25 - 0 = 1.0
        assert_eq!(out[0], 1.0);

        // Index 1: raw=-1.0 (inhibitory)
        // ampa[1] = 0*0.9 + 0 = 0
        // nmda[1] = 0*0.9 + 0 = 0
        // gaba[1] = 0*0.9 + 1.0 = 1.0
        // out[1] = 0 + 0 - 1.0 = -1.0
        assert_eq!(out[1], -1.0);
    }

    #[test]
    fn test_stp_update_cpu() {
        let pre_spks = vec![1, 0];
        let mut u = Array1::from(vec![0.2, 0.2]);
        let mut x = Array1::from(vec![1.0, 1.0]);
        let mut release = Array1::zeros(2);
        stp_update_cpu(&pre_spks, &mut u, &mut x, &mut release, 0.2, 0.9, 0.9);

        // Spike at index 0
        assert!(release[0] > 0.0);
        assert!(x[0] < 1.0);

        // No spike at index 1
        assert_eq!(release[1], 0.0);
        assert_eq!(x[1], 1.0);
    }

    #[test]
    fn test_run_snn_basic() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let cfg = NetworkConfig::default();
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let built = build_network(&cfg, &mut rng);
        let sensory_spikes = Array2::<i8>::zeros((10, cfg.num_sensory_neurons));

        let out = run_snn(
            10.0,
            &lif,
            &stdp,
            &cfg,
            built,
            &sensory_spikes,
            NeuronModel::Lif,
            Learning::Stdp,
        );

        assert_eq!(out.spikes_h.len(), cfg.num_hidden_layers);
        assert_eq!(out.spikes_o.shape(), &[10, cfg.num_output_neurons]);
    }
}
