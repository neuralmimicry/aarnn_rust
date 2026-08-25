use aarnn_rust::causal::{
    CausalEvent, LocalExecutor, SettlementError, SettlementOutcome, TransitionProcessor,
};
#[cfg(feature = "superdense_executor")]
use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::data_plane::{
    CausalEnvelope, EnvelopeKind, FaultInjectingTransport, ReceiveResult, ReliableReceiver,
    ReliableSender, TerminationProof,
};
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, ComponentId, EventId, EventStage, LeaseTerm, LogicalTag,
    PartitionGeneration, RouteId, SchemaVersion, ShardId, StreamId, TopologyGeneration,
};
use aarnn_rust::durability::{CausalWal, CheckpointStore, DurabilityError, WarmReplica};
use aarnn_rust::management::{
    Capability, ManagementError, ManagementOrchestrator, MutationContext, OperationKind,
    OperationState, Policy, Principal,
};
use aarnn_rust::multi_brain::{
    FairScheduler, PlacementRequest, ResourceInventory, choose_placement,
};
use aarnn_rust::peripheral::{
    ActuatorGate, ChannelGrant, ChannelKind, ClockMapping, Direction, EffectCommand,
    PeripheralSession, UsbAerFrame,
};
#[cfg(feature = "superdense_executor")]
use aarnn_rust::runner::Runner;
#[cfg(feature = "superdense_executor")]
use aarnn_rust::sim::{Learning, NeuronModel};
#[cfg(feature = "superdense_executor")]
use aarnn_rust::superdense::SuperdenseController;
use aarnn_rust::topology_model::{
    ExecutionPlanError, ExecutionPlanRegistry, NeuronRecord, OwnershipRecord, ShardCapacity,
    SynapseRecord, TopologyGenerationModel, TopologyProposal, VirtualShardAssignment,
    compile_execution_plan, plan_virtual_shards, validate_complete_ownership,
};
use std::collections::BTreeMap;

fn event(id: u64, tag: LogicalTag, payload: &[u8]) -> CausalEvent {
    CausalEvent::new(
        EventId::new(id).unwrap(),
        CanonicalEventKey::new(tag, EventStage::SynapticTransition, 1, 2, id),
        payload.to_vec(),
    )
}

struct Amplifier {
    next_id: u64,
}

impl TransitionProcessor for Amplifier {
    fn apply(&mut self, input: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError> {
        let tag = input.key.tag.advance(0)?;
        let output = event(self.next_id, tag, &[self.next_id as u8]);
        self.next_id += 1;
        Ok(vec![output])
    }
}

#[test]
fn phase2_preserves_nonconvergent_work_and_never_calls_it_quiescent() {
    let mut executor = LocalExecutor::new(ComponentId::new(1).unwrap(), 32);
    executor.admit(event(1, LogicalTag::ZERO, &[1])).unwrap();
    let outcome = executor
        .settle(
            LogicalTag::ZERO,
            3,
            LogicalTag::new(4, 0),
            &mut Amplifier { next_id: 2 },
        )
        .unwrap();
    let SettlementOutcome::DeferredNonConvergent { record, unresolved } = outcome else {
        panic!("settling-limit exhaustion must be non-convergence");
    };
    assert_eq!(record.completed_microsteps, 3);
    assert_eq!(record.unresolved_count, 1);
    assert_eq!(unresolved[0].key.tag, LogicalTag::new(4, 0));
    assert_eq!(unresolved[0].original_tag, LogicalTag::new(0, 3));
    assert!(unresolved[0].deferred_from_nonconvergence);
    assert_eq!(executor.committed().len(), 3);
}

#[test]
fn phase2_failed_transition_retains_uncommitted_event() {
    struct FailingProcessor;

    impl TransitionProcessor for FailingProcessor {
        fn apply(&mut self, _event: &CausalEvent) -> Result<Vec<CausalEvent>, SettlementError> {
            Err(SettlementError::Model(
                "deterministic fixture failure".to_owned(),
            ))
        }
    }

    let mut executor = LocalExecutor::new(ComponentId::new(1).unwrap(), 4);
    executor.admit(event(1, LogicalTag::ZERO, &[1])).unwrap();
    assert!(matches!(
        executor.settle(
            LogicalTag::ZERO,
            1,
            LogicalTag::new(1, 0),
            &mut FailingProcessor,
        ),
        Err(SettlementError::Model(_))
    ));
    assert_eq!(executor.pending_len(), 1);
    assert!(executor.committed().is_empty());
}

#[test]
fn phase3_scc_ownership_routes_and_planner_are_deterministic() {
    let topology = TopologyGenerationModel::new(
        TopologyGeneration::INITIAL,
        (1..=3)
            .map(|id| NeuronRecord {
                id: aarnn_rust::deterministic::NeuronId::new(id).unwrap(),
            })
            .collect(),
        vec![
            SynapseRecord {
                id: aarnn_rust::deterministic::SynapseId::new(1).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(1).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
                delay_ticks: 0,
            },
            SynapseRecord {
                id: aarnn_rust::deterministic::SynapseId::new(2).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(1).unwrap(),
                delay_ticks: 0,
            },
            SynapseRecord {
                id: aarnn_rust::deterministic::SynapseId::new(3).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(2).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(3).unwrap(),
                delay_ticks: 1,
            },
        ],
    )
    .unwrap();
    let graph = topology.zero_delay_components();
    assert_eq!(graph.components.len(), 2);
    assert_eq!(graph.components[0].members.len(), 2);
    let routes = topology.compile_routes(PartitionGeneration::INITIAL);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].id.raw(), routes[0].synapse.raw());
    let assignments = plan_virtual_shards(
        &graph,
        &BTreeMap::from([
            (ComponentId::new(1).unwrap(), 10),
            (ComponentId::new(2).unwrap(), 1),
        ]),
        vec![
            ShardCapacity {
                shard: ShardId::new(2).unwrap(),
                capacity: 10,
            },
            ShardCapacity {
                shard: ShardId::new(1).unwrap(),
                capacity: 10,
            },
        ],
    )
    .unwrap();
    assert_eq!(assignments[0].shard, ShardId::new(1).unwrap());
    assert!(
        plan_virtual_shards(
            &graph,
            &BTreeMap::new(),
            vec![
                ShardCapacity {
                    shard: ShardId::new(1).unwrap(),
                    capacity: 10,
                },
                ShardCapacity {
                    shard: ShardId::new(1).unwrap(),
                    capacity: 10,
                },
            ],
        )
        .is_err()
    );
    validate_complete_ownership(
        topology.synapses(),
        &[
            OwnershipRecord {
                synapse: aarnn_rust::deterministic::SynapseId::new(1).unwrap(),
                terminal_owner: ShardId::new(1).unwrap(),
                weight_owner: ShardId::new(1).unwrap(),
                release_owner: ShardId::new(2).unwrap(),
                plasticity_owner: ShardId::new(2).unwrap(),
            },
            OwnershipRecord {
                synapse: aarnn_rust::deterministic::SynapseId::new(2).unwrap(),
                terminal_owner: ShardId::new(1).unwrap(),
                weight_owner: ShardId::new(1).unwrap(),
                release_owner: ShardId::new(2).unwrap(),
                plasticity_owner: ShardId::new(2).unwrap(),
            },
            OwnershipRecord {
                synapse: aarnn_rust::deterministic::SynapseId::new(3).unwrap(),
                terminal_owner: ShardId::new(2).unwrap(),
                weight_owner: ShardId::new(2).unwrap(),
                release_owner: ShardId::new(2).unwrap(),
                plasticity_owner: ShardId::new(2).unwrap(),
            },
        ],
    )
    .unwrap();
    let mut complete_plus_extra = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: ShardId::new(1).unwrap(),
            weight_owner: ShardId::new(1).unwrap(),
            release_owner: ShardId::new(1).unwrap(),
            plasticity_owner: ShardId::new(1).unwrap(),
        })
        .collect::<Vec<_>>();
    complete_plus_extra.push(OwnershipRecord {
        synapse: aarnn_rust::deterministic::SynapseId::new(99).unwrap(),
        terminal_owner: ShardId::new(1).unwrap(),
        weight_owner: ShardId::new(1).unwrap(),
        release_owner: ShardId::new(1).unwrap(),
        plasticity_owner: ShardId::new(1).unwrap(),
    });
    assert!(matches!(
        validate_complete_ownership(topology.synapses(), &complete_plus_extra),
        Err(aarnn_rust::topology_model::TopologyError::UnexpectedOwnership(_))
    ));

    let ownership = topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: ShardId::new(if synapse.id.raw() == 3 { 2 } else { 1 }).unwrap(),
            weight_owner: ShardId::new(if synapse.id.raw() == 3 { 2 } else { 1 }).unwrap(),
            release_owner: ShardId::new(if synapse.id.raw() == 3 { 2 } else { 1 }).unwrap(),
            plasticity_owner: ShardId::new(if synapse.id.raw() == 3 { 2 } else { 1 }).unwrap(),
        })
        .collect::<Vec<_>>();
    let initial_plan = compile_execution_plan(
        &topology,
        PartitionGeneration::INITIAL,
        vec![
            VirtualShardAssignment {
                shard: ShardId::new(1).unwrap(),
                components: vec![ComponentId::new(1).unwrap()],
                load: 10,
            },
            VirtualShardAssignment {
                shard: ShardId::new(2).unwrap(),
                components: vec![ComponentId::new(2).unwrap()],
                load: 1,
            },
        ],
        ownership,
    )
    .unwrap();
    let route = initial_plan.route(RouteId::new(3).unwrap()).unwrap();
    initial_plan
        .validate_event(
            TopologyGeneration::INITIAL,
            PartitionGeneration::INITIAL,
            route.from,
            route.to,
            Some(route.id),
        )
        .unwrap();
    assert!(matches!(
        initial_plan.validate_event(
            TopologyGeneration::INITIAL,
            PartitionGeneration::new(2).unwrap(),
            route.from,
            route.to,
            Some(route.id),
        ),
        Err(ExecutionPlanError::StalePartitionGeneration { .. })
    ));

    let next_topology = topology
        .apply_proposal(TopologyProposal {
            base_generation: TopologyGeneration::INITIAL,
            effective_tag: LogicalTag::new(2, 0),
            add_neurons: vec![NeuronRecord {
                id: aarnn_rust::deterministic::NeuronId::new(4).unwrap(),
            }],
            add_synapses: vec![SynapseRecord {
                id: aarnn_rust::deterministic::SynapseId::new(4).unwrap(),
                source: aarnn_rust::deterministic::NeuronId::new(3).unwrap(),
                target: aarnn_rust::deterministic::NeuronId::new(4).unwrap(),
                delay_ticks: 1,
            }],
        })
        .unwrap();
    let next_ownership = next_topology
        .synapses()
        .map(|synapse| OwnershipRecord {
            synapse: synapse.id,
            terminal_owner: ShardId::new(if synapse.id.raw() >= 3 { 2 } else { 1 }).unwrap(),
            weight_owner: ShardId::new(if synapse.id.raw() >= 3 { 2 } else { 1 }).unwrap(),
            release_owner: ShardId::new(if synapse.id.raw() >= 3 { 2 } else { 1 }).unwrap(),
            plasticity_owner: ShardId::new(if synapse.id.raw() >= 3 { 2 } else { 1 }).unwrap(),
        })
        .collect::<Vec<_>>();
    let next_plan = compile_execution_plan(
        &next_topology,
        PartitionGeneration::new(2).unwrap(),
        vec![
            VirtualShardAssignment {
                shard: ShardId::new(1).unwrap(),
                components: vec![ComponentId::new(1).unwrap()],
                load: 10,
            },
            VirtualShardAssignment {
                shard: ShardId::new(2).unwrap(),
                components: vec![ComponentId::new(2).unwrap()],
                load: 1,
            },
            VirtualShardAssignment {
                shard: ShardId::new(3).unwrap(),
                components: vec![ComponentId::new(3).unwrap()],
                load: 1,
            },
        ],
        next_ownership,
    )
    .unwrap();
    let mut registry = ExecutionPlanRegistry::new(initial_plan);
    registry
        .propose(LogicalTag::new(2, 0), LogicalTag::ZERO, next_plan)
        .unwrap();
    assert!(!registry.activate_at(LogicalTag::new(1, 0)).unwrap());
    assert!(registry.activate_at(LogicalTag::new(2, 0)).unwrap());
    assert_eq!(registry.active().topology_generation().raw(), 2);

    #[cfg(feature = "superdense_executor")]
    {
        let mut controller = SuperdenseController::new_with_plan(
            registry.active().clone(),
            ComponentId::new(1).unwrap(),
        )
        .unwrap();
        let mut runner = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Lif,
            Learning::Stdp,
        );
        controller.step(&mut runner, None).unwrap();
        assert_eq!(
            controller
                .active_execution_plan()
                .unwrap()
                .topology_generation()
                .raw(),
            2
        );
    }
}

#[test]
fn phase4_reliable_streams_dedupe_and_do_not_use_watermarks_as_termination() {
    let stream = StreamId::new(1).unwrap();
    let term = LeaseTerm::INITIAL;
    let generation = PartitionGeneration::INITIAL;
    let mut sender = ReliableSender::new(
        BrainId::new(1).unwrap(),
        stream,
        term,
        RouteId::new(1).unwrap(),
        generation,
        2,
        8,
    );
    let first = sender
        .send(LogicalTag::ZERO, EventId::new(1).unwrap(), vec![1])
        .unwrap();
    let second = sender
        .send(LogicalTag::new(1, 0), EventId::new(2).unwrap(), vec![2])
        .unwrap();
    let mut transport = FaultInjectingTransport::new(2);
    transport.send(first.clone()).unwrap();
    transport.fault_next_send();
    assert!(transport.send(second.clone()).is_err());
    assert_eq!(
        sender.inflight_len(),
        2,
        "failed transport must not discard committed sender state"
    );
    let mut receiver = ReliableReceiver::new(BrainId::new(1).unwrap(), stream, term, generation, 8);
    assert_eq!(
        receiver.accept(&first).unwrap(),
        ReceiveResult::Accepted { sequence: 0 }
    );
    assert_eq!(
        receiver.accept(&first).unwrap(),
        ReceiveResult::Duplicate { sequence: 0 }
    );
    receiver.observe_watermark(LogicalTag::new(99, 0)).unwrap();
    assert!(
        !TerminationProof {
            component: ComponentId::new(1).unwrap(),
            tag: LogicalTag::new(99, 0),
            membership_epoch: 1,
            local_queue_empty: true,
            output_staging_empty: true,
            send_balance: 1,
            activity_epoch: 1,
        }
        .proves_closure()
    );
    sender.acknowledge(0, 1);
    assert_eq!(sender.inflight_len(), 1);
}

#[test]
fn phase4_rejects_unsupported_schema_and_unknown_kind() {
    let brain = BrainId::new(1).unwrap();
    let stream = StreamId::new(1).unwrap();
    let generation = PartitionGeneration::INITIAL;
    let mut receiver = ReliableReceiver::new(brain, stream, LeaseTerm::INITIAL, generation, 8);
    let mut envelope = CausalEnvelope {
        schema_version: SchemaVersion::new(2).unwrap(),
        brain,
        stream,
        sequence: 0,
        lease_term: LeaseTerm::INITIAL,
        route: RouteId::new(1).unwrap(),
        partition_generation: generation,
        source: None,
        target: None,
        tag: LogicalTag::ZERO,
        event: EventId::new(1).unwrap(),
        stage: aarnn_rust::deterministic::EventStage::SpikeDecision,
        kind: EnvelopeKind::Event,
        payload: vec![1],
        deferred_from_nonconvergence: false,
    };
    assert!(matches!(
        receiver.accept(&envelope),
        Err(aarnn_rust::data_plane::DataPlaneError::UnsupportedSchema(_))
    ));
    envelope.schema_version = SchemaVersion::CURRENT;
    envelope.kind = EnvelopeKind::Unknown;
    assert!(matches!(
        receiver.accept(&envelope),
        Err(aarnn_rust::data_plane::DataPlaneError::UnknownEnvelopeKind)
    ));
}

#[test]
fn phase5_brains_dispatch_independently_and_phase6_is_fenced_immutable() {
    let mut scheduler = FairScheduler::new(8);
    scheduler.admit_brain(BrainId::new(2).unwrap(), 4);
    scheduler.admit_brain(BrainId::new(1).unwrap(), 4);
    scheduler
        .admit(BrainId::new(1).unwrap(), LogicalTag::ZERO, 1)
        .unwrap();
    scheduler
        .admit(BrainId::new(2).unwrap(), LogicalTag::ZERO, 1)
        .unwrap();
    assert_eq!(
        scheduler.dispatch_one().unwrap().brain,
        BrainId::new(1).unwrap()
    );
    assert_eq!(
        scheduler.dispatch_one().unwrap().brain,
        BrainId::new(2).unwrap()
    );
    assert_eq!(
        choose_placement(
            &PlacementRequest {
                brain: BrainId::new(1).unwrap(),
                cpu_cores: 2,
                memory_bytes: 10,
                gpu_memory_bytes: 0
            },
            &[(
                ShardId::new(2).unwrap(),
                ResourceInventory {
                    cpu_cores: 2,
                    memory_bytes: 10,
                    gpu_memory_bytes: 0
                }
            )],
        )
        .unwrap()
        .shard,
        ShardId::new(2).unwrap()
    );

    let mut wal = CausalWal::new(LeaseTerm::INITIAL);
    let neural_event = event(5, LogicalTag::ZERO, &[9]);
    let record = wal.append(LeaseTerm::INITIAL, &neural_event).unwrap();
    assert!(matches!(
        wal.append(LeaseTerm::new(2).unwrap(), &neural_event),
        Err(DurabilityError::StaleTerm { .. })
    ));
    let mut replica = WarmReplica::new(LeaseTerm::INITIAL);
    replica.apply(record).unwrap();
    assert_eq!(replica.applied(), 1);
    let mut store = CheckpointStore::default();
    store
        .publish(
            EventId::new(8).unwrap(),
            LeaseTerm::INITIAL,
            generation(),
            wal.last_sequence(),
            vec![1, 2],
        )
        .unwrap();
    assert!(matches!(
        store.publish(
            EventId::new(8).unwrap(),
            LeaseTerm::INITIAL,
            generation(),
            None,
            vec![3]
        ),
        Err(DurabilityError::CheckpointAlreadyPublished(_))
    ));
    store.verify(EventId::new(8).unwrap()).unwrap();
}

fn generation() -> PartitionGeneration {
    PartitionGeneration::INITIAL
}

#[test]
fn phase7_management_is_default_deny_and_idempotent_and_phase8_channels_are_independent() {
    let principal = Principal {
        id: "operator".to_owned(),
    };
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    let mut manager = ManagementOrchestrator::new(LeaseTerm::INITIAL, policy);
    let context = || MutationContext {
        observed_leader_term: LeaseTerm::INITIAL,
        expected_version: 0,
        idempotency_key: "start-1".to_owned(),
        request_id: "request-1".to_owned(),
    };
    let denied = manager.submit(
        Principal {
            id: "viewer".to_owned(),
        },
        Capability::Operate,
        context(),
        OperationKind::Start,
    );
    assert!(matches!(denied, Err(ManagementError::Forbidden { .. })));
    let operation = manager
        .submit(
            principal.clone(),
            Capability::Operate,
            context(),
            OperationKind::Start,
        )
        .unwrap();
    let duplicate = manager
        .submit(
            principal,
            Capability::Operate,
            context(),
            OperationKind::Start,
        )
        .unwrap();
    assert_eq!(operation.id, duplicate.id);
    manager
        .transition(operation.id, LeaseTerm::INITIAL, OperationState::Succeeded)
        .unwrap();
    assert_eq!(manager.audit().len(), 2);

    let mapping = ClockMapping {
        version: 1,
        capture_origin_ns: 100,
        biological_origin_tick: 4,
        nanos_per_tick: 10,
        uncertainty_ns: 2,
    };
    let mut session = PeripheralSession::new(
        EventId::new(20).unwrap(),
        BrainId::new(1).unwrap(),
        "operator",
    );
    let aer = StreamId::new(1).unwrap();
    let audio = StreamId::new(2).unwrap();
    session
        .bind_channel(
            aer,
            ChannelKind::UsbAer,
            Direction::Input,
            ChannelGrant {
                input: true,
                output: false,
            },
            mapping,
            4,
        )
        .unwrap();
    session
        .bind_channel(
            audio,
            ChannelKind::Audio,
            Direction::Input,
            ChannelGrant {
                input: true,
                output: false,
            },
            mapping,
            4,
        )
        .unwrap();
    let sample = session.admit_sample(aer, 1, 7, 120, 1, vec![1]).unwrap();
    assert_eq!(sample.biological_tag, LogicalTag::new(6, 0));
    session.disconnect(aer).unwrap();
    assert!(session.admit_sample(aer, 1, 8, 130, 1, vec![2]).is_err());
    assert!(session.admit_sample(audio, 1, 1, 120, 1, vec![3]).is_ok());
    session.revoke();
    assert!(session.admit_sample(audio, 1, 2, 130, 1, vec![4]).is_err());
    UsbAerFrame {
        protocol_version: 1,
        sequence: 1,
        timestamp_ns: 120,
        address: 4,
        polarity: true,
        crc16: None,
    }
    .validate(8)
    .unwrap();
    let mut gate = ActuatorGate::new(LeaseTerm::INITIAL);
    gate.arm(LeaseTerm::INITIAL).unwrap();
    let effect = EffectCommand {
        id: EventId::new(30).unwrap(),
        channel: aer,
        device_epoch: 1,
        lease_term: LeaseTerm::INITIAL,
        payload: vec![1],
    };
    assert!(gate.commit(effect.clone(), false, 1).is_err());
    assert!(gate.commit(effect.clone(), true, 1).unwrap());
    assert!(!gate.commit(effect, true, 1).unwrap());
}
