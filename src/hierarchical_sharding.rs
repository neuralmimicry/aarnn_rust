//! Communication-aware hierarchical sharding.
//!
//! This is the active deterministic placement planner used by the distributed
//! compatibility executor and the stable-shard adapter. It keeps biological
//! area/layer ownership separate from physical hosts while returning a
//! concrete, executable host choice for every sub-shard. A plan is
//! deterministic for the same topology, workload, measured latency matrix and
//! resource inventory.

use crate::deterministic::{
    LogicalTag, NeuronId, PartitionGeneration, PrimitiveError, ShardId, StateDigest,
    StateDigestBuilder, SubShardId, SynapseId, TopologyGeneration,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const HIERARCHICAL_SHARDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeuralUnitKind {
    BiologicalNeuron,
    AreaLayerAggregate,
}

impl Default for NeuralUnitKind {
    fn default() -> Self {
        Self::BiologicalNeuron
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeuralUnit {
    pub network_id: String,
    pub area_id: u64,
    /// Stable human readable identity for the physical area. The numeric ID
    /// is used in wire identities; this label makes the placement explainable
    /// and is derived from persisted topology evidence or a deterministic
    /// topology fallback.
    #[serde(default)]
    pub area_label: String,
    pub layer: u32,
    pub neuron_id: NeuronId,
    /// Aggregate planner units are useful for the compatibility layer but
    /// cannot be promoted into the biological ownership graph.
    #[serde(default)]
    pub kind: NeuralUnitKind,
    pub work_units: u64,
    pub state_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeuralNetworkInput {
    pub network_id: String,
    /// The node from which this network normally enters the fabric. It is a
    /// placement hint only; it never grants authority or admission.
    pub home_node: String,
    pub units: Vec<NeuralUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInteraction {
    pub source_network: String,
    pub target_network: String,
    pub events_per_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCapacity {
    pub node_id: String,
    pub capacity_units: u64,
    pub memory_bytes: u64,
}

/// Measured one-way or round-trip values must use one declared convention at
/// the adapter boundary. The planner only compares the integer values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyMatrix {
    pub nodes: Vec<String>,
    pub latency_us: BTreeMap<(String, String), u64>,
}

impl LatencyMatrix {
    pub fn new(nodes: Vec<String>, measurements: Vec<(String, String, u64)>) -> Self {
        let mut latency_us = BTreeMap::new();
        for node in &nodes {
            latency_us.insert((node.clone(), node.clone()), 0);
        }
        for (from, to, latency) in measurements {
            latency_us.insert((from, to), latency);
        }
        Self { nodes, latency_us }
    }

    pub(crate) fn latency(&self, from: &str, to: &str) -> Option<u64> {
        self.latency_us
            .get(&(from.to_owned(), to.to_owned()))
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchicalShardingRequest {
    pub networks: Vec<NeuralNetworkInput>,
    pub interactions: Vec<NetworkInteraction>,
    pub hosts: Vec<HostCapacity>,
    pub latency: LatencyMatrix,
    /// A layer is split into bounded sub-shards when its measured work exceeds
    /// this target. A host capacity shortage can split it further.
    pub target_sub_shard_work_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkGroupPlacement {
    pub group_id: String,
    pub network_ids: Vec<String>,
    pub communication_weight: u64,
    pub anchor_node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubShardPlacement {
    pub sub_shard_id: SubShardId,
    pub parent_shard_id: ShardId,
    pub layer: u32,
    pub neuron_ids: Vec<NeuronId>,
    /// Parallel to `neuron_ids`; old plans without this additive field are
    /// interpreted as biological IDs for compatibility.
    #[serde(default)]
    pub neuron_kinds: Vec<NeuralUnitKind>,
    pub work_units: u64,
    pub state_bytes: u64,
    pub active_node: String,
    pub latency_to_group_anchor_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerPlacement {
    pub layer: u32,
    pub sub_shards: Vec<SubShardPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaShardPlacement {
    pub shard_id: ShardId,
    pub network_id: String,
    pub group_id: String,
    pub area_id: u64,
    pub area_label: String,
    pub layers: Vec<LayerPlacement>,
    pub total_work_units: u64,
    pub total_state_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchicalShardPlan {
    pub schema_version: u32,
    pub groups: Vec<NetworkGroupPlacement>,
    pub shards: Vec<AreaShardPlacement>,
    pub digest: StateDigest,
}

/// The biological owner of one stable neuron in the active topology.
///
/// This is deliberately separate from physical placement telemetry.  An
/// active neuron has exactly one record here; a warm copy is represented by
/// durability metadata and is never added as a second biological owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BiologicalNeuronPlacement {
    pub neuron_id: NeuronId,
    pub area_id: u64,
    pub area_label: String,
    pub layer: u32,
    pub sub_shard_id: SubShardId,
    pub active_node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BiologicalNeuronTransfer {
    pub neuron_id: NeuronId,
    /// `None` is used for a newly grown neuron that is admitted in this
    /// transaction.  Existing neurons must name their current active owner.
    pub source: Option<BiologicalNeuronPlacement>,
    /// A new neuron must identify the stable parent whose area admitted its
    /// growth.  Existing-neuron migration may leave this unset.
    pub parent_neuron: Option<NeuronId>,
    /// When a newly grown neuron has already crossed an area boundary before
    /// admission, this records the pre-crossing owner so the boundary is
    /// still represented as one explicit migration in the commit.
    pub origin: Option<BiologicalNeuronPlacement>,
    pub destination: BiologicalNeuronPlacement,
    /// Stable state evidence for the transfer.  The transaction carries the
    /// evidence boundary; the runner/durable owner carries the actual state.
    pub state_digest: StateDigest,
    pub synapse_ids: Vec<SynapseId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiologicalTopologyTransaction {
    pub base_topology_generation: TopologyGeneration,
    pub base_partition_generation: PartitionGeneration,
    pub effective_tag: LogicalTag,
    pub transfers: Vec<BiologicalNeuronTransfer>,
    /// Optional immutable spatial result committed with the ownership change.
    /// It is validated against `base_growth_space_digest` by
    /// `apply_transaction_with_space`.
    #[serde(default)]
    pub base_growth_space_digest: Option<StateDigest>,
    #[serde(default)]
    pub growth_space: Option<BiologicalGrowthSpace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiologicalOwnershipMap {
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    owners: BTreeMap<NeuronId, BiologicalNeuronPlacement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiologicalOwnershipCommit {
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub effective_tag: LogicalTag,
    pub ownership: BiologicalOwnershipMap,
    pub transfers: Vec<BiologicalNeuronTransfer>,
    pub growth_space: Option<BiologicalGrowthSpace>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BiologicalOwnershipError {
    #[error("topology transaction is based on topology generation {actual}, expected {expected}")]
    StaleTopologyGeneration {
        expected: TopologyGeneration,
        actual: TopologyGeneration,
    },
    #[error("topology transaction is based on partition generation {actual}, expected {expected}")]
    StalePartitionGeneration {
        expected: PartitionGeneration,
        actual: PartitionGeneration,
    },
    #[error("topology transaction must take effect at microstep zero")]
    InvalidBoundary,
    #[error("neuron {0} is not present in the active ownership map")]
    UnknownNeuron(NeuronId),
    #[error("neuron {0} is already present in the active ownership map")]
    DuplicateNeuron(NeuronId),
    #[error("neuron {neuron} source owner does not match the active owner")]
    SourceOwnerMismatch { neuron: NeuronId },
    #[error("neuron {0} is admitted more than once in one transaction")]
    DuplicateTransactionNeuron(NeuronId),
    #[error("destination owner for neuron {neuron} has an empty node")]
    EmptyDestinationNode { neuron: NeuronId },
    #[error("neuron {neuron} has no parent for a new growth admission")]
    MissingGrowthParent { neuron: NeuronId },
    #[error("new neuron {neuron} crosses area {from} -> {to} without an explicit migration origin")]
    UnauthorisedGrowthBoundary {
        neuron: NeuronId,
        from: u64,
        to: u64,
    },
    #[error("generation overflow while publishing biological ownership")]
    GenerationOverflow,
    #[error("a growth-space update requires the current biological growth space")]
    GrowthSpaceContextMissing,
    #[error("growth-space digest does not match the current biological growth space")]
    GrowthSpaceDigestMismatch,
    #[error("planner unit {0} is an area/layer aggregate, not a biological neuron")]
    AggregateUnit(NeuronId),
    #[error(transparent)]
    GrowthSpace(#[from] BiologicalSpaceError),
    #[error(transparent)]
    Primitive(#[from] PrimitiveError),
}

/// A deterministic ellipsoid used for the occupied volume of an area or the
/// enclosing biological membrane.  The radii are kept per axis so expansion
/// preserves the broad biological shape instead of turning every growth event
/// into an isotropic bounding sphere.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BiologicalEllipsoid {
    pub centre: [f32; 3],
    pub radii: [f32; 3],
}

impl BiologicalEllipsoid {
    pub fn new(centre: [f32; 3], radii: [f32; 3]) -> Result<Self, BiologicalSpaceError> {
        let volume = Self { centre, radii };
        volume.validate()?;
        Ok(volume)
    }

    fn validate(&self) -> Result<(), BiologicalSpaceError> {
        if self
            .centre
            .iter()
            .chain(self.radii.iter())
            .any(|value| !value.is_finite())
            || self.radii.iter().any(|value| *value <= 0.0)
        {
            return Err(BiologicalSpaceError::NonFiniteOrNonPositiveGeometry);
        }
        Ok(())
    }

    pub fn contains_with_margin(&self, point: [f32; 3], margin: f32) -> bool {
        let margin = margin.max(0.0);
        let mut metric = 0.0f32;
        for axis in 0..3 {
            let radius = self.radii[axis] + margin;
            let delta = point[axis] - self.centre[axis];
            metric += (delta * delta) / (radius * radius);
        }
        metric <= 1.0
    }

    fn contains_ellipsoid(&self, inner: &Self) -> bool {
        (0..8).all(|mask| {
            let point = std::array::from_fn(|axis| {
                let sign = if (mask & (1 << axis)) == 0 { -1.0 } else { 1.0 };
                inner.centre[axis] + sign * inner.radii[axis]
            });
            self.contains_with_margin(point, 0.0)
        })
    }

    fn expanded_to_include(
        &self,
        point: [f32; 3],
        margin: f32,
        max_step: f32,
        max_radii: [f32; 3],
    ) -> Result<Option<Self>, BiologicalSpaceError> {
        let mut radii = self.radii;
        for axis in 0..3 {
            let required = (point[axis] - self.centre[axis]).abs() + margin.max(0.0);
            let target = radii[axis].max(required);
            if target > max_radii[axis] + f32::EPSILON {
                return Ok(None);
            }
            if target > radii[axis] + max_step.max(0.0) + f32::EPSILON {
                return Ok(None);
            }
            radii[axis] = target;
        }
        Ok(Some(Self {
            centre: self.centre,
            radii,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiologicalAreaVolume {
    pub area_id: u64,
    pub volume: BiologicalEllipsoid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiologicalMembrane {
    pub volume: BiologicalEllipsoid,
    pub maximum_radii: [f32; 3],
    pub maximum_expansion_per_transaction: f32,
    pub pressure_threshold: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiologicalGrowthSpace {
    pub membrane: BiologicalMembrane,
    pub areas: Vec<BiologicalAreaVolume>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BiologicalGrowthDecision {
    WithinArea {
        area_id: u64,
    },
    ExpandArea {
        area_id: u64,
        volume: BiologicalAreaVolume,
    },
    MigrateToArea {
        from_area_id: u64,
        to_area_id: u64,
    },
    /// A newly grown neuron is outside all occupied areas. It is admitted to
    /// the nearest existing area and its physical position is moved inside
    /// that area's volume, so growth cannot silently invent a new topology.
    AttachToNearestArea {
        area_id: u64,
        point: [f32; 3],
    },
    AwaitingMembranePressure {
        observed: u32,
        required: u32,
    },
    ExpandMembrane {
        membrane: BiologicalMembrane,
    },
    ConstrainedByMembrane,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BiologicalSpaceError {
    #[error("biological geometry contains a non-finite or non-positive value")]
    NonFiniteOrNonPositiveGeometry,
    #[error("area {0} is not present in the biological growth space")]
    UnknownArea(u64),
    #[error("biological area identity is repeated")]
    DuplicateArea,
    #[error("growth position overlaps more than one destination area")]
    AmbiguousAreaOverlap,
    #[error("area {0} is outside the enclosing biological membrane")]
    AreaOutsideMembrane(u64),
}

impl BiologicalGrowthSpace {
    pub fn new(
        membrane: BiologicalMembrane,
        areas: Vec<BiologicalAreaVolume>,
    ) -> Result<Self, BiologicalSpaceError> {
        membrane.volume.validate()?;
        if membrane
            .maximum_radii
            .iter()
            .zip(membrane.volume.radii.iter())
            .any(|(maximum, current)| !maximum.is_finite() || *maximum < *current)
            || !membrane.maximum_expansion_per_transaction.is_finite()
            || membrane.maximum_expansion_per_transaction < 0.0
        {
            return Err(BiologicalSpaceError::NonFiniteOrNonPositiveGeometry);
        }
        let mut ids = BTreeSet::new();
        for area in &areas {
            area.volume.validate()?;
            if !membrane.volume.contains_ellipsoid(&area.volume) {
                return Err(BiologicalSpaceError::AreaOutsideMembrane(area.area_id));
            }
            if !ids.insert(area.area_id) {
                return Err(BiologicalSpaceError::DuplicateArea);
            }
        }
        Ok(Self { membrane, areas })
    }

    pub fn digest(&self) -> StateDigest {
        let bytes = serde_json::to_vec(self).expect("biological growth space is serialisable");
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("biological-growth-space:v1", bytes);
        digest.finish()
    }

    /// Classify a candidate before ownership mutation. Free space expands the
    /// current area's volume. Only occupied volume belonging to another area
    /// produces migration pressure. Positions beyond the membrane are held
    /// until enough independent pressure exists for a bounded membrane step.
    pub fn assess_growth(
        &self,
        source_area_id: u64,
        point: [f32; 3],
        soma_radius: f32,
        pressure_samples: u32,
    ) -> Result<BiologicalGrowthDecision, BiologicalSpaceError> {
        if point.iter().any(|value| !value.is_finite())
            || !soma_radius.is_finite()
            || soma_radius < 0.0
        {
            return Err(BiologicalSpaceError::NonFiniteOrNonPositiveGeometry);
        }
        let source = self
            .areas
            .iter()
            .find(|area| area.area_id == source_area_id)
            .ok_or(BiologicalSpaceError::UnknownArea(source_area_id))?;
        let destination_areas = self
            .areas
            .iter()
            .filter(|area| area.area_id != source_area_id)
            .filter(|area| area.volume.contains_with_margin(point, soma_radius))
            .collect::<Vec<_>>();
        if destination_areas.len() > 1 {
            return Err(BiologicalSpaceError::AmbiguousAreaOverlap);
        }
        if let Some(destination) = destination_areas.first() {
            return Ok(BiologicalGrowthDecision::MigrateToArea {
                from_area_id: source_area_id,
                to_area_id: destination.area_id,
            });
        }
        if source.volume.contains_with_margin(point, soma_radius) {
            return Ok(BiologicalGrowthDecision::WithinArea {
                area_id: source_area_id,
            });
        }

        if self
            .membrane
            .volume
            .contains_with_margin(point, soma_radius)
        {
            let mut nearest = None;
            for area in &self.areas {
                let mut metric = 0.0f32;
                for axis in 0..3 {
                    let radius = area.volume.radii[axis] + soma_radius.max(0.0);
                    let delta = point[axis] - area.volume.centre[axis];
                    metric += (delta * delta) / (radius * radius);
                }
                if nearest
                    .as_ref()
                    .is_none_or(|(_, current): &(u64, f32)| metric < *current)
                {
                    nearest = Some((area.area_id, metric));
                }
            }
            if let Some((nearest_area_id, nearest_metric)) = nearest {
                if nearest_area_id != source_area_id {
                    let area = self
                        .areas
                        .iter()
                        .find(|area| area.area_id == nearest_area_id)
                        .expect("nearest area came from the area list");
                    let scale = if nearest_metric > 0.8 {
                        (0.8 / nearest_metric.sqrt()).min(1.0)
                    } else {
                        1.0
                    };
                    let projected = std::array::from_fn(|axis| {
                        area.volume.centre[axis] + (point[axis] - area.volume.centre[axis]) * scale
                    });
                    return Ok(BiologicalGrowthDecision::AttachToNearestArea {
                        area_id: nearest_area_id,
                        point: projected,
                    });
                }
            }
            let Some(volume) = source.volume.expanded_to_include(
                point,
                soma_radius,
                self.membrane.maximum_expansion_per_transaction,
                source.volume.radii.map(|radius| radius * 2.0),
            )?
            else {
                return Ok(BiologicalGrowthDecision::ConstrainedByMembrane);
            };
            if !self.membrane.volume.contains_ellipsoid(&volume) {
                return Ok(BiologicalGrowthDecision::ConstrainedByMembrane);
            }
            return Ok(BiologicalGrowthDecision::ExpandArea {
                area_id: source_area_id,
                volume: BiologicalAreaVolume {
                    area_id: source_area_id,
                    volume,
                },
            });
        }

        if pressure_samples < self.membrane.pressure_threshold {
            return Ok(BiologicalGrowthDecision::AwaitingMembranePressure {
                observed: pressure_samples,
                required: self.membrane.pressure_threshold,
            });
        }
        let Some(volume) = self.membrane.volume.expanded_to_include(
            point,
            soma_radius,
            self.membrane.maximum_expansion_per_transaction,
            self.membrane.maximum_radii,
        )?
        else {
            return Ok(BiologicalGrowthDecision::ConstrainedByMembrane);
        };
        Ok(BiologicalGrowthDecision::ExpandMembrane {
            membrane: BiologicalMembrane {
                volume,
                ..self.membrane.clone()
            },
        })
    }

    pub fn apply_decision(
        &self,
        decision: &BiologicalGrowthDecision,
    ) -> Result<Self, BiologicalSpaceError> {
        let mut next = self.clone();
        match decision {
            BiologicalGrowthDecision::ExpandArea { area_id, volume } => {
                let area = next
                    .areas
                    .iter_mut()
                    .find(|area| area.area_id == *area_id)
                    .ok_or(BiologicalSpaceError::UnknownArea(*area_id))?;
                *area = volume.clone();
            }
            BiologicalGrowthDecision::ExpandMembrane { membrane } => {
                membrane.volume.validate()?;
                next.membrane = membrane.clone();
            }
            BiologicalGrowthDecision::WithinArea { .. }
            | BiologicalGrowthDecision::MigrateToArea { .. }
            | BiologicalGrowthDecision::AttachToNearestArea { .. }
            | BiologicalGrowthDecision::AwaitingMembranePressure { .. }
            | BiologicalGrowthDecision::ConstrainedByMembrane => {}
        }
        BiologicalGrowthSpace::new(next.membrane, next.areas)
    }
}

impl BiologicalOwnershipMap {
    pub fn new(
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        placements: Vec<BiologicalNeuronPlacement>,
    ) -> Result<Self, BiologicalOwnershipError> {
        let mut owners = BTreeMap::new();
        for placement in placements {
            if placement.active_node.trim().is_empty() {
                return Err(BiologicalOwnershipError::EmptyDestinationNode {
                    neuron: placement.neuron_id,
                });
            }
            let neuron_id = placement.neuron_id;
            if owners.insert(neuron_id, placement).is_some() {
                return Err(BiologicalOwnershipError::DuplicateNeuron(neuron_id));
            }
        }
        Ok(Self {
            topology_generation,
            partition_generation,
            owners,
        })
    }

    pub fn topology_generation(&self) -> TopologyGeneration {
        self.topology_generation
    }

    pub fn partition_generation(&self) -> PartitionGeneration {
        self.partition_generation
    }

    pub fn owner(&self, neuron: NeuronId) -> Option<&BiologicalNeuronPlacement> {
        self.owners.get(&neuron)
    }

    pub fn owners(&self) -> impl Iterator<Item = &BiologicalNeuronPlacement> {
        self.owners.values()
    }

    pub fn neuron_count(&self) -> usize {
        self.owners.len()
    }

    /// Apply all growth and ownership changes as one generation transition.
    /// Validation completes before the returned map is constructed, so a
    /// rejected boundary crossing cannot expose a half-migrated neuron.
    pub fn apply_transaction(
        &self,
        transaction: BiologicalTopologyTransaction,
    ) -> Result<BiologicalOwnershipCommit, BiologicalOwnershipError> {
        self.apply_transaction_with_space(transaction, None)
    }

    /// Apply ownership and, when supplied, the enclosing biological-space
    /// update at the same logical boundary.  The space digest prevents two
    /// workers from independently expanding the membrane from different
    /// pressure observations.
    pub fn apply_transaction_with_space(
        &self,
        transaction: BiologicalTopologyTransaction,
        current_space: Option<&BiologicalGrowthSpace>,
    ) -> Result<BiologicalOwnershipCommit, BiologicalOwnershipError> {
        let next_growth_space = match (transaction.growth_space.as_ref(), current_space) {
            (Some(proposed), Some(current)) => {
                if transaction.base_growth_space_digest != Some(current.digest()) {
                    return Err(BiologicalOwnershipError::GrowthSpaceDigestMismatch);
                }
                Some(BiologicalGrowthSpace::new(
                    proposed.membrane.clone(),
                    proposed.areas.clone(),
                )?)
            }
            (Some(_), None) => return Err(BiologicalOwnershipError::GrowthSpaceContextMissing),
            (None, _) => None,
        };
        if transaction.base_topology_generation != self.topology_generation {
            return Err(BiologicalOwnershipError::StaleTopologyGeneration {
                expected: self.topology_generation,
                actual: transaction.base_topology_generation,
            });
        }
        if transaction.base_partition_generation != self.partition_generation {
            return Err(BiologicalOwnershipError::StalePartitionGeneration {
                expected: self.partition_generation,
                actual: transaction.base_partition_generation,
            });
        }
        if transaction.effective_tag.microstep != 0 {
            return Err(BiologicalOwnershipError::InvalidBoundary);
        }

        let mut next = self.owners.clone();
        let mut seen = BTreeSet::new();
        let mut transfers = transaction.transfers;
        transfers.sort_by_key(|transfer| transfer.neuron_id);

        for transfer in &transfers {
            if !seen.insert(transfer.neuron_id) {
                return Err(BiologicalOwnershipError::DuplicateTransactionNeuron(
                    transfer.neuron_id,
                ));
            }
            if transfer.destination.active_node.trim().is_empty() {
                return Err(BiologicalOwnershipError::EmptyDestinationNode {
                    neuron: transfer.neuron_id,
                });
            }

            match (&transfer.source, next.get(&transfer.neuron_id)) {
                (Some(source), Some(current)) if source == current => {}
                (Some(_), Some(_)) => {
                    return Err(BiologicalOwnershipError::SourceOwnerMismatch {
                        neuron: transfer.neuron_id,
                    });
                }
                (Some(_), None) => {
                    return Err(BiologicalOwnershipError::UnknownNeuron(transfer.neuron_id));
                }
                (None, Some(_)) => {
                    return Err(BiologicalOwnershipError::DuplicateNeuron(
                        transfer.neuron_id,
                    ));
                }
                (None, None) => {
                    let Some(parent) = transfer.parent_neuron else {
                        return Err(BiologicalOwnershipError::MissingGrowthParent {
                            neuron: transfer.neuron_id,
                        });
                    };
                    let parent_owner = self
                        .owner(parent)
                        .ok_or(BiologicalOwnershipError::UnknownNeuron(parent))?;
                    let origin = transfer.origin.as_ref().unwrap_or(parent_owner);
                    if origin != parent_owner {
                        return Err(BiologicalOwnershipError::SourceOwnerMismatch {
                            neuron: transfer.neuron_id,
                        });
                    }
                    if transfer.origin.is_none()
                        && transfer.destination.area_id != parent_owner.area_id
                    {
                        return Err(BiologicalOwnershipError::UnauthorisedGrowthBoundary {
                            neuron: transfer.neuron_id,
                            from: parent_owner.area_id,
                            to: transfer.destination.area_id,
                        });
                    }
                }
            }
            next.insert(transfer.neuron_id, transfer.destination.clone());
        }

        let next_topology_generation = TopologyGeneration::new(
            self.topology_generation
                .raw()
                .checked_add(1)
                .ok_or(BiologicalOwnershipError::GenerationOverflow)?,
        )?;
        let next_partition_generation = PartitionGeneration::new(
            self.partition_generation
                .raw()
                .checked_add(1)
                .ok_or(BiologicalOwnershipError::GenerationOverflow)?,
        )?;
        let ownership = Self {
            topology_generation: next_topology_generation,
            partition_generation: next_partition_generation,
            owners: next,
        };
        Ok(BiologicalOwnershipCommit {
            topology_generation: next_topology_generation,
            partition_generation: next_partition_generation,
            effective_tag: transaction.effective_tag,
            ownership,
            transfers,
            growth_space: next_growth_space,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HierarchicalShardingError {
    #[error("at least one network and host are required")]
    EmptyInput,
    #[error("network identity is empty or duplicated: {0}")]
    InvalidNetwork(String),
    #[error("network {network} has invalid home node {node}")]
    InvalidHome { network: String, node: String },
    #[error("network interaction references unknown network: {0}")]
    UnknownNetwork(String),
    #[error("neural unit {neuron} is duplicated in network {network}")]
    DuplicateNeuron { network: String, neuron: NeuronId },
    #[error("neural unit {neuron} has zero work or state in network {network}")]
    ZeroDemand { network: String, neuron: NeuronId },
    #[error("host identity is empty, duplicated, or has zero capacity: {0}")]
    InvalidHost(String),
    #[error("target sub-shard work must be non-zero")]
    InvalidTarget,
    #[error("missing measured latency from {from} to {to}")]
    MissingLatency { from: String, to: String },
    #[error("sub-shard {sub_shard} cannot fit on any latency-reachable host")]
    NoCapacity { sub_shard: SubShardId },
    #[error("stable shard identity collision for key {0}")]
    IdentityCollision(String),
    #[error("numeric overflow while building hierarchical shard plan")]
    Overflow,
}

#[derive(Default)]
struct DisjointSet {
    parent: BTreeMap<String, String>,
}

impl DisjointSet {
    fn add(&mut self, value: &str) {
        self.parent
            .entry(value.to_owned())
            .or_insert_with(|| value.to_owned());
    }

    fn find(&mut self, value: &str) -> String {
        let parent = self.parent.get(value).cloned().expect("value was added");
        if parent == value {
            parent
        } else {
            let root = self.find(&parent);
            self.parent.insert(value.to_owned(), root.clone());
            root
        }
    }

    fn union(&mut self, left: &str, right: &str) {
        let left_root = self.find(left);
        let right_root = self.find(right);
        if left_root != right_root {
            let (small, large) = if left_root < right_root {
                (left_root, right_root)
            } else {
                (right_root, left_root)
            };
            self.parent.insert(small, large);
        }
    }
}

fn stable_id(material: &str) -> u64 {
    // FNV-1a gives stable IDs without depending on a random process hash.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in material.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash.max(1)
}

/// A persisted physical/topological observation for one biological neuron.
/// `index` is only a deterministic fallback identity; when coordinates or a
/// region are present they are preferred for area assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalNeuronLocation {
    pub layer: u32,
    pub index: u64,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
    pub region_label: Option<String>,
}

/// Return the stable numeric identity used by area shards and their wire
/// projections. This deliberately uses the canonical, deterministic hash
/// above rather than a process-random hash.
pub fn stable_area_id(area_label: &str) -> u64 {
    stable_id(area_label)
}

/// Derive a stable biological identity from an immutable lineage key.  Runner
/// and distributed adapters use this for persisted neuron lineage; dense layer
/// indices remain execution coordinates and are never used as ownership IDs.
pub fn stable_biological_neuron_id(namespace: &str, lineage: &str) -> NeuronId {
    NeuronId::new(stable_id(&format!(
        "{namespace}:biological-neuron:{lineage}"
    )))
    .expect("stable biological neuron ID is non-zero")
}

fn normalized_region_label(raw: &str) -> Option<String> {
    let label = raw.trim().to_ascii_lowercase();
    (!label.is_empty()).then_some(label)
}

fn spatial_area_label(x: f32, y: f32, z: f32) -> String {
    // Eight bins per axis cover the normalised growth volume. Quantising the
    // persisted position keeps nearby newly grown cells in their parent's
    // area while making imports and replay independent of input order.
    fn bin(value: f32) -> i32 {
        if !value.is_finite() {
            return 0;
        }
        (((value.clamp(-1.0, 1.0) + 1.0) * 4.0).floor() as i32).clamp(0, 7)
    }
    format!("spatial:x{}:y{}:z{}", bin(x), bin(y), bin(z))
}

/// Build deterministic area/layer units from persisted physical observations.
///
/// The returned unit is an executable planning slice, not a biological state
/// mutation. Explicit region labels win, then persisted coordinates, then a
/// fixed topology block. The last fallback is intentionally append-stable:
/// adding neurons to a grown network does not relabel earlier blocks.
pub fn area_layer_units(
    network_id: &str,
    layer_counts: &BTreeMap<u32, u64>,
    locations: &[PhysicalNeuronLocation],
    state_bytes_per_neuron: u64,
) -> Vec<NeuralUnit> {
    let mut locations_by_neuron = BTreeMap::<(u32, u64), &PhysicalNeuronLocation>::new();
    for location in locations {
        locations_by_neuron.insert((location.layer, location.index), location);
    }
    let mut aggregates = BTreeMap::<(u64, u32), (String, u64)>::new();
    for (&layer, &count) in layer_counts {
        for index in 0..count {
            let label = locations_by_neuron
                .get(&(layer, index))
                .and_then(|location| {
                    location
                        .region_label
                        .as_deref()
                        .and_then(normalized_region_label)
                        .or_else(|| match (location.x, location.y, location.z) {
                            (Some(x), Some(y), Some(z))
                                if x.is_finite() && y.is_finite() && z.is_finite() =>
                            {
                                Some(spatial_area_label(x, y, z))
                            }
                            _ => None,
                        })
                })
                .unwrap_or_else(|| format!("topology:l{layer}:block{}", index / 32));
            let area_id = stable_area_id(&label);
            let entry = aggregates.entry((area_id, layer)).or_insert((label, 0));
            entry.1 = entry.1.saturating_add(1);
        }
    }

    aggregates
        .into_iter()
        .map(|((area_id, layer), (area_label, count))| NeuralUnit {
            network_id: network_id.to_owned(),
            area_id,
            area_label,
            layer,
            // A planning unit represents the area/layer slice. Its stable ID
            // is independent of the order in which source neurons arrived.
            neuron_id: NeuronId::new(stable_id(&format!(
                "{network_id}:area:{area_id}:layer:{layer}"
            )))
            .expect("stable area/layer ID is non-zero"),
            kind: NeuralUnitKind::AreaLayerAggregate,
            work_units: count.max(1),
            state_bytes: count.max(1).saturating_mul(state_bytes_per_neuron.max(1)),
        })
        .collect()
}

fn communication_key(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_owned(), right.to_owned())
    } else {
        (right.to_owned(), left.to_owned())
    }
}

fn add_u64(left: &mut u64, right: u64) -> Result<(), HierarchicalShardingError> {
    *left = left
        .checked_add(right)
        .ok_or(HierarchicalShardingError::Overflow)?;
    Ok(())
}

pub fn plan_hierarchical_shards(
    mut request: HierarchicalShardingRequest,
) -> Result<HierarchicalShardPlan, HierarchicalShardingError> {
    if request.networks.is_empty() || request.hosts.is_empty() {
        return Err(HierarchicalShardingError::EmptyInput);
    }
    if request.target_sub_shard_work_units == 0 {
        return Err(HierarchicalShardingError::InvalidTarget);
    }

    request
        .networks
        .sort_by(|left, right| left.network_id.cmp(&right.network_id));
    request
        .hosts
        .sort_by(|left, right| left.node_id.cmp(&right.node_id));
    let mut networks = BTreeMap::new();
    for network in request.networks {
        if network.network_id.trim().is_empty() || networks.contains_key(&network.network_id) {
            return Err(HierarchicalShardingError::InvalidNetwork(
                network.network_id,
            ));
        }
        if network.home_node.trim().is_empty() {
            return Err(HierarchicalShardingError::InvalidHome {
                network: network.network_id,
                node: network.home_node,
            });
        }
        if !request
            .latency
            .nodes
            .iter()
            .any(|node| node == &network.home_node)
        {
            return Err(HierarchicalShardingError::InvalidHome {
                network: network.network_id,
                node: network.home_node,
            });
        }
        let mut units = network.units;
        units.sort_by_key(|unit| (unit.area_id, unit.layer, unit.neuron_id));
        let mut neurons = BTreeSet::new();
        for unit in &units {
            if unit.network_id != network.network_id {
                return Err(HierarchicalShardingError::InvalidNetwork(
                    unit.network_id.clone(),
                ));
            }
            if !neurons.insert(unit.neuron_id) {
                return Err(HierarchicalShardingError::DuplicateNeuron {
                    network: network.network_id.clone(),
                    neuron: unit.neuron_id,
                });
            }
            if unit.work_units == 0 || unit.state_bytes == 0 {
                return Err(HierarchicalShardingError::ZeroDemand {
                    network: network.network_id.clone(),
                    neuron: unit.neuron_id,
                });
            }
        }
        networks.insert(network.network_id.clone(), (network.home_node, units));
    }

    let mut hosts = BTreeMap::new();
    for host in request.hosts {
        if host.node_id.trim().is_empty()
            || host.capacity_units == 0
            || host.memory_bytes == 0
            || hosts.insert(host.node_id.clone(), host).is_some()
        {
            return Err(HierarchicalShardingError::InvalidHost(
                "host identity or capacity".to_owned(),
            ));
        }
    }
    for node in hosts.keys() {
        if !request.latency.nodes.contains(node) {
            return Err(HierarchicalShardingError::MissingLatency {
                from: node.clone(),
                to: node.clone(),
            });
        }
    }
    for network in networks.values() {
        if !hosts.contains_key(&network.0) {
            return Err(HierarchicalShardingError::InvalidHome {
                network: "unknown".to_owned(),
                node: network.0.clone(),
            });
        }
    }

    let network_ids = networks.keys().cloned().collect::<BTreeSet<_>>();
    let mut interactions = BTreeMap::<(String, String), u64>::new();
    let mut dsu = DisjointSet::default();
    for network in &network_ids {
        dsu.add(network);
    }
    for interaction in request.interactions {
        if !network_ids.contains(&interaction.source_network) {
            return Err(HierarchicalShardingError::UnknownNetwork(
                interaction.source_network,
            ));
        }
        if !network_ids.contains(&interaction.target_network) {
            return Err(HierarchicalShardingError::UnknownNetwork(
                interaction.target_network,
            ));
        }
        if interaction.source_network != interaction.target_network
            && interaction.events_per_tick > 0
        {
            dsu.union(&interaction.source_network, &interaction.target_network);
        }
        let key = communication_key(&interaction.source_network, &interaction.target_network);
        let value = interactions.entry(key).or_default();
        add_u64(value, interaction.events_per_tick)?;
    }

    let mut group_members = BTreeMap::<String, Vec<String>>::new();
    for network in &network_ids {
        let root = dsu.find(network);
        group_members.entry(root).or_default().push(network.clone());
    }
    for members in group_members.values_mut() {
        members.sort();
    }

    let mut groups = Vec::new();
    let mut group_by_network = BTreeMap::new();
    let mut used_group_ids = BTreeSet::new();
    for members in group_members.values() {
        let group_id = format!("group-{:016x}", stable_id(&members.join("|")));
        if !used_group_ids.insert(group_id.clone()) {
            return Err(HierarchicalShardingError::IdentityCollision(group_id));
        }
        let mut weight = 0;
        for (left, right) in interactions.keys() {
            if members.binary_search(left).is_ok() && members.binary_search(right).is_ok() {
                add_u64(&mut weight, interactions[&(left.clone(), right.clone())])?;
            }
        }
        let mut best: Option<(u128, String)> = None;
        for candidate in hosts.keys() {
            let mut cost = 0u128;
            for network in members {
                let home = &networks[network].0;
                let incident = interactions
                    .iter()
                    .filter(|((left, right), _)| left == network || right == network)
                    .map(|(_, value)| *value)
                    .sum::<u64>()
                    .max(1);
                let latency = request.latency.latency(home, candidate).ok_or_else(|| {
                    HierarchicalShardingError::MissingLatency {
                        from: home.clone(),
                        to: candidate.clone(),
                    }
                })?;
                cost = cost
                    .checked_add(u128::from(latency) * u128::from(incident))
                    .ok_or(HierarchicalShardingError::Overflow)?;
            }
            let candidate_key = (cost, candidate.clone());
            if best.as_ref().is_none_or(|current| candidate_key < *current) {
                best = Some(candidate_key);
            }
        }
        let anchor_node = best.expect("hosts is non-empty").1;
        let placement = NetworkGroupPlacement {
            group_id: group_id.clone(),
            network_ids: members.clone(),
            communication_weight: weight,
            anchor_node,
        };
        for network in members {
            group_by_network.insert(network.clone(), group_id.clone());
        }
        groups.push(placement);
    }
    groups.sort_by_key(|group| group.group_id.clone());

    let mut used_capacity = BTreeMap::<String, (u64, u64)>::new();
    let mut used_shard_ids = BTreeSet::new();
    let mut used_sub_shard_ids = BTreeSet::new();
    let mut shards = Vec::new();
    for (network_id, (_, units)) in &networks {
        let group_id = group_by_network[network_id].clone();
        let anchor = groups
            .iter()
            .find(|group| group.group_id == group_id)
            .expect("group exists")
            .anchor_node
            .clone();
        let mut by_area = BTreeMap::<u64, BTreeMap<u32, Vec<&NeuralUnit>>>::new();
        for unit in units {
            by_area
                .entry(unit.area_id)
                .or_default()
                .entry(unit.layer)
                .or_default()
                .push(unit);
        }
        for (area_id, by_layer) in by_area {
            let area_label = by_layer
                .values()
                .flat_map(|units| units.iter())
                .map(|unit| unit.area_label.as_str())
                .next()
                .unwrap_or("topology:unlabelled")
                .to_owned();
            let shard_key = format!("{network_id}:area:{area_id}");
            let shard_id = ShardId::new(stable_id(&shard_key))
                .map_err(|_| HierarchicalShardingError::IdentityCollision(shard_key.clone()))?;
            if !used_shard_ids.insert(shard_id) {
                return Err(HierarchicalShardingError::IdentityCollision(shard_key));
            }
            let mut total_work = 0;
            let mut total_state = 0;
            let mut layers = Vec::new();
            for (layer, layer_units) in by_layer {
                let mut chunks: Vec<Vec<&NeuralUnit>> = Vec::new();
                for unit in layer_units {
                    let start_new = chunks.last().is_none_or(|chunk| {
                        let work = chunk.iter().map(|item| item.work_units).sum::<u64>();
                        work > 0
                            && work
                                .checked_add(unit.work_units)
                                .is_none_or(|next| next > request.target_sub_shard_work_units)
                    });
                    if start_new {
                        chunks.push(Vec::new());
                    }
                    chunks.last_mut().expect("chunk exists").push(unit);
                }
                let mut sub_shards = Vec::new();
                for (chunk_index, chunk) in chunks.into_iter().enumerate() {
                    let mut work = 0;
                    let mut state = 0;
                    let mut neuron_ids = Vec::new();
                    let mut neuron_kinds = Vec::new();
                    for unit in chunk {
                        add_u64(&mut work, unit.work_units)?;
                        add_u64(&mut state, unit.state_bytes)?;
                        neuron_ids.push(unit.neuron_id);
                        neuron_kinds.push(unit.kind);
                    }
                    let sub_key =
                        format!("{network_id}:area:{area_id}:layer:{layer}:part:{chunk_index}");
                    let sub_shard_id = SubShardId::new(stable_id(&sub_key)).map_err(|_| {
                        HierarchicalShardingError::IdentityCollision(sub_key.clone())
                    })?;
                    if !used_sub_shard_ids.insert(sub_shard_id) {
                        return Err(HierarchicalShardingError::IdentityCollision(sub_key));
                    }
                    let active_node = choose_host(
                        &request.latency,
                        &hosts,
                        &mut used_capacity,
                        &anchor,
                        sub_shard_id,
                        work,
                        state,
                    )?;
                    let latency_to_group_anchor_us = request
                        .latency
                        .latency(&anchor, &active_node)
                        .ok_or_else(|| HierarchicalShardingError::MissingLatency {
                            from: anchor.clone(),
                            to: active_node.clone(),
                        })?;
                    add_u64(&mut total_work, work)?;
                    add_u64(&mut total_state, state)?;
                    sub_shards.push(SubShardPlacement {
                        sub_shard_id,
                        parent_shard_id: shard_id,
                        layer,
                        neuron_ids,
                        neuron_kinds,
                        work_units: work,
                        state_bytes: state,
                        active_node,
                        latency_to_group_anchor_us,
                    });
                }
                layers.push(LayerPlacement { layer, sub_shards });
            }
            shards.push(AreaShardPlacement {
                shard_id,
                network_id: network_id.clone(),
                group_id: group_id.clone(),
                area_id,
                area_label,
                layers,
                total_work_units: total_work,
                total_state_bytes: total_state,
            });
        }
    }
    shards.sort_by_key(|shard| shard.shard_id);
    let mut plan = HierarchicalShardPlan {
        schema_version: HIERARCHICAL_SHARDING_SCHEMA_VERSION,
        groups,
        shards,
        digest: StateDigest([0; 16]),
    };
    plan.digest = plan.calculate_digest();
    Ok(plan)
}

fn choose_host(
    latency: &LatencyMatrix,
    hosts: &BTreeMap<String, HostCapacity>,
    used: &mut BTreeMap<String, (u64, u64)>,
    anchor: &str,
    sub_shard_id: SubShardId,
    work: u64,
    state: u64,
) -> Result<String, HierarchicalShardingError> {
    let mut candidates = Vec::<(u64, u64, u64, String)>::new();
    for (node, host) in hosts {
        let Some(network_latency) = latency.latency(anchor, node) else {
            return Err(HierarchicalShardingError::MissingLatency {
                from: anchor.to_owned(),
                to: node.clone(),
            });
        };
        let (used_work, used_state) = used.get(node).copied().unwrap_or_default();
        if used_work.saturating_add(work) <= host.capacity_units
            && used_state.saturating_add(state) <= host.memory_bytes
        {
            // Latency is the primary placement key. Remaining headroom makes
            // equal-latency choices deterministic without overriding locality.
            candidates.push((
                network_latency,
                used_work.saturating_add(work),
                used_state.saturating_add(state),
                node.clone(),
            ));
        }
    }
    candidates.sort();
    let Some((_, _, _, node)) = candidates.into_iter().next() else {
        return Err(HierarchicalShardingError::NoCapacity {
            sub_shard: sub_shard_id,
        });
    };
    let entry = used.entry(node.clone()).or_default();
    entry.0 = entry
        .0
        .checked_add(work)
        .ok_or(HierarchicalShardingError::Overflow)?;
    entry.1 = entry
        .1
        .checked_add(state)
        .ok_or(HierarchicalShardingError::Overflow)?;
    Ok(node)
}

impl HierarchicalShardPlan {
    fn calculate_digest(&self) -> StateDigest {
        let mut material = self.clone();
        material.digest = StateDigest([0; 16]);
        let bytes = serde_json::to_vec(&material).expect("hierarchical plan is serialisable");
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("hierarchical-sharding:v1", bytes);
        digest.finish()
    }

    pub fn verify(&self) -> Result<(), HierarchicalShardingError> {
        if self.schema_version != HIERARCHICAL_SHARDING_SCHEMA_VERSION {
            return Err(HierarchicalShardingError::InvalidTarget);
        }
        let expected = self.calculate_digest();
        if expected != self.digest {
            return Err(HierarchicalShardingError::IdentityCollision(
                "plan digest mismatch".to_owned(),
            ));
        }
        let mut shard_ids = BTreeSet::new();
        let mut sub_ids = BTreeSet::new();
        let mut active_neuron_ids = BTreeSet::new();
        for shard in &self.shards {
            if !shard_ids.insert(shard.shard_id) {
                return Err(HierarchicalShardingError::IdentityCollision(
                    "duplicate shard".to_owned(),
                ));
            }
            for layer in &shard.layers {
                for sub_shard in &layer.sub_shards {
                    if !sub_ids.insert(sub_shard.sub_shard_id)
                        || sub_shard.parent_shard_id != shard.shard_id
                        || sub_shard.layer != layer.layer
                    {
                        return Err(HierarchicalShardingError::IdentityCollision(
                            "invalid sub-shard ownership".to_owned(),
                        ));
                    }
                    for neuron_id in &sub_shard.neuron_ids {
                        if !active_neuron_ids.insert(*neuron_id) {
                            return Err(HierarchicalShardingError::IdentityCollision(
                                "duplicate active neuron ownership".to_owned(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn sub_shard_count(&self) -> usize {
        self.shards
            .iter()
            .flat_map(|shard| &shard.layers)
            .map(|layer| layer.sub_shards.len())
            .sum()
    }

    /// Build the one biological ownership view for the active plan.  This
    /// consumes only active sub-shards from the plan; callers must keep warm
    /// replicas in their durability model rather than adding them here.
    pub fn active_biological_ownership(
        &self,
    ) -> Result<BiologicalOwnershipMap, BiologicalOwnershipError> {
        let mut placements = Vec::new();
        for shard in &self.shards {
            for layer in &shard.layers {
                for sub_shard in &layer.sub_shards {
                    for (index, neuron_id) in sub_shard.neuron_ids.iter().enumerate() {
                        if sub_shard
                            .neuron_kinds
                            .get(index)
                            .copied()
                            .unwrap_or(NeuralUnitKind::BiologicalNeuron)
                            == NeuralUnitKind::AreaLayerAggregate
                        {
                            return Err(BiologicalOwnershipError::AggregateUnit(*neuron_id));
                        }
                        placements.push(BiologicalNeuronPlacement {
                            neuron_id: *neuron_id,
                            area_id: shard.area_id,
                            area_label: shard.area_label.clone(),
                            layer: layer.layer,
                            sub_shard_id: sub_shard.sub_shard_id,
                            active_node: sub_shard.active_node.clone(),
                        });
                    }
                }
            }
        }
        BiologicalOwnershipMap::new(
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            placements,
        )
    }
}
