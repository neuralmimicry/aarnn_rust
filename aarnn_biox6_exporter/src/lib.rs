pub mod adapter;
pub mod artifacts;
pub mod calibration;
pub mod config;
pub mod gcode;
pub mod layout;
pub mod model;
pub mod preview;
pub mod toolpath;
pub mod validate;

use anyhow::{Context, Result};
pub use artifacts::{export_artifacts, write_artifacts, zip_artifacts, ExportArtifact};
pub use preview::{render_preview_html, render_preview_svg};

use calibration::CalibrationSet;
use config::MachineProfile;
use model::{AarnnSnapshot, ExportBundle, PhysicalPlan};
use std::path::Path;

pub fn load_snapshot(path: &Path) -> Result<AarnnSnapshot> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read snapshot {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("invalid AARNN snapshot JSON {}", path.display()))
}

pub fn load_machine(path: &Path) -> Result<MachineProfile> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read machine profile {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("invalid machine YAML {}", path.display()))
}

pub fn load_calibration(path: &Path) -> Result<CalibrationSet> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read calibration {}", path.display()))?;
    serde_yaml::from_str(&raw)
        .with_context(|| format!("invalid calibration YAML {}", path.display()))
}

pub fn default_machine() -> Result<MachineProfile> {
    serde_yaml::from_str(include_str!("../config/machine.example.yaml"))
        .context("invalid embedded BIO X6 machine profile")
}

pub fn default_calibration() -> Result<CalibrationSet> {
    serde_yaml::from_str(include_str!("../config/calibration.example.yaml"))
        .context("invalid embedded BIO X6 calibration profile")
}

pub fn snapshot_from_json_str(raw: &str, network_id_hint: Option<&str>) -> Result<AarnnSnapshot> {
    adapter::snapshot_from_json_str(raw, network_id_hint)
}

pub fn build_plan(
    snapshot: &AarnnSnapshot,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<PhysicalPlan> {
    validate::validate_snapshot(snapshot)?;
    validate::validate_machine(machine)?;
    validate::validate_calibration(calibration)?;
    let mut plan = layout::map_snapshot(snapshot, machine, calibration)?;
    if let Err(err) = validate::validate_plan(&mut plan, machine, calibration) {
        plan.warnings.push(model::PlanWarning {
            code: "PHYSICAL_VALIDATION_FAILED_BEST_EFFORT".into(),
            entity_id: None,
            message: format!(
                "Best-effort BIO X6 export continued after physical validation failed: {err}"
            ),
        });
    }
    Ok(plan)
}

pub fn export_bundle(
    plan: &PhysicalPlan,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<ExportBundle> {
    let toolpaths = toolpath::plan_toolpaths(plan, machine, calibration)?;
    let gcode = gcode::render(machine, &toolpaths)?;
    Ok(ExportBundle {
        plan: plan.clone(),
        toolpaths,
        gcode,
    })
}

pub fn export_snapshot_with_defaults(
    raw_snapshot_json: &str,
    network_id_hint: Option<&str>,
) -> Result<(ExportBundle, MachineProfile, CalibrationSet)> {
    let (plan, machine, calibration) =
        plan_snapshot_with_defaults(raw_snapshot_json, network_id_hint)?;
    let bundle = export_bundle(&plan, &machine, &calibration)?;
    Ok((bundle, machine, calibration))
}

pub fn plan_snapshot_with_defaults(
    raw_snapshot_json: &str,
    network_id_hint: Option<&str>,
) -> Result<(PhysicalPlan, MachineProfile, CalibrationSet)> {
    let snapshot = snapshot_from_json_str(raw_snapshot_json, network_id_hint)?;
    let machine = default_machine()?;
    let calibration = default_calibration()?;
    let plan = build_plan(&snapshot, &machine, &calibration)?;
    Ok((plan, machine, calibration))
}

pub fn preview_html_with_defaults(
    raw_snapshot_json: &str,
    network_id_hint: Option<&str>,
) -> Result<(String, PhysicalPlan)> {
    let (plan, machine, calibration) =
        plan_snapshot_with_defaults(raw_snapshot_json, network_id_hint)?;
    let html = render_preview_html(&plan, &machine, &calibration);
    Ok((html, plan))
}

pub fn zip_export_with_defaults(
    raw_snapshot_json: &str,
    network_id_hint: Option<&str>,
) -> Result<(Vec<u8>, ExportBundle)> {
    let (bundle, machine, calibration) =
        export_snapshot_with_defaults(raw_snapshot_json, network_id_hint)?;
    let artifacts = export_artifacts(&bundle, &machine, &calibration)?;
    let zip = zip_artifacts(&artifacts)?;
    Ok((zip, bundle))
}

pub fn write_export_directory_with_defaults(
    raw_snapshot_json: &str,
    network_id_hint: Option<&str>,
    output_dir: &Path,
) -> Result<ExportBundle> {
    let (bundle, machine, calibration) =
        export_snapshot_with_defaults(raw_snapshot_json, network_id_hint)?;
    let artifacts = export_artifacts(&bundle, &machine, &calibration)?;
    write_artifacts(output_dir, &artifacts)?;
    Ok(bundle)
}
