//! Durable, fenced migration-operation state.
//!
//! A placement plan says where authority should run.  It does not prove that
//! state has arrived there.  This module is the small control-plane journal
//! between those two facts: it serialises brain-wide migration requests,
//! records resumable phases, and persists every transition before returning
//! success to its caller.  The journal intentionally does not move neural
//! state itself; shard executors and checkpoint/WAL adapters consume the
//! operation and provide the evidence required by the next phase.
//!
//! The implementation is deterministic and transport-neutral.  A production
//! orchestrator can put the same value transitions behind a replicated log,
//! while the file-backed adapter provides crash and fencing evidence for the
//! local reference and multi-process QA profiles.

use crate::deterministic::{BrainId, LeaseTerm, LogicalTag, StateDigest};
use crate::migration_group::{
    MigrationGroup, MigrationGroupAction, MigrationGroupError, MigrationGroupSpec,
    MigrationGroupUpdate,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const MIGRATION_OPERATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MigrationKind {
    Move,
    MigrateBrain,
    Consolidate,
    Evacuate,
    Reclaim,
    Repartition,
    ScaleShards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationPhase {
    Prepared,
    Reserving,
    Transferring,
    CatchingUp,
    Draining,
    CutoverReady,
    Committed,
    RecoveryRequired,
    Aborting,
    Aborted,
    Failed,
}

impl MigrationPhase {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Committed | Self::Aborted | Self::Failed)
    }

    fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Prepared, Self::Reserving | Self::Aborting)
                | (Self::Reserving, Self::Transferring | Self::Aborting)
                | (Self::Transferring, Self::CatchingUp | Self::Aborting)
                | (Self::CatchingUp, Self::Draining | Self::Aborting)
                | (Self::Draining, Self::CutoverReady | Self::Aborting)
                | (Self::CutoverReady, Self::Committed | Self::Aborting)
                | (Self::RecoveryRequired, Self::Reserving | Self::Aborting)
                | (Self::Aborting, Self::Aborted)
                | (Self::Prepared, Self::Failed)
                | (Self::Reserving, Self::Failed)
                | (Self::Transferring, Self::Failed)
                | (Self::CatchingUp, Self::Failed)
                | (Self::Draining, Self::Failed)
                | (Self::CutoverReady, Self::Failed)
                | (Self::RecoveryRequired, Self::Failed)
                | (Self::Aborting, Self::Failed)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationProgress {
    pub completed_shards: u32,
    pub total_shards: u32,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub cut_tag: Option<LogicalTag>,
}

impl MigrationProgress {
    pub fn new(total_shards: u32, total_bytes: u64) -> Result<Self, MigrationOperationError> {
        if total_shards == 0 {
            return Err(MigrationOperationError::InvalidProgress(
                "total_shards must be non-zero",
            ));
        }
        Ok(Self {
            completed_shards: 0,
            total_shards,
            transferred_bytes: 0,
            total_bytes,
            cut_tag: None,
        })
    }

    fn validate(&self) -> Result<(), MigrationOperationError> {
        if self.total_shards == 0
            || self.completed_shards > self.total_shards
            || self.transferred_bytes > self.total_bytes
        {
            return Err(MigrationOperationError::InvalidProgress(
                "progress exceeds its declared bounds",
            ));
        }
        Ok(())
    }

    fn monotonic_from(&self, previous: &Self) -> Result<(), MigrationOperationError> {
        self.validate()?;
        if self.total_shards != previous.total_shards
            || self.total_bytes != previous.total_bytes
            || self.completed_shards < previous.completed_shards
            || self.transferred_bytes < previous.transferred_bytes
        {
            return Err(MigrationOperationError::NonMonotonicProgress);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRequest {
    pub request_id: String,
    pub idempotency_key: String,
    pub brain_id: BrainId,
    pub observed_leader_term: LeaseTerm,
    pub expected_resource_version: u64,
    pub kind: MigrationKind,
    pub source_plan_digest: StateDigest,
    pub target_plan_digest: StateDigest,
    pub total_shards: u32,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationOperation {
    pub operation_id: u64,
    pub request_id: String,
    pub idempotency_key: String,
    pub brain_id: BrainId,
    pub kind: MigrationKind,
    pub source_plan_digest: StateDigest,
    pub target_plan_digest: StateDigest,
    pub phase: MigrationPhase,
    pub progress: MigrationProgress,
    pub resource_version: u64,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationTransition {
    pub operation_id: u64,
    pub observed_leader_term: LeaseTerm,
    pub expected_resource_version: u64,
    pub next_phase: MigrationPhase,
    pub progress: MigrationProgress,
    pub error_code: Option<String>,
}

/// Fenced cancellation request shared by the local CLI rehearsal and remote
/// management adapter.  The journal performs the authoritative validation,
/// including the bounded reason and the two-step abort transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationCancellation {
    pub operation_id: u64,
    pub observed_leader_term: LeaseTerm,
    pub expected_resource_version: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationAuditRecord {
    pub sequence: u64,
    pub operation_id: u64,
    pub phase: MigrationPhase,
    pub resource_version: u64,
    pub leader_term: LeaseTerm,
    pub digest: String,
    pub previous_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrationOperationError {
    #[error("request ID is empty")]
    EmptyRequestId,
    #[error("idempotency key is empty")]
    EmptyIdempotencyKey,
    #[error("request used stale leader term: expected {expected}, received {received}")]
    StaleLeader {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("expected resource version {expected}, current version is {current}")]
    VersionConflict { expected: u64, current: u64 },
    #[error("idempotency key was reused for a different migration")]
    IdempotencyConflict,
    #[error("brain already has an active migration")]
    BrainBusy,
    #[error("migration operation {0} is missing")]
    MissingOperation(u64),
    #[error("operation transition from {from:?} to {to:?} is invalid")]
    InvalidTransition {
        from: MigrationPhase,
        to: MigrationPhase,
    },
    #[error("migration progress is invalid: {0}")]
    InvalidProgress(&'static str),
    #[error("migration progress moved backwards")]
    NonMonotonicProgress,
    #[error("terminal operation cannot be changed")]
    TerminalOperation,
    #[error("operation ID space is exhausted")]
    OperationIdExhausted,
    #[error("resource version space is exhausted")]
    ResourceVersionExhausted,
    #[error("I/O failure: {0}")]
    Io(String),
    #[error("encoding failure: {0}")]
    Encoding(String),
    #[error("invalid journal: {0}")]
    InvalidJournal(String),
    #[error("migration group error: {0}")]
    Group(#[from] MigrationGroupError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationJournal {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub leader_term: LeaseTerm,
    pub resource_version: u64,
    next_operation_id: u64,
    operations: BTreeMap<u64, MigrationOperation>,
    idempotency: BTreeMap<String, (u64, String)>,
    /// Optional brain-wide barriers. Legacy single-shard journal entries do
    /// not have a group and retain their previous transition contract.
    #[serde(default)]
    groups: BTreeMap<u64, MigrationGroup>,
    audit: Vec<MigrationAuditRecord>,
}

impl MigrationJournal {
    pub fn new(brain_id: BrainId, leader_term: LeaseTerm) -> Self {
        Self {
            schema_version: MIGRATION_OPERATION_SCHEMA_VERSION,
            brain_id,
            leader_term,
            resource_version: 0,
            next_operation_id: 1,
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            groups: BTreeMap::new(),
            audit: Vec::new(),
        }
    }

    pub fn operation(&self, operation_id: u64) -> Option<&MigrationOperation> {
        self.operations.get(&operation_id)
    }

    pub fn operations(&self) -> impl Iterator<Item = &MigrationOperation> {
        self.operations.values()
    }

    pub fn audit(&self) -> &[MigrationAuditRecord] {
        &self.audit
    }

    pub fn group(&self, operation_id: u64) -> Option<&MigrationGroup> {
        self.groups.get(&operation_id)
    }

    /// Record the durable journal side of a complete brain-wide cutover after
    /// the placement registry has published it.  The caller supplies the
    /// already committed group produced by the migration coordinator; this
    /// method verifies its identity and all shard phases before advancing the
    /// operation in one journal mutation.
    pub fn commit_prepared_group(
        &mut self,
        group: &MigrationGroup,
        cut_tag: LogicalTag,
        transferred_bytes: u64,
        expected_resource_version: u64,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.validate_term(self.leader_term)?;
        if expected_resource_version != self.resource_version {
            return Err(MigrationOperationError::VersionConflict {
                expected: expected_resource_version,
                current: self.resource_version,
            });
        }
        let operation = self.operations.get(&group.operation_id).cloned().ok_or(
            MigrationOperationError::MissingOperation(group.operation_id),
        )?;
        if operation.phase != MigrationPhase::CutoverReady {
            return Err(MigrationOperationError::InvalidTransition {
                from: operation.phase,
                to: MigrationPhase::Committed,
            });
        }
        let registered = self.groups.get(&group.operation_id).ok_or_else(|| {
            MigrationOperationError::InvalidJournal("operation has no migration group".to_owned())
        })?;
        if group.brain_id != self.brain_id
            || group.leader_term != self.leader_term
            || group.phase != crate::migration_group::MigrationGroupPhase::Committed
            || group
                .shards
                .keys()
                .collect::<std::collections::BTreeSet<_>>()
                != registered
                    .shards
                    .keys()
                    .collect::<std::collections::BTreeSet<_>>()
            || group
                .shards
                .values()
                .any(|shard| shard.phase != crate::migration_group::ShardMigrationPhase::Published)
        {
            return Err(MigrationOperationError::InvalidJournal(
                "committed group does not match the journal barrier".to_owned(),
            ));
        }
        if group.verify().is_err()
            || cut_tag.microstep != 0
            || operation.progress.completed_shards != operation.progress.total_shards
            || transferred_bytes != operation.progress.total_bytes
        {
            return Err(MigrationOperationError::InvalidProgress(
                "committed group evidence does not match operation bounds",
            ));
        }
        let resource_version = self.next_resource_version()?;
        let mut next_operation = operation;
        next_operation.phase = MigrationPhase::Committed;
        next_operation.progress.cut_tag = Some(cut_tag);
        next_operation.resource_version = resource_version;
        self.resource_version = resource_version;
        self.groups.insert(group.operation_id, group.clone());
        self.operations
            .insert(next_operation.operation_id, next_operation.clone());
        self.append_audit(&next_operation);
        self.verify()?;
        Ok(next_operation)
    }

    /// Finalise a group returned by a registered live executor.
    ///
    /// The executor owns transfer, fencing and placement publication, so the
    /// journal cannot reconstruct those facts from a caller supplied progress
    /// counter.  This method accepts only a complete, independently verified
    /// committed group, checks that its shard set is the one registered for
    /// the operation, and then uses the same atomic commit path as the
    /// persisted reference session.  A clone is mutated first so a rejected
    /// receipt leaves the journal byte-for-byte unchanged in memory.
    pub fn commit_dispatched_group(
        &mut self,
        group: &MigrationGroup,
        cut_tag: LogicalTag,
        transferred_bytes: u64,
        expected_resource_version: u64,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        if group.brain_id != self.brain_id
            || group.leader_term != self.leader_term
            || group.phase != crate::migration_group::MigrationGroupPhase::Committed
            || group.verify().is_err()
        {
            return Err(MigrationOperationError::InvalidJournal(
                "live executor returned an invalid or foreign committed group".to_owned(),
            ));
        }
        let Some(registered) = self.groups.get(&group.operation_id) else {
            return Err(MigrationOperationError::InvalidJournal(
                "operation has no migration group".to_owned(),
            ));
        };
        if registered
            .shards
            .keys()
            .collect::<std::collections::BTreeSet<_>>()
            != group
                .shards
                .keys()
                .collect::<std::collections::BTreeSet<_>>()
        {
            return Err(MigrationOperationError::InvalidJournal(
                "live executor returned a different shard set".to_owned(),
            ));
        }
        let operation = self.operations.get(&group.operation_id).ok_or(
            MigrationOperationError::MissingOperation(group.operation_id),
        )?;
        if operation.progress.total_shards != group.shards.len() as u32
            || operation.progress.total_shards != operation.progress.completed_shards
            || operation.progress.total_bytes != transferred_bytes
            || cut_tag.microstep != 0
        {
            return Err(MigrationOperationError::InvalidProgress(
                "live executor receipt does not match operation bounds",
            ));
        }
        let mut candidate = self.clone();
        candidate.groups.insert(group.operation_id, group.clone());
        let committed = candidate.commit_prepared_group(
            group,
            cut_tag,
            transferred_bytes,
            expected_resource_version,
        )?;
        *self = candidate;
        Ok(committed)
    }

    pub fn submit(
        &mut self,
        request: MigrationRequest,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.submit_with_group(request, None)
    }

    pub fn submit_with_group(
        &mut self,
        request: MigrationRequest,
        group_spec: Option<MigrationGroupSpec>,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.validate_term(request.observed_leader_term)?;
        if request.request_id.trim().is_empty() {
            return Err(MigrationOperationError::EmptyRequestId);
        }
        if request.idempotency_key.trim().is_empty() {
            return Err(MigrationOperationError::EmptyIdempotencyKey);
        }
        let fingerprint = request_fingerprint(&request, group_spec.as_ref())?;
        if let Some((operation_id, existing_fingerprint)) =
            self.idempotency.get(&request.idempotency_key)
        {
            if existing_fingerprint != &fingerprint {
                return Err(MigrationOperationError::IdempotencyConflict);
            }
            return self
                .operation(*operation_id)
                .cloned()
                .ok_or(MigrationOperationError::MissingOperation(*operation_id));
        }
        if request.expected_resource_version != self.resource_version {
            return Err(MigrationOperationError::VersionConflict {
                expected: request.expected_resource_version,
                current: self.resource_version,
            });
        }
        if self
            .operations
            .values()
            .any(|operation| operation.brain_id == request.brain_id && !operation.phase.terminal())
        {
            return Err(MigrationOperationError::BrainBusy);
        }
        if let Some(spec) = &group_spec {
            if spec.brain_id != request.brain_id
                || spec.leader_term != request.observed_leader_term
                || spec.shard_ids.len() != request.total_shards as usize
            {
                return Err(MigrationOperationError::InvalidJournal(
                    "migration group does not match the migration request".to_owned(),
                ));
            }
        }
        let operation_id = self.next_operation_id;
        let next_operation_id = self
            .next_operation_id
            .checked_add(1)
            .ok_or(MigrationOperationError::OperationIdExhausted)?;
        let group = group_spec
            .map(|spec| spec.build(operation_id))
            .transpose()?;
        let progress = MigrationProgress::new(request.total_shards, request.total_bytes)?;
        let resource_version = self.next_resource_version()?;
        let operation = MigrationOperation {
            operation_id,
            request_id: request.request_id,
            idempotency_key: request.idempotency_key.clone(),
            brain_id: request.brain_id,
            kind: request.kind,
            source_plan_digest: request.source_plan_digest,
            target_plan_digest: request.target_plan_digest,
            phase: MigrationPhase::Prepared,
            progress,
            resource_version,
            error_code: None,
        };
        self.next_operation_id = next_operation_id;
        self.resource_version = resource_version;
        self.idempotency
            .insert(request.idempotency_key, (operation_id, fingerprint));
        self.operations.insert(operation_id, operation.clone());
        if let Some(group) = group {
            self.groups.insert(operation_id, group);
        }
        self.append_audit(&operation);
        self.verify()?;
        Ok(operation)
    }

    pub fn transition(
        &mut self,
        transition: MigrationTransition,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.validate_term(transition.observed_leader_term)?;
        if transition.expected_resource_version != self.resource_version {
            return Err(MigrationOperationError::VersionConflict {
                expected: transition.expected_resource_version,
                current: self.resource_version,
            });
        }
        let previous = self
            .operations
            .get(&transition.operation_id)
            .cloned()
            .ok_or(MigrationOperationError::MissingOperation(
                transition.operation_id,
            ))?;
        if previous.phase.terminal() {
            return Err(MigrationOperationError::TerminalOperation);
        }
        if !previous.phase.may_transition_to(transition.next_phase) {
            return Err(MigrationOperationError::InvalidTransition {
                from: previous.phase,
                to: transition.next_phase,
            });
        }
        transition.progress.monotonic_from(&previous.progress)?;
        if transition.next_phase == MigrationPhase::Committed
            && (transition.progress.completed_shards != transition.progress.total_shards
                || transition.progress.transferred_bytes != transition.progress.total_bytes
                || transition.progress.cut_tag.is_none())
        {
            return Err(MigrationOperationError::InvalidProgress(
                "commit requires complete transfer and a cut tag",
            ));
        }
        if transition.next_phase == MigrationPhase::Committed
            && self
                .groups
                .get(&transition.operation_id)
                .is_some_and(|group| {
                    group.phase != crate::migration_group::MigrationGroupPhase::Committed
                })
        {
            return Err(MigrationOperationError::InvalidProgress(
                "grouped commit requires a committed migration group",
            ));
        }
        if transition.next_phase == MigrationPhase::Failed
            && transition.error_code.as_deref().unwrap_or("").is_empty()
        {
            return Err(MigrationOperationError::InvalidProgress(
                "failed operations require an error code",
            ));
        }
        let resource_version = self.next_resource_version()?;
        let mut operation = previous;
        operation.phase = transition.next_phase;
        operation.progress = transition.progress;
        operation.error_code = transition.error_code;
        operation.resource_version = resource_version;
        self.resource_version = resource_version;
        self.operations
            .insert(operation.operation_id, operation.clone());
        self.append_audit(&operation);
        self.verify()?;
        Ok(operation)
    }

    /// Apply one shard-level barrier fact through the same journal resource
    /// version as ordinary migration progress. Data transfer can run in
    /// parallel, while these authority facts remain serialised and durable.
    pub fn apply_group_update(
        &mut self,
        update: MigrationGroupUpdate,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.validate_term(update.observed_leader_term)?;
        if update.expected_resource_version != self.resource_version {
            return Err(MigrationOperationError::VersionConflict {
                expected: update.expected_resource_version,
                current: self.resource_version,
            });
        }
        let operation = self.operations.get(&update.operation_id).cloned().ok_or(
            MigrationOperationError::MissingOperation(update.operation_id),
        )?;
        if operation.phase.terminal() {
            return Err(MigrationOperationError::TerminalOperation);
        }
        // Mutate a clone until every journal-side allocation succeeds. A
        // rejected update must leave both the barrier and operation version
        // unchanged, just like a rejected placement transaction.
        let mut group = self
            .groups
            .get(&update.operation_id)
            .cloned()
            .ok_or_else(|| {
                MigrationOperationError::InvalidJournal(
                    "operation has no migration group".to_owned(),
                )
            })?;
        match update.action {
            MigrationGroupAction::BeginTransfer { shard_id } => {
                group.begin_transfer(shard_id, update.observed_leader_term)?;
            }
            MigrationGroupAction::MarkCaughtUp {
                shard_id,
                checkpoint_digest,
                cut_tag,
                destination_term,
                route_cursor_digest,
                effect_cursor_digest,
            } => group.mark_caught_up(
                shard_id,
                update.observed_leader_term,
                checkpoint_digest,
                cut_tag,
                destination_term,
                route_cursor_digest,
                effect_cursor_digest,
            )?,
            MigrationGroupAction::MarkFenced {
                shard_id,
                destination_term,
            } => group.mark_fenced(shard_id, update.observed_leader_term, destination_term)?,
            MigrationGroupAction::MarkPublished { shard_id } => {
                group.mark_published(shard_id, update.observed_leader_term)?;
            }
            MigrationGroupAction::Commit => group.commit(update.observed_leader_term)?,
            MigrationGroupAction::Abort => group.abort(update.observed_leader_term)?,
        }
        let resource_version = self.next_resource_version()?;
        self.resource_version = resource_version;
        self.groups.insert(update.operation_id, group);
        let mut operation = operation;
        operation.resource_version = resource_version;
        self.operations
            .insert(operation.operation_id, operation.clone());
        self.append_audit(&operation);
        self.verify()?;
        Ok(operation)
    }

    /// Cancel an in-flight operation through the same fenced journal as normal
    /// progress. Cancellation is two durable transitions so recovery can
    /// distinguish a request that began aborting from one that completed the
    /// abort. A committed migration cannot be cancelled by reviving its old
    /// writer.
    pub fn cancel(
        &mut self,
        operation_id: u64,
        observed_leader_term: LeaseTerm,
        expected_resource_version: u64,
        reason: impl Into<String>,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        let reason = reason.into();
        if reason.trim().is_empty() || reason.len() > 1024 {
            return Err(MigrationOperationError::InvalidProgress(
                "cancellation reason must be present and bounded",
            ));
        }
        let operation = self
            .operation(operation_id)
            .cloned()
            .ok_or(MigrationOperationError::MissingOperation(operation_id))?;
        if operation.phase.terminal() {
            return Err(MigrationOperationError::TerminalOperation);
        }
        let aborting = self.transition(MigrationTransition {
            operation_id,
            observed_leader_term,
            expected_resource_version,
            next_phase: MigrationPhase::Aborting,
            progress: operation.progress.clone(),
            error_code: Some(format!("cancellation_requested:{reason}")),
        })?;
        self.transition(MigrationTransition {
            operation_id,
            observed_leader_term,
            expected_resource_version: aborting.resource_version,
            next_phase: MigrationPhase::Aborted,
            progress: aborting.progress,
            error_code: aborting.error_code,
        })
    }

    /// A new leader never assumes that an in-flight transfer completed.  It
    /// records recovery as the next durable fact; the shard adapter must then
    /// inspect checkpoint/WAL evidence and explicitly resume or abort.
    pub fn take_over(&mut self, new_term: LeaseTerm) -> Result<(), MigrationOperationError> {
        if new_term <= self.leader_term {
            return Err(MigrationOperationError::StaleLeader {
                expected: self.leader_term,
                received: new_term,
            });
        }
        self.leader_term = new_term;
        for group in self.groups.values_mut() {
            if !matches!(
                group.phase,
                crate::migration_group::MigrationGroupPhase::Committed
                    | crate::migration_group::MigrationGroupPhase::Aborted
            ) {
                group.take_over(new_term)?;
            }
        }
        let active_ids = self
            .operations
            .values()
            .filter(|operation| !operation.phase.terminal())
            .map(|operation| operation.operation_id)
            .collect::<Vec<_>>();
        for operation_id in active_ids {
            let resource_version = self.next_resource_version()?;
            self.resource_version = resource_version;
            let operation = self
                .operations
                .get_mut(&operation_id)
                .expect("active operation was collected from the operation map");
            operation.phase = MigrationPhase::RecoveryRequired;
            operation.resource_version = resource_version;
            let operation = operation.clone();
            self.append_audit(&operation);
        }
        self.verify()
    }

    fn validate_term(&self, received: LeaseTerm) -> Result<(), MigrationOperationError> {
        if received != self.leader_term {
            return Err(MigrationOperationError::StaleLeader {
                expected: self.leader_term,
                received,
            });
        }
        Ok(())
    }

    fn next_resource_version(&self) -> Result<u64, MigrationOperationError> {
        self.resource_version
            .checked_add(1)
            .ok_or(MigrationOperationError::ResourceVersionExhausted)
    }

    fn append_audit(&mut self, operation: &MigrationOperation) {
        let previous_digest = self
            .audit
            .last()
            .map(|record| record.digest.clone())
            .unwrap_or_else(|| "0".repeat(64));
        let sequence = self.audit.len() as u64 + 1;
        let material = serde_json::json!({
            "sequence": sequence,
            "operation_id": operation.operation_id,
            "phase": operation.phase,
            "leader_term": self.leader_term,
            "previous_digest": previous_digest,
            "resource_version": operation.resource_version,
        });
        let digest = hex::encode(Sha256::digest(
            serde_json::to_vec(&material).expect("audit material is serializable"),
        ));
        self.audit.push(MigrationAuditRecord {
            sequence,
            operation_id: operation.operation_id,
            phase: operation.phase,
            resource_version: operation.resource_version,
            leader_term: self.leader_term,
            digest,
            previous_digest,
        });
    }

    pub fn verify(&self) -> Result<(), MigrationOperationError> {
        if self.schema_version != MIGRATION_OPERATION_SCHEMA_VERSION {
            return Err(MigrationOperationError::InvalidJournal(
                "unsupported schema version".to_owned(),
            ));
        }
        for operation in self.operations.values() {
            operation.progress.validate()?;
            if operation.resource_version > self.resource_version {
                return Err(MigrationOperationError::InvalidJournal(
                    "operation version is newer than journal".to_owned(),
                ));
            }
        }
        for (operation_id, group) in &self.groups {
            if !self.operations.contains_key(operation_id)
                || group.operation_id != *operation_id
                || group.brain_id != self.brain_id
            {
                return Err(MigrationOperationError::InvalidJournal(
                    "migration group is not bound to its operation".to_owned(),
                ));
            }
            group.verify()?;
        }
        let mut previous_digest = "0".repeat(64);
        for (index, record) in self.audit.iter().enumerate() {
            if record.sequence != index as u64 + 1
                || record.digest.len() != 64
                || record.previous_digest != previous_digest
            {
                return Err(MigrationOperationError::InvalidJournal(
                    "audit chain is invalid".to_owned(),
                ));
            }
            let material = serde_json::json!({
                "sequence": record.sequence,
                "operation_id": record.operation_id,
                "phase": record.phase,
                "leader_term": record.leader_term,
                "previous_digest": record.previous_digest,
                "resource_version": record.resource_version,
            });
            let expected = hex::encode(Sha256::digest(
                serde_json::to_vec(&material).expect("audit material is serializable"),
            ));
            if record.digest != expected {
                return Err(MigrationOperationError::InvalidJournal(
                    "audit digest is invalid".to_owned(),
                ));
            }
            previous_digest = record.digest.clone();
        }
        Ok(())
    }
}

fn request_fingerprint(
    request: &MigrationRequest,
    group_spec: Option<&MigrationGroupSpec>,
) -> Result<String, MigrationOperationError> {
    let bytes = serde_json::to_vec(&(request, group_spec))
        .map_err(|error| MigrationOperationError::Encoding(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationJournalDocument {
    schema_version: u32,
    journal: MigrationJournal,
}

/// File-backed journal with exclusive locking, fsync and atomic publication.
/// The lock is held only for one state transition, so long checkpoint or data
/// transfer work never blocks unrelated control-plane readers.
#[derive(Debug)]
pub struct PersistedMigrationJournal {
    path: PathBuf,
    lock_path: PathBuf,
    journal: MigrationJournal,
}

impl PersistedMigrationJournal {
    pub fn open(
        path: impl Into<PathBuf>,
        brain_id: BrainId,
        leader_term: LeaseTerm,
    ) -> Result<Self, MigrationOperationError> {
        let path = path.into();
        let lock_path = path.with_extension("migration.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| MigrationOperationError::Io(error.to_string()))?;
        }
        let lock = lock_file(&lock_path)?;
        let result = if path.exists() {
            let journal = read_journal(&path)?;
            if journal.brain_id != brain_id {
                Err(MigrationOperationError::InvalidJournal(
                    "brain identity does not match journal".to_owned(),
                ))
            } else if leader_term < journal.leader_term {
                Err(MigrationOperationError::StaleLeader {
                    expected: journal.leader_term,
                    received: leader_term,
                })
            } else {
                let mut journal = journal;
                if leader_term > journal.leader_term {
                    journal.take_over(leader_term)?;
                    persist_journal(&path, &journal)?;
                }
                Ok(journal)
            }
        } else {
            let journal = MigrationJournal::new(brain_id, leader_term);
            persist_journal(&path, &journal)?;
            Ok(journal)
        };
        unlock_file(lock)?;
        Ok(Self {
            path,
            lock_path,
            journal: result?,
        })
    }

    /// Reopen an existing journal using its persisted leader term.
    ///
    /// This is used by background execution callbacks that intentionally do
    /// not carry a caller supplied term.  The normal management request path
    /// should continue to use [`Self::open`] so a client cannot select a
    /// stale term.  The callback still re-reads and validates the journal
    /// under the transition lock before changing it.
    pub fn open_existing(
        path: impl Into<PathBuf>,
        brain_id: BrainId,
    ) -> Result<Self, MigrationOperationError> {
        let path = path.into();
        let journal = read_journal(&path)?;
        if journal.brain_id != brain_id {
            return Err(MigrationOperationError::InvalidJournal(
                "brain identity does not match journal".to_owned(),
            ));
        }
        Self::open(path, brain_id, journal.leader_term)
    }

    pub fn journal(&self) -> &MigrationJournal {
        &self.journal
    }

    /// Read one operation from the last verified journal snapshot.
    pub fn operation(&self, operation_id: u64) -> Option<&MigrationOperation> {
        self.journal.operation(operation_id)
    }

    pub fn refresh(&mut self) -> Result<(), MigrationOperationError> {
        self.journal = read_journal(&self.path)?;
        Ok(())
    }

    pub fn submit(
        &mut self,
        request: MigrationRequest,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| journal.submit(request))
    }

    pub fn submit_with_group(
        &mut self,
        request: MigrationRequest,
        group_spec: Option<MigrationGroupSpec>,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| journal.submit_with_group(request, group_spec))
    }

    pub fn transition(
        &mut self,
        transition: MigrationTransition,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| journal.transition(transition))
    }

    pub fn apply_group_update(
        &mut self,
        update: MigrationGroupUpdate,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| journal.apply_group_update(update))
    }

    /// Persist the final group barrier after the placement registry has
    /// published the complete destination plan.
    pub fn commit_prepared_group(
        &mut self,
        group: &MigrationGroup,
        cut_tag: LogicalTag,
        transferred_bytes: u64,
        expected_resource_version: u64,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| {
            journal.commit_prepared_group(
                group,
                cut_tag,
                transferred_bytes,
                expected_resource_version,
            )
        })
    }

    pub fn commit_dispatched_group(
        &mut self,
        group: &MigrationGroup,
        cut_tag: LogicalTag,
        transferred_bytes: u64,
        expected_resource_version: u64,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| {
            journal.commit_dispatched_group(
                group,
                cut_tag,
                transferred_bytes,
                expected_resource_version,
            )
        })
    }

    pub fn cancel(
        &mut self,
        operation_id: u64,
        observed_leader_term: LeaseTerm,
        expected_resource_version: u64,
        reason: impl Into<String>,
    ) -> Result<MigrationOperation, MigrationOperationError> {
        self.with_update(|journal| {
            journal.cancel(
                operation_id,
                observed_leader_term,
                expected_resource_version,
                reason,
            )
        })
    }

    fn with_update<T>(
        &mut self,
        update: impl FnOnce(&mut MigrationJournal) -> Result<T, MigrationOperationError>,
    ) -> Result<T, MigrationOperationError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut journal = read_journal(&self.path)?;
            let value = update(&mut journal)?;
            persist_journal(&self.path, &journal)?;
            self.journal = journal;
            Ok(value)
        })();
        unlock_file(lock)?;
        result
    }
}

fn lock_file(path: &Path) -> Result<std::fs::File, MigrationOperationError> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .map_err(|error| MigrationOperationError::Io(error.to_string()))?;
    file.lock_exclusive()
        .map_err(|error| MigrationOperationError::Io(error.to_string()))?;
    Ok(file)
}

fn unlock_file(file: std::fs::File) -> Result<(), MigrationOperationError> {
    file.unlock()
        .map_err(|error| MigrationOperationError::Io(error.to_string()))
}

fn read_journal(path: &Path) -> Result<MigrationJournal, MigrationOperationError> {
    let bytes = fs::read(path).map_err(|error| MigrationOperationError::Io(error.to_string()))?;
    let document: MigrationJournalDocument = serde_json::from_slice(&bytes)
        .map_err(|error| MigrationOperationError::Encoding(error.to_string()))?;
    if document.schema_version != MIGRATION_OPERATION_SCHEMA_VERSION {
        return Err(MigrationOperationError::InvalidJournal(
            "unsupported schema version".to_owned(),
        ));
    }
    document.journal.verify()?;
    Ok(document.journal)
}

fn persist_journal(path: &Path, journal: &MigrationJournal) -> Result<(), MigrationOperationError> {
    journal.verify()?;
    let document = MigrationJournalDocument {
        schema_version: MIGRATION_OPERATION_SCHEMA_VERSION,
        journal: journal.clone(),
    };
    let bytes = serde_json::to_vec(&document)
        .map_err(|error| MigrationOperationError::Encoding(error.to_string()))?;
    let temporary = path.with_extension(format!("migration.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| MigrationOperationError::Io(error.to_string()))?;
    let result = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, path))
        .map_err(|error| MigrationOperationError::Io(error.to_string()));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| MigrationOperationError::Io(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration_group::{MigrationGroupAction, MigrationGroupSpec, MigrationGroupUpdate};

    fn request(version: u64) -> MigrationRequest {
        MigrationRequest {
            request_id: "request-1".to_owned(),
            idempotency_key: "key-1".to_owned(),
            brain_id: BrainId::new(7).unwrap(),
            observed_leader_term: LeaseTerm::INITIAL,
            expected_resource_version: version,
            kind: MigrationKind::Consolidate,
            source_plan_digest: StateDigest([1; 16]),
            target_plan_digest: StateDigest([2; 16]),
            total_shards: 2,
            total_bytes: 100,
        }
    }

    fn progress(completed: u32, bytes: u64, cut_tag: Option<LogicalTag>) -> MigrationProgress {
        MigrationProgress {
            completed_shards: completed,
            total_shards: 2,
            transferred_bytes: bytes,
            total_bytes: 100,
            cut_tag,
        }
    }

    #[test]
    fn grouped_operation_persists_barrier_and_requires_group_commit() {
        let mut journal = MigrationJournal::new(BrainId::new(7).unwrap(), LeaseTerm::INITIAL);
        let operation = journal
            .submit_with_group(
                request(0),
                Some(MigrationGroupSpec {
                    brain_id: BrainId::new(7).unwrap(),
                    leader_term: LeaseTerm::INITIAL,
                    topology_generation: crate::deterministic::TopologyGeneration::INITIAL,
                    partition_generation: crate::deterministic::PartitionGeneration::INITIAL,
                    shard_ids: vec![
                        crate::deterministic::ShardId::new(1).unwrap(),
                        crate::deterministic::ShardId::new(2).unwrap(),
                    ],
                }),
            )
            .unwrap();
        assert!(journal.group(operation.operation_id).is_some());

        let advance = |journal: &mut MigrationJournal,
                       action: MigrationGroupAction,
                       expected_resource_version| {
            journal.apply_group_update(MigrationGroupUpdate {
                operation_id: operation.operation_id,
                observed_leader_term: LeaseTerm::INITIAL,
                expected_resource_version,
                action,
            })
        };
        let shard = crate::deterministic::ShardId::new(1).unwrap();
        let second = crate::deterministic::ShardId::new(2).unwrap();
        let mut version = 1;
        for shard_id in [shard, second] {
            advance(
                &mut journal,
                MigrationGroupAction::BeginTransfer { shard_id },
                version,
            )
            .unwrap();
            version += 1;
            advance(
                &mut journal,
                MigrationGroupAction::MarkCaughtUp {
                    shard_id,
                    checkpoint_digest: StateDigest([1; 16]),
                    cut_tag: LogicalTag::ZERO,
                    destination_term: LeaseTerm::new(2).unwrap(),
                    route_cursor_digest: StateDigest([2; 16]),
                    effect_cursor_digest: StateDigest([3; 16]),
                },
                version,
            )
            .unwrap();
            version += 1;
            advance(
                &mut journal,
                MigrationGroupAction::MarkFenced {
                    shard_id,
                    destination_term: LeaseTerm::new(2).unwrap(),
                },
                version,
            )
            .unwrap();
            version += 1;
            advance(
                &mut journal,
                MigrationGroupAction::MarkPublished { shard_id },
                version,
            )
            .unwrap();
            version += 1;
        }
        advance(&mut journal, MigrationGroupAction::Commit, version).unwrap();
        assert_eq!(
            journal.group(operation.operation_id).unwrap().phase,
            crate::migration_group::MigrationGroupPhase::Committed
        );

        let mut current_progress = progress(0, 0, None);
        for phase in [
            MigrationPhase::Reserving,
            MigrationPhase::Transferring,
            MigrationPhase::CatchingUp,
            MigrationPhase::Draining,
            MigrationPhase::CutoverReady,
        ] {
            current_progress = match phase {
                MigrationPhase::CutoverReady => progress(2, 100, Some(LogicalTag::ZERO)),
                _ => current_progress.clone(),
            };
            let current = journal.resource_version;
            journal
                .transition(MigrationTransition {
                    operation_id: operation.operation_id,
                    observed_leader_term: LeaseTerm::INITIAL,
                    expected_resource_version: current,
                    next_phase: phase,
                    progress: current_progress.clone(),
                    error_code: None,
                })
                .unwrap();
        }
        let committed = journal
            .transition(MigrationTransition {
                operation_id: operation.operation_id,
                observed_leader_term: LeaseTerm::INITIAL,
                expected_resource_version: journal.resource_version,
                next_phase: MigrationPhase::Committed,
                progress: current_progress,
                error_code: None,
            })
            .unwrap();
        assert_eq!(committed.phase, MigrationPhase::Committed);
        journal.verify().unwrap();
    }

    #[test]
    fn journal_is_fenced_idempotent_and_serialises_one_brain() {
        let mut journal = MigrationJournal::new(BrainId::new(7).unwrap(), LeaseTerm::INITIAL);
        let first = journal.submit(request(0)).unwrap();
        assert_eq!(journal.submit(request(0)).unwrap(), first);
        let mut conflicting = request(0);
        conflicting.target_plan_digest = StateDigest([3; 16]);
        assert_eq!(
            journal.submit(conflicting).unwrap_err(),
            MigrationOperationError::IdempotencyConflict
        );
        let mut second = request(1);
        second.idempotency_key = "key-2".to_owned();
        assert_eq!(
            journal.submit(second).unwrap_err(),
            MigrationOperationError::BrainBusy
        );
    }

    #[test]
    fn transition_requires_complete_cut_before_commit_and_rejects_stale_versions() {
        let mut journal = MigrationJournal::new(BrainId::new(7).unwrap(), LeaseTerm::INITIAL);
        let operation = journal.submit(request(0)).unwrap();
        let advance =
            |journal: &mut MigrationJournal, phase, progress, expected_resource_version| {
                journal.transition(MigrationTransition {
                    operation_id: operation.operation_id,
                    observed_leader_term: LeaseTerm::INITIAL,
                    expected_resource_version,
                    next_phase: phase,
                    progress,
                    error_code: None,
                })
            };
        let reserving = advance(
            &mut journal,
            MigrationPhase::Reserving,
            progress(0, 0, None),
            1,
        )
        .unwrap();
        assert_eq!(
            advance(
                &mut journal,
                MigrationPhase::Transferring,
                progress(0, 0, None),
                reserving.resource_version - 1,
            )
            .unwrap_err(),
            MigrationOperationError::VersionConflict {
                expected: 1,
                current: 2
            }
        );
        let transferring = advance(
            &mut journal,
            MigrationPhase::Transferring,
            progress(0, 0, None),
            reserving.resource_version,
        )
        .unwrap();
        let catching = advance(
            &mut journal,
            MigrationPhase::CatchingUp,
            progress(1, 50, None),
            transferring.resource_version,
        )
        .unwrap();
        let draining = advance(
            &mut journal,
            MigrationPhase::Draining,
            progress(2, 100, None),
            catching.resource_version,
        )
        .unwrap();
        let ready = advance(
            &mut journal,
            MigrationPhase::CutoverReady,
            progress(2, 100, Some(LogicalTag::ZERO)),
            draining.resource_version,
        )
        .unwrap();
        let committed = advance(
            &mut journal,
            MigrationPhase::Committed,
            ready.progress.clone(),
            ready.resource_version,
        )
        .unwrap();
        assert_eq!(committed.phase, MigrationPhase::Committed);
    }

    #[test]
    fn cancellation_is_fenced_two_phase_and_cannot_revive_a_committed_operation() {
        let mut journal = MigrationJournal::new(BrainId::new(7).unwrap(), LeaseTerm::INITIAL);
        let operation = journal.submit(request(0)).unwrap();
        let aborted = journal
            .cancel(
                operation.operation_id,
                LeaseTerm::INITIAL,
                operation.resource_version,
                "operator requested shutdown",
            )
            .unwrap();
        assert_eq!(aborted.phase, MigrationPhase::Aborted);
        assert_eq!(aborted.resource_version, 3);
        assert!(
            aborted
                .error_code
                .as_deref()
                .is_some_and(|code| code.contains("operator requested shutdown"))
        );
        assert_eq!(journal.audit().len(), 3);
        assert!(matches!(
            journal.cancel(
                operation.operation_id,
                LeaseTerm::INITIAL,
                aborted.resource_version,
                "retry",
            ),
            Err(MigrationOperationError::TerminalOperation)
        ));
    }

    #[test]
    fn takeover_marks_inflight_work_for_recovery() {
        let mut journal = MigrationJournal::new(BrainId::new(7).unwrap(), LeaseTerm::INITIAL);
        let operation = journal.submit(request(0)).unwrap();
        journal
            .transition(MigrationTransition {
                operation_id: operation.operation_id,
                observed_leader_term: LeaseTerm::INITIAL,
                expected_resource_version: 1,
                next_phase: MigrationPhase::Reserving,
                progress: progress(0, 0, None),
                error_code: None,
            })
            .unwrap();
        journal.take_over(LeaseTerm::new(2).unwrap()).unwrap();
        assert_eq!(
            journal.operation(operation.operation_id).unwrap().phase,
            MigrationPhase::RecoveryRequired
        );
    }

    #[test]
    fn persisted_journal_survives_reopen_and_fences_old_leader() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-migration-journal-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let brain = BrainId::new(7).unwrap();
        let mut persisted =
            PersistedMigrationJournal::open(&path, brain, LeaseTerm::INITIAL).unwrap();
        persisted.submit(request(0)).unwrap();
        drop(persisted);
        let reopened = PersistedMigrationJournal::open(&path, brain, LeaseTerm::INITIAL).unwrap();
        assert!(reopened.journal().operation(1).is_some());
        let promoted =
            PersistedMigrationJournal::open(&path, brain, LeaseTerm::new(2).unwrap()).unwrap();
        assert_eq!(promoted.journal().leader_term, LeaseTerm::new(2).unwrap());
        assert!(matches!(
            PersistedMigrationJournal::open(&path, brain, LeaseTerm::INITIAL),
            Err(MigrationOperationError::StaleLeader { .. })
        ));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("migration.lock"));
    }
}
