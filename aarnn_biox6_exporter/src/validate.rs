use crate::calibration::{CalibrationSet, MaterialRole};
use crate::config::MachineProfile;
use crate::model::{AarnnSnapshot, PhysicalConnection, PhysicalPlan, PlanWarning, Vec3};
use anyhow::{anyhow, bail, Result};
use std::collections::HashSet;

const GEOMETRY_EPSILON_MM: f64 = 1.0e-6;
const EXHAUSTIVE_GEOMETRY_VALIDATION_LIMIT: usize = 2_048;

pub fn validate_snapshot(snapshot: &AarnnSnapshot) -> Result<()> {
    if snapshot.nodes.is_empty() {
        bail!("snapshot contains no nodes");
    }
    let mut ids = HashSet::new();
    for node in &snapshot.nodes {
        if !ids.insert(&node.id) {
            bail!("duplicate node id {}", node.id);
        }
        if !(0.0..=1.0).contains(&node.threshold) {
            bail!("node {} threshold must be in [0,1]", node.id);
        }
    }
    let mut edge_ids = HashSet::new();
    for edge in &snapshot.connections {
        if !edge_ids.insert(&edge.id) {
            bail!("duplicate connection id {}", edge.id);
        }
        if !ids.contains(&edge.source_node) || !ids.contains(&edge.target_node) {
            bail!("edge {} references an unknown node", edge.id);
        }
        if edge.delay_us < 0.0 {
            bail!("edge {} has negative delay", edge.id);
        }
    }
    Ok(())
}

pub fn validate_machine(machine: &MachineProfile) -> Result<()> {
    if machine.maximum_tools == 0 {
        bail!("machine has zero tools");
    }
    if machine.tools.len() > machine.maximum_tools as usize {
        bail!(
            "profile assigns {} tools but machine maximum is {}",
            machine.tools.len(),
            machine.maximum_tools
        );
    }
    let mut tool_ids = HashSet::new();
    let mut material_ids = HashSet::new();
    for tool in &machine.tools {
        if !tool_ids.insert(tool.tool) {
            bail!("duplicate tool position {}", tool.tool);
        }
        if !material_ids.insert(tool.material_id.clone()) {
            bail!(
                "material {} is assigned to more than one tool",
                tool.material_id
            );
        }
        if tool.nozzle_diameter_mm <= 0.0 {
            bail!("tool {} has invalid nozzle diameter", tool.tool);
        }
        if tool.pressure_kpa < 0.0 {
            bail!("tool {} has negative pressure", tool.tool);
        }
    }
    Ok(())
}

pub fn validate_calibration(calibration: &CalibrationSet) -> Result<()> {
    let mapping = &calibration.mapping;
    let core = calibration
        .materials
        .get(&mapping.core_material_id)
        .ok_or_else(|| anyhow!("missing core material"))?;
    let cladding = calibration
        .materials
        .get(&mapping.cladding_material_id)
        .ok_or_else(|| anyhow!("missing cladding material"))?;
    if !matches!(core.role, MaterialRole::Core) {
        bail!("mapped core material does not have role core");
    }
    if !matches!(cladding.role, MaterialRole::Cladding) {
        bail!("mapped cladding material does not have role cladding");
    }
    if core.refractive_index <= cladding.refractive_index {
        bail!(
            "core refractive index {} must exceed cladding refractive index {}",
            core.refractive_index,
            cladding.refractive_index
        );
    }
    Ok(())
}

pub fn validate_plan(
    plan: &mut PhysicalPlan,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<()> {
    let max = &machine.build_volume_mm;
    for node in &plan.nodes {
        let p = node.position_mm;
        if p.x < 0.0 || p.y < 0.0 || p.z < 0.0 || p.x > max.x || p.y > max.y || p.z > max.z {
            bail!("node {} lies outside build volume", node.id);
        }
    }

    let core = calibration
        .materials
        .get(&calibration.mapping.core_material_id)
        .ok_or_else(|| anyhow!("missing core material"))?;
    for edge in &plan.connections {
        for point in &edge.path_mm {
            if point.x < 0.0
                || point.y < 0.0
                || point.z < 0.0
                || point.x > max.x
                || point.y > max.y
                || point.z > max.z
            {
                bail!("edge {} path lies outside build volume", edge.id);
            }
        }
        if edge.core_diameter_mm < core.minimum_feature_mm {
            plan.warnings.push(PlanWarning {
                code: "FEATURE_BELOW_CALIBRATED_MINIMUM".into(),
                entity_id: Some(edge.id.clone()),
                message: format!(
                    "Core diameter {:.3} mm is below calibrated minimum {:.3} mm.",
                    edge.core_diameter_mm, core.minimum_feature_mm
                ),
            });
        }
    }
    if plan.connections.len() > EXHAUSTIVE_GEOMETRY_VALIDATION_LIMIT {
        plan.warnings.push(PlanWarning {
            code: "PHYSICAL_VALIDATION_EXHAUSTIVE_SKIPPED_BEST_EFFORT".into(),
            entity_id: None,
            message: format!(
                "Skipped exhaustive pairwise non-overlap validation for {} connections; this dense export remains a best-effort review artefact and must not be treated as printer-ready.",
                plan.connections.len()
            ),
        });
    } else {
        validate_non_overlapping_geometry(plan, machine, calibration)?;
    }
    Ok(())
}

fn validate_non_overlapping_geometry(
    plan: &PhysicalPlan,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<()> {
    let cladding = calibration
        .materials
        .get(&calibration.mapping.cladding_material_id)
        .ok_or_else(|| anyhow!("missing cladding material"))?;
    let edge_radius = (calibration
        .mapping
        .default_core_diameter_mm
        .max(cladding.minimum_feature_mm)
        .max(cladding.preferred_nozzle_mm))
        / 2.0;

    for node in &plan.nodes {
        let p = node.position_mm;
        let r = node.radius_mm.max(0.0);
        if p.x - r < -GEOMETRY_EPSILON_MM
            || p.y - r < -GEOMETRY_EPSILON_MM
            || p.z - r < -GEOMETRY_EPSILON_MM
            || p.x + r > machine.build_volume_mm.x + GEOMETRY_EPSILON_MM
            || p.y + r > machine.build_volume_mm.y + GEOMETRY_EPSILON_MM
            || p.z + r > machine.build_volume_mm.z + GEOMETRY_EPSILON_MM
        {
            bail!(
                "node {} volume extends outside build volume (center {:.3},{:.3},{:.3}, radius {:.3} mm)",
                node.id,
                p.x,
                p.y,
                p.z,
                r
            );
        }
    }

    for edge in &plan.connections {
        for point in &edge.path_mm {
            if point.x - edge_radius < -GEOMETRY_EPSILON_MM
                || point.y - edge_radius < -GEOMETRY_EPSILON_MM
                || point.z - edge_radius < -GEOMETRY_EPSILON_MM
                || point.x + edge_radius > machine.build_volume_mm.x + GEOMETRY_EPSILON_MM
                || point.y + edge_radius > machine.build_volume_mm.y + GEOMETRY_EPSILON_MM
                || point.z + edge_radius > machine.build_volume_mm.z + GEOMETRY_EPSILON_MM
            {
                bail!(
                    "edge {} volume extends outside build volume at {:.3},{:.3},{:.3} (radius {:.3} mm)",
                    edge.id,
                    point.x,
                    point.y,
                    point.z,
                    edge_radius
                );
            }
        }
    }

    for i in 0..plan.nodes.len() {
        for j in (i + 1)..plan.nodes.len() {
            let a = &plan.nodes[i];
            let b = &plan.nodes[j];
            let required = a.radius_mm + b.radius_mm;
            let actual = distance(a.position_mm, b.position_mm);
            if actual < required - GEOMETRY_EPSILON_MM {
                bail!(
                    "geometry collision: node {} overlaps node {} by {:.6} mm",
                    a.id,
                    b.id,
                    required - actual
                );
            }
        }
    }

    for node in &plan.nodes {
        for edge in &plan.connections {
            if edge.path_mm.len() < 2 {
                continue;
            }
            for segment_index in 0..(edge.path_mm.len() - 1) {
                if allowed_node_edge_port_touch(&node.id, edge, segment_index) {
                    continue;
                }
                let actual = point_segment_distance(
                    node.position_mm,
                    edge.path_mm[segment_index],
                    edge.path_mm[segment_index + 1],
                );
                let required = node.radius_mm + edge_radius;
                if actual < required - GEOMETRY_EPSILON_MM {
                    bail!(
                        "geometry collision: node {} overlaps edge {} segment {} by {:.6} mm",
                        node.id,
                        edge.id,
                        segment_index,
                        required - actual
                    );
                }
            }
        }
    }

    for edge_index_a in 0..plan.connections.len() {
        let edge_a = &plan.connections[edge_index_a];
        if edge_a.path_mm.len() < 2 {
            continue;
        }
        for edge_index_b in edge_index_a..plan.connections.len() {
            let edge_b = &plan.connections[edge_index_b];
            if edge_b.path_mm.len() < 2 {
                continue;
            }
            for segment_a in 0..(edge_a.path_mm.len() - 1) {
                for segment_b in 0..(edge_b.path_mm.len() - 1) {
                    if edge_index_a == edge_index_b
                        && segments_belong_to_same_polyline_touch(segment_a, segment_b)
                    {
                        continue;
                    }
                    if edge_index_a == edge_index_b && segment_a == segment_b {
                        continue;
                    }
                    if !segment_aabb_may_overlap(
                        edge_a.path_mm[segment_a],
                        edge_a.path_mm[segment_a + 1],
                        edge_b.path_mm[segment_b],
                        edge_b.path_mm[segment_b + 1],
                        edge_radius * 2.0,
                    ) {
                        continue;
                    }
                    let actual = segment_segment_distance(
                        edge_a.path_mm[segment_a],
                        edge_a.path_mm[segment_a + 1],
                        edge_b.path_mm[segment_b],
                        edge_b.path_mm[segment_b + 1],
                    );
                    let required = edge_radius * 2.0;
                    if actual < required - GEOMETRY_EPSILON_MM {
                        bail!(
                            "geometry collision: edge {} segment {} overlaps edge {} segment {} by {:.6} mm",
                            edge_a.id,
                            segment_a,
                            edge_b.id,
                            segment_b,
                            required - actual
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

fn allowed_node_edge_port_touch(
    node_id: &str,
    edge: &PhysicalConnection,
    segment_index: usize,
) -> bool {
    if node_id == edge.source_node && segment_index == 0 {
        return true;
    }
    node_id == edge.target_node && segment_index + 2 == edge.path_mm.len()
}

fn segments_belong_to_same_polyline_touch(a: usize, b: usize) -> bool {
    a.abs_diff(b) <= 1
}

fn segment_aabb_may_overlap(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3, margin: f64) -> bool {
    let (amin_x, amax_x) = sorted_pair(a0.x, a1.x);
    let (amin_y, amax_y) = sorted_pair(a0.y, a1.y);
    let (amin_z, amax_z) = sorted_pair(a0.z, a1.z);
    let (bmin_x, bmax_x) = sorted_pair(b0.x, b1.x);
    let (bmin_y, bmax_y) = sorted_pair(b0.y, b1.y);
    let (bmin_z, bmax_z) = sorted_pair(b0.z, b1.z);
    amin_x <= bmax_x + margin
        && amax_x + margin >= bmin_x
        && amin_y <= bmax_y + margin
        && amax_y + margin >= bmin_y
        && amin_z <= bmax_z + margin
        && amax_z + margin >= bmin_z
}

fn sorted_pair(a: f64, b: f64) -> (f64, f64) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

fn point_segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = sub(b, a);
    let ap = sub(p, a);
    let denom = dot(ab, ab);
    if denom <= GEOMETRY_EPSILON_MM {
        return distance(p, a);
    }
    let t = (dot(ap, ab) / denom).clamp(0.0, 1.0);
    distance(p, add(a, scale(ab, t)))
}

fn segment_segment_distance(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> f64 {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);

    if a <= GEOMETRY_EPSILON_MM && e <= GEOMETRY_EPSILON_MM {
        return distance(p1, p2);
    }
    if a <= GEOMETRY_EPSILON_MM {
        let t = (f / e).clamp(0.0, 1.0);
        return distance(p1, add(p2, scale(d2, t)));
    }

    let c = dot(d1, r);
    if e <= GEOMETRY_EPSILON_MM {
        let s = (-c / a).clamp(0.0, 1.0);
        return distance(add(p1, scale(d1, s)), p2);
    }

    let b = dot(d1, d2);
    let denom = a * e - b * b;
    let mut s = if denom.abs() > GEOMETRY_EPSILON_MM {
        ((b * f - c * e) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = (b * s + f) / e;

    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }

    distance(add(p1, scale(d1, s)), add(p2, scale(d2, t)))
}

fn distance(a: Vec3, b: Vec3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn scale(a: Vec3, factor: f64) -> Vec3 {
    Vec3 {
        x: a.x * factor,
        y: a.y * factor,
        z: a.z * factor,
    }
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
