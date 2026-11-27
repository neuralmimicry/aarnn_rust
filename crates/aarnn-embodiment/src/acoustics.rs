// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Engine-independent acoustic proxy: what each ear or microphone of a body
//! hears from the scene's sound sources. Every engine (MuJoCo, Webots, Isaac,
//! Unity, Unreal) passes world positions and gets the same channel values, as
//! with the analytic stimulus fields in `aer`.
//!
//! Model (free field, quasi-static; no waveform is synthesised):
//! * a source has a spectrum, sound pressure levels (dB SPL at 1 m) at a few
//!   frequencies, read in log-frequency at a channel's band centre, and an
//!   optional on/off schedule (calls, song pulses, footsteps);
//! * spherical spreading, -20 log10(r / 1 m), with r floored at 5 cm;
//! * a receiver's directivity `(1 - k) + k cos(theta)` (theta from its
//!   acoustic axis), with `k` growing with frequency up to `directivity` at
//!   and above 1 kHz: the head shadow that makes interaural level
//!   differences larger for high sounds, as in mammal and bird hearing;
//! * incoherent summation over sources;
//! * levels map to [0, 1] between `floor_db` and `ceil_db`;
//! * the interaural time difference of the loudest source between two
//!   receivers, normalised to their spacing (0.5 = straight ahead).
//!
//! Not modelled: occlusion by walls, reverberation, ego-noise of the body's
//! own motors, near-field particle velocity (a fly's Johnston's organ is
//! approximated by a directional pressure receiver).
use serde::Deserialize;

/// Speed of sound in air at 20 C (m/s).
pub const SPEED_OF_SOUND: f64 = 343.0;
const MIN_RANGE_M: f64 = 0.05;
const SILENT_DB: f64 = -200.0;

/// One sound source in world coordinates.
#[derive(Debug, Clone, Deserialize)]
pub struct SoundSource {
    #[serde(default)]
    pub id: String,
    pub position: [f64; 3],
    /// `[frequency_hz, dB SPL at 1 m]` pairs, any order.
    pub spectrum: Vec<[f64; 2]>,
    /// On/off schedule (s): sounding for `on_s` every `period_s` from `start_s`.
    #[serde(default)]
    pub period_s: Option<f64>,
    #[serde(default)]
    pub on_s: Option<f64>,
    #[serde(default)]
    pub start_s: f64,
}

impl SoundSource {
    /// Whether the source sounds at time `t` (s).
    pub fn sounding(&self, t: f64) -> bool {
        if t < self.start_s {
            return false;
        }
        match (self.period_s, self.on_s) {
            (Some(period), Some(on)) if period > 0.0 && period.is_finite() => {
                (t - self.start_s).rem_euclid(period) < on
            }
            _ => true,
        }
    }

    /// Level (dB SPL at 1 m) at `band_hz`, interpolated in log-frequency
    /// between the nearest spectrum points (held flat beyond them, -200 dB
    /// for an empty spectrum).
    pub fn level_at_1m(&self, band_hz: f64) -> f64 {
        let mut points: Vec<[f64; 2]> = self
            .spectrum
            .iter()
            .copied()
            .filter(|p| p[0] > 0.0 && p[0].is_finite() && p[1].is_finite())
            .collect();
        if points.is_empty() || band_hz <= 0.0 {
            return SILENT_DB;
        }
        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let x = band_hz.ln();
        if x <= points[0][0].ln() {
            return points[0][1];
        }
        for pair in points.windows(2) {
            let (a, b) = (pair[0][0].ln(), pair[1][0].ln());
            if x <= b {
                let t = if b > a { (x - a) / (b - a) } else { 0.0 };
                return pair[0][1] + t * (pair[1][1] - pair[0][1]);
            }
        }
        points[points.len() - 1][1]
    }
}

/// A receiver (ear, microphone, antennal organ) in world coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Receiver {
    pub position: [f64; 3],
    /// Acoustic axis (unit or not; normalised here).
    pub axis: [f64; 3],
    /// Directivity at and above 1 kHz: 0 omnidirectional, 1 cardioid.
    pub directivity: f64,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Level (dB SPL) of one source at a receiver in one band.
pub fn source_level_db(source: &SoundSource, rx: &Receiver, band_hz: f64) -> f64 {
    let to_source = sub(source.position, rx.position);
    let r = norm(to_source).max(MIN_RANGE_M);
    let axis_len = norm(rx.axis);
    let cos = if axis_len > 0.0 {
        (to_source[0] * rx.axis[0] + to_source[1] * rx.axis[1] + to_source[2] * rx.axis[2])
            / (r * axis_len)
    } else {
        1.0
    };
    let k = rx.directivity.clamp(0.0, 1.0) * (band_hz / 1000.0).clamp(0.0, 1.0);
    let gain = ((1.0 - k) + k * cos.clamp(-1.0, 1.0)).max(0.05);
    source.level_at_1m(band_hz) - 20.0 * r.log10() + 20.0 * gain.log10()
}

/// Incoherent sum (dB SPL) of every sounding source at a receiver.
pub fn band_level_db(sources: &[SoundSource], rx: &Receiver, band_hz: f64, t: f64) -> f64 {
    let power: f64 = sources
        .iter()
        .filter(|s| s.sounding(t))
        .map(|s| 10f64.powf(source_level_db(s, rx, band_hz) / 10.0))
        .sum();
    if power > 0.0 {
        10.0 * power.log10()
    } else {
        SILENT_DB
    }
}

/// Level mapped to [0, 1] between `floor_db` (0) and `ceil_db` (1).
pub fn normalised_level(level_db: f64, floor_db: f64, ceil_db: f64) -> f64 {
    if ceil_db <= floor_db {
        return 0.0;
    }
    ((level_db - floor_db) / (ceil_db - floor_db)).clamp(0.0, 1.0)
}

/// Interaural time difference of the loudest sounding source (in `band_hz`
/// at receiver `a`), normalised to the receivers' spacing: 0.5 straight
/// ahead, 1 when the source is on `b`'s side of the axis through a and b
/// (the sound reaches `b` first), 0 on `a`'s side. 0.5 when silent.
pub fn itd_normalised(
    sources: &[SoundSource],
    a: &Receiver,
    b: &Receiver,
    band_hz: f64,
    t: f64,
) -> f64 {
    let loudest = sources
        .iter()
        .filter(|s| s.sounding(t))
        .map(|s| (s, source_level_db(s, a, band_hz)))
        .filter(|(_, l)| *l > SILENT_DB / 2.0)
        .max_by(|x, y| x.1.total_cmp(&y.1));
    let Some((source, _)) = loudest else {
        return 0.5;
    };
    let spacing = norm(sub(a.position, b.position));
    if spacing <= 0.0 {
        return 0.5;
    }
    let dt = (norm(sub(source.position, a.position)) - norm(sub(source.position, b.position)))
        / SPEED_OF_SOUND;
    let max_dt = spacing / SPEED_OF_SOUND;
    (0.5 + 0.5 * dt / max_dt).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(pos: [f64; 3], db: f64) -> SoundSource {
        SoundSource {
            id: "t".into(),
            position: pos,
            spectrum: vec![[1000.0, db]],
            period_s: None,
            on_s: None,
            start_s: 0.0,
        }
    }
    fn ear(pos: [f64; 3], axis: [f64; 3]) -> Receiver {
        Receiver {
            position: pos,
            axis,
            directivity: 0.6,
        }
    }

    #[test]
    fn spreading_loses_six_db_per_doubling_and_sums_incoherently() {
        let rx = Receiver {
            position: [0.0; 3],
            axis: [1.0, 0.0, 0.0],
            directivity: 0.0,
        };
        let near = band_level_db(&[tone([1.0, 0.0, 0.0], 70.0)], &rx, 1000.0, 0.0);
        let far = band_level_db(&[tone([2.0, 0.0, 0.0], 70.0)], &rx, 1000.0, 0.0);
        assert!((near - 70.0).abs() < 1e-9);
        assert!((near - far - 6.0206).abs() < 1e-3);
        let two = band_level_db(
            &[tone([1.0, 0.0, 0.0], 70.0), tone([0.0, 1.0, 0.0], 70.0)],
            &rx,
            1000.0,
            0.0,
        );
        assert!(
            (two - 73.0103).abs() < 1e-3,
            "two equal sources add 3 dB: {two}"
        );
    }

    #[test]
    fn head_shadow_gives_level_and_time_differences_toward_the_source() {
        // Ears 10 cm apart facing outward; a source 1 m to the left.
        let left = ear([0.0, 0.05, 0.0], [0.0, 1.0, 0.0]);
        let right = ear([0.0, -0.05, 0.0], [0.0, -1.0, 0.0]);
        let src = [tone([0.0, 1.0, 0.0], 70.0)];
        let ild =
            band_level_db(&src, &left, 4000.0, 0.0) - band_level_db(&src, &right, 4000.0, 0.0);
        let ild_low =
            band_level_db(&src, &left, 250.0, 0.0) - band_level_db(&src, &right, 250.0, 0.0);
        assert!(ild > 10.0, "high band ILD {ild}");
        assert!(
            ild_low < ild && ild_low > 0.0,
            "low band ILD {ild_low} smaller"
        );
        // a = right, b = left: the sound reaches b (left) first -> towards 1.
        let itd = itd_normalised(&src, &right, &left, 1000.0, 0.0);
        assert!(itd > 0.9, "itd {itd}");
        let ahead = itd_normalised(&[tone([1.0, 0.0, 0.0], 70.0)], &right, &left, 1000.0, 0.0);
        assert!((ahead - 0.5).abs() < 1e-6);
    }

    #[test]
    fn spectra_interpolate_in_log_frequency_and_schedules_gate() {
        let mut s = SoundSource {
            id: "song".into(),
            position: [0.0; 3],
            spectrum: vec![[100.0, 60.0], [1000.0, 80.0]],
            period_s: Some(0.035),
            on_s: Some(0.01),
            start_s: 1.0,
        };
        assert!((s.level_at_1m(316.227766) - 70.0).abs() < 1e-6);
        assert_eq!(s.level_at_1m(50.0), 60.0);
        assert_eq!(s.level_at_1m(5000.0), 80.0);
        assert!(!s.sounding(0.5));
        assert!(s.sounding(1.005));
        assert!(!s.sounding(1.02));
        s.period_s = None;
        assert!(s.sounding(3.0));
        assert_eq!(normalised_level(60.0, 20.0, 100.0), 0.5);
        assert_eq!(normalised_level(-200.0, 20.0, 100.0), 0.0);
    }
}
