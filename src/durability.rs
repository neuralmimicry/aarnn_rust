//! Immutable checkpoint and fenced causal-log reference storage.

use crate::causal::CausalEvent;
use crate::data_plane::{CausalEnvelope, DataPlaneError, ReceiveResult, ReliableReceiver};
use crate::deterministic::{
    CanonicalEventKey, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId, PartitionGeneration,
    RouteId, SchemaVersion, ShardId, StateDigest, StateDigestBuilder, StreamId, TopologyGeneration,
};
use crate::peripheral::PeripheralCursorState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalRecord {
    pub sequence: u64,
    pub lease_term: LeaseTerm,
    pub tag: LogicalTag,
    pub event: EventId,
    pub payload: Vec<u8>,
    /// Replay provenance retained for records written through the authoritative
    /// causal envelope path. Older WAL documents omit this field and remain
    /// valid for checkpoint restore, but they cannot be used for live
    /// post-checkpoint replay without an explicit migration adapter.
    #[serde(default)]
    pub replay: Option<WalReplayMetadata>,
    /// Opaque channel boundary committed with this event. Legacy records omit
    /// it; such records remain restorable but cannot prove a complete live
    /// catch-up boundary.
    #[serde(default)]
    pub channel_state: Option<Vec<u8>>,
    /// Digest of the preceding record.  The zero digest is the chain genesis.
    pub previous_digest: StateDigest,
    /// Digest of the canonical record fields and `previous_digest`.
    pub record_digest: StateDigest,
}

/// Causal envelope fields that are not represented by the compact WAL digest
/// fields but are required to reconstruct an admitted event exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalReplayMetadata {
    pub stream_id: StreamId,
    pub producer_sequence: u64,
    pub route: RouteId,
    pub partition_generation: PartitionGeneration,
    pub source: Option<NeuronId>,
    pub target: Option<NeuronId>,
    pub stage: EventStage,
    pub deferred_from_nonconvergence: bool,
}

const WAL_GENESIS_DIGEST: StateDigest = StateDigest([0; 16]);

pub(crate) fn reterm_records(
    records: &[WalRecord],
    term: LeaseTerm,
) -> Result<Vec<WalRecord>, DurabilityError> {
    let mut previous_digest = WAL_GENESIS_DIGEST;
    let mut retagged = Vec::with_capacity(records.len());
    for record in records {
        let mut next = record.clone();
        next.lease_term = term;
        next.previous_digest = previous_digest;
        next.record_digest = WalRecord::digest_for(
            next.sequence,
            next.lease_term,
            next.tag,
            next.event,
            &next.payload,
            next.previous_digest,
            next.replay.as_ref(),
            next.channel_state.as_deref(),
        );
        previous_digest = next.record_digest;
        retagged.push(next);
    }
    Ok(retagged)
}

impl WalRecord {
    fn digest_for(
        sequence: u64,
        lease_term: LeaseTerm,
        tag: LogicalTag,
        event: EventId,
        payload: &[u8],
        previous_digest: StateDigest,
        replay: Option<&WalReplayMetadata>,
        channel_state: Option<&[u8]>,
    ) -> StateDigest {
        let mut digest = StateDigestBuilder::default();
        let mut identity = Vec::with_capacity(8 * 5 + 4 + payload.len() + 16);
        identity.extend_from_slice(&sequence.to_be_bytes());
        identity.extend_from_slice(&lease_term.raw().to_be_bytes());
        identity.extend_from_slice(&tag.tick.to_be_bytes());
        identity.extend_from_slice(&tag.microstep.to_be_bytes());
        identity.extend_from_slice(&event.raw().to_be_bytes());
        identity.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        identity.extend_from_slice(payload);
        identity.extend_from_slice(&previous_digest.0);
        if let Some(replay) = replay {
            identity.extend_from_slice(&replay.stream_id.raw().to_be_bytes());
            identity.extend_from_slice(&replay.producer_sequence.to_be_bytes());
            identity.extend_from_slice(&replay.route.raw().to_be_bytes());
            identity.extend_from_slice(&replay.partition_generation.raw().to_be_bytes());
            identity
                .extend_from_slice(&replay.source.map(|id| id.raw()).unwrap_or(0).to_be_bytes());
            identity
                .extend_from_slice(&replay.target.map(|id| id.raw()).unwrap_or(0).to_be_bytes());
            identity.push(replay.stage as u8);
            identity.push(u8::from(replay.deferred_from_nonconvergence));
        }
        if let Some(channel_state) = channel_state {
            identity.extend_from_slice(&(channel_state.len() as u64).to_be_bytes());
            identity.extend_from_slice(channel_state);
        }
        digest.add_domain("wal-record:v1", identity);
        digest.finish()
    }

    fn is_integrity_valid(&self, previous_digest: StateDigest) -> bool {
        self.previous_digest == previous_digest
            && self.record_digest
                == Self::digest_for(
                    self.sequence,
                    self.lease_term,
                    self.tag,
                    self.event,
                    &self.payload,
                    self.previous_digest,
                    self.replay.as_ref(),
                    self.channel_state.as_deref(),
                )
    }

    /// Reconstruct the original causal envelope for catch-up. Records from
    /// legacy WALs intentionally fail closed because arrival order cannot
    /// safely supply the missing provenance.
    pub fn replay_envelope(
        &self,
        brain: crate::deterministic::BrainId,
        lease_term: LeaseTerm,
    ) -> Result<CausalEnvelope, DurabilityError> {
        let replay = self.replay.as_ref().ok_or_else(|| {
            DurabilityError::Corrupt(
                "WAL record lacks causal replay provenance; a new checkpoint is required"
                    .to_owned(),
            )
        })?;
        Ok(CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain,
            stream: replay.stream_id,
            sequence: replay.producer_sequence,
            lease_term,
            route: replay.route,
            partition_generation: replay.partition_generation,
            source: replay.source,
            target: replay.target,
            tag: self.tag,
            event: self.event,
            stage: replay.stage,
            kind: crate::data_plane::EnvelopeKind::Event,
            payload: self.payload.clone(),
            deferred_from_nonconvergence: replay.deferred_from_nonconvergence,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DurabilityError {
    #[error("stale lease term: expected {expected}, received {received}")]
    StaleTerm {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("checkpoint {0} is immutable and already published")]
    CheckpointAlreadyPublished(EventId),
    #[error("checkpoint {0} failed digest verification")]
    CheckpointDigestMismatch(EventId),
    #[error("shard checkpoint failed digest verification")]
    ShardCheckpointDigestMismatch,
    #[error("checkpoint {0} is not available")]
    MissingCheckpoint(EventId),
    #[error("replica sequence gap: expected {expected}, received {received}")]
    ReplicaSequenceGap { expected: u64, received: u64 },
    #[error("causal log sequence exhausted")]
    SequenceOverflow,
    #[error("durability I/O failed: {0}")]
    Io(String),
    #[error("durability record encoding failed: {0}")]
    Encoding(String),
    #[error("durability log is corrupt: {0}")]
    Corrupt(String),
    #[error("checkpoint payload exceeds the configured bound ({bytes} bytes)")]
    PayloadTooLarge { bytes: usize },
    #[error("durable shard transition failed: {0}")]
    Transition(String),
    #[error("persisted authority rejected the shard writer: {0}")]
    Authority(String),
    #[error(transparent)]
    DataPlane(#[from] DataPlaneError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalDocument {
    schema_version: SchemaVersion,
    active_term: LeaseTerm,
    next_sequence: u64,
    records: Vec<WalRecord>,
}

#[derive(Debug, Clone)]
pub struct CausalWal {
    active_term: LeaseTerm,
    next_sequence: u64,
    records: Vec<WalRecord>,
}

impl CausalWal {
    pub fn new(term: LeaseTerm) -> Self {
        Self {
            active_term: term,
            next_sequence: 0,
            records: Vec::new(),
        }
    }

    pub fn fence(&mut self, term: LeaseTerm) {
        if term > self.active_term {
            self.active_term = term;
        }
    }

    pub fn append(
        &mut self,
        term: LeaseTerm,
        event: &CausalEvent,
    ) -> Result<WalRecord, DurabilityError> {
        self.append_with_replay(term, event, None, None)
    }

    /// Append an event while retaining the envelope provenance required for a
    /// later post-checkpoint replay. The compact legacy path remains
    /// available for deterministic callers that do not have an envelope.
    pub fn append_envelope(
        &mut self,
        term: LeaseTerm,
        event: &CausalEvent,
        envelope: &CausalEnvelope,
        channel_state: &[u8],
    ) -> Result<WalRecord, DurabilityError> {
        if envelope.lease_term != term
            || envelope.event != event.id
            || envelope.tag != event.key.tag
            || envelope.payload != event.payload
        {
            return Err(DurabilityError::Corrupt(
                "causal envelope does not match the staged event".to_owned(),
            ));
        }
        self.append_with_replay(
            term,
            event,
            Some(WalReplayMetadata {
                stream_id: envelope.stream,
                producer_sequence: envelope.sequence,
                route: envelope.route,
                partition_generation: envelope.partition_generation,
                source: envelope.source,
                target: envelope.target,
                stage: envelope.stage,
                deferred_from_nonconvergence: envelope.deferred_from_nonconvergence,
            }),
            Some(channel_state),
        )
    }

    fn append_with_replay(
        &mut self,
        term: LeaseTerm,
        event: &CausalEvent,
        replay: Option<WalReplayMetadata>,
        channel_state: Option<&[u8]>,
    ) -> Result<WalRecord, DurabilityError> {
        self.require_term(term)?;
        let previous_digest = self
            .records
            .last()
            .map(|record| record.record_digest)
            .unwrap_or(WAL_GENESIS_DIGEST);
        let sequence = self.next_sequence;
        let record_digest = WalRecord::digest_for(
            sequence,
            term,
            event.key.tag,
            event.id,
            &event.payload,
            previous_digest,
            replay.as_ref(),
            channel_state,
        );
        let record = WalRecord {
            sequence,
            lease_term: term,
            tag: event.key.tag,
            event: event.id,
            payload: event.payload.clone(),
            replay,
            channel_state: channel_state.map(ToOwned::to_owned),
            previous_digest,
            record_digest,
        };
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(DurabilityError::SequenceOverflow)?;
        self.records.push(record.clone());
        Ok(record)
    }

    pub fn require_term(&self, term: LeaseTerm) -> Result<(), DurabilityError> {
        if term == self.active_term {
            Ok(())
        } else {
            Err(DurabilityError::StaleTerm {
                expected: self.active_term,
                received: term,
            })
        }
    }

    pub fn records_since(&self, sequence: u64) -> impl Iterator<Item = &WalRecord> {
        self.records
            .iter()
            .filter(move |record| record.sequence >= sequence)
    }

    /// Reconstruct a WAL from a checkpoint after validating the complete
    /// contiguous hash chain. Recovery never skips a damaged record.
    pub fn from_records(term: LeaseTerm, records: Vec<WalRecord>) -> Result<Self, DurabilityError> {
        let mut previous_digest = WAL_GENESIS_DIGEST;
        for (index, record) in records.iter().enumerate() {
            if record.sequence != index as u64
                || record.lease_term != term
                || !record.is_integrity_valid(previous_digest)
            {
                return Err(DurabilityError::Corrupt(format!(
                    "WAL record chain is invalid at sequence {}",
                    record.sequence
                )));
            }
            previous_digest = record.record_digest;
        }
        Ok(Self {
            active_term: term,
            next_sequence: records.len() as u64,
            records,
        })
    }

    pub fn records(&self) -> &[WalRecord] {
        &self.records
    }

    pub const fn last_sequence(&self) -> Option<u64> {
        if self.next_sequence == 0 {
            None
        } else {
            Some(self.next_sequence - 1)
        }
    }
}

/// Filesystem-backed WAL adapter with an atomic replace-and-sync append.
///
/// The in-memory [`CausalWal`] remains the semantic core.  This wrapper makes
/// its state reconstructible without allowing a partially written document to
/// become the current log.  It is deliberately single-writer; external lease
/// fencing must select that writer before opening the path.
#[derive(Debug, Clone)]
pub struct FileCausalWal {
    path: PathBuf,
    wal: CausalWal,
}

impl FileCausalWal {
    pub fn open(path: impl Into<PathBuf>, term: LeaseTerm) -> Result<Self, DurabilityError> {
        let path = path.into();
        if !path.exists() {
            return Ok(Self {
                path,
                wal: CausalWal::new(term),
            });
        }
        let bytes = fs::read(&path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let document: WalDocument = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        if document.schema_version != SchemaVersion::CURRENT {
            return Err(DurabilityError::Corrupt(format!(
                "unsupported WAL schema {}",
                document.schema_version
            )));
        }
        let mut previous_digest = WAL_GENESIS_DIGEST;
        if document.records.iter().enumerate().any(|(index, record)| {
            let valid = record.sequence == index as u64
                && record.lease_term.raw() != 0
                && record.is_integrity_valid(previous_digest);
            previous_digest = record.record_digest;
            !valid
        }) {
            return Err(DurabilityError::Corrupt(
                "WAL records are not a contiguous, integrity-verified chain".to_owned(),
            ));
        }
        if document.next_sequence != document.records.len() as u64 {
            return Err(DurabilityError::Corrupt(
                "WAL next sequence does not follow its records".to_owned(),
            ));
        }
        let loaded_term = document.active_term;
        if term < loaded_term {
            return Err(DurabilityError::StaleTerm {
                expected: loaded_term,
                received: term,
            });
        }
        let mut wal = CausalWal {
            active_term: loaded_term,
            next_sequence: document.next_sequence,
            records: document.records,
        };
        wal.fence(term);
        let instance = Self { path, wal };
        if term > loaded_term {
            instance.persist(&instance.wal)?;
        }
        Ok(instance)
    }

    pub fn append(
        &mut self,
        term: LeaseTerm,
        event: &CausalEvent,
    ) -> Result<WalRecord, DurabilityError> {
        let mut candidate = self.wal.clone();
        let record = candidate.append(term, event)?;
        self.persist(&candidate)?;
        self.wal = candidate;
        Ok(record)
    }

    pub fn fence(&mut self, term: LeaseTerm) -> Result<(), DurabilityError> {
        if term > self.wal.active_term {
            let mut candidate = self.wal.clone();
            candidate.fence(term);
            self.persist(&candidate)?;
            self.wal = candidate;
        }
        Ok(())
    }

    pub fn records_since(&self, sequence: u64) -> impl Iterator<Item = &WalRecord> {
        self.wal.records_since(sequence)
    }

    pub const fn last_sequence(&self) -> Option<u64> {
        self.wal.last_sequence()
    }

    fn persist(&self, wal: &CausalWal) -> Result<(), DurabilityError> {
        let document = WalDocument {
            schema_version: SchemaVersion::CURRENT,
            active_term: wal.active_term,
            next_sequence: wal.next_sequence,
            records: wal.records.clone(),
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        atomic_replace_and_sync(&self.path, &bytes)
    }
}

pub(crate) fn atomic_replace_and_sync(path: &Path, bytes: &[u8]) -> Result<(), DurabilityError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| DurabilityError::Io(error.to_string()))?;
    }
    let (temporary, mut file) = create_unique_temp(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(DurabilityError::Io(error.to_string()));
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(DurabilityError::Io(error.to_string()));
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
    }
    Ok(())
}

fn create_unique_temp(path: &Path) -> Result<(PathBuf, fs::File), DurabilityError> {
    for attempt in 0..32u32 {
        let temporary = path.with_extension(format!("tmp-{}-{attempt}", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(DurabilityError::Io(error.to_string())),
        }
    }
    Err(DurabilityError::Io(
        "unable to allocate a unique temporary durability path".to_owned(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointManifest {
    pub schema_version: SchemaVersion,
    pub checkpoint_id: EventId,
    pub lease_term: LeaseTerm,
    pub partition_generation: PartitionGeneration,
    pub last_wal_sequence: Option<u64>,
    pub state_digest: StateDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImmutableCheckpoint {
    pub manifest: CheckpointManifest,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct CheckpointStore {
    published: BTreeMap<EventId, ImmutableCheckpoint>,
}

impl CheckpointStore {
    pub fn publish(
        &mut self,
        checkpoint_id: EventId,
        lease_term: LeaseTerm,
        partition_generation: PartitionGeneration,
        last_wal_sequence: Option<u64>,
        payload: Vec<u8>,
    ) -> Result<CheckpointManifest, DurabilityError> {
        if self.published.contains_key(&checkpoint_id) {
            return Err(DurabilityError::CheckpointAlreadyPublished(checkpoint_id));
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("checkpoint-payload", &payload);
        let manifest = CheckpointManifest {
            schema_version: SchemaVersion::CURRENT,
            checkpoint_id,
            lease_term,
            partition_generation,
            last_wal_sequence,
            state_digest: digest.finish(),
        };
        self.published.insert(
            checkpoint_id,
            ImmutableCheckpoint {
                manifest: manifest.clone(),
                payload,
            },
        );
        Ok(manifest)
    }

    pub fn verify(&self, checkpoint_id: EventId) -> Result<&ImmutableCheckpoint, DurabilityError> {
        let checkpoint = self
            .published
            .get(&checkpoint_id)
            .ok_or(DurabilityError::MissingCheckpoint(checkpoint_id))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("checkpoint-payload", &checkpoint.payload);
        if digest.finish() != checkpoint.manifest.state_digest {
            return Err(DurabilityError::CheckpointDigestMismatch(checkpoint_id));
        }
        Ok(checkpoint)
    }

    pub fn publish_shard(
        &mut self,
        checkpoint_id: EventId,
        payload: ShardCheckpointPayload,
    ) -> Result<CheckpointManifest, DurabilityError> {
        let payload = payload.seal()?;
        let lease_term = payload.lease_term;
        let partition_generation = payload.partition_generation;
        let last_wal_sequence = payload.durable_wal_sequence;
        let bytes = serde_json::to_vec(&payload)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        self.publish(
            checkpoint_id,
            lease_term,
            partition_generation,
            last_wal_sequence,
            bytes,
        )
    }

    pub fn verify_shard(
        &self,
        checkpoint_id: EventId,
    ) -> Result<ShardCheckpointPayload, DurabilityError> {
        let checkpoint = self.verify(checkpoint_id)?;
        let payload: ShardCheckpointPayload = serde_json::from_slice(&checkpoint.payload)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        payload.verify()?;
        if payload.lease_term != checkpoint.manifest.lease_term
            || payload.partition_generation != checkpoint.manifest.partition_generation
            || payload.durable_wal_sequence != checkpoint.manifest.last_wal_sequence
        {
            return Err(DurabilityError::Corrupt(
                "shard checkpoint payload does not match its manifest".to_owned(),
            ));
        }
        Ok(payload)
    }
}

/// Immutable filesystem checkpoint publication.
///
/// The checkpoint file is linked into place only after its temporary contents
/// have been flushed.  A partial temporary object therefore remains
/// undiscoverable, and an existing checkpoint can never be replaced.
#[derive(Debug, Clone)]
pub struct FileCheckpointStore {
    root: PathBuf,
}

impl FileCheckpointStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DurabilityError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|error| DurabilityError::Io(error.to_string()))?;
        Ok(Self { root })
    }

    pub fn publish(
        &self,
        checkpoint_id: EventId,
        lease_term: LeaseTerm,
        partition_generation: PartitionGeneration,
        last_wal_sequence: Option<u64>,
        payload: Vec<u8>,
    ) -> Result<CheckpointManifest, DurabilityError> {
        let path = self.path_for(checkpoint_id);
        if path.exists() {
            return Err(DurabilityError::CheckpointAlreadyPublished(checkpoint_id));
        }
        let mut memory = CheckpointStore::default();
        let manifest = memory.publish(
            checkpoint_id,
            lease_term,
            partition_generation,
            last_wal_sequence,
            payload.clone(),
        )?;
        let checkpoint = ImmutableCheckpoint {
            manifest: manifest.clone(),
            payload,
        };
        let bytes = serde_json::to_vec(&checkpoint)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        atomic_publish_no_replace(&path, &bytes, checkpoint_id)?;
        Ok(manifest)
    }

    pub fn verify(&self, checkpoint_id: EventId) -> Result<ImmutableCheckpoint, DurabilityError> {
        let path = self.path_for(checkpoint_id);
        let bytes = fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DurabilityError::MissingCheckpoint(checkpoint_id)
            } else {
                DurabilityError::Io(error.to_string())
            }
        })?;
        let checkpoint: ImmutableCheckpoint = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        if checkpoint.manifest.checkpoint_id != checkpoint_id {
            return Err(DurabilityError::CheckpointDigestMismatch(checkpoint_id));
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("checkpoint-payload", &checkpoint.payload);
        if digest.finish() != checkpoint.manifest.state_digest {
            return Err(DurabilityError::CheckpointDigestMismatch(checkpoint_id));
        }
        Ok(checkpoint)
    }

    pub fn publish_shard(
        &self,
        checkpoint_id: EventId,
        payload: ShardCheckpointPayload,
    ) -> Result<CheckpointManifest, DurabilityError> {
        let payload = payload.seal()?;
        let lease_term = payload.lease_term;
        let partition_generation = payload.partition_generation;
        let last_wal_sequence = payload.durable_wal_sequence;
        let bytes = serde_json::to_vec(&payload)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        self.publish(
            checkpoint_id,
            lease_term,
            partition_generation,
            last_wal_sequence,
            bytes,
        )
    }

    pub fn verify_shard(
        &self,
        checkpoint_id: EventId,
    ) -> Result<ShardCheckpointPayload, DurabilityError> {
        let checkpoint = self.verify(checkpoint_id)?;
        let payload: ShardCheckpointPayload = serde_json::from_slice(&checkpoint.payload)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        payload.verify()?;
        if payload.lease_term != checkpoint.manifest.lease_term
            || payload.partition_generation != checkpoint.manifest.partition_generation
            || payload.durable_wal_sequence != checkpoint.manifest.last_wal_sequence
        {
            return Err(DurabilityError::Corrupt(
                "shard checkpoint payload does not match its manifest".to_owned(),
            ));
        }
        Ok(payload)
    }

    fn path_for(&self, checkpoint_id: EventId) -> PathBuf {
        self.root
            .join(format!("checkpoint-{}.json", checkpoint_id.raw()))
    }
}

fn atomic_publish_no_replace(
    path: &Path,
    bytes: &[u8],
    checkpoint_id: EventId,
) -> Result<(), DurabilityError> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file =
        fs::File::create(&temporary).map_err(|error| DurabilityError::Io(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| DurabilityError::Io(error.to_string()))?;
    let result = fs::hard_link(&temporary, path);
    let _ = fs::remove_file(&temporary);
    if let Err(error) = result {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(DurabilityError::CheckpointAlreadyPublished(checkpoint_id));
        }
        return Err(DurabilityError::Io(error.to_string()));
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct WarmReplica {
    next_sequence: u64,
    term: LeaseTerm,
    records: Vec<WalRecord>,
}

impl WarmReplica {
    pub fn new(term: LeaseTerm) -> Self {
        Self {
            next_sequence: 0,
            term,
            records: Vec::new(),
        }
    }

    pub fn apply(&mut self, record: WalRecord) -> Result<(), DurabilityError> {
        if record.lease_term != self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: record.lease_term,
            });
        }
        if record.sequence < self.next_sequence {
            let existing = self.records.get(record.sequence as usize).ok_or_else(|| {
                DurabilityError::Corrupt("replica sequence index missing".to_owned())
            })?;
            if existing == &record {
                // At-least-once transport may retransmit an already durable
                // record. Exact duplicates are successful no-ops.
                return Ok(());
            }
            return Err(DurabilityError::Corrupt(format!(
                "conflicting duplicate WAL record at sequence {}",
                record.sequence
            )));
        }
        if record.sequence != self.next_sequence {
            return Err(DurabilityError::ReplicaSequenceGap {
                expected: self.next_sequence,
                received: record.sequence,
            });
        }
        let previous_digest = self
            .records
            .last()
            .map(|item| item.record_digest)
            .unwrap_or(WAL_GENESIS_DIGEST);
        if !record.is_integrity_valid(previous_digest) {
            return Err(DurabilityError::Corrupt(format!(
                "invalid WAL record integrity at sequence {}",
                record.sequence
            )));
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(DurabilityError::SequenceOverflow)?;
        self.records.push(record);
        Ok(())
    }

    pub const fn applied(&self) -> usize {
        self.records.len()
    }

    pub fn from_records(
        term: LeaseTerm,
        records: impl IntoIterator<Item = WalRecord>,
    ) -> Result<Self, DurabilityError> {
        let mut replica = Self::new(term);
        for record in records {
            replica.apply(record)?;
        }
        Ok(replica)
    }

    pub const fn durable_sequence(&self) -> Option<u64> {
        if self.next_sequence == 0 {
            None
        } else {
            Some(self.next_sequence - 1)
        }
    }
}

/// A process-safe warm WAL replica.
///
/// The active shard still owns the biological state.  This adapter owns only
/// the replicated WAL prefix and acknowledges a record after the complete
/// prefix has been atomically replaced, synced and unlocked.  A lock file is
/// used because the replica may be shared by independent worker processes;
/// the record file itself is never used as an advisory lock during rename.
#[derive(Debug, Clone)]
pub struct FileWarmReplica {
    path: PathBuf,
    lock_path: PathBuf,
    term: LeaseTerm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WarmReplicaDocument {
    schema_version: SchemaVersion,
    term: LeaseTerm,
    records: Vec<WalRecord>,
    /// The last complete shard boundary acknowledged by the active owner.
    /// Older warm documents did not carry this field; those documents remain
    /// readable and are upgraded when the active owner supplies a checkpoint.
    #[serde(default)]
    checkpoint: Option<ShardCheckpointPayload>,
    /// A two-phase publication slot.  It lets a restarted process distinguish
    /// a warm state prepared before an active publication from a state that
    /// was acknowledged by both sides.
    #[serde(default)]
    pending_checkpoint: Option<ShardCheckpointPayload>,
}

impl FileWarmReplica {
    pub fn open(
        path: impl Into<PathBuf>,
        term: LeaseTerm,
        seed: impl IntoIterator<Item = WalRecord>,
    ) -> Result<Self, DurabilityError> {
        Self::open_with_checkpoint(path, term, seed, None)
    }

    /// Open a warm replica and optionally reconcile it with the active
    /// owner's current checkpoint.  A pending checkpoint is committed only
    /// when the active owner has the same WAL prefix; if the active owner is
    /// still at the prior prefix, the pending preparation is discarded.
    pub fn open_with_checkpoint(
        path: impl Into<PathBuf>,
        term: LeaseTerm,
        seed: impl IntoIterator<Item = WalRecord>,
        checkpoint: Option<ShardCheckpointPayload>,
    ) -> Result<Self, DurabilityError> {
        let path = path.into();
        let lock_path = path.with_extension("lock");
        let replica = Self {
            path,
            lock_path,
            term,
        };
        let mut seed = seed.into_iter().collect::<Vec<_>>();
        // A caller that supplies a complete active checkpoint but omits its
        // WAL seed is still giving us an authoritative prefix.  Recover the
        // seed from that checkpoint so a crash after a warm-record fsync is
        // repaired against the active boundary rather than mistaken for
        // corruption.  With neither value present, preserve the warm state
        // for a replacement owner to inspect.
        if seed.is_empty() {
            if let Some(checkpoint) = checkpoint.as_ref() {
                checkpoint.verify()?;
                seed = checkpoint_records(checkpoint)?;
            }
        }
        replica.with_lock(|replica| {
            let mut document = replica.read_document()?;
            let mut records = document.records.clone();
            if document.term != replica.term {
                return Err(DurabilityError::StaleTerm {
                    expected: document.term,
                    received: replica.term,
                });
            }
            let _ = WarmReplica::from_records(replica.term, records.clone())?;

            if let Some(pending) = document.pending_checkpoint.take() {
                pending.verify()?;
                let pending_records = checkpoint_records(&pending)?;
                if seed == pending_records {
                    document.checkpoint = Some(pending);
                } else if seed.is_empty() {
                    // There is no active owner to compare against yet. Keep
                    // the prepared state visible to a recovery process; it
                    // must not be guessed to be committed or discarded.
                    document.pending_checkpoint = Some(pending);
                } else if is_prefix(&seed, &pending_records) {
                    // The active publication did not complete.  The pending
                    // slot is deliberately not treated as acknowledged. The
                    // WAL preparation is rolled back to the active prefix as
                    // part of the same recovery decision.
                    document.records = seed.clone();
                    records = seed.clone();
                } else {
                    return Err(DurabilityError::Corrupt(
                        "warm pending checkpoint does not match the active prefix".to_owned(),
                    ));
                }
            }

            if let Some(existing) = document.checkpoint.as_ref() {
                existing.verify()?;
                let existing_records = checkpoint_records(existing)?;
                if !is_prefix(&existing_records, &records)
                    && !is_prefix(&records, &existing_records)
                {
                    return Err(DurabilityError::Corrupt(
                        "warm checkpoint and WAL records diverge".to_owned(),
                    ));
                }
            }
            if !records.is_empty() && (!seed.is_empty() || checkpoint.is_some()) {
                let existing = WarmReplica::from_records(replica.term, records.clone())?;
                if existing.applied() > seed.len() {
                    if !is_prefix(&seed, &records) {
                        return Err(DurabilityError::Corrupt(
                            "warm replica is ahead of the active checkpoint but diverges from its prefix"
                                .to_owned(),
                        ));
                    }
                    // A process can crash after the warm WAL record is
                    // synced but before the active checkpoint publication
                    // begins.  The active checkpoint is authoritative in
                    // that case: discard only the uncommitted suffix and
                    // retain the acknowledged prefix.  Treating this as
                    // corruption would turn an ordinary two-phase crash
                    // window into an unrecoverable shard.
                    records = seed.clone();
                    document.records = records.clone();
                    let checkpoint_is_ahead = document
                        .checkpoint
                        .as_ref()
                        .map(checkpoint_records)
                        .transpose()?
                        .is_some_and(|checkpoint| checkpoint.len() > seed.len());
                    if checkpoint_is_ahead {
                        document.checkpoint = None;
                    }
                }
                if existing
                    .records
                    .iter()
                    .zip(seed.iter())
                    .any(|(left, right)| left != right)
                {
                    return Err(DurabilityError::Corrupt(
                        "warm replica prefix diverges from active checkpoint".to_owned(),
                    ));
                }
            }
            if records.len() < seed.len() {
                let mut warm = WarmReplica::from_records(replica.term, records)?;
                for record in &seed {
                    warm.apply(record.clone())?;
                }
                records = warm.records;
                document.records = records.clone();
            }
            if let Some(checkpoint) = checkpoint {
                checkpoint.verify()?;
                if checkpoint.lease_term != replica.term {
                    return Err(DurabilityError::StaleTerm {
                        expected: replica.term,
                        received: checkpoint.lease_term,
                    });
                }
                let checkpoint_records = checkpoint_records(&checkpoint)?;
                if checkpoint_records != seed {
                    return Err(DurabilityError::Corrupt(
                        "active checkpoint does not match the supplied WAL seed".to_owned(),
                    ));
                }
                document.checkpoint = Some(checkpoint);
            }
            document.records = records;
            replica.write_document(&document)
        })?;
        Ok(replica)
    }

    /// Replicate one record and return only after the warm process-visible
    /// prefix contains it. Exact retransmission is idempotent; conflicting or
    /// skipped records fail closed.
    pub fn apply(&mut self, record: WalRecord) -> Result<(), DurabilityError> {
        self.with_lock(|replica| {
            let document = replica.read_document()?;
            let records = document.records;
            let mut warm = WarmReplica::from_records(replica.term, records)?;
            warm.apply(record)?;
            let mut document = replica.read_document()?;
            document.records = warm.records;
            replica.write_document(&document)
        })
    }

    /// Prepare a complete biological/channel checkpoint before the active
    /// owner publishes it.  This is intentionally separate from `apply` so a
    /// crash between the two files is recoverable rather than ambiguous.
    pub fn prepare_checkpoint(
        &mut self,
        checkpoint: &ShardCheckpointPayload,
    ) -> Result<(), DurabilityError> {
        checkpoint.verify()?;
        if checkpoint.lease_term != self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: checkpoint.lease_term,
            });
        }
        let expected = checkpoint_records(checkpoint)?;
        self.with_lock(|replica| {
            let mut document = replica.read_document()?;
            if !is_prefix(&document.records, &expected) {
                return Err(DurabilityError::ReplicaSequenceGap {
                    expected: document.records.len() as u64,
                    received: expected.len() as u64,
                });
            }
            document.pending_checkpoint = Some(checkpoint.clone());
            replica.write_document(&document)
        })
    }

    /// Complete the prepared checkpoint after the active owner has published
    /// the same boundary.  The sequence is checked so callers cannot finalize
    /// a different state after a retry or a stale failover.
    pub fn finalize_checkpoint(&mut self, sequence: Option<u64>) -> Result<(), DurabilityError> {
        self.with_lock(|replica| {
            let mut document = replica.read_document()?;
            let Some(pending) = document.pending_checkpoint.take() else {
                return Err(DurabilityError::Corrupt(
                    "warm checkpoint finalization has no prepared state".to_owned(),
                ));
            };
            if pending.durable_wal_sequence != sequence {
                return Err(DurabilityError::Corrupt(
                    "warm checkpoint finalization sequence differs from the active boundary"
                        .to_owned(),
                ));
            }
            document.checkpoint = Some(pending);
            replica.write_document(&document)
        })
    }

    /// Remove an uncommitted preparation after the active transaction rolls
    /// back.  This is idempotent and leaves the last acknowledged checkpoint
    /// intact.
    pub fn rollback_pending(&mut self) -> Result<(), DurabilityError> {
        self.with_lock(|replica| {
            let mut document = replica.read_document()?;
            document.pending_checkpoint = None;
            replica.write_document(&document)
        })
    }

    /// Return the last complete biological/channel boundary available to a
    /// replacement process.  This is distinct from the WAL-only sequence.
    pub fn checkpoint(&self) -> Result<Option<ShardCheckpointPayload>, DurabilityError> {
        self.with_lock(|replica| {
            let document = replica.read_document()?;
            if let Some(checkpoint) = document.checkpoint.as_ref() {
                checkpoint.verify()?;
            }
            Ok(document.checkpoint)
        })
    }

    /// Return a complete state candidate for owner recovery.  A pending
    /// checkpoint is preferred because the active file may have disappeared
    /// after the warm prepare and before its final publication.
    pub fn recovery_checkpoint(&self) -> Result<Option<ShardCheckpointPayload>, DurabilityError> {
        self.with_lock(|replica| {
            let document = replica.read_document()?;
            let checkpoint = document
                .pending_checkpoint
                .as_ref()
                .or(document.checkpoint.as_ref());
            if let Some(checkpoint) = checkpoint {
                checkpoint.verify()?;
                return Ok(Some(checkpoint.clone()));
            }
            Ok(None)
        })
    }

    pub fn durable_sequence(&self) -> Result<Option<u64>, DurabilityError> {
        self.with_lock(|replica| {
            let records = replica.read_records()?;
            Ok(WarmReplica::from_records(replica.term, records)?.durable_sequence())
        })
    }

    pub fn records(&self) -> Result<Vec<WalRecord>, DurabilityError> {
        self.with_lock(|replica| replica.read_records())
    }

    /// Validate that the process-shared document is readable at this
    /// instance's authority term without changing it.
    pub fn validate_term(&self, term: LeaseTerm) -> Result<(), DurabilityError> {
        if term != self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: term,
            });
        }
        self.with_lock(|replica| {
            let _ = replica.read_records()?;
            Ok(())
        })
    }

    pub fn persisted_term(path: &Path) -> Result<Option<LeaseTerm>, DurabilityError> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let document: WarmReplicaDocument = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        if document.schema_version != SchemaVersion::CURRENT {
            return Err(DurabilityError::Corrupt(
                "unsupported warm replica schema".to_owned(),
            ));
        }
        WarmReplica::from_records(document.term, document.records)?;
        Ok(Some(document.term))
    }

    /// Re-sign an already validated warm prefix after quorum promotion.
    /// Biological bytes remain in the checkpoint owner; only authority-bound
    /// WAL/receipt terms are changed.
    pub fn reissue_term(&mut self, term: LeaseTerm) -> Result<(), DurabilityError> {
        if term <= self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: term,
            });
        }
        self.with_lock(|replica| {
            let mut document = replica.read_document()?;
            let current = document.records.clone();
            let records = reterm_records(&current, term)?;
            let checkpoint = document
                .checkpoint
                .take()
                .map(|checkpoint| reterm_checkpoint(checkpoint, term))
                .transpose()?;
            let pending_checkpoint = document
                .pending_checkpoint
                .take()
                .map(|checkpoint| reterm_checkpoint(checkpoint, term))
                .transpose()?;
            document.term = term;
            document.records = records;
            document.checkpoint = checkpoint;
            document.pending_checkpoint = pending_checkpoint;
            replica.write_records_with_document(&document)
        })?;
        self.term = term;
        Ok(())
    }

    /// Roll a replica back to the last active sequence after a two-file
    /// commit fails. This is only used by the owning shard transaction and is
    /// guarded by the same process-shared lock as replication.
    pub fn truncate_to(&mut self, sequence: Option<u64>) -> Result<(), DurabilityError> {
        self.with_lock(|replica| {
            let mut document = replica.read_document()?;
            let records = document.records;
            let keep = sequence.map_or(0usize, |value| {
                usize::try_from(value.saturating_add(1)).unwrap_or(usize::MAX)
            });
            if keep > records.len() {
                return Err(DurabilityError::ReplicaSequenceGap {
                    expected: records.len() as u64,
                    received: keep as u64,
                });
            }
            document.records = records[..keep].to_vec();
            if document
                .pending_checkpoint
                .as_ref()
                .is_some_and(|checkpoint| checkpoint.durable_wal_sequence != sequence)
            {
                document.pending_checkpoint = None;
            }
            replica.write_document(&document)
        })
    }

    fn read_records(&self) -> Result<Vec<WalRecord>, DurabilityError> {
        Ok(self.read_document()?.records)
    }

    fn read_document(&self) -> Result<WarmReplicaDocument, DurabilityError> {
        if !self.path.exists() {
            return Ok(WarmReplicaDocument {
                schema_version: SchemaVersion::CURRENT,
                term: self.term,
                records: Vec::new(),
                checkpoint: None,
                pending_checkpoint: None,
            });
        }
        let bytes = fs::read(&self.path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let document: WarmReplicaDocument = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        if document.schema_version != SchemaVersion::CURRENT {
            return Err(DurabilityError::Corrupt(
                "unsupported warm replica schema".to_owned(),
            ));
        }
        if document.term != self.term {
            return Err(DurabilityError::StaleTerm {
                expected: document.term,
                received: self.term,
            });
        }
        Ok(document)
    }

    fn write_document(&self, document: &WarmReplicaDocument) -> Result<(), DurabilityError> {
        if document.term != self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: document.term,
            });
        }
        self.write_records_with_document(document)
    }

    fn write_records_with_document(
        &self,
        document: &WarmReplicaDocument,
    ) -> Result<(), DurabilityError> {
        let bytes = serde_json::to_vec(document)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        atomic_replace_and_sync(&self.path, &bytes)
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, DurabilityError>,
    ) -> Result<T, DurabilityError> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| DurabilityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = operation(self);
        let unlock =
            fs2::FileExt::unlock(&lock).map_err(|error| DurabilityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

fn checkpoint_records(
    checkpoint: &ShardCheckpointPayload,
) -> Result<Vec<WalRecord>, DurabilityError> {
    serde_json::from_slice(&checkpoint.causal_state)
        .map_err(|error| DurabilityError::Encoding(error.to_string()))
}

fn is_prefix<T: PartialEq>(prefix: &[T], full: &[T]) -> bool {
    prefix.len() <= full.len()
        && prefix
            .iter()
            .zip(full.iter())
            .all(|(left, right)| left == right)
}

fn reterm_checkpoint(
    checkpoint: ShardCheckpointPayload,
    term: LeaseTerm,
) -> Result<ShardCheckpointPayload, DurabilityError> {
    let records = checkpoint_records(&checkpoint)?;
    let records = reterm_records(&records, term)?;
    let causal_state = serde_json::to_vec(&records)
        .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
    let mut receipts = ReceiptLedger::default();
    for mut receipt in checkpoint.receipts.receipts().cloned() {
        receipt.lease_term = term;
        receipts.record(receipt)?;
    }
    let mut peripheral_state = checkpoint.peripheral_state;
    peripheral_state
        .reterm(term)
        .map_err(|error| DurabilityError::Corrupt(error.to_string()))?;
    ShardCheckpointPayload::new(
        checkpoint.brain_id,
        checkpoint.shard_id,
        checkpoint.topology_generation,
        checkpoint.partition_generation,
        term,
        checkpoint.committed_tag,
        checkpoint.applied_tag,
        checkpoint.durable_wal_sequence,
        checkpoint.biological_state,
        causal_state,
        checkpoint.channel_state,
        receipts,
    )
    .with_peripheral_state(peripheral_state)?
    .seal()
}

/// A durable acknowledgement for one accepted causal stream item.
///
/// Receipts are separate from applied state: a receiver can durably remember
/// an accepted event while its biological application is still queued.  The
/// stream/sequence and event identity are both checked so reconnect retries
/// cannot turn an at-least-once transport into duplicate application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableReceipt {
    pub stream_id: StreamId,
    pub sequence: u64,
    pub event_id: EventId,
    pub lease_term: LeaseTerm,
    pub partition_generation: PartitionGeneration,
    pub tag: LogicalTag,
    pub payload_digest: StateDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptOutcome {
    New,
    Duplicate,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiptLedger {
    receipts: BTreeMap<(StreamId, u64), DurableReceipt>,
}

// Use a sequence on the wire instead of exposing a tuple-keyed map. JSON
// object keys must be strings, and a canonical sequence also makes the schema
// explicit for non-JSON product implementations.
impl Serialize for ReceiptLedger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.receipts
            .values()
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReceiptLedger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let entries = Vec::<DurableReceipt>::deserialize(deserializer)?;
        let mut ledger = Self::default();
        for entry in entries {
            ledger.record(entry).map_err(serde::de::Error::custom)?;
        }
        Ok(ledger)
    }
}

impl ReceiptLedger {
    pub fn record(&mut self, receipt: DurableReceipt) -> Result<ReceiptOutcome, DurabilityError> {
        let key = (receipt.stream_id, receipt.sequence);
        if let Some(existing) = self.receipts.get(&key) {
            if existing == &receipt {
                return Ok(ReceiptOutcome::Duplicate);
            }
            return Err(DurabilityError::Corrupt(format!(
                "conflicting receipt for stream {} sequence {}",
                receipt.stream_id, receipt.sequence
            )));
        }
        if self
            .receipts
            .values()
            .any(|existing| existing.event_id == receipt.event_id)
        {
            return Err(DurabilityError::Corrupt(format!(
                "event {} is acknowledged at more than one stream position",
                receipt.event_id
            )));
        }
        self.receipts.insert(key, receipt);
        Ok(ReceiptOutcome::New)
    }

    pub fn record_event(
        &mut self,
        stream_id: StreamId,
        sequence: u64,
        event: &CausalEvent,
        lease_term: LeaseTerm,
        partition_generation: PartitionGeneration,
    ) -> Result<ReceiptOutcome, DurabilityError> {
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("causal-payload", &event.payload);
        self.record(DurableReceipt {
            stream_id,
            sequence,
            event_id: event.id,
            lease_term,
            partition_generation,
            tag: event.key.tag,
            payload_digest: digest.finish(),
        })
    }

    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    pub fn contains(&self, stream_id: StreamId, sequence: u64) -> bool {
        self.receipts.contains_key(&(stream_id, sequence))
    }

    pub fn digest(&self) -> Result<StateDigest, DurabilityError> {
        let bytes = serde_json::to_vec(&self.receipts)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("durable-receipts", bytes);
        Ok(digest.finish())
    }

    pub fn receipts(&self) -> impl Iterator<Item = &DurableReceipt> {
        self.receipts.values()
    }

    /// Return the durable receipt prefix for one producer stream in sequence
    /// order. This is used to reconstruct a receiver cursor after a process
    /// restart without treating the WAL's local append sequence as a network
    /// stream sequence.
    pub fn stream_receipts(&self, stream_id: StreamId) -> Vec<DurableReceipt> {
        self.receipts
            .values()
            .filter(|receipt| receipt.stream_id == stream_id)
            .cloned()
            .collect()
    }
}

/// Result of applying one causally ordered envelope at a shard's durable
/// commit boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableApplyOutcome {
    Applied { sequence: u64 },
    Duplicate { sequence: u64 },
}

/// The authoritative, single-writer durability boundary for one shard.
///
/// The biological representation is intentionally a versioned byte payload:
/// the biological owner supplies a pure staged transition and receives the
/// resulting bytes only after receiver validation, WAL append, synchronous
/// warm replication and receipt validation all succeed. This keeps storage
/// independent of a particular neuron implementation while making the
/// commit unit explicit and testable.
#[derive(Debug, Clone)]
pub struct DurableShard {
    brain_id: crate::deterministic::BrainId,
    shard_id: ShardId,
    topology_generation: TopologyGeneration,
    partition_generation: PartitionGeneration,
    lease_term: LeaseTerm,
    stream_id: StreamId,
    max_payload: usize,
    committed_tag: LogicalTag,
    applied_tag: LogicalTag,
    biological_state: Vec<u8>,
    channel_state: Vec<u8>,
    peripheral_state: PeripheralCursorState,
    /// One causal cursor per admitted producer stream. The WAL remains a
    /// single authoritative commit order, while sender sequence spaces stay
    /// independent and therefore cannot collide when multiple nodes forward
    /// the same shard concurrently.
    receivers: BTreeMap<StreamId, ReliableReceiver>,
    wal: CausalWal,
    warm_replica: WarmReplica,
    receipts: ReceiptLedger,
}

impl DurableShard {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        brain_id: crate::deterministic::BrainId,
        shard_id: ShardId,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        lease_term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
        biological_state: Vec<u8>,
        channel_state: Vec<u8>,
    ) -> Self {
        Self {
            brain_id,
            shard_id,
            topology_generation,
            partition_generation,
            lease_term,
            stream_id,
            max_payload,
            committed_tag: LogicalTag::ZERO,
            applied_tag: LogicalTag::ZERO,
            biological_state,
            channel_state,
            peripheral_state: PeripheralCursorState::empty(),
            receivers: BTreeMap::from([(
                stream_id,
                ReliableReceiver::new(
                    brain_id,
                    stream_id,
                    lease_term,
                    partition_generation,
                    max_payload,
                ),
            )]),
            wal: CausalWal::new(lease_term),
            warm_replica: WarmReplica::new(lease_term),
            receipts: ReceiptLedger::default(),
        }
    }

    /// Validate and stage one event, then publish one atomic in-memory commit.
    /// The transition closure must be pure with respect to its input state;
    /// this is the seam that a shard actor uses to stage biological deltas.
    pub fn apply_once<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        let channel_state = self.channel_state.clone();
        self.apply_once_with_channel_state(envelope, channel_state, transition)
    }

    /// Apply one event while publishing the matching in-transit channel
    /// boundary in the same checkpoint transaction.  The channel bytes are
    /// opaque to durability, but they are part of the authoritative cut and
    /// therefore cannot be updated after the biological commit.
    pub fn apply_once_with_channel_state<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        channel_state: Vec<u8>,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        if envelope.kind != crate::data_plane::EnvelopeKind::Event {
            return Err(DataPlaneError::UnknownEnvelopeKind.into());
        }

        let mut receivers = self.receivers.clone();
        // A stream is admitted on first use. The transport service binds the
        // stream to an authenticated sender before calling this method; the
        // storage layer also keeps this permissive for standalone/reference
        // callers that do not have a node identity.
        let receiver = receivers.entry(envelope.stream).or_insert_with(|| {
            ReliableReceiver::new(
                self.brain_id,
                envelope.stream,
                self.lease_term,
                self.partition_generation,
                self.max_payload,
            )
        });
        let mut receiver = receiver.clone();
        let receive_result = receiver.accept(envelope)?;
        let mut event = CausalEvent::new(
            envelope.event,
            CanonicalEventKey::new(
                envelope.tag,
                envelope.stage,
                envelope.source.map(|id| id.raw()).unwrap_or(0),
                envelope.target.map(|id| id.raw()).unwrap_or(0),
                envelope.event.raw(),
            ),
            envelope.payload.clone(),
        );
        event.deferred_from_nonconvergence = envelope.deferred_from_nonconvergence;
        let mut receipts = self.receipts.clone();
        let receipt_result = receipts.record_event(
            envelope.stream,
            envelope.sequence,
            &event,
            envelope.lease_term,
            envelope.partition_generation,
        )?;

        if matches!(receive_result, ReceiveResult::Duplicate { .. }) {
            if matches!(receipt_result, ReceiptOutcome::Duplicate) {
                return Ok(DurableApplyOutcome::Duplicate {
                    sequence: envelope.sequence,
                });
            }
            return Err(DurabilityError::Corrupt(
                "receiver and durable receipt state disagree for a duplicate".to_owned(),
            ));
        }
        if !matches!(receipt_result, ReceiptOutcome::New) {
            return Err(DurabilityError::Corrupt(
                "new receiver sequence already has a durable receipt".to_owned(),
            ));
        }
        if event.key.tag < self.applied_tag {
            return Err(DurabilityError::Corrupt(format!(
                "causal event tag {} moves behind applied tag {}",
                event.key.tag, self.applied_tag
            )));
        }

        // Every candidate is private until the transition has succeeded.
        // WarmReplica::apply is the synchronous replication acknowledgement
        // represented by this reference boundary.
        let mut wal = self.wal.clone();
        let record = wal.append_envelope(self.lease_term, &event, envelope, &channel_state)?;
        let mut warm_replica = self.warm_replica.clone();
        warm_replica.apply(record)?;
        let biological_state = transition(&self.biological_state, &event)
            .map_err(|error| DurabilityError::Transition(error.to_string()))?;

        receivers.insert(envelope.stream, receiver);
        self.receivers = receivers;
        self.receipts = receipts;
        self.wal = wal;
        self.warm_replica = warm_replica;
        self.biological_state = biological_state;
        self.channel_state = channel_state;
        self.committed_tag = event.key.tag;
        self.applied_tag = event.key.tag;
        Ok(DurableApplyOutcome::Applied {
            sequence: envelope.sequence,
        })
    }

    pub fn checkpoint_payload(&self) -> Result<ShardCheckpointPayload, DurabilityError> {
        let causal_state = serde_json::to_vec(self.wal.records())
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        ShardCheckpointPayload::new(
            self.brain_id,
            self.shard_id,
            self.topology_generation,
            self.partition_generation,
            self.lease_term,
            self.committed_tag,
            self.applied_tag,
            self.wal.last_sequence(),
            self.biological_state.clone(),
            causal_state,
            self.channel_state.clone(),
            self.receipts.clone(),
        )
        .with_peripheral_state(self.peripheral_state.clone())?
        .seal()
    }

    pub fn restore_from_checkpoint(
        payload: ShardCheckpointPayload,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<Self, DurabilityError> {
        payload.verify()?;
        let records: Vec<WalRecord> = serde_json::from_slice(&payload.causal_state)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        let wal = CausalWal::from_records(payload.lease_term, records.clone())?;
        if wal.last_sequence() != payload.durable_wal_sequence {
            return Err(DurabilityError::Corrupt(
                "checkpoint WAL position does not match its records".to_owned(),
            ));
        }
        // WAL sequence is the shard-global commit order; receipt sequence is
        // the producer-local order. They are intentionally different once
        // more than one sender is admitted, so validate by event identity
        // rather than indexing the WAL with the receipt sequence.
        let receipts_by_event = payload
            .receipts
            .receipts()
            .map(|receipt| (receipt.event_id, receipt))
            .collect::<BTreeMap<_, _>>();
        if payload.receipts.len() != records.len()
            || payload.receipts.receipts().any(|receipt| {
                receipt.lease_term != payload.lease_term
                    || receipt.partition_generation != payload.partition_generation
                    || records
                        .iter()
                        .find(|record| record.event == receipt.event_id)
                        .is_none_or(|record| {
                            let mut payload_digest = StateDigestBuilder::default();
                            payload_digest.add_domain("causal-payload", &record.payload);
                            record.tag != receipt.tag
                                || payload_digest.finish() != receipt.payload_digest
                        })
            })
            || records
                .iter()
                .any(|record| !receipts_by_event.contains_key(&record.event))
        {
            return Err(DurabilityError::Corrupt(
                "checkpoint receipts do not match the causal WAL".to_owned(),
            ));
        }
        let warm_replica = WarmReplica::from_records(payload.lease_term, records)?;
        let mut stream_sequences = BTreeMap::<StreamId, u64>::new();
        for receipt in payload.receipts.receipts() {
            let next = receipt
                .sequence
                .checked_add(1)
                .ok_or(DurabilityError::SequenceOverflow)?;
            let entry = stream_sequences.entry(receipt.stream_id).or_insert(0);
            if receipt.sequence != *entry {
                return Err(DurabilityError::Corrupt(format!(
                    "checkpoint causal stream {} has a sequence gap: expected {}, received {}",
                    receipt.stream_id, *entry, receipt.sequence
                )));
            }
            *entry = next;
        }
        stream_sequences.entry(stream_id).or_insert(0);
        let receivers = stream_sequences
            .into_iter()
            .map(|(stream, expected)| {
                ReliableReceiver::from_progress(
                    payload.brain_id,
                    stream,
                    payload.lease_term,
                    payload.partition_generation,
                    max_payload,
                    expected,
                    Some(payload.committed_tag),
                )
                .map(|receiver| (stream, receiver))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Self {
            brain_id: payload.brain_id,
            shard_id: payload.shard_id,
            topology_generation: payload.topology_generation,
            partition_generation: payload.partition_generation,
            lease_term: payload.lease_term,
            stream_id,
            max_payload,
            committed_tag: payload.committed_tag,
            applied_tag: payload.applied_tag,
            biological_state: payload.biological_state,
            channel_state: payload.channel_state,
            peripheral_state: payload.peripheral_state,
            receivers,
            wal,
            warm_replica,
            receipts: payload.receipts,
        })
    }

    /// Reissue the complete durable prefix under a strictly newer lease term.
    ///
    /// Promotion changes authority, not biological state.  WAL and receipt
    /// records are therefore deterministically re-signed with the new term so
    /// the promoted writer cannot accept the old authority's envelopes while
    /// preserving the exact replay prefix and checkpoint digest semantics.
    pub fn reissue_term(&mut self, term: LeaseTerm) -> Result<(), DurabilityError> {
        if term <= self.lease_term {
            return Err(DurabilityError::StaleTerm {
                expected: self.lease_term,
                received: term,
            });
        }
        let records = reterm_records(self.wal.records(), term)?;
        let wal = CausalWal::from_records(term, records.clone())?;
        let mut receipts = ReceiptLedger::default();
        for mut receipt in self.receipts.receipts().cloned().collect::<Vec<_>>() {
            receipt.lease_term = term;
            receipts.record(receipt)?;
        }
        let mut receivers = BTreeMap::new();
        for stream in self.receivers.keys().copied() {
            let expected = self
                .receipts
                .receipts()
                .filter(|receipt| receipt.stream_id == stream)
                .map(|receipt| receipt.sequence)
                .max()
                .map(|sequence| sequence.saturating_add(1))
                .unwrap_or(0);
            receivers.insert(
                stream,
                ReliableReceiver::from_progress(
                    self.brain_id,
                    stream,
                    term,
                    self.partition_generation,
                    self.receiver_max_payload(),
                    expected,
                    Some(self.committed_tag),
                )?,
            );
        }
        receivers
            .entry(self.stream_id)
            .or_insert(ReliableReceiver::from_progress(
                self.brain_id,
                self.stream_id,
                term,
                self.partition_generation,
                self.receiver_max_payload(),
                0,
                Some(self.committed_tag),
            )?);
        self.lease_term = term;
        self.receivers = receivers;
        self.wal = wal;
        self.warm_replica = WarmReplica::from_records(term, records)?;
        self.receipts = receipts;
        Ok(())
    }

    pub fn peripheral_cursor_state(&self) -> &PeripheralCursorState {
        &self.peripheral_state
    }

    /// Stage a peripheral cursor update at the same immutable checkpoint
    /// boundary as the biological and causal state.  This operation does not
    /// advance neural time; callers use it only at a declared safe boundary.
    pub fn set_peripheral_cursor_state(
        &mut self,
        state: PeripheralCursorState,
    ) -> Result<(), DurabilityError> {
        state
            .verify()
            .map_err(|error| DurabilityError::Corrupt(error.to_string()))?;
        self.peripheral_state = state;
        Ok(())
    }

    pub fn biological_state(&self) -> &[u8] {
        &self.biological_state
    }

    pub fn channel_state(&self) -> &[u8] {
        &self.channel_state
    }

    pub const fn applied_tag(&self) -> LogicalTag {
        self.applied_tag
    }

    pub const fn durable_log_sequence(&self) -> Option<u64> {
        self.wal.last_sequence()
    }

    pub fn receipt_count(&self) -> usize {
        self.receipts.len()
    }

    pub fn stream_receipts(
        &self,
        stream_id: crate::deterministic::StreamId,
    ) -> Vec<DurableReceipt> {
        self.receipts.stream_receipts(stream_id)
    }

    fn receiver_max_payload(&self) -> usize {
        self.max_payload
    }
}

/// Filesystem-backed owner for a single shard's current durable boundary.
///
/// This is intentionally a current-state record, not a replacement for the
/// immutable checkpoint catalogue.  The wrapper makes the in-memory staged
/// commit recoverable: an event is returned to the caller only after the
/// complete verified payload has been atomically replaced and directory
/// metadata synced.
#[derive(Debug)]
pub struct FileDurableShard {
    path: PathBuf,
    lock_path: PathBuf,
    shard: DurableShard,
}

impl FileDurableShard {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        path: impl Into<PathBuf>,
        brain_id: crate::deterministic::BrainId,
        shard_id: ShardId,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        lease_term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
        biological_state: Vec<u8>,
        channel_state: Vec<u8>,
    ) -> Result<Self, DurabilityError> {
        let path = path.into();
        let lock_path = path.with_extension("lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| DurabilityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            if path.exists() {
                let bytes =
                    fs::read(&path).map_err(|error| DurabilityError::Io(error.to_string()))?;
                let payload: ShardCheckpointPayload = serde_json::from_slice(&bytes)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
                if payload.brain_id != brain_id
                    || payload.shard_id != shard_id
                    || payload.topology_generation != topology_generation
                    || payload.partition_generation != partition_generation
                {
                    return Err(DurabilityError::Corrupt(
                        "durable shard identity or generation does not match the requested owner"
                            .to_owned(),
                    ));
                }
                if payload.lease_term != lease_term {
                    return Err(DurabilityError::StaleTerm {
                        expected: payload.lease_term,
                        received: lease_term,
                    });
                }
                return DurableShard::restore_from_checkpoint(payload, stream_id, max_payload);
            }

            let shard = DurableShard::new(
                brain_id,
                shard_id,
                topology_generation,
                partition_generation,
                lease_term,
                stream_id,
                max_payload,
                biological_state,
                channel_state,
            );
            let payload = shard.checkpoint_payload()?;
            let bytes = serde_json::to_vec(&payload)
                .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
            atomic_replace_and_sync(&path, &bytes)?;
            Ok(shard)
        })();
        let unlock =
            fs2::FileExt::unlock(&lock).map_err(|error| DurabilityError::Io(error.to_string()));
        let shard = match (result, unlock) {
            (Ok(shard), Ok(())) => shard,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        Ok(Self {
            path,
            lock_path,
            shard,
        })
    }

    /// Recreate an owner from a complete warm checkpoint when the previous
    /// active process disappeared before its current-state file was
    /// recovered.  The checkpoint is verified before publication and is
    /// written under the same exclusive lock used by normal owner updates.
    pub fn restore_from_checkpoint(
        path: impl Into<PathBuf>,
        payload: ShardCheckpointPayload,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<Self, DurabilityError> {
        payload.verify()?;
        let path = path.into();
        let lock_path = path.with_extension("lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| DurabilityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            if path.exists() {
                let existing = Self::read_persisted_payload(&path)?;
                if existing != payload {
                    return Err(DurabilityError::Corrupt(
                        "existing owner differs from warm recovery checkpoint".to_owned(),
                    ));
                }
                return DurableShard::restore_from_checkpoint(existing, stream_id, max_payload);
            }
            let shard =
                DurableShard::restore_from_checkpoint(payload.clone(), stream_id, max_payload)?;
            let bytes = serde_json::to_vec(&payload)
                .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
            atomic_replace_and_sync(&path, &bytes)?;
            Ok(shard)
        })();
        let unlock =
            fs2::FileExt::unlock(&lock).map_err(|error| DurabilityError::Io(error.to_string()));
        let shard = match (result, unlock) {
            (Ok(shard), Ok(())) => shard,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        Ok(Self {
            path,
            lock_path,
            shard,
        })
    }

    pub fn shard(&self) -> &DurableShard {
        &self.shard
    }

    pub fn persisted_term(path: &Path) -> Result<Option<LeaseTerm>, DurabilityError> {
        if !path.exists() {
            return Ok(None);
        }
        let payload = Self::read_persisted_payload(path)?;
        Ok(Some(payload.lease_term))
    }

    pub fn read_persisted_payload(path: &Path) -> Result<ShardCheckpointPayload, DurabilityError> {
        let bytes = fs::read(path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let payload: ShardCheckpointPayload = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        payload.verify()?;
        Ok(payload)
    }

    pub fn apply_once<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        let channel_state = self.shard.channel_state.clone();
        self.apply_once_with_channel_state(envelope, channel_state, transition)
    }

    pub fn apply_once_with_channel_state<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        channel_state: Vec<u8>,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        let mut candidate = self.shard.clone();
        let outcome =
            candidate.apply_once_with_channel_state(envelope, channel_state, transition)?;
        self.persist(&candidate)?;
        self.shard = candidate;
        Ok(outcome)
    }

    /// Apply atomically with a process-safe warm replica.  Both the warm
    /// acknowledgement and the active current-state publication happen before
    /// this method changes the in-memory active shard.  A failed replica write
    /// therefore cannot expose a locally committed transition.
    pub fn apply_once_with_warm_replica<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        warm: &mut FileWarmReplica,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        let channel_state = self.shard.channel_state.clone();
        self.apply_once_with_warm_replica_and_channel_state(
            envelope,
            warm,
            channel_state,
            transition,
        )
    }

    pub fn apply_once_with_warm_replica_and_channel_state<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        warm: &mut FileWarmReplica,
        channel_state: Vec<u8>,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        let previous_sequence = self.shard.durable_log_sequence();
        let mut candidate = self.shard.clone();
        let outcome =
            candidate.apply_once_with_channel_state(envelope, channel_state, transition)?;
        if matches!(outcome, DurableApplyOutcome::Applied { .. }) {
            let records: Vec<WalRecord> =
                serde_json::from_slice(&candidate.checkpoint_payload()?.causal_state)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
            if let Some(record) = records.last().cloned() {
                if let Err(error) = warm.apply(record).and_then(|_| {
                    let checkpoint = candidate.checkpoint_payload()?;
                    warm.prepare_checkpoint(&checkpoint)
                }) {
                    let _ = warm.truncate_to(previous_sequence);
                    let _ = warm.rollback_pending();
                    return Err(error);
                }
            }
        }
        if let Err(error) = self.persist(&candidate) {
            if matches!(outcome, DurableApplyOutcome::Applied { .. }) {
                // The warm record was acknowledged first. Do not leave a
                // replica ahead of an active owner that never published the
                // candidate; restore the previous durable prefix.
                let _ = warm.truncate_to(previous_sequence);
                let _ = warm.rollback_pending();
            }
            return Err(error);
        }
        if matches!(outcome, DurableApplyOutcome::Applied { .. }) {
            if let Err(error) = warm.finalize_checkpoint(candidate.durable_log_sequence()) {
                // The active file is still rolled back if the final warm
                // publication fails.  A crash before this branch is safe:
                // the pending slot is reconciled by `open_with_checkpoint`.
                let _ = self.persist(&self.shard);
                let _ = warm.truncate_to(previous_sequence);
                let _ = warm.rollback_pending();
                return Err(error);
            }
        }
        self.shard = candidate;
        Ok(outcome)
    }

    pub fn checkpoint_payload(&self) -> Result<ShardCheckpointPayload, DurabilityError> {
        self.shard.checkpoint_payload()
    }

    pub fn biological_state(&self) -> &[u8] {
        self.shard.biological_state()
    }

    pub fn channel_state(&self) -> &[u8] {
        self.shard.channel_state()
    }

    /// Promote a recovered owner under a newer quorum-issued term while
    /// retaining the exact biological and causal prefix.
    pub fn reissue_term(&mut self, term: LeaseTerm) -> Result<(), DurabilityError> {
        let mut candidate = self.shard.clone();
        candidate.reissue_term(term)?;
        self.persist(&candidate)?;
        self.shard = candidate;
        Ok(())
    }

    /// Publish a peripheral cursor update together with the warm checkpoint.
    /// No WAL record is invented for this control-state-only update; the
    /// owner and warm checkpoint still advance as one recoverable boundary.
    pub fn set_peripheral_cursor_state_with_warm(
        &mut self,
        warm: &mut FileWarmReplica,
        state: PeripheralCursorState,
    ) -> Result<(), DurabilityError> {
        let mut candidate = self.shard.clone();
        candidate.set_peripheral_cursor_state(state)?;
        let checkpoint = candidate.checkpoint_payload()?;
        if let Err(error) = warm.prepare_checkpoint(&checkpoint) {
            let _ = warm.rollback_pending();
            return Err(error);
        }
        if let Err(error) = self.persist(&candidate) {
            let _ = warm.rollback_pending();
            return Err(error);
        }
        if let Err(error) = warm.finalize_checkpoint(candidate.durable_log_sequence()) {
            let _ = self.persist(&self.shard);
            let _ = warm.rollback_pending();
            return Err(error);
        }
        self.shard = candidate;
        Ok(())
    }

    /// Persist a validated peripheral cursor update at the current neural
    /// boundary. This is intentionally separate from event admission so a
    /// caller can checkpoint a channel drain or actuator fence atomically.
    pub fn set_peripheral_cursor_state(
        &mut self,
        state: PeripheralCursorState,
    ) -> Result<(), DurabilityError> {
        let mut candidate = self.shard.clone();
        candidate.set_peripheral_cursor_state(state)?;
        self.persist(&candidate)?;
        self.shard = candidate;
        Ok(())
    }

    fn persist(&self, shard: &DurableShard) -> Result<(), DurabilityError> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| DurabilityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            if self.path.exists() {
                let bytes =
                    fs::read(&self.path).map_err(|error| DurabilityError::Io(error.to_string()))?;
                let existing: ShardCheckpointPayload = serde_json::from_slice(&bytes)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
                if existing.lease_term > shard.lease_term {
                    return Err(DurabilityError::StaleTerm {
                        expected: existing.lease_term,
                        received: shard.lease_term,
                    });
                }
            }
            let payload = shard.checkpoint_payload()?;
            let bytes = serde_json::to_vec(&payload)
                .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
            atomic_replace_and_sync(&self.path, &bytes)
        })();
        let unlock =
            fs2::FileExt::unlock(&lock).map_err(|error| DurabilityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
        }
    }
}

/// Explicit serialisable contents of a shard checkpoint.
///
/// The biological bytes are intentionally opaque to this storage boundary;
/// the shard kernel owns their schema.  Causal queues, channel state and
/// receipts are nevertheless first-class fields and cannot be mistaken for a
/// complete checkpoint when omitted.  `seal` computes a deterministic digest
/// over all fields except the digest itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardCheckpointPayload {
    pub schema_version: SchemaVersion,
    pub brain_id: crate::deterministic::BrainId,
    pub shard_id: ShardId,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub lease_term: LeaseTerm,
    pub committed_tag: LogicalTag,
    pub applied_tag: LogicalTag,
    pub durable_wal_sequence: Option<u64>,
    pub biological_state: Vec<u8>,
    pub causal_state: Vec<u8>,
    pub channel_state: Vec<u8>,
    #[serde(default)]
    pub peripheral_state: PeripheralCursorState,
    pub receipts: ReceiptLedger,
    pub state_digest: StateDigest,
}

impl ShardCheckpointPayload {
    pub const MAX_BYTES: usize = 64 * 1024 * 1024;

    pub fn new(
        brain_id: crate::deterministic::BrainId,
        shard_id: ShardId,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        lease_term: LeaseTerm,
        committed_tag: LogicalTag,
        applied_tag: LogicalTag,
        durable_wal_sequence: Option<u64>,
        biological_state: Vec<u8>,
        causal_state: Vec<u8>,
        channel_state: Vec<u8>,
        receipts: ReceiptLedger,
    ) -> Self {
        Self {
            schema_version: SchemaVersion::CURRENT,
            brain_id,
            shard_id,
            topology_generation,
            partition_generation,
            lease_term,
            committed_tag,
            applied_tag,
            durable_wal_sequence,
            biological_state,
            causal_state,
            channel_state,
            peripheral_state: PeripheralCursorState::empty(),
            receipts,
            state_digest: WAL_GENESIS_DIGEST,
        }
    }

    pub fn with_peripheral_state(
        mut self,
        peripheral_state: PeripheralCursorState,
    ) -> Result<Self, DurabilityError> {
        peripheral_state
            .verify()
            .map_err(|error| DurabilityError::Corrupt(error.to_string()))?;
        self.peripheral_state = peripheral_state;
        Ok(self)
    }

    pub fn seal(mut self) -> Result<Self, DurabilityError> {
        let bytes = self.encoded_bytes_without_digest()?;
        if bytes.len() > Self::MAX_BYTES {
            return Err(DurabilityError::PayloadTooLarge { bytes: bytes.len() });
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-checkpoint:v1", bytes);
        self.state_digest = digest.finish();
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), DurabilityError> {
        if self.schema_version != SchemaVersion::CURRENT {
            return Err(DurabilityError::Corrupt(format!(
                "unsupported shard checkpoint schema {}",
                self.schema_version
            )));
        }
        self.peripheral_state
            .verify()
            .map_err(|error| DurabilityError::Corrupt(error.to_string()))?;
        let bytes = self.encoded_bytes_without_digest()?;
        if bytes.len() > Self::MAX_BYTES {
            return Err(DurabilityError::PayloadTooLarge { bytes: bytes.len() });
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-checkpoint:v1", bytes);
        if digest.finish() != self.state_digest {
            // Checkpoints written before the explicit peripheral cursor
            // domain was added have no field on the wire. They remain
            // recoverable when their original digest is valid; every newly
            // sealed checkpoint includes the richer domain above.
            if self.peripheral_state == PeripheralCursorState::empty() {
                let legacy_bytes = self.encoded_bytes_without_peripheral_state()?;
                let mut legacy_digest = StateDigestBuilder::default();
                legacy_digest.add_domain("shard-checkpoint:v1", legacy_bytes);
                if legacy_digest.finish() == self.state_digest {
                    return Ok(());
                }
            }
            return Err(DurabilityError::ShardCheckpointDigestMismatch);
        }
        Ok(())
    }

    fn encoded_bytes_without_digest(&self) -> Result<Vec<u8>, DurabilityError> {
        #[derive(Serialize)]
        struct DigestMaterial<'a> {
            schema_version: SchemaVersion,
            brain_id: crate::deterministic::BrainId,
            shard_id: ShardId,
            topology_generation: TopologyGeneration,
            partition_generation: PartitionGeneration,
            lease_term: LeaseTerm,
            committed_tag: LogicalTag,
            applied_tag: LogicalTag,
            durable_wal_sequence: Option<u64>,
            biological_state: &'a [u8],
            causal_state: &'a [u8],
            channel_state: &'a [u8],
            peripheral_state: &'a PeripheralCursorState,
            receipts: &'a ReceiptLedger,
        }
        serde_json::to_vec(&DigestMaterial {
            schema_version: self.schema_version,
            brain_id: self.brain_id,
            shard_id: self.shard_id,
            topology_generation: self.topology_generation,
            partition_generation: self.partition_generation,
            lease_term: self.lease_term,
            committed_tag: self.committed_tag,
            applied_tag: self.applied_tag,
            durable_wal_sequence: self.durable_wal_sequence,
            biological_state: &self.biological_state,
            causal_state: &self.causal_state,
            channel_state: &self.channel_state,
            peripheral_state: &self.peripheral_state,
            receipts: &self.receipts,
        })
        .map_err(|error| DurabilityError::Encoding(error.to_string()))
    }

    fn encoded_bytes_without_peripheral_state(&self) -> Result<Vec<u8>, DurabilityError> {
        #[derive(Serialize)]
        struct LegacyDigestMaterial<'a> {
            schema_version: SchemaVersion,
            brain_id: crate::deterministic::BrainId,
            shard_id: ShardId,
            topology_generation: TopologyGeneration,
            partition_generation: PartitionGeneration,
            lease_term: LeaseTerm,
            committed_tag: LogicalTag,
            applied_tag: LogicalTag,
            durable_wal_sequence: Option<u64>,
            biological_state: &'a [u8],
            causal_state: &'a [u8],
            channel_state: &'a [u8],
            receipts: &'a ReceiptLedger,
        }
        serde_json::to_vec(&LegacyDigestMaterial {
            schema_version: self.schema_version,
            brain_id: self.brain_id,
            shard_id: self.shard_id,
            topology_generation: self.topology_generation,
            partition_generation: self.partition_generation,
            lease_term: self.lease_term,
            committed_tag: self.committed_tag,
            applied_tag: self.applied_tag,
            durable_wal_sequence: self.durable_wal_sequence,
            biological_state: &self.biological_state,
            causal_state: &self.causal_state,
            channel_state: &self.channel_state,
            receipts: &self.receipts,
        })
        .map_err(|error| DurabilityError::Encoding(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_plane::EnvelopeKind;
    use crate::deterministic::{BrainId, CanonicalEventKey, EventStage, RouteId, StreamId};

    fn event(id: u64) -> CausalEvent {
        let event_id = EventId::new(id).expect("event id");
        CausalEvent::new(
            event_id,
            CanonicalEventKey::new(LogicalTag::ZERO, EventStage::SynapticTransition, 1, 2, id),
            vec![id as u8],
        )
    }

    #[test]
    fn file_wal_reopens_only_after_an_atomic_synced_append() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-wal-{}-{}.json",
            std::process::id(),
            EventId::new(1).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let term = LeaseTerm::INITIAL;
        let mut wal = FileCausalWal::open(&path, term).expect("open WAL");
        wal.append(term, &event(1)).expect("append WAL");
        let next_term = LeaseTerm::new(2).expect("term");
        wal.fence(next_term).expect("fence WAL");
        wal.append(next_term, &event(2)).expect("append fenced WAL");
        assert_eq!(wal.last_sequence(), Some(1));
        drop(wal);

        let reopened = FileCausalWal::open(&path, next_term).expect("reopen WAL");
        assert_eq!(reopened.records_since(0).count(), 2);
        assert!(matches!(
            FileCausalWal::open(&path, term),
            Err(DurabilityError::StaleTerm { .. })
        ));
        std::fs::remove_file(path).expect("remove test WAL");
    }

    #[test]
    fn file_checkpoint_publication_is_immutable_and_verifiable() {
        let root =
            std::env::temp_dir().join(format!("aarnn-file-checkpoint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = FileCheckpointStore::new(&root).expect("checkpoint store");
        let checkpoint_id = EventId::new(7).expect("checkpoint id");
        let manifest = store
            .publish(
                checkpoint_id,
                LeaseTerm::INITIAL,
                PartitionGeneration::INITIAL,
                Some(3),
                vec![1, 2, 3],
            )
            .expect("publish checkpoint");
        assert_eq!(
            store.verify(checkpoint_id).expect("verify").manifest,
            manifest
        );
        assert!(matches!(
            store.publish(
                checkpoint_id,
                LeaseTerm::INITIAL,
                PartitionGeneration::INITIAL,
                Some(3),
                vec![9],
            ),
            Err(DurabilityError::CheckpointAlreadyPublished(_))
        ));
        std::fs::remove_dir_all(root).expect("remove test checkpoint store");
    }

    #[test]
    fn file_wal_rejects_payload_mutation_even_when_sequence_is_contiguous() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-wal-integrity-{}-{}.json",
            std::process::id(),
            EventId::new(11).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let mut wal = FileCausalWal::open(&path, LeaseTerm::INITIAL).expect("open WAL");
        wal.append(LeaseTerm::INITIAL, &event(11))
            .expect("append WAL");
        drop(wal);

        let bytes = std::fs::read(&path).expect("read WAL");
        let mut document: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON WAL");
        document["records"][0]["payload"] = serde_json::json!([99]);
        std::fs::write(
            &path,
            serde_json::to_vec(&document).expect("encode JSON WAL"),
        )
        .expect("tamper WAL");
        assert!(matches!(
            FileCausalWal::open(&path, LeaseTerm::INITIAL),
            Err(DurabilityError::Corrupt(_))
        ));
        std::fs::remove_file(path).expect("remove test WAL");
    }

    #[test]
    fn file_warm_replica_reopens_across_process_boundaries_and_deduplicates() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-warm-{}-{}.json",
            std::process::id(),
            EventId::new(21).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let mut active = CausalWal::new(LeaseTerm::INITIAL);
        let record = active
            .append(LeaseTerm::INITIAL, &event(21))
            .expect("active record");
        let mut first = FileWarmReplica::open(&path, LeaseTerm::INITIAL, []).expect("open warm");
        first.apply(record.clone()).expect("replicate");
        first.apply(record).expect("duplicate retransmission");
        drop(first);
        let reopened = FileWarmReplica::open(&path, LeaseTerm::INITIAL, []).expect("reopen warm");
        assert_eq!(reopened.durable_sequence().expect("sequence"), Some(0));
        assert!(matches!(
            FileWarmReplica::open(&path, LeaseTerm::new(2).unwrap(), []),
            Err(DurabilityError::StaleTerm { .. })
        ));
        std::fs::remove_file(&path).expect("remove warm");
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn warm_replica_recovers_biological_state_across_a_crash_window() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-warm-checkpoint-{}-{}.json",
            std::process::id(),
            EventId::new(24).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));

        let initial = shard();
        let initial_checkpoint = initial.checkpoint_payload().expect("initial checkpoint");
        let initial_records: Vec<WalRecord> =
            serde_json::from_slice(&initial_checkpoint.causal_state).expect("initial WAL");
        let mut advanced = initial.clone();
        advanced
            .apply_once(&envelope(0, 24, &[7]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("advance active shard");
        let advanced_checkpoint = advanced.checkpoint_payload().expect("advanced checkpoint");
        let advanced_records: Vec<WalRecord> =
            serde_json::from_slice(&advanced_checkpoint.causal_state).expect("advanced WAL");

        let mut warm = FileWarmReplica::open_with_checkpoint(
            &path,
            LeaseTerm::INITIAL,
            initial_records.clone(),
            Some(initial_checkpoint.clone()),
        )
        .expect("open warm checkpoint");
        warm.apply(advanced_records[0].clone())
            .expect("prepare warm WAL");
        warm.prepare_checkpoint(&advanced_checkpoint)
            .expect("prepare warm state");
        drop(warm);

        // The active publication did not complete, so recovery rolls the
        // prepared state back to the active checkpoint and does not expose
        // the biological bytes as committed.
        let recovered_old = FileWarmReplica::open_with_checkpoint(
            &path,
            LeaseTerm::INITIAL,
            initial_records.clone(),
            Some(initial_checkpoint.clone()),
        )
        .expect("recover old active boundary");
        assert_eq!(
            recovered_old.checkpoint().expect("old checkpoint"),
            Some(initial_checkpoint)
        );
        drop(recovered_old);

        // If the active file did publish before the process died, the same
        // pending slot is promoted by matching the active WAL prefix.
        let mut warm =
            FileWarmReplica::open_with_checkpoint(&path, LeaseTerm::INITIAL, initial_records, None)
                .expect("reopen warm after rollback");
        warm.apply(advanced_records[0].clone())
            .expect("replicate advanced WAL");
        warm.prepare_checkpoint(&advanced_checkpoint)
            .expect("prepare advanced state");
        drop(warm);
        let recovered_new = FileWarmReplica::open_with_checkpoint(
            &path,
            LeaseTerm::INITIAL,
            advanced_records,
            Some(advanced_checkpoint.clone()),
        )
        .expect("recover published active boundary");
        assert_eq!(
            recovered_new
                .recovery_checkpoint()
                .expect("recovery checkpoint")
                .expect("checkpoint")
                .biological_state,
            advanced_checkpoint.biological_state
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn warm_replica_repairs_a_synced_record_when_active_publish_did_not_start() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-warm-prefix-repair-{}-{}.json",
            std::process::id(),
            EventId::new(25).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));

        let initial = shard();
        let initial_checkpoint = initial.checkpoint_payload().expect("initial checkpoint");
        let initial_records: Vec<WalRecord> =
            serde_json::from_slice(&initial_checkpoint.causal_state).expect("initial WAL");
        let mut advanced = initial.clone();
        advanced
            .apply_once(&envelope(0, 25, &[8]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("advance active shard");
        let advanced_checkpoint = advanced.checkpoint_payload().expect("advanced checkpoint");
        let advanced_records: Vec<WalRecord> =
            serde_json::from_slice(&advanced_checkpoint.causal_state).expect("advanced WAL");

        let mut warm = FileWarmReplica::open_with_checkpoint(
            &path,
            LeaseTerm::INITIAL,
            initial_records.clone(),
            Some(initial_checkpoint.clone()),
        )
        .expect("open warm checkpoint");
        // Model the crash after the record fsync and before prepare_checkpoint.
        warm.apply(advanced_records[0].clone())
            .expect("replicate WAL record");
        drop(warm);

        let repaired = FileWarmReplica::open_with_checkpoint(
            &path,
            LeaseTerm::INITIAL,
            initial_records,
            Some(initial_checkpoint.clone()),
        )
        .expect("repair uncommitted warm suffix");
        assert_eq!(
            repaired.records().expect("records"),
            Vec::<WalRecord>::new()
        );
        assert_eq!(
            repaired.checkpoint().expect("checkpoint"),
            Some(initial_checkpoint)
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn file_warm_replica_is_shared_by_independent_child_processes() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-warm-child-{}-{}.json",
            std::process::id(),
            EventId::new(22).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));
        for _ in 0..2 {
            let status =
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args([
                        "--exact",
                        "durability::tests::cross_process_warm_replica_worker",
                        "--nocapture",
                    ])
                    .env("AARNN_DURABILITY_CHILD", "apply")
                    .env("AARNN_DURABILITY_WARM_PATH", &path)
                    .status()
                    .expect("spawn warm replica child");
            assert!(status.success(), "warm replica child failed: {status}");
        }
        let reopened = FileWarmReplica::open(&path, LeaseTerm::INITIAL, [])
            .expect("reopen child-written warm replica");
        assert_eq!(reopened.durable_sequence().expect("sequence"), Some(0));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn cross_process_warm_replica_worker() {
        if std::env::var("AARNN_DURABILITY_CHILD").ok().as_deref() != Some("apply") {
            return;
        }
        let path = std::env::var_os("AARNN_DURABILITY_WARM_PATH").expect("warm path");
        let mut wal = CausalWal::new(LeaseTerm::INITIAL);
        let record = wal
            .append(LeaseTerm::INITIAL, &event(22))
            .expect("child record");
        let mut replica =
            FileWarmReplica::open(path, LeaseTerm::INITIAL, []).expect("open child warm replica");
        replica.apply(record).expect("apply child record");
    }

    #[test]
    fn promoted_file_owner_reissues_term_and_fences_stale_process() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-owner-promotion-{}-{}.json",
            std::process::id(),
            EventId::new(23).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));
        let term = LeaseTerm::INITIAL;
        let next_term = LeaseTerm::new(2).expect("next term");
        let mut active = FileDurableShard::open(
            &path,
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            term,
            StreamId::new(2).expect("stream"),
            64,
            vec![0],
            b"empty-channel".to_vec(),
        )
        .expect("active owner");
        active
            .apply_once(&envelope(0, 60, &[4]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("initial durable apply");
        let mut stale = FileDurableShard::open(
            &path,
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            term,
            StreamId::new(2).expect("stream"),
            64,
            vec![0],
            b"empty-channel".to_vec(),
        )
        .expect("stale process view");
        active.reissue_term(next_term).expect("promote owner");
        let mut promoted = FileDurableShard::open(
            &path,
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            next_term,
            StreamId::new(2).expect("stream"),
            64,
            vec![0],
            b"empty-channel".to_vec(),
        )
        .expect("promoted owner");
        assert_eq!(promoted.biological_state(), &[4]);
        assert!(matches!(
            stale.apply_once(&envelope(1, 61, &[1]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            }),
            Err(DurabilityError::StaleTerm { .. })
        ));
        promoted
            .apply_once(
                &envelope_with_term(1, 61, &[1], next_term),
                |current, event| Ok::<_, &'static str>(vec![current[0] + event.payload[0]]),
            )
            .expect("new owner continues under new term");
        assert_eq!(promoted.biological_state(), &[5]);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn warm_replica_accepts_exact_retransmission_but_rejects_conflict() {
        let mut wal = CausalWal::new(LeaseTerm::INITIAL);
        let record = wal
            .append(LeaseTerm::INITIAL, &event(12))
            .expect("append WAL");
        let mut replica = WarmReplica::new(LeaseTerm::INITIAL);
        replica.apply(record.clone()).expect("first receipt");
        assert_eq!(
            replica.apply(record.clone()).expect("duplicate receipt"),
            ()
        );
        assert_eq!(replica.applied(), 1);

        let mut conflicting = record;
        conflicting.payload = vec![42];
        assert!(matches!(
            replica.apply(conflicting),
            Err(DurabilityError::Corrupt(_))
        ));
    }

    #[test]
    fn receipt_ledger_deduplicates_by_stream_position_and_event_identity() {
        let stream = StreamId::new(3).expect("stream");
        let generation = PartitionGeneration::INITIAL;
        let mut ledger = ReceiptLedger::default();
        let input = event(13);
        assert_eq!(
            ledger
                .record_event(stream, 0, &input, LeaseTerm::INITIAL, generation)
                .expect("record receipt"),
            ReceiptOutcome::New
        );
        assert_eq!(
            ledger
                .record_event(stream, 0, &input, LeaseTerm::INITIAL, generation)
                .expect("deduplicate receipt"),
            ReceiptOutcome::Duplicate
        );
        let mut changed = event(13);
        changed.payload = vec![200];
        assert!(matches!(
            ledger.record_event(stream, 0, &changed, LeaseTerm::INITIAL, generation),
            Err(DurabilityError::Corrupt(_))
        ));
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn shard_checkpoint_roundtrip_verifies_all_explicit_state_domains() {
        let mut payload = ShardCheckpointPayload::new(
            crate::deterministic::BrainId::new(1).expect("brain"),
            ShardId::new(2).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            LogicalTag::new(8, 0),
            LogicalTag::new(8, 0),
            Some(4),
            braced_bytes(b"biological"),
            braced_bytes(b"causal"),
            braced_bytes(b"channels"),
            ReceiptLedger::default(),
        );
        let mut store = CheckpointStore::default();
        let id = EventId::new(14).expect("checkpoint");
        store
            .publish_shard(id, payload.clone())
            .expect("publish shard checkpoint");
        let restored = store.verify_shard(id).expect("verify shard checkpoint");
        assert_eq!(restored, payload.clone().seal().expect("seal payload"));

        payload.channel_state[0] ^= 1;
        payload.state_digest = StateDigest([0; 16]);
        assert!(matches!(
            payload.verify(),
            Err(DurabilityError::ShardCheckpointDigestMismatch)
        ));
    }

    #[test]
    fn legacy_checkpoint_without_peripheral_domain_remains_recoverable() {
        let payload = ShardCheckpointPayload::new(
            crate::deterministic::BrainId::new(1).expect("brain"),
            ShardId::new(2).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            LogicalTag::ZERO,
            LogicalTag::ZERO,
            None,
            braced_bytes(b"biological"),
            braced_bytes(b"causal"),
            braced_bytes(b"channels"),
            ReceiptLedger::default(),
        )
        .seal()
        .expect("current checkpoint");
        let mut legacy = payload.clone();
        let mut digest = StateDigestBuilder::default();
        digest.add_domain(
            "shard-checkpoint:v1",
            legacy
                .encoded_bytes_without_peripheral_state()
                .expect("legacy material"),
        );
        legacy.state_digest = digest.finish();
        legacy.verify().expect("legacy checkpoint remains readable");
    }

    fn braced_bytes(value: &[u8]) -> Vec<u8> {
        value.to_vec()
    }

    fn envelope(sequence: u64, event_id: u64, payload: &[u8]) -> CausalEnvelope {
        CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain: BrainId::new(1).expect("brain"),
            stream: StreamId::new(2).expect("stream"),
            sequence,
            lease_term: LeaseTerm::INITIAL,
            route: RouteId::new(3).expect("route"),
            partition_generation: PartitionGeneration::INITIAL,
            source: None,
            target: None,
            tag: LogicalTag::new(sequence, 0),
            event: EventId::new(event_id).expect("event"),
            stage: EventStage::SpikeDecision,
            kind: EnvelopeKind::Event,
            payload: payload.to_vec(),
            deferred_from_nonconvergence: false,
        }
    }

    fn envelope_with_term(
        sequence: u64,
        event_id: u64,
        payload: &[u8],
        lease_term: LeaseTerm,
    ) -> CausalEnvelope {
        let mut envelope = envelope(sequence, event_id, payload);
        envelope.lease_term = lease_term;
        envelope
    }

    fn shard() -> DurableShard {
        DurableShard::new(
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(2).expect("stream"),
            64,
            vec![0],
            braced_bytes(b"empty-channel"),
        )
    }

    #[test]
    fn warm_replica_accepts_consecutive_records_without_skipping_a_sequence() {
        let mut wal = CausalWal::new(LeaseTerm::INITIAL);
        let first = wal.append(LeaseTerm::INITIAL, &event(20)).expect("first");
        let second = wal.append(LeaseTerm::INITIAL, &event(21)).expect("second");
        let mut replica = WarmReplica::new(LeaseTerm::INITIAL);
        replica.apply(first).expect("first receipt");
        replica.apply(second).expect("second receipt");
        assert_eq!(replica.applied(), 2);
        assert_eq!(replica.durable_sequence(), Some(1));
    }

    #[test]
    fn durable_shard_stages_transition_and_deduplicates_replay() {
        let mut shard = shard();
        let first = envelope(0, 30, &[2]);
        assert_eq!(
            shard
                .apply_once(&first, |current, event| {
                    Ok::<_, &'static str>(vec![current[0].saturating_add(event.payload[0])])
                })
                .expect("apply first"),
            DurableApplyOutcome::Applied { sequence: 0 }
        );
        assert_eq!(shard.biological_state(), &[2]);

        assert_eq!(
            shard
                .apply_once(&first, |_, _| -> Result<Vec<u8>, &'static str> {
                    panic!("duplicate must not invoke the biological transition")
                })
                .expect("duplicate"),
            DurableApplyOutcome::Duplicate { sequence: 0 }
        );
        assert_eq!(shard.receipt_count(), 1);

        let second = envelope(1, 31, &[3]);
        shard
            .apply_once(&second, |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("apply second");
        assert_eq!(shard.biological_state(), &[5]);
        assert_eq!(shard.durable_log_sequence(), Some(1));
    }

    #[test]
    fn durable_shard_keeps_independent_sender_cursors_in_one_wal_order() {
        let mut shard = shard();
        let sender_b = StreamId::new(44).expect("second sender stream");
        let mut second = envelope(0, 41, &[3]);
        second.stream = sender_b;
        let mut third = envelope(1, 42, &[4]);
        third.event = EventId::new(42).expect("third event");

        shard
            .apply_once(&envelope(0, 40, &[2]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("first sender event");
        shard
            .apply_once(&second, |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("second sender event");
        shard
            .apply_once(&third, |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("first sender resumes");
        assert_eq!(shard.biological_state(), &[9]);
        assert_eq!(shard.durable_log_sequence(), Some(2));
        assert_eq!(shard.receipt_count(), 3);

        let checkpoint = shard.checkpoint_payload().expect("checkpoint");
        let restored = DurableShard::restore_from_checkpoint(
            checkpoint,
            StreamId::new(2).expect("primary stream"),
            64,
        )
        .expect("multi-sender checkpoint restores");
        assert_eq!(restored.biological_state(), &[9]);

        let mut replay = second.clone();
        replay.payload = vec![99];
        assert!(matches!(
            restored
                .clone()
                .apply_once(&replay, |_, _| Ok::<_, &'static str>(vec![0])),
            Err(DurabilityError::Corrupt(_))
        ));
    }

    #[test]
    fn failed_transition_leaves_the_durable_boundary_unchanged() {
        let mut shard = shard();
        let input = envelope(0, 40, &[7]);
        assert!(matches!(
            shard.apply_once(&input, |_, _| Err::<Vec<u8>, _>("reject")),
            Err(DurabilityError::Transition(_))
        ));
        assert_eq!(shard.biological_state(), &[0]);
        assert_eq!(shard.durable_log_sequence(), None);
        assert_eq!(shard.receipt_count(), 0);

        shard
            .apply_once(&input, |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("retry after failed staging");
        assert_eq!(shard.biological_state(), &[7]);
    }

    #[test]
    fn shard_checkpoint_restores_wal_receiver_receipts_and_biological_state() {
        let mut shard = shard();
        let input = envelope(0, 50, &[9]);
        shard
            .apply_once(&input, |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("apply");
        let checkpoint = shard.checkpoint_payload().expect("checkpoint");
        let mut restored = DurableShard::restore_from_checkpoint(
            checkpoint,
            StreamId::new(2).expect("stream"),
            64,
        )
        .expect("restore");
        assert_eq!(restored.biological_state(), &[9]);
        assert_eq!(restored.durable_log_sequence(), Some(0));
        assert_eq!(restored.receipt_count(), 1);
        assert_eq!(
            restored
                .apply_once(&envelope(1, 51, &[1]), |current, event| {
                    Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
                })
                .expect("replay tail"),
            DurableApplyOutcome::Applied { sequence: 1 }
        );
        assert_eq!(restored.biological_state(), &[10]);
    }

    #[test]
    fn file_durable_shard_reopens_at_the_last_verified_commit() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-durable-shard-{}-{}.json",
            std::process::id(),
            EventId::new(60).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let mut durable = FileDurableShard::open(
            &path,
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(2).expect("stream"),
            64,
            vec![0],
            b"empty-channel".to_vec(),
        )
        .expect("open durable shard");
        durable
            .apply_once(&envelope(0, 61, &[4]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("durable apply");
        drop(durable);

        let mut reopened = FileDurableShard::open(
            &path,
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(2).expect("stream"),
            64,
            vec![99],
            b"wrong-default".to_vec(),
        )
        .expect("reopen durable shard");
        assert_eq!(reopened.shard().biological_state(), &[4]);
        assert_eq!(reopened.shard().durable_log_sequence(), Some(0));
        assert!(matches!(
            FileDurableShard::open(
                &path,
                BrainId::new(1).expect("brain"),
                ShardId::new(10).expect("different shard"),
                TopologyGeneration::INITIAL,
                PartitionGeneration::INITIAL,
                LeaseTerm::INITIAL,
                StreamId::new(2).expect("stream"),
                64,
                vec![0],
                Vec::new(),
            ),
            Err(DurabilityError::Corrupt(_))
        ));
        reopened
            .apply_once(&envelope(1, 62, &[3]), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("apply after reopen");
        assert_eq!(reopened.shard().biological_state(), &[7]);
        std::fs::remove_file(path).expect("remove test durable shard");
    }

    #[test]
    fn file_durable_shard_rejects_tampered_current_state() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-file-durable-shard-tamper-{}-{}.json",
            std::process::id(),
            EventId::new(63).expect("constant")
        ));
        let _ = std::fs::remove_file(&path);
        let durable = FileDurableShard::open(
            &path,
            BrainId::new(1).expect("brain"),
            ShardId::new(9).expect("shard"),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(2).expect("stream"),
            64,
            vec![0],
            Vec::new(),
        )
        .expect("open durable shard");
        drop(durable);
        let mut document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read state")).expect("JSON");
        document["biological_state"] = serde_json::json!([99]);
        std::fs::write(&path, serde_json::to_vec(&document).expect("encode state"))
            .expect("tamper state");
        assert!(matches!(
            FileDurableShard::open(
                &path,
                BrainId::new(1).expect("brain"),
                ShardId::new(9).expect("shard"),
                TopologyGeneration::INITIAL,
                PartitionGeneration::INITIAL,
                LeaseTerm::INITIAL,
                StreamId::new(2).expect("stream"),
                64,
                vec![0],
                Vec::new(),
            ),
            Err(DurabilityError::ShardCheckpointDigestMismatch)
        ));
        std::fs::remove_file(path).expect("remove test durable shard");
    }
}
