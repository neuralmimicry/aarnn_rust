//! Integration coverage for the bounded automatic placement coordinator.
//!
//! These tests use the same immutable stable-runtime/checkpoint shape as the
//! worker bootstrap tests.  They deliberately stop at the orchestrator
//! activation boundary: a physical migration is admitted only after a
//! separately verified cutover proof, and that proof cannot be fabricated by
//! a placement proposal alone.

#![cfg(feature = "stable_executor_live")]

use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::deterministic::{
    ComponentId, EventId, LeaseTerm, LogicalTag, NeuronId, PartitionGeneration, ShardId,
    StateDigest, StreamId, TopologyGeneration,
};
use aarnn_rust::managed_durability::managed_brain_id;
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, ResourceObservation, ShardDemand,
};
use aarnn_rust::placement_automation::{
    PLACEMENT_AUTOMATION_SCHEMA_VERSION, PlacementActivationDispatch,
    PlacementAutomationCoordinator, PlacementAutomationError, PlacementAutomationSpec,
    PlacementReconcileOutcome,
};
use aarnn_rust::placement_controller::AutomaticPlacementPolicy;
use aarnn_rust::placement_registry::{
    CutoverEvidence, PlacementActivationState, ShardCutoverEvidence,
};
use aarnn_rust::shard_executor::StableShardExecutor;
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
use aarnn_rust::stable_runtime_bootstrap::{
    STABLE_RUNTIME_BOOTSTRAP_SCHEMA_VERSION, StableRuntimeBootstrapManifest,
};
use aarnn_rust::stable_worker::StableWorkerCheckpointTransferReference;
use aarnn_rust::topology_model::{
    NeuronRecord, TopologyGenerationModel, VirtualShardAssignment, compile_execution_plan,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn test_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "aarnn-placement-automation-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn runtime_fixture(root: &Path, network_id: &str) -> StableRuntimeBootstrapManifest {
    let brain_id = managed_brain_id(network_id);
    let neurons = vec![
        NeuronRecord {
            id: NeuronId::new(1).unwrap(),
        },
        NeuronRecord {
            id: NeuronId::new(2).unwrap(),
        },
    ];
    let topology =
        TopologyGenerationModel::new(TopologyGeneration::INITIAL, neurons.clone(), Vec::new())
            .unwrap();
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
        assignments.clone(),
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
    let checkpoint_root = root.join("checkpoints");
    StableExecutorCheckpointStore::new(&checkpoint_root)
        .unwrap()
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

fn resource(
    node_id: &str,
    failure_domain: &str,
    enrolled: bool,
    authorised: bool,
) -> ResourceObservation {
    ResourceObservation {
        node_id: node_id.to_owned(),
        device_id: format!("{node_id}-cpu"),
        healthy: true,
        enrolled,
        compute_authorised: authorised,
        failure_domain: failure_domain.to_owned(),
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

fn spec(root: &Path, network_id: &str) -> PlacementAutomationSpec {
    let runtime = runtime_fixture(root, network_id);
    let allowed_nodes = ["worker-a", "worker-b"]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    PlacementAutomationSpec {
        schema_version: PLACEMENT_AUTOMATION_SCHEMA_VERSION,
        network_id: network_id.to_owned(),
        runtime,
        demands: vec![
            ShardDemand {
                shard_id: ShardId::new(10).unwrap(),
                load_units: 1,
                memory_bytes: 1,
                checkpoint_bytes: 100,
                network_bytes_per_second: 1,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: Some("worker-a".to_owned()),
            },
            ShardDemand {
                shard_id: ShardId::new(20).unwrap(),
                load_units: 1,
                memory_bytes: 1,
                checkpoint_bytes: 100,
                network_bytes_per_second: 1,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: Some("worker-b".to_owned()),
            },
        ],
        constraints: PlacementConstraints {
            minimum_warm_replicas: 0,
            ..PlacementConstraints::default()
        },
        allowed_nodes,
        failure_domains: [
            ("worker-a".to_owned(), "rack-a".to_owned()),
            ("worker-b".to_owned(), "rack-b".to_owned()),
        ]
        .into_iter()
        .collect(),
        source_nodes: ["worker-a", "worker-b"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        endpoint_addresses: [
            ("worker-a".to_owned(), "http://127.0.0.1:50052".to_owned()),
            ("worker-b".to_owned(), "http://127.0.0.1:50053".to_owned()),
        ]
        .into_iter()
        .collect(),
        checkpoint_transfers: BTreeMap::new(),
        worker_state_root: root.join("workers"),
        storage_bytes_per_node: 10_000,
        network_bytes_per_second_per_node: 10_000,
        max_pending_outbound: 16,
        max_outbound_per_step: 16,
    }
}

#[test]
fn activation_binds_a_validated_target_local_checkpoint_receipt() {
    let root = test_root("checkpoint-reference");
    let _ = std::fs::remove_dir_all(&root);
    let network_id = "placement-automation-checkpoint-reference";
    let mut configuration = spec(&root, network_id);
    let runtime = configuration.runtime.clone();
    configuration.checkpoint_transfers.insert(
        "worker-b".to_owned(),
        StableWorkerCheckpointTransferReference {
            schema_version: 1,
            transfer_id: 101,
            checkpoint_id: runtime.checkpoint_id.raw(),
            brain_id: runtime.brain_id.raw(),
            lease_term: runtime.lease_term.raw(),
            partition_generation: runtime.partition_generation.raw(),
            plan_digest: runtime.plan_digest.to_string(),
            payload_digest: "11".repeat(16),
            manifest_digest: "22".repeat(16),
        },
    );
    let mut coordinator = open_coordinator_with_spec(configuration, &root);
    let outcome = coordinator
        .reconcile(
            resources(),
            LogicalTag::ZERO,
            PlacementIntent::Automatic,
            0,
            None,
        )
        .unwrap();
    let PlacementReconcileOutcome::Applied { activations, .. } = outcome else {
        panic!("the first authoritative observation must publish a placement");
    };
    let worker_b = activations
        .iter()
        .find(|activation| activation.target_node == "worker-b")
        .expect("worker-b activation must be present");
    assert_eq!(
        worker_b
            .command
            .checkpoint_transfer
            .as_ref()
            .unwrap()
            .transfer_id,
        101
    );
    worker_b.command.verify().unwrap();
    drop(coordinator);
    let _ = std::fs::remove_dir_all(root);
}

fn resources() -> Vec<ResourceObservation> {
    vec![
        resource("worker-a", "rack-a", true, true),
        resource("worker-b", "rack-b", true, true),
        // These observations must never become eligible merely because they
        // advertise enough capacity.
        resource("un-enrolled", "rack-c", false, true),
        resource("unauthorised", "rack-d", true, false),
        resource("outside-grant", "rack-e", true, true),
    ]
}

fn open_coordinator(root: &Path, network_id: &str) -> PlacementAutomationCoordinator {
    open_coordinator_with_spec(spec(root, network_id), root)
}

fn open_coordinator_with_spec(
    configuration: PlacementAutomationSpec,
    root: &Path,
) -> PlacementAutomationCoordinator {
    PlacementAutomationCoordinator::open(
        configuration,
        root.join("placement.json"),
        AutomaticPlacementPolicy::default(),
    )
    .unwrap()
}

fn activation_manifest(dispatch: &PlacementActivationDispatch) -> serde_json::Value {
    serde_json::from_str(&dispatch.command.manifest_json).unwrap()
}

#[test]
fn initial_reconcile_publishes_one_bounded_activation_per_target_and_reopens_for_retry() {
    let root = test_root("initial");
    let _ = std::fs::remove_dir_all(&root);
    let network_id = "placement-automation-initial";
    // Reuse the same validated runtime manifest on reopen.  Recreating the
    // fixture would attempt to publish checkpoint 100 a second time, which
    // the immutable checkpoint store must reject.
    let configuration = spec(&root, network_id);
    let mut coordinator = open_coordinator_with_spec(configuration.clone(), &root);
    let outcome = coordinator
        .reconcile(
            resources(),
            LogicalTag::ZERO,
            PlacementIntent::Automatic,
            0,
            None,
        )
        .unwrap();
    let PlacementReconcileOutcome::Applied {
        activations, plan, ..
    } = outcome
    else {
        panic!("the first authoritative observation must publish a placement");
    };
    assert_eq!(plan.placements.len(), 2);
    assert_eq!(activations.len(), 2);
    assert_eq!(
        activations
            .iter()
            .map(|activation| activation.target_node.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["worker-a", "worker-b"])
    );
    for activation in &activations {
        activation.command.verify().unwrap();
        assert_eq!(
            activation.command.placement_idempotency_key,
            coordinator
                .registry()
                .state()
                .activation_statuses
                .get(&activation.activation_idempotency_key)
                .unwrap()
                .placement_idempotency_key
        );
        let manifest = activation_manifest(activation);
        let owned_shards = manifest["owned_shards"].as_array().unwrap();
        assert_eq!(owned_shards.len(), 1);
        assert_eq!(
            manifest["node_id"].as_str(),
            Some(activation.target_node.as_str())
        );
    }

    // Release the first registry before reopening.  The runtime fixture also
    // publishes its immutable checkpoint, so rebuilding the spec while the
    // first fixture is alive would correctly be rejected as a duplicate
    // publication rather than exercising restart recovery.
    drop(coordinator);
    let reopened = open_coordinator_with_spec(configuration, &root);
    let retryable = reopened.retryable_activation_dispatches().unwrap();
    assert_eq!(retryable.len(), 2);
    assert!(retryable.iter().all(|activation| {
        activation.command.verify().is_ok()
            && activation.command.target_node == activation.target_node
    }));
    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unauthorised_or_unenrolled_resources_cannot_drive_placement() {
    let root = test_root("filter");
    let _ = std::fs::remove_dir_all(&root);
    let mut coordinator = open_coordinator(&root, "placement-automation-filter");
    let error = coordinator
        .reconcile(
            vec![
                resource("outside-grant", "rack-e", true, true),
                resource("un-enrolled", "rack-c", false, true),
                resource("unauthorised", "rack-d", true, false),
            ],
            LogicalTag::ZERO,
            PlacementIntent::Automatic,
            0,
            None,
        )
        .expect_err("resource observations must cross the explicit grant boundary");
    assert!(matches!(
        error,
        PlacementAutomationError::NoEligibleResources
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn movement_is_rejected_without_cutover_evidence_and_accepted_with_complete_proof() {
    let root = test_root("cutover");
    let _ = std::fs::remove_dir_all(&root);
    let network_id = "placement-automation-cutover";
    let mut coordinator = open_coordinator(&root, network_id);
    let initial = coordinator
        .reconcile(
            resources(),
            LogicalTag::ZERO,
            PlacementIntent::Consolidate {
                target_node: "worker-a".to_owned(),
            },
            0,
            None,
        )
        .unwrap();
    let PlacementReconcileOutcome::Applied {
        plan: initial_plan,
        activations,
        ..
    } = initial
    else {
        panic!("initial placement must be applied");
    };
    for activation in &activations {
        coordinator
            .registry_mut()
            .record_activation_outcome(
                &activation.activation_idempotency_key,
                PlacementActivationState::Active,
                "",
            )
            .unwrap();
    }
    coordinator.registry_mut().commit_prepared().unwrap();
    let missing_proof = coordinator.reconcile(
        resources(),
        LogicalTag::new(1, 0),
        PlacementIntent::Consolidate {
            target_node: "worker-b".to_owned(),
        },
        0,
        None,
    );
    assert!(matches!(
        missing_proof,
        Err(PlacementAutomationError::Controller(_))
    ));

    let shards = [10_u64, 20_u64]
        .into_iter()
        .map(|shard| {
            (
                ShardId::new(shard).unwrap(),
                ShardCutoverEvidence {
                    source_node: "worker-a".to_owned(),
                    source_term: LeaseTerm::INITIAL,
                    checkpoint_digest: StateDigest([shard as u8; 16]),
                    caught_up: true,
                    route_cursor_digest: StateDigest([7; 16]),
                    effect_cursor_digest: StateDigest([8; 16]),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let proof = CutoverEvidence {
        operation_id: EventId::new(902).unwrap(),
        source_plan_digest: initial_plan.digest(),
        cut_tag: LogicalTag::new(1, 0),
        destination_term: LeaseTerm::INITIAL,
        shards,
    };
    let moved = coordinator
        .reconcile(
            resources(),
            LogicalTag::new(1, 0),
            PlacementIntent::Consolidate {
                target_node: "worker-b".to_owned(),
            },
            0,
            Some(proof),
        )
        .unwrap();
    let PlacementReconcileOutcome::Applied { plan, review, .. } = moved else {
        panic!("verified movement must publish a new placement");
    };
    assert!(review.requires_migration);
    assert!(
        plan.placements
            .iter()
            .all(|placement| placement.active_node == "worker-b")
    );
    drop(coordinator);
    let _ = std::fs::remove_dir_all(root);
}
