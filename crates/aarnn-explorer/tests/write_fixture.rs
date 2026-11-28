// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! The checked-in demo model must equal the authored circuit (generator-
//! versioned fixture). Regenerate with:
//! `AARNN_EXPLORER_WRITE_FIXTURE=1 cargo test -p aarnn-explorer --test write_fixture`.
use aarnn_explorer::author::{DemoWeights, demo_circuit};

const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/explorer/models/synthetic_sensorimotor.sparse.json"
);

#[test]
fn checked_in_fixture_matches_the_authored_circuit() {
    let doc = serde_json::to_string_pretty(&demo_circuit(DemoWeights::default()).sparse()).unwrap()
        + "\n";
    if std::env::var_os("AARNN_EXPLORER_WRITE_FIXTURE").is_some() {
        std::fs::create_dir_all(std::path::Path::new(PATH).parent().unwrap()).unwrap();
        std::fs::write(PATH, &doc).unwrap();
    }
    let on_disk = std::fs::read_to_string(PATH)
        .expect("fixture present; regenerate with AARNN_EXPLORER_WRITE_FIXTURE=1");
    assert_eq!(on_disk, doc, "fixture is stale; regenerate it");
}
