#![cfg(feature = "stable_executor_live")]

use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::deterministic::{
    EventId, LeaseTerm, NeuronId, PartitionGeneration, ShardId, StateDigest, StreamId,
    TopologyGeneration,
};
use aarnn_rust::distributed::ManagedNetwork;
use aarnn_rust::distributed::{DistributedNode, proto::NetworkCommand};
use aarnn_rust::managed_durability::managed_brain_id;
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlanner, PlacementRequest, ResourceObservation,
    ShardDemand,
};
use aarnn_rust::shard_executor::StableShardExecutor;
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
use aarnn_rust::stable_runtime_bootstrap::{
    STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION, StablePartialWorkerBootstrapManifest,
    StableRuntimeBootstrapError, StableRuntimeBootstrapManifest, StableWorkerEndpoint,
};
use aarnn_rust::topology_model::{
    NeuronRecord, TopologyGenerationModel, VirtualShardAssignment, compile_execution_plan,
};

fn fixture(root: &std::path::Path) -> StableRuntimeBootstrapManifest {
    let brain_id = managed_brain_id("bootstrap-brain");
    let neurons = vec![
        NeuronRecord {
            id: NeuronId::new(1).unwrap(),
        },
        NeuronRecord {
            id: NeuronId::new(2).unwrap(),
        },
    ];
    let topology =
        TopologyGenerationModel::new(TopologyGeneration::INITIAL, neurons.clone(), vec![]).unwrap();
    let assignments = vec![
        VirtualShardAssignment {
            shard: ShardId::new(10).unwrap(),
            components: vec![aarnn_rust::deterministic::ComponentId::new(1).unwrap()],
            load: 1,
        },
        VirtualShardAssignment {
            shard: ShardId::new(20).unwrap(),
            components: vec![aarnn_rust::deterministic::ComponentId::new(2).unwrap()],
            load: 1,
        },
    ];
    let plan = compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        assignments.clone(),
        vec![],
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
    let checkpoint_root = root.join("checkpoints");
    let store = StableExecutorCheckpointStore::new(&checkpoint_root).unwrap();
    store
        .publish(EventId::new(100).unwrap(), LeaseTerm::INITIAL, &executor)
        .unwrap();

    StableRuntimeBootstrapManifest {
        schema_version: STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION,
        brain_id,
        topology_generation: topology.generation,
        topology_digest: topology.digest(),
        neurons,
        synapses: Vec::new(),
        partition_generation: PartitionGeneration::INITIAL,
        assignments,
        ownership: Vec::new(),
        plan_digest: plan.digest(),
        checkpoint_id: EventId::new(100).unwrap(),
        checkpoint_root,
        owner_root: root.join("owners"),
        warm_root: root.join("warm"),
        lease_term: LeaseTerm::INITIAL,
        fencing_token: 1,
        stream_id: StreamId::new(77).unwrap(),
        max_payload: 1024 * 1024,
        max_input_events: 8,
        max_steps_per_poll: 8,
        threshold: FIXED_POINT_SCALE,
        weight: FIXED_POINT_SCALE,
        queue_capacity: 32,
        dedupe_capacity: 128,
        channel_state: Vec::new(),
        sensory_targets: vec![NeuronId::new(1).unwrap()],
    }
}

fn worker_placement(
    brain_id: aarnn_rust::deterministic::BrainId,
) -> aarnn_rust::placement::PlacementPlan {
    PlacementPlanner
        .plan(PlacementRequest {
            brain_id,
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: LeaseTerm::INITIAL,
            fencing_token: 1,
            effective_tag: aarnn_rust::deterministic::LogicalTag::ZERO,
            demands: vec![
                ShardDemand {
                    shard_id: ShardId::new(10).unwrap(),
                    load_units: 1,
                    memory_bytes: 1,
                    checkpoint_bytes: 1,
                    network_bytes_per_second: 1,
                    zero_delay_component: None,
                    required_numerical_profile: "reference-cpu-v1".to_owned(),
                    preferred_node: Some("worker-a".to_owned()),
                },
                ShardDemand {
                    shard_id: ShardId::new(20).unwrap(),
                    load_units: 1,
                    memory_bytes: 1,
                    checkpoint_bytes: 1,
                    network_bytes_per_second: 1,
                    zero_delay_component: None,
                    required_numerical_profile: "reference-cpu-v1".to_owned(),
                    preferred_node: Some("worker-b".to_owned()),
                },
            ],
            resources: ["worker-a", "worker-b"]
                .into_iter()
                .map(|node_id| ResourceObservation {
                    node_id: node_id.to_owned(),
                    device_id: format!("{node_id}-cpu"),
                    healthy: true,
                    enrolled: true,
                    compute_authorised: true,
                    failure_domain: format!("domain-{node_id}"),
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
                })
                .collect(),
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Automatic,
        })
        .unwrap()
}

#[tokio::test]
async fn orchestrator_activation_command_registers_and_deduplicates_partial_worker() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-worker-command-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let runtime = fixture(&root);
    let placement = worker_placement(runtime.brain_id);
    let manifest = StablePartialWorkerBootstrapManifest::from_authoritative_state(
        runtime,
        placement,
        "worker-b",
        vec![ShardId::new(20).unwrap()],
        vec!["worker-a".to_owned()],
        root.join("command-receiver.json"),
        root.join("command-outbound.json"),
        16,
        16,
        vec![StableWorkerEndpoint {
            node_id: "worker-a".to_owned(),
            address: "http://127.0.0.1:50052".to_owned(),
        }],
    )
    .expect("validated partial-worker manifest");
    let command = manifest
        .activation_command("activation-request-1", 1, "bootstrap-brain")
        .unwrap();
    let wire = NetworkCommand {
        r#type: aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
            as i32,
        network_id: "bootstrap-brain".to_owned(),
        config_json: serde_json::to_vec(&command).unwrap(),
        ..Default::default()
    };
    let node = DistributedNode::new("worker-b".to_owned(), false);
    node.handle_command(wire.clone()).await;
    let registrations = node.get_stable_executor_registrations().await;
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].owned_shard_ids, vec![20]);

    node.handle_command(wire).await;
    assert_eq!(node.get_stable_executor_registrations().await.len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn manifest_factory_rejects_shards_not_active_on_target() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-worker-manifest-factory-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let runtime = fixture(&root);
    let placement = worker_placement(runtime.brain_id);
    let result = StablePartialWorkerBootstrapManifest::from_authoritative_state(
        runtime,
        placement,
        "worker-b",
        vec![ShardId::new(10).unwrap()],
        vec!["worker-a".to_owned()],
        root.join("receiver.json"),
        root.join("outbound.json"),
        16,
        16,
        Vec::new(),
    );
    assert!(matches!(
        result,
        Err(StableRuntimeBootstrapError::Placement(message))
            if message.contains("not active on the declared worker node")
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bootstrap_reopens_verified_cut_and_drives_bounded_sensory_input() {
    let root = std::env::temp_dir().join(format!("aarnn-stable-bootstrap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let manifest = fixture(&root);
    let mut bootstrap = manifest
        .clone()
        .open(Some(manifest.brain_id))
        .expect("verified bootstrap");
    assert_eq!(bootstrap.runtime.sensory_target_count(), 1);
    let poll = bootstrap
        .runtime
        .poll_sensory(LeaseTerm::INITIAL, 1, &[1])
        .expect("bounded sensory poll");
    assert_eq!(poll.steps.len(), 1);
    assert!(poll.is_quiescent());
    drop(bootstrap);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bootstrap_holds_one_authority_lock_and_allows_reopen_after_release() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-bootstrap-lock-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let manifest = fixture(&root);
    let first = manifest.clone().open(Some(manifest.brain_id)).unwrap();
    let error = manifest
        .clone()
        .open(Some(manifest.brain_id))
        .expect_err("duplicate authority must be rejected");
    assert!(matches!(
        error,
        StableRuntimeBootstrapError::AuthorityAlreadyHeld
    ));
    drop(first);
    let reopened = manifest
        .clone()
        .open(Some(manifest.brain_id))
        .expect("authority can be reopened after the owner exits");
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bootstrap_rejects_topology_plan_and_checkpoint_mismatches() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-bootstrap-mismatch-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let manifest = fixture(&root);

    let mut topology_mismatch = manifest.clone();
    topology_mismatch.topology_digest = StateDigest([1; 16]);
    assert!(matches!(
        topology_mismatch.compile_plan(),
        Err(StableRuntimeBootstrapError::DigestMismatch { field: "topology" })
    ));

    let mut plan_mismatch = manifest.clone();
    plan_mismatch.plan_digest = StateDigest([2; 16]);
    assert!(matches!(
        plan_mismatch.compile_plan(),
        Err(StableRuntimeBootstrapError::DigestMismatch { field: "plan" })
    ));

    let mut checkpoint_mismatch = manifest.clone();
    checkpoint_mismatch.lease_term = LeaseTerm::new(2).unwrap();
    assert!(matches!(
        checkpoint_mismatch.open(Some(manifest.brain_id)),
        Err(StableRuntimeBootstrapError::CheckpointMismatch)
    ));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_network_registers_stable_runtime_without_a_legacy_durable_owner() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-managed-network-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let manifest = fixture(&root);
    let bootstrap = manifest
        .clone()
        .open(Some(manifest.brain_id))
        .expect("verified bootstrap");
    let config = NetworkConfig::default();
    let lif = LIFParams::default();
    let stdp = STDPParams::default();
    let runner = aarnn_rust::runner::Runner::new(
        lif.clone(),
        stdp.clone(),
        config.clone(),
        aarnn_rust::sim::NeuronModel::Lif,
        aarnn_rust::sim::Learning::Stdp,
    );
    let mut network = ManagedNetwork::new(
        "bootstrap-brain".to_owned(),
        runner,
        config,
        aarnn_rust::sim::NeuronModel::Lif,
        aarnn_rust::sim::Learning::Stdp,
        lif,
        stdp,
    );
    network
        .register_stable_executor(bootstrap.runtime)
        .expect("stable runtime registration");
    network.playing = true;
    let poll = network
        .poll_stable_executor_sensory(Some(&[1]))
        .expect("stable sensory poll");
    assert_eq!(poll.steps.len(), 1);
    assert!(network.stable_executor_registered());
    assert!(network.durable_owner.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_worker_bootstrap_materialises_only_the_authorised_shard_subset() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-partial-worker-bootstrap-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let runtime = fixture(&root);
    let placement = worker_placement(runtime.brain_id);
    assert_eq!(
        placement
            .placements
            .iter()
            .find(|placement| placement.shard_id == ShardId::new(20).unwrap())
            .unwrap()
            .active_node,
        "worker-b"
    );
    let manifest = StablePartialWorkerBootstrapManifest {
        schema_version: StablePartialWorkerBootstrapManifest::SCHEMA_VERSION,
        runtime,
        node_id: "worker-b".to_owned(),
        owned_shards: vec![ShardId::new(20).unwrap()],
        allowed_source_nodes: vec!["worker-a".to_owned()],
        receiver_path: root.join("receiver.json"),
        outbound_path: root.join("outbound.json"),
        max_pending_outbound: 16,
        max_outbound_per_step: 16,
        endpoints: vec![StableWorkerEndpoint {
            node_id: "worker-a".to_owned(),
            address: "http://127.0.0.1:50052".to_owned(),
        }],
        placement,
        checkpoint_lease_term: None,
    };
    let bootstrap = manifest.open().expect("partial worker bootstrap");
    assert_eq!(
        bootstrap.receiver.owned_shard_ids(),
        vec![ShardId::new(20).unwrap()]
    );
    assert!(bootstrap.receiver.pending_outbound().is_empty());
    drop(bootstrap);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_worker_bootstrap_rejects_a_shard_not_active_on_the_declared_node() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-partial-worker-bootstrap-reject-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let runtime = fixture(&root);
    let manifest = StablePartialWorkerBootstrapManifest {
        schema_version: StablePartialWorkerBootstrapManifest::SCHEMA_VERSION,
        runtime,
        node_id: "worker-b".to_owned(),
        owned_shards: vec![ShardId::new(10).unwrap()],
        allowed_source_nodes: vec!["worker-a".to_owned()],
        receiver_path: root.join("receiver.json"),
        outbound_path: root.join("outbound.json"),
        max_pending_outbound: 16,
        max_outbound_per_step: 16,
        endpoints: Vec::new(),
        placement: worker_placement(managed_brain_id("bootstrap-brain")),
        checkpoint_lease_term: None,
    };
    assert!(matches!(
        manifest.open(),
        Err(StableRuntimeBootstrapError::Placement(_))
    ));
    std::fs::remove_dir_all(root).unwrap();
}
