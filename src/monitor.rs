use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "sysinfo")]
use sysinfo::{Components, ProcessRefreshKind, ProcessesToUpdate, System};

// use crate::obs;

#[cfg(feature = "sysinfo")]
fn sys_mb_from_raw(raw: u64) -> u64 {
    if raw > 1_000_000_000 {
        raw / 1024 / 1024
    } else {
        raw / 1024
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorHeuristics {
    pub temp_warn_c: f32,
    pub temp_hot_c: f32,
    pub mem_free_min_mb: u64,
    pub mem_rss_warn_mb: u64,
    pub mem_rss_abort_mb: u64,
    pub mem_rss_growth_warn_mb: u64,
    pub mem_rss_growth_abort_mb: u64,
    pub gpu_util_warn_pct: f32,
    pub gpu_util_hot_pct: f32,
    pub gpu_vram_free_min_mb: u64,
}

impl Default for MonitorHeuristics {
    fn default() -> Self {
        Self {
            temp_warn_c: 80.0,
            temp_hot_c: 90.0,
            mem_free_min_mb: 1024,
            mem_rss_warn_mb: 16384,
            mem_rss_abort_mb: 24576,
            mem_rss_growth_warn_mb: 4096,
            mem_rss_growth_abort_mb: 8192,
            gpu_util_warn_pct: 90.0,
            gpu_util_hot_pct: 98.0,
            gpu_vram_free_min_mb: 512,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SafetySnapshot {
    pub cpu_usage_pct: Option<f32>,
    pub mem_free_mb: Option<u64>,
    pub proc_rss_mb: Option<u64>,
    pub proc_rss_growth_mb: Option<u64>,
    pub total_mem_mb: Option<u64>,
    pub temp_c: Option<f32>,
    pub gpu_util_pct: Option<f32>,
    pub gpu_vram_free_mb: Option<u64>,
    pub ui_frame_ms: Option<f32>,
}

struct TempCache {
    last_update: Instant,
    max_c: Option<f32>,
}

struct SysCache {
    last_update: Instant,
    cpu_usage_pct: Option<f32>,
    mem_free_mb: Option<u64>,
    proc_rss_mb: Option<u64>,
    total_mem_mb: Option<u64>,
}

struct GpuCache {
    last_update: Instant,
    gpu_util_pct: Option<f32>,
    gpu_vram_free_mb: Option<u64>,
}

static TEMP_CACHE: OnceLock<Mutex<TempCache>> = OnceLock::new();
static SYS_CACHE: OnceLock<Mutex<SysCache>> = OnceLock::new();
static GPU_CACHE: OnceLock<Mutex<GpuCache>> = OnceLock::new();
static GPU_UPDATE_INFLIGHT: AtomicBool = AtomicBool::new(false);

pub fn update_temp_cache() -> Option<f32> {
    #[cfg(feature = "sysinfo")]
    {
        let cache = TEMP_CACHE.get_or_init(|| Mutex::new(TempCache {
            last_update: Instant::now() - Duration::from_secs(10),
            max_c: None,
        }));

        {
            let guard = cache.lock().expect("Temperature cache poisoned");
            if guard.last_update.elapsed() < Duration::from_secs(2) {
                return guard.max_c;
            }
        }

        let mut components = Components::new_with_refreshed_list();
        components.refresh(false);
        let mut max_c = None;
        for component in &components {
            if let Some(temp) = component.temperature() {
                if temp.is_finite() {
                    max_c = Some(max_c.map_or(temp, |prev: f32| prev.max(temp)));
                }
            }
        }

        let mut guard = cache.lock().expect("Temperature cache poisoned");
        guard.last_update = Instant::now();
        guard.max_c = max_c;
        max_c
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        None
    }
}

pub fn update_sys_cache() -> (Option<f32>, Option<u64>, Option<u64>, Option<u64>) {
    #[cfg(feature = "sysinfo")]
    {
        let cache = SYS_CACHE.get_or_init(|| Mutex::new(SysCache {
            last_update: Instant::now() - Duration::from_secs(10),
            cpu_usage_pct: None,
            mem_free_mb: None,
            proc_rss_mb: None,
            total_mem_mb: None,
        }));
        {
            let guard = cache.lock().expect("System cache poisoned");
            if guard.last_update.elapsed() < Duration::from_secs(1) {
                return (guard.cpu_usage_pct, guard.mem_free_mb, guard.proc_rss_mb, guard.total_mem_mb);
            }
        }
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let cpu = sys.global_cpu_usage();
        let total_raw = sys.total_memory() as u64;
        let scale_is_bytes = total_raw > 1_000_000_000;
        let total_mb = sys_mb_from_raw(total_raw);
        let free_raw = sys.available_memory() as u64;
        let free_mb = sys_mb_from_raw(free_raw);
        let rss_mb = sysinfo::get_current_pid()
            .ok()
            .and_then(|pid| {
                sys.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    true,
                    ProcessRefreshKind::nothing().with_memory(),
                );
                sys.process(pid).map(|p| {
                    let raw = p.memory() as u64;
                    if scale_is_bytes { raw / 1024 / 1024 } else { raw / 1024 }
                })
            });
        let mut guard = cache.lock().expect("System cache poisoned");
        guard.last_update = Instant::now();
        guard.cpu_usage_pct = Some(cpu);
        guard.mem_free_mb = Some(free_mb);
        guard.proc_rss_mb = rss_mb;
        guard.total_mem_mb = Some(total_mb);
        (guard.cpu_usage_pct, guard.mem_free_mb, guard.proc_rss_mb, guard.total_mem_mb)
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        (None, None, None, None)
    }
}

pub fn update_gpu_cache() -> (Option<f32>, Option<u64>) {
    let cache = GPU_CACHE.get_or_init(|| Mutex::new(GpuCache {
        last_update: Instant::now() - Duration::from_secs(10),
        gpu_util_pct: None,
        gpu_vram_free_mb: None,
    }));
    {
        let guard = cache.lock().expect("GPU cache poisoned");
        if guard.last_update.elapsed() < Duration::from_secs(2) {
            return (guard.gpu_util_pct, guard.gpu_vram_free_mb);
        }
    }
    if GPU_UPDATE_INFLIGHT.swap(true, Ordering::SeqCst) {
        let guard = cache.lock().expect("GPU cache poisoned");
        return (guard.gpu_util_pct, guard.gpu_vram_free_mb);
    }

    std::thread::spawn(|| {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=utilization.gpu,memory.free",
                "--format=csv,noheader,nounits",
            ])
            .output();
        let mut util = None;
        let mut free_mb = None;
        if let Ok(out) = output {
            if out.status.success() {
                if let Ok(text) = String::from_utf8(out.stdout) {
                    let parts: Vec<&str> = text.split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 2 {
                        util = parts[0].parse::<f32>().ok();
                        free_mb = parts[1].parse::<u64>().ok();
                    }
                }
            }
        }
        let cache = GPU_CACHE.get_or_init(|| Mutex::new(GpuCache {
            last_update: Instant::now() - Duration::from_secs(10),
            gpu_util_pct: None,
            gpu_vram_free_mb: None,
        }));
        let mut guard = cache.lock().expect("GPU cache poisoned");
        guard.last_update = Instant::now();
        guard.gpu_util_pct = util;
        guard.gpu_vram_free_mb = free_mb;
        GPU_UPDATE_INFLIGHT.store(false, Ordering::SeqCst);
    });

    let guard = cache.lock().expect("GPU cache poisoned");
    (guard.gpu_util_pct, guard.gpu_vram_free_mb)
}

pub fn get_safety_snapshot(ui_frame_ms: Option<f32>) -> SafetySnapshot {
    let (cpu, free, rss, total) = update_sys_cache();
    let temp = update_temp_cache();
    let (gpu_util, gpu_vram) = update_gpu_cache();
    
    SafetySnapshot {
        cpu_usage_pct: cpu,
        mem_free_mb: free,
        proc_rss_mb: rss,
        proc_rss_growth_mb: None, // This needs to be tracked externally if needed
        total_mem_mb: total,
        temp_c: temp,
        gpu_util_pct: gpu_util,
        gpu_vram_free_mb: gpu_vram,
        ui_frame_ms,
    }
}

pub async fn thermal_wait_if_hot(kind: &str, h: &MonitorHeuristics, abort_flag: &AtomicBool) -> Duration {
    let Some(mut temp) = update_temp_cache() else { return Duration::ZERO; };
    if temp < h.temp_hot_c {
        return Duration::ZERO;
    }
    let wait_start = Instant::now();
    crate::nm_log!(
        "[info] {} queued; temperature {:.1}C >= {:.1}C. Waiting to cool.",
        kind,
        temp,
        h.temp_hot_c
    );
    let mut cool_start: Option<Instant> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }
        if let Some(latest) = update_temp_cache() {
            temp = latest;
            if temp <= h.temp_warn_c {
                let start = cool_start.get_or_insert_with(Instant::now);
                if start.elapsed() >= Duration::from_secs(5) {
                    break;
                }
            } else {
                cool_start = None;
            }
        } else {
            break;
        }
    }
    wait_start.elapsed()
}

pub fn thermal_wait_blocking(kind: &str, h: &MonitorHeuristics, abort_flag: &AtomicBool) -> Duration {
    let Some(mut temp) = update_temp_cache() else { return Duration::ZERO; };
    if temp < h.temp_hot_c {
        return Duration::ZERO;
    }
    let wait_start = Instant::now();
    crate::nm_log!(
        "[info] {} throttled; temperature {:.1}C >= {:.1}C. Waiting to cool.",
        kind,
        temp,
        h.temp_hot_c
    );
    let mut cool_start: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }
        if let Some(latest) = update_temp_cache() {
            temp = latest;
            if temp <= h.temp_warn_c {
                let start = cool_start.get_or_insert_with(Instant::now);
                if start.elapsed() >= Duration::from_secs(5) {
                    break;
                }
            } else {
                cool_start = None;
            }
        } else {
            break;
        }
    }
    wait_start.elapsed()
}
