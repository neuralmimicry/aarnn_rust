//! Runtime ownership boundary for stable virtual shards.
//!
//! The placement planner produces a value describing where a shard should
//! run.  That value is not sufficient to admit biological work: the process
//! must also prove that it owns the same shard, generations, lease term and
//! fencing token.  This module is the small, transport-neutral directory used
//! by the live node loop to make that distinction explicit.
//!
//! The directory deliberately does not contain a neural-kernel implementation
//! or an async lock.  It can therefore be used by the orchestrator, a server
//! worker, a workstation rehearsal, and deterministic fault tests.  A runtime
//! adapter calls [`ManagedShardRuntime::admit`] immediately before a step and
//! [`ManagedShardRuntime::commit`] after the authoritative durable boundary.
//! A compatibility `Runner` may be retained as a projection, but it cannot
//! pass this gate without matching authoritative evidence.

use crate::deterministic::{
    BrainId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest, TopologyGeneration,
};
use crate::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlan, PlacementPlanner, PlacementRequest,
    ResourceObservation, ShardDemand, ShardPlacement,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MANAGED_SHARD_RUNTIME_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeShardEvidence {
    pub shard_id: ShardId,
    pub brain_id: BrainId,
    pub node_id: String,
    pub device_id: String,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub authoritative_state_digest: StateDigest,
}

impl RuntimeShardEvidence {
    pub fn verify(&self) -> Result<(), RuntimeDirectoryError> {
        if self.node_id.trim().is_empty()
            || self.device_id.trim().is_empty()
            || self.lease_term.raw() == 0
            || self.fencing_token == 0
            || self.authoritative_state_digest == StateDigest([0; 16])
        {
            return Err(RuntimeDirectoryError::InvalidEvidence(self.shard_id));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePlacementGeneration {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub resource_version: u64,
    pub plan_digest: StateDigest,
    pub topology_generation: TopologyGeneration,
    pub partition_generation: PartitionGeneration,
    pub effective_tag: LogicalTag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedShardRuntime {
    pub evidence: RuntimeShardEvidence,
    pub generation: Option<RuntimePlacementGeneration>,
    pub committed_tag: LogicalTag,
    pub committed_state_digest: StateDigest,
}

impl ManagedShardRuntime {
    pub fn new(evidence: RuntimeShardEvidence) -> Result<Self, RuntimeDirectoryError> {
        evidence.verify()?;
        Ok(Self {
            evidence,
            generation: None,
            committed_tag: LogicalTag::ZERO,
            committed_state_digest: StateDigest([0; 16]),
        })
    }

    /// Build the smallest valid placement for a durable single-shard owner.
    /// This is used only while a network is still on the compatibility path;
    /// multi-shard plans must come from the orchestrator planner and registry.
    pub fn single_owner_plan(
        evidence: &RuntimeShardEvidence,
        effective_tag: LogicalTag,
    ) -> Result<PlacementPlan, RuntimeDirectoryError> {
        let request = PlacementRequest {
            brain_id: evidence.brain_id,
            topology_generation: evidence.topology_generation,
            partition_generation: evidence.partition_generation,
            lease_term: evidence.lease_term,
            fencing_token: evidence.fencing_token,
            effective_tag,
            demands: vec![ShardDemand {
                shard_id: evidence.shard_id,
                load_units: 100,
                memory_bytes: 100,
                checkpoint_bytes: 100,
                network_bytes_per_second: 100,
                zero_delay_component: None,
                required_numerical_profile: "reference-fixed-point-v1".to_owned(),
                preferred_node: Some(evidence.node_id.clone()),
            }],
            resources: vec![ResourceObservation {
                node_id: evidence.node_id.clone(),
                device_id: evidence.device_id.clone(),
                healthy: true,
                enrolled: true,
                compute_authorised: true,
                failure_domain: evidence.node_id.clone(),
                numerical_profiles: vec!["reference-fixed-point-v1".to_owned()],
                capacity_units: 1_000,
                reserved_capacity_units: 0,
                memory_bytes: 1_000,
                reserved_memory_bytes: 0,
                storage_bytes: 1_000,
                reserved_storage_bytes: 0,
                network_bytes_per_second: 1_000,
                reserved_network_bytes_per_second: 0,
                cpu_pressure_milli: 0,
                memory_pressure_milli: 0,
                network_pressure_milli: 0,
                thermal_pressure_milli: 0,
            }],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                allow_single_host_degraded_durability: true,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: evidence.node_id.clone(),
            },
        };
        PlacementPlanner
            .plan(request)
            .map_err(|error| RuntimeDirectoryError::InvalidPlan(error.to_string()))
    }

    pub fn adopt_generation(
        &mut self,
        plan: &PlacementPlan,
        resource_version: u64,
    ) -> Result<(), RuntimeDirectoryError> {
        plan.verify()
            .map_err(|error| RuntimeDirectoryError::InvalidPlan(error.to_string()))?;
        let Some(placement) = plan
            .placements
            .iter()
            .find(|placement| placement.shard_id == self.evidence.shard_id)
        else {
            return Err(RuntimeDirectoryError::MissingEvidence(
                self.evidence.shard_id,
            ));
        };
        if !placement_matches_evidence(plan, placement, &self.evidence) {
            return Err(RuntimeDirectoryError::EvidenceMismatch(
                self.evidence.shard_id,
            ));
        }
        self.generation = Some(RuntimePlacementGeneration {
            schema_version: MANAGED_SHARD_RUNTIME_SCHEMA_VERSION,
            brain_id: plan.brain_id,
            resource_version,
            plan_digest: plan.digest(),
            topology_generation: plan.topology_generation,
            partition_generation: plan.partition_generation,
            effective_tag: plan.effective_tag,
        });
        Ok(())
    }

    /// Check the complete writer identity before a biological transition.
    pub fn admit(
        &self,
        brain_id: BrainId,
        shard_id: ShardId,
        topology_generation: TopologyGeneration,
        partition_generation: PartitionGeneration,
        lease_term: LeaseTerm,
        fencing_token: u64,
    ) -> Result<(), RuntimeAdmissionError> {
        if self.evidence.brain_id != brain_id
            || self.evidence.shard_id != shard_id
            || self.evidence.topology_generation != topology_generation
            || self.evidence.partition_generation != partition_generation
            || self.evidence.lease_term != lease_term
            || self.evidence.fencing_token != fencing_token
        {
            return Err(RuntimeAdmissionError::StaleWriter {
                shard_id,
                expected_term: self.evidence.lease_term,
                received_term: lease_term,
            });
        }
        let Some(generation) = &self.generation else {
            return Err(RuntimeAdmissionError::PlanNotAdopted(shard_id));
        };
        if generation.topology_generation != topology_generation
            || generation.partition_generation != partition_generation
        {
            return Err(RuntimeAdmissionError::GenerationMismatch(shard_id));
        }
        Ok(())
    }

    pub fn commit(
        &mut self,
        tag: LogicalTag,
        state_digest: StateDigest,
    ) -> Result<(), RuntimeAdmissionError> {
        if tag < self.committed_tag || tag.microstep != 0 || state_digest == StateDigest([0; 16]) {
            return Err(RuntimeAdmissionError::InvalidCommit(self.evidence.shard_id));
        }
        self.committed_tag = tag;
        self.committed_state_digest = state_digest;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedShardRuntimeDirectory {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub resource_version: u64,
    pub active_generation: Option<RuntimePlacementGeneration>,
    pub runtimes: BTreeMap<ShardId, ManagedShardRuntime>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RuntimeDirectoryError {
    #[error("runtime directory schema is unsupported")]
    UnsupportedSchema,
    #[error("runtime directory brain identity does not match")]
    BrainMismatch,
    #[error("runtime directory resource version conflict: expected {expected}, current {current}")]
    VersionConflict { expected: u64, current: u64 },
    #[error("runtime evidence for shard {0} is invalid")]
    InvalidEvidence(ShardId),
    #[error("runtime evidence for shard {0} is missing")]
    MissingEvidence(ShardId),
    #[error("runtime evidence for shard {0} does not match its placement")]
    EvidenceMismatch(ShardId),
    #[error("runtime plan is invalid: {0}")]
    InvalidPlan(String),
    #[error("runtime shard {0} is not registered")]
    UnknownShard(ShardId),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RuntimeAdmissionError {
    #[error("shard {0} has no adopted placement generation")]
    PlanNotAdopted(ShardId),
    #[error(
        "shard {shard_id} writer evidence is stale (expected term {expected_term}, received {received_term})"
    )]
    StaleWriter {
        shard_id: ShardId,
        expected_term: LeaseTerm,
        received_term: LeaseTerm,
    },
    #[error("shard {0} placement generations do not match")]
    GenerationMismatch(ShardId),
    #[error("shard {0} commit evidence is invalid")]
    InvalidCommit(ShardId),
}

impl ManagedShardRuntimeDirectory {
    pub fn new(brain_id: BrainId) -> Self {
        Self {
            schema_version: MANAGED_SHARD_RUNTIME_SCHEMA_VERSION,
            brain_id,
            resource_version: 0,
            active_generation: None,
            runtimes: BTreeMap::new(),
        }
    }

    /// Register or replace the process-local evidence for one shard. This is
    /// intentionally separate from plan adoption: observing a worker does not
    /// grant it authority.
    pub fn register(&mut self, runtime: ManagedShardRuntime) -> Result<(), RuntimeDirectoryError> {
        if runtime.evidence.brain_id != self.brain_id {
            return Err(RuntimeDirectoryError::BrainMismatch);
        }
        self.runtimes.insert(runtime.evidence.shard_id, runtime);
        Ok(())
    }

    /// Atomically adopt a placement generation after every active placement
    /// has supplied matching authoritative evidence. No partial generation is
    /// visible when one shard is missing, stale, or owned by another device.
    pub fn adopt_plan(
        &mut self,
        plan: &PlacementPlan,
        expected_resource_version: u64,
        evidence: &BTreeMap<ShardId, RuntimeShardEvidence>,
    ) -> Result<RuntimePlacementGeneration, RuntimeDirectoryError> {
        if self.resource_version != expected_resource_version {
            return Err(RuntimeDirectoryError::VersionConflict {
                expected: expected_resource_version,
                current: self.resource_version,
            });
        }
        plan.verify()
            .map_err(|error| RuntimeDirectoryError::InvalidPlan(error.to_string()))?;
        if plan.brain_id != self.brain_id {
            return Err(RuntimeDirectoryError::BrainMismatch);
        }

        for placement in &plan.placements {
            let Some(candidate) = evidence.get(&placement.shard_id) else {
                return Err(RuntimeDirectoryError::MissingEvidence(placement.shard_id));
            };
            candidate.verify()?;
            if !placement_matches_evidence(plan, placement, candidate) {
                return Err(RuntimeDirectoryError::EvidenceMismatch(placement.shard_id));
            }
        }

        let next_version =
            self.resource_version
                .checked_add(1)
                .ok_or(RuntimeDirectoryError::InvalidPlan(
                    "resource version exhausted".to_owned(),
                ))?;
        let generation = RuntimePlacementGeneration {
            schema_version: MANAGED_SHARD_RUNTIME_SCHEMA_VERSION,
            brain_id: plan.brain_id,
            resource_version: next_version,
            plan_digest: plan.digest(),
            topology_generation: plan.topology_generation,
            partition_generation: plan.partition_generation,
            effective_tag: plan.effective_tag,
        };
        let mut next_runtimes = self.runtimes.clone();
        for placement in &plan.placements {
            let runtime = next_runtimes
                .get_mut(&placement.shard_id)
                .ok_or(RuntimeDirectoryError::UnknownShard(placement.shard_id))?;
            runtime.evidence = evidence[&placement.shard_id].clone();
            runtime.generation = Some(generation.clone());
        }
        self.runtimes = next_runtimes;
        self.resource_version = next_version;
        self.active_generation = Some(generation.clone());
        Ok(generation)
    }

    pub fn runtime(&self, shard_id: ShardId) -> Option<&ManagedShardRuntime> {
        self.runtimes.get(&shard_id)
    }

    pub fn verify(&self) -> Result<(), RuntimeDirectoryError> {
        if self.schema_version != MANAGED_SHARD_RUNTIME_SCHEMA_VERSION {
            return Err(RuntimeDirectoryError::UnsupportedSchema);
        }
        if let Some(generation) = &self.active_generation {
            if generation.brain_id != self.brain_id
                || generation.resource_version > self.resource_version
            {
                return Err(RuntimeDirectoryError::InvalidPlan(
                    "active generation is inconsistent".to_owned(),
                ));
            }
        }
        for runtime in self.runtimes.values() {
            runtime.evidence.verify()?;
            if runtime.evidence.brain_id != self.brain_id {
                return Err(RuntimeDirectoryError::BrainMismatch);
            }
        }
        Ok(())
    }
}

fn placement_matches_evidence(
    plan: &PlacementPlan,
    placement: &ShardPlacement,
    evidence: &RuntimeShardEvidence,
) -> bool {
    evidence.brain_id == plan.brain_id
        && evidence.shard_id == placement.shard_id
        && evidence.node_id == placement.active_node
        && evidence.device_id == placement.active_device
        && evidence.topology_generation == plan.topology_generation
        && evidence.partition_generation == plan.partition_generation
        && evidence.lease_term == plan.lease_term
        && evidence.fencing_token == plan.fencing_token
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> PlacementPlan {
        ManagedShardRuntime::single_owner_plan(&evidence(), LogicalTag::ZERO).unwrap()
    }

    fn evidence() -> RuntimeShardEvidence {
        RuntimeShardEvidence {
            shard_id: ShardId::new(1).unwrap(),
            brain_id: BrainId::new(7).unwrap(),
            node_id: "node-a".to_owned(),
            device_id: "cpu".to_owned(),
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: LeaseTerm::INITIAL,
            fencing_token: 1,
            authoritative_state_digest: StateDigest([9; 16]),
        }
    }

    #[test]
    fn plan_adoption_is_atomic_and_requires_matching_evidence() {
        let plan = plan();
        let mut directory = ManagedShardRuntimeDirectory::new(plan.brain_id);
        let mut runtime = ManagedShardRuntime::new(evidence()).unwrap();
        assert!(matches!(
            runtime.admit(
                plan.brain_id,
                ShardId::new(1).unwrap(),
                plan.topology_generation,
                plan.partition_generation,
                plan.lease_term,
                plan.fencing_token,
            ),
            Err(RuntimeAdmissionError::PlanNotAdopted(_))
        ));
        runtime.adopt_generation(&plan, 0).unwrap();
        runtime
            .admit(
                plan.brain_id,
                ShardId::new(1).unwrap(),
                plan.topology_generation,
                plan.partition_generation,
                plan.lease_term,
                plan.fencing_token,
            )
            .unwrap();
        runtime
            .commit(LogicalTag::ZERO, StateDigest([5; 16]))
            .unwrap();
        directory.register(runtime).unwrap();
        let mut evidence_map = BTreeMap::new();
        evidence_map.insert(ShardId::new(1).unwrap(), evidence());
        directory.adopt_plan(&plan, 0, &evidence_map).unwrap();
        assert_eq!(
            directory
                .active_generation
                .as_ref()
                .unwrap()
                .resource_version,
            1
        );
        assert!(
            directory
                .runtime(ShardId::new(1).unwrap())
                .unwrap()
                .generation
                .is_some()
        );
    }

    #[test]
    fn stale_evidence_does_not_partially_replace_generation() {
        let plan = plan();
        let mut directory = ManagedShardRuntimeDirectory::new(plan.brain_id);
        directory
            .register(ManagedShardRuntime::new(evidence()).unwrap())
            .unwrap();
        let before = directory.clone();
        let mut stale = evidence();
        stale.node_id = "node-b".to_owned();
        let mut map = BTreeMap::new();
        map.insert(stale.shard_id, stale);
        assert!(matches!(
            directory.adopt_plan(&plan, 0, &map),
            Err(RuntimeDirectoryError::EvidenceMismatch(_))
        ));
        assert_eq!(directory, before);
    }
}
