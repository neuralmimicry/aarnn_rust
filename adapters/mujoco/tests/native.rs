#![cfg(feature = "native")]
use aarnn_mujoco::{aer::Model, native::Physics, world::World};
use std::path::PathBuf;
fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sim/content/embodiments/generated")
}
#[test]
fn all_twenty_eight_native_models_and_reset_snapshots() {
    let mut count = 0;
    for entry in std::fs::read_dir(directory()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "xml") {
            continue;
        }
        let record = aarnn_mujoco::schema::load_model(&path).unwrap();
        let mut world = World::load(&path, record).unwrap();
        for _ in 0..100 {
            world.advance().unwrap();
        }
        assert!(
            world.physics.qpos().iter().all(|v| v.is_finite()),
            "{}",
            path.display()
        );
        let q = world.physics.qpos().to_vec();
        let snapshot = world.physics.snapshot();
        world.physics.step();
        world.physics.restore(&snapshot).unwrap();
        assert_eq!(world.physics.qpos(), q);
        world.reset();
        assert_eq!(world.step, 0);
        assert_eq!(world.reset_epoch, 1);
        assert!(world.disarmed);
        assert!(world.actuation_inhibited);
        count += 1;
    }
    assert_eq!(
        count, 28,
        "generate all profiles before the required native lane"
    );
}
#[test]
fn checkpoint_identity_tamper_and_disconnected_restore() {
    let path = directory().join("crow_corvus_corone.xml");
    let mut world = World::load(&path, aarnn_mujoco::schema::load_model(&path).unwrap()).unwrap();
    for _ in 0..12 {
        world.advance().unwrap();
    }
    let saved = world.checkpoint();
    let mut bad = saved.clone();
    bad.io_digest = "wrong-mapping".into();
    assert!(world.restore(&bad).is_err());
    assert_eq!(world.step, 12);
    bad = saved.clone();
    bad.instance_id = "another-instance".into();
    assert!(world.restore(&bad).is_err());
    bad = saved.clone();
    bad.energy_j = f64::NAN;
    assert!(world.restore(&bad).is_err());
    let q = world.physics.qpos().to_vec();
    world.advance().unwrap();
    world.restore(&saved).unwrap();
    assert_eq!(world.physics.qpos(), q);
    assert_eq!(world.step, 12);
    assert_eq!(world.reset_epoch, 1);
    assert!(world.client.is_none());
    assert!(world.disarmed && world.actuation_inhibited);
    assert!(world.arm().is_err());
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
    record["io"]["sensors"][0]["name"] = "tampered-channel".into();
    assert!(aarnn_mujoco::schema::verify_record(&record).is_err());
}
#[test]
fn free_fall_bounds_and_exact_abi_ownership() {
    let path = std::env::temp_dir().join(format!("aarnn-rust-fall-{}.xml", std::process::id()));
    std::fs::write(&path,r#"<mujoco><option timestep="0.002" gravity="0 0 -9.81"/><worldbody><body name="ball" pos="0 0 3"><freejoint/><geom type="sphere" size="0.05" mass="0.51"/></body></worldbody></mujoco>"#).unwrap();
    for _ in 0..8 {
        let mut physics = Physics::load(&path).unwrap();
        assert_eq!(physics.dimensions(), (7, 6, 0));
        physics.disarm();
        for _ in 0..20 {
            physics.step();
        }
        assert!(physics.qpos()[2] < 2.996);
        assert!(physics.set_control(0, 1.).is_err());
        assert!(physics.body("absent").is_err());
        assert!(physics.restore(&[f64::NAN]).is_err());
        physics.reset();
        assert_eq!(physics.qpos()[2], 3.);
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn own_model_measurements_and_projection_gates() {
    for profile in [
        "galbot_g1_golf",
        "ugot_mecanum",
        "crow_corvus_corone",
        "celegans",
        "hexapod",
        "drosophila_banc",
        "drosophila_fafb",
        "nao",
        "zebrafish",
    ] {
        let path = directory().join(format!("{profile}.xml"));
        let record: Model =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        let expected = record.io.sensors.len();
        let mut world = World::load(&path, record).unwrap();
        let before = world.sensors().unwrap();
        assert_eq!(before.len(), expected);
        for _ in 0..30 {
            world.advance().unwrap();
        }
        let after = world.sensors().unwrap();
        assert!(
            after
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
        );
        assert_ne!(
            before, after,
            "actual body measurements must change as passive dynamics evolves"
        );
    }
}

#[test]
fn legacy_named_readouts_cover_outputs_silence_reset_and_joint_response() {
    use aarnn_embodiment::{actuation::Actuation, aer::Reply};
    for profile in [
        "hexapod",
        "drosophila_banc",
        "drosophila_fafb",
        "nao",
        "zebrafish",
    ] {
        let path = directory().join(format!("{profile}.xml"));
        let model = aarnn_mujoco::schema::load_model(&path).unwrap();
        let mut covered = std::collections::HashSet::new();
        for a in &model.actuators {
            for c in &a.readout {
                covered.insert(c.channel);
            }
        }
        if profile == "zebrafish" {
            covered.insert(31);
        }
        assert_eq!(
            covered.len(),
            model.io.output_names.len(),
            "{profile}: unmapped output"
        );
        let mut state = Actuation::new(&model);
        let empty = Reply {
            source_step: 0,
            timestamp_us: 0,
            spikes: vec![0; model.io.output_names.len()],
        };
        state.accept(0, &empty).unwrap();
        state.advance(0);
        assert!(
            (0..model.actuators.len()).all(|i| !state.active(i)),
            "{profile}: silent drive"
        );
        // A named pulse must drive exactly its declared readout, never the next
        // motor by vector position. Repeat across every output including paired
        // signs and fish stiffness. Each instance/filter starts independently.
        for channel in 0..model.io.output_names.len() {
            let mut single = Actuation::new(&model);
            let mut spikes = empty.spikes.clone();
            spikes[channel] = 1;
            single
                .accept(
                    1,
                    &Reply {
                        source_step: 1,
                        timestamp_us: 2000,
                        spikes,
                    },
                )
                .unwrap();
            single.advance(1);
            assert!(
                (0..model.actuators.len()).any(|i| single.active(i)),
                "{profile} output {channel}"
            );
            single.advance(52);
            assert!(single.disarmed);
            single.reset();
            assert!(single.actuation_inhibited);
        }
        let mut world = World::load(&path, model).unwrap();
        let (q, _) = world
            .physics
            .joint(&world.model.actuators[0].joint)
            .unwrap();
        let initial = world.physics.qpos()[q];
        let a = world.model.actuators[0].clone();
        // Direct diagnostic servo impulse verifies the actual physical joint,
        // separately from the neural/projection tests above.
        for _ in 0..50 {
            world.physics.control(0, a.control_range[1], true).unwrap();
            world.physics.step();
        }
        assert!(
            (world.physics.qpos()[q] - initial).abs() > 1e-5,
            "{profile}: no physical joint response"
        );
        world.reset();
        assert!(!world.physics.has_active_effort());
        assert_eq!(world.sensors().unwrap().len(), world.model.io.sensors.len());
    }
}

#[test]
fn native_foot_contacts_and_fish_displacement_are_physical() {
    let path = directory().join("hexapod.xml");
    let mut world = World::load(&path, aarnn_mujoco::schema::load_model(&path).unwrap()).unwrap();
    for _ in 0..500 {
        world.advance().unwrap();
    }
    let sensors = world.sensors().unwrap();
    assert!(
        sensors[18..24].iter().any(|v| *v > 0.),
        "foot sensors never detect floor"
    );
    let path = directory().join("zebrafish.xml");
    let mut fish = World::load(&path, aarnn_mujoco::schema::load_model(&path).unwrap()).unwrap();
    let start = fish.physics.qpos()[2];
    for _ in 0..300 {
        fish.advance().unwrap();
    }
    assert!(
        (fish.physics.qpos()[2] - start).abs() < 0.03,
        "neutral displaced water must support mass without motor drive"
    );
    assert!(!fish.physics.has_active_effort());
    assert_eq!(fish.npc_steps, 0);
}

#[test]
fn celegans_named_muscle_effort_silence_and_proper_acceleration() {
    use aarnn_mujoco::aer::Reply;
    let path = directory().join("celegans.xml");
    let mut world = World::load(&path, aarnn_mujoco::schema::load_model(&path).unwrap()).unwrap();
    let mut state = aarnn_embodiment::actuation::Actuation::new(&world.model);
    for step in 0..1000 {
        world.advance().unwrap();
        state
            .accept(
                step,
                &Reply {
                    source_step: step,
                    timestamp_us: step * 2000,
                    spikes: vec![0; 96],
                },
            )
            .unwrap();
        state.advance(step);
    }
    assert!(state.controls(&world.model).iter().all(|v| *v == 0.));
    assert!(!world.physics.has_active_effort());
    let sensors = world.sensors().unwrap();
    assert!((sensors[0] - 0.5).abs() < 0.001);
    assert!(
        (sensors[1] - (9.81 + 20.) / 40.).abs() < 0.001,
        "proper acceleration {:?}",
        &sensors[..3]
    );
    assert_eq!(sensors[6], 0.);
    assert_eq!(sensors[7], 0.);
    let mut spikes = vec![0; 96];
    spikes[world
        .model
        .io
        .output_names
        .iter()
        .position(|n| n.ends_with("MDL12"))
        .unwrap()] = 1;
    let (joint, _) = world.physics.joint("celegans_spine_12").unwrap();
    let initial = world.physics.qpos()[joint];
    for step in 1000..1400 {
        state
            .accept(
                step,
                &Reply {
                    source_step: step,
                    timestamp_us: step * 2000,
                    spikes: spikes.clone(),
                },
            )
            .unwrap();
        state.advance(step);
        for (i, target) in state
            .controls(&world.model)
            .iter()
            .take(world.model.actuators.len())
            .enumerate()
        {
            world.physics.control(i, *target, state.active(i)).unwrap();
        }
        world.physics.step();
    }
    assert!(world.physics.qpos()[joint] < initial - 1e-3);
    assert_eq!(state.npc_steps, 0);
    world.reset();
    assert!(!world.physics.has_active_effort());
    state.stop();
    assert!(state.controls(&world.model).iter().all(|v| *v == 0.));
}

#[test]
fn no_crow_metabolism_is_invented_for_commercial_batteries() {
    let path = directory().join("ugot_mecanum.xml");
    let mut world = World::load(&path, aarnn_mujoco::schema::load_model(&path).unwrap()).unwrap();
    let initial = world.energy_j;
    for _ in 0..200 {
        world.advance().unwrap();
    }
    assert_eq!(world.energy_j, initial);
}

#[test]
fn all_profiles_have_explicit_npc_mechanics_and_stop_clears_drives() {
    for entry in std::fs::read_dir(directory()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "xml") {
            continue;
        }
        let mut world =
            World::load(&path, aarnn_mujoco::schema::load_model(&path).unwrap()).unwrap();
        world.npc_enabled = true;
        for _ in 0..20 {
            world.advance().unwrap();
        }
        assert_eq!(world.npc_steps, 20);
        assert_eq!(world.applied, 0);
        assert_eq!(world.active_steps, 0);
        assert!(world.physics.qpos().iter().all(|v| v.is_finite()));
        world.stop();
        for _ in 0..3 {
            world.advance().unwrap();
        }
        assert!(world.disarmed && world.actuation_inhibited);
        assert!(!world.physics.has_active_effort());
        assert_eq!(world.npc_steps, 20);
    }
}
