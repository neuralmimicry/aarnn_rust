//! Named Webots muscle-response v2 port (55 ms muscle, 80 ms spine).
//! Engineering projection, not measured muscle physiology. No twitch fallback.
use crate::aer::Model;

pub const VERSION: &str = "celegans-muscle-v2-sim-v1";
#[derive(Clone)]
pub struct Muscles {
    groups: [[Option<usize>; 4]; 24],
    vulva: usize,
    segments: Vec<usize>,
    traces: [[f64; 4]; 24],
    vulva_trace: f64,
    targets: Vec<f64>,
    input: Vec<u32>,
}
impl Muscles {
    pub fn new(model: &Model) -> Result<Self, String> {
        if model.io.output_names.len() != 96 {
            return Err("C. elegans requires 96 named muscles".into());
        }
        let mut groups = [[None; 4]; 24];
        let mut vulva = None;
        for (index, name) in model.io.output_names.iter().enumerate() {
            let suffix = name
                .strip_prefix("celegans_o_")
                .and_then(|s| s.get(4..))
                .ok_or("muscle name")?;
            if suffix == "MVULVA" {
                if vulva.replace(index).is_some() {
                    return Err("duplicate MVULVA".into());
                }
                continue;
            }
            let group = ["MDL", "MDR", "MVL", "MVR"]
                .iter()
                .position(|g| suffix.starts_with(g))
                .ok_or("muscle quadrant")?;
            let segment = suffix[3..].parse::<usize>().map_err(|_| "muscle segment")?;
            if !(1..=24).contains(&segment) || groups[segment - 1][group].replace(index).is_some() {
                return Err("duplicate/invalid muscle".into());
            }
        }
        // The legacy projection has MVL24 absent and a separate MVULVA.
        for (seg, row) in groups.iter().enumerate() {
            for (group, index) in row.iter().enumerate() {
                if index.is_none() && !(seg == 23 && group == 2) {
                    return Err("missing named muscle".into());
                }
            }
        }
        let segments = model
            .actuators
            .iter()
            .map(|a| {
                let n = a
                    .joint
                    .strip_prefix("celegans_spine_")
                    .ok_or("named spine required")?
                    .parse::<usize>()
                    .map_err(|_| "spine segment")?;
                if !(1..=24).contains(&n) {
                    return Err("spine segment range");
                }
                Ok(n - 1)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            groups,
            vulva: vulva.ok_or("missing MVULVA")?,
            targets: vec![0.; segments.len()],
            segments,
            traces: [[0.; 4]; 24],
            vulva_trace: 0.,
            input: vec![0; 96],
        })
    }
    pub fn reset(&mut self) {
        self.traces = [[0.; 4]; 24];
        self.vulva_trace = 0.;
        self.targets.fill(0.);
        self.input.fill(0);
    }
    pub fn input(&mut self, spikes: &[u32]) {
        self.input.copy_from_slice(spikes);
    }
    pub fn advance(&mut self) -> &[f64] {
        let muscle_alpha = 1. - (-2_f64 / 55.).exp();
        let spine_alpha = 1. - (-2_f64 / 80.).exp();
        for (i, &seg) in self.segments.iter().enumerate() {
            let mut c = [0.; 4];
            for (g, contraction) in c.iter_mut().enumerate() {
                let drive = self.groups[seg][g].map_or(0., |n| f64::from(self.input[n].min(1)));
                self.traces[seg][g] += muscle_alpha * (drive - self.traces[seg][g]);
                *contraction = 0.5 + 0.5 * self.traces[seg][g];
            }
            // Preserve the original bridge's per-spine MVULVA filter update.
            self.vulva_trace +=
                muscle_alpha * (f64::from(self.input[self.vulva].min(1)) - self.vulva_trace);
            let dorsal = 0.5 * (c[0] + c[1]);
            let mut ventral = 0.5 * (c[2] + c[3]);
            if (9..=14).contains(&seg) {
                let blend = 0.22 * (1. - ((seg as f64 - 11.5).abs() / 3.)).max(0.);
                ventral = (1. - blend) * ventral + blend * (0.5 + 0.5 * self.vulva_trace);
            }
            let mut drive = ventral - dorsal;
            if seg <= 3 {
                drive += 0.16 * (0.5 * (c[1] + c[3]) - 0.5 * (c[0] + c[2]));
            }
            if !(5..=20).contains(&seg) {
                drive *= 0.72;
            }
            let normalized = ((0.5 + 0.44 * (3. * drive).tanh()).clamp(0.05, 0.95) - 0.5) * 2.;
            self.targets[i] += spine_alpha * (normalized - self.targets[i]);
        }
        self.input.fill(0);
        &self.targets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/celegans.json"
        ))
        .unwrap()
    }
    #[test]
    fn named_muscles_silence_directions_vulva_and_reset() {
        let m = model();
        let mut state = Muscles::new(&m).unwrap();
        for _ in 0..500 {
            assert!(state.advance().iter().all(|v| *v == 0.));
        }
        let mut spikes = vec![0; 96];
        spikes[m
            .io
            .output_names
            .iter()
            .position(|n| n.ends_with("MDL12"))
            .unwrap()] = 1;
        state.input(&spikes);
        let target = state.advance()[10];
        assert!(target < 0.);
        // Independent scalar evaluation of C++ response v2, first 2ms pulse.
        let contraction = 0.5 + 0.5 * (1. - (-2_f64 / 55.).exp());
        let expected =
            0.88 * (3. * (0.5 - 0.5 * (contraction + 0.5))).tanh() * (1. - (-2_f64 / 80.).exp());
        assert!((target - expected).abs() < 1e-14);
        state.reset();
        spikes.fill(0);
        spikes[m
            .io
            .output_names
            .iter()
            .position(|n| n.ends_with("MVULVA"))
            .unwrap()] = 1;
        state.input(&spikes);
        let targets = state.advance();
        assert!(targets[10] > 0.);
        assert_eq!(targets[0], 0.);
        assert_eq!(targets[22], 0.);
        state.reset();
        assert!(state.advance().iter().all(|v| *v == 0.));
        let mut permuted = m.clone();
        permuted.io.output_names.reverse();
        let mut a = Muscles::new(&permuted).unwrap();
        spikes.reverse();
        a.input(&spikes);
        state.input(&spikes.iter().copied().rev().collect::<Vec<_>>());
        assert_eq!(a.advance(), state.advance());
    }
}
