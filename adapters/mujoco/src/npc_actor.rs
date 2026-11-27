// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Programmed NPC actor sharing a body's MuJoCo world.
//!
//! A kinematic (mocap) conspecific-sized body that follows a pre-programmed
//! keyframe script in world coordinates: the NPC teacher for social-learning
//! experiments. It is sensed through the body's own senses only - range rays
//! and retinal rays hit its geoms like any other object - and it has no
//! contact with the body (contype/conaffinity 0), so it can never push or
//! trip the learner. The pinned robot model is not modified: a sibling MJCF
//! with the actor appended is loaded after the record's digest check, as the
//! physics service mirrors its obstacle voxels.
//!
//! Script (`NM_MUJOCO_NPC_ACTOR=path.json`):
//! `{"profile": "nao", "frame": "world", "keyframes": [[t_s, x_m, y_m, yaw_deg], ...]}`
//! or, relative to the learner, `"frame": "learner"` with keyframes
//! `[t_s, distance_m, bearing_deg, facing_deg]`: distance from the learner's
//! chest (5 cm ahead of its root body), bearing from its heading (+ = left),
//! and the actor's heading relative to facing the learner (0 = facing it).
//! A learner-frame NPC approaches the learner wherever it has moved or turned
//! to - a programmed interaction partner. Keyframes are linearly
//! interpolated (angles along the shorter arc) and held before the first and
//! after the last. `profile` (optional) restricts the actor to bodies of
//! that profile.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::native::Physics;

pub const ENV: &str = "NM_MUJOCO_NPC_ACTOR";
pub const BODY: &str = "npc_actor";

#[derive(Clone, Copy, Debug, PartialEq)]
struct Keyframe {
    t: f64,
    x: f64,
    y: f64,
    yaw: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame {
    World,
    Learner,
}

#[derive(Clone, Debug)]
pub struct NpcActor {
    keys: Vec<Keyframe>,
    pub frame: Frame,
    pub source: PathBuf,
    /// Last applied pose: ([x, y, z], yaw in radians).
    pub last: Option<([f64; 3], f64)>,
    /// Footstep detection: (time, position) of the last speed sample.
    heard: Option<(f64, [f64; 3])>,
    moving: bool,
}

impl NpcActor {
    /// The actor named by `NM_MUJOCO_NPC_ACTOR`, if any applies to `profile`.
    pub fn from_env(profile: &str) -> Result<Option<Self>, String> {
        let Ok(path) = std::env::var(ENV) else {
            return Ok(None);
        };
        if path.trim().is_empty() {
            return Ok(None);
        }
        let actor = Self::load(Path::new(&path))?;
        let text = fs::read_to_string(&path).map_err(|e| format!("{ENV} {path}: {e}"))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{ENV} {path}: {e}"))?;
        match value.get("profile").and_then(|p| p.as_str()) {
            Some(only) if only != profile => Ok(None),
            _ => Ok(Some(actor)),
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("npc actor {}: {e}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("npc actor {}: {e}", path.display()))?;
        let frame = match value
            .get("frame")
            .and_then(|f| f.as_str())
            .unwrap_or("world")
        {
            "world" => Frame::World,
            "learner" => Frame::Learner,
            other => {
                return Err(format!(
                    "npc actor: unknown frame {other:?} (world|learner)"
                ));
            }
        };
        let keys = parse_keyframes(&value, frame)?;
        Ok(Self {
            keys,
            frame,
            source: path.to_path_buf(),
            last: None,
            heard: None,
            moving: false,
        })
    }

    /// Pose at simulated time `t`: ([x, y, z], yaw radians).
    pub fn pose(&self, t: f64) -> ([f64; 3], f64) {
        let first = self.keys[0];
        let last = *self.keys.last().unwrap_or(&first);
        let k = if t <= first.t {
            (first, first, 0.0)
        } else if t >= last.t {
            (last, last, 0.0)
        } else {
            let i = self.keys.partition_point(|k| k.t <= t).max(1) - 1;
            let (a, b) = (self.keys[i], self.keys[i + 1]);
            let span = (b.t - a.t).max(1.0e-9);
            (a, b, (t - a.t) / span)
        };
        let (a, b, f) = k;
        let mut dyaw = (b.yaw - a.yaw) % std::f64::consts::TAU;
        if dyaw > std::f64::consts::PI {
            dyaw -= std::f64::consts::TAU;
        } else if dyaw < -std::f64::consts::PI {
            dyaw += std::f64::consts::TAU;
        }
        (
            [a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f, 0.0],
            a.yaw + dyaw * f,
        )
    }

    /// World pose for a learner-frame keyframe pose, given the learner's
    /// root position and heading.
    pub fn learner_to_world(
        local: ([f64; 3], f64),
        root: [f64; 3],
        heading: f64,
    ) -> ([f64; 3], f64) {
        let ([distance, bearing, _], facing) = local;
        let chest = [
            root[0] + 0.05 * heading.cos(),
            root[1] + 0.05 * heading.sin(),
        ];
        let direction = heading + bearing;
        (
            [
                chest[0] + distance * direction.cos(),
                chest[1] + distance * direction.sin(),
                0.0,
            ],
            direction + std::f64::consts::PI + facing,
        )
    }

    /// Move the actor to its pose at the current physics time.
    pub fn apply(&mut self, physics: &mut Physics) -> Result<(), String> {
        let local = self.pose(physics.time());
        let (pos, yaw) = match self.frame {
            Frame::World => local,
            Frame::Learner => {
                let state = physics.body_state(1)?;
                let heading = state.1[1][0].atan2(state.1[0][0]);
                Self::learner_to_world(local, state.0, heading)
            }
        };
        let quat = [(yaw * 0.5).cos(), 0.0, 0.0, (yaw * 0.5).sin()];
        physics.set_mocap_pose(BODY, pos, quat)?;
        self.last = Some((pos, yaw));
        Ok(())
    }

    /// The actor as a sound source: footsteps while it moves (speed above
    /// 5 cm/s between calls), broadband ~55 dB SPL at 1 m from 250 Hz to
    /// 4 kHz at foot height, as a person walking on a hard floor; silent at
    /// rest. A learner with ears hears an NPC approaching, not only its echo.
    pub fn sound_source(&mut self, t: f64) -> Option<aarnn_embodiment::acoustics::SoundSource> {
        let (pos, _) = self.last?;
        let moving = match self.heard {
            Some((t0, p0)) if t > t0 => {
                let v = ((pos[0] - p0[0]).powi(2) + (pos[1] - p0[1]).powi(2)).sqrt() / (t - t0);
                v > 0.05
            }
            _ => false,
        };
        if self.heard.is_none_or(|(t0, _)| t - t0 >= 0.1) {
            self.heard = Some((t, pos));
            self.moving = moving;
        }
        self.moving
            .then(|| aarnn_embodiment::acoustics::SoundSource {
                id: "npc_footsteps".into(),
                position: [pos[0], pos[1], 0.02],
                spectrum: vec![
                    [250.0, 52.0],
                    [1000.0, 55.0],
                    [4000.0, 50.0],
                    [8000.0, 40.0],
                ],
                period_s: Some(0.55),
                on_s: Some(0.08),
                start_s: 0.0,
            })
    }

    /// Trace field: the actor's pose at the sample.
    pub fn trace_value(&self) -> serde_json::Value {
        match self.last {
            Some((pos, yaw)) => serde_json::json!({"pos": pos, "yaw_rad": yaw}),
            None => serde_json::Value::Null,
        }
    }
}

fn parse_keyframes(value: &serde_json::Value, frame: Frame) -> Result<Vec<Keyframe>, String> {
    let rows = value
        .get("keyframes")
        .and_then(|k| k.as_array())
        .ok_or("npc actor: \"keyframes\" array required")?;
    let mut keys = Vec::with_capacity(rows.len());
    for row in rows {
        let r = row
            .as_array()
            .filter(|r| r.len() == 4)
            .ok_or("npc actor: each keyframe is [t_s, x_m, y_m, yaw_deg]")?;
        let n = |i: usize| r[i].as_f64().filter(|v| v.is_finite());
        let (Some(t), Some(x), Some(y), Some(yaw)) = (n(0), n(1), n(2), n(3)) else {
            return Err("npc actor: keyframe values must be finite numbers".into());
        };
        match frame {
            Frame::World => {
                if x.abs() > 50.0 || y.abs() > 50.0 {
                    return Err("npc actor: keyframe outside |x|,|y| <= 50 m".into());
                }
                keys.push(Keyframe {
                    t,
                    x,
                    y,
                    yaw: yaw.to_radians(),
                });
            }
            Frame::Learner => {
                if !(0.0..=50.0).contains(&x) {
                    return Err("npc actor: learner-frame distance must be 0..=50 m".into());
                }
                // distance in x, bearing (radians) in y, facing in yaw
                keys.push(Keyframe {
                    t,
                    x,
                    y: y.to_radians(),
                    yaw: yaw.to_radians(),
                });
            }
        }
    }
    if keys.is_empty() {
        return Err("npc actor: at least one keyframe required".into());
    }
    if keys.windows(2).any(|w| w[1].t < w[0].t) {
        return Err("npc actor: keyframe times must not decrease".into());
    }
    Ok(keys)
}

/// MJCF fragment of the actor: NAO-sized (0.58 m) legs, torso and head in a
/// distinct orange, no contacts, geom group 0 so range and retinal rays see it.
pub fn actor_mjcf() -> String {
    let geom = |name: &str, body: &str| {
        format!(
            r#"<geom name="{BODY}_{name}" {body} rgba="0.95 0.55 0.15 1" contype="0" conaffinity="0" group="0"/>"#
        )
    };
    format!(
        r#"<body name="{BODY}" mocap="true" pos="40 40 0">{}{}{}{}</body>"#,
        geom(
            "leg_left",
            r#"type="capsule" fromto="0 0.05 0.04 0 0.05 0.30" size="0.035""#
        ),
        geom(
            "leg_right",
            r#"type="capsule" fromto="0 -0.05 0.04 0 -0.05 0.30" size="0.035""#
        ),
        geom(
            "torso",
            r#"type="box" pos="0 0 0.40" size="0.05 0.09 0.11""#
        ),
        geom("head", r#"type="sphere" pos="0 0 0.56" size="0.065""#),
    )
}

/// Sibling copy of the model with the actor appended to the worldbody (the
/// pinned model is never written). The caller removes the returned file.
pub fn augment_model(path: &Path) -> Result<PathBuf, String> {
    let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if source.matches("</worldbody>").count() != 1 {
        return Err("npc actor: one native worldbody required".into());
    }
    let augmented = source.replace("</worldbody>", &format!("{}</worldbody>", actor_mjcf()));
    let temp = path.with_file_name(format!(
        ".npc-actor-{}-{}.xml",
        std::process::id(),
        unique()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(augmented.as_bytes()) {
        let _ = fs::remove_file(&temp);
        return Err(e.to_string());
    }
    Ok(temp)
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(keys: serde_json::Value) -> NpcActor {
        NpcActor {
            keys: parse_keyframes(&serde_json::json!({ "keyframes": keys }), Frame::World).unwrap(),
            frame: Frame::World,
            source: PathBuf::new(),
            last: None,
            heard: None,
            moving: false,
        }
    }

    #[test]
    fn keyframes_interpolate_and_hold() {
        let a = actor(serde_json::json!([
            [0.0, 0.0, 0.0, 0.0],
            [10.0, 1.0, -2.0, 90.0]
        ]));
        let (p, yaw) = a.pose(5.0);
        assert!((p[0] - 0.5).abs() < 1e-12 && (p[1] + 1.0).abs() < 1e-12);
        assert!((yaw - 45f64.to_radians()).abs() < 1e-12);
        assert_eq!(a.pose(-1.0).0, [0.0, 0.0, 0.0]);
        assert_eq!(a.pose(20.0).0, [1.0, -2.0, 0.0]);
    }

    #[test]
    fn yaw_takes_the_shorter_arc() {
        let a = actor(serde_json::json!([
            [0.0, 0.0, 0.0, 170.0],
            [1.0, 0.0, 0.0, -170.0]
        ]));
        let (_, yaw) = a.pose(0.5);
        assert!(
            (yaw - 180f64.to_radians()).abs() < 1e-9,
            "{}",
            yaw.to_degrees()
        );
    }

    #[test]
    fn scripts_are_validated() {
        let bad = |v: serde_json::Value| {
            parse_keyframes(&serde_json::json!({ "keyframes": v }), Frame::World).is_err()
        };
        assert!(bad(serde_json::json!([])));
        assert!(bad(serde_json::json!([[1.0, 0.0, 0.0]])));
        assert!(bad(serde_json::json!([
            [1.0, 0.0, 0.0, 0.0],
            [0.5, 0.0, 0.0, 0.0]
        ])));
        assert!(bad(serde_json::json!([[0.0, 99.0, 0.0, 0.0]])));
        assert!(actor_mjcf().contains(r#"mocap="true""#));
        let learner_bad = |v: serde_json::Value| {
            parse_keyframes(&serde_json::json!({ "keyframes": v }), Frame::Learner).is_err()
        };
        assert!(learner_bad(serde_json::json!([[0.0, -1.0, 20.0, 0.0]])));
        assert!(!learner_bad(serde_json::json!([[0.0, 1.0, 20.0, 0.0]])));
    }

    #[test]
    fn learner_frame_places_the_actor_relative_to_the_learner() {
        // Learner at (1, 2) heading +90 deg; actor 1 m away, 20 deg to its left.
        let heading = std::f64::consts::FRAC_PI_2;
        let (pos, yaw) = NpcActor::learner_to_world(
            ([1.0, 20f64.to_radians(), 0.0], 0.0),
            [1.0, 2.0, 0.3],
            heading,
        );
        let chest = [1.0, 2.05];
        let dir = heading + 20f64.to_radians();
        assert!((pos[0] - (chest[0] + dir.cos())).abs() < 1e-12);
        assert!((pos[1] - (chest[1] + dir.sin())).abs() < 1e-12);
        // facing 0 = facing the learner
        assert!(((yaw - dir).to_degrees() - 180.0).abs() < 1e-9);
    }
}
