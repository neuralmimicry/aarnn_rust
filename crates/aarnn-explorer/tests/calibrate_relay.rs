// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Development probe (ignored): relay weights for exact spiking cells.
use aarnn_explorer::author::{Circuit, exact_spiking};
use aarnn_explorer::exec::{RunSpec, run};

#[test]
#[ignore]
fn relay_weight_sweep() {
    for w in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
        let mut c = Circuit::new("relay");
        c.ty("E", exact_spiking())
            .sensory("S")
            .hidden("H1", "E", "core")
            .hidden("H2", "E", "core")
            .output("O", "E");
        c.syn("S", "H1", w).syn("H1", "H2", w).syn("H2", "O", w);
        let (_, snap) = c.compile().unwrap();
        // One sensory spike at frame 5, then silence.
        let mut frames = vec![vec![0.0f32]; 40];
        frames[5][0] = 1.0;
        let out = run(
            &snap,
            &RunSpec {
                seed: 1,
                frames,
                background_p: 0.0,
                noise_seed: 0,
                record_events: true,
                graded_events: false,
                plasticity_policy: "frozen".into(),
                silence: vec![],
            },
        )
        .unwrap();
        let ev: Vec<String> = out
            .events
            .iter()
            .map(|e| format!("{}@{}.{}", e.neuron, e.tick, e.micro))
            .collect();
        println!("w {w}: {ev:?}");
    }
}
