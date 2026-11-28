// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Execution of a network through the authoritative AARNN `Runner` (no
//! second simulator). Used inside worker processes.
//!
//! Protocol, as the embodiment client and `examples/celegans_depth_probe.rs`
//! use it:
//! - a frame is 2 ms;
//! - sensor values in [0, 1] pass through a deterministic phase-accumulator
//!   encoder into a spike frame;
//! - the runner then steps twice: `step_sync(1.0, frame)` followed by a zero
//!   frame;
//! - outputs are read after each step: graded activation, or spikes / 2 for
//!   spiking outputs.
//!
//! Determinism: the network RNG is seeded with `runner.rng.seed(seed)` after
//! import (the runner reseeds `fastrand` from it every step). Task noise uses
//! its own `fastrand::Rng`. Deterministic replay additionally needs the
//! serial step path: the worker launcher sets `NM_SIM_PAR_MAX_WORKERS=1` and
//! `RAYON_NUM_THREADS=1`.

use std::time::Instant;

use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::runner::Runner;
use aarnn_rust::sim::{Learning, NeuronModel};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ExplorerError, Result};
use crate::gnat::{Event, EventKind};

/// Within-step stage, used as the event microstep (D-003).
pub const MICRO_SENSORY: u32 = 0;
pub const MICRO_HIDDEN: u32 = 1;
pub const MICRO_OUTPUT: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Silence {
    /// Hidden dense index.
    pub hidden: u32,
    pub start_tick: u64,
    pub end_tick: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    pub seed: u64,
    /// Sensor values per 2 ms frame (frames x channels).
    pub frames: Vec<Vec<f32>>,
    /// Per-channel per-frame probability of an extra background spike
    /// (task noise; drawn from `noise_seed`, never from the network RNG).
    #[serde(default)]
    pub background_p: f64,
    #[serde(default)]
    pub noise_seed: u64,
    /// Record trace events (spikes, graded equivalents).
    #[serde(default)]
    pub record_events: bool,
    /// Also emit spike-equivalent events for graded cells (D-005).
    #[serde(default)]
    pub graded_events: bool,
    /// `frozen`: plasticity and growth off for the run. `source`: as declared.
    pub plasticity_policy: String,
    #[serde(default)]
    pub silence: Vec<Silence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunOutput {
    pub frames: usize,
    pub ticks: u64,
    pub sensory: usize,
    pub hidden: usize,
    pub outputs: usize,
    /// Output value per frame (frames x outputs).
    pub output_series: Vec<Vec<f32>>,
    pub events: Vec<Event>,
    /// Events dropped from the trace with their reason (e.g. newborn cells
    /// outside topology generation 1); never silent.
    pub excluded_events: u64,
    pub exclusion_reasons: Vec<String>,
    pub step_ns_p50: u64,
    pub step_ns_p95: u64,
    pub step_ns_p99: u64,
    pub wall_ms: f64,
    pub hidden_spikes: u64,
    pub graded_cells: usize,
    /// FNV digest of the output series (observation must not change it).
    pub digest: String,
    /// FNV digest of the recorded events (empty when not recorded).
    pub events_digest: String,
}

/// Apply the plasticity policy to a snapshot before import.
pub fn apply_policy(snapshot: &mut Value, policy: &str) -> Result<()> {
    match policy {
        "frozen" => {
            let net = snapshot
                .get_mut("net")
                .ok_or_else(|| ExplorerError::InvalidInput("snapshot has no net".into()))?;
            for k in [
                "aarnn_plasticity_enabled",
                "growth_enabled",
                "morpho_growth_enabled",
            ] {
                net[k] = Value::Bool(false);
            }
            Ok(())
        }
        "source" => Ok(()),
        p => Err(ExplorerError::InvalidInput(format!(
            "plasticity policy {p:?}"
        ))),
    }
}

fn digest(series: &[Vec<f32>], events: &[Event]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut mix = |b: &[u8]| {
        for &x in b {
            h ^= x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    for row in series {
        for v in row {
            mix(&v.to_le_bytes());
        }
    }
    for e in events {
        mix(&e.tick.to_le_bytes());
        mix(&e.micro.to_le_bytes());
        mix(&e.neuron.to_le_bytes());
    }
    format!("{h:016x}")
}

fn percentile(v: &mut [u64], q: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[((v.len() - 1) as f64 * q).round() as usize]
}

/// Run one open-loop trial: sensor values come from `spec.frames`.
pub fn run(snapshot: &Value, spec: &RunSpec) -> Result<RunOutput> {
    let frames = spec.frames.clone();
    run_with(snapshot, spec, frames.len(), &mut |f, _| frames[f].clone())
}

/// Run one trial whose sensor frame `f` is produced by `input(f, outputs of
/// frame f-1)`. A closed-loop environment uses this so the controller always
/// receives the sensory consequences of its own actions (brief Section 10).
pub fn run_with(
    snapshot: &Value,
    spec: &RunSpec,
    n_frames: usize,
    input: &mut dyn FnMut(usize, &[f32]) -> Vec<f32>,
) -> Result<RunOutput> {
    let mut snap = snapshot.clone();
    apply_policy(&mut snap, &spec.plasticity_policy)?;
    if !spec.silence.is_empty() && spec.plasticity_policy != "frozen" {
        return Err(ExplorerError::Unsupported(
            "transient silencing restores the silenced cell's outgoing weights at the interval end, which would erase plastic changes: use plasticity_policy = frozen".into(),
        ));
    }
    let raw = snap.to_string();
    let mut runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        NetworkConfig::default(),
        NeuronModel::Aarnn,
        Learning::Aarnn,
    );
    runner
        .import_network_json(&raw)
        .map_err(|e| ExplorerError::InvalidInput(format!("runner import: {e}")))?;
    runner.rng.seed(spec.seed);
    runner.set_dt(1.0);
    let s = runner.net.num_sensory_neurons;
    let h0 = runner.v_h.first().map(|v| v.len()).unwrap_or(0);
    let o = runner.v_o.len();
    for sl in &spec.silence {
        if sl.hidden as usize >= h0 || sl.end_tick <= sl.start_tick {
            return Err(ExplorerError::InvalidInput(format!(
                "silence {sl:?}: bad cell or interval"
            )));
        }
    }
    let graded_mask: Vec<bool> = runner
        .graded_equivalent_rate_h(0)
        .iter()
        .map(|x| x.is_some())
        .collect();
    let mut noise = fastrand::Rng::with_seed(spec.noise_seed);
    let mut phase = vec![0.0f64; s];
    let mut gphase = vec![0.0f64; h0];
    let mut events = Vec::new();
    let mut seq: u64 = 0;
    let mut series: Vec<Vec<f32>> = Vec::with_capacity(n_frames);
    let mut excluded = 0u64;
    let mut reasons = Vec::new();
    let mut step_ns = Vec::with_capacity(n_frames * 2);
    let mut hidden_spikes = 0u64;
    let zero = vec![0i8; s];
    let mut saved: Vec<Option<(Vec<f64>, Vec<f64>)>> = vec![None; spec.silence.len()];
    let started = Instant::now();
    let (sg, og) = (s as u32, (s + h0) as u32);
    let no_outputs = vec![0.0f32; o];
    for f in 0..n_frames {
        let values = input(
            f,
            series.last().map(|v| v.as_slice()).unwrap_or(&no_outputs),
        );
        if values.len() != s {
            return Err(ExplorerError::InvalidInput(format!(
                "frame {f} has {} channels; the network has {s} sensory channels",
                values.len()
            )));
        }
        let mut frame = vec![0i8; s];
        for i in 0..s {
            phase[i] += (values[i] as f64).clamp(0.0, 1.0);
            if phase[i] >= 1.0 {
                phase[i] -= 1.0;
                frame[i] = 1;
            }
            if spec.background_p > 0.0 && frame[i] == 0 && noise.f64() < spec.background_p {
                frame[i] = 1;
            }
        }
        let mut out_acc = vec![0.0f32; o];
        for sub in 0..2u64 {
            let tick = f as u64 * 2 + sub;
            for (k, sl) in spec.silence.iter().enumerate() {
                let j = sl.hidden as usize;
                if tick == sl.start_tick && saved[k].is_none() {
                    let rec: Vec<f64> = (0..h0).map(|r| runner.w_hh_rec[0][(r, j)]).collect();
                    let out: Vec<f64> = (0..o).map(|r| runner.w_out[(r, j)]).collect();
                    for r in 0..h0 {
                        runner.w_hh_rec[0][(r, j)] = 0.0;
                    }
                    for r in 0..o {
                        runner.w_out[(r, j)] = 0.0;
                    }
                    saved[k] = Some((rec, out));
                }
                if tick == sl.end_tick {
                    if let Some((rec, out)) = saved[k].take() {
                        for r in 0..h0 {
                            runner.w_hh_rec[0][(r, j)] = rec[r];
                        }
                        for r in 0..o {
                            runner.w_out[(r, j)] = out[r];
                        }
                    }
                }
            }
            let input: &[i8] = if sub == 0 { &frame } else { &zero };
            let t0 = Instant::now();
            let _ = runner.step_sync(1.0, Some(input));
            step_ns.push(t0.elapsed().as_nanos() as u64);
            let h_now = runner.v_h.first().map(|v| v.len()).unwrap_or(0);
            if h_now != h0 && reasons.is_empty() {
                reasons.push(format!(
                    "growth changed the hidden layer from {h0} to {h_now} cells at tick {tick}: newborn events are outside topology generation 1 and excluded"
                ));
            }
            if spec.record_events {
                if sub == 0 {
                    for (i, &x) in frame.iter().enumerate() {
                        if x != 0 {
                            events.push(Event {
                                id: seq,
                                tick,
                                micro: MICRO_SENSORY,
                                neuron: i as u32,
                                kind: EventKind::Spike,
                            });
                            seq += 1;
                        }
                    }
                }
                for (j, &x) in runner.last_spk_h[0].iter().enumerate() {
                    if x != 0 {
                        if j < h0 {
                            events.push(Event {
                                id: seq,
                                tick,
                                micro: MICRO_HIDDEN,
                                neuron: sg + j as u32,
                                kind: EventKind::Spike,
                            });
                            seq += 1;
                        } else {
                            excluded += 1;
                        }
                    }
                }
                if spec.graded_events {
                    for (j, r) in runner.graded_equivalent_rate_h(0).into_iter().enumerate() {
                        if j >= h0 {
                            break;
                        }
                        if let Some(hz) = r {
                            gphase[j] += hz.max(0.0) / 1000.0;
                            if gphase[j] >= 1.0 {
                                gphase[j] -= 1.0;
                                events.push(Event {
                                    id: seq,
                                    tick,
                                    micro: MICRO_HIDDEN,
                                    neuron: sg + j as u32,
                                    kind: EventKind::GradedEquivalent,
                                });
                                seq += 1;
                            }
                        }
                    }
                }
                for (k, &x) in runner.last_spk_o.iter().enumerate() {
                    if x != 0 {
                        events.push(Event {
                            id: seq,
                            tick,
                            micro: MICRO_OUTPUT,
                            neuron: og + k as u32,
                            kind: EventKind::Spike,
                        });
                        seq += 1;
                    }
                }
            }
            hidden_spikes += runner.last_spk_h[0].iter().filter(|&&x| x != 0).count() as u64;
            let act = runner.graded_activation_o();
            for k in 0..o {
                out_acc[k] += match act.get(k).copied().flatten() {
                    Some(a) => a as f32 * 0.5,
                    None => runner
                        .last_spk_o
                        .get(k)
                        .map(|&x| if x != 0 { 0.5 } else { 0.0 })
                        .unwrap_or(0.0),
                };
            }
        }
        series.push(out_acc);
    }
    if excluded > 0 && reasons.is_empty() {
        reasons.push(format!("{excluded} newborn-cell events excluded"));
    }
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    let d = digest(&series, &[]);
    let ed = digest(&[], &events);
    let mut ns = step_ns;
    Ok(RunOutput {
        frames: n_frames,
        ticks: n_frames as u64 * 2,
        sensory: s,
        hidden: h0,
        outputs: o,
        output_series: series,
        events,
        excluded_events: excluded,
        exclusion_reasons: reasons,
        step_ns_p50: percentile(&mut ns, 0.5),
        step_ns_p95: percentile(&mut ns, 0.95),
        step_ns_p99: percentile(&mut ns, 0.99),
        wall_ms,
        hidden_spikes,
        graded_cells: graded_mask.iter().filter(|&&g| g).count(),
        digest: d,
        events_digest: ed,
    })
}
