use aarnn_rust::deterministic::NeuronId;
use aarnn_rust::hierarchical_sharding::{
    HierarchicalShardingRequest, HostCapacity, LatencyMatrix, NetworkInteraction,
    NeuralNetworkInput, NeuralUnit, PhysicalNeuronLocation, area_layer_units,
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
