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

use crate::aarnn::plasticity::{
    ShortTermPlasticityParams, ShortTermPlasticityState, release_draw, release_probability,
    stp_step,
};
use crate::config::{IzhikevichParams, LIFParams};
use crate::gpu_api::{
    CL_DEVICE_TYPE_CPU, CL_DEVICE_TYPE_GPU, CommandQueue, Context, Device, Kernel, Program,
    cl_device_id, cl_device_type,
};
use crate::neuron_kernels::{izh_transition, lif_transition};
use opencl3::platform::get_platforms;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
#[cfg(feature = "cuda")]
use std::{fs, process::Command};

static GLOBAL_CL_MANAGER: OnceLock<Option<Arc<OpenCLManager>>> = OnceLock::new();
#[cfg(feature = "cuda")]
static CUDA_GPU_COUNT: OnceLock<usize> = OnceLock::new();

pub use crate::gpu_api::{
    Buffer, CL_INVALID_VALUE, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE, CL_TRUE, ClError, ExecuteKernel,
    Result as ClResult,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenCLExecutionTarget {
    Gpu,
    Cpu,
}

impl OpenCLExecutionTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gpu => "GPU",
            Self::Cpu => "CPU",
        }
    }
}

pub fn get_global_cl_manager() -> Option<Arc<OpenCLManager>> {
    GLOBAL_CL_MANAGER
        .get_or_init(|| {
            if accelerated_compute_disabled() {
                let source = if parse_env_bool("NM_DISABLE_OPENCL").unwrap_or(false) {
                    "NM_DISABLE_OPENCL"
                } else {
                    "test default"
                };
                nm_log!(
                    "[info] Accelerated compute backend disabled ({}); using CPU-only execution.",
                    source
                );
                return None;
            }
            // UI/global device selection: NM_UI_CL_DEVICE_INDEX or NM_CL_DEVICE_INDEX.
            let idx = parse_env_usize("NM_UI_CL_DEVICE_INDEX")
                .or_else(|| parse_env_usize("NM_CL_DEVICE_INDEX"))
                .unwrap_or(0);
            let init_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                OpenCLManager::new_with_preferred_device_index(idx)
            }));
            match init_result {
                Ok(Ok(manager)) => {
                    let device_name = manager
                        .device
                        .name()
                        .unwrap_or_else(|_| "<unknown>".to_string());
                    let device_vendor = manager
                        .device
                        .vendor()
                        .unwrap_or_else(|_| "<unknown vendor>".to_string());
                    let backend = if manager.is_cuda_backend() {
                        "CUDA"
                    } else {
                        "OpenCL"
                    };
                    nm_log!(
                        "[info] Compute backend initialized: {} {} device: {} ({})",
                        backend,
                        manager.execution_target().label(),
                        device_name,
                        device_vendor
                    );
                    Some(Arc::new(manager))
                }
                Ok(Err(e)) => {
                    nm_err!("[warn] OpenCL unavailable: {}", e);
                    None
                }
                Err(payload) => {
                    nm_err!(
                        "[warn] OpenCL/CUDA initialization panicked: {}. Falling back to CPU-only execution.",
                        panic_payload_to_string(payload)
                    );
                    None
                }
            }
        })
        .clone()
}

fn parse_env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
}

fn parse_env_bool(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn accelerated_compute_disabled() -> bool {
    if parse_env_bool("NM_DISABLE_OPENCL").unwrap_or(false) {
        return true;
    }

    cfg!(test) && !parse_env_bool("NM_ENABLE_OPENCL_IN_TESTS").unwrap_or(false)
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn opencl_device_ids_by_type(
    device_type: cl_device_type,
    label: &str,
) -> anyhow::Result<Vec<cl_device_id>> {
    let platforms = get_platforms().map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;
    if platforms.is_empty() {
        return Err(anyhow::anyhow!("No OpenCL platforms found"));
    }

    let mut devices = Vec::new();
    let mut errors = Vec::new();
    for platform in &platforms {
        match platform.get_devices(device_type) {
            Ok(mut ids) => devices.append(&mut ids),
            Err(e) => errors.push(format!("{}", e)),
        }
    }

    if devices.is_empty() {
        if errors.is_empty() {
            return Err(anyhow::anyhow!("No OpenCL {} devices found", label));
        }
        return Err(anyhow::anyhow!(
            "No OpenCL {} devices found (platform query errors: {})",
            label,
            errors.join(" | ")
        ));
    }
    Ok(devices)
}

fn gpu_device_ids() -> anyhow::Result<Vec<cl_device_id>> {
    opencl_device_ids_by_type(CL_DEVICE_TYPE_GPU, "GPU")
}

fn cpu_device_ids() -> anyhow::Result<Vec<cl_device_id>> {
    opencl_device_ids_by_type(CL_DEVICE_TYPE_CPU, "CPU")
}

#[cfg(feature = "cuda")]
fn probe_nvidia_cuda_gpu_count() -> usize {
    let query_count = |args: &[&str], starts_with_gpu_prefix: bool| -> Option<usize> {
        let output = Command::new("nvidia-smi").args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let count = stdout
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty()
                    && (!starts_with_gpu_prefix
                        || line.starts_with("GPU ")
                        || line.starts_with("GPU"))
            })
            .count();
        if count > 0 { Some(count) } else { None }
    };

    if let Some(count) = query_count(&["--query-gpu=name", "--format=csv,noheader"], false) {
        return count;
    }
    if let Some(count) = query_count(&["-L"], true) {
        return count;
    }

    fs::read_dir("/proc/driver/nvidia/gpus")
        .ok()
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

#[cfg(feature = "cuda")]
fn nvidia_cuda_gpu_count() -> usize {
    *CUDA_GPU_COUNT.get_or_init(probe_nvidia_cuda_gpu_count)
}

fn select_device_id(
    devices: &[cl_device_id],
    preferred_index: usize,
    target: OpenCLExecutionTarget,
) -> anyhow::Result<cl_device_id> {
    if devices.is_empty() {
        return Err(anyhow::anyhow!(
            "No OpenCL {} devices available",
            target.label()
        ));
    }
    if let Some(device_id) = devices.get(preferred_index) {
        return Ok(*device_id);
    }
    nm_log!(
        "[warn] Requested OpenCL {} device index {} is out of range ({} devices); using index 0.",
        target.label(),
        preferred_index,
        devices.len()
    );
    Ok(devices[0])
}

pub fn gpu_device_ids_for_indices(indices: Option<&[usize]>) -> anyhow::Result<Vec<cl_device_id>> {
    if accelerated_compute_disabled() {
        return Err(anyhow::anyhow!(
            "accelerated compute backend disabled by configuration"
        ));
    }
    let devices = gpu_device_ids()?;
    if let Some(indices) = indices {
        let mut selected = Vec::new();
        for &idx in indices {
            if let Some(id) = devices.get(idx) {
                selected.push(*id);
            }
        }
        if selected.is_empty() {
            return Err(anyhow::anyhow!(
                "No matching GPU devices for requested indices"
            ));
        }
        return Ok(selected);
    }
    Ok(devices)
}

pub struct CLBuffers {
    pub v: Buffer<f64>,
    pub u: Option<Buffer<f64>>,
    pub refr: Option<Buffer<i32>>,
    /// AARNN adaptive-threshold offset.  It is allocated for every population
    /// so changing models does not require a device-buffer layout transition.
    pub threshold_offset: Buffer<f64>,
    pub i_total: Buffer<f64>,
    pub spk: Buffer<i8>,
    pub x_trace: Buffer<f64>,
    pub size: usize,
}

impl CLBuffers {
    pub fn create(context: &Context, size: usize, has_u: bool, has_refr: bool) -> ClResult<Self> {
        let f64_size = size * std::mem::size_of::<f64>();
        let i32_size = size * std::mem::size_of::<i32>();
        let i8_size = size * std::mem::size_of::<i8>();

        let v = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };
        let u = if has_u {
            Some(unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? })
        } else {
            None
        };
        let refr = if has_refr {
            Some(unsafe { Buffer::create(context, CL_MEM_READ_WRITE, i32_size, ptr::null_mut())? })
        } else {
            None
        };
        let threshold_offset =
            unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };
        let i_total =
            unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };
        let spk = unsafe { Buffer::create(context, CL_MEM_READ_WRITE, i8_size, ptr::null_mut())? };
        let x_trace =
            unsafe { Buffer::create(context, CL_MEM_READ_WRITE, f64_size, ptr::null_mut())? };

        Ok(Self {
            v,
            u,
            refr,
            threshold_offset,
            i_total,
            spk,
            x_trace,
            size,
        })
    }
}

#[allow(dead_code)]
pub struct CLSparseBuffers {
    pub row_ptr: Buffer<i32>,
    pub col_indices: Buffer<i32>,
    pub weights: Buffer<f64>,
    pub delays: Option<Buffer<i32>>,
    /// Per-synapse admission mask for the current logical step.  The mask is
    /// populated from the certified deterministic release stream immediately
    /// before an AARNN sparse transaction.  It is deliberately separate from
    /// `weights`: STP remains a per-presynaptic state transition and release
    /// probability is a per-synapse event decision.
    pub release_mask: Buffer<i8>,
    /// Host-side mapping used to project the global morphology synapse IDs
    /// onto the CSR order without making topology state device-owned.
    pub synapse_ids: Vec<usize>,
    pub n_syn: usize,
    pub n_post: usize,
}

impl CLSparseBuffers {
    #[allow(dead_code)]
    pub fn create(
        context: &Context,
        n_syn: usize,
        n_post: usize,
        has_delays: bool,
    ) -> ClResult<Self> {
        let row_ptr = unsafe {
            Buffer::create(
                context,
                CL_MEM_READ_ONLY,
                (n_post + 1) * std::mem::size_of::<i32>(),
                ptr::null_mut(),
            )?
        };
        let col_indices = unsafe {
            Buffer::create(
                context,
                CL_MEM_READ_ONLY,
                n_syn * std::mem::size_of::<i32>(),
                ptr::null_mut(),
            )?
        };
        let weights = unsafe {
            Buffer::create(
                context,
                CL_MEM_READ_WRITE,
                n_syn * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )?
        };
        let delays = if has_delays {
            Some(unsafe {
                Buffer::create(
                    context,
                    CL_MEM_READ_ONLY,
                    n_syn * std::mem::size_of::<i32>(),
                    ptr::null_mut(),
                )?
            })
        } else {
            None
        };
        let release_mask = unsafe {
            Buffer::create(
                context,
                CL_MEM_READ_ONLY,
                n_syn * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )?
        };
        Ok(Self {
            row_ptr,
            col_indices,
            weights,
            delays,
            release_mask,
            synapse_ids: Vec::with_capacity(n_syn),
            n_syn,
            n_post,
        })
    }
}

#[allow(dead_code)]
pub struct OpenCLManager {
    pub device: Device,
    pub execution_target: OpenCLExecutionTarget,
    is_cuda_backend: bool,
    pub context: Context,
    pub queue: CommandQueue,
    pub program: Program,
    // Kernels
    pub kernel_lif_step: Mutex<Kernel>,
    pub kernel_izh_step: Mutex<Kernel>,
    pub kernel_aarnn_step: Mutex<Kernel>,
    pub kernel_syn_acc: Mutex<Kernel>,
    pub kernel_syn_acc_stp: Mutex<Kernel>,
    pub kernel_syn_acc_sparse: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_stp: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_delay: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_delay_stp: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_delay_release: Mutex<Kernel>,
    pub kernel_syn_acc_sparse_delay_release_stp: Mutex<Kernel>,
    pub kernel_syn_filter: Mutex<Kernel>,
    pub kernel_stp_update: Mutex<Kernel>,
    pub kernel_release_decision: Mutex<Kernel>,
    pub kernel_homeostasis_decay: Mutex<Kernel>,
    pub kernel_homeostasis_spikes: Mutex<Kernel>,
    pub kernel_neuromodulation: Mutex<Kernel>,
    pub kernel_growth_candidates: Mutex<Kernel>,
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
    double rest_v = isfinite(membrane_reset_potential_c) ? membrane_reset_potential_c : -65.0;
    double rest_u = recovery_sensitivity_b * rest_v;
    double cv = v[id];
    double cu = u[id];
    if (!isfinite(cv) || !isfinite(cu)) {
        cv = rest_v;
        cu = rest_u;
    }
    double v_min = fmin(rest_v - 120.0, -150.0);
    double v_max = fmax(v_th + 80.0, 40.0);
    double u_min = fmin(rest_u - 400.0, -600.0);
    double u_max = fmax(rest_u + 400.0, 600.0);
    cv = fmin(fmax(cv, v_min), v_max);
    cu = fmin(fmax(cu, u_min), u_max);
    double current = isfinite(i_total[id]) ? i_total[id] : 0.0;
    double nv = cv + dt * (0.04 * cv * cv + 5.0 * cv + 140.0 - cu + current);
    double nu = cu + dt * (recovery_time_constant_a * (recovery_sensitivity_b * nv - cu));
    if (!isfinite(nv) || !isfinite(nu)) {
        nv = rest_v;
        nu = rest_u;
    }
    nv = fmin(fmax(nv, v_min), v_max);
    nu = fmin(fmax(nu, u_min), u_max);
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

// Full deterministic AARNN membrane transition.  Synaptic currents,
// morphology, delays, release/STP, and structural state are prepared by the
// caller; this kernel owns the same per-neuron transition as
// neuron_kernels::izh_transition, including adaptive threshold and optional
// refractory state.  Keeping those states in device buffers avoids silently
// reverting AARNN populations to the CPU while LIF/Izh populations use the
// accelerator.
kernel void aarnn_step(
    global double* v,
    global double* u,
    global const double* i_total,
    global double* threshold_offset,
    global int* refr,
    const double dt,
    const double recovery_time_constant_a,
    const double recovery_sensitivity_b,
    const double membrane_reset_potential_c,
    const double recovery_increment_d,
    const double v_th,
    const double threshold_increment,
    const double threshold_min,
    const double threshold_max,
    const int adaptive_threshold_enabled,
    const int refractory_enabled,
    const int refractory_steps,
    global char* spk
) {
    size_t id = get_global_id(0);
    double rest_v = isfinite(membrane_reset_potential_c) ? membrane_reset_potential_c : -65.0;
    double rest_u = recovery_sensitivity_b * rest_v;
    double cv = v[id];
    double cu = u[id];
    int unstable = 0;
    if (!isfinite(cv) || !isfinite(cu)) {
        cv = rest_v;
        cu = rest_u;
        unstable = 1;
    }
    double v_min = fmin(rest_v - 120.0, -150.0);
    double v_max = fmax(v_th + 80.0, 40.0);
    double u_min = fmin(rest_u - 400.0, -600.0);
    double u_max = fmax(rest_u + 400.0, 600.0);
    cv = fmin(fmax(cv, v_min), v_max);
    cu = fmin(fmax(cu, u_min), u_max);
    double current = isfinite(i_total[id]) ? i_total[id] : 0.0;
    double nv = cv + dt * (0.04 * cv * cv + 5.0 * cv + 140.0 - cu + current);
    double nu = cu + dt * (recovery_time_constant_a * (recovery_sensitivity_b * nv - cu));
    if (!isfinite(nv) || !isfinite(nu)) {
        nv = rest_v;
        nu = rest_u;
        unstable = 1;
    }
    nv = fmin(fmax(nv, v_min), v_max);
    nu = fmin(fmax(nu, u_min), u_max);
    double input_threshold = threshold_offset[id];
    double effective_threshold = 0.0;
    if (adaptive_threshold_enabled != 0) {
        effective_threshold = fmin(fmax(input_threshold, threshold_min), threshold_max);
    }
    int old_refr = refr[id];
    int blocked = refractory_enabled != 0 && old_refr > 0;
    int fired = unstable == 0 && blocked == 0 && nv >= (v_th + effective_threshold);
    if (fired != 0) {
        v[id] = membrane_reset_potential_c;
        u[id] = nu + recovery_increment_d;
    } else {
        v[id] = nv;
        u[id] = nu;
    }
    if (adaptive_threshold_enabled != 0) {
        threshold_offset[id] = fired != 0
            ? fmin(fmax(input_threshold + threshold_increment, threshold_min), threshold_max)
            : effective_threshold;
    } else {
        threshold_offset[id] = 0.0;
    }
    if (refractory_enabled != 0) {
        refr[id] = fired != 0 ? refractory_steps : (old_refr > 0 ? old_refr - 1 : 0);
    } else {
        refr[id] = 0;
    }
    spk[id] = (char)fired;
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
        if (delay >= 0 && delay < hist_len) {
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
        if (delay >= 0 && delay < hist_len) {
            if (spk_history[delay * neurons_per_frame + pre_id] != 0) {
                acc += weights[k] * pre_rel[pre_id];
            }
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

// Sparse delayed accumulation with a deterministic per-synapse release mask.
// The mask is supplied by the release-decision stage; no device-side random
// state or packet-arrival order participates in biological admission.
kernel void syn_acc_sparse_delay_release(
    global double* i_acc,
    global const char* spk_history,
    global const char* release_mask,
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
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (release_mask[k] != 0 && delay >= 0 && delay < hist_len &&
            spk_history[delay * neurons_per_frame + pre_id] != 0) {
            acc += weights[k];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

kernel void syn_acc_sparse_delay_release_stp(
    global double* i_acc,
    global const char* spk_history,
    global const double* pre_rel,
    global const char* release_mask,
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
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (release_mask[k] != 0 && delay >= 0 && delay < hist_len &&
            spk_history[delay * neurons_per_frame + pre_id] != 0) {
            acc += weights[k] * pre_rel[pre_id];
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
    global const double* vmem,
    const double nmda_voltage_sensitivity,
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
    double nmda_gate = 1.0;
    if (nmda_voltage_sensitivity > 0.0) {
        double x = clamp(nmda_voltage_sensitivity * (vmem[id] + 40.0), -60.0, 60.0);
        nmda_gate = 1.0 / (1.0 + exp(-x));
    }
    ampa[id] = ampa[id] * decay_ampa + exc * (1.0 - nmda_ratio);
    nmda[id] = nmda[id] * decay_nmda + exc * nmda_ratio * nmda_gate;
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

// Deterministic release decisions.  The hash and its two coordinates mirror
// aarnn::plasticity::release_draw/should_release exactly.  Each work item owns
// one synapse result, so work-group order cannot affect the event stream.
ulong aarnn_mix64(ulong x) {
    x ^= x >> 33;
    x *= (ulong)0xff51afd7ed558ccdUL;
    x ^= x >> 33;
    x *= (ulong)0xc4ceb9fe1a85ec53UL;
    x ^= x >> 33;
    return x;
}

kernel void release_decision(
    global char* decisions,
    const float base_probability,
    const float heterogeneity,
    const int time_low,
    const int time_high,
    const int synapse_count
) {
    size_t id = get_global_id(0);
    if (id >= (size_t)synapse_count) return;
    ulong time_step = ((ulong)(uint)time_high << 32) | (ulong)(uint)time_low;
    ulong syn_seed = ((ulong)id) * (ulong)0x9e3779b185ebca87UL;
    ulong draw_seed = syn_seed + time_step * (ulong)0xd2b74407b1ce6e93UL;
    // The draw is quantized to f32 because the Rust reference returns f32.
    // The heterogeneous probability keeps the reference's f64 hash arithmetic
    // until the final delta cast; changing that cast point can flip a boundary
    // decision for an otherwise identical event stream.
    float draw = (float)((double)aarnn_mix64(draw_seed) / 18446744073709551615.0);
    float base = clamp(base_probability, 0.0f, 1.0f);
    float spread = clamp(heterogeneity, 0.0f, 1.0f);
    float probability = base;
    if (spread > 0.0) {
        double individual = (double)aarnn_mix64(syn_seed) / 18446744073709551615.0;
        float delta = (float)((2.0 * individual) - 1.0) * spread;
        probability = clamp(base + delta, 0.0f, 1.0f);
    }
    decisions[id] = (char)(draw <= probability);
}

// The two homeostasis kernels are deliberately split at the same phase
// boundary as Runner: decay is before the neuron transition, spike/rate
// feedback is after it.  This preserves the CPU reference ordering.
kernel void homeostasis_decay(
    global double* threshold_offset,
    global double* rate_ema,
    const double threshold_decay,
    const double homeostasis_decay,
    const int update_threshold,
    const int update_rate,
    const int count
) {
    size_t id = get_global_id(0);
    if (id >= (size_t)count) return;
    if (update_threshold != 0) threshold_offset[id] *= threshold_decay;
    if (update_rate != 0) rate_ema[id] *= homeostasis_decay;
}

kernel void homeostasis_spikes(
    global double* threshold_offset,
    global double* rate_ema,
    global const char* spikes,
    const double homeostasis_decay,
    const double target_rate,
    const double gain,
    const int update_rate,
    const int count
) {
    size_t id = get_global_id(0);
    if (id >= (size_t)count) return;
    if (update_rate != 0 && spikes[id] != 0) {
        rate_ema[id] += 1.0 - homeostasis_decay;
    }
    if (update_rate != 0) {
        threshold_offset[id] += gain * (rate_ema[id] - target_rate);
    }
}

// Neuromodulator and resonance state is a four-scalar transactional update:
// dopamine, acetylcholine, serotonin, resonance.  Signal reduction remains
// explicit and deterministic on the host; this kernel owns only the EMA.
kernel void neuromodulation(
    global double* state,
    global const double* targets,
    const double decay,
    const double resonance_decay,
    const double resonance_target
) {
    if (get_global_id(0) != 0) return;
    double d = clamp(decay, 0.0, 1.0);
    double r = clamp(resonance_decay, 0.0, 1.0);
    state[0] = state[0] * (1.0 - d) + targets[0] * d;
    state[1] = state[1] * (1.0 - d) + targets[1] * d;
    state[2] = state[2] * (1.0 - d) + targets[2] * d;
    state[3] = state[3] * (1.0 - r) + clamp(resonance_target, 0.0, 1.0) * r;
}

// Growth admission is a proposal stage only.  Structural allocation,
// generation publication and deterministic event ordering stay on the CPU
// commit boundary required by INV-009/INV-014.
kernel void growth_candidates(
    global const double* firing_rate,
    global const double* since_growth,
    global char* candidates,
    const double saturation_threshold,
    const double cooldown,
    const int count
) {
    size_t id = get_global_id(0);
    if (id >= (size_t)count) return;
    candidates[id] = (char)(firing_rate[id] >= saturation_threshold && since_growth[id] >= cooldown);
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

#[cfg(feature = "cuda")]
const CUDA_PROGRAM_SOURCE: &str = r#"
#include <cuda_runtime.h>

__device__ __forceinline__ double clampd(double x, double lo, double hi) {
    return x < lo ? lo : (x > hi ? hi : x);
}

extern "C" __global__ void lif_step(
    double* v,
    int* refr,
    const double* i_total,
    const double decay_m,
    const double v_th,
    const double v_reset,
    const int refractory_steps,
    signed char* spk,
    const int n_neurons
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= n_neurons) return;
    double cur_v = v[id] * decay_m + i_total[id];
    cur_v = clampd(cur_v, -5.0, 5.0);
    int active = refr[id] <= 0;
    int fired = active && (cur_v >= v_th);
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

extern "C" __global__ void izh_step(
    double* v,
    double* u,
    const double* i_total,
    const double dt,
    const double recovery_time_constant_a,
    const double recovery_sensitivity_b,
    const double membrane_reset_potential_c,
    const double recovery_increment_d,
    const double v_th,
    signed char* spk,
    const int n_neurons
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= n_neurons) return;
    double rest_v = isfinite(membrane_reset_potential_c) ? membrane_reset_potential_c : -65.0;
    double rest_u = recovery_sensitivity_b * rest_v;
    double cv = v[id];
    double cu = u[id];
    if (!isfinite(cv) || !isfinite(cu)) {
        cv = rest_v;
        cu = rest_u;
    }
    double v_min = fmin(rest_v - 120.0, -150.0);
    double v_max = fmax(v_th + 80.0, 40.0);
    double u_min = fmin(rest_u - 400.0, -600.0);
    double u_max = fmax(rest_u + 400.0, 600.0);
    cv = clampd(cv, v_min, v_max);
    cu = clampd(cu, u_min, u_max);
    double current = isfinite(i_total[id]) ? i_total[id] : 0.0;
    double nv = cv + dt * (0.04 * cv * cv + 5.0 * cv + 140.0 - cu + current);
    double nu = cu + dt * (recovery_time_constant_a * (recovery_sensitivity_b * nv - cu));
    if (!isfinite(nv) || !isfinite(nu)) {
        nv = rest_v;
        nu = rest_u;
    }
    nv = clampd(nv, v_min, v_max);
    nu = clampd(nu, u_min, u_max);
    int fired = nv >= v_th;
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

extern "C" __global__ void aarnn_step(
    double* v,
    double* u,
    const double* i_total,
    double* threshold_offset,
    int* refr,
    const double dt,
    const double recovery_time_constant_a,
    const double recovery_sensitivity_b,
    const double membrane_reset_potential_c,
    const double recovery_increment_d,
    const double v_th,
    const double threshold_increment,
    const double threshold_min,
    const double threshold_max,
    const int adaptive_threshold_enabled,
    const int refractory_enabled,
    const int refractory_steps,
    signed char* spk,
    const int n_neurons
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= n_neurons) return;
    double rest_v = isfinite(membrane_reset_potential_c) ? membrane_reset_potential_c : -65.0;
    double rest_u = recovery_sensitivity_b * rest_v;
    double cv = v[id];
    double cu = u[id];
    int unstable = 0;
    if (!isfinite(cv) || !isfinite(cu)) {
        cv = rest_v;
        cu = rest_u;
        unstable = 1;
    }
    double v_min = fmin(rest_v - 120.0, -150.0);
    double v_max = fmax(v_th + 80.0, 40.0);
    double u_min = fmin(rest_u - 400.0, -600.0);
    double u_max = fmax(rest_u + 400.0, 600.0);
    cv = clampd(cv, v_min, v_max);
    cu = clampd(cu, u_min, u_max);
    double current = isfinite(i_total[id]) ? i_total[id] : 0.0;
    double nv = cv + dt * (0.04 * cv * cv + 5.0 * cv + 140.0 - cu + current);
    double nu = cu + dt * (recovery_time_constant_a * (recovery_sensitivity_b * nv - cu));
    if (!isfinite(nv) || !isfinite(nu)) {
        nv = rest_v;
        nu = rest_u;
        unstable = 1;
    }
    nv = clampd(nv, v_min, v_max);
    nu = clampd(nu, u_min, u_max);
    double input_threshold = threshold_offset[id];
    double effective_threshold = adaptive_threshold_enabled != 0
        ? clampd(input_threshold, threshold_min, threshold_max)
        : 0.0;
    int old_refr = refr[id];
    int blocked = refractory_enabled != 0 && old_refr > 0;
    int fired = unstable == 0 && blocked == 0 && nv >= (v_th + effective_threshold);
    if (fired != 0) {
        v[id] = membrane_reset_potential_c;
        u[id] = nu + recovery_increment_d;
    } else {
        v[id] = nv;
        u[id] = nu;
    }
    threshold_offset[id] = adaptive_threshold_enabled != 0
        ? (fired != 0
            ? clampd(input_threshold + threshold_increment, threshold_min, threshold_max)
            : effective_threshold)
        : 0.0;
    refr[id] = refractory_enabled != 0
        ? (fired != 0 ? refractory_steps : (old_refr > 0 ? old_refr - 1 : 0))
        : 0;
    spk[id] = (signed char)fired;
}

extern "C" __global__ void syn_acc_dense(
    double* i_acc,
    const signed char* pre_spks,
    const double* weights,
    const int n_pre,
    const int n_post
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    for (int i = 0; i < n_pre; i++) {
        if (pre_spks[i] != 0) acc += weights[j * n_pre + i];
    }
    i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_dense_stp(
    double* i_acc,
    const double* pre_rel,
    const double* weights,
    const int n_pre,
    const int n_post
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    for (int i = 0; i < n_pre; i++) {
        double rel = pre_rel[i];
        if (rel != 0.0) acc += weights[j * n_pre + i] * rel;
    }
    i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_sparse(
    double* i_acc,
    const signed char* pre_spks,
    const int* row_ptr,
    const int* col_indices,
    const double* weights,
    const int n_post,
    const int accumulate
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        if (pre_spks[col_indices[k]] != 0) acc += weights[k];
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_sparse_stp(
    double* i_acc,
    const signed char* pre_spks,
    const double* pre_rel,
    const int* row_ptr,
    const int* col_indices,
    const double* weights,
    const int n_post,
    const int accumulate
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        if (pre_spks[pre_id] != 0) acc += weights[k] * pre_rel[pre_id];
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_sparse_delay(
    double* i_acc,
    const signed char* spk_history,
    const int* row_ptr,
    const int* col_indices,
    const int* delays,
    const double* weights,
    const int n_post,
    const int hist_len,
    const int neurons_per_frame,
    const int accumulate
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (delay >= 0 && delay < hist_len) {
            if (spk_history[delay * neurons_per_frame + pre_id] != 0) acc += weights[k];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_sparse_delay_stp(
    double* i_acc,
    const signed char* spk_history,
    const double* pre_rel,
    const int* row_ptr,
    const int* col_indices,
    const int* delays,
    const double* weights,
    const int n_post,
    const int hist_len,
    const int neurons_per_frame,
    const int accumulate
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (delay >= 0 && delay < hist_len) {
            if (spk_history[delay * neurons_per_frame + pre_id] != 0) acc += weights[k] * pre_rel[pre_id];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_sparse_delay_release(
    double* i_acc,
    const signed char* spk_history,
    const signed char* release_mask,
    const int* row_ptr,
    const int* col_indices,
    const int* delays,
    const double* weights,
    const int n_post,
    const int hist_len,
    const int neurons_per_frame,
    const int accumulate
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (release_mask[k] != 0 && delay >= 0 && delay < hist_len &&
            spk_history[delay * neurons_per_frame + pre_id] != 0) {
            acc += weights[k];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

extern "C" __global__ void syn_acc_sparse_delay_release_stp(
    double* i_acc,
    const signed char* spk_history,
    const double* pre_rel,
    const signed char* release_mask,
    const int* row_ptr,
    const int* col_indices,
    const int* delays,
    const double* weights,
    const int n_post,
    const int hist_len,
    const int neurons_per_frame,
    const int accumulate
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)j >= n_post) return;
    double acc = 0.0;
    int start = row_ptr[j];
    int end = row_ptr[j + 1];
    for (int k = start; k < end; k++) {
        int pre_id = col_indices[k];
        int delay = delays[k];
        if (release_mask[k] != 0 && delay >= 0 && delay < hist_len &&
            spk_history[delay * neurons_per_frame + pre_id] != 0) {
            acc += weights[k] * pre_rel[pre_id];
        }
    }
    if (accumulate != 0) i_acc[j] += acc;
    else i_acc[j] = acc;
}

extern "C" __global__ void syn_filter(
    double* i_acc,
    double* ampa,
    double* nmda,
    double* gaba,
    const double* vmem,
    const double nmda_voltage_sensitivity,
    const double decay_ampa,
    const double decay_nmda,
    const double decay_gaba,
    const double nmda_ratio,
    const double syn_gain,
    const int n_post
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= n_post) return;
    double val = i_acc[id];
    double exc = val > 0.0 ? val : 0.0;
    double inh = val < 0.0 ? -val : 0.0;
    double nmda_gate = 1.0;
    if (nmda_voltage_sensitivity > 0.0) {
        double x = clampd(nmda_voltage_sensitivity * (vmem[id] + 40.0), -60.0, 60.0);
        nmda_gate = 1.0 / (1.0 + exp(-x));
    }
    ampa[id] = ampa[id] * decay_ampa + exc * (1.0 - nmda_ratio);
    nmda[id] = nmda[id] * decay_nmda + exc * nmda_ratio * nmda_gate;
    gaba[id] = gaba[id] * decay_gaba + inh;
    i_acc[id] = (ampa[id] + nmda[id] - gaba[id]) * syn_gain;
}

extern "C" __global__ void stp_update(
    double* u,
    double* x,
    const signed char* pre_spk,
    double* release,
    const double stp_u,
    const double decay_rec,
    const double decay_facil,
    const int n_pre
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= n_pre) return;
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

__device__ __forceinline__ unsigned long long aarnn_mix64(unsigned long long x) {
    x ^= x >> 33;
    x *= 0xff51afd7ed558ccdULL;
    x ^= x >> 33;
    x *= 0xc4ceb9fe1a85ec53ULL;
    x ^= x >> 33;
    return x;
}

extern "C" __global__ void release_decision(
    signed char* decisions,
    const float base_probability,
    const float heterogeneity,
    const int time_low,
    const int time_high,
    const int synapse_count
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= synapse_count) return;
    unsigned long long time_step = ((unsigned long long)(unsigned int)time_high << 32)
        | (unsigned long long)(unsigned int)time_low;
    unsigned long long syn_seed = ((unsigned long long)id) * 0x9e3779b185ebca87ULL;
    unsigned long long draw_seed = syn_seed + time_step * 0xd2b74407b1ce6e93ULL;
    // Match the Rust reference's cast points exactly; release is an event
    // admission decision and must not depend on device precision.
    float draw = (float)((double)aarnn_mix64(draw_seed) / 18446744073709551615.0);
    float base = fminf(fmaxf(base_probability, 0.0f), 1.0f);
    float spread = fminf(fmaxf(heterogeneity, 0.0f), 1.0f);
    float probability = base;
    if (spread > 0.0) {
        double individual = (double)aarnn_mix64(syn_seed) / 18446744073709551615.0;
        float delta = (float)((2.0 * individual) - 1.0) * spread;
        probability = fminf(fmaxf(base + delta, 0.0f), 1.0f);
    }
    decisions[id] = (signed char)(draw <= probability);
}

extern "C" __global__ void homeostasis_decay(
    double* threshold_offset,
    double* rate_ema,
    const double threshold_decay,
    const double homeostasis_decay,
    const int update_threshold,
    const int update_rate,
    const int count
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= count) return;
    if (update_threshold != 0) threshold_offset[id] *= threshold_decay;
    if (update_rate != 0) rate_ema[id] *= homeostasis_decay;
}

extern "C" __global__ void homeostasis_spikes(
    double* threshold_offset,
    double* rate_ema,
    const signed char* spikes,
    const double homeostasis_decay,
    const double target_rate,
    const double gain,
    const int update_rate,
    const int count
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= count) return;
    if (update_rate != 0 && spikes[id] != 0) rate_ema[id] += 1.0 - homeostasis_decay;
    if (update_rate != 0) threshold_offset[id] += gain * (rate_ema[id] - target_rate);
}

extern "C" __global__ void neuromodulation(
    double* state,
    const double* targets,
    const double decay,
    const double resonance_decay,
    const double resonance_target
) {
    if (blockIdx.x * blockDim.x + threadIdx.x != 0) return;
    double d = fmin(fmax(decay, 0.0), 1.0);
    double r = fmin(fmax(resonance_decay, 0.0), 1.0);
    state[0] = state[0] * (1.0 - d) + targets[0] * d;
    state[1] = state[1] * (1.0 - d) + targets[1] * d;
    state[2] = state[2] * (1.0 - d) + targets[2] * d;
    double target = fmin(fmax(resonance_target, 0.0), 1.0);
    state[3] = state[3] * (1.0 - r) + target * r;
}

extern "C" __global__ void growth_candidates(
    const double* firing_rate,
    const double* since_growth,
    signed char* candidates,
    const double saturation_threshold,
    const double cooldown,
    const int count
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= count) return;
    candidates[id] = (signed char)(firing_rate[id] >= saturation_threshold && since_growth[id] >= cooldown);
}

extern "C" __global__ void plasticity_update(
    double* weights,
    const signed char* pre_spks,
    const signed char* post_spks,
    const double* x_pre,
    const double* x_post,
    const double eta,
    const double w_min,
    const double w_max,
    const int n_pre,
    const int n_post,
    const int rule
) {
    unsigned int j = blockIdx.x * blockDim.x + threadIdx.x;
    unsigned int i = blockIdx.y * blockDim.y + threadIdx.y;
    if ((int)j >= n_post || (int)i >= n_pre) return;
    unsigned int idx = j * n_pre + i;
    double pre = (pre_spks[i] != 0) ? 1.0 : 0.0;
    double post = (post_spks[j] != 0) ? 1.0 : 0.0;
    double dw = 0.0;
    if (rule == 0) dw = eta * (post * x_pre[i] - pre * x_post[j]);
    else if (rule == 1) dw = eta * post * pre;
    else if (rule == 2) dw = eta * (post * pre - post * post * weights[idx]);
    weights[idx] = clampd(weights[idx] + dw, w_min, w_max);
}

extern "C" __global__ void morpho_energy(
    const float4* points,
    const float4* syn_sites,
    const float* syn_stimuli,
    float* energies,
    const int n_syn,
    const float radius_sq,
    const float kernel_k,
    const int n_points
) {
    unsigned int id = blockIdx.x * blockDim.x + threadIdx.x;
    if ((int)id >= n_points) return;
    float4 p = points[id];
    float total = 0.0f;
    for (int i = 0; i < n_syn; i++) {
        float4 s = syn_sites[i];
        float dx = p.x - s.x;
        float dy = p.y - s.y;
        float dz = p.z - s.z;
        float d2 = dx*dx + dy*dy + dz*dz;
        if (d2 < radius_sq) total += syn_stimuli[i] / (1.0f + kernel_k * d2);
    }
    energies[id] = total;
}
"#;

impl OpenCLManager {
    #[allow(dead_code)]
    pub fn new() -> anyhow::Result<Self> {
        Self::new_with_device_index(0)
    }

    pub fn new_with_preferred_device_index(index: usize) -> anyhow::Result<Self> {
        let requested_backend = std::env::var("NM_GPU_BACKEND")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let force_opencl = requested_backend.as_deref() == Some("opencl");
        let force_cuda = requested_backend.as_deref() == Some("cuda");
        if let Some(value) = requested_backend.as_deref() {
            if value != "auto" && value != "opencl" && value != "cuda" {
                nm_log!(
                    "[warn] Ignoring unsupported NM_GPU_BACKEND={value:?}; using automatic latency selection."
                );
            }
        }

        let opencl_attempt = if force_cuda {
            Err(anyhow::anyhow!(
                "OpenCL GPU disabled by NM_GPU_BACKEND=cuda"
            ))
        } else {
            match gpu_device_ids() {
                Ok(devices) => {
                    match select_device_id(&devices, index, OpenCLExecutionTarget::Gpu) {
                        Ok(device_id) => Self::new_with_device_id(device_id),
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            }
        };
        let opencl_gpu_err = match &opencl_attempt {
            Ok(_) => String::new(),
            Err(error) => {
                let message = format!("OpenCL GPU initialization failed: {error}");
                nm_log!("[warn] {message}");
                message
            }
        };

        #[cfg(feature = "cuda")]
        let cuda_attempt = if force_opencl {
            Err(anyhow::anyhow!(
                "CUDA GPU disabled by NM_GPU_BACKEND=opencl"
            ))
        } else {
            let probed = nvidia_cuda_gpu_count();
            if probed > 0 {
                nm_log!("[info] NVIDIA CUDA probe detected {} GPU(s).", probed);
            } else {
                nm_log!(
                    "[info] NVIDIA CUDA probe detected no GPUs; attempting CUDA runtime initialization anyway."
                );
            }
            match Self::new_with_cuda_device_index(index) {
                Ok(manager) => Ok(manager),
                Err(error) if index != 0 => {
                    nm_log!(
                        "[warn] CUDA device index {} initialization failed: {}. Retrying index 0.",
                        index,
                        error
                    );
                    Self::new_with_cuda_device_index(0).map_err(|fallback| {
                        anyhow::anyhow!(
                            "CUDA GPU initialization failed for index {index}: {error}; index 0 retry failed: {fallback}"
                        )
                    })
                }
                Err(error) => Err(error),
            }
        };

        #[cfg(not(feature = "cuda"))]
        let cuda_attempt: anyhow::Result<Self> =
            Err(anyhow::anyhow!("binary built without `--features cuda`"));

        let cuda_err = match &cuda_attempt {
            Ok(_) => String::new(),
            Err(error) => {
                let message = format!("CUDA GPU initialization failed: {error}");
                if !force_opencl {
                    nm_log!("[warn] {message}");
                }
                message
            }
        };

        match (opencl_attempt, cuda_attempt) {
            (Ok(opencl), Ok(cuda)) => {
                if force_opencl {
                    nm_log!("[info] GPU backend forced to OpenCL by NM_GPU_BACKEND.");
                    return Ok(opencl);
                }
                if force_cuda {
                    nm_log!("[info] GPU backend forced to CUDA by NM_GPU_BACKEND.");
                    return Ok(cuda);
                }
                return Ok(Self::select_lower_latency_gpu(opencl, cuda));
            }
            (Ok(opencl), Err(error)) => {
                nm_log!("[info] Using OpenCL GPU; CUDA candidate unavailable: {error}");
                return Ok(opencl);
            }
            (Err(error), Ok(cuda)) => {
                nm_log!("[info] Using CUDA GPU; OpenCL candidate unavailable: {error}");
                return Ok(cuda);
            }
            (Err(opencl_error), Err(cuda_error)) => {
                let opencl_message = if opencl_gpu_err.is_empty() {
                    format!("OpenCL GPU initialization failed: {opencl_error}")
                } else {
                    opencl_gpu_err
                };
                let cuda_message = if cuda_err.is_empty() {
                    format!("CUDA GPU initialization failed: {cuda_error}")
                } else {
                    cuda_err
                };
                nm_log!("[warn] {}. Attempting OpenCL CPU fallback.", opencl_message);
                let cpu_devices = cpu_device_ids().map_err(|cpu_error| {
                    anyhow::anyhow!(
                        "{}. {}. OpenCL CPU discovery failed: {}",
                        opencl_message,
                        cuda_message,
                        cpu_error
                    )
                })?;
                let device_id = select_device_id(&cpu_devices, index, OpenCLExecutionTarget::Cpu)
                    .map_err(|cpu_error| {
                    anyhow::anyhow!(
                        "{}. {}. OpenCL CPU selection failed: {}",
                        opencl_message,
                        cuda_message,
                        cpu_error
                    )
                })?;
                Self::new_with_device_id(device_id).map_err(|cpu_error| {
                    anyhow::anyhow!(
                        "{}. {}. OpenCL CPU initialization failed: {}",
                        opencl_message,
                        cuda_message,
                        cpu_error
                    )
                })
            }
        }
    }

    /// Measure the same small AARNN auxiliary transaction on each candidate.
    /// The probe includes device launch and the readback boundary used by the
    /// transactional runner, so a faster kernel with an expensive transfer is
    /// not selected on kernel time alone.
    fn startup_latency_probe(&self) -> anyhow::Result<Duration> {
        let mut threshold = vec![0.0f64; 256];
        let mut rates = vec![0.25f64; 256];
        let start = Instant::now();
        for _ in 0..4 {
            self.homeostasis_decay(&mut threshold, &mut rates, 0.97, 0.99, true, true)?;
        }
        self.queue.finish().map_err(|error| {
            anyhow::anyhow!("GPU latency probe synchronization failed: {error}")
        })?;
        Ok(start.elapsed())
    }

    fn select_lower_latency_gpu(opencl: Self, cuda: Self) -> Self {
        let opencl_latency = opencl.startup_latency_probe();
        let cuda_latency = cuda.startup_latency_probe();
        match (opencl_latency, cuda_latency) {
            (Ok(opencl_time), Ok(cuda_time)) => {
                nm_log!(
                    "[info] GPU latency selection: OpenCL={:.3} ms, CUDA={:.3} ms; selected {}.",
                    opencl_time.as_secs_f64() * 1_000.0,
                    cuda_time.as_secs_f64() * 1_000.0,
                    if cuda_time < opencl_time {
                        "CUDA"
                    } else {
                        "OpenCL"
                    }
                );
                if cuda_time < opencl_time {
                    cuda
                } else {
                    opencl
                }
            }
            (Ok(opencl_time), Err(cuda_error)) => {
                nm_log!(
                    "[warn] CUDA latency probe failed: {cuda_error}; using OpenCL ({:.3} ms).",
                    opencl_time.as_secs_f64() * 1_000.0
                );
                opencl
            }
            (Err(opencl_error), Ok(cuda_time)) => {
                nm_log!(
                    "[warn] OpenCL latency probe failed: {opencl_error}; using CUDA ({:.3} ms).",
                    cuda_time.as_secs_f64() * 1_000.0
                );
                cuda
            }
            (Err(opencl_error), Err(cuda_error)) => {
                nm_log!(
                    "[warn] Both GPU latency probes failed (OpenCL: {opencl_error}; CUDA: {cuda_error}); retaining OpenCL candidate."
                );
                opencl
            }
        }
    }

    #[cfg(feature = "cuda")]
    pub fn new_with_cuda_device_index(index: usize) -> anyhow::Result<Self> {
        let device = Device::cuda(index);
        Self::new_with_device(device)
    }

    pub fn new_with_device_index(index: usize) -> anyhow::Result<Self> {
        let devices = gpu_device_ids()?;
        let device_id = *devices
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("GPU device index {} out of range", index))?;
        Self::new_with_device_id(device_id)
    }

    pub fn new_with_device_id(device_id: cl_device_id) -> anyhow::Result<Self> {
        let device = Device::new(device_id);
        Self::new_with_device(device)
    }

    fn new_with_device(device: Device) -> anyhow::Result<Self> {
        let execution_target = match device.dev_type() {
            Ok(t) if (t & CL_DEVICE_TYPE_GPU) != 0 => OpenCLExecutionTarget::Gpu,
            Ok(_) => OpenCLExecutionTarget::Cpu,
            Err(_) => OpenCLExecutionTarget::Cpu,
        };
        let context =
            Context::from_device(&device).map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;
        let is_cuda_backend = context.is_cuda();
        let queue = unsafe {
            CommandQueue::create_with_properties(&context, device.id(), 0, 0)
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?
        };
        let program_source = if is_cuda_backend {
            #[cfg(feature = "cuda")]
            {
                CUDA_PROGRAM_SOURCE
            }
            #[cfg(not(feature = "cuda"))]
            {
                PROGRAM_SOURCE
            }
        } else {
            PROGRAM_SOURCE
        };

        let program = Program::create_and_build_from_source(&context, program_source, "")
            .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?;

        let kernel_lif_step = Mutex::new(
            Kernel::create(&program, "lif_step")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_izh_step = Mutex::new(
            Kernel::create(&program, "izh_step")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_aarnn_step = Mutex::new(
            Kernel::create(&program, "aarnn_step")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc = Mutex::new(
            Kernel::create(&program, "syn_acc_dense")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_stp = Mutex::new(
            Kernel::create(&program, "syn_acc_dense_stp")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_sparse = Mutex::new(
            Kernel::create(&program, "syn_acc_sparse")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_sparse_stp = Mutex::new(
            Kernel::create(&program, "syn_acc_sparse_stp")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_sparse_delay = Mutex::new(
            Kernel::create(&program, "syn_acc_sparse_delay")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_sparse_delay_stp = Mutex::new(
            Kernel::create(&program, "syn_acc_sparse_delay_stp")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_sparse_delay_release = Mutex::new(
            Kernel::create(&program, "syn_acc_sparse_delay_release")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_acc_sparse_delay_release_stp = Mutex::new(
            Kernel::create(&program, "syn_acc_sparse_delay_release_stp")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_syn_filter = Mutex::new(
            Kernel::create(&program, "syn_filter")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_stp_update = Mutex::new(
            Kernel::create(&program, "stp_update")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_release_decision = Mutex::new(
            Kernel::create(&program, "release_decision")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_homeostasis_decay = Mutex::new(
            Kernel::create(&program, "homeostasis_decay")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_homeostasis_spikes = Mutex::new(
            Kernel::create(&program, "homeostasis_spikes")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_neuromodulation = Mutex::new(
            Kernel::create(&program, "neuromodulation")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_growth_candidates = Mutex::new(
            Kernel::create(&program, "growth_candidates")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_plasticity_update = Mutex::new(
            Kernel::create(&program, "plasticity_update")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );
        let kernel_morpho_energy = Mutex::new(
            Kernel::create(&program, "morpho_energy")
                .map_err(|e| anyhow::anyhow!("OpenCL error: {}", e))?,
        );

        let manager = Self {
            device,
            execution_target,
            is_cuda_backend,
            context,
            queue,
            program,
            kernel_lif_step,
            kernel_izh_step,
            kernel_aarnn_step,
            kernel_syn_acc,
            kernel_syn_acc_stp,
            kernel_syn_acc_sparse,
            kernel_syn_acc_sparse_stp,
            kernel_syn_acc_sparse_delay,
            kernel_syn_acc_sparse_delay_stp,
            kernel_syn_acc_sparse_delay_release,
            kernel_syn_acc_sparse_delay_release_stp,
            kernel_syn_filter,
            kernel_stp_update,
            kernel_release_decision,
            kernel_homeostasis_decay,
            kernel_homeostasis_spikes,
            kernel_neuromodulation,
            kernel_growth_candidates,
            kernel_plasticity_update,
            kernel_morpho_energy,
        };
        manager.verify_reference_equivalence()?;
        Ok(manager)
    }

    pub fn execution_target(&self) -> OpenCLExecutionTarget {
        self.execution_target
    }

    pub fn is_cuda_backend(&self) -> bool {
        self.is_cuda_backend
    }

    /// Execute bounded reference vectors on the selected device before it is
    /// allowed to service biological state.  A device that cannot reproduce
    /// the shared CPU transition is rejected and the caller falls back to the
    /// deterministic software path.  This is a certification gate, not a
    /// biological-validation claim.
    fn verify_reference_equivalence(&self) -> anyhow::Result<()> {
        const N: usize = 4;
        let lif_params = LIFParams::default();
        let lif_v = [0.0, 0.5, 1.5, -3.0];
        let lif_refr = [0, 2, 0, 1];
        let lif_current = [2.0, 2.0, -0.25, 9.0];
        let decay = 0.95;
        let mut v_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut refr_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<i32>(),
                ptr::null_mut(),
            )
        }?;
        let mut current_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let spk_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut v_buf, CL_TRUE, 0, &lif_v, &[])?;
            self.queue
                .enqueue_write_buffer(&mut refr_buf, CL_TRUE, 0, &lif_refr, &[])?;
            self.queue
                .enqueue_write_buffer(&mut current_buf, CL_TRUE, 0, &lif_current, &[])?;
            let kernel = self.kernel_lif_step.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&v_buf)
                .set_arg(&refr_buf)
                .set_arg(&current_buf)
                .set_arg(&decay)
                .set_arg(&lif_params.v_th)
                .set_arg(&lif_params.v_reset)
                .set_arg(&(lif_params.refractory as i32))
                .set_arg(&spk_buf)
                .set_global_work_size(N)
                .enqueue_nd_range(&self.queue)?;
        }
        let mut actual_v = [0.0; N];
        let mut actual_refr = [0; N];
        let mut actual_spk = [0; N];
        unsafe {
            self.queue
                .enqueue_read_buffer(&v_buf, CL_TRUE, 0, &mut actual_v, &[])?;
            self.queue
                .enqueue_read_buffer(&refr_buf, CL_TRUE, 0, &mut actual_refr, &[])?;
            self.queue
                .enqueue_read_buffer(&spk_buf, CL_TRUE, 0, &mut actual_spk, &[])?;
        }
        for i in 0..N {
            let expected = lif_transition(lif_v[i], lif_refr[i], lif_current[i], decay, lif_params);
            if !actual_v[i].is_finite()
                || (actual_v[i] - expected.voltage).abs() > 1.0e-12
                || actual_refr[i] != expected.refractory
                || actual_spk[i] != expected.fired as i8
            {
                anyhow::bail!("device LIF reference mismatch at index {}", i);
            }
        }

        let izh_params = IzhikevichParams::from_preset("RS", 1.0);
        let izh_v = [f64::NAN, -65.0, 30.0, -80.0];
        let izh_u = [f64::INFINITY, -13.0, -12.0, -20.0];
        let izh_current = [0.0, 1_000.0, 0.0, -2.0];
        let mut izh_v_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut izh_u_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut izh_current_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let izh_spk_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut izh_v_buf, CL_TRUE, 0, &izh_v, &[])?;
            self.queue
                .enqueue_write_buffer(&mut izh_u_buf, CL_TRUE, 0, &izh_u, &[])?;
            self.queue
                .enqueue_write_buffer(&mut izh_current_buf, CL_TRUE, 0, &izh_current, &[])?;
            let kernel = self.kernel_izh_step.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&izh_v_buf)
                .set_arg(&izh_u_buf)
                .set_arg(&izh_current_buf)
                .set_arg(&izh_params.dt)
                .set_arg(&izh_params.recovery_time_constant_a)
                .set_arg(&izh_params.recovery_sensitivity_b)
                .set_arg(&izh_params.membrane_reset_potential_c)
                .set_arg(&izh_params.recovery_increment_d)
                .set_arg(&izh_params.v_th)
                .set_arg(&izh_spk_buf)
                .set_global_work_size(N)
                .enqueue_nd_range(&self.queue)?;
        }
        let mut actual_izh_v = [0.0; N];
        let mut actual_izh_u = [0.0; N];
        let mut actual_izh_spk = [0; N];
        unsafe {
            self.queue
                .enqueue_read_buffer(&izh_v_buf, CL_TRUE, 0, &mut actual_izh_v, &[])?;
            self.queue
                .enqueue_read_buffer(&izh_u_buf, CL_TRUE, 0, &mut actual_izh_u, &[])?;
            self.queue
                .enqueue_read_buffer(&izh_spk_buf, CL_TRUE, 0, &mut actual_izh_spk, &[])?;
        }
        for i in 0..N {
            let expected = izh_transition(
                izh_v[i],
                izh_u[i],
                izh_current[i],
                izh_params,
                0.0,
                false,
                0.0,
                0.0,
                0.0,
                None,
                0,
            );
            if !actual_izh_v[i].is_finite()
                || !actual_izh_u[i].is_finite()
                || (actual_izh_v[i] - expected.voltage).abs() > 1.0e-10
                || (actual_izh_u[i] - expected.recovery).abs() > 1.0e-10
                || actual_izh_spk[i] != expected.fired as i8
            {
                anyhow::bail!("device Izhikevich reference mismatch at index {}", i);
            }
        }

        // AARNN uses a separate transition kernel because adaptive threshold
        // and optional refractory state are biological state, not a cosmetic
        // Izhikevich label. Exercise both enabled and disabled branches and
        // compare every returned state against the canonical CPU transition.
        let aarnn_v = [-65.0, -62.0, f64::NAN, -80.0];
        let aarnn_u = [-13.0, -12.0, f64::INFINITY, -20.0];
        let aarnn_current = [1_000.0, 0.0, 0.0, -2.0];
        let aarnn_threshold = [0.0, 3.0, 1.0, 0.0];
        let aarnn_refr = [0, 2, 0, 1];
        let aarnn_params = IzhikevichParams::from_preset("RS", 1.0);
        let aarnn_increment = 2.0;
        let aarnn_min = 0.0;
        let aarnn_max = 10.0;
        let aarnn_adaptive = 1i32;
        let aarnn_refractory_enabled = 1i32;
        let aarnn_refractory_steps = 3i32;
        let mut aarnn_v_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut aarnn_u_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut aarnn_i_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                N * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut aarnn_threshold_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut aarnn_refr_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * std::mem::size_of::<i32>(),
                ptr::null_mut(),
            )
        }?;
        let aarnn_spk_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut aarnn_v_buf, CL_TRUE, 0, &aarnn_v, &[])?;
            self.queue
                .enqueue_write_buffer(&mut aarnn_u_buf, CL_TRUE, 0, &aarnn_u, &[])?;
            self.queue
                .enqueue_write_buffer(&mut aarnn_i_buf, CL_TRUE, 0, &aarnn_current, &[])?;
            self.queue.enqueue_write_buffer(
                &mut aarnn_threshold_buf,
                CL_TRUE,
                0,
                &aarnn_threshold,
                &[],
            )?;
            self.queue
                .enqueue_write_buffer(&mut aarnn_refr_buf, CL_TRUE, 0, &aarnn_refr, &[])?;
            let kernel = self.kernel_aarnn_step.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&aarnn_v_buf)
                .set_arg(&aarnn_u_buf)
                .set_arg(&aarnn_i_buf)
                .set_arg(&aarnn_threshold_buf)
                .set_arg(&aarnn_refr_buf)
                .set_arg(&aarnn_params.dt)
                .set_arg(&aarnn_params.recovery_time_constant_a)
                .set_arg(&aarnn_params.recovery_sensitivity_b)
                .set_arg(&aarnn_params.membrane_reset_potential_c)
                .set_arg(&aarnn_params.recovery_increment_d)
                .set_arg(&aarnn_params.v_th)
                .set_arg(&aarnn_increment)
                .set_arg(&aarnn_min)
                .set_arg(&aarnn_max)
                .set_arg(&aarnn_adaptive)
                .set_arg(&aarnn_refractory_enabled)
                .set_arg(&aarnn_refractory_steps)
                .set_arg(&aarnn_spk_buf)
                .set_global_work_size(N)
                .enqueue_nd_range(&self.queue)?;
        }
        let mut actual_aarnn_v = [0.0; N];
        let mut actual_aarnn_u = [0.0; N];
        let mut actual_aarnn_threshold = [0.0; N];
        let mut actual_aarnn_refr = [0i32; N];
        let mut actual_aarnn_spk = [0i8; N];
        unsafe {
            self.queue
                .enqueue_read_buffer(&aarnn_v_buf, CL_TRUE, 0, &mut actual_aarnn_v, &[])?;
            self.queue
                .enqueue_read_buffer(&aarnn_u_buf, CL_TRUE, 0, &mut actual_aarnn_u, &[])?;
            self.queue.enqueue_read_buffer(
                &aarnn_threshold_buf,
                CL_TRUE,
                0,
                &mut actual_aarnn_threshold,
                &[],
            )?;
            self.queue.enqueue_read_buffer(
                &aarnn_refr_buf,
                CL_TRUE,
                0,
                &mut actual_aarnn_refr,
                &[],
            )?;
            self.queue.enqueue_read_buffer(
                &aarnn_spk_buf,
                CL_TRUE,
                0,
                &mut actual_aarnn_spk,
                &[],
            )?;
        }
        for i in 0..N {
            let expected = izh_transition(
                aarnn_v[i],
                aarnn_u[i],
                aarnn_current[i],
                aarnn_params,
                aarnn_threshold[i],
                true,
                aarnn_increment,
                aarnn_min,
                aarnn_max,
                Some(aarnn_refr[i]),
                aarnn_refractory_steps,
            );
            if (actual_aarnn_v[i] - expected.voltage).abs() > 1.0e-12
                || (actual_aarnn_u[i] - expected.recovery).abs() > 1.0e-12
                || (actual_aarnn_threshold[i] - expected.threshold_offset).abs() > 1.0e-12
                || actual_aarnn_refr[i] != expected.refractory
                || actual_aarnn_spk[i] != expected.fired as i8
            {
                anyhow::bail!("device AARNN transition mismatch at index {}", i);
            }
        }

        let pre = [0, 1, 0, 1];
        let params = ShortTermPlasticityParams {
            baseline_utilization: 0.2,
            recovery_decay: 0.9,
            facilitation_decay: 0.9,
        };
        let mut pre_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                N * size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        let mut stp_u_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut stp_x_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let release_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                N * size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let initial_u = [0.2, 0.4, 0.1, 0.8];
        let initial_x = [1.0, 0.7, 0.3, 0.9];
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut pre_buf, CL_TRUE, 0, &pre, &[])?;
            self.queue
                .enqueue_write_buffer(&mut stp_u_buf, CL_TRUE, 0, &initial_u, &[])?;
            self.queue
                .enqueue_write_buffer(&mut stp_x_buf, CL_TRUE, 0, &initial_x, &[])?;
            let kernel = self.kernel_stp_update.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&stp_u_buf)
                .set_arg(&stp_x_buf)
                .set_arg(&pre_buf)
                .set_arg(&release_buf)
                .set_arg(&params.baseline_utilization)
                .set_arg(&params.recovery_decay)
                .set_arg(&params.facilitation_decay)
                .set_global_work_size(N)
                .enqueue_nd_range(&self.queue)?;
        }
        let mut actual_release = [0.0; N];
        let mut actual_pre = [0_i8; N];
        let mut actual_u = [0.0; N];
        let mut actual_x = [0.0; N];
        unsafe {
            self.queue
                .enqueue_read_buffer(&release_buf, CL_TRUE, 0, &mut actual_release, &[])?;
            self.queue
                .enqueue_read_buffer(&pre_buf, CL_TRUE, 0, &mut actual_pre, &[])?;
            self.queue
                .enqueue_read_buffer(&stp_u_buf, CL_TRUE, 0, &mut actual_u, &[])?;
            self.queue
                .enqueue_read_buffer(&stp_x_buf, CL_TRUE, 0, &mut actual_x, &[])?;
        }
        for i in 0..N {
            let expected = stp_step(
                &mut ShortTermPlasticityState {
                    utilization: initial_u[i],
                    available_resources: initial_x[i],
                },
                pre[i] != 0,
                params,
            );
            if (actual_release[i] - expected).abs() > 1.0e-12 {
                anyhow::bail!(
                    "device STP reference mismatch at index {}: pre={} actual={:.17e}, expected={:.17e}, u={:.17e}, x={:.17e}",
                    i,
                    actual_pre[i],
                    actual_release[i],
                    expected,
                    actual_u[i],
                    actual_x[i]
                );
            }
        }
        self.verify_auxiliary_kernel_equivalence()?;
        self.verify_full_aarnn_kernel_equivalence()?;
        Ok(())
    }

    fn verify_full_aarnn_kernel_equivalence(&self) -> anyhow::Result<()> {
        let base = 0.61f32;
        let heterogeneity = 0.17f32;
        let time_step = 37u64;
        let actual_release = self.release_decisions(base, heterogeneity, time_step, 16)?;
        for (index, actual) in actual_release.iter().copied().enumerate() {
            let expected = release_draw(index, time_step)
                <= release_probability(base, heterogeneity, Some(index), time_step);
            if actual != expected as i8 {
                anyhow::bail!("device release decision mismatch at index {index}");
            }
        }

        let mut threshold = [0.5f64, -0.25, 1.25, 0.0];
        let mut rates = [0.1f64, 0.4, 1.0, 2.0];
        let spikes = [1i8, 0, 1, 0];
        let threshold_decay = 0.91;
        let homeostasis_decay = 0.83;
        let target = 0.7;
        let gain = 0.22;
        let mut expected_threshold = threshold;
        let mut expected_rates = rates;
        for value in &mut expected_threshold {
            *value *= threshold_decay;
        }
        for value in &mut expected_rates {
            *value *= homeostasis_decay;
        }
        self.homeostasis_decay(
            &mut threshold,
            &mut rates,
            threshold_decay,
            homeostasis_decay,
            true,
            true,
        )?;
        for index in 0..spikes.len() {
            if spikes[index] != 0 {
                expected_rates[index] += 1.0 - homeostasis_decay;
            }
            expected_threshold[index] += gain * (expected_rates[index] - target);
        }
        self.homeostasis_spikes(
            &mut threshold,
            &mut rates,
            &spikes,
            homeostasis_decay,
            target,
            gain,
            true,
        )?;
        for index in 0..spikes.len() {
            if (threshold[index] - expected_threshold[index]).abs() > 1.0e-12
                || (rates[index] - expected_rates[index]).abs() > 1.0e-12
            {
                anyhow::bail!("device homeostasis mismatch at index {index}");
            }
        }

        let mut state = [0.8f64, 1.2, 0.4, 0.1];
        let targets = [1.4, 0.7, 1.1];
        let decay = 0.13;
        let resonance_decay = 0.27;
        let resonance_target = 0.65;
        let mut expected_state = state;
        for index in 0..3 {
            expected_state[index] = expected_state[index] * (1.0 - decay) + targets[index] * decay;
        }
        expected_state[3] =
            expected_state[3] * (1.0 - resonance_decay) + resonance_target * resonance_decay;
        self.neuromodulation_step(
            &mut state,
            targets,
            decay,
            resonance_decay,
            resonance_target,
        )?;
        for index in 0..4 {
            if (state[index] - expected_state[index]).abs() > 1.0e-12 {
                anyhow::bail!("device neuromodulation mismatch at index {index}");
            }
        }

        let firing_rate = [0.1f64, 0.8, 0.9, 0.2];
        let since_growth = [4.0f64, 1.0, 6.0, 9.0];
        let candidates = self.growth_candidates(&firing_rate, &since_growth, 0.75, 3.0)?;
        let expected_candidates = [0i8, 0, 1, 0];
        if candidates != expected_candidates {
            anyhow::bail!("device growth candidate mismatch");
        }
        Ok(())
    }

    /// Verify the device stages that surround the AARNN membrane transition.
    ///
    /// These checks deliberately use the same small, ordered vectors as the
    /// reference implementation.  Device kernels cover delayed accumulation,
    /// filtering, STP, plasticity, morphology energy, release decisions,
    /// adaptive/homeostatic state, neuromodulation and growth eligibility.
    /// Structural topology publication and event ordering remain CPU commit
    /// boundaries by design.  A failed check rejects the device at construction
    /// time, so a production run cannot silently mix an unverified kernel into
    /// an AARNN transition.
    fn verify_auxiliary_kernel_equivalence(&self) -> anyhow::Result<()> {
        const N_POST: usize = 2;
        const N_PRE: usize = 3;
        const HIST_LEN: usize = 3;

        let history: [i8; HIST_LEN * N_PRE] = [1, 0, 1, 0, 1, 0, 1, 1, 0];
        let row_ptr = [0i32, 2, 3];
        let col_indices = [0i32, 1, 2];
        let delays = [0i32, 1, 2];
        let weights = [0.5f64, -0.25, 0.75];
        let mut acc = [0.0f64; N_POST];
        let mut history_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                history.len() * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        let mut row_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                row_ptr.len() * std::mem::size_of::<i32>(),
                ptr::null_mut(),
            )
        }?;
        let mut col_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                col_indices.len() * std::mem::size_of::<i32>(),
                ptr::null_mut(),
            )
        }?;
        let mut delay_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                delays.len() * std::mem::size_of::<i32>(),
                ptr::null_mut(),
            )
        }?;
        let mut weight_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                weights.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut acc_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                acc.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut history_buf, CL_TRUE, 0, &history, &[])?;
            self.queue
                .enqueue_write_buffer(&mut row_buf, CL_TRUE, 0, &row_ptr, &[])?;
            self.queue
                .enqueue_write_buffer(&mut col_buf, CL_TRUE, 0, &col_indices, &[])?;
            self.queue
                .enqueue_write_buffer(&mut delay_buf, CL_TRUE, 0, &delays, &[])?;
            self.queue
                .enqueue_write_buffer(&mut weight_buf, CL_TRUE, 0, &weights, &[])?;
            let kernel = self.kernel_syn_acc_sparse_delay.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&acc_buf)
                .set_arg(&history_buf)
                .set_arg(&row_buf)
                .set_arg(&col_buf)
                .set_arg(&delay_buf)
                .set_arg(&weight_buf)
                .set_arg(&(N_POST as i32))
                .set_arg(&(HIST_LEN as i32))
                .set_arg(&(N_PRE as i32))
                .set_arg(&0i32)
                .set_global_work_size(N_POST)
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(&acc_buf, CL_TRUE, 0, &mut acc, &[])?;
        }
        let expected_delay = [0.25, 0.0];
        for (index, (&actual, &expected)) in acc.iter().zip(expected_delay.iter()).enumerate() {
            if (actual - expected).abs() > 1.0e-12 {
                anyhow::bail!("device delayed accumulation mismatch at index {index}");
            }
        }

        let mut filter_i = [2.0f64, -1.0];
        let mut filter_ampa = [0.1f64, 0.2];
        let mut filter_nmda = [0.3f64, 0.4];
        let mut filter_gaba = [0.5f64, 0.6];
        let filter_vmem = [-60.0f64, -20.0];
        let decay_ampa = 0.9;
        let decay_nmda = 0.8;
        let decay_gaba = 0.7;
        let nmda_ratio = 0.25;
        let syn_gain = 1.2;
        let nmda_voltage_sensitivity = 0.05;
        let mut filter_i_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                filter_i.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut filter_ampa_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                filter_ampa.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut filter_nmda_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                filter_nmda.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut filter_gaba_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                filter_gaba.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut filter_vmem_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                filter_vmem.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut filter_i_buf, CL_TRUE, 0, &filter_i, &[])?;
            self.queue
                .enqueue_write_buffer(&mut filter_ampa_buf, CL_TRUE, 0, &filter_ampa, &[])?;
            self.queue
                .enqueue_write_buffer(&mut filter_nmda_buf, CL_TRUE, 0, &filter_nmda, &[])?;
            self.queue
                .enqueue_write_buffer(&mut filter_gaba_buf, CL_TRUE, 0, &filter_gaba, &[])?;
            self.queue
                .enqueue_write_buffer(&mut filter_vmem_buf, CL_TRUE, 0, &filter_vmem, &[])?;
            let kernel = self.kernel_syn_filter.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&filter_i_buf)
                .set_arg(&filter_ampa_buf)
                .set_arg(&filter_nmda_buf)
                .set_arg(&filter_gaba_buf)
                .set_arg(&filter_vmem_buf)
                .set_arg(&nmda_voltage_sensitivity)
                .set_arg(&decay_ampa)
                .set_arg(&decay_nmda)
                .set_arg(&decay_gaba)
                .set_arg(&nmda_ratio)
                .set_arg(&syn_gain)
                .set_global_work_size(filter_i.len())
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(&filter_i_buf, CL_TRUE, 0, &mut filter_i, &[])?;
            self.queue
                .enqueue_read_buffer(&filter_ampa_buf, CL_TRUE, 0, &mut filter_ampa, &[])?;
            self.queue
                .enqueue_read_buffer(&filter_nmda_buf, CL_TRUE, 0, &mut filter_nmda, &[])?;
            self.queue
                .enqueue_read_buffer(&filter_gaba_buf, CL_TRUE, 0, &mut filter_gaba, &[])?;
        }
        let nmda_gate = 1.0 / (1.0 + 1.0f64.exp());
        let expected_filter = [
            (
                1.59,
                0.24 + 0.5 * nmda_gate,
                0.35,
                (1.59 + 0.24 + 0.5 * nmda_gate - 0.35) * 1.2,
            ),
            (0.18, 0.32, 1.42, -1.104),
        ];
        for (index, ((&actual_i, &actual_a), (&actual_n, &actual_g))) in filter_i
            .iter()
            .zip(filter_ampa.iter())
            .zip(filter_nmda.iter().zip(filter_gaba.iter()))
            .enumerate()
        {
            let (expected_a, expected_n, expected_g, expected_i) = expected_filter[index];
            if (actual_a - expected_a).abs() > 1.0e-12
                || (actual_n - expected_n).abs() > 1.0e-12
                || (actual_g - expected_g).abs() > 1.0e-12
                || (actual_i - expected_i).abs() > 1.0e-12
            {
                anyhow::bail!(
                    "device synaptic filter mismatch at index {index}: actual=({actual_a:.17e}, {actual_n:.17e}, {actual_g:.17e}, {actual_i:.17e}) expected=({expected_a:.17e}, {expected_n:.17e}, {expected_g:.17e}, {expected_i:.17e})"
                );
            }
        }

        let mut plasticity_weights = [0.2f64, -0.3, 0.4, 0.5];
        let plasticity_pre = [1i8, 0];
        let plasticity_post = [1i8, 1];
        let x_pre = [0.4f64, 0.2];
        let x_post = [0.1f64, 0.3];
        let eta = 0.1;
        let w_min = -1.0;
        let w_max = 1.0;
        let rule = 0i32;
        let mut plasticity_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                plasticity_weights.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut plasticity_pre_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                plasticity_pre.len() * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        let mut plasticity_post_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                plasticity_post.len() * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        let mut x_pre_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                x_pre.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut x_post_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                x_post.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue.enqueue_write_buffer(
                &mut plasticity_buf,
                CL_TRUE,
                0,
                &plasticity_weights,
                &[],
            )?;
            self.queue.enqueue_write_buffer(
                &mut plasticity_pre_buf,
                CL_TRUE,
                0,
                &plasticity_pre,
                &[],
            )?;
            self.queue.enqueue_write_buffer(
                &mut plasticity_post_buf,
                CL_TRUE,
                0,
                &plasticity_post,
                &[],
            )?;
            self.queue
                .enqueue_write_buffer(&mut x_pre_buf, CL_TRUE, 0, &x_pre, &[])?;
            self.queue
                .enqueue_write_buffer(&mut x_post_buf, CL_TRUE, 0, &x_post, &[])?;
            let kernel = self.kernel_plasticity_update.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&plasticity_buf)
                .set_arg(&plasticity_pre_buf)
                .set_arg(&plasticity_post_buf)
                .set_arg(&x_pre_buf)
                .set_arg(&x_post_buf)
                .set_arg(&eta)
                .set_arg(&w_min)
                .set_arg(&w_max)
                .set_arg(&2i32)
                .set_arg(&2i32)
                .set_arg(&rule)
                .set_global_work_sizes(&[2, 2])
                .enqueue_nd_range(&self.queue)?;
            self.queue.enqueue_read_buffer(
                &plasticity_buf,
                CL_TRUE,
                0,
                &mut plasticity_weights,
                &[],
            )?;
        }
        let expected_weights = [0.23, -0.28, 0.41, 0.52];
        for (index, (&actual, &expected)) in plasticity_weights
            .iter()
            .zip(expected_weights.iter())
            .enumerate()
        {
            if (actual - expected).abs() > 1.0e-12 {
                anyhow::bail!("device plasticity mismatch at index {index}");
            }
        }

        let points = [[0.0f32, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];
        let syn_sites = [[0.1f32, 0.0, 0.0, 0.0], [0.0, 0.5, 0.0, 0.0]];
        let syn_stimuli = [2.0f32, -1.0];
        let radius_sq = 1.0f32;
        let kernel_k = 0.5f32;
        let mut energies = [0.0f32; 2];
        let mut points_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                points.len() * std::mem::size_of::<[f32; 4]>(),
                ptr::null_mut(),
            )
        }?;
        let mut syn_sites_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                syn_sites.len() * std::mem::size_of::<[f32; 4]>(),
                ptr::null_mut(),
            )
        }?;
        let mut syn_stimuli_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                syn_stimuli.len() * std::mem::size_of::<f32>(),
                ptr::null_mut(),
            )
        }?;
        let mut energies_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                energies.len() * std::mem::size_of::<f32>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut points_buf, CL_TRUE, 0, &points, &[])?;
            self.queue
                .enqueue_write_buffer(&mut syn_sites_buf, CL_TRUE, 0, &syn_sites, &[])?;
            self.queue
                .enqueue_write_buffer(&mut syn_stimuli_buf, CL_TRUE, 0, &syn_stimuli, &[])?;
            let kernel = self.kernel_morpho_energy.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&points_buf)
                .set_arg(&syn_sites_buf)
                .set_arg(&syn_stimuli_buf)
                .set_arg(&energies_buf)
                .set_arg(&2i32)
                .set_arg(&radius_sq)
                .set_arg(&kernel_k)
                .set_global_work_size(points.len())
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(&energies_buf, CL_TRUE, 0, &mut energies, &[])?;
        }
        let expected_energy = [
            2.0 / (1.0 + 0.5 * 0.1 * 0.1) - 1.0 / (1.0 + 0.5 * 0.5 * 0.5),
            2.0 / (1.0 + 0.5 * 0.9 * 0.9),
        ];
        for (index, (&actual, &expected)) in energies.iter().zip(expected_energy.iter()).enumerate()
        {
            if (actual as f64 - expected).abs() > 2.0e-6 {
                anyhow::bail!("device morphology energy mismatch at index {index}");
            }
        }
        Ok(())
    }

    /// Run deterministic per-synapse release decisions on the certified device.
    /// The returned byte vector is safe to use as an event-admission mask; no
    /// topology or event ordering state is mutated by this method.
    /// Run one complete matrix plasticity transaction and return staged device
    /// weights.  The caller publishes the returned vector only after the read
    /// succeeds, so a launch or transfer error leaves the authoritative host
    /// matrix unchanged and can be replayed by the CPU reference path.
    pub fn plasticity_update_matrix(
        &self,
        weights: &mut Buffer<f64>,
        pre_spikes: &mut Buffer<i8>,
        post_spikes: &mut Buffer<i8>,
        pre_trace: &mut Buffer<f64>,
        post_trace: &mut Buffer<f64>,
        pre_spikes_host: &[i8],
        post_spikes_host: &[i8],
        pre_trace_host: &[f64],
        post_trace_host: &[f64],
        eta: f64,
        w_min: f64,
        w_max: f64,
        n_pre: usize,
        n_post: usize,
        rule: i32,
    ) -> anyhow::Result<Vec<f64>> {
        if pre_spikes_host.len() != n_pre
            || post_spikes_host.len() != n_post
            || pre_trace_host.len() != n_pre
            || post_trace_host.len() != n_post
        {
            anyhow::bail!(
                "plasticity buffer shape mismatch: pre={}/{} post={}/{} pre_trace={}/{} post_trace={}/{}",
                pre_spikes_host.len(),
                n_pre,
                post_spikes_host.len(),
                n_post,
                pre_trace_host.len(),
                n_pre,
                post_trace_host.len(),
                n_post
            );
        }
        let weight_count = n_pre.saturating_mul(n_post);
        let mut staged_weights = vec![0.0; weight_count];
        unsafe {
            self.queue
                .enqueue_write_buffer(pre_spikes, CL_TRUE, 0, pre_spikes_host, &[])?;
            self.queue
                .enqueue_write_buffer(post_spikes, CL_TRUE, 0, post_spikes_host, &[])?;
            self.queue
                .enqueue_write_buffer(pre_trace, CL_TRUE, 0, pre_trace_host, &[])?;
            self.queue
                .enqueue_write_buffer(post_trace, CL_TRUE, 0, post_trace_host, &[])?;
            let kernel = self.kernel_plasticity_update.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&mut *weights)
                .set_arg(pre_spikes)
                .set_arg(post_spikes)
                .set_arg(pre_trace)
                .set_arg(post_trace)
                .set_arg(&eta)
                .set_arg(&w_min)
                .set_arg(&w_max)
                .set_arg(&(n_pre as i32))
                .set_arg(&(n_post as i32))
                .set_arg(&rule)
                .set_global_work_sizes(&[n_post, n_pre])
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(weights, CL_TRUE, 0, &mut staged_weights, &[])?;
        }
        Ok(staged_weights)
    }

    pub fn release_decisions(
        &self,
        base_probability: f32,
        heterogeneity: f32,
        time_step: u64,
        synapse_count: usize,
    ) -> anyhow::Result<Vec<i8>> {
        if synapse_count == 0 {
            return Ok(Vec::new());
        }
        let mut decisions = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                synapse_count * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            let kernel = self.kernel_release_decision.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&mut decisions)
                .set_arg(&base_probability)
                .set_arg(&heterogeneity)
                .set_arg(&(time_step as u32 as i32))
                .set_arg(&((time_step >> 32) as u32 as i32))
                .set_arg(&(synapse_count as i32))
                .set_global_work_size(synapse_count)
                .enqueue_nd_range(&self.queue)?;
        }
        let mut out = vec![0i8; synapse_count];
        unsafe {
            self.queue
                .enqueue_read_buffer(&decisions, CL_TRUE, 0, &mut out, &[])?;
        }
        Ok(out)
    }

    /// Apply the pre-neuron adaptive-threshold/homeostatic decay phase on the
    /// device.  The slices are read back before returning, making the call a
    /// transactional stage with a straightforward CPU fallback.
    pub fn homeostasis_decay(
        &self,
        threshold_offset: &mut [f64],
        rate_ema: &mut [f64],
        threshold_decay: f64,
        homeostasis_decay: f64,
        update_threshold: bool,
        update_rate: bool,
    ) -> anyhow::Result<()> {
        let count = threshold_offset.len().min(rate_ema.len());
        if count == 0 {
            return Ok(());
        }
        let mut threshold_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                count * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut rate_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                count * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut next_threshold = vec![0.0; count];
        let mut next_rate = vec![0.0; count];
        unsafe {
            self.queue.enqueue_write_buffer(
                &mut threshold_buf,
                CL_TRUE,
                0,
                &threshold_offset[..count],
                &[],
            )?;
            self.queue
                .enqueue_write_buffer(&mut rate_buf, CL_TRUE, 0, &rate_ema[..count], &[])?;
            let kernel = self.kernel_homeostasis_decay.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&mut threshold_buf)
                .set_arg(&mut rate_buf)
                .set_arg(&threshold_decay)
                .set_arg(&homeostasis_decay)
                .set_arg(&(update_threshold as i32))
                .set_arg(&(update_rate as i32))
                .set_arg(&(count as i32))
                .set_global_work_size(count)
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(&threshold_buf, CL_TRUE, 0, &mut next_threshold, &[])?;
            self.queue
                .enqueue_read_buffer(&rate_buf, CL_TRUE, 0, &mut next_rate, &[])?;
        }
        threshold_offset[..count].copy_from_slice(&next_threshold);
        rate_ema[..count].copy_from_slice(&next_rate);
        Ok(())
    }

    /// Apply the post-neuron spike/rate homeostatic phase on the device.
    pub fn homeostasis_spikes(
        &self,
        threshold_offset: &mut [f64],
        rate_ema: &mut [f64],
        spikes: &[i8],
        homeostasis_decay: f64,
        target_rate: f64,
        gain: f64,
        update_rate: bool,
    ) -> anyhow::Result<()> {
        let count = threshold_offset.len().min(rate_ema.len()).min(spikes.len());
        if count == 0 {
            return Ok(());
        }
        let mut threshold_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                count * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut rate_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                count * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut spike_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                count * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        let mut next_threshold = vec![0.0; count];
        let mut next_rate = vec![0.0; count];
        unsafe {
            self.queue.enqueue_write_buffer(
                &mut threshold_buf,
                CL_TRUE,
                0,
                &threshold_offset[..count],
                &[],
            )?;
            self.queue
                .enqueue_write_buffer(&mut rate_buf, CL_TRUE, 0, &rate_ema[..count], &[])?;
            self.queue
                .enqueue_write_buffer(&mut spike_buf, CL_TRUE, 0, &spikes[..count], &[])?;
            let kernel = self.kernel_homeostasis_spikes.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&mut threshold_buf)
                .set_arg(&mut rate_buf)
                .set_arg(&spike_buf)
                .set_arg(&homeostasis_decay)
                .set_arg(&target_rate)
                .set_arg(&gain)
                .set_arg(&(update_rate as i32))
                .set_arg(&(count as i32))
                .set_global_work_size(count)
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(&threshold_buf, CL_TRUE, 0, &mut next_threshold, &[])?;
            self.queue
                .enqueue_read_buffer(&rate_buf, CL_TRUE, 0, &mut next_rate, &[])?;
        }
        threshold_offset[..count].copy_from_slice(&next_threshold);
        rate_ema[..count].copy_from_slice(&next_rate);
        Ok(())
    }

    /// Apply one deterministic neuromodulator/resonance EMA transition.
    pub fn neuromodulation_step(
        &self,
        state: &mut [f64; 4],
        targets: [f64; 3],
        decay: f64,
        resonance_decay: f64,
        resonance_target: f64,
    ) -> anyhow::Result<()> {
        let mut state_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                state.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut targets_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                targets.len() * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut next_state = [0.0; 4];
        unsafe {
            self.queue
                .enqueue_write_buffer(&mut state_buf, CL_TRUE, 0, state, &[])?;
            self.queue
                .enqueue_write_buffer(&mut targets_buf, CL_TRUE, 0, &targets, &[])?;
            let kernel = self.kernel_neuromodulation.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&mut state_buf)
                .set_arg(&targets_buf)
                .set_arg(&decay)
                .set_arg(&resonance_decay)
                .set_arg(&resonance_target)
                .set_global_work_size(1)
                .enqueue_nd_range(&self.queue)?;
            self.queue
                .enqueue_read_buffer(&state_buf, CL_TRUE, 0, &mut next_state, &[])?;
        }
        *state = next_state;
        Ok(())
    }

    /// Evaluate growth eligibility in parallel.  The CPU remains responsible
    /// for selecting the canonical first candidate and publishing topology
    /// generations, so this cannot reorder or partially apply growth.
    pub fn growth_candidates(
        &self,
        firing_rate: &[f64],
        since_growth: &[f64],
        saturation_threshold: f64,
        cooldown: f64,
    ) -> anyhow::Result<Vec<i8>> {
        let count = firing_rate.len().min(since_growth.len());
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut rate_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                count * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut since_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_ONLY,
                count * std::mem::size_of::<f64>(),
                ptr::null_mut(),
            )
        }?;
        let mut candidates_buf = unsafe {
            Buffer::create(
                &self.context,
                CL_MEM_READ_WRITE,
                count * std::mem::size_of::<i8>(),
                ptr::null_mut(),
            )
        }?;
        unsafe {
            self.queue.enqueue_write_buffer(
                &mut rate_buf,
                CL_TRUE,
                0,
                &firing_rate[..count],
                &[],
            )?;
            self.queue.enqueue_write_buffer(
                &mut since_buf,
                CL_TRUE,
                0,
                &since_growth[..count],
                &[],
            )?;
            let kernel = self.kernel_growth_candidates.lock().unwrap();
            ExecuteKernel::new(&kernel)
                .set_arg(&rate_buf)
                .set_arg(&since_buf)
                .set_arg(&mut candidates_buf)
                .set_arg(&saturation_threshold)
                .set_arg(&cooldown)
                .set_arg(&(count as i32))
                .set_global_work_size(count)
                .enqueue_nd_range(&self.queue)?;
        }
        let mut candidates = vec![0i8; count];
        unsafe {
            self.queue
                .enqueue_read_buffer(&candidates_buf, CL_TRUE, 0, &mut candidates, &[])?;
        }
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::OpenCLManager;

    /// Hardware CI opts in explicitly; ordinary tests remain deterministic on
    /// hosts without an OpenCL platform.  The production constructor always
    /// runs this gate, so this test exercises the same path when a device is
    /// available.
    #[test]
    fn hardware_reference_gate_is_opt_in() {
        if std::env::var("NM_ENABLE_OPENCL_IN_TESTS").ok().as_deref() != Some("1") {
            return;
        }
        OpenCLManager::new_with_preferred_device_index(0)
            .expect("selected accelerator must pass reference equivalence");
    }
}
