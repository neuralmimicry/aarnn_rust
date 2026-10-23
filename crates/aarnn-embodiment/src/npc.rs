//! Programmed NPC motor intents. These are not neural outputs, diagnostic
//! efforts, learned skills, verified locomotion or a biological controller.
//! Versioned source-clock patterns act only through the model's real motors.
use crate::aer::Model;
pub const POLICY: &str = "programmed-mechanics-v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Auto,
    Npc,
    Silent,
    Aarnn,
}
impl Control {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "npc" => Ok(Self::Npc),
            "silent" => Ok(Self::Silent),
            "aarnn" => Ok(Self::Aarnn),
            _ => Err("control must be auto, npc, silent or aarnn".into()),
        }
    }
    pub fn npc(self, bound: bool) -> Result<bool, String> {
        match self {
            Self::Auto=>Ok(true),
            Self::Npc if !bound=>Ok(true),
            Self::Silent if !bound=>Ok(false),
            Self::Aarnn if bound=>Ok(false),
            _=>Err("NPC/silent require an unbound body; aarnn requires an explicit appropriate binding".into()),
        }
    }
}
pub fn rates(model: &Model, step: u64, measurements: Option<&[f64]>) -> Vec<f64> {
    let time = step as f64 * 0.002;
    let near = measurements.is_some_and(|s| {
        model
            .io
            .sensors
            .iter()
            .enumerate()
            .any(|(i, c)| c.name == "proximity_0" && s.get(i).is_some_and(|v| *v < 0.2))
    });
    model
        .actuators
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let name = a.joint.to_ascii_lowercase();
            let wheel = name.contains("wheel")
                || (model.profile_id == "ugot_mecanum" && !name.contains("roller"));
            if wheel {
                let turn = if near {
                    if name.contains("left") { -1. } else { 1. }
                } else {
                    1.
                };
                return 0.08 * turn;
            }
            // Reduced NPC joint exploration. No unmeasured task competence is
            // implied for walking, flight, grasping, cleaning or process actuators.
            let phase = i as f64 * std::f64::consts::PI / 3.;
            let frequency = if name.contains("head") || name.contains("neck") {
                0.2
            } else {
                0.5
            };
            0.06 * (2. * std::f64::consts::PI * frequency * time + phase).sin()
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bound_brains_are_exclusive_and_npc_is_source_clocked() {
        assert!(Control::Auto.npc(true).unwrap());
        assert!(Control::Npc.npc(true).is_err());
        assert!(!Control::Silent.npc(false).unwrap());
        let model: Model = serde_json::from_slice(include_bytes!(
            "../../../sim/content/embodiments/generated/ugot_mecanum.json"
        ))
        .unwrap();
        let far = rates(&model, 7, None);
        let mut sensory = vec![1.; model.io.sensors.len()];
        sensory[8] = 0.;
        let near = rates(&model, 7, Some(&sensory));
        assert_eq!(far, rates(&model, 7, None));
        assert_ne!(far, near);
        assert!(near.iter().all(|v| v.abs() <= 0.08));
    }
}
