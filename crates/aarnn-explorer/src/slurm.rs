// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Optional Slurm launcher using the same worker job file and result
//! protocol. Generating software never submits: `submit` runs only when the
//! user invokes it, and `--dry-run` prints the command. Commands use
//! structured arguments, never a shell string. The worker binary must be
//! built for the allocated nodes' architecture (an x86_64 binary does not run
//! on an ARM64 partition): `--worker-exe` names the binary on the compute
//! nodes, and `--sbatch-arg` pins or excludes nodes (`--nodelist=...`). The
//! job file, its snapshot and the result directory must exist at the same
//! paths on the compute node (shared storage, or staged copies).

use std::path::Path;

use serde_json::{Value, json};

use crate::error::{ExplorerError, Result, invalid};

pub trait CommandRunner {
    fn run(&mut self, program: &str, args: &[String]) -> Result<(i32, String, String)>;
}

pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&mut self, program: &str, args: &[String]) -> Result<(i32, String, String)> {
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|e| ExplorerError::Unsupported(format!("{program}: {e}")))?;
        Ok((
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn render(
    job: &Path,
    exe: &Path,
    cpus: u32,
    mem_mib: u64,
    time: &str,
    partition: Option<&str>,
) -> Result<String> {
    let j: crate::worker::Job = serde_json::from_slice(&std::fs::read(job)?)?;
    if cpus == 0 || mem_mib < 64 {
        return invalid("cpus >= 1 and mem_mib >= 64");
    }
    if !time
        .chars()
        .all(|c| c.is_ascii_digit() || c == ':' || c == '-')
    {
        return invalid(format!("time {time:?}"));
    }
    let out = job.with_file_name("result.json");
    let mut s = String::from("#!/bin/bash\n");
    s += &format!(
        "#SBATCH --job-name=aarnn-explorer-{}\n",
        &j.job_id.chars().take(24).collect::<String>()
    );
    s += &format!(
        "#SBATCH --cpus-per-task={cpus}\n#SBATCH --mem={mem_mib}M\n#SBATCH --time={time}\n"
    );
    if let Some(p) = partition {
        if !p
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return invalid(format!("partition {p:?}"));
        }
        s += &format!("#SBATCH --partition={p}\n");
    }
    s += &format!(
        "#SBATCH --output={}\n",
        quote(&job.with_file_name("slurm-%j.log").display().to_string())
    );
    s += &format!(
        "export RAYON_NUM_THREADS={cpus} NM_SIM_PAR_MAX_WORKERS={cpus} OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 NM_DISABLE_OPENCL=1\n"
    );
    s += &format!(
        "exec {} worker --job {} --out {}\n",
        quote(&exe.display().to_string()),
        quote(&job.display().to_string()),
        quote(&out.display().to_string())
    );
    Ok(s)
}

/// Resources and pass-through options for one submitted job.
#[derive(Debug, Clone)]
pub struct Submission<'a> {
    pub cpus: u32,
    pub mem_mib: u64,
    pub time: &'a str,
    pub partition: Option<&'a str>,
    /// Extra `sbatch` options, each one argument starting with `--`
    /// (e.g. `--nodelist=qc02`), passed as structured arguments.
    pub sbatch_args: &'a [String],
}

impl Default for Submission<'_> {
    fn default() -> Self {
        Self {
            cpus: 1,
            mem_mib: 4096,
            time: "00:30:00",
            partition: None,
            sbatch_args: &[],
        }
    }
}

fn check_sbatch_arg(a: &str) -> Result<()> {
    let ok = a.len() > 2
        && a.starts_with("--")
        && a[2..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_=,.:/[]".contains(c));
    if ok {
        Ok(())
    } else {
        invalid(format!("sbatch argument {a:?}: expected --option[=value]"))
    }
}

pub fn submit(
    job: &Path,
    exe: &Path,
    dry_run: bool,
    options: &Submission,
    runner: &mut dyn CommandRunner,
) -> Result<Value> {
    for a in options.sbatch_args {
        check_sbatch_arg(a)?;
    }
    let script = render(
        job,
        exe,
        options.cpus,
        options.mem_mib,
        options.time,
        options.partition,
    )?;
    let path = job.with_file_name("sbatch.sh");
    std::fs::write(&path, &script)?;
    let mut args = vec!["--parsable".to_string()];
    args.extend(options.sbatch_args.iter().cloned());
    args.push(path.display().to_string());
    if dry_run {
        let command: Vec<String> = std::iter::once("sbatch".to_string()).chain(args).collect();
        return Ok(
            json!({"dry_run": true, "command": command, "script": path.display().to_string()}),
        );
    }
    let (code, out, err) = runner.run("sbatch", &args)?;
    if code != 0 {
        return Err(ExplorerError::Unsupported(format!(
            "sbatch failed ({code}): {err}"
        )));
    }
    let id = out.trim().split(';').next().unwrap_or("").to_string();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(ExplorerError::Internal(format!(
            "unexpected sbatch output {out:?}"
        )));
    }
    Ok(json!({"submitted": id, "script": path.display().to_string()}))
}

fn check_id(id: &str) -> Result<()> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        invalid(format!("slurm job id {id:?}"))
    } else {
        Ok(())
    }
}

pub fn status(id: &str, runner: &mut dyn CommandRunner) -> Result<Value> {
    check_id(id)?;
    let (code, out, err) = runner.run(
        "squeue",
        &[
            "-h".into(),
            "-j".into(),
            id.into(),
            "-o".into(),
            "%T".into(),
        ],
    )?;
    let state = out.trim();
    Ok(
        json!({"id": id, "state": if code == 0 && !state.is_empty() { state.to_string() } else { "not-queued (finished or unknown; reconcile with the result file)".into() }, "stderr": err.trim()}),
    )
}

pub fn cancel(id: &str, runner: &mut dyn CommandRunner) -> Result<Value> {
    check_id(id)?;
    let (code, _, err) = runner.run("scancel", &[id.into()])?;
    Ok(json!({"id": id, "cancelled": code == 0, "stderr": err.trim()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<(String, Vec<String>)>, (i32, String, String));
    impl CommandRunner for Fake {
        fn run(&mut self, p: &str, a: &[String]) -> Result<(i32, String, String)> {
            self.0.push((p.into(), a.to_vec()));
            Ok(self.1.clone())
        }
    }

    fn job_file(dir: &Path) -> std::path::PathBuf {
        let j = crate::worker::Job {
            schema: crate::worker::JOB_SCHEMA.into(),
            job_id: "job-abc".into(),
            snapshot_path: "/x".into(),
            snapshot_sha256: "0".repeat(64),
            suite: crate::tasks::TaskSuite::parse(crate::pipeline::DEFAULT_SUITE).unwrap(),
            policy: "frozen".into(),
            interventions: vec![],
            lower_neurons: vec![],
            lower_synapses: vec![],
            protect_io: true,
            capabilities: vec![],
            job: crate::worker::JobKind::Evaluate {
                split: "development".into(),
                reference: None,
            },
        };
        let p = dir.join("job.json");
        std::fs::write(&p, serde_json::to_vec(&j).unwrap()).unwrap();
        p
    }

    #[test]
    fn render_submit_status_cancel_via_fakes() {
        let d = tempfile::tempdir().unwrap();
        let job = job_file(d.path());
        let s = render(
            &job,
            Path::new("/opt/aarnn-explorer"),
            2,
            2048,
            "01:00:00",
            Some("arm64"),
        )
        .unwrap();
        assert!(
            s.contains("#SBATCH --cpus-per-task=2")
                && s.contains("--partition=arm64")
                && s.contains("worker --job")
        );
        assert!(
            render(&job, Path::new("/x"), 1, 2048, "1;rm", None).is_err(),
            "time is validated"
        );
        let mut f = Fake(vec![], (0, "4242;cluster\n".into(), String::new()));
        let none = Submission::default();
        let dry = submit(&job, Path::new("/x"), true, &none, &mut f).unwrap();
        assert!(f.0.is_empty(), "dry run never calls sbatch: {dry}");
        let pin = vec!["--nodelist=qc02".to_string()];
        let opts = Submission {
            cpus: 2,
            sbatch_args: &pin,
            ..Submission::default()
        };
        assert_eq!(
            submit(&job, Path::new("/opt/x"), false, &opts, &mut f).unwrap()["submitted"],
            "4242"
        );
        assert_eq!(f.0[0].0, "sbatch");
        assert_eq!(f.0[0].1[..2], ["--parsable", "--nodelist=qc02"]);
        let script = std::fs::read_to_string(job.with_file_name("sbatch.sh")).unwrap();
        assert!(script.contains("--cpus-per-task=2") && script.contains("'/opt/x' worker"));
        for bad in ["-w qc02", "--nodelist=qc02;rm", "--", "nodelist=qc02"] {
            let a = vec![bad.to_string()];
            let o = Submission {
                sbatch_args: &a,
                ..Submission::default()
            };
            assert!(
                submit(&job, Path::new("/x"), false, &o, &mut f).is_err(),
                "{bad:?} is refused"
            );
        }
        assert_eq!(f.0.len(), 1, "refused arguments never reach sbatch");
        let mut q = Fake(vec![], (0, "RUNNING\n".into(), String::new()));
        assert_eq!(status("4242", &mut q).unwrap()["state"], "RUNNING");
        assert!(status("42; rm -rf /", &mut q).is_err());
        let mut c = Fake(vec![], (0, String::new(), String::new()));
        assert_eq!(cancel("4242", &mut c).unwrap()["cancelled"], true);
        assert_eq!(c.0[0], ("scancel".to_string(), vec!["4242".to_string()]));
    }
}
