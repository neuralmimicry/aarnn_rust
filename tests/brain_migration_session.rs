//! End-to-end reference migration using real stable-executor transfer sources.
//!
//! The test crosses the same boundaries used by an orchestrator: a durable
//! bridge emits sources, the session reassembles frames and materialises
//! destination actors, the group is cut over, the registry is published, and
//! the durable operation journal records the completed group.

use aarnn_rust::authoritative_shard::FIXED_POINT_SCALE;
use aarnn_rust::brain_migration_session::BrainMigrationSession;
use aarnn_rust::consistent_cut::{ChannelMarker, ConsistentCutCoordinator, ParticipantReport};
use aarnn_rust::deterministic::{
    BrainId, ComponentId, EventId, LeaseTerm, LogicalTag, PartitionGeneration, ShardId, StreamId,
    SynapseId, TopologyGeneration,
};
use aarnn_rust::management::ReplicatedQuorumLeaseAuthority;
use aarnn_rust::migration_executor::{
    MigrationExecutor, StableExecutorMigrationConfig, StableExecutorMigrationExecutor,
    StableExecutorMigrationSettings,
};
use aarnn_rust::migration_group::MigrationGroupSpec;
use aarnn_rust::migration_operation::{
    MigrationKind, MigrationOperation, MigrationProgress, MigrationRequest, MigrationTransition,
    PersistedMigrationJournal,
};
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlan, PlacementPlanner, PlacementRequest,
    ResourceObservation, ShardDemand,
};
use aarnn_rust::placement_registry::{PersistedPlacementRegistry, PlacementApplyRequest};
use aarnn_rust::shard_executor::StableShardExecutor;
use aarnn_rust::stable_executor_durable::StableExecutorDurableBridge;
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};

fn source_executor() -> StableShardExecutor {
    let topology = TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        (1..=4)
            .map(|id| NeuronRecord {
                id: aarnn_rust::deterministic::NeuronId::new(id).unwrap(),
            })
            .collect(),
        vec![SynapseRecord {
            id: SynapseId::new(11).unwrap(),
            source: aarnn_rust::deterministic::NeuronId::new(1).unwrap(),
            target: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
            delay_ticks: 0,
        }],
    )
    .unwrap();
    let source_shard = ShardId::new(10).unwrap();
    let destination_shard = ShardId::new(20).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: source_shard,
            weight_owner: source_shard,
            release_owner: source_shard,
            plasticity_owner: source_shard,
        })
        .collect::<Vec<_>>();
    let plan = compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        vec![
            VirtualShardAssignment {
                shard: source_shard,
                components: vec![ComponentId::new(1).unwrap(), ComponentId::new(2).unwrap()],
                load: 2,
            },
            VirtualShardAssignment {
                shard: destination_shard,
                components: vec![ComponentId::new(3).unwrap(), ComponentId::new(4).unwrap()],
                load: 2,
            },
        ],
        ownership,
    )
    .unwrap();
    StableShardExecutor::from_topology(
        BrainId::new(700).unwrap(),
        &topology,
        plan,
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        32,
        128,
    )
    .unwrap()
}

fn cut() -> aarnn_rust::consistent_cut::ConsistentCut {
    let mut coordinator =
        ConsistentCutCoordinator::begin(1, ["laptop".to_owned()], ["laptop->network".to_owned()])
            .unwrap();
    coordinator
        .record_report(ParticipantReport {
            participant: "laptop".to_owned(),
            local_frontier: LogicalTag::ZERO,
            queued_min: None,
            in_flight_min: None,
            activity_epoch: 1,
        })
        .unwrap();
    coordinator
        .record_marker(ChannelMarker::new("laptop->network", 1, None, b"empty-channel").unwrap())
        .unwrap();
    coordinator.finalise().unwrap()
}

fn resource(node: &str, domain: &str) -> ResourceObservation {
    ResourceObservation {
        node_id: node.to_owned(),
        device_id: format!("{node}-cpu"),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
        failure_domain: domain.to_owned(),
        numerical_profiles: vec!["reference-cpu-v1".to_owned()],
        capacity_units: 100,
        reserved_capacity_units: 0,
        memory_bytes: 1_000_000,
        reserved_memory_bytes: 0,
        storage_bytes: 1_000_000,
        reserved_storage_bytes: 0,
        network_bytes_per_second: 1_000_000,
        reserved_network_bytes_per_second: 0,
        cpu_pressure_milli: 100,
        memory_pressure_milli: 100,
        network_pressure_milli: 100,
        thermal_pressure_milli: 100,
    }
}

fn placement(term: LeaseTerm, target: &str) -> PlacementPlan {
    PlacementPlanner
        .plan(PlacementRequest {
            brain_id: BrainId::new(700).unwrap(),
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: term,
            fencing_token: term.raw(),
            effective_tag: LogicalTag::ZERO,
            demands: [10u64, 20]
                .into_iter()
                .map(|id| ShardDemand {
                    shard_id: ShardId::new(id).unwrap(),
                    load_units: 1,
                    memory_bytes: 100,
                    checkpoint_bytes: 100,
                    network_bytes_per_second: 1,
                    zero_delay_component: None,
                    required_numerical_profile: "reference-cpu-v1".to_owned(),
                    preferred_node: Some(target.to_owned()),
                })
                .collect(),
            resources: vec![resource("laptop", "home"), resource("network", "rack-a")],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: target.to_owned(),
            },
        })
        .unwrap()
}

#[test]
fn stable_bridge_sources_complete_brain_migration_and_journal_commit() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-brain-session-{}-{}",
        std::process::id(),
        EventId::new(1).unwrap().raw()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let source_owner = root.join("source-owner");
    let source_warm = root.join("source-warm");
    let target_owner = root.join("target-owner");
    let target_warm = root.join("target-warm");
    let source_store = StableExecutorCheckpointStore::new(root.join("source-checkpoints")).unwrap();
    let bridge = StableExecutorDurableBridge::new(
        source_executor(),
        source_store,
        LeaseTerm::INITIAL,
        LeaseTerm::INITIAL.raw(),
        EventId::new(2).unwrap(),
        &source_owner,
        &source_warm,
        StreamId::new(3).unwrap(),
        128,
        b"initial-channel".to_vec(),
    )
    .unwrap();
    let source_plan = placement(LeaseTerm::INITIAL, "laptop");
    let target_plan = placement(LeaseTerm::new(2).unwrap(), "network");
    let sources = bridge
        .prepare_transfer_sources(
            EventId::new(100).unwrap(),
            "laptop",
            &cut(),
            source_plan.digest(),
            32,
        )
        .unwrap();
    let total_bytes = sources
        .iter()
        .map(|source| source.manifest().total_bytes)
        .sum();
    let shard_ids = vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()];
    let spec = MigrationGroupSpec {
        brain_id: BrainId::new(700).unwrap(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: TopologyGeneration::INITIAL,
        partition_generation: PartitionGeneration::INITIAL,
        shard_ids: shard_ids.clone(),
    };
    let journal_path = root.join("migration-journal.json");
    let mut journal = PersistedMigrationJournal::open(
        &journal_path,
        BrainId::new(700).unwrap(),
        LeaseTerm::INITIAL,
    )
    .unwrap();
    journal
        .submit_with_group(
            MigrationRequest {
                request_id: "brain-session".to_owned(),
                idempotency_key: "brain-session".to_owned(),
                brain_id: BrainId::new(700).unwrap(),
                observed_leader_term: LeaseTerm::INITIAL,
                expected_resource_version: 0,
                kind: MigrationKind::MigrateBrain,
                source_plan_digest: source_plan.digest(),
                target_plan_digest: target_plan.digest(),
                total_shards: 2,
                total_bytes,
            },
            Some(spec.clone()),
        )
        .unwrap();
    let mut progress = MigrationProgress::new(2, total_bytes).unwrap();
    for phase in [
        aarnn_rust::migration_operation::MigrationPhase::Reserving,
        aarnn_rust::migration_operation::MigrationPhase::Transferring,
        aarnn_rust::migration_operation::MigrationPhase::CatchingUp,
        aarnn_rust::migration_operation::MigrationPhase::Draining,
    ] {
        journal
            .transition(MigrationTransition {
                operation_id: 1,
                observed_leader_term: LeaseTerm::INITIAL,
                expected_resource_version: journal.journal().resource_version,
                next_phase: phase,
                progress: progress.clone(),
                error_code: None,
            })
            .unwrap();
    }
    progress.completed_shards = 2;
    progress.transferred_bytes = total_bytes;
    progress.cut_tag = Some(LogicalTag::ZERO);
    journal
        .transition(MigrationTransition {
            operation_id: 1,
            observed_leader_term: LeaseTerm::INITIAL,
            expected_resource_version: journal.journal().resource_version,
            next_phase: aarnn_rust::migration_operation::MigrationPhase::CutoverReady,
            progress,
            error_code: None,
        })
        .unwrap();

    let mut group = spec.build(1).unwrap();
    let prepared = BrainMigrationSession::prepare_from_sources(
        &mut group,
        source_plan.digest(),
        sources,
        &target_owner,
        &target_warm,
        LeaseTerm::new(2).unwrap(),
        StreamId::new(3).unwrap(),
        128,
    )
    .unwrap();
    assert_eq!(prepared.destinations.len(), 2);
    assert!(
        prepared
            .destinations
            .values()
            .all(|actor| actor.term() == LeaseTerm::new(2).unwrap())
    );
    assert_eq!(
        group.phase,
        aarnn_rust::migration_group::MigrationGroupPhase::Transferring
    );

    let registry_path = root.join("placement-registry.json");
    let mut registry = PersistedPlacementRegistry::open(
        &registry_path,
        BrainId::new(700).unwrap(),
        LeaseTerm::INITIAL,
    )
    .unwrap();
    registry
        .apply(PlacementApplyRequest {
            request_id: "source".to_owned(),
            idempotency_key: "source".to_owned(),
            expected_resource_version: 0,
            observed_leader_term: LeaseTerm::INITIAL,
            plan: source_plan,
            cutover: None,
            repartition: None,
        })
        .unwrap();
    registry
        .set_leader_term(LeaseTerm::new(2).unwrap())
        .unwrap();
    let resource_version = journal.journal().resource_version;
    let outcome = BrainMigrationSession::publish_and_finalize_persisted(
        &mut group,
        prepared,
        &mut registry,
        PlacementApplyRequest {
            request_id: "target".to_owned(),
            idempotency_key: "target".to_owned(),
            expected_resource_version: 1,
            observed_leader_term: LeaseTerm::new(2).unwrap(),
            plan: target_plan,
            cutover: None,
            repartition: None,
        },
        Some((&mut journal, resource_version)),
    )
    .unwrap();
    assert_eq!(
        group.phase,
        aarnn_rust::migration_group::MigrationGroupPhase::Committed
    );
    assert_eq!(
        outcome.operation.unwrap().phase,
        aarnn_rust::migration_operation::MigrationPhase::Committed
    );
    assert_eq!(registry.state().authorities.len(), 2);
    assert!(
        registry
            .state()
            .authorities
            .values()
            .all(|authority| authority.node_id == "network")
    );
    drop(registry);
    drop(journal);
    let reopened_registry = PersistedPlacementRegistry::open(
        &registry_path,
        BrainId::new(700).unwrap(),
        LeaseTerm::new(2).unwrap(),
    )
    .unwrap();
    assert_eq!(reopened_registry.state().resource_version, 2);
    let reopened_journal = PersistedMigrationJournal::open(
        &journal_path,
        BrainId::new(700).unwrap(),
        LeaseTerm::INITIAL,
    )
    .unwrap();
    assert_eq!(
        reopened_journal.operation(1).unwrap().phase,
        aarnn_rust::migration_operation::MigrationPhase::Committed
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn registered_stable_executor_performs_bridge_backed_cutover() {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    let root =
        std::env::temp_dir().join(format!("aarnn-registered-executor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let source_owner = root.join("source-owner");
    let source_warm = root.join("source-warm");
    let destination_owner = root.join("destination-owner");
    let destination_warm = root.join("destination-warm");
    let bridge = StableExecutorDurableBridge::new(
        source_executor(),
        StableExecutorCheckpointStore::new(root.join("source-checkpoints")).unwrap(),
        LeaseTerm::INITIAL,
        LeaseTerm::INITIAL.raw(),
        EventId::new(300).unwrap(),
        &source_owner,
        &source_warm,
        StreamId::new(30).unwrap(),
        128,
        b"initial-channel".to_vec(),
    )
    .unwrap();
    let bridge = Arc::new(Mutex::new(bridge));
    let source_plan = placement(LeaseTerm::INITIAL, "laptop");
    let target_plan = placement(LeaseTerm::new(2).unwrap(), "network");
    let cut = cut();
    let total_bytes = bridge
        .lock()
        .unwrap()
        .prepare_transfer_sources(
            EventId::new(400).unwrap(),
            "laptop",
            &cut,
            source_plan.digest(),
            32,
        )
        .unwrap()
        .iter()
        .map(|source| source.manifest().total_bytes)
        .sum::<u64>();

    let authority = Arc::new(Mutex::new(
        ReplicatedQuorumLeaseAuthority::open(
            ["laptop", "network", "witness"]
                .into_iter()
                .map(|member| (member.to_owned(), root.join(format!("{member}.authority")))),
            ["laptop", "network", "witness"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap(),
    ));
    let leases = authority
        .lock()
        .unwrap()
        .issue_leases(
            [ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
            "laptop",
        )
        .unwrap();
    let placement_registry = Arc::new(Mutex::new(
        PersistedPlacementRegistry::open(
            root.join("placement.json"),
            BrainId::new(700).unwrap(),
            LeaseTerm::INITIAL,
        )
        .unwrap(),
    ));
    let executor = StableExecutorMigrationExecutor::new(StableExecutorMigrationConfig {
        bridge: Arc::clone(&bridge),
        settings: StableExecutorMigrationSettings {
            consistent_cut: cut,
            source_node: "laptop".to_owned(),
            destination_root: destination_owner,
            warm_root: destination_warm,
            authority,
            destination_nodes: [
                (ShardId::new(10).unwrap(), "network".to_owned()),
                (ShardId::new(20).unwrap(), "network".to_owned()),
            ]
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
            source_fencing_tokens: leases
                .into_iter()
                .map(|(shard, lease)| (shard, lease.fencing_token))
                .collect(),
            placement_registry,
            target_plan: target_plan.clone(),
            stream_id: StreamId::new(31).unwrap(),
            max_payload: 128,
            frame_bytes: 32,
            destination_endpoints: BTreeMap::new(),
            target_activation_commands: BTreeMap::new(),
            activation_gate: None,
        },
    })
    .unwrap();
    let operation = MigrationOperation {
        operation_id: 401,
        request_id: "registered-cutover".to_owned(),
        idempotency_key: "registered-cutover".to_owned(),
        brain_id: BrainId::new(700).unwrap(),
        kind: MigrationKind::MigrateBrain,
        source_plan_digest: source_plan.digest(),
        target_plan_digest: target_plan.digest(),
        phase: aarnn_rust::migration_operation::MigrationPhase::Prepared,
        progress: MigrationProgress {
            completed_shards: 0,
            total_shards: 2,
            transferred_bytes: 0,
            total_bytes,
            cut_tag: None,
        },
        resource_version: 1,
        error_code: None,
    };
    let group_spec = MigrationGroupSpec {
        brain_id: BrainId::new(700).unwrap(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: TopologyGeneration::INITIAL,
        partition_generation: PartitionGeneration::INITIAL,
        shard_ids: vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
    };
    let receipt = executor.execute(operation.clone(), group_spec).unwrap();
    assert_eq!(receipt.operation_id, operation.operation_id);
    assert_eq!(receipt.transferred_bytes, total_bytes);
    assert_eq!(
        receipt.group.phase,
        aarnn_rust::migration_group::MigrationGroupPhase::Committed
    );
    assert_eq!(
        bridge.lock().unwrap().authority().term(),
        LeaseTerm::new(2).unwrap()
    );
}
