//! Immutable checkpoint and fenced causal-log reference storage.

use crate::causal::CausalEvent;
use crate::deterministic::{
    EventId, LeaseTerm, LogicalTag, PartitionGeneration, SchemaVersion, StateDigest,
    StateDigestBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
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
        self.require_term(term)?;
        let record = WalRecord {
            sequence: self.next_sequence,
            lease_term: term,
            tag: event.key.tag,
            event: event.id,
            payload: event.payload.clone(),
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
        if document
            .records
            .iter()
            .enumerate()
            .any(|(index, record)| record.sequence != index as u64 || record.lease_term.raw() == 0)
        {
            return Err(DurabilityError::Corrupt(
                "WAL records are not a contiguous sequence".to_owned(),
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

fn atomic_replace_and_sync(path: &Path, bytes: &[u8]) -> Result<(), DurabilityError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| DurabilityError::Io(error.to_string()))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file =
        fs::File::create(&temporary).map_err(|error| DurabilityError::Io(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| DurabilityError::Io(error.to_string()))?;
    fs::rename(&temporary, path).map_err(|error| DurabilityError::Io(error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
    }
    Ok(())
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
        if record.sequence != self.next_sequence {
            return Err(DurabilityError::ReplicaSequenceGap {
                expected: self.next_sequence,
                received: record.sequence,
            });
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::{CanonicalEventKey, EventStage};

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
}
