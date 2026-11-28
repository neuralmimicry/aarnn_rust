// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Functional contracts: task definitions, trial distributions, measurement
//! and oracles (brief Section 10). Simulation and evaluation are separate:
//! `exec` produces output series, and this module turns them into measures
//! and verdicts.
//!
//! - Every required task must meet its own threshold. There is no weighted
//!   average.
//! - A criterion against the reference uses an absolute floor whenever the
//!   reference measure is zero or negative, so a silent reference cannot let
//!   an empty candidate pass.
//! - Seeds are split into development, locked-validation and optional
//!   challenge sets. Searches only ever see development seeds.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ExplorerError, Result, invalid};
use crate::exec::{self, RunOutput, RunSpec};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskSuite {
    pub schema_version: u32,
    pub name: String,
    pub splits: Splits,
    #[serde(rename = "task")]
    pub tasks: Vec<TaskDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Splits {
    pub development: Vec<u64>,
    pub locked_validation: Vec<u64>,
    #[serde(default)]
    pub biological_challenge: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskDef {
    pub id: String,
    /// discrimination | timing | memory | learning | closed-loop | quiescence
    pub family: String,
    #[serde(default = "yes")]
    pub required: bool,
    pub duration_ms: u32,
    #[serde(default)]
    pub stimulus: Vec<Segment>,
    /// Common onset jitter (+- ms, in whole 2 ms frames) applied to
    /// `jitter = true` segments.
    #[serde(default)]
    pub jitter_ms: u32,
    /// Amplitude scaled by U(1 - a, 1) per trial.
    #[serde(default)]
    pub amplitude_jitter: f64,
    #[serde(default)]
    pub background_p: f64,
    #[serde(default, rename = "measure")]
    pub measures: Vec<Measure>,
    #[serde(default, rename = "criterion")]
    pub criteria: Vec<Criterion>,
    #[serde(default)]
    pub plant: Option<PlantDef>,
    /// Fraction of trials that must succeed (default all).
    #[serde(default = "one")]
    pub min_success: f64,
    /// Capabilities the task needs; an unavailable required capability fails
    /// admission instead of passing.
    #[serde(default)]
    pub requires: Vec<String>,
}

/// Finite stand-in for "no response within the measurement window".
pub const NO_RESPONSE: f64 = 1.0e9;

fn yes() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub channel: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub value: f64,
    #[serde(default)]
    pub jitter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    pub name: String,
    /// Output name, or `plant.<field>` for closed-loop tasks.
    pub output: String,
    #[serde(default)]
    pub start_ms: u32,
    #[serde(default)]
    pub end_ms: u32,
    /// spikes | mean | max | latency_ms | plant_final | plant_settle_ms
    pub stat: String,
    /// latency: activation level counted as a response.
    #[serde(default)]
    pub level: f64,
    /// latency: measured from this segment's (jittered) onset.
    #[serde(default)]
    pub from_channel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub measure: String,
    /// `>=`, `<=`, `==` against `value`; `within-reference` (|candidate -
    /// reference| <= amount x |reference| + value); or `ratio-of-reference` (candidate
    /// >= amount x reference, with `value` as the absolute floor when the
    /// reference is <= 0); or `reference-plus` (candidate <= reference +
    /// amount).
    pub op: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub amount: f64,
}

/// Synthetic closed-loop heading plant. The authored plant is not an
/// embodiment adapter and makes no claim about a body. Error e (degrees):
/// the left sensor reads clamp(e/90), the right sensor clamp(-e/90), and the
/// left/right outputs turn by `gain_deg` per unit activation per frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlantDef {
    pub kind: String,
    pub left_channel: String,
    pub right_channel: String,
    pub left_output: String,
    pub right_output: String,
    pub gain_deg: f64,
    pub initial_error_deg: [f64; 2],
    pub tolerance_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrialResult {
    pub task: String,
    pub seed: u64,
    pub success: bool,
    pub measures: BTreeMap<String, f64>,
    pub failed_criteria: Vec<String>,
    pub output_events: u64,
    pub digest: String,
    pub step_ns_p50: u64,
    pub step_ns_p95: u64,
    pub step_ns_p99: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskVerdict {
    pub task: String,
    pub family: String,
    pub required: bool,
    pub trials: u64,
    pub successes: u64,
    pub passed: bool,
    /// Mean of each measure over trials.
    pub mean_measures: BTreeMap<String, f64>,
    /// Measures of each trial, by seed (paired reference for candidates).
    #[serde(default)]
    pub trial_measures: BTreeMap<u64, BTreeMap<String, f64>>,
    pub failures: Vec<TrialResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuiteVerdict {
    pub suite: String,
    pub split: String,
    pub passed: bool,
    /// The candidate produced no output events in any response task.
    pub quiescent: bool,
    pub tasks: Vec<TaskVerdict>,
    pub unavailable: Vec<String>,
    pub step_ns_p50: u64,
    pub step_ns_p95: u64,
    pub step_ns_p99: u64,
}

impl TaskSuite {
    pub fn parse(text: &str) -> Result<TaskSuite> {
        let s: TaskSuite = toml::from_str(text)
            .map_err(|e| ExplorerError::InvalidInput(format!("task suite: {e}")))?;
        s.validate()?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return invalid("task suite schema_version must be 1");
        }
        let (d, v) = (&self.splits.development, &self.splits.locked_validation);
        if d.is_empty() || v.is_empty() {
            return invalid("splits.development and splits.locked_validation must be non-empty");
        }
        if d.iter()
            .any(|x| v.contains(x) || self.splits.biological_challenge.contains(x))
            || v.iter()
                .any(|x| self.splits.biological_challenge.contains(x))
        {
            return invalid("splits must be disjoint (a seed used for selection cannot validate)");
        }
        let fams = [
            "discrimination",
            "timing",
            "memory",
            "learning",
            "closed-loop",
            "quiescence",
        ];
        let mut ids = std::collections::BTreeSet::new();
        for t in &self.tasks {
            if !ids.insert(&t.id) {
                return invalid(format!("duplicate task id {}", t.id));
            }
            if !fams.contains(&t.family.as_str()) {
                return invalid(format!("task {}: family {:?}", t.id, t.family));
            }
            if t.duration_ms < 2 || !(0.0..=1.0).contains(&t.min_success) {
                return invalid(format!(
                    "task {}: duration_ms >= 2 and min_success in [0, 1]",
                    t.id
                ));
            }
            if t.criteria.is_empty() {
                return invalid(format!("task {}: at least one criterion", t.id));
            }
            for c in &t.criteria {
                if !t.measures.iter().any(|m| m.name == c.measure) {
                    return invalid(format!(
                        "task {}: criterion on unknown measure {}",
                        t.id, c.measure
                    ));
                }
                if ![
                    "<=",
                    ">=",
                    "==",
                    "ratio-of-reference",
                    "reference-plus",
                    "within-reference",
                ]
                .contains(&c.op.as_str())
                {
                    return invalid(format!("task {}: criterion op {:?}", t.id, c.op));
                }
            }
            for m in &t.measures {
                if ![
                    "spikes",
                    "mean",
                    "max",
                    "latency_ms",
                    "plant_final",
                    "plant_settle_ms",
                ]
                .contains(&m.stat.as_str())
                {
                    return invalid(format!("task {}: measure stat {:?}", t.id, m.stat));
                }
            }
            if t.family == "closed-loop" && t.plant.is_none() && t.requires.is_empty() {
                return invalid(format!(
                    "task {}: closed-loop tasks need a plant or an adapter capability",
                    t.id
                ));
            }
        }
        Ok(())
    }

    pub fn seeds(&self, split: &str) -> Result<&[u64]> {
        match split {
            "development" => Ok(&self.splits.development),
            "locked-validation" => Ok(&self.splits.locked_validation),
            "biological-challenge" => Ok(&self.splits.biological_challenge),
            s => invalid(format!("split {s:?}")),
        }
    }
}

struct Io {
    sensory: Vec<String>,
    outputs: Vec<String>,
    /// Physical channel names (designed embodiment circuits), same order.
    physical_sensory: Vec<String>,
    physical_outputs: Vec<String>,
}

fn io_of(snapshot: &Value) -> Io {
    let l = &snapshot["connectome_labels"];
    let names = |k: &str| -> Vec<String> {
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
    Io {
        sensory: names("sensory_nodes"),
        outputs: names("output_nodes"),
        physical_sensory: names("physical_sensory_nodes"),
        physical_outputs: names("physical_output_nodes"),
    }
}

/// Channel lookup by stable (neural) name, then by physical name.
fn chan(io: &Io, name: &str) -> Result<usize> {
    io.sensory
        .iter()
        .position(|s| s == name)
        .or_else(|| io.physical_sensory.iter().position(|s| s == name))
        .ok_or_else(|| ExplorerError::InvalidInput(format!("unknown sensory channel {name:?}")))
}

fn outp(io: &Io, name: &str) -> Result<usize> {
    io.outputs
        .iter()
        .position(|s| s == name)
        .or_else(|| io.physical_outputs.iter().position(|s| s == name))
        .ok_or_else(|| ExplorerError::InvalidInput(format!("unknown output {name:?}")))
}

/// Run one trial of a task and measure it.
pub fn run_trial(
    snapshot: &Value,
    task: &TaskDef,
    seed: u64,
    policy: &str,
    reference: Option<&BTreeMap<String, f64>>,
    record_events: bool,
    graded_events: bool,
) -> Result<(TrialResult, RunOutput)> {
    run_trial_silenced(
        snapshot,
        task,
        seed,
        policy,
        reference,
        record_events,
        graded_events,
        &[],
    )
}

/// As `run_trial`, with transient silencing windows (interventions).
#[allow(clippy::too_many_arguments)]
pub fn run_trial_silenced(
    snapshot: &Value,
    task: &TaskDef,
    seed: u64,
    policy: &str,
    reference: Option<&BTreeMap<String, f64>>,
    record_events: bool,
    graded_events: bool,
    silence: &[crate::exec::Silence],
) -> Result<(TrialResult, RunOutput)> {
    let io = io_of(snapshot);
    let frames = (task.duration_ms as usize).div_ceil(2);
    let mut rng = fastrand::Rng::with_seed(seed ^ 0x5eed_7a5c);
    // Onset jitter in whole 2 ms frames, so a pulse keeps its frame count
    // (an odd-ms shift would split a 2 ms pulse over two frames and double
    // its spikes).
    let shift: i64 = if task.jitter_ms >= 2 {
        2 * rng.i64(-(task.jitter_ms as i64 / 2)..=(task.jitter_ms as i64 / 2))
    } else {
        0
    };
    let amp = 1.0 - rng.f64() * task.amplitude_jitter.clamp(0.0, 1.0);
    let mut onset: BTreeMap<String, f64> = BTreeMap::new();
    let mut grid = vec![vec![0.0f32; io.sensory.len()]; frames];
    for seg in &task.stimulus {
        let c = chan(&io, &seg.channel)?;
        let off = if seg.jitter { shift } else { 0 };
        let a = (seg.start_ms as i64 + off).max(0) as usize / 2;
        let b = ((seg.end_ms as i64 + off).max(0) as usize)
            .div_ceil(2)
            .min(frames);
        onset.entry(seg.channel.clone()).or_insert(a as f64 * 2.0);
        for row in grid.iter_mut().take(b).skip(a) {
            row[c] = (seg.value * amp).clamp(0.0, 1.0) as f32;
        }
    }
    let spec = RunSpec {
        seed,
        frames: Vec::new(),
        background_p: task.background_p,
        noise_seed: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15),
        record_events,
        graded_events,
        plasticity_policy: policy.to_string(),
        silence: silence.to_vec(),
    };
    // Plant state (closed loop).
    let mut err_trace: Vec<f64> = Vec::new();
    let out = if let Some(p) = &task.plant {
        if p.kind != "heading" {
            return Err(ExplorerError::Unsupported(format!(
                "plant kind {:?}",
                p.kind
            )));
        }
        let (cl, cr) = (chan(&io, &p.left_channel)?, chan(&io, &p.right_channel)?);
        let (ol, or) = (outp(&io, &p.left_output)?, outp(&io, &p.right_output)?);
        let mag =
            p.initial_error_deg[0] + rng.f64() * (p.initial_error_deg[1] - p.initial_error_deg[0]);
        let mut e = if rng.bool() { mag } else { -mag };
        let gain = p.gain_deg;
        let n = io.sensory.len();
        let mut input = |f: usize, last: &[f32]| -> Vec<f32> {
            if f > 0 {
                e -= gain
                    * (last.get(ol).copied().unwrap_or(0.0) as f64
                        - last.get(or).copied().unwrap_or(0.0) as f64);
            }
            err_trace.push(e);
            let mut v = grid.get(f).cloned().unwrap_or_else(|| vec![0.0; n]);
            v[cl] = (e / 90.0).clamp(0.0, 1.0) as f32;
            v[cr] = (-e / 90.0).clamp(0.0, 1.0) as f32;
            v
        };
        exec::run_with(snapshot, &spec, frames, &mut input)?
    } else {
        let g = grid.clone();
        exec::run_with(snapshot, &spec, frames, &mut |f, _| g[f].clone())?
    };
    let mut measures = BTreeMap::new();
    for m in &task.measures {
        let v = if m.stat.starts_with("plant") {
            let tol = task.plant.as_ref().map(|p| p.tolerance_deg).unwrap_or(0.0);
            match m.stat.as_str() {
                "plant_final" => err_trace.last().map(|e| e.abs()).unwrap_or(f64::NAN),
                _ => {
                    // Time after which |e| stays within tolerance.
                    let last_out = err_trace.iter().rposition(|e| e.abs() > tol);
                    match last_out {
                        None => 0.0,
                        Some(i) if i + 1 >= err_trace.len() => f64::INFINITY,
                        Some(i) => (i + 1) as f64 * 2.0,
                    }
                }
            }
        } else {
            let k = outp(&io, &m.output)?;
            let a = (m.start_ms as usize / 2).min(out.frames);
            let b = if m.end_ms == 0 {
                out.frames
            } else {
                (m.end_ms as usize).div_ceil(2).min(out.frames)
            };
            let vals: Vec<f64> = out.output_series[a..b]
                .iter()
                .map(|r| r[k] as f64)
                .collect();
            match m.stat.as_str() {
                // spikes: per frame, 0.5 per spike step (exec), so x2.
                "spikes" => vals.iter().sum::<f64>() * 2.0,
                "mean" => vals.iter().sum::<f64>() / vals.len().max(1) as f64,
                "max" => vals.iter().cloned().fold(0.0, f64::max),
                _ => {
                    let from = m
                        .from_channel
                        .as_ref()
                        .and_then(|c| onset.get(c))
                        .copied()
                        .unwrap_or(a as f64 * 2.0);
                    let start = (from / 2.0) as usize;
                    let lvl = if m.level > 0.0 { m.level } else { 0.25 };
                    out.output_series
                        .iter()
                        .enumerate()
                        .skip(start.max(a))
                        .take(b.saturating_sub(start.max(a)))
                        .find(|(_, r)| r[k] as f64 >= lvl)
                        .map(|(f, _)| f as f64 * 2.0 - from)
                        .unwrap_or(f64::INFINITY)
                }
            }
        };
        // JSON has no infinity: "no response within the window" (latency,
        // settling) and undefined values are stored as the finite sentinel
        // NO_RESPONSE, so records round-trip exactly.
        measures.insert(m.name.clone(), if v.is_finite() { v } else { NO_RESPONSE });
    }
    // Without an external reference the run is its own reference (it defines
    // the intact behaviour); absolute criteria and the quiescence check still
    // apply. Candidates are always evaluated against the intact reference.
    let own = measures.clone();
    let reference = Some(reference.unwrap_or(&own));
    let mut failed = Vec::new();
    for c in &task.criteria {
        let x = measures[&c.measure];
        let ok = match c.op.as_str() {
            ">=" => x >= c.value.unwrap_or(0.0),
            "<=" => x <= c.value.unwrap_or(0.0),
            "==" => (x - c.value.unwrap_or(0.0)).abs() < 1e-9,
            "within-reference" => {
                // |candidate - reference| <= amount x |reference| + value:
                // output equivalence within a declared tolerance. Without a
                // reference the criterion cannot pass.
                match reference.and_then(|r| r.get(&c.measure)).copied() {
                    Some(r) => (x - r).abs() <= c.amount * r.abs() + c.value.unwrap_or(0.0),
                    None => false,
                }
            }
            "ratio-of-reference" => {
                // `value` is an absolute floor that always applies; the ratio
                // applies only to a positive reference. With neither a
                // positive reference nor a floor the criterion cannot pass.
                let floor_ok = c.value.is_none_or(|f| x >= f);
                match reference.and_then(|r| r.get(&c.measure)).copied() {
                    Some(r) if r > 0.0 => floor_ok && x >= c.amount * r,
                    _ => c.value.is_some() && floor_ok,
                }
            }
            _ => {
                // reference-plus: candidate <= reference + amount, and never
                // a missing response; an absolute `value` cap always applies.
                let cap_ok = c.value.is_none_or(|v| x <= v);
                match reference.and_then(|r| r.get(&c.measure)).copied() {
                    Some(r) if r < NO_RESPONSE => x < NO_RESPONSE && x <= r + c.amount && cap_ok,
                    _ => x < NO_RESPONSE && cap_ok,
                }
            }
        };
        if !ok {
            failed.push(format!(
                "{} {} {:?} (measured {x})",
                c.measure,
                c.op,
                c.value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| format!("amount {}", c.amount))
            ));
        }
    }
    let output_events = out
        .output_series
        .iter()
        .flatten()
        .filter(|&&v| v > 0.0)
        .count() as u64;
    Ok((
        TrialResult {
            task: task.id.clone(),
            seed,
            success: failed.is_empty(),
            measures,
            failed_criteria: failed,
            output_events,
            digest: out.digest.clone(),
            step_ns_p50: out.step_ns_p50,
            step_ns_p95: out.step_ns_p95,
            step_ns_p99: out.step_ns_p99,
        },
        out,
    ))
}

/// Evaluate a suite split. `reference` holds per-task mean measures of the
/// intact reference (for relative criteria); `available` lists capabilities.
pub fn evaluate(
    snapshot: &Value,
    suite: &TaskSuite,
    split: &str,
    policy: &str,
    reference: Option<&BTreeMap<String, BTreeMap<String, f64>>>,
    available: &[String],
) -> Result<SuiteVerdict> {
    evaluate_silenced(snapshot, suite, split, policy, reference, available, &[])
}

/// As `evaluate`, with transient silencing windows applied to every trial.
pub fn evaluate_silenced(
    snapshot: &Value,
    suite: &TaskSuite,
    split: &str,
    policy: &str,
    reference: Option<&BTreeMap<String, BTreeMap<String, f64>>>,
    available: &[String],
    silence: &[crate::exec::Silence],
) -> Result<SuiteVerdict> {
    let seeds = suite.seeds(split)?.to_vec();
    let mut tasks = Vec::new();
    let mut unavailable = Vec::new();
    let mut ns: Vec<(u64, u64, u64)> = Vec::new();
    let mut response_events = 0u64;
    let mut response_tasks = 0usize;
    for t in &suite.tasks {
        if let Some(missing) = t.requires.iter().find(|c| !available.contains(c)) {
            unavailable.push(format!(
                "{}: requires unavailable capability {missing}",
                t.id
            ));
            tasks.push(TaskVerdict {
                task: t.id.clone(),
                family: t.family.clone(),
                required: t.required,
                trials: 0,
                successes: 0,
                passed: !t.required,
                mean_measures: BTreeMap::new(),
                trial_measures: BTreeMap::new(),
                failures: Vec::new(),
            });
            continue;
        }
        let mut successes = 0u64;
        let mut sums: BTreeMap<String, f64> = BTreeMap::new();
        let mut failures = Vec::new();
        let mut trial_measures = BTreeMap::new();
        for &seed in &seeds {
            // Paired reference: the intact trial with the same seed (same
            // jitter and noise), falling back to the intact task means.
            let rref =
                reference.and_then(|r| r.get(&format!("{}@{seed}", t.id)).or_else(|| r.get(&t.id)));
            let (tr, _) =
                run_trial_silenced(snapshot, t, seed, policy, rref, false, false, silence)?;
            trial_measures.insert(seed, tr.measures.clone());
            if tr.success {
                successes += 1;
            } else if failures.len() < 8 {
                failures.push(tr.clone());
            }
            for (k, v) in &tr.measures {
                *sums.entry(k.clone()).or_insert(0.0) += v;
            }
            if t.family != "quiescence" {
                response_events += tr.output_events;
            }
            ns.push((tr.step_ns_p50, tr.step_ns_p95, tr.step_ns_p99));
        }
        if t.family != "quiescence" {
            response_tasks += 1;
        }
        let n = seeds.len() as u64;
        let mean_measures = sums.into_iter().map(|(k, v)| (k, v / n as f64)).collect();
        tasks.push(TaskVerdict {
            task: t.id.clone(),
            family: t.family.clone(),
            required: t.required,
            trials: n,
            successes,
            passed: successes as f64 >= t.min_success * n as f64,
            mean_measures,
            trial_measures,
            failures,
        });
    }
    let quiescent = response_tasks > 0 && response_events == 0;
    let mut p50: Vec<u64> = ns.iter().map(|x| x.0).collect();
    let mut p95: Vec<u64> = ns.iter().map(|x| x.1).collect();
    let mut p99: Vec<u64> = ns.iter().map(|x| x.2).collect();
    let med = |v: &mut Vec<u64>| {
        v.sort_unstable();
        v.get(v.len() / 2).copied().unwrap_or(0)
    };
    Ok(SuiteVerdict {
        suite: suite.name.clone(),
        split: split.to_string(),
        passed: !quiescent
            && tasks.iter().all(|t| !t.required || t.passed)
            && unavailable.iter().all(|u| {
                let id = u.split(':').next().unwrap_or("");
                !suite.tasks.iter().any(|t| t.id == id && t.required)
            }),
        quiescent,
        tasks,
        unavailable,
        step_ns_p50: med(&mut p50),
        step_ns_p95: med(&mut p95),
        step_ns_p99: med(&mut p99),
    })
}

/// Reference for relative criteria: per-task mean measures (key `task`)
/// plus each trial's measures (key `task@seed`) for paired comparison.
pub fn reference_measures(v: &SuiteVerdict) -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut out: BTreeMap<String, BTreeMap<String, f64>> = v
        .tasks
        .iter()
        .map(|t| (t.task.clone(), t.mean_measures.clone()))
        .collect();
    for t in &v.tasks {
        for (seed, m) in &t.trial_measures {
            out.insert(format!("{}@{seed}", t.task), m.clone());
        }
    }
    out
}
