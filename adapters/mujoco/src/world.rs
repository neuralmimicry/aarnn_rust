// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
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
/// Sensor mount: body id, world position and body rotation matrix.
type SensorMount = (usize, [f64; 3], [[f64; 3]; 3]);
/// Declared panel: body, point, panel frame in the body, area, polar.
type AeroPanel = (usize, [f64; 3], [[f64; 3]; 3], f64, [f64; 5]);
/// Flight motor joints: (qpos, dof) of the [stroke, pitch] joints per wing.
type FlightJoints = [[(usize, usize); 2]; 2];

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
    /// Declared `quasi-steady-panel-v2` panels (body, point, panel frame in
    /// the body, area, polar) and air density (scene `aerodynamics`).
    aero: Vec<AeroPanel>,
    air_density: f64,
    /// Hydrodynamic ellipsoids (body, semi-axes, centre offset) and water.
    hydro: Vec<(usize, [f64; 3], [f64; 3])>,
    water: Option<aarnn_embodiment::force::Water>,
    buoyancy_offset: [f64; 3],
    flight: Option<(aarnn_embodiment::flight::Flight, FlightJoints)>,
    legacy: crate::legacy::Senses,
    /// Sensory vector most recently submitted to the bound brain, or a
    /// passive-control observation from [`World::observe_unbound`].
    pub last_sensors: Vec<f64>,
    /// Cumulative output spikes per named channel, counted only for replies the
    /// motor state machine applied; expired or inhibited replies are excluded.
    pub applied_spikes: Vec<u64>,
    /// Captures admitted by the reference client, including the initial one.
    pub submitted: u64,
    /// C. elegans v2 vibration high-pass state (Webots axes) and its step.
    vibration_lp: [f64; 3],
    vibration_step: Option<u64>,
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
        let mut aero = Vec::new();
        let mut air_density = 1.225;
        if let Some(a) = record.scene.as_ref().and_then(|s| s.aerodynamics.as_ref()) {
            // quasi-steady-panel-v2: every engine applies the same declared
            // panels (crow: three per wing, tail, tail-base keel, body drag).
            if a.model != "quasi-steady-panel-v2" {
                return Err(format!("unsupported aerodynamics model {}", a.model));
            }
            air_density = a.density_kg_m3;
            for p in &a.panels {
                let polar = a.polars.get(&p.polar).ok_or("undeclared panel polar")?;
                let [w, x, y, z] = p.quaternion_wxyz;
                let frame = [
                    [
                        1. - 2. * (y * y + z * z),
                        2. * (x * y - w * z),
                        2. * (x * z + w * y),
                    ],
                    [
                        2. * (x * y + w * z),
                        1. - 2. * (x * x + z * z),
                        2. * (y * z - w * x),
                    ],
                    [
                        2. * (x * z - w * y),
                        2. * (y * z + w * x),
                        1. - 2. * (x * x + y * y),
                    ],
                ];
                aero.push((
                    physics.body(&p.body)?,
                    p.point_m,
                    frame,
                    p.area_m2,
                    polar.coefficients(),
                ));
            }
        } else if record.profile_id == "crow_corvus_corone" {
            for (name, point, area) in [
                ("left_wing_wrist_twist_link", [0., 0.06, 0.], 0.045),
                ("right_wing_wrist_twist_link", [0., -0.06, 0.], 0.045),
                ("tail_fan_link", [-0.03, 0., 0.], 0.008),
            ] {
                panels.push((physics.body(name)?, point, area));
            }
        }
        // resonant-flight-v1 (record `flight`): stroke and pitch joints
        // (qpos, dof) per wing; the blades' panels are scene `aerodynamics`.
        let flight = match &record.flight {
            Some(f) => {
                f.validate()?;
                let joints = |w: &aarnn_embodiment::flight::FlightWing| -> Result<[(usize, usize); 2], String> {
                    Ok([physics.joint(&w.stroke_joint)?, physics.joint(&w.pitch_joint)?])
                };
                Some((f.clone(), [joints(&f.wings[0])?, joints(&f.wings[1])?]))
            }
            None => None,
        };
        let mut hydro = Vec::new();
        let buoyancy_offset = record
            .scene
            .as_ref()
            .and_then(|s| s.hydrodynamics.as_ref())
            .map_or([0.; 3], |h| h.buoyancy_offset_m);
        let water = match record.scene.as_ref().and_then(|s| s.hydrodynamics.as_ref()) {
            Some(h) => {
                if h.model != "resistive-ellipsoid-v1" {
                    return Err(format!("unsupported hydrodynamics model {}", h.model));
                }
                for b in &h.bodies {
                    hydro.push((physics.body(&b.body)?, b.half_size_m, b.offset_m));
                }
                Some(h.water())
            }
            None => None,
        };
        let actuation = Actuation::new(&record);
        let outputs = record.io.output_names.len();
        let mut world = Self {
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
            aero,
            air_density,
            hydro,
            water,
            buoyancy_offset,
            flight,
            legacy: crate::legacy::Senses::default(),
            last_sensors: Vec::new(),
            applied_spikes: vec![0; outputs],
            submitted: 0,
            vibration_lp: [0.; 3],
            vibration_step: None,
        };
        world.launch()?;
        Ok(world)
    }
    /// Scene `launch`: give the free root its declared initial velocity (a
    /// bird released in flight). Applied at load and after every reset.
    fn launch(&mut self) -> Result<(), String> {
        let Some(velocity) = self
            .model
            .scene
            .as_ref()
            .and_then(|s| s.launch.as_ref())
            .map(|l| l.root_linear_velocity_mps)
        else {
            return Ok(());
        };
        // Integration state = [time, qpos (nq), qvel (nv), ...]; the free
        // root's first three velocities are its world linear velocity.
        let (nq, nv, _) = self.physics.dimensions();
        let mut state = self.physics.snapshot();
        if nv < 6 || state.len() < 1 + nq + 3 {
            return Err("launch requires a free root".into());
        }
        state[1 + nq..1 + nq + 3].copy_from_slice(&velocity);
        self.physics.restore(&state)
    }
    /// Passive-control observation for the stateless C. elegans projection
    /// only; the values are never submitted. Other projections carry temporal
    /// state (retina/camera cells) and are left unsampled rather than perturbed.
    pub fn observe_unbound(&mut self) -> Result<(), String> {
        if self.client.is_none() && !self.npc_enabled && self.model.profile_id == "celegans" {
            self.last_sensors = self.celegans_sensors()?;
        }
        Ok(())
    }
    /// Read-only evidence sample: never steps physics or alters controls.
    /// Sensors are the last submitted or passive-control values, not a new reading.
    pub fn trace_record(&self) -> Result<serde_json::Value, String> {
        let bodies = (1..self.physics.body_count())
            .map(|b| self.physics.body_state(b).map(|(p, _, _)| p))
            .collect::<Result<Vec<_>, _>>()?;
        let joints: Vec<f64> = self
            .joint_indices
            .iter()
            .map(|(q, _)| self.physics.qpos()[*q])
            .collect();
        Ok(serde_json::json!({
            "step": self.step,
            "reset_epoch": self.reset_epoch,
            "time_s": self.physics.time(),
            "body_xyz": bodies,
            "joint_q": joints,
            "motor_rates": self.actuation.rates(),
            "disarmed": self.disarmed,
            "npc_control": self.actuation.npc_control,
            "sensors": self.last_sensors,
            "sensor_source": if self.client.is_some() { "submitted" } else if self.last_sensors.is_empty() { "unsampled" } else { "passive-observation" },
            "applied_spikes": self.applied_spikes,
            "applied": self.applied,
            "expired": self.expired,
            "exchanges": self.client.as_ref().map(|c| c.status().exchanges),
            // Brain output encoding negotiated in the handshake (selects the
            // C. elegans muscle decoder: twitch for spikes, linear for pulse density).
            "output_code": self.client.as_ref().map(|c| c.status().output_code),
            // Full generalised position for exact frame-by-frame replay.
            "qpos": self.physics.qpos(),
            "muscle_activation": self.actuation.muscle_activations().map(|rows| rows.iter().flatten().copied().collect::<Vec<_>>()),
        }))
    }
    /// Static trace header naming every recorded vector element.
    pub fn trace_header(&self) -> serde_json::Value {
        let segments: Vec<(String, usize)> = (0..self.physics.body_count())
            .map_while(|i| {
                let name = format!("segment_{i:02}");
                self.physics.body(&name).ok().map(|id| (name, id))
            })
            .collect();
        serde_json::json!({
            "schema": "aarnn-mujoco-trace-v2",
            "muscle_activation_layout": "row 01 (head) .. 24 (tail) x [MDL, MDR, MVL, MVR]",
            "instance_id": self.instance_id,
            "profile": self.model.profile_id,
            "brain_id": self.brain_id,
            "model_digest": self.model.digest,
            "io_digest": self.model.io.digest,
            "body_count": self.physics.body_count(),
            "segment_body_ids": segments,
            "actuated_joints": self.model.actuators.iter().map(|a| &a.joint).collect::<Vec<_>>(),
            "sensor_names": self.model.io.sensors.iter().map(|s| &s.name).collect::<Vec<_>>(),
            "output_names": self.model.io.output_names,
            "scope": "read-only physics/IO observation; no production commit or effect authority",
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
        self.last_sensors.clear();
        self.vibration_step = None;
        self.energy_j = 20000.;
        self.actuation.stop();
        // A declared launch velocity always fits its own validated model.
        let _ = self.launch();
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
        self.last_sensors.clear();
        self.vibration_step = None;
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
                        "body_gravity" => inverse(r, [0., 0., -1.])
                            [*sensor.axis.as_ref().ok_or("missing gravity axis")?],
                        "range_ray" => {
                            let angle = sensor.angle_rad.ok_or("missing ray direction")?;
                            let delta = rotation(r, [0., 0., 0.1]);
                            let point = std::array::from_fn(|i| position[i] + delta[i]);
                            self.physics
                                .ray_beyond_self(
                                    point,
                                    rotation(r, [angle.cos(), angle.sin(), 0.]),
                                    1,
                                )?
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
    /// Normalised body-bend sample `k` (1 = head .. 12 = tail) in radians.
    ///
    /// Hinge `celegans_spine_NN` sits at the centre of segment `NN - 1`
    /// (tail = 0, head = 23), i.e. at body fraction `(23.5 - s) / 24` from
    /// the nose. The sample interpolates linearly between the two nearest
    /// hinges. Positive angles are ventral (see the v3 muscle transducer).
    fn celegans_bend(&self, k: usize) -> Result<f64, String> {
        let ap = 0.06 + 0.88 * (k.saturating_sub(1)) as f64 / 11.0;
        let mut hinges: Vec<(f64, f64)> = self
            .model
            .actuators
            .iter()
            .zip(&self.joint_indices)
            .filter_map(|(a, (q, _))| {
                let n: f64 = a.joint.strip_prefix("celegans_spine_")?.parse().ok()?;
                Some(((23.5 - (n - 1.)) / 24., self.physics.qpos()[*q]))
            })
            .collect();
        if hinges.is_empty() {
            return Err("C. elegans bend requires named spine hinges".into());
        }
        hinges.sort_by(|a, b| a.0.total_cmp(&b.0));
        if ap <= hinges[0].0 {
            return Ok(hinges[0].1);
        }
        for w in hinges.windows(2) {
            if ap <= w[1].0 {
                let t = (ap - w[0].0) / (w[1].0 - w[0].0);
                return Ok(w[0].1 + t * (w[1].1 - w[0].1));
            }
        }
        Ok(hinges[hinges.len() - 1].1)
    }
    /// Sensor mount point of a body-attached C. elegans channel.
    fn celegans_mount(&self, s: &aarnn_embodiment::aer::Sensor) -> Result<SensorMount, String> {
        let body = self.physics.body(s.body.as_deref().ok_or("sensor body")?)?;
        let (p, r, _) = self.physics.body_state(body)?;
        let offset = rotation(r, s.position_m.ok_or("sensor mount")?);
        Ok((body, std::array::from_fn(|i| p[i] + offset[i]), r))
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
        // v2 vibration: rectified high-pass of proper acceleration (tau 0.2 s),
        // so the static gravity reference no longer drives the touch cells.
        // The filter advances once per physics step even if sampled twice.
        if self.vibration_step != Some(self.step) {
            let alpha = 1. - (-0.002_f64 / 0.2).exp();
            if self.vibration_step.is_none() {
                self.vibration_lp = accel;
            }
            for (lp, a) in self.vibration_lp.iter_mut().zip(accel) {
                *lp += alpha * (a - *lp);
            }
            self.vibration_step = Some(self.step);
        }
        let empty = Vec::new();
        let stimuli = self
            .model
            .scene
            .as_ref()
            .map(|s| &s.stimuli)
            .unwrap_or(&empty);
        let sensors = self.model.io.sensors.clone();
        sensors
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
                    "celegans-vibration-v2" => {
                        let axis = s.axis.ok_or("sensor axis")?;
                        // Full scale 20 m/s^2: a plate tap (~30 m/s^2 above 1 g) saturates,
                        // the worm's own wriggling (~1 m/s^2) only ~0.05.
                        ((accel[axis] - self.vibration_lp[axis]).abs() / 20.).clamp(0., 1.)
                    }
                    "celegans-rotation-v2" => {
                        let axis = s.axis.ok_or("sensor axis")?;
                        // Full scale 10 rad/s (undulation turns at ~1-2 rad/s).
                        (gyro[axis].abs() / 10.).clamp(0., 1.)
                    }
                    "celegans-contact-v1" => self.physics.object_contact(
                        self.physics.body(s.body.as_deref().ok_or("sensor body")?)?,
                    ),
                    "celegans-field-v2" => {
                        let (_, point, _) = self.celegans_mount(s)?;
                        let field = s.mode.as_deref().ok_or("field sensor mode")?;
                        aarnn_embodiment::aer::field_value(stimuli, field, point[0], point[1])
                    }
                    "celegans-range-v2" => {
                        let (body, point, r) = self.celegans_mount(s)?;
                        let angle = s.angle_rad.ok_or("range direction")?;
                        let direction = rotation(r, [angle.cos(), angle.sin(), 0.]);
                        // Group 1 is the worm's own cuticle: never self-occlude.
                        let distance = self
                            .physics
                            .ray_groups(point, direction, body, [1, 0, 0, 1, 1, 1])?
                            .unwrap_or(s.range[1]);
                        1. - normal(distance, s.range)
                    }
                    "celegans-bend-v1" => {
                        let k = s.axis.ok_or("bend sample")?;
                        let theta = self.celegans_bend(k)?;
                        let reach = s.range[1].max(1.0e-6);
                        match s.mode.as_deref() {
                            Some("dorsal") => (-theta / reach).clamp(0., 1.),
                            Some("ventral") => (theta / reach).clamp(0., 1.),
                            _ => return Err("bend side".into()),
                        }
                    }
                    "celegans-range-v1" | "celegans-irradiance-v1" => {
                        let (body, point, r) = self.celegans_mount(s)?;
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
    /// Passive substrate and plate stimuli for the worm: resistive-force
    /// (agar groove) drag on every segment and the periodic plate tap.
    /// Applied identically with or without a brain, so the passive control
    /// shares the mechanics exactly.
    pub fn celegans_substrate_forces(&mut self) -> Result<(), String> {
        let Some(scene) = self.model.scene.clone() else {
            return Ok(());
        };
        let tap = scene
            .taps
            .as_ref()
            .is_some_and(|t| t.active(self.physics.time()));
        let tap_accel = scene
            .taps
            .as_ref()
            .map(|t| t.acceleration_mps2)
            .unwrap_or(0.);
        for b in 1..self.physics.body_count() {
            let (_, r, v) = self.physics.body_state(b)?;
            let com = self.physics.center_of_mass(b)?;
            let mut force = [0.; 3];
            if let Some(rft) = &scene.resistive_force {
                // Body x-axis in world coordinates, projected to the plane.
                let t = [r[0][0], r[1][0], 0.];
                let n = (t[0] * t[0] + t[1] * t[1]).sqrt();
                if n > 1.0e-9 {
                    let t = [t[0] / n, t[1] / n];
                    let vel = [v[3], v[4]];
                    let vt = vel[0] * t[0] + vel[1] * t[1];
                    let vn = [vel[0] - vt * t[0], vel[1] - vt * t[1]];
                    force[0] = -rft.tangential * vt * t[0] - rft.normal * vn[0];
                    force[1] = -rft.tangential * vt * t[1] - rft.normal * vn[1];
                }
            }
            if tap {
                force[2] += self.physics.body_mass(b)? * tap_accel;
            }
            if force.iter().any(|f| *f != 0.) {
                self.physics.force_at(b, force, com)?;
            }
        }
        Ok(())
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
        let applied = self.actuation.applied;
        self.actuation.select(
            &self.model,
            self.step,
            self.npc_enabled,
            neural_ready,
            reply.as_ref(),
            observations.as_deref(),
        )?;
        if self.actuation.applied > applied
            && let Some(reply) = &reply
        {
            for (total, spikes) in self.applied_spikes.iter_mut().zip(&reply.spikes) {
                *total += u64::from(*spikes);
            }
        }
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
        if self.model.profile_id == "celegans" {
            self.celegans_substrate_forces()?;
        }
        if let Some(water) = self.water {
            // resistive-ellipsoid-v1 (scene hydrodynamics): neutral buoyancy at
            // each body's COM plus anisotropic quadratic drag on each declared
            // ellipsoid, identical with or without a brain.
            for body in 1..self.physics.body_count() {
                let (_, r, _) = self.physics.body_state(body)?;
                let com = self.physics.center_of_mass(body)?;
                let shift = rotation(r, self.buoyancy_offset);
                let centre: [f64; 3] = std::array::from_fn(|i| com[i] + shift[i]);
                let mass = self.physics.body_mass(body)?;
                self.physics
                    .force_at(body, water.buoyancy(mass, centre[2]), centre)?;
            }
            for &(body, half, offset) in &self.hydro {
                let (position, r, velocity) = self.physics.body_state(body)?;
                let com = self.physics.center_of_mass(body)?;
                let shift = rotation(r, offset);
                let centre: [f64; 3] = std::array::from_fn(|i| position[i] + shift[i]);
                let angular = [velocity[0], velocity[1], velocity[2]];
                let arm: [f64; 3] = std::array::from_fn(|i| centre[i] - com[i]);
                let spin = cross(angular, arm);
                let linear: [f64; 3] = std::array::from_fn(|i| velocity[i + 3] + spin[i]);
                let (force, torque) = aarnn_embodiment::force::ellipsoid_drag(
                    &water,
                    half,
                    r,
                    linear,
                    angular,
                    water.submersion(centre[2]),
                )
                .map_err(str::to_string)?;
                self.physics.wrench_at(body, force, torque, centre)?;
            }
        } else if self.model.profile_id == "zebrafish" {
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
        for &(body, local, frame, area, polar) in &self.aero {
            let (position, r, velocity) = self.physics.body_state(body)?;
            let offset = rotation(r, local);
            let point = std::array::from_fn(|i| position[i] + offset[i]);
            let com = self.physics.center_of_mass(body)?;
            let arm = std::array::from_fn(|i| point[i] - com[i]);
            let rotational = cross([velocity[0], velocity[1], velocity[2]], arm);
            let relative = std::array::from_fn(|i| velocity[i + 3] + rotational[i]);
            // Panel axes in world coordinates: body rotation times panel frame.
            let axes: [[f64; 3]; 3] = std::array::from_fn(|i| {
                std::array::from_fn(|j| (0..3).map(|k| r[i][k] * frame[k][j]).sum())
            });
            let force = aarnn_embodiment::force::polar_panel_force(
                relative,
                axes,
                area,
                self.air_density,
                &polar,
            )
            .map_err(str::to_string)?;
            self.physics.force_at(body, force, point)?;
        }
        if let Some((flight, joints)) = &self.flight {
            // Flight motor torques from the decoded commands (zero, i.e.
            // folded wings, whenever actuation is disarmed).
            let (q, v) = (self.physics.qpos(), self.physics.qvel());
            let state = joints.map(|w| w.map(|(qi, di)| [q[qi], v[di]]));
            let torques = flight.torques(self.physics.time(), self.actuation.commands(), state)?;
            for (wing, tau) in joints.iter().zip(torques) {
                for ((_, dof), t) in wing.iter().zip(tau) {
                    self.physics.add_joint_force(*dof, t)?;
                }
            }
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
            let values = self.sensors()?;
            self.last_sensors.clone_from(&values);
            let capture = Capture {
                step: self.step,
                timestamp_us: self.step.checked_mul(2000).ok_or("source clock overflow")?,
                values,
            };
            if self.client.as_ref().unwrap().submit(capture) {
                self.submitted += 1;
            }
        }
        Ok(())
    }
}
