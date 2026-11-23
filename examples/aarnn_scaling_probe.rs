// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Scaling probe: does a recurrent AARNN network separate and hold more
//! input patterns as it gains neurons?
//!
//! One run = one network size `N` and one seed. The network is a single
//! recurrent layer of `N` AARNN (Izhikevich RS, depth 1) cells with a fixed
//! in-degree (every cell receives `--k-rec` recurrent and `--k-in` input
//! synapses, 20 % of cells inhibitory, Dale's law), so only the number of
//! cells changes between sizes - not the drive any one cell receives.
//! Plasticity and growth are off: this measures the network as built.
//!
//! `--patterns` random input patterns over `--sensors` channels (25 % of
//! channels on) are each shown `--repeats` times in a shuffled order:
//! `--stim-ms` of the pattern (each presentation re-draws `--flip` of the
//! channels and the encoder phases, so no two presentations are alike),
//! then `--gap-ms` of background. For every presentation the output JSON
//! holds every cell's spike count while the pattern is on (`evoked`) and
//! in the window after it ends (`memory`). `scripts/qa/scaling_study.py`
//! decodes pattern identity from them (leave-one-out nearest centroid).
//!
//!   aarnn_scaling_probe --neurons 256 --seed 1 --out run.json

use aarnn_rust::config::{ClumpingDesign, LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::runner::Runner;
use aarnn_rust::sim::{Learning, NeuronModel};

struct Args {
    neurons: usize,
    seed: u64,
    sensors: usize,
    patterns: usize,
    repeats: usize,
    stim_ms: usize,
    gap_ms: usize,
    memory_ms: usize,
    k_rec: usize,
    k_in: usize,
    w_exc: f64,
    w_inh: f64,
    w_in: f64,
    drive: f64,
    background: f64,
    flip: f64,
    module_size: usize,
    attach: usize,
    branching: usize,
    within_p: f64,
    base: Option<String>,
    out: String,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        neurons: 128,
        seed: 1,
        sensors: 32,
        patterns: 32,
        repeats: 6,
        stim_ms: 100,
        gap_ms: 200,
        memory_ms: 150,
        k_rec: 20,
        k_in: 4,
        w_exc: 0.06,
        w_inh: 0.24,
        w_in: 0.5,
        drive: 0.2,
        background: 0.01,
        flip: 0.05,
        module_size: 0,
        attach: 2,
        branching: 0,
        within_p: 1.0,
        base: Some("target/qa/celegans-depth-study-forward-crawl/network_grafted.json".into()),
        out: "scaling.json".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        let num = |s: String| s.parse::<f64>().map_err(|e| e.to_string());
        match k.as_str() {
            "--neurons" => a.neurons = num(v()?)? as usize,
            "--seed" => a.seed = num(v()?)? as u64,
            "--sensors" => a.sensors = num(v()?)? as usize,
            "--patterns" => a.patterns = num(v()?)? as usize,
            "--repeats" => a.repeats = num(v()?)? as usize,
            "--stim-ms" => a.stim_ms = num(v()?)? as usize,
            "--gap-ms" => a.gap_ms = num(v()?)? as usize,
            "--memory-ms" => a.memory_ms = num(v()?)? as usize,
            "--k-rec" => a.k_rec = num(v()?)? as usize,
            "--k-in" => a.k_in = num(v()?)? as usize,
            "--w-exc" => a.w_exc = num(v()?)?,
            "--w-inh" => a.w_inh = num(v()?)?,
            "--w-in" => a.w_in = num(v()?)?,
            "--drive" => a.drive = num(v()?)?,
            "--background" => a.background = num(v()?)?,
            "--flip" => a.flip = num(v()?)?,
            "--module-size" => a.module_size = num(v()?)? as usize,
            "--attach" => a.attach = num(v()?)? as usize,
            "--branching" => a.branching = num(v()?)? as usize,
            "--within-p" => a.within_p = num(v()?)?,
            "--out" => a.out = v()?,
            "--base" => a.base = Some(v()?).filter(|p| p != "default"),
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if a.memory_ms > a.gap_ms {
        return Err("--memory-ms must not exceed --gap-ms".into());
    }
    Ok(a)
}

fn main() -> Result<(), String> {
    let a = parse()?;
    let n = a.neurons.max(2);
    let s = a.sensors.max(1);
    // Base physiology: a validated network's config (default: the C. elegans
    // depth-study network, which fires only when driven), so the cells are
    // the same AARNN cells as the embodied studies; sizes and wiring below.
    let mut net: NetworkConfig = match &a.base {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            serde_json::from_value(v["net"].clone()).map_err(|e| format!("{path} net: {e}"))?
        }
        None => NetworkConfig::default(),
    };
    net.neuron_types = Vec::new();
    // Build with no connections: this probe writes its own sparse wiring
    // below. With the worm config's p_hidden = 1 the constructor would build
    // (and route a morphology for) a dense N x N network first.
    net.p_in = 0.0;
    net.p_hidden = 0.0;
    net.p_out = 0.0;
    net.clumping_design = ClumpingDesign::None;
    net.brain_regions = Vec::new();
    net.growth_enabled = false;
    net.use_morphology = false;
    net.morpho_growth_enabled = false;
    net.use_aarnn_delays = false;
    net.num_hidden_layers = 1;
    net.num_hidden_per_layer_initial = n;
    net.num_sensory_neurons = s;
    net.num_output_neurons = 1;
    net.aarnn_layer_depth = 1;
    net.aarnn_plasticity_enabled = false;
    net.aarnn_synaptic_scaling_strength = 0.0;
    net.aarnn_dale_strictness = 0.0;
    // No ambient energy or resonance drive: cells fire only from their inputs.
    net.aarnn_ambient_energy_level = 0.0;
    net.aarnn_resonance_gain = 0.0;
    let mut runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        net,
        NeuronModel::Aarnn,
        Learning::Aarnn,
    );
    runner.set_dt(1.0);
    let h = runner.v_h.first().map(|v| v.len()).unwrap_or(0);
    if h != n {
        return Err(format!("built {h} hidden cells, wanted {n}"));
    }

    // Fixed in-degree wiring (seeded): the drive per cell does not change with N.
    let mut rng = fastrand::Rng::with_seed(a.seed.wrapping_mul(1_000_003) ^ n as u64);
    let inhibitory: Vec<bool> = (0..n).map(|_| rng.f64() < 0.2).collect();
    let k_rec = a.k_rec.min(n - 1);
    let mut rec = ndarray::Array2::<f64>::zeros((n, n));
    for j in 0..n {
        let mut chosen = std::collections::BTreeSet::new();
        while chosen.len() < k_rec {
            let i = rng.usize(0..n);
            if i != j {
                chosen.insert(i);
            }
        }
        for i in chosen {
            let jitter = 0.5 + rng.f64();
            rec[(j, i)] = if inhibitory[i] { -a.w_inh * jitter } else { a.w_exc * jitter };
        }
    }
    if a.module_size > 0 {
        // Modular ("groups of groups") wiring replaces the interwoven clump.
        // It draws from its own stream, so the input wiring, patterns and
        // noise below stay identical to the clump and disconnected runs of
        // the same seed (paired comparison).
        rec = modular_wiring(&a, n, &inhibitory);
    }
    let mut w_in = ndarray::Array2::<f64>::zeros((n, s));
    for j in 0..n {
        for _ in 0..a.k_in.min(s) {
            w_in[(j, rng.usize(0..s))] = a.w_in * (0.5 + rng.f64());
        }
    }
    runner.w_hh_rec[0] = rec;
    runner.w_in = w_in;
    runner.w_out.fill(0.0);

    // Patterns: 25 % of channels on.
    let on = (s / 4).max(1);
    let patterns: Vec<Vec<bool>> = (0..a.patterns)
        .map(|_| {
            let mut p = vec![false; s];
            let mut c = 0;
            while c < on {
                let i = rng.usize(0..s);
                if !p[i] {
                    p[i] = true;
                    c += 1;
                }
            }
            p
        })
        .collect();
    let mut order: Vec<usize> = (0..a.patterns).flat_map(|p| std::iter::repeat_n(p, a.repeats)).collect();
    rng.shuffle(&mut order);

    let step = |runner: &mut Runner, values: &[f64], phase: &mut [f64], rng: &mut fastrand::Rng| {
        let mut frame = vec![0i8; s];
        for i in 0..s {
            phase[i] += values[i];
            if phase[i] >= 1.0 {
                phase[i] -= 1.0;
                frame[i] = 1;
            } else if rng.f64() < a.background {
                frame[i] = 1;
            }
        }
        let _ = runner.step_sync(1.0, Some(&frame));
        runner.last_spk_h[0].iter().map(|x| (*x != 0) as u32).collect::<Vec<u32>>()
    };

    let started = std::time::Instant::now();
    let mut phase = vec![0.0f64; s];
    let silent = vec![0.0f64; s];
    let mut total_spikes = 0u64;
    for _ in 0..500 {
        total_spikes += step(&mut runner, &silent, &mut phase, &mut rng).iter().sum::<u32>() as u64;
    }
    let mut records = Vec::with_capacity(order.len());
    let mut stim_spikes = 0u64;
    for &p in &order {
        let values: Vec<f64> = patterns[p]
            .iter()
            .map(|&o| if o ^ (rng.f64() < a.flip) { a.drive } else { 0.0 })
            .collect();
        for x in phase.iter_mut() {
            *x = rng.f64();
        }
        let mut evoked = vec![0u32; n];
        let mut memory = vec![0u32; n];
        for _ in 0..a.stim_ms {
            for (c, x) in evoked.iter_mut().zip(step(&mut runner, &values, &mut phase, &mut rng)) {
                *c += x;
            }
        }
        for t in 0..a.gap_ms {
            let spk = step(&mut runner, &silent, &mut phase, &mut rng);
            if t < a.memory_ms {
                for (c, x) in memory.iter_mut().zip(&spk) {
                    *c += x;
                }
            }
        }
        stim_spikes += evoked.iter().map(|x| *x as u64).sum::<u64>();
        total_spikes += evoked.iter().chain(&memory).map(|x| *x as u64).sum::<u64>();
        records.push(serde_json::json!({"pattern": p, "evoked": evoked, "memory": memory}));
    }
    let wall = started.elapsed().as_secs_f64();
    let stim_hz = stim_spikes as f64 / (n as f64 * (order.len() * a.stim_ms) as f64 / 1000.0);
    let out = serde_json::json!({
        "schema": "aarnn-scaling-probe-v1",
        "neurons": n, "seed": a.seed, "sensors": s, "patterns": a.patterns, "repeats": a.repeats,
        "stim_ms": a.stim_ms, "gap_ms": a.gap_ms, "memory_ms": a.memory_ms,
        "k_rec": k_rec, "k_in": a.k_in, "w_exc": a.w_exc, "w_inh": a.w_inh, "w_in": a.w_in,
        "drive": a.drive, "background": a.background, "flip": a.flip, "base": a.base,
        "module_size": a.module_size, "attach": a.attach, "branching": a.branching, "within_p": a.within_p,
        "recurrent_synapses": runner.w_hh_rec[0].iter().filter(|w| **w != 0.0).count(),
        "inhibitory_fraction": inhibitory.iter().filter(|x| **x).count() as f64 / n as f64,
        "stim_rate_hz": stim_hz, "total_spikes": total_spikes, "wall_s": wall,
        "records": records,
    });
    std::fs::write(&a.out, out.to_string()).map_err(|e| e.to_string())?;
    println!("N={n} seed={} stim rate {stim_hz:.1} Hz wall {wall:.1} s -> {}", a.seed, a.out);
    Ok(())
}


/// Recursive modular wiring: groups of `module_size` cells wired inside
/// (all-to-all, or each pair with probability `within_p`) (Dale: a cell's outgoing synapses share its sign); every pair of
/// groups in the same collection of `branching` groups is joined by a
/// "simple attachment" of `attach` excitatory synapses each way; every pair
/// of collections is joined the same way through their first groups.
fn modular_wiring(a: &Args, n: usize, inhibitory: &[bool]) -> ndarray::Array2<f64> {
    let g = a.module_size.max(2);
    let b = if a.branching == 0 { g } else { a.branching.max(2) };
    let mut r = fastrand::Rng::with_seed(a.seed.wrapping_mul(7_777_777) ^ (n as u64) << 8 ^ g as u64);
    let mut rec = ndarray::Array2::<f64>::zeros((n, n));
    let module = |c: usize| c / g;
    for j in 0..n {
        for i in 0..n {
            if i != j && module(i) == module(j) {
                // Sparse groups: each within-group pair is a synapse with
                // probability `within_p` (no draw at 1, so full groups keep
                // their exact wiring).
                if a.within_p < 1.0 && r.f64() >= a.within_p {
                    continue;
                }
                let jitter = 0.5 + r.f64();
                rec[(j, i)] = if inhibitory[i] { -a.w_inh * jitter } else { a.w_exc * jitter };
            }
        }
    }
    let n_mod = n.div_ceil(g);
    let cells = |m: usize| (m * g)..((m + 1) * g).min(n);
    let mut link = |rec: &mut ndarray::Array2<f64>, ma: usize, mb: usize| {
        let src: Vec<usize> = cells(ma).filter(|c| !inhibitory[*c]).collect();
        let dst: Vec<usize> = cells(mb).collect();
        if src.is_empty() || dst.is_empty() {
            return;
        }
        for _ in 0..a.attach {
            let i = src[r.usize(0..src.len())];
            let j = dst[r.usize(0..dst.len())];
            rec[(j, i)] = a.w_exc * (0.5 + r.f64());
        }
    };
    for ma in 0..n_mod {
        for mb in 0..n_mod {
            if ma != mb && ma / b == mb / b {
                link(&mut rec, ma, mb);
            }
        }
    }
    // Recursive levels: units of `unit` groups (collections, then
    // collections of collections, ...) attached pairwise within their parent
    // of `b` units, through the first group of each unit.
    let mut unit = b;
    loop {
        let n_units = n_mod.div_ceil(unit);
        if n_units <= 1 {
            break;
        }
        for ua in 0..n_units {
            for ub in 0..n_units {
                if ua != ub && ua / b == ub / b {
                    link(&mut rec, ua * unit, ub * unit);
                }
            }
        }
        unit *= b;
    }
    rec
}
