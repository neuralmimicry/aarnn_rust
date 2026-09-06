//! Deterministic multi-shard biological execution fabric.
//!
//! This module is the first executable owner boundary between the stable-ID
//! topology planner and the distributed transport.  Each virtual shard owns
//! its local neuron state and the synapse fields assigned to it by the
//! compiled plan.  Causal work is admitted to the shard that owns its target
//! neuron, processed in canonical `(LogicalTag, EventStage, source, target,
//! event)` order, and emitted through a route only after the local transition
//! has succeeded.
//!
//! The fabric is intentionally transport-neutral and deterministic.  It does
//! not open sockets, use wall-clock time, or infer quiescence from an empty
//! queue.  A server adapter can place each [`StableShardExecutor`] state in an
//! [`crate::authoritative_shard::AuthoritativeShard`] once the distributed
//! durable actor path is promoted.  Until then this bounded in-memory fabric
//! supplies a complete multi-shard reference execution and fault boundary.

use crate::authoritative_shard::{
    BIOLOGICAL_STATE_SCHEMA_VERSION, FIXED_POINT_SCALE, ShardState, StableBiologicalState,
    StableBiologyError, StableNeuronState, StableSynapseState, StableTransitionInput,
};
use crate::causal::CausalEvent;
use crate::data_plane::{CausalEnvelope, EnvelopeKind};
use crate::deterministic::{
    BrainId, CanonicalEventKey, EventId, EventStage, LogicalTag, NeuronId, PrimitiveError, RouteId,
    SchemaVersion, ShardId, StateDigest, StateDigestBuilder,
};
use crate::durability::{ReceiptLedger, ShardCheckpointPayload};
use crate::topology_model::{CompiledExecutionPlan, ExecutionPlanError, TopologyGenerationModel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const SHARD_EXECUTOR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedCausalEvent {
    pub route: Option<RouteId>,
    pub event: CausalEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionOutcome {
    Accepted { shard: ShardId },
    Duplicate { shard: ShardId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardExecutionResult {
    pub consumed: RoutedCausalEvent,
    pub fired: Vec<NeuronId>,
    pub emitted: Vec<RoutedCausalEvent>,
}

/// Convert a validated causal envelope into the transport-neutral event used
/// by the stable executor. Lease, generation and brain checks remain the
/// responsibility of the managed durable boundary; this helper only preserves
/// the canonical biological identity and route.
pub fn routed_event_from_envelope(
    envelope: &CausalEnvelope,
) -> Result<RoutedCausalEvent, ShardExecutionError> {
    if envelope.kind != EnvelopeKind::Event {
        return Err(ShardExecutionError::UnsupportedEnvelopeKind);
    }
    let target = envelope.target.ok_or(ShardExecutionError::InvalidTarget)?;
    let source = envelope.source.map(NeuronId::raw).unwrap_or(0);
    Ok(RoutedCausalEvent {
        route: Some(envelope.route),
        event: CausalEvent {
            key: CanonicalEventKey::new(
                envelope.tag,
                envelope.stage,
                source,
                target.raw(),
                envelope.event.raw(),
            ),
            id: envelope.event,
            payload: envelope.payload.clone(),
            original_tag: envelope.tag,
            deferred_from_nonconvergence: envelope.deferred_from_nonconvergence,
        },
    })
}

/// A deterministic, generation-bound checkpoint for one virtual shard.
///
/// The checkpoint contains the local biological bytes plus every causal item
/// needed to reconstruct the fabric: pending work, the admitted deduplication
/// window and consumed event history.  `fabric_digest` binds sibling shard
/// checkpoints to one consistent cut; a destination must not combine pieces
/// from different cuts or partition plans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableShardCheckpoint {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub shard_id: ShardId,
    pub topology_generation: crate::deterministic::TopologyGeneration,
    pub partition_generation: crate::deterministic::PartitionGeneration,
    pub plan_digest: StateDigest,
    pub fabric_digest: StateDigest,
    pub queue_capacity: usize,
    pub dedupe_capacity: usize,
    pub current_tag: LogicalTag,
    pub biological_state: Vec<u8>,
    pub pending: Vec<RoutedCausalEvent>,
    pub admitted: Vec<RoutedCausalEvent>,
    pub committed: Vec<RoutedCausalEvent>,
    pub checkpoint_digest: StateDigest,
}

impl StableShardCheckpoint {
    pub fn verify(&self) -> Result<(), ShardExecutionError> {
        verify_checkpoint(self)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ShardExecutionError {
    #[error("shard executor schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("causal envelope belongs to brain {actual}, expected {expected}")]
    BrainMismatch { expected: BrainId, actual: BrainId },
    #[error("causal envelope kind is not an event")]
    UnsupportedEnvelopeKind,
    #[error("causal event target is missing or invalid")]
    InvalidTarget,
    #[error("causal event source {0} is not present in the compiled topology")]
    UnknownSource(NeuronId),
    #[error("causal event target {0} is not present in the compiled topology")]
    UnknownTarget(NeuronId),
    #[error("compiled plan has no runtime state for shard {0}")]
    UnknownShard(ShardId),
    #[error("shard {shard} pending queue reached its bound {capacity}")]
    QueueFull { shard: ShardId, capacity: usize },
    #[error("shard executor deduplication window reached its bound {capacity}")]
    DedupeWindowFull { capacity: usize },
    #[error("event {event} was reused with different route or payload")]
    ConflictingDuplicate { event: EventId },
    #[error("event tag {received} precedes executor tag {current}")]
    BackwardsTag {
        current: LogicalTag,
        received: LogicalTag,
    },
    #[error(
        "synapse {synapse} has split mutable-field ownership, which this kernel cannot safely combine"
    )]
    SplitSynapseOwnership {
        synapse: crate::deterministic::SynapseId,
    },
    #[error("generated child event ID {0} collides with an existing event")]
    EventIdCollision(EventId),
    #[error("settle budget must be at least one")]
    InvalidSettleBudget,
    #[error("stable biological transition failed: {0}")]
    Biology(#[from] StableBiologyError),
    #[error("execution plan rejected the event: {0}")]
    Plan(#[from] ExecutionPlanError),
    #[error("stable primitive rejected the event: {0}")]
    Primitive(#[from] PrimitiveError),
    #[error("stable transition encoding failed: {0}")]
    Encoding(String),
    #[error("stable shard checkpoint is invalid: {0}")]
    InvalidCheckpoint(&'static str),
    #[error("stable shard checkpoint plan digest does not match the active plan")]
    CheckpointPlanMismatch,
    #[error("stable shard checkpoint set does not belong to one fabric cut")]
    CheckpointFabricMismatch,
}

/// A bounded, deterministic executor for all virtual shards in one compiled
/// partition.  The executor owns no transport or physical-placement state;
/// those concerns remain in the orchestrator and worker adapters.
#[derive(Debug, Clone)]
pub struct StableShardExecutor {
    brain_id: BrainId,
    plan: CompiledExecutionPlan,
    shards: BTreeMap<ShardId, StableBiologicalState>,
    pending: BTreeMap<ShardId, BTreeMap<LogicalTag, Vec<RoutedCausalEvent>>>,
    admitted: BTreeMap<EventId, RoutedCausalEvent>,
    committed: Vec<RoutedCausalEvent>,
    queue_capacity: usize,
    dedupe_capacity: usize,
    current_tag: LogicalTag,
}

impl StableShardExecutor {
    /// Build one local state per planned virtual shard.  Synapses are stored
    /// only at their terminal owner, while endpoint IDs may refer to neurons
    /// owned by another shard.  A plan that splits the mutable fields of one
    /// synapse is rejected until a field-granular state representation exists;
    /// refusing it preserves the single-authoritative-owner invariant.
    pub fn from_topology(
        brain_id: BrainId,
        topology: &TopologyGenerationModel,
        plan: CompiledExecutionPlan,
        threshold: i64,
        weight: i64,
        queue_capacity: usize,
        dedupe_capacity: usize,
    ) -> Result<Self, ShardExecutionError> {
        if queue_capacity == 0 || dedupe_capacity == 0 {
            return Err(ShardExecutionError::InvalidSettleBudget);
        }
        for ownership in plan.ownership_records() {
            if ownership.terminal_owner != ownership.weight_owner
                || ownership.terminal_owner != ownership.release_owner
                || ownership.terminal_owner != ownership.plasticity_owner
            {
                return Err(ShardExecutionError::SplitSynapseOwnership {
                    synapse: ownership.synapse,
                });
            }
        }

        let known_neurons = topology
            .neurons()
            .map(|neuron| neuron.id)
            .collect::<BTreeSet<_>>();
        let mut shards = BTreeMap::new();
        let mut pending = BTreeMap::new();
        for assignment in plan.assignments() {
            let shard = assignment.shard;
            let neurons = topology
                .neurons()
                .filter(|neuron| plan.neuron_owner(neuron.id) == Some(shard))
                .map(|neuron| StableNeuronState {
                    id: neuron.id,
                    membrane: 0,
                    threshold,
                    refractory_until: LogicalTag::ZERO,
                    adaptation: 0,
                })
                .collect::<Vec<_>>();
            let synapses = topology
                .synapses()
                .filter(|synapse| {
                    plan.ownership(synapse.id)
                        .is_some_and(|owner| owner.terminal_owner == shard)
                })
                .map(|synapse| StableSynapseState {
                    id: synapse.id,
                    source: synapse.source,
                    target: synapse.target,
                    weight,
                    delay_ticks: synapse.delay_ticks,
                    release_state: FIXED_POINT_SCALE,
                    plasticity_trace: 0,
                })
                .collect::<Vec<_>>();
            let state = StableBiologicalState::new_shard(
                topology.generation,
                neurons,
                synapses,
                known_neurons.iter().copied(),
            )?;
            if shards.insert(shard, state).is_some() {
                return Err(ShardExecutionError::UnknownShard(shard));
            }
            pending.insert(shard, BTreeMap::new());
        }

        Ok(Self {
            brain_id,
            plan,
            shards,
            pending,
            admitted: BTreeMap::new(),
            committed: Vec::new(),
            queue_capacity,
            dedupe_capacity,
            current_tag: LogicalTag::ZERO,
        })
    }

    pub fn brain_id(&self) -> BrainId {
        self.brain_id
    }

    pub fn plan(&self) -> &CompiledExecutionPlan {
        &self.plan
    }

    pub fn current_tag(&self) -> LogicalTag {
        self.current_tag
    }

    pub fn pending_count(&self, shard: ShardId) -> Result<usize, ShardExecutionError> {
        let queues = self
            .pending
            .get(&shard)
            .ok_or(ShardExecutionError::UnknownShard(shard))?;
        Ok(queues.values().map(Vec::len).sum())
    }

    pub fn total_pending(&self) -> usize {
        self.pending
            .values()
            .flat_map(|queues| queues.values())
            .map(Vec::len)
            .sum()
    }

    pub fn admitted_count(&self) -> usize {
        self.admitted.len()
    }

    /// Return the largest event identity retained by the executor.  A
    /// restart-safe input adapter uses this frontier to allocate new external
    /// event IDs without reusing an ID already present in the immutable cut.
    pub fn event_id_frontier(&self) -> u64 {
        self.admitted
            .keys()
            .map(|event| event.raw())
            .chain(self.committed.iter().map(|event| event.event.id.raw()))
            .chain(
                self.pending
                    .values()
                    .flat_map(|queues| queues.values().flatten())
                    .map(|event| event.event.id.raw()),
            )
            .max()
            .unwrap_or(0)
    }

    pub fn shard_state(
        &self,
        shard: ShardId,
    ) -> Result<&StableBiologicalState, ShardExecutionError> {
        self.shards
            .get(&shard)
            .ok_or(ShardExecutionError::UnknownShard(shard))
    }

    pub fn shard_state_bytes(&self, shard: ShardId) -> Result<Vec<u8>, ShardExecutionError> {
        Ok(self.shard_state(shard)?.encode()?)
    }

    /// Convert a generated transport envelope into a transport-neutral event
    /// and admit it through the same plan and queue checks as local work.
    pub fn admit_envelope(
        &mut self,
        envelope: &CausalEnvelope,
    ) -> Result<AdmissionOutcome, ShardExecutionError> {
        if envelope.schema_version != SchemaVersion::CURRENT {
            return Err(ShardExecutionError::UnsupportedSchema(u32::from(
                envelope.schema_version.raw(),
            )));
        }
        if envelope.brain != self.brain_id {
            return Err(ShardExecutionError::BrainMismatch {
                expected: self.brain_id,
                actual: envelope.brain,
            });
        }
        self.admit(routed_event_from_envelope(envelope)?)
    }

    /// Admit one event before any biological state is mutated.  Exact
    /// retransmission is idempotent; reuse of an event ID with changed bytes
    /// or route is rejected.
    pub fn admit(
        &mut self,
        routed: RoutedCausalEvent,
    ) -> Result<AdmissionOutcome, ShardExecutionError> {
        if let Some(previous) = self.admitted.get(&routed.event.id) {
            if previous == &routed {
                let shard = self.target_shard(&routed.event)?;
                return Ok(AdmissionOutcome::Duplicate { shard });
            }
            return Err(ShardExecutionError::ConflictingDuplicate {
                event: routed.event.id,
            });
        }
        if self.admitted.len() >= self.dedupe_capacity {
            return Err(ShardExecutionError::DedupeWindowFull {
                capacity: self.dedupe_capacity,
            });
        }
        if routed.event.key.tag < self.current_tag {
            return Err(ShardExecutionError::BackwardsTag {
                current: self.current_tag,
                received: routed.event.key.tag,
            });
        }

        let shard = self.validate_route_and_target(&routed)?;
        if self.pending_count(shard)? >= self.queue_capacity {
            return Err(ShardExecutionError::QueueFull {
                shard,
                capacity: self.queue_capacity,
            });
        }
        let tag = routed.event.key.tag;
        self.admitted.insert(routed.event.id, routed.clone());
        let queues = self
            .pending
            .get_mut(&shard)
            .expect("validated shard has a pending queue");
        queues.entry(tag).or_default().push(routed);
        if let Some(events) = queues.get_mut(&tag) {
            events.sort_by(|left, right| left.event.key.cmp(&right.event.key));
        }
        Ok(AdmissionOutcome::Accepted { shard })
    }

    /// Process at most one globally canonical event.  The full fabric is
    /// cloned before the transition so queue overflow, malformed output or a
    /// failed owner mutation restores every shard atomically.
    pub fn step(&mut self) -> Result<Option<ShardExecutionResult>, ShardExecutionError> {
        let Some((shard_id, tag)) = self.peek_next() else {
            return Ok(None);
        };
        let before = self.clone();
        match self.step_one(shard_id, tag) {
            Ok(result) => Ok(Some(result)),
            Err(error) => {
                *self = before;
                Err(error)
            }
        }
    }

    fn step_one(
        &mut self,
        shard_id: ShardId,
        tag: LogicalTag,
    ) -> Result<ShardExecutionResult, ShardExecutionError> {
        let routed = self.pop_next(shard_id, tag)?;
        let target = self.target_neuron(&routed.event)?;
        let state = self
            .shards
            .get_mut(&shard_id)
            .ok_or(ShardExecutionError::UnknownShard(shard_id))?;
        let transition = state.apply_neuron_event(&routed.event)?;

        let incoming_synapses = self
            .shards
            .iter()
            .flat_map(|(owner, state)| {
                state
                    .synapses()
                    .iter()
                    .filter(move |synapse| synapse.target == target)
                    .map(move |synapse| (*owner, synapse.id))
            })
            .collect::<Vec<_>>();
        for (owner, synapse) in incoming_synapses {
            self.shards
                .get_mut(&owner)
                .expect("synapse owner was collected from the shard map")
                .apply_synapse_effect(synapse, transition_charge(&routed.event))?;
        }

        let mut emitted = Vec::new();
        for fired in &transition.fired {
            let outgoing = self
                .shards
                .iter()
                .flat_map(|(owner, state)| {
                    state
                        .synapses_from(*fired)
                        .cloned()
                        .map(move |synapse| (*owner, synapse))
                })
                .collect::<Vec<_>>();
            for (_terminal_owner, synapse) in outgoing {
                let child_tag = routed.event.key.tag.advance(synapse.delay_ticks)?;
                let child_id =
                    derive_child_event_id(routed.event.id, synapse.id, child_tag, synapse.target)?;
                if self.admitted.contains_key(&child_id) {
                    return Err(ShardExecutionError::EventIdCollision(child_id));
                }
                let payload = serde_json::to_vec(&StableTransitionInput {
                    schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
                    source: Some(*fired),
                    target: synapse.target,
                    charge: synapse.weight / FIXED_POINT_SCALE,
                    delay_ticks: 0,
                })
                .map_err(|error| ShardExecutionError::Encoding(error.to_string()))?;
                let event = CausalEvent {
                    key: CanonicalEventKey::new(
                        child_tag,
                        EventStage::SynapticTransition,
                        fired.raw(),
                        synapse.target.raw(),
                        child_id.raw(),
                    ),
                    id: child_id,
                    payload,
                    original_tag: child_tag,
                    deferred_from_nonconvergence: false,
                };
                let route = if self.plan.component_for_neuron(*fired)
                    == self.plan.component_for_neuron(synapse.target)
                {
                    None
                } else {
                    Some(
                        self.plan
                            .route_for_synapse(synapse.id)
                            .ok_or(ShardExecutionError::Plan(
                                ExecutionPlanError::InvalidRoute {
                                    route: RouteId::new(synapse.id.raw())?,
                                    from: self
                                        .plan
                                        .component_for_neuron(*fired)
                                        .ok_or(ShardExecutionError::UnknownSource(*fired))?,
                                    to: self.plan.component_for_neuron(synapse.target).ok_or(
                                        ShardExecutionError::UnknownTarget(synapse.target),
                                    )?,
                                },
                            ))?
                            .id,
                    )
                };
                let child = RoutedCausalEvent { route, event };
                self.admit(child.clone())?;
                emitted.push(child);
            }
        }

        self.current_tag = self.current_tag.max(routed.event.key.tag);
        self.committed.push(routed.clone());
        Ok(ShardExecutionResult {
            consumed: routed,
            fired: transition.fired,
            emitted,
        })
    }

    /// Settle a bounded number of events.  An empty result means there is no
    /// admitted work; it is not a proof of distributed quiescence.
    pub fn settle(
        &mut self,
        max_events: usize,
    ) -> Result<Vec<ShardExecutionResult>, ShardExecutionError> {
        if max_events == 0 {
            return Err(ShardExecutionError::InvalidSettleBudget);
        }
        let mut results = Vec::new();
        while results.len() < max_events {
            let Some(result) = self.step()? else { break };
            results.push(result);
        }
        Ok(results)
    }

    /// Digest all authoritative shard bytes and queued causal work in a
    /// shard-ID/key-independent canonical order.
    pub fn state_digest(&self) -> Result<StateDigest, ShardExecutionError> {
        let mut digest = StateDigestBuilder::default();
        digest.add_domain(
            "executor-schema",
            SHARD_EXECUTOR_SCHEMA_VERSION.to_be_bytes(),
        );
        digest.add_domain("brain", self.brain_id.raw().to_be_bytes());
        digest.add_domain(
            "topology",
            self.plan.topology_generation().raw().to_be_bytes(),
        );
        digest.add_domain(
            "partition",
            self.plan.partition_generation().raw().to_be_bytes(),
        );
        digest.add_domain("current-tag", {
            let mut bytes = Vec::with_capacity(12);
            bytes.extend_from_slice(&self.current_tag.tick.to_be_bytes());
            bytes.extend_from_slice(&self.current_tag.microstep.to_be_bytes());
            bytes
        });
        let mut admitted = Vec::new();
        for routed in self.admitted.values() {
            append_event_digest_bytes(&mut admitted, routed);
        }
        digest.add_domain("dedupe-window", admitted);
        for (shard, state) in &self.shards {
            digest.add_domain(format!("shard:{shard}:state"), state.encode()?);
            let mut pending = Vec::new();
            if let Some(queues) = self.pending.get(shard) {
                for (tag, events) in queues {
                    pending.extend_from_slice(&tag.tick.to_be_bytes());
                    pending.extend_from_slice(&tag.microstep.to_be_bytes());
                    for routed in events {
                        append_event_digest_bytes(&mut pending, routed);
                    }
                }
            }
            digest.add_domain(format!("shard:{shard}:pending"), pending);
        }
        Ok(digest.finish())
    }

    /// Export one immutable checkpoint per virtual shard.  Each returned
    /// checkpoint is independently self-digested, while `fabric_digest`
    /// prevents a caller from assembling a brain from different cuts.
    pub fn checkpoint_shards(&self) -> Result<Vec<StableShardCheckpoint>, ShardExecutionError> {
        let fabric_digest = self.state_digest()?;
        let plan_digest = self.plan.digest();
        let shard_ids = self.plan.shard_ids().collect::<Vec<_>>();
        let mut checkpoints = Vec::with_capacity(shard_ids.len());
        for shard_id in shard_ids {
            let state = self
                .shards
                .get(&shard_id)
                .ok_or(ShardExecutionError::UnknownShard(shard_id))?;
            let mut pending = self
                .pending
                .get(&shard_id)
                .into_iter()
                .flat_map(|queues| queues.values().flatten().cloned())
                .collect::<Vec<_>>();
            pending.sort_by(|left, right| left.event.key.cmp(&right.event.key));

            let mut admitted = Vec::new();
            for routed in self.admitted.values() {
                if self.target_shard(&routed.event)? == shard_id {
                    admitted.push(routed.clone());
                }
            }
            admitted.sort_by(|left, right| left.event.key.cmp(&right.event.key));

            let mut committed = Vec::new();
            for routed in &self.committed {
                if self.target_shard(&routed.event)? == shard_id {
                    committed.push(routed.clone());
                }
            }
            committed.sort_by(|left, right| left.event.key.cmp(&right.event.key));

            let checkpoint = StableShardCheckpoint {
                schema_version: SHARD_EXECUTOR_SCHEMA_VERSION,
                brain_id: self.brain_id,
                shard_id,
                topology_generation: self.plan.topology_generation(),
                partition_generation: self.plan.partition_generation(),
                plan_digest,
                fabric_digest,
                queue_capacity: self.queue_capacity,
                dedupe_capacity: self.dedupe_capacity,
                current_tag: self.current_tag,
                biological_state: state.encode()?,
                pending,
                admitted,
                committed,
                checkpoint_digest: StateDigest([0; 16]),
            };
            checkpoints.push(seal_checkpoint(checkpoint)?);
        }
        Ok(checkpoints)
    }

    /// Restore a complete executor from a sibling-consistent checkpoint set.
    /// The caller supplies the independently validated compiled plan; the
    /// checkpoint can never smuggle in a different ownership or route map.
    pub fn restore_from_checkpoints(
        brain_id: BrainId,
        plan: CompiledExecutionPlan,
        checkpoints: Vec<StableShardCheckpoint>,
    ) -> Result<Self, ShardExecutionError> {
        if checkpoints.is_empty() {
            return Err(ShardExecutionError::InvalidCheckpoint(
                "at least one shard checkpoint is required",
            ));
        }
        let expected_plan_digest = plan.digest();
        let expected_shards = plan.shard_ids().collect::<BTreeSet<_>>();
        let mut shards = BTreeMap::new();
        let mut pending = BTreeMap::new();
        let mut admitted = BTreeMap::new();
        let mut committed = Vec::new();
        let mut queue_capacity = None;
        let mut dedupe_capacity = None;
        let mut current_tag = None;
        let mut fabric_digest = None;

        for checkpoint in checkpoints {
            verify_checkpoint(&checkpoint)?;
            if checkpoint.schema_version != SHARD_EXECUTOR_SCHEMA_VERSION
                || checkpoint.brain_id != brain_id
                || checkpoint.plan_digest != expected_plan_digest
                || checkpoint.topology_generation != plan.topology_generation()
                || checkpoint.partition_generation != plan.partition_generation()
            {
                return Err(ShardExecutionError::CheckpointPlanMismatch);
            }
            let state = StableBiologicalState::decode(&checkpoint.biological_state)?;
            if state.topology_generation() != checkpoint.topology_generation {
                return Err(ShardExecutionError::CheckpointPlanMismatch);
            }
            if !expected_shards.contains(&checkpoint.shard_id)
                || shards.insert(checkpoint.shard_id, state).is_some()
            {
                return Err(ShardExecutionError::InvalidCheckpoint(
                    "checkpoint shard set is incomplete or duplicated",
                ));
            }
            if queue_capacity
                .replace(checkpoint.queue_capacity)
                .is_some_and(|value| value != checkpoint.queue_capacity)
                || dedupe_capacity
                    .replace(checkpoint.dedupe_capacity)
                    .is_some_and(|value| value != checkpoint.dedupe_capacity)
                || current_tag
                    .replace(checkpoint.current_tag)
                    .is_some_and(|value| value != checkpoint.current_tag)
            {
                return Err(ShardExecutionError::InvalidCheckpoint(
                    "checkpoint capacity or logical tag differs across shards",
                ));
            }
            if fabric_digest
                .replace(checkpoint.fabric_digest)
                .is_some_and(|value| value != checkpoint.fabric_digest)
            {
                return Err(ShardExecutionError::CheckpointFabricMismatch);
            }

            let shard_id = checkpoint.shard_id;
            let mut shard_pending = BTreeMap::<LogicalTag, Vec<RoutedCausalEvent>>::new();
            for routed in checkpoint.pending {
                let target_shard = target_shard_for_plan(&plan, &routed)?;
                if target_shard != shard_id {
                    return Err(ShardExecutionError::InvalidCheckpoint(
                        "pending event is owned by another shard",
                    ));
                }
                shard_pending
                    .entry(routed.event.key.tag)
                    .or_default()
                    .push(routed);
            }
            if shard_pending.values().map(Vec::len).sum::<usize>() > checkpoint.queue_capacity {
                return Err(ShardExecutionError::QueueFull {
                    shard: shard_id,
                    capacity: checkpoint.queue_capacity,
                });
            }
            for events in shard_pending.values_mut() {
                events.sort_by(|left, right| left.event.key.cmp(&right.event.key));
            }
            pending.insert(shard_id, shard_pending);

            for routed in checkpoint.admitted {
                if target_shard_for_plan(&plan, &routed)? != shard_id {
                    return Err(ShardExecutionError::InvalidCheckpoint(
                        "admitted event is owned by another shard",
                    ));
                }
                if admitted.insert(routed.event.id, routed).is_some() {
                    return Err(ShardExecutionError::InvalidCheckpoint(
                        "deduplication window repeats an event ID",
                    ));
                }
            }
            committed.extend(checkpoint.committed);
        }

        if shards.keys().copied().collect::<BTreeSet<_>>() != expected_shards {
            return Err(ShardExecutionError::InvalidCheckpoint(
                "checkpoint shard set does not match the active plan",
            ));
        }
        let executor = Self {
            brain_id,
            plan,
            shards,
            pending,
            admitted,
            committed,
            queue_capacity: queue_capacity.ok_or(ShardExecutionError::InvalidCheckpoint(
                "missing queue capacity",
            ))?,
            dedupe_capacity: dedupe_capacity.ok_or(ShardExecutionError::InvalidCheckpoint(
                "missing deduplication capacity",
            ))?,
            current_tag: current_tag.ok_or(ShardExecutionError::InvalidCheckpoint(
                "missing current logical tag",
            ))?,
        };
        if executor.admitted.len() > executor.dedupe_capacity {
            return Err(ShardExecutionError::DedupeWindowFull {
                capacity: executor.dedupe_capacity,
            });
        }
        for routed in executor.admitted.values() {
            executor.validate_route_and_target(routed)?;
        }
        for routed in &executor.committed {
            target_shard_for_plan(&executor.plan, routed)?;
        }
        for queues in executor.pending.values() {
            for routed in queues.values().flatten() {
                if executor.admitted.get(&routed.event.id) != Some(routed) {
                    return Err(ShardExecutionError::InvalidCheckpoint(
                        "pending event is absent from the deduplication window",
                    ));
                }
            }
        }
        if executor.state_digest()? != fabric_digest.expect("checkpoint digest present") {
            return Err(ShardExecutionError::CheckpointFabricMismatch);
        }
        Ok(executor)
    }

    /// Encode each stable shard as the repository's durable `ShardState`
    /// envelope. The stable checkpoint remains the biological payload while
    /// the outer envelope supplies lease, generation and immutable-checkpoint
    /// integrity required by migration transfer.
    pub fn shard_states(
        &self,
        lease_term: crate::deterministic::LeaseTerm,
        durable_wal_sequence: Option<u64>,
    ) -> Result<Vec<ShardState>, ShardExecutionError> {
        self.checkpoint_shards()?
            .into_iter()
            .map(|checkpoint| {
                let biological_state = serde_json::to_vec(&checkpoint)
                    .map_err(|error| ShardExecutionError::Encoding(error.to_string()))?;
                let payload = ShardCheckpointPayload::new(
                    checkpoint.brain_id,
                    checkpoint.shard_id,
                    checkpoint.topology_generation,
                    checkpoint.partition_generation,
                    lease_term,
                    checkpoint.current_tag,
                    checkpoint.current_tag,
                    durable_wal_sequence,
                    biological_state,
                    serde_json::to_vec(&Vec::<crate::durability::WalRecord>::new())
                        .map_err(|error| ShardExecutionError::Encoding(error.to_string()))?,
                    Vec::new(),
                    ReceiptLedger::default(),
                )
                .seal()
                .map_err(|error| ShardExecutionError::Encoding(error.to_string()))?;
                payload
                    .try_into()
                    .map_err(|error: crate::durability::DurabilityError| {
                        ShardExecutionError::Encoding(error.to_string())
                    })
            })
            .collect()
    }

    /// Reconstruct the stable executor from durable shard envelopes after a
    /// verified transfer or restart. Every outer state and inner checkpoint
    /// is checked before the sibling-cut and plan checks in
    /// [`Self::restore_from_checkpoints`].
    pub fn restore_from_shard_states(
        brain_id: BrainId,
        plan: CompiledExecutionPlan,
        states: Vec<ShardState>,
    ) -> Result<Self, ShardExecutionError> {
        let mut checkpoints = Vec::with_capacity(states.len());
        for state in states {
            state
                .verify()
                .map_err(|error| ShardExecutionError::Encoding(error.to_string()))?;
            if state.brain_id != brain_id {
                return Err(ShardExecutionError::BrainMismatch {
                    expected: brain_id,
                    actual: state.brain_id,
                });
            }
            let checkpoint: StableShardCheckpoint = serde_json::from_slice(&state.biological_state)
                .map_err(|error| ShardExecutionError::Encoding(error.to_string()))?;
            if checkpoint.brain_id != state.brain_id
                || checkpoint.shard_id != state.shard_id
                || checkpoint.topology_generation != state.topology_generation
                || checkpoint.partition_generation != state.partition_generation
                || checkpoint.current_tag != state.committed_tag
            {
                return Err(ShardExecutionError::InvalidCheckpoint(
                    "durable envelope and stable checkpoint metadata differ",
                ));
            }
            checkpoints.push(checkpoint);
        }
        Self::restore_from_checkpoints(brain_id, plan, checkpoints)
    }

    fn target_shard(&self, event: &CausalEvent) -> Result<ShardId, ShardExecutionError> {
        let target = self.target_neuron(event)?;
        self.plan
            .neuron_owner(target)
            .ok_or(ShardExecutionError::UnknownTarget(target))
    }

    fn target_neuron(&self, event: &CausalEvent) -> Result<NeuronId, ShardExecutionError> {
        NeuronId::new(event.key.target).map_err(|_| ShardExecutionError::InvalidTarget)
    }

    fn validate_route_and_target(
        &self,
        routed: &RoutedCausalEvent,
    ) -> Result<ShardId, ShardExecutionError> {
        let target = self.target_neuron(&routed.event)?;
        let target_component = self
            .plan
            .component_for_neuron(target)
            .ok_or(ShardExecutionError::UnknownTarget(target))?;
        let source_component = if routed.event.key.source == 0 {
            target_component
        } else {
            let source = NeuronId::new(routed.event.key.source)?;
            self.plan
                .component_for_neuron(source)
                .ok_or(ShardExecutionError::UnknownSource(source))?
        };
        self.plan.validate_event(
            self.plan.topology_generation(),
            self.plan.partition_generation(),
            source_component,
            target_component,
            routed.route,
        )?;
        self.target_shard(&routed.event)
    }

    fn peek_next(&self) -> Option<(ShardId, LogicalTag)> {
        self.pending
            .iter()
            .filter_map(|(shard, queues)| {
                queues
                    .values()
                    .next()
                    .and_then(|events| events.first())
                    .map(|event| (*shard, event.event.key))
            })
            .min_by(|(left_shard, left_key), (right_shard, right_key)| {
                left_key
                    .cmp(right_key)
                    .then_with(|| left_shard.cmp(right_shard))
            })
            .map(|(shard, key)| (shard, key.tag))
    }

    fn pop_next(
        &mut self,
        shard: ShardId,
        tag: LogicalTag,
    ) -> Result<RoutedCausalEvent, ShardExecutionError> {
        let queues = self
            .pending
            .get_mut(&shard)
            .ok_or(ShardExecutionError::UnknownShard(shard))?;
        let events = queues
            .get_mut(&tag)
            .ok_or(ShardExecutionError::UnknownShard(shard))?;
        events.sort_by(|left, right| left.event.key.cmp(&right.event.key));
        let event = events.remove(0);
        if events.is_empty() {
            queues.remove(&tag);
        }
        Ok(event)
    }
}

fn checkpoint_material(checkpoint: &StableShardCheckpoint) -> Result<Vec<u8>, ShardExecutionError> {
    let mut material = checkpoint.clone();
    material.checkpoint_digest = StateDigest([0; 16]);
    serde_json::to_vec(&material).map_err(|error| ShardExecutionError::Encoding(error.to_string()))
}

fn seal_checkpoint(
    mut checkpoint: StableShardCheckpoint,
) -> Result<StableShardCheckpoint, ShardExecutionError> {
    let mut digest = StateDigestBuilder::default();
    digest.add_domain(
        "stable-shard-checkpoint:v1",
        checkpoint_material(&checkpoint)?,
    );
    checkpoint.checkpoint_digest = digest.finish();
    Ok(checkpoint)
}

fn verify_checkpoint(checkpoint: &StableShardCheckpoint) -> Result<(), ShardExecutionError> {
    if checkpoint.schema_version != SHARD_EXECUTOR_SCHEMA_VERSION
        || checkpoint.queue_capacity == 0
        || checkpoint.dedupe_capacity == 0
        || checkpoint.biological_state.is_empty()
        || checkpoint.fabric_digest == StateDigest([0; 16])
        || checkpoint.plan_digest == StateDigest([0; 16])
        || checkpoint.checkpoint_digest == StateDigest([0; 16])
    {
        return Err(ShardExecutionError::InvalidCheckpoint(
            "schema, capacities, state bytes or digests are invalid",
        ));
    }
    let mut digest = StateDigestBuilder::default();
    digest.add_domain(
        "stable-shard-checkpoint:v1",
        checkpoint_material(checkpoint)?,
    );
    if digest.finish() != checkpoint.checkpoint_digest {
        return Err(ShardExecutionError::InvalidCheckpoint(
            "checkpoint digest does not match its contents",
        ));
    }
    Ok(())
}

fn target_shard_for_plan(
    plan: &CompiledExecutionPlan,
    routed: &RoutedCausalEvent,
) -> Result<ShardId, ShardExecutionError> {
    let event = &routed.event;
    let target = NeuronId::new(event.key.target).map_err(|_| ShardExecutionError::InvalidTarget)?;
    let target_component = plan
        .component_for_neuron(target)
        .ok_or(ShardExecutionError::UnknownTarget(target))?;
    let source_component = if event.key.source == 0 {
        target_component
    } else {
        let source = NeuronId::new(event.key.source)?;
        plan.component_for_neuron(source)
            .ok_or(ShardExecutionError::UnknownSource(source))?
    };
    plan.validate_event(
        plan.topology_generation(),
        plan.partition_generation(),
        source_component,
        target_component,
        routed.route,
    )?;
    plan.neuron_owner(target)
        .ok_or(ShardExecutionError::UnknownTarget(target))
}

fn transition_charge(event: &CausalEvent) -> i64 {
    serde_json::from_slice::<StableTransitionInput>(&event.payload)
        .map(|input| input.charge)
        .unwrap_or(0)
}

pub(crate) fn derive_child_event_id(
    parent: EventId,
    synapse: crate::deterministic::SynapseId,
    tag: LogicalTag,
    target: NeuronId,
) -> Result<EventId, ShardExecutionError> {
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("parent", parent.raw().to_be_bytes());
    digest.add_domain("synapse", synapse.raw().to_be_bytes());
    digest.add_domain("target", target.raw().to_be_bytes());
    digest.add_domain("tick", tag.tick.to_be_bytes());
    digest.add_domain("microstep", tag.microstep.to_be_bytes());
    let digest = digest.finish();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest.0[..8]);
    let raw = u64::from_be_bytes(bytes).max(1);
    Ok(EventId::new(raw)?)
}

/// Seal a shard checkpoint for a physical worker transfer without exposing
/// the complete-fabric executor internals to transport adapters.
pub(crate) fn seal_checkpoint_for_transfer(
    checkpoint: StableShardCheckpoint,
) -> Result<StableShardCheckpoint, ShardExecutionError> {
    seal_checkpoint(checkpoint)
}

fn append_event_digest_bytes(bytes: &mut Vec<u8>, routed: &RoutedCausalEvent) {
    bytes.extend_from_slice(&routed.event.key.tag.tick.to_be_bytes());
    bytes.extend_from_slice(&routed.event.key.tag.microstep.to_be_bytes());
    bytes.push(routed.event.key.stage as u8);
    bytes.extend_from_slice(&routed.event.key.source.to_be_bytes());
    bytes.extend_from_slice(&routed.event.key.target.to_be_bytes());
    bytes.extend_from_slice(&routed.event.id.raw().to_be_bytes());
    bytes.extend_from_slice(&routed.route.map(RouteId::raw).unwrap_or(0).to_be_bytes());
    bytes.extend_from_slice(&(routed.event.payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&routed.event.payload);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::{
        ComponentId, PartitionGeneration, ShardId, SynapseId, TopologyGeneration,
    };
    use crate::topology_model::{
        OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
        compile_execution_plan,
    };

    fn fixture() -> (StableShardExecutor, BrainId, ShardId, ShardId) {
        let neurons = (1..=4)
            .map(|id| crate::topology_model::NeuronRecord {
                id: NeuronId::new(id).unwrap(),
            })
            .collect::<Vec<_>>();
        let topology = TopologyGenerationModel::new(
            TopologyGeneration::INITIAL,
            neurons,
            vec![
                SynapseRecord {
                    id: SynapseId::new(11).unwrap(),
                    source: NeuronId::new(1).unwrap(),
                    target: NeuronId::new(2).unwrap(),
                    delay_ticks: 0,
                },
                SynapseRecord {
                    id: SynapseId::new(12).unwrap(),
                    source: NeuronId::new(2).unwrap(),
                    target: NeuronId::new(3).unwrap(),
                    delay_ticks: 1,
                },
                SynapseRecord {
                    id: SynapseId::new(13).unwrap(),
                    source: NeuronId::new(3).unwrap(),
                    target: NeuronId::new(4).unwrap(),
                    delay_ticks: 0,
                },
            ],
        )
        .unwrap();
        let ownership = topology
            .synapses()
            .map(|synapse| OwnershipRecord {
                synapse: synapse.id,
                terminal_owner: if synapse.id == SynapseId::new(13).unwrap() {
                    ShardId::new(2).unwrap()
                } else {
                    ShardId::new(1).unwrap()
                },
                weight_owner: if synapse.id == SynapseId::new(13).unwrap() {
                    ShardId::new(2).unwrap()
                } else {
                    ShardId::new(1).unwrap()
                },
                release_owner: if synapse.id == SynapseId::new(13).unwrap() {
                    ShardId::new(2).unwrap()
                } else {
                    ShardId::new(1).unwrap()
                },
                plasticity_owner: if synapse.id == SynapseId::new(13).unwrap() {
                    ShardId::new(2).unwrap()
                } else {
                    ShardId::new(1).unwrap()
                },
            })
            .collect::<Vec<_>>();
        let plan = compile_execution_plan(
            &topology,
            PartitionGeneration::INITIAL,
            vec![
                VirtualShardAssignment {
                    shard: ShardId::new(1).unwrap(),
                    components: vec![ComponentId::new(1).unwrap(), ComponentId::new(2).unwrap()],
                    load: 2,
                },
                VirtualShardAssignment {
                    shard: ShardId::new(2).unwrap(),
                    components: vec![ComponentId::new(3).unwrap(), ComponentId::new(4).unwrap()],
                    load: 2,
                },
            ],
            ownership,
        )
        .unwrap();
        let brain = BrainId::new(900).unwrap();
        let executor = StableShardExecutor::from_topology(
            brain,
            &topology,
            plan,
            FIXED_POINT_SCALE,
            FIXED_POINT_SCALE,
            16,
            128,
        )
        .unwrap();
        (
            executor,
            brain,
            ShardId::new(1).unwrap(),
            ShardId::new(2).unwrap(),
        )
    }

    fn input_event(id: u64, target: u64, tag: LogicalTag) -> RoutedCausalEvent {
        let target = NeuronId::new(target).unwrap();
        let event = EventId::new(id).unwrap();
        let payload = serde_json::to_vec(&StableTransitionInput {
            schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
            source: None,
            target,
            charge: 1,
            delay_ticks: 0,
        })
        .unwrap();
        RoutedCausalEvent {
            route: None,
            event: CausalEvent {
                key: CanonicalEventKey::new(
                    tag,
                    EventStage::SynapticTransition,
                    0,
                    target.raw(),
                    event.raw(),
                ),
                id: event,
                payload,
                original_tag: tag,
                deferred_from_nonconvergence: false,
            },
        }
    }

    #[test]
    fn routes_causal_output_between_independently_owned_shards() {
        let (mut executor, _brain, first_shard, second_shard) = fixture();
        executor.admit(input_event(1, 1, LogicalTag::ZERO)).unwrap();
        let first_step = executor.step().unwrap().unwrap();
        assert_eq!(first_step.consumed.event.key.target, 1);
        assert_eq!(first_step.emitted.len(), 1);
        assert_eq!(executor.pending_count(first_shard).unwrap(), 1);
        assert_eq!(executor.pending_count(second_shard).unwrap(), 0);
        let second_step = executor.step().unwrap().unwrap();
        assert_eq!(second_step.consumed.event.key.target, 2);
        assert_eq!(executor.pending_count(first_shard).unwrap(), 0);
        assert_eq!(executor.pending_count(second_shard).unwrap(), 1);
        let third = executor.step().unwrap().unwrap();
        assert_eq!(third.consumed.event.key.target, 3);
        assert_eq!(third.consumed.event.key.tag, LogicalTag::new(1, 0));
        assert_eq!(executor.pending_count(second_shard).unwrap(), 1);
        assert_eq!(executor.current_tag(), LogicalTag::new(1, 0));
    }

    #[test]
    fn duplicate_is_idempotent_and_conflicting_reuse_fails_before_mutation() {
        let (mut executor, _brain, _first, _second) = fixture();
        let event = input_event(2, 1, LogicalTag::ZERO);
        assert_eq!(
            executor.admit(event.clone()).unwrap(),
            AdmissionOutcome::Accepted {
                shard: ShardId::new(1).unwrap()
            }
        );
        assert_eq!(
            executor.admit(event.clone()).unwrap(),
            AdmissionOutcome::Duplicate {
                shard: ShardId::new(1).unwrap()
            }
        );
        let mut conflicting = event;
        conflicting.event.payload.push(9);
        assert!(matches!(
            executor.admit(conflicting),
            Err(ShardExecutionError::ConflictingDuplicate { .. })
        ));
        assert_eq!(executor.pending_count(ShardId::new(1).unwrap()).unwrap(), 1);
    }

    #[test]
    fn failed_route_or_queue_admission_does_not_mutate_the_fabric() {
        let (mut executor, _brain, _first, _second) = fixture();
        let before = executor.state_digest().unwrap();
        let mut invalid = input_event(3, 3, LogicalTag::ZERO);
        invalid.route = Some(RouteId::new(999).unwrap());
        assert!(matches!(
            executor.admit(invalid),
            Err(ShardExecutionError::Plan(_))
        ));
        assert_eq!(executor.state_digest().unwrap(), before);
    }

    #[test]
    fn canonical_order_and_digest_are_independent_of_admission_order() {
        let (mut first, _brain, _first_shard, _second_shard) = fixture();
        let (mut second, _brain, _first_shard, _second_shard) = fixture();
        let low_key = input_event(20, 1, LogicalTag::ZERO);
        let high_key = input_event(10, 3, LogicalTag::ZERO);
        first.admit(low_key.clone()).unwrap();
        first.admit(high_key.clone()).unwrap();
        second.admit(high_key).unwrap();
        second.admit(low_key).unwrap();
        assert_eq!(
            first.state_digest().unwrap(),
            second.state_digest().unwrap()
        );
        assert_eq!(first.step().unwrap().unwrap().consumed.event.key.target, 1);
        assert_eq!(second.step().unwrap().unwrap().consumed.event.key.target, 1);
    }

    #[test]
    fn split_synapse_mutable_ownership_is_rejected_before_execution() {
        let topology = TopologyGenerationModel::new(
            TopologyGeneration::INITIAL,
            vec![
                crate::topology_model::NeuronRecord {
                    id: NeuronId::new(1).unwrap(),
                },
                crate::topology_model::NeuronRecord {
                    id: NeuronId::new(2).unwrap(),
                },
            ],
            vec![SynapseRecord {
                id: SynapseId::new(11).unwrap(),
                source: NeuronId::new(1).unwrap(),
                target: NeuronId::new(2).unwrap(),
                delay_ticks: 0,
            }],
        )
        .unwrap();
        let plan = compile_execution_plan(
            &topology,
            PartitionGeneration::INITIAL,
            vec![
                VirtualShardAssignment {
                    shard: ShardId::new(1).unwrap(),
                    components: vec![ComponentId::new(1).unwrap()],
                    load: 1,
                },
                VirtualShardAssignment {
                    shard: ShardId::new(2).unwrap(),
                    components: vec![ComponentId::new(2).unwrap()],
                    load: 1,
                },
            ],
            vec![OwnershipRecord {
                synapse: SynapseId::new(11).unwrap(),
                terminal_owner: ShardId::new(1).unwrap(),
                weight_owner: ShardId::new(2).unwrap(),
                release_owner: ShardId::new(1).unwrap(),
                plasticity_owner: ShardId::new(1).unwrap(),
            }],
        )
        .unwrap();

        assert_eq!(
            StableShardExecutor::from_topology(
                BrainId::new(901).unwrap(),
                &topology,
                plan,
                FIXED_POINT_SCALE,
                FIXED_POINT_SCALE,
                8,
                8,
            )
            .unwrap_err(),
            ShardExecutionError::SplitSynapseOwnership {
                synapse: SynapseId::new(11).unwrap(),
            }
        );
    }

    #[test]
    fn multi_output_queue_overflow_rolls_back_every_mutation() {
        let topology = TopologyGenerationModel::new(
            TopologyGeneration::INITIAL,
            (1..=3)
                .map(|id| crate::topology_model::NeuronRecord {
                    id: NeuronId::new(id).unwrap(),
                })
                .collect(),
            vec![
                SynapseRecord {
                    id: SynapseId::new(11).unwrap(),
                    source: NeuronId::new(1).unwrap(),
                    target: NeuronId::new(2).unwrap(),
                    delay_ticks: 0,
                },
                SynapseRecord {
                    id: SynapseId::new(12).unwrap(),
                    source: NeuronId::new(1).unwrap(),
                    target: NeuronId::new(3).unwrap(),
                    delay_ticks: 0,
                },
            ],
        )
        .unwrap();
        let shard = ShardId::new(1).unwrap();
        let plan = compile_execution_plan(
            &topology,
            PartitionGeneration::INITIAL,
            vec![VirtualShardAssignment {
                shard,
                components: vec![
                    ComponentId::new(1).unwrap(),
                    ComponentId::new(2).unwrap(),
                    ComponentId::new(3).unwrap(),
                ],
                load: 3,
            }],
            topology
                .synapses()
                .map(|synapse| OwnershipRecord {
                    synapse: synapse.id,
                    terminal_owner: shard,
                    weight_owner: shard,
                    release_owner: shard,
                    plasticity_owner: shard,
                })
                .collect(),
        )
        .unwrap();
        let mut executor = StableShardExecutor::from_topology(
            BrainId::new(902).unwrap(),
            &topology,
            plan,
            FIXED_POINT_SCALE,
            FIXED_POINT_SCALE,
            1,
            8,
        )
        .unwrap();
        executor
            .admit(input_event(30, 1, LogicalTag::ZERO))
            .unwrap();
        let before = executor.state_digest().unwrap();

        assert_eq!(
            executor.step().unwrap_err(),
            ShardExecutionError::QueueFull { shard, capacity: 1 }
        );
        assert_eq!(executor.state_digest().unwrap(), before);
        assert_eq!(executor.pending_count(shard).unwrap(), 1);
        assert_eq!(
            executor.shard_state(shard).unwrap().committed_tag(),
            LogicalTag::ZERO
        );
    }

    #[test]
    fn checkpoint_restore_preserves_pending_work_and_fabric_digest() {
        let (mut executor, brain, _first_shard, _second_shard) = fixture();
        executor
            .admit(input_event(40, 1, LogicalTag::ZERO))
            .unwrap();
        executor.step().unwrap().unwrap();
        let before = executor.state_digest().unwrap();
        let checkpoints = executor.checkpoint_shards().unwrap();
        assert_eq!(checkpoints.len(), 2);
        assert!(checkpoints.iter().all(|checkpoint| {
            serde_json::to_vec(checkpoint)
                .and_then(|bytes| serde_json::from_slice(&bytes))
                .map(|restored: StableShardCheckpoint| restored == *checkpoint)
                .unwrap_or(false)
        }));

        let restored = StableShardExecutor::restore_from_checkpoints(
            brain,
            executor.plan().clone(),
            checkpoints,
        )
        .unwrap();
        assert_eq!(restored.state_digest().unwrap(), before);
        assert_eq!(restored.total_pending(), executor.total_pending());
    }

    #[test]
    fn checkpoint_restore_rejects_incomplete_or_tampered_cuts() {
        let (mut executor, brain, _first_shard, _second_shard) = fixture();
        executor
            .admit(input_event(41, 1, LogicalTag::ZERO))
            .unwrap();
        let mut checkpoints = executor.checkpoint_shards().unwrap();
        let incomplete = checkpoints.pop().unwrap();
        assert!(matches!(
            StableShardExecutor::restore_from_checkpoints(
                brain,
                executor.plan().clone(),
                vec![incomplete],
            ),
            Err(ShardExecutionError::InvalidCheckpoint(_))
        ));

        let mut tampered = executor.checkpoint_shards().unwrap();
        tampered[0].pending[0].event.payload.push(0xff);
        assert!(matches!(
            StableShardExecutor::restore_from_checkpoints(brain, executor.plan().clone(), tampered,),
            Err(ShardExecutionError::InvalidCheckpoint(_))
        ));
    }

    #[test]
    fn durable_shard_envelopes_restore_the_complete_fabric() {
        let (mut executor, brain, _first_shard, _second_shard) = fixture();
        executor
            .admit(input_event(42, 1, LogicalTag::ZERO))
            .unwrap();
        executor.step().unwrap().unwrap();
        let before = executor.state_digest().unwrap();
        let states = executor
            .shard_states(crate::deterministic::LeaseTerm::INITIAL, Some(7))
            .unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|state| state.verify().is_ok()));

        let restored =
            StableShardExecutor::restore_from_shard_states(brain, executor.plan().clone(), states)
                .unwrap();
        assert_eq!(restored.state_digest().unwrap(), before);
    }
}
