use crate::aer::SynapseId;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynapseRange {
    pub start: SynapseId,
    pub end: SynapseId,
}

impl SynapseRange {
    pub fn contains(self, synapse_id: SynapseId) -> bool {
        synapse_id >= self.start && synapse_id <= self.end
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRoute {
    pub node_slot: u16,
    pub node_uuid: Uuid,
    pub ip: IpAddr,
    pub event_port: u16,
    pub synapse_ranges: Vec<SynapseRange>,
    pub last_seen_ns: u64,
    pub ready: bool,
}

impl BridgeRoute {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.event_port)
    }
}

#[derive(Default)]
pub struct BridgeRouteTable {
    routes: RwLock<HashMap<u16, BridgeRoute>>,
}

impl BridgeRouteTable {
    pub fn upsert(&self, route: BridgeRoute) {
        self.routes.write().insert(route.node_slot, route);
    }

    pub fn route_for_node_slot(&self, node_slot: u16) -> Option<BridgeRoute> {
        self.routes.read().get(&node_slot).cloned()
    }

    pub fn owner_for_synapse(&self, synapse_id: SynapseId) -> Option<BridgeRoute> {
        self.routes
            .read()
            .values()
            .find(|route| {
                route
                    .synapse_ranges
                    .iter()
                    .any(|range| range.contains(synapse_id))
            })
            .cloned()
    }

    pub fn routes(&self) -> Vec<BridgeRoute> {
        self.routes.read().values().cloned().collect()
    }
}
