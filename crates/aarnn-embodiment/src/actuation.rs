// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! One reference motor state machine shared by native environments and the
//! high-rate numerical ABI. It confers no production commit/fencing authority.
use crate::{
    aer::{Model, Reply, WeightedChannel},
    motor,
};
fn flight_commands(model: &Model) -> Vec<Vec<WeightedChannel>> {
    model
        .flight
        .as_ref()
        .map(|f| f.commands.iter().map(|c| c.readout.clone()).collect())
        .unwrap_or_default()
}
#[derive(Clone)]
pub struct Actuation {
    rates: Vec<f64>,
    process: [f64; 2],
    expected_output: usize,
    crow: bool,
    fish: bool,
    stiffness_joints: Vec<bool>,
    stiffness_output: Option<usize>,
    muscles: Option<crate::celegans::Muscles>,
    /// Actuator readouts, then the record's flight commands (if any), which
    /// are decoded exactly like actuator readouts into the tail of `rates`.
    readouts: Vec<Vec<crate::aer::WeightedChannel>>,
    actuators: usize,
    /// Graded brain (`output_code = pulse_density`): each readout's event
    /// density is decoded linearly (first-order filter) instead of spike
    /// accumulation, and single-sign position readouts span the full joint
    /// range (activation 0..1 -> control_range). Joints stay stiff while armed.
    graded: bool,
    /// Latest readout drive of a graded brain, consumed by `advance`.
    graded_drive: Vec<f64>,
    last_source: Option<u64>,
    last_received: Option<u64>,
    pub disarmed: bool,
    pub actuation_inhibited: bool,
    pub applied: u64,
    pub expired: u64,
    pub npc_steps: u64,
    pub npc_control: bool,
    /// Armed frames since the last disarm (servo wake-up ramp).
    armed_frames: u64,
    /// Per-rate graded decode retention per 2 ms frame (actuators, then
    /// flight commands): exp(-dt / tau), tau = the actuator's
    /// `decode_tau_s` or motor::RATE_TAU_S.
    graded_keep: Vec<f64>,
    /// Decoded rate a wake-enabled graded position readout starts from on
    /// arming (its `wake.from` angle as a 0..1 density), so the decoded
    /// target starts at the rest pose instead of the joint's lower limit.
    wake_rate: Vec<Option<f64>>,
}
impl Actuation {
    pub fn new(model: &Model) -> Self {
        Self {
            rates: vec![0.; model.actuators.len() + flight_commands(model).len()],
            process: [0.; 2],
            expected_output: model.io.output_names.len(),
            crow: model.profile_id == "crow_corvus_corone",
            fish: model.profile_id == "zebrafish",
            stiffness_joints: model
                .actuators
                .iter()
                .map(|a| a.joint.starts_with("tail_joint_"))
                .collect(),
            stiffness_output: model
                .io
                .output_names
                .iter()
                .position(|n| n == "zebrafish_o_31_trunk_stiffness"),
            muscles: (model.profile_id == "celegans").then(|| {
                crate::celegans::Muscles::new(model).expect("validated C. elegans muscle map")
            }),
            readouts: model
                .actuators
                .iter()
                .map(|a| a.readout.clone())
                .chain(flight_commands(model))
                .collect(),
            actuators: model.actuators.len(),
            graded: false,
            graded_drive: vec![0.; model.actuators.len() + flight_commands(model).len()],
            last_source: None,
            last_received: None,
            disarmed: true,
            actuation_inhibited: false,
            applied: 0,
            expired: 0,
            npc_steps: 0,
            npc_control: false,
            armed_frames: 0,
            graded_keep: model
                .actuators
                .iter()
                .map(|a| a.decode_tau_s.unwrap_or(motor::RATE_TAU_S))
                .chain(model.flight.iter().flat_map(|f| {
                    f.commands
                        .iter()
                        .map(|c| c.decode_tau_s.unwrap_or(motor::RATE_TAU_S))
                }))
                .map(|tau| (-0.002 / tau).exp())
                .collect(),
            wake_rate: model
                .actuators
                .iter()
                .map(|a| {
                    let [lo, hi] = a.control_range;
                    a.wake
                        .filter(|_| a.position_readout && hi > lo)
                        .map(|w| ((w.from - lo) / (hi - lo)).clamp(0., 1.))
                })
                .collect(),
        }
    }
    /// Filtered C. elegans body-wall activations `[row - 1][MDL, MDR, MVL, MVR]`.
    pub fn muscle_activations(&self) -> Option<&[[f64; 4]; crate::celegans::ROWS]> {
        self.muscles.as_ref().map(|m| m.activations())
    }
    pub fn rates(&self) -> &[f64] {
        &self.rates
    }
    /// Decoded flight commands (`flight::COMMANDS` order); empty without a
    /// flight motor. Zero whenever actuation is disarmed.
    pub fn commands(&self) -> &[f64] {
        &self.rates[self.actuators..]
    }
    pub fn process(&self) -> [f64; 2] {
        self.process
    }
    pub fn active(&self, index: usize) -> bool {
        !self.disarmed
            && !self.actuation_inhibited
            && (self.graded
                || self.rates[index].abs() > 1e-4
                || (self.fish && self.stiffness_joints[index] && self.process[0] > 1e-4))
    }
    pub fn disarm(&mut self) {
        self.rates.fill(0.);
        self.graded_drive.fill(0.);
        self.process.fill(0.);
        self.disarmed = true;
        self.armed_frames = 0;
        if let Some(muscles) = &mut self.muscles {
            muscles.reset();
        }
    }
    pub fn stop(&mut self) {
        self.actuation_inhibited = true;
        self.disarm();
    }
    pub fn arm(&mut self) {
        self.actuation_inhibited = false;
    }
    pub fn reset(&mut self) {
        self.stop();
        self.last_source = None;
        self.last_received = None;
    }
    pub fn accept(&mut self, step: u64, reply: &Reply) -> Result<(), String> {
        if reply.spikes.len() != self.expected_output || (self.crow && reply.spikes.len() != 64) {
            self.stop();
            return Err("output width mismatch".into());
        }
        if self.actuation_inhibited {
            self.disarm();
            return Ok(());
        }
        if reply.source_step > step
            || step - reply.source_step > 50
            || self.last_source.is_some_and(|s| reply.source_step <= s)
        {
            self.expired += 1;
            self.disarm();
            return Ok(());
        }
        if self.npc_control {
            self.disarm();
            self.npc_control = false;
        }
        if let Some(muscles) = &mut self.muscles {
            muscles.input(&reply.spikes, reply.output_code);
        } else if self.readouts.iter().any(|r| !r.is_empty()) {
            if self
                .readouts
                .iter()
                .flatten()
                .any(|c| c.channel >= reply.spikes.len() || !c.coefficient.is_finite())
            {
                self.stop();
                return Err("named motor channel outside pinned output map".into());
            }
            self.graded = reply.output_code == crate::aer::OutputCode::PulseDensity;
            for ((rate, readout), pending) in self
                .rates
                .iter_mut()
                .zip(&self.readouts)
                .zip(self.graded_drive.iter_mut())
            {
                let mut drive = 0.;
                for channel in readout {
                    // A pulse-density frame carries at most one event.
                    let spikes = reply.spikes[channel.channel];
                    let events = if self.graded { spikes.min(1) } else { spikes };
                    drive += channel.coefficient * f64::from(events);
                }
                if self.graded {
                    *pending = drive;
                } else {
                    *rate = (*rate + drive).clamp(-1., 1.);
                }
            }
        } else if reply.output_code == crate::aer::OutputCode::PulseDensity {
            // Implicit antagonist pairs (antagonist-rate-v1: positive, negative
            // per actuator) under a graded brain: the signed density
            // difference is decoded linearly, like explicit readouts.
            if reply.spikes.len() < self.actuators * 2 {
                self.stop();
                return Err("antagonist width mismatch".into());
            }
            self.graded = true;
            for (i, pending) in self
                .graded_drive
                .iter_mut()
                .take(self.actuators)
                .enumerate()
            {
                *pending = f64::from(reply.spikes[2 * i].min(1))
                    - f64::from(reply.spikes[2 * i + 1].min(1));
            }
        } else if let Err(error) =
            motor::accumulate(&mut self.rates[..self.actuators], &reply.spikes)
        {
            self.stop();
            return Err(error);
        }
        if self.crow {
            for (i, rate) in self.process.iter_mut().enumerate() {
                *rate = (*rate + f64::from(reply.spikes[62 + i])).min(1.);
            }
        }
        if let Some(output) = self.stiffness_output {
            self.process[0] = (self.process[0] + f64::from(reply.spikes[output])).min(1.);
        }
        self.last_source = Some(reply.source_step);
        self.last_received = Some(step);
        self.disarmed = false;
        self.applied += 1;
        Ok(())
    }
    /// One exclusive controller per physics boundary. A verified ready brain
    /// owns even a silent output stream. Auto fallback clears the former drive
    /// before NPC resumes after a connection failure; an explicit stop wins.
    pub fn select(
        &mut self,
        model: &Model,
        step: u64,
        npc_allowed: bool,
        neural_ready: bool,
        reply: Option<&Reply>,
        observations: Option<&[f64]>,
    ) -> Result<(), String> {
        if neural_ready {
            if self.npc_control {
                self.disarm();
                self.npc_control = false;
            }
            if let Some(reply) = reply {
                self.accept(step, reply)?;
            }
        } else {
            if reply.is_some() {
                self.expired += 1;
            }
            self.disarm();
        }
        self.advance(step);
        if npc_allowed && !neural_ready {
            self.npc(model, step, observations);
        }
        Ok(())
    }
    /// Explicit non-neural controller. Call only when this instance has no
    /// configured brain client. A stop/reset latch always wins.
    pub fn npc(&mut self, model: &Model, step: u64, observations: Option<&[f64]>) {
        if self.actuation_inhibited {
            self.disarm();
            return;
        }
        self.rates = crate::npc::rates(model, step, observations);
        // NPC intents drive joints only; flight commands stay at rest.
        self.rates
            .resize(self.readouts.len().max(self.actuators), 0.);
        self.process.fill(0.);
        self.last_source = None;
        self.last_received = None;
        self.npc_control = true;
        self.disarmed = false;
        self.npc_steps += 1;
    }
    /// Call once before each 2ms physics step, independently of network progress.
    pub fn advance(&mut self, step: u64) {
        if self
            .last_received
            .is_some_and(|s| step < s || step - s > 50)
            || self.actuation_inhibited
        {
            self.disarm();
        }
        if !self.disarmed {
            if let Some(muscles) = &mut self.muscles {
                if !self.npc_control {
                    self.rates.copy_from_slice(muscles.advance());
                } else {
                    motor::decay(&mut self.rates);
                }
            } else if self.graded {
                // Linear decode: rate -> mean event density x coefficient,
                // per-actuator time constant (default motor::RATE_TAU_S).
                if self.armed_frames == 0 {
                    for (rate, start) in self.rates.iter_mut().zip(&self.wake_rate) {
                        if let Some(start) = start {
                            *rate = *start;
                        }
                    }
                }
                for ((rate, pending), keep) in self
                    .rates
                    .iter_mut()
                    .zip(self.graded_drive.iter_mut())
                    .zip(&self.graded_keep)
                {
                    *rate = (*rate * keep + (1. - keep) * *pending).clamp(-1., 1.);
                    *pending = 0.;
                }
            } else {
                motor::decay(&mut self.rates);
            }
            motor::decay(&mut self.process);
            self.armed_frames += 1;
        }
    }
    /// Native control targets; gains, gear and force limits remain the model's
    /// mechanical transmission. The last two values are process drives.
    pub fn controls(&self, model: &Model) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.rates.len() + 2);
        for (i, a) in model.actuators.iter().enumerate() {
            let rate = self.rates[i];
            let single_sign =
                !a.readout.is_empty() && a.readout.iter().all(|c| c.coefficient >= 0.);
            out.push(if self.active(i) {
                (if a.position_readout && self.graded && single_sign {
                    // Graded activation 0..1 spans the whole joint range.
                    let target = a.control_range[0]
                        + rate.clamp(0., 1.) * (a.control_range[1] - a.control_range[0]);
                    match a.wake {
                        Some(w) if w.seconds > 0. => {
                            let t = self.armed_frames as f64 * model.io.dt_us as f64 * 1e-6;
                            w.from + (t / w.seconds).clamp(0., 1.) * (target - w.from)
                        }
                        _ => target,
                    }
                } else if a.position_readout {
                    (a.control_range[0] + a.control_range[1]) * 0.5
                        + rate * (a.control_range[1] - a.control_range[0]) * 0.5
                } else {
                    rate * if rate >= 0. {
                        a.control_range[1]
                    } else {
                        -a.control_range[0]
                    }
                })
                .clamp(a.control_range[0], a.control_range[1])
            } else {
                0.
            });
        }
        out.extend(self.process);
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/ugot_mecanum.json"
        ))
        .unwrap()
    }
    #[test]
    fn npc_reference_handoff_and_silent_ready_brain_are_exclusive() {
        let model = model();
        let mut state = Actuation::new(&model);
        state.select(&model, 0, true, false, None, None).unwrap();
        assert!(state.npc_control);
        assert!(state.controls(&model)[0] > 0.);
        assert_eq!(state.applied, 0);
        state.select(&model, 1, true, true, None, None).unwrap();
        assert!(!state.npc_control);
        assert!(state.disarmed);
        assert!(state.controls(&model).iter().all(|v| *v == 0.));
        let quiet = Reply {
            source_step: 1,
            timestamp_us: 2000,
            spikes: vec![0; 8],
            ..Default::default()
        };
        state
            .select(&model, 2, true, true, Some(&quiet), None)
            .unwrap();
        assert!(!state.npc_control);
        assert!(state.controls(&model).iter().all(|v| *v == 0.));
        state.select(&model, 3, true, false, None, None).unwrap();
        assert!(state.npc_control);
        assert!(state.controls(&model)[0] > 0.);
        state.stop();
        state.select(&model, 4, true, false, None, None).unwrap();
        assert!(state.disarmed);
        assert!(state.controls(&model).iter().all(|v| *v == 0.));
        state.arm();
        state.select(&model, 5, true, false, None, None).unwrap();
        assert!(state.controls(&model)[0] > 0.);
        state.reset();
        state.select(&model, 0, true, false, None, None).unwrap();
        assert!(state.actuation_inhibited && state.disarmed);
    }
    /// A graded brain's pulse-density code on a single positive position
    /// readout settles at control_range[0] + density x range: 0.5 is the
    /// joint midpoint, and the joint stays actuated (not limp) at density 0.
    #[test]
    fn graded_position_readout_spans_the_full_joint_range() {
        let model: Model = serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/hexapod.json"
        ))
        .unwrap();
        let joint = 1; // hex femur, control range [-1.4, -0.2]
        let [lo, hi] = model.actuators[joint].control_range;
        let channel = model.actuators[joint].readout[0].channel;
        for density in [0.0, 0.5, 1.0] {
            let mut state = Actuation::new(&model);
            state.arm();
            let mut phase = 0.0;
            for step in 1..=400u64 {
                phase += density;
                let mut spikes = vec![0u32; model.io.output_names.len()];
                if phase >= 1.0 - 1e-12 {
                    phase -= 1.0;
                    spikes[channel] = 1;
                }
                let reply = Reply {
                    source_step: step,
                    timestamp_us: step * 2000,
                    spikes,
                    output_code: crate::aer::OutputCode::PulseDensity,
                };
                state
                    .select(&model, step, false, true, Some(&reply), None)
                    .unwrap();
            }
            let control = state.controls(&model)[joint];
            let expected = lo + density * (hi - lo);
            assert!(
                (control - expected).abs() < 0.08 * (hi - lo),
                "density {density}: {control} vs {expected}"
            );
            assert!(state.active(joint), "graded joints stay stiff");
        }
    }

    /// NAO declares a servo wake-up and a 200 ms decode: on arming, a silent
    /// graded brain leaves each joint at its rest pose (not its lower limit);
    /// the target then reaches the brain's density and its ripple is small.
    #[test]
    fn wake_starts_at_rest_pose_and_slow_decode_smooths_ripple() {
        let model: Model = serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/nao.json"
        ))
        .unwrap();
        let joint = model
            .actuators
            .iter()
            .position(|a| a.name == "LKneePitch")
            .unwrap();
        let a = &model.actuators[joint];
        let wake = a.wake.expect("NAO declares a wake-up");
        assert!(a.decode_tau_s.is_some_and(|t| t > motor::RATE_TAU_S));
        let [lo, hi] = a.control_range;
        let channel = a.readout[0].channel;
        let density = 0.37;
        let mut state = Actuation::new(&model);
        state.arm();
        let mut phase = 0.0;
        let mut tail = Vec::new();
        for step in 1..=1500u64 {
            phase += if step > 20 { density } else { 0.0 }; // silent first frames
            let mut spikes = vec![0u32; model.io.output_names.len()];
            if phase >= 1.0 - 1e-12 {
                phase -= 1.0;
                spikes[channel] = 1;
            }
            let reply = Reply {
                source_step: step,
                timestamp_us: step * 2000,
                spikes,
                output_code: crate::aer::OutputCode::PulseDensity,
            };
            state
                .select(&model, step, false, true, Some(&reply), None)
                .unwrap();
            let control = state.controls(&model)[joint];
            if step <= 20 {
                assert!(
                    (control - wake.from).abs() < 0.05,
                    "step {step}: {control} left the rest pose"
                );
            }
            if step > 1000 {
                tail.push(control);
            }
        }
        let expected = lo + density * (hi - lo);
        let mean = tail.iter().sum::<f64>() / tail.len() as f64;
        let spread = tail.iter().fold(0f64, |m, c| m.max((c - mean).abs()));
        assert!(
            (mean - expected).abs() < 0.03 * (hi - lo),
            "{mean} vs {expected}"
        );
        assert!(spread < 0.02 * (hi - lo), "ripple {spread}");
    }

    /// Archetype robots (antagonist-rate-v1, no explicit readout) decode a
    /// graded brain's positive/negative pulse densities linearly: effort is
    /// (density+ - density-) x control range, not a spike accumulation.
    #[test]
    fn graded_antagonist_rate_decodes_signed_density() {
        let model = model();
        let joint = 0;
        let [lo, hi] = model.actuators[joint].control_range;
        for (pos, neg) in [(0.5, 0.0), (0.0, 0.75), (0.6, 0.6)] {
            let mut state = Actuation::new(&model);
            state.arm();
            let (mut a, mut b) = (0.0, 0.0);
            for step in 1..=400u64 {
                let mut spikes = vec![0u32; model.io.output_names.len()];
                a += pos;
                b += neg;
                if a >= 1.0 - 1e-12 {
                    a -= 1.0;
                    spikes[2 * joint] = 1;
                }
                if b >= 1.0 - 1e-12 {
                    b -= 1.0;
                    spikes[2 * joint + 1] = 1;
                }
                let reply = Reply {
                    source_step: step,
                    timestamp_us: step * 2000,
                    spikes,
                    output_code: crate::aer::OutputCode::PulseDensity,
                };
                state
                    .select(&model, step, false, true, Some(&reply), None)
                    .unwrap();
            }
            let control = state.controls(&model)[joint];
            let d: f64 = pos - neg;
            let expected = d * if d >= 0. { hi } else { -lo };
            assert!(
                (control - expected).abs() < 0.1,
                "{pos}/{neg}: {control} vs {expected}"
            );
            assert!(state.active(joint));
        }
    }

    #[test]
    fn stale_duplicate_stop_and_reset_never_replay_effort() {
        let model = model();
        let mut state = Actuation::new(&model);
        let reply = Reply {
            source_step: 1,
            timestamp_us: 2000,
            spikes: vec![1, 0, 0, 0, 0, 0, 0, 0],
            ..Default::default()
        };
        state.accept(1, &reply).unwrap();
        state.advance(1);
        assert!(state.controls(&model)[0] > 0.);
        state.accept(2, &reply).unwrap();
        assert!(state.disarmed);
        assert_eq!(state.expired, 1);
        let next = Reply {
            source_step: 3,
            timestamp_us: 6000,
            spikes: reply.spikes.clone(),
            ..Default::default()
        };
        state.stop();
        state.accept(3, &next).unwrap();
        assert!(state.controls(&model).iter().all(|v| *v == 0.));
        state.arm();
        state.accept(3, &next).unwrap();
        state.advance(54);
        assert!(state.disarmed);
        state.reset();
        assert!(state.actuation_inhibited);
        assert_eq!(state.applied, 2);
        state.arm();
        let wrong = Reply {
            source_step: 0,
            timestamp_us: 0,
            spikes: vec![1],
            ..Default::default()
        };
        assert!(state.accept(0, &wrong).is_err());
        assert!(state.actuation_inhibited);
    }
}
