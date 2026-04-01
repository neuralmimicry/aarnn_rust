//! Shared spike encoding and decoding helpers.
//!
//! The functions here are intentionally small and composable so transports and
//! network-specific profiles can build their own policies on top.

use serde::{Deserialize, Serialize};

/// Broad class of external source the spike train originated from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalDomain {
    Digital,
    Analog,
    Biological,
    Physical,
    Hybrid,
    Synthetic,
}

/// Temporal context used by time-based encoders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TemporalEncodingContext {
    pub step_index: usize,
    pub time_ms: f32,
    pub dt_ms: f32,
}

impl Default for TemporalEncodingContext {
    fn default() -> Self {
        Self {
            step_index: 0,
            time_ms: 0.0,
            dt_ms: 1.0,
        }
    }
}

/// Parameters for probabilistic/rate-style coding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RateEncoding {
    /// Probability floor used when some values are subthreshold but still meaningful.
    pub quiet_floor: f32,
    /// Additional floor boost applied when the whole source vector is quiet.
    pub quiet_floor_boost: f32,
    /// Values below `high_value_threshold` use `quiet_floor + low_gain * value`.
    pub low_gain: f32,
    /// Crossover between low-rate and high-rate branches.
    pub high_value_threshold: f32,
    /// Base probability used on the high-rate branch.
    pub high_value_bias: f32,
    /// Additional high-rate gain applied above `high_value_threshold`.
    pub high_value_scale: f32,
    /// Clamp for the low-rate branch.
    pub max_low_probability: f32,
    /// Clamp for the overall probability.
    pub max_probability: f32,
    /// Values at or above this threshold always emit a spike.
    pub hard_fire_threshold: f32,
    /// Values below or equal to this threshold are treated as silent.
    pub silence_threshold: f32,
}

impl Default for RateEncoding {
    fn default() -> Self {
        Self {
            quiet_floor: 0.002,
            quiet_floor_boost: 2.0,
            low_gain: 0.22,
            high_value_threshold: 0.5,
            high_value_bias: 0.82,
            high_value_scale: 0.18,
            max_low_probability: 0.95,
            max_probability: 1.0,
            hard_fire_threshold: 0.999,
            silence_threshold: 0.0,
        }
    }
}

/// Parameters for time-to-first-spike coding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TtfsEncoding {
    pub threshold: f32,
    pub window_steps: usize,
}

impl Default for TtfsEncoding {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            window_steps: 16,
        }
    }
}

/// Parameters for inter-spike-interval coding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IsiEncoding {
    pub threshold: f32,
    pub min_interval_steps: usize,
    pub max_interval_steps: usize,
}

impl Default for IsiEncoding {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            min_interval_steps: 1,
            max_interval_steps: 16,
        }
    }
}

/// Parameters for phase coding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhaseEncoding {
    pub threshold: f32,
    pub frequency_hz: f32,
    pub phase_jitter: f32,
    pub phase_span: f32,
}

impl Default for PhaseEncoding {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            frequency_hz: 6.0,
            phase_jitter: 0.0,
            phase_span: std::f32::consts::TAU,
        }
    }
}

#[inline]
fn sanitize_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[inline]
fn deterministic_phase_offset(index: usize, jitter: f32) -> f32 {
    if jitter <= 0.0 {
        return 0.0;
    }
    let h = (index as u32).wrapping_mul(2654435761) & 0xFFFF;
    let base = (h as f32) / 65535.0;
    base * std::f32::consts::TAU * jitter.clamp(0.0, 1.0)
}

/// Hard-threshold a continuous signal into a spike vector.
pub fn threshold_encode(inputs: &[f32], dst: &mut [i8], threshold: f32) {
    dst.fill(0);
    let threshold = threshold.clamp(0.0, 1.0);
    for i in 0..dst.len().min(inputs.len()) {
        dst[i] = if sanitize_unit(inputs[i]) >= threshold {
            1
        } else {
            0
        };
    }
}

/// Encode one value per neuron using probabilistic/rate coding.
pub fn rate_encode_with<F>(inputs: &[f32], dst: &mut [i8], mut sample: F, cfg: RateEncoding)
where
    F: FnMut() -> f32,
{
    dst.fill(0);
    let all_quiet = inputs.iter().all(|v| sanitize_unit(*v) <= 1.0e-3);
    let floor = if all_quiet {
        (cfg.quiet_floor * cfg.quiet_floor_boost).clamp(0.0, 0.2)
    } else {
        cfg.quiet_floor.clamp(0.0, 0.1)
    };

    for i in 0..dst.len().min(inputs.len()) {
        let value = sanitize_unit(inputs[i]);
        if value >= cfg.hard_fire_threshold {
            dst[i] = 1;
            continue;
        }
        if value <= cfg.silence_threshold {
            continue;
        }
        let probability = if value >= cfg.high_value_threshold {
            (cfg.high_value_bias + cfg.high_value_scale * value).min(cfg.max_probability)
        } else {
            (floor + cfg.low_gain * value).clamp(0.0, cfg.max_low_probability)
        };
        if probability > 0.0 && sample() < probability {
            dst[i] = 1;
        }
    }
}

/// Encode one value per neuron using `fastrand`.
pub fn rate_encode(inputs: &[f32], dst: &mut [i8], cfg: RateEncoding) {
    rate_encode_with(inputs, dst, fastrand::f32, cfg);
}

/// Replicate deterministic threshold coding across a neuron population.
pub fn population_threshold_encode(
    values: &[f32],
    dst: &mut [i8],
    neurons_per_value: usize,
    threshold: f32,
) {
    dst.fill(0);
    let neurons_per_value = neurons_per_value.max(1);
    let threshold = threshold.clamp(0.0, 1.0);
    for (value_idx, value) in values.iter().enumerate() {
        let spike = if sanitize_unit(*value) >= threshold {
            1
        } else {
            0
        };
        let start = value_idx * neurons_per_value;
        if start >= dst.len() {
            break;
        }
        let end = (start + neurons_per_value).min(dst.len());
        for target in &mut dst[start..end] {
            *target = spike;
        }
    }
}

/// Replicate probabilistic/rate coding across a neuron population.
pub fn population_rate_encode_with<F>(
    values: &[f32],
    dst: &mut [i8],
    neurons_per_value: usize,
    mut sample: F,
) where
    F: FnMut() -> f32,
{
    dst.fill(0);
    let neurons_per_value = neurons_per_value.max(1);
    for (value_idx, value) in values.iter().enumerate() {
        let probability = sanitize_unit(*value);
        let start = value_idx * neurons_per_value;
        if start >= dst.len() {
            break;
        }
        let end = (start + neurons_per_value).min(dst.len());
        for target in &mut dst[start..end] {
            if sample() < probability {
                *target = 1;
            }
        }
    }
}

/// Replicate probabilistic/rate coding across a population using `fastrand`.
pub fn population_rate_encode(values: &[f32], dst: &mut [i8], neurons_per_value: usize) {
    population_rate_encode_with(values, dst, neurons_per_value, fastrand::f32);
}

/// Deterministic level/population coding where larger values activate more neurons.
pub fn population_level_encode(values: &[f32], dst: &mut [i8], neurons_per_value: usize) {
    dst.fill(0);
    let neurons_per_value = neurons_per_value.max(1);
    for (value_idx, value) in values.iter().enumerate() {
        let value = sanitize_unit(*value);
        let active = (value * neurons_per_value as f32).round() as usize;
        let start = value_idx * neurons_per_value;
        if start >= dst.len() {
            break;
        }
        let end = (start + neurons_per_value).min(dst.len());
        let active = active.min(end - start);
        for offset in 0..active {
            dst[start + offset] = 1;
        }
    }
}

/// Decode a population-coded spike vector into unit-interval values.
pub fn population_decode_average(spikes: &[i8], dst: &mut [f32], neurons_per_value: usize) {
    dst.fill(0.0);
    let neurons_per_value = neurons_per_value.max(1);
    for (value_idx, out) in dst.iter_mut().enumerate() {
        let start = value_idx * neurons_per_value;
        if start >= spikes.len() {
            break;
        }
        let end = (start + neurons_per_value).min(spikes.len());
        let mut acc = 0.0f32;
        let mut count = 0usize;
        for &spike in &spikes[start..end] {
            acc += if spike > 0 { 1.0 } else { 0.0 };
            count += 1;
        }
        if count > 0 {
            *out = acc / count as f32;
        }
    }
}

/// Convert a spike vector directly into unit-interval values.
pub fn spikes_to_unit_interval(spikes: &[i8], dst: &mut [f32]) {
    dst.fill(0.0);
    for i in 0..dst.len().min(spikes.len()) {
        dst[i] = if spikes[i] != 0 { 1.0 } else { 0.0 };
    }
}

/// Time-to-first-spike coding across a periodic window.
pub fn ttfs_encode(
    values: &[f32],
    dst: &mut [i8],
    ctx: TemporalEncodingContext,
    cfg: TtfsEncoding,
) {
    dst.fill(0);
    let window_steps = cfg.window_steps.max(1);
    let step = ctx.step_index % window_steps;
    for i in 0..dst.len().min(values.len()) {
        let value = sanitize_unit(values[i]);
        if value <= cfg.threshold {
            continue;
        }
        let target = ((1.0 - value) * (window_steps.saturating_sub(1)) as f32).round() as usize;
        if step == target {
            dst[i] = 1;
        }
    }
}

/// Inter-spike-interval coding where stronger values spike more often.
pub fn isi_encode(values: &[f32], dst: &mut [i8], ctx: TemporalEncodingContext, cfg: IsiEncoding) {
    dst.fill(0);
    let min_interval = cfg.min_interval_steps.max(1);
    let max_interval = cfg.max_interval_steps.max(min_interval);
    let span = max_interval.saturating_sub(min_interval);

    for i in 0..dst.len().min(values.len()) {
        let value = sanitize_unit(values[i]);
        if value <= cfg.threshold {
            continue;
        }
        let interval = max_interval.saturating_sub((value * span as f32).round() as usize);
        if ctx.step_index % interval.max(1) == 0 {
            dst[i] = 1;
        }
    }
}

/// Phase coding relative to a reference oscillation.
pub fn phase_encode(
    values: &[f32],
    dst: &mut [i8],
    ctx: TemporalEncodingContext,
    cfg: PhaseEncoding,
) {
    dst.fill(0);
    let time_s = ctx.time_ms.max(0.0) / 1000.0;
    let base_phase = std::f32::consts::TAU * cfg.frequency_hz.max(0.01) * time_s;
    let threshold = cfg.threshold.clamp(0.0, 1.0);

    for i in 0..dst.len().min(values.len()) {
        let value = sanitize_unit(values[i]);
        let phase =
            base_phase + deterministic_phase_offset(i, cfg.phase_jitter) + value * cfg.phase_span;
        let gate = phase.sin() * 0.5 + 0.5;
        if gate >= threshold {
            dst[i] = 1;
        }
    }
}

/// Combine multiple spike trains into one multiplexed train using logical OR.
pub fn multiplex_or(dst: &mut [i8], trains: &[&[i8]]) {
    dst.fill(0);
    for train in trains {
        for i in 0..dst.len().min(train.len()) {
            if train[i] != 0 {
                dst[i] = 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_encoding_is_binary() {
        let mut out = vec![0i8; 4];
        threshold_encode(&[0.1, 0.5, 0.6, 1.0], &mut out, 0.5);
        assert_eq!(out, vec![0, 1, 1, 1]);
    }

    #[test]
    fn population_level_encoding_scales_with_value() {
        let mut out = vec![0i8; 8];
        population_level_encode(&[0.25, 0.75], &mut out, 4);
        assert_eq!(out, vec![1, 0, 0, 0, 1, 1, 1, 0]);
    }

    #[test]
    fn population_decode_returns_average_activity() {
        let mut out = vec![0.0f32; 2];
        population_decode_average(&[1, 1, 0, 0, 1, 0], &mut out, 3);
        assert_eq!(out, vec![2.0 / 3.0, 1.0 / 3.0]);
    }

    #[test]
    fn ttfs_fires_earlier_for_stronger_values() {
        let cfg = TtfsEncoding {
            threshold: 0.0,
            window_steps: 4,
        };
        let strong = [1.0];
        let weak = [0.25];
        let mut strong_out = vec![0i8; 1];
        let mut weak_out = vec![0i8; 1];

        ttfs_encode(
            &strong,
            &mut strong_out,
            TemporalEncodingContext {
                step_index: 0,
                ..Default::default()
            },
            cfg,
        );
        ttfs_encode(
            &weak,
            &mut weak_out,
            TemporalEncodingContext {
                step_index: 2,
                ..Default::default()
            },
            cfg,
        );

        assert_eq!(strong_out, vec![1]);
        assert_eq!(weak_out, vec![1]);
    }

    #[test]
    fn multiplex_or_merges_trains() {
        let mut out = vec![0i8; 4];
        multiplex_or(&mut out, &[&[1, 0, 0, 1], &[0, 1, 0, 0], &[0, 0, 1, 0]]);
        assert_eq!(out, vec![1, 1, 1, 1]);
    }
}
