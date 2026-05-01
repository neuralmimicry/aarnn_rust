#![cfg(feature = "openmpi")]

use anyhow::anyhow;
use mpi::environment::{Threading, Universe, threading_support};
use mpi::traits::*;
use std::net::UdpSocket;
use std::sync::{Mutex, OnceLock};

pub const SPIKE_TRANSPORT_TAG: i32 = 0x4E4D;

#[derive(Debug, Clone, Copy)]
struct OpenMpiContext {
    rank: i32,
    size: i32,
    threading: Threading,
    transport_eligible: bool,
}

static MPI_UNIVERSE: OnceLock<Universe> = OnceLock::new();
static MPI_CONTEXT: OnceLock<OpenMpiContext> = OnceLock::new();
static MPI_CALL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct OpenMpiBootstrap {
    pub rank: i32,
    pub size: i32,
    pub local_rank: Option<i32>,
    pub orchestrator_addr: String,
}

pub fn bootstrap(
    grpc_addr: &str,
    explicit_orchestrator_addr: Option<&str>,
) -> anyhow::Result<Option<OpenMpiBootstrap>> {
    let ctx = ensure_context()?;
    let size = ctx.size;
    let rank = ctx.rank;
    let local_rank = detect_local_rank();

    let force = std::env::var("NM_MPI_FORCE_BOOTSTRAP")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if size <= 1 && !force {
        return Ok(None);
    }

    let _guard = mpi_call_lock()
        .lock()
        .map_err(|_| anyhow!("MPI call lock poisoned"))?;
    let world = mpi::topology::SimpleCommunicator::world();
    let mut payload = if rank == 0 {
        derive_orchestrator_addr(grpc_addr, explicit_orchestrator_addr).into_bytes()
    } else {
        Vec::new()
    };
    let mut payload_len = payload.len() as i32;
    let root = world.process_at_rank(0);
    root.broadcast_into(&mut payload_len);
    if payload_len <= 0 || payload_len > 4096 {
        return Err(anyhow!(
            "invalid MPI orchestrator address payload length {}",
            payload_len
        ));
    }
    if rank != 0 {
        payload.resize(payload_len as usize, 0);
    }
    root.broadcast_into(payload.as_mut_slice());
    let orchestrator_addr = String::from_utf8(payload).map_err(|e| {
        anyhow!(
            "invalid utf8 from MPI orchestrator address broadcast: {}",
            e
        )
    })?;
    if orchestrator_addr.trim().is_empty() {
        return Err(anyhow!("MPI orchestrator address broadcast was empty"));
    }

    Ok(Some(OpenMpiBootstrap {
        rank,
        size,
        local_rank,
        orchestrator_addr,
    }))
}

pub fn spike_transport_available() -> bool {
    let enabled = std::env::var("NM_MPI_TRANSPORT")
        .ok()
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(true);
    if !enabled {
        return false;
    }
    match ensure_context() {
        Ok(ctx) => ctx.size > 1 && ctx.transport_eligible,
        Err(_) => false,
    }
}

pub fn send_tagged_bytes(dest_rank: i32, tag: i32, payload: &[u8]) -> anyhow::Result<()> {
    let ctx = ensure_context()?;
    if !ctx.transport_eligible {
        return Err(anyhow!(
            "MPI transport disabled: threading mode {:?} is not safe for concurrent runtime calls",
            ctx.threading
        ));
    }
    if dest_rank < 0 || dest_rank >= ctx.size {
        return Err(anyhow!(
            "destination MPI rank {} outside world size {}",
            dest_rank,
            ctx.size
        ));
    }
    let _guard = mpi_call_lock()
        .lock()
        .map_err(|_| anyhow!("MPI call lock poisoned"))?;
    let world = mpi::topology::SimpleCommunicator::world();
    world.process_at_rank(dest_rank).send_with_tag(payload, tag);
    Ok(())
}

pub fn try_recv_tagged_bytes(tag: i32) -> anyhow::Result<Option<(i32, Vec<u8>)>> {
    let ctx = ensure_context()?;
    if !ctx.transport_eligible {
        return Ok(None);
    }
    let _guard = mpi_call_lock()
        .lock()
        .map_err(|_| anyhow!("MPI call lock poisoned"))?;
    let world = mpi::topology::SimpleCommunicator::world();
    if world.any_process().immediate_probe_with_tag(tag).is_none() {
        return Ok(None);
    }
    let (payload, status) = world.any_process().receive_vec_with_tag::<u8>(tag);
    Ok(Some((status.source_rank(), payload)))
}

fn ensure_context() -> anyhow::Result<&'static OpenMpiContext> {
    if let Some(ctx) = MPI_CONTEXT.get() {
        return Ok(ctx);
    }

    let provided =
        if let Some((universe, p)) = mpi::initialize_with_threading(Threading::Serialized) {
            let _ = MPI_UNIVERSE.set(universe);
            p
        } else {
            threading_support()
        };

    let world = mpi::topology::SimpleCommunicator::world();
    let rank = world.rank();
    let size = world.size();
    let transport_eligible = matches!(provided, Threading::Serialized | Threading::Multiple);

    if matches!(provided, Threading::Single | Threading::Funneled) {
        nm_err!(
            "[warn] OpenMPI threading support is {:?}; MPI spike transport disabled",
            provided
        );
    }

    let ctx = OpenMpiContext {
        rank,
        size,
        threading: provided,
        transport_eligible,
    };
    let _ = MPI_CONTEXT.set(ctx);
    MPI_CONTEXT
        .get()
        .ok_or_else(|| anyhow!("failed to initialize OpenMPI context"))
}

fn mpi_call_lock() -> &'static Mutex<()> {
    MPI_CALL_LOCK.get_or_init(|| Mutex::new(()))
}

fn detect_local_rank() -> Option<i32> {
    [
        "OMPI_COMM_WORLD_LOCAL_RANK",
        "MPI_LOCALRANKID",
        "SLURM_LOCALID",
        "PMI_RANK",
    ]
    .iter()
    .find_map(|k| std::env::var(k).ok().and_then(|v| v.parse::<i32>().ok()))
}

fn derive_orchestrator_addr(grpc_addr: &str, explicit_orchestrator_addr: Option<&str>) -> String {
    if let Some(explicit) = explicit_orchestrator_addr {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return normalize_http_addr(trimmed);
        }
    }
    if let Ok(env_addr) = std::env::var("NM_MPI_ORCHESTRATOR_ADDR") {
        let trimmed = env_addr.trim();
        if !trimmed.is_empty() {
            return normalize_http_addr(trimmed);
        }
    }
    if let Some((host, port)) = parse_host_port(grpc_addr) {
        let advertise_host = choose_advertise_host(&host);
        return format!("http://{}", format_host_port(&advertise_host, port));
    }
    normalize_http_addr(grpc_addr)
}

fn normalize_http_addr(raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{}", raw)
    }
}

fn parse_host_port(addr: &str) -> Option<(String, u16)> {
    let trimmed = addr.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let without_path = without_scheme.split('/').next().unwrap_or(without_scheme);
    if without_path.starts_with('[') {
        let end = without_path.find(']')?;
        let host = &without_path[1..end];
        let port_str = without_path.get(end + 1..)?.strip_prefix(':')?;
        let port = port_str.parse().ok()?;
        return Some((host.to_string(), port));
    }
    let mut parts = without_path.rsplitn(2, ':');
    let port_str = parts.next()?;
    let host = parts.next()?;
    let port = port_str.parse().ok()?;
    Some((host.to_string(), port))
}

fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

fn choose_advertise_host(bind_host: &str) -> String {
    if let Ok(val) = std::env::var("NM_MPI_ADVERTISE_ADDR") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let lower = bind_host.to_ascii_lowercase();
    let wildcard = matches!(
        lower.as_str(),
        "0.0.0.0" | "::" | "0:0:0:0:0:0:0:0" | "localhost" | "127.0.0.1" | "::1"
    );
    if !wildcard {
        return bind_host.to_string();
    }

    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("1.1.1.1:53").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                return addr.ip().to_string();
            }
        }
    }

    if let Ok(hostname) = std::env::var("HOSTNAME") {
        let trimmed = hostname.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    "127.0.0.1".to_string()
}
