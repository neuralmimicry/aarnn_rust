use aarnn_rust::authoritative_shard::{
    AuthoritativeShard, BIOLOGICAL_STATE_SCHEMA_VERSION, StableBiologicalState, StableNeuronState,
    StableSynapseState, StableTransitionInput,
};
use aarnn_rust::causal_transport::proto::causal_data_plane_client::CausalDataPlaneClient;
use aarnn_rust::causal_transport::proto::causal_data_plane_server::CausalDataPlaneServer;
use aarnn_rust::causal_transport::proto::causal_event_envelope::EventStage;
use aarnn_rust::causal_transport::proto::{CausalEventEnvelope, LogicalTag};
use aarnn_rust::causal_transport::{AuthoritativeCausalService, DurableCausalService};
use aarnn_rust::data_plane::{CausalEnvelope, EnvelopeKind};
use aarnn_rust::deterministic::{
    BrainId, ComponentId, EventId, EventStage as DomainEventStage, LeaseTerm,
    LogicalTag as DomainTag, PartitionGeneration, RouteId, SchemaVersion, ShardId, StreamId,
    TopologyGeneration,
};
use aarnn_rust::durability::DurableShard;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Request;

#[cfg(feature = "stable_executor_live")]
use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::distributed::DistributedNode;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::distributed::proto::NodeStatus;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::distributed::proto::distributed_neuromorphic_server::DistributedNeuromorphic;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::managed_durability::{managed_brain_id, managed_link_stream_id};
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::managed_stable_executor::ManagedStableExecutor;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::runner::Runner;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::shard_executor::StableShardExecutor;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::sim::{Learning, NeuronModel};
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::stable_executor_durable::StableExecutorDurableBridge;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
#[cfg(feature = "stable_executor_live")]
use aarnn_rust::topology_model::{
    NeuronRecord, TopologyGenerationModel, VirtualShardAssignment, compile_execution_plan,
};
#[cfg(feature = "stable_executor_live")]
use std::sync::Arc;
#[cfg(feature = "stable_executor_live")]
use tokio::sync::RwLock;

fn envelope() -> CausalEnvelope {
    CausalEnvelope {
        schema_version: SchemaVersion::CURRENT,
        brain: BrainId::new(11).unwrap(),
        stream: StreamId::new(12).unwrap(),
        sequence: 0,
        lease_term: LeaseTerm::INITIAL,
        route: RouteId::new(13).unwrap(),
        partition_generation: PartitionGeneration::INITIAL,
        source: None,
        target: None,
        tag: DomainTag::new(4, 0),
        event: EventId::new(14).unwrap(),
        stage: DomainEventStage::SynapticTransition,
        kind: EnvelopeKind::Event,
        payload: vec![3, 4, 5],
        deferred_from_nonconvergence: false,
    }
}

fn wire(envelope: &CausalEnvelope) -> CausalEventEnvelope {
    CausalEventEnvelope {
        schema_version: u32::from(envelope.schema_version.raw()),
        brain_id: envelope.brain.raw(),
        stream_id: envelope.stream.raw(),
        sequence: envelope.sequence,
        lease_term: envelope.lease_term.raw(),
        route_id: envelope.route.raw(),
        partition_generation: envelope.partition_generation.raw(),
        tag: Some(LogicalTag {
            tick: envelope.tag.tick,
            microstep: envelope.tag.microstep,
        }),
        event_id: envelope.event.raw(),
        stage: EventStage::SynapticTransition as i32,
        source_id: 0,
        target_id: 0,
        payload: envelope.payload.clone(),
        deferred_from_nonconvergence: false,
        sender_node_id: "sender-a".to_owned(),
    }
}

async fn start_server(
    service: DurableCausalService,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("causal listener");
    let address = listener.local_addr().expect("causal address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(CausalDataPlaneServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("causal server");
    });
    (format!("http://{address}"), shutdown_tx, task)
}

async fn start_authoritative_server(
    service: AuthoritativeCausalService,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("authoritative causal listener");
    let address = listener.local_addr().expect("authoritative causal address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(CausalDataPlaneServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("authoritative causal server");
    });
    (format!("http://{address}"), shutdown_tx, task)
}

async fn round_trip(
    client: &mut CausalDataPlaneClient<tonic::transport::Channel>,
    frames: Vec<CausalEventEnvelope>,
) -> Result<Vec<CausalEventEnvelope>, tonic::Status> {
    let response = client
        .stream_events(Request::new(tokio_stream::iter(frames)))
        .await?;
    let mut stream = response.into_inner();
    let mut received = Vec::new();
    while let Some(frame) = stream.message().await? {
        received.push(frame);
    }
    Ok(received)
}

#[tokio::test]
async fn generated_causal_client_exercises_apply_duplicate_reconnect_and_fencing() {
    let original = envelope();
    let shard = DurableShard::new(
        original.brain,
        ShardId::new(15).unwrap(),
        TopologyGeneration::INITIAL,
        original.partition_generation,
        original.lease_term,
        original.stream,
        1024,
        vec![10],
        vec![1, 2],
    );
    let service = DurableCausalService::new(shard, 8);
    let adapter = service.adapter();
    let (address, shutdown, task) = start_server(service).await;
    let mut client = CausalDataPlaneClient::connect(address)
        .await
        .expect("causal client");

    let first = round_trip(&mut client, vec![wire(&original), wire(&original)])
        .await
        .expect("apply and duplicate stream");
    assert_eq!(first.len(), 2);
    assert_eq!(first[0], first[1]);
    assert_eq!(first[0].sender_node_id, "sender-a");

    // A new transport stream resumes from the same acknowledged sequence;
    // it must remain a durable duplicate rather than re-running the transition.
    let reconnected = round_trip(&mut client, vec![wire(&original)])
        .await
        .expect("reconnect replay");
    assert_eq!(reconnected.len(), 1);
    let state = adapter.lock().expect("adapter lock");
    assert_eq!(state.shard().durable_log_sequence(), Some(0));
    assert_eq!(state.shard().receipt_count(), 1);
    assert_eq!(state.shard().biological_state(), &[3, 4, 5]);
    drop(state);

    let mut stale = wire(&original);
    stale.lease_term = 2;
    let stale_response = client
        .stream_events(Request::new(tokio_stream::iter(vec![stale])))
        .await
        .expect_err("stale lease must be rejected");
    assert_eq!(stale_response.code(), tonic::Code::FailedPrecondition);

    let mut stale_generation = wire(&original);
    stale_generation.partition_generation = 2;
    let generation_response = client
        .stream_events(Request::new(tokio_stream::iter(vec![stale_generation])))
        .await
        .expect_err("stale generation must be rejected");
    assert_eq!(generation_response.code(), tonic::Code::FailedPrecondition);

    let _ = shutdown.send(());
    task.await.expect("causal server join");
}

#[tokio::test]
async fn generated_causal_service_accepts_concurrent_sender_streams() {
    let first = envelope();
    let mut second = envelope();
    second.stream = StreamId::new(112).unwrap();
    second.event = EventId::new(113).unwrap();
    second.payload = vec![6, 7, 8];
    second.tag = DomainTag::new(5, 0);

    let shard = DurableShard::new(
        first.brain,
        ShardId::new(114).unwrap(),
        TopologyGeneration::INITIAL,
        first.partition_generation,
        first.lease_term,
        first.stream,
        1024,
        vec![0],
        vec![],
    );
    let service = DurableCausalService::new(shard, 8);
    let adapter = service.adapter();
    let (address, shutdown, task) = start_server(service).await;
    let mut client = CausalDataPlaneClient::connect(address)
        .await
        .expect("causal client");

    let responses = round_trip(&mut client, vec![wire(&first), wire(&second)])
        .await
        .expect("both sender streams apply");
    assert_eq!(responses.len(), 2);
    assert!(
        responses
            .iter()
            .all(|frame| frame.sender_node_id == "sender-a")
    );
    let state = adapter.lock().expect("adapter lock");
    assert_eq!(state.shard().receipt_count(), 2);
    assert_eq!(state.shard().durable_log_sequence(), Some(1));
    assert_eq!(state.shard().biological_state(), &[6, 7, 8]);
    drop(state);

    let duplicate = round_trip(&mut client, vec![wire(&second)])
        .await
        .expect("second sender reconnect");
    assert_eq!(duplicate.len(), 1);
    let state = adapter.lock().expect("adapter lock");
    assert_eq!(state.shard().receipt_count(), 2);
    assert_eq!(state.shard().durable_log_sequence(), Some(1));
    drop(state);

    let _ = shutdown.send(());
    task.await.expect("causal server join");
}

#[tokio::test]
async fn authoritative_causal_client_uses_durable_shard_owner_after_restartable_apply() {
    let original = envelope();
    let root = std::env::temp_dir().join(format!(
        "aarnn-authoritative-causal-grpc-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let shard = AuthoritativeShard::open(
        root.join("active.json"),
        Some(root.join("warm.json")),
        original.brain,
        ShardId::new(25).unwrap(),
        TopologyGeneration::INITIAL,
        original.partition_generation,
        original.lease_term,
        original.stream,
        1024,
        vec![0],
        vec![],
    )
    .expect("authoritative shard");
    let service = AuthoritativeCausalService::new(shard, 8);
    let state = service.shard();
    let (address, shutdown, task) = start_authoritative_server(service).await;
    let mut client = CausalDataPlaneClient::connect(address)
        .await
        .expect("authoritative client");
    let echoed = round_trip(&mut client, vec![wire(&original), wire(&original)])
        .await
        .expect("durably applied frames");
    assert_eq!(echoed.len(), 2);
    assert!(
        echoed
            .iter()
            .all(|frame| frame.sender_node_id == "sender-a")
    );
    let shard = state.lock().expect("authoritative shard lock");
    assert_eq!(shard.biological_state(), &[3, 4, 5]);
    assert_eq!(shard.receipt_count(), 1);
    assert_eq!(shard.durable_sequence(), Some(0));
    drop(shard);
    let _ = shutdown.send(());
    task.await.expect("authoritative server join");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn authoritative_causal_service_can_run_the_stable_id_biological_kernel() {
    let source = aarnn_rust::deterministic::NeuronId::new(801).unwrap();
    let target = aarnn_rust::deterministic::NeuronId::new(802).unwrap();
    let biology = StableBiologicalState::new(
        TopologyGeneration::INITIAL,
        vec![
            StableNeuronState {
                id: source,
                membrane: 0,
                threshold: 2_000_000,
                refractory_until: DomainTag::ZERO,
                adaptation: 0,
            },
            StableNeuronState {
                id: target,
                membrane: 0,
                threshold: 2_000_000,
                refractory_until: DomainTag::ZERO,
                adaptation: 0,
            },
        ],
        vec![StableSynapseState {
            id: aarnn_rust::deterministic::SynapseId::new(803).unwrap(),
            source,
            target,
            weight: 1_000_000,
            delay_ticks: 0,
            release_state: 1_000_000,
            plasticity_trace: 0,
        }],
    )
    .unwrap()
    .encode()
    .unwrap();
    let mut event = envelope();
    event.payload = serde_json::to_vec(&StableTransitionInput {
        schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
        source: Some(source),
        target,
        charge: 1,
        delay_ticks: 0,
    })
    .unwrap();
    event.source = Some(source);
    event.target = Some(target);
    event.event = EventId::new(804).unwrap();
    let mut frame = wire(&event);
    frame.source_id = source.raw();
    frame.target_id = target.raw();

    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-causal-grpc-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let shard = AuthoritativeShard::open(
        root.join("active.json"),
        Some(root.join("warm.json")),
        event.brain,
        ShardId::new(805).unwrap(),
        TopologyGeneration::INITIAL,
        event.partition_generation,
        event.lease_term,
        event.stream,
        64 * 1024,
        biology,
        vec![],
    )
    .unwrap();
    let service = AuthoritativeCausalService::new_with_stable_biology(shard, 8);
    let state = service.shard();
    let (address, shutdown, task) = start_authoritative_server(service).await;
    let mut client = CausalDataPlaneClient::connect(address).await.unwrap();
    let response = round_trip(&mut client, vec![frame]).await.unwrap();
    assert_eq!(response.len(), 1);
    let state = state.lock().unwrap();
    let biology = StableBiologicalState::decode(state.biological_state()).unwrap();
    assert_eq!(biology.neurons()[1].membrane, 1_000_000);
    assert_eq!(state.receipt_count(), 1);
    drop(state);
    let _ = shutdown.send(());
    task.await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "stable_executor_live")]
fn stable_managed_runtime(network_id: &str, root: &std::path::Path) -> ManagedStableExecutor {
    let brain = managed_brain_id(network_id);
    let neuron = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let topology = TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        vec![NeuronRecord { id: neuron }],
        Vec::new(),
    )
    .unwrap();
    let shard = ShardId::new(10).unwrap();
    let plan = compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        vec![VirtualShardAssignment {
            shard,
            components: vec![ComponentId::new(1).unwrap()],
            load: 1,
        }],
        Vec::new(),
    )
    .unwrap();
    let executor =
        StableShardExecutor::from_topology(brain, &topology, plan, 1_000_000, 1_000_000, 8, 64)
            .unwrap();
    let store = StableExecutorCheckpointStore::new(root.join("checkpoints")).unwrap();
    let bridge = StableExecutorDurableBridge::new(
        executor,
        store,
        LeaseTerm::INITIAL,
        7,
        EventId::new(1000).unwrap(),
        root.join("owner"),
        root.join("warm"),
        StreamId::new(77).unwrap(),
        64 * 1024,
        Vec::new(),
    )
    .unwrap();
    ManagedStableExecutor::new(bridge, EventId::new(1000).unwrap(), 8, 8).unwrap()
}

#[cfg(feature = "stable_executor_live")]
fn stable_managed_network(
    network_id: &str,
    root: &std::path::Path,
) -> aarnn_rust::distributed::ManagedNetwork {
    let config = NetworkConfig::default();
    let runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        config.clone(),
        NeuronModel::Lif,
        Learning::Stdp,
    );
    let mut network = aarnn_rust::distributed::ManagedNetwork::new(
        network_id.to_owned(),
        runner,
        config,
        NeuronModel::Lif,
        Learning::Stdp,
        LIFParams::default(),
        STDPParams::default(),
    );
    network
        .register_stable_executor(stable_managed_runtime(network_id, root))
        .unwrap();
    network
}

#[cfg(feature = "stable_executor_live")]
fn distributed_stable_frame(
    network_id: &str,
    sender_node_id: &str,
    receiver_node_id: &str,
    sequence: u64,
    event_id: u64,
    brain: BrainId,
    lease_term: u64,
    stream: Option<StreamId>,
) -> CausalEventEnvelope {
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let payload = serde_json::to_vec(&StableTransitionInput {
        schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
        source: None,
        target,
        charge: 1,
        delay_ticks: 0,
    })
    .unwrap();
    CausalEventEnvelope {
        schema_version: u32::from(SchemaVersion::CURRENT.raw()),
        brain_id: brain.raw(),
        stream_id: stream
            .unwrap_or_else(|| managed_link_stream_id(network_id, sender_node_id, receiver_node_id))
            .raw(),
        sequence,
        lease_term,
        route_id: 1,
        partition_generation: PartitionGeneration::INITIAL.raw(),
        tag: Some(LogicalTag {
            tick: 0,
            microstep: 0,
        }),
        event_id,
        stage: EventStage::SynapticTransition as i32,
        source_id: 0,
        target_id: target.raw(),
        payload,
        deferred_from_nonconvergence: false,
        sender_node_id: sender_node_id.to_owned(),
    }
}

#[cfg(feature = "stable_executor_live")]
async fn start_distributed_causal_server(
    node: DistributedNode,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("distributed causal listener");
    let address = listener.local_addr().expect("distributed causal address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(CausalDataPlaneServer::new(node))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("distributed causal server");
    });
    (format!("http://{address}"), shutdown_tx, task)
}

#[cfg(feature = "stable_executor_live")]
#[tokio::test]
async fn distributed_node_stable_causal_stream_enforces_identity_and_cursor() {
    let network_id = "stable-grpc-network";
    let sender_node_id = "sender-a";
    let receiver_node_id = "receiver-a";
    let root = std::env::temp_dir().join(format!(
        "aarnn-distributed-stable-causal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let node = DistributedNode::new(receiver_node_id.to_owned(), false);
    {
        let mut state = node.state.write().await;
        state.nodes.insert(
            sender_node_id.to_owned(),
            NodeStatus {
                node_id: sender_node_id.to_owned(),
                ..Default::default()
            },
        );
        state.networks.insert(
            network_id.to_owned(),
            Arc::new(RwLock::new(stable_managed_network(network_id, &root))),
        );
    }
    let brain = managed_brain_id(network_id);
    let (address, shutdown, task) = start_distributed_causal_server(node.clone()).await;
    let mut client = CausalDataPlaneClient::connect(address)
        .await
        .expect("distributed causal client");

    let unknown_sender = distributed_stable_frame(
        network_id,
        "unknown-sender",
        receiver_node_id,
        0,
        1001,
        brain,
        1,
        None,
    );
    let error = client
        .stream_events(Request::new(tokio_stream::iter(vec![unknown_sender])))
        .await
        .expect_err("unknown sender must be rejected before stable admission");
    assert_eq!(error.code(), tonic::Code::PermissionDenied);

    let first = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        0,
        1001,
        brain,
        1,
        None,
    );
    let response = round_trip(&mut client, vec![first.clone()])
        .await
        .expect("enrolled sender must reach stable executor");
    assert_eq!(response.len(), 1);

    let stream_id = managed_link_stream_id(network_id, sender_node_id, receiver_node_id);
    {
        let state = node.state.read().await;
        let network = state.networks[network_id].read().await;
        let progress = network
            .stable_executor
            .as_ref()
            .unwrap()
            .bridge()
            .stream_progress(stream_id)
            .unwrap()
            .expect("external stream receipt must be durable");
        assert_eq!(progress.next_sequence, 1);
        assert_eq!(progress.entries[&0].event_id, EventId::new(1001).unwrap());
    }

    let digest_after_first = {
        let state = node.state.read().await;
        let network = state.networks[network_id].read().await;
        network
            .stable_executor
            .as_ref()
            .unwrap()
            .bridge()
            .authority()
            .state_digest()
            .unwrap()
    };
    let duplicate = round_trip(&mut client, vec![first])
        .await
        .expect("same event replay must be idempotent");
    assert_eq!(duplicate.len(), 1);
    let digest_after_duplicate = {
        let state = node.state.read().await;
        let network = state.networks[network_id].read().await;
        network
            .stable_executor
            .as_ref()
            .unwrap()
            .bridge()
            .authority()
            .state_digest()
            .unwrap()
    };
    assert_eq!(digest_after_duplicate, digest_after_first);

    let mut conflicting = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        0,
        1001,
        brain,
        1,
        None,
    );
    conflicting.payload.push(0);
    let error = client
        .stream_events(Request::new(tokio_stream::iter(vec![conflicting])))
        .await
        .expect_err("conflicting replay must fail closed");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    let gap = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        2,
        1003,
        brain,
        1,
        None,
    );
    let error = client
        .stream_events(Request::new(tokio_stream::iter(vec![gap])))
        .await
        .expect_err("a sequence gap must fail closed");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    let mut stale_term = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        1,
        1004,
        brain,
        2,
        None,
    );
    stale_term.lease_term = 2;
    let error = client
        .stream_events(Request::new(tokio_stream::iter(vec![stale_term])))
        .await
        .expect_err("stale lease must fail before cursor movement");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    let wrong_brain = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        1,
        1005,
        BrainId::new(brain.raw() + 1).unwrap(),
        1,
        None,
    );
    let error = client
        .stream_events(Request::new(tokio_stream::iter(vec![wrong_brain])))
        .await
        .expect_err("brain mismatch must fail at the stable authority");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    let wrong_stream = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        1,
        1006,
        brain,
        1,
        Some(StreamId::new(999).unwrap()),
    );
    let error = client
        .stream_events(Request::new(tokio_stream::iter(vec![wrong_stream])))
        .await
        .expect_err("stream mismatch must fail at the stable cursor");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    let second = distributed_stable_frame(
        network_id,
        sender_node_id,
        receiver_node_id,
        1,
        1007,
        brain,
        1,
        None,
    );
    let response = round_trip(&mut client, vec![second])
        .await
        .expect("rejected frames must not advance the cursor");
    assert_eq!(response.len(), 1);

    let _ = shutdown.send(());
    task.await.expect("distributed causal server join");
    std::fs::remove_dir_all(root).unwrap();
}
