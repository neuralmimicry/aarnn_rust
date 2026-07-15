use aarnn_biox6_exporter::{
    build_plan, export_artifacts, export_bundle, load_calibration, load_machine,
    render_preview_html, snapshot_from_json_str, write_artifacts,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Validate {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        machine: PathBuf,
        #[arg(long)]
        calibration: PathBuf,
    },
    Plan {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        machine: PathBuf,
        #[arg(long)]
        calibration: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Export {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        machine: PathBuf,
        #[arg(long)]
        calibration: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
    Preview {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        machine: PathBuf,
        #[arg(long)]
        calibration: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        network_id: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate {
            input,
            machine,
            calibration,
        } => {
            let raw = std::fs::read_to_string(&input)
                .with_context(|| format!("failed to read {}", input.display()))?;
            let snapshot = snapshot_from_json_str(&raw, None)?;
            let machine = load_machine(&machine)?;
            let calibration = load_calibration(&calibration)?;
            let plan = build_plan(&snapshot, &machine, &calibration)?;
            println!(
                "valid: {} nodes, {} connections, {} warnings",
                plan.nodes.len(),
                plan.connections.len(),
                plan.warnings.len()
            );
        }
        Command::Plan {
            input,
            machine,
            calibration,
            output,
        } => {
            let raw = std::fs::read_to_string(&input)
                .with_context(|| format!("failed to read {}", input.display()))?;
            let snapshot = snapshot_from_json_str(&raw, None)?;
            let machine = load_machine(&machine)?;
            let calibration = load_calibration(&calibration)?;
            let plan = build_plan(&snapshot, &machine, &calibration)?;
            write_json(&output, &plan)?;
            println!("wrote {}", output.display());
        }
        Command::Export {
            input,
            machine,
            calibration,
            output_dir,
        } => {
            let raw = std::fs::read_to_string(&input)
                .with_context(|| format!("failed to read {}", input.display()))?;
            let snapshot = snapshot_from_json_str(&raw, None)?;
            let machine = load_machine(&machine)?;
            let calibration = load_calibration(&calibration)?;
            let plan = build_plan(&snapshot, &machine, &calibration)?;
            let bundle = export_bundle(&plan, &machine, &calibration)?;
            let artifacts = export_artifacts(&bundle, &machine, &calibration)?;
            write_artifacts(&output_dir, &artifacts)?;
            println!("wrote export bundle to {}", output_dir.display());
        }
        Command::Preview {
            input,
            machine,
            calibration,
            output,
            network_id,
        } => {
            let raw = std::fs::read_to_string(&input)
                .with_context(|| format!("failed to read {}", input.display()))?;
            let snapshot = snapshot_from_json_str(&raw, network_id.as_deref())?;
            let machine = load_machine(&machine)?;
            let calibration = load_calibration(&calibration)?;
            let plan = build_plan(&snapshot, &machine, &calibration)?;
            std::fs::write(&output, render_preview_html(&plan, &machine, &calibration))
                .with_context(|| format!("failed to write {}", output.display()))?;
            println!("wrote preview to {}", output.display());
        }
    }
    Ok(())
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))
}
