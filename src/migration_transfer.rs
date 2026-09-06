//! Bounded shard checkpoint transfer and cutover evidence.
//!
//! This module is the data movement seam between the durable shard owner and
//! the fenced placement registry. It transfers the canonical [`ShardState`]
//! produced by one immutable boundary; it does not invent a second snapshot
//! representation and it never grants destination authority by itself.
//!
//! Transfer work is deliberately outside the journal lock. A source creates a
//! signed-by-digest manifest, a destination receives bounded frames, and only
//! a fully reconstructed state can produce [`CutoverEvidence`]. The caller
//! must still apply that evidence through the orchestrator registry and issue
//! the destination lease through the control plane.

use crate::authoritative_shard::{AuthoritativeShard, ShardState};
use crate::consistent_cut::ConsistentCut;
use crate::deterministic::{
    EventId, LeaseTerm, LogicalTag, ShardId, StateDigest, StateDigestBuilder, StreamId,
};
use crate::durability::{
    DurabilityError, FileWarmReplica, ReceiptLedger, ShardCheckpointPayload, WalRecord, WarmReplica,
};
use crate::management::{
    PersistedAuthorityError, QuorumError, ReplicatedQuorumLeaseAuthority, ShardLease,
};
use crate::placement_registry::{CutoverEvidence, PlacementRegistryError, ShardCutoverEvidence};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Bumped when the source placement generation became part of the manifest.
pub const SHARD_TRANSFER_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_TRANSFER_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_TRANSFER_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CATCH_UP_RECORDS: usize = 65_536;

fn cursor_digest(domain: &str, bytes: &[u8]) -> StateDigest {
    let mut digest = StateDigestBuilder::default();
    digest.add_domain(domain, bytes);
    digest.finish()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardTransferManifest {
    pub schema_version: u32,
    pub transfer_id: EventId,
    pub source_node: String,
    pub brain_id: crate::deterministic::BrainId,
    pub shard_id: ShardId,
    pub source_term: LeaseTerm,
    pub topology_generation: crate::deterministic::TopologyGeneration,
    pub partition_generation: crate::deterministic::PartitionGeneration,
    pub cut_tag: LogicalTag,
    /// Digest of the placement generation from which this checkpoint was
    /// read. This binds the transfer to the control-plane owner record.
    pub source_plan_digest: StateDigest,
    pub checkpoint_digest: StateDigest,
    pub payload_digest: StateDigest,
    pub durable_wal_sequence: Option<u64>,
    pub total_bytes: u64,
    pub frame_bytes: u32,
    pub frame_count: u32,
    pub manifest_digest: StateDigest,
}

impl ShardTransferManifest {
    fn seal(mut self) -> Result<Self, MigrationTransferError> {
        let bytes = serde_json::to_vec(&ManifestMaterial {
            schema_version: self.schema_version,
            transfer_id: self.transfer_id,
            source_node: &self.source_node,
            brain_id: self.brain_id,
            shard_id: self.shard_id,
            source_term: self.source_term,
            topology_generation: self.topology_generation,
            partition_generation: self.partition_generation,
            cut_tag: self.cut_tag,
            source_plan_digest: self.source_plan_digest,
            checkpoint_digest: self.checkpoint_digest,
            payload_digest: self.payload_digest,
            durable_wal_sequence: self.durable_wal_sequence,
            total_bytes: self.total_bytes,
            frame_bytes: self.frame_bytes,
            frame_count: self.frame_count,
        })
        .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-transfer-manifest:v2", bytes);
        self.manifest_digest = digest.finish();
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), MigrationTransferError> {
        if self.schema_version != SHARD_TRANSFER_SCHEMA_VERSION
            || self.source_node.trim().is_empty()
            || self.source_term.raw() == 0
            || self.cut_tag.microstep != 0
            || self.source_plan_digest == StateDigest([0; 16])
            || self.checkpoint_digest == StateDigest([0; 16])
            || self.payload_digest == StateDigest([0; 16])
            || self.total_bytes == 0
            || self.frame_bytes == 0
            || self.frame_bytes as usize > MAX_TRANSFER_FRAME_BYTES
            || self.frame_count == 0
        {
            return Err(MigrationTransferError::InvalidManifest(
                "manifest identity, digest, boundary or frame limits are invalid",
            ));
        }
        let expected_count = self
            .total_bytes
            .checked_add(self.frame_bytes as u64 - 1)
            .ok_or(MigrationTransferError::SizeOverflow)?
            / self.frame_bytes as u64;
        if expected_count != u64::from(self.frame_count) {
            return Err(MigrationTransferError::InvalidManifest(
                "frame count does not cover the declared payload",
            ));
        }
        let expected = self.clone().seal()?.manifest_digest;
        if expected != self.manifest_digest {
            return Err(MigrationTransferError::DigestMismatch { kind: "manifest" });
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ManifestMaterial<'a> {
    schema_version: u32,
    transfer_id: EventId,
    source_node: &'a str,
    brain_id: crate::deterministic::BrainId,
    shard_id: ShardId,
    source_term: LeaseTerm,
    topology_generation: crate::deterministic::TopologyGeneration,
    partition_generation: crate::deterministic::PartitionGeneration,
    cut_tag: LogicalTag,
    source_plan_digest: StateDigest,
    checkpoint_digest: StateDigest,
    payload_digest: StateDigest,
    durable_wal_sequence: Option<u64>,
    total_bytes: u64,
    frame_bytes: u32,
    frame_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardTransferFrame {
    pub schema_version: u32,
    pub transfer_id: EventId,
    pub manifest_digest: StateDigest,
    pub frame_index: u32,
    pub frame_count: u32,
    pub payload: Vec<u8>,
    pub frame_digest: StateDigest,
}

impl ShardTransferFrame {
    fn seal(mut self) -> Result<Self, MigrationTransferError> {
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-transfer-frame:v2", frame_material(&self));
        self.frame_digest = digest.finish();
        Ok(self)
    }

    fn verify(&self, manifest: &ShardTransferManifest) -> Result<(), MigrationTransferError> {
        if self.schema_version != SHARD_TRANSFER_SCHEMA_VERSION
            || self.transfer_id != manifest.transfer_id
            || self.manifest_digest != manifest.manifest_digest
            || self.frame_count != manifest.frame_count
            || self.frame_index >= self.frame_count
            || self.payload.is_empty()
            || self.payload.len() > manifest.frame_bytes as usize
        {
            return Err(MigrationTransferError::InvalidFrame);
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-transfer-frame:v2", frame_material(self));
        if digest.finish() != self.frame_digest {
            return Err(MigrationTransferError::DigestMismatch { kind: "frame" });
        }
        if self.frame_index + 1 < self.frame_count
            && self.payload.len() != manifest.frame_bytes as usize
        {
            return Err(MigrationTransferError::InvalidFrame);
        }
        Ok(())
    }
}

fn frame_material(frame: &ShardTransferFrame) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + 4 + frame.payload.len());
    bytes.extend_from_slice(&frame.transfer_id.raw().to_be_bytes());
    bytes.extend_from_slice(&frame.manifest_digest.0);
    bytes.extend_from_slice(&frame.frame_index.to_be_bytes());
    bytes.extend_from_slice(&frame.frame_count.to_be_bytes());
    bytes.extend_from_slice(&(frame.payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&frame.payload);
    bytes
}

#[derive(Debug, Clone)]
pub struct ShardTransferSource {
    manifest: ShardTransferManifest,
    payload: Vec<u8>,
}

impl ShardTransferSource {
    pub fn prepare(
        transfer_id: EventId,
        source_node: impl Into<String>,
        state: &ShardState,
        cut: &ConsistentCut,
        source_plan_digest: StateDigest,
        frame_bytes: usize,
    ) -> Result<Self, MigrationTransferError> {
        cut.verify().map_err(MigrationTransferError::Cut)?;
        state.verify().map_err(MigrationTransferError::Durability)?;
        if frame_bytes == 0 || frame_bytes > MAX_TRANSFER_FRAME_BYTES {
            return Err(MigrationTransferError::InvalidFrameSize);
        }
        if state.committed_tag < cut.safe_tag {
            return Err(MigrationTransferError::CutStateAhead {
                state: state.committed_tag,
                cut: cut.safe_tag,
            });
        }
        let source_node = source_node.into();
        if source_node.trim().is_empty()
            || !cut
                .participants
                .iter()
                .any(|participant| participant.participant == source_node)
        {
            return Err(MigrationTransferError::MissingSourceParticipant);
        }
        let payload = serde_json::to_vec(state)
            .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        if payload.is_empty() || payload.len() > ShardCheckpointPayload::MAX_BYTES {
            return Err(MigrationTransferError::PayloadTooLarge(payload.len()));
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-transfer-payload:v1", &payload);
        let payload_digest = digest.finish();
        let frame_count = payload
            .len()
            .checked_add(frame_bytes - 1)
            .ok_or(MigrationTransferError::SizeOverflow)?
            / frame_bytes;
        let frame_count =
            u32::try_from(frame_count).map_err(|_| MigrationTransferError::SizeOverflow)?;
        let manifest = ShardTransferManifest {
            schema_version: SHARD_TRANSFER_SCHEMA_VERSION,
            transfer_id,
            source_node,
            brain_id: state.brain_id,
            shard_id: state.shard_id,
            source_term: state.lease_term,
            topology_generation: state.topology_generation,
            partition_generation: state.partition_generation,
            cut_tag: cut.safe_tag,
            source_plan_digest,
            checkpoint_digest: state.state_digest,
            payload_digest,
            durable_wal_sequence: state.durable_wal_sequence,
            total_bytes: payload.len() as u64,
            frame_bytes: u32::try_from(frame_bytes)
                .map_err(|_| MigrationTransferError::SizeOverflow)?,
            frame_count,
            manifest_digest: StateDigest([0; 16]),
        }
        .seal()?;
        manifest.verify()?;
        Ok(Self { manifest, payload })
    }

    pub fn manifest(&self) -> &ShardTransferManifest {
        &self.manifest
    }

    pub fn frames(&self) -> Result<Vec<ShardTransferFrame>, MigrationTransferError> {
        self.manifest.verify()?;
        self.payload
            .chunks(self.manifest.frame_bytes as usize)
            .enumerate()
            .map(|(index, payload)| {
                ShardTransferFrame {
                    schema_version: SHARD_TRANSFER_SCHEMA_VERSION,
                    transfer_id: self.manifest.transfer_id,
                    manifest_digest: self.manifest.manifest_digest,
                    frame_index: u32::try_from(index)
                        .map_err(|_| MigrationTransferError::SizeOverflow)?,
                    frame_count: self.manifest.frame_count,
                    payload: payload.to_vec(),
                    frame_digest: StateDigest([0; 16]),
                }
                .seal()
            })
            .collect()
    }

    /// Reconstruct the verified immutable source state locally. This is used
    /// by the source-side drain coordinator to compare the post-drain state
    /// with the exact checkpoint that was transferred, without exposing the
    /// payload buffer or bypassing frame verification.
    pub fn imported_state(&self) -> Result<ImportedShardState, MigrationTransferError> {
        let mut receiver = ShardTransferReceiver::new(self.manifest.clone())?;
        for frame in self.frames()? {
            receiver.accept(frame)?;
        }
        receiver.finalize()
    }
}

/// A bounded post-checkpoint WAL tail. The source keeps its original lease
/// term in the evidence; destination application retags each reconstructed
/// envelope under the already-fenced destination term before committing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardCatchUpBatch {
    pub schema_version: u32,
    pub transfer_id: EventId,
    pub manifest_digest: StateDigest,
    pub source_term: LeaseTerm,
    pub base_wal_sequence: Option<u64>,
    pub source_state_digest: StateDigest,
    pub records: Vec<WalRecord>,
    pub batch_digest: StateDigest,
}

#[derive(Serialize)]
struct CatchUpMaterial<'a> {
    schema_version: u32,
    transfer_id: EventId,
    manifest_digest: StateDigest,
    source_term: LeaseTerm,
    base_wal_sequence: Option<u64>,
    source_state_digest: StateDigest,
    records: &'a [WalRecord],
}

impl ShardCatchUpBatch {
    fn seal(mut self) -> Result<Self, MigrationTransferError> {
        let bytes = serde_json::to_vec(&CatchUpMaterial {
            schema_version: self.schema_version,
            transfer_id: self.transfer_id,
            manifest_digest: self.manifest_digest,
            source_term: self.source_term,
            base_wal_sequence: self.base_wal_sequence,
            source_state_digest: self.source_state_digest,
            records: &self.records,
        })
        .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-catch-up:v1", bytes);
        self.batch_digest = digest.finish();
        Ok(self)
    }

    pub fn verify(&self, manifest: &ShardTransferManifest) -> Result<(), MigrationTransferError> {
        if self.schema_version != SHARD_TRANSFER_SCHEMA_VERSION
            || self.transfer_id != manifest.transfer_id
            || self.manifest_digest != manifest.manifest_digest
            || self.source_term != manifest.source_term
            || self.base_wal_sequence != manifest.durable_wal_sequence
            || self.source_state_digest == StateDigest([0; 16])
            || self.records.len() > MAX_CATCH_UP_RECORDS
        {
            return Err(MigrationTransferError::InvalidCatchUp(
                "catch-up identity, term, base sequence or bounds are invalid",
            ));
        }
        let expected_first = self
            .base_wal_sequence
            .map_or(0, |sequence| sequence.saturating_add(1));
        if self.records.iter().enumerate().any(|(index, record)| {
            record.lease_term != self.source_term
                || record.sequence != expected_first.saturating_add(index as u64)
                || record.replay.is_none()
                || record.channel_state.is_none()
        }) {
            return Err(MigrationTransferError::InvalidCatchUp(
                "catch-up records are not a contiguous replayable source tail",
            ));
        }
        let expected = self.clone().seal()?.batch_digest;
        if expected != self.batch_digest {
            return Err(MigrationTransferError::DigestMismatch { kind: "catch-up" });
        }
        Ok(())
    }

    /// Apply the tail to a destination actor that was restored from this
    /// transfer's checkpoint. Every record passes through the actor's normal
    /// receiver, biological transition, WAL and durable warm-replica commit.
    pub fn apply_to_authoritative<F, E>(
        &self,
        manifest: &ShardTransferManifest,
        shard: &mut AuthoritativeShard,
        destination_term: LeaseTerm,
        mut transition: F,
    ) -> Result<usize, MigrationTransferError>
    where
        F: FnMut(&[u8], &crate::causal::CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        self.verify(manifest)?;
        if shard.term() != destination_term
            || shard.shard_id() != manifest.shard_id
            || shard.topology_generation() != manifest.topology_generation
            || shard.partition_generation() != manifest.partition_generation
        {
            return Err(MigrationTransferError::CatchUpTargetMismatch);
        }
        let current = shard.state().map_err(MigrationTransferError::Durability)?;
        if current.durable_wal_sequence != self.base_wal_sequence {
            return Err(MigrationTransferError::CatchUpBaseMismatch {
                expected: self.base_wal_sequence,
                received: current.durable_wal_sequence,
            });
        }
        for record in &self.records {
            let envelope = record
                .replay_envelope(current.brain_id, destination_term)
                .map_err(MigrationTransferError::Durability)?;
            shard
                .apply(
                    &envelope,
                    record
                        .channel_state
                        .clone()
                        .ok_or(MigrationTransferError::InvalidCatchUp(
                            "catch-up record has no channel boundary",
                        ))?,
                    |state, event| transition(state, event),
                )
                .map_err(MigrationTransferError::Durability)?;
        }
        Ok(self.records.len())
    }

    /// Apply a source-drain tail and verify the resulting destination actor
    /// against the source's final drained state. The source state contains
    /// the complete stable-executor checkpoint, while the WAL tail contains
    /// only the causal records needed to advance the durable actor boundary.
    /// The final biological bytes are therefore installed on the last
    /// replayed record through the same durable apply path; no checkpoint is
    /// published by bypassing the actor WAL or warm replica.
    pub fn apply_to_authoritative_with_final_state(
        &self,
        manifest: &ShardTransferManifest,
        shard: &mut AuthoritativeShard,
        destination_term: LeaseTerm,
        final_state: &ShardState,
    ) -> Result<usize, MigrationTransferError> {
        self.verify(manifest)?;
        final_state
            .verify()
            .map_err(MigrationTransferError::Durability)?;
        if final_state.brain_id != manifest.brain_id
            || final_state.shard_id != manifest.shard_id
            || final_state.lease_term != manifest.source_term
            || final_state.topology_generation != manifest.topology_generation
            || final_state.partition_generation != manifest.partition_generation
        {
            return Err(MigrationTransferError::StateMismatch);
        }
        if self.records.is_empty() {
            let current = shard.state().map_err(MigrationTransferError::Durability)?;
            if current.biological_state != final_state.biological_state
                || current.channel_state != final_state.channel_state
                || current.committed_tag != final_state.committed_tag
                || current.applied_tag != final_state.applied_tag
                || current.durable_wal_sequence != final_state.durable_wal_sequence
            {
                return Err(MigrationTransferError::StateMismatch);
            }
            return Ok(0);
        }

        let final_biological_state = final_state.biological_state.clone();
        let record_count = self.records.len();
        let mut applied = 0usize;
        self.apply_to_authoritative(manifest, shard, destination_term, |current, _event| {
            applied = applied.saturating_add(1);
            if applied == record_count {
                Ok::<Vec<u8>, MigrationTransferError>(final_biological_state.clone())
            } else {
                Ok::<Vec<u8>, MigrationTransferError>(current.to_vec())
            }
        })?;
        let current = shard.state().map_err(MigrationTransferError::Durability)?;
        if current.biological_state != final_state.biological_state
            || current.channel_state != final_state.channel_state
            || current.committed_tag != final_state.committed_tag
            || current.applied_tag != final_state.applied_tag
            || current.durable_wal_sequence != final_state.durable_wal_sequence
        {
            return Err(MigrationTransferError::StateMismatch);
        }
        Ok(record_count)
    }
}

#[derive(Debug, Clone)]
pub struct ShardTransferReceiver {
    manifest: ShardTransferManifest,
    frames: BTreeMap<u32, ShardTransferFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedShardState {
    pub manifest: ShardTransferManifest,
    pub state: ShardState,
}

impl ShardTransferReceiver {
    pub fn new(manifest: ShardTransferManifest) -> Result<Self, MigrationTransferError> {
        manifest.verify()?;
        Ok(Self {
            manifest,
            frames: BTreeMap::new(),
        })
    }

    pub fn accept(&mut self, frame: ShardTransferFrame) -> Result<(), MigrationTransferError> {
        frame.verify(&self.manifest)?;
        if let Some(existing) = self.frames.get(&frame.frame_index) {
            if existing != &frame {
                return Err(MigrationTransferError::ConflictingDuplicate {
                    frame: frame.frame_index,
                });
            }
            return Ok(());
        }
        self.frames.insert(frame.frame_index, frame);
        Ok(())
    }

    pub fn received_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn finalize(self) -> Result<ImportedShardState, MigrationTransferError> {
        if self.frames.len() != self.manifest.frame_count as usize {
            return Err(MigrationTransferError::Incomplete {
                received: self.frames.len(),
                expected: self.manifest.frame_count as usize,
            });
        }
        let mut payload = Vec::with_capacity(self.manifest.total_bytes as usize);
        for index in 0..self.manifest.frame_count {
            let frame = self
                .frames
                .get(&index)
                .ok_or(MigrationTransferError::MissingFrame { frame: index })?;
            payload.extend_from_slice(&frame.payload);
        }
        if payload.len() as u64 != self.manifest.total_bytes {
            return Err(MigrationTransferError::PayloadLengthMismatch);
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("shard-transfer-payload:v1", &payload);
        if digest.finish() != self.manifest.payload_digest {
            return Err(MigrationTransferError::DigestMismatch { kind: "payload" });
        }
        let state: ShardState = serde_json::from_slice(&payload)
            .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        state.verify().map_err(MigrationTransferError::Durability)?;
        if state.brain_id != self.manifest.brain_id
            || state.shard_id != self.manifest.shard_id
            || state.lease_term != self.manifest.source_term
            || state.topology_generation != self.manifest.topology_generation
            || state.partition_generation != self.manifest.partition_generation
            || state.committed_tag < self.manifest.cut_tag
            || state.state_digest != self.manifest.checkpoint_digest
        {
            return Err(MigrationTransferError::StateMismatch);
        }
        Ok(ImportedShardState {
            manifest: self.manifest,
            state,
        })
    }
}

impl ImportedShardState {
    /// Re-seal the complete checkpoint under the newly issued term. This is a
    /// state transformation, not authority publication; the caller must still
    /// use the returned digest as cutover evidence and fence the source.
    pub fn promote(
        self,
        destination_term: LeaseTerm,
    ) -> Result<ShardState, MigrationTransferError> {
        if destination_term <= self.manifest.source_term {
            return Err(MigrationTransferError::InvalidDestinationTerm);
        }
        let mut state = self.state;
        // Current durable shards encode their WAL as `Vec<WalRecord>`. Keep
        // the compatibility reader for older opaque checkpoint fixtures: an
        // opaque causal blob cannot be re-signed, but it can still be copied
        // and the destination checkpoint remains explicitly versioned.
        if let Ok(records) = serde_json::from_slice::<Vec<WalRecord>>(&state.causal_state) {
            let records = crate::durability::reterm_records(&records, destination_term)
                .map_err(MigrationTransferError::Durability)?;
            state.causal_state = serde_json::to_vec(&records)
                .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        }
        let mut receipts = ReceiptLedger::default();
        for mut receipt in state.receipts.receipts().cloned().collect::<Vec<_>>() {
            receipt.lease_term = destination_term;
            receipts
                .record(receipt)
                .map_err(MigrationTransferError::Durability)?;
        }
        state.receipts = receipts;
        let mut peripheral_state = state.peripheral_state.clone();
        peripheral_state
            .reterm(destination_term)
            .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        let checkpoint = ShardCheckpointPayload::new(
            state.brain_id,
            state.shard_id,
            state.topology_generation,
            state.partition_generation,
            destination_term,
            state.committed_tag,
            state.applied_tag,
            state.durable_wal_sequence,
            state.biological_state,
            state.causal_state,
            state.channel_state,
            state.receipts,
        )
        .with_peripheral_state(peripheral_state)
        .map_err(MigrationTransferError::Durability)?
        .seal()
        .map_err(MigrationTransferError::Durability)?;
        checkpoint
            .try_into()
            .map_err(MigrationTransferError::Durability)
    }

    /// Materialise the verified destination state as a durable authoritative
    /// shard. The warm checkpoint is published before the actor is reopened,
    /// so a crash during this handoff leaves a recoverable destination
    /// boundary. Control-plane lease publication and source fencing remain
    /// separate caller responsibilities.
    pub fn promote_into_authoritative(
        self,
        owner_path: impl Into<std::path::PathBuf>,
        warm_path: impl Into<std::path::PathBuf>,
        destination_term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<AuthoritativeShard, MigrationTransferError> {
        let state = self.promote(destination_term)?;
        let records: Vec<WalRecord> = serde_json::from_slice(&state.causal_state)
            .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        let checkpoint = ShardCheckpointPayload::new(
            state.brain_id,
            state.shard_id,
            state.topology_generation,
            state.partition_generation,
            state.lease_term,
            state.committed_tag,
            state.applied_tag,
            state.durable_wal_sequence,
            state.biological_state,
            state.causal_state,
            state.channel_state,
            state.receipts,
        )
        .with_peripheral_state(state.peripheral_state)
        .map_err(MigrationTransferError::Durability)?
        .seal()
        .map_err(MigrationTransferError::Durability)?;
        let warm_path = warm_path.into();
        FileWarmReplica::open_with_checkpoint(
            &warm_path,
            destination_term,
            records,
            Some(checkpoint),
        )
        .map_err(MigrationTransferError::Durability)?;
        AuthoritativeShard::recover_from_warm(
            owner_path,
            warm_path,
            destination_term,
            stream_id,
            max_payload,
        )
        .map_err(MigrationTransferError::Durability)
    }

    /// Perform the authority-sensitive part of promotion as one fenced
    /// operation. It proves that the source currently owns the shard, issues
    /// a newer lease through the majority-backed authority, verifies that the
    /// old source lease is fenced, materialises the destination from the
    /// verified checkpoint, and binds the new actor to the same authority
    /// before returning it to the caller.
    ///
    /// The placement registry is intentionally still a separate atomic
    /// publication step: callers must publish the returned cutover evidence
    /// only after this method succeeds. If materialisation fails after lease
    /// issuance, the lease is revoked so the shard has no ambiguous active
    /// owner and recovery can resume from the journal.
    pub fn promote_with_quorum(
        self,
        owner_path: impl Into<std::path::PathBuf>,
        warm_path: impl Into<std::path::PathBuf>,
        authority: &mut ReplicatedQuorumLeaseAuthority,
        destination_node: impl Into<String>,
        operation_id: u64,
        source_fencing_token: u64,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<QuorumPromotedShard, MigrationTransferError> {
        let destination_node = destination_node.into();
        let source_node = self.manifest.source_node.clone();
        let shard_id = self.manifest.shard_id;
        authority
            .validate_current(
                shard_id,
                &source_node,
                self.manifest.source_term,
                source_fencing_token,
            )
            .map_err(MigrationTransferError::authority)?;
        let lease = authority
            .issue_lease(shard_id, destination_node)
            .map_err(MigrationTransferError::authority)?;
        if lease.term <= self.manifest.source_term {
            let _ = authority.revoke(shard_id);
            return Err(MigrationTransferError::InvalidDestinationTerm);
        }
        let cutover = match self.cutover_evidence(operation_id, lease.term) {
            Ok(cutover) => cutover,
            Err(error) => {
                let _ = authority.revoke(shard_id);
                return Err(error);
            }
        };
        match authority.validate_current(
            shard_id,
            &source_node,
            self.manifest.source_term,
            source_fencing_token,
        ) {
            Err(PersistedAuthorityError::Quorum(QuorumError::Fenced { .. })) => {}
            Ok(()) => {
                let _ = authority.revoke(shard_id);
                return Err(MigrationTransferError::SourceStillAuthoritative);
            }
            Err(error) => {
                let _ = authority.revoke(shard_id);
                return Err(MigrationTransferError::authority(error));
            }
        }
        let mut shard = match self.promote_into_authoritative(
            owner_path,
            warm_path,
            lease.term,
            stream_id,
            max_payload,
        ) {
            Ok(shard) => shard,
            Err(error) => {
                let _ = authority.revoke(shard_id);
                return Err(error);
            }
        };
        let (replicas, members) = authority.replica_binding();
        shard.bind_replicated_fencing(
            replicas,
            members,
            lease.node_id.clone(),
            lease.fencing_token,
        );
        authority
            .validate_current(
                lease.shard_id,
                &lease.node_id,
                lease.term,
                lease.fencing_token,
            )
            .map_err(MigrationTransferError::authority)?;
        Ok(QuorumPromotedShard {
            lease,
            shard,
            cutover,
        })
    }

    /// Compare a later immutable source boundary with the transferred
    /// checkpoint and return only the contiguous WAL suffix. The source
    /// remains authoritative while this evidence is produced.
    pub fn catch_up_from(
        &self,
        latest_state: &ShardState,
    ) -> Result<ShardCatchUpBatch, MigrationTransferError> {
        self.state
            .verify()
            .map_err(MigrationTransferError::Durability)?;
        latest_state
            .verify()
            .map_err(MigrationTransferError::Durability)?;
        if latest_state.brain_id != self.manifest.brain_id
            || latest_state.shard_id != self.manifest.shard_id
            || latest_state.lease_term != self.manifest.source_term
            || latest_state.topology_generation != self.manifest.topology_generation
            || latest_state.partition_generation != self.manifest.partition_generation
        {
            return Err(MigrationTransferError::StateMismatch);
        }
        let base_records: Vec<WalRecord> = serde_json::from_slice(&self.state.causal_state)
            .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        let latest_records: Vec<WalRecord> = serde_json::from_slice(&latest_state.causal_state)
            .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?;
        WarmReplica::from_records(self.manifest.source_term, base_records.clone())
            .map_err(MigrationTransferError::Durability)?;
        WarmReplica::from_records(self.manifest.source_term, latest_records.clone())
            .map_err(MigrationTransferError::Durability)?;
        if self.state.durable_wal_sequence != base_records.last().map(|record| record.sequence)
            || latest_records.len() < base_records.len()
            || latest_records[..base_records.len()] != base_records
        {
            return Err(MigrationTransferError::CatchUpBaseMismatch {
                expected: self.state.durable_wal_sequence,
                received: latest_state.durable_wal_sequence,
            });
        }
        let records = latest_records[base_records.len()..].to_vec();
        ShardCatchUpBatch {
            schema_version: SHARD_TRANSFER_SCHEMA_VERSION,
            transfer_id: self.manifest.transfer_id,
            manifest_digest: self.manifest.manifest_digest,
            source_term: self.manifest.source_term,
            base_wal_sequence: self.state.durable_wal_sequence,
            source_state_digest: latest_state.state_digest,
            records,
            batch_digest: StateDigest([0; 16]),
        }
        .seal()
    }

    pub fn cutover_evidence(
        &self,
        operation_id: u64,
        destination_term: LeaseTerm,
    ) -> Result<CutoverEvidence, MigrationTransferError> {
        let operation_id =
            EventId::new(operation_id).map_err(|_| MigrationTransferError::InvalidOperationId)?;
        let evidence = CutoverEvidence {
            operation_id,
            source_plan_digest: self.manifest.source_plan_digest,
            cut_tag: self.manifest.cut_tag,
            destination_term,
            shards: BTreeMap::from([(
                self.manifest.shard_id,
                ShardCutoverEvidence {
                    source_node: self.manifest.source_node.clone(),
                    source_term: self.manifest.source_term,
                    checkpoint_digest: self.manifest.checkpoint_digest,
                    caught_up: true,
                    route_cursor_digest: cursor_digest(
                        "migration-route-cursor:v1",
                        &serde_json::to_vec(&(
                            &self.state.causal_state,
                            &self.state.channel_state,
                            &self.state.peripheral_state.admissions,
                        ))
                        .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?,
                    ),
                    effect_cursor_digest: cursor_digest(
                        "migration-effect-cursor:v1",
                        &serde_json::to_vec(&(
                            &self.state.receipts,
                            &self.state.peripheral_state.effects,
                        ))
                        .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?,
                    ),
                },
            )]),
        };
        evidence
            .verify()
            .map_err(MigrationTransferError::Placement)?;
        Ok(evidence)
    }

    /// Produce cutover evidence for the final source-drain state rather than
    /// the initial checkpoint. This prevents a destination that replayed a
    /// valid WAL suffix from being published with stale route/effect cursors.
    pub fn cutover_evidence_after_catch_up(
        &self,
        final_state: &ShardState,
        operation_id: u64,
        destination_term: LeaseTerm,
    ) -> Result<CutoverEvidence, MigrationTransferError> {
        final_state
            .verify()
            .map_err(MigrationTransferError::Durability)?;
        if final_state.brain_id != self.manifest.brain_id
            || final_state.shard_id != self.manifest.shard_id
            || final_state.lease_term != self.manifest.source_term
            || final_state.topology_generation != self.manifest.topology_generation
            || final_state.partition_generation != self.manifest.partition_generation
            || final_state.committed_tag < self.manifest.cut_tag
            || final_state.committed_tag.microstep != 0
        {
            return Err(MigrationTransferError::StateMismatch);
        }
        let operation_id =
            EventId::new(operation_id).map_err(|_| MigrationTransferError::InvalidOperationId)?;
        let evidence = CutoverEvidence {
            operation_id,
            source_plan_digest: self.manifest.source_plan_digest,
            cut_tag: final_state.committed_tag,
            destination_term,
            shards: BTreeMap::from([(
                self.manifest.shard_id,
                ShardCutoverEvidence {
                    source_node: self.manifest.source_node.clone(),
                    source_term: self.manifest.source_term,
                    checkpoint_digest: final_state.state_digest,
                    caught_up: true,
                    route_cursor_digest: cursor_digest(
                        "migration-route-cursor:v1",
                        &serde_json::to_vec(&(
                            &final_state.causal_state,
                            &final_state.channel_state,
                            &final_state.peripheral_state.admissions,
                        ))
                        .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?,
                    ),
                    effect_cursor_digest: cursor_digest(
                        "migration-effect-cursor:v1",
                        &serde_json::to_vec(&(
                            &final_state.receipts,
                            &final_state.peripheral_state.effects,
                        ))
                        .map_err(|error| MigrationTransferError::Encoding(error.to_string()))?,
                    ),
                },
            )]),
        };
        evidence
            .verify()
            .map_err(MigrationTransferError::Placement)?;
        Ok(evidence)
    }
}

/// Destination actor and the quorum lease that fences its source. The lease
/// is returned as evidence for the placement-registry cutover transaction.
#[derive(Debug)]
pub struct QuorumPromotedShard {
    pub lease: ShardLease,
    pub shard: AuthoritativeShard,
    pub cutover: CutoverEvidence,
}

#[derive(Debug, Error)]
pub enum MigrationTransferError {
    #[error("consistent cut is invalid: {0}")]
    Cut(#[from] crate::consistent_cut::ConsistentCutError),
    #[error("durable shard state is invalid: {0}")]
    Durability(#[from] DurabilityError),
    #[error("placement cutover evidence is invalid: {0}")]
    Placement(#[from] PlacementRegistryError),
    #[error("migration transfer encoding failed: {0}")]
    Encoding(String),
    #[error("migration transfer manifest is invalid: {0}")]
    InvalidManifest(&'static str),
    #[error("migration transfer frame is invalid")]
    InvalidFrame,
    #[error("migration transfer frame size is invalid")]
    InvalidFrameSize,
    #[error("migration catch-up batch is invalid: {0}")]
    InvalidCatchUp(&'static str),
    #[error("migration transfer payload is too large: {0} bytes")]
    PayloadTooLarge(usize),
    #[error("migration transfer payload length does not match its manifest")]
    PayloadLengthMismatch,
    #[error("migration transfer {kind} digest does not match")]
    DigestMismatch { kind: &'static str },
    #[error("migration transfer contains a conflicting duplicate frame {frame}")]
    ConflictingDuplicate { frame: u32 },
    #[error("migration transfer is incomplete: received {received} of {expected} frames")]
    Incomplete { received: usize, expected: usize },
    #[error("migration transfer is missing frame {frame}")]
    MissingFrame { frame: u32 },
    #[error("migration transfer state is inconsistent with its manifest")]
    StateMismatch,
    #[error("migration transfer state is ahead of its consistent cut: state={state}, cut={cut}")]
    CutStateAhead { state: LogicalTag, cut: LogicalTag },
    #[error("migration transfer source is absent from the consistent cut")]
    MissingSourceParticipant,
    #[error("migration transfer destination term must be newer than the source")]
    InvalidDestinationTerm,
    #[error("migration catch-up target does not match the transferred shard")]
    CatchUpTargetMismatch,
    #[error(
        "migration catch-up base sequence mismatch: expected {expected:?}, received {received:?}"
    )]
    CatchUpBaseMismatch {
        expected: Option<u64>,
        received: Option<u64>,
    },
    #[error("migration operation ID is invalid")]
    InvalidOperationId,
    #[error("migration transfer size overflow")]
    SizeOverflow,
    #[error("migration transfer authority operation failed: {0}")]
    Authority(String),
    #[error("migration transfer source lease remained authoritative after promotion")]
    SourceStillAuthoritative,
}

impl MigrationTransferError {
    fn authority(error: impl std::fmt::Display) -> Self {
        Self::Authority(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoritative_shard::ShardState;
    use crate::consistent_cut::{ChannelMarker, ConsistentCutCoordinator, ParticipantReport};
    use crate::deterministic::{
        BrainId, EventId, LogicalTag, PartitionGeneration, StreamId, TopologyGeneration,
    };
    use crate::durability::ReceiptLedger;
    use crate::peripheral::{
        PERIPHERAL_CURSOR_SCHEMA_VERSION, PeripheralAdmissionCursor, PeripheralCursorState,
        PeripheralEffectCursor, PeripheralSample,
    };

    fn state() -> ShardState {
        let brain_id = BrainId::new(7).unwrap();
        let shard_id = ShardId::new(11).unwrap();
        let checkpoint = ShardCheckpointPayload::new(
            brain_id,
            shard_id,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            LogicalTag::ZERO,
            LogicalTag::ZERO,
            None,
            b"biology".to_vec(),
            b"wal".to_vec(),
            b"channel".to_vec(),
            ReceiptLedger::default(),
        )
        .seal()
        .unwrap();
        checkpoint.try_into().unwrap()
    }

    fn cut() -> ConsistentCut {
        let mut coordinator = ConsistentCutCoordinator::begin(
            1,
            ["source".to_owned()],
            ["source->destination".to_owned()],
        )
        .unwrap();
        coordinator
            .record_report(ParticipantReport {
                participant: "source".to_owned(),
                local_frontier: LogicalTag::ZERO,
                queued_min: None,
                in_flight_min: None,
                activity_epoch: 1,
            })
            .unwrap();
        coordinator
            .record_marker(ChannelMarker::new("source->destination", 1, None, b"channel").unwrap())
            .unwrap();
        coordinator.finalise().unwrap()
    }

    #[test]
    fn transfer_reassembles_out_of_order_duplicate_frames_and_promotes() {
        let source = ShardTransferSource::prepare(
            EventId::new(1).unwrap(),
            "source",
            &state(),
            &cut(),
            StateDigest([3; 16]),
            7,
        )
        .unwrap();
        let frames = source.frames().unwrap();
        let mut receiver = ShardTransferReceiver::new(source.manifest().clone()).unwrap();
        for frame in frames.iter().rev() {
            receiver.accept(frame.clone()).unwrap();
            receiver.accept(frame.clone()).unwrap();
        }
        let imported = receiver.finalize().unwrap();
        let evidence = imported
            .cutover_evidence(9, LeaseTerm::new(2).unwrap())
            .unwrap();
        assert!(evidence.verify().is_ok());
        let promoted = imported.promote(LeaseTerm::new(2).unwrap()).unwrap();
        assert_eq!(promoted.lease_term, LeaseTerm::new(2).unwrap());
        assert_eq!(promoted.biological_state, b"biology");
    }

    #[test]
    fn transfer_rejects_missing_or_conflicting_frames_and_tampered_payload() {
        let source = ShardTransferSource::prepare(
            EventId::new(1).unwrap(),
            "source",
            &state(),
            &cut(),
            StateDigest([3; 16]),
            7,
        )
        .unwrap();
        let frames = source.frames().unwrap();
        let mut receiver = ShardTransferReceiver::new(source.manifest().clone()).unwrap();
        receiver.accept(frames[0].clone()).unwrap();
        assert!(matches!(
            receiver.finalize(),
            Err(MigrationTransferError::Incomplete { .. })
        ));
        let mut tampered = frames[0].clone();
        tampered.payload[0] ^= 1;
        assert!(matches!(
            ShardTransferReceiver::new(source.manifest().clone())
                .unwrap()
                .accept(tampered),
            Err(MigrationTransferError::DigestMismatch { kind: "frame" })
        ));
    }

    #[test]
    fn promotion_preserves_explicit_peripheral_cursors_and_refences_effects() {
        let channel = StreamId::new(21).unwrap();
        let effect = EventId::new(22).unwrap();
        let cursor = PeripheralCursorState {
            schema_version: PERIPHERAL_CURSOR_SCHEMA_VERSION,
            admissions: vec![PeripheralAdmissionCursor {
                channel,
                device_epoch: 3,
                mapping_version: 4,
                last_capture_sequence: Some(8),
                admitted_sequences: vec![7, 8],
                queued_samples: vec![PeripheralSample {
                    channel,
                    device_epoch: 3,
                    capture_sequence: 8,
                    capture_time_ns: 100,
                    mapping_version: 4,
                    uncertainty_ns: 2,
                    biological_tag: LogicalTag::new(5, 0),
                    payload: vec![1, 2, 3],
                }],
            }],
            effects: vec![PeripheralEffectCursor {
                channel,
                device_epoch: 3,
                lease_term: LeaseTerm::INITIAL,
                armed: true,
                accepted_effect_ids: vec![effect],
            }],
        };
        cursor.verify().unwrap();
        let base = ShardCheckpointPayload::new(
            BrainId::new(7).unwrap(),
            ShardId::new(11).unwrap(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            LogicalTag::ZERO,
            LogicalTag::ZERO,
            None,
            b"biology".to_vec(),
            b"[]".to_vec(),
            b"channel".to_vec(),
            ReceiptLedger::default(),
        )
        .with_peripheral_state(cursor.clone())
        .unwrap()
        .seal()
        .unwrap();
        let source_state: ShardState = base.try_into().unwrap();
        let source = ShardTransferSource::prepare(
            EventId::new(1).unwrap(),
            "source",
            &source_state,
            &cut(),
            StateDigest([3; 16]),
            7,
        )
        .unwrap();
        let mut receiver = ShardTransferReceiver::new(source.manifest().clone()).unwrap();
        for frame in source.frames().unwrap() {
            receiver.accept(frame).unwrap();
        }
        let promoted = receiver
            .finalize()
            .unwrap()
            .promote(LeaseTerm::new(2).unwrap())
            .unwrap();
        assert_eq!(promoted.peripheral_state.admissions, cursor.admissions);
        assert_eq!(
            promoted.peripheral_state.effects[0].accepted_effect_ids,
            vec![effect]
        );
        assert_eq!(
            promoted.peripheral_state.effects[0].lease_term,
            LeaseTerm::new(2).unwrap()
        );
        assert!(promoted.peripheral_state.effects[0].armed);
    }
}
