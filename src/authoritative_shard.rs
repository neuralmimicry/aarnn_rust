//! Authoritative shard owner for the distributed causal data plane.
//!
//! This module is the migration boundary between the biological kernel and
//! the distributed runtime.  A shard owns the durable biological bytes,
//! causal receipts, WAL, channel cut and warm replica.  Callers may supply a
//! pure transition, but they cannot publish a causal acknowledgement without
//! the shard validating the stream and current fencing decision first.
//!
//! The filesystem authority used here is a deterministic local deployment
//! adapter.  It is suitable for process/restart/fault tests; a production
//! deployment must bind the same interface to the approved replicated quorum
//! implementation before enabling the migration feature.

use crate::causal::CausalEvent;
use crate::data_plane::CausalEnvelope;
use crate::deterministic::{
    BrainId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest, StreamId,
    TopologyGeneration,
};
use crate::durability::{
    DurabilityError, DurableApplyOutcome, FileDurableShard, FileWarmReplica, ReceiptLedger,
    ShardCheckpointPayload, WalRecord,
};
use crate::management::{PersistedQuorumLeaseAuthority, ReplicatedQuorumLeaseAuthority};
use crate::peripheral::PeripheralCursorState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Schema version for the platform-neutral stable-ID biological state.
pub const BIOLOGICAL_STATE_SCHEMA_VERSION: u32 = 1;
/// Scale used by the portable fixed-point reference kernel.  Keeping this
/// public lets a multi-shard adapter construct the same transition payloads
/// without depending on private dense-kernel details.
pub const FIXED_POINT_SCALE: i64 = 1_000_000;

/// State carried by one stable biological neuron. Dense positions used by a
/// kernel are derived from the ordered IDs and are never persisted as the
/// identity of the neuron.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableNeuronState {
    pub id: crate::deterministic::NeuronId,
    pub membrane: i64,
    pub threshold: i64,
    pub refractory_until: LogicalTag,
    pub adaptation: i64,
}

/// All mutable synaptic fields have one stable owner in this state. The
/// representation is intentionally explicit so a checkpoint cannot omit
/// release or plasticity state while still looking like a valid model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableSynapseState {
    pub id: crate::deterministic::SynapseId,
    pub source: crate::deterministic::NeuronId,
    pub target: crate::deterministic::NeuronId,
    pub weight: i64,
    pub delay_ticks: u64,
    pub release_state: i64,
    pub plasticity_trace: i64,
}

/// A future event retained by the authoritative shard. Its tag is biological
/// time, never packet-arrival time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableFutureEvent {
    pub id: crate::deterministic::EventId,
    pub source: Option<crate::deterministic::NeuronId>,
    pub target: crate::deterministic::NeuronId,
    pub tag: LogicalTag,
    pub charge: i64,
}

/// Versioned input understood by the stable reference biological kernel.
/// Production adapters must translate their model-specific input into this
/// DTO before admission; no transport type is used as biological state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableTransitionInput {
    pub schema_version: u32,
    pub source: Option<crate::deterministic::NeuronId>,
    pub target: crate::deterministic::NeuronId,
    /// Charge in model units; the kernel converts it to its fixed-point scale.
    pub charge: i64,
    pub delay_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableTransitionResult {
    pub applied_tag: LogicalTag,
    pub fired: Vec<crate::deterministic::NeuronId>,
    pub queued: Vec<StableFutureEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StableBiologicalDocument {
    schema_version: u32,
    topology_generation: TopologyGeneration,
    neurons: Vec<StableNeuronState>,
    synapses: Vec<StableSynapseState>,
    future_events: Vec<StableFutureEvent>,
    committed_tag: LogicalTag,
    /// The complete generation-scoped neuron identity set.  A shard may hold
    /// a synapse whose remote endpoint is owned by another shard, so decoding
    /// must retain endpoint validation context without duplicating remote
    /// mutable neuron state.
    #[serde(default)]
    known_neurons: Vec<crate::deterministic::NeuronId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StableBiologyError {
    Encoding(String),
    UnsupportedSchema(u32),
    DuplicateNeuron(crate::deterministic::NeuronId),
    DuplicateSynapse(crate::deterministic::SynapseId),
    MissingNeuron(crate::deterministic::NeuronId),
    MissingSynapse(crate::deterministic::SynapseId),
    DuplicateFutureEvent(crate::deterministic::EventId),
    BackwardsTag {
        current: LogicalTag,
        received: LogicalTag,
    },
    InvalidTransition(String),
    Overflow,
}

impl std::fmt::Display for StableBiologyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encoding(error) => write!(formatter, "stable biology encoding failed: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported stable biology schema {version}")
            }
            Self::DuplicateNeuron(id) => write!(formatter, "duplicate stable neuron {id}"),
            Self::DuplicateSynapse(id) => write!(formatter, "duplicate stable synapse {id}"),
            Self::MissingNeuron(id) => {
                write!(formatter, "stable biology references missing neuron {id}")
            }
            Self::MissingSynapse(id) => {
                write!(formatter, "stable biology references missing synapse {id}")
            }
            Self::DuplicateFutureEvent(id) => write!(formatter, "duplicate future event {id}"),
            Self::BackwardsTag { current, received } => {
                write!(
                    formatter,
                    "stable biology tag moved backwards from {current} to {received}"
                )
            }
            Self::InvalidTransition(error) => {
                write!(formatter, "invalid stable transition: {error}")
            }
            Self::Overflow => write!(formatter, "stable biology arithmetic overflow"),
        }
    }
}

impl std::error::Error for StableBiologyError {}

/// Platform-neutral, stable-ID biological state owned by an authoritative
/// shard. This is a reference kernel for the migration boundary; it does not
/// alias or retain a mutable `Runner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableBiologicalState {
    topology_generation: TopologyGeneration,
    neurons: Vec<StableNeuronState>,
    synapses: Vec<StableSynapseState>,
    future_events: Vec<StableFutureEvent>,
    committed_tag: LogicalTag,
    known_neurons: BTreeSet<crate::deterministic::NeuronId>,
}

impl StableBiologicalState {
    pub fn new(
        topology_generation: TopologyGeneration,
        neurons: Vec<StableNeuronState>,
        synapses: Vec<StableSynapseState>,
    ) -> Result<Self, StableBiologyError> {
        let known_neurons = neurons.iter().map(|neuron| neuron.id).collect();
        Self::new_with_known_neurons(topology_generation, neurons, synapses, known_neurons, true)
    }

    /// Construct one shard's local state while validating cross-shard
    /// synapse endpoints against the complete topology identity set.  Remote
    /// endpoints are references only; their mutable neuron state remains in
    /// the owning shard.
    pub fn new_shard(
        topology_generation: TopologyGeneration,
        neurons: Vec<StableNeuronState>,
        synapses: Vec<StableSynapseState>,
        known_neurons: impl IntoIterator<Item = crate::deterministic::NeuronId>,
    ) -> Result<Self, StableBiologyError> {
        Self::new_with_known_neurons(
            topology_generation,
            neurons,
            synapses,
            known_neurons.into_iter().collect(),
            false,
        )
    }

    fn new_with_known_neurons(
        topology_generation: TopologyGeneration,
        mut neurons: Vec<StableNeuronState>,
        mut synapses: Vec<StableSynapseState>,
        mut known_neurons: BTreeSet<crate::deterministic::NeuronId>,
        require_local_endpoints: bool,
    ) -> Result<Self, StableBiologyError> {
        neurons.sort_by_key(|neuron| neuron.id);
        synapses.sort_by_key(|synapse| synapse.id);
        let mut neuron_ids = BTreeSet::new();
        for neuron in &neurons {
            if !neuron_ids.insert(neuron.id) {
                return Err(StableBiologyError::DuplicateNeuron(neuron.id));
            }
        }
        if require_local_endpoints {
            known_neurons = neuron_ids.clone();
        } else if !neuron_ids.is_subset(&known_neurons) {
            known_neurons.extend(neuron_ids.iter().copied());
        }
        let mut synapse_ids = BTreeSet::new();
        for synapse in &synapses {
            if !synapse_ids.insert(synapse.id) {
                return Err(StableBiologyError::DuplicateSynapse(synapse.id));
            }
            if !known_neurons.contains(&synapse.source) {
                return Err(StableBiologyError::MissingNeuron(synapse.source));
            }
            if !known_neurons.contains(&synapse.target) {
                return Err(StableBiologyError::MissingNeuron(synapse.target));
            }
        }
        Ok(Self {
            topology_generation,
            neurons,
            synapses,
            future_events: Vec::new(),
            committed_tag: LogicalTag::ZERO,
            known_neurons,
        })
    }

    pub fn from_topology(
        topology: &crate::topology_model::TopologyGenerationModel,
        threshold: i64,
        weight: i64,
    ) -> Result<Self, StableBiologyError> {
        let neurons = topology
            .neurons()
            .map(|neuron| StableNeuronState {
                id: neuron.id,
                membrane: 0,
                threshold,
                refractory_until: LogicalTag::ZERO,
                adaptation: 0,
            })
            .collect();
        let synapses = topology
            .synapses()
            .map(|synapse| StableSynapseState {
                id: synapse.id,
                source: synapse.source,
                target: synapse.target,
                weight,
                delay_ticks: synapse.delay_ticks,
                release_state: FIXED_POINT_SCALE,
                plasticity_trace: 0,
            })
            .collect();
        Self::new(topology.generation, neurons, synapses)
    }

    pub fn committed_tag(&self) -> LogicalTag {
        self.committed_tag
    }

    pub fn topology_generation(&self) -> TopologyGeneration {
        self.topology_generation
    }

    pub fn neurons(&self) -> &[StableNeuronState] {
        &self.neurons
    }

    pub fn synapses(&self) -> &[StableSynapseState] {
        &self.synapses
    }

    pub fn future_events(&self) -> &[StableFutureEvent] {
        &self.future_events
    }

    pub fn apply_event(
        &mut self,
        event: &CausalEvent,
    ) -> Result<StableTransitionResult, StableBiologyError> {
        self.apply_event_internal(event, true)
    }

    /// Apply a transition to the local neuron state without mutating synapse
    /// fields.  A multi-shard executor uses this method before routing the
    /// terminal/release/plasticity mutation to the single owner recorded in
    /// the compiled execution plan.
    pub fn apply_neuron_event(
        &mut self,
        event: &CausalEvent,
    ) -> Result<StableTransitionResult, StableBiologyError> {
        self.apply_event_internal(event, false)
    }

    fn apply_event_internal(
        &mut self,
        event: &CausalEvent,
        apply_synaptic_effect: bool,
    ) -> Result<StableTransitionResult, StableBiologyError> {
        if event.key.tag < self.committed_tag {
            return Err(StableBiologyError::BackwardsTag {
                current: self.committed_tag,
                received: event.key.tag,
            });
        }
        let input: StableTransitionInput = serde_json::from_slice(&event.payload)
            .map_err(|error| StableBiologyError::Encoding(error.to_string()))?;
        if input.schema_version != BIOLOGICAL_STATE_SCHEMA_VERSION {
            return Err(StableBiologyError::UnsupportedSchema(input.schema_version));
        }
        let canonical_source = if event.key.source == 0 {
            None
        } else {
            Some(
                crate::deterministic::NeuronId::new(event.key.source).map_err(|_| {
                    StableBiologyError::InvalidTransition("invalid event source".to_owned())
                })?,
            )
        };
        if input.source != canonical_source {
            return Err(StableBiologyError::InvalidTransition(
                "payload source does not match the canonical event source".to_owned(),
            ));
        }
        if event.key.target != 0 && event.key.target != input.target.raw() {
            return Err(StableBiologyError::InvalidTransition(
                "payload target does not match the canonical event target".to_owned(),
            ));
        }
        let target = self
            .neurons
            .iter_mut()
            .find(|neuron| neuron.id == input.target)
            .ok_or(StableBiologyError::MissingNeuron(input.target))?;
        let delta = input
            .charge
            .checked_mul(FIXED_POINT_SCALE)
            .ok_or(StableBiologyError::Overflow)?;
        target.membrane = target
            .membrane
            .checked_add(delta)
            .ok_or(StableBiologyError::Overflow)?;
        let mut fired = Vec::new();
        if event.key.tag >= target.refractory_until && target.membrane >= target.threshold {
            target.membrane = 0;
            target.refractory_until = event
                .key
                .tag
                .positive_delay(1)
                .map_err(|_| StableBiologyError::Overflow)?;
            target.adaptation = target
                .adaptation
                .checked_add(FIXED_POINT_SCALE)
                .ok_or(StableBiologyError::Overflow)?;
            fired.push(target.id);
        }
        let mut queued = Vec::new();
        if input.delay_ticks > 0 {
            let tag = event
                .key
                .tag
                .positive_delay(input.delay_ticks)
                .map_err(|_| StableBiologyError::Overflow)?;
            let queued_event = StableFutureEvent {
                id: event.id,
                source: input.source,
                target: input.target,
                tag,
                charge: input.charge,
            };
            if self
                .future_events
                .iter()
                .any(|existing| existing.id == queued_event.id)
            {
                return Err(StableBiologyError::DuplicateFutureEvent(queued_event.id));
            }
            self.future_events.push(queued_event.clone());
            self.future_events.sort_by_key(|item| (item.tag, item.id));
            queued.push(queued_event);
        }
        if apply_synaptic_effect {
            for synapse in &mut self.synapses {
                if synapse.target == input.target
                    && input.source.is_none_or(|source| synapse.source == source)
                {
                    synapse.release_state = synapse
                        .release_state
                        .checked_add(input.charge)
                        .ok_or(StableBiologyError::Overflow)?;
                    synapse.plasticity_trace = synapse
                        .plasticity_trace
                        .checked_add(FIXED_POINT_SCALE)
                        .ok_or(StableBiologyError::Overflow)?;
                }
            }
        }
        self.committed_tag = event.key.tag;
        Ok(StableTransitionResult {
            applied_tag: self.committed_tag,
            fired,
            queued,
        })
    }

    /// Apply the mutable synaptic fields for one owned synapse.  The caller
    /// must have validated that this state is the authoritative owner.
    pub fn apply_synapse_effect(
        &mut self,
        synapse_id: crate::deterministic::SynapseId,
        charge: i64,
    ) -> Result<(), StableBiologyError> {
        let synapse = self
            .synapses
            .iter_mut()
            .find(|synapse| synapse.id == synapse_id)
            .ok_or(StableBiologyError::MissingSynapse(synapse_id))?;
        synapse.release_state = synapse
            .release_state
            .checked_add(charge)
            .ok_or(StableBiologyError::Overflow)?;
        synapse.plasticity_trace = synapse
            .plasticity_trace
            .checked_add(FIXED_POINT_SCALE)
            .ok_or(StableBiologyError::Overflow)?;
        Ok(())
    }

    /// Read one mutable synapse owned by this shard.  The returned reference
    /// is short-lived so callers cannot retain mutable biological state across
    /// a durable boundary.
    pub fn synapse(
        &self,
        synapse_id: crate::deterministic::SynapseId,
    ) -> Option<&StableSynapseState> {
        self.synapses
            .iter()
            .find(|synapse| synapse.id == synapse_id)
    }

    pub fn synapses_from(
        &self,
        source: crate::deterministic::NeuronId,
    ) -> impl Iterator<Item = &StableSynapseState> {
        self.synapses
            .iter()
            .filter(move |synapse| synapse.source == source)
    }

    pub fn encode(&self) -> Result<Vec<u8>, StableBiologyError> {
        serde_json::to_vec(&StableBiologicalDocument {
            schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
            topology_generation: self.topology_generation,
            neurons: self.neurons.clone(),
            synapses: self.synapses.clone(),
            future_events: self.future_events.clone(),
            committed_tag: self.committed_tag,
            known_neurons: self.known_neurons.iter().copied().collect(),
        })
        .map_err(|error| StableBiologyError::Encoding(error.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, StableBiologyError> {
        let document: StableBiologicalDocument = serde_json::from_slice(bytes)
            .map_err(|error| StableBiologyError::Encoding(error.to_string()))?;
        if document.schema_version != BIOLOGICAL_STATE_SCHEMA_VERSION {
            return Err(StableBiologyError::UnsupportedSchema(
                document.schema_version,
            ));
        }
        let known_neurons = if document.known_neurons.is_empty() {
            document.neurons.iter().map(|neuron| neuron.id).collect()
        } else {
            document.known_neurons.into_iter().collect()
        };
        let mut state = Self::new_with_known_neurons(
            document.topology_generation,
            document.neurons,
            document.synapses,
            known_neurons,
            false,
        )?;
        let mut event_ids = BTreeSet::new();
        for event in document.future_events {
            if !event_ids.insert(event.id) {
                return Err(StableBiologyError::DuplicateFutureEvent(event.id));
            }
            if !state.neurons.iter().any(|neuron| neuron.id == event.target) {
                return Err(StableBiologyError::MissingNeuron(event.target));
            }
            state.future_events.push(event);
        }
        state.future_events.sort_by_key(|item| (item.tag, item.id));
        state.committed_tag = document.committed_tag;
        Ok(state)
    }

    pub fn state_digest(&self) -> Result<StateDigest, StableBiologyError> {
        let bytes = self.encode()?;
        let mut digest = crate::deterministic::StateDigestBuilder::default();
        digest.add_domain("stable-biological-state:v1", bytes);
        Ok(digest.finish())
    }
}

#[derive(Clone)]
enum FencingBinding {
    Single {
        path: PathBuf,
        members: Vec<String>,
    },
    Replicated {
        replicas: Vec<(String, PathBuf)>,
        members: Vec<String>,
    },
}

/// The single read model for an authoritative shard.
///
/// This is a typed copy of the complete sealed checkpoint, not a collection
/// of independently-read biological/channel projections. Biological bytes
/// remain owned by the model schema; causal receipts, channel state,
/// generations and fencing metadata are explicit here so a biological
/// snapshot cannot be mistaken for a complete recovery point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardState {
    pub schema_version: crate::deterministic::SchemaVersion,
    pub brain_id: BrainId,
    pub shard_id: ShardId,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub lease_term: LeaseTerm,
    pub committed_tag: LogicalTag,
    pub applied_tag: LogicalTag,
    pub durable_wal_sequence: Option<u64>,
    pub biological_state: Vec<u8>,
    pub causal_state: Vec<u8>,
    pub channel_state: Vec<u8>,
    pub peripheral_state: PeripheralCursorState,
    pub receipts: ReceiptLedger,
    pub state_digest: StateDigest,
}

impl TryFrom<ShardCheckpointPayload> for ShardState {
    type Error = DurabilityError;

    fn try_from(checkpoint: ShardCheckpointPayload) -> Result<Self, Self::Error> {
        checkpoint.verify()?;
        Ok(Self {
            schema_version: checkpoint.schema_version,
            brain_id: checkpoint.brain_id,
            shard_id: checkpoint.shard_id,
            topology_generation: checkpoint.topology_generation,
            partition_generation: checkpoint.partition_generation,
            lease_term: checkpoint.lease_term,
            committed_tag: checkpoint.committed_tag,
            applied_tag: checkpoint.applied_tag,
            durable_wal_sequence: checkpoint.durable_wal_sequence,
            biological_state: checkpoint.biological_state,
            causal_state: checkpoint.causal_state,
            channel_state: checkpoint.channel_state,
            peripheral_state: checkpoint.peripheral_state,
            receipts: checkpoint.receipts,
            state_digest: checkpoint.state_digest,
        })
    }
}

impl ShardState {
    pub fn verify(&self) -> Result<(), DurabilityError> {
        let checkpoint = ShardCheckpointPayload {
            schema_version: self.schema_version,
            brain_id: self.brain_id,
            shard_id: self.shard_id,
            topology_generation: self.topology_generation,
            partition_generation: self.partition_generation,
            lease_term: self.lease_term,
            committed_tag: self.committed_tag,
            applied_tag: self.applied_tag,
            durable_wal_sequence: self.durable_wal_sequence,
            biological_state: self.biological_state.clone(),
            causal_state: self.causal_state.clone(),
            channel_state: self.channel_state.clone(),
            peripheral_state: self.peripheral_state.clone(),
            receipts: self.receipts.clone(),
            state_digest: self.state_digest,
        };
        checkpoint.verify()
    }
}

impl std::fmt::Debug for FencingBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FencingBinding")
            .finish_non_exhaustive()
    }
}

/// One authoritative, durable shard and its optional process-visible warm
/// replica.  The type intentionally exposes no mutable biological buffers;
/// mutation happens only through [`Self::apply`].
#[derive(Debug)]
pub struct AuthoritativeShard {
    brain_id: BrainId,
    shard_id: ShardId,
    stream_id: StreamId,
    topology_generation: TopologyGeneration,
    partition_generation: PartitionGeneration,
    owner: FileDurableShard,
    warm: Option<FileWarmReplica>,
    term: LeaseTerm,
    fencing: Option<(FencingBinding, String, u64)>,
}

impl AuthoritativeShard {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        owner_path: impl Into<PathBuf>,
        warm_path: Option<impl Into<PathBuf>>,
        brain_id: BrainId,
        shard_id: ShardId,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
        biological_state: Vec<u8>,
        channel_state: Vec<u8>,
    ) -> Result<Self, DurabilityError> {
        let owner_path = owner_path.into();
        let owner = FileDurableShard::open(
            &owner_path,
            brain_id,
            shard_id,
            topology_generation,
            partition_generation,
            term,
            stream_id,
            max_payload,
            biological_state,
            channel_state,
        )?;
        let warm = warm_path
            .map(|path| {
                let checkpoint = owner.checkpoint_payload()?;
                let records: Vec<WalRecord> = serde_json::from_slice(&checkpoint.causal_state)
                    .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
                FileWarmReplica::open_with_checkpoint(path.into(), term, records, Some(checkpoint))
            })
            .transpose()?;
        Ok(Self {
            brain_id,
            shard_id,
            stream_id,
            topology_generation,
            partition_generation,
            owner,
            warm,
            term,
            fencing: None,
        })
    }

    /// Recover a new owner from the complete warm checkpoint and immediately
    /// re-sign it under the newly issued term.  The old active path is never
    /// overwritten, which makes the recovery decision auditable and lets the
    /// old process fail closed when it next checks its authority.
    pub fn recover_from_warm(
        owner_path: impl Into<PathBuf>,
        warm_path: impl Into<PathBuf>,
        term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<Self, DurabilityError> {
        let warm_path = warm_path.into();
        let warm_term = FileWarmReplica::persisted_term(&warm_path)?.ok_or_else(|| {
            DurabilityError::Corrupt("warm replica has no persisted active term".to_owned())
        })?;
        let mut warm = FileWarmReplica::open(&warm_path, warm_term, [])?;
        let checkpoint = warm.recovery_checkpoint()?.ok_or_else(|| {
            DurabilityError::Corrupt("warm replica has no complete checkpoint".to_owned())
        })?;
        let brain_id = checkpoint.brain_id;
        let shard_id = checkpoint.shard_id;
        let topology_generation = checkpoint.topology_generation;
        let partition_generation = checkpoint.partition_generation;
        let owner_path = owner_path.into();
        let mut owner = FileDurableShard::restore_from_checkpoint(
            &owner_path,
            checkpoint,
            stream_id,
            max_payload,
        )?;
        if term > warm_term {
            owner.reissue_term(term)?;
            warm.reissue_term(term)?;
        } else if term < warm_term {
            return Err(DurabilityError::StaleTerm {
                expected: warm_term,
                received: term,
            });
        }
        let records: Vec<WalRecord> =
            serde_json::from_slice(&owner.checkpoint_payload()?.causal_state)
                .map_err(|error| DurabilityError::Encoding(error.to_string()))?;
        let warm = FileWarmReplica::open_with_checkpoint(
            &warm_path,
            term,
            records,
            Some(owner.checkpoint_payload()?),
        )?;
        Ok(Self {
            brain_id,
            shard_id,
            stream_id,
            topology_generation,
            partition_generation,
            owner,
            warm: Some(warm),
            term,
            fencing: None,
        })
    }

    /// Bind the shard to the persisted control-plane lease.  Binding is
    /// required for the authoritative service; an unbound shard remains
    /// useful only for isolated deterministic unit tests.
    pub fn bind_fencing(
        &mut self,
        authority_path: impl Into<PathBuf>,
        members: Vec<String>,
        node_id: impl Into<String>,
        fencing_token: u64,
    ) {
        self.fencing = Some((
            FencingBinding::Single {
                path: authority_path.into(),
                members,
            },
            node_id.into(),
            fencing_token,
        ));
    }

    /// Bind against a majority-replicated authority document set.  The
    /// authority is reopened for every commit so a long-lived shard observes
    /// revocations made by another control-plane process.
    pub fn bind_replicated_fencing(
        &mut self,
        replicas: Vec<(String, PathBuf)>,
        members: Vec<String>,
        node_id: impl Into<String>,
        fencing_token: u64,
    ) {
        self.fencing = Some((
            FencingBinding::Replicated { replicas, members },
            node_id.into(),
            fencing_token,
        ));
    }

    fn validate_fence(&self, envelope: &CausalEnvelope) -> Result<(), DurabilityError> {
        // The owner stream is the local mirror stream. External producer
        // streams are admitted by the durable receiver map below, so stream
        // identity must not be rejected at this shard-wide fence.
        if envelope.brain != self.brain_id
            || envelope.partition_generation != self.partition_generation
            || envelope.lease_term != self.term
        {
            return Err(DurabilityError::DataPlane(
                crate::data_plane::DataPlaneError::StaleGeneration {
                    expected: self.partition_generation,
                    received: envelope.partition_generation,
                },
            ));
        }
        if let Some((binding, node_id, fencing_token)) = &self.fencing {
            match binding {
                FencingBinding::Single { path, members } => {
                    let authority = PersistedQuorumLeaseAuthority::open(path, members.clone())
                        .map_err(|error| DurabilityError::Authority(error.to_string()))?;
                    authority
                        .validate_current(self.shard_id, node_id, self.term, *fencing_token)
                        .map_err(|error| DurabilityError::Authority(error.to_string()))?;
                }
                FencingBinding::Replicated { replicas, members } => {
                    let authority =
                        ReplicatedQuorumLeaseAuthority::open(replicas.clone(), members.clone())
                            .map_err(|error| DurabilityError::Authority(error.to_string()))?;
                    authority
                        .validate_current(self.shard_id, node_id, self.term, *fencing_token)
                        .map_err(|error| DurabilityError::Authority(error.to_string()))?;
                }
            }
        }
        Ok(())
    }

    /// Apply one causal event.  The transition is evaluated against the
    /// previous immutable byte payload and only the fully replicated result is
    /// published.  Exact retransmission is a durable no-op.
    pub fn apply<F, E>(
        &mut self,
        envelope: &CausalEnvelope,
        channel_state: Vec<u8>,
        transition: F,
    ) -> Result<DurableApplyOutcome, DurabilityError>
    where
        F: FnOnce(&[u8], &CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        self.validate_fence(envelope)?;
        match self.warm.as_mut() {
            Some(warm) => self.owner.apply_once_with_warm_replica_and_channel_state(
                envelope,
                warm,
                channel_state,
                transition,
            ),
            None => self
                .owner
                .apply_once_with_channel_state(envelope, channel_state, transition),
        }
    }

    /// Apply a versioned stable-ID biological transition at the same durable
    /// boundary as causal receipt, WAL and warm replication. The biological
    /// state is decoded into a private candidate first; a failed transition
    /// cannot publish a receipt or advance the shard.
    pub fn apply_stable_event(
        &mut self,
        envelope: &CausalEnvelope,
        channel_state: Vec<u8>,
    ) -> Result<(DurableApplyOutcome, StableTransitionResult), DurabilityError> {
        let current = StableBiologicalState::decode(self.owner.biological_state())
            .map_err(|error| DurabilityError::Transition(error.to_string()))?;
        let event = CausalEvent::new(
            envelope.event,
            crate::deterministic::CanonicalEventKey::new(
                envelope.tag,
                envelope.stage,
                envelope.source.map(|id| id.raw()).unwrap_or(0),
                envelope.target.map(|id| id.raw()).unwrap_or(0),
                envelope.event.raw(),
            ),
            envelope.payload.clone(),
        );
        let mut candidate = current;
        let result = candidate
            .apply_event(&event)
            .map_err(|error| DurabilityError::Transition(error.to_string()))?;
        let biological_state = candidate
            .encode()
            .map_err(|error| DurabilityError::Transition(error.to_string()))?;
        let outcome = self.apply(envelope, channel_state, move |_, _| {
            Ok::<Vec<u8>, DurabilityError>(biological_state)
        })?;
        if matches!(outcome, DurableApplyOutcome::Duplicate { .. }) {
            return Ok((
                outcome,
                StableTransitionResult {
                    applied_tag: self.owner.shard().applied_tag(),
                    fired: Vec::new(),
                    queued: Vec::new(),
                },
            ));
        }
        Ok((outcome, result))
    }

    /// Persist a precomputed stable-shard checkpoint through the same causal
    /// receipt, WAL and warm-replica boundary as an ordinary transition.
    ///
    /// The stable multi-shard executor owns deterministic execution and may
    /// need to calculate a complete fabric step before any individual shard
    /// publishes its resulting bytes.  This method is the narrow durable
    /// handoff: it refuses a stale mirror, then treats the supplied biological
    /// bytes as the staged result of the envelope.  The caller remains
    /// responsible for coordinating the sibling cut and fencing; this method
    /// never makes a partial shard update look like a complete brain cut.
    pub fn apply_stable_checkpoint(
        &mut self,
        envelope: &CausalEnvelope,
        expected_previous_state_digest: StateDigest,
        next_biological_state: Vec<u8>,
        channel_state: Vec<u8>,
    ) -> Result<DurableApplyOutcome, DurabilityError> {
        let current = self.checkpoint()?;
        let already_admitted = current
            .receipts
            .contains(envelope.stream, envelope.sequence);
        if current.state_digest != expected_previous_state_digest && !already_admitted {
            return Err(DurabilityError::Corrupt(
                "stable-shard durable mirror is behind or diverged from the expected cut"
                    .to_owned(),
            ));
        }
        if already_admitted && current.biological_state != next_biological_state {
            return Err(DurabilityError::Corrupt(
                "stable-shard duplicate has different resulting biological bytes".to_owned(),
            ));
        }
        self.apply(envelope, channel_state, move |_, _| {
            Ok::<Vec<u8>, DurabilityError>(next_biological_state)
        })
    }

    pub fn checkpoint(&self) -> Result<ShardCheckpointPayload, DurabilityError> {
        self.owner.checkpoint_payload()
    }

    /// Return the complete immutable state boundary used by snapshots,
    /// recovery and causal acknowledgement. Every field is obtained from one
    /// sealed checkpoint, so callers cannot observe a mixed biological and
    /// channel frontier.
    pub fn state(&self) -> Result<ShardState, DurabilityError> {
        self.checkpoint()?.try_into()
    }

    pub fn biological_state(&self) -> &[u8] {
        self.owner.biological_state()
    }

    pub fn channel_state(&self) -> &[u8] {
        self.owner.channel_state()
    }

    pub fn peripheral_cursor_state(&self) -> Result<PeripheralCursorState, DurabilityError> {
        Ok(self.owner.checkpoint_payload()?.peripheral_state)
    }

    /// Publish peripheral admission/effect cursors without advancing neural
    /// time. The cursor DTO is validated and persisted as part of one sealed
    /// checkpoint, so a migration cannot observe a mixed biological/effect
    /// boundary.
    pub fn set_peripheral_cursor_state(
        &mut self,
        state: PeripheralCursorState,
    ) -> Result<(), DurabilityError> {
        match self.warm.as_mut() {
            Some(warm) => self
                .owner
                .set_peripheral_cursor_state_with_warm(warm, state),
            None => self.owner.set_peripheral_cursor_state(state),
        }
    }

    pub const fn term(&self) -> LeaseTerm {
        self.term
    }

    pub const fn shard_id(&self) -> ShardId {
        self.shard_id
    }

    pub const fn topology_generation(&self) -> TopologyGeneration {
        self.topology_generation
    }

    pub const fn partition_generation(&self) -> PartitionGeneration {
        self.partition_generation
    }

    pub fn durable_sequence(&self) -> Option<u64> {
        self.owner.shard().durable_log_sequence()
    }

    pub fn receipt_count(&self) -> usize {
        self.owner.shard().receipt_count()
    }

    /// Return the durable producer receipts for one transport stream. The
    /// caller uses this only to reconstruct a bounded reconnect cursor; it
    /// does not expose mutable biological state.
    pub fn stream_receipts(
        &self,
        stream_id: crate::deterministic::StreamId,
    ) -> Vec<crate::durability::DurableReceipt> {
        self.owner.shard().stream_receipts(stream_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_plane::EnvelopeKind;
    use crate::deterministic::{EventId, EventStage, LogicalTag, RouteId, SchemaVersion};
    use crate::management::ReplicatedQuorumLeaseAuthority;
    use std::fs;

    fn envelope(
        brain: BrainId,
        stream: StreamId,
        term: LeaseTerm,
        sequence: u64,
    ) -> CausalEnvelope {
        CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain,
            stream,
            sequence,
            lease_term: term,
            route: RouteId::new(9).unwrap(),
            partition_generation: PartitionGeneration::INITIAL,
            source: None,
            target: None,
            tag: LogicalTag::new(sequence + 1, 0),
            event: EventId::new(sequence + 1).unwrap(),
            stage: EventStage::SynapticTransition,
            kind: EnvelopeKind::Event,
            payload: vec![sequence as u8 + 1],
            deferred_from_nonconvergence: false,
        }
    }

    #[test]
    fn shard_owns_replication_and_restarts_from_the_same_checkpoint() {
        let root =
            std::env::temp_dir().join(format!("aarnn-authoritative-shard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let owner_path = root.join("active.json");
        let warm_path = root.join("warm.json");
        let brain = BrainId::new(101).unwrap();
        let stream = StreamId::new(102).unwrap();
        let mut shard = AuthoritativeShard::open(
            &owner_path,
            Some(&warm_path),
            brain,
            ShardId::new(103).unwrap(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            stream,
            1024,
            vec![0],
            vec![],
        )
        .unwrap();
        for sequence in 0..3 {
            let frame = envelope(brain, stream, LeaseTerm::INITIAL, sequence);
            shard
                .apply(frame_ref(&frame), vec![sequence as u8], |current, event| {
                    Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
                })
                .unwrap();
        }
        assert_eq!(shard.biological_state(), &[6]);
        assert_eq!(shard.durable_sequence(), Some(2));
        drop(shard);

        let reopened = AuthoritativeShard::open(
            &owner_path,
            Some(&warm_path),
            brain,
            ShardId::new(103).unwrap(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            stream,
            1024,
            vec![99],
            vec![],
        )
        .unwrap();
        assert_eq!(reopened.biological_state(), &[6]);
        assert_eq!(reopened.channel_state(), &[2]);
        assert_eq!(reopened.receipt_count(), 3);
        let state = reopened.state().expect("complete authoritative state");
        assert_eq!(state.biological_state, vec![6]);
        assert_eq!(state.channel_state, vec![2]);
        assert_eq!(state.receipts.len(), 3);
        state.verify().expect("state digest verifies");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn peripheral_cursor_update_is_atomic_and_survives_owner_restart() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-peripheral-cursor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let brain = BrainId::new(111).unwrap();
        let shard_id = ShardId::new(112).unwrap();
        let stream = StreamId::new(113).unwrap();
        let mut shard = AuthoritativeShard::open(
            root.join("active.json"),
            Some(root.join("warm.json")),
            brain,
            shard_id,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            stream,
            1024,
            vec![0],
            vec![],
        )
        .unwrap();
        let effect_channel = StreamId::new(114).unwrap();
        let effect = crate::peripheral::PeripheralCursorState {
            schema_version: crate::peripheral::PERIPHERAL_CURSOR_SCHEMA_VERSION,
            admissions: Vec::new(),
            effects: vec![crate::peripheral::PeripheralEffectCursor {
                channel: effect_channel,
                device_epoch: 2,
                lease_term: LeaseTerm::INITIAL,
                armed: true,
                accepted_effect_ids: vec![EventId::new(115).unwrap()],
            }],
        };
        shard
            .set_peripheral_cursor_state(effect.clone())
            .expect("cursor checkpoint");
        assert_eq!(shard.peripheral_cursor_state().unwrap(), effect);
        drop(shard);

        let reopened = AuthoritativeShard::open(
            root.join("active.json"),
            Some(root.join("warm.json")),
            brain,
            shard_id,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            stream,
            1024,
            vec![99],
            vec![],
        )
        .unwrap();
        assert_eq!(reopened.peripheral_cursor_state().unwrap(), effect);
        fs::remove_dir_all(root).unwrap();
    }

    fn frame_ref(frame: &CausalEnvelope) -> &CausalEnvelope {
        frame
    }

    fn stable_state() -> StableBiologicalState {
        StableBiologicalState::new(
            TopologyGeneration::INITIAL,
            vec![
                StableNeuronState {
                    id: crate::deterministic::NeuronId::new(501).unwrap(),
                    membrane: 0,
                    threshold: 2 * FIXED_POINT_SCALE,
                    refractory_until: LogicalTag::ZERO,
                    adaptation: 0,
                },
                StableNeuronState {
                    id: crate::deterministic::NeuronId::new(502).unwrap(),
                    membrane: 0,
                    threshold: 2 * FIXED_POINT_SCALE,
                    refractory_until: LogicalTag::ZERO,
                    adaptation: 0,
                },
            ],
            vec![StableSynapseState {
                id: crate::deterministic::SynapseId::new(503).unwrap(),
                source: crate::deterministic::NeuronId::new(501).unwrap(),
                target: crate::deterministic::NeuronId::new(502).unwrap(),
                weight: FIXED_POINT_SCALE,
                delay_ticks: 1,
                release_state: FIXED_POINT_SCALE,
                plasticity_trace: 0,
            }],
        )
        .unwrap()
    }

    fn stable_envelope(sequence: u64, tag: LogicalTag, event: u64) -> CausalEnvelope {
        let source = crate::deterministic::NeuronId::new(501).unwrap();
        let target = crate::deterministic::NeuronId::new(502).unwrap();
        let payload = serde_json::to_vec(&StableTransitionInput {
            schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
            source: Some(source),
            target,
            charge: 1,
            delay_ticks: 0,
        })
        .unwrap();
        CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain: BrainId::new(601).unwrap(),
            stream: StreamId::new(602).unwrap(),
            sequence,
            lease_term: LeaseTerm::INITIAL,
            route: RouteId::new(603).unwrap(),
            partition_generation: PartitionGeneration::INITIAL,
            source: Some(source),
            target: Some(target),
            tag,
            event: EventId::new(event).unwrap(),
            stage: EventStage::SynapticTransition,
            kind: EnvelopeKind::Event,
            payload,
            deferred_from_nonconvergence: false,
        }
    }

    #[test]
    fn stable_biological_state_is_id_owned_deterministic_and_replayable() {
        let mut state = stable_state();
        let first = stable_envelope(0, LogicalTag::new(1, 0), 701);
        let first_event = CausalEvent::new(
            first.event,
            crate::deterministic::CanonicalEventKey::new(
                first.tag,
                first.stage,
                first.source.unwrap().raw(),
                first.target.unwrap().raw(),
                first.event.raw(),
            ),
            first.payload.clone(),
        );
        let result = state.apply_event(&first_event).unwrap();
        assert!(result.fired.is_empty());
        assert_eq!(state.committed_tag(), LogicalTag::new(1, 0));
        let second = stable_envelope(1, LogicalTag::new(1, 1), 702);
        let second_event = CausalEvent::new(
            second.event,
            crate::deterministic::CanonicalEventKey::new(
                second.tag,
                second.stage,
                second.source.unwrap().raw(),
                second.target.unwrap().raw(),
                second.event.raw(),
            ),
            second.payload.clone(),
        );
        let result = state.apply_event(&second_event).unwrap();
        assert_eq!(
            result.fired,
            vec![crate::deterministic::NeuronId::new(502).unwrap()]
        );
        let encoded = state.encode().unwrap();
        let restored = StableBiologicalState::decode(&encoded).unwrap();
        assert_eq!(restored, state);
        assert_eq!(
            restored.state_digest().unwrap(),
            state.state_digest().unwrap()
        );
    }

    #[test]
    fn stable_shard_state_round_trip_retains_remote_synapse_endpoints() {
        let local = crate::deterministic::NeuronId::new(601).unwrap();
        let remote = crate::deterministic::NeuronId::new(602).unwrap();
        let synapse = StableSynapseState {
            id: crate::deterministic::SynapseId::new(603).unwrap(),
            source: local,
            target: remote,
            weight: FIXED_POINT_SCALE,
            delay_ticks: 1,
            release_state: FIXED_POINT_SCALE,
            plasticity_trace: 0,
        };
        let state = StableBiologicalState::new_shard(
            TopologyGeneration::INITIAL,
            vec![StableNeuronState {
                id: local,
                membrane: 0,
                threshold: FIXED_POINT_SCALE,
                refractory_until: LogicalTag::ZERO,
                adaptation: 0,
            }],
            vec![synapse.clone()],
            [local, remote],
        )
        .unwrap();

        let restored = StableBiologicalState::decode(&state.encode().unwrap()).unwrap();
        assert_eq!(restored, state);
        assert_eq!(restored.synapses(), &[synapse]);
        assert_eq!(restored.neurons().len(), 1);
    }

    #[test]
    fn stable_transition_is_committed_with_receipt_and_warm_checkpoint() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-stable-authority-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let biology = stable_state().encode().unwrap();
        let mut shard = AuthoritativeShard::open(
            root.join("active.json"),
            Some(root.join("warm.json")),
            BrainId::new(601).unwrap(),
            ShardId::new(604).unwrap(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(602).unwrap(),
            64 * 1024,
            biology,
            vec![],
        )
        .unwrap();
        let frame = stable_envelope(0, LogicalTag::new(1, 0), 701);
        let (outcome, _) = shard.apply_stable_event(&frame, vec![9]).unwrap();
        assert_eq!(outcome, DurableApplyOutcome::Applied { sequence: 0 });
        assert_eq!(shard.receipt_count(), 1);
        assert_eq!(shard.channel_state(), &[9]);
        let state = StableBiologicalState::decode(shard.biological_state()).unwrap();
        assert_eq!(state.neurons()[1].membrane, FIXED_POINT_SCALE);
        drop(shard);

        let reopened = AuthoritativeShard::open(
            root.join("active.json"),
            Some(root.join("warm.json")),
            BrainId::new(601).unwrap(),
            ShardId::new(604).unwrap(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(602).unwrap(),
            64 * 1024,
            vec![99],
            vec![],
        )
        .unwrap();
        assert_eq!(reopened.receipt_count(), 1);
        assert_eq!(reopened.channel_state(), &[9]);
        assert_eq!(
            StableBiologicalState::decode(reopened.biological_state())
                .unwrap()
                .neurons()[1]
                .membrane,
            FIXED_POINT_SCALE
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_term_is_rejected_before_transition_execution() {
        let root =
            std::env::temp_dir().join(format!("aarnn-authoritative-fence-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut shard = AuthoritativeShard::open(
            root.join("active.json"),
            None::<PathBuf>,
            BrainId::new(201).unwrap(),
            ShardId::new(202).unwrap(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(203).unwrap(),
            1024,
            vec![0],
            vec![],
        )
        .unwrap();
        let frame = envelope(
            BrainId::new(201).unwrap(),
            StreamId::new(203).unwrap(),
            LeaseTerm::new(2).unwrap(),
            0,
        );
        let mut called = false;
        let result = shard.apply(&frame, vec![], |_, _| {
            called = true;
            Ok::<_, &'static str>(vec![1])
        });
        assert!(result.is_err());
        assert!(!called);
        assert_eq!(shard.durable_sequence(), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn warm_checkpoint_can_rejoin_under_a_new_term_without_replaying_events() {
        let root =
            std::env::temp_dir().join(format!("aarnn-authoritative-rejoin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let brain = BrainId::new(301).unwrap();
        let stream = StreamId::new(302).unwrap();
        let shard_id = ShardId::new(303).unwrap();
        let warm_path = root.join("warm.json");
        let mut active = AuthoritativeShard::open(
            root.join("active.json"),
            Some(&warm_path),
            brain,
            shard_id,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            stream,
            1024,
            vec![0],
            vec![],
        )
        .unwrap();
        let first = envelope(brain, stream, LeaseTerm::INITIAL, 0);
        active
            .apply(&first, vec![], |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .unwrap();
        drop(active);

        let next_term = LeaseTerm::new(2).unwrap();
        let mut rejoined = AuthoritativeShard::recover_from_warm(
            root.join("rejoined.json"),
            &warm_path,
            next_term,
            stream,
            1024,
        )
        .unwrap();
        assert_eq!(rejoined.biological_state(), &[1]);
        assert_eq!(rejoined.receipt_count(), 1);
        let second = envelope(brain, stream, next_term, 1);
        rejoined
            .apply(&second, vec![], |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .unwrap();
        assert_eq!(rejoined.biological_state(), &[3]);
        assert_eq!(rejoined.receipt_count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replicated_quorum_fences_old_owner_before_causal_apply() {
        let root =
            std::env::temp_dir().join(format!("aarnn-authoritative-quorum-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let members = ["cp-a", "cp-b", "cp-c"];
        let replicas = members
            .iter()
            .map(|member| ((*member).to_owned(), root.join(format!("{member}.json"))))
            .collect::<Vec<_>>();
        let mut quorum = ReplicatedQuorumLeaseAuthority::open(
            replicas.clone(),
            members.iter().map(|member| (*member).to_owned()),
        )
        .unwrap();
        let shard_id = ShardId::new(403).unwrap();
        let first_lease = quorum.issue_lease(shard_id, "cp-a").unwrap();
        let brain = BrainId::new(401).unwrap();
        let stream = StreamId::new(402).unwrap();
        let warm_path = root.join("warm.json");
        let mut active = AuthoritativeShard::open(
            root.join("active.json"),
            Some(&warm_path),
            brain,
            shard_id,
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            first_lease.term,
            stream,
            1024,
            vec![0],
            vec![],
        )
        .unwrap();
        active.bind_replicated_fencing(
            replicas.clone(),
            members.iter().map(|member| (*member).to_owned()).collect(),
            "cp-a",
            first_lease.fencing_token,
        );
        let first = envelope(brain, stream, first_lease.term, 0);
        active
            .apply(&first, vec![], |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .unwrap();
        let replacement = quorum.issue_lease(shard_id, "cp-b").unwrap();
        let stale = envelope(brain, stream, first_lease.term, 1);
        assert!(
            active
                .apply(&stale, vec![], |_, _| -> Result<Vec<u8>, &'static str> {
                    panic!("fenced transition must not run")
                })
                .is_err()
        );

        drop(active);
        let mut rejoined = AuthoritativeShard::recover_from_warm(
            root.join("rejoined.json"),
            &warm_path,
            replacement.term,
            stream,
            1024,
        )
        .unwrap();
        rejoined.bind_replicated_fencing(
            replicas,
            members.iter().map(|member| (*member).to_owned()).collect(),
            "cp-b",
            replacement.fencing_token,
        );
        let next = envelope(brain, stream, replacement.term, 1);
        rejoined
            .apply(&next, vec![], |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .unwrap();
        assert_eq!(rejoined.biological_state(), &[3]);
        fs::remove_dir_all(root).unwrap();
    }
}
