//! Orchestrator-owned migration executor registration and dispatch.
//!
//! The management API owns migration intent and durable operation state.  It
//! must not know how a worker stores a checkpoint or how a transport moves a
//! frame.  This module is the narrow adapter between those responsibilities:
//! an orchestrator registers one executor for a brain, dispatches a validated
//! [`MigrationOperation`], and receives immutable cutover evidence back.
//!
//! Dispatch is deliberately bounded and non-blocking from the gRPC task.  The
//! executor itself may perform filesystem or transport work, but it runs in a
//! blocking worker and is protected by one brain-scoped in-flight lease.  A
//! second request for the same brain cannot start a concurrent migration, and
//! a retry of the same operation is rejected while the original execution is
//! still owned.  The journal remains the authority for the durable operation;
//! this registry is only the live execution adapter.

use crate::brain_migration_session::BrainMigrationSession;
use crate::checkpoint_transfer::send_checkpoint_transfer;
use crate::consistent_cut::ConsistentCut;
use crate::deterministic::{BrainId, EventId, LogicalTag, ShardId, StreamId};
use crate::management::ReplicatedQuorumLeaseAuthority;
use crate::migration_group::{MigrationGroup, MigrationGroupSpec};
use crate::migration_operation::MigrationOperation;
use crate::placement::PlacementPlan;
use crate::placement_registry::{PersistedPlacementRegistry, PlacementApplyRequest};
use crate::stable_executor_durable::StableExecutorDurableBridge;
use crate::stable_worker::{
    StableWorkerActivationCommand, StableWorkerCheckpointTransferReference,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Schema version for the deployment-controlled live migration registration
/// manifest.  The manifest contains configuration and immutable target
/// evidence; it never grants authority by itself.  Startup still verifies the
/// source runtime identity and the target activation boundary before the
/// executor is registered.
pub const STABLE_MIGRATION_DEPLOYMENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_STABLE_MIGRATION_DEPLOYMENT_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
/// Maximum amount of already-admitted work a source may settle while entering
/// the migration boundary. Exhaustion is an explicit non-convergence failure;
/// it never becomes an implicit quiescence decision.
pub const DEFAULT_MIGRATION_DRAIN_STEP_LIMIT: usize = 65_536;

/// Explicit startup input for one orchestrator-hosted stable brain.
///
/// This is intentionally a DTO rather than a serialised
/// [`StableExecutorMigrationSettings`]: locks, live callbacks and authority
/// handles must be constructed by the process that loads the manifest.  The
/// deployment system owns the paths and endpoint allow-list, while the
/// orchestrator binds the result to its local stable runtime before calling
/// [`DistributedNode::register_stable_network_migration_executor`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableMigrationDeploymentManifest {
    pub schema_version: u32,
    pub network_id: String,
    pub source_node: String,
    pub consistent_cut: ConsistentCut,
    pub destination_root: PathBuf,
    pub warm_root: PathBuf,
    pub authority_replica_paths: BTreeMap<String, PathBuf>,
    pub authority_members: BTreeSet<String>,
    pub destination_nodes: BTreeMap<ShardId, String>,
    pub source_fencing_tokens: BTreeMap<ShardId, u64>,
    pub placement_registry_path: PathBuf,
    pub target_plan: PlacementPlan,
    pub stream_id: StreamId,
    pub max_payload: usize,
    pub frame_bytes: usize,
    pub destination_endpoints: BTreeMap<String, String>,
    pub target_activation_commands: BTreeMap<String, StableWorkerActivationCommand>,
}

impl StableMigrationDeploymentManifest {
    /// Load a bounded manifest from a deployment-controlled path.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let bytes = std::fs::read(path.as_ref())
            .map_err(|error| format!("stable migration manifest I/O failed: {error}"))?;
        if bytes.len() > MAX_STABLE_MIGRATION_DEPLOYMENT_MANIFEST_BYTES {
            return Err("stable migration manifest exceeds its bounded size".to_owned());
        }
        let manifest: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("stable migration manifest JSON is invalid: {error}"))?;
        manifest.validate()
    }

    /// Validate deployment input before opening any authority or registry
    /// files.  Endpoint and command sets must agree exactly so an omitted
    /// worker cannot be mistaken for a successfully activated destination.
    pub fn validate(self) -> Result<Self, String> {
        if self.schema_version != STABLE_MIGRATION_DEPLOYMENT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported stable migration manifest schema {}",
                self.schema_version
            ));
        }
        if self.network_id.trim().is_empty()
            || self.source_node.trim().is_empty()
            || self.destination_root.as_os_str().is_empty()
            || self.warm_root.as_os_str().is_empty()
            || self.placement_registry_path.as_os_str().is_empty()
            || self.max_payload == 0
            || self.max_payload > 64 * 1024 * 1024
            || self.frame_bytes == 0
            || self.frame_bytes > self.max_payload
        {
            return Err(
                "stable migration manifest contains an empty or unbounded field".to_owned(),
            );
        }
        if self.destination_root == self.warm_root {
            return Err("stable migration destination and warm roots must differ".to_owned());
        }
        self.consistent_cut
            .verify()
            .map_err(|error| format!("stable migration consistent cut is invalid: {error}"))?;
        self.target_plan
            .verify()
            .map_err(|error| format!("stable migration target plan is invalid: {error}"))?;
        if self.authority_members.len() < 3
            || self
                .authority_replica_paths
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != self.authority_members
            || self
                .authority_replica_paths
                .values()
                .collect::<BTreeSet<_>>()
                .len()
                != self.authority_replica_paths.len()
        {
            return Err(
                "stable migration authority requires distinct paths for every configured member"
                    .to_owned(),
            );
        }
        if self.destination_nodes.is_empty()
            || self
                .destination_nodes
                .values()
                .any(|node| node.trim().is_empty())
        {
            return Err("stable migration destination nodes are invalid".to_owned());
        }
        let target_nodes = self
            .destination_nodes
            .values()
            .cloned()
            .collect::<BTreeSet<_>>();
        let plan_nodes = self
            .target_plan
            .placements
            .iter()
            .map(|placement| placement.active_node.clone())
            .collect::<BTreeSet<_>>();
        if target_nodes != plan_nodes {
            return Err(
                "stable migration destination nodes must cover exactly the target plan".to_owned(),
            );
        }
        if self
            .destination_endpoints
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != target_nodes
            || self.destination_endpoints.values().any(|address| {
                address.len() > 2048
                    || !(address.starts_with("http://") || address.starts_with("https://"))
            })
        {
            return Err(
                "stable migration destination endpoints must exactly cover target nodes".to_owned(),
            );
        }
        if self
            .target_activation_commands
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != target_nodes
        {
            return Err(
                "stable migration activation commands must exactly cover target nodes".to_owned(),
            );
        }
        for (node, command) in &self.target_activation_commands {
            command
                .verify()
                .map_err(|error| format!("activation command for {node} is invalid: {error}"))?;
            if command.target_node != *node
                || command.network_id != self.network_id
                || command.brain_id != self.target_plan.brain_id.raw()
            {
                return Err(format!(
                    "activation command for {node} is not bound to this network, brain, or node"
                ));
            }
        }
        if self.source_fencing_tokens.is_empty()
            || self.source_fencing_tokens.values().any(|token| *token == 0)
        {
            return Err("stable migration source fencing tokens are invalid".to_owned());
        }
        Ok(self)
    }

    /// Construct live handles only after manifest validation has completed.
    pub fn into_settings(self) -> Result<StableExecutorMigrationSettings, String> {
        let authority = ReplicatedQuorumLeaseAuthority::open(
            self.authority_replica_paths.clone(),
            self.authority_members.clone(),
        )
        .map_err(|error| format!("stable migration authority failed to open: {error}"))?;
        let placement_registry = PersistedPlacementRegistry::open(
            self.placement_registry_path,
            self.target_plan.brain_id,
            self.target_plan.lease_term,
        )
        .map_err(|error| format!("stable migration placement registry failed to open: {error}"))?;
        Ok(StableExecutorMigrationSettings {
            consistent_cut: self.consistent_cut,
            source_node: self.source_node,
            destination_root: self.destination_root,
            warm_root: self.warm_root,
            authority: Arc::new(Mutex::new(authority)),
            destination_nodes: self.destination_nodes,
            source_fencing_tokens: self.source_fencing_tokens,
            placement_registry: Arc::new(Mutex::new(placement_registry)),
            target_plan: self.target_plan,
            stream_id: self.stream_id,
            max_payload: self.max_payload,
            frame_bytes: self.frame_bytes,
            destination_endpoints: self.destination_endpoints,
            target_activation_commands: self.target_activation_commands,
            activation_gate: None,
        })
    }
}

/// Verified result returned by a live migration executor.
///
/// The group must already be committed by the data-plane/cutover adapter.  A
/// management caller cannot manufacture this value from a progress update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationDispatchReceipt {
    pub operation_id: u64,
    pub brain_id: BrainId,
    pub group: MigrationGroup,
    pub cut_tag: LogicalTag,
    pub transferred_bytes: u64,
}

impl MigrationDispatchReceipt {
    pub fn verify_against(&self, operation: &MigrationOperation) -> Result<(), String> {
        if self.operation_id != operation.operation_id || self.brain_id != operation.brain_id {
            return Err("migration dispatch receipt identity does not match operation".to_owned());
        }
        if self.group.operation_id != operation.operation_id
            || self.group.brain_id != operation.brain_id
        {
            return Err("migration dispatch group identity does not match operation".to_owned());
        }
        if self.group.phase != crate::migration_group::MigrationGroupPhase::Committed {
            return Err("migration dispatch returned an uncommitted group".to_owned());
        }
        if self.cut_tag.microstep != 0 {
            return Err("migration dispatch cut tag is not a committed boundary".to_owned());
        }
        if self.transferred_bytes != operation.progress.total_bytes {
            return Err("migration dispatch byte count does not match operation".to_owned());
        }
        if operation.progress.total_shards != self.group.shards.len() as u32 {
            return Err("migration dispatch shard count does not match operation".to_owned());
        }
        Ok(())
    }
}

/// Error returned when registration or live dispatch cannot be accepted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrationExecutorError {
    #[error("migration executor for brain {0} is already registered")]
    AlreadyRegistered(BrainId),
    #[error("migration executor for brain {0} is not registered")]
    NotRegistered(BrainId),
    #[error("brain {0} already has an in-flight migration")]
    BrainBusy(BrainId),
    #[error("migration operation {0} is already in flight")]
    OperationBusy(u64),
    #[error("migration executor registration lock is poisoned")]
    LockPoisoned,
}

/// A synchronous executor implementation.  Implementations must validate
/// the operation and group, perform bounded transfer/cutover work, and return
/// only after the destination evidence is durable.  The trait is intentionally
/// transport-neutral so stable executors, remote workers, and test harnesses
/// can share the same management contract.
pub trait MigrationExecutor: Send + Sync + 'static {
    fn execute(
        &self,
        operation: MigrationOperation,
        group: MigrationGroupSpec,
    ) -> Result<MigrationDispatchReceipt, String>;
}

pub type MigrationDispatchFuture =
    Pin<Box<dyn Future<Output = Result<MigrationDispatchReceipt, String>> + Send>>;
pub type MigrationDispatchHandler =
    Arc<dyn Fn(MigrationOperation, MigrationGroupSpec) -> MigrationDispatchFuture + Send + Sync>;

/// Verified target-side materialisation evidence required before a remote
/// migration may publish its destination placement. The activation adapter is
/// responsible for authenticating each target, opening the transferred
/// checkpoint, and waiting for a durable registration bound to `target_plan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableMigrationActivationRequest {
    pub operation_id: u64,
    pub brain_id: BrainId,
    pub target_plan: PlacementPlan,
    pub checkpoint_references: BTreeMap<String, StableWorkerCheckpointTransferReference>,
    /// One digest-bound activation command per distinct target worker.  The
    /// command is carried with the gate request so an adapter cannot invent a
    /// worker manifest after checkpoint transfer or silently activate a
    /// different plan.
    pub activation_commands: BTreeMap<String, StableWorkerActivationCommand>,
}

/// Remote migration cannot infer worker readiness from checkpoint-transfer
/// acknowledgement alone. This gate must return only after all target workers
/// have completed digest-bound activation and registration. Omitting it when
/// remote endpoints are configured is a deliberate fail-closed error.
pub type StableMigrationActivationGate =
    Arc<dyn Fn(StableMigrationActivationRequest) -> Result<(), String> + Send + Sync>;

#[derive(Default)]
struct RegistryState {
    executors: BTreeMap<BrainId, Arc<dyn MigrationExecutor>>,
    in_flight: BTreeSet<(BrainId, u64)>,
}

/// Process-local registry owned by one orchestrator instance.
///
/// Registration is explicit and brain-scoped.  Discovery, node enrolment or
/// a management principal never implicitly creates an executor entry.
#[derive(Clone, Default)]
pub struct MigrationExecutorRegistry {
    state: Arc<Mutex<RegistryState>>,
}

/// Configuration for the stable-ID durable bridge adapter.
///
/// This is the concrete registration path used by reference and QA
/// orchestrators.  All mutable authority is supplied through shared locks so
/// a registration can be owned by the async control plane without holding a
/// blocking mutex across unrelated management requests.
#[derive(Clone)]
pub struct StableExecutorMigrationSettings {
    pub consistent_cut: ConsistentCut,
    pub source_node: String,
    pub destination_root: PathBuf,
    pub warm_root: PathBuf,
    pub authority: Arc<Mutex<ReplicatedQuorumLeaseAuthority>>,
    pub destination_nodes: BTreeMap<ShardId, String>,
    pub source_fencing_tokens: BTreeMap<ShardId, u64>,
    pub placement_registry: Arc<Mutex<PersistedPlacementRegistry>>,
    pub target_plan: PlacementPlan,
    pub stream_id: StreamId,
    pub max_payload: usize,
    pub frame_bytes: usize,
    /// Explicitly enrolled target-node checkpoint-transfer endpoints. An
    /// empty map selects the deterministic in-process reference path used by
    /// unit tests. When populated, every distinct destination node must have
    /// an endpoint and the complete immutable checkpoint is transferred and
    /// acknowledged before any lease or placement publication occurs.
    pub destination_endpoints: BTreeMap<String, String>,
    /// Explicit, already validated partial-worker activation commands keyed
    /// by target node.  A remote migration cannot derive topology, checkpoint
    /// paths, source allow-lists or worker bounds from a placement plan alone;
    /// callers must provide the complete command envelope produced from an
    /// authoritative `StablePartialWorkerBootstrapManifest`.
    pub target_activation_commands: BTreeMap<String, StableWorkerActivationCommand>,
    /// Target activation/registration barrier for remote migration. This is
    /// unused by the deterministic in-process reference profile.
    pub activation_gate: Option<StableMigrationActivationGate>,
}

/// Configuration for the stable-ID durable bridge adapter.
///
/// The bridge is kept separate from the immutable migration settings so a
/// managed runtime can borrow its already-authoritative bridge for the
/// duration of one fenced migration without copying or re-opening it.
pub struct StableExecutorMigrationConfig {
    pub bridge: Arc<Mutex<StableExecutorDurableBridge>>,
    pub settings: StableExecutorMigrationSettings,
}

/// Bridge-backed executor that performs the complete bounded reference
/// migration session.  It transfers verified shard state, requests one
/// brain-wide destination lease transaction, publishes the placement registry
/// only after all shards are materialised, then fences the source bridge.
pub struct StableExecutorMigrationExecutor {
    settings: StableExecutorMigrationSettings,
    bridge: Option<Arc<Mutex<StableExecutorDurableBridge>>>,
    fenced: Mutex<bool>,
}

impl StableExecutorMigrationExecutor {
    pub fn new(config: StableExecutorMigrationConfig) -> Result<Self, String> {
        Self::new_internal(config.settings, Some(config.bridge))
    }

    /// Construct an executor for a managed runtime whose bridge is borrowed
    /// only while `execute_with_bridge` runs. This avoids duplicating a
    /// durable authority merely to make it visible to the management plane.
    pub fn new_for_managed_runtime(
        settings: StableExecutorMigrationSettings,
    ) -> Result<Self, String> {
        Self::new_internal(settings, None)
    }

    fn new_internal(
        settings: StableExecutorMigrationSettings,
        bridge: Option<Arc<Mutex<StableExecutorDurableBridge>>>,
    ) -> Result<Self, String> {
        settings
            .consistent_cut
            .verify()
            .map_err(|error| format!("consistent cut is invalid: {error}"))?;
        settings
            .target_plan
            .verify()
            .map_err(|error| format!("target placement plan is invalid: {error}"))?;
        if settings.source_node.trim().is_empty()
            || settings.destination_nodes.is_empty()
            || settings.max_payload == 0
            || settings.frame_bytes == 0
        {
            return Err(
                "stable migration registration contains an empty or unbounded field".to_owned(),
            );
        }
        if !settings.destination_endpoints.is_empty() {
            let destination_nodes = settings.destination_nodes.values().collect::<BTreeSet<_>>();
            if settings
                .destination_endpoints
                .keys()
                .any(|node| !destination_nodes.contains(node))
                || destination_nodes
                    .iter()
                    .any(|node| !settings.destination_endpoints.contains_key(*node))
                || settings.destination_endpoints.values().any(|address| {
                    address.len() > 2048
                        || !(address.starts_with("http://") || address.starts_with("https://"))
                })
            {
                return Err(
                    "stable migration destination endpoints do not exactly cover target nodes"
                        .to_owned(),
                );
            }
            if settings
                .target_activation_commands
                .keys()
                .collect::<BTreeSet<_>>()
                != destination_nodes
            {
                return Err(
                    "stable migration activation commands do not exactly cover target nodes"
                        .to_owned(),
                );
            }
        }
        Ok(Self {
            settings,
            bridge,
            fenced: Mutex::new(false),
        })
    }

    /// Execute using a bridge borrowed from the live managed runtime.
    ///
    /// The caller must hold the runtime's exclusive authority guard. The
    /// method does not retain the borrow or the bridge after returning.
    pub fn execute_with_bridge(
        &self,
        bridge: &mut StableExecutorDurableBridge,
        operation: MigrationOperation,
        group_spec: MigrationGroupSpec,
    ) -> Result<MigrationDispatchReceipt, String> {
        self.execute_inner(bridge, operation, group_spec)
    }
}

impl MigrationExecutor for StableExecutorMigrationExecutor {
    fn execute(
        &self,
        operation: MigrationOperation,
        group_spec: MigrationGroupSpec,
    ) -> Result<MigrationDispatchReceipt, String> {
        let bridge = self
            .bridge
            .as_ref()
            .ok_or_else(|| "managed migration executor requires a live bridge borrow".to_owned())?
            .clone();
        let mut bridge = bridge
            .lock()
            .map_err(|_| "stable bridge lock poisoned".to_owned())?;
        self.execute_inner(&mut bridge, operation, group_spec)
    }
}

impl StableExecutorMigrationExecutor {
    fn execute_inner(
        &self,
        bridge: &mut StableExecutorDurableBridge,
        operation: MigrationOperation,
        group_spec: MigrationGroupSpec,
    ) -> Result<MigrationDispatchReceipt, String> {
        if *self
            .fenced
            .lock()
            .map_err(|_| "stable migration fence lock poisoned".to_owned())?
        {
            return Err("stable executor source has already been fenced".to_owned());
        }
        if group_spec.brain_id != operation.brain_id
            || group_spec.shard_ids.len() != operation.progress.total_shards as usize
            || self.settings.target_plan.brain_id != operation.brain_id
            || self.settings.target_plan.digest() != operation.target_plan_digest
        {
            return Err("stable migration registration does not match operation".to_owned());
        }
        if operation.progress.total_bytes == 0 {
            return Err("stable migration operation must declare a non-zero byte bound".to_owned());
        }
        let group = group_spec
            .build(operation.operation_id)
            .map_err(|error| format!("migration group is invalid: {error}"))?;
        let first_transfer_id = EventId::new(operation.operation_id)
            .map_err(|error| format!("invalid migration operation identity: {error}"))?;
        let sources = bridge
            .prepare_transfer_sources(
                first_transfer_id,
                self.settings.source_node.clone(),
                &self.settings.consistent_cut,
                operation.source_plan_digest,
                self.settings.frame_bytes,
            )
            .map_err(|error| format!("source transfer preparation failed: {error}"))?;
        let total_bytes = sources
            .iter()
            .map(|source| source.manifest().total_bytes)
            .try_fold(0u64, u64::checked_add)
            .ok_or_else(|| "source transfer byte count overflowed".to_owned())?;
        if total_bytes != operation.progress.total_bytes {
            return Err("source transfer byte count does not match operation".to_owned());
        }
        if sources.len() != operation.progress.total_shards as usize {
            return Err("source transfer shard count does not match operation".to_owned());
        }
        // Everything after this point is one source-drain transaction. The
        // bridge is held exclusively by the managed runtime adapter, so the
        // returned latest states and WAL tails describe the exact boundary
        // that the destination must replay before cutover.
        let result = (|| {
            let observed_term = bridge.authority().term();
            let observed_fencing_token = bridge.authority().fencing_token();
            let checkpoint_start = bridge.next_checkpoint_id().map_err(|error| {
                format!("migration drain checkpoint allocation failed: {error}")
            })?;
            let latest_states = bridge
                .drain_for_migration(
                    observed_term,
                    observed_fencing_token,
                    checkpoint_start,
                    DEFAULT_MIGRATION_DRAIN_STEP_LIMIT,
                )
                .map_err(|error| format!("source drain failed: {error}"))?;
            let latest_by_shard = latest_states
                .into_iter()
                .map(|state| (state.shard_id, state))
                .collect::<BTreeMap<_, _>>();
            if latest_by_shard.len() != sources.len() {
                return Err("source drain returned an incomplete shard state set".to_owned());
            }
            let mut catch_up = BTreeMap::new();
            for source in &sources {
                let shard_id = source.manifest().shard_id;
                let latest = latest_by_shard
                    .get(&shard_id)
                    .ok_or_else(|| format!("source drain omitted shard {}", shard_id.raw()))?;
                let batch = source
                    .imported_state()
                    .map_err(|error| format!("source checkpoint reconstruction failed: {error}"))?
                    .catch_up_from(latest)
                    .map_err(|error| format!("source WAL catch-up failed: {error}"))?;
                catch_up.insert(shard_id, (batch, latest.clone()));
            }
            self.execute_after_source_drain(
                bridge,
                operation,
                group,
                sources,
                catch_up,
                total_bytes,
            )
        })();
        if result.is_err() {
            bridge.abort_migration_drain();
        }
        result
    }

    fn execute_after_source_drain(
        &self,
        bridge: &mut StableExecutorDurableBridge,
        operation: MigrationOperation,
        mut group: MigrationGroup,
        sources: Vec<crate::migration_transfer::ShardTransferSource>,
        catch_up: BTreeMap<
            ShardId,
            (
                crate::migration_transfer::ShardCatchUpBatch,
                crate::authoritative_shard::ShardState,
            ),
        >,
        total_bytes: u64,
    ) -> Result<MigrationDispatchReceipt, String> {
        if !self.settings.destination_endpoints.is_empty() {
            let checkpoint_references =
                transfer_checkpoint_to_targets(&sources, bridge, &self.settings, &operation)?;
            let activation_commands = bind_activation_commands(
                &self.settings.target_activation_commands,
                &checkpoint_references,
                &operation,
            )?;
            let activation_gate = self.settings.activation_gate.as_ref().ok_or_else(|| {
                "remote migration requires a target activation and registration gate".to_owned()
            })?;
            activation_gate(StableMigrationActivationRequest {
                operation_id: operation.operation_id,
                brain_id: operation.brain_id,
                target_plan: self.settings.target_plan.clone(),
                checkpoint_references,
                activation_commands,
            })?;
        }
        let mut authority = self
            .settings
            .authority
            .lock()
            .map_err(|_| "migration authority lock poisoned".to_owned())?;
        let prepared = BrainMigrationSession::prepare_from_sources_with_quorum_and_catch_up(
            &mut group,
            operation.source_plan_digest,
            sources,
            self.settings.destination_root.clone(),
            self.settings.warm_root.clone(),
            &mut authority,
            self.settings.destination_nodes.clone(),
            self.settings.source_fencing_tokens.clone(),
            self.settings.stream_id,
            self.settings.max_payload,
            catch_up,
        )
        .map_err(|error| format!("brain migration preparation failed: {error}"))?;
        let destination_term = prepared
            .destinations
            .values()
            .map(|actor| actor.term())
            .next()
            .ok_or_else(|| "migration produced no destination actors".to_owned())?;
        if prepared
            .destinations
            .values()
            .any(|actor| actor.term() != destination_term)
            || self.settings.target_plan.lease_term != destination_term
        {
            return Err("destination actors and placement plan use different terms".to_owned());
        }
        let mut registry = self
            .settings
            .placement_registry
            .lock()
            .map_err(|_| "placement registry lock poisoned".to_owned())?;
        if registry.state().leader_term != destination_term {
            registry
                .set_leader_term(destination_term)
                .map_err(|error| format!("placement registry term update failed: {error}"))?;
        }
        let expected_resource_version = registry.state().resource_version;
        let outcome = BrainMigrationSession::publish_and_finalize_persisted(
            &mut group,
            prepared,
            &mut registry,
            PlacementApplyRequest {
                request_id: operation.request_id.clone(),
                idempotency_key: operation.idempotency_key.clone(),
                expected_resource_version,
                observed_leader_term: destination_term,
                plan: self.settings.target_plan.clone(),
                cutover: None,
                repartition: None,
            },
            None,
        )
        .map_err(|error| format!("placement publication failed: {error}"))?;
        drop(registry);
        drop(authority);
        bridge
            .fence_after_migration(destination_term)
            .map_err(|error| format!("source bridge fencing failed: {error}"))?;
        *self
            .fenced
            .lock()
            .map_err(|_| "stable migration fence lock poisoned".to_owned())? = true;
        Ok(MigrationDispatchReceipt {
            operation_id: operation.operation_id,
            brain_id: operation.brain_id,
            group: outcome.group,
            cut_tag: outcome.receipt.cut_tag,
            transferred_bytes: total_bytes,
        })
    }
}

/// Bind the immutable target-local checkpoint receipt to each activation
/// command before dispatch.  This is intentionally a separate validation
/// boundary from the transport: a command for a different brain, operation,
/// target node or manifest cannot be paired with a valid transferred cut.
fn bind_activation_commands(
    commands: &BTreeMap<String, StableWorkerActivationCommand>,
    references: &BTreeMap<String, StableWorkerCheckpointTransferReference>,
    operation: &MigrationOperation,
) -> Result<BTreeMap<String, StableWorkerActivationCommand>, String> {
    if commands.len() != references.len() {
        return Err(
            "stable migration activation command and checkpoint target sets differ".to_owned(),
        );
    }
    let mut bound = BTreeMap::new();
    for (node_id, reference) in references {
        let mut command = commands.get(node_id).cloned().ok_or_else(|| {
            format!("stable migration activation command missing for target {node_id}")
        })?;
        command
            .verify()
            .map_err(|error| format!("stable worker activation command is invalid: {error}"))?;
        if command.operation_id != operation.operation_id
            || command.brain_id != operation.brain_id.raw()
            || command.target_node != *node_id
        {
            return Err(format!(
                "stable worker activation command for {node_id} is not bound to the migration"
            ));
        }
        if let Some(existing) = command.checkpoint_transfer.as_ref() {
            if existing != reference {
                return Err(format!(
                    "stable worker activation command for {node_id} carries a conflicting checkpoint reference"
                ));
            }
        } else {
            command
                .bind_checkpoint_transfer(reference.clone())
                .map_err(|error| format!("stable worker checkpoint binding failed: {error}"))?;
        }
        bound.insert(node_id.clone(), command);
    }
    Ok(bound)
}

/// Transfer one complete immutable fabric checkpoint to every distinct target
/// node before the local reference session allocates destination leases.
///
/// The transport calls are concurrent but bounded by the transfer client's
/// four-item channel per target. The function is synchronous because the
/// migration registry already executes it on `spawn_blocking`; no async
/// control-plane mutex is held while a target applies backpressure.
fn transfer_checkpoint_to_targets(
    _sources: &[crate::migration_transfer::ShardTransferSource],
    bridge: &StableExecutorDurableBridge,
    settings: &StableExecutorMigrationSettings,
    operation: &MigrationOperation,
) -> Result<BTreeMap<String, StableWorkerCheckpointTransferReference>, String> {
    let target_nodes = settings
        .destination_nodes
        .values()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut transfers = Vec::with_capacity(target_nodes.len());
    for (index, destination_node) in target_nodes.into_iter().enumerate() {
        let transfer_id = operation
            .operation_id
            .checked_add(
                u64::try_from(index)
                    .map_err(|_| "checkpoint transfer target index overflowed".to_owned())?,
            )
            .and_then(|raw| EventId::new(raw).ok())
            .ok_or_else(|| "checkpoint transfer ID space is exhausted".to_owned())?;
        let source = bridge
            .prepare_checkpoint_transfer_source(
                transfer_id,
                settings.source_node.clone(),
                bridge.executor().plan().digest(),
                settings.frame_bytes,
            )
            .map_err(|error| format!("checkpoint transfer source preparation failed: {error}"))?;
        let address = settings
            .destination_endpoints
            .get(&destination_node)
            .cloned()
            .ok_or_else(|| {
                format!("checkpoint transfer endpoint missing for {destination_node}")
            })?;
        transfers.push((destination_node, address, source));
    }

    let transfer_future = async move {
        futures_util::future::join_all(transfers.into_iter().map(
            |(destination_node, address, source)| async move {
                let reference = source.manifest().activation_reference();
                let source_node = source.manifest().source_node.clone();
                reference
                    .validate()
                    .map_err(|error| format!("activation reference is invalid: {error}"))?;
                let acknowledgement = send_checkpoint_transfer(
                    &address,
                    &source_node,
                    &destination_node,
                    source,
                )
                .await
                .map_err(|error| {
                    format!(
                        "checkpoint transfer to {destination_node} at {address} failed: {error}"
                    )
                })?;
                if acknowledgement.checkpoint_id != reference.checkpoint_id
                    || acknowledgement.brain_id != reference.brain_id
                    || acknowledgement.transfer_id != reference.transfer_id
                {
                    return Err(format!(
                        "checkpoint transfer acknowledgement from {destination_node} does not match its activation reference"
                    ));
                }
                Ok::<(String, StableWorkerCheckpointTransferReference), String>((
                    destination_node,
                    reference,
                ))
            },
        ))
        .await
    };
    let results = if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.block_on(transfer_future)
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("checkpoint transfer runtime creation failed: {error}"))?
            .block_on(transfer_future)
    };
    results.into_iter().collect::<Result<BTreeMap<_, _>, _>>()
}

impl MigrationExecutorRegistry {
    pub fn register(
        &self,
        brain_id: BrainId,
        executor: Arc<dyn MigrationExecutor>,
    ) -> Result<(), MigrationExecutorError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| MigrationExecutorError::LockPoisoned)?;
        if state.executors.insert(brain_id, executor).is_some() {
            return Err(MigrationExecutorError::AlreadyRegistered(brain_id));
        }
        Ok(())
    }

    pub fn unregister(&self, brain_id: BrainId) -> Result<bool, MigrationExecutorError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| MigrationExecutorError::LockPoisoned)?;
        if state.in_flight.iter().any(|(brain, _)| *brain == brain_id) {
            return Err(MigrationExecutorError::BrainBusy(brain_id));
        }
        Ok(state.executors.remove(&brain_id).is_some())
    }

    pub fn contains(&self, brain_id: BrainId) -> Result<bool, MigrationExecutorError> {
        let state = self
            .state
            .lock()
            .map_err(|_| MigrationExecutorError::LockPoisoned)?;
        Ok(state.executors.contains_key(&brain_id))
    }

    pub fn is_in_flight(
        &self,
        brain_id: BrainId,
        operation_id: u64,
    ) -> Result<bool, MigrationExecutorError> {
        let state = self
            .state
            .lock()
            .map_err(|_| MigrationExecutorError::LockPoisoned)?;
        Ok(state.in_flight.contains(&(brain_id, operation_id)))
    }

    /// Return a handler suitable for the secured management service.
    pub fn handler(&self) -> MigrationDispatchHandler {
        let registry = self.clone();
        Arc::new(move |operation, group| {
            let registry = registry.clone();
            Box::pin(async move { registry.dispatch(operation, group).await })
        })
    }

    pub async fn dispatch(
        &self,
        operation: MigrationOperation,
        group: MigrationGroupSpec,
    ) -> Result<MigrationDispatchReceipt, String> {
        let executor =
            {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| MigrationExecutorError::LockPoisoned.to_string())?;
                let executor = state
                    .executors
                    .get(&operation.brain_id)
                    .cloned()
                    .ok_or(MigrationExecutorError::NotRegistered(operation.brain_id))
                    .map_err(|error| error.to_string())?;
                if !state
                    .in_flight
                    .insert((operation.brain_id, operation.operation_id))
                {
                    return Err(
                        MigrationExecutorError::OperationBusy(operation.operation_id).to_string(),
                    );
                }
                if state.in_flight.iter().any(|(brain, id)| {
                    *brain == operation.brain_id && *id != operation.operation_id
                }) {
                    state
                        .in_flight
                        .remove(&(operation.brain_id, operation.operation_id));
                    return Err(MigrationExecutorError::BrainBusy(operation.brain_id).to_string());
                }
                executor
            };
        let state = Arc::clone(&self.state);
        let brain_id = operation.brain_id;
        let operation_id = operation.operation_id;
        let result = tokio::task::spawn_blocking(move || executor.execute(operation, group))
            .await
            .map_err(|error| format!("migration executor worker failed: {error}"))?;
        if let Ok(mut state) = state.lock() {
            state.in_flight.remove(&(brain_id, operation_id));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::{LeaseTerm, PartitionGeneration, StateDigest, TopologyGeneration};
    use crate::migration_operation::{MigrationKind, MigrationPhase, MigrationProgress};
    use std::sync::mpsc::{self, Receiver, Sender};

    struct BlockingExecutor {
        started: Mutex<Option<Sender<()>>>,
        release: Mutex<Receiver<()>>,
    }

    impl MigrationExecutor for BlockingExecutor {
        fn execute(
            &self,
            _operation: MigrationOperation,
            _group: MigrationGroupSpec,
        ) -> Result<MigrationDispatchReceipt, String> {
            self.started
                .lock()
                .map_err(|_| "started lock poisoned".to_owned())?
                .take()
                .ok_or_else(|| "executor was entered twice".to_owned())?
                .send(())
                .map_err(|error| error.to_string())?;
            self.release
                .lock()
                .map_err(|_| "release lock poisoned".to_owned())?
                .recv()
                .map_err(|error| error.to_string())?;
            Err("test executor failure".to_owned())
        }
    }

    fn operation(brain_id: BrainId, operation_id: u64) -> MigrationOperation {
        MigrationOperation {
            operation_id,
            request_id: format!("request-{operation_id}"),
            idempotency_key: format!("idempotency-{operation_id}"),
            brain_id,
            kind: MigrationKind::Move,
            source_plan_digest: StateDigest([1; 16]),
            target_plan_digest: StateDigest([2; 16]),
            phase: MigrationPhase::Prepared,
            progress: MigrationProgress::new(1, 1).expect("valid progress"),
            resource_version: 1,
            error_code: None,
        }
    }

    fn group(brain_id: BrainId) -> MigrationGroupSpec {
        MigrationGroupSpec {
            brain_id,
            leader_term: LeaseTerm::INITIAL,
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            shard_ids: vec![ShardId::new(1).expect("valid shard")],
        }
    }

    #[test]
    fn duplicate_runtime_registration_is_rejected() {
        let registry = MigrationExecutorRegistry::default();
        let brain_id = BrainId::new(1).expect("valid brain");
        let first = Arc::new(BlockingExecutor {
            started: Mutex::new(None),
            release: Mutex::new(mpsc::channel().1),
        });
        let second = Arc::new(BlockingExecutor {
            started: Mutex::new(None),
            release: Mutex::new(mpsc::channel().1),
        });
        registry
            .register(brain_id, first)
            .expect("first registration");
        assert_eq!(
            registry.register(brain_id, second),
            Err(MigrationExecutorError::AlreadyRegistered(brain_id))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_migrations_for_one_brain_are_serialised() {
        let registry = MigrationExecutorRegistry::default();
        let brain_id = BrainId::new(2).expect("valid brain");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        registry
            .register(
                brain_id,
                Arc::new(BlockingExecutor {
                    started: Mutex::new(Some(started_tx)),
                    release: Mutex::new(release_rx),
                }),
            )
            .expect("registration");

        let first_registry = registry.clone();
        let first = tokio::spawn(async move {
            first_registry
                .dispatch(operation(brain_id, 10), group(brain_id))
                .await
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("first migration entered executor");

        let second = registry
            .dispatch(operation(brain_id, 11), group(brain_id))
            .await
            .expect_err("second migration must be rejected while first is active");
        assert!(second.contains("already has an in-flight migration"));

        release_tx.send(()).expect("release first migration");
        assert_eq!(
            first.await.expect("first task join"),
            Err("test executor failure".to_owned())
        );
        assert!(!registry.is_in_flight(brain_id, 10).expect("registry state"));
    }

    #[tokio::test]
    async fn unregistered_brain_is_rejected_before_execution() {
        let registry = MigrationExecutorRegistry::default();
        let brain_id = BrainId::new(3).expect("valid brain");
        let error = registry
            .dispatch(operation(brain_id, 12), group(brain_id))
            .await
            .expect_err("unregistered brain must be rejected");
        assert!(error.contains("is not registered"));
    }
}
