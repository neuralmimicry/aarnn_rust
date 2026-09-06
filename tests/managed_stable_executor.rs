#![cfg(feature = "stable_executor_live")]

use aarnn_rust::authoritative_shard::{
    BIOLOGICAL_STATE_SCHEMA_VERSION, FIXED_POINT_SCALE, StableTransitionInput,
};
use aarnn_rust::causal::CausalEvent;
use aarnn_rust::data_plane::{CausalEnvelope, EnvelopeKind};
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId,
    PartitionGeneration, RouteId, SchemaVersion, ShardId, StreamId, SynapseId, TopologyGeneration,
};
use aarnn_rust::managed_stable_executor::ManagedStableExecutor;
use aarnn_rust::shard_executor::{RoutedCausalEvent, StableShardExecutor};
use aarnn_rust::stable_executor_durable::StableExecutorDurableBridge;
use aarnn_rust::stable_executor_store::StableExecutorCheckpointStore;
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};

fn executor(with_synapses: bool) -> StableShardExecutor {
    let neurons = (1..=4)
        .map(|id| NeuronRecord {
            id: aarnn_rust::deterministic::NeuronId::new(id).unwrap(),
        })
        .collect::<Vec<_>>();
    let synapses = if with_synapses {
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
        ]
    } else {
        Vec::new()
    };
    let topology =
        TopologyGenerationModel::new(TopologyGeneration::INITIAL, neurons, synapses).unwrap();
    let first = ShardId::new(10).unwrap();
    let second = ShardId::new(20).unwrap();
    let assignments = vec![
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
    ];
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
        assignments,
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

fn input(event_id: u64, target: u64) -> RoutedCausalEvent {
    let target = aarnn_rust::deterministic::NeuronId::new(target).unwrap();
    RoutedCausalEvent {
        route: None,
        event: CausalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::ZERO,
                EventStage::SynapticTransition,
                0,
                target.raw(),
                event_id,
            ),
            id: EventId::new(event_id).unwrap(),
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
    }
}

fn runtime(with_synapses: bool, max_steps_per_poll: usize, suffix: &str) -> ManagedStableExecutor {
    let root = std::env::temp_dir().join(format!(
        "aarnn-managed-stable-{}-{}-{}",
        std::process::id(),
        suffix,
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&root);
    let store = StableExecutorCheckpointStore::new(root.join("checkpoints")).unwrap();
    let bridge = StableExecutorDurableBridge::new(
        executor(with_synapses),
        store,
        LeaseTerm::INITIAL,
        7,
        EventId::new(1000).unwrap(),
        root.join("owner"),
        root.join("warm"),
        StreamId::new(77).unwrap(),
        1024 * 1024,
        Vec::new(),
    )
    .unwrap();
    ManagedStableExecutor::new(bridge, EventId::new(1000).unwrap(), 8, max_steps_per_poll).unwrap()
}

#[test]
fn managed_runtime_executes_and_publishes_one_stable_transition() {
    let mut runtime = runtime(false, 4, "commit");
    let before = runtime.bridge().authority().state_digest().unwrap();
    let poll = runtime.poll(LeaseTerm::INITIAL, 7, &[input(1, 1)]).unwrap();

    assert_eq!(poll.steps.len(), 1);
    assert_eq!(poll.steps[0].consumed.event.id, EventId::new(1).unwrap());
    assert!(!poll.budget_exhausted);
    assert!(poll.is_quiescent());
    assert_ne!(runtime.bridge().authority().state_digest().unwrap(), before);
    assert!(runtime.bridge().authority().last_checkpoint().is_some());
    assert_eq!(runtime.bridge().pending_mirror_event(), None);
}

#[test]
fn external_causal_envelope_uses_the_fenced_stable_commit_boundary() {
    let mut runtime = runtime(false, 4, "envelope");
    let routed = input(41, 1);
    let envelope = CausalEnvelope {
        schema_version: SchemaVersion::CURRENT,
        brain: BrainId::new(700).unwrap(),
        stream: StreamId::new(77).unwrap(),
        sequence: 0,
        lease_term: LeaseTerm::INITIAL,
        route: RouteId::new(1).unwrap(),
        partition_generation: PartitionGeneration::INITIAL,
        source: None,
        target: Some(NeuronId::new(1).unwrap()),
        tag: routed.event.key.tag,
        event: routed.event.id,
        stage: routed.event.key.stage,
        kind: EnvelopeKind::Event,
        payload: routed.event.payload.clone(),
        deferred_from_nonconvergence: false,
    };
    let poll = runtime
        .poll_envelope(&envelope, LeaseTerm::INITIAL, 7)
        .expect("causal envelope must commit through the stable boundary");
    assert_eq!(poll.steps.len(), 1);

    let mut stale = envelope;
    stale.lease_term = LeaseTerm::new(2).unwrap();
    let error = runtime
        .poll_envelope(&stale, LeaseTerm::INITIAL, 7)
        .expect_err("stale causal term must be rejected before mutation");
    assert!(error.to_string().contains("lease term"));
}

#[test]
fn queued_causal_work_is_drained_through_the_same_durable_boundary() {
    let mut runtime = runtime(true, 1, "pending");
    let first = runtime.poll(LeaseTerm::INITIAL, 7, &[input(2, 1)]).unwrap();
    assert_eq!(first.steps.len(), 1);
    assert!(first.budget_exhausted);
    assert!(first.pending_after > 0);

    let second = runtime.drain(LeaseTerm::INITIAL, 7).unwrap();
    assert_eq!(second.steps.len(), 1);
    assert!(second.steps[0].consumed.event.key.target == 2);
    assert!(runtime.bridge().pending_mirror_event().is_none());
}

#[test]
fn migration_drain_freezes_admission_until_aborted() {
    let mut runtime = runtime(true, 1, "migration-drain");
    let first = runtime.poll(LeaseTerm::INITIAL, 7, &[input(8, 1)]).unwrap();
    assert!(first.pending_after > 0);

    let checkpoint_start = runtime.bridge().next_checkpoint_id().unwrap();
    let states = runtime
        .bridge_mut()
        .drain_for_migration(LeaseTerm::INITIAL, 7, checkpoint_start, 64)
        .unwrap();
    assert_eq!(states.len(), 2);
    assert_eq!(runtime.bridge().executor().total_pending(), 0);
    assert!(runtime.bridge().migration_draining());

    let error = runtime
        .poll(LeaseTerm::INITIAL, 7, &[input(9, 1)])
        .expect_err("migration drain must reject new admission");
    assert!(error.to_string().contains("draining for migration"));

    runtime.bridge_mut().abort_migration_drain();
    assert!(!runtime.bridge().migration_draining());
    let mut retry = input(9, 1);
    retry.event.key.tag = runtime.bridge().executor().current_tag();
    retry.event.original_tag = retry.event.key.tag;
    let checkpoint = runtime.bridge().next_checkpoint_id().unwrap();
    runtime
        .bridge_mut()
        .admit_and_step(LeaseTerm::INITIAL, 7, retry, checkpoint)
        .expect("aborting a pre-fence drain must reopen admission");
}

#[test]
fn migration_drain_reports_a_bounded_frontier_failure() {
    let mut runtime = runtime(true, 1, "migration-drain-limit");
    let first = runtime
        .poll(LeaseTerm::INITIAL, 7, &[input(10, 1)])
        .unwrap();
    assert!(first.pending_after > 0);

    let checkpoint_start = runtime.bridge().next_checkpoint_id().unwrap();
    let error = runtime
        .bridge_mut()
        .drain_for_migration(LeaseTerm::INITIAL, 7, checkpoint_start, 1)
        .expect_err("a continuing pending frontier must hit the explicit bound");
    assert!(matches!(
        error,
        aarnn_rust::stable_executor_durable::StableExecutorDurableError::MigrationDrainLimit
    ));
    assert!(!runtime.bridge().migration_draining());
}

#[test]
fn duplicate_input_is_idempotent_and_does_not_publish_a_second_transition() {
    let mut runtime = runtime(false, 4, "duplicate");
    let event = input(3, 1);
    let first = runtime
        .poll(LeaseTerm::INITIAL, 7, std::slice::from_ref(&event))
        .unwrap();
    let digest = runtime.bridge().authority().state_digest().unwrap();
    let second = runtime
        .poll(LeaseTerm::INITIAL, 7, std::slice::from_ref(&event))
        .unwrap();

    assert_eq!(first.steps.len(), 1);
    assert!(second.steps.is_empty());
    assert!(second.is_quiescent());
    assert_eq!(runtime.bridge().authority().state_digest().unwrap(), digest);
}

#[test]
fn stale_writer_is_rejected_before_the_stable_executor_mutates() {
    let mut runtime = runtime(false, 4, "fence");
    let before = runtime.bridge().authority().state_digest().unwrap();
    let error = runtime.poll(LeaseTerm::INITIAL, 8, &[input(4, 1)]);

    assert!(error.is_err());
    assert_eq!(runtime.bridge().authority().state_digest().unwrap(), before);
}

#[test]
fn poll_budget_reports_deferred_work_instead_of_quiescence() {
    let mut runtime = runtime(true, 1, "budget");
    let poll = runtime.poll(LeaseTerm::INITIAL, 7, &[input(5, 1)]).unwrap();

    assert_eq!(poll.steps.len(), 1);
    assert!(poll.pending_after > 0);
    assert!(poll.budget_exhausted);
    assert!(!poll.is_quiescent());
}

#[test]
fn restart_reopens_the_published_cut_and_continues_mirror_sequence() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-managed-stable-reopen-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = std::fs::remove_dir_all(&root);
    let store = StableExecutorCheckpointStore::new(root.join("checkpoints")).unwrap();
    let source = executor(false);
    let plan = source.plan().clone();
    let bridge = StableExecutorDurableBridge::new(
        source,
        store.clone(),
        LeaseTerm::INITIAL,
        7,
        EventId::new(1000).unwrap(),
        root.join("owner"),
        root.join("warm"),
        StreamId::new(77).unwrap(),
        1024 * 1024,
        Vec::new(),
    )
    .unwrap();
    let mut running =
        ManagedStableExecutor::new(bridge, EventId::new(1000).unwrap(), 8, 4).unwrap();

    running.poll(LeaseTerm::INITIAL, 7, &[input(6, 1)]).unwrap();
    let digest_before_restart = running.bridge().authority().state_digest().unwrap();
    let acknowledgements_before_restart = running.bridge().application_acknowledgements().unwrap();
    assert_eq!(
        running
            .bridge()
            .actor(ShardId::new(10).unwrap())
            .unwrap()
            .durable_sequence(),
        Some(0)
    );
    drop(running);

    let restored = store
        .load(
            EventId::new(1001).unwrap(),
            BrainId::new(700).unwrap(),
            plan,
        )
        .unwrap();
    let reopened_bridge = StableExecutorDurableBridge::open_existing(
        restored,
        store,
        LeaseTerm::INITIAL,
        7,
        root.join("owner"),
        root.join("warm"),
        StreamId::new(77).unwrap(),
        1024 * 1024,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        reopened_bridge.authority().state_digest().unwrap(),
        digest_before_restart
    );
    assert_eq!(
        reopened_bridge.application_acknowledgements().unwrap(),
        acknowledgements_before_restart,
        "registration evidence must be reconstructed from durable actor checkpoints"
    );
    assert_eq!(reopened_bridge.pending_mirror_event(), None);

    let mut resumed =
        ManagedStableExecutor::new(reopened_bridge, EventId::new(1001).unwrap(), 8, 4).unwrap();
    resumed.poll(LeaseTerm::INITIAL, 7, &[input(7, 2)]).unwrap();
    assert_eq!(
        resumed
            .bridge()
            .actor(ShardId::new(10).unwrap())
            .unwrap()
            .durable_sequence(),
        Some(1)
    );
    assert_ne!(
        resumed.bridge().authority().state_digest().unwrap(),
        digest_before_restart
    );
}
