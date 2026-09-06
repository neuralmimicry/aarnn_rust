//! Local reference causal executor used before distributed transport exists.
//!
//! The executor is intentionally single-threaded and reconstructible. It is
//! the semantic reference for later shard actors; it does not use wall-clock
//! time, queue silence or a process-wide barrier to infer quiescence.

use crate::deterministic::{
    CanonicalEvent, CanonicalEventKey, ComponentId, EventId, EventStage, LogicalTag,
    PrimitiveError, StateDigest, canonical_event_digest,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// A causal event with explicit provenance for later non-convergence deferral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalEvent {
    pub key: CanonicalEventKey,
    pub id: EventId,
    pub payload: Vec<u8>,
    pub original_tag: LogicalTag,
    pub deferred_from_nonconvergence: bool,
}

impl CausalEvent {
    pub fn new(id: EventId, key: CanonicalEventKey, payload: Vec<u8>) -> Self {
        Self {
            original_tag: key.tag,
            key,
            id,
            payload,
            deferred_from_nonconvergence: false,
        }
    }

    fn defer_to(&self, tag: LogicalTag) -> Self {
        let mut deferred = self.clone();
        deferred.key.tag = tag;
        deferred.deferred_from_nonconvergence = true;
        deferred
    }
}

/// Failure while applying a pure model transition.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettlementError {
    #[error("causal queue capacity {capacity} exceeded")]
    QueueFull { capacity: usize },
    #[error("settling limit must be at least one")]
    InvalidSettlingLimit,
    #[error("event admission moved backwards from {current} to {next}")]
    BackwardsAdmission {
        current: LogicalTag,
        next: LogicalTag,
    },
    #[error("model output moved backwards from {current} to {next}")]
    BackwardsOutput {
        current: LogicalTag,
        next: LogicalTag,
    },
    #[error("causal output did not advance from {current} to {next}")]
    NonAdvancingOutput {
        current: LogicalTag,
        next: LogicalTag,
    },
    #[error(
        "same-tick causal output from {current} must advance exactly one microstep, received {next}"
    )]
    InvalidZeroDelayProgression {
        current: LogicalTag,
        next: LogicalTag,
    },
    #[error("positive-delay causal output must start at microstep zero, received {next}")]
    InvalidPositiveDelayProgression { next: LogicalTag },
    #[error("model transition failed: {0}")]
    Model(String),
    #[error("model transition stage {stage:?} is not owned by this processor")]
    UnsupportedStage { stage: EventStage },
    #[error(transparent)]
    Primitive(#[from] PrimitiveError),
}

/// The only model hook required by the local reference interpreter.
pub trait TransitionProcessor {
    /// Prepare one transition without publishing irreversible state.
    fn apply(&mut self, event: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError>;

    /// Publish the transition after outputs and queue capacity have been
    /// validated. Pure processors use the no-op default; stateful kernels
    /// stage private state in `apply` and install it here.
    fn commit(&mut self, _event: &CausalEvent) -> Result<(), SettlementError> {
        Ok(())
    }
}

/// Evidence that a component reached exact local quiescence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvergenceEvidence {
    pub component: ComponentId,
    pub last_tag: Option<LogicalTag>,
    pub committed_events: usize,
}

/// Why exact component closure cannot currently be proved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CausalDependency {
    LocalTransition,
    OutputStaging,
    OutstandingEvents(u64),
    ProducerHorizon { producer: u64, through: LogicalTag },
    UnstableTopology,
    InvalidatedActivityEpoch { expected: u64, observed: u64 },
}

/// Auditable evidence required before an otherwise idle local component may
/// be called quiescent. This is supplied by topology/route ownership; an empty
/// producer set is an explicit local fact, not an inference from silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalClosureProof {
    pub local_transition_complete: bool,
    pub output_staging_empty: bool,
    pub outstanding_events: u64,
    pub expected_producers: BTreeSet<u64>,
    pub producer_horizons: BTreeMap<u64, LogicalTag>,
    pub topology_stable: bool,
    pub activity_epoch: u64,
    pub observed_activity_epoch: u64,
}

impl LocalClosureProof {
    /// Explicit proof for a component with no external producers or staged
    /// output. Callers must not use this constructor for routed components.
    pub fn isolated() -> Self {
        Self {
            local_transition_complete: true,
            output_staging_empty: true,
            outstanding_events: 0,
            expected_producers: BTreeSet::new(),
            producer_horizons: BTreeMap::new(),
            topology_stable: true,
            activity_epoch: 0,
            observed_activity_epoch: 0,
        }
    }

    pub fn dependencies(&self, through: LogicalTag) -> Vec<CausalDependency> {
        let mut dependencies = Vec::new();
        if !self.local_transition_complete {
            dependencies.push(CausalDependency::LocalTransition);
        }
        if !self.output_staging_empty {
            dependencies.push(CausalDependency::OutputStaging);
        }
        if self.outstanding_events != 0 {
            dependencies.push(CausalDependency::OutstandingEvents(self.outstanding_events));
        }
        for producer in &self.expected_producers {
            let horizon = self
                .producer_horizons
                .get(producer)
                .copied()
                .unwrap_or(LogicalTag::ZERO);
            if horizon <= through {
                dependencies.push(CausalDependency::ProducerHorizon {
                    producer: *producer,
                    through: horizon,
                });
            }
        }
        if !self.topology_stable {
            dependencies.push(CausalDependency::UnstableTopology);
        }
        if self.activity_epoch != self.observed_activity_epoch {
            dependencies.push(CausalDependency::InvalidatedActivityEpoch {
                expected: self.activity_epoch,
                observed: self.observed_activity_epoch,
            });
        }
        dependencies
    }
}

/// Evidence-preserving non-convergence result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonConvergenceRecord {
    pub component: ComponentId,
    pub first_tag: LogicalTag,
    pub settling_limit: usize,
    pub completed_microsteps: usize,
    pub unresolved_count: usize,
    pub unresolved_digest: StateDigest,
    pub deferred_tag: LogicalTag,
}

/// Explicit local settlement result. Non-convergence is never reported as
/// quiescence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettlementOutcome {
    Converged(ConvergenceEvidence),
    DeferredNonConvergent {
        record: NonConvergenceRecord,
        unresolved: Vec<CausalEvent>,
    },
    Blocked {
        waiting_on: Vec<CausalDependency>,
    },
    Failed {
        error: SettlementError,
    },
}

/// A bounded component-scoped event executor.
#[derive(Debug, Clone)]
pub struct LocalExecutor {
    component: ComponentId,
    capacity: usize,
    pending: BTreeMap<LogicalTag, Vec<CausalEvent>>,
    committed: Vec<CausalEvent>,
    committed_total: u64,
    last_processed: Option<LogicalTag>,
}

impl LocalExecutor {
    pub fn new(component: ComponentId, capacity: usize) -> Self {
        Self {
            component,
            capacity,
            pending: BTreeMap::new(),
            committed: Vec::new(),
            committed_total: 0,
            last_processed: None,
        }
    }

    pub const fn component(&self) -> ComponentId {
        self.component
    }

    pub fn pending_len(&self) -> usize {
        self.pending.values().map(Vec::len).sum()
    }

    pub fn committed(&self) -> &[CausalEvent] {
        &self.committed
    }

    pub fn committed_count(&self) -> usize {
        self.committed.len()
    }

    /// Total number of events committed since construction, independent of
    /// the bounded diagnostic history retained by [`Self::committed`].
    pub fn committed_total(&self) -> u64 {
        self.committed_total
    }

    /// Admit external work before model execution. Admission is bounded and
    /// rejects stale tags rather than silently dropping them.
    pub fn admit(&mut self, event: CausalEvent) -> Result<(), SettlementError> {
        self.insert_pending(event)
    }

    fn insert_pending(&mut self, event: CausalEvent) -> Result<(), SettlementError> {
        if self.pending_len() >= self.capacity {
            return Err(SettlementError::QueueFull {
                capacity: self.capacity,
            });
        }
        if let Some(current) = self.last_processed {
            if event.key.tag < current {
                return Err(SettlementError::BackwardsAdmission {
                    current,
                    next: event.key.tag,
                });
            }
        }
        self.pending.entry(event.key.tag).or_default().push(event);
        Ok(())
    }

    /// Settle all currently admitted work in canonical order.
    ///
    /// A failed transition leaves the failed event and all later events from
    /// that microstep pending. Implementations of `TransitionProcessor` should
    /// therefore treat `Err` as a pre-commit result and avoid publishing an
    /// irreversible model mutation before returning success.
    pub fn settle<P: TransitionProcessor>(
        &mut self,
        first_tag: LogicalTag,
        settling_limit: usize,
        deferred_tag: LogicalTag,
        processor: &mut P,
    ) -> Result<SettlementOutcome, SettlementError> {
        self.settle_with_closure(
            first_tag,
            settling_limit,
            deferred_tag,
            &LocalClosureProof::isolated(),
            processor,
        )
    }

    /// Settle one biological tick and require explicit closure evidence before
    /// reporting quiescence. Future-tick work remains queued and does not
    /// prevent the current tick from closing.
    pub fn settle_with_closure<P: TransitionProcessor>(
        &mut self,
        first_tag: LogicalTag,
        settling_limit: usize,
        deferred_tag: LogicalTag,
        closure: &LocalClosureProof,
        processor: &mut P,
    ) -> Result<SettlementOutcome, SettlementError> {
        if settling_limit == 0 {
            return Err(SettlementError::InvalidSettlingLimit);
        }
        let mut completed_microsteps = 0;
        let mut last_tag = None;
        let committed_before = self.committed_total;

        while let Some(tag) = self.pending.keys().next().copied() {
            if tag < first_tag {
                return Err(SettlementError::BackwardsAdmission {
                    current: first_tag,
                    next: tag,
                });
            }
            if tag.tick > first_tag.tick {
                break;
            }
            if completed_microsteps == settling_limit {
                let unresolved = self.defer_tick_pending(first_tag.tick, deferred_tag);
                let digest = canonical_event_digest(
                    &unresolved
                        .iter()
                        .map(|event| CanonicalEvent {
                            key: event.key,
                            payload: event.payload.clone(),
                        })
                        .collect::<Vec<_>>(),
                );
                let record = NonConvergenceRecord {
                    component: self.component,
                    first_tag,
                    settling_limit,
                    completed_microsteps,
                    unresolved_count: unresolved.len(),
                    unresolved_digest: digest,
                    deferred_tag,
                };
                return Ok(SettlementOutcome::DeferredNonConvergent { record, unresolved });
            }

            let mut events = self.pending.remove(&tag).unwrap_or_default();
            events.sort_by(|left, right| {
                left.key
                    .cmp(&right.key)
                    .then_with(|| left.id.cmp(&right.id))
                    .then_with(|| left.payload.cmp(&right.payload))
            });
            for (index, event) in events.iter().enumerate() {
                let outputs = match processor.apply(event) {
                    Ok(outputs) => outputs,
                    Err(error) => {
                        self.restore_pending(events[index..].iter().cloned());
                        return Err(error);
                    }
                };
                if let Some(output) = outputs.iter().find(|output| output.key.tag < event.key.tag) {
                    self.restore_pending(events[index..].iter().cloned());
                    return Err(SettlementError::BackwardsOutput {
                        current: event.key.tag,
                        next: output.key.tag,
                    });
                }
                for output in &outputs {
                    if let Err(error) = validate_progression(event.key.tag, output.key.tag) {
                        self.restore_pending(events[index..].iter().cloned());
                        return Err(error);
                    }
                }
                if outputs.len() > self.capacity.saturating_sub(self.pending_len()) {
                    self.restore_pending(events[index..].iter().cloned());
                    return Err(SettlementError::QueueFull {
                        capacity: self.capacity,
                    });
                }
                if let Err(error) = processor.commit(event) {
                    self.restore_pending(events[index..].iter().cloned());
                    return Err(error);
                }
                self.committed_total = self.committed_total.saturating_add(1);
                if self.committed.len() == self.capacity {
                    self.committed.remove(0);
                }
                self.committed.push(event.clone());
                for output in outputs {
                    self.insert_pending(output)?;
                }
            }
            completed_microsteps += 1;
            last_tag = Some(tag);
            self.last_processed = Some(tag);
        }

        let closure_tag = last_tag.unwrap_or(first_tag);
        let waiting_on = closure.dependencies(closure_tag);
        if !waiting_on.is_empty() {
            return Ok(SettlementOutcome::Blocked { waiting_on });
        }

        Ok(SettlementOutcome::Converged(ConvergenceEvidence {
            component: self.component,
            last_tag,
            committed_events: usize::try_from(self.committed_total - committed_before)
                .unwrap_or(usize::MAX),
        }))
    }

    /// Outcome-oriented facade for schedulers that must distinguish blocked,
    /// non-convergent and failed components without conflating failure with
    /// quiescence. Failed transitions remain pending for deterministic retry.
    pub fn settle_outcome<P: TransitionProcessor>(
        &mut self,
        first_tag: LogicalTag,
        settling_limit: usize,
        deferred_tag: LogicalTag,
        closure: &LocalClosureProof,
        processor: &mut P,
    ) -> SettlementOutcome {
        match self.settle_with_closure(first_tag, settling_limit, deferred_tag, closure, processor)
        {
            Ok(outcome) => outcome,
            Err(error) => SettlementOutcome::Failed { error },
        }
    }

    fn defer_tick_pending(
        &mut self,
        capped_tick: u64,
        deferred_tag: LogicalTag,
    ) -> Vec<CausalEvent> {
        let mut unresolved = self
            .pending
            .range(LogicalTag::new(capped_tick, 0)..=LogicalTag::new(capped_tick, u32::MAX))
            .flat_map(|(_, events)| events.iter().map(|event| event.defer_to(deferred_tag)))
            .collect::<Vec<_>>();
        unresolved.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.id.cmp(&right.id))
        });
        self.pending.retain(|tag, _| tag.tick != capped_tick);
        for event in unresolved.iter().cloned() {
            self.pending.entry(event.key.tag).or_default().push(event);
        }
        unresolved
    }

    fn restore_pending(&mut self, events: impl IntoIterator<Item = CausalEvent>) {
        for event in events {
            self.pending.entry(event.key.tag).or_default().push(event);
        }
    }
}

fn validate_progression(current: LogicalTag, next: LogicalTag) -> Result<(), SettlementError> {
    if next <= current {
        return Err(SettlementError::NonAdvancingOutput { current, next });
    }
    if next.tick == current.tick {
        let expected = current.zero_delay()?;
        if next != expected {
            return Err(SettlementError::InvalidZeroDelayProgression { current, next });
        }
    } else if next.microstep != 0 {
        return Err(SettlementError::InvalidPositiveDelayProgression { next });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::{EventStage, ShardId};

    struct StableProcessor;

    impl TransitionProcessor for StableProcessor {
        fn apply(&mut self, _event: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn empty_component_is_converged_without_waiting_for_silence() {
        let component = ComponentId::new(1).unwrap();
        let mut executor = LocalExecutor::new(component, 4);
        let outcome = executor
            .settle(
                LogicalTag::ZERO,
                2,
                LogicalTag::new(1, 0),
                &mut StableProcessor,
            )
            .unwrap();
        assert!(matches!(outcome, SettlementOutcome::Converged(_)));
        let _ = ShardId::new(1).unwrap();
        let _ = EventStage::SpikeDecision;
    }

    fn event(id: u64, tag: LogicalTag) -> CausalEvent {
        CausalEvent::new(
            EventId::new(id).unwrap(),
            CanonicalEventKey::new(tag, EventStage::SynapticTransition, 1, 2, id),
            vec![id as u8],
        )
    }

    #[test]
    fn closure_requires_declared_producer_progress() {
        let mut executor = LocalExecutor::new(ComponentId::new(1).unwrap(), 4);
        let mut proof = LocalClosureProof::isolated();
        proof.expected_producers.insert(7);
        let outcome = executor
            .settle_with_closure(
                LogicalTag::ZERO,
                2,
                LogicalTag::new(1, 0),
                &proof,
                &mut StableProcessor,
            )
            .unwrap();
        assert!(matches!(
            outcome,
            SettlementOutcome::Blocked { ref waiting_on }
                if waiting_on.contains(&CausalDependency::ProducerHorizon {
                    producer: 7,
                    through: LogicalTag::ZERO,
                })
        ));
    }

    #[test]
    fn future_tick_work_does_not_block_current_tick_closure() {
        let mut executor = LocalExecutor::new(ComponentId::new(1).unwrap(), 4);
        executor.admit(event(1, LogicalTag::new(3, 0))).unwrap();
        let outcome = executor
            .settle(
                LogicalTag::new(2, 0),
                2,
                LogicalTag::new(3, 0),
                &mut StableProcessor,
            )
            .unwrap();
        assert!(matches!(outcome, SettlementOutcome::Converged(_)));
        assert_eq!(executor.pending_len(), 1);
        assert_eq!(executor.committed_total(), 0);
    }

    #[test]
    fn non_advancing_model_output_is_a_failed_outcome_and_is_retained() {
        struct InvalidProcessor;
        impl TransitionProcessor for InvalidProcessor {
            fn apply(&mut self, input: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError> {
                Ok(vec![event(2, input.key.tag)])
            }
        }

        let mut executor = LocalExecutor::new(ComponentId::new(1).unwrap(), 4);
        executor.admit(event(1, LogicalTag::ZERO)).unwrap();
        let outcome = executor.settle_outcome(
            LogicalTag::ZERO,
            2,
            LogicalTag::new(1, 0),
            &LocalClosureProof::isolated(),
            &mut InvalidProcessor,
        );
        assert!(matches!(
            outcome,
            SettlementOutcome::Failed {
                error: SettlementError::NonAdvancingOutput { .. }
            }
        ));
        assert_eq!(executor.pending_len(), 1);
        assert_eq!(executor.committed_total(), 0);
    }

    #[test]
    fn output_capacity_failure_retains_each_event_once() {
        struct OverflowProcessor;
        impl TransitionProcessor for OverflowProcessor {
            fn apply(&mut self, input: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError> {
                Ok(vec![
                    event(2, input.key.tag.zero_delay().unwrap()),
                    event(3, input.key.tag.zero_delay().unwrap()),
                ])
            }
        }

        let mut executor = LocalExecutor::new(ComponentId::new(1).unwrap(), 1);
        executor.admit(event(1, LogicalTag::ZERO)).unwrap();
        let outcome = executor.settle_outcome(
            LogicalTag::ZERO,
            2,
            LogicalTag::new(1, 0),
            &LocalClosureProof::isolated(),
            &mut OverflowProcessor,
        );
        assert!(matches!(
            outcome,
            SettlementOutcome::Failed {
                error: SettlementError::QueueFull { capacity: 1 }
            }
        ));
        assert_eq!(executor.pending_len(), 1);
        assert_eq!(executor.committed_total(), 0);
    }
}
