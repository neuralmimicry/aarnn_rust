// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Function-preserving reduction search and resilience campaigns (brief
//! Sections 11 and 12).
//!
//! Deterministic baseline optimiser:
//! 1. Revalidate the intact reference on the development split. A failing
//!    reference stops the search with a negative result.
//! 2. Remove the verified-inert cells (`lesion::inert_cells`).
//! 3. Screen single ablations. Each cell gets an importance score:
//!    1000 x the number of required tasks its ablation fails, plus its GNAT
//!    participation (events inside GNATs, from the analysis when one exists).
//!    These are hypotheses, not proofs.
//! 4. Beam search over physically lowered candidates. A child is its parent
//!    plus one removal. Every child is re-evaluated in full (cumulative
//!    revalidation), because two individually dispensable cells can be
//!    jointly essential. Beam selection sorts by (cost key, removal set), so
//!    it does not depend on job completion order.
//! 5. A greedy synapse pass on the best state.
//! 6. A Pareto frontier over the configured cost dimensions, then selection,
//!    then resilience checks on frontier candidates in cost order. The first
//!    candidate that passes the profile is selected.
//!
//! Validation on the locked split happens later (`validate`), never here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{ExplorerError, Result};
use crate::lesion::{self, Action, Cost, Intervention, Protection, Selection};
use crate::model::{Role, Topology};
use crate::pipeline::{Ctx, verdict_of};
use crate::stats;
use crate::store::RecordKind;
use crate::tasks::TaskSuite;
use crate::worker::{Job, JobKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Proposal {
    pub removed_neurons: Vec<String>,
    pub removed_synapses: Vec<String>,
    pub passed: bool,
    pub failed_tasks: Vec<String>,
    pub cost: Cost,
    pub round: u32,
    pub job: String,
}

fn cost_key(c: &Cost, dims: &[String]) -> Vec<u64> {
    dims.iter()
        .map(|d| match d.as_str() {
            "state_bytes" => c.state_bytes,
            "synapses" => c.synapses,
            "neurons" => c.neurons,
            "memory_bytes" => c.memory_bytes,
            "latency_ms" => c.latency_ns_p50.unwrap_or(u64::MAX),
            _ => 0,
        })
        .collect()
}

/// Pareto frontier (minimisation) over the cost dimensions. Latency is a
/// noisy measurement, so it never breaks dominance alone: two candidates
/// with equal structural costs are both kept.
pub fn pareto(props: &[Proposal], dims: &[String]) -> Vec<usize> {
    let structural: Vec<String> = dims
        .iter()
        .filter(|d| d.as_str() != "latency_ms")
        .cloned()
        .collect();
    let keys: Vec<Vec<u64>> = props
        .iter()
        .map(|p| cost_key(&p.cost, &structural))
        .collect();
    (0..props.len())
        .filter(|&i| props[i].passed)
        .filter(|&i| {
            !(0..props.len()).any(|j| {
                j != i
                    && props[j].passed
                    && keys[j].iter().zip(&keys[i]).all(|(a, b)| a <= b)
                    && keys[j].iter().zip(&keys[i]).any(|(a, b)| a < b)
            })
        })
        .collect()
}

fn lower_and_store(
    ctx: &Ctx,
    topo: &Topology,
    snap: &Value,
    neurons: &[String],
    synapses: &[String],
) -> Result<(String, std::path::PathBuf, Value)> {
    let prot = Protection {
        protect_io: true,
        ..Default::default()
    };
    let (low, map) = lesion::lower(topo, snap, neurons, synapses, &prot)?;
    let h = ctx.project.put(low.to_string().as_bytes())?;
    Ok((
        h.clone(),
        ctx.project.object_file(&h)?,
        serde_json::to_value(map)?,
    ))
}

fn eval_jobs(
    ctx: &Ctx,
    suite: &TaskSuite,
    base_h: &str,
    base_p: &std::path::Path,
    split: &str,
    rm: &BTreeMap<String, BTreeMap<String, f64>>,
    states: &[(Vec<String>, Vec<String>)],
) -> Vec<Job> {
    states
        .iter()
        .map(|(n, s)| {
            let mut j = ctx.job(
                base_h,
                base_p,
                suite,
                JobKind::Evaluate {
                    split: split.into(),
                    reference: Some(rm.clone()),
                },
            );
            j.lower_neurons = n.clone();
            j.lower_synapses = s.clone();
            j
        })
        .collect()
}

/// Exhaustive single-neuron faults over the eligible population of a
/// (lowered) snapshot. Returns (id, passed, failed tasks).
pub fn single_faults(
    ctx: &Ctx,
    suite: &TaskSuite,
    snap_h: &str,
    snap_p: &std::path::Path,
    topo: &Topology,
    split: &str,
    rm: &BTreeMap<String, BTreeMap<String, f64>>,
) -> Result<Vec<(String, bool, Vec<String>)>> {
    let regions = &ctx.config.resilience.fault_regions;
    let pop: Vec<String> = topo
        .neurons
        .iter()
        .filter(|n| n.role == Role::Hidden && (regions.is_empty() || regions.contains(&n.region)))
        .map(|n| n.id.clone())
        .collect();
    if pop.len() > 2000 {
        return Err(ExplorerError::Budget(format!(
            "{} eligible cells: exhaustive single-fault testing is bounded to 2000",
            pop.len()
        )));
    }
    let jobs: Vec<Job> = pop
        .iter()
        .map(|id| {
            let mut j = ctx.job(
                snap_h,
                snap_p,
                suite,
                JobKind::Evaluate {
                    split: split.into(),
                    reference: Some(rm.clone()),
                },
            );
            j.interventions = vec![Intervention {
                id: format!("fault-{id}"),
                selection: Selection::Neurons {
                    ids: vec![id.clone()],
                },
                action: Action::NeuronAblation,
            }];
            j
        })
        .collect();
    let mut out = Vec::new();
    for (j, d) in ctx.run_jobs(jobs)? {
        let id = match &j.interventions[0].selection {
            Selection::Neurons { ids } => ids[0].clone(),
            _ => unreachable!(),
        };
        let (v, _, _) = verdict_of(&d)?;
        out.push((
            id,
            v.passed,
            v.tasks
                .iter()
                .filter(|t| !t.passed)
                .map(|t| t.task.clone())
                .collect(),
        ));
    }
    Ok(out)
}

/// Random neuron-loss claim: `trials` independent random selections of the
/// eligible population; success = the suite split passes.
pub fn random_loss(
    ctx: &Ctx,
    suite: &TaskSuite,
    snap_h: &str,
    snap_p: &std::path::Path,
    split: &str,
    rm: &BTreeMap<String, BTreeMap<String, f64>>,
    fraction: f64,
    trials: u64,
    seed: u64,
) -> Result<(u64, u64, Vec<Value>)> {
    let jobs: Vec<Job> = (0..trials)
        .map(|k| {
            let mut j = ctx.job(
                snap_h,
                snap_p,
                suite,
                JobKind::Evaluate {
                    split: split.into(),
                    reference: Some(rm.clone()),
                },
            );
            j.interventions = vec![Intervention {
                id: format!("random-{k}"),
                selection: Selection::Random {
                    fraction,
                    seed: seed.wrapping_add(k),
                    regions: ctx.config.resilience.fault_regions.clone(),
                },
                action: Action::NeuronAblation,
            }];
            j
        })
        .collect();
    let mut ok = 0u64;
    let mut failures = Vec::new();
    for (j, d) in ctx.run_jobs(jobs)? {
        let (v, _, meta) = verdict_of(&d)?;
        if v.passed {
            ok += 1;
        } else if failures.len() < 10 {
            failures.push(json!({"selection": meta["applied"][0]["neurons"], "failed": v.tasks.iter().filter(|t| !t.passed).map(|t| t.task.clone()).collect::<Vec<_>>(), "job": j.job_id}));
        }
    }
    Ok((ok, trials, failures))
}

/// Resilience verdict of an executable snapshot under the context's profile.
pub fn resilience(
    ctx: &Ctx,
    suite: &TaskSuite,
    snap_h: &str,
    snap_p: &std::path::Path,
    topo: &Topology,
    split: &str,
    rm: &BTreeMap<String, BTreeMap<String, f64>>,
) -> Result<Value> {
    let r = &ctx.config.resilience;
    let mut claims = Vec::new();
    let mut singles = Value::Null;
    if r.targeted_single_faults_required {
        let s = single_faults(ctx, suite, snap_h, snap_p, topo, split, rm)?;
        let survived = s.iter().filter(|x| x.1).count();
        claims.push(json!({
            "claim": format!("every single neuron loss in {:?} is survived (exhaustive, {} cells)", if r.fault_regions.is_empty() { vec!["all".to_string()] } else { r.fault_regions.clone() }, s.len()),
            "verdict": if survived == s.len() { "passed" } else { "failed" }, "survived": survived, "tested": s.len(),
            "scope": "exhaustive single faults over the declared population only",
        }));
        singles = json!(
            s.iter()
                .map(|(id, p, f)| json!({"neuron": id, "passed": p, "failed_tasks": f}))
                .collect::<Vec<_>>()
        );
    }
    let sampled = r.random_neuron_loss_fraction > 0.0;
    let n_claims = claims.len() + usize::from(sampled);
    let per_conf = if r.confidence_scope == "simultaneous-required-claims" {
        stats::bonferroni_per_claim_confidence(r.confidence, n_claims.max(1))
    } else {
        r.confidence
    };
    if sampled {
        let (ok, n, failures) = random_loss(
            ctx,
            suite,
            snap_h,
            snap_p,
            split,
            rm,
            r.random_neuron_loss_fraction,
            r.trials_per_sampled_claim,
            ctx.config.execution.seed,
        )?;
        let (v, lo, hi) = stats::probability_claim(ok, n, r.minimum_success_probability, per_conf);
        claims.push(json!({
            "claim": format!("P(success | random {:.0} % neuron loss) >= {}", 100.0 * r.random_neuron_loss_fraction, r.minimum_success_probability),
            "verdict": v, "successes": ok, "trials": n, "lower_bound": lo, "upper_bound": hi, "per_claim_confidence": per_conf,
            "trials_needed_if_no_failures": stats::zero_failure_trials_needed(r.minimum_success_probability, per_conf),
            "failures": failures, "method": "one-sided Clopper-Pearson, Bonferroni-adjusted",
        }));
    }
    let verdict = if claims.is_empty() {
        "not-required"
    } else if claims.iter().any(|c| c["verdict"] == "failed") {
        "failed"
    } else if claims.iter().any(|c| c["verdict"] == "inconclusive") {
        "inconclusive"
    } else {
        "passed"
    };
    Ok(
        json!({"profile": ctx.config.resilience.preset, "expanded": ctx.config.expanded_resilience(), "verdict": verdict, "claims": claims, "single_faults": singles, "family_confidence": r.confidence}),
    )
}

pub fn optimise(
    ctx: &Ctx,
    source: Option<&str>,
    select: &str,
    analysis: Option<&str>,
) -> Result<Value> {
    let source = ctx.default_source(source)?;
    let rec: Value = ctx
        .project
        .get_json(&ctx.project.lookup(RecordKind::Source, &source)?)?;
    let snap_h = rec["snapshot_object"]
        .as_str()
        .ok_or_else(|| ExplorerError::Unsupported(format!("source {source} is not executable")))?
        .to_string();
    let snap_p = ctx.project.object_file(&snap_h)?;
    let topo: Topology = ctx
        .project
        .get_json(rec["topology_object"].as_str().unwrap_or(""))?;
    let snap: Value = ctx.project.get_json(&snap_h)?;
    let (suite, suite_h) = ctx.suite()?;
    let dev = ctx.config.functional.development_split.clone();
    let dims = ctx.config.optimisation.cost_dimensions.clone();
    let budget = ctx.config.optimisation.proposal_budget as usize;
    let mut used = 0usize;
    // 1. Intact reference.
    let intact = ctx.run_jobs(vec![ctx.job(
        &snap_h,
        &snap_p,
        &suite,
        JobKind::Evaluate {
            split: dev.clone(),
            reference: None,
        },
    )])?;
    let (iv, icost, _) = verdict_of(&intact[0].1)?;
    if !iv.passed {
        let neg = json!({"schema": "aarnn-explorer-optimisation-v1", "source": source, "result": "negative", "reason": "the intact reference fails its own contract on the development split", "intact": iv});
        let h = ctx.project.put_json(&neg)?;
        return Ok(json!({"result": "negative", "object": h}));
    }
    let rm = crate::tasks::reference_measures(&iv);
    let mut log: Vec<Proposal> = vec![Proposal {
        removed_neurons: vec![],
        removed_synapses: vec![],
        passed: true,
        failed_tasks: vec![],
        cost: icost.clone(),
        round: 0,
        job: intact[0].0.job_id.clone(),
    }];
    // 2. Verified inert cells.
    let (inert, inert_notes) = lesion::inert_cells(&topo, &snap);
    let mut start: Vec<String> = Vec::new();
    if !inert.is_empty() && ctx.config.optimisation.allow_neuron_removal {
        let jobs = eval_jobs(
            ctx,
            &suite,
            &snap_h,
            &snap_p,
            &dev,
            &rm,
            &[(inert.clone(), vec![])],
        );
        let (j, d) = ctx.run_jobs(jobs)?.remove(0);
        used += 1;
        let (v, c, _) = verdict_of(&d)?;
        log.push(Proposal {
            removed_neurons: inert.clone(),
            removed_synapses: vec![],
            passed: v.passed,
            failed_tasks: failed(&v),
            cost: c,
            round: 1,
            job: j.job_id,
        });
        if v.passed {
            start = inert.clone();
        }
    }
    // 3. Single-ablation screen + GNAT participation -> importance.
    let participation: BTreeMap<String, u64> = analysis
        .and_then(|a| ctx.project.lookup(RecordKind::Analysis, a).ok())
        .and_then(|h| ctx.project.get_json::<Value>(&h).ok())
        .map(|a| {
            a["participation"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|p| {
                    (
                        p["id"].as_str().unwrap_or("").to_string(),
                        p["events_in_gnats"].as_u64().unwrap_or(0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    // Learning substrate: plastic cells, presynaptic plastic cells and
    // modulatory cells are protected unless the contract tests learning; a
    // frozen open-loop contract cannot show they are dispensable.
    let substrate = learning_substrate(&topo, &snap);
    let protect_substrate = !ctx.config.functional.learning_required;
    let declared = declared_protection(&topo, &ctx.config.optimisation)?;
    let candidates: Vec<String> = topo
        .neurons
        .iter()
        .filter(|n| {
            n.role == Role::Hidden
                && !start.contains(&n.id)
                && !(protect_substrate && substrate.contains(&n.id))
                && !declared.contains(&n.id)
        })
        .map(|n| n.id.clone())
        .collect();
    let screen_jobs: Vec<Job> = candidates
        .iter()
        .map(|id| {
            let mut j = ctx.job(
                &snap_h,
                &snap_p,
                &suite,
                JobKind::Evaluate {
                    split: dev.clone(),
                    reference: Some(rm.clone()),
                },
            );
            j.interventions = vec![Intervention {
                id: format!("screen-{id}"),
                selection: Selection::Neurons {
                    ids: vec![id.clone()],
                },
                action: Action::NeuronAblation,
            }];
            j
        })
        .collect();
    let mut importance: Vec<(u64, String, Vec<String>)> = Vec::new();
    for (j, d) in ctx.run_jobs(screen_jobs)? {
        let id = match &j.interventions[0].selection {
            Selection::Neurons { ids } => ids[0].clone(),
            _ => unreachable!(),
        };
        let (v, _, _) = verdict_of(&d)?;
        let f = failed(&v);
        let req = v.tasks.iter().filter(|t| t.required && !t.passed).count() as u64;
        importance.push((
            1000 * req + participation.get(&id).copied().unwrap_or(0),
            id,
            f,
        ));
    }
    importance.sort();
    let hypotheses: Vec<String> = importance
        .iter()
        .filter(|x| x.2.is_empty())
        .map(|x| x.1.clone())
        .collect();
    // 4. Beam search over neuron removals.
    let width = ctx.config.optimisation.beam_width as usize;
    let mut beam: Vec<Vec<String>> = vec![start.clone()];
    let mut seen: BTreeSet<Vec<String>> = BTreeSet::new();
    seen.insert(start.clone());
    let mut round = 2u32;
    while ctx.config.optimisation.allow_neuron_removal && used < budget {
        let mut children: Vec<Vec<String>> = Vec::new();
        for state in &beam {
            for h in &hypotheses {
                if state.contains(h) {
                    continue;
                }
                let mut c = state.clone();
                c.push(h.clone());
                c.sort();
                if seen.insert(c.clone()) {
                    children.push(c);
                }
            }
        }
        children.truncate(budget - used);
        if children.is_empty() {
            break;
        }
        used += children.len();
        let states: Vec<(Vec<String>, Vec<String>)> =
            children.iter().map(|c| (c.clone(), vec![])).collect();
        let mut passing: Vec<(Vec<u64>, Vec<String>)> = Vec::new();
        for ((n, _), (j, d)) in states
            .iter()
            .zip(ctx.run_jobs(eval_jobs(ctx, &suite, &snap_h, &snap_p, &dev, &rm, &states))?)
        {
            let (v, c, _) = verdict_of(&d)?;
            if v.passed {
                passing.push((cost_key(&c, &dims), n.clone()));
            }
            log.push(Proposal {
                removed_neurons: n.clone(),
                removed_synapses: vec![],
                passed: v.passed,
                failed_tasks: failed(&v),
                cost: c,
                round,
                job: j.job_id,
            });
        }
        if passing.is_empty() {
            break;
        }
        passing.sort();
        beam = passing.into_iter().take(width).map(|x| x.1).collect();
        round += 1;
    }
    // 5. Greedy synapse pass on the best neuron state.
    let best = log
        .iter()
        .filter(|p| p.passed && p.removed_synapses.is_empty())
        .min_by_key(|p| (cost_key(&p.cost, &dims), p.removed_neurons.clone()))
        .map(|p| p.removed_neurons.clone())
        .unwrap_or_default();
    let mut syn_removed: Vec<String> = Vec::new();
    if ctx.config.optimisation.allow_synapse_removal {
        let gone: BTreeSet<u32> = best.iter().filter_map(|id| topo.index_of(id)).collect();
        let mut syns: Vec<&crate::model::Synapse> = topo
            .synapses
            .iter()
            .filter(|s| !gone.contains(&s.pre) && !gone.contains(&s.post))
            .collect();
        syns.sort_by(|a, b| {
            a.weight
                .abs()
                .total_cmp(&b.weight.abs())
                .then(a.id.cmp(&b.id))
        });
        for s in syns {
            if used >= budget {
                break;
            }
            let mut trial = syn_removed.clone();
            trial.push(s.id.clone());
            used += 1;
            let (j, d) = ctx
                .run_jobs(eval_jobs(
                    ctx,
                    &suite,
                    &snap_h,
                    &snap_p,
                    &dev,
                    &rm,
                    &[(best.clone(), trial.clone())],
                ))?
                .remove(0);
            let (v, c, _) = verdict_of(&d)?;
            log.push(Proposal {
                removed_neurons: best.clone(),
                removed_synapses: trial.clone(),
                passed: v.passed,
                failed_tasks: failed(&v),
                cost: c,
                round,
                job: j.job_id,
            });
            if v.passed {
                syn_removed = trial;
            }
        }
    }
    // 6. Frontier, selection, resilience.
    let front = pareto(&log, &dims);
    let sel_dim = match select {
        "min-state-bytes" => "state_bytes",
        "min-neurons" => "neurons",
        "min-synapses" => "synapses",
        s => {
            return Err(ExplorerError::InvalidInput(format!(
                "--select {s}: min-state-bytes, min-neurons or min-synapses"
            )));
        }
    };
    let mut order: Vec<usize> = front.clone();
    order.sort_by_key(|&i| {
        (
            cost_key(&log[i].cost, &[sel_dim.to_string()]),
            log[i].removed_neurons.len() + log[i].removed_synapses.len(),
            log[i].removed_neurons.clone(),
            log[i].removed_synapses.clone(),
        )
    });
    // Candidates beyond the frontier are needed when resilience rejects the
    // compact frontier: walk every passing proposal in the same order.
    let mut fallback: Vec<usize> = (0..log.len())
        .filter(|&i| log[i].passed && !front.contains(&i))
        .collect();
    fallback.sort_by_key(|&i| {
        (
            cost_key(&log[i].cost, &[sel_dim.to_string()]),
            log[i].removed_neurons.clone(),
            log[i].removed_synapses.clone(),
        )
    });
    order.extend(fallback);
    let needs_resilience = ctx.config.resilience.targeted_single_faults_required
        || ctx.config.resilience.random_neuron_loss_fraction > 0.0;
    let mut chosen = None;
    let mut checked = Vec::new();
    for &i in &order {
        let p = &log[i];
        let (low_h, low_p, mapping) =
            lower_and_store(ctx, &topo, &snap, &p.removed_neurons, &p.removed_synapses)?;
        if !needs_resilience {
            chosen = Some((i, low_h, mapping, json!({"verdict": "not-required"})));
            break;
        }
        let low_snap: Value = ctx.project.get_json(&low_h)?;
        let (low_topo, _) = crate::model::from_native(&low_snap.to_string())?;
        // Single faults during selection: the loss of cell x from candidate R
        // is the lowered state R + {x}, the same job the beam evaluates, so
        // completed results are reused. Physical removal is also the most
        // faithful model of losing a neuron. Sampled claims are assessed at
        // validation on the locked split.
        let regions = &ctx.config.resilience.fault_regions;
        let pop: Vec<String> = low_topo
            .neurons
            .iter()
            .filter(|n| {
                n.role == Role::Hidden && (regions.is_empty() || regions.contains(&n.region))
            })
            .map(|n| n.id.clone())
            .collect();
        let states: Vec<(Vec<String>, Vec<String>)> = pop
            .iter()
            .map(|x| {
                let mut r = p.removed_neurons.clone();
                r.push(x.clone());
                r.sort();
                (r, p.removed_synapses.clone())
            })
            .collect();
        let mut faults = Vec::new();
        if ctx.config.resilience.targeted_single_faults_required {
            for ((r, _), (_, d)) in states
                .iter()
                .zip(ctx.run_jobs(eval_jobs(ctx, &suite, &snap_h, &snap_p, &dev, &rm, &states))?)
            {
                let (v, _, _) = verdict_of(&d)?;
                let x = r
                    .iter()
                    .find(|c| !p.removed_neurons.contains(c))
                    .cloned()
                    .unwrap_or_default();
                faults.push(json!({"neuron": x, "passed": v.passed, "failed_tasks": failed(&v)}));
            }
        }
        let single_ok = faults.iter().all(|f| f["passed"] == true);
        let res = json!({
            "verdict": if !ctx.config.resilience.targeted_single_faults_required { "not-required" } else if single_ok { "passed" } else { "failed" },
            "scope": "development split; exhaustive single neuron loss (physical removal) over the fault regions; sampled claims assessed at validation",
            "single_faults": faults, "tested": pop.len(),
        });
        let _ = (&low_p, &low_h);
        checked.push(json!({"removed": p.removed_neurons, "resilience": res["verdict"], "single_faults_ok": single_ok}));
        if single_ok {
            chosen = Some((i, low_h, mapping, res));
            break;
        }
    }
    let Some((ci, low_h, mapping, res)) = chosen else {
        let neg = json!({"schema": "aarnn-explorer-optimisation-v1", "source": source, "result": "negative", "reason": "no passing candidate satisfies the resilience profile", "checked": checked, "proposals": log});
        let h = ctx.project.put_json(&neg)?;
        return Ok(json!({"result": "negative", "object": h}));
    };
    let p = &log[ci];
    let cand = json!({
        "schema": "aarnn-explorer-candidate-v1", "source": source, "source_snapshot": snap_h, "suite_sha256": suite_h,
        "config_sha256": ctx.config_sha, "profile": ctx.config.name, "selection_rule": select,
        "removed_neurons": p.removed_neurons, "removed_synapses": p.removed_synapses,
        "snapshot_object": low_h, "mapping": mapping, "development": {"passed": p.passed, "cost": p.cost, "job": p.job},
        "intact_cost": icost, "development_resilience": res, "classification": "physical-reduction", "validated": false,
    });
    let ch = ctx.project.put_json(&cand)?;
    let name = format!("cand-{}", &ch[..12]);
    ctx.project.bind(RecordKind::Candidate, &name, &ch)?;
    ctx.project.set_alias("selected", &ch)?;
    let frontier: Vec<Value> = front.iter().map(|&i| json!({"removed_neurons": log[i].removed_neurons, "removed_synapses": log[i].removed_synapses, "cost": log[i].cost})).collect();
    let summary = json!({
        "schema": "aarnn-explorer-optimisation-v1", "source": source, "result": "candidate", "candidate": name, "candidate_hash": ch,
        "proposals_evaluated": used, "budget": budget, "inert_removed_first": inert, "inert_notes": inert_notes,
        "protected_learning_substrate": if protect_substrate { substrate.iter().cloned().collect::<Vec<_>>() } else { vec![] },
        "declared_protected": declared.iter().cloned().collect::<Vec<_>>(),
        "hypothesis_order": importance.iter().map(|(s, id, f)| json!({"id": id, "importance": s, "single_ablation_failed": f})).collect::<Vec<_>>(),
        "frontier": frontier, "resilience_checked": checked, "proposals": log,
        "intact_cost": icost, "selected_cost": p.cost,
    });
    let sh = ctx.project.put_json(&summary)?;
    ctx.project.bind(
        RecordKind::Report,
        &format!("optimisation-{}", &sh[..12]),
        &sh,
    )?;
    Ok(json!({
        "result": "candidate", "candidate": name, "candidate_hash": ch, "selected_alias": "selected",
        "removed_neurons": p.removed_neurons, "removed_synapses": p.removed_synapses,
        "intact_cost": icost, "selected_cost": p.cost, "proposals_evaluated": used, "frontier_size": front.len(),
        "development_resilience": res["verdict"], "report": sh,
    }))
}

/// Hidden cells the configuration declares out of the contract's scope
/// (`optimisation.protect_cell_types` prefixes, `protect_neurons` IDs).
pub fn declared_protection(
    topo: &Topology,
    o: &crate::config::Optimisation,
) -> Result<BTreeSet<String>> {
    for id in &o.protect_neurons {
        if topo.index_of(id).is_none() {
            return Err(ExplorerError::InvalidInput(format!(
                "optimisation.protect_neurons: unknown neuron {id:?}"
            )));
        }
    }
    Ok(topo
        .neurons
        .iter()
        .filter(|n| {
            n.role == Role::Hidden
                && (o.protect_neurons.contains(&n.id)
                    || o.protect_cell_types
                        .iter()
                        .any(|p| n.type_name.starts_with(p.as_str())))
        })
        .map(|n| n.id.clone())
        .collect())
}

/// Plastic synapses of a native snapshot: existing excitatory hidden ->
/// hidden synapses onto a cell of the plastic-cell mask, from a cell its
/// presynaptic mask allows (the runner's `PlasticCellMask` rule; only
/// recurrent hidden synapses learn). Requires an enabled mask: without one
/// every synapse would count as plastic.
pub fn plastic_synapses(topo: &Topology, snap: &Value) -> Result<Vec<String>> {
    let m = &snap["net"]["aarnn_plastic_cells"];
    if !m.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false) {
        return Err(ExplorerError::Unsupported(
            "no enabled aarnn_plastic_cells mask: the plastic synapses are not declared".into(),
        ));
    }
    let mask = |v: &Value| -> Option<Vec<bool>> {
        v.get(0)
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|x| x.as_bool().unwrap_or(false)).collect())
    };
    let post = mask(&m["hidden"]).unwrap_or_default();
    let pre = mask(&m["pre_hidden"]);
    let newborns = m["newborns"].as_bool().unwrap_or(false);
    let newborn_outputs = m["newborn_outputs"].as_bool().unwrap_or(false);
    let learns = |d: u32| post.get(d as usize).copied().unwrap_or(newborns);
    let pre_learns = |d: u32| match &pre {
        Some(p) if !p.is_empty() => p.get(d as usize).copied().unwrap_or(newborn_outputs),
        _ => true,
    };
    Ok(topo
        .synapses
        .iter()
        .filter(|s| {
            let (a, b) = (
                &topo.neurons[s.pre as usize],
                &topo.neurons[s.post as usize],
            );
            s.class == crate::model::SynClass::Chemical
                && s.weight > 0.0
                && a.role == Role::Hidden
                && b.role == Role::Hidden
                && learns(b.dense)
                && pre_learns(a.dense)
        })
        .map(|s| s.id.clone())
        .collect())
}

/// Hidden cells marked plastic (`aarnn_plastic_cells.hidden`), presynaptic
/// plastic (`pre_hidden`) or modulatory (`aarnn_modulatory_cells.hidden`).
pub fn learning_substrate(topo: &Topology, snap: &Value) -> BTreeSet<String> {
    let net = &snap["net"];
    let mask = |v: &Value| -> Vec<bool> {
        v.get(0)
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|x| x.as_bool().unwrap_or(false)).collect())
            .unwrap_or_default()
    };
    let on = |v: &Value| v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false);
    let mut masks = Vec::new();
    if on(&net["aarnn_plastic_cells"]) {
        masks.push(mask(&net["aarnn_plastic_cells"]["hidden"]));
        masks.push(mask(&net["aarnn_plastic_cells"]["pre_hidden"]));
    }
    if on(&net["aarnn_modulatory_cells"]) {
        masks.push(mask(&net["aarnn_modulatory_cells"]["hidden"]));
    }
    let core: BTreeSet<u32> = topo
        .neurons
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            n.role == Role::Hidden
                && masks
                    .iter()
                    .any(|m| m.get(n.dense as usize).copied().unwrap_or(false))
        })
        .map(|(i, _)| i as u32)
        .collect();
    // Learned-value readouts: hidden cells with a positive input from a
    // plastic/modulatory cell (e.g. STEER, RISE, FALL, V_SLOW read V). They
    // are silent until learning, so a frozen cold-start contract cannot see
    // them either (a G2 salamander reduction removed STEER, RISE and FALL).
    let readouts: BTreeSet<u32> = topo
        .synapses
        .iter()
        .filter(|s| {
            s.weight > 0.0
                && core.contains(&s.pre)
                && topo.neurons[s.post as usize].role == Role::Hidden
        })
        .map(|s| s.post)
        .collect();
    core.union(&readouts)
        .map(|&i| topo.neurons[i as usize].id.clone())
        .collect()
}

fn failed(v: &crate::tasks::SuiteVerdict) -> Vec<String> {
    v.tasks
        .iter()
        .filter(|t| !t.passed)
        .map(|t| t.task.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(n: u64, s: u64, passed: bool) -> Proposal {
        Proposal {
            removed_neurons: vec![],
            removed_synapses: vec![],
            passed,
            failed_tasks: vec![],
            cost: Cost {
                neurons: n,
                synapses: s,
                state_bytes: n * 10 + s,
                ..Default::default()
            },
            round: 0,
            job: String::new(),
        }
    }

    #[test]
    fn pareto_keeps_non_dominated_passing_candidates() {
        let dims = vec!["neurons".to_string(), "synapses".to_string()];
        let p = vec![
            prop(10, 10, true),
            prop(8, 12, true),
            prop(9, 9, true),
            prop(5, 5, false),
            prop(9, 11, true),
        ];
        let f = pareto(&p, &dims);
        assert_eq!(f, vec![1, 2], "failing candidates never enter the frontier");
    }
}
