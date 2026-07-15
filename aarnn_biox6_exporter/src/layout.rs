use crate::calibration::{CalibrationSet, InhibitoryEncoding};
use crate::config::MachineProfile;
use crate::model::*;
use anyhow::{anyhow, Result};
use std::collections::HashMap;

const C_M_PER_S: f64 = 299_792_458.0;
const LAYOUT_SAFETY_MM: f64 = 0.02;
const DEFAULT_ROUTE_CANDIDATES_PER_EDGE: usize = 512;
const DENSE_ROUTE_MODE_CONNECTIONS: usize = 2_048;
const DENSE_PORT_ALLOCATION_THRESHOLD: usize = 256;
const MAX_PER_EDGE_BEST_EFFORT_WARNINGS: usize = 64;

pub fn map_snapshot(
    snapshot: &AarnnSnapshot,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> Result<PhysicalPlan> {
    let map = &calibration.mapping;
    let core = calibration
        .materials
        .get(&map.core_material_id)
        .ok_or_else(|| anyhow!("missing core material {}", map.core_material_id))?;
    let cladding = calibration
        .materials
        .get(&map.cladding_material_id)
        .ok_or_else(|| anyhow!("missing cladding material {}", map.cladding_material_id))?;
    let channel_diameter_mm = map
        .default_core_diameter_mm
        .max(cladding.minimum_feature_mm)
        .max(cladding.preferred_nozzle_mm);
    let edge_radius_mm = channel_diameter_mm / 2.0;
    let port_escape_mm = edge_radius_mm;

    let delay_strategy = parse_delay_strategy(&map.delay_strategy)?;
    let realisation = parse_node_realisation(&map.node_realisation)?;
    let active_connections = snapshot
        .connections
        .iter()
        .filter(|e| e.enabled)
        .collect::<Vec<_>>();
    let node_degrees = incident_connection_counts(snapshot, &active_connections);
    let node_radii = calculate_node_radii(
        snapshot,
        &node_degrees,
        map.default_node_radius_mm,
        channel_diameter_mm,
    );
    let positions = place_nodes(
        snapshot,
        machine,
        &node_radii,
        edge_radius_mm,
        port_escape_mm,
    )?;
    let routing_base_z = positions
        .iter()
        .filter_map(|(id, p)| {
            node_radii
                .get(id)
                .map(|r| p.z + r + port_escape_mm + edge_radius_mm + channel_diameter_mm)
        })
        .fold(edge_radius_mm, f64::max);
    let mut warnings = Vec::new();
    let (port_allocations, port_warnings) = assign_ports(
        &active_connections,
        &positions,
        &node_radii,
        machine,
        edge_radius_mm,
        port_escape_mm,
        routing_base_z,
    )?;
    warnings.extend(port_warnings);
    let lanes = routing_lanes(machine, routing_base_z, edge_radius_mm);
    let route_attempt_budget = route_attempt_budget(active_connections.len());
    let route_collision_reference_limit = route_collision_reference_limit(active_connections.len());
    let dense_routing_mode = active_connections.len() > DENSE_ROUTE_MODE_CONNECTIONS;
    if lanes.len() < active_connections.len() {
        warnings.push(PlanWarning {
            code: "ROUTING_LANES_REUSED_BEST_EFFORT".into(),
            entity_id: None,
            message: format!(
                "BIO X6 router found {} available Y/Z lanes for {} active connections; lanes will be reused where necessary and unresolved conflicts will be recorded.",
                lanes.len(),
                active_connections.len()
            ),
        });
    }
    let x_lanes = routing_x_lanes(machine, edge_radius_mm);
    if x_lanes.is_empty() || lanes.is_empty() {
        warnings.push(PlanWarning {
            code: "ROUTING_GRID_DEGRADED_BEST_EFFORT".into(),
            entity_id: None,
            message: format!(
                "BIO X6 routing grid is degraded ({} Y/Z lanes, {} X lanes); direct fallback routes will be used when grid candidates cannot be generated.",
                lanes.len(),
                x_lanes.len()
            ),
        });
    }
    if route_attempt_budget < DEFAULT_ROUTE_CANDIDATES_PER_EDGE {
        warnings.push(PlanWarning {
            code: "ROUTING_SEARCH_BUDGET_ADAPTIVE".into(),
            entity_id: None,
            message: format!(
                "Dense BIO X6 export detected ({} active connections); route search is capped at {} candidates per edge and {} previous routes per collision scan so preview/export remains interactive.",
                active_connections.len(),
                route_attempt_budget,
                route_collision_reference_limit
            ),
        });
    }
    if dense_routing_mode {
        warnings.push(PlanWarning {
            code: "DENSE_ROUTING_DETERMINISTIC_BEST_EFFORT".into(),
            entity_id: None,
            message: format!(
                "Dense BIO X6 export has {} active connections; routes are assigned deterministically to layered lanes without exhaustive per-edge collision search. Review physical validation warnings and preview artefacts before any print planning.",
                active_connections.len()
            ),
        });
    }

    let mut nodes = Vec::with_capacity(snapshot.nodes.len());

    for node in &snapshot.nodes {
        let position = *positions
            .get(&node.id)
            .ok_or_else(|| anyhow!("missing position for node {}", node.id))?;
        let radius_mm = *node_radii
            .get(&node.id)
            .ok_or_else(|| anyhow!("missing radius for node {}", node.id))?;
        let threshold_mw = threshold_to_power(node.threshold, map.nominal_input_power_mw);
        nodes.push(PhysicalNode {
            id: node.id.clone(),
            logical_kind: node.kind.clone(),
            position_mm: position,
            radius_mm,
            realisation: realisation.clone(),
            logical_threshold: node.threshold,
            realised_threshold_mw: threshold_mw,
            logical_refractory_us: node.refractory_us,
            realised_refractory_us: node.refractory_us
                * map.physical_seconds_per_aarnn_us
                * 1_000_000.0,
            optical_threshold_mw: threshold_mw,
            refractory_physical_us: node.refractory_us
                * map.physical_seconds_per_aarnn_us
                * 1_000_000.0,
            material_id: map.fluorescent_node_material_id.clone(),
            source_component_ids: node.source_component_ids.clone(),
        });
    }

    let mut connections = Vec::new();
    let mut routed_paths: Vec<RoutedPath> = Vec::new();
    let mut clean_route_count = 0usize;
    let mut compromised_route_count = 0usize;

    for (edge_index, edge) in active_connections.iter().enumerate() {
        let start_port = *port_allocations
            .get(&edge.source_node)
            .and_then(|ports| ports.get(&edge.id))
            .ok_or_else(|| anyhow!("missing source port for edge {}", edge.id))?;
        let end_port = *port_allocations
            .get(&edge.target_node)
            .and_then(|ports| ports.get(&edge.id))
            .ok_or_else(|| anyhow!("missing target port for edge {}", edge.id))?;

        let routed = if dense_routing_mode {
            route_edge_dense_best_effort(
                edge_index,
                start_port,
                end_port,
                &lanes,
                &x_lanes,
                machine,
                edge_radius_mm,
            )
        } else {
            route_edge_with_drc(
                edge,
                edge_index,
                start_port,
                end_port,
                &lanes,
                &x_lanes,
                &positions,
                &node_radii,
                machine,
                edge_radius_mm,
                &routed_paths,
                route_attempt_budget,
                route_collision_reference_limit,
            )?
        };
        let path = routed.path;
        if routed.clean {
            clean_route_count += 1;
        } else {
            compromised_route_count += 1;
            if compromised_route_count <= MAX_PER_EDGE_BEST_EFFORT_WARNINGS {
                warnings.push(PlanWarning {
                    code: "ROUTE_BEST_EFFORT_OVERLAP".into(),
                    entity_id: Some(edge.id.clone()),
                    message: format!(
                        "No fully non-overlapping route was found after {} attempts; using lowest-conflict candidate. Worst estimated overlap: {:.6} mm. Last rejection: {}",
                        routed.attempts,
                        routed.worst_overlap_mm,
                        routed.diagnostic
                    ),
                });
            } else if compromised_route_count == MAX_PER_EDGE_BEST_EFFORT_WARNINGS + 1 {
                warnings.push(PlanWarning {
                    code: "ROUTE_BEST_EFFORT_WARNINGS_TRUNCATED".into(),
                    entity_id: None,
                    message: format!(
                        "More than {} routes required best-effort fallback; per-edge route warnings are truncated and the routing summary gives the final count.",
                        MAX_PER_EDGE_BEST_EFFORT_WARNINGS
                    ),
                });
            }
        }
        let length_mm = polyline_length(&path);
        let geometric_delay_us = propagation_delay_us(length_mm, core.refractive_index);

        let target_delay_us = edge.delay_us * map.physical_seconds_per_aarnn_us * 1_000_000.0;
        let residual_delay_us = target_delay_us - geometric_delay_us;

        let estimated_transmission =
            transmission_from_loss(core.propagation_loss_db_per_cm, length_mm / 10.0);

        let polarity = if edge.weight >= 0.0 {
            Polarity::Excitatory
        } else {
            Polarity::Inhibitory
        };

        let target_transmission = weight_to_transmission(edge.weight.abs());

        let implementation = choose_edge_implementation(
            &polarity,
            residual_delay_us,
            target_transmission,
            estimated_transmission,
            &map.inhibitory_encoding,
            &delay_strategy,
        );

        if residual_delay_us > 0.001 {
            warnings.push(PlanWarning {
                code: "DELAY_REQUIRES_NON_GEOMETRIC_ELEMENT".into(),
                entity_id: Some(edge.id.clone()),
                message: format!(
                    "Residual delay {:.6} us cannot be supplied by this centimetre-scale path; use the selected material/electronic delay strategy.",
                    residual_delay_us
                ),
            });
        }
        if estimated_transmission < map.minimum_target_transmission {
            warnings.push(PlanWarning {
                code: "POWER_BUDGET_LOW".into(),
                entity_id: Some(edge.id.clone()),
                message: format!(
                    "Estimated transmission {:.4} is below policy minimum {:.4}.",
                    estimated_transmission, map.minimum_target_transmission
                ),
            });
        }

        connections.push(PhysicalConnection {
            id: edge.id.clone(),
            source_node: edge.source_node.clone(),
            target_node: edge.target_node.clone(),
            polarity,
            path_mm: path.clone(),
            path_length_mm: length_mm,
            core_material_id: map.core_material_id.clone(),
            cladding_material_id: map.cladding_material_id.clone(),
            core_diameter_mm: map.default_core_diameter_mm,
            target_delay_us,
            geometric_delay_us,
            residual_delay_us,
            target_transmission,
            estimated_transmission,
            implementation,
            source_component_ids: edge.source_component_ids.clone(),
        });
        routed_paths.push(RoutedPath {
            id: edge.id.clone(),
            path,
        });
    }

    warnings.push(PlanWarning {
        code: if compromised_route_count == 0 {
            "ROUTING_COMPLETE_CLEAN".into()
        } else {
            "ROUTING_COMPLETE_BEST_EFFORT".into()
        },
        entity_id: None,
        message: format!(
            "BIO X6 routing completed with {} clean routes and {} best-effort routes.",
            clean_route_count, compromised_route_count
        ),
    });

    let metrics = calculate_metrics(&connections, map.default_core_diameter_mm);

    Ok(PhysicalPlan {
        schema_version: "0.1".into(),
        source_network_id: snapshot.network_id.clone(),
        source_tick: snapshot.captured_at_tick,
        wavelength_nm: calibration.measured_at_wavelength_nm,
        time_scale: TimeScale {
            physical_seconds_per_aarnn_us: map.physical_seconds_per_aarnn_us,
            strategy: delay_strategy,
        },
        nodes,
        connections,
        warnings,
        metrics,
    })
}

fn place_nodes(
    snapshot: &AarnnSnapshot,
    machine: &MachineProfile,
    node_radii: &HashMap<String, f64>,
    edge_radius_mm: f64,
    port_escape_mm: f64,
) -> Result<HashMap<String, Vec3>> {
    let preferred = preferred_positions(snapshot, machine);
    if positions_fit(
        &preferred,
        node_radii,
        machine,
        edge_radius_mm,
        port_escape_mm,
    ) {
        return Ok(preferred);
    }
    packed_positions(
        snapshot,
        machine,
        node_radii,
        edge_radius_mm,
        port_escape_mm,
    )
}

fn preferred_positions(
    snapshot: &AarnnSnapshot,
    machine: &MachineProfile,
) -> HashMap<String, Vec3> {
    let mut positions = HashMap::new();
    let margin = 10.0;
    let available_x = (machine.build_volume_mm.x - margin * 2.0).max(1.0);
    let available_y = (machine.build_volume_mm.y - margin * 2.0).max(1.0);
    let columns = (snapshot.nodes.len() as f64).sqrt().ceil().max(1.0) as usize;

    for (i, node) in snapshot.nodes.iter().enumerate() {
        let p = node.preferred_position_mm.unwrap_or_else(|| {
            let col = i % columns;
            let row = i / columns;
            let dx = available_x / columns.max(1) as f64;
            let dy = available_y / columns.max(1) as f64;
            Vec3 {
                x: margin + (col as f64 + 0.5) * dx,
                y: margin + (row as f64 + 0.5) * dy,
                z: 2.0 + (i % 3) as f64 * 2.0,
            }
        });
        positions.insert(node.id.clone(), p);
    }
    positions
}

fn packed_positions(
    snapshot: &AarnnSnapshot,
    machine: &MachineProfile,
    node_radii: &HashMap<String, f64>,
    edge_radius_mm: f64,
    port_escape_mm: f64,
) -> Result<HashMap<String, Vec3>> {
    let mut positions = HashMap::new();
    let margin = (port_escape_mm + edge_radius_mm * 2.0).max(0.5);
    let clearance = edge_radius_mm * 2.0 + LAYOUT_SAFETY_MM;
    let mut ordered_nodes = snapshot.nodes.iter().collect::<Vec<_>>();
    ordered_nodes.sort_by(|a, b| {
        let ar = node_radii.get(&a.id).copied().unwrap_or(0.0);
        let br = node_radii.get(&b.id).copied().unwrap_or(0.0);
        br.partial_cmp(&ar)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut x = margin;
    let mut y = margin;
    let mut row_height: f64 = 0.0;
    for node in ordered_nodes {
        let radius = *node_radii
            .get(&node.id)
            .ok_or_else(|| anyhow!("missing radius for node {}", node.id))?;
        let diameter = radius * 2.0;
        if x + diameter + margin > machine.build_volume_mm.x && x > margin {
            x = margin;
            y += row_height + clearance;
            row_height = 0.0;
        }
        if y + diameter + margin > machine.build_volume_mm.y {
            return Err(anyhow!(
                "BIO X6 node layout exceeds build plate before routing; {} nodes with degree-aware port radii do not fit in {:.1} x {:.1} mm",
                snapshot.nodes.len(),
                machine.build_volume_mm.x,
                machine.build_volume_mm.y
            ));
        }
        let center = Vec3 {
            x: x + radius,
            y: y + radius,
            z: margin + radius,
        };
        positions.insert(node.id.clone(), center);
        x += diameter + clearance;
        row_height = row_height.max(diameter);
    }

    Ok(positions)
}

fn positions_fit(
    positions: &HashMap<String, Vec3>,
    node_radii: &HashMap<String, f64>,
    machine: &MachineProfile,
    edge_radius_mm: f64,
    port_escape_mm: f64,
) -> bool {
    let items = positions.iter().collect::<Vec<_>>();
    for (id, p) in &items {
        let Some(radius) = node_radii.get(*id) else {
            return false;
        };
        let xy_margin = radius + port_escape_mm + edge_radius_mm * 2.0;
        let z_top_margin = radius + port_escape_mm + edge_radius_mm * 2.0;
        if p.x < xy_margin
            || p.y < xy_margin
            || p.z < *radius
            || p.x > machine.build_volume_mm.x - xy_margin
            || p.y > machine.build_volume_mm.y - xy_margin
            || p.z > machine.build_volume_mm.z - z_top_margin
        {
            return false;
        }
    }

    for i in 0..items.len() {
        let (id_a, a) = items[i];
        let Some(radius_a) = node_radii.get(id_a) else {
            return false;
        };
        for (id_b, b) in items.iter().skip(i + 1) {
            let Some(radius_b) = node_radii.get(*id_b) else {
                return false;
            };
            let min_distance = radius_a + radius_b + edge_radius_mm * 2.0 + LAYOUT_SAFETY_MM;
            if distance(*a, **b) < min_distance - 1.0e-6 {
                return false;
            }
        }
    }

    true
}

#[derive(Debug, Clone)]
struct RoutedPath {
    id: String,
    path: Vec<Vec3>,
}

#[derive(Debug, Clone)]
struct RouteChoice {
    path: Vec<Vec3>,
    clean: bool,
    attempts: usize,
    worst_overlap_mm: f64,
    diagnostic: String,
}

#[derive(Debug, Clone)]
struct RouteAssessment {
    clean: bool,
    worst_overlap_mm: f64,
    diagnostic: String,
}

#[derive(Debug, Clone, Copy)]
enum RoutePattern {
    YThenX,
    XThenY,
    YDogleg,
    XDogleg,
}

fn route_edge_dense_best_effort(
    edge_index: usize,
    start: PortGeometry,
    end: PortGeometry,
    y_z_lanes: &[(f64, f64)],
    x_lanes: &[f64],
    machine: &MachineProfile,
    edge_radius_mm: f64,
) -> RouteChoice {
    let preferred_x = (start.escape.x + end.escape.x) / 2.0;
    let (lane_y, lane_z) = y_z_lanes
        .get(edge_index % y_z_lanes.len().max(1))
        .copied()
        .unwrap_or_else(|| {
            (
                (machine.build_volume_mm.y / 2.0)
                    .max(edge_radius_mm)
                    .min((machine.build_volume_mm.y - edge_radius_mm).max(edge_radius_mm)),
                (start.escape.z.max(end.escape.z) + edge_radius_mm * 4.0)
                    .max(edge_radius_mm)
                    .min((machine.build_volume_mm.z - edge_radius_mm).max(edge_radius_mm)),
            )
        });
    let lane_x = x_lanes
        .get(edge_index % x_lanes.len().max(1))
        .copied()
        .unwrap_or(preferred_x);
    let pattern = match edge_index % 4 {
        0 => RoutePattern::YThenX,
        1 => RoutePattern::XThenY,
        2 => RoutePattern::YDogleg,
        _ => RoutePattern::XDogleg,
    };
    RouteChoice {
        path: candidate_route(start, end, lane_y, lane_x, lane_z, pattern),
        clean: false,
        attempts: 1,
        worst_overlap_mm: 0.0,
        diagnostic:
            "dense routing mode assigned this lane without exhaustive per-edge collision search"
                .into(),
    }
}

fn route_edge_with_drc(
    edge: &LogicalConnection,
    edge_index: usize,
    start: PortGeometry,
    end: PortGeometry,
    y_z_lanes: &[(f64, f64)],
    x_lanes: &[f64],
    positions: &HashMap<String, Vec3>,
    node_radii: &HashMap<String, f64>,
    machine: &MachineProfile,
    edge_radius_mm: f64,
    routed_paths: &[RoutedPath],
    max_attempts: usize,
    collision_reference_limit: usize,
) -> Result<RouteChoice> {
    let lane_order = cyclic_candidate_indices(y_z_lanes.len(), edge_index);
    let preferred_x = (start.escape.x + end.escape.x) / 2.0;
    let x_order = coordinate_candidate_indices(x_lanes, preferred_x);
    let mut attempts = 0usize;
    let mut best_fallback: Option<RouteChoice> = None;

    let path = direct_fallback_route(start, end, machine, edge_radius_mm);
    attempts += 1;
    let assessment = assess_route_drc(
        edge,
        &path,
        positions,
        node_radii,
        machine,
        edge_radius_mm,
        routed_paths,
        collision_reference_limit,
    );
    if assessment.clean {
        return Ok(RouteChoice {
            path,
            clean: true,
            attempts,
            worst_overlap_mm: 0.0,
            diagnostic: "route is DRC clean".into(),
        });
    }
    update_best_route_fallback(&mut best_fallback, path, attempts, assessment);

    for lane_index in lane_order {
        let (lane_y, lane_z) = y_z_lanes[lane_index];
        for pattern in [RoutePattern::YThenX, RoutePattern::XThenY] {
            let lane_x = x_lanes
                .get(edge_index % x_lanes.len().max(1))
                .copied()
                .unwrap_or(preferred_x);
            let path = candidate_route(start, end, lane_y, lane_x, lane_z, pattern);
            attempts += 1;
            let assessment = assess_route_drc(
                edge,
                &path,
                positions,
                node_radii,
                machine,
                edge_radius_mm,
                routed_paths,
                collision_reference_limit,
            );
            if assessment.clean {
                return Ok(RouteChoice {
                    path,
                    clean: true,
                    attempts,
                    worst_overlap_mm: 0.0,
                    diagnostic: "route is DRC clean".into(),
                });
            }
            update_best_route_fallback(&mut best_fallback, path, attempts, assessment);
            if attempts >= max_attempts {
                let mut fallback = best_fallback.expect("direct fallback route is always recorded");
                fallback.attempts = attempts;
                return Ok(fallback);
            }
        }

        for &x_index in x_order.iter().take(16) {
            let lane_x = x_lanes[x_index];
            for pattern in [
                RoutePattern::XThenY,
                RoutePattern::YDogleg,
                RoutePattern::XDogleg,
            ] {
                let path = candidate_route(start, end, lane_y, lane_x, lane_z, pattern);
                attempts += 1;
                let assessment = assess_route_drc(
                    edge,
                    &path,
                    positions,
                    node_radii,
                    machine,
                    edge_radius_mm,
                    routed_paths,
                    collision_reference_limit,
                );
                if assessment.clean {
                    return Ok(RouteChoice {
                        path,
                        clean: true,
                        attempts,
                        worst_overlap_mm: 0.0,
                        diagnostic: "route is DRC clean".into(),
                    });
                }
                update_best_route_fallback(&mut best_fallback, path, attempts, assessment);
                if attempts >= max_attempts {
                    let mut fallback =
                        best_fallback.expect("direct fallback route is always recorded");
                    fallback.attempts = attempts;
                    return Ok(fallback);
                }
            }
        }
    }

    let mut fallback = best_fallback.expect("direct fallback route is always recorded");
    fallback.attempts = attempts;
    Ok(fallback)
}

fn update_best_route_fallback(
    best_fallback: &mut Option<RouteChoice>,
    path: Vec<Vec3>,
    attempts: usize,
    assessment: RouteAssessment,
) {
    let replace = best_fallback
        .as_ref()
        .map(|current| assessment.worst_overlap_mm < current.worst_overlap_mm)
        .unwrap_or(true);
    if replace {
        *best_fallback = Some(RouteChoice {
            path,
            clean: false,
            attempts,
            worst_overlap_mm: assessment.worst_overlap_mm,
            diagnostic: assessment.diagnostic,
        });
    }
}

fn candidate_route(
    start: PortGeometry,
    end: PortGeometry,
    lane_y: f64,
    lane_x: f64,
    lane_z: f64,
    pattern: RoutePattern,
) -> Vec<Vec3> {
    let start_rise = Vec3 {
        x: start.escape.x,
        y: start.escape.y,
        z: lane_z,
    };
    let end_rise = Vec3 {
        x: end.escape.x,
        y: end.escape.y,
        z: lane_z,
    };

    let mut points = vec![start.port, start.escape, start_rise];
    match pattern {
        RoutePattern::YThenX => {
            points.push(Vec3 {
                x: start.escape.x,
                y: lane_y,
                z: lane_z,
            });
            points.push(Vec3 {
                x: end.escape.x,
                y: lane_y,
                z: lane_z,
            });
        }
        RoutePattern::XThenY => {
            points.push(Vec3 {
                x: lane_x,
                y: start.escape.y,
                z: lane_z,
            });
            points.push(Vec3 {
                x: lane_x,
                y: end.escape.y,
                z: lane_z,
            });
        }
        RoutePattern::YDogleg => {
            points.push(Vec3 {
                x: start.escape.x,
                y: lane_y,
                z: lane_z,
            });
            points.push(Vec3 {
                x: lane_x,
                y: lane_y,
                z: lane_z,
            });
            points.push(Vec3 {
                x: lane_x,
                y: end.escape.y,
                z: lane_z,
            });
        }
        RoutePattern::XDogleg => {
            points.push(Vec3 {
                x: lane_x,
                y: start.escape.y,
                z: lane_z,
            });
            points.push(Vec3 {
                x: lane_x,
                y: lane_y,
                z: lane_z,
            });
            points.push(Vec3 {
                x: end.escape.x,
                y: lane_y,
                z: lane_z,
            });
        }
    }
    points.push(end_rise);
    points.push(end.escape);
    points.push(end.port);
    clean_route_points(points)
}

fn direct_fallback_route(
    start: PortGeometry,
    end: PortGeometry,
    machine: &MachineProfile,
    edge_radius_mm: f64,
) -> Vec<Vec3> {
    let z_max = (machine.build_volume_mm.z - edge_radius_mm).max(edge_radius_mm);
    let mid_z = (start.escape.z.max(end.escape.z) + edge_radius_mm * 4.0)
        .max(edge_radius_mm)
        .min(z_max);
    clean_route_points(vec![
        start.port,
        start.escape,
        Vec3 {
            x: start.escape.x,
            y: start.escape.y,
            z: mid_z,
        },
        Vec3 {
            x: end.escape.x,
            y: end.escape.y,
            z: mid_z,
        },
        end.escape,
        end.port,
    ])
}

fn assess_route_drc(
    edge: &LogicalConnection,
    path: &[Vec3],
    positions: &HashMap<String, Vec3>,
    node_radii: &HashMap<String, f64>,
    machine: &MachineProfile,
    edge_radius_mm: f64,
    routed_paths: &[RoutedPath],
    collision_reference_limit: usize,
) -> RouteAssessment {
    if path.len() < 2 {
        return RouteAssessment {
            clean: false,
            worst_overlap_mm: f64::INFINITY,
            diagnostic: "candidate path has fewer than two points".into(),
        };
    }

    let mut assessment = RouteAssessment {
        clean: true,
        worst_overlap_mm: 0.0,
        diagnostic: "route is DRC clean".into(),
    };

    for point in path {
        let outside_by = (edge_radius_mm - point.x)
            .max(edge_radius_mm - point.y)
            .max(edge_radius_mm - point.z)
            .max(point.x + edge_radius_mm - machine.build_volume_mm.x)
            .max(point.y + edge_radius_mm - machine.build_volume_mm.y)
            .max(point.z + edge_radius_mm - machine.build_volume_mm.z);
        if outside_by > 1.0e-6 {
            record_route_violation(
                &mut assessment,
                outside_by,
                format!(
                    "candidate path exits build volume at {:.3},{:.3},{:.3}",
                    point.x, point.y, point.z
                ),
            );
        }
    }

    for (node_id, node_position) in positions {
        let Some(node_radius) = node_radii.get(node_id) else {
            continue;
        };
        for segment_index in 0..(path.len() - 1) {
            if route_node_port_touch_allowed(node_id, edge, segment_index, path.len()) {
                continue;
            }
            let actual = point_segment_distance(
                *node_position,
                path[segment_index],
                path[segment_index + 1],
            );
            let required = node_radius + edge_radius_mm;
            if actual < required - 1.0e-6 {
                let overlap = required - actual;
                record_route_violation(
                    &mut assessment,
                    overlap,
                    format!(
                        "node {} would overlap segment {} by {:.6} mm",
                        node_id, segment_index, overlap
                    ),
                );
            }
        }
    }

    for segment_a in 0..(path.len() - 1) {
        for segment_b in (segment_a + 1)..(path.len() - 1) {
            if segment_a.abs_diff(segment_b) <= 1 {
                continue;
            }
            let actual = segment_segment_distance(
                path[segment_a],
                path[segment_a + 1],
                path[segment_b],
                path[segment_b + 1],
            );
            let required = edge_radius_mm * 2.0;
            if actual < required - 1.0e-6 {
                let overlap = required - actual;
                record_route_violation(
                    &mut assessment,
                    overlap,
                    format!(
                        "candidate self-overlaps segments {} and {} by {:.6} mm",
                        segment_a, segment_b, overlap
                    ),
                );
            }
        }
    }

    for routed in routed_paths.iter().rev().take(collision_reference_limit) {
        for segment_a in 0..(path.len() - 1) {
            for segment_b in 0..(routed.path.len() - 1) {
                if !segment_aabb_may_overlap(
                    path[segment_a],
                    path[segment_a + 1],
                    routed.path[segment_b],
                    routed.path[segment_b + 1],
                    edge_radius_mm * 2.0,
                ) {
                    continue;
                }
                let actual = segment_segment_distance(
                    path[segment_a],
                    path[segment_a + 1],
                    routed.path[segment_b],
                    routed.path[segment_b + 1],
                );
                let required = edge_radius_mm * 2.0;
                if actual < required - 1.0e-6 {
                    let overlap = required - actual;
                    record_route_violation(
                        &mut assessment,
                        overlap,
                        format!(
                            "would overlap edge {} segment {} at candidate segment {} by {:.6} mm",
                            routed.id, segment_b, segment_a, overlap
                        ),
                    );
                }
            }
        }
    }

    assessment
}

fn record_route_violation(assessment: &mut RouteAssessment, overlap_mm: f64, diagnostic: String) {
    assessment.clean = false;
    if overlap_mm > assessment.worst_overlap_mm {
        assessment.worst_overlap_mm = overlap_mm;
        assessment.diagnostic = diagnostic;
    }
}

fn route_node_port_touch_allowed(
    node_id: &str,
    edge: &LogicalConnection,
    segment_index: usize,
    path_len: usize,
) -> bool {
    if node_id == edge.source_node && segment_index == 0 {
        return true;
    }
    node_id == edge.target_node && segment_index + 2 == path_len
}

fn cyclic_candidate_indices(count: usize, preferred: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let preferred = preferred % count;
    let mut indices = (0..count).collect::<Vec<_>>();
    indices.sort_by_key(|idx| idx.abs_diff(preferred));
    indices
}

fn coordinate_candidate_indices(values: &[f64], preferred: f64) -> Vec<usize> {
    let mut indices = (0..values.len()).collect::<Vec<_>>();
    indices.sort_by(|a, b| {
        (values[*a] - preferred)
            .abs()
            .partial_cmp(&(values[*b] - preferred).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    indices
}

fn distance(a: Vec3, b: Vec3) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
}

fn point_segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = Vec3 {
        x: b.x - a.x,
        y: b.y - a.y,
        z: b.z - a.z,
    };
    let ap = Vec3 {
        x: p.x - a.x,
        y: p.y - a.y,
        z: p.z - a.z,
    };
    let denom = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z;
    if denom <= 1.0e-12 {
        return distance(p, a);
    }
    let t = ((ap.x * ab.x + ap.y * ab.y + ap.z * ab.z) / denom).clamp(0.0, 1.0);
    distance(
        p,
        Vec3 {
            x: a.x + ab.x * t,
            y: a.y + ab.y * t,
            z: a.z + ab.z * t,
        },
    )
}

fn segment_segment_distance(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> f64 {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);

    if a <= 1.0e-12 && e <= 1.0e-12 {
        return distance(p1, p2);
    }
    if a <= 1.0e-12 {
        let t = (f / e).clamp(0.0, 1.0);
        return distance(p1, add(p2, scale(d2, t)));
    }

    let c = dot(d1, r);
    if e <= 1.0e-12 {
        let s = (-c / a).clamp(0.0, 1.0);
        return distance(add(p1, scale(d1, s)), p2);
    }

    let b = dot(d1, d2);
    let denom = a * e - b * b;
    let mut s = if denom.abs() > 1.0e-12 {
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

fn incident_connection_counts(
    snapshot: &AarnnSnapshot,
    active_connections: &[&LogicalConnection],
) -> HashMap<String, usize> {
    let mut counts = snapshot
        .nodes
        .iter()
        .map(|node| (node.id.clone(), 0usize))
        .collect::<HashMap<_, _>>();
    for edge in active_connections {
        if let Some(count) = counts.get_mut(&edge.source_node) {
            *count += 1;
        }
        if let Some(count) = counts.get_mut(&edge.target_node) {
            *count += 1;
        }
    }
    counts
}

fn calculate_node_radii(
    snapshot: &AarnnSnapshot,
    node_degrees: &HashMap<String, usize>,
    default_radius_mm: f64,
    channel_diameter_mm: f64,
) -> HashMap<String, f64> {
    snapshot
        .nodes
        .iter()
        .map(|node| {
            let degree = node_degrees.get(&node.id).copied().unwrap_or(0);
            let radius = if degree <= 1 {
                default_radius_mm.max(channel_diameter_mm)
            } else {
                default_radius_mm.max(hemisphere_radius_for_ports(degree, channel_diameter_mm))
            };
            (node.id.clone(), radius)
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct PortGeometry {
    port: Vec3,
    escape: Vec3,
}

#[derive(Debug, Clone, Copy)]
struct PortChoice {
    candidate_index: usize,
    geometry: PortGeometry,
    clearance_mm: f64,
    outside_by_mm: f64,
    clean: bool,
}

impl PortGeometry {
    fn rise_point(&self, routing_base_z: f64) -> Vec3 {
        Vec3 {
            x: self.escape.x,
            y: self.escape.y,
            z: routing_base_z,
        }
    }
}

fn assign_ports(
    active_connections: &[&LogicalConnection],
    positions: &HashMap<String, Vec3>,
    node_radii: &HashMap<String, f64>,
    machine: &MachineProfile,
    edge_radius_mm: f64,
    port_escape_mm: f64,
    routing_base_z: f64,
) -> Result<(
    HashMap<String, HashMap<String, PortGeometry>>,
    Vec<PlanWarning>,
)> {
    let mut incident_edges: HashMap<String, Vec<String>> = HashMap::new();
    for edge in active_connections {
        incident_edges
            .entry(edge.source_node.clone())
            .or_default()
            .push(edge.id.clone());
        incident_edges
            .entry(edge.target_node.clone())
            .or_default()
            .push(edge.id.clone());
    }

    let mut out = HashMap::new();
    let mut warnings = Vec::new();
    for (node_id, mut edges) in incident_edges {
        edges.sort();
        edges.dedup();
        let center = *positions
            .get(&node_id)
            .ok_or_else(|| anyhow!("missing position for node {}", node_id))?;
        let radius = *node_radii
            .get(&node_id)
            .ok_or_else(|| anyhow!("missing radius for node {}", node_id))?;
        let total = edges.len();
        let candidate_count = (total.saturating_mul(4)).max(total + 32).max(64);
        let mut used_candidates = vec![false; candidate_count];
        let mut allocated = HashMap::new();

        if total > DENSE_PORT_ALLOCATION_THRESHOLD {
            warnings.push(PlanWarning {
                code: "PORT_DENSE_ALLOCATION_BEST_EFFORT".into(),
                entity_id: Some(node_id.clone()),
                message: format!(
                    "Node {} has {} incident BIO X6 ports; using deterministic hemisphere packing without exhaustive neighbor-clearance search.",
                    node_id, total
                ),
            });
            for (edge_index, edge_id) in edges.into_iter().enumerate() {
                let candidate_index =
                    distributed_candidate_index(edge_index, total, candidate_count);
                let normal = hemisphere_candidate_normal(candidate_index, candidate_count);
                let geometry =
                    port_geometry(center, radius, edge_radius_mm, port_escape_mm, normal);
                allocated.insert(edge_id, geometry);
            }
            out.insert(node_id, allocated);
            continue;
        }

        for (edge_index, edge_id) in edges.into_iter().enumerate() {
            let preferred = if total <= 1 {
                0
            } else {
                distributed_candidate_index(edge_index, total, candidate_count)
            };
            let mut order = (0..candidate_count).collect::<Vec<_>>();
            order.sort_by_key(|idx| idx.abs_diff(preferred));

            let mut chosen = None;
            let mut best_candidate = None;
            for candidate_index in order {
                if used_candidates[candidate_index] {
                    continue;
                }
                let normal = hemisphere_candidate_normal(candidate_index, candidate_count);
                let geometry =
                    port_geometry(center, radius, edge_radius_mm, port_escape_mm, normal);
                let outside_by_mm =
                    port_geometry_outside_by_mm(&geometry, machine, edge_radius_mm, routing_base_z);
                let clearance = port_geometry_clearance_mm(
                    &node_id,
                    &geometry,
                    positions,
                    node_radii,
                    edge_radius_mm,
                    routing_base_z,
                );
                let choice = PortChoice {
                    candidate_index,
                    geometry,
                    clearance_mm: clearance,
                    outside_by_mm,
                    clean: outside_by_mm <= 1.0e-6 && clearance >= LAYOUT_SAFETY_MM,
                };
                if choice.clean {
                    chosen = Some(choice);
                    break;
                }
                if better_port_choice(choice, best_candidate) {
                    best_candidate = Some(choice);
                }
            }

            let choice = chosen.or(best_candidate).unwrap_or_else(|| {
                let geometry = port_geometry(
                    center,
                    radius,
                    edge_radius_mm,
                    port_escape_mm,
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                );
                PortChoice {
                    candidate_index: 0,
                    geometry,
                    clearance_mm: f64::NEG_INFINITY,
                    outside_by_mm: port_geometry_outside_by_mm(
                        &geometry,
                        machine,
                        edge_radius_mm,
                        routing_base_z,
                    ),
                    clean: false,
                }
            });
            if !choice.clean {
                warnings.push(PlanWarning {
                    code: "PORT_BEST_EFFORT_OVERLAP".into(),
                    entity_id: Some(edge_id.clone()),
                    message: format!(
                        "No fully clear BIO X6 port candidate was found for node {}; using best local candidate. Clearance: {:.6} mm, build-volume overrun: {:.6} mm.",
                        node_id,
                        choice.clearance_mm,
                        choice.outside_by_mm.max(0.0)
                    ),
                });
            }
            if let Some(used) = used_candidates.get_mut(choice.candidate_index) {
                *used = true;
            }
            allocated.insert(edge_id, choice.geometry);
        }

        out.insert(node_id, allocated);
    }

    Ok((out, warnings))
}

fn port_geometry(
    center: Vec3,
    node_radius_mm: f64,
    edge_radius_mm: f64,
    port_escape_mm: f64,
    normal: Vec3,
) -> PortGeometry {
    let port_offset = node_radius_mm + edge_radius_mm;
    let escape_offset = port_offset + port_escape_mm;
    PortGeometry {
        port: Vec3 {
            x: center.x + normal.x * port_offset,
            y: center.y + normal.y * port_offset,
            z: center.z + normal.z * port_offset,
        },
        escape: Vec3 {
            x: center.x + normal.x * escape_offset,
            y: center.y + normal.y * escape_offset,
            z: center.z + normal.z * escape_offset,
        },
    }
}

fn distributed_candidate_index(
    edge_index: usize,
    total_edges: usize,
    candidate_count: usize,
) -> usize {
    if total_edges <= 1 || candidate_count <= 1 {
        return 0;
    }
    ((edge_index as f64) * ((candidate_count - 1) as f64) / ((total_edges - 1) as f64)).round()
        as usize
}

fn hemisphere_radius_for_ports(port_count: usize, channel_diameter_mm: f64) -> f64 {
    let packing_factor = 4.5;
    channel_diameter_mm * (port_count as f64 / packing_factor).sqrt() + channel_diameter_mm / 2.0
}

fn hemisphere_candidate_normal(index: usize, total: usize) -> Vec3 {
    if total <= 1 {
        return Vec3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
    }

    let golden_angle = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    let i = index as f64 + 0.5;
    let total = total as f64;
    let min_z = 0.18;
    let z = 1.0 - (1.0 - min_z) * (i / total);
    let radial = (1.0 - z * z).max(0.0).sqrt();
    let theta = i * golden_angle;
    Vec3 {
        x: radial * theta.cos(),
        y: radial * theta.sin(),
        z,
    }
}

fn better_port_choice(candidate: PortChoice, current: Option<PortChoice>) -> bool {
    current
        .map(|choice| port_choice_score(candidate) > port_choice_score(choice))
        .unwrap_or(true)
}

fn port_choice_score(choice: PortChoice) -> f64 {
    if choice.outside_by_mm > 1.0e-6 {
        return -1_000_000.0 - choice.outside_by_mm;
    }
    choice.clearance_mm
}

fn port_geometry_outside_by_mm(
    geometry: &PortGeometry,
    machine: &MachineProfile,
    edge_radius_mm: f64,
    routing_base_z: f64,
) -> f64 {
    [
        geometry.port,
        geometry.escape,
        geometry.rise_point(routing_base_z),
    ]
    .into_iter()
    .map(|p| {
        (edge_radius_mm - p.x)
            .max(edge_radius_mm - p.y)
            .max(edge_radius_mm - p.z)
            .max(p.x + edge_radius_mm - machine.build_volume_mm.x)
            .max(p.y + edge_radius_mm - machine.build_volume_mm.y)
            .max(p.z + edge_radius_mm - machine.build_volume_mm.z)
    })
    .fold(f64::NEG_INFINITY, f64::max)
}

fn port_geometry_clearance_mm(
    node_id: &str,
    geometry: &PortGeometry,
    positions: &HashMap<String, Vec3>,
    node_radii: &HashMap<String, f64>,
    edge_radius_mm: f64,
    routing_base_z: f64,
) -> f64 {
    let rise = geometry.rise_point(routing_base_z);
    positions
        .iter()
        .filter_map(|(other_id, other_position)| {
            if other_id == node_id {
                return None;
            }
            let radius = node_radii.get(other_id)?;
            let required = radius + edge_radius_mm;
            let escape_clearance =
                point_segment_distance(*other_position, geometry.port, geometry.escape) - required;
            let rise_clearance =
                point_segment_distance(*other_position, geometry.escape, rise) - required;
            Some(escape_clearance.min(rise_clearance))
        })
        .fold(f64::INFINITY, f64::min)
}

fn route_attempt_budget(active_connection_count: usize) -> usize {
    match active_connection_count {
        0..=128 => DEFAULT_ROUTE_CANDIDATES_PER_EDGE,
        129..=512 => 128,
        513..=2_048 => 48,
        _ => 16,
    }
}

fn route_collision_reference_limit(active_connection_count: usize) -> usize {
    match active_connection_count {
        0..=128 => usize::MAX,
        129..=512 => 256,
        513..=2_048 => 96,
        _ => 32,
    }
}

fn routing_lanes(
    machine: &MachineProfile,
    routing_base_z: f64,
    edge_radius_mm: f64,
) -> Vec<(f64, f64)> {
    let spacing = (edge_radius_mm * 2.0).max(1.0e-6);
    let mut lanes = Vec::new();
    let y_min = edge_radius_mm;
    let y_max = machine.build_volume_mm.y - edge_radius_mm;
    let z_min = routing_base_z.max(edge_radius_mm);
    let z_max = machine.build_volume_mm.z - edge_radius_mm;
    if y_min > y_max || z_min > z_max {
        return lanes;
    }

    let mut y = y_min;
    while y <= y_max + 1.0e-9 {
        let mut z = z_min;
        while z <= z_max + 1.0e-9 {
            lanes.push((y, z));
            z += spacing;
        }
        y += spacing;
    }
    lanes
}

fn routing_x_lanes(machine: &MachineProfile, edge_radius_mm: f64) -> Vec<f64> {
    let spacing = (edge_radius_mm * 2.0).max(1.0e-6);
    let mut lanes = Vec::new();
    let x_min = edge_radius_mm;
    let x_max = machine.build_volume_mm.x - edge_radius_mm;
    if x_min > x_max {
        return lanes;
    }

    let mut x = x_min;
    while x <= x_max + 1.0e-9 {
        lanes.push(x);
        x += spacing;
    }
    lanes
}

fn clean_route_points(points: Vec<Vec3>) -> Vec<Vec3> {
    let mut cleaned = Vec::with_capacity(points.len());
    for point in points {
        if cleaned
            .last()
            .is_some_and(|previous| distance(*previous, point) <= 1.0e-9)
        {
            continue;
        }
        cleaned.push(point);
    }

    let mut simplified: Vec<Vec3> = Vec::with_capacity(cleaned.len());
    for point in cleaned {
        simplified.push(point);
        while simplified.len() >= 3 {
            let len = simplified.len();
            let a = simplified[len - 3];
            let b = simplified[len - 2];
            let c = simplified[len - 1];
            if are_collinear(a, b, c) {
                simplified.remove(len - 2);
            } else {
                break;
            }
        }
    }

    simplified
}

fn are_collinear(a: Vec3, b: Vec3, c: Vec3) -> bool {
    let ab = Vec3 {
        x: b.x - a.x,
        y: b.y - a.y,
        z: b.z - a.z,
    };
    let bc = Vec3 {
        x: c.x - b.x,
        y: c.y - b.y,
        z: c.z - b.z,
    };
    let cross_x = ab.y * bc.z - ab.z * bc.y;
    let cross_y = ab.z * bc.x - ab.x * bc.z;
    let cross_z = ab.x * bc.y - ab.y * bc.x;
    let cross_len = (cross_x * cross_x + cross_y * cross_y + cross_z * cross_z).sqrt();
    cross_len <= 1.0e-9
}

fn polyline_length(path: &[Vec3]) -> f64 {
    path.windows(2)
        .map(|pair| {
            let a = pair[0];
            let b = pair[1];
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
        })
        .sum()
}

fn propagation_delay_us(length_mm: f64, refractive_index: f64) -> f64 {
    let length_m = length_mm / 1000.0;
    (length_m * refractive_index / C_M_PER_S) * 1_000_000.0
}

fn transmission_from_loss(loss_db_per_cm: f64, length_cm: f64) -> f64 {
    10f64.powf(-(loss_db_per_cm * length_cm) / 10.0)
}

fn threshold_to_power(threshold: f64, nominal_input_power_mw: f64) -> f64 {
    threshold.clamp(0.0, 1.0) * nominal_input_power_mw
}

fn weight_to_transmission(abs_weight: f64) -> f64 {
    abs_weight.clamp(0.0, 1.0)
}

fn choose_edge_implementation(
    polarity: &Polarity,
    residual_delay_us: f64,
    target_transmission: f64,
    estimated_transmission: f64,
    inhibitory_encoding: &InhibitoryEncoding,
    delay_strategy: &DelayStrategy,
) -> EdgeImplementation {
    if matches!(polarity, Polarity::Inhibitory) {
        return match inhibitory_encoding {
            InhibitoryEncoding::DualRail
            | InhibitoryEncoding::SeparateWavelength
            | InhibitoryEncoding::ElectronicSubtraction => EdgeImplementation::DualRailInhibitory,
        };
    }
    if residual_delay_us > 0.001 {
        return match delay_strategy {
            DelayStrategy::FluorescenceAndGeometry => EdgeImplementation::FluorescentDelayNode,
            DelayStrategy::HybridElectronic | DelayStrategy::TimeBinned => {
                EdgeImplementation::ExternalDelayChannel
            }
            DelayStrategy::GeometryOnly => EdgeImplementation::DirectWaveguide,
        };
    }
    if target_transmission + 0.02 < estimated_transmission {
        EdgeImplementation::AttenuatedWaveguide
    } else {
        EdgeImplementation::DirectWaveguide
    }
}

fn calculate_metrics(connections: &[PhysicalConnection], diameter_mm: f64) -> PlanMetrics {
    let total_length: f64 = connections.iter().map(|e| e.path_length_mm).sum();
    let area_mm2 = std::f64::consts::PI * (diameter_mm / 2.0).powi(2);
    let core_volume_mm3 = total_length * area_mm2;
    PlanMetrics {
        total_path_length_mm: total_length,
        estimated_core_volume_ml: core_volume_mm3 / 1000.0,
        estimated_cladding_volume_ml: core_volume_mm3 * 1.5 / 1000.0,
        maximum_delay_error_us: connections
            .iter()
            .map(|e| e.residual_delay_us.abs())
            .fold(0.0, f64::max),
        minimum_estimated_transmission: connections
            .iter()
            .map(|e| e.estimated_transmission)
            .fold(1.0, f64::min),
    }
}

fn parse_delay_strategy(value: &str) -> Result<DelayStrategy> {
    match value {
        "geometry_only" => Ok(DelayStrategy::GeometryOnly),
        "fluorescence_and_geometry" => Ok(DelayStrategy::FluorescenceAndGeometry),
        "hybrid_electronic" => Ok(DelayStrategy::HybridElectronic),
        "time_binned" => Ok(DelayStrategy::TimeBinned),
        other => Err(anyhow!("unknown delay strategy {other}")),
    }
}

fn parse_node_realisation(value: &str) -> Result<NodeRealisation> {
    match value {
        "passive_fluorescent" => Ok(NodeRealisation::PassiveFluorescent),
        "soft_threshold_fluorescent" => Ok(NodeRealisation::SoftThresholdFluorescent),
        "photochromic" => Ok(NodeRealisation::Photochromic),
        "thermoresponsive" => Ok(NodeRealisation::Thermoresponsive),
        "optoelectronic_regenerator" => Ok(NodeRealisation::OptoelectronicRegenerator),
        other => Err(anyhow!("unknown node realisation {other}")),
    }
}
