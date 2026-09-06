use aarnn_rust::causal::CausalEvent;
use aarnn_rust::deterministic::{
    BrainId, EventId, EventStage, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest,
    TopologyGeneration,
};
use aarnn_rust::partial_shard_executor::PartialShardOutbound;
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlanner, PlacementRequest, ResourceObservation,
    ShardDemand,
};
use aarnn_rust::placement_registry::{PlacementApplyRequest, PlacementRegistry};
use aarnn_rust::shard_executor::RoutedCausalEvent;
use aarnn_rust::stable_outbound::{StableOutboundError, StableOutboundLog};
use aarnn_rust::stable_shard_dispatch::{StableShardDispatchError, StableShardDispatcher};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

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

fn plan(term: LeaseTerm, topology: TopologyGeneration) -> aarnn_rust::placement::PlacementPlan {
    PlacementPlanner
        .plan(PlacementRequest {
            brain_id: BrainId::new(501).unwrap(),
            topology_generation: topology,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: term,
            fencing_token: term.raw(),
            effective_tag: LogicalTag::ZERO,
            demands: vec![ShardDemand {
                shard_id: ShardId::new(1).unwrap(),
                load_units: 10,
                memory_bytes: 100,
                checkpoint_bytes: 100,
                network_bytes_per_second: 10,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: None,
            }],
            resources: vec![resource("worker")],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: "worker".to_owned(),
            },
        })
        .unwrap()
}

fn registry(plan: aarnn_rust::placement::PlacementPlan) -> PlacementRegistry {
    let brain_id = plan.brain_id;
    let term = plan.lease_term;
    let mut registry = PlacementRegistry::new(brain_id, term);
    registry
        .apply(PlacementApplyRequest {
            request_id: format!("request-{}", plan.digest()),
            idempotency_key: format!("placement-{}", plan.digest()),
            expected_resource_version: 0,
            observed_leader_term: term,
            plan,
            cutover: None,
            repartition: None,
        })
        .unwrap();
    registry
}

fn message(event_id: u64) -> PartialShardOutbound {
    let event = EventId::new(event_id).unwrap();
    let neuron = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    PartialShardOutbound::CausalEvent {
        plan_digest: StateDigest([7; 16]),
        destination_shard: ShardId::new(1).unwrap(),
        event: RoutedCausalEvent {
            route: None,
            event: CausalEvent {
                key: aarnn_rust::deterministic::CanonicalEventKey::new(
                    LogicalTag::ZERO,
                    EventStage::SynapticTransition,
                    0,
                    neuron.raw(),
                    event.raw(),
                ),
                id: event,
                payload: vec![1, 2, 3],
                original_tag: LogicalTag::ZERO,
                deferred_from_nonconvergence: false,
            },
        },
    }
}

fn outbox_path(label: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "aarnn-stable-dispatch-{label}-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join("outbound.json")
}

async fn dispatcher(
    label: &str,
    placement: PlacementRegistry,
) -> (
    StableShardDispatcher,
    Arc<tokio::sync::Mutex<StableOutboundLog>>,
    PathBuf,
) {
    let path = outbox_path(label);
    let outbox = Arc::new(tokio::sync::Mutex::new(
        StableOutboundLog::open(&path, BrainId::new(501).unwrap(), 64).unwrap(),
    ));
    let dispatcher = StableShardDispatcher::new(
        "source",
        Arc::new(RwLock::new(placement)),
        Arc::clone(&outbox),
    )
    .unwrap();
    (dispatcher, outbox, path)
}

#[tokio::test]
async fn planner_registry_and_dispatcher_preserve_physical_plan_identity() {
    let physical_plan = plan(LeaseTerm::INITIAL, TopologyGeneration::INITIAL);
    physical_plan.verify().unwrap();
    let placement_digest = physical_plan.digest();
    let (dispatcher, outbox, path) = dispatcher("identity", registry(physical_plan)).await;

    let record = dispatcher.enqueue(message(1)).await.unwrap();
    assert_eq!(record.destination_node, "worker");
    assert_eq!(record.placement_plan_digest, placement_digest);
    assert_eq!(record.plan_digest, StateDigest([7; 16]));
    assert_ne!(record.placement_plan_digest, record.plan_digest);
    assert_eq!(outbox.lock().await.pending("worker").unwrap().len(), 1);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn dispatcher_rejects_missing_or_invalid_endpoints() {
    let (dispatcher, _, path) = dispatcher(
        "endpoint",
        registry(plan(LeaseTerm::INITIAL, TopologyGeneration::INITIAL)),
    )
    .await;
    dispatcher.enqueue(message(2)).await.unwrap();
    assert!(matches!(
        dispatcher.dispatch_pending().await,
        Err(StableShardDispatchError::MissingEndpoint(node)) if node == "worker"
    ));
    assert!(matches!(
        dispatcher.register_endpoint("worker", "ftp://127.0.0.1:1"),
        Err(StableShardDispatchError::InvalidEndpoint)
    ));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn failed_connection_keeps_durable_records_pending_for_retry() {
    let (dispatcher, outbox, path) = dispatcher(
        "retry",
        registry(plan(LeaseTerm::INITIAL, TopologyGeneration::INITIAL)),
    )
    .await;
    dispatcher.enqueue(message(3)).await.unwrap();
    dispatcher
        .register_endpoint("worker", "http://127.0.0.1:1")
        .unwrap();
    assert!(matches!(
        dispatcher.dispatch_pending().await,
        Err(StableShardDispatchError::Transport(_))
    ));
    assert_eq!(outbox.lock().await.pending("worker").unwrap().len(), 1);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn stale_placement_fence_or_generation_is_rejected_before_network_send() {
    for (label, replacement) in [
        (
            "stale-placement",
            plan(LeaseTerm::INITIAL, TopologyGeneration::new(2).unwrap()),
        ),
        (
            "stale-fence",
            plan(LeaseTerm::new(2).unwrap(), TopologyGeneration::INITIAL),
        ),
    ] {
        let placement = registry(plan(LeaseTerm::INITIAL, TopologyGeneration::INITIAL));
        let placement_state = Arc::new(RwLock::new(placement));
        let path = outbox_path(label);
        let outbox = Arc::new(tokio::sync::Mutex::new(
            StableOutboundLog::open(&path, BrainId::new(501).unwrap(), 64).unwrap(),
        ));
        let dispatcher =
            StableShardDispatcher::new("source", Arc::clone(&placement_state), Arc::clone(&outbox))
                .unwrap();
        dispatcher.enqueue(message(10)).await.unwrap();
        *placement_state.write().unwrap() = registry(replacement);
        dispatcher
            .register_endpoint("worker", "http://127.0.0.1:1")
            .unwrap();
        assert!(matches!(
            dispatcher.dispatch_pending().await,
            Err(StableShardDispatchError::PlacementDigestMismatch { shard: 1 })
        ));
        assert_eq!(outbox.lock().await.pending("worker").unwrap().len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn bounded_batch_enqueue_fails_before_unbounded_work() {
    let (dispatcher, _, path) = dispatcher(
        "batch",
        registry(plan(LeaseTerm::INITIAL, TopologyGeneration::INITIAL)),
    )
    .await;
    let messages = (0..4097).map(|id| message(id + 20)).collect::<Vec<_>>();
    assert!(matches!(
        dispatcher.enqueue_batch(messages).await,
        Err(StableShardDispatchError::BatchTooLarge(4096))
    ));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn durable_batch_enqueue_rolls_back_when_a_later_record_hits_the_queue_bound() {
    let physical_plan = plan(LeaseTerm::INITIAL, TopologyGeneration::INITIAL);
    let placement = registry(physical_plan);
    let path = outbox_path("atomic");
    let outbox = Arc::new(tokio::sync::Mutex::new(
        StableOutboundLog::open(&path, BrainId::new(501).unwrap(), 1).unwrap(),
    ));
    let dispatcher =
        StableShardDispatcher::new("source", Arc::new(RwLock::new(placement)), outbox.clone())
            .unwrap();

    assert!(matches!(
        dispatcher.enqueue_batch([message(30), message(31)]).await,
        Err(StableShardDispatchError::Outbound(
            StableOutboundError::QueueFull { .. }
        ))
    ));
    assert!(outbox.lock().await.pending("worker").unwrap().is_empty());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
