//! Bounded managed runtime for a physically partial stable-shard worker.
//!
//! [`PartialShardExecutor`] owns the deterministic biological state for the
//! shards assigned to one worker. [`StableShardDispatcher`] owns placement
//! resolution and the durable physical handoff. This module composes them at a
//! single commit boundary: biological work is staged on a clone, every typed
//! outbound message is atomically sealed in the durable outbox, and only then
//! is the staged worker state published as the current in-memory state.
//!
//! A network failure after the outbox commit is safe: the worker state and its
//! outbound records describe the same transition, and the dispatcher can retry
//! the pending stream. A validation or outbox failure before that commit leaves
//! the worker unchanged. This is a reference worker-loop adapter; quorum
//! authority, durable receiver ownership and production cutover remain the
//! explicit gates recorded in the active ExecPlans.

use crate::partial_shard_executor::{
    PartialShardApply, PartialShardExecutor, PartialShardExecutorError, PartialShardOutbound,
    PartialShardStep,
};
use crate::shard_executor::{RoutedCausalEvent, StableShardCheckpoint};
use crate::stable_shard_dispatch::{
    StableShardDispatchError, StableShardDispatchReport, StableShardDispatcher,
};
use thiserror::Error;

const MAX_INPUT_EVENTS: usize = 4096;

#[derive(Debug, Error)]
pub enum ManagedPartialShardRuntimeError {
    #[error("partial worker poll has {actual} input events, exceeding bound {max}")]
    InputBatchTooLarge { actual: usize, max: usize },
    #[error("partial worker step budget must be positive")]
    InvalidStepBudget,
    #[error(transparent)]
    Executor(#[from] PartialShardExecutorError),
    #[error(transparent)]
    Dispatch(#[from] StableShardDispatchError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPartialShardPoll {
    pub steps: Vec<PartialShardStep>,
    pub outbound_records: usize,
    pub pending_after: usize,
    pub budget_exhausted: bool,
}

impl ManagedPartialShardPoll {
    pub fn is_quiescent(&self) -> bool {
        self.pending_after == 0 && !self.budget_exhausted
    }
}

/// A bounded, placement-aware partial worker.
#[derive(Debug)]
pub struct ManagedPartialShardRuntime {
    executor: PartialShardExecutor,
    dispatcher: StableShardDispatcher,
    max_steps_per_poll: usize,
}

impl ManagedPartialShardRuntime {
    pub fn new(
        executor: PartialShardExecutor,
        dispatcher: StableShardDispatcher,
        max_steps_per_poll: usize,
    ) -> Result<Self, ManagedPartialShardRuntimeError> {
        if max_steps_per_poll == 0 {
            return Err(ManagedPartialShardRuntimeError::InvalidStepBudget);
        }
        Ok(Self {
            executor,
            dispatcher,
            max_steps_per_poll,
        })
    }

    pub fn executor(&self) -> &PartialShardExecutor {
        &self.executor
    }

    pub fn max_steps_per_poll(&self) -> usize {
        self.max_steps_per_poll
    }

    pub fn set_max_steps_per_poll(&mut self, value: usize) {
        if value != 0 {
            self.max_steps_per_poll = value;
        }
    }

    /// Clone the placement-aware dispatcher without exposing the executor.
    ///
    /// The dispatcher owns shared durable outbox and endpoint handles, so a
    /// caller can take this snapshot while holding the worker mutex and then
    /// perform network I/O after releasing that mutex. This keeps a slow peer
    /// from blocking local biological progress for the same worker or other
    /// independent workers.
    pub fn dispatcher(&self) -> StableShardDispatcher {
        self.dispatcher.clone()
    }

    pub fn register_endpoint(
        &self,
        node_id: impl Into<String>,
        address: impl Into<String>,
    ) -> Result<(), ManagedPartialShardRuntimeError> {
        self.dispatcher.register_endpoint(node_id, address)?;
        Ok(())
    }

    pub fn remove_endpoint(&self, node_id: &str) -> Result<bool, ManagedPartialShardRuntimeError> {
        Ok(self.dispatcher.remove_endpoint(node_id)?)
    }

    /// Stage local input, settle at most `max_steps_per_poll`, atomically seal
    /// all cross-worker output, and then publish the staged executor state.
    pub async fn poll(
        &mut self,
        inputs: &[RoutedCausalEvent],
    ) -> Result<ManagedPartialShardPoll, ManagedPartialShardRuntimeError> {
        if inputs.len() > MAX_INPUT_EVENTS {
            return Err(ManagedPartialShardRuntimeError::InputBatchTooLarge {
                actual: inputs.len(),
                max: MAX_INPUT_EVENTS,
            });
        }
        let mut staged = self.executor.clone();
        let mut steps = Vec::new();
        for input in inputs {
            if steps.len() >= self.max_steps_per_poll {
                break;
            }
            staged.admit(input.clone())?;
            self.settle_staged(&mut staged, &mut steps)?;
        }
        self.settle_staged(&mut staged, &mut steps)?;
        self.commit_staged(staged, steps).await
    }

    /// Apply one durably received cross-worker message, settle any resulting
    /// local work, and atomically seal any further remote output.
    pub async fn apply_remote(
        &mut self,
        message: PartialShardOutbound,
    ) -> Result<ManagedPartialShardPoll, ManagedPartialShardRuntimeError> {
        let mut staged = self.executor.clone();
        let PartialShardApply { outbound, .. } = staged.apply_outbound(message)?;
        let mut steps = Vec::new();
        self.settle_staged(&mut staged, &mut steps)?;
        let mut all_outbound = outbound;
        all_outbound.extend(steps.iter().flat_map(|step| step.outbound.iter().cloned()));
        self.commit_staged_with_outbound(staged, steps, all_outbound)
            .await
    }

    /// Flush all currently sealed streams concurrently. The worker state is
    /// already committed before this method performs network I/O.
    pub async fn dispatch_pending(
        &self,
    ) -> Result<StableShardDispatchReport, ManagedPartialShardRuntimeError> {
        Ok(self.dispatcher.dispatch_pending().await?)
    }

    pub fn checkpoint_shards(
        &self,
    ) -> Result<Vec<StableShardCheckpoint>, ManagedPartialShardRuntimeError> {
        Ok(self.executor.checkpoint_shards()?)
    }

    pub fn state_digest(
        &self,
    ) -> Result<crate::deterministic::StateDigest, ManagedPartialShardRuntimeError> {
        Ok(self.executor.state_digest()?)
    }

    fn settle_staged(
        &self,
        staged: &mut PartialShardExecutor,
        steps: &mut Vec<PartialShardStep>,
    ) -> Result<(), ManagedPartialShardRuntimeError> {
        while steps.len() < self.max_steps_per_poll {
            let Some(step) = staged.step()? else { break };
            steps.push(step);
        }
        Ok(())
    }

    async fn commit_staged(
        &mut self,
        staged: PartialShardExecutor,
        steps: Vec<PartialShardStep>,
    ) -> Result<ManagedPartialShardPoll, ManagedPartialShardRuntimeError> {
        let outbound = steps
            .iter()
            .flat_map(|step| step.outbound.iter().cloned())
            .collect::<Vec<_>>();
        self.commit_staged_with_outbound(staged, steps, outbound)
            .await
    }

    async fn commit_staged_with_outbound(
        &mut self,
        staged: PartialShardExecutor,
        steps: Vec<PartialShardStep>,
        outbound: Vec<PartialShardOutbound>,
    ) -> Result<ManagedPartialShardPoll, ManagedPartialShardRuntimeError> {
        let outbound_records = outbound.len();
        if !outbound.is_empty() {
            self.dispatcher.enqueue_batch(outbound).await?;
        }
        let pending_after = staged.total_pending();
        let budget_exhausted = pending_after > 0 && steps.len() >= self.max_steps_per_poll;
        self.executor = staged;
        Ok(ManagedPartialShardPoll {
            steps,
            outbound_records,
            pending_after,
            budget_exhausted,
        })
    }
}
