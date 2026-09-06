use aarnn_rust::authoritative_shard::{
    AuthoritativeShard, BIOLOGICAL_STATE_SCHEMA_VERSION, FIXED_POINT_SCALE, StableTransitionInput,
};
use aarnn_rust::causal::CausalEvent;
use aarnn_rust::consistent_cut::{ChannelMarker, ConsistentCutCoordinator, ParticipantReport};
use aarnn_rust::data_plane::{CausalEnvelope, EnvelopeKind};
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LeaseTerm, LogicalTag,
    PartitionGeneration, RouteId, SchemaVersion, ShardId, StateDigest, StreamId, SynapseId,
    TopologyGeneration,
};
use aarnn_rust::shard_executor::{RoutedCausalEvent, StableShardExecutor};
use aarnn_rust::stable_executor_authority::StableExecutorAuthority;
use aarnn_rust::stable_executor_durable::StableExecutorDurableBridge;
use aarnn_rust::stable_executor_store::{
    StableExecutorCheckpointSet, StableExecutorCheckpointStore,
};
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};

fn fixture() -> StableShardExecutor {
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
    let source_owner = ShardId::new(10).unwrap();
    let destination_owner = ShardId::new(20).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: source_owner,
            weight_owner: source_owner,
            release_owner: source_owner,
            plasticity_owner: source_owner,
        })
        .collect::<Vec<_>>();
    let plan = compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        vec![
            VirtualShardAssignment {
                shard: source_owner,
                components: vec![ComponentId::new(1).unwrap(), ComponentId::new(2).unwrap()],
                load: 2,
            },
            VirtualShardAssignment {
                shard: destination_owner,
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

fn initial_cut() -> aarnn_rust::consistent_cut::ConsistentCut {
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

#[test]
fn public_fabric_routes_stable_id_output_across_shard_boundary() {
    let mut executor = fixture();
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let event = EventId::new(1).unwrap();
    let payload = serde_json::to_vec(&StableTransitionInput {
        schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
        source: None,
        target,
        charge: 1,
        delay_ticks: 0,
    })
    .unwrap();
    executor
        .admit(RoutedCausalEvent {
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
        })
        .unwrap();

    let first = executor.step().unwrap().unwrap();
    assert_eq!(first.emitted.len(), 1);
    let routed = &first.emitted[0];
    assert_eq!(routed.route, Some(RouteId::new(11).unwrap()));
    assert_eq!(routed.event.key.target, 2);

    let second = executor.step().unwrap().unwrap();
    assert_eq!(second.consumed.event.key.target, 2);
    assert_eq!(second.emitted.len(), 1);
    assert_eq!(second.emitted[0].route, Some(RouteId::new(12).unwrap()));
    assert_eq!(second.emitted[0].event.key.target, 3);
    assert_eq!(second.emitted[0].event.key.tag, LogicalTag::new(2, 0));
    assert_eq!(executor.total_pending(), 1);
    assert_ne!(
        executor.state_digest().unwrap(),
        aarnn_rust::deterministic::StateDigest([0; 16])
    );
}

#[test]
fn immutable_checkpoint_store_reopens_and_restores_the_whole_fabric() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-executor-store-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mut executor = fixture();
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let event = EventId::new(900).unwrap();
    let payload = serde_json::to_vec(&StableTransitionInput {
        schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
        source: None,
        target,
        charge: 1,
        delay_ticks: 0,
    })
    .unwrap();
    executor
        .admit(RoutedCausalEvent {
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
        })
        .unwrap();
    executor.step().unwrap().unwrap();
    let digest = executor.state_digest().unwrap();
    let plan = executor.plan().clone();
    let checkpoints = executor.checkpoint_shards().unwrap();
    let canonical =
        StableExecutorCheckpointSet::new(LeaseTerm::INITIAL, checkpoints.clone()).unwrap();
    let mut reversed = checkpoints;
    reversed.reverse();
    let reordered = StableExecutorCheckpointSet::new(LeaseTerm::INITIAL, reversed).unwrap();
    assert_eq!(canonical.set_digest, reordered.set_digest);
    let store = StableExecutorCheckpointStore::new(&root).unwrap();
    store
        .publish(EventId::new(901).unwrap(), LeaseTerm::INITIAL, &executor)
        .unwrap();
    drop(store);

    let reopened = StableExecutorCheckpointStore::new(&root).unwrap();
    let restored = reopened
        .load(EventId::new(901).unwrap(), BrainId::new(700).unwrap(), plan)
        .unwrap();
    assert_eq!(restored.state_digest().unwrap(), digest);
    assert!(
        reopened
            .publish(EventId::new(901).unwrap(), LeaseTerm::INITIAL, &restored)
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fenced_authority_rolls_back_when_immutable_publication_fails() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-executor-authority-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let executor = fixture();
    let store = StableExecutorCheckpointStore::new(&root).unwrap();
    let mut authority = StableExecutorAuthority::new(executor, store, LeaseTerm::INITIAL, 7);
    authority
        .checkpoint(LeaseTerm::INITIAL, 7, EventId::new(910).unwrap())
        .unwrap();
    let before = authority.state_digest().unwrap();
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let event = EventId::new(911).unwrap();
    let payload = serde_json::to_vec(&StableTransitionInput {
        schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
        source: None,
        target,
        charge: 1,
        delay_ticks: 0,
    })
    .unwrap();
    let routed = RoutedCausalEvent {
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
    };
    assert!(
        authority
            .admit_and_step(
                LeaseTerm::INITIAL,
                7,
                routed.clone(),
                EventId::new(910).unwrap()
            )
            .is_err()
    );
    assert_eq!(authority.state_digest().unwrap(), before);
    assert!(
        authority
            .admit_and_step(
                LeaseTerm::new(2).unwrap(),
                7,
                routed,
                EventId::new(912).unwrap()
            )
            .is_err()
    );
    assert_eq!(authority.state_digest().unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stable_executor_step_can_be_committed_through_a_durable_shard_mirror() {
    let root =
        std::env::temp_dir().join(format!("aarnn-stable-shard-mirror-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let mut executor = fixture();
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let event_id = EventId::new(920).unwrap();
    let input = RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event_id.raw(),
            ),
            id: event_id,
            payload: serde_json::to_vec(&StableTransitionInput {
                schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
                source: None,
                target,
                charge: 1,
                delay_ticks: 0,
            })
            .unwrap(),
            original_tag: LogicalTag::ZERO,
            deferred_from_nonconvergence: false,
        },
    };
    let before = executor
        .checkpoint_shards()
        .unwrap()
        .into_iter()
        .find(|checkpoint| checkpoint.shard_id == ShardId::new(10).unwrap())
        .unwrap();

    let mut actor = AuthoritativeShard::open(
        root.join("owner.json"),
        Some(root.join("warm.json")),
        BrainId::new(700).unwrap(),
        before.shard_id,
        before.topology_generation,
        before.partition_generation,
        LeaseTerm::INITIAL,
        StreamId::new(77).unwrap(),
        1024 * 1024,
        serde_json::to_vec(&before).unwrap(),
        Vec::new(),
    )
    .unwrap();
    let actor_digest = actor.checkpoint().unwrap().state_digest;

    executor.admit(input.clone()).unwrap();
    let result = executor.step().unwrap().unwrap();
    let after = executor
        .checkpoint_shards()
        .unwrap()
        .into_iter()
        .find(|checkpoint| checkpoint.shard_id == before.shard_id)
        .unwrap();
    let envelope = CausalEnvelope {
        schema_version: SchemaVersion::CURRENT,
        brain: BrainId::new(700).unwrap(),
        stream: StreamId::new(77).unwrap(),
        sequence: 0,
        lease_term: LeaseTerm::INITIAL,
        route: RouteId::new(11).unwrap(),
        partition_generation: before.partition_generation,
        source: None,
        target: Some(target),
        tag: result.consumed.event.key.tag,
        event: event_id,
        stage: EventStage::SynapticTransition,
        kind: EnvelopeKind::Event,
        payload: result.consumed.event.payload.clone(),
        deferred_from_nonconvergence: false,
    };
    let after_bytes = serde_json::to_vec(&after).unwrap();
    assert!(
        actor
            .apply_stable_checkpoint(
                &envelope,
                StateDigest([9; 16]),
                after_bytes.clone(),
                Vec::new(),
            )
            .is_err()
    );
    let applied = actor
        .apply_stable_checkpoint(
            &envelope,
            actor_digest,
            after_bytes.clone(),
            serde_json::to_vec(&result.emitted).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        applied,
        aarnn_rust::durability::DurableApplyOutcome::Applied { .. }
    ));
    let duplicate = actor
        .apply_stable_checkpoint(&envelope, actor_digest, after_bytes.clone(), Vec::new())
        .unwrap();
    assert!(matches!(
        duplicate,
        aarnn_rust::durability::DurableApplyOutcome::Duplicate { .. }
    ));
    drop(actor);

    let reopened = AuthoritativeShard::open(
        root.join("owner.json"),
        Some(root.join("warm.json")),
        BrainId::new(700).unwrap(),
        before.shard_id,
        before.topology_generation,
        before.partition_generation,
        LeaseTerm::INITIAL,
        StreamId::new(77).unwrap(),
        1024 * 1024,
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(reopened.biological_state(), after_bytes);
    assert_eq!(reopened.durable_sequence(), Some(0));
    assert_eq!(reopened.receipt_count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn durable_bridge_publishes_one_complete_cut_to_all_shard_mirrors() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-executor-bridge-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let store_root = root.join("fabric");
    let owner_root = root.join("owners");
    let warm_root = root.join("warm");
    let bridge = StableExecutorDurableBridge::new(
        fixture(),
        StableExecutorCheckpointStore::new(&store_root).unwrap(),
        LeaseTerm::INITIAL,
        7,
        EventId::new(930).unwrap(),
        &owner_root,
        &warm_root,
        StreamId::new(88).unwrap(),
        1024 * 1024,
        Vec::new(),
    )
    .unwrap();
    let mut bridge = bridge;
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let event_id = EventId::new(931).unwrap();
    let input = RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event_id.raw(),
            ),
            id: event_id,
            payload: serde_json::to_vec(&StableTransitionInput {
                schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
                source: None,
                target,
                charge: 1,
                delay_ticks: 0,
            })
            .unwrap(),
            original_tag: LogicalTag::ZERO,
            deferred_from_nonconvergence: false,
        },
    };
    bridge
        .admit_and_step(LeaseTerm::INITIAL, 7, input, EventId::new(932).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(bridge.pending_mirror_event(), None);
    let durable_states = bridge.durable_states().unwrap();
    assert_eq!(durable_states.len(), 2);
    assert!(durable_states.iter().all(|state| state.verify().is_ok()));
    for checkpoint in bridge.executor().checkpoint_shards().unwrap() {
        assert_eq!(
            bridge
                .actor(checkpoint.shard_id)
                .unwrap()
                .biological_state(),
            serde_json::to_vec(&checkpoint).unwrap().as_slice()
        );
        assert_eq!(
            bridge.actor(checkpoint.shard_id).unwrap().receipt_count(),
            1
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn durable_bridge_retries_a_partial_mirror_and_prepares_verified_transfers() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-stable-executor-retry-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store_root = root.join("fabric");
    let owner_root = root.join("owners");
    let warm_root = root.join("warm");
    let mut bridge = StableExecutorDurableBridge::new(
        fixture(),
        StableExecutorCheckpointStore::new(&store_root).unwrap(),
        LeaseTerm::INITIAL,
        7,
        EventId::new(940).unwrap(),
        &owner_root,
        &warm_root,
        StreamId::new(89).unwrap(),
        1024 * 1024,
        Vec::new(),
    )
    .unwrap();

    // Make only the second mirror unavailable after the complete fabric cut
    // is published. The first actor must commit, while the bridge retains a
    // resumable operation instead of claiming a brain-wide success.
    let failed_warm_path = warm_root.join("shard-20.warm.json");
    std::fs::remove_file(&failed_warm_path).unwrap();
    std::fs::create_dir(&failed_warm_path).unwrap();
    let target = aarnn_rust::deterministic::NeuronId::new(1).unwrap();
    let event_id = EventId::new(941).unwrap();
    let input = RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event_id.raw(),
            ),
            id: event_id,
            payload: serde_json::to_vec(&StableTransitionInput {
                schema_version: BIOLOGICAL_STATE_SCHEMA_VERSION,
                source: None,
                target,
                charge: 1,
                delay_ticks: 0,
            })
            .unwrap(),
            original_tag: LogicalTag::ZERO,
            deferred_from_nonconvergence: false,
        },
    };
    assert!(
        bridge
            .admit_and_step(LeaseTerm::INITIAL, 7, input, EventId::new(942).unwrap())
            .is_err()
    );
    assert_eq!(bridge.pending_mirror_event(), Some(event_id));
    assert_eq!(
        bridge
            .actor(ShardId::new(10).unwrap())
            .unwrap()
            .receipt_count(),
        1
    );
    assert_eq!(
        bridge
            .actor(ShardId::new(20).unwrap())
            .unwrap()
            .receipt_count(),
        0
    );

    std::fs::remove_dir(&failed_warm_path).unwrap();
    bridge.retry_mirror().unwrap();
    assert_eq!(bridge.pending_mirror_event(), None);
    assert_eq!(
        bridge
            .actor(ShardId::new(20).unwrap())
            .unwrap()
            .receipt_count(),
        1
    );

    let sources = bridge
        .prepare_transfer_sources(
            EventId::new(950).unwrap(),
            "laptop",
            &initial_cut(),
            StateDigest([3; 16]),
            37,
        )
        .unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].manifest().shard_id, ShardId::new(10).unwrap());
    assert_eq!(sources[1].manifest().shard_id, ShardId::new(20).unwrap());

    for (index, source) in sources.into_iter().enumerate() {
        let mut receiver =
            aarnn_rust::migration_transfer::ShardTransferReceiver::new(source.manifest().clone())
                .unwrap();
        for frame in source.frames().unwrap().into_iter().rev() {
            receiver.accept(frame).unwrap();
        }
        let imported = receiver.finalize().unwrap();
        let destination_root = root.join(format!("destination-{index}"));
        let promoted = imported
            .promote_into_authoritative(
                destination_root.join("owner.json"),
                destination_root.join("warm.json"),
                LeaseTerm::new(2).unwrap(),
                StreamId::new(90 + index as u64).unwrap(),
                1024 * 1024,
            )
            .unwrap();
        assert_eq!(promoted.term(), LeaseTerm::new(2).unwrap());
        assert!(promoted.state().unwrap().verify().is_ok());
    }
    std::fs::remove_dir_all(root).unwrap();
}
