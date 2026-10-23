//! Native Webots reference physics/AER controller. No production effect grant.
use aarnn_embodiment::{
    actuation::Actuation,
    aer::{Binding, Capture, Client},
    force::panel_force,
    motor::Coupling,
    schema,
};
use aarnn_webots::native::{Controller, Node};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
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
}
fn default_control() -> String {
    "auto".into()
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
    if legacy && config.binding.is_some() {
        return Err("This native graph's legacy sensory adapter is implemented in MuJoCo; use the preserved Webots legacy route or the MuJoCo physics service".into());
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
    let outcome = (|| -> Result<(), String> {
        for iteration in 0..config.steps {
            if cancel.load(Ordering::Acquire) {
                break;
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
            if legacy {
                sensors.resize(model.io.sensors.len(), 0.);
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
                for sensor in &model.io.sensors {
                    let value = match sensor.kind.as_str() {
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
            if client.as_ref().is_some_and(|c| c.status().ready) {
                client.as_ref().unwrap().submit(Capture {
                    step,
                    timestamp_us: step.checked_mul(2000).ok_or("source clock overflow")?,
                    values: sensors.clone(),
                });
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
    let result = json!({"schema_version":1,"status":if outcome.is_ok()&&transport.as_ref().is_none_or(|s|s.fault.is_none()){"pass"}else{"fail"},"scope":"native articulated physics and optional AER1 reference; no learned task/production commitment/parity claim","profile":model.profile_id,"instance_id":config.instance_id,"brain_id":config.binding.as_ref().map(|b|&b.brain_id),"model_digest":model.digest,"io_digest":model.io.digest,"steps":step,"reset_epoch":reset_epoch,"disarmed_at_stop":true,"npc_policy":aarnn_embodiment::npc::POLICY,"npc_steps":actuation.npc_steps,"controller_mode":if actuation.npc_control{"programmed-NPC"}else if config.diagnostic.is_some(){"diagnostic-mechanics"}else if config.binding.is_some(){"AER1-reference-untrained"}else{"silent-mechanics"},"initial_root":initial_root,"final_root":final_root,"initial_joints":initial_q,"final_joints":final_q,"sensor_values":sensors,"transport":transport,"applied":actuation.applied,"active_steps":active_steps,"expired":actuation.expired,"energy_J":if crow{Some(energy)}else{None},"elapsed_s":start.elapsed().as_secs_f64(),"error":outcome.as_ref().err()});
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
