//! Crash safe outbound handoff for physically distributed stable shards.
//!
//! A partial worker may finish a deterministic transition before the
//! destination worker is reachable.  The transition therefore publishes
//! typed [`PartialShardOutbound`] messages to this log before it reports the
//! cut as complete.  Each destination has an independent sequence space,
//! bounded pending queue and durable acknowledgement frontier.  Retries send
//! the same sealed record; they never allocate a new logical event or use
//! packet arrival time.
//!
//! This is a local persistence adapter.  It provides crash safe append,
//! replay and fencing tests, but it is not a quorum or network transport.  A
//! server adapter must place the file behind the approved replicated owner
//! and use the record metadata when admitting it at the destination.

use crate::deterministic::{
    BrainId, EventId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest,
    StateDigestBuilder, TopologyGeneration,
};
use crate::durability::{DurabilityError, atomic_replace_and_sync};
use crate::partial_shard_executor::PartialShardOutbound;
use crate::placement_registry::PlacementRegistry;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const STABLE_OUTBOUND_SCHEMA_VERSION: u32 = 1;
const MAX_DESTINATION_ID_BYTES: usize = 256;
const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const MAX_BATCH_RECORDS: usize = 4096;

fn initial_topology_generation() -> TopologyGeneration {
    TopologyGeneration::INITIAL
}

fn initial_partition_generation() -> PartitionGeneration {
    PartitionGeneration::INITIAL
}

fn zero_state_digest() -> StateDigest {
    StateDigest([0; 16])
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StableOutboundError {
    #[error(transparent)]
    Durability(#[from] DurabilityError),
    #[error("stable outbound destination identity is invalid")]
    InvalidDestination,
    #[error("stable outbound queue for {destination} reached its bound {capacity}")]
    QueueFull {
        destination: String,
        capacity: usize,
    },
    #[error("stable outbound record exceeds the configured bound ({bytes} bytes)")]
    RecordTooLarge { bytes: usize },
    #[error("stable outbound sequence space is exhausted")]
    SequenceOverflow,
    #[error("stable outbound log is corrupt: {0}")]
    Corrupt(String),
    #[error(
        "stable outbound authority is stale: expected term {expected_term}/fence {expected_fencing_token}, received term {received_term}/fence {received_fencing_token}"
    )]
    StaleAuthority {
        expected_term: LeaseTerm,
        expected_fencing_token: u64,
        received_term: LeaseTerm,
        received_fencing_token: u64,
    },
    #[error("stable outbound acknowledgement does not match the sealed record")]
    AcknowledgementMismatch,
    #[error("stable outbound acknowledgement sequence {0} is unknown")]
    UnknownSequence(u64),
    #[error("stable outbound destination shard {0} is not in the active placement")]
    UnknownShard(ShardId),
    #[error("stable outbound message plan digest does not match the shard authority")]
    PlanMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableOutboundRecord {
    pub sequence: u64,
    pub brain_id: BrainId,
    pub destination_node: String,
    pub destination_shard: ShardId,
    pub plan_digest: StateDigest,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub logical_tag: LogicalTag,
    pub event_id: EventId,
    #[serde(default = "initial_topology_generation")]
    pub topology_generation: TopologyGeneration,
    #[serde(default = "initial_partition_generation")]
    pub partition_generation: PartitionGeneration,
    /// Physical placement-plan identity. This is distinct from the compiled
    /// execution-plan digest carried inside `PartialShardOutbound`.
    #[serde(default = "zero_state_digest")]
    pub placement_plan_digest: StateDigest,
    pub message: PartialShardOutbound,
    pub record_digest: StateDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableOutboundAcknowledgement {
    pub destination_node: String,
    pub sequence: u64,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub record_digest: StateDigest,
}

impl StableOutboundRecord {
    /// Recompute and verify the sealed record digest before it crosses a
    /// transport boundary.  A protobuf frame is untrusted input even when it
    /// arrived over an authenticated session; the digest binds the complete
    /// typed message and all routing/fencing metadata.
    pub fn verify_integrity(&self) -> Result<(), StableOutboundError> {
        if self.record_digest != record_digest(self)? {
            return Err(StableOutboundError::Corrupt(
                "stable outbound record digest verification failed".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StableOutboundStream {
    next_sequence: u64,
    acknowledged_through: Option<u64>,
    acknowledged_digest: Option<StateDigest>,
    authority_term: LeaseTerm,
    authority_fencing_token: u64,
    pending: Vec<StableOutboundRecord>,
}

impl Default for StableOutboundStream {
    fn default() -> Self {
        Self {
            next_sequence: 0,
            acknowledged_through: None,
            acknowledged_digest: None,
            authority_term: LeaseTerm::INITIAL,
            authority_fencing_token: 0,
            pending: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StableOutboundDocument {
    schema_version: u32,
    brain_id: BrainId,
    streams: BTreeMap<String, StableOutboundStream>,
    digest: StateDigest,
}

#[derive(Debug)]
pub struct StableOutboundLog {
    path: PathBuf,
    lock_path: PathBuf,
    brain_id: BrainId,
    max_pending_per_destination: usize,
    streams: BTreeMap<String, StableOutboundStream>,
}

impl StableOutboundLog {
    /// Open or create a bounded outbound log. Existing bytes are validated
    /// before the handle is returned; malformed state fails closed.
    pub fn open(
        path: impl Into<PathBuf>,
        brain_id: BrainId,
        max_pending_per_destination: usize,
    ) -> Result<Self, StableOutboundError> {
        if max_pending_per_destination == 0 {
            return Err(StableOutboundError::Corrupt(
                "outbound queue bound must be positive".to_owned(),
            ));
        }
        let path = path.into();
        let lock_path = path.with_extension("stable-outbound.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| DurabilityError::Io(error.to_string()))?;
        }
        let mut log = Self {
            path,
            lock_path,
            brain_id,
            max_pending_per_destination,
            streams: BTreeMap::new(),
        };
        log.streams = log.with_locked_read(|streams| Ok(streams.clone()))?;
        Ok(log)
    }

    pub fn pending(
        &mut self,
        destination_node: &str,
    ) -> Result<Vec<StableOutboundRecord>, StableOutboundError> {
        validate_destination(destination_node)?;
        self.with_locked_read(|streams| {
            Ok(streams
                .get(destination_node)
                .map(|stream| stream.pending.clone())
                .unwrap_or_default())
        })
    }

    pub fn next_sequence(&mut self, destination_node: &str) -> Result<u64, StableOutboundError> {
        validate_destination(destination_node)?;
        self.with_locked_read(|streams| {
            Ok(streams
                .get(destination_node)
                .map(|stream| stream.next_sequence)
                .unwrap_or(0))
        })
    }

    /// Return the bounded set of physical destinations with durable stream
    /// state. Callers use this to schedule independent network flushes without
    /// exposing the log's internal document representation.
    pub fn destinations(&mut self) -> Result<Vec<String>, StableOutboundError> {
        self.with_locked_read(|streams| Ok(streams.keys().cloned().collect()))
    }

    /// Advance a destination's authority without appending data. This fences
    /// an old sender before reassignment or migration.
    pub fn fence(
        &mut self,
        destination_node: &str,
        lease_term: LeaseTerm,
        fencing_token: u64,
    ) -> Result<(), StableOutboundError> {
        validate_destination(destination_node)?;
        self.with_locked_update(|streams| {
            let stream = streams.entry(destination_node.to_owned()).or_default();
            validate_authority(stream, lease_term, fencing_token)?;
            if (lease_term, fencing_token) > (stream.authority_term, stream.authority_fencing_token)
            {
                stream.authority_term = lease_term;
                stream.authority_fencing_token = fencing_token;
            }
            Ok(())
        })
    }

    /// Append one message and return its sealed durable record. The caller
    /// may retry the returned record until the receiver acknowledges it.
    pub fn append(
        &mut self,
        destination_node: &str,
        lease_term: LeaseTerm,
        fencing_token: u64,
        message: PartialShardOutbound,
    ) -> Result<StableOutboundRecord, StableOutboundError> {
        self.append_generation_bound(
            destination_node,
            lease_term,
            fencing_token,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            message,
        )
    }

    /// Append a record with the exact topology and partition generations
    /// selected by the active placement plan.  The compatibility `append`
    /// method remains available for initial-generation reference fixtures.
    pub fn append_generation_bound(
        &mut self,
        destination_node: &str,
        lease_term: LeaseTerm,
        fencing_token: u64,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        message: PartialShardOutbound,
    ) -> Result<StableOutboundRecord, StableOutboundError> {
        self.append_generation_bound_with_placement(
            destination_node,
            lease_term,
            fencing_token,
            topology_generation,
            partition_generation,
            StateDigest([0; 16]),
            message,
        )
    }

    /// Append a record bound to both the compiled execution plan and the
    /// physical placement plan that selected its destination.
    pub fn append_generation_bound_with_placement(
        &mut self,
        destination_node: &str,
        lease_term: LeaseTerm,
        fencing_token: u64,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        placement_plan_digest: StateDigest,
        message: PartialShardOutbound,
    ) -> Result<StableOutboundRecord, StableOutboundError> {
        validate_destination(destination_node)?;
        self.append_prepared_batch([(
            destination_node.to_owned(),
            lease_term,
            fencing_token,
            topology_generation,
            partition_generation,
            placement_plan_digest,
            message,
        )])
        .map(|mut records| records.remove(0))
    }

    /// Resolve a virtual shard through the authoritative placement registry
    /// and append only to its current active node. The plan digest and fence
    /// are copied from the same authority record, so callers cannot route a
    /// message to a stale or merely observed worker by supplying node text.
    pub fn append_for_shard(
        &mut self,
        placement: &PlacementRegistry,
        message: PartialShardOutbound,
    ) -> Result<StableOutboundRecord, StableOutboundError> {
        self.append_for_shard_generation_bound(placement, message)
    }

    /// Placement-aware variant that binds every physical handoff to the
    /// active plan's topology and partition generations.
    pub fn append_for_shard_generation_bound(
        &mut self,
        placement: &PlacementRegistry,
        message: PartialShardOutbound,
    ) -> Result<StableOutboundRecord, StableOutboundError> {
        self.append_for_shard_generation_bound_batch(placement, [message])
            .map(|mut records| records.remove(0))
    }

    /// Atomically append a bounded batch of placement-resolved messages.
    /// Every record is prepared against the same placement snapshot and the
    /// durable document is replaced only after all records validate. This is
    /// the commit boundary used by partial workers: a failed later record
    /// cannot leave an earlier outbound message stranded without its matching
    /// biological state transition.
    pub fn append_for_shard_generation_bound_batch<I>(
        &mut self,
        placement: &PlacementRegistry,
        messages: I,
    ) -> Result<Vec<StableOutboundRecord>, StableOutboundError>
    where
        I: IntoIterator<Item = PartialShardOutbound>,
    {
        let messages = messages
            .into_iter()
            .take(MAX_BATCH_RECORDS.saturating_add(1))
            .collect::<Vec<_>>();
        if messages.len() > MAX_BATCH_RECORDS {
            return Err(StableOutboundError::RecordTooLarge {
                bytes: messages.len(),
            });
        }
        let plan = placement
            .active_plan
            .as_ref()
            .ok_or(StableOutboundError::UnknownShard(
                message_metadata(messages.first().ok_or_else(|| {
                    StableOutboundError::Corrupt("outbound batch is empty".to_owned())
                })?)
                .1,
            ))?;
        let mut prepared = Vec::with_capacity(messages.len());
        for message in messages {
            let (_, destination_shard, _, _) = message_metadata(&message);
            let authority = placement
                .authority(destination_shard)
                .ok_or(StableOutboundError::UnknownShard(destination_shard))?;
            prepared.push((
                authority.node_id.clone(),
                authority.lease_term,
                authority.fencing_token,
                plan.topology_generation,
                plan.partition_generation,
                plan.digest(),
                message,
            ));
        }
        self.append_prepared_batch(prepared)
    }

    fn append_prepared_batch<I>(
        &mut self,
        prepared: I,
    ) -> Result<Vec<StableOutboundRecord>, StableOutboundError>
    where
        I: IntoIterator<
            Item = (
                String,
                LeaseTerm,
                u64,
                TopologyGeneration,
                PartitionGeneration,
                StateDigest,
                PartialShardOutbound,
            ),
        >,
    {
        let prepared = prepared.into_iter().collect::<Vec<_>>();
        if prepared.is_empty() || prepared.len() > MAX_BATCH_RECORDS {
            return Err(StableOutboundError::Corrupt(
                "outbound batch size is outside its configured bound".to_owned(),
            ));
        }
        let max_pending_per_destination = self.max_pending_per_destination;
        let brain_id = self.brain_id;
        self.with_locked_update(|streams| {
            // Work on a private copy so any validation, size or queue failure
            // leaves the in-memory view consistent with the durable file.
            let mut staged = streams.clone();
            let mut records = Vec::with_capacity(prepared.len());
            for (
                destination_node,
                lease_term,
                fencing_token,
                topology_generation,
                partition_generation,
                placement_plan_digest,
                message,
            ) in prepared
            {
                validate_destination(&destination_node)?;
                let (plan_digest, destination_shard, logical_tag, event_id) =
                    message_metadata(&message);
                let stream = staged.entry(destination_node.clone()).or_default();
                validate_authority(stream, lease_term, fencing_token)?;
                // A receiver may recover after it durably applied an inbound
                // frame but before the generated outbound work was cleared
                // from its pending journal. Re-enqueuing that exact message
                // must return the already sealed record instead of allocating
                // a second stream sequence. This keeps retry/recovery
                // idempotent without treating a different payload with the
                // same biological identity as a duplicate.
                if let Some(existing) = stream.pending.iter().find(|record| {
                    record.destination_shard == destination_shard
                        && record.plan_digest == plan_digest
                        && record.lease_term == lease_term
                        && record.fencing_token == fencing_token
                        && record.topology_generation == topology_generation
                        && record.partition_generation == partition_generation
                        && record.placement_plan_digest == placement_plan_digest
                        && record.logical_tag == logical_tag
                        && record.event_id == event_id
                        && record.message == message
                }) {
                    records.push(existing.clone());
                    continue;
                }
                if stream.pending.len() >= max_pending_per_destination {
                    return Err(StableOutboundError::QueueFull {
                        destination: destination_node,
                        capacity: max_pending_per_destination,
                    });
                }
                let sequence = stream.next_sequence;
                let mut record = StableOutboundRecord {
                    sequence,
                    brain_id,
                    destination_node,
                    destination_shard,
                    plan_digest,
                    lease_term,
                    fencing_token,
                    logical_tag,
                    event_id,
                    topology_generation,
                    partition_generation,
                    placement_plan_digest,
                    message,
                    record_digest: StateDigest([0; 16]),
                };
                record.record_digest = record_digest(&record)?;
                let bytes = serde_json::to_vec(&record)
                    .map_err(|error| StableOutboundError::Corrupt(error.to_string()))?;
                if bytes.len() > MAX_RECORD_BYTES {
                    return Err(StableOutboundError::RecordTooLarge { bytes: bytes.len() });
                }
                stream.next_sequence = sequence
                    .checked_add(1)
                    .ok_or(StableOutboundError::SequenceOverflow)?;
                stream.pending.push(record.clone());
                stream.authority_term = lease_term;
                stream.authority_fencing_token = fencing_token;
                records.push(record);
            }
            *streams = staged;
            Ok(records)
        })
    }
    /// Acknowledge a sealed prefix only when the acknowledgement is fenced by
    /// the stream's current authority and names the exact final record.
    pub fn acknowledge(
        &mut self,
        acknowledgement: StableOutboundAcknowledgement,
    ) -> Result<(), StableOutboundError> {
        validate_destination(&acknowledgement.destination_node)?;
        self.with_locked_update(|streams| {
            let stream = streams.get_mut(&acknowledgement.destination_node).ok_or(
                StableOutboundError::UnknownSequence(acknowledgement.sequence),
            )?;
            if stream.authority_term != acknowledgement.lease_term
                || stream.authority_fencing_token != acknowledgement.fencing_token
            {
                return Err(StableOutboundError::StaleAuthority {
                    expected_term: stream.authority_term,
                    expected_fencing_token: stream.authority_fencing_token,
                    received_term: acknowledgement.lease_term,
                    received_fencing_token: acknowledgement.fencing_token,
                });
            }
            if stream
                .acknowledged_through
                .is_some_and(|sequence| acknowledgement.sequence <= sequence)
            {
                if stream.acknowledged_through == Some(acknowledgement.sequence)
                    && stream.acknowledged_digest == Some(acknowledgement.record_digest)
                {
                    return Ok(());
                }
                return Err(StableOutboundError::AcknowledgementMismatch);
            }
            let index = stream
                .pending
                .iter()
                .position(|record| record.sequence == acknowledgement.sequence)
                .ok_or(StableOutboundError::UnknownSequence(
                    acknowledgement.sequence,
                ))?;
            if stream.pending[index].record_digest != acknowledgement.record_digest {
                return Err(StableOutboundError::AcknowledgementMismatch);
            }
            stream.pending.drain(..=index);
            stream.acknowledged_through = Some(acknowledgement.sequence);
            stream.acknowledged_digest = Some(acknowledgement.record_digest);
            Ok(())
        })
    }

    fn with_locked_read<T>(
        &mut self,
        read: impl FnOnce(&BTreeMap<String, StableOutboundStream>) -> Result<T, StableOutboundError>,
    ) -> Result<T, StableOutboundError> {
        let lock = open_lock(&self.lock_path)?;
        lock.lock_exclusive()
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            self.reload_unlocked()?;
            read(&self.streams)
        })();
        let unlock = lock
            .unlock()
            .map_err(|error| DurabilityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn with_locked_update<T>(
        &mut self,
        update: impl FnOnce(
            &mut BTreeMap<String, StableOutboundStream>,
        ) -> Result<T, StableOutboundError>,
    ) -> Result<T, StableOutboundError> {
        let lock = open_lock(&self.lock_path)?;
        lock.lock_exclusive()
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            self.reload_unlocked()?;
            let value = update(&mut self.streams)?;
            self.persist_unlocked()?;
            Ok(value)
        })();
        let unlock = lock
            .unlock()
            .map_err(|error| DurabilityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn reload_unlocked(&mut self) -> Result<(), StableOutboundError> {
        if !self.path.exists() {
            self.streams.clear();
            return Ok(());
        }
        let bytes =
            std::fs::read(&self.path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let document: StableOutboundDocument = serde_json::from_slice(&bytes)
            .map_err(|error| StableOutboundError::Corrupt(error.to_string()))?;
        if document.schema_version != STABLE_OUTBOUND_SCHEMA_VERSION
            || document.brain_id != self.brain_id
        {
            return Err(StableOutboundError::Corrupt(
                "outbound schema or brain identity mismatch".to_owned(),
            ));
        }
        if document.digest != streams_digest(&document.streams)? {
            return Err(StableOutboundError::Corrupt(
                "outbound log digest verification failed".to_owned(),
            ));
        }
        for (destination, stream) in &document.streams {
            validate_destination(destination)?;
            if stream.pending.len() > self.max_pending_per_destination {
                return Err(StableOutboundError::QueueFull {
                    destination: destination.clone(),
                    capacity: self.max_pending_per_destination,
                });
            }
            for (offset, record) in stream.pending.iter().enumerate() {
                let expected = stream
                    .next_sequence
                    .checked_sub(stream.pending.len() as u64)
                    .ok_or_else(|| {
                        StableOutboundError::Corrupt("invalid sequence frontier".to_owned())
                    })?
                    .checked_add(offset as u64)
                    .ok_or(StableOutboundError::SequenceOverflow)?;
                if record.sequence != expected
                    || record.brain_id != self.brain_id
                    || record.destination_node != *destination
                    || record.record_digest != record_digest(record)?
                {
                    return Err(StableOutboundError::Corrupt(
                        "outbound record sequence, identity or digest is invalid".to_owned(),
                    ));
                }
                let bytes = serde_json::to_vec(record)
                    .map_err(|error| StableOutboundError::Corrupt(error.to_string()))?;
                if bytes.len() > MAX_RECORD_BYTES {
                    return Err(StableOutboundError::RecordTooLarge { bytes: bytes.len() });
                }
            }
        }
        self.streams = document.streams;
        Ok(())
    }

    fn persist_unlocked(&self) -> Result<(), StableOutboundError> {
        let document = StableOutboundDocument {
            schema_version: STABLE_OUTBOUND_SCHEMA_VERSION,
            brain_id: self.brain_id,
            streams: self.streams.clone(),
            digest: streams_digest(&self.streams)?,
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| StableOutboundError::Corrupt(error.to_string()))?;
        atomic_replace_and_sync(&self.path, &bytes)?;
        Ok(())
    }
}

fn open_lock(path: &Path) -> Result<std::fs::File, StableOutboundError> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .map_err(|error| DurabilityError::Io(error.to_string()).into())
}

fn validate_destination(destination: &str) -> Result<(), StableOutboundError> {
    if destination.trim().is_empty()
        || destination.len() > MAX_DESTINATION_ID_BYTES
        || destination.contains(['/', '\\', '\0'])
    {
        return Err(StableOutboundError::InvalidDestination);
    }
    Ok(())
}

fn validate_authority(
    stream: &StableOutboundStream,
    lease_term: LeaseTerm,
    fencing_token: u64,
) -> Result<(), StableOutboundError> {
    if (lease_term, fencing_token) < (stream.authority_term, stream.authority_fencing_token) {
        return Err(StableOutboundError::StaleAuthority {
            expected_term: stream.authority_term,
            expected_fencing_token: stream.authority_fencing_token,
            received_term: lease_term,
            received_fencing_token: fencing_token,
        });
    }
    Ok(())
}

fn message_metadata(message: &PartialShardOutbound) -> (StateDigest, ShardId, LogicalTag, EventId) {
    match message {
        PartialShardOutbound::CausalEvent {
            plan_digest,
            destination_shard,
            event,
        } => (
            *plan_digest,
            *destination_shard,
            event.event.key.tag,
            event.event.id,
        ),
        PartialShardOutbound::SynapseEffect {
            plan_digest,
            destination_shard,
            event_id,
            logical_tag,
            ..
        } => (*plan_digest, *destination_shard, *logical_tag, *event_id),
        PartialShardOutbound::SynapseActivation {
            plan_digest,
            destination_shard,
            parent_event,
            child_tag,
            ..
        } => (*plan_digest, *destination_shard, *child_tag, *parent_event),
    }
}

fn record_digest(record: &StableOutboundRecord) -> Result<StateDigest, StableOutboundError> {
    let bytes = serde_json::to_vec(&(
        record.sequence,
        record.brain_id,
        &record.destination_node,
        record.destination_shard,
        record.plan_digest,
        record.lease_term,
        record.fencing_token,
        record.logical_tag,
        record.event_id,
        record.topology_generation,
        record.partition_generation,
        record.placement_plan_digest,
        &record.message,
    ))
    .map_err(|error| StableOutboundError::Corrupt(error.to_string()))?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("stable-outbound-record:v1", bytes);
    Ok(digest.finish())
}

fn streams_digest(
    streams: &BTreeMap<String, StableOutboundStream>,
) -> Result<StateDigest, StableOutboundError> {
    let bytes = serde_json::to_vec(streams)
        .map_err(|error| StableOutboundError::Corrupt(error.to_string()))?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("stable-outbound-log:v1", bytes);
    Ok(digest.finish())
}
