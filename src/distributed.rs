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
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
#[cfg(feature = "sysinfo")]
use sysinfo::{Components, CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::{RwLock, mpsc};
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
use crate::cluster_snapshot::{self, ShardSnapshotInput};
use crate::config::{LIFParams, NetworkConfig, STDPParams};
use crate::consistent_cut::{
    AsyncConsistentCutCollector, ChannelMarker, ConsistentCutCoordinator, ConsistentCutMessage,
    ParticipantReport,
};
use crate::deployment::{DeploymentConfig, ExecutionMode};
#[cfg(feature = "replicated_durability")]
use crate::deterministic::LeaseTerm;
use crate::deterministic::LogicalTag;
pub(crate) use crate::node_auth::{
    authenticated_request, certificate_sha256_der,
    configured_node_cert_fingerprints as configured_causal_node_cert_fingerprints,
    configured_node_tokens as configured_causal_node_tokens, live_causal_transport_enabled,
    validate_live_request, validate_peer_metadata as validate_causal_peer_metadata,
};
use crate::runner::Runner;
use crate::sim::{Learning, NeuronModel};
use crate::spike_io::transport::{encode_exchange, spikes_from_transport};
use crate::stable_worker::{
    StableExecutorCapability as StableExecutorCapabilityModel, StableShardApplicationAck,
    StableWorkerActivationCommand, StableWorkerRegistration,
};
#[cfg(feature = "superdense_executor")]
use crate::superdense::SuperdenseController;
use anyhow::Context;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
/// Default timeout budget for burst-mode spike forwarding fallback.
const DEFAULT_SPIKE_BURST_TIMEOUT_MS: u64 = 120;
/// Timeout budget for short-lived gRPC connections used by burst forwarding.
const SPIKE_BURST_CONNECT_TIMEOUT: Duration = Duration::from_millis(80);
/// EWMA smoothing for per-peer transport latency tracking.
const SPIKE_LATENCY_EWMA_ALPHA: f64 = 0.2;
/// Consecutive failures before preferring the alternate transport method.
const SPIKE_FAILOVER_STREAK: u32 = 3;
/// Cap queued control/config commands per node so heartbeat payloads remain bounded.
const MAX_PENDING_COMMANDS_PER_NODE: usize = 64;
/// Bound command-result error text so a failed worker cannot inflate a
/// heartbeat indefinitely. The result is diagnostic data, never executable
/// input.
const MAX_COMMAND_RESULT_ERROR_BYTES: usize = 2048;
/// Treat configs above this as "large" and avoid broadcasting to all nodes without affinity.
const LARGE_NETWORK_CONFIG_BYTES: usize = 64 * 1024 * 1024;

fn stable_registration_to_proto(
    registration: StableWorkerRegistration,
) -> StableExecutorRegistration {
    let application_acks = registration
        .application_acks
        .into_iter()
        .map(stable_application_ack_to_proto)
        .collect();
    StableExecutorRegistration {
        schema_version: registration.schema_version,
        profile: registration.profile,
        network_id: registration.network_id,
        brain_id: registration.brain_id,
        topology_generation: registration.topology_generation,
        partition_generation: registration.partition_generation,
        topology_digest: registration.topology_digest,
        plan_digest: registration.plan_digest,
        shard_ids: registration.shard_ids,
        owned_shard_ids: registration.owned_shard_ids,
        application_acks,
        lease_term: registration.lease_term,
        fencing_token: registration.fencing_token,
        current_tick: registration.current_tick,
        current_microstep: registration.current_microstep,
        state_digest: registration.state_digest,
        max_input_events: registration.max_input_events,
        max_steps_per_poll: registration.max_steps_per_poll,
        authoritative: registration.authoritative,
    }
}

/// Extract the immutable identity carried by an activation command. The
/// manifest itself remains opaque here; the target worker is responsible for
/// opening and validating it. The orchestrator only needs this identity to
/// make heartbeat delivery at-least-once and acknowledgement idempotent.
fn stable_activation_command_identity(
    command: &NetworkCommand,
) -> Option<(String, String, String, String)> {
    if command.r#type != proto::network_command::CommandType::ActivateStableWorker as i32 {
        return None;
    }
    let activation = serde_json::from_slice::<crate::stable_worker::StableWorkerActivationCommand>(
        &command.config_json,
    )
    .ok()?;
    Some((
        activation.network_id,
        activation.request_id,
        activation.manifest_digest,
        activation.placement_idempotency_key,
    ))
}

fn validate_command_result(result: &proto::NetworkCommandResult) -> Result<(), Status> {
    if result.command_type != proto::network_command::CommandType::ActivateStableWorker as i32 {
        return Err(Status::invalid_argument(
            "unsupported network command result type",
        ));
    }
    for (field, value) in [
        ("network_id", result.network_id.as_str()),
        ("request_id", result.request_id.as_str()),
        ("manifest_digest", result.manifest_digest.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 256 {
            return Err(Status::invalid_argument(format!(
                "network command result {field} is invalid"
            )));
        }
    }
    if result.placement_idempotency_key.len() > 256 {
        return Err(Status::invalid_argument(
            "network command result placement key is too large",
        ));
    }
    if result.brain_id == 0 {
        return Err(Status::invalid_argument(
            "network command result brain identity is invalid",
        ));
    }
    if result.manifest_digest.len() != 64
        || !result
            .manifest_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Status::invalid_argument(
            "network command result manifest digest is invalid",
        ));
    }
    if result.error.len() > MAX_COMMAND_RESULT_ERROR_BYTES {
        return Err(Status::invalid_argument(
            "network command result error is too large",
        ));
    }
    if result.accepted && !result.error.is_empty() {
        return Err(Status::invalid_argument(
            "accepted network command result cannot contain an error",
        ));
    }
    if !result.accepted && result.error.trim().is_empty() {
        return Err(Status::invalid_argument(
            "rejected network command result must contain an error",
        ));
    }
    Ok(())
}

fn stable_application_ack_to_proto(
    ack: StableShardApplicationAck,
) -> proto::StableShardApplicationAck {
    proto::StableShardApplicationAck {
        shard_id: ack.shard_id,
        brain_id: ack.brain_id,
        topology_generation: ack.topology_generation,
        partition_generation: ack.partition_generation,
        plan_digest: ack.plan_digest,
        lease_term: ack.lease_term,
        fencing_token: ack.fencing_token,
        applied_tick: ack.applied_tick,
        applied_microstep: ack.applied_microstep,
        state_digest: ack.state_digest,
        durable_wal_sequence: ack.durable_wal_sequence.unwrap_or(0),
        durable_wal_sequence_present: ack.durable_wal_sequence.is_some(),
        committed: ack.committed,
    }
}

fn stable_application_ack_from_proto(
    ack: &proto::StableShardApplicationAck,
) -> StableShardApplicationAck {
    StableShardApplicationAck {
        shard_id: ack.shard_id,
        brain_id: ack.brain_id,
        topology_generation: ack.topology_generation,
        partition_generation: ack.partition_generation,
        plan_digest: ack.plan_digest.clone(),
        lease_term: ack.lease_term,
        fencing_token: ack.fencing_token,
        applied_tick: ack.applied_tick,
        applied_microstep: ack.applied_microstep,
        state_digest: ack.state_digest.clone(),
        durable_wal_sequence: ack
            .durable_wal_sequence_present
            .then_some(ack.durable_wal_sequence),
        committed: ack.committed,
    }
}

fn stable_registration_from_proto(
    registration: &StableExecutorRegistration,
) -> StableWorkerRegistration {
    StableWorkerRegistration {
        schema_version: registration.schema_version,
        profile: registration.profile.clone(),
        network_id: registration.network_id.clone(),
        brain_id: registration.brain_id,
        topology_generation: registration.topology_generation,
        partition_generation: registration.partition_generation,
        topology_digest: registration.topology_digest.clone(),
        plan_digest: registration.plan_digest.clone(),
        shard_ids: registration.shard_ids.clone(),
        owned_shard_ids: registration.owned_shard_ids.clone(),
        application_acks: registration
            .application_acks
            .iter()
            .map(stable_application_ack_from_proto)
            .collect(),
        lease_term: registration.lease_term,
        fencing_token: registration.fencing_token,
        current_tick: registration.current_tick,
        current_microstep: registration.current_microstep,
        state_digest: registration.state_digest.clone(),
        max_input_events: registration.max_input_events,
        max_steps_per_poll: registration.max_steps_per_poll,
        authoritative: registration.authoritative,
    }
}

fn validate_stable_registration_shape(
    registration: &StableExecutorRegistration,
) -> Result<StableWorkerRegistration, Status> {
    let registration = stable_registration_from_proto(registration);
    registration.validate().map_err(|error| {
        Status::invalid_argument(format!("invalid stable executor registration: {error}"))
    })?;
    Ok(registration)
}

fn validate_stable_registration_admission(
    state: &NodeState,
    node_id: &str,
    network_resources: &HashMap<String, NetworkResources>,
    registrations: &[StableExecutorRegistration],
) -> Result<Vec<StableWorkerRegistration>, Status> {
    let mut validated = Vec::with_capacity(registrations.len());
    let mut network_ids = HashSet::new();
    for wire_registration in registrations {
        let registration = validate_stable_registration_shape(wire_registration)?;
        if !network_resources.contains_key(&registration.network_id) {
            return Err(Status::invalid_argument(format!(
                "stable executor network '{}' is absent from network_resources",
                registration.network_id
            )));
        }
        if registration.brain_id
            != crate::managed_durability::managed_brain_id(&registration.network_id).raw()
        {
            return Err(Status::failed_precondition(format!(
                "stable executor network '{}' reports a brain identity inconsistent with its network",
                registration.network_id
            )));
        }
        if !network_ids.insert(registration.network_id.clone()) {
            return Err(Status::invalid_argument(format!(
                "stable executor network '{}' is registered more than once",
                registration.network_id
            )));
        }

        if let Some(network) = state.network_registry.get(&registration.network_id) {
            if !network.distribution.is_empty()
                && state
                    .nodes
                    .get(node_id)
                    .and_then(|node| {
                        node.stable_executors
                            .iter()
                            .find(|existing| existing.network_id == registration.network_id)
                    })
                    .is_none()
            {
                return Err(Status::failed_precondition(format!(
                    "stable executor network '{}' cannot register while legacy placement exists",
                    registration.network_id
                )));
            }
        }

        for (other_node_id, other_node) in &state.nodes {
            if other_node_id == node_id {
                continue;
            }
            for existing in other_node
                .stable_executors
                .iter()
                .filter(|existing| existing.network_id == registration.network_id)
            {
                let existing = stable_registration_from_proto(existing);
                if !registration.same_plan_identity(&existing) {
                    return Err(Status::failed_precondition(format!(
                        "stable executor network '{}' has incompatible plan identity on node {}",
                        registration.network_id, other_node_id
                    )));
                }
                let ownership_overlaps = registration
                    .owned_shard_ids
                    .iter()
                    .any(|shard| existing.owned_shard_ids.binary_search(shard).is_ok());
                if ownership_overlaps
                    && (registration.lease_term <= existing.lease_term
                        || registration.fencing_token <= existing.fencing_token)
                {
                    return Err(Status::failed_precondition(format!(
                        "stable executor network '{}' has overlapping shard ownership without a newer fenced boundary",
                        registration.network_id
                    )));
                }
            }
        }

        if let Some(existing) = state.nodes.get(node_id).and_then(|node| {
            node.stable_executors
                .iter()
                .find(|existing| existing.network_id == registration.network_id)
        }) {
            let existing = stable_registration_from_proto(existing);
            registration
                .validate_update_from(&existing)
                .map_err(|error| {
                    Status::failed_precondition(format!(
                        "stable executor network '{}' registration update rejected: {error}",
                        registration.network_id
                    ))
                })?;
        }

        let has_pending_activation = state
            .pending_commands
            .get(node_id)
            .into_iter()
            .flatten()
            .filter_map(|command| {
                serde_json::from_slice::<StableWorkerActivationCommand>(&command.config_json).ok()
            })
            .any(|command| {
                command.network_id == registration.network_id
                    && command.brain_id == registration.brain_id
                    && command.target_node == node_id
            });
        if state.stable_network_ids.contains(&registration.network_id)
            && !has_pending_activation
            && state
                .nodes
                .get(node_id)
                .and_then(|node| {
                    node.stable_executors
                        .iter()
                        .find(|existing| existing.network_id == registration.network_id)
                })
                .is_none()
        {
            return Err(Status::failed_precondition(format!(
                "stable executor network '{}' is fenced until an explicit migration transaction reopens it",
                registration.network_id
            )));
        }
        validated.push(registration);
    }
    Ok(validated)
}

fn stable_capability_to_proto(
    capability: StableExecutorCapabilityModel,
) -> proto::StableExecutorCapability {
    proto::StableExecutorCapability {
        schema_version: capability.schema_version,
        profile: capability.profile,
        activation_schema_version: capability.activation_schema_version,
        max_input_events: capability.max_input_events,
        max_steps_per_poll: capability.max_steps_per_poll,
    }
}

fn validate_stable_capability_admission(
    capabilities: &[proto::StableExecutorCapability],
) -> Result<Vec<StableExecutorCapabilityModel>, Status> {
    let mut validated = Vec::with_capacity(capabilities.len());
    let mut profiles = HashSet::new();
    for wire in capabilities {
        let capability = StableExecutorCapabilityModel {
            schema_version: wire.schema_version,
            profile: wire.profile.clone(),
            activation_schema_version: wire.activation_schema_version,
            max_input_events: wire.max_input_events,
            max_steps_per_poll: wire.max_steps_per_poll,
        };
        capability.validate().map_err(|error| {
            Status::invalid_argument(format!("invalid stable executor capability: {error}"))
        })?;
        if !profiles.insert(capability.profile.clone()) {
            return Err(Status::invalid_argument(
                "stable executor capability profile is advertised more than once",
            ));
        }
        validated.push(capability);
    }
    Ok(validated)
}
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

#[cfg(feature = "replicated_durability")]
use crate::causal_transport::proto::causal_data_plane_client::CausalDataPlaneClient;

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
    let mut file = std::fs::File::create(&tmp_path)
        .with_context(|| format!("failed to create '{}'", tmp_path.display()))?;
    std::io::Write::write_all(&mut file, payload)
        .with_context(|| format!("failed to write '{}'", tmp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to flush '{}'", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename '{}' to '{}'",
            tmp_path.display(),
            path.display()
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)
            .with_context(|| format!("failed to open parent '{}'", parent.display()))?
            .sync_all()
            .with_context(|| format!("failed to flush parent '{}'", parent.display()))?;
    }
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
            #[cfg(feature = "superdense_executor")]
            net.superdense.reset();
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
            #[cfg(feature = "superdense_executor")]
            net.superdense.reset();
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
            #[cfg(feature = "superdense_executor")]
            net.superdense.reset();
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

/// Normalize and de-duplicate a sequence of orchestrator endpoints while
/// preserving preference order. Values may be comma, semicolon, or
/// whitespace separated so they can be supplied conveniently through env.
pub fn merge_orchestrator_endpoints<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut endpoints = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        for raw in value.as_ref().split([',', ';', ' ', '\t', '\n']) {
            let raw = raw.trim().trim_end_matches('/');
            if raw.is_empty() {
                continue;
            }
            let endpoint = if raw.starts_with("http://") || raw.starts_with("https://") {
                raw.to_string()
            } else {
                format!("http://{raw}")
            };
            if seen.insert(endpoint.clone()) {
                endpoints.push(endpoint);
            }
        }
    }
    endpoints
}

fn discovery_target_tokens(configured: Option<&str>) -> Vec<String> {
    let configured = configured.unwrap_or_default();
    let mut targets = vec![
        "255.255.255.255:50050".to_string(),
        "127.0.0.1:50050".to_string(),
    ];
    let mut seen = targets.iter().cloned().collect::<HashSet<_>>();
    for raw in configured.split([',', ';', ' ', '\t', '\n']) {
        let raw = raw
            .trim()
            .trim_start_matches("udp://")
            .trim_end_matches('/');
        if raw.is_empty() {
            continue;
        }
        let target = if split_host_port(raw).is_some() {
            raw.to_string()
        } else {
            format!("{raw}:50050")
        };
        if seen.insert(target.clone()) {
            targets.push(target);
        }
    }
    targets
}

async fn resolve_discovery_targets() -> Vec<SocketAddr> {
    let configured = std::env::var("NM_DISCOVERY_TARGETS").ok();
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();
    for target in discovery_target_tokens(configured.as_deref()) {
        match tokio::net::lookup_host(target.as_str()).await {
            Ok(addrs) => {
                for addr in addrs {
                    if seen.insert(addr) {
                        resolved.push(addr);
                    }
                }
            }
            Err(err) => nm_err!(
                "[warn] Ignoring invalid discovery target '{}': {}",
                target,
                err
            ),
        }
    }
    resolved
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
    let endpoint = crate::management::grpc_client_endpoint(&target)?
        .connect_timeout(timeout_budget)
        .timeout(timeout_budget);
    match tokio::time::timeout(
        timeout_budget,
        DistributedNeuromorphicClient::connect(endpoint),
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

/// Production cutover is a deployment gate, not an alias for enabling the
/// migration features.  Keep this check in the distributed startup path so a
/// binary cannot silently run the legacy SpikeBatch/Runner path while an
/// operator believes production mode is active.
pub fn validate_production_cutover_config(
    node_id: &str,
    is_orchestrator: bool,
) -> Result<(), String> {
    if !crate::management::production_cutover_enabled() {
        return Ok(());
    }
    if node_id.trim().is_empty() {
        return Err("NM_PRODUCTION_CUTOVER requires a stable node identity".to_owned());
    }
    if !live_causal_transport_enabled() {
        return Err(
            "NM_PRODUCTION_CUTOVER requires NM_CAUSAL_TRANSPORT_LIVE=1 on every node".to_owned(),
        );
    }
    if !cfg!(feature = "management_v1") {
        return Err("NM_PRODUCTION_CUTOVER requires the management_v1 feature".to_owned());
    }
    validate_live_causal_transport_config(node_id)?;
    if is_orchestrator {
        crate::management::validate_production_management_config()?;
        validate_cluster_snapshot_root(std::env::var("NM_CLUSTER_SNAPSHOT_ROOT").ok().as_deref())?;
    }
    Ok(())
}

fn validate_cluster_snapshot_root(root: Option<&str>) -> Result<(), String> {
    let root = root.ok_or_else(|| {
        "NM_PRODUCTION_CUTOVER requires NM_CLUSTER_SNAPSHOT_ROOT for durable cluster cuts"
            .to_owned()
    })?;
    if root.trim().is_empty() {
        return Err("NM_CLUSTER_SNAPSHOT_ROOT must not be empty".to_owned());
    }
    Ok(())
}

/// Validate the deployment contract required by the live authoritative causal
/// path.  The normal reference profile may use plaintext and omitted node
/// credentials; an enabled live path may not.  `NM_CAUSAL_NODE_TOKENS` is a
/// temporary deployment credential bridge until node identity is supplied by
/// the production workload-mTLS provider.  It is intentionally per-node,
/// rather than one shared gateway secret.
pub fn validate_live_causal_transport_config(node_id: &str) -> Result<(), String> {
    if !live_causal_transport_enabled() {
        return Ok(());
    }
    if !cfg!(feature = "replicated_durability") {
        return Err(
            "NM_CAUSAL_TRANSPORT_LIVE requires the replicated_durability feature".to_owned(),
        );
    }
    if crate::managed_durability::configured_root().is_none() {
        return Err(
            "live causal transport requires NM_DURABLE_SHARD_ROOT for every managed network"
                .to_owned(),
        );
    }
    let warm_root = crate::managed_durability::configured_warm_root().ok_or_else(|| {
        "live causal transport requires NM_WARM_REPLICA_ROOT for warm recovery".to_owned()
    })?;
    let durable_root = crate::managed_durability::configured_root().expect("checked above");
    if durable_root == warm_root {
        return Err(
            "NM_DURABLE_SHARD_ROOT and NM_WARM_REPLICA_ROOT must be distinct failure domains"
                .to_owned(),
        );
    }
    let (replicas, members) = crate::managed_durability::configured_replicated_authority()?
        .ok_or_else(|| {
            "live causal transport requires an explicit replicated authority; the single-file authority is reference-only".to_owned()
        })?;
    if members.len() < 3 {
        return Err("live causal transport requires at least three authority members".to_owned());
    }
    let mut replica_paths = std::collections::BTreeSet::new();
    for (_, path) in replicas {
        if !replica_paths.insert(path.clone()) {
            return Err("authority replica paths must be distinct".to_owned());
        }
    }
    crate::management::configured_grpc_server_tls()?
        .ok_or_else(|| "live causal transport requires mutual TLS configuration".to_owned())?;
    let local_token = std::env::var("NM_CAUSAL_NODE_TOKEN")
        .map_err(|_| "live causal transport requires NM_CAUSAL_NODE_TOKEN".to_owned())?;
    if local_token.trim().is_empty() {
        return Err("NM_CAUSAL_NODE_TOKEN must not be empty".to_owned());
    }
    let tokens = configured_causal_node_tokens()?;
    if tokens
        .get(node_id)
        .is_none_or(|expected| expected != &local_token)
    {
        return Err(format!(
            "NM_CAUSAL_NODE_TOKENS must contain the configured token for node {node_id}"
        ));
    }
    let fingerprints = configured_causal_node_cert_fingerprints()?;
    if !fingerprints.contains_key(node_id) {
        return Err(format!(
            "NM_CAUSAL_NODE_CERT_SHA256 must contain the configured node {node_id}"
        ));
    }
    Ok(())
}

fn unix_timestamp_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
struct DeploymentTransitionRecord {
    observed_at: std::time::Instant,
    ts_ms: u64,
    reason: String,
    source: String,
}

fn sync_network_status_transition(
    status: &mut proto::NetworkStatus,
    transition: Option<&DeploymentTransitionRecord>,
) {
    if let Some(transition) = transition {
        status.last_transition_reason = transition.reason.clone();
        status.last_transition_ts_ms = transition.ts_ms;
        status.last_transition_source = transition.source.clone();
    } else {
        status.last_transition_reason.clear();
        status.last_transition_ts_ms = 0;
        status.last_transition_source.clear();
    }
}

pub(crate) fn sync_network_status_deployment(
    status: &mut proto::NetworkStatus,
    deployment: &DeploymentConfig,
) {
    status.deployment_modes = deployment
        .modes
        .iter()
        .map(|mode| mode.as_str().to_string())
        .collect();
    status.deployment_scope = deployment.scope.as_str().to_string();
    status.live_transition_allowed = deployment.allows_live_transition();
    status.autonomous_transition_enabled = deployment.allows_autonomous_transition();
}

fn deployment_modes_label(deployment: &DeploymentConfig) -> String {
    if deployment.modes.is_empty() {
        "auto".to_string()
    } else {
        deployment
            .modes
            .iter()
            .map(|mode| mode.as_str())
            .collect::<Vec<_>>()
            .join("+")
    }
}

#[allow(dead_code)]
pub(crate) fn sync_network_status_deployment_from_payload(
    status: &mut proto::NetworkStatus,
    payload: &str,
) {
    sync_network_status_deployment_from_payload_with_transition(status, payload, None);
}

fn sync_network_status_deployment_from_payload_with_transition(
    status: &mut proto::NetworkStatus,
    payload: &str,
    transition: Option<&DeploymentTransitionRecord>,
) {
    let deployment = network_deployment_from_payload(payload).unwrap_or_default();
    sync_network_status_deployment(status, &deployment);
    sync_network_status_transition(status, transition);
}

fn network_deployment_from_payload(payload: &str) -> Option<DeploymentConfig> {
    if payload.trim().is_empty() {
        return None;
    }
    if let Ok(snapshot) = crate::runner::decode_snapshot_with_profile_backfill(payload) {
        let mut deployment = snapshot.net.deployment;
        deployment.normalize();
        return Some(deployment);
    }
    serde_json::from_str::<NetworkConfig>(payload)
        .ok()
        .map(|cfg| {
            let mut deployment = cfg.deployment;
            deployment.normalize();
            deployment
        })
}

fn payload_with_updated_deployment(payload: &str, deployment: &DeploymentConfig) -> Option<String> {
    if payload.trim().is_empty() {
        return None;
    }
    if let Ok(mut snapshot) = crate::runner::decode_snapshot_with_profile_backfill(payload) {
        snapshot.net.deployment = deployment.clone();
        return serde_json::to_string(&snapshot).ok();
    }
    if let Ok(mut cfg) = serde_json::from_str::<NetworkConfig>(payload) {
        cfg.deployment = deployment.clone();
        return serde_json::to_string(&cfg).ok();
    }
    None
}

fn network_config_from_config_payload(payload: &str) -> Option<NetworkConfig> {
    if payload.trim().is_empty() {
        return None;
    }
    if crate::runner::decode_snapshot_with_profile_backfill(payload).is_ok() {
        return None;
    }
    serde_json::from_str::<NetworkConfig>(payload).ok()
}

fn network_config_from_payload(payload: &str) -> Option<NetworkConfig> {
    if payload.trim().is_empty() {
        return None;
    }
    if let Ok(snapshot) = crate::runner::decode_snapshot_with_profile_backfill(payload) {
        return Some(snapshot.net);
    }
    serde_json::from_str::<NetworkConfig>(payload).ok()
}

fn network_config_shape_compatible(
    current_cfg: &NetworkConfig,
    requested_cfg: &NetworkConfig,
) -> bool {
    current_cfg.num_sensory_neurons == requested_cfg.num_sensory_neurons
        && current_cfg.num_hidden_layers == requested_cfg.num_hidden_layers
        && current_cfg.num_hidden_per_layer_initial == requested_cfg.num_hidden_per_layer_initial
        && current_cfg.num_output_neurons == requested_cfg.num_output_neurons
}

fn total_neurons_from_distribution(distribution: &HashMap<String, LayerRange>) -> u64 {
    let mut total = 0u64;
    let mut seen_layers = HashSet::new();
    for range in distribution.values() {
        for (&layer, &count) in &range.layer_neuron_counts {
            if seen_layers.insert(layer) {
                total = total.saturating_add(count);
            }
        }
    }
    total
}

/// Build bounded, deterministic placement telemetry from two immutable
/// placement projections.
///
/// The legacy layer assignment path does not yet expose a live transfer
/// acknowledgement. Consequently, `moving` means that the orchestrator has
/// queued a different owner/replica assignment, while `considering` means the
/// autonomous planner is evaluating the current assignment. The records are
/// intentionally advisory and must never be interpreted as a lease, fencing
/// token, cutover receipt or proof that a shard has become authoritative.
fn build_shard_placement_movements(
    network_id: &str,
    previous: &HashMap<String, LayerRange>,
    next: &HashMap<String, LayerRange>,
    automation_enabled: bool,
    reason: &str,
    updated_at_ms: u64,
) -> Vec<proto::ShardPlacementMovement> {
    const MAX_MOVEMENT_RECORDS: usize = 128;

    fn owners_by_layer(
        distribution: &HashMap<String, LayerRange>,
        backup: bool,
    ) -> BTreeMap<u32, Vec<String>> {
        let mut owners = BTreeMap::<u32, Vec<String>>::new();
        let mut node_ids: Vec<&String> = distribution.keys().collect();
        node_ids.sort();
        for node_id in node_ids {
            let Some(range) = distribution.get(node_id) else {
                continue;
            };
            let layers = if backup {
                &range.backup_layers
            } else {
                &range.layers
            };
            for layer in layers {
                owners.entry(*layer).or_default().push(node_id.clone());
            }
        }
        owners
    }

    let old_active = owners_by_layer(previous, false);
    let new_active = owners_by_layer(next, false);
    let old_backup = owners_by_layer(previous, true);
    let new_backup = owners_by_layer(next, true);
    let mut records = Vec::new();

    for (role, old, new) in [
        ("active", &old_active, &new_active),
        ("backup", &old_backup, &new_backup),
    ] {
        let mut layers = BTreeMap::<u32, ()>::new();
        old.keys().chain(new.keys()).for_each(|layer| {
            layers.insert(*layer, ());
        });

        for layer in layers.keys().copied() {
            let old_owners = old.get(&layer).cloned().unwrap_or_default();
            let new_owners = new.get(&layer).cloned().unwrap_or_default();
            let width = old_owners.len().max(new_owners.len());
            for replica in 0..width {
                let source = old_owners
                    .get(replica)
                    .cloned()
                    .unwrap_or_else(|| "unassigned".to_string());
                let destination = new_owners
                    .get(replica)
                    .cloned()
                    .unwrap_or_else(|| "unassigned".to_string());
                let changed = source != destination;
                if !changed && !automation_enabled {
                    continue;
                }
                if !changed && (source == "unassigned" || destination == "unassigned") {
                    continue;
                }

                let phase = if changed { "moving" } else { "considering" };
                let reported_destination = if changed {
                    destination.clone()
                } else {
                    String::new()
                };
                let movement_reason = if reason.trim().is_empty() {
                    if changed {
                        "placement assignment changed"
                    } else {
                        "autonomous placement review"
                    }
                } else {
                    reason
                };
                records.push(proto::ShardPlacementMovement {
                    shard_id: format!(
                        "{}:{}:layer-{}:replica-{}",
                        network_id, role, layer, replica
                    ),
                    source_node: source,
                    destination_node: reported_destination,
                    role: role.to_string(),
                    phase: phase.to_string(),
                    progress_milli: 0,
                    reason: movement_reason.to_string(),
                    updated_at_ms,
                });
                if records.len() >= MAX_MOVEMENT_RECORDS {
                    return records;
                }
            }
        }
    }
    records
}

fn snapshot_with_network_config(snapshot_payload: &str, net_cfg: &NetworkConfig) -> Option<String> {
    let mut snapshot =
        crate::runner::decode_snapshot_with_profile_backfill(snapshot_payload).ok()?;
    snapshot.net = net_cfg.clone();
    serde_json::to_string(&snapshot).ok()
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ManagedChannelState {
    remote_spikes_fwd: std::collections::BTreeMap<u32, Vec<i8>>,
    remote_spikes_bwd: std::collections::BTreeMap<u32, Vec<i8>>,
    remote_spike_steps_fwd: std::collections::BTreeMap<u32, i64>,
    remote_spike_steps_bwd: std::collections::BTreeMap<u32, i64>,
    external_sensory_spikes: Option<Vec<i8>>,
}

/// Versioned payload carried by an authoritative causal envelope for a
/// cross-process layer input.  Network identity is repeated in the payload so
/// a receiver cannot route a valid envelope to a different local network.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CausalSpikeIngress {
    schema_version: u32,
    network_id: String,
    layer_index: u32,
    step_index: i64,
    is_backward: bool,
    spike_indices: Vec<u32>,
    aer_payload: Vec<u8>,
    aer_base: u32,
}

#[cfg(feature = "replicated_durability")]
const CAUSAL_INGRESS_SCHEMA_VERSION: u32 = 1;
#[cfg(feature = "replicated_durability")]
const MAX_CAUSAL_INGRESS_SPIKES: usize = 16 * 1024 * 1024;

fn capture_channel_state(net: &ManagedNetwork) -> ManagedChannelState {
    ManagedChannelState {
        remote_spikes_fwd: net
            .remote_spikes_fwd
            .iter()
            .map(|(key, value)| (*key, value.clone()))
            .collect(),
        remote_spikes_bwd: net
            .remote_spikes_bwd
            .iter()
            .map(|(key, value)| (*key, value.clone()))
            .collect(),
        remote_spike_steps_fwd: net
            .remote_spike_steps_fwd
            .iter()
            .map(|(key, value)| (*key, *value))
            .collect(),
        remote_spike_steps_bwd: net
            .remote_spike_steps_bwd
            .iter()
            .map(|(key, value)| (*key, *value))
            .collect(),
        external_sensory_spikes: net.external_sensory_spikes.clone(),
    }
}

#[cfg(any(feature = "superdense_executor", feature = "replicated_durability"))]
fn restore_channel_state(net: &mut ManagedNetwork, state: ManagedChannelState) {
    net.remote_spikes_fwd = state.remote_spikes_fwd.into_iter().collect();
    net.remote_spikes_bwd = state.remote_spikes_bwd.into_iter().collect();
    net.remote_spike_steps_fwd = state.remote_spike_steps_fwd.into_iter().collect();
    net.remote_spike_steps_bwd = state.remote_spike_steps_bwd.into_iter().collect();
    net.external_sensory_spikes = state.external_sensory_spikes;
}

fn local_channel_state_json(net: &ManagedNetwork) -> Result<String, String> {
    serde_json::to_string(&capture_channel_state(net)).map_err(|error| error.to_string())
}

fn local_shard_snapshot(net: &ManagedNetwork) -> Result<(String, String, u64, u64), String> {
    #[cfg(feature = "replicated_durability")]
    let (snapshot_json, channel_state_json) = if let Some(owner) = net.durable_owner.as_ref() {
        let state = owner
            .authoritative_state()
            .map_err(|error| error.to_string())?;
        let snapshot = String::from_utf8(state.biological_state.clone())
            .map_err(|error| format!("durable biological snapshot is not UTF-8: {error}"))?;
        let channel = String::from_utf8(state.channel_state.clone())
            .map_err(|error| format!("durable channel state is not UTF-8: {error}"))?;
        (snapshot, channel)
    } else {
        (
            net.runner
                .export_network_json()
                .map_err(|error| error.to_string())?,
            local_channel_state_json(net)?,
        )
    };
    #[cfg(not(feature = "replicated_durability"))]
    let snapshot_json = net
        .runner
        .export_network_json()
        .map_err(|error| error.to_string())?;
    #[cfg(not(feature = "replicated_durability"))]
    let channel_state_json = local_channel_state_json(net)?;
    if snapshot_json.len().saturating_add(channel_state_json.len())
        > cluster_snapshot::MAX_SHARD_SNAPSHOT_BYTES
    {
        return Err(format!(
            "shard snapshot exceeds {} bytes",
            cluster_snapshot::MAX_SHARD_SNAPSHOT_BYTES
        ));
    }
    let snapshot = crate::runner::decode_snapshot_with_profile_backfill(&snapshot_json)
        .map_err(|error| error.to_string())?;
    Ok((
        snapshot_json,
        channel_state_json,
        snapshot.t as u64,
        snapshot.t_ms.to_bits(),
    ))
}

/// Return the complete sealed shard boundary for a durable managed network.
/// Compatibility runner networks intentionally return an empty value so a
/// caller cannot accidentally label a projection as a recoverable shard
/// state.  Cluster snapshot assembly validates and digests a non-empty value
/// as `authoritative_shard::ShardState`.
fn local_authoritative_state_json(_net: &ManagedNetwork) -> Result<String, String> {
    #[cfg(feature = "replicated_durability")]
    if let Some(owner) = _net.durable_owner.as_ref() {
        let state = owner
            .authoritative_state()
            .map_err(|error| error.to_string())?;
        return serde_json::to_string(&state).map_err(|error| error.to_string());
    }
    Ok(String::new())
}

fn local_cut_evidence(
    network_id: &str,
    node_id: &str,
    epoch: u64,
    snapshot_json: &str,
    channel_state_json: &str,
) -> Result<(ParticipantReport, ChannelMarker), String> {
    if epoch == 0 {
        return Err("consistent-cut epoch must be non-zero".to_owned());
    }
    let snapshot = crate::runner::decode_snapshot_with_profile_backfill(snapshot_json)
        .map_err(|error| format!("invalid captured shard snapshot: {error}"))?;
    let channel_state: ManagedChannelState = serde_json::from_str(channel_state_json)
        .map_err(|error| format!("invalid captured channel state: {error}"))?;
    let queued_min = channel_state
        .remote_spike_steps_fwd
        .values()
        .chain(channel_state.remote_spike_steps_bwd.values())
        .filter_map(|step| u64::try_from(*step).ok())
        .min()
        .map(|tick| LogicalTag::new(tick, 0));
    let local_frontier = LogicalTag::new(
        u64::try_from(snapshot.t).map_err(|_| "captured shard tick exceeds u64".to_owned())?,
        0,
    );
    let participant = ParticipantReport {
        participant: node_id.to_owned(),
        local_frontier,
        queued_min,
        in_flight_min: None,
        // The runner has no biological activity epoch of its own yet; the
        // non-zero monotonic step-derived epoch still makes stale reports
        // distinguishable within a cut protocol.
        activity_epoch: local_frontier.tick.saturating_add(1),
    };
    let marker = ChannelMarker::new(
        format!("{network_id}/{node_id}"),
        epoch,
        queued_min,
        channel_state_json.as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    Ok((participant, marker))
}

#[derive(Clone, Debug)]
struct LiveSnapshotSource {
    network_id: String,
    primary_node_id: Option<String>,
    peer_addr: Option<String>,
    local: bool,
}

fn connect_addr_for_node(state: &NodeState, node_id: &str) -> Option<String> {
    if let Some(addr) = state.peers.get(node_id) {
        return Some(addr.clone());
    }
    let advertised = state.nodes.get(node_id)?.address.trim();
    if advertised.is_empty() {
        return None;
    }
    if advertised.starts_with("http://") || advertised.starts_with("https://") {
        Some(advertised.to_string())
    } else {
        Some(format!("http://{}", advertised))
    }
}

fn live_snapshot_source_for(
    state: &NodeState,
    network_id: &str,
    distribution: &HashMap<String, LayerRange>,
) -> Option<LiveSnapshotSource> {
    let hosted_locally = state.networks.contains_key(network_id);
    let primary_node_id = primary_node_for_distribution(distribution)
        .or_else(|| hosted_locally.then(|| state.node_id.clone()));

    let local = primary_node_id
        .as_deref()
        .map(|node_id| node_id == state.node_id && hosted_locally)
        .unwrap_or(hosted_locally);
    let peer_addr = primary_node_id.as_ref().and_then(|node_id| {
        if node_id == &state.node_id {
            None
        } else {
            connect_addr_for_node(state, node_id)
        }
    });

    if !local && peer_addr.is_none() {
        return None;
    }

    Some(LiveSnapshotSource {
        network_id: network_id.to_string(),
        primary_node_id,
        peer_addr,
        local,
    })
}

#[derive(Clone, Debug)]
struct AutonomousTransitionPlan {
    network_id: String,
    next_deployment: DeploymentConfig,
    fallback_payload: String,
    reason: String,
    snapshot_source: Option<LiveSnapshotSource>,
}

#[derive(Clone, Debug, Default)]
struct ExternalTelemetrySnapshot {
    source: String,
    ts_ms: u64,
    cpu_usage_pct: Option<f32>,
    mem_used_pct: Option<f32>,
    net_rx_bps: Option<f64>,
    net_tx_bps: Option<f64>,
    disk_used_pct: Option<f32>,
    disk_read_bps: Option<f64>,
    disk_write_bps: Option<f64>,
    gpu_count: u32,
    gpu_util_pct: Option<f32>,
    gpu_temp_c: Option<f32>,
    gpu_power_w: Option<f32>,
    gpu_mem_used_pct: Option<f32>,
    recent_action_count: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct TraceyStatusEnvelope {
    ts_ms: u64,
    continuum_telemetry: Option<TraceyContinuumTelemetry>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct TraceyContinuumTelemetry {
    ts_ms: u64,
    server: TraceyContinuumServer,
    gpus: Vec<TraceyContinuumGpu>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct TraceyContinuumServer {
    cpu_usage_pct: Option<f64>,
    mem_used_pct: Option<f64>,
    net_rx_bps: Option<f64>,
    net_tx_bps: Option<f64>,
    recent_action_count: usize,
    disks: Vec<TraceyContinuumDisk>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct TraceyContinuumDisk {
    used_ratio: Option<f64>,
    read_bps: Option<f64>,
    write_bps: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct TraceyContinuumGpu {
    util_pct: Option<f64>,
    temp_c: Option<f64>,
    power_w: Option<f64>,
    mem_used_pct: Option<f64>,
}

#[derive(Clone, Debug, Default)]
struct TraceyProbeCache {
    last_attempt: Option<std::time::Instant>,
    last_success: Option<std::time::Instant>,
    snapshot: Option<ExternalTelemetrySnapshot>,
}

#[derive(Clone)]
struct TraceyStatusProbe {
    client: reqwest::Client,
    url: String,
    cache_ttl: Duration,
    failure_backoff: Duration,
    cache: Arc<RwLock<TraceyProbeCache>>,
}

impl TraceyStatusProbe {
    fn from_env() -> Option<Self> {
        #[cfg(test)]
        if std::env::var("NM_TRACEY_STATUS_URL").is_err() {
            return None;
        }

        let url = tracey_status_url_from_env()?;
        let timeout = tracey_status_timeout();
        let client = reqwest::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .ok()?;
        Some(Self {
            client,
            url,
            cache_ttl: tracey_status_cache_ttl(),
            failure_backoff: tracey_status_failure_backoff(),
            cache: Arc::new(RwLock::new(TraceyProbeCache::default())),
        })
    }

    async fn snapshot(&self) -> Option<ExternalTelemetrySnapshot> {
        {
            let cache = self.cache.read().await;
            if let (Some(snapshot), Some(last_success)) = (&cache.snapshot, cache.last_success) {
                if last_success.elapsed() <= self.cache_ttl {
                    return Some(snapshot.clone());
                }
            }
            if cache.snapshot.is_none()
                && cache
                    .last_attempt
                    .map(|last_attempt| last_attempt.elapsed() <= self.failure_backoff)
                    .unwrap_or(false)
            {
                return None;
            }
        }

        {
            let mut cache = self.cache.write().await;
            if let (Some(snapshot), Some(last_success)) = (&cache.snapshot, cache.last_success) {
                if last_success.elapsed() <= self.cache_ttl {
                    return Some(snapshot.clone());
                }
            }
            if cache
                .last_attempt
                .map(|last_attempt| last_attempt.elapsed() <= self.failure_backoff)
                .unwrap_or(false)
                && cache.snapshot.is_none()
            {
                return None;
            }
            cache.last_attempt = Some(std::time::Instant::now());
        }

        let stale_snapshot = self.cache.read().await.snapshot.clone();
        let response = match self.client.get(&self.url).send().await {
            Ok(response) => response,
            Err(_) => return stale_snapshot,
        };
        let envelope = match response.json::<TraceyStatusEnvelope>().await {
            Ok(envelope) => envelope,
            Err(_) => return stale_snapshot,
        };
        let Some(snapshot) = external_telemetry_from_tracey(&self.url, envelope) else {
            return stale_snapshot;
        };
        let mut cache = self.cache.write().await;
        cache.last_success = Some(std::time::Instant::now());
        cache.snapshot = Some(snapshot.clone());
        Some(snapshot)
    }
}

fn normalize_tracey_status_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let disabled = trimmed.to_ascii_lowercase();
    if matches!(
        disabled.as_str(),
        "0" | "false" | "off" | "disable" | "disabled"
    ) {
        return None;
    }

    let mut url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    };
    if !url.trim_end_matches('/').ends_with("/status") {
        url = format!("{}/status", url.trim_end_matches('/'));
    }
    Some(url)
}

fn tracey_status_url_from_env() -> Option<String> {
    if let Ok(raw) = std::env::var("NM_TRACEY_STATUS_URL") {
        return normalize_tracey_status_url(&raw);
    }
    normalize_tracey_status_url("http://127.0.0.1:48000")
}

fn tracey_duration_env(name: &str, default_ms: u64) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

fn tracey_status_timeout() -> Duration {
    tracey_duration_env("NM_TRACEY_STATUS_TIMEOUT_MS", 80)
}

fn spike_burst_timeout() -> Duration {
    tracey_duration_env("NM_SPIKE_BURST_TIMEOUT_MS", DEFAULT_SPIKE_BURST_TIMEOUT_MS)
}

fn tracey_status_cache_ttl() -> Duration {
    tracey_duration_env("NM_TRACEY_STATUS_CACHE_TTL_MS", 1_000)
}

fn tracey_status_failure_backoff() -> Duration {
    tracey_duration_env("NM_TRACEY_STATUS_FAILURE_BACKOFF_MS", 2_000)
}

fn external_telemetry_from_tracey(
    url: &str,
    envelope: TraceyStatusEnvelope,
) -> Option<ExternalTelemetrySnapshot> {
    let continuum = envelope.continuum_telemetry?;
    let disk_used_pct = continuum
        .server
        .disks
        .iter()
        .filter_map(|disk| disk.used_ratio)
        .map(|ratio| (ratio * 100.0).clamp(0.0, 100.0) as f32)
        .max_by(f32::total_cmp);
    let disk_read_bps = continuum
        .server
        .disks
        .iter()
        .filter_map(|disk| disk.read_bps)
        .max_by(f64::total_cmp);
    let disk_write_bps = continuum
        .server
        .disks
        .iter()
        .filter_map(|disk| disk.write_bps)
        .max_by(f64::total_cmp);
    let gpu_count = continuum.gpus.len().min(u32::MAX as usize) as u32;
    let gpu_util_pct = if continuum.gpus.is_empty() {
        None
    } else {
        let mut samples = 0usize;
        let mut total = 0.0f64;
        for gpu in &continuum.gpus {
            if let Some(util_pct) = gpu.util_pct {
                total += util_pct.clamp(0.0, 100.0);
                samples += 1;
            }
        }
        (samples > 0).then_some((total / samples as f64) as f32)
    };
    let gpu_temp_c = continuum
        .gpus
        .iter()
        .filter_map(|gpu| gpu.temp_c)
        .max_by(f64::total_cmp)
        .map(|value| value as f32);
    let gpu_power_total = continuum
        .gpus
        .iter()
        .filter_map(|gpu| gpu.power_w)
        .sum::<f64>();
    let gpu_power_w = (gpu_power_total > 0.0).then_some(gpu_power_total as f32);
    let gpu_mem_used_pct = continuum
        .gpus
        .iter()
        .filter_map(|gpu| gpu.mem_used_pct)
        .max_by(f64::total_cmp)
        .map(|value| value as f32);

    Some(ExternalTelemetrySnapshot {
        source: url.to_string(),
        ts_ms: continuum.ts_ms.max(envelope.ts_ms),
        cpu_usage_pct: continuum.server.cpu_usage_pct.map(|value| value as f32),
        mem_used_pct: continuum.server.mem_used_pct.map(|value| value as f32),
        net_rx_bps: continuum.server.net_rx_bps,
        net_tx_bps: continuum.server.net_tx_bps,
        disk_used_pct,
        disk_read_bps,
        disk_write_bps,
        gpu_count,
        gpu_util_pct,
        gpu_temp_c,
        gpu_power_w,
        gpu_mem_used_pct,
        recent_action_count: continuum.server.recent_action_count.min(u32::MAX as usize) as u32,
    })
}

fn resource_memory_pressure(resources: &Resources) -> f32 {
    if !resources.telemetry_source.is_empty() && resources.telemetry_mem_used_pct > 0.0 {
        return (resources.telemetry_mem_used_pct / 100.0).clamp(0.0, 1.0);
    }
    if resources.total_ram == 0 {
        return 0.0;
    }
    let used = resources.total_ram.saturating_sub(resources.available_ram);
    (used as f32 / resources.total_ram as f32).clamp(0.0, 1.0)
}

fn external_telemetry_network_pressure(snapshot: &ExternalTelemetrySnapshot) -> f32 {
    let rx = snapshot.net_rx_bps.unwrap_or_default().max(0.0);
    let tx = snapshot.net_tx_bps.unwrap_or_default().max(0.0);
    ((rx.max(tx) / 125_000_000.0) as f32).clamp(0.0, 1.0)
}

fn external_telemetry_disk_pressure(snapshot: &ExternalTelemetrySnapshot) -> f32 {
    let usage = snapshot
        .disk_used_pct
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let throughput = ((snapshot
        .disk_read_bps
        .unwrap_or_default()
        .max(snapshot.disk_write_bps.unwrap_or_default())
        / 200_000_000.0) as f32)
        .clamp(0.0, 1.0);
    usage.max(throughput)
}

fn external_telemetry_gpu_pressure(snapshot: &ExternalTelemetrySnapshot) -> f32 {
    let util = snapshot
        .gpu_util_pct
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let mem = snapshot
        .gpu_mem_used_pct
        .map(|value| (value / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let temp = snapshot
        .gpu_temp_c
        .map(|value| (value / 85.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let power = if snapshot.gpu_count > 0 {
        snapshot
            .gpu_power_w
            .map(|value| (value / (snapshot.gpu_count as f32 * 300.0)).clamp(0.0, 1.0))
            .unwrap_or(0.0)
    } else {
        0.0
    };
    util.max(mem).max(temp).max(power)
}

fn external_telemetry_pressure(snapshot: &ExternalTelemetrySnapshot) -> f32 {
    let network = external_telemetry_network_pressure(snapshot);
    let disk = external_telemetry_disk_pressure(snapshot);
    let gpu = external_telemetry_gpu_pressure(snapshot);
    let actions = (snapshot.recent_action_count as f32 / 16.0).clamp(0.0, 1.0);
    ((network * 0.20) + (disk * 0.20) + (gpu * 0.45) + (actions * 0.15)).clamp(0.0, 1.0)
}

fn resource_network_pressure(resources: &Resources) -> f32 {
    if resources.telemetry_source.is_empty() {
        return 0.0;
    }
    external_telemetry_network_pressure(&ExternalTelemetrySnapshot {
        net_rx_bps: Some(resources.telemetry_net_rx_bps),
        net_tx_bps: Some(resources.telemetry_net_tx_bps),
        ..ExternalTelemetrySnapshot::default()
    })
}

fn resource_disk_pressure(resources: &Resources) -> f32 {
    if resources.telemetry_source.is_empty() {
        return 0.0;
    }
    external_telemetry_disk_pressure(&ExternalTelemetrySnapshot {
        disk_used_pct: Some(resources.telemetry_disk_used_pct),
        disk_read_bps: Some(resources.telemetry_disk_read_bps),
        disk_write_bps: Some(resources.telemetry_disk_write_bps),
        ..ExternalTelemetrySnapshot::default()
    })
}

fn resource_gpu_pressure(resources: &Resources) -> f32 {
    if resources.telemetry_source.is_empty() {
        return 0.0;
    }
    external_telemetry_gpu_pressure(&ExternalTelemetrySnapshot {
        gpu_count: resources.num_gpus,
        gpu_util_pct: Some(resources.telemetry_gpu_util_pct),
        gpu_temp_c: Some(resources.telemetry_gpu_temp_c),
        gpu_power_w: Some(resources.telemetry_gpu_power_w),
        gpu_mem_used_pct: Some(resources.telemetry_gpu_mem_used_pct),
        ..ExternalTelemetrySnapshot::default()
    })
}

fn resource_external_pressure(resources: &Resources) -> f32 {
    if resources.telemetry_source.is_empty() {
        return 0.0;
    }
    external_telemetry_pressure(&ExternalTelemetrySnapshot {
        source: resources.telemetry_source.clone(),
        net_rx_bps: Some(resources.telemetry_net_rx_bps),
        net_tx_bps: Some(resources.telemetry_net_tx_bps),
        disk_used_pct: Some(resources.telemetry_disk_used_pct),
        disk_read_bps: Some(resources.telemetry_disk_read_bps),
        disk_write_bps: Some(resources.telemetry_disk_write_bps),
        gpu_count: resources.num_gpus,
        gpu_util_pct: Some(resources.telemetry_gpu_util_pct),
        gpu_temp_c: Some(resources.telemetry_gpu_temp_c),
        gpu_power_w: Some(resources.telemetry_gpu_power_w),
        gpu_mem_used_pct: Some(resources.telemetry_gpu_mem_used_pct),
        recent_action_count: resources.telemetry_recent_action_count,
        ..ExternalTelemetrySnapshot::default()
    })
}

fn effective_capacity_score(resources: &Resources, rebalance_target_step_ms: f32) -> f32 {
    let mut effective = resources.capacity_score.max(0.1);
    let latency_ms = resources.avg_step_time_ms.max(0.0);
    if latency_ms > 0.0 {
        let latency_scale = (rebalance_target_step_ms / latency_ms).clamp(0.25, 2.0);
        effective *= latency_scale;
    }
    let external_pressure = resource_external_pressure(resources);
    if external_pressure > 0.0 {
        effective *= (1.0 - (external_pressure * 0.55)).clamp(0.35, 1.0);
    }
    effective.max(0.05)
}

fn node_memory_pressure(resources: &Resources) -> f32 {
    resource_memory_pressure(resources)
}

#[derive(Clone, Debug, Default)]
struct DeploymentTelemetry {
    avg_step_time_ms: f32,
    max_step_time_ms: f32,
    active_nodes: usize,
    avg_cpu_utilization: f32,
    max_cpu_utilization: f32,
    avg_memory_pressure: f32,
    avg_network_pressure: f32,
    avg_disk_pressure: f32,
    avg_gpu_pressure: f32,
    shared_with_related: bool,
    related_hotspot: bool,
}

fn maybe_autonomous_transition(
    deployment: &DeploymentConfig,
    telemetry: &DeploymentTelemetry,
    available_nodes: usize,
) -> Option<(DeploymentConfig, String)> {
    if !deployment.allows_autonomous_transition() || available_nodes == 0 {
        return None;
    }

    let target_step_time_ms = deployment.transition_policy.target_step_time_ms();
    let has_related_networks = !deployment.related_network_ids.is_empty()
        || deployment.combined_group.is_some()
        || deployment.federation_group.is_some();
    let can_shard =
        deployment.transition_mode_allowed(ExecutionMode::Sharded) && available_nodes > 1;
    let can_individual = deployment.transition_mode_allowed(ExecutionMode::Individual);
    let can_combined =
        has_related_networks && deployment.transition_mode_allowed(ExecutionMode::Combined);
    let can_federated =
        has_related_networks && deployment.transition_mode_allowed(ExecutionMode::Federated);

    let hot_network = telemetry.avg_step_time_ms > target_step_time_ms
        || telemetry.max_step_time_ms > target_step_time_ms * 1.15;
    let cluster_busy = telemetry.avg_cpu_utilization > 0.80
        || telemetry.max_cpu_utilization > 0.92
        || telemetry.avg_memory_pressure > 0.82
        || telemetry.avg_network_pressure > 0.80
        || telemetry.avg_disk_pressure > 0.82
        || telemetry.avg_gpu_pressure > 0.85;
    let underutilized =
        telemetry.avg_step_time_ms > 0.0 && telemetry.avg_step_time_ms < target_step_time_ms * 0.55;
    let current_shards = telemetry
        .active_nodes
        .max(if deployment.prefers_sharding() { 2 } else { 1 })
        .min(available_nodes.max(1));

    let mut next = deployment.clone();
    let mut reasons = Vec::new();

    if (hot_network || cluster_busy) && can_shard {
        let desired_shards = current_shards.saturating_add(1).clamp(2, available_nodes);
        if !next.prefers_sharding() || next.desired_shards != desired_shards {
            next.set_mode(ExecutionMode::Individual, false);
            next.add_mode(ExecutionMode::Distributed);
            next.add_mode(ExecutionMode::Sharded);
            next.desired_shards = desired_shards;
            reasons.push(format!("scale out to {} shard targets", desired_shards));
        }
    } else if underutilized && (next.prefers_sharding() || telemetry.active_nodes > 1) {
        if current_shards > 2 {
            let desired_shards = current_shards.saturating_sub(1).clamp(2, available_nodes);
            if next.desired_shards != desired_shards {
                next.desired_shards = desired_shards;
                reasons.push(format!("scale in to {} shard targets", desired_shards));
            }
        } else if can_individual {
            if next.prefers_sharding() || !next.has_mode(ExecutionMode::Individual) {
                next.set_mode(ExecutionMode::Sharded, false);
                next.set_mode(ExecutionMode::Individual, true);
                next.desired_shards = 1;
                reasons.push("collapse to isolated single-target execution".to_string());
            }
        }
    }

    if telemetry.shared_with_related && (hot_network || cluster_busy || telemetry.related_hotspot) {
        if can_federated && !next.has_mode(ExecutionMode::Federated) {
            next.set_mode(ExecutionMode::Federated, true);
            next.set_mode(ExecutionMode::Combined, false);
            reasons.push("spread related networks across different targets".to_string());
        }
    } else if can_combined && underutilized && !cluster_busy && !telemetry.shared_with_related {
        if !next.has_mode(ExecutionMode::Combined) || next.has_mode(ExecutionMode::Federated) {
            next.set_mode(ExecutionMode::Combined, true);
            next.set_mode(ExecutionMode::Federated, false);
            reasons.push("co-locate related networks to reduce coordination latency".to_string());
        }
    }

    if !next.prefers_sharding() && can_individual && !next.constrains_to_single_target() {
        next.set_mode(ExecutionMode::Individual, true);
    }
    if next.prefers_sharding() {
        next.set_mode(ExecutionMode::Individual, false);
    }

    next.normalize();
    (next != *deployment).then(|| (next, reasons.join("; ")))
}

fn collect_autonomous_transition_plans(
    state: &NodeState,
    transition_now: std::time::Instant,
) -> Vec<AutonomousTransitionPlan> {
    let node_ids: Vec<String> = state.nodes.keys().cloned().collect();
    if node_ids.is_empty() {
        return Vec::new();
    }

    let active_network_counts: HashMap<String, usize> = state
        .nodes
        .iter()
        .map(|(node_id, status)| (node_id.clone(), status.active_networks.len()))
        .collect();
    let existing_primary_nodes: HashMap<String, String> = state
        .network_registry
        .iter()
        .filter_map(|(net_id, status)| {
            primary_node_for_distribution(&status.distribution)
                .map(|node_id| (net_id.clone(), node_id))
        })
        .collect();
    let deployment_by_network: HashMap<String, DeploymentConfig> = state
        .network_registry
        .iter()
        .map(|(net_id, status)| {
            let payload = state
                .network_snapshots
                .get(net_id)
                .filter(|payload| !payload.trim().is_empty())
                .map(String::as_str)
                .unwrap_or(status.config_json.as_str());
            let deployment = network_deployment_from_payload(payload).unwrap_or_default();
            (net_id.clone(), deployment)
        })
        .collect();

    let mut plans = Vec::new();
    for (net_id, deployment) in &deployment_by_network {
        if !deployment.allows_autonomous_transition() {
            continue;
        }
        let cooldown_ms = deployment.transition_policy.cooldown_ms;
        if cooldown_ms > 0
            && state
                .last_deployment_transition
                .get(net_id)
                .map(|last| {
                    transition_now.duration_since(last.observed_at).as_millis()
                        < cooldown_ms as u128
                })
                .unwrap_or(false)
        {
            continue;
        }

        let Some(net_status) = state.network_registry.get(net_id) else {
            continue;
        };
        let related_ids = related_network_ids_for(net_id, deployment, &deployment_by_network);
        let primary_node = existing_primary_nodes.get(net_id);
        let shared_with_related = primary_node
            .map(|node_id| {
                related_ids
                    .iter()
                    .filter_map(|related_id| existing_primary_nodes.get(related_id))
                    .any(|other_node| other_node == node_id)
            })
            .unwrap_or(false);
        let related_hotspot = related_ids.iter().any(|related_id| {
            let Some(node_id) = existing_primary_nodes.get(related_id) else {
                return false;
            };
            let active_networks = active_network_counts.get(node_id).copied().unwrap_or(0);
            let node_resources = state
                .nodes
                .get(node_id)
                .and_then(|node| node.resources.as_ref());
            let cpu_ratio = node_resources
                .map(|resources| (resources.cpu_usage / 100.0).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let memory_pressure = node_resources.map(node_memory_pressure).unwrap_or(0.0);
            let external_pressure = node_resources
                .map(resource_external_pressure)
                .unwrap_or(0.0);
            active_networks > deployment.max_concurrent_networks.max(1)
                || cpu_ratio > 0.80
                || memory_pressure > 0.82
                || external_pressure > 0.82
        });

        let mut telemetry = DeploymentTelemetry {
            active_nodes: net_status.distribution.len(),
            shared_with_related,
            related_hotspot,
            ..DeploymentTelemetry::default()
        };

        if let Some(per_node_metrics) = state.network_runtime_metrics.get(net_id) {
            telemetry.active_nodes = telemetry.active_nodes.max(per_node_metrics.len());
            let metric_count = per_node_metrics.len() as f32;
            if metric_count > 0.0 {
                telemetry.avg_step_time_ms = per_node_metrics
                    .values()
                    .map(|metrics| metrics.avg_step_time_ms.max(0.0))
                    .sum::<f32>()
                    / metric_count;
                telemetry.max_step_time_ms = per_node_metrics
                    .values()
                    .map(|metrics| metrics.avg_step_time_ms.max(0.0))
                    .fold(0.0, f32::max);
            }
        }

        let mut cpu_sum = 0.0f32;
        let mut mem_sum = 0.0f32;
        let mut net_sum = 0.0f32;
        let mut disk_sum = 0.0f32;
        let mut gpu_sum = 0.0f32;
        let mut util_samples = 0usize;
        let mut max_cpu = 0.0f32;
        let mut assignment_nodes: HashSet<String> =
            net_status.distribution.keys().cloned().collect();
        if assignment_nodes.is_empty() {
            if let Some(metric_nodes) = state.network_runtime_metrics.get(net_id) {
                assignment_nodes.extend(metric_nodes.keys().cloned());
            }
        }
        for node_id in assignment_nodes {
            if let Some(resources) = state
                .nodes
                .get(&node_id)
                .and_then(|node| node.resources.as_ref())
            {
                let cpu_ratio = (resources.cpu_usage / 100.0).clamp(0.0, 1.0);
                let memory_pressure = node_memory_pressure(resources);
                let network_pressure = resource_network_pressure(resources);
                let disk_pressure = resource_disk_pressure(resources);
                let gpu_pressure = resource_gpu_pressure(resources);
                cpu_sum += cpu_ratio;
                mem_sum += memory_pressure;
                net_sum += network_pressure;
                disk_sum += disk_pressure;
                gpu_sum += gpu_pressure;
                max_cpu = max_cpu.max(cpu_ratio);
                util_samples += 1;
            }
        }
        if util_samples > 0 {
            telemetry.avg_cpu_utilization = cpu_sum / util_samples as f32;
            telemetry.avg_memory_pressure = mem_sum / util_samples as f32;
            telemetry.avg_network_pressure = net_sum / util_samples as f32;
            telemetry.avg_disk_pressure = disk_sum / util_samples as f32;
            telemetry.avg_gpu_pressure = gpu_sum / util_samples as f32;
            telemetry.max_cpu_utilization = max_cpu;
        }

        let Some((next_deployment, reason)) =
            maybe_autonomous_transition(deployment, &telemetry, node_ids.len())
        else {
            continue;
        };

        let fallback_payload = state
            .network_snapshots
            .get(net_id)
            .filter(|payload| !payload.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| net_status.config_json.clone());

        plans.push(AutonomousTransitionPlan {
            network_id: net_id.clone(),
            next_deployment,
            fallback_payload,
            reason,
            snapshot_source: live_snapshot_source_for(state, net_id, &net_status.distribution),
        });
    }

    plans
}

fn primary_node_for_distribution(distribution: &HashMap<String, LayerRange>) -> Option<String> {
    distribution
        .iter()
        .max_by(|lhs, rhs| {
            let lhs_len = lhs.1.layers.len();
            let rhs_len = rhs.1.layers.len();
            lhs_len
                .cmp(&rhs_len)
                .then_with(|| {
                    let lhs_min = lhs.1.layers.iter().min().copied().unwrap_or(u32::MAX);
                    let rhs_min = rhs.1.layers.iter().min().copied().unwrap_or(u32::MAX);
                    rhs_min.cmp(&lhs_min)
                })
                .then_with(|| lhs.0.cmp(rhs.0))
        })
        .map(|(node_id, _)| node_id.clone())
}

fn related_network_ids_for(
    network_id: &str,
    deployment: &DeploymentConfig,
    deployments: &HashMap<String, DeploymentConfig>,
) -> Vec<String> {
    let mut related: Vec<String> = deployment.related_network_ids.clone();
    if let Some(group) = deployment.combined_group.as_deref() {
        for (other_id, other) in deployments {
            if other_id != network_id && other.combined_group.as_deref() == Some(group) {
                related.push(other_id.clone());
            }
        }
    }
    if let Some(group) = deployment.federation_group.as_deref() {
        for (other_id, other) in deployments {
            if other_id != network_id && other.federation_group.as_deref() == Some(group) {
                related.push(other_id.clone());
            }
        }
    }
    let mut seen = HashSet::new();
    related.retain(|item| seen.insert(item.clone()));
    related
}

fn choose_single_node_target(
    network_id: &str,
    candidates: &[(String, f32)],
    deployment: &DeploymentConfig,
    deployments: &HashMap<String, DeploymentConfig>,
    primary_nodes: &HashMap<String, String>,
) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    let related = related_network_ids_for(network_id, deployment, deployments);
    let related_nodes: HashSet<String> = related
        .iter()
        .filter_map(|related_id| primary_nodes.get(related_id).cloned())
        .collect();

    if deployment.combined_group.is_some()
        || deployment.has_mode(crate::deployment::ExecutionMode::Combined)
    {
        if let Some(best) = candidates
            .iter()
            .filter(|(node_id, _)| related_nodes.contains(node_id))
            .max_by(|lhs, rhs| lhs.1.total_cmp(&rhs.1))
        {
            return Some(best.0.clone());
        }
    }

    if deployment.federation_group.is_some()
        || deployment.has_mode(crate::deployment::ExecutionMode::Federated)
    {
        if let Some(best) = candidates
            .iter()
            .filter(|(node_id, _)| !related_nodes.contains(node_id))
            .max_by(|lhs, rhs| lhs.1.total_cmp(&rhs.1))
        {
            return Some(best.0.clone());
        }
    }

    candidates
        .iter()
        .max_by(|lhs, rhs| lhs.1.total_cmp(&rhs.1))
        .map(|(node_id, _)| node_id.clone())
}

fn build_sharded_node_assignments(
    target_node_capacities: &[(String, f32)],
    total_layers: u32,
) -> Vec<(String, Vec<u32>, Vec<u32>)> {
    if target_node_capacities.is_empty() || total_layers == 0 {
        return Vec::new();
    }

    let mut sorted_targets = target_node_capacities.to_vec();
    sorted_targets.sort_by(|lhs, rhs| rhs.1.total_cmp(&lhs.1).then_with(|| lhs.0.cmp(&rhs.0)));

    let all_layers: Vec<u32> = (0..total_layers).collect();

    // Very small networks (for example Celegans snapshots with one hidden layer
    // plus output) cannot be split into more unique contiguous ranges than there
    // are layers. If we apply the normal ±1 overlap expansion here, every node
    // ends up with the full network and the per-node views become useless.
    //
    // Keep the strongest node as an anchor that hosts the full end-to-end path
    // for UI/Webots and assign single-layer partial ranges to the remaining
    // nodes so local managed views still reflect actual per-node ownership.
    if (total_layers as usize) <= sorted_targets.len() {
        let mut assignments = Vec::with_capacity(sorted_targets.len());
        assignments.push((
            sorted_targets[0].0.clone(),
            all_layers.clone(),
            all_layers.clone(),
        ));
        for (idx, (node_id, _)) in sorted_targets.iter().enumerate().skip(1) {
            let layer = ((idx - 1) % total_layers as usize) as u32;
            assignments.push((node_id.clone(), vec![layer], vec![layer]));
        }
        return assignments;
    }

    let mut layer_counts = vec![0u32; total_layers as usize];
    let mut node_assignments = Vec::with_capacity(sorted_targets.len());
    let mut target_capacity_sum: f32 = sorted_targets.iter().map(|(_, cap)| *cap).sum();
    if target_capacity_sum <= 0.0 {
        target_capacity_sum = sorted_targets.len() as f32;
    }

    let mut current_cap_sum = 0.0;
    for (node_id, cap) in &sorted_targets {
        let start_ratio = current_cap_sum / target_capacity_sum;
        current_cap_sum += cap;
        let end_ratio = current_cap_sum / target_capacity_sum;

        let start = (start_ratio * total_layers as f32).round() as u32;
        let end = (end_ratio * total_layers as f32).round() as u32;

        // Ensure at least one layer if there's any remaining capacity.
        let end = if start == end && end < total_layers {
            end + 1
        } else {
            end
        };

        // Add overlap for boundary synchronization/redundancy.
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

    node_assignments
        .into_iter()
        .map(|(node_id, layers)| {
            let redundant: Vec<u32> = layers
                .iter()
                .filter(|&&l| (l as usize) < layer_counts.len() && layer_counts[l as usize] > 1)
                .copied()
                .collect();
            (node_id, layers, redundant)
        })
        .collect()
}

fn deployment_prefers_combined(deployment: &DeploymentConfig) -> bool {
    deployment.combined_group.is_some()
        || deployment.has_mode(crate::deployment::ExecutionMode::Combined)
}

fn deployment_prefers_federated(deployment: &DeploymentConfig) -> bool {
    deployment.federation_group.is_some()
        || deployment.has_mode(crate::deployment::ExecutionMode::Federated)
}

fn should_shard_across_nodes(deployment: &DeploymentConfig) -> bool {
    if deployment.constrains_to_single_target() {
        return false;
    }
    deployment.modes.is_empty() || deployment.prefers_sharding()
}

fn limit_target_nodes_for_deployment(
    network_id: &str,
    candidates: &[(String, f32)],
    deployment: &DeploymentConfig,
    deployments: &HashMap<String, DeploymentConfig>,
    primary_nodes: &HashMap<String, String>,
    active_network_counts: &HashMap<String, usize>,
    existing_affinity_nodes: &HashSet<String>,
) -> Vec<(String, f32)> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut filtered: Vec<(String, f32)> = candidates.to_vec();

    if deployment.max_concurrent_networks > 0 {
        let concurrency_limit = deployment.max_concurrent_networks.max(1);
        let under_limit: Vec<(String, f32)> = filtered
            .iter()
            .filter(|(node_id, _)| {
                active_network_counts.get(node_id).copied().unwrap_or(0) < concurrency_limit
                    || existing_affinity_nodes.contains(node_id)
            })
            .cloned()
            .collect();
        if !under_limit.is_empty() {
            filtered = under_limit;
        }
    }

    if !should_shard_across_nodes(deployment) {
        return filtered;
    }

    let target_count = deployment.requested_shard_count(filtered.len());
    if target_count == 0 || filtered.len() <= target_count {
        return filtered;
    }

    let related_nodes: HashSet<String> =
        related_network_ids_for(network_id, deployment, deployments)
            .iter()
            .filter_map(|related_id| primary_nodes.get(related_id).cloned())
            .collect();
    let prefers_combined = deployment_prefers_combined(deployment);
    let prefers_federated = deployment_prefers_federated(deployment);

    filtered.sort_by(|lhs, rhs| {
        let lhs_affinity = existing_affinity_nodes.contains(&lhs.0);
        let rhs_affinity = existing_affinity_nodes.contains(&rhs.0);
        rhs_affinity
            .cmp(&lhs_affinity)
            .then_with(|| {
                if prefers_combined {
                    let lhs_related = related_nodes.contains(&lhs.0);
                    let rhs_related = related_nodes.contains(&rhs.0);
                    rhs_related.cmp(&lhs_related)
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| {
                if prefers_federated {
                    let lhs_separate = !related_nodes.contains(&lhs.0);
                    let rhs_separate = !related_nodes.contains(&rhs.0);
                    rhs_separate.cmp(&lhs_separate)
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| rhs.1.total_cmp(&lhs.1))
            .then_with(|| lhs.0.cmp(&rhs.0))
    });
    filtered.truncate(target_count);
    filtered
}

/// Represents a partial or whole neural network running on this node.
pub struct ManagedNetwork {
    pub id: String,
    pub runner: Runner,
    /// Fenced stable-shard admission gate. The legacy runner is a projection
    /// until this gate has matching durable owner evidence and an adopted
    /// placement generation.
    pub shard_runtime: Option<crate::managed_shard_runtime::ManagedShardRuntime>,
    /// Explicit stable-ID durable executor registration. When present, the
    /// compatibility Runner is a presentation projection and the legacy
    /// managed step path is fenced off until the stable adapter is removed.
    #[cfg(feature = "stable_executor_live")]
    pub stable_executor: Option<crate::managed_stable_executor::ManagedStableExecutor>,
    /// Opt-in durable owner for the live managed-network loop. The legacy
    /// runner remains the rollback path until the production migration gate is
    /// explicitly enabled and its storage/authority policy is configured.
    #[cfg(feature = "replicated_durability")]
    pub durable_owner: Option<crate::managed_durability::ManagedDurability>,
    #[cfg(feature = "superdense_executor")]
    pub(crate) superdense: SuperdenseController,
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

/// Adapter that binds one explicitly registered stable runtime to the
/// orchestrator migration registry.
///
/// The registry invokes this adapter on a blocking worker. The adapter then
/// takes the managed network's exclusive async lock, marks the network
/// paused, and borrows the existing durable bridge for one migration. No
/// bridge or biological state is copied, and a failed operation leaves the
/// source paused so an operator can inspect and retry it safely.
#[cfg(feature = "stable_executor_live")]
pub struct ManagedStableNetworkMigrationExecutor {
    network: Arc<RwLock<ManagedNetwork>>,
    delegate: crate::migration_executor::StableExecutorMigrationExecutor,
}

#[cfg(feature = "stable_executor_live")]
impl ManagedStableNetworkMigrationExecutor {
    pub fn new(
        network: Arc<RwLock<ManagedNetwork>>,
        settings: crate::migration_executor::StableExecutorMigrationSettings,
    ) -> Result<Self, String> {
        Ok(Self {
            network,
            delegate:
                crate::migration_executor::StableExecutorMigrationExecutor::new_for_managed_runtime(
                    settings,
                )?,
        })
    }
}

#[cfg(feature = "stable_executor_live")]
impl crate::migration_executor::MigrationExecutor for ManagedStableNetworkMigrationExecutor {
    fn execute(
        &self,
        operation: crate::migration_operation::MigrationOperation,
        group: crate::migration_group::MigrationGroupSpec,
    ) -> Result<crate::migration_executor::MigrationDispatchReceipt, String> {
        // MigrationExecutorRegistry always calls implementations from
        // spawn_blocking. Holding this guard serialises migration with the
        // simulation loop without blocking an async runtime worker thread.
        let mut network = self.network.blocking_write();
        network.playing = false;
        let Some(runtime) = network.stable_executor.as_mut() else {
            return Err("stable migration source runtime is not registered".to_owned());
        };
        let result = self
            .delegate
            .execute_with_bridge(runtime.bridge_mut(), operation, group);
        if let Err(error) = &result {
            nm_err!(
                "[warn] stable migration source {} remains paused after failure: {}",
                network.id,
                error
            );
        }
        result
    }
}

impl ManagedNetwork {
    /// Construct a managed network with no attached durable authority.
    ///
    /// Runtime owners are attached explicitly after construction so callers
    /// cannot accidentally create two authorities by copying internal state.
    /// The compatibility runner remains paused until the caller deliberately
    /// starts it, or a stable executor is registered and started.
    pub fn new(
        id: String,
        runner: Runner,
        initial_config: NetworkConfig,
        initial_model: NeuronModel,
        initial_learning: Learning,
        initial_lif: LIFParams,
        initial_stdp: STDPParams,
    ) -> Self {
        Self {
            id,
            runner,
            shard_runtime: None,
            #[cfg(feature = "stable_executor_live")]
            stable_executor: None,
            #[cfg(feature = "replicated_durability")]
            durable_owner: None,
            #[cfg(feature = "superdense_executor")]
            superdense: SuperdenseController::new(),
            assigned_layers: Vec::new(),
            redundant_layers: Vec::new(),
            remote_spikes_fwd: HashMap::new(),
            remote_spikes_bwd: HashMap::new(),
            remote_spike_steps_fwd: HashMap::new(),
            remote_spike_steps_bwd: HashMap::new(),
            external_sensory_spikes: None,
            avg_step_time_ms: 0.0,
            desired_aarnn_depth: initial_config.aarnn_layer_depth as u32,
            playing: false,
            initial_config,
            initial_model,
            initial_learning,
            initial_lif,
            initial_stdp,
            last_config_fingerprint: None,
            workspace_binding: None,
        }
    }

    fn admit_shard_step(&self) -> Result<(), String> {
        let Some(runtime) = self.shard_runtime.as_ref() else {
            return Ok(());
        };
        let evidence = &runtime.evidence;
        runtime
            .admit(
                evidence.brain_id,
                evidence.shard_id,
                evidence.topology_generation,
                evidence.partition_generation,
                evidence.lease_term,
                evidence.fencing_token,
            )
            .map_err(|error| error.to_string())
    }

    #[cfg(feature = "replicated_durability")]
    fn commit_shard_step(
        &mut self,
        tag: LogicalTag,
        digest: crate::deterministic::StateDigest,
    ) -> Result<(), String> {
        if let Some(runtime) = self.shard_runtime.as_mut() {
            runtime
                .commit(tag, digest)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    #[cfg(feature = "superdense_executor")]
    fn step_with_superdense(
        &mut self,
        sensory: Option<&[i8]>,
    ) -> Result<crate::runner::StepOut, crate::superdense::SuperdenseError> {
        self.superdense.step(&mut self.runner, sensory)
    }

    #[cfg(feature = "stable_executor_live")]
    /// Register the authoritative stable-ID executor explicitly. Discovery,
    /// placement telemetry, and a durable legacy Runner owner never perform
    /// this registration implicitly.
    pub fn register_stable_executor(
        &mut self,
        runtime: crate::managed_stable_executor::ManagedStableExecutor,
    ) -> Result<(), String> {
        if self.stable_executor.is_some() {
            return Err("stable executor is already registered".to_owned());
        }
        if self.playing {
            return Err(
                "pause the managed network before registering a stable executor".to_owned(),
            );
        }
        #[cfg(feature = "replicated_durability")]
        if self.durable_owner.is_some() {
            return Err(
                "legacy durable Runner owner is still attached; stable executor registration would create two authorities"
                    .to_owned(),
            );
        }
        if runtime.bridge().authority().brain_id()
            != crate::managed_durability::managed_brain_id(&self.id)
        {
            return Err(
                "stable executor brain identity does not match the managed network".to_owned(),
            );
        }
        self.stable_executor = Some(runtime);
        Ok(())
    }

    #[cfg(feature = "stable_executor_live")]
    pub fn stable_executor_registered(&self) -> bool {
        self.stable_executor.is_some()
    }

    #[cfg(feature = "stable_executor_live")]
    /// Poll the explicitly registered stable executor without touching the
    /// legacy Runner projection. The caller owns scheduling and must invoke
    /// this from a bounded worker context rather than a UI thread.
    pub fn poll_stable_executor(
        &mut self,
        observed_term: crate::deterministic::LeaseTerm,
        observed_fencing_token: u64,
        inputs: &[crate::shard_executor::RoutedCausalEvent],
    ) -> Result<crate::managed_stable_executor::ManagedStablePoll, String> {
        self.stable_executor
            .as_mut()
            .ok_or_else(|| "no stable executor is registered".to_owned())?
            .poll(observed_term, observed_fencing_token, inputs)
            .map_err(|error| error.to_string())
    }

    #[cfg(feature = "stable_executor_live")]
    /// Poll the registered stable runtime with a bounded sensory vector. The
    /// runtime's own lease/fencing evidence is supplied to the executor; a
    /// caller cannot substitute the legacy Runner's authority values.
    pub fn poll_stable_executor_sensory(
        &mut self,
        sensory: Option<&[i8]>,
    ) -> Result<crate::managed_stable_executor::ManagedStablePoll, String> {
        let runtime = self
            .stable_executor
            .as_mut()
            .ok_or_else(|| "no stable executor is registered".to_owned())?;
        let term = runtime.lease_term();
        let fencing_token = runtime.fencing_token();
        match sensory {
            Some(values) => runtime
                .poll_sensory(term, fencing_token, values)
                .map_err(|error| error.to_string()),
            None => runtime
                .drain(term, fencing_token)
                .map_err(|error| error.to_string()),
        }
    }

    #[cfg(feature = "stable_executor_live")]
    /// Admit one network causal envelope through the registered stable
    /// executor. The worker loop supplies the runtime's own term and fence;
    /// callers cannot substitute the legacy Runner authority.
    pub fn poll_stable_executor_envelope(
        &mut self,
        envelope: &crate::data_plane::CausalEnvelope,
    ) -> Result<crate::managed_stable_executor::ManagedStablePoll, String> {
        let runtime = self
            .stable_executor
            .as_mut()
            .ok_or_else(|| "no stable executor is registered".to_owned())?;
        let term = runtime.lease_term();
        let fencing_token = runtime.fencing_token();
        runtime
            .poll_envelope(envelope, term, fencing_token)
            .map_err(|error| error.to_string())
    }
}

#[cfg(feature = "replicated_durability")]
impl ManagedNetwork {
    /// Execute one managed-network step and publish its result through the
    /// durable owner before the caller exposes any distributed output.  The
    /// legacy Runner remains the biological kernel during migration, but its
    /// mutation is treated as staged state and is restored when durability or
    /// fencing rejects the commit.
    #[cfg(test)]
    fn step_and_commit_durable(
        &mut self,
        sensory: Option<&[i8]>,
        previous_channel_state: ManagedChannelState,
    ) -> Result<crate::runner::StepOut, String> {
        self.step_and_commit_durable_with_outbox(sensory, previous_channel_state, &[])
            .map(|(out, _)| out)
    }

    /// Step the compatibility biological kernel and publish the resulting
    /// snapshot together with all destination-scoped causal output in one
    /// recoverable durable transaction. The returned batches are exactly the
    /// batches recorded in the outbox; the caller only performs network I/O
    /// after this method succeeds.
    fn step_and_commit_durable_with_outbox(
        &mut self,
        sensory: Option<&[i8]>,
        previous_channel_state: ManagedChannelState,
        outbox_peers: &[String],
    ) -> Result<(crate::runner::StepOut, Vec<SpikeBatch>), String> {
        #[cfg(feature = "stable_executor_live")]
        if self.stable_executor.is_some() {
            return Err(
                "stable executor is registered; use poll_stable_executor for authoritative work"
                    .to_owned(),
            );
        }
        let mut rollback_channel_state = previous_channel_state;
        let previous_runner = self
            .durable_owner
            .as_ref()
            .map(|_| {
                self.runner
                    .export_network_json()
                    .map_err(|error| error.to_string())
            })
            .transpose()?;

        // The Runner is only a working projection while the durable profile
        // is enabled. Always refresh it from the shard owner before running a
        // transition so a restart/rejoin or another authoritative apply
        // cannot be overwritten by a stale local vector state.
        if let Some(owner) = self.durable_owner.as_ref() {
            if let Some(snapshot) = owner
                .authoritative_snapshot()
                .map_err(|error| error.to_string())?
            {
                self.runner
                    .import_network_json(&snapshot)
                    .map_err(|error| error.to_string())?;
            }
            let channel_state = owner
                .authoritative_channel_state()
                .map_err(|error| error.to_string())?;
            rollback_channel_state = serde_json::from_str(&channel_state)
                .map_err(|error| format!("invalid durable channel state: {error}"))?;
            restore_channel_state(self, rollback_channel_state.clone());
        }

        #[cfg(feature = "superdense_executor")]
        let out = self
            .step_with_superdense(sensory)
            .map_err(|error| error.to_string())?;
        #[cfg(not(feature = "superdense_executor"))]
        let out = if let Some(sensory) = sensory {
            self.runner.step(Some(sensory))
        } else {
            self.runner.step(None)
        };

        let batches = managed_spike_batches(self, out.t as i64);
        let mut durable_outbox = std::collections::BTreeMap::new();
        if !batches.is_empty() {
            let durable_batches = batches
                .iter()
                .map(|batch| crate::managed_durability::DurableCausalBatch {
                    layer_index: batch.layer_index,
                    step_index: batch.step_index,
                    is_backward: batch.is_backward,
                    spike_indices: batch.spike_indices.clone(),
                    aer_payload: batch.aer_payload.clone(),
                    aer_base: batch.aer_base,
                })
                .collect::<Vec<_>>();
            for peer in outbox_peers {
                durable_outbox.insert(peer.clone(), durable_batches.clone());
            }
        }

        if self.durable_owner.is_none() {
            return Ok((out, batches));
        }
        let snapshot = match self.runner.export_network_json() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if let Some(previous_runner) = previous_runner.as_ref() {
                    let _ = self.runner.import_network_json(previous_runner);
                }
                restore_channel_state(self, rollback_channel_state);
                return Err(error.to_string());
            }
        };
        let channel_state = match local_channel_state_json(self) {
            Ok(channel_state) => channel_state,
            Err(error) => {
                if let Some(previous_runner) = previous_runner.as_ref() {
                    let _ = self.runner.import_network_json(previous_runner);
                }
                restore_channel_state(self, rollback_channel_state);
                return Err(error);
            }
        };
        let owner = self
            .durable_owner
            .as_mut()
            .expect("durable owner was checked above");
        if let Err(error) = owner.commit_snapshot_with_channel_state_and_outbox(
            snapshot.as_bytes(),
            self.runner.t as u64,
            channel_state.as_bytes(),
            durable_outbox,
        ) {
            if let Some(previous_runner) = previous_runner.as_ref() {
                let _ = self.runner.import_network_json(previous_runner);
            }
            restore_channel_state(self, rollback_channel_state);
            return Err(error.to_string());
        }
        Ok((out, batches))
    }
}

/// Build the transport representation before durable publication. Keeping
/// this pure with respect to the managed network lets the commit intent record
/// the exact causal batches that correspond to the biological boundary.
fn managed_spike_batches(net: &ManagedNetwork, step_index: i64) -> Vec<SpikeBatch> {
    let ts_us = (net.runner.t_ms * 1000.0) as u64;
    let num_hidden = net.runner.net.num_hidden_layers as u32;
    let mut batches = Vec::new();
    for &layer in &net.redundant_layers {
        if layer >= num_hidden {
            continue;
        }
        let layer_idx = layer as usize;
        if layer_idx >= net.runner.last_spk_h.len() {
            continue;
        }
        let layer_spikes: Vec<i8> = net.runner.last_spk_h[layer_idx].iter().copied().collect();
        let exchange = encode_exchange(ts_us, 0, &layer_spikes);
        let indices = exchange.spike_indices;
        let mut aer_payload = exchange.aer_payload;
        if aer_payload.is_empty() {
            aer_payload.extend_from_slice(b"AER1");
            aer_payload.extend_from_slice(&ts_us.to_le_bytes());
        }
        batches.push(SpikeBatch {
            network_id: net.id.clone(),
            layer_index: layer,
            step_index,
            spike_indices: indices,
            is_backward: false,
            aer_payload,
            aer_base: 0,
        });
    }
    batches
}

fn config_payload_fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(feature = "replicated_durability")]
pub fn open_managed_durability(
    network_id: &str,
    node_id: &str,
    runner: &mut Runner,
) -> Option<crate::managed_durability::ManagedDurability> {
    let Some(root) = crate::managed_durability::configured_root() else {
        return None;
    };
    let warm_root = crate::managed_durability::configured_warm_root();
    let lease = match crate::managed_durability::configured_shard_lease(network_id, node_id) {
        Ok(lease) => lease,
        Err(error) => {
            nm_err!(
                "[error] Durable owner for network {} on node {} has no valid persisted lease: {}",
                network_id,
                node_id,
                error
            );
            return None;
        }
    };
    let term = lease
        .as_ref()
        .map(|lease| lease.term)
        .or_else(|| {
            std::env::var("NM_LEASE_TERM")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(LeaseTerm::INITIAL);
    let mut owner = match crate::managed_durability::ManagedDurability::open(
        root,
        network_id,
        node_id,
        runner,
        term,
        warm_root.as_deref(),
    ) {
        Ok(owner) => owner,
        Err(error) => {
            nm_err!(
                "[error] Durable owner for network {} on node {} could not open: {}",
                network_id,
                node_id,
                error
            );
            return None;
        }
    };
    if let Some(lease) = lease {
        if lease.fencing_token != lease.term.raw() {
            nm_err!(
                "[error] Durable owner for network {} received an invalid fencing token",
                network_id
            );
            return None;
        }
        owner.set_fencing_token(lease.fencing_token);
    }
    match crate::managed_durability::configured_replicated_authority() {
        Ok(Some((replicas, members))) => owner.bind_replicated_authority(replicas, members),
        Ok(None) => match crate::managed_durability::configured_authority() {
            Ok(Some((path, members))) => owner.bind_persisted_authority(path, members),
            Ok(None) => {}
            Err(error) => {
                nm_err!(
                    "[error] Durable owner for network {} has invalid authority configuration: {}",
                    network_id,
                    error
                );
                return None;
            }
        },
        Err(error) => {
            nm_err!(
                "[error] Durable owner for network {} has invalid authority configuration: {}",
                network_id,
                error
            );
            return None;
        }
    }
    {
        if let Ok(Some(snapshot)) = owner.recovered_snapshot() {
            if let Err(error) = runner.import_network_json(&snapshot) {
                nm_err!(
                    "[error] Durable owner for network {} recovered an invalid runner snapshot: {}",
                    network_id,
                    error
                );
                return None;
            }
        }
        Some(owner)
    }
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
    /// Explicitly registered partial stable-shard workers. Registration is
    /// an embedding seam: discovery and placement observations never create
    /// an executor or grant it writer authority.
    pub partial_shard_runtimes: HashMap<
        String,
        Arc<tokio::sync::Mutex<crate::managed_partial_shard_runtime::ManagedPartialShardRuntime>>,
    >,
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
    /// Serialises durable outbox reservation and acknowledgement of each
    /// causal batch. A sender must not append the same peer concurrently.
    causal_send_guard: std::sync::Arc<tokio::sync::Mutex<()>>,
    spike_transport_stats: HashMap<String, SpikeTransportStats>,
    #[cfg(feature = "openmpi")]
    mpi_receiver_started: bool,

    // Cluster-wide status (only relevant if is_orchestrator)
    pub nodes: HashMap<String, NodeStatus>,
    pub network_registry: HashMap<String, NetworkStatus>,
    /// Networks that have crossed into the stable-worker profile. This marker
    /// remains after a worker disappears so the legacy layer scheduler cannot
    /// silently reacquire the brain without an explicit migration transaction.
    pub stable_network_ids: HashSet<String>,
    pub network_snapshots: HashMap<String, String>,
    /// Monotonic control-plane epochs for asynchronous consistent cuts. This
    /// is coordination metadata, not biological time.
    pub consistent_cut_epochs: HashMap<String, u64>,
    pub network_runtime_metrics: HashMap<String, HashMap<String, NetworkResources>>,
    pub last_heartbeat: HashMap<String, std::time::Instant>,
    pub pending_commands: HashMap<String, Vec<NetworkCommand>>, // node_id -> commands
    /// Idempotently remembered activation results. A worker may replay a
    /// result when the heartbeat response was lost; retaining the bounded
    /// result lets the orchestrator acknowledge that replay without making
    /// the worker resurrect a completed command.
    pub stable_activation_results: HashMap<(String, String), NetworkCommandResult>,
    last_deployment_transition: HashMap<String, DeploymentTransitionRecord>,

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
    /// Node-owned management dispatch registry. Keeping this handle on the
    /// node makes live-runtime registration explicit and prevents `main` from
    /// accidentally creating a registry disconnected from worker state.
    migration_executor_registry: crate::migration_executor::MigrationExecutorRegistry,
    /// Node-scoped stable-shard data-plane receivers. The registry starts
    /// empty and only an explicit bootstrap or migration handoff can add a
    /// durable receiver; network discovery and placement telemetry never do.
    stable_shard_data_plane: crate::stable_shard_transport::StableShardDataPlaneService,
    /// Control-plane metadata for explicitly bootstrapped partial workers.
    /// These maps do not own executor state or grant writer authority.
    stable_worker_networks: Arc<std::sync::RwLock<BTreeMap<crate::deterministic::BrainId, String>>>,
    stable_worker_limits:
        Arc<std::sync::RwLock<BTreeMap<crate::deterministic::BrainId, (u32, u32)>>>,
    /// Idempotency records for orchestrator-issued worker activation
    /// commands. The durable receiver remains the authority after a
    /// successful activation; this short-lived map only prevents heartbeat
    /// retries from reopening the same worker in one process.
    stable_worker_operations: Arc<std::sync::RwLock<BTreeMap<String, (u64, String)>>>,
    tracey_probe: Option<TraceyStatusProbe>,
    /// Optional orchestrator-side audit hook. It is installed by the
    /// management adapter and receives only validated worker results; the
    /// worker never receives the callback or a registry handle.
    stable_activation_result_handler: Arc<std::sync::RwLock<Option<StableActivationResultHandler>>>,
    /// Optional orchestrator-side placement evidence hook. Registrations have
    /// already passed wire-shape, identity and committed-ack validation before
    /// this callback is scheduled outside the heartbeat lock.
    stable_worker_registration_handler:
        Arc<std::sync::RwLock<Option<StableWorkerRegistrationHandler>>>,
}

pub type StableActivationResultHandler = Arc<dyn Fn(NetworkCommandResult) + Send + Sync>;
pub type StableWorkerRegistrationHandler =
    Arc<dyn Fn(String, StableWorkerRegistration) + Send + Sync>;

impl DistributedNode {
    pub fn new(node_id: String, is_orchestrator: bool) -> Self {
        let stable_shard_data_plane =
            crate::stable_shard_transport::StableShardDataPlaneService::empty(node_id.clone());
        Self {
            state: Arc::new(RwLock::new(NodeState {
                node_id,
                networks: HashMap::new(),
                partial_shard_runtimes: HashMap::new(),
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
                causal_send_guard: std::sync::Arc::new(tokio::sync::Mutex::new(())),
                spike_transport_stats: HashMap::new(),
                #[cfg(feature = "openmpi")]
                mpi_receiver_started: false,
                nodes: HashMap::new(),
                network_registry: HashMap::new(),
                stable_network_ids: HashSet::new(),
                network_snapshots: HashMap::new(),
                consistent_cut_epochs: HashMap::new(),
                network_runtime_metrics: HashMap::new(),
                last_heartbeat: HashMap::new(),
                pending_commands: HashMap::new(),
                stable_activation_results: HashMap::new(),
                last_deployment_transition: HashMap::new(),
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
            migration_executor_registry:
                crate::migration_executor::MigrationExecutorRegistry::default(),
            stable_shard_data_plane,
            stable_worker_networks: Arc::new(std::sync::RwLock::new(BTreeMap::new())),
            stable_worker_limits: Arc::new(std::sync::RwLock::new(BTreeMap::new())),
            stable_worker_operations: Arc::new(std::sync::RwLock::new(BTreeMap::new())),
            tracey_probe: TraceyStatusProbe::from_env(),
            stable_activation_result_handler: Arc::new(std::sync::RwLock::new(None)),
            stable_worker_registration_handler: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Return the node-owned migration registry used by the secured
    /// orchestrator management service. Cloning the handle does not clone an
    /// executor or grant authority; it only shares the registry's bounded
    /// registration/in-flight state.
    pub fn migration_executor_registry(
        &self,
    ) -> crate::migration_executor::MigrationExecutorRegistry {
        self.migration_executor_registry.clone()
    }

    /// Register the stable runtime hosted by one managed network with the
    /// node-owned migration dispatcher.
    ///
    /// This is intentionally an explicit, asynchronous operation. A network
    /// name, discovery observation, placement proposal or capability report
    /// cannot create the registration. The target plan and source node are
    /// checked before the executor enters the registry.
    #[cfg(feature = "stable_executor_live")]
    pub async fn register_stable_network_migration_executor(
        &self,
        network_id: &str,
        mut settings: crate::migration_executor::StableExecutorMigrationSettings,
    ) -> Result<crate::deterministic::BrainId, String> {
        let network = {
            let state = self.state.read().await;
            if !state.is_orchestrator {
                return Err(
                    "live migration executors may only be registered by an orchestrator node"
                        .to_owned(),
                );
            }
            if settings.source_node != state.node_id {
                return Err(
                    "migration source node does not match the hosting orchestrator node".to_owned(),
                );
            }
            state
                .networks
                .get(network_id)
                .cloned()
                .ok_or_else(|| format!("managed network {network_id} is not registered"))?
        };
        let brain_id = {
            let network = network.read().await;
            let runtime = network
                .stable_executor
                .as_ref()
                .ok_or_else(|| "managed network has no stable executor".to_owned())?;
            let brain_id = runtime.bridge().authority().brain_id();
            if settings.target_plan.brain_id != brain_id {
                return Err(
                    "migration target plan brain identity does not match the live runtime"
                        .to_owned(),
                );
            }
            brain_id
        };
        if !settings.destination_endpoints.is_empty() && settings.activation_gate.is_none() {
            settings.activation_gate = Some(self.stable_migration_activation_gate(
                Duration::from_secs(120),
                Duration::from_millis(100),
            ));
        }
        let executor = Arc::new(ManagedStableNetworkMigrationExecutor::new(
            network, settings,
        )?);
        self.migration_executor_registry
            .register(brain_id, executor)
            .map_err(|error| error.to_string())?;
        Ok(brain_id)
    }

    /// Build the default remote-migration activation barrier for this
    /// orchestrator. The migration registry invokes its synchronous executor
    /// from `spawn_blocking`; this adapter uses the node's authenticated
    /// heartbeat/session state and never opens a worker connection itself.
    ///
    /// A successful return means every target command was queued through the
    /// enrolled-node path, acknowledged by its digest-bound command result,
    /// and followed by a validated authoritative registration whose committed
    /// shard acknowledgements exactly cover that worker's active ownership.
    /// Placement publication remains the caller's next step, so any timeout or
    /// mismatch fails before destination authority is published.
    pub fn stable_migration_activation_gate(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> crate::migration_executor::StableMigrationActivationGate {
        let node = self.clone();
        let timeout = timeout.max(Duration::from_millis(1));
        let poll_interval = poll_interval.max(Duration::from_millis(1));
        Arc::new(move |request| {
            let node = node.clone();
            let future = async move {
                node.await_stable_migration_activation(request, timeout, poll_interval)
                    .await
            };
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.block_on(future)
            } else {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| {
                        format!("stable migration activation runtime creation failed: {error}")
                    })?
                    .block_on(future)
            }
        })
    }

    async fn await_stable_migration_activation(
        &self,
        request: crate::migration_executor::StableMigrationActivationRequest,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<(), String> {
        request
            .target_plan
            .verify()
            .map_err(|error| format!("stable migration target placement is invalid: {error}"))?;
        let expected_nodes = request
            .target_plan
            .placements
            .iter()
            .map(|placement| placement.active_node.clone())
            .collect::<BTreeSet<_>>();
        let expected_shards = request
            .target_plan
            .placements
            .iter()
            .map(|placement| placement.shard_id.raw())
            .collect::<Vec<_>>();
        if expected_nodes.is_empty()
            || request
                .checkpoint_references
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != expected_nodes
            || request
                .activation_commands
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != expected_nodes
        {
            return Err(
                "stable migration activation targets do not match the target placement".to_owned(),
            );
        }

        let mut network_id: Option<String> = None;
        for (node_id, command) in &request.activation_commands {
            command
                .verify()
                .map_err(|error| format!("stable worker activation is invalid: {error}"))?;
            if command.operation_id != request.operation_id
                || command.brain_id != request.brain_id.raw()
                || command.target_node != *node_id
            {
                return Err(format!(
                    "stable worker activation for {node_id} is not bound to this migration"
                ));
            }
            if command.checkpoint_transfer.as_ref() != request.checkpoint_references.get(node_id) {
                return Err(format!(
                    "stable worker activation for {node_id} is not bound to its transferred checkpoint"
                ));
            }
            match &network_id {
                Some(expected) if expected != &command.network_id => {
                    return Err(
                        "stable migration activation commands name different networks".to_owned(),
                    );
                }
                None => network_id = Some(command.network_id.clone()),
                _ => {}
            }
        }
        let network_id = network_id
            .ok_or_else(|| "stable migration activation command set is empty".to_owned())?;

        let queue_results = futures_util::future::join_all(
            request
                .activation_commands
                .values()
                .cloned()
                .map(|command| async move { self.queue_stable_worker_activation(command).await }),
        )
        .await;
        for result in queue_results {
            result.map_err(|error| format!("stable worker activation queue failed: {error}"))?;
        }

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let status = {
                let state = self.state.read().await;
                let mut failures = Vec::new();
                let mut complete = true;
                for node_id in &expected_nodes {
                    let command = &request.activation_commands[node_id];
                    let result = state
                        .stable_activation_results
                        .get(&(node_id.clone(), command.request_id.clone()));
                    match result {
                        Some(result) if !result.accepted => failures.push(format!(
                            "target {node_id} rejected activation: {}",
                            result.error
                        )),
                        Some(result)
                            if result.network_id == command.network_id
                                && result.request_id == command.request_id
                                && result.manifest_digest == command.manifest_digest
                                && result.brain_id == request.brain_id.raw() => {}
                        Some(_) => failures.push(format!(
                            "target {node_id} returned an activation result with mismatched identity"
                        )),
                        None => complete = false,
                    }

                    let registration = state.nodes.get(node_id).and_then(|node| {
                        node.stable_executors.iter().find(|registration| {
                            registration.network_id == network_id
                                && registration.brain_id == request.brain_id.raw()
                        })
                    });
                    let Some(registration) = registration else {
                        complete = false;
                        continue;
                    };
                    let registration = stable_registration_from_proto(registration);
                    if let Err(error) = registration.validate() {
                        failures.push(format!("target {node_id} registration is invalid: {error}"));
                        continue;
                    }
                    let expected_owned = request
                        .target_plan
                        .placements
                        .iter()
                        .filter(|placement| placement.active_node == *node_id)
                        .map(|placement| placement.shard_id.raw())
                        .collect::<Vec<_>>();
                    if registration.shard_ids != expected_shards
                        || registration.owned_shard_ids != expected_owned
                        || registration.topology_generation
                            != request.target_plan.topology_generation.raw()
                        || registration.partition_generation
                            != request.target_plan.partition_generation.raw()
                        || registration.lease_term != request.target_plan.lease_term.raw()
                        || registration.fencing_token != request.target_plan.fencing_token
                    {
                        failures.push(format!(
                            "target {node_id} registration does not match the target placement"
                        ));
                    }
                }
                if !failures.is_empty() {
                    Err(failures.join("; "))
                } else if complete {
                    Ok(())
                } else {
                    Err(String::new())
                }
            };
            match status {
                Ok(()) => return Ok(()),
                Err(error) if !error.is_empty() => return Err(error),
                Err(_) if tokio::time::Instant::now() >= deadline => {
                    return Err(
                        "stable migration activation timed out waiting for target evidence"
                            .to_owned(),
                    );
                }
                Err(_) => {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    tokio::time::sleep(poll_interval.min(remaining)).await;
                }
            }
        }
    }

    /// Install the management-plane outcome hook without exposing any
    /// placement-registry object to a worker or data-plane caller.
    pub fn set_stable_activation_result_handler(
        &self,
        handler: Option<StableActivationResultHandler>,
    ) -> Result<(), String> {
        *self
            .stable_activation_result_handler
            .write()
            .map_err(|_| "stable activation result handler lock is poisoned".to_owned())? = handler;
        Ok(())
    }

    /// Install the management callback that consumes validated stable-worker
    /// registration evidence. It is kept separate from command-result
    /// handling because command acceptance only proves delivery, while the
    /// registration proves durable activation and shard ownership.
    pub fn set_stable_worker_registration_handler(
        &self,
        handler: Option<StableWorkerRegistrationHandler>,
    ) -> Result<(), String> {
        *self
            .stable_worker_registration_handler
            .write()
            .map_err(|_| "stable worker registration handler lock is poisoned".to_owned())? =
            handler;
        Ok(())
    }

    /// Return the stable-shard service used by the node's gRPC listener.
    /// Cloning the service only clones its registry handles; receiver state
    /// remains shared and serialised per brain.
    pub fn stable_shard_data_plane_service(
        &self,
    ) -> crate::stable_shard_transport::StableShardDataPlaneService {
        self.stable_shard_data_plane.clone()
    }

    /// Register one durable receiver after the caller has completed the
    /// checkpoint, placement, lease and authenticated-session checks.
    pub fn register_stable_shard_receiver(
        &self,
        receiver: crate::stable_shard_transport::DurableStableShardReceiver,
    ) -> Result<(), String> {
        self.stable_shard_data_plane
            .registry()
            .register(receiver)
            .map_err(|error| error.to_string())
    }

    /// Register a receiver and bind its stable brain identity to the
    /// deployment's network name and bounded heartbeat budgets. The binding
    /// is telemetry metadata; all data-plane admission remains fenced by the
    /// receiver's durable plan and lease.
    pub fn register_stable_shard_receiver_for_network(
        &self,
        network_id: impl Into<String>,
        max_input_events: usize,
        max_steps_per_poll: usize,
        receiver: crate::stable_shard_transport::DurableStableShardReceiver,
    ) -> Result<(), String> {
        let network_id = network_id.into();
        let brain_id = receiver.brain_id();
        let limits = (
            u32::try_from(max_input_events)
                .map_err(|_| "stable worker input budget exceeds wire bounds".to_owned())?,
            u32::try_from(max_steps_per_poll)
                .map_err(|_| "stable worker step budget exceeds wire bounds".to_owned())?,
        );
        if network_id.trim().is_empty() || limits.0 == 0 || limits.1 == 0 {
            return Err("stable worker network identity or budgets are invalid".to_owned());
        }
        self.register_stable_shard_receiver(receiver)?;
        let result = (|| {
            let mut networks = self
                .stable_worker_networks
                .write()
                .map_err(|_| "stable worker network metadata lock is poisoned".to_owned())?;
            if networks.insert(brain_id, network_id).is_some() {
                return Err("stable worker network identity is already registered".to_owned());
            }
            self.stable_worker_limits
                .write()
                .map_err(|_| "stable worker limit metadata lock is poisoned".to_owned())?
                .insert(brain_id, limits);
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            let _ = self.unregister_stable_shard_receiver(brain_id);
            if let Ok(mut networks) = self.stable_worker_networks.write() {
                networks.remove(&brain_id);
            }
            if let Ok(mut limits_map) = self.stable_worker_limits.write() {
                limits_map.remove(&brain_id);
            }
            return Err(error);
        }
        Ok(())
    }

    /// Remove a receiver only after the migration adapter has drained and
    /// fenced its source. A missing receiver is reported as a normal no-op so
    /// retry cleanup remains idempotent.
    pub fn unregister_stable_shard_receiver(
        &self,
        brain_id: crate::deterministic::BrainId,
    ) -> Result<bool, String> {
        let removed = self
            .stable_shard_data_plane
            .registry()
            .unregister(brain_id)
            .map_err(|error| error.to_string())?;
        if removed {
            if let Ok(mut networks) = self.stable_worker_networks.write() {
                networks.remove(&brain_id);
            }
            if let Ok(mut limits) = self.stable_worker_limits.write() {
                limits.remove(&brain_id);
            }
        }
        Ok(removed)
    }

    /// Bind the durable outbound queue used for generated work from a
    /// registered stable receiver. This is a separate explicit operation so
    /// receiver registration cannot accidentally grant a routing authority.
    pub fn register_stable_shard_dispatcher(
        &self,
        brain_id: crate::deterministic::BrainId,
        dispatcher: crate::stable_shard_dispatch::StableShardDispatcher,
    ) -> Result<(), String> {
        self.stable_shard_data_plane
            .register_dispatcher(brain_id, dispatcher)
            .map_err(|error| error.to_string())
    }

    pub fn unregister_stable_shard_dispatcher(
        &self,
        brain_id: crate::deterministic::BrainId,
    ) -> Result<bool, String> {
        self.stable_shard_data_plane
            .unregister_dispatcher(brain_id)
            .map_err(|error| error.to_string())
    }

    /// Queue an orchestrator-authorised stable-worker activation for one
    /// already enrolled node. Discovery, a peer address, or ordinary resource
    /// telemetry is insufficient: the target must have reported the stable
    /// executor capability for this network and brain, and it must have an
    /// active authenticated peer session. Replays of the same command are
    /// acknowledged without adding another heartbeat command; conflicting
    /// commands for the same network remain visible and are rejected.
    pub async fn queue_stable_worker_activation(
        &self,
        command: crate::stable_worker::StableWorkerActivationCommand,
    ) -> Result<bool, String> {
        command.verify().map_err(|error| error.to_string())?;
        let mut state = self.state.write().await;
        if !state.is_orchestrator {
            return Err(
                "stable worker activation can only be queued by an orchestrator".to_owned(),
            );
        }
        let target = state
            .nodes
            .get(&command.target_node)
            .ok_or_else(|| "stable worker activation target is not an enrolled node".to_owned())?;
        let capability = target
            .stable_executor_capabilities
            .iter()
            .any(|capability| {
                capability.profile == crate::stable_worker::STABLE_EXECUTOR_PROFILE
                    && capability.activation_schema_version
                        == crate::stable_worker::STABLE_WORKER_ACTIVATION_SCHEMA_VERSION
            });
        if !capability {
            return Err(
                "stable worker activation target has no enrolled stable-worker activation capability"
                    .to_owned(),
            );
        }
        let session_active = state
            .last_heartbeat
            .get(&command.target_node)
            .is_some_and(|last| last.elapsed() <= PEER_STALE_AFTER);
        if !session_active {
            return Err("stable worker activation target has no active peer session".to_owned());
        }
        let address = state
            .peers
            .get(&command.target_node)
            .or_else(|| (!target.address.trim().is_empty()).then_some(&target.address))
            .filter(|address| !address.trim().is_empty())
            .cloned()
            .ok_or_else(|| "stable worker activation target has no peer address".to_owned())?;
        let config_json = serde_json::to_vec(&command)
            .map_err(|error| format!("stable worker activation encoding failed: {error}"))?;
        let network_id = command.network_id.clone();
        let queue = state
            .pending_commands
            .entry(command.target_node.clone())
            .or_default();
        if let Some(existing) = queue.iter().find(|existing| {
            existing.network_id == network_id
                && existing.r#type
                    == proto::network_command::CommandType::ActivateStableWorker as i32
        }) {
            if existing.config_json == config_json {
                return Ok(false);
            }
            return Err(
                "a different stable worker activation is already queued for this network"
                    .to_owned(),
            );
        }
        let pending = NetworkCommand {
            r#type: proto::network_command::CommandType::ActivateStableWorker as i32,
            network_id,
            config_json,
            layers: Vec::new(),
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 0,
            neuron_model: String::new(),
            learning_rule: String::new(),
        };
        // The address is intentionally read and validated above. Keep the
        // binding in this method so a future transport adapter cannot enqueue
        // a command for a node that has silently lost its session.
        if address.trim().is_empty() {
            return Err("stable worker activation target peer address is empty".to_owned());
        }
        if queue.len() >= MAX_PENDING_COMMANDS_PER_NODE {
            return Err("stable worker activation queue capacity exceeded".to_owned());
        }
        queue.push(pending);
        Ok(true)
    }

    /// Activate a partial stable worker from an orchestrator-issued command.
    ///
    /// Manifest decoding, checkpoint verification and filesystem work run on
    /// a blocking worker so heartbeat command handling remains responsive.
    /// Registration happens only after the complete manifest has been
    /// verified, and a dispatcher failure rolls the receiver registration
    /// back. Replaying the same request is idempotent; reusing its request ID
    /// with different content is rejected.
    #[cfg(feature = "stable_executor_live")]
    pub async fn activate_stable_worker(
        &self,
        command: crate::stable_worker::StableWorkerActivationCommand,
    ) -> Result<(), String> {
        let expected_node_id = self.state.read().await.node_id.clone();
        let transfer_root = crate::checkpoint_transfer::StableCheckpointTransferService::from_env(
            expected_node_id.clone(),
        )?
        .root()
        .to_path_buf();
        let worker_state_root =
            crate::checkpoint_transfer::StableCheckpointTransferService::worker_state_root(
                &expected_node_id,
            )?;
        self.activate_stable_worker_with_roots(command, transfer_root, worker_state_root)
            .await
    }

    /// Activate a worker using explicit target-local durable roots.
    ///
    /// The normal process entry point obtains these roots from deployment
    /// configuration. Keeping the roots injectable makes the same activation
    /// boundary usable by embedded orchestrators and deterministic in-process
    /// fault tests without mutating process-global environment variables.
    #[cfg(feature = "stable_executor_live")]
    pub async fn activate_stable_worker_with_roots(
        &self,
        command: crate::stable_worker::StableWorkerActivationCommand,
        checkpoint_transfer_root: impl Into<std::path::PathBuf>,
        worker_state_root: impl Into<std::path::PathBuf>,
    ) -> Result<(), String> {
        command.verify().map_err(|error| error.to_string())?;
        let expected_node_id = self.state.read().await.node_id.clone();
        if command.target_node != expected_node_id {
            return Err(format!(
                "stable worker activation targets {}, local node is {}",
                command.target_node, expected_node_id
            ));
        }
        if let Some((brain_id, digest)) = self
            .stable_worker_operations
            .read()
            .map_err(|_| "stable worker operation lock is poisoned".to_owned())?
            .get(&command.request_id)
            .cloned()
        {
            if brain_id == command.brain_id && digest == command.manifest_digest {
                return Ok(());
            }
            return Err(
                "stable worker activation request ID was reused with different content".to_owned(),
            );
        }

        let expected_node_for_bootstrap = expected_node_id.clone();
        let manifest_json = command.manifest_json.clone();
        let expected_brain_id = command.brain_id;
        let checkpoint_transfer_root = checkpoint_transfer_root.into();
        let worker_state_root = worker_state_root.into();
        let bootstrap = tokio::task::spawn_blocking(move || {
            let mut manifest = serde_json::from_str::<
                crate::stable_runtime_bootstrap::StablePartialWorkerBootstrapManifest,
            >(&manifest_json)
            .map_err(|error| format!("stable worker manifest JSON is invalid: {error}"))?;
            if manifest.node_id != expected_node_for_bootstrap {
                return Err(format!(
                    "stable worker manifest targets {}, local node is {}",
                    manifest.node_id, expected_node_for_bootstrap
                ));
            }
            if manifest.runtime.brain_id.raw() != expected_brain_id
                || manifest.placement.brain_id.raw() != expected_brain_id
            {
                return Err(
                    "stable worker activation brain identity does not match manifest".to_owned(),
                );
            }
            if let Some(reference) = command.checkpoint_transfer.as_ref() {
                let transfer_service =
                    crate::checkpoint_transfer::StableCheckpointTransferService::new(
                        expected_node_for_bootstrap.clone(),
                        checkpoint_transfer_root,
                    )
                    .map_err(|error| {
                        format!("checkpoint transfer service configuration failed: {error}")
                    })?;
                transfer_service
                    .verify_activation_reference(reference)
                    .map_err(|error| {
                        format!("target-local checkpoint transfer verification failed: {error}")
                    })?;
                manifest
                    .rebase_to_transferred_checkpoint(
                        transfer_service.root().to_path_buf(),
                        worker_state_root,
                        reference,
                    )
                    .map_err(|error| {
                        format!("target-local worker path rebasing failed: {error}")
                    })?;
            }
            manifest
                .open()
                .map_err(|error| format!("stable worker manifest verification failed: {error}"))
        })
        .await
        .map_err(|error| format!("stable worker bootstrap task failed: {error}"))??;

        let brain_id = bootstrap.manifest.runtime.brain_id;
        let network_id = command.network_id.clone();
        let max_input_events = bootstrap.manifest.runtime.max_input_events;
        let max_steps_per_poll = bootstrap.manifest.runtime.max_steps_per_poll;
        self.register_stable_shard_receiver_for_network(
            network_id,
            max_input_events,
            max_steps_per_poll,
            bootstrap.receiver,
        )?;
        if let Err(error) = self.register_stable_shard_dispatcher(brain_id, bootstrap.dispatcher) {
            let _ = self.unregister_stable_shard_receiver(brain_id);
            return Err(error);
        }
        self.stable_worker_operations
            .write()
            .map_err(|_| "stable worker operation lock is poisoned".to_owned())?
            .insert(
                command.request_id,
                (command.brain_id, command.manifest_digest),
            );
        nm_log!(
            "[stable-worker] activated brain {} on node {} from orchestrator command {}",
            brain_id,
            expected_node_id,
            command.operation_id
        );
        Ok(())
    }

    /// Flush sealed stable-shard outbox records in parallel by destination.
    /// A failed destination remains durable and is retried on the next pass.
    pub async fn flush_stable_shard_outboxes(&self) -> Result<usize, String> {
        self.stable_shard_data_plane
            .dispatch_pending()
            .await
            .map_err(|error| error.to_string())
    }

    /// Register one bounded partial-shard worker for an explicitly selected
    /// network. The caller must have already validated placement, checkpoint
    /// identity and authority; this method only makes the worker reachable by
    /// the node's bounded execution adapter.
    pub async fn register_partial_shard_runtime(
        &self,
        network_id: impl Into<String>,
        runtime: crate::managed_partial_shard_runtime::ManagedPartialShardRuntime,
    ) -> Result<(), String> {
        let network_id = network_id.into();
        if network_id.trim().is_empty() {
            return Err("partial-shard runtime network identity is empty".to_owned());
        }
        let mut state = self.state.write().await;
        if state.partial_shard_runtimes.contains_key(&network_id) {
            return Err(format!(
                "partial-shard runtime for network {network_id} is already registered"
            ));
        }
        state
            .partial_shard_runtimes
            .insert(network_id, Arc::new(tokio::sync::Mutex::new(runtime)));
        Ok(())
    }

    /// Add a data-plane endpoint to an already registered partial worker.
    /// Endpoint registration is deliberately separate from runtime
    /// registration so discovery cannot silently turn a peer address into a
    /// routing grant.
    pub async fn register_partial_shard_endpoint(
        &self,
        network_id: &str,
        node_id: impl Into<String>,
        address: impl Into<String>,
    ) -> Result<(), String> {
        let runtime = {
            let state = self.state.read().await;
            state.partial_shard_runtimes.get(network_id).cloned()
        }
        .ok_or_else(|| {
            format!("partial-shard runtime for network {network_id} is not registered")
        })?;
        let runtime = runtime.lock().await;
        runtime
            .dispatcher()
            .register_endpoint(node_id, address)
            .map_err(|error| error.to_string())
    }

    /// Remove a partial worker only after its owner has been drained by the
    /// caller. The explicit removal boundary prevents node disappearance or
    /// discovery expiry from silently discarding a live worker handle.
    pub async fn unregister_partial_shard_runtime(&self, network_id: &str) -> Result<bool, String> {
        let mut state = self.state.write().await;
        Ok(state.partial_shard_runtimes.remove(network_id).is_some())
    }

    /// Poll a registered partial worker through its bounded asynchronous loop.
    /// The node state lock is released before biological work or network I/O;
    /// only the worker handle remains serialised.
    pub async fn poll_partial_shard_runtime(
        &self,
        network_id: &str,
        inputs: &[crate::shard_executor::RoutedCausalEvent],
    ) -> Result<crate::managed_partial_shard_runtime::ManagedPartialShardPoll, String> {
        let runtime = {
            let state = self.state.read().await;
            state.partial_shard_runtimes.get(network_id).cloned()
        }
        .ok_or_else(|| {
            format!("partial-shard runtime for network {network_id} is not registered")
        })?;
        runtime
            .lock()
            .await
            .poll(inputs)
            .await
            .map_err(|error| error.to_string())
    }

    /// Flush pending partial-worker streams without holding the node state
    /// lock. Failed destinations remain durable and are retried by the next
    /// explicit call or by the enclosing scheduler.
    pub async fn dispatch_partial_shard_runtime(
        &self,
        network_id: &str,
    ) -> Result<crate::stable_shard_dispatch::StableShardDispatchReport, String> {
        let runtime = {
            let state = self.state.read().await;
            state.partial_shard_runtimes.get(network_id).cloned()
        }
        .ok_or_else(|| {
            format!("partial-shard runtime for network {network_id} is not registered")
        })?;
        // The dispatcher contains its own shared outbox and endpoint handles.
        // Snapshot it under the worker lock, then release the lock before any
        // network await so a slow destination cannot stall local execution.
        let dispatcher = runtime.lock().await.dispatcher();
        dispatcher
            .dispatch_pending()
            .await
            .map_err(|error| error.to_string())
    }

    /// Service every explicitly registered partial stable worker once.
    ///
    /// Each worker is independently serialised, while workers belonging to
    /// different networks are polled and flushed concurrently. The biological
    /// poll is bounded by the runtime's configured step budget. Durable
    /// outboxes are flushed even when a poll fails, so a transient local error
    /// cannot strand records that were committed by an earlier poll.
    pub async fn service_partial_shard_runtimes_once(&self) -> usize {
        let runtimes = {
            let state = self.state.read().await;
            state
                .partial_shard_runtimes
                .iter()
                .map(|(network_id, runtime)| (network_id.clone(), runtime.clone()))
                .collect::<Vec<_>>()
        };
        let results = futures_util::future::join_all(runtimes.into_iter().map(
            |(network_id, runtime)| async move {
                let (poll_result, dispatcher) = {
                    let mut runtime = runtime.lock().await;
                    let poll_result = runtime.poll(&[]).await;
                    let dispatcher = runtime.dispatcher();
                    (poll_result, dispatcher)
                };
                if let Err(error) = poll_result {
                    nm_err!(
                        "[warn] partial stable worker {} poll deferred: {}",
                        network_id,
                        error
                    );
                }
                match dispatcher.dispatch_pending().await {
                    Ok(report) => Ok(report.acknowledged_records),
                    Err(error) => {
                        nm_err!(
                            "[warn] partial stable worker {} outbox flush deferred: {}",
                            network_id,
                            error
                        );
                        Err(error)
                    }
                }
            },
        ))
        .await;
        results.into_iter().filter_map(Result::ok).sum()
    }

    /// Run the bounded partial-worker lifecycle until the node is asked to
    /// stop. The interval is operational scheduling metadata and never enters
    /// biological logical time.
    pub async fn run_partial_shard_workers(&self, mut shutdown: watch::Receiver<bool>) {
        let interval_ms = std::env::var("NM_STABLE_PARTIAL_WORKER_INTERVAL_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(100)
            .clamp(10, 10_000);
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
                _ = interval.tick() => {
                    let _ = self.service_partial_shard_runtimes_once().await;
                }
            }
        }
    }

    /// Execute a generated management operation through the orchestrator's
    /// existing fenced network-update path. Workers are never addressed here;
    /// the update path enqueues commands for assigned workers and applies the
    /// local command only when this process is the orchestrator.
    pub async fn execute_management_operation(
        &self,
        network_id: &str,
        operation: crate::management::OperationKind,
    ) -> Result<(), String> {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let action = match operation {
            crate::management::OperationKind::Start => proto::control_update::Action::Start,
            crate::management::OperationKind::Stop => proto::control_update::Action::Stop,
            crate::management::OperationKind::Reset => proto::control_update::Action::Reset,
            crate::management::OperationKind::Export => {
                return Err("export requires an explicit export destination".to_owned());
            }
        };
        self.update_network(Request::new(NetworkUpdateRequest {
            network_id: network_id.to_owned(),
            update: Some(proto::network_update_request::Update::Control(
                proto::ControlUpdate {
                    action: action as i32,
                },
            )),
        }))
        .await
        .map(|_| ())
        .map_err(|status| status.to_string())
    }

    #[cfg(feature = "replicated_durability")]
    async fn admit_causal_spike_ingress(
        &self,
        envelope: &crate::causal_transport::proto::CausalEventEnvelope,
        causal: crate::data_plane::CausalEnvelope,
        ingress: CausalSpikeIngress,
    ) -> Result<(), Status> {
        if ingress.schema_version != CAUSAL_INGRESS_SCHEMA_VERSION
            || ingress.network_id.trim().is_empty()
            || ingress.spike_indices.len() > MAX_CAUSAL_INGRESS_SPIKES
            || ingress.aer_payload.len() > cluster_snapshot::MAX_SHARD_SNAPSHOT_BYTES
        {
            return Err(Status::invalid_argument("invalid causal ingress payload"));
        }
        if causal.brain != crate::managed_durability::managed_brain_id(&ingress.network_id) {
            return Err(Status::failed_precondition(
                "causal brain identity does not match ingress network",
            ));
        }

        let network = {
            let state = self.state.read().await;
            state.networks.get(&ingress.network_id).cloned()
        }
        .ok_or_else(|| Status::not_found("causal ingress network is not loaded"))?;

        let mut net = network.write().await;
        if causal.stage != crate::deterministic::EventStage::SpikeDecision
            || causal.kind != crate::data_plane::EnvelopeKind::Event
        {
            return Err(Status::invalid_argument(
                "causal ingress must be a spike-decision event",
            ));
        }

        let sensory_neurons = net.runner.net.num_sensory_neurons;
        let layer = (ingress.layer_index != EXTERNAL_SENSORY_LAYER_INDEX)
            .then_some(ingress.layer_index as usize);
        let layer_size = layer.map(|index| net.runner.layer_size(index)).unwrap_or(0);
        let layer_assigned = layer
            .map(|index| net.runner.is_layer_assigned(index))
            .unwrap_or(false);
        let owner = net
            .durable_owner
            .as_mut()
            .ok_or_else(|| Status::failed_precondition("network has no durable shard owner"))?;

        let mut channel: ManagedChannelState = serde_json::from_str(
            &owner
                .authoritative_channel_state()
                .map_err(|error| Status::failed_precondition(error.to_string()))?,
        )
        .map_err(|error| {
            Status::failed_precondition(format!("invalid durable channel state: {error}"))
        })?;

        let expected_size = if ingress.layer_index == EXTERNAL_SENSORY_LAYER_INDEX {
            sensory_neurons
        } else {
            layer_size
        };
        let spikes = match spikes_from_transport(
            &ingress.aer_payload,
            ingress.aer_base,
            &ingress.spike_indices,
            expected_size,
        ) {
            Ok(spikes) => spikes,
            Err(_error) if ingress.spike_indices.is_empty() => vec![0i8; expected_size],
            Err(error) => {
                return Err(Status::invalid_argument(format!(
                    "invalid causal AER payload: {error}"
                )));
            }
        };

        if ingress.layer_index == EXTERNAL_SENSORY_LAYER_INDEX {
            if spikes.len() != sensory_neurons {
                return Err(Status::invalid_argument(
                    "sensory ingress shape does not match the shard",
                ));
            }
            channel.external_sensory_spikes = Some(spikes);
        } else {
            if layer_size == 0 || layer_assigned {
                return Err(Status::failed_precondition(
                    "causal ingress is not addressed to a remote-owned layer",
                ));
            }
            if spikes.len() != layer_size {
                return Err(Status::invalid_argument(
                    "causal ingress shape does not match the layer",
                ));
            }
            let steps = if ingress.is_backward {
                &mut channel.remote_spike_steps_bwd
            } else {
                &mut channel.remote_spike_steps_fwd
            };
            if steps
                .get(&ingress.layer_index)
                .is_some_and(|previous| ingress.step_index < *previous)
            {
                return Err(Status::failed_precondition(
                    "causal ingress moves the layer frontier backwards",
                ));
            }
            steps.insert(ingress.layer_index, ingress.step_index);
            if ingress.is_backward {
                channel
                    .remote_spikes_bwd
                    .insert(ingress.layer_index, spikes.clone());
            } else {
                channel
                    .remote_spikes_fwd
                    .insert(ingress.layer_index, spikes.clone());
            }
        }

        let channel_json = serde_json::to_vec(&channel)
            .map_err(|error| Status::internal(format!("encode causal channel state: {error}")))?;
        let outcome = owner
            .admit_causal_event(&causal, &channel_json)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if matches!(
            outcome,
            crate::durability::DurableApplyOutcome::Applied { .. }
        ) {
            restore_channel_state(&mut net, channel);
        }
        let _ = envelope;
        Ok(())
    }

    #[cfg(not(feature = "replicated_durability"))]
    async fn admit_causal_spike_ingress(
        &self,
        _envelope: &crate::causal_transport::proto::CausalEventEnvelope,
        _causal: crate::data_plane::CausalEnvelope,
        _ingress: CausalSpikeIngress,
    ) -> Result<(), Status> {
        Err(Status::failed_precondition(
            "authoritative causal transport requires replicated_durability",
        ))
    }

    #[cfg(feature = "stable_executor_live")]
    async fn stable_network_for_causal_stream(
        &self,
        brain: crate::deterministic::BrainId,
        stream: crate::deterministic::StreamId,
        sender_node_id: &str,
        receiver_node_id: &str,
    ) -> Option<Arc<RwLock<ManagedNetwork>>> {
        let networks = {
            let state = self.state.read().await;
            state
                .networks
                .iter()
                .map(|(network_id, network)| (network_id.clone(), network.clone()))
                .collect::<Vec<_>>()
        };
        for (network_id, network) in networks {
            let stream_matches = crate::managed_durability::managed_link_stream_id(
                &network_id,
                sender_node_id,
                receiver_node_id,
            ) == stream;
            let brain_matches = crate::managed_durability::managed_brain_id(&network_id) == brain;
            if !stream_matches && !brain_matches {
                continue;
            }
            if network.read().await.stable_executor_registered() {
                return Some(network);
            }
        }
        None
    }

    #[cfg(feature = "stable_executor_live")]
    async fn admit_stable_causal_event(
        &self,
        network: Arc<RwLock<ManagedNetwork>>,
        causal: crate::data_plane::CausalEnvelope,
    ) -> Result<(), Status> {
        tokio::task::spawn_blocking(move || {
            let mut network = network.blocking_write();
            network
                .poll_stable_executor_envelope(&causal)
                .map(|_| ())
                .map_err(|error| Status::failed_precondition(error.to_string()))
        })
        .await
        .map_err(|error| Status::internal(format!("stable causal worker failed: {error}")))?
    }

    async fn fetch_live_network_snapshot(&self, source: &LiveSnapshotSource) -> Option<String> {
        if source.local {
            let net_arc = {
                let state = self.state.read().await;
                state.networks.get(&source.network_id).cloned()
            };
            if let Some(net_arc) = net_arc {
                match tokio::task::spawn_blocking(move || {
                    let net = net_arc.blocking_read();
                    local_shard_snapshot(&net).map(|(snapshot_json, _, _, _)| snapshot_json)
                })
                .await
                {
                    Ok(Ok(snapshot_json)) => return Some(snapshot_json),
                    Ok(Err(err)) => nm_err!(
                        "[warn] Failed to export live snapshot for network {}: {}",
                        source.network_id,
                        err
                    ),
                    Err(err) => nm_err!(
                        "[warn] Snapshot export task failed for network {}: {}",
                        source.network_id,
                        err
                    ),
                }
            }
        }

        let mut client = {
            let state = self.state.read().await;
            source
                .primary_node_id
                .as_ref()
                .and_then(|node_id| state.clients.get(node_id).cloned())
        };

        if client.is_none() {
            let addr = source.peer_addr.as_deref()?;
            match connect_peer_with_timeout(addr, Duration::from_millis(750)).await {
                Ok(connected) => {
                    client = Some(connected);
                }
                Err(err) => {
                    nm_err!(
                        "[warn] Failed to connect to live snapshot source for network {} at {}: {}",
                        source.network_id,
                        addr,
                        err
                    );
                    return None;
                }
            }
        }

        let mut client = client?;
        let sender_node_id = self.state.read().await.node_id.clone();
        let request = match authenticated_request(
            NetworkSnapshotRequest {
                network_id: source.network_id.clone(),
                cut_epoch: 0,
            },
            &sender_node_id,
        ) {
            Ok(request) => request,
            Err(error) => {
                nm_err!(
                    "[warn] Cannot authenticate live snapshot request for network {}: {}",
                    source.network_id,
                    error
                );
                return None;
            }
        };
        let snapshot_result =
            tokio::time::timeout(Duration::from_secs(2), client.get_network_snapshot(request))
                .await;

        match snapshot_result {
            Ok(Ok(response)) => {
                if let Some(node_id) = source.primary_node_id.as_ref() {
                    let mut state = self.state.write().await;
                    state.clients.insert(node_id.clone(), client);
                }
                Some(response.into_inner().snapshot_json)
            }
            Ok(Err(err)) => {
                nm_err!(
                    "[warn] Live snapshot RPC failed for network {}{}: {}",
                    source.network_id,
                    source
                        .primary_node_id
                        .as_deref()
                        .map(|node_id| format!(" on {}", node_id))
                        .unwrap_or_default(),
                    err
                );
                None
            }
            Err(_) => {
                nm_err!(
                    "[warn] Live snapshot RPC timed out for network {}{}",
                    source.network_id,
                    source
                        .primary_node_id
                        .as_deref()
                        .map(|node_id| format!(" on {}", node_id))
                        .unwrap_or_default()
                );
                None
            }
        }
    }

    async fn resolve_autonomous_transition_payloads(
        &self,
        plans: &[AutonomousTransitionPlan],
    ) -> HashMap<String, String> {
        let mut join_set = tokio::task::JoinSet::new();
        for plan in plans.iter().cloned() {
            let node = self.clone();
            join_set.spawn(async move {
                let refreshed_payload = if let Some(source) = plan.snapshot_source.as_ref() {
                    node.fetch_live_network_snapshot(source)
                        .await
                        .and_then(|snapshot_json| {
                            payload_with_updated_deployment(&snapshot_json, &plan.next_deployment)
                        })
                } else {
                    None
                };
                let fallback_payload =
                    payload_with_updated_deployment(&plan.fallback_payload, &plan.next_deployment)
                        .unwrap_or_else(|| plan.fallback_payload.clone());
                (
                    plan.network_id,
                    refreshed_payload.unwrap_or(fallback_payload),
                )
            });
        }

        let mut payloads = HashMap::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((network_id, payload)) => {
                    payloads.insert(network_id, payload);
                }
                Err(err) => {
                    nm_err!("[warn] Autonomous deployment refresh task failed: {}", err);
                }
            }
        }
        payloads
    }

    async fn maybe_refresh_manual_transition_payload(
        &self,
        network_id: &str,
        requested_payload: &str,
    ) -> Option<String> {
        let requested_cfg = network_config_from_config_payload(requested_payload)?;
        let snapshot_source = {
            let state = self.state.read().await;
            if !state.is_orchestrator {
                return None;
            }
            let net_status = state.network_registry.get(network_id)?;
            let previous_payload = state
                .network_snapshots
                .get(network_id)
                .filter(|payload| !payload.trim().is_empty())
                .map(String::as_str)
                .unwrap_or(net_status.config_json.as_str());
            let current_cfg = network_config_from_payload(previous_payload)?;
            let previous_deployment =
                network_deployment_from_payload(previous_payload).unwrap_or_default();
            let next_deployment =
                network_deployment_from_payload(requested_payload).unwrap_or_default();
            if previous_deployment == next_deployment {
                return None;
            }
            if !network_config_shape_compatible(&current_cfg, &requested_cfg) {
                return None;
            }
            live_snapshot_source_for(&state, network_id, &net_status.distribution)
        }?;

        self.fetch_live_network_snapshot(&snapshot_source)
            .await
            .and_then(|snapshot_json| snapshot_with_network_config(&snapshot_json, &requested_cfg))
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

        let (network_registry, network_snapshots, pending_commands, last_deployment_transition) = {
            let state = &mut *state;
            (
                &mut state.network_registry,
                &mut state.network_snapshots,
                &mut state.pending_commands,
                &mut state.last_deployment_transition,
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
                sync_network_status_deployment(net_status, &fresh_cfg.deployment);
                sync_network_status_transition(net_status, None);
                last_deployment_transition.remove(network_id);
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
        advertise_addr: Option<String>,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;

        let advertised = advertise_addr
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(grpc_addr);
        let msg = format!("NEUROMORPHIC_ORCHESTRATOR:{}", advertised.trim());
        let targets = resolve_discovery_targets().await;
        anyhow::ensure!(!targets.is_empty(), "no valid UDP discovery targets");

        nm_log!(
            "[info] Discovery beacon advertising {} to {} target(s)",
            advertised,
            targets.len()
        );

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

    async fn external_telemetry_snapshot(&self) -> Option<ExternalTelemetrySnapshot> {
        let Some(probe) = self.tracey_probe.as_ref() else {
            return None;
        };
        probe.snapshot().await
    }

    pub async fn get_resources(&self) -> Resources {
        let external_telemetry = self.external_telemetry_snapshot().await;

        let mut sys = self.system.write().await;
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let local_cpu_usage = sys.global_cpu_usage();
        let total_ram = sys.total_memory();
        let available_ram = sys.available_memory();

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

        let cpu_usage = external_telemetry
            .as_ref()
            .and_then(|snapshot| snapshot.cpu_usage_pct)
            .unwrap_or(local_cpu_usage);
        let mem_ratio = external_telemetry
            .as_ref()
            .and_then(|snapshot| snapshot.mem_used_pct)
            .map(|used_pct| (1.0 - (used_pct / 100.0)).clamp(0.0, 1.0))
            .unwrap_or_else(|| {
                if total_ram > 0 {
                    available_ram as f32 / total_ram as f32
                } else {
                    0.0
                }
            });

        let mut capacity = 1.0;
        capacity += (1.0 - (cpu_usage / 100.0).clamp(0.0, 1.0)) * 10.0;
        capacity += mem_ratio * 10.0;
        // Bias node capacity by parallelism so stronger hosts naturally receive
        // more layer assignments during orchestrator rebalancing.
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get() as f32)
            .unwrap_or(1.0)
            .max(1.0);
        capacity += (cpu_cores / 4.0).min(8.0);
        let num_gpus = external_telemetry
            .as_ref()
            .map(|snapshot| snapshot.gpu_count)
            .unwrap_or(0);
        if num_gpus > 0 {
            capacity += (num_gpus as f32 * 2.0).min(8.0);
        }
        if let Some(snapshot) = external_telemetry.as_ref() {
            let telemetry_pressure = external_telemetry_pressure(snapshot);
            if telemetry_pressure > 0.0 {
                capacity *= (1.0 - (telemetry_pressure * 0.55)).clamp(0.35, 1.0);
            }
        }
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
            cpu_usage,
            total_ram,
            available_ram,
            num_gpus,
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
            telemetry_source: external_telemetry
                .as_ref()
                .map(|snapshot| snapshot.source.clone())
                .unwrap_or_default(),
            telemetry_ts_ms: external_telemetry
                .as_ref()
                .map(|snapshot| snapshot.ts_ms)
                .unwrap_or(0),
            telemetry_cpu_usage_pct: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.cpu_usage_pct)
                .unwrap_or(0.0),
            telemetry_mem_used_pct: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.mem_used_pct)
                .unwrap_or(0.0),
            telemetry_net_rx_bps: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.net_rx_bps)
                .unwrap_or(0.0),
            telemetry_net_tx_bps: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.net_tx_bps)
                .unwrap_or(0.0),
            telemetry_disk_used_pct: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.disk_used_pct)
                .unwrap_or(0.0),
            telemetry_disk_read_bps: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.disk_read_bps)
                .unwrap_or(0.0),
            telemetry_disk_write_bps: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.disk_write_bps)
                .unwrap_or(0.0),
            telemetry_gpu_util_pct: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.gpu_util_pct)
                .unwrap_or(0.0),
            telemetry_gpu_temp_c: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.gpu_temp_c)
                .unwrap_or(0.0),
            telemetry_gpu_power_w: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.gpu_power_w)
                .unwrap_or(0.0),
            telemetry_gpu_mem_used_pct: external_telemetry
                .as_ref()
                .and_then(|snapshot| snapshot.gpu_mem_used_pct)
                .unwrap_or(0.0),
            telemetry_recent_action_count: external_telemetry
                .as_ref()
                .map(|snapshot| snapshot.recent_action_count)
                .unwrap_or(0),
        }
    }

    /// Convert only authenticated, currently enrolled stable-worker sessions
    /// into the placement planner's deterministic resource contract. The
    /// caller supplies the explicit compute allow-list and deployment facts
    /// that generic heartbeats cannot prove (storage, network budget and
    /// failure domain). Discovery and an unscoped telemetry observation never
    /// become placement authority through this method.
    #[cfg(feature = "stable_executor_live")]
    pub async fn get_placement_resource_observations(
        &self,
        allowed_nodes: &std::collections::BTreeSet<String>,
        failure_domains: &std::collections::BTreeMap<String, String>,
        numerical_profile: &str,
        storage_bytes_per_node: u64,
        network_bytes_per_second_per_node: u64,
    ) -> Vec<crate::placement::ResourceObservation> {
        if numerical_profile.trim().is_empty()
            || storage_bytes_per_node == 0
            || network_bytes_per_second_per_node == 0
        {
            return Vec::new();
        }
        let state = self.state.read().await;
        let now = std::time::Instant::now();
        let mut observations = Vec::new();
        for (node_id, node) in &state.nodes {
            if !allowed_nodes.contains(node_id)
                || !state
                    .last_heartbeat
                    .get(node_id)
                    .is_some_and(|last| now.duration_since(*last) <= PEER_STALE_AFTER)
                || !node.stable_executor_capabilities.iter().any(|capability| {
                    capability.profile == crate::stable_worker::STABLE_EXECUTOR_PROFILE
                        && capability.activation_schema_version
                            == crate::stable_worker::STABLE_WORKER_ACTIVATION_SCHEMA_VERSION
                })
            {
                continue;
            }
            let Some(failure_domain) = failure_domains.get(node_id) else {
                continue;
            };
            let resources = node.resources.clone().unwrap_or_default();
            let total_ram = resources.total_ram;
            let available_ram = resources.available_ram.min(total_ram);
            let capacity_units = if resources.capacity_score.is_finite() {
                (resources.capacity_score.max(0.001) * 1_000.0)
                    .round()
                    .min(u64::MAX as f32) as u64
            } else {
                0
            };
            let cpu_pct = if resources.telemetry_cpu_usage_pct.is_finite()
                && resources.telemetry_cpu_usage_pct > 0.0
            {
                resources.telemetry_cpu_usage_pct
            } else {
                resources.cpu_usage
            };
            let cpu_pressure = (cpu_pct.clamp(0.0, 100.0) * 10.0).round() as u16;
            let memory_pressure = if total_ram == 0 {
                1_000
            } else {
                1_000u64
                    .saturating_sub(available_ram.saturating_mul(1_000) / total_ram)
                    .min(1_000) as u16
            };
            let thermal_pressure =
                if resources.temperature_c.is_finite() && resources.temperature_c >= 0.0 {
                    ((resources.temperature_c - 40.0).max(0.0) * 16.6667)
                        .round()
                        .clamp(0.0, 1_000.0) as u16
                } else {
                    0
                };
            observations.push(crate::placement::ResourceObservation {
                node_id: node_id.clone(),
                device_id: format!("{node_id}-cpu"),
                healthy: true,
                enrolled: true,
                compute_authorised: true,
                failure_domain: failure_domain.clone(),
                numerical_profiles: vec![numerical_profile.to_owned()],
                capacity_units,
                reserved_capacity_units: 0,
                memory_bytes: total_ram,
                reserved_memory_bytes: total_ram.saturating_sub(available_ram),
                storage_bytes: storage_bytes_per_node,
                reserved_storage_bytes: 0,
                network_bytes_per_second: network_bytes_per_second_per_node,
                reserved_network_bytes_per_second: 0,
                cpu_pressure_milli: cpu_pressure,
                memory_pressure_milli: memory_pressure,
                network_pressure_milli: 0,
                thermal_pressure_milli: thermal_pressure,
            });
        }
        observations.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        observations
    }

    /// Return a logical placement boundary observed from an authoritative
    /// stable registration. Wall-clock time is intentionally never converted
    /// into biological time. A brain without a registered stable owner starts
    /// at the zero boundary and must publish its initial plan before any
    /// movement can be considered.
    #[cfg(feature = "stable_executor_live")]
    pub async fn get_authoritative_placement_tag(
        &self,
        network_id: &str,
        brain_id: crate::deterministic::BrainId,
    ) -> LogicalTag {
        let state = self.state.read().await;
        state
            .nodes
            .values()
            .flat_map(|node| node.stable_executors.iter())
            .filter(|registration| {
                registration.network_id == network_id
                    && registration.brain_id == brain_id.raw()
                    && registration.authoritative
            })
            .map(|registration| {
                LogicalTag::new(registration.current_tick, registration.current_microstep)
            })
            .max()
            .unwrap_or(LogicalTag::ZERO)
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

    /// Report the local stable executor capabilities for authenticated
    /// orchestrator observation. A non-stable build reports no capability and
    /// therefore cannot accidentally present a compatibility Runner as a
    /// stable shard owner.
    pub async fn get_stable_executor_registrations(&self) -> Vec<StableExecutorRegistration> {
        let mut registrations = Vec::new();
        #[cfg(feature = "stable_executor_live")]
        {
            let state = self.state.read().await;
            for (network_id, network) in &state.networks {
                let network = network.read().await;
                if let Some(executor) = network.stable_executor.as_ref() {
                    registrations.push(stable_registration_to_proto(
                        executor.registration_identity(network_id.clone()),
                    ));
                }
            }
        }

        // Partial workers are registered in the stable data plane rather than
        // in `ManagedNetwork`. Publish only durable receiver observations and
        // never infer an executor from a resource or discovery report.
        let network_names = self
            .stable_worker_networks
            .read()
            .ok()
            .map(|map| map.clone());
        let limits = self.stable_worker_limits.read().ok().map(|map| map.clone());
        if let (Some(network_names), Some(limits)) = (network_names, limits) {
            if let Ok(snapshots) = self.stable_shard_data_plane.registry().snapshots() {
                for snapshot in snapshots {
                    let Some(network_id) = network_names.get(&snapshot.brain_id) else {
                        continue;
                    };
                    let Some((max_input_events, max_steps_per_poll)) =
                        limits.get(&snapshot.brain_id)
                    else {
                        continue;
                    };
                    let application_acks = snapshot
                        .checkpoints
                        .iter()
                        .map(|checkpoint| StableShardApplicationAck {
                            shard_id: checkpoint.shard_id.raw(),
                            brain_id: snapshot.brain_id.raw(),
                            topology_generation: snapshot.topology_generation.raw(),
                            partition_generation: snapshot.partition_generation.raw(),
                            plan_digest: snapshot.plan_digest.to_string(),
                            lease_term: snapshot.lease_term.raw(),
                            fencing_token: snapshot.fencing_token,
                            applied_tick: checkpoint.current_tag.tick,
                            applied_microstep: checkpoint.current_tag.microstep,
                            state_digest: checkpoint.checkpoint_digest.to_string(),
                            durable_wal_sequence: None,
                            committed: true,
                        })
                        .collect::<Vec<_>>();
                    let registration = StableWorkerRegistration {
                        schema_version:
                            crate::stable_worker::STABLE_WORKER_REGISTRATION_SCHEMA_VERSION,
                        profile: crate::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
                        network_id: network_id.clone(),
                        brain_id: snapshot.brain_id.raw(),
                        topology_generation: snapshot.topology_generation.raw(),
                        partition_generation: snapshot.partition_generation.raw(),
                        topology_digest: snapshot.topology_digest.to_string(),
                        plan_digest: snapshot.plan_digest.to_string(),
                        shard_ids: snapshot.shard_ids.iter().map(|id| id.raw()).collect(),
                        owned_shard_ids: snapshot
                            .owned_shard_ids
                            .iter()
                            .map(|id| id.raw())
                            .collect(),
                        application_acks,
                        lease_term: snapshot.lease_term.raw(),
                        fencing_token: snapshot.fencing_token,
                        current_tick: snapshot.current_tag.tick,
                        current_microstep: snapshot.current_tag.microstep,
                        state_digest: snapshot.state_digest.to_string(),
                        max_input_events: *max_input_events,
                        max_steps_per_poll: *max_steps_per_poll,
                        authoritative: true,
                    };
                    if registration.validate().is_ok() {
                        registrations.push(stable_registration_to_proto(registration));
                    }
                }
            }
        }
        registrations.sort_by(|left, right| left.network_id.cmp(&right.network_id));
        registrations
    }

    /// Report an idle worker's ability to accept a future, digest-bound
    /// stable-worker manifest. The capability is deliberately not inferred
    /// from resources or discovery and never grants placement authority.
    pub async fn get_stable_executor_capabilities(&self) -> Vec<proto::StableExecutorCapability> {
        #[cfg(feature = "stable_executor_live")]
        {
            let capability = StableExecutorCapabilityModel {
                schema_version: crate::stable_worker::STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
                profile: crate::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
                activation_schema_version:
                    crate::stable_worker::STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
                max_input_events: crate::stable_worker::DEFAULT_STABLE_WORKER_MAX_INPUT_EVENTS,
                max_steps_per_poll: crate::stable_worker::DEFAULT_STABLE_WORKER_MAX_STEPS_PER_POLL,
            };
            return capability
                .validate()
                .map(|_| vec![stable_capability_to_proto(capability)])
                .unwrap_or_default();
        }
        #[cfg(not(feature = "stable_executor_live"))]
        {
            Vec::new()
        }
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
        let sender_node_id = self.state.read().await.node_id.clone();
        let request = authenticated_request(outbound, &sender_node_id)?;
        let response = client
            .stream_spikes(request)
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
            let sender_node_id = node.state.read().await.node_id.clone();
            let request = match authenticated_request(outbound, &sender_node_id) {
                Ok(request) => request,
                Err(error) => {
                    nm_err!(
                        "[warn] spike stream authentication {} failed: {}",
                        addr,
                        error
                    );
                    return;
                }
            };
            let response = client.stream_spikes(request).await;

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

    /// Send the inter-node boundary through the authoritative causal service.
    /// This path intentionally has no legacy fallback: once selected, an
    /// admission failure is visible and the event remains unsent for retry.
    #[cfg(feature = "replicated_durability")]
    async fn send_causal_batches(
        &self,
        network_id: &str,
        peer_id: &str,
        addr: &str,
        batches: &[SpikeBatch],
    ) -> Result<(), String> {
        // Keep durable outbox reservation and acknowledgement serialised from
        // the sender's point of view. If the stream fails, entries remain on
        // disk and the next attempt retransmits the same bounded prefix.
        let send_guard = {
            let state = self.state.read().await;
            state.causal_send_guard.clone()
        };
        let _send_guard = send_guard.lock().await;
        let (lease_term, partition_generation, entries) = {
            let network = {
                let state = self.state.read().await;
                state.networks.get(network_id).cloned()
            }
            .ok_or_else(|| format!("causal network {network_id} is not loaded"))?;
            let mut network = network.write().await;
            let owner = network
                .durable_owner
                .as_mut()
                .ok_or_else(|| "live causal transport requires a durable shard owner".to_owned())?;
            let durable_batches = batches
                .iter()
                .map(|batch| crate::managed_durability::DurableCausalBatch {
                    layer_index: batch.layer_index,
                    step_index: batch.step_index,
                    is_backward: batch.is_backward,
                    spike_indices: batch.spike_indices.clone(),
                    aer_payload: batch.aer_payload.clone(),
                    aer_base: batch.aer_base,
                })
                .collect::<Vec<_>>();
            // The managed step reserves the exact destination suffix in the
            // same durable commit as the biological state. Transmission may
            // only read that reservation; appending here would duplicate a
            // committed event every time the sender retries or forwards it.
            let entries = owner
                .pending_causal_outbox(peer_id)
                .map_err(|error| format!("causal outbox read failed: {error}"))?;
            if !durable_batches.is_empty() {
                let suffix_matches = entries.len() >= durable_batches.len()
                    && entries[entries.len() - durable_batches.len()..]
                        .iter()
                        .map(|entry| &entry.batch)
                        .eq(durable_batches.iter());
                if !suffix_matches {
                    return Err(
                        "causal batches were not present in the committed outbox suffix".to_owned(),
                    );
                }
            }
            (owner.lease_term(), owner.partition_generation(), entries)
        };
        if entries.is_empty() {
            return Ok(());
        }
        let brain = crate::managed_durability::managed_brain_id(network_id);
        let sender_node_id = {
            let state = self.state.read().await;
            state.node_id.clone()
        };
        let stream =
            crate::managed_durability::managed_link_stream_id(network_id, &sender_node_id, peer_id);
        let route = crate::managed_durability::managed_route_id(network_id);
        let mut frames = Vec::with_capacity(entries.len());
        for entry in &entries {
            let event = crate::managed_durability::managed_link_event_id(
                network_id,
                &sender_node_id,
                peer_id,
                entry.sequence,
            );
            let batch = &entry.batch;
            let ingress = CausalSpikeIngress {
                schema_version: CAUSAL_INGRESS_SCHEMA_VERSION,
                network_id: network_id.to_owned(),
                layer_index: batch.layer_index,
                step_index: batch.step_index,
                is_backward: batch.is_backward,
                spike_indices: batch.spike_indices.clone(),
                aer_payload: batch.aer_payload.clone(),
                aer_base: batch.aer_base,
            };
            let payload = serde_json::to_vec(&ingress)
                .map_err(|error| format!("encode causal ingress: {error}"))?;
            let envelope = crate::data_plane::CausalEnvelope {
                schema_version: crate::deterministic::SchemaVersion::CURRENT,
                brain,
                stream,
                sequence: entry.sequence,
                lease_term,
                route,
                partition_generation,
                source: None,
                target: None,
                tag: crate::deterministic::LogicalTag::new(
                    u64::try_from(batch.step_index.max(0)).unwrap_or(0),
                    0,
                ),
                event,
                stage: crate::deterministic::EventStage::SpikeDecision,
                kind: crate::data_plane::EnvelopeKind::Event,
                payload,
                deferred_from_nonconvergence: false,
            };
            frames.push(crate::causal_transport::proto::CausalEventEnvelope::from(
                &envelope,
            ));
            if let Some(frame) = frames.last_mut() {
                frame.sender_node_id = sender_node_id.clone();
            }
        }

        let target = if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.to_owned()
        } else {
            format!("http://{addr}")
        };
        let endpoint = crate::management::grpc_client_endpoint(&target)
            .map_err(|error| format!("causal endpoint configuration failed: {error}"))?;
        let mut client = CausalDataPlaneClient::connect(endpoint)
            .await
            .map_err(|error| format!("causal connect to {target} failed: {error}"))?
            .max_decoding_message_size(grpc_max_message_bytes())
            .max_encoding_message_size(grpc_max_message_bytes());
        let (sender, receiver) = mpsc::channel::<crate::causal_transport::proto::CausalEventEnvelope>(
            entries.len().clamp(1, 256),
        );
        let mut stream_request =
            Request::new(tokio_stream::wrappers::ReceiverStream::new(receiver));
        if live_causal_transport_enabled() {
            let token = std::env::var("NM_CAUSAL_NODE_TOKEN")
                .map_err(|_| "live causal transport requires NM_CAUSAL_NODE_TOKEN".to_owned())?;
            let node_header = tonic::metadata::MetadataValue::try_from(sender_node_id.as_str())
                .map_err(|_| "causal sender node identity is not valid metadata".to_owned())?;
            let token_header = tonic::metadata::MetadataValue::try_from(token.as_str())
                .map_err(|_| "causal sender credential is not valid metadata".to_owned())?;
            stream_request
                .metadata_mut()
                .insert("x-aarnn-node-id", node_header);
            stream_request
                .metadata_mut()
                .insert("x-aarnn-node-token", token_header);
        }
        let response =
            tokio::time::timeout(spike_burst_timeout(), client.stream_events(stream_request))
                .await
                .map_err(|_| "causal stream start timed out".to_owned())?
                .map_err(|error| format!("causal stream start failed: {error}"))?;
        let mut inbound = response.into_inner();
        for frame in &frames {
            sender
                .send(frame.clone())
                .await
                .map_err(|_| "causal stream request channel closed".to_owned())?;
        }
        drop(sender);
        let mut acknowledged = 0usize;
        while let Some(frame) = inbound
            .message()
            .await
            .map_err(|error| format!("causal acknowledgement failed: {error}"))?
        {
            let expected_frame = frames
                .get(acknowledged)
                .ok_or_else(|| "causal acknowledgement exceeded the sent batch".to_owned())?;
            if &frame != expected_frame {
                return Err(format!(
                    "causal acknowledgement mismatch at batch position {acknowledged}"
                ));
            }
            let sequence = frame.sequence;
            let expected = entries[acknowledged].sequence;
            if sequence != expected {
                return Err(format!(
                    "causal acknowledgement sequence mismatch: expected {expected}, received {sequence}"
                ));
            }
            acknowledged += 1;
        }
        if acknowledged != entries.len() {
            return Err(format!(
                "causal stream closed after {acknowledged}/{} acknowledgements",
                entries.len()
            ));
        }
        let last_sequence = entries
            .last()
            .map(|entry| entry.sequence)
            .ok_or_else(|| "causal outbox returned no entries".to_owned())?;
        let network = {
            let state = self.state.read().await;
            state.networks.get(network_id).cloned()
        }
        .ok_or_else(|| format!("causal network {network_id} is not loaded"))?;
        let mut network = network.write().await;
        let owner = network
            .durable_owner
            .as_mut()
            .ok_or_else(|| "live causal transport requires a durable shard owner".to_owned())?;
        owner
            .acknowledge_causal_outbox(peer_id, last_sequence)
            .map_err(|error| format!("causal outbox acknowledgement failed: {error}"))?;
        Ok(())
    }

    #[cfg(not(feature = "replicated_durability"))]
    async fn send_causal_batches(
        &self,
        _network_id: &str,
        _peer_id: &str,
        _addr: &str,
        _batches: &[SpikeBatch],
    ) -> Result<(), String> {
        Err("live causal transport requires replicated_durability".to_owned())
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
        // MPI carries the legacy SpikeBatch contract and therefore cannot be
        // active beside the authoritative causal profile. A transport
        // fallback here would allow one committed boundary to arrive through
        // two independent sequence/deduplication domains.
        if live_causal_transport_enabled() {
            return;
        }
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
        if batches.is_empty() && !live_causal_transport_enabled() {
            return;
        }
        let targets = self
            .spike_targets_for_network(network_id, exclude_node)
            .await;
        if targets.is_empty() {
            return;
        }

        // The live causal profile is an exclusive transport selection. It
        // must never send a legacy batch as a fallback after an authoritative
        // causal admission has been attempted, otherwise a retry could apply
        // the same neural boundary through two independent paths.
        if live_causal_transport_enabled() {
            for (key, addr) in targets {
                if let Err(error) = self
                    .send_causal_batches(network_id, &key, &addr, batches)
                    .await
                {
                    nm_err!(
                        "[warn] authoritative causal forwarding to {} failed: {}",
                        key,
                        error
                    );
                    let mut state = self.state.write().await;
                    state.record_spike_drop(&key, batches.len() as u64);
                }
            }
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
                            spike_burst_timeout(),
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
                                    spike_burst_timeout()
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
        let transition_now = std::time::Instant::now();
        let autonomous_transition_plans: Vec<_> = {
            let state = self.state.read().await;
            if !state.is_orchestrator {
                return;
            }
            if state.nodes.is_empty() {
                return;
            }
            collect_autonomous_transition_plans(&state, transition_now)
                .into_iter()
                .filter(|plan| !state.stable_network_ids.contains(&plan.network_id))
                .collect()
        };
        let autonomous_transition_payloads = if autonomous_transition_plans.is_empty() {
            HashMap::new()
        } else {
            self.resolve_autonomous_transition_payloads(&autonomous_transition_plans)
                .await
        };

        let mut state = self.state.write().await;
        if !state.is_orchestrator {
            return;
        }

        let node_ids: Vec<String> = state.nodes.keys().cloned().collect();
        if node_ids.is_empty() {
            return;
        }

        for plan in autonomous_transition_plans {
            let updated_payload = autonomous_transition_payloads
                .get(&plan.network_id)
                .cloned()
                .or_else(|| {
                    payload_with_updated_deployment(&plan.fallback_payload, &plan.next_deployment)
                })
                .unwrap_or_else(|| plan.fallback_payload.clone());
            let transition_record = DeploymentTransitionRecord {
                observed_at: transition_now,
                ts_ms: unix_timestamp_ms_now(),
                reason: plan.reason.clone(),
                source: "autonomous".to_string(),
            };

            if let Some(net_status) = state.network_registry.get_mut(&plan.network_id) {
                net_status.config_json = updated_payload.clone();
                sync_network_status_deployment(net_status, &plan.next_deployment);
                sync_network_status_transition(net_status, Some(&transition_record));
            }
            if crate::runner::decode_snapshot_with_profile_backfill(&updated_payload).is_ok() {
                state
                    .network_snapshots
                    .insert(plan.network_id.clone(), updated_payload.clone());
            } else {
                state.network_snapshots.remove(&plan.network_id);
            }
            state
                .last_deployment_transition
                .insert(plan.network_id.clone(), transition_record);
            nm_log!(
                "[info] Autonomous deployment transition for network {} -> {:?} ({})",
                plan.network_id,
                plan.next_deployment.modes,
                plan.reason
            );
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
                .map(|resources| effective_capacity_score(resources, rebalance_target_step_ms))
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
        let active_network_counts: HashMap<String, usize> = state
            .nodes
            .iter()
            .map(|(node_id, status)| (node_id.clone(), status.active_networks.len()))
            .collect();

        let existing_primary_nodes: HashMap<String, String> = state
            .network_registry
            .iter()
            .filter_map(|(net_id, status)| {
                primary_node_for_distribution(&status.distribution)
                    .map(|node_id| (net_id.clone(), node_id))
            })
            .collect();
        let deployment_by_network: HashMap<String, DeploymentConfig> = state
            .network_registry
            .iter()
            .map(|(net_id, status)| {
                let payload = state
                    .network_snapshots
                    .get(net_id)
                    .filter(|payload| !payload.trim().is_empty())
                    .map(String::as_str)
                    .unwrap_or(status.config_json.as_str());
                let deployment = network_deployment_from_payload(payload).unwrap_or_default();
                (net_id.clone(), deployment)
            })
            .collect();
        let stable_network_ids = state.stable_network_ids.clone();

        let mut all_pending = Vec::new();
        let (network_registry, network_snapshots) = {
            let state = &mut *state;
            (&mut state.network_registry, &mut state.network_snapshots)
        };

        for (net_id, net_status) in network_registry.iter_mut() {
            if stable_network_ids.contains(net_id) {
                // Stable workers own a complete virtual-shard fabric. The
                // compatibility layer scheduler must leave its placement and
                // commands untouched until an explicit migration transaction
                // changes the authority profile.
                if !net_status
                    .deployment_modes
                    .iter()
                    .any(|mode| mode == "stable-executor")
                {
                    net_status
                        .deployment_modes
                        .push("stable-executor".to_owned());
                }
                continue;
            }
            let mut snapshot_layers: Option<u32> = None;
            let mut config_payload: Option<String> = None;

            if let Some(snap_json) = network_snapshots.get(net_id).cloned() {
                let mut effective_snapshot = snap_json.clone();
                if let Some(requested_cfg) =
                    network_config_from_config_payload(&net_status.config_json)
                {
                    if let Ok(snap) =
                        crate::runner::decode_snapshot_with_profile_backfill(&snap_json)
                    {
                        if network_config_shape_compatible(&snap.net, &requested_cfg) {
                            if let Some(merged) =
                                snapshot_with_network_config(&snap_json, &requested_cfg)
                            {
                                if merged != snap_json {
                                    network_snapshots.insert(net_id.clone(), merged.clone());
                                }
                                effective_snapshot = merged;
                            }
                        } else {
                            // Snapshot shape conflicts with requested config (e.g. stale S/O from
                            // previous runs). Prefer config payload to keep hosted runners aligned.
                            config_payload = serde_json::to_string(&requested_cfg).ok();
                            snapshot_layers = Some((requested_cfg.num_hidden_layers + 1) as u32);
                            network_snapshots.remove(net_id);
                        }
                    }
                }
                if config_payload.is_none() {
                    config_payload = Some(effective_snapshot.clone());
                    if let Ok(snap) =
                        crate::runner::decode_snapshot_with_profile_backfill(&effective_snapshot)
                    {
                        snapshot_layers = Some((snap.net.num_hidden_layers + 1) as u32);
                    }
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
            let deployment = deployment_by_network
                .get(net_id)
                .cloned()
                .unwrap_or_default();
            let shard_across_nodes = should_shard_across_nodes(&deployment);
            let config_json = config_payload.unwrap_or_else(|| net_status.config_json.clone());
            let affinity_node_capacities: Vec<(String, f32)> = network_affinity
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
            // Affinity preserves placement for single-target networks, but it
            // must not prevent a sharded network from expanding onto workers
            // that joined or restarted after the initial assignment.
            let mut target_node_capacities = if shard_across_nodes {
                node_capacities.clone()
            } else {
                affinity_node_capacities
            };
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
            let existing_affinity_nodes: HashSet<String> = network_affinity
                .get(net_id)
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            target_node_capacities = limit_target_nodes_for_deployment(
                net_id,
                &target_node_capacities,
                &deployment,
                &deployment_by_network,
                &existing_primary_nodes,
                &active_network_counts,
                &existing_affinity_nodes,
            );
            if target_node_capacities.is_empty() {
                continue;
            }
            let mut target_capacity_sum: f32 =
                target_node_capacities.iter().map(|(_, cap)| *cap).sum();
            if target_capacity_sum <= 0.0 {
                target_capacity_sum = target_node_capacities.len() as f32;
            }

            // Preserve existing layer neuron counts to avoid UI flicker during rebalance
            let previous_nodes: HashSet<String> = net_status.distribution.keys().cloned().collect();
            let previous_distribution = net_status.distribution.clone();
            let mut old_counts = HashMap::new();
            for (nid, range) in &net_status.distribution {
                old_counts.insert(nid.clone(), range.layer_neuron_counts.clone());
            }

            net_status.distribution.clear();

            if !shard_across_nodes {
                let Some(node_id) = choose_single_node_target(
                    net_id,
                    &target_node_capacities,
                    &deployment,
                    &deployment_by_network,
                    &existing_primary_nodes,
                ) else {
                    continue;
                };

                let layers: Vec<u32> = (0..total_layers).collect();
                net_status.distribution.insert(
                    node_id.clone(),
                    LayerRange {
                        layers: layers.clone(),
                        layer_neuron_counts: old_counts.remove(&node_id).unwrap_or_default(),
                        backup_layers: Vec::new(),
                    },
                );

                let cmd = NetworkCommand {
                    r#type: proto::network_command::CommandType::LoadNetwork as i32,
                    network_id: net_id.clone(),
                    config_json: config_json.as_bytes().to_vec(),
                    layers,
                    redundant_layers: Vec::new(),
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
            } else {
                let node_assignments =
                    build_sharded_node_assignments(&target_node_capacities, total_layers);

                for (node_id, layers, redundant) in node_assignments {
                    net_status.distribution.insert(
                        node_id.clone(),
                        LayerRange {
                            layers: layers.clone(),
                            layer_neuron_counts: old_counts.remove(&node_id).unwrap_or_default(),
                            backup_layers: redundant.clone(),
                        },
                    );

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
            }

            let new_nodes: HashSet<String> = net_status.distribution.keys().cloned().collect();
            for removed_node in previous_nodes.difference(&new_nodes) {
                let unload_cmd = NetworkCommand {
                    r#type: proto::network_command::CommandType::UnloadNetwork as i32,
                    network_id: net_id.clone(),
                    config_json: Vec::new(),
                    layers: Vec::new(),
                    redundant_layers: Vec::new(),
                    desired_aarnn_depth: net_status.desired_aarnn_depth,
                    neuron_model: String::new(),
                    learning_rule: String::new(),
                };
                all_pending.push((removed_node.clone(), unload_cmd));
            }

            // A network total must only contain that network's reports. Layers can be
            // repeated on redundant shards, so count each global layer once.
            net_status.total_neurons = total_neurons_from_distribution(&net_status.distribution);
            net_status.shard_movements = build_shard_placement_movements(
                net_id,
                &previous_distribution,
                &net_status.distribution,
                net_status.autonomous_transition_enabled,
                &net_status.last_transition_reason,
                unix_timestamp_ms_now(),
            );
        }

        for (node_id, cmd) in all_pending {
            enqueue_pending_command(&mut state.pending_commands, node_id, cmd);
        }
    }

    pub async fn handle_command(&self, cmd: NetworkCommand) {
        use proto::network_command::CommandType;
        let cmd_type = CommandType::try_from(cmd.r#type).unwrap_or(CommandType::Stop);
        if cmd_type == CommandType::ActivateStableWorker {
            #[cfg(feature = "stable_executor_live")]
            {
                let command = match serde_json::from_str::<
                    crate::stable_worker::StableWorkerActivationCommand,
                >(&String::from_utf8_lossy(&cmd.config_json))
                {
                    Ok(command) => command,
                    Err(error) => {
                        nm_err!(
                            "[error] rejecting stable worker activation command for {}: invalid command JSON: {}",
                            cmd.network_id,
                            error
                        );
                        return;
                    }
                };
                if let Err(error) = self.activate_stable_worker(command).await {
                    nm_err!(
                        "[error] stable worker activation failed for {}: {}",
                        cmd.network_id,
                        error
                    );
                }
            }
            #[cfg(not(feature = "stable_executor_live"))]
            nm_err!(
                "[error] stable worker activation requested for {} but this node binary lacks stable_executor_live",
                cmd.network_id
            );
            return;
        }
        let mut state = self.state.write().await;
        match cmd_type {
            CommandType::LoadNetwork => {
                if let Some(net_arc) = state.networks.get(&cmd.network_id) {
                    let mut net = net_arc.write().await;
                    #[cfg(feature = "stable_executor_live")]
                    if net.stable_executor_registered() {
                        nm_err!(
                            "[warn] Ignoring legacy layer load for stable network {}",
                            cmd.network_id
                        );
                        return;
                    }
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
                            #[cfg(feature = "superdense_executor")]
                            net.superdense.reset();
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
                            #[cfg(feature = "superdense_executor")]
                            net.superdense.reset();
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
                                #[cfg(feature = "superdense_executor")]
                                net.superdense.reset();
                            }
                        }
                    }
                    if !cmd.learning_rule.is_empty() {
                        if let Some(l) = Learning::from_str(&cmd.learning_rule) {
                            if net.runner.learning != l {
                                net.runner.set_learning(l);
                                #[cfg(feature = "superdense_executor")]
                                net.superdense.reset();
                            }
                        }
                    }
                } else {
                    nm_log!(
                        "[info] Loading network {} with layers {:?} (redundant: {:?}, depth: {}, model: {}, learning: {})",
                        cmd.network_id,
                        cmd.layers,
                        cmd.redundant_layers,
                        cmd.desired_aarnn_depth,
                        cmd.neuron_model,
                        cmd.learning_rule
                    );

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
                    #[cfg(feature = "replicated_durability")]
                    let durable_owner =
                        open_managed_durability(&network_id, &state.node_id, &mut runner);
                    #[cfg(feature = "replicated_durability")]
                    if crate::managed_durability::configured_root().is_some()
                        && durable_owner.is_none()
                    {
                        // Once the durable profile is explicitly configured,
                        // falling back to the mutable Runner would turn a
                        // storage, lease or recovery failure into an
                        // unadvertised loss of the commit boundary.
                        nm_err!(
                            "[error] Refusing to load network {} without its configured durable owner",
                            network_id
                        );
                        return;
                    }

                    let recovered_channel = {
                        #[cfg(feature = "replicated_durability")]
                        let recovered_channel = if let Some(owner) = durable_owner.as_ref() {
                            match owner.authoritative_channel_state().and_then(|state| {
                                serde_json::from_str::<ManagedChannelState>(&state).map_err(
                                    |error| {
                                        crate::durability::DurabilityError::Corrupt(
                                            error.to_string(),
                                        )
                                    },
                                )
                            }) {
                                Ok(channel) => channel,
                                Err(error) => {
                                    nm_err!(
                                        "[error] Refusing to load network {} with invalid durable channel state: {}",
                                        network_id,
                                        error
                                    );
                                    return;
                                }
                            }
                        } else {
                            ManagedChannelState::default()
                        };
                        #[cfg(not(feature = "replicated_durability"))]
                        let recovered_channel = ManagedChannelState::default();
                        recovered_channel
                    };

                    // A durable managed owner can enter the stable-shard
                    // runtime gate immediately. The compatibility Runner is
                    // still only a projection until this evidence and its
                    // single-owner placement generation have been adopted.
                    #[cfg(feature = "replicated_durability")]
                    let shard_runtime = durable_owner.as_ref().and_then(|owner| {
                        let shard_state = owner.authoritative_state().ok()?;
                        let evidence = crate::managed_shard_runtime::RuntimeShardEvidence {
                            shard_id: crate::managed_durability::managed_shard_id(&network_id),
                            brain_id: crate::managed_durability::managed_brain_id(&network_id),
                            node_id: state.node_id.clone(),
                            device_id: "cpu".to_owned(),
                            topology_generation: owner.topology_generation(),
                            partition_generation: owner.partition_generation(),
                            lease_term: owner.lease_term(),
                            fencing_token: owner.fencing_token(),
                            authoritative_state_digest: shard_state.state_digest,
                        };
                        let mut runtime =
                            crate::managed_shard_runtime::ManagedShardRuntime::new(evidence)
                                .ok()?;
                        let plan =
                            crate::managed_shard_runtime::ManagedShardRuntime::single_owner_plan(
                                &runtime.evidence,
                                shard_state.committed_tag,
                            )
                            .ok()?;
                        runtime.adopt_generation(&plan, 0).ok()?;
                        Some(runtime)
                    });
                    #[cfg(not(feature = "replicated_durability"))]
                    let shard_runtime = None;

                    state.networks.insert(
                        network_id.clone(),
                        Arc::new(RwLock::new(ManagedNetwork {
                            id: network_id,
                            runner,
                            shard_runtime,
                            #[cfg(feature = "stable_executor_live")]
                            stable_executor: None,
                            #[cfg(feature = "replicated_durability")]
                            durable_owner,
                            #[cfg(feature = "superdense_executor")]
                            superdense: SuperdenseController::new(),
                            assigned_layers: cmd.layers,
                            redundant_layers: cmd.redundant_layers,
                            remote_spikes_fwd: recovered_channel
                                .remote_spikes_fwd
                                .into_iter()
                                .collect(),
                            remote_spikes_bwd: recovered_channel
                                .remote_spikes_bwd
                                .into_iter()
                                .collect(),
                            remote_spike_steps_fwd: recovered_channel
                                .remote_spike_steps_fwd
                                .into_iter()
                                .collect(),
                            remote_spike_steps_bwd: recovered_channel
                                .remote_spike_steps_bwd
                                .into_iter()
                                .collect(),
                            external_sensory_spikes: recovered_channel.external_sensory_spikes,
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
            CommandType::UnloadNetwork => {
                #[cfg(feature = "stable_executor_live")]
                if let Some(net_arc) = state.networks.get(&cmd.network_id) {
                    if net_arc.read().await.stable_executor_registered() {
                        nm_err!(
                            "[warn] Ignoring legacy unload for stable network {}",
                            cmd.network_id
                        );
                        return;
                    }
                }
                if state.networks.remove(&cmd.network_id).is_some() {
                    nm_log!("[info] Unloaded network {} from local node", cmd.network_id);
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

    /// Apply one command and, for stable-worker activation, return a bounded
    /// digest-bound result suitable for the next orchestrator heartbeat.
    /// Legacy commands retain their historical fire-and-forget contract until
    /// their own versioned acknowledgement protocol exists.
    pub async fn handle_command_with_result(
        &self,
        cmd: NetworkCommand,
    ) -> Option<NetworkCommandResult> {
        use proto::network_command::CommandType;
        if CommandType::try_from(cmd.r#type).ok() != Some(CommandType::ActivateStableWorker) {
            self.handle_command(cmd).await;
            return None;
        }

        let activation = match serde_json::from_slice::<
            crate::stable_worker::StableWorkerActivationCommand,
        >(&cmd.config_json)
        {
            Ok(command) => command,
            Err(error) => {
                nm_err!(
                    "[error] cannot acknowledge malformed stable activation for {}: {}",
                    cmd.network_id,
                    error
                );
                return None;
            }
        };
        let mut result = NetworkCommandResult {
            command_type: CommandType::ActivateStableWorker as i32,
            network_id: activation.network_id.clone(),
            request_id: activation.request_id.clone(),
            manifest_digest: activation.manifest_digest.clone(),
            accepted: false,
            error: String::new(),
            brain_id: activation.brain_id,
            placement_idempotency_key: activation.placement_idempotency_key.clone(),
        };

        #[cfg(feature = "stable_executor_live")]
        {
            match self.activate_stable_worker(activation).await {
                Ok(()) => result.accepted = true,
                Err(error) => result.error = error,
            }
        }
        #[cfg(not(feature = "stable_executor_live"))]
        {
            result.error = "stable worker activation requires stable_executor_live".to_owned();
        }
        Some(result)
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
                #[cfg(feature = "replicated_durability")]
                let live_outbox_peers = if live_causal_transport_enabled() {
                    let network_id = net_arc.read().await.id.clone();
                    self.spike_targets_for_network(&network_id, None)
                        .await
                        .into_iter()
                        .map(|(node_id, _)| node_id)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let mut net = net_arc.write().await;
                if !net.playing {
                    continue;
                }
                if let Err(error) = net.admit_shard_step() {
                    nm_err!(
                        "[warn] Refusing network {} step because shard ownership evidence is stale: {}",
                        net.id,
                        error
                    );
                    continue;
                }
                any_playing = true;

                #[cfg(feature = "stable_executor_live")]
                if net.stable_executor_registered() {
                    // The stable manifest profile currently owns every virtual
                    // shard in this process. Receiving a legacy layer batch
                    // would otherwise be silently ignored, so stop at the
                    // safety boundary until physical stable-shard routing is
                    // available.
                    if !net.remote_spikes_fwd.is_empty() || !net.remote_spikes_bwd.is_empty() {
                        net.playing = false;
                        nm_err!(
                            "[error] Pausing stable network {} because it received work outside its local stable-shard profile",
                            net.id
                        );
                        continue;
                    }
                    let external_sensory = net.external_sensory_spikes.take();
                    let stable_start = std::time::Instant::now();
                    let poll = match net.poll_stable_executor_sensory(external_sensory.as_deref()) {
                        Ok(poll) => poll,
                        Err(error) => {
                            net.playing = false;
                            nm_err!(
                                "[error] Pausing stable network {} after authoritative poll failure: {}",
                                net.id,
                                error
                            );
                            continue;
                        }
                    };
                    let elapsed = stable_start.elapsed().as_secs_f32() * 1000.0;
                    if net.avg_step_time_ms == 0.0 {
                        net.avg_step_time_ms = elapsed;
                    } else {
                        net.avg_step_time_ms = 0.9 * net.avg_step_time_ms + 0.1 * elapsed;
                    }
                    if poll.budget_exhausted {
                        nm_log!(
                            "[info] Stable network {} retained {} bounded causal events for the next poll",
                            net.id,
                            poll.pending_after
                        );
                    }
                    // All shards in this explicitly local profile share the
                    // same durable executor. Emitted events are either
                    // consumed by the bounded drain or retained in its
                    // immutable pending checkpoint; no transport output is
                    // fabricated or discarded here.
                    drop(net);
                    continue;
                }

                // The input queues are drained into the compatibility kernel
                // before the step. Keep an owned pre-step image so a failed
                // durable publication can retry the exact same admission
                // without silently losing queued causal input.
                #[cfg(any(feature = "superdense_executor", feature = "replicated_durability"))]
                let previous_channel_state = capture_channel_state(&net);

                // Sync remote spikes into runner before stepping.
                // Use copy_from_slice instead of Array1::from_vec to reuse the existing
                // allocation and avoid per-step heap allocation on the hot path.
                let fwd_spikes = std::mem::take(&mut net.remote_spikes_fwd);
                for (l, spikes) in fwd_spikes {
                    let li = l as usize;
                    if li < net.runner.last_spk_h.len() {
                        let sz = net.runner.layer_size(li);
                        if spikes.len() == sz {
                            if let Some(dst) = net.runner.last_spk_h[li].as_slice_mut() {
                                dst.copy_from_slice(&spikes);
                            }
                        } else {
                            // Topology mismatch: resize-and-copy (rare path).
                            let n = sz.min(spikes.len());
                            if let Some(dst) = net.runner.last_spk_h[li].as_slice_mut() {
                                dst[..n].copy_from_slice(&spikes[..n]);
                                for v in dst[n..].iter_mut() {
                                    *v = 0;
                                }
                            }
                        }
                    }
                }
                let bwd_spikes = std::mem::take(&mut net.remote_spikes_bwd);
                for (l, spikes) in bwd_spikes {
                    let li = l as usize;
                    if li < net.runner.last_spk_h.len() {
                        let sz = net.runner.layer_size(li);
                        if spikes.len() == sz {
                            if let Some(dst) = net.runner.last_spk_h[li].as_slice_mut() {
                                dst.copy_from_slice(&spikes);
                            }
                        } else {
                            let n = sz.min(spikes.len());
                            if let Some(dst) = net.runner.last_spk_h[li].as_slice_mut() {
                                dst[..n].copy_from_slice(&spikes[..n]);
                                for v in dst[n..].iter_mut() {
                                    *v = 0;
                                }
                            }
                        }
                    }
                }

                let external_sensory = net.external_sensory_spikes.take();
                #[cfg(feature = "replicated_durability")]
                let (_out, durable_batches) = match net.step_and_commit_durable_with_outbox(
                    external_sensory.as_deref(),
                    previous_channel_state.clone(),
                    &live_outbox_peers,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        nm_err!(
                            "[warn] Durable managed step for network {} deferred for retry: {}",
                            net.id,
                            error
                        );
                        continue;
                    }
                };
                #[cfg(not(feature = "replicated_durability"))]
                #[cfg(feature = "superdense_executor")]
                let out = match net.step_with_superdense(external_sensory.as_deref()) {
                    Ok(out) => out,
                    Err(error) => {
                        restore_channel_state(&mut net, previous_channel_state.clone());
                        nm_err!(
                            "[warn] Superdense step for network {} deferred for retry: {}",
                            net.id,
                            error
                        );
                        continue;
                    }
                };
                #[cfg(all(
                    not(feature = "replicated_durability"),
                    not(feature = "superdense_executor")
                ))]
                let out = if let Some(ref sensory) = external_sensory {
                    net.runner.step(Some(sensory.as_slice()))
                } else {
                    net.runner.step(None)
                };

                #[cfg(not(feature = "replicated_durability"))]
                let step_index = out.t as i64;
                let net_id = net.id.clone();
                #[cfg(feature = "replicated_durability")]
                let batches = durable_batches;
                #[cfg(not(feature = "replicated_durability"))]
                let batches = managed_spike_batches(&net, step_index);

                #[cfg(feature = "replicated_durability")]
                if let Some(owner) = net.durable_owner.as_ref() {
                    match owner.authoritative_state() {
                        Ok(state) => {
                            if let Err(error) =
                                net.commit_shard_step(state.committed_tag, state.state_digest)
                            {
                                nm_err!(
                                    "[error] Refusing network {} output after shard commit evidence failed: {}",
                                    net.id,
                                    error
                                );
                                continue;
                            }
                        }
                        Err(error) => {
                            nm_err!(
                                "[error] Refusing network {} output because durable shard state cannot be read: {}",
                                net.id,
                                error
                            );
                            continue;
                        }
                    }
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
                        match local_shard_snapshot(&net) {
                            Ok((snapshot_json, _, _, _)) => {
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
                // A depth-zero network can remain transport-ready while
                // producing no meaningful network output. Keep a configurable
                // floor for output-producing workloads; operators can still
                // set it to zero for intentionally shallow simulations.
                let minimum_depth = std::env::var("NM_AARNN_MIN_DEPTH")
                    .ok()
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(net.desired_aarnn_depth as usize);

                if auto_adjust_depth && net.runner.t >= warmup_steps {
                    if net.avg_step_time_ms > target_ms
                        && net.runner.net.aarnn_layer_depth > minimum_depth
                    {
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
impl crate::causal_transport::proto::causal_data_plane_server::CausalDataPlane for DistributedNode {
    type StreamEventsStream = tokio_stream::wrappers::ReceiverStream<
        Result<crate::causal_transport::proto::CausalEventEnvelope, Status>,
    >;

    async fn stream_events(
        &self,
        request: Request<tonic::Streaming<crate::causal_transport::proto::CausalEventEnvelope>>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let live_authentication = live_causal_transport_enabled();
        let request_metadata = request.metadata().clone();
        let peer_certificate_sha256 = request.peer_certs().and_then(|certificates| {
            certificates
                .first()
                .map(|certificate| certificate_sha256_der(certificate.as_ref()))
        });
        let mut inbound = request.into_inner();
        let (sender, receiver) = mpsc::channel(128);
        while let Some(frame) = inbound.message().await? {
            let causal =
                crate::data_plane::CausalEnvelope::try_from(frame.clone()).map_err(|error| {
                    Status::invalid_argument(format!("invalid authoritative causal frame: {error}"))
                })?;
            let sender_node_id = frame.sender_node_id.trim();
            if sender_node_id.is_empty() {
                return Err(Status::unauthenticated(
                    "causal transport sender identity is required",
                ));
            }
            if live_authentication {
                validate_causal_peer_metadata(
                    &request_metadata,
                    sender_node_id,
                    peer_certificate_sha256.as_deref(),
                )?;
            }
            let receiver_node_id = {
                let state = self.state.read().await;
                state.node_id.clone()
            };
            let known_sender = {
                let state = self.state.read().await;
                state.node_id == sender_node_id
                    || state.peers.contains_key(sender_node_id)
                    || state.nodes.contains_key(sender_node_id)
            };
            if !known_sender {
                return Err(Status::permission_denied(
                    "causal sender is not enrolled with this node",
                ));
            }
            // Keep the stable executor adapter entirely behind its feature
            // gate.  The local/reference profile must compile and retain its
            // established JSON ingress path without linking or naming the
            // durable stable executor methods.
            #[cfg(feature = "stable_executor_live")]
            {
                if let Some(network) = self
                    .stable_network_for_causal_stream(
                        causal.brain,
                        causal.stream,
                        sender_node_id,
                        &receiver_node_id,
                    )
                    .await
                {
                    self.admit_stable_causal_event(network, causal).await?;
                    sender
                        .send(Ok(frame))
                        .await
                        .map_err(|_| Status::cancelled("causal response stream closed"))?;
                    continue;
                }
            }
            let ingress: CausalSpikeIngress =
                serde_json::from_slice(&causal.payload).map_err(|error| {
                    Status::invalid_argument(format!("invalid causal ingress: {error}"))
                })?;
            let expected_stream = crate::managed_durability::managed_link_stream_id(
                &ingress.network_id,
                sender_node_id,
                &receiver_node_id,
            );
            if causal.stream != expected_stream {
                return Err(Status::permission_denied(
                    "causal stream is not bound to the declared sender",
                ));
            }
            self.admit_causal_spike_ingress(&frame, causal, ingress)
                .await?;
            sender
                .send(Ok(frame))
                .await
                .map_err(|_| Status::cancelled("causal response stream closed"))?;
        }
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            receiver,
        )))
    }
}

#[tonic::async_trait]
impl DistributedNeuromorphic for DistributedNode {
    async fn join(&self, request: Request<JoinRequest>) -> Result<Response<JoinResponse>, Status> {
        let remote_addr = request.remote_addr();
        let request_metadata = request.metadata().clone();
        let peer_certificate_sha256 = request.peer_certs().and_then(|certificates| {
            certificates
                .first()
                .map(|certificate| certificate_sha256_der(certificate.as_ref()))
        });
        let req = request.into_inner();
        if live_causal_transport_enabled() {
            validate_causal_peer_metadata(
                &request_metadata,
                &req.node_id,
                peer_certificate_sha256.as_deref(),
            )?;
        }
        let (display_addr, connect_addr) = normalize_peer_address(&req.address, remote_addr);
        let node_id = req.node_id.clone();

        let mut state = self.state.write().await;
        if !state.is_orchestrator {
            return Err(Status::permission_denied("Not an orchestrator"));
        }

        let stable_registrations = validate_stable_registration_admission(
            &state,
            &node_id,
            &req.network_resources,
            &req.stable_executors,
        )?;
        let stable_capabilities =
            validate_stable_capability_admission(&req.stable_executor_capabilities)?;
        let stable_executors = stable_registrations
            .iter()
            .cloned()
            .map(stable_registration_to_proto)
            .collect::<Vec<_>>();
        let stable_executor_capabilities = stable_capabilities
            .iter()
            .cloned()
            .map(stable_capability_to_proto)
            .collect::<Vec<_>>();

        let active_networks: Vec<String> = req.network_resources.keys().cloned().collect();
        let node_status = NodeStatus {
            node_id: node_id.clone(),
            address: display_addr.clone(),
            resources: req.resources,
            active_networks,
            stable_executors: stable_executors.clone(),
            stable_executor_capabilities: stable_executor_capabilities.clone(),
        };

        state.nodes.insert(node_id.clone(), node_status);
        for registration in &stable_registrations {
            state
                .stable_network_ids
                .insert(registration.network_id.clone());
        }
        state.peers.insert(node_id.clone(), connect_addr.clone());
        // A process may rejoin with the same stable node id after a restart.
        // Refresh its heartbeat here so another node's concurrent heartbeat
        // cannot evict the new session using the previous process timestamp.
        state
            .last_heartbeat
            .insert(node_id.clone(), std::time::Instant::now());
        for (net_id, net_res) in req.network_resources {
            // Auto-register networks reported by the joining worker that the
            // orchestrator does not already know about (e.g. the worker was
            // configured via NM_BRAINS but the orchestrator was not given a
            // matching NM_ORCHESTRATOR_NETWORK_SPECS entry).
            if !state.network_registry.contains_key(&net_id) {
                let num_layers = (net_res.layer_neuron_counts.len() as u32).max(1);
                state.network_registry.insert(
                    net_id.clone(),
                    proto::NetworkStatus {
                        network_id: net_id.clone(),
                        num_layers,
                        total_neurons: net_res.num_neurons,
                        playing: true,
                        ..Default::default()
                    },
                );
            }
            state
                .network_runtime_metrics
                .entry(net_id)
                .or_default()
                .insert(node_id.clone(), net_res);
        }

        // Trigger rebalance when new node joins. Registration callbacks run
        // after releasing the state lock because they may persist management
        // evidence and must never delay another heartbeat.
        let registration_observations = stable_registrations.clone();
        let registration_handler = self
            .stable_worker_registration_handler
            .read()
            .ok()
            .and_then(|handler| handler.clone());
        drop(state);
        if let Some(handler) = registration_handler {
            for registration in registration_observations {
                let handler = Arc::clone(&handler);
                let node_id = node_id.clone();
                tokio::task::spawn_blocking(move || handler(node_id, registration));
            }
        }
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
        let request_metadata = request.metadata().clone();
        let peer_certificate_sha256 = request.peer_certs().and_then(|certificates| {
            certificates
                .first()
                .map(|certificate| certificate_sha256_der(certificate.as_ref()))
        });
        let mut state = self.state.write().await;
        let req = request.into_inner();
        if live_causal_transport_enabled() {
            validate_causal_peer_metadata(
                &request_metadata,
                &req.node_id,
                peer_certificate_sha256.as_deref(),
            )?;
        }
        let now = std::time::Instant::now();

        // A worker can outlive its orchestrator membership record when a slow
        // startup heartbeat is pruned.  Do not acknowledge that worker forever:
        // returning an error makes its connection manager perform Join again,
        // which restores the advertised peer address and resource record.
        if state.is_orchestrator && !state.nodes.contains_key(&req.node_id) {
            return Err(Status::not_found(format!(
                "Node {} is not registered; rejoin required",
                req.node_id
            )));
        }

        let stable_registrations = validate_stable_registration_admission(
            &state,
            &req.node_id,
            &req.network_resources,
            &req.stable_executors,
        )?;
        let stable_capabilities =
            validate_stable_capability_admission(&req.stable_executor_capabilities)?;
        let stable_executors = stable_registrations
            .iter()
            .cloned()
            .map(stable_registration_to_proto)
            .collect::<Vec<_>>();
        let stable_executor_capabilities = stable_capabilities
            .iter()
            .cloned()
            .map(stable_capability_to_proto)
            .collect::<Vec<_>>();
        for registration in &stable_registrations {
            state
                .stable_network_ids
                .insert(registration.network_id.clone());
        }

        state.last_heartbeat.insert(req.node_id.clone(), now);

        let mut commands = Vec::new();
        let mut connect_target: Option<String> = None;
        let mut peer_map = HashMap::new();
        let mut network_peers = HashMap::new();
        let mut needs_rebalance = false;
        let mut new_activation_results = Vec::new();
        if state.is_orchestrator {
            // Stable activation commands use an at-least-once delivery
            // contract. Validate and consume only a matching result; a
            // worker cannot acknowledge another network, request or manifest
            // digest by guessing a request ID. Replayed identical results are
            // harmless after the command has already been consumed.
            for result in &req.command_results {
                validate_command_result(result)?;
                let result_key = (req.node_id.clone(), result.request_id.clone());
                if let Some(previous) = state.stable_activation_results.get(&result_key) {
                    if previous != result {
                        return Err(Status::failed_precondition(
                            "stable activation result conflicts with a previous acknowledgement",
                        ));
                    }
                    continue;
                }
                let Some(queue) = state.pending_commands.get_mut(&req.node_id) else {
                    return Err(Status::failed_precondition(
                        "stable activation result has no pending command",
                    ));
                };
                let matching = queue.iter().position(|command| {
                    stable_activation_command_identity(command).is_some_and(
                        |(network_id, request_id, manifest_digest, placement_idempotency_key)| {
                            network_id == result.network_id
                                && request_id == result.request_id
                                && manifest_digest == result.manifest_digest
                                && placement_idempotency_key == result.placement_idempotency_key
                        },
                    )
                });
                let Some(index) = matching else {
                    return Err(Status::failed_precondition(
                        "stable activation result does not match a pending command",
                    ));
                };
                queue.remove(index);
                if state.stable_activation_results.len() >= MAX_PENDING_COMMANDS_PER_NODE {
                    if let Some(oldest) = state.stable_activation_results.keys().next().cloned() {
                        state.stable_activation_results.remove(&oldest);
                    }
                }
                state
                    .stable_activation_results
                    .insert(result_key, result.clone());
                new_activation_results.push(result.clone());
            }

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
                    state
                        .stable_activation_results
                        .retain(|(worker, _), _| worker != &node_id);
                    state.ga_inflight_by_peer.remove(&node_id);
                    for metrics in state.network_runtime_metrics.values_mut() {
                        metrics.remove(&node_id);
                    }
                    state
                        .network_runtime_metrics
                        .retain(|_, metrics| !metrics.is_empty());
                    for net in state.network_registry.values_mut() {
                        net.distribution.remove(&node_id);
                    }
                }
            }

            let reported_network_ids: HashSet<String> =
                req.network_resources.keys().cloned().collect();
            if let Some(node) = state.nodes.get_mut(&req.node_id) {
                node.resources = req.resources;
                node.active_networks = req.network_resources.keys().cloned().collect();
                node.stable_executors = stable_executors;
                node.stable_executor_capabilities = stable_executor_capabilities;

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

            for metrics in state.network_runtime_metrics.values_mut() {
                metrics.remove(&req.node_id);
            }
            // Update network distribution info with current neuron counts.
            // Also auto-register any networks the worker reports that the
            // orchestrator has not seen yet (handles workers that started
            // before NM_ORCHESTRATOR_NETWORK_SPECS was populated).
            for (net_id, net_res) in req.network_resources {
                if !state.network_registry.contains_key(&net_id) {
                    let num_layers = (net_res.layer_neuron_counts.len() as u32).max(1);
                    state.network_registry.insert(
                        net_id.clone(),
                        proto::NetworkStatus {
                            network_id: net_id.clone(),
                            num_layers,
                            total_neurons: net_res.num_neurons,
                            playing: true,
                            ..Default::default()
                        },
                    );
                    needs_rebalance = true;
                }
                state
                    .network_runtime_metrics
                    .entry(net_id.clone())
                    .or_default()
                    .insert(req.node_id.clone(), net_res.clone());
                if let Some(net_status) = state.network_registry.get_mut(&net_id) {
                    if let Some(range) = net_status.distribution.get_mut(&req.node_id) {
                        range.layer_neuron_counts = net_res.layer_neuron_counts;
                    }
                }
            }
            state.network_runtime_metrics.retain(|net_id, metrics| {
                !metrics.is_empty() || reported_network_ids.contains(net_id)
            });
            state
                .network_runtime_metrics
                .retain(|_, metrics| !metrics.is_empty());

            if let Some(pending) = state.pending_commands.get_mut(&req.node_id) {
                // Legacy commands retain their existing one-shot heartbeat
                // behaviour. Activation commands remain queued until a
                // digest-bound result arrives, so a worker crash between
                // receipt and bootstrap is retried after it rejoins.
                let mut retained = Vec::new();
                for command in std::mem::take(pending) {
                    if stable_activation_command_identity(&command).is_some() {
                        commands.push(command.clone());
                        retained.push(command);
                    } else {
                        commands.push(command);
                    }
                }
                *pending = retained;
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

        let registration_observations = stable_registrations.clone();
        let registration_handler = self
            .stable_worker_registration_handler
            .read()
            .ok()
            .and_then(|handler| handler.clone());
        let registration_node_id = req.node_id.clone();
        let response = Ok(Response::new(HeartbeatResponse {
            acknowledged: true,
            commands,
            peers: peer_map,
            network_peers,
        }));
        drop(state);
        if let Some(handler) = registration_handler {
            for registration in registration_observations {
                let handler = Arc::clone(&handler);
                let node_id = registration_node_id.clone();
                // Management persistence is intentionally outside the
                // heartbeat lock and response path. A slow disk must not
                // block unrelated node heartbeats.
                tokio::task::spawn_blocking(move || handler(node_id, registration));
            }
        }
        if !new_activation_results.is_empty() {
            let handler = self
                .stable_activation_result_handler
                .read()
                .ok()
                .and_then(|handler| handler.clone());
            if let Some(handler) = handler {
                for result in new_activation_results {
                    let handler = Arc::clone(&handler);
                    // Registry persistence may involve filesystem I/O. Keep
                    // it outside the heartbeat state lock and RPC response
                    // path so a slow disk cannot block another node's join.
                    tokio::task::spawn_blocking(move || handler(result));
                }
            }
        }
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
        if live_causal_transport_enabled() {
            return Err(Status::failed_precondition(
                "legacy SpikeBatch transport is disabled by the authoritative causal profile",
            ));
        }
        let remote_addr = request.remote_addr();
        let mut stream = request.into_inner();
        let node = self.clone();

        let (response_keepalive, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            // Keep the response side open for as long as the request stream is
            // alive. Dropping this sender when the RPC handler returned made
            // every nominally persistent stream observe EOF immediately, so
            // peers fell back to short-lived burst streams and eventually hit
            // HTTP/2 ENHANCE_YOUR_CALM/too_many_resets failures.
            let response_keepalive = response_keepalive;
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
            drop(response_keepalive);
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
        let network_id = req.network_id.clone();
        let refreshed_transition_payload =
            if let Some(proto::network_update_request::Update::Config(config_update)) =
                req.update.as_ref()
            {
                if config_update.config_json.is_empty() {
                    None
                } else {
                    let requested_payload =
                        String::from_utf8_lossy(&config_update.config_json).to_string();
                    self.maybe_refresh_manual_transition_payload(&network_id, &requested_payload)
                        .await
                }
            } else {
                None
            };
        let mut state = self.state.write().await;

        if !state.is_orchestrator {
            // Workers have no management authority. Commands must be
            // authorised and recorded by the orchestrator, then delivered
            // through the existing fenced heartbeat command queue. This
            // closes the direct worker mutation bypass during management
            // cutover and prevents a forged worker address from controlling a
            // local shard.
            return Err(Status::permission_denied(
                "management mutations are orchestrator-only",
            ));
        }

        let mut commands_to_send = Vec::new();
        let mut local_control: Option<proto::control_update::Action> = None;
        let mut local_net_arc: Option<Arc<RwLock<ManagedNetwork>>> = None;
        let mut needs_rebalance = false;
        let local_net_arc_candidate = state.networks.get(&network_id).cloned();

        let response = {
            let (network_registry, network_snapshots, pending_commands, last_deployment_transition) = {
                let state = &mut *state;
                (
                    &mut state.network_registry,
                    &mut state.network_snapshots,
                    &mut state.pending_commands,
                    &mut state.last_deployment_transition,
                )
            };
            if let Some(net_status) = network_registry.get_mut(&network_id) {
                if let Some(update) = req.update {
                    match update {
                        proto::network_update_request::Update::Config(c) => {
                            let mut effective_cfg_bytes = c.config_json.clone();
                            if !c.config_json.is_empty() {
                                let effective_cfg_json =
                                    refreshed_transition_payload.clone().unwrap_or_else(|| {
                                        String::from_utf8_lossy(&c.config_json).to_string()
                                    });
                                effective_cfg_bytes = effective_cfg_json.as_bytes().to_vec();
                                let previous_payload = network_snapshots
                                    .get(&network_id)
                                    .filter(|payload| !payload.trim().is_empty())
                                    .map(String::as_str)
                                    .unwrap_or(net_status.config_json.as_str());
                                let previous_deployment =
                                    network_deployment_from_payload(previous_payload)
                                        .unwrap_or_default();
                                let next_deployment =
                                    network_deployment_from_payload(&effective_cfg_json)
                                        .unwrap_or_default();
                                let live_transition_requested =
                                    previous_deployment != next_deployment;
                                let network_is_live = local_net_arc_candidate.is_some()
                                    || !net_status.distribution.is_empty();
                                let manual_transition_allowed = previous_deployment
                                    .allows_live_transition()
                                    || next_deployment.allows_live_transition();
                                if live_transition_requested
                                    && network_is_live
                                    && !manual_transition_allowed
                                {
                                    return Err(Status::failed_precondition(format!(
                                        "live deployment transition for '{}' requires deployment.transition_policy.allow_live_transition=true",
                                        network_id
                                    )));
                                }
                                net_status.config_json = effective_cfg_json.clone();
                                sync_network_status_deployment(net_status, &next_deployment);
                                if let Ok(snap) =
                                    crate::runner::decode_snapshot_with_profile_backfill(
                                        &effective_cfg_json,
                                    )
                                {
                                    network_snapshots
                                        .insert(network_id.clone(), effective_cfg_json.clone());
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
                                if live_transition_requested {
                                    needs_rebalance = true;
                                    let transition_record = DeploymentTransitionRecord {
                                        observed_at: std::time::Instant::now(),
                                        ts_ms: unix_timestamp_ms_now(),
                                        reason: format!(
                                            "manual deployment transition: {} -> {}",
                                            deployment_modes_label(&previous_deployment),
                                            deployment_modes_label(&next_deployment)
                                        ),
                                        source: "manual".to_string(),
                                    };
                                    sync_network_status_transition(
                                        net_status,
                                        Some(&transition_record),
                                    );
                                    last_deployment_transition
                                        .insert(network_id.clone(), transition_record);
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
                                    config_json: effective_cfg_bytes.clone(),
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
                                sync_network_status_deployment(net_status, &fresh_cfg.deployment);
                                sync_network_status_transition(net_status, None);
                                last_deployment_transition.remove(&network_id);
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
        let mut networks = state.network_registry.values().cloned().collect::<Vec<_>>();
        for status in &mut networks {
            let network_id = status.network_id.clone();
            let payload = state
                .network_snapshots
                .get(&network_id)
                .filter(|payload| !payload.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| status.config_json.clone());
            sync_network_status_deployment_from_payload_with_transition(
                status,
                &payload,
                state.last_deployment_transition.get(&network_id),
            );
        }
        Ok(Response::new(StatusResponse {
            nodes: state.nodes.values().cloned().collect(),
            networks,
        }))
    }

    async fn run_ga_evaluation(
        &self,
        request: Request<GaEvaluationRequest>,
    ) -> Result<Response<GaEvaluationResponse>, Status> {
        if live_causal_transport_enabled() {
            validate_live_request(&request)?;
        }
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
            let sender_node_id = self.state.read().await.node_id.clone();
            let req_fwd = authenticated_request(req_fwd, &sender_node_id).map_err(|error| {
                Status::failed_precondition(format!(
                    "cannot authenticate GA evaluation forwarding request: {error}"
                ))
            })?;
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
        if live_causal_transport_enabled() {
            validate_live_request(&request)?;
        }
        let req = request.into_inner();
        let net_id = req.network_id.clone();
        let cut_epoch = req.cut_epoch;
        let net_id_for_cut = net_id.clone();
        let net_arc = {
            let state = self.state.read().await;
            state.networks.get(&req.network_id).cloned()
        };

        let Some(net_arc) = net_arc else {
            return Err(Status::not_found("network not hosted on this node"));
        };
        let local_node_id = self.state.read().await.node_id.clone();

        let (
            snapshot_json,
            channel_state_json,
            authoritative_state_json,
            step,
            sim_time_ms_bits,
            cut_evidence,
        ) = tokio::task::spawn_blocking(move || {
            let net = net_arc.blocking_read();
            let (snapshot_json, channel_state_json, step, sim_time_ms_bits) =
                local_shard_snapshot(&net)?;
            let authoritative_state_json = local_authoritative_state_json(&net)?;
            let cut_evidence = if cut_epoch == 0 {
                None
            } else {
                Some(local_cut_evidence(
                    &net_id_for_cut,
                    &local_node_id,
                    cut_epoch,
                    &snapshot_json,
                    &channel_state_json,
                )?)
            };
            Ok::<_, String>((
                snapshot_json,
                channel_state_json,
                authoritative_state_json,
                step,
                sim_time_ms_bits,
                cut_evidence,
            ))
        })
        .await
        .map_err(|e| Status::internal(format!("snapshot task failed: {}", e)))?
        .map_err(|e| Status::internal(format!("snapshot export failed: {}", e)))?;

        let (participant_json, channel_marker_json) = match cut_evidence {
            Some((participant, marker)) => (
                serde_json::to_string(&participant)
                    .map_err(|error| Status::internal(error.to_string()))?,
                serde_json::to_string(&marker)
                    .map_err(|error| Status::internal(error.to_string()))?,
            ),
            None => (String::new(), String::new()),
        };
        Ok(Response::new(NetworkSnapshotResponse {
            network_id: net_id,
            snapshot_json,
            step,
            sim_time_ms_bits,
            channel_state_json,
            authoritative_state_json,
            cut_epoch,
            participant_json,
            channel_marker_json,
        }))
    }

    async fn get_cluster_network_snapshot(
        &self,
        request: Request<ClusterNetworkSnapshotRequest>,
    ) -> Result<Response<ClusterNetworkSnapshotResponse>, Status> {
        if live_causal_transport_enabled() {
            validate_live_request(&request)?;
        }
        let network_id = request.into_inner().network_id;
        let (node_id, expected_assignment, local_network, clients, addresses, is_orchestrator) = {
            let state = self.state.read().await;
            let Some(status) = state.network_registry.get(&network_id) else {
                return Err(Status::not_found(
                    "network is not registered on the orchestrator",
                ));
            };
            let expected_assignment = status
                .distribution
                .iter()
                .map(|(node_id, range)| (node_id.clone(), range.layers.clone()))
                .collect::<std::collections::BTreeMap<_, _>>();
            let addresses = expected_assignment
                .keys()
                .filter(|candidate| candidate.as_str() != state.node_id)
                .map(|candidate| (candidate.clone(), connect_addr_for_node(&state, candidate)))
                .collect::<HashMap<_, _>>();
            (
                state.node_id.clone(),
                expected_assignment,
                state.networks.get(&network_id).cloned(),
                state.clients.clone(),
                addresses,
                state.is_orchestrator,
            )
        };
        if !is_orchestrator {
            return Err(Status::permission_denied(
                "cluster snapshots may only be assembled by the orchestrator",
            ));
        }

        // A configured evidence root makes the control-plane epoch survive a
        // restart. Without it, retain the in-memory compatibility counter;
        // neither mode uses wall-clock time as biological time.
        let cut_store = match std::env::var_os("NM_CONSISTENT_CUT_ROOT") {
            Some(root) => Some(
                tokio::task::spawn_blocking(move || {
                    crate::consistent_cut::FileConsistentCutStore::new(PathBuf::from(root))
                })
                .await
                .map_err(|error| {
                    Status::internal(format!("consistent-cut store task failed: {error}"))
                })?
                .map_err(|error| Status::failed_precondition(error.to_string()))?,
            ),
            None => None,
        };
        let snapshot_store = match std::env::var_os("NM_CLUSTER_SNAPSHOT_ROOT") {
            Some(root) => Some(
                tokio::task::spawn_blocking(move || {
                    crate::cluster_snapshot::FileClusterSnapshotStore::new(PathBuf::from(root))
                })
                .await
                .map_err(|error| {
                    Status::internal(format!("cluster snapshot store task failed: {error}"))
                })?
                .map_err(|error| Status::failed_precondition(error.to_string()))?,
            ),
            None => None,
        };
        let cut_epoch = if let Some(store) = cut_store.as_ref() {
            let store = store.clone();
            tokio::task::spawn_blocking(move || store.next_epoch())
                .await
                .map_err(|error| {
                    Status::internal(format!("consistent-cut epoch task failed: {error}"))
                })?
                .map_err(|error| Status::failed_precondition(error.to_string()))?
        } else {
            let mut state = self.state.write().await;
            let epoch = state
                .consistent_cut_epochs
                .entry(network_id.clone())
                .or_insert(0);
            *epoch = epoch
                .checked_add(1)
                .ok_or_else(|| Status::failed_precondition("consistent-cut epoch exhausted"))?;
            *epoch
        };
        if cut_store.is_some() {
            self.state
                .write()
                .await
                .consistent_cut_epochs
                .insert(network_id.clone(), cut_epoch);
        }

        let (cut_sender, cut_receiver) =
            tokio::sync::mpsc::channel(expected_assignment.len().saturating_mul(2).max(1));
        let cut_participants = expected_assignment.keys().cloned().collect::<Vec<_>>();
        let cut_channels = expected_assignment
            .keys()
            .map(|node_id| format!("{network_id}/{node_id}"))
            .collect::<Vec<_>>();
        // Start the collector before any shard request. Evidence is consumed
        // as independent RPCs finish, so a slow shard does not make the
        // control path pretend that all markers arrived together.
        let cut_task = if let Some(store) = cut_store.as_ref() {
            let store = store.clone();
            let participants = cut_participants.clone();
            let channels = cut_channels.clone();
            let coordinator =
                tokio::task::spawn_blocking(move || store.begin(cut_epoch, participants, channels))
                    .await
                    .map_err(|error| {
                        Status::internal(format!("consistent-cut coordinator task failed: {error}"))
                    })?
                    .map_err(|error| Status::failed_precondition(error.to_string()))?;
            tokio::spawn(async move {
                AsyncConsistentCutCollector::new_persisted(coordinator, cut_receiver)
                    .finalise()
                    .await
            })
        } else {
            let coordinator =
                ConsistentCutCoordinator::begin(cut_epoch, cut_participants, cut_channels)
                    .map_err(|error| Status::failed_precondition(error.to_string()))?;
            tokio::spawn(async move {
                AsyncConsistentCutCollector::new(coordinator, cut_receiver)
                    .finalise()
                    .await
            })
        };
        let mut tasks = tokio::task::JoinSet::new();
        for (shard_node_id, layers) in expected_assignment.iter() {
            if shard_node_id == &node_id {
                let Some(network) = local_network.clone() else {
                    continue;
                };
                let shard_node_id = shard_node_id.clone();
                let layers = layers.clone();
                let network_id_for_task = network_id.clone();
                let cut_sender = cut_sender.clone();
                tasks.spawn(async move {
                    let (input, evidence) = tokio::task::spawn_blocking(move || {
                        let network = network.blocking_read();
                        let (snapshot_json, channel_state_json, _step, _sim_time_ms_bits) =
                            local_shard_snapshot(&network)?;
                        let authoritative_state_json = local_authoritative_state_json(&network)?;
                        let (participant, marker) = local_cut_evidence(
                            &network_id_for_task,
                            &shard_node_id,
                            cut_epoch,
                            &snapshot_json,
                            &channel_state_json,
                        )?;
                        Ok::<_, String>((
                            ShardSnapshotInput {
                                node_id: shard_node_id,
                                layers,
                                snapshot_json,
                                channel_state_json,
                                authoritative_state_json,
                            },
                            cluster_snapshot::LiveCutEvidence {
                                participant,
                                marker,
                            },
                        ))
                    })
                    .await
                    .map_err(|error| error.to_string())??;
                    cut_sender
                        .send(ConsistentCutMessage::Participant(
                            evidence.participant.clone(),
                        ))
                        .await
                        .map_err(|_| "consistent-cut collector closed".to_owned())?;
                    cut_sender
                        .send(ConsistentCutMessage::Marker(evidence.marker.clone()))
                        .await
                        .map_err(|_| "consistent-cut collector closed".to_owned())?;
                    Ok::<_, String>((input, evidence))
                });
                continue;
            }

            let Some(address) = addresses.get(shard_node_id).and_then(Clone::clone) else {
                continue;
            };
            let client = clients.get(shard_node_id).cloned();
            let network_id_for_task = network_id.clone();
            let shard_node_id = shard_node_id.clone();
            let layers = layers.clone();
            let cut_sender = cut_sender.clone();
            let sender_node_id = node_id.clone();
            tasks.spawn(async move {
                let mut client = match client {
                    Some(client) => client,
                    None => connect_peer_with_timeout(&address, Duration::from_secs(2))
                        .await
                        .map_err(|error| error.to_string())?,
                };
                let request = authenticated_request(
                    NetworkSnapshotRequest {
                        network_id: network_id_for_task.clone(),
                        cut_epoch,
                    },
                    &sender_node_id,
                )
                .map_err(|error| error.to_string())?;
                let response = tokio::time::timeout(
                    Duration::from_secs(3),
                    client.get_network_snapshot(request),
                )
                .await
                .map_err(|_| "shard snapshot request timed out".to_owned())?
                .map_err(|error| error.to_string())?
                .into_inner();
                if response.network_id != network_id_for_task {
                    return Err(format!(
                        "shard returned network '{}' instead of requested network",
                        response.network_id
                    ));
                }
                if response.cut_epoch != cut_epoch
                    || response.participant_json.is_empty()
                    || response.channel_marker_json.is_empty()
                {
                    return Err("shard did not return matching consistent-cut evidence".to_owned());
                }
                let participant = serde_json::from_str(&response.participant_json)
                    .map_err(|error| format!("invalid participant evidence: {error}"))?;
                let marker = serde_json::from_str(&response.channel_marker_json)
                    .map_err(|error| format!("invalid channel marker evidence: {error}"))?;
                let evidence = cluster_snapshot::LiveCutEvidence {
                    participant,
                    marker,
                };
                cut_sender
                    .send(ConsistentCutMessage::Participant(
                        evidence.participant.clone(),
                    ))
                    .await
                    .map_err(|_| "consistent-cut collector closed".to_owned())?;
                cut_sender
                    .send(ConsistentCutMessage::Marker(evidence.marker.clone()))
                    .await
                    .map_err(|_| "consistent-cut collector closed".to_owned())?;
                Ok::<_, String>((
                    ShardSnapshotInput {
                        node_id: shard_node_id,
                        layers,
                        snapshot_json: response.snapshot_json,
                        channel_state_json: response.channel_state_json,
                        authoritative_state_json: response.authoritative_state_json,
                    },
                    evidence,
                ))
            });
        }
        drop(cut_sender);

        let mut inputs = Vec::with_capacity(expected_assignment.len());
        let mut evidence = Vec::with_capacity(expected_assignment.len());
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok((input, cut_evidence))) => {
                    inputs.push(input);
                    evidence.push(cut_evidence);
                }
                Ok(Err(error)) => {
                    return Err(Status::failed_precondition(format!(
                        "cluster snapshot shard unavailable: {}",
                        error
                    )));
                }
                Err(error) => {
                    return Err(Status::internal(format!(
                        "cluster snapshot task failed: {}",
                        error
                    )));
                }
            }
        }

        let cut = cut_task
            .await
            .map_err(|error| Status::internal(format!("consistent-cut task failed: {error}")))?
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let snapshot = cluster_snapshot::assemble_live(
            network_id,
            &expected_assignment,
            inputs,
            evidence,
            cut,
        )
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if let Some(store) = snapshot_store {
            let snapshot_to_publish = snapshot.clone();
            tokio::task::spawn_blocking(move || store.publish_idempotent(&snapshot_to_publish))
                .await
                .map_err(|error| {
                    Status::internal(format!("cluster snapshot publish task failed: {error}"))
                })?
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
        }
        if let Some(store) = cut_store {
            let cut = snapshot
                .consistent_cut
                .clone()
                .ok_or_else(|| Status::internal("live cluster snapshot omitted cut evidence"))?;
            tokio::task::spawn_blocking(move || store.publish(&cut))
                .await
                .map_err(|error| {
                    Status::internal(format!("consistent-cut publish task failed: {error}"))
                })?
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
        }
        Ok(Response::new(ClusterNetworkSnapshotResponse {
            network_id: snapshot.network_id,
            schema_version: snapshot.schema_version,
            cut_tick: snapshot.cut_tag.tick,
            cut_microstep: snapshot.cut_tag.microstep,
            cluster_digest: snapshot.cluster_digest.to_string(),
            shards: snapshot
                .shards
                .into_iter()
                .map(|shard| ClusterShardSnapshot {
                    node_id: shard.node_id,
                    layers: shard.layers,
                    snapshot_json: shard.snapshot_json,
                    step: shard.step,
                    sim_time_ms_bits: shard.sim_time_ms_bits,
                    state_digest: shard.state_digest.to_string(),
                    channel_state_json: shard.channel_state_json,
                    channel_state_digest: shard.channel_state_digest.to_string(),
                    authoritative_state_json: shard.authoritative_state_json,
                    authoritative_state_digest: shard
                        .authoritative_state_digest
                        .map(|digest| digest.to_string())
                        .unwrap_or_default(),
                })
                .collect(),
            consistent_cut_json: snapshot
                .consistent_cut
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| Status::internal(error.to_string()))?
                .unwrap_or_default(),
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

        let (hidden, output, output_history, sim_step, sim_time_ms) =
            tokio::task::spawn_blocking(move || {
                let net = net_arc.blocking_read();
                let ts_us = (net.runner.t_ms * 1000.0) as u64;
                let sim_step = net.runner.t as u64;
                let sim_time_ms = net.runner.t_ms;
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
                let output_history = net
                    .runner
                    .spk_hist_o
                    .iter()
                    .take(128)
                    .map(|frame| {
                        let frame_vec: Vec<i8> = frame.iter().copied().collect();
                        let exchange = encode_exchange(ts_us, 0, &frame_vec);
                        SpikeIndices {
                            indices: exchange.spike_indices,
                            aer_payload: exchange.aer_payload,
                            aer_base: exchange.aer_base,
                        }
                    })
                    .collect::<Vec<_>>();
                (hidden, output, output_history, sim_step, sim_time_ms)
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
            sim_step,
            sim_time_ms,
            output_history,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn production_cutover_requires_a_durable_cluster_snapshot_catalogue() {
        assert!(validate_cluster_snapshot_root(Some("/var/lib/aarnn/cuts")).is_ok());
        assert!(
            validate_cluster_snapshot_root(None)
                .expect_err("production needs a snapshot catalogue")
                .contains("NM_CLUSTER_SNAPSHOT_ROOT")
        );
        assert!(
            validate_cluster_snapshot_root(Some("  "))
                .expect_err("an empty snapshot root is invalid")
                .contains("must not be empty")
        );
    }

    #[tokio::test]
    async fn rejoin_refreshes_a_stale_heartbeat_before_rebalancing() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_string(), true);
        let stale = std::time::Instant::now()
            .checked_sub(PEER_STALE_AFTER + Duration::from_secs(1))
            .expect("stale timestamp");
        node.state
            .write()
            .await
            .last_heartbeat
            .insert("native-qc04".to_string(), stale);

        node.join(Request::new(JoinRequest {
            node_id: "native-qc04".to_string(),
            address: "127.0.0.1:65534".to_string(),
            resources: None,
            network_resources: HashMap::new(),
            stable_executors: Vec::new(),
            stable_executor_capabilities: Vec::new(),
        }))
        .await
        .expect("rejoin succeeds");

        let state = node.state.read().await;
        assert!(state.nodes.contains_key("native-qc04"));
        assert!(
            state
                .last_heartbeat
                .get("native-qc04")
                .is_some_and(|seen| seen.elapsed() < PEER_STALE_AFTER)
        );
    }

    #[tokio::test]
    async fn unknown_worker_heartbeat_requires_rejoin() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_string(), true);
        let err = node
            .heartbeat(Request::new(HeartbeatRequest {
                node_id: "pruned-worker".to_string(),
                resources: None,
                network_resources: HashMap::new(),
                stable_executors: Vec::new(),
                stable_executor_capabilities: Vec::new(),
                command_results: Vec::new(),
            }))
            .await
            .expect_err("an unknown worker must rejoin");

        assert_eq!(err.code(), tonic::Code::NotFound);
        assert!(
            !node
                .state
                .read()
                .await
                .last_heartbeat
                .contains_key("pruned-worker")
        );
    }

    #[cfg(feature = "stable_executor_live")]
    fn stable_registration_wire(
        network_id: &str,
        shard_ids: Vec<u64>,
    ) -> StableExecutorRegistration {
        StableExecutorRegistration {
            schema_version: crate::stable_worker::STABLE_WORKER_REGISTRATION_SCHEMA_VERSION,
            profile: crate::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
            network_id: network_id.to_owned(),
            brain_id: crate::managed_durability::managed_brain_id(network_id).raw(),
            topology_generation: 1,
            partition_generation: 1,
            topology_digest: "00".repeat(16),
            plan_digest: "11".repeat(16),
            owned_shard_ids: shard_ids.clone(),
            shard_ids: shard_ids.clone(),
            lease_term: 1,
            fencing_token: 7,
            current_tick: 0,
            current_microstep: 0,
            state_digest: "22".repeat(16),
            max_input_events: 8,
            max_steps_per_poll: 8,
            authoritative: true,
            application_acks: shard_ids
                .iter()
                .map(|shard_id| proto::StableShardApplicationAck {
                    shard_id: *shard_id,
                    brain_id: crate::managed_durability::managed_brain_id(network_id).raw(),
                    topology_generation: 1,
                    partition_generation: 1,
                    plan_digest: "11".repeat(16),
                    lease_term: 1,
                    fencing_token: 7,
                    applied_tick: 0,
                    applied_microstep: 0,
                    state_digest: "33".repeat(16),
                    durable_wal_sequence: 0,
                    durable_wal_sequence_present: true,
                    committed: true,
                })
                .collect(),
        }
    }

    #[cfg(feature = "stable_executor_live")]
    fn stable_capability_wire() -> proto::StableExecutorCapability {
        proto::StableExecutorCapability {
            schema_version: crate::stable_worker::STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
            profile: crate::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
            activation_schema_version:
                crate::stable_worker::STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
            max_input_events: crate::stable_worker::DEFAULT_STABLE_WORKER_MAX_INPUT_EVENTS,
            max_steps_per_poll: crate::stable_worker::DEFAULT_STABLE_WORKER_MAX_STEPS_PER_POLL,
        }
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_worker_join_is_registered_and_legacy_rebalance_is_fenced() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        node.join(Request::new(JoinRequest {
            node_id: "stable-worker".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![stable_registration_wire("alpha", vec![1, 2])],
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("stable worker join");

        let state = node.state.read().await;
        let worker = state.nodes.get("stable-worker").expect("worker status");
        assert_eq!(worker.stable_executors.len(), 1);
        assert_eq!(worker.stable_executors[0].shard_ids, vec![1, 2]);
        assert_eq!(worker.stable_executors[0].owned_shard_ids, vec![1, 2]);
        assert!(state.stable_network_ids.contains("alpha"));
        let network = state.network_registry.get("alpha").expect("network status");
        assert!(network.distribution.is_empty());
        assert!(
            network
                .deployment_modes
                .iter()
                .any(|mode| mode == "stable-executor")
        );
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_worker_activation_queue_requires_capability_and_is_idempotent() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        node.join(Request::new(JoinRequest {
            node_id: "stable-worker".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![stable_registration_wire("alpha", vec![1, 2])],
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("stable worker join");

        let brain_id = crate::managed_durability::managed_brain_id("alpha");
        let command = crate::stable_worker::StableWorkerActivationCommand::new(
            "activation-request-1",
            1,
            brain_id.raw(),
            "alpha",
            "stable-worker",
            "{}",
        )
        .expect("activation command");
        assert!(
            node.queue_stable_worker_activation(command.clone())
                .await
                .expect("activation queued")
        );
        assert!(
            !node
                .queue_stable_worker_activation(command)
                .await
                .expect("activation replay is idempotent")
        );

        let state = node.state.read().await;
        let queued = state
            .pending_commands
            .get("stable-worker")
            .expect("pending worker commands");
        assert_eq!(queued.len(), 1);
        assert_eq!(
            queued[0].r#type,
            proto::network_command::CommandType::ActivateStableWorker as i32
        );
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_activation_delivery_replays_until_digest_bound_result_arrives() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        node.join(Request::new(JoinRequest {
            node_id: "stable-worker".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![stable_registration_wire("alpha", vec![1, 2])],
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("stable worker join");

        let brain_id = crate::managed_durability::managed_brain_id("alpha");
        let activation = crate::stable_worker::StableWorkerActivationCommand::new(
            "activation-at-least-once",
            19,
            brain_id.raw(),
            "alpha",
            "stable-worker",
            "{}",
        )
        .expect("activation command");
        let manifest_digest = activation.manifest_digest.clone();
        node.queue_stable_worker_activation(activation)
            .await
            .expect("activation queued");

        let heartbeat = |command_results| HeartbeatRequest {
            node_id: "stable-worker".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![stable_registration_wire("alpha", vec![1, 2])],
            stable_executor_capabilities: vec![stable_capability_wire()],
            command_results,
        };

        let first = node
            .heartbeat(Request::new(heartbeat(Vec::new())))
            .await
            .expect("first heartbeat")
            .into_inner();
        assert_eq!(first.commands.len(), 1);
        let delivered = first.commands[0].clone();

        // A lost response must not lose the activation command.
        let retry = node
            .heartbeat(Request::new(heartbeat(Vec::new())))
            .await
            .expect("retry heartbeat")
            .into_inner();
        assert_eq!(retry.commands, vec![delivered.clone()]);

        let forged_result = NetworkCommandResult {
            command_type: proto::network_command::CommandType::ActivateStableWorker as i32,
            network_id: "alpha".to_owned(),
            request_id: "activation-at-least-once".to_owned(),
            manifest_digest: "00".repeat(32),
            accepted: true,
            error: String::new(),
            brain_id: brain_id.raw(),
            placement_idempotency_key: String::new(),
        };
        let forged_error = node
            .heartbeat(Request::new(heartbeat(vec![forged_result])))
            .await
            .expect_err("a mismatched manifest digest must not acknowledge delivery");
        assert_eq!(forged_error.code(), tonic::Code::FailedPrecondition);

        let result = NetworkCommandResult {
            command_type: proto::network_command::CommandType::ActivateStableWorker as i32,
            network_id: "alpha".to_owned(),
            request_id: "activation-at-least-once".to_owned(),
            manifest_digest,
            accepted: true,
            error: String::new(),
            brain_id: brain_id.raw(),
            placement_idempotency_key: String::new(),
        };
        let acknowledged = node
            .heartbeat(Request::new(heartbeat(vec![result.clone()])))
            .await
            .expect("acknowledgement heartbeat")
            .into_inner();
        assert!(acknowledged.commands.is_empty());

        // Replaying the same result after a lost acknowledgement is
        // idempotent and does not resurrect the command.
        let replay = node
            .heartbeat(Request::new(heartbeat(vec![result])))
            .await
            .expect("replayed acknowledgement heartbeat")
            .into_inner();
        assert!(replay.commands.is_empty());

        // A terminal worker rejection is also consumed, while its bounded
        // diagnostic remains available for the orchestrator's audit hook.
        let failed_activation = crate::stable_worker::StableWorkerActivationCommand::new(
            "activation-terminal-failure",
            20,
            brain_id.raw(),
            "alpha",
            "stable-worker",
            "{}",
        )
        .expect("second activation command");
        let failed_digest = failed_activation.manifest_digest.clone();
        node.queue_stable_worker_activation(failed_activation)
            .await
            .expect("second activation queued");
        let second_delivery = node
            .heartbeat(Request::new(heartbeat(Vec::new())))
            .await
            .expect("second delivery heartbeat")
            .into_inner();
        assert_eq!(second_delivery.commands.len(), 1);
        let failed_result = NetworkCommandResult {
            command_type: proto::network_command::CommandType::ActivateStableWorker as i32,
            network_id: "alpha".to_owned(),
            request_id: "activation-terminal-failure".to_owned(),
            manifest_digest: failed_digest,
            accepted: false,
            error: "checkpoint no longer available".to_owned(),
            brain_id: brain_id.raw(),
            placement_idempotency_key: String::new(),
        };
        let failed_ack = node
            .heartbeat(Request::new(heartbeat(vec![failed_result])))
            .await
            .expect("terminal failure heartbeat")
            .into_inner();
        assert!(failed_ack.commands.is_empty());
        let state = node.state.read().await;
        assert!(
            state
                .pending_commands
                .get("stable-worker")
                .map(Vec::is_empty)
                .unwrap_or(true)
        );
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn idle_enrolled_worker_can_receive_its_first_stable_activation() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        node.join(Request::new(JoinRequest {
            node_id: "idle-worker".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: Vec::new(),
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("idle worker enrollment");

        let state = node.state.read().await;
        let worker = state.nodes.get("idle-worker").expect("worker status");
        assert!(worker.stable_executors.is_empty());
        assert_eq!(worker.stable_executor_capabilities.len(), 1);
        drop(state);

        let brain_id = crate::managed_durability::managed_brain_id("alpha");
        let command = crate::stable_worker::StableWorkerActivationCommand::new(
            "idle-activation-request",
            2,
            brain_id.raw(),
            "alpha",
            "idle-worker",
            "{}",
        )
        .expect("activation command");
        assert!(
            node.queue_stable_worker_activation(command)
                .await
                .expect("idle worker activation queued")
        );
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_registration_allows_disjoint_workers_after_queued_activation() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        let mut first = stable_registration_wire("alpha", vec![1, 2]);
        first.owned_shard_ids = vec![1];
        first.application_acks.truncate(1);
        node.join(Request::new(JoinRequest {
            node_id: "stable-worker-a".to_owned(),
            address: "127.0.0.1:65531".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![first],
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("first stable worker join");

        node.join(Request::new(JoinRequest {
            node_id: "stable-worker-b".to_owned(),
            address: "127.0.0.1:65532".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: Vec::new(),
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("second stable worker enrollment");

        let brain_id = crate::managed_durability::managed_brain_id("alpha");
        let activation = crate::stable_worker::StableWorkerActivationCommand::new(
            "disjoint-worker-b-activation",
            88,
            brain_id.raw(),
            "alpha",
            "stable-worker-b",
            "{}",
        )
        .expect("activation command");
        let manifest_digest = activation.manifest_digest.clone();
        node.queue_stable_worker_activation(activation)
            .await
            .expect("second worker activation queued");

        let mut second = stable_registration_wire("alpha", vec![1, 2]);
        second.owned_shard_ids = vec![2];
        second.application_acks = vec![second.application_acks.remove(1)];
        let result = NetworkCommandResult {
            command_type: proto::network_command::CommandType::ActivateStableWorker as i32,
            network_id: "alpha".to_owned(),
            request_id: "disjoint-worker-b-activation".to_owned(),
            manifest_digest,
            accepted: true,
            error: String::new(),
            brain_id: brain_id.raw(),
            placement_idempotency_key: String::new(),
        };
        node.heartbeat(Request::new(HeartbeatRequest {
            node_id: "stable-worker-b".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![second],
            stable_executor_capabilities: vec![stable_capability_wire()],
            command_results: vec![result],
        }))
        .await
        .expect("disjoint stable registration");

        let state = node.state.read().await;
        assert_eq!(
            state.nodes["stable-worker-a"].stable_executors[0].owned_shard_ids,
            vec![1]
        );
        assert_eq!(
            state.nodes["stable-worker-b"].stable_executors[0].owned_shard_ids,
            vec![2]
        );
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn resource_or_network_observation_without_activation_capability_is_denied() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        node.join(Request::new(JoinRequest {
            node_id: "unqualified-worker".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: Vec::new(),
            stable_executor_capabilities: Vec::new(),
        }))
        .await
        .expect("worker enrollment");

        let brain_id = crate::managed_durability::managed_brain_id("alpha");
        let command = crate::stable_worker::StableWorkerActivationCommand::new(
            "denied-activation-request",
            3,
            brain_id.raw(),
            "alpha",
            "unqualified-worker",
            "{}",
        )
        .expect("activation command");
        let error = node
            .queue_stable_worker_activation(command)
            .await
            .expect_err("resource telemetry must not grant activation capability");
        assert!(error.contains("activation capability"));
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_worker_registration_rejects_duplicate_owner_and_bad_shape() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        let resources = HashMap::from([(
            "alpha".to_owned(),
            NetworkResources {
                num_neurons: 2,
                layer_neuron_counts: HashMap::from([(0, 2)]),
                avg_step_time_ms: 1.0,
            },
        )]);
        let first = JoinRequest {
            node_id: "stable-a".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: resources.clone(),
            stable_executors: vec![stable_registration_wire("alpha", vec![1])],
            stable_executor_capabilities: vec![stable_capability_wire()],
        };
        node.join(Request::new(first)).await.expect("first join");

        let duplicate = node
            .join(Request::new(JoinRequest {
                node_id: "stable-b".to_owned(),
                address: "127.0.0.1:65535".to_owned(),
                resources: Some(Resources::default()),
                network_resources: resources.clone(),
                stable_executors: vec![stable_registration_wire("alpha", vec![1])],
                stable_executor_capabilities: vec![stable_capability_wire()],
            }))
            .await
            .expect_err("one stable writer per network");
        assert_eq!(duplicate.code(), tonic::Code::FailedPrecondition);

        let mut invalid = stable_registration_wire("beta", vec![2, 1]);
        invalid.authoritative = false;
        let error = node
            .join(Request::new(JoinRequest {
                node_id: "invalid".to_owned(),
                address: "127.0.0.1:65536".to_owned(),
                resources: Some(Resources::default()),
                network_resources: HashMap::from([(
                    "beta".to_owned(),
                    NetworkResources::default(),
                )]),
                stable_executors: vec![invalid],
                stable_executor_capabilities: vec![stable_capability_wire()],
            }))
            .await
            .expect_err("invalid stable registration");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_worker_registration_rejects_missing_or_stale_application_evidence() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        let resources = |network_id: &str| {
            HashMap::from([(
                network_id.to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )])
        };

        let mut missing = stable_registration_wire("missing-acks", vec![1]);
        missing.application_acks.clear();
        let error = node
            .join(Request::new(JoinRequest {
                node_id: "missing-acks-worker".to_owned(),
                address: "127.0.0.1:65536".to_owned(),
                resources: Some(Resources::default()),
                network_resources: resources("missing-acks"),
                stable_executors: vec![missing],
                stable_executor_capabilities: vec![stable_capability_wire()],
            }))
            .await
            .expect_err("a worker without a durable ack set must not join");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);

        let mut stale_plan = stable_registration_wire("stale-plan", vec![1]);
        stale_plan.application_acks[0].plan_digest = "44".repeat(16);
        let error = node
            .join(Request::new(JoinRequest {
                node_id: "stale-plan-worker".to_owned(),
                address: "127.0.0.1:65537".to_owned(),
                resources: Some(Resources::default()),
                network_resources: resources("stale-plan"),
                stable_executors: vec![stale_plan],
                stable_executor_capabilities: vec![stable_capability_wire()],
            }))
            .await
            .expect_err("a durable ack for another plan must not join");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);

        let mut stale_fence = stable_registration_wire("stale-fence", vec![1]);
        stale_fence.application_acks[0].fencing_token = 6;
        let error = node
            .join(Request::new(JoinRequest {
                node_id: "stale-fence-worker".to_owned(),
                address: "127.0.0.1:65538".to_owned(),
                resources: Some(Resources::default()),
                network_resources: resources("stale-fence"),
                stable_executors: vec![stale_fence],
                stable_executor_capabilities: vec![stable_capability_wire()],
            }))
            .await
            .expect_err("a durable ack from an older fence must not join");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[cfg(feature = "stable_executor_live")]
    #[tokio::test]
    async fn stable_worker_ownership_subset_can_change_without_plan_identity_change() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_owned(), true);
        let resources = HashMap::from([(
            "alpha".to_owned(),
            NetworkResources {
                num_neurons: 2,
                layer_neuron_counts: HashMap::from([(0, 2)]),
                avg_step_time_ms: 1.0,
            },
        )]);
        node.join(Request::new(JoinRequest {
            node_id: "stable-worker".to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: resources.clone(),
            stable_executors: vec![stable_registration_wire("alpha", vec![1, 2])],
            stable_executor_capabilities: vec![stable_capability_wire()],
        }))
        .await
        .expect("initial stable worker join");

        let mut update = stable_registration_wire("alpha", vec![1, 2]);
        update.owned_shard_ids = vec![2];
        update.application_acks.retain(|ack| ack.shard_id == 2);
        let rejected = update.clone();
        let error = node
            .heartbeat(Request::new(HeartbeatRequest {
                node_id: "stable-worker".to_owned(),
                resources: Some(Resources::default()),
                network_resources: resources.clone(),
                stable_executors: vec![rejected],
                stable_executor_capabilities: vec![stable_capability_wire()],
                command_results: Vec::new(),
            }))
            .await
            .expect_err("ownership cannot change without a fenced boundary");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        update.lease_term = 2;
        update.fencing_token = 8;
        update.application_acks[0].lease_term = 2;
        update.application_acks[0].fencing_token = 8;
        node.heartbeat(Request::new(HeartbeatRequest {
            node_id: "stable-worker".to_owned(),
            resources: Some(Resources::default()),
            network_resources: resources,
            stable_executors: vec![update.clone()],
            stable_executor_capabilities: vec![stable_capability_wire()],
            command_results: Vec::new(),
        }))
        .await
        .expect("ownership telemetry update");

        let state = node.state.read().await;
        let registration = &state.nodes["stable-worker"].stable_executors[0];
        assert_eq!(registration.shard_ids, vec![1, 2]);
        assert_eq!(registration.owned_shard_ids, vec![2]);
        drop(state);

        // A completed source-side drain is represented by an empty owned set
        // and no acknowledgements. It still requires a newer fenced boundary
        // so a stale worker cannot silently reclaim the source shard.
        update.owned_shard_ids.clear();
        update.application_acks.clear();
        update.lease_term = 3;
        update.fencing_token = 9;
        node.heartbeat(Request::new(HeartbeatRequest {
            node_id: "stable-worker".to_owned(),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "alpha".to_owned(),
                NetworkResources {
                    num_neurons: 2,
                    layer_neuron_counts: HashMap::from([(0, 2)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: vec![update],
            stable_executor_capabilities: vec![stable_capability_wire()],
            command_results: Vec::new(),
        }))
        .await
        .expect("a fenced source drain must be accepted");

        let state = node.state.read().await;
        assert!(
            state.nodes["stable-worker"].stable_executors[0]
                .owned_shard_ids
                .is_empty()
        );
    }

    #[test]
    fn consistent_cut_evidence_binds_to_captured_payloads() {
        let (_, snapshot_json) = fresh_single_neuron_snapshot(1, NeuronModel::Lif, Learning::Stdp)
            .expect("fresh snapshot");
        let channel_state = serde_json::to_string(&ManagedChannelState {
            remote_spike_steps_fwd: BTreeMap::from([(2, 6)]),
            ..ManagedChannelState::default()
        })
        .expect("channel state");

        let (participant, marker) =
            local_cut_evidence("alpha", "node-a", 3, &snapshot_json, &channel_state)
                .expect("captured evidence");

        assert_eq!(participant.local_frontier, LogicalTag::new(0, 0));
        assert_eq!(participant.queued_min, Some(LogicalTag::new(6, 0)));
        assert_eq!(participant.activity_epoch, 1);
        assert_eq!(marker.epoch, 3);
        assert_eq!(marker.first_in_transit, participant.queued_min);
    }

    #[tokio::test]
    async fn cluster_snapshot_rpc_returns_a_complete_common_frontier() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_string(), true);
        let (_, snapshot_json) = fresh_single_neuron_snapshot(1, NeuronModel::Lif, Learning::Stdp)
            .expect("fresh snapshot");
        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::LoadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: snapshot_json.clone().into_bytes(),
            layers: vec![0],
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: "lif".to_string(),
            learning_rule: "stdp".to_string(),
        })
        .await;
        node.state.write().await.network_registry.insert(
            "alpha".to_string(),
            NetworkStatus {
                network_id: "alpha".to_string(),
                distribution: HashMap::from([(
                    "orch".to_string(),
                    LayerRange {
                        layers: vec![0],
                        layer_neuron_counts: HashMap::new(),
                        backup_layers: Vec::new(),
                    },
                )]),
                config_json: snapshot_json,
                num_layers: 1,
                desired_aarnn_depth: 1,
                neuron_model: "lif".to_string(),
                learning_rule: "stdp".to_string(),
                ..Default::default()
            },
        );

        let response = node
            .get_cluster_network_snapshot(Request::new(ClusterNetworkSnapshotRequest {
                network_id: "alpha".to_string(),
            }))
            .await
            .expect("cluster snapshot")
            .into_inner();
        assert_eq!(
            response.schema_version,
            cluster_snapshot::CLUSTER_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(response.cut_tick, 0);
        assert_eq!(response.cut_microstep, 0);
        assert!(!response.cluster_digest.is_empty());
        assert_eq!(response.shards.len(), 1);
        assert_eq!(response.shards[0].node_id, "orch");
        assert_eq!(response.shards[0].step, 0);
        assert!(!response.shards[0].state_digest.is_empty());
        assert!(!response.shards[0].channel_state_json.is_empty());
        assert!(!response.shards[0].channel_state_digest.is_empty());
    }

    #[tokio::test]
    async fn cluster_snapshot_rpc_fails_closed_when_a_shard_is_unavailable() {
        use proto::distributed_neuromorphic_server::DistributedNeuromorphic;

        let node = DistributedNode::new("orch".to_string(), true);
        node.state.write().await.network_registry.insert(
            "alpha".to_string(),
            NetworkStatus {
                network_id: "alpha".to_string(),
                distribution: HashMap::from([
                    (
                        "orch".to_string(),
                        LayerRange {
                            layers: vec![0],
                            layer_neuron_counts: HashMap::new(),
                            backup_layers: Vec::new(),
                        },
                    ),
                    (
                        "missing-worker".to_string(),
                        LayerRange {
                            layers: vec![1],
                            layer_neuron_counts: HashMap::new(),
                            backup_layers: Vec::new(),
                        },
                    ),
                ]),
                ..Default::default()
            },
        );

        let error = node
            .get_cluster_network_snapshot(Request::new(ClusterNetworkSnapshotRequest {
                network_id: "alpha".to_string(),
            }))
            .await
            .expect_err("incomplete cluster cuts must not be published");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    #[cfg(feature = "replicated_durability")]
    #[test]
    fn live_managed_step_publishes_runner_state_only_after_durable_commit() {
        let root =
            std::env::temp_dir().join(format!("aarnn-live-managed-step-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let config = NetworkConfig::default();
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let model = NeuronModel::Lif;
        let learning = Learning::Stdp;
        let runner = Runner::new(lif.clone(), stdp.clone(), config.clone(), model, learning);
        let owner = crate::managed_durability::ManagedDurability::open(
            &root,
            "live-managed",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .expect("durable live owner");
        let mut network = ManagedNetwork {
            id: "live-managed".to_owned(),
            runner,
            shard_runtime: None,
            #[cfg(feature = "stable_executor_live")]
            stable_executor: None,
            durable_owner: Some(owner),
            #[cfg(feature = "superdense_executor")]
            superdense: SuperdenseController::new(),
            assigned_layers: Vec::new(),
            redundant_layers: Vec::new(),
            remote_spikes_fwd: HashMap::new(),
            remote_spikes_bwd: HashMap::new(),
            remote_spike_steps_fwd: HashMap::new(),
            remote_spike_steps_bwd: HashMap::new(),
            external_sensory_spikes: None,
            avg_step_time_ms: 0.0,
            desired_aarnn_depth: 0,
            playing: true,
            initial_config: config,
            initial_model: model,
            initial_learning: learning,
            initial_lif: lif,
            initial_stdp: stdp,
            last_config_fingerprint: None,
            workspace_binding: None,
        };
        let before = network
            .runner
            .export_network_json()
            .expect("before snapshot");
        let previous_channel_state = capture_channel_state(&network);
        let out = network
            .step_and_commit_durable(None, previous_channel_state)
            .expect("durable managed step");
        assert!(out.t > 0);
        let committed = network
            .durable_owner
            .as_ref()
            .expect("owner")
            .authoritative_snapshot()
            .expect("authoritative read")
            .expect("committed snapshot");
        assert_ne!(committed, before);
        assert_eq!(committed, network.runner.export_network_json().unwrap());
        assert_eq!(
            network.durable_owner.as_ref().unwrap().durable_sequence(),
            Some(0)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature = "replicated_durability")]
    #[test]
    fn durable_snapshot_projection_does_not_read_ahead_of_the_authoritative_owner() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-authoritative-snapshot-projection-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let config = NetworkConfig::default();
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut runner = Runner::new(
            lif.clone(),
            stdp.clone(),
            config.clone(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let owner = crate::managed_durability::ManagedDurability::open(
            &root,
            "projection",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .expect("durable owner");
        runner.step(None);
        let net = ManagedNetwork {
            id: "projection".to_owned(),
            runner,
            shard_runtime: None,
            #[cfg(feature = "stable_executor_live")]
            stable_executor: None,
            durable_owner: Some(owner),
            #[cfg(feature = "superdense_executor")]
            superdense: SuperdenseController::new(),
            assigned_layers: Vec::new(),
            redundant_layers: Vec::new(),
            remote_spikes_fwd: HashMap::new(),
            remote_spikes_bwd: HashMap::new(),
            remote_spike_steps_fwd: HashMap::new(),
            remote_spike_steps_bwd: HashMap::new(),
            external_sensory_spikes: None,
            avg_step_time_ms: 0.0,
            desired_aarnn_depth: 0,
            playing: false,
            initial_config: config,
            initial_model: NeuronModel::Lif,
            initial_learning: Learning::Stdp,
            initial_lif: lif,
            initial_stdp: stdp,
            last_config_fingerprint: None,
            workspace_binding: None,
        };
        let (snapshot, _, step, _) = local_shard_snapshot(&net).expect("authoritative snapshot");
        assert_eq!(step, 0);
        let owner_snapshot = net
            .durable_owner
            .as_ref()
            .unwrap()
            .authoritative_snapshot()
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, owner_snapshot);
        assert_ne!(snapshot, net.runner.export_network_json().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn network_total_only_counts_its_own_unique_layers() {
        let distribution = HashMap::from([
            (
                "node-a".to_string(),
                LayerRange {
                    layers: vec![0, 1],
                    layer_neuron_counts: HashMap::from([(0, 32), (1, 64)]),
                    backup_layers: Vec::new(),
                },
            ),
            (
                "node-b".to_string(),
                LayerRange {
                    layers: vec![1, 2],
                    layer_neuron_counts: HashMap::from([(1, 64), (2, 16)]),
                    backup_layers: Vec::new(),
                },
            ),
        ]);

        assert_eq!(total_neurons_from_distribution(&distribution), 112);
        assert_eq!(total_neurons_from_distribution(&HashMap::new()), 0);
    }

    #[test]
    fn placement_telemetry_distinguishes_active_and_backup_moves() {
        let previous = HashMap::from([
            (
                "node-a".to_string(),
                LayerRange {
                    layers: vec![0],
                    layer_neuron_counts: HashMap::new(),
                    backup_layers: vec![1],
                },
            ),
            (
                "node-b".to_string(),
                LayerRange {
                    layers: vec![1],
                    layer_neuron_counts: HashMap::new(),
                    backup_layers: vec![0],
                },
            ),
        ]);
        let next = HashMap::from([
            (
                "node-a".to_string(),
                LayerRange {
                    layers: vec![1],
                    layer_neuron_counts: HashMap::new(),
                    backup_layers: vec![0],
                },
            ),
            (
                "node-b".to_string(),
                LayerRange {
                    layers: vec![0],
                    layer_neuron_counts: HashMap::new(),
                    backup_layers: vec![1],
                },
            ),
        ]);
        let movements = build_shard_placement_movements(
            "brain",
            &previous,
            &next,
            false,
            "capacity rebalance",
            42,
        );
        assert_eq!(movements.len(), 4);
        assert!(movements.iter().all(|movement| movement.phase == "moving"));
        assert!(movements.iter().any(|movement| movement.role == "active"));
        assert!(movements.iter().any(|movement| movement.role == "backup"));
        assert!(
            movements
                .iter()
                .all(|movement| movement.progress_milli <= 1000)
        );
        assert!(
            movements
                .windows(2)
                .all(|pair| pair[0].shard_id <= pair[1].shard_id)
        );
    }

    #[test]
    fn placement_telemetry_only_reports_consideration_when_automation_is_enabled() {
        let current = HashMap::from([(
            "node-a".to_string(),
            LayerRange {
                layers: vec![0, 1],
                layer_neuron_counts: HashMap::new(),
                backup_layers: vec![1],
            },
        )]);
        assert!(
            build_shard_placement_movements("brain", &current, &current, false, "", 42,).is_empty()
        );
        let considering =
            build_shard_placement_movements("brain", &current, &current, true, "", 42);
        assert_eq!(considering.len(), 3);
        assert!(
            considering
                .iter()
                .all(|movement| movement.phase == "considering")
        );
        assert!(
            considering
                .iter()
                .all(|movement| movement.updated_at_ms == 42)
        );
    }

    #[test]
    fn orchestrator_endpoints_are_normalized_and_deduplicated_in_order() {
        let endpoints = merge_orchestrator_endpoints([
            "qc01:50051, http://spirit:32051",
            "http://qc01:50051;https://edge.example:443/",
        ]);

        assert_eq!(
            endpoints,
            vec![
                "http://qc01:50051",
                "http://spirit:32051",
                "https://edge.example:443",
            ]
        );
    }

    #[test]
    fn discovery_targets_keep_broadcast_and_add_unicast_hosts() {
        let targets =
            discovery_target_tokens(Some("192.168.1.60,udp://192.168.1.62:51000 192.168.1.60"));

        assert_eq!(
            targets,
            vec![
                "255.255.255.255:50050",
                "127.0.0.1:50050",
                "192.168.1.60:50050",
                "192.168.1.62:51000",
            ]
        );
    }

    #[test]
    fn combined_networks_prefer_related_primary_node() {
        let mut deployments = HashMap::new();
        deployments.insert(
            "vision".to_string(),
            DeploymentConfig {
                combined_group: Some("ensemble-a".to_string()),
                ..DeploymentConfig::default()
            },
        );
        deployments.insert(
            "motor".to_string(),
            DeploymentConfig {
                combined_group: Some("ensemble-a".to_string()),
                ..DeploymentConfig::default()
            },
        );
        let primary_nodes = HashMap::from([("vision".to_string(), "node-b".to_string())]);
        let chosen = choose_single_node_target(
            "motor",
            &[("node-a".to_string(), 2.0), ("node-b".to_string(), 1.0)],
            deployments.get("motor").unwrap(),
            &deployments,
            &primary_nodes,
        );
        assert_eq!(chosen.as_deref(), Some("node-b"));
    }

    #[test]
    fn federated_networks_avoid_related_primary_node_when_possible() {
        let mut deployments = HashMap::new();
        deployments.insert(
            "client-a".to_string(),
            DeploymentConfig {
                federation_group: Some("fed-a".to_string()),
                ..DeploymentConfig::default()
            },
        );
        deployments.insert(
            "client-b".to_string(),
            DeploymentConfig {
                federation_group: Some("fed-a".to_string()),
                ..DeploymentConfig::default()
            },
        );
        let primary_nodes = HashMap::from([("client-a".to_string(), "node-a".to_string())]);
        let chosen = choose_single_node_target(
            "client-b",
            &[("node-a".to_string(), 4.0), ("node-b".to_string(), 2.0)],
            deployments.get("client-b").unwrap(),
            &deployments,
            &primary_nodes,
        );
        assert_eq!(chosen.as_deref(), Some("node-b"));
    }

    #[test]
    fn node_scope_disables_cross_node_sharding() {
        let deployment = DeploymentConfig {
            modes: vec![
                crate::deployment::ExecutionMode::Distributed,
                crate::deployment::ExecutionMode::Sharded,
            ],
            scope: crate::deployment::ExecutionScope::Node,
            ..DeploymentConfig::default()
        };

        assert!(!should_shard_across_nodes(&deployment));
    }

    #[test]
    fn desired_shards_limits_target_nodes() {
        let deployments = HashMap::from([(
            "vision".to_string(),
            DeploymentConfig {
                modes: vec![
                    crate::deployment::ExecutionMode::Distributed,
                    crate::deployment::ExecutionMode::Sharded,
                ],
                desired_shards: 2,
                ..DeploymentConfig::default()
            },
        )]);

        let selected = limit_target_nodes_for_deployment(
            "vision",
            &[
                ("node-a".to_string(), 1.0),
                ("node-b".to_string(), 2.0),
                ("node-c".to_string(), 3.0),
            ],
            deployments.get("vision").unwrap(),
            &deployments,
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
        );

        assert_eq!(
            selected,
            vec![("node-c".to_string(), 3.0), ("node-b".to_string(), 2.0),]
        );
    }

    #[tokio::test]
    async fn sharded_rebalance_expands_beyond_existing_affinity() {
        let node = DistributedNode::new("orch".to_string(), true);
        {
            let mut state = node.state.write().await;
            for node_id in ["node-a", "node-b", "node-c"] {
                state.nodes.insert(
                    node_id.to_string(),
                    NodeStatus {
                        node_id: node_id.to_string(),
                        address: format!("{node_id}:50051"),
                        resources: Some(Resources::default()),
                        active_networks: (node_id == "node-a")
                            .then(|| vec!["shared".to_string()])
                            .unwrap_or_default(),
                        stable_executors: Vec::new(),
                        stable_executor_capabilities: Vec::new(),
                    },
                );
            }
            state.network_registry.insert(
                "shared".to_string(),
                proto::NetworkStatus {
                    network_id: "shared".to_string(),
                    num_layers: 3,
                    distribution: HashMap::from([(
                        "node-a".to_string(),
                        LayerRange {
                            layers: vec![0, 1, 2],
                            layer_neuron_counts: HashMap::new(),
                            backup_layers: Vec::new(),
                        },
                    )]),
                    ..Default::default()
                },
            );
        }

        node.rebalance_networks().await;

        let state = node.state.read().await;
        let distribution = &state
            .network_registry
            .get("shared")
            .expect("network retained")
            .distribution;
        assert_eq!(distribution.len(), 3);
        assert!(distribution.contains_key("node-a"));
        assert!(distribution.contains_key("node-b"));
        assert!(distribution.contains_key("node-c"));
    }

    #[test]
    fn tiny_networks_keep_partial_views_when_targets_exceed_layers() {
        let assignments = build_sharded_node_assignments(
            &[
                ("node-b".to_string(), 2.0),
                ("node-c".to_string(), 1.0),
                ("node-a".to_string(), 3.0),
            ],
            2,
        );

        assert_eq!(
            assignments,
            vec![
                ("node-a".to_string(), vec![0, 1], vec![0, 1],),
                ("node-b".to_string(), vec![0], vec![0]),
                ("node-c".to_string(), vec![1], vec![1]),
            ]
        );
    }

    #[test]
    fn saturated_nodes_are_skipped_when_concurrency_limit_is_hit() {
        let deployment = DeploymentConfig {
            max_concurrent_networks: 1,
            ..DeploymentConfig::default()
        };

        let selected = limit_target_nodes_for_deployment(
            "alpha",
            &[("node-a".to_string(), 4.0), ("node-b".to_string(), 2.0)],
            &deployment,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::from([
                ("node-a".to_string(), 1usize),
                ("node-b".to_string(), 0usize),
            ]),
            &HashSet::new(),
        );

        assert_eq!(selected, vec![("node-b".to_string(), 2.0)]);
    }

    #[test]
    fn autonomous_transition_scales_out_hot_networks() {
        let deployment = DeploymentConfig {
            modes: vec![crate::deployment::ExecutionMode::Individual],
            transition_policy: crate::deployment::DeploymentTransitionPolicy {
                allow_live_transition: true,
                autonomous: true,
                permitted_modes: vec![
                    crate::deployment::ExecutionMode::Individual,
                    crate::deployment::ExecutionMode::Sharded,
                ],
                target_step_time_ms: Some(5.0),
                ..crate::deployment::DeploymentTransitionPolicy::default()
            },
            ..DeploymentConfig::default()
        };
        let telemetry = DeploymentTelemetry {
            avg_step_time_ms: 8.5,
            max_step_time_ms: 9.0,
            active_nodes: 1,
            ..DeploymentTelemetry::default()
        };

        let (next, _reason) = maybe_autonomous_transition(&deployment, &telemetry, 4)
            .expect("hot network should scale out");

        assert!(next.has_mode(crate::deployment::ExecutionMode::Sharded));
        assert!(!next.has_mode(crate::deployment::ExecutionMode::Individual));
        assert_eq!(next.desired_shards, 2);
    }

    #[test]
    fn autonomous_transition_collapses_idle_networks() {
        let deployment = DeploymentConfig {
            modes: vec![
                crate::deployment::ExecutionMode::Distributed,
                crate::deployment::ExecutionMode::Sharded,
            ],
            desired_shards: 2,
            transition_policy: crate::deployment::DeploymentTransitionPolicy {
                allow_live_transition: true,
                autonomous: true,
                permitted_modes: vec![
                    crate::deployment::ExecutionMode::Individual,
                    crate::deployment::ExecutionMode::Sharded,
                ],
                target_step_time_ms: Some(10.0),
                ..crate::deployment::DeploymentTransitionPolicy::default()
            },
            ..DeploymentConfig::default()
        };
        let telemetry = DeploymentTelemetry {
            avg_step_time_ms: 2.0,
            max_step_time_ms: 2.5,
            active_nodes: 2,
            ..DeploymentTelemetry::default()
        };

        let (next, _reason) = maybe_autonomous_transition(&deployment, &telemetry, 4)
            .expect("idle network should scale in");

        assert!(!next.has_mode(crate::deployment::ExecutionMode::Sharded));
        assert!(next.has_mode(crate::deployment::ExecutionMode::Individual));
        assert_eq!(next.desired_shards, 1);
    }

    #[test]
    fn snapshot_with_network_config_replaces_config_without_losing_state() {
        let (current_cfg, snapshot_json) =
            fresh_single_neuron_snapshot(1, NeuronModel::Aarnn, Learning::Aarnn)
                .expect("fresh snapshot");
        let original = crate::runner::decode_snapshot_with_profile_backfill(&snapshot_json)
            .expect("decode original snapshot");

        let mut requested_cfg = current_cfg.clone();
        requested_cfg
            .deployment
            .add_mode(crate::deployment::ExecutionMode::Sharded);
        requested_cfg.deployment.desired_shards = 2;
        requested_cfg.deployment.normalize();

        let refreshed =
            snapshot_with_network_config(&snapshot_json, &requested_cfg).expect("refresh snapshot");
        let updated = crate::runner::decode_snapshot_with_profile_backfill(&refreshed)
            .expect("decode updated snapshot");

        assert_eq!(updated.net.deployment, requested_cfg.deployment);
        assert_eq!(updated.w_in.data, original.w_in.data);
        assert_eq!(updated.w_out.data, original.w_out.data);
        assert_eq!(updated.t, original.t);
        assert_eq!(updated.t_ms, original.t_ms);
    }

    #[tokio::test]
    async fn manual_transition_payload_refresh_uses_live_snapshot_for_deployment_only_updates() {
        let node = DistributedNode::new("orch".to_string(), true);
        let (_, snapshot_json) =
            fresh_single_neuron_snapshot(1, NeuronModel::Aarnn, Learning::Aarnn)
                .expect("fresh snapshot");
        let original = crate::runner::decode_snapshot_with_profile_backfill(&snapshot_json)
            .expect("decode original snapshot");
        let current_from_payload =
            network_config_from_payload(&snapshot_json).expect("current config from snapshot");

        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::LoadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: snapshot_json.as_bytes().to_vec(),
            layers: vec![0],
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: "aarnn".to_string(),
            learning_rule: "aarnn".to_string(),
        })
        .await;

        {
            let mut state = node.state.write().await;
            state.network_registry.insert(
                "alpha".to_string(),
                NetworkStatus {
                    network_id: "alpha".to_string(),
                    distribution: HashMap::from([(
                        "orch".to_string(),
                        LayerRange {
                            layers: vec![0],
                            layer_neuron_counts: HashMap::new(),
                            backup_layers: Vec::new(),
                        },
                    )]),
                    num_layers: (current_from_payload.num_hidden_layers + 1) as u32,
                    desired_aarnn_depth: 1,
                    config_json: snapshot_json.clone(),
                    neuron_model: "aarnn".to_string(),
                    learning_rule: "aarnn".to_string(),
                    playing: true,
                    ..Default::default()
                },
            );
            state
                .network_snapshots
                .insert("alpha".to_string(), snapshot_json.clone());
        }

        let mut requested_cfg = current_from_payload.clone();
        requested_cfg
            .deployment
            .add_mode(crate::deployment::ExecutionMode::Combined);
        requested_cfg.deployment.combined_group = Some("ensemble-a".to_string());
        requested_cfg.deployment.normalize();
        let requested_payload =
            serde_json::to_string(&requested_cfg).expect("serialize requested config");

        assert!(network_config_shape_compatible(
            &current_from_payload,
            &requested_cfg
        ));
        let live_source = {
            let state = node.state.read().await;
            let net_status = state.network_registry.get("alpha").expect("network status");
            live_snapshot_source_for(&state, "alpha", &net_status.distribution)
                .expect("live snapshot source")
        };
        let live_snapshot = node
            .fetch_live_network_snapshot(&live_source)
            .await
            .expect("fetched live snapshot");
        assert!(snapshot_with_network_config(&live_snapshot, &requested_cfg).is_some());

        let refreshed = node
            .maybe_refresh_manual_transition_payload("alpha", &requested_payload)
            .await
            .expect("live deployment refresh");
        let updated = crate::runner::decode_snapshot_with_profile_backfill(&refreshed)
            .expect("decode refreshed snapshot");

        assert_eq!(updated.net.deployment, requested_cfg.deployment);
        assert_eq!(updated.w_in.data, original.w_in.data);
        assert_eq!(updated.w_out.data, original.w_out.data);
        assert_eq!(updated.t, original.t);
    }

    #[tokio::test]
    async fn manual_transition_payload_refresh_skips_structural_config_changes() {
        let node = DistributedNode::new("orch".to_string(), true);
        let (_, snapshot_json) =
            fresh_single_neuron_snapshot(1, NeuronModel::Aarnn, Learning::Aarnn)
                .expect("fresh snapshot");
        let current_from_payload =
            network_config_from_payload(&snapshot_json).expect("current config from snapshot");

        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::LoadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: snapshot_json.as_bytes().to_vec(),
            layers: vec![0],
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: "aarnn".to_string(),
            learning_rule: "aarnn".to_string(),
        })
        .await;

        {
            let mut state = node.state.write().await;
            state.network_registry.insert(
                "alpha".to_string(),
                NetworkStatus {
                    network_id: "alpha".to_string(),
                    distribution: HashMap::from([(
                        "orch".to_string(),
                        LayerRange {
                            layers: vec![0],
                            layer_neuron_counts: HashMap::new(),
                            backup_layers: Vec::new(),
                        },
                    )]),
                    num_layers: (current_from_payload.num_hidden_layers + 1) as u32,
                    desired_aarnn_depth: 1,
                    config_json: snapshot_json.clone(),
                    neuron_model: "aarnn".to_string(),
                    learning_rule: "aarnn".to_string(),
                    playing: true,
                    ..Default::default()
                },
            );
            state
                .network_snapshots
                .insert("alpha".to_string(), snapshot_json.clone());
        }

        let mut requested_cfg = current_from_payload.clone();
        requested_cfg.num_hidden_layers = requested_cfg.num_hidden_layers.saturating_add(1);
        requested_cfg
            .deployment
            .add_mode(crate::deployment::ExecutionMode::Combined);
        requested_cfg.deployment.combined_group = Some("ensemble-a".to_string());
        requested_cfg.deployment.normalize();
        let requested_payload =
            serde_json::to_string(&requested_cfg).expect("serialize requested config");

        assert!(
            node.maybe_refresh_manual_transition_payload("alpha", &requested_payload)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn manual_live_transition_requires_explicit_permission() {
        let node = DistributedNode::new("orch".to_string(), true);
        let (_, snapshot_json) =
            fresh_single_neuron_snapshot(1, NeuronModel::Aarnn, Learning::Aarnn)
                .expect("fresh snapshot");
        let current_cfg =
            network_config_from_payload(&snapshot_json).expect("current config from snapshot");

        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::LoadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: snapshot_json.as_bytes().to_vec(),
            layers: vec![0],
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: "aarnn".to_string(),
            learning_rule: "aarnn".to_string(),
        })
        .await;

        {
            let mut state = node.state.write().await;
            let mut status = NetworkStatus {
                network_id: "alpha".to_string(),
                distribution: HashMap::from([(
                    "orch".to_string(),
                    LayerRange {
                        layers: vec![0],
                        layer_neuron_counts: HashMap::new(),
                        backup_layers: Vec::new(),
                    },
                )]),
                num_layers: (current_cfg.num_hidden_layers + 1) as u32,
                desired_aarnn_depth: 1,
                config_json: snapshot_json.clone(),
                neuron_model: "aarnn".to_string(),
                learning_rule: "aarnn".to_string(),
                playing: true,
                ..Default::default()
            };
            sync_network_status_deployment_from_payload(&mut status, &snapshot_json);
            state.network_registry.insert("alpha".to_string(), status);
            state
                .network_snapshots
                .insert("alpha".to_string(), snapshot_json.clone());
        }

        let mut requested_cfg = current_cfg.clone();
        requested_cfg
            .deployment
            .add_mode(crate::deployment::ExecutionMode::Combined);
        requested_cfg.deployment.combined_group = Some("ensemble-a".to_string());
        requested_cfg.deployment.normalize();
        let requested_payload =
            serde_json::to_string(&requested_cfg).expect("serialize requested config");

        let err = node
            .update_network(Request::new(NetworkUpdateRequest {
                network_id: "alpha".to_string(),
                update: Some(proto::network_update_request::Update::Config(
                    ConfigUpdate {
                        config_json: requested_payload.into_bytes(),
                        neuron_model: "aarnn".to_string(),
                        learning_rule: "aarnn".to_string(),
                    },
                )),
            }))
            .await
            .expect_err("live transition should be rejected without permission");

        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
        let state = node.state.read().await;
        let status = state.network_registry.get("alpha").expect("status");
        assert!(status.last_transition_reason.is_empty());
        assert!(state.last_deployment_transition.get("alpha").is_none());
    }

    #[tokio::test]
    async fn manual_live_transition_updates_status_when_permission_is_granted() {
        let node = DistributedNode::new("orch".to_string(), true);
        let (_, snapshot_json) =
            fresh_single_neuron_snapshot(1, NeuronModel::Aarnn, Learning::Aarnn)
                .expect("fresh snapshot");
        let current_cfg =
            network_config_from_payload(&snapshot_json).expect("current config from snapshot");

        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::LoadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: snapshot_json.as_bytes().to_vec(),
            layers: vec![0],
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: "aarnn".to_string(),
            learning_rule: "aarnn".to_string(),
        })
        .await;

        {
            let mut state = node.state.write().await;
            let mut status = NetworkStatus {
                network_id: "alpha".to_string(),
                distribution: HashMap::from([(
                    "orch".to_string(),
                    LayerRange {
                        layers: vec![0],
                        layer_neuron_counts: HashMap::new(),
                        backup_layers: Vec::new(),
                    },
                )]),
                num_layers: (current_cfg.num_hidden_layers + 1) as u32,
                desired_aarnn_depth: 1,
                config_json: snapshot_json.clone(),
                neuron_model: "aarnn".to_string(),
                learning_rule: "aarnn".to_string(),
                playing: true,
                ..Default::default()
            };
            sync_network_status_deployment_from_payload(&mut status, &snapshot_json);
            state.network_registry.insert("alpha".to_string(), status);
            state
                .network_snapshots
                .insert("alpha".to_string(), snapshot_json.clone());
        }

        let mut requested_cfg = current_cfg.clone();
        requested_cfg
            .deployment
            .add_mode(crate::deployment::ExecutionMode::Combined);
        requested_cfg.deployment.combined_group = Some("ensemble-a".to_string());
        requested_cfg
            .deployment
            .transition_policy
            .allow_live_transition = true;
        requested_cfg.deployment.normalize();
        let requested_payload =
            serde_json::to_string(&requested_cfg).expect("serialize requested config");

        let response = node
            .update_network(Request::new(NetworkUpdateRequest {
                network_id: "alpha".to_string(),
                update: Some(proto::network_update_request::Update::Config(
                    ConfigUpdate {
                        config_json: requested_payload.into_bytes(),
                        neuron_model: "aarnn".to_string(),
                        learning_rule: "aarnn".to_string(),
                    },
                )),
            }))
            .await
            .expect("live transition should succeed with permission")
            .into_inner();

        assert!(response.success);
        let state = node.state.read().await;
        let status = state.network_registry.get("alpha").expect("status");
        assert!(status.live_transition_allowed);
        assert_eq!(status.last_transition_source, "manual");
        assert!(status.last_transition_ts_ms > 0);
        assert!(
            status
                .last_transition_reason
                .contains("manual deployment transition")
        );
        assert!(
            status
                .deployment_modes
                .iter()
                .any(|mode| mode == "combined")
        );
        assert!(state.last_deployment_transition.contains_key("alpha"));
    }

    #[test]
    fn external_telemetry_pressure_reduces_effective_capacity() {
        let baseline = Resources {
            capacity_score: 20.0,
            avg_step_time_ms: 5.0,
            ..Default::default()
        };
        let pressured = Resources {
            capacity_score: 20.0,
            avg_step_time_ms: 5.0,
            num_gpus: 2,
            telemetry_source: "http://127.0.0.1:48000/status".to_string(),
            telemetry_cpu_usage_pct: 91.0,
            telemetry_mem_used_pct: 88.0,
            telemetry_net_rx_bps: 125_000_000.0,
            telemetry_net_tx_bps: 110_000_000.0,
            telemetry_disk_used_pct: 93.0,
            telemetry_disk_read_bps: 180_000_000.0,
            telemetry_disk_write_bps: 175_000_000.0,
            telemetry_gpu_util_pct: 97.0,
            telemetry_gpu_temp_c: 84.0,
            telemetry_gpu_power_w: 540.0,
            telemetry_gpu_mem_used_pct: 96.0,
            telemetry_recent_action_count: 18,
            ..Default::default()
        };

        assert!(resource_external_pressure(&pressured) > 0.8);
        assert!(
            effective_capacity_score(&pressured, 10.0) < effective_capacity_score(&baseline, 10.0)
        );
    }

    #[tokio::test]
    async fn unload_network_command_removes_local_network() {
        let node = DistributedNode::new("test-node".to_string(), false);
        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::LoadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: Vec::new(),
            layers: vec![0],
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: "aarnn".to_string(),
            learning_rule: "aarnn".to_string(),
        })
        .await;
        assert!(node.state.read().await.networks.contains_key("alpha"));

        node.handle_command(NetworkCommand {
            r#type: proto::network_command::CommandType::UnloadNetwork as i32,
            network_id: "alpha".to_string(),
            config_json: Vec::new(),
            layers: Vec::new(),
            redundant_layers: Vec::new(),
            desired_aarnn_depth: 1,
            neuron_model: String::new(),
            learning_rule: String::new(),
        })
        .await;

        assert!(!node.state.read().await.networks.contains_key("alpha"));
    }

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

    #[test]
    fn causal_certificate_binding_is_exact_and_uses_der_sha256() {
        let certificate = b"synthetic-mtls-leaf";
        let fingerprint = certificate_sha256_der(certificate);
        let enrolled =
            std::collections::BTreeMap::from([("node-a".to_owned(), fingerprint.clone())]);
        assert_eq!(fingerprint.len(), 64);
        assert!(
            enrolled
                .get("node-a")
                .is_some_and(|expected| expected == &fingerprint)
        );
        assert!(
            !enrolled
                .get("node-a")
                .is_some_and(|expected| expected == &certificate_sha256_der(b"other-leaf"))
        );
        assert!(!enrolled.contains_key("node-b"));
    }

    #[tokio::test]
    async fn phase0_seven_node_capture_matches_current_compatibility_behaviour() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../docs/architecture/baseline/phase0-seven-node-layer.json"
        ))
        .expect("valid seven-node Phase 0 fixture");
        let assignments = build_sharded_node_assignments(
            &[
                ("node-01".to_string(), 7.0),
                ("node-02".to_string(), 6.0),
                ("node-03".to_string(), 5.0),
                ("node-04".to_string(), 4.0),
                ("node-05".to_string(), 3.0),
                ("node-06".to_string(), 2.0),
                ("node-07".to_string(), 1.0),
            ],
            3,
        );
        let captured_assignments: Vec<serde_json::Value> = assignments
            .iter()
            .map(|(node_id, layers, redundant_layers)| {
                serde_json::json!({
                    "node_id": node_id,
                    "layers": layers,
                    "redundant_layers": redundant_layers,
                })
            })
            .collect();
        assert_eq!(
            serde_json::json!({
                "anchor_node": assignments[0].0,
                "assignments": captured_assignments,
            }),
            fixture["assignment_observation"]
        );

        let node = DistributedNode::new("fixture-node".to_string(), false);
        let initial_transport = node
            .state
            .read()
            .await
            .choose_spike_transport("fixture-peer", true, false)
            .as_str();
        let mut state = node.state.write().await;
        for _ in 0..SPIKE_FAILOVER_STREAK {
            state.record_spike_transport_failure(
                "fixture-peer",
                SpikeTransportMethod::PersistentStream,
            );
        }
        let fallback_transport = state
            .choose_spike_transport("fixture-peer", true, false)
            .as_str();
        assert_eq!(
            serde_json::json!({
                "burst_timeout_ms": spike_burst_timeout().as_millis(),
                "initial_preference_with_persistent_stream": initial_transport,
                "preference_after_three_persistent_failures": fallback_transport,
            }),
            fixture["transport_observation"]
        );
    }

    #[cfg(feature = "replicated_durability")]
    #[tokio::test]
    async fn live_causal_ingress_is_durable_before_channel_projection() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-live-causal-ingress-{}-{}",
            std::process::id(),
            unix_timestamp_ms_now()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create causal test root");
        let mut config = NetworkConfig::default();
        config.num_sensory_neurons = 2;
        config.num_hidden_layers = 2;
        config.num_hidden_per_layer_initial = 2;
        config.num_output_neurons = 1;
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let mut runner = Runner::new(lif, stdp, config.clone(), NeuronModel::Lif, Learning::Stdp);
        runner.layer_range = Some(0..1);
        let pre_ingress_biological_snapshot =
            serde_json::to_vec(&runner.snapshot()).expect("encode pre-ingress biological snapshot");
        let owner = crate::managed_durability::ManagedDurability::open(
            &root,
            "causal-live",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            None,
        )
        .expect("open durable owner");
        let brain = crate::managed_durability::managed_brain_id("causal-live");
        let stream = crate::managed_durability::managed_stream_id("causal-live");
        let route = crate::managed_durability::managed_route_id("causal-live");
        let exchange = encode_exchange(0, 0, &[1, 0]);
        let ingress = CausalSpikeIngress {
            schema_version: CAUSAL_INGRESS_SCHEMA_VERSION,
            network_id: "causal-live".to_owned(),
            layer_index: 1,
            step_index: 0,
            is_backward: false,
            spike_indices: exchange.spike_indices,
            aer_payload: exchange.aer_payload,
            aer_base: exchange.aer_base,
        };
        let frame = crate::causal_transport::proto::CausalEventEnvelope {
            schema_version: u32::from(crate::deterministic::SchemaVersion::CURRENT.raw()),
            brain_id: brain.raw(),
            stream_id: stream.raw(),
            sequence: 0,
            lease_term: LeaseTerm::INITIAL.raw(),
            route_id: route.raw(),
            partition_generation: crate::deterministic::PartitionGeneration::INITIAL.raw(),
            tag: Some(crate::causal_transport::proto::LogicalTag {
                tick: 0,
                microstep: 0,
            }),
            event_id: 1,
            stage: 1,
            source_id: 0,
            target_id: 0,
            payload: serde_json::to_vec(&ingress).expect("encode ingress"),
            deferred_from_nonconvergence: false,
            sender_node_id: String::new(),
        };
        let network = ManagedNetwork {
            id: "causal-live".to_owned(),
            runner,
            shard_runtime: None,
            #[cfg(feature = "stable_executor_live")]
            stable_executor: None,
            durable_owner: Some(owner),
            superdense: SuperdenseController::new(),
            assigned_layers: vec![0],
            redundant_layers: Vec::new(),
            remote_spikes_fwd: HashMap::new(),
            remote_spikes_bwd: HashMap::new(),
            remote_spike_steps_fwd: HashMap::new(),
            remote_spike_steps_bwd: HashMap::new(),
            external_sensory_spikes: None,
            avg_step_time_ms: 0.0,
            desired_aarnn_depth: 1,
            playing: true,
            initial_config: config,
            initial_model: NeuronModel::Lif,
            initial_learning: Learning::Stdp,
            initial_lif: LIFParams::default(),
            initial_stdp: STDPParams::default(),
            last_config_fingerprint: None,
            workspace_binding: None,
        };
        let node = DistributedNode::new("node-b".to_owned(), false);
        node.state
            .write()
            .await
            .networks
            .insert("causal-live".to_owned(), Arc::new(RwLock::new(network)));
        let causal = crate::data_plane::CausalEnvelope::try_from(frame.clone())
            .expect("wire frame converts");
        let ingress: CausalSpikeIngress =
            serde_json::from_slice(&causal.payload).expect("decode ingress");
        node.admit_causal_spike_ingress(&frame, causal, ingress)
            .await
            .expect("causal ingress commits");
        let network = node
            .state
            .read()
            .await
            .networks
            .get("causal-live")
            .cloned()
            .expect("network remains loaded");
        let network = network.read().await;
        let post_ingress_biological_snapshot = serde_json::to_vec(&network.runner.snapshot())
            .expect("encode post-ingress biological snapshot");
        assert_eq!(
            post_ingress_biological_snapshot, pre_ingress_biological_snapshot,
            "causal ingress must not mutate biological bytes before a shard step commits"
        );
        assert_eq!(network.remote_spikes_fwd.get(&1), Some(&vec![1, 0]));
        assert_eq!(
            network
                .durable_owner
                .as_ref()
                .expect("owner")
                .durable_sequence(),
            Some(0)
        );
        let owner = network.durable_owner.as_ref().expect("owner");
        let first_checkpoint = owner.checkpoint_payload().expect("checkpoint");
        assert_eq!(first_checkpoint.receipts.len(), 1);
        let first_channel_state = owner
            .authoritative_channel_state()
            .expect("durable channel state after first ingress");
        drop(network);

        let retry_causal = crate::data_plane::CausalEnvelope::try_from(frame.clone())
            .expect("retry wire frame converts");
        let retry_ingress: CausalSpikeIngress =
            serde_json::from_slice(&retry_causal.payload).expect("decode retry ingress");
        node.admit_causal_spike_ingress(&frame, retry_causal, retry_ingress)
            .await
            .expect("duplicate causal ingress is idempotent");

        let network = node
            .state
            .read()
            .await
            .networks
            .get("causal-live")
            .cloned()
            .expect("network remains loaded");
        let network = network.read().await;
        let owner = network.durable_owner.as_ref().expect("owner");
        let retry_checkpoint = owner.checkpoint_payload().expect("retry checkpoint");
        assert_eq!(retry_checkpoint.receipts.len(), 1);
        assert_eq!(
            owner
                .authoritative_channel_state()
                .expect("durable channel state after retry"),
            first_channel_state,
            "retry must not rewrite the durable channel projection"
        );
        assert_eq!(
            serde_json::to_vec(&network.runner.snapshot())
                .expect("encode retry biological snapshot"),
            pre_ingress_biological_snapshot,
            "retry must not mutate biological bytes"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
