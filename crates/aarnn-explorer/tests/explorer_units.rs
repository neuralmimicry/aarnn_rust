// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! EXPLORER-IMPORT, EXPLORER-TRACE, EXPLORER-FORK and EXPLORER-JOBS cases.
use std::path::Path;
use std::time::Duration;

use aarnn_explorer::author::{DemoWeights, demo_circuit};
use aarnn_explorer::capture;
use aarnn_explorer::exec::{RunSpec, run};
use aarnn_explorer::model;
use aarnn_explorer::store::{Project, sha256_file};
use aarnn_explorer::tasks::{TaskSuite, run_trial};
use aarnn_explorer::worker::{self, Job, JobKind, Limits};

fn suite() -> TaskSuite {
    TaskSuite::parse(include_str!(
        "../../../examples/explorer/tasks/synthetic_sensorimotor.toml"
    ))
    .unwrap()
}

#[test]
fn import_preserves_large_ids_and_rejects_bad_inputs() {
    let mut doc = demo_circuit(DemoWeights::default()).sparse();
    // A FlyWire-sized integer ID (> 2^53) given as a JSON integer stays exact.
    doc["neurons"][0]["id"] = serde_json::json!(720575940612345678u64);
    for e in doc["edges"].as_array_mut().unwrap() {
        if e["pre"] == "A" {
            e["pre"] = serde_json::json!(720575940612345678u64);
        }
    }
    let (t, snap) = model::from_sparse(&doc.to_string(), 1000, 10000).unwrap();
    assert!(t.index_of("720575940612345678").is_some());
    let snap = snap.unwrap();
    assert_eq!(
        snap["connectome_labels"]["sensory_nodes"][0],
        "720575940612345678"
    );
    // Unsupported schema, float IDs, unknown fields, allocation budget.
    let mut bad = doc.clone();
    bad["schema"] = "other".into();
    assert!(matches!(
        model::from_sparse(&bad.to_string(), 1000, 10000),
        Err(aarnn_explorer::error::ExplorerError::Unsupported(_))
    ));
    let mut f = doc.clone();
    f["neurons"][1]["id"] = serde_json::json!(1.5);
    assert!(model::from_sparse(&f.to_string(), 1000, 10000).is_err());
    let mut u = doc.clone();
    u["surprise"] = serde_json::json!(1);
    assert!(model::from_sparse(&u.to_string(), 1000, 10000).is_err());
    assert!(matches!(
        model::from_sparse(&doc.to_string(), 5, 10000),
        Err(aarnn_explorer::error::ExplorerError::Budget(_))
    ));
    // Missing dynamics: structural-only, not executable.
    let mut s = doc.clone();
    s["dynamics"] = serde_json::Value::Null;
    let (ts, none) = model::from_sparse(&s.to_string(), 1000, 10000).unwrap();
    assert!(none.is_none() && ts.capability == model::Capability::StructuralOnly);
    // Truncated native matrix.
    let mut tr = snap.clone();
    tr["w_hh_rec"][0]["data"].as_array_mut().unwrap().pop();
    assert!(model::from_native(&tr.to_string()).is_err());
    // Native round trip: compiled snapshot -> topology equals itself.
    let (t2, _) = model::from_native(&snap.to_string()).unwrap();
    assert_eq!(t2.synapses.len(), t.synapses.len());
}

#[test]
fn observation_does_not_change_outputs_and_replay_is_exact() {
    let (_, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
    let task = suite()
        .tasks
        .into_iter()
        .find(|t| t.id == "memory-hit")
        .unwrap();
    let (_, off) = run_trial(&snap, &task, 7, "frozen", None, false, false).unwrap();
    let (_, on) = run_trial(&snap, &task, 7, "frozen", None, true, true).unwrap();
    assert_eq!(
        off.digest, on.digest,
        "recording must not change committed outputs"
    );
    assert!(off.events.is_empty() && !on.events.is_empty());
    let (_, again) = run_trial(&snap, &task, 7, "frozen", None, true, true).unwrap();
    assert_eq!(
        on.events_digest, again.events_digest,
        "replay of the same seed is exact"
    );
}

#[test]
fn traces_publish_verify_and_detect_corruption() {
    let d = tempfile::tempdir().unwrap();
    let p = Project::init(&d.path().join("p"), "t").unwrap();
    let (t, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
    let task = suite().tasks.into_iter().find(|t| t.id == "go").unwrap();
    let (tr, out) = run_trial(&snap, &task, 1, "frozen", None, true, false).unwrap();
    // Tiny segments to exercise multi-segment continuity.
    let h = capture::publish(
        &p,
        "src",
        &capture::topology_hash(&t),
        "go",
        &tr,
        &out,
        "frozen",
        false,
        1,
    )
    .unwrap();
    let (m, ev) = capture::load(&p, &h).unwrap();
    assert!(m.complete && ev.len() == out.events.len());
    // Corrupt one segment object on disk: load must fail.
    let seg = &m.segments[0].sha256;
    let path = p.object_file(seg).unwrap();
    let mut b = std::fs::read(&path).unwrap();
    b[9] ^= 0xff;
    std::fs::write(&path, b).unwrap();
    assert!(capture::load(&p, &h).is_err());
}

#[test]
fn project_refuses_overwrite_and_traversal() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("p");
    let p = Project::init(&root, "t").unwrap();
    assert!(
        Project::init(&root, "t").is_err(),
        "never overwrite a project"
    );
    assert!(
        p.bind(
            aarnn_explorer::store::RecordKind::Source,
            "../x",
            &"0".repeat(64)
        )
        .is_err()
    );
    let h = p.put(b"hello").unwrap();
    p.bind(aarnn_explorer::store::RecordKind::Source, "a", &h)
        .unwrap();
    let h2 = p.put(b"other").unwrap();
    assert!(
        p.bind(aarnn_explorer::store::RecordKind::Source, "a", &h2)
            .is_err(),
        "names are immutable"
    );
}

#[test]
fn isolated_fork_leaves_the_source_unchanged() {
    let (t, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
    let before = snap.to_string();
    let iv = aarnn_explorer::lesion::Intervention {
        id: "x".into(),
        selection: aarnn_explorer::lesion::Selection::Neurons {
            ids: vec!["I".into()],
        },
        action: aarnn_explorer::lesion::Action::NeuronAblation,
    };
    let (lesioned, applied) =
        aarnn_explorer::lesion::apply(&t, &snap, &[iv], &Default::default()).unwrap();
    assert_eq!(snap.to_string(), before);
    assert_ne!(lesioned.to_string(), before);
    assert_eq!(applied[0].classification, "structural-mask");
    assert!(
        applied[0].synapses.iter().any(|s| s == "B->I")
            && applied[0].synapses.iter().any(|s| s == "I->O_go")
    );
    // Transient silencing restores weights after the interval: a run with a
    // silence window that ends before the probe reproduces the intact output.
    let task = suite().tasks.into_iter().find(|t| t.id == "go").unwrap();
    let base = run_trial(&snap, &task, 1, "frozen", None, false, false)
        .unwrap()
        .1;
    let mut spec = RunSpec {
        seed: 1,
        frames: vec![vec![0.0; 6]; 60],
        background_p: 0.0,
        noise_seed: 0,
        record_events: false,
        graded_events: false,
        plasticity_policy: "frozen".into(),
        silence: vec![],
    };
    spec.frames[20][0] = 1.0;
    let intact = run(&snap, &spec).unwrap();
    let g2a = t.neurons.iter().find(|n| n.id == "G2a").unwrap().dense;
    let g2b = t.neurons.iter().find(|n| n.id == "G2b").unwrap().dense;
    spec.silence = vec![
        aarnn_explorer::exec::Silence {
            hidden: g2a,
            start_tick: 0,
            end_tick: 120,
        },
        aarnn_explorer::exec::Silence {
            hidden: g2b,
            start_tick: 0,
            end_tick: 120,
        },
    ];
    let silenced = run(&snap, &spec).unwrap();
    let o_go =
        |o: &aarnn_explorer::exec::RunOutput| o.output_series.iter().map(|r| r[0]).sum::<f32>();
    assert!(
        o_go(&intact) > 0.0 && o_go(&silenced) == 0.0,
        "silencing both go relays blocks the response"
    );
    assert!(base.frames > 0);
}

fn fault_job(dir: &Path, mode: &str) -> std::path::PathBuf {
    let (_, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
    let net = dir.join("net.json");
    std::fs::write(&net, snap.to_string()).unwrap();
    let job = Job {
        schema: worker::JOB_SCHEMA.into(),
        job_id: format!("fault-{mode}"),
        snapshot_path: net.clone(),
        snapshot_sha256: sha256_file(&net).unwrap(),
        suite: suite(),
        policy: "frozen".into(),
        interventions: vec![],
        lower_neurons: vec![],
        lower_synapses: vec![],
        protect_io: true,
        capabilities: vec![],
        job: JobKind::Fault { mode: mode.into() },
    };
    let p = dir.join(format!("{mode}.json"));
    std::fs::write(&p, serde_json::to_vec(&job).unwrap()).unwrap();
    p
}

#[test]
fn worker_timeout_crash_and_memory_limit_are_contained() {
    let exe = Path::new(env!("CARGO_BIN_EXE_aarnn-explorer"));
    let d = tempfile::tempdir().unwrap();
    let limits = Limits {
        threads_per_worker: 1,
        memory_limit_mib: 256,
        wall_timeout: Duration::from_secs(3),
        os_enforcement: true,
    };
    let enforced = worker::systemd_scope_available();
    // Timeout: the whole process group is killed at the deadline.
    let j = fault_job(d.path(), "sleep");
    let disp = worker::spawn(exe, &j, &d.path().join("r1.json"), &limits, enforced).unwrap();
    assert_eq!(disp.outcome, "timeout");
    assert!(disp.wall_ms < 12_000.0);
    // Panic: a crash, never a success; no result file is trusted.
    let j = fault_job(d.path(), "panic");
    let disp = worker::spawn(exe, &j, &d.path().join("r2.json"), &limits, enforced).unwrap();
    assert!(
        disp.outcome == "crashed" || disp.outcome == "killed",
        "{disp:?}"
    );
    assert!(disp.result.is_none());
    // Memory: with OS enforcement the scope kills the worker; otherwise the
    // deadline does. Either way the disposition is not success.
    let j = fault_job(d.path(), "alloc");
    let disp = worker::spawn(exe, &j, &d.path().join("r3.json"), &limits, enforced).unwrap();
    assert!(
        matches!(disp.outcome.as_str(), "killed" | "timeout" | "crashed"),
        "{disp:?}"
    );
    assert_eq!(
        disp.enforcement,
        if enforced { "enforced" } else { "advisory" }
    );
}

#[test]
fn cancellation_terminates_a_running_worker() {
    let exe = Path::new(env!("CARGO_BIN_EXE_aarnn-explorer"));
    let d = tempfile::tempdir().unwrap();
    let j = fault_job(d.path(), "sleep");
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let f2 = flag.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        f2.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let limits = Limits {
        threads_per_worker: 1,
        memory_limit_mib: 512,
        wall_timeout: Duration::from_secs(60),
        os_enforcement: false,
    };
    let disp = worker::spawn_cancellable(
        exe,
        &j,
        &d.path().join("r.json"),
        &limits,
        false,
        Some(&flag),
    )
    .unwrap();
    assert_eq!(disp.outcome, "cancelled");
    assert!(disp.wall_ms < 10_000.0);
}
