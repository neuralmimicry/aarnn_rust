//! Versioned simulation motor/constraint numerics, independent of engine FFI.
use serde::Deserialize;
pub const RATE_TAU_S: f64 = 0.02;
/// Saturate active servo drive without clamping passive body velocity. An
/// external impact may exceed the motor envelope; braking remains permitted.
pub fn servo_speed_allows(target: f64, q: f64, velocity: f64, limit: f64) -> bool {
    [target, q, velocity, limit].into_iter().all(f64::is_finite)
        && limit > 0.
        && (velocity.abs() <= limit || velocity * (target - q) <= 0.)
}
pub fn accumulate(rates: &mut [f64], spikes: &[u32]) -> Result<(), String> {
    if spikes.len() < rates.len() * 2 {
        return Err("antagonist width mismatch".into());
    }
    for (i, rate) in rates.iter_mut().enumerate() {
        *rate = (*rate + f64::from(spikes[2 * i]) - f64::from(spikes[2 * i + 1])).clamp(-1., 1.);
    }
    Ok(())
}
pub fn decay(rates: &mut [f64]) {
    for rate in rates {
        *rate *= (-0.002 / RATE_TAU_S).exp();
    }
}
#[derive(Clone, Deserialize)]
pub struct Coupling {
    pub joint1: String,
    pub joint2: String,
    pub ratio: f64,
    pub offset: f64,
    pub stiffness: f64,
    pub damping: f64,
}
impl Coupling {
    pub fn efforts(&self, q: [f64; 2], v: [f64; 2]) -> Result<[f64; 2], String> {
        if !q
            .iter()
            .chain(v.iter())
            .chain([&self.ratio, &self.offset, &self.stiffness, &self.damping])
            .all(|v| v.is_finite())
            || self.ratio == 0.
            || self.stiffness < 0.
            || self.damping < 0.
        {
            return Err("invalid compliant coupling".into());
        }
        let error = q[0] - self.ratio * q[1] - self.offset;
        let velocity = v[0] - self.ratio * v[1];
        let lambda = self.stiffness * error + self.damping * velocity;
        Ok([-lambda, self.ratio * lambda])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn motor_speed_saturation_keeps_braking_and_passive_motion() {
        assert!(!servo_speed_allows(1., 0., 8., 7.5));
        assert!(servo_speed_allows(-1., 0., 8., 7.5));
        assert!(!servo_speed_allows(-1., 0., -8., 7.5));
        assert!(servo_speed_allows(1., 0., -8., 7.5));
        assert!(!servo_speed_allows(f64::NAN, 0., 0., 7.5));
    }
    #[test]
    fn compliant_coupling_has_no_hidden_power_source() {
        for ratio in [-1., 1., 2.] {
            let c = Coupling {
                joint1: "a".into(),
                joint2: "b".into(),
                ratio,
                offset: 0.1,
                stiffness: 20.,
                damping: 0.5,
            };
            let q = [0.2, -0.1];
            let v = [-0.3, 0.5];
            let t = c.efforts(q, v).unwrap();
            let potential_rate =
                c.stiffness * (q[0] - ratio * q[1] - c.offset) * (v[0] - ratio * v[1]);
            assert!(
                (t[0] * v[0]
                    + t[1] * v[1]
                    + potential_rate
                    + c.damping * (v[0] - ratio * v[1]).powi(2))
                .abs()
                    < 1e-12
            );
        }
    }
}
