use crate::config::{LIFParams, NetworkConfig, STDPParams};
use crate::morphology_contract::{
    AnatomicalId, AnatomicalKind, AxisAlignedBox, DisplayEdge, DisplayMode, DisplayNode,
    DisplayProvenance, DisplayRole, DisplaySnapshot, Vec3, display_layer_samples,
};
#[cfg(all(feature = "morpho", feature = "growth3d"))]
use crate::morphology_contract::{DisplayMarker, DisplayPath};
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
    #[serde(default)]
    pub sensory_history: Vec<EngineSpikeHistoryFrame>,
    #[serde(default)]
    pub output_history: Vec<EngineSpikeHistoryFrame>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineSpikeHistoryFrame {
    pub step: u64,
    pub indices: Vec<usize>,
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
        let mut activity = self.last_activity.clone();
        let step = self.runner.t as u64;
        let frames = |history: &std::collections::VecDeque<ndarray::Array1<i8>>| {
            history
                .iter()
                .take(128)
                .enumerate()
                .filter_map(|(offset, frame)| {
                    step.checked_sub(offset as u64)
                        .map(|frame_step| EngineSpikeHistoryFrame {
                            step: frame_step,
                            indices: frame
                                .iter()
                                .enumerate()
                                .filter_map(|(index, &spike)| (spike != 0).then_some(index))
                                .collect(),
                        })
                })
                .collect()
        };
        activity.sensory_history = frames(&self.runner.spk_hist_s);
        activity.output_history = frames(&self.runner.spk_hist_o);
        activity
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

    /// Return the bounded, versioned display contract shared by native and web
    /// clients. Legacy topology points are explicitly marked procedural: they
    /// are useful for navigation, but they do not claim to be measured anatomy
    /// or physical route geometry.
    pub fn display_snapshot(
        &self,
        mode: DisplayMode,
        sequence: u64,
        requested_max_nodes: usize,
        requested_max_edges: usize,
    ) -> anyhow::Result<DisplaySnapshot> {
        self.display_snapshot_in_region(
            mode,
            sequence,
            requested_max_nodes,
            requested_max_edges,
            None,
        )
    }

    pub fn display_snapshot_in_region(
        &self,
        mode: DisplayMode,
        sequence: u64,
        requested_max_nodes: usize,
        requested_max_edges: usize,
        region: Option<AxisAlignedBox>,
    ) -> anyhow::Result<DisplaySnapshot> {
        Self::display_snapshot_for_runner_in_region(
            &self.runner,
            mode,
            sequence,
            requested_max_nodes,
            requested_max_edges,
            true,
            region,
        )
    }

    pub fn display_snapshot_for_runner(
        runner: &Runner,
        mode: DisplayMode,
        sequence: u64,
        requested_max_nodes: usize,
        requested_max_edges: usize,
        include_edges: bool,
    ) -> anyhow::Result<DisplaySnapshot> {
        Self::display_snapshot_for_runner_in_region(
            runner,
            mode,
            sequence,
            requested_max_nodes,
            requested_max_edges,
            include_edges,
            None,
        )
    }

    /// Return a bounded tile for a renderer. Candidate neurons are filtered
    /// against the requested world-space region before sampling, so a dense
    /// tile does not disappear just because it was absent from the global
    /// overview sample.
    pub fn display_snapshot_for_runner_in_region(
        runner: &Runner,
        mode: DisplayMode,
        sequence: u64,
        requested_max_nodes: usize,
        requested_max_edges: usize,
        include_edges: bool,
        region: Option<AxisAlignedBox>,
    ) -> anyhow::Result<DisplaySnapshot> {
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

        let hidden_layers = runner.net.num_hidden_layers;
        let mut layer_counts = Vec::with_capacity(hidden_layers + 2);
        layer_counts.push(runner.net.num_sensory_neurons);
        for layer in 0..hidden_layers {
            layer_counts.push(runner.layer_size(layer));
        }
        layer_counts.push(runner.net.num_output_neurons);

        let topology_epoch = display_topology_epoch(&layer_counts);
        let use_synthetic = matches!(mode, DisplayMode::SyntheticColumns);
        if let Some(region) = region {
            region
                .validate()
                .map_err(|error| anyhow::anyhow!("invalid display tile region: {error}"))?;
        }
        #[cfg(feature = "growth3d")]
        if include_edges && region.is_none() {
            if let Some(reconstruction) = &runner.procedural_reconstruction {
                return if use_synthetic {
                    reconstruction
                        .synthetic_display_snapshot(sequence, max_nodes, max_edges)
                        .map_err(Into::into)
                } else {
                    #[cfg(feature = "morpho")]
                    if let Some(snapshot) = live_morphology_display_snapshot(
                        runner, sequence, max_nodes, max_edges, region,
                    )? {
                        return Ok(snapshot);
                    }
                    reconstruction
                        .display_snapshot(sequence, max_nodes, max_edges)
                        .map_err(Into::into)
                };
            }
        }
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if !use_synthetic && include_edges {
            if let Some(snapshot) =
                live_morphology_display_snapshot(runner, sequence, max_nodes, max_edges, region)?
            {
                return Ok(snapshot);
            }
        }
        #[cfg(feature = "growth3d")]
        if !use_synthetic && include_edges && region.is_none() {
            if let Some(reconstruction) = &runner.procedural_reconstruction {
                return reconstruction
                    .display_snapshot(sequence, max_nodes, max_edges)
                    .map_err(Into::into);
            }
        }
        let has_legacy_points = {
            #[cfg(feature = "growth3d")]
            {
                !runner.topo.sensory_nodes.is_empty()
                    || runner.topo.layers.iter().any(|layer| !layer.is_empty())
                    || !runner.topo.output_nodes.is_empty()
            }
            #[cfg(not(feature = "growth3d"))]
            {
                false
            }
        };
        let provenance = if use_synthetic {
            DisplayProvenance::SyntheticTopology
        } else if has_legacy_points {
            DisplayProvenance::ProceduralAnatomy
        } else {
            DisplayProvenance::Unavailable
        };
        let unavailable_reason = if use_synthetic {
            None
        } else if has_legacy_points {
            Some("legacy topology exposes procedural soma points only; physical neurite paths are unavailable".to_owned())
        } else {
            Some("no anatomical geometry is available in this snapshot".to_owned())
        };

        let display_position = |layer: usize, index: usize| {
            let role = display_role(layer, hidden_layers);
            if use_synthetic {
                synthetic_display_position(layer, index, layer_counts.len(), layer_counts[layer])
            } else {
                legacy_display_position(runner, role, layer, index).unwrap_or_else(|| {
                    synthetic_display_position(
                        layer,
                        index,
                        layer_counts.len(),
                        layer_counts[layer],
                    )
                })
            }
        };
        let (samples_by_layer, regional_node_count) = if let Some(region) = region {
            display_layer_samples_in_region(&layer_counts, max_nodes, region, display_position)
        } else {
            (display_layer_samples(&layer_counts, max_nodes), 0)
        };
        let mut nodes = Vec::with_capacity(max_nodes);
        for layer in 0..layer_counts.len() {
            for &index in &samples_by_layer[layer] {
                let role = display_role(layer, hidden_layers);
                let position = if use_synthetic {
                    synthetic_display_position(
                        layer,
                        index,
                        layer_counts.len(),
                        layer_counts[layer],
                    )
                } else {
                    legacy_display_position(&runner, role, layer, index).unwrap_or_else(|| {
                        synthetic_display_position(
                            layer,
                            index,
                            layer_counts.len(),
                            layer_counts[layer],
                        )
                    })
                };
                nodes.push(DisplayNode {
                    id: legacy_display_id(role, layer, index),
                    role,
                    layer: matches!(role, DisplayRole::Hidden).then_some(layer.saturating_sub(1)),
                    position_mm: position,
                    kind: AnatomicalKind::Soma,
                    soma_radius_mm: None,
                    colour_slot: 0,
                });
            }
        }

        let mut edges = Vec::with_capacity(max_edges.min(4096));
        let mut omitted_edges = false;
        if include_edges {
            omitted_edges |= add_display_matrix_edges(
                &runner.w_in,
                DisplayRole::Sensory,
                DisplayRole::Hidden,
                0,
                1,
                "input",
                &samples_by_layer[0],
                samples_by_layer
                    .get(1)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                max_edges,
                &mut edges,
            );
            for (layer, matrix) in runner.w_hh_fwd.iter().enumerate() {
                omitted_edges |= add_display_matrix_edges(
                    matrix,
                    DisplayRole::Hidden,
                    DisplayRole::Hidden,
                    layer + 1,
                    layer + 2,
                    "forward",
                    samples_by_layer
                        .get(layer + 1)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    samples_by_layer
                        .get(layer + 2)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    max_edges,
                    &mut edges,
                );
            }
            for (layer, matrix) in runner.w_hh_bwd.iter().enumerate() {
                omitted_edges |= add_display_matrix_edges(
                    matrix,
                    DisplayRole::Hidden,
                    DisplayRole::Hidden,
                    layer + 2,
                    layer + 1,
                    "backward",
                    samples_by_layer
                        .get(layer + 2)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    samples_by_layer
                        .get(layer + 1)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    max_edges,
                    &mut edges,
                );
            }
            for (layer, matrix) in runner.w_hh_rec.iter().enumerate() {
                omitted_edges |= add_display_matrix_edges(
                    matrix,
                    DisplayRole::Hidden,
                    DisplayRole::Hidden,
                    layer + 1,
                    layer + 1,
                    "recurrent",
                    samples_by_layer
                        .get(layer + 1)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    samples_by_layer
                        .get(layer + 1)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    max_edges,
                    &mut edges,
                );
            }
            if hidden_layers > 0 {
                omitted_edges |= add_display_matrix_edges(
                    &runner.w_out,
                    DisplayRole::Hidden,
                    DisplayRole::Output,
                    hidden_layers,
                    hidden_layers + 1,
                    "output",
                    samples_by_layer
                        .get(hidden_layers)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    samples_by_layer
                        .get(hidden_layers + 1)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    max_edges,
                    &mut edges,
                );
            }
        }
        let visible_node_count = nodes.len();
        let total_node_count = layer_counts
            .iter()
            .fold(0usize, |total, count| total.saturating_add(*count));
        let sampled_nodes = visible_node_count
            < if region.is_some() {
                regional_node_count
            } else {
                total_node_count
            };
        let coverage = if let Some(region) = region {
            Some(region)
        } else if use_synthetic || !has_legacy_points {
            None
        } else {
            display_bounds(&nodes)
        };
        let mut snapshot = DisplaySnapshot::bounded(
            1,
            topology_epoch,
            topology_epoch,
            sequence,
            mode,
            provenance,
            coverage,
            nodes,
            edges,
            max_nodes,
            max_edges,
            unavailable_reason,
        )?;
        if sampled_nodes || omitted_edges {
            snapshot.coverage.truncated = true;
            snapshot.coverage.complete = false;
        }
        Ok(snapshot)
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
        let mut cfg: crate::config::NetworkConfig = serde_json::from_str(config_json)?;
        cfg.deployment.migrate_legacy_import();
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
        // Older runtime snapshots did not persist deployment metadata. Keep a
        // deployment policy already supplied by the workspace manifest when
        // importing one of those snapshots; otherwise the next autosave would
        // silently turn a cluster deployment back into the default policy.
        let manifest_deployment = self.spec.net.deployment.clone();
        self.runner.import_network_json(snapshot_json)?;
        if manifest_deployment != crate::deployment::DeploymentConfig::default() {
            self.runner.net.deployment = manifest_deployment;
        }
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
            sensory_history: Vec::new(),
            output_history: Vec::new(),
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

#[cfg(all(feature = "morpho", feature = "growth3d"))]
fn live_morphology_display_snapshot(
    runner: &Runner,
    sequence: u64,
    max_nodes: usize,
    max_edges: usize,
    region: Option<AxisAlignedBox>,
) -> anyhow::Result<Option<DisplaySnapshot>> {
    let morphology = &runner.morph;
    let has_geometry = morphology.somas.iter().any(|layer| !layer.is_empty())
        || !morphology.sensory_somas.is_empty()
        || !morphology.output_somas.is_empty()
        || morphology
            .axons
            .iter()
            .flatten()
            .any(|axon| !axon.segments.is_empty())
        || morphology
            .dendrites
            .iter()
            .flatten()
            .any(|dendrite| !dendrite.tree.branches.is_empty())
        || !morphology.synapses.is_empty();
    if !has_geometry {
        return Ok(None);
    }

    let hidden_layers = runner.net.num_hidden_layers;
    let layer_counts = std::iter::once(runner.net.num_sensory_neurons)
        .chain((0..hidden_layers).map(|layer| runner.layer_size(layer)))
        .chain(std::iter::once(runner.net.num_output_neurons))
        .collect::<Vec<_>>();
    let total_node_count = layer_counts
        .iter()
        .fold(0usize, |total, count| total.saturating_add(*count));
    let soma_position = |role: DisplayRole, layer: usize, index: usize| {
        let point = match role {
            DisplayRole::Sensory => morphology.sensory_somas.get(index).map(|soma| soma.pos),
            DisplayRole::Hidden => morphology
                .somas
                .get(layer.saturating_sub(1))
                .and_then(|somas| somas.get(index))
                .map(|soma| soma.pos),
            DisplayRole::Output => morphology.output_somas.get(index).map(|soma| soma.pos),
            DisplayRole::Unassigned => None,
        }?;
        Some(Vec3 {
            x: f64::from(point.x),
            y: f64::from(point.y),
            z: f64::from(point.z),
        })
    };
    let display_position = |layer: usize, index: usize| {
        let role = display_role(layer, hidden_layers);
        soma_position(role, layer, index)
            .or_else(|| legacy_display_position(runner, role, layer, index))
            .unwrap_or_else(|| {
                synthetic_display_position(layer, index, layer_counts.len(), layer_counts[layer])
            })
    };
    let (samples_by_layer, regional_node_count) = if let Some(region) = region {
        display_layer_samples_in_region(&layer_counts, max_nodes, region, display_position)
    } else {
        (
            display_layer_samples(&layer_counts, max_nodes),
            total_node_count,
        )
    };
    let visible_capacity = if region.is_some() {
        regional_node_count.min(max_nodes)
    } else {
        total_node_count.min(max_nodes)
    };
    let mut nodes = Vec::with_capacity(visible_capacity);
    for (layer, sample_indices) in samples_by_layer.iter().enumerate() {
        let role = display_role(layer, hidden_layers);
        for &index in sample_indices {
            let position = soma_position(role, layer, index)
                .or_else(|| legacy_display_position(runner, role, layer, index))
                .unwrap_or_else(|| {
                    synthetic_display_position(
                        layer,
                        index,
                        layer_counts.len(),
                        layer_counts[layer],
                    )
                });
            nodes.push(DisplayNode {
                id: legacy_display_id(role, layer, index),
                role,
                layer: matches!(role, DisplayRole::Hidden).then_some(layer.saturating_sub(1)),
                position_mm: position,
                kind: AnatomicalKind::Soma,
                // The legacy morphology model stores centre positions but
                // no physical soma radius. Do not invent a volume measure.
                soma_radius_mm: None,
                colour_slot: 0,
            });
        }
    }

    let node_for = |role: DisplayRole, layer: usize, index: usize| {
        (index < layer_counts.get(layer).copied().unwrap_or(0)
            && samples_by_layer
                .get(layer)
                .is_some_and(|samples| samples.binary_search(&index).is_ok()))
        .then(|| legacy_display_id(role, layer, index))
    };
    let display_item_limit = max_edges.max(1);
    let mut paths = Vec::with_capacity(display_item_limit.min(4096));
    let mut omitted_paths = false;
    // Keep the derived path identity namespace separate from the legacy
    // display soma IDs. The owner identity is still the stable soma ID.
    let mut path_value = 1u64 << 32;
    let mut add_segments = |owner: AnatomicalId,
                            kind: AnatomicalKind,
                            segments: Box<
        dyn Iterator<Item = (crate::morphology::Point3, crate::morphology::Point3)> + '_,
    >,
                            paths: &mut Vec<DisplayPath>,
                            path_value: &mut u64| {
        for (from, to) in segments {
            if region.is_some_and(|tile| !segment_overlaps_region(tile, from, to)) {
                continue;
            }
            if paths.len() >= display_item_limit {
                omitted_paths = true;
                break;
            }
            if !from.x.is_finite()
                || !from.y.is_finite()
                || !from.z.is_finite()
                || !to.x.is_finite()
                || !to.y.is_finite()
                || !to.z.is_finite()
            {
                continue;
            }
            let id = AnatomicalId::new(*path_value, 1).ok();
            *path_value = path_value.saturating_add(1);
            if let Some(id) = id {
                paths.push(DisplayPath {
                    id,
                    owner,
                    kind,
                    points_mm: vec![
                        Vec3 {
                            x: f64::from(from.x),
                            y: f64::from(from.y),
                            z: f64::from(from.z),
                        },
                        Vec3 {
                            x: f64::from(to.x),
                            y: f64::from(to.y),
                            z: f64::from(to.z),
                        },
                    ],
                    // Endpoints are the committed grown segment geometry, so
                    // clients preserve its actual length and placement. The
                    // legacy model has no stored radius; zero deliberately
                    // keeps volumetric stages unavailable.
                    radius_mm: 0.0,
                });
            }
        }
    };
    for (layer, sample_indices) in samples_by_layer
        .iter()
        .enumerate()
        .skip(1)
        .take(hidden_layers)
    {
        for &index in sample_indices {
            let Some(soma) = morphology
                .somas
                .get(layer - 1)
                .and_then(|somas| somas.get(index))
            else {
                continue;
            };
            let Some(owner) = node_for(DisplayRole::Hidden, layer, index) else {
                continue;
            };
            if let Some(axon) = morphology
                .axons
                .get(layer - 1)
                .and_then(|items| items.get(soma.id))
            {
                add_segments(
                    owner,
                    AnatomicalKind::Axon,
                    Box::new(axon.segments.iter().map(|s| (s.from, s.to))),
                    &mut paths,
                    &mut path_value,
                );
            }
            if let Some(dendrite) = morphology
                .dendrites
                .get(layer)
                .and_then(|items| items.get(soma.id))
            {
                add_segments(
                    owner,
                    AnatomicalKind::Dendrite,
                    Box::new(dendrite.tree.branches.iter().map(|s| (s.from, s.to))),
                    &mut paths,
                    &mut path_value,
                );
            }
        }
    }
    for (role, somas, axons, dendrites, layer, sample_indices) in [
        (
            DisplayRole::Sensory,
            &morphology.sensory_somas,
            &morphology.sensory_axons,
            &morphology.sensory_dendrites,
            0,
            samples_by_layer[0].as_slice(),
        ),
        (
            DisplayRole::Output,
            &morphology.output_somas,
            &morphology.output_axons,
            &morphology.output_dendrites,
            hidden_layers + 1,
            samples_by_layer[hidden_layers + 1].as_slice(),
        ),
    ] {
        for &index in sample_indices {
            if somas.get(index).is_none() {
                continue;
            }
            let Some(owner) = node_for(role, layer, index) else {
                continue;
            };
            if let Some(axon) = axons.get(index) {
                add_segments(
                    owner,
                    AnatomicalKind::Axon,
                    Box::new(axon.segments.iter().map(|s| (s.from, s.to))),
                    &mut paths,
                    &mut path_value,
                );
            }
            if let Some(dendrite) = dendrites.get(index) {
                add_segments(
                    owner,
                    AnatomicalKind::Dendrite,
                    Box::new(dendrite.tree.branches.iter().map(|s| (s.from, s.to))),
                    &mut paths,
                    &mut path_value,
                );
            }
        }
    }

    const MAX_DISPLAY_BRANCH_DEPTH: usize = 128;
    let axon_branch_points = |segments: &[crate::morphology::AxonSeg], terminal: Option<usize>| {
        let Some(mut index) = terminal.filter(|index| *index < segments.len()) else {
            return Vec::new();
        };
        let mut chain = Vec::with_capacity(MAX_DISPLAY_BRANCH_DEPTH);
        while chain.len() < MAX_DISPLAY_BRANCH_DEPTH {
            chain.push(index);
            let Some(parent) = segments[index].parent_idx else {
                break;
            };
            if parent >= segments.len() {
                break;
            }
            index = parent;
        }
        chain.reverse();
        let mut points = Vec::new();
        for index in chain {
            let segment = &segments[index];
            if points.is_empty() {
                points.push(segment.from);
            }
            points.push(segment.to);
        }
        points
    };
    let dendrite_branch_points = |segments: &[crate::morphology::DendSeg],
                                  terminal: Option<usize>| {
        let Some(mut index) = terminal.filter(|index| *index < segments.len()) else {
            return Vec::new();
        };
        let mut chain = Vec::with_capacity(MAX_DISPLAY_BRANCH_DEPTH);
        while chain.len() < MAX_DISPLAY_BRANCH_DEPTH {
            chain.push(index);
            let Some(parent) = segments[index].parent_idx else {
                break;
            };
            if parent >= segments.len() {
                break;
            }
            index = parent;
        }
        chain.reverse();
        let mut points = Vec::new();
        for index in chain {
            let segment = &segments[index];
            if points.is_empty() {
                points.push(segment.from);
            }
            points.push(segment.to);
        }
        points
    };
    let point = |p: crate::morphology::Point3| Vec3 {
        x: f64::from(p.x),
        y: f64::from(p.y),
        z: f64::from(p.z),
    };
    let (sampled_synapse_layers, regional_synapse_count) = if let Some(region) = region {
        display_layer_samples_in_region(
            &[morphology.synapses.len()],
            display_item_limit,
            region,
            |_, index| {
                let synapse = &morphology.synapses[index];
                Vec3 {
                    x: (f64::from(synapse.pre_site.x) + f64::from(synapse.post_site.x)) * 0.5,
                    y: (f64::from(synapse.pre_site.y) + f64::from(synapse.post_site.y)) * 0.5,
                    z: (f64::from(synapse.pre_site.z) + f64::from(synapse.post_site.z)) * 0.5,
                }
            },
        )
    } else {
        (
            display_layer_samples(&[morphology.synapses.len()], display_item_limit),
            morphology.synapses.len(),
        )
    };
    let sampled_synapses = sampled_synapse_layers
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut omitted_display_items = sampled_synapses.len() < regional_synapse_count;
    let mut edges = Vec::with_capacity(display_item_limit.min(4096));
    let mut markers = Vec::with_capacity(display_item_limit.min(4096));
    for synapse_index in sampled_synapses {
        let Some(synapse) = morphology.synapses.get(synapse_index) else {
            continue;
        };
        let (source, target, axon_segments, dendrite_segments) = match synapse.kind {
            crate::morphology::SynKind::In => (
                node_for(DisplayRole::Sensory, 0, synapse.pre_id),
                node_for(
                    DisplayRole::Hidden,
                    synapse.post_layer.max(0) as usize + 1,
                    synapse.post_id,
                ),
                morphology
                    .sensory_axons
                    .get(synapse.pre_id)
                    .map(|axon| axon.segments.as_slice()),
                morphology
                    .dendrites
                    .get(synapse.post_layer.max(0) as usize)
                    .and_then(|items| items.get(synapse.post_id))
                    .map(|dendrite| dendrite.tree.branches.as_slice()),
            ),
            crate::morphology::SynKind::Out => (
                node_for(
                    DisplayRole::Hidden,
                    synapse.pre_layer.max(0) as usize + 1,
                    synapse.pre_id,
                ),
                node_for(DisplayRole::Output, hidden_layers + 1, synapse.post_id),
                morphology
                    .axons
                    .get(synapse.pre_layer.max(0) as usize)
                    .and_then(|items| items.get(synapse.pre_id))
                    .map(|axon| axon.segments.as_slice()),
                morphology
                    .output_dendrites
                    .get(synapse.post_id)
                    .map(|dendrite| dendrite.tree.branches.as_slice()),
            ),
            crate::morphology::SynKind::HiddenFwd
            | crate::morphology::SynKind::HiddenBwd
            | crate::morphology::SynKind::HiddenRec => (
                node_for(
                    DisplayRole::Hidden,
                    synapse.pre_layer.max(0) as usize + 1,
                    synapse.pre_id,
                ),
                node_for(
                    DisplayRole::Hidden,
                    synapse.post_layer.max(0) as usize + 1,
                    synapse.post_id,
                ),
                morphology
                    .axons
                    .get(synapse.pre_layer.max(0) as usize)
                    .and_then(|items| items.get(synapse.pre_id))
                    .map(|axon| axon.segments.as_slice()),
                morphology
                    .dendrites
                    .get(synapse.post_layer.max(0) as usize)
                    .and_then(|items| items.get(synapse.post_id))
                    .map(|dendrite| dendrite.tree.branches.as_slice()),
            ),
        };
        let (Some(source), Some(target)) = (source, target) else {
            continue;
        };
        if paths
            .len()
            .saturating_add(edges.len())
            .saturating_add(markers.len())
            .saturating_add(4)
            > display_item_limit
        {
            omitted_display_items = true;
            continue;
        }
        let marker_base = (1u64 << 48).saturating_add((synapse_index as u64).saturating_mul(4));
        if let Ok(id) = AnatomicalId::new(marker_base.saturating_add(1), 1) {
            markers.push(DisplayMarker {
                id,
                owner: source,
                kind: AnatomicalKind::Bouton,
                position_mm: point(synapse.pre_site),
                synapse_id: None,
            });
        }
        if let Ok(id) = AnatomicalId::new(marker_base.saturating_add(2), 1) {
            markers.push(DisplayMarker {
                id,
                owner: target,
                kind: AnatomicalKind::PostsynapticSite,
                position_mm: point(synapse.post_site),
                synapse_id: None,
            });
        }
        if let Ok(id) = AnatomicalId::new(marker_base.saturating_add(3), 1) {
            markers.push(DisplayMarker {
                id,
                owner: source,
                kind: AnatomicalKind::Synapse,
                position_mm: Vec3 {
                    x: (f64::from(synapse.pre_site.x) + f64::from(synapse.post_site.x)) * 0.5,
                    y: (f64::from(synapse.pre_site.y) + f64::from(synapse.post_site.y)) * 0.5,
                    z: (f64::from(synapse.pre_site.z) + f64::from(synapse.post_site.z)) * 0.5,
                },
                synapse_id: None,
            });
        }
        let mut route = axon_segments
            .map(|segments| axon_branch_points(segments, synapse.axon_seg_idx))
            .unwrap_or_default();
        let dendrite = dendrite_segments
            .map(|segments| dendrite_branch_points(segments, synapse.dend_seg_idx))
            .unwrap_or_default();
        let mut points = route.drain(..).map(point).collect::<Vec<_>>();
        if points.is_empty() {
            points.push(point(synapse.pre_site));
        }
        points.push(point(synapse.pre_site));
        if dendrite.is_empty() {
            points.push(point(synapse.post_site));
        } else {
            for p in dendrite.iter().rev() {
                points.push(point(*p));
            }
        }
        points.push(point(synapse.post_site));
        edges.push(DisplayEdge {
            source,
            target,
            points_mm: points,
            multiplicity: 1,
            kind: "live_morphology_route".to_owned(),
        });
    }
    drop(add_segments);
    // The electrical graph can already contain weighted links while physical
    // synapse routes are still growing. Keep those links available to the
    // schematic stage-5/6 views without representing them as neurite paths.
    // A physical route takes precedence for the same pair of neurons.
    omitted_display_items |=
        append_missing_live_graph_edges(runner, &samples_by_layer, display_item_limit, &mut edges);
    omitted_paths |= paths.len() >= display_item_limit;
    let mut hash = display_topology_epoch(&layer_counts);
    hash ^= (paths.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    hash ^= (edges.len() as u64).rotate_left(17);
    let coverage = region.or_else(|| {
        morphology
            .skull_membrane
            .map(|membrane| {
                let radii =
                    membrane
                        .radii
                        .unwrap_or((membrane.radius, membrane.radius, membrane.radius));
                let centre = Vec3 {
                    x: f64::from(membrane.center.x),
                    y: f64::from(membrane.center.y),
                    z: f64::from(membrane.center.z),
                };
                let extent = Vec3 {
                    x: f64::from(radii.0.abs()),
                    y: f64::from(radii.1.abs()),
                    z: f64::from(radii.2.abs()),
                };
                AxisAlignedBox {
                    min: Vec3 {
                        x: centre.x - extent.x,
                        y: centre.y - extent.y,
                        z: centre.z - extent.z,
                    },
                    max: Vec3 {
                        x: centre.x + extent.x,
                        y: centre.y + extent.y,
                        z: centre.z + extent.z,
                    },
                }
            })
            .or_else(|| display_bounds(&nodes))
    });
    let mut snapshot = DisplaySnapshot::bounded_with_paths_and_markers(
        hash.max(1),
        hash.max(1),
        hash.max(1),
        sequence,
        DisplayMode::Anatomical,
        DisplayProvenance::ProceduralAnatomy,
        coverage,
        nodes,
        edges,
        paths,
        markers,
        max_nodes,
        max_edges,
        None,
    )?;
    if snapshot.nodes.len() < regional_node_count || omitted_paths || omitted_display_items {
        snapshot.coverage.truncated = true;
        snapshot.coverage.complete = false;
    }
    snapshot.coverage.membrane = morphology.skull_membrane.and_then(|membrane| {
        let (x, y, z) =
            membrane
                .radii
                .unwrap_or((membrane.radius, membrane.radius, membrane.radius));
        let centre_mm = Vec3 {
            x: membrane.center.x as f64,
            y: membrane.center.y as f64,
            z: membrane.center.z as f64,
        };
        let radii_mm = Vec3 {
            x: x as f64,
            y: y as f64,
            z: z as f64,
        };
        (centre_mm.is_finite() && radii_mm.is_finite() && x > 0.0 && y > 0.0 && z > 0.0).then_some(
            crate::morphology_contract::DisplayMembrane {
                centre_mm,
                radii_mm,
            },
        )
    });
    Ok(Some(snapshot))
}

fn topology_node_id(layer: usize, index: usize) -> String {
    match layer {
        0 => format!("sensory:{index}"),
        _ => format!("layer:{layer}:{index}"),
    }
}

fn point_inside_region(region: AxisAlignedBox, point: Vec3) -> bool {
    point.is_finite()
        && point.x >= region.min.x
        && point.x <= region.max.x
        && point.y >= region.min.y
        && point.y <= region.max.y
        && point.z >= region.min.z
        && point.z <= region.max.z
}

#[cfg(all(feature = "morpho", feature = "growth3d"))]
fn segment_overlaps_region(
    region: AxisAlignedBox,
    from: crate::morphology::Point3,
    to: crate::morphology::Point3,
) -> bool {
    let (from, to) = (
        Vec3 {
            x: f64::from(from.x),
            y: f64::from(from.y),
            z: f64::from(from.z),
        },
        Vec3 {
            x: f64::from(to.x),
            y: f64::from(to.y),
            z: f64::from(to.z),
        },
    );
    from.is_finite()
        && to.is_finite()
        && from.x.min(to.x) <= region.max.x
        && from.x.max(to.x) >= region.min.x
        && from.y.min(to.y) <= region.max.y
        && from.y.max(to.y) >= region.min.y
        && from.z.min(to.z) <= region.max.z
        && from.z.max(to.z) >= region.min.z
}

/// Two-pass deterministic regional sampling uses memory proportional to the
/// requested tile budget rather than the number of neurons in the network.
/// The first pass counts visible candidates per layer; the second selects
/// evenly distributed candidate ranks from each layer.
fn display_layer_samples_in_region<F>(
    layer_counts: &[usize],
    max_nodes: usize,
    region: AxisAlignedBox,
    mut position_for: F,
) -> (Vec<Vec<usize>>, usize)
where
    F: FnMut(usize, usize) -> Vec3,
{
    let candidate_counts = layer_counts
        .iter()
        .enumerate()
        .map(|(layer, count)| {
            (0..*count)
                .filter(|index| point_inside_region(region, position_for(layer, *index)))
                .count()
        })
        .collect::<Vec<_>>();
    let total_candidates = candidate_counts
        .iter()
        .fold(0usize, |total, count| total.saturating_add(*count));
    let ranks_by_layer = display_layer_samples(&candidate_counts, max_nodes)
        .into_iter()
        .map(|ranks| ranks.into_iter().collect::<std::collections::BTreeSet<_>>())
        .collect::<Vec<_>>();
    let mut samples = Vec::with_capacity(layer_counts.len());
    for (layer, count) in layer_counts.iter().enumerate() {
        let selected_ranks = ranks_by_layer.get(layer);
        let mut candidate_rank = 0usize;
        let mut layer_samples = Vec::new();
        for index in 0..*count {
            if !point_inside_region(region, position_for(layer, index)) {
                continue;
            }
            if selected_ranks.is_some_and(|ranks| ranks.contains(&candidate_rank)) {
                layer_samples.push(index);
            }
            candidate_rank = candidate_rank.saturating_add(1);
        }
        samples.push(layer_samples);
    }
    (samples, total_candidates)
}

fn display_role(layer: usize, hidden_layers: usize) -> DisplayRole {
    if layer == 0 {
        DisplayRole::Sensory
    } else if layer == hidden_layers + 1 {
        DisplayRole::Output
    } else {
        DisplayRole::Hidden
    }
}

fn legacy_display_id(role: DisplayRole, layer: usize, index: usize) -> AnatomicalId {
    let role_tag = match role {
        DisplayRole::Sensory => 1u64,
        DisplayRole::Hidden => 2,
        DisplayRole::Output => 3,
        DisplayRole::Unassigned => 4,
    };
    // The IDs are stable within a topology generation and are deliberately
    // scoped to this legacy display adapter. They are not persisted biological
    // ownership IDs until the legacy dense topology has been migrated.
    let value =
        (role_tag << 60) | ((layer as u64 & 0x0fff_ffff) << 32) | (index as u64).saturating_add(1);
    AnatomicalId::new(value.max(1), 1).expect("display identity is non-zero")
}

pub fn display_neuron_id(role: DisplayRole, layer: usize, index: usize) -> AnatomicalId {
    legacy_display_id(role, layer, index)
}

fn synthetic_display_position(
    layer: usize,
    index: usize,
    layer_count: usize,
    layer_size: usize,
) -> Vec3 {
    let x = if layer_count <= 1 {
        0.0
    } else {
        -1.0 + 2.0 * layer as f64 / (layer_count - 1) as f64
    };
    let y = if layer_size <= 1 {
        0.0
    } else {
        -1.0 + 2.0 * index as f64 / (layer_size - 1) as f64
    };
    Vec3 { x, y, z: 0.0 }
}

#[cfg(feature = "growth3d")]
fn legacy_display_position(
    runner: &Runner,
    role: DisplayRole,
    layer: usize,
    index: usize,
) -> Option<Vec3> {
    let node = match role {
        DisplayRole::Sensory => runner.topo.sensory_nodes.get(index),
        DisplayRole::Hidden => runner.topo.layers.get(layer.saturating_sub(1))?.get(index),
        DisplayRole::Output => runner.topo.output_nodes.get(index),
        DisplayRole::Unassigned => None,
    }?;
    Some(Vec3 {
        x: f64::from(node.x),
        y: f64::from(node.y),
        z: f64::from(node.z),
    })
}

#[cfg(not(feature = "growth3d"))]
fn legacy_display_position(
    _runner: &Runner,
    _role: DisplayRole,
    _layer: usize,
    _index: usize,
) -> Option<Vec3> {
    None
}

fn add_display_matrix_edges(
    matrix: &ndarray::Array2<f64>,
    source_role: DisplayRole,
    target_role: DisplayRole,
    source_layer: usize,
    target_layer: usize,
    kind: &str,
    source_indices: &[usize],
    target_indices: &[usize],
    max_edges: usize,
    edges: &mut Vec<DisplayEdge>,
) -> bool {
    for &target in target_indices {
        if target >= matrix.nrows() {
            continue;
        }
        for &source in source_indices {
            if source >= matrix.ncols() {
                continue;
            }
            let weight = matrix[(target, source)];
            if !weight.is_finite() || weight == 0.0 {
                continue;
            }
            if edges.len() >= max_edges {
                return true;
            }
            edges.push(DisplayEdge {
                source: legacy_display_id(source_role, source_layer, source),
                target: legacy_display_id(target_role, target_layer, target),
                points_mm: Vec::new(),
                multiplicity: 1,
                kind: kind.to_owned(),
            });
        }
    }
    false
}

#[cfg(all(feature = "morpho", feature = "growth3d"))]
fn append_missing_live_graph_edges(
    runner: &Runner,
    samples_by_layer: &[Vec<usize>],
    max_edges: usize,
    edges: &mut Vec<DisplayEdge>,
) -> bool {
    let hidden_layers = runner.net.num_hidden_layers;
    let mut seen = edges
        .iter()
        .map(|edge| (edge.source, edge.target))
        .collect::<std::collections::HashSet<_>>();
    let mut omitted = false;
    let mut append = |matrix: &ndarray::Array2<f64>,
                      source_role: DisplayRole,
                      target_role: DisplayRole,
                      source_layer: usize,
                      target_layer: usize,
                      kind: &str| {
        let sources = samples_by_layer
            .get(source_layer)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let targets = samples_by_layer
            .get(target_layer)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for &target in targets {
            if target >= matrix.nrows() {
                continue;
            }
            for &source in sources {
                if source >= matrix.ncols()
                    || !matrix[(target, source)].is_finite()
                    || matrix[(target, source)] == 0.0
                {
                    continue;
                }
                let source_id = legacy_display_id(source_role, source_layer, source);
                let target_id = legacy_display_id(target_role, target_layer, target);
                if !seen.insert((source_id, target_id)) {
                    continue;
                }
                if edges.len() >= max_edges {
                    omitted = true;
                    continue;
                }
                edges.push(DisplayEdge {
                    source: source_id,
                    target: target_id,
                    points_mm: Vec::new(),
                    multiplicity: 1,
                    kind: format!("graph_{kind}"),
                });
            }
        }
    };
    append(
        &runner.w_in,
        DisplayRole::Sensory,
        DisplayRole::Hidden,
        0,
        1,
        "input",
    );
    for (layer, matrix) in runner.w_hh_fwd.iter().enumerate() {
        append(
            matrix,
            DisplayRole::Hidden,
            DisplayRole::Hidden,
            layer + 1,
            layer + 2,
            "forward",
        );
    }
    for (layer, matrix) in runner.w_hh_bwd.iter().enumerate() {
        append(
            matrix,
            DisplayRole::Hidden,
            DisplayRole::Hidden,
            layer + 2,
            layer + 1,
            "backward",
        );
    }
    for (layer, matrix) in runner.w_hh_rec.iter().enumerate() {
        append(
            matrix,
            DisplayRole::Hidden,
            DisplayRole::Hidden,
            layer + 1,
            layer + 1,
            "recurrent",
        );
    }
    if hidden_layers > 0 {
        append(
            &runner.w_out,
            DisplayRole::Hidden,
            DisplayRole::Output,
            hidden_layers,
            hidden_layers + 1,
            "output",
        );
    }
    omitted
}

fn display_topology_epoch(layer_counts: &[usize]) -> u64 {
    let mut hash = 14695981039346656037u64;
    for count in layer_counts {
        hash ^= *count as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash.max(1)
}

fn display_bounds(nodes: &[DisplayNode]) -> Option<AxisAlignedBox> {
    let first = nodes.first()?.position_mm;
    let mut min = first;
    let mut max = first;
    for node in nodes.iter().skip(1) {
        let point = node.position_mm;
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        min.z = min.z.min(point.z);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
        max.z = max.z.max(point.z);
    }
    Some(AxisAlignedBox { min, max })
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
    fn spatial_tile_filters_before_applying_the_node_budget() {
        let tile = AxisAlignedBox {
            min: Vec3 {
                x: -1.0,
                y: 100.0,
                z: -1.0,
            },
            max: Vec3 {
                x: 1.0,
                y: 199.0,
                z: 1.0,
            },
        };
        let (samples, candidates) =
            display_layer_samples_in_region(&[10_000], 10, tile, |_, index| Vec3 {
                x: 0.0,
                y: index as f64,
                z: 0.0,
            });

        assert_eq!(candidates, 100);
        assert_eq!(samples[0].len(), 10);
        assert!(samples[0].iter().all(|index| (100..=199).contains(index)));
        assert_eq!(samples[0].first(), Some(&100));
        assert_eq!(samples[0].last(), Some(&199));
    }

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

    #[test]
    fn legacy_snapshot_import_preserves_manifest_deployment_policy() {
        let mut manifest_spec = EngineSpec::default();
        manifest_spec
            .net
            .deployment
            .add_mode(crate::deployment::ExecutionMode::Sharded);
        manifest_spec.net.deployment.scope = crate::deployment::ExecutionScope::Cluster;

        let source = RunnerEngine::new(EngineSpec::default()).expect("source engine");
        let snapshot = source.export_snapshot_json().expect("snapshot");
        let mut legacy: serde_json::Value = serde_json::from_str(&snapshot).expect("json");
        legacy["engine"]["net"]["deployment"] = serde_json::json!({});

        let mut engine = RunnerEngine::new(manifest_spec).expect("engine");
        engine
            .import_snapshot_json(&serde_json::to_string(&legacy).expect("legacy json"))
            .expect("legacy snapshot import");

        assert!(
            engine
                .spec()
                .net
                .deployment
                .has_mode(crate::deployment::ExecutionMode::Sharded)
        );
        assert_eq!(
            engine.spec().net.deployment.scope,
            crate::deployment::ExecutionScope::Cluster
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
    fn workspace_activity_history_tracks_admitted_sensory_steps() {
        let mut spec = EngineSpec::default();
        spec.net.num_sensory_neurons = 4;
        let mut engine = RunnerEngine::new(spec).expect("engine");
        let first = engine.step(Some(&[0, 1, 0, 0]));
        let activity = engine.activity();
        assert_eq!(activity.step, first.step);
        assert_eq!(activity.sensory_history[0].step, activity.step);
        assert_eq!(activity.sensory_history[0].indices, vec![1]);
        assert_eq!(activity.output_history[0].step, activity.step);
        engine.step(Some(&[0, 0, 0, 0]));
        let next = engine.activity();
        assert_eq!(next.sensory_history[0].indices, Vec::<usize>::new());
        assert_eq!(next.sensory_history[1].indices, vec![1]);
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

    #[test]
    fn display_snapshot_is_versioned_bounded_and_explicit_about_provenance() {
        let mut spec = EngineSpec::default();
        spec.net.num_sensory_neurons = 2;
        spec.net.num_hidden_layers = 1;
        spec.net.num_hidden_per_layer_initial = 3;
        spec.net.num_output_neurons = 1;
        let engine = RunnerEngine::new(spec).expect("engine");

        let snapshot = engine
            .display_snapshot(DisplayMode::SyntheticColumns, 7, 3, 2)
            .expect("display snapshot");
        assert_eq!(
            snapshot.schema_version.raw(),
            DisplaySnapshot::SCHEMA_VERSION
        );
        assert_eq!(snapshot.sequence, 7);
        assert_eq!(snapshot.provenance, DisplayProvenance::SyntheticTopology);
        assert!(snapshot.coverage.truncated);
        assert!(!snapshot.coverage.complete);
        assert!(snapshot.nodes.len() <= 3);
        assert!(snapshot.edges.len() <= 2);
        snapshot.validate().expect("valid display snapshot");
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    #[test]
    #[ignore = "MORPH-VIS-002: run with cargo xtask qa run --suite anatomical-growth"]
    fn anatomy_sustained_growth_capture() {
        assert_eq!(
            std::env::var("NM_MORPHO_ASYNC").as_deref(),
            Ok("0"),
            "capture requires ordered, synchronous growth"
        );
        fastrand::seed(42);
        let mut spec = EngineSpec::default();
        spec.net = serde_json::from_str(include_str!("../config.json")).unwrap();
        let mut engine = RunnerEngine::new(spec).unwrap();
        let dir = std::env::var("ANATOMY_QA_DIR").expect("QA artefact directory");
        std::fs::create_dir_all(&dir).unwrap();
        let mut metrics = Vec::new();
        let mut checked_resizes = 0;
        let mut checked_roots = 0;
        let started = std::time::Instant::now();
        for step in 0..=1200 {
            if step % 100 == 0 {
                let snapshot = engine
                    .display_snapshot(DisplayMode::Anatomical, step + 1, 512, 4096)
                    .unwrap();
                snapshot.validate().unwrap();
                let synthetic = engine
                    .display_snapshot(DisplayMode::SyntheticColumns, step + 1, 512, 4096)
                    .unwrap();
                synthetic.validate().unwrap();
                assert_eq!(
                    snapshot
                        .nodes
                        .iter()
                        .map(|n| n.id)
                        .collect::<std::collections::BTreeSet<_>>(),
                    synthetic.nodes.iter().map(|n| n.id).collect()
                );
                let longest = snapshot
                    .paths
                    .iter()
                    .map(|path| path.points_mm[0].distance(path.points_mm[1]))
                    .fold(0.0, f64::max);
                metrics.push(serde_json::json!({"step": step, "nodes": snapshot.nodes.len(), "paths": snapshot.paths.len(), "longest_segment": longest, "truncated": snapshot.coverage.truncated, "elapsed_seconds": started.elapsed().as_secs_f64()}));
                std::fs::write(
                    format!("{dir}/frame-{step:04}.json"),
                    serde_json::to_vec(&snapshot).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    format!("{dir}/synthetic-{step:04}.json"),
                    serde_json::to_vec(&synthetic).unwrap(),
                )
                .unwrap();
            }
            if step == 1200 {
                break;
            }
            // Observe the actual admission step rather than inferring it from
            // the presentation sequence. This test-only copy also covers a
            // growth clock whose boundary falls between capture frames.
            let before = engine.runner.morph.clone();
            engine.step(None);
            assert!(engine.last_step_error().is_none());
            {
                let after = &engine.runner.morph;
                if before.sensory_somas.len() != after.sensory_somas.len()
                    || before.output_somas.len() != after.output_somas.len()
                {
                    checked_resizes += 1;
                    for (old, new) in before
                        .axons
                        .iter()
                        .zip(&after.axons)
                        .flat_map(|(old, new)| old.iter().zip(new))
                    {
                        // The same step may legitimately prune and compact
                        // leaves. Root trunks are explicitly protected by the
                        // legacy model and must survive ordinary I/O growth.
                        for a in old
                            .segments
                            .iter()
                            .filter(|s| s.is_trunk && s.parent_idx.is_none())
                        {
                            checked_roots += 1;
                            assert!(
                                new.segments.iter().any(|b| b.is_trunk
                                    && b.parent_idx.is_none()
                                    && a.from.dist(b.from) <= 0.05
                                    && a.to.dist(b.to) <= 0.05),
                                "I/O formation replaced a protected axon root"
                            );
                        }
                    }
                    for (old, new) in before
                        .dendrites
                        .iter()
                        .zip(&after.dendrites)
                        .flat_map(|(old, new)| old.iter().zip(new))
                    {
                        for a in old
                            .tree
                            .branches
                            .iter()
                            .filter(|s| s.is_trunk && s.parent_idx.is_none())
                        {
                            checked_roots += 1;
                            assert!(
                                new.tree.branches.iter().any(|b| b.is_trunk
                                    && b.parent_idx.is_none()
                                    && a.from.dist(b.from) <= 0.05
                                    && a.to.dist(b.to) <= 0.05),
                                "I/O formation replaced a protected dendritic root"
                            );
                        }
                    }
                }
            }
        }
        assert!(
            checked_resizes >= 2,
            "must cross both reported development boundaries"
        );
        assert!(
            checked_roots >= 210,
            "must exercise the grown hidden population"
        );
        std::fs::write(format!("{dir}/growth-metrics.json"), serde_json::to_vec_pretty(&serde_json::json!({"seed":42,"profile":"debug CPU, synchronous offline growth; not a stimulus latency benchmark","checked_resizes":checked_resizes,"checked_protected_roots":checked_roots,"frames":metrics})).unwrap()).unwrap();
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    #[test]
    fn live_morphology_display_contains_paths_and_keeps_mode_ids_stable() {
        let mut spec = EngineSpec::default();
        spec.net.num_sensory_neurons = 1;
        spec.net.num_hidden_layers = 1;
        spec.net.num_hidden_per_layer_initial = 1;
        spec.net.num_output_neurons = 1;
        spec.net.use_morphology = true;
        let mut engine = RunnerEngine::new(spec).expect("engine");
        let soma = crate::morphology::Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        engine.runner.morph.somas = vec![vec![crate::morphology::Soma {
            id: 0,
            layer: 0,
            pos: soma,
            stimuli: 0.0,
            atp: 1.0,
            organelles: Vec::new(),
            prev_err: Default::default(),
            integral_err: Default::default(),
            region_name: None,
            type_name: None,
        }]];
        engine.runner.morph.axons = vec![vec![crate::morphology::Axon::default()]];
        engine.runner.morph.axons[0][0]
            .segments
            .push(crate::morphology::AxonSeg {
                from: soma,
                to: crate::morphology::Point3 {
                    x: soma.x + 0.15,
                    y: soma.y + 0.05,
                    z: soma.z + 0.10,
                },
                length: 0.187,
                ..Default::default()
            });
        engine.runner.morph.dendrites = vec![
            Vec::new(),
            vec![crate::morphology::Dendrite {
                neuron_layer: 0,
                neuron_id: 0,
                tree: crate::morphology::DendriticTree {
                    branches: vec![crate::morphology::DendSeg {
                        from: soma,
                        to: crate::morphology::Point3 {
                            x: soma.x - 0.12,
                            y: soma.y + 0.04,
                            z: soma.z + 0.08,
                        },
                        length: 0.15,
                        ..Default::default()
                    }],
                },
                stimuli: 0.0,
                atp: 1.0,
                organelles: Vec::new(),
            }],
        ];
        let anatomical = engine
            .display_snapshot(DisplayMode::Anatomical, 1, 64, 64)
            .expect("anatomical snapshot");
        let synthetic = engine
            .display_snapshot(DisplayMode::SyntheticColumns, 1, 64, 64)
            .expect("synthetic snapshot");
        assert_eq!(anatomical.provenance, DisplayProvenance::ProceduralAnatomy);
        assert!(!anatomical.paths.is_empty());
        assert_eq!(
            anatomical
                .nodes
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            synthetic
                .nodes
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>()
        );
        assert!(
            anatomical
                .paths
                .iter()
                .any(|path| path.kind == AnatomicalKind::Axon)
        );
        assert!(
            anatomical
                .paths
                .iter()
                .any(|path| path.kind == AnatomicalKind::Dendrite)
        );
        let owner = legacy_display_id(DisplayRole::Hidden, 1, 0);
        let committed_segments = [
            (
                AnatomicalKind::Axon,
                engine.runner.morph.axons[0][0].segments[0].from,
                engine.runner.morph.axons[0][0].segments[0].to,
            ),
            (
                AnatomicalKind::Dendrite,
                engine.runner.morph.dendrites[1][0].tree.branches[0].from,
                engine.runner.morph.dendrites[1][0].tree.branches[0].to,
            ),
        ];
        for (kind, from, to) in committed_segments {
            // The legacy segment stores coordinates as f32. Compare the
            // promoted endpoints exactly, then allow the expected f32 rounding
            // when comparing f64 display length with Point3::dist. Match by
            // stable owner and anatomical kind because other population
            // arbors may be present in the same bounded snapshot.
            let expected_from = Vec3 {
                x: f64::from(from.x),
                y: f64::from(from.y),
                z: f64::from(from.z),
            };
            let expected_to = Vec3 {
                x: f64::from(to.x),
                y: f64::from(to.y),
                z: f64::from(to.z),
            };
            let path = anatomical
                .paths
                .iter()
                .find(|path| {
                    path.owner == owner
                        && path.kind == kind
                        && path.points_mm.len() == 2
                        && path.points_mm[0] == expected_from
                        && path.points_mm[1] == expected_to
                })
                .unwrap_or_else(|| {
                    panic!(
                        "committed segment missing for {owner:?} {kind:?} {expected_from:?}->{expected_to:?}; available={:?}",
                        anatomical
                            .paths
                            .iter()
                            .map(|path| (path.owner, path.kind, &path.points_mm))
                            .collect::<Vec<_>>()
                    )
                });
            let measured_length = path.points_mm[0].distance(path.points_mm[1]);
            let committed_length = f64::from(from.dist(to));
            assert!((measured_length - committed_length).abs() < 1.0e-6);
            assert_eq!(
                path.radius_mm, 0.0,
                "legacy radii are unavailable, not guessed"
            );
        }
        assert!(
            anatomical
                .nodes
                .iter()
                .all(|node| node.soma_radius_mm.is_none())
        );
        assert!(!anatomical.coverage.volumetric_clearance_verified);
        assert!(
            anatomical
                .markers
                .iter()
                .any(|marker| marker.kind == AnatomicalKind::Synapse)
        );
        anatomical.validate().expect("valid anatomical snapshot");
        synthetic.validate().expect("valid synthetic snapshot");
    }

    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    #[test]
    fn live_anatomical_view_keeps_weighted_graph_links_before_synapses_grow() {
        let mut spec = EngineSpec::default();
        spec.net.num_sensory_neurons = 0;
        spec.net.num_hidden_layers = 2;
        spec.net.num_hidden_per_layer_initial = 1;
        spec.net.num_output_neurons = 0;
        spec.net.use_morphology = true;
        let mut engine = RunnerEngine::new(spec).expect("engine");
        let soma = |layer, x| crate::morphology::Soma {
            id: 0,
            layer,
            pos: crate::morphology::Point3 { x, y: 0.0, z: 0.0 },
            stimuli: 0.0,
            atp: 1.0,
            organelles: Vec::new(),
            prev_err: Default::default(),
            integral_err: Default::default(),
            region_name: None,
            type_name: None,
        };
        engine.runner.morph.somas = vec![vec![soma(0, -0.25)], vec![soma(1, 0.25)]];
        engine.runner.morph.synapses.clear();
        engine.runner.w_hh_fwd[0][(0, 0)] = 0.75;

        let anatomical = engine
            .display_snapshot(DisplayMode::Anatomical, 1, 64, 64)
            .expect("anatomical snapshot");
        assert!(anatomical.edges.iter().any(|edge| {
            edge.source == legacy_display_id(DisplayRole::Hidden, 1, 0)
                && edge.target == legacy_display_id(DisplayRole::Hidden, 2, 0)
                && edge.kind == "graph_forward"
                && edge.points_mm.is_empty()
        }));
        assert!(!anatomical.coverage.volumetric_clearance_verified);
        assert_eq!(
            crate::visualization::highest_supported_stage(None, Some(&anatomical)),
            Some(crate::visualization::VisualizationStage::AnatomicalBranchingEdges),
        );
        anatomical.validate().expect("valid anatomical snapshot");
    }
}
