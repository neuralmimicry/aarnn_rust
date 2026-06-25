use crate::aer::SynapseId;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointId {
    pub node_slot: u16,
    pub fpaa_index: Option<u8>,
    pub neuron_id: Option<u32>,
    pub endpoint_type: EndpointType,
    pub bouton_id: Option<u32>,
    pub io_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointType {
    AxonBouton,
    DendriteBouton,
    SensoryInput,
    MotorOutput,
    HostSubscriber,
}

impl std::fmt::Display for EndpointType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::AxonBouton => "axon_bouton",
            Self::DendriteBouton => "dendrite_bouton",
            Self::SensoryInput => "sensory_input",
            Self::MotorOutput => "motor_output",
            Self::HostSubscriber => "host_subscriber",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointLocation {
    LocalSameFpaa,
    LocalSamePika,
    LocalSameBridge,
    RemoteBridge,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointRoute {
    LocalSameFpaa,
    LocalSamePika,
    LocalSameBridge,
    RemoteBridge,
    Host,
}

impl Default for EndpointRoute {
    fn default() -> Self {
        Self::LocalSameBridge
    }
}

#[derive(Default)]
pub struct LineSynapseIndex {
    inner: DashMap<String, SynapseId>,
}

impl LineSynapseIndex {
    pub fn insert(&self, line: impl Into<String>, synapse_id: SynapseId) {
        self.inner.insert(line.into(), synapse_id);
    }

    pub fn resolve(&self, line: &str) -> Option<SynapseId> {
        self.inner.get(line).map(|entry| *entry)
    }

    pub fn lines(&self) -> Vec<String> {
        self.inner.iter().map(|entry| entry.key().clone()).collect()
    }
}
