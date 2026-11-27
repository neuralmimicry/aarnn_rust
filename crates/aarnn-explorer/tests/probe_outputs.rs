// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Development probe (ignored): open-loop output rhythm of a network.
//! `PROBE_NET=path PROBE_SET=needs_energy=0.9 cargo test --release -p aarnn-explorer --test probe_outputs -- --ignored --nocapture`
use aarnn_explorer::exec::{RunSpec, run};

#[test]
#[ignore]
fn output_rhythm() {
    let path = std::env::var("PROBE_NET").expect("PROBE_NET");
    let snap: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let l = &snap["connectome_labels"];
    let names = |k: &str| {
        l[k].as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let sens = names("physical_sensory_nodes");
    let outs = names("physical_output_nodes");
    let mut v = vec![0.0f32; sens.len()];
    for kv in std::env::var("PROBE_SET")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let (k, x) = kv.split_once('=').unwrap();
        v[sens.iter().position(|s| s == k).unwrap()] = x.parse().unwrap();
    }
    let frames = 1500; // 3 s
    let out = run(
        &snap,
        &RunSpec {
            seed: 1,
            frames: vec![v; frames],
            background_p: 0.0,
            noise_seed: 0,
            record_events: false,
            graded_events: false,
            plasticity_policy: "frozen".into(),
            silence: vec![],
        },
    )
    .unwrap();
    for (k, o) in outs.iter().enumerate() {
        let xs: Vec<f32> = out.output_series[250..].iter().map(|r| r[k]).collect();
        let m = xs.iter().sum::<f32>() / xs.len() as f32;
        let ups = xs.windows(2).filter(|w| w[0] < m && w[1] >= m).count();
        let (lo, hi) = xs
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        // First upward mean-crossing after 0.5 s, in ms (phase reference).
        let first = xs
            .windows(2)
            .position(|w| w[0] < m && w[1] >= m)
            .map(|i| (i + 250) * 2);
        println!(
            "{o:40} mean {m:.3} range {lo:.3}-{hi:.3} cycles/2.5s {ups:3} first_up_ms {first:?}"
        );
    }
}
