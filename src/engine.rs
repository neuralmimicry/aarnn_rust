use crate::config::{LIFParams, NetworkConfig, STDPParams};
use crate::runner::Runner;
use crate::sim::{Learning, NeuronModel};
use ndarray::Array1;
use serde::{Deserialize, Serialize};

fn default_model_name() -> String {
    "aarnn".to_string()
}

fn default_learning_name() -> String {
    "aarnn".to_string()
}

fn active_indices(spikes: &Array1<i8>) -> Vec<usize> {
    spikes
        .iter()
        .enumerate()
        .filter_map(|(idx, value)| (*value != 0).then_some(idx))
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineSpec {
    #[serde(default)]
    pub lif: LIFParams,
    #[serde(default)]
    pub stdp: STDPParams,
    #[serde(default)]
    pub net: NetworkConfig,
    #[serde(default = "default_model_name")]
    pub neuron_model: String,
    #[serde(default = "default_learning_name")]
    pub learning_rule: String,
}

impl Default for EngineSpec {
    fn default() -> Self {
        Self {
            lif: LIFParams::default(),
            stdp: STDPParams::default(),
            net: NetworkConfig::default(),
            neuron_model: default_model_name(),
            learning_rule: default_learning_name(),
        }
    }
}

impl EngineSpec {
    pub fn neuron_model(&self) -> anyhow::Result<NeuronModel> {
        NeuronModel::from_str(&self.neuron_model)
            .ok_or_else(|| anyhow::anyhow!("unsupported neuron model '{}'", self.neuron_model))
    }

    pub fn learning(&self) -> anyhow::Result<Learning> {
        Learning::from_str(&self.learning_rule)
            .ok_or_else(|| anyhow::anyhow!("unsupported learning rule '{}'", self.learning_rule))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineStatus {
    pub step: u64,
    pub sim_time_ms: f64,
    pub num_sensory_neurons: usize,
    pub num_hidden_layers: usize,
    pub num_output_neurons: usize,
    pub total_neurons: usize,
    pub desired_aarnn_depth: usize,
    pub neuron_model: String,
    pub learning_rule: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineActivity {
    pub step: u64,
    pub sim_time_ms: f64,
    pub sensory: Vec<usize>,
    pub hidden: Vec<Vec<usize>>,
    pub output: Vec<usize>,
}

/// Bounded, read-only topology projection for management and visualisation
/// clients. Node identifiers are valid only within `topology_generation`; they
/// are not biological ownership identifiers and must not be used to address a
/// mutable runner vector across generations.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineTopologySnapshot {
    pub schema_version: u32,
    pub topology_generation: String,
    pub step: u64,
    pub sim_time_ms: f64,
    pub layers: Vec<EngineTopologyLayer>,
    pub nodes: Vec<EngineTopologyNode>,
    pub edges: Vec<EngineTopologyEdge>,
    pub total_node_count: usize,
    pub total_edge_count: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineTopologyLayer {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub neuron_count: usize,
    pub visible_node_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineTopologyNode {
    pub id: String,
    pub layer_id: String,
    pub index: usize,
    pub active: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineTopologyEdge {
    pub source_id: String,
    pub target_id: String,
    pub kind: String,
    pub weight: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnginePayloadKind {
    Auto,
    Config,
    Snapshot,
}

pub struct RunnerEngine {
    spec: EngineSpec,
    runner: Runner,
    last_activity: EngineActivity,
    last_step_error: Option<String>,
    #[cfg(feature = "superdense_executor")]
    superdense: crate::superdense::SuperdenseController,
}

impl RunnerEngine {
    pub fn new(spec: EngineSpec) -> anyhow::Result<Self> {
        let runner = Runner::new(
            spec.lif.clone(),
            spec.stdp.clone(),
            spec.net.clone(),
            spec.neuron_model()?,
            spec.learning()?,
        );
        let status = Self::status_from_runner(&runner, &spec);
        Ok(Self {
            spec,
            runner,
            last_activity: EngineActivity {
                step: status.step,
                sim_time_ms: status.sim_time_ms,
                ..EngineActivity::default()
            },
            last_step_error: None,
            #[cfg(feature = "superdense_executor")]
            superdense: crate::superdense::SuperdenseController::new(),
        })
    }

    pub fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    pub fn status(&self) -> EngineStatus {
        Self::status_from_runner(&self.runner, &self.spec)
    }

    pub fn activity(&self) -> EngineActivity {
        self.last_activity.clone()
    }

    /// Return a deterministic, bounded topology projection from the current
    /// authoritative runner state. The endpoint consumer can render exact
    /// non-zero matrix edges for the included nodes without receiving mutable
    /// executor state or a write-capable handle.
    pub fn topology_snapshot(
        &self,
        requested_max_nodes: usize,
        requested_max_edges: usize,
    ) -> EngineTopologySnapshot {
        const SCHEMA_VERSION: u32 = 1;
        const DEFAULT_MAX_NODES: usize = 512;
        const DEFAULT_MAX_EDGES: usize = 4096;
        const HARD_MAX_NODES: usize = 4096;
        const HARD_MAX_EDGES: usize = 32_768;

        let max_nodes = if requested_max_nodes == 0 {
            DEFAULT_MAX_NODES
        } else {
            requested_max_nodes.clamp(1, HARD_MAX_NODES)
        };
        let max_edges = if requested_max_edges == 0 {
            DEFAULT_MAX_EDGES
        } else {
            requested_max_edges.clamp(1, HARD_MAX_EDGES)
        };
        let hidden_layers = self.runner.net.num_hidden_layers;
        let layer_count = hidden_layers + 2;
        // Distribute the budget deterministically from the first layer. Do not
        // force one node into every layer: a caller requesting fewer nodes than
        // layers must still receive a response within its declared bound.
        let node_budget = max_nodes / layer_count.max(1);
        let remainder = max_nodes % layer_count.max(1);

        let mut layer_counts = Vec::with_capacity(layer_count);
        layer_counts.push(self.runner.net.num_sensory_neurons);
        for layer in 0..hidden_layers {
            layer_counts.push(self.runner.layer_size(layer));
        }
        layer_counts.push(self.runner.net.num_output_neurons);

        let mut layer_ids = Vec::with_capacity(layer_count);
        layer_ids.push("sensory".to_string());
        for layer in 0..hidden_layers {
            layer_ids.push(format!("hidden-{layer}"));
        }
        layer_ids.push("output".to_string());

        let layer_names = std::iter::once("Sensory".to_string())
            .chain((0..hidden_layers).map(|layer| format!("Hidden {}", layer + 1)))
            .chain(std::iter::once("Output".to_string()))
            .collect::<Vec<_>>();
        let layer_kinds = std::iter::once("sensory".to_string())
            .chain((0..hidden_layers).map(|_| "hidden".to_string()))
            .chain(std::iter::once("output".to_string()))
            .collect::<Vec<_>>();

        let visible_counts = layer_counts
            .iter()
            .enumerate()
            .map(|(layer, count)| (*count).min(node_budget + usize::from(layer < remainder)))
            .collect::<Vec<_>>();
        let mut visible = visible_counts
            .iter()
            .map(|count| vec![false; *count])
            .collect::<Vec<_>>();
        let mut nodes = Vec::with_capacity(visible_counts.iter().sum());
        let mut layers = Vec::with_capacity(layer_count);
        for layer in 0..layer_count {
            let layer_id = layer_ids[layer].clone();
            let visible_count = visible_counts[layer];
            visible[layer].fill(true);
            for index in 0..visible_count {
                nodes.push(EngineTopologyNode {
                    id: topology_node_id(layer, index),
                    layer_id: layer_id.clone(),
                    index,
                    active: topology_node_active(&self.last_activity, hidden_layers, layer, index),
                });
            }
            layers.push(EngineTopologyLayer {
                id: layer_id,
                name: layer_names[layer].clone(),
                kind: layer_kinds[layer].clone(),
                neuron_count: layer_counts[layer],
                visible_node_count: visible_count,
            });
        }

        let mut edges = Vec::with_capacity(max_edges.min(DEFAULT_MAX_EDGES));
        let mut total_edge_count = 0usize;
        let mut generation_hash = 14695981039346656037u64;
        for (layer, count) in layer_counts.iter().enumerate() {
            hash_topology_u64(&mut generation_hash, layer as u64);
            hash_topology_u64(&mut generation_hash, *count as u64);
        }
        add_topology_matrix_edges(
            &self.runner.w_in,
            0,
            1,
            "input",
            &visible,
            max_edges,
            &mut edges,
            &mut total_edge_count,
            &mut generation_hash,
        );
        for (layer, matrix) in self.runner.w_hh_fwd.iter().enumerate() {
            add_topology_matrix_edges(
                matrix,
                layer + 1,
                layer + 2,
                "forward",
                &visible,
                max_edges,
                &mut edges,
                &mut total_edge_count,
                &mut generation_hash,
            );
        }
        for (layer, matrix) in self.runner.w_hh_bwd.iter().enumerate() {
            add_topology_matrix_edges(
                matrix,
                layer + 2,
                layer + 1,
                "backward",
                &visible,
                max_edges,
                &mut edges,
                &mut total_edge_count,
                &mut generation_hash,
            );
        }
        for (layer, matrix) in self.runner.w_hh_rec.iter().enumerate() {
            add_topology_matrix_edges(
                matrix,
                layer + 1,
                layer + 1,
                "recurrent",
                &visible,
                max_edges,
                &mut edges,
                &mut total_edge_count,
                &mut generation_hash,
            );
        }
        if hidden_layers > 0 {
            add_topology_matrix_edges(
                &self.runner.w_out,
                hidden_layers,
                hidden_layers + 1,
                "output",
                &visible,
                max_edges,
                &mut edges,
                &mut total_edge_count,
                &mut generation_hash,
            );
        }

        let total_node_count = layer_counts.iter().sum::<usize>();
        EngineTopologySnapshot {
            schema_version: SCHEMA_VERSION,
            topology_generation: format!("topology-v{SCHEMA_VERSION}-{generation_hash:016x}"),
            step: self.runner.t as u64,
            sim_time_ms: self.runner.t_ms,
            layers,
            nodes,
            edges,
            total_node_count,
            total_edge_count,
            truncated: total_node_count > visible_counts.iter().sum::<usize>()
                || total_edge_count > max_edges,
        }
    }

    pub fn last_step_error(&self) -> Option<&str> {
        self.last_step_error.as_deref()
    }

    pub fn export_snapshot_json(&self) -> anyhow::Result<String> {
        self.runner.export_network_json()
    }

    pub fn export_config_json(&self) -> anyhow::Result<String> {
        self.runner.export_config_json()
    }

    pub fn import_payload_json(
        &mut self,
        payload_json: &str,
        kind: EnginePayloadKind,
    ) -> anyhow::Result<EnginePayloadKind> {
        match kind {
            EnginePayloadKind::Auto => {
                if self.import_snapshot_json(payload_json).is_ok() {
                    Ok(EnginePayloadKind::Snapshot)
                } else {
                    self.import_config_json(payload_json)?;
                    Ok(EnginePayloadKind::Config)
                }
            }
            EnginePayloadKind::Config => {
                self.import_config_json(payload_json)?;
                Ok(EnginePayloadKind::Config)
            }
            EnginePayloadKind::Snapshot => {
                self.import_snapshot_json(payload_json)?;
                Ok(EnginePayloadKind::Snapshot)
            }
        }
    }

    pub fn import_config_json(&mut self, config_json: &str) -> anyhow::Result<()> {
        let cfg: crate::config::NetworkConfig = serde_json::from_str(config_json)?;
        // A config import may change the physical network shape. The old
        // path only replaced Runner::net, leaving hidden-layer matrices and
        // state vectors at their previous dimensions. That made a persisted
        // 1x1 workspace report the new config while still containing one
        // hidden neuron. Preserve the runner when compatible; otherwise
        // rebuild it so every matrix and state array matches the request.
        let shape_compatible = self.runner.net.num_sensory_neurons == cfg.num_sensory_neurons
            && self.runner.net.num_output_neurons == cfg.num_output_neurons
            && self.runner.net.num_hidden_layers == cfg.num_hidden_layers
            && (0..cfg.num_hidden_layers)
                .all(|layer| self.runner.layer_size(layer) == cfg.num_hidden_per_layer_initial);

        if shape_compatible {
            self.runner.import_config_json(config_json)?;
            self.spec.net = self.runner.net.clone();
        } else {
            let mut spec = self.spec.clone();
            spec.net = cfg;
            self.runner = Runner::new(
                spec.lif.clone(),
                spec.stdp.clone(),
                spec.net.clone(),
                spec.neuron_model()?,
                spec.learning()?,
            );
            self.spec = spec;
        }
        #[cfg(feature = "superdense_executor")]
        self.superdense.reset();
        self.last_step_error = None;
        self.clear_activity();
        Ok(())
    }

    pub fn import_snapshot_json(&mut self, snapshot_json: &str) -> anyhow::Result<()> {
        self.runner.import_network_json(snapshot_json)?;
        self.spec.net = self.runner.net.clone();
        #[cfg(feature = "superdense_executor")]
        self.superdense.reset();
        self.last_step_error = None;
        self.clear_activity();
        Ok(())
    }

    pub fn set_neuron_model_name(&mut self, model_name: &str) -> anyhow::Result<()> {
        let model = NeuronModel::from_str(model_name)
            .ok_or_else(|| anyhow::anyhow!("unsupported neuron model '{}'", model_name))?;
        self.runner.set_model(model);
        self.spec.neuron_model = model.to_str().to_string();
        self.clear_activity();
        Ok(())
    }

    pub fn set_learning_rule_name(&mut self, learning_rule: &str) -> anyhow::Result<()> {
        let learning = Learning::from_str(learning_rule)
            .ok_or_else(|| anyhow::anyhow!("unsupported learning rule '{}'", learning_rule))?;
        self.runner.set_learning(learning);
        self.spec.learning_rule = learning.to_str().to_string();
        self.clear_activity();
        Ok(())
    }

    pub fn reset_from_spec(&mut self) -> anyhow::Result<()> {
        let spec = self.spec.clone();
        let last_activity = self.last_activity.clone();
        *self = Self::new(spec)?;
        self.last_activity.sensory = last_activity.sensory;
        Ok(())
    }

    pub fn step(&mut self, sensory_spikes: Option<&[i8]>) -> EngineActivity {
        let sensory = sensory_spikes
            .map(|spikes| {
                spikes
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, value)| (*value != 0).then_some(idx))
                    .collect()
            })
            .unwrap_or_default();
        #[cfg(feature = "superdense_executor")]
        let out = match self.superdense.step(&mut self.runner, sensory_spikes) {
            Ok(out) => out,
            Err(error) => {
                self.last_step_error = Some(error.to_string());
                return self.last_activity.clone();
            }
        };
        #[cfg(not(feature = "superdense_executor"))]
        let out = self.runner.step(sensory_spikes);
        self.last_step_error = None;
        let hidden = out.spk_h.iter().map(active_indices).collect();
        let output = active_indices(&out.spk_o);
        self.last_activity = EngineActivity {
            step: out.t as u64,
            sim_time_ms: out.t_ms,
            sensory,
            hidden,
            output,
        };
        self.last_activity.clone()
    }

    pub const fn uses_superdense_executor() -> bool {
        cfg!(feature = "superdense_executor")
    }

    fn clear_activity(&mut self) {
        let status = self.status();
        self.last_activity = EngineActivity {
            step: status.step,
            sim_time_ms: status.sim_time_ms,
            ..EngineActivity::default()
        };
    }

    fn status_from_runner(runner: &Runner, spec: &EngineSpec) -> EngineStatus {
        let total_neurons = runner.net.num_sensory_neurons
            + runner.net.num_output_neurons
            + (0..runner.net.num_hidden_layers)
                .map(|layer| runner.layer_size(layer))
                .sum::<usize>();
        EngineStatus {
            step: runner.t as u64,
            sim_time_ms: runner.t_ms,
            num_sensory_neurons: runner.net.num_sensory_neurons,
            num_hidden_layers: runner.net.num_hidden_layers,
            num_output_neurons: runner.net.num_output_neurons,
            total_neurons,
            desired_aarnn_depth: runner.net.aarnn_layer_depth,
            neuron_model: spec.neuron_model.clone(),
            learning_rule: spec.learning_rule.clone(),
        }
    }
}

fn topology_node_id(layer: usize, index: usize) -> String {
    match layer {
        0 => format!("sensory:{index}"),
        _ => format!("layer:{layer}:{index}"),
    }
}

fn topology_node_active(
    activity: &EngineActivity,
    hidden_layers: usize,
    layer: usize,
    index: usize,
) -> bool {
    let active = match layer {
        0 => Some(&activity.sensory),
        layer if layer == hidden_layers + 1 => Some(&activity.output),
        layer => activity.hidden.get(layer.saturating_sub(1)),
    };
    active.is_some_and(|indices| indices.contains(&index))
}

fn hash_topology_u64(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(1099511628211);
    }
}

fn add_topology_matrix_edges(
    matrix: &ndarray::Array2<f64>,
    source_layer: usize,
    target_layer: usize,
    kind: &str,
    visible: &[Vec<bool>],
    max_edges: usize,
    edges: &mut Vec<EngineTopologyEdge>,
    total_edge_count: &mut usize,
    generation_hash: &mut u64,
) {
    for ((target, source), weight) in matrix.indexed_iter() {
        if !weight.is_finite() || *weight == 0.0 {
            continue;
        }
        hash_topology_u64(generation_hash, source_layer as u64);
        hash_topology_u64(generation_hash, source as u64);
        hash_topology_u64(generation_hash, target_layer as u64);
        hash_topology_u64(generation_hash, target as u64);
        *total_edge_count = total_edge_count.saturating_add(1);
        if edges.len() >= max_edges
            || !visible
                .get(source_layer)
                .and_then(|layer| layer.get(source))
                .copied()
                .unwrap_or(false)
            || !visible
                .get(target_layer)
                .and_then(|layer| layer.get(target))
                .copied()
                .unwrap_or(false)
        {
            continue;
        }
        edges.push(EngineTopologyEdge {
            source_id: topology_node_id(source_layer, source),
            target_id: topology_node_id(target_layer, target),
            kind: kind.to_string(),
            weight: *weight,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_import_rebuilds_changed_hidden_topology() {
        let mut engine = RunnerEngine::new(EngineSpec::default()).expect("engine");
        let mut cfg = engine.spec().net.clone();
        cfg.num_hidden_layers = 2;
        cfg.num_hidden_per_layer_initial = 3;

        engine
            .import_config_json(&serde_json::to_string(&cfg).expect("config json"))
            .expect("config import");

        let status = engine.status();
        assert_eq!(status.num_hidden_layers, 2);
        assert_eq!(
            status.total_neurons,
            cfg.num_sensory_neurons + 6 + cfg.num_output_neurons
        );
    }

    #[cfg(feature = "superdense_executor")]
    #[test]
    fn feature_gated_engine_admits_steps_through_local_executor() {
        let mut engine = RunnerEngine::new(EngineSpec::default()).expect("engine");
        assert!(RunnerEngine::uses_superdense_executor());
        let first = engine.step(None);
        let second = engine.step(None);
        assert_eq!(first.step + 1, second.step);
        assert_eq!(engine.last_step_error(), None);
    }

    #[test]
    fn default_path_selection_is_explicit() {
        assert_eq!(
            RunnerEngine::uses_superdense_executor(),
            cfg!(feature = "superdense_executor")
        );
    }

    #[test]
    fn topology_snapshot_contains_bounded_weighted_edges() {
        let mut spec = EngineSpec::default();
        spec.net.num_sensory_neurons = 3;
        spec.net.num_hidden_layers = 2;
        spec.net.num_hidden_per_layer_initial = 4;
        spec.net.num_output_neurons = 2;
        let engine = RunnerEngine::new(spec).expect("engine");

        let topology = engine.topology_snapshot(8, 5);

        assert_eq!(topology.schema_version, 1);
        assert_eq!(topology.layers.len(), 4);
        assert_eq!(
            topology.total_node_count,
            topology
                .layers
                .iter()
                .map(|layer| layer.neuron_count)
                .sum::<usize>()
        );
        assert!(topology.nodes.len() <= 8);
        assert!(topology.edges.len() <= 5);
        assert!(topology.total_edge_count >= topology.edges.len());
        assert!(
            topology
                .edges
                .iter()
                .all(|edge| edge.weight.is_finite() && !edge.source_id.is_empty())
        );
        assert!(topology.truncated);

        let tiny = engine.topology_snapshot(1, 1);
        assert!(tiny.nodes.len() <= 1);
    }
}
