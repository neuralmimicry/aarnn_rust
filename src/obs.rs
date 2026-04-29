use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Global flag to suppress all console/logging output.
pub static SILENT: AtomicBool = AtomicBool::new(false);

static LOG_FILE: OnceLock<Mutex<BufWriter<std::fs::File>>> = OnceLock::new();
static LOG_STATE: OnceLock<Mutex<LogState>> = OnceLock::new();

struct LogState {
    path: PathBuf,
    max_bytes: u64,
    max_files: usize,
    check_every: u64,
    writes: u64,
}

#[inline]
pub fn is_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

pub fn init_log_file(path: &Path) -> std::io::Result<()> {
    if is_silent() {
        return Ok(());
    }
    if LOG_FILE.get().is_some() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let _ = LOG_FILE.set(Mutex::new(BufWriter::new(file)));
    let max_bytes = std::env::var("NM_LOG_ROTATE_BYTES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20 * 1024 * 1024);
    let max_files = std::env::var("NM_LOG_ROTATE_COUNT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1);
    let _ = LOG_STATE.set(Mutex::new(LogState {
        path: path.to_path_buf(),
        max_bytes,
        max_files,
        check_every: 50,
        writes: 0,
    }));
    Ok(())
}

pub fn flush_log() {
    if let Some(lock) = LOG_FILE.get() {
        if let Ok(mut guard) = lock.lock() {
            let _ = guard.flush();
        }
    }
}

fn rotate_logs_if_needed() {
    let Some(state_lock) = LOG_STATE.get() else {
        return;
    };
    let mut rotate = false;
    let (path, max_files, max_bytes) = {
        let mut state = state_lock.lock().expect("log state lock poisoned");
        state.writes = state.writes.saturating_add(1);
        if state.max_bytes > 0 && state.writes % state.check_every == 0 {
            if let Ok(meta) = std::fs::metadata(&state.path) {
                if meta.len() >= state.max_bytes {
                    rotate = true;
                }
            }
        }
        (state.path.clone(), state.max_files, state.max_bytes)
    };
    if !rotate || max_bytes == 0 {
        return;
    }
    if let Some(lock) = LOG_FILE.get() {
        if let Ok(mut guard) = lock.lock() {
            let _ = guard.flush();
        }
    }
    for idx in (1..=max_files).rev() {
        let src = if idx == 1 {
            path.clone()
        } else {
            PathBuf::from(format!("{}.{}", path.display(), idx - 1))
        };
        let dst = PathBuf::from(format!("{}.{}", path.display(), idx));
        if dst.exists() {
            let _ = std::fs::remove_file(&dst);
        }
        if src.exists() {
            let _ = std::fs::rename(&src, &dst);
        }
    }
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        if let Some(lock) = LOG_FILE.get() {
            if let Ok(mut guard) = lock.lock() {
                *guard = BufWriter::new(file);
            }
        }
    }
}

pub(crate) fn log_to_file(is_err: bool, msg: &str) {
    if is_silent() {
        return;
    }
    let Some(lock) = LOG_FILE.get() else {
        return;
    };
    rotate_logs_if_needed();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let prefix = if is_err { "ERR" } else { "OUT" };
    let line = format!(
        "[{}.{:03}] {} {}",
        now.as_secs(),
        now.subsec_millis(),
        prefix,
        msg
    );
    if let Ok(mut guard) = lock.lock() {
        let _ = writeln!(guard, "{}", line);
    }
}

/// Represents a single recorded performance metric.
pub struct Metric {
    /// Number of times this metric was recorded.
    pub count: u64,
    /// Total accumulated time across all hits.
    pub total_time: Duration,
    /// Minimum time recorded for a single hit.
    pub min_time: Duration,
    /// Maximum time recorded for a single hit.
    pub max_time: Duration,
}

/// Global registry for tracking and reporting performance metrics.
///
/// This structure provides a thread-safe way to collect timing and hit data
/// from across the entire application and periodically log a summary report.
pub struct Metrics {
    data: Mutex<HashMap<&'static str, Metric>>,
    last_report: Mutex<Instant>,
    report_interval: Duration,
}

impl Metrics {
    /// Provides access to the global singleton instance of the Metrics registry.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<Metrics> = OnceLock::new();
        INSTANCE.get_or_init(|| Self {
            data: Mutex::new(HashMap::new()),
            last_report: Mutex::new(Instant::now()),
            report_interval: Duration::from_secs(2),
        })
    }

    /// Records a timing duration for a specific named metric.
    pub fn record(&self, name: &'static str, duration: Duration) {
        let mut data = self.data.lock().unwrap();
        let entry = data.entry(name).or_insert(Metric {
            count: 0,
            total_time: Duration::ZERO,
            min_time: Duration::from_secs(3600 * 24),
            max_time: Duration::ZERO,
        });
        entry.count += 1;
        entry.total_time += duration;
        entry.min_time = entry.min_time.min(duration);
        entry.max_time = entry.max_time.max(duration);

        self.maybe_report(&mut data);
    }

    /// Increments a hit counter for a specific named metric without recording duration.
    pub fn increment(&self, name: &'static str) {
        let mut data = self.data.lock().unwrap();
        let entry = data.entry(name).or_insert(Metric {
            count: 0,
            total_time: Duration::ZERO,
            min_time: Duration::ZERO,
            max_time: Duration::ZERO,
        });
        entry.count += 1;
        self.maybe_report(&mut data);
    }

    /// Periodically triggers a report if the `report_interval` has elapsed.
    fn maybe_report(&self, data: &mut HashMap<&'static str, Metric>) {
        let mut last_report = self.last_report.lock().unwrap();
        if last_report.elapsed() >= self.report_interval {
            self.report_and_reset(data);
            *last_report = Instant::now();
        }
    }

    /// Prints a formatted summary of all collected metrics to stderr and resets the counters.
    fn report_and_reset(&self, data: &mut HashMap<&'static str, Metric>) {
        if is_silent() {
            data.clear();
            return;
        }
        if data.is_empty() {
            return;
        }

        let mut sorted: Vec<_> = data.iter().collect();
        sorted.sort_by(|a, b| {
            b.1.total_time
                .cmp(&a.1.total_time)
                .then_with(|| b.1.count.cmp(&a.1.count))
        });

        eprintln!("\n--- [METRICS REPORT] ---");
        log_to_file(true, "--- [METRICS REPORT] ---");
        for (name, m) in sorted {
            if m.total_time > Duration::ZERO {
                let avg = m.total_time / m.count as u32;
                let line = format!(
                    " - {:<35} | hits={:<8} | total={:>10.2?} | avg={:>10.2?} | min={:>10.2?} | max={:>10.2?}",
                    name, m.count, m.total_time, avg, m.min_time, m.max_time
                );
                eprintln!("{}", line);
                log_to_file(true, &line);
            } else {
                let line = format!(" - {:<35} | hits={:<8} | (counter only)", name, m.count);
                eprintln!("{}", line);
                log_to_file(true, &line);
            }
        }
        eprintln!("------------------------\n");
        log_to_file(true, "------------------------");
        data.clear();
    }
}

/// RAID-style timer that records the duration of the current scope when dropped.
pub struct DebugTimer {
    name: &'static str,
    start: Instant,
}

impl DebugTimer {
    /// Creates a new timer for the given metric name.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for DebugTimer {
    fn drop(&mut self) {
        Metrics::global().record(self.name, self.start.elapsed());
    }
}

/// Macro to measure the execution time of the current scope and record it under a metric name.
#[macro_export]
macro_rules! observe_time {
    ($name:expr) => {
        let _timer = $crate::obs::DebugTimer::new($name);
    };
}

/// Macro to increment a hit counter for a specific metric name.
#[macro_export]
macro_rules! observe_hit {
    ($name:expr) => {
        $crate::obs::Metrics::global().increment($name);
    };
}

/// Macro for logging to stdout, suppressed if SILENT is set.
#[macro_export]
macro_rules! nm_log {
    ($($arg:tt)*) => {
        if !$crate::obs::is_silent() {
            let msg = format!($($arg)*);
            println!("{}", msg);
            $crate::obs::log_to_file(false, &msg);
        }
    };
}

/// Spin-yield until a Tokio `RwLock` write-guard is obtained.
///
/// Unlike `blocking_write()`, this loop never registers writer intent while
/// waiting, so concurrent `try_read()` calls from the UI thread can still
/// succeed in the interim.  Call only from blocking (non-async) contexts.
#[macro_export]
macro_rules! sim_write_spin {
    ($lock:expr) => {{
        loop {
            if let Ok(g) = $lock.try_write() {
                break g;
            }
            std::thread::yield_now();
        }
    }};
}

/// Macro for logging to stderr, suppressed if SILENT is set.
#[macro_export]
macro_rules! nm_err {
    ($($arg:tt)*) => {
        if !$crate::obs::is_silent() {
            let msg = format!($($arg)*);
            eprintln!("{}", msg);
            $crate::obs::log_to_file(true, &msg);
        }
    };
}
