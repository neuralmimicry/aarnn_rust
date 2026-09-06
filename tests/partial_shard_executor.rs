use aarnn_rust::authoritative_shard::{
    BIOLOGICAL_STATE_SCHEMA_VERSION, FIXED_POINT_SCALE, StableTransitionInput,
};
use aarnn_rust::causal::CausalEvent;
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LogicalTag, NeuronId,
    PartitionGeneration, ShardId, SynapseId, TopologyGeneration,
};
use aarnn_rust::partial_shard_executor::{PartialShardExecutor, PartialShardOutbound};
use aarnn_rust::shard_executor::{RoutedCausalEvent, StableShardExecutor};
use aarnn_rust::topology_model::{
    NeuronRecord, OwnershipRecord, SynapseRecord, TopologyGenerationModel, VirtualShardAssignment,
    compile_execution_plan,
};
use std::collections::VecDeque;

fn fixture() -> (
    TopologyGenerationModel,
    aarnn_rust::topology_model::CompiledExecutionPlan,
    StableShardExecutor,
    BrainId,
) {
    let topology = TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        (1..=4)
            .map(|id| NeuronRecord {
                id: NeuronId::new(id).unwrap(),
            })
            .collect(),
        vec![
            SynapseRecord {
                id: SynapseId::new(11).unwrap(),
                source: NeuronId::new(1).unwrap(),
                target: NeuronId::new(2).unwrap(),
                delay_ticks: 0,
            },
            SynapseRecord {
                id: SynapseId::new(12).unwrap(),
                source: NeuronId::new(2).unwrap(),
                target: NeuronId::new(3).unwrap(),
                delay_ticks: 1,
            },
            SynapseRecord {
                id: SynapseId::new(13).unwrap(),
                source: NeuronId::new(3).unwrap(),
                target: NeuronId::new(4).unwrap(),
                delay_ticks: 0,
            },
        ],
    )
    .unwrap();
    let first = ShardId::new(1).unwrap();
    let second = ShardId::new(2).unwrap();
    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
            weight_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
            release_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
            plasticity_owner: if synapse.id == SynapseId::new(13).unwrap() {
                second
            } else {
                first
            },
        })
        .collect::<Vec<_>>();
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
    let brain = BrainId::new(901).unwrap();
    let executor = StableShardExecutor::from_topology(
        brain,
        &topology,
        plan.clone(),
        FIXED_POINT_SCALE,
        FIXED_POINT_SCALE,
        16,
        128,
    )
    .unwrap();
    (topology, plan, executor, brain)
}

fn input(event_id: u64, target: u64) -> RoutedCausalEvent {
    let target = NeuronId::new(target).unwrap();
    let event = EventId::new(event_id).unwrap();
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

fn deliver(
    destination: ShardId,
    message: PartialShardOutbound,
    first: &mut PartialShardExecutor,
    second: &mut PartialShardExecutor,
    queue: &mut VecDeque<(ShardId, PartialShardOutbound)>,
) {
    let target = match &message {
        PartialShardOutbound::CausalEvent {
            destination_shard, ..
        }
        | PartialShardOutbound::SynapseEffect {
            destination_shard, ..
        }
        | PartialShardOutbound::SynapseActivation {
            destination_shard, ..
        } => *destination_shard,
    };
    assert_eq!(target, destination);
    let result = if destination == ShardId::new(1).unwrap() {
        first.apply_outbound(message).unwrap()
    } else {
        second.apply_outbound(message).unwrap()
    };
    for outbound in result.outbound {
        let next = match &outbound {
            PartialShardOutbound::CausalEvent {
                destination_shard, ..
            }
            | PartialShardOutbound::SynapseEffect {
                destination_shard, ..
            }
            | PartialShardOutbound::SynapseActivation {
                destination_shard, ..
            } => *destination_shard,
        };
        queue.push_back((next, outbound));
    }
}

#[test]
fn partial_workers_route_cross_shard_work_and_match_the_complete_reference() {
    let (topology, plan, mut reference, brain) = fixture();
    let initial = reference.checkpoint_shards().unwrap();
    let first = ShardId::new(1).unwrap();
    let second = ShardId::new(2).unwrap();
    let mut worker_a = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        initial
            .iter()
            .filter(|cut| cut.shard_id == first)
            .cloned()
            .collect(),
        [first],
        32,
    )
    .unwrap();
    let mut worker_b = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan,
        initial
            .iter()
            .filter(|cut| cut.shard_id == second)
            .cloned()
            .collect(),
        [second],
        32,
    )
    .unwrap();

    let event = input(1, 1);
    reference.admit(event.clone()).unwrap();
    worker_a.admit(event).unwrap();

    let mut messages = VecDeque::new();
    for _ in 0..32 {
        if let Some((destination, message)) = messages.pop_front() {
            deliver(
                destination,
                message,
                &mut worker_a,
                &mut worker_b,
                &mut messages,
            );
            continue;
        }
        let mut progressed = false;
        for worker in [&mut worker_a, &mut worker_b] {
            if let Some(step) = worker.step().unwrap() {
                progressed = true;
                for outbound in step.outbound {
                    let destination = match &outbound {
                        PartialShardOutbound::CausalEvent {
                            destination_shard, ..
                        }
                        | PartialShardOutbound::SynapseEffect {
                            destination_shard, ..
                        }
                        | PartialShardOutbound::SynapseActivation {
                            destination_shard, ..
                        } => *destination_shard,
                    };
                    messages.push_back((destination, outbound));
                }
            }
        }
        if !progressed {
            break;
        }
    }
    assert!(
        messages.is_empty(),
        "all cross-shard messages must be delivered"
    );
    reference.settle(32).unwrap();
    assert_eq!(
        worker_a.state_bytes(first).unwrap(),
        reference.shard_state_bytes(first).unwrap()
    );
    assert_eq!(
        worker_b.state_bytes(second).unwrap(),
        reference.shard_state_bytes(second).unwrap()
    );
    assert_eq!(worker_a.total_pending(), 0);
    assert_eq!(worker_b.total_pending(), 0);
}

#[test]
fn duplicate_remote_control_is_idempotent_and_drained_worker_is_valid() {
    let (topology, plan, reference, brain) = fixture();
    let cuts = reference.checkpoint_shards().unwrap();
    let second = ShardId::new(2).unwrap();
    let mut worker = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        cuts.iter()
            .filter(|cut| cut.shard_id == second)
            .cloned()
            .collect(),
        [second],
        8,
    )
    .unwrap();
    let effect = PartialShardOutbound::SynapseEffect {
        plan_digest: plan.digest(),
        destination_shard: second,
        event_id: EventId::new(99).unwrap(),
        logical_tag: LogicalTag::ZERO,
        synapse: SynapseId::new(13).unwrap(),
        charge: 1,
    };
    assert!(!worker.apply_outbound(effect.clone()).unwrap().duplicate);
    assert!(worker.apply_outbound(effect).unwrap().duplicate);

    let drained = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan,
        Vec::new(),
        Vec::<ShardId>::new(),
        8,
    )
    .unwrap();
    assert_eq!(drained.owned_shards().count(), 0);
    assert_eq!(drained.total_pending(), 0);
}

#[test]
fn remote_control_conflicts_and_wrong_declared_destination_fail_closed() {
    let (topology, plan, reference, brain) = fixture();
    let cuts = reference.checkpoint_shards().unwrap();
    let first = ShardId::new(1).unwrap();
    let second = ShardId::new(2).unwrap();
    let mut worker = PartialShardExecutor::from_checkpoints(
        brain,
        &topology,
        plan.clone(),
        cuts.iter()
            .filter(|cut| cut.shard_id == second)
            .cloned()
            .collect(),
        [second],
        8,
    )
    .unwrap();

    let effect = PartialShardOutbound::SynapseEffect {
        plan_digest: plan.digest(),
        destination_shard: second,
        event_id: EventId::new(100).unwrap(),
        logical_tag: LogicalTag::ZERO,
        synapse: SynapseId::new(13).unwrap(),
        charge: 1,
    };
    worker.apply_outbound(effect.clone()).unwrap();
    let conflicting = PartialShardOutbound::SynapseEffect {
        plan_digest: plan.digest(),
        destination_shard: second,
        event_id: EventId::new(100).unwrap(),
        logical_tag: LogicalTag::ZERO,
        synapse: SynapseId::new(13).unwrap(),
        charge: 2,
    };
    assert!(matches!(
        worker.apply_outbound(conflicting),
        Err(aarnn_rust::partial_shard_executor::PartialShardExecutorError::ConflictingControl(
            id
        )) if id == EventId::new(100).unwrap()
    ));

    let wrong_destination = PartialShardOutbound::CausalEvent {
        plan_digest: plan.digest(),
        destination_shard: second,
        event: input(101, 1),
    };
    assert!(matches!(
        worker.apply_outbound(wrong_destination),
        Err(aarnn_rust::partial_shard_executor::PartialShardExecutorError::DestinationMismatch {
            declared,
            expected,
        }) if declared == second && expected == first
    ));
}
