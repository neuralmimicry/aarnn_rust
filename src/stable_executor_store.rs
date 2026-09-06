//! Crash-safe immutable storage for complete stable-executor cuts.
//!
//! The generic checkpoint store already provides atomic publication and
//! no-replace semantics. This adapter adds the whole-fabric invariants that a
//! collection of independent shard files cannot express: every shard belongs
//! to one compiled plan, one logical cut and one lease term. It deliberately
//! stores a reference executor checkpoint rather than silently selecting the
//! legacy `Runner` persistence path.

use crate::deterministic::{
    BrainId, EventId, LeaseTerm, PartitionGeneration, StateDigest, StateDigestBuilder,
    TopologyGeneration,
};
use crate::durability::{CheckpointManifest, FileCheckpointStore};
use crate::shard_executor::{
    SHARD_EXECUTOR_SCHEMA_VERSION, ShardExecutionError, StableShardCheckpoint, StableShardExecutor,
};
use crate::topology_model::CompiledExecutionPlan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use thiserror::Error;

pub const STABLE_EXECUTOR_CHECKPOINT_SCHEMA_VERSION: u32 = 1;
pub const MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum StableExecutorStoreError {
    #[error("stable executor storage failed: {0}")]
    Storage(String),
    #[error("stable executor checkpoint encoding failed: {0}")]
    Encoding(String),
    #[error("stable executor checkpoint set is invalid: {0}")]
    InvalidSet(&'static str),
    #[error(transparent)]
    Execution(#[from] ShardExecutionError),
}

/// The immutable manifest payload for one complete stable-executor cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableExecutorCheckpointSet {
    pub schema_version: u32,
    pub executor_schema_version: u32,
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub lease_term: LeaseTerm,
    pub plan_digest: StateDigest,
    pub fabric_digest: StateDigest,
    pub shards: Vec<StableShardCheckpoint>,
    pub set_digest: StateDigest,
}

impl StableExecutorCheckpointSet {
    pub fn new(
        lease_term: LeaseTerm,
        mut checkpoints: Vec<StableShardCheckpoint>,
    ) -> Result<Self, StableExecutorStoreError> {
        checkpoints.sort_by_key(|checkpoint| checkpoint.shard_id);
        let first = checkpoints
            .first()
            .ok_or(StableExecutorStoreError::InvalidSet(
                "at least one shard checkpoint is required",
            ))?;
        let set = Self {
            schema_version: STABLE_EXECUTOR_CHECKPOINT_SCHEMA_VERSION,
            executor_schema_version: SHARD_EXECUTOR_SCHEMA_VERSION,
            brain_id: first.brain_id,
            topology_generation: first.topology_generation,
            partition_generation: first.partition_generation,
            lease_term,
            plan_digest: first.plan_digest,
            fabric_digest: first.fabric_digest,
            shards: checkpoints,
            set_digest: StateDigest([0; 16]),
        };
        set.seal()
    }

    pub fn seal(mut self) -> Result<Self, StableExecutorStoreError> {
        self.validate_contents(false)?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-executor-checkpoint-set:v1", set_material(&self)?);
        self.set_digest = digest.finish();
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), StableExecutorStoreError> {
        self.validate_contents(true)?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-executor-checkpoint-set:v1", set_material(self)?);
        if digest.finish() != self.set_digest {
            return Err(StableExecutorStoreError::InvalidSet(
                "checkpoint-set digest does not match its contents",
            ));
        }
        Ok(())
    }

    fn validate_contents(&self, require_digest: bool) -> Result<(), StableExecutorStoreError> {
        if self.schema_version != STABLE_EXECUTOR_CHECKPOINT_SCHEMA_VERSION
            || self.executor_schema_version != SHARD_EXECUTOR_SCHEMA_VERSION
            || self.shards.is_empty()
            || (require_digest && self.set_digest == StateDigest([0; 16]))
        {
            return Err(StableExecutorStoreError::InvalidSet(
                "schema, digest or shard contents are invalid",
            ));
        }
        let mut shard_ids = BTreeSet::new();
        for checkpoint in &self.shards {
            if !shard_ids.insert(checkpoint.shard_id)
                || checkpoint.brain_id != self.brain_id
                || checkpoint.topology_generation != self.topology_generation
                || checkpoint.partition_generation != self.partition_generation
                || checkpoint.plan_digest != self.plan_digest
                || checkpoint.fabric_digest != self.fabric_digest
            {
                return Err(StableExecutorStoreError::InvalidSet(
                    "shard checkpoint identities or sibling cut differ",
                ));
            }
            checkpoint.verify()?;
        }
        Ok(())
    }
}

fn set_material(set: &StableExecutorCheckpointSet) -> Result<Vec<u8>, StableExecutorStoreError> {
    let mut material = set.clone();
    // Transfer receivers may reconstruct sibling checkpoints in any order.
    // Canonicalise before hashing so transport order cannot change the
    // identity of an otherwise identical complete cut.
    material
        .shards
        .sort_by_key(|checkpoint| checkpoint.shard_id);
    material.set_digest = StateDigest([0; 16]);
    serde_json::to_vec(&material)
        .map_err(|error| StableExecutorStoreError::Encoding(error.to_string()))
}

/// Immutable filesystem adapter for complete stable-executor checkpoint sets.
#[derive(Debug, Clone)]
pub struct StableExecutorCheckpointStore {
    store: FileCheckpointStore,
}

impl StableExecutorCheckpointStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StableExecutorStoreError> {
        FileCheckpointStore::new(root.into())
            .map(|store| Self { store })
            .map_err(|error| StableExecutorStoreError::Storage(error.to_string()))
    }

    pub fn publish(
        &self,
        checkpoint_id: EventId,
        lease_term: LeaseTerm,
        executor: &StableShardExecutor,
    ) -> Result<CheckpointManifest, StableExecutorStoreError> {
        let set = StableExecutorCheckpointSet::new(lease_term, executor.checkpoint_shards()?)?;
        let partition_generation = set.partition_generation;
        let payload = serde_json::to_vec(&set)
            .map_err(|error| StableExecutorStoreError::Encoding(error.to_string()))?;
        if payload.len() > MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES {
            return Err(StableExecutorStoreError::InvalidSet(
                "checkpoint set exceeds the bounded storage limit",
            ));
        }
        self.store
            .publish(
                checkpoint_id,
                lease_term,
                partition_generation,
                None,
                payload,
            )
            .map_err(|error| StableExecutorStoreError::Storage(error.to_string()))
    }

    /// Publish a previously verified checkpoint-set payload received through
    /// the bounded checkpoint transfer service. The payload is decoded and
    /// fully verified before it reaches the immutable filesystem store. An
    /// identical retry is accepted idempotently; a different payload can
    /// never replace an existing checkpoint ID.
    pub fn publish_payload(
        &self,
        checkpoint_id: EventId,
        lease_term: LeaseTerm,
        partition_generation: PartitionGeneration,
        payload: Vec<u8>,
    ) -> Result<CheckpointManifest, StableExecutorStoreError> {
        if payload.len() > MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES {
            return Err(StableExecutorStoreError::InvalidSet(
                "checkpoint set exceeds the bounded storage limit",
            ));
        }
        let set: StableExecutorCheckpointSet = serde_json::from_slice(&payload)
            .map_err(|error| StableExecutorStoreError::Encoding(error.to_string()))?;
        set.verify()?;
        if set.lease_term != lease_term || set.partition_generation != partition_generation {
            return Err(StableExecutorStoreError::InvalidSet(
                "checkpoint-set identity does not match the transfer manifest",
            ));
        }
        match self.verify(checkpoint_id) {
            Ok(existing) => {
                if existing.manifest.lease_term == lease_term
                    && existing.manifest.partition_generation == partition_generation
                    && existing.payload == payload
                {
                    Ok(existing.manifest)
                } else {
                    Err(StableExecutorStoreError::Storage(
                        "checkpoint ID is already published with different contents".to_owned(),
                    ))
                }
            }
            Err(StableExecutorStoreError::Storage(error))
                if error.contains("checkpoint") && error.contains("not available") =>
            {
                self.store
                    .publish(
                        checkpoint_id,
                        lease_term,
                        partition_generation,
                        None,
                        payload,
                    )
                    .map_err(|error| StableExecutorStoreError::Storage(error.to_string()))
            }
            Err(error) => Err(error),
        }
    }

    /// Verify and return one immutable complete-fabric checkpoint envelope.
    /// Callers use the manifest metadata before restoring the executor so a
    /// deployment manifest cannot bind a checkpoint from another term or
    /// partition generation.
    pub fn verify(
        &self,
        checkpoint_id: EventId,
    ) -> Result<crate::durability::ImmutableCheckpoint, StableExecutorStoreError> {
        self.store
            .verify(checkpoint_id)
            .map_err(|error| StableExecutorStoreError::Storage(error.to_string()))
    }

    pub fn load(
        &self,
        checkpoint_id: EventId,
        brain_id: BrainId,
        plan: CompiledExecutionPlan,
    ) -> Result<StableShardExecutor, StableExecutorStoreError> {
        let checkpoint = self.verify(checkpoint_id)?;
        if checkpoint.payload.len() > MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES {
            return Err(StableExecutorStoreError::InvalidSet(
                "checkpoint payload exceeds the bounded storage limit",
            ));
        }
        let set: StableExecutorCheckpointSet = serde_json::from_slice(&checkpoint.payload)
            .map_err(|error| StableExecutorStoreError::Encoding(error.to_string()))?;
        set.verify()?;
        if set.lease_term != checkpoint.manifest.lease_term
            || set.partition_generation != checkpoint.manifest.partition_generation
        {
            return Err(StableExecutorStoreError::InvalidSet(
                "checkpoint manifest and payload differ",
            ));
        }
        let executor = StableShardExecutor::restore_from_checkpoints(brain_id, plan, set.shards)?;
        if executor.state_digest()? != set.fabric_digest {
            return Err(StableExecutorStoreError::InvalidSet(
                "restored fabric digest differs from the published cut",
            ));
        }
        Ok(executor)
    }
}
