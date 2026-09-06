//! Bounded execution of only the virtual shards assigned to one worker.
//!
//! [`crate::shard_executor::StableShardExecutor`] is the deterministic
//! complete-fabric reference oracle.  It deliberately keeps every shard in
//! one process so the first implementation could prove biological ordering
//! and checkpoint consistency.  This module is the next physical-placement
//! seam: a worker materialises only its owned shard checkpoints and uses
//! typed, generation-bound messages for work whose mutable owner is remote.
//!
//! The module is transport-neutral.  A server adapter must persist the
//! outbound messages before acknowledging a cut, attach lease/fencing and
//! stream sequence metadata, and deliver them through the authoritative
//! causal service.  The in-memory message boundary here is therefore useful
//! for deterministic parity and fault tests, but it does not by itself grant
//! a worker authority or enable production placement.

use crate::authoritative_shard::{
    BIOLOGICAL_STATE_SCHEMA_VERSION, FIXED_POINT_SCALE, StableBiologicalState,
    StableTransitionInput,
};
use crate::causal::CausalEvent;
use crate::deterministic::{
    BrainId, CanonicalEventKey, EventId, EventStage, LogicalTag, NeuronId, ShardId, StateDigest,
    StateDigestBuilder,
};
use crate::shard_executor::{RoutedCausalEvent, StableShardCheckpoint, derive_child_event_id};
use crate::topology_model::{CompiledExecutionPlan, TopologyGenerationModel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const PARTIAL_SHARD_EXECUTOR_SCHEMA_VERSION: u32 = 1;

/// Immutable graph information present on every worker.  Mutable synapse
/// fields remain exclusively in the checkpoint owned by `owner`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SynapseDescriptor {
    id: crate::deterministic::SynapseId,
    source: NeuronId,
    target: NeuronId,
    delay_ticks: u64,
    owner: ShardId,
}

/// A causal message produced when a local transition needs another virtual
/// shard.  `plan_digest` binds the message to the immutable biological plan;
/// the network transport adds stream sequence, lease and fencing metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartialShardOutbound {
    /// A child event whose mutable synapse fields were read locally.
    CausalEvent {
        plan_digest: StateDigest,
        destination_shard: ShardId,
        event: RoutedCausalEvent,
    },
    /// The target event changed a synapse whose mutable fields live remotely.
    SynapseEffect {
        plan_digest: StateDigest,
        destination_shard: ShardId,
        event_id: EventId,
        logical_tag: LogicalTag,
        synapse: crate::deterministic::SynapseId,
        charge: i64,
    },
    /// A fired neuron reached a synapse whose owner is remote.  The owner
    /// reads its current weight and creates the child event, preventing a
    /// stale replicated weight from becoming authoritative on the source.
    SynapseActivation {
        plan_digest: StateDigest,
        destination_shard: ShardId,
        parent_event: EventId,
        synapse: crate::deterministic::SynapseId,
        source: NeuronId,
        target: NeuronId,
        child_tag: LogicalTag,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialShardStep {
    pub consumed: RoutedCausalEvent,
    pub fired: Vec<NeuronId>,
    pub outbound: Vec<PartialShardOutbound>,
    pub logical_tag: LogicalTag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialShardApply {
    pub outbound: Vec<PartialShardOutbound>,
    pub duplicate: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PartialShardExecutorError {
    #[error("partial shard executor schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("partial shard executor requires a positive queue and dedupe capacity")]
    InvalidCapacity,
    #[error("partial shard executor has no owned shard {0}")]
    UnknownOwnedShard(ShardId),
    #[error("partial shard executor received a checkpoint for an unowned shard {0}")]
    UnownedCheckpoint(ShardId),
    #[error("partial shard executor checkpoint set is incomplete for the owned shards")]
    IncompleteCheckpointSet,
    #[error("partial shard executor checkpoint metadata does not match its plan")]
    CheckpointPlanMismatch,
    #[error("partial shard executor checkpoints do not share one fabric cut")]
    CheckpointFabricMismatch,
    #[error("partial shard executor checkpoint contains state outside its owned shard")]
    CheckpointStateMismatch,
    #[error("partial shard executor has no local owner for target neuron {0}")]
    RemoteTarget(NeuronId),
    #[error("partial shard executor received a message for shard {0}")]
    WrongDestination(ShardId),
    #[error("partial shard executor message plan digest does not match")]
    PlanDigestMismatch,
    #[error(
        "partial shard executor message declares destination {declared}, but the plan routes it to {expected}"
    )]
    DestinationMismatch {
        declared: ShardId,
        expected: ShardId,
    },
    #[error("partial shard executor event is a conflicting duplicate: {0}")]
    ConflictingDuplicate(EventId),
    #[error("partial shard executor queue reached its bound on shard {shard}")]
    QueueFull { shard: ShardId, capacity: usize },
    #[error("partial shard executor deduplication window reached its bound {0}")]
    DedupeWindowFull(usize),
    #[error("partial shard executor outbound message bound {0} was exceeded")]
    OutboundFull(usize),
    #[error("partial shard executor control message was replayed with different contents: {0}")]
    ConflictingControl(EventId),
    #[error("partial shard executor synapse {0} splits mutable ownership across shards")]
    SplitSynapseOwnership(crate::deterministic::SynapseId),
    #[error("partial shard executor event ID collides with an existing event: {0}")]
    EventIdCollision(EventId),
    #[error("partial shard executor has no mutable synapse owner {0}")]
    MissingSynapse(crate::deterministic::SynapseId),
    #[error("partial shard executor biological transition failed: {0}")]
    Biology(String),
    #[error("partial shard executor event is invalid: {0}")]
    Event(String),
    #[error("partial shard executor checkpoint failed: {0}")]
    Checkpoint(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ControlKind {
    SynapseEffect,
    SynapseActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ControlKey {
    kind: ControlKind,
    event_id: EventId,
    synapse: crate::deterministic::SynapseId,
}

/// A bounded worker that materialises only `owned_shards`.
#[derive(Debug, Clone)]
pub struct PartialShardExecutor {
    brain_id: BrainId,
    plan: CompiledExecutionPlan,
    plan_digest: StateDigest,
    descriptors: BTreeMap<crate::deterministic::SynapseId, SynapseDescriptor>,
    outgoing: BTreeMap<NeuronId, Vec<crate::deterministic::SynapseId>>,
    incoming: BTreeMap<NeuronId, Vec<crate::deterministic::SynapseId>>,
    owned_shards: BTreeSet<ShardId>,
    states: BTreeMap<ShardId, StableBiologicalState>,
    pending: BTreeMap<ShardId, BTreeMap<LogicalTag, Vec<RoutedCausalEvent>>>,
    admitted: BTreeMap<EventId, RoutedCausalEvent>,
    committed: Vec<RoutedCausalEvent>,
    /// The digest is retained with each dedupe key.  A repeated message is
    /// idempotent only when its complete payload is identical; accepting a
    /// same-key/different-payload replay would let a delayed or forged frame
    /// mutate the authoritative shard with an ambiguous meaning.
    applied_controls: BTreeMap<ControlKey, StateDigest>,
    queue_capacity: usize,
    dedupe_capacity: usize,
    max_outbound_per_step: usize,
    current_tag: LogicalTag,
    fabric_digest: StateDigest,
}

impl PartialShardExecutor {
    /// Transport adapters need the brain identity to bind durable receiver
    /// documents.  The biological owner remains private; this accessor does
    /// not expose mutable state or grant authority.
    pub(crate) fn brain_id_for_transport(&self) -> BrainId {
        self.brain_id
    }

    /// Open a worker from immutable topology metadata and a subset of a
    /// complete checkpoint cut. Empty ownership is allowed for a drained
    /// source worker, but such a worker cannot admit or execute events.
    pub fn from_checkpoints(
        brain_id: BrainId,
        topology: &TopologyGenerationModel,
        plan: CompiledExecutionPlan,
        checkpoints: Vec<StableShardCheckpoint>,
        owned_shards: impl IntoIterator<Item = ShardId>,
        max_outbound_per_step: usize,
    ) -> Result<Self, PartialShardExecutorError> {
        if max_outbound_per_step == 0 {
            return Err(PartialShardExecutorError::InvalidCapacity);
        }
        if topology.digest() != plan.topology_digest() {
            return Err(PartialShardExecutorError::CheckpointPlanMismatch);
        }
        let owned_shards = owned_shards.into_iter().collect::<BTreeSet<_>>();
        let plan_shards = plan.shard_ids().collect::<BTreeSet<_>>();
        if !owned_shards.is_subset(&plan_shards) {
            return Err(PartialShardExecutorError::UnknownOwnedShard(
                *owned_shards.difference(&plan_shards).next().unwrap(),
            ));
        }
        let plan_digest = plan.digest();
        for ownership in plan.ownership_records() {
            if ownership.terminal_owner != ownership.weight_owner
                || ownership.terminal_owner != ownership.release_owner
                || ownership.terminal_owner != ownership.plasticity_owner
            {
                return Err(PartialShardExecutorError::SplitSynapseOwnership(
                    ownership.synapse,
                ));
            }
        }
        let mut descriptors = BTreeMap::new();
        let mut outgoing = BTreeMap::<NeuronId, Vec<_>>::new();
        let mut incoming = BTreeMap::<NeuronId, Vec<_>>::new();
        for synapse in topology.synapses() {
            let owner = plan
                .ownership(synapse.id)
                .ok_or(PartialShardExecutorError::MissingSynapse(synapse.id))?
                .terminal_owner;
            let descriptor = SynapseDescriptor {
                id: synapse.id,
                source: synapse.source,
                target: synapse.target,
                delay_ticks: synapse.delay_ticks,
                owner,
            };
            descriptors.insert(synapse.id, descriptor);
            outgoing.entry(synapse.source).or_default().push(synapse.id);
            incoming.entry(synapse.target).or_default().push(synapse.id);
        }
        for ids in outgoing.values_mut() {
            ids.sort_unstable();
        }
        for ids in incoming.values_mut() {
            ids.sort_unstable();
        }

        let mut states = BTreeMap::new();
        let mut pending: BTreeMap<ShardId, BTreeMap<LogicalTag, Vec<RoutedCausalEvent>>> =
            BTreeMap::new();
        let mut admitted = BTreeMap::new();
        let mut committed = Vec::new();
        let mut queue_capacity = None;
        let mut dedupe_capacity = None;
        let mut current_tag = None;
        let mut fabric_digest = None;
        for checkpoint in checkpoints {
            checkpoint
                .verify()
                .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))?;
            if checkpoint.schema_version != crate::shard_executor::SHARD_EXECUTOR_SCHEMA_VERSION
                || checkpoint.brain_id != brain_id
                || checkpoint.plan_digest != plan_digest
                || checkpoint.topology_generation != plan.topology_generation()
                || checkpoint.partition_generation != plan.partition_generation()
            {
                return Err(PartialShardExecutorError::CheckpointPlanMismatch);
            }
            if !owned_shards.contains(&checkpoint.shard_id) {
                return Err(PartialShardExecutorError::UnownedCheckpoint(
                    checkpoint.shard_id,
                ));
            }
            let state = StableBiologicalState::decode(&checkpoint.biological_state)
                .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))?;
            let expected_neurons = topology
                .neurons()
                .filter(|neuron| plan.neuron_owner(neuron.id) == Some(checkpoint.shard_id))
                .map(|neuron| neuron.id)
                .collect::<BTreeSet<_>>();
            let actual_neurons = state
                .neurons()
                .iter()
                .map(|neuron| neuron.id)
                .collect::<BTreeSet<_>>();
            let expected_synapses = topology
                .synapses()
                .filter(|synapse| {
                    plan.ownership(synapse.id)
                        .is_some_and(|owner| owner.terminal_owner == checkpoint.shard_id)
                })
                .map(|synapse| synapse.id)
                .collect::<BTreeSet<_>>();
            let actual_synapses = state
                .synapses()
                .iter()
                .map(|synapse| synapse.id)
                .collect::<BTreeSet<_>>();
            if state.topology_generation() != checkpoint.topology_generation
                || actual_neurons != expected_neurons
                || actual_synapses != expected_synapses
            {
                return Err(PartialShardExecutorError::CheckpointStateMismatch);
            }
            if states.insert(checkpoint.shard_id, state).is_some() {
                return Err(PartialShardExecutorError::CheckpointStateMismatch);
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
                return Err(PartialShardExecutorError::CheckpointStateMismatch);
            }
            if fabric_digest
                .replace(checkpoint.fabric_digest)
                .is_some_and(|value| value != checkpoint.fabric_digest)
            {
                return Err(PartialShardExecutorError::CheckpointFabricMismatch);
            }
            let shard = checkpoint.shard_id;
            let queues = pending.entry(shard).or_insert_with(BTreeMap::new);
            for event in checkpoint.pending {
                if target_shard(&plan, &event)? != shard {
                    return Err(PartialShardExecutorError::CheckpointStateMismatch);
                }
                queues.entry(event.event.key.tag).or_default().push(event);
            }
            for events in queues.values_mut() {
                events.sort_by(|left, right| left.event.key.cmp(&right.event.key));
            }
            for event in checkpoint.admitted {
                if target_shard(&plan, &event)? != shard
                    || admitted.insert(event.event.id, event).is_some()
                {
                    return Err(PartialShardExecutorError::CheckpointStateMismatch);
                }
            }
            committed.extend(checkpoint.committed);
        }
        if states.keys().copied().collect::<BTreeSet<_>>() != owned_shards {
            return Err(PartialShardExecutorError::IncompleteCheckpointSet);
        }
        let queue_capacity = if owned_shards.is_empty() {
            1
        } else {
            queue_capacity.ok_or(PartialShardExecutorError::IncompleteCheckpointSet)?
        };
        let dedupe_capacity = if owned_shards.is_empty() {
            1
        } else {
            dedupe_capacity.ok_or(PartialShardExecutorError::IncompleteCheckpointSet)?
        };
        Ok(Self {
            brain_id,
            plan,
            plan_digest,
            descriptors,
            outgoing,
            incoming,
            owned_shards,
            states,
            pending,
            admitted,
            committed,
            applied_controls: BTreeMap::new(),
            queue_capacity,
            dedupe_capacity,
            max_outbound_per_step,
            current_tag: current_tag.unwrap_or(LogicalTag::ZERO),
            fabric_digest: fabric_digest.unwrap_or(StateDigest([0; 16])),
        })
    }

    pub fn owned_shards(&self) -> impl Iterator<Item = ShardId> + '_ {
        self.owned_shards.iter().copied()
    }

    pub fn plan(&self) -> &CompiledExecutionPlan {
        &self.plan
    }

    pub fn current_tag(&self) -> LogicalTag {
        self.current_tag
    }

    pub fn pending_count(&self, shard: ShardId) -> Result<usize, PartialShardExecutorError> {
        let queues = self
            .pending
            .get(&shard)
            .ok_or(PartialShardExecutorError::UnknownOwnedShard(shard))?;
        Ok(queues.values().map(Vec::len).sum())
    }

    pub fn total_pending(&self) -> usize {
        self.pending
            .values()
            .flat_map(|queues| queues.values())
            .map(Vec::len)
            .sum()
    }

    pub fn state_bytes(&self, shard: ShardId) -> Result<Vec<u8>, PartialShardExecutorError> {
        self.states
            .get(&shard)
            .ok_or(PartialShardExecutorError::UnknownOwnedShard(shard))?
            .encode()
            .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))
    }

    /// Admit an event only when its target shard is materialised here.
    pub fn admit(&mut self, routed: RoutedCausalEvent) -> Result<bool, PartialShardExecutorError> {
        let shard = target_shard(&self.plan, &routed)?;
        if !self.owned_shards.contains(&shard) {
            return Err(PartialShardExecutorError::RemoteTarget(
                NeuronId::new(routed.event.key.target)
                    .map_err(|_| PartialShardExecutorError::Event("invalid target".to_owned()))?,
            ));
        }
        if let Some(previous) = self.admitted.get(&routed.event.id) {
            if previous == &routed {
                return Ok(false);
            }
            return Err(PartialShardExecutorError::ConflictingDuplicate(
                routed.event.id,
            ));
        }
        if self.admitted.len() >= self.dedupe_capacity {
            return Err(PartialShardExecutorError::DedupeWindowFull(
                self.dedupe_capacity,
            ));
        }
        if routed.event.key.tag < self.current_tag {
            return Err(PartialShardExecutorError::Event(
                "event tag moved backwards".to_owned(),
            ));
        }
        let queue_len = self.pending_count(shard)?;
        if queue_len >= self.queue_capacity {
            return Err(PartialShardExecutorError::QueueFull {
                shard,
                capacity: self.queue_capacity,
            });
        }
        let tag = routed.event.key.tag;
        self.admitted.insert(routed.event.id, routed.clone());
        self.pending
            .entry(shard)
            .or_default()
            .entry(tag)
            .or_default()
            .push(routed);
        if let Some(events) = self
            .pending
            .get_mut(&shard)
            .and_then(|queues| queues.get_mut(&tag))
        {
            events.sort_by(|left, right| left.event.key.cmp(&right.event.key));
        }
        Ok(true)
    }

    /// Process one canonical local event. Remote work is returned as typed
    /// outbound messages and is never silently inserted into a missing shard.
    pub fn step(&mut self) -> Result<Option<PartialShardStep>, PartialShardExecutorError> {
        let Some((shard, tag)) = self.peek_next() else {
            return Ok(None);
        };
        let before = self.clone();
        match self.step_one(shard, tag) {
            Ok(step) => Ok(Some(step)),
            Err(error) => {
                *self = before;
                Err(error)
            }
        }
    }

    pub fn settle(
        &mut self,
        max_events: usize,
    ) -> Result<Vec<PartialShardStep>, PartialShardExecutorError> {
        if max_events == 0 {
            return Err(PartialShardExecutorError::InvalidCapacity);
        }
        let mut steps = Vec::new();
        while steps.len() < max_events {
            let Some(step) = self.step()? else { break };
            steps.push(step);
        }
        Ok(steps)
    }

    /// Apply one message from another worker. The caller is responsible for
    /// durable stream sequencing and physical peer authentication.
    pub fn apply_outbound(
        &mut self,
        message: PartialShardOutbound,
    ) -> Result<PartialShardApply, PartialShardExecutorError> {
        let before = self.clone();
        let result = self.apply_outbound_inner(message);
        if result.is_err() {
            *self = before;
        }
        result
    }

    fn apply_outbound_inner(
        &mut self,
        message: PartialShardOutbound,
    ) -> Result<PartialShardApply, PartialShardExecutorError> {
        let destination = match &message {
            PartialShardOutbound::CausalEvent {
                destination_shard, ..
            }
            | PartialShardOutbound::SynapseEffect {
                destination_shard, ..
            }
            | PartialShardOutbound::SynapseActivation {
                destination_shard, ..
            } => *destination_shard,
        };
        if !self.owned_shards.contains(&destination) {
            return Err(PartialShardExecutorError::WrongDestination(destination));
        }
        let message_plan = match &message {
            PartialShardOutbound::CausalEvent { plan_digest, .. }
            | PartialShardOutbound::SynapseEffect { plan_digest, .. }
            | PartialShardOutbound::SynapseActivation { plan_digest, .. } => *plan_digest,
        };
        if message_plan != self.plan_digest {
            return Err(PartialShardExecutorError::PlanDigestMismatch);
        }
        match message {
            PartialShardOutbound::CausalEvent {
                destination_shard,
                event,
                ..
            } => {
                let expected = target_shard(&self.plan, &event)?;
                if destination_shard != expected {
                    return Err(PartialShardExecutorError::DestinationMismatch {
                        declared: destination_shard,
                        expected,
                    });
                }
                Ok(PartialShardApply {
                    duplicate: !self.admit(event)?,
                    outbound: Vec::new(),
                })
            }
            PartialShardOutbound::SynapseEffect {
                event_id,
                logical_tag,
                synapse,
                charge,
                ..
            } => {
                let descriptor = self
                    .descriptors
                    .get(&synapse)
                    .ok_or(PartialShardExecutorError::MissingSynapse(synapse))?;
                if descriptor.owner != destination {
                    return Err(PartialShardExecutorError::WrongDestination(destination));
                }
                let control = ControlKey {
                    kind: ControlKind::SynapseEffect,
                    event_id,
                    synapse,
                };
                if !self.record_control(
                    control,
                    message_digest(&PartialShardOutbound::SynapseEffect {
                        plan_digest: self.plan_digest,
                        destination_shard: destination,
                        event_id,
                        logical_tag,
                        synapse,
                        charge,
                    })?,
                )? {
                    return Ok(PartialShardApply {
                        duplicate: true,
                        outbound: Vec::new(),
                    });
                }
                self.states
                    .get_mut(&destination)
                    .ok_or(PartialShardExecutorError::UnknownOwnedShard(destination))?
                    .apply_synapse_effect(synapse, charge)
                    .map_err(|error| PartialShardExecutorError::Biology(error.to_string()))?;
                Ok(PartialShardApply {
                    duplicate: false,
                    outbound: Vec::new(),
                })
            }
            PartialShardOutbound::SynapseActivation {
                parent_event,
                synapse,
                source,
                target,
                child_tag,
                ..
            } => {
                let descriptor = self
                    .descriptors
                    .get(&synapse)
                    .ok_or(PartialShardExecutorError::MissingSynapse(synapse))?
                    .clone();
                if descriptor.owner != destination
                    || descriptor.source != source
                    || descriptor.target != target
                {
                    return Err(PartialShardExecutorError::Event(
                        "synapse activation metadata does not match the plan".to_owned(),
                    ));
                }
                let control = ControlKey {
                    kind: ControlKind::SynapseActivation,
                    event_id: parent_event,
                    synapse,
                };
                if !self.record_control(
                    control,
                    message_digest(&PartialShardOutbound::SynapseActivation {
                        plan_digest: self.plan_digest,
                        destination_shard: destination,
                        parent_event,
                        synapse,
                        source,
                        target,
                        child_tag,
                    })?,
                )? {
                    return Ok(PartialShardApply {
                        duplicate: true,
                        outbound: Vec::new(),
                    });
                }
                let weight = self
                    .states
                    .get(&destination)
                    .and_then(|state| state.synapse(synapse))
                    .ok_or(PartialShardExecutorError::MissingSynapse(synapse))?
                    .weight;
                let event =
                    self.child_event(parent_event, &descriptor, source, target, child_tag, weight)?;
                let outbound = self.dispatch_child(event)?;
                Ok(PartialShardApply {
                    duplicate: false,
                    outbound,
                })
            }
        }
    }

    fn step_one(
        &mut self,
        shard: ShardId,
        tag: LogicalTag,
    ) -> Result<PartialShardStep, PartialShardExecutorError> {
        let event = self.pop_next(shard, tag)?;
        let target = NeuronId::new(event.event.key.target)
            .map_err(|_| PartialShardExecutorError::Event("invalid target".to_owned()))?;
        let transition = self
            .states
            .get_mut(&shard)
            .ok_or(PartialShardExecutorError::UnknownOwnedShard(shard))?
            .apply_neuron_event(&event.event)
            .map_err(|error| PartialShardExecutorError::Biology(error.to_string()))?;
        let mut outbound = Vec::new();
        for synapse in self.incoming.get(&target).cloned().unwrap_or_default() {
            let descriptor = self
                .descriptors
                .get(&synapse)
                .ok_or(PartialShardExecutorError::MissingSynapse(synapse))?;
            let charge = transition_charge(&event.event);
            if self.owned_shards.contains(&descriptor.owner) {
                self.states
                    .get_mut(&descriptor.owner)
                    .ok_or(PartialShardExecutorError::UnknownOwnedShard(
                        descriptor.owner,
                    ))?
                    .apply_synapse_effect(synapse, charge)
                    .map_err(|error| PartialShardExecutorError::Biology(error.to_string()))?;
            } else {
                self.push_outbound(
                    &mut outbound,
                    PartialShardOutbound::SynapseEffect {
                        plan_digest: self.plan_digest,
                        destination_shard: descriptor.owner,
                        event_id: event.event.id,
                        logical_tag: event.event.key.tag,
                        synapse,
                        charge,
                    },
                )?;
            }
        }
        for fired in &transition.fired {
            for synapse in self.outgoing.get(fired).cloned().unwrap_or_default() {
                let descriptor = self
                    .descriptors
                    .get(&synapse)
                    .ok_or(PartialShardExecutorError::MissingSynapse(synapse))?
                    .clone();
                let child_tag = event
                    .event
                    .key
                    .tag
                    .advance(descriptor.delay_ticks)
                    .map_err(|error| PartialShardExecutorError::Event(error.to_string()))?;
                if self.owned_shards.contains(&descriptor.owner) {
                    let weight = self
                        .states
                        .get(&descriptor.owner)
                        .and_then(|state| state.synapse(synapse))
                        .ok_or(PartialShardExecutorError::MissingSynapse(synapse))?
                        .weight;
                    let child = self.child_event(
                        event.event.id,
                        &descriptor,
                        *fired,
                        descriptor.target,
                        child_tag,
                        weight,
                    )?;
                    for message in self.dispatch_child(child)? {
                        self.push_outbound(&mut outbound, message)?;
                    }
                } else {
                    self.push_outbound(
                        &mut outbound,
                        PartialShardOutbound::SynapseActivation {
                            plan_digest: self.plan_digest,
                            destination_shard: descriptor.owner,
                            parent_event: event.event.id,
                            synapse,
                            source: *fired,
                            target: descriptor.target,
                            child_tag,
                        },
                    )?;
                }
            }
        }
        self.current_tag = self.current_tag.max(event.event.key.tag);
        self.committed.push(event.clone());
        Ok(PartialShardStep {
            logical_tag: event.event.key.tag,
            consumed: event,
            fired: transition.fired,
            outbound,
        })
    }

    fn child_event(
        &self,
        parent_event: EventId,
        descriptor: &SynapseDescriptor,
        source: NeuronId,
        target: NeuronId,
        child_tag: LogicalTag,
        weight: i64,
    ) -> Result<RoutedCausalEvent, PartialShardExecutorError> {
        let child_id = derive_child_event_id(parent_event, descriptor.id, child_tag, target)
            .map_err(|error| PartialShardExecutorError::Event(error.to_string()))?;
        if self.admitted.contains_key(&child_id) {
            return Err(PartialShardExecutorError::EventIdCollision(child_id));
        }
        let payload = serde_json::to_vec(&StableTransitionInput {
            schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
            source: Some(source),
            target,
            charge: weight / FIXED_POINT_SCALE,
            delay_ticks: 0,
        })
        .map_err(|error| PartialShardExecutorError::Event(error.to_string()))?;
        let from = self
            .plan
            .component_for_neuron(source)
            .ok_or_else(|| PartialShardExecutorError::Event("unknown source".to_owned()))?;
        let to = self
            .plan
            .component_for_neuron(target)
            .ok_or_else(|| PartialShardExecutorError::Event("unknown target".to_owned()))?;
        let route = if from == to {
            None
        } else {
            Some(
                self.plan
                    .route_for_synapse(descriptor.id)
                    .ok_or_else(|| PartialShardExecutorError::Event("missing route".to_owned()))?
                    .id,
            )
        };
        Ok(RoutedCausalEvent {
            route,
            event: CausalEvent {
                key: CanonicalEventKey::new(
                    child_tag,
                    EventStage::SynapticTransition,
                    source.raw(),
                    target.raw(),
                    child_id.raw(),
                ),
                id: child_id,
                payload,
                original_tag: child_tag,
                deferred_from_nonconvergence: false,
            },
        })
    }

    fn dispatch_child(
        &mut self,
        event: RoutedCausalEvent,
    ) -> Result<Vec<PartialShardOutbound>, PartialShardExecutorError> {
        let destination = target_shard(&self.plan, &event)?;
        if self.owned_shards.contains(&destination) {
            self.admit(event)?;
            Ok(Vec::new())
        } else {
            Ok(vec![PartialShardOutbound::CausalEvent {
                plan_digest: self.plan_digest,
                destination_shard: destination,
                event,
            }])
        }
    }

    fn record_control(
        &mut self,
        key: ControlKey,
        digest: StateDigest,
    ) -> Result<bool, PartialShardExecutorError> {
        if let Some(previous) = self.applied_controls.get(&key) {
            if previous == &digest {
                return Ok(false);
            }
            return Err(PartialShardExecutorError::ConflictingControl(key.event_id));
        }
        if self.applied_controls.len() >= self.dedupe_capacity {
            return Err(PartialShardExecutorError::DedupeWindowFull(
                self.dedupe_capacity,
            ));
        }
        self.applied_controls.insert(key, digest);
        Ok(true)
    }

    fn push_outbound(
        &self,
        outbound: &mut Vec<PartialShardOutbound>,
        message: PartialShardOutbound,
    ) -> Result<(), PartialShardExecutorError> {
        if outbound.len() >= self.max_outbound_per_step {
            return Err(PartialShardExecutorError::OutboundFull(
                self.max_outbound_per_step,
            ));
        }
        outbound.push(message);
        Ok(())
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
    ) -> Result<RoutedCausalEvent, PartialShardExecutorError> {
        let queues = self
            .pending
            .get_mut(&shard)
            .ok_or(PartialShardExecutorError::UnknownOwnedShard(shard))?;
        let events = queues
            .get_mut(&tag)
            .ok_or(PartialShardExecutorError::UnknownOwnedShard(shard))?;
        events.sort_by(|left, right| left.event.key.cmp(&right.event.key));
        let event = events.remove(0);
        if events.is_empty() {
            queues.remove(&tag);
        }
        Ok(event)
    }
}

fn target_shard(
    plan: &CompiledExecutionPlan,
    routed: &RoutedCausalEvent,
) -> Result<ShardId, PartialShardExecutorError> {
    let target = NeuronId::new(routed.event.key.target)
        .map_err(|_| PartialShardExecutorError::Event("invalid target".to_owned()))?;
    let target_component = plan
        .component_for_neuron(target)
        .ok_or_else(|| PartialShardExecutorError::Event("unknown target".to_owned()))?;
    let source_component = if routed.event.key.source == 0 {
        target_component
    } else {
        let source = NeuronId::new(routed.event.key.source)
            .map_err(|_| PartialShardExecutorError::Event("invalid source".to_owned()))?;
        plan.component_for_neuron(source)
            .ok_or_else(|| PartialShardExecutorError::Event("unknown source".to_owned()))?
    };
    plan.validate_event(
        plan.topology_generation(),
        plan.partition_generation(),
        source_component,
        target_component,
        routed.route,
    )
    .map_err(|error| PartialShardExecutorError::Event(error.to_string()))?;
    plan.neuron_owner(target)
        .ok_or_else(|| PartialShardExecutorError::Event("target has no owner".to_owned()))
}

fn transition_charge(event: &CausalEvent) -> i64 {
    serde_json::from_slice::<StableTransitionInput>(&event.payload)
        .map(|input| input.charge)
        .unwrap_or(0)
}

fn message_digest(
    message: &PartialShardOutbound,
) -> Result<StateDigest, PartialShardExecutorError> {
    let bytes = serde_json::to_vec(message)
        .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("partial-shard-control:v1", bytes);
    Ok(digest.finish())
}

#[derive(Serialize)]
struct PartialDigestMaterial<'a> {
    brain_id: BrainId,
    plan_digest: StateDigest,
    current_tag: LogicalTag,
    states: &'a BTreeMap<ShardId, Vec<u8>>,
    /// JSON object keys must be strings. Encode the logical-tag keyed event
    /// queues as an ordered sequence so digesting remains portable and does
    /// not fail when a worker has an admitted but unsettled event.
    pending: BTreeMap<ShardId, Vec<(LogicalTag, RoutedCausalEvent)>>,
    admitted: &'a BTreeMap<EventId, RoutedCausalEvent>,
}

impl PartialShardExecutor {
    pub fn state_digest(&self) -> Result<StateDigest, PartialShardExecutorError> {
        let states = self
            .states
            .iter()
            .map(|(shard, state)| {
                state
                    .encode()
                    .map(|bytes| (*shard, bytes))
                    .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let pending = self
            .pending
            .iter()
            .map(|(shard, queues)| {
                (
                    *shard,
                    queues
                        .iter()
                        .flat_map(|(tag, events)| events.iter().cloned().map(|event| (*tag, event)))
                        .collect(),
                )
            })
            .collect::<BTreeMap<_, Vec<_>>>();
        let bytes = serde_json::to_vec(&PartialDigestMaterial {
            brain_id: self.brain_id,
            plan_digest: self.plan_digest,
            current_tag: self.current_tag,
            states: &states,
            pending,
            admitted: &self.admitted,
        })
        .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("partial-shard-executor:v1", bytes);
        Ok(digest.finish())
    }

    /// Return the stable shard identities materialised by this worker in
    /// canonical order. The list is telemetry and migration evidence; it does
    /// not grant writer authority.
    pub fn owned_shard_ids(&self) -> Vec<ShardId> {
        self.owned_shards.iter().copied().collect()
    }

    /// Export the locally owned portion as stable checkpoints for migration.
    /// The original fabric digest is retained so a coordinator can prove that
    /// sibling shards came from the same source cut.
    pub fn checkpoint_shards(
        &self,
    ) -> Result<Vec<StableShardCheckpoint>, PartialShardExecutorError> {
        let mut result = Vec::with_capacity(self.states.len());
        for shard in &self.owned_shards {
            let state = self
                .states
                .get(shard)
                .ok_or(PartialShardExecutorError::UnknownOwnedShard(*shard))?;
            let mut pending = self
                .pending
                .get(shard)
                .into_iter()
                .flat_map(|queues| queues.values().flatten().cloned())
                .collect::<Vec<_>>();
            pending.sort_by(|left, right| left.event.key.cmp(&right.event.key));
            let mut admitted = self
                .admitted
                .values()
                .filter(|event| target_shard(&self.plan, event).ok() == Some(*shard))
                .cloned()
                .collect::<Vec<_>>();
            admitted.sort_by(|left, right| left.event.key.cmp(&right.event.key));
            let mut committed = self
                .committed
                .iter()
                .filter(|event| target_shard(&self.plan, event).ok() == Some(*shard))
                .cloned()
                .collect::<Vec<_>>();
            committed.sort_by(|left, right| left.event.key.cmp(&right.event.key));
            let checkpoint = StableShardCheckpoint {
                schema_version: crate::shard_executor::SHARD_EXECUTOR_SCHEMA_VERSION,
                brain_id: self.brain_id,
                shard_id: *shard,
                topology_generation: self.plan.topology_generation(),
                partition_generation: self.plan.partition_generation(),
                plan_digest: self.plan_digest,
                fabric_digest: self.fabric_digest,
                queue_capacity: self.queue_capacity,
                dedupe_capacity: self.dedupe_capacity,
                current_tag: self.current_tag,
                biological_state: state
                    .encode()
                    .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))?,
                pending,
                admitted,
                committed,
                checkpoint_digest: StateDigest([0; 16]),
            };
            result.push(
                crate::shard_executor::seal_checkpoint_for_transfer(checkpoint)
                    .map_err(|error| PartialShardExecutorError::Checkpoint(error.to_string()))?,
            );
        }
        Ok(result)
    }
}
