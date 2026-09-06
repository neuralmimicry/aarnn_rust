//! Bounded automatic placement reconciliation for the live stable executor.
//!
//! This adapter composes the deterministic planner, the placement controller,
//! the crash safe registry and the partial worker manifest factory.  It does
//! not inspect discovery advertisements, dense `Runner` state or untrusted
//! telemetry.  Callers must provide an authoritative runtime manifest,
//! versioned shard demands, and resource observations that have already
//! crossed the node enrolment and compute-authorisation boundary.
//!
//! The coordinator is deliberately synchronous.  Planning and manifest
//! construction are bounded CPU work and can run in `spawn_blocking`; worker
//! dispatch is returned as durable commands so the caller can send them on a
//! non-blocking control-plane task without holding placement locks.

use crate::deterministic::{LogicalTag, ShardId};
use crate::placement::{
    PlacementConstraints, PlacementError, PlacementIntent, PlacementPlan, PlacementPlanner,
    PlacementRequest, ResourceObservation, ShardDemand,
};
use crate::placement_controller::{
    AutomaticPlacementPolicy, PlacementController, PlacementControllerError, PlacementReview,
};
use crate::placement_registry::{
    CutoverEvidence, PersistedPlacementRegistry, PlacementActivationState, PlacementApplyReceipt,
    PlacementApplyRequest, PlacementRegistryError,
};
use crate::stable_runtime_bootstrap::{
    StablePartialWorkerBootstrapManifest, StableRuntimeBootstrapError,
    StableRuntimeBootstrapManifest, StableWorkerEndpoint,
};
use crate::stable_worker::{
    StableWorkerActivationCommand, StableWorkerCheckpointTransferReference,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const PLACEMENT_AUTOMATION_SCHEMA_VERSION: u32 = 1;
pub const MAX_AUTOMATION_SPEC_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_AUTOMATION_NODES: usize = 256;
pub const MAX_AUTOMATION_DEMANDS: usize = 1_000_000;

/// Deployment controlled inputs for one automatically reconciled brain.
///
/// The runtime manifest is the authoritative biological/checkpoint catalogue.
/// `demands` and `source_nodes` are explicit control-plane data; they are not
/// inferred from a node's resource report. `allowed_nodes` is an operator
/// compute grant and is intersected with every observed resource set before
/// the planner is called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementAutomationSpec {
    pub schema_version: u32,
    pub network_id: String,
    pub runtime: StableRuntimeBootstrapManifest,
    pub demands: Vec<ShardDemand>,
    pub constraints: PlacementConstraints,
    pub allowed_nodes: BTreeSet<String>,
    pub failure_domains: BTreeMap<String, String>,
    /// Explicit source identities accepted by a partial worker receiver.
    /// This is kept separate from discovery and endpoint observations.
    pub source_nodes: BTreeSet<String>,
    /// Explicitly enrolled peer addresses used to build worker dispatch
    /// manifests. Keys are stable node IDs, never user supplied URLs alone.
    pub endpoint_addresses: BTreeMap<String, String>,
    /// Target-local immutable checkpoint receipts produced by the checkpoint
    /// transfer adapter.  A receipt contains no source filesystem path; the
    /// target resolves it below its own configured transfer root.  The map is
    /// optional for same-host/reference activation, but every supplied entry
    /// is checked against the runtime brain, checkpoint, partition and source
    /// plan before it can reach a worker command.
    #[serde(default)]
    pub checkpoint_transfers: BTreeMap<String, StableWorkerCheckpointTransferReference>,
    pub worker_state_root: PathBuf,
    /// Capacity inventory not exposed by the generic heartbeat contract.
    /// These values are deployment facts and therefore must be configured,
    /// rather than guessed from discovery or a telemetry sample.
    pub storage_bytes_per_node: u64,
    pub network_bytes_per_second_per_node: u64,
    pub max_pending_outbound: usize,
    pub max_outbound_per_step: usize,
}

impl PlacementAutomationSpec {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PlacementAutomationError> {
        let bytes = std::fs::read(path.as_ref())
            .map_err(|error| PlacementAutomationError::Io(error.to_string()))?;
        if bytes.len() > MAX_AUTOMATION_SPEC_BYTES {
            return Err(PlacementAutomationError::SpecTooLarge);
        }
        let spec: Self = serde_json::from_slice(&bytes)
            .map_err(|error| PlacementAutomationError::Encoding(error.to_string()))?;
        spec.validate()
    }

    pub fn validate(self) -> Result<Self, PlacementAutomationError> {
        if self.schema_version != PLACEMENT_AUTOMATION_SCHEMA_VERSION {
            return Err(PlacementAutomationError::Invalid(
                "unsupported placement automation schema",
            ));
        }
        if self.network_id.trim().is_empty()
            || self.network_id.len() > 256
            || self.allowed_nodes.is_empty()
            || self.allowed_nodes.len() > MAX_AUTOMATION_NODES
            || self.source_nodes.is_empty()
            || self.worker_state_root.as_os_str().is_empty()
            || self.storage_bytes_per_node == 0
            || self.network_bytes_per_second_per_node == 0
            || self.max_pending_outbound == 0
            || self.max_outbound_per_step == 0
        {
            return Err(PlacementAutomationError::Invalid(
                "network, node grants, source grants, state root, or worker bounds are invalid",
            ));
        }
        if self.demands.is_empty() || self.demands.len() > MAX_AUTOMATION_DEMANDS {
            return Err(PlacementAutomationError::Invalid(
                "bounded shard demands are required",
            ));
        }
        if self.allowed_nodes.iter().any(|node| node.trim().is_empty())
            || self.source_nodes.iter().any(|node| node.trim().is_empty())
        {
            return Err(PlacementAutomationError::Invalid(
                "node identities must not be empty",
            ));
        }
        for (node, address) in &self.endpoint_addresses {
            if node.trim().is_empty()
                || node.len() > 256
                || address.len() > 2048
                || !(address.starts_with("http://") || address.starts_with("https://"))
            {
                return Err(PlacementAutomationError::Invalid(
                    "worker endpoint identity or address is invalid",
                ));
            }
        }
        for (node, reference) in &self.checkpoint_transfers {
            if !self.allowed_nodes.contains(node) {
                return Err(PlacementAutomationError::Invalid(
                    "checkpoint transfer target is outside the compute grant",
                ));
            }
            reference.validate().map_err(|_| {
                PlacementAutomationError::Invalid("checkpoint transfer reference is invalid")
            })?;
            if reference.brain_id != self.runtime.brain_id.raw()
                || reference.checkpoint_id != self.runtime.checkpoint_id.raw()
                || reference.partition_generation != self.runtime.partition_generation.raw()
                || reference.plan_digest != self.runtime.plan_digest.to_string()
            {
                return Err(PlacementAutomationError::Invalid(
                    "checkpoint transfer reference does not match the runtime checkpoint",
                ));
            }
        }
        if self.allowed_nodes.iter().any(|node| {
            self.failure_domains
                .get(node)
                .is_none_or(|domain| domain.trim().is_empty())
        }) {
            return Err(PlacementAutomationError::Invalid(
                "every authorised node requires an explicit failure domain",
            ));
        }
        let (_, compiled) = self
            .runtime
            .compile_plan()
            .map_err(PlacementAutomationError::Runtime)?;
        if self.runtime.brain_id != crate::managed_durability::managed_brain_id(&self.network_id) {
            return Err(PlacementAutomationError::Invalid(
                "runtime brain identity does not match the managed network identity",
            ));
        }
        let plan_shards = compiled.shard_ids().collect::<BTreeSet<_>>();
        let demand_shards = self
            .demands
            .iter()
            .map(|demand| demand.shard_id)
            .collect::<BTreeSet<_>>();
        if plan_shards != demand_shards {
            return Err(PlacementAutomationError::Invalid(
                "demands must cover exactly the immutable runtime shard set",
            ));
        }
        if self
            .demands
            .iter()
            .any(|demand| demand.required_numerical_profile.trim().is_empty())
        {
            return Err(PlacementAutomationError::Invalid(
                "every demand requires a numerical profile",
            ));
        }
        Ok(self)
    }
}

/// A command is returned only after its placement publication and activation
/// journal entry have been durably written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementActivationDispatch {
    pub activation_idempotency_key: String,
    pub target_node: String,
    pub command: StableWorkerActivationCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementReconcileOutcome {
    NoChange {
        plan: PlacementPlan,
        review: PlacementReview,
    },
    Applied {
        receipt: PlacementApplyReceipt,
        plan: PlacementPlan,
        review: PlacementReview,
        activations: Vec<PlacementActivationDispatch>,
    },
}

#[derive(Debug, Error)]
pub enum PlacementAutomationError {
    #[error("placement automation specification is too large")]
    SpecTooLarge,
    #[error("placement automation I/O failed: {0}")]
    Io(String),
    #[error("placement automation encoding failed: {0}")]
    Encoding(String),
    #[error("placement automation specification is invalid: {0}")]
    Invalid(&'static str),
    #[error("placement automation runtime manifest is invalid: {0}")]
    Runtime(#[from] StableRuntimeBootstrapError),
    #[error("placement planner rejected the proposal: {0}")]
    Planner(#[from] PlacementError),
    #[error("placement controller rejected the proposal: {0}")]
    Controller(#[from] PlacementControllerError),
    #[error("placement registry rejected the publication: {0}")]
    Registry(#[from] PlacementRegistryError),
    #[error("activation command construction failed: {0}")]
    Activation(StableRuntimeBootstrapError),
    #[error("observed resources contain no authorised eligible nodes")]
    NoEligibleResources,
}

/// Stateful automatic reconciler for one brain. Construct one per persisted
/// registry; sharing it between unrelated brains would mix fences and
/// residence timers.
pub struct PlacementAutomationCoordinator {
    spec: PlacementAutomationSpec,
    planner: PlacementPlanner,
    controller: PlacementController,
    registry: PersistedPlacementRegistry,
    next_operation_id: u64,
}

impl PlacementAutomationCoordinator {
    pub fn open(
        spec: PlacementAutomationSpec,
        registry_path: impl Into<PathBuf>,
        policy: AutomaticPlacementPolicy,
    ) -> Result<Self, PlacementAutomationError> {
        let spec = spec.validate()?;
        let registry = PersistedPlacementRegistry::open(
            registry_path,
            spec.runtime.brain_id,
            spec.runtime.lease_term,
        )?;
        let mut controller = PlacementController::new(policy)?;
        if let Some(plan) = registry.state().active_plan.clone() {
            controller.adopt(plan)?;
        }
        Ok(Self {
            spec,
            planner: PlacementPlanner,
            controller,
            registry,
            next_operation_id: 1,
        })
    }

    pub fn spec(&self) -> &PlacementAutomationSpec {
        &self.spec
    }

    pub fn registry(&self) -> &PersistedPlacementRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut PersistedPlacementRegistry {
        &mut self.registry
    }

    /// Reconstruct the bounded activation commands retained by the registry
    /// after a process restart. The manifest is decoded and the command's
    /// placement binding is revalidated before it is returned for dispatch.
    pub fn retryable_activation_dispatches(
        &self,
    ) -> Result<Vec<PlacementActivationDispatch>, PlacementAutomationError> {
        let mut dispatches = Vec::new();
        for (activation_idempotency_key, status) in
            self.registry.state().retryable_activation_commands()
        {
            let command: StableWorkerActivationCommand =
                serde_json::from_str(&status.activation_command_json)
                    .map_err(|error| PlacementAutomationError::Encoding(error.to_string()))?;
            command.verify().map_err(|error| {
                PlacementAutomationError::Invalid(match error {
                    crate::stable_worker::StableWorkerActivationError::UnsupportedSchema => {
                        "durable activation schema is unsupported"
                    }
                    crate::stable_worker::StableWorkerActivationError::InvalidField(_) => {
                        "durable activation contains an invalid field"
                    }
                    crate::stable_worker::StableWorkerActivationError::ManifestTooLarge => {
                        "durable activation manifest is too large"
                    }
                    crate::stable_worker::StableWorkerActivationError::DigestMismatch => {
                        "durable activation manifest digest is invalid"
                    }
                    crate::stable_worker::StableWorkerActivationError::InvalidManifest => {
                        "durable activation manifest is invalid"
                    }
                })
            })?;
            if status.placement_idempotency_key.trim().is_empty()
                || command.placement_idempotency_key != status.placement_idempotency_key
            {
                return Err(PlacementAutomationError::Invalid(
                    "durable activation placement binding is invalid",
                ));
            }
            dispatches.push(PlacementActivationDispatch {
                activation_idempotency_key,
                target_node: command.target_node.clone(),
                command,
            });
        }
        Ok(dispatches)
    }

    pub fn controller(&self) -> &PlacementController {
        &self.controller
    }

    /// Reconcile one bounded resource snapshot. The caller must provide a
    /// cutover proof for any movement after the initial publication. This is
    /// the safety stop that prevents automation from declaring migration
    /// complete merely because a new node is visible.
    pub fn reconcile(
        &mut self,
        resources: Vec<ResourceObservation>,
        now: LogicalTag,
        intent: PlacementIntent,
        active_migrations: u16,
        cutover: Option<CutoverEvidence>,
    ) -> Result<PlacementReconcileOutcome, PlacementAutomationError> {
        let resources = self.authorised_resources(resources)?;
        let request = PlacementRequest {
            brain_id: self.spec.runtime.brain_id,
            topology_generation: self.spec.runtime.topology_generation,
            partition_generation: self.spec.runtime.partition_generation,
            lease_term: self.spec.runtime.lease_term,
            fencing_token: self.spec.runtime.fencing_token,
            effective_tag: now,
            demands: self.spec.demands.clone(),
            resources,
            constraints: self.spec.constraints.clone(),
            intent,
        };
        let plan = self
            .planner
            .plan(request.clone())
            .map_err(PlacementAutomationError::Planner)?;

        if let Some(prepared) = self.registry.state().prepared_plan() {
            if prepared.digest() != plan.digest() {
                return Err(PlacementAutomationError::Controller(
                    PlacementControllerError::Blocked(
                        "a previous placement is awaiting target activation evidence".to_owned(),
                    ),
                ));
            }
        }

        if let Some(current) = self.registry.state().active_plan.as_ref() {
            if self
                .controller
                .active_plan
                .as_ref()
                .map(PlacementPlan::digest)
                != Some(current.digest())
            {
                self.controller.adopt(current.clone())?;
            }
        }
        let demands = request
            .demands
            .iter()
            .cloned()
            .map(|demand| (demand.shard_id, demand))
            .collect::<BTreeMap<_, _>>();
        let review =
            self.controller
                .review(&plan, &demands, &request.resources, now, active_migrations)?;
        if !review.requires_migration {
            if self.registry.state().active_plan.is_some() {
                return Ok(PlacementReconcileOutcome::NoChange { plan, review });
            }
        } else if cutover.is_none() {
            return Err(PlacementAutomationError::Controller(
                PlacementControllerError::Blocked(
                    "automatic movement requires checkpoint and cutover evidence".to_owned(),
                ),
            ));
        }

        let placement_key = format!("auto:{}:{}", self.spec.network_id, plan.digest());
        let request_id = format!("{placement_key}:publish");
        let apply = PlacementApplyRequest {
            request_id: request_id.clone(),
            idempotency_key: placement_key.clone(),
            expected_resource_version: self.registry.state().resource_version,
            observed_leader_term: self.registry.state().leader_term,
            plan: plan.clone(),
            cutover,
            repartition: None,
        };

        // Advance a clone first. If the registry rejects the proposal the
        // in-memory controller remains at the previous authoritative plan.
        let mut next_controller = self.controller.clone();
        if review.requires_migration {
            next_controller.commit(plan.clone(), &review, now)?;
        } else {
            next_controller.record_committed(plan.clone(), now)?;
        }
        let activations = self.activation_dispatches(&plan, &placement_key)?;
        // A target activation is part of the publication barrier.  Keep the
        // old authoritative placement visible until every target presents a
        // validated registration.
        let receipt = if activations.is_empty() {
            let receipt = self.registry.apply(apply)?;
            self.controller = next_controller;
            receipt
        } else {
            self.registry.prepare(apply)?
        };
        for dispatch in &activations {
            let json = serde_json::to_string(&dispatch.command)
                .map_err(|error| PlacementAutomationError::Encoding(error.to_string()))?;
            self.registry.record_activation_dispatch_status(
                placement_key.clone(),
                dispatch.activation_idempotency_key.clone(),
                receipt.request_id.clone(),
                receipt.plan_digest,
                PlacementActivationState::Pending,
                "",
                json,
            )?;
        }
        Ok(PlacementReconcileOutcome::Applied {
            receipt,
            plan,
            review,
            activations,
        })
    }

    fn authorised_resources(
        &self,
        resources: Vec<ResourceObservation>,
    ) -> Result<Vec<ResourceObservation>, PlacementAutomationError> {
        let mut seen = BTreeSet::new();
        let mut selected = Vec::new();
        for resource in resources {
            if !self.spec.allowed_nodes.contains(&resource.node_id)
                || !resource.enrolled
                || !resource.compute_authorised
                || !resource.healthy
                || !seen.insert(resource.node_id.clone())
            {
                continue;
            }
            selected.push(resource);
        }
        if selected.is_empty() {
            return Err(PlacementAutomationError::NoEligibleResources);
        }
        Ok(selected)
    }

    fn activation_dispatches(
        &mut self,
        plan: &PlacementPlan,
        placement_key: &str,
    ) -> Result<Vec<PlacementActivationDispatch>, PlacementAutomationError> {
        let mut by_node = BTreeMap::<String, Vec<ShardId>>::new();
        for placement in &plan.placements {
            by_node
                .entry(placement.active_node.clone())
                .or_default()
                .push(placement.shard_id);
        }
        let endpoints = self
            .spec
            .endpoint_addresses
            .iter()
            .map(|(node_id, address)| StableWorkerEndpoint {
                node_id: node_id.clone(),
                address: address.clone(),
            })
            .collect::<Vec<_>>();
        let mut result = Vec::with_capacity(by_node.len());
        for (target_node, mut owned_shards) in by_node {
            owned_shards.sort_unstable();
            let target_endpoints = endpoints
                .iter()
                .filter(|endpoint| endpoint.node_id != target_node)
                .cloned()
                .collect::<Vec<_>>();
            let manifest = StablePartialWorkerBootstrapManifest::from_authoritative_state(
                self.spec.runtime.clone(),
                plan.clone(),
                target_node.clone(),
                owned_shards,
                self.spec.source_nodes.iter().cloned().collect(),
                self.spec
                    .worker_state_root
                    .join(&target_node)
                    .join("receiver.json"),
                self.spec
                    .worker_state_root
                    .join(&target_node)
                    .join("outbound.json"),
                self.spec.max_pending_outbound,
                self.spec.max_outbound_per_step,
                target_endpoints,
            )
            .map_err(PlacementAutomationError::Activation)?;
            let operation_id = self.next_operation_id;
            self.next_operation_id =
                self.next_operation_id
                    .checked_add(1)
                    .ok_or(PlacementAutomationError::Invalid(
                        "activation operation ID exhausted",
                    ))?;
            let activation_idempotency_key = format!("{placement_key}:target:{target_node}");
            let request_id = format!("{activation_idempotency_key}:request");
            let mut command = manifest
                .activation_command(request_id, operation_id, self.spec.network_id.clone())
                .map_err(PlacementAutomationError::Activation)?;
            command
                .bind_placement_idempotency_key(placement_key.to_owned())
                .map_err(|error| {
                    PlacementAutomationError::Activation(StableRuntimeBootstrapError::Activation(
                        error,
                    ))
                })?;
            if let Some(reference) = self.spec.checkpoint_transfers.get(&target_node) {
                command
                    .bind_checkpoint_transfer(reference.clone())
                    .map_err(|error| {
                        PlacementAutomationError::Activation(
                            StableRuntimeBootstrapError::Activation(error),
                        )
                    })?;
            }
            result.push(PlacementActivationDispatch {
                activation_idempotency_key,
                target_node,
                command,
            });
        }
        Ok(result)
    }
}
