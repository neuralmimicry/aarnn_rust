// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Development probe (ignored): memory loop, coincidence and inhibition.
use aarnn_explorer::author::{DemoWeights, demo_circuit_with};
use aarnn_explorer::exec::{RunSpec, run};

fn go(
    w: DemoWeights,
    lp: &str,
    ip: &str,
    pulses: &[(usize, usize, usize)],
    frames: usize,
) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let (t, snap) = demo_circuit_with(w, lp, ip).compile().unwrap();
    let mut f = vec![vec![0.0f32; 6]; frames];
    for &(ch, a, b) in pulses {
        for row in f.iter_mut().take(b).skip(a) {
            row[ch] = 1.0;
        }
    }
    let out = run(
        &snap,
        &RunSpec {
            seed: 3,
            frames: f,
            background_p: 0.0,
            noise_seed: 0,
            record_events: true,
            graded_events: false,
            plasticity_policy: "frozen".into(),
            silence: vec![],
        },
    )
    .unwrap();
    let id = |n: &str| t.index_of(n).unwrap();
    let count = |n: &str| out.events.iter().filter(|e| e.neuron == id(n)).count() as u32;
    let ticks = |n: &str| {
        out.events
            .iter()
            .filter(|e| e.neuron == id(n))
            .map(|e| e.tick as u32)
            .collect::<Vec<_>>()
    };
    let names = ["M1", "M3", "Q", "O_mem", "O_go", "I", "G2a"];
    (
        names.iter().map(|n| count(n)).collect(),
        ticks("O_go"),
        ticks("M3").into_iter().rev().take(3).collect(),
    )
}

#[test]
#[ignore]
fn demo_sweep() {
    for (mq, pq) in [
        (1.0, 2.5),
        (1.5, 2.5),
        (2.0, 2.0),
        (2.0, 2.5),
        (1.0, 3.0),
        (2.5, 2.0),
        (3.0, 1.5),
    ] {
        let w = DemoWeights {
            mem_to_q: mq,
            probe_to_q: pq,
            inhibit: -16.0,
            ..Default::default()
        };
        let mem = go(w, "FS", "FS", &[(2, 10, 11)], 300).0[2];
        let mut hits = Vec::new();
        let mut probe_only = 0;
        for on in 100..106 {
            hits.push(go(w, "FS", "FS", &[(2, 10, 11), (3, on, on + 1)], 140).0[3]);
            probe_only += go(w, "FS", "FS", &[(3, on, on + 1)], 140).0[3];
        }
        println!(
            "mq {mq} pq {pq}: mem-only Q spikes {mem}; probe-only O_mem {probe_only}; both O_mem per phase {hits:?}"
        );
    }
}
