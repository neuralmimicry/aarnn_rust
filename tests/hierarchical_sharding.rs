use aarnn_rust::deterministic::NeuronId;
use aarnn_rust::hierarchical_sharding::{
    BiologicalAreaVolume, BiologicalEllipsoid, BiologicalGrowthDecision, BiologicalGrowthSpace,
    BiologicalMembrane, BiologicalNeuronTransfer, BiologicalTopologyTransaction,
    HierarchicalShardingRequest, HostCapacity, LatencyMatrix, NetworkInteraction,
    NeuralNetworkInput, NeuralUnit, NeuralUnitKind, PhysicalNeuronLocation, area_layer_units,
    plan_hierarchical_shards,
};
use aarnn_rust::runner::{Snapshot, decode_snapshot_with_profile_backfill};
use std::collections::BTreeMap;

fn unit(network: &str, area: u64, layer: u32, neuron: u64, work: u64) -> NeuralUnit {
    NeuralUnit {
        network_id: network.to_owned(),
        area_id: area,
        area_label: format!("test-area-{area}"),
        layer,
        neuron_id: NeuronId::new(neuron).unwrap(),
        kind: NeuralUnitKind::BiologicalNeuron,
        work_units: work,
        state_bytes: work * 2,
    }
}

fn request() -> HierarchicalShardingRequest {
    HierarchicalShardingRequest {
        networks: vec![
            NeuralNetworkInput {
                network_id: "brain-a".to_owned(),
                home_node: "host-a".to_owned(),
                units: vec![
                    unit("brain-a", 10, 0, 1, 8),
                    unit("brain-a", 10, 0, 2, 8),
                    unit("brain-a", 10, 1, 3, 8),
                    unit("brain-a", 20, 0, 4, 4),
                ],
            },
            NeuralNetworkInput {
                network_id: "brain-b".to_owned(),
                home_node: "host-b".to_owned(),
                units: vec![unit("brain-b", 30, 0, 5, 4)],
            },
        ],
        interactions: vec![NetworkInteraction {
            source_network: "brain-a".to_owned(),
            target_network: "brain-b".to_owned(),
            events_per_tick: 100,
        }],
        hosts: vec![
            HostCapacity {
                node_id: "host-a".to_owned(),
                capacity_units: 16,
                memory_bytes: 128,
            },
            HostCapacity {
                node_id: "host-b".to_owned(),
                capacity_units: 64,
                memory_bytes: 256,
            },
            HostCapacity {
                node_id: "host-c".to_owned(),
                capacity_units: 64,
                memory_bytes: 256,
            },
        ],
        latency: LatencyMatrix::new(
            vec![
                "host-a".to_owned(),
                "host-b".to_owned(),
                "host-c".to_owned(),
            ],
            vec![
                ("host-a".to_owned(), "host-b".to_owned(), 20),
                ("host-b".to_owned(), "host-a".to_owned(), 20),
                ("host-a".to_owned(), "host-c".to_owned(), 5),
                ("host-c".to_owned(), "host-a".to_owned(), 5),
                ("host-b".to_owned(), "host-c".to_owned(), 5),
                ("host-c".to_owned(), "host-b".to_owned(), 5),
            ],
        ),
        target_sub_shard_work_units: 10,
    }
}

#[test]
fn network_groups_precede_area_then_layer_and_sub_shard_placement() {
    let plan = plan_hierarchical_shards(request()).unwrap();
    plan.verify().unwrap();
    assert_eq!(
        plan.groups.len(),
        1,
        "communication joins the two brains first"
    );
    assert_eq!(plan.groups[0].communication_weight, 100);
    assert_eq!(plan.shards.len(), 3, "areas, not layers, are parent shards");
    assert!(plan.shards.iter().all(|shard| !shard.layers.is_empty()));
    assert!(plan.sub_shard_count() > plan.shards.len());
    assert!(
        plan.shards
            .iter()
            .flat_map(|shard| shard.layers.iter())
            .flat_map(|layer| layer.sub_shards.iter())
            .any(|sub_shard| sub_shard.active_node == "host-c")
    );
}

#[test]
fn measured_latency_is_the_primary_host_choice_and_input_order_is_irrelevant() {
    let first = plan_hierarchical_shards(request()).unwrap();
    let mut reversed = request();
    reversed.networks.reverse();
    reversed.hosts.reverse();
    reversed.networks[0].units.reverse();
    let second = plan_hierarchical_shards(reversed).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.digest, second.digest);
    assert!(
        first
            .shards
            .iter()
            .flat_map(|shard| shard.layers.iter())
            .flat_map(|layer| layer.sub_shards.iter())
            .all(|sub_shard| sub_shard.latency_to_group_anchor_us <= 5)
    );
}

#[test]
fn physical_area_labels_are_deterministic_and_growth_is_append_stable() {
    let counts = BTreeMap::from([(0, 3), (1, 2)]);
    let initial_locations = vec![
        PhysicalNeuronLocation {
            layer: 0,
            index: 0,
            x: Some(-0.8),
            y: Some(0.0),
            z: Some(0.0),
            region_label: Some("Central_Brain_Left".to_owned()),
        },
        PhysicalNeuronLocation {
            layer: 0,
            index: 1,
            x: Some(-0.7),
            y: Some(0.0),
            z: Some(0.0),
            region_label: Some("Central_Brain_Left".to_owned()),
        },
        PhysicalNeuronLocation {
            layer: 1,
            index: 0,
            x: Some(0.7),
            y: Some(0.0),
            z: Some(0.0),
            region_label: Some("Central_Brain_Right".to_owned()),
        },
    ];
    let initial = area_layer_units("grown", &counts, &initial_locations, 8);
    assert_eq!(initial.len(), 4);
    assert!(
        initial
            .iter()
            .any(|unit| unit.area_label == "central_brain_left")
    );
    assert!(
        initial
            .iter()
            .any(|unit| unit.area_label == "central_brain_right")
    );

    let mut grown_counts = counts.clone();
    grown_counts.insert(0, 4);
    let grown = area_layer_units("grown", &grown_counts, &initial_locations, 8);
    for old in &initial {
        let replacement = grown
            .iter()
            .find(|candidate| candidate.area_id == old.area_id && candidate.layer == old.layer)
            .expect("existing area/layer remains labelled after growth");
        assert_eq!(replacement.area_label, old.area_label);
        assert!(replacement.work_units >= old.work_units);
    }

    let mut reversed = initial_locations.clone();
    reversed.reverse();
    assert_eq!(
        area_layer_units("grown", &grown_counts, &reversed, 8),
        area_layer_units("grown", &grown_counts, &initial_locations, 8)
    );
}

#[test]
fn snapshot_import_export_round_trip_keeps_additive_placement_schema_compatible() {
    let original = serde_json::to_string(&Snapshot::default()).unwrap();
    let decoded = decode_snapshot_with_profile_backfill(&original).unwrap();
    let exported = serde_json::to_string(&decoded).unwrap();
    let reimported = decode_snapshot_with_profile_backfill(&exported).unwrap();
    assert_eq!(decoded.net, reimported.net);
    assert_eq!(
        serde_json::to_value(&decoded.w_hh_fwd).unwrap(),
        serde_json::to_value(&reimported.w_hh_fwd).unwrap()
    );
    assert_eq!(decoded.layer_range, reimported.layer_range);
}

#[test]
fn active_plan_has_one_biological_owner_and_excludes_replica_views() {
    let plan = plan_hierarchical_shards(request()).unwrap();
    let ownership = plan.active_biological_ownership().unwrap();

    assert_eq!(ownership.neuron_count(), 5);
    assert_eq!(
        ownership
            .owners()
            .map(|placement| placement.neuron_id)
            .collect::<Vec<_>>()
            .len(),
        ownership
            .owners()
            .map(|placement| placement.neuron_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
}

#[test]
fn area_layer_aggregates_cannot_be_promoted_to_biological_neurons() {
    let mut aggregate_request = request();
    aggregate_request.networks[0].units =
        area_layer_units("brain-a", &BTreeMap::from([(0, 2)]), &[], 8);
    let plan = plan_hierarchical_shards(aggregate_request).unwrap();
    assert!(matches!(
        plan.active_biological_ownership(),
        Err(aarnn_rust::hierarchical_sharding::BiologicalOwnershipError::AggregateUnit(_))
    ));
}

#[test]
fn local_growth_stays_in_parent_area_and_publishes_both_generations() {
    let plan = plan_hierarchical_shards(request()).unwrap();
    let ownership = plan.active_biological_ownership().unwrap();
    let parent = ownership.owner(NeuronId::new(1).unwrap()).unwrap().clone();
    let grown_id = NeuronId::new(99).unwrap();
    let mut destination = parent.clone();
    destination.neuron_id = grown_id;

    let commit = ownership
        .apply_transaction(BiologicalTopologyTransaction {
            base_topology_generation: ownership.topology_generation(),
            base_partition_generation: ownership.partition_generation(),
            effective_tag: aarnn_rust::deterministic::LogicalTag::new(12, 0),
            transfers: vec![BiologicalNeuronTransfer {
                neuron_id: grown_id,
                source: None,
                parent_neuron: Some(parent.neuron_id),
                origin: None,
                destination,
                state_digest: aarnn_rust::deterministic::StateDigest([1; 16]),
                synapse_ids: Vec::new(),
            }],
            base_growth_space_digest: None,
            growth_space: None,
        })
        .unwrap();

    assert_eq!(commit.ownership.neuron_count(), 6);
    assert_eq!(
        commit.ownership.owner(grown_id).unwrap().area_id,
        parent.area_id
    );
    assert_eq!(commit.topology_generation.raw(), 2);
    assert_eq!(commit.partition_generation.raw(), 2);
}

#[test]
fn boundary_crossing_releases_source_and_admits_destination_atomically() {
    let plan = plan_hierarchical_shards(request()).unwrap();
    let ownership = plan.active_biological_ownership().unwrap();
    let source = ownership.owner(NeuronId::new(1).unwrap()).unwrap().clone();
    let target = ownership.owner(NeuronId::new(4).unwrap()).unwrap().clone();
    let mut destination = target.clone();
    destination.neuron_id = source.neuron_id;

    let commit = ownership
        .apply_transaction(BiologicalTopologyTransaction {
            base_topology_generation: ownership.topology_generation(),
            base_partition_generation: ownership.partition_generation(),
            effective_tag: aarnn_rust::deterministic::LogicalTag::new(20, 0),
            transfers: vec![BiologicalNeuronTransfer {
                neuron_id: source.neuron_id,
                source: Some(source.clone()),
                parent_neuron: None,
                origin: None,
                destination: destination.clone(),
                state_digest: aarnn_rust::deterministic::StateDigest([2; 16]),
                synapse_ids: vec![],
            }],
            base_growth_space_digest: None,
            growth_space: None,
        })
        .unwrap();

    assert_eq!(commit.transfers.len(), 1);
    assert_eq!(commit.transfers[0].source.as_ref(), Some(&source));
    assert_eq!(commit.ownership.owner(source.neuron_id), Some(&destination));
    assert_eq!(
        commit
            .ownership
            .owners()
            .filter(|placement| placement.neuron_id == source.neuron_id)
            .count(),
        1,
        "a boundary crossing must not leave source and destination owners"
    );
    assert_eq!(commit.ownership.neuron_count(), ownership.neuron_count());
}

#[test]
fn new_growth_may_cross_only_with_explicit_migration_origin() {
    let plan = plan_hierarchical_shards(request()).unwrap();
    let ownership = plan.active_biological_ownership().unwrap();
    let parent = ownership.owner(NeuronId::new(1).unwrap()).unwrap().clone();
    let target = ownership.owner(NeuronId::new(4).unwrap()).unwrap().clone();
    let grown_id = NeuronId::new(100).unwrap();
    let mut destination = target.clone();
    destination.neuron_id = grown_id;

    let rejected = ownership.apply_transaction(BiologicalTopologyTransaction {
        base_topology_generation: ownership.topology_generation(),
        base_partition_generation: ownership.partition_generation(),
        effective_tag: aarnn_rust::deterministic::LogicalTag::new(21, 0),
        transfers: vec![BiologicalNeuronTransfer {
            neuron_id: grown_id,
            source: None,
            parent_neuron: Some(parent.neuron_id),
            origin: None,
            destination: destination.clone(),
            state_digest: aarnn_rust::deterministic::StateDigest([3; 16]),
            synapse_ids: vec![],
        }],
        base_growth_space_digest: None,
        growth_space: None,
    });
    assert!(rejected.is_err());
    assert!(ownership.owner(grown_id).is_none());

    let commit = ownership
        .apply_transaction(BiologicalTopologyTransaction {
            base_topology_generation: ownership.topology_generation(),
            base_partition_generation: ownership.partition_generation(),
            effective_tag: aarnn_rust::deterministic::LogicalTag::new(21, 0),
            transfers: vec![BiologicalNeuronTransfer {
                neuron_id: grown_id,
                source: None,
                parent_neuron: Some(parent.neuron_id),
                origin: Some(parent),
                destination,
                state_digest: aarnn_rust::deterministic::StateDigest([3; 16]),
                synapse_ids: vec![],
            }],
            base_growth_space_digest: None,
            growth_space: None,
        })
        .unwrap();
    assert_eq!(
        commit.ownership.owner(grown_id).unwrap().area_id,
        target.area_id
    );
}

fn biological_space() -> BiologicalGrowthSpace {
    BiologicalGrowthSpace::new(
        BiologicalMembrane {
            volume: BiologicalEllipsoid::new([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap(),
            maximum_radii: [3.0, 3.0, 3.0],
            maximum_expansion_per_transaction: 0.2,
            pressure_threshold: 2,
        },
        vec![
            BiologicalAreaVolume {
                area_id: 10,
                volume: BiologicalEllipsoid::new([0.0, 0.0, 0.0], [0.5, 0.5, 0.5]).unwrap(),
            },
            BiologicalAreaVolume {
                area_id: 20,
                volume: BiologicalEllipsoid::new([1.0, 0.0, 0.0], [0.3, 0.3, 0.3]).unwrap(),
            },
        ],
    )
    .unwrap()
}

#[test]
fn free_membrane_volume_expands_the_source_area_without_migration() {
    let space = biological_space();
    let decision = space.assess_growth(10, [0.0, 0.65, 0.0], 0.05, 0).unwrap();
    let BiologicalGrowthDecision::ExpandArea { area_id, volume } = &decision else {
        panic!("free volume should expand the source area: {decision:?}");
    };
    assert_eq!(*area_id, 10);
    assert!(volume.volume.radii[1] > 0.5);
    let expanded = space.apply_decision(&decision).unwrap();
    assert_ne!(space.digest(), expanded.digest());
}

#[test]
fn occupied_volume_of_another_area_requires_neuron_migration() {
    let space = biological_space();
    assert_eq!(
        space.assess_growth(10, [1.0, 0.0, 0.0], 0.05, 0).unwrap(),
        BiologicalGrowthDecision::MigrateToArea {
            from_area_id: 10,
            to_area_id: 20,
        }
    );
}

#[test]
fn loose_growth_is_projected_into_the_nearest_existing_area() {
    let space = biological_space();
    let decision = space.assess_growth(10, [1.3, 0.3, 0.0], 0.05, 0).unwrap();
    let BiologicalGrowthDecision::AttachToNearestArea { area_id, point } = decision else {
        panic!("loose growth should attach to the nearest area: {decision:?}");
    };
    assert_eq!(area_id, 20);
    let target = space
        .areas
        .iter()
        .find(|area| area.area_id == area_id)
        .unwrap();
    assert!(target.volume.contains_with_margin(point, 0.05));
}

#[test]
fn repeated_membrane_pressure_grows_the_enclosing_shape_in_bounded_steps() {
    let space = biological_space();
    assert_eq!(
        space.assess_growth(10, [2.1, 0.0, 0.0], 0.0, 1).unwrap(),
        BiologicalGrowthDecision::AwaitingMembranePressure {
            observed: 1,
            required: 2,
        }
    );
    let decision = space.assess_growth(10, [2.1, 0.0, 0.0], 0.0, 2).unwrap();
    let BiologicalGrowthDecision::ExpandMembrane { membrane } = &decision else {
        panic!("sustained pressure should expand the membrane: {decision:?}");
    };
    assert_eq!(membrane.volume.radii[0], 2.1);
    assert!(membrane.volume.radii[0] <= 2.0 + 0.2 + f32::EPSILON);
    let expanded = space.apply_decision(&decision).unwrap();
    assert_eq!(expanded.membrane.volume.radii[0], 2.1);
}

#[test]
fn membrane_expansion_and_neuron_admission_share_one_generation_boundary() {
    let plan = plan_hierarchical_shards(request()).unwrap();
    let ownership = plan.active_biological_ownership().unwrap();
    let space = biological_space();
    let decision = space.assess_growth(10, [2.1, 0.0, 0.0], 0.0, 2).unwrap();
    let next_space = space.apply_decision(&decision).unwrap();
    let parent = ownership.owner(NeuronId::new(1).unwrap()).unwrap().clone();
    let grown_id = NeuronId::new(101).unwrap();
    let mut destination = parent.clone();
    destination.neuron_id = grown_id;

    let commit = ownership
        .apply_transaction_with_space(
            BiologicalTopologyTransaction {
                base_topology_generation: ownership.topology_generation(),
                base_partition_generation: ownership.partition_generation(),
                effective_tag: aarnn_rust::deterministic::LogicalTag::new(22, 0),
                transfers: vec![BiologicalNeuronTransfer {
                    neuron_id: grown_id,
                    source: None,
                    parent_neuron: Some(parent.neuron_id),
                    origin: None,
                    destination,
                    state_digest: aarnn_rust::deterministic::StateDigest([4; 16]),
                    synapse_ids: vec![],
                }],
                base_growth_space_digest: Some(space.digest()),
                growth_space: Some(next_space.clone()),
            },
            Some(&space),
        )
        .unwrap();
    assert_eq!(commit.topology_generation.raw(), 2);
    assert_eq!(commit.growth_space, Some(next_space));
    assert!(commit.ownership.owner(grown_id).is_some());
}
