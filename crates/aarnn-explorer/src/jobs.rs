// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Job journal: monotonic cursor (line number), last state per job, and
//! restart reconciliation. A job whose last state is `running` after the
//! controller is gone becomes `interrupted`; it is never marked succeeded and
//! never resubmitted automatically.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::error::Result;
use crate::store::Project;

pub fn list(project: &Project, reconcile: bool) -> Result<Value> {
    let lines = project.journal_lines()?;
    let mut last: BTreeMap<String, (usize, Value)> = BTreeMap::new();
    for (cursor, l) in lines.iter().enumerate() {
        if let Some(j) = l["job"].as_str() {
            last.insert(j.to_string(), (cursor, l.clone()));
        }
    }
    let mut interrupted = Vec::new();
    if reconcile {
        for (j, (_, l)) in &last {
            if l["state"] == "running" {
                project.journal(&json!({"job": j, "state": "interrupted", "reason": "controller restart found no completion record"}))?;
                interrupted.push(j.clone());
            }
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, l) in last.values() {
        *counts
            .entry(l["state"].as_str().unwrap_or("?").to_string())
            .or_insert(0) += 1;
    }
    Ok(json!({
        "cursor": lines.len(), "jobs": last.len(), "states": counts, "interrupted_now": interrupted,
        "recent": last.values().rev().take(20).map(|(c, l)| json!({"cursor": c, "entry": l})).collect::<Vec<_>>(),
    }))
}
