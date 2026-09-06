//! Fenced commit boundary for the stable multi-shard reference executor.
//!
//! This adapter makes the whole-fabric checkpoint set the commit point for a
//! reference authority. It is intentionally separate from the legacy
//! `ManagedNetwork` runner and from the per-shard `AuthoritativeShard` actor:
//! the latter still owns the production WAL/election integration. The adapter
//! is useful for deterministic crash/retry and migration rehearsals because a
//! failed immutable publication restores the exact pre-step executor.

use crate::deterministic::{BrainId, EventId, LeaseTerm, StateDigest};
use crate::durability::CheckpointManifest;
use crate::shard_executor::{
    AdmissionOutcome, RoutedCausalEvent, ShardExecutionError, ShardExecutionResult,
    StableShardExecutor,
};
use crate::stable_executor_store::{StableExecutorCheckpointStore, StableExecutorStoreError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StableExecutorAuthorityError {
    #[error(
        "stable executor authority is fenced: expected term {expected_term} and token {expected_token}, received term {received_term} and token {received_token}"
    )]
    Fenced {
        expected_term: LeaseTerm,
        expected_token: u64,
        received_term: LeaseTerm,
        received_token: u64,
    },
    #[error(transparent)]
    Execution(#[from] ShardExecutionError),
    #[error(transparent)]
    Storage(#[from] StableExecutorStoreError),
}

/// One-writer reference authority for a complete stable executor.
#[derive(Debug)]
pub struct StableExecutorAuthority {
    executor: StableShardExecutor,
    store: StableExecutorCheckpointStore,
    term: LeaseTerm,
    fencing_token: u64,
    last_checkpoint: Option<CheckpointManifest>,
}

impl StableExecutorAuthority {
    pub fn new(
        executor: StableShardExecutor,
        store: StableExecutorCheckpointStore,
        term: LeaseTerm,
        fencing_token: u64,
    ) -> Self {
        Self {
            executor,
            store,
            term,
            fencing_token,
            last_checkpoint: None,
        }
    }

    pub fn brain_id(&self) -> BrainId {
        self.executor.brain_id()
    }

    pub fn term(&self) -> LeaseTerm {
        self.term
    }

    pub fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    pub fn executor(&self) -> &StableShardExecutor {
        &self.executor
    }

    pub fn last_checkpoint(&self) -> Option<&CheckpointManifest> {
        self.last_checkpoint.as_ref()
    }

    /// Return a clone of the immutable checkpoint store used by this
    /// authority.  The store is itself a filesystem-backed immutable view;
    /// exposing a clone lets a migration adapter read the last published cut
    /// without gaining a second writer or bypassing the authority fence.
    pub fn checkpoint_store(&self) -> StableExecutorCheckpointStore {
        self.store.clone()
    }

    pub fn state_digest(&self) -> Result<StateDigest, StableExecutorAuthorityError> {
        self.executor
            .state_digest()
            .map_err(StableExecutorAuthorityError::from)
    }

    /// Publish the current complete fabric as an immutable initial/recovery
    /// boundary. Callers choose the operation/checkpoint ID so retries are
    /// explicit and a reused ID fails closed rather than replacing history.
    pub fn checkpoint(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        checkpoint_id: EventId,
    ) -> Result<CheckpointManifest, StableExecutorAuthorityError> {
        self.validate_fence(observed_term, observed_fencing_token)?;
        let manifest = self
            .store
            .publish(checkpoint_id, self.term, &self.executor)?;
        self.last_checkpoint = Some(manifest.clone());
        Ok(manifest)
    }

    /// Admit and commit one canonical event. The executor is cloned before
    /// mutation; if execution or immutable publication fails, the previous
    /// fabric—including queues and dedupe state—is restored exactly.
    pub fn admit_and_step(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        routed: RoutedCausalEvent,
        checkpoint_id: EventId,
    ) -> Result<Option<ShardExecutionResult>, StableExecutorAuthorityError> {
        self.validate_fence(observed_term, observed_fencing_token)?;
        let before = self.executor.clone();
        let admission = self.executor.admit(routed)?;
        if matches!(admission, AdmissionOutcome::Duplicate { .. }) {
            return Ok(None);
        }
        let result = match self.executor.step() {
            Ok(Some(result)) => result,
            Ok(None) => {
                self.executor = before;
                return Ok(None);
            }
            Err(error) => {
                self.executor = before;
                return Err(error.into());
            }
        };
        match self.store.publish(checkpoint_id, self.term, &self.executor) {
            Ok(manifest) => {
                self.last_checkpoint = Some(manifest);
                Ok(Some(result))
            }
            Err(error) => {
                self.executor = before;
                Err(error.into())
            }
        }
    }

    /// Process the next already-admitted causal event and publish the
    /// resulting complete fabric cut.  This is the durable counterpart to
    /// [`StableShardExecutor::step`]: a caller never has to execute queued
    /// work outside the same term/fencing/publication boundary used for new
    /// admissions.
    pub fn step_pending(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        checkpoint_id: EventId,
    ) -> Result<Option<ShardExecutionResult>, StableExecutorAuthorityError> {
        self.validate_fence(observed_term, observed_fencing_token)?;
        let before = self.executor.clone();
        let result = match self.executor.step() {
            Ok(Some(result)) => result,
            Ok(None) => return Ok(None),
            Err(error) => {
                self.executor = before;
                return Err(error.into());
            }
        };
        match self.store.publish(checkpoint_id, self.term, &self.executor) {
            Ok(manifest) => {
                self.last_checkpoint = Some(manifest);
                Ok(Some(result))
            }
            Err(error) => {
                self.executor = before;
                Err(error.into())
            }
        }
    }

    /// Move the reference authority to a strictly newer term. This operation
    /// changes only the writer fence; biological state is published at the
    /// next checkpoint under the new term.
    pub fn reissue_term(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        new_term: LeaseTerm,
        new_fencing_token: u64,
    ) -> Result<(), StableExecutorAuthorityError> {
        self.validate_fence(observed_term, observed_fencing_token)?;
        if new_term <= self.term || new_fencing_token == 0 {
            return Err(StableExecutorAuthorityError::Fenced {
                expected_term: self.term,
                expected_token: self.fencing_token,
                received_term: new_term,
                received_token: new_fencing_token,
            });
        }
        self.term = new_term;
        self.fencing_token = new_fencing_token;
        Ok(())
    }

    fn validate_fence(
        &self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
    ) -> Result<(), StableExecutorAuthorityError> {
        if observed_term != self.term || observed_fencing_token != self.fencing_token {
            return Err(StableExecutorAuthorityError::Fenced {
                expected_term: self.term,
                expected_token: self.fencing_token,
                received_term: observed_term,
                received_token: observed_fencing_token,
            });
        }
        Ok(())
    }
}
