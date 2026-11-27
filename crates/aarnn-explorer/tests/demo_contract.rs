// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! EXPLORER-TASK / EXPLORER-LESION on the authored circuit, through the real
//! AARNN runner.
use aarnn_explorer::author::{DemoWeights, demo_circuit};
use aarnn_explorer::lesion::{Action, Intervention, Protection, Selection, apply};
use aarnn_explorer::tasks::{TaskSuite, evaluate, reference_measures};

fn suite() -> TaskSuite {
    TaskSuite::parse(include_str!(
        "../../../examples/explorer/tasks/synthetic_sensorimotor.toml"
    ))
    .unwrap()
}

fn verdict_with(ablate: &[&str]) -> aarnn_explorer::tasks::SuiteVerdict {
    let (t, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
    let s = suite();
    let reference = evaluate(&snap, &s, "development", "frozen", None, &[]).unwrap();
    let rm = reference_measures(&reference);
    let snap = if ablate.is_empty() {
        snap
    } else {
        let iv = Intervention {
            id: "t".into(),
            selection: Selection::Neurons {
                ids: ablate.iter().map(|x| x.to_string()).collect(),
            },
            action: Action::NeuronAblation,
        };
        apply(
            &t,
            &snap,
            &[iv],
            &Protection {
                protect_io: true,
                ..Default::default()
            },
        )
        .unwrap()
        .0
    };
    evaluate(&snap, &s, "development", "frozen", Some(&rm), &[]).unwrap()
}

fn failed(v: &aarnn_explorer::tasks::SuiteVerdict) -> Vec<String> {
    v.tasks
        .iter()
        .filter(|t| !t.passed)
        .map(|t| t.task.clone())
        .collect()
}

#[test]
fn intact_circuit_passes_every_task() {
    let v = verdict_with(&[]);
    assert!(
        v.passed,
        "{:#?}",
        v.tasks.iter().filter(|t| !t.passed).collect::<Vec<_>>()
    );
    assert!(!v.quiescent);
}

#[test]
fn single_redundant_pathway_removal_keeps_function_but_both_fail() {
    assert!(verdict_with(&["G1a"]).passed);
    assert!(verdict_with(&["G2b"]).passed);
    assert_eq!(failed(&verdict_with(&["G1a", "G1b"])), vec!["go"]);
}

#[test]
fn inhibition_is_essential_and_memory_loop_is_required() {
    assert_eq!(failed(&verdict_with(&["I"])), vec!["nogo"]);
    assert_eq!(failed(&verdict_with(&["M2"])), vec!["memory-hit"]);
    assert!(verdict_with(&["TR"]).passed, "TR2 carries the right turn");
    assert_eq!(failed(&verdict_with(&["TR", "TR2"])), vec!["steer"]);
}

#[test]
fn removing_everything_downstream_is_rejected_as_quiescent() {
    let all = [
        "G1a", "G2a", "G1b", "G2b", "I", "M1", "M2", "M3", "Q", "TL", "TL2", "TR", "TR2", "D1",
        "D2",
    ];
    let v = verdict_with(&all);
    assert!(
        !v.passed && v.quiescent,
        "an empty circuit must not pass through the quiet task"
    );
}
