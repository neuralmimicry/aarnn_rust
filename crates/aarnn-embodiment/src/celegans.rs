// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Named C. elegans body-wall muscle response for the planar MuJoCo worm.
//!
//! v3 (`celegans-muscle-v3-sim-v1`) corrects the v2 port:
//! * muscle row 1 is the most anterior row (WormAtlas); the head is
//!   `segment_23`, so row `r` belongs to segment `24 - r`. v2 indexed rows
//!   from the tail, so head motor output bent the tail and row 01 drove
//!   nothing;
//! * every hinge is driven by the two muscle rows that span it, so all 24
//!   rows (95 body-wall muscles) contribute;
//! * the MVULVA filter advances once per 2 ms step (v2 advanced it once per
//!   hinge, i.e. 23 times per step) and vulval muscles do not bend the body;
//! * the side-lying worm bends dorsoventrally in the plane, so left/right
//!   quadrant imbalance has no in-plane effect (v2 added an L-R term).
//! * activation sums saturating twitches (`TWITCH` per spike frame, decaying
//!   with the 55 ms muscle time constant). v2 low-pass filtered the binary
//!   frame train, so activation equalled the fraction of 2 ms frames with a
//!   spike: a muscle firing at a physiological 5-20 Hz reached 1-4 % and
//!   full contraction needed 500 Hz.
//!
//! * graded brains (`output_code = pulse_density`, negotiated in the
//!   handshake) send each muscle's graded activation as a pulse-density code
//!   (fraction of 2 ms frames with an event). Those trains are decoded
//!   linearly by the same 55 ms first-order excitation-contraction filter, so
//!   a steady activation `a` gives a steady muscle state `a`. Twitch
//!   summation would compress such a code (a steady 0.2 reads about 0.67).
//!
//! Sign: ventral contraction gives a positive hinge angle (+z), so the
//! ventral side is +y in the body frame. This is a functional surrogate, not
//! measured muscle physiology. No twitch or gait fallback.
use crate::aer::{Model, OutputCode};

pub const VERSION: &str = "celegans-muscle-v3-sim-v1";
/// Muscle activation time constant (ms) and hinge-target time constant (ms).
pub const MUSCLE_TAU_MS: f64 = 55.;
pub const SPINE_TAU_MS: f64 = 80.;
pub const SOURCE_DT_MS: f64 = 2.;
/// Fraction of the remaining contraction recruited by one muscle spike.
pub const TWITCH: f64 = 0.3;
/// Body-wall rows per quadrant (MVL24 is absent in the animal).
pub const ROWS: usize = 24;
#[derive(Clone)]
pub struct Muscles {
    /// `groups[row - 1][q]`, quadrants MDL, MDR, MVL, MVR.
    groups: [[Option<usize>; 4]; ROWS],
    vulva: usize,
    /// Per actuator: the (anterior, posterior) muscle rows spanning its hinge.
    rows: Vec<(usize, usize)>,
    traces: [[f64; 4]; ROWS],
    vulva_trace: f64,
    targets: Vec<f64>,
    input: Vec<u32>,
    code: OutputCode,
}
impl Muscles {
    pub fn new(model: &Model) -> Result<Self, String> {
        if model.io.output_names.len() != 96 {
            return Err("C. elegans requires 96 named muscles".into());
        }
        let mut groups = [[None; 4]; ROWS];
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
            let row = suffix[3..].parse::<usize>().map_err(|_| "muscle row")?;
            if !(1..=ROWS).contains(&row) || groups[row - 1][group].replace(index).is_some() {
                return Err("duplicate/invalid muscle".into());
            }
        }
        for (row, quadrants) in groups.iter().enumerate() {
            for (group, index) in quadrants.iter().enumerate() {
                if index.is_none() && !(row == ROWS - 1 && group == 2) {
                    return Err("missing named muscle".into());
                }
            }
        }
        let rows = model
            .actuators
            .iter()
            .map(|a| {
                let n = a
                    .joint
                    .strip_prefix("celegans_spine_")
                    .ok_or("named spine required")?
                    .parse::<usize>()
                    .map_err(|_| "spine segment")?;
                // `celegans_spine_NN` is the hinge on segment NN-1 (tail = 0,
                // head = 23) joining it to segment NN-2. Segment s carries
                // muscle row 24 - s.
                if !(2..=ROWS).contains(&n) {
                    return Err("spine segment range");
                }
                let segment = n - 1;
                Ok((ROWS - segment, ROWS + 1 - segment))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            groups,
            vulva: vulva.ok_or("missing MVULVA")?,
            targets: vec![0.; rows.len()],
            rows,
            traces: [[0.; 4]; ROWS],
            vulva_trace: 0.,
            input: vec![0; 96],
            code: OutputCode::Spikes,
        })
    }
    pub fn reset(&mut self) {
        self.traces = [[0.; 4]; ROWS];
        self.vulva_trace = 0.;
        self.targets.fill(0.);
        self.input.fill(0);
    }
    pub fn input(&mut self, spikes: &[u32], code: OutputCode) {
        self.input.copy_from_slice(spikes);
        self.code = code;
    }
    /// Decoder in use: twitch summation (spikes) or linear (pulse density).
    pub fn output_code(&self) -> OutputCode {
        self.code
    }
    /// Filtered activation in [0, 1] of every quadrant, `[row - 1][q]`.
    pub fn activations(&self) -> &[[f64; 4]; ROWS] {
        &self.traces
    }
    pub fn vulva_activation(&self) -> f64 {
        self.vulva_trace
    }
    pub fn advance(&mut self) -> &[f64] {
        let decay = (-SOURCE_DT_MS / MUSCLE_TAU_MS).exp();
        let spine_alpha = 1. - (-SOURCE_DT_MS / SPINE_TAU_MS).exp();
        let code = self.code;
        let twitch = |a: f64, spikes: u32| {
            let a = a * decay;
            let event = f64::from(spikes.min(1));
            match code {
                OutputCode::Spikes => a + TWITCH * event * (1. - a),
                OutputCode::PulseDensity => a + (1. - decay) * event,
            }
        };
        // Each muscle state advances exactly once per source step.
        for (row, quadrants) in self.groups.iter().enumerate() {
            for (q, index) in quadrants.iter().enumerate() {
                let spikes = index.map_or(0, |n| self.input[n]);
                self.traces[row][q] = twitch(self.traces[row][q], spikes);
            }
        }
        self.vulva_trace = twitch(self.vulva_trace, self.input[self.vulva]);
        for (i, &(anterior, posterior)) in self.rows.iter().enumerate() {
            let mut dorsal = 0.;
            let mut ventral = 0.;
            let mut ventral_n = 0.;
            for row in [anterior, posterior] {
                let t = &self.traces[row - 1];
                dorsal += 0.5 * (t[0] + t[1]);
                // MVL24 is absent: average only the ventral cells that exist.
                for q in [2, 3] {
                    if self.groups[row - 1][q].is_some() {
                        ventral += t[q];
                        ventral_n += 1.;
                    }
                }
            }
            dorsal *= 0.5;
            let ventral = ventral / ventral_n;
            // v2 contraction scale (0.5 + 0.5 * trace) kept so the hinge gain
            // is unchanged: drive in [-0.5, 0.5].
            let drive = 0.5 * (ventral - dorsal);
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
    fn index(m: &Model, suffix: &str) -> usize {
        m.io.output_names
            .iter()
            .position(|n| n.ends_with(suffix))
            .unwrap()
    }
    fn actuator(m: &Model, spine: usize) -> usize {
        m.actuators
            .iter()
            .position(|a| a.joint == format!("celegans_spine_{spine:02}"))
            .unwrap()
    }
    #[test]
    fn rows_map_head_to_tail_with_independent_scalar_reference() {
        let m = model();
        let mut state = Muscles::new(&m).unwrap();
        for _ in 0..500 {
            assert!(state.advance().iter().all(|v| *v == 0.));
        }
        // Row 01 (most anterior) bends only the head hinge (spine_24, segment 23).
        let mut spikes = vec![0; 96];
        spikes[index(&m, "MDL01")] = 1;
        state.input(&spikes, OutputCode::Spikes);
        let targets = state.advance().to_vec();
        let head = actuator(&m, 24);
        assert!(
            targets[head] < 0.,
            "dorsal contraction bends dorsally (negative)"
        );
        for (i, t) in targets.iter().enumerate() {
            if i != head {
                assert_eq!(*t, 0., "row 01 must not move hinge {i}");
            }
        }
        // Independent scalar evaluation for one dorsal-left pulse at row 01:
        // dorsal mean over the two spanning rows = 0.5 * (0.5 * a) where the
        // first twitch from rest is a = TWITCH; drive = 0.5 * (0 - dorsal).
        let a = TWITCH;
        let drive = 0.5 * (0. - 0.5 * (0.5 * a));
        let expected = 0.88 * (3. * drive).tanh() * (1. - (-2_f64 / 80.).exp());
        assert!((targets[head] - expected).abs() < 1e-14);
        // Row 24 (tail) reaches the tail hinge (spine_02, segment 1).
        state.reset();
        spikes.fill(0);
        spikes[index(&m, "MVR24")] = 1;
        state.input(&spikes, OutputCode::Spikes);
        let targets = state.advance().to_vec();
        assert!(
            targets[actuator(&m, 2)] > 0.,
            "ventral contraction is positive"
        );
        assert_eq!(targets[head], 0.);
        // Vulval muscles never bend the body and filter once per step.
        state.reset();
        spikes.fill(0);
        spikes[index(&m, "MVULVA")] = 1;
        state.input(&spikes, OutputCode::Spikes);
        assert!(state.advance().iter().all(|v| *v == 0.));
        assert!((state.vulva_activation() - a).abs() < 1e-15);
        // Twitch summation saturates below 1 and decays with tau = 55 ms.
        state.reset();
        spikes.fill(0);
        spikes[index(&m, "MDR05")] = 1;
        for _ in 0..50 {
            state.input(&spikes, OutputCode::Spikes);
            state.advance();
        }
        let a_sat = state.activations()[4][1];
        assert!(a_sat > 0.9 && a_sat < 1.0, "{a_sat}");
        spikes.fill(0);
        state.input(&spikes, OutputCode::Spikes);
        state.advance();
        let decayed = state.activations()[4][1];
        assert!((decayed - a_sat * (-2_f64 / 55.).exp()).abs() < 1e-12);
        // Left/right imbalance within a quadrant pair has no in-plane effect.
        state.reset();
        spikes.fill(0);
        spikes[index(&m, "MDL10")] = 1;
        spikes[index(&m, "MVL10")] = 1;
        let mut mirrored = vec![0; 96];
        mirrored[index(&m, "MDR10")] = 1;
        mirrored[index(&m, "MVR10")] = 1;
        let mut other = Muscles::new(&m).unwrap();
        state.input(&spikes, OutputCode::Spikes);
        other.input(&mirrored, OutputCode::Spikes);
        assert_eq!(state.advance(), other.advance());
        state.reset();
        assert!(state.advance().iter().all(|v| *v == 0.));
        // Channel order is irrelevant: names, not indices, define the map.
        let mut permuted = m.clone();
        permuted.io.output_names.reverse();
        let mut a2 = Muscles::new(&permuted).unwrap();
        let mut rev = vec![0; 96];
        rev[index(&m, "MDL07")] = 1;
        state.input(&rev, OutputCode::Spikes);
        a2.input(
            &rev.iter().copied().rev().collect::<Vec<_>>(),
            OutputCode::Spikes,
        );
        assert_eq!(a2.advance(), state.advance());
    }
    /// Pulse-density decoding is linear: a steady event density `p` per
    /// frame settles at activation `p`, unlike twitch summation.
    #[test]
    fn pulse_density_code_decodes_linearly() {
        let m = model();
        let mdr = index(&m, "MDR05");
        for (p, twitch_floor) in [(0.2, 0.6), (0.5, 0.8)] {
            let mut graded = Muscles::new(&m).unwrap();
            let mut spiking = Muscles::new(&m).unwrap();
            let mut phase = 0.;
            let (mut sum_g, mut sum_s, mut n) = (0., 0., 0.);
            for frame in 0..5000 {
                phase += p;
                let mut spikes = vec![0; 96];
                if phase >= 1. {
                    phase -= 1.;
                    spikes[mdr] = 1;
                }
                graded.input(&spikes, OutputCode::PulseDensity);
                spiking.input(&spikes, OutputCode::Spikes);
                graded.advance();
                spiking.advance();
                if frame >= 2500 {
                    sum_g += graded.activations()[4][1];
                    sum_s += spiking.activations()[4][1];
                    n += 1.;
                }
            }
            assert_eq!(graded.output_code(), OutputCode::PulseDensity);
            assert!((sum_g / n - p).abs() < 0.01, "p {p}: {}", sum_g / n);
            assert!(sum_s / n > twitch_floor, "twitch compresses: {}", sum_s / n);
        }
        let hs = |v: serde_json::Value| OutputCode::from_handshake(&v);
        assert_eq!(
            hs(serde_json::json!({"expected_o": 96})).unwrap(),
            OutputCode::Spikes
        );
        assert_eq!(
            hs(serde_json::json!({"output_code": "pulse_density"})).unwrap(),
            OutputCode::PulseDensity
        );
        assert!(hs(serde_json::json!({"output_code": "analog"})).is_err());
    }
}
