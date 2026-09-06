//! Deterministic resource admission and virtual-shard placement contracts.
//!
//! This module deliberately contains no networking, storage, executor locks or
//! operating-system calls.  It is the reusable decision boundary between the
//! topology planner, the scheduler and a later migration controller.  A plan
//! is a value: callers may evaluate it off the async control thread, persist it
//! through their own adapter, and apply it only after the control plane has
//! fenced the affected shards.
//!
//! The current implementation is a reference planner.  It is safe to use for
//! proposal generation and deterministic tests; it does not itself grant a
//! node authority or move biological state.

use crate::deterministic::{
    BrainId, ComponentId, EventId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId,
    StateDigest, StateDigestBuilder, TopologyGeneration,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Version of the serialised placement contract.
pub const PLACEMENT_SCHEMA_VERSION: u32 = 2;
/// Version of the orchestrator/CLI placement command envelope.
pub const PLACEMENT_COMMAND_SCHEMA_VERSION: u32 = 2;

/// A bounded observation of one enrolled compute node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceObservation {
    pub node_id: String,
    /// Stable device identity within the node. The reference planner accepts
    /// one schedulable observation per node; multi-device expansion belongs to
    /// the resource-inventory adapter.
    pub device_id: String,
    pub healthy: bool,
    pub enrolled: bool,
    pub compute_authorised: bool,
    pub failure_domain: String,
    pub numerical_profiles: Vec<String>,
    pub capacity_units: u64,
    pub reserved_capacity_units: u64,
    pub memory_bytes: u64,
    pub reserved_memory_bytes: u64,
    /// Usable local storage for checkpoint/WAL reservations. Inventory
    /// adapters should report space after filesystem safety reserves.
    pub storage_bytes: u64,
    pub reserved_storage_bytes: u64,
    pub network_bytes_per_second: u64,
    pub reserved_network_bytes_per_second: u64,
    /// Pressure values are fixed-point thousandths, avoiding floating-point
    /// ordering in the planner and making replay independent of CPU/ABI.
    pub cpu_pressure_milli: u16,
    pub memory_pressure_milli: u16,
    pub network_pressure_milli: u16,
    pub thermal_pressure_milli: u16,
}

impl ResourceObservation {
    fn validate(&self) -> Result<(), PlacementError> {
        if self.node_id.trim().is_empty()
            || self.device_id.trim().is_empty()
            || self.failure_domain.trim().is_empty()
        {
            return Err(PlacementError::InvalidResource(
                "node, device and failure domain are required",
            ));
        }
        if self.capacity_units == 0 || self.memory_bytes == 0 || self.storage_bytes == 0 {
            return Err(PlacementError::InvalidResource(
                "capacity, memory and storage must be non-zero",
            ));
        }
        for value in [
            self.cpu_pressure_milli,
            self.memory_pressure_milli,
            self.network_pressure_milli,
            self.thermal_pressure_milli,
        ] {
            if value > 1_000 {
                return Err(PlacementError::InvalidResource(
                    "pressure must be between 0 and 1000 milli-units",
                ));
            }
        }
        if self.reserved_capacity_units > self.capacity_units
            || self.reserved_memory_bytes > self.memory_bytes
            || self.reserved_storage_bytes > self.storage_bytes
            || self.reserved_network_bytes_per_second > self.network_bytes_per_second
        {
            return Err(PlacementError::InvalidResource(
                "reserved resources exceed node capacity",
            ));
        }
        Ok(())
    }

    fn available_capacity(&self) -> u64 {
        self.capacity_units
            .saturating_sub(self.reserved_capacity_units)
    }

    fn available_memory(&self) -> u64 {
        self.memory_bytes.saturating_sub(self.reserved_memory_bytes)
    }

    fn available_storage(&self) -> u64 {
        self.storage_bytes
            .saturating_sub(self.reserved_storage_bytes)
    }

    fn available_network(&self) -> u64 {
        self.network_bytes_per_second
            .saturating_sub(self.reserved_network_bytes_per_second)
    }
}

/// A biological virtual shard's measured or bounded resource demand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardDemand {
    pub shard_id: ShardId,
    pub load_units: u64,
    pub memory_bytes: u64,
    /// Immutable checkpoint/WAL storage required by this shard. It is charged
    /// to both active and warm roles before admission.
    pub checkpoint_bytes: u64,
    pub network_bytes_per_second: u64,
    /// Components with the same identity must remain on one active node unless
    /// a later distributed-SCC protocol explicitly handles that component.
    pub zero_delay_component: Option<ComponentId>,
    pub required_numerical_profile: String,
    pub preferred_node: Option<String>,
}

impl ShardDemand {
    fn validate(&self) -> Result<(), PlacementError> {
        if self.load_units == 0 || self.memory_bytes == 0 || self.checkpoint_bytes == 0 {
            return Err(PlacementError::InvalidDemand {
                shard: self.shard_id,
                reason: "load, memory and checkpoint storage must be non-zero",
            });
        }
        if self.required_numerical_profile.trim().is_empty() {
            return Err(PlacementError::InvalidDemand {
                shard: self.shard_id,
                reason: "a numerical profile is required",
            });
        }
        Ok(())
    }
}

/// Hard and soft placement policy.  Values are intentionally explicit rather
/// than inferred from local process state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementConstraints {
    pub minimum_headroom_milli: u16,
    pub maximum_cpu_pressure_milli: u16,
    pub maximum_memory_pressure_milli: u16,
    pub maximum_network_pressure_milli: u16,
    pub maximum_thermal_pressure_milli: u16,
    pub minimum_warm_replicas: u16,
    pub require_distinct_warm_failure_domain: bool,
    pub allow_single_host_degraded_durability: bool,
}

impl Default for PlacementConstraints {
    fn default() -> Self {
        Self {
            minimum_headroom_milli: 150,
            maximum_cpu_pressure_milli: 850,
            maximum_memory_pressure_milli: 850,
            maximum_network_pressure_milli: 850,
            maximum_thermal_pressure_milli: 800,
            minimum_warm_replicas: 1,
            require_distinct_warm_failure_domain: true,
            allow_single_host_degraded_durability: false,
        }
    }
}

impl PlacementConstraints {
    fn validate(&self) -> Result<(), PlacementError> {
        if self.minimum_headroom_milli > 1_000
            || self.maximum_cpu_pressure_milli > 1_000
            || self.maximum_memory_pressure_milli > 1_000
            || self.maximum_network_pressure_milli > 1_000
            || self.maximum_thermal_pressure_milli > 1_000
        {
            return Err(PlacementError::InvalidConstraints);
        }
        Ok(())
    }
}

/// The physical intent requested by automation or an operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementIntent {
    /// Choose from all eligible enrolled resources.
    Automatic,
    /// Co-locate all requested shards on one named node.
    Consolidate { target_node: String },
    /// Remove a node from the active placement set.
    Evacuate { source_node: String },
    /// Move requested shards onto a returning node.
    Reclaim { target_node: String },
}

/// Inputs to the deterministic placement planner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementRequest {
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    /// Control-plane term observed while this proposal was built.
    pub lease_term: LeaseTerm,
    /// Fencing token paired with `lease_term`. The reference authority uses
    /// the term value as its token, but the fields remain explicit on the
    /// wire for future token implementations.
    pub fencing_token: u64,
    pub effective_tag: LogicalTag,
    pub demands: Vec<ShardDemand>,
    pub resources: Vec<ResourceObservation>,
    pub constraints: PlacementConstraints,
    pub intent: PlacementIntent,
}

/// Active and warm locations for one stable virtual shard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardPlacement {
    pub shard_id: ShardId,
    pub active_node: String,
    pub active_device: String,
    pub active_failure_domain: String,
    pub warm_nodes: Vec<String>,
}

/// Actual reservations consumed by a proposed placement.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NodeReservation {
    pub node_id: String,
    pub device_id: String,
    pub failure_domain: String,
    pub active_load_units: u64,
    pub active_memory_bytes: u64,
    pub active_storage_bytes: u64,
    pub active_network_bytes_per_second: u64,
    pub warm_load_units: u64,
    pub warm_memory_bytes: u64,
    pub warm_storage_bytes: u64,
    pub warm_network_bytes_per_second: u64,
    pub warm_shard_count: u16,
}

/// A deterministic explanation suitable for audit and scheduler replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementDecision {
    pub intent: PlacementIntent,
    pub candidate_count: u32,
    pub accepted_shards: u32,
    pub degraded_durability: bool,
    pub explanation: String,
}

/// Immutable proposal consumed by a migration controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub effective_tag: LogicalTag,
    pub placements: Vec<ShardPlacement>,
    pub reservations: Vec<NodeReservation>,
    pub decision: PlacementDecision,
    pub digest: StateDigest,
}

impl PlacementPlan {
    /// Verify the canonical digest and basic plan invariants before persistence
    /// or handing the plan to an external migration adapter.
    pub fn verify(&self) -> Result<(), PlacementError> {
        if self.schema_version != PLACEMENT_SCHEMA_VERSION || self.placements.is_empty() {
            return Err(PlacementError::InvalidPlan(
                "schema or shard set is invalid",
            ));
        }
        if self.effective_tag.microstep != 0 {
            return Err(PlacementError::InvalidPlan(
                "placement activation must be at microstep zero",
            ));
        }
        if self.fencing_token == 0 {
            return Err(PlacementError::InvalidPlan(
                "fencing token must be non-zero",
            ));
        }
        if self.decision.accepted_shards as usize != self.placements.len() {
            return Err(PlacementError::InvalidPlan(
                "decision shard count does not match placements",
            ));
        }
        let mut reservation_nodes = BTreeSet::new();
        for reservation in &self.reservations {
            if reservation.node_id.trim().is_empty()
                || reservation.device_id.trim().is_empty()
                || reservation.failure_domain.trim().is_empty()
                || !reservation_nodes.insert(reservation.node_id.as_str())
            {
                return Err(PlacementError::InvalidPlan(
                    "reservation identities are invalid or duplicated",
                ));
            }
        }
        let mut shards = BTreeSet::new();
        for placement in &self.placements {
            if !shards.insert(placement.shard_id)
                || placement.active_node.trim().is_empty()
                || placement.active_device.trim().is_empty()
                || placement.active_failure_domain.trim().is_empty()
                || !reservation_nodes.contains(placement.active_node.as_str())
            {
                return Err(PlacementError::InvalidPlan(
                    "duplicate or empty shard placement",
                ));
            }
            let active_reservation = self
                .reservations
                .iter()
                .find(|reservation| reservation.node_id == placement.active_node)
                .expect("active node was checked against reservations");
            if active_reservation.device_id != placement.active_device
                || active_reservation.failure_domain != placement.active_failure_domain
            {
                return Err(PlacementError::InvalidPlan(
                    "active placement does not match its reservation",
                ));
            }
            let mut warm = BTreeSet::new();
            for node in &placement.warm_nodes {
                if node.trim().is_empty()
                    || node == &placement.active_node
                    || !warm.insert(node)
                    || !reservation_nodes.contains(node.as_str())
                {
                    return Err(PlacementError::InvalidPlan(
                        "invalid warm replica placement",
                    ));
                }
            }
        }
        if self.calculate_digest()? != self.digest {
            return Err(PlacementError::DigestMismatch);
        }
        Ok(())
    }

    /// Return the immutable identity used by migration and audit records.
    pub const fn digest(&self) -> StateDigest {
        self.digest
    }

    fn calculate_digest(&self) -> Result<StateDigest, PlacementError> {
        let mut material = self.clone();
        material.digest = StateDigest([0; 16]);
        let encoded = serde_json::to_vec(&material)
            .map_err(|error| PlacementError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("placement-plan:v1", encoded);
        Ok(digest.finish())
    }
}

/// Errors returned before a plan can become authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlacementError {
    #[error("invalid resource observation: {0}")]
    InvalidResource(&'static str),
    #[error("invalid shard {shard}: {reason}")]
    InvalidDemand {
        shard: ShardId,
        reason: &'static str,
    },
    #[error("placement constraints are invalid")]
    InvalidConstraints,
    #[error("placement request has no shards")]
    NoShards,
    #[error("no eligible node can host shard {shard}: {reason}")]
    NoEligibleNode { shard: ShardId, reason: String },
    #[error("zero-delay component {component} would be split")]
    SplitZeroDelayComponent { component: ComponentId },
    #[error("placement plan is invalid: {0}")]
    InvalidPlan(&'static str),
    #[error("placement plan digest does not match its contents")]
    DigestMismatch,
    #[error("placement encoding failed: {0}")]
    Encoding(String),
    #[error("shard {shard} occurs more than once in the placement request")]
    DuplicateShard { shard: ShardId },
    #[error("repartition plan is invalid: {0}")]
    InvalidRepartition(&'static str),
    #[error("repartition does not change the shard count")]
    NoShardCountChange,
}

/// A typed operation submitted by an orchestrator or CLI. The envelope is
/// deliberately independent of gRPC, HTTP and UI code so every product can
/// use the same validation and digest rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementCommandKind {
    /// Evaluate a placement proposal without changing authority.
    PlanPlacement(PlacementRequest),
    /// Apply a previously verified immutable placement plan through the
    /// orchestrator's fenced operation path.
    ApplyPlacement(PlacementPlan),
    /// Change virtual shard count through an explicit lineage transaction.
    Repartition(RepartitionPlan),
    /// Move one stable shard between physical resources.
    Migrate(MigrationOperation),
    /// Verify that a node has no remaining local authority or output work.
    PrepareForShutdown(ShutdownReadiness),
}

/// Idempotent, optimistic-concurrency command envelope for placement control.
///
/// Constructing or verifying this value does not grant authority, move state,
/// or contact a node. The orchestrator must persist it, authenticate the
/// principal, check its leader term/resource version and only then dispatch the
/// operation to a fenced adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementCommand {
    pub schema_version: u32,
    pub request_id: String,
    pub idempotency_key: String,
    pub principal_id: String,
    pub brain_id: BrainId,
    pub expected_resource_version: u64,
    pub observed_leader_term: LeaseTerm,
    pub kind: PlacementCommandKind,
    pub digest: StateDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlacementCommandError {
    #[error("command field {0} is empty")]
    EmptyField(&'static str),
    #[error("command field {field} exceeds the {limit}-byte limit")]
    FieldTooLong { field: &'static str, limit: usize },
    #[error("command brain identity does not match its payload")]
    BrainMismatch,
    #[error("embedded placement command is invalid: {0}")]
    Placement(PlacementError),
    #[error("embedded migration command is invalid: {0}")]
    Migration(MigrationError),
    #[error("shutdown command is invalid: {0}")]
    Shutdown(&'static str),
    #[error("placement command digest does not match its contents")]
    DigestMismatch,
    #[error("placement command encoding failed: {0}")]
    Encoding(String),
}

impl PlacementCommand {
    /// Create a command with a canonical digest and validate its payload.
    pub fn new(
        request_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        principal_id: impl Into<String>,
        brain_id: BrainId,
        expected_resource_version: u64,
        observed_leader_term: LeaseTerm,
        kind: PlacementCommandKind,
    ) -> Result<Self, PlacementCommandError> {
        let command = Self {
            schema_version: PLACEMENT_COMMAND_SCHEMA_VERSION,
            request_id: request_id.into(),
            idempotency_key: idempotency_key.into(),
            principal_id: principal_id.into(),
            brain_id,
            expected_resource_version,
            observed_leader_term,
            kind,
            digest: StateDigest([0; 16]),
        };
        command.validate_payload()?;
        let mut command = command;
        command.digest = command.calculate_digest()?;
        command.verify()?;
        Ok(command)
    }

    /// Verify identity, bounded fields, embedded contracts and the canonical
    /// digest after persistence or deserialisation.
    pub fn verify(&self) -> Result<(), PlacementCommandError> {
        if self.schema_version != PLACEMENT_COMMAND_SCHEMA_VERSION {
            return Err(PlacementCommandError::Encoding(
                "unsupported placement command schema".to_owned(),
            ));
        }
        self.validate_fields()?;
        self.validate_payload()?;
        if self.calculate_digest()? != self.digest {
            return Err(PlacementCommandError::DigestMismatch);
        }
        Ok(())
    }

    pub const fn digest(&self) -> StateDigest {
        self.digest
    }

    fn validate_fields(&self) -> Result<(), PlacementCommandError> {
        for (field, value, limit) in [
            ("request_id", self.request_id.as_str(), 256),
            ("idempotency_key", self.idempotency_key.as_str(), 256),
            ("principal_id", self.principal_id.as_str(), 256),
        ] {
            if value.trim().is_empty() {
                return Err(PlacementCommandError::EmptyField(field));
            }
            if value.len() > limit {
                return Err(PlacementCommandError::FieldTooLong { field, limit });
            }
        }
        Ok(())
    }

    fn validate_payload(&self) -> Result<(), PlacementCommandError> {
        match &self.kind {
            PlacementCommandKind::PlanPlacement(request) => {
                if request.brain_id != self.brain_id {
                    return Err(PlacementCommandError::BrainMismatch);
                }
                PlacementPlanner
                    .plan(request.clone())
                    .map(|_| ())
                    .map_err(PlacementCommandError::Placement)
            }
            PlacementCommandKind::ApplyPlacement(plan) => {
                if plan.brain_id != self.brain_id {
                    return Err(PlacementCommandError::BrainMismatch);
                }
                plan.verify().map_err(PlacementCommandError::Placement)
            }
            PlacementCommandKind::Repartition(plan) => {
                if plan.brain_id != self.brain_id {
                    return Err(PlacementCommandError::BrainMismatch);
                }
                plan.verify().map_err(PlacementCommandError::Placement)
            }
            PlacementCommandKind::Migrate(operation) => {
                if operation.brain_id != self.brain_id {
                    return Err(PlacementCommandError::BrainMismatch);
                }
                operation.verify().map_err(PlacementCommandError::Migration)
            }
            PlacementCommandKind::PrepareForShutdown(readiness) => {
                if readiness.brain_id != self.brain_id {
                    return Err(PlacementCommandError::BrainMismatch);
                }
                readiness
                    .validate()
                    .map_err(PlacementCommandError::Shutdown)
            }
        }
    }

    fn calculate_digest(&self) -> Result<StateDigest, PlacementCommandError> {
        let mut material = self.clone();
        material.digest = StateDigest([0; 16]);
        let encoded = serde_json::to_vec(&material)
            .map_err(|error| PlacementCommandError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("placement-command:v1", encoded);
        Ok(digest.finish())
    }
}

/// Stateless deterministic planner.  Constructing a new planner is cheap and
/// makes it safe to run several proposals concurrently on bounded worker
/// tasks without shared mutable scheduler state.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlacementPlanner;

impl PlacementPlanner {
    pub fn plan(&self, mut request: PlacementRequest) -> Result<PlacementPlan, PlacementError> {
        if request.demands.is_empty() {
            return Err(PlacementError::NoShards);
        }
        request.constraints.validate()?;
        for demand in &request.demands {
            demand.validate()?;
        }
        let mut demand_ids = BTreeSet::new();
        for demand in &request.demands {
            if !demand_ids.insert(demand.shard_id) {
                return Err(PlacementError::DuplicateShard {
                    shard: demand.shard_id,
                });
            }
        }
        let mut resources = BTreeMap::new();
        for resource in request.resources {
            resource.validate()?;
            if resources
                .insert(resource.node_id.clone(), resource)
                .is_some()
            {
                return Err(PlacementError::InvalidResource("duplicate node identity"));
            }
        }
        if resources.is_empty() {
            return Err(PlacementError::NoEligibleNode {
                shard: request.demands[0].shard_id,
                reason: "resource inventory is empty".to_owned(),
            });
        }

        request.demands.sort_by(|left, right| {
            right
                .load_units
                .cmp(&left.load_units)
                .then_with(|| left.shard_id.cmp(&right.shard_id))
        });
        let mut reservations = resources
            .keys()
            .map(|node_id| {
                (
                    node_id.clone(),
                    NodeReservation {
                        node_id: node_id.clone(),
                        device_id: resources
                            .get(node_id)
                            .expect("reservation resource exists")
                            .device_id
                            .clone(),
                        failure_domain: resources
                            .get(node_id)
                            .expect("reservation resource exists")
                            .failure_domain
                            .clone(),
                        ..NodeReservation::default()
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut component_nodes = BTreeMap::<ComponentId, String>::new();
        let mut placements = Vec::with_capacity(request.demands.len());
        let candidate_nodes = candidate_nodes(&request.intent, &resources);
        if candidate_nodes.is_empty() {
            return Err(PlacementError::NoEligibleNode {
                shard: request.demands[0].shard_id,
                reason: "intent has no candidate nodes".to_owned(),
            });
        }

        for demand in &request.demands {
            let mut eligible = candidate_nodes
                .iter()
                .filter(|node_id| {
                    let resource = resources.get(*node_id).expect("candidate exists");
                    let component_is_local = demand.zero_delay_component.is_none_or(|component| {
                        component_nodes
                            .get(&component)
                            .is_none_or(|assigned| assigned == *node_id)
                    });
                    resource_eligible(resource, demand, &request.constraints) && component_is_local
                })
                .filter(|node_id| {
                    let reservation = reservations.get(*node_id).expect("reservation exists");
                    fits(
                        resource_for(&resources, node_id),
                        reservation,
                        demand,
                        &request.constraints,
                    )
                })
                .collect::<Vec<_>>();
            eligible.sort_by(|left, right| {
                let left_score = placement_score(
                    resources.get(*left).expect("candidate exists"),
                    reservations.get(*left).expect("reservation exists"),
                    demand,
                    demand.preferred_node.as_deref() == Some(left.as_str()),
                );
                let right_score = placement_score(
                    resources.get(*right).expect("candidate exists"),
                    reservations.get(*right).expect("reservation exists"),
                    demand,
                    demand.preferred_node.as_deref() == Some(right.as_str()),
                );
                right_score.cmp(&left_score).then_with(|| left.cmp(right))
            });
            let Some(node_id) = eligible.first() else {
                let reason = if let Some(component) = demand.zero_delay_component {
                    if component_nodes.contains_key(&component) {
                        format!("zero-delay component {component} cannot fit its existing node")
                    } else {
                        "capacity, profile, health or policy constraints".to_owned()
                    }
                } else {
                    "capacity, profile, health or policy constraints".to_owned()
                };
                return Err(PlacementError::NoEligibleNode {
                    shard: demand.shard_id,
                    reason,
                });
            };
            if let Some(component) = demand.zero_delay_component {
                if let Some(previous) = component_nodes.insert(component, (*node_id).clone()) {
                    if previous != (*node_id).as_str() {
                        return Err(PlacementError::SplitZeroDelayComponent { component });
                    }
                }
            }
            add_reservation(
                reservations.get_mut(*node_id).expect("reservation exists"),
                demand,
                false,
            )?;
            placements.push(ShardPlacement {
                shard_id: demand.shard_id,
                active_node: (*node_id).clone(),
                active_device: resources
                    .get(*node_id)
                    .expect("candidate resource exists")
                    .device_id
                    .clone(),
                active_failure_domain: resources
                    .get(*node_id)
                    .expect("candidate resource exists")
                    .failure_domain
                    .clone(),
                warm_nodes: Vec::new(),
            });
        }

        // Warm replicas are selected after active placement so they cannot
        // consume the capacity needed to establish the primary authority.
        let active_by_shard = placements
            .iter()
            .map(|placement| (placement.shard_id, placement.active_node.clone()))
            .collect::<BTreeMap<_, _>>();
        let demand_by_shard = request
            .demands
            .iter()
            .map(|demand| (demand.shard_id, demand))
            .collect::<BTreeMap<_, _>>();
        let mut degraded = false;
        for placement in &mut placements {
            let demand = demand_by_shard
                .get(&placement.shard_id)
                .expect("every placement has a demand");
            let active = active_by_shard
                .get(&placement.shard_id)
                .expect("active placement exists");
            let mut warm_candidates = candidate_nodes
                .iter()
                .filter(|node_id| {
                    let resource = resources.get(*node_id).expect("candidate exists");
                    node_id.as_str() != active.as_str()
                        && (!request.constraints.require_distinct_warm_failure_domain
                            || resource.failure_domain
                                != resources.get(active).expect("active exists").failure_domain)
                        && resource_eligible(resource, demand, &request.constraints)
                        && fits(
                            resource,
                            reservations.get(*node_id).expect("reservation exists"),
                            demand,
                            &request.constraints,
                        )
                })
                .collect::<Vec<_>>();
            warm_candidates.sort_by(|left, right| left.cmp(right));
            for node_id in warm_candidates
                .into_iter()
                .take(request.constraints.minimum_warm_replicas as usize)
            {
                add_reservation(
                    reservations.get_mut(node_id).expect("reservation exists"),
                    demand,
                    true,
                )?;
                placement.warm_nodes.push(node_id.clone());
            }
            if placement.warm_nodes.len() < request.constraints.minimum_warm_replicas as usize {
                if request.constraints.allow_single_host_degraded_durability {
                    degraded = true;
                } else {
                    return Err(PlacementError::NoEligibleNode {
                        shard: placement.shard_id,
                        reason: "warm replica durability requirement cannot be met".to_owned(),
                    });
                }
            }
        }

        placements.sort_by_key(|placement| placement.shard_id);
        let reservations = reservations.into_values().collect::<Vec<_>>();
        let mut plan = PlacementPlan {
            schema_version: PLACEMENT_SCHEMA_VERSION,
            brain_id: request.brain_id,
            topology_generation: request.topology_generation,
            partition_generation: request.partition_generation,
            lease_term: request.lease_term,
            fencing_token: request.fencing_token,
            effective_tag: request.effective_tag,
            placements,
            reservations,
            decision: PlacementDecision {
                intent: request.intent,
                candidate_count: candidate_nodes.len() as u32,
                accepted_shards: request.demands.len() as u32,
                degraded_durability: degraded,
                explanation: if degraded {
                    "accepted with explicitly authorised single-host durability degradation"
                        .to_owned()
                } else {
                    "accepted by deterministic capacity, profile, SCC and durability checks"
                        .to_owned()
                },
            },
            digest: StateDigest([0; 16]),
        };
        plan.digest = plan.calculate_digest()?;
        plan.verify()?;
        Ok(plan)
    }
}

/// A stable lineage edge used when a topology transaction changes the number
/// of virtual shards. A source may map to several successors for a split, and
/// several sources may map to one successor for a consolidation. The actual
/// state transfer and biological ownership change remain a later fenced
/// transaction; this value only proves that the proposed mapping is complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardLineage {
    pub source_shard: ShardId,
    pub successor_shards: Vec<ShardId>,
}

/// Immutable proposal for increasing or decreasing virtual shard count.
///
/// Physical co-location does not require this contract. It is used only when
/// stable virtual ownership itself changes, which must happen at a committed
/// logical boundary and a new partition generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepartitionPlan {
    pub schema_version: u32,
    pub operation_id: EventId,
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub source_partition_generation: PartitionGeneration,
    pub target_partition_generation: PartitionGeneration,
    pub effective_tag: LogicalTag,
    pub source_shards: Vec<ShardId>,
    pub target_shards: Vec<ShardId>,
    pub lineage: Vec<ShardLineage>,
    pub digest: StateDigest,
}

impl RepartitionPlan {
    /// Build and validate a lineage proposal before it reaches the topology
    /// transaction coordinator.
    pub fn new(
        operation_id: EventId,
        brain_id: BrainId,
        topology_generation: TopologyGeneration,
        source_partition_generation: PartitionGeneration,
        target_partition_generation: PartitionGeneration,
        effective_tag: LogicalTag,
        source_shards: Vec<ShardId>,
        target_shards: Vec<ShardId>,
        lineage: Vec<ShardLineage>,
    ) -> Result<Self, PlacementError> {
        if source_shards.is_empty() || target_shards.is_empty() || lineage.is_empty() {
            return Err(PlacementError::InvalidRepartition(
                "source, target and lineage sets are required",
            ));
        }
        if source_partition_generation >= target_partition_generation {
            return Err(PlacementError::InvalidRepartition(
                "target partition generation must advance",
            ));
        }
        if effective_tag.microstep != 0 {
            return Err(PlacementError::InvalidRepartition(
                "repartition must activate at microstep zero",
            ));
        }
        let source_set = unique_shards(&source_shards, "source")?;
        let target_set = unique_shards(&target_shards, "target")?;
        if source_set.len() == target_set.len() {
            return Err(PlacementError::NoShardCountChange);
        }
        let mut source_shards = source_shards;
        let mut target_shards = target_shards;
        source_shards.sort_unstable();
        target_shards.sort_unstable();
        let mut lineage = lineage;
        for edge in &mut lineage {
            edge.successor_shards.sort_unstable();
        }
        lineage.sort_by_key(|edge| edge.source_shard);
        let mut mapped_sources = BTreeSet::new();
        let mut mapped_targets = BTreeSet::new();
        for edge in &lineage {
            if !source_set.contains(&edge.source_shard)
                || !mapped_sources.insert(edge.source_shard)
                || edge.successor_shards.is_empty()
            {
                return Err(PlacementError::InvalidRepartition(
                    "lineage must cover each source exactly once",
                ));
            }
            let mut edge_targets = BTreeSet::new();
            for successor in &edge.successor_shards {
                if !target_set.contains(successor) || !edge_targets.insert(*successor) {
                    return Err(PlacementError::InvalidRepartition(
                        "lineage contains an unknown or duplicate successor",
                    ));
                }
                mapped_targets.insert(*successor);
            }
        }
        if mapped_sources != source_set || mapped_targets != target_set {
            return Err(PlacementError::InvalidRepartition(
                "lineage must cover the complete source and target sets",
            ));
        }
        let mut plan = Self {
            schema_version: PLACEMENT_SCHEMA_VERSION,
            operation_id,
            brain_id,
            topology_generation,
            source_partition_generation,
            target_partition_generation,
            effective_tag,
            source_shards,
            target_shards,
            lineage,
            digest: StateDigest([0; 16]),
        };
        plan.digest = plan.calculate_digest()?;
        plan.verify()?;
        Ok(plan)
    }

    /// Recheck the immutable lineage and digest after deserialisation or
    /// before handing the proposal to a topology transaction coordinator.
    pub fn verify(&self) -> Result<(), PlacementError> {
        if self.schema_version != PLACEMENT_SCHEMA_VERSION
            || self.source_partition_generation >= self.target_partition_generation
            || self.effective_tag.microstep != 0
        {
            return Err(PlacementError::InvalidRepartition(
                "schema, generation or activation boundary is invalid",
            ));
        }
        let source_set = unique_shards(&self.source_shards, "source")?;
        let target_set = unique_shards(&self.target_shards, "target")?;
        if source_set.len() == target_set.len() {
            return Err(PlacementError::NoShardCountChange);
        }
        let mut mapped_sources = BTreeSet::new();
        let mut mapped_targets = BTreeSet::new();
        for edge in &self.lineage {
            if !source_set.contains(&edge.source_shard)
                || !mapped_sources.insert(edge.source_shard)
                || edge.successor_shards.is_empty()
            {
                return Err(PlacementError::InvalidRepartition(
                    "lineage must cover each source exactly once",
                ));
            }
            let mut edge_targets = BTreeSet::new();
            for successor in &edge.successor_shards {
                if !target_set.contains(successor) || !edge_targets.insert(*successor) {
                    return Err(PlacementError::InvalidRepartition(
                        "lineage contains an unknown or duplicate successor",
                    ));
                }
                mapped_targets.insert(*successor);
            }
        }
        if mapped_sources != source_set || mapped_targets != target_set {
            return Err(PlacementError::InvalidRepartition(
                "lineage must cover the complete source and target sets",
            ));
        }
        if self.calculate_digest()? != self.digest {
            return Err(PlacementError::DigestMismatch);
        }
        Ok(())
    }

    pub const fn digest(&self) -> StateDigest {
        self.digest
    }

    fn calculate_digest(&self) -> Result<StateDigest, PlacementError> {
        let mut material = self.clone();
        material.digest = StateDigest([0; 16]);
        let encoded = serde_json::to_vec(&material)
            .map_err(|error| PlacementError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("repartition-plan:v1", encoded);
        Ok(digest.finish())
    }
}

fn unique_shards(
    shards: &[ShardId],
    label: &'static str,
) -> Result<BTreeSet<ShardId>, PlacementError> {
    let mut unique = BTreeSet::new();
    for shard in shards {
        if !unique.insert(*shard) {
            return Err(PlacementError::InvalidRepartition(label));
        }
    }
    Ok(unique)
}

fn candidate_nodes(
    intent: &PlacementIntent,
    resources: &BTreeMap<String, ResourceObservation>,
) -> Vec<String> {
    let mut candidates = resources
        .values()
        .filter(|resource| match intent {
            PlacementIntent::Automatic => true,
            PlacementIntent::Consolidate { target_node }
            | PlacementIntent::Reclaim { target_node } => &resource.node_id == target_node,
            PlacementIntent::Evacuate { source_node } => &resource.node_id != source_node,
        })
        .map(|resource| resource.node_id.clone())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates
}

fn resource_for<'a>(
    resources: &'a BTreeMap<String, ResourceObservation>,
    node_id: &str,
) -> &'a ResourceObservation {
    resources.get(node_id).expect("candidate resource exists")
}

fn resource_eligible(
    resource: &ResourceObservation,
    demand: &ShardDemand,
    constraints: &PlacementConstraints,
) -> bool {
    resource.healthy
        && resource.enrolled
        && resource.compute_authorised
        && resource
            .numerical_profiles
            .iter()
            .any(|profile| profile == &demand.required_numerical_profile)
        && resource.cpu_pressure_milli <= constraints.maximum_cpu_pressure_milli
        && resource.memory_pressure_milli <= constraints.maximum_memory_pressure_milli
        && resource.network_pressure_milli <= constraints.maximum_network_pressure_milli
        && resource.thermal_pressure_milli <= constraints.maximum_thermal_pressure_milli
}

fn fits(
    resource: &ResourceObservation,
    reservation: &NodeReservation,
    demand: &ShardDemand,
    constraints: &PlacementConstraints,
) -> bool {
    let headroom = u128::from(1_000u16.saturating_sub(constraints.minimum_headroom_milli));
    let capacity = u128::from(resource.available_capacity()) * headroom / 1_000;
    let memory = u128::from(resource.available_memory()) * headroom / 1_000;
    let storage = u128::from(resource.available_storage()) * headroom / 1_000;
    let network = u128::from(resource.available_network()) * headroom / 1_000;
    u128::from(reservation.active_load_units)
        + u128::from(reservation.warm_load_units)
        + u128::from(demand.load_units)
        <= capacity
        && u128::from(reservation.active_memory_bytes)
            + u128::from(reservation.warm_memory_bytes)
            + u128::from(demand.memory_bytes)
            <= memory
        && u128::from(reservation.active_storage_bytes)
            + u128::from(reservation.warm_storage_bytes)
            + u128::from(demand.checkpoint_bytes)
            <= storage
        && u128::from(reservation.active_network_bytes_per_second)
            + u128::from(reservation.warm_network_bytes_per_second)
            + u128::from(demand.network_bytes_per_second)
            <= network
}

fn add_reservation(
    reservation: &mut NodeReservation,
    demand: &ShardDemand,
    warm: bool,
) -> Result<(), PlacementError> {
    if warm {
        reservation.warm_shard_count = reservation
            .warm_shard_count
            .checked_add(1)
            .ok_or(PlacementError::InvalidPlan("warm shard count overflow"))?;
        reservation.warm_load_units = reservation
            .warm_load_units
            .checked_add(demand.load_units)
            .ok_or(PlacementError::InvalidPlan(
                "warm load reservation overflow",
            ))?;
        reservation.warm_memory_bytes = reservation
            .warm_memory_bytes
            .checked_add(demand.memory_bytes)
            .ok_or(PlacementError::InvalidPlan(
                "warm memory reservation overflow",
            ))?;
        reservation.warm_storage_bytes = reservation
            .warm_storage_bytes
            .checked_add(demand.checkpoint_bytes)
            .ok_or(PlacementError::InvalidPlan(
                "warm storage reservation overflow",
            ))?;
        reservation.warm_network_bytes_per_second = reservation
            .warm_network_bytes_per_second
            .checked_add(demand.network_bytes_per_second)
            .ok_or(PlacementError::InvalidPlan(
                "warm network reservation overflow",
            ))?;
    } else {
        reservation.active_load_units = reservation
            .active_load_units
            .checked_add(demand.load_units)
            .ok_or(PlacementError::InvalidPlan("load reservation overflow"))?;
        reservation.active_memory_bytes = reservation
            .active_memory_bytes
            .checked_add(demand.memory_bytes)
            .ok_or(PlacementError::InvalidPlan("memory reservation overflow"))?;
        reservation.active_storage_bytes = reservation
            .active_storage_bytes
            .checked_add(demand.checkpoint_bytes)
            .ok_or(PlacementError::InvalidPlan("storage reservation overflow"))?;
        reservation.active_network_bytes_per_second = reservation
            .active_network_bytes_per_second
            .checked_add(demand.network_bytes_per_second)
            .ok_or(PlacementError::InvalidPlan("network reservation overflow"))?;
    }
    Ok(())
}

fn placement_score(
    resource: &ResourceObservation,
    reservation: &NodeReservation,
    demand: &ShardDemand,
    preferred: bool,
) -> u128 {
    let available = u128::from(resource.available_capacity().max(1));
    let used = u128::from(
        reservation
            .active_load_units
            .saturating_add(demand.load_units),
    );
    let headroom = available.saturating_mul(1_000_000) / used.max(1);
    let pressure = u128::from(
        u32::from(resource.cpu_pressure_milli)
            + u32::from(resource.memory_pressure_milli)
            + u32::from(resource.network_pressure_milli)
            + u32::from(resource.thermal_pressure_milli),
    );
    // A preference is a deterministic policy input, not a hard constraint.
    // It cannot outweigh the eligibility and headroom checks.
    let preference_bonus: u128 = if preferred { 1_000_000_000 } else { 0 };
    preference_bonus
        .saturating_add(headroom.saturating_mul(1_000))
        .saturating_add(4_000_000u128.saturating_sub(pressure.saturating_mul(1_000)))
}

/// Explicit stages make migration progress durable/replayable without holding
/// an async lock or embedding transport/storage behaviour in the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationStage {
    Planned,
    Reserved,
    Transferring,
    CatchingUp,
    AwaitingCutover,
    Committed,
    Aborted,
    Failed,
}

impl MigrationStage {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Committed | Self::Aborted | Self::Failed)
    }
}

/// A resumable migration operation record.  The payload transfer itself is
/// supplied by a bounded adapter; this type records only authoritative state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationOperation {
    pub operation_id: EventId,
    pub brain_id: BrainId,
    pub source_plan_digest: StateDigest,
    pub destination_plan_digest: StateDigest,
    pub source_node: String,
    pub destination_node: String,
    pub source_term: LeaseTerm,
    pub stage: MigrationStage,
    pub cut_tag: Option<LogicalTag>,
    pub destination_term: Option<LeaseTerm>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrationError {
    #[error("migration operation is already terminal")]
    Terminal,
    #[error("invalid migration stage transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: MigrationStage,
        to: MigrationStage,
    },
    #[error("migration identities are invalid")]
    InvalidIdentity,
    #[error("cutover term must advance beyond source term")]
    NonAdvancingTerm,
    #[error("cutover tag is required")]
    MissingCutTag,
    #[error("migration record is invalid: {0}")]
    InvalidRecord(&'static str),
}

impl MigrationOperation {
    pub fn new(
        operation_id: EventId,
        brain_id: BrainId,
        source_plan_digest: StateDigest,
        destination_plan_digest: StateDigest,
        source_node: impl Into<String>,
        destination_node: impl Into<String>,
        source_term: LeaseTerm,
    ) -> Result<Self, MigrationError> {
        let source_node = source_node.into();
        let destination_node = destination_node.into();
        if source_node.trim().is_empty()
            || destination_node.trim().is_empty()
            || source_node == destination_node
            || source_plan_digest == destination_plan_digest
        {
            return Err(MigrationError::InvalidIdentity);
        }
        let operation = Self {
            operation_id,
            brain_id,
            source_plan_digest,
            destination_plan_digest,
            source_node,
            destination_node,
            source_term,
            stage: MigrationStage::Planned,
            cut_tag: None,
            destination_term: None,
            error_code: None,
        };
        operation.verify()?;
        Ok(operation)
    }

    /// Validate a deserialised or recovered operation record before resuming
    /// work. This catches impossible combinations that a legal in-memory
    /// transition sequence could never produce.
    pub fn verify(&self) -> Result<(), MigrationError> {
        if self.source_node.trim().is_empty()
            || self.destination_node.trim().is_empty()
            || self.source_node == self.destination_node
            || self.source_plan_digest == self.destination_plan_digest
        {
            return Err(MigrationError::InvalidRecord("migration identities"));
        }
        if self.cut_tag.is_some_and(|tag| tag.microstep != 0) {
            return Err(MigrationError::InvalidRecord(
                "cutover tag is not a committed boundary",
            ));
        }
        if self.stage != MigrationStage::Failed && self.error_code.is_some() {
            return Err(MigrationError::InvalidRecord(
                "only failed migrations may carry an error code",
            ));
        }
        match self.stage {
            MigrationStage::Planned | MigrationStage::Reserved | MigrationStage::Transferring => {
                if self.cut_tag.is_some() || self.destination_term.is_some() {
                    return Err(MigrationError::InvalidRecord(
                        "cutover evidence precedes the cutover stage",
                    ));
                }
            }
            MigrationStage::CatchingUp => {
                if self.cut_tag.is_some() || self.destination_term.is_some() {
                    return Err(MigrationError::InvalidRecord(
                        "catch-up record contains cutover evidence",
                    ));
                }
            }
            MigrationStage::AwaitingCutover => {
                if self.cut_tag.is_none() || self.destination_term.is_some() {
                    return Err(MigrationError::InvalidRecord(
                        "cutover stage requires only a cut tag",
                    ));
                }
            }
            MigrationStage::Committed => {
                if self.cut_tag.is_none() || self.destination_term.is_none() {
                    return Err(MigrationError::InvalidRecord(
                        "committed migration requires cutover evidence",
                    ));
                }
            }
            MigrationStage::Aborted => {
                if self.destination_term.is_some() {
                    return Err(MigrationError::InvalidRecord(
                        "aborted migration cannot have a destination term",
                    ));
                }
            }
            MigrationStage::Failed => {
                if self.error_code.as_deref().is_none_or(str::is_empty) {
                    return Err(MigrationError::InvalidRecord(
                        "failed migration requires an error code",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn reserve(&mut self) -> Result<(), MigrationError> {
        self.advance(MigrationStage::Reserved)
    }

    pub fn begin_transfer(&mut self) -> Result<(), MigrationError> {
        self.advance(MigrationStage::Transferring)
    }

    pub fn begin_catch_up(&mut self) -> Result<(), MigrationError> {
        self.advance(MigrationStage::CatchingUp)
    }

    pub fn mark_ready_for_cutover(&mut self, cut_tag: LogicalTag) -> Result<(), MigrationError> {
        if self.stage != MigrationStage::CatchingUp {
            return Err(MigrationError::InvalidTransition {
                from: self.stage,
                to: MigrationStage::AwaitingCutover,
            });
        }
        if cut_tag.microstep != 0 {
            return Err(MigrationError::InvalidRecord(
                "cutover must use a committed microstep boundary",
            ));
        }
        self.cut_tag = Some(cut_tag);
        self.stage = MigrationStage::AwaitingCutover;
        self.verify()?;
        Ok(())
    }

    pub fn commit_cutover(&mut self, destination_term: LeaseTerm) -> Result<(), MigrationError> {
        if self.stage != MigrationStage::AwaitingCutover || self.cut_tag.is_none() {
            return Err(MigrationError::MissingCutTag);
        }
        if destination_term <= self.source_term {
            return Err(MigrationError::NonAdvancingTerm);
        }
        self.destination_term = Some(destination_term);
        self.stage = MigrationStage::Committed;
        self.verify()?;
        Ok(())
    }

    pub fn abort(&mut self) -> Result<(), MigrationError> {
        if self.stage.terminal() {
            return Err(MigrationError::Terminal);
        }
        self.stage = MigrationStage::Aborted;
        Ok(())
    }

    pub fn fail(&mut self, error_code: impl Into<String>) -> Result<(), MigrationError> {
        if self.stage.terminal() {
            return Err(MigrationError::Terminal);
        }
        let error_code = error_code.into();
        if error_code.trim().is_empty() {
            return Err(MigrationError::InvalidRecord(
                "failed migration requires an error code",
            ));
        }
        self.error_code = Some(error_code);
        self.stage = MigrationStage::Failed;
        self.verify()?;
        Ok(())
    }

    fn advance(&mut self, to: MigrationStage) -> Result<(), MigrationError> {
        if self.stage.terminal() {
            return Err(MigrationError::Terminal);
        }
        let valid = matches!(
            (self.stage, to),
            (MigrationStage::Planned, MigrationStage::Reserved)
                | (MigrationStage::Reserved, MigrationStage::Transferring)
                | (MigrationStage::Transferring, MigrationStage::CatchingUp)
        );
        if !valid {
            return Err(MigrationError::InvalidTransition {
                from: self.stage,
                to,
            });
        }
        self.stage = to;
        Ok(())
    }
}

/// Evidence that a node may be powered down after a graceful drain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownReadiness {
    pub brain_id: BrainId,
    pub node_id: String,
    pub plan_digest: StateDigest,
    pub checkpoint_digest: StateDigest,
    pub safe_tag: LogicalTag,
    pub active_shard_leases: u32,
    pub unacknowledged_committed_sends: u64,
    pub untransferred_output_commitments: u64,
    pub local_only_input_count: u64,
    pub control_plane_reachable: bool,
}

impl ShutdownReadiness {
    pub fn is_ready(&self) -> bool {
        !self.node_id.trim().is_empty()
            && self.active_shard_leases == 0
            && self.unacknowledged_committed_sends == 0
            && self.untransferred_output_commitments == 0
            && self.local_only_input_count == 0
            && self.control_plane_reachable
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_ready() {
            Ok(())
        } else {
            Err("node still has leases, untransferred state, local-only input, or no control plane")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brain() -> BrainId {
        BrainId::new(7).expect("test brain")
    }

    fn node(id: &str, domain: &str) -> ResourceObservation {
        ResourceObservation {
            node_id: id.to_owned(),
            device_id: format!("{id}-cpu"),
            healthy: true,
            enrolled: true,
            compute_authorised: true,
            failure_domain: domain.to_owned(),
            numerical_profiles: vec!["reference-cpu-v1".to_owned()],
            capacity_units: 100,
            reserved_capacity_units: 0,
            memory_bytes: 1_000,
            reserved_memory_bytes: 0,
            storage_bytes: 1_000,
            reserved_storage_bytes: 0,
            network_bytes_per_second: 1_000,
            reserved_network_bytes_per_second: 0,
            cpu_pressure_milli: 100,
            memory_pressure_milli: 100,
            network_pressure_milli: 100,
            thermal_pressure_milli: 100,
        }
    }

    fn demand(id: u64, component: Option<u64>) -> ShardDemand {
        ShardDemand {
            shard_id: ShardId::new(id).expect("test shard"),
            load_units: 10,
            memory_bytes: 100,
            checkpoint_bytes: 100,
            network_bytes_per_second: 10,
            zero_delay_component: component.map(|value| ComponentId::new(value).unwrap()),
            required_numerical_profile: "reference-cpu-v1".to_owned(),
            preferred_node: None,
        }
    }

    fn request(demands: Vec<ShardDemand>) -> PlacementRequest {
        PlacementRequest {
            brain_id: brain(),
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: LeaseTerm::INITIAL,
            fencing_token: LeaseTerm::INITIAL.raw(),
            effective_tag: LogicalTag::ZERO,
            demands,
            resources: vec![node("laptop", "home"), node("worker", "rack-a")],
            constraints: PlacementConstraints::default(),
            intent: PlacementIntent::Automatic,
        }
    }

    #[test]
    fn placement_is_deterministic_and_digest_verified() {
        let planner = PlacementPlanner;
        let first = planner
            .plan(request(vec![demand(1, Some(1)), demand(2, None)]))
            .unwrap();
        let second = planner
            .plan(request(vec![demand(2, None), demand(1, Some(1))]))
            .unwrap();
        assert_eq!(first, second);
        first.verify().unwrap();
    }

    #[test]
    fn consolidation_requires_one_host_and_can_explicitly_degrade() {
        let planner = PlacementPlanner;
        let mut request = request(vec![demand(1, None)]);
        request.intent = PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        };
        assert!(matches!(
            planner.plan(request.clone()),
            Err(PlacementError::NoEligibleNode { .. })
        ));
        request.constraints.allow_single_host_degraded_durability = true;
        let plan = planner.plan(request).unwrap();
        assert!(plan.decision.degraded_durability);
        assert_eq!(plan.placements[0].active_node, "laptop");
    }

    #[test]
    fn unsafe_scc_does_not_split_or_silently_fallback() {
        let planner = PlacementPlanner;
        let mut request = request(vec![demand(1, Some(9)), demand(2, Some(9))]);
        request.resources[0].capacity_units = 30;
        request.resources[1].capacity_units = 30;
        request.constraints.minimum_warm_replicas = 0;
        let plan = planner.plan(request).unwrap();
        assert_eq!(
            plan.placements[0].active_node,
            plan.placements[1].active_node
        );
    }

    #[test]
    fn migration_requires_catchup_cut_and_new_term() {
        let mut operation = MigrationOperation::new(
            EventId::new(1).unwrap(),
            brain(),
            StateDigest([1; 16]),
            StateDigest([2; 16]),
            "laptop",
            "worker",
            LeaseTerm::INITIAL,
        )
        .unwrap();
        operation.reserve().unwrap();
        operation.begin_transfer().unwrap();
        operation.begin_catch_up().unwrap();
        operation
            .mark_ready_for_cutover(LogicalTag::new(4, 0))
            .unwrap();
        assert!(matches!(
            operation.commit_cutover(LeaseTerm::INITIAL),
            Err(MigrationError::NonAdvancingTerm)
        ));
        operation
            .commit_cutover(LeaseTerm::new(2).unwrap())
            .unwrap();
        assert_eq!(operation.stage, MigrationStage::Committed);
    }

    #[test]
    fn repartition_requires_complete_lineage_and_new_partition_generation() {
        let plan = RepartitionPlan::new(
            EventId::new(9).unwrap(),
            brain(),
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            PartitionGeneration::new(2).unwrap(),
            LogicalTag::ZERO,
            vec![ShardId::new(2).unwrap(), ShardId::new(1).unwrap()],
            vec![ShardId::new(3).unwrap()],
            vec![
                ShardLineage {
                    source_shard: ShardId::new(2).unwrap(),
                    successor_shards: vec![ShardId::new(3).unwrap()],
                },
                ShardLineage {
                    source_shard: ShardId::new(1).unwrap(),
                    successor_shards: vec![ShardId::new(3).unwrap()],
                },
            ],
        )
        .unwrap();
        plan.verify().unwrap();
        assert_eq!(plan.source_shards[0], ShardId::new(1).unwrap());
        assert_eq!(plan.target_shards, vec![ShardId::new(3).unwrap()]);
    }

    #[test]
    fn duplicate_demand_is_rejected_before_capacity_planning() {
        let duplicate = demand(1, None);
        assert!(matches!(
            PlacementPlanner.plan(request(vec![duplicate.clone(), duplicate])),
            Err(PlacementError::DuplicateShard { .. })
        ));
    }

    #[test]
    fn checkpoint_storage_is_a_hard_admission_constraint() {
        let mut request = request(vec![demand(1, None)]);
        request.constraints.minimum_warm_replicas = 0;
        request.demands[0].checkpoint_bytes = 1_000;
        assert!(matches!(
            PlacementPlanner.plan(request),
            Err(PlacementError::NoEligibleNode { .. })
        ));
    }

    #[test]
    fn command_envelope_is_idempotent_and_digest_protected() {
        let command = PlacementCommand::new(
            "request-1",
            "placement-1",
            "operator",
            brain(),
            7,
            LeaseTerm::INITIAL,
            PlacementCommandKind::PlanPlacement(request(vec![demand(1, None)])),
        )
        .unwrap();
        command.verify().unwrap();
        let mut tampered = command.clone();
        tampered.expected_resource_version += 1;
        assert!(matches!(
            tampered.verify(),
            Err(PlacementCommandError::DigestMismatch)
        ));
    }

    #[test]
    fn shutdown_readiness_rejects_local_state_and_accepts_clean_drain() {
        let mut ready = ShutdownReadiness {
            brain_id: brain(),
            node_id: "laptop".to_owned(),
            plan_digest: StateDigest([1; 16]),
            checkpoint_digest: StateDigest([2; 16]),
            safe_tag: LogicalTag::ZERO,
            active_shard_leases: 1,
            unacknowledged_committed_sends: 0,
            untransferred_output_commitments: 0,
            local_only_input_count: 0,
            control_plane_reachable: true,
        };
        assert!(ready.validate().is_err());
        ready.active_shard_leases = 0;
        assert!(ready.validate().is_ok());
    }
}
