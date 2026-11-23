// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Storage probe: how many experiences can a recurrent AARNN network of `N`
//! cells store and recall, as `N` grows one neuron at a time?
//!
//! An experience is a sparse assembly of `--assembly` (M) co-active cells. It
//! is stored the way co-activity is stored in synapses (Hebb): every pair of
//! cells that took part in the same experience gets an excitatory synapse
//! of strength `--w-assoc` (binary, "clipped" Hebbian storage; Willshaw,
//! Buneman & Longuet-Higgins 1969). Every cell also inhibits every other by
//! `--w-inhib` (shared lateral inhibition, folded into the weights).
//!
//! Recall: half of an assembly (`--cue-frac`) is driven through its input
//! channels for `--cue-ms`; an experience counts as recalled when at least
//! `--min-recall` of its uncued cells fire in the read window and fewer
//! spurious cells fire than half the uncued ones. The load `P` (number of
//! stored experiences) is raised until fewer than `--criterion` of the
//! tested experiences are recalled; the capacity is the largest load that
//! still met the criterion.
//!
//! Disconnected cells (`--w-assoc 0`) cannot complete anything: their
//! capacity is 0 at every `N`, so whatever the connected network stores is
//! held by the connections between cells - the whole, not the parts.
//!
//!   aarnn_storage_probe --neurons 40 --seed 1 --out run.json

use aarnn_rust::config::{ClumpingDesign, LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::runner::Runner;
use aarnn_rust::sim::{Learning, NeuronModel};

struct Args {
    neurons: usize,
    seed: u64,
    assembly: usize,
    cue_frac: f64,
    w_assoc: f64,
    w_inhib: f64,
    w_cue: f64,
    drive: f64,
    cue_ms: usize,
    read_from_ms: usize,
    rest_ms: usize,
    test_patterns: usize,
    max_patterns: usize,
    min_recall: f64,
    min_spikes: u32,
    release_p: f64,
    best_match: bool,
    reset_ms: usize,
    reset_gain: f64,
    interneurons: f64,
    read_to_ms: usize,
    pool_ref: usize,
    module_size: usize,
    within_p: f64,
    w_ei: f64,
    w_ie: f64,
    criterion: f64,
    base: Option<String>,
    out: String,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        neurons: 40,
        seed: 1,
        assembly: 6,
        cue_frac: 0.5,
        w_assoc: 3.0,
        w_inhib: 0.3,
        w_cue: 20.0,
        drive: 0.5,
        cue_ms: 60,
        read_from_ms: 10,
        rest_ms: 80,
        test_patterns: 20,
        max_patterns: 4000,
        min_recall: 0.75,
        min_spikes: 2,
        release_p: -1.0,
        best_match: true,
        reset_ms: 100,
        reset_gain: 3.0,
        interneurons: 0.0,
        read_to_ms: 0,
        pool_ref: 0,
        module_size: 0,
        within_p: 1.0,
        w_ei: 2.0,
        w_ie: 4.0,
        criterion: 0.9,
        base: Some("target/qa/celegans-depth-study-forward-crawl/network_grafted.json".into()),
        out: "storage.json".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        let num = |s: String| s.parse::<f64>().map_err(|e| e.to_string());
        match k.as_str() {
            "--neurons" => a.neurons = num(v()?)? as usize,
            "--seed" => a.seed = num(v()?)? as u64,
            "--assembly" => a.assembly = num(v()?)? as usize,
            "--cue-frac" => a.cue_frac = num(v()?)?,
            "--w-assoc" => a.w_assoc = num(v()?)?,
            "--w-inhib" => a.w_inhib = num(v()?)?,
            "--w-cue" => a.w_cue = num(v()?)?,
            "--drive" => a.drive = num(v()?)?,
            "--cue-ms" => a.cue_ms = num(v()?)? as usize,
            "--read-from-ms" => a.read_from_ms = num(v()?)? as usize,
            "--rest-ms" => a.rest_ms = num(v()?)? as usize,
            "--test-patterns" => a.test_patterns = num(v()?)? as usize,
            "--max-patterns" => a.max_patterns = num(v()?)? as usize,
            "--min-recall" => a.min_recall = num(v()?)?,
            "--min-spikes" => a.min_spikes = num(v()?)? as u32,
            "--release-p" => a.release_p = num(v()?)?,
            "--strict" => a.best_match = false,
            "--reset-ms" => a.reset_ms = num(v()?)? as usize,
            "--reset-gain" => a.reset_gain = num(v()?)?,
            "--interneurons" => a.interneurons = num(v()?)?,
            "--read-to-ms" => a.read_to_ms = num(v()?)? as usize,
            "--pool-ref" => a.pool_ref = num(v()?)? as usize,
            "--module-size" => a.module_size = num(v()?)? as usize,
            "--within-p" => a.within_p = num(v()?)?,
            "--w-ei" => a.w_ei = num(v()?)?,
            "--w-ie" => a.w_ie = num(v()?)?,
            "--criterion" => a.criterion = num(v()?)?,
            "--base" => a.base = Some(v()?).filter(|p| p != "default"),
            "--out" => a.out = v()?,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    Ok(a)
}

fn build_runner(a: &Args, n: usize, n_i: usize) -> Result<Runner, String> {
    let h = n + n_i;
    let mut net: NetworkConfig = match &a.base {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            serde_json::from_value(v["net"].clone()).map_err(|e| format!("{path} net: {e}"))?
        }
        None => NetworkConfig::default(),
    };
    net.neuron_types = Vec::new();
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
    net.num_hidden_per_layer_initial = h;
    net.num_sensory_neurons = n + 1;
    net.num_output_neurons = 1;
    net.aarnn_layer_depth = 1;
    net.aarnn_plasticity_enabled = false;
    net.aarnn_synaptic_scaling_strength = 0.0;
    net.aarnn_dale_strictness = 0.0;
    net.aarnn_ambient_energy_level = 0.0;
    net.aarnn_resonance_gain = 0.0;
    if a.release_p >= 0.0 {
        net.p_release_default = a.release_p as f32;
    }
    let mut runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        net,
        NeuronModel::Aarnn,
        Learning::Aarnn,
    );
    runner.set_dt(1.0);
    if runner.v_h.first().map(|v| v.len()).unwrap_or(0) != h {
        return Err("hidden size mismatch".into());
    }
    // Channel j cues excitatory cell j; channel n is the reset input: it
    // drives the interneuron pool (which silences the excitatory cells) or,
    // without a pool, inhibits the excitatory cells directly.
    let mut w_in = ndarray::Array2::<f64>::zeros((h, n + 1));
    for j in 0..n {
        w_in[(j, j)] = a.w_cue;
        if n_i == 0 {
            w_in[(j, n)] = -a.w_cue * a.reset_gain;
        }
    }
    for k in n..h {
        w_in[(k, n)] = a.w_cue * a.reset_gain;
    }
    runner.w_in = w_in;
    runner.w_out.fill(0.0);
    Ok(runner)
}

/// Store `patterns` as clipped Hebbian synapses between excitatory cells.
/// With an interneuron pool (`n_i` > 0, cells n..n+n_i) every excitatory
/// cell excites every interneuron (`w_ei`) and every interneuron inhibits
/// every excitatory cell (`w_ie`): feedback inhibition that grows with the
/// number of active cells (Dale's law holds). Without a pool, lateral
/// inhibition `w_inhib` is folded into the excitatory weights.
#[allow(clippy::too_many_arguments)]
fn store(
    runner: &mut Runner,
    n: usize,
    n_i: usize,
    patterns: &[Vec<usize>],
    w_assoc: f64,
    w_inhib: f64,
    w_ei: f64,
    w_ie: f64,
    allowed: &dyn Fn(usize, usize) -> bool,
) {
    let h = n + n_i;
    let mut assoc = vec![false; n * n];
    for p in patterns {
        for &i in p {
            for &j in p {
                if i != j && allowed(i, j) {
                    assoc[j * n + i] = true;
                }
            }
        }
    }
    let mut rec = ndarray::Array2::<f64>::zeros((h, h));
    let fold = if n_i == 0 { w_inhib } else { 0.0 };
    for j in 0..n {
        for i in 0..n {
            if i != j {
                rec[(j, i)] = if assoc[j * n + i] { w_assoc } else { 0.0 } - fold;
            }
        }
    }
    for k in n..h {
        for i in 0..n {
            rec[(k, i)] = w_ei;
            rec[(i, k)] = -w_ie;
        }
    }
    runner.w_hh_rec[0] = rec;
}

fn step(runner: &mut Runner, drive: &[f64], phase: &mut [f64]) -> Vec<u8> {
    let mut frame = vec![0i8; drive.len()];
    for i in 0..drive.len() {
        phase[i] += drive[i];
        if phase[i] >= 1.0 {
            phase[i] -= 1.0;
            frame[i] = 1;
        }
    }
    let _ = runner.step_sync(1.0, Some(&frame));
    runner.last_spk_h[0]
        .iter()
        .map(|x| (*x != 0) as u8)
        .collect()
}

fn main() -> Result<(), String> {
    let a = parse()?;
    let n = a.neurons;
    let m = a.assembly.min(n);
    let cue = ((m as f64 * a.cue_frac).round() as usize).clamp(1, m.saturating_sub(1).max(1));
    let mut rng = fastrand::Rng::with_seed(a.seed.wrapping_mul(7_919) ^ (n as u64) << 20);
    let n_i = if a.interneurons > 0.0 {
        ((n as f64 * a.interneurons / (1.0 - a.interneurons)).round() as usize).max(1)
    } else {
        0
    };
    let mut runner = build_runner(&a, n, n_i)?;
    // Balanced pool: the pool's total gain stays constant as the network
    // grows (weights scale with 1/N, as in balanced networks): each
    // interneuron sums over n excitatory cells and each excitatory cell over
    // n_i interneurons, so the per-synapse weights are referred to the
    // calibration size `--pool-ref` (excitatory cells; n_i = ref/4).
    // Anatomically possible synapses: any pair (open network), or, with
    // `--module-size`, pairs inside the same group, each present with
    // probability `--within-p` (fixed per pair and seed). Learned weights
    // are scaled by 1/within_p so a sparse group's recall drive matches a
    // full group's (synaptic scaling).
    let seed = a.seed;
    let g = a.module_size;
    let wp = a.within_p.clamp(0.0, 1.0);
    let allowed = move |i: usize, j: usize| -> bool {
        if g == 0 {
            return true;
        }
        if i / g != j / g {
            return false;
        }
        if wp >= 1.0 {
            return true;
        }
        let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        (x as f64 / u64::MAX as f64) < wp
    };
    let w_assoc = if g > 0 && wp > 0.0 {
        a.w_assoc / wp
    } else {
        a.w_assoc
    };
    let (w_ei, w_ie) = if a.pool_ref > 0 && n_i > 0 {
        let ref_i = (a.pool_ref as f64 * a.interneurons / (1.0 - a.interneurons))
            .round()
            .max(1.0);
        (
            a.w_ei * a.pool_ref as f64 / n as f64,
            a.w_ie * ref_i / n_i as f64,
        )
    } else {
        (a.w_ei, a.w_ie)
    };
    let started = std::time::Instant::now();
    let mut loads = Vec::new();
    let mut capacity = 0usize;
    let mut p = 1usize;
    let mut patterns: Vec<Vec<usize>> = Vec::new();
    let silent = vec![0.0f64; n + 1];
    let mut phase = vec![0.0f64; n + 1];
    let mut failures_in_row = 0;
    while p <= a.max_patterns && m >= 2 && n >= 2 {
        // Grow the set of stored experiences to p (earlier ones are kept).
        while patterns.len() < p {
            let mut set = Vec::with_capacity(m);
            // Modular: an experience forms inside one (full) group.
            let (lo, hi) = if a.module_size >= m {
                let full = (n / a.module_size).max(1);
                let g = rng.usize(0..full);
                (g * a.module_size, ((g + 1) * a.module_size).min(n))
            } else {
                (0, n)
            };
            while set.len() < m {
                let c = rng.usize(lo..hi);
                if !set.contains(&c) {
                    set.push(c);
                }
            }
            set.sort_unstable();
            patterns.push(set);
        }
        store(
            &mut runner,
            n,
            n_i,
            &patterns,
            w_assoc,
            a.w_inhib,
            w_ei,
            w_ie,
            &allowed,
        );
        // Test a random sample of the stored experiences.
        let mut idx: Vec<usize> = (0..patterns.len()).collect();
        rng.shuffle(&mut idx);
        idx.truncate(a.test_patterns.max(1));
        let mut recalled = 0usize;
        let mut recall_sum = 0.0;
        let mut spurious_sum = 0.0;
        for &k in &idx {
            let pat = &patterns[k];
            let mut order = pat.clone();
            rng.shuffle(&mut order);
            let cued: Vec<usize> = order[..cue].to_vec();
            let uncued: Vec<usize> = order[cue..].to_vec();
            // Each recall starts from rest: a global inhibitory pulse (channel
            // n, onto every cell) silences any attractor still active from the
            // previous test, then the network rests.
            let mut reset_drive = vec![0.0f64; n + 1];
            reset_drive[n] = a.drive;
            for _ in 0..a.reset_ms {
                step(&mut runner, &reset_drive, &mut phase);
            }
            for _ in 0..a.rest_ms {
                step(&mut runner, &silent, &mut phase);
            }
            let mut drive = vec![0.0f64; n + 1];
            for &c in &cued {
                drive[c] = a.drive;
            }
            for x in phase.iter_mut() {
                *x = rng.f64();
            }
            let mut fired = vec![0u32; n + n_i];
            // Cue for `cue_ms`, then keep reading (input off) until
            // `read_to_ms` (0 = end of the cue).
            let read_to = if a.read_to_ms > 0 {
                a.read_to_ms
            } else {
                a.cue_ms
            };
            for t in 0..read_to.max(a.cue_ms) {
                let d = if t < a.cue_ms { &drive } else { &silent };
                let spk = step(&mut runner, d, &mut phase);
                if t >= a.read_from_ms && t < read_to {
                    for (f, s) in fired.iter_mut().zip(spk) {
                        *f += s as u32;
                    }
                }
            }
            // A cell takes part when it fires at least `min_spikes` in the read
            // window (cells fire ~1 Hz spontaneously: a single spike is noise).
            let on = |c: usize| fired[c] >= a.min_spikes;
            if std::env::var("STORAGE_DEBUG").is_ok() {
                let cf: Vec<u32> = cued.iter().map(|c| fired[*c]).collect();
                let uf: Vec<u32> = uncued.iter().map(|c| fired[*c]).collect();
                let other = (0..n)
                    .filter(|c| !pat.contains(c))
                    .map(|c| fired[c])
                    .max()
                    .unwrap_or(0);
                let other_pat: Vec<(usize, f64)> = patterns
                    .iter()
                    .enumerate()
                    .filter(|(qi, _)| *qi != k)
                    .map(|(qi, q)| {
                        (
                            qi,
                            q.iter().map(|c| fired[*c] as f64).sum::<f64>() / q.len() as f64,
                        )
                    })
                    .collect();
                let tot: u32 = fired.iter().sum();
                let spur: Vec<(usize, u32)> = (0..n)
                    .filter(|c| !pat.contains(c) && fired[*c] >= a.min_spikes)
                    .map(|c| (c, fired[c]))
                    .collect();
                eprintln!(
                    "P={} test={} cued_ids {:?} cued {:?} uncued {:?} spurious {:?} patterns {:?}",
                    patterns.len(),
                    k,
                    cued,
                    cf,
                    uf,
                    spur,
                    patterns
                );
            }
            let hit = uncued.iter().filter(|c| on(**c)).count();
            let spurious = (0..n).filter(|c| !pat.contains(c) && on(*c)).count();
            let rec = if uncued.is_empty() {
                1.0
            } else {
                hit as f64 / uncued.len() as f64
            };
            recall_sum += rec;
            spurious_sum += spurious as f64;
            // Best-match retrieval: score every stored experience by the mean
            // spike count of its cells outside the cue; recalled when the
            // cued experience's uncued cells fire most (ties fail) and at least
            // one spike each on average.
            let score = |q: &Vec<usize>| -> f64 {
                let cells: Vec<usize> = q.iter().copied().filter(|c| !cued.contains(c)).collect();
                if cells.is_empty() {
                    return 0.0;
                }
                cells.iter().map(|c| fired[*c] as f64).sum::<f64>() / cells.len() as f64
            };
            let own = score(pat);
            let best_other = patterns
                .iter()
                .enumerate()
                .filter(|(qi, _)| *qi != k)
                .map(|(_, q)| score(q))
                .fold(0.0f64, f64::max);
            let ok = if a.best_match {
                own >= 1.0 && own > best_other
            } else {
                rec >= a.min_recall && (spurious as f64) < (uncued.len() as f64 / 2.0).max(1.0)
            };
            if ok {
                recalled += 1;
            }
        }
        let frac = recalled as f64 / idx.len() as f64;
        loads.push(serde_json::json!({
            "patterns": p, "tested": idx.len(), "recalled_fraction": frac,
            "mean_recall": recall_sum / idx.len() as f64,
            "mean_spurious": spurious_sum / idx.len() as f64,
        }));
        if frac >= a.criterion {
            capacity = p;
            failures_in_row = 0;
        } else {
            failures_in_row += 1;
            if failures_in_row >= 2 {
                break;
            }
        }
        p = if p < 20 {
            p + 1
        } else {
            ((p as f64) * 1.1).ceil() as usize
        };
    }
    // Information held at capacity: each stored experience picks M of N cells.
    let log2_choose = |n: usize, k: usize| -> f64 {
        (0..k)
            .map(|i| ((n - i) as f64).log2() - ((i + 1) as f64).log2())
            .sum()
    };
    let bits = capacity as f64 * log2_choose(n, m);
    let wall = started.elapsed().as_secs_f64();
    let out = serde_json::json!({
        "schema": "aarnn-storage-probe-v1",
        "neurons": n, "seed": a.seed, "assembly": m, "cued": cue,
        "w_assoc": a.w_assoc, "w_inhib": a.w_inhib, "w_cue": a.w_cue, "drive": a.drive,
        "cue_ms": a.cue_ms, "read_from_ms": a.read_from_ms, "rest_ms": a.rest_ms,
        "min_recall": a.min_recall, "min_spikes": a.min_spikes, "release_p": a.release_p, "best_match": a.best_match, "reset_ms": a.reset_ms, "reset_gain": a.reset_gain,
        "interneurons": n_i, "read_to_ms": a.read_to_ms, "pool_ref": a.pool_ref, "module_size": a.module_size, "within_p": a.within_p, "w_ei_eff": w_ei, "w_ie_eff": w_ie, "w_ei": a.w_ei, "w_ie": a.w_ie, "total_cells": n + n_i, "criterion": a.criterion, "base": a.base,
        "capacity": capacity, "bits_at_capacity": bits, "bits_per_neuron": bits / n as f64,
        "possible_synapses": n * (n - 1), "loads": loads, "wall_s": wall,
    });
    std::fs::write(&a.out, out.to_string()).map_err(|e| e.to_string())?;
    println!(
        "N={n} M={m} capacity {capacity} experiences, {bits:.0} bits ({:.2}/neuron), wall {wall:.1}s",
        bits / n as f64
    );
    Ok(())
}
