// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Development probe (ignored).
use aarnn_explorer::author::{DemoWeights, demo_circuit};
use aarnn_explorer::tasks::{TaskSuite, run_trial};

#[test]
#[ignore]
fn trials() {
    let (t, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
    let s = TaskSuite::parse(include_str!(
        "../../../examples/explorer/tasks/synthetic_sensorimotor.toml"
    ))
    .unwrap();
    for id in ["memory-miss", "nogo"] {
        let task = s.tasks.iter().find(|x| x.id == id).unwrap();
        for seed in 1..=6u64 {
            let (tr, out) = run_trial(&snap, task, seed, "frozen", None, true, false).unwrap();
            let names: Vec<String> = out
                .events
                .iter()
                .filter(|e| e.tick > 60)
                .take(40)
                .map(|e| format!("{}@{}", t.neurons[e.neuron as usize].id, e.tick))
                .collect();
            println!(
                "{id} seed {seed} ok={} {:?}: {}",
                tr.success,
                tr.measures,
                names.join(" ")
            );
        }
    }
}
