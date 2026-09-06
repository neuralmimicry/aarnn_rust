use aarnn_rust::deterministic::{
    BrainId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StateDigest, TopologyGeneration,
};
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlanner, PlacementRequest, ResourceObservation,
    ShardDemand,
};
use aarnn_rust::placement_controller::{
    AutomaticPlacementPolicy, PlacementController, PlacementControllerError,
};
use std::collections::BTreeMap;

fn resource(node: &str) -> ResourceObservation {
    ResourceObservation {
        node_id: node.to_owned(),
        device_id: format!("{node}-cpu"),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
        failure_domain: format!("domain-{node}"),
        numerical_profiles: vec!["deterministic-cpu".to_owned()],
        capacity_units: 100,
        reserved_capacity_units: 0,
        memory_bytes: 1_000,
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

fn demand(shard: u64, load_units: u64) -> ShardDemand {
    ShardDemand {
        shard_id: ShardId::new(shard).unwrap(),
        load_units,
        memory_bytes: 100,
        checkpoint_bytes: 100,
        network_bytes_per_second: 100,
        zero_delay_component: None,
        required_numerical_profile: "deterministic-cpu".to_owned(),
        preferred_node: None,
    }
}

fn request(resources: Vec<ResourceObservation>, intent: PlacementIntent) -> PlacementRequest {
    PlacementRequest {
        brain_id: BrainId::new(7).unwrap(),
        topology_generation: TopologyGeneration::INITIAL,
        partition_generation: PartitionGeneration::INITIAL,
        lease_term: LeaseTerm::INITIAL,
        fencing_token: LeaseTerm::INITIAL.raw(),
        effective_tag: LogicalTag::ZERO,
        demands: vec![demand(1, 80), demand(2, 20)],
        resources,
        constraints: PlacementConstraints {
            // This fixture intentionally exercises controller hysteresis and
            // transfer limits. Use the full synthetic capacity so the two
            // demands fit on the single-node baseline; production placement
            // tests cover headroom admission separately.
            minimum_headroom_milli: 0,
            minimum_warm_replicas: 0,
            ..PlacementConstraints::default()
        },
        intent,
    }
}

fn plans() -> (
    aarnn_rust::placement::PlacementPlan,
    aarnn_rust::placement::PlacementPlan,
) {
    let planner = PlacementPlanner;
    let current = planner
        .plan(request(vec![resource("a")], PlacementIntent::Automatic))
        .unwrap();
    let proposed = planner
        .plan(request(
            vec![resource("a"), resource("b")],
            PlacementIntent::Automatic,
        ))
        .unwrap();
    (current, proposed)
}

fn demands() -> BTreeMap<ShardId, ShardDemand> {
    [demand(1, 80), demand(2, 20)]
        .into_iter()
        .map(|demand| (demand.shard_id, demand))
        .collect()
}

fn controller(minimum_residence_quanta: u64) -> PlacementController {
    let (current, _) = plans();
    let mut controller = PlacementController::new(AutomaticPlacementPolicy {
        minimum_residence_quanta,
        minimum_improvement_milli: 50,
        maximum_concurrent_migrations: 1,
        migration_budget_bytes: 200,
    })
    .unwrap();
    controller.adopt(current).unwrap();
    controller
}

#[test]
fn automatic_review_requires_residence_and_measurable_benefit() {
    let (_, proposed) = plans();
    let controller = controller(100);
    let blocked = controller
        .review(
            &proposed,
            &demands(),
            &[resource("a"), resource("b")],
            LogicalTag::new(10, 0),
            0,
        )
        .unwrap_err();
    assert!(
        matches!(blocked, PlacementControllerError::Blocked(message) if message.contains("residence"))
    );

    let review = controller
        .review(
            &proposed,
            &demands(),
            &[resource("a"), resource("b")],
            LogicalTag::new(200, 0),
            0,
        )
        .unwrap();
    assert!(review.approved);
    assert_eq!(review.moved_shards.len(), 1);
    assert_eq!(review.estimated_transfer_bytes, 100);
    assert!(review.improvement_milli >= 50);
}

#[test]
fn automatic_review_enforces_budget_and_concurrency_limits() {
    let (_, proposed) = plans();
    let mut controller = controller(0);
    controller.policy.migration_budget_bytes = 99;
    let budget = controller
        .review(
            &proposed,
            &demands(),
            &[resource("a"), resource("b")],
            LogicalTag::new(200, 0),
            0,
        )
        .unwrap_err();
    assert!(
        matches!(budget, PlacementControllerError::Blocked(message) if message.contains("budget"))
    );

    controller.policy.migration_budget_bytes = 200;
    let concurrent = controller
        .review(
            &proposed,
            &demands(),
            &[resource("a"), resource("b")],
            LogicalTag::new(200, 0),
            1,
        )
        .unwrap_err();
    assert!(
        matches!(concurrent, PlacementControllerError::Blocked(message) if message.contains("active"))
    );
}

#[test]
fn unhealthy_active_node_allows_emergency_review_without_hysteresis_bypass_for_healthy_nodes() {
    let (_, proposed) = plans();
    let controller = controller(1_000);
    let mut unhealthy = resource("a");
    unhealthy.healthy = false;
    let review = controller
        .review(
            &proposed,
            &demands(),
            &[unhealthy, resource("b")],
            LogicalTag::new(1, 0),
            0,
        )
        .unwrap();
    assert!(review.approved);
    assert!(review.emergency);
}

#[test]
fn explicit_operator_move_can_bypass_optimisation_hysteresis_but_commit_updates_residence() {
    let planner = PlacementPlanner;
    let current = planner
        .plan(request(vec![resource("a")], PlacementIntent::Automatic))
        .unwrap();
    let proposed = planner
        .plan(request(
            vec![resource("a"), resource("b")],
            PlacementIntent::Consolidate {
                target_node: "b".to_owned(),
            },
        ))
        .unwrap();
    let mut controller = controller(1_000);
    let review = controller
        .review(
            &proposed,
            &demands(),
            &[resource("a"), resource("b")],
            LogicalTag::new(1, 0),
            0,
        )
        .unwrap();
    assert!(review.approved);
    controller
        .commit(proposed, &review, LogicalTag::new(1, 0))
        .unwrap();
    assert_eq!(
        controller.active_plan.as_ref().unwrap().decision.intent,
        PlacementIntent::Consolidate {
            target_node: "b".to_owned()
        }
    );
    assert_eq!(controller.residence.len(), 2);
    assert_eq!(current.placements.len(), 2);
}

#[test]
fn commit_rejects_stale_or_non_canonical_review_evidence() {
    let planner = PlacementPlanner;
    let current = planner
        .plan(request(vec![resource("a")], PlacementIntent::Automatic))
        .unwrap();
    let proposed = planner
        .plan(request(
            vec![resource("a"), resource("b")],
            PlacementIntent::Consolidate {
                target_node: "b".to_owned(),
            },
        ))
        .unwrap();
    let mut controller = controller(0);
    let review = controller
        .review(
            &proposed,
            &demands(),
            &[resource("a"), resource("b")],
            LogicalTag::new(1, 0),
            0,
        )
        .unwrap();

    let mut stale = review.clone();
    stale.source_plan_digest = StateDigest([9; 16]);
    assert_eq!(
        controller.commit(proposed.clone(), &stale, LogicalTag::new(1, 0)),
        Err(PlacementControllerError::IdentityMismatch)
    );

    let mut non_canonical = review.clone();
    non_canonical.moved_shards.push(ShardId::new(1).unwrap());
    assert_eq!(
        controller.commit(proposed.clone(), &non_canonical, LogicalTag::new(1, 0)),
        Err(PlacementControllerError::InvalidState)
    );

    controller
        .commit(proposed, &review, LogicalTag::new(1, 0))
        .unwrap();
    assert_eq!(
        controller.active_plan.as_ref().unwrap().digest,
        review.proposed_plan_digest
    );

    // Keep the source plan construction visible in this test: the controller
    // must have started from the exact plan that was reviewed.
    assert_eq!(current.digest, review.source_plan_digest);
}

#[test]
fn record_committed_restarts_residence_for_every_shard() {
    let (current, proposed) = plans();
    let mut controller = controller(0);
    controller
        .record_committed(proposed.clone(), LogicalTag::new(300, 0))
        .unwrap();

    assert_eq!(
        controller.active_plan.as_ref().unwrap().digest,
        proposed.digest
    );
    assert_eq!(controller.residence.len(), proposed.placements.len());
    assert!(
        controller
            .residence
            .values()
            .all(|residence| residence.last_committed_tag == LogicalTag::new(300, 0))
    );
    assert_ne!(current.digest, proposed.digest);
}
