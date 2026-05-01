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
        self.runner.import_config_json(config_json)?;
        self.spec.net = self.runner.net.clone();
        self.clear_activity();
        Ok(())
    }

    pub fn import_snapshot_json(&mut self, snapshot_json: &str) -> anyhow::Result<()> {
        self.runner.import_network_json(snapshot_json)?;
        self.spec.net = self.runner.net.clone();
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
        let out = self.runner.step(sensory_spikes);
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
