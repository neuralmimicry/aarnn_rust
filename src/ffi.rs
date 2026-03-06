//! # Foreign Function Interface (FFI) Bridge
//!
//! This module provides a C-compatible API for integrating the neuromorphic
//! simulation engine into external applications written in C, C++, or Python.
//!
//! It is specifically designed for ultra-low-latency, in-process communication,
//! making it suitable for high-frequency robot control loops (e.g., within a
//! Webots controller).
//!
//! ## Workflow
//! 1. `nm_init()`: Initialize the engine with a JSON config.
//! 2. `nm_set_port_by_index()`: Copy sensor data from a C-style array into the engine.
//! 3. `nm_step()`: Advance the neural network simulation by one time step.
//! 4. `nm_get_port_by_index()`: Copy processed actuator values from the engine back to a C-style array.
//! 5. `nm_shutdown()`: Clean up resources.

#![cfg(feature = "ffi_bridge")]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::sync::{Mutex, OnceLock};

use crate::config::{LIFParams, NetworkConfig, STDPParams};
#[cfg(feature = "ui")]
use crate::runner::Runner;
use crate::bridge::{ExternalRunnerBridge, InMemoryAdapter, IoMapping, PortKind, PortSpec, Quantizer};

struct State {
    map: IoMapping,
    #[cfg(feature = "ui")]
    bridge: ExternalRunnerBridge<InMemoryAdapter, InMemoryAdapter>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn parse_config(json: &str) -> anyhow::Result<(IoMapping, Runner, f32)> {
    // Schema: {"sensory":S, "output":O, "threshold":optional_f32, "s_names":[], "o_names":[]}
    #[derive(serde::Deserialize)]
    struct Cfg { 
        sensory: usize, 
        output: usize, 
        #[allow(dead_code)] threshold: Option<f32>,
        s_names: Option<Vec<String>>,
        o_names: Option<Vec<String>>,
    }
    let cfg: Cfg = serde_json::from_str(json)?;
    let mut map = IoMapping::new(cfg.sensory, cfg.output);
    
    if let Some(names) = cfg.s_names {
        for (i, name) in names.iter().enumerate() {
            map.add_port(PortSpec::new(name, PortKind::Sensor, i, 1));
        }
    } else {
        map.add_port(PortSpec::new("__S_ALL__", PortKind::Sensor, 0, cfg.sensory));
    }

    if let Some(names) = cfg.o_names {
        for (i, name) in names.iter().enumerate() {
            map.add_port(PortSpec::new(name, PortKind::Actuator, i, 1));
        }
    } else {
        map.add_port(PortSpec::new("__O_ALL__", PortKind::Actuator, 0, cfg.output));
    }

    // Build a small fixed Runner
    let lif = LIFParams::default();
    let stdp = STDPParams::default();
    let net = NetworkConfig { num_sensory_neurons: cfg.sensory, num_hidden_layers: 2, num_hidden_per_layer_initial: 32, num_output_neurons: cfg.output, ..NetworkConfig::default() };
    let runner = Runner::new(lif, stdp, net, crate::sim::NeuronModel::Lif, crate::sim::Learning::Stdp);
    let thr = cfg.threshold.unwrap_or(Quantizer::default().threshold);
    Ok((map, runner, thr))
}

#[no_mangle]
pub extern "C" fn nm_init(config_json: *const c_char) -> c_int {
    if config_json.is_null() { return -1; }
    let s = unsafe { CStr::from_ptr(config_json) };
    let json = match s.to_str() { Ok(x) => x, Err(_) => return -2 };
    let (map, runner, threshold) = match parse_config(json) { Ok(t) => t, Err(_) => return -3 };
    let sensor = InMemoryAdapter::new(map.clone());
    let actuator = InMemoryAdapter::new(map.clone());
    let quant = Quantizer { threshold, probabilistic: true };
    let bridge = ExternalRunnerBridge::new(runner, map.clone(), sensor, actuator, quant);
    let state_instance = State { map: map.clone(), bridge };
    if STATE.set(Mutex::new(state_instance)).is_err() { return -4; }
    0
}

#[no_mangle]
pub extern "C" fn nm_set_port_by_index(start: usize, len: usize, data: *const f32) -> c_int {
    let state_mutex = match STATE.get() { Some(m) => m, None => return -1 };
    let mut guard = state_mutex.lock().unwrap();
    if start.checked_add(len).unwrap_or(usize::MAX) > guard.map.total_sensor_values() { return -2; }
    if data.is_null() { return -3; }
    let src = unsafe { std::slice::from_raw_parts(data, len) };
    // Fast path: write directly into the bridge.sensor adapter internal buffer
    guard.bridge.sensor.set_inputs_at(start, src);
    0
}

#[no_mangle]
pub extern "C" fn nm_get_port_by_index(start: usize, len: usize, out: *mut f32) -> c_int {
    let state_mutex = match STATE.get() { Some(m) => m, None => return -1 };
    let guard = state_mutex.lock().unwrap();
    if start.checked_add(len).unwrap_or(usize::MAX) > guard.map.total_actuator_values() { return -2; }
    if out.is_null() { return -3; }
    let dst = unsafe { std::slice::from_raw_parts_mut(out, len) };
    // Read directly from the bridge's actuator adapter (latest outputs)
    guard.bridge.actuator.get_outputs_at(start, dst);
    0
}

#[no_mangle]
pub extern "C" fn nm_step(t_ms: f64) -> c_int {
    let state_mutex = match STATE.get() { Some(m) => m, None => return -1 };
    let mut guard = state_mutex.lock().unwrap();
    // Step using the already staged bridge.sensor internal buffer
    let _ = guard.bridge.step(t_ms);
    0
}

#[no_mangle]
pub extern "C" fn nm_set_quantizer_threshold(threshold: f32) -> c_int {
    let state_mutex = match STATE.get() { Some(m) => m, None => return -1 };
    let mut guard = state_mutex.lock().unwrap();
    guard.bridge.quant.threshold = threshold;
    0
}

#[no_mangle]
pub extern "C" fn nm_shutdown() {
    // Drop state by replacing with a fresh OnceLock (not supported); simply leak for process lifetime
}
