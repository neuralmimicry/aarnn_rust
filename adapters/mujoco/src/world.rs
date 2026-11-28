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
    refall: Option<Refall>,
    release: Option<Release>,
    /// Energy needs (`needs-v1`), when the scene declares them and
    /// `NM_MUJOCO_NEEDS` is not `0`.
    pub needs: Option<NeedsState>,
}
/// Runtime energy needs of a body (see `aarnn_embodiment::aer::Needs`).
/// The store itself is `World::energy_j`.
pub struct NeedsState {
    pub cfg: aarnn_embodiment::aer::Needs,
    pub mass_kg: f64,
    pub capacity_j: f64,
    pub basal_w: f64,
    pub effort_ref_w: f64,
    pub refill_w: f64,
    /// 1 while energy flows in this step, else 0.
    pub consume: f64,
    /// Simulated time until which the caretaker is feeding / charging.
    pub feeding_until_s: f64,
    pub caretaker_until_s: f64,
    pub caretaker_feeds: u64,
    /// Source the body is at, and arrivals at a source so far.
    pub at_source: Option<usize>,
    pub source_visits: u64,
    pub consumed_j: f64,
    pub exhausted: bool,
    /// Intake in progress at a source (started below `refill_start_fraction`,
    /// continues to full).
    pub refilling: bool,
    /// Foraging trials (`NM_MUJOCO_RELEASE_*`): releases so far.
    pub releases: u64,
}
impl NeedsState {
    fn from_record(model: &Model, physics: &Physics) -> Result<Option<Self>, String> {
        let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f64>().ok());
        if std::env::var("NM_MUJOCO_NEEDS").is_ok_and(|v| v == "0") {
            return Ok(None);
        }
        let Some(mut cfg) = model.scene.as_ref().and_then(|s| s.needs.clone()) else {
            return Ok(None);
        };
        if let Some(v) = env("NM_MUJOCO_NEEDS_ENDURANCE_S") {
            cfg.endurance_s = v;
        }
        if let Some(v) = env("NM_MUJOCO_NEEDS_START") {
            cfg.start_fraction = v;
        }
        if !(cfg.endurance_s > 0.
            && cfg.refill_s > 0.
            && cfg.basal_w_per_kg > 0.
            && cfg.effort_ref_w_per_kg > 0.)
            || !(0. ..=1.).contains(&cfg.start_fraction)
        {
            return Err("invalid needs parameters".into());
        }
        let mut mass_kg = 0.;
        for b in 1..physics.body_count() {
            mass_kg += physics.body_mass(b)?;
        }
        let mass_kg = f64::max(mass_kg, 1e-6);
        let basal_w = cfg.basal_w_per_kg * mass_kg;
        let capacity_j = basal_w * cfg.endurance_s;
        let caretaker_until_s = env("NM_MUJOCO_CARETAKER_S")
            .or(cfg.caretaker.as_ref().map(|c| c.until_s))
            .unwrap_or(0.);
        Ok(Some(Self {
            mass_kg,
            capacity_j,
            basal_w,
            effort_ref_w: cfg.effort_ref_w_per_kg * mass_kg,
            refill_w: capacity_j / cfg.refill_s,
            consume: 0.,
            feeding_until_s: f64::NEG_INFINITY,
            caretaker_until_s,
            caretaker_feeds: 0,
            at_source: None,
            source_visits: 0,
            consumed_j: 0.,
            exhausted: false,
            refilling: false,
            releases: 0,
            cfg,
        }))
    }
    fn restart(&mut self) {
        self.consume = 0.;
        self.feeding_until_s = f64::NEG_INFINITY;
        self.caretaker_feeds = 0;
        self.at_source = None;
        self.source_visits = 0;
        self.consumed_j = 0.;
        self.exhausted = false;
        self.refilling = false;
        self.releases = 0;
    }
    fn start_energy(&self) -> f64 {
        self.capacity_j * self.cfg.start_fraction
    }
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
        // Habitat variants: NM_MUJOCO_REMOVE_GEOMS=prefix,... takes scenery out
        // (e.g. the hexapod's slalom posts, which its sonar cone misses).
        if let Ok(list) = std::env::var("NM_MUJOCO_REMOVE_GEOMS") {
            let prefixes: Vec<&str> = list.split(',').map(str::trim).collect();
            let n = physics.remove_geoms(&prefixes)?;
            eprintln!(
                "{{\"habitat_removed_geoms\": {n}, \"prefixes\": {:?}}}",
                prefixes
            );
        }
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
            refall: Refall::from_env()?,
            release: Release::from_env()?,
            needs: None,
        };
        world.needs = NeedsState::from_record(&world.model, &world.physics)?;
        if let Some(n) = &world.needs {
            world.energy_j = n.start_energy();
        }
        world.launch()?;
        world.start_pose()?;
        Ok(world)
    }
    /// Test harness for fall recovery: `NM_MUJOCO_START_POSE=front|back`
    /// starts the free root lying chest-down or chest-up (pitched +-90 deg
    /// about its own y axis, as `scripts/qa/nao_getup_probe.py` lays the NAO
    /// down) at `NM_MUJOCO_START_Z` metres (default 0.11), at rest. Unset or
    /// `stand`: the model's own start. `NM_MUJOCO_START_JOINTS` (a JSON file
    /// `{joint: rad}`) sets joint angles with it, e.g. the NAO lying in
    /// StandInit rather than in the zero pose, whose arms point into the
    /// floor face-down (24 Nov: the body was thrown onto its back).
    /// Applied at load and after every reset.
    fn start_pose(&mut self) -> Result<(), String> {
        let pose = std::env::var("NM_MUJOCO_START_POSE").unwrap_or_default();
        let phi = match pose.as_str() {
            "" | "stand" => return Ok(()),
            "front" => 0.,
            "back" => std::f64::consts::PI,
            other => {
                return Err(format!(
                    "NM_MUJOCO_START_POSE {other:?}: expected front, back or stand"
                ));
            }
        };
        let z = match std::env::var("NM_MUJOCO_START_Z") {
            Ok(v) => v
                .parse::<f64>()
                .ok()
                .filter(|z| z.is_finite() && *z > 0.)
                .ok_or(format!(
                    "NM_MUJOCO_START_Z {v:?}: expected a height in metres"
                ))?,
            Err(_) => 0.11,
        };
        self.lay_down(phi, 0., 0., 0., 0., z)
    }
    /// Lay the free root down: the upright body rotated by `phi` about its
    /// own long axis (0 chest-down, pi chest-up, +-pi/2 on a side), its long
    /// axis horizontal and pitched by `tilt`, the head heading `yaw`, at
    /// (`x`, `y`, `z`), at rest; joints from `NM_MUJOCO_START_JOINTS` when
    /// set. Used by the start pose and the fall generator.
    /// Foraging trial release: the free root is put upright at `[x, y, yaw]`
    /// (height `z` when given, else kept; velocities zero, joints unchanged)
    /// and the energy reserve set to the configured fraction; the brain keeps
    /// running.
    fn release_at(&mut self, place: [f64; 5]) -> Result<(), String> {
        let [x, y, yaw, z, fraction] = place;
        let (nq, nv, _) = self.physics.dimensions();
        let mut state = self.physics.snapshot();
        if nq < 7 || nv < 6 || state.len() < 1 + nq + nv {
            return Err("trial release requires a free root".into());
        }
        state[1] = x;
        state[2] = y;
        if z.is_finite() {
            state[3] = z;
        }
        state[4..8].copy_from_slice(&[(yaw / 2.).cos(), 0., 0., (yaw / 2.).sin()]);
        state[1 + nq..1 + nq + nv].fill(0.);
        self.physics.restore(&state)?;
        let t = self.physics.time();
        if let Some(n) = self.needs.as_mut() {
            self.energy_j = (fraction.clamp(0., 1.)) * n.capacity_j;
            n.refilling = false;
            // A caretaker feed still running at the first release (care ends
            // at 400 s, a feed lasts 20 s) would carry into the trial: cue at
            // 1 and the feeding reflex halting the body (26 Nov).
            n.feeding_until_s = f64::NEG_INFINITY;
            n.releases += 1;
        }
        eprintln!(
            "{{\"release\": {{\"t\": {t:.3}, \"x\": {x:.3}, \"y\": {y:.3}, \"yaw\": {yaw:.3}, \"energy\": {fraction:.3}}}}}"
        );
        Ok(())
    }
    fn lay_down(
        &mut self,
        phi: f64,
        tilt: f64,
        x: f64,
        y: f64,
        yaw: f64,
        z: f64,
    ) -> Result<(), String> {
        fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
            [
                a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
                a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
                a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
                a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
            ]
        }
        let (nq, nv, _) = self.physics.dimensions();
        let mut state = self.physics.snapshot();
        if nq < 7 || nv < 6 || state.len() < 1 + nq + nv {
            return Err("lying pose requires a free root".into());
        }
        let pitch = std::f64::consts::FRAC_PI_2 + tilt;
        let qy = [(yaw / 2.).cos(), 0., 0., (yaw / 2.).sin()];
        let qp = [(pitch / 2.).cos(), 0., (pitch / 2.).sin(), 0.];
        let qz = [(phi / 2.).cos(), 0., 0., (phi / 2.).sin()];
        let q = qmul(qmul(qy, qp), qz);
        state[1] = x;
        state[2] = y;
        state[3] = z;
        state[4..8].copy_from_slice(&q);
        state[1 + nq..1 + nq + nv].fill(0.);
        if let Ok(path) = std::env::var("NM_MUJOCO_START_JOINTS") {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("NM_MUJOCO_START_JOINTS {path}: {e}"))?;
            let joints: std::collections::BTreeMap<String, f64> = serde_json::from_str(&text)
                .map_err(|e| format!("NM_MUJOCO_START_JOINTS {path}: {e}"))?;
            for (name, angle) in joints {
                if !angle.is_finite() {
                    return Err(format!("NM_MUJOCO_START_JOINTS: {name} is not finite"));
                }
                let (qadr, _) = self.physics.joint(&name)?;
                state[1 + qadr] = angle;
            }
        }
        self.physics.restore(&state)
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
            let mut values = self.celegans_sensors()?;
            self.fill_needs(&mut values)?;
            self.last_sensors = values;
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
            // Energy needs: store (J), fraction, refill this step, caretaker
            // feeding, source reached, cumulative events.
            "needs": self.needs.as_ref().map(|n| serde_json::json!({
                "energy_j": self.energy_j,
                "fraction": self.energy_j / n.capacity_j,
                "consume": n.consume,
                "caretaker_feeding": self.physics.time() < n.feeding_until_s,
                "at_source": n.at_source.map(|i| &n.cfg.sources[i].id),
                "caretaker_feeds": n.caretaker_feeds,
                "source_visits": n.source_visits,
                "consumed_j": n.consumed_j,
                "exhausted": n.exhausted,
                "releases": n.releases,
            })),
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
            "needs": self.needs.as_ref().map(|n| serde_json::json!({
                "version": n.cfg.version,
                "resource": n.cfg.resource,
                "mass_kg": n.mass_kg,
                "capacity_j": n.capacity_j,
                "basal_w": n.basal_w,
                "refill_w": n.refill_w,
                "caretaker_until_s": n.caretaker_until_s,
                "sources": n.cfg.sources.iter().map(|s| serde_json::json!({"id": s.id, "position_m": s.position_m, "radius_m": s.radius_m})).collect::<Vec<_>>(),
            })),
            "scope": "read-only physics/IO observation; no production commit or effect authority",
        })
    }
    pub fn measured_energy(&self) -> Option<f64> {
        (self.model.profile_id == "crow_corvus_corone" || self.needs.is_some())
            .then_some(self.energy_j)
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
        if let Some(n) = self.needs.as_mut() {
            n.restart();
            self.energy_j = n.start_energy();
        }
        self.actuation.stop();
        // A declared launch velocity always fits its own validated model.
        let _ = self.launch();
        let _ = self.start_pose();
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
            || !(0. ..=self.needs.as_ref().map_or(20000., |n| n.capacity_j))
                .contains(&saved.energy_j)
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
    /// Moving sound sources for this step (`legacy-acoustic` receivers).
    pub fn set_dynamic_sound_sources(
        &mut self,
        sources: Vec<aarnn_embodiment::acoustics::SoundSource>,
    ) {
        self.legacy.dynamic_sound_sources = sources;
    }
    pub fn sensors(&mut self) -> Result<Vec<f64>, String> {
        let mut values = self.sensors_raw()?;
        self.fill_needs(&mut values)?;
        Ok(values)
    }
    /// Energy-needs channels (`needs-*`, optional group `needs`): reserve,
    /// refill and the left/right cue field. The engine paths leave them 0.
    fn fill_needs(&self, values: &mut [f64]) -> Result<(), String> {
        for (i, s) in self.model.io.sensors.iter().enumerate() {
            if !s.kind.starts_with("needs-") {
                continue;
            }
            let v = match (s.kind.as_str(), self.needs.as_ref()) {
                ("needs-energy", Some(n)) => self.energy_j / n.capacity_j,
                // Without a needs model the store is never drawn down.
                ("needs-energy", None) => 1.,
                ("needs-consume", Some(n)) => n.consume,
                ("needs-cue", Some(n)) => self.needs_cue(s, n)?,
                _ => 0.,
            };
            if let Some(slot) = values.get_mut(i) {
                *slot = v.clamp(0., 1.);
            }
        }
        Ok(())
    }
    /// Odour / beacon intensity at a cue sensor: the strongest source's
    /// Gaussian field (horizontal distance) weighted by the receptor's
    /// direction (`0.5 + 0.5 cos` of the bearing). A sum saturated wherever
    /// food is plentiful (the worm's seven lawns and spots read 1 everywhere). Food presented by
    /// the caretaker is at the mouth: full intensity on both sides.
    fn needs_cue(&self, s: &aarnn_embodiment::aer::Sensor, n: &NeedsState) -> Result<f64, String> {
        if self.physics.time() < n.feeding_until_s {
            return Ok(1.);
        }
        let body = match s.body.as_deref() {
            Some(name) => self.physics.body(name)?,
            None => 1,
        };
        let (position, r, _) = self.physics.body_state(body)?;
        let offset = rotation(r, s.position_m.unwrap_or([0.; 3]));
        let point: [f64; 3] = std::array::from_fn(|i| position[i] + offset[i]);
        let aim = rotation(r, s.direction.unwrap_or([1., 0., 0.]));
        let aim_len = aim[0].hypot(aim[1]).max(1e-9);
        let mut value = 0.;
        for src in &n.cfg.sources {
            let (dx, dy) = (src.position_m[0] - point[0], src.position_m[1] - point[1]);
            let dist = dx.hypot(dy);
            let sigma = src.field_radius_m.max(1e-6);
            let field = (-(dist * dist) / (2. * sigma * sigma)).exp();
            let cos = if dist > 1e-9 {
                (dx * aim[0] + dy * aim[1]) / (dist * aim_len)
            } else {
                1.
            };
            value = f64::max(value, field * (0.5 + 0.5 * cos));
        }
        Ok(value.min(1.))
    }
    /// One 2 ms step of the energy store: basal drain x (1 + effort), refill
    /// at a source or while the caretaker feeds (only below full), caretaker
    /// onset below its threshold in early life, exhaustion at zero.
    fn update_needs(&mut self) -> Result<(), String> {
        let t = self.physics.time();
        let (root, _, _) = self.physics.body_state(1)?;
        let mechanical = self.physics.mechanical_power();
        let Some(n) = self.needs.as_mut() else {
            return Ok(());
        };
        let effort = (mechanical / n.effort_ref_w.max(1e-9)).clamp(0., 1.);
        let drain_w = n.basal_w * (1. + effort);
        let here = n.cfg.sources.iter().position(|s| {
            (s.position_m[0] - root[0]).hypot(s.position_m[1] - root[1]) <= s.radius_m
        });
        if here.is_some() && here != n.at_source {
            n.source_visits += 1;
        }
        n.at_source = here;
        if let Some(c) = &n.cfg.caretaker {
            if t < n.caretaker_until_s
                && t >= n.feeding_until_s
                && self.energy_j < c.threshold * n.capacity_j
            {
                n.feeding_until_s = t + c.feed_s;
                n.caretaker_feeds += 1;
            }
        }
        let feeding = t < n.feeding_until_s;
        // At a source, intake starts below the refill threshold and continues
        // to full (hysteresis); the caretaker feeds whenever it feeds.
        let fraction = self.energy_j / n.capacity_j;
        if here.is_none() || fraction >= 1. {
            n.refilling = false;
        } else if fraction < n.cfg.refill_start_fraction {
            n.refilling = true;
        }
        let inflow_w = if (n.refilling || feeding) && self.energy_j < n.capacity_j {
            n.refill_w
        } else {
            0.
        };
        n.consume = if inflow_w > 0. { 1. } else { 0. };
        n.consumed_j += inflow_w * 0.002;
        self.energy_j = (self.energy_j + (inflow_w - drain_w) * 0.002).clamp(0., n.capacity_j);
        let exhausted = self.energy_j <= 0. && !n.exhausted;
        if exhausted {
            n.exhausted = true;
            eprintln!(
                "{{\"needs_exhausted\": {{\"t\": {t:.3}, \"resource\": \"{}\", \"caretaker_feeds\": {}, \"source_visits\": {}}}}}",
                n.cfg.resource, n.caretaker_feeds, n.source_visits
            );
        }
        if exhausted {
            self.stop();
        }
        Ok(())
    }
    fn sensors_raw(&mut self) -> Result<Vec<f64>, String> {
        if self.model.profile_id == "celegans" {
            return self.celegans_sensors();
        }
        if self
            .model
            .io
            .sensors
            .iter()
            .all(|s| s.kind.starts_with("legacy-") || s.kind.starts_with("needs-"))
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
                        // Filled by `fill_needs` after the engine paths.
                        k if k.starts_with("needs-") => 0.,
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
        // The caretaker presents food at the nose: while it feeds, the worm's
        // own food senses (soluble taste, volatile odour) read the food.
        let caretaker_feeding = self
            .needs
            .as_ref()
            .is_some_and(|n| self.physics.time() < n.feeding_until_s);
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
                    k if k.starts_with("needs-") => 0.,
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
                    "celegans-contact-v1" => match &s.bodies {
                        // Receptive field: any segment of the touch cell's half.
                        Some(names) => {
                            let mut v = 0.;
                            for name in names {
                                v = f64::max(
                                    v,
                                    self.physics.object_contact(self.physics.body(name)?),
                                );
                            }
                            v
                        }
                        None => self.physics.object_contact(
                            self.physics.body(s.body.as_deref().ok_or("sensor body")?)?,
                        ),
                    },
                    "celegans-field-v2" => {
                        let (_, point, _) = self.celegans_mount(s)?;
                        let field = s.mode.as_deref().ok_or("field sensor mode")?;
                        let value =
                            aarnn_embodiment::aer::field_value(stimuli, field, point[0], point[1]);
                        if caretaker_feeding
                            && matches!(field, "attractant_soluble" | "attractant_volatile")
                        {
                            value.max(1.)
                        } else {
                            value
                        }
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
        if let Some(fall) = self
            .refall
            .as_mut()
            .and_then(|r| r.due(self.physics.time()))
        {
            eprintln!(
                "{{\"refall\": {{\"t\": {:.3}, \"phi\": {:.3}, \"tilt\": {:.3}, \"x\": {:.3}, \"y\": {:.3}, \"yaw\": {:.3}}}}}",
                self.physics.time(),
                fall[0],
                fall[1],
                fall[2],
                fall[3],
                fall[4]
            );
            self.lay_down(fall[0], fall[1], fall[2], fall[3], fall[4], 0.14)?;
        }
        if let Some(place) = self
            .release
            .as_mut()
            .and_then(|r| r.due(self.physics.time()))
        {
            self.release_at(place)?;
        }
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
        if self.needs.is_some() && !self.disarmed {
            self.update_needs()?;
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

/// Fall generator for learning by experience: `NM_MUJOCO_REFALL_S` (period,
/// seconds) lays the body down again every period, in a new orientation
/// about its long axis (uniform), a tilt of +-0.2 rad and one of
/// `NM_MUJOCO_REFALL_PLACES` ("x,y,yaw;x,y,yaw;...", default the origin),
/// chosen by a seeded generator (`NM_MUJOCO_REFALL_SEED`, default 1). The
/// brain keeps running: what it learned carries over from fall to fall.
/// Each fall is logged on stderr as one JSON line.
struct Refall {
    period: f64,
    next: f64,
    rng: u64,
    places: Vec<[f64; 3]>,
}

/// Foraging trials: from `NM_MUJOCO_RELEASE_FROM_S`, every
/// `NM_MUJOCO_RELEASE_S` seconds the body is released upright at the next of
/// `NM_MUJOCO_RELEASE_PLACES` ("x,y,yaw[,z];..."; without z the height is
/// kept), in order, with its reserve set to `NM_MUJOCO_RELEASE_ENERGY`
/// (default 0.45, hungry).
struct Release {
    period: f64,
    next: f64,
    energy: f64,
    places: Vec<[f64; 4]>,
    index: usize,
}
impl Release {
    fn from_env() -> Result<Option<Self>, String> {
        let Ok(period) = std::env::var("NM_MUJOCO_RELEASE_S") else {
            return Ok(None);
        };
        let num = |name: &str, default: f64| -> Result<f64, String> {
            match std::env::var(name) {
                Ok(v) => v
                    .parse::<f64>()
                    .map_err(|_| format!("{name} {v:?}: expected a number")),
                Err(_) => Ok(default),
            }
        };
        let period: f64 = period
            .parse()
            .ok()
            .filter(|p: &f64| *p > 0.)
            .ok_or(format!("NM_MUJOCO_RELEASE_S {period:?}: expected seconds"))?;
        let places = std::env::var("NM_MUJOCO_RELEASE_PLACES")
            .map_err(|_| "NM_MUJOCO_RELEASE_PLACES required with NM_MUJOCO_RELEASE_S")?
            .split(';')
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                let v: Vec<f64> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                match v[..] {
                    [x, y, yaw] => Ok([x, y, yaw, f64::NAN]),
                    [x, y, yaw, z] => Ok([x, y, yaw, z]),
                    _ => Err(format!(
                        "NM_MUJOCO_RELEASE_PLACES entry {p:?}: expected x,y,yaw[,z]"
                    )),
                }
            })
            .collect::<Result<Vec<[f64; 4]>, String>>()?;
        if places.is_empty() {
            return Err("NM_MUJOCO_RELEASE_PLACES has no places".into());
        }
        Ok(Some(Self {
            period,
            next: num("NM_MUJOCO_RELEASE_FROM_S", period)?,
            energy: num("NM_MUJOCO_RELEASE_ENERGY", 0.45)?,
            places,
            index: 0,
        }))
    }
    /// `[x, y, yaw, z, energy]` (z NaN: keep the height) when a release is
    /// due at time `t`.
    fn due(&mut self, t: f64) -> Option<[f64; 5]> {
        if t < self.next {
            return None;
        }
        self.next += self.period;
        let p = self.places[self.index % self.places.len()];
        self.index += 1;
        Some([p[0], p[1], p[2], p[3], self.energy])
    }
}

impl Refall {
    fn from_env() -> Result<Option<Self>, String> {
        let Ok(period) = std::env::var("NM_MUJOCO_REFALL_S") else {
            return Ok(None);
        };
        let period: f64 = period
            .parse()
            .ok()
            .filter(|p: &f64| p.is_finite() && *p > 0.)
            .ok_or(format!("NM_MUJOCO_REFALL_S {period:?}: expected seconds"))?;
        let seed: u64 = std::env::var("NM_MUJOCO_REFALL_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let places = match std::env::var("NM_MUJOCO_REFALL_PLACES") {
            Ok(text) => text
                .split(';')
                .filter(|p| !p.trim().is_empty())
                .map(|p| {
                    let v: Vec<f64> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    (v.len() == 3 && v.iter().all(|x| x.is_finite()))
                        .then(|| [v[0], v[1], v[2]])
                        .ok_or(format!(
                            "NM_MUJOCO_REFALL_PLACES entry {p:?}: expected x,y,yaw"
                        ))
                })
                .collect::<Result<Vec<_>, _>>()?,
            Err(_) => vec![[0., 0., 0.]],
        };
        if places.is_empty() {
            return Err("NM_MUJOCO_REFALL_PLACES has no places".into());
        }
        Ok(Some(Self {
            period,
            next: period,
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            places,
        }))
    }
    fn uniform(&mut self) -> f64 {
        // xorshift64*: reproducible falls without a dependency.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    /// [phi, tilt, x, y, yaw] when a fall is due at time `t`.
    fn due(&mut self, t: f64) -> Option<[f64; 5]> {
        if t < self.next {
            return None;
        }
        self.next += self.period;
        let phi = (self.uniform() * 2. - 1.) * std::f64::consts::PI;
        let tilt = (self.uniform() * 2. - 1.) * 0.2;
        let u = self.uniform();
        let n = self.places.len();
        let place = self.places[((u * n as f64) as usize).min(n - 1)];
        Some([phi, tilt, place[0], place[1], place[2]])
    }
}
