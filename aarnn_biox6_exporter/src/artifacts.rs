use crate::calibration::CalibrationSet;
use crate::config::MachineProfile;
use crate::model::{ExportBundle, PhysicalPlan, ToolpathBundle};
use crate::preview::{render_preview_html, render_preview_svg};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExportArtifact {
    pub relative_path: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct VerificationReport<'a> {
    schema_version: &'static str,
    source_network_id: &'a str,
    source_tick: u64,
    calibration_id: &'a str,
    machine_profile_id: &'a str,
    printer_ready: bool,
    printer_ready_reason: String,
    warnings_count: usize,
    warnings: &'a [crate::model::PlanWarning],
    metrics: &'a crate::model::PlanMetrics,
}

pub fn export_artifacts(
    bundle: &ExportBundle,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<Vec<ExportArtifact>> {
    let mut artifacts = Vec::new();
    push_json(
        &mut artifacts,
        "physical-plan.json",
        &bundle.plan,
        "application/json",
    )?;
    push_json(
        &mut artifacts,
        "toolpaths.json",
        &bundle.toolpaths,
        "application/json",
    )?;
    push_json(
        &mut artifacts,
        "verification.json",
        &VerificationReport {
            schema_version: "0.1",
            source_network_id: &bundle.plan.source_network_id,
            source_tick: bundle.plan.source_tick,
            calibration_id: &calibration.id,
            machine_profile_id: &machine.id,
            printer_ready: false,
            printer_ready_reason: printer_ready_reason(&bundle.plan),
            warnings_count: bundle.plan.warnings.len(),
            warnings: &bundle.plan.warnings,
            metrics: &bundle.plan.metrics,
        },
        "application/json",
    )?;
    push_text(
        &mut artifacts,
        "provenance.csv",
        &render_provenance_csv(&bundle.plan),
        "text/csv",
    );
    push_text(
        &mut artifacts,
        "materials.yaml",
        &serde_yaml::to_string(calibration)?,
        "application/yaml",
    );
    push_text(
        &mut artifacts,
        "machine-setup.yaml",
        &serde_yaml::to_string(machine)?,
        "application/yaml",
    );
    push_text(&mut artifacts, "job.gcode", &bundle.gcode, "text/plain");
    push_text(
        &mut artifacts,
        "README-PRINT.txt",
        &readme_print(&bundle.plan, machine, calibration),
        "text/plain",
    );
    push_text(
        &mut artifacts,
        "preview/preview.svg",
        &render_preview_svg(&bundle.plan, machine),
        "image/svg+xml",
    );
    push_text(
        &mut artifacts,
        "preview/preview.html",
        &render_preview_html(&bundle.plan, machine, calibration),
        "text/html",
    );
    push_text(
        &mut artifacts,
        "preview/core.stl",
        &render_core_stl(&bundle.plan, false),
        "model/stl",
    );
    push_text(
        &mut artifacts,
        "preview/cladding.stl",
        &render_core_stl(&bundle.plan, true),
        "model/stl",
    );
    push_text(
        &mut artifacts,
        "preview/fluorescent-nodes.stl",
        &render_nodes_stl(&bundle.plan),
        "model/stl",
    );
    push_text(
        &mut artifacts,
        "preview/attenuators.stl",
        &render_empty_stl("attenuators"),
        "model/stl",
    );
    push_text(
        &mut artifacts,
        "preview/support.stl",
        &render_empty_stl("support"),
        "model/stl",
    );
    Ok(artifacts)
}

pub fn write_artifacts(output_dir: &Path, artifacts: &[ExportArtifact]) -> Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    for artifact in artifacts {
        let path = output_dir.join(&artifact.relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&path, &artifact.bytes)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

pub fn zip_artifacts(artifacts: &[ExportArtifact]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for artifact in artifacts {
        let name = artifact.relative_path.as_bytes();
        let data = &artifact.bytes;
        let crc = crc32(data);
        let offset = out.len() as u32;
        write_u32(&mut out, 0x0403_4b50);
        write_u16(&mut out, 20);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u32(&mut out, crc);
        write_u32(&mut out, data.len() as u32);
        write_u32(&mut out, data.len() as u32);
        write_u16(&mut out, name.len() as u16);
        write_u16(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        write_u32(&mut central, 0x0201_4b50);
        write_u16(&mut central, 20);
        write_u16(&mut central, 20);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, crc);
        write_u32(&mut central, data.len() as u32);
        write_u32(&mut central, data.len() as u32);
        write_u16(&mut central, name.len() as u16);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u16(&mut central, 0);
        write_u32(&mut central, 0);
        write_u32(&mut central, offset);
        central.extend_from_slice(name);
    }

    let central_offset = out.len() as u32;
    let central_len = central.len() as u32;
    out.extend_from_slice(&central);
    write_u32(&mut out, 0x0605_4b50);
    write_u16(&mut out, 0);
    write_u16(&mut out, 0);
    write_u16(&mut out, artifacts.len() as u16);
    write_u16(&mut out, artifacts.len() as u16);
    write_u32(&mut out, central_len);
    write_u32(&mut out, central_offset);
    write_u16(&mut out, 0);
    Ok(out)
}

fn push_json(
    artifacts: &mut Vec<ExportArtifact>,
    relative_path: &str,
    value: &impl Serialize,
    content_type: &str,
) -> Result<()> {
    artifacts.push(ExportArtifact {
        relative_path: relative_path.to_string(),
        content_type: content_type.to_string(),
        bytes: serde_json::to_vec_pretty(value)?,
    });
    Ok(())
}

fn push_text(
    artifacts: &mut Vec<ExportArtifact>,
    relative_path: &str,
    text: &str,
    content_type: &str,
) {
    artifacts.push(ExportArtifact {
        relative_path: relative_path.to_string(),
        content_type: content_type.to_string(),
        bytes: text.as_bytes().to_vec(),
    });
}

fn readme_print(
    plan: &PhysicalPlan,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> String {
    let best_effort_note = if has_best_effort_geometry_warning(plan) {
        "\nBest-effort geometry note: this export contains unresolved routing or physical validation warnings. Use it for preview, measurement planning, and debugging only; do not print until the listed conflicts are corrected.\n"
    } else {
        ""
    };
    format!(
        "AARNN BIO X6 physical-network export\n\nNetwork: {}\nTick: {}\nMachine profile: {} ({})\nCalibration: {}\n\nThis output is not marked printer-ready. Review preview/preview.html, physical-plan.json, verification.json and job.gcode in CELLINK DNA Studio. Replace or verify every machine command template before running this job on hardware.{}\n\nPhysical limitation note: centimetre-scale optical paths only provide nanosecond-scale propagation delay. Residual delays in the physical plan are assigned to fluorescence, responsive material, time-bin or external electronic implementation strategies.\n",
        plan.source_network_id,
        plan.source_tick,
        machine.id,
        machine.model,
        calibration.id,
        best_effort_note,
    )
}

fn printer_ready_reason(plan: &PhysicalPlan) -> String {
    if has_best_effort_geometry_warning(plan) {
        return "Generated as a best-effort BIO X6 review artefact. Routing or physical non-overlap validation reported unresolved geometry conflicts, and machine command templates still require printer-profile verification.".into();
    }
    "Generated BIO X6 output is a DNA Studio review artefact until every command template is verified for the installed printer profile.".into()
}

fn has_best_effort_geometry_warning(plan: &PhysicalPlan) -> bool {
    plan.warnings.iter().any(|warning| {
        matches!(
            warning.code.as_str(),
            "PORT_BEST_EFFORT_OVERLAP"
                | "PORT_DENSE_ALLOCATION_BEST_EFFORT"
                | "ROUTE_BEST_EFFORT_OVERLAP"
                | "ROUTE_BEST_EFFORT_WARNINGS_TRUNCATED"
                | "DENSE_ROUTING_DETERMINISTIC_BEST_EFFORT"
                | "ROUTING_COMPLETE_BEST_EFFORT"
                | "ROUTING_GRID_DEGRADED_BEST_EFFORT"
                | "ROUTING_LANES_REUSED_BEST_EFFORT"
                | "PHYSICAL_VALIDATION_EXHAUSTIVE_SKIPPED_BEST_EFFORT"
                | "PHYSICAL_VALIDATION_FAILED_BEST_EFFORT"
        )
    })
}

fn render_provenance_csv(plan: &PhysicalPlan) -> String {
    let mut out = String::from("object_id,object_kind,source_component_ids\n");
    for node in &plan.nodes {
        out.push_str(&format!(
            "{},node,\"{}\"\n",
            csv_escape(&node.id),
            csv_escape(&node.source_component_ids.join(";"))
        ));
    }
    for edge in &plan.connections {
        out.push_str(&format!(
            "{},connection,\"{}\"\n",
            csv_escape(&edge.id),
            csv_escape(&edge.source_component_ids.join(";"))
        ));
    }
    out
}

fn render_core_stl(plan: &PhysicalPlan, cladding: bool) -> String {
    let name = if cladding { "cladding" } else { "core" };
    let mut out = format!("solid {name}\n");
    let width = if cladding { 0.9 } else { 0.45 };
    for edge in &plan.connections {
        for pair in edge.path_mm.windows(2) {
            let a = pair[0];
            let b = pair[1];
            out.push_str(&facet_for_segment(a.x, a.y, a.z, b.x, b.y, b.z, width));
        }
    }
    out.push_str(&format!("endsolid {name}\n"));
    out
}

fn render_nodes_stl(plan: &PhysicalPlan) -> String {
    let mut out = String::from("solid fluorescent_nodes\n");
    for node in &plan.nodes {
        let p = node.position_mm;
        let r = node.radius_mm.max(0.5);
        out.push_str(&facet(
            p.x - r,
            p.y,
            p.z,
            p.x,
            p.y + r,
            p.z,
            p.x,
            p.y,
            p.z + r,
        ));
        out.push_str(&facet(
            p.x + r,
            p.y,
            p.z,
            p.x,
            p.y - r,
            p.z,
            p.x,
            p.y,
            p.z + r,
        ));
        out.push_str(&facet(
            p.x - r,
            p.y,
            p.z,
            p.x,
            p.y - r,
            p.z,
            p.x,
            p.y,
            p.z - r,
        ));
        out.push_str(&facet(
            p.x + r,
            p.y,
            p.z,
            p.x,
            p.y + r,
            p.z,
            p.x,
            p.y,
            p.z - r,
        ));
    }
    out.push_str("endsolid fluorescent_nodes\n");
    out
}

fn render_empty_stl(name: &str) -> String {
    format!("solid {name}\nendsolid {name}\n")
}

fn facet_for_segment(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64, width: f64) -> String {
    facet(ax, ay, az, bx, by, bz, ax, ay + width, az)
}

fn facet(
    ax: f64,
    ay: f64,
    az: f64,
    bx: f64,
    by: f64,
    bz: f64,
    cx: f64,
    cy: f64,
    cz: f64,
) -> String {
    format!(
        "  facet normal 0 0 0\n    outer loop\n      vertex {ax:.6} {ay:.6} {az:.6}\n      vertex {bx:.6} {by:.6} {bz:.6}\n      vertex {cx:.6} {cy:.6} {cz:.6}\n    endloop\n  endfacet\n"
    )
}

fn csv_escape(value: &str) -> String {
    value.replace('"', "\"\"")
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[allow(dead_code)]
fn _toolpath_reference(_: &ToolpathBundle) {}
