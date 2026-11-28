// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Cross-network motif library (G2) and contract templates.
//!
//! **Role mapping.** Comparing motifs between different circuits needs an
//! explicit source-to-source mapping, because same-neuron recurrence is not
//! graph matching between circuits. Designed AARNN circuits are built from
//! shared blocks with systematic names, so the mapping strips side, leg and
//! index tokens:
//! - `E_lf_sw` -> `E_sw`
//! - `RS_l` -> `RS`
//! - `TCH_l_3` -> `TCH`
//! - `REV_rm_st` -> `REV_st`
//!
//! The rule is versioned (`role-name-v1`). Two cells with the same role in
//! different networks are hypotheses of homology, not proofs.
//!
//! **Library entries.** Each entry is a role-level edge multiset (pre role ->
//! post role) taken from verified recurring motifs. It records the networks
//! and tasks where the motif occurs, its occurrence count, and the GNAT
//! participation of its roles.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::error::{ExplorerError, Result};
use crate::store::RecordKind;

pub const ROLE_RULE: &str = "role-name-v1";

const SIDE: &[&str] = &[
    "l", "r", "f", "b", "lf", "lm", "lr", "rf", "rm", "rr", "lh", "rh", "fl", "fr", "hl", "hr",
    "left", "right", "front", "rear",
];

pub fn role(name: &str) -> String {
    let parts: Vec<&str> = name
        .split('_')
        .filter(|t| {
            !t.is_empty()
                && !SIDE.contains(t)
                && t.parse::<i64>().is_err()
                && !t.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    if parts.is_empty() {
        name.to_string()
    } else {
        parts.join("_")
    }
}

/// Aggregate the motifs of several analysis records into a role library.
pub fn build(ctx: &crate::pipeline::Ctx, analyses: &[String]) -> Result<Value> {
    let mut entries: BTreeMap<Vec<(String, String)>, (BTreeSet<String>, BTreeSet<String>, u64)> =
        BTreeMap::new();
    let mut roles: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut nets = Vec::new();
    // Hidden-to-hidden role edges seen inside any recurring motif, per network.
    let mut role_edges: BTreeMap<(String, String), BTreeMap<String, u64>> = BTreeMap::new();
    for a in analyses {
        let h = ctx.project.lookup(RecordKind::Analysis, a)?;
        let rec: Value = ctx.project.get_json(&h)?;
        let source = rec["source"].as_str().unwrap_or(a).to_string();
        let src: Value = ctx
            .project
            .get_json(&ctx.project.lookup(RecordKind::Source, &source)?)?;
        let topo: crate::model::Topology = ctx
            .project
            .get_json(src["topology_object"].as_str().unwrap_or(""))?;
        let hidden: BTreeSet<String> = topo
            .neurons
            .iter()
            .filter(|n| n.role == crate::model::Role::Hidden)
            .map(|n| n.id.clone())
            .collect();
        nets.push(json!({"analysis": a, "source": source, "method": rec["method"]}));
        for p in rec["participation"].as_array().cloned().unwrap_or_default() {
            let r = role(p["id"].as_str().unwrap_or(""));
            *roles
                .entry(r)
                .or_default()
                .entry(source.clone())
                .or_insert(0) += p["events_in_gnats"].as_u64().unwrap_or(0);
        }
        for t in rec["tasks"].as_array().cloned().unwrap_or_default() {
            let task = t["task"].as_str().unwrap_or("").to_string();
            for m in t["motifs"].as_array().cloned().unwrap_or_default() {
                let mut key: Vec<(String, String)> = m["edges"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|e| {
                        e.as_str()
                            .and_then(|s| s.split_once("->"))
                            .map(|(a, b)| (role(a), role(b)))
                    })
                    .collect();
                key.sort();
                key.dedup();
                if key.is_empty() {
                    continue;
                }
                let e = entries.entry(key).or_default();
                e.0.insert(source.clone());
                e.1.insert(format!("{source}/{task}"));
                e.2 += m["occurrences"].as_u64().unwrap_or(1);
                for ed in m["edges"].as_array().cloned().unwrap_or_default() {
                    if let Some((x, y)) = ed.as_str().and_then(|s| s.split_once("->")) {
                        if hidden.contains(x) && hidden.contains(y) {
                            *role_edges
                                .entry((role(x), role(y)))
                                .or_default()
                                .entry(source.clone())
                                .or_insert(0) += m["occurrences"].as_u64().unwrap_or(1);
                        }
                    }
                }
            }
        }
    }
    if nets.len() < 2 {
        return Err(ExplorerError::InvalidInput(
            "a cross-network library needs at least two analyses".into(),
        ));
    }
    let mut lib: Vec<Value> = entries
        .into_iter()
        .map(|(k, (n, t, occ))| {
            json!({
                "role_edges": k.iter().map(|(a, b)| format!("{a}->{b}")).collect::<Vec<_>>(),
                "networks": n, "shared_by": n.len(), "tasks": t, "occurrences": occ,
            })
        })
        .collect();
    lib.sort_by(|a, b| {
        b["shared_by"]
            .as_u64()
            .cmp(&a["shared_by"].as_u64())
            .then(b["occurrences"].as_u64().cmp(&a["occurrences"].as_u64()))
    });
    let shared_roles: Vec<Value> = roles
        .iter()
        .filter(|(_, by)| by.len() >= 2)
        .map(|(r, by)| json!({"role": r, "networks": by.keys().collect::<Vec<_>>(), "events_in_gnats": by}))
        .collect();
    let shared_edges: Vec<Value> = role_edges
        .iter()
        .filter(|(_, by)| by.len() >= 2)
        .map(|((a, b), by)| json!({"role_edge": format!("{a}->{b}"), "networks": by.keys().collect::<Vec<_>>(), "occurrences": by}))
        .collect();
    // Pairwise Jaccard overlap of the networks' hidden role-edge sets.
    let names: Vec<String> = nets
        .iter()
        .map(|n| n["source"].as_str().unwrap_or("").to_string())
        .collect();
    let set = |n: &str| -> BTreeSet<&(String, String)> {
        role_edges
            .iter()
            .filter(|(_, by)| by.contains_key(n))
            .map(|(k, _)| k)
            .collect()
    };
    let mut overlap = Vec::new();
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let (a, b) = (set(&names[i]), set(&names[j]));
            let inter = a.intersection(&b).count();
            let uni = a.union(&b).count();
            overlap.push(json!({"a": names[i], "b": names[j], "shared_role_edges": inter, "jaccard": if uni > 0 { inter as f64 / uni as f64 } else { 0.0 }}));
        }
    }
    let out = json!({
        "schema": "aarnn-explorer-library-v1", "role_rule": ROLE_RULE, "networks": nets,
        "motifs": lib, "shared_roles": shared_roles, "shared_role_edges": shared_edges, "pairwise_overlap": overlap,
        "note": "role identity is a naming-based homology hypothesis; motif sharing is evidence of reused circuit blocks, not of equal behavioural role",
    });
    let h = ctx.project.put_json(&out)?;
    ctx.project
        .bind(RecordKind::Report, &format!("library-{}", &h[..12]), &h)?;
    Ok(
        json!({"library": h, "motifs": out["motifs"].as_array().map(|m| m.len()), "shared_motifs": out["motifs"].as_array().map(|m| m.iter().filter(|x| x["shared_by"].as_u64().unwrap_or(0) >= 2).count()), "shared_roles": shared_roles.len(), "shared_role_edges": out["shared_role_edges"], "pairwise_overlap": out["pairwise_overlap"]}),
    )
}

/// Generic output-equivalence contract for any executable source: a quiet
/// baseline, plus `patterns` seeded patterns of sensory segments (10 % of
/// the channels each, value 0.5, 80-160 ms long). Each task requires every
/// output's window mean to stay within 10 % + 0.02 of the reference. This is
/// a template to be reviewed, not a validated behavioural contract.
pub fn suite_template(
    snapshot: &Value,
    name: &str,
    patterns: usize,
    seed: u64,
    duration_ms: u32,
) -> Result<String> {
    let l = &snapshot["connectome_labels"];
    let list = |k: &str| -> Vec<String> {
        l[k].as_array()
            .map(|a| {
                a.iter()
                    .map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| x.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let sensory = {
        let p = list("physical_sensory_nodes");
        if p.is_empty() {
            list("sensory_nodes")
        } else {
            p
        }
    };
    let outputs = {
        let p = list("physical_output_nodes");
        if p.is_empty() {
            list("output_nodes")
        } else {
            p
        }
    };
    if sensory.is_empty() || outputs.is_empty() {
        return Err(ExplorerError::InvalidInput(
            "source has no labelled sensory/output channels".into(),
        ));
    }
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut s = format!(
        "# GENERATED by `aarnn-explorer suite-template` (seed {seed}): a template output-equivalence\n# contract to review, not a validated behavioural contract.\nschema_version = 1\nname = {}\n\n[splits]\ndevelopment = [1, 2, 3]\nlocked_validation = [101, 102, 103]\n",
        q(name)
    );
    let mut rng = fastrand::Rng::with_seed(seed);
    for k in 0..=patterns {
        let id = if k == 0 {
            "baseline".to_string()
        } else {
            format!("pattern-{k}")
        };
        s += &format!(
            "\n[[task]]\nid = {}\nfamily = \"discrimination\"\nduration_ms = {duration_ms}\njitter_ms = {}\nstimulus = [\n",
            q(&id),
            if k == 0 { 0 } else { 4 }
        );
        if k > 0 {
            let n = (sensory.len() / 10).max(1);
            let mut idx: Vec<usize> = (0..sensory.len()).collect();
            rng.shuffle(&mut idx);
            for &i in idx.iter().take(n) {
                let a = 100 + 2 * rng.u32(0..(duration_ms / 4).max(1));
                let len = 80 + 2 * rng.u32(0..40);
                s += &format!(
                    "  {{ channel = {}, start_ms = {a}, end_ms = {}, value = 0.5, jitter = true }},\n",
                    q(&sensory[i]),
                    (a + len).min(duration_ms)
                );
            }
        }
        s += "]\nmeasure = [\n";
        for (j, o) in outputs.iter().enumerate() {
            s += &format!(
                "  {{ name = \"m{j:03}\", output = {}, start_ms = 100, end_ms = {duration_ms}, stat = \"mean\" }},\n",
                q(o)
            );
        }
        s += "]\ncriterion = [\n";
        for j in 0..outputs.len() {
            s += &format!(
                "  {{ measure = \"m{j:03}\", op = \"within-reference\", amount = 0.1, value = 0.02 }},\n"
            );
        }
        s += "]\n";
    }
    crate::tasks::TaskSuite::parse(&s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_rule_strips_sides_legs_and_indices() {
        assert_eq!(role("E_lf_sw"), "E_sw");
        assert_eq!(role("RS_l"), "RS");
        assert_eq!(role("TCH_l_3"), "TCH");
        assert_eq!(role("REV_rm_st"), "REV_st");
        assert_eq!(role("MLR"), "MLR");
        assert_eq!(role("ESC_A_r"), "ESC_A");
        assert_eq!(role("Q+_leg_-1"), "Q+_leg");
    }

    #[test]
    fn suite_template_parses_for_the_demo_circuit() {
        let (_, snap) = crate::author::demo_circuit(crate::author::DemoWeights::default())
            .compile()
            .unwrap();
        let s = suite_template(&snap, "tmpl", 2, 5, 400).unwrap();
        let suite = crate::tasks::TaskSuite::parse(&s).unwrap();
        assert_eq!(suite.tasks.len(), 3);
    }
}

/// Computation-depth comparison across analyses (VX-001): per analysis, the
/// GNAT depth statistics pooled over every task and trial, with the source's
/// hidden-cell count.
pub fn depth_report(ctx: &crate::pipeline::Ctx, analyses: &[String]) -> Result<Value> {
    let mut rows = Vec::new();
    for a in analyses {
        let rec: Value = ctx
            .project
            .get_json(&ctx.project.lookup(RecordKind::Analysis, a)?)?;
        let src: Value = ctx.project.get_json(
            &ctx.project
                .lookup(RecordKind::Source, rec["source"].as_str().unwrap_or(""))?,
        )?;
        let hidden = src["summary"]["hidden"].as_u64().unwrap_or(0);
        let (mut n, mut gnats, mut sum_mean, mut sum_dpn, mut maxd, mut part, mut p95) =
            (0u64, 0u64, 0.0f64, 0.0f64, 0u64, 0u64, 0u64);
        let mut per_task = Vec::new();
        for t in rec["tasks"].as_array().cloned().unwrap_or_default() {
            let ds = t["depth"].as_array().cloned().unwrap_or_default();
            let tm = ds
                .iter()
                .map(|d| d["mean_depth"].as_f64().unwrap_or(0.0))
                .sum::<f64>()
                / ds.len().max(1) as f64;
            per_task.push(json!({"task": t["task"], "mean_depth": tm, "max_depth": ds.iter().map(|d| d["max_depth"].as_u64().unwrap_or(0)).max()}));
            for d in ds {
                n += 1;
                gnats += d["gnats"].as_u64().unwrap_or(0);
                sum_mean += d["mean_depth"].as_f64().unwrap_or(0.0);
                sum_dpn += d["mean_depth_per_neuron"].as_f64().unwrap_or(0.0);
                maxd = maxd.max(d["max_depth"].as_u64().unwrap_or(0));
                p95 = p95.max(d["p95_depth"].as_u64().unwrap_or(0));
                part = part.max(d["participating_neurons"].as_u64().unwrap_or(0));
            }
        }
        let nf = n.max(1) as f64;
        rows.push(json!({
            "analysis": a, "source": rec["source"], "method": rec["method"], "hidden_cells": hidden, "trials": n,
            "gnats_per_trial": gnats as f64 / nf, "mean_depth": sum_mean / nf, "max_depth": maxd, "max_p95_depth": p95,
            "mean_depth_per_neuron": sum_dpn / nf, "max_participating_neurons": part,
            "mean_depth_per_hidden_cell": if hidden > 0 { sum_mean / nf / hidden as f64 } else { 0.0 },
            "per_task": per_task,
        }));
    }
    Ok(
        json!({"schema": "aarnn-explorer-depth-report-v1", "rows": rows,
        "note": "depth = longest causal chain (edges) inside a GNAT; graded cells contribute spike-equivalent events when the method is effective-delivery-graded-v1"}),
    )
}
