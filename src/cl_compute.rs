//! # OpenCL GPGPU Acceleration Manager
//!
//! This module provides the infrastructure to accelerate neural simulation
//! tasks using OpenCL on compatible GPUs.
//!
//! ## Accelerated Operations:
//! - **Neuron Step**: Parallel update of membrane potentials (LIF/Izhikevich).
//! - **Synaptic Accumulation**: Both dense and sparse (CSR) matrix-vector
//!   multiplication for current integration.
//! - **Synaptic Plasticity**: Online weight updates (STDP/Hebb/Oja).
//! - **Morphology Energy**: Spatial density calculations for growth guidance.
//!
//! The manager handles OpenCL context creation, program compilation, and
//! command queue orchestration. Data is managed via `CLBuffers` and `CLSparseBuffers`.

#![cfg(feature = "opencl")]

use opencl3::command_queue::CommandQueue;
use opencl3::context::Context;
use opencl3::device::{Device, CL_DEVICE_TYPE_GPU};
use opencl3::kernel::Kernel;
use opencl3::memory::{Buffer, CL_MEM_READ_WRITE, CL_MEM_READ_ONLY};
use opencl3::platform::get_platforms;
use opencl3::program::Program;
use opencl3::types::cl_device_id;
use std::ptr;
use std::sync::{Mutex, Arc, OnceLock};

static GLOBAL_CL_MANAGER: OnceLock<Option<Arc<OpenCLManager>>> = OnceLock::new();

pub fn get_global_cl_manager() -> Option<Arc<OpenCLManager>> {
    GLOBAL_CL_MANAGER.get_or_init(|| {
        // UI/global device selection: NM_UI_CL_DEVICE_INDEX or NM_CL_DEVICE_INDEX.
        let idx = parse_env_usize("NM_UI_CL_DEVICE_INDEX")
            .or_else(|| parse_env_usize("NM_CL_DEVICE_INDEX"))
            .unwrap_or(0);
        OpenCLManager::new_with_device_index(idx).ok().map(Arc::new)
    }).clone()
}

fn parse_env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok().and_then(|v| v.parse::<usize>().ok())
}

fn gpu_device_ids() -> anyhow::Result<Vec<cl_device_id>> {
    let platforms = get_platforms().map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;
    if platforms.is_empty() {
        return Err(anyhow::anyhow!("No OpenCL platforms found"));
    }
    let mut devices = Vec::new();
    for platform in &platforms {
        let mut ids = platform
            .get_devices(CL_DEVICE_TYPE_GPU)
            .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;
        devices.append(&mut ids);
    }
    if devices.is_empty() {
        return Err(anyhow::anyhow!("No GPU devices found on platforms"));
    }
    Ok(devices)
}

pub fn gpu_device_ids_for_indices(indices: Option<&[usize]>) -> anyhow::Result<Vec<cl_device_id>> {
    let devices = gpu_device_ids()?;
    if let Some(indices) = indices {
        let mut selected = Vec::new();
        for &idx in indices {
            if let Some(id) = devices.get(idx) {
                selected.push(*id);
            }
        }
        if selected.is_empty() {
            return Err(anyhow::anyhow!("No matching GPU devices for requested indices"));
        }
        return Ok(selected);
    }
    Ok(devices)
}

pub struct CLBuffers {
    pub v: Buffer<f64>,
    pub u: Option<Buffer<f64>>,
    pub refr: Option<Buffer<i32>>,
    pub i_total: Buffer<f64>,
    pub spk: Buffer<i8>,
    pub x_trace: Buffer<f64>,
    pub size: usize,
}

impl CLBuffers {
    pub fn create(context: &Context, size: usize, has_u: bool, has_refr: bool) -> opencl3::Result<Self> {
        let f64_size = size * std::mem::size_of::<f64>();
        let i32_size = size * std::mem::size_of::<i32>();
        let i8_size = size * std::mem::size_of::<i8>();

        let v = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };
        let u = if has_u { Some(unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? }) } else { None };
        let refr = if has_refr { Some(unsafe { Buffer::create(context, CL_MEM_READ_WRITE, i32_size, ptr::null_mut())? }) } else { None };
        let i_total = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };
        let spk = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, i8_size, ptr::null_mut())? };
        let x_trace = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };
        
        Ok(Self { v, u, refr, i_total, spk, x_trace, size })
    }
}

#[allow(dead_code)]
pub struct CLSparseBuffers {
    pub row_ptr: Buffer<i32>,
    pub col_indices: Buffer<i32>,
    pub weights: Buffer<f64>,
    pub delays: Option<Buffer<i32>>,
    pub n_syn: usize,
    pub n_post: usize,
}

impl CLSparseBuffers {
    #[allow(dead_code)]
    pub fn create(context: &Context, n_syn: usize, n_post: usize, has_delays: bool) -> opencl3::Result<Self> {
        let row_ptr = unsafe { Buffer::create(context, CL_MEM_READ_ONLY, (n_post + 1) * std::mem::size_of::<i32>(), ptr::null_mut())? };
        let col_indices = unsafe { Buffer::create(context, CL_MEM_READ_ONLY, n_syn * std::mem::size_of::<i32>(), ptr::null_mut())? };
        let weights = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, n_syn * std::mem::size_of::<f64>(), ptr::null_mut())? };
        let delays = if has_delays {
            Some(unsafe { Buffer::create(context, CL_MEM_READ_ONLY, n_syn * std::mem::size_of::<i32>(), ptr::null_mut())? })
        } else {
            None
        };
        Ok(Self { row_ptr, col_indices, weights, delays, n_syn, n_post })
    }
}

#[allow(dead_code)]
pub struct OpenCLManager {
    pub device: Device,
    pub context: Context,
    pub queue: CommandQueue,
    pub program: Program,
    // Kernels
    pub kernel_lif_step: Mutex<Kernel>,
    pub kernel_izh_step: Mutex<Kernel>,
    pub kernel_syn_acc: Mutex<Kernel>,
    pub kernel_syn_acc_stp: Mutex<Kernel>,
    pub kernel_syn_acc_sparse: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_stp: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_delay: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_delay_stp: Mutex<Kernel>,
    pub kernel_syn_filter: Mutex<Kernel>,
    pub kernel_stp_update: Mutex<Kernel>,
    pub kernel_plasticity_update: Mutex<Kernel>,
    pub kernel_morpho_energy: Mutex<Kernel>,
}

const PROGRAM_SOURCE: &str = r#"
// LIF neuron step kernel
kernel void lif_step(
    global double* v,
    global int* refr,
    global const double* i_total,
    const double decay_m,
    const double v_th,
    const double v_reset,
    const int refractory_steps,
    global char* spk
) {
    size_t id = get_global_id(0);
    double cur_v = v[id] * decay_m + i_total[id];
    
    // clamp v for stability
    if (cur_v < -5.0) cur_v = -5.0;
    if (cur_v > 5.0) cur_v = 5.0;

    bool active = refr[id] <= 0;
    bool fired = active && (cur_v >= v_th);

    if (fired) {
        v[id] = v_reset;
        refr[id] = refractory_steps;
        spk[id] = 1;
    } else {
        v[id] = cur_v;
        refr[id] = (refr[id] > 0) ? refr[id] - 1 : 0;
        spk[id] = 0;
    }
}

// Izhikevich neuron step kernel
kernel void izh_step(
    global double* v,
    global double* u,
    global const double* i_total,
    const double dt,
    const double recovery_time_constant_a,
    const double recovery_sensitivity_b,
    const double membrane_reset_potential_c,
    const double recovery_increment_d,
    const double v_th,
    global char* spk
) {
    size_t id = get_global_id(0);
    double cv = v[id];
    double cu = u[id];
    
    double nv = cv + dt * (0.04 * cv * cv + 5.0 * cv + 140.0 - cu + i_total[id]);
    double nu = cu + dt * (recovery_time_constant_a * (recovery_sensitivity_b * nv - cu));
    
    bool fired = nv >= v_th;
    if (fired) {
        v[id] = membrane_reset_potential_c;
        u[id] = nu + recovery_increment_d;
        spk[id] = 1;
    } else {
        v[id] = nv;
        u[id] = nu;
        spk[id] = 0;
    }
}

// Simple synaptic current accumulation (dense fallback)
kernel void syn_acc_dense(
    global double* i_acc,
    global const char* pre_spks,
    global const double* weights,
    const int n_pre,
    const int n_post
) {
    size_t j = get_global_id(0); // post-synaptic index
    if (j >= (size_t)n_post) return;
    
    double acc = 0.0;
    for (int i = 0; i < n_pre; i++) {
        if (pre_spks[i] != 0) {
            acc += weights[j * n_pre + i];
        }
    }
    i_acc[j] = acc;
}

// Dense synaptic accumulation using STP release factors
kernel void syn_acc_dense_stp(
    global double* i_acc,
    global const double* pre_rel,
    global const double* weights,
    const int n_pre,
    const int n_post
) {
    size_t j = get_global_id(0); // post-synaptic index
    if (j >= (size_t)n_post) return;
    
    double acc = 0.0;
    for (int i = 0; i < n_pre; i++) {
        double rel = pre_rel[i];
        if (rel != 0.0) {
            acc += weights[j * n_pre + i] * rel;
        }
    }
    i_acc[j] = acc;
}

// Sparse synaptic accumulation (CSR)
kernel void syn_acc_sparse(
    global double* i_acc,
    global const char* pre_spks,
    global const int* row_ptr,
    global const int* col_indices,
    global const double* weights,
    const int n_post,
    const int accumulate
) {
    size_t j = get_global_id(0);
    if (j >= (size_t)n_post) return;
    
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j+1];
    for (int k = start; k < end; k++) {
        if (pre_spks[col_indices[k]] != 0) {
            acc += weights[k];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

// Sparse synaptic accumulation (CSR) with STP release scaling
kernel void syn_acc_sparse_stp(
    global double* i_acc,
    global const char* pre_spks,
    global const double* pre_rel,
    global const int* row_ptr,
    global const int* col_indices,
    global const double* weights,
    const int n_post,
    const int accumulate
) {
    size_t j = get_global_id(0);
    if (j >= (size_t)n_post) return;
    
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j+1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        if (pre_spks[pre_id] != 0) {
            acc += weights[k] * pre_rel[pre_id];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

// Sparse synaptic accumulation with delays (CSR)
kernel void syn_acc_sparse_delay(
    global double* i_acc,
    global const char* spk_history, // [hist_len][neurons_per_frame]
    global const int* row_ptr,
    global const int* col_indices,
    global const int* delays,
    global const double* weights,
    const int n_post,
    const int hist_len,
    const int neurons_per_frame,
    const int accumulate
) {
    size_t j = get_global_id(0);
    if (j >= (size_t)n_post) return;
    
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j+1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (delay < hist_len) {
            if (spk_history[delay * neurons_per_frame + pre_id] != 0) {
                acc += weights[k];
            }
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

// Sparse synaptic accumulation with delays (CSR) and STP release scaling
kernel void syn_acc_sparse_delay_stp(
    global double* i_acc,
    global const char* spk_history, // [hist_len][neurons_per_frame]
    global const double* pre_rel,
    global const int* row_ptr,
    global const int* col_indices,
    global const int* delays,
    global const double* weights,
    const int n_post,
    const int hist_len,
    const int neurons_per_frame,
    const int accumulate
) {
    size_t j = get_global_id(0);
    if (j >= (size_t)n_post) return;
    
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j+1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (delay < hist_len) {
            if (spk_history[delay * neurons_per_frame + pre_id] != 0) {
                acc += weights[k] * pre_rel[pre_id];
            }
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

// Synaptic filtering (AMPA/NMDA/GABA) applied in-place to i_acc
kernel void syn_filter(
    global double* i_acc,
    global double* ampa,
    global double* nmda,
    global double* gaba,
    const double decay_ampa,
    const double decay_nmda,
    const double decay_gaba,
    const double nmda_ratio,
    const double syn_gain
) {
    size_t id = get_global_id(0);
    double val = i_acc[id];
    double exc = val > 0.0 ? val : 0.0;
    double inh = val < 0.0 ? -val : 0.0;
    ampa[id] = ampa[id] * decay_ampa + exc * (1.0 - nmda_ratio);
    nmda[id] = nmda[id] * decay_nmda + exc * nmda_ratio;
    gaba[id] = gaba[id] * decay_gaba + inh;
    i_acc[id] = (ampa[id] + nmda[id] - gaba[id]) * syn_gain;
}

// Short-term plasticity (STP) update kernel
kernel void stp_update(
    global double* u,
    global double* x,
    global const char* pre_spk,
    global double* release,
    const double stp_u,
    const double decay_rec,
    const double decay_facil
) {
    size_t id = get_global_id(0);
    double uu = u[id];
    double xx = x[id];
    uu = uu * decay_facil + stp_u * (1.0 - decay_facil);
    xx = xx * decay_rec + (1.0 - decay_rec);
    if (pre_spk[id] != 0) {
        double rel = uu * xx;
        if (rel < 0.0) rel = 0.0;
        if (rel > 1.0) rel = 1.0;
        xx = xx - rel;
        if (xx < 0.0) xx = 0.0;
        uu = uu + stp_u * (1.0 - uu);
        if (uu < 0.0) uu = 0.0;
        if (uu > 1.0) uu = 1.0;
        release[id] = rel;
    } else {
        release[id] = 0.0;
    }
    u[id] = uu;
    x[id] = xx;
}

// Plasticity learning update kernel
kernel void plasticity_update(
    global double* weights,
    global const char* pre_spks,
    global const char* post_spks,
    global const double* x_pre,
    global const double* x_post,
    const double eta,
    const double w_min,
    const double w_max,
    const int n_pre,
    const int n_post,
    const int rule // 0: stdp, 1: hebb, 2: oja
) {
    size_t j = get_global_id(0); // post
    size_t i = get_global_id(1); // pre
    if (j >= n_post || i >= n_pre) return;
    
    size_t idx = j * n_pre + i;
    double pre = (pre_spks[i] != 0) ? 1.0 : 0.0;
    double post = (post_spks[j] != 0) ? 1.0 : 0.0;
    
    double dw = 0.0;
    if (rule == 0) {
        // STDP: eta * (post * x_pre - pre * x_post)
        dw = eta * (post * x_pre[i] - pre * x_post[j]);
    } else if (rule == 1) {
        // Hebb: eta * post * pre
        dw = eta * post * pre;
    } else if (rule == 2) {
        // Oja: eta * (post * pre - post * post * w)
        dw = eta * (post * pre - post * post * weights[idx]);
    }
    
    weights[idx] = clamp(weights[idx] + dw, w_min, w_max);
}

// Morphological energy density at points
kernel void morpho_energy(
    global const float4* points,
    global const float4* syn_sites,
    global const float* syn_stimuli,
    global float* energies,
    const int n_syn,
    const float radius_sq,
    const float kernel_k
) {
    size_t id = get_global_id(0);
    float4 p = points[id];
    float total = 0.0f;
    
    for (int i = 0; i < n_syn; i++) {
        float4 s = syn_sites[i];
        float4 d = p - s;
        float d2 = d.x*d.x + d.y*d.y + d.z*d.z;
        if (d2 < radius_sq) {
            total += syn_stimuli[i] / (1.0f + kernel_k * d2);
        }
    }
    energies[id] = total;
}
"#;

impl OpenCLManager {
    #[allow(dead_code)]
    pub fn new() -> anyhow::Result<Self> {
        Self::new_with_device_index(0)
    }

    pub fn new_with_device_index(index: usize) -> anyhow::Result<Self> {
        let devices = gpu_device_ids()?;
        let device_id = *devices.get(index).ok_or_else(|| anyhow::anyhow!("GPU device index {} out of range", index))?;
        Self::new_with_device_id(device_id)
    }

    pub fn new_with_device_id(device_id: cl_device_id) -> anyhow::Result<Self> {
        let device = Device::new(device_id);
        let context = Context::from_device(&device).map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;
        let queue = unsafe { CommandQueue::create_with_properties(&context, device_id, 0, 0).map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))? };

        let program = Program::create_and_build_from_source(&context, PROGRAM_SOURCE, "").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;
        
        let kernel_lif_step = Mutex::new(Kernel::create(&program, "lif_step").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_izh_step = Mutex::new(Kernel::create(&program, "izh_step").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_acc = Mutex::new(Kernel::create(&program, "syn_acc_dense").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_acc_stp = Mutex::new(Kernel::create(&program, "syn_acc_dense_stp").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_acc_sparse = Mutex::new(Kernel::create(&program, "syn_acc_sparse").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_acc_sparse_stp = Mutex::new(Kernel::create(&program, "syn_acc_sparse_stp").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_acc_sparse_delay = Mutex::new(Kernel::create(&program, "syn_acc_sparse_delay").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_acc_sparse_delay_stp = Mutex::new(Kernel::create(&program, "syn_acc_sparse_delay_stp").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_syn_filter = Mutex::new(Kernel::create(&program, "syn_filter").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_stp_update = Mutex::new(Kernel::create(&program, "stp_update").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_plasticity_update = Mutex::new(Kernel::create(&program, "plasticity_update").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        let kernel_morpho_energy = Mutex::new(Kernel::create(&program, "morpho_energy").map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?);
        
        Ok(Self {
            device,
            context,
            queue,
            program,
            kernel_lif_step,
            kernel_izh_step,
            kernel_syn_acc,
            kernel_syn_acc_stp,
            kernel_syn_acc_sparse,
            kernel_syn_acc_sparse_stp,
            kernel_syn_acc_sparse_delay,
            kernel_syn_acc_sparse_delay_stp,
            kernel_syn_filter,
            kernel_stp_update,
            kernel_plasticity_update,
            kernel_morpho_energy,
        })
    }
}
