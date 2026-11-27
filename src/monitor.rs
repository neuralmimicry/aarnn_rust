// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
        let cache = TEMP_CACHE.get_or_init(|| {
            Mutex::new(TempCache {
                last_update: Instant::now() - Duration::from_secs(10),
                max_c: None,
            })
        });

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
        let cache = SYS_CACHE.get_or_init(|| {
            Mutex::new(SysCache {
                last_update: Instant::now() - Duration::from_secs(10),
                cpu_usage_pct: None,
                mem_free_mb: None,
                proc_rss_mb: None,
                total_mem_mb: None,
            })
        });
        {
            let guard = cache.lock().expect("System cache poisoned");
            if guard.last_update.elapsed() < Duration::from_secs(1) {
                return (
                    guard.cpu_usage_pct,
                    guard.mem_free_mb,
                    guard.proc_rss_mb,
                    guard.total_mem_mb,
                );
            }
        }
        // One long-lived System: sysinfo derives CPU usage from the change
        // between two refreshes of the same object, so a fresh System per
        // call always reported 0 % (every CPU-based back-off was inert).
        static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
        let mut sys = SYSTEM
            .get_or_init(|| {
                let mut s = System::new();
                s.refresh_cpu_usage();
                Mutex::new(s)
            })
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let cpu = sys.global_cpu_usage();
        let total_raw = sys.total_memory() as u64;
        let scale_is_bytes = total_raw > 1_000_000_000;
        let total_mb = sys_mb_from_raw(total_raw);
        let free_raw = sys.available_memory() as u64;
        let free_mb = sys_mb_from_raw(free_raw);
        let rss_mb = sysinfo::get_current_pid().ok().and_then(|pid| {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_memory(),
            );
            sys.process(pid).map(|p| {
                let raw = p.memory() as u64;
                if scale_is_bytes {
                    raw / 1024 / 1024
                } else {
                    raw / 1024
                }
            })
        });
        let mut guard = cache.lock().expect("System cache poisoned");
        guard.last_update = Instant::now();
        guard.cpu_usage_pct = Some(cpu);
        guard.mem_free_mb = Some(free_mb);
        guard.proc_rss_mb = rss_mb;
        guard.total_mem_mb = Some(total_mb);
        (
            guard.cpu_usage_pct,
            guard.mem_free_mb,
            guard.proc_rss_mb,
            guard.total_mem_mb,
        )
    }
    #[cfg(not(feature = "sysinfo"))]
    {
        (None, None, None, None)
    }
}

pub fn update_gpu_cache() -> (Option<f32>, Option<u64>) {
    let cache = GPU_CACHE.get_or_init(|| {
        Mutex::new(GpuCache {
            last_update: Instant::now() - Duration::from_secs(10),
            gpu_util_pct: None,
            gpu_vram_free_mb: None,
        })
    });
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
        let cache = GPU_CACHE.get_or_init(|| {
            Mutex::new(GpuCache {
                last_update: Instant::now() - Duration::from_secs(10),
                gpu_util_pct: None,
                gpu_vram_free_mb: None,
            })
        });
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

pub async fn thermal_wait_if_hot(
    kind: &str,
    h: &MonitorHeuristics,
    abort_flag: &AtomicBool,
) -> Duration {
    let Some(mut temp) = update_temp_cache() else {
        return Duration::ZERO;
    };
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

pub fn thermal_wait_blocking(
    kind: &str,
    h: &MonitorHeuristics,
    abort_flag: &AtomicBool,
) -> Duration {
    let Some(mut temp) = update_temp_cache() else {
        return Duration::ZERO;
    };
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

// ---------------------------------------------------------------------------
// Resource sample and pressure level (Linux /proc and cgroup v2/v1).
//
// One sample serves the orchestrator, every node and the UIs. It reads the
// process's own cgroup (from /proc/self/cgroup, walking up to the tightest
// ancestor limit, e.g. a `systemd-run -p MemoryMax=` scope), so a process
// that the host or an operator capped paces itself against that cap and not
// against the host's free memory alone.
// ---------------------------------------------------------------------------

/// Memory pressure as seen by this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum PressureLevel {
    #[default]
    Normal,
    /// Headroom below the warning band: shed optional work, slow down.
    Warn,
    /// Headroom below the reserve: stop growing, checkpoint, refuse work.
    Critical,
}

impl PressureLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PressureLevel::Normal => "normal",
            PressureLevel::Warn => "warn",
            PressureLevel::Critical => "critical",
        }
    }
    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceSample {
    /// Resident set of this process (VmRSS) and its peak (VmHWM).
    pub rss_bytes: Option<u64>,
    pub rss_peak_bytes: Option<u64>,
    /// Host memory (MemTotal / MemAvailable).
    pub host_total_bytes: Option<u64>,
    pub host_available_bytes: Option<u64>,
    /// Tightest cgroup memory limit on this process's path and its usage.
    pub cgroup_limit_bytes: Option<u64>,
    pub cgroup_current_bytes: Option<u64>,
    /// Bytes this process may still allocate before the tighter of the
    /// cgroup limit and host memory runs out.
    pub headroom_bytes: Option<u64>,
    /// Effective limit: min(cgroup limit, host total).
    pub limit_bytes: Option<u64>,
    /// Kernel pressure stall information, memory "some" avg10 (%).
    pub psi_memory_some_avg10: Option<f32>,
    pub load1: Option<f32>,
    pub threads: Option<u32>,
    pub cpu_count: Option<u32>,
    pub level: PressureLevel,
}

impl ResourceSample {
    pub fn headroom_mb(&self) -> Option<u64> {
        self.headroom_bytes.map(|b| b / (1024 * 1024))
    }
    pub fn summary(&self) -> String {
        let mb = |b: Option<u64>| {
            b.map(|v| format!("{} MB", v / (1024 * 1024)))
                .unwrap_or("?".into())
        };
        format!(
            "{} (rss {}, headroom {}, limit {}, psi {:?}, load {:?})",
            self.level.as_str(),
            mb(self.rss_bytes),
            mb(self.headroom_bytes),
            mb(self.limit_bytes),
            self.psi_memory_some_avg10,
            self.load1
        )
    }
}

/// Reserve (MB) kept free for the host and the other processes; below it the
/// level is critical, below twice it (or 10 % of the limit) it is a warning.
/// `NM_MEM_RESERVE_MB` (default 1024).
pub fn memory_reserve_bytes() -> u64 {
    std::env::var("NM_MEM_RESERVE_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(1024)
        .saturating_mul(1024 * 1024)
}

fn read_kv_kb(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.trim_start_matches(':').trim();
        let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        Some(value.saturating_mul(1024))
    })
}

fn read_u64_file(path: &std::path::Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let t = text.trim();
    if t == "max" {
        return None;
    }
    t.parse::<u64>().ok()
}

/// (limit, current) of the tightest memory limit on this process's cgroup
/// path (cgroup v2 unified hierarchy, or the v1 memory controller).
fn cgroup_memory() -> (Option<u64>, Option<u64>) {
    let Ok(table) = std::fs::read_to_string("/proc/self/cgroup") else {
        return (None, None);
    };
    let root = std::path::Path::new("/sys/fs/cgroup");
    // v2: "0::/user.slice/.../run-xyz.scope"
    if let Some(rel) = table.lines().find_map(|l| l.strip_prefix("0::")) {
        let mut dir = root.join(rel.trim_start_matches('/'));
        let current = read_u64_file(&dir.join("memory.current"));
        let mut limit: Option<u64> = None;
        loop {
            for file in ["memory.max", "memory.high"] {
                if let Some(v) = read_u64_file(&dir.join(file)) {
                    limit = Some(limit.map_or(v, |l| l.min(v)));
                }
            }
            if dir == root || !dir.pop() || !dir.starts_with(root) {
                break;
            }
        }
        return (limit, current);
    }
    // v1: "N:memory:/path"
    if let Some(rel) = table
        .lines()
        .find_map(|l| l.split_once(":memory:").map(|(_, p)| p.to_string()))
    {
        let dir = root.join("memory").join(rel.trim_start_matches('/'));
        let limit = read_u64_file(&dir.join("memory.limit_in_bytes")).filter(|v| *v < (1u64 << 60));
        let current = read_u64_file(&dir.join("memory.usage_in_bytes"));
        return (limit, current);
    }
    (None, None)
}

/// Classify headroom against the reserve and the effective limit.
pub fn classify_pressure(
    headroom: Option<u64>,
    limit: Option<u64>,
    psi_avg10: Option<f32>,
) -> PressureLevel {
    let reserve = memory_reserve_bytes();
    let Some(h) = headroom else {
        return PressureLevel::Normal;
    };
    let warn_band = reserve
        .saturating_mul(2)
        .max(limit.map(|l| l / 10).unwrap_or(0));
    if h < reserve {
        PressureLevel::Critical
    } else if h < warn_band || psi_avg10.is_some_and(|p| p >= 10.0) {
        PressureLevel::Warn
    } else {
        PressureLevel::Normal
    }
}

fn sample_now() -> ResourceSample {
    let mut out = ResourceSample::default();
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        out.rss_bytes = read_kv_kb(&status, "VmRSS");
        out.rss_peak_bytes = read_kv_kb(&status, "VmHWM");
        out.threads = status.lines().find_map(|l| {
            l.strip_prefix("Threads:")
                .and_then(|v| v.trim().parse::<u32>().ok())
        });
    }
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        out.host_total_bytes = read_kv_kb(&meminfo, "MemTotal");
        out.host_available_bytes = read_kv_kb(&meminfo, "MemAvailable");
    }
    let (cg_limit, cg_current) = cgroup_memory();
    out.cgroup_limit_bytes = cg_limit;
    out.cgroup_current_bytes = cg_current;
    out.limit_bytes = match (cg_limit, out.host_total_bytes) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let cg_headroom = match (cg_limit, cg_current.or(out.rss_bytes)) {
        (Some(l), Some(c)) => Some(l.saturating_sub(c)),
        _ => None,
    };
    out.headroom_bytes = match (cg_headroom, out.host_available_bytes) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    out.psi_memory_some_avg10 = std::fs::read_to_string("/proc/pressure/memory")
        .ok()
        .and_then(|t| {
            let line = t.lines().find(|l| l.starts_with("some"))?;
            line.split_whitespace()
                .find_map(|f| f.strip_prefix("avg10="))
                .and_then(|v| v.parse::<f32>().ok())
        });
    out.load1 = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|t| t.split_whitespace().next()?.parse::<f32>().ok());
    out.cpu_count = std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u32);
    out.level = classify_pressure(
        out.headroom_bytes,
        out.limit_bytes,
        out.psi_memory_some_avg10,
    );
    out
}

/// Current resource sample, cached for 500 ms (cheap /proc reads).
pub fn resource_sample() -> ResourceSample {
    static CACHE: OnceLock<Mutex<(Instant, ResourceSample)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        Mutex::new((
            Instant::now() - Duration::from_secs(10),
            ResourceSample::default(),
        ))
    });
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if guard.0.elapsed() >= Duration::from_millis(500) {
        *guard = (Instant::now(), sample_now());
    }
    guard.1.clone()
}

/// Free bytes on the filesystem holding `path` (or its nearest existing
/// ancestor, for a file about to be created).
pub fn disk_free_bytes(path: &std::path::Path) -> Option<u64> {
    let mut probe = path.to_path_buf();
    loop {
        if probe.exists() {
            return fs2::available_space(&probe).ok();
        }
        if !probe.pop() {
            return fs2::available_space(std::path::Path::new(".")).ok();
        }
    }
}

/// Refuse a write of about `bytes` to `path` that would leave less than
/// `NM_DISK_RESERVE_MB` (default 2048) free.
pub fn check_disk_for_write(path: &std::path::Path, bytes: u64) -> Result<(), String> {
    let reserve = std::env::var("NM_DISK_RESERVE_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(2048)
        .saturating_mul(1024 * 1024);
    match disk_free_bytes(path) {
        Some(free) if free < bytes.saturating_add(reserve) => Err(format!(
            "refusing to write ~{} MB to {}: {} MB free, {} MB reserve (NM_DISK_RESERVE_MB)",
            bytes / (1024 * 1024),
            path.display(),
            free / (1024 * 1024),
            reserve / (1024 * 1024)
        )),
        _ => Ok(()),
    }
}

/// Admission check before allocating about `bytes`: the allocation must fit
/// in the current headroom while keeping the reserve.
pub fn admit_allocation(what: &str, bytes: u64) -> Result<ResourceSample, String> {
    let sample = resource_sample();
    if std::env::var("NM_ADMISSION_CONTROL")
        .is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
    {
        return Ok(sample);
    }
    if let Some(headroom) = sample.headroom_bytes {
        let reserve = memory_reserve_bytes();
        if bytes.saturating_add(reserve) > headroom {
            return Err(format!(
                "{what}: needs ~{} MB but only {} MB of memory headroom remain (reserve {} MB; limit {}). \
                 Free memory, use a lighter configuration, shard across more nodes, or set NM_ADMISSION_CONTROL=0.",
                bytes / (1024 * 1024),
                headroom / (1024 * 1024),
                reserve / (1024 * 1024),
                sample
                    .limit_bytes
                    .map(|l| format!("{} MB", l / (1024 * 1024)))
                    .unwrap_or("unknown".into())
            ));
        }
    }
    Ok(sample)
}

#[cfg(test)]
mod resource_tests {
    use super::*;

    #[test]
    fn pressure_levels_follow_headroom_against_the_reserve() {
        let gb = 1024 * 1024 * 1024u64;
        // Default reserve 1 GB; warning band max(2 GB, 10 % of the limit).
        assert_eq!(
            classify_pressure(Some(20 * gb), Some(60 * gb), None),
            PressureLevel::Normal
        );
        assert_eq!(
            classify_pressure(Some(5 * gb), Some(60 * gb), None),
            PressureLevel::Warn
        );
        assert_eq!(
            classify_pressure(Some(gb / 2), Some(60 * gb), None),
            PressureLevel::Critical
        );
        assert_eq!(
            classify_pressure(Some(20 * gb), Some(60 * gb), Some(25.0)),
            PressureLevel::Warn
        );
        assert_eq!(classify_pressure(None, None, None), PressureLevel::Normal);
    }

    #[test]
    fn a_live_sample_reads_this_process() {
        let s = resource_sample();
        eprintln!(
            "resource sample: {} cgroup limit {:?}",
            s.summary(),
            s.cgroup_limit_bytes
        );
        if cfg!(target_os = "linux") {
            assert!(s.rss_bytes.unwrap_or(0) > 0);
            assert!(s.host_total_bytes.unwrap_or(0) > 0);
            assert!(s.headroom_bytes.is_some());
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-pacing: a step loop (a whole network or one node's shard) asks its
// Pacer before each step. Under memory pressure it sheds optional structural
// work (growth), slows down, holds, and as a last resort pauses the network
// (state kept in memory, resumable) instead of being OOM-killed or taking the
// host down with it. `NM_RESOURCE_PACING=0` disables it.
// ---------------------------------------------------------------------------

static SHED_OPTIONAL: AtomicBool = AtomicBool::new(false);

/// True while memory pressure asks step loops to skip optional structural
/// work (growth spawns), which is what grows a network's memory.
pub fn shed_optional_work() -> bool {
    SHED_OPTIONAL.load(Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaceAction {
    /// Step now.
    Continue,
    /// Sleep this long, then step (warning band: slower stepping).
    Delay(Duration),
    /// Sleep this long and do not step (critical: wait for headroom).
    Hold(Duration),
    /// Critical for longer than the grace period: pause the network.
    Pause(String),
}

pub struct Pacer {
    label: String,
    enabled: bool,
    last_check: Instant,
    level: PressureLevel,
    delay: Duration,
    max_delay: Duration,
    grace: Duration,
    critical_since: Option<Instant>,
}

impl Pacer {
    pub fn new(label: impl Into<String>) -> Self {
        let env_ms = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(default)
        };
        Self {
            label: label.into(),
            enabled: !std::env::var("NM_RESOURCE_PACING")
                .is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false")),
            last_check: Instant::now() - Duration::from_secs(10),
            level: PressureLevel::Normal,
            delay: Duration::ZERO,
            max_delay: Duration::from_millis(env_ms("NM_PACE_MAX_DELAY_MS", 50)),
            grace: Duration::from_secs(env_ms("NM_PRESSURE_CRITICAL_GRACE_S", 30)),
            critical_since: None,
        }
    }

    pub fn level(&self) -> PressureLevel {
        self.level
    }

    /// Decide the next step from the current resource sample (re-read at most
    /// every 250 ms; between reads the last decision's pacing holds).
    pub fn tick(&mut self) -> PaceAction {
        if !self.enabled {
            return PaceAction::Continue;
        }
        if self.last_check.elapsed() >= Duration::from_millis(250) {
            self.last_check = Instant::now();
            let sample = resource_sample();
            self.apply(sample.level, &sample.summary());
        }
        self.action()
    }

    /// State transition for a measured level (separate for tests).
    pub fn apply(&mut self, level: PressureLevel, summary: &str) {
        if level != self.level {
            match level {
                PressureLevel::Normal => crate::nm_log!(
                    "[info] {}: memory pressure cleared, full speed and growth resumed: {}",
                    self.label,
                    summary
                ),
                PressureLevel::Warn => crate::nm_log!(
                    "[warn] {}: memory pressure, slowing steps and pausing structural growth: {}",
                    self.label,
                    summary
                ),
                PressureLevel::Critical => crate::nm_err!(
                    "[warn] {}: critical memory pressure, holding steps until headroom returns: {}",
                    self.label,
                    summary
                ),
            }
        }
        self.level = level;
        match level {
            PressureLevel::Normal => {
                self.delay = Duration::ZERO;
                self.critical_since = None;
                SHED_OPTIONAL.store(false, Ordering::Relaxed);
            }
            PressureLevel::Warn => {
                self.critical_since = None;
                self.delay = (self.delay * 2)
                    .max(Duration::from_millis(1))
                    .min(self.max_delay);
                SHED_OPTIONAL.store(true, Ordering::Relaxed);
            }
            PressureLevel::Critical => {
                self.critical_since.get_or_insert_with(Instant::now);
                self.delay = self.max_delay;
                SHED_OPTIONAL.store(true, Ordering::Relaxed);
            }
        }
    }

    fn action(&mut self) -> PaceAction {
        match self.level {
            PressureLevel::Normal => PaceAction::Continue,
            PressureLevel::Warn => PaceAction::Delay(self.delay),
            PressureLevel::Critical => {
                let since = self.critical_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= self.grace {
                    self.critical_since = None;
                    PaceAction::Pause(format!(
                        "{}: memory critical for {} s; paused (state kept, resume once memory is freed)",
                        self.label,
                        self.grace.as_secs()
                    ))
                } else {
                    PaceAction::Hold(Duration::from_millis(500))
                }
            }
        }
    }
}

#[cfg(test)]
mod pacer_tests {
    use super::*;

    #[test]
    fn pacer_slows_holds_and_pauses_under_pressure_then_recovers() {
        let mut p = Pacer::new("test");
        p.enabled = true;
        p.grace = Duration::ZERO;
        p.apply(PressureLevel::Normal, "");
        assert_eq!(p.action(), PaceAction::Continue);
        p.apply(PressureLevel::Warn, "");
        assert_eq!(p.action(), PaceAction::Delay(Duration::from_millis(1)));
        p.apply(PressureLevel::Warn, "");
        assert_eq!(p.action(), PaceAction::Delay(Duration::from_millis(2)));
        assert!(shed_optional_work());
        p.apply(PressureLevel::Critical, "");
        assert!(matches!(p.action(), PaceAction::Pause(_)));
        p.apply(PressureLevel::Normal, "");
        assert_eq!(p.action(), PaceAction::Continue);
        assert!(!shed_optional_work());
    }
}
