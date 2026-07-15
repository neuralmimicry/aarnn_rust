use crate::calibration::CalibrationSet;
use crate::config::MachineProfile;
use crate::model::*;
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub fn plan_toolpaths(
    plan: &PhysicalPlan,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<ToolpathBundle> {
    let mut operations = Vec::new();
    let mut volumes = BTreeMap::new();

    operations.push(ToolpathOperation::Comment {
        text: format!(
            "AARNN network {} tick {}",
            plan.source_network_id, plan.source_tick
        ),
    });
    operations.push(ToolpathOperation::Comment {
        text: "Generated toolpath is a review artefact; validate in DNA Studio and against the installed BIO X6 firmware.".into(),
    });

    let core_id = &calibration.mapping.core_material_id;
    let cladding_id = &calibration.mapping.cladding_material_id;
    let core_tool = machine
        .tools
        .iter()
        .find(|t| &t.material_id == core_id)
        .ok_or_else(|| anyhow!("no tool assigned to core material {core_id}"))?;
    let cladding_tool = machine
        .tools
        .iter()
        .find(|t| &t.material_id == cladding_id)
        .ok_or_else(|| anyhow!("no tool assigned to cladding material {cladding_id}"))?;

    operations.push(ToolpathOperation::SelectTool {
        tool: cladding_tool.tool,
        material_id: cladding_id.clone(),
    });
    operations.push(ToolpathOperation::SetPressure {
        tool: cladding_tool.tool,
        pressure_kpa: cladding_tool.pressure_kpa,
    });
    if let Some(temp) = cladding_tool.temperature_c {
        operations.push(ToolpathOperation::SetTemperature {
            tool: cladding_tool.tool,
            temperature_c: temp,
        });
    }

    for edge in &plan.connections {
        operations.push(ToolpathOperation::Comment {
            text: format!("cladding for edge {}", edge.id),
        });
        let first = edge
            .path_mm
            .first()
            .ok_or_else(|| anyhow!("edge {} has empty path", edge.id))?;
        operations.push(ToolpathOperation::Move {
            position_mm: *first,
            feed_mm_min: machine.travel_feed_mm_min,
        });
        operations.push(ToolpathOperation::ExtrusionStart {
            tool: cladding_tool.tool,
        });
        for point in edge.path_mm.iter().skip(1) {
            operations.push(ToolpathOperation::Move {
                position_mm: *point,
                feed_mm_min: machine.default_feed_mm_min,
            });
        }
        operations.push(ToolpathOperation::ExtrusionStop {
            tool: cladding_tool.tool,
        });
    }

    operations.push(ToolpathOperation::SelectTool {
        tool: core_tool.tool,
        material_id: core_id.clone(),
    });
    operations.push(ToolpathOperation::SetPressure {
        tool: core_tool.tool,
        pressure_kpa: core_tool.pressure_kpa,
    });
    if let Some(temp) = core_tool.temperature_c {
        operations.push(ToolpathOperation::SetTemperature {
            tool: core_tool.tool,
            temperature_c: temp,
        });
    }

    for edge in &plan.connections {
        operations.push(ToolpathOperation::Comment {
            text: format!(
                "edge {} {} -> {} {:?}",
                edge.id, edge.source_node, edge.target_node, edge.implementation
            ),
        });
        let first = edge
            .path_mm
            .first()
            .ok_or_else(|| anyhow!("edge {} has empty path", edge.id))?;
        operations.push(ToolpathOperation::Move {
            position_mm: *first,
            feed_mm_min: machine.travel_feed_mm_min,
        });
        operations.push(ToolpathOperation::ExtrusionStart {
            tool: core_tool.tool,
        });
        for point in edge.path_mm.iter().skip(1) {
            operations.push(ToolpathOperation::Move {
                position_mm: *point,
                feed_mm_min: machine.default_feed_mm_min,
            });
        }
        operations.push(ToolpathOperation::ExtrusionStop {
            tool: core_tool.tool,
        });
    }

    let fluorescent_id = &calibration.mapping.fluorescent_node_material_id;
    let node_tool = machine
        .tools
        .iter()
        .find(|t| &t.material_id == fluorescent_id)
        .ok_or_else(|| anyhow!("no tool assigned to node material {fluorescent_id}"))?;

    operations.push(ToolpathOperation::SelectTool {
        tool: node_tool.tool,
        material_id: fluorescent_id.clone(),
    });
    operations.push(ToolpathOperation::SetPressure {
        tool: node_tool.tool,
        pressure_kpa: node_tool.pressure_kpa,
    });

    for node in &plan.nodes {
        operations.push(ToolpathOperation::Comment {
            text: format!(
                "node {} threshold_mw={:.6} realisation={:?}",
                node.id, node.optical_threshold_mw, node.realisation
            ),
        });
        operations.push(ToolpathOperation::Move {
            position_mm: node.position_mm,
            feed_mm_min: machine.travel_feed_mm_min,
        });
        operations.push(ToolpathOperation::ExtrusionStart {
            tool: node_tool.tool,
        });
        operations.push(ToolpathOperation::DwellMs { milliseconds: 250 });
        operations.push(ToolpathOperation::ExtrusionStop {
            tool: node_tool.tool,
        });
        if matches!(node.realisation, NodeRealisation::OptoelectronicRegenerator) {
            operations.push(ToolpathOperation::PauseForComponent {
                message: format!(
                    "Insert detector/comparator/emitter assembly for node {} before continuing.",
                    node.id
                ),
            });
        }
    }

    for material in calibration.materials.values() {
        if let Some(cure) = &material.cure {
            operations.push(ToolpathOperation::Cure {
                wavelength_nm: cure.wavelength_nm,
                seconds: cure.seconds_per_layer,
            });
        }
    }

    volumes.insert(core_id.clone(), plan.metrics.estimated_core_volume_ml);
    volumes.insert(
        calibration.mapping.cladding_material_id.clone(),
        plan.metrics.estimated_cladding_volume_ml,
    );

    Ok(ToolpathBundle {
        operations,
        material_volumes_ml: volumes,
    })
}
