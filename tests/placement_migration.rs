//! Public-contract verification for deterministic placement and migration.
//!
//! These tests intentionally use the library boundary.  They prove the
//! planner can be reused by an orchestrator, CLI or native product without
//! depending on the legacy distributed runner.

use aarnn_rust::deterministic::{BrainId, ComponentId, EventId, LeaseTerm, LogicalTag, ShardId};
use aarnn_rust::placement::{
    MigrationOperation, MigrationStage, PlacementCommand, PlacementCommandKind,
    PlacementConstraints, PlacementError, PlacementIntent, PlacementPlanner, PlacementRequest,
    RepartitionPlan, ResourceObservation, ShardDemand, ShardLineage, ShutdownReadiness,
};

fn resource(id: &str, domain: &str) -> ResourceObservation {
    ResourceObservation {
        node_id: id.to_owned(),
        device_id: format!("{id}-cpu"),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
        failure_domain: domain.to_owned(),
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

fn demand(id: u64, component: Option<u64>) -> ShardDemand {
    ShardDemand {
        shard_id: ShardId::new(id).expect("non-zero shard identity"),
        load_units: 20,
        memory_bytes: 500,
        checkpoint_bytes: 500,
        network_bytes_per_second: 20,
        zero_delay_component: component.map(|value| ComponentId::new(value).unwrap()),
        required_numerical_profile: "reference-cpu-v1".to_owned(),
        preferred_node: None,
    }
}

fn request(intent: PlacementIntent, demands: Vec<ShardDemand>) -> PlacementRequest {
    PlacementRequest {
        brain_id: BrainId::new(42).unwrap(),
        topology_generation: aarnn_rust::deterministic::TopologyGeneration::INITIAL,
        partition_generation: aarnn_rust::deterministic::PartitionGeneration::INITIAL,
        lease_term: LeaseTerm::INITIAL,
        fencing_token: LeaseTerm::INITIAL.raw(),
        effective_tag: LogicalTag::ZERO,
        demands,
        resources: vec![
            resource("laptop", "home"),
            resource("worker-a", "rack-a"),
            resource("worker-b", "rack-b"),
        ],
        constraints: PlacementConstraints {
            minimum_warm_replicas: 1,
            ..PlacementConstraints::default()
        },
        intent,
    }
}

#[test]
fn automatic_growth_uses_multiple_enrolled_resources_without_diluting_profile() {
    let plan = PlacementPlanner
        .plan(request(
            PlacementIntent::Automatic,
            vec![
                demand(1, None),
                demand(2, None),
                demand(3, None),
                demand(4, None),
            ],
        ))
        .expect("eligible fleet should admit the fixture");

    plan.verify().expect("planner must emit a verified plan");
    let active_nodes = plan
        .placements
        .iter()
        .map(|placement| placement.active_node.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        active_nodes.len() > 1,
        "the planner should use available capacity"
    );
    assert!(!plan.decision.degraded_durability);
}

#[test]
fn discovery_or_health_without_compute_authority_cannot_receive_a_shard() {
    let mut request = request(PlacementIntent::Automatic, vec![demand(1, None)]);
    request.resources[0].compute_authorised = false;
    request.resources[1].compute_authorised = false;
    request.resources[2].healthy = false;
    let error = PlacementPlanner
        .plan(request)
        .expect_err("no resource is admitted");
    assert!(matches!(error, PlacementError::NoEligibleNode { .. }));
}

#[test]
fn consolidation_co_locates_state_and_evacuation_excludes_the_origin() {
    let mut consolidate = request(
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        vec![demand(1, None)],
    );
    consolidate
        .constraints
        .allow_single_host_degraded_durability = true;
    let consolidated = PlacementPlanner.plan(consolidate).unwrap();
    assert_eq!(consolidated.placements[0].active_node, "laptop");
    assert!(consolidated.decision.degraded_durability);

    let evacuated = PlacementPlanner
        .plan(request(
            PlacementIntent::Evacuate {
                source_node: "laptop".to_owned(),
            },
            vec![demand(2, None), demand(3, None)],
        ))
        .unwrap();
    assert!(
        evacuated
            .placements
            .iter()
            .all(|placement| placement.active_node != "laptop")
    );
}

#[test]
fn public_migration_contract_rejects_stale_term_and_allows_resumable_cutover() {
    let mut operation = MigrationOperation::new(
        EventId::new(10).unwrap(),
        BrainId::new(42).unwrap(),
        aarnn_rust::deterministic::StateDigest([1; 16]),
        aarnn_rust::deterministic::StateDigest([2; 16]),
        "laptop",
        "worker-a",
        LeaseTerm::INITIAL,
    )
    .unwrap();
    operation.reserve().unwrap();
    operation.begin_transfer().unwrap();
    operation.begin_catch_up().unwrap();
    operation
        .mark_ready_for_cutover(LogicalTag::new(12, 0))
        .unwrap();
    assert!(operation.commit_cutover(LeaseTerm::INITIAL).is_err());
    operation
        .commit_cutover(LeaseTerm::new(2).unwrap())
        .unwrap();
    assert_eq!(operation.stage, MigrationStage::Committed);
    operation.verify().unwrap();
    assert!(operation.abort().is_err());
}

#[test]
fn shutdown_readiness_distinguishes_graceful_drain_from_power_loss() {
    let not_ready = ShutdownReadiness {
        brain_id: BrainId::new(42).unwrap(),
        node_id: "laptop".to_owned(),
        plan_digest: aarnn_rust::deterministic::StateDigest([1; 16]),
        checkpoint_digest: aarnn_rust::deterministic::StateDigest([2; 16]),
        safe_tag: LogicalTag::new(4, 0),
        active_shard_leases: 1,
        unacknowledged_committed_sends: 0,
        untransferred_output_commitments: 0,
        local_only_input_count: 0,
        control_plane_reachable: true,
    };
    assert!(not_ready.validate().is_err());

    let mut ready = not_ready;
    ready.active_shard_leases = 0;
    assert!(ready.validate().is_ok());
}

#[test]
fn preferred_device_is_used_without_bypassing_resource_constraints() {
    let mut request = request(PlacementIntent::Automatic, vec![demand(1, None)]);
    request.demands[0].preferred_node = Some("worker-b".to_owned());
    let plan = PlacementPlanner.plan(request).unwrap();
    assert_eq!(plan.placements[0].active_node, "worker-b");
    assert_eq!(plan.placements[0].active_device, "worker-b-cpu");
}

#[test]
fn fewer_shards_use_explicit_lineage_and_a_new_partition_generation() {
    let first = ShardId::new(1).unwrap();
    let second = ShardId::new(2).unwrap();
    let successor = ShardId::new(3).unwrap();
    let plan = RepartitionPlan::new(
        EventId::new(11).unwrap(),
        BrainId::new(42).unwrap(),
        aarnn_rust::deterministic::TopologyGeneration::INITIAL,
        aarnn_rust::deterministic::PartitionGeneration::INITIAL,
        aarnn_rust::deterministic::PartitionGeneration::new(2).unwrap(),
        LogicalTag::ZERO,
        vec![second, first],
        vec![successor],
        vec![
            ShardLineage {
                source_shard: second,
                successor_shards: vec![successor],
            },
            ShardLineage {
                source_shard: first,
                successor_shards: vec![successor],
            },
        ],
    )
    .unwrap();
    plan.verify().unwrap();
    assert_eq!(plan.source_shards, vec![first, second]);
}

#[test]
fn orchestrator_command_envelope_requires_a_verified_shutdown_boundary() {
    let readiness = ShutdownReadiness {
        brain_id: BrainId::new(42).unwrap(),
        node_id: "laptop".to_owned(),
        plan_digest: aarnn_rust::deterministic::StateDigest([1; 16]),
        checkpoint_digest: aarnn_rust::deterministic::StateDigest([2; 16]),
        safe_tag: LogicalTag::ZERO,
        active_shard_leases: 0,
        unacknowledged_committed_sends: 0,
        untransferred_output_commitments: 0,
        local_only_input_count: 0,
        control_plane_reachable: true,
    };
    let command = PlacementCommand::new(
        "request-shutdown",
        "shutdown-laptop-1",
        "operator",
        readiness.brain_id,
        19,
        LeaseTerm::INITIAL,
        PlacementCommandKind::PrepareForShutdown(readiness),
    )
    .unwrap();
    command.verify().unwrap();
}
