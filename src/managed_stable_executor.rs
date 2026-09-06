//! Explicit managed adapter for the stable-ID durable executor.
//!
//! `ManagedStableExecutor` is the bounded worker-facing seam between a node
//! loop and [`StableExecutorDurableBridge`].  It deliberately accepts only a
//! previously constructed bridge: discovery, placement telemetry, or a
//! compatibility `Runner` can never implicitly grant biological authority.
//!
//! Each input and each queued causal event is processed through the same
//! fenced complete-fabric checkpoint and durable mirror boundary.  Work is
//! bounded by `max_steps_per_poll`; when that limit is reached the result
//! reports remaining queued work so a caller can schedule another poll.  An
//! exhausted budget is never reported as quiescence.

use crate::authoritative_shard::BIOLOGICAL_STATE_SCHEMA_VERSION;
use crate::authoritative_shard::StableTransitionInput;
use crate::causal::CausalEvent;
use crate::data_plane::{
    CausalEnvelope, DataPlaneError, EnvelopeKind, ReceiveResult, ReliableReceiver,
};
use crate::deterministic::{
    CanonicalEventKey, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId, StateDigest,
    StateDigestBuilder,
};
use crate::shard_executor::{RoutedCausalEvent, ShardExecutionResult};
use crate::stable_executor_durable::{StableExecutorDurableBridge, StableExecutorDurableError};
use std::collections::BTreeMap;
use std::fs::File;
use std::sync::Arc;
use thiserror::Error;

/// A committed stable-executor transition exposed to a managed node loop.
///
/// The result carries stable neuron identities and the original routed event;
/// it does not expose dense runner-layer indices as authoritative ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedStableStep {
    pub consumed: RoutedCausalEvent,
    pub fired: Vec<NeuronId>,
    pub emitted: Vec<RoutedCausalEvent>,
    pub logical_tag: LogicalTag,
}

impl From<ShardExecutionResult> for ManagedStableStep {
    fn from(result: ShardExecutionResult) -> Self {
        let logical_tag = result.consumed.event.key.tag;
        Self {
            consumed: result.consumed,
            fired: result.fired,
            emitted: result.emitted,
            logical_tag,
        }
    }
}

/// Outcome of one bounded worker poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedStablePoll {
    pub steps: Vec<ManagedStableStep>,
    pub pending_after: usize,
    pub budget_exhausted: bool,
}

impl ManagedStablePoll {
    pub fn is_quiescent(&self) -> bool {
        self.pending_after == 0 && !self.budget_exhausted
    }
}

#[derive(Debug, Error)]
pub enum ManagedStableExecutorError {
    #[error(transparent)]
    Durable(#[from] StableExecutorDurableError),
    #[error("stable executor input batch has {actual} events, exceeding bound {max}")]
    InputBatchTooLarge { actual: usize, max: usize },
    #[error("stable executor poll budget must be at least one")]
    InvalidPollBudget,
    #[error("stable executor checkpoint ID space is exhausted")]
    CheckpointIdExhausted,
    #[error("stable executor sensory vector has {actual} values, expected {expected}")]
    SensoryInputShapeMismatch { actual: usize, expected: usize },
    #[error("stable executor sensory event ID space is exhausted")]
    SensoryEventIdExhausted,
    #[error("stable executor sensory target mapping is invalid")]
    InvalidSensoryTargets,
    #[error("stable executor authority lock is already held")]
    AuthorityAlreadyHeld,
    #[error("stable executor authority lock failed: {0}")]
    AuthorityLock(String),
    #[error("stable executor causal envelope does not match the active authority: {0}")]
    EnvelopeAuthorityMismatch(&'static str),
    #[error(
        "stable executor causal sequence {sequence} conflicts with the previously accepted event"
    )]
    CausalSequenceConflict { sequence: u64 },
    #[error(transparent)]
    CausalTransport(#[from] DataPlaneError),
    #[error(transparent)]
    Shard(#[from] crate::shard_executor::ShardExecutionError),
}

/// Bounded, explicitly registered runtime for one neural brain.
#[derive(Debug)]
pub struct ManagedStableExecutor {
    bridge: StableExecutorDurableBridge,
    next_checkpoint_id: u64,
    max_input_events: usize,
    max_steps_per_poll: usize,
    sensory_targets: Vec<NeuronId>,
    next_sensory_event_id: u64,
    authority_lock: Option<Arc<File>>,
    /// Receiver cursor for the authenticated external causal stream. It is
    /// deliberately separate from the bridge's internal mirror sequence:
    /// generated same-tick events and incoming transport frames have
    /// different sequence spaces.
    causal_receiver: Option<ReliableReceiver>,
    /// Bounded event identity history used to reject a conflicting replay at
    /// an already acknowledged sequence position.
    causal_event_ids: BTreeMap<u64, (EventId, StateDigest)>,
}

const MAX_CAUSAL_CURSOR_ENTRIES: usize = 8192;

fn causal_payload_digest(envelope: &CausalEnvelope) -> StateDigest {
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("stable-causal-payload", &envelope.payload);
    digest.finish()
}

impl ManagedStableExecutor {
    /// Construct a managed runtime from an already opened durable bridge.
    ///
    /// `initial_checkpoint_id` must be the ID used to publish the bridge's
    /// initial cut. Subsequent operation IDs are allocated monotonically in
    /// this process. A restart must restore the allocator from operation
    /// metadata before accepting work; silently reusing an ID is forbidden by
    /// the immutable checkpoint store.
    pub fn new(
        bridge: StableExecutorDurableBridge,
        initial_checkpoint_id: EventId,
        max_input_events: usize,
        max_steps_per_poll: usize,
    ) -> Result<Self, ManagedStableExecutorError> {
        if max_input_events == 0 || max_steps_per_poll == 0 {
            return Err(ManagedStableExecutorError::InvalidPollBudget);
        }
        let next_checkpoint_id = initial_checkpoint_id
            .raw()
            .checked_add(1)
            .ok_or(ManagedStableExecutorError::CheckpointIdExhausted)?;
        let next_sensory_event_id = bridge
            .executor()
            .event_id_frontier()
            .checked_add(1)
            .ok_or(ManagedStableExecutorError::SensoryEventIdExhausted)?;
        Ok(Self {
            bridge,
            next_checkpoint_id,
            max_input_events,
            max_steps_per_poll,
            sensory_targets: Vec::new(),
            next_sensory_event_id,
            authority_lock: None,
            causal_receiver: None,
            causal_event_ids: BTreeMap::new(),
        })
    }

    /// Retain the process/file authority lock acquired by the bootstrap
    /// boundary. The lock lives as long as the managed runtime and is
    /// released only after that runtime is dropped.
    pub(crate) fn with_authority_lock(mut self, lock: Arc<File>) -> Self {
        self.authority_lock = Some(lock);
        self
    }

    /// Bind a stable neuron mapping for bounded sensory vectors.  The mapping
    /// is deployment configuration and is validated before the runtime can
    /// be registered as authoritative.
    pub fn with_sensory_targets(
        mut self,
        targets: Vec<NeuronId>,
    ) -> Result<Self, ManagedStableExecutorError> {
        let mut seen = std::collections::BTreeSet::new();
        if targets.iter().any(|target| !seen.insert(*target)) {
            return Err(ManagedStableExecutorError::InvalidSensoryTargets);
        }
        self.sensory_targets = targets;
        Ok(self)
    }

    pub fn sensory_target_count(&self) -> usize {
        self.sensory_targets.len()
    }

    pub fn lease_term(&self) -> LeaseTerm {
        self.bridge.authority().term()
    }

    pub fn fencing_token(&self) -> u64 {
        self.bridge.authority().fencing_token()
    }

    /// Return the immutable biological/partition identity opened by this
    /// worker. The orchestrator uses this for registration consistency; it
    /// never treats the observation as a placement or writer grant.
    pub fn registration_identity(
        &self,
        network_id: impl Into<String>,
    ) -> crate::stable_worker::StableWorkerRegistration {
        let executor = self.bridge.executor();
        let plan = executor.plan();
        crate::stable_worker::StableWorkerRegistration {
            schema_version: crate::stable_worker::STABLE_WORKER_REGISTRATION_SCHEMA_VERSION,
            profile: crate::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
            network_id: network_id.into(),
            brain_id: executor.brain_id().raw(),
            topology_generation: plan.topology_generation().raw(),
            partition_generation: plan.partition_generation().raw(),
            topology_digest: plan.topology_digest().to_string(),
            plan_digest: plan.digest().to_string(),
            shard_ids: plan.shard_ids().map(|shard| shard.raw()).collect(),
            // The current stable runtime owns the complete fabric in one
            // process. Partial ownership is intentionally represented in the
            // contract but cannot be produced until remote causal admission
            // and durable handoff are integrated.
            owned_shard_ids: plan.shard_ids().map(|shard| shard.raw()).collect(),
            // A registration with missing actor evidence is rejected by the
            // orchestrator. Do not synthesise acknowledgements when a sealed
            // actor checkpoint cannot be read.
            application_acks: self
                .bridge
                .application_acknowledgements()
                .unwrap_or_default(),
            lease_term: self.lease_term().raw(),
            fencing_token: self.fencing_token(),
            current_tick: executor.current_tag().tick,
            current_microstep: executor.current_tag().microstep,
            state_digest: executor
                .state_digest()
                .map(|digest| digest.to_string())
                .unwrap_or_else(|_| "00".repeat(16)),
            max_input_events: self.max_input_events() as u32,
            max_steps_per_poll: self.max_steps_per_poll() as u32,
            authoritative: true,
        }
    }

    pub fn bridge(&self) -> &StableExecutorDurableBridge {
        &self.bridge
    }

    pub fn bridge_mut(&mut self) -> &mut StableExecutorDurableBridge {
        &mut self.bridge
    }

    pub fn max_input_events(&self) -> usize {
        self.max_input_events
    }

    pub fn max_steps_per_poll(&self) -> usize {
        self.max_steps_per_poll
    }

    pub fn set_max_steps_per_poll(&mut self, max_steps_per_poll: usize) {
        if max_steps_per_poll != 0 {
            self.max_steps_per_poll = max_steps_per_poll;
        }
    }

    /// Process external events and then drain admitted causal work within one
    /// bounded poll. The input slice is borrowed and never retained after the
    /// call; queued work lives in the durable stable executor checkpoint.
    pub fn poll(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        inputs: &[RoutedCausalEvent],
    ) -> Result<ManagedStablePoll, ManagedStableExecutorError> {
        self.poll_with_transport(observed_term, observed_fencing_token, inputs, None)
    }

    fn poll_with_transport(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        inputs: &[RoutedCausalEvent],
        transport: Option<&CausalEnvelope>,
    ) -> Result<ManagedStablePoll, ManagedStableExecutorError> {
        if inputs.len() > self.max_input_events {
            return Err(ManagedStableExecutorError::InputBatchTooLarge {
                actual: inputs.len(),
                max: self.max_input_events,
            });
        }

        let mut steps = Vec::new();
        for (index, input) in inputs.iter().enumerate() {
            if steps.len() >= self.max_steps_per_poll {
                break;
            }
            let checkpoint_id = self.allocate_checkpoint_id()?;
            let result = match transport.filter(|_| index == 0) {
                Some(transport) => self.bridge.admit_and_step_with_transport(
                    observed_term,
                    observed_fencing_token,
                    transport,
                    input.clone(),
                    checkpoint_id,
                )?,
                None => self.bridge.admit_and_step(
                    observed_term,
                    observed_fencing_token,
                    input.clone(),
                    checkpoint_id,
                )?,
            };
            if let Some(result) = result {
                steps.push(result.into());
            }
            self.drain_pending(observed_term, observed_fencing_token, &mut steps)?;
        }

        self.drain_pending(observed_term, observed_fencing_token, &mut steps)?;
        let pending_after = self.bridge.executor().total_pending();
        Ok(ManagedStablePoll {
            budget_exhausted: pending_after > 0 && steps.len() >= self.max_steps_per_poll,
            steps,
            pending_after,
        })
    }

    /// Admit one authenticated causal envelope through the same durable,
    /// fenced boundary as local stable-ID input. The transport adapter must
    /// validate sender identity and stream sequencing before calling this
    /// method; this method binds the biological frame to this brain's current
    /// term and partition generation before any state can mutate.
    pub fn poll_envelope(
        &mut self,
        envelope: &CausalEnvelope,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
    ) -> Result<ManagedStablePoll, ManagedStableExecutorError> {
        if envelope.kind != EnvelopeKind::Event {
            return Err(ManagedStableExecutorError::EnvelopeAuthorityMismatch(
                "envelope kind is not an event",
            ));
        }
        if envelope.schema_version != crate::deterministic::SchemaVersion::CURRENT {
            return Err(ManagedStableExecutorError::EnvelopeAuthorityMismatch(
                "envelope schema is not supported by the stable runtime",
            ));
        }
        if envelope.brain != self.bridge.authority().brain_id() {
            return Err(ManagedStableExecutorError::EnvelopeAuthorityMismatch(
                "brain identity differs from the registered runtime",
            ));
        }
        if envelope.lease_term != observed_term || envelope.lease_term != self.lease_term() {
            return Err(ManagedStableExecutorError::EnvelopeAuthorityMismatch(
                "lease term differs from the observed authority",
            ));
        }
        if envelope.partition_generation != self.bridge.executor().plan().partition_generation() {
            return Err(ManagedStableExecutorError::EnvelopeAuthorityMismatch(
                "partition generation differs from the registered plan",
            ));
        }

        // Stage the receiver and bounded identity history locally. If the
        // biological/durable poll fails, the transport cursor remains at its
        // previous committed boundary and the sender can retry safely.
        let (mut receiver, mut event_ids): (
            ReliableReceiver,
            BTreeMap<u64, (EventId, StateDigest)>,
        ) = if let Some(receiver) = &self.causal_receiver {
            (receiver.clone(), self.causal_event_ids.clone())
        } else {
            let progress = self.bridge.stream_progress(envelope.stream)?;
            let expected_sequence = progress
                .as_ref()
                .map(|progress| progress.next_sequence)
                .unwrap_or(0);
            let mut event_ids: BTreeMap<u64, (EventId, StateDigest)> = progress
                .map(|progress| {
                    progress
                        .entries
                        .into_iter()
                        .map(|(sequence, receipt)| {
                            (sequence, (receipt.event_id, receipt.payload_digest))
                        })
                        .collect()
                })
                .unwrap_or_default();
            while event_ids.len() > MAX_CAUSAL_CURSOR_ENTRIES {
                let Some(oldest) = event_ids.keys().next().copied() else {
                    break;
                };
                event_ids.remove(&oldest);
            }
            (
                ReliableReceiver::from_progress(
                    envelope.brain,
                    envelope.stream,
                    observed_term,
                    envelope.partition_generation,
                    self.bridge.max_payload(),
                    expected_sequence,
                    None,
                )?,
                event_ids,
            )
        };
        let receive_result = receiver.accept(envelope)?;
        let payload_digest = causal_payload_digest(envelope);
        if matches!(receive_result, ReceiveResult::Duplicate { .. }) {
            if event_ids.get(&envelope.sequence) != Some(&(envelope.event, payload_digest)) {
                return Err(ManagedStableExecutorError::CausalSequenceConflict {
                    sequence: envelope.sequence,
                });
            }
            // A replay is already durably represented by the committed cut.
            // Returning an empty poll is essential: replay must not drain a
            // different queued event and make the acknowledgement appear to
            // advance biological work.
            return Ok(ManagedStablePoll {
                steps: Vec::new(),
                pending_after: self.bridge.executor().total_pending(),
                budget_exhausted: false,
            });
        }
        event_ids.insert(envelope.sequence, (envelope.event, payload_digest));
        while event_ids.len() > MAX_CAUSAL_CURSOR_ENTRIES {
            let Some(oldest) = event_ids.keys().next().copied() else {
                break;
            };
            event_ids.remove(&oldest);
        }
        let target = envelope
            .target
            .ok_or(crate::shard_executor::ShardExecutionError::InvalidTarget)?;
        let mut routed = crate::shard_executor::routed_event_from_envelope(envelope)?;
        let plan = self.bridge.executor().plan();
        let target_component = plan.component_for_neuron(target);
        let source_component = envelope
            .source
            .and_then(|source| plan.component_for_neuron(source));
        if envelope.source.is_none() || source_component == target_component {
            // The wire protocol carries a route identity for stream binding,
            // but the deterministic plan represents same-component delivery
            // with no cross-shard route.
            routed.route = None;
        }
        let poll = self.poll_with_transport(
            observed_term,
            observed_fencing_token,
            &[routed],
            Some(envelope),
        )?;
        self.causal_receiver = Some(receiver);
        self.causal_event_ids = event_ids;
        Ok(poll)
    }

    /// Drain already admitted causal work without admitting a new external
    /// event. This is used after a transport batch, after reconnect, and by
    /// tests that prove queued same-tick work crosses the durable boundary.
    pub fn drain(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
    ) -> Result<ManagedStablePoll, ManagedStableExecutorError> {
        let mut steps = Vec::new();
        self.drain_pending(observed_term, observed_fencing_token, &mut steps)?;
        let pending_after = self.bridge.executor().total_pending();
        Ok(ManagedStablePoll {
            budget_exhausted: pending_after > 0 && steps.len() >= self.max_steps_per_poll,
            steps,
            pending_after,
        })
    }

    /// Convert one bounded sensory vector into stable-ID causal transitions
    /// and process it through the same durable poll boundary as network input.
    /// Zero-valued samples are omitted; their position remains represented by
    /// the manifest mapping, so no dense runner index becomes authoritative.
    pub fn poll_sensory(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        sensory: &[i8],
    ) -> Result<ManagedStablePoll, ManagedStableExecutorError> {
        if sensory.len() != self.sensory_targets.len() {
            return Err(ManagedStableExecutorError::SensoryInputShapeMismatch {
                actual: sensory.len(),
                expected: self.sensory_targets.len(),
            });
        }
        let tag = self.bridge.executor().current_tag();
        let sensory_targets = self.sensory_targets.clone();
        let mut inputs = Vec::new();
        for (value, target) in sensory.iter().zip(&sensory_targets) {
            if *value == 0 {
                continue;
            }
            let event_id = self.allocate_sensory_event_id()?;
            let payload = serde_json::to_vec(&StableTransitionInput {
                schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
                source: None,
                target: *target,
                charge: i64::from(*value),
                delay_ticks: 0,
            })
            .map_err(|error| {
                ManagedStableExecutorError::Durable(StableExecutorDurableError::Encoding(
                    error.to_string(),
                ))
            })?;
            inputs.push(RoutedCausalEvent {
                route: None,
                event: CausalEvent {
                    key: CanonicalEventKey::new(
                        tag,
                        EventStage::SynapticTransition,
                        0,
                        target.raw(),
                        event_id.raw(),
                    ),
                    id: event_id,
                    payload,
                    original_tag: tag,
                    deferred_from_nonconvergence: false,
                },
            });
        }
        self.poll(observed_term, observed_fencing_token, &inputs)
    }

    fn drain_pending(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        steps: &mut Vec<ManagedStableStep>,
    ) -> Result<(), ManagedStableExecutorError> {
        while steps.len() < self.max_steps_per_poll {
            if self.bridge.executor().total_pending() == 0 {
                break;
            }
            let checkpoint_id = self.allocate_checkpoint_id()?;
            let Some(result) =
                self.bridge
                    .step_pending(observed_term, observed_fencing_token, checkpoint_id)?
            else {
                break;
            };
            steps.push(result.into());
        }
        Ok(())
    }

    fn allocate_checkpoint_id(&mut self) -> Result<EventId, ManagedStableExecutorError> {
        let id = EventId::new(self.next_checkpoint_id)
            .map_err(|_| ManagedStableExecutorError::CheckpointIdExhausted)?;
        self.next_checkpoint_id = self
            .next_checkpoint_id
            .checked_add(1)
            .ok_or(ManagedStableExecutorError::CheckpointIdExhausted)?;
        Ok(id)
    }

    fn allocate_sensory_event_id(&mut self) -> Result<EventId, ManagedStableExecutorError> {
        let id = EventId::new(self.next_sensory_event_id)
            .map_err(|_| ManagedStableExecutorError::SensoryEventIdExhausted)?;
        self.next_sensory_event_id = self
            .next_sensory_event_id
            .checked_add(1)
            .ok_or(ManagedStableExecutorError::SensoryEventIdExhausted)?;
        Ok(id)
    }
}
