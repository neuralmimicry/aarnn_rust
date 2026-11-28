// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Controller: the command implementations over the project store and
//! bounded worker processes. Every experiment executes in a worker; the
//! controller only schedules, records and analyses.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::capture;
use crate::config::ExplorerConfig;
use crate::error::{ExplorerError, Result, invalid};
use crate::gnat::{self, Bounds, GnatParams, Norm};
use crate::lesion::{self, Intervention, Protection, Selection};
use crate::model::{self, Role, Topology};
use crate::motifs;
use crate::store::{Project, RecordKind, sha256_hex};
use crate::tasks::{SuiteVerdict, TaskSuite};
use crate::worker::{self, Disposition, Job, JobKind, Limits};

/// Bump when worker execution semantics change (job results are reused by
/// key). 2026-11-27-b: transient silencing reaches evaluation runs; paired
/// same-seed references.
pub const SEMANTICS_VERSION: &str = "2026-11-27-b";

pub const DEFAULT_CONFIG: &str = include_str!("../../../configs/explorer/default.toml");
pub const DEFAULT_SUITE: &str =
    include_str!("../../../examples/explorer/tasks/synthetic_sensorimotor.toml");

pub struct Ctx {
    pub project: Project,
    pub config: ExplorerConfig,
    pub config_sha: String,
    pub exe: PathBuf,
    pub limits: Limits,
    pub enforced: bool,
    /// Set to stop admission of new workers and terminate running ones.
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Ctx {
    pub fn new(project: Project, config_path: Option<&Path>) -> Result<Ctx> {
        let text = match config_path {
            Some(p) => std::fs::read_to_string(p)
                .map_err(|e| ExplorerError::InvalidInput(format!("{}: {e}", p.display())))?,
            None => DEFAULT_CONFIG.to_string(),
        };
        let config = ExplorerConfig::parse(&text)?;
        let r = &config.resources;
        let limits = Limits {
            threads_per_worker: r.threads_per_worker,
            memory_limit_mib: r.memory_limit_mib / r.max_worker_processes.max(1) as u64,
            wall_timeout: Duration::from_secs(r.worker_wall_timeout_seconds),
            os_enforcement: std::env::var_os("AARNN_EXPLORER_NO_OS_LIMITS").is_none(),
        };
        let enforced = limits.os_enforcement && worker::systemd_scope_available();
        let exe = std::env::var_os("AARNN_EXPLORER_EXE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .ok_or_else(|| {
                ExplorerError::Internal("cannot locate the explorer binary for workers".into())
            })?;
        Ok(Ctx {
            project,
            config_sha: sha256_hex(text.as_bytes()),
            config,
            exe,
            limits,
            enforced,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    pub fn suite(&self) -> Result<(TaskSuite, String)> {
        let path = &self.config.functional.task_suite;
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) if path == "examples/explorer/tasks/synthetic_sensorimotor.toml" => {
                DEFAULT_SUITE.to_string()
            }
            Err(e) => return invalid(format!("task suite {path}: {e}")),
        };
        let suite = TaskSuite::parse(&text)?;
        let h = self.project.put(text.as_bytes())?;
        Ok((suite, h))
    }

    fn source_record(&self, name: &str) -> Result<Value> {
        let h = self.project.lookup(RecordKind::Source, name)?;
        self.project.get_json(&h)
    }

    pub fn default_source(&self, given: Option<&str>) -> Result<String> {
        match given {
            Some(s) => Ok(s.to_string()),
            None => self.project.default_value("source")?.ok_or_else(|| {
                ExplorerError::InvalidInput(
                    "no --source given and the project has no default source (run import)".into(),
                )
            }),
        }
    }

    /// Executable snapshot object of a source (hash, path, topology).
    fn executable(&self, source: &str) -> Result<(String, PathBuf, Topology)> {
        let rec = self.source_record(source)?;
        let snap = rec["snapshot_object"].as_str().ok_or_else(|| {
            ExplorerError::Unsupported(format!(
                "source {source} is {} (no executable dynamics)",
                rec["capability"]
            ))
        })?;
        let topo: Topology = self
            .project
            .get_json(rec["topology_object"].as_str().unwrap_or(""))?;
        Ok((snap.to_string(), self.project.object_file(snap)?, topo))
    }

    /// Run jobs on at most `max_worker_processes` concurrent workers.
    /// Results are returned in submission order, independent of completion
    /// order. Successful results are reused by job id (idempotent).
    pub fn run_jobs(&self, mut jobs: Vec<Job>) -> Result<Vec<(Job, Disposition)>> {
        for j in jobs.iter_mut() {
            let mut key = serde_json::to_value(&*j)?;
            key["job_id"] = Value::Null;
            // Reuse is keyed on execution semantics too: a fix that changes
            // what a job computes must not reuse results computed before it.
            key["semantics"] = Value::from(SEMANTICS_VERSION);
            j.job_id = format!("job-{}", &sha256_hex(key.to_string().as_bytes())[..20]);
        }
        let n = jobs.len();
        let out: Arc<Mutex<Vec<Option<Disposition>>>> = Arc::new(Mutex::new(vec![None; n]));
        let next = Arc::new(Mutex::new(0usize));
        let workers = (self.config.resources.max_worker_processes as usize)
            .max(1)
            .min(n.max(1));
        let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let (out, next, jobs, errors) = (out.clone(), next.clone(), &jobs, errors.clone());
                scope.spawn(move || {
                    loop {
                        let i = {
                            let mut g = next.lock().expect("lock");
                            if *g >= n {
                                break;
                            }
                            *g += 1;
                            *g - 1
                        };
                        match self.run_one(&jobs[i]) {
                            Ok(d) => out.lock().expect("lock")[i] = Some(d),
                            Err(e) => errors
                                .lock()
                                .expect("lock")
                                .push(format!("{}: {e}", jobs[i].job_id)),
                        }
                    }
                });
            }
        });
        let errs = errors.lock().expect("lock").clone();
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(ExplorerError::Cancelled(
                "cancelled; completed job results are kept and reused on resume".into(),
            ));
        }
        if !errs.is_empty() {
            return Err(ExplorerError::Internal(errs.join("; ")));
        }
        let out = Arc::try_unwrap(out)
            .map_err(|_| ExplorerError::Internal("worker results still shared".into()))?
            .into_inner()
            .expect("lock");
        Ok(jobs
            .into_iter()
            .zip(out.into_iter().map(|d| d.expect("every job ran")))
            .collect())
    }

    fn run_one(&self, job: &Job) -> Result<Disposition> {
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            self.project.journal(&json!({"job": job.job_id, "state": "cancelled", "t": now_s(), "reason": "not admitted after cancellation"}))?;
            return Err(ExplorerError::Cancelled(format!(
                "job {} not started",
                job.job_id
            )));
        }
        let dir = self.project.runs_dir().join(&job.job_id);
        let result_path = dir.join("result.json");
        if let Ok(b) = std::fs::read(&result_path) {
            if let Ok(r) = serde_json::from_slice::<worker::JobResult>(&b) {
                if r.status == "succeeded" && r.source_unchanged {
                    self.project
                        .journal(&json!({"job": job.job_id, "state": "reused", "t": now_s()}))?;
                    return Ok(Disposition {
                        outcome: "succeeded".into(),
                        exit: Some(0),
                        signal: None,
                        enforcement: "reused".into(),
                        wall_ms: 0.0,
                        result: Some(r),
                        stderr_tail: String::new(),
                    });
                }
            }
        }
        std::fs::create_dir_all(&dir)?;
        let job_path = dir.join("job.json");
        crate::store::atomic_write(
            &self.project.tmp_dir(),
            &job_path,
            serde_json::to_string(job)?.as_bytes(),
        )?;
        let _ = std::fs::remove_file(&result_path);
        self.project.journal(
            &json!({"job": job.job_id, "state": "running", "t": now_s(), "kind": job.job}),
        )?;
        let d = worker::spawn_cancellable(
            &self.exe,
            &job_path,
            &result_path,
            &self.limits,
            self.enforced,
            Some(&self.cancel),
        )?;
        let state = match d.outcome.as_str() {
            "succeeded" => "succeeded",
            "cancelled" => "cancelled",
            "timeout" => "failed:timeout",
            "killed" => "failed:killed",
            _ => "failed",
        };
        self.project.journal(&json!({"job": job.job_id, "state": state, "t": now_s(), "wall_ms": d.wall_ms, "enforcement": d.enforcement}))?;
        if let Some(r) = &d.result {
            if !r.source_unchanged {
                return Err(ExplorerError::Internal(format!(
                    "job {}: the source object changed during the experiment",
                    job.job_id
                )));
            }
        }
        Ok(d)
    }

    pub fn job(
        &self,
        snapshot_sha: &str,
        snapshot_path: &Path,
        suite: &TaskSuite,
        kind: JobKind,
    ) -> Job {
        Job {
            schema: worker::JOB_SCHEMA.into(),
            job_id: String::new(),
            snapshot_path: snapshot_path.to_path_buf(),
            snapshot_sha256: snapshot_sha.into(),
            suite: suite.clone(),
            policy: self.config.execution.plasticity_policy.clone(),
            interventions: Vec::new(),
            lower_neurons: Vec::new(),
            lower_synapses: Vec::new(),
            protect_io: self.config.optimisation.protect_input_output_mapping,
            capabilities: Vec::new(),
            job: kind,
        }
    }
}

/// Extract the suite verdict and cost from a disposition.
pub fn verdict_of(d: &Disposition) -> Result<(SuiteVerdict, lesion::Cost, Value)> {
    let r = d.result.as_ref().ok_or_else(|| {
        ExplorerError::Internal(format!("worker {}: {}", d.outcome, d.stderr_tail))
    })?;
    if r.status != "succeeded" {
        return Err(ExplorerError::Internal(format!(
            "worker failed: {}",
            r.error.clone().unwrap_or_default()
        )));
    }
    let v: SuiteVerdict = serde_json::from_value(r.output["verdict"].clone())?;
    let mut c: lesion::Cost = serde_json::from_value(r.output["cost"].clone())?;
    c.latency_ns_p50 = Some(v.step_ns_p50);
    Ok((v, c, r.output["meta"].clone()))
}

// --- import -------------------------------------------------------------

pub fn import(ctx: &Ctx, format: &str, input: &Path, name: &str) -> Result<Value> {
    let meta = std::fs::metadata(input)
        .map_err(|e| ExplorerError::InvalidInput(format!("{}: {e}", input.display())))?;
    if meta.len() > crate::store::MAX_OBJECT_BYTES {
        return Err(ExplorerError::Budget(format!(
            "{} is {} bytes (limit {})",
            input.display(),
            meta.len(),
            crate::store::MAX_OBJECT_BYTES
        )));
    }
    let raw = std::fs::read_to_string(input)?;
    let input_sha = sha256_hex(raw.as_bytes());
    let (topo, snapshot) = match format {
        "aarnn-snapshot" => {
            let (t, v) = model::from_native(&raw)?;
            (t, Some(v))
        }
        "sparse" => model::from_sparse(&raw, 200_000, 100_000_000)?,
        f => return invalid(format!("--format {f:?}: aarnn-snapshot or sparse")),
    };
    let raw_h = ctx.project.put(raw.as_bytes())?;
    let topo_h = ctx.project.put_json(&topo)?;
    let snap_h = match &snapshot {
        Some(s) => Some(ctx.project.put(s.to_string().as_bytes())?),
        None => None,
    };
    let rec = json!({
        "schema": "aarnn-explorer-source-v1",
        "name": name, "format": format,
        "input_file": input.display().to_string(), "input_sha256": input_sha,
        "raw_object": raw_h, "topology_object": topo_h, "snapshot_object": snap_h,
        "capability": topo.capability, "summary": topo.summary(),
        "assumptions": topo.assumptions, "limitations": topo.limitations, "model_flags": topo.model_flags,
        "tool_version": env!("CARGO_PKG_VERSION"),
    });
    let h = ctx.project.put_json(&rec)?;
    ctx.project.bind(RecordKind::Source, name, &h)?;
    if ctx.project.default_value("source")?.is_none() {
        ctx.project.set_default("source", name)?;
    }
    Ok(
        json!({"source": name, "record": h, "capability": topo.capability, "summary": topo.summary()}),
    )
}

// --- derive -------------------------------------------------------------

/// What `derive` builds a new source from.
#[derive(Debug, Default, Clone)]
pub struct DeriveSpec<'a> {
    /// `learned-cap`: every plastic synapse (optimise::plastic_synapses) at
    /// the network's plasticity cap, a fully learned state. A contract run
    /// on it exercises the learned pathways (value readouts, hunger gate)
    /// that stay silent in a naive, frozen network.
    pub preset: Option<&'a str>,
    /// An `aarnn-explorer-plan-v1` file (or a bare array) of persistent
    /// interventions; transient silencing is refused.
    pub interventions: Option<&'a Path>,
    /// Physically remove a candidate's removal set (by stable ID), e.g. to
    /// carry a reduction found on a learned-state source back to the naive
    /// source that learns by experience.
    pub lower_like: Option<&'a str>,
    /// Also write the derived native snapshot here.
    pub write: Option<&'a Path>,
}

/// Derive a new source from an existing one: a persistent state change
/// and/or a candidate's removal set, recorded with its provenance. The
/// original source is never modified.
pub fn derive(ctx: &Ctx, source: Option<&str>, name: &str, spec: &DeriveSpec) -> Result<Value> {
    let source = ctx.default_source(source)?;
    let rec = ctx.source_record(&source)?;
    let (snap_h, _, topo) = ctx.executable(&source)?;
    let snap: Value = ctx.project.get_json(&snap_h)?;
    let mut ivs: Vec<Intervention> = Vec::new();
    match spec.preset {
        None => {}
        Some("learned-cap") => {
            let cap = snap["net"]["aarnn_plasticity_w_max"]
                .as_f64()
                .unwrap_or(0.0);
            if cap <= 0.0 {
                return Err(ExplorerError::Unsupported(
                    "learned-cap: the source sets no aarnn_plasticity_w_max".into(),
                ));
            }
            let ids = crate::optimise::plastic_synapses(&topo, &snap)?;
            if ids.is_empty() {
                return invalid("learned-cap: the source has no plastic synapses");
            }
            ivs.push(Intervention {
                id: "learned-cap".into(),
                selection: Selection::Synapses { ids },
                action: lesion::Action::WeightSet { value: cap },
            });
        }
        Some(p) => return invalid(format!("--preset {p:?}: learned-cap")),
    }
    if let Some(path) = spec.interventions {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        let list = if v.is_array() {
            v
        } else {
            v["interventions"].clone()
        };
        ivs.extend(serde_json::from_value::<Vec<Intervention>>(list)?);
    }
    if let Some(iv) = ivs
        .iter()
        .find(|iv| matches!(iv.action, lesion::Action::Silence { .. }))
    {
        return invalid(format!(
            "{}: silencing is transient; a derived source holds persistent state only",
            iv.id
        ));
    }
    if ivs.is_empty() && spec.lower_like.is_none() {
        return invalid("derive: give --preset, --interventions or --lower-like");
    }
    let prot = Protection {
        protect_io: true,
        ..Default::default()
    };
    let (mut out, applied) = lesion::apply(&topo, &snap, &ivs, &prot)?;
    let mut lowered = Value::Null;
    if let Some(c) = spec.lower_like {
        let (ch, cand) = candidate_record(ctx, c)?;
        let strs = |k: &str| -> Vec<String> {
            cand[k]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let (n, s) = (strs("removed_neurons"), strs("removed_synapses"));
        let (low, map) = lesion::lower(&topo, &out, &n, &s, &prot)?;
        out = low;
        lowered = json!({"candidate": c, "candidate_hash": ch, "candidate_source": cand["source"], "removed_neurons": n, "removed_synapses": s, "mapping": map});
    }
    let raw = out.to_string();
    let (dtopo, dsnap) = model::from_native(&raw)?;
    let raw_h = ctx.project.put(raw.as_bytes())?;
    let topo_h = ctx.project.put_json(&dtopo)?;
    let dsnap_h = ctx.project.put(dsnap.to_string().as_bytes())?;
    let changed: usize = applied.iter().map(|a| a.synapses.len()).sum();
    let derived = json!({
        "source": source, "source_record": ctx.project.lookup(RecordKind::Source, &source)?,
        "source_snapshot": snap_h, "preset": spec.preset, "applied": applied, "lower_like": lowered,
    });
    let drec = json!({
        "schema": "aarnn-explorer-source-v1",
        "name": name, "format": "derived",
        "input_file": Value::Null, "input_sha256": sha256_hex(raw.as_bytes()),
        "raw_object": raw_h, "topology_object": topo_h, "snapshot_object": dsnap_h,
        "capability": dtopo.capability, "summary": dtopo.summary(),
        "assumptions": dtopo.assumptions, "limitations": dtopo.limitations, "model_flags": dtopo.model_flags,
        "derived_from": derived, "parent_capability": rec["capability"],
        "tool_version": env!("CARGO_PKG_VERSION"),
    });
    let h = ctx.project.put_json(&drec)?;
    ctx.project.bind(RecordKind::Source, name, &h)?;
    if let Some(w) = spec.write {
        let dir = w
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        crate::store::atomic_write(dir, w, raw.as_bytes())?;
    }
    Ok(json!({
        "source": name, "record": h, "derived_from": source, "preset": spec.preset,
        "synapses_changed": changed, "lowered": !lowered.is_null(),
        "summary": dtopo.summary(), "written": spec.write.map(|p| p.display().to_string()),
    }))
}

// --- baseline -----------------------------------------------------------

pub fn baseline(
    ctx: &Ctx,
    source: Option<&str>,
    split: &str,
    label: &str,
    reuse: bool,
) -> Result<Value> {
    let source = ctx.default_source(source)?;
    let (snap_h, snap_p, _) = ctx.executable(&source)?;
    let (suite, suite_h) = ctx.suite()?;
    let inputs = json!({"source": snap_h, "suite": suite_h, "split": split, "policy": ctx.config.execution.plasticity_policy});
    if let Ok(existing) = ctx.project.lookup(RecordKind::Result, label) {
        let prev: Value = ctx.project.get_json(&existing)?;
        if prev["inputs"] == inputs {
            if reuse {
                return Ok(
                    json!({"label": label, "result": existing, "reused": true, "passed": prev["verdict"]["passed"]}),
                );
            }
            return Err(ExplorerError::Conflict(format!(
                "result {label} exists; pass --reuse-compatible to reuse it"
            )));
        }
        return Err(ExplorerError::Conflict(format!(
            "result {label} exists with different inputs; choose a new label"
        )));
    }
    let job = ctx.job(
        &snap_h,
        &snap_p,
        &suite,
        JobKind::Evaluate {
            split: split.into(),
            reference: None,
        },
    );
    let (_, d) = ctx.run_jobs(vec![job])?.remove(0);
    let (v, cost, _) = verdict_of(&d)?;
    let rec = json!({
        "schema": "aarnn-explorer-result-v1", "kind": "baseline", "label": label, "source_name": source,
        "inputs": inputs, "config_sha256": ctx.config_sha, "verdict": v,
        "reference_measures": crate::tasks::reference_measures(&v), "cost": cost,
        // Path-independent: the record stays identical if the project moves.
        "enforcement": d.enforcement, "reproduce": ["aarnn-explorer", "baseline", "--project", "<this project>", "--split", split, "--label", label],
    });
    let h = ctx.project.put_json(&rec)?;
    ctx.project.bind(RecordKind::Result, label, &h)?;
    Ok(
        json!({"label": label, "result": h, "passed": v.passed, "quiescent": v.quiescent, "tasks": v.tasks.iter().map(|t| json!({"task": t.task, "passed": t.passed, "successes": t.successes, "trials": t.trials})).collect::<Vec<_>>()}),
    )
}

fn load_result(ctx: &Ctx, label: &str) -> Result<Value> {
    let h = ctx.project.lookup(RecordKind::Result, label)?;
    ctx.project.get_json(&h)
}

// --- capture + analyse --------------------------------------------------

pub fn gnat_params(ctx: &Ctx) -> GnatParams {
    GnatParams {
        tau_ms: ctx.config.gnat.tau_ms,
        dt_ms: 1.0,
        threshold: ctx.config.gnat.minimum_score,
    }
}

/// Capture complete traces of every task of a baseline's source over its
/// split, then construct GNATs, recurrence, calibration, sensitivity and
/// per-neuron participation.
pub fn analyse(ctx: &Ctx, run: &str) -> Result<Value> {
    // One analysis per baseline run: an existing one is reused (choose a new
    // baseline label to analyse under different settings).
    if let Ok(h) = ctx
        .project
        .lookup(RecordKind::Analysis, &format!("{run}-gnat"))
    {
        let rec: Value = ctx.project.get_json(&h)?;
        return Ok(
            json!({"analysis": format!("{run}-gnat"), "object": h, "method": rec["method"], "tasks": rec["tasks"].as_array().map(|t| t.len()), "traces": rec["traces"].as_object().map(|m| m.len()), "reused": true}),
        );
    }
    let base = load_result(ctx, run)?;
    let source = base["source_name"]
        .as_str()
        .ok_or_else(|| ExplorerError::InvalidInput(format!("{run} is not a baseline")))?
        .to_string();
    let split = base["inputs"]["split"]
        .as_str()
        .unwrap_or("development")
        .to_string();
    let (snap_h, snap_p, topo) = ctx.executable(&source)?;
    let (suite, _) = ctx.suite()?;
    let graded = ctx.config.gnat.method == gnat::METHOD_GRADED
        || topo
            .neurons
            .iter()
            .any(|n| n.graded && n.role == Role::Hidden);
    let seeds = suite.seeds(&split)?.to_vec();
    let jobs: Vec<Job> = suite
        .tasks
        .iter()
        .map(|t| {
            ctx.job(
                &snap_h,
                &snap_p,
                &suite,
                JobKind::Capture {
                    task: t.id.clone(),
                    seeds: seeds.clone(),
                    graded_events: graded,
                },
            )
        })
        .collect();
    let topo_h = capture::topology_hash(&topo);
    let norm = Norm::parse(&ctx.config.gnat.norm).unwrap_or(Norm::L1Abs);
    let adj = topo.adjacency(norm);
    let params = gnat_params(ctx);
    let mut per_task = Vec::new();
    let mut participation = vec![(0u64, 0u32); topo.neurons.len()];
    let mut all_depth = Vec::new();
    let mut traces = BTreeMap::new();
    let mut calibration = Value::Null;
    let mut sensitivity = Value::Null;
    for (job, d) in ctx.run_jobs(jobs)? {
        let JobKind::Capture { task, .. } = &job.job else {
            continue;
        };
        let r = d
            .result
            .as_ref()
            .filter(|r| r.status == "succeeded")
            .ok_or_else(|| {
                ExplorerError::Internal(format!("capture {task}: {} {}", d.outcome, d.stderr_tail))
            })?;
        let mut graphs: Vec<(Vec<gnat::Event>, gnat::GnatResult)> = Vec::new();
        for tr in r.output["trials"].as_array().cloned().unwrap_or_default() {
            let trial: crate::tasks::TrialResult = serde_json::from_value(tr["trial"].clone())?;
            let out: crate::exec::RunOutput = serde_json::from_value(tr["run"].clone())?;
            let mh = capture::publish(
                &ctx.project,
                &snap_h,
                &topo_h,
                task,
                &trial,
                &out,
                &job.policy,
                graded,
                ctx.config.capture.max_segment_mib,
            )?;
            let (_m, events) = capture::load(&ctx.project, &mh)?;
            traces.insert(format!("{task}/{}", trial.seed), mh);
            let res = gnat::analyse(
                &adj,
                &events,
                &params,
                Bounds::whole_run(out.ticks),
                if graded {
                    gnat::METHOD_GRADED
                } else {
                    gnat::METHOD
                },
            )
            .map_err(|e| ExplorerError::InvalidInput(e.to_string()))?;
            for (i, p) in gnat::participation(&res, &events, topo.neurons.len())
                .into_iter()
                .enumerate()
            {
                participation[i].0 += p.0;
                participation[i].1 += p.1;
            }
            if calibration.is_null() && !events.is_empty() && task != "quiet" {
                calibration = serde_json::to_value(
                    gnat::calibrate(
                        &adj,
                        &events,
                        &params,
                        Bounds::whole_run(out.ticks),
                        4,
                        ctx.config.execution.seed,
                        0.1,
                    )
                    .map_err(|e| ExplorerError::InvalidInput(e.to_string()))?,
                )?;
                sensitivity = serde_json::to_value(
                    gnat::sensitivity(&adj, &events, &params, Bounds::whole_run(out.ticks))
                        .map_err(|e| ExplorerError::InvalidInput(e.to_string()))?,
                )?;
            }
            all_depth.push(gnat::depth_summary(&res));
            graphs.push((events, res));
        }
        // Recurrence: first trial against each later trial of the same task.
        let mut motifs_found = Vec::new();
        let mut approximate = false;
        if ctx.config.gnat.recurrence_mode == "bounded-verified" && graphs.len() > 1 {
            let budget = ctx.config.gnat.max_motif_candidates;
            let mut subs = Vec::new();
            for k in 1..graphs.len() {
                let rec = motifs::analogous_subthreads(
                    &graphs[0].0,
                    &graphs[0].1.edges,
                    &graphs[k].0,
                    &graphs[k].1.edges,
                    false,
                    budget,
                    3,
                );
                approximate |= rec.approximate;
                subs.extend(rec.subthreads);
            }
            motifs_found = motifs::group_motifs(&subs);
        }
        let named = |ns: &[u32]| {
            ns.iter()
                .map(|&i| topo.neurons[i as usize].id.clone())
                .collect::<Vec<_>>()
        };
        let first_seed = seeds.first().copied();
        let first_trace = first_seed.and_then(|sd| traces.get(&format!("{task}/{sd}")).cloned());
        per_task.push(json!({
            "task": task,
            "trials": graphs.len(),
            "depth": graphs.iter().map(|g| gnat::depth_summary(&g.1)).collect::<Vec<_>>(),
            "first_trial": {"seed": first_seed, "trace": first_trace},
            "gnats_first_trial": graphs.first().map(|g| g.1.components.iter().take(50).map(|c| json!({
                "id": c.id, "neurons": named(&c.neurons), "events": c.events.len(), "depth": c.depth,
                "event_ids": c.events.iter().take(2000).map(|&i| g.0[i as usize].id.to_string()).collect::<Vec<_>>(),
                "event_ids_truncated": c.events.len() > 2000,
                "start_tick": c.start_tick, "end_tick": c.end_tick, "open_start": c.open_start, "open_end": c.open_end})).collect::<Vec<_>>()),
            "motifs": motifs_found.iter().take(20).map(|m| json!({
                "fingerprint": m.fingerprint, "occurrences": m.occurrences, "neurons": named(&m.neurons),
                "edges": m.edge_pairs.iter().map(|(a, b)| format!("{}->{}", topo.neurons[*a as usize].id, topo.neurons[*b as usize].id)).collect::<Vec<_>>(),
                "mean_timing_jitter_ticks": m.mean_timing_jitter_ticks, "collisions_separated": m.collisions_separated})).collect::<Vec<_>>(),
            "recurrence_approximate": approximate,
        }));
    }
    let part: Vec<Value> = topo
        .neurons
        .iter()
        .enumerate()
        .filter(|(_, n)| n.role == Role::Hidden)
        .map(|(i, n)| json!({"id": n.id, "region": n.region, "events_in_gnats": participation[i].0, "gnats": participation[i].1}))
        .collect();
    let rec = json!({
        "schema": "aarnn-explorer-analysis-v1", "run": run, "source": source, "method": if graded { gnat::METHOD_GRADED } else { gnat::METHOD },
        "params": params, "norm": ctx.config.gnat.norm, "traces": traces, "tasks": per_task, "participation": part,
        "calibration": calibration, "sensitivity": sensitivity,
        "coverage_note": "complete isolated capture of every task over the run's split; positive-edge GNATs only (inhibition and gap junctions are overlays)",
    });
    let h = ctx.project.put_json(&rec)?;
    ctx.project
        .bind(RecordKind::Analysis, &format!("{run}-gnat"), &h)?;
    Ok(
        json!({"analysis": format!("{run}-gnat"), "object": h, "method": rec["method"], "tasks": per_task.len(), "traces": rec["traces"].as_object().map(|m| m.len())}),
    )
}

// --- experiment ---------------------------------------------------------

pub fn experiment(ctx: &Ctx, plan_path: &Path) -> Result<Value> {
    let plan: Value = serde_json::from_slice(&std::fs::read(plan_path)?)?;
    if plan["schema"].as_str() != Some("aarnn-explorer-plan-v1") {
        return invalid("plan schema must be aarnn-explorer-plan-v1");
    }
    let source = ctx.default_source(plan["source"].as_str())?;
    let split = plan["split"].as_str().unwrap_or("development").to_string();
    let (snap_h, snap_p, _) = ctx.executable(&source)?;
    let before = crate::store::sha256_file(&snap_p)?;
    let (suite, suite_h) = ctx.suite()?;
    let ivs: Vec<Intervention> = serde_json::from_value(plan["interventions"].clone())?;
    // Intact reference and a sham control first.
    let mut jobs = vec![ctx.job(
        &snap_h,
        &snap_p,
        &suite,
        JobKind::Evaluate {
            split: split.clone(),
            reference: None,
        },
    )];
    let mut sham = ctx.job(
        &snap_h,
        &snap_p,
        &suite,
        JobKind::Evaluate {
            split: split.clone(),
            reference: None,
        },
    );
    sham.interventions = vec![Intervention {
        id: "sham".into(),
        selection: lesion::Selection::Neurons { ids: vec![] },
        action: lesion::Action::Sham,
    }];
    jobs.push(sham);
    let ref_d = ctx.run_jobs(jobs)?;
    let (intact, _, _) = verdict_of(&ref_d[0].1)?;
    let (shamv, _, _) = verdict_of(&ref_d[1].1)?;
    let rm = crate::tasks::reference_measures(&intact);
    let digests = |v: &SuiteVerdict| {
        v.tasks
            .iter()
            .map(|t| (t.task.clone(), t.mean_measures.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let sham_matches = digests(&intact) == digests(&shamv);
    let mut jobs = Vec::new();
    for iv in &ivs {
        let mut j = ctx.job(
            &snap_h,
            &snap_p,
            &suite,
            JobKind::Evaluate {
                split: split.clone(),
                reference: Some(rm.clone()),
            },
        );
        j.interventions = vec![iv.clone()];
        jobs.push(j);
    }
    let mut rows = Vec::new();
    for (j, d) in ctx.run_jobs(jobs)? {
        let iv = &j.interventions[0];
        match verdict_of(&d) {
            Ok((v, cost, meta)) => rows.push(json!({
                "intervention": iv, "applied": meta["applied"], "passed": v.passed, "quiescent": v.quiescent,
                "failed_tasks": v.tasks.iter().filter(|t| !t.passed).map(|t| t.task.clone()).collect::<Vec<_>>(),
                "cost": cost, "job": j.job_id,
            })),
            Err(e) => rows.push(json!({"intervention": iv, "error": e.to_string(), "job": j.job_id})),
        }
    }
    let after = crate::store::sha256_file(&snap_p)?;
    let rec = json!({
        "schema": "aarnn-explorer-result-v1", "kind": "experiment", "plan": plan, "source": source, "split": split,
        "suite_sha256": suite_h, "intact_passed": intact.passed, "sham_matches_intact": sham_matches,
        "source_unchanged": before == after, "rows": rows,
    });
    let h = ctx.project.put_json(&rec)?;
    let label = format!("experiment-{}", &h[..12]);
    ctx.project.bind(RecordKind::Result, &label, &h)?;
    Ok(
        json!({"label": label, "object": h, "intact_passed": intact.passed, "sham_matches_intact": sham_matches, "source_unchanged": before == after, "rows": rec["rows"]}),
    )
}

// --- validate -----------------------------------------------------------

fn candidate_record(ctx: &Ctx, candidate: &str) -> Result<(String, Value)> {
    let h = ctx.project.lookup(RecordKind::Candidate, candidate)?;
    Ok((h.clone(), ctx.project.get_json(&h)?))
}

/// Locked assessment of a fixed candidate: functional verdict against the
/// intact reference on the same split, resilience under the profile, cost.
pub fn validate(ctx: &Ctx, candidate: &str, split: &str) -> Result<Value> {
    let (ch, cand) = candidate_record(ctx, candidate)?;
    // One locked assessment per (candidate, split, profile): reuse it.
    let label = format!("validation-{}-{split}-{}", &ch[..12], &ctx.config_sha[..8]);
    if let Ok(h) = ctx.project.lookup(RecordKind::Result, &label) {
        let rec: Value = ctx.project.get_json(&h)?;
        return Ok(
            json!({"candidate": ch, "validation": h, "split": split, "behavioural_preservation": rec["verdicts"]["behavioural_preservation"], "resilience": rec["verdicts"]["resilience"], "intact_cost": rec["cost"]["intact"], "candidate_cost": rec["cost"]["candidate"], "reused": true}),
        );
    }
    let locked = split == ctx.config.functional.validation_split;
    if split == ctx.config.functional.development_split {
        return invalid(
            "validation on the development split is not an assessment; use the locked split",
        );
    }
    let (suite, suite_h) = ctx.suite()?;
    if cand["suite_sha256"].as_str() != Some(suite_h.as_str()) {
        return Err(ExplorerError::Conflict(
            "the candidate was selected under a different task suite".into(),
        ));
    }
    let src_h = cand["source_snapshot"].as_str().unwrap_or("").to_string();
    let src_p = ctx.project.object_file(&src_h)?;
    let low_h = cand["snapshot_object"].as_str().unwrap_or("").to_string();
    let low_p = ctx.project.object_file(&low_h)?;
    let intact = ctx.run_jobs(vec![ctx.job(
        &src_h,
        &src_p,
        &suite,
        JobKind::Evaluate {
            split: split.into(),
            reference: None,
        },
    )])?;
    let (iv, icost, _) = verdict_of(&intact[0].1)?;
    let rm = crate::tasks::reference_measures(&iv);
    let run = ctx.run_jobs(vec![ctx.job(
        &low_h,
        &low_p,
        &suite,
        JobKind::Evaluate {
            split: split.into(),
            reference: Some(rm.clone()),
        },
    )])?;
    let (cv, ccost, _) = verdict_of(&run[0].1)?;
    let low_snap: Value = ctx.project.get_json(&low_h)?;
    let (low_topo, _) = model::from_native(&low_snap.to_string())?;
    let res = crate::optimise::resilience(ctx, &suite, &low_h, &low_p, &low_topo, split, &rm)?;
    let rec = json!({
        "schema": "aarnn-explorer-validation-v1", "candidate": ch, "split": split, "locked": locked,
        "config_sha256": ctx.config_sha, "profile": ctx.config.name,
        "verdicts": {
            "implementation": "see test suite (cargo test -p aarnn-explorer)",
            "reference_agreement": if cv.passed { "passed" } else { "failed" },
            "behavioural_preservation": if cv.passed && !cv.quiescent { "passed" } else { "failed" },
            "biological_validity": "not-assessed: authored/simulated tasks only",
            "resilience": res["verdict"],
            "hardware_benefit": "estimated (proxy cost model) plus measured CPU step latency",
        },
        "intact": iv, "candidate_verdict": cv, "reference_measures": rm,
        "cost": {"intact": icost, "candidate": ccost, "basis": ccost.cost_basis},
        "resilience": res,
        // The resolved hash, not the submitted alias/name, so the record is
        // identical however the candidate was named.
        "reproduce": ["aarnn-explorer", "validate", "--project", "<this project>", "--candidate", &ch, "--split", split],
    });
    let h = ctx.project.put_json(&rec)?;
    ctx.project.bind(
        RecordKind::Result,
        &format!("validation-{}-{split}-{}", &ch[..12], &ctx.config_sha[..8]),
        &h,
    )?;
    Ok(
        json!({"candidate": ch, "validation": h, "split": split, "behavioural_preservation": rec["verdicts"]["behavioural_preservation"], "resilience": res["verdict"], "intact_cost": icost, "candidate_cost": ccost}),
    )
}

// --- export / verify ----------------------------------------------------

pub fn export(ctx: &Ctx, candidate: &str, output: &Path, allow_unvalidated: bool) -> Result<Value> {
    let (ch, cand) = candidate_record(ctx, candidate)?;
    let split = ctx.config.functional.validation_split.clone();
    let vlabel = format!("validation-{}-{split}-{}", &ch[..12], &ctx.config_sha[..8]);
    let validation: Option<Value> = ctx
        .project
        .lookup(RecordKind::Result, &vlabel)
        .ok()
        .and_then(|h| ctx.project.get_json(&h).ok());
    let status = match &validation {
        Some(v) if v["verdicts"]["behavioural_preservation"] == "passed" => "validated",
        Some(_) => "validation-failed",
        None if allow_unvalidated => "exploratory-unvalidated",
        None => {
            return Err(ExplorerError::InvalidInput(format!(
                "candidate {ch} has no locked validation under this profile; run validate, or pass --exploratory"
            )));
        }
    };
    if output.exists() && std::fs::read_dir(output)?.next().is_some() {
        return Err(ExplorerError::Conflict(format!(
            "{} exists and is not empty",
            output.display()
        )));
    }
    std::fs::create_dir_all(output)?;
    let low: Value = ctx
        .project
        .get_json(cand["snapshot_object"].as_str().unwrap_or(""))?;
    let (suite, _) = ctx.suite()?;
    let source: Value = ctx.project.get_json(
        &ctx.project
            .lookup(RecordKind::Source, cand["source"].as_str().unwrap_or(""))?,
    )?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert(
        "candidate.network.json".into(),
        low.to_string().into_bytes(),
    );
    files.insert(
        "mapping.json".into(),
        serde_json::to_vec_pretty(&cand["mapping"])?,
    );
    files.insert(
        "contract.json".into(),
        serde_json::to_vec_pretty(&json!({"config": ctx.config, "suite": suite}))?,
    );
    files.insert(
        "validation.json".into(),
        serde_json::to_vec_pretty(&validation.clone().unwrap_or(Value::Null))?,
    );
    let cost = json!({
        // Both costs from the same split's runs: the validation record when it
        // exists, otherwise the development search.
        "intact": validation.as_ref().map(|v| v["cost"]["intact"].clone()).unwrap_or(cand["intact_cost"].clone()),
        "candidate": validation.as_ref().map(|v| v["cost"]["candidate"].clone()).unwrap_or(cand["development"]["cost"].clone()),
        "split": validation.as_ref().map(|v| v["split"].clone()).unwrap_or(Value::from("development")),
        "measured": ["latency_ns_p50 (CPU step time of the native runner, this host)"],
        "estimated": ["state_bytes", "memory_bytes (native dense allocation model)"],
        "not_available": ["energy", "silicon area", "FPAA/ASIC placement (target-specific tools)"],
    });
    files.insert("cost.json".into(), serde_json::to_vec_pretty(&cost)?);
    let manifest = json!({
        "schema": "aarnn-explorer-bundle-v1", "status": status, "candidate": ch,
        "classification": "declared-cold-start", "target": ctx.config.hardware.target,
        "required_capabilities": ["aarnn_rust::runner::Runner (NeuronModel::Aarnn, Learning::Aarnn)", "single hidden layer import", "features: parallel, growth3d, morpho"],
        "source": {"name": cand["source"], "snapshot_sha256": cand["source_snapshot"], "input_sha256": source["input_sha256"], "capability": source["capability"], "limitations": source["limitations"], "assumptions": source["assumptions"]},
        "suite_sha256": cand["suite_sha256"], "config_sha256": ctx.config_sha, "tool_version": env!("CARGO_PKG_VERSION"),
        "removed_neurons": cand["removed_neurons"], "removed_synapses": cand["removed_synapses"],
        "licences": {"explorer_and_candidate_format": "MIT", "source_dataset": source["summary"]["limitations"]},
        "reproduce": [
            format!("aarnn-explorer verify-bundle --bundle {}", output.display()),
        ],
        "claims": "behavioural preservation of the named tasks over the declared seeds only; no biological equivalence is claimed",
    });
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest)?,
    );
    let mut sums = String::new();
    for (name, bytes) in &files {
        crate::store::atomic_write(output, &output.join(name), bytes)?;
        sums.push_str(&format!("{}  {name}\n", sha256_hex(bytes)));
    }
    crate::store::atomic_write(output, &output.join("sha256sums.txt"), sums.as_bytes())?;
    Ok(
        json!({"bundle": output.display().to_string(), "status": status, "candidate": ch, "files": files.keys().collect::<Vec<_>>(), "cost": cost}),
    )
}

/// Clean-process verification: checksums, structural re-import and a fresh
/// worker re-evaluation of the exported network on the validated split. The
/// candidate's per-task mean measures must match the validation record.
pub fn verify_bundle(bundle: &Path, scratch: &Path, exe: &Path) -> Result<Value> {
    let sums = std::fs::read_to_string(bundle.join("sha256sums.txt"))?;
    for line in sums.lines() {
        let (h, name) = line
            .split_once("  ")
            .ok_or_else(|| ExplorerError::InvalidInput("malformed sha256sums.txt".into()))?;
        let actual = crate::store::sha256_file(&bundle.join(name))?;
        if actual != h {
            return invalid(format!("{name}: checksum mismatch"));
        }
    }
    let manifest: Value = serde_json::from_slice(&std::fs::read(bundle.join("manifest.json"))?)?;
    let validation: Value =
        serde_json::from_slice(&std::fs::read(bundle.join("validation.json"))?)?;
    let contract: Value = serde_json::from_slice(&std::fs::read(bundle.join("contract.json"))?)?;
    let net_path = bundle.join("candidate.network.json");
    let (topo, _) = model::from_native(&std::fs::read_to_string(&net_path)?)?;
    let suite: TaskSuite = serde_json::from_value(contract["suite"].clone())?;
    let config: ExplorerConfig = serde_json::from_value(contract["config"].clone())?;
    if validation.is_null() {
        return Ok(
            json!({"bundle": bundle.display().to_string(), "status": manifest["status"], "reimport": "ok", "revalidation": "not-run: the bundle is exploratory and unvalidated"}),
        );
    }
    let split = validation["split"]
        .as_str()
        .unwrap_or("locked-validation")
        .to_string();
    let rm: BTreeMap<String, BTreeMap<String, f64>> =
        serde_json::from_value(validation["reference_measures"].clone())?;
    let job = Job {
        schema: worker::JOB_SCHEMA.into(),
        job_id: "verify".into(),
        snapshot_path: net_path.clone(),
        snapshot_sha256: crate::store::sha256_file(&net_path)?,
        suite,
        policy: config.execution.plasticity_policy.clone(),
        interventions: vec![],
        lower_neurons: vec![],
        lower_synapses: vec![],
        protect_io: true,
        capabilities: vec![],
        job: JobKind::Evaluate {
            split,
            reference: Some(rm),
        },
    };
    std::fs::create_dir_all(scratch)?;
    let jp = scratch.join("verify-job.json");
    let rp = scratch.join("verify-result.json");
    crate::store::atomic_write(scratch, &jp, serde_json::to_string(&job)?.as_bytes())?;
    let limits = Limits {
        threads_per_worker: 1,
        memory_limit_mib: config.resources.memory_limit_mib,
        wall_timeout: Duration::from_secs(config.resources.worker_wall_timeout_seconds),
        os_enforcement: false,
    };
    let d = worker::spawn(exe, &jp, &rp, &limits, false)?;
    let (v, _, _) = verdict_of(&d)?;
    let expected: SuiteVerdict = serde_json::from_value(validation["candidate_verdict"].clone())?;
    let same = v.tasks.iter().zip(&expected.tasks).all(|(a, b)| {
        a.task == b.task && a.passed == b.passed && a.mean_measures == b.mean_measures
    });
    Ok(json!({
        "bundle": bundle.display().to_string(), "status": manifest["status"], "reimport": "ok",
        "neurons": topo.neurons.len(), "revalidation_passed": v.passed, "reproduces_validation": same,
    }))
}

// --- demo ---------------------------------------------------------------

pub fn demo(project_dir: &Path) -> Result<Value> {
    let repo = std::env::current_dir()?;
    let fixture = repo.join("examples/explorer/models/synthetic_sensorimotor.sparse.json");
    let plan = repo.join("examples/explorer/interventions/single_ablation.json");
    let resilient_cfg = repo.join("configs/explorer/resilient.toml");
    for p in [&fixture, &plan, &resilient_cfg] {
        if !p.exists() {
            return invalid(format!(
                "{} not found: run the demo from the repository root",
                p.display()
            ));
        }
    }
    let project = Project::open_or_init(project_dir, "explorer-demo")?;
    let ctx = Ctx::new(project, None)?;
    let mut report = serde_json::Map::new();
    let src = "synthetic-sensorimotor";
    let imported = match ctx.project.lookup(RecordKind::Source, src) {
        Ok(h) => json!({"source": src, "record": h, "reused": true}),
        Err(_) => import(&ctx, "sparse", &fixture, src)?,
    };
    report.insert("import".into(), imported);
    report.insert(
        "baseline".into(),
        baseline(&ctx, Some(src), "development", "intact-development", true)?,
    );
    report.insert("analysis".into(), analyse(&ctx, "intact-development")?);
    report.insert("experiment".into(), experiment(&ctx, &plan)?);
    let compact = crate::optimise::optimise(
        &ctx,
        Some(src),
        "min-state-bytes",
        Some("intact-development-gnat"),
    )?;
    report.insert("optimise_compact".into(), compact.clone());
    let compact_name = compact["candidate"]
        .as_str()
        .ok_or_else(|| ExplorerError::Internal(format!("compact optimisation: {compact}")))?
        .to_string();
    report.insert(
        "validate_compact".into(),
        validate(&ctx, &compact_name, "locked-validation")?,
    );
    // The same compact candidate under the resilient profile, then the
    // resilient profile's own search.
    let rctx = Ctx::new(Project::open(project_dir)?, Some(&resilient_cfg))?;
    let compact_under_resilient = validate(&rctx, &compact_name, "locked-validation")?;
    report.insert(
        "compact_under_resilient_profile".into(),
        compact_under_resilient,
    );
    let resilient = crate::optimise::optimise(
        &rctx,
        Some(src),
        "min-state-bytes",
        Some("intact-development-gnat"),
    )?;
    report.insert("optimise_resilient".into(), resilient.clone());
    if let Some(rn) = resilient["candidate"].as_str() {
        report.insert(
            "validate_resilient".into(),
            validate(&rctx, rn, "locked-validation")?,
        );
    }
    let out = project_dir.join("export-compact");
    let exported = if out.exists() {
        json!({"bundle": out.display().to_string(), "reused": true})
    } else {
        export(&ctx, &compact_name, &out, false)?
    };
    report.insert("export".into(), exported);
    report.insert(
        "verify_bundle".into(),
        verify_bundle(&out, &project_dir.join("tmp/verify"), &ctx.exe)?,
    );
    let report = Value::Object(report);
    crate::store::atomic_write(
        &ctx.project.tmp_dir(),
        &project_dir.join("demo-report.json"),
        serde_json::to_string_pretty(&report)?.as_bytes(),
    )?;
    Ok(report)
}
