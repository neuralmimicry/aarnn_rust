//! Proposed versioned sensory projection. Native engines supply measurements;
//! this module owns ordering, scaling and retinal history. No measured crow
//! connectome or calibrated transducer adequacy is asserted.
use crate::aer::Model;
#[derive(Default)]
pub struct Projection {
    retina: Vec<f64>,
}
impl Projection {
    /// This common raw-measurement layout does not stand in for a
    /// backend-specific legacy sensory projection (e.g. the MuJoCo worm).
    pub fn supported(model: &Model) -> bool {
        !model
            .controller_transfer
            .as_deref()
            .is_some_and(|s| s.starts_with("blocked"))
            && (model.profile_id == "crow_corvus_corone"
                || model.io.sensors.iter().all(|s| {
                    matches!(
                        s.kind.as_str(),
                        "angle" | "angular_velocity" | "body_gyro" | "range_ray"
                    )
                }))
    }
    /// actuator (q radians/metres, dq per second) pairs, local gyro XYZ,
    /// eight range rays in metres. Crow appends 192 retinal luminances,
    /// specific acceleration XYZ, local gravity unit XYZ, 12 contacts,
    /// 16 normalized acoustic bands, local airflow XYZ m/s, six internal values.
    pub fn width(model: &Model) -> usize {
        model.actuators.len() * 2
            + 11
            + if model.profile_id == "crow_corvus_corone" {
                235
            } else {
                0
            }
    }
    pub fn measure(&mut self, model: &Model, raw: &[f64]) -> Result<Vec<f64>, String> {
        if !Self::supported(model) {
            return Err("legacy sensory/muscle calibration incomplete".into());
        }
        if raw.len() != Self::width(model) || !raw.iter().all(|v| v.is_finite()) {
            return Err("measurement shape/nonfinite input".into());
        }
        let n = model.actuators.len() * 2;
        let norm = |v: f64, r: [f64; 2]| ((v - r[0]) / (r[1] - r[0])).clamp(0., 1.);
        if model.profile_id != "crow_corvus_corone" {
            return model
                .io
                .sensors
                .iter()
                .map(|s| {
                    let value = match s.kind.as_str() {
                        "angle" | "angular_velocity" => {
                            let index = model
                                .actuators
                                .iter()
                                .position(|a| Some(&a.joint) == s.joint.as_ref())
                                .ok_or("projection joint absent")?;
                            raw[index * 2 + usize::from(s.kind == "angular_velocity")]
                        }
                        "body_gyro" => raw[n + s.axis.filter(|i| *i < 3).ok_or("gyro axis")?],
                        "range_ray" => {
                            raw[n
                                + 3
                                + s.name
                                    .strip_prefix("proximity_")
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .filter(|i| *i < 8)
                                    .ok_or("range index")?]
                        }
                        _ => return Err("unsupported measurement kind".into()),
                    };
                    Ok(norm(value, s.range))
                })
                .collect();
        }
        let extra = &raw[n + 11..];
        if extra[..192]
            .iter()
            .chain(extra[198..226].iter())
            .chain(extra[229..235].iter())
            .any(|v| !(0. ..=1.).contains(v))
        {
            return Err("invalid normalized crow measurement".into());
        }
        if self.retina.is_empty() {
            self.retina = vec![0.; 192];
        }
        let mut out = Vec::with_capacity(492);
        for (value, old) in extra[..192].iter().zip(&self.retina) {
            let d = value - old;
            out.extend([d.max(0.), (-d).max(0.)]);
        }
        for i in 0..model.actuators.len() {
            out.extend([
                (0.5 + raw[2 * i] / 3.).clamp(0., 1.),
                (0.5 + raw[2 * i + 1] / 80.).clamp(0., 1.),
            ]);
        }
        out.extend(raw[n..n + 3].iter().map(|v| (0.5 + v / 80.).clamp(0., 1.)));
        out.extend(
            extra[192..195]
                .iter()
                .map(|v| (0.5 + v / 80.).clamp(0., 1.)),
        );
        out.extend(extra[195..198].iter().map(|v| (0.5 + v / 2.).clamp(0., 1.)));
        out.extend_from_slice(&extra[198..226]);
        out.extend(
            extra[226..229]
                .iter()
                .map(|v| (0.5 + v / 40.).clamp(0., 1.)),
        );
        out.extend_from_slice(&extra[229..235]);
        if out.len() != model.io.sensors.len() {
            return Err("projection contract width mismatch".into());
        }
        self.retina.copy_from_slice(&extra[..192]);
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_ranges_and_crow_retinal_history() {
        let m: Model = serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/ugot_mecanum.json"
        ))
        .unwrap();
        let mut p = Projection::default();
        let mut raw = vec![0.; Projection::width(&m)];
        raw[11] = 3.;
        let s = p.measure(&m, &raw).unwrap();
        assert_eq!(s.len(), 19);
        assert_eq!(s[8], 1.);
        let crow: Model = serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/crow_corvus_corone.json"
        ))
        .unwrap();
        let mut p = Projection::default();
        let mut raw = vec![0.; Projection::width(&crow)];
        raw[73] = 0.75;
        let first = p.measure(&crow, &raw).unwrap();
        assert_eq!(first.len(), 492);
        assert_eq!(first[0], 0.75);
        assert_eq!(p.measure(&crow, &raw).unwrap()[0], 0.);
        raw[73] = 0.25;
        let last = p.measure(&crow, &raw).unwrap();
        assert_eq!(last[1], 0.5);
        raw[73] = f64::NAN;
        assert!(p.measure(&crow, &raw).is_err());
    }
}
