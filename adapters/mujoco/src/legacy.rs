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
        for s in &model.io.sensors {
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
                    if let Some(direction) = s.direction {
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
                    let distance = physics
                        .ray(
                            point,
                            rotate(r, s.direction.ok_or("legacy ray direction absent")?),
                            body,
                        )?
                        .unwrap_or(reach)
                        .min(reach);
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
                    if !cameras.contains_key(name) {
                        let [rows, cols] = s.retina_shape.ok_or("camera shape absent")?;
                        if rows == 0 || cols == 0 || rows * cols > 192 {
                            return Err("camera bound".into());
                        }
                        let (point, cr, owner) = physics.camera_state(name)?;
                        let vertical = (physics.camera_fovy(name)?.to_radians() / 2.).tan();
                        let cells = self
                            .cells
                            .entry(name.clone())
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
                        cameras.insert(name.clone(), readings);
                    }
                    let [y, x] = s.cell.ok_or("camera cell absent")?;
                    let [_, cols] = s.retina_shape.unwrap();
                    let sample = cameras[name]
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
            out.push(v.clamp(0., 1.));
        }
        self.step = Some(step);
        self.values = out.clone();
        Ok(out)
    }
}
