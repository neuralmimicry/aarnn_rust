use crate::aer::SynapseId;
use crate::hardware::software_kernel::SoftwareKernel;
use crate::routing::endpoint_table::{
    EndpointId, EndpointLocation, EndpointRoute, EndpointType, LineSynapseIndex,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynapseEntry {
    pub synapse_id: SynapseId,
    pub description: Option<String>,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default)]
    pub delay_ns: u32,
    #[serde(default)]
    pub mirror_to_host: bool,
    #[serde(default)]
    pub producers: Vec<SynapseEndpoint>,
    #[serde(default)]
    pub consumers: Vec<SynapseEndpoint>,
}

fn default_weight() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynapseEndpoint {
    #[serde(default)]
    pub endpoint_id: Option<EndpointId>,
    #[serde(rename = "type")]
    pub endpoint_type: EndpointType,
    #[serde(default)]
    pub location: Option<EndpointLocation>,
    pub node_slot: u16,
    #[serde(default)]
    pub fpaa_index: Option<u8>,
    #[serde(default)]
    pub neuron_id: Option<u32>,
    #[serde(default)]
    pub bouton_id: Option<u32>,
    #[serde(default)]
    pub io_name: Option<String>,
    #[serde(default)]
    pub route: EndpointRoute,
    #[serde(default)]
    pub weight: Option<f32>,
    #[serde(default)]
    pub delay_ns: Option<u32>,
    #[serde(default)]
    pub pulse_width_ns: Option<u32>,
    #[serde(default)]
    pub gpio_line: Option<String>,
    #[serde(default)]
    pub gpio_mask: Option<u32>,
    #[serde(default, alias = "kernel")]
    pub software_kernel: Option<SoftwareKernel>,
}

impl SynapseEndpoint {
    pub fn endpoint_id(&self) -> EndpointId {
        self.endpoint_id.clone().unwrap_or(EndpointId {
            node_slot: self.node_slot,
            fpaa_index: self.fpaa_index,
            neuron_id: self.neuron_id,
            endpoint_type: self.endpoint_type,
            bouton_id: self.bouton_id,
            io_name: self.io_name.clone(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct SynapsesConfig {
    #[serde(default)]
    synapses: Vec<SynapseEntry>,
}

#[derive(Debug, Default)]
pub struct LocalSynapseTable {
    entries: HashMap<SynapseId, SynapseEntry>,
}

impl LocalSynapseTable {
    pub fn from_entries(entries: Vec<SynapseEntry>) -> Self {
        let mut map = HashMap::new();
        for entry in entries {
            map.insert(entry.synapse_id, entry);
        }
        Self { entries: map }
    }

    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read '{}'", path.display()))?;
        let parsed: SynapsesConfig = toml::from_str(&text)
            .with_context(|| format!("failed to parse synapses file '{}'", path.display()))?;
        Ok(Self::from_entries(parsed.synapses))
    }

    pub fn lookup(&self, synapse_id: SynapseId) -> Option<&SynapseEntry> {
        self.entries.get(&synapse_id)
    }

    pub fn all(&self) -> impl Iterator<Item = (&SynapseId, &SynapseEntry)> {
        self.entries.iter()
    }

    pub fn build_capture_line_index(&self, local_node_slot: u16) -> LineSynapseIndex {
        let index = LineSynapseIndex::default();
        for (synapse_id, entry) in &self.entries {
            for producer in &entry.producers {
                if producer.node_slot != local_node_slot {
                    continue;
                }
                if let Some(line) = producer.gpio_line.as_deref() {
                    index.insert(line.to_string(), *synapse_id);
                }
            }
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalSynapseTable, SynapseEndpoint, SynapseEntry};
    use crate::aer::SynapseId;
    use crate::routing::endpoint_table::{EndpointRoute, EndpointType};

    #[test]
    fn local_synapse_lookup_works() {
        let table = LocalSynapseTable::from_entries(vec![SynapseEntry {
            synapse_id: SynapseId(0x5001_0002_0000_1234),
            description: None,
            weight: 0.9,
            delay_ns: 0,
            mirror_to_host: false,
            producers: vec![],
            consumers: vec![SynapseEndpoint {
                endpoint_id: None,
                endpoint_type: EndpointType::DendriteBouton,
                location: None,
                node_slot: 0,
                fpaa_index: Some(0),
                neuron_id: Some(22),
                bouton_id: Some(1),
                io_name: None,
                route: EndpointRoute::LocalSameFpaa,
                weight: None,
                delay_ns: None,
                pulse_width_ns: Some(5_000),
                gpio_line: Some("FPAA0_IO5P".to_string()),
                gpio_mask: None,
                software_kernel: None,
            }],
        }]);

        let entry = table.lookup(SynapseId(0x5001_0002_0000_1234));
        assert!(entry.is_some());
    }
}
