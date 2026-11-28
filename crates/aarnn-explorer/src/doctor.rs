// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! `doctor`: read-only capability and resource-enforcement report. Nothing
//! here mutates infrastructure or probes physical devices.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn which(prog: &str) -> Option<String> {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {prog}")])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && !s.is_empty() {
        Some(s)
    } else {
        None
    }
}

pub fn report(project: Option<&Path>) -> Value {
    let systemd = crate::worker::systemd_scope_available();
    let cgroup2 = Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    let mujoco_dir = std::env::var("AARNN_MUJOCO_DIR").ok();
    let adapter = ["target/release/aarnn-mujoco", "target/debug/aarnn-mujoco"]
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|s| s.to_string());
    let project_status = project.map(|p| match crate::store::Project::open(p) {
        Ok(pr) => {
            let idx = pr.read_index().ok();
            json!({
                "path": p.display().to_string(), "schema": pr.manifest.schema_version, "id": pr.manifest.project_id,
                "sources": idx.as_ref().map(|i| i.sources.len()), "results": idx.as_ref().map(|i| i.results.len()),
                "candidates": idx.as_ref().map(|i| i.candidates.len()), "selected": idx.as_ref().and_then(|i| i.aliases.get("selected").cloned()),
                "journal_entries": pr.journal_lines().map(|l| l.len()).unwrap_or(0),
            })
        }
        Err(e) => json!({"path": p.display().to_string(), "error": e.to_string()}),
    });
    json!({
        "tool": "aarnn-explorer", "version": env!("CARGO_PKG_VERSION"),
        "features": {"web-ui": cfg!(feature = "web-ui")},
        "target": {"arch": std::env::consts::ARCH, "os": std::env::consts::OS},
        "backends": {
            "legacy-runner": "available: aarnn_rust::runner::Runner (NeuronModel::Aarnn), declared-cold-start",
            "stable-shard-executor-trace": "unsupported: StableShardExecutor exposes no event-observer seam yet",
            "accelerator": "not admitted: the explorer does not use GPU/OpenCL/CUDA",
            "fpaa": "not probed: physical device; see the main runtime's FPAA status",
        },
        "resource_enforcement": {
            "systemd_user_scope": if systemd { "enforced" } else { "unavailable" },
            "cgroup_v2": if cgroup2 { "present" } else { "absent" },
            "effective": if systemd { "enforced (MemoryMax, CPUQuota, TasksMax per worker)" } else { "advisory (internal admission limits only)" },
            "worker_threads": "RAYON_NUM_THREADS / NM_SIM_PAR_MAX_WORKERS = threads_per_worker; OPENBLAS/OMP = 1",
        },
        "closed_loop": {
            "authored-heading-plant": "available (synthetic task environment, not an embodiment)",
            "mujoco-adapter": match (&adapter, &mujoco_dir) {
                (Some(a), Some(d)) => format!("available: {a} with AARNN_MUJOCO_DIR={d}"),
                (Some(a), None) => format!("binary {a} present; AARNN_MUJOCO_DIR unset"),
                _ => "unavailable: build with cargo build -p aarnn-mujoco".into(),
            },
        },
        "build_requirements": {
            "protoc": which("protoc").unwrap_or_else(|| "missing (needed by the root build.rs)".into()),
            "blas": "not required at runtime (ndarray blas feature links no BLAS in this workspace)",
            "slurm_sbatch": which("sbatch").unwrap_or_else(|| "unavailable (optional launcher)".into()),
        },
        "project": project_status,
        "data_schemas": ["aarnn-explorer-sparse-v1", "aarnn-native-snapshot", "trace v1", "job aarnn-explorer-job-v1", "bundle aarnn-explorer-bundle-v1"],
    })
}
