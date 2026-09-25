use std::collections::HashMap;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Global flag to suppress all console/logging output.
pub static SILENT: AtomicBool = AtomicBool::new(false);

static LOG_FILE: OnceLock<Mutex<Option<BufWriter<std::fs::File>>>> = OnceLock::new();
static LOG_STATE: OnceLock<Mutex<LogState>> = OnceLock::new();
static SHUTDOWN_SIGNAL: AtomicU8 = AtomicU8::new(0);
static UI_SHUTDOWN_CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);

const DEFAULT_LOG_ROTATE_BYTES: u64 = 20 * 1024 * 1024;
const DEFAULT_LOG_ROTATE_COUNT: usize = 5;
const DEFAULT_LOG_HISTORY_SESSIONS: usize = 10;
const DEFAULT_LOG_HISTORY_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_LOG_WARN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_LOG_STOP_FREE_BYTES: u64 = 512 * 1024 * 1024;
const MIN_LOG_ROTATE_BYTES: u64 = 64 * 1024;
const MAX_LOG_ROTATE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_LOG_ROTATE_COUNT: usize = 20;
const MIN_LOG_HISTORY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LOG_HISTORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_LOG_HISTORY_SESSIONS: usize = 100;
const MAX_LOG_LINE_BYTES: usize = 32 * 1024;

struct LogState {
    path: PathBuf,
    max_bytes: u64,
    max_files: usize,
    max_history_sessions: usize,
    max_history_bytes: u64,
    warn_free_bytes: u64,
    stop_free_bytes: u64,
    current_bytes: u64,
    writes: u64,
    last_space_check: Instant,
    file_disabled: bool,
    warned_low_space: bool,
    warned_space_probe: bool,
    manages_session_history: bool,
}

#[inline]
pub fn is_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

pub fn init_log_file(path: &Path) -> std::io::Result<()> {
    init_log_file_inner(path, false)
}

pub fn init_session_log_file(path: &Path) -> std::io::Result<()> {
    init_log_file_inner(path, true)
}

fn init_log_file_inner(path: &Path, manage_session_history: bool) -> std::io::Result<()> {
    if is_silent() {
        return Ok(());
    }
    if LOG_FILE.get().is_some() {
        return Ok(());
    }
    let parent = log_parent(path);
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(&parent)?;
    }

    let max_files = bounded_env_usize(
        "NM_LOG_ROTATE_COUNT",
        DEFAULT_LOG_ROTATE_COUNT,
        1,
        MAX_LOG_ROTATE_COUNT,
    );
    let max_history_sessions = bounded_env_usize(
        "NM_LOG_ROTATE_SESSIONS",
        DEFAULT_LOG_HISTORY_SESSIONS,
        1,
        MAX_LOG_HISTORY_SESSIONS,
    );
    let max_history_bytes = bounded_env_u64(
        "NM_LOG_TOTAL_BYTES",
        DEFAULT_LOG_HISTORY_BYTES,
        MIN_LOG_HISTORY_BYTES,
        MAX_LOG_HISTORY_BYTES,
    );
    let requested_max_bytes = bounded_env_u64(
        "NM_LOG_ROTATE_BYTES",
        DEFAULT_LOG_ROTATE_BYTES,
        MIN_LOG_ROTATE_BYTES,
        MAX_LOG_ROTATE_BYTES,
    );
    let manages_session_history = manage_session_history && session_log_id(path).is_some();
    // One active file plus its retained backups must fit within the aggregate
    // AARNN session-log budget even when operators raise per-file settings.
    let max_bytes = if manages_session_history {
        requested_max_bytes
            .min(max_history_bytes / (max_files as u64 + 1))
            .max(MIN_LOG_ROTATE_BYTES)
    } else {
        requested_max_bytes
    };
    let stop_free_bytes = bounded_env_u64(
        "NM_LOG_DISK_STOP_FREE_BYTES",
        DEFAULT_LOG_STOP_FREE_BYTES,
        16 * 1024 * 1024,
        MAX_LOG_HISTORY_BYTES,
    );
    let warn_free_bytes = bounded_env_u64(
        "NM_LOG_DISK_WARN_FREE_BYTES",
        DEFAULT_LOG_WARN_FREE_BYTES,
        stop_free_bytes,
        MAX_LOG_HISTORY_BYTES,
    )
    .max(stop_free_bytes);
    if manages_session_history {
        prune_session_log_history(
            &parent,
            max_history_sessions,
            max_history_bytes,
            session_log_id(path).as_deref(),
        );
    }
    if let Ok(available) = fs2::available_space(&parent)
        && available <= stop_free_bytes
    {
        return Err(io::Error::other(format!(
            "log file not opened: filesystem has {available} free bytes, below the configured {stop_free_bytes}-byte reserve"
        )));
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let current_bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    if LOG_FILE
        .set(Mutex::new(Some(BufWriter::new(file))))
        .is_err()
    {
        return Ok(());
    }
    let _ = LOG_STATE.set(Mutex::new(LogState {
        path: path.to_path_buf(),
        max_bytes,
        max_files,
        max_history_sessions,
        max_history_bytes,
        warn_free_bytes,
        stop_free_bytes,
        current_bytes,
        writes: 0,
        last_space_check: Instant::now(),
        file_disabled: false,
        warned_low_space: false,
        warned_space_probe: false,
        manages_session_history,
    }));
    if manages_session_history {
        prune_session_log_history(
            &parent,
            max_history_sessions,
            max_history_bytes,
            session_log_id(path).as_deref(),
        );
    }
    Ok(())
}

pub fn flush_log() {
    if let Some(lock) = LOG_FILE.get() {
        if let Ok(mut guard) = lock.lock()
            && let Some(writer) = guard.as_mut()
        {
            let _ = writer.flush();
        }
    }
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
}

pub fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        std::panic::set_hook(Box::new(|panic| {
            if is_silent() {
                return;
            }
            let backtrace = std::backtrace::Backtrace::force_capture();
            crate::nm_err!("[panic] {panic}\nbacktrace:\n{backtrace}");
            flush_log();
        }));
    });
}

pub fn note_shutdown_signal(signal: &'static str) {
    let code = match signal {
        "SIGHUP" => 1,
        "SIGTERM" => 2,
        "SIGINT" => 3,
        _ => 4,
    };
    if SHUTDOWN_SIGNAL
        .compare_exchange(0, code, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        crate::nm_log!("[shutdown] signal_received={signal} action=graceful_shutdown");
        flush_log();
    }
}

pub fn shutdown_signal_name() -> Option<&'static str> {
    match SHUTDOWN_SIGNAL.load(Ordering::SeqCst) {
        1 => Some("SIGHUP"),
        2 => Some("SIGTERM"),
        3 => Some("SIGINT"),
        4 => Some("unknown"),
        _ => None,
    }
}

pub fn take_ui_shutdown_close_request() -> bool {
    shutdown_signal_name().is_some() && !UI_SHUTDOWN_CLOSE_REQUESTED.swap(true, Ordering::SeqCst)
}

pub async fn wait_for_shutdown_signal() -> io::Result<&'static str> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::SignalKind;
        let mut hangup = tokio::signal::unix::signal(SignalKind::hangup())?;
        let mut terminate = tokio::signal::unix::signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                Ok("SIGINT")
            }
            _ = hangup.recv() => Ok("SIGHUP"),
            _ = terminate.recv() => Ok("SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok("SIGINT")
    }
}

pub struct FinalLogFlush;

impl FinalLogFlush {
    pub fn new() -> Self {
        Self
    }
}

impl Drop for FinalLogFlush {
    fn drop(&mut self) {
        if let Some(signal) = shutdown_signal_name() {
            crate::nm_log!("[shutdown] graceful_shutdown_complete=1 signal={signal}");
        }
        flush_log();
    }
}

fn log_parent(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn bounded_env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
        .clamp(min, max)
}

fn bounded_env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
        .clamp(min, max)
}

fn session_log_id(path: &Path) -> Option<String> {
    let filename = path.file_name()?.to_str()?;
    let filename = match filename.rsplit_once('.') {
        Some((base, suffix)) if suffix.chars().all(|ch| ch.is_ascii_digit()) => base,
        _ => filename,
    };
    let session = filename.strip_prefix("nm-")?.strip_suffix(".log")?;
    (!session.is_empty() && session.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| session.to_string())
}

fn prune_session_log_history(
    directory: &Path,
    max_sessions: usize,
    max_bytes: u64,
    protected_session: Option<&str>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut sessions: HashMap<String, Vec<(PathBuf, u64, SystemTime)>> = HashMap::new();
    let mut total_bytes = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(session) = session_log_id(&path) else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        total_bytes = total_bytes.saturating_add(size);
        sessions
            .entry(session)
            .or_default()
            .push((path, size, modified));
    }

    while sessions.len() > max_sessions || total_bytes > max_bytes {
        let oldest_session = sessions
            .iter()
            .filter(|(session, _)| Some(session.as_str()) != protected_session)
            .min_by_key(|(_, files)| {
                files
                    .iter()
                    .map(|(_, _, modified)| *modified)
                    .min()
                    .unwrap_or(UNIX_EPOCH)
            })
            .map(|(session, _)| session.clone());
        let Some(oldest_session) = oldest_session else {
            break;
        };
        let Some(files) = sessions.remove(&oldest_session) else {
            break;
        };
        let mut removed_any = false;
        for (path, size, _) in files {
            if std::fs::remove_file(path).is_ok() {
                total_bytes = total_bytes.saturating_sub(size);
                removed_any = true;
            }
        }
        if !removed_any {
            break;
        }
    }
}

fn rotate_log_files(
    state: &mut LogState,
    writer_slot: &mut Option<BufWriter<std::fs::File>>,
) -> io::Result<()> {
    let Some(mut writer) = writer_slot.take() else {
        return Err(io::Error::other("log writer is unavailable"));
    };
    writer.flush()?;
    drop(writer);

    for index in (1..=state.max_files).rev() {
        let source = if index == 1 {
            state.path.clone()
        } else {
            PathBuf::from(format!("{}.{}", state.path.display(), index - 1))
        };
        let destination = PathBuf::from(format!("{}.{}", state.path.display(), index));
        if destination.exists() {
            std::fs::remove_file(&destination)?;
        }
        if source.exists() {
            std::fs::rename(source, destination)?;
        }
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.path)?;
    *writer_slot = Some(BufWriter::new(file));
    state.current_bytes = 0;

    if state.manages_session_history {
        prune_session_log_history(
            &log_parent(&state.path),
            state.max_history_sessions,
            state.max_history_bytes,
            session_log_id(&state.path).as_deref(),
        );
    }
    Ok(())
}

fn truncate_log_line(mut line: String) -> String {
    if line.len() <= MAX_LOG_LINE_BYTES {
        return line;
    }
    const MARKER: &str = "...[log entry truncated]";
    let mut boundary = MAX_LOG_LINE_BYTES - MARKER.len();
    while !line.is_char_boundary(boundary) {
        boundary -= 1;
    }
    line.truncate(boundary);
    line.push_str(MARKER);
    line
}

fn emit_log_alert(message: &str) {
    if !is_silent() {
        eprintln!("{message}");
    }
}

pub(crate) fn log_to_file(is_err: bool, msg: &str) {
    if is_silent() {
        return;
    }
    let (Some(file_lock), Some(state_lock)) = (LOG_FILE.get(), LOG_STATE.get()) else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let prefix = if is_err { "ERR" } else { "OUT" };
    let line = truncate_log_line(format!(
        "[{}.{:03}] {} {}",
        now.as_secs(),
        now.subsec_millis(),
        prefix,
        msg
    ));
    let mut alert = None;

    let Ok(mut state) = state_lock.lock() else {
        return;
    };
    if state.file_disabled {
        return;
    }

    state.writes = state.writes.saturating_add(1);
    let check_space = state.writes == 1
        || state.writes.is_multiple_of(16)
        || state.last_space_check.elapsed() >= Duration::from_secs(5);
    if check_space {
        state.last_space_check = Instant::now();
        let directory = log_parent(&state.path);
        match fs2::available_space(&directory) {
            Ok(available) if available <= state.stop_free_bytes => {
                state.file_disabled = true;
                if let Ok(mut writer_slot) = file_lock.lock()
                    && let Some(mut writer) = writer_slot.take()
                {
                    let _ = writer.flush();
                }
                alert = Some(format!(
                    "[warn][logging] disk_pressure=critical available_bytes={available} stop_below_bytes={} action=disable_aarnn_file_sink path={}",
                    state.stop_free_bytes,
                    state.path.display()
                ));
            }
            Ok(available) if available <= state.warn_free_bytes && !state.warned_low_space => {
                state.warned_low_space = true;
                alert = Some(format!(
                    "[warn][logging] disk_pressure=warning available_bytes={available} warn_below_bytes={} action=retain_bounded_rotation_and_monitor path={}",
                    state.warn_free_bytes,
                    state.path.display()
                ));
            }
            Err(error) if !state.warned_space_probe => {
                state.warned_space_probe = true;
                alert = Some(format!(
                    "[warn][logging] disk_space_probe=unavailable path={} error={error}",
                    directory.display()
                ));
            }
            _ => {}
        }
    }

    if !state.file_disabled {
        if state.current_bytes.saturating_add(line.len() as u64 + 1) > state.max_bytes {
            match file_lock.lock() {
                Ok(mut writer) => {
                    if let Err(error) = rotate_log_files(&mut state, &mut writer) {
                        state.file_disabled = true;
                        alert.get_or_insert_with(|| {
                            format!(
                                "[warn][logging] rotation=failed action=disable_aarnn_file_sink path={} error={error}",
                                state.path.display()
                            )
                        });
                    }
                }
                Err(_) => {
                    state.file_disabled = true;
                    alert.get_or_insert_with(|| {
                        format!(
                            "[warn][logging] rotation=failed action=disable_aarnn_file_sink path={} error=writer_lock_poisoned",
                            state.path.display()
                        )
                    });
                }
            }
        }
    }

    if !state.file_disabled {
        match file_lock.lock() {
            Ok(mut writer_slot) => {
                let Some(writer) = writer_slot.as_mut() else {
                    state.file_disabled = true;
                    alert.get_or_insert_with(|| {
                        format!(
                            "[warn][logging] writer=unavailable action=disable_aarnn_file_sink path={}",
                            state.path.display()
                        )
                    });
                    drop(state);
                    if let Some(alert) = alert {
                        emit_log_alert(&alert);
                    }
                    return;
                };
                match writeln!(writer, "{line}") {
                    Ok(()) => {
                        state.current_bytes =
                            state.current_bytes.saturating_add(line.len() as u64 + 1);
                    }
                    Err(error) => {
                        state.file_disabled = true;
                        alert.get_or_insert_with(|| {
                            format!(
                                "[warn][logging] write=failed action=disable_aarnn_file_sink path={} error={error}",
                                state.path.display()
                            )
                        });
                    }
                }
            }
            Err(_) => {
                state.file_disabled = true;
                alert.get_or_insert_with(|| {
                    format!(
                        "[warn][logging] writer_lock=poisoned action=disable_aarnn_file_sink path={}",
                        state.path.display()
                    )
                });
            }
        }
    }

    drop(state);
    if let Some(alert) = alert {
        emit_log_alert(&alert);
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
        if is_silent() {
            return;
        }
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
        if is_silent() {
            return;
        }
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
    start: Option<Instant>,
}

impl DebugTimer {
    /// Creates a new timer for the given metric name.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: (!is_silent()).then(Instant::now),
        }
    }
}

impl Drop for DebugTimer {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            Metrics::global().record(self.name, start.elapsed());
        }
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
        if !$crate::obs::is_silent() {
            $crate::obs::Metrics::global().increment($name);
        }
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
