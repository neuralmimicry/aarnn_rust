//! End-to-end reference migration evidence.
//!
//! This test keeps the data plane and control plane explicit: a source
//! checkpoint is framed and reconstructed, the migration journal records the
//! durable phases, and only then does the fenced placement registry publish
//! the destination owner.

use aarnn_rust::authoritative_shard::{
    AuthoritativeShard, BIOLOGICAL_STATE_SCHEMA_VERSION, FIXED_POINT_SCALE, StableTransitionInput,
};
use aarnn_rust::causal::CausalEvent;
use aarnn_rust::consistent_cut::{ChannelMarker, ConsistentCutCoordinator, ParticipantReport};
use aarnn_rust::data_plane::{CausalEnvelope, EnvelopeKind};
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LeaseTerm, LogicalTag,
    PartitionGeneration, RouteId, SchemaVersion, ShardId, StreamId, SynapseId, TopologyGeneration,
};
use aarnn_rust::management::ReplicatedQuorumLeaseAuthority;
use aarnn_rust::migration_coordinator::QuorumShardCutover;
use aarnn_rust::migration_operation::{
    MigrationJournal, MigrationKind, MigrationPhase, MigrationProgress, MigrationRequest,
    MigrationTransition,
};
use aarnn_rust::migration_transfer::ShardTransferSource;
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlan, PlacementPlanner, PlacementRequest,
    ResourceObservation, ShardDemand,
};
use aarnn_rust::placement_registry::{PlacementApplyRequest, PlacementRegistry};
use aarnn_rust::shard_executor::{RoutedCausalEvent, StableShardExecutor};
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};

fn brain() -> BrainId {
    BrainId::new(77).unwrap()
}

fn resource(node_id: &str, failure_domain: &str) -> ResourceObservation {
    ResourceObservation {
        node_id: node_id.to_owned(),
        device_id: format!("{node_id}-cpu"),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
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

fn plan(term: LeaseTerm, target_node: &str) -> PlacementPlan {
    PlacementPlanner
        .plan(PlacementRequest {
            brain_id: brain(),
            topology_generation: TopologyGeneration::INITIAL,
            partition_generation: PartitionGeneration::INITIAL,
            lease_term: term,
            fencing_token: term.raw(),
            effective_tag: LogicalTag::ZERO,
            demands: vec![ShardDemand {
                shard_id: ShardId::new(11).unwrap(),
                load_units: 10,
                memory_bytes: 100,
                checkpoint_bytes: 100,
                network_bytes_per_second: 10,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: None,
            }],
            resources: vec![resource("laptop", "home"), resource("worker", "rack-a")],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: target_node.to_owned(),
            },
        })
        .unwrap()
}

fn cut() -> aarnn_rust::consistent_cut::ConsistentCut {
    let mut coordinator =
        ConsistentCutCoordinator::begin(1, ["laptop".to_owned()], ["laptop->worker".to_owned()])
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
        .record_marker(ChannelMarker::new("laptop->worker", 1, None, b"channel").unwrap())
        .unwrap();
    coordinator.finalise().unwrap()
}

fn progress(
    completed_shards: u32,
    transferred_bytes: u64,
    cut_tag: Option<LogicalTag>,
    total_bytes: u64,
) -> MigrationProgress {
    MigrationProgress {
        completed_shards,
        total_shards: 1,
        transferred_bytes,
        total_bytes,
        cut_tag,
    }
}

fn stable_executor_fixture() -> StableShardExecutor {
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
                delay_ticks: 2,
            },
        ],
    )
    .unwrap();
    let first = ShardId::new(10).unwrap();
    let second = ShardId::new(20).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: first,
            weight_owner: first,
            release_owner: first,
            plasticity_owner: first,
        })
        .collect();
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
    StableShardExecutor::from_topology(
        brain(),
        &topology,
        plan,
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        32,
        128,
    )
    .unwrap()
}

fn stable_input_event(id: u64, target: u64) -> RoutedCausalEvent {
    let target = aarnn_rust::deterministic::NeuronId::new(target).unwrap();
    let event = EventId::new(id).unwrap();
    let payload = serde_json::to_vec(&StableTransitionInput {
        schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
        source: None,
        target,
        charge: 1,
        delay_ticks: 0,
    })
    .unwrap();
    RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event.raw(),
            ),
            id: event,
            payload,
            original_tag: LogicalTag::ZERO,
            deferred_from_nonconvergence: false,
        },
    }
}

#[test]
fn transfer_journal_and_registry_publish_one_new_owner() {
    let source_plan = plan(LeaseTerm::INITIAL, "laptop");
    let destination_plan = plan(LeaseTerm::new(2).unwrap(), "worker");
    let mut registry = PlacementRegistry::new(brain(), LeaseTerm::INITIAL);
    registry
        .apply(PlacementApplyRequest {
            request_id: "bootstrap".to_owned(),
            idempotency_key: "bootstrap".to_owned(),
            expected_resource_version: 0,
            observed_leader_term: LeaseTerm::INITIAL,
            plan: source_plan.clone(),
            cutover: None,
            repartition: None,
        })
        .unwrap();

    let source_root = std::env::temp_dir().join(format!(
        "aarnn-source-{}-{}",
        std::process::id(),
        EventId::new(100).unwrap().raw()
    ));
    let _ = std::fs::remove_dir_all(&source_root);
    std::fs::create_dir_all(&source_root).unwrap();
    let mut source_actor = AuthoritativeShard::open(
        source_root.join("owner.json"),
        Some(source_root.join("warm.json")),
        brain(),
        ShardId::new(11).unwrap(),
        TopologyGeneration::INITIAL,
        PartitionGeneration::INITIAL,
        LeaseTerm::INITIAL,
        StreamId::new(5).unwrap(),
        1024 * 1024,
        b"biology".to_vec(),
        b"channel".to_vec(),
    )
    .unwrap();
    let source_state = source_actor.state().unwrap();

    let source = ShardTransferSource::prepare(
        EventId::new(100).unwrap(),
        "laptop",
        &source_state,
        &cut(),
        source_plan.digest(),
        17,
    )
    .unwrap();
    let mut receiver =
        aarnn_rust::migration_transfer::ShardTransferReceiver::new(source.manifest().clone())
            .unwrap();
    for frame in source.frames().unwrap().into_iter().rev() {
        receiver.accept(frame).unwrap();
    }
    let imported = receiver.finalize().unwrap();

    source_actor
        .apply(
            &CausalEnvelope {
                schema_version: SchemaVersion::CURRENT,
                brain: brain(),
                stream: StreamId::new(5).unwrap(),
                sequence: 0,
                lease_term: LeaseTerm::INITIAL,
                route: RouteId::new(9).unwrap(),
                partition_generation: PartitionGeneration::INITIAL,
                source: None,
                target: None,
                tag: LogicalTag::new(1, 0),
                event: EventId::new(200).unwrap(),
                stage: EventStage::SynapticTransition,
                kind: EnvelopeKind::Event,
                payload: b":post".to_vec(),
                deferred_from_nonconvergence: false,
            },
            b"channel-after".to_vec(),
            |state, _| {
                let mut next = state.to_vec();
                next.extend_from_slice(b":post");
                Ok::<_, std::convert::Infallible>(next)
            },
        )
        .unwrap();
    let latest_source_state = source_actor.state().unwrap();
    let catch_up = imported.catch_up_from(&latest_source_state).unwrap();
    assert_eq!(catch_up.records.len(), 1);
    let mut tampered_catch_up = catch_up.clone();
    tampered_catch_up.records[0].payload[0] ^= 1;
    assert!(matches!(
        tampered_catch_up.verify(source.manifest()),
        Err(
            aarnn_rust::migration_transfer::MigrationTransferError::DigestMismatch {
                kind: "catch-up"
            }
        )
    ));
    let root = std::env::temp_dir().join(format!(
        "aarnn-transfer-{}-{}",
        std::process::id(),
        source.manifest().transfer_id.raw()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut promoted = imported
        .promote_into_authoritative(
            root.join("owner.json"),
            root.join("warm.json"),
            LeaseTerm::new(2).unwrap(),
            StreamId::new(5).unwrap(),
            1024 * 1024,
        )
        .unwrap();
    assert_eq!(promoted.term(), LeaseTerm::new(2).unwrap());
    let applied = catch_up
        .apply_to_authoritative_with_final_state(
            &source.manifest().clone(),
            &mut promoted,
            LeaseTerm::new(2).unwrap(),
            &latest_source_state,
        )
        .unwrap();
    assert_eq!(applied, 1);
    assert_eq!(promoted.state().unwrap().biological_state, b"biology:post");
    assert_eq!(promoted.state().unwrap().channel_state, b"channel-after");
    assert_eq!(
        latest_source_state.biological_state,
        promoted.state().unwrap().biological_state
    );
    assert_eq!(
        latest_source_state.committed_tag,
        promoted.state().unwrap().committed_tag
    );
    assert_eq!(
        latest_source_state.durable_wal_sequence,
        promoted.state().unwrap().durable_wal_sequence
    );
    assert_eq!(
        latest_source_state.applied_tag,
        promoted.state().unwrap().applied_tag
    );
    let evidence = imported_cutover_evidence_after_catch_up(
        &source,
        &latest_source_state,
        LeaseTerm::new(2).unwrap(),
    );
    assert_eq!(evidence.cut_tag, latest_source_state.committed_tag);
    assert_eq!(
        evidence
            .shards
            .get(&ShardId::new(11).unwrap())
            .unwrap()
            .checkpoint_digest,
        latest_source_state.state_digest
    );
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(source_root).unwrap();

    let mut journal = MigrationJournal::new(brain(), LeaseTerm::INITIAL);
    let operation = journal
        .submit(MigrationRequest {
            request_id: "move-100".to_owned(),
            idempotency_key: "move-100".to_owned(),
            brain_id: brain(),
            observed_leader_term: LeaseTerm::INITIAL,
            expected_resource_version: 0,
            kind: MigrationKind::Move,
            source_plan_digest: source_plan.digest(),
            target_plan_digest: destination_plan.digest(),
            total_shards: 1,
            total_bytes: source.manifest().total_bytes,
        })
        .unwrap();
    let total_bytes = source.manifest().total_bytes;
    let advance = |journal: &mut MigrationJournal,
                   expected_resource_version: u64,
                   phase: MigrationPhase,
                   progress: MigrationProgress| {
        journal
            .transition(MigrationTransition {
                operation_id: operation.operation_id,
                observed_leader_term: LeaseTerm::INITIAL,
                expected_resource_version,
                next_phase: phase,
                progress,
                error_code: None,
            })
            .unwrap()
    };
    let reserving = advance(
        &mut journal,
        1,
        MigrationPhase::Reserving,
        progress(0, 0, None, total_bytes),
    );
    let transferring = advance(
        &mut journal,
        reserving.resource_version,
        MigrationPhase::Transferring,
        progress(0, 0, None, total_bytes),
    );
    let catching_up = advance(
        &mut journal,
        transferring.resource_version,
        MigrationPhase::CatchingUp,
        progress(1, total_bytes, None, total_bytes),
    );
    let draining = advance(
        &mut journal,
        catching_up.resource_version,
        MigrationPhase::Draining,
        progress(1, total_bytes, None, total_bytes),
    );
    let ready = advance(
        &mut journal,
        draining.resource_version,
        MigrationPhase::CutoverReady,
        progress(1, total_bytes, Some(LogicalTag::ZERO), total_bytes),
    );

    registry
        .set_leader_term(LeaseTerm::new(2).unwrap())
        .unwrap();
    let evidence = imported_cutover_evidence(&source, LeaseTerm::new(2).unwrap());
    registry
        .apply(PlacementApplyRequest {
            request_id: "move-100-apply".to_owned(),
            idempotency_key: "move-100-apply".to_owned(),
            expected_resource_version: 1,
            observed_leader_term: LeaseTerm::new(2).unwrap(),
            plan: destination_plan,
            cutover: Some(evidence),
            repartition: None,
        })
        .unwrap();
    assert_eq!(
        registry
            .authority(ShardId::new(11).unwrap())
            .unwrap()
            .node_id,
        "worker"
    );
    assert_eq!(
        registry
            .authority(ShardId::new(11).unwrap())
            .unwrap()
            .lease_term,
        LeaseTerm::new(2).unwrap()
    );

    let committed = advance(
        &mut journal,
        ready.resource_version,
        MigrationPhase::Committed,
        ready.progress,
    );
    assert_eq!(committed.phase, MigrationPhase::Committed);
    assert_eq!(journal.audit().len(), 7);
}

#[test]
fn stable_multi_shard_cut_transfers_out_of_order_and_restores_under_new_term() {
    let mut executor = stable_executor_fixture();
    executor.admit(stable_input_event(801, 1)).unwrap();
    executor.step().unwrap().unwrap();
    let before = executor.state_digest().unwrap();
    let source_states = executor.shard_states(LeaseTerm::INITIAL, Some(12)).unwrap();
    assert_eq!(source_states.len(), 2);

    let mut destination_states = Vec::new();
    for (index, state) in source_states.into_iter().enumerate() {
        let source = ShardTransferSource::prepare(
            EventId::new(810 + index as u64).unwrap(),
            "laptop",
            &state,
            &cut(),
            executor.plan().digest(),
            19,
        )
        .unwrap();
        let mut receiver =
            aarnn_rust::migration_transfer::ShardTransferReceiver::new(source.manifest().clone())
                .unwrap();
        for frame in source.frames().unwrap().into_iter().rev() {
            receiver.accept(frame).unwrap();
        }
        destination_states.push(
            receiver
                .finalize()
                .unwrap()
                .promote(LeaseTerm::new(2).unwrap())
                .unwrap(),
        );
    }

    let restored = StableShardExecutor::restore_from_shard_states(
        brain(),
        executor.plan().clone(),
        destination_states,
    )
    .unwrap();
    assert_eq!(restored.state_digest().unwrap(), before);
    assert_eq!(restored.total_pending(), executor.total_pending());
}

#[test]
fn quorum_promotion_fences_source_before_destination_accepts_events() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-quorum-transfer-{}-{}",
        std::process::id(),
        EventId::new(501).unwrap().raw()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let members = ["laptop", "worker", "witness"];
    let replicas = members
        .iter()
        .map(|member| {
            (
                (*member).to_owned(),
                root.join(format!("authority-{member}.json")),
            )
        })
        .collect::<Vec<_>>();
    let mut authority = ReplicatedQuorumLeaseAuthority::open(
        replicas.clone(),
        members.iter().map(|member| (*member).to_owned()),
    )
    .unwrap();
    let shard_id = ShardId::new(11).unwrap();
    let source_lease = authority.issue_lease(shard_id, "laptop").unwrap();
    let brain = brain();
    let stream = StreamId::new(502).unwrap();
    let source_root = root.join("source");
    let mut source_actor = AuthoritativeShard::open(
        source_root.join("owner.json"),
        Some(source_root.join("warm.json")),
        brain,
        shard_id,
        TopologyGeneration::INITIAL,
        PartitionGeneration::INITIAL,
        source_lease.term,
        stream,
        1024 * 1024,
        b"biology".to_vec(),
        b"channel".to_vec(),
    )
    .unwrap();
    source_actor.bind_replicated_fencing(
        replicas.clone(),
        members.iter().map(|member| (*member).to_owned()).collect(),
        "laptop",
        source_lease.fencing_token,
    );
    source_actor
        .apply(
            &CausalEnvelope {
                schema_version: SchemaVersion::CURRENT,
                brain,
                stream,
                sequence: 0,
                lease_term: source_lease.term,
                route: RouteId::new(9).unwrap(),
                partition_generation: PartitionGeneration::INITIAL,
                source: None,
                target: None,
                tag: LogicalTag::new(1, 0),
                event: EventId::new(503).unwrap(),
                stage: EventStage::SynapticTransition,
                kind: EnvelopeKind::Event,
                payload: b":source".to_vec(),
                deferred_from_nonconvergence: false,
            },
            b"channel-source".to_vec(),
            |state, event| {
                let mut next = state.to_vec();
                next.extend_from_slice(event.payload.as_slice());
                Ok::<_, std::convert::Infallible>(next)
            },
        )
        .unwrap();
    let source_state = source_actor.state().unwrap();
    let source_plan = plan(source_lease.term, "laptop");
    // The source lease is the authority's initial term, so the next fenced
    // destination lease is term 2. The placement plan must carry that exact
    // term/token pair before the authority is allowed to fence the source.
    let destination_term = LeaseTerm::new(2).unwrap();
    let destination_plan = plan(destination_term, "worker");
    let mut registry = PlacementRegistry::new(brain, source_lease.term);
    registry
        .apply(PlacementApplyRequest {
            request_id: "quorum-bootstrap".to_owned(),
            idempotency_key: "quorum-bootstrap".to_owned(),
            expected_resource_version: 0,
            observed_leader_term: source_lease.term,
            plan: source_plan.clone(),
            cutover: None,
            repartition: None,
        })
        .unwrap();
    registry.set_leader_term(destination_term).unwrap();
    let source_transfer = ShardTransferSource::prepare(
        EventId::new(504).unwrap(),
        "laptop",
        &source_state,
        &cut(),
        source_plan.digest(),
        256,
    )
    .unwrap();
    let mut receiver = aarnn_rust::migration_transfer::ShardTransferReceiver::new(
        source_transfer.manifest().clone(),
    )
    .unwrap();
    for frame in source_transfer.frames().unwrap() {
        receiver.accept(frame).unwrap();
    }
    let imported = receiver.finalize().unwrap();
    let destination_root = root.join("destination");
    let outcome = QuorumShardCutover::promote_and_publish(
        imported,
        destination_root.join("owner.json"),
        destination_root.join("warm.json"),
        &mut authority,
        &mut registry,
        PlacementApplyRequest {
            request_id: "quorum-cutover".to_owned(),
            idempotency_key: "quorum-cutover".to_owned(),
            expected_resource_version: 1,
            observed_leader_term: destination_term,
            plan: destination_plan,
            cutover: None,
            repartition: None,
        },
        "worker",
        507,
        source_lease.fencing_token,
        stream,
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(outcome.receipt.resource_version, 2);
    let mut promoted = outcome.promoted;
    assert!(promoted.lease.term > source_lease.term);
    assert_eq!(promoted.cutover.operation_id.raw(), 507);
    promoted.cutover.verify().unwrap();

    let stale_result = source_actor.apply(
        &CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain,
            stream,
            sequence: 1,
            lease_term: source_lease.term,
            route: RouteId::new(9).unwrap(),
            partition_generation: PartitionGeneration::INITIAL,
            source: None,
            target: None,
            tag: LogicalTag::new(2, 0),
            event: EventId::new(505).unwrap(),
            stage: EventStage::SynapticTransition,
            kind: EnvelopeKind::Event,
            payload: b":stale".to_vec(),
            deferred_from_nonconvergence: false,
        },
        b"stale".to_vec(),
        |_, _| -> Result<Vec<u8>, &'static str> {
            panic!("source transition must be fenced before it executes")
        },
    );
    assert!(stale_result.is_err());

    promoted
        .shard
        .apply(
            &CausalEnvelope {
                schema_version: SchemaVersion::CURRENT,
                brain,
                stream,
                sequence: 1,
                lease_term: promoted.lease.term,
                route: RouteId::new(9).unwrap(),
                partition_generation: PartitionGeneration::INITIAL,
                source: None,
                target: None,
                tag: LogicalTag::new(2, 0),
                event: EventId::new(506).unwrap(),
                stage: EventStage::SynapticTransition,
                kind: EnvelopeKind::Event,
                payload: b":destination".to_vec(),
                deferred_from_nonconvergence: false,
            },
            b"channel-destination".to_vec(),
            |state, event| {
                let mut next = state.to_vec();
                next.extend_from_slice(event.payload.as_slice());
                Ok::<_, std::convert::Infallible>(next)
            },
        )
        .unwrap();
    assert_eq!(
        promoted.shard.biological_state(),
        b"biology:source:destination"
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn imported_cutover_evidence(
    source: &ShardTransferSource,
    destination_term: LeaseTerm,
) -> aarnn_rust::placement_registry::CutoverEvidence {
    let mut receiver =
        aarnn_rust::migration_transfer::ShardTransferReceiver::new(source.manifest().clone())
            .unwrap();
    for frame in source.frames().unwrap() {
        receiver.accept(frame).unwrap();
    }
    receiver
        .finalize()
        .unwrap()
        .cutover_evidence(100, destination_term)
        .unwrap()
}

fn imported_cutover_evidence_after_catch_up(
    source: &ShardTransferSource,
    final_state: &aarnn_rust::authoritative_shard::ShardState,
    destination_term: LeaseTerm,
) -> aarnn_rust::placement_registry::CutoverEvidence {
    source
        .imported_state()
        .unwrap()
        .cutover_evidence_after_catch_up(final_state, 100, destination_term)
        .unwrap()
}
