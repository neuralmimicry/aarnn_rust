//! Generic robot IO bridge for mapping external sensors and actuators to the neural network.
//!
//! This module provides the infrastructure to connect the neuromorphic simulation
//! engine to real-world or simulated robotic systems (e.g., Webots). It handles the
//! conversion between analog signal values (floats) and the discrete spike events
//! (i8) used by the spiking neural network.
//!
//! ## Core Concepts
//! - **IO Mapping (`IoMapping`)**: Defines named "ports" (like "Sonar/Left" or "Motor/Left")
//!   and their corresponding indices in the network's input (sensory) and output layers.
//! - **Quantization (`Quantizer`)**: The process of encoding analog sensor values into
//!   stochastic (Poisson) or deterministic spike trains, and decoding output spikes
//!   back into analog control signals (e.g., via population averaging).
//! - **Adapters (`SensorSource`, `ActuatorSink`)**: Traits that represent the external
//!   system. `InMemoryAdapter` is a common implementation for local testing.
//! - **Bridge (`ExternalRunnerBridge`)**: Orchestrates the data flow:
//!   `Sensors -> Quantize -> Network Step -> Dequantize -> Actuators`.
//!
//! ## Normalization
//! Ports can define `scale` and `bias` to automatically normalize raw sensor data
//! to the `[0.0, 1.0]` range expected by the Poisson quantizer.

use std::collections::HashMap;

/// Direction of a port range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PortKind {
    Sensor,
    Actuator,
}

/// Declarative description of a named port range.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct PortSpec {
    pub name: String,
    pub kind: PortKind,
    /// Start index within the flattened sensory or output vector.
    pub start: usize,
    /// Number of elements in this port.
    pub length_neurons: usize,
    /// Number of neurons used to represent each individual value in this port.
    /// Used for population encoding/decoding.
    pub neurons_per_value: usize,
    /// Optional normalization: scale (multiply) then add bias for floats.
    pub scale: f32,
    pub bias: f32,
}

#[allow(dead_code)]
impl PortSpec {
    pub fn new(name: impl Into<String>, kind: PortKind, start: usize, len: usize) -> Self {
        Self { name: name.into(), kind, start, length_neurons: len, neurons_per_value: 1, scale: 1.0, bias: 0.0 }
    }
    pub fn with_neurons_per_value(mut self, n: usize) -> Self { self.neurons_per_value = n.max(1); self }
    pub fn with_norm(mut self, scale: f32, bias: f32) -> Self { self.scale = scale; self.bias = bias; self }
}

/// Mapping of named ports to index ranges for both sensory and actuator sides.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct IoMapping {
    /// Total sensory size S expected by the network.
    pub sensory_size: usize,
    /// Total output size O produced by the network.
    pub output_size: usize,
    sensors: Vec<PortSpec>,
    actuators: Vec<PortSpec>,
    by_name: HashMap<String, PortSpec>,
}

#[allow(dead_code)]
impl IoMapping {
    pub fn new(sensory_size: usize, output_size: usize) -> Self {
        Self { sensory_size, output_size, sensors: vec![], actuators: vec![], by_name: HashMap::new() }
    }
    pub fn add_port(&mut self, port: PortSpec) {
        let name = port.name.clone();
        match port.kind {
            PortKind::Sensor => self.sensors.push(port.clone()),
            PortKind::Actuator => self.actuators.push(port.clone()),
        }
        self.by_name.insert(name, port);
    }
    pub fn port(&self, name: &str) -> Option<&PortSpec> { self.by_name.get(name) }
    pub fn sensors(&self) -> &[PortSpec] { &self.sensors }
    pub fn actuators(&self) -> &[PortSpec] { &self.actuators }

    pub fn total_sensor_values(&self) -> usize {
        self.sensors.iter().map(|p| p.length_neurons / p.neurons_per_value).sum()
    }

    pub fn total_actuator_values(&self) -> usize {
        self.actuators.iter().map(|p| p.length_neurons / p.neurons_per_value).sum()
    }

    pub fn get_sensor_label(&self, index: usize) -> String {
        for p in &self.sensors {
            if index >= p.start && index < p.start + p.length_neurons {
                if p.length_neurons == 1 { return p.name.clone(); }
                else { return format!("{}[{}]", p.name, index - p.start); }
            }
        }
        format!("S{}", index)
    }

    pub fn get_actuator_label(&self, index: usize) -> String {
        for p in &self.actuators {
            if index >= p.start && index < p.start + p.length_neurons {
                if p.length_neurons == 1 { return p.name.clone(); }
                else { return format!("{}[{}]", p.name, index - p.start); }
            }
        }
        format!("O{}", index)
    }
}

/// Source of sensor data as contiguous float vector (length = `sensory_size`).
#[allow(dead_code)]
pub trait SensorSource {
    /// Fill the provided slice with the latest sensor values at time `t_ms`.
    /// Implementations should write exactly `inputs.len()` elements.
    fn fill_inputs(&mut self, t_ms: f64, inputs: &mut [f32]);
    /// Optional external reward channel (0..1). Default is None.
    fn reward(&self) -> Option<f32> { None }
}

/// Sink for actuator commands as contiguous float vector (length = `output_size`).
#[allow(dead_code)]
pub trait ActuatorSink {
    /// Consume network outputs for time `t_ms`.
    fn consume_outputs(&mut self, t_ms: f64, outputs: &[f32]);
}

/// Utility for synchronizing internal simulation time with an external clock source.
///
/// This provides a generic way to derive simulation deltas from external
/// timestamps or provided deltas, used to keep the neuromorphic engine in sync
/// with simulators like Webots.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct TimeSync {
    /// Last absolute external time received, if any.
    pub last_external_time: Option<f64>,
}

#[allow(dead_code)]
impl TimeSync {
    pub fn new() -> Self { Self { last_external_time: None } }

    /// Calculate simulation delta (dt) from an external time value.
    ///
    /// - If `is_delta` is true, `val` is treated as a provided delta (e.g. Webots basicTimeStep).
    /// - If `is_delta` is false, `val` is treated as an absolute timestamp; Δt is computed
    ///   as the difference from the last call.
    pub fn sync_dt(&mut self, val: f64, is_delta: bool, fallback_dt: f64) -> f64 {
        if is_delta {
            self.last_external_time = None; // Reset mode
            if val > 0.0 { val } else { fallback_dt }
        } else {
            let dt = if let Some(prev) = self.last_external_time {
                val - prev
            } else {
                fallback_dt
            };
            self.last_external_time = Some(val);
            if dt > 0.0 { dt } else { fallback_dt }
        }
    }
}

/// Simple spike quantizer from float bands to binary spikes.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct Quantizer {
    /// Threshold above which a value becomes a spike (1), otherwise 0.
    /// Only used if neurons_per_value is 1 and probabilistic is false.
    pub threshold: f32,
    /// If true, use probabilistic (Poisson) encoding instead of fixed thresholding.
    pub probabilistic: bool,
}

impl Default for Quantizer { fn default() -> Self { Self { threshold: 0.5, probabilistic: true } } }

#[allow(dead_code)]
impl Quantizer {
    pub fn to_spikes(&self, mapping: &IoMapping, inputs: &[f32], dst: &mut [i8]) {
        let mut in_idx = 0;
        for p in mapping.sensors() {
            let num_vals = p.length_neurons / p.neurons_per_value;
            for v in 0..num_vals {
                let val = inputs.get(in_idx).copied().unwrap_or(0.0);
                let val = (val * p.scale + p.bias).clamp(0.0, 1.0);
                in_idx += 1;
                
                for n in 0..p.neurons_per_value {
                    let target = p.start + v * p.neurons_per_value + n;
                    if target < dst.len() {
                        if self.probabilistic {
                            dst[target] = if fastrand::f32() < val { 1 } else { 0 };
                        } else {
                            dst[target] = if val >= self.threshold { 1 } else { 0 };
                        }
                    }
                }
            }
        }
    }

    pub fn from_spikes(&self, mapping: &IoMapping, spikes: &[i8], dst: &mut [f32]) {
        let mut out_idx = 0;
        for p in mapping.actuators() {
            let num_vals = p.length_neurons / p.neurons_per_value;
            for v in 0..num_vals {
                let mut acc = 0.0;
                for n in 0..p.neurons_per_value {
                    let src = p.start + v * p.neurons_per_value + n;
                    if src < spikes.len() && spikes[src] > 0 {
                        acc += 1.0;
                    }
                }
                if out_idx < dst.len() {
                    dst[out_idx] = acc / (p.neurons_per_value as f32);
                    out_idx += 1;
                }
            }
        }
    }
}

/// A thread-safe in-memory adapter that lets external code push sensor values
/// per named port and read back actuator values after each step.
///
/// This is useful as a generic bridge layer; a Webots controller can update the
/// sensor ports and poll actuator ports via any IPC you prefer. This crate does
/// not prescribe the transport.
#[derive(Clone)]
#[allow(dead_code)]
pub struct InMemoryAdapter {
    mapping: IoMapping,
    // Flattened buffers matching S and O
    inputs: Vec<f32>,
    outputs: Vec<f32>,
    // Per-port scratch maps (optional usage by host code)
}

#[allow(dead_code)]
impl InMemoryAdapter {
    pub fn new(mapping: IoMapping) -> Self {
        let inputs = vec![0.0; mapping.total_sensor_values()];
        let outputs = vec![0.0; mapping.total_actuator_values()];
        Self { mapping, inputs, outputs }
    }
    pub fn mapping(&self) -> &IoMapping { &self.mapping }
    /// Overwrite a contiguous slice of the internal sensory buffer.
    pub fn set_inputs_at(&mut self, start: usize, data: &[f32]) {
        let end = start.saturating_add(data.len());
        if end <= self.inputs.len() { self.inputs[start..end].copy_from_slice(data); }
    }
    /// Read a contiguous slice of the internal actuator buffer.
    pub fn get_outputs_at(&self, start: usize, out: &mut [f32]) {
        let end = start.saturating_add(out.len());
        if end <= self.outputs.len() { out.copy_from_slice(&self.outputs[start..end]); }
    }
    pub fn set_port(&mut self, name: &str, values: &[f32]) {
        if let Some(p) = self.mapping.port(name) {
            let num_vals = p.length_neurons / p.neurons_per_value;
            if num_vals == values.len() {
                // We need to find the start index in the COMPRESSED inputs buffer.
                // This requires iterating sensors to find the port's relative start.
                let mut current_offset = 0;
                for s in self.mapping.sensors() {
                    if s.name == p.name { break; }
                    current_offset += s.length_neurons / s.neurons_per_value;
                }
                for (i, &v) in values.iter().enumerate() {
                    if current_offset + i < self.inputs.len() {
                        self.inputs[current_offset + i] = v * p.scale + p.bias;
                    }
                }
            }
        }
    }
    pub fn get_port(&self, name: &str, out: &mut [f32]) {
        if let Some(p) = self.mapping.port(name) {
            let num_vals = p.length_neurons / p.neurons_per_value;
            if num_vals == out.len() {
                let mut current_offset = 0;
                for a in self.mapping.actuators() {
                    if a.name == p.name { break; }
                    current_offset += a.length_neurons / a.neurons_per_value;
                }
                for (i, d) in out.iter_mut().enumerate() {
                    if current_offset + i < self.outputs.len() {
                        *d = self.outputs[current_offset + i];
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
impl SensorSource for InMemoryAdapter {
    fn fill_inputs(&mut self, _t_ms: f64, inputs: &mut [f32]) {
        inputs.copy_from_slice(&self.inputs);
    }
}

#[allow(dead_code)]
impl ActuatorSink for InMemoryAdapter {
    fn consume_outputs(&mut self, _t_ms: f64, outputs: &[f32]) { self.outputs.copy_from_slice(outputs); }
}

#[cfg(feature = "ui")]
use crate::runner::{Runner, StepOut};

/// Optional glue for driving the interactive `Runner` with external IO.
///
/// Typical usage per time step:
/// - `sensor.fill_inputs(t, &mut in_f32)`
/// - quantize to spikes and call `runner.step(Some(&spikes))`
/// - convert `spk_o` to floats and pass to `actuator.consume_outputs`
/// - optional: `sensor.reward()` feeds the runner's external reward channel
#[cfg(feature = "ui")]
#[allow(dead_code)]
pub struct ExternalRunnerBridge<S: SensorSource, A: ActuatorSink> {
    pub runner: Runner,
    pub mapping: IoMapping,
    pub sensor: S,
    pub actuator: A,
    pub quant: Quantizer,
    pub sync: TimeSync,
    pub in_buf: Vec<f32>,
    pub spk_s: Vec<i8>,
    pub out_buf: Vec<f32>,
}

#[cfg(feature = "ui")]
impl<S: SensorSource, A: ActuatorSink> ExternalRunnerBridge<S, A> {
    #[allow(dead_code)]
    pub fn new(runner: Runner, mapping: IoMapping, sensor: S, actuator: A, quant: Quantizer) -> Self {
        let in_buf = vec![0.0; mapping.total_sensor_values()];
        let spk_s = vec![0; mapping.sensory_size];
        let out_buf = vec![0.0; mapping.total_actuator_values()];
        Self { runner, mapping, sensor, actuator, quant, sync: TimeSync::new(), in_buf, spk_s, out_buf }
    }
    /// Advance one simulation step using external IO.
    ///
    /// t_ms is the simulation time value from the external source.
    /// By default it is treated as a delta (dt). To use absolute timestamps,
    /// call sync.sync_dt manually or use a specialized variant.
    #[allow(dead_code)]
    pub fn step(&mut self, t_ms: f64) -> StepOut {
        // Sync Runner's simulation time step with external delta (defaulting to delta mode)
        let dt = self.sync.sync_dt(t_ms, true, self.runner.lif.dt);
        self.runner.set_dt(dt);

        // Fill inputs and quantize to spikes
        self.sensor.fill_inputs(dt, &mut self.in_buf);
        self.runner.external_reward = self.sensor.reward().unwrap_or(0.0);
        self.quant.to_spikes(&self.mapping, &self.in_buf, &mut self.spk_s);
        // Step the runner with external sensory spikes
        let out = self.runner.step(Some(&self.spk_s));
        // Convert outputs to floats and publish to actuator sink
        self.quant.from_spikes(&self.mapping, out.spk_o.as_slice().unwrap(), &mut self.out_buf);
        self.actuator.consume_outputs(t_ms, &self.out_buf);
        out
    }
}
