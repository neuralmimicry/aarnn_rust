use crate::model::{AarnnSnapshot, LogicalConnection, LogicalNode, NodeKind, Vec3};
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};

const DEFAULT_TIME_UNIT_US: f64 = 1_000.0;
const WEIGHT_EPSILON: f64 = 1.0e-12;

#[derive(Debug, Clone)]
struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

impl Matrix {
    fn value(&self, row: usize, col: usize) -> f64 {
        self.data
            .get(row.saturating_mul(self.cols).saturating_add(col))
            .copied()
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone)]
struct NodeRecord {
    position_mm: Option<Vec3>,
}

pub fn snapshot_from_json_str(raw: &str, network_id_hint: Option<&str>) -> Result<AarnnSnapshot> {
    if let Ok(mut snapshot) = serde_json::from_str::<AarnnSnapshot>(raw) {
        if !snapshot.nodes.is_empty() {
            if snapshot.network_id.trim().is_empty() {
                if let Some(hint) = network_id_hint {
                    snapshot.network_id = hint.to_string();
                }
            }
            return Ok(snapshot);
        }
    }

    let value: Value = serde_json::from_str(raw).context("invalid JSON snapshot payload")?;
    snapshot_from_runner_value(&value, network_id_hint)
}

fn snapshot_from_runner_value(
    value: &Value,
    network_id_hint: Option<&str>,
) -> Result<AarnnSnapshot> {
    let net = value.get("net").and_then(Value::as_object).ok_or_else(|| {
        anyhow!("snapshot is neither BIO X6 logical schema nor AARNN runner schema")
    })?;

    let w_in = matrix(value.get("w_in"));
    let w_out = matrix(value.get("w_out"));
    let w_hh_fwd = matrix_vec(value.get("w_hh_fwd"));
    let w_hh_bwd = matrix_vec(value.get("w_hh_bwd"));
    let w_hh_rec = matrix_vec(value.get("w_hh_rec"));
    let p_in = matrix(value.get("p_in"));
    let p_out = matrix(value.get("p_out"));
    let p_fwd = matrix_vec(value.get("p_fwd"));
    let p_bwd = matrix_vec(value.get("p_bwd"));
    let p_rec = matrix_vec(value.get("p_rec"));

    let sensory_count = net_usize(net, "num_sensory_neurons")
        .or_else(|| w_in.as_ref().map(|m| m.cols))
        .unwrap_or(0);
    let output_count = net_usize(net, "num_output_neurons")
        .or_else(|| w_out.as_ref().map(|m| m.rows))
        .unwrap_or(0);
    let hidden_sizes = infer_hidden_sizes(net, w_in.as_ref(), &w_hh_fwd, &w_hh_rec, w_out.as_ref());
    if sensory_count == 0 && output_count == 0 && hidden_sizes.iter().sum::<usize>() == 0 {
        return Err(anyhow!("runner snapshot contains no exportable nodes"));
    }

    let mut node_records: HashMap<String, NodeRecord> = HashMap::new();
    let mut nodes = Vec::new();
    let default_refractory_us = get_path_f64(value, &["net", "aarnn_bio", "izh_refractory_ms"])
        .or_else(|| get_path_f64(value, &["net", "bouton_latency_ms"]))
        .unwrap_or(2.0)
        .max(0.0)
        * 1_000.0;

    for i in 0..sensory_count {
        let id = sensory_id(i);
        let position = topology_position(value, "sensory_nodes", 0, i)
            .or_else(|| layered_position(0, i, sensory_count, hidden_sizes.len() + 2));
        node_records.insert(
            id.clone(),
            NodeRecord {
                position_mm: position,
            },
        );
        nodes.push(LogicalNode {
            id: id.clone(),
            kind: NodeKind::Sensory,
            threshold: 0.20,
            bias: 0.0,
            refractory_us: default_refractory_us,
            preferred_position_mm: position,
            source_component_ids: vec![format!("soma:{id}"), format!("axon_hillock:{id}")],
        });
    }

    for (layer, count) in hidden_sizes.iter().copied().enumerate() {
        for index in 0..count {
            let id = hidden_id(layer, index);
            let position = topology_position(value, "layers", layer, index)
                .or_else(|| layered_position(layer + 1, index, count, hidden_sizes.len() + 2));
            node_records.insert(
                id.clone(),
                NodeRecord {
                    position_mm: position,
                },
            );
            nodes.push(LogicalNode {
                id: id.clone(),
                kind: NodeKind::Interneuron,
                threshold: 0.50,
                bias: 0.0,
                refractory_us: default_refractory_us,
                preferred_position_mm: position,
                source_component_ids: vec![format!("soma:{id}"), format!("axon_hillock:{id}")],
            });
        }
    }

    for i in 0..output_count {
        let id = output_id(i);
        let position = topology_position(value, "output_nodes", 0, i).or_else(|| {
            layered_position(
                hidden_sizes.len() + 1,
                i,
                output_count,
                hidden_sizes.len() + 2,
            )
        });
        node_records.insert(
            id.clone(),
            NodeRecord {
                position_mm: position,
            },
        );
        nodes.push(LogicalNode {
            id: id.clone(),
            kind: NodeKind::Readout,
            threshold: 0.60,
            bias: 0.0,
            refractory_us: default_refractory_us,
            preferred_position_mm: position,
            source_component_ids: vec![format!("soma:{id}"), format!("axon_hillock:{id}")],
        });
    }

    let mut connections = Vec::new();
    if let Some(morph_connections) =
        morph_connections(value, &hidden_sizes, &node_records, net).filter(|v| !v.is_empty())
    {
        connections.extend(morph_connections);
    } else {
        add_matrix_connections(
            &mut connections,
            "w_in",
            w_in.as_ref(),
            p_in.as_ref(),
            sensory_count,
            hidden_sizes.first().copied().unwrap_or(0),
            |col| sensory_id(col),
            |row| hidden_id(0, row),
            &node_records,
            net,
        );
        for (layer, mat) in w_hh_fwd.iter().enumerate() {
            add_matrix_connections(
                &mut connections,
                &format!("w_hh_fwd:{layer}"),
                Some(mat),
                p_fwd.get(layer),
                hidden_sizes.get(layer).copied().unwrap_or(mat.cols),
                hidden_sizes.get(layer + 1).copied().unwrap_or(mat.rows),
                |col| hidden_id(layer, col),
                |row| hidden_id(layer + 1, row),
                &node_records,
                net,
            );
        }
        for (layer, mat) in w_hh_bwd.iter().enumerate() {
            add_matrix_connections(
                &mut connections,
                &format!("w_hh_bwd:{layer}"),
                Some(mat),
                p_bwd.get(layer),
                hidden_sizes.get(layer + 1).copied().unwrap_or(mat.cols),
                hidden_sizes.get(layer).copied().unwrap_or(mat.rows),
                |col| hidden_id(layer + 1, col),
                |row| hidden_id(layer, row),
                &node_records,
                net,
            );
        }
        for (layer, mat) in w_hh_rec.iter().enumerate() {
            add_matrix_connections(
                &mut connections,
                &format!("w_hh_rec:{layer}"),
                Some(mat),
                p_rec.get(layer),
                hidden_sizes.get(layer).copied().unwrap_or(mat.cols),
                hidden_sizes.get(layer).copied().unwrap_or(mat.rows),
                |col| hidden_id(layer, col),
                |row| hidden_id(layer, row),
                &node_records,
                net,
            );
        }
        if let Some(last_layer) = hidden_sizes.len().checked_sub(1) {
            add_matrix_connections(
                &mut connections,
                "w_out",
                w_out.as_ref(),
                p_out.as_ref(),
                hidden_sizes.get(last_layer).copied().unwrap_or(0),
                output_count,
                |col| hidden_id(last_layer, col),
                |row| output_id(row),
                &node_records,
                net,
            );
        }
    }

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "source_schema".into(),
        Value::String("aarnn_runner_snapshot".into()),
    );
    metadata.insert(
        "adapter".into(),
        Value::String("aarnn_biox6_exporter".into()),
    );
    metadata.insert(
        "logical_connection_policy".into(),
        Value::String("morphology_synapses_when_available_else_active_weight_matrices".into()),
    );

    Ok(AarnnSnapshot {
        schema_version: "0.1".into(),
        network_id: network_id_hint
            .map(str::to_string)
            .or_else(|| {
                get_path_str(value, &["net", "deployment", "network_id"]).map(str::to_string)
            })
            .unwrap_or_else(|| "aarnn-network".into()),
        captured_at_tick: value.get("t").and_then(Value::as_u64).unwrap_or(0),
        time_unit_us: infer_time_unit_us(value),
        nodes,
        connections,
        metadata,
    })
}

fn infer_hidden_sizes(
    net: &Map<String, Value>,
    w_in: Option<&Matrix>,
    w_hh_fwd: &[Matrix],
    w_hh_rec: &[Matrix],
    w_out: Option<&Matrix>,
) -> Vec<usize> {
    let mut sizes = Vec::new();
    if let Some(m) = w_in {
        if m.rows > 0 {
            sizes.push(m.rows);
        }
    }
    for mat in w_hh_fwd {
        if mat.rows > 0 {
            sizes.push(mat.rows);
        }
    }
    if sizes.is_empty() {
        for mat in w_hh_rec {
            if mat.rows > 0 {
                sizes.push(mat.rows);
            }
        }
    }
    if sizes.is_empty() {
        if let Some(m) = w_out {
            if m.cols > 0 {
                sizes.push(m.cols);
            }
        }
    }
    if sizes.is_empty() {
        let layers = net_usize(net, "num_hidden_layers").unwrap_or(1).max(1);
        let per_layer = net_usize(net, "num_hidden_per_layer_initial").unwrap_or(1);
        sizes.resize(layers, per_layer);
    }
    sizes
}

fn add_matrix_connections(
    out: &mut Vec<LogicalConnection>,
    prefix: &str,
    weights: Option<&Matrix>,
    presence: Option<&Matrix>,
    source_count: usize,
    target_count: usize,
    source_id: impl Fn(usize) -> String,
    target_id: impl Fn(usize) -> String,
    nodes: &HashMap<String, NodeRecord>,
    net: &Map<String, Value>,
) {
    let Some(weights) = weights else {
        return;
    };
    let rows = target_count.min(weights.rows);
    let cols = source_count.min(weights.cols);
    for row in 0..rows {
        for col in 0..cols {
            let weight = weights.value(row, col);
            let present = presence
                .map(|p| row < p.rows && col < p.cols && p.value(row, col) > 0.0)
                .unwrap_or_else(|| weight.abs() > WEIGHT_EPSILON);
            if !present {
                continue;
            }
            let source_node = source_id(col);
            let target_node = target_id(row);
            if !nodes.contains_key(&source_node) || !nodes.contains_key(&target_node) {
                continue;
            }
            let delay_us = estimate_delay_us(&source_node, &target_node, nodes, net);
            out.push(LogicalConnection {
                id: format!("synapse:{prefix}:{source_node}:{target_node}"),
                source_node: source_node.clone(),
                target_node: target_node.clone(),
                weight,
                delay_us,
                enabled: true,
                source_component_ids: vec![
                    format!("axon:{source_node}"),
                    format!("axon_bouton:{source_node}->{target_node}"),
                    format!("synaptic_gap:{prefix}:{row}:{col}"),
                    format!("dendrite_bouton:{target_node}"),
                ],
            });
        }
    }
}

fn morph_connections(
    value: &Value,
    hidden_sizes: &[usize],
    nodes: &HashMap<String, NodeRecord>,
    net: &Map<String, Value>,
) -> Option<Vec<LogicalConnection>> {
    let synapses = value
        .get("runtime_state")
        .and_then(|v| v.get("morph"))
        .and_then(|v| v.get("synapses"))
        .and_then(Value::as_array)
        .or_else(|| {
            value
                .get("morph")
                .and_then(|v| v.get("synapses"))
                .and_then(Value::as_array)
        })?;

    let output_layer = hidden_sizes.len() as isize;
    let mut out = Vec::new();
    for (idx, syn) in synapses.iter().enumerate() {
        let pre_layer = syn.get("pre_layer").and_then(Value::as_i64).unwrap_or(0) as isize;
        let post_layer = syn.get("post_layer").and_then(Value::as_i64).unwrap_or(0) as isize;
        let pre_id = syn.get("pre_id").and_then(Value::as_u64).unwrap_or(0) as usize;
        let post_id = syn.get("post_id").and_then(Value::as_u64).unwrap_or(0) as usize;
        let source_node = layer_node_id(pre_layer, pre_id, output_layer)?;
        let target_node = layer_node_id(post_layer, post_id, output_layer)?;
        if !nodes.contains_key(&source_node) || !nodes.contains_key(&target_node) {
            continue;
        }
        let weight = syn.get("weight").and_then(Value::as_f64).unwrap_or(0.0);
        if weight.abs() <= WEIGHT_EPSILON {
            continue;
        }
        let delay_us = syn
            .get("delay_ms")
            .and_then(Value::as_f64)
            .map(|v| v.max(0.0) * 1_000.0)
            .unwrap_or_else(|| estimate_delay_us(&source_node, &target_node, nodes, net));
        out.push(LogicalConnection {
            id: format!("synapse:morph:{idx}:{source_node}:{target_node}"),
            source_node: source_node.clone(),
            target_node: target_node.clone(),
            weight,
            delay_us,
            enabled: true,
            source_component_ids: vec![
                format!("axon:{source_node}"),
                format!("axon_segment:{:?}", syn.get("axon_seg_idx")),
                format!("axon_bouton:{source_node}->{target_node}"),
                format!("synaptic_gap:morph:{idx}"),
                format!("dendrite_segment:{:?}", syn.get("dend_seg_idx")),
                format!("dendrite_bouton:{target_node}"),
            ],
        });
    }
    Some(out)
}

fn layer_node_id(layer: isize, index: usize, output_layer: isize) -> Option<String> {
    if layer < 0 {
        Some(sensory_id(index))
    } else if layer == output_layer {
        Some(output_id(index))
    } else if layer < output_layer {
        Some(hidden_id(layer as usize, index))
    } else {
        None
    }
}

fn estimate_delay_us(
    source: &str,
    target: &str,
    nodes: &HashMap<String, NodeRecord>,
    net: &Map<String, Value>,
) -> f64 {
    let bouton_latency_ms = net_f64(net, "bouton_latency_ms").unwrap_or(0.5).max(0.0);
    let use_delays = net
        .get("use_aarnn_delays")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !use_delays {
        return bouton_latency_ms * 1_000.0;
    }
    let velocity = net_f64(net, "aarnn_velocity").unwrap_or(10.0).max(1.0e-6);
    let distance_mm = match (
        nodes.get(source).and_then(|n| n.position_mm),
        nodes.get(target).and_then(|n| n.position_mm),
    ) {
        (Some(a), Some(b)) => {
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
        }
        _ => 50.0,
    };
    let normalized_distance = distance_mm / 50.0;
    (bouton_latency_ms + normalized_distance / velocity) * 1_000.0
}

fn infer_time_unit_us(value: &Value) -> f64 {
    let tick = value.get("t").and_then(Value::as_u64).unwrap_or(0);
    let time_ms = value.get("t_ms").and_then(Value::as_f64).unwrap_or(0.0);
    if tick > 0 && time_ms.is_finite() && time_ms > 0.0 {
        return (time_ms / tick as f64) * 1_000.0;
    }
    get_path_f64(value, &["net", "lif", "dt"])
        .or_else(|| get_path_f64(value, &["net", "dt_ms"]))
        .map(|ms| ms.max(0.0) * 1_000.0)
        .unwrap_or(DEFAULT_TIME_UNIT_US)
}

fn matrix(value: Option<&Value>) -> Option<Matrix> {
    let value = value?;
    let rows = value.get("rows").and_then(Value::as_u64)? as usize;
    let cols = value.get("cols").and_then(Value::as_u64)? as usize;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
        .unwrap_or_else(|| vec![0.0; rows.saturating_mul(cols)]);
    Some(Matrix { rows, cols, data })
}

fn matrix_vec(value: Option<&Value>) -> Vec<Matrix> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|v| matrix(Some(v))).collect())
        .unwrap_or_default()
}

fn topology_position(value: &Value, kind: &str, layer: usize, index: usize) -> Option<Vec3> {
    let topo = value.get("topo")?;
    let node = if kind == "layers" {
        topo.get("layers")?
            .as_array()?
            .get(layer)?
            .as_array()?
            .get(index)?
    } else {
        topo.get(kind)?.as_array()?.get(index)?
    };
    let x = node.get("x").and_then(Value::as_f64)?;
    let y = node.get("y").and_then(Value::as_f64)?;
    let z = node.get("z").and_then(Value::as_f64).unwrap_or(0.0);
    Some(normalized_to_mm(x, y, z))
}

fn normalized_to_mm(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: (65.0 + x.clamp(-1.0, 1.0) * 50.0).clamp(5.0, 125.0),
        y: (45.0 + y.clamp(-1.0, 1.0) * 32.0).clamp(5.0, 85.0),
        z: (8.0 + z.clamp(-1.0, 1.0) * 6.0).clamp(1.0, 20.0),
    }
}

fn layered_position(column: usize, row: usize, row_count: usize, columns: usize) -> Option<Vec3> {
    let columns = columns.max(2);
    let x = 12.0 + (column as f64) * (106.0 / (columns - 1) as f64);
    let y = if row_count <= 1 {
        45.0
    } else {
        12.0 + (row as f64) * (66.0 / (row_count - 1) as f64)
    };
    Some(Vec3 {
        x,
        y,
        z: 2.0 + (column % 4) as f64 * 2.0,
    })
}

fn sensory_id(index: usize) -> String {
    format!("sensory-{index}")
}

fn hidden_id(layer: usize, index: usize) -> String {
    format!("hidden-{layer}-{index}")
}

fn output_id(index: usize) -> String {
    format!("output-{index}")
}

fn net_usize(net: &Map<String, Value>, key: &str) -> Option<usize> {
    net.get(key).and_then(Value::as_u64).map(|v| v as usize)
}

fn net_f64(net: &Map<String, Value>, key: &str) -> Option<f64> {
    net.get(key).and_then(Value::as_f64)
}

fn get_path_f64<'a>(value: &'a Value, path: &[&str]) -> Option<f64> {
    let mut cur = value;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_f64()
}

fn get_path_str<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = value;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str()
}
