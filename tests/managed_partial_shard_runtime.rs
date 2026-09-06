use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::causal::CausalEvent;
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId,
    PartitionGeneration, ShardId, StateDigest, SynapseId, TopologyGeneration,
};
use aarnn_rust::distributed::DistributedNode;
use aarnn_rust::managed_partial_shard_runtime::ManagedPartialShardRuntime;
use aarnn_rust::partial_shard_executor::PartialShardExecutor;
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlanner, PlacementRequest, ResourceObservation,
    ShardDemand,
};
use aarnn_rust::placement_registry::{PlacementApplyRequest, PlacementRegistry};
use aarnn_rust::shard_executor::{RoutedCausalEvent, StableShardExecutor};
use aarnn_rust::stable_outbound::StableOutboundLog;
use aarnn_rust::stable_shard_dispatch::StableShardDispatcher;
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;

fn topology_and_plan() -> (
    TopologyGenerationModel,
    aarnn_rust::topology_model::CompiledExecutionPlan,
    StableShardExecutor,
    BrainId,
) {
    let topology = TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        (1..=4)
            .map(|id| NeuronRecord {
                id: NeuronId::new(id).unwrap(),
            })
            .collect(),
        vec![
            SynapseRecord {
                id: SynapseId::new(11).unwrap(),
                source: NeuronId::new(1).unwrap(),
                target: NeuronId::new(2).unwrap(),
                delay_ticks: 0,
            },
            SynapseRecord {
                id: SynapseId::new(12).unwrap(),
                source: NeuronId::new(2).unwrap(),
                target: NeuronId::new(3).unwrap(),
                delay_ticks: 1,
            },
            SynapseRecord {
                id: SynapseId::new(13).unwrap(),
                source: NeuronId::new(3).unwrap(),
                target: NeuronId::new(4).unwrap(),
                delay_ticks: 0,
            },
        ],
    )
    .unwrap();
    let first = ShardId::new(1).unwrap();
    let second = ShardId::new(2).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| {
            let owner = if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            };
            OwnershipRecord {
                synapse: synapse.id,
                terminal_owner: owner,
                weight_owner: owner,
                release_owner: owner,
                plasticity_owner: owner,
            }
        })
        .collect::<Vec<_>>();
    let plan = compile_execution_plan(
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
    .unwrap();
    let brain = BrainId::new(901).unwrap();
    let complete = StableShardExecutor::from_topology(
        brain,
        &topology,
        plan.clone(),
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        16,
        128,
    )
    .unwrap();
    (topology, plan, complete, brain)
}

fn input(event_id: u64, target: u64) -> RoutedCausalEvent {
    let target = NeuronId::new(target).unwrap();
    let event = EventId::new(event_id).unwrap();
    RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event.raw(),
            ),
            id: event,
            payload: serde_json::to_vec(&aarnn_rust::authoritative_shard::StableTransitionInput {
                schema_version: aarnn_rust::authoritative_shard::BIOLOGICAL_STATE_SCHEMA_VERSION,
                source: None,
                target,
                charge: 1,
                delay_ticks: 0,
            })
            .unwrap(),
            original_tag: LogicalTag::ZERO,
            deferred_from_nonconvergence: false,
        },
    }
}

fn resource(node: &str) -> ResourceObservation {
    ResourceObservation {
        node_id: node.to_owned(),
        device_id: format!("{node}-cpu"),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
        failure_domain: format!("domain-{node}"),
        numerical_profiles: vec!["reference-cpu-v1".to_owned()],
        capacity_units: 100,
        reserved_capacity_units: 0,
        memory_bytes: 10_000,
        reserved_memory_bytes: 0,
        storage_bytes: 10_000,
        reserved_storage_bytes: 0,
        network_bytes_per_second: 10_000,
        reserved_network_bytes_per_second: 0,
        cpu_pressure_milli: 100,
        memory_pressure_milli: 100,
        network_pressure_milli: 100,
        thermal_pressure_milli: 100,
    }
}

fn placement(brain: BrainId) -> PlacementRegistry {
    placement_for(brain, ShardId::new(2).unwrap(), "worker-b")
}

fn placement_for(brain: BrainId, target: ShardId, target_node: &str) -> PlacementRegistry {
    let plan = PlacementPlanner
        .plan(PlacementRequest {
            brain_id: brain,
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: LeaseTerm::INITIAL,
            fencing_token: 1,
            effective_tag: LogicalTag::ZERO,
            demands: vec![ShardDemand {
                shard_id: target,
                load_units: 10,
                memory_bytes: 100,
                checkpoint_bytes: 100,
                network_bytes_per_second: 10,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: None,
            }],
            resources: vec![resource(target_node)],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: target_node.to_owned(),
            },
        })
        .unwrap();
    let mut registry = PlacementRegistry::new(brain, LeaseTerm::INITIAL);
    registry
        .apply(PlacementApplyRequest {
            request_id: "partial-runtime-placement".to_owned(),
            idempotency_key: "partial-runtime-placement".to_owned(),
            expected_resource_version: 0,
            observed_leader_term: LeaseTerm::INITIAL,
            plan,
            cutover: None,
            repartition: None,
        })
        .unwrap();
    registry
}

fn temp_path() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "aarnn-managed-partial-runtime-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&root).unwrap();
    root.join("outbound.json")
}

#[tokio::test]
async fn partial_runtime_commits_local_state_only_after_durable_outbound_seal() {
    let (topology, plan, complete, brain) = topology_and_plan();
    let checkpoints = complete.checkpoint_shards().unwrap();
    let first = ShardId::new(1).unwrap();
    let executor = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        checkpoints
            .into_iter()
            .filter(|checkpoint| checkpoint.shard_id == first)
            .collect::<Vec<_>>(),
        [first],
        32,
    )
    .unwrap();
    let path = temp_path();
    let outbox = Arc::new(tokio::sync::Mutex::new(
        StableOutboundLog::open(&path, brain, 64).unwrap(),
    ));
    let dispatcher = StableShardDispatcher::new(
        "worker-a",
        Arc::new(RwLock::new(placement(brain))),
        outbox.clone(),
    )
    .unwrap();
    let runtime = ManagedPartialShardRuntime::new(executor, dispatcher, 16).unwrap();

    let receiver_path = temp_path();
    let second = ShardId::new(2).unwrap();
    let receiver_executor = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan,
        complete
            .checkpoint_shards()
            .unwrap()
            .into_iter()
            .filter(|checkpoint| checkpoint.shard_id == second)
            .collect::<Vec<_>>(),
        [second],
        32,
    )
    .unwrap();
    let receiver = aarnn_rust::stable_shard_transport::DurableStableShardReceiver::new(
        &receiver_path,
        "worker-b",
        receiver_executor,
        LeaseTerm::INITIAL,
        1,
        ["worker-a".to_owned()],
    )
    .unwrap();
    let receiver_node = DistributedNode::new("worker-b".to_owned(), false);
    receiver_node
        .register_stable_shard_receiver_for_network("brain-901", 16, 16, receiver)
        .unwrap();
    let receiver_outbox_path = temp_path();
    let receiver_outbox = Arc::new(tokio::sync::Mutex::new(
        StableOutboundLog::open(&receiver_outbox_path, brain, 64).unwrap(),
    ));
    let receiver_dispatcher = StableShardDispatcher::new(
        "worker-b",
        Arc::new(RwLock::new(placement_for(
            brain,
            ShardId::new(1).unwrap(),
            "worker-a",
        ))),
        receiver_outbox,
    )
    .unwrap();
    receiver_node
        .register_stable_shard_dispatcher(brain, receiver_dispatcher)
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let service = receiver_node.stable_shard_data_plane_service();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                aarnn_rust::stable_shard_transport::proto::stable_shard_data_plane_server::StableShardDataPlaneServer::new(service),
            )
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    runtime
        .register_endpoint("worker-b", format!("http://{address}"))
        .unwrap();
    let node = DistributedNode::new("worker-a".to_owned(), false);
    node.register_partial_shard_runtime("brain-901", runtime)
        .await
        .unwrap();
    let poll = node
        .poll_partial_shard_runtime("brain-901", &[input(1, 1)])
        .await
        .unwrap();
    assert!(poll.steps.iter().any(|step| !step.outbound.is_empty()));
    assert!(poll.outbound_records > 0);
    let handle = node
        .state
        .read()
        .await
        .partial_shard_runtimes
        .get("brain-901")
        .cloned()
        .unwrap();
    assert_ne!(
        handle.lock().await.state_digest().unwrap(),
        StateDigest([0; 16])
    );
    assert!(!outbox.lock().await.pending("worker-b").unwrap().is_empty());
    assert_eq!(node.service_partial_shard_runtimes_once().await, 1);
    assert!(outbox.lock().await.pending("worker-b").unwrap().is_empty());
    let _ = shutdown_tx.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(receiver_path.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(receiver_outbox_path.parent().unwrap()).unwrap();
}
