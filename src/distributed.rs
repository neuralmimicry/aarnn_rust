//! Distributed simulation module for scaling neuromorphic workloads across multiple nodes.
//!
//! This module implements a gRPC-based distributed architecture that allows a large
//! neural network to be partitioned and simulated across a cluster of compute nodes.
//!
//! ## Architecture
//! - **Orchestrator**: A singleton node that manages the cluster, monitors node
//!   health/resources, and handles network partitioning and rebalancing.
//! - **Compute Node**: A participant that executes a subset of the neural network
//!   layers. It communicates with the Orchestrator via gRPC (heartbeats, commands)
//!   and with other compute nodes via spike streaming.
//! - **Network Partitioning**: The network is divided by layers. Each node is
//!   assigned a range of layers to simulate. Boundary layers may be duplicated
//!   for synchronization and redundancy.
//!
//! ## Communication
//! - **Discovery**: Nodes find the Orchestrator using UDP broadcast/multicast beacons.
//! - **Heartbeats**: Nodes periodically report their resource usage (CPU, RAM) and
//!   simulation performance to the Orchestrator.
//! - **Spike Streaming**: Real-time spike events are streamed between nodes to
//!   synchronize activity across layer boundaries.
//!
//! ## Key Components
//! - `DistributedNode`: The primary interface for both Orchestrator and Compute roles.
//! - `NodeState`: Maintains the local view of the cluster and managed networks.
//! - `ManagedNetwork`: Represents a partition of a neural network being simulated on the local node.
#[cfg(not(feature = "sysinfo"))]
use self::sysinfo_dummy::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
#[cfg(feature = "openmpi")]
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
#[cfg(feature = "sysinfo")]
use sysinfo::{Components, CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::{mpsc, RwLock};
use tonic::{Request, Response, Status};

#[cfg(not(feature = "sysinfo"))]
mod sysinfo_dummy {
    pub struct System;
    impl System {
        pub fn new_with_specifics(_: RefreshKind) -> Self {
            Self
        }
        pub fn refresh_cpu_usage(&mut self) {}
        pub fn refresh_memory(&mut self) {}
        pub fn global_cpu_usage(&self) -> f32 {
            0.0
        }
        pub fn available_memory(&self) -> u64 {
            0
        }
        pub fn total_memory(&self) -> u64 {
            0
        }
    }
    pub struct RefreshKind;
    impl RefreshKind {
        pub fn nothing() -> Self {
            Self
        }
        pub fn with_cpu(self, _: CpuRefreshKind) -> Self {
            self
        }
        pub fn with_memory(self, _: MemoryRefreshKind) -> Self {
            self
        }
    }
    pub struct CpuRefreshKind;
    impl CpuRefreshKind {
        pub fn everything() -> Self {
            Self
        }
    }
    pub struct MemoryRefreshKind;
    impl MemoryRefreshKind {
        pub fn everything() -> Self {
            Self
        }
    }
}
use crate::config::{LIFParams, NetworkConfig, STDPParams};
use crate::runner::Runner;
use crate::sim::{Learning, NeuronModel};
use crate::spike_io::transport::{encode_exchange, spikes_from_transport};
use anyhow::Context;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::watch;

// Include the generated gRPC code
pub mod proto {
    tonic::include_proto!("distributed");
}

pub const PEER_STALE_AFTER: Duration = Duration::from_secs(20);
/// Special layer index used on `StreamSpikes` to inject sensory spikes from external
/// AER/HTTP sources into the network's next simulation step.
pub const EXTERNAL_SENSORY_LAYER_INDEX: u32 = u32::MAX;
/// Timeout budget for burst-mode spike forwarding fallback.
const SPIKE_BURST_TIMEOUT: Duration = Duration::from_millis(120);
/// Timeout budget for short-lived gRPC connections used by burst forwarding.
const SPIKE_BURST_CONNECT_TIMEOUT: Duration = Duration::from_millis(80);
/// EWMA smoothing for per-peer transport latency tracking.
const SPIKE_LATENCY_EWMA_ALPHA: f64 = 0.2;
/// Consecutive failures before preferring the alternate transport method.
const SPIKE_FAILOVER_STREAK: u32 = 3;
/// Cap queued control/config commands per node so heartbeat payloads remain bounded.
const MAX_PENDING_COMMANDS_PER_NODE: usize = 64;
/// Treat configs above this as "large" and avoid broadcasting to all nodes without affinity.
const LARGE_NETWORK_CONFIG_BYTES: usize = 64 * 1024 * 1024;
fn grpc_max_message_bytes() -> usize {
    const DEFAULT: usize = 512 * 1024 * 1024;
    const MIN: usize = 4 * 1024 * 1024;
    std::env::var("NM_GRPC_MAX_MESSAGE_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v >= MIN)
        .unwrap_or(DEFAULT)
}

use proto::distributed_neuromorphic_client::DistributedNeuromorphicClient;
use proto::distributed_neuromorphic_server::DistributedNeuromorphic;
use proto::*;

fn control_action_from_command(
    cmd_type: proto::network_command::CommandType,
) -> Option<proto::control_update::Action> {
    use proto::control_update::Action;
    use proto::network_command::CommandType;
    match cmd_type {
        CommandType::Start => Some(Action::Start),
        CommandType::Stop => Some(Action::Stop),
        CommandType::Repeat => Some(Action::Repeat),
        CommandType::Reset => Some(Action::Reset),
        _ => None,
    }
}

fn command_type_from_action(
    action: proto::control_update::Action,
) -> proto::network_command::CommandType {
    use proto::control_update::Action;
    use proto::network_command::CommandType;
    match action {
        Action::Start => CommandType::Start,
        Action::Stop => CommandType::Stop,
        Action::Repeat => CommandType::Repeat,
        Action::Reset => CommandType::Reset,
        Action::New => CommandType::LoadNetwork,
    }
}

fn enqueue_pending_command(
    pending_commands: &mut HashMap<String, Vec<NetworkCommand>>,
    node_id: String,
    cmd: NetworkCommand,
) {
    let queue = pending_commands.entry(node_id.clone()).or_default();
    queue.retain(|existing| {
        !(existing.network_id == cmd.network_id && existing.r#type == cmd.r#type)
    });
    queue.push(cmd);
    if queue.len() > MAX_PENDING_COMMANDS_PER_NODE {
        let overflow = queue.len() - MAX_PENDING_COMMANDS_PER_NODE;
        queue.drain(0..overflow);
        nm_err!(
            "[warn] Pending command queue overflow for {} (max {}); dropped {} oldest commands",
            node_id,
            MAX_PENDING_COMMANDS_PER_NODE,
            overflow
        );
    }
}

fn fresh_single_neuron_config(desired_depth: u32) -> NetworkConfig {
    let mut cfg = NetworkConfig::default();
    if desired_depth > 0 {
        cfg.aarnn_layer_depth = desired_depth as usize;
    }
    cfg
}

fn fresh_single_neuron_snapshot(
    desired_depth: u32,
    model: NeuronModel,
    learning: Learning,
) -> Result<(NetworkConfig, String), String> {
    let cfg = fresh_single_neuron_config(desired_depth);
    let runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        cfg.clone(),
        model,
        learning,
    );
    runner
        .export_network_json()
        .map(|json| (cfg, json))
        .map_err(|e| e.to_string())
}

fn default_workspace_autosave_steps() -> u64 {
    10
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NetworkWorkspaceBinding {
    pub workspace_id: String,
    pub latest_snapshot_path: String,
    #[serde(default)]
    pub manifest_path: Option<String>,
    #[serde(default = "default_workspace_autosave_steps")]
    pub autosave_steps: u64,
}

fn load_workspace_bindings_from_env() -> HashMap<String, NetworkWorkspaceBinding> {
    let Some(raw) = std::env::var("NM_RUNTIME_WORKSPACE_BINDINGS").ok() else {
        return HashMap::new();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return HashMap::new();
    }
    serde_json::from_str(trimmed).unwrap_or_else(|err| {
        nm_err!(
            "[warn] Failed to parse NM_RUNTIME_WORKSPACE_BINDINGS: {}",
            err
        );
        HashMap::new()
    })
}

fn atomic_write_workspace_snapshot(path: &str, payload: &[u8]) -> anyhow::Result<()> {
    let path = std::path::Path::new(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let tmp_path = path.with_extension(format!("tmp-{}", fastrand::u32(..)));
    std::fs::write(&tmp_path, payload)
        .with_context(|| format!("failed to write '{}'", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename '{}' to '{}'",
            tmp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn update_workspace_manifest_saved_at(path: &str, saved_at_ms: u64) -> anyhow::Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("failed to read '{}'", path))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("failed to parse '{}'", path))?;
    let Some(obj) = value.as_object_mut() else {
        anyhow::bail!("workspace manifest '{}' is not a JSON object", path);
    };
    obj.insert("updated_at_ms".to_string(), serde_json::json!(saved_at_ms));
    obj.insert(
        "last_saved_at_ms".to_string(),
        serde_json::json!(saved_at_ms),
    );
    let payload = serde_json::to_vec_pretty(&value)
        .with_context(|| format!("failed to serialize '{}'", path))?;
    atomic_write_workspace_snapshot(path, &payload)
}

fn persist_workspace_snapshot(
    binding: &NetworkWorkspaceBinding,
    snapshot_json: &str,
) -> anyhow::Result<()> {
    atomic_write_workspace_snapshot(&binding.latest_snapshot_path, snapshot_json.as_bytes())?;
    if let Some(manifest_path) = binding.manifest_path.as_deref() {
        if let Err(err) = update_workspace_manifest_saved_at(manifest_path, now_ms()) {
            nm_err!(
                "[warn] Failed to update workspace manifest '{}' after snapshot save: {}",
                manifest_path,
                err
            );
        }
    }
    Ok(())
}

fn apply_control_to_managed_network(
    net: &mut ManagedNetwork,
    action: proto::control_update::Action,
) {
    match action {
        proto::control_update::Action::Start => {
            net.playing = true;
        }
        proto::control_update::Action::Stop => {
            net.playing = false;
            net.remote_spikes_fwd.clear();
            net.remote_spikes_bwd.clear();
            net.remote_spike_steps_fwd.clear();
            net.remote_spike_steps_bwd.clear();
        }
        proto::control_update::Action::Repeat => {
            net.runner.reset();
            net.remote_spikes_fwd.clear();
            net.remote_spikes_bwd.clear();
            net.remote_spike_steps_fwd.clear();
            net.remote_spike_steps_bwd.clear();
            net.avg_step_time_ms = 0.0;
            net.playing = true;
        }
        proto::control_update::Action::Reset => {
            let mut runner = Runner::new(
                net.initial_lif.clone(),
                net.initial_stdp.clone(),
                net.initial_config.clone(),
                net.initial_model.clone(),
                net.initial_learning.clone(),
            );
            if !net.assigned_layers.is_empty() {
                if let (Some(min), Some(max)) = (
                    net.assigned_layers.iter().min(),
                    net.assigned_layers.iter().max(),
                ) {
                    runner.layer_range = Some(*min as usize..(*max as usize + 1));
                    #[cfg(feature = "growth3d")]
                    runner.rebuild_default_topology();
                }
            }
            net.runner = runner;
            net.remote_spikes_fwd.clear();
            net.remote_spikes_bwd.clear();
            net.remote_spike_steps_fwd.clear();
            net.remote_spike_steps_bwd.clear();
            net.avg_step_time_ms = 0.0;
            net.playing = false;
        }
        proto::control_update::Action::New => {
            let lif = net.runner.lif.clone();
            let stdp = net.runner.stdp.clone();
            let model = net.runner.neuron_model;
            let learning = net.runner.learning;
            let cfg = fresh_single_neuron_config(net.desired_aarnn_depth);
            let mut runner = Runner::new(lif.clone(), stdp.clone(), cfg.clone(), model, learning);
            if !net.assigned_layers.is_empty() {
                if let (Some(min), Some(max)) = (
                    net.assigned_layers.iter().min(),
                    net.assigned_layers.iter().max(),
                ) {
                    runner.layer_range = Some(*min as usize..(*max as usize + 1));
                    #[cfg(feature = "growth3d")]
                    runner.rebuild_default_topology();
                }
            }
            net.runner = runner;
            net.remote_spikes_fwd.clear();
            net.remote_spikes_bwd.clear();
            net.remote_spike_steps_fwd.clear();
            net.remote_spike_steps_bwd.clear();
            net.avg_step_time_ms = 0.0;
            net.playing = false;
            net.initial_config = cfg;
            net.initial_model = model;
            net.initial_learning = learning;
            net.initial_lif = lif;
            net.initial_stdp = stdp;
        }
    }
}

fn split_host_port(addr: &str) -> Option<(String, u16)> {
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

fn peer_id_from_remote_addr(state: &NodeState, remote_addr: Option<SocketAddr>) -> Option<String> {
    let remote = remote_addr?;
    for (node_id, addr) in &state.peers {
        if let Some((host, port)) = split_host_port(addr) {
            if port != remote.port() {
                continue;
            }
            if host == remote.ip().to_string() {
                return Some(node_id.clone());
            }
            if host == "0.0.0.0" {
                return Some(node_id.clone());
            }
            if host.eq_ignore_ascii_case("localhost") && remote.ip().is_loopback() {
                return Some(node_id.clone());
            }
            if host == "127.0.0.1" && remote.ip().is_loopback() {
                return Some(node_id.clone());
            }
        }
    }
    None
}

#[cfg(feature = "openmpi")]
fn mpi_rank_from_node_id(node_id: &str) -> Option<i32> {
    node_id.rsplit_once("_mpi")?.1.parse::<i32>().ok()
}

#[cfg(feature = "openmpi")]
fn peer_id_from_mpi_rank(state: &NodeState, rank: i32) -> Option<String> {
    if mpi_rank_from_node_id(&state.node_id) == Some(rank) {
        return Some(state.node_id.clone());
    }
    for node_id in state.peers.keys() {
        if mpi_rank_from_node_id(node_id) == Some(rank) {
            return Some(node_id.clone());
        }
    }
    None
}

fn normalize_peer_address(advertised: &str, remote_addr: Option<SocketAddr>) -> (String, String) {
    let trimmed = advertised.trim();
    let fallback_display = trimmed.to_string();
    let fallback_connect = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    };

    let Some((mut host, port)) = split_host_port(trimmed) else {
        return (fallback_display, fallback_connect);
    };

    if let Some(remote_ip) = remote_addr.map(|addr| addr.ip()) {
        let host_lc = host.to_ascii_lowercase();
        let needs_replace = match host_lc.as_str() {
            "0.0.0.0" | "::" | "0:0:0:0:0:0:0:0" | "localhost" => true,
            "127.0.0.1" | "::1" => !remote_ip.is_loopback(),
            _ => false,
        };
        if needs_replace {
            host = remote_ip.to_string();
        }
    }

    let display_addr = format_host_port(&host, port);
    let connect_addr = format!("http://{}", display_addr);
    (display_addr, connect_addr)
}

async fn connect_peer_with_timeout(
    addr: &str,
    timeout_budget: Duration,
) -> Result<DistributedNeuromorphicClient<tonic::transport::Channel>, String> {
    let target = if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{}", addr)
    };
    match tokio::time::timeout(
        timeout_budget,
        DistributedNeuromorphicClient::connect(target.clone()),
    )
    .await
    {
        Ok(Ok(client)) => {
            let grpc_max_msg_bytes = grpc_max_message_bytes();
            Ok(client
                .max_decoding_message_size(grpc_max_msg_bytes)
                .max_encoding_message_size(grpc_max_msg_bytes))
        }
        Ok(Err(e)) => Err(format!("connect failed for {}: {}", target, e)),
        Err(_) => Err(format!("connect timeout for {}", target)),
    }
}

async fn connect_peer(
    addr: &str,
) -> Result<DistributedNeuromorphicClient<tonic::transport::Channel>, String> {
    connect_peer_with_timeout(addr, Duration::from_secs(3)).await
}

fn env_flag(name: &str) -> Option<bool> {
    match std::env::var(name)
        .ok()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Represents a partial or whole neural network running on this node.
pub struct ManagedNetwork {
    pub id: String,
    pub runner: Runner,
    pub assigned_layers: Vec<u32>,
    pub redundant_layers: Vec<u32>,
    /// Spikes received from other nodes for layers adjacent to our assigned layers.
    /// Key: layer_index, Value: spikes
    pub remote_spikes_fwd: HashMap<u32, Vec<i8>>,
    pub remote_spikes_bwd: HashMap<u32, Vec<i8>>,
    /// Last received step index per layer (forward/backward).
    pub remote_spike_steps_fwd: HashMap<u32, i64>,
    pub remote_spike_steps_bwd: HashMap<u32, i64>,
    /// Optional external sensory spikes to apply on the next step.
    pub external_sensory_spikes: Option<Vec<i8>>,
    pub avg_step_time_ms: f32,
    pub desired_aarnn_depth: u32,
    pub playing: bool,
    pub initial_config: NetworkConfig,
    pub initial_model: NeuronModel,
    pub initial_learning: Learning,
    pub initial_lif: LIFParams,
    pub initial_stdp: STDPParams,
    /// Fingerprint of the most recently applied distributed config/snapshot payload.
    /// Used to avoid expensive no-op reimports on periodic rebalance heartbeats.
    pub last_config_fingerprint: Option<u64>,
    pub workspace_binding: Option<NetworkWorkspaceBinding>,
}

fn config_payload_fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

struct SpikeStreamHandle {
    tx: mpsc::Sender<SpikeBatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpikeTransportMethod {
    Mpi,
    PersistentStream,
    BurstStream,
}

impl SpikeTransportMethod {
    fn as_str(self) -> &'static str {
        match self {
            SpikeTransportMethod::Mpi => "mpi",
            SpikeTransportMethod::PersistentStream => "persistent-grpc-stream",
            SpikeTransportMethod::BurstStream => "burst-grpc-stream",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct SpikeTransportStats {
    preferred: Option<SpikeTransportMethod>,
    mpi_ewma_us: Option<f64>,
    stream_ewma_us: Option<f64>,
    burst_ewma_us: Option<f64>,
    mpi_fail_streak: u32,
    stream_fail_streak: u32,
    burst_fail_streak: u32,
}

pub struct NodeState {
    pub node_id: String,
    pub networks: HashMap<String, Arc<RwLock<ManagedNetwork>>>,
    pub workspace_bindings: HashMap<String, NetworkWorkspaceBinding>,
    pub peers: HashMap<String, String>, // node_id -> address
    pub network_peers: HashMap<String, Vec<String>>, // network_id -> node ids
    pub peer_last_seen: HashMap<String, std::time::Instant>,
    pub clients: HashMap<
        String,
        proto::distributed_neuromorphic_client::DistributedNeuromorphicClient<
            tonic::transport::Channel,
        >,
    >,
    pub _orchestrator_addr: Option<String>,
    pub is_orchestrator: bool,
    spike_streams: HashMap<String, SpikeStreamHandle>,
    spike_stream_backoff: HashMap<String, std::time::Instant>,
    spike_drop_counts: HashMap<String, u64>,
    spike_transport_stats: HashMap<String, SpikeTransportStats>,
    #[cfg(feature = "openmpi")]
    mpi_receiver_started: bool,

    // Cluster-wide status (only relevant if is_orchestrator)
    pub nodes: HashMap<String, NodeStatus>,
    pub network_registry: HashMap<String, NetworkStatus>,
    pub network_snapshots: HashMap<String, String>,
    pub last_heartbeat: HashMap<String, std::time::Instant>,
    pub pending_commands: HashMap<String, Vec<NetworkCommand>>, // node_id -> commands

    // Local GA status (for reporting to orchestrator)
    pub ga_running: bool,
    pub ga_generation: u32,
    pub ga_best_fitness: f64,
    pub ga_best_config_json: String,
    pub ga_evaluating: bool,
    pub ga_eval_progress: f32,
    pub ga_total_evaluations: u64,
    pub ga_active_eval_seed: u64,
    pub ga_inflight_by_peer: HashMap<String, usize>,
}

impl NodeState {
    pub fn prune_peer_maps(&mut self, now: std::time::Instant, ttl: Duration) {
        self.peer_last_seen
            .retain(|_, last| now.duration_since(*last) <= ttl);
        self.peers
            .retain(|node_id, _| self.peer_last_seen.contains_key(node_id));
        for peers in self.network_peers.values_mut() {
            peers.retain(|node_id| self.peers.contains_key(node_id) && node_id != &self.node_id);
        }
        self.network_peers.retain(|_, peers| !peers.is_empty());
    }

    fn choose_spike_transport(
        &self,
        key: &str,
        has_stream: bool,
        has_mpi: bool,
    ) -> SpikeTransportMethod {
        let mut available = vec![SpikeTransportMethod::BurstStream];
        if has_stream {
            available.push(SpikeTransportMethod::PersistentStream);
        }
        if has_mpi {
            available.push(SpikeTransportMethod::Mpi);
        }

        let Some(stats) = self.spike_transport_stats.get(key) else {
            return if has_mpi {
                SpikeTransportMethod::Mpi
            } else if has_stream {
                SpikeTransportMethod::PersistentStream
            } else {
                SpikeTransportMethod::BurstStream
            };
        };

        let fail_streak = |m: SpikeTransportMethod| -> u32 {
            match m {
                SpikeTransportMethod::Mpi => stats.mpi_fail_streak,
                SpikeTransportMethod::PersistentStream => stats.stream_fail_streak,
                SpikeTransportMethod::BurstStream => stats.burst_fail_streak,
            }
        };
        let ewma = |m: SpikeTransportMethod| -> Option<f64> {
            match m {
                SpikeTransportMethod::Mpi => stats.mpi_ewma_us,
                SpikeTransportMethod::PersistentStream => stats.stream_ewma_us,
                SpikeTransportMethod::BurstStream => stats.burst_ewma_us,
            }
        };

        let viable: Vec<SpikeTransportMethod> = available
            .iter()
            .copied()
            .filter(|m| fail_streak(*m) < SPIKE_FAILOVER_STREAK)
            .collect();
        let candidates: Vec<SpikeTransportMethod> = if viable.is_empty() {
            available.clone()
        } else {
            viable
        };

        if let Some(pref) = stats.preferred {
            if candidates.contains(&pref) && fail_streak(pref) < SPIKE_FAILOVER_STREAK {
                return pref;
            }
        }

        let mut best_method = None;
        let mut best_ewma = f64::MAX;
        for method in &candidates {
            if let Some(sample) = ewma(*method) {
                if sample < best_ewma {
                    best_ewma = sample;
                    best_method = Some(*method);
                }
            }
        }
        if let Some(best) = best_method {
            return best;
        }

        if candidates.contains(&SpikeTransportMethod::Mpi) {
            SpikeTransportMethod::Mpi
        } else if candidates.contains(&SpikeTransportMethod::PersistentStream) {
            SpikeTransportMethod::PersistentStream
        } else {
            SpikeTransportMethod::BurstStream
        }
    }

    fn record_spike_transport_success(
        &mut self,
        key: &str,
        method: SpikeTransportMethod,
        elapsed: Duration,
    ) {
        fn update_ewma(slot: &mut Option<f64>, sample_us: f64) {
            if let Some(cur) = slot {
                *cur = (*cur * (1.0 - SPIKE_LATENCY_EWMA_ALPHA))
                    + (sample_us * SPIKE_LATENCY_EWMA_ALPHA);
            } else {
                *slot = Some(sample_us);
            }
        }

        let sample_us = elapsed.as_micros() as f64;
        let stats = self
            .spike_transport_stats
            .entry(key.to_string())
            .or_default();
        let prev_pref = stats.preferred;

        match method {
            SpikeTransportMethod::Mpi => {
                stats.mpi_fail_streak = 0;
                update_ewma(&mut stats.mpi_ewma_us, sample_us);
            }
            SpikeTransportMethod::PersistentStream => {
                stats.stream_fail_streak = 0;
                update_ewma(&mut stats.stream_ewma_us, sample_us);
            }
            SpikeTransportMethod::BurstStream => {
                stats.burst_fail_streak = 0;
                update_ewma(&mut stats.burst_ewma_us, sample_us);
            }
        }

        let mut best = (
            SpikeTransportMethod::BurstStream,
            stats.burst_ewma_us.unwrap_or(f64::MAX),
        );
        let stream_ewma = stats.stream_ewma_us.unwrap_or(f64::MAX);
        if stream_ewma < best.1 {
            best = (SpikeTransportMethod::PersistentStream, stream_ewma);
        }
        let mpi_ewma = stats.mpi_ewma_us.unwrap_or(f64::MAX);
        if mpi_ewma < best.1 {
            best = (SpikeTransportMethod::Mpi, mpi_ewma);
        }
        if best.1.is_finite() {
            stats.preferred = Some(best.0);
        } else {
            stats.preferred = Some(method);
        }

        if prev_pref != stats.preferred {
            nm_log!(
                "[info] Spike transport switched for {} -> {}",
                key,
                stats.preferred.unwrap_or(method).as_str()
            );
        }
    }

    fn record_spike_transport_failure(&mut self, key: &str, method: SpikeTransportMethod) {
        let stats = self
            .spike_transport_stats
            .entry(key.to_string())
            .or_default();
        match method {
            SpikeTransportMethod::Mpi => {
                stats.mpi_fail_streak = stats.mpi_fail_streak.saturating_add(1);
                if stats.mpi_fail_streak >= SPIKE_FAILOVER_STREAK {
                    stats.preferred = Some(if stats.stream_fail_streak < SPIKE_FAILOVER_STREAK {
                        SpikeTransportMethod::PersistentStream
                    } else {
                        SpikeTransportMethod::BurstStream
                    });
                }
            }
            SpikeTransportMethod::PersistentStream => {
                stats.stream_fail_streak = stats.stream_fail_streak.saturating_add(1);
                if stats.stream_fail_streak >= SPIKE_FAILOVER_STREAK {
                    stats.preferred = Some(if stats.mpi_fail_streak < SPIKE_FAILOVER_STREAK {
                        SpikeTransportMethod::Mpi
                    } else {
                        SpikeTransportMethod::BurstStream
                    });
                }
            }
            SpikeTransportMethod::BurstStream => {
                stats.burst_fail_streak = stats.burst_fail_streak.saturating_add(1);
                if stats.burst_fail_streak >= SPIKE_FAILOVER_STREAK {
                    stats.preferred = Some(if stats.mpi_fail_streak < SPIKE_FAILOVER_STREAK {
                        SpikeTransportMethod::Mpi
                    } else {
                        SpikeTransportMethod::PersistentStream
                    });
                }
            }
        }
    }

    fn record_spike_drop(&mut self, key: &str, count: u64) {
        let entry = self.spike_drop_counts.entry(key.to_string()).or_insert(0);
        *entry = entry.saturating_add(count);
    }
}

#[derive(Clone)]
pub struct DistributedNode {
    pub state: Arc<RwLock<NodeState>>,
    pub system: Arc<RwLock<System>>,
}

impl DistributedNode {
    pub fn new(node_id: String, is_orchestrator: bool) -> Self {
        Self {
            state: Arc::new(RwLock::new(NodeState {
                node_id,
                networks: HashMap::new(),
                workspace_bindings: load_workspace_bindings_from_env(),
                peers: HashMap::new(),
                network_peers: HashMap::new(),
                peer_last_seen: HashMap::new(),
                clients: HashMap::new(),
                _orchestrator_addr: None,
                is_orchestrator,
                spike_streams: HashMap::new(),
                spike_stream_backoff: HashMap::new(),
                spike_drop_counts: HashMap::new(),
                spike_transport_stats: HashMap::new(),
                #[cfg(feature = "openmpi")]
                mpi_receiver_started: false,
                nodes: HashMap::new(),
                network_registry: HashMap::new(),
                network_snapshots: HashMap::new(),
                last_heartbeat: HashMap::new(),
                pending_commands: HashMap::new(),
                ga_running: false,
                ga_generation: 0,
                ga_best_fitness: 0.0,
                ga_best_config_json: String::new(),
                ga_evaluating: false,
                ga_eval_progress: 0.0,
                ga_total_evaluations: 0,
                ga_active_eval_seed: 0,
                ga_inflight_by_peer: HashMap::new(),
            })),
            system: Arc::new(RwLock::new(System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            ))),
        }
    }

    #[allow(dead_code)]
    pub fn apply_network_control(
        &self,
        network_id: &str,
        action: proto::control_update::Action,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .try_write()
            .map_err(|_| "Cluster state busy".to_string())?;
        let mut cmd_type = command_type_from_action(action);
        let mut found = false;
        let mut local_busy = false;
        let mut config_payload: Option<Vec<u8>> = None;
        let mut use_distribution_layers = false;
        let mut command_model = String::new();
        let mut command_learning = String::new();

        if let Some(net_arc) = state.networks.get(network_id) {
            found = true;
            match net_arc.try_write() {
                Ok(mut net) => {
                    apply_control_to_managed_network(&mut net, action);
                }
                Err(_) => {
                    local_busy = true;
                }
            }
        }

        let (network_registry, network_snapshots, pending_commands) = {
            let state = &mut *state;
            (
                &mut state.network_registry,
                &mut state.network_snapshots,
                &mut state.pending_commands,
            )
        };

        if let Some(net_status) = network_registry.get_mut(network_id) {
            found = true;
            match action {
                proto::control_update::Action::Start | proto::control_update::Action::Repeat => {
                    net_status.playing = true;
                }
                proto::control_update::Action::Stop
                | proto::control_update::Action::Reset
                | proto::control_update::Action::New => {
                    net_status.playing = false;
                }
            }
            if matches!(action, proto::control_update::Action::New) {
                let model =
                    NeuronModel::from_str(&net_status.neuron_model).unwrap_or(NeuronModel::Aarnn);
                let learning =
                    Learning::from_str(&net_status.learning_rule).unwrap_or(Learning::Aarnn);
                let (fresh_cfg, fresh_json) =
                    fresh_single_neuron_snapshot(net_status.desired_aarnn_depth, model, learning)?;
                net_status.config_json = fresh_json.clone();
                net_status.num_layers = (fresh_cfg.num_hidden_layers + 1) as u32;
                if net_status.neuron_model.is_empty() {
                    net_status.neuron_model = model.to_str().to_string();
                }
                if net_status.learning_rule.is_empty() {
                    net_status.learning_rule = learning.to_str().to_string();
                }
                network_snapshots.insert(network_id.to_string(), fresh_json.clone());
                config_payload = Some(fresh_json.into_bytes());
                use_distribution_layers = true;
                cmd_type = proto::network_command::CommandType::LoadNetwork;
                command_model = net_status.neuron_model.clone();
                command_learning = net_status.learning_rule.clone();
            }
            let desired_depth = net_status.desired_aarnn_depth;
            let node_ids: Vec<String> = net_status.distribution.keys().cloned().collect();
            for node_id in node_ids {
                let (layers, redundant_layers) = if use_distribution_layers {
                    if let Some(range) = net_status.distribution.get(&node_id) {
                        let layers: Vec<u32> = range
                            .layers
                            .iter()
                            .copied()
                            .filter(|l| (*l as usize) < net_status.num_layers as usize)
                            .collect();
                        (layers.clone(), layers)
                    } else {
                        (Vec::new(), Vec::new())
                    }
                } else {
                    (Vec::new(), Vec::new())
                };
                let cmd = NetworkCommand {
                    r#type: cmd_type as i32,
                    network_id: network_id.to_string(),
                    config_json: config_payload.clone().unwrap_or_default(),
                    layers,
                    redundant_layers,
                    desired_aarnn_depth: desired_depth,
                    neuron_model: if use_distribution_layers {
                        command_model.clone()
                    } else {
                        String::new()
                    },
                    learning_rule: if use_distribution_layers {
                        command_learning.clone()
                    } else {
                        String::new()
                    },
                };
                enqueue_pending_command(pending_commands, node_id, cmd);
            }
        }

        if !found {
            return Err("Network not found".to_string());
        }
        if local_busy {
            return Err("Local network busy; command queued for cluster nodes".to_string());
        }
        Ok(())
    }

    pub async fn start_discovery_beacon(
        grpc_addr: String,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;

        let msg = format!("NEUROMORPHIC_ORCHESTRATOR:{}", grpc_addr);
        let targets = vec![
            "255.255.255.255:50050".parse::<SocketAddr>()?,
            "127.0.0.1:50050".parse::<SocketAddr>()?,
        ];

        nm_log!("[info] Discovery beacon started (port 50050)");

        tokio::spawn(async move {
            loop {
                if *shutdown.borrow() {
                    break;
                }
                for &target in &targets {
                    let _ = socket.send_to(msg.as_bytes(), target).await;
                }
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                }
            }
        });
        Ok(())
    }

    pub async fn discover_orchestrator() -> anyhow::Result<String> {
        let socket = UdpSocket::bind("0.0.0.0:50050").await?;
        nm_log!("[info] Waiting for orchestrator discovery beacon...");

        let mut buf = [0u8; 1024];
        loop {
            if let Ok((len, src_addr)) = socket.recv_from(&mut buf).await {
                let msg = String::from_utf8_lossy(&buf[..len]);
                if msg.starts_with("NEUROMORPHIC_ORCHESTRATOR:") {
                    let mut addr = msg
                        .trim_start_matches("NEUROMORPHIC_ORCHESTRATOR:")
                        .to_string();
                    if addr.starts_with("0.0.0.0") {
                        addr = addr.replace("0.0.0.0", &src_addr.ip().to_string());
                    }
                    let full_addr = if addr.starts_with("http") {
                        addr
                    } else {
                        format!("http://{}", addr)
                    };
                    nm_log!("[info] Discovered orchestrator at {}", full_addr);
                    return Ok(full_addr);
                }
            }
        }
    }

    pub async fn get_resources(&self) -> Resources {
        let mut sys = self.system.write().await;
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let state = self.state.read().await;
        let mut total_node_neurons = 0u64;
        let mut redundant_node_neurons = 0u64;
        let mut max_current_depth = 0u32;
        let mut max_desired_depth = 0u32;
        let mut total_desired_dt = 1.0;
        let mut total_avg_step_time = 0.0f32;
        let mut count = 0;

        for net_arc in state.networks.values() {
            let net = net_arc.read().await;
            let mut net_neurons = 0u64;
            let mut red_neurons = 0u64;
            for &l in &net.assigned_layers {
                let size = if (l as usize) < net.runner.net.num_hidden_layers {
                    net.runner.layer_size(l as usize) as u64
                } else if (l as usize) == net.runner.net.num_hidden_layers {
                    net.runner.net.num_output_neurons as u64
                } else {
                    0
                };
                net_neurons += size;
                if net.redundant_layers.contains(&l) {
                    red_neurons += size;
                }
            }
            total_node_neurons += net_neurons;
            redundant_node_neurons += red_neurons;
            max_current_depth = max_current_depth.max(net.runner.net.aarnn_layer_depth as u32);
            max_desired_depth = max_desired_depth.max(net.desired_aarnn_depth);
            total_desired_dt += net.runner.lif.dt;
            total_avg_step_time += net.avg_step_time_ms;
            count += 1;
        }
        let desired_dt = if count > 0 {
            total_desired_dt / count as f64
        } else {
            1.0
        };

        let mut capacity = 1.0;
        capacity += (1.0 - sys.global_cpu_usage() / 100.0) * 10.0;
        let mem_ratio = if sys.total_memory() > 0 {
            sys.available_memory() as f32 / sys.total_memory() as f32
        } else {
            0.0
        };
        capacity += mem_ratio * 10.0;
        // Bias node capacity by parallelism so stronger hosts naturally receive
        // more layer assignments during orchestrator rebalancing.
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get() as f32)
            .unwrap_or(1.0)
            .max(1.0);
        capacity += (cpu_cores / 4.0).min(8.0);
        if let Ok(mult_raw) = std::env::var("NM_CAPACITY_MULTIPLIER") {
            if let Ok(mult) = mult_raw.parse::<f32>() {
                if mult.is_finite() && mult > 0.0 {
                    capacity *= mult;
                }
            }
        }

        let temperature_c = {
            #[cfg(feature = "sysinfo")]
            {
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
                max_c.unwrap_or(-1.0)
            }
            #[cfg(not(feature = "sysinfo"))]
            {
                -1.0
            }
        };

        let (ga_pacing, ga_pacing_reason) = crate::ga::ga_pacing_status();
        let ga_ramp = crate::ga::ga_ramp_runtime_status();
        let ga_ramp_active = ga_ramp.is_some();
        let (
            ga_ramp_population,
            ga_ramp_worker_cap,
            ga_ramp_sim_time_ms,
            ga_ramp_eval_ms,
            ga_ramp_eval_neurons,
            ga_ramp_eval_conns,
        ) = if let Some(ramp) = ga_ramp {
            (
                ramp.population_size.min(u32::MAX as usize) as u32,
                ramp.worker_cap.min(u32::MAX as usize) as u32,
                ramp.sim_time_ms,
                ramp.eval_ms.unwrap_or(0),
                ramp.eval_neurons.unwrap_or(0).min(u64::MAX as usize) as u64,
                ramp.eval_conns.unwrap_or(0).min(u64::MAX as usize) as u64,
            )
        } else {
            (0, 0, 0.0, 0, 0, 0)
        };

        let (comm_protocol, peer_comm_protocols) = {
            #[cfg(feature = "openmpi")]
            let mpi_available = crate::openmpi_runtime::spike_transport_available();

            let mut protocols = HashMap::new();
            let mut counts: HashMap<String, usize> = HashMap::new();
            for peer_id in state.peers.keys() {
                let has_stream = state
                    .spike_streams
                    .get(peer_id)
                    .map(|h| !h.tx.is_closed())
                    .unwrap_or(false);
                #[cfg(feature = "openmpi")]
                let has_mpi = mpi_available && mpi_rank_from_node_id(peer_id).is_some();
                #[cfg(not(feature = "openmpi"))]
                let has_mpi = false;
                let method = state
                    .choose_spike_transport(peer_id, has_stream, has_mpi)
                    .as_str()
                    .to_string();
                *counts.entry(method.clone()).or_insert(0) += 1;
                protocols.insert(peer_id.clone(), method);
            }

            let summary = if protocols.is_empty() {
                "local-only".to_string()
            } else if counts.len() == 1 {
                counts
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string())
            } else {
                let mut items = counts
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>();
                items.sort();
                format!("mixed ({})", items.join(", "))
            };

            (summary, protocols)
        };

        Resources {
            cpu_usage: sys.global_cpu_usage(),
            total_ram: sys.total_memory(),
            available_ram: sys.available_memory(),
            num_gpus: 0,
            num_tpus: 0,
            num_fpgas: 0,
            capacity_score: capacity,
            desired_dt,
            num_neurons: total_node_neurons,
            redundant_neurons: redundant_node_neurons,
            current_aarnn_depth: max_current_depth,
            desired_aarnn_depth: max_desired_depth,
            avg_step_time_ms: total_avg_step_time,
            ga_running: state.ga_running,
            ga_generation: state.ga_generation,
            ga_best_fitness: state.ga_best_fitness,
            ga_best_config_json: state.ga_best_config_json.clone(),
            ga_evaluating: state.ga_evaluating,
            ga_eval_progress: state.ga_eval_progress,
            temperature_c,
            ga_pacing,
            ga_pacing_reason,
            ga_total_evaluations: crate::ga::ga_total_evaluations(),
            ga_active_eval_seed: state.ga_active_eval_seed,
            ga_ramp_active,
            ga_ramp_population,
            ga_ramp_worker_cap,
            ga_ramp_sim_time_ms,
            ga_ramp_eval_ms,
            ga_ramp_eval_neurons,
            ga_ramp_eval_conns,
            comm_protocol,
            peer_comm_protocols,
        }
    }

    pub async fn get_network_resources(&self) -> HashMap<String, NetworkResources> {
        let state = self.state.read().await;
        let mut res = HashMap::new();
        for (id, net_arc) in &state.networks {
            let net = net_arc.read().await;
            let mut layer_neuron_counts = HashMap::new();
            let mut total_neurons = 0u64;

            for &l in &net.assigned_layers {
                let size = if (l as usize) < net.runner.net.num_hidden_layers {
                    net.runner.layer_size(l as usize) as u64
                } else if (l as usize) == net.runner.net.num_hidden_layers {
                    net.runner.net.num_output_neurons as u64
                } else {
                    0
                };
                layer_neuron_counts.insert(l, size);
                total_neurons += size;
            }

            res.insert(
                id.clone(),
                NetworkResources {
                    num_neurons: total_neurons,
                    layer_neuron_counts,
                    avg_step_time_ms: net.avg_step_time_ms,
                },
            );
        }
        res
    }

    async fn spike_targets_for_network(
        &self,
        network_id: &str,
        exclude_node: Option<&str>,
    ) -> Vec<(String, String)> {
        let state = self.state.read().await;
        if state.is_orchestrator {
            if let Some(net) = state.network_registry.get(network_id) {
                let mut targets = Vec::new();
                for (node_id, addr) in &state.peers {
                    if Some(node_id.as_str()) == exclude_node {
                        continue;
                    }
                    if net.distribution.contains_key(node_id) {
                        targets.push((node_id.clone(), addr.clone()));
                    }
                }
                return targets;
            }
            return Vec::new();
        }
        if let Some(peers) = state.network_peers.get(network_id) {
            let mut targets = Vec::new();
            for node_id in peers {
                if node_id == &state.node_id {
                    continue;
                }
                if let Some(addr) = state.peers.get(node_id) {
                    targets.push((node_id.clone(), addr.clone()));
                }
            }
            if !targets.is_empty() {
                return targets;
            }
        }
        if let Some(addr) = state._orchestrator_addr.clone() {
            return vec![("orchestrator".to_string(), addr)];
        }
        Vec::new()
    }

    async fn send_spike_batches_burst(
        &self,
        key: &str,
        addr: &str,
        batches: Vec<SpikeBatch>,
    ) -> Result<(), String> {
        if batches.is_empty() {
            return Ok(());
        }

        let cached_client = {
            let state = self.state.read().await;
            state.clients.get(key).cloned()
        };

        let mut client = if let Some(client) = cached_client {
            client
        } else {
            connect_peer_with_timeout(addr, SPIKE_BURST_CONNECT_TIMEOUT).await?
        };

        let (tx, rx) = mpsc::channel::<SpikeBatch>(batches.len().clamp(1, 256));
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
        let response = client
            .stream_spikes(Request::new(outbound))
            .await
            .map_err(|e| format!("burst stream start {} failed: {}", key, e))?;
        let mut inbound = response.into_inner();
        let drain =
            tokio::spawn(async move { while let Ok(Some(_msg)) = inbound.message().await {} });

        for batch in batches {
            tx.send(batch)
                .await
                .map_err(|e| format!("burst stream send {} failed: {}", key, e))?;
        }
        drop(tx);
        let _ = tokio::time::timeout(Duration::from_millis(20), drain).await;

        let mut state = self.state.write().await;
        state.clients.insert(key.to_string(), client);
        Ok(())
    }

    async fn request_spike_stream(&self, key: String, addr: String) {
        let now = std::time::Instant::now();
        {
            let mut state = self.state.write().await;
            if let Some(next) = state.spike_stream_backoff.get(&key) {
                if *next > now {
                    return;
                }
            }
            state
                .spike_stream_backoff
                .insert(key.clone(), now + Duration::from_secs(2));
        }

        let node = self.clone();
        tokio::spawn(async move {
            let mut client = match connect_peer(&addr).await {
                Ok(c) => c,
                Err(e) => {
                    nm_err!("[warn] spike stream connect {} failed: {}", addr, e);
                    return;
                }
            };

            let (tx, rx) = mpsc::channel::<SpikeBatch>(256);
            let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
            let response = client.stream_spikes(Request::new(outbound)).await;

            let mut inbound = match response {
                Ok(resp) => {
                    {
                        let mut state = node.state.write().await;
                        state
                            .spike_streams
                            .insert(key.clone(), SpikeStreamHandle { tx });
                    }
                    resp.into_inner()
                }
                Err(e) => {
                    nm_err!("[warn] spike stream start {} failed: {}", addr, e);
                    return;
                }
            };

            while let Ok(Some(_msg)) = inbound.message().await {}

            let mut state = node.state.write().await;
            state.spike_streams.remove(&key);
        });
    }

    async fn handle_incoming_spike_batch(&self, batch: SpikeBatch, exclude_node: Option<String>) {
        let (network, is_orchestrator) = {
            let state_lock = self.state.read().await;
            (
                state_lock.networks.get(&batch.network_id).cloned(),
                state_lock.is_orchestrator,
            )
        };

        if let Some(net_arc) = network {
            let mut net = net_arc.write().await;
            if batch.layer_index == EXTERNAL_SENSORY_LAYER_INDEX {
                let sensory_len = net.runner.net.num_sensory_neurons;
                let spikes = spikes_from_transport(
                    &batch.aer_payload,
                    batch.aer_base,
                    &batch.spike_indices,
                    sensory_len,
                )
                .unwrap_or_else(|_| vec![0i8; sensory_len]);
                net.external_sensory_spikes = Some(spikes);
            } else {
                let layer_index = batch.layer_index as usize;
                let layer_size = net.runner.layer_size(layer_index);
                if layer_size != 0 {
                    let is_assigned = net.runner.is_layer_assigned(layer_index);
                    let is_redundant = net.redundant_layers.contains(&batch.layer_index);
                    if !is_assigned || is_redundant {
                        let step_map = if batch.is_backward {
                            &mut net.remote_spike_steps_bwd
                        } else {
                            &mut net.remote_spike_steps_fwd
                        };
                        let stale = step_map
                            .get(&batch.layer_index)
                            .map(|prev| batch.step_index < *prev)
                            .unwrap_or(false);
                        if !stale {
                            step_map.insert(batch.layer_index, batch.step_index);
                            let spikes = spikes_from_transport(
                                &batch.aer_payload,
                                batch.aer_base,
                                &batch.spike_indices,
                                layer_size,
                            )
                            .unwrap_or_else(|_| vec![0i8; layer_size]);
                            if batch.is_backward {
                                net.remote_spikes_bwd.insert(batch.layer_index, spikes);
                            } else {
                                net.remote_spikes_fwd.insert(batch.layer_index, spikes);
                            }
                        }
                    }
                }
            }
        }

        if is_orchestrator {
            self.send_spike_batches(
                &batch.network_id,
                std::slice::from_ref(&batch),
                exclude_node.as_deref(),
            )
            .await;
        }
    }

    #[cfg(feature = "openmpi")]
    async fn send_spike_batches_mpi(
        &self,
        key: &str,
        dest_rank: i32,
        batches: Vec<SpikeBatch>,
    ) -> Result<(), String> {
        if batches.is_empty() {
            return Ok(());
        }
        for batch in batches {
            let payload = batch.encode_to_vec();
            crate::openmpi_runtime::send_tagged_bytes(
                dest_rank,
                crate::openmpi_runtime::SPIKE_TRANSPORT_TAG,
                &payload,
            )
            .map_err(|e| format!("MPI send to {} (rank {}) failed: {}", key, dest_rank, e))?;
        }
        Ok(())
    }

    #[cfg(not(feature = "openmpi"))]
    async fn send_spike_batches_mpi(
        &self,
        _key: &str,
        _dest_rank: i32,
        _batches: Vec<SpikeBatch>,
    ) -> Result<(), String> {
        Err("MPI transport not compiled in".to_string())
    }

    pub async fn start_optional_mpi_spike_receiver(&self) {
        #[cfg(not(feature = "openmpi"))]
        {
            return;
        }
        #[cfg(feature = "openmpi")]
        {
            if !crate::openmpi_runtime::spike_transport_available() {
                return;
            }
            {
                let mut state = self.state.write().await;
                if state.mpi_receiver_started {
                    return;
                }
                state.mpi_receiver_started = true;
            }
            let node = self.clone();
            tokio::spawn(async move {
                nm_log!("[info] MPI spike transport receiver enabled");
                loop {
                    match crate::openmpi_runtime::try_recv_tagged_bytes(
                        crate::openmpi_runtime::SPIKE_TRANSPORT_TAG,
                    ) {
                        Ok(Some((src_rank, payload))) => {
                            let batch = match SpikeBatch::decode(payload.as_slice()) {
                                Ok(batch) => batch,
                                Err(e) => {
                                    nm_err!(
                                        "[warn] failed to decode MPI spike payload from rank {}: {}",
                                        src_rank,
                                        e
                                    );
                                    continue;
                                }
                            };
                            let exclude_node = {
                                let state = node.state.read().await;
                                peer_id_from_mpi_rank(&state, src_rank)
                            };
                            node.handle_incoming_spike_batch(batch, exclude_node).await;
                        }
                        Ok(None) => {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                        Err(e) => {
                            nm_err!("[warn] MPI spike receive failed: {}", e);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    }
                }
            });
        }
    }

    async fn send_spike_batches(
        &self,
        network_id: &str,
        batches: &[SpikeBatch],
        exclude_node: Option<&str>,
    ) {
        if batches.is_empty() {
            return;
        }
        let targets = self
            .spike_targets_for_network(network_id, exclude_node)
            .await;
        if targets.is_empty() {
            return;
        }

        for (key, addr) in targets {
            #[cfg(feature = "openmpi")]
            let mpi_rank_opt = if crate::openmpi_runtime::spike_transport_available() {
                mpi_rank_from_node_id(&key)
            } else {
                None
            };
            #[cfg(not(feature = "openmpi"))]
            let mpi_rank_opt: Option<i32> = None;

            let (sender_opt, preferred_transport) = {
                let mut state = self.state.write().await;
                let sender_opt = if let Some(handle) = state.spike_streams.get(&key) {
                    if !handle.tx.is_closed() {
                        Some(handle.tx.clone())
                    } else {
                        state.spike_streams.remove(&key);
                        None
                    }
                } else {
                    None
                };
                let preferred = state.choose_spike_transport(
                    &key,
                    sender_opt.is_some(),
                    mpi_rank_opt.is_some(),
                );
                (sender_opt, preferred)
            };

            let mut methods = vec![preferred_transport];
            if mpi_rank_opt.is_some() && !methods.contains(&SpikeTransportMethod::Mpi) {
                methods.push(SpikeTransportMethod::Mpi);
            }
            if preferred_transport != SpikeTransportMethod::PersistentStream && sender_opt.is_some()
            {
                methods.push(SpikeTransportMethod::PersistentStream);
            }
            if !methods.contains(&SpikeTransportMethod::BurstStream) {
                methods.push(SpikeTransportMethod::BurstStream);
            }

            let mut remaining: Vec<SpikeBatch> = batches.to_vec();
            let mut delivered = false;

            for method in methods {
                if remaining.is_empty() {
                    delivered = true;
                    break;
                }

                match method {
                    SpikeTransportMethod::Mpi => {
                        let Some(dest_rank) = mpi_rank_opt else {
                            let mut state = self.state.write().await;
                            state.record_spike_transport_failure(&key, method);
                            continue;
                        };
                        let mpi_start = std::time::Instant::now();
                        match self
                            .send_spike_batches_mpi(&key, dest_rank, remaining.clone())
                            .await
                        {
                            Ok(()) => {
                                let mut state = self.state.write().await;
                                state.record_spike_transport_success(
                                    &key,
                                    method,
                                    mpi_start.elapsed(),
                                );
                                remaining.clear();
                                delivered = true;
                                break;
                            }
                            Err(e) => {
                                nm_err!("[warn] MPI spike forwarding to {} failed: {}", key, e);
                                let mut state = self.state.write().await;
                                state.record_spike_transport_failure(&key, method);
                            }
                        }
                    }
                    SpikeTransportMethod::PersistentStream => {
                        let Some(sender) = sender_opt.clone() else {
                            let mut state = self.state.write().await;
                            state.record_spike_transport_failure(&key, method);
                            continue;
                        };
                        let stream_start = std::time::Instant::now();
                        let mut sent_count = 0usize;
                        let mut stream_closed = false;
                        for batch in &remaining {
                            match sender.try_send(batch.clone()) {
                                Ok(_) => sent_count += 1,
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => break,
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    stream_closed = true;
                                    break;
                                }
                            }
                        }

                        if sent_count == remaining.len() {
                            let mut state = self.state.write().await;
                            state.record_spike_transport_success(
                                &key,
                                method,
                                stream_start.elapsed(),
                            );
                            delivered = true;
                            break;
                        }

                        remaining = remaining.split_off(sent_count);
                        let mut state = self.state.write().await;
                        state.record_spike_transport_failure(&key, method);
                        if stream_closed {
                            state.spike_streams.remove(&key);
                            state.spike_stream_backoff.insert(
                                key.clone(),
                                std::time::Instant::now() + Duration::from_secs(2),
                            );
                        }
                    }
                    SpikeTransportMethod::BurstStream => {
                        self.request_spike_stream(key.clone(), addr.clone()).await;
                        let burst_start = std::time::Instant::now();
                        let burst_result = tokio::time::timeout(
                            SPIKE_BURST_TIMEOUT,
                            self.send_spike_batches_burst(&key, &addr, remaining.clone()),
                        )
                        .await;
                        match burst_result {
                            Ok(Ok(())) => {
                                let mut state = self.state.write().await;
                                state.record_spike_transport_success(
                                    &key,
                                    method,
                                    burst_start.elapsed(),
                                );
                                remaining.clear();
                                delivered = true;
                                break;
                            }
                            Ok(Err(e)) => {
                                nm_err!("[warn] burst spike forwarding to {} failed: {}", key, e);
                                let mut state = self.state.write().await;
                                state.record_spike_transport_failure(&key, method);
                            }
                            Err(_) => {
                                nm_err!(
                                    "[warn] burst spike forwarding to {} timed out after {:?}",
                                    key,
                                    SPIKE_BURST_TIMEOUT
                                );
                                let mut state = self.state.write().await;
                                state.record_spike_transport_failure(&key, method);
                            }
                        }
                    }
                }
            }

            if !delivered && !remaining.is_empty() {
                let mut state = self.state.write().await;
                state.record_spike_drop(&key, remaining.len() as u64);
            }
        }
    }

    pub async fn rebalance_networks(&self) {
        let mut state = self.state.write().await;
        if !state.is_orchestrator {
            return;
        }

        let node_ids: Vec<String> = state.nodes.keys().cloned().collect();
        if node_ids.is_empty() {
            return;
        }

        // Collect per-node capacity estimates used for layer assignment.
        // Capacity is dynamic: base resource score (CPU/RAM/cores/weight multiplier)
        // scaled by measured step-latency so overloaded/slower nodes receive less work.
        let rebalance_target_step_ms = std::env::var("NM_REBALANCE_TARGET_STEP_MS")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(10.0);
        let mut node_capacities = Vec::new();
        for node_id in &node_ids {
            let cap = state
                .nodes
                .get(node_id)
                .and_then(|n| n.resources.as_ref())
                .map(|r| {
                    let mut effective = r.capacity_score.max(0.1);
                    // Convert measured step latency into a bounded scale factor.
                    // Faster-than-target nodes can absorb moderately more layers;
                    // slower nodes are proportionally de-emphasized.
                    let latency_ms = r.avg_step_time_ms.max(0.0);
                    if latency_ms > 0.0 {
                        let latency_scale =
                            (rebalance_target_step_ms / latency_ms).clamp(0.25, 2.0);
                        effective *= latency_scale;
                    }
                    effective.max(0.05)
                })
                .unwrap_or(1.0);
            node_capacities.push((node_id.clone(), cap));
        }
        let node_capacity_map: HashMap<String, f32> = node_capacities.iter().cloned().collect();
        let mut network_affinity: HashMap<String, Vec<String>> = HashMap::new();
        for (node_id, status) in &state.nodes {
            for net_id in &status.active_networks {
                network_affinity
                    .entry(net_id.clone())
                    .or_default()
                    .push(node_id.clone());
            }
        }

        // Calculate network neurons first to avoid double borrow
        let mut network_neurons = 0u64;
        for status in state.nodes.values() {
            if let Some(res) = &status.resources {
                network_neurons += res.num_neurons;
            }
        }

        let mut all_pending = Vec::new();
        let (network_registry, network_snapshots) = {
            let state = &mut *state;
            (&mut state.network_registry, &mut state.network_snapshots)
        };

        for (net_id, net_status) in network_registry.iter_mut() {
            let mut snapshot_layers: Option<u32> = None;
            let mut config_payload: Option<String> = None;

            if let Some(snap_json) = network_snapshots.get(net_id) {
                config_payload = Some(snap_json.clone());
                if let Ok(snap) = crate::runner::decode_snapshot_with_profile_backfill(snap_json) {
                    snapshot_layers = Some((snap.net.num_hidden_layers + 1) as u32);
                }
            } else if !net_status.config_json.is_empty() {
                if let Ok(snap) =
                    crate::runner::decode_snapshot_with_profile_backfill(&net_status.config_json)
                {
                    let snap_json = net_status.config_json.clone();
                    network_snapshots.insert(net_id.clone(), snap_json.clone());
                    config_payload = Some(snap_json);
                    snapshot_layers = Some((snap.net.num_hidden_layers + 1) as u32);
                }
            }

            let total_layers = if let Some(layers) = snapshot_layers {
                net_status.num_layers = layers;
                layers
            } else if net_status.num_layers > 0 {
                net_status.num_layers
            } else {
                7
            };
            let config_json = config_payload.unwrap_or_else(|| net_status.config_json.clone());
            let mut target_node_capacities: Vec<(String, f32)> = network_affinity
                .get(net_id)
                .into_iter()
                .flatten()
                .filter_map(|node_id| {
                    node_capacity_map
                        .get(node_id)
                        .copied()
                        .map(|cap| (node_id.clone(), cap))
                })
                .collect();
            if target_node_capacities.is_empty() {
                if config_json.len() >= LARGE_NETWORK_CONFIG_BYTES {
                    nm_log!(
                        "[warn] Rebalance deferred for network {}: no eligible nodes advertise it yet (config={} bytes)",
                        net_id,
                        config_json.len()
                    );
                    continue;
                }
                target_node_capacities = node_capacities.clone();
            }
            let mut target_capacity_sum: f32 =
                target_node_capacities.iter().map(|(_, cap)| *cap).sum();
            if target_capacity_sum <= 0.0 {
                target_capacity_sum = target_node_capacities.len() as f32;
            }

            // Preserve existing layer neuron counts to avoid UI flicker during rebalance
            let mut old_counts = HashMap::new();
            for (nid, range) in &net_status.distribution {
                old_counts.insert(nid.clone(), range.layer_neuron_counts.clone());
            }

            net_status.distribution.clear();

            let mut layer_counts = vec![0u32; total_layers as usize];
            let mut node_assignments = Vec::new();

            let mut current_cap_sum = 0.0;
            for (node_id, cap) in &target_node_capacities {
                let start_ratio = current_cap_sum / target_capacity_sum;
                current_cap_sum += cap;
                let end_ratio = current_cap_sum / target_capacity_sum;

                let start = (start_ratio * total_layers as f32).round() as u32;
                let end = (end_ratio * total_layers as f32).round() as u32;

                // Ensure at least one layer if there's any capacity
                let end = if start == end && end < total_layers {
                    end + 1
                } else {
                    end
                };

                // Add overlap for boundary synchronization/redundancy
                let r_start = start.saturating_sub(1);
                let r_end = (end + 1).min(total_layers);

                let layers: Vec<u32> = (r_start..r_end).collect();
                for &l in &layers {
                    if (l as usize) < layer_counts.len() {
                        layer_counts[l as usize] += 1;
                    }
                }
                node_assignments.push((node_id.clone(), layers));
            }

            for (node_id, layers) in node_assignments {
                net_status.distribution.insert(
                    node_id.clone(),
                    LayerRange {
                        layers: layers.clone(),
                        layer_neuron_counts: old_counts.remove(&node_id).unwrap_or_default(),
                    },
                );

                let redundant: Vec<u32> = layers
                    .iter()
                    .filter(|&&l| (l as usize) < layer_counts.len() && layer_counts[l as usize] > 1)
                    .copied()
                    .collect();

                let cmd = NetworkCommand {
                    r#type: proto::network_command::CommandType::LoadNetwork as i32,
                    network_id: net_id.clone(),
                    config_json: config_json.as_bytes().to_vec(),
                    layers: layers.clone(),
                    redundant_layers: redundant,
                    desired_aarnn_depth: net_status.desired_aarnn_depth,
                    neuron_model: net_status.neuron_model.clone(),
                    learning_rule: net_status.learning_rule.clone(),
                };
                let node_id_clone = node_id.clone();
                all_pending.push((node_id, cmd));
                if !net_status.playing {
                    let stop_cmd = NetworkCommand {
                        r#type: proto::network_command::CommandType::Stop as i32,
                        network_id: net_id.clone(),
                        config_json: Vec::new(),
                        layers: Vec::new(),
                        redundant_layers: Vec::new(),
                        desired_aarnn_depth: net_status.desired_aarnn_depth,
                        neuron_model: String::new(),
                        learning_rule: String::new(),
                    };
                    all_pending.push((node_id_clone, stop_cmd));
                }
            }

            // Update total neurons from distribution reports if available
            let mut calculated_total = 0u64;
            let mut seen_layers = std::collections::HashSet::new();
            for range in net_status.distribution.values() {
                for (&l, &count) in &range.layer_neuron_counts {
                    if !seen_layers.contains(&l) {
                        calculated_total += count;
                        seen_layers.insert(l);
                    }
                }
            }
            if calculated_total > 0 {
                net_status.total_neurons = calculated_total;
            } else if net_status.total_neurons == 0 {
                // Fallback to cluster-wide neuron count if no per-network data yet
                net_status.total_neurons = network_neurons;
            }
        }

        for (node_id, cmd) in all_pending {
            enqueue_pending_command(&mut state.pending_commands, node_id, cmd);
        }
    }

    pub async fn handle_command(&self, cmd: NetworkCommand) {
        use proto::network_command::CommandType;
        let mut state = self.state.write().await;

        let cmd_type = CommandType::try_from(cmd.r#type).unwrap_or(CommandType::Stop);
        match cmd_type {
            CommandType::LoadNetwork => {
                if let Some(net_arc) = state.networks.get(&cmd.network_id) {
                    let mut net = net_arc.write().await;
                    let layers_changed = net.assigned_layers != cmd.layers
                        || net.redundant_layers != cmd.redundant_layers;
                    let depth_changed = net.desired_aarnn_depth != cmd.desired_aarnn_depth;
                    let incoming_cfg_fp = (!cmd.config_json.is_empty())
                        .then(|| config_payload_fingerprint(&cmd.config_json));
                    let config_changed =
                        incoming_cfg_fp.is_some() && incoming_cfg_fp != net.last_config_fingerprint;
                    let requested_model = if !cmd.neuron_model.is_empty() {
                        NeuronModel::from_str(&cmd.neuron_model)
                    } else {
                        None
                    };
                    let model_changed = requested_model
                        .map(|m| net.runner.neuron_model != m)
                        .unwrap_or(false);
                    let requested_learning = if !cmd.learning_rule.is_empty() {
                        Learning::from_str(&cmd.learning_rule)
                    } else {
                        None
                    };
                    let learning_changed = requested_learning
                        .map(|l| net.runner.learning != l)
                        .unwrap_or(false);

                    if !layers_changed
                        && !depth_changed
                        && !config_changed
                        && !model_changed
                        && !learning_changed
                    {
                        return;
                    }

                    nm_log!(
                        "[info] Updating network {} layers to {:?} (redundant: {:?}){}",
                        cmd.network_id,
                        cmd.layers,
                        cmd.redundant_layers,
                        if config_changed {
                            " [config changed]"
                        } else {
                            ""
                        }
                    );
                    net.assigned_layers = cmd.layers;
                    net.redundant_layers = cmd.redundant_layers;
                    net.desired_aarnn_depth = cmd.desired_aarnn_depth;
                    net.remote_spikes_fwd.clear();
                    net.remote_spikes_bwd.clear();
                    net.remote_spike_steps_fwd.clear();
                    net.remote_spike_steps_bwd.clear();

                    if config_changed {
                        let cfg_str = String::from_utf8_lossy(&cmd.config_json).to_string();
                        if let Ok(_snap) =
                            crate::runner::decode_snapshot_with_profile_backfill(&cfg_str)
                        {
                            #[cfg(feature = "growth3d")]
                            let has_snapshot_topo = _snap.topo.is_some();
                            if let Err(e) = net.runner.import_network_json(&cfg_str) {
                                nm_err!(
                                    "[warn] Failed to import snapshot for {}: {}",
                                    cmd.network_id,
                                    e
                                );
                            }
                            net.last_config_fingerprint = incoming_cfg_fp;
                            if !net.assigned_layers.is_empty() {
                                if let (Some(min), Some(max)) = (
                                    net.assigned_layers.iter().min(),
                                    net.assigned_layers.iter().max(),
                                ) {
                                    net.runner.layer_range =
                                        Some(*min as usize..(*max as usize + 1));
                                    #[cfg(feature = "growth3d")]
                                    if !has_snapshot_topo {
                                        net.runner.rebuild_default_topology();
                                    }
                                }
                            }
                        } else if let Ok(new_cfg) = serde_json::from_str::<NetworkConfig>(&cfg_str)
                        {
                            net.runner.apply_config(new_cfg);
                            net.last_config_fingerprint = incoming_cfg_fp;
                        }
                    }
                    if layers_changed && !net.assigned_layers.is_empty() {
                        if let (Some(min), Some(max)) = (
                            net.assigned_layers.iter().min(),
                            net.assigned_layers.iter().max(),
                        ) {
                            net.runner.layer_range = Some(*min as usize..(*max as usize + 1));
                        }
                    }
                    if !cmd.neuron_model.is_empty() {
                        if let Some(m) = NeuronModel::from_str(&cmd.neuron_model) {
                            if net.runner.neuron_model != m {
                                net.runner.set_model(m);
                            }
                        }
                    }
                    if !cmd.learning_rule.is_empty() {
                        if let Some(l) = Learning::from_str(&cmd.learning_rule) {
                            if net.runner.learning != l {
                                net.runner.set_learning(l);
                            }
                        }
                    }
                } else {
                    nm_log!("[info] Loading network {} with layers {:?} (redundant: {:?}, depth: {}, model: {}, learning: {})", 
                        cmd.network_id, cmd.layers, cmd.redundant_layers, cmd.desired_aarnn_depth,
                        cmd.neuron_model, cmd.learning_rule);

                    let mut snapshot_json: Option<String> = None;
                    #[cfg(feature = "growth3d")]
                    let mut snapshot_has_topo = false;
                    let mut net_cfg = if !cmd.config_json.is_empty() {
                        let cfg_str = String::from_utf8_lossy(&cmd.config_json).to_string();
                        if let Ok(snap) =
                            crate::runner::decode_snapshot_with_profile_backfill(&cfg_str)
                        {
                            #[cfg(feature = "growth3d")]
                            {
                                snapshot_has_topo = snap.topo.is_some();
                            }
                            snapshot_json = Some(cfg_str);
                            snap.net
                        } else {
                            serde_json::from_str(&cfg_str).unwrap_or_else(|e| {
                                nm_err!(
                                    "[error] Failed to parse config JSON in LoadNetwork: {}",
                                    e
                                );
                                NetworkConfig::default()
                            })
                        }
                    } else {
                        let mut cfg = NetworkConfig::default();
                        cfg.aarnn_layer_depth = cmd.desired_aarnn_depth as usize;
                        cfg
                    };
                    // Default distributed networks to full AARNN mode if not specified.
                    if cmd.neuron_model.is_empty() || cmd.neuron_model == "aarnn" {
                        net_cfg.growth_enabled = true;
                        net_cfg.use_morphology = true;
                        net_cfg.use_aarnn_delays = true;
                        net_cfg.morpho_growth_enabled = true;
                        net_cfg.aarnn_layer_depth = cmd.desired_aarnn_depth as usize;
                        if net_cfg.aarnn_velocity <= 0.0 {
                            net_cfg.aarnn_velocity = 10.0;
                        }
                    }

                    let model = if !cmd.neuron_model.is_empty() {
                        NeuronModel::from_str(&cmd.neuron_model).unwrap_or(NeuronModel::Aarnn)
                    } else {
                        NeuronModel::Aarnn
                    };
                    let learning = if !cmd.learning_rule.is_empty() {
                        Learning::from_str(&cmd.learning_rule).unwrap_or(Learning::Aarnn)
                    } else {
                        Learning::Aarnn
                    };

                    let desired_depth = cmd.desired_aarnn_depth;
                    let lif = LIFParams::default();
                    let stdp = STDPParams::default();
                    let mut runner =
                        Runner::new(lif.clone(), stdp.clone(), net_cfg.clone(), model, learning);

                    if let Some(json) = snapshot_json {
                        if let Err(e) = runner.import_network_json(&json) {
                            nm_err!(
                                "[error] Failed to import snapshot JSON in LoadNetwork: {}",
                                e
                            );
                        }
                    }

                    if !cmd.layers.is_empty() {
                        let min = *cmd.layers.iter().min().unwrap() as usize;
                        let max = *cmd.layers.iter().max().unwrap() as usize + 1;
                        runner.layer_range = Some(min..max);
                        #[cfg(feature = "growth3d")]
                        if !snapshot_has_topo {
                            runner.rebuild_default_topology();
                        }
                    }

                    let network_id = cmd.network_id.clone();
                    let workspace_binding = state.workspace_bindings.get(&network_id).cloned();

                    state.networks.insert(
                        network_id.clone(),
                        Arc::new(RwLock::new(ManagedNetwork {
                            id: network_id,
                            runner,
                            assigned_layers: cmd.layers,
                            redundant_layers: cmd.redundant_layers,
                            remote_spikes_fwd: HashMap::new(),
                            remote_spikes_bwd: HashMap::new(),
                            remote_spike_steps_fwd: HashMap::new(),
                            remote_spike_steps_bwd: HashMap::new(),
                            external_sensory_spikes: None,
                            avg_step_time_ms: 0.0,
                            desired_aarnn_depth: desired_depth,
                            playing: true,
                            initial_config: net_cfg,
                            initial_model: model,
                            initial_learning: learning,
                            initial_lif: lif,
                            initial_stdp: stdp,
                            last_config_fingerprint: (!cmd.config_json.is_empty())
                                .then(|| config_payload_fingerprint(&cmd.config_json)),
                            workspace_binding,
                        })),
                    );
                }
            }
            CommandType::Start | CommandType::Stop | CommandType::Repeat | CommandType::Reset => {
                if let Some(net_arc) = state.networks.get(&cmd.network_id) {
                    let mut net = net_arc.write().await;
                    if let Some(action) = control_action_from_command(cmd_type) {
                        apply_control_to_managed_network(&mut net, action);
                    }
                }
            }
            _ => {}
        }
    }

    pub async fn run_simulation(&self, mut shutdown: watch::Receiver<bool>) {
        let node_id = self.state.read().await.node_id.clone();
        nm_log!("[info] Node {} simulation loop started", node_id);

        loop {
            if *shutdown.borrow() {
                break;
            }
            let networks = {
                let state = self.state.read().await;
                state.networks.values().cloned().collect::<Vec<_>>()
            };

            if networks.is_empty() {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            let mut any_playing = false;
            for net_arc in networks {
                if *shutdown.borrow() {
                    break;
                }
                observe_time!("distributed/node_step");
                let step_start = std::time::Instant::now();
                let mut net = net_arc.write().await;
                if !net.playing {
                    continue;
                }
                any_playing = true;

                // Sync remote spikes into runner before stepping
                let fwd_spikes = std::mem::take(&mut net.remote_spikes_fwd);
                for (l, spikes) in fwd_spikes {
                    if (l as usize) < net.runner.last_spk_h.len() {
                        let sz = net.runner.layer_size(l as usize);
                        if spikes.len() == sz {
                            net.runner.last_spk_h[l as usize] = ndarray::Array1::from_vec(spikes);
                        } else {
                            let mut arr = ndarray::Array1::zeros(sz);
                            let n = sz.min(spikes.len());
                            for i in 0..n {
                                arr[i] = spikes[i];
                            }
                            net.runner.last_spk_h[l as usize] = arr;
                        }
                    }
                }
                let bwd_spikes = std::mem::take(&mut net.remote_spikes_bwd);
                for (l, spikes) in bwd_spikes {
                    if (l as usize) < net.runner.last_spk_h.len() {
                        let sz = net.runner.layer_size(l as usize);
                        if spikes.len() == sz {
                            net.runner.last_spk_h[l as usize] = ndarray::Array1::from_vec(spikes);
                        } else {
                            let mut arr = ndarray::Array1::zeros(sz);
                            let n = sz.min(spikes.len());
                            for i in 0..n {
                                arr[i] = spikes[i];
                            }
                            net.runner.last_spk_h[l as usize] = arr;
                        }
                    }
                }

                let external_sensory = net.external_sensory_spikes.take();
                let out = if let Some(ref sensory) = external_sensory {
                    net.runner.step(Some(sensory.as_slice()))
                } else {
                    net.runner.step(None)
                };

                let step_index = out.t as i64;
                let ts_us = (net.runner.t_ms * 1000.0) as u64;
                let net_id = net.id.clone();
                let num_hidden = net.runner.net.num_hidden_layers as u32;
                let mut batches = Vec::new();
                for &l in &net.redundant_layers {
                    if l >= num_hidden {
                        continue;
                    }
                    let layer_idx = l as usize;
                    if layer_idx >= net.runner.last_spk_h.len() {
                        continue;
                    }
                    let layer_spikes: Vec<i8> =
                        net.runner.last_spk_h[layer_idx].iter().copied().collect();
                    let exchange = encode_exchange(ts_us, 0, &layer_spikes);
                    let indices = exchange.spike_indices;
                    let mut aer_payload = exchange.aer_payload;
                    if aer_payload.is_empty() {
                        aer_payload.extend_from_slice(b"AER1");
                        aer_payload.extend_from_slice(&ts_us.to_le_bytes());
                    }
                    batches.push(SpikeBatch {
                        network_id: net_id.clone(),
                        layer_index: l,
                        step_index,
                        spike_indices: indices,
                        is_backward: false,
                        aer_payload,
                        aer_base: 0,
                    });
                }

                let elapsed = step_start.elapsed().as_secs_f32() * 1000.0;
                if net.avg_step_time_ms == 0.0 {
                    net.avg_step_time_ms = elapsed;
                } else {
                    net.avg_step_time_ms = 0.9 * net.avg_step_time_ms + 0.1 * elapsed;
                }

                if let Some(binding) = net.workspace_binding.as_ref() {
                    let autosave_steps = binding.autosave_steps.max(1) as usize;
                    if autosave_steps == 1 || net.runner.t % autosave_steps == 0 {
                        match net.runner.export_network_json() {
                            Ok(snapshot_json) => {
                                if let Err(err) =
                                    persist_workspace_snapshot(binding, &snapshot_json)
                                {
                                    nm_err!(
                                        "[warn] Failed to persist workspace '{}' for network {}: {}",
                                        binding.workspace_id,
                                        net.id,
                                        err
                                    );
                                }
                            }
                            Err(err) => {
                                nm_err!(
                                    "[warn] Failed to export workspace snapshot for network {}: {}",
                                    net.id,
                                    err
                                );
                            }
                        }
                    }
                }

                // Auto-adjust AARNN depth down if lagging.
                // Can be disabled to preserve configured bio depth exactly.
                let realtime_ipc = env_flag("NM_REALTIME_IPC").unwrap_or(false);
                let auto_adjust_depth = env_flag("NM_AUTO_AARNN_DEPTH").unwrap_or(!realtime_ipc);
                let target_ms = std::env::var("NM_AARNN_DEPTH_TARGET_STEP_MS")
                    .ok()
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.5)
                    .unwrap_or(10.0);
                let warmup_steps = std::env::var("NM_AARNN_DEPTH_WARMUP_STEPS")
                    .ok()
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(250);

                if auto_adjust_depth && net.runner.t >= warmup_steps {
                    if net.avg_step_time_ms > target_ms && net.runner.net.aarnn_layer_depth > 0 {
                        net.runner.net.aarnn_layer_depth -= 1;
                        nm_log!(
                            "[info] Node {} auto-adjusting AARNN depth down to {} for network {}",
                            node_id,
                            net.runner.net.aarnn_layer_depth,
                            net.id
                        );
                    } else if net.avg_step_time_ms < target_ms * 0.5
                        && net.runner.net.aarnn_layer_depth < net.desired_aarnn_depth as usize
                    {
                        net.runner.net.aarnn_layer_depth += 1;
                    }
                }

                drop(net);
                if !batches.is_empty() {
                    self.send_spike_batches(&net_id, &batches, None).await;
                }
            }
            let sleep_ms = if any_playing { 1 } else { 20 };
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() { break; }
                }
                _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
            }
        }
        nm_log!("[info] Node {} simulation loop stopped", node_id);
    }
}

#[tonic::async_trait]
impl DistributedNeuromorphic for DistributedNode {
    async fn join(&self, request: Request<JoinRequest>) -> Result<Response<JoinResponse>, Status> {
        let remote_addr = request.remote_addr();
        let req = request.into_inner();
        let (display_addr, connect_addr) = normalize_peer_address(&req.address, remote_addr);
        let node_id = req.node_id.clone();

        let mut state = self.state.write().await;
        if !state.is_orchestrator {
            return Err(Status::permission_denied("Not an orchestrator"));
        }

        let node_status = NodeStatus {
            node_id: node_id.clone(),
            address: display_addr.clone(),
            resources: req.resources,
            active_networks: req.network_resources.keys().cloned().collect(),
        };

        state.nodes.insert(node_id.clone(), node_status);
        state.peers.insert(node_id.clone(), connect_addr.clone());

        // Trigger rebalance when new node joins
        drop(state);
        let node_clone = self.clone();
        let node_id_clone = node_id.clone();
        tokio::spawn(async move {
            match connect_peer(&connect_addr).await {
                Ok(client) => {
                    let mut state = node_clone.state.write().await;
                    state.clients.insert(node_id_clone, client);
                }
                Err(e) => {
                    nm_err!(
                        "[warn] Failed to connect to peer {} at {}: {}",
                        node_id_clone,
                        connect_addr,
                        e
                    );
                }
            }
        });
        self.rebalance_networks().await;

        let state = self.state.read().await;
        Ok(Response::new(JoinResponse {
            success: true,
            manager_id: state.node_id.clone(),
            initial_assignments: Vec::new(),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let remote_addr = request.remote_addr();
        let mut state = self.state.write().await;
        let req = request.into_inner();
        let now = std::time::Instant::now();

        state.last_heartbeat.insert(req.node_id.clone(), now);

        let mut commands = Vec::new();
        let mut connect_target: Option<String> = None;
        let mut peer_map = HashMap::new();
        let mut network_peers = HashMap::new();
        let mut needs_rebalance = false;
        if state.is_orchestrator {
            let stale_nodes: Vec<String> = state
                .last_heartbeat
                .iter()
                .filter_map(|(node_id, last)| {
                    (now.duration_since(*last) > PEER_STALE_AFTER).then_some(node_id.clone())
                })
                .collect();
            if !stale_nodes.is_empty() {
                needs_rebalance = true;
                for node_id in stale_nodes {
                    state.last_heartbeat.remove(&node_id);
                    state.nodes.remove(&node_id);
                    state.peers.remove(&node_id);
                    state.clients.remove(&node_id);
                    state.pending_commands.remove(&node_id);
                    state.ga_inflight_by_peer.remove(&node_id);
                    for net in state.network_registry.values_mut() {
                        net.distribution.remove(&node_id);
                    }
                }
            }

            if let Some(node) = state.nodes.get_mut(&req.node_id) {
                node.resources = req.resources;
                node.active_networks = req.network_resources.keys().cloned().collect();

                let (display_addr, connect_addr) =
                    normalize_peer_address(&node.address, remote_addr);
                if display_addr != node.address {
                    node.address = display_addr;
                }
                state
                    .peers
                    .insert(req.node_id.clone(), connect_addr.clone());
                if !state.clients.contains_key(&req.node_id) {
                    connect_target = Some(connect_addr);
                }
            }

            // Update network distribution info with current neuron counts
            for (net_id, net_res) in req.network_resources {
                if let Some(net_status) = state.network_registry.get_mut(&net_id) {
                    if let Some(range) = net_status.distribution.get_mut(&req.node_id) {
                        range.layer_neuron_counts = net_res.layer_neuron_counts;
                    }
                }
            }

            if let Some(pending) = state.pending_commands.get_mut(&req.node_id) {
                commands = std::mem::take(pending);
            }

            for (node_id, addr) in &state.peers {
                let fresh = state
                    .last_heartbeat
                    .get(node_id)
                    .map(|t| now.duration_since(*t) <= PEER_STALE_AFTER)
                    .unwrap_or(false);
                if fresh {
                    peer_map.insert(node_id.clone(), addr.clone());
                }
            }
            for (net_id, net) in &state.network_registry {
                let nodes = net
                    .distribution
                    .keys()
                    .filter(|node_id| peer_map.contains_key(*node_id))
                    .cloned()
                    .collect::<Vec<_>>();
                network_peers.insert(net_id.clone(), proto::NetworkPeerList { node_ids: nodes });
            }
        }

        let node_id = req.node_id.clone();
        if let Some(addr) = connect_target {
            let node_clone = self.clone();
            tokio::spawn(async move {
                match connect_peer(&addr).await {
                    Ok(client) => {
                        let mut state = node_clone.state.write().await;
                        state.clients.insert(node_id, client);
                    }
                    Err(e) => {
                        nm_err!("[warn] Failed to refresh peer client at {}: {}", addr, e);
                    }
                }
            });
        }

        let response = Ok(Response::new(HeartbeatResponse {
            acknowledged: true,
            commands,
            peers: peer_map,
            network_peers,
        }));
        drop(state);
        if needs_rebalance {
            self.rebalance_networks().await;
        }
        response
    }

    type StreamSpikesStream = tokio_stream::wrappers::ReceiverStream<Result<SpikeBatch, Status>>;

    async fn stream_spikes(
        &self,
        request: Request<tonic::Streaming<SpikeBatch>>,
    ) -> Result<Response<Self::StreamSpikesStream>, Status> {
        let remote_addr = request.remote_addr();
        let mut stream = request.into_inner();
        let node = self.clone();

        let (_tx, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            while let Some(batch) = stream.message().await.unwrap_or(None) {
                let exclude_node = {
                    let state_lock = node.state.read().await;
                    if state_lock.is_orchestrator {
                        peer_id_from_remote_addr(&state_lock, remote_addr)
                    } else {
                        None
                    }
                };
                node.handle_incoming_spike_batch(batch, exclude_node).await;
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn update_network(
        &self,
        request: Request<NetworkUpdateRequest>,
    ) -> Result<Response<NetworkUpdateResponse>, Status> {
        let req = request.into_inner();
        let mut state = self.state.write().await;

        if !state.is_orchestrator {
            return Err(Status::permission_denied("Not an orchestrator"));
        }

        let network_id = req.network_id.clone();
        let mut commands_to_send = Vec::new();
        let mut local_control: Option<proto::control_update::Action> = None;
        let mut local_net_arc: Option<Arc<RwLock<ManagedNetwork>>> = None;
        let mut needs_rebalance = false;
        let local_net_arc_candidate = state.networks.get(&network_id).cloned();

        let response = {
            let (network_registry, network_snapshots, pending_commands) = {
                let state = &mut *state;
                (
                    &mut state.network_registry,
                    &mut state.network_snapshots,
                    &mut state.pending_commands,
                )
            };
            if let Some(net_status) = network_registry.get_mut(&network_id) {
                if let Some(update) = req.update {
                    match update {
                        proto::network_update_request::Update::Config(c) => {
                            if !c.config_json.is_empty() {
                                let cfg_str = String::from_utf8_lossy(&c.config_json).to_string();
                                net_status.config_json = cfg_str.clone();
                                if let Ok(snap) =
                                    crate::runner::decode_snapshot_with_profile_backfill(&cfg_str)
                                {
                                    network_snapshots.insert(network_id.clone(), cfg_str);
                                    net_status.num_layers = (snap.net.num_hidden_layers + 1) as u32;
                                    // Snapshot imports should be redistributed across all active nodes.
                                    needs_rebalance = true;
                                } else if let Ok(net_cfg) =
                                    serde_json::from_str::<NetworkConfig>(&net_status.config_json)
                                {
                                    // Keep layer metadata in sync for config-only updates too.
                                    let updated_layers = (net_cfg.num_hidden_layers + 1) as u32;
                                    if updated_layers > 0 && updated_layers != net_status.num_layers
                                    {
                                        net_status.num_layers = updated_layers;
                                        needs_rebalance = true;
                                    }
                                    // Avoid stale snapshot reuse after switching to config-only payloads.
                                    network_snapshots.remove(&network_id);
                                } else {
                                    // Unknown payload shape: clear stale snapshots to avoid replaying old topology.
                                    network_snapshots.remove(&network_id);
                                }
                            }
                            if !c.neuron_model.is_empty() {
                                net_status.neuron_model = c.neuron_model.clone();
                            }
                            if !c.learning_rule.is_empty() {
                                net_status.learning_rule = c.learning_rule.clone();
                            }

                            // Prepare commands for all nodes in the distribution
                            for (node_id, range) in &net_status.distribution {
                                let redundant: Vec<u32> = range.layers.iter().copied().collect();

                                let cmd = NetworkCommand {
                                    r#type: proto::network_command::CommandType::LoadNetwork as i32,
                                    network_id: network_id.clone(),
                                    config_json: c.config_json.clone(),
                                    layers: range.layers.clone(),
                                    redundant_layers: redundant,
                                    desired_aarnn_depth: net_status.desired_aarnn_depth,
                                    neuron_model: c.neuron_model.clone(),
                                    learning_rule: c.learning_rule.clone(),
                                };
                                commands_to_send.push((node_id.clone(), cmd));
                            }
                        }
                        proto::network_update_request::Update::Control(c) => {
                            let action = proto::control_update::Action::try_from(c.action)
                                .map_err(|_| Status::invalid_argument("invalid control action"))?;
                            let cmd_type = command_type_from_action(action);

                            match action {
                                proto::control_update::Action::Start
                                | proto::control_update::Action::Repeat => {
                                    net_status.playing = true;
                                }
                                proto::control_update::Action::Stop
                                | proto::control_update::Action::Reset
                                | proto::control_update::Action::New => {
                                    net_status.playing = false;
                                }
                            }

                            local_control = Some(action);
                            local_net_arc = local_net_arc_candidate.clone();

                            if matches!(action, proto::control_update::Action::New) {
                                let model = NeuronModel::from_str(&net_status.neuron_model)
                                    .unwrap_or(NeuronModel::Aarnn);
                                let learning = Learning::from_str(&net_status.learning_rule)
                                    .unwrap_or(Learning::Aarnn);
                                let (fresh_cfg, fresh_json) = fresh_single_neuron_snapshot(
                                    net_status.desired_aarnn_depth,
                                    model,
                                    learning,
                                )
                                .map_err(|e| {
                                    Status::internal(format!("new network failed: {}", e))
                                })?;
                                net_status.config_json = fresh_json.clone();
                                net_status.num_layers = (fresh_cfg.num_hidden_layers + 1) as u32;
                                if net_status.neuron_model.is_empty() {
                                    net_status.neuron_model = model.to_str().to_string();
                                }
                                if net_status.learning_rule.is_empty() {
                                    net_status.learning_rule = learning.to_str().to_string();
                                }
                                network_snapshots.insert(network_id.clone(), fresh_json);
                                needs_rebalance = true;
                            } else {
                                for (node_id, _range) in &net_status.distribution {
                                    let cmd = NetworkCommand {
                                        r#type: cmd_type as i32,
                                        network_id: network_id.clone(),
                                        config_json: Vec::new(),
                                        layers: Vec::new(),
                                        redundant_layers: Vec::new(),
                                        desired_aarnn_depth: net_status.desired_aarnn_depth,
                                        neuron_model: String::new(),
                                        learning_rule: String::new(),
                                    };
                                    commands_to_send.push((node_id.clone(), cmd));
                                }
                            }
                        }
                        _ => {
                            nm_log!("[warn] Unsupported network update type");
                        }
                    }
                }

                // Apply all pending commands
                for (node_id, cmd) in commands_to_send {
                    enqueue_pending_command(pending_commands, node_id, cmd);
                }

                Ok(Response::new(NetworkUpdateResponse { success: true }))
            } else {
                Err(Status::not_found("Network not found"))
            }
        };
        drop(state);

        if let (Some(net_arc), Some(action)) = (local_net_arc, local_control) {
            let mut net = net_arc.write().await;
            apply_control_to_managed_network(&mut net, action);
        }
        if needs_rebalance {
            self.rebalance_networks().await;
        }

        response
    }

    async fn get_system_status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let state = self.state.read().await;
        Ok(Response::new(StatusResponse {
            nodes: state.nodes.values().cloned().collect(),
            networks: state.network_registry.values().cloned().collect(),
        }))
    }

    async fn run_ga_evaluation(
        &self,
        request: Request<GaEvaluationRequest>,
    ) -> Result<Response<GaEvaluationResponse>, Status> {
        let req = request.into_inner();
        let req_json = req.config_json;
        let config: crate::config::NetworkConfig = serde_json::from_str(&req_json)
            .map_err(|e| Status::invalid_argument(format!("Invalid config JSON: {}", e)))?;

        let sim_time_ms = req.sim_time_ms;
        let seed = req.seed;

        let mut tried_peers: HashSet<String> = HashSet::new();
        let eval_timeout = crate::ga::ga_remote_eval_timeout();
        loop {
            let forward_target: Option<(
                String,
                DistributedNeuromorphicClient<tonic::transport::Channel>,
            )> = {
                let mut state = self.state.write().await;
                if state.is_orchestrator && !state.clients.is_empty() {
                    let mut best: Option<(
                        String,
                        f32,
                        DistributedNeuromorphicClient<tonic::transport::Channel>,
                    )> = None;
                    let mut fallback: Option<(
                        String,
                        f32,
                        DistributedNeuromorphicClient<tonic::transport::Channel>,
                    )> = None;
                    for (peer_id, client) in state.clients.iter() {
                        if tried_peers.contains(peer_id) {
                            continue;
                        }
                        let res = state.nodes.get(peer_id).and_then(|n| n.resources.as_ref());
                        let capacity = res.map(|r| r.capacity_score.max(0.1)).unwrap_or(1.0);
                        let busy = res.map(|r| r.ga_evaluating).unwrap_or(false);
                        let pacing = res.map(|r| r.ga_pacing).unwrap_or(false);
                        let inflight = *state.ga_inflight_by_peer.get(peer_id).unwrap_or(&0);
                        if inflight >= 1 {
                            continue;
                        }
                        let score = capacity / (1.0 + inflight as f32);
                        if !busy
                            && !pacing
                            && best.as_ref().map(|(_, s, _)| score > *s).unwrap_or(true)
                        {
                            best = Some((peer_id.clone(), score, client.clone()));
                        }
                        if fallback
                            .as_ref()
                            .map(|(_, s, _)| score > *s)
                            .unwrap_or(true)
                        {
                            fallback = Some((peer_id.clone(), score, client.clone()));
                        }
                    }

                    let pick = if best.is_none() { fallback } else { best };
                    if let Some((peer_id, _score, client)) = pick {
                        *state
                            .ga_inflight_by_peer
                            .entry(peer_id.clone())
                            .or_insert(0) += 1;
                        Some((peer_id, client))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            let Some((peer_id, mut client)) = forward_target else {
                break;
            };
            let req_fwd = GaEvaluationRequest {
                config_json: req_json.clone(),
                sim_time_ms,
                seed,
            };
            let resp = tokio::time::timeout(eval_timeout, client.run_ga_evaluation(req_fwd)).await;
            {
                let mut state = self.state.write().await;
                if let Some(count) = state.ga_inflight_by_peer.get_mut(&peer_id) {
                    *count = count.saturating_sub(1);
                }
            }
            match resp {
                Ok(Ok(resp)) => return Ok(resp),
                Ok(Err(e)) => {
                    nm_err!("[warn] GA eval forward to {} failed: {}", peer_id, e);
                }
                Err(_) => {
                    nm_err!(
                        "[warn] GA eval forward to {} timed out after {:?}.",
                        peer_id,
                        eval_timeout
                    );
                }
            }

            {
                let mut state = self.state.write().await;
                state.clients.remove(&peer_id);
            }
            tried_peers.insert(peer_id);
        }
        if !tried_peers.is_empty() {
            nm_err!("[warn] GA eval forwarding failed; falling back to local eval.");
        }

        let _permit = crate::ga::acquire_evaluation_permit().await;

        {
            let mut state = self.state.write().await;
            state.ga_evaluating = true;
            state.ga_eval_progress = 0.0;
            state.ga_active_eval_seed = seed;
        }

        // Run simulation in a blocking task to avoid stalling the executor
        let fitness = tokio::task::spawn_blocking(move || {
            crate::ga::GASearch::evaluate_individual(&config, sim_time_ms, seed)
        })
        .await
        .map_err(|e| {
            nm_err!("[error] Simulation task failed: {}", e);
            Status::internal(format!("Simulation task failed: {}", e))
        })?;

        {
            let mut state = self.state.write().await;
            state.ga_evaluating = false;
            state.ga_eval_progress = 1.0;
            state.ga_total_evaluations += 1;
        }

        Ok(Response::new(GaEvaluationResponse { fitness }))
    }

    async fn get_network_snapshot(
        &self,
        request: Request<NetworkSnapshotRequest>,
    ) -> Result<Response<NetworkSnapshotResponse>, Status> {
        let req = request.into_inner();
        let net_id = req.network_id.clone();
        let net_arc = {
            let state = self.state.read().await;
            state.networks.get(&req.network_id).cloned()
        };

        let Some(net_arc) = net_arc else {
            return Err(Status::not_found("network not hosted on this node"));
        };

        let snapshot_json = tokio::task::spawn_blocking(move || {
            let net = net_arc.blocking_read();
            net.runner.export_network_json()
        })
        .await
        .map_err(|e| Status::internal(format!("snapshot task failed: {}", e)))?
        .map_err(|e| Status::internal(format!("snapshot export failed: {}", e)))?;

        Ok(Response::new(NetworkSnapshotResponse {
            network_id: net_id,
            snapshot_json,
        }))
    }

    async fn get_network_activity(
        &self,
        request: Request<NetworkActivityRequest>,
    ) -> Result<Response<NetworkActivityResponse>, Status> {
        let req = request.into_inner();
        let net_arc = {
            let state = self.state.read().await;
            state.networks.get(&req.network_id).cloned()
        };

        let Some(net_arc) = net_arc else {
            return Err(Status::not_found("network not hosted on this node"));
        };

        let (hidden, output) = tokio::task::spawn_blocking(move || {
            let net = net_arc.blocking_read();
            let ts_us = (net.runner.t_ms * 1000.0) as u64;
            let hidden = net
                .runner
                .last_spk_h
                .iter()
                .map(|layer| {
                    let layer_vec: Vec<i8> = layer.iter().copied().collect();
                    let exchange = encode_exchange(ts_us, 0, &layer_vec);
                    SpikeIndices {
                        indices: exchange.spike_indices,
                        aer_payload: exchange.aer_payload,
                        aer_base: exchange.aer_base,
                    }
                })
                .collect::<Vec<_>>();
            let output_vec: Vec<i8> = net.runner.last_spk_o.iter().copied().collect();
            let exchange = encode_exchange(ts_us, 0, &output_vec);
            let output = SpikeIndices {
                indices: exchange.spike_indices,
                aer_payload: exchange.aer_payload,
                aer_base: exchange.aer_base,
            };
            (hidden, output)
        })
        .await
        .map_err(|e| Status::internal(format!("activity task failed: {}", e)))?;

        Ok(Response::new(NetworkActivityResponse {
            network_id: req.network_id,
            sensory: Some(SpikeIndices {
                indices: Vec::new(),
                aer_payload: Vec::new(),
                aer_base: 0,
            }),
            hidden,
            output: Some(output),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spike_transport_defaults_to_persistent_when_available() {
        let node = DistributedNode::new("test-node".to_string(), false);
        let state = node.state.read().await;
        assert_eq!(
            state.choose_spike_transport("peer-a", true, false),
            SpikeTransportMethod::PersistentStream
        );
        assert_eq!(
            state.choose_spike_transport("peer-a", false, false),
            SpikeTransportMethod::BurstStream
        );
    }

    #[tokio::test]
    async fn spike_transport_fails_over_after_stream_failures() {
        let node = DistributedNode::new("test-node".to_string(), false);
        let mut state = node.state.write().await;
        for _ in 0..SPIKE_FAILOVER_STREAK {
            state.record_spike_transport_failure("peer-b", SpikeTransportMethod::PersistentStream);
        }
        assert_eq!(
            state.choose_spike_transport("peer-b", true, false),
            SpikeTransportMethod::BurstStream
        );
    }

    #[tokio::test]
    async fn spike_transport_prefers_lower_latency_path() {
        let node = DistributedNode::new("test-node".to_string(), false);
        let mut state = node.state.write().await;
        state.record_spike_transport_success(
            "peer-c",
            SpikeTransportMethod::PersistentStream,
            Duration::from_micros(800),
        );
        state.record_spike_transport_success(
            "peer-c",
            SpikeTransportMethod::BurstStream,
            Duration::from_micros(200),
        );
        assert_eq!(
            state.choose_spike_transport("peer-c", true, false),
            SpikeTransportMethod::BurstStream
        );
    }

    #[tokio::test]
    async fn spike_transport_prefers_mpi_when_lowest_latency() {
        let node = DistributedNode::new("test-node".to_string(), false);
        let mut state = node.state.write().await;
        state.record_spike_transport_success(
            "peer-d",
            SpikeTransportMethod::PersistentStream,
            Duration::from_micros(500),
        );
        state.record_spike_transport_success(
            "peer-d",
            SpikeTransportMethod::BurstStream,
            Duration::from_micros(300),
        );
        state.record_spike_transport_success(
            "peer-d",
            SpikeTransportMethod::Mpi,
            Duration::from_micros(120),
        );
        assert_eq!(
            state.choose_spike_transport("peer-d", true, true),
            SpikeTransportMethod::Mpi
        );
    }
}
