#![cfg(feature = "stable_executor_live")]

//! Exercises the node-owned live migration registration seam.
//!
//! The test deliberately dispatches through `DistributedNode`'s registry,
//! rather than calling the reference migration executor directly. This
//! catches the failure mode where management dispatch is wired to a registry
//! that is disconnected from the hosted stable runtime.

use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::checkpoint_transfer::StableCheckpointTransferService;
use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::consistent_cut::{ChannelMarker, ConsistentCutCoordinator, ParticipantReport};
use aarnn_rust::deterministic::{
    BrainId, ComponentId, EventId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StreamId,
    SynapseId, TopologyGeneration,
};
use aarnn_rust::distributed::proto::distributed_neuromorphic_server::{
    DistributedNeuromorphic, DistributedNeuromorphicServer,
};
use aarnn_rust::distributed::proto::stable_checkpoint_transfer_server::StableCheckpointTransferServer;
use aarnn_rust::distributed::proto::{
    HeartbeatRequest, JoinRequest, NetworkCommandResult, NetworkResources, Resources,
};
use aarnn_rust::distributed::{DistributedNode, ManagedNetwork};
use aarnn_rust::managed_stable_executor::ManagedStableExecutor;
use aarnn_rust::management::ReplicatedQuorumLeaseAuthority;
use aarnn_rust::migration_executor::{
    StableExecutorMigrationSettings, StableMigrationActivationGate,
    StableMigrationActivationRequest,
};
use aarnn_rust::migration_group::MigrationGroupSpec;
use aarnn_rust::migration_operation::{MigrationKind, MigrationOperation, MigrationProgress};
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlan, PlacementPlanner, PlacementRequest,
    ResourceObservation, ShardDemand,
};
use aarnn_rust::placement_registry::PersistedPlacementRegistry;
use aarnn_rust::runner::Runner;
use aarnn_rust::shard_executor::StableShardExecutor;
use aarnn_rust::sim::{Learning, NeuronModel};
use aarnn_rust::stable_executor_durable::StableExecutorDurableBridge;
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
use aarnn_rust::stable_runtime_bootstrap::{
    STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION, StablePartialWorkerBootstrapManifest,
    StableRuntimeBootstrapManifest, StableWorkerEndpoint,
};
use aarnn_rust::stable_worker::StableWorkerActivationCommand;
use aarnn_rust::stable_worker::{
    STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION, STABLE_EXECUTOR_PROFILE,
    STABLE_WORKER_ACTIVATION_SCHEMA_VERSION, StableWorkerCheckpointTransferReference,
};
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Request;

fn brain() -> BrainId {
    aarnn_rust::managed_durability::managed_brain_id("migration-network")
}

fn source_topology() -> TopologyGenerationModel {
    TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        (1..=4)
            .map(|id| NeuronRecord {
                id: aarnn_rust::deterministic::NeuronId::new(id).unwrap(),
            })
            .collect(),
        vec![SynapseRecord {
            id: SynapseId::new(11).unwrap(),
            source: aarnn_rust::deterministic::NeuronId::new(1).unwrap(),
            target: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
            delay_ticks: 0,
        }],
    )
    .unwrap()
}

fn source_plan(
    topology: &TopologyGenerationModel,
) -> aarnn_rust::topology_model::CompiledExecutionPlan {
    let first = ShardId::new(10).unwrap();
    let second = ShardId::new(20).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: first,
            weight_owner: first,
            release_owner: first,
            plasticity_owner: first,
        })
        .collect();
    compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        vec![
            VirtualShardAssignment {
                shard: first,
                components: vec![ComponentId::new(1).unwrap(), ComponentId::new(2).unwrap()],
                load: 2,
            },
            VirtualShardAssignment {
                shard: second,
                components: vec![ComponentId::new(3).unwrap(), ComponentId::new(4).unwrap()],
                load: 2,
            },
        ],
        ownership,
    )
    .unwrap()
}

fn source_executor() -> StableShardExecutor {
    let topology = source_topology();
    let plan = source_plan(&topology);
    StableShardExecutor::from_topology(
        brain(),
        &topology,
        plan,
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        32,
        128,
    )
    .unwrap()
}

fn source_runtime_manifest(
    root: &std::path::Path,
    checkpoint_root: std::path::PathBuf,
    target_plan: &PlacementPlan,
) -> StableRuntimeBootstrapManifest {
    let topology = source_topology();
    let plan = source_plan(&topology);
    StableRuntimeBootstrapManifest {
        schema_version: STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION,
        brain_id: brain(),
        topology_generation: topology.generation,
        topology_digest: topology.digest(),
        neurons: topology.neurons().cloned().collect(),
        synapses: topology.synapses().cloned().collect(),
        partition_generation: plan.partition_generation(),
        assignments: plan.assignments().to_vec(),
        ownership: plan.ownership_records().cloned().collect(),
        plan_digest: plan.digest(),
        checkpoint_id: EventId::new(2).unwrap(),
        checkpoint_root,
        owner_root: root.join("target-owner-from-manifest"),
        warm_root: root.join("target-warm-from-manifest"),
        lease_term: target_plan.lease_term,
        fencing_token: target_plan.fencing_token,
        stream_id: StreamId::new(31).unwrap(),
        max_payload: 128,
        max_input_events: 8,
        max_steps_per_poll: 8,
        threshold: FIXED_POINT_SCALE,
        weight: FIXED_POINT_SCALE,
        queue_capacity: 32,
        dedupe_capacity: 128,
        channel_state: b"initial-channel".to_vec(),
        sensory_targets: Vec::new(),
    }
}

fn cut() -> aarnn_rust::consistent_cut::ConsistentCut {
    let mut coordinator =
        ConsistentCutCoordinator::begin(1, ["source".to_owned()], ["source->target".to_owned()])
            .unwrap();
    coordinator
        .record_report(ParticipantReport {
            participant: "source".to_owned(),
            local_frontier: LogicalTag::ZERO,
            queued_min: None,
            in_flight_min: None,
            activity_epoch: 1,
        })
        .unwrap();
    coordinator
        .record_marker(ChannelMarker::new("source->target", 1, None, b"empty-channel").unwrap())
        .unwrap();
    coordinator.finalise().unwrap()
}

fn resource(node: &str, domain: &str) -> ResourceObservation {
    ResourceObservation {
        node_id: node.to_owned(),
        device_id: format!("{node}-cpu"),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
        failure_domain: domain.to_owned(),
        numerical_profiles: vec!["reference-cpu-v1".to_owned()],
        capacity_units: 100,
        reserved_capacity_units: 0,
        memory_bytes: 1_000_000,
        reserved_memory_bytes: 0,
        storage_bytes: 1_000_000,
        reserved_storage_bytes: 0,
        network_bytes_per_second: 1_000_000,
        reserved_network_bytes_per_second: 0,
        cpu_pressure_milli: 100,
        memory_pressure_milli: 100,
        network_pressure_milli: 100,
        thermal_pressure_milli: 100,
    }
}

fn placement(term: LeaseTerm, target: &str) -> PlacementPlan {
    PlacementPlanner
        .plan(PlacementRequest {
            brain_id: brain(),
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: term,
            fencing_token: term.raw(),
            effective_tag: LogicalTag::ZERO,
            demands: [10_u64, 20]
                .into_iter()
                .map(|id| ShardDemand {
                    shard_id: ShardId::new(id).unwrap(),
                    load_units: 1,
                    memory_bytes: 100,
                    checkpoint_bytes: 100,
                    network_bytes_per_second: 1,
                    zero_delay_component: None,
                    required_numerical_profile: "reference-cpu-v1".to_owned(),
                    preferred_node: Some(target.to_owned()),
                })
                .collect(),
            resources: vec![
                resource("source", "source-domain"),
                resource("target", "target-domain"),
            ],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: target.to_owned(),
            },
        })
        .unwrap()
}

fn placement_across_targets(
    term: LeaseTerm,
    first_target: &str,
    second_target: &str,
) -> PlacementPlan {
    PlacementPlanner
        .plan(PlacementRequest {
            brain_id: brain(),
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: term,
            fencing_token: term.raw(),
            effective_tag: LogicalTag::ZERO,
            demands: [(10_u64, first_target), (20_u64, second_target)]
                .into_iter()
                .map(|(id, preferred_node)| ShardDemand {
                    shard_id: ShardId::new(id).unwrap(),
                    load_units: 1,
                    memory_bytes: 100,
                    checkpoint_bytes: 100,
                    network_bytes_per_second: 1,
                    zero_delay_component: None,
                    required_numerical_profile: "reference-cpu-v1".to_owned(),
                    preferred_node: Some(preferred_node.to_owned()),
                })
                .collect(),
            resources: vec![
                resource("source", "source-domain"),
                resource(first_target, "target-domain-a"),
                resource(second_target, "target-domain-b"),
            ],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Automatic,
        })
        .unwrap()
}

fn registration_wire(
    network_id: &str,
    shard_ids: Vec<u64>,
) -> aarnn_rust::distributed::proto::StableExecutorRegistration {
    let brain_id = aarnn_rust::managed_durability::managed_brain_id(network_id).raw();
    aarnn_rust::distributed::proto::StableExecutorRegistration {
        schema_version: aarnn_rust::stable_worker::STABLE_WORKER_REGISTRATION_SCHEMA_VERSION,
        profile: STABLE_EXECUTOR_PROFILE.to_owned(),
        network_id: network_id.to_owned(),
        brain_id,
        topology_generation: 1,
        partition_generation: 1,
        topology_digest: "00".repeat(16),
        plan_digest: "11".repeat(16),
        shard_ids: shard_ids.clone(),
        owned_shard_ids: shard_ids.clone(),
        application_acks: shard_ids
            .into_iter()
            .map(
                |shard_id| aarnn_rust::distributed::proto::StableShardApplicationAck {
                    shard_id,
                    brain_id,
                    topology_generation: 1,
                    partition_generation: 1,
                    plan_digest: "11".repeat(16),
                    lease_term: 1,
                    fencing_token: 1,
                    applied_tick: 0,
                    applied_microstep: 0,
                    state_digest: "22".repeat(16),
                    durable_wal_sequence: 0,
                    durable_wal_sequence_present: true,
                    committed: true,
                },
            )
            .collect(),
        lease_term: 1,
        fencing_token: 1,
        current_tick: 0,
        current_microstep: 0,
        state_digest: "22".repeat(16),
        max_input_events: 8,
        max_steps_per_poll: 8,
        authoritative: true,
    }
}

struct RemoteMigrationFixture {
    node: DistributedNode,
    network: Arc<RwLock<ManagedNetwork>>,
    settings: StableExecutorMigrationSettings,
    target_plan: PlacementPlan,
    source_plan_digest: aarnn_rust::deterministic::StateDigest,
    total_bytes: u64,
    shutdown_tx: oneshot::Sender<()>,
    transfer_server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

async fn remote_migration_fixture(
    activation_gate: StableMigrationActivationGate,
) -> RemoteMigrationFixture {
    let root = std::env::temp_dir().join(format!("aarnn-live-failed-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let target_plan = placement(LeaseTerm::new(2).unwrap(), "target");
    let source_store = StableExecutorCheckpointStore::new(root.join("source-checkpoints")).unwrap();
    let bridge = StableExecutorDurableBridge::new(
        source_executor(),
        source_store,
        LeaseTerm::INITIAL,
        LeaseTerm::INITIAL.raw(),
        EventId::new(2).unwrap(),
        root.join("source-owner"),
        root.join("source-warm"),
        StreamId::new(3).unwrap(),
        128,
        b"initial-channel".to_vec(),
    )
    .unwrap();
    let source_plan_digest = source_executor().plan().digest();
    let sources = bridge
        .prepare_transfer_sources(
            EventId::new(100).unwrap(),
            "source",
            &cut(),
            source_plan_digest,
            32,
        )
        .unwrap();
    let total_bytes = sources
        .iter()
        .map(|source| source.manifest().total_bytes)
        .sum();
    let authority = Arc::new(Mutex::new(
        ReplicatedQuorumLeaseAuthority::open(
            ["source", "target", "witness"]
                .into_iter()
                .map(|member| (member.to_owned(), root.join(format!("{member}.authority")))),
            ["source", "target", "witness"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap(),
    ));
    let leases = authority
        .lock()
        .unwrap()
        .issue_leases(
            [ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
            "source",
        )
        .unwrap();
    let placement_registry = Arc::new(Mutex::new(
        PersistedPlacementRegistry::open(root.join("placement.json"), brain(), LeaseTerm::INITIAL)
            .unwrap(),
    ));
    let runtime = ManagedStableExecutor::new(bridge, EventId::new(2).unwrap(), 8, 8).unwrap();
    let target_checkpoint_root = root.join("target-checkpoint-transfer");
    let transfer_service =
        StableCheckpointTransferService::new("target", &target_checkpoint_root).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let transfer_address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let transfer_server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(StableCheckpointTransferServer::new(transfer_service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let mut managed = ManagedNetwork::new(
        "migration-network".to_owned(),
        Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        ),
        NetworkConfig::default(),
        NeuronModel::Lif,
        Learning::Stdp,
        LIFParams::default(),
        STDPParams::default(),
    );
    managed.register_stable_executor(runtime).unwrap();
    managed.playing = true;
    let network = Arc::new(RwLock::new(managed));
    let node = DistributedNode::new("source".to_owned(), true);
    node.state
        .write()
        .await
        .networks
        .insert("migration-network".to_owned(), network.clone());
    let settings = StableExecutorMigrationSettings {
        consistent_cut: cut(),
        source_node: "source".to_owned(),
        destination_root: root.join("target-owner"),
        warm_root: root.join("target-warm"),
        authority,
        destination_nodes: [
            (ShardId::new(10).unwrap(), "target".to_owned()),
            (ShardId::new(20).unwrap(), "target".to_owned()),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
        source_fencing_tokens: leases
            .into_iter()
            .map(|(shard, lease)| (shard, lease.fencing_token))
            .collect(),
        placement_registry,
        target_plan: target_plan.clone(),
        stream_id: StreamId::new(31).unwrap(),
        max_payload: 128,
        frame_bytes: 32,
        destination_endpoints: [("target".to_owned(), format!("http://{transfer_address}"))]
            .into_iter()
            .collect(),
        target_activation_commands: [(
            "target".to_owned(),
            StableWorkerActivationCommand::new(
                "failed-gate-target",
                704,
                brain().raw(),
                "migration-network",
                "target",
                "{}",
            )
            .unwrap(),
        )]
        .into_iter()
        .collect(),
        activation_gate: Some(activation_gate),
    };
    node.register_stable_network_migration_executor("migration-network", settings.clone())
        .await
        .unwrap();
    RemoteMigrationFixture {
        node,
        network,
        settings,
        target_plan,
        source_plan_digest,
        total_bytes,
        shutdown_tx,
        transfer_server,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_owned_registry_dispatches_against_hosted_stable_runtime() {
    let root = std::env::temp_dir().join(format!("aarnn-live-registration-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let target_plan = placement(LeaseTerm::new(2).unwrap(), "target");
    let source_store = StableExecutorCheckpointStore::new(root.join("source-checkpoints")).unwrap();
    let bridge = StableExecutorDurableBridge::new(
        source_executor(),
        source_store,
        LeaseTerm::INITIAL,
        LeaseTerm::INITIAL.raw(),
        EventId::new(2).unwrap(),
        root.join("source-owner"),
        root.join("source-warm"),
        StreamId::new(3).unwrap(),
        128,
        b"initial-channel".to_vec(),
    )
    .unwrap();
    let source_plan_digest = source_executor().plan().digest();
    let sources = bridge
        .prepare_transfer_sources(
            EventId::new(100).unwrap(),
            "source",
            &cut(),
            source_plan_digest,
            32,
        )
        .unwrap();
    let total_bytes = sources
        .iter()
        .map(|source| source.manifest().total_bytes)
        .sum();

    let authority = Arc::new(Mutex::new(
        ReplicatedQuorumLeaseAuthority::open(
            ["source", "target", "witness"]
                .into_iter()
                .map(|member| (member.to_owned(), root.join(format!("{member}.authority")))),
            ["source", "target", "witness"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap(),
    ));
    let leases = authority
        .lock()
        .unwrap()
        .issue_leases(
            [ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
            "source",
        )
        .unwrap();
    let placement_registry = Arc::new(Mutex::new(
        PersistedPlacementRegistry::open(root.join("placement.json"), brain(), LeaseTerm::INITIAL)
            .unwrap(),
    ));

    let runtime = ManagedStableExecutor::new(bridge, EventId::new(2).unwrap(), 8, 8).unwrap();
    let target_checkpoint_root = root.join("target-checkpoint-transfer");
    let transfer_service =
        StableCheckpointTransferService::new("target", &target_checkpoint_root).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let transfer_address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let transfer_server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(StableCheckpointTransferServer::new(transfer_service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    let mut managed = ManagedNetwork::new(
        "migration-network".to_owned(),
        Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        ),
        NetworkConfig::default(),
        NeuronModel::Lif,
        Learning::Stdp,
        LIFParams::default(),
        STDPParams::default(),
    );
    managed.register_stable_executor(runtime).unwrap();
    managed.playing = true;
    let network = Arc::new(RwLock::new(managed));
    let node = DistributedNode::new("source".to_owned(), true);
    node.state
        .write()
        .await
        .networks
        .insert("migration-network".to_owned(), network.clone());

    let expected_target_plan_digest = target_plan.digest();
    let settings = StableExecutorMigrationSettings {
        consistent_cut: cut(),
        source_node: "source".to_owned(),
        destination_root: root.join("target-owner"),
        warm_root: root.join("target-warm"),
        authority,
        destination_nodes: [
            (ShardId::new(10).unwrap(), "target".to_owned()),
            (ShardId::new(20).unwrap(), "target".to_owned()),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
        source_fencing_tokens: leases
            .into_iter()
            .map(|(shard, lease)| (shard, lease.fencing_token))
            .collect(),
        placement_registry,
        target_plan: target_plan.clone(),
        stream_id: StreamId::new(31).unwrap(),
        max_payload: 128,
        frame_bytes: 32,
        destination_endpoints: [("target".to_owned(), format!("http://{transfer_address}"))]
            .into_iter()
            .collect(),
        target_activation_commands: [(
            "target".to_owned(),
            StableWorkerActivationCommand::new(
                "live-registration-target",
                401,
                brain().raw(),
                "migration-network",
                "target",
                "{}",
            )
            .unwrap(),
        )]
        .into_iter()
        .collect(),
        activation_gate: Some(Arc::new(
            move |request: StableMigrationActivationRequest| {
                assert_eq!(request.operation_id, 401);
                assert_eq!(request.brain_id, brain());
                assert_eq!(request.target_plan.digest(), expected_target_plan_digest);
                assert_eq!(request.checkpoint_references.len(), 1);
                assert!(request.checkpoint_references.contains_key("target"));
                assert_eq!(request.activation_commands.len(), 1);
                assert_eq!(
                    request.activation_commands["target"].checkpoint_transfer,
                    request.checkpoint_references.get("target").cloned()
                );
                Ok(())
            },
        )),
    };
    let registered_brain = node
        .register_stable_network_migration_executor("migration-network", settings)
        .await
        .unwrap();
    assert_eq!(registered_brain, brain());

    let operation = MigrationOperation {
        operation_id: 401,
        request_id: "live-registration".to_owned(),
        idempotency_key: "live-registration".to_owned(),
        brain_id: brain(),
        kind: MigrationKind::MigrateBrain,
        source_plan_digest,
        target_plan_digest: target_plan.digest(),
        phase: aarnn_rust::migration_operation::MigrationPhase::Prepared,
        progress: MigrationProgress {
            completed_shards: 0,
            total_shards: 2,
            transferred_bytes: 0,
            total_bytes,
            cut_tag: None,
        },
        resource_version: 1,
        error_code: None,
    };
    let group = MigrationGroupSpec {
        brain_id: brain(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: TopologyGeneration::INITIAL,
        partition_generation: PartitionGeneration::INITIAL,
        shard_ids: vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
    };
    let receipt = node
        .migration_executor_registry()
        .dispatch(operation, group)
        .await
        .unwrap();
    assert_eq!(receipt.transferred_bytes, total_bytes);
    let network = network.read().await;
    assert!(
        !network.playing,
        "successful migration must leave source paused"
    );
    assert_eq!(
        network.stable_executor.as_ref().unwrap().lease_term(),
        LeaseTerm::new(2).unwrap()
    );
    assert!(
        StableExecutorCheckpointStore::new(&target_checkpoint_root)
            .unwrap()
            .verify(EventId::new(2).unwrap())
            .is_ok(),
        "live migration must transfer the immutable checkpoint before cutover"
    );
    drop(network);
    let _ = shutdown_tx.send(());
    tokio::time::timeout(std::time::Duration::from_secs(2), transfer_server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_remote_activation_gate_preserves_source_authority_and_placement() {
    let gate_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate_called_by_executor = gate_called.clone();
    let fixture = remote_migration_fixture(Arc::new(move |_request| {
        gate_called_by_executor.store(true, std::sync::atomic::Ordering::SeqCst);
        Err("target registration was rejected".to_owned())
    }))
    .await;

    let authority_before = fixture
        .settings
        .authority
        .lock()
        .unwrap()
        .authority()
        .clone();
    let placement_before = fixture
        .settings
        .placement_registry
        .lock()
        .unwrap()
        .state()
        .clone();
    let source_term_before = fixture
        .network
        .read()
        .await
        .stable_executor
        .as_ref()
        .unwrap()
        .lease_term();

    let operation = MigrationOperation {
        operation_id: 704,
        request_id: "failed-remote-activation".to_owned(),
        idempotency_key: "failed-remote-activation".to_owned(),
        brain_id: brain(),
        kind: MigrationKind::MigrateBrain,
        source_plan_digest: fixture.source_plan_digest,
        target_plan_digest: fixture.target_plan.digest(),
        phase: aarnn_rust::migration_operation::MigrationPhase::Prepared,
        progress: MigrationProgress {
            completed_shards: 0,
            total_shards: 2,
            transferred_bytes: 0,
            total_bytes: fixture.total_bytes,
            cut_tag: None,
        },
        resource_version: 1,
        error_code: None,
    };
    let group = MigrationGroupSpec {
        brain_id: brain(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: TopologyGeneration::INITIAL,
        partition_generation: PartitionGeneration::INITIAL,
        shard_ids: vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
    };
    let error = fixture
        .node
        .migration_executor_registry()
        .dispatch(operation, group)
        .await
        .expect_err("a rejected target activation must abort migration");
    assert!(error.contains("target registration was rejected"));
    assert!(gate_called.load(std::sync::atomic::Ordering::SeqCst));

    let authority_after = fixture
        .settings
        .authority
        .lock()
        .unwrap()
        .authority()
        .clone();
    assert_eq!(
        serde_json::to_vec(&authority_after).unwrap(),
        serde_json::to_vec(&authority_before).unwrap(),
        "target activation failure must not promote destination leases"
    );
    assert_eq!(
        fixture
            .settings
            .placement_registry
            .lock()
            .unwrap()
            .state()
            .clone(),
        placement_before,
        "target activation failure must not publish a destination placement"
    );
    let network = fixture.network.read().await;
    assert!(!network.playing, "a failed migration remains safely paused");
    assert_eq!(
        network.stable_executor.as_ref().unwrap().lease_term(),
        source_term_before,
        "target activation failure must not fence the source authority"
    );
    drop(network);
    let _ = fixture.shutdown_tx.send(());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), fixture.transfer_server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn distributed_activation_gate_waits_for_command_and_registration_evidence() {
    let node = DistributedNode::new("source".to_owned(), true);
    let target_plan = placement(LeaseTerm::new(2).unwrap(), "target");
    let brain_id = brain();
    let operation_id = 702;
    let source_plan_digest = source_executor().plan().digest();
    let checkpoint_reference = StableWorkerCheckpointTransferReference {
        schema_version: 1,
        transfer_id: 700,
        checkpoint_id: 2,
        brain_id: brain_id.raw(),
        lease_term: target_plan.lease_term.raw(),
        partition_generation: target_plan.partition_generation.raw(),
        plan_digest: source_plan_digest.to_string(),
        payload_digest: "11".repeat(16),
        manifest_digest: "22".repeat(16),
    };
    let mut command = StableWorkerActivationCommand::new(
        "gate-target-activation",
        operation_id,
        brain_id.raw(),
        "migration-network",
        "target",
        "{}",
    )
    .unwrap();
    command
        .bind_checkpoint_transfer(checkpoint_reference.clone())
        .unwrap();

    node.join(Request::new(aarnn_rust::distributed::proto::JoinRequest {
        node_id: "target".to_owned(),
        address: "127.0.0.1:65534".to_owned(),
        resources: Some(Resources::default()),
        network_resources: BTreeMap::from([(
            "migration-network".to_owned(),
            NetworkResources {
                num_neurons: 4,
                layer_neuron_counts: BTreeMap::from([(0, 4)]).into_iter().collect(),
                avg_step_time_ms: 1.0,
            },
        )])
        .into_iter()
        .collect(),
        stable_executors: Vec::new(),
        stable_executor_capabilities: vec![
            aarnn_rust::distributed::proto::StableExecutorCapability {
                schema_version: STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
                profile: STABLE_EXECUTOR_PROFILE.to_owned(),
                activation_schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
                max_input_events: 8,
                max_steps_per_poll: 8,
            },
        ],
    }))
    .await
    .unwrap();
    // The legacy network-resource join path may create a compatibility
    // distribution entry. This test exercises the explicit stable-worker
    // activation boundary, so remove that provisional legacy placement before
    // presenting the first stable registration.
    node.state
        .write()
        .await
        .network_registry
        .get_mut("migration-network")
        .expect("joined network status")
        .distribution
        .clear();

    let mut registration = registration_wire("migration-network", vec![10, 20]);
    registration.lease_term = target_plan.lease_term.raw();
    registration.fencing_token = target_plan.fencing_token;
    registration.plan_digest = source_plan_digest.to_string();
    for ack in &mut registration.application_acks {
        ack.plan_digest = source_plan_digest.to_string();
        ack.lease_term = target_plan.lease_term.raw();
        ack.fencing_token = target_plan.fencing_token;
    }
    let heartbeat = |stable_executors, command_results| HeartbeatRequest {
        node_id: "target".to_owned(),
        resources: Some(Resources::default()),
        network_resources: BTreeMap::from([(
            "migration-network".to_owned(),
            NetworkResources {
                num_neurons: 4,
                layer_neuron_counts: BTreeMap::from([(0, 4)]).into_iter().collect(),
                avg_step_time_ms: 1.0,
            },
        )])
        .into_iter()
        .collect(),
        stable_executors,
        stable_executor_capabilities: vec![
            aarnn_rust::distributed::proto::StableExecutorCapability {
                schema_version: STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
                profile: STABLE_EXECUTOR_PROFILE.to_owned(),
                activation_schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
                max_input_events: 8,
                max_steps_per_poll: 8,
            },
        ],
        command_results,
    };

    let gate = node.stable_migration_activation_gate(
        std::time::Duration::from_secs(2),
        std::time::Duration::from_millis(5),
    );
    let gate_request = StableMigrationActivationRequest {
        operation_id,
        brain_id,
        target_plan: target_plan.clone(),
        checkpoint_references: [("target".to_owned(), checkpoint_reference)]
            .into_iter()
            .collect(),
        activation_commands: [("target".to_owned(), command.clone())]
            .into_iter()
            .collect(),
    };
    let gate_task = tokio::task::spawn_blocking(move || gate(gate_request));

    for _ in 0..100 {
        // The gate is a synchronous migration callback and the production
        // registry invokes it from spawn_blocking. Wait for the specific
        // activation command, rather than any legacy compatibility command
        // queued by the worker join path.
        if gate_task.is_finished() {
            panic!(
                "activation gate exited before delivery: {:?}",
                gate_task.await
            );
        }
        let queued = node
            .state
            .read()
            .await
            .pending_commands
            .get("target")
            .is_some_and(|commands| {
                commands.iter().any(|command| {
                    command.r#type
                        == aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                            as i32
                })
            });
        if queued {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    if gate_task.is_finished() {
        panic!(
            "activation gate exited before heartbeat delivery: {:?}",
            gate_task.await
        );
    }
    let delivered = node
        .heartbeat(Request::new(heartbeat(Vec::new(), Vec::new())))
        .await
        .unwrap()
        .into_inner()
        .commands;
    let delivered_command_wire = delivered
        .iter()
        .find(|command| {
            command.r#type
                == aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                    as i32
        })
        .expect("activation command was delivered");
    let delivered_command: StableWorkerActivationCommand =
        serde_json::from_slice(&delivered_command_wire.config_json).unwrap();
    assert_eq!(delivered_command, command);
    let result = NetworkCommandResult {
        command_type:
            aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                as i32,
        network_id: delivered_command.network_id.clone(),
        request_id: delivered_command.request_id.clone(),
        manifest_digest: delivered_command.manifest_digest.clone(),
        accepted: true,
        error: String::new(),
        brain_id: brain_id.raw(),
        placement_idempotency_key: String::new(),
    };
    node.heartbeat(Request::new(heartbeat(vec![registration], vec![result])))
        .await
        .unwrap();

    gate_task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn distributed_activation_gate_waits_for_all_target_workers() {
    let node = DistributedNode::new("source".to_owned(), true);
    let first_target = "target-a";
    let second_target = "target-b";
    let target_plan =
        placement_across_targets(LeaseTerm::new(2).unwrap(), first_target, second_target);
    assert_eq!(
        target_plan
            .placements
            .iter()
            .map(|placement| placement.active_node.as_str())
            .collect::<Vec<_>>(),
        vec![first_target, second_target]
    );
    let brain_id = brain();
    let operation_id = 703;
    let source_plan_digest = source_executor().plan().digest();

    let mut activation_commands = BTreeMap::new();
    let mut checkpoint_references = BTreeMap::new();
    for (index, target_node) in [first_target, second_target].into_iter().enumerate() {
        let checkpoint_reference = StableWorkerCheckpointTransferReference {
            schema_version: 1,
            transfer_id: 710 + index as u64,
            checkpoint_id: 2,
            brain_id: brain_id.raw(),
            lease_term: target_plan.lease_term.raw(),
            partition_generation: target_plan.partition_generation.raw(),
            plan_digest: source_plan_digest.to_string(),
            payload_digest: format!("{:02x}", 0x31 + index).repeat(16),
            manifest_digest: format!("{:02x}", 0x41 + index).repeat(16),
        };
        let mut command = StableWorkerActivationCommand::new(
            format!("gate-{target_node}-activation"),
            operation_id,
            brain_id.raw(),
            "migration-network",
            target_node,
            "{}",
        )
        .unwrap();
        command
            .bind_checkpoint_transfer(checkpoint_reference.clone())
            .unwrap();
        checkpoint_references.insert(target_node.to_owned(), checkpoint_reference);
        activation_commands.insert(target_node.to_owned(), command);
    }

    for target_node in [first_target, second_target] {
        node.join(Request::new(aarnn_rust::distributed::proto::JoinRequest {
            node_id: target_node.to_owned(),
            address: "127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: BTreeMap::from([(
                "migration-network".to_owned(),
                NetworkResources {
                    num_neurons: 4,
                    layer_neuron_counts: BTreeMap::from([(0, 4)]).into_iter().collect(),
                    avg_step_time_ms: 1.0,
                },
            )])
            .into_iter()
            .collect(),
            stable_executors: Vec::new(),
            stable_executor_capabilities: vec![
                aarnn_rust::distributed::proto::StableExecutorCapability {
                    schema_version: STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
                    profile: STABLE_EXECUTOR_PROFILE.to_owned(),
                    activation_schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
                    max_input_events: 8,
                    max_steps_per_poll: 8,
                },
            ],
        }))
        .await
        .unwrap();
    }
    node.state
        .write()
        .await
        .network_registry
        .get_mut("migration-network")
        .expect("joined network status")
        .distribution
        .clear();

    let registration_for = |shard_id: u64| {
        let mut registration = registration_wire("migration-network", vec![10, 20]);
        registration.owned_shard_ids = vec![shard_id];
        registration
            .application_acks
            .retain(|ack| ack.shard_id == shard_id);
        registration.lease_term = target_plan.lease_term.raw();
        registration.fencing_token = target_plan.fencing_token;
        registration.plan_digest = source_plan_digest.to_string();
        for ack in &mut registration.application_acks {
            ack.plan_digest = source_plan_digest.to_string();
            ack.lease_term = target_plan.lease_term.raw();
            ack.fencing_token = target_plan.fencing_token;
        }
        registration
    };

    let heartbeat = |node_id: &str, stable_executors, command_results| HeartbeatRequest {
        node_id: node_id.to_owned(),
        resources: Some(Resources::default()),
        network_resources: BTreeMap::from([(
            "migration-network".to_owned(),
            NetworkResources {
                num_neurons: 4,
                layer_neuron_counts: BTreeMap::from([(0, 4)]).into_iter().collect(),
                avg_step_time_ms: 1.0,
            },
        )])
        .into_iter()
        .collect(),
        stable_executors,
        stable_executor_capabilities: vec![
            aarnn_rust::distributed::proto::StableExecutorCapability {
                schema_version: STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
                profile: STABLE_EXECUTOR_PROFILE.to_owned(),
                activation_schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
                max_input_events: 8,
                max_steps_per_poll: 8,
            },
        ],
        command_results,
    };

    let gate = node.stable_migration_activation_gate(
        std::time::Duration::from_secs(2),
        std::time::Duration::from_millis(5),
    );
    let gate_request = StableMigrationActivationRequest {
        operation_id,
        brain_id,
        target_plan: target_plan.clone(),
        checkpoint_references,
        activation_commands,
    };
    let gate_task = tokio::task::spawn_blocking(move || gate(gate_request));

    for _ in 0..100 {
        if gate_task.is_finished() {
            panic!(
                "activation gate exited before both targets were ready: {:?}",
                gate_task.await
            );
        }
        let state = node.state.read().await;
        let both_queued = [first_target, second_target].into_iter().all(|target_node| {
            state
                .pending_commands
                .get(target_node)
                .is_some_and(|commands| {
                    commands.iter().any(|command| {
                        command.r#type
                            == aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                                as i32
                    })
                })
        });
        drop(state);
        if both_queued {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        !gate_task.is_finished(),
        "activation gate timed out before heartbeat delivery"
    );

    for (target_node, shard_id) in [(first_target, 10_u64), (second_target, 20_u64)] {
        let delivered = node
            .heartbeat(Request::new(heartbeat(target_node, Vec::new(), Vec::new())))
            .await
            .unwrap()
            .into_inner()
            .commands;
        let delivered_command_wire = delivered
            .iter()
            .find(|command| {
                command.r#type
                    == aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                        as i32
            })
            .expect("activation command was delivered");
        let delivered_command: StableWorkerActivationCommand =
            serde_json::from_slice(&delivered_command_wire.config_json).unwrap();
        let result = NetworkCommandResult {
            command_type:
                aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                    as i32,
            network_id: delivered_command.network_id.clone(),
            request_id: delivered_command.request_id.clone(),
            manifest_digest: delivered_command.manifest_digest.clone(),
            accepted: true,
            error: String::new(),
            brain_id: brain_id.raw(),
            placement_idempotency_key: String::new(),
        };
        node.heartbeat(Request::new(heartbeat(
            target_node,
            vec![registration_for(shard_id)],
            vec![result],
        )))
        .await
        .unwrap();
    }

    gate_task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_migration_activates_real_target_worker_and_reports_durable_registration() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-live-real-target-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();

    let target_plan = placement(LeaseTerm::new(2).unwrap(), "target");
    let source_checkpoint_root = root.join("source-checkpoints");
    let source_store = StableExecutorCheckpointStore::new(&source_checkpoint_root).unwrap();
    let bridge = StableExecutorDurableBridge::new(
        source_executor(),
        source_store,
        LeaseTerm::INITIAL,
        LeaseTerm::INITIAL.raw(),
        EventId::new(2).unwrap(),
        root.join("source-owner"),
        root.join("source-warm"),
        StreamId::new(31).unwrap(),
        128,
        b"initial-channel".to_vec(),
    )
    .unwrap();
    let source_plan_digest = source_executor().plan().digest();
    let sources = bridge
        .prepare_transfer_sources(
            EventId::new(500).unwrap(),
            "source",
            &cut(),
            source_plan_digest,
            32,
        )
        .unwrap();
    let total_bytes = sources
        .iter()
        .map(|source| source.manifest().total_bytes)
        .sum();

    let authority = Arc::new(Mutex::new(
        ReplicatedQuorumLeaseAuthority::open(
            ["source", "target", "witness"]
                .into_iter()
                .map(|member| (member.to_owned(), root.join(format!("{member}.authority")))),
            ["source", "target", "witness"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap(),
    ));
    let leases = authority
        .lock()
        .unwrap()
        .issue_leases(
            [ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
            "source",
        )
        .unwrap();
    let placement_registry = Arc::new(Mutex::new(
        PersistedPlacementRegistry::open(root.join("placement.json"), brain(), LeaseTerm::INITIAL)
            .unwrap(),
    ));

    let mut managed = ManagedNetwork::new(
        "migration-network".to_owned(),
        Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        ),
        NetworkConfig::default(),
        NeuronModel::Lif,
        Learning::Stdp,
        LIFParams::default(),
        STDPParams::default(),
    );
    managed
        .register_stable_executor(
            ManagedStableExecutor::new(bridge, EventId::new(2).unwrap(), 8, 8).unwrap(),
        )
        .unwrap();
    managed.playing = true;
    let network = Arc::new(RwLock::new(managed));
    let source = DistributedNode::new("source".to_owned(), true);
    source
        .state
        .write()
        .await
        .networks
        .insert("migration-network".to_owned(), network.clone());
    let target = DistributedNode::new("target".to_owned(), false);

    let target_checkpoint_root = root.join("target-checkpoint-transfer");
    let target_worker_root = root.join("target-worker-state");
    let target_transfer_service =
        StableCheckpointTransferService::new("target", &target_checkpoint_root).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = listener.local_addr().unwrap();
    let (server_shutdown_tx, server_shutdown_rx) = oneshot::channel();
    let target_for_server = target.clone();
    let target_server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(DistributedNeuromorphicServer::new(target_for_server))
            .add_service(StableCheckpointTransferServer::new(target_transfer_service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = server_shutdown_rx.await;
            })
            .await
    });

    let target_capabilities = target.get_stable_executor_capabilities().await;
    source
        .join(Request::new(JoinRequest {
            node_id: "target".to_owned(),
            address: format!("http://{target_address}"),
            resources: Some(Resources::default()),
            network_resources: HashMap::from([(
                "migration-network".to_owned(),
                NetworkResources {
                    num_neurons: 4,
                    layer_neuron_counts: HashMap::from([(0, 4)]),
                    avg_step_time_ms: 1.0,
                },
            )]),
            stable_executors: Vec::new(),
            stable_executor_capabilities: target_capabilities,
        }))
        .await
        .unwrap();
    source
        .state
        .write()
        .await
        .network_registry
        .get_mut("migration-network")
        .unwrap()
        .distribution
        .clear();

    let runtime_manifest = source_runtime_manifest(&root, source_checkpoint_root, &target_plan);
    let target_manifest = StablePartialWorkerBootstrapManifest::from_authoritative_state(
        runtime_manifest,
        target_plan.clone(),
        "target",
        vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
        vec!["source".to_owned()],
        target_worker_root.join("receiver.json"),
        target_worker_root.join("outbound.json"),
        64,
        16,
        Vec::<StableWorkerEndpoint>::new(),
    )
    .unwrap();
    let target_activation = target_manifest
        .activation_command("real-target-activation", 705, "migration-network")
        .unwrap();

    let settings = StableExecutorMigrationSettings {
        consistent_cut: cut(),
        source_node: "source".to_owned(),
        destination_root: root.join("destination-owner"),
        warm_root: root.join("destination-warm"),
        authority,
        destination_nodes: [
            (ShardId::new(10).unwrap(), "target".to_owned()),
            (ShardId::new(20).unwrap(), "target".to_owned()),
        ]
        .into_iter()
        .collect(),
        source_fencing_tokens: leases
            .into_iter()
            .map(|(shard, lease)| (shard, lease.fencing_token))
            .collect(),
        placement_registry,
        target_plan: target_plan.clone(),
        stream_id: StreamId::new(31).unwrap(),
        max_payload: 128,
        frame_bytes: 32,
        destination_endpoints: [("target".to_owned(), format!("http://{target_address}"))]
            .into_iter()
            .collect(),
        target_activation_commands: [("target".to_owned(), target_activation)]
            .into_iter()
            .collect(),
        activation_gate: Some(source.stable_migration_activation_gate(
            std::time::Duration::from_secs(5),
            std::time::Duration::from_millis(2),
        )),
    };
    source
        .register_stable_network_migration_executor("migration-network", settings)
        .await
        .unwrap();

    let (driver_stop_tx, mut driver_stop_rx) = watch::channel(false);
    let source_for_driver = source.clone();
    let target_for_driver = target.clone();
    let driver_checkpoint_root = target_checkpoint_root.clone();
    let driver_worker_root = target_worker_root.clone();
    let delivered_activation = Arc::new(Mutex::new(None::<StableWorkerActivationCommand>));
    let delivered_activation_for_driver = delivered_activation.clone();
    let target_driver = tokio::spawn(async move {
        let mut command_results = Vec::new();
        loop {
            if *driver_stop_rx.borrow() {
                break;
            }
            let request = HeartbeatRequest {
                node_id: "target".to_owned(),
                resources: Some(Resources::default()),
                network_resources: HashMap::from([(
                    "migration-network".to_owned(),
                    NetworkResources {
                        num_neurons: 4,
                        layer_neuron_counts: HashMap::from([(0, 4)]),
                        avg_step_time_ms: 1.0,
                    },
                )]),
                stable_executors: target_for_driver.get_stable_executor_registrations().await,
                stable_executor_capabilities: target_for_driver
                    .get_stable_executor_capabilities()
                    .await,
                command_results: std::mem::take(&mut command_results),
            };
            let response = source_for_driver
                .heartbeat(Request::new(request))
                .await
                .map_err(|error| error.to_string())?
                .into_inner();
            for wire_command in response.commands {
                if wire_command.r#type
                    != aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                        as i32
                {
                    continue;
                }
                let command: StableWorkerActivationCommand =
                    serde_json::from_slice(&wire_command.config_json)
                        .map_err(|error| error.to_string())?;
                delivered_activation_for_driver
                    .lock()
                    .unwrap()
                    .replace(command.clone());
                let result = match target_for_driver
                    .activate_stable_worker_with_roots(
                        command.clone(),
                        driver_checkpoint_root.clone(),
                        driver_worker_root.clone(),
                    )
                    .await
                {
                    Ok(()) => NetworkCommandResult {
                        command_type: wire_command.r#type,
                        network_id: command.network_id.clone(),
                        request_id: command.request_id.clone(),
                        manifest_digest: command.manifest_digest.clone(),
                        accepted: true,
                        error: String::new(),
                        brain_id: command.brain_id,
                        placement_idempotency_key: command.placement_idempotency_key.clone(),
                    },
                    Err(error) => NetworkCommandResult {
                        command_type: wire_command.r#type,
                        network_id: command.network_id.clone(),
                        request_id: command.request_id.clone(),
                        manifest_digest: command.manifest_digest.clone(),
                        accepted: false,
                        error,
                        brain_id: command.brain_id,
                        placement_idempotency_key: command.placement_idempotency_key.clone(),
                    },
                };
                command_results.push(result);
            }
            tokio::select! {
                changed = driver_stop_rx.changed() => {
                    if changed.is_err() || *driver_stop_rx.borrow() {
                        break;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(2)) => {}
            }
        }
        Ok::<(), String>(())
    });

    let operation = MigrationOperation {
        operation_id: 705,
        request_id: "real-target-migration".to_owned(),
        idempotency_key: "real-target-migration".to_owned(),
        brain_id: brain(),
        kind: MigrationKind::MigrateBrain,
        source_plan_digest,
        target_plan_digest: target_plan.digest(),
        phase: aarnn_rust::migration_operation::MigrationPhase::Prepared,
        progress: MigrationProgress {
            completed_shards: 0,
            total_shards: 2,
            transferred_bytes: 0,
            total_bytes,
            cut_tag: None,
        },
        resource_version: 1,
        error_code: None,
    };
    let group = MigrationGroupSpec {
        brain_id: brain(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: TopologyGeneration::INITIAL,
        partition_generation: PartitionGeneration::INITIAL,
        shard_ids: vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
    };
    let receipt = source
        .migration_executor_registry()
        .dispatch(operation, group)
        .await
        .unwrap();
    assert_eq!(receipt.transferred_bytes, total_bytes);

    driver_stop_tx.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), target_driver)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let registrations = target.get_stable_executor_registrations().await;
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].network_id, "migration-network");
    assert_eq!(registrations[0].owned_shard_ids, vec![10, 20]);
    assert_eq!(registrations[0].lease_term, target_plan.lease_term.raw());
    assert_eq!(registrations[0].fencing_token, target_plan.fencing_token);
    assert!(target_worker_root.join("receiver.json").exists());
    assert!(
        StableExecutorCheckpointStore::new(&target_checkpoint_root)
            .unwrap()
            .verify(EventId::new(2).unwrap())
            .is_ok()
    );

    // Reopen the durable receiver through a fresh node object to model a
    // target process restart after checkpoint transfer. The same activation
    // command is safe to retry because the receiver document and activation
    // identity are both durable; a second retry is idempotent in the new
    // process as well.
    let replay_command = delivered_activation
        .lock()
        .unwrap()
        .clone()
        .expect("target driver must have received the activation command");
    let restarted_target = DistributedNode::new("target".to_owned(), false);
    restarted_target
        .activate_stable_worker_with_roots(
            replay_command.clone(),
            target_checkpoint_root.clone(),
            target_worker_root.clone(),
        )
        .await
        .unwrap();
    restarted_target
        .activate_stable_worker_with_roots(
            replay_command,
            target_checkpoint_root.clone(),
            target_worker_root.clone(),
        )
        .await
        .unwrap();
    let restarted_registrations = restarted_target.get_stable_executor_registrations().await;
    assert_eq!(restarted_registrations.len(), 1);
    assert_eq!(restarted_registrations[0].owned_shard_ids, vec![10, 20]);
    assert_eq!(
        restarted_registrations[0].lease_term,
        target_plan.lease_term.raw()
    );

    let _ = server_shutdown_tx.send(());
    tokio::time::timeout(std::time::Duration::from_secs(2), target_server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
