// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! EX-EXPLORER-001: the offline end-to-end demonstration through the real
//! binary, worker processes and AARNN runner. Run optimised:
//! `cargo test --locked --release -p aarnn-explorer --test explorer_e2e`.
use std::process::Command;

#[test]
fn demo_produces_every_required_outcome() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("demo");
    let out = Command::new(env!("CARGO_BIN_EXE_aarnn-explorer"))
        .current_dir(&repo)
        .env("AARNN_EXPLORER_NO_OS_LIMITS", "1")
        .args(["demo", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "demo failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let r: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.join("demo-report.json")).unwrap()).unwrap();
    // Intact reference passes.
    assert_eq!(r["baseline"]["passed"], true, "{}", r["baseline"]);
    // Isolation: sham matches intact and the source is unchanged.
    assert_eq!(r["experiment"]["sham_matches_intact"], true);
    assert_eq!(r["experiment"]["source_unchanged"], true);
    // A damaging lesion fails (inhibitor -> nogo), a redundant one passes.
    let rows = r["experiment"]["rows"].as_array().unwrap();
    let row = |id: &str| rows.iter().find(|x| x["intervention"]["id"] == id).unwrap();
    assert_eq!(row("ablate-inhibitor")["passed"], false);
    assert_eq!(
        row("ablate-inhibitor")["failed_tasks"],
        serde_json::json!(["nogo"])
    );
    assert_eq!(row("ablate-G1a")["passed"], true);
    assert_eq!(row("ablate-go-both")["passed"], false);
    // Transient silencing of a loop cell breaks the reverberating memory.
    assert_eq!(row("silence-M2-80-120ms")["passed"], false);
    assert_eq!(
        row("silence-M2-80-120ms")["failed_tasks"],
        serde_json::json!(["memory-hit"])
    );
    // Doubling the inhibition causes rebound bursts (S-003): nogo fails.
    assert_eq!(row("inhibition-x2")["passed"], false);
    // GNATs were extracted from complete traces.
    assert!(r["analysis"]["traces"].as_u64().unwrap() > 0);
    // A genuinely smaller candidate passes the locked split.
    let c = &r["optimise_compact"];
    assert_eq!(c["result"], "candidate");
    assert!(
        c["selected_cost"]["hidden"].as_u64().unwrap()
            < c["intact_cost"]["hidden"].as_u64().unwrap()
    );
    assert!(
        c["selected_cost"]["state_bytes"].as_u64().unwrap()
            < c["intact_cost"]["state_bytes"].as_u64().unwrap()
    );
    assert_eq!(r["validate_compact"]["behavioural_preservation"], "passed");
    // The compact candidate loses resilience relative to the resilient one.
    assert_eq!(r["compact_under_resilient_profile"]["resilience"], "failed");
    let rc = &r["optimise_resilient"];
    assert_eq!(rc["result"], "candidate");
    assert!(
        rc["selected_cost"]["hidden"].as_u64().unwrap()
            > c["selected_cost"]["hidden"].as_u64().unwrap()
    );
    // Its sampled claim cannot be established with 20 trials: inconclusive.
    assert_eq!(r["validate_resilient"]["resilience"], "inconclusive");
    // Clean-process reimport reproduces the validation.
    assert_eq!(r["verify_bundle"]["reproduces_validation"], true);
    assert_eq!(r["verify_bundle"]["revalidation_passed"], true);
}
