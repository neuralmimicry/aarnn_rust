// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Native Webots reference physics/AER controller. No production effect grant.
use aarnn_embodiment::{
    actuation::Actuation,
    aer::{Binding, Capture, Client},
    force::panel_force,
    motor::{self, Coupling},
    schema,
    vision::Cell,
};
use aarnn_webots::{
    legacy,
    native::{Controller, Node},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Deserialize)]
struct Config {
    model: PathBuf,
    target: PathBuf,
    instance_id: String,
    output: PathBuf,
    steps: u64,
    binding: Option<Binding>,
    reset_at: Option<u64>,
    #[serde(default)]
    realtime: bool,
    #[serde(default)]
    quit_when_done: bool,
    diagnostic: Option<Diagnostic>,
    #[serde(default = "default_control")]
    control: String,
    /// Wait for each reference reply before the next physics step (as the
    /// MuJoCo `--lockstep` reference), so a slow brain receives every frame.
    #[serde(default)]
    lockstep: bool,
    /// Managed startup gate: no physics step before this file exists.
    arm_file: Option<PathBuf>,
    /// Directory for a read-only `INSTANCE.jsonl` body/joint/sensor trace.
    trace: Option<PathBuf>,
    #[serde(default = "default_trace_every")]
    trace_every: u64,
}
fn default_control() -> String {
    "auto".into()
}
fn default_trace_every() -> u64 {
    25
}
/// One preserved legacy sensory name resolved to its Webots measurement.
#[derive(Clone, Copy)]
enum Source {
    Angle(usize),
    Contact(Node, Option<[f64; 3]>),
    Accel(usize, usize),
    Gyro(usize, usize),
    Flow(usize, usize),
    Depth(usize),
    Pitch(usize),
    Range(usize, f64),
    Camera(usize, [usize; 2]),
}
#[derive(Deserialize)]
struct Diagnostic {
    joint: String,
    effort: f64,
    steps: u64,
}
#[derive(Deserialize)]
struct Target {
    schema_version: u32,
    model_digest: String,
    io_digest: String,
    targets: Vec<Joint>,
    constraints: Vec<Coupling>,
}
#[derive(Deserialize)]
struct Joint {
    joint: String,
    device: String,
    sensor: String,
    kind: String,
    actuator: Option<usize>,
    gain: f64,
    bias: [f64; 3],
    gear: f64,
    force_range: [f64; 2],
    driven: bool,
}
fn rotate(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| r[i][j] * v[j]).sum())
}
fn inverse(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| r[j][i] * v[j]).sum())
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normal(v: f64, r: [f64; 2]) -> f64 {
    ((v - r[0]) / (r[1] - r[0])).clamp(0., 1.)
}
fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("controller requires launch config")?;
    let config: Config = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if config.steps == 0 || config.steps > 10_000_000 || config.instance_id.is_empty() {
        return Err("invalid instance/budget".into());
    }
    let model = schema::load_model(&config.model)?;
    let npc_enabled =
        aarnn_embodiment::npc::Control::parse(&config.control)?.npc(config.binding.is_some())?;
    let legacy = !aarnn_embodiment::projection::Projection::supported(&model);
    let unsupported: BTreeSet<&str> = model
        .io
        .sensors
        .iter()
        .map(|s| s.kind.as_str())
        .filter(|k| !legacy::SUPPORTED.contains(k))
        .collect();
    if legacy && config.binding.is_some() && !unsupported.is_empty() {
        return Err(format!(
            "Webots legacy sensory adapter lacks {unsupported:?} (implemented in MuJoCo); use the preserved Webots legacy route or the MuJoCo physics service"
        ));
    }
    if legacy
        && config.binding.is_some()
        && (model.profile_id == "zebrafish"
            || model
                .scene
                .as_ref()
                .is_some_and(|s| s.resistive_force.is_some()))
    {
        return Err("Webots native water/substrate forces are not implemented; use MuJoCo".into());
    }
    if config.trace_every == 0 || config.trace_every > 100_000 {
        return Err("trace interval outside 1..=100000 steps".into());
    }
    let mut value: Value =
        serde_json::from_slice(&fs::read(&config.target).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let expected = value
        .as_object_mut()
        .ok_or("invalid target map")?
        .remove("digest")
        .ok_or("target digest missing")?;
    if schema::digest(&value)? != expected.as_str().ok_or("invalid digest")? {
        return Err("target mapping digest mismatch".into());
    }
    let target: Target = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if target.schema_version != 1
        || target.model_digest != model.digest
        || target.io_digest != model.io.digest
    {
        return Err("target/model mismatch".into());
    }
    if config.binding.is_some() && config.diagnostic.is_some() {
        return Err("diagnostic effort cannot run in neural mode".into());
    }
    if let Some(d) = &config.diagnostic
        && (!d.effort.is_finite()
            || d.effort.abs() > 2.
            || d.steps > 100
            || !target
                .targets
                .iter()
                .any(|j| j.joint == d.joint && j.driven))
    {
        return Err("bounded named diagnostic required".into());
    }
    let model_value: Value = serde_json::from_slice(
        &fs::read(config.model.with_extension("json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let body_names: HashMap<String, usize> = model_value["bodies"]
        .as_array()
        .ok_or("body manifest")?
        .iter()
        .enumerate()
        .map(|(i, b)| Ok((b["name"].as_str().ok_or("body name")?.to_owned(), i + 1)))
        .collect::<Result<_, String>>()?;
    let mut engine = Controller::init();
    let handles: Vec<_> = target
        .targets
        .iter()
        .map(|j| {
            engine.joint(
                &j.sensor,
                if j.driven {
                    Some((&j.device, j.kind == "linear"))
                } else {
                    None
                },
            )
        })
        .collect::<Result<_, _>>()?;
    let roots: Vec<Node> = model_value["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, b)| b["parent"] == 0)
        .map(|(i, _)| engine.node(&format!("{}_BODY_{}", config.instance_id, i + 1)))
        .collect::<Result<_, _>>()?;
    let node = |name: &str| -> Result<Node, String> {
        engine.node(&format!(
            "{}_BODY_{}",
            config.instance_id,
            body_names.get(name).ok_or("named body absent")?
        ))
    };
    let root = engine.node(&format!("{}_BODY_1", config.instance_id))?;
    let crow = model.profile_id == "crow_corvus_corone";
    let panels = if crow {
        vec![
            (node("left_wing_wrist_twist_link")?, [0., 0.06, 0.], 0.045),
            (node("right_wing_wrist_twist_link")?, [0., -0.06, 0.], 0.045),
            (node("tail_fan_link")?, [-0.03, 0., 0.], 0.008),
        ]
    } else {
        vec![]
    };
    let head = if crow {
        Some(node("neck_roll_link")?)
    } else {
        None
    };
    let contacts: Vec<Node> = if crow {
        let mut names = vec!["beak_gape_link".to_owned(), "beak_gape_link".to_owned()];
        for side in ["left", "right"] {
            for _ in 0..3 {
                names.push(format!("{side}_leg_front_toes_curl_link"));
            }
            names.push(format!("{side}_leg_hallux_curl_link"));
        }
        for side in ["left", "right"] {
            names.push(format!("{side}_wing_wrist_twist_link"));
        }
        names.iter().map(|n| node(n)).collect::<Result<_, _>>()?
    } else {
        vec![]
    };
    let gyro = engine.sensor(if crow { "head_gyro" } else { "body_gyro" }, "gyro")?;
    let accel = if crow {
        Some(engine.sensor("head_acceleration", "acceleration")?)
    } else {
        None
    };
    let retina = if crow {
        vec![
            engine.sensor("retina_left", "camera")?,
            engine.sensor("retina_right", "camera")?,
        ]
    } else {
        vec![]
    };
    let ranges = if !crow {
        (0..8)
            .map(|i| engine.sensor(&format!("proximity_{i}"), "range"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![]
    };
    // Preserved legacy names: resolve each to its Webots measurement once.
    let mut sources = Vec::new();
    let mut sense_bodies: Vec<Node> = Vec::new();
    let mut sense_slots: HashMap<String, usize> = HashMap::new();
    let mut cameras: Vec<(u16, [usize; 2], Vec<Cell>)> = Vec::new();
    let mut camera_slots: HashMap<String, usize> = HashMap::new();
    let mut proxies = BTreeSet::new();
    if legacy && unsupported.is_empty() {
        let target_index: HashMap<&str, usize> = target
            .targets
            .iter()
            .enumerate()
            .map(|(i, j)| (j.joint.as_str(), i))
            .collect();
        for s in &model.io.sensors {
            if legacy::PROXY.contains(&s.kind.as_str()) {
                proxies.insert(s.kind.clone());
            }
            let axis = s.axis.unwrap_or(0);
            if axis >= 3 {
                return Err("sensor axis outside XYZ".into());
            }
            let mut slot = |name: &Option<String>| -> Result<usize, String> {
                let name = name.as_deref().ok_or("legacy sensor body absent")?;
                if let Some(&i) = sense_slots.get(name) {
                    return Ok(i);
                }
                sense_bodies.push(node(name)?);
                sense_slots.insert(name.to_owned(), sense_bodies.len() - 1);
                Ok(sense_bodies.len() - 1)
            };
            sources.push(match s.kind.as_str() {
                "legacy-angle" => Source::Angle(
                    *target_index
                        .get(s.joint.as_deref().ok_or("legacy joint absent")?)
                        .ok_or("legacy joint has no Webots sensor")?,
                ),
                "legacy-contact" => Source::Contact(
                    node(s.body.as_deref().ok_or("legacy sensor body absent")?)?,
                    s.direction,
                ),
                "legacy-accel" => Source::Accel(slot(&s.body)?, axis),
                "legacy-gyro" => Source::Gyro(slot(&s.body)?, axis),
                "legacy-flow" => Source::Flow(slot(&s.body)?, axis),
                "legacy-depth" => Source::Depth(slot(&s.body)?),
                "legacy-pitch" => Source::Pitch(slot(&s.body)?),
                "legacy-range" => {
                    if ranges.is_empty() {
                        return Err("legacy range proxy needs the root laser rays".into());
                    }
                    Source::Range(
                        legacy::nearest_ray(
                            s.direction.ok_or("legacy ray direction absent")?,
                            ranges.len(),
                        ),
                        s.angle_rad.ok_or("legacy range reach absent")?,
                    )
                }
                "legacy-camera" => {
                    let name = s.camera.clone().ok_or("legacy camera absent")?;
                    let shape = s.retina_shape.ok_or("camera shape absent")?;
                    if shape[0] == 0 || shape[1] == 0 || shape[0] * shape[1] > 192 {
                        return Err("camera bound".into());
                    }
                    let index = match camera_slots.get(&name) {
                        Some(&i) => i,
                        None => {
                            cameras.push((
                                engine.sensor(&name, "camera")?,
                                shape,
                                vec![Cell::default(); shape[0] * shape[1]],
                            ));
                            camera_slots.insert(name, cameras.len() - 1);
                            cameras.len() - 1
                        }
                    };
                    let cell = s.cell.ok_or("camera cell absent")?;
                    if cell[0] >= shape[0] || cell[1] >= shape[1] {
                        return Err("camera cell outside retina".into());
                    }
                    let mode = match s.mode.as_deref() {
                        Some("on") => 0,
                        Some("off") => 1,
                        Some("lum") => 2,
                        Some("gradient") => 3,
                        _ => return Err("camera mode".into()),
                    };
                    Source::Camera(index, [cell[0] * shape[1] + cell[1], mode])
                }
                _ => return Err(format!("unimplemented legacy sensor {} {}", s.name, s.kind)),
            });
        }
    }
    let mut previous_velocity: Vec<Option<[f64; 3]>> = vec![None; sense_bodies.len()];
    let mut camera_unavailable = 0u64;
    // Pose-only nodes of every body, for the trace (MuJoCo `body_xyz` order).
    let trace_nodes: Vec<Node> = (1..=body_names.len())
        .map(|i| engine.pose_node(&format!("{}_BODY_{}", config.instance_id, i)))
        .collect::<Result<_, _>>()?;
    for &n in &roots {
        engine.save(n);
    }
    if !engine.step() {
        return Err("world stopped before sensor warmup".into());
    }
    let mut q = handles
        .iter()
        .map(|(s, _)| engine.position(*s))
        .collect::<Result<Vec<_>, _>>()?;
    let initial_q = q.clone();
    let initial_root = engine.body(root)?.0;
    let indices: HashMap<_, _> = target
        .targets
        .iter()
        .enumerate()
        .map(|(i, j)| (j.joint.clone(), i))
        .collect();
    let mut client = if let Some(b) = config.binding.clone() {
        Some(Client::start(model.clone(), b)?)
    } else {
        None
    };
    let cancel = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, cancel.clone()).map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    let mut deadline = start;
    let mut step = 0u64;
    let mut reset_epoch = 0u64;
    let mut actuation = Actuation::new(&model);
    let mut active_steps = 0u64;
    let mut previous_retina = vec![0.; 192];
    let mut energy = 20000.;
    let mut sensors = vec![];
    let mut submitted = 0u64;
    let mut trace = match &config.trace {
        Some(dir) => {
            fs::create_dir_all(dir).map_err(|e| format!("trace {}: {e}", dir.display()))?;
            let path = dir.join(format!("{}.jsonl", config.instance_id));
            let mut writer = std::io::BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| format!("trace {}: {e}", path.display()))?,
            );
            let header = json!({"schema":"aarnn-webots-trace-v1","physics":"Webots R2025a (ODE)","instance_id":config.instance_id,"profile":model.profile_id,"brain_id":config.binding.as_ref().map(|b|&b.brain_id),"model_digest":model.digest,"io_digest":model.io.digest,"body_count":trace_nodes.len()+1,"actuated_joints":model.actuators.iter().map(|a|&a.joint).collect::<Vec<_>>(),"sensor_names":model.io.sensors.iter().map(|s|&s.name).collect::<Vec<_>>(),"output_names":model.io.output_names,"legacy_proxies":proxies,"lockstep":config.lockstep,"trace_every_steps":config.trace_every,"scope":"read-only physics/IO observation; no production commit or effect authority"});
            writeln!(writer, "{header}").map_err(|e| e.to_string())?;
            Some((path, writer, 0u64))
        }
        None => None,
    };
    // Data-timeout backstop identical to the MuJoCo lock-step reference.
    let lockstep_deadline = Duration::from_millis(
        (std::env::var("NM_AER_REFERENCE_DATA_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v.clamp(1_000, 600_000))
            .unwrap_or(0)
            + 5_000)
            .max(30_000),
    );
    // Managed startup: no physics step before the arm gate and handshake.
    if let Some(c) = &client {
        while !cancel.load(Ordering::Acquire) {
            let status = c.status();
            if let Some(fault) = status.fault {
                return Err(format!("reference startup failed: {fault}"));
            }
            if status.ready && config.arm_file.as_ref().is_none_or(|p| p.is_file()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let outcome = (|| -> Result<(), String> {
        for iteration in 0..config.steps {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            if config.lockstep
                && let Some(c) = &client
            {
                let waited = Instant::now();
                loop {
                    let status = c.status();
                    if let Some(fault) = status.fault {
                        return Err(format!("reference transport failed: {fault}"));
                    }
                    if status.exchanges >= submitted || cancel.load(Ordering::Acquire) {
                        break;
                    }
                    if waited.elapsed() >= lockstep_deadline {
                        return Err("lock-step reply deadline exceeded".into());
                    }
                    std::thread::sleep(Duration::from_micros(250));
                }
            }
            if config.reset_at == Some(iteration) {
                client.take();
                actuation.reset();
                for &n in &roots {
                    engine.reset(n);
                }
                step = 0;
                reset_epoch += 1;
                previous_retina.fill(0.);
                energy = 20000.;
                if !engine.step() {
                    break;
                }
                q = handles
                    .iter()
                    .map(|(s, _)| engine.position(*s))
                    .collect::<Result<_, _>>()?;
            }
            let mut velocity = vec![0.; q.len()];
            let mut efforts = vec![0.; q.len()];
            let neural_ready = client.as_ref().is_some_and(|c| {
                let s = c.status();
                s.ready && s.fault.is_none()
            });
            let reply = client.as_ref().and_then(Client::receive);
            actuation.select(
                &model,
                step,
                npc_enabled,
                neural_ready,
                reply.as_ref(),
                if sensors.is_empty() {
                    None
                } else {
                    Some(&sensors)
                },
            )?;
            let controls = actuation.controls(&model);
            let process = actuation.process();
            // Velocities are measured from consecutive physics states, not
            // rendered joint animation or network-arrival timestamps.
            let next = handles
                .iter()
                .map(|(s, _)| engine.position(*s))
                .collect::<Result<Vec<_>, _>>()?;
            for i in 0..q.len() {
                velocity[i] = (next[i] - q[i]) / 0.002;
            }
            q = next;
            for (i, j) in target.targets.iter().enumerate() {
                if let Some(aid) = j.actuator
                    && actuation.active(aid)
                    && model.actuators[aid].velocity_limit.is_none_or(|limit| {
                        motor::servo_speed_allows(controls[aid], q[i], velocity[i], limit)
                    })
                {
                    let command = controls[aid];
                    let scalar =
                        (j.gain * command + j.bias[0] + j.bias[1] * q[i] + j.bias[2] * velocity[i])
                            .clamp(j.force_range[0], j.force_range[1]);
                    efforts[i] = scalar * j.gear;
                }
            }
            if crow {
                let power: f64 = efforts
                    .iter()
                    .zip(&velocity)
                    .map(|(t, v)| (t * v).abs())
                    .sum();
                let scale = if power > 25. { 25. / power } else { 1. } * (energy / 2000f64).min(1.);
                for e in &mut efforts {
                    *e *= scale;
                }
                energy = (energy
                    - (power * scale / (0.20 + 0.05 * process[1]) + 1. + 2. * process[0]) * 0.002)
                    .max(0.);
                if energy == 0. {
                    efforts.fill(0.);
                    actuation.stop();
                }
            }
            if !actuation.disarmed
                && !actuation.npc_control
                && efforts.iter().any(|e| e.abs() > 1e-9)
            {
                active_steps += 1;
            }
            for c in &target.constraints {
                let i = *indices.get(&c.joint1).ok_or("coupling joint1")?;
                let j = *indices.get(&c.joint2).ok_or("coupling joint2")?;
                let t = c.efforts([q[i], q[j]], [velocity[i], velocity[j]])?;
                efforts[i] += t[0];
                efforts[j] += t[1];
            }
            if let Some(d) = &config.diagnostic
                && iteration < d.steps
            {
                efforts[indices[&d.joint]] += d.effort;
            }
            for (i, (_, motor)) in handles.iter().enumerate() {
                if let Some(m) = motor {
                    engine.effort(*m, target.targets[i].kind == "linear", efforts[i])?;
                }
            }
            for &(node, point, area) in &panels {
                let (p, r, v, com) = engine.body(node)?;
                let offset = rotate(r, point);
                let arm = std::array::from_fn(|i| p[i] + offset[i] - com[i]);
                let angular = cross([v[3], v[4], v[5]], arm);
                let local = std::array::from_fn(|i| v[i] + angular[i]);
                engine.force(
                    node,
                    panel_force(local, r, area).map_err(str::to_string)?,
                    point,
                )?;
            }
            if !engine.step() {
                break;
            }
            step += 1;
            // Capture real next-state sensory measurements once per source step.
            let measured = handles
                .iter()
                .map(|(s, _)| engine.position(*s))
                .collect::<Result<Vec<_>, _>>()?;
            let velocity: Vec<_> = measured
                .iter()
                .zip(&q)
                .map(|(n, p)| (n - p) / 0.002)
                .collect();
            let gyro_values = engine.vector(gyro, false)?;
            sensors.clear();
            if legacy && sources.is_empty() {
                sensors.resize(model.io.sensors.len(), 0.);
            } else if legacy {
                let states = sense_bodies
                    .iter()
                    .map(|n| engine.body(*n))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut frames: Vec<Vec<[f64; 4]>> = Vec::with_capacity(cameras.len());
                for (tag, shape, cells) in &mut cameras {
                    let image = engine.luminance(*tag);
                    if image.is_err() {
                        camera_unavailable += 1;
                    }
                    let mut out = Vec::with_capacity(cells.len());
                    for (k, cell) in cells.iter_mut().enumerate() {
                        let light = match &image {
                            Ok((w, h, data)) => legacy::cell_luminance(
                                data,
                                [k / shape[1], k % shape[1]],
                                *shape,
                                *w,
                                *h,
                            ),
                            Err(_) => 0.,
                        };
                        out.push(cell.sample(light, 0.002)?);
                    }
                    frames.push(out);
                }
                let specific: Vec<[f64; 3]> = states
                    .iter()
                    .zip(&previous_velocity)
                    .map(|(st, previous)| {
                        let v = [st.2[0], st.2[1], st.2[2]];
                        legacy::local(
                            st.1,
                            legacy::specific_force(previous.unwrap_or(v), v, 0.002),
                        )
                    })
                    .collect();
                for (source, s) in sources.iter().zip(&model.io.sensors) {
                    let mode = s.mode.as_deref();
                    let value = match *source {
                        Source::Angle(i) => legacy::angle(measured[i]),
                        Source::Contact(n, None) => {
                            if engine.contact(n) {
                                1.
                            } else {
                                0.
                            }
                        }
                        Source::Contact(n, Some(direction)) => {
                            let (p, r, _, _) = engine.body(n)?;
                            legacy::directional(p, r, &engine.contact_points(n), direction)
                        }
                        Source::Accel(b, axis) => legacy::inertial(specific[b], axis, mode, true),
                        Source::Gyro(b, axis) => {
                            let st = &states[b];
                            legacy::inertial(
                                legacy::local(st.1, [st.2[3], st.2[4], st.2[5]]),
                                axis,
                                mode,
                                false,
                            )
                        }
                        Source::Flow(b, axis) => {
                            let st = &states[b];
                            legacy::flow(
                                legacy::local(st.1, [st.2[0], st.2[1], st.2[2]]),
                                axis,
                                mode,
                            )
                        }
                        Source::Depth(b) => legacy::depth(states[b].0[2]),
                        Source::Pitch(b) => legacy::pitch(states[b].1),
                        Source::Range(ray, reach) => legacy::range_reading(
                            engine.range(ranges[ray])?,
                            reach,
                            s.lookup.as_deref(),
                        )?,
                        Source::Camera(c, [k, m]) => frames[c][k][m],
                    };
                    if !value.is_finite() {
                        return Err(format!("nonfinite sensor {}", s.name));
                    }
                    sensors.push(value.clamp(0., 1.));
                }
                for (previous, st) in previous_velocity.iter_mut().zip(&states) {
                    *previous = Some([st.2[0], st.2[1], st.2[2]]);
                }
            } else if crow {
                let mut next_retina = Vec::with_capacity(192);
                for &camera in &retina {
                    next_retina.extend(engine.camera(camera)?);
                }
                for (now, old) in next_retina.iter().zip(&previous_retina) {
                    let delta = now - old;
                    sensors.extend([delta.max(0.), (-delta).max(0.)]);
                }
                previous_retina = next_retina;
                for a in &model.actuators {
                    let i = indices[&a.joint];
                    sensors.extend([
                        (0.5 + measured[i] / 3.).clamp(0., 1.),
                        (0.5 + velocity[i] / 80.).clamp(0., 1.),
                    ]);
                }
                sensors.extend(gyro_values.map(|v| (0.5 + v / 80.).clamp(0., 1.)));
                sensors.extend(
                    engine
                        .vector(accel.unwrap(), true)?
                        .map(|v| (0.5 + v / 80.).clamp(0., 1.)),
                );
                let (_, r, _, _) = engine.body(head.unwrap())?;
                sensors.extend(inverse(r, [0., 0., -1.]).map(|v| (0.5 + v / 2.).clamp(0., 1.)));
                sensors.extend(
                    contacts
                        .iter()
                        .map(|&n| if engine.contact(n) { 1. } else { 0. }),
                );
                for i in 0..16 {
                    sensors.push(if i % 8 == 2 { process[0] } else { 0. });
                }
                let (_, r, v, _) = engine.body(root)?;
                sensors
                    .extend(inverse(r, [v[0], v[1], v[2]]).map(|v| (0.5 + v / 40.).clamp(0., 1.)));
                sensors.extend([
                    1. - energy / 20000.,
                    0.,
                    1. - energy / 20000.,
                    energy / 20000.,
                    0.,
                    0.,
                ]);
            } else {
                let gravity = if model.io.sensors.iter().any(|s| s.kind == "body_gravity") {
                    let (_, r, _, _) = engine.body(root)?;
                    inverse(r, [0., 0., -1.])
                } else {
                    [0.; 3]
                };
                for sensor in &model.io.sensors {
                    let value = match sensor.kind.as_str() {
                        "body_gravity" => gravity[sensor.axis.ok_or("gravity axis")?],
                        "angle" => {
                            measured[indices[sensor.joint.as_ref().ok_or("sensor joint")?]]
                        }
                        "angular_velocity" => {
                            velocity[indices[sensor.joint.as_ref().ok_or("sensor joint")?]]
                        }
                        "body_gyro" => gyro_values[sensor.axis.ok_or("gyro axis")?],
                        "range_ray" => engine.range(
                            ranges[sensor
                                .name
                                .strip_prefix("proximity_")
                                .ok_or("ray name")?
                                .parse::<usize>()
                                .map_err(|_| "ray index")?],
                        )?,
                        _ => return Err("unsupported sensor projection".into()),
                    };
                    sensors.push(normal(value, sensor.range));
                }
            }
            if sensors.len() != model.io.sensors.len() {
                return Err("ordered projection width mismatch".into());
            }
            if client.as_ref().is_some_and(|c| c.status().ready)
                && client.as_ref().unwrap().submit(Capture {
                    step,
                    timestamp_us: step.checked_mul(2000).ok_or("source clock overflow")?,
                    values: sensors.clone(),
                })
            {
                submitted += 1;
            }
            if let Some((path, writer, records)) = &mut trace
                && step.is_multiple_of(config.trace_every)
                && *records < 200_000
            {
                let bodies = trace_nodes
                    .iter()
                    .map(|n| engine.origin(*n))
                    .collect::<Result<Vec<_>, _>>()?;
                let joints: Vec<f64> = model
                    .actuators
                    .iter()
                    .map(|a| indices.get(&a.joint).map_or(0., |&i| measured[i]))
                    .collect();
                let status = client.as_ref().map(Client::status);
                let record = json!({"step":step,"reset_epoch":reset_epoch,"time_s":step as f64*0.002,"body_xyz":bodies,"joint_q":joints,"motor_rates":actuation.rates(),"disarmed":actuation.disarmed,"npc_control":actuation.npc_control,"sensors":sensors,"applied":actuation.applied,"expired":actuation.expired,"exchanges":status.as_ref().map(|s|s.exchanges),"output_code":status.as_ref().map(|s|s.output_code)});
                writeln!(writer, "{record}")
                    .map_err(|e| format!("trace {}: {e}", path.display()))?;
                *records += 1;
            }
            if config.realtime {
                deadline += Duration::from_millis(2);
                if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        }
        Ok(())
    })();
    engine.disarm();
    let final_q = handles
        .iter()
        .map(|(s, _)| engine.position(*s))
        .collect::<Result<Vec<_>, _>>()?;
    let transport = client.as_ref().map(Client::status);
    client.take();
    let final_root = engine.body(root)?.0;
    let result = json!({"schema_version":1,"status":if outcome.is_ok()&&transport.as_ref().is_none_or(|s|s.fault.is_none()){"pass"}else{"fail"},"scope":"native articulated physics and optional AER1 reference; no learned task/production commitment/parity claim","profile":model.profile_id,"instance_id":config.instance_id,"brain_id":config.binding.as_ref().map(|b|&b.brain_id),"model_digest":model.digest,"io_digest":model.io.digest,"steps":step,"reset_epoch":reset_epoch,"disarmed_at_stop":true,"npc_policy":aarnn_embodiment::npc::POLICY,"npc_steps":actuation.npc_steps,"controller_mode":if actuation.npc_control{"programmed-NPC"}else if config.diagnostic.is_some(){"diagnostic-mechanics"}else if config.binding.is_some(){"AER1-reference-untrained"}else{"silent-mechanics"},"initial_root":initial_root,"final_root":final_root,"initial_joints":initial_q,"final_joints":final_q,"sensor_values":sensors,"transport":transport,"applied":actuation.applied,"active_steps":active_steps,"expired":actuation.expired,"energy_J":if crow{Some(energy)}else{None},"elapsed_s":start.elapsed().as_secs_f64(),"error":outcome.as_ref().err(),"physics":"Webots R2025a (ODE)","time_s":step as f64*0.002,"lockstep":config.lockstep,"legacy_proxies":proxies,"camera_unavailable":camera_unavailable,"trace":trace.as_mut().map(|(path,writer,records)|json!({"path":path,"records":records,"flush_error":writer.flush().err().map(|e|e.to_string())}))});
    if let Some(parent) = config.output.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = config.output.with_extension("json.pending");
    fs::write(&temporary, serde_json::to_vec_pretty(&result).unwrap())
        .map_err(|e| e.to_string())?;
    fs::rename(&temporary, &config.output).map_err(|e| e.to_string())?;
    let status = if result["status"] == "pass" { 0 } else { 1 };
    if config.quit_when_done || status != 0 {
        engine.quit(status);
    } else {
        while !cancel.load(Ordering::Acquire) {
            let next = handles
                .iter()
                .map(|(s, _)| engine.position(*s))
                .collect::<Result<Vec<_>, _>>()?;
            let velocity: Vec<_> = next.iter().zip(&q).map(|(n, p)| (n - p) / 0.002).collect();
            q = next;
            let mut efforts = vec![0.; q.len()];
            for c in &target.constraints {
                let i = indices[&c.joint1];
                let j = indices[&c.joint2];
                let t = c.efforts([q[i], q[j]], [velocity[i], velocity[j]])?;
                efforts[i] += t[0];
                efforts[j] += t[1];
            }
            for (i, (_, motor)) in handles.iter().enumerate() {
                if let Some(m) = motor {
                    engine.effort(*m, target.targets[i].kind == "linear", efforts[i])?;
                }
            }
            for &(node, point, area) in &panels {
                let (p, r, v, com) = engine.body(node)?;
                let offset = rotate(r, point);
                let arm = std::array::from_fn(|i| p[i] + offset[i] - com[i]);
                let angular = cross([v[3], v[4], v[5]], arm);
                let local = std::array::from_fn(|i| v[i] + angular[i]);
                engine.force(
                    node,
                    panel_force(local, r, area).map_err(str::to_string)?,
                    point,
                )?;
            }
            if !engine.step() {
                break;
            }
        }
    }
    outcome
}
