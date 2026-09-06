//! Versioned bootstrap for the stable-ID durable execution runtime.
//!
//! A deployed process must never infer biological authority from a dense
//! [`Runner`](crate::runner::Runner), discovery observation, placement
//! telemetry, or a partially present checkpoint directory.  This module is
//! the explicit recovery boundary: an operator supplies a bounded manifest,
//! the manifest reconstructs the stable topology and compiled partition, and
//! the immutable complete-fabric checkpoint is verified before a managed
//! executor is returned.
//!
//! The module is intentionally filesystem-only.  It does not open sockets or
//! contact an orchestrator, so callers can run it in `spawn_blocking` and
//! perform registration only after all validation succeeds.

use crate::deterministic::{
    BrainId, EventId, LeaseTerm, NeuronId, PartitionGeneration, StateDigest, StreamId,
    TopologyGeneration,
};
use crate::managed_stable_executor::{ManagedStableExecutor, ManagedStableExecutorError};
use crate::partial_shard_executor::PartialShardExecutor;
use crate::placement::{PLACEMENT_SCHEMA_VERSION, PlacementPlan};
use crate::placement_registry::{PlacementApplyRequest, PlacementRegistry};
use crate::stable_executor_durable::{StableExecutorDurableBridge, StableExecutorDurableError};
use crate::stable_executor_store::{StableExecutorCheckpointStore, StableExecutorStoreError};
use crate::stable_outbound::StableOutboundLog;
use crate::stable_shard_dispatch::StableShardDispatcher;
use crate::stable_shard_transport::DurableStableShardReceiver;
use crate::stable_worker::{StableWorkerActivationCommand, StableWorkerActivationError};
use crate::topology_model::{
    CompiledExecutionPlan, NeuronRecord, OwnershipRecord, SynapseRecord, TopologyError,
    TopologyGenerationModel, VirtualShardAssignment, compile_execution_plan,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

pub const STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION: u32 = 1;
pub const MAX_STABLE_RUNTIME_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_EXECUTOR_BOUND: usize = 1_000_000;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Explicit, versioned deployment input for one stable whole-brain runtime.
///
/// The topology and partition are embedded so a restart cannot silently
/// compile a different plan from a mutable model file.  Paths are deployment
/// controlled and are checked for separation before any durable actor is
/// opened.  The manifest itself is configuration, not authority: the
/// checkpoint, term and actor contents must all agree before registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableRuntimeBootstrapManifest {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub topology_generation: TopologyGeneration,
    pub topology_digest: StateDigest,
    pub neurons: Vec<NeuronRecord>,
    pub synapses: Vec<SynapseRecord>,
    pub partition_generation: PartitionGeneration,
    pub assignments: Vec<VirtualShardAssignment>,
    pub ownership: Vec<OwnershipRecord>,
    pub plan_digest: StateDigest,
    pub checkpoint_id: EventId,
    pub checkpoint_root: PathBuf,
    pub owner_root: PathBuf,
    pub warm_root: PathBuf,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub stream_id: StreamId,
    pub max_payload: usize,
    pub max_input_events: usize,
    pub max_steps_per_poll: usize,
    pub threshold: i64,
    pub weight: i64,
    pub queue_capacity: usize,
    pub dedupe_capacity: usize,
    #[serde(default)]
    pub channel_state: Vec<u8>,
    /// Stable neuron IDs receiving a sensory vector in index order.  An empty
    /// mapping is valid for a runtime driven only by causal events.
    #[serde(default)]
    pub sensory_targets: Vec<NeuronId>,
}

/// The validated material retained alongside the managed runtime for callers
/// that need to publish placement or migration evidence.
#[derive(Debug)]
pub struct StableRuntimeBootstrap {
    pub manifest: StableRuntimeBootstrapManifest,
    pub topology: TopologyGenerationModel,
    pub plan: CompiledExecutionPlan,
    pub runtime: ManagedStableExecutor,
}

/// One endpoint that a partial worker is explicitly permitted to use for
/// durable stable-shard output. Endpoint discovery does not populate this
/// list; the orchestrator or operator must supply and validate it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableWorkerEndpoint {
    pub node_id: String,
    pub address: String,
}

/// Explicit bootstrap input for a worker that materialises only a subset of
/// the stable virtual shards in a complete immutable cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StablePartialWorkerBootstrapManifest {
    pub schema_version: u32,
    pub runtime: StableRuntimeBootstrapManifest,
    pub node_id: String,
    pub owned_shards: Vec<crate::deterministic::ShardId>,
    pub allowed_source_nodes: Vec<String>,
    pub receiver_path: PathBuf,
    pub outbound_path: PathBuf,
    pub max_pending_outbound: usize,
    pub max_outbound_per_step: usize,
    pub endpoints: Vec<StableWorkerEndpoint>,
    pub placement: PlacementPlan,
    /// Lease term recorded by the immutable checkpoint being opened. A
    /// migrated checkpoint may have been published under the source term
    /// while the worker is opened under the newer destination term. Keeping
    /// that provenance explicit prevents checkpoint history being confused
    /// with current writer authority.
    #[serde(default)]
    pub checkpoint_lease_term: Option<LeaseTerm>,
}

/// Validated physical partial-worker bootstrap. Registration with a node is a
/// separate caller action, so constructing this value cannot grant authority
/// or make the worker reachable from the network.
#[derive(Debug)]
pub struct StablePartialWorkerBootstrap {
    pub manifest: StablePartialWorkerBootstrapManifest,
    pub topology: TopologyGenerationModel,
    pub plan: CompiledExecutionPlan,
    pub receiver: DurableStableShardReceiver,
    pub dispatcher: StableShardDispatcher,
}

#[derive(Debug, Error)]
pub enum StableRuntimeBootstrapError {
    #[error("stable runtime bootstrap manifest is too large")]
    ManifestTooLarge,
    #[error("stable runtime bootstrap manifest I/O failed: {0}")]
    Io(String),
    #[error("stable runtime bootstrap manifest encoding failed: {0}")]
    Encoding(String),
    #[error("stable runtime bootstrap manifest schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("stable runtime bootstrap manifest is invalid: {0}")]
    Invalid(&'static str),
    #[error("stable runtime bootstrap topology is invalid: {0}")]
    Topology(#[from] TopologyError),
    #[error("stable runtime bootstrap checkpoint store failed: {0}")]
    Store(#[from] StableExecutorStoreError),
    #[error("stable runtime bootstrap durable bridge failed: {0}")]
    Durable(#[from] StableExecutorDurableError),
    #[error("stable runtime bootstrap managed executor failed: {0}")]
    Managed(#[from] ManagedStableExecutorError),
    #[error("stable runtime bootstrap expected brain {expected}, manifest names {actual}")]
    BrainMismatch { expected: BrainId, actual: BrainId },
    #[error("stable runtime bootstrap {field} digest does not match its contents")]
    DigestMismatch { field: &'static str },
    #[error("stable runtime bootstrap checkpoint manifest does not match the bootstrap manifest")]
    CheckpointMismatch,
    #[error("stable runtime bootstrap authority lock is already held")]
    AuthorityAlreadyHeld,
    #[error("stable runtime bootstrap authority lock failed: {0}")]
    AuthorityLock(String),
    #[error("stable partial worker bootstrap placement failed: {0}")]
    Placement(String),
    #[error("stable partial worker bootstrap transport failed: {0}")]
    Transport(String),
    #[error("stable partial worker bootstrap dispatch failed: {0}")]
    Dispatch(String),
    #[error("stable worker activation command construction failed: {0}")]
    Activation(#[from] StableWorkerActivationError),
}

impl StableRuntimeBootstrapManifest {
    /// Read and decode a bounded manifest from a deployment-controlled path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, StableRuntimeBootstrapError> {
        let bytes =
            fs::read(path).map_err(|error| StableRuntimeBootstrapError::Io(error.to_string()))?;
        if bytes.len() > MAX_STABLE_RUNTIME_MANIFEST_BYTES {
            return Err(StableRuntimeBootstrapError::ManifestTooLarge);
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| StableRuntimeBootstrapError::Encoding(error.to_string()))
    }

    /// Open a runtime whose identity has already been bound by the caller.
    /// The returned runtime is not implicitly registered with any node.
    pub fn open(
        self,
        expected_brain: Option<BrainId>,
    ) -> Result<StableRuntimeBootstrap, StableRuntimeBootstrapError> {
        self.validate(expected_brain)?;
        let (topology, plan) = self.compile_plan()?;
        let store = StableExecutorCheckpointStore::new(self.checkpoint_root.clone())?;
        let checkpoint = store.verify(self.checkpoint_id).map_err(|error| {
            StableRuntimeBootstrapError::Store(StableExecutorStoreError::Storage(error.to_string()))
        })?;
        if checkpoint.manifest.lease_term != self.lease_term
            || checkpoint.manifest.partition_generation != self.partition_generation
        {
            return Err(StableRuntimeBootstrapError::CheckpointMismatch);
        }
        let executor = store.load(self.checkpoint_id, self.brain_id, plan.clone())?;
        if executor.plan().digest() != self.plan_digest
            || executor.plan().topology_generation() != self.topology_generation
            || executor.plan().partition_generation() != self.partition_generation
        {
            return Err(StableRuntimeBootstrapError::DigestMismatch { field: "plan" });
        }
        let authority_lock = acquire_authority_lock(&self.owner_root)?;
        let bridge = StableExecutorDurableBridge::open_existing(
            executor,
            store,
            self.lease_term,
            self.fencing_token,
            self.owner_root.clone(),
            self.warm_root.clone(),
            self.stream_id,
            self.max_payload,
            self.channel_state.clone(),
        )?;
        let runtime = ManagedStableExecutor::new(
            bridge,
            self.checkpoint_id,
            self.max_input_events,
            self.max_steps_per_poll,
        )?
        .with_sensory_targets(self.sensory_targets.clone())?;
        let runtime = runtime.with_authority_lock(authority_lock);
        Ok(StableRuntimeBootstrap {
            manifest: self,
            topology,
            plan,
            runtime,
        })
    }

    /// Convenience wrapper for a network name.  Network names are placement
    /// metadata; the stable brain identity is the shared managed identity.
    pub fn open_for_network(
        self,
        network_id: &str,
    ) -> Result<StableRuntimeBootstrap, StableRuntimeBootstrapError> {
        let expected = crate::managed_durability::managed_brain_id(network_id);
        self.open(Some(expected))
    }

    pub fn compile_plan(
        &self,
    ) -> Result<(TopologyGenerationModel, CompiledExecutionPlan), StableRuntimeBootstrapError> {
        let topology = TopologyGenerationModel::new(
            self.topology_generation,
            self.neurons.clone(),
            self.synapses.clone(),
        )?;
        if topology.digest() != self.topology_digest {
            return Err(StableRuntimeBootstrapError::DigestMismatch { field: "topology" });
        }
        let plan = compile_execution_plan(
            &topology,
            self.partition_generation,
            self.assignments.clone(),
            self.ownership.clone(),
        )?;
        if plan.digest() != self.plan_digest {
            return Err(StableRuntimeBootstrapError::DigestMismatch { field: "plan" });
        }
        Ok((topology, plan))
    }

    fn validate(&self, expected_brain: Option<BrainId>) -> Result<(), StableRuntimeBootstrapError> {
        if self.schema_version != STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION {
            return Err(StableRuntimeBootstrapError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if let Some(expected) = expected_brain {
            if self.brain_id != expected {
                return Err(StableRuntimeBootstrapError::BrainMismatch {
                    expected,
                    actual: self.brain_id,
                });
            }
        }
        if self.fencing_token == 0
            || self.max_payload == 0
            || self.max_payload > MAX_PAYLOAD_BYTES
            || self.max_input_events == 0
            || self.max_input_events > MAX_EXECUTOR_BOUND
            || self.max_steps_per_poll == 0
            || self.max_steps_per_poll > MAX_EXECUTOR_BOUND
            || self.queue_capacity == 0
            || self.queue_capacity > MAX_EXECUTOR_BOUND
            || self.dedupe_capacity == 0
            || self.dedupe_capacity > MAX_EXECUTOR_BOUND
            || self.checkpoint_root.as_os_str().is_empty()
            || self.owner_root.as_os_str().is_empty()
            || self.warm_root.as_os_str().is_empty()
            || self.owner_root == self.warm_root
            || self.owner_root == self.checkpoint_root
            || self.warm_root == self.checkpoint_root
        {
            return Err(StableRuntimeBootstrapError::Invalid(
                "bounds, fencing token, or durable roots are invalid",
            ));
        }
        if self.neurons.is_empty() || self.assignments.is_empty() {
            return Err(StableRuntimeBootstrapError::Invalid(
                "topology and assignments must not be empty",
            ));
        }
        let mut targets = BTreeSet::new();
        for target in &self.sensory_targets {
            if !targets.insert(*target) || !self.neurons.iter().any(|neuron| neuron.id == *target) {
                return Err(StableRuntimeBootstrapError::Invalid(
                    "sensory targets must be unique topology neuron IDs",
                ));
            }
        }
        Ok(())
    }
}

impl StablePartialWorkerBootstrapManifest {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

    /// Construct a partial-worker manifest from an already verified runtime
    /// description and an immutable placement plan. The caller must still
    /// provide the source and endpoint allowlists explicitly; discovery and
    /// resource telemetry never populate those security-sensitive fields.
    /// Validation runs before the manifest is returned, so a target cannot be
    /// paired with shards it does not actively own in the supplied plan.
    #[allow(clippy::too_many_arguments)]
    pub fn from_authoritative_state(
        mut runtime: StableRuntimeBootstrapManifest,
        placement: PlacementPlan,
        node_id: impl Into<String>,
        owned_shards: Vec<crate::deterministic::ShardId>,
        allowed_source_nodes: Vec<String>,
        receiver_path: PathBuf,
        outbound_path: PathBuf,
        max_pending_outbound: usize,
        max_outbound_per_step: usize,
        endpoints: Vec<StableWorkerEndpoint>,
    ) -> Result<Self, StableRuntimeBootstrapError> {
        // A placement plan carries the new writer authority. The runtime
        // description may still refer to the source checkpoint's previous
        // term, so bind the target manifest to the placement term before it
        // is serialized into an activation command. The transferred
        // checkpoint term is retained separately once the target verifies
        // the immutable transfer receipt.
        if placement.lease_term < runtime.lease_term {
            return Err(StableRuntimeBootstrapError::Placement(
                "target placement term regresses the runtime authority".to_owned(),
            ));
        }
        runtime.lease_term = placement.lease_term;
        runtime.fencing_token = placement.fencing_token;
        let manifest = Self {
            schema_version: Self::SCHEMA_VERSION,
            runtime,
            node_id: node_id.into(),
            owned_shards,
            allowed_source_nodes,
            receiver_path,
            outbound_path,
            max_pending_outbound,
            max_outbound_per_step,
            endpoints,
            placement,
            checkpoint_lease_term: None,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Create the orchestrator command envelope from this already validated
    /// manifest. The command derives brain and target identities from the
    /// manifest so callers cannot accidentally bind a valid payload to a
    /// different worker or brain. Checkpoint bytes are still verified by the
    /// target's `open` step before registration.
    pub fn activation_command(
        &self,
        request_id: impl Into<String>,
        operation_id: u64,
        network_id: impl Into<String>,
    ) -> Result<StableWorkerActivationCommand, StableRuntimeBootstrapError> {
        self.validate()?;
        let manifest_json = serde_json::to_string(self)
            .map_err(|error| StableRuntimeBootstrapError::Encoding(error.to_string()))?;
        StableWorkerActivationCommand::new(
            request_id,
            operation_id,
            self.runtime.brain_id.raw(),
            network_id,
            self.node_id.clone(),
            manifest_json,
        )
        .map_err(StableRuntimeBootstrapError::Activation)
    }

    /// Rebase a transferred activation onto target-local durable roots.
    /// Source paths are deployment metadata and are never trusted across a
    /// node boundary. The checkpoint root is supplied by the target's local
    /// transfer service; worker state paths are derived from the target's
    /// configured state root.
    pub fn rebase_to_transferred_checkpoint(
        &mut self,
        checkpoint_root: impl Into<PathBuf>,
        worker_state_root: impl Into<PathBuf>,
        reference: &crate::stable_worker::StableWorkerCheckpointTransferReference,
    ) -> Result<(), StableRuntimeBootstrapError> {
        reference
            .validate()
            .map_err(StableRuntimeBootstrapError::Activation)?;
        if reference.brain_id != self.runtime.brain_id.raw()
            || reference.checkpoint_id != self.runtime.checkpoint_id.raw()
            || reference.partition_generation != self.runtime.partition_generation.raw()
            || reference.plan_digest != self.runtime.plan_digest.to_string()
        {
            return Err(StableRuntimeBootstrapError::CheckpointMismatch);
        }
        let source_term = LeaseTerm::new(reference.lease_term)
            .map_err(|_| StableRuntimeBootstrapError::CheckpointMismatch)?;
        if source_term > self.placement.lease_term {
            return Err(StableRuntimeBootstrapError::CheckpointMismatch);
        }
        let worker_state_root = worker_state_root.into();
        self.runtime.checkpoint_root = checkpoint_root.into();
        self.runtime.owner_root = worker_state_root.join("owners");
        self.runtime.warm_root = worker_state_root.join("warm");
        self.receiver_path = worker_state_root.join("receiver.json");
        self.outbound_path = worker_state_root.join("outbound.json");
        self.checkpoint_lease_term = Some(source_term);
        self.validate()
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, StableRuntimeBootstrapError> {
        let bytes =
            fs::read(path).map_err(|error| StableRuntimeBootstrapError::Io(error.to_string()))?;
        if bytes.len() > Self::MAX_MANIFEST_BYTES {
            return Err(StableRuntimeBootstrapError::ManifestTooLarge);
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| StableRuntimeBootstrapError::Encoding(error.to_string()))
    }

    /// Open only the selected checkpoint subset and bind it to an explicit
    /// physical placement. The complete checkpoint is still verified first so
    /// a worker cannot assemble itself from sibling cuts or an untrusted plan.
    pub fn open(self) -> Result<StablePartialWorkerBootstrap, StableRuntimeBootstrapError> {
        self.validate()?;
        let (topology, plan) = self.runtime.compile_plan()?;
        let compiled_shards = plan.shard_ids().collect::<BTreeSet<_>>();
        let placement_shards = self
            .placement
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        if compiled_shards != placement_shards {
            return Err(StableRuntimeBootstrapError::Placement(
                "placement shard set does not match the compiled biological plan".to_owned(),
            ));
        }
        let store = StableExecutorCheckpointStore::new(self.runtime.checkpoint_root.clone())?;
        let checkpoint = store.verify(self.runtime.checkpoint_id).map_err(|error| {
            StableRuntimeBootstrapError::Store(StableExecutorStoreError::Storage(error.to_string()))
        })?;
        let expected_checkpoint_term = self
            .checkpoint_lease_term
            .unwrap_or(self.runtime.lease_term);
        if checkpoint.manifest.lease_term != expected_checkpoint_term
            || checkpoint.manifest.partition_generation != self.runtime.partition_generation
        {
            return Err(StableRuntimeBootstrapError::CheckpointMismatch);
        }
        let complete = store.load(
            self.runtime.checkpoint_id,
            self.runtime.brain_id,
            plan.clone(),
        )?;
        let checkpoints = complete
            .checkpoint_shards()
            .map_err(|error| StableRuntimeBootstrapError::Transport(error.to_string()))?;
        let owned = self.owned_shards.iter().copied().collect::<BTreeSet<_>>();
        let selected = checkpoints
            .into_iter()
            .filter(|checkpoint| owned.contains(&checkpoint.shard_id))
            .collect::<Vec<_>>();
        let worker = PartialShardExecutor::from_checkpoints(
            self.runtime.brain_id,
            &topology,
            plan.clone(),
            selected,
            owned,
            self.max_outbound_per_step,
        )
        .map_err(|error| StableRuntimeBootstrapError::Transport(error.to_string()))?;

        let receiver = if self.receiver_path.exists() {
            let receiver = DurableStableShardReceiver::open_with_placement_digest(
                self.receiver_path.clone(),
                self.node_id.clone(),
                &topology,
                plan.clone(),
                Some(self.placement.digest()),
                self.allowed_source_nodes.clone(),
            )
            .map_err(|error| StableRuntimeBootstrapError::Transport(error.to_string()))?;
            if receiver.brain_id() != self.runtime.brain_id
                || receiver.lease_term() != self.runtime.lease_term
                || receiver.fencing_token() != self.runtime.fencing_token
                || receiver.owned_shard_ids() != self.owned_shards
            {
                return Err(StableRuntimeBootstrapError::CheckpointMismatch);
            }
            receiver
        } else {
            DurableStableShardReceiver::new_with_placement_digest(
                self.receiver_path.clone(),
                self.node_id.clone(),
                worker,
                self.runtime.lease_term,
                self.runtime.fencing_token,
                Some(self.placement.digest()),
                self.allowed_source_nodes.clone(),
            )
            .map_err(|error| StableRuntimeBootstrapError::Transport(error.to_string()))?
        };

        let outbox = Arc::new(tokio::sync::Mutex::new(
            StableOutboundLog::open(
                self.outbound_path.clone(),
                self.runtime.brain_id,
                self.max_pending_outbound,
            )
            .map_err(|error| StableRuntimeBootstrapError::Dispatch(error.to_string()))?,
        ));
        let mut registry = PlacementRegistry::new(self.runtime.brain_id, self.placement.lease_term);
        registry
            .apply(PlacementApplyRequest {
                request_id: format!("stable-worker-bootstrap-{}", self.placement.digest()),
                idempotency_key: format!("stable-worker-bootstrap-{}", self.placement.digest()),
                expected_resource_version: 0,
                observed_leader_term: self.placement.lease_term,
                plan: self.placement.clone(),
                cutover: None,
                repartition: None,
            })
            .map_err(|error| StableRuntimeBootstrapError::Placement(error.to_string()))?;
        let dispatcher = StableShardDispatcher::new(
            self.node_id.clone(),
            Arc::new(std::sync::RwLock::new(registry)),
            outbox,
        )
        .map_err(|error| StableRuntimeBootstrapError::Dispatch(error.to_string()))?;
        for endpoint in &self.endpoints {
            dispatcher
                .register_endpoint(&endpoint.node_id, &endpoint.address)
                .map_err(|error| StableRuntimeBootstrapError::Dispatch(error.to_string()))?;
        }

        Ok(StablePartialWorkerBootstrap {
            manifest: self,
            topology,
            plan,
            receiver,
            dispatcher,
        })
    }

    fn validate(&self) -> Result<(), StableRuntimeBootstrapError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(StableRuntimeBootstrapError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        self.runtime.validate(None)?;
        if self.node_id.trim().is_empty()
            || self.receiver_path.as_os_str().is_empty()
            || self.outbound_path.as_os_str().is_empty()
            || self.receiver_path == self.outbound_path
            || self.max_pending_outbound == 0
            || self.max_outbound_per_step == 0
            || self.owned_shards.is_empty()
            || self.allowed_source_nodes.is_empty()
        {
            return Err(StableRuntimeBootstrapError::Invalid(
                "partial worker identity, paths, shard set, source set, or bounds are invalid",
            ));
        }
        if self
            .owned_shards
            .windows(2)
            .any(|window| window[0] >= window[1])
        {
            return Err(StableRuntimeBootstrapError::Invalid(
                "owned shards must be sorted and unique",
            ));
        }
        if self
            .allowed_source_nodes
            .iter()
            .any(|node| node.trim().is_empty())
        {
            return Err(StableRuntimeBootstrapError::Invalid(
                "allowed source node identities must not be empty",
            ));
        }
        if self.placement.schema_version != PLACEMENT_SCHEMA_VERSION {
            return Err(StableRuntimeBootstrapError::Placement(
                "placement schema version is unsupported".to_owned(),
            ));
        }
        self.placement
            .verify()
            .map_err(|error| StableRuntimeBootstrapError::Placement(error.to_string()))?;
        if self.placement.brain_id != self.runtime.brain_id
            || self.placement.topology_generation != self.runtime.topology_generation
            || self.placement.partition_generation != self.runtime.partition_generation
            || self.placement.lease_term != self.runtime.lease_term
            || self.placement.fencing_token != self.runtime.fencing_token
        {
            return Err(StableRuntimeBootstrapError::Placement(
                "placement identity does not match the runtime manifest".to_owned(),
            ));
        }
        if self
            .checkpoint_lease_term
            .is_some_and(|term| term > self.runtime.lease_term)
        {
            return Err(StableRuntimeBootstrapError::CheckpointMismatch);
        }
        let placement_shards = self
            .placement
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        if self
            .owned_shards
            .iter()
            .any(|shard| !placement_shards.contains(shard))
            || self.owned_shards.iter().any(|shard| {
                self.placement
                    .placements
                    .iter()
                    .find(|placement| placement.shard_id == *shard)
                    .is_none_or(|placement| placement.active_node != self.node_id)
            })
        {
            return Err(StableRuntimeBootstrapError::Placement(
                "owned shards are not active on the declared worker node".to_owned(),
            ));
        }
        let mut endpoint_ids = BTreeSet::new();
        if self.endpoints.iter().any(|endpoint| {
            endpoint.node_id.trim().is_empty()
                || endpoint.address.len() > 2048
                || !(endpoint.address.starts_with("http://")
                    || endpoint.address.starts_with("https://"))
                || !endpoint_ids.insert(endpoint.node_id.as_str())
        }) {
            return Err(StableRuntimeBootstrapError::Invalid(
                "worker endpoints are invalid or duplicated",
            ));
        }
        Ok(())
    }
}

fn acquire_authority_lock(root: &Path) -> Result<Arc<File>, StableRuntimeBootstrapError> {
    fs::create_dir_all(root).map_err(|error| StableRuntimeBootstrapError::Io(error.to_string()))?;
    let path = root.join(".stable-runtime-authority.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| StableRuntimeBootstrapError::AuthorityLock(error.to_string()))?;
    file.try_lock_exclusive()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::WouldBlock => StableRuntimeBootstrapError::AuthorityAlreadyHeld,
            _ => StableRuntimeBootstrapError::AuthorityLock(error.to_string()),
        })?;
    Ok(Arc::new(file))
}
