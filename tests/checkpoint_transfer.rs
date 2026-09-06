//! Verification of the bounded network checkpoint/bootstrap transfer seam.

use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::checkpoint_transfer::{
    CheckpointTransferReceiver, CheckpointTransferSource, StableCheckpointTransferService,
    send_checkpoint_transfer,
};
use aarnn_rust::deterministic::{
    ComponentId, EventId, LeaseTerm, NeuronId, PartitionGeneration, ShardId, TopologyGeneration,
};
use aarnn_rust::distributed::proto::stable_checkpoint_transfer_server::StableCheckpointTransferServer;
use aarnn_rust::managed_durability::managed_brain_id;
use aarnn_rust::shard_executor::StableShardExecutor;
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
use aarnn_rust::topology_model::{
    NeuronRecord, TopologyGenerationModel, VirtualShardAssignment, compile_execution_plan,
};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;

fn fixture(
    root: &std::path::Path,
) -> (
    aarnn_rust::deterministic::BrainId,
    aarnn_rust::deterministic::StateDigest,
    StableExecutorCheckpointStore,
    EventId,
) {
    let brain_id = managed_brain_id("checkpoint-transfer-test");
    let neurons = vec![
        NeuronRecord {
            id: NeuronId::new(1).unwrap(),
        },
        NeuronRecord {
            id: NeuronId::new(2).unwrap(),
        },
    ];
    let topology =
        TopologyGenerationModel::new(TopologyGeneration::INITIAL, neurons, Vec::new()).unwrap();
    let assignments = vec![
        VirtualShardAssignment {
            shard: ShardId::new(10).unwrap(),
            components: vec![ComponentId::new(1).unwrap()],
            load: 1,
        },
        VirtualShardAssignment {
            shard: ShardId::new(20).unwrap(),
            components: vec![ComponentId::new(2).unwrap()],
            load: 1,
        },
    ];
    let plan = compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        assignments,
        Vec::new(),
    )
    .unwrap();
    let executor = StableShardExecutor::from_topology(
        brain_id,
        &topology,
        plan.clone(),
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        32,
        128,
    )
    .unwrap();
    let store = StableExecutorCheckpointStore::new(root.join("source-checkpoints")).unwrap();
    let checkpoint_id = EventId::new(100).unwrap();
    store
        .publish(checkpoint_id, LeaseTerm::INITIAL, &executor)
        .unwrap();
    (brain_id, plan.digest(), store, checkpoint_id)
}

#[test]
fn transfer_reassembles_out_of_order_frames_and_publishes_idempotently() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-checkpoint-transfer-core-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let (brain_id, plan_digest, source_store, checkpoint_id) = fixture(&root);
    let source = CheckpointTransferSource::from_store(
        &source_store,
        EventId::new(200).unwrap(),
        "source-node",
        checkpoint_id,
        brain_id,
        plan_digest,
        64,
    )
    .unwrap();
    let frames = source.frames().unwrap();
    assert!(frames.len() > 1);
    let mut receiver = CheckpointTransferReceiver::new(source.manifest().clone()).unwrap();
    for frame in frames.iter().rev() {
        receiver.accept(frame.clone()).unwrap();
    }
    let received = receiver.finalize().unwrap();
    let target_root = root.join("target-checkpoints");
    received.clone().publish(&target_root).unwrap();
    // An identical retry is safe and cannot replace the immutable file.
    received.publish(&target_root).unwrap();
    let source_payload = source_store.verify(checkpoint_id).unwrap().payload;
    let target_payload = StableExecutorCheckpointStore::new(&target_root)
        .unwrap()
        .verify(checkpoint_id)
        .unwrap()
        .payload;
    assert_eq!(source_payload, target_payload);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn transfer_rejects_a_tampered_frame_before_publication() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-checkpoint-transfer-tamper-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let (brain_id, plan_digest, source_store, checkpoint_id) = fixture(&root);
    let source = CheckpointTransferSource::from_store(
        &source_store,
        EventId::new(201).unwrap(),
        "source-node",
        checkpoint_id,
        brain_id,
        plan_digest,
        64,
    )
    .unwrap();
    let mut frame = source.frames().unwrap().remove(0);
    frame.payload[0] ^= 0x80;
    let mut receiver = CheckpointTransferReceiver::new(source.manifest().clone()).unwrap();
    assert!(receiver.accept(frame).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn transfer_service_publishes_only_after_the_stream_finishes() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-checkpoint-transfer-grpc-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let (brain_id, plan_digest, source_store, checkpoint_id) = fixture(&root);
    let source = CheckpointTransferSource::from_store(
        &source_store,
        EventId::new(202).unwrap(),
        "source-node",
        checkpoint_id,
        brain_id,
        plan_digest,
        64,
    )
    .unwrap();
    let activation_reference = source.manifest().activation_reference();
    let target_root = root.join("network-target");
    let service = StableCheckpointTransferService::new("target-node", &target_root).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(StableCheckpointTransferServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let acknowledgement = send_checkpoint_transfer(
        &format!("http://{address}"),
        "source-node",
        "target-node",
        source,
    )
    .await
    .unwrap();
    assert!(acknowledgement.accepted, "{acknowledgement:?}");
    assert!(acknowledgement.durable);
    assert_eq!(acknowledgement.checkpoint_id, checkpoint_id.raw());
    assert_eq!(acknowledgement.brain_id, brain_id.raw());
    assert_eq!(
        StableExecutorCheckpointStore::new(&target_root)
            .unwrap()
            .verify(checkpoint_id)
            .unwrap()
            .payload,
        source_store.verify(checkpoint_id).unwrap().payload
    );
    StableCheckpointTransferService::new("target-node", &target_root)
        .unwrap()
        .verify_activation_reference(&activation_reference)
        .unwrap();
    let mut invalid_reference = activation_reference.clone();
    invalid_reference.payload_digest.replace_range(..1, "0");
    assert!(
        StableCheckpointTransferService::new("target-node", &target_root)
            .unwrap()
            .verify_activation_reference(&invalid_reference)
            .is_err()
    );

    let _ = shutdown_tx.send(());
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
