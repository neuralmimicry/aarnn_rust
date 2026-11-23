// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Resonant insect flight motor (`resonant-flight-v1`, record `flight`).
//!
//! Insect asynchronous flight muscle does not beat the wings at a neural
//! rhythm: stretch activation drives the thorax at its mechanical resonance,
//! and both wings beat in phase because the thorax couples them. Descending
//! commands set power and, through the steering muscles of the wing hinge,
//! each wing's stroke amplitude and stroke centre. This reduced model, shared
//! by every engine, keeps exactly that split:
//!
//! * one thoracic phase `phi = 2 pi f t` at the wingbeat frequency `f` set
//!   by the stroke springs and the wing inertia (`k = I (2 pi f)^2`);
//! * per wing, on its stroke joint (`+q` moves the wing backward on both
//!   sides): the thoracic spring `k (c - q)` about the stroke centre `c` and
//!   the power drive `Kp (q* - q) + Kd q*'` towards `q* = c + A cos phi`,
//!   limited to `+-torque_limit g`. The stroke joint's own passive damping
//!   `Kd` (implicit in the engine) supplies the `-Kd q'` half of the
//!   derivative term, so the drive is stiff enough for a 2 ms step;
//! * the wing hinge (pitch joint, `+theta` puts the anatomical leading edge
//!   down) is servoed to `theta* = g alpha0 tanh(q*'(phi + lead) / w)`, the
//!   reference stroke velocity a little ahead of the thoracic phase:
//!   pronation and supination keep the leading edge up on both half-strokes
//!   (a passive hinge weathervanes to zero angle of attack in a quasi-steady
//!   model and gives no lift), and the wing turns over just before each
//!   stroke reversal (advanced rotation; timed from the measured stroke the
//!   servo turned it ~5 ms late and lost a third of the lift);
//! * neural commands, decoded like motor readouts (graded density or spike
//!   rate, 20 ms): `power` p gives the recruitment
//!   `g = smoothstep((p - p0) / (p1 - p0))`; `amplitude_<side>` a gives
//!   `A = g A_max a`; `centre_<side>` u in [-1, 1] gives
//!   `c = (1 - g) c_fold + g c_max u`. Unrecruited wings fold back
//!   (`c_fold`) and do not beat; there is no flight without a neural command.
//!
//! Aerodynamic forces are not part of this motor: the wing blades carry
//! scene `aerodynamics` panels (`quasi-steady-panel-v2`).
use crate::aer::WeightedChannel;
use serde::Deserialize;

/// Command order of `resonant-flight-v1`; `Flight::commands` must match it.
pub const COMMANDS: [&str; 5] = [
    "power",
    "amplitude_left",
    "amplitude_right",
    "centre_left",
    "centre_right",
];

#[derive(Debug, Clone, Deserialize)]
pub struct FlightWing {
    pub side: String,
    pub stroke_joint: String,
    pub pitch_joint: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct FlightCommand {
    pub name: String,
    pub readout: Vec<WeightedChannel>,
    /// Decode time constant of this command (default motor::RATE_TAU_S).
    /// Steering muscles answer within a few ms (the haltere-to-steering
    /// reflex is monosynaptic); the legs' 20 ms decode would add ~12 ms to
    /// a flight loop that has little phase margin.
    #[serde(default)]
    pub decode_tau_s: Option<f64>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Flight {
    pub model: String,
    pub wingbeat_hz: f64,
    pub stroke_stiffness_n_m_per_rad: f64,
    pub drive_stiffness_n_m_per_rad: f64,
    /// Equal to the stroke joint's passive damping (see module docs).
    pub drive_damping_n_m_s_per_rad: f64,
    pub drive_torque_limit_n_m: f64,
    pub amplitude_max_rad: f64,
    pub centre_max_rad: f64,
    pub fold_rad: f64,
    /// Power command at which recruitment starts and is complete.
    pub recruitment: [f64; 2],
    pub hinge_angle_rad: f64,
    pub hinge_rate_rad_s: f64,
    /// Phase advance of wing rotation over the stroke reference.
    pub hinge_lead_rad: f64,
    pub hinge_stiffness_n_m_per_rad: f64,
    /// Left then right.
    pub wings: Vec<FlightWing>,
    pub commands: Vec<FlightCommand>,
}

/// Joint state `[position, velocity]` of one wing's stroke and pitch joints.
pub type WingState = [[f64; 2]; 2];

impl Flight {
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            self.wingbeat_hz,
            self.stroke_stiffness_n_m_per_rad,
            self.drive_torque_limit_n_m,
            self.amplitude_max_rad,
            self.hinge_rate_rad_s,
        ];
        let non_negative = [
            self.drive_stiffness_n_m_per_rad,
            self.drive_damping_n_m_s_per_rad,
            self.centre_max_rad,
            self.hinge_angle_rad,
            self.hinge_lead_rad,
            self.hinge_stiffness_n_m_per_rad,
        ];
        if self.model != "resonant-flight-v1" {
            return Err(format!("unsupported flight model {}", self.model));
        }
        if !positive.iter().all(|v| v.is_finite() && *v > 0.)
            || !non_negative.iter().all(|v| v.is_finite() && *v >= 0.)
            || !self.fold_rad.is_finite()
            || !self.recruitment.iter().all(|v| v.is_finite())
            || self.recruitment[1] <= self.recruitment[0]
        {
            return Err("invalid flight motor parameters".into());
        }
        if self.wings.len() != 2 || self.wings[0].side != "left" || self.wings[1].side != "right" {
            return Err("flight motor needs a left and a right wing".into());
        }
        if self.commands.len() != COMMANDS.len()
            || self.commands.iter().zip(COMMANDS).any(|(c, n)| c.name != n)
            || self
                .commands
                .iter()
                .flat_map(|c| &c.readout)
                .any(|c| !c.coefficient.is_finite())
            || self
                .commands
                .iter()
                .filter_map(|c| c.decode_tau_s)
                .any(|t| !t.is_finite() || t < 0.002)
        {
            return Err(
                "flight commands must be power, amplitude_left/right, centre_left/right".into(),
            );
        }
        Ok(())
    }
    /// Recruitment of the flight motor by the power command, in [0, 1].
    pub fn recruitment(&self, power: f64) -> f64 {
        let [p0, p1] = self.recruitment;
        let x = ((power - p0) / (p1 - p0)).clamp(0., 1.);
        x * x * (3. - 2. * x)
    }
    /// Commanded stroke amplitude and centre `[A, c]` of each wing for
    /// decoded command rates in `COMMANDS` order.
    pub fn targets(&self, commands: &[f64]) -> Result<[[f64; 2]; 2], String> {
        if commands.len() != COMMANDS.len() || !commands.iter().all(|v| v.is_finite()) {
            return Err("flight command width mismatch".into());
        }
        let g = self.recruitment(commands[0].clamp(0., 1.));
        Ok(std::array::from_fn(|s| {
            [
                g * self.amplitude_max_rad * commands[1 + s].clamp(0., 1.),
                (1. - g) * self.fold_rad + g * self.centre_max_rad * commands[3 + s].clamp(-1., 1.),
            ]
        }))
    }
    /// Generalised torques `[stroke, pitch]` per wing (left, right) at
    /// simulation time `time_s`, excluding the stroke joint's passive damping.
    pub fn torques(
        &self,
        time_s: f64,
        commands: &[f64],
        wings: [WingState; 2],
    ) -> Result<[[f64; 2]; 2], String> {
        if !time_s.is_finite() || !wings.iter().flatten().flatten().all(|v| v.is_finite()) {
            return Err("nonfinite flight state".into());
        }
        let targets = self.targets(commands)?;
        let g = self.recruitment(commands[0].clamp(0., 1.));
        let omega = 2. * std::f64::consts::PI * self.wingbeat_hz;
        let phase = omega * time_s;
        let limit = self.drive_torque_limit_n_m * g;
        Ok(std::array::from_fn(|s| {
            let [amplitude, centre] = targets[s];
            let [[q, _], [theta, _]] = wings[s];
            let spring = self.stroke_stiffness_n_m_per_rad * (centre - q);
            let reference = centre + amplitude * phase.cos();
            let reference_rate = -amplitude * omega * phase.sin();
            let drive = (self.drive_stiffness_n_m_per_rad * (reference - q)
                + self.drive_damping_n_m_s_per_rad * reference_rate)
                .clamp(-limit, limit);
            let lead_rate = -amplitude * omega * (phase + self.hinge_lead_rad).sin();
            let hinge_target =
                g * self.hinge_angle_rad * (lead_rate / self.hinge_rate_rad_s).tanh();
            [
                spring + drive,
                self.hinge_stiffness_n_m_per_rad * (hinge_target - theta),
            ]
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn motor() -> Flight {
        let channel = |c| {
            vec![WeightedChannel {
                channel: c,
                coefficient: 1.,
            }]
        };
        Flight {
            model: "resonant-flight-v1".into(),
            wingbeat_hz: 40.,
            stroke_stiffness_n_m_per_rad: 0.0165,
            drive_stiffness_n_m_per_rad: 0.03,
            drive_damping_n_m_s_per_rad: 5e-4,
            drive_torque_limit_n_m: 0.2,
            amplitude_max_rad: 1.2,
            centre_max_rad: 0.4,
            fold_rad: 1.45,
            recruitment: [0.2, 0.5],
            hinge_angle_rad: 0.5,
            hinge_rate_rad_s: 50.,
            hinge_lead_rad: 0.5,
            hinge_stiffness_n_m_per_rad: 2e-3,
            wings: ["left", "right"]
                .map(|s| FlightWing {
                    side: s.into(),
                    stroke_joint: format!("{s}_wing_flap"),
                    pitch_joint: format!("{s}_wing_pitch"),
                })
                .to_vec(),
            commands: COMMANDS
                .iter()
                .enumerate()
                .map(|(i, n)| FlightCommand {
                    name: (*n).into(),
                    readout: channel(i),
                    decode_tau_s: Some(0.008),
                })
                .collect(),
        }
    }
    #[test]
    fn unpowered_wings_fold_and_never_beat() {
        let f = motor();
        f.validate().unwrap();
        // No power: both wings are held folded back by the thoracic spring
        // alone, whatever the amplitude/centre commands say.
        let off = [0.1, 1., 1., 1., -1.];
        assert_eq!(f.targets(&off).unwrap(), [[0., 1.45]; 2]);
        let rest = [[[1.45, 0.], [0., 0.]]; 2];
        for t in [0., 0.003, 0.0125, 0.7] {
            assert_eq!(f.torques(t, &off, rest).unwrap(), [[0., 0.]; 2]);
        }
        // The drive is bounded by recruitment: half power, half the limit.
        let q = [[[0.0, 0.], [0., 0.]]; 2];
        let half = f.recruitment(0.35);
        assert!((half - 0.5).abs() < 1e-12);
        let tau = f.torques(0., &[0.35, 1., 1., 0., 0.], q).unwrap();
        let spring = 0.0165 * (0.5 * 1.45);
        assert!((tau[0][0] - spring).abs() <= 0.2 * half + 1e-12);
    }
    #[test]
    fn one_thoracic_phase_drives_mirrored_wings_and_steers_per_side() {
        let f = motor();
        let cmd = [1., 0.5, 0.7, 0.25, -0.25];
        let [[a_l, c_l], [a_r, c_r]] = f.targets(&cmd).unwrap();
        assert!((a_l - 0.6).abs() < 1e-12 && (a_r - 0.84).abs() < 1e-12);
        assert!((c_l - 0.1).abs() < 1e-12 && (c_r + 0.1).abs() < 1e-12);
        // Identical wing states with identical commands get identical
        // torques (mirrored joints): the motor itself never rolls the body.
        let sym = [1., 0.6, 0.6, 0., 0.];
        let state = [[0.3, -12.], [0.2, 1.]];
        for t in [0.001, 0.0061, 0.0199] {
            let tau = f.torques(t, &sym, [state; 2]).unwrap();
            assert_eq!(tau[0], tau[1]);
        }
        // The drive pulls towards c + A cos(phi): at phi = 0 a wing at its
        // centre is pushed to +A (backward), half a beat later to -A.
        let centred = [[[0., 0.], [0., 0.]]; 2];
        assert!(f.torques(0., &sym, centred).unwrap()[0][0] > 0.);
        assert!(f.torques(0.0125, &sym, centred).unwrap()[0][0] < 0.);
        // Hinge: leading edge up (negative pitch) through the forward
        // half-stroke (phase 0..pi), already turned over at its start
        // (advanced rotation), and down-and-back through the backward one.
        let level = [[[0., 0.], [0., 0.]]; 2];
        for (t, sign) in [(0., -1.), (0.006, -1.), (0.0125, 1.), (0.019, 1.)] {
            assert!(sign * f.torques(t, &sym, level).unwrap()[0][1] > 0., "{t}");
        }
    }
    #[test]
    fn malformed_motor_or_commands_fail_closed() {
        let mut f = motor();
        assert!(f.targets(&[1., 0.5]).is_err());
        assert!(f.torques(f64::NAN, &[0.; 5], [[[0.; 2]; 2]; 2]).is_err());
        f.commands.swap(1, 2);
        assert!(f.validate().is_err());
        let mut f = motor();
        f.recruitment = [0.5, 0.5];
        assert!(f.validate().is_err());
        let mut f = motor();
        f.model = "resonant-flight-v0".into();
        assert!(f.validate().is_err());
        let mut f = motor();
        f.commands[0].decode_tau_s = Some(0.0005);
        assert!(f.validate().is_err());
    }
}
