//! Native physics scheduling and source-clock transduction. Reference mode has
//! no production lease/commit claim; stale packets expire visibly and disarm.
use crate::{
    aer::{Capture, Client, Model},
    force::panel_force,
    native::Physics,
};
use aarnn_embodiment::actuation::Actuation;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

/// Local embodiment checkpoint, deliberately excluding the remote brain and
/// in-flight transport. Restoring always disconnects and inhibits actuation.
#[derive(Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub model_digest: String,
    pub io_digest: String,
    pub instance_id: String,
    pub brain_id: Option<String>,
    pub reset_epoch: u64,
    pub step: u64,
    pub physics: Vec<f64>,
    pub rates: Vec<f64>,
    pub process_rates: [f64; 2],
    pub retina: Vec<f64>,
    pub energy_j: f64,
}
fn rotation(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
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
fn normal(value: f64, limits: [f64; 2]) -> f64 {
    ((value - limits[0]) / (limits[1] - limits[0])).clamp(0., 1.)
}
pub struct World {
    pub physics: Physics,
    pub model: Model,
    pub instance_id: String,
    pub brain_id: Option<String>,
    pub client: Option<Client>,
    pub step: u64,
    pub reset_epoch: u64,
    pub energy_j: f64,
    pub active_steps: u64,
    pub npc_enabled: bool,
    actuator_ids: Vec<usize>,
    joint_indices: Vec<(usize, usize)>,
    pub actuation: Actuation,
    retina: Vec<f64>,
    panels: Vec<(usize, [f64; 3], f64)>,
    legacy: crate::legacy::Senses,
}
impl std::ops::Deref for World {
    type Target = Actuation;
    fn deref(&self) -> &Actuation {
        &self.actuation
    }
}
impl World {
    pub fn load(model_path: &Path, record: Model) -> Result<Self, String> {
        let mut physics = Physics::load(model_path)?;
        physics.disarm();
        let mut actuator_ids = Vec::new();
        let mut joint_indices = Vec::new();
        for a in &record.actuators {
            actuator_ids.push(physics.actuator(&a.name)?);
            joint_indices.push(physics.joint(&a.joint)?);
        }
        let mut panels = Vec::new();
        if record.profile_id == "crow_corvus_corone" {
            for (name, point, area) in [
                ("left_wing_wrist_twist_link", [0., 0.06, 0.], 0.045),
                ("right_wing_wrist_twist_link", [0., -0.06, 0.], 0.045),
                ("tail_fan_link", [-0.03, 0., 0.], 0.008),
            ] {
                panels.push((physics.body(name)?, point, area));
            }
        }
        if record.profile_id.starts_with("drosophila_") {
            for side in ["left", "right"] {
                panels.push((physics.body(&format!("{side}_wing"))?, [0., 0., 0.], 0.0025));
            }
        }
        let actuation = Actuation::new(&record);
        Ok(Self {
            physics,
            instance_id: format!("{}_0", record.profile_id),
            brain_id: None,
            model: record,
            client: None,
            step: 0,
            reset_epoch: 0,
            energy_j: 20000.,
            active_steps: 0,
            npc_enabled: false,
            actuator_ids,
            joint_indices,
            actuation,
            retina: vec![0.; 192],
            panels,
            legacy: crate::legacy::Senses::default(),
        })
    }
    pub fn measured_energy(&self) -> Option<f64> {
        (self.model.profile_id == "crow_corvus_corone").then_some(self.energy_j)
    }
    pub fn disarm(&mut self) {
        self.actuation.disarm();
        self.physics.disarm();
    }
    pub fn reset(&mut self) {
        self.client.take();
        self.disarm();
        self.physics.reset();
        self.step = 0;
        self.reset_epoch += 1;
        self.actuation.reset();
        self.retina.fill(0.);
        self.legacy = crate::legacy::Senses::default();
        self.energy_j = 20000.;
        self.actuation.stop();
    }
    pub fn stop(&mut self) {
        self.actuation.stop();
        self.disarm();
    }
    pub fn arm(&mut self) -> Result<(), String> {
        if self.client.is_none() && self.npc_enabled {
            self.actuation.arm();
            return Ok(());
        }
        if !self.client.as_ref().is_some_and(|c| {
            let s = c.status();
            s.ready && s.fault.is_none()
        }) {
            return Err("ready explicit reference binding required".into());
        }
        self.actuation.arm();
        Ok(())
    }
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            schema_version: 1,
            model_digest: self.model.digest.clone(),
            io_digest: self.model.io.digest.clone(),
            instance_id: self.instance_id.clone(),
            brain_id: self.brain_id.clone(),
            reset_epoch: self.reset_epoch,
            step: self.step,
            physics: self.physics.snapshot(),
            rates: self.actuation.rates().to_vec(),
            process_rates: self.actuation.process(),
            retina: self.retina.clone(),
            energy_j: self.energy_j,
        }
    }
    pub fn restore(&mut self, saved: &Checkpoint) -> Result<(), String> {
        if saved.schema_version != 1
            || saved.model_digest != self.model.digest
            || saved.io_digest != self.model.io.digest
            || saved.instance_id != self.instance_id
            || saved.brain_id != self.brain_id
            || saved.rates.len() != self.actuation.rates().len()
            || saved.retina.len() != 192
            || !saved.energy_j.is_finite()
            || !(0. ..=20000.).contains(&saved.energy_j)
            || !saved
                .rates
                .iter()
                .all(|v| v.is_finite() && (-1. ..=1.).contains(v))
            || !saved
                .retina
                .iter()
                .chain(saved.process_rates.iter())
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            || saved.step.checked_mul(2000).is_none()
        {
            return Err("checkpoint model/IO/scalar contract mismatch".into());
        }
        let epoch = self
            .reset_epoch
            .max(saved.reset_epoch)
            .checked_add(1)
            .ok_or("reset epoch overflow")?;
        // Native restore validates the entire state before any native mutation.
        self.physics.restore(&saved.physics)?;
        self.client.take();
        self.stop();
        self.reset_epoch = epoch;
        self.step = saved.step;
        self.energy_j = saved.energy_j;
        self.retina.clone_from(&saved.retina);
        self.legacy = crate::legacy::Senses::default();
        self.actuation.reset();
        // Saved output filters are evidence only. A new binding must supply new
        // output; neither a motor command nor a process effect is replayed.
        Ok(())
    }
    pub fn sensors(&mut self) -> Result<Vec<f64>, String> {
        if self.model.profile_id == "celegans" {
            return self.celegans_sensors();
        }
        if self
            .model
            .io
            .sensors
            .iter()
            .all(|s| s.kind.starts_with("legacy-"))
        {
            return self
                .legacy
                .measure(&mut self.physics, &self.model, self.step);
        }
        if self
            .model
            .controller_transfer
            .as_deref()
            .is_some_and(|s| s.starts_with("blocked"))
        {
            return Err("legacy sensory/muscle calibration incomplete".into());
        }
        let mut values = HashMap::new();
        for (a, (q, v)) in self.model.actuators.iter().zip(&self.joint_indices) {
            values.insert(format!("{}_angle", a.joint), self.physics.qpos()[*q]);
            values.insert(
                format!("{}_angular_velocity", a.joint),
                self.physics.qvel()[*v],
            );
        }
        let (position, r, velocity) = self.physics.body_state(1)?;
        let local_omega = inverse(r, [velocity[0], velocity[1], velocity[2]]);
        if self.model.profile_id != "crow_corvus_corone" {
            return self
                .model
                .io
                .sensors
                .iter()
                .map(|sensor| {
                    let raw = match sensor.kind.as_str() {
                        "angle" | "angular_velocity" => {
                            *values.get(&sensor.name).ok_or("missing joint projection")?
                        }
                        "body_gyro" => {
                            local_omega[*sensor.axis.as_ref().ok_or("missing gyro axis")?]
                        }
                        "range_ray" => {
                            let angle = sensor.angle_rad.ok_or("missing ray direction")?;
                            let delta = rotation(r, [0., 0., 0.1]);
                            let point = std::array::from_fn(|i| position[i] + delta[i]);
                            self.physics
                                .ray(point, rotation(r, [angle.cos(), angle.sin(), 0.]), 1)?
                                .unwrap_or(3.)
                                .min(3.)
                        }
                        _ => return Err("unsupported sensor projection".into()),
                    };
                    Ok(normal(raw, sensor.range))
                })
                .collect();
        }
        let mut out = Vec::with_capacity(492);
        let mut next_retina = Vec::with_capacity(192);
        for side in ["left", "right"] {
            let (position, r, body) = self.physics.camera_state(&format!("retina_{side}"))?;
            for y in 0..8 {
                for x in 0..12 {
                    let mut ray = rotation(r, [(x as f64 - 5.5) / 8., (3.5 - y as f64) / 8., -1.]);
                    let norm = ray.iter().map(|v| v * v).sum::<f64>().sqrt();
                    ray = ray.map(|v| v / norm);
                    let light = self.physics.ray_light(position, ray, body)?;
                    let index = next_retina.len();
                    let delta = light - self.retina[index];
                    out.extend([delta.max(0.), (-delta).max(0.)]);
                    next_retina.push(light);
                }
            }
        }
        self.retina = next_retina;
        for a in &self.model.actuators {
            out.push((0.5 + values[&format!("{}_angle", a.joint)] / 3.).clamp(0., 1.));
            out.push((0.5 + values[&format!("{}_angular_velocity", a.joint)] / 80.).clamp(0., 1.));
        }
        let head = self.physics.body("neck_roll_link")?;
        let (_, r, v) = self.physics.body_state(head)?;
        let omega = inverse(r, [v[0], v[1], v[2]]);
        out.extend(omega.map(|x| (0.5 + x / 80.).clamp(0., 1.)));
        let acceleration = self.physics.body_acceleration(head)?;
        let gravity = inverse(r, [0., 0., -9.81]);
        out.extend((0..3).map(|i| (0.5 + (acceleration[i + 3] - gravity[i]) / 80.).clamp(0., 1.)));
        out.extend(inverse(r, [0., 0., -1.]).map(|v| (0.5 + v / 2.).clamp(0., 1.)));
        let contacts = self.physics.contact_names();
        let active = |name: &str| {
            if contacts.iter().any(|n| n == name) {
                1.
            } else {
                0.
            }
        };
        out.extend([active("beak_gape_geom"); 2]);
        for side in ["left", "right"] {
            for i in 1..=3 {
                out.push(active(&format!("{side}_toe{i}")));
            }
            out.push(active(&format!("{side}_leg_hallux_curl_geom")));
        }
        for side in ["left", "right"] {
            out.push(active(&format!("{side}_wing_wrist_twist_geom")));
        }
        for i in 0..16 {
            out.push(if i % 8 == 2 {
                self.actuation.process()[0]
            } else {
                0.
            });
        }
        let (_, r, v) = self.physics.body_state(1)?;
        out.extend(inverse(r, [v[3], v[4], v[5]]).map(|x| (0.5 + x / 40.).clamp(0., 1.)));
        out.extend([
            1. - self.energy_j / 20000.,
            0.,
            1. - self.energy_j / 20000.,
            self.energy_j / 20000.,
            0.,
            0.,
        ]);
        if out.len() != 492 {
            return Err("crow projection width".into());
        }
        Ok(out)
    }
    fn celegans_sensors(&mut self) -> Result<Vec<f64>, String> {
        let root = self.physics.body("segment_00")?;
        let acceleration = self.physics.body_acceleration(root)?;
        let (_, r, velocity) = self.physics.body_state(root)?;
        let omega = inverse(r, [velocity[0], velocity[1], velocity[2]]);
        // Original Webots local axes: X forward, Y up, Z right.
        // MuJoCo's post-constraint object acceleration already includes the
        // proper-acceleration gravity reference (zero during free fall).
        let accel = [acceleration[3], acceleration[5], -acceleration[4]];
        let gyro = [omega[0], omega[2], -omega[1]];
        self.model
            .io
            .sensors
            .iter()
            .map(|s| {
                let value = match s.kind.as_str() {
                    "celegans-inertial-v1" => {
                        let axis = s.axis.ok_or("sensor axis")?;
                        if s.name.contains("accel") {
                            normal(accel[axis], [-20., 20.])
                        } else {
                            normal(gyro[axis], [-10., 10.])
                        }
                    }
                    "celegans-contact-v1" => self.physics.object_contact(
                        self.physics.body(s.body.as_deref().ok_or("sensor body")?)?,
                    ),
                    "celegans-range-v1" | "celegans-irradiance-v1" => {
                        let body = self.physics.body(s.body.as_deref().ok_or("sensor body")?)?;
                        let (p, r, _) = self.physics.body_state(body)?;
                        let offset = rotation(r, s.position_m.ok_or("sensor mount")?);
                        let point = std::array::from_fn(|i| p[i] + offset[i]);
                        if s.kind == "celegans-range-v1" {
                            let angle = s.angle_rad.ok_or("range direction")?;
                            let direction = rotation(r, [angle.cos(), angle.sin(), 0.]);
                            let distance = self
                                .physics
                                .ray(point, direction, body)?
                                .unwrap_or(s.range[1]);
                            1. - normal(distance, s.range)
                        } else {
                            self.physics.irradiance_proxy(point, s.axis == Some(0))
                        }
                    }
                    _ => return Err("C. elegans projection kind".into()),
                };
                if !value.is_finite() {
                    return Err("nonfinite C. elegans sensor".into());
                }
                Ok(value.clamp(0., 1.))
            })
            .collect()
    }
    pub fn advance(&mut self) -> Result<(), String> {
        let reply = self.client.as_ref().and_then(Client::receive);
        let neural_ready = self.client.as_ref().is_some_and(|c| {
            let s = c.status();
            s.ready && s.fault.is_none()
        });
        let observations = if self.npc_enabled
            && !neural_ready
            && !self
                .model
                .controller_transfer
                .as_deref()
                .is_some_and(|s| s.starts_with("blocked"))
        {
            Some(self.sensors()?)
        } else {
            None
        };
        self.actuation.select(
            &self.model,
            self.step,
            self.npc_enabled,
            neural_ready,
            reply.as_ref(),
            observations.as_deref(),
        )?;
        if self.disarmed {
            self.physics.disarm();
        } else {
            let controls = self.actuation.controls(&self.model);
            for (i, target) in controls.iter().take(self.actuator_ids.len()).enumerate() {
                let stiffness = if self.model.profile_id == "zebrafish"
                    && self.model.actuators[i].joint.starts_with("tail_joint_")
                {
                    1. + 4. * self.actuation.process()[0]
                } else {
                    1.
                };
                self.physics.control_with_gain(
                    self.actuator_ids[i],
                    *target,
                    self.actuation.active(i)
                        && self.model.actuators[i].velocity_limit.is_none_or(|limit| {
                            aarnn_embodiment::motor::servo_speed_allows(
                                *target,
                                self.physics.qpos()[self.joint_indices[i].0],
                                self.physics.qvel()[self.joint_indices[i].1],
                                limit,
                            )
                        }),
                    stiffness,
                )?;
            }
        }
        self.physics.clear_forces();
        if self.model.profile_id == "zebrafish" {
            // Versioned neutral-density water: actual displacement buoyancy and
            // dissipative drag, independent of spikes and active control.
            for body in 1..self.physics.body_count() {
                let (position, _, velocity) = self.physics.body_state(body)?;
                let mass = self.physics.body_mass(body)?;
                let force = aarnn_embodiment::force::water_force(
                    mass,
                    position[2],
                    [velocity[3], velocity[4], velocity[5]],
                )
                .map_err(str::to_string)?;
                self.physics
                    .force_at(body, force, self.physics.center_of_mass(body)?)?;
            }
        }
        for &(body, local, area) in &self.panels {
            let (position, r, velocity) = self.physics.body_state(body)?;
            let offset = rotation(r, local);
            let point = std::array::from_fn(|i| position[i] + offset[i]);
            let com = self.physics.center_of_mass(body)?;
            let arm = std::array::from_fn(|i| point[i] - com[i]);
            let rotational = cross([velocity[0], velocity[1], velocity[2]], arm);
            let relative = std::array::from_fn(|i| velocity[i + 3] + rotational[i]);
            let force = panel_force(relative, r, area).map_err(str::to_string)?;
            self.physics.force_at(body, force, point)?;
        }
        if self.model.profile_id == "crow_corvus_corone" && !self.disarmed {
            let predicted = self.physics.predicted_power();
            let budget_scale = if predicted > 25. { 25. / predicted } else { 1. };
            self.physics
                .scale_controls(budget_scale * (self.energy_j / 2000.).min(1.))?;
        }
        self.physics.step();
        if !self.disarmed && !self.actuation.npc_control && self.physics.has_active_effort() {
            self.active_steps += 1;
        }
        self.step += 1;
        // Only reduced-crow-v1 declares metabolic parameters. Commercial
        // batteries and legacy biology have no calibrated energy model yet.
        if self.model.profile_id == "crow_corvus_corone" {
            let efficiency = 0.20 + 0.05 * self.actuation.process()[1];
            self.energy_j = (self.energy_j
                - (self.physics.mechanical_power() / efficiency
                    + 1.
                    + 2. * self.actuation.process()[0])
                    * 0.002)
                .max(0.);
            if self.energy_j == 0. {
                self.stop();
            }
        }
        if self.client.as_ref().is_some_and(|c| c.status().ready) {
            let capture = Capture {
                step: self.step,
                timestamp_us: self.step.checked_mul(2000).ok_or("source clock overflow")?,
                values: self.sensors()?,
            };
            self.client.as_ref().unwrap().submit(capture);
        }
        Ok(())
    }
}
