// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Open-loop AARNN depth probe for connectome snapshots (C. elegans first).
//!
//! Reproduces the managed MuJoCo IPC frame contract without a body:
//! every 2 ms source frame, each sensor value in [0, 1] drives a phase
//! accumulator (the embodiment client's spike encoder), the App encoder adds
//! its 1 % floor (8 % when the whole frame is silent), and the Runner takes
//! two `step_sync(1.0)` substeps with the frame spikes on the first. Output
//! spikes of both substeps are OR-ed, as in the reply path.
//!
//! The stimulus is either a recorded MuJoCo trace (`--input trace.jsonl`,
//! `sensors` per record, held between records) or a scripted protocol.
//! Results are written as JSON: per-neuron spike counts, a sparse raster,
//! synchrony and numerical-health checks. Nothing here touches the
//! production path; it is a measurement instrument.
//!
//! cargo run --release --example celegans_depth_probe -- \
//!   --network network_celegans.json --depth 3 --ms 4000 --out probe.json
use std::io::{BufRead, Write};
use std::path::PathBuf;

use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::runner::Runner;
use aarnn_rust::sim::{Learning, NeuronModel};

struct Args {
    network: PathBuf,
    depth: Option<usize>,
    ms: usize,
    seed: u64,
    input: Option<PathBuf>,
    out: PathBuf,
    raster: bool,
    set: Vec<(String, String)>,
    export_frozen: Option<PathBuf>,
    /// Time series of every hidden cell's activity (Hz; spike rate or graded
    /// spike-equivalent rate) and output activation, one JSON line per window.
    series: Option<PathBuf>,
    series_ms: usize,
    /// Hidden cells whose incoming recurrent synapses (nonzero weights) are
    /// added to every series line, to follow learning over time.
    watch_rows: Vec<String>,
    /// Write every hidden cell's incoming morphology routes (presynaptic
    /// name, weight, delay steps, attenuation) after the run.
    route_report: Option<PathBuf>,
    /// Write the network (learned weights, runtime state, frozen
    /// morphology) after the run, for a later closed-loop test.
    export_after: Option<PathBuf>,
    dump_synapse_gains: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        network: PathBuf::from("network_celegans.json"),
        depth: None,
        ms: 2000,
        seed: 7,
        input: None,
        out: PathBuf::from("celegans_depth_probe.json"),
        raster: true,
        set: Vec::new(),
        export_frozen: None,
        series: None,
        series_ms: 20,
        watch_rows: Vec::new(),
        route_report: None,
        export_after: None,
        dump_synapse_gains: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--network" => a.network = PathBuf::from(v()?),
            "--depth" => a.depth = Some(v()?.parse().map_err(|_| "depth")?),
            "--ms" => a.ms = v()?.parse().map_err(|_| "ms")?,
            "--seed" => a.seed = v()?.parse().map_err(|_| "seed")?,
            "--input" => a.input = Some(PathBuf::from(v()?)),
            "--out" => a.out = PathBuf::from(v()?),
            "--no-raster" => a.raster = false,
            "--series" => a.series = Some(PathBuf::from(v()?)),
            "--series-ms" => a.series_ms = v()?.parse().map_err(|_| "series-ms")?,
            "--route-report" => a.route_report = Some(PathBuf::from(v()?)),
            "--export-after" => a.export_after = Some(PathBuf::from(v()?)),
            "--dump-synapse-gains" => a.dump_synapse_gains = Some(PathBuf::from(v()?)),
            "--watch-rows" => a.watch_rows = v()?.split(',').map(str::to_string).collect(),
            // Write the imported network with its built morphology frozen in
            // runtime state, so later loads (all depth variants) share one
            // anatomy instead of regrowing dendrites with thread-local RNGs.
            "--export-frozen" => a.export_frozen = Some(PathBuf::from(v()?)),
            // --set net.field=json  (applied to the imported NetworkConfig)
            "--set" => {
                let kv = v()?;
                let (key, value) = kv.split_once('=').ok_or("--set key=value")?;
                a.set.push((key.to_string(), value.to_string()));
            }
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    Ok(a)
}

/// Scripted protocol: quiet baseline, then each modality in turn, then a
/// travelling proprioceptive bend wave (head to tail, 0.5 Hz).
fn scripted(names: &[String], t_ms: f64) -> Vec<f64> {
    let mut v = vec![0.0; names.len()];
    let phase = |a: f64, b: f64| t_ms >= a && t_ms < b;
    for (i, n) in names.iter().enumerate() {
        let x = if n.contains("touch_front") && phase(1000.0, 1300.0) {
            1.0
        } else if n.contains("touch_rear") && phase(2000.0, 2300.0) {
            1.0
        } else if n.contains("chem_left") || n.contains("chem_right") {
            if phase(3000.0, 4000.0) { 0.6 } else { 0.0 }
        } else if n.contains("taste_front_center") && phase(4000.0, 5000.0) {
            0.7
        } else if n.contains("light_") && phase(5000.0, 5500.0) {
            0.9
        } else if let Some(rest) = n.split("_bend_").nth(1) {
            if t_ms >= 6000.0 {
                let dorsal = rest.starts_with("dorsal");
                let k: f64 = rest
                    .trim_start_matches(|c: char| c.is_alphabetic())
                    .parse()
                    .unwrap_or(1.0);
                let s = (std::f64::consts::TAU * (0.5 * t_ms / 1000.0 - k / 12.0 * 0.65)).sin();
                if dorsal {
                    (-s).max(0.0) * 0.6
                } else {
                    s.max(0.0) * 0.6
                }
            } else {
                0.0
            }
        } else {
            0.0
        };
        v[i] = x;
    }
    v
}

fn load_trace(path: &PathBuf) -> Result<Vec<(f64, Vec<f64>)>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in std::io::BufReader::new(file).lines().skip(1) {
        let line = line.map_err(|e| e.to_string())?;
        let rec: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let t = rec["time_s"].as_f64().unwrap_or(0.0) * 1000.0;
        if let Some(s) = rec["sensors"].as_array() {
            if !s.is_empty() {
                out.push((t, s.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()));
            }
        }
    }
    Ok(out)
}

fn main() -> Result<(), String> {
    let a = parse()?;
    let raw = std::fs::read_to_string(&a.network).map_err(|e| e.to_string())?;
    let labels: serde_json::Value = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|e| e.to_string())?["connectome_labels"]
        .clone();
    let mut runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        NetworkConfig::default(),
        NeuronModel::Aarnn,
        Learning::Aarnn,
    );
    runner
        .import_network_json(&raw)
        .map_err(|e| e.to_string())?;
    if let Some(d) = a.depth {
        runner.net.aarnn_layer_depth = d;
    }
    if !a.set.is_empty() {
        let mut net = serde_json::to_value(&runner.net).map_err(|e| e.to_string())?;
        for (k, v) in &a.set {
            let key = k.strip_prefix("net.").unwrap_or(k);
            let mut node = &mut net;
            let parts: Vec<&str> = key.split('.').collect();
            for p in &parts[..parts.len() - 1] {
                node = node.get_mut(*p).ok_or(format!("no field {p}"))?;
            }
            let leaf = parts[parts.len() - 1];
            if node.get(leaf).is_none() {
                return Err(format!("no field {key}"));
            }
            node[leaf] = serde_json::from_str(v).map_err(|e| format!("{key}: {e}"))?;
        }
        // Per-cell biology overrides must reach every typed neuron too:
        // typed cells carry their own `bio_params`, not the network default.
        for (k, v) in &a.set {
            if let Some(field) = k
                .strip_prefix("net.")
                .unwrap_or(k)
                .strip_prefix("aarnn_bio.")
            {
                if let Some(types) = net.get_mut("neuron_types").and_then(|t| t.as_array_mut()) {
                    for t in types {
                        t["bio_params"][field] =
                            serde_json::from_str(v).map_err(|e| format!("{k}: {e}"))?;
                    }
                }
            }
        }
        runner.net = serde_json::from_value(net).map_err(|e| e.to_string())?;
        runner.sync_bio_from_topo();
    }
    // The import seeds the thread RNG from the snapshot (fixed anatomy);
    // the probe seed then selects the stochastic input/noise sequence.
    fastrand::seed(a.seed);
    runner.set_dt(1.0);
    if let Some(path) = &a.dump_synapse_gains {
        // Spatial-mode calibration (scripts/calibrate_spatial_network.py):
        // every morphological synapse's route delay and attenuation (axon and
        // dendrite length, dendritic compartment gain, myelination, fatigue)
        // as the runtime computes them, plus its release probability.
        let mut lines = Vec::new();
        for (idx, syn) in runner.morph.synapses.iter().enumerate() {
            let (steps, atten) = runner.syn_delay_and_atten(idx);
            lines.push(
                serde_json::json!({
                    "pre_layer": syn.pre_layer, "pre_id": syn.pre_id,
                    "post_layer": syn.post_layer, "post_id": syn.post_id,
                    "weight": syn.weight, "p_release": syn.p_release,
                    "delay_steps": steps, "attenuation": atten,
                })
                .to_string(),
            );
        }
        std::fs::write(path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
        println!(
            "{} synapses (use_morphology={}, use_aarnn_delays={}, attenuation/unit={}, p_release={}) written to {}",
            lines.len(),
            runner.net.use_morphology,
            runner.net.use_aarnn_delays,
            runner.net.aarnn_distance_attenuation_per_unit,
            runner.net.p_release_default,
            path.display()
        );
        return Ok(());
    }
    if let Some(path) = &a.export_frozen {
        let mut frozen: serde_json::Value =
            serde_json::from_str(&runner.export_network_json().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        frozen["connectome_labels"] = labels.clone();
        frozen["connectome_labels"]["frozen_morphology"] = serde_json::json!({
            "source": a.network,
            "note": "morphology built once and persisted in runtime_state; imports restore it instead of regrowing"
        });
        std::fs::write(path, frozen.to_string()).map_err(|e| e.to_string())?;
        println!("frozen snapshot written to {}", path.display());
        return Ok(());
    }
    let names: Vec<String> = labels["sensory_nodes"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let s_count = runner.net.num_sensory_neurons;
    // With structural growth on, the hidden layer can grow during the run;
    // the per-cell tallies below follow it (`h_count` is updated each step).
    let mut h_count = runner.v_h.first().map(|v| v.len()).unwrap_or(0);
    let h_count_initial = h_count;
    let o_count = runner.net.num_output_neurons;
    let trace = match &a.input {
        Some(p) => Some(load_trace(p)?),
        None => None,
    };
    let pass_through = matches!(
        runner.net.spike_io.input_strategy,
        aarnn_rust::spike_io::profiles::SpikeInputEncodingStrategy::Threshold
    );
    let mut phase = vec![0.0f64; s_count];
    let mut counts_h = vec![0u64; h_count];
    let mut counts_o = vec![0u64; o_count];
    // Graded (non-spiking) cells: summed spike-equivalent rate per step.
    let mut graded_hz_sum = vec![0.0f64; h_count];
    let mut graded = vec![false; h_count];
    let mut activation_sum_o = vec![0.0f64; o_count];
    let mut series = match &a.series {
        Some(p) => Some(std::io::BufWriter::new(
            std::fs::File::create(p).map_err(|e| e.to_string())?,
        )),
        None => None,
    };
    let hidden_names: Vec<String> = labels["hidden_nodes"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let watch: Vec<(String, usize)> = a
        .watch_rows
        .iter()
        .filter_map(|n| {
            hidden_names
                .iter()
                .position(|h| h == n)
                .map(|i| (n.clone(), i))
        })
        .collect();
    let mut win_h = vec![0.0f64; h_count];
    let mut win_o = vec![0.0f64; o_count];
    // Output spikes per series window (spiking outputs report no graded
    // activation; adaptation tests need their time course too).
    let mut win_os = vec![0u32; o_count];
    let mut win_v = vec![0.0f64; h_count];
    let mut win_steps = 0usize;
    let mut raster_h: Vec<(u32, u16)> = Vec::new();
    let mut raster_o: Vec<(u32, u16)> = Vec::new();
    let mut active_frames_h = Vec::new();
    let mut nonfinite = 0u64;
    let mut v_min = f64::INFINITY;
    let mut v_max = f64::NEG_INFINITY;
    let mut i_abs_max = 0.0f64;
    let mut sensory_spikes = 0u64;
    let mut i_o_abs_max = 0.0f64;
    let mut i_o_pos_sum = 0.0f64;
    let started = std::time::Instant::now();
    let frames = a.ms / 2;
    let mut cursor = 0usize;
    for f in 0..frames {
        let t = (f * 2) as f64;
        let values: Vec<f64> = match &trace {
            Some(tr) => {
                while cursor + 1 < tr.len() && tr[cursor + 1].0 <= t {
                    cursor += 1;
                }
                tr.get(cursor)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| vec![0.0; s_count])
            }
            None => scripted(&names, t),
        };
        // Embodiment client: deterministic phase-accumulator encoder.
        let mut frame = vec![0i8; s_count];
        for i in 0..s_count {
            phase[i] += values.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            if phase[i] >= 1.0 {
                phase[i] -= 1.0;
                frame[i] = 1;
            }
        }
        // App celegans profile encoder on binary inputs: 1 -> always fires;
        // silent channels fire with the 1 % floor (8 % if the frame is silent).
        // The threshold strategy passes AER spikes through unchanged.
        let silent = frame.iter().all(|x| *x == 0);
        let floor = if pass_through {
            0.0
        } else if silent {
            0.08
        } else {
            0.01
        };
        for x in frame.iter_mut() {
            if *x == 0 && fastrand::f64() < floor {
                *x = 1;
            }
        }
        sensory_spikes += frame.iter().filter(|x| **x != 0).count() as u64;
        let zero = vec![0i8; s_count];
        let mut spk_o = vec![0i8; o_count];
        let mut frame_h = 0usize;
        for sub in 0..2 {
            let input = if sub == 0 { &frame } else { &zero };
            let _ = runner.step_sync(1.0, Some(input));
            let grown = runner.v_h.first().map(|v| v.len()).unwrap_or(0);
            if grown > h_count {
                counts_h.resize(grown, 0);
                graded_hz_sum.resize(grown, 0.0);
                graded.resize(grown, false);
                win_h.resize(grown, 0.0);
                win_v.resize(grown, 0.0);
                h_count = grown;
            }
            let tick = (f * 2 + sub) as u32;
            for (j, s) in runner.last_spk_h[0].iter().enumerate() {
                if *s != 0 {
                    counts_h[j] += 1;
                    frame_h += 1;
                    if a.raster {
                        raster_h.push((tick, j as u16));
                    }
                }
            }
            for (k, s) in runner.last_spk_o.iter().enumerate() {
                if *s != 0 {
                    spk_o[k] = 1;
                    if let Some(c) = win_os.get_mut(k) {
                        *c += 1;
                    }
                }
            }
            for (j, hz) in runner.graded_equivalent_rate_h(0).into_iter().enumerate() {
                if let (Some(hz), true) = (hz, j < h_count) {
                    graded[j] = true;
                    graded_hz_sum[j] += hz;
                    win_h[j] += hz;
                } else if j < h_count && runner.last_spk_h[0][j] != 0 {
                    win_h[j] += 1000.0;
                }
            }
            for (k, act) in runner.graded_activation_o().into_iter().enumerate() {
                if let (Some(a), true) = (act, k < o_count) {
                    activation_sum_o[k] += a;
                    win_o[k] += a;
                }
            }
            for (j, v) in runner.v_h[0].iter().enumerate().take(h_count) {
                win_v[j] += v;
            }
            win_steps += 1;
            if let Some(w) = series.as_mut()
                && win_steps >= a.series_ms.max(1)
            {
                let n = win_steps as f64;
                let mut watched = serde_json::Map::new();
                if let Some(m) = runner.w_hh_rec.first() {
                    for (name, row) in &watch {
                        let mut inc = serde_json::Map::new();
                        for (col, &wv) in m.row(*row).iter().enumerate() {
                            if wv != 0.0 {
                                let pre = hidden_names
                                    .get(col)
                                    .cloned()
                                    .unwrap_or_else(|| col.to_string());
                                inc.insert(pre, serde_json::json!((wv * 1.0e6).round() / 1.0e6));
                            }
                        }
                        watched.insert(name.clone(), serde_json::Value::Object(inc));
                    }
                }
                let line = serde_json::json!({
                    "t_ms": tick + 1,
                    "hidden_hz": win_h.iter().map(|x| (x / n * 100.0).round() / 100.0).collect::<Vec<_>>(),
                    "output_activation": win_o.iter().map(|x| (x / n * 1000.0).round() / 1000.0).collect::<Vec<_>>(),
                    "output_spikes": win_os.clone(),
                    "hidden_v": win_v.iter().map(|x| (x / n * 100.0).round() / 100.0).collect::<Vec<_>>(),
                    "weights": watched,
                });
                writeln!(w, "{line}").map_err(|e| e.to_string())?;
                win_h.fill(0.0);
                win_o.fill(0.0);
                win_os.fill(0);
                win_v.fill(0.0);
                win_steps = 0;
            }
            for v in runner.v_h[0].iter().chain(runner.v_o.iter()) {
                if !v.is_finite() {
                    nonfinite += 1;
                } else {
                    v_min = v_min.min(*v);
                    v_max = v_max.max(*v);
                }
            }
            if let Some(i) = runner.last_i_o.as_ref() {
                for x in i.iter() {
                    i_o_abs_max = i_o_abs_max.max(x.abs());
                    i_o_pos_sum += x.max(0.0);
                }
            }
            if let Some(i) = runner.last_i_h0.as_ref() {
                for x in i.iter() {
                    if !x.is_finite() {
                        nonfinite += 1;
                    } else {
                        i_abs_max = i_abs_max.max(x.abs());
                    }
                }
            }
        }
        active_frames_h.push(frame_h as u32);
        for (k, s) in spk_o.iter().enumerate() {
            if *s != 0 {
                counts_o[k] += 1;
                if a.raster {
                    raster_o.push(((f * 2) as u32, k as u16));
                }
            }
        }
    }
    let routes = |r: &Vec<Vec<(usize, usize)>>| r.iter().map(|v| v.len()).sum::<usize>();
    let route_counts = serde_json::json!({
        "in": routes(&runner.recv_in),
        "rec0": runner.recv_rec.first().map(routes).unwrap_or(0),
        "out": routes(&runner.recv_out),
        "out_rows_without_route": runner.recv_out.iter().filter(|v| v.is_empty()).count(),
        "w_in_nonzero": runner.w_in.iter().filter(|w| w.abs() > 1e-12).count(),
        "w_rec_nonzero": runner.w_hh_rec.first().map(|m| m.iter().filter(|w| w.abs() > 1e-12).count()).unwrap_or(0),
        "w_rec_negative": runner.w_hh_rec.first().map(|m| m.iter().filter(|w| **w < -1e-12).count()).unwrap_or(0),
        "w_out_nonzero": runner.w_out.iter().filter(|w| w.abs() > 1e-12).count(),
        "w_out_negative": runner.w_out.iter().filter(|w| **w < -1e-12).count(),
        "hist_len": runner.spk_hist_s.len(),
        "hidden_initial": h_count_initial,
        "hidden_final": h_count,
        "hidden_grown": h_count.saturating_sub(h_count_initial),
    });
    let wall = started.elapsed().as_secs_f64();
    let secs = a.ms as f64 / 1000.0;
    let synchronous_frames = active_frames_h
        .iter()
        .filter(|n| **n as f64 > 0.3 * h_count as f64)
        .count();
    let out = serde_json::json!({
        "schema": "celegans-depth-probe-v1",
        "network": a.network,
        "depth": runner.net.aarnn_layer_depth,
        "ms": a.ms,
        "seed": a.seed,
        "input": a.input,
        "overrides": a.set,
        "wall_s": wall,
        "ms_per_frame": wall * 1000.0 / frames.max(1) as f64,
        "hidden_names": labels["hidden_nodes"],
        "output_names": labels["output_nodes"],
        "sensory_names": names,
        "hidden_rate_hz": counts_h.iter().map(|c| *c as f64 / secs).collect::<Vec<_>>(),
        "output_frame_rate_hz": counts_o.iter().map(|c| *c as f64 / secs).collect::<Vec<_>>(),
        // Spike rate for spiking cells, mean release x release_max_hz for graded cells.
        "hidden_graded": graded,
        "hidden_equiv_rate_hz": (0..h_count).map(|j| if graded[j] { graded_hz_sum[j] / (2.0 * frames.max(1) as f64) } else { counts_h[j] as f64 / secs }).collect::<Vec<_>>(),
        "output_mean_activation": activation_sum_o.iter().map(|s| s / (2.0 * frames.max(1) as f64)).collect::<Vec<_>>(),
        "output_code": runner.output_code(),
        "sensory_spikes_per_s": sensory_spikes as f64 / secs,
        "hidden_active_per_frame": active_frames_h,
        "synchronous_frames": synchronous_frames,
        "health": {"nonfinite": nonfinite, "v_min": v_min, "v_max": v_max, "i_h0_abs_max": i_abs_max, "i_o_abs_max": i_o_abs_max, "i_o_mean_pos": i_o_pos_sum / (2.0 * frames.max(1) as f64 * o_count.max(1) as f64)},
        "routes": route_counts,
        "raster_hidden": raster_h,
        "raster_output": raster_o,
    });
    if let Some(path) = &a.export_after {
        let mut learned: serde_json::Value =
            serde_json::from_str(&runner.export_network_json().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        learned["connectome_labels"] = labels.clone();
        learned["connectome_labels"]["experience"] = serde_json::json!({
            "source": a.network, "input": a.input, "ms": a.ms, "seed": a.seed,
            "note": "network after the probe run: weights learned during it, runtime state and morphology kept"
        });
        std::fs::write(path, learned.to_string()).map_err(|e| e.to_string())?;
    }
    if let Some(path) = &a.route_report {
        let sensory_names: Vec<String> = labels["sensory_nodes"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let mut cells = serde_json::Map::new();
        for (j, name) in hidden_names.iter().enumerate() {
            let mut routes = Vec::new();
            for &(i, si) in runner.recv_in.get(j).map(|v| v.as_slice()).unwrap_or(&[]) {
                let (steps, atten) = runner.syn_delay_and_atten(si);
                let w = runner.w_in.get((j, i)).copied().unwrap_or(0.0);
                routes.push(
                    serde_json::json!({"pre": sensory_names.get(i).cloned().unwrap_or_default(),
                    "kind": "sensory", "w": w, "steps": steps, "atten": atten}),
                );
            }
            for &(i, si) in runner
                .recv_rec
                .first()
                .and_then(|v| v.get(j))
                .map(|v| v.as_slice())
                .unwrap_or(&[])
            {
                let (steps, atten) = runner.syn_delay_and_atten(si);
                let w = runner
                    .w_hh_rec
                    .first()
                    .and_then(|m| m.get((j, i)))
                    .copied()
                    .unwrap_or(0.0);
                routes.push(
                    serde_json::json!({"pre": hidden_names.get(i).cloned().unwrap_or_default(),
                    "kind": "recurrent", "w": w, "steps": steps, "atten": atten}),
                );
            }
            cells.insert(name.clone(), serde_json::Value::Array(routes));
        }
        std::fs::write(path, serde_json::Value::Object(cells).to_string())
            .map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(&a.out).map_err(|e| e.to_string())?;
    writeln!(file, "{out}").map_err(|e| e.to_string())?;
    println!(
        "depth={} ms={} wall={:.2}s ({:.2} ms/frame) hidden_mean={:.2}Hz output_mean={:.2}Hz sync_frames={} nonfinite={} i_o_max={:.3} routes={}",
        runner.net.aarnn_layer_depth,
        a.ms,
        wall,
        wall * 1000.0 / frames.max(1) as f64,
        counts_h.iter().sum::<u64>() as f64 / secs / h_count.max(1) as f64,
        counts_o.iter().sum::<u64>() as f64 / secs / o_count.max(1) as f64,
        synchronous_frames,
        nonfinite,
        i_o_abs_max,
        route_counts
    );
    Ok(())
}
