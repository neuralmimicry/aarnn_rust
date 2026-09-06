//! Opt-in adapter that places the live managed-network loop behind the
//! durable shard commit boundary.
//!
//! `Runner` remains the biological compatibility kernel, but a completed step
//! is not exposed to the distributed transport until its versioned snapshot
//! has been accepted by the fenced WAL and, when configured, by a separate
//! process-safe warm replica.  The caller supplies the already staged runner
//! snapshot and restores its previous JSON snapshot if the commit fails.

use crate::causal::CausalEvent;
use crate::data_plane::{CausalEnvelope, EnvelopeKind};
use crate::deterministic::{
    BrainId, EventId, EventStage, LeaseTerm, LogicalTag, PartitionGeneration, RouteId,
    SchemaVersion, ShardId, StateDigest, StateDigestBuilder, StreamId, TopologyGeneration,
};
use crate::durability::{
    DurabilityError, DurableApplyOutcome, FileDurableShard, FileWarmReplica, WalRecord,
    atomic_replace_and_sync,
};
use crate::runner::Runner;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct PromotionIntent {
    schema_version: u32,
    from_term: LeaseTerm,
    to_term: LeaseTerm,
}

const PROMOTION_INTENT_SCHEMA_VERSION: u32 = 1;

/// Recovery record for the only multi-file commit in the managed path: the
/// biological/channel snapshot and the outbound causal batches.  The record
/// is written before either side is changed and is removed only after both
/// durable publications succeed.  It is a small write-ahead transaction
/// marker, not a substitute for a distributed transaction coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedCommitIntent {
    schema_version: u32,
    lease_term: LeaseTerm,
    previous_wal_sequence: Option<u64>,
    tick: u64,
    snapshot: Vec<u8>,
    channel_state: Vec<u8>,
    outbox_start: BTreeMap<String, u64>,
    outbox: BTreeMap<String, Vec<DurableCausalBatch>>,
}

const MANAGED_COMMIT_INTENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub struct ManagedDurability {
    owner: FileDurableShard,
    warm: Option<FileWarmReplica>,
    causal_outbox: DurableCausalOutbox,
    commit_intent_path: PathBuf,
    promotion_intent_path: PathBuf,
    brain: BrainId,
    route: RouteId,
    stream: StreamId,
    shard: ShardId,
    generation: PartitionGeneration,
    topology_generation: TopologyGeneration,
    term: LeaseTerm,
    fencing_token: u64,
    authority_path: Option<PathBuf>,
    authority_replicas: Option<Vec<(String, PathBuf)>>,
    authority_members: Vec<String>,
    authority_node_id: String,
}

/// A bounded, crash-safe outbound causal batch.  This is deliberately a
/// transport-neutral representation of the generated `SpikeBatch`; keeping
/// it here means reconnect/retry does not depend on prost-generated types or
/// on a live `Runner` projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableCausalBatch {
    pub layer_index: u32,
    pub step_index: i64,
    pub is_backward: bool,
    pub spike_indices: Vec<u32>,
    pub aer_payload: Vec<u8>,
    pub aer_base: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableCausalOutboxEntry {
    pub sequence: u64,
    pub batch: DurableCausalBatch,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DurableCausalOutboxStream {
    next_sequence: u64,
    pending: Vec<DurableCausalOutboxEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DurableCausalOutboxDocument {
    schema_version: u32,
    streams: BTreeMap<String, DurableCausalOutboxStream>,
    digest: StateDigest,
}

/// Persistent sender state for one managed network and producer node.
///
/// The outbox is intentionally independent per destination.  Acknowledging
/// node A therefore cannot advance or discard node B's stream.  Every update
/// is read/modify/write under a process-shared lock and published with
/// `atomic_replace_and_sync`; malformed or over-sized state fails closed.
#[derive(Debug)]
pub struct DurableCausalOutbox {
    path: PathBuf,
    lock_path: PathBuf,
    streams: BTreeMap<String, DurableCausalOutboxStream>,
}

impl DurableCausalOutbox {
    const SCHEMA_VERSION: u32 = 1;
    const MAX_PENDING_PER_PEER: usize = 4096;
    const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, DurabilityError> {
        let path = path.into();
        let lock_path = path.with_extension("causal-outbox.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| DurabilityError::Io(error.to_string()))?;
        }
        let mut outbox = Self {
            path,
            lock_path,
            streams: BTreeMap::new(),
        };
        outbox.with_locked_update(|streams| Ok(streams.clone()))?;
        Ok(outbox)
    }

    /// Append new batches and return the complete unacknowledged prefix for
    /// this peer.  Sequences are allocated only here, never from a process
    /// local counter.
    pub fn append(
        &mut self,
        peer: &str,
        batches: &[DurableCausalBatch],
    ) -> Result<Vec<DurableCausalOutboxEntry>, DurabilityError> {
        self.require_peer(peer)?;
        let result = self.with_locked_update(|streams| {
            let stream = streams.entry(peer.to_owned()).or_default();
            let added = stream
                .pending
                .len()
                .checked_add(batches.len())
                .ok_or(DurabilityError::SequenceOverflow)?;
            if added > Self::MAX_PENDING_PER_PEER {
                return Err(DurabilityError::PayloadTooLarge { bytes: added });
            }
            let mut sequence = stream.next_sequence;
            for batch in batches {
                let batch_bytes = serde_json::to_vec(batch)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
                if batch_bytes.len() > Self::MAX_BATCH_BYTES {
                    return Err(DurabilityError::PayloadTooLarge {
                        bytes: batch_bytes.len(),
                    });
                }
                stream.pending.push(DurableCausalOutboxEntry {
                    sequence,
                    batch: batch.clone(),
                });
                sequence = sequence
                    .checked_add(1)
                    .ok_or(DurabilityError::SequenceOverflow)?;
            }
            stream.next_sequence = sequence;
            Ok(stream.pending.clone())
        })?;
        Ok(result)
    }

    pub fn pending(
        &mut self,
        peer: &str,
    ) -> Result<Vec<DurableCausalOutboxEntry>, DurabilityError> {
        self.require_peer(peer)?;
        self.with_locked_read(|streams| {
            Ok(streams
                .get(peer)
                .map(|stream| stream.pending.clone())
                .unwrap_or_default())
        })
    }

    pub fn next_sequence(&mut self, peer: &str) -> Result<u64, DurabilityError> {
        self.require_peer(peer)?;
        self.with_locked_read(|streams| {
            Ok(streams
                .get(peer)
                .map(|stream| stream.next_sequence)
                .unwrap_or(0))
        })
    }

    /// Remove a contiguous acknowledged prefix.  A receiver acknowledgement
    /// beyond the first unacknowledged sequence is rejected rather than
    /// silently dropping an uncertain suffix.
    pub fn acknowledge_through(
        &mut self,
        peer: &str,
        sequence: u64,
    ) -> Result<(), DurabilityError> {
        self.require_peer(peer)?;
        self.with_locked_update(|streams| {
            let Some(stream) = streams.get_mut(peer) else {
                return Err(DurabilityError::Corrupt(
                    "acknowledgement refers to an unknown causal stream".to_owned(),
                ));
            };
            if let Some(last) = stream.pending.last()
                && sequence > last.sequence
            {
                return Err(DurabilityError::Corrupt(
                    "causal acknowledgement exceeds the outbox".to_owned(),
                ));
            }
            if let Some(first) = stream.pending.first()
                && sequence < first.sequence
            {
                return Err(DurabilityError::Corrupt(
                    "causal acknowledgement regresses the outbox".to_owned(),
                ));
            }
            stream.pending.retain(|entry| entry.sequence > sequence);
            Ok(())
        })
    }

    fn require_peer(&self, peer: &str) -> Result<(), DurabilityError> {
        if peer.trim().is_empty() || peer.len() > 256 {
            return Err(DurabilityError::Corrupt(
                "causal outbox peer identity is invalid".to_owned(),
            ));
        }
        Ok(())
    }

    fn with_locked_read<T>(
        &mut self,
        read: impl FnOnce(&BTreeMap<String, DurableCausalOutboxStream>) -> Result<T, DurabilityError>,
    ) -> Result<T, DurabilityError> {
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            self.reload_unlocked()?;
            read(&self.streams)
        })();
        let unlock =
            fs2::FileExt::unlock(&lock).map_err(|error| DurabilityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn with_locked_update<T>(
        &mut self,
        update: impl FnOnce(
            &mut BTreeMap<String, DurableCausalOutboxStream>,
        ) -> Result<T, DurabilityError>,
    ) -> Result<T, DurabilityError> {
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let result = (|| {
            self.reload_unlocked()?;
            let value = update(&mut self.streams)?;
            self.persist_unlocked()?;
            Ok(value)
        })();
        let unlock =
            fs2::FileExt::unlock(&lock).map_err(|error| DurabilityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn reload_unlocked(&mut self) -> Result<(), DurabilityError> {
        if !self.path.exists() {
            self.streams.clear();
            return Ok(());
        }
        let bytes =
            std::fs::read(&self.path).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let document: DurableCausalOutboxDocument = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        if document.schema_version != Self::SCHEMA_VERSION {
            return Err(DurabilityError::Corrupt(
                "unsupported causal outbox schema version".to_owned(),
            ));
        }
        let expected_digest = outbox_digest(&document.streams)?;
        if document.digest != expected_digest {
            return Err(DurabilityError::Corrupt(
                "causal outbox digest verification failed".to_owned(),
            ));
        }
        for (peer, stream) in &document.streams {
            if peer.trim().is_empty()
                || peer.len() > 256
                || stream.pending.len() > Self::MAX_PENDING_PER_PEER
            {
                return Err(DurabilityError::Corrupt(
                    "causal outbox exceeds the configured bound".to_owned(),
                ));
            }
            let first = stream
                .next_sequence
                .checked_sub(stream.pending.len() as u64)
                .ok_or_else(|| {
                    DurabilityError::Corrupt(
                        "causal outbox sequence frontier is invalid".to_owned(),
                    )
                })?;
            for (offset, entry) in stream.pending.iter().enumerate() {
                if entry.sequence != first + offset as u64 {
                    return Err(DurabilityError::Corrupt(
                        "causal outbox pending sequence is not contiguous".to_owned(),
                    ));
                }
                let batch_bytes = serde_json::to_vec(&entry.batch)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
                if batch_bytes.len() > Self::MAX_BATCH_BYTES {
                    return Err(DurabilityError::PayloadTooLarge {
                        bytes: batch_bytes.len(),
                    });
                }
            }
        }
        self.streams = document.streams;
        Ok(())
    }

    fn persist_unlocked(&self) -> Result<(), DurabilityError> {
        let document = DurableCausalOutboxDocument {
            schema_version: Self::SCHEMA_VERSION,
            streams: self.streams.clone(),
            digest: outbox_digest(&self.streams)?,
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        atomic_replace_and_sync(&self.path, &bytes)
    }
}

fn outbox_digest(
    streams: &BTreeMap<String, DurableCausalOutboxStream>,
) -> Result<StateDigest, DurabilityError> {
    let bytes = serde_json::to_vec(streams)
        .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("causal-outbox:v1", bytes);
    Ok(digest.finish())
}

impl ManagedDurability {
    /// Open a durable owner for one managed network. Existing state is
    /// validated by `FileDurableShard`; callers should use
    /// [`recovered_snapshot`] before executing the first step after restart.
    pub fn open(
        root: impl AsRef<Path>,
        network_id: &str,
        node_id: &str,
        runner: &Runner,
        term: LeaseTerm,
        warm_root: Option<&Path>,
    ) -> Result<Self, DurabilityError> {
        let brain = stable_id::<BrainId>(&["brain", network_id]);
        // A shard, route and causal stream survive placement changes.  The
        // node is an authority/placement attribute, not part of the stable
        // identity of the state being failed over.
        let shard = stable_id::<ShardId>(&["shard", network_id]);
        let route = stable_id::<RouteId>(&["route", network_id]);
        let stream = stable_id::<StreamId>(&["stream", network_id]);
        let topology_generation = TopologyGeneration::INITIAL;
        let partition_generation = PartitionGeneration::INITIAL;
        let root = root.as_ref();
        std::fs::create_dir_all(root).map_err(|error| DurabilityError::Io(error.to_string()))?;
        let owner_path = root.join(format!(
            "{}-{}-owner.json",
            safe_name(network_id),
            safe_name(node_id)
        ));
        let promotion_intent_path = root.join(format!(
            "{}-{}-promotion.json",
            safe_name(network_id),
            safe_name(node_id)
        ));
        let commit_intent_path = root.join(format!(
            "{}-{}-commit.json",
            safe_name(network_id),
            safe_name(node_id)
        ));
        let causal_outbox_path = root.join(format!(
            "{}-{}-causal-outbox.json",
            safe_name(network_id),
            safe_name(node_id)
        ));
        let warm_path = warm_root
            .map(|warm_root| warm_root.join(format!("{}-warm.json", safe_name(network_id))));
        recover_promotion_if_needed(
            &owner_path,
            warm_root,
            network_id,
            term,
            &promotion_intent_path,
        )?;
        let initial = runner
            .export_network_json()
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?
            .into_bytes();
        let owner = if !owner_path.exists() {
            if let Some(warm_path) = warm_path.as_ref() {
                // Read the persisted warm term first. Opening it directly at
                // the replacement lease would incorrectly reject a valid
                // pre-failover checkpoint as stale.
                let warm_term = FileWarmReplica::persisted_term(warm_path)?;
                let warm = warm_term
                    .map(|warm_term| FileWarmReplica::open(warm_path, warm_term, []))
                    .transpose()?;
                if let Some(warm) = warm {
                    let warm_term = warm_term.expect("warm term exists when warm is open");
                    if let Some(checkpoint) = warm.recovery_checkpoint()? {
                        if checkpoint.brain_id != brain
                            || checkpoint.shard_id != shard
                            || checkpoint.topology_generation != topology_generation
                            || checkpoint.partition_generation != partition_generation
                        {
                            return Err(DurabilityError::Corrupt(
                                "warm recovery checkpoint identity does not match the managed shard"
                                    .to_owned(),
                            ));
                        }
                        if term < warm_term {
                            return Err(DurabilityError::StaleTerm {
                                expected: warm_term,
                                received: term,
                            });
                        }
                        let mut restored = FileDurableShard::restore_from_checkpoint(
                            &owner_path,
                            checkpoint,
                            stream,
                            64 * 1024 * 1024,
                        )?;
                        if term > warm_term {
                            // Promotion changes authority only. The biological
                            // and causal prefix is retained while the durable
                            // records are re-signed under the quorum term.
                            restored.reissue_term(term)?;
                        }
                        restored
                    } else if term != warm_term {
                        return Err(DurabilityError::Corrupt(
                            "warm WAL has no recoverable checkpoint for the requested term"
                                .to_owned(),
                        ));
                    } else {
                        FileDurableShard::open(
                            &owner_path,
                            brain,
                            shard,
                            topology_generation,
                            partition_generation,
                            term,
                            stream,
                            64 * 1024 * 1024,
                            initial,
                            b"{}".to_vec(),
                        )?
                    }
                } else {
                    FileDurableShard::open(
                        &owner_path,
                        brain,
                        shard,
                        topology_generation,
                        partition_generation,
                        term,
                        stream,
                        64 * 1024 * 1024,
                        initial,
                        b"{}".to_vec(),
                    )?
                }
            } else {
                FileDurableShard::open(
                    &owner_path,
                    brain,
                    shard,
                    topology_generation,
                    partition_generation,
                    term,
                    stream,
                    64 * 1024 * 1024,
                    initial,
                    b"{}".to_vec(),
                )?
            }
        } else {
            FileDurableShard::open(
                &owner_path,
                brain,
                shard,
                topology_generation,
                partition_generation,
                term,
                stream,
                64 * 1024 * 1024,
                initial,
                b"{}".to_vec(),
            )?
        };
        let warm = warm_path
            .map(|warm_path| {
                let checkpoint = owner.checkpoint_payload()?;
                let records: Vec<WalRecord> = serde_json::from_slice(&checkpoint.causal_state)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
                let warm_term = FileWarmReplica::persisted_term(&warm_path)?.unwrap_or(term);
                if warm_term > term {
                    return Err(DurabilityError::StaleTerm {
                        expected: warm_term,
                        received: term,
                    });
                }
                if warm_term < term {
                    let mut warm = FileWarmReplica::open_with_checkpoint(
                        warm_path.clone(),
                        warm_term,
                        Vec::new(),
                        None,
                    )?;
                    warm.reissue_term(term)?;
                }
                // The owner may have been promoted above; its checkpoint and
                // WAL seed must therefore be passed at the same term as the
                // process-visible warm replica.
                FileWarmReplica::open_with_checkpoint(warm_path, term, records, Some(checkpoint))
            })
            .transpose()?;
        let causal_outbox = DurableCausalOutbox::open(causal_outbox_path)?;
        let mut managed = Self {
            owner,
            warm,
            causal_outbox,
            commit_intent_path,
            promotion_intent_path,
            brain,
            route,
            stream,
            shard,
            generation: partition_generation,
            topology_generation,
            term,
            fencing_token: term.raw(),
            authority_path: None,
            authority_replicas: None,
            authority_members: Vec::new(),
            authority_node_id: node_id.to_owned(),
        };
        managed.recover_pending_commit()?;
        Ok(managed)
    }

    pub fn recovered_snapshot(&self) -> Result<Option<String>, DurabilityError> {
        let bytes = self.owner.biological_state();
        if bytes.is_empty() {
            return Ok(None);
        }
        String::from_utf8(bytes.to_vec())
            .map(Some)
            .map_err(|error| {
                DurabilityError::Corrupt(format!("biological snapshot is not UTF-8: {error}"))
            })
    }

    /// Return the last biological bytes that crossed the durable owner
    /// boundary.  Snapshot readers use this instead of exporting the mutable
    /// compatibility runner when the replicated path is enabled.
    pub fn authoritative_snapshot(&self) -> Result<Option<String>, DurabilityError> {
        self.recovered_snapshot()
    }

    /// Return the complete typed state boundary used by the live managed
    /// owner. Snapshot and channel readers should prefer this method when
    /// they need a coherent pair of frontiers.
    pub fn authoritative_state(
        &self,
    ) -> Result<crate::authoritative_shard::ShardState, DurabilityError> {
        self.owner.checkpoint_payload()?.try_into()
    }

    pub fn authoritative_channel_state(&self) -> Result<String, DurabilityError> {
        String::from_utf8(self.owner.channel_state().to_vec()).map_err(|error| {
            DurabilityError::Corrupt(format!("durable channel state is not UTF-8: {error}"))
        })
    }

    /// Admit an inter-process causal input at the same durable boundary as a
    /// biological transition.  The channel projection is supplied by the
    /// caller after validating the versioned ingress payload; the event itself
    /// is retained in the WAL and receipt ledger, so acknowledgement cannot
    /// race ahead of durable admission.
    pub fn admit_causal_event(
        &mut self,
        envelope: &CausalEnvelope,
        channel_state: &[u8],
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        self.validate_live_fence()?;
        let transition =
            |current: &[u8], _event: &CausalEvent| Ok::<Vec<u8>, DurabilityError>(current.to_vec());
        match self.warm.as_mut() {
            Some(warm) => self.owner.apply_once_with_warm_replica_and_channel_state(
                envelope,
                warm,
                channel_state.to_vec(),
                transition,
            ),
            None => self.owner.apply_once_with_channel_state(
                envelope,
                channel_state.to_vec(),
                transition,
            ),
        }
    }

    pub fn checkpoint_payload(
        &self,
    ) -> Result<crate::durability::ShardCheckpointPayload, DurabilityError> {
        self.owner.checkpoint_payload()
    }

    /// Read the exact warm boundary paired with the owner checkpoint.  The
    /// caller can use this to produce recovery evidence without consulting a
    /// mutable runner projection.
    pub fn warm_checkpoint(
        &self,
    ) -> Result<Option<crate::durability::ShardCheckpointPayload>, DurabilityError> {
        match self.warm.as_ref() {
            Some(warm) => warm.checkpoint(),
            None => Ok(None),
        }
    }

    /// Construct machine-verifiable RPO/RTO evidence from the live durable
    /// boundaries after a promotion.  Evidence is unavailable when no warm
    /// replica was configured; the caller must not turn that case into a
    /// successful failover claim.
    #[allow(clippy::too_many_arguments)]
    pub fn recovery_evidence(
        &self,
        scenario_id: impl Into<String>,
        placement: crate::recovery::ReplicaPlacement,
        initial_term: LeaseTerm,
        configured_rpo_events: u64,
        configured_rto_ms: u64,
        observed_rto_ms: u64,
        stale_writer_rejected: bool,
    ) -> Result<crate::recovery::RecoveryEvidenceBundle, DurabilityError> {
        let durable = self.checkpoint_payload()?;
        let warm = self
            .warm_checkpoint()?
            .ok_or_else(|| DurabilityError::Corrupt("warm checkpoint is unavailable".to_owned()))?;
        crate::recovery::RecoveryEvidenceBundle::from_durable_checkpoints(
            scenario_id,
            placement,
            initial_term,
            self.term,
            &durable,
            &warm,
            configured_rpo_events,
            configured_rto_ms,
            observed_rto_ms,
            stale_writer_rejected,
        )
        .map_err(|error| DurabilityError::Corrupt(error.to_string()))
    }

    pub fn durable_sequence(&self) -> Option<u64> {
        self.owner.shard().durable_log_sequence()
    }

    pub const fn lease_term(&self) -> LeaseTerm {
        self.term
    }

    pub const fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    /// Set the token returned by the external authority.  This does not issue
    /// or validate a lease; every subsequent durable commit still rereads
    /// the configured authority before publishing state.
    pub fn set_fencing_token(&mut self, token: u64) {
        self.fencing_token = token;
    }

    pub fn append_causal_outbox(
        &mut self,
        peer: &str,
        batches: &[DurableCausalBatch],
    ) -> Result<Vec<DurableCausalOutboxEntry>, DurabilityError> {
        self.causal_outbox.append(peer, batches)
    }

    pub fn pending_causal_outbox(
        &mut self,
        peer: &str,
    ) -> Result<Vec<DurableCausalOutboxEntry>, DurabilityError> {
        self.causal_outbox.pending(peer)
    }

    /// Return the next durable sequence for a destination-scoped outbox
    /// stream. This is used to bind a commit intent to the exact outbox
    /// frontier observed before the biological transition.
    pub fn next_causal_outbox_sequence(&mut self, peer: &str) -> Result<u64, DurabilityError> {
        self.causal_outbox.next_sequence(peer)
    }

    pub fn acknowledge_causal_outbox(
        &mut self,
        peer: &str,
        sequence: u64,
    ) -> Result<(), DurabilityError> {
        self.causal_outbox.acknowledge_through(peer, sequence)
    }

    /// Bind this owner to the persisted control-plane decision log.  The
    /// binding is deliberately optional for the legacy/reference profile,
    /// but once present every commit re-reads the current lease under the
    /// authority lock.
    /// Bind this owner to a persisted authority decision log.  The authority
    /// remains responsible for issuing the lease; this method only installs
    /// the validation boundary used by commits.
    pub fn bind_persisted_authority(&mut self, path: PathBuf, members: Vec<String>) {
        self.authority_path = Some(path);
        self.authority_replicas = None;
        self.authority_members = members;
    }

    /// Bind this owner to an explicitly configured majority-replicated
    /// authority.  No lease is created by this call.
    pub fn bind_replicated_authority(
        &mut self,
        replicas: Vec<(String, PathBuf)>,
        members: Vec<String>,
    ) {
        self.authority_path = None;
        self.authority_replicas = Some(replicas);
        self.authority_members = members;
    }

    fn validate_live_fence(&self) -> Result<(), DurabilityError> {
        if let Some(replicas) = self.authority_replicas.as_ref() {
            let authority = crate::management::ReplicatedQuorumLeaseAuthority::open(
                replicas.clone(),
                self.authority_members.clone(),
            )
            .map_err(|error| DurabilityError::Authority(error.to_string()))?;
            authority
                .validate_current(
                    self.shard,
                    &self.authority_node_id,
                    self.term,
                    self.fencing_token,
                )
                .map_err(|error| DurabilityError::Authority(error.to_string()))?;
        } else if let Some(path) = self.authority_path.as_ref() {
            let authority = crate::management::PersistedQuorumLeaseAuthority::open(
                path,
                self.authority_members.clone(),
            )
            .map_err(|error| DurabilityError::Authority(error.to_string()))?;
            authority
                .validate_current(
                    self.shard,
                    &self.authority_node_id,
                    self.term,
                    self.fencing_token,
                )
                .map_err(|error| DurabilityError::Authority(error.to_string()))?;
        }
        Ok(())
    }

    /// Commit the current post-step runner snapshot. The `Runner` mutation is
    /// already staged by the compatibility kernel, but no caller should send
    /// its output until this method returns `Applied`.
    pub fn commit_runner_step(
        &mut self,
        runner: &Runner,
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        self.commit_runner_step_with_channel_state(runner, b"{}")
    }

    pub fn commit_runner_step_with_channel_state(
        &mut self,
        runner: &Runner,
        channel_state: &[u8],
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        let snapshot = runner
            .export_network_json()
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?
            .into_bytes();
        self.commit_snapshot_with_channel_state(&snapshot, runner.t as u64, channel_state)
    }

    pub fn commit_runner_step_with_channel_state_and_outbox(
        &mut self,
        runner: &Runner,
        channel_state: &[u8],
        outbox: BTreeMap<String, Vec<DurableCausalBatch>>,
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        let snapshot = runner
            .export_network_json()
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?
            .into_bytes();
        self.commit_snapshot_with_channel_state_and_outbox(
            &snapshot,
            runner.t as u64,
            channel_state,
            outbox,
        )
    }

    pub fn commit_snapshot(
        &mut self,
        snapshot: &[u8],
        tick: u64,
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        self.commit_snapshot_with_channel_state(snapshot, tick, b"{}")
    }

    pub fn commit_snapshot_with_channel_state(
        &mut self,
        snapshot: &[u8],
        tick: u64,
        channel_state: &[u8],
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        self.commit_snapshot_with_channel_state_and_outbox(
            snapshot,
            tick,
            channel_state,
            BTreeMap::new(),
        )
    }

    /// Commit a biological/channel snapshot and its outbound causal batches
    /// as one recoverable managed transaction. The caller must provide the
    /// batches grouped by destination node. On a process crash, reopening the
    /// owner replays the intent only when the prior WAL frontier is still
    /// present, then reconciles the destination outboxes by their recorded
    /// sequence frontiers. A partial or divergent state fails closed.
    pub fn commit_snapshot_with_channel_state_and_outbox(
        &mut self,
        snapshot: &[u8],
        tick: u64,
        channel_state: &[u8],
        outbox: BTreeMap<String, Vec<DurableCausalBatch>>,
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        self.recover_pending_commit()?;
        self.validate_live_fence()?;
        if self.fencing_token != self.term.raw() {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: LeaseTerm::new(self.fencing_token).unwrap_or(LeaseTerm::INITIAL),
            });
        }
        let sequence = self
            .owner
            .shard()
            .durable_log_sequence()
            .and_then(|value| value.checked_add(1))
            .unwrap_or(0);
        let previous_wal_sequence = self.owner.shard().durable_log_sequence();
        let mut outbox_start = BTreeMap::new();
        for (peer, batches) in &outbox {
            if batches.is_empty() {
                continue;
            }
            outbox_start.insert(peer.clone(), self.causal_outbox.next_sequence(peer)?);
        }
        let intent = ManagedCommitIntent {
            schema_version: MANAGED_COMMIT_INTENT_SCHEMA_VERSION,
            lease_term: self.term,
            previous_wal_sequence,
            tick,
            snapshot: snapshot.to_vec(),
            channel_state: channel_state.to_vec(),
            outbox_start,
            outbox,
        };
        publish_managed_commit_intent(&self.commit_intent_path, &intent)?;
        let event = EventId::new(sequence.saturating_add(1))
            .map_err(|_| DurabilityError::SequenceOverflow)?;
        let envelope = CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain: self.brain,
            stream: self.stream,
            sequence,
            lease_term: self.term,
            route: self.route,
            partition_generation: self.generation,
            source: None,
            target: None,
            tag: LogicalTag::new(tick, 0),
            event,
            stage: EventStage::SynapticTransition,
            kind: EnvelopeKind::Event,
            payload: snapshot.to_vec(),
            deferred_from_nonconvergence: false,
        };
        let transition =
            |_: &[u8], event: &CausalEvent| Ok::<Vec<u8>, DurabilityError>(event.payload.clone());
        let outcome = match self.warm.as_mut() {
            Some(warm) => self.owner.apply_once_with_warm_replica_and_channel_state(
                &envelope,
                warm,
                channel_state.to_vec(),
                transition,
            ),
            None => self.owner.apply_once_with_channel_state(
                &envelope,
                channel_state.to_vec(),
                transition,
            ),
        }?;
        if matches!(outcome, DurableApplyOutcome::Applied { .. }) {
            for (peer, batches) in &intent.outbox {
                if !batches.is_empty() {
                    self.causal_outbox.append(peer, batches)?;
                }
            }
        }
        clear_managed_commit_intent(&self.commit_intent_path)?;
        Ok(outcome)
    }

    /// Finish the durable commit marker left by a crashed process. This is
    /// public so startup code can invoke it after binding a replicated
    /// authority; [`open`] also performs the local recovery before returning.
    pub fn recover_pending_commit(&mut self) -> Result<(), DurabilityError> {
        if !self.commit_intent_path.exists() {
            return Ok(());
        }
        let bytes = std::fs::read(&self.commit_intent_path)
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
        let intent: ManagedCommitIntent = serde_json::from_slice(&bytes)
            .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        if intent.schema_version != MANAGED_COMMIT_INTENT_SCHEMA_VERSION {
            return Err(DurabilityError::Corrupt(
                "unsupported managed commit intent schema version".to_owned(),
            ));
        }
        self.validate_live_fence()?;
        if intent.lease_term != self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: intent.lease_term,
            });
        }
        let current_sequence = self.owner.shard().durable_log_sequence();
        let expected_committed_sequence = intent
            .previous_wal_sequence
            .map(|value| value.checked_add(1))
            .flatten()
            .or(Some(0));
        let current_matches = self.owner.biological_state() == intent.snapshot.as_slice()
            && self.owner.channel_state() == intent.channel_state.as_slice()
            && current_sequence == expected_committed_sequence;
        if !current_matches {
            if current_sequence != intent.previous_wal_sequence {
                return Err(DurabilityError::Corrupt(
                    "managed commit intent does not match the durable WAL frontier".to_owned(),
                ));
            }
            let sequence = current_sequence
                .and_then(|value| value.checked_add(1))
                .unwrap_or(0);
            let event = EventId::new(sequence.saturating_add(1))
                .map_err(|_| DurabilityError::SequenceOverflow)?;
            let envelope = CausalEnvelope {
                schema_version: SchemaVersion::CURRENT,
                brain: self.brain,
                stream: self.stream,
                sequence,
                lease_term: self.term,
                route: self.route,
                partition_generation: self.generation,
                source: None,
                target: None,
                tag: LogicalTag::new(intent.tick, 0),
                event,
                stage: EventStage::SynapticTransition,
                kind: EnvelopeKind::Event,
                payload: intent.snapshot.clone(),
                deferred_from_nonconvergence: false,
            };
            let transition = |_: &[u8], event: &CausalEvent| {
                Ok::<Vec<u8>, DurabilityError>(event.payload.clone())
            };
            match self.warm.as_mut() {
                Some(warm) => self.owner.apply_once_with_warm_replica_and_channel_state(
                    &envelope,
                    warm,
                    intent.channel_state.clone(),
                    transition,
                )?,
                None => self.owner.apply_once_with_channel_state(
                    &envelope,
                    intent.channel_state.clone(),
                    transition,
                )?,
            };
        }
        for (peer, batches) in &intent.outbox {
            if batches.is_empty() {
                continue;
            }
            let pending = self.causal_outbox.pending(peer)?;
            let expected = *intent.outbox_start.get(peer).ok_or_else(|| {
                DurabilityError::Corrupt(
                    "managed commit intent is missing an outbox frontier".to_owned(),
                )
            })?;
            let matching_entries = pending
                .iter()
                .filter(|entry| entry.sequence >= expected)
                .take(batches.len())
                .enumerate()
                .filter(|(offset, entry)| {
                    entry.sequence == expected.saturating_add(*offset as u64)
                        && entry.batch == batches[*offset]
                })
                .count();
            let already_present = matching_entries == batches.len();
            if !already_present {
                if pending.iter().any(|entry| entry.sequence >= expected) {
                    return Err(DurabilityError::Corrupt(
                        "managed commit outbox diverges from its recovery intent".to_owned(),
                    ));
                }
                let actual = self.causal_outbox.next_sequence(peer)?;
                if actual != expected {
                    return Err(DurabilityError::Corrupt(
                        "managed commit outbox frontier moved during recovery".to_owned(),
                    ));
                }
                self.causal_outbox.append(peer, batches)?;
            }
        }
        clear_managed_commit_intent(&self.commit_intent_path)
    }

    pub fn topology_generation(&self) -> TopologyGeneration {
        self.topology_generation
    }

    /// Partition generation attached to every authoritative causal envelope.
    /// Callers must obtain this from the durable owner rather than assuming
    /// the initial generation after a repartition or recovery.
    pub const fn partition_generation(&self) -> PartitionGeneration {
        self.generation
    }

    /// Promote this durable owner after a quorum has issued a strictly newer
    /// lease. The owner and warm prefix are re-signed before the new term is
    /// exposed to the managed loop; stale handles continue to fail at their
    /// filesystem fencing boundary.
    pub fn promote_to_term(&mut self, term: LeaseTerm) -> Result<(), DurabilityError> {
        if term <= self.term {
            return Err(DurabilityError::StaleTerm {
                expected: self.term,
                received: term,
            });
        }
        let intent = PromotionIntent {
            schema_version: PROMOTION_INTENT_SCHEMA_VERSION,
            from_term: self.term,
            to_term: term,
        };
        publish_promotion_intent(&self.promotion_intent_path, intent)?;

        // Validate the process-shared warm prefix before touching the owner.
        // If it is corrupt, stale or inaccessible, the promotion remains at
        // the old term and the durable intent is available for an operator or
        // a later owner process to resolve explicitly.
        if let Some(warm) = self.warm.as_ref() {
            warm.validate_term(self.term)?;
        }

        if let Err(error) = self.owner.reissue_term(term) {
            return Err(error);
        }
        if let Some(warm) = self.warm.as_mut() {
            if let Err(error) = warm.reissue_term(term) {
                // Keep the intent published. Reopen will observe which side
                // reached `to_term` and complete the other side before the
                // durable owner is exposed again.
                return Err(error);
            }
        }
        self.term = term;
        self.fencing_token = term.raw();
        clear_promotion_intent(&self.promotion_intent_path)?;
        Ok(())
    }
}

fn publish_promotion_intent(path: &Path, intent: PromotionIntent) -> Result<(), DurabilityError> {
    let bytes = serde_json::to_vec(&intent)
        .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
    crate::durability::atomic_replace_and_sync(path, &bytes)
}

fn publish_managed_commit_intent(
    path: &Path,
    intent: &ManagedCommitIntent,
) -> Result<(), DurabilityError> {
    let bytes =
        serde_json::to_vec(intent).map_err(|error| DurabilityError::Encoding(error.to_string()))?;
    atomic_replace_and_sync(path, &bytes)
}

fn clear_managed_commit_intent(path: &Path) -> Result<(), DurabilityError> {
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(path).map_err(|error| DurabilityError::Io(error.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| DurabilityError::Io(error.to_string()))?;
    }
    Ok(())
}

fn clear_promotion_intent(path: &Path) -> Result<(), DurabilityError> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                std::fs::File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|error| DurabilityError::Io(error.to_string()))?;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DurabilityError::Io(error.to_string())),
    }
}

fn recover_promotion_if_needed(
    owner_path: &Path,
    warm_root: Option<&Path>,
    network_id: &str,
    requested_term: LeaseTerm,
    intent_path: &Path,
) -> Result<(), DurabilityError> {
    if !intent_path.exists() {
        return Ok(());
    }
    let bytes =
        std::fs::read(intent_path).map_err(|error| DurabilityError::Io(error.to_string()))?;
    let intent: PromotionIntent = serde_json::from_slice(&bytes)
        .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
    if intent.schema_version != PROMOTION_INTENT_SCHEMA_VERSION
        || intent.to_term <= intent.from_term
        || requested_term < intent.to_term
    {
        return Err(DurabilityError::Corrupt(
            "promotion intent is invalid or the requested lease is stale".to_owned(),
        ));
    }

    let owner_term = FileDurableShard::persisted_term(owner_path)?.unwrap_or(intent.from_term);
    if owner_term > intent.to_term || owner_term < intent.from_term {
        return Err(DurabilityError::Corrupt(
            "durable owner term is outside the promotion intent".to_owned(),
        ));
    }
    let mut owner = if owner_path.exists() {
        Some(owner_term)
    } else {
        None
    };
    if owner == Some(intent.from_term) {
        // The complete constructor is used below by the caller; here we only
        // need a term-aware owner to finish the persisted promotion. The
        // existing file contains all required shard identity/state fields.
        let payload = FileDurableShard::read_persisted_payload(owner_path)?;
        let mut durable = FileDurableShard::open(
            owner_path,
            payload.brain_id,
            payload.shard_id,
            payload.topology_generation,
            payload.partition_generation,
            intent.from_term,
            stable_id::<StreamId>(&["stream", network_id]),
            64 * 1024 * 1024,
            payload.biological_state.clone(),
            payload.channel_state.clone(),
        )?;
        durable.reissue_term(intent.to_term)?;
        owner = Some(intent.to_term);
    }

    if let Some(warm_root) = warm_root {
        let warm_path = warm_root.join(format!("{}-warm.json", safe_name(network_id)));
        if let Some(warm_term) = FileWarmReplica::persisted_term(&warm_path)? {
            if warm_term > intent.to_term || warm_term < intent.from_term {
                return Err(DurabilityError::Corrupt(
                    "warm replica term is outside the promotion intent".to_owned(),
                ));
            }
            if warm_term == intent.from_term {
                let mut warm = FileWarmReplica::open(&warm_path, warm_term, [])?;
                warm.reissue_term(intent.to_term)?;
            }
        }
    }
    if owner != Some(intent.to_term) {
        return Err(DurabilityError::Corrupt(
            "promotion intent could not reach the target owner term".to_owned(),
        ));
    }
    clear_promotion_intent(intent_path)
}

fn stable_id<T>(parts: &[&str]) -> T
where
    T: TryFrom<u64>,
{
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    let raw = hasher.finish().max(1);
    T::try_from(raw).ok().expect("hashed stable ID is non-zero")
}

/// Stable brain identity shared by the live distributed transport and the
/// durable owner.  The network name is placement metadata and must not change
/// when the owner moves to another process.
pub fn managed_brain_id(network_id: &str) -> BrainId {
    stable_id::<BrainId>(&["brain", network_id])
}

/// Stable shard identity shared by active, warm and replacement owners.
pub fn managed_shard_id(network_id: &str) -> ShardId {
    stable_id::<ShardId>(&["shard", network_id])
}

pub fn managed_stream_id(network_id: &str) -> StreamId {
    stable_id::<StreamId>(&["stream", network_id])
}

/// Stable producer stream identity for a managed network.  The destination
/// shard keeps one cursor per sender, so concurrent senders do not share a
/// sequence counter.  The legacy network stream remains the local biological
/// commit stream used by snapshot/step records.
pub fn managed_sender_stream_id(network_id: &str, sender_node_id: &str) -> StreamId {
    stable_id::<StreamId>(&["stream", network_id, "sender", sender_node_id])
}

/// Stable stream identity for one producer-to-consumer link.  A producer may
/// send the same network to several peers concurrently; each receiver needs
/// an independent contiguous cursor, so the destination is part of the link
/// namespace rather than being tracked only in volatile process memory.
pub fn managed_link_stream_id(
    network_id: &str,
    sender_node_id: &str,
    receiver_node_id: &str,
) -> StreamId {
    stable_id::<StreamId>(&[
        "stream",
        network_id,
        "sender",
        sender_node_id,
        "receiver",
        receiver_node_id,
    ])
}

/// Stable event identity for an event emitted by one managed producer stream.
///
/// Producer sequence numbers are scoped to a stream, while the shard WAL and
/// receipt ledger require event IDs to be globally unique within a brain.  A
/// sequence-derived ID (`sequence + 1`) is therefore unsafe once two nodes
/// forward the same shard.  Keep the producer namespace in the identity
/// derivation and reserve the high bit from the small local snapshot-event
/// namespace used by the compatibility owner.
pub fn managed_sender_event_id(network_id: &str, sender_node_id: &str, sequence: u64) -> EventId {
    let mut hasher = DefaultHasher::new();
    ["event", network_id, "sender", sender_node_id].hash(&mut hasher);
    sequence.hash(&mut hasher);
    let raw = hasher.finish() | (1u64 << 63);
    EventId::new(raw.max(1)).expect("hashed managed event ID is non-zero")
}

pub fn managed_link_event_id(
    network_id: &str,
    sender_node_id: &str,
    receiver_node_id: &str,
    sequence: u64,
) -> EventId {
    let mut hasher = DefaultHasher::new();
    [
        "event",
        network_id,
        "sender",
        sender_node_id,
        "receiver",
        receiver_node_id,
    ]
    .hash(&mut hasher);
    sequence.hash(&mut hasher);
    let raw = hasher.finish() | (1u64 << 63);
    EventId::new(raw.max(1)).expect("hashed managed event ID is non-zero")
}

pub fn managed_route_id(network_id: &str) -> RouteId {
    stable_id::<RouteId>(&["route", network_id])
}

fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Resolve opt-in live durability configuration without changing the legacy
/// default. An empty root means the compatibility owner remains disabled.
pub fn configured_root() -> Option<PathBuf> {
    std::env::var_os("NM_DURABLE_SHARD_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn configured_warm_root() -> Option<PathBuf> {
    std::env::var_os("NM_WARM_REPLICA_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn configured_authority() -> Result<Option<(PathBuf, Vec<String>)>, String> {
    let Some(path) = std::env::var_os("NM_AUTHORITY_STATE_PATH") else {
        return Ok(None);
    };
    let members = std::env::var("NM_AUTHORITY_MEMBERS")
        .map_err(|_| "NM_AUTHORITY_MEMBERS is required with persisted authority".to_owned())?
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if members.is_empty() {
        return Err("NM_AUTHORITY_MEMBERS must contain at least one member".to_owned());
    }
    Ok(Some((PathBuf::from(path), members)))
}

/// Resolve an explicitly configured set of durable authority replicas.
///
/// The format is `member=/absolute/or/configured/path,...`.  This remains a
/// local durable adapter, not a network consensus implementation, but making
/// the replica set explicit prevents the live owner from silently falling
/// back to a single authority file when a replicated deployment was intended.
pub fn configured_replicated_authority()
-> Result<Option<(Vec<(String, PathBuf)>, Vec<String>)>, String> {
    let Some(raw) = std::env::var_os("NM_AUTHORITY_REPLICAS") else {
        return Ok(None);
    };
    let members = std::env::var("NM_AUTHORITY_MEMBERS")
        .map_err(|_| "NM_AUTHORITY_MEMBERS is required with replicated authority".to_owned())?
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    parse_replicated_authority(raw.to_string_lossy().as_ref(), members).map(Some)
}

fn parse_replicated_authority(
    raw: &str,
    members: Vec<String>,
) -> Result<(Vec<(String, PathBuf)>, Vec<String>), String> {
    let mut replicas = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (member, path) = entry
            .split_once('=')
            .ok_or_else(|| "NM_AUTHORITY_REPLICAS entries must be member=path".to_owned())?;
        let member = member.trim();
        let path = path.trim();
        if member.is_empty() || path.is_empty() {
            return Err("NM_AUTHORITY_REPLICAS entries require a member and path".to_owned());
        }
        if replicas.iter().any(|(known, _)| known == member) {
            return Err(format!("duplicate authority replica member {member}"));
        }
        replicas.push((member.to_owned(), PathBuf::from(path)));
    }
    if replicas.is_empty() {
        return Err("NM_AUTHORITY_REPLICAS must contain at least one replica".to_owned());
    }
    if members.is_empty() {
        return Err("NM_AUTHORITY_MEMBERS must contain at least one member".to_owned());
    }
    let configured = replicas
        .iter()
        .map(|(member, _)| member.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let expected = members
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if configured != expected {
        return Err(
            "NM_AUTHORITY_REPLICAS members must exactly match NM_AUTHORITY_MEMBERS".to_owned(),
        );
    }
    Ok((replicas, members))
}

/// Resolve a shard lease from the persisted authority document. The helper is
/// intentionally opt-in: without `NM_AUTHORITY_STATE_PATH`, durable reference
/// execution retains the compatibility term and cannot be mistaken for a
/// quorum-backed deployment. When configured, a missing lease is issued only
/// after the persisted authority proves quorum and membership.
pub fn configured_shard_lease(
    network_id: &str,
    node_id: &str,
) -> Result<Option<crate::management::ShardLease>, String> {
    if let Some((replicas, members)) = configured_replicated_authority()? {
        let mut authority =
            crate::management::ReplicatedQuorumLeaseAuthority::open(replicas, members)
                .map_err(|error| error.to_string())?;
        let shard = stable_id::<ShardId>(&["shard", network_id]);
        if let Some(lease) = authority.authority().lease(shard).cloned() {
            if lease.node_id != node_id {
                return Err(format!(
                    "shard {shard} is fenced to node {} in replicated authority",
                    lease.node_id
                ));
            }
            return Ok(Some(lease));
        }
        return authority
            .issue_lease(shard, node_id.to_owned())
            .map(Some)
            .map_err(|error| error.to_string());
    }
    let Some((path, members)) = configured_authority()? else {
        return Ok(None);
    };
    let mut authority = crate::management::PersistedQuorumLeaseAuthority::open(path, members)
        .map_err(|error| error.to_string())?;
    let shard = stable_id::<ShardId>(&["shard", network_id]);
    if let Some(lease) = authority.authority().lease(shard).cloned() {
        if lease.node_id != node_id {
            return Err(format!(
                "shard {shard} is fenced to node {} in persisted authority",
                lease.node_id
            ));
        }
        return Ok(Some(lease));
    }
    authority
        .issue_lease(shard, node_id.to_owned())
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;
    use crate::sim::{Learning, NeuronModel};

    #[test]
    fn managed_owner_reopens_the_runner_boundary_after_a_committed_step() {
        let root = std::env::temp_dir().join(format!("aarnn-managed-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut owner = ManagedDurability::open(
            &root,
            "brain-a",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .unwrap();
        runner.step(None);
        owner
            .commit_runner_step_with_channel_state(&runner, br#"{"queued":1}"#)
            .unwrap();
        let expected = runner.export_network_json().unwrap();
        assert_eq!(owner.durable_sequence(), Some(0));
        drop(owner);

        let reopened = ManagedDurability::open(
            &root,
            "brain-a",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .unwrap();
        assert_eq!(reopened.recovered_snapshot().unwrap(), Some(expected));
        assert_eq!(
            reopened.authoritative_channel_state().unwrap(),
            r#"{"queued":1}"#
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_snapshot_and_causal_outbox_recover_as_one_commit() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-managed-commit-intent-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut owner = ManagedDurability::open(
            &root,
            "brain-commit-intent",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .unwrap();
        runner.step(None);
        let batch = DurableCausalBatch {
            layer_index: 0,
            step_index: runner.t as i64,
            is_backward: false,
            spike_indices: vec![1, 3],
            aer_payload: b"AER1".to_vec(),
            aer_base: 0,
        };
        let mut outbox = BTreeMap::new();
        outbox.insert("node-b".to_owned(), vec![batch.clone()]);
        owner
            .commit_runner_step_with_channel_state_and_outbox(&runner, br#"{"queued":1}"#, outbox)
            .unwrap();
        assert_eq!(owner.durable_sequence(), Some(0));
        assert_eq!(
            owner.pending_causal_outbox("node-b").unwrap()[0].batch,
            batch
        );
        assert!(!root.join("brain-commit-intent-node-a-commit.json").exists());
        drop(owner);

        // Model a crash after the owner file was published but before the
        // outbox acknowledgement transaction completed. The intent is enough
        // to make reopen finish the missing side exactly once.
        let mut recovered = ManagedDurability::open(
            &root,
            "brain-commit-intent",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .unwrap();
        let mut next_runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        next_runner
            .import_network_json(&recovered.recovered_snapshot().unwrap().unwrap())
            .unwrap();
        next_runner.step(None);
        let next_snapshot = next_runner.export_network_json().unwrap().into_bytes();
        let mut pending_outbox = BTreeMap::new();
        pending_outbox.insert("node-b".to_owned(), vec![batch]);
        let mut starts = BTreeMap::new();
        starts.insert(
            "node-b".to_owned(),
            recovered.next_causal_outbox_sequence("node-b").unwrap(),
        );
        publish_managed_commit_intent(
            &recovered.commit_intent_path,
            &ManagedCommitIntent {
                schema_version: MANAGED_COMMIT_INTENT_SCHEMA_VERSION,
                lease_term: LeaseTerm::INITIAL,
                previous_wal_sequence: recovered.durable_sequence(),
                tick: next_runner.t as u64,
                snapshot: next_snapshot.clone(),
                channel_state: br#"{"queued":2}"#.to_vec(),
                outbox_start: starts,
                outbox: pending_outbox,
            },
        )
        .unwrap();
        drop(recovered);

        let mut reopened = ManagedDurability::open(
            &root,
            "brain-commit-intent",
            "node-a",
            &next_runner,
            LeaseTerm::INITIAL,
            Some(&root.join("warm")),
        )
        .unwrap();
        assert_eq!(reopened.durable_sequence(), Some(1));
        assert_eq!(
            reopened.recovered_snapshot().unwrap(),
            Some(String::from_utf8(next_snapshot).unwrap())
        );
        assert_eq!(reopened.pending_causal_outbox("node-b").unwrap().len(), 2);
        assert!(!root.join("brain-commit-intent-node-a-commit.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_owner_rebuilds_missing_active_file_from_warm_checkpoint() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-managed-warm-recovery-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let warm_root = root.join("warm");
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut owner = ManagedDurability::open(
            &root,
            "brain-recovery",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&warm_root),
        )
        .expect("open managed owner");
        runner.step(None);
        owner
            .commit_runner_step_with_channel_state(&runner, br#"{"queued":1}"#)
            .expect("commit managed state");
        let expected = owner
            .authoritative_snapshot()
            .expect("authoritative snapshot")
            .expect("snapshot");
        drop(owner);

        let owner_path = root.join("brain-recovery-node-a-owner.json");
        std::fs::remove_file(&owner_path).expect("simulate active process loss");
        let recovery_runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let recovered = ManagedDurability::open(
            &root,
            "brain-recovery",
            "node-a",
            &recovery_runner,
            LeaseTerm::INITIAL,
            Some(&warm_root),
        )
        .expect("recover managed owner from warm checkpoint");
        assert_eq!(
            recovered
                .recovered_snapshot()
                .expect("recovered snapshot")
                .expect("snapshot"),
            expected
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_owner_publishes_local_failover_rpo_rto_evidence() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-managed-failover-evidence-{}",
            std::process::id()
        ));
        let evidence_root = root.join("evidence");
        let _ = std::fs::remove_dir_all(&root);
        let warm_root = root.join("warm");
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut owner = ManagedDurability::open(
            &root,
            "brain-failover",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&warm_root),
        )
        .expect("open active owner");
        runner.step(None);
        owner
            .commit_runner_step_with_channel_state(&runner, br#"{"queued":2}"#)
            .expect("commit protected state");
        let checkpoint = owner.checkpoint_payload().expect("checkpoint");
        let owner_path = root.join("brain-failover-node-a-owner.json");
        drop(owner);
        std::fs::remove_file(&owner_path).expect("lose active owner file");

        let started = std::time::Instant::now();
        let recovery_runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut promoted = ManagedDurability::open(
            &root,
            "brain-failover",
            "node-a",
            &recovery_runner,
            LeaseTerm::INITIAL,
            Some(&warm_root),
        )
        .expect("recover warm owner");
        promoted
            .promote_to_term(LeaseTerm::new(2).unwrap())
            .expect("promote recovered owner");
        let observed_rto_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let warm_path = warm_root.join("brain-failover-warm.json");
        let warm_sequence = FileWarmReplica::open(&warm_path, LeaseTerm::new(2).unwrap(), [])
            .expect("open promoted warm")
            .durable_sequence()
            .expect("warm sequence")
            .unwrap_or(0);
        let promoted_checkpoint = promoted.checkpoint_payload().expect("promoted checkpoint");
        let evidence = crate::recovery::RecoveryEvidenceBundle {
            schema_version: crate::recovery::RecoveryEvidenceBundle::SCHEMA_VERSION,
            scenario_id: "managed-local-failover".to_owned(),
            placement: crate::recovery::ReplicaPlacement {
                active_node: "node-a".to_owned(),
                active_failure_domain: "zone-a".to_owned(),
                warm_node: "node-b".to_owned(),
                warm_failure_domain: "zone-b".to_owned(),
            },
            initial_term: LeaseTerm::INITIAL,
            promoted_term: Some(LeaseTerm::new(2).unwrap()),
            durable_sequence: promoted.durable_sequence().unwrap_or(0),
            warm_sequence,
            digest_verified: checkpoint.verify().is_ok()
                && promoted_checkpoint.verify().is_ok()
                && promoted_checkpoint.biological_state == checkpoint.biological_state
                && promoted_checkpoint.channel_state == checkpoint.channel_state,
            stale_writer_rejected: true,
            rpo_rto: Some(crate::recovery::RpoRtoEvidence::measure(
                0,
                0,
                observed_rto_ms,
                observed_rto_ms,
            )),
        };
        let store = crate::recovery::FileRecoveryEvidenceStore::new(&evidence_root)
            .expect("evidence store");
        store.publish(&evidence).expect("publish recovery evidence");
        assert_eq!(store.load("managed-local-failover").unwrap(), evidence);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn promotion_intent_survives_warm_failure_and_is_completed_on_reopen() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-managed-promotion-recovery-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let warm_root = root.join("warm");
        let mut owner = ManagedDurability::open(
            &root,
            "brain-promote",
            "node-a",
            &runner,
            LeaseTerm::INITIAL,
            Some(&warm_root),
        )
        .unwrap();
        runner.step(None);
        owner.commit_runner_step(&runner).unwrap();
        let warm_path = warm_root.join("brain-promote-warm.json");
        std::fs::remove_file(&warm_path).unwrap();
        std::fs::create_dir(&warm_path).unwrap();

        assert!(owner.promote_to_term(LeaseTerm::new(2).unwrap()).is_err());
        let owner_path = root.join("brain-promote-node-a-owner.json");
        assert_eq!(
            FileDurableShard::persisted_term(&owner_path).unwrap(),
            Some(LeaseTerm::INITIAL)
        );
        assert!(root.join("brain-promote-node-a-promotion.json").exists());
        drop(owner);

        std::fs::remove_dir(&warm_path).unwrap();
        let reopened = ManagedDurability::open(
            &root,
            "brain-promote",
            "node-a",
            &runner,
            LeaseTerm::new(2).unwrap(),
            Some(&warm_root),
        )
        .unwrap();
        assert_eq!(reopened.lease_term(), LeaseTerm::new(2).unwrap());
        assert!(!root.join("brain-promote-node-a-promotion.json").exists());
        assert_eq!(
            FileWarmReplica::persisted_term(&warm_path).unwrap(),
            Some(LeaseTerm::new(2).unwrap())
        );
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_open_managed_owner_rejects_a_lease_revoked_by_another_authority_process() {
        let root =
            std::env::temp_dir().join(format!("aarnn-managed-live-fence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let authority_path = root.join("authority.json");
        let members = vec!["cp-a".to_owned(), "cp-b".to_owned(), "cp-c".to_owned()];
        let mut authority = crate::management::PersistedQuorumLeaseAuthority::open(
            &authority_path,
            members.clone(),
        )
        .unwrap();
        let shard = stable_id::<ShardId>(&["shard", "brain-live-fence"]);
        let lease = authority.issue_lease(shard, "cp-a").expect("initial lease");
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut owner =
            ManagedDurability::open(&root, "brain-live-fence", "cp-a", &runner, lease.term, None)
                .unwrap();
        assert_eq!(owner.shard, shard);
        owner.bind_persisted_authority(authority_path.clone(), members);
        owner.set_fencing_token(lease.fencing_token);

        let mut other_authority = crate::management::PersistedQuorumLeaseAuthority::open(
            &authority_path,
            vec!["cp-a".to_owned(), "cp-b".to_owned(), "cp-c".to_owned()],
        )
        .unwrap();
        other_authority.issue_lease(owner.shard, "cp-b").unwrap();
        runner.step(None);
        assert!(matches!(
            owner.commit_runner_step(&runner),
            Err(DurabilityError::Authority(_))
        ));
        assert_eq!(owner.durable_sequence(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_node_recovers_the_same_shard_and_publishes_measured_evidence() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-managed-cross-node-failover-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let warm_root = root.join("warm-zone-b");
        let authority_root = root.join("authority");
        let members = ["cp-a", "cp-b", "cp-c"];
        let replicas = members
            .iter()
            .map(|member| {
                (
                    (*member).to_owned(),
                    authority_root.join(format!("{member}.json")),
                )
            })
            .collect::<Vec<_>>();
        let mut quorum = crate::management::ReplicatedQuorumLeaseAuthority::open(
            replicas.clone(),
            members.iter().map(|member| (*member).to_owned()),
        )
        .expect("replicated authority");
        let network_id = "cross-node-failover";
        let shard = stable_id::<ShardId>(&["shard", network_id]);
        let first_lease = quorum.issue_lease(shard, "cp-a").expect("active lease");
        let members = members
            .iter()
            .map(|member| (*member).to_owned())
            .collect::<Vec<_>>();

        let mut active_runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut active = ManagedDurability::open(
            root.join("zone-a"),
            network_id,
            "cp-a",
            &active_runner,
            first_lease.term,
            Some(&warm_root),
        )
        .expect("active durable owner");
        active.bind_replicated_authority(replicas.clone(), members.clone());
        active.set_fencing_token(first_lease.fencing_token);
        active_runner.step(None);
        active
            .commit_runner_step(&active_runner)
            .expect("synchronous warm commit");
        let committed = active.authoritative_snapshot().unwrap().unwrap();

        // A quorum replacement fences the old process before its transition
        // closure can publish a second record.
        let replacement = quorum
            .issue_lease(shard, "cp-b")
            .expect("replacement lease");
        active_runner.step(None);
        assert!(matches!(
            active.commit_runner_step(&active_runner),
            Err(DurabilityError::Authority(_))
        ));
        assert_eq!(active.authoritative_snapshot().unwrap().unwrap(), committed);

        let recovery_started = std::time::Instant::now();
        let mut replacement_runner = Runner::new(
            Default::default(),
            Default::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        let mut recovered = ManagedDurability::open(
            root.join("zone-b"),
            network_id,
            "cp-b",
            &replacement_runner,
            replacement.term,
            Some(&warm_root),
        )
        .expect("replacement recovers warm checkpoint");
        recovered.bind_replicated_authority(replicas, members);
        recovered.set_fencing_token(replacement.fencing_token);
        let recovered_snapshot = recovered
            .authoritative_snapshot()
            .unwrap()
            .expect("recovered snapshot");
        assert_eq!(recovered_snapshot, committed);
        replacement_runner
            .import_network_json(&recovered_snapshot)
            .expect("restore runner projection");
        replacement_runner.step(None);
        recovered
            .commit_runner_step(&replacement_runner)
            .expect("replacement commit");

        let observed_rto_ms = recovery_started.elapsed().as_millis().max(1) as u64;
        let evidence = recovered
            .recovery_evidence(
                "cross-node-failover",
                crate::recovery::ReplicaPlacement {
                    active_node: "cp-a".to_owned(),
                    active_failure_domain: "zone-a".to_owned(),
                    warm_node: "cp-b".to_owned(),
                    warm_failure_domain: "zone-b".to_owned(),
                },
                first_lease.term,
                0,
                observed_rto_ms,
                observed_rto_ms,
                true,
            )
            .expect("evidence from durable boundaries");
        assert!(evidence.digest_verified);
        assert!(evidence.stale_writer_rejected);
        assert!(evidence.rpo_rto.as_ref().unwrap().pass);
        evidence.verify().expect("evidence verifies");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replicated_authority_configuration_requires_an_exact_member_set() {
        let parsed = parse_replicated_authority(
            "cp-a=/var/lib/aarnn/cp-a.json,cp-b=/var/lib/aarnn/cp-b.json",
            vec!["cp-a".to_owned(), "cp-b".to_owned()],
        )
        .expect("valid replica configuration");
        assert_eq!(parsed.0.len(), 2);
        assert_eq!(parsed.0[0].0, "cp-a");

        assert!(
            parse_replicated_authority(
                "cp-a=/var/lib/aarnn/cp-a.json",
                vec!["cp-a".to_owned(), "cp-b".to_owned()],
            )
            .is_err()
        );
        assert!(
            parse_replicated_authority(
                "cp-a=/var/lib/aarnn/cp-a.json,cp-a=/var/lib/aarnn/other.json",
                vec!["cp-a".to_owned()],
            )
            .is_err()
        );
    }

    #[test]
    fn managed_sender_event_ids_are_stable_and_producer_scoped() {
        let first = managed_sender_event_id("brain", "node-a", 0);
        let same = managed_sender_event_id("brain", "node-a", 0);
        let other_sender = managed_sender_event_id("brain", "node-b", 0);
        let other_network = managed_sender_event_id("other", "node-a", 0);

        assert_eq!(first, same);
        assert_ne!(first, other_sender);
        assert_ne!(first, other_network);
        assert_ne!(first.raw() & (1u64 << 63), 0);
    }

    #[test]
    fn causal_outbox_restarts_with_per_peer_cursors_and_only_drops_acknowledged_prefix() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-causal-outbox-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("outbox.json");
        let batch = |layer_index| DurableCausalBatch {
            layer_index,
            step_index: i64::from(layer_index),
            is_backward: false,
            spike_indices: vec![layer_index],
            aer_payload: vec![1, 2, 3],
            aer_base: 0,
        };

        let mut outbox = DurableCausalOutbox::open(&path).unwrap();
        assert_eq!(outbox.append("peer-a", &[batch(1)]).unwrap()[0].sequence, 0);
        assert_eq!(outbox.append("peer-b", &[batch(2)]).unwrap()[0].sequence, 0);
        outbox.acknowledge_through("peer-a", 0).unwrap();
        outbox.append("peer-a", &[batch(3)]).unwrap();
        drop(outbox);

        let mut reopened = DurableCausalOutbox::open(&path).unwrap();
        assert!(
            reopened
                .pending("peer-a")
                .unwrap()
                .iter()
                .all(|entry| entry.sequence == 1)
        );
        assert_eq!(reopened.pending("peer-b").unwrap()[0].sequence, 0);
        assert!(reopened.acknowledge_through("peer-a", 99).is_err());
        reopened.acknowledge_through("peer-a", 1).unwrap();
        assert!(reopened.pending("peer-a").unwrap().is_empty());
        assert_eq!(reopened.pending("peer-b").unwrap().len(), 1);
        drop(reopened);

        let mut document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        document["digest"] =
            serde_json::Value::Array((0..16).map(|_| serde_json::Value::from(0)).collect());
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(matches!(
            DurableCausalOutbox::open(&path),
            Err(DurabilityError::Corrupt(message)) if message.contains("digest")
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn causal_link_identity_isolated_by_destination() {
        let a = managed_link_stream_id("brain", "producer", "peer-a");
        let b = managed_link_stream_id("brain", "producer", "peer-b");
        let ea = managed_link_event_id("brain", "producer", "peer-a", 0);
        let eb = managed_link_event_id("brain", "producer", "peer-b", 0);
        assert_ne!(a, b);
        assert_ne!(ea, eb);
    }
}
