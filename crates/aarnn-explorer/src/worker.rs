// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Bounded child worker processes (brief Sections 4, 15 and 17).
//!
//! A worker is this binary in `worker` mode. It gets one job file, writes one
//! result file atomically, and exits. Each worker:
//! - builds its own `Runner` from an immutable source object and verifies the
//!   object's hash before and after the run;
//! - starts with a cleared environment: no credentials, no live peripheral,
//!   AER, robot or federation settings;
//! - gets bounded threads (`RAYON_NUM_THREADS`, `NM_SIM_PAR_MAX_WORKERS`,
//!   `OPENBLAS_NUM_THREADS`, `OMP_NUM_THREADS`);
//! - runs in its own process group, so the controller can kill it and every
//!   descendant at a deadline.
//!
//! OS enforcement is `enforced` when `systemd-run --user --scope` applies
//! MemoryMax/CPUQuota/TasksMax, and `advisory` otherwise. Internal admission
//! limits apply in both cases.

use std::collections::BTreeMap;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ExplorerError, Result};
use crate::lesion::{Intervention, Protection};
use crate::model::{self, Topology};
use crate::store::{atomic_write, sha256_file};
use crate::tasks::{self, TaskSuite};

pub const JOB_SCHEMA: &str = "aarnn-explorer-job-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JobKind {
    /// Evaluate a (possibly intervened / lowered) network on a suite split.
    Evaluate {
        split: String,
        #[serde(default)]
        reference: Option<BTreeMap<String, BTreeMap<String, f64>>>,
    },
    /// Record complete traces of one task over seeds.
    Capture {
        task: String,
        seeds: Vec<u64>,
        graded_events: bool,
    },
    /// Deliberately misbehave (tests of timeout/crash handling).
    Fault { mode: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub schema: String,
    pub job_id: String,
    pub snapshot_path: PathBuf,
    pub snapshot_sha256: String,
    pub suite: TaskSuite,
    pub policy: String,
    #[serde(default)]
    pub interventions: Vec<Intervention>,
    /// Hidden neurons / synapses to remove physically before running.
    #[serde(default)]
    pub lower_neurons: Vec<String>,
    #[serde(default)]
    pub lower_synapses: Vec<String>,
    pub protect_io: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub job: JobKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobResult {
    pub schema: String,
    pub job_id: String,
    /// Process-level outcome: `succeeded` | `failed`.
    pub status: String,
    pub error: Option<String>,
    pub source_unchanged: bool,
    pub output: Value,
    pub wall_ms: f64,
}

/// Prepare the executable network of a job: interventions (masks) on an
/// isolated copy, then physical lowering.
pub fn prepare(job: &Job) -> Result<(Topology, Value, Value)> {
    let raw = std::fs::read_to_string(&job.snapshot_path)?;
    let (topo, snap) = model::from_native(&raw)?;
    let prot = Protection {
        protect_io: job.protect_io,
        ..Default::default()
    };
    let (snap, applied) = crate::lesion::apply(&topo, &snap, &job.interventions, &prot)?;
    let (snap, mapping) = if job.lower_neurons.is_empty() && job.lower_synapses.is_empty() {
        (snap, Value::Null)
    } else {
        let (s, m) =
            crate::lesion::lower(&topo, &snap, &job.lower_neurons, &job.lower_synapses, &prot)?;
        (s, serde_json::to_value(m)?)
    };
    let meta = serde_json::json!({"applied": applied, "mapping": mapping});
    Ok((topo, snap, meta))
}

/// Worker entry point: run one job file and write its result.
pub fn run_job(job_path: &Path, out_path: &Path) -> Result<JobResult> {
    let started = Instant::now();
    let job: Job = serde_json::from_slice(&std::fs::read(job_path)?)?;
    if job.schema != JOB_SCHEMA {
        return Err(ExplorerError::Unsupported(format!(
            "job schema {:?}",
            job.schema
        )));
    }
    let before = sha256_file(&job.snapshot_path)?;
    if before != job.snapshot_sha256 {
        return Err(ExplorerError::InvalidInput(format!(
            "source object {} has hash {before}, expected {}",
            job.snapshot_path.display(),
            job.snapshot_sha256
        )));
    }
    let outcome: Result<Value> = (|| {
        let (_topo, snap, meta) = prepare(&job)?;
        // Transient silencing windows from the applied interventions.
        let silence: Vec<crate::exec::Silence> = meta["applied"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|a| a["silence"].as_array().cloned().unwrap_or_default())
            .filter_map(|x| {
                let v = x.as_array()?;
                Some(crate::exec::Silence {
                    hidden: v.first()?.as_u64()? as u32,
                    start_tick: v.get(1)?.as_u64()?,
                    end_tick: v.get(2)?.as_u64()?,
                })
            })
            .collect();
        if !silence.is_empty() && !(job.lower_neurons.is_empty() && job.lower_synapses.is_empty()) {
            return Err(ExplorerError::Unsupported("transient silencing and physical lowering in one job: dense indices would refer to different generations".into()));
        }
        match &job.job {
            JobKind::Evaluate { split, reference } => {
                let v = tasks::evaluate_silenced(
                    &snap,
                    &job.suite,
                    split,
                    &job.policy,
                    reference.as_ref(),
                    &job.capabilities,
                    &silence,
                )?;
                Ok(
                    serde_json::json!({"verdict": v, "cost": crate::lesion::cost(&snap), "meta": meta}),
                )
            }
            JobKind::Capture {
                task,
                seeds,
                graded_events,
            } => {
                let t = job
                    .suite
                    .tasks
                    .iter()
                    .find(|t| &t.id == task)
                    .ok_or_else(|| ExplorerError::InvalidInput(format!("unknown task {task}")))?;
                let mut trials = Vec::new();
                for &seed in seeds {
                    let (tr, out) =
                        crate::capture::capture_trial(&snap, t, seed, &job.policy, *graded_events)?;
                    trials.push(serde_json::json!({"trial": tr, "run": out}));
                }
                Ok(serde_json::json!({"trials": trials, "meta": meta}))
            }
            JobKind::Fault { mode } => match mode.as_str() {
                "sleep" => {
                    std::thread::sleep(Duration::from_secs(3600));
                    Ok(Value::Null)
                }
                "panic" => panic!("deliberate worker fault"),
                "alloc" => {
                    // Grow until the OS limit (or the deadline) stops it.
                    let mut v: Vec<Vec<u8>> = Vec::new();
                    loop {
                        v.push(vec![1u8; 64 << 20]);
                    }
                }
                m => Err(ExplorerError::InvalidInput(format!("fault mode {m}"))),
            },
        }
    })();
    let after = sha256_file(&job.snapshot_path)?;
    let (status, error, output) = match outcome {
        Ok(v) => ("succeeded", None, v),
        Err(e) => ("failed", Some(format!("{}: {e}", e.code())), Value::Null),
    };
    let res = JobResult {
        schema: JOB_SCHEMA.into(),
        job_id: job.job_id.clone(),
        status: status.into(),
        error,
        source_unchanged: after == before,
        output,
        wall_ms: started.elapsed().as_secs_f64() * 1000.0,
    };
    let tmp = out_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    atomic_write(&tmp, out_path, serde_json::to_string(&res)?.as_bytes())?;
    Ok(res)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    pub threads_per_worker: u32,
    pub memory_limit_mib: u64,
    pub wall_timeout: Duration,
    /// Try `systemd-run --user --scope` for OS enforcement.
    pub os_enforcement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Disposition {
    /// succeeded | failed | timeout | killed | crashed
    pub outcome: String,
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub enforcement: String,
    pub wall_ms: f64,
    pub result: Option<JobResult>,
    pub stderr_tail: String,
}

/// Whether `systemd-run --user --scope` works on this host (cached per call
/// site by the caller).
pub fn systemd_scope_available() -> bool {
    Command::new("systemd-run")
        .args(["--user", "--scope", "-q", "--", "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawn a worker for `job_path`, enforce the deadline, return its
/// disposition. The result file is read only if the worker exited cleanly.
pub fn spawn(
    exe: &Path,
    job_path: &Path,
    out_path: &Path,
    limits: &Limits,
    enforced: bool,
) -> Result<Disposition> {
    spawn_cancellable(exe, job_path, out_path, limits, enforced, None)
}

/// As `spawn`; when `cancel` becomes true the worker's process group is
/// terminated (checked once per second) and the outcome is `cancelled`.
pub fn spawn_cancellable(
    exe: &Path,
    job_path: &Path,
    out_path: &Path,
    limits: &Limits,
    enforced: bool,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Disposition> {
    let started = Instant::now();
    let threads = limits.threads_per_worker.max(1).to_string();
    let mut cmd = if enforced && limits.os_enforcement {
        let mut c = Command::new("systemd-run");
        c.args([
            "--user",
            "--scope",
            "-q",
            "-p",
            &format!("MemoryMax={}M", limits.memory_limit_mib),
            "-p",
            "MemorySwapMax=0",
            "-p",
            &format!("CPUQuota={}%", 100 * limits.threads_per_worker.max(1)),
            "-p",
            "TasksMax=64",
            "--",
        ]);
        c.arg(exe);
        c
    } else {
        Command::new(exe)
    };
    cmd.arg("worker")
        .arg("--job")
        .arg(job_path)
        .arg("--out")
        .arg(out_path);
    cmd.env_clear();
    for k in [
        "PATH",
        "HOME",
        "LANG",
        "LD_LIBRARY_PATH",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        if let Ok(v) = std::env::var(k) {
            cmd.env(k, v);
        }
    }
    cmd.env("RAYON_NUM_THREADS", &threads)
        .env("NM_SIM_PAR_MAX_WORKERS", &threads)
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("OMP_NUM_THREADS", "1")
        .env("NM_DISABLE_OPENCL", "1")
        .env("AARNN_EXPLORER_WORKER", "1");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| ExplorerError::Internal(format!("spawn worker: {e}")))?;
    let pid = child.id() as i32;
    let mut stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let st = child.wait();
        let _ = tx.send(st);
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            use std::io::Read;
            let _ = e.take(1 << 20).read_to_string(&mut s);
        }
        s
    });
    let deadline = started + limits.wall_timeout;
    let mut cancelled = false;
    let first = loop {
        let now = Instant::now();
        if now >= deadline {
            break Err(mpsc::RecvTimeoutError::Timeout);
        }
        let slice = (deadline - now).min(Duration::from_secs(1));
        match rx.recv_timeout(slice) {
            Ok(st) => break Ok(st),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    cancelled = true;
                    break Err(mpsc::RecvTimeoutError::Timeout);
                }
            }
            Err(e) => break Err(e),
        }
    };
    let status = match first {
        Ok(st) => Some(st?),
        Err(_) => {
            // Deadline: terminate the whole process group, then kill.
            unsafe_kill_group(pid, libc::SIGTERM);
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(_) => {}
                Err(_) => {
                    unsafe_kill_group(pid, libc::SIGKILL);
                    let _ = rx.recv_timeout(Duration::from_secs(5));
                }
            }
            None
        }
    };
    let _ = waiter.join();
    let stderr_full = err_reader.join().unwrap_or_default();
    let tail: String = stderr_full
        .chars()
        .rev()
        .take(2000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    let enforcement = if enforced && limits.os_enforcement {
        "enforced"
    } else {
        "advisory"
    }
    .to_string();
    let Some(st) = status else {
        if cancelled {
            return Ok(Disposition {
                outcome: "cancelled".into(),
                exit: None,
                signal: Some(libc::SIGTERM),
                enforcement,
                wall_ms,
                result: None,
                stderr_tail: tail,
            });
        }
        return Ok(Disposition {
            outcome: "timeout".into(),
            exit: None,
            signal: Some(libc::SIGKILL),
            enforcement,
            wall_ms,
            result: None,
            stderr_tail: tail,
        });
    };
    if let Some(sig) = st.signal() {
        return Ok(Disposition {
            outcome: "killed".into(),
            exit: None,
            signal: Some(sig),
            enforcement,
            wall_ms,
            result: None,
            stderr_tail: tail,
        });
    }
    let code = st.code();
    let result: Option<JobResult> = std::fs::read(out_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let outcome = match (code, &result) {
        (Some(0), Some(r)) if r.status == "succeeded" => "succeeded",
        (Some(0), Some(_)) => "failed",
        (Some(_), _) => "crashed",
        _ => "crashed",
    };
    Ok(Disposition {
        outcome: outcome.into(),
        exit: code,
        signal: None,
        enforcement,
        wall_ms,
        result,
        stderr_tail: tail,
    })
}

/// Signal a whole process group. `libc::kill` is an FFI call, so it is
/// unsafe; the pid is our own child's group id (process_group(0) makes the
/// child its group leader), and a stale id at worst signals nothing.
fn unsafe_kill_group(pid: i32, sig: i32) {
    if pid > 0 {
        // SAFETY: kill(2) has no memory-safety preconditions; a negative pid
        // addresses the process group created for this child.
        unsafe {
            libc::kill(-pid, sig);
        }
    }
}
