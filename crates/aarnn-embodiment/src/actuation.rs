//! One reference motor state machine shared by native environments and the
//! high-rate numerical ABI. It confers no production commit/fencing authority.
use crate::{
    aer::{Model, Reply},
    motor,
};
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
    readouts: Vec<Vec<crate::aer::WeightedChannel>>,
    last_source: Option<u64>,
    last_received: Option<u64>,
    pub disarmed: bool,
    pub actuation_inhibited: bool,
    pub applied: u64,
    pub expired: u64,
    pub npc_steps: u64,
    pub npc_control: bool,
}
impl Actuation {
    pub fn new(model: &Model) -> Self {
        Self {
            rates: vec![0.; model.actuators.len()],
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
            readouts: model.actuators.iter().map(|a| a.readout.clone()).collect(),
            last_source: None,
            last_received: None,
            disarmed: true,
            actuation_inhibited: false,
            applied: 0,
            expired: 0,
            npc_steps: 0,
            npc_control: false,
        }
    }
    pub fn rates(&self) -> &[f64] {
        &self.rates
    }
    pub fn process(&self) -> [f64; 2] {
        self.process
    }
    pub fn active(&self, index: usize) -> bool {
        !self.disarmed
            && !self.actuation_inhibited
            && (self.rates[index].abs() > 1e-4
                || (self.fish && self.stiffness_joints[index] && self.process[0] > 1e-4))
    }
    pub fn disarm(&mut self) {
        self.rates.fill(0.);
        self.process.fill(0.);
        self.disarmed = true;
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
            muscles.input(&reply.spikes);
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
            for (rate, readout) in self.rates.iter_mut().zip(&self.readouts) {
                let mut drive = 0.;
                for channel in readout {
                    let spikes = &reply.spikes[channel.channel];
                    drive += channel.coefficient * f64::from(*spikes);
                }
                *rate = (*rate + drive).clamp(-1., 1.);
            }
        } else if let Err(error) = motor::accumulate(&mut self.rates, &reply.spikes) {
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
            } else {
                motor::decay(&mut self.rates);
            }
            motor::decay(&mut self.process);
        }
    }
    /// Native control targets; gains, gear and force limits remain the model's
    /// mechanical transmission. The last two values are process drives.
    pub fn controls(&self, model: &Model) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.rates.len() + 2);
        for (i, a) in model.actuators.iter().enumerate() {
            let rate = self.rates[i];
            out.push(if self.active(i) {
                (if a.position_readout {
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
    #[test]
    fn stale_duplicate_stop_and_reset_never_replay_effort() {
        let model = model();
        let mut state = Actuation::new(&model);
        let reply = Reply {
            source_step: 1,
            timestamp_us: 2000,
            spikes: vec![1, 0, 0, 0, 0, 0, 0, 0],
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
        };
        assert!(state.accept(0, &wrong).is_err());
        assert!(state.actuation_inhibited);
    }
}
