use crate::config::{GcodeDialect, MachineProfile};
use crate::model::{ToolpathBundle, ToolpathOperation};
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub fn render(machine: &MachineProfile, bundle: &ToolpathBundle) -> Result<String> {
    let dialect = &machine.gcode;
    let mut out = String::new();

    for line in &dialect.header {
        out.push_str(line);
        out.push('\n');
    }

    for op in &bundle.operations {
        let line = match op {
            ToolpathOperation::Comment { text } => format!("; {}", text),
            ToolpathOperation::SelectTool { tool, material_id } => render_template(
                &dialect.select_tool,
                vars(&[
                    ("tool", tool.to_string()),
                    ("material", material_id.clone()),
                ]),
            )?,
            ToolpathOperation::SetPressure { tool, pressure_kpa } => render_template(
                &dialect.set_pressure,
                vars(&[
                    ("tool", tool.to_string()),
                    ("pressure_kpa", format!("{pressure_kpa:.3}")),
                ]),
            )?,
            ToolpathOperation::SetTemperature {
                tool,
                temperature_c,
            } => render_template(
                &dialect.set_temperature,
                vars(&[
                    ("tool", tool.to_string()),
                    ("temperature_c", format!("{temperature_c:.3}")),
                ]),
            )?,
            ToolpathOperation::Move {
                position_mm,
                feed_mm_min,
            } => render_template(
                &dialect.move_linear,
                vars(&[
                    ("x", format!("{:.4}", position_mm.x)),
                    ("y", format!("{:.4}", position_mm.y)),
                    ("z", format!("{:.4}", position_mm.z)),
                    ("feed_mm_min", format!("{feed_mm_min:.3}")),
                ]),
            )?,
            ToolpathOperation::ExtrusionStart { tool } => render_template(
                &dialect.extrusion_start,
                vars(&[("tool", tool.to_string())]),
            )?,
            ToolpathOperation::ExtrusionStop { tool } => {
                render_template(&dialect.extrusion_stop, vars(&[("tool", tool.to_string())]))?
            }
            ToolpathOperation::DwellMs { milliseconds } => render_template(
                &dialect.dwell_ms,
                vars(&[("milliseconds", milliseconds.to_string())]),
            )?,
            ToolpathOperation::Cure {
                wavelength_nm,
                seconds,
            } => render_template(
                &dialect.cure,
                vars(&[
                    ("wavelength_nm", wavelength_nm.to_string()),
                    ("seconds", format!("{seconds:.3}")),
                ]),
            )?,
            ToolpathOperation::PauseForComponent { message } => {
                format!("; PAUSE_FOR_COMPONENT {}", message)
            }
        };
        if !line.trim().is_empty() {
            out.push_str(&line);
            out.push('\n');
        }
    }

    for line in &dialect.footer {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

fn vars(items: &[(&str, String)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn render_template(template: &str, vars: BTreeMap<String, String>) -> Result<String> {
    let mut rendered = template.to_owned();
    for (key, value) in vars {
        rendered = rendered.replace(&format!("{{{key}}}"), &value);
    }
    if rendered.contains('{') || rendered.contains('}') {
        return Err(anyhow!(
            "unresolved variable in G-code template: {rendered}"
        ));
    }
    Ok(rendered)
}

#[allow(dead_code)]
fn _dialect_reference(_: &GcodeDialect) {}
