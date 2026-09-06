//! Brain-wide migration barrier for coordinating independent shard cutovers.
//!
//! A shard transfer can finish while another shard is still catching up. This
//! module keeps those per-shard facts explicit and prevents the brain-wide
//! operation from committing until every affected shard has a fenced
//! destination, a committed logical cut, and verified route/effect cursor
//! boundaries. It is transport- and storage-neutral so the durable migration
//! journal or a replicated orchestrator can persist the value after each
//! transition.

use crate::deterministic::{
    BrainId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest, TopologyGeneration,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MIGRATION_GROUP_SCHEMA_VERSION: u32 = 2;

/// Versioned input used by management clients when they want the journal to
/// enforce a brain-wide barrier. The operation ID is assigned by the journal,
/// so callers cannot forge or guess the identity of a future operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationGroupSpec {
    pub brain_id: BrainId,
    pub leader_term: LeaseTerm,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub shard_ids: Vec<ShardId>,
}

/// One fenced, resource-versioned update to a persisted migration barrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationGroupUpdate {
    pub operation_id: u64,
    pub observed_leader_term: LeaseTerm,
    pub expected_resource_version: u64,
    pub action: MigrationGroupAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationGroupAction {
    BeginTransfer {
        shard_id: ShardId,
    },
    MarkCaughtUp {
        shard_id: ShardId,
        checkpoint_digest: StateDigest,
        cut_tag: LogicalTag,
        destination_term: LeaseTerm,
        route_cursor_digest: StateDigest,
        effect_cursor_digest: StateDigest,
    },
    MarkFenced {
        shard_id: ShardId,
        destination_term: LeaseTerm,
    },
    MarkPublished {
        shard_id: ShardId,
    },
    Commit,
    Abort,
}

impl MigrationGroupSpec {
    pub fn build(&self, operation_id: u64) -> Result<MigrationGroup, MigrationGroupError> {
        MigrationGroup::new(
            operation_id,
            self.brain_id,
            self.leader_term,
            self.topology_generation,
            self.partition_generation,
            self.shard_ids.iter().copied(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationGroupPhase {
    Prepared,
    Transferring,
    CutoverReady,
    Committed,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardMigrationPhase {
    Planned,
    Transferring,
    CaughtUp,
    Fenced,
    Published,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardMigrationStatus {
    pub shard_id: ShardId,
    pub phase: ShardMigrationPhase,
    pub checkpoint_digest: Option<StateDigest>,
    pub cut_tag: Option<LogicalTag>,
    pub destination_term: Option<LeaseTerm>,
    /// Digest of the route/credit/dedupe cursor boundary at the cut.
    pub route_cursor_digest: Option<StateDigest>,
    /// Digest of the committed external-effect cursor boundary at the cut.
    pub effect_cursor_digest: Option<StateDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationGroup {
    pub schema_version: u32,
    pub operation_id: u64,
    pub brain_id: BrainId,
    pub leader_term: LeaseTerm,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub phase: MigrationGroupPhase,
    pub resource_version: u64,
    pub shards: BTreeMap<ShardId, ShardMigrationStatus>,
    pub audit: Vec<MigrationGroupAudit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationGroupAudit {
    pub sequence: u64,
    pub resource_version: u64,
    pub operation_id: u64,
    pub leader_term: LeaseTerm,
    pub phase: MigrationGroupPhase,
    pub digest: String,
    pub previous_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrationGroupError {
    #[error("migration group must contain at least one shard")]
    EmptyGroup,
    #[error("migration group contains duplicate shard {0}")]
    DuplicateShard(ShardId),
    #[error("migration group operation ID must be non-zero")]
    InvalidOperationId,
    #[error("migration group uses a stale leader term: expected {expected}, received {received}")]
    StaleLeader {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("migration group is terminal")]
    Terminal,
    #[error("shard {0} is not part of the migration group")]
    UnknownShard(ShardId),
    #[error("shard {shard} cannot move from {from:?} to {to:?}")]
    InvalidShardTransition {
        shard: ShardId,
        from: ShardMigrationPhase,
        to: ShardMigrationPhase,
    },
    #[error("shard {0} lacks complete cutover cursor evidence")]
    MissingCursorEvidence(ShardId),
    #[error("shard {0} has no verified destination term")]
    MissingDestinationTerm(ShardId),
    #[error("migration group cannot commit while shard {0} is incomplete")]
    IncompleteShard(ShardId),
    #[error("migration group resource version exhausted")]
    ResourceVersionExhausted,
    #[error("migration group audit chain is invalid")]
    InvalidAudit,
    #[error("migration group schema version is unsupported")]
    UnsupportedSchema,
}

impl MigrationGroup {
    pub fn new<I>(
        operation_id: u64,
        brain_id: BrainId,
        leader_term: LeaseTerm,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        shard_ids: I,
    ) -> Result<Self, MigrationGroupError>
    where
        I: IntoIterator<Item = ShardId>,
    {
        if operation_id == 0 {
            return Err(MigrationGroupError::InvalidOperationId);
        }
        let mut shards = BTreeMap::new();
        for shard_id in shard_ids {
            if shards
                .insert(
                    shard_id,
                    ShardMigrationStatus {
                        shard_id,
                        phase: ShardMigrationPhase::Planned,
                        checkpoint_digest: None,
                        cut_tag: None,
                        destination_term: None,
                        route_cursor_digest: None,
                        effect_cursor_digest: None,
                    },
                )
                .is_some()
            {
                return Err(MigrationGroupError::DuplicateShard(shard_id));
            }
        }
        if shards.is_empty() {
            return Err(MigrationGroupError::EmptyGroup);
        }
        let mut group = Self {
            schema_version: MIGRATION_GROUP_SCHEMA_VERSION,
            operation_id,
            brain_id,
            leader_term,
            topology_generation,
            partition_generation,
            phase: MigrationGroupPhase::Prepared,
            resource_version: 0,
            shards,
            audit: Vec::new(),
        };
        group.record_audit();
        group.verify()?;
        Ok(group)
    }

    pub fn shard(&self, shard_id: ShardId) -> Result<&ShardMigrationStatus, MigrationGroupError> {
        self.shards
            .get(&shard_id)
            .ok_or(MigrationGroupError::UnknownShard(shard_id))
    }

    pub fn begin_transfer(
        &mut self,
        shard_id: ShardId,
        observed_leader_term: LeaseTerm,
    ) -> Result<(), MigrationGroupError> {
        self.mutate(
            shard_id,
            observed_leader_term,
            ShardMigrationPhase::Transferring,
            |status| {
                status.phase = ShardMigrationPhase::Transferring;
                Ok(())
            },
        )
    }

    pub fn mark_caught_up(
        &mut self,
        shard_id: ShardId,
        observed_leader_term: LeaseTerm,
        checkpoint_digest: StateDigest,
        cut_tag: LogicalTag,
        destination_term: LeaseTerm,
        route_cursor_digest: StateDigest,
        effect_cursor_digest: StateDigest,
    ) -> Result<(), MigrationGroupError> {
        if checkpoint_digest == StateDigest([0; 16])
            || route_cursor_digest == StateDigest([0; 16])
            || effect_cursor_digest == StateDigest([0; 16])
        {
            return Err(MigrationGroupError::MissingCursorEvidence(shard_id));
        }
        if destination_term <= self.leader_term || cut_tag.microstep != 0 {
            return Err(MigrationGroupError::MissingDestinationTerm(shard_id));
        }
        self.mutate(
            shard_id,
            observed_leader_term,
            ShardMigrationPhase::CaughtUp,
            |status| {
                status.phase = ShardMigrationPhase::CaughtUp;
                status.checkpoint_digest = Some(checkpoint_digest);
                status.cut_tag = Some(cut_tag);
                status.destination_term = Some(destination_term);
                status.route_cursor_digest = Some(route_cursor_digest);
                status.effect_cursor_digest = Some(effect_cursor_digest);
                Ok(())
            },
        )
    }

    pub fn mark_fenced(
        &mut self,
        shard_id: ShardId,
        observed_leader_term: LeaseTerm,
        destination_term: LeaseTerm,
    ) -> Result<(), MigrationGroupError> {
        self.mutate(
            shard_id,
            observed_leader_term,
            ShardMigrationPhase::Fenced,
            |status| {
                if status.destination_term != Some(destination_term) {
                    return Err(MigrationGroupError::MissingDestinationTerm(shard_id));
                }
                status.phase = ShardMigrationPhase::Fenced;
                Ok(())
            },
        )
    }

    pub fn mark_published(
        &mut self,
        shard_id: ShardId,
        observed_leader_term: LeaseTerm,
    ) -> Result<(), MigrationGroupError> {
        self.mutate(
            shard_id,
            observed_leader_term,
            ShardMigrationPhase::Published,
            |status| {
                if status.checkpoint_digest.is_none()
                    || status.cut_tag.is_none()
                    || status.destination_term.is_none()
                    || status.route_cursor_digest.is_none()
                    || status.effect_cursor_digest.is_none()
                {
                    return Err(MigrationGroupError::MissingCursorEvidence(shard_id));
                }
                status.phase = ShardMigrationPhase::Published;
                Ok(())
            },
        )
    }

    pub fn commit(&mut self, observed_leader_term: LeaseTerm) -> Result<(), MigrationGroupError> {
        self.validate_term(observed_leader_term)?;
        if self.phase == MigrationGroupPhase::Committed
            || self.phase == MigrationGroupPhase::Aborted
        {
            return Err(MigrationGroupError::Terminal);
        }
        if let Some(status) = self
            .shards
            .values()
            .find(|status| status.phase != ShardMigrationPhase::Published)
        {
            return Err(MigrationGroupError::IncompleteShard(status.shard_id));
        }
        self.next_version()?;
        self.resource_version += 1;
        self.phase = MigrationGroupPhase::Committed;
        self.record_audit();
        self.verify()
    }

    pub fn abort(&mut self, observed_leader_term: LeaseTerm) -> Result<(), MigrationGroupError> {
        self.validate_term(observed_leader_term)?;
        if matches!(
            self.phase,
            MigrationGroupPhase::Committed | MigrationGroupPhase::Aborted
        ) {
            return Err(MigrationGroupError::Terminal);
        }
        self.next_version()?;
        self.resource_version += 1;
        self.phase = MigrationGroupPhase::Aborted;
        for status in self.shards.values_mut() {
            if status.phase != ShardMigrationPhase::Published {
                status.phase = ShardMigrationPhase::Aborted;
            }
        }
        self.record_audit();
        self.verify()
    }

    /// Rebind an in-flight barrier to a newer control-plane term after
    /// takeover. The old leader cannot resume because future updates validate
    /// the new term.
    pub fn take_over(&mut self, new_term: LeaseTerm) -> Result<(), MigrationGroupError> {
        if new_term <= self.leader_term {
            return Err(MigrationGroupError::StaleLeader {
                expected: self.leader_term,
                received: new_term,
            });
        }
        if matches!(
            self.phase,
            MigrationGroupPhase::Committed | MigrationGroupPhase::Aborted
        ) {
            return Err(MigrationGroupError::Terminal);
        }
        let next_resource_version = self.next_version()?;
        self.leader_term = new_term;
        self.resource_version = next_resource_version;
        self.record_audit();
        self.verify()
    }

    fn mutate<F>(
        &mut self,
        shard_id: ShardId,
        observed_leader_term: LeaseTerm,
        target: ShardMigrationPhase,
        update: F,
    ) -> Result<(), MigrationGroupError>
    where
        F: FnOnce(&mut ShardMigrationStatus) -> Result<(), MigrationGroupError>,
    {
        self.validate_term(observed_leader_term)?;
        if matches!(
            self.phase,
            MigrationGroupPhase::Committed | MigrationGroupPhase::Aborted
        ) {
            return Err(MigrationGroupError::Terminal);
        }
        let next_resource_version = self.next_version()?;
        let status = self
            .shards
            .get_mut(&shard_id)
            .ok_or(MigrationGroupError::UnknownShard(shard_id))?;
        let valid = matches!(
            (status.phase, target),
            (
                ShardMigrationPhase::Planned,
                ShardMigrationPhase::Transferring
            ) | (
                ShardMigrationPhase::Transferring,
                ShardMigrationPhase::CaughtUp
            ) | (ShardMigrationPhase::CaughtUp, ShardMigrationPhase::Fenced)
                | (ShardMigrationPhase::Fenced, ShardMigrationPhase::Published)
        );
        if !valid {
            return Err(MigrationGroupError::InvalidShardTransition {
                shard: shard_id,
                from: status.phase,
                to: target,
            });
        }
        update(status)?;
        self.resource_version = next_resource_version;
        if self
            .shards
            .values()
            .any(|status| status.phase != ShardMigrationPhase::Planned)
        {
            self.phase = MigrationGroupPhase::Transferring;
        }
        if self
            .shards
            .values()
            .all(|status| status.phase == ShardMigrationPhase::Published)
        {
            self.phase = MigrationGroupPhase::CutoverReady;
        }
        self.record_audit();
        self.verify()
    }

    fn validate_term(&self, received: LeaseTerm) -> Result<(), MigrationGroupError> {
        if received != self.leader_term {
            return Err(MigrationGroupError::StaleLeader {
                expected: self.leader_term,
                received,
            });
        }
        Ok(())
    }

    fn next_version(&self) -> Result<u64, MigrationGroupError> {
        self.resource_version
            .checked_add(1)
            .ok_or(MigrationGroupError::ResourceVersionExhausted)
    }

    fn record_audit(&mut self) {
        let previous_digest = self
            .audit
            .last()
            .map(|record| record.digest.clone())
            .unwrap_or_else(|| "0".repeat(64));
        let sequence = self.audit.len() as u64 + 1;
        let material = serde_json::json!({
            "sequence": sequence,
            "resource_version": self.resource_version,
            "operation_id": self.operation_id,
            "leader_term": self.leader_term,
            "phase": self.phase,
            "previous_digest": previous_digest,
        });
        let digest = hex::encode(Sha256::digest(
            serde_json::to_vec(&material).expect("group audit material is serializable"),
        ));
        self.audit.push(MigrationGroupAudit {
            sequence,
            resource_version: self.resource_version,
            operation_id: self.operation_id,
            leader_term: self.leader_term,
            phase: self.phase,
            digest,
            previous_digest,
        });
    }

    pub fn verify(&self) -> Result<(), MigrationGroupError> {
        if self.schema_version != MIGRATION_GROUP_SCHEMA_VERSION
            || self.operation_id == 0
            || self.leader_term.raw() == 0
            || self.shards.is_empty()
            || self.audit.is_empty()
        {
            return Err(MigrationGroupError::UnsupportedSchema);
        }
        let mut previous_digest = "0".repeat(64);
        for (index, record) in self.audit.iter().enumerate() {
            if record.sequence != index as u64 + 1
                || record.resource_version > self.resource_version
                || record.leader_term.raw() == 0
                || record.operation_id != self.operation_id
                || record.previous_digest != previous_digest
                || record.digest.len() != 64
            {
                return Err(MigrationGroupError::InvalidAudit);
            }
            let material = serde_json::json!({
                "sequence": record.sequence,
                "resource_version": record.resource_version,
                "operation_id": record.operation_id,
                "leader_term": record.leader_term,
                "phase": record.phase,
                "previous_digest": record.previous_digest,
            });
            let expected = hex::encode(Sha256::digest(
                serde_json::to_vec(&material).expect("group audit material is serializable"),
            ));
            if record.digest != expected {
                return Err(MigrationGroupError::InvalidAudit);
            }
            previous_digest = record.digest.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> MigrationGroup {
        MigrationGroup::new(
            9,
            BrainId::new(1).unwrap(),
            LeaseTerm::INITIAL,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            [ShardId::new(1).unwrap(), ShardId::new(2).unwrap()],
        )
        .unwrap()
    }

    fn digest(value: u8) -> StateDigest {
        StateDigest([value; 16])
    }

    fn complete_shard(group: &mut MigrationGroup, shard: ShardId) {
        group.begin_transfer(shard, LeaseTerm::INITIAL).unwrap();
        group
            .mark_caught_up(
                shard,
                LeaseTerm::INITIAL,
                digest(1),
                LogicalTag::ZERO,
                LeaseTerm::new(2).unwrap(),
                digest(2),
                digest(3),
            )
            .unwrap();
        group
            .mark_fenced(shard, LeaseTerm::INITIAL, LeaseTerm::new(2).unwrap())
            .unwrap();
        group.mark_published(shard, LeaseTerm::INITIAL).unwrap();
    }

    #[test]
    fn commit_requires_every_shard_and_complete_cursor_evidence() {
        let mut group = group();
        complete_shard(&mut group, ShardId::new(1).unwrap());
        assert!(matches!(
            group.commit(LeaseTerm::INITIAL),
            Err(MigrationGroupError::IncompleteShard(_))
        ));
        complete_shard(&mut group, ShardId::new(2).unwrap());
        group.commit(LeaseTerm::INITIAL).unwrap();
        assert_eq!(group.phase, MigrationGroupPhase::Committed);
        assert!(matches!(
            group.abort(LeaseTerm::INITIAL),
            Err(MigrationGroupError::Terminal)
        ));
        group.verify().unwrap();
    }

    #[test]
    fn invalid_order_stale_terms_and_missing_cursors_fail_closed() {
        let mut group = group();
        let shard = ShardId::new(1).unwrap();
        assert!(matches!(
            group.mark_published(shard, LeaseTerm::INITIAL),
            Err(MigrationGroupError::InvalidShardTransition { .. })
        ));
        group.begin_transfer(shard, LeaseTerm::INITIAL).unwrap();
        assert!(matches!(
            group.mark_caught_up(
                shard,
                LeaseTerm::INITIAL,
                digest(1),
                LogicalTag::ZERO,
                LeaseTerm::new(2).unwrap(),
                StateDigest([0; 16]),
                digest(3),
            ),
            Err(MigrationGroupError::MissingCursorEvidence(_))
        ));
        assert!(matches!(
            group.begin_transfer(ShardId::new(2).unwrap(), LeaseTerm::new(2).unwrap()),
            Err(MigrationGroupError::StaleLeader { .. })
        ));
    }

    #[test]
    fn abort_marks_unpublished_shards_and_is_audited() {
        let mut group = group();
        group
            .begin_transfer(ShardId::new(1).unwrap(), LeaseTerm::INITIAL)
            .unwrap();
        group.abort(LeaseTerm::INITIAL).unwrap();
        assert_eq!(group.phase, MigrationGroupPhase::Aborted);
        assert_eq!(
            group.shard(ShardId::new(1).unwrap()).unwrap().phase,
            ShardMigrationPhase::Aborted
        );
        assert!(group.audit.len() >= 3);
        group.verify().unwrap();
    }

    #[test]
    fn takeover_rebinds_inflight_group_and_fences_old_leader() {
        let mut group = group();
        group.take_over(LeaseTerm::new(2).unwrap()).unwrap();
        assert!(matches!(
            group.begin_transfer(ShardId::new(1).unwrap(), LeaseTerm::INITIAL),
            Err(MigrationGroupError::StaleLeader { .. })
        ));
        group
            .begin_transfer(ShardId::new(1).unwrap(), LeaseTerm::new(2).unwrap())
            .unwrap();
        assert_eq!(
            group.audit.last().unwrap().leader_term,
            LeaseTerm::new(2).unwrap()
        );
        group.verify().unwrap();
    }
}
