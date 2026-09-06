//! Versioned topology, conservative zero-delay components and ownership maps.

use crate::deterministic::{
    ComponentId, LogicalTag, NeuronId, PartitionGeneration, PrimitiveError, RouteId, ShardId,
    StateDigest, StateDigestBuilder, SynapseId, TopologyGeneration,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeuronRecord {
    pub id: NeuronId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynapseRecord {
    pub id: SynapseId,
    pub source: NeuronId,
    pub target: NeuronId,
    pub delay_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TopologyError {
    #[error("duplicate topology identity {0}")]
    DuplicateIdentity(u64),
    #[error("synapse {synapse} references a missing neuron")]
    MissingNeuron { synapse: SynapseId },
    #[error("topology proposal belongs to generation {actual}, expected {expected}")]
    StaleGeneration {
        expected: TopologyGeneration,
        actual: TopologyGeneration,
    },
    #[error("topology proposal is not at a logical boundary")]
    InvalidBoundary,
    #[error("zero-capacity shard {0}")]
    ZeroCapacityShard(ShardId),
    #[error("at least one shard is required")]
    NoShards,
    #[error("shard {0} appears more than once in the capacity inventory")]
    DuplicateShard(ShardId),
    #[error("ownership map repeats synapse {0}")]
    DuplicateOwnership(SynapseId),
    #[error("synapse {0} has no ownership record")]
    MissingOwnership(SynapseId),
    #[error("shard {shard} planned load {load} exceeds capacity {capacity}")]
    CapacityExceeded {
        shard: ShardId,
        load: u64,
        capacity: u64,
    },
    #[error("topology planner counter or load overflow")]
    NumericOverflow,
    #[error("ownership record refers to synapse {0}, which is not in the topology")]
    UnexpectedOwnership(SynapseId),
    #[error("component {0} is assigned to more than one shard")]
    DuplicateComponentAssignment(ComponentId),
    #[error("component {0} has no shard assignment")]
    MissingComponentAssignment(ComponentId),
    #[error("shard {0} is not present in the partition plan")]
    UnknownShard(ShardId),
    #[error("route {route} does not match its endpoint components")]
    RouteEndpointMismatch { route: RouteId },
    #[error("route {0} is missing from the compiled route table")]
    MissingRoute(RouteId),
    #[error(transparent)]
    Primitive(#[from] PrimitiveError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionPlanError {
    #[error("execution plan is not valid: {0}")]
    Invalid(#[from] TopologyError),
    #[error("event belongs to topology generation {actual}, expected {expected}")]
    StaleTopologyGeneration {
        expected: TopologyGeneration,
        actual: TopologyGeneration,
    },
    #[error("event belongs to partition generation {actual}, expected {expected}")]
    StalePartitionGeneration {
        expected: PartitionGeneration,
        actual: PartitionGeneration,
    },
    #[error("component {0} is not owned by the active execution plan")]
    UnknownComponent(ComponentId),
    #[error("event targets component {target}, but this executor owns {local}")]
    EventNotForComponent {
        local: ComponentId,
        target: ComponentId,
    },
    #[error("a local event cannot connect component {from} to component {to} without a route")]
    MissingLocalRoute { from: ComponentId, to: ComponentId },
    #[error("route {route} is not valid for {from} -> {to}")]
    InvalidRoute {
        route: RouteId,
        from: ComponentId,
        to: ComponentId,
    },
    #[error("execution plan activation must occur at microstep zero")]
    InvalidActivationBoundary,
    #[error("execution plan activation tag {effective} is before current tag {current}")]
    ActivationMovedBackwards {
        current: LogicalTag,
        effective: LogicalTag,
    },
    #[error("a pending execution plan is already waiting for activation")]
    PendingActivation,
    #[error("execution plan has no pending activation at {0}")]
    NoPendingActivation(LogicalTag),
    #[error("execution plan generation did not advance")]
    GenerationDidNotAdvance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroDelayComponent {
    pub id: ComponentId,
    pub members: Vec<NeuronId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentEdge {
    pub from: ComponentId,
    pub to: ComponentId,
    pub synapses: Vec<SynapseId>,
    pub positive_delay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentGraph {
    pub components: Vec<ZeroDelayComponent>,
    pub edges: Vec<ComponentEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDefinition {
    pub id: RouteId,
    pub partition_generation: PartitionGeneration,
    pub synapse: SynapseId,
    pub from: ComponentId,
    pub to: ComponentId,
    pub delay_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyGenerationModel {
    pub generation: TopologyGeneration,
    neurons: BTreeMap<NeuronId, NeuronRecord>,
    synapses: BTreeMap<SynapseId, SynapseRecord>,
}

impl TopologyGenerationModel {
    pub fn new(
        generation: TopologyGeneration,
        neurons: Vec<NeuronRecord>,
        synapses: Vec<SynapseRecord>,
    ) -> Result<Self, TopologyError> {
        let mut neuron_map = BTreeMap::new();
        for neuron in neurons {
            let id = neuron.id;
            if neuron_map.insert(id, neuron).is_some() {
                return Err(TopologyError::DuplicateIdentity(id.raw()));
            }
        }
        let mut synapse_map = BTreeMap::new();
        for synapse in synapses {
            if !neuron_map.contains_key(&synapse.source)
                || !neuron_map.contains_key(&synapse.target)
            {
                return Err(TopologyError::MissingNeuron {
                    synapse: synapse.id,
                });
            }
            if synapse_map.insert(synapse.id, synapse.clone()).is_some() {
                return Err(TopologyError::DuplicateIdentity(synapse.id.raw()));
            }
        }
        Ok(Self {
            generation,
            neurons: neuron_map,
            synapses: synapse_map,
        })
    }

    pub fn neurons(&self) -> impl Iterator<Item = &NeuronRecord> {
        self.neurons.values()
    }

    pub fn synapses(&self) -> impl Iterator<Item = &SynapseRecord> {
        self.synapses.values()
    }

    /// Return the canonical identity of the biological topology generation.
    ///
    /// The digest is deliberately independent of dense runner layout and
    /// insertion order.  Bootstrap manifests use it to prove that the
    /// topology used to compile a stable execution plan is the topology that
    /// was authorised for the immutable checkpoint being reopened.
    pub fn digest(&self) -> StateDigest {
        #[derive(Serialize)]
        struct TopologyMaterial<'a> {
            generation: TopologyGeneration,
            neurons: Vec<&'a NeuronRecord>,
            synapses: Vec<&'a SynapseRecord>,
        }

        let bytes = serde_json::to_vec(&TopologyMaterial {
            generation: self.generation,
            neurons: self.neurons.values().collect(),
            synapses: self.synapses.values().collect(),
        })
        .expect("topology contains only serialisable primitives");
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("topology-generation:v1", bytes);
        digest.finish()
    }

    pub fn zero_delay_components(&self) -> ComponentGraph {
        let nodes = self.neurons.keys().copied().collect::<Vec<_>>();
        let mut forward = BTreeMap::<NeuronId, Vec<NeuronId>>::new();
        let mut reverse = BTreeMap::<NeuronId, Vec<NeuronId>>::new();
        for node in &nodes {
            forward.entry(*node).or_default();
            reverse.entry(*node).or_default();
        }
        for synapse in self
            .synapses
            .values()
            .filter(|synapse| synapse.delay_ticks == 0)
        {
            forward
                .entry(synapse.source)
                .or_default()
                .push(synapse.target);
            reverse
                .entry(synapse.target)
                .or_default()
                .push(synapse.source);
        }
        for values in forward.values_mut() {
            values.sort_unstable();
        }
        for values in reverse.values_mut() {
            values.sort_unstable();
        }

        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        fn visit(
            node: NeuronId,
            graph: &BTreeMap<NeuronId, Vec<NeuronId>>,
            visited: &mut BTreeSet<NeuronId>,
            order: &mut Vec<NeuronId>,
        ) {
            if !visited.insert(node) {
                return;
            }
            if let Some(next) = graph.get(&node) {
                for target in next {
                    visit(*target, graph, visited, order);
                }
            }
            order.push(node);
        }
        for node in &nodes {
            visit(*node, &forward, &mut visited, &mut order);
        }

        let mut groups = Vec::<Vec<NeuronId>>::new();
        visited.clear();
        fn collect(
            node: NeuronId,
            graph: &BTreeMap<NeuronId, Vec<NeuronId>>,
            visited: &mut BTreeSet<NeuronId>,
            group: &mut Vec<NeuronId>,
        ) {
            if !visited.insert(node) {
                return;
            }
            group.push(node);
            if let Some(next) = graph.get(&node) {
                for target in next {
                    collect(*target, graph, visited, group);
                }
            }
        }
        while let Some(node) = order.pop() {
            if visited.contains(&node) {
                continue;
            }
            let mut group = Vec::new();
            collect(node, &reverse, &mut visited, &mut group);
            group.sort_unstable();
            groups.push(group);
        }
        groups.sort_by_key(|group| group[0]);
        let mut component_for = BTreeMap::new();
        let components = groups
            .into_iter()
            .enumerate()
            .map(|(index, members)| {
                let id = ComponentId::new(index as u64 + 1).expect("component index is non-zero");
                for member in &members {
                    component_for.insert(*member, id);
                }
                ZeroDelayComponent { id, members }
            })
            .collect::<Vec<_>>();

        let mut edges = BTreeMap::<(ComponentId, ComponentId), ComponentEdge>::new();
        for synapse in self.synapses.values() {
            let from = component_for[&synapse.source];
            let to = component_for[&synapse.target];
            if from == to && synapse.delay_ticks == 0 {
                continue;
            }
            let edge = edges.entry((from, to)).or_insert_with(|| ComponentEdge {
                from,
                to,
                synapses: Vec::new(),
                positive_delay: false,
            });
            edge.synapses.push(synapse.id);
            edge.positive_delay |= synapse.delay_ticks > 0;
        }
        let mut edges = edges.into_values().collect::<Vec<_>>();
        for edge in &mut edges {
            edge.synapses.sort_unstable();
        }
        ComponentGraph { components, edges }
    }

    pub fn apply_proposal(&self, proposal: TopologyProposal) -> Result<Self, TopologyError> {
        if proposal.base_generation != self.generation {
            return Err(TopologyError::StaleGeneration {
                expected: self.generation,
                actual: proposal.base_generation,
            });
        }
        if proposal.effective_tag.microstep != 0 {
            return Err(TopologyError::InvalidBoundary);
        }
        let mut neurons = self.neurons.values().cloned().collect::<Vec<_>>();
        neurons.extend(proposal.add_neurons);
        let mut synapses = self.synapses.values().cloned().collect::<Vec<_>>();
        synapses.extend(proposal.add_synapses);
        Self::new(
            TopologyGeneration::new(self.generation.raw().checked_add(1).ok_or(
                PrimitiveError::LogicalTimeOverflow {
                    operation: "advancing topology generation",
                },
            )?)?,
            neurons,
            synapses,
        )
    }

    /// Compile only cross-component edges into stable route identities.
    pub fn compile_routes(
        &self,
        partition_generation: PartitionGeneration,
    ) -> Vec<RouteDefinition> {
        let graph = self.zero_delay_components();
        let mut component_for = BTreeMap::new();
        for component in &graph.components {
            for neuron in &component.members {
                component_for.insert(*neuron, component.id);
            }
        }
        let mut routes = self
            .synapses
            .values()
            .filter_map(|synapse| {
                let from = component_for[&synapse.source];
                let to = component_for[&synapse.target];
                (from != to).then_some((synapse.id, from, to, synapse.delay_ticks))
            })
            .collect::<Vec<_>>();
        routes.sort_by_key(|route| route.0);
        routes
            .into_iter()
            .map(|(synapse, from, to, delay_ticks)| RouteDefinition {
                // A route is the transport identity of a synapse.  It must
                // not be renumbered merely because a partition generation
                // was recompiled.
                id: RouteId::new(synapse.raw()).expect("synapse identity is non-zero"),
                partition_generation,
                synapse,
                from,
                to,
                delay_ticks,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyProposal {
    pub base_generation: TopologyGeneration,
    pub effective_tag: LogicalTag,
    pub add_neurons: Vec<NeuronRecord>,
    pub add_synapses: Vec<SynapseRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardCapacity {
    pub shard: ShardId,
    pub capacity: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualShardAssignment {
    pub shard: ShardId,
    pub components: Vec<ComponentId>,
    pub load: u64,
}

/// The immutable, generation-fenced plan consumed by the reference executor.
///
/// The plan deliberately contains both biological identities and virtual
/// execution identities.  Dense array indices remain an implementation detail
/// of a runner and cannot be used as an ownership or route key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledExecutionPlan {
    topology_generation: TopologyGeneration,
    topology_digest: StateDigest,
    partition_generation: PartitionGeneration,
    assignments: Vec<VirtualShardAssignment>,
    component_owners: BTreeMap<ComponentId, ShardId>,
    neuron_components: BTreeMap<NeuronId, ComponentId>,
    neuron_owners: BTreeMap<NeuronId, ShardId>,
    ownership: BTreeMap<SynapseId, OwnershipRecord>,
    routes: BTreeMap<RouteId, RouteDefinition>,
}

impl CompiledExecutionPlan {
    pub fn topology_generation(&self) -> TopologyGeneration {
        self.topology_generation
    }

    pub fn partition_generation(&self) -> PartitionGeneration {
        self.partition_generation
    }

    pub fn topology_digest(&self) -> StateDigest {
        self.topology_digest
    }

    pub fn assignments(&self) -> &[VirtualShardAssignment] {
        &self.assignments
    }

    pub fn component_owner(&self, component: ComponentId) -> Option<ShardId> {
        self.component_owners.get(&component).copied()
    }

    pub fn neuron_owner(&self, neuron: NeuronId) -> Option<ShardId> {
        self.neuron_owners.get(&neuron).copied()
    }

    /// Return the zero-delay component containing a stable neuron identity.
    /// Component membership is part of the compiled plan, so callers do not
    /// need to reconstruct SCC state from dense runner indices.
    pub fn component_for_neuron(&self, neuron: NeuronId) -> Option<ComponentId> {
        self.neuron_components.get(&neuron).copied()
    }

    pub fn ownership(&self, synapse: SynapseId) -> Option<&OwnershipRecord> {
        self.ownership.get(&synapse)
    }

    pub fn ownership_records(&self) -> impl Iterator<Item = &OwnershipRecord> {
        self.ownership.values()
    }

    pub fn route(&self, route: RouteId) -> Option<&RouteDefinition> {
        self.routes.get(&route)
    }

    pub fn route_for_synapse(&self, synapse: SynapseId) -> Option<&RouteDefinition> {
        self.routes.values().find(|route| route.synapse == synapse)
    }

    pub fn shard_ids(&self) -> impl Iterator<Item = ShardId> + '_ {
        self.assignments.iter().map(|assignment| assignment.shard)
    }

    /// Return the canonical identity of the topology/partition ownership
    /// decision. This digest intentionally excludes physical node placement;
    /// moving a stable virtual shard must not change its biological plan.
    pub fn digest(&self) -> StateDigest {
        #[derive(Serialize)]
        struct PlanMaterial<'a> {
            topology_generation: TopologyGeneration,
            topology_digest: StateDigest,
            partition_generation: PartitionGeneration,
            assignments: &'a [VirtualShardAssignment],
            component_owners: &'a BTreeMap<ComponentId, ShardId>,
            neuron_components: &'a BTreeMap<NeuronId, ComponentId>,
            neuron_owners: &'a BTreeMap<NeuronId, ShardId>,
            ownership: &'a BTreeMap<SynapseId, OwnershipRecord>,
            routes: &'a BTreeMap<RouteId, RouteDefinition>,
        }
        let mut assignments = self.assignments.clone();
        for assignment in &mut assignments {
            assignment.components.sort_unstable();
        }
        assignments.sort_by_key(|assignment| assignment.shard);
        let bytes = serde_json::to_vec(&PlanMaterial {
            topology_generation: self.topology_generation,
            topology_digest: self.topology_digest,
            partition_generation: self.partition_generation,
            assignments: &assignments,
            component_owners: &self.component_owners,
            neuron_components: &self.neuron_components,
            neuron_owners: &self.neuron_owners,
            ownership: &self.ownership,
            routes: &self.routes,
        })
        .expect("compiled execution plan contains only serialisable primitives");
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("compiled-execution-plan:v1", bytes);
        digest.finish()
    }

    /// Validate an event admission against both generations and its route.
    /// `route == None` is reserved for work local to one component.
    pub fn validate_event(
        &self,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        from: ComponentId,
        to: ComponentId,
        route: Option<RouteId>,
    ) -> Result<(), ExecutionPlanError> {
        if topology_generation != self.topology_generation {
            return Err(ExecutionPlanError::StaleTopologyGeneration {
                expected: self.topology_generation,
                actual: topology_generation,
            });
        }
        if partition_generation != self.partition_generation {
            return Err(ExecutionPlanError::StalePartitionGeneration {
                expected: self.partition_generation,
                actual: partition_generation,
            });
        }
        if self.component_owner(from).is_none() {
            return Err(ExecutionPlanError::UnknownComponent(from));
        }
        if self.component_owner(to).is_none() {
            return Err(ExecutionPlanError::UnknownComponent(to));
        }
        match route {
            None if from == to => Ok(()),
            None => Err(ExecutionPlanError::MissingLocalRoute { from, to }),
            Some(route_id) => {
                let Some(route) = self.route(route_id) else {
                    return Err(ExecutionPlanError::InvalidRoute {
                        route: route_id,
                        from,
                        to,
                    });
                };
                if route.from != from
                    || route.to != to
                    || route.partition_generation != partition_generation
                {
                    return Err(ExecutionPlanError::InvalidRoute {
                        route: route_id,
                        from,
                        to,
                    });
                }
                Ok(())
            }
        }
    }
}

/// Compile and validate one complete topology/partition ownership plan.
pub fn compile_execution_plan(
    topology: &TopologyGenerationModel,
    partition_generation: PartitionGeneration,
    assignments: Vec<VirtualShardAssignment>,
    ownership: Vec<OwnershipRecord>,
) -> Result<CompiledExecutionPlan, TopologyError> {
    let graph = topology.zero_delay_components();
    let mut shard_ids = BTreeSet::new();
    let mut component_owners = BTreeMap::new();
    for assignment in &assignments {
        if !shard_ids.insert(assignment.shard) {
            return Err(TopologyError::DuplicateShard(assignment.shard));
        }
        for component in &assignment.components {
            if !graph
                .components
                .iter()
                .any(|candidate| candidate.id == *component)
            {
                return Err(TopologyError::MissingComponentAssignment(*component));
            }
            if component_owners
                .insert(*component, assignment.shard)
                .is_some()
            {
                return Err(TopologyError::DuplicateComponentAssignment(*component));
            }
        }
    }
    for component in &graph.components {
        if !component_owners.contains_key(&component.id) {
            return Err(TopologyError::MissingComponentAssignment(component.id));
        }
    }

    let mut neuron_components = BTreeMap::new();
    let mut neuron_owners = BTreeMap::new();
    for component in &graph.components {
        let shard = component_owners[&component.id];
        for neuron in &component.members {
            neuron_components.insert(*neuron, component.id);
            neuron_owners.insert(*neuron, shard);
        }
    }

    validate_complete_ownership(topology.synapses(), &ownership)?;
    let mut ownership_map = BTreeMap::new();
    for record in ownership {
        for owner in [
            record.terminal_owner,
            record.weight_owner,
            record.release_owner,
            record.plasticity_owner,
        ] {
            if !shard_ids.contains(&owner) {
                return Err(TopologyError::UnknownShard(owner));
            }
        }
        ownership_map.insert(record.synapse, record);
    }

    let routes = topology
        .compile_routes(partition_generation)
        .into_iter()
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    for route in routes.values() {
        if route.partition_generation != partition_generation
            || !component_owners.contains_key(&route.from)
            || !component_owners.contains_key(&route.to)
        {
            return Err(TopologyError::RouteEndpointMismatch { route: route.id });
        }
    }

    Ok(CompiledExecutionPlan {
        topology_generation: topology.generation,
        topology_digest: topology.digest(),
        partition_generation,
        assignments,
        component_owners,
        neuron_components,
        neuron_owners,
        ownership: ownership_map,
        routes,
    })
}

/// A small atomic activation state machine for plans.  A pending plan is not
/// visible to event admission until its declared logical boundary.
#[derive(Debug, Clone)]
pub struct ExecutionPlanRegistry {
    active: CompiledExecutionPlan,
    pending: Option<(LogicalTag, CompiledExecutionPlan)>,
}

impl ExecutionPlanRegistry {
    pub fn new(active: CompiledExecutionPlan) -> Self {
        Self {
            active,
            pending: None,
        }
    }

    pub fn active(&self) -> &CompiledExecutionPlan {
        &self.active
    }

    pub fn propose(
        &mut self,
        effective_tag: LogicalTag,
        current_tag: LogicalTag,
        plan: CompiledExecutionPlan,
    ) -> Result<(), ExecutionPlanError> {
        if effective_tag.microstep != 0 {
            return Err(ExecutionPlanError::InvalidActivationBoundary);
        }
        if effective_tag < current_tag {
            return Err(ExecutionPlanError::ActivationMovedBackwards {
                current: current_tag,
                effective: effective_tag,
            });
        }
        if plan.topology_generation < self.active.topology_generation
            || plan.partition_generation < self.active.partition_generation
        {
            return Err(ExecutionPlanError::GenerationDidNotAdvance);
        }
        if plan.topology_generation == self.active.topology_generation
            && plan.partition_generation == self.active.partition_generation
        {
            return Err(ExecutionPlanError::GenerationDidNotAdvance);
        }
        if self.pending.is_some() {
            return Err(ExecutionPlanError::PendingActivation);
        }
        self.pending = Some((effective_tag, plan));
        Ok(())
    }

    pub fn activate_at(&mut self, tag: LogicalTag) -> Result<bool, ExecutionPlanError> {
        if tag.microstep != 0 {
            return Err(ExecutionPlanError::InvalidActivationBoundary);
        }
        let Some((effective_tag, _)) = self.pending.as_ref() else {
            return Ok(false);
        };
        if *effective_tag > tag {
            return Ok(false);
        }
        let (_, plan) = self.pending.take().expect("pending plan was checked");
        self.active = plan;
        Ok(true)
    }

    pub fn validate_event(
        &self,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        from: ComponentId,
        to: ComponentId,
        route: Option<RouteId>,
    ) -> Result<(), ExecutionPlanError> {
        self.active
            .validate_event(topology_generation, partition_generation, from, to, route)
    }
}

/// Deterministic weighted greedy planner. It is a placement plan only; it does
/// not claim to alter biological ownership.
pub fn plan_virtual_shards(
    graph: &ComponentGraph,
    weights: &BTreeMap<ComponentId, u64>,
    mut capacities: Vec<ShardCapacity>,
) -> Result<Vec<VirtualShardAssignment>, TopologyError> {
    capacities.sort_by_key(|capacity| capacity.shard);
    if capacities.is_empty() {
        return Err(TopologyError::NoShards);
    }
    for capacity in &capacities {
        if capacity.capacity == 0 {
            return Err(TopologyError::ZeroCapacityShard(capacity.shard));
        }
    }
    for pair in capacities.windows(2) {
        if pair[0].shard == pair[1].shard {
            return Err(TopologyError::DuplicateShard(pair[0].shard));
        }
    }
    let mut assignments = capacities
        .iter()
        .map(|capacity| VirtualShardAssignment {
            shard: capacity.shard,
            components: Vec::new(),
            load: 0,
        })
        .collect::<Vec<_>>();
    let mut component_order = graph
        .components
        .iter()
        .map(|component| component.id)
        .collect::<Vec<_>>();
    component_order.sort_by(|left, right| {
        weights
            .get(right)
            .unwrap_or(&1)
            .cmp(weights.get(left).unwrap_or(&1))
            .then_with(|| left.cmp(right))
    });
    for component in component_order {
        let weight = *weights.get(&component).unwrap_or(&1);
        let chosen = (0..assignments.len())
            .min_by(|left, right| {
                let left_capacity = capacities[*left].capacity as u128;
                let right_capacity = capacities[*right].capacity as u128;
                let left_load = assignments[*left].load as u128;
                let right_load = assignments[*right].load as u128;
                (left_load * right_capacity)
                    .cmp(&(right_load * left_capacity))
                    .then_with(|| assignments[*left].shard.cmp(&assignments[*right].shard))
            })
            .expect("at least one shard is required");
        assignments[chosen].components.push(component);
        assignments[chosen].load = assignments[chosen]
            .load
            .checked_add(weight)
            .ok_or(TopologyError::NumericOverflow)?;
        if assignments[chosen].load > capacities[chosen].capacity {
            return Err(TopologyError::CapacityExceeded {
                shard: assignments[chosen].shard,
                load: assignments[chosen].load,
                capacity: capacities[chosen].capacity,
            });
        }
    }
    Ok(assignments)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipRecord {
    pub synapse: SynapseId,
    pub terminal_owner: ShardId,
    pub weight_owner: ShardId,
    pub release_owner: ShardId,
    pub plasticity_owner: ShardId,
}

pub fn validate_ownership(records: &[OwnershipRecord]) -> Result<(), TopologyError> {
    let mut seen = BTreeSet::new();
    for record in records {
        if !seen.insert(record.synapse) {
            return Err(TopologyError::DuplicateOwnership(record.synapse));
        }
        if record.terminal_owner.raw() == 0
            || record.weight_owner.raw() == 0
            || record.release_owner.raw() == 0
            || record.plasticity_owner.raw() == 0
        {
            return Err(TopologyError::Primitive(PrimitiveError::ZeroId));
        }
    }
    Ok(())
}

pub fn validate_complete_ownership<'a, I>(
    synapses: I,
    records: &[OwnershipRecord],
) -> Result<(), TopologyError>
where
    I: IntoIterator<Item = &'a SynapseRecord>,
{
    validate_ownership(records)?;
    let expected = synapses
        .into_iter()
        .map(|synapse| synapse.id)
        .collect::<BTreeSet<_>>();
    let actual = records
        .iter()
        .map(|record| record.synapse)
        .collect::<BTreeSet<_>>();
    if let Some(synapse) = actual.difference(&expected).next().copied() {
        return Err(TopologyError::UnexpectedOwnership(synapse));
    }
    expected
        .difference(&actual)
        .next()
        .copied()
        .map_or(Ok(()), |synapse| {
            Err(TopologyError::MissingOwnership(synapse))
        })
}
