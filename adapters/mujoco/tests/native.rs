#![cfg(feature = "native")]
// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
use aarnn_mujoco::{aer::Model, native::Physics, world::World};
use std::path::PathBuf;
fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sim/content/embodiments/generated")
}
#[test]
fn all_twenty_nine_native_models_and_reset_snapshots() {
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
        count, 29,
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
        "salamander_crawler",
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
        "salamander_crawler",
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
            ..Default::default()
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
                        ..Default::default()
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
                    ..Default::default()
                },
            )
            .unwrap();
        state.advance(step);
    }
    assert!(state.controls(&world.model).iter().all(|v| *v == 0.));
    assert!(!world.physics.has_active_effort());
    let sensors = world.sensors().unwrap();
    // v2 vibration is a rectified high-pass of proper acceleration: the
    // static 1 g reference of a resting worm no longer drives the touch cells.
    for (i, v) in sensors[..6].iter().enumerate() {
        assert!(*v < 0.01, "resting inertial channel {i} = {v}");
    }
    assert_eq!(sensors[6], 0.);
    assert_eq!(sensors[7], 0.);
    // A straight body reports no bend on any of the 24 proprioceptive channels.
    // Channels 24..48: the core proprioceptive contract (the optional
    // energy-needs group follows it).
    assert!(
        sensors[24..48].iter().all(|v| *v < 0.05),
        "{:?}",
        &sensors[24..48]
    );
    let mut spikes = vec![0; 96];
    spikes[world
        .model
        .io
        .output_names
        .iter()
        .position(|n| n.ends_with("MDL12"))
        .unwrap()] = 1;
    // v3 rows run head to tail: row 12 spans the hinges on segments 12 and
    // 13 (celegans_spine_13 and _14).
    let (joint, _) = world.physics.joint("celegans_spine_13").unwrap();
    let initial = world.physics.qpos()[joint];
    for step in 1000..1400 {
        state
            .accept(
                step,
                &Reply {
                    source_step: step,
                    timestamp_us: step * 2000,
                    spikes: spikes.clone(),
                    ..Default::default()
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
    // The dorsal bend is sensed on the dorsal bend channel nearest that hinge
    // (body fraction (23.5 - 12) / 24 = 0.48, i.e. sample 6), not the ventral.
    let bent = world.sensors().unwrap();
    let names: Vec<&str> = world
        .model
        .io
        .sensors
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let dorsal = names
        .iter()
        .position(|n| n.ends_with("bend_dorsal06"))
        .unwrap();
    let ventral = names
        .iter()
        .position(|n| n.ends_with("bend_ventral06"))
        .unwrap();
    assert!(
        bent[dorsal] > 0.02 && bent[ventral] == 0.,
        "dorsal {} ventral {}",
        bent[dorsal],
        bent[ventral]
    );
    assert_eq!(state.npc_steps, 0);
    world.reset();
    assert!(!world.physics.has_active_effort());
    state.stop();
    assert!(state.controls(&world.model).iter().all(|v| *v == 0.));
}

#[test]
fn celegans_trace_and_passive_observation_never_perturb_physics() {
    let path = directory().join("celegans.xml");
    let record = aarnn_mujoco::schema::load_model(&path).unwrap();
    let mut observed = World::load(&path, record.clone()).unwrap();
    let mut plain = World::load(&path, record).unwrap();
    let header = observed.trace_header();
    assert_eq!(header["segment_body_ids"].as_array().unwrap().len(), 24);
    // 48 core channels plus the optional energy-needs group (4).
    assert_eq!(header["sensor_names"].as_array().unwrap().len(), 52);
    for _ in 0..500 {
        observed.advance().unwrap();
        plain.advance().unwrap();
        observed.observe_unbound().unwrap();
        let sample = observed.trace_record().unwrap();
        assert_eq!(sample["sensor_source"], "passive-observation");
        assert_eq!(sample["sensors"].as_array().unwrap().len(), 52);
        assert_eq!(sample["joint_q"].as_array().unwrap().len(), 23);
        assert_eq!(
            sample["qpos"].as_array().unwrap().len(),
            observed.physics.qpos().len()
        );
    }
    assert_eq!(observed.physics.snapshot(), plain.physics.snapshot());
    assert!(observed.applied_spikes.iter().all(|n| *n == 0));
    observed.reset();
    assert!(observed.last_sensors.is_empty());
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

#[test]
fn celegans_stimulus_fields_follow_the_habitat() {
    use aarnn_embodiment::aer::field_value;
    let path = directory().join("celegans.xml");
    let record = aarnn_mujoco::schema::load_model(&path).unwrap();
    let scene = record.scene.clone().unwrap();
    assert!(scene.resistive_force.is_some() && scene.taps.is_some());
    let s = &scene.stimuli;
    // Food odour peaks on the largest lawn and decays away from it.
    let lawn = s
        .iter()
        .find(|x| x.field == "attractant_volatile")
        .unwrap()
        .center;
    let on = field_value(s, "attractant_volatile", lawn[0], lawn[1]);
    let off = field_value(s, "attractant_volatile", -0.55, 0.55);
    let between = field_value(s, "attractant_volatile", 0.5 * lawn[0], 0.5 * lawn[1]);
    assert!(
        on > 0.7 && off < 0.1 * on && between > off && between < on,
        "on {on} between {between} off {off}"
    );
    // O2 is lower inside the lawn than in open air.
    assert!(field_value(s, "oxygen", lawn[0], lawn[1]) < field_value(s, "oxygen", -0.55, 0.55));
    // The plate is warmer towards +x; the warm patch is warmer still.
    assert!(field_value(s, "temperature", 0.3, -0.5) > field_value(s, "temperature", -0.3, -0.5));
    // Every field stays normalised everywhere on the plate.
    for field in [
        "light",
        "temperature",
        "attractant_soluble",
        "repellent_soluble",
        "attractant_volatile",
        "oxygen",
    ] {
        for i in 0..27 {
            for j in 0..27 {
                let (x, y) = (-0.65 + 0.05 * i as f64, -0.65 + 0.05 * j as f64);
                let v = field_value(s, field, x, y);
                assert!(v.is_finite() && (0. ..=1.).contains(&v));
            }
        }
    }
    // A passive worm under substrate drag stays put; taps register on the
    // vibration channels only while a pulse is active.
    let mut world = World::load(&path, record).unwrap();
    let start = world.physics.qpos()[..2].to_vec();
    let mut tapped = 0.0f64;
    for _ in 0..4500 {
        world.advance().unwrap();
        if scene.taps.as_ref().unwrap().active(world.physics.time()) {
            let v = world.sensors().unwrap();
            tapped = tapped.max(v[0].max(v[1]).max(v[2]));
        }
    }
    assert!(tapped > 0.2, "plate tap vibration {tapped}");
    let end = &world.physics.qpos()[..2];
    let drift = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
    assert!(drift < 0.03, "passive drift {drift} m");
}

/// The calibrated body crawls under a biological travelling wave: a 0.5 Hz,
/// 0.65 body-length head-to-tail wave moves it forward, the reversed wave
/// backward (resistive-force substrate, mechanics v3).
#[test]
fn celegans_travelling_wave_crawls_in_the_wave_direction() {
    let path = directory().join("celegans.xml");
    let speed = |direction: f64| {
        let record = aarnn_mujoco::schema::load_model(&path).unwrap();
        let mut world = World::load(&path, record).unwrap();
        let hinges: Vec<(usize, f64)> = world
            .model
            .actuators
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let n: f64 = a
                    .joint
                    .strip_prefix("celegans_spine_")
                    .unwrap()
                    .parse()
                    .unwrap();
                (i, (23.5 - (n - 1.)) / 24.)
            })
            .collect();
        let head = world.physics.body("segment_23").unwrap();
        let tail = world.physics.body("segment_00").unwrap();
        let axis = {
            let (h, _, _) = world.physics.body_state(head).unwrap();
            let (t, _, _) = world.physics.body_state(tail).unwrap();
            let v = [h[0] - t[0], h[1] - t[1]];
            let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
            [v[0] / n, v[1] / n]
        };
        let centroid = |w: &World| {
            let mut c = [0.; 2];
            for b in 1..25 {
                let (p, _, _) = w.physics.body_state(b).unwrap();
                c[0] += p[0] / 24.;
                c[1] += p[1] / 24.;
            }
            c
        };
        let start = centroid(&world);
        let steps = 4000;
        for k in 0..steps {
            let t = k as f64 * 0.002;
            let ramp = (t / 1.0).min(1.0);
            for &(i, ap) in &hinges {
                let target =
                    ramp * 0.3 * (std::f64::consts::TAU * (0.5 * t - direction * ap / 0.65)).sin();
                world.physics.control(i, target, true).unwrap();
            }
            world.physics.clear_forces();
            world.celegans_substrate_forces().unwrap();
            world.physics.step();
        }
        let end = centroid(&world);
        ((end[0] - start[0]) * axis[0] + (end[1] - start[1]) * axis[1])
            / 0.34
            / (steps as f64 * 0.002)
    };
    let forward = speed(1.0);
    let backward = speed(-1.0);
    assert!(forward > 0.08, "head-to-tail wave speed {forward} L/s");
    assert!(backward < -0.08, "tail-to-head wave speed {backward} L/s");
}

/// Fly mechanics v3 (`resonant-flight-v1`): the wings fold and the fly
/// stands without a brain; graded pulse-density commands on the record's
/// flight readouts (decoded by the shared actuation) beat both wings in phase
/// at the declared resonance and lift the fly off. Exercises the record's
/// readouts, the shared decoder, the crate motor and the declared blade
/// panels on the native physics, as `World::advance` applies them.
#[test]
fn fly_flight_motor_lifts_off_only_on_a_neural_command() {
    use aarnn_embodiment::{
        actuation::Actuation,
        aer::{OutputCode, Reply},
        flight::COMMANDS,
        force::polar_panel_force,
    };
    let path = directory().join("drosophila_banc.xml");
    let model = aarnn_mujoco::schema::load_model(&path).unwrap();
    let flight = model.flight.clone().expect("fly declares a flight motor");
    flight.validate().unwrap();
    // No brain: the thoracic spring holds both wings folded back and the
    // fly stays on its feet.
    let mut world = World::load(&path, model.clone()).unwrap();
    for _ in 0..500 {
        world.advance().unwrap();
    }
    let fold = |p: &Physics, side: &str| p.qpos()[p.joint(&format!("{side}_wing_flap")).unwrap().0];
    for side in ["left", "right"] {
        assert!(
            (fold(&world.physics, side) - flight.fold_rad).abs() < 0.05,
            "{side} wing not folded"
        );
    }
    let standing = world.physics.qpos()[2];
    assert!(standing < 0.06 && !world.physics.has_active_effort());
    // Programmed brain: power 1, amplitude 0.85 (1.02 rad; the 10.3 g fly
    // hovers near 0.85 rad), centred strokes, as sigma-delta pulse densities
    // on the readouts.
    let aero = model.scene.as_ref().unwrap().aerodynamics.clone().unwrap();
    let mut physics = Physics::load(&path).unwrap();
    let panels: Vec<_> = aero
        .panels
        .iter()
        .map(|p| {
            (
                physics.body(&p.body).unwrap(),
                p.point_m,
                p.area_m2,
                aero.polars[&p.polar].coefficients(),
            )
        })
        .collect();
    let joints: Vec<[(usize, usize); 2]> = flight
        .wings
        .iter()
        .map(|w| {
            [
                physics.joint(&w.stroke_joint).unwrap(),
                physics.joint(&w.pitch_joint).unwrap(),
            ]
        })
        .collect();
    let mut state = Actuation::new(&model);
    state.arm();
    let density = [1.0, 0.85, 0.85, 0.5, 0.5];
    let mut phase = vec![0.0; model.io.output_names.len()];
    let mut stroke = Vec::new();
    for step in 1..=600u64 {
        let mut drive = vec![0.0; phase.len()];
        for (command, d) in flight.commands.iter().zip(density) {
            // Signed centre readouts: equal densities on both channels.
            for c in &command.readout {
                drive[c.channel] = d;
            }
        }
        let spikes = phase
            .iter_mut()
            .zip(&drive)
            .map(|(p, d)| {
                *p += d;
                let spike = u32::from(*p >= 1. - 1e-12);
                *p -= f64::from(spike);
                spike
            })
            .collect();
        let reply = Reply {
            source_step: step,
            timestamp_us: step * 2000,
            spikes,
            output_code: OutputCode::PulseDensity,
        };
        state
            .select(&model, step, false, true, Some(&reply), None)
            .unwrap();
        assert_eq!(state.commands().len(), COMMANDS.len());
        for (i, target) in state
            .controls(&model)
            .iter()
            .take(model.actuators.len())
            .enumerate()
        {
            physics.control(i, *target, state.active(i)).unwrap();
        }
        physics.clear_forces();
        let (q, v) = (physics.qpos().to_vec(), physics.qvel().to_vec());
        let wings = [0, 1].map(|s| joints[s].map(|(qi, di)| [q[qi], v[di]]));
        let torques = flight
            .torques(physics.time(), state.commands(), wings)
            .unwrap();
        for (wing, tau) in joints.iter().zip(torques) {
            for ((_, dof), t) in wing.iter().zip(tau) {
                physics.add_joint_force(*dof, t).unwrap();
            }
        }
        for &(body, local, area, polar) in &panels {
            let (position, r, velocity) = physics.body_state(body).unwrap();
            let point: [f64; 3] = std::array::from_fn(|i| {
                position[i] + (0..3).map(|j| r[i][j] * local[j]).sum::<f64>()
            });
            let com = physics.center_of_mass(body).unwrap();
            let arm: [f64; 3] = std::array::from_fn(|i| point[i] - com[i]);
            let w = [velocity[0], velocity[1], velocity[2]];
            let spin = [
                w[1] * arm[2] - w[2] * arm[1],
                w[2] * arm[0] - w[0] * arm[2],
                w[0] * arm[1] - w[1] * arm[0],
            ];
            let relative: [f64; 3] = std::array::from_fn(|i| velocity[i + 3] + spin[i]);
            let force = polar_panel_force(relative, r, area, aero.density_kg_m3, &polar).unwrap();
            physics.force_at(body, force, point).unwrap();
        }
        physics.step();
        stroke.push((fold(&physics, "left"), fold(&physics, "right")));
    }
    // In phase (one thoracic oscillator) and mirrored.
    assert!(stroke[300..].iter().all(|(l, r)| (l - r).abs() < 0.03));
    // 40 Hz: 20 full beats (40 sign changes) in the last 0.5 s.
    let late: Vec<f64> = stroke[350..].iter().map(|s| s.0).collect();
    let mean = late.iter().sum::<f64>() / late.len() as f64;
    let crossings = late
        .windows(2)
        .filter(|w| (w[0] - mean) * (w[1] - mean) < 0.)
        .count();
    assert!(
        (38..=42).contains(&crossings),
        "{crossings} stroke reversals in 0.5 s"
    );
    // Amplitude of the fundamental from the stroke's rms (sqrt 2 x rms of a
    // sine); the 8 ms decode leaves a little pulse-density ripple on peaks.
    let rms = (late.iter().map(|q| (q - mean).powi(2)).sum::<f64>() / late.len() as f64).sqrt();
    let amplitude = std::f64::consts::SQRT_2 * rms;
    assert!(
        (amplitude - 1.02).abs() < 0.1,
        "stroke amplitude {amplitude}"
    );
    assert!(
        physics.qpos()[2] > standing + 0.03,
        "no lift-off: {} m",
        physics.qpos()[2]
    );
}

/// NAO V6 senses: toe bumpers are switches on the toe shell (never pressed
/// by standing on the floor) and the 60 deg sonar cone sees a low obstacle
/// below the chest while the floor, struck obliquely, returns no echo.
#[test]
fn nao_v6_toe_bumpers_and_sonar_cone_are_physical() {
    let path = directory().join("nao.xml");
    let load = || {
        let record: Model =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        let names: Vec<String> = record.io.sensors.iter().map(|s| s.name.clone()).collect();
        (World::load(&path, record).unwrap(), names)
    };
    let index = |names: &[String], n: &str| names.iter().position(|x| x == n).unwrap();
    // The room's low target ball (0.225 m, centre (0.78, -0.645)).
    let (ball, radius) = ([0.78, -0.645], 0.1125);
    // Measured in the standing pose itself: without a brain the unpowered
    // servos let the body fall within a second.
    let placed = |root: [f64; 2]| {
        let (mut world, names) = load();
        let mut state = world.physics.snapshot();
        // mjSTATE_INTEGRATION starts with time, then qpos (free joint xyz).
        state[1] = root[0];
        state[2] = root[1];
        world.physics.restore(&state).unwrap();
        // Settle the 2 mm onto the floor (50 ms; toppling takes ~0.5 s).
        for _ in 0..25 {
            world.advance().unwrap();
        }
        let s = world.sensors().unwrap();
        (s, names)
    };
    // Standing at the centre of the room (walls ~1.5 m away, beyond the
    // 0.8 m range): feet loaded on the floor, yet no bumper and no echo.
    let (s, names) = placed([0., 0.]);
    for n in ["LFsr.z", "RFsr.z"] {
        assert!(
            (s[index(&names, n)] - 0.5).abs() > 1e-6,
            "{n}: foot not loaded"
        );
    }
    for n in [
        "LFoot/Bumper/Left",
        "LFoot/Bumper/Right",
        "RFoot/Bumper/Left",
        "RFoot/Bumper/Right",
    ] {
        assert_eq!(s[index(&names, n)], 0., "{n} pressed by the floor");
    }
    for n in ["Sonar/Left", "Sonar/Right"] {
        assert_eq!(
            s[index(&names, n)],
            0.,
            "{n} echoes the floor: {}",
            s[index(&names, n)]
        );
    }
    // Left toe pressed 5 mm into the ball at the top edge of the toe.
    let toe_reach = (radius * radius - (radius - 0.026f64).powi(2)).sqrt();
    let (s, names) = placed([ball[0] - 0.11 - toe_reach + 0.005, ball[1] - 0.05]);
    assert!(
        s[index(&names, "LFoot/Bumper/Left")] + s[index(&names, "LFoot/Bumper/Right")] > 0.,
        "left toe bumper misses the ball"
    );
    // 0.45 m from the ball, facing it: below the chest, inside the cone.
    let (s, names) = placed([ball[0] - radius - 0.45 - 0.0477, ball[1]]);
    let nearest = s[index(&names, "Sonar/Left")].max(s[index(&names, "Sonar/Right")]);
    assert!(nearest > 0., "sonar cone misses the low ball");
}
