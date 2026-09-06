//! Versioned, durable transport for physically distributed stable shards.
//!
//! The stable executor produces a [`StableOutboundRecord`] only after the
//! sender has sealed it in its durable outbox.  This module converts that
//! record to a bounded protobuf frame and provides the corresponding receiver
//! boundary.  The receiver validates every identity, digest, generation
//! binding and fence, applies the typed message to its partial executor, then
//! atomically publishes the updated shard checkpoints and receipt frontier.
//!
//! The service is an additive reference adapter.  It does not provide quorum
//! election or transport authentication by itself; the enclosing node session
//! must authenticate `source_node_id` and configure the receiver's allow-list.
//! Production promotion remains behind the phase 4/6/7 evidence gates.

use crate::deterministic::{
    BrainId, EventId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest,
    StateDigestBuilder, TopologyGeneration,
};
use crate::durability::{DurabilityError, atomic_replace_and_sync};
use crate::node_auth::{
    certificate_sha256_der, live_causal_transport_enabled, validate_peer_metadata,
};
use crate::partial_shard_executor::{
    PartialShardExecutor, PartialShardExecutorError, PartialShardOutbound,
};
use crate::stable_outbound::{StableOutboundError, StableOutboundRecord};
use crate::stable_shard_dispatch::{StableShardDispatchError, StableShardDispatcher};
use crate::topology_model::{CompiledExecutionPlan, TopologyGenerationModel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

/// Generated bindings for the stable-shard service in `distributed.proto`.
pub use crate::causal_transport::proto;

pub const STABLE_SHARD_TRANSPORT_SCHEMA_VERSION: u32 = 1;
pub const STABLE_SHARD_SOURCE_NODE_METADATA: &str = "x-aarnn-source-node-id";
const STABLE_SHARD_RECEIVER_LEGACY_SCHEMA_VERSION: u32 = 2;
const STABLE_SHARD_RECEIVER_SCHEMA_VERSION: u32 = 3;
const MAX_NODE_ID_BYTES: usize = 256;
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECEIPTS: usize = 8192;
const MAX_PENDING_OUTBOUND: usize = 4096;
const MAX_RECEIVER_STEPS_PER_FRAME: usize = 1024;

#[derive(Debug, Error)]
pub enum StableShardTransportError {
    #[error("stable-shard frame schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("stable-shard frame field {0} is invalid")]
    InvalidField(&'static str),
    #[error("stable-shard frame payload is too large ({0} bytes)")]
    FrameTooLarge(usize),
    #[error("stable-shard frame payload could not be decoded: {0}")]
    Payload(String),
    #[error("stable-shard frame message kind does not match its payload")]
    MessageKindMismatch,
    #[error("stable-shard frame metadata does not match its typed payload")]
    MetadataMismatch,
    #[error("stable-shard source node is not enrolled for this receiver")]
    SourceNotAllowed,
    #[error("stable-shard source node does not match the authenticated session identity")]
    SessionSourceMismatch,
    #[error("stable-shard receiver is addressed to {expected}, received {received}")]
    DestinationMismatch { expected: String, received: String },
    #[error("stable-shard frame sequence gap: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("stable-shard frame sequence {sequence} conflicts with its durable receipt")]
    ConflictingReceipt { sequence: u64 },
    #[error("stable-shard frame lease/fence does not match the receiver authority")]
    StaleAuthority,
    #[error(transparent)]
    Outbound(#[from] StableOutboundError),
    #[error(transparent)]
    Executor(#[from] PartialShardExecutorError),
    #[error(transparent)]
    Primitive(#[from] crate::deterministic::PrimitiveError),
    #[error(transparent)]
    Durability(#[from] DurabilityError),
    #[error("stable-shard receiver persistence failed: {0}")]
    Persistence(String),
    #[error("stable-shard receiver registry lock was poisoned")]
    RegistryLock,
    #[error("stable-shard receiver for brain {0} is already registered")]
    ReceiverAlreadyRegistered(BrainId),
    #[error("stable-shard receiver for brain {0} is not registered")]
    ReceiverNotRegistered(BrainId),
    #[error("stable-shard receiver belongs to node {actual}, expected {expected}")]
    ReceiverNodeMismatch { expected: String, actual: String },
}

#[derive(Debug, Error)]
pub enum StableShardFlushError {
    #[error(transparent)]
    Outbound(#[from] StableOutboundError),
    #[error(transparent)]
    Frame(#[from] StableShardTransportError),
    #[error("stable-shard data-plane connection failed: {0}")]
    Connect(String),
    #[error(transparent)]
    Rpc(#[from] tonic::Status),
    #[error("stable-shard acknowledgement is invalid: {0}")]
    InvalidAcknowledgement(String),
    #[error("stable-shard source session identity could not be encoded: {0}")]
    InvalidSessionIdentity(String),
    #[error("stable-shard outbound log lock was poisoned")]
    LogLock,
}

/// A decoded frame retains the authenticated transport identity separately
/// from the biological message.  The source identity must be checked by the
/// node/session layer and is never allowed to affect logical time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedStableShardFrame {
    pub source_node_id: String,
    pub record: StableOutboundRecord,
}

fn validate_node_id(value: &str) -> Result<(), StableShardTransportError> {
    if value.trim().is_empty()
        || value.len() > MAX_NODE_ID_BYTES
        || value.contains(['/', '\\', '\0'])
    {
        return Err(StableShardTransportError::InvalidField("node_id"));
    }
    Ok(())
}

fn digest_from_wire(
    value: &[u8],
    field: &'static str,
) -> Result<StateDigest, StableShardTransportError> {
    let bytes: [u8; 16] = value
        .try_into()
        .map_err(|_| StableShardTransportError::InvalidField(field))?;
    Ok(StateDigest(bytes))
}

fn message_kind(message: &crate::partial_shard_executor::PartialShardOutbound) -> i32 {
    match message {
        crate::partial_shard_executor::PartialShardOutbound::CausalEvent { .. } => 1,
        crate::partial_shard_executor::PartialShardOutbound::SynapseEffect { .. } => 2,
        crate::partial_shard_executor::PartialShardOutbound::SynapseActivation { .. } => 3,
    }
}

fn message_metadata(
    message: &crate::partial_shard_executor::PartialShardOutbound,
) -> (StateDigest, ShardId, LogicalTag, EventId) {
    match message {
        crate::partial_shard_executor::PartialShardOutbound::CausalEvent {
            plan_digest,
            destination_shard,
            event,
        } => (
            *plan_digest,
            *destination_shard,
            event.event.key.tag,
            event.event.id,
        ),
        crate::partial_shard_executor::PartialShardOutbound::SynapseEffect {
            plan_digest,
            destination_shard,
            event_id,
            logical_tag,
            ..
        } => (*plan_digest, *destination_shard, *logical_tag, *event_id),
        crate::partial_shard_executor::PartialShardOutbound::SynapseActivation {
            plan_digest,
            destination_shard,
            parent_event,
            child_tag,
            ..
        } => (*plan_digest, *destination_shard, *child_tag, *parent_event),
    }
}

/// Encode a sealed outbound record into its bounded protobuf representation.
pub fn encode_frame(
    record: &StableOutboundRecord,
    source_node_id: &str,
) -> Result<proto::StableShardFrame, StableShardTransportError> {
    validate_node_id(source_node_id)?;
    validate_node_id(&record.destination_node)?;
    record.verify_integrity()?;
    let payload = serde_json::to_vec(&record.message)
        .map_err(|error| StableShardTransportError::Payload(error.to_string()))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(StableShardTransportError::FrameTooLarge(payload.len()));
    }
    Ok(proto::StableShardFrame {
        schema_version: STABLE_SHARD_TRANSPORT_SCHEMA_VERSION,
        brain_id: record.brain_id.raw(),
        source_node_id: source_node_id.to_owned(),
        destination_node_id: record.destination_node.clone(),
        destination_shard: record.destination_shard.raw(),
        plan_digest: record.plan_digest.0.to_vec(),
        lease_term: record.lease_term.raw(),
        fencing_token: record.fencing_token,
        sequence: record.sequence,
        logical_tag: Some(proto::LogicalTag {
            tick: record.logical_tag.tick,
            microstep: record.logical_tag.microstep,
        }),
        event_id: record.event_id.raw(),
        message_kind: message_kind(&record.message),
        payload,
        record_digest: record.record_digest.0.to_vec(),
        topology_generation: record.topology_generation.raw(),
        partition_generation: record.partition_generation.raw(),
        placement_plan_digest: record.placement_plan_digest.0.to_vec(),
    })
}

/// Decode and validate all frame metadata before it reaches executor state.
pub fn decode_frame(
    frame: proto::StableShardFrame,
) -> Result<DecodedStableShardFrame, StableShardTransportError> {
    if frame.schema_version != STABLE_SHARD_TRANSPORT_SCHEMA_VERSION {
        return Err(StableShardTransportError::UnsupportedSchema(
            frame.schema_version,
        ));
    }
    validate_node_id(&frame.source_node_id)?;
    validate_node_id(&frame.destination_node_id)?;
    if frame.payload.len() > MAX_FRAME_BYTES {
        return Err(StableShardTransportError::FrameTooLarge(
            frame.payload.len(),
        ));
    }
    let message = serde_json::from_slice(&frame.payload)
        .map_err(|error| StableShardTransportError::Payload(error.to_string()))?;
    if frame.message_kind != message_kind(&message) {
        return Err(StableShardTransportError::MessageKindMismatch);
    }
    let (plan_digest, destination_shard, logical_tag, event_id) = message_metadata(&message);
    let tag = frame
        .logical_tag
        .ok_or(StableShardTransportError::InvalidField("logical_tag"))?;
    if plan_digest != digest_from_wire(&frame.plan_digest, "plan_digest")?
        || destination_shard != ShardId::new(frame.destination_shard)?
        || logical_tag != LogicalTag::new(tag.tick, tag.microstep)
        || event_id != EventId::new(frame.event_id)?
    {
        return Err(StableShardTransportError::MetadataMismatch);
    }
    let record = StableOutboundRecord {
        sequence: frame.sequence,
        brain_id: BrainId::new(frame.brain_id)?,
        destination_node: frame.destination_node_id,
        destination_shard,
        plan_digest,
        lease_term: LeaseTerm::new(frame.lease_term)?,
        fencing_token: frame.fencing_token,
        logical_tag,
        event_id,
        message,
        topology_generation: TopologyGeneration::new(frame.topology_generation)?,
        partition_generation: PartitionGeneration::new(frame.partition_generation)?,
        placement_plan_digest: digest_from_wire(
            &frame.placement_plan_digest,
            "placement_plan_digest",
        )?,
        record_digest: digest_from_wire(&frame.record_digest, "record_digest")?,
    };
    record.verify_integrity()?;
    Ok(DecodedStableShardFrame {
        source_node_id: frame.source_node_id,
        record,
    })
}

/// Send every currently pending record for one destination and acknowledge the
/// durable prefix in the sender outbox.  The log is read before opening the
/// network stream and is updated only after each receiver acknowledgement, so
/// a connection loss leaves unsent or unacknowledged records available for a
/// later retry.  The bounded gRPC stream is asynchronous; filesystem work is
/// limited to short critical sections around the existing crash-safe log.
pub async fn flush_pending(
    log: Arc<tokio::sync::Mutex<crate::stable_outbound::StableOutboundLog>>,
    destination_node: &str,
    source_node_id: &str,
    address: &str,
) -> Result<usize, StableShardFlushError> {
    validate_node_id(destination_node)?;
    validate_node_id(source_node_id)?;
    let records = {
        let mut log_guard = log.lock().await;
        log_guard.pending(destination_node)?
    };
    if records.is_empty() {
        return Ok(0);
    }
    let frames = records
        .iter()
        .map(|record| encode_frame(record, source_node_id))
        .collect::<Result<Vec<_>, _>>()?;
    let endpoint =
        crate::management::grpc_client_endpoint(address).map_err(StableShardFlushError::Connect)?;
    let mut client =
        proto::stable_shard_data_plane_client::StableShardDataPlaneClient::connect(endpoint)
            .await
            .map_err(|error| StableShardFlushError::Connect(error.to_string()))?;
    let mut request = Request::new(tokio_stream::iter(frames));
    let source_metadata = tonic::metadata::MetadataValue::try_from(source_node_id)
        .map_err(|error| StableShardFlushError::InvalidSessionIdentity(error.to_string()))?;
    request
        .metadata_mut()
        .insert(STABLE_SHARD_SOURCE_NODE_METADATA, source_metadata);
    if crate::distributed::live_causal_transport_enabled() {
        let token = crate::node_auth::configured_token_for_node(source_node_id)
            .map_err(StableShardFlushError::InvalidSessionIdentity)?;
        let node_metadata = tonic::metadata::MetadataValue::try_from(source_node_id)
            .map_err(|error| StableShardFlushError::InvalidSessionIdentity(error.to_string()))?;
        let token_metadata = tonic::metadata::MetadataValue::try_from(token.as_str())
            .map_err(|error| StableShardFlushError::InvalidSessionIdentity(error.to_string()))?;
        request
            .metadata_mut()
            .insert("x-aarnn-node-id", node_metadata);
        request
            .metadata_mut()
            .insert("x-aarnn-node-token", token_metadata);
    }
    let response = client.stream_shard_frames(request).await?;
    let mut acknowledgements = response.into_inner();
    let mut acknowledged = 0usize;
    while let Some(acknowledgement) = acknowledgements.message().await? {
        if acknowledgement.schema_version != STABLE_SHARD_TRANSPORT_SCHEMA_VERSION
            || !acknowledgement.durable
            || acknowledgement.destination_node_id != destination_node
            || acknowledgement.brain_id != records[0].brain_id.raw()
        {
            return Err(StableShardFlushError::InvalidAcknowledgement(
                "schema, durability, destination or brain identity mismatch".to_owned(),
            ));
        }
        let record = records
            .iter()
            .find(|record| record.sequence == acknowledgement.sequence)
            .ok_or_else(|| {
                StableShardFlushError::InvalidAcknowledgement(format!(
                    "unknown sequence {}",
                    acknowledgement.sequence
                ))
            })?;
        let digest = digest_from_wire(&acknowledgement.record_digest, "record_digest")?;
        if digest != record.record_digest
            || acknowledgement.lease_term != record.lease_term.raw()
            || acknowledgement.fencing_token != record.fencing_token
            || acknowledgement
                .applied_tag
                .as_ref()
                .map(|tag| LogicalTag::new(tag.tick, tag.microstep))
                != Some(record.logical_tag)
        {
            return Err(StableShardFlushError::InvalidAcknowledgement(
                "acknowledgement does not match the sealed record fence or digest".to_owned(),
            ));
        }
        let mut log_guard = log.lock().await;
        log_guard.acknowledge(crate::stable_outbound::StableOutboundAcknowledgement {
            destination_node: destination_node.to_owned(),
            sequence: record.sequence,
            lease_term: record.lease_term,
            fencing_token: record.fencing_token,
            record_digest: record.record_digest,
        })?;
        acknowledged = acknowledged.saturating_add(1);
    }
    Ok(acknowledged)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableShardApplyReceipt {
    pub sequence: u64,
    pub record_digest: StateDigest,
    pub applied_tag: LogicalTag,
    pub state_digest: StateDigest,
    pub duplicate: bool,
    /// Outbound work generated while applying this frame or retained from a
    /// previous crash window. The caller must durably enqueue this work
    /// before acknowledging the inbound frame.
    pub pending_outbound: Vec<PartialShardOutbound>,
}

#[derive(Debug, Error)]
pub enum StableShardReceiverError {
    #[error(transparent)]
    Transport(#[from] StableShardTransportError),
    #[error(transparent)]
    Executor(#[from] PartialShardExecutorError),
    #[error("stable-shard receiver identity mismatch")]
    IdentityMismatch,
    #[error("stable-shard receiver authority is stale")]
    StaleAuthority,
    #[error("stable-shard frame topology or partition generation does not match the receiver plan")]
    GenerationMismatch,
    #[error("stable-shard frame physical placement-plan digest does not match the receiver")]
    PlacementDigestMismatch,
    #[error("stable-shard receipt sequence {sequence} conflicts with a prior digest")]
    ConflictingReceipt { sequence: u64 },
    #[error("stable-shard receipt sequence gap: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("stable-shard durable receipt window reached its bound {0}")]
    ReceiptWindowFull(usize),
    #[error("stable-shard pending outbound window reached its bound {0}")]
    PendingOutboundFull(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReceiverDocument {
    schema_version: u32,
    brain_id: BrainId,
    node_id: String,
    lease_term: LeaseTerm,
    fencing_token: u64,
    #[serde(default)]
    placement_plan_digest: Option<StateDigest>,
    /// Receipt frontiers are scoped by authenticated source node.  Each
    /// source owns an independent sequence space in its durable outbound
    /// log, so a receiver-wide sequence map would incorrectly reject valid
    /// frames whenever two workers send to the same destination.
    receipts: BTreeMap<String, BTreeMap<u64, StateDigest>>,
    checkpoints: Vec<crate::shard_executor::StableShardCheckpoint>,
    #[serde(default)]
    pending_outbound: Vec<PartialShardOutbound>,
    digest: StateDigest,
}

/// Schema-2 receiver documents used one global receipt sequence.  They can
/// be recovered safely only when the deployment supplies exactly one allowed
/// source; with multiple possible sources the historical records cannot be
/// attributed without guessing, so recovery fails closed.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyReceiverDocument {
    schema_version: u32,
    brain_id: BrainId,
    node_id: String,
    lease_term: LeaseTerm,
    fencing_token: u64,
    #[serde(default)]
    placement_plan_digest: Option<StateDigest>,
    receipts: BTreeMap<u64, StateDigest>,
    checkpoints: Vec<crate::shard_executor::StableShardCheckpoint>,
    #[serde(default)]
    pending_outbound: Vec<PartialShardOutbound>,
    digest: StateDigest,
}

impl ReceiverDocument {
    fn from_legacy(legacy: LegacyReceiverDocument, source: Option<String>) -> Self {
        let receipts = source
            .map(|source| {
                let mut by_source = BTreeMap::new();
                by_source.insert(source, legacy.receipts);
                by_source
            })
            .unwrap_or_default();
        Self {
            schema_version: STABLE_SHARD_RECEIVER_SCHEMA_VERSION,
            brain_id: legacy.brain_id,
            node_id: legacy.node_id,
            lease_term: legacy.lease_term,
            fencing_token: legacy.fencing_token,
            placement_plan_digest: legacy.placement_plan_digest,
            receipts,
            checkpoints: legacy.checkpoints,
            pending_outbound: legacy.pending_outbound,
            digest: StateDigest([0; 16]),
        }
    }
}

fn validate_allowed_sources(
    sources: impl IntoIterator<Item = String>,
) -> Result<BTreeSet<String>, StableShardTransportError> {
    let mut allowed = BTreeSet::new();
    for source in sources {
        validate_node_id(&source)?;
        if !allowed.insert(source) {
            return Err(StableShardTransportError::InvalidField(
                "duplicate allowed source node",
            ));
        }
    }
    Ok(allowed)
}

fn validate_receipt_sources(
    receipts: &BTreeMap<String, BTreeMap<u64, StateDigest>>,
    allowed_sources: &BTreeSet<String>,
) -> Result<(), StableShardTransportError> {
    let mut count = 0usize;
    for (source, source_receipts) in receipts {
        validate_node_id(source)?;
        if !allowed_sources.contains(source) {
            return Err(StableShardTransportError::SourceNotAllowed);
        }
        if source_receipts.is_empty() {
            return Err(StableShardTransportError::InvalidField(
                "empty source receipt map",
            ));
        }
        count = count.checked_add(source_receipts.len()).ok_or_else(|| {
            StableShardTransportError::Persistence(
                "stable-shard receipt window exceeds its bound".to_owned(),
            )
        })?;
    }
    if count > MAX_RECEIPTS {
        return Err(StableShardTransportError::Persistence(
            "stable-shard receipt window exceeds its bound".to_owned(),
        ));
    }
    Ok(())
}

/// Durable receiver-side application boundary for a partial worker.
#[derive(Debug)]
pub struct DurableStableShardReceiver {
    path: PathBuf,
    brain_id: BrainId,
    node_id: String,
    lease_term: LeaseTerm,
    fencing_token: u64,
    placement_plan_digest: Option<StateDigest>,
    allowed_sources: BTreeSet<String>,
    receipts: BTreeMap<String, BTreeMap<u64, StateDigest>>,
    pending_outbound: Vec<PartialShardOutbound>,
    executor: PartialShardExecutor,
}

/// A point-in-time view of a receiver's durable application state.
///
/// The snapshot is deliberately made from the same executor/checkpoint
/// boundary that the receiver persists.  Callers may publish it as control
/// plane telemetry, but it never grants a lease or changes the receiver.
#[derive(Debug, Clone)]
pub struct StableShardReceiverSnapshot {
    pub brain_id: BrainId,
    pub node_id: String,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub topology_generation: crate::deterministic::TopologyGeneration,
    pub partition_generation: crate::deterministic::PartitionGeneration,
    pub topology_digest: StateDigest,
    pub plan_digest: StateDigest,
    pub shard_ids: Vec<ShardId>,
    pub owned_shard_ids: Vec<ShardId>,
    pub current_tag: LogicalTag,
    pub state_digest: StateDigest,
    pub checkpoints: Vec<crate::shard_executor::StableShardCheckpoint>,
}

impl DurableStableShardReceiver {
    /// Create and publish a receiver's initial checkpoint before it accepts a
    /// frame.  `allowed_sources` must contain the authenticated peer IDs.
    pub fn new(
        path: impl Into<PathBuf>,
        node_id: impl Into<String>,
        executor: PartialShardExecutor,
        lease_term: LeaseTerm,
        fencing_token: u64,
        allowed_sources: impl IntoIterator<Item = String>,
    ) -> Result<Self, StableShardTransportError> {
        Self::new_with_placement_digest(
            path,
            node_id,
            executor,
            lease_term,
            fencing_token,
            None,
            allowed_sources,
        )
    }

    /// Create a receiver bound to the physical placement plan that assigned
    /// its shards. The optional digest is required for production dispatch;
    /// `new` remains a compatibility constructor for transport-only fixtures.
    pub fn new_with_placement_digest(
        path: impl Into<PathBuf>,
        node_id: impl Into<String>,
        executor: PartialShardExecutor,
        lease_term: LeaseTerm,
        fencing_token: u64,
        placement_plan_digest: Option<StateDigest>,
        allowed_sources: impl IntoIterator<Item = String>,
    ) -> Result<Self, StableShardTransportError> {
        let node_id = node_id.into();
        validate_node_id(&node_id)?;
        let allowed_sources = validate_allowed_sources(allowed_sources)?;
        let receiver = Self {
            path: path.into(),
            brain_id: executor.brain_id_for_transport(),
            node_id,
            lease_term,
            fencing_token,
            placement_plan_digest,
            allowed_sources,
            receipts: BTreeMap::new(),
            pending_outbound: Vec::new(),
            executor,
        };
        receiver.persist()?;
        Ok(receiver)
    }

    /// Reopen a receiver after a process failure.  The persisted checkpoints
    /// and receipt frontier are reconstructed before the node can accept more
    /// frames, so a crash between network retries cannot lose application
    /// state or acknowledge an uncommitted record.
    pub fn open(
        path: impl Into<PathBuf>,
        node_id: impl Into<String>,
        topology: &TopologyGenerationModel,
        plan: CompiledExecutionPlan,
        allowed_sources: impl IntoIterator<Item = String>,
    ) -> Result<Self, StableShardTransportError> {
        Self::open_with_placement_digest(path, node_id, topology, plan, None, allowed_sources)
    }

    /// Reopen a receiver while retaining the physical placement-plan binding.
    pub fn open_with_placement_digest(
        path: impl Into<PathBuf>,
        node_id: impl Into<String>,
        topology: &TopologyGenerationModel,
        plan: CompiledExecutionPlan,
        expected_placement_plan_digest: Option<StateDigest>,
        allowed_sources: impl IntoIterator<Item = String>,
    ) -> Result<Self, StableShardTransportError> {
        let path = path.into();
        let node_id = node_id.into();
        validate_node_id(&node_id)?;
        let allowed_sources = validate_allowed_sources(allowed_sources)?;
        let bytes = std::fs::read(&path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                StableShardTransportError::Persistence(
                    "receiver document schema version is missing".to_owned(),
                )
            })? as u32;
        let document = match schema_version {
            STABLE_SHARD_RECEIVER_SCHEMA_VERSION => {
                let document: ReceiverDocument = serde_json::from_value(value)
                    .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?;
                if document.node_id != node_id || document.digest != receiver_digest(&document)? {
                    return Err(StableShardTransportError::Persistence(
                        "receiver document identity or digest is invalid".to_owned(),
                    ));
                }
                document
            }
            STABLE_SHARD_RECEIVER_LEGACY_SCHEMA_VERSION => {
                let legacy: LegacyReceiverDocument = serde_json::from_value(value)
                    .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?;
                if legacy.node_id != node_id || legacy.digest != legacy_receiver_digest(&legacy)? {
                    return Err(StableShardTransportError::Persistence(
                        "legacy receiver document identity or digest is invalid".to_owned(),
                    ));
                }
                let source = if allowed_sources.len() == 1 {
                    allowed_sources.iter().next().cloned()
                } else if !legacy.receipts.is_empty() {
                    return Err(StableShardTransportError::Persistence(
                        "legacy receiver receipts cannot be attributed to multiple sources"
                            .to_owned(),
                    ));
                } else {
                    None
                };
                ReceiverDocument::from_legacy(legacy, source)
            }
            other => {
                return Err(StableShardTransportError::UnsupportedSchema(other));
            }
        };
        validate_receipt_sources(&document.receipts, &allowed_sources)?;
        if expected_placement_plan_digest.is_some()
            && document.placement_plan_digest != expected_placement_plan_digest
        {
            return Err(StableShardTransportError::Persistence(
                "receiver placement-plan identity does not match the requested authority"
                    .to_owned(),
            ));
        }
        let owned = document
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.shard_id)
            .collect::<Vec<_>>();
        let executor = PartialShardExecutor::from_checkpoints(
            document.brain_id,
            topology,
            plan,
            document.checkpoints,
            owned,
            1024,
        )?;
        Ok(Self {
            path,
            brain_id: document.brain_id,
            node_id,
            lease_term: document.lease_term,
            fencing_token: document.fencing_token,
            placement_plan_digest: document
                .placement_plan_digest
                .or(expected_placement_plan_digest),
            allowed_sources,
            receipts: document.receipts,
            pending_outbound: document.pending_outbound,
            executor,
        })
    }

    pub fn executor(&self) -> &PartialShardExecutor {
        &self.executor
    }

    pub fn lease_term(&self) -> LeaseTerm {
        self.lease_term
    }

    pub fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    pub fn owned_shard_ids(&self) -> Vec<ShardId> {
        self.executor.owned_shard_ids()
    }

    /// Stable identity used by the node-scoped receiver registry.  The
    /// registry never derives this identity from a frame or discovery data.
    pub const fn brain_id(&self) -> BrainId {
        self.brain_id
    }

    /// Authenticated physical node identity bound when the receiver was
    /// opened.  A receiver cannot be registered on another node.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn executor_mut(&mut self) -> &mut PartialShardExecutor {
        &mut self.executor
    }

    /// Return the next sequence for one authenticated source stream.
    pub fn next_sequence_for_source(&self, source_node_id: &str) -> u64 {
        self.receipts
            .get(source_node_id)
            .and_then(|receipts| receipts.last_key_value())
            .map(|(sequence, _)| sequence.saturating_add(1))
            .unwrap_or(0)
    }

    /// Compatibility telemetry for callers that historically observed a
    /// receiver-wide frontier. With multiple sources this returns the
    /// greatest source frontier; correctness-sensitive callers must use
    /// [`Self::next_sequence_for_source`].
    pub fn next_sequence(&self) -> u64 {
        self.receipts
            .values()
            .filter_map(|receipts| receipts.last_key_value())
            .map(|(sequence, _)| sequence.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// Return the durable generated work awaiting insertion into the local
    /// outbound log. The slice is immutable; acknowledgement requires the
    /// exact prefix through [`Self::acknowledge_pending_outbound`].
    pub fn pending_outbound(&self) -> &[PartialShardOutbound] {
        &self.pending_outbound
    }

    /// Capture durable worker telemetry without exposing mutable executor
    /// state.  Checkpoint digests are retained per shard so the orchestrator
    /// can correlate an acknowledgement with the exact applied cut.
    pub fn snapshot(&self) -> Result<StableShardReceiverSnapshot, StableShardTransportError> {
        let plan = self.executor.plan();
        Ok(StableShardReceiverSnapshot {
            brain_id: self.brain_id,
            node_id: self.node_id.clone(),
            lease_term: self.lease_term,
            fencing_token: self.fencing_token,
            topology_generation: plan.topology_generation(),
            partition_generation: plan.partition_generation(),
            topology_digest: plan.topology_digest(),
            plan_digest: plan.digest(),
            shard_ids: plan.shard_ids().collect(),
            owned_shard_ids: self.executor.owned_shard_ids(),
            current_tag: self.executor.current_tag(),
            state_digest: self.executor.state_digest()?,
            checkpoints: self.executor.checkpoint_shards()?,
        })
    }

    pub fn apply(
        &mut self,
        frame: proto::StableShardFrame,
    ) -> Result<StableShardApplyReceipt, StableShardReceiverError> {
        let decoded = decode_frame(frame)?;
        if !self.allowed_sources.contains(&decoded.source_node_id) {
            return Err(StableShardTransportError::SourceNotAllowed.into());
        }
        let record = decoded.record;
        if record.brain_id != self.brain_id || record.destination_node != self.node_id {
            return Err(StableShardTransportError::InvalidField("receiver identity").into());
        }
        if record.lease_term != self.lease_term || record.fencing_token != self.fencing_token {
            return Err(StableShardReceiverError::StaleAuthority);
        }
        if record.topology_generation != self.executor.plan().topology_generation()
            || record.partition_generation != self.executor.plan().partition_generation()
        {
            return Err(StableShardReceiverError::GenerationMismatch);
        }
        if self
            .placement_plan_digest
            .is_some_and(|expected| expected != record.placement_plan_digest)
        {
            return Err(StableShardReceiverError::PlacementDigestMismatch);
        }
        let source_node_id = decoded.source_node_id;
        if let Some(previous) = self
            .receipts
            .get(&source_node_id)
            .and_then(|receipts| receipts.get(&record.sequence))
        {
            if previous != &record.record_digest {
                return Err(StableShardReceiverError::ConflictingReceipt {
                    sequence: record.sequence,
                });
            }
            return Ok(StableShardApplyReceipt {
                sequence: record.sequence,
                record_digest: record.record_digest,
                applied_tag: record.logical_tag,
                state_digest: self.executor.state_digest()?,
                duplicate: true,
                pending_outbound: self.pending_outbound.clone(),
            });
        }
        let expected = self.next_sequence_for_source(&source_node_id);
        if record.sequence != expected {
            return Err(StableShardReceiverError::SequenceGap {
                expected,
                received: record.sequence,
            });
        }
        let receipt_count = self.receipts.values().map(BTreeMap::len).sum::<usize>();
        if receipt_count >= MAX_RECEIPTS {
            return Err(StableShardReceiverError::ReceiptWindowFull(MAX_RECEIPTS));
        }
        let before = self.executor.clone();
        let mut staged = self.executor.clone();
        let applied = staged.apply_outbound(record.message.clone())?;
        let mut generated_outbound = applied.outbound;
        for step in staged.settle(MAX_RECEIVER_STEPS_PER_FRAME)? {
            generated_outbound.extend(step.outbound);
        }
        if self
            .pending_outbound
            .len()
            .saturating_add(generated_outbound.len())
            > MAX_PENDING_OUTBOUND
        {
            return Err(StableShardReceiverError::PendingOutboundFull(
                MAX_PENDING_OUTBOUND,
            ));
        }
        let generated_count = generated_outbound.len();
        self.executor = staged;
        self.receipts
            .entry(source_node_id.clone())
            .or_default()
            .insert(record.sequence, record.record_digest);
        self.pending_outbound.extend(generated_outbound);
        if let Err(error) = self.persist() {
            self.executor = before;
            if let Some(source_receipts) = self.receipts.get_mut(&source_node_id) {
                source_receipts.remove(&record.sequence);
                if source_receipts.is_empty() {
                    self.receipts.remove(&source_node_id);
                }
            }
            self.pending_outbound
                .truncate(self.pending_outbound.len().saturating_sub(generated_count));
            return Err(error.into());
        }
        Ok(StableShardApplyReceipt {
            sequence: record.sequence,
            record_digest: record.record_digest,
            applied_tag: record.logical_tag,
            state_digest: self.executor.state_digest()?,
            duplicate: false,
            pending_outbound: self.pending_outbound.clone(),
        })
    }

    /// Clear exactly the pending generated work that has been durably sealed
    /// in the local outbound log. The caller must pass the complete pending
    /// prefix returned by [`Self::apply`]; a mismatch fails closed so a
    /// partially acknowledged output set cannot be forgotten.
    pub fn acknowledge_pending_outbound(
        &mut self,
        sealed: &[PartialShardOutbound],
    ) -> Result<(), StableShardTransportError> {
        if sealed.len() > self.pending_outbound.len()
            || self.pending_outbound[..sealed.len()] != *sealed
        {
            return Err(StableShardTransportError::Persistence(
                "sealed outbound work does not match the receiver pending prefix".to_owned(),
            ));
        }
        if sealed.is_empty() {
            return Ok(());
        }
        self.pending_outbound.drain(..sealed.len());
        if let Err(error) = self.persist() {
            // The durable document still contains the old pending prefix if
            // publication failed; restore the in-memory view for retry.
            self.pending_outbound.splice(0..0, sealed.iter().cloned());
            return Err(error);
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), StableShardTransportError> {
        let checkpoints = self.executor.checkpoint_shards()?;
        let mut document = ReceiverDocument {
            schema_version: STABLE_SHARD_RECEIVER_SCHEMA_VERSION,
            brain_id: self.brain_id,
            node_id: self.node_id.clone(),
            lease_term: self.lease_term,
            fencing_token: self.fencing_token,
            placement_plan_digest: self.placement_plan_digest,
            receipts: self.receipts.clone(),
            checkpoints,
            pending_outbound: self.pending_outbound.clone(),
            digest: StateDigest([0; 16]),
        };
        document.digest = receiver_digest(&document)?;
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(StableShardTransportError::FrameTooLarge(bytes.len()));
        }
        atomic_replace_and_sync(&self.path, &bytes)?;
        Ok(())
    }
}

/// Explicit receiver set owned by one distributed node.
///
/// Registration is a local authority handoff performed by the bootstrap or
/// migration adapter.  A protobuf frame, resource observation, or discovery
/// result can only select an already registered brain; it can never create a
/// receiver or grant shard ownership.  The map is read briefly to select a
/// receiver and the receiver mutex then serialises its durable sequence
/// frontier for that brain.
#[derive(Clone, Debug)]
pub struct StableShardReceiverRegistry {
    node_id: String,
    receivers: Arc<RwLock<BTreeMap<BrainId, Arc<Mutex<DurableStableShardReceiver>>>>>,
}

impl StableShardReceiverRegistry {
    pub fn new(node_id: impl Into<String>) -> Result<Self, StableShardTransportError> {
        let node_id = node_id.into();
        validate_node_id(&node_id)?;
        Ok(Self {
            node_id,
            receivers: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    /// Construct an empty registry for a node whose identity is validated by
    /// the enclosing node bootstrap. No receiver is registered by this call.
    pub fn empty(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            receivers: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn register(
        &self,
        receiver: DurableStableShardReceiver,
    ) -> Result<(), StableShardTransportError> {
        if receiver.node_id() != self.node_id {
            return Err(StableShardTransportError::ReceiverNodeMismatch {
                expected: self.node_id.clone(),
                actual: receiver.node_id().to_owned(),
            });
        }
        let brain_id = receiver.brain_id();
        let mut receivers = self
            .receivers
            .write()
            .map_err(|_| StableShardTransportError::RegistryLock)?;
        if receivers.contains_key(&brain_id) {
            return Err(StableShardTransportError::ReceiverAlreadyRegistered(
                brain_id,
            ));
        }
        receivers.insert(brain_id, Arc::new(Mutex::new(receiver)));
        Ok(())
    }

    pub fn unregister(&self, brain_id: BrainId) -> Result<bool, StableShardTransportError> {
        Ok(self
            .receivers
            .write()
            .map_err(|_| StableShardTransportError::RegistryLock)?
            .remove(&brain_id)
            .is_some())
    }

    pub fn contains(&self, brain_id: BrainId) -> Result<bool, StableShardTransportError> {
        Ok(self
            .receivers
            .read()
            .map_err(|_| StableShardTransportError::RegistryLock)?
            .contains_key(&brain_id))
    }

    /// Return durable snapshots in brain-ID order for bounded control-plane
    /// telemetry.  Receiver mutexes are held one at a time, never across an
    /// await or another receiver, so independent brains remain independent.
    pub fn snapshots(&self) -> Result<Vec<StableShardReceiverSnapshot>, StableShardTransportError> {
        let receivers = self
            .receivers
            .read()
            .map_err(|_| StableShardTransportError::RegistryLock)?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        receivers
            .into_iter()
            .map(|receiver| {
                receiver
                    .lock()
                    .map_err(|_| StableShardTransportError::RegistryLock)?
                    .snapshot()
            })
            .collect()
    }

    fn receiver(
        &self,
        brain_id: BrainId,
    ) -> Result<Arc<Mutex<DurableStableShardReceiver>>, StableShardTransportError> {
        self.receivers
            .read()
            .map_err(|_| StableShardTransportError::RegistryLock)?
            .get(&brain_id)
            .cloned()
            .ok_or(StableShardTransportError::ReceiverNotRegistered(brain_id))
    }

    fn apply(
        &self,
        frame: proto::StableShardFrame,
        session_source_node: &str,
    ) -> Result<StableShardReceiverApply, StableShardReceiverError> {
        if frame.source_node_id != session_source_node {
            return Err(StableShardTransportError::SessionSourceMismatch.into());
        }
        let brain_id = BrainId::new(frame.brain_id).map_err(StableShardTransportError::from)?;
        let receiver = self.receiver(brain_id)?;
        let mut receiver = receiver
            .lock()
            .map_err(|_| StableShardTransportError::RegistryLock)?;
        let receipt = receiver.apply(frame)?;
        Ok(StableShardReceiverApply {
            receipt,
            brain_id: receiver.brain_id(),
            destination_node_id: receiver.node_id().to_owned(),
            lease_term: receiver.lease_term,
            fencing_token: receiver.fencing_token,
            pending_outbound: receiver.pending_outbound.clone(),
        })
    }

    fn acknowledge_pending_outbound(
        &self,
        brain_id: BrainId,
        sealed: &[PartialShardOutbound],
    ) -> Result<(), StableShardReceiverError> {
        let receiver = self.receiver(brain_id)?;
        let mut receiver = receiver
            .lock()
            .map_err(|_| StableShardTransportError::RegistryLock)?;
        receiver
            .acknowledge_pending_outbound(sealed)
            .map_err(StableShardReceiverError::from)
    }
}

#[derive(Debug)]
struct StableShardReceiverApply {
    receipt: StableShardApplyReceipt,
    brain_id: BrainId,
    destination_node_id: String,
    lease_term: LeaseTerm,
    fencing_token: u64,
    pending_outbound: Vec<PartialShardOutbound>,
}

fn receiver_digest(document: &ReceiverDocument) -> Result<StateDigest, StableShardTransportError> {
    let mut material = document.clone();
    material.digest = StateDigest([0; 16]);
    let bytes = serde_json::to_vec(&material)
        .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("stable-shard-receiver:v3", bytes);
    Ok(digest.finish())
}

fn legacy_receiver_digest(
    document: &LegacyReceiverDocument,
) -> Result<StateDigest, StableShardTransportError> {
    let mut material = document.clone();
    material.digest = StateDigest([0; 16]);
    let bytes = serde_json::to_vec(&material)
        .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("stable-shard-receiver:v2", bytes);
    Ok(digest.finish())
}

impl From<StableShardApplyReceipt> for proto::StableShardAcknowledgement {
    fn from(receipt: StableShardApplyReceipt) -> Self {
        Self {
            schema_version: STABLE_SHARD_TRANSPORT_SCHEMA_VERSION,
            brain_id: 0,
            destination_node_id: String::new(),
            sequence: receipt.sequence,
            lease_term: 0,
            fencing_token: 0,
            record_digest: receipt.record_digest.0.to_vec(),
            duplicate: receipt.duplicate,
            durable: true,
            applied_tag: Some(proto::LogicalTag {
                tick: receipt.applied_tag.tick,
                microstep: receipt.applied_tag.microstep,
            }),
            state_digest: receipt.state_digest.0.to_vec(),
        }
    }
}

/// Tonic adapter for the durable receiver boundary.
#[derive(Clone)]
pub struct StableShardDataPlaneService {
    registry: StableShardReceiverRegistry,
    dispatchers: Arc<RwLock<BTreeMap<BrainId, StableShardDispatcher>>>,
}

impl StableShardDataPlaneService {
    pub fn new(receiver: DurableStableShardReceiver) -> Self {
        let registry = StableShardReceiverRegistry::empty(receiver.node_id().to_owned());
        registry
            .register(receiver)
            .expect("receiver identity must match its newly created registry");
        Self {
            registry,
            dispatchers: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn empty(node_id: impl Into<String>) -> Self {
        Self {
            registry: StableShardReceiverRegistry::empty(node_id),
            dispatchers: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn from_registry(registry: StableShardReceiverRegistry) -> Self {
        Self {
            registry,
            dispatchers: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn registry(&self) -> StableShardReceiverRegistry {
        self.registry.clone()
    }

    /// Attach a durable placement-aware outbound queue for an explicitly
    /// registered brain. Incoming frames cannot create this association.
    pub fn register_dispatcher(
        &self,
        brain_id: BrainId,
        dispatcher: StableShardDispatcher,
    ) -> Result<(), StableShardTransportError> {
        if dispatcher
            .brain_id()
            .map_err(|error| StableShardTransportError::Persistence(error.to_string()))?
            != brain_id
        {
            return Err(StableShardTransportError::Persistence(
                "stable-shard dispatcher brain identity does not match the receiver".to_owned(),
            ));
        }
        let mut dispatchers = self
            .dispatchers
            .write()
            .map_err(|_| StableShardTransportError::RegistryLock)?;
        if dispatchers.insert(brain_id, dispatcher).is_some() {
            return Err(StableShardTransportError::Persistence(
                "stable-shard dispatcher is already registered for this brain".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn unregister_dispatcher(
        &self,
        brain_id: BrainId,
    ) -> Result<bool, StableShardTransportError> {
        Ok(self
            .dispatchers
            .write()
            .map_err(|_| StableShardTransportError::RegistryLock)?
            .remove(&brain_id)
            .is_some())
    }

    /// Flush all registered worker outboxes concurrently.  A failed
    /// destination remains pending in its durable log and is retried by the
    /// next lifecycle pass; successful destinations may be acknowledged in
    /// the same pass.
    pub async fn dispatch_pending(&self) -> Result<usize, StableShardDispatchError> {
        let dispatchers = self
            .dispatchers
            .read()
            .map_err(|_| StableShardDispatchError::EndpointLock)?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let results = futures_util::future::join_all(
            dispatchers
                .into_iter()
                .map(|dispatcher| async move { dispatcher.dispatch_pending().await }),
        )
        .await;
        results.into_iter().try_fold(0usize, |total, result| {
            result.map(|report| total + report.acknowledged_records)
        })
    }
}

fn status_for(error: &StableShardReceiverError) -> Status {
    match error {
        StableShardReceiverError::StaleAuthority
        | StableShardReceiverError::GenerationMismatch
        | StableShardReceiverError::PlacementDigestMismatch
        | StableShardReceiverError::SequenceGap { .. }
        | StableShardReceiverError::ConflictingReceipt { .. }
        | StableShardReceiverError::ReceiptWindowFull(_)
        | StableShardReceiverError::PendingOutboundFull(_) => {
            Status::failed_precondition(error.to_string())
        }
        StableShardReceiverError::Transport(StableShardTransportError::SourceNotAllowed) => {
            Status::permission_denied(error.to_string())
        }
        StableShardReceiverError::Transport(StableShardTransportError::SessionSourceMismatch) => {
            Status::permission_denied(error.to_string())
        }
        StableShardReceiverError::Transport(_)
        | StableShardReceiverError::IdentityMismatch
        | StableShardReceiverError::Executor(_) => Status::invalid_argument(error.to_string()),
    }
}

fn status_for_dispatch(error: &StableShardDispatchError) -> Status {
    match error {
        StableShardDispatchError::Transport(StableShardFlushError::Rpc(status)) => status.clone(),
        _ => Status::failed_precondition(error.to_string()),
    }
}

#[tonic::async_trait]
impl proto::stable_shard_data_plane_server::StableShardDataPlane for StableShardDataPlaneService {
    type StreamShardFramesStream =
        ReceiverStream<Result<proto::StableShardAcknowledgement, Status>>;

    async fn stream_shard_frames(
        &self,
        request: Request<tonic::Streaming<proto::StableShardFrame>>,
    ) -> Result<Response<Self::StreamShardFramesStream>, Status> {
        let request_metadata = request.metadata().clone();
        let peer_certificate_sha256 = request.peer_certs().and_then(|certificates| {
            certificates
                .first()
                .map(|certificate| certificate_sha256_der(certificate.as_ref()))
        });
        let session_source_node = request
            .metadata()
            .get(STABLE_SHARD_SOURCE_NODE_METADATA)
            .ok_or_else(|| {
                Status::unauthenticated("stable-shard source session identity metadata is required")
            })?
            .to_str()
            .map_err(|_| {
                Status::unauthenticated("stable-shard source session identity metadata is invalid")
            })?
            .to_owned();
        validate_node_id(&session_source_node)
            .map_err(|error| Status::unauthenticated(error.to_string()))?;
        if live_causal_transport_enabled() {
            validate_peer_metadata(
                &request_metadata,
                &session_source_node,
                peer_certificate_sha256.as_deref(),
            )?;
        }
        let mut stream = request.into_inner();
        let registry = self.registry.clone();
        let dispatchers = Arc::clone(&self.dispatchers);
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Ok(Some(frame)) = stream.message().await {
                let apply_registry = registry.clone();
                let session_source_node = session_source_node.clone();
                let result = tokio::task::spawn_blocking(move || {
                    apply_registry
                        .apply(frame, &session_source_node)
                        .map_err(|error| match error {
                            StableShardReceiverError::Transport(
                                StableShardTransportError::ReceiverNotRegistered(_),
                            ) => Status::not_found(error.to_string()),
                            StableShardReceiverError::Transport(
                                StableShardTransportError::RegistryLock,
                            ) => Status::internal(error.to_string()),
                            other => status_for(&other),
                        })
                })
                .await
                .unwrap_or_else(|_| Err(Status::internal("stable-shard receiver task failed")));
                match result {
                    Ok(applied) => {
                        if !applied.pending_outbound.is_empty() {
                            let dispatcher = dispatchers
                                .read()
                                .ok()
                                .and_then(|entries| entries.get(&applied.brain_id).cloned());
                            let Some(dispatcher) = dispatcher else {
                                let _ = tx
                                    .send(Err(Status::failed_precondition(
                                        "stable-shard application generated outbound work but no dispatcher is registered",
                                    )))
                                    .await;
                                break;
                            };
                            if let Err(error) = dispatcher
                                .enqueue_batch(applied.pending_outbound.clone())
                                .await
                            {
                                let _ = tx.send(Err(status_for_dispatch(&error))).await;
                                break;
                            }
                            let clear_registry = registry.clone();
                            let brain_id = applied.brain_id;
                            let sealed = applied.pending_outbound.clone();
                            let clear_result = tokio::task::spawn_blocking(move || {
                                clear_registry
                                    .acknowledge_pending_outbound(brain_id, &sealed)
                                    .map_err(|error| status_for(&error))
                            })
                            .await
                            .unwrap_or_else(|_| {
                                Err(Status::internal(
                                    "stable-shard pending outbound acknowledgement task failed",
                                ))
                            });
                            if let Err(status) = clear_result {
                                let _ = tx.send(Err(status)).await;
                                break;
                            }
                        }
                        let mut acknowledgement =
                            proto::StableShardAcknowledgement::from(applied.receipt);
                        acknowledgement.brain_id = applied.brain_id.raw();
                        acknowledgement.destination_node_id = applied.destination_node_id;
                        acknowledgement.lease_term = applied.lease_term.raw();
                        acknowledgement.fencing_token = applied.fencing_token;
                        if tx.send(Ok(acknowledgement)).await.is_err() {
                            break;
                        }
                    }
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        break;
                    }
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::StateDigest;
    use crate::partial_shard_executor::PartialShardOutbound;
    use crate::stable_outbound::StableOutboundLog;

    #[test]
    fn frame_round_trip_preserves_sealed_metadata() {
        let message = PartialShardOutbound::SynapseEffect {
            plan_digest: StateDigest([7; 16]),
            destination_shard: ShardId::new(4).unwrap(),
            event_id: EventId::new(8).unwrap(),
            logical_tag: LogicalTag::new(9, 2),
            synapse: crate::deterministic::SynapseId::new(10).unwrap(),
            charge: 11,
        };
        let path = std::env::temp_dir().join(format!(
            "aarnn-stable-shard-transport-test-{}-{}.json",
            std::process::id(),
            1u64
        ));
        let _ = std::fs::remove_file(&path);
        let mut log = StableOutboundLog::open(&path, BrainId::new(1).unwrap(), 4).unwrap();
        let record = log
            .append("worker-b", LeaseTerm::INITIAL, 3, message)
            .unwrap();
        let frame = encode_frame(&record, "worker-a").unwrap();
        let decoded = decode_frame(frame).unwrap();
        assert_eq!(decoded.source_node_id, "worker-a");
        assert_eq!(decoded.record, record);
        let _ = std::fs::remove_file(path);
    }
}
