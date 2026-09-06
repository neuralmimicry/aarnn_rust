//! Authoritative placement apply, fencing and restart verification.

use aarnn_rust::deterministic::{BrainId, EventId, LeaseTerm, LogicalTag, ShardId, StateDigest};
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlan, PlacementPlanner, PlacementRequest,
    RepartitionPlan, ResourceObservation, ShardDemand, ShardLineage,
};
use aarnn_rust::placement_registry::{
    CutoverEvidence, PersistedPlacementRegistry, PlacementActivationState, PlacementApplyRequest,
    PlacementRegistry, PlacementRegistryError, ShardCutoverEvidence,
};
use std::collections::BTreeMap;

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

fn request(term: LeaseTerm, intent: PlacementIntent, shards: &[u64]) -> PlacementRequest {
    PlacementRequest {
        brain_id: BrainId::new(77).unwrap(),
        topology_generation: aarnn_rust::deterministic::TopologyGeneration::INITIAL,
        partition_generation: aarnn_rust::deterministic::PartitionGeneration::INITIAL,
        lease_term: term,
        fencing_token: term.raw(),
        effective_tag: LogicalTag::ZERO,
        demands: shards
            .iter()
            .map(|shard| ShardDemand {
                shard_id: ShardId::new(*shard).unwrap(),
                load_units: 10,
                memory_bytes: 100,
                checkpoint_bytes: 100,
                network_bytes_per_second: 10,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: None,
            })
            .collect(),
        resources: vec![resource("laptop", "home"), resource("worker", "rack-a")],
        constraints: PlacementConstraints {
            minimum_warm_replicas: 0,
            ..PlacementConstraints::default()
        },
        intent,
    }
}

fn plan(term: LeaseTerm, intent: PlacementIntent, shards: &[u64]) -> PlacementPlan {
    PlacementPlanner
        .plan(request(term, intent, shards))
        .unwrap()
}

fn apply_request(
    plan: PlacementPlan,
    version: u64,
    key: &str,
    cutover: Option<CutoverEvidence>,
    repartition: Option<RepartitionPlan>,
) -> PlacementApplyRequest {
    PlacementApplyRequest {
        request_id: format!("request-{key}"),
        idempotency_key: key.to_owned(),
        expected_resource_version: version,
        observed_leader_term: plan.lease_term,
        plan,
        cutover,
        repartition,
    }
}

#[test]
fn bootstrap_is_atomic_and_retries_are_idempotent() {
    let first_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1],
    );
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    let first = registry
        .apply(apply_request(
            first_plan.clone(),
            0,
            "bootstrap",
            None,
            None,
        ))
        .unwrap();
    assert_eq!(first.resource_version, 1);
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "laptop"
    );

    let retry = registry
        .apply(apply_request(first_plan, 999, "bootstrap", None, None))
        .unwrap();
    assert_eq!(retry, first);
    assert_eq!(registry.resource_version, 1);
}

#[test]
fn prepared_activation_keeps_old_authority_until_every_target_is_active() {
    let first_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1, 2],
    );
    let target_plan = plan(
        LeaseTerm::new(2).unwrap(),
        PlacementIntent::Consolidate {
            target_node: "worker".to_owned(),
        },
        &[1, 2],
    );
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    registry
        .apply(apply_request(
            first_plan.clone(),
            0,
            "bootstrap",
            None,
            None,
        ))
        .unwrap();
    registry
        .set_leader_term(LeaseTerm::new(2).unwrap())
        .unwrap();

    let cutover = CutoverEvidence {
        operation_id: EventId::new(701).unwrap(),
        source_plan_digest: first_plan.digest(),
        cut_tag: LogicalTag::new(4, 0),
        destination_term: LeaseTerm::new(2).unwrap(),
        shards: [1_u64, 2]
            .into_iter()
            .map(|shard| {
                (
                    ShardId::new(shard).unwrap(),
                    ShardCutoverEvidence {
                        source_node: "laptop".to_owned(),
                        source_term: LeaseTerm::INITIAL,
                        checkpoint_digest: StateDigest([shard as u8; 16]),
                        caught_up: true,
                        route_cursor_digest: StateDigest([3; 16]),
                        effect_cursor_digest: StateDigest([4; 16]),
                    },
                )
            })
            .collect(),
    };
    let receipt = registry
        .prepare(apply_request(
            target_plan.clone(),
            1,
            "move",
            Some(cutover),
            None,
        ))
        .unwrap();
    assert!(!receipt.committed);
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "laptop"
    );
    assert_eq!(
        registry.prepared_plan().unwrap().digest(),
        target_plan.digest()
    );

    for activation in ["move:worker-a", "move:worker-b"] {
        registry
            .record_activation_dispatch_status(
                "move",
                activation,
                "request-move",
                receipt.plan_digest,
                PlacementActivationState::Pending,
                "",
                "{}",
            )
            .unwrap();
    }
    assert!(matches!(
        registry.commit_prepared(),
        Err(PlacementRegistryError::ActivationIncomplete)
    ));
    registry
        .record_activation_outcome("move:worker-a", PlacementActivationState::Active, "")
        .unwrap();
    assert!(matches!(
        registry.commit_prepared(),
        Err(PlacementRegistryError::ActivationIncomplete)
    ));
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "laptop"
    );
    registry
        .record_activation_outcome("move:worker-b", PlacementActivationState::Active, "")
        .unwrap();
    let committed = registry.commit_prepared().unwrap();
    assert!(committed.committed);
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "worker"
    );
    assert!(registry.prepared_plan().is_none());
}

#[test]
fn failed_prepared_activation_aborts_without_changing_authority_and_allows_new_key() {
    let first_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1],
    );
    let target_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "worker".to_owned(),
        },
        &[1],
    );
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    registry
        .apply(apply_request(
            first_plan.clone(),
            0,
            "bootstrap",
            None,
            None,
        ))
        .unwrap();
    let evidence = CutoverEvidence {
        operation_id: EventId::new(702).unwrap(),
        source_plan_digest: first_plan.digest(),
        cut_tag: LogicalTag::new(5, 0),
        destination_term: LeaseTerm::INITIAL,
        shards: [(
            ShardId::new(1).unwrap(),
            ShardCutoverEvidence {
                source_node: "laptop".to_owned(),
                source_term: LeaseTerm::INITIAL,
                checkpoint_digest: StateDigest([5; 16]),
                caught_up: true,
                route_cursor_digest: StateDigest([6; 16]),
                effect_cursor_digest: StateDigest([7; 16]),
            },
        )]
        .into_iter()
        .collect(),
    };
    let receipt = registry
        .prepare(apply_request(
            target_plan.clone(),
            1,
            "failed-move",
            Some(evidence.clone()),
            None,
        ))
        .unwrap();
    registry
        .record_activation_dispatch_status(
            "failed-move",
            "failed-move:worker",
            "request-failed-move",
            receipt.plan_digest,
            PlacementActivationState::Failed,
            "worker unavailable",
            "{}",
        )
        .unwrap();
    assert!(matches!(
        registry.commit_prepared(),
        Err(PlacementRegistryError::ActivationFailed(_))
    ));
    registry.abort_prepared().unwrap();
    assert!(
        registry
            .active_plan
            .as_ref()
            .is_some_and(|plan| plan.digest() == first_plan.digest())
    );
    assert!(registry.prepared_plan().is_none());
    assert_eq!(
        registry.activation_statuses["failed-move:worker"].state,
        PlacementActivationState::Failed
    );
    let retry = registry
        .prepare(apply_request(
            target_plan,
            1,
            "retry-move",
            Some(evidence),
            None,
        ))
        .unwrap();
    assert!(!retry.committed);
}

#[test]
fn activation_status_is_bound_to_the_applied_plan_and_survives_retry() {
    let first_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1],
    );
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    let receipt = registry
        .apply(apply_request(first_plan, 0, "activation-key", None, None))
        .unwrap();

    registry
        .record_activation_status_with_command(
            &receipt.idempotency_key,
            &receipt.request_id,
            receipt.plan_digest,
            PlacementActivationState::Pending,
            "",
            r#"{"schema_version":1,"request_id":"activation"}"#,
        )
        .unwrap();
    assert_eq!(
        registry
            .activation_statuses
            .get("activation-key")
            .unwrap()
            .state,
        PlacementActivationState::Pending
    );
    assert_eq!(registry.retryable_activation_commands().len(), 1);

    registry
        .record_activation_status(
            &receipt.idempotency_key,
            &receipt.request_id,
            receipt.plan_digest,
            PlacementActivationState::Failed,
            "worker session unavailable",
        )
        .unwrap();
    assert_eq!(
        registry
            .activation_statuses
            .get("activation-key")
            .unwrap()
            .error,
        "worker session unavailable"
    );

    // A retry uses a new activation idempotency key while remaining bound to
    // the same immutable placement publication. A terminal failure cannot be
    // resurrected under its original key.
    registry
        .record_activation_dispatch_status(
            &receipt.idempotency_key,
            "activation-retry",
            &receipt.request_id,
            receipt.plan_digest,
            PlacementActivationState::Queued,
            "",
            r#"{"schema_version":1,"request_id":"activation-retry"}"#,
        )
        .unwrap();
    assert_eq!(registry.resource_version, 1);
    assert_eq!(
        registry
            .activation_statuses
            .get("activation-retry")
            .unwrap()
            .state,
        PlacementActivationState::Queued
    );

    // A later worker result can use only the immutable idempotency key; the
    // registry derives the original request and plan digest itself.
    registry
        .record_activation_outcome(
            "activation-retry",
            PlacementActivationState::Failed,
            "worker rejected the manifest",
        )
        .unwrap();
    assert_eq!(
        registry
            .activation_statuses
            .get("activation-retry")
            .unwrap()
            .error,
        "worker rejected the manifest"
    );
    assert!(matches!(
        registry.record_activation_outcome(
            "activation-retry",
            PlacementActivationState::Queued,
            ""
        ),
        Err(PlacementRegistryError::InvalidPersisted(_))
    ));
    assert!(registry.retryable_activation_commands().is_empty());

    let wrong_digest = registry.record_activation_status(
        &receipt.idempotency_key,
        &receipt.request_id,
        StateDigest([8; 16]),
        PlacementActivationState::Queued,
        "",
    );
    assert!(matches!(
        wrong_digest,
        Err(PlacementRegistryError::IdempotencyConflict { .. })
    ));
}

#[test]
fn active_activation_is_terminal_and_idempotent() {
    let first_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1],
    );
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    let receipt = registry
        .apply(apply_request(first_plan, 0, "active-key", None, None))
        .unwrap();
    registry
        .record_activation_status_with_command(
            &receipt.idempotency_key,
            &receipt.request_id,
            receipt.plan_digest,
            PlacementActivationState::Queued,
            "",
            "{}",
        )
        .unwrap();
    registry
        .record_activation_outcome("active-key", PlacementActivationState::Active, "")
        .unwrap();
    registry
        .record_activation_outcome("active-key", PlacementActivationState::Active, "")
        .unwrap();
    assert!(matches!(
        registry.record_activation_outcome("active-key", PlacementActivationState::Queued, ""),
        Err(PlacementRegistryError::InvalidPersisted(_))
    ));
    assert!(registry.retryable_activation_commands().is_empty());
}

#[test]
fn active_owner_change_requires_verified_cutover_and_new_term() {
    let first_plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1],
    );
    let target_plan = plan(
        LeaseTerm::new(2).unwrap(),
        PlacementIntent::Consolidate {
            target_node: "worker".to_owned(),
        },
        &[1],
    );
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    registry
        .apply(apply_request(
            first_plan.clone(),
            0,
            "bootstrap",
            None,
            None,
        ))
        .unwrap();

    registry
        .set_leader_term(LeaseTerm::new(2).unwrap())
        .unwrap();
    let missing = registry.apply(apply_request(
        target_plan.clone(),
        1,
        "move-no-proof",
        None,
        None,
    ));
    assert!(matches!(
        missing,
        Err(PlacementRegistryError::CutoverRequired)
    ));
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "laptop"
    );

    let mut shards = BTreeMap::new();
    shards.insert(
        ShardId::new(1).unwrap(),
        ShardCutoverEvidence {
            source_node: "laptop".to_owned(),
            source_term: LeaseTerm::INITIAL,
            checkpoint_digest: StateDigest([9; 16]),
            caught_up: true,
            route_cursor_digest: StateDigest([10; 16]),
            effect_cursor_digest: StateDigest([11; 16]),
        },
    );
    let evidence = CutoverEvidence {
        operation_id: EventId::new(1).unwrap(),
        source_plan_digest: first_plan.digest(),
        cut_tag: LogicalTag::new(8, 0),
        destination_term: LeaseTerm::new(2).unwrap(),
        shards,
    };
    let receipt = registry
        .apply(apply_request(
            target_plan,
            1,
            "move-with-proof",
            Some(evidence),
            None,
        ))
        .unwrap();
    assert_eq!(receipt.resource_version, 2);
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "worker"
    );
    assert_eq!(
        registry
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .lease_term,
        LeaseTerm::new(2).unwrap()
    );

    let stale = registry.apply(apply_request(
        plan(
            LeaseTerm::INITIAL,
            PlacementIntent::Consolidate {
                target_node: "laptop".to_owned(),
            },
            &[1],
        ),
        2,
        "stale-term",
        None,
        None,
    ));
    assert!(matches!(
        stale,
        Err(PlacementRegistryError::StaleLeader { .. })
    ));
}

#[test]
fn shard_count_reduction_requires_lineage_and_is_published_atomically() {
    let first_plan = plan(LeaseTerm::INITIAL, PlacementIntent::Automatic, &[1, 2]);
    let target_same_generation = plan(
        LeaseTerm::new(2).unwrap(),
        PlacementIntent::Consolidate {
            target_node: "worker".to_owned(),
        },
        &[3],
    );
    let repartition = RepartitionPlan::new(
        EventId::new(3).unwrap(),
        BrainId::new(77).unwrap(),
        first_plan.topology_generation,
        first_plan.partition_generation,
        aarnn_rust::deterministic::PartitionGeneration::new(2).unwrap(),
        LogicalTag::ZERO,
        vec![ShardId::new(1).unwrap(), ShardId::new(2).unwrap()],
        vec![ShardId::new(3).unwrap()],
        vec![
            ShardLineage {
                source_shard: ShardId::new(1).unwrap(),
                successor_shards: vec![ShardId::new(3).unwrap()],
            },
            ShardLineage {
                source_shard: ShardId::new(2).unwrap(),
                successor_shards: vec![ShardId::new(3).unwrap()],
            },
        ],
    )
    .unwrap();
    let mut registry = PlacementRegistry::new(BrainId::new(77).unwrap(), LeaseTerm::INITIAL);
    registry
        .apply(apply_request(
            first_plan.clone(),
            0,
            "bootstrap",
            None,
            None,
        ))
        .unwrap();
    registry
        .set_leader_term(LeaseTerm::new(2).unwrap())
        .unwrap();

    let mut evidence_shards = BTreeMap::new();
    for (id, node) in [(1, "laptop"), (2, "worker")] {
        evidence_shards.insert(
            ShardId::new(id).unwrap(),
            ShardCutoverEvidence {
                source_node: node.to_owned(),
                source_term: LeaseTerm::INITIAL,
                checkpoint_digest: StateDigest([id as u8; 16]),
                caught_up: true,
                route_cursor_digest: StateDigest([10; 16]),
                effect_cursor_digest: StateDigest([11; 16]),
            },
        );
    }
    let evidence = CutoverEvidence {
        operation_id: EventId::new(4).unwrap(),
        source_plan_digest: first_plan.digest(),
        cut_tag: LogicalTag::ZERO,
        destination_term: LeaseTerm::new(2).unwrap(),
        shards: evidence_shards,
    };
    // The target plan deliberately uses the same partition generation first;
    // the registry must reject it before inspecting or mutating authorities.
    let invalid = registry.apply(apply_request(
        target_same_generation,
        1,
        "missing-generation",
        Some(evidence.clone()),
        Some(repartition.clone()),
    ));
    assert!(matches!(
        invalid,
        Err(PlacementRegistryError::MissingRepartition)
    ));
    assert_eq!(registry.authorities.len(), 2);

    let mut target_request = request(
        LeaseTerm::new(2).unwrap(),
        PlacementIntent::Consolidate {
            target_node: "worker".to_owned(),
        },
        &[3],
    );
    target_request.partition_generation = repartition.target_partition_generation;
    let target_plan = PlacementPlanner.plan(target_request).unwrap();
    let applied = registry.apply(apply_request(
        target_plan,
        1,
        "valid-repartition",
        Some(evidence),
        Some(repartition),
    ));
    assert!(applied.is_ok());
    assert_eq!(registry.authorities.len(), 1);
    assert!(registry.authority(ShardId::new(1).unwrap()).is_none());
}

#[test]
fn persisted_registry_reopens_without_losing_the_fence() {
    let path = std::env::temp_dir().join(format!(
        "aarnn-placement-registry-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("placement.lock"));
    let plan = plan(
        LeaseTerm::INITIAL,
        PlacementIntent::Consolidate {
            target_node: "laptop".to_owned(),
        },
        &[1],
    );
    let mut persisted =
        PersistedPlacementRegistry::open(&path, BrainId::new(77).unwrap(), LeaseTerm::INITIAL)
            .unwrap();
    let receipt = persisted
        .apply(apply_request(plan, 0, "persisted", None, None))
        .unwrap();
    persisted
        .record_activation_status_with_command(
            &receipt.idempotency_key,
            &receipt.request_id,
            receipt.plan_digest,
            PlacementActivationState::Pending,
            "",
            r#"{"schema_version":1,"request_id":"persisted-activation"}"#,
        )
        .unwrap();
    drop(persisted);

    let mut reopened =
        PersistedPlacementRegistry::open(&path, BrainId::new(77).unwrap(), LeaseTerm::INITIAL)
            .unwrap();
    assert_eq!(reopened.state().resource_version, 1);
    assert_eq!(
        reopened
            .state()
            .retryable_activation_commands()
            .first()
            .map(|(_, status)| status.activation_command_json.as_str()),
        Some(r#"{"schema_version":1,"request_id":"persisted-activation"}"#)
    );
    reopened
        .record_activation_outcome("persisted", PlacementActivationState::Active, "")
        .unwrap();
    drop(reopened);

    let reopened_active =
        PersistedPlacementRegistry::open(&path, BrainId::new(77).unwrap(), LeaseTerm::INITIAL)
            .unwrap();
    assert_eq!(
        reopened_active
            .state()
            .activation_statuses
            .get("persisted")
            .map(|status| &status.state),
        Some(&PlacementActivationState::Active)
    );
    assert!(
        reopened_active
            .state()
            .retryable_activation_commands()
            .is_empty()
    );
    assert_eq!(
        reopened_active
            .state()
            .authority(ShardId::new(1).unwrap())
            .unwrap()
            .node_id,
        "laptop"
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("placement.lock"));
}
