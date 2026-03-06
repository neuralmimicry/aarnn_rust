#[cfg(feature = "core_affinity")]
use std::sync::OnceLock;
#[cfg(feature = "core_affinity")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "core_affinity")]
fn parse_env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "y" | "on" => true,
            "0" | "false" | "no" | "n" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

#[cfg(feature = "core_affinity")]
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

#[cfg(feature = "core_affinity")]
#[allow(dead_code)]
pub fn affinity_rotation_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| parse_env_bool("NM_CORE_AFFINITY_ROTATE", true))
}

#[cfg(not(feature = "core_affinity"))]
#[allow(dead_code)]
pub fn affinity_rotation_enabled() -> bool {
    false
}

#[cfg(feature = "core_affinity")]
fn configured_core_ids() -> &'static Vec<core_affinity::CoreId> {
    static CORE_IDS: OnceLock<Vec<core_affinity::CoreId>> = OnceLock::new();
    CORE_IDS.get_or_init(|| {
        let available = core_affinity::get_core_ids().unwrap_or_default();
        if available.is_empty() {
            return Vec::new();
        }
        let selected = parse_env_usize_list("NM_CORE_AFFINITY_LIST");
        if let Some(indices) = selected {
            let mut out = Vec::new();
            for wanted in indices {
                if let Some(core) = available.iter().find(|c| c.id == wanted).cloned() {
                    out.push(core);
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        available
    })
}

#[cfg(feature = "core_affinity")]
fn set_next_core() -> Option<usize> {
    static NEXT_CORE: AtomicUsize = AtomicUsize::new(0);
    if !affinity_rotation_enabled() {
        return None;
    }
    let cores = configured_core_ids();
    if cores.is_empty() {
        return None;
    }
    let idx = NEXT_CORE.fetch_add(1, Ordering::Relaxed) % cores.len();
    let core = cores.get(idx).cloned()?;
    let _ = core_affinity::set_for_current(core);
    Some(core.id)
}

pub fn apply_rotating_current_thread(_label: &str) -> Option<usize> {
    #[cfg(feature = "core_affinity")]
    {
        set_next_core()
    }
    #[cfg(not(feature = "core_affinity"))]
    {
        None
    }
}

pub fn configure_tokio_runtime_affinity(builder: &mut tokio::runtime::Builder, _pool_label: &'static str) {
    #[cfg(feature = "core_affinity")]
    {
        if affinity_rotation_enabled() {
            builder.on_thread_start(move || {
                let _ = set_next_core();
            });
        }
    }
    #[cfg(not(feature = "core_affinity"))]
    {
        let _ = builder;
    }
}
