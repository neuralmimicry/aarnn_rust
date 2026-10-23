//! Versioned log-luminance event cell from the repository's DeviceMapper.
//! Native adapters supply measured light; no neural or engine state is shared.
#[derive(Clone, Default)]
pub struct Cell {
    previous: Option<f64>,
    memory: f64,
    refractory: f64,
    polarity: i8,
}
impl Cell {
    /// Returns ON, OFF, log luminance, absolute temporal gradient.
    pub fn sample(&mut self, luminance: f64, dt: f64) -> Result<[f64; 4], String> {
        if !luminance.is_finite() || !(0. ..=1.).contains(&luminance) || !dt.is_finite() || dt <= 0.
        {
            return Err("invalid optical sample/time".into());
        }
        let current = (9. * luminance).ln_1p() / 10_f64.ln();
        let previous = self.previous.unwrap_or(current);
        let delta = current - previous;
        self.memory = (self.memory * (-dt / 0.045).exp() + delta).clamp(-1., 1.);
        let mut result = [0., 0., current, (delta.abs() / 0.08).min(1.)];
        if self.refractory > 0. {
            self.refractory = (self.refractory - dt).max(0.);
        } else if self.memory >= 0.08 + if self.polarity < 0 { 0.02 } else { 0. } {
            result[0] = 1.;
            self.polarity = 1;
            self.refractory = 0.012;
            self.memory *= 0.25;
        } else if self.memory <= -0.08 - if self.polarity > 0 { 0.02 } else { 0. } {
            result[1] = 1.;
            self.polarity = -1;
            self.refractory = 0.012;
            self.memory *= 0.25;
        } else if self.memory.abs() < 0.04 {
            self.polarity = 0;
        }
        self.previous = Some(previous + 0.35 * delta);
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optical_polarity_refractory_and_reset() {
        let mut c = Cell::default();
        assert_eq!(&c.sample(0., 0.002).unwrap()[..2], &[0., 0.]);
        assert_eq!(&c.sample(1., 0.002).unwrap()[..2], &[1., 0.]);
        assert_eq!(&c.sample(1., 0.002).unwrap()[..2], &[0., 0.]);
        let mut dark = Cell::default();
        dark.sample(1., 0.002).unwrap();
        assert_eq!(&dark.sample(0., 0.002).unwrap()[..2], &[0., 1.]);
        assert!(dark.sample(f64::NAN, 0.002).is_err());
    }
}
