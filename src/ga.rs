//! Genetic Algorithm for optimizing neural network parameters.
//!
//! This module implements a Genetic Algorithm (GA) to automatically find optimal
//! simulation parameters (e.g., connection probabilities, growth thresholds, and
//! morphological rates) for the spiking neural network.
//!
//! ## Fitness Metric
//! The GA optimizes for the formation of "longterm connections". In this context,
//! longterm connections are synapses that have remained stable (non-zero weight)
//! over a significant portion of the simulation, indicating a maturing and
//! stable network architecture.
//!
//! ## Execution Modes
//! - **Local Parallel**: Evaluates individuals using all available CPU cores via `rayon`.
//! - **Distributed Cluster**: Dispatches evaluation tasks to remote nodes in the
//!   cluster via gRPC, allowing for massive parallelization.
//!
//! ## Evolution Process
//! 1. **Initialization**: Create a population of individuals with randomized configurations.
//! 2. **Evaluation**: Run a simulation for each individual and calculate fitness.
//! 3. **Selection**: Sort individuals by fitness and keep the best (elitism).
//! 4. **Crossover**: Combine parameters from successful parents to create offspring.
//! 5. **Mutation**: Apply random changes to offspring parameters to maintain diversity.

use rand::{RngExt, SeedableRng, prelude::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
#[cfg(feature = "sysinfo")]
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[cfg(feature = "sysinfo")]
fn ga_sys_mb_from_raw(raw: u64) -> u64 {
    if raw > 1_000_000_000 {
        raw / 1024 / 1024
    } else {
        raw / 1024
    }
}
#[cfg(feature = "opencl")]
use crate::cl_compute::OpenCLManager;
#[cfg(feature = "opencl")]
use crate::cl_compute::gpu_device_ids_for_indices;
use crate::config::{ClumpingDesign, NetworkConfig, NeuromodSignal, apply_clumping_design};
use crate::monitor::{
    self, MonitorHeuristics, SafetySnapshot, update_sys_cache, update_temp_cache,
};
use crate::runner::Runner;
use crate::sim;
#[cfg(feature = "core_affinity")]
use core_affinity::CoreId;
#[cfg(feature = "opencl")]
use std::cell::RefCell;

/// Represents an individual in the GA population.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Individual {
    pub config: NetworkConfig,
    pub fitness: f64,
    pub last_fitness: f64,
    pub mutation_rate: f64,
    pub crossover_rate: f64,
    pub stagnation: usize,
    #[serde(skip)]
    pub evaluating_node: Option<String>,
}

impl Individual {
    pub fn new(config: NetworkConfig, fitness: f64) -> Self {
        Self {
            config,
            fitness,
            last_fitness: fitness,
            mutation_rate: 0.05,
            crossover_rate: 0.7,
            stagnation: 0,
            evaluating_node: None,
        }
    }
}

#[derive(Clone)]
pub struct GASearch {
    pub population: Vec<Individual>,
    pub leaderboard: Vec<Individual>,
    pub generation: usize,
    pub best_fitness: f64,
    pub best_config: Option<NetworkConfig>,
    pub distributed_node: Option<crate::distributed::DistributedNode>,
    pub current_eval_idx: usize,
    pub force_morphology: bool,
    pub inflight: Vec<usize>,
}

const DEFAULT_GA_RESERVED_CORES: usize = 2;
const DEFAULT_GA_MAX_CONCURRENT_POPULATIONS_SMALL: usize = 1;
const DEFAULT_GA_MAX_CONCURRENT_POPULATIONS_LARGE: usize = 2;
const DEFAULT_GA_TEMP_WARN_C: f32 = 80.0;
const DEFAULT_GA_TEMP_HOT_C: f32 = 90.0;
const DEFAULT_GA_OPENCL_WORKERS: usize = 2;
const DEFAULT_GA_EVAL_CHECK_INTERVAL_STEPS: usize = 50;
const DEFAULT_GA_CPU_WARN_PCT: f32 = 90.0;
const DEFAULT_GA_CPU_HOT_PCT: f32 = 97.0;
const DEFAULT_GA_UI_FRAME_WARN_MS: f32 = 50.0;
const DEFAULT_GA_UI_FRAME_HOT_MS: f32 = 120.0;
const DEFAULT_GA_GEN_MAX_MS: u64 = 120_000;
const DEFAULT_GA_GPU_UTIL_WARN_PCT: f32 = 90.0;
const DEFAULT_GA_GPU_UTIL_HOT_PCT: f32 = 98.0;
const DEFAULT_GA_GPU_VRAM_FREE_MIN_MB: u64 = 512;
const DEFAULT_GA_MEM_FREE_MIN_MB: u64 = 1024;
const DEFAULT_GA_STALL_TIMEOUT_SECS: u64 = 60;
const GA_PROGRESS_TICK_MS: u64 = 500;
const GA_AARNN_MAX_DEPTH: usize = 5;
const GA_IZH_PRESETS: [&str; 8] = ["RS", "FS", "IB", "CH", "LTS", "RZ", "TC", "P"];
const GA_CLUMPING_DESIGNS: [ClumpingDesign; 6] = [
    ClumpingDesign::None,
    ClumpingDesign::HumanBrain,
    ClumpingDesign::FruitFly,
    ClumpingDesign::FruitFlyLarva,
    ClumpingDesign::ZebraFish,
    ClumpingDesign::NematodeWorm,
];
const GA_NEUROMOD_SIGNALS: [NeuromodSignal; 8] = [
    NeuromodSignal::None,
    NeuromodSignal::RewardProxy,
    NeuromodSignal::PerceptualError,
    NeuromodSignal::WorldModelError,
    NeuromodSignal::OutputSpikes,
    NeuromodSignal::SensorySpikes,
    NeuromodSignal::HiddenSpikes,
    NeuromodSignal::Stability,
];

static GA_ACTIVE_POPULATIONS: AtomicUsize = AtomicUsize::new(0);
static GA_POPULATION_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static GA_EVAL_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn pick_izh_preset(rng: &mut StdRng) -> String {
    let idx = rng.random_range(0..GA_IZH_PRESETS.len());
    GA_IZH_PRESETS[idx].to_string()
}

fn pick_clumping_design(rng: &mut StdRng) -> ClumpingDesign {
    let idx = rng.random_range(0..GA_CLUMPING_DESIGNS.len());
    GA_CLUMPING_DESIGNS[idx]
}

fn pick_neuromod_signal(rng: &mut StdRng) -> NeuromodSignal {
    let idx = rng.random_range(0..GA_NEUROMOD_SIGNALS.len());
    GA_NEUROMOD_SIGNALS[idx]
}

fn apply_clumping_design_keep_max_total(cfg: &mut NetworkConfig, design: ClumpingDesign) {
    let max_total = cfg.max_total_neurons;
    apply_clumping_design(cfg, design);
    cfg.max_total_neurons = max_total;
}

fn min_total_neurons(cfg: &NetworkConfig) -> u64 {
    let hidden = cfg
        .num_hidden_layers
        .saturating_mul(cfg.num_hidden_per_layer_initial) as u64;
    let total = cfg.num_sensory_neurons as u64 + cfg.num_output_neurons as u64 + hidden;
    total.max(1)
}

fn sanitize_io_layers(cfg: &mut NetworkConfig) {
    let max_layer = cfg.num_hidden_layers.saturating_sub(1);
    if cfg.num_hidden_layers == 0 {
        cfg.sensory_target_layer = None;
        cfg.output_source_layer = None;
        return;
    }
    if let Some(v) = cfg.sensory_target_layer {
        if v > max_layer {
            cfg.sensory_target_layer = Some(max_layer);
        }
    }
    if let Some(v) = cfg.output_source_layer {
        if v > max_layer {
            cfg.output_source_layer = Some(max_layer);
        }
    }
}
static GA_ACTIVE_EVALS: AtomicUsize = AtomicUsize::new(0);
static GA_TOTAL_EVALUATIONS: AtomicU64 = AtomicU64::new(0);
// GA_TEMP_CACHE moved to monitor.rs
static GA_SEM_WAITERS: AtomicUsize = AtomicUsize::new(0);
static GA_THERMAL_WAITERS: AtomicUsize = AtomicUsize::new(0);
static GA_ABORT_REQUESTED: AtomicBool = AtomicBool::new(false);
static GA_HARD_STOP_ARMED: AtomicBool = AtomicBool::new(false);
static GA_UI_FRAME_MS_X100: AtomicU32 = AtomicU32::new(0);
static GA_THROTTLE_MS: AtomicU32 = AtomicU32::new(0);
static GA_THROTTLE_TICKET: AtomicU32 = AtomicU32::new(0);
static GA_HEURISTICS: OnceLock<Mutex<GAHeuristics>> = OnceLock::new();
static GA_MEM_TRACKER: OnceLock<Mutex<GAMemTracker>> = OnceLock::new();
static GA_ABORT_REASON: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static GA_UI_LAG_START: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static GA_LAST_UI_FRAME_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static GA_PAUSED_MS: AtomicU32 = AtomicU32::new(0);
static GA_STALL_TIMEOUT_SECS: AtomicU32 = AtomicU32::new(DEFAULT_GA_STALL_TIMEOUT_SECS as u32);
static GA_RSS_CRITICAL_STREAK: AtomicU32 = AtomicU32::new(0);
static GA_RSS_CRITICAL_SINCE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static GA_RUN_START: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static GA_MEM_GROWTH_CRITICAL_STREAK: AtomicU32 = AtomicU32::new(0);
static GA_MEM_GROWTH_CRITICAL_SINCE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static GA_WORKER_LIMIT_AUTO: AtomicBool = AtomicBool::new(false);
static GA_MEM_FREE_CRITICAL_STREAK: AtomicU32 = AtomicU32::new(0);
static GA_MEM_FREE_CRITICAL_SINCE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static GA_EVAL_INDIVIDUAL_EMA_MS: AtomicU64 = AtomicU64::new(0);
static GA_CURRENT_POP_SIZE: AtomicUsize = AtomicUsize::new(0);
static GA_COMPLETED_EVALS: AtomicUsize = AtomicUsize::new(0);
static GA_THROTTLED_MS: AtomicU64 = AtomicU64::new(0);
static GA_REMOTE_WAIT_MS: AtomicU64 = AtomicU64::new(0);
static GA_EVAL_MS_OVERRIDE: AtomicU64 = AtomicU64::new(0);
static GA_EVAL_NEURONS_OVERRIDE: AtomicUsize = AtomicUsize::new(0);
static GA_EVAL_CONNS_OVERRIDE: AtomicUsize = AtomicUsize::new(0);
static GA_EVAL_SEGMENTS_OVERRIDE: AtomicUsize = AtomicUsize::new(0);
static GA_WORKER_LIMIT_OVERRIDE: AtomicUsize = AtomicUsize::new(0);
static GA_UI_CLEANUP_REQUESTED: AtomicBool = AtomicBool::new(false);
static GA_EVAL_MEM_WARN: AtomicBool = AtomicBool::new(false);
static GA_RAMP_RUNTIME_ACTIVE: AtomicBool = AtomicBool::new(false);
static GA_RAMP_RUNTIME_POPULATION: AtomicUsize = AtomicUsize::new(0);
static GA_RAMP_RUNTIME_WORKER_CAP: AtomicUsize = AtomicUsize::new(0);
static GA_RAMP_RUNTIME_SIM_TIME_BITS: AtomicU64 = AtomicU64::new(0);
static GA_RAMP_RUNTIME_EVAL_MS: AtomicU64 = AtomicU64::new(0);
static GA_RAMP_RUNTIME_EVAL_NEURONS: AtomicUsize = AtomicUsize::new(0);
static GA_RAMP_RUNTIME_EVAL_CONNS: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "opencl")]
static GA_FORCE_CPU: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "core_affinity")]
static GA_CORE_HISTORY: OnceLock<Mutex<GACoreHistory>> = OnceLock::new();
#[cfg(feature = "core_affinity")]
static GA_LAST_CORE_AFFINITY: OnceLock<Mutex<Option<Vec<usize>>>> = OnceLock::new();
#[cfg(feature = "opencl")]
static GA_CL_DEVICE_INDICES: OnceLock<Vec<usize>> = OnceLock::new();
#[cfg(feature = "opencl")]
static GA_CL_DEVICE_COUNT: OnceLock<usize> = OnceLock::new();
#[cfg(feature = "opencl")]
static GA_OPENCL_FALLBACK_IDX: AtomicUsize = AtomicUsize::new(0);

fn parse_env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
}

fn parse_env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok().and_then(|v| v.parse::<f32>().ok())
}

#[allow(dead_code)]
fn parse_env_bool(name: &str) -> Option<bool> {
    let raw = std::env::var(name).ok()?;
    let val = raw.trim().to_ascii_lowercase();
    match val.as_str() {
        "1" | "true" | "yes" | "y" | "on" => Some(true),
        "0" | "false" | "no" | "n" | "off" => Some(false),
        _ => None,
    }
}

#[allow(dead_code)]
fn parse_env_usize_list(name: &str) -> Option<Vec<usize>> {
    let raw = std::env::var(name).ok()?;
    let mut out = Vec::new();
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = trimmed.parse::<usize>() {
            out.push(v);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn parse_env_string_list(name: &str) -> Vec<String> {
    let raw = match std::env::var(name) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for part in raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("http://{}", trimmed)
        };
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

fn ga_remote_orchestrators() -> Vec<String> {
    parse_env_string_list("NM_GA_REMOTE_ORCHESTRATORS")
}

fn ga_stall_timeout() -> Duration {
    let secs = parse_env_usize("NM_GA_STALL_TIMEOUT_SECS")
        .or_else(|| parse_env_usize("NM_GA_STALL_TIMEOUT_MS").map(|v| v / 1000))
        .unwrap_or(GA_STALL_TIMEOUT_SECS.load(Ordering::Relaxed) as usize)
        .max(5);
    Duration::from_secs(secs as u64)
}

pub fn ga_remote_eval_timeout() -> Duration {
    if let Some(v) = parse_env_usize("NM_GA_REMOTE_EVAL_TIMEOUT_MS") {
        return Duration::from_millis(v.max(1) as u64);
    }
    let base = ga_max_eval_ms().unwrap_or_else(|| ga_gen_max_ms().max(30_000));
    Duration::from_millis(base.saturating_add(10_000))
}

pub fn ga_set_stall_timeout_secs(secs: u64) {
    let clamped = secs.clamp(5, u32::MAX as u64) as u32;
    GA_STALL_TIMEOUT_SECS.store(clamped, Ordering::Relaxed);
}

pub fn ga_set_eval_limits_override(
    max_ms: Option<u64>,
    max_neurons: Option<usize>,
    max_conns: Option<usize>,
) {
    GA_EVAL_MS_OVERRIDE.store(max_ms.unwrap_or(0), Ordering::Relaxed);
    GA_EVAL_NEURONS_OVERRIDE.store(max_neurons.unwrap_or(0), Ordering::Relaxed);
    GA_EVAL_CONNS_OVERRIDE.store(max_conns.unwrap_or(0), Ordering::Relaxed);
}

pub fn ga_clear_eval_limits_override() {
    ga_set_eval_limits_override(None, None, None);
}

pub fn ga_set_worker_limit_override(limit: Option<usize>) {
    GA_WORKER_LIMIT_OVERRIDE.store(limit.unwrap_or(0), Ordering::Relaxed);
    GA_WORKER_LIMIT_AUTO.store(false, Ordering::Relaxed);
}

fn ga_worker_limit_override() -> Option<usize> {
    let v = GA_WORKER_LIMIT_OVERRIDE.load(Ordering::Relaxed);
    if v > 0 { Some(v) } else { None }
}

pub fn ga_total_evaluations() -> u64 {
    GA_TOTAL_EVALUATIONS.load(Ordering::Relaxed)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GAHeuristics {
    version: u32,
    cpu_warn_pct: f32,
    cpu_hot_pct: f32,
    temp_warn_c: f32,
    temp_hot_c: f32,
    mem_free_min_mb: u64,
    #[serde(default = "ga_default_mem_rss_warn_mb")]
    mem_rss_warn_mb: u64,
    #[serde(default = "ga_default_mem_rss_abort_mb")]
    mem_rss_abort_mb: u64,
    #[serde(default = "ga_default_mem_rss_growth_warn_mb")]
    mem_rss_growth_warn_mb: u64,
    #[serde(default = "ga_default_mem_rss_growth_abort_mb")]
    mem_rss_growth_abort_mb: u64,
    ui_frame_warn_ms: f32,
    ui_frame_hot_ms: f32,
    gen_max_ms: u64,
    gpu_util_warn_pct: f32,
    gpu_util_hot_pct: f32,
    gpu_vram_free_min_mb: u64,
    dirty_run: bool,
    dirty_count: u32,
    safe_generations: u32,
    last_event: Option<String>,
}

// GASafetySnapshot replaced by monitor::SafetySnapshot

impl GAHeuristics {
    fn to_monitor(&self) -> MonitorHeuristics {
        MonitorHeuristics {
            temp_warn_c: self.temp_warn_c,
            temp_hot_c: self.temp_hot_c,
            mem_free_min_mb: self.mem_free_min_mb,
            mem_rss_warn_mb: self.mem_rss_warn_mb,
            mem_rss_abort_mb: self.mem_rss_abort_mb,
            mem_rss_growth_warn_mb: self.mem_rss_growth_warn_mb,
            mem_rss_growth_abort_mb: self.mem_rss_growth_abort_mb,
            gpu_util_warn_pct: self.gpu_util_warn_pct,
            gpu_util_hot_pct: self.gpu_util_hot_pct,
            gpu_vram_free_min_mb: self.gpu_vram_free_min_mb,
        }
    }
}

fn ga_heuristics_path() -> PathBuf {
    PathBuf::from("ga_safety_heuristics.json")
}

#[cfg(feature = "core_affinity")]
fn ga_core_history_path() -> PathBuf {
    PathBuf::from("ga_core_affinity.json")
}

fn ga_default_mem_free_min_mb() -> u64 {
    #[cfg(feature = "sysinfo")]
    {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = ga_sys_mb_from_raw(sys.total_memory() as u64);
        let ten_percent = ((total_mb as f32) * 0.1).ceil() as u64;
        return ten_percent.max(DEFAULT_GA_MEM_FREE_MIN_MB);
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        DEFAULT_GA_MEM_FREE_MIN_MB
    }
}

fn ga_default_mem_rss_warn_mb() -> u64 {
    #[cfg(feature = "sysinfo")]
    {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = ga_sys_mb_from_raw(sys.total_memory() as u64);
        let warn_mb = ((total_mb as f32) * 0.60).ceil() as u64;
        return warn_mb.max(8192);
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        16_384
    }
}

fn ga_default_mem_rss_abort_mb() -> u64 {
    #[cfg(feature = "sysinfo")]
    {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = ga_sys_mb_from_raw(sys.total_memory() as u64);
        let abort_mb = ((total_mb as f32) * 0.75).ceil() as u64;
        let warn_mb = ga_default_mem_rss_warn_mb();
        return abort_mb.max(warn_mb + 512).max(12_288);
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        24_576
    }
}

fn ga_default_mem_rss_growth_warn_mb() -> u64 {
    #[cfg(feature = "sysinfo")]
    {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = ga_sys_mb_from_raw(sys.total_memory() as u64);
        let warn_mb = ((total_mb as f32) * 0.2).ceil() as u64;
        return warn_mb.max(2048);
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        4096
    }
}

fn ga_default_mem_rss_growth_abort_mb() -> u64 {
    #[cfg(feature = "sysinfo")]
    {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = ga_sys_mb_from_raw(sys.total_memory() as u64);
        let abort_mb = ((total_mb as f32) * 0.35).ceil() as u64;
        let warn_mb = ga_default_mem_rss_growth_warn_mb();
        return abort_mb.max(warn_mb + 512).max(4096);
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        8192
    }
}

fn ga_sanitize_heuristics(h: &mut GAHeuristics) -> bool {
    let mut changed = false;
    #[cfg(feature = "sysinfo")]
    {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = ga_sys_mb_from_raw(sys.total_memory() as u64);
        if total_mb > 0 {
            let min_free = ((total_mb as f32) * 0.05).ceil() as u64;
            let max_free = ((total_mb as f32) * 0.35).ceil() as u64;
            let clamp_min = min_free.max(256);
            let clamp_max = max_free.max(clamp_min);
            if h.mem_free_min_mb < clamp_min {
                h.mem_free_min_mb = clamp_min;
                changed = true;
            }
            if h.mem_free_min_mb > clamp_max {
                h.mem_free_min_mb = clamp_max;
                changed = true;
            }

            let rss_warn = ((total_mb as f32) * 0.60).ceil() as u64;
            let rss_abort = ((total_mb as f32) * 0.75).ceil() as u64;
            if h.mem_rss_warn_mb > total_mb {
                h.mem_rss_warn_mb = rss_warn.max(2048);
                changed = true;
            }
            if h.mem_rss_abort_mb > total_mb {
                h.mem_rss_abort_mb = rss_abort.max(h.mem_rss_warn_mb + 512);
                changed = true;
            }

            let growth_warn = ((total_mb as f32) * 0.20).ceil() as u64;
            let growth_abort = ((total_mb as f32) * 0.35).ceil() as u64;
            if h.mem_rss_growth_warn_mb > total_mb {
                h.mem_rss_growth_warn_mb = growth_warn.max(512);
                changed = true;
            }
            if h.mem_rss_growth_abort_mb > total_mb {
                h.mem_rss_growth_abort_mb = growth_abort.max(h.mem_rss_growth_warn_mb + 256);
                changed = true;
            }
        }
    }
    changed
}

fn ga_load_heuristics() -> GAHeuristics {
    let path = ga_heuristics_path();
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(mut h) = serde_json::from_str::<GAHeuristics>(&s) {
            if h.version < 3 {
                h.version = 3;
                if h.mem_rss_warn_mb == 0 {
                    h.mem_rss_warn_mb = ga_default_mem_rss_warn_mb();
                }
                if h.mem_rss_abort_mb == 0 {
                    h.mem_rss_abort_mb = ga_default_mem_rss_abort_mb();
                }
                if h.mem_rss_growth_warn_mb == 0 {
                    h.mem_rss_growth_warn_mb = ga_default_mem_rss_growth_warn_mb();
                }
                if h.mem_rss_growth_abort_mb == 0 {
                    h.mem_rss_growth_abort_mb = ga_default_mem_rss_growth_abort_mb();
                }
                let _ = ga_save_heuristics(&h);
            }
            if h.dirty_run {
                h.dirty_run = false;
                h.dirty_count = h.dirty_count.saturating_add(1);
                ga_tighten_heuristics(&mut h, "unclean_shutdown");
                let _ = ga_save_heuristics(&h);
            }
            if ga_sanitize_heuristics(&mut h) {
                let _ = ga_save_heuristics(&h);
            }
            return h;
        }
    }
    let h = GAHeuristics {
        version: 3,
        cpu_warn_pct: DEFAULT_GA_CPU_WARN_PCT,
        cpu_hot_pct: DEFAULT_GA_CPU_HOT_PCT,
        temp_warn_c: ga_temp_warn_c(),
        temp_hot_c: ga_temp_hot_c(),
        mem_free_min_mb: ga_default_mem_free_min_mb(),
        mem_rss_warn_mb: ga_default_mem_rss_warn_mb(),
        mem_rss_abort_mb: ga_default_mem_rss_abort_mb(),
        mem_rss_growth_warn_mb: ga_default_mem_rss_growth_warn_mb(),
        mem_rss_growth_abort_mb: ga_default_mem_rss_growth_abort_mb(),
        ui_frame_warn_ms: DEFAULT_GA_UI_FRAME_WARN_MS,
        ui_frame_hot_ms: DEFAULT_GA_UI_FRAME_HOT_MS,
        gen_max_ms: DEFAULT_GA_GEN_MAX_MS,
        gpu_util_warn_pct: DEFAULT_GA_GPU_UTIL_WARN_PCT,
        gpu_util_hot_pct: DEFAULT_GA_GPU_UTIL_HOT_PCT,
        gpu_vram_free_min_mb: DEFAULT_GA_GPU_VRAM_FREE_MIN_MB,
        dirty_run: false,
        dirty_count: 0,
        safe_generations: 0,
        last_event: None,
    };
    let _ = ga_save_heuristics(&h);
    h
}

fn ga_save_heuristics(h: &GAHeuristics) -> anyhow::Result<()> {
    let s = serde_json::to_string_pretty(h)?;
    std::fs::write(ga_heuristics_path(), s)?;
    Ok(())
}

fn ga_heuristics() -> &'static Mutex<GAHeuristics> {
    GA_HEURISTICS.get_or_init(|| Mutex::new(ga_load_heuristics()))
}

fn ga_tighten_heuristics(h: &mut GAHeuristics, reason: &str) {
    h.cpu_warn_pct = (h.cpu_warn_pct - 2.0).clamp(60.0, 99.0);
    h.cpu_hot_pct = (h.cpu_hot_pct - 2.0).clamp(h.cpu_warn_pct + 1.0, 99.5);
    h.temp_warn_c = (h.temp_warn_c - 2.0).clamp(40.0, 95.0);
    h.temp_hot_c = (h.temp_hot_c - 2.0).clamp(h.temp_warn_c + 2.0, 98.0);
    h.mem_free_min_mb = h.mem_free_min_mb.saturating_add(256);
    h.mem_rss_warn_mb = h.mem_rss_warn_mb.saturating_sub(512).max(4096);
    h.mem_rss_abort_mb = h
        .mem_rss_abort_mb
        .saturating_sub(1024)
        .max(h.mem_rss_warn_mb + 512);
    h.mem_rss_growth_warn_mb = h.mem_rss_growth_warn_mb.saturating_sub(256).max(512);
    h.mem_rss_growth_abort_mb = h
        .mem_rss_growth_abort_mb
        .saturating_sub(512)
        .max(h.mem_rss_growth_warn_mb + 256);
    h.ui_frame_warn_ms = (h.ui_frame_warn_ms - 5.0).clamp(10.0, 250.0);
    h.ui_frame_hot_ms = (h.ui_frame_hot_ms - 10.0).clamp(h.ui_frame_warn_ms + 10.0, 1000.0);
    h.gen_max_ms = h.gen_max_ms.saturating_sub(10_000).max(30_000);
    h.gpu_util_warn_pct = (h.gpu_util_warn_pct - 2.0).clamp(50.0, 99.0);
    h.gpu_util_hot_pct = (h.gpu_util_hot_pct - 2.0).clamp(h.gpu_util_warn_pct + 1.0, 99.5);
    h.gpu_vram_free_min_mb = h.gpu_vram_free_min_mb.saturating_add(256);
    h.safe_generations = 0;
    h.last_event = Some(reason.to_string());
}

fn ga_relax_heuristics(h: &mut GAHeuristics) {
    h.cpu_warn_pct = (h.cpu_warn_pct + 1.0).clamp(60.0, 99.0);
    h.cpu_hot_pct = (h.cpu_hot_pct + 1.0).clamp(h.cpu_warn_pct + 1.0, 99.5);
    h.temp_warn_c = (h.temp_warn_c + 1.0).clamp(40.0, 95.0);
    h.temp_hot_c = (h.temp_hot_c + 1.0).clamp(h.temp_warn_c + 2.0, 98.0);
    h.mem_free_min_mb = h.mem_free_min_mb.saturating_sub(128).max(256);
    h.mem_rss_warn_mb = (h.mem_rss_warn_mb + 256).min(65_536);
    h.mem_rss_abort_mb = (h.mem_rss_abort_mb + 512)
        .min(98_304)
        .max(h.mem_rss_warn_mb + 512);
    h.mem_rss_growth_warn_mb = (h.mem_rss_growth_warn_mb + 128).min(32_768);
    h.mem_rss_growth_abort_mb = (h.mem_rss_growth_abort_mb + 256)
        .min(49_152)
        .max(h.mem_rss_growth_warn_mb + 256);
    h.ui_frame_warn_ms = (h.ui_frame_warn_ms + 2.0).clamp(10.0, 250.0);
    h.ui_frame_hot_ms = (h.ui_frame_hot_ms + 4.0).clamp(h.ui_frame_warn_ms + 10.0, 1000.0);
    h.gen_max_ms = (h.gen_max_ms + 5_000).min(300_000);
    h.gpu_util_warn_pct = (h.gpu_util_warn_pct + 1.0).clamp(50.0, 99.0);
    h.gpu_util_hot_pct = (h.gpu_util_hot_pct + 1.0).clamp(h.gpu_util_warn_pct + 1.0, 99.5);
    h.gpu_vram_free_min_mb = h.gpu_vram_free_min_mb.saturating_sub(128).max(128);
}

fn ga_record_mem_rss(rss_mb: Option<u64>) -> (Option<u64>, Option<u64>) {
    let Some(rss_mb) = rss_mb else {
        return (None, None);
    };
    let tracker = GA_MEM_TRACKER.get_or_init(|| {
        Mutex::new(GAMemTracker {
            baseline_rss_mb: None,
            last_rss_mb: None,
            max_rss_mb: None,
        })
    });
    let mut guard = tracker.lock().expect("GA memory tracker poisoned");
    if let Some(base) = guard.baseline_rss_mb {
        let low = base / 4;
        let high = base.saturating_mul(4);
        if rss_mb < low || rss_mb > high {
            guard.baseline_rss_mb = Some(rss_mb);
            guard.last_rss_mb = Some(rss_mb);
            guard.max_rss_mb = Some(rss_mb);
        }
    }
    if guard.baseline_rss_mb.is_none() {
        guard.baseline_rss_mb = Some(rss_mb);
    }
    guard.last_rss_mb = Some(rss_mb);
    guard.max_rss_mb = Some(guard.max_rss_mb.map_or(rss_mb, |prev| prev.max(rss_mb)));
    let growth = guard
        .baseline_rss_mb
        .map(|base| rss_mb.saturating_sub(base));
    (guard.baseline_rss_mb, growth)
}

fn ga_mem_baseline_mb() -> Option<u64> {
    let tracker = GA_MEM_TRACKER.get_or_init(|| {
        Mutex::new(GAMemTracker {
            baseline_rss_mb: None,
            last_rss_mb: None,
            max_rss_mb: None,
        })
    });
    let guard = tracker.lock().expect("GA memory tracker poisoned");
    guard.baseline_rss_mb
}

fn ga_reset_mem_tracker() {
    let tracker = GA_MEM_TRACKER.get_or_init(|| {
        Mutex::new(GAMemTracker {
            baseline_rss_mb: None,
            last_rss_mb: None,
            max_rss_mb: None,
        })
    });
    let mut guard = tracker.lock().expect("GA memory tracker poisoned");
    guard.baseline_rss_mb = None;
    guard.last_rss_mb = None;
    guard.max_rss_mb = None;
    #[cfg(feature = "sysinfo")]
    {
        if let Ok(pid) = sysinfo::get_current_pid() {
            let mut sys = System::new();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_memory(),
            );
            if let Some(proc) = sys.process(pid) {
                let rss_mb = (proc.memory() / 1024) as u64;
                guard.baseline_rss_mb = Some(rss_mb);
                guard.last_rss_mb = Some(rss_mb);
                guard.max_rss_mb = Some(rss_mb);
            }
        }
    }
    ga_clear_rss_critical();
    ga_clear_mem_growth_critical();
    ga_clear_mem_backoff();
    ga_clear_mem_free_critical();
}

fn ga_clear_mem_tracker() {
    let tracker = GA_MEM_TRACKER.get_or_init(|| {
        Mutex::new(GAMemTracker {
            baseline_rss_mb: None,
            last_rss_mb: None,
            max_rss_mb: None,
        })
    });
    let mut guard = tracker.lock().expect("GA memory tracker poisoned");
    guard.baseline_rss_mb = None;
    guard.last_rss_mb = None;
    guard.max_rss_mb = None;
    ga_clear_rss_critical();
    ga_clear_mem_growth_critical();
    ga_clear_mem_backoff();
    ga_clear_mem_free_critical();
}

pub fn ga_mark_dirty() {
    let mut h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
    if !h.dirty_run {
        h.dirty_run = true;
        let _ = ga_save_heuristics(&h);
    }
    ga_reset_mem_tracker();
    ga_clear_ramp_runtime_status();
    let slot = GA_RUN_START.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA run start lock poisoned");
    *guard = Some(Instant::now());
}

pub fn ga_mark_clean() {
    let mut h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
    if h.dirty_run {
        h.dirty_run = false;
        let _ = ga_save_heuristics(&h);
    }
    #[cfg(feature = "core_affinity")]
    {
        let history = ga_core_history()
            .lock()
            .expect("GA core history lock poisoned");
        ga_save_core_history(&history);
    }
    ga_clear_mem_tracker();
    ga_clear_ramp_runtime_status();
    let slot = GA_RUN_START.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA run start lock poisoned");
    *guard = None;
}

#[allow(dead_code)]
pub fn ga_update_ui_frame_ms(ms: f32) {
    let clamped = ms.clamp(0.0, 5000.0);
    let scaled = (clamped * 100.0) as u32;
    GA_UI_FRAME_MS_X100.store(scaled, Ordering::Relaxed);
    let slot = GA_LAST_UI_FRAME_AT.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA ui frame time lock poisoned");
    *guard = Some(Instant::now());
}

thread_local! {
    static LAST_THROTTLE_TICKET: std::cell::Cell<u32> = std::cell::Cell::new(0);
}

fn ga_request_throttle(ms: u64) {
    GA_THROTTLE_MS.store(ms.min(u32::MAX as u64) as u32, Ordering::Relaxed);
    GA_THROTTLE_TICKET.fetch_add(1, Ordering::SeqCst);
}

pub fn ga_request_ui_cleanup() {
    GA_UI_CLEANUP_REQUESTED.store(true, Ordering::SeqCst);
}

#[allow(dead_code)]
pub fn ga_take_ui_cleanup_request() -> bool {
    GA_UI_CLEANUP_REQUESTED.swap(false, Ordering::SeqCst)
}

fn ga_log_abort_snapshot(reason: &str) {
    let snapshot = ga_safety_snapshot();
    let pop_size = GA_CURRENT_POP_SIZE.load(Ordering::Relaxed);
    let active_evals = GA_ACTIVE_EVALS.load(Ordering::Relaxed);
    let throttled_ms = GA_THROTTLED_MS.load(Ordering::Relaxed);
    nm_err!(
        "[warn] GA abort snapshot: reason {} pop {} active_evals {} rss {:?}MB free {:?}MB growth {:?}MB throttled_ms {}.",
        reason,
        pop_size,
        active_evals,
        snapshot.proc_rss_mb,
        snapshot.mem_free_mb,
        snapshot.proc_rss_growth_mb,
        throttled_ms
    );
}

fn ga_arm_hard_stop(reason: &str) {
    if GA_HARD_STOP_ARMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (delay_secs, severity) = ga_hard_stop_delay_secs(reason, &snapshot, &h);
    nm_err!(
        "[warn] GA hard stop armed due to {} (delay {}s, severity {:.2}).",
        reason,
        delay_secs,
        severity
    );
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(delay_secs));
        crate::obs::flush_log();
        std::process::exit(1);
    });
}

fn ga_hard_stop_delay_secs(
    reason: &str,
    snapshot: &SafetySnapshot,
    h: &GAHeuristics,
) -> (u64, f32) {
    if let Some(v) = parse_env_usize("NM_GA_HARD_STOP_SECS") {
        return (v.max(1) as u64, 0.0);
    }
    let mut delay = 6;
    let mut severity = 0.0_f32;
    if reason.contains("mem_critical") || reason.contains("mem_growth") {
        delay = delay.min(3);
        severity = severity.max(0.5);
    }
    if let Some(total_mb) = snapshot.total_mem_mb {
        let (free_warn, free_abort) = ga_mem_free_limits(snapshot, h);
        let (rss_warn, rss_abort) = ga_effective_rss_limits(snapshot, h);
        if let Some(free_mb) = snapshot.mem_free_mb {
            if free_mb < free_abort {
                let deficit_ratio = (free_abort - free_mb) as f32 / total_mb as f32;
                severity = severity.max(deficit_ratio);
                if deficit_ratio >= 0.10 {
                    delay = delay.min(1);
                } else if deficit_ratio >= 0.05 {
                    delay = delay.min(2);
                } else {
                    delay = delay.min(3);
                }
            } else if free_mb < free_warn {
                delay = delay.min(4);
                severity = severity.max(0.25);
            }
        }
        if let Some(rss_mb) = snapshot.proc_rss_mb {
            if rss_mb >= rss_abort {
                let excess_ratio = (rss_mb - rss_abort) as f32 / total_mb as f32;
                severity = severity.max(excess_ratio);
                if excess_ratio >= 0.10 {
                    delay = delay.min(1);
                } else if excess_ratio >= 0.05 {
                    delay = delay.min(2);
                } else {
                    delay = delay.min(3);
                }
            } else if rss_mb >= rss_warn {
                delay = delay.min(4);
                severity = severity.max(0.25);
            }
        }
    }
    if let Some(growth_mb) = snapshot.proc_rss_growth_mb {
        if growth_mb >= h.mem_rss_growth_abort_mb {
            let ratio = growth_mb as f32 / h.mem_rss_growth_abort_mb.max(1) as f32;
            severity = severity.max((ratio - 1.0).max(0.0));
            if ratio >= 1.5 {
                delay = delay.min(1);
            } else if ratio >= 1.2 {
                delay = delay.min(2);
            } else {
                delay = delay.min(3);
            }
        } else if growth_mb >= h.mem_rss_growth_warn_mb {
            delay = delay.min(4);
            severity = severity.max(0.25);
        }
    }
    if h.last_event.as_deref().map_or(false, |e| {
        e.contains("mem_critical") || e.contains("mem_growth")
    }) {
        delay = delay.min(2);
        severity = severity.max(0.5);
    }
    if h.dirty_count >= 2 {
        delay = delay.min(3);
        severity = severity.max(0.35);
    }
    let active_evals = GA_ACTIVE_EVALS.load(Ordering::Relaxed);
    if active_evals > 0 {
        let ui_recent = GA_LAST_UI_FRAME_AT
            .get_or_init(|| Mutex::new(None))
            .lock()
            .ok()
            .and_then(|g| g.map(|t| t.elapsed() < Duration::from_secs(2)))
            .unwrap_or(false);
        if ui_recent {
            delay = delay.max(5);
        } else {
            delay = delay.max(4);
        }
        severity = severity.max(0.4);
    }
    (delay.max(1), severity)
}

fn ga_request_abort(reason: &str) {
    let slot = GA_ABORT_REASON.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA abort reason lock poisoned");
    *guard = Some(reason.to_string());
    GA_ABORT_REQUESTED.store(true, Ordering::SeqCst);
    ga_log_abort_snapshot(reason);
    if reason.contains("mem_critical")
        || reason.contains("mem_growth")
        || reason.contains("mem_eval_guard_abort")
    {
        ga_request_ui_cleanup();
    }
    if reason == "mem_critical" || reason == "mem_eval_guard_abort" {
        ga_arm_hard_stop(reason);
    }
}

fn ga_adjust_eval_limits_on_pressure(snapshot: &SafetySnapshot) {
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (free_warn_mb, _) = ga_mem_free_limits(snapshot, &h);
    let (rss_warn_mb, _) = ga_effective_rss_limits(snapshot, &h);
    let pressure = snapshot.mem_free_mb.map_or(false, |m| m < free_warn_mb)
        || snapshot.proc_rss_mb.map_or(false, |r| r >= rss_warn_mb)
        || snapshot
            .proc_rss_growth_mb
            .map_or(false, |g| g >= h.mem_rss_growth_warn_mb);
    if !pressure {
        return;
    }
    let total_mb = snapshot.total_mem_mb.unwrap_or(0);
    let neuron_cap = if total_mb > 0 {
        ((total_mb as f32) * 12.0).round() as usize
    } else {
        750_000
    };
    let conns_cap = if total_mb > 0 {
        ((total_mb as f32) * 60.0).round() as usize
    } else {
        6_000_000
    };
    let segs_cap = if total_mb > 0 {
        ((total_mb as f32) * 2.0).round() as usize
    } else {
        200_000
    };
    let cur_neurons = GA_EVAL_NEURONS_OVERRIDE.load(Ordering::Relaxed);
    let cur_conns = GA_EVAL_CONNS_OVERRIDE.load(Ordering::Relaxed);
    let cur_segs = GA_EVAL_SEGMENTS_OVERRIDE.load(Ordering::Relaxed);
    let next_neurons = if cur_neurons == 0 {
        neuron_cap
    } else {
        cur_neurons.min(neuron_cap)
    };
    let next_conns = if cur_conns == 0 {
        conns_cap
    } else {
        cur_conns.min(conns_cap)
    };
    let next_segs = if cur_segs == 0 {
        segs_cap
    } else {
        cur_segs.min(segs_cap)
    };
    if next_neurons != cur_neurons || next_conns != cur_conns || next_segs != cur_segs {
        GA_EVAL_NEURONS_OVERRIDE.store(next_neurons, Ordering::Relaxed);
        GA_EVAL_CONNS_OVERRIDE.store(next_conns, Ordering::Relaxed);
        GA_EVAL_SEGMENTS_OVERRIDE.store(next_segs, Ordering::Relaxed);
        ga_set_worker_limit_auto(1);
        nm_log!(
            "[info] GA eval limits tightened due to pressure: neurons <= {} conns <= {} segments <= {} (workers=1).",
            next_neurons,
            next_conns,
            next_segs
        );
    }
}

fn ga_clear_ui_lag_start() {
    let slot = GA_UI_LAG_START.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA UI lag lock poisoned");
    *guard = None;
}

#[allow(dead_code)]
pub fn ga_reset_abort_reason() {
    ga_clear_abort_reason();
}

/// Request a cooperative GA stop from external controllers (e.g. UI shutdown).
/// This sets the global abort flag so in-flight evaluations can exit quickly.
#[allow(dead_code)]
pub fn ga_request_stop(reason: &str) {
    let r = if reason.trim().is_empty() {
        "stop_requested"
    } else {
        reason
    };
    ga_request_abort(r);
}

fn ga_clear_abort_reason() {
    let slot = GA_ABORT_REASON.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA abort reason lock poisoned");
    *guard = None;
}

pub fn ga_abort_reason() -> Option<String> {
    let slot = GA_ABORT_REASON.get_or_init(|| Mutex::new(None));
    let guard = slot.lock().expect("GA abort reason lock poisoned");
    guard.clone()
}

#[allow(dead_code)]
pub fn ga_backend_label() -> String {
    #[cfg(feature = "opencl")]
    {
        if ga_should_use_opencl() {
            if ga_auto_opencl() && parse_env_bool("NM_GA_USE_OPENCL").is_none() {
                return "OpenCL (auto)".to_string();
            }
            return "OpenCL".to_string();
        }
        "CPU".to_string()
    }
    #[cfg(not(feature = "opencl"))]
    {
        "CPU".to_string()
    }
}

#[allow(dead_code)]
pub fn ga_affinity_label() -> Option<String> {
    #[cfg(feature = "core_affinity")]
    {
        let slot = GA_LAST_CORE_AFFINITY.get_or_init(|| Mutex::new(None));
        let guard = slot.lock().expect("GA core affinity lock poisoned");
        guard.as_ref().map(|ids| {
            let mut ids = ids.clone();
            ids.sort_unstable();
            format!(
                "cores {}",
                ids.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
    }
    #[cfg(not(feature = "core_affinity"))]
    {
        None
    }
}

fn ga_effective_rss_limits(snapshot: &SafetySnapshot, h: &GAHeuristics) -> (u64, u64) {
    let mut warn = h.mem_rss_warn_mb.max(1024);
    let mut abort = h.mem_rss_abort_mb.max(warn + 512);
    if let Some(baseline) = ga_mem_baseline_mb() {
        warn = warn.max(baseline + 512);
        abort = abort.max(warn + 512);
    }
    if let Some(total_mb) = snapshot.total_mem_mb {
        let auto_warn = ((total_mb as f32) * 0.50).ceil() as u64;
        let auto_abort = ((total_mb as f32) * 0.65).ceil() as u64;
        warn = warn.max(auto_warn.max(2048));
        abort = abort.max(auto_abort.max(warn + 512));
        let max_cap = total_mb.saturating_sub(256);
        if abort > max_cap {
            abort = max_cap.max(warn + 512);
        }
        if warn >= abort {
            warn = abort.saturating_sub(256);
        }
    }
    (warn, abort)
}

fn ga_note_rss_critical() -> (u32, Duration) {
    let streak = GA_RSS_CRITICAL_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
    let slot = GA_RSS_CRITICAL_SINCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA RSS critical lock poisoned");
    if guard.is_none() {
        *guard = Some(Instant::now());
    }
    let elapsed = guard.unwrap().elapsed();
    (streak, elapsed)
}

fn ga_clear_rss_critical() {
    GA_RSS_CRITICAL_STREAK.store(0, Ordering::Relaxed);
    let slot = GA_RSS_CRITICAL_SINCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA RSS critical lock poisoned");
    *guard = None;
}

fn ga_note_mem_growth_critical() -> (u32, Duration) {
    let streak = GA_MEM_GROWTH_CRITICAL_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
    let slot = GA_MEM_GROWTH_CRITICAL_SINCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA mem growth critical lock poisoned");
    if guard.is_none() {
        *guard = Some(Instant::now());
    }
    let elapsed = guard.unwrap().elapsed();
    (streak, elapsed)
}

fn ga_clear_mem_growth_critical() {
    GA_MEM_GROWTH_CRITICAL_STREAK.store(0, Ordering::Relaxed);
    let slot = GA_MEM_GROWTH_CRITICAL_SINCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA mem growth critical lock poisoned");
    *guard = None;
}

fn ga_note_mem_free_critical() -> (u32, Duration) {
    let streak = GA_MEM_FREE_CRITICAL_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
    let slot = GA_MEM_FREE_CRITICAL_SINCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA mem free critical lock poisoned");
    if guard.is_none() {
        *guard = Some(Instant::now());
    }
    let elapsed = guard.unwrap().elapsed();
    (streak, elapsed)
}

fn ga_clear_mem_free_critical() {
    GA_MEM_FREE_CRITICAL_STREAK.store(0, Ordering::Relaxed);
    let slot = GA_MEM_FREE_CRITICAL_SINCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA mem free critical lock poisoned");
    *guard = None;
}

fn ga_apply_mem_backoff() {
    ga_set_worker_limit_auto(1);
    ga_request_throttle(3000);
}

fn ga_set_worker_limit_auto(limit: usize) {
    GA_WORKER_LIMIT_OVERRIDE.store(limit.max(1), Ordering::Relaxed);
    GA_WORKER_LIMIT_AUTO.store(true, Ordering::Relaxed);
}

fn ga_try_clear_auto_worker_limit(snapshot: &SafetySnapshot, h: &GAHeuristics) {
    if !GA_WORKER_LIMIT_AUTO.load(Ordering::Relaxed) {
        return;
    }
    let (rss_warn_mb, _) = ga_effective_rss_limits(snapshot, h);
    let (free_warn_mb, _) = ga_mem_free_limits(snapshot, h);
    let cpu_ok = snapshot
        .cpu_usage_pct
        .map_or(true, |cpu| cpu < (h.cpu_warn_pct * 0.9));
    let temp_ok = snapshot
        .temp_c
        .or_else(update_temp_cache)
        .map_or(true, |temp| temp < (h.temp_warn_c - 1.0));
    let free_ok = snapshot
        .mem_free_mb
        .map_or(true, |free_mb| free_mb >= free_warn_mb);
    let rss_ok = snapshot
        .proc_rss_mb
        .map_or(true, |rss_mb| rss_mb < rss_warn_mb);
    let growth_ok = snapshot
        .proc_rss_growth_mb
        .map_or(true, |growth_mb| growth_mb < h.mem_rss_growth_warn_mb);
    let ui_ok = snapshot
        .ui_frame_ms
        .map_or(true, |ui_ms| ui_ms < h.ui_frame_warn_ms);
    if cpu_ok && temp_ok && free_ok && rss_ok && growth_ok && ui_ok {
        GA_WORKER_LIMIT_OVERRIDE.store(0, Ordering::Relaxed);
        GA_WORKER_LIMIT_AUTO.store(false, Ordering::Relaxed);
        nm_log!("[info] GA worker cap recovered; restoring adaptive parallelism.");
    }
}

fn ga_clear_mem_backoff() {
    if GA_WORKER_LIMIT_AUTO.load(Ordering::Relaxed) {
        GA_WORKER_LIMIT_OVERRIDE.store(0, Ordering::Relaxed);
        GA_WORKER_LIMIT_AUTO.store(false, Ordering::Relaxed);
    }
}

fn ga_mem_tracker_snapshot() -> (Option<u64>, Option<u64>, Option<u64>) {
    let tracker = GA_MEM_TRACKER.get_or_init(|| {
        Mutex::new(GAMemTracker {
            baseline_rss_mb: None,
            last_rss_mb: None,
            max_rss_mb: None,
        })
    });
    let guard = tracker.lock().expect("GA memory tracker poisoned");
    (guard.baseline_rss_mb, guard.last_rss_mb, guard.max_rss_mb)
}

fn ga_mem_free_limits(snapshot: &SafetySnapshot, h: &GAHeuristics) -> (u64, u64) {
    let mut warn = h.mem_free_min_mb.max(256);
    let mut abort = (warn / 2).max(128);
    if let Some(total_mb) = snapshot.total_mem_mb {
        let auto_warn = ((total_mb as f32) * 0.30).ceil() as u64;
        let auto_abort = ((total_mb as f32) * 0.18).ceil() as u64;
        let max_warn = ((total_mb as f32) * 0.40).ceil() as u64;
        let max_abort = ((total_mb as f32) * 0.25).ceil() as u64;
        warn = warn
            .max(auto_warn.max(512))
            .min(max_warn.max(auto_warn.max(512)));
        abort = abort
            .max(auto_abort.max(256))
            .min(max_abort.max(auto_abort.max(256)));
        let max_cap = total_mb.saturating_sub(256);
        if warn > max_cap {
            warn = max_cap.max(256);
        }
        if abort >= warn {
            abort = warn.saturating_sub(128).max(128);
        }
    }
    (warn, abort)
}

fn ga_should_split_batches() -> bool {
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (_, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
    let (_, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
    if snapshot.mem_free_mb.map_or(false, |m| m < free_abort_mb) {
        return true;
    }
    if snapshot.proc_rss_mb.map_or(false, |r| r >= rss_abort_mb) {
        return true;
    }
    if snapshot
        .proc_rss_growth_mb
        .map_or(false, |g| g >= h.mem_rss_growth_abort_mb)
    {
        return true;
    }
    false
}

fn ga_min_parallel_evals() -> usize {
    parse_env_usize("NM_GA_MIN_PARALLEL_EVALS")
        .unwrap_or(2)
        .max(1)
}

fn ga_resources_healthy_for_parallelism() -> bool {
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (rss_warn_mb, _) = ga_effective_rss_limits(&snapshot, &h);
    let (free_warn_mb, _) = ga_mem_free_limits(&snapshot, &h);
    let cpu_ok = snapshot
        .cpu_usage_pct
        .map_or(true, |cpu| cpu < h.cpu_warn_pct);
    let temp_ok = snapshot
        .temp_c
        .or_else(update_temp_cache)
        .map_or(true, |temp| temp < h.temp_warn_c);
    let free_ok = snapshot
        .mem_free_mb
        .map_or(true, |free_mb| free_mb >= free_warn_mb);
    let rss_ok = snapshot
        .proc_rss_mb
        .map_or(true, |rss_mb| rss_mb < rss_warn_mb);
    let growth_ok = snapshot
        .proc_rss_growth_mb
        .map_or(true, |growth_mb| growth_mb < h.mem_rss_growth_warn_mb);
    cpu_ok && temp_ok && free_ok && rss_ok && growth_ok
}

fn ga_morph_growth_parallel_threads(pop_size: usize) -> usize {
    let budget = population_worker_threads(pop_size).max(1);
    let base_budget = population_worker_threads_base(pop_size).max(1);
    let min_parallel = ga_min_parallel_evals().min(pop_size.max(1)).max(1);
    if budget <= 1 {
        if base_budget >= min_parallel && ga_resources_healthy_for_parallelism() {
            return min_parallel;
        }
        return 1;
    }

    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (rss_warn_mb, _) = ga_effective_rss_limits(&snapshot, &h);
    let (free_warn_mb, _) = ga_mem_free_limits(&snapshot, &h);

    let cpu_ok = snapshot
        .cpu_usage_pct
        .map_or(true, |cpu| cpu < h.cpu_warn_pct);
    let temp_ok = snapshot
        .temp_c
        .or_else(update_temp_cache)
        .map_or(true, |temp| temp < h.temp_warn_c);
    let free_ok = snapshot
        .mem_free_mb
        .map_or(true, |free_mb| free_mb >= free_warn_mb);
    let rss_ok = snapshot
        .proc_rss_mb
        .map_or(true, |rss_mb| rss_mb < rss_warn_mb);
    let growth_ok = snapshot
        .proc_rss_growth_mb
        .map_or(true, |growth_mb| growth_mb < h.mem_rss_growth_warn_mb);
    let ui_ok = ga_ui_frame_ms().map_or(true, |ui_ms| ui_ms < h.ui_frame_warn_ms);
    let sem_ok = GA_SEM_WAITERS.load(Ordering::SeqCst) == 0;
    let thermal_ok = GA_THERMAL_WAITERS.load(Ordering::SeqCst) == 0;

    if cpu_ok && temp_ok && free_ok && rss_ok && growth_ok && ui_ok && sem_ok && thermal_ok {
        return budget;
    }
    // Degrade smoothly instead of collapsing to one worker on minor pressure.
    if free_ok && rss_ok && growth_ok && temp_ok {
        return (budget / 2).max(2).min(pop_size.max(1));
    }
    1
}

fn ga_record_individual_timing(pop_size: usize, elapsed: Duration) {
    if pop_size == 0 {
        return;
    }
    let per_ms = (elapsed.as_millis() / pop_size as u128) as u64;
    if per_ms == 0 {
        return;
    }
    let prev = GA_EVAL_INDIVIDUAL_EMA_MS.load(Ordering::Relaxed);
    let ema = if prev == 0 {
        per_ms
    } else {
        (prev.saturating_mul(8) + per_ms.saturating_mul(2)) / 10
    };
    GA_EVAL_INDIVIDUAL_EMA_MS.store(ema, Ordering::Relaxed);
}

fn ga_effective_parallelism(pop_size: usize) -> usize {
    let worker_budget = population_worker_threads(pop_size).max(1);
    let active_evals = GA_ACTIVE_EVALS.load(Ordering::Relaxed).max(1);
    worker_budget.min(active_evals).max(1)
}

fn ga_dynamic_gen_timeout_ms(pop_size: usize) -> u64 {
    let base = ga_gen_max_ms().max(30_000);
    let ema_ms = GA_EVAL_INDIVIDUAL_EMA_MS.load(Ordering::Relaxed);
    if ema_ms == 0 || pop_size == 0 {
        return base;
    }
    let workers = ga_effective_parallelism(pop_size);
    let batches = (pop_size + workers - 1) / workers;
    let expected = ema_ms.saturating_mul(batches as u64);
    let padded = expected.saturating_mul(4); // 4x headroom to tolerate temporary slowdowns
    padded.max(base)
}

fn ga_rss_grace_active() -> bool {
    let slot = GA_RUN_START.get_or_init(|| Mutex::new(None));
    let guard = slot.lock().expect("GA run start lock poisoned");
    guard
        .map(|t| t.elapsed() < Duration::from_secs(20))
        .unwrap_or(false)
}

fn ga_should_abort_on_rss(
    rss_mb: u64,
    rss_warn_mb: u64,
    rss_abort_mb: u64,
    total_mb: Option<u64>,
    context: &str,
) -> bool {
    if rss_mb < rss_abort_mb {
        ga_clear_rss_critical();
        return false;
    }
    if ga_rss_grace_active() {
        ga_request_throttle(2000);
        nm_log!(
            "[info] GA pacing: RSS {}MB above critical threshold during {} (startup grace).",
            rss_mb,
            context
        );
        return false;
    }
    let (streak, elapsed) = ga_note_rss_critical();
    ga_request_throttle(2000);
    nm_log!(
        "[info] GA pacing: RSS {}MB above critical threshold during {} (warn {}MB, abort {}MB, total {:?}MB, {} checks, {:.1}s).",
        rss_mb,
        context,
        rss_warn_mb,
        rss_abort_mb,
        total_mb,
        streak,
        elapsed.as_secs_f32()
    );
    if streak >= 6 || elapsed >= Duration::from_secs(30) {
        nm_err!(
            "[warn] GA abort: RSS {}MB stayed above critical threshold for {:.1}s ({} checks, warn {}MB, abort {}MB, total {:?}MB). Possible leak.",
            rss_mb,
            elapsed.as_secs_f32(),
            streak,
            rss_warn_mb,
            rss_abort_mb,
            total_mb
        );
        ga_request_abort("mem_rss_abort");
        return true;
    }
    false
}

fn ga_should_abort_on_mem_growth(
    growth_mb: u64,
    warn_mb: u64,
    abort_mb: u64,
    context: &str,
) -> bool {
    if growth_mb < warn_mb {
        ga_clear_mem_growth_critical();
        ga_clear_mem_backoff();
        return false;
    }
    if growth_mb >= abort_mb {
        let (streak, elapsed) = ga_note_mem_growth_critical();
        ga_apply_mem_backoff();
        nm_log!(
            "[info] GA pacing: RSS growth {}MB above critical threshold during {} (warn {}MB, abort {}MB, {} checks, {:.1}s).",
            growth_mb,
            context,
            warn_mb,
            abort_mb,
            streak,
            elapsed.as_secs_f32()
        );
        if streak >= 6 || elapsed >= Duration::from_secs(30) {
            nm_err!(
                "[warn] GA abort: RSS growth {}MB stayed above critical threshold for {:.1}s ({} checks, warn {}MB, abort {}MB). Possible leak.",
                growth_mb,
                elapsed.as_secs_f32(),
                streak,
                warn_mb,
                abort_mb
            );
            ga_request_abort("mem_growth_abort");
            return true;
        }
        return false;
    }
    nm_log!(
        "[info] GA pacing: RSS growth {}MB exceeds warning during {}.",
        growth_mb,
        context
    );
    ga_apply_mem_backoff();
    false
}

fn ga_should_abort_on_mem_free(
    free_mb: u64,
    warn_mb: u64,
    abort_mb: u64,
    total_mb: Option<u64>,
    context: &str,
) -> bool {
    if free_mb >= warn_mb {
        ga_clear_mem_free_critical();
        ga_clear_mem_backoff();
        return false;
    }
    ga_apply_mem_backoff();
    if free_mb < abort_mb {
        if GA_ACTIVE_EVALS.load(Ordering::Relaxed) > 0 {
            nm_err!(
                "[warn] GA abort: free memory {}MB below abort threshold during {} (warn {}MB, abort {}MB, total {:?}MB).",
                free_mb,
                context,
                warn_mb,
                abort_mb,
                total_mb
            );
            GA_EVAL_MEM_WARN.store(true, Ordering::Relaxed);
            ga_request_abort("mem_critical");
            return true;
        }
        let (streak, elapsed) = ga_note_mem_free_critical();
        nm_log!(
            "[info] GA pacing: free memory {}MB below critical threshold during {} (warn {}MB, abort {}MB, total {:?}MB, {} checks, {:.1}s).",
            free_mb,
            context,
            warn_mb,
            abort_mb,
            total_mb,
            streak,
            elapsed.as_secs_f32()
        );
        if streak >= 6 || elapsed >= Duration::from_secs(30) {
            nm_err!(
                "[warn] GA abort: free memory {}MB stayed below critical threshold for {:.1}s ({} checks, warn {}MB, abort {}MB, total {:?}MB). Possible leak.",
                free_mb,
                elapsed.as_secs_f32(),
                streak,
                warn_mb,
                abort_mb,
                total_mb
            );
            ga_request_abort("mem_critical");
            return true;
        }
        return false;
    }
    nm_log!(
        "[info] GA pacing: free memory {}MB below warning during {} (warn {}MB, total {:?}MB).",
        free_mb,
        context,
        warn_mb,
        total_mb
    );
    false
}

fn ga_check_mem_pressure(context: &str) -> bool {
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (rss_warn_mb, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
    let (free_warn_mb, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
    if let Some(free_mb) = snapshot.mem_free_mb {
        if ga_should_abort_on_mem_free(
            free_mb,
            free_warn_mb,
            free_abort_mb,
            snapshot.total_mem_mb,
            context,
        ) {
            return true;
        }
        if free_mb < free_warn_mb {
            GA_EVAL_MEM_WARN.store(true, Ordering::Relaxed);
            ga_set_worker_limit_auto(1);
        }
    }
    if let Some(rss_mb) = snapshot.proc_rss_mb {
        if ga_should_abort_on_rss(
            rss_mb,
            rss_warn_mb,
            rss_abort_mb,
            snapshot.total_mem_mb,
            context,
        ) {
            return true;
        }
        if rss_mb >= rss_warn_mb {
            GA_EVAL_MEM_WARN.store(true, Ordering::Relaxed);
            ga_set_worker_limit_auto(1);
        }
        if rss_mb >= rss_warn_mb {
            nm_log!(
                "[info] GA pacing: RSS {}MB exceeds warning during {}.",
                rss_mb,
                context
            );
            ga_apply_mem_backoff();
        } else {
            ga_clear_rss_critical();
        }
    }
    if let Some(growth_mb) = snapshot.proc_rss_growth_mb {
        if ga_should_abort_on_mem_growth(
            growth_mb,
            h.mem_rss_growth_warn_mb,
            h.mem_rss_growth_abort_mb,
            context,
        ) {
            return true;
        }
        if growth_mb >= h.mem_rss_growth_warn_mb {
            GA_EVAL_MEM_WARN.store(true, Ordering::Relaxed);
            ga_set_worker_limit_auto(1);
            let (base_rss, last_rss, max_rss) = ga_mem_tracker_snapshot();
            nm_log!(
                "[info] GA mem_growth stats: base {:?}MB last {:?}MB max {:?}MB (growth {}MB).",
                base_rss,
                last_rss,
                max_rss,
                growth_mb
            );
        }
    }
    false
}

fn ga_throttle_if_needed() -> Duration {
    let ticket = GA_THROTTLE_TICKET.load(Ordering::Relaxed);
    let last_ticket = LAST_THROTTLE_TICKET.with(|c| c.get());

    if ticket > last_ticket {
        let ms = GA_THROTTLE_MS.load(Ordering::Relaxed);
        if ms > 0 {
            if ga_check_mem_pressure("throttle_wait") {
                LAST_THROTTLE_TICKET.with(|c| c.set(ticket));
                return Duration::ZERO;
            }
            let dur = Duration::from_millis(ms as u64);
            GA_THROTTLED_MS.fetch_add(ms as u64, Ordering::Relaxed);
            std::thread::sleep(dur);
            let _ = ga_check_mem_pressure("throttle_wait");
            LAST_THROTTLE_TICKET.with(|c| c.set(ticket));
            return dur;
        }
        LAST_THROTTLE_TICKET.with(|c| c.set(ticket));
    }
    Duration::ZERO
}

fn ga_wait_for_eval_mem(context: &str) -> bool {
    let start = Instant::now();
    let mut last_log = Instant::now() - Duration::from_secs(5);
    loop {
        if GA_ABORT_REQUESTED.load(Ordering::Relaxed) {
            return false;
        }
        let snapshot = ga_safety_snapshot();
        let h = ga_heuristics()
            .lock()
            .expect("GA heuristics lock poisoned")
            .clone();
        let (rss_warn_mb, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
        let (free_warn_mb, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
        let mut critical = false;
        let mut warn = false;
        if let Some(free_mb) = snapshot.mem_free_mb {
            if free_mb < free_abort_mb {
                critical = true;
            } else if free_mb < free_warn_mb {
                warn = true;
            }
        }
        if let Some(rss_mb) = snapshot.proc_rss_mb {
            if rss_mb >= rss_abort_mb {
                critical = true;
            } else if rss_mb >= rss_warn_mb {
                warn = true;
            }
        }
        if let Some(growth_mb) = snapshot.proc_rss_growth_mb {
            if growth_mb >= h.mem_rss_growth_abort_mb {
                critical = true;
            } else if growth_mb >= h.mem_rss_growth_warn_mb {
                warn = true;
            }
        }

        if critical {
            if last_log.elapsed() > Duration::from_secs(2) {
                nm_log!(
                    "[info] GA eval mem gate: critical pressure during {} (rss {:?}MB free {:?}MB growth {:?}MB).",
                    context,
                    snapshot.proc_rss_mb,
                    snapshot.mem_free_mb,
                    snapshot.proc_rss_growth_mb
                );
                last_log = Instant::now();
            }
            ga_request_throttle(250);
            std::thread::sleep(Duration::from_millis(250));
            if start.elapsed() > Duration::from_secs(10) {
                nm_err!(
                    "[warn] GA eval mem gate abort: pressure stayed critical for {:.1}s during {}.",
                    start.elapsed().as_secs_f32(),
                    context
                );
                ga_request_abort("mem_eval_gate_abort");
                return false;
            }
            continue;
        }
        if warn {
            if last_log.elapsed() > Duration::from_secs(5) {
                nm_log!(
                    "[info] GA eval mem gate: waiting under pressure during {} (rss {:?}MB free {:?}MB growth {:?}MB).",
                    context,
                    snapshot.proc_rss_mb,
                    snapshot.mem_free_mb,
                    snapshot.proc_rss_growth_mb
                );
                last_log = Instant::now();
            }
            ga_request_throttle(150);
            std::thread::sleep(Duration::from_millis(150));
            if start.elapsed() > Duration::from_secs(20) {
                return true;
            }
            continue;
        }
        return true;
    }
}

fn ga_eval_mem_guard(start_rss_mb: Option<u64>, context: &str) -> bool {
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    if let Some(free_mb) = snapshot.mem_free_mb {
        let (free_warn_mb, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
        if free_mb < free_abort_mb {
            nm_err!(
                "[warn] GA eval mem guard: free {}MB below abort during {} (warn {}MB, abort {}MB).",
                free_mb,
                context,
                free_warn_mb,
                free_abort_mb
            );
            ga_request_abort("mem_eval_guard_abort");
            return false;
        }
    }
    if let Some(rss_mb) = snapshot.proc_rss_mb {
        let (rss_warn_mb, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
        if rss_mb >= rss_abort_mb {
            nm_err!(
                "[warn] GA eval mem guard: RSS {}MB above abort during {} (warn {}MB, abort {}MB).",
                rss_mb,
                context,
                rss_warn_mb,
                rss_abort_mb
            );
            ga_request_abort("mem_eval_guard_abort");
            return false;
        }
    }
    if let (Some(start), Some(cur)) = (start_rss_mb, snapshot.proc_rss_mb) {
        let growth = cur.saturating_sub(start);
        let mut eval_warn = h.mem_rss_growth_warn_mb;
        let mut eval_abort = h.mem_rss_growth_abort_mb;
        if let Some(total_mb) = snapshot.total_mem_mb {
            let pct_warn = ((total_mb as f32) * 0.08).ceil() as u64;
            let pct_abort = ((total_mb as f32) * 0.12).ceil() as u64;
            eval_warn = eval_warn.min(pct_warn.max(512));
            eval_abort = eval_abort.min(pct_abort.max(eval_warn + 256));
        }
        if growth >= eval_abort {
            nm_err!(
                "[warn] GA eval mem guard: RSS growth {}MB above abort during {} (abort {}MB).",
                growth,
                context,
                eval_abort
            );
            ga_request_abort("mem_eval_guard_abort");
            return false;
        }
        if growth >= eval_warn {
            nm_log!(
                "[info] GA eval mem guard: RSS growth {}MB above warn during {} (warn {}MB).",
                growth,
                context,
                eval_warn
            );
        }
        if growth >= eval_warn {
            GA_EVAL_MEM_WARN.store(true, Ordering::Relaxed);
        }
    }
    true
}

#[allow(dead_code)]
fn ga_use_opencl() -> bool {
    parse_env_bool("NM_GA_USE_OPENCL").unwrap_or(false)
}

#[allow(dead_code)]
fn ga_auto_opencl() -> bool {
    parse_env_bool("NM_GA_AUTO_OPENCL").unwrap_or(true)
}

fn ga_should_use_opencl() -> bool {
    #[cfg(feature = "opencl")]
    {
        if GA_FORCE_CPU.load(Ordering::Relaxed) {
            return false;
        }
        if let Some(force) = parse_env_bool("NM_GA_USE_OPENCL") {
            return force;
        }
        if parse_env_bool("NM_GA_FORCE_CPU").unwrap_or(false) {
            return false;
        }
        if !ga_auto_opencl() {
            return false;
        }
        let snapshot = ga_safety_snapshot();
        let h = ga_heuristics()
            .lock()
            .expect("GA heuristics lock poisoned")
            .clone();
        let cpu_hot = snapshot
            .cpu_usage_pct
            .map(|v| v >= h.cpu_warn_pct)
            .unwrap_or(false);
        let gpu_hot = snapshot
            .gpu_util_pct
            .map(|v| v >= h.gpu_util_warn_pct)
            .unwrap_or(false);
        let gpu_low_vram = snapshot
            .gpu_vram_free_mb
            .map(|v| v < h.gpu_vram_free_min_mb)
            .unwrap_or(false);
        if cpu_hot && !gpu_hot && !gpu_low_vram {
            return true;
        }
        if gpu_hot || gpu_low_vram {
            return false;
        }
        false
    }
    #[cfg(not(feature = "opencl"))]
    {
        false
    }
}

fn ga_opencl_workers() -> usize {
    parse_env_usize("NM_GA_OPENCL_WORKERS")
        .unwrap_or(DEFAULT_GA_OPENCL_WORKERS)
        .max(1)
}

#[cfg(feature = "core_affinity")]
fn ga_core_history() -> &'static Mutex<GACoreHistory> {
    GA_CORE_HISTORY.get_or_init(|| {
        let mut history = if let Ok(s) = std::fs::read_to_string(ga_core_history_path()) {
            serde_json::from_str::<GACoreHistory>(&s).unwrap_or(GACoreHistory {
                version: 1,
                counts: HashMap::new(),
                last_save: Instant::now(),
            })
        } else {
            GACoreHistory {
                version: 1,
                counts: HashMap::new(),
                last_save: Instant::now(),
            }
        };
        history.last_save = Instant::now() - Duration::from_secs(60);
        Mutex::new(history)
    })
}

#[cfg(feature = "core_affinity")]
fn ga_save_core_history(history: &GACoreHistory) {
    let s = serde_json::to_string_pretty(history);
    if let Ok(s) = s {
        let _ = std::fs::write(ga_core_history_path(), s);
    }
}

#[cfg(feature = "core_affinity")]
fn ga_record_core_usage(core_ids: &[CoreId]) {
    let mut history = ga_core_history()
        .lock()
        .expect("GA core history lock poisoned");
    for id in core_ids {
        *history.counts.entry(id.id).or_insert(0) += 1;
    }
    if history.last_save.elapsed() > Duration::from_secs(30) {
        history.last_save = Instant::now();
        ga_save_core_history(&history);
    }
    let slot = GA_LAST_CORE_AFFINITY.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("GA core affinity lock poisoned");
    *guard = Some(core_ids.iter().map(|c| c.id).collect());
}

#[cfg(feature = "core_affinity")]
fn ga_affinity_core_ids_from_env() -> Option<Vec<CoreId>> {
    let indices = parse_env_usize_list("NM_GA_CORE_AFFINITY")
        .or_else(|| parse_env_usize_list("NM_GA_CPU_CORES"))?;
    let available = core_affinity::get_core_ids()?;
    let mut selected = Vec::new();
    for idx in indices {
        if let Some(id) = available.iter().find(|c| c.id == idx) {
            selected.push(*id);
        }
    }
    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

#[cfg(feature = "core_affinity")]
fn ga_core_history_count_limit(available: usize) -> usize {
    if let Some(v) = parse_env_usize("NM_GA_CORE_HISTORY_COUNT") {
        return v.clamp(1, available.max(1));
    }
    (available / 2).max(1)
}

#[cfg(feature = "core_affinity")]
fn ga_affinity_core_ids() -> Option<Vec<CoreId>> {
    if let Some(ids) = ga_affinity_core_ids_from_env() {
        return Some(ids);
    }
    let use_history = parse_env_bool("NM_GA_USE_CORE_HISTORY").unwrap_or(true);
    if !use_history {
        return None;
    }
    let available = core_affinity::get_core_ids()?;
    let history = ga_core_history()
        .lock()
        .expect("GA core history lock poisoned");
    if history.counts.is_empty() {
        return None;
    }
    let mut scored: Vec<(CoreId, u64)> = available
        .iter()
        .map(|c| (*c, *history.counts.get(&c.id).unwrap_or(&0)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    let limit = ga_core_history_count_limit(scored.len());
    let selected: Vec<CoreId> = scored.into_iter().take(limit).map(|(c, _)| c).collect();
    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

#[cfg(feature = "opencl")]
fn ga_opencl_device_indices() -> Vec<usize> {
    GA_CL_DEVICE_INDICES
        .get_or_init(|| {
            // GA device selection: NM_GA_CL_DEVICE_INDICES or NM_GA_CL_DEVICES (comma-separated).
            let indices = parse_env_usize_list("NM_GA_CL_DEVICE_INDICES")
                .or_else(|| parse_env_usize_list("NM_GA_CL_DEVICES"));
            indices.unwrap_or_default()
        })
        .clone()
}

#[cfg(feature = "opencl")]
fn ga_opencl_device_count() -> usize {
    *GA_CL_DEVICE_COUNT.get_or_init(|| {
        let indices = ga_opencl_device_indices();
        let ids = if indices.is_empty() {
            gpu_device_ids_for_indices(None)
        } else {
            gpu_device_ids_for_indices(Some(indices.as_slice()))
        };
        match ids {
            Ok(ids) => ids.len().max(1),
            Err(e) => {
                nm_err!("[warn] GA OpenCL device discovery failed: {}", e);
                1
            }
        }
    })
}

fn ga_opencl_worker_limit_base(pop_size: usize) -> usize {
    let mut workers = ga_opencl_workers();
    let active = GA_ACTIVE_POPULATIONS.load(Ordering::SeqCst).max(1);
    workers = (workers / active).max(1);
    #[cfg(feature = "opencl")]
    {
        let device_count = ga_opencl_device_count();
        workers = workers.saturating_mul(device_count).max(1);
    }
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    let (rss_warn_mb, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
    let (free_warn_mb, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
    if let Some(cpu) = snapshot.cpu_usage_pct {
        if cpu >= h.cpu_hot_pct {
            workers = (workers / 2).max(1);
        } else if cpu >= h.cpu_warn_pct && !ga_should_use_opencl() {
            workers = (workers / 2).max(1);
        }
    }
    if let Some(free_mb) = snapshot.mem_free_mb {
        if free_mb < free_abort_mb {
            workers = 1;
        } else if free_mb < free_warn_mb {
            workers = (workers / 2).max(1);
        }
    }
    if let Some(rss_mb) = snapshot.proc_rss_mb {
        if rss_mb >= rss_abort_mb {
            workers = 1;
        } else if rss_mb >= rss_warn_mb {
            workers = (workers / 2).max(1);
        }
    }
    if let Some(ui_ms) = snapshot.ui_frame_ms {
        if ui_ms >= h.ui_frame_hot_ms {
            workers = 1;
        } else if ui_ms >= h.ui_frame_warn_ms {
            workers = (workers / 2).max(1);
        }
    }
    if let Some(temp) = update_temp_cache() {
        let warn = ga_temp_warn_c();
        let hot = ga_temp_hot_c();
        if temp >= hot {
            workers = 1;
        } else if temp >= warn {
            workers = (workers / 2).max(1);
        }
    }
    #[cfg(feature = "ui")]
    {
        if workers > 1 {
            workers = workers.saturating_sub(1).max(1);
        }
    }
    workers.min(pop_size.max(1))
}

#[cfg(feature = "opencl")]
thread_local! {
    static GA_OPENCL_MANAGER: RefCell<Option<Arc<OpenCLManager>>> = RefCell::new(None);
}

#[cfg(feature = "opencl")]
fn ga_thread_opencl_manager() -> Option<Arc<OpenCLManager>> {
    let opencl_disabled = std::env::var("NM_DISABLE_OPENCL")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);
    if opencl_disabled {
        return None;
    }

    GA_OPENCL_MANAGER.with(|cell| {
        let mut mgr = cell.borrow_mut();
        if mgr.is_none() {
            let indices = ga_opencl_device_indices();
            let gpu_device_ids = if indices.is_empty() {
                match gpu_device_ids_for_indices(None) {
                    Ok(ids) => ids,
                    Err(e) => {
                        nm_err!("[warn] GA OpenCL GPU discovery failed: {}", e);
                        Vec::new()
                    }
                }
            } else {
                match gpu_device_ids_for_indices(Some(indices.as_slice())) {
                    Ok(ids) => ids,
                    Err(e) => {
                        nm_err!("[warn] GA OpenCL GPU discovery failed: {}", e);
                        Vec::new()
                    }
                }
            };
            if !gpu_device_ids.is_empty() {
                let thread_idx = std::thread::current()
                    .name()
                    .and_then(|name| name.rsplit('-').next())
                    .and_then(|suffix| suffix.parse::<usize>().ok())
                    .unwrap_or_else(|| GA_OPENCL_FALLBACK_IDX.fetch_add(1, Ordering::SeqCst));
                let device_id = gpu_device_ids[thread_idx % gpu_device_ids.len()];
                *mgr = OpenCLManager::new_with_device_id(device_id)
                    .ok()
                    .map(Arc::new);
            }
            if mgr.is_none() {
                *mgr = OpenCLManager::new_with_preferred_device_index(0)
                    .ok()
                    .map(Arc::new);
            }
            if mgr.is_none() {
                nm_err!("[warn] GA OpenCL requested but per-thread manager failed to initialize.");
            }
        }
        mgr.clone()
    })
}

fn ga_reserved_cores() -> usize {
    if let Some(v) = parse_env_usize("NM_GA_RESERVE_CORES") {
        return v;
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if cores <= 2 {
        return 1;
    }
    let reserve = (cores / 4).max(DEFAULT_GA_RESERVED_CORES);
    reserve.min(cores.saturating_sub(1)).max(1)
}

fn default_max_concurrent_populations() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if cores >= 16 {
        DEFAULT_GA_MAX_CONCURRENT_POPULATIONS_LARGE
    } else {
        DEFAULT_GA_MAX_CONCURRENT_POPULATIONS_SMALL
    }
}

fn max_concurrent_populations() -> usize {
    parse_env_usize("NM_GA_MAX_CONCURRENT_POPULATIONS")
        .unwrap_or_else(default_max_concurrent_populations)
        .max(1)
}

fn max_concurrent_evaluations() -> usize {
    if let Some(v) = parse_env_usize("NM_GA_MAX_CONCURRENT_EVALS") {
        return v.max(1);
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let reserve = ga_reserved_cores();
    cores.saturating_sub(reserve).max(1)
}

fn ga_temp_warn_c() -> f32 {
    parse_env_f32("NM_GA_TEMP_WARN_C").unwrap_or(DEFAULT_GA_TEMP_WARN_C)
}

fn ga_temp_hot_c() -> f32 {
    parse_env_f32("NM_GA_TEMP_HOT_C").unwrap_or(DEFAULT_GA_TEMP_HOT_C)
}

fn ga_max_eval_ms_configured() -> Option<u64> {
    if let Some(v) = parse_env_usize("NM_GA_MAX_EVAL_MS") {
        return Some(v.max(1) as u64);
    }
    let h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
    Some(h.gen_max_ms.min(h.gen_max_ms.saturating_mul(2)).max(10_000))
}

fn ga_max_eval_ms() -> Option<u64> {
    let configured = ga_max_eval_ms_configured();
    let override_ms = GA_EVAL_MS_OVERRIDE.load(Ordering::Relaxed);
    let mut base = if override_ms > 0 {
        configured.map_or(override_ms, |v| v.min(override_ms))
    } else {
        configured.unwrap_or(120_000)
    };

    // Under thermal pressure, allow more time for individual evaluations.
    let snapshot = ga_safety_snapshot();
    if let Some(temp) = snapshot.temp_c {
        let h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
        if temp >= h.temp_warn_c {
            let factor = if temp >= h.temp_hot_c { 2.5 } else { 1.5 };
            base = (base as f64 * factor) as u64;
        }
    }
    Some(base)
}

fn ga_max_eval_neurons_configured() -> Option<usize> {
    parse_env_usize("NM_GA_MAX_EVAL_NEURONS").map(|v| v.max(1))
}

static GA_AUTO_EVAL_LIMITS_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn ga_active_eval_factor() -> usize {
    GA_ACTIVE_EVALS.load(Ordering::SeqCst).max(1)
}

fn ga_auto_eval_neuron_cap(snapshot: &SafetySnapshot) -> Option<usize> {
    let total_mb = snapshot.total_mem_mb?;
    let mut cap = (total_mb as f64 * 1.5).round() as usize;

    // Reduce cap if system is hot or under high CPU load
    if let Some(temp) = snapshot.temp_c {
        let h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
        if temp >= h.temp_hot_c {
            cap /= 4;
        } else if temp >= h.temp_warn_c {
            cap /= 2;
        }
    }
    if let Some(cpu) = snapshot.cpu_usage_pct {
        if cpu > 90.0 {
            cap = (cap as f64 * 0.7).round() as usize;
        }
    }

    let cap = cap / ga_active_eval_factor();
    Some(cap.clamp(1_000, 500_000))
}

fn ga_auto_eval_connection_cap(
    snapshot: &SafetySnapshot,
    neuron_cap: Option<usize>,
) -> Option<usize> {
    if let Some(n_cap) = neuron_cap {
        let cap = (n_cap as f64 * 30.0).round() as usize;
        return Some(cap.clamp(10_000, 25_000_000));
    }
    let total_mb = snapshot.total_mem_mb?;
    let cap = (total_mb as f64 * 120.0).round() as usize / ga_active_eval_factor();
    Some(cap.clamp(50_000, 20_000_000))
}

fn ga_max_eval_segments_configured() -> Option<usize> {
    parse_env_usize("NM_GA_MAX_EVAL_SEGMENTS").map(|v| v.max(1))
}

fn ga_auto_eval_segment_cap(snapshot: &SafetySnapshot, neuron_cap: Option<usize>) -> Option<usize> {
    if let Some(n_cap) = neuron_cap {
        let cap = (n_cap as f64 * 6.0).round() as usize;
        return Some(cap.clamp(10_000, 300_000));
    }
    let total_mb = snapshot.total_mem_mb?;
    let cap = (total_mb as f64 * 3.0).round() as usize / ga_active_eval_factor();
    Some(cap.clamp(10_000, 250_000))
}

fn ga_max_eval_neurons() -> Option<usize> {
    let configured = ga_max_eval_neurons_configured();
    let snapshot = ga_safety_snapshot();
    let auto = configured.or_else(|| ga_auto_eval_neuron_cap(&snapshot));
    let override_neurons = GA_EVAL_NEURONS_OVERRIDE.load(Ordering::Relaxed);
    if override_neurons > 0 {
        return Some(auto.map_or(override_neurons, |v| v.min(override_neurons)));
    }
    if configured.is_none() {
        if !GA_AUTO_EVAL_LIMITS_LOGGED.swap(true, Ordering::SeqCst) {
            nm_log!(
                "[info] GA auto eval caps: max_neurons {:?} max_conns {:?} max_segments {:?} total_mem {:?}MB active_evals {}.",
                auto,
                ga_auto_eval_connection_cap(&snapshot, auto),
                ga_auto_eval_segment_cap(&snapshot, auto),
                snapshot.total_mem_mb,
                ga_active_eval_factor()
            );
        }
    }
    auto
}

fn ga_max_eval_connections_configured() -> Option<usize> {
    parse_env_usize("NM_GA_MAX_EVAL_CONNECTIONS").map(|v| v.max(1))
}

fn ga_max_eval_connections() -> Option<usize> {
    let configured = ga_max_eval_connections_configured();
    let snapshot = ga_safety_snapshot();
    let auto_neurons = ga_max_eval_neurons();
    let auto = configured.or_else(|| ga_auto_eval_connection_cap(&snapshot, auto_neurons));
    let override_conns = GA_EVAL_CONNS_OVERRIDE.load(Ordering::Relaxed);
    if override_conns > 0 {
        return Some(auto.map_or(override_conns, |v| v.min(override_conns)));
    }
    auto
}

fn ga_max_eval_segments() -> Option<usize> {
    let configured = ga_max_eval_segments_configured();
    let override_segs = GA_EVAL_SEGMENTS_OVERRIDE.load(Ordering::Relaxed);
    if override_segs > 0 {
        return Some(configured.map_or(override_segs, |v| v.min(override_segs)));
    }
    if configured.is_some() {
        return configured;
    }
    let snapshot = ga_safety_snapshot();
    let auto_neurons = ga_max_eval_neurons();
    ga_auto_eval_segment_cap(&snapshot, auto_neurons)
}

#[cfg(all(feature = "morpho", feature = "growth3d"))]
fn ga_morph_segment_count(runner: &Runner) -> usize {
    let mut total = runner.morph.synapses.len();
    for layer in &runner.morph.axons {
        for axon in layer {
            total = total.saturating_add(axon.segments.len());
        }
    }
    for axon in &runner.morph.sensory_axons {
        total = total.saturating_add(axon.segments.len());
    }
    for axon in &runner.morph.output_axons {
        total = total.saturating_add(axon.segments.len());
    }
    for layer in &runner.morph.dendrites {
        for den in layer {
            total = total.saturating_add(den.tree.branches.len());
        }
    }
    for den in &runner.morph.sensory_dendrites {
        total = total.saturating_add(den.tree.branches.len());
    }
    for den in &runner.morph.output_dendrites {
        total = total.saturating_add(den.tree.branches.len());
    }
    total
}

#[cfg(not(all(feature = "morpho", feature = "growth3d")))]
fn ga_morph_segment_count(_runner: &Runner) -> usize {
    0
}

pub fn ga_eval_limits_max() -> (Option<u64>, Option<usize>, Option<usize>) {
    (
        ga_max_eval_ms_configured(),
        ga_max_eval_neurons_configured(),
        ga_max_eval_connections_configured(),
    )
}

fn ga_gen_max_ms() -> u64 {
    if let Some(v) = parse_env_usize("NM_GA_GEN_MAX_MS") {
        return v.max(1) as u64;
    }
    let h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
    h.gen_max_ms.max(30_000)
}

fn population_semaphore() -> &'static Arc<Semaphore> {
    GA_POPULATION_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(max_concurrent_populations())))
}

fn evaluation_semaphore() -> &'static Arc<Semaphore> {
    GA_EVAL_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(max_concurrent_evaluations())))
}

#[derive(Clone, Copy)]
// Cache structs moved to monitor.rs

// GA_SYS_CACHE and GA_GPU_CACHE moved to monitor.rs

struct GAMemTracker {
    baseline_rss_mb: Option<u64>,
    last_rss_mb: Option<u64>,
    max_rss_mb: Option<u64>,
}

#[cfg(feature = "core_affinity")]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct GACoreHistory {
    version: u32,
    counts: HashMap<usize, u64>,
    #[serde(skip, default = "ga_now")]
    last_save: Instant,
}

#[cfg(feature = "core_affinity")]
fn ga_now() -> Instant {
    Instant::now()
}

// update_*_cache functions moved to monitor.rs

fn ga_ui_frame_ms() -> Option<f32> {
    let raw = GA_UI_FRAME_MS_X100.load(Ordering::Relaxed);
    if raw == 0 {
        None
    } else {
        Some(raw as f32 / 100.0)
    }
}

fn ga_safety_snapshot() -> SafetySnapshot {
    let mut snap = monitor::get_safety_snapshot(ga_ui_frame_ms());
    let (_, proc_rss_growth_mb) = ga_record_mem_rss(snap.proc_rss_mb);
    snap.proc_rss_growth_mb = proc_rss_growth_mb;
    snap
}

enum GASafetyAction {
    None,
    Throttle(Duration, String),
    Abort(String),
}

fn ga_record_safety_event(reason: &str) {
    let mut h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
    ga_tighten_heuristics(&mut h, reason);
    let _ = ga_save_heuristics(&h);
}

fn ga_record_safe_generation() {
    let mut h = ga_heuristics().lock().expect("GA heuristics lock poisoned");
    h.safe_generations = h.safe_generations.saturating_add(1);
    if h.safe_generations >= 5 {
        ga_relax_heuristics(&mut h);
        h.safe_generations = 0;
    }
    let _ = ga_save_heuristics(&h);
}

fn ga_safety_decision(gen_elapsed: Duration) -> GASafetyAction {
    let mut gen_elapsed = gen_elapsed;
    let mut snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    ga_try_clear_auto_worker_limit(&snapshot, &h);

    if let Some(temp) = snapshot.temp_c {
        if temp >= h.temp_hot_c {
            ga_set_worker_limit_auto(1);
            let paused_before = GA_PAUSED_MS.load(Ordering::Relaxed);
            thermal_wait_blocking("generation");
            let paused_after = GA_PAUSED_MS.load(Ordering::Relaxed);
            let waited_ms = paused_after.saturating_sub(paused_before) as u64;
            if waited_ms > 0 {
                gen_elapsed = gen_elapsed.saturating_add(Duration::from_millis(waited_ms));
            }
            if GA_ABORT_REQUESTED.load(Ordering::SeqCst) {
                if let Some(reason) = ga_abort_reason() {
                    return GASafetyAction::Abort(reason);
                }
                return GASafetyAction::Abort("abort_requested".to_string());
            }
            snapshot = ga_safety_snapshot();
        }
    }

    let (rss_warn_mb, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
    let (free_warn_mb, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
    let paused_ms = GA_PAUSED_MS.load(Ordering::Relaxed) as u64;
    let throttled_ms = GA_THROTTLED_MS.load(Ordering::Relaxed);
    let remote_wait_ms = GA_REMOTE_WAIT_MS.load(Ordering::Relaxed);

    // Account for throttled time correctly in generation timeout.
    // Since workers are parallel, we take the average throttled time per worker as a heuristic,
    // but a safer approach is to track wall-clock time spent in throttles.
    // For now, we'll use GA_THROTTLED_MS but cap it at gen_elapsed to avoid negative durations.
    let pop_size = GA_CURRENT_POP_SIZE.load(Ordering::Relaxed);
    let worker_budget = population_worker_threads(pop_size).max(1);
    let effective_workers = ga_effective_parallelism(pop_size);
    let wall_throttled_ms = throttled_ms / effective_workers as u64;

    let effective_elapsed = gen_elapsed.saturating_sub(Duration::from_millis(
        paused_ms + wall_throttled_ms + remote_wait_ms,
    ));
    let mut max_ms = ga_dynamic_gen_timeout_ms(pop_size);
    if worker_budget > effective_workers {
        let slowdown = (worker_budget as f64 / effective_workers as f64).clamp(1.0, 8.0);
        max_ms = (max_ms as f64 * slowdown).round() as u64;
    }

    // Under thermal pressure, ease off by extending the allowed time.
    if let Some(temp) = snapshot.temp_c {
        if temp >= h.temp_warn_c {
            let factor = if temp >= h.temp_hot_c { 2.5 } else { 1.5 };
            max_ms = (max_ms as f64 * factor) as u64;
        }
    }

    let grace_ms = if wall_throttled_ms > 0 { 10_000 } else { 2_000 };
    let timeout_budget_ms = max_ms.saturating_add(grace_ms);
    if effective_elapsed > Duration::from_millis(timeout_budget_ms) {
        let ema_ms = GA_EVAL_INDIVIDUAL_EMA_MS.load(Ordering::Relaxed);
        let completed = GA_COMPLETED_EVALS.load(Ordering::Relaxed).min(pop_size);
        let progress_pct = if pop_size == 0 {
            100.0
        } else {
            (completed as f64 * 100.0) / pop_size as f64
        };
        let hard_abort_ms = timeout_budget_ms.saturating_mul(4);
        if effective_elapsed <= Duration::from_millis(hard_abort_ms) {
            nm_log!(
                "[info] GA generation slow: effective_elapsed {}ms (wall {}ms, paused {}ms, throttled_avg {}ms, remote_wait {}ms) > budget {}ms. progress {}/{} ({:.1}%), workers budget/effective {}/{} ema_ind_ms {}. Continuing with throttle.",
                effective_elapsed.as_millis(),
                gen_elapsed.as_millis(),
                paused_ms,
                wall_throttled_ms,
                remote_wait_ms,
                timeout_budget_ms,
                completed,
                pop_size,
                progress_pct,
                worker_budget,
                effective_workers,
                ema_ms
            );
            return GASafetyAction::Throttle(
                Duration::from_millis(400),
                "generation_slow".to_string(),
            );
        }
        nm_err!(
            "[warn] GA generation timeout hard abort: effective_elapsed {}ms > hard budget {}ms (budget {}ms). progress {}/{} workers budget/effective {}/{} ema_ind_ms {} rss {:?}MB free {:?}MB.",
            effective_elapsed.as_millis(),
            hard_abort_ms,
            timeout_budget_ms,
            completed,
            pop_size,
            worker_budget,
            effective_workers,
            ema_ms,
            snapshot.proc_rss_mb,
            snapshot.mem_free_mb
        );
        return GASafetyAction::Abort("generation_timeout".to_string());
    }
    if let Some(cpu) = snapshot.cpu_usage_pct {
        if cpu >= h.cpu_warn_pct {
            return GASafetyAction::Throttle(Duration::from_millis(200), "cpu_warn".to_string());
        }
    }
    if let Some(free_mb) = snapshot.mem_free_mb {
        if free_mb < free_abort_mb {
            if GA_ACTIVE_EVALS.load(Ordering::Relaxed) > 0 {
                nm_err!(
                    "[warn] GA abort: free memory {}MB below abort threshold (hard abort).",
                    free_mb
                );
                return GASafetyAction::Abort("mem_critical".to_string());
            }
            let (streak, elapsed) = ga_note_mem_free_critical();
            ga_apply_mem_backoff();
            if streak >= 6 || elapsed >= Duration::from_secs(30) {
                nm_err!(
                    "[warn] GA abort: free memory {}MB stayed below critical threshold for {:.1}s ({} checks). Possible leak.",
                    free_mb,
                    elapsed.as_secs_f32(),
                    streak
                );
                return GASafetyAction::Abort("mem_critical".to_string());
            }
            return GASafetyAction::Throttle(
                Duration::from_millis(1000),
                "mem_critical".to_string(),
            );
        }
        if free_mb < free_warn_mb {
            ga_apply_mem_backoff();
            return GASafetyAction::Throttle(Duration::from_millis(200), "mem_warn".to_string());
        }
        ga_clear_mem_free_critical();
        ga_clear_mem_backoff();
    }
    if let Some(rss_mb) = snapshot.proc_rss_mb {
        if rss_mb >= rss_abort_mb {
            if ga_rss_grace_active() {
                ga_request_throttle(2000);
                nm_log!(
                    "[info] GA pacing: RSS {}MB above critical threshold during {} (startup grace).",
                    rss_mb,
                    "safety_decision"
                );
                return GASafetyAction::Throttle(
                    Duration::from_millis(1000),
                    "mem_rss_grace".to_string(),
                );
            }
            let (streak, elapsed) = ga_note_rss_critical();
            ga_request_throttle(2000);
            if streak >= 6 || elapsed >= Duration::from_secs(30) {
                nm_err!(
                    "[warn] GA abort: RSS {}MB stayed above critical threshold for {:.1}s ({} checks). Possible leak.",
                    rss_mb,
                    elapsed.as_secs_f32(),
                    streak
                );
                return GASafetyAction::Abort("mem_rss_abort".to_string());
            }
            return GASafetyAction::Throttle(
                Duration::from_millis(1000),
                "mem_rss_critical".to_string(),
            );
        }
        if rss_mb >= rss_warn_mb {
            return GASafetyAction::Throttle(
                Duration::from_millis(200),
                "mem_rss_warn".to_string(),
            );
        }
        ga_clear_rss_critical();
    }
    if let Some(growth_mb) = snapshot.proc_rss_growth_mb {
        if growth_mb >= h.mem_rss_growth_abort_mb {
            let (streak, elapsed) = ga_note_mem_growth_critical();
            ga_apply_mem_backoff();
            if streak >= 6 || elapsed >= Duration::from_secs(30) {
                nm_err!(
                    "[warn] GA abort: RSS growth {}MB stayed above critical threshold for {:.1}s ({} checks). Possible leak.",
                    growth_mb,
                    elapsed.as_secs_f32(),
                    streak
                );
                return GASafetyAction::Abort("mem_growth_abort".to_string());
            }
            return GASafetyAction::Throttle(
                Duration::from_millis(1000),
                "mem_growth_critical".to_string(),
            );
        }
        if growth_mb >= h.mem_rss_growth_warn_mb {
            ga_apply_mem_backoff();
            return GASafetyAction::Throttle(
                Duration::from_millis(200),
                format!("mem_growth_{}_mb", growth_mb),
            );
        }
        ga_clear_mem_growth_critical();
        ga_clear_mem_backoff();
    }
    if let Some(ui_ms) = snapshot.ui_frame_ms {
        if ui_ms >= h.ui_frame_hot_ms {
            let slot = GA_UI_LAG_START.get_or_init(|| Mutex::new(None));
            let mut guard = slot.lock().expect("GA UI lag lock poisoned");
            if guard.is_none() {
                *guard = Some(Instant::now());
            }
            return GASafetyAction::Throttle(Duration::from_millis(200), "ui_lag_hot".to_string());
        }
        if ui_ms >= h.ui_frame_warn_ms {
            return GASafetyAction::Throttle(Duration::from_millis(200), "ui_lag_warn".to_string());
        }
        ga_clear_ui_lag_start();
    }
    if let Some(util) = snapshot.gpu_util_pct {
        if util >= h.gpu_util_hot_pct {
            return GASafetyAction::Abort("gpu_hot".to_string());
        }
        if util >= h.gpu_util_warn_pct {
            return GASafetyAction::Throttle(Duration::from_millis(200), "gpu_warn".to_string());
        }
    }
    if let Some(temp) = snapshot.temp_c {
        if temp >= h.temp_hot_c {
            ga_set_worker_limit_auto(1);
        }
    }
    if let Some(free_mb) = snapshot.gpu_vram_free_mb {
        if free_mb < (h.gpu_vram_free_min_mb / 2).max(128) {
            #[cfg(feature = "opencl")]
            {
                GA_FORCE_CPU.store(true, Ordering::Relaxed);
            }
            ga_set_worker_limit_auto(1);
            return GASafetyAction::Throttle(
                Duration::from_millis(200),
                "gpu_vram_critical".to_string(),
            );
        }
        if free_mb < h.gpu_vram_free_min_mb {
            return GASafetyAction::Throttle(
                Duration::from_millis(200),
                "gpu_vram_warn".to_string(),
            );
        }
        #[cfg(feature = "opencl")]
        {
            if free_mb >= h.gpu_vram_free_min_mb {
                GA_FORCE_CPU.store(false, Ordering::Relaxed);
            }
        }
    }
    GASafetyAction::None
}

async fn thermal_wait_if_hot(kind: &str) {
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .to_monitor();
    GA_THERMAL_WAITERS.fetch_add(1, Ordering::SeqCst);
    let waited = monitor::thermal_wait_if_hot(kind, &h, &GA_ABORT_REQUESTED).await;
    GA_THERMAL_WAITERS.fetch_sub(1, Ordering::SeqCst);
    let waited_ms = waited.as_millis() as u32;
    if waited_ms > 0 {
        GA_PAUSED_MS.fetch_add(waited_ms, Ordering::Relaxed);
    }
}

fn thermal_wait_blocking(kind: &str) {
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .to_monitor();
    GA_THERMAL_WAITERS.fetch_add(1, Ordering::SeqCst);
    let waited = monitor::thermal_wait_blocking(kind, &h, &GA_ABORT_REQUESTED);
    GA_THERMAL_WAITERS.fetch_sub(1, Ordering::SeqCst);
    let waited_ms = waited.as_millis() as u32;
    if waited_ms > 0 {
        GA_PAUSED_MS.fetch_add(waited_ms, Ordering::Relaxed);
    }
}

pub fn ga_wait_for_generation_headroom() -> bool {
    thermal_wait_blocking("generation");
    !GA_ABORT_REQUESTED.load(Ordering::SeqCst)
}

pub struct GAPopulationPermit {
    _permit: OwnedSemaphorePermit,
}

impl Drop for GAPopulationPermit {
    fn drop(&mut self) {
        GA_ACTIVE_POPULATIONS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub async fn acquire_population_permit() -> GAPopulationPermit {
    thermal_wait_if_hot("population").await;
    let sem = Arc::clone(population_semaphore());
    let will_wait = sem.available_permits() == 0;
    if will_wait {
        nm_log!("[info] GA population queued; waiting for system headroom.");
        GA_SEM_WAITERS.fetch_add(1, Ordering::SeqCst);
    }
    let permit = sem
        .acquire_owned()
        .await
        .expect("GA population semaphore closed");
    if will_wait {
        GA_SEM_WAITERS.fetch_sub(1, Ordering::SeqCst);
    }
    GA_ACTIVE_POPULATIONS.fetch_add(1, Ordering::SeqCst);
    GAPopulationPermit { _permit: permit }
}

pub async fn acquire_evaluation_permit() -> OwnedSemaphorePermit {
    thermal_wait_if_hot("evaluation").await;
    let sem = Arc::clone(evaluation_semaphore());
    let will_wait = sem.available_permits() == 0;
    if will_wait {
        nm_log!("[info] GA evaluation queued; waiting for system headroom.");
        GA_SEM_WAITERS.fetch_add(1, Ordering::SeqCst);
    }
    let permit = sem
        .acquire_owned()
        .await
        .expect("GA evaluation semaphore closed");
    if will_wait {
        GA_SEM_WAITERS.fetch_sub(1, Ordering::SeqCst);
    }
    permit
}

pub fn ga_pacing_status() -> (bool, String) {
    let thermal = GA_THERMAL_WAITERS.load(Ordering::SeqCst) > 0;
    let sem_wait = GA_SEM_WAITERS.load(Ordering::SeqCst) > 0;
    if !thermal && !sem_wait {
        return (false, String::new());
    }
    let mut reasons = Vec::new();
    if thermal {
        reasons.push("thermal");
    }
    if sem_wait {
        reasons.push("capacity");
    }
    (true, reasons.join(", "))
}

#[allow(dead_code)]
pub fn ga_active_evals() -> usize {
    GA_ACTIVE_EVALS.load(Ordering::SeqCst)
}

#[allow(dead_code)]
pub fn ga_temperature_status() -> (Option<f32>, f32, f32) {
    let temp = update_temp_cache();
    (temp, ga_temp_warn_c(), ga_temp_hot_c())
}

fn population_worker_threads_base(pop_size: usize) -> usize {
    if ga_should_use_opencl() {
        return ga_opencl_worker_limit_base(pop_size);
    }
    let active = GA_ACTIVE_POPULATIONS.load(Ordering::SeqCst).max(1);
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let rayon_limit = rayon::current_num_threads().max(1);
    let base = available.min(rayon_limit).max(1);
    let reserve = ga_reserved_cores();
    let budget = base.saturating_sub(reserve).max(1);
    let mut threads = (budget / active).max(1);
    let snapshot = ga_safety_snapshot();
    let h = ga_heuristics()
        .lock()
        .expect("GA heuristics lock poisoned")
        .clone();
    ga_try_clear_auto_worker_limit(&snapshot, &h);
    let (rss_warn_mb, rss_abort_mb) = ga_effective_rss_limits(&snapshot, &h);
    let (free_warn_mb, free_abort_mb) = ga_mem_free_limits(&snapshot, &h);
    if let Some(cpu) = snapshot.cpu_usage_pct {
        if cpu >= h.cpu_hot_pct {
            threads = (threads / 2).max(1);
        } else if cpu >= h.cpu_warn_pct {
            threads = (threads / 2).max(1);
        }
    }
    if let Some(free_mb) = snapshot.mem_free_mb {
        if free_mb < free_abort_mb {
            threads = 1;
        } else if free_mb < free_warn_mb {
            threads = (threads / 2).max(1);
        }
    }
    if let Some(rss_mb) = snapshot.proc_rss_mb {
        if rss_mb >= rss_abort_mb {
            threads = 1;
        } else if rss_mb >= rss_warn_mb {
            threads = (threads / 2).max(1);
        }
    }
    if let Some(temp) = update_temp_cache() {
        if temp >= ga_temp_warn_c() {
            threads = (threads / 2).max(1);
        }
    }
    threads.min(pop_size.max(1))
}

fn population_worker_threads(pop_size: usize) -> usize {
    let mut threads = population_worker_threads_base(pop_size);
    if let Some(limit) = ga_worker_limit_override() {
        threads = threads.min(limit.max(1));
    }
    threads
}

pub fn ga_worker_budget_max(pop_size: usize) -> usize {
    population_worker_threads_base(pop_size)
}

#[derive(Clone, Copy, Debug)]
pub struct GARampPlan {
    pub population_size: usize,
    pub sim_time_ms: f64,
    pub eval_ms: Option<u64>,
    pub eval_neurons: Option<usize>,
    pub eval_conns: Option<usize>,
    pub worker_cap: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GARampRuntimeStatus {
    pub population_size: usize,
    pub worker_cap: usize,
    pub sim_time_ms: f64,
    pub eval_ms: Option<u64>,
    pub eval_neurons: Option<usize>,
    pub eval_conns: Option<usize>,
}

pub fn ga_set_ramp_runtime(plan: &GARampPlan, _generation: usize) {
    GA_RAMP_RUNTIME_ACTIVE.store(false, Ordering::SeqCst);
    GA_RAMP_RUNTIME_POPULATION.store(plan.population_size.max(1), Ordering::Relaxed);
    GA_RAMP_RUNTIME_WORKER_CAP.store(plan.worker_cap.max(1), Ordering::Relaxed);
    GA_RAMP_RUNTIME_SIM_TIME_BITS.store(plan.sim_time_ms.max(1.0).to_bits(), Ordering::Relaxed);
    GA_RAMP_RUNTIME_EVAL_MS.store(plan.eval_ms.unwrap_or(0), Ordering::Relaxed);
    GA_RAMP_RUNTIME_EVAL_NEURONS.store(plan.eval_neurons.unwrap_or(0), Ordering::Relaxed);
    GA_RAMP_RUNTIME_EVAL_CONNS.store(plan.eval_conns.unwrap_or(0), Ordering::Relaxed);
    GA_RAMP_RUNTIME_ACTIVE.store(true, Ordering::SeqCst);
}

pub fn ga_ramp_runtime_status() -> Option<GARampRuntimeStatus> {
    if !GA_RAMP_RUNTIME_ACTIVE.load(Ordering::SeqCst) {
        return None;
    }
    let eval_ms = match GA_RAMP_RUNTIME_EVAL_MS.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v),
    };
    let eval_neurons = match GA_RAMP_RUNTIME_EVAL_NEURONS.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v),
    };
    let eval_conns = match GA_RAMP_RUNTIME_EVAL_CONNS.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v),
    };
    Some(GARampRuntimeStatus {
        population_size: GA_RAMP_RUNTIME_POPULATION.load(Ordering::Relaxed).max(1),
        worker_cap: GA_RAMP_RUNTIME_WORKER_CAP.load(Ordering::Relaxed).max(1),
        sim_time_ms: f64::from_bits(GA_RAMP_RUNTIME_SIM_TIME_BITS.load(Ordering::Relaxed)).max(1.0),
        eval_ms,
        eval_neurons,
        eval_conns,
    })
}

pub fn ga_clear_ramp_runtime_status() {
    GA_RAMP_RUNTIME_ACTIVE.store(false, Ordering::SeqCst);
    GA_RAMP_RUNTIME_POPULATION.store(0, Ordering::Relaxed);
    GA_RAMP_RUNTIME_WORKER_CAP.store(0, Ordering::Relaxed);
    GA_RAMP_RUNTIME_SIM_TIME_BITS.store(0, Ordering::Relaxed);
    GA_RAMP_RUNTIME_EVAL_MS.store(0, Ordering::Relaxed);
    GA_RAMP_RUNTIME_EVAL_NEURONS.store(0, Ordering::Relaxed);
    GA_RAMP_RUNTIME_EVAL_CONNS.store(0, Ordering::Relaxed);
}

#[derive(Clone, Debug)]
pub struct GARampController {
    max_pop: usize,
    current_pop: usize,
    last_safe_pop: usize,
    ramp_locked: bool,
    max_sim_time: f64,
    min_sim_time: f64,
    max_eval_ms: Option<u64>,
    min_eval_ms: Option<u64>,
    max_eval_neurons: Option<usize>,
    min_eval_neurons: Option<usize>,
    max_eval_conns: Option<usize>,
    min_eval_conns: Option<usize>,
}

impl GARampController {
    pub fn new(pop_size: usize, sim_time_ms: f64) -> Self {
        let max_pop = pop_size.max(1);
        let max_sim_time = sim_time_ms.max(1.0);
        let min_sim_time = (max_sim_time * 0.1).max(1000.0).min(max_sim_time);
        let (max_eval_ms, max_eval_neurons, max_eval_conns) = ga_eval_limits_max();
        let min_eval_ms = max_eval_ms.map(|v| (v / 4).max(1000));
        let min_eval_neurons = max_eval_neurons.map(|v| (v / 4).max(1));
        let min_eval_conns = max_eval_conns.map(|v| (v / 4).max(1));
        let start_pop = ga_ramp_start_population(max_pop, ga_worker_budget_max(max_pop));
        Self {
            max_pop,
            current_pop: start_pop,
            last_safe_pop: start_pop,
            ramp_locked: false,
            max_sim_time,
            min_sim_time,
            max_eval_ms,
            min_eval_ms,
            max_eval_neurons,
            min_eval_neurons,
            max_eval_conns,
            min_eval_conns,
        }
    }

    pub fn generation_plan(&self) -> GARampPlan {
        let ratio = if self.max_pop <= 1 {
            1.0
        } else {
            (self.current_pop.saturating_sub(1)) as f64 / (self.max_pop.saturating_sub(1)) as f64
        };
        let sim_time_current = self.min_sim_time + (self.max_sim_time - self.min_sim_time) * ratio;
        let eval_ms_current = self.max_eval_ms.map(|max| {
            let min = self.min_eval_ms.unwrap_or(max);
            (min as f64 + (max.saturating_sub(min) as f64 * ratio))
                .round()
                .max(1.0) as u64
        });
        let eval_neurons_current = self.max_eval_neurons.map(|max| {
            let min = self.min_eval_neurons.unwrap_or(max);
            (min as f64 + (max.saturating_sub(min) as f64 * ratio))
                .round()
                .max(1.0) as usize
        });
        let eval_conns_current = self.max_eval_conns.map(|max| {
            let min = self.min_eval_conns.unwrap_or(max);
            (min as f64 + (max.saturating_sub(min) as f64 * ratio))
                .round()
                .max(1.0) as usize
        });
        let worker_budget_now = ga_worker_budget_max(self.max_pop).max(1);
        let worker_cap = if worker_budget_now <= 1 {
            1
        } else {
            let cap = 1.0 + ((worker_budget_now - 1) as f64 * ratio);
            cap.round().max(1.0) as usize
        };
        let min_parallel = if self.current_pop >= 2 && worker_budget_now >= 2 {
            2
        } else {
            1
        };
        GARampPlan {
            population_size: self.current_pop,
            sim_time_ms: sim_time_current,
            eval_ms: eval_ms_current,
            eval_neurons: eval_neurons_current,
            eval_conns: eval_conns_current,
            worker_cap: worker_cap.max(min_parallel).min(worker_budget_now),
        }
    }

    pub fn apply_plan_overrides(plan: &GARampPlan) {
        ga_set_eval_limits_override(plan.eval_ms, plan.eval_neurons, plan.eval_conns);
        ga_set_worker_limit_override(Some(plan.worker_cap));
    }

    pub fn note_generation_result(&mut self, success: bool) {
        if success {
            self.last_safe_pop = self.current_pop;
            if !self.ramp_locked && self.current_pop < self.max_pop {
                self.current_pop = self.current_pop.saturating_add(1).min(self.max_pop);
            }
        } else if !self.ramp_locked {
            self.ramp_locked = true;
            self.current_pop = self.last_safe_pop.max(1);
        }
    }
}

fn ga_ramp_start_population(max_pop: usize, worker_budget: usize) -> usize {
    let max_pop = max_pop.max(1);
    if let Some(start) = parse_env_usize("NM_GA_RAMP_START_POP") {
        return start.clamp(1, max_pop);
    }
    if max_pop <= 1 || worker_budget <= 1 {
        1
    } else {
        2.min(max_pop)
    }
}

impl GASearch {
    pub fn new(
        pop_size: usize,
        initial_config: &NetworkConfig,
        rng: &mut StdRng,
        distributed_node: Option<crate::distributed::DistributedNode>,
        is_restart: bool,
        existing_leaderboard: Vec<Individual>,
    ) -> Self {
        let mut population = Vec::with_capacity(pop_size);
        let leaderboard = existing_leaderboard;
        let mut best_fitness = 0.0;
        let mut best_config = None;

        if let Some(best) = leaderboard.first() {
            best_fitness = best.fitness;
            best_config = Some(best.config.clone());
        }

        let include_current = initial_config.use_morphology;
        if include_current {
            // Only seed with the current config for AARNN/morphology runs.
            population.push(Individual::new(initial_config.clone(), 0.0));
        }

        // Seed with leaderboard entries (if any), avoiding duplicates.
        for ind in &leaderboard {
            if population.len() >= pop_size {
                break;
            }
            if !population
                .iter()
                .any(|existing| existing.config == ind.config)
            {
                population.push(Individual::new(ind.config.clone(), 0.0));
            }
        }

        if population.len() > pop_size {
            population.truncate(pop_size);
        }

        let seed_configs: Vec<NetworkConfig> =
            population.iter().map(|ind| ind.config.clone()).collect();

        if population.len() < pop_size {
            if (is_restart || !leaderboard.is_empty()) && !seed_configs.is_empty() {
                // Fill the rest of the population by mutating seed configurations
                let mut attempts = 0;
                while population.len() < pop_size && attempts < pop_size * 20 {
                    attempts += 1;
                    let seed_idx = rng.random_range(0..seed_configs.len());
                    let mut cfg = seed_configs[seed_idx].clone();
                    // We use a slightly higher mutation rate for the initial population seeding to ensure diversity
                    mutate(&mut cfg, 0.3, rng);
                    if !population.iter().any(|ind| ind.config == cfg) {
                        population.push(Individual::new(cfg, 0.0));
                    }
                }
            } else {
                // Fresh search: completely randomize all individuals based on base structural parameters
                let mut attempts = 0;
                while population.len() < pop_size && attempts < pop_size * 20 {
                    attempts += 1;
                    let cfg = randomize_config(initial_config, rng);
                    if !population.iter().any(|ind| ind.config == cfg) {
                        population.push(Individual::new(cfg, 0.0));
                    }
                }
            }
        }

        // Fallback: if we still don't have a full population, just fill it (shouldn't happen with enough entropy)
        while population.len() < pop_size {
            population.push(Individual::new(initial_config.clone(), 0.0));
        }
        if initial_config.use_morphology {
            for ind in &mut population {
                ind.config.use_morphology = true;
            }
        }
        Self {
            population,
            leaderboard,
            generation: 0,
            best_fitness,
            best_config,
            distributed_node,
            current_eval_idx: 0,
            force_morphology: initial_config.use_morphology,
            inflight: Vec::new(),
        }
    }

    pub fn resize_population(
        &mut self,
        new_size: usize,
        base_cfg: &NetworkConfig,
        rng: &mut StdRng,
    ) {
        let new_size = new_size.max(1);
        if new_size == self.population.len() {
            return;
        }
        if new_size < self.population.len() {
            self.population.truncate(new_size);
            return;
        }

        let mut seed_configs: Vec<NetworkConfig> = self
            .population
            .iter()
            .map(|ind| ind.config.clone())
            .collect();
        if seed_configs.is_empty() {
            seed_configs.push(base_cfg.clone());
        }
        let mut attempts = 0usize;
        while self.population.len() < new_size && attempts < new_size * 20 {
            attempts += 1;
            let seed_idx = rng.random_range(0..seed_configs.len());
            let mut cfg = seed_configs[seed_idx].clone();
            mutate(&mut cfg, 0.3, rng);
            if !self.population.iter().any(|ind| ind.config == cfg) {
                self.population.push(Individual::new(cfg, 0.0));
            }
        }
        while self.population.len() < new_size {
            self.population.push(Individual::new(base_cfg.clone(), 0.0));
        }
        if self.force_morphology {
            for ind in &mut self.population {
                ind.config.use_morphology = true;
            }
        }
    }

    /// Evaluates the entire population, potentially using the cluster.
    pub async fn evaluate_population(
        &mut self,
        sim_time_ms: f64,
        seed: u64,
        status_tx: &std::sync::mpsc::Sender<GASearch>,
    ) {
        let _population_permit = acquire_population_permit().await;
        GA_ABORT_REQUESTED.store(false, Ordering::SeqCst);
        ga_clear_abort_reason();
        ga_clear_ui_lag_start();
        // Reset per-generation transient memory pressure flags so GA can recover parallelism.
        GA_EVAL_MEM_WARN.store(false, Ordering::Relaxed);
        ga_clear_mem_backoff();
        GA_PAUSED_MS.store(0, Ordering::Relaxed);
        GA_THROTTLE_MS.store(0, Ordering::SeqCst);
        GA_THROTTLED_MS.store(0, Ordering::Relaxed);
        GA_REMOTE_WAIT_MS.store(0, Ordering::Relaxed);
        let gen_start = Instant::now();
        if self.generation == 0 && self.current_eval_idx == 0 {
            nm_log!(
                "[info] GA tunables: growth[sat=0..2 win=20..10000 cooldown=0..10000 global=0..5000 spawn=0.01..2 split=1..256 layers=1..10] morpho[energy_win=100..30000 attract=0.05..5 sprout=0..2 contact=0.005..2 ambient=0..1 reson=0..1 decay=0..1 stabil=0..1 seg=0.1..2 out_cap=1..128 max_total=0..dynamic] aarnn_state[perceptual_lr=0..1 world_dim=2..32 sleep_cycle=1000..600000 theta_hz=0.5..12 thalamic_hz=0.5..20 neuromod_baseline=0..3 signal=all] io[target=auto or 0..9 source=auto or 0..9] bio[preset=RS FS IB CH LTS RZ TC P stp_u=0..1 tau_rec=10..5000 tau_facil=10..2000 ampa=1..50 nmda=10..300 gaba=1..50 nmda_ratio=0..1 gain=0.1..5 thresh_tau=10..1000 inc=0..5 min=-5..0 max=0..10 izh_refr=0..10 homeo_rate=0..20 tau=100..10000 gain=0..5 neuromod_gain=0.1..3] clumping[None HumanBrain FruitFly FruitFlyLarva ZebraFish NematodeWorm + spatial fine-tuning]."
            );
        }
        let (_, gen_start_free_mb, gen_start_rss_mb, _) = update_sys_cache();
        GA_CURRENT_POP_SIZE.store(self.population.len(), Ordering::Relaxed);
        let mut safety_event: Option<String> = None;
        let mut last_throttle_reason = String::new();
        let mut last_throttle_log = Instant::now() - Duration::from_secs(5);
        let mut apply_safety = |phase: &str| -> bool {
            match ga_safety_decision(gen_start.elapsed()) {
                GASafetyAction::None => false,
                GASafetyAction::Throttle(delay, reason) => {
                    ga_request_throttle(delay.as_millis() as u64);
                    if last_throttle_log.elapsed() > Duration::from_secs(2)
                        || reason != last_throttle_reason
                    {
                        nm_log!("[info] GA throttling ({}) during {}.", reason, phase);
                        last_throttle_log = Instant::now();
                        last_throttle_reason = reason;
                    }
                    false
                }
                GASafetyAction::Abort(reason) => {
                    nm_err!("[warn] GA abort triggered by {} during {}.", reason, phase);
                    ga_request_abort(&reason);
                    if safety_event.is_none() {
                        safety_event = Some(reason);
                    }
                    true
                }
            }
        };
        self.current_eval_idx = 0;
        GA_COMPLETED_EVALS.store(0, Ordering::Relaxed);
        let pop_size = self.population.len();
        let stall_timeout = ga_stall_timeout();
        let remote_eval_timeout = ga_remote_eval_timeout();

        // Distributed evaluation with load-balanced scheduling.
        type GaClient = crate::distributed::proto::distributed_neuromorphic_client::DistributedNeuromorphicClient<tonic::transport::Channel>;
        struct PeerEval {
            id: String,
            client: Option<GaClient>,
            capacity: f32,
            busy: bool,
            pacing: bool,
            max_inflight: usize,
        }

        enum EvalOutcome {
            Ok {
                idx: usize,
                fitness: f64,
                peer_id: String,
            },
            Err {
                idx: usize,
                peer_id: String,
                reason: String,
            },
        }

        let mut peer_pool: Vec<PeerEval> = Vec::new();
        if let Some(dist) = &self.distributed_node {
            let (is_orchestrator, peers, clients, nodes) = {
                let state = dist.state.read().await;
                (
                    state.is_orchestrator,
                    state.peers.keys().cloned().collect::<Vec<_>>(),
                    state.clients.clone(),
                    state.nodes.clone(),
                )
            };
            if is_orchestrator {
                for peer_id in peers.iter() {
                    let client = match clients.get(peer_id).cloned() {
                        Some(c) => c,
                        None => continue,
                    };
                    let (capacity, busy, pacing) = nodes
                        .get(peer_id)
                        .and_then(|n| n.resources.as_ref())
                        .map(|r| (r.capacity_score.max(0.1), r.ga_evaluating, r.ga_pacing))
                        .unwrap_or((1.0, false, false));
                    peer_pool.push(PeerEval {
                        id: peer_id.clone(),
                        client: Some(client),
                        capacity,
                        busy,
                        pacing,
                        max_inflight: 1,
                    });
                }
            }
        }

        let remote_orchs = ga_remote_orchestrators();
        if !remote_orchs.is_empty() {
            for addr in remote_orchs {
                let label = addr
                    .trim_start_matches("http://")
                    .trim_start_matches("https://");
                let peer_id = format!("orch@{}", label);
                let client_res =
                    tokio::time::timeout(Duration::from_secs(3), GaClient::connect(addr.clone()))
                        .await;
                let mut client = match client_res {
                    Ok(Ok(c)) => c,
                    Ok(Err(e)) => {
                        nm_err!(
                            "[warn] GA remote orchestrator connect failed ({}): {}",
                            addr,
                            e
                        );
                        continue;
                    }
                    Err(_) => {
                        nm_err!("[warn] GA remote orchestrator connect timeout ({})", addr);
                        continue;
                    }
                };

                let mut capacity = 1.0f32;
                let mut busy = false;
                let mut pacing = false;
                let mut max_inflight = 1usize;
                let status_res = tokio::time::timeout(
                    Duration::from_secs(3),
                    client.get_system_status(crate::distributed::proto::StatusRequest {}),
                )
                .await;
                match status_res {
                    Ok(Ok(resp)) => {
                        let status = resp.into_inner();
                        let mut total_capacity = 0.0f32;
                        let mut node_count = 0usize;
                        let mut busy_count = 0usize;
                        let mut pacing_count = 0usize;
                        for node in status.nodes {
                            if let Some(res) = node.resources {
                                node_count += 1;
                                total_capacity += res.capacity_score.max(0.1);
                                if res.ga_evaluating {
                                    busy_count += 1;
                                }
                                if res.ga_pacing {
                                    pacing_count += 1;
                                }
                            }
                        }
                        if node_count > 0 {
                            capacity = total_capacity.max(1.0);
                            let available = node_count.saturating_sub(pacing_count).max(1);
                            max_inflight = available;
                            busy = busy_count >= max_inflight;
                            pacing = pacing_count >= node_count;
                        }
                    }
                    Ok(Err(e)) => {
                        nm_err!(
                            "[warn] GA remote orchestrator status failed ({}): {}",
                            addr,
                            e
                        );
                    }
                    Err(_) => {
                        nm_err!("[warn] GA remote orchestrator status timeout ({})", addr);
                    }
                }

                peer_pool.push(PeerEval {
                    id: peer_id,
                    client: Some(client),
                    capacity,
                    busy,
                    pacing,
                    max_inflight: max_inflight.max(1),
                });
            }
        }

        if peer_pool.iter().any(|peer| peer.client.is_some()) {
            let mut join_set = tokio::task::JoinSet::new();
            let mut done = vec![false; pop_size];
            let mut last_progress = std::time::Instant::now();
            let mut inflight: HashSet<usize> = HashSet::new();
            let mut inflight_by_peer: HashMap<String, usize> = HashMap::new();
            let mut inflight_owner: HashMap<usize, String> = HashMap::new();
            let mut pending: VecDeque<usize> = (0..pop_size).collect();

            let schedule_next =
                |population: &Vec<Individual>,
                 pending: &mut VecDeque<usize>,
                 peer_pool: &Vec<PeerEval>,
                 inflight_by_peer: &mut HashMap<String, usize>,
                 inflight_owner: &mut HashMap<usize, String>,
                 inflight: &mut HashSet<usize>,
                 join_set: &mut tokio::task::JoinSet<EvalOutcome>| {
                    loop {
                        if pending.is_empty() {
                            break;
                        }
                        let mut best: Option<(usize, f32)> = None;
                        let mut fallback: Option<(usize, f32)> = None;
                        for (idx, peer) in peer_pool.iter().enumerate() {
                            if peer.client.is_none() {
                                continue;
                            }
                            let inflight_count = *inflight_by_peer.get(&peer.id).unwrap_or(&0);
                            if inflight_count >= peer.max_inflight {
                                continue;
                            }
                            let score = peer.capacity / (1.0 + inflight_count as f32);
                            if !peer.busy
                                && !peer.pacing
                                && best.map(|(_, s)| score > s).unwrap_or(true)
                            {
                                best = Some((idx, score));
                            }
                            if fallback.map(|(_, s)| score > s).unwrap_or(true) {
                                fallback = Some((idx, score));
                            }
                        }
                        let best = if best.is_none() && inflight.is_empty() {
                            fallback
                        } else {
                            best
                        };
                        let Some((peer_idx, _)) = best else {
                            break;
                        };
                        let idx = pending.pop_front().unwrap();
                        let peer = &peer_pool[peer_idx];
                        inflight.insert(idx);
                        *inflight_by_peer.entry(peer.id.clone()).or_insert(0) += 1;
                        inflight_owner.insert(idx, peer.id.clone());

                        let config_json = serde_json::to_string(&population[idx].config).unwrap();
                        let seed = seed + idx as u64;
                        let peer_id = peer.id.clone();
                        if let Some(mut client) = peer.client.clone() {
                            join_set.spawn(async move {
                                let req = crate::distributed::proto::GaEvaluationRequest {
                                    config_json,
                                    sim_time_ms,
                                    seed,
                                };
                                let res = tokio::time::timeout(
                                    remote_eval_timeout,
                                    client.run_ga_evaluation(req),
                                )
                                .await;
                                match res {
                                    Ok(Ok(resp)) => EvalOutcome::Ok {
                                        idx,
                                        fitness: resp.into_inner().fitness,
                                        peer_id,
                                    },
                                    Ok(Err(e)) => EvalOutcome::Err {
                                        idx,
                                        peer_id,
                                        reason: format!("rpc: {}", e),
                                    },
                                    Err(_) => EvalOutcome::Err {
                                        idx,
                                        peer_id,
                                        reason: "timeout".to_string(),
                                    },
                                }
                            });
                        }
                    }
                };

            // Initial scheduling
            schedule_next(
                &self.population,
                &mut pending,
                &peer_pool,
                &mut inflight_by_peer,
                &mut inflight_owner,
                &mut inflight,
                &mut join_set,
            );
            for (idx, ind) in self.population.iter_mut().enumerate() {
                ind.evaluating_node = inflight_owner.get(&idx).cloned();
            }
            self.inflight = inflight.iter().copied().collect();
            let _ = status_tx.send(self.clone());

            while self.current_eval_idx < pop_size {
                if apply_safety("distributed") {
                    break;
                }

                let has_peers = peer_pool.iter().any(|peer| peer.client.is_some());
                if !has_peers && inflight.is_empty() && !pending.is_empty() {
                    nm_err!(
                        "[warn] GA distributed peers unavailable; evaluating {} individuals locally.",
                        pending.len()
                    );
                    let pending_indices: Vec<usize> = pending.drain(..).collect();
                    for idx in pending_indices {
                        if apply_safety("local_fallback") {
                            break;
                        }
                        self.population[idx].evaluating_node = Some("local".to_string());
                        self.inflight = vec![idx];
                        let _ = status_tx.send(self.clone());
                        let cfg = self.population[idx].config.clone();
                        let eval_seed = seed + idx as u64;
                        let _permit = acquire_evaluation_permit().await;
                        let fitness = match tokio::task::spawn_blocking(move || {
                            Self::evaluate_individual(&cfg, sim_time_ms, eval_seed)
                        })
                        .await
                        {
                            Ok(f) => f,
                            Err(e) => {
                                nm_err!("[error] Local GA fallback task failed: {}", e);
                                0.0
                            }
                        };
                        self.population[idx].fitness = fitness;
                        if !done[idx] {
                            done[idx] = true;
                            self.current_eval_idx += 1;
                            GA_COMPLETED_EVALS.store(self.current_eval_idx, Ordering::Relaxed);
                            last_progress = std::time::Instant::now();
                        }
                        self.inflight.clear();
                        if self.current_eval_idx % 2 == 0 || self.current_eval_idx == pop_size {
                            let _ = status_tx.send(self.clone());
                        }
                    }
                    break;
                }

                let tick_start = std::time::Instant::now();
                let join_res = tokio::time::timeout(
                    Duration::from_millis(GA_PROGRESS_TICK_MS),
                    join_set.join_next(),
                )
                .await;
                let tick_elapsed = tick_start.elapsed();
                if !inflight.is_empty() {
                    GA_REMOTE_WAIT_MS.fetch_add(tick_elapsed.as_millis() as u64, Ordering::Relaxed);
                }
                match join_res {
                    Ok(Some(res)) => match res {
                        Ok(EvalOutcome::Ok {
                            idx,
                            fitness,
                            peer_id,
                        }) => {
                            self.population[idx].fitness = fitness;
                            if !done[idx] {
                                done[idx] = true;
                                self.current_eval_idx += 1;
                                GA_COMPLETED_EVALS.store(self.current_eval_idx, Ordering::Relaxed);
                                last_progress = std::time::Instant::now();
                            }
                            inflight.remove(&idx);
                            inflight_owner.remove(&idx);
                            if let Some(count) = inflight_by_peer.get_mut(&peer_id) {
                                *count = count.saturating_sub(1);
                            }
                            schedule_next(
                                &self.population,
                                &mut pending,
                                &peer_pool,
                                &mut inflight_by_peer,
                                &mut inflight_owner,
                                &mut inflight,
                                &mut join_set,
                            );
                            for (idx, ind) in self.population.iter_mut().enumerate() {
                                ind.evaluating_node = inflight_owner.get(&idx).cloned();
                            }
                            self.inflight = inflight.iter().copied().collect();

                            // Throttle status updates
                            if self.current_eval_idx % 2 == 0 || self.current_eval_idx == pop_size {
                                let _ = status_tx.send(self.clone());
                            }
                        }
                        Ok(EvalOutcome::Err {
                            idx,
                            peer_id,
                            reason,
                        }) => {
                            nm_err!(
                                "[warn] Distributed GA evaluation failed for peer {} ({}); requeueing.",
                                peer_id,
                                reason
                            );
                            if let Some(peer) = peer_pool.iter_mut().find(|peer| peer.id == peer_id)
                            {
                                peer.client = None;
                            }
                            inflight.remove(&idx);
                            inflight_owner.remove(&idx);
                            if let Some(count) = inflight_by_peer.get_mut(&peer_id) {
                                *count = count.saturating_sub(1);
                            }
                            last_progress = std::time::Instant::now();
                            pending.push_back(idx);
                            schedule_next(
                                &self.population,
                                &mut pending,
                                &peer_pool,
                                &mut inflight_by_peer,
                                &mut inflight_owner,
                                &mut inflight,
                                &mut join_set,
                            );
                            for (idx, ind) in self.population.iter_mut().enumerate() {
                                ind.evaluating_node = inflight_owner.get(&idx).cloned();
                            }
                            self.inflight = inflight.iter().copied().collect();
                            if self.current_eval_idx % 2 == 0 || self.current_eval_idx == pop_size {
                                let _ = status_tx.send(self.clone());
                            }
                        }
                        Err(e) => {
                            nm_err!("[warn] Distributed GA evaluation task failed: {}", e);
                        }
                    },
                    Ok(None) => break,
                    Err(_) => {
                        if last_progress.elapsed() > stall_timeout {
                            nm_err!(
                                "[warn] Distributed GA evaluation stalled for {:?}; marking remaining individuals as failed.",
                                stall_timeout
                            );
                            break;
                        }
                    }
                }
            }

            if self.current_eval_idx < pop_size {
                for (i, ind) in self.population.iter_mut().enumerate() {
                    if !done[i] {
                        ind.fitness = 0.0;
                    }
                }
                self.current_eval_idx = pop_size;
                GA_COMPLETED_EVALS.store(self.current_eval_idx, Ordering::Relaxed);
                self.inflight.clear();
                let _ = status_tx.send(self.clone());
            } else if last_progress.elapsed() > stall_timeout {
                nm_err!(
                    "[warn] Distributed GA evaluation experienced extended stalls; consider reducing population or sim time."
                );
            }
            if let Some(reason) = safety_event {
                ga_record_safety_event(&reason);
            } else {
                ga_record_safe_generation();
            }
            let paused_ms = GA_PAUSED_MS.load(Ordering::Relaxed) as u64;
            let remote_wait_ms = GA_REMOTE_WAIT_MS.load(Ordering::Relaxed);
            let effective_elapsed = gen_start
                .elapsed()
                .saturating_sub(Duration::from_millis(paused_ms + remote_wait_ms));
            if !GA_ABORT_REQUESTED.load(Ordering::Relaxed) && self.current_eval_idx >= pop_size {
                ga_record_individual_timing(pop_size, effective_elapsed);
            }
            let (_, end_free_mb, end_rss_mb, _) = update_sys_cache();
            let (base_rss, last_rss, max_rss) = ga_mem_tracker_snapshot();
            nm_log!(
                "[info] GA gen memory: start_rss {:?}MB start_free {:?}MB end_rss {:?}MB end_free {:?}MB base_rss {:?}MB last_rss {:?}MB max_rss {:?}MB.",
                gen_start_rss_mb,
                gen_start_free_mb,
                end_rss_mb,
                end_free_mb,
                base_rss,
                last_rss,
                max_rss
            );
            if let (Some(start), Some(end)) = (gen_start_rss_mb, end_rss_mb) {
                if end > start.saturating_add(512u64) {
                    nm_err!(
                        "[warn] GA gen memory increased by {}MB and did not drop after generation. Possible leak.",
                        end - start
                    );
                }
            }
            return;
        }

        // Local parallel evaluation using a worker pool to track progress
        let (fit_tx, fit_rx) = std::sync::mpsc::channel::<(usize, f64)>();
        let configs: Vec<_> = self
            .population
            .iter()
            .enumerate()
            .map(|(i, ind)| (i, ind.config.clone()))
            .collect();
        let morpho_growth_active = configs.iter().any(|(_, cfg)| {
            cfg.use_morphology && (cfg.growth_enabled || cfg.morpho_growth_enabled)
        });
        let inflight = Arc::new(Mutex::new(HashSet::<usize>::new()));
        let inflight_worker = Arc::clone(&inflight);
        let mut last_hb_log = Instant::now() - Duration::from_secs(15);

        // Assign "local" to all individuals for UI feedback
        for ind in self.population.iter_mut() {
            ind.evaluating_node = Some("local".to_string());
        }
        self.inflight.clear();
        let _ = status_tx.send(self.clone());

        let worker_handle = std::thread::spawn(move || {
            // Use a dedicated thread pool for GA to avoid saturating all cores
            // and leaving some for the UI and main simulation.
            let mut pending = configs;
            let mut last_split_log = Instant::now() - Duration::from_secs(10);
            let mut last_floor_log = Instant::now() - Duration::from_secs(10);
            let mut last_runtime_log = Instant::now() - Duration::from_secs(10);
            let mut last_morph_threads = 0usize;
            while !pending.is_empty() {
                if GA_ABORT_REQUESTED.load(Ordering::SeqCst) {
                    break;
                }
                let mut num_threads = population_worker_threads(pending.len()).max(1);
                let mut batch_len = (num_threads * 2).min(pending.len()).max(1);
                let mut healthy_for_parallel = false;
                let min_parallel = ga_min_parallel_evals().min(pending.len()).max(1);
                if morpho_growth_active {
                    num_threads = ga_morph_growth_parallel_threads(pending.len()).max(1);
                    batch_len = if num_threads > 1 {
                        (num_threads * 2).min(pending.len()).max(1)
                    } else {
                        1
                    };
                    if num_threads != last_morph_threads {
                        if num_threads == 1 {
                            nm_log!(
                                "[info] GA batching: morpho growth active; sequential evals until resources recover."
                            );
                        } else {
                            nm_log!(
                                "[info] GA batching: morpho growth active; scaling to {} workers.",
                                num_threads
                            );
                        }
                        last_morph_threads = num_threads;
                    }
                }
                let split_for_pressure = ga_should_split_batches();
                if split_for_pressure {
                    num_threads = 1;
                    batch_len = 1;
                    last_morph_threads = 1;
                    if last_split_log.elapsed() > Duration::from_secs(5) {
                        nm_log!(
                            "[info] GA batching: memory pressure detected; running sequential batches."
                        );
                        last_split_log = Instant::now();
                    }
                } else {
                    healthy_for_parallel = ga_resources_healthy_for_parallelism();
                    if min_parallel > 1 && num_threads < min_parallel && healthy_for_parallel {
                        num_threads = min_parallel;
                        batch_len = (num_threads * 2).min(pending.len()).max(min_parallel);
                        if last_floor_log.elapsed() > Duration::from_secs(5) {
                            nm_log!(
                                "[info] GA batching: healthy resources; raising workers to {} (minimum parallel floor).",
                                num_threads
                            );
                            last_floor_log = Instant::now();
                        }
                    }
                }
                if last_runtime_log.elapsed() > Duration::from_secs(5) {
                    let snapshot = ga_safety_snapshot();
                    let override_limit = ga_worker_limit_override().unwrap_or(0);
                    let auto_backoff = GA_WORKER_LIMIT_AUTO.load(Ordering::Relaxed);
                    let mem_warn = GA_EVAL_MEM_WARN.load(Ordering::Relaxed);
                    let sem_waiters = GA_SEM_WAITERS.load(Ordering::Relaxed);
                    let thermal_waiters = GA_THERMAL_WAITERS.load(Ordering::Relaxed);
                    nm_log!(
                        "[info] GA runtime: pending {} workers {} batch {} min_parallel {} override {} auto_backoff {} split {} healthy {} mem_warn {} sem_wait {} thermal_wait {} cpu {:?}% rss {:?}MB free {:?}MB temp {:?}C ui {:?}ms.",
                        pending.len(),
                        num_threads,
                        batch_len,
                        min_parallel,
                        override_limit,
                        auto_backoff,
                        split_for_pressure,
                        healthy_for_parallel,
                        mem_warn,
                        sem_waiters,
                        thermal_waiters,
                        snapshot.cpu_usage_pct,
                        snapshot.proc_rss_mb,
                        snapshot.mem_free_mb,
                        snapshot.temp_c,
                        snapshot.ui_frame_ms
                    );
                    last_runtime_log = Instant::now();
                }
                let start = pending.len().saturating_sub(batch_len);
                let batch: Vec<_> = pending.split_off(start);
                let inflight_batch = Arc::clone(&inflight_worker);
                #[cfg_attr(not(feature = "core_affinity"), allow(unused_mut))]
                let mut builder = rayon::ThreadPoolBuilder::new()
                    .num_threads(num_threads)
                    .thread_name(|i| format!("ga-worker-{}", i));
                #[cfg(feature = "core_affinity")]
                if let Some(core_ids) = ga_affinity_core_ids() {
                    ga_record_core_usage(&core_ids);
                    let ids = core_ids.clone();
                    builder = builder.start_handler(move |idx| {
                        let core = ids[idx % ids.len()];
                        let _ = core_affinity::set_for_current(core);
                    });
                }
                let pool_result = builder.build();
                if let Ok(pool) = pool_result {
                    pool.install(|| {
                        use rayon::prelude::*;
                        batch.into_par_iter().for_each(|(i, cfg)| {
                            if GA_ABORT_REQUESTED.load(Ordering::SeqCst) {
                                return;
                            }
                            {
                                let mut guard =
                                    inflight_batch.lock().expect("GA inflight lock poisoned");
                                guard.insert(i);
                            }
                            thermal_wait_blocking("evaluation");
                            if GA_ABORT_REQUESTED.load(Ordering::SeqCst) {
                                let mut guard =
                                    inflight_batch.lock().expect("GA inflight lock poisoned");
                                guard.remove(&i);
                                return;
                            }
                            let fitness =
                                Self::evaluate_individual(&cfg, sim_time_ms, seed + i as u64);
                            let _ = fit_tx.send((i, fitness));
                            let mut guard =
                                inflight_batch.lock().expect("GA inflight lock poisoned");
                            guard.remove(&i);
                        });
                    });
                } else {
                    // Fallback to global pool if custom pool creation fails
                    use rayon::prelude::*;
                    batch.into_par_iter().for_each(|(i, cfg)| {
                        if GA_ABORT_REQUESTED.load(Ordering::SeqCst) {
                            return;
                        }
                        {
                            let mut guard =
                                inflight_batch.lock().expect("GA inflight lock poisoned");
                            guard.insert(i);
                        }
                        thermal_wait_blocking("evaluation");
                        if GA_ABORT_REQUESTED.load(Ordering::SeqCst) {
                            let mut guard =
                                inflight_batch.lock().expect("GA inflight lock poisoned");
                            guard.remove(&i);
                            return;
                        }
                        let fitness = Self::evaluate_individual(&cfg, sim_time_ms, seed + i as u64);
                        let _ = fit_tx.send((i, fitness));
                        let mut guard = inflight_batch.lock().expect("GA inflight lock poisoned");
                        guard.remove(&i);
                    });
                }
            }
        });

        let mut done = vec![false; pop_size];
        let mut last_progress = std::time::Instant::now();
        let mut abort_after_loop = false;
        while self.current_eval_idx < pop_size {
            if apply_safety("local") {
                abort_after_loop = true;
                break;
            }
            match fit_rx.recv_timeout(Duration::from_millis(GA_PROGRESS_TICK_MS)) {
                Ok((idx, fitness)) => {
                    self.population[idx].fitness = fitness;
                    if !done[idx] {
                        done[idx] = true;
                        self.current_eval_idx += 1;
                        GA_COMPLETED_EVALS.store(self.current_eval_idx, Ordering::Relaxed);
                        last_progress = std::time::Instant::now();
                    }
                    if let Ok(guard) = inflight.lock() {
                        self.inflight = guard.iter().copied().collect();
                    }

                    // Throttle status updates to avoid overwhelming the UI thread
                    if self.current_eval_idx % 2 == 0 || self.current_eval_idx == pop_size {
                        let _ = status_tx.send(self.clone());
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if last_progress.elapsed() > stall_timeout {
                        nm_err!(
                            "[warn] Local GA evaluation stalled for {:?}; marking remaining individuals as failed.",
                            stall_timeout
                        );
                        GA_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                        abort_after_loop = true;
                        break;
                    }
                    if last_hb_log.elapsed() > Duration::from_secs(10)
                        && last_progress.elapsed() > Duration::from_secs(10)
                    {
                        let inflight_count = inflight.lock().map(|g| g.len()).unwrap_or(0);
                        let snapshot = ga_safety_snapshot();
                        let workers = population_worker_threads(pop_size).max(1);
                        let worker_override = ga_worker_limit_override().unwrap_or(0);
                        let auto_backoff = GA_WORKER_LIMIT_AUTO.load(Ordering::Relaxed);
                        let mem_warn = GA_EVAL_MEM_WARN.load(Ordering::Relaxed);
                        nm_log!(
                            "[info] GA heartbeat: gen_eval {}/{} inflight {} workers {} override {} auto_backoff {} mem_warn {} rss {:?}MB free {:?}MB.",
                            self.current_eval_idx,
                            pop_size,
                            inflight_count,
                            workers,
                            worker_override,
                            auto_backoff,
                            mem_warn,
                            snapshot.proc_rss_mb,
                            snapshot.mem_free_mb
                        );
                        last_hb_log = Instant::now();
                    }
                    if let Ok(guard) = inflight.lock() {
                        self.inflight = guard.iter().copied().collect();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    nm_err!(
                        "[warn] Local GA evaluation worker channel closed early; marking remaining individuals as failed."
                    );
                    GA_ABORT_REQUESTED.store(true, Ordering::SeqCst);
                    abort_after_loop = true;
                    break;
                }
            }
        }

        let mut worker_handle = Some(worker_handle);
        if self.current_eval_idx < pop_size {
            if abort_after_loop {
                if let Some(handle) = worker_handle.take() {
                    let _ = handle.join();
                }
            }
            for (i, ind) in self.population.iter_mut().enumerate() {
                if !done[i] {
                    ind.fitness = 0.0;
                }
            }
            self.current_eval_idx = pop_size;
            GA_COMPLETED_EVALS.store(self.current_eval_idx, Ordering::Relaxed);
            self.inflight.clear();
            let _ = status_tx.send(self.clone());
        } else if last_progress.elapsed() > stall_timeout {
            nm_err!(
                "[warn] Local GA evaluation experienced extended stalls; consider reducing population or sim time."
            );
        }
        let paused_ms = GA_PAUSED_MS.load(Ordering::Relaxed) as u64;
        let remote_wait_ms = GA_REMOTE_WAIT_MS.load(Ordering::Relaxed);
        let effective_elapsed = gen_start
            .elapsed()
            .saturating_sub(Duration::from_millis(paused_ms + remote_wait_ms));
        if !GA_ABORT_REQUESTED.load(Ordering::Relaxed) && self.current_eval_idx >= pop_size {
            ga_record_individual_timing(pop_size, effective_elapsed);
        }
        if let Some(reason) = safety_event {
            ga_record_safety_event(&reason);
        } else {
            ga_record_safe_generation();
        }
        let (_, end_free_mb, end_rss_mb, _) = update_sys_cache();
        let (base_rss, last_rss, max_rss) = ga_mem_tracker_snapshot();
        nm_log!(
            "[info] GA gen memory: start_rss {:?}MB start_free {:?}MB end_rss {:?}MB end_free {:?}MB base_rss {:?}MB last_rss {:?}MB max_rss {:?}MB.",
            gen_start_rss_mb,
            gen_start_free_mb,
            end_rss_mb,
            end_free_mb,
            base_rss,
            last_rss,
            max_rss
        );
        if let (Some(start), Some(end)) = (gen_start_rss_mb, end_rss_mb) {
            if end > start.saturating_add(512u64) {
                nm_err!(
                    "[warn] GA gen memory increased by {}MB and did not drop after generation. Possible leak.",
                    end - start
                );
            }
        }
        if let Some(handle) = worker_handle.take() {
            let _ = handle.join();
        }
    }

    /// Evaluates the fitness of an individual locally.
    pub fn evaluate_individual(config: &NetworkConfig, sim_time_ms: f64, seed: u64) -> f64 {
        struct EvalGuard;
        impl Drop for EvalGuard {
            fn drop(&mut self) {
                GA_ACTIVE_EVALS.fetch_sub(1, Ordering::SeqCst);
            }
        }
        GA_ACTIVE_EVALS.fetch_add(1, Ordering::SeqCst);
        let _eval_guard = EvalGuard;

        if !ga_wait_for_eval_mem("eval_init") {
            return 0.0;
        }
        let eval_start_snapshot = ga_safety_snapshot();
        let eval_start_rss_mb = eval_start_snapshot.proc_rss_mb;
        let mut rng = StdRng::seed_from_u64(seed);

        let mut lif = crate::config::LIFParams::default();
        lif.dt = 1.0;
        let stdp = crate::config::STDPParams::default();

        let eval_config = config.clone();
        let max_eval_neurons = ga_max_eval_neurons();
        let max_eval_connections = ga_max_eval_connections();
        let max_eval_segments = ga_max_eval_segments();
        let est_layers = if eval_config.growth_enabled {
            eval_config.max_layers.max(eval_config.num_hidden_layers)
        } else {
            eval_config.num_hidden_layers
        };
        let hidden_per_layer = eval_config.num_hidden_per_layer_initial.max(1);
        let hidden_total = est_layers.saturating_mul(hidden_per_layer);
        let total_neurons_est = eval_config
            .num_sensory_neurons
            .saturating_add(eval_config.num_output_neurons)
            .saturating_add(hidden_total);
        let dense_in = hidden_per_layer.saturating_mul(eval_config.num_sensory_neurons);
        let dense_hh_fwd = hidden_per_layer
            .saturating_mul(hidden_per_layer)
            .saturating_mul(est_layers.saturating_sub(1));
        let dense_hh_bwd = dense_hh_fwd;
        let dense_hh_rec = hidden_per_layer
            .saturating_mul(hidden_per_layer)
            .saturating_mul(est_layers);
        let dense_out = hidden_per_layer.saturating_mul(eval_config.num_output_neurons);
        let dense_total_est = dense_in
            .saturating_add(dense_hh_fwd)
            .saturating_add(dense_hh_bwd)
            .saturating_add(dense_hh_rec)
            .saturating_add(dense_out);
        nm_log!(
            "[info] GA eval preflight: est_neurons {} est_dense_conns {} caps {:?}/{:?}/{:?} growth={} morpho_growth={}.",
            total_neurons_est,
            dense_total_est,
            max_eval_neurons,
            max_eval_connections,
            max_eval_segments,
            eval_config.growth_enabled,
            eval_config.morpho_growth_enabled
        );
        if let Some(max_neurons) = max_eval_neurons {
            if total_neurons_est > max_neurons {
                nm_err!(
                    "[warn] GA eval preflight exceeded neuron cap {} (estimated {}). Aborting individual.",
                    max_neurons,
                    total_neurons_est
                );
                return 0.0;
            }
        }
        if let Some(max_conns) = max_eval_connections {
            if dense_total_est > max_conns {
                nm_err!(
                    "[warn] GA eval preflight exceeded connection cap {} (estimated dense {}). Aborting individual.",
                    max_conns,
                    dense_total_est
                );
                return 0.0;
            }
        }

        // Use AARNN if morphology is requested, else LIF
        let neuron_model = if eval_config.use_morphology {
            sim::NeuronModel::Aarnn
        } else {
            sim::NeuronModel::Lif
        };
        let learning = if eval_config.use_morphology {
            sim::Learning::Aarnn
        } else {
            sim::Learning::Stdp
        };

        let mut runner = Runner::new(lif, stdp, eval_config.clone(), neuron_model, learning);
        #[cfg(feature = "opencl")]
        {
            let allow_opencl =
                ga_should_use_opencl() && GA_ACTIVE_EVALS.load(Ordering::SeqCst) <= 1;
            if !allow_opencl {
                // GA runs are highly parallel; disable OpenCL here to avoid shared-context buffer issues.
                if ga_should_use_opencl() {
                    nm_log!(
                        "[info] GA eval OpenCL disabled (active_evals={})",
                        GA_ACTIVE_EVALS.load(Ordering::SeqCst)
                    );
                }
                runner.cl = None;
                runner.clear_cl_buffers();
            } else {
                runner.cl = ga_thread_opencl_manager();
                runner.clear_cl_buffers();
            }
        }

        if !ga_eval_mem_guard(eval_start_rss_mb, "eval_post_init") {
            return 0.0;
        }
        let dt = lif.dt;
        let steps = (sim_time_ms / dt).round() as usize;
        let eval_start = Instant::now();
        let max_eval_ms = ga_max_eval_ms();
        let max_eval_neurons = ga_max_eval_neurons();
        let max_eval_connections = ga_max_eval_connections();
        let max_eval_segments = ga_max_eval_segments();
        let mut check_interval = DEFAULT_GA_EVAL_CHECK_INTERVAL_STEPS.max(1);
        if GA_EVAL_MEM_WARN.load(Ordering::Relaxed) {
            check_interval = 1;
        }
        if eval_start.elapsed() < Duration::from_millis(1) {
            nm_log!(
                "[info] GA eval start: seed {} steps {} max_eval_ms {:?} max_neurons {:?} max_conns {:?}.",
                seed,
                steps,
                max_eval_ms,
                max_eval_neurons,
                max_eval_connections
            );
        }

        let num_sensory = eval_config.num_sensory_neurons;
        let base_rate = 2.0_f64;
        let burst_rate = 25.0_f64;
        let base_spike_probability = base_rate * dt / 1000.0;
        let burst_spike_probability = burst_rate * dt / 1000.0;
        let mut groups: Vec<Vec<usize>> = vec![Vec::new(), Vec::new(), Vec::new()];
        for (i, idx) in (0..num_sensory).enumerate() {
            groups[i % 3].push(idx);
        }
        let chunk = (steps / 6).max(1);
        let schedule = [0usize, 1, 2, 0, 2, 1];
        let mut spk_buf = vec![0i8; num_sensory];

        let mut mem_logged = false;
        let mut mem_warn_streak = 0u32;
        let mut mem_warn_logged = false;
        let mut throttled_duration = Duration::ZERO;
        for t in 0..steps {
            let pat_idx = if t >= steps {
                schedule[schedule.len() - 1]
            } else {
                let sched_idx = (t / chunk).min(schedule.len() - 1);
                schedule[sched_idx]
            };
            for i in 0..num_sensory {
                spk_buf[i] = (rng.random::<f64>() < base_spike_probability) as i8;
            }
            for &i in &groups[pat_idx] {
                spk_buf[i] = (rng.random::<f64>() < burst_spike_probability) as i8;
            }
            // Runner::step takes Option<&[i8]>
            runner.step(Some(&spk_buf));

            let warn_interval = if GA_EVAL_MEM_WARN.load(Ordering::Relaxed)
                || GA_ABORT_REQUESTED.load(Ordering::Relaxed)
            {
                1
            } else {
                check_interval
            };
            if t % warn_interval == 0 {
                throttled_duration += ga_throttle_if_needed();
                if GA_ABORT_REQUESTED.load(Ordering::Relaxed) {
                    return 0.0;
                }
                if ga_check_mem_pressure("eval_loop") {
                    return 0.0;
                }
                if warn_interval == 1 {
                    ga_adjust_eval_limits_on_pressure(&ga_safety_snapshot());
                }
                if !ga_eval_mem_guard(eval_start_rss_mb, "eval_loop") {
                    return 0.0;
                }
                if let (Some(start), Some(cur)) =
                    (eval_start_rss_mb, ga_safety_snapshot().proc_rss_mb)
                {
                    let growth = cur.saturating_sub(start);
                    let h = ga_heuristics()
                        .lock()
                        .expect("GA heuristics lock poisoned")
                        .clone();
                    let mut eval_warn = h.mem_rss_growth_warn_mb;
                    let mut eval_abort = h.mem_rss_growth_abort_mb;
                    if let Some(total_mb) = ga_safety_snapshot().total_mem_mb {
                        let pct_warn = ((total_mb as f32) * 0.08).ceil() as u64;
                        let pct_abort = ((total_mb as f32) * 0.12).ceil() as u64;
                        eval_warn = eval_warn.min(pct_warn.max(512));
                        eval_abort = eval_abort.min(pct_abort.max(eval_warn + 256));
                    }
                    if growth >= eval_warn {
                        mem_warn_streak = mem_warn_streak.saturating_add(1);
                        if !mem_warn_logged {
                            let segs = if eval_config.use_morphology {
                                ga_morph_segment_count(&runner)
                            } else {
                                0
                            };
                            nm_err!(
                                "[warn] GA eval mem warn streak: growth {}MB warn {}MB abort {}MB segments {}.",
                                growth,
                                eval_warn,
                                eval_abort,
                                segs
                            );
                            mem_warn_logged = true;
                        }
                        if mem_warn_streak >= 25 {
                            nm_err!(
                                "[warn] GA eval abort: sustained RSS growth {}MB >= warn {}MB for {} checks.",
                                growth,
                                eval_warn,
                                mem_warn_streak
                            );
                            ga_request_abort("mem_eval_guard_warn_abort");
                            return 0.0;
                        }
                    } else {
                        mem_warn_streak = 0;
                    }
                }
                if !mem_logged {
                    let snapshot = ga_safety_snapshot();
                    let h = ga_heuristics()
                        .lock()
                        .expect("GA heuristics lock poisoned")
                        .clone();
                    let growth_from_start = eval_start_rss_mb.and_then(|start| {
                        snapshot.proc_rss_mb.map(|cur| cur.saturating_sub(start))
                    });
                    if growth_from_start.map_or(false, |g| g >= h.mem_rss_growth_warn_mb) {
                        let total_neurons = runner.total_neurons();
                        let total_conn = runner.connection_counts().iter().sum::<usize>()
                            + runner.output_connection_count();
                        let spike_bytes = (steps as u64)
                            .saturating_mul(eval_config.num_sensory_neurons as u64)
                            .saturating_mul(std::mem::size_of::<i8>() as u64);
                        let segs = if eval_config.use_morphology {
                            ga_morph_segment_count(&runner)
                        } else {
                            0
                        };
                        nm_err!(
                            "[warn] GA eval mem spike: seed {} steps {} sensors {} spike_bytes {}MB neurons {} conns {} segments {} rss {:?}MB free {:?}MB.",
                            seed,
                            steps,
                            eval_config.num_sensory_neurons,
                            spike_bytes / 1024 / 1024,
                            total_neurons,
                            total_conn,
                            segs,
                            snapshot.proc_rss_mb,
                            snapshot.mem_free_mb
                        );
                        mem_logged = true;
                    }
                }
                // No extra throttle here, we already did it at the start of the warn_interval block.
                if let Some(max_ms) = max_eval_ms {
                    let active_elapsed = eval_start.elapsed().saturating_sub(throttled_duration);
                    if active_elapsed > Duration::from_millis(max_ms) {
                        nm_err!(
                            "[warn] GA eval exceeded {}ms active time ({}ms wall) at step {} (seed {}). Aborting individual.",
                            max_ms,
                            eval_start.elapsed().as_millis(),
                            t,
                            seed
                        );
                        return 0.0;
                    }
                }
                if let Some(max_neurons) = max_eval_neurons {
                    let total_neurons = runner.total_neurons();
                    if total_neurons > max_neurons {
                        nm_err!(
                            "[warn] GA eval exceeded neuron cap {} (got {}) at step {}. Aborting individual.",
                            max_neurons,
                            total_neurons,
                            t
                        );
                        return 0.0;
                    }
                }
                if let Some(max_conns) = max_eval_connections {
                    let total_conn = runner.connection_counts().iter().sum::<usize>()
                        + runner.output_connection_count();
                    if total_conn > max_conns {
                        nm_err!(
                            "[warn] GA eval exceeded connection cap {} (got {}) at step {}. Aborting individual.",
                            max_conns,
                            total_conn,
                            t
                        );
                        return 0.0;
                    }
                }
                if let Some(max_segments) = max_eval_segments {
                    if eval_config.use_morphology {
                        let total_segments = ga_morph_segment_count(&runner);
                        if total_segments > max_segments {
                            nm_err!(
                                "[warn] GA eval exceeded segment cap {} (got {}) at step {}. Aborting individual.",
                                max_segments,
                                total_segments,
                                t
                            );
                            return 0.0;
                        }
                    }
                }
            }
        }

        let (lt, total) = runner.calculate_longterm_connections();
        let stability_ratio = if total > 0 {
            lt as f64 / total as f64
        } else {
            0.0
        };

        let layer_sizes: Vec<usize> = (0..runner.net.num_hidden_layers)
            .map(|l| runner.layer_size(l))
            .collect();
        let layer_count = layer_sizes.iter().filter(|&&s| s > 0).count();

        let total_conn =
            runner.connection_counts().iter().sum::<usize>() + runner.output_connection_count();
        let mut total_possible: usize = 0;
        if !layer_sizes.is_empty() {
            let h0 = layer_sizes[0];
            total_possible += h0 * runner.net.num_sensory_neurons;
            for l in 0..layer_sizes.len().saturating_sub(1) {
                total_possible += layer_sizes[l] * layer_sizes[l + 1]; // fwd
                total_possible += layer_sizes[l] * layer_sizes[l + 1]; // bwd
            }
            for &h in &layer_sizes {
                total_possible += h * h; // rec
            }
            if runner.net.num_output_neurons > 0 {
                total_possible +=
                    runner.net.num_output_neurons * layer_sizes[layer_sizes.len() - 1];
            }
        }

        let mut max_neuron_ratio = 0.0f64;
        if !layer_sizes.is_empty() {
            let eps = 1.0e-8f64;
            let s_count = runner.net.num_sensory_neurons;
            let o_count = runner.net.num_output_neurons;
            let (in_l, out_l) = runner.get_io_layers();

            let mut sensory_out = vec![0usize; s_count];
            let mut hidden_in: Vec<Vec<usize>> =
                layer_sizes.iter().map(|&n| vec![0usize; n]).collect();
            let mut hidden_out: Vec<Vec<usize>> =
                layer_sizes.iter().map(|&n| vec![0usize; n]).collect();
            let mut output_in = vec![0usize; o_count];

            if in_l < layer_sizes.len() {
                for j in 0..layer_sizes[in_l] {
                    for i in 0..s_count.min(runner.w_in.ncols()) {
                        if j < runner.w_in.nrows() && runner.w_in[(j, i)].abs() > eps {
                            sensory_out[i] += 1;
                            hidden_in[in_l][j] += 1;
                        }
                    }
                }
            }

            for l in 0..layer_sizes.len().saturating_sub(1) {
                if let Some(w) = runner.w_hh_fwd.get(l) {
                    for j in 0..layer_sizes[l + 1].min(w.nrows()) {
                        for i in 0..layer_sizes[l].min(w.ncols()) {
                            if w[(j, i)].abs() > eps {
                                hidden_in[l + 1][j] += 1;
                                hidden_out[l][i] += 1;
                            }
                        }
                    }
                }
                if let Some(w) = runner.w_hh_bwd.get(l) {
                    for j in 0..layer_sizes[l].min(w.nrows()) {
                        for i in 0..layer_sizes[l + 1].min(w.ncols()) {
                            if w[(j, i)].abs() > eps {
                                hidden_in[l][j] += 1;
                                hidden_out[l + 1][i] += 1;
                            }
                        }
                    }
                }
            }

            for l in 0..layer_sizes.len() {
                if let Some(w) = runner.w_hh_rec.get(l) {
                    for j in 0..layer_sizes[l].min(w.nrows()) {
                        for i in 0..layer_sizes[l].min(w.ncols()) {
                            if w[(j, i)].abs() > eps {
                                hidden_in[l][j] += 1;
                                hidden_out[l][i] += 1;
                            }
                        }
                    }
                }
            }

            if out_l < layer_sizes.len() && o_count > 0 {
                for k in 0..o_count.min(runner.w_out.nrows()) {
                    for j in 0..layer_sizes[out_l].min(runner.w_out.ncols()) {
                        if runner.w_out[(k, j)].abs() > eps {
                            output_in[k] += 1;
                            hidden_out[out_l][j] += 1;
                        }
                    }
                }
            }

            let sens_possible = if in_l < layer_sizes.len() {
                layer_sizes[in_l]
            } else {
                0
            };
            if sens_possible > 0 {
                for &count in &sensory_out {
                    let ratio = count as f64 / sens_possible as f64;
                    if ratio > max_neuron_ratio {
                        max_neuron_ratio = ratio;
                    }
                }
            }

            let out_possible = if out_l < layer_sizes.len() {
                layer_sizes[out_l]
            } else {
                0
            };
            if out_possible > 0 {
                for &count in &output_in {
                    let ratio = count as f64 / out_possible as f64;
                    if ratio > max_neuron_ratio {
                        max_neuron_ratio = ratio;
                    }
                }
            }

            for l in 0..layer_sizes.len() {
                let n = layer_sizes[l];
                if n == 0 {
                    continue;
                }
                let possible_in = (if l == in_l { s_count } else { 0 })
                    + (if l > 0 { layer_sizes[l - 1] } else { 0 })
                    + (if l + 1 < layer_sizes.len() {
                        layer_sizes[l + 1]
                    } else {
                        0
                    })
                    + n;
                let possible_out = (if l + 1 < layer_sizes.len() {
                    layer_sizes[l + 1]
                } else {
                    0
                }) + (if l > 0 { layer_sizes[l - 1] } else { 0 })
                    + n
                    + (if l == out_l { o_count } else { 0 });
                let possible_total = possible_in + possible_out;
                if possible_total == 0 {
                    continue;
                }
                for j in 0..n {
                    let actual = hidden_in[l][j] + hidden_out[l][j];
                    let ratio = actual as f64 / possible_total as f64;
                    if ratio > max_neuron_ratio {
                        max_neuron_ratio = ratio;
                    }
                }
            }
        }

        let density = if total_possible > 0 {
            total_conn as f64 / total_possible as f64
        } else {
            1.0
        };
        let target_density = 0.12;
        let mut sparse_score = (1.0 - (density - target_density).abs() / target_density).max(0.0);
        if density > 0.3 {
            sparse_score *= 0.1;
        }
        if density > 0.6 {
            sparse_score = 0.0;
        }
        let cap = 0.12;
        let per_neuron_score = if max_neuron_ratio <= cap {
            1.0
        } else {
            (1.0 - ((max_neuron_ratio - cap) / cap)).max(0.0)
        };

        let layer_score = if layer_count > 0 {
            (layer_count.min(6) as f64) / 6.0
        } else {
            0.0
        };
        let longterm_score = stability_ratio;

        let aarnn_depth_score = if eval_config.use_morphology {
            let depth = eval_config.aarnn_layer_depth.min(GA_AARNN_MAX_DEPTH);
            depth as f64 / GA_AARNN_MAX_DEPTH.max(1) as f64
        } else {
            0.0
        };

        let bio_score = if eval_config.use_morphology {
            let bio = &eval_config.aarnn_bio;
            let mut hits = 0.0f64;
            let mut total = 0.0f64;

            let add_bool = |ok: bool, hits: &mut f64, total: &mut f64| {
                *total += 1.0;
                if ok {
                    *hits += 1.0;
                }
            };

            let add_granular = |val: f64, min: f64, max: f64, hits: &mut f64, total: &mut f64| {
                *total += 1.0;
                if val >= min && val <= max {
                    *hits += 1.0;
                } else {
                    let range = (max - min).max(1e-9);
                    let dist = if val < min { min - val } else { val - max };
                    // Continuous penalty: 1.0 at boundary, decreasing as we get further
                    let score = (1.0 - (dist / range)).max(0.0);
                    *hits += score;
                }
            };

            add_bool(bio.stp_enabled, &mut hits, &mut total);
            add_granular(bio.stp_u as f64, 0.1, 0.5, &mut hits, &mut total);
            add_granular(
                bio.stp_tau_rec_ms as f64,
                100.0,
                1500.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.stp_tau_facil_ms as f64,
                50.0,
                800.0,
                &mut hits,
                &mut total,
            );
            add_granular(bio.ampa_tau_ms as f64, 2.0, 10.0, &mut hits, &mut total);
            add_granular(bio.nmda_tau_ms as f64, 40.0, 200.0, &mut hits, &mut total);
            add_granular(bio.gaba_tau_ms as f64, 5.0, 30.0, &mut hits, &mut total);
            add_granular(bio.nmda_ratio as f64, 0.1, 0.5, &mut hits, &mut total);
            add_granular(bio.synaptic_gain as f64, 0.5, 2.0, &mut hits, &mut total);
            add_bool(bio.adaptive_threshold_enabled, &mut hits, &mut total);
            add_granular(
                bio.adaptive_threshold_tau_ms as f64,
                50.0,
                800.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.adaptive_threshold_increment as f64,
                0.2,
                1.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.homeostasis_target_rate_hz as f64,
                1.0,
                5.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.homeostasis_tau_ms as f64,
                500.0,
                3000.0,
                &mut hits,
                &mut total,
            );
            add_bool(bio.neuromodulation_enabled, &mut hits, &mut total);
            add_bool(bio.dendritic_active_enabled, &mut hits, &mut total);
            add_granular(
                bio.dendritic_ca_tau_ms as f64,
                60.0,
                250.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.dendritic_plateau_tau_ms as f64,
                120.0,
                900.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.dendritic_ca_influx_gain as f64,
                0.04,
                0.25,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.dendritic_plateau_threshold as f64,
                0.6,
                1.5,
                &mut hits,
                &mut total,
            );
            add_granular(
                bio.dendritic_plateau_gain as f64,
                0.15,
                0.8,
                &mut hits,
                &mut total,
            );

            // Extended AARNN plausibility priors (kept soft to avoid over-constraining search).
            add_granular(
                eval_config.aarnn_inhibitory_fraction as f64,
                0.15,
                0.30,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_dale_strictness as f64,
                0.70,
                1.00,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_gap_junction_strength as f64,
                0.005,
                0.06,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_gap_junction_radius as f64,
                0.05,
                0.35,
                &mut hits,
                &mut total,
            );
            add_bool(
                eval_config.aarnn_gap_junction_inhibitory_only,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_nmda_voltage_sensitivity as f64,
                0.02,
                0.12,
                &mut hits,
                &mut total,
            );
            add_bool(
                eval_config.volume_transmission_enabled,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.volume_transmission_radius as f64,
                0.15,
                0.6,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.volume_transmission_strength as f64,
                0.02,
                0.25,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_triplet_ltp_gain as f64,
                0.10,
                0.80,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_triplet_ltd_gain as f64,
                0.05,
                0.60,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_synaptic_scaling_strength as f64,
                0.005,
                0.08,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_synaptic_scaling_target as f64,
                0.6,
                1.8,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_distance_attenuation_per_unit as f64,
                0.05,
                0.6,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_release_prob_heterogeneity as f64,
                0.05,
                0.40,
                &mut hits,
                &mut total,
            );
            add_bool(eval_config.aarnn_myelination_enabled, &mut hits, &mut total);
            add_granular(
                eval_config.aarnn_myelination_rate as f64,
                0.0003,
                0.01,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_demyelination_rate as f64,
                0.0001,
                0.005,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_myelination_activity_target as f64,
                0.05,
                0.25,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_myelin_min_conduction_gain as f64,
                0.6,
                1.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_myelin_max_conduction_gain as f64,
                1.2,
                3.0,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_myelin_initial as f64,
                0.15,
                0.7,
                &mut hits,
                &mut total,
            );

            // Biological tendency: potentiation and depression gains are generally same-order,
            // with mild LTP dominance often observed in stable learning regimes.
            add_granular(
                (eval_config.aarnn_triplet_ltp_gain as f64
                    - eval_config.aarnn_triplet_ltd_gain as f64)
                    .abs(),
                0.0,
                0.5,
                &mut hits,
                &mut total,
            );
            add_granular(
                (eval_config.aarnn_myelin_max_conduction_gain as f64
                    - eval_config.aarnn_myelin_min_conduction_gain as f64)
                    .abs(),
                0.3,
                2.8,
                &mut hits,
                &mut total,
            );
            add_granular(
                eval_config.aarnn_myelination_rate as f64
                    - eval_config.aarnn_demyelination_rate as f64,
                0.00005,
                0.01,
                &mut hits,
                &mut total,
            );

            if total > 0.0 { hits / total } else { 0.0 }
        } else {
            0.0
        };

        let aarnn_feature_score = if eval_config.use_morphology {
            let bio = &eval_config.aarnn_bio;
            let mut enabled = 0.0;
            if bio.stp_enabled {
                enabled += 1.0;
            }
            if bio.neuromodulation_enabled {
                enabled += 1.0;
            }
            if bio.dendritic_active_enabled {
                enabled += 1.0;
            }
            if eval_config.volume_transmission_enabled {
                enabled += 1.0;
            }
            if eval_config.aarnn_myelination_enabled {
                enabled += 1.0;
            }
            enabled / 5.0
        } else {
            0.0
        };

        let mut score = (0.36 * longterm_score)
            + (0.14 * layer_score)
            + (0.25 * sparse_score)
            + (0.08 * per_neuron_score)
            + (0.05 * bio_score)
            + (0.07 * aarnn_depth_score)
            + (0.05 * aarnn_feature_score);
        if layer_count < 2 {
            score *= 0.2;
        }
        GA_TOTAL_EVALUATIONS.fetch_add(1, Ordering::Relaxed);
        score.clamp(0.0, 1.0)
    }

    pub fn evolve(&mut self, n_elite: usize, use_dk_bias: bool, rng: &mut StdRng) {
        let pop_size = self.population.len();
        if pop_size == 0 {
            return;
        }

        // 1. Update stagnation and self-adaptation for the current population (recently evaluated)
        for ind in &mut self.population {
            if ind.fitness > ind.last_fitness + 1e-6 {
                ind.stagnation = 0;
                ind.last_fitness = ind.fitness;
            } else {
                ind.stagnation += 1;
            }

            // If stagnant for more than 5 generations, randomize rates to escape local optima
            if ind.stagnation >= 5 {
                ind.mutation_rate = rng.random_range(0.01..0.2);
                ind.crossover_rate = rng.random_range(0.4..0.9);
            }
        }

        // 2. Apply Dunning-Kruger (DK) Fitness Bias for selection
        // We use a temporary vector to store adjusted fitness for sorting/selection
        let mut selection_pool: Vec<(usize, f64)> = self
            .population
            .iter()
            .enumerate()
            .map(|(i, ind)| (i, ind.fitness))
            .collect();

        // Sort by raw fitness to determine ranks
        selection_pool.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if use_dk_bias {
            // Apply bias based on rank
            for rank in 0..pop_size {
                let (_idx, raw_fitness) = selection_pool[rank];
                let percentile = 1.0 - (rank as f64 / pop_size as f64);
                // DK Bias: Boost lower performers, slightly penalize top dominance to maintain diversity
                let bias = 0.1 * (1.0 - percentile) - 0.03 * percentile;
                selection_pool[rank].1 = (raw_fitness + bias).clamp(0.0, 1.0);
            }

            // Re-sort selection pool by adjusted fitness
            selection_pool
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        // 3. Update global best (using raw fitness)
        // Note: population is not yet sorted by raw fitness here, but selection_pool[0] was the best
        // if bias didn't push someone else higher. Let's find the absolute best.
        let (best_idx, _) = self
            .population
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.fitness
                    .partial_cmp(&b.fitness)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();

        if self.population[best_idx].fitness > self.best_fitness {
            self.best_fitness = self.population[best_idx].fitness;
            self.best_config = Some(self.population[best_idx].config.clone());
        }

        // 4. Update leaderboard
        let to_add: Vec<Individual> = self
            .population
            .iter()
            .filter(|ind| ind.fitness > 0.0)
            .cloned()
            .collect();
        for ind in to_add {
            self.add_to_leaderboard(ind);
        }

        let mut next_gen = Vec::with_capacity(pop_size);

        // 5. Elitism (using best adjusted individuals)
        let mut elites_added = 0;
        for (idx, _) in &selection_pool {
            if elites_added >= n_elite {
                break;
            }
            let ind = &self.population[*idx];
            if !next_gen.iter().any(|x: &Individual| x.config == ind.config) {
                next_gen.push(ind.clone());
                elites_added += 1;
            }
        }

        // 6. Selection, Crossover, Mutation
        let mut attempts = 0;
        let mut children_added = 0;
        let top_selection_range = (pop_size / 2).max(1);

        while next_gen.len() < pop_size && attempts < pop_size * 20 {
            attempts += 1;

            // Tournament/Weighted selection from adjusted fitness pool
            let p1_idx = selection_pool[rng.random_range(0..top_selection_range)].0;
            let p2_idx = selection_pool[rng.random_range(0..top_selection_range)].0;
            let p1 = &self.population[p1_idx];
            let p2 = &self.population[p2_idx];

            // Crossover using self-adaptive rate
            let crossover_prob = (p1.crossover_rate + p2.crossover_rate) / 2.0;
            let mut child_config = if rng.random_bool(crossover_prob) {
                crossover(&p1.config, &p2.config, rng)
            } else {
                if rng.random_bool(0.5) {
                    p1.config.clone()
                } else {
                    p2.config.clone()
                }
            };

            // Mutation using inherited/averaged rates
            let mut child_mutation_rate = (p1.mutation_rate + p2.mutation_rate) / 2.0;
            // Meta-mutation: slightly mutate the rates themselves
            child_mutation_rate =
                (child_mutation_rate + rng.random_range(-0.005..0.005)).clamp(0.001, 0.3);
            let child_crossover_rate = ((p1.crossover_rate + p2.crossover_rate) / 2.0
                + rng.random_range(-0.01..0.01))
            .clamp(0.1, 0.95);

            mutate(&mut child_config, child_mutation_rate, rng);

            if !next_gen
                .iter()
                .any(|x: &Individual| x.config == child_config)
            {
                let mut child = Individual::new(child_config, 0.0);
                child.mutation_rate = child_mutation_rate;
                child.crossover_rate = child_crossover_rate;
                // Children start with 0 stagnation
                next_gen.push(child);
                children_added += 1;
            }
        }

        // If we still didn't fill the population, randomize the rest
        let mut randomized_added = 0;
        while next_gen.len() < pop_size {
            let base_idx = selection_pool[0].0;
            let mut ind = Individual::new(
                randomize_config(&self.population[base_idx].config, rng),
                0.0,
            );
            // Randomize rates for new random individuals too
            ind.mutation_rate = rng.random_range(0.01..0.2);
            ind.crossover_rate = rng.random_range(0.4..0.9);
            next_gen.push(ind);
            randomized_added += 1;
        }

        nm_log!(
            "[info] Evolved generation {}: {} elites, {} children, {} randomized",
            self.generation + 1,
            elites_added,
            children_added,
            randomized_added
        );

        self.population = next_gen;
        if self.force_morphology {
            for ind in &mut self.population {
                ind.config.use_morphology = true;
            }
        }
        self.generation += 1;
    }

    pub fn add_to_leaderboard(&mut self, ind: Individual) {
        if ind.fitness <= 0.0 {
            return;
        }
        self.leaderboard.push(ind);
        self.leaderboard.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.leaderboard
            .dedup_by(|a, b| (a.fitness - b.fitness).abs() < 1e-9);
        self.leaderboard.truncate(10);

        if let Some(best) = self.leaderboard.first() {
            if best.fitness > self.best_fitness {
                self.best_fitness = best.fitness;
                self.best_config = Some(best.config.clone());
            }
        }
    }

    pub fn save_leaderboard(&self, path: &str) -> anyhow::Result<()> {
        let s = serde_json::to_string_pretty(&self.leaderboard)?;
        std::fs::write(path, s)?;
        Ok(())
    }

    pub fn load_leaderboard(&mut self, path: &str) -> anyhow::Result<()> {
        if std::path::Path::new(path).exists() {
            let s = std::fs::read_to_string(path)?;
            self.leaderboard = serde_json::from_str(&s)?;
            if let Some(best) = self.leaderboard.first() {
                if best.fitness > self.best_fitness {
                    self.best_fitness = best.fitness;
                    self.best_config = Some(best.config.clone());
                }
            }
        }
        Ok(())
    }
}

fn randomize_config(base: &NetworkConfig, rng: &mut StdRng) -> NetworkConfig {
    let mut cfg = base.clone();
    let design = pick_clumping_design(rng);
    apply_clumping_design(&mut cfg, design);
    // Randomize a subset of parameters
    cfg.num_hidden_layers = rng.random_range(2..=6);
    cfg.num_hidden_per_layer_initial = rng.random_range(2..=64);
    cfg.sensory_target_layer = if rng.random_bool(0.5) {
        Some(rng.random_range(0..cfg.num_hidden_layers.max(1)))
    } else {
        None
    };
    cfg.output_source_layer = if rng.random_bool(0.5) {
        Some(rng.random_range(0..cfg.num_hidden_layers.max(1)))
    } else {
        None
    };
    cfg.max_layers = rng.random_range(1..=10);
    cfg.layer_split_threshold = rng.random_range(1..=256);
    let min_total = min_total_neurons(&cfg);
    let max_total = min_total
        .saturating_mul(20)
        .max(min_total.saturating_add(16));
    cfg.max_total_neurons = if rng.random_bool(0.2) {
        0
    } else {
        rng.random_range(min_total..=max_total)
    };
    cfg.p_in = rng.random_range(0.05..0.5);
    cfg.p_hidden = rng.random_range(0.01..0.3);
    cfg.p_out = rng.random_range(0.05..0.5);
    cfg.growth_enabled = rng.random_bool(0.9);
    cfg.saturation_threshold = rng.random_range(0.0..2.0);
    cfg.saturation_window_ms = rng.random_range(20.0..10000.0);
    cfg.growth_cooldown_ms = rng.random_range(0.0..10000.0);
    cfg.global_growth_cooldown_ms = rng.random_range(0.0..5000.0);
    cfg.proximity_degree_cap = rng.random_range(0..=64);
    cfg.spawn_radius = rng.random_range(0.01..2.0);
    cfg.migrate_in_prob = rng.random_range(0.0..1.0);
    cfg.migrate_out_prob = rng.random_range(0.0..1.0);
    cfg.new_edge_prob = rng.random_range(0.0..1.0);
    cfg.aarnn_velocity = rng.random_range(0.1..50.0);
    cfg.axon_velocity = rng.random_range(0.0..100.0);
    cfg.dend_velocity = rng.random_range(0.0..100.0);
    cfg.use_aarnn_delays = rng.random_bool(0.7);
    cfg.bouton_latency_ms = rng.random_range(0.0..20.0);
    cfg.bouton_jitter_ms = rng.random_range(0.0..10.0);

    // Geometry
    cfg.enforce_unique_geometry = rng.random_bool(0.8);
    cfg.min_node_sep = rng.random_range(0.005..0.1);
    cfg.min_segment_sep = rng.random_range(0.001..0.05);
    cfg.synapse_offset = rng.random_range(0.001..0.05);
    cfg.max_place_tries = rng.random_range(4..=64);
    cfg.relax_iters = rng.random_range(0..=8);
    cfg.relax_step = rng.random_range(0.0..0.02);
    cfg.seg_eps = rng.random_range(0.0005..0.01);
    cfg.max_reroute_tries = rng.random_range(1..=16);
    cfg.use_mid_bends = rng.random_bool(0.7);

    // Morphology growth
    cfg.morpho_growth_enabled = rng.random_bool(0.8);
    cfg.trunk_growth_rate = rng.random_range(0.0001..0.5);
    cfg.branch_growth_rate = rng.random_range(0.001..1.0);
    cfg.bouton_growth_rate = rng.random_range(0.005..2.0);
    cfg.max_segment_length = rng.random_range(0.1..2.0);
    cfg.spatial_repulsion_strength = rng.random_range(0.0..0.2);
    cfg.spatial_clumping_strength = rng.random_range(0.0..0.2);
    cfg.columnar_enabled = rng.random_bool(0.5);
    cfg.columnar_spacing = rng.random_range(0.05..1.0);
    cfg.columnar_strength = rng.random_range(0.0..0.1);
    cfg.columnar_jitter = rng.random_range(0.0..1.0);
    cfg.density_target = rng.random_range(0.01..0.2);
    cfg.skull_pid_kp = rng.random_range(0.001..0.2);
    cfg.skull_pid_ki = rng.random_range(0.0..0.02);
    cfg.skull_pid_kd = rng.random_range(0.0..0.05);
    cfg.energy_attraction_radius = rng.random_range(0.05..5.0);
    cfg.dendrite_sprout_prob = rng.random_range(0.0..2.0);
    cfg.axon_contact_dist = rng.random_range(0.005..2.0);
    cfg.aarnn_ambient_energy_level = rng.random_range(0.0..1.0);
    cfg.aarnn_resonance_gain = rng.random_range(0.0..1.0);
    cfg.aarnn_resonance_decay = rng.random_range(0.0..1.0);
    cfg.aarnn_neuromod_baseline_dopamine = rng.random_range(0.0..3.0);
    cfg.aarnn_neuromod_baseline_ach = rng.random_range(0.0..3.0);
    cfg.aarnn_neuromod_baseline_serotonin = rng.random_range(0.0..3.0);
    cfg.aarnn_neuromod_dopamine_signal = pick_neuromod_signal(rng);
    cfg.aarnn_neuromod_ach_signal = pick_neuromod_signal(rng);
    cfg.aarnn_neuromod_serotonin_signal = pick_neuromod_signal(rng);
    cfg.aarnn_reward_proxy = rng.random_range(0.0..1.0);
    cfg.aarnn_neuromod_decay = rng.random_range(0.0..0.5);
    cfg.aarnn_neuromod_error_gain = rng.random_range(0.0..3.0);
    cfg.aarnn_neuromod_activity_gain = rng.random_range(0.0..3.0);
    cfg.aarnn_neuromod_stability_gain = rng.random_range(0.0..3.0);
    cfg.aarnn_inhibitory_fraction = rng.random_range(0.1..0.35);
    cfg.aarnn_dale_strictness = rng.random_range(0.5..1.0);
    cfg.aarnn_gap_junction_strength = rng.random_range(0.0..0.08);
    cfg.aarnn_gap_junction_radius = rng.random_range(0.03..0.4);
    cfg.aarnn_gap_junction_inhibitory_only = rng.random_bool(0.7);
    cfg.aarnn_nmda_voltage_sensitivity = rng.random_range(0.01..0.15);
    cfg.volume_transmission_enabled = rng.random_bool(0.6);
    cfg.volume_transmission_radius = rng.random_range(0.1..0.8);
    cfg.volume_transmission_strength = rng.random_range(0.0..0.3);
    cfg.aarnn_triplet_ltp_gain = rng.random_range(0.05..1.0);
    cfg.aarnn_triplet_ltd_gain = rng.random_range(0.03..0.8);
    cfg.aarnn_synaptic_scaling_strength = rng.random_range(0.002..0.12);
    cfg.aarnn_synaptic_scaling_target = rng.random_range(0.4..2.2);
    cfg.aarnn_distance_attenuation_per_unit = rng.random_range(0.02..0.8);
    cfg.aarnn_release_prob_heterogeneity = rng.random_range(0.01..0.5);
    cfg.aarnn_myelination_enabled = rng.random_bool(0.6);
    cfg.aarnn_myelination_rate = rng.random_range(0.0001..0.01);
    cfg.aarnn_demyelination_rate = rng.random_range(0.00005..0.006);
    cfg.aarnn_myelination_activity_target = rng.random_range(0.03..0.3);
    cfg.aarnn_myelin_min_conduction_gain = rng.random_range(0.6..1.0);
    cfg.aarnn_myelin_max_conduction_gain =
        rng.random_range((cfg.aarnn_myelin_min_conduction_gain + 0.2)..3.2);
    cfg.aarnn_myelin_initial = rng.random_range(0.1..0.8);
    cfg.synaptic_stabilization_strength = rng.random_range(0.0..1.0);
    cfg.component_decay_rate = rng.random_range(0.01..1.0);
    cfg.p_release_default = rng.random_range(0.0..1.0);
    cfg.aarnn_synaptic_energy_randomness = rng.random_range(0.0..1.0);
    cfg.perceptual_loop_enabled = rng.random_bool(0.5);
    cfg.perceptual_prediction_lr = rng.random_range(0.0..1.0);
    cfg.perceptual_prediction_decay = rng.random_range(0.0..0.5);
    cfg.perceptual_prediction_threshold = rng.random_range(0.0..1.0);
    cfg.perceptual_error_gain = rng.random_range(0.0..20.0);
    cfg.perceptual_feedback_gain = rng.random_range(0.0..1.0);
    cfg.world_model_enabled = rng.random_bool(0.5);
    cfg.world_model_dim = rng.random_range(2..=32);
    cfg.world_model_decay = rng.random_range(0.0..0.5);
    cfg.sleep_enabled = rng.random_bool(0.5);
    cfg.sleep_cycle_ms = rng.random_range(1000.0..600000.0);
    cfg.sleep_duration_ms = rng.random_range(100.0..120000.0);
    cfg.sleep_dream_replay_prob = rng.random_range(0.0..1.0);
    cfg.sleep_dream_threshold = rng.random_range(0.0..1.0);
    cfg.sleep_consolidation_gain = rng.random_range(0.0..1.0);
    cfg.theta_rhythm_enabled = rng.random_bool(0.5);
    cfg.theta_rhythm_hz = rng.random_range(0.5..12.0);
    cfg.theta_rhythm_duty = rng.random_range(0.05..0.9);
    cfg.theta_rhythm_drive = rng.random_range(0.0..20.0);
    cfg.theta_rhythm_phase_jitter = rng.random_range(0.0..1.0);
    cfg.thalamic_gating_enabled = rng.random_bool(0.5);
    cfg.thalamic_gate_hz = rng.random_range(0.5..20.0);
    cfg.thalamic_gate_duty = rng.random_range(0.05..0.95);
    cfg.thalamic_gate_floor = rng.random_range(0.0..1.0);
    cfg.energy_kernel_k = rng.random_range(0.01..10.0);
    cfg.synaptic_energy_window_ms = rng.random_range(100.0..30000.0);
    cfg.aarnn_layer_depth = rng.random_range(0..=5);
    cfg.use_morphology = rng.random_bool(0.8);
    cfg.aarnn_bio.izh_preset = pick_izh_preset(rng);
    cfg.aarnn_bio.stp_enabled = rng.random_bool(0.5);
    cfg.aarnn_bio.stp_u = rng.random_range(0.0..1.0);
    cfg.aarnn_bio.stp_tau_rec_ms = rng.random_range(10.0..5000.0);
    cfg.aarnn_bio.stp_tau_facil_ms = rng.random_range(10.0..2000.0);
    cfg.aarnn_bio.ampa_tau_ms = rng.random_range(1.0..50.0);
    cfg.aarnn_bio.nmda_tau_ms = rng.random_range(10.0..300.0);
    cfg.aarnn_bio.gaba_tau_ms = rng.random_range(1.0..50.0);
    cfg.aarnn_bio.nmda_ratio = rng.random_range(0.0..1.0);
    cfg.aarnn_bio.synaptic_gain = rng.random_range(0.1..5.0);
    cfg.aarnn_bio.dendritic_active_enabled = rng.random_bool(0.6);
    cfg.aarnn_bio.dendritic_ca_tau_ms = rng.random_range(40.0..400.0);
    cfg.aarnn_bio.dendritic_plateau_tau_ms = rng.random_range(100.0..1200.0);
    cfg.aarnn_bio.dendritic_ca_influx_gain = rng.random_range(0.02..0.4);
    cfg.aarnn_bio.dendritic_plateau_threshold = rng.random_range(0.4..2.0);
    cfg.aarnn_bio.dendritic_plateau_gain = rng.random_range(0.05..1.2);
    cfg.aarnn_bio.adaptive_threshold_enabled = rng.random_bool(0.7);
    cfg.aarnn_bio.adaptive_threshold_tau_ms = rng.random_range(10.0..1000.0);
    cfg.aarnn_bio.adaptive_threshold_increment = rng.random_range(0.0..5.0);
    cfg.aarnn_bio.adaptive_threshold_min = rng.random_range(-5.0..0.0);
    cfg.aarnn_bio.adaptive_threshold_max = rng.random_range(0.0..10.0);
    cfg.aarnn_bio.izh_refractory_ms = rng.random_range(0.0..10.0);
    cfg.aarnn_bio.homeostasis_target_rate_hz = rng.random_range(0.0..20.0);
    cfg.aarnn_bio.homeostasis_tau_ms = rng.random_range(100.0..10000.0);
    cfg.aarnn_bio.homeostasis_gain = rng.random_range(0.0..5.0);
    cfg.aarnn_bio.neuromodulation_enabled = rng.random_bool(0.5);
    cfg.aarnn_bio.dopamine_gain = rng.random_range(0.1..3.0);
    cfg.aarnn_bio.acetylcholine_gain = rng.random_range(0.1..3.0);
    cfg.aarnn_bio.serotonin_gain = rng.random_range(0.1..3.0);

    // Stability & Search optimization parameters
    cfg.component_pruning_threshold = rng.random_range(0.001..0.2);
    cfg.initial_synaptic_weight = rng.random_range(0.001..0.5);
    cfg.synaptic_growth_threshold = rng.random_range(0.1..0.9);
    cfg.synaptic_consolidation_factor = rng.random_range(0.0..1.0);
    cfg.spontaneous_neuron_interval_ms = rng.random_range(20.0..10000.0);
    cfg.neuron_removal_delay_ms = rng.random_range(500.0..180000.0);
    cfg.max_sensory_connections = rng.random_range(1..=128);
    cfg.max_output_connections = rng.random_range(1..=128);

    if cfg.aarnn_myelin_max_conduction_gain <= cfg.aarnn_myelin_min_conduction_gain {
        cfg.aarnn_myelin_max_conduction_gain = cfg.aarnn_myelin_min_conduction_gain + 0.2;
    }

    if cfg.max_layers < cfg.num_hidden_layers {
        cfg.max_layers = cfg.num_hidden_layers;
    }
    sanitize_io_layers(&mut cfg);

    cfg
}

fn crossover(p1: &NetworkConfig, p2: &NetworkConfig, rng: &mut StdRng) -> NetworkConfig {
    let mut child = p1.clone();

    macro_rules! crossover_field {
        ($field:ident) => {
            if rng.random_bool(0.5) {
                child.$field = p2.$field;
            }
        };
    }

    if rng.random_bool(0.5) {
        apply_clumping_design_keep_max_total(&mut child, p2.clumping_design);
    }

    // List of fields to crossover
    crossover_field!(p_in);
    crossover_field!(p_hidden);
    crossover_field!(p_out);
    crossover_field!(num_hidden_layers);
    crossover_field!(num_hidden_per_layer_initial);
    crossover_field!(max_layers);
    crossover_field!(max_total_neurons);
    crossover_field!(sensory_target_layer);
    crossover_field!(output_source_layer);
    crossover_field!(layer_split_threshold);
    crossover_field!(growth_enabled);
    crossover_field!(saturation_threshold);
    crossover_field!(saturation_window_ms);
    crossover_field!(growth_cooldown_ms);
    crossover_field!(global_growth_cooldown_ms);
    crossover_field!(proximity_degree_cap);
    crossover_field!(spawn_radius);
    crossover_field!(migrate_in_prob);
    crossover_field!(migrate_out_prob);
    crossover_field!(new_edge_prob);
    crossover_field!(aarnn_velocity);
    crossover_field!(axon_velocity);
    crossover_field!(dend_velocity);
    crossover_field!(use_aarnn_delays);
    crossover_field!(bouton_latency_ms);
    crossover_field!(bouton_jitter_ms);
    crossover_field!(enforce_unique_geometry);
    crossover_field!(min_node_sep);
    crossover_field!(min_segment_sep);
    crossover_field!(synapse_offset);
    crossover_field!(max_place_tries);
    crossover_field!(relax_iters);
    crossover_field!(relax_step);
    crossover_field!(seg_eps);
    crossover_field!(max_reroute_tries);
    crossover_field!(use_mid_bends);
    crossover_field!(morpho_growth_enabled);
    crossover_field!(trunk_growth_rate);
    crossover_field!(branch_growth_rate);
    crossover_field!(bouton_growth_rate);
    crossover_field!(max_segment_length);
    crossover_field!(spatial_repulsion_strength);
    crossover_field!(spatial_clumping_strength);
    crossover_field!(columnar_enabled);
    crossover_field!(columnar_spacing);
    crossover_field!(columnar_strength);
    crossover_field!(columnar_jitter);
    crossover_field!(density_target);
    crossover_field!(skull_pid_kp);
    crossover_field!(skull_pid_ki);
    crossover_field!(skull_pid_kd);
    crossover_field!(energy_attraction_radius);
    crossover_field!(dendrite_sprout_prob);
    crossover_field!(axon_contact_dist);
    crossover_field!(aarnn_ambient_energy_level);
    crossover_field!(aarnn_resonance_gain);
    crossover_field!(aarnn_resonance_decay);
    crossover_field!(aarnn_neuromod_baseline_dopamine);
    crossover_field!(aarnn_neuromod_baseline_ach);
    crossover_field!(aarnn_neuromod_baseline_serotonin);
    crossover_field!(aarnn_neuromod_dopamine_signal);
    crossover_field!(aarnn_neuromod_ach_signal);
    crossover_field!(aarnn_neuromod_serotonin_signal);
    crossover_field!(aarnn_reward_proxy);
    crossover_field!(aarnn_neuromod_decay);
    crossover_field!(aarnn_neuromod_error_gain);
    crossover_field!(aarnn_neuromod_activity_gain);
    crossover_field!(aarnn_neuromod_stability_gain);
    crossover_field!(aarnn_inhibitory_fraction);
    crossover_field!(aarnn_dale_strictness);
    crossover_field!(aarnn_gap_junction_strength);
    crossover_field!(aarnn_gap_junction_radius);
    crossover_field!(aarnn_gap_junction_inhibitory_only);
    crossover_field!(aarnn_nmda_voltage_sensitivity);
    crossover_field!(volume_transmission_enabled);
    crossover_field!(volume_transmission_radius);
    crossover_field!(volume_transmission_strength);
    crossover_field!(aarnn_triplet_ltp_gain);
    crossover_field!(aarnn_triplet_ltd_gain);
    crossover_field!(aarnn_synaptic_scaling_strength);
    crossover_field!(aarnn_synaptic_scaling_target);
    crossover_field!(aarnn_distance_attenuation_per_unit);
    crossover_field!(aarnn_release_prob_heterogeneity);
    crossover_field!(aarnn_myelination_enabled);
    crossover_field!(aarnn_myelination_rate);
    crossover_field!(aarnn_demyelination_rate);
    crossover_field!(aarnn_myelination_activity_target);
    crossover_field!(aarnn_myelin_min_conduction_gain);
    crossover_field!(aarnn_myelin_max_conduction_gain);
    crossover_field!(aarnn_myelin_initial);
    crossover_field!(synaptic_stabilization_strength);
    crossover_field!(component_decay_rate);
    crossover_field!(p_release_default);
    crossover_field!(aarnn_synaptic_energy_randomness);
    crossover_field!(perceptual_loop_enabled);
    crossover_field!(perceptual_prediction_lr);
    crossover_field!(perceptual_prediction_decay);
    crossover_field!(perceptual_prediction_threshold);
    crossover_field!(perceptual_error_gain);
    crossover_field!(perceptual_feedback_gain);
    crossover_field!(world_model_enabled);
    crossover_field!(world_model_dim);
    crossover_field!(world_model_decay);
    crossover_field!(sleep_enabled);
    crossover_field!(sleep_cycle_ms);
    crossover_field!(sleep_duration_ms);
    crossover_field!(sleep_dream_replay_prob);
    crossover_field!(sleep_dream_threshold);
    crossover_field!(sleep_consolidation_gain);
    crossover_field!(theta_rhythm_enabled);
    crossover_field!(theta_rhythm_hz);
    crossover_field!(theta_rhythm_duty);
    crossover_field!(theta_rhythm_drive);
    crossover_field!(theta_rhythm_phase_jitter);
    crossover_field!(thalamic_gating_enabled);
    crossover_field!(thalamic_gate_hz);
    crossover_field!(thalamic_gate_duty);
    crossover_field!(thalamic_gate_floor);
    crossover_field!(energy_kernel_k);
    crossover_field!(synaptic_energy_window_ms);
    crossover_field!(component_pruning_threshold);
    crossover_field!(initial_synaptic_weight);
    crossover_field!(synaptic_growth_threshold);
    crossover_field!(synaptic_consolidation_factor);
    crossover_field!(spontaneous_neuron_interval_ms);
    crossover_field!(neuron_removal_delay_ms);
    crossover_field!(max_sensory_connections);
    crossover_field!(max_output_connections);
    crossover_field!(aarnn_layer_depth);
    crossover_field!(use_morphology);
    if rng.random_bool(0.5) {
        child.aarnn_bio.izh_preset = p2.aarnn_bio.izh_preset.clone();
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.stp_enabled = p2.aarnn_bio.stp_enabled;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.stp_u = p2.aarnn_bio.stp_u;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.stp_tau_rec_ms = p2.aarnn_bio.stp_tau_rec_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.stp_tau_facil_ms = p2.aarnn_bio.stp_tau_facil_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.ampa_tau_ms = p2.aarnn_bio.ampa_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.nmda_tau_ms = p2.aarnn_bio.nmda_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.gaba_tau_ms = p2.aarnn_bio.gaba_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.nmda_ratio = p2.aarnn_bio.nmda_ratio;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.synaptic_gain = p2.aarnn_bio.synaptic_gain;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dendritic_active_enabled = p2.aarnn_bio.dendritic_active_enabled;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dendritic_ca_tau_ms = p2.aarnn_bio.dendritic_ca_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dendritic_plateau_tau_ms = p2.aarnn_bio.dendritic_plateau_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dendritic_ca_influx_gain = p2.aarnn_bio.dendritic_ca_influx_gain;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dendritic_plateau_threshold = p2.aarnn_bio.dendritic_plateau_threshold;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dendritic_plateau_gain = p2.aarnn_bio.dendritic_plateau_gain;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.adaptive_threshold_enabled = p2.aarnn_bio.adaptive_threshold_enabled;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.adaptive_threshold_tau_ms = p2.aarnn_bio.adaptive_threshold_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.adaptive_threshold_increment = p2.aarnn_bio.adaptive_threshold_increment;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.adaptive_threshold_min = p2.aarnn_bio.adaptive_threshold_min;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.adaptive_threshold_max = p2.aarnn_bio.adaptive_threshold_max;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.izh_refractory_ms = p2.aarnn_bio.izh_refractory_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.homeostasis_target_rate_hz = p2.aarnn_bio.homeostasis_target_rate_hz;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.homeostasis_tau_ms = p2.aarnn_bio.homeostasis_tau_ms;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.homeostasis_gain = p2.aarnn_bio.homeostasis_gain;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.neuromodulation_enabled = p2.aarnn_bio.neuromodulation_enabled;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.dopamine_gain = p2.aarnn_bio.dopamine_gain;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.acetylcholine_gain = p2.aarnn_bio.acetylcholine_gain;
    }
    if rng.random_bool(0.5) {
        child.aarnn_bio.serotonin_gain = p2.aarnn_bio.serotonin_gain;
    }

    if child.max_layers < child.num_hidden_layers {
        child.max_layers = child.num_hidden_layers;
    }
    if child.aarnn_myelin_max_conduction_gain <= child.aarnn_myelin_min_conduction_gain {
        child.aarnn_myelin_max_conduction_gain = child.aarnn_myelin_min_conduction_gain + 0.2;
    }
    sanitize_io_layers(&mut child);

    child
}

fn mutate(cfg: &mut NetworkConfig, rate: f64, rng: &mut StdRng) {
    let mut changed = false;
    macro_rules! mutate_field {
        ($field:ident, $range:expr) => {
            if rng.random_bool(rate) {
                cfg.$field = rng.random_range($range);
                changed = true;
            }
        };
    }

    mutate_field!(p_in, 0.05..0.5);
    mutate_field!(p_hidden, 0.01..0.3);
    mutate_field!(p_out, 0.05..0.5);
    if rng.random_bool(rate) {
        cfg.growth_enabled = rng.random_bool(0.8);
        changed = true;
    }
    mutate_field!(num_hidden_layers, 2..=6);
    mutate_field!(num_hidden_per_layer_initial, 2..=64);
    mutate_field!(max_layers, 1..=10);
    if rng.random_bool(rate) {
        cfg.sensory_target_layer = if rng.random_bool(0.5) {
            Some(rng.random_range(0..cfg.num_hidden_layers.max(1)))
        } else {
            None
        };
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.output_source_layer = if rng.random_bool(0.5) {
            Some(rng.random_range(0..cfg.num_hidden_layers.max(1)))
        } else {
            None
        };
        changed = true;
    }
    if rng.random_bool(rate) {
        let design = pick_clumping_design(rng);
        apply_clumping_design_keep_max_total(cfg, design);
        changed = true;
    }
    mutate_field!(layer_split_threshold, 1..=256);
    if rng.random_bool(rate) {
        let min_total = min_total_neurons(cfg);
        let max_total = min_total
            .saturating_mul(20)
            .max(min_total.saturating_add(16));
        cfg.max_total_neurons = if rng.random_bool(0.2) {
            0
        } else {
            rng.random_range(min_total..=max_total)
        };
        changed = true;
    }
    mutate_field!(saturation_threshold, 0.0..2.0);
    mutate_field!(saturation_window_ms, 20.0..10000.0);
    mutate_field!(growth_cooldown_ms, 0.0..10000.0);
    mutate_field!(global_growth_cooldown_ms, 0.0..5000.0);
    mutate_field!(proximity_degree_cap, 0..=64);
    mutate_field!(spawn_radius, 0.01..2.0);
    mutate_field!(migrate_in_prob, 0.0..1.0);
    mutate_field!(migrate_out_prob, 0.0..1.0);
    mutate_field!(new_edge_prob, 0.0..1.0);
    mutate_field!(aarnn_velocity, 0.1..50.0);
    mutate_field!(axon_velocity, 0.0..100.0);
    mutate_field!(dend_velocity, 0.0..100.0);
    if rng.random_bool(rate) {
        cfg.use_aarnn_delays = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(bouton_latency_ms, 0.0..20.0);
    mutate_field!(bouton_jitter_ms, 0.0..10.0);
    if rng.random_bool(rate) {
        cfg.enforce_unique_geometry = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(min_node_sep, 0.005..0.1);
    mutate_field!(min_segment_sep, 0.001..0.05);
    mutate_field!(synapse_offset, 0.001..0.05);
    mutate_field!(max_place_tries, 4..=64);
    mutate_field!(relax_iters, 0..=8);
    mutate_field!(relax_step, 0.0..0.02);
    mutate_field!(seg_eps, 0.0005..0.01);
    mutate_field!(max_reroute_tries, 1..=16);
    if rng.random_bool(rate) {
        cfg.use_mid_bends = rng.random_bool(0.5);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.morpho_growth_enabled = rng.random_bool(0.7);
        changed = true;
    }
    mutate_field!(trunk_growth_rate, 0.0001..0.5);
    mutate_field!(branch_growth_rate, 0.001..1.0);
    mutate_field!(bouton_growth_rate, 0.005..2.0);
    mutate_field!(max_segment_length, 0.1..2.0);
    mutate_field!(spatial_repulsion_strength, 0.0..0.2);
    mutate_field!(spatial_clumping_strength, 0.0..0.2);
    if rng.random_bool(rate) {
        cfg.columnar_enabled = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(columnar_spacing, 0.05..1.0);
    mutate_field!(columnar_strength, 0.0..0.1);
    mutate_field!(columnar_jitter, 0.0..1.0);
    mutate_field!(density_target, 0.01..0.2);
    mutate_field!(skull_pid_kp, 0.001..0.2);
    mutate_field!(skull_pid_ki, 0.0..0.02);
    mutate_field!(skull_pid_kd, 0.0..0.05);
    mutate_field!(energy_attraction_radius, 0.05..5.0);
    mutate_field!(dendrite_sprout_prob, 0.0..2.0);
    mutate_field!(axon_contact_dist, 0.005..2.0);
    mutate_field!(aarnn_ambient_energy_level, 0.0..1.0);
    mutate_field!(aarnn_resonance_gain, 0.0..1.0);
    mutate_field!(aarnn_resonance_decay, 0.0..1.0);
    mutate_field!(aarnn_neuromod_baseline_dopamine, 0.0..3.0);
    mutate_field!(aarnn_neuromod_baseline_ach, 0.0..3.0);
    mutate_field!(aarnn_neuromod_baseline_serotonin, 0.0..3.0);
    if rng.random_bool(rate) {
        cfg.aarnn_neuromod_dopamine_signal = pick_neuromod_signal(rng);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_neuromod_ach_signal = pick_neuromod_signal(rng);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_neuromod_serotonin_signal = pick_neuromod_signal(rng);
        changed = true;
    }
    mutate_field!(aarnn_reward_proxy, 0.0..1.0);
    mutate_field!(aarnn_neuromod_decay, 0.0..0.5);
    mutate_field!(aarnn_neuromod_error_gain, 0.0..3.0);
    mutate_field!(aarnn_neuromod_activity_gain, 0.0..3.0);
    mutate_field!(aarnn_neuromod_stability_gain, 0.0..3.0);
    mutate_field!(aarnn_inhibitory_fraction, 0.1..0.35);
    mutate_field!(aarnn_dale_strictness, 0.5..1.0);
    mutate_field!(aarnn_gap_junction_strength, 0.0..0.08);
    mutate_field!(aarnn_gap_junction_radius, 0.03..0.4);
    if rng.random_bool(rate) {
        cfg.aarnn_gap_junction_inhibitory_only = rng.random_bool(0.7);
        changed = true;
    }
    mutate_field!(aarnn_nmda_voltage_sensitivity, 0.01..0.15);
    if rng.random_bool(rate) {
        cfg.volume_transmission_enabled = rng.random_bool(0.6);
        changed = true;
    }
    mutate_field!(volume_transmission_radius, 0.1..0.8);
    mutate_field!(volume_transmission_strength, 0.0..0.3);
    mutate_field!(aarnn_triplet_ltp_gain, 0.05..1.0);
    mutate_field!(aarnn_triplet_ltd_gain, 0.03..0.8);
    mutate_field!(aarnn_synaptic_scaling_strength, 0.002..0.12);
    mutate_field!(aarnn_synaptic_scaling_target, 0.4..2.2);
    mutate_field!(aarnn_distance_attenuation_per_unit, 0.02..0.8);
    mutate_field!(aarnn_release_prob_heterogeneity, 0.01..0.5);
    if rng.random_bool(rate) {
        cfg.aarnn_myelination_enabled = rng.random_bool(0.6);
        changed = true;
    }
    mutate_field!(aarnn_myelination_rate, 0.0001..0.01);
    mutate_field!(aarnn_demyelination_rate, 0.00005..0.006);
    mutate_field!(aarnn_myelination_activity_target, 0.03..0.3);
    mutate_field!(aarnn_myelin_min_conduction_gain, 0.6..1.0);
    mutate_field!(aarnn_myelin_max_conduction_gain, 0.8..3.2);
    mutate_field!(aarnn_myelin_initial, 0.1..0.8);
    mutate_field!(synaptic_stabilization_strength, 0.0..1.0);
    mutate_field!(component_decay_rate, 0.01..1.0);
    mutate_field!(p_release_default, 0.0..1.0);
    mutate_field!(aarnn_synaptic_energy_randomness, 0.0..1.0);
    if rng.random_bool(rate) {
        cfg.perceptual_loop_enabled = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(perceptual_prediction_lr, 0.0..1.0);
    mutate_field!(perceptual_prediction_decay, 0.0..0.5);
    mutate_field!(perceptual_prediction_threshold, 0.0..1.0);
    mutate_field!(perceptual_error_gain, 0.0..20.0);
    mutate_field!(perceptual_feedback_gain, 0.0..1.0);
    if rng.random_bool(rate) {
        cfg.world_model_enabled = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(world_model_dim, 2..=32);
    mutate_field!(world_model_decay, 0.0..0.5);
    if rng.random_bool(rate) {
        cfg.sleep_enabled = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(sleep_cycle_ms, 1000.0..600000.0);
    mutate_field!(sleep_duration_ms, 100.0..120000.0);
    mutate_field!(sleep_dream_replay_prob, 0.0..1.0);
    mutate_field!(sleep_dream_threshold, 0.0..1.0);
    mutate_field!(sleep_consolidation_gain, 0.0..1.0);
    if rng.random_bool(rate) {
        cfg.theta_rhythm_enabled = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(theta_rhythm_hz, 0.5..12.0);
    mutate_field!(theta_rhythm_duty, 0.05..0.9);
    mutate_field!(theta_rhythm_drive, 0.0..20.0);
    mutate_field!(theta_rhythm_phase_jitter, 0.0..1.0);
    if rng.random_bool(rate) {
        cfg.thalamic_gating_enabled = rng.random_bool(0.5);
        changed = true;
    }
    mutate_field!(thalamic_gate_hz, 0.5..20.0);
    mutate_field!(thalamic_gate_duty, 0.05..0.95);
    mutate_field!(thalamic_gate_floor, 0.0..1.0);
    mutate_field!(energy_kernel_k, 0.01..10.0);
    mutate_field!(synaptic_energy_window_ms, 100.0..30000.0);
    mutate_field!(component_pruning_threshold, 0.001..0.2);
    mutate_field!(initial_synaptic_weight, 0.001..0.5);
    mutate_field!(synaptic_growth_threshold, 0.1..0.9);
    mutate_field!(synaptic_consolidation_factor, 0.0..1.0);
    mutate_field!(spontaneous_neuron_interval_ms, 20.0..10000.0);
    mutate_field!(neuron_removal_delay_ms, 500.0..180000.0);
    mutate_field!(max_sensory_connections, 1..=128);
    mutate_field!(max_output_connections, 1..=128);
    mutate_field!(aarnn_layer_depth, 0..=5);
    if rng.random_bool(rate) {
        cfg.use_morphology = rng.random_bool(0.7);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.izh_preset = pick_izh_preset(rng);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.stp_enabled = rng.random_bool(0.5);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.stp_u = rng.random_range(0.0..1.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.stp_tau_rec_ms = rng.random_range(10.0..5000.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.stp_tau_facil_ms = rng.random_range(10.0..2000.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.ampa_tau_ms = rng.random_range(1.0..50.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.nmda_tau_ms = rng.random_range(10.0..300.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.gaba_tau_ms = rng.random_range(1.0..50.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.nmda_ratio = rng.random_range(0.0..1.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.synaptic_gain = rng.random_range(0.1..5.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dendritic_active_enabled = rng.random_bool(0.6);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dendritic_ca_tau_ms = rng.random_range(40.0..400.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dendritic_plateau_tau_ms = rng.random_range(100.0..1200.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dendritic_ca_influx_gain = rng.random_range(0.02..0.4);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dendritic_plateau_threshold = rng.random_range(0.4..2.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dendritic_plateau_gain = rng.random_range(0.05..1.2);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.adaptive_threshold_enabled = rng.random_bool(0.5);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.adaptive_threshold_tau_ms = rng.random_range(10.0..1000.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.adaptive_threshold_increment = rng.random_range(0.0..5.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.adaptive_threshold_min = rng.random_range(-5.0..0.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.adaptive_threshold_max = rng.random_range(0.0..10.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.izh_refractory_ms = rng.random_range(0.0..10.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.homeostasis_target_rate_hz = rng.random_range(0.0..20.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.homeostasis_tau_ms = rng.random_range(100.0..10000.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.homeostasis_gain = rng.random_range(0.0..5.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.neuromodulation_enabled = rng.random_bool(0.5);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.dopamine_gain = rng.random_range(0.1..3.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.acetylcholine_gain = rng.random_range(0.1..3.0);
        changed = true;
    }
    if rng.random_bool(rate) {
        cfg.aarnn_bio.serotonin_gain = rng.random_range(0.1..3.0);
        changed = true;
    }

    // Force at least one change if none happened by chance
    if !changed {
        cfg.p_in = rng.random_range(0.05..0.5);
    }

    if cfg.max_layers < cfg.num_hidden_layers {
        cfg.max_layers = cfg.num_hidden_layers;
    }
    if cfg.aarnn_myelin_max_conduction_gain <= cfg.aarnn_myelin_min_conduction_gain {
        cfg.aarnn_myelin_max_conduction_gain = cfg.aarnn_myelin_min_conduction_gain + 0.2;
    }
    sanitize_io_layers(cfg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn test_ga_restart_seeding() {
        let mut rng = StdRng::seed_from_u64(42);
        let base_cfg = NetworkConfig::default();
        let pop_size = 10;

        // Test fresh start
        let ga_fresh = GASearch::new(pop_size, &base_cfg, &mut rng, None, false, Vec::new());
        assert_eq!(ga_fresh.population.len(), pop_size);
        // AARNN fresh run should always include the current configuration.
        assert_eq!(ga_fresh.population[0].config, base_cfg);

        // Test restart
        let mut best_cfg = base_cfg.clone();
        best_cfg.p_in = 0.99; // Set a distinctive value
        let ga_restart = GASearch::new(pop_size, &best_cfg, &mut rng, None, true, Vec::new());

        assert_eq!(ga_restart.population.len(), pop_size);
        // First individual must be exactly best_cfg
        assert_eq!(ga_restart.population[0].config.p_in, 0.99);

        // Others should be mutated from best_cfg, but many fields should remain 0.99
        let mut same_p_in_count = 0;
        for i in 1..pop_size {
            if ga_restart.population[i].config.p_in == 0.99 {
                same_p_in_count += 1;
            }
        }
        // With 0.3 mutation rate, about 70% should have the same p_in
        assert!(same_p_in_count > 0);
    }

    #[test]
    fn test_ga_leaderboard_persistence() {
        let mut rng = StdRng::seed_from_u64(42);
        let base_cfg = NetworkConfig::default();
        let mut ga = GASearch::new(5, &base_cfg, &mut rng, None, false, Vec::new());

        let mut ind = Individual::new(base_cfg.clone(), 0.85);
        ind.config.p_in = 0.123;

        ga.add_to_leaderboard(ind);
        assert_eq!(ga.leaderboard.len(), 1);
        assert_eq!(ga.best_fitness, 0.85);

        let temp_path = "test_leaderboard.json";
        ga.save_leaderboard(temp_path).unwrap();

        let mut ga2 = GASearch::new(5, &base_cfg, &mut rng, None, false, Vec::new());
        ga2.load_leaderboard(temp_path).unwrap();

        assert_eq!(ga2.leaderboard.len(), 1);
        assert_eq!(ga2.leaderboard[0].fitness, 0.85);
        assert_eq!(ga2.leaderboard[0].config.p_in, 0.123);
        assert_eq!(ga2.best_fitness, 0.85);

        std::fs::remove_file(temp_path).unwrap();
    }

    #[test]
    fn test_ga_seeds_from_leaderboard() {
        let mut rng = StdRng::seed_from_u64(7);
        let base_cfg = NetworkConfig::default();

        let mut cfg_a = base_cfg.clone();
        cfg_a.p_in = 0.11;
        let mut cfg_b = base_cfg.clone();
        cfg_b.p_in = 0.22;

        let leaderboard = vec![
            Individual::new(cfg_a.clone(), 0.9),
            Individual::new(cfg_b.clone(), 0.8),
        ];

        let ga = GASearch::new(6, &base_cfg, &mut rng, None, true, leaderboard);
        assert!(ga.population.iter().any(|ind| ind.config == base_cfg));
        assert!(ga.population.iter().any(|ind| ind.config == cfg_a));
        assert!(ga.population.iter().any(|ind| ind.config == cfg_b));
    }

    #[test]
    fn test_ga_skips_non_aarnn_seed() {
        let mut rng = StdRng::seed_from_u64(123);
        let mut base_cfg = NetworkConfig::default();
        base_cfg.use_morphology = false;

        let ga = GASearch::new(8, &base_cfg, &mut rng, None, false, Vec::new());
        assert!(ga.population.iter().all(|ind| ind.config != base_cfg));
    }

    #[test]
    fn test_randomize_config() {
        let mut rng = StdRng::seed_from_u64(42);
        let base = NetworkConfig::default();
        let rand_cfg = randomize_config(&base, &mut rng);
        assert_ne!(base, rand_cfg);
    }

    #[test]
    fn test_crossover() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut p1 = NetworkConfig::default();
        p1.p_in = 0.1;
        let mut p2 = NetworkConfig::default();
        p2.p_in = 0.9;

        let child = crossover(&p1, &p2, &mut rng);
        assert!(child.p_in == 0.1 || child.p_in == 0.9);
    }

    #[test]
    fn test_mutate() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut cfg = NetworkConfig::default();
        let original = cfg.clone();
        mutate(&mut cfg, 1.0, &mut rng); // High mutation rate
        assert_ne!(original, cfg);
    }

    #[test]
    fn test_apply_clumping_design_keep_max_total_preserves_limit() {
        let mut cfg = NetworkConfig::default();
        cfg.max_total_neurons = 1234;
        apply_clumping_design_keep_max_total(&mut cfg, ClumpingDesign::FruitFly);
        assert_eq!(cfg.clumping_design, ClumpingDesign::FruitFly);
        assert!(!cfg.brain_regions.is_empty());
        assert_eq!(cfg.max_total_neurons, 1234);
    }

    #[test]
    fn test_mutate_extended_aarnn_ranges() {
        let mut rng = StdRng::seed_from_u64(1234);
        let mut cfg = NetworkConfig::default();
        mutate(&mut cfg, 1.0, &mut rng);

        assert!((0.0..=1.0).contains(&cfg.perceptual_prediction_lr));
        assert!((0.0..=0.5).contains(&cfg.perceptual_prediction_decay));
        assert!((0.0..=20.0).contains(&cfg.perceptual_error_gain));
        assert!((2..=32).contains(&cfg.world_model_dim));
        assert!((1000.0..=600000.0).contains(&cfg.sleep_cycle_ms));
        assert!((0.5..=12.0).contains(&cfg.theta_rhythm_hz));
        assert!((0.5..=20.0).contains(&cfg.thalamic_gate_hz));
        assert!((0.0..=3.0).contains(&cfg.aarnn_neuromod_baseline_dopamine));
        assert!((0.0..=3.0).contains(&cfg.aarnn_neuromod_baseline_ach));
        assert!((0.0..=3.0).contains(&cfg.aarnn_neuromod_baseline_serotonin));
        assert!((0.05..=1.0).contains(&cfg.columnar_spacing));
        assert!((0.03..=0.4).contains(&cfg.aarnn_gap_junction_radius));
        assert!((0.1..=0.8).contains(&cfg.volume_transmission_radius));
        assert!((0.0..=0.3).contains(&cfg.volume_transmission_strength));
        assert!((0.0001..=0.01).contains(&cfg.aarnn_myelination_rate));
        assert!((0.00005..=0.006).contains(&cfg.aarnn_demyelination_rate));
        assert!((0.03..=0.3).contains(&cfg.aarnn_myelination_activity_target));
        assert!((0.6..=1.0).contains(&cfg.aarnn_myelin_min_conduction_gain));
        assert!((0.8..=3.2).contains(&cfg.aarnn_myelin_max_conduction_gain));
        assert!(cfg.aarnn_myelin_max_conduction_gain > cfg.aarnn_myelin_min_conduction_gain);
        assert!((0.1..=0.8).contains(&cfg.aarnn_myelin_initial));
        assert!((40.0..=400.0).contains(&cfg.aarnn_bio.dendritic_ca_tau_ms));
        assert!((100.0..=1200.0).contains(&cfg.aarnn_bio.dendritic_plateau_tau_ms));
        assert!((0.02..=0.4).contains(&cfg.aarnn_bio.dendritic_ca_influx_gain));
        assert!((0.4..=2.0).contains(&cfg.aarnn_bio.dendritic_plateau_threshold));
        assert!((0.05..=1.2).contains(&cfg.aarnn_bio.dendritic_plateau_gain));
    }
}
