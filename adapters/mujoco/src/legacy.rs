// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Native measurements for preserved legacy names. Engineering optical/range
//! proxies and proposed readouts are versioned separately from neural state.
use crate::native::Physics;
use aarnn_embodiment::{aer::Model, vision::Cell};
use std::collections::HashMap;

fn rotate(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| r[i][j] * v[j]).sum())
}
fn local(r: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| r[j][i] * v[j]).sum())
}
fn norm(v: f64, a: f64, b: f64) -> f64 {
    ((v - a) / (b - a)).clamp(0., 1.)
}

#[derive(Default)]
pub struct Senses {
    cells: HashMap<String, Vec<Cell>>,
    step: Option<u64>,
    values: Vec<f64>,
    /// Sample-and-hold readings (`period_s`): sensor index -> (time, value).
    held: HashMap<usize, (f64, f64)>,
    /// Moving sound sources this step (an NPC, other robots), added to the
    /// scene's static ones for `legacy-acoustic` receivers.
    pub dynamic_sound_sources: Vec<aarnn_embodiment::acoustics::SoundSource>,
}

/// World-frame receiver of a `legacy-acoustic` sensor.
fn acoustic_receiver(
    physics: &Physics,
    s: &aarnn_embodiment::aer::Sensor,
) -> Result<aarnn_embodiment::acoustics::Receiver, String> {
    let body = physics.body(s.body.as_deref().ok_or("acoustic sensor body absent")?)?;
    let (position, r, _) = physics.body_state(body)?;
    let delta = rotate(r, s.position_m.unwrap_or([0.; 3]));
    Ok(aarnn_embodiment::acoustics::Receiver {
        position: std::array::from_fn(|i| position[i] + delta[i]),
        axis: rotate(r, s.direction.unwrap_or([1., 0., 0.])),
        directivity: s.directivity.unwrap_or(0.),
    })
}
impl Senses {
    pub fn measure(
        &mut self,
        physics: &mut Physics,
        model: &Model,
        step: u64,
    ) -> Result<Vec<f64>, String> {
        if self.step == Some(step) {
            return Ok(self.values.clone());
        }
        let mut cameras: HashMap<String, Vec<[f64; 4]>> = HashMap::new();
        let mut out = Vec::with_capacity(model.io.sensors.len());
        let now = physics.time();
        for (index, s) in model.io.sensors.iter().enumerate() {
            // Energy-needs channels are filled by the world after this pass.
            if s.kind.starts_with("needs-") {
                out.push(0.);
                continue;
            }
            if let (Some(period), Some(&(at, value))) = (s.period_s, self.held.get(&index)) {
                if now >= at && now - at < period - 1e-9 {
                    out.push(value);
                    continue;
                }
            }
            let body = physics.body(s.body.as_deref().ok_or("legacy sensor body absent")?)?;
            let (position, r, velocity) = physics.body_state(body)?;
            let axis = s.axis.unwrap_or(0);
            if axis >= 3 {
                return Err("sensor axis outside XYZ".into());
            }
            let v = match s.kind.as_str() {
                "legacy-angle" => {
                    let (q, _) = physics.joint(s.joint.as_deref().ok_or("legacy joint absent")?)?;
                    // Exact DeviceMapper heuristic, deliberately not PI.
                    const HALF_RANGE: f64 = 314. / 100.;
                    norm(physics.qpos()[q], -HALF_RANGE, HALF_RANGE)
                }
                "legacy-accel" | "legacy-gyro" => {
                    let (mut v, bound) = if s.kind == "legacy-accel" {
                        let a = physics.body_acceleration(body)?;
                        ([a[3], a[4], a[5]], 20.)
                    } else {
                        (local(r, [velocity[0], velocity[1], velocity[2]]), 10.)
                    };
                    if s.mode.as_deref() == Some("legacy-y-up") {
                        v = [v[0], v[2], -v[1]];
                    }
                    norm(v[axis], -bound, bound)
                }
                "legacy-contact" => {
                    if let Some(region) = s.region_m {
                        physics.region_contact(body, region)?
                    } else if let Some(direction) = s.direction {
                        physics.directional_contact(body, direction)?
                    } else {
                        physics.body_contact(body)?
                    }
                }
                "legacy-contact-force" => norm(physics.contact_force(body)?[axis], -100., 100.),
                "legacy-range" => {
                    let delta = rotate(r, s.position_m.unwrap_or([0.; 3]));
                    let point = std::array::from_fn(|i| position[i] + delta[i]);
                    let reach = s.angle_rad.ok_or("legacy range reach absent")?;
                    // Rays step over the robot's own links (a fly's thorax-
                    // centred ring otherwise reports its legs as obstacles).
                    // A cone (`rays`) reads its nearest echo; a sonar loses
                    // rays that strike a surface too obliquely.
                    let single = [s.direction.ok_or("legacy ray direction absent")?];
                    let rays = s.rays.as_deref().unwrap_or(&single);
                    if rays.is_empty() {
                        return Err(format!("range {} has no rays", s.name));
                    }
                    let mut distance = reach;
                    for ray in rays {
                        if let Some(d) = physics.echo_beyond_self(
                            point,
                            rotate(r, *ray),
                            body,
                            s.incidence_max_rad,
                        )? {
                            distance = distance.min(d);
                        }
                    }
                    let distance = distance.min(reach).max(s.min_m.unwrap_or(0.));
                    if let Some(table) = &s.lookup {
                        let mut raw = table.last().ok_or("empty range lookup")?[1];
                        for pair in table.windows(2) {
                            if distance <= pair[1][0] {
                                let t = norm(distance, pair[0][0], pair[1][0]);
                                raw = pair[0][1] + t * (pair[1][1] - pair[0][1]);
                                break;
                            }
                        }
                        norm(raw, 120., 1000.)
                    } else {
                        1. - distance / reach
                    }
                }
                "legacy-camera" => {
                    let name = s.camera.as_ref().ok_or("legacy camera absent")?;
                    // One camera may feed retinas of different shapes (a fish
                    // eye's luminance cell and its 6 x 8 retina): cache per shape.
                    let [rows, cols] = s.retina_shape.ok_or("camera shape absent")?;
                    let key = format!("{name}#{rows}x{cols}");
                    if !cameras.contains_key(&key) {
                        if rows == 0 || cols == 0 || rows * cols > 192 {
                            return Err("camera bound".into());
                        }
                        let (point, cr, owner) = physics.camera_state(name)?;
                        let vertical = (physics.camera_fovy(name)?.to_radians() / 2.).tan();
                        let cells = self
                            .cells
                            .entry(key.clone())
                            .or_insert_with(|| vec![Cell::default(); rows * cols]);
                        let mut readings = Vec::new();
                        for y in 0..rows {
                            for x in 0..cols {
                                // One native ray per receptor, explicit optical proxy.
                                let mut ray = rotate(
                                    cr,
                                    [
                                        (2. * (x as f64 + 0.5) / cols as f64 - 1.) * vertical * 4.
                                            / 3.,
                                        (1. - 2. * (y as f64 + 0.5) / rows as f64) * vertical,
                                        -1.,
                                    ],
                                );
                                let length = ray.iter().map(|v| v * v).sum::<f64>().sqrt();
                                ray = ray.map(|v| v / length);
                                let light = physics.ray_light(point, ray, owner)?;
                                readings.push(cells[y * cols + x].sample(light, 0.002)?);
                            }
                        }
                        cameras.insert(key.clone(), readings);
                    }
                    let [y, x] = s.cell.ok_or("camera cell absent")?;
                    let [_, cols] = s.retina_shape.unwrap();
                    let sample = cameras[&key]
                        .get(y * cols + x)
                        .ok_or("camera cell outside retina")?;
                    sample[match s.mode.as_deref() {
                        Some("on") => 0,
                        Some("off") => 1,
                        Some("lum") => 2,
                        Some("gradient") => 3,
                        _ => return Err("camera mode".into()),
                    }]
                }
                "legacy-acoustic" => {
                    use aarnn_embodiment::acoustics;
                    let band = s.band_hz.ok_or("acoustic band absent")?;
                    let rx = acoustic_receiver(physics, s)?;
                    let mut sources: Vec<acoustics::SoundSource> = model
                        .scene
                        .as_ref()
                        .map(|sc| sc.sound_sources.clone())
                        .unwrap_or_default();
                    sources.extend(self.dynamic_sound_sources.iter().cloned());
                    if s.mode.as_deref() == Some("itd") {
                        let pair_name = s.pair.as_deref().ok_or("itd pair absent")?;
                        let pair = model
                            .io
                            .sensors
                            .iter()
                            .find(|x| x.name == pair_name)
                            .ok_or("itd pair sensor not in the record")?;
                        let other = acoustic_receiver(physics, pair)?;
                        acoustics::itd_normalised(&sources, &rx, &other, band, now)
                    } else {
                        acoustics::normalised_level(
                            acoustics::band_level_db(&sources, &rx, band, now),
                            s.floor_db.unwrap_or(20.),
                            s.ceil_db.unwrap_or(100.),
                        )
                    }
                }
                "legacy-flow" => {
                    let v = local(r, [velocity[3], velocity[4], velocity[5]])[axis];
                    if s.mode.as_deref() == Some("left") {
                        v.clamp(0., 1.)
                    } else if s.mode.as_deref() == Some("right") {
                        (-v).clamp(0., 1.)
                    } else {
                        v.abs().min(1.)
                    }
                }
                "legacy-depth" => ((0.4 - position[2]) / 0.4).clamp(0., 1.),
                "legacy-pitch" => norm(
                    (-r[2][0]).clamp(-1., 1.).asin(),
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::FRAC_PI_2,
                ),
                _ => return Err(format!("unimplemented native sensor {} {}", s.name, s.kind)),
            };
            if !v.is_finite() {
                return Err(format!("nonfinite sensor {}", s.name));
            }
            if s.period_s.is_some() {
                self.held.insert(index, (now, v.clamp(0., 1.)));
            }
            out.push(v.clamp(0., 1.));
        }
        self.step = Some(step);
        self.values = out.clone();
        Ok(out)
    }
}
