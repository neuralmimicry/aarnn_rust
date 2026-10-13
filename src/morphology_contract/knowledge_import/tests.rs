use super::*;
use crate::config::LIFParams;
use crate::knowledge::KnowledgeNeuron;
use crate::knowledge_region::{DenseIr, FfnMesh, FfnRegion, PopulationIr, UnitIr};
use crate::morphology_contract::PointOnlyConnection;

fn neuron(id: u64, x: f64, y: f64, role: ConnectomeRole) -> PointOnlyNeuron {
    PointOnlyNeuron {
        id: NeuronId::new(id).unwrap(),
        position_mm: Vec3 { x, y, z: 0.0 },
        role,
        layer: Some(0),
        cell_type: "fixture".into(),
        formation_order: id,
    }
}

fn fixtures() -> (PointOnlyConnectome, PointOnlyConnectome, AttachmentPolicy) {
    let target = PointOnlyConnectome {
        neurons: vec![
            neuron(1, 0.0, 0.0, ConnectomeRole::Output),
            neuron(2, -5.0, 0.0, ConnectomeRole::Hidden),
        ],
        connections: vec![PointOnlyConnection {
            id: 1,
            pre: NeuronId::new(2).unwrap(),
            post: NeuronId::new(1).unwrap(),
            kind: "existing".into(),
        }],
    };
    let source = PointOnlyConnectome {
        neurons: vec![
            neuron(10, 20.0, 0.0, ConnectomeRole::Sensory),
            neuron(11, 20.0, 0.25, ConnectomeRole::Sensory),
            neuron(12, 22.0, 0.0, ConnectomeRole::Output),
        ],
        connections: vec![PointOnlyConnection {
            id: 10,
            pre: NeuronId::new(10).unwrap(),
            post: NeuronId::new(12).unwrap(),
            kind: "imported".into(),
        }],
    };
    let policy = AttachmentPolicy {
        maximum_contact_distance_mm: 1.0,
        minimum_soma_separation_mm: 0.125,
        maximum_neuron_records: 64,
        maximum_candidates: 6,
        maximum_distance_checks: 1024,
    };
    (target, source, policy)
}

fn existing(connectome: &PointOnlyConnectome) -> ExistingSnn<'_> {
    ExistingSnn {
        brain_id: BrainId::new(1).unwrap(),
        topology_generation: TopologyGeneration::INITIAL,
        connectome,
    }
}

fn mesh() -> FfnMesh {
    let code = PopulationIr {
        tonic: 0.0,
        units: vec![],
        rheobase: 1.0,
        max_rate: 1.0,
    };
    FfnMesh {
        name: "fixture".into(),
        neuron: KnowledgeNeuron::Lif(LIFParams::default()),
        noise_std: 0.0,
        steps: 10,
        warmup: 0,
        gate: DenseIr {
            inputs: 2,
            outputs: 1,
            weights_file: "gate.f32".into(),
        },
        up: DenseIr {
            inputs: 2,
            outputs: 1,
            weights_file: "up.f32".into(),
        },
        down: DenseIr {
            inputs: 1,
            outputs: 1,
            weights_file: "down.f32".into(),
        },
        gate_code: code.clone(),
        up_code: code,
        up_scale_file: None,
        input_placement_policy: KnowledgeInputPlacementPolicy::default(),
    }
}

#[test]
fn every_sensory_input_is_near_an_existing_output_and_both_networks_are_preserved() {
    let (target, source, policy) = fixtures();
    let before = target.clone();
    let source_before = source.clone();
    let plan = mesh()
        .plan_attachment(existing(&target), &source, policy.clone())
        .unwrap();
    assert_eq!(plan.sensory_contacts.len(), 2);
    let combined = plan
        .prepare_connectome(existing(&target), &source, &policy)
        .unwrap();
    assert_eq!(target, before);
    assert_eq!(source, source_before);
    assert_eq!(&combined.neurons[..target.neurons.len()], target.neurons);
    let imported = &combined.neurons[target.neurons.len()..];
    for (i, a) in imported.iter().enumerate() {
        assert_eq!(a.id, source.neurons[i].id);
        assert_eq!(a.role, source.neurons[i].role);
        assert_eq!(a.cell_type, source.neurons[i].cell_type);
        for (j, b) in imported.iter().enumerate() {
            assert_eq!(
                a.position_mm.distance(b.position_mm),
                source.neurons[i]
                    .position_mm
                    .distance(source.neurons[j].position_mm)
            );
        }
        for b in &target.neurons {
            assert!(a.position_mm.distance(b.position_mm) >= policy.minimum_soma_separation_mm);
        }
    }
    for contact in &plan.sensory_contacts {
        let input = imported.iter().find(|n| n.id == contact.sensory).unwrap();
        let output = target
            .neurons
            .iter()
            .find(|n| n.id == contact.existing_output)
            .unwrap();
        assert_eq!(input.role, ConnectomeRole::Sensory);
        assert_eq!(output.role, ConnectomeRole::Output);
        assert_eq!(
            input.position_mm.distance(output.position_mm),
            contact.distance_mm
        );
        assert!(contact.distance_mm <= policy.maximum_contact_distance_mm);
    }
    assert_eq!(
        combined.connections,
        target
            .connections
            .iter()
            .chain(&source.connections)
            .cloned()
            .collect::<Vec<_>>()
    );
}

#[test]
fn permutation_independent_plan_and_replay() {
    let (mut target, mut source, policy) = fixtures();
    let plan = plan_attachment(existing(&target), &source, policy.clone()).unwrap();
    target.neurons.reverse();
    source.neurons.reverse();
    assert_eq!(
        plan,
        plan_attachment(existing(&target), &source, policy.clone()).unwrap()
    );
    let encoded = serde_json::to_vec(&plan).unwrap();
    let replay: KnowledgeAttachmentPlan = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(replay, plan);
    assert!(
        replay
            .prepare_connectome(existing(&target), &source, &policy)
            .is_ok()
    );
}

#[test]
fn absent_sensory_or_output_refuses_import() {
    let (mut target, mut source, policy) = fixtures();
    target.neurons[0].role = ConnectomeRole::Hidden;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::MissingInterface)
    ));
    target.neurons[0].role = ConnectomeRole::Output;
    for n in &mut source.neurons {
        n.role = ConnectomeRole::Hidden;
    }
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::MissingInterface)
    ));
}

#[test]
fn identity_and_connection_collisions_are_rejected() {
    let (target, mut source, policy) = fixtures();
    source.neurons[1].id = target.neurons[0].id;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::IdentityCollision)
    ));
    source.neurons[1].id = NeuronId::new(11).unwrap();
    source.connections[0].id = target.connections[0].id;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::IdentityCollision)
    ));
}

#[test]
fn invalid_and_nonfinite_policy_refuses_import() {
    let (target, source, policy) = fixtures();
    for separation in [0.0, -1.0, f64::NAN, f64::INFINITY, 2.0] {
        let mut p = policy.clone();
        p.minimum_soma_separation_mm = separation;
        assert!(matches!(
            plan_attachment(existing(&target), &source, p),
            Err(AttachmentError::InvalidPolicy)
        ));
    }
    for radius in [-1.0, 0.0, 0.125, f64::NAN, f64::INFINITY] {
        let mut p = policy.clone();
        p.maximum_contact_distance_mm = radius;
        assert!(matches!(
            plan_attachment(existing(&target), &source, p),
            Err(AttachmentError::InvalidPolicy)
        ));
    }
}

#[test]
fn malformed_geometry_and_oversized_metadata_are_rejected() {
    let (target, mut source, policy) = fixtures();
    source.neurons[0].position_mm.x = f64::NAN;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::Geometry(_))
    ));
    source.neurons[0].position_mm.x = 20.0;
    source.connections[0].kind = "x".repeat(257);
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::Geometry(_))
    ));
}

#[test]
fn record_and_work_budgets_are_enforced() {
    let (target, source, mut policy) = fixtures();
    policy.maximum_neuron_records = 4;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::BudgetExceeded)
    ));
    policy.maximum_neuron_records = 64;
    policy.maximum_distance_checks = 1;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::BudgetExceeded)
    ));
}

#[test]
fn overlap_and_all_sensory_proximity_are_mandatory() {
    let (target, mut source, policy) = fixtures();
    source.neurons[1].position_mm = source.neurons[0].position_mm;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::SourceOverlap)
    ));
    source.neurons[1].position_mm.y = 100.0;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::NoPlacement)
    ));
}

#[test]
fn occupied_candidate_is_skipped_without_moving_the_target() {
    let (mut target, source, mut policy) = fixtures();
    target
        .neurons
        .push(neuron(3, 0.5625, 0.0, ConnectomeRole::Hidden));
    policy.maximum_candidates = 1;
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::BudgetExceeded)
    ));
    policy.maximum_candidates = 6;
    let plan = plan_attachment(existing(&target), &source, policy).unwrap();
    assert!(source.neurons[0].position_mm.add(plan.translation_mm).x < 0.0);
}

#[test]
fn floating_point_translation_cannot_collapse_or_warp_source_geometry() {
    let (mut target, source, policy) = fixtures();
    target.neurons[0].position_mm = Vec3 {
        x: 1.0e16,
        y: 1.0e16,
        z: 1.0e16,
    };
    assert!(matches!(
        plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::NoPlacement)
    ));
}

#[test]
fn replay_rejects_stale_generation_identity_target_source_and_policy() {
    let (target, source, policy) = fixtures();
    let plan = plan_attachment(existing(&target), &source, policy.clone()).unwrap();
    let mut context = existing(&target);
    context.topology_generation = TopologyGeneration::new(2).unwrap();
    assert!(matches!(
        plan.prepare_connectome(context, &source, &policy),
        Err(AttachmentError::StalePlan)
    ));
    let mut context = existing(&target);
    context.brain_id = BrainId::new(2).unwrap();
    assert!(matches!(
        plan.prepare_connectome(context, &source, &policy),
        Err(AttachmentError::StalePlan)
    ));
    let mut moved = target.clone();
    moved.neurons[0].position_mm.y = 0.5;
    assert!(matches!(
        plan.prepare_connectome(existing(&moved), &source, &policy),
        Err(AttachmentError::StalePlan)
    ));
    let mut changed = source.clone();
    changed.connections[0].kind = "changed".into();
    assert!(matches!(
        plan.prepare_connectome(existing(&target), &changed, &policy),
        Err(AttachmentError::StalePlan)
    ));
    let mut p = policy.clone();
    p.maximum_contact_distance_mm = 2.0;
    assert!(matches!(
        plan.prepare_connectome(existing(&target), &source, &p),
        Err(AttachmentError::StalePlan)
    ));
}

#[test]
fn replay_revalidates_forged_geometry_and_contact_witnesses() {
    let (target, mut source, policy) = fixtures();
    let plan = plan_attachment(existing(&target), &source, policy.clone()).unwrap();
    let mut forged = plan.clone();
    forged.sensory_contacts.pop();
    assert!(matches!(
        forged.prepare_connectome(existing(&target), &source, &policy),
        Err(AttachmentError::StalePlan)
    ));
    let mut forged = plan.clone();
    forged.translation_mm = Vec3::ZERO;
    assert!(matches!(
        forged.prepare_connectome(existing(&target), &source, &policy),
        Err(AttachmentError::StalePlan)
    ));
    let mut forged = plan.clone();
    forged.translation_mm.x = f64::INFINITY;
    assert!(matches!(
        forged.prepare_connectome(existing(&target), &source, &policy),
        Err(AttachmentError::StalePlan)
    ));
    source.neurons[1].position_mm = source.neurons[0].position_mm;
    let mut forged = plan;
    forged.source_digest = digest(&source).unwrap();
    assert!(matches!(
        forged.prepare_connectome(existing(&target), &source, &policy),
        Err(AttachmentError::SourceOverlap)
    ));
}

#[test]
fn converted_mesh_interface_must_match_before_planning() {
    let (target, source, policy) = fixtures();
    let mut mesh = mesh();
    mesh.gate.inputs = 3;
    assert!(matches!(
        mesh.plan_attachment(existing(&target), &source, policy.clone()),
        Err(AttachmentError::InterfaceMismatch)
    ));
    mesh.gate.inputs = 2;
    mesh.down.outputs = 2;
    assert!(matches!(
        mesh.plan_attachment(existing(&target), &source, policy),
        Err(AttachmentError::InterfaceMismatch)
    ));
}

#[test]
fn legacy_mesh_defaults_to_adjacency_and_unknown_policies_are_rejected() {
    let mut json = serde_json::to_value(mesh()).unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("input_placement_policy");
    let legacy: FfnMesh = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        legacy.input_placement_policy,
        KnowledgeInputPlacementPolicy::SensoryNearExistingOutputs
    );
    json["input_placement_policy"] = "far_from_outputs".into();
    assert!(serde_json::from_value::<FfnMesh>(json).is_err());
}

#[test]
fn loaded_legacy_region_keeps_numerical_evaluation_and_explicit_planning() {
    let dir = std::env::temp_dir().join(format!("aarnn-adjacency-region-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    let mut mesh = mesh();
    mesh.gate_code.tonic = 1.0;
    mesh.gate_code.units.push(UnitIr {
        knot: 0.0,
        direction: 1.0,
        gain: 1.0,
        weight: 1.0,
    });
    mesh.up_code = mesh.gate_code.clone();
    for (name, weights) in [
        ("gate.f32", vec![0.5f32, 0.25]),
        ("up.f32", vec![0.125, 0.5]),
        ("down.f32", vec![2.0]),
    ] {
        std::fs::write(
            dir.join(name),
            weights
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    let mut json = serde_json::to_value(&mesh).unwrap();
    std::fs::write(dir.join("new.json"), serde_json::to_vec(&json).unwrap()).unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("input_placement_policy");
    std::fs::write(dir.join("old.json"), serde_json::to_vec(&json).unwrap()).unwrap();
    let current = FfnRegion::load(&dir.join("new.json")).unwrap();
    let legacy = FfnRegion::load(&dir.join("old.json")).unwrap();
    let (result, stats) = current.run(&[0.5, 0.25], 1).unwrap();
    let (old_result, old_stats) = legacy.run(&[0.5, 0.25], 1).unwrap();
    assert_eq!(result, old_result);
    assert!(result[0] >= 2.0);
    assert_eq!(stats.neuron_steps, old_stats.neuron_steps);
    assert_eq!(stats.neurons, old_stats.neurons);
    let (target, source, policy) = fixtures();
    let before = target.clone();
    let plan = legacy
        .plan_attachment(existing(&target), &source, policy.clone())
        .unwrap();
    assert!(
        plan.prepare_connectome(existing(&target), &source, &policy)
            .is_ok()
    );
    assert_eq!(target, before);
}
