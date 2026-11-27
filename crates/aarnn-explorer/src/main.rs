// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! `aarnn-explorer`: connectome explorer CLI. Every command prints a JSON
//! summary on stdout and exits with a stable code (see `error::ExitCode`).

use std::path::PathBuf;

use aarnn_explorer::error::{ExitCode, ExplorerError, Result};
use aarnn_explorer::pipeline::{self, Ctx};
use aarnn_explorer::store::Project;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    name = "aarnn-explorer",
    version,
    about = "Explore AARNN connectomes and activity, extract GNATs, run isolated interventions, and search for smaller function-preserving circuits"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check build features, backends, resource enforcement and optional tools (read-only).
    Doctor {
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Project management.
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Import a native AARNN snapshot or a sparse interchange document.
    Import {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, default_value = "aarnn-snapshot")]
        format: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        name: String,
    },
    /// Derive a new source: a learned state (--preset learned-cap),
    /// persistent interventions, and/or a candidate's removal set.
    Derive {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        interventions: Option<PathBuf>,
        #[arg(long)]
        lower_like: Option<String>,
        #[arg(long)]
        write: Option<PathBuf>,
    },
    /// Reference runs of the task suite on the source.
    Baseline {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, default_value = "development")]
        split: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        reuse_compatible: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Capture complete traces and construct GNATs, recurrence and sensitivity for a baseline run.
    Analyse {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        run: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Run an intervention plan on isolated copies (with intact and sham controls).
    Experiment {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Search for smaller circuits under the configured contract.
    Optimise {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, default_value = "min-state-bytes")]
        select: String,
        /// Analysis record used to rank hypotheses (default: <baseline>-gnat if present).
        #[arg(long)]
        analysis: Option<String>,
    },
    /// Locked assessment of a fixed candidate.
    Validate {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, default_value = "selected")]
        candidate: String,
        #[arg(long, default_value = "locked-validation")]
        split: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Export an executable candidate bundle with its evidence.
    Export {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, default_value = "selected")]
        candidate: String,
        #[arg(long)]
        output: PathBuf,
        /// Export without a locked validation (marked exploratory-unvalidated).
        #[arg(long)]
        exploratory: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Clean-process re-import and re-validation of an exported bundle.
    VerifyBundle {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        scratch: Option<PathBuf>,
    },
    /// Serve the interactive UI and job API on loopback (feature `web-ui`).
    Serve {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: String,
        #[arg(long)]
        open: bool,
    },
    /// Authored offline end-to-end scenario through real AARNN execution.
    Demo {
        #[arg(long)]
        project: PathBuf,
    },
    /// Job journal: list or reconcile interrupted jobs.
    Jobs {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        reconcile: bool,
    },
    /// Render, submit, query or cancel a Slurm launch of a worker manifest.
    Slurm {
        #[command(subcommand)]
        cmd: SlurmCmd,
    },
    /// Cross-network motif library from two or more analysis records (G2).
    Library {
        #[arg(long)]
        project: PathBuf,
        #[arg(long = "analysis", required = true)]
        analyses: Vec<String>,
    },
    /// GNAT computation-depth comparison across analysis records (VX-001).
    DepthReport {
        #[arg(long)]
        project: PathBuf,
        #[arg(long = "analysis", required = true)]
        analyses: Vec<String>,
    },
    /// Write a template output-equivalence contract for a source's IO.
    SuiteTemplate {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "3")]
        patterns: usize,
        #[arg(long, default_value = "1")]
        seed: u64,
        #[arg(long, default_value = "800")]
        duration_ms: u32,
    },
    /// Internal bounded execution protocol (one job file -> one result file).
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        job: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Create a new versioned project (never overwrites).
    Init {
        #[arg(long)]
        project: PathBuf,
        #[arg(long, default_value = "explorer-project")]
        name: String,
    },
}

#[derive(Subcommand)]
enum SlurmCmd {
    /// Print the sbatch script for a job file (no submission).
    Render {
        #[arg(long)]
        job: PathBuf,
        #[arg(long, default_value = "1")]
        cpus: u32,
        #[arg(long, default_value = "4096")]
        mem_mib: u64,
        #[arg(long, default_value = "00:30:00")]
        time: String,
        #[arg(long)]
        partition: Option<String>,
        /// Worker binary on the compute nodes (default: this binary).
        #[arg(long)]
        worker_exe: Option<PathBuf>,
    },
    /// Submit (only when explicitly invoked; --dry-run prints the command).
    Submit {
        #[arg(long)]
        job: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "1")]
        cpus: u32,
        #[arg(long, default_value = "4096")]
        mem_mib: u64,
        #[arg(long, default_value = "00:30:00")]
        time: String,
        #[arg(long)]
        partition: Option<String>,
        /// Worker binary on the compute nodes (default: this binary).
        #[arg(long)]
        worker_exe: Option<PathBuf>,
        /// Extra sbatch option, e.g. --sbatch-arg=--nodelist=qc02 (repeatable).
        #[arg(long = "sbatch-arg", allow_hyphen_values = true)]
        sbatch_args: Vec<String>,
    },
    Status {
        #[arg(long)]
        id: String,
    },
    Cancel {
        #[arg(long)]
        id: String,
    },
}

fn ctx(project: &PathBuf, config: &Option<PathBuf>) -> Result<Ctx> {
    Ctx::new(Project::open(project)?, config.as_deref())
}

fn run(cli: Cli) -> Result<Value> {
    match cli.cmd {
        Cmd::Doctor { project, json: _ } => Ok(aarnn_explorer::doctor::report(project.as_deref())),
        Cmd::Project {
            cmd: ProjectCmd::Init { project, name },
        } => {
            let p = Project::init(&project, &name)?;
            Ok(json!({"project": project.display().to_string(), "manifest": p.manifest}))
        }
        Cmd::Import {
            project,
            format,
            input,
            name,
        } => {
            let c = Ctx::new(Project::open_or_init(&project, "explorer-project")?, None)?;
            pipeline::import(&c, &format, &input, &name)
        }
        Cmd::Derive {
            project,
            source,
            name,
            preset,
            interventions,
            lower_like,
            write,
        } => {
            let c = Ctx::new(Project::open(&project)?, None)?;
            pipeline::derive(
                &c,
                source.as_deref(),
                &name,
                &pipeline::DeriveSpec {
                    preset: preset.as_deref(),
                    interventions: interventions.as_deref(),
                    lower_like: lower_like.as_deref(),
                    write: write.as_deref(),
                },
            )
        }
        Cmd::Baseline {
            project,
            source,
            split,
            label,
            reuse_compatible,
            config,
        } => pipeline::baseline(
            &ctx(&project, &config)?,
            source.as_deref(),
            &split,
            &label,
            reuse_compatible,
        ),
        Cmd::Analyse {
            project,
            run,
            config,
        } => pipeline::analyse(&ctx(&project, &config)?, &run),
        Cmd::Experiment {
            project,
            plan,
            config,
        } => pipeline::experiment(&ctx(&project, &config)?, &plan),
        Cmd::Optimise {
            project,
            config,
            source,
            select,
            analysis,
        } => {
            let c = ctx(&project, &config)?;
            let a = analysis.or_else(|| {
                c.project
                    .lookup(
                        aarnn_explorer::store::RecordKind::Analysis,
                        "intact-development-gnat",
                    )
                    .ok()
                    .map(|_| "intact-development-gnat".to_string())
            });
            aarnn_explorer::optimise::optimise(&c, source.as_deref(), &select, a.as_deref())
        }
        Cmd::Validate {
            project,
            candidate,
            split,
            config,
        } => pipeline::validate(&ctx(&project, &config)?, &candidate, &split),
        Cmd::Export {
            project,
            candidate,
            output,
            exploratory,
            config,
        } => pipeline::export(&ctx(&project, &config)?, &candidate, &output, exploratory),
        Cmd::VerifyBundle { bundle, scratch } => {
            let exe = std::env::current_exe()?;
            let s = scratch.unwrap_or_else(|| {
                std::env::temp_dir().join(format!("aarnn-explorer-verify-{}", std::process::id()))
            });
            pipeline::verify_bundle(&bundle, &s, &exe)
        }
        Cmd::Serve {
            project,
            listen,
            open,
        } => aarnn_explorer::api::serve(&project, &listen, open),
        Cmd::Demo { project } => pipeline::demo(&project),
        Cmd::Jobs { project, reconcile } => {
            aarnn_explorer::jobs::list(&Project::open(&project)?, reconcile)
        }
        Cmd::Slurm { cmd } => match cmd {
            SlurmCmd::Render {
                job,
                cpus,
                mem_mib,
                time,
                partition,
                worker_exe,
            } => {
                let exe = match worker_exe {
                    Some(p) => p,
                    None => std::env::current_exe()?,
                };
                Ok(
                    json!({"script": aarnn_explorer::slurm::render(&job, &exe, cpus, mem_mib, &time, partition.as_deref())?}),
                )
            }
            SlurmCmd::Submit {
                job,
                dry_run,
                cpus,
                mem_mib,
                time,
                partition,
                worker_exe,
                sbatch_args,
            } => {
                let exe = match worker_exe {
                    Some(p) => p,
                    None => std::env::current_exe()?,
                };
                let options = aarnn_explorer::slurm::Submission {
                    cpus,
                    mem_mib,
                    time: &time,
                    partition: partition.as_deref(),
                    sbatch_args: &sbatch_args,
                };
                aarnn_explorer::slurm::submit(
                    &job,
                    &exe,
                    dry_run,
                    &options,
                    &mut aarnn_explorer::slurm::SystemRunner,
                )
            }
            SlurmCmd::Status { id } => {
                aarnn_explorer::slurm::status(&id, &mut aarnn_explorer::slurm::SystemRunner)
            }
            SlurmCmd::Cancel { id } => {
                aarnn_explorer::slurm::cancel(&id, &mut aarnn_explorer::slurm::SystemRunner)
            }
        },
        Cmd::Library { project, analyses } => {
            aarnn_explorer::library::build(&Ctx::new(Project::open(&project)?, None)?, &analyses)
        }
        Cmd::DepthReport { project, analyses } => aarnn_explorer::library::depth_report(
            &Ctx::new(Project::open(&project)?, None)?,
            &analyses,
        ),
        Cmd::SuiteTemplate {
            project,
            source,
            output,
            patterns,
            seed,
            duration_ms,
        } => {
            let p = Project::open(&project)?;
            let rec: Value =
                p.get_json(&p.lookup(aarnn_explorer::store::RecordKind::Source, &source)?)?;
            let snap: Value = p.get_json(rec["snapshot_object"].as_str().ok_or_else(|| {
                ExplorerError::Unsupported(format!("{source} is not executable"))
            })?)?;
            let text = aarnn_explorer::library::suite_template(
                &snap,
                &format!("{source}-template"),
                patterns,
                seed,
                duration_ms,
            )?;
            if output.exists() {
                return Err(ExplorerError::Conflict(format!(
                    "{} exists",
                    output.display()
                )));
            }
            std::fs::write(&output, text)?;
            Ok(json!({"suite": output.display().to_string(), "tasks": patterns + 1}))
        }
        Cmd::Worker { job, out } => {
            let r = aarnn_explorer::worker::run_job(&job, &out)?;
            Ok(json!({"job": r.job_id, "status": r.status}))
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let worker = matches!(cli.cmd, Cmd::Worker { .. });
    match run(cli) {
        Ok(v) => {
            if !worker {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            }
            let code = if v.get("result").and_then(|r| r.as_str()) == Some("negative") {
                ExitCode::FailedAcceptance
            } else {
                ExitCode::Ok
            };
            std::process::exit(code as i32);
        }
        Err(e) => {
            let code = e.exit_code();
            eprintln!("aarnn-explorer: {e}");
            println!("{}", json!({"error": e.code(), "message": e.to_string()}));
            std::process::exit(code as i32);
        }
    }
}
