use crate::config::NodeConfig;
use crate::time::{unix_time_ms, unix_time_ns};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub node_uuid: Uuid,
    pub node_name: String,
    pub cluster_name: String,
    pub boot_id: u64,
    pub boot_unix_ms: u64,
}

impl NodeIdentity {
    pub fn from_config(cfg: &NodeConfig) -> Self {
        Self {
            node_uuid: cfg.node_uuid,
            node_name: cfg.node_name.clone(),
            cluster_name: cfg.cluster_name.clone(),
            boot_id: unix_time_ns(),
            boot_unix_ms: unix_time_ms(),
        }
    }

    pub fn uptime_ms(&self) -> u64 {
        unix_time_ms().saturating_sub(self.boot_unix_ms)
    }
}
