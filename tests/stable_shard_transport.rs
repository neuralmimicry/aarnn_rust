use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::causal::CausalEvent;
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId,
    PartitionGeneration, ShardId, SynapseId, TopologyGeneration,
};
use aarnn_rust::partial_shard_executor::PartialShardOutbound;
use aarnn_rust::shard_executor::RoutedCausalEvent;
use aarnn_rust::shard_executor::StableShardExecutor;
use aarnn_rust::stable_outbound::StableOutboundLog;
use aarnn_rust::stable_shard_transport::{
    DurableStableShardReceiver, STABLE_SHARD_SOURCE_NODE_METADATA, StableShardDataPlaneService,
    StableShardFlushError, StableShardReceiverError, StableShardReceiverRegistry,
    StableShardTransportError, encode_frame, flush_pending,
};
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Request;

fn fixture() -> (
    TopologyGenerationModel,
    aarnn_rust::topology_model::CompiledExecutionPlan,
    Vec<aarnn_rust::shard_executor::StableShardCheckpoint>,
    BrainId,
) {
    let topology = TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        (1..=4)
            .map(|id| NeuronRecord {
                id: aarnn_rust::deterministic::NeuronId::new(id).unwrap(),
            })
            .collect(),
        vec![
            SynapseRecord {
                id: SynapseId::new(11).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(1).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
                delay_ticks: 0,
            },
            SynapseRecord {
                id: SynapseId::new(12).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(3).unwrap(),
                delay_ticks: 1,
            },
            SynapseRecord {
                id: SynapseId::new(13).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(3).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(4).unwrap(),
                delay_ticks: 0,
            },
        ],
    )
    .unwrap();
    let first = ShardId::new(1).unwrap();
    let second = ShardId::new(2).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
            weight_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
            release_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
            plasticity_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
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
    let reference = StableShardExecutor::from_topology(
        brain,
        &topology,
        plan.clone(),
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        16,
        128,
    )
    .unwrap();
    (
        topology,
        plan,
        reference.checkpoint_shards().unwrap(),
        brain,
    )
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aarnn-{name}-{}-{}.json",
        std::process::id(),
        fastrand::u64(..)
    ))
}

fn effect(
    plan: &aarnn_rust::topology_model::CompiledExecutionPlan,
    event_id: u64,
) -> PartialShardOutbound {
    PartialShardOutbound::SynapseEffect {
        plan_digest: plan.digest(),
        destination_shard: ShardId::new(2).unwrap(),
        event_id: EventId::new(event_id).unwrap(),
        logical_tag: LogicalTag::ZERO,
        synapse: SynapseId::new(13).unwrap(),
        charge: 1,
    }
}

fn cross_shard_event(event_id: u64) -> RoutedCausalEvent {
    let event_id = EventId::new(event_id).unwrap();
    let target = NeuronId::new(3).unwrap();
    RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event_id.raw(),
            ),
            id: event_id,
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

fn receiver_for(
    topology: &TopologyGenerationModel,
    plan: &aarnn_rust::topology_model::CompiledExecutionPlan,
    checkpoints: &[aarnn_rust::shard_executor::StableShardCheckpoint],
    brain: BrainId,
    path: &PathBuf,
    node_id: &str,
) -> DurableStableShardReceiver {
    let second = ShardId::new(2).unwrap();
    let worker = aarnn_rust::partial_shard_executor::PartialShardExecutor::from_checkpoints(
        brain,
        topology,
        plan.clone(),
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.shard_id == second)
            .cloned()
            .collect(),
        [second],
        32,
    )
    .unwrap();
    DurableStableShardReceiver::new(
        path,
        node_id,
        worker,
        LeaseTerm::INITIAL,
        7,
        ["worker-a".to_owned()],
    )
    .unwrap()
}

#[test]
fn receiver_registry_enforces_identity_uniqueness_and_idempotent_cleanup() {
    let (topology, plan, checkpoints, brain) = fixture();
    let first_path = temp_path("stable-registry-first");
    let duplicate_path = temp_path("stable-registry-duplicate");
    let wrong_node_path = temp_path("stable-registry-wrong-node");
    let registry = StableShardReceiverRegistry::new("worker-b").unwrap();

    registry
        .register(receiver_for(
            &topology,
            &plan,
            &checkpoints,
            brain,
            &first_path,
            "worker-b",
        ))
        .unwrap();
    assert!(registry.contains(brain).unwrap());

    assert!(matches!(
        registry.register(receiver_for(
            &topology,
            &plan,
            &checkpoints,
            brain,
            &duplicate_path,
            "worker-b",
        )),
        Err(StableShardTransportError::ReceiverAlreadyRegistered(id)) if id == brain
    ));
    assert!(matches!(
        registry.register(receiver_for(
            &topology,
            &plan,
            &checkpoints,
            brain,
            &wrong_node_path,
            "worker-c",
        )),
        Err(StableShardTransportError::ReceiverNodeMismatch { expected, actual })
            if expected == "worker-b" && actual == "worker-c"
    ));

    assert!(registry.unregister(brain).unwrap());
    assert!(!registry.unregister(brain).unwrap());
    assert!(!registry.contains(brain).unwrap());

    for path in [first_path, duplicate_path, wrong_node_path] {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn durable_receiver_applies_once_restarts_and_rejects_gaps_and_stale_fences() {
    let (topology, plan, checkpoints, brain) = fixture();
    let receiver_path = temp_path("stable-receiver");
    let sender_path = temp_path("stable-sender");
    let second = ShardId::new(2).unwrap();
    let worker = aarnn_rust::partial_shard_executor::PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.shard_id == second)
            .cloned()
            .collect(),
        [second],
        32,
    )
    .unwrap();
    let mut receiver = DurableStableShardReceiver::new(
        &receiver_path,
        "worker-b",
        worker,
        LeaseTerm::INITIAL,
        7,
        ["worker-a".to_owned()],
    )
    .unwrap();
    let mut sender = StableOutboundLog::open(&sender_path, brain, 8).unwrap();
    let record = sender
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 99))
        .unwrap();
    let frame = encode_frame(&record, "worker-a").unwrap();
    let first = receiver.apply(frame.clone()).unwrap();
    assert!(!first.duplicate);
    assert_eq!(first.sequence, 0);
    assert_eq!(receiver.next_sequence(), 1);
    let duplicate = receiver.apply(frame.clone()).unwrap();
    assert!(duplicate.duplicate);
    assert_eq!(duplicate.state_digest, first.state_digest);

    let reopened = DurableStableShardReceiver::open(
        &receiver_path,
        "worker-b",
        &topology,
        plan.clone(),
        ["worker-a".to_owned()],
    )
    .unwrap();
    assert_eq!(reopened.next_sequence(), 1);
    assert_eq!(
        reopened.executor().state_digest().unwrap(),
        first.state_digest
    );

    let second_record = sender
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 100))
        .unwrap();
    let third_record = sender
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 101))
        .unwrap();
    assert!(matches!(
        receiver.apply(encode_frame(&third_record, "worker-a").unwrap()),
        Err(StableShardReceiverError::SequenceGap {
            expected: 1,
            received: 2
        })
    ));
    receiver
        .apply(encode_frame(&second_record, "worker-a").unwrap())
        .unwrap();

    let wrong_generation = sender
        .append_generation_bound(
            "worker-b",
            LeaseTerm::INITIAL,
            7,
            TopologyGeneration::new(2).unwrap(),
            PartitionGeneration::INITIAL,
            effect(&plan, 102),
        )
        .unwrap();
    assert!(matches!(
        receiver.apply(encode_frame(&wrong_generation, "worker-a").unwrap()),
        Err(StableShardReceiverError::GenerationMismatch)
    ));

    let stale_record = sender
        .append(
            "worker-b",
            LeaseTerm::new(2).unwrap(),
            8,
            effect(&plan, 103),
        )
        .unwrap();
    let error = receiver
        .apply(encode_frame(&stale_record, "worker-a").unwrap())
        .unwrap_err();
    assert!(matches!(error, StableShardReceiverError::StaleAuthority));

    let _ = std::fs::remove_file(receiver_path);
    let _ = std::fs::remove_file(sender_path);
}

#[tokio::test]
async fn receiver_keeps_independent_receipt_frontiers_for_multiple_sources() {
    let (topology, plan, checkpoints, brain) = fixture();
    let receiver_path = temp_path("stable-receiver-multi-source");
    let sender_a_path = temp_path("stable-sender-a");
    let sender_c_path = temp_path("stable-sender-c");
    let second = ShardId::new(2).unwrap();
    let worker = aarnn_rust::partial_shard_executor::PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.shard_id == second)
            .cloned()
            .collect(),
        [second],
        32,
    )
    .unwrap();
    let mut receiver = DurableStableShardReceiver::new(
        &receiver_path,
        "worker-b",
        worker,
        LeaseTerm::INITIAL,
        7,
        ["worker-a".to_owned(), "worker-c".to_owned()],
    )
    .unwrap();
    let mut sender_a = StableOutboundLog::open(&sender_a_path, brain, 8).unwrap();
    let mut sender_c = StableOutboundLog::open(&sender_c_path, brain, 8).unwrap();

    let a0 = sender_a
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 200))
        .unwrap();
    let c0 = sender_c
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 201))
        .unwrap();
    assert_eq!(a0.sequence, 0);
    assert_eq!(c0.sequence, 0);

    receiver
        .apply(encode_frame(&a0, "worker-a").unwrap())
        .unwrap();
    receiver
        .apply(encode_frame(&c0, "worker-c").unwrap())
        .unwrap();
    assert_eq!(receiver.next_sequence_for_source("worker-a"), 1);
    assert_eq!(receiver.next_sequence_for_source("worker-c"), 1);
    assert!(
        receiver
            .apply(encode_frame(&a0, "worker-a").unwrap())
            .unwrap()
            .duplicate
    );

    let a1 = sender_a
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 202))
        .unwrap();
    let c1 = sender_c
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 203))
        .unwrap();
    let reopened = DurableStableShardReceiver::open(
        &receiver_path,
        "worker-b",
        &topology,
        plan.clone(),
        ["worker-a".to_owned(), "worker-c".to_owned()],
    )
    .unwrap();
    receiver = reopened;
    assert_eq!(receiver.next_sequence_for_source("worker-a"), 1);
    assert_eq!(receiver.next_sequence_for_source("worker-c"), 1);
    receiver
        .apply(encode_frame(&a1, "worker-a").unwrap())
        .unwrap();
    receiver
        .apply(encode_frame(&c1, "worker-c").unwrap())
        .unwrap();
    assert_eq!(receiver.next_sequence_for_source("worker-a"), 2);
    assert_eq!(receiver.next_sequence_for_source("worker-c"), 2);

    for path in [receiver_path, sender_a_path, sender_c_path] {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("stable-outbound.lock"));
    }
}

#[tokio::test]
async fn receiver_persists_generated_outbound_until_durably_sealed() {
    let (topology, plan, checkpoints, brain) = fixture();
    let receiver_path = temp_path("stable-generated-outbound-receiver");
    let sender_path = temp_path("stable-generated-outbound-sender");
    let second = ShardId::new(2).unwrap();
    let receiver = receiver_for(
        &topology,
        &plan,
        &checkpoints,
        brain,
        &receiver_path,
        "worker-b",
    );
    let mut sender = StableOutboundLog::open(&sender_path, brain, 8).unwrap();
    let inbound = sender
        .append(
            "worker-b",
            LeaseTerm::INITIAL,
            7,
            PartialShardOutbound::CausalEvent {
                plan_digest: plan.digest(),
                destination_shard: second,
                event: cross_shard_event(200),
            },
        )
        .unwrap();
    let frame = encode_frame(&inbound, "worker-a").unwrap();
    let mut receiver = receiver;

    let applied = receiver.apply(frame.clone()).unwrap();
    assert!(!applied.duplicate);
    assert_eq!(applied.pending_outbound.len(), 1);
    assert!(matches!(
        &applied.pending_outbound[0],
        PartialShardOutbound::SynapseEffect {
            destination_shard,
            synapse,
            ..
        } if *destination_shard == ShardId::new(1).unwrap()
            && *synapse == SynapseId::new(12).unwrap()
    ));

    drop(receiver);
    let mut reopened = DurableStableShardReceiver::open(
        &receiver_path,
        "worker-b",
        &topology,
        plan.clone(),
        ["worker-a".to_owned()],
    )
    .unwrap();
    let retry = reopened.apply(frame).unwrap();
    assert!(retry.duplicate);
    assert_eq!(retry.pending_outbound.len(), 1);
    reopened
        .acknowledge_pending_outbound(&retry.pending_outbound)
        .unwrap();
    drop(reopened);

    let reopened_again = DurableStableShardReceiver::open(
        &receiver_path,
        "worker-b",
        &topology,
        plan,
        ["worker-a".to_owned()],
    )
    .unwrap();
    let _ = std::fs::remove_file(receiver_path);
    let _ = std::fs::remove_file(sender_path);
    assert!(reopened_again.pending_outbound().is_empty());
}

#[tokio::test]
async fn grpc_receiver_refuses_to_ack_generated_outbound_without_a_dispatcher() {
    let (topology, plan, checkpoints, brain) = fixture();
    let receiver_path = temp_path("stable-grpc-generated-receiver");
    let sender_path = temp_path("stable-grpc-generated-sender");
    let receiver = receiver_for(
        &topology,
        &plan,
        &checkpoints,
        brain,
        &receiver_path,
        "worker-b",
    );
    let service = StableShardDataPlaneService::new(receiver);
    let mut sender = StableOutboundLog::open(&sender_path, brain, 8).unwrap();
    let record = sender
        .append(
            "worker-b",
            LeaseTerm::INITIAL,
            7,
            PartialShardOutbound::CausalEvent {
                plan_digest: plan.digest(),
                destination_shard: ShardId::new(2).unwrap(),
                event: cross_shard_event(201),
            },
        )
        .unwrap();
    let frame = encode_frame(&record, "worker-a").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
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

    let mut client = aarnn_rust::stable_shard_transport::proto::stable_shard_data_plane_client::StableShardDataPlaneClient::connect(format!("http://{address}"))
        .await
        .unwrap();
    let mut request = Request::new(tokio_stream::iter(vec![frame]));
    request.metadata_mut().insert(
        STABLE_SHARD_SOURCE_NODE_METADATA,
        tonic::metadata::MetadataValue::try_from("worker-a").unwrap(),
    );
    let response = client.stream_shard_frames(request).await.unwrap();
    let mut acknowledgements = response.into_inner();
    let status = acknowledgements
        .message()
        .await
        .expect_err("generated outbound work must block acknowledgement");
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);

    let reopened = DurableStableShardReceiver::open(
        &receiver_path,
        "worker-b",
        &topology,
        plan,
        ["worker-a".to_owned()],
    )
    .unwrap();
    assert_eq!(reopened.pending_outbound().len(), 1);
    let _ = shutdown_tx.send(());
    task.await.unwrap();
    let _ = std::fs::remove_file(receiver_path);
    let _ = std::fs::remove_file(sender_path);
}

#[tokio::test]
async fn generated_stable_shard_stream_returns_durable_duplicate_ack() {
    let (topology, plan, checkpoints, brain) = fixture();
    let receiver_path = temp_path("stable-grpc-receiver");
    let sender_path = temp_path("stable-grpc-sender");
    let second = ShardId::new(2).unwrap();
    let worker = aarnn_rust::partial_shard_executor::PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.shard_id == second)
            .cloned()
            .collect(),
        [second],
        32,
    )
    .unwrap();
    let receiver = DurableStableShardReceiver::new(
        &receiver_path,
        "worker-b",
        worker,
        LeaseTerm::INITIAL,
        7,
        ["worker-a".to_owned()],
    )
    .unwrap();
    let service = StableShardDataPlaneService::new(receiver);
    let sender = Arc::new(tokio::sync::Mutex::new(
        StableOutboundLog::open(&sender_path, brain, 8).unwrap(),
    ));
    let record = sender
        .lock()
        .await
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 99))
        .unwrap();
    let frame = encode_frame(&record, "worker-a").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
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

    let mut client = aarnn_rust::stable_shard_transport::proto::stable_shard_data_plane_client::StableShardDataPlaneClient::connect(format!("http://{address}"))
        .await
        .unwrap();
    let missing_session = client
        .stream_shard_frames(Request::new(tokio_stream::iter(vec![frame.clone()])))
        .await
        .expect_err("stable-shard streams require authenticated source metadata");
    assert_eq!(missing_session.code(), tonic::Code::Unauthenticated);
    let mut mismatched_request = Request::new(tokio_stream::iter(vec![frame.clone()]));
    mismatched_request.metadata_mut().insert(
        STABLE_SHARD_SOURCE_NODE_METADATA,
        tonic::metadata::MetadataValue::try_from("worker-c").unwrap(),
    );
    let mut mismatched_stream = client
        .stream_shard_frames(mismatched_request)
        .await
        .unwrap()
        .into_inner();
    let mismatched_session = mismatched_stream
        .message()
        .await
        .expect_err("frame source must match the session identity");
    assert_eq!(mismatched_session.code(), tonic::Code::PermissionDenied);
    let mut request = Request::new(tokio_stream::iter(vec![frame.clone(), frame]));
    request.metadata_mut().insert(
        STABLE_SHARD_SOURCE_NODE_METADATA,
        tonic::metadata::MetadataValue::try_from("worker-a").unwrap(),
    );
    let response = client.stream_shard_frames(request).await.unwrap();
    let mut acknowledgements = response.into_inner();
    let first = acknowledgements.message().await.unwrap().unwrap();
    let duplicate = acknowledgements.message().await.unwrap().unwrap();
    assert!(first.durable);
    assert!(!first.duplicate);
    assert!(duplicate.duplicate);
    assert_eq!(first.record_digest, duplicate.record_digest);

    let flushed = flush_pending(
        sender.clone(),
        "worker-b",
        "worker-a",
        &format!("http://{address}"),
    )
    .await
    .unwrap();
    assert_eq!(
        flushed, 1,
        "duplicate durable ack must clear the sender log"
    );

    let _ = shutdown_tx.send(());
    task.await.unwrap();
    let _ = std::fs::remove_file(receiver_path);
    let _ = std::fs::remove_file(sender_path);
}

#[tokio::test]
async fn distributed_node_routes_registered_brain_to_stable_data_plane() {
    let (topology, plan, checkpoints, brain) = fixture();
    let receiver_path = temp_path("stable-node-registry-receiver");
    let sender_path = temp_path("stable-node-registry-sender");
    let second = ShardId::new(2).unwrap();
    let worker = aarnn_rust::partial_shard_executor::PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.shard_id == second)
            .cloned()
            .collect(),
        [second],
        32,
    )
    .unwrap();
    let receiver = DurableStableShardReceiver::new(
        &receiver_path,
        "worker-b",
        worker,
        LeaseTerm::INITIAL,
        7,
        ["worker-a".to_owned()],
    )
    .unwrap();
    let node = aarnn_rust::distributed::DistributedNode::new("worker-b".to_owned(), false);
    node.register_stable_shard_receiver_for_network("stable-node-network", 8, 8, receiver)
        .expect("explicit receiver registration");
    assert!(
        node.stable_shard_data_plane_service()
            .registry()
            .contains(brain)
            .unwrap()
    );
    let registrations = node.get_stable_executor_registrations().await;
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].network_id, "stable-node-network");
    assert_eq!(registrations[0].owned_shard_ids, vec![second.raw()]);
    assert_eq!(registrations[0].application_acks.len(), 1);
    assert_eq!(registrations[0].application_acks[0].shard_id, second.raw());

    let sender = Arc::new(tokio::sync::Mutex::new(
        StableOutboundLog::open(&sender_path, brain, 8).unwrap(),
    ));
    let _ = sender
        .lock()
        .await
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 123))
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let service = node.stable_shard_data_plane_service();
    let task = tokio::spawn(async move {
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

    let flushed = flush_pending(
        sender.clone(),
        "worker-b",
        "worker-a",
        &format!("http://{address}"),
    )
    .await
    .expect("registered node accepts stable frame");
    assert_eq!(flushed, 1);
    assert!(
        node.unregister_stable_shard_receiver(brain)
            .expect("receiver cleanup")
    );
    assert!(
        !node
            .stable_shard_data_plane_service()
            .registry()
            .contains(brain)
            .unwrap()
    );

    let _ = sender
        .lock()
        .await
        .append("worker-b", LeaseTerm::INITIAL, 7, effect(&plan, 124))
        .unwrap();
    let error = flush_pending(sender, "worker-b", "worker-a", &format!("http://{address}"))
        .await
        .expect_err("an unregistered brain must not be accepted by the data plane");
    assert!(matches!(
        error,
        StableShardFlushError::Rpc(status)
            if status.code() == tonic::Code::NotFound
    ));

    let _ = shutdown_tx.send(());
    task.await.unwrap();
    let _ = std::fs::remove_file(receiver_path);
    let _ = std::fs::remove_file(sender_path);
}
