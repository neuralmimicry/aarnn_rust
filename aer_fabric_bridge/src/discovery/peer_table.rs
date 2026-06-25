use crate::time::unix_time_ns;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_uuid: Uuid,
    pub node_name: String,
    pub cluster_name: String,
    pub ip: IpAddr,
    pub control_port: u16,
    pub event_port: u16,
    pub fpaa_count: u8,
    pub last_seen_ns: u64,
    pub node_slot: Option<u16>,
    pub ready: bool,
}

#[derive(Default)]
pub struct PeerTable {
    peers: DashMap<Uuid, PeerInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerUpsertOutcome {
    Inserted,
    Refreshed,
}

impl PeerTable {
    pub fn upsert_hello(
        &self,
        node_uuid: Uuid,
        node_name: String,
        cluster_name: String,
        ip: IpAddr,
        control_port: u16,
        event_port: u16,
        fpaa_count: u8,
    ) -> PeerUpsertOutcome {
        let last_seen_ns = unix_time_ns();
        match self.peers.entry(node_uuid) {
            Entry::Occupied(mut occupied) => {
                let entry = occupied.get_mut();
                entry.node_name = node_name.clone();
                entry.cluster_name = cluster_name.clone();
                entry.ip = ip;
                entry.control_port = control_port;
                entry.event_port = event_port;
                entry.fpaa_count = fpaa_count;
                entry.last_seen_ns = last_seen_ns;
                PeerUpsertOutcome::Refreshed
            }
            Entry::Vacant(vacant) => {
                vacant.insert(PeerInfo {
                    node_uuid,
                    node_name,
                    cluster_name,
                    ip,
                    control_port,
                    event_port,
                    fpaa_count,
                    last_seen_ns,
                    node_slot: None,
                    ready: false,
                });
                PeerUpsertOutcome::Inserted
            }
        }
    }

    pub fn mark_ready(&self, node_uuid: Uuid, slot: u16) -> bool {
        if let Some(mut peer) = self.peers.get_mut(&node_uuid) {
            let changed = peer.node_slot != Some(slot) || !peer.ready;
            peer.node_slot = Some(slot);
            peer.ready = true;
            peer.last_seen_ns = unix_time_ns();
            return changed;
        }
        false
    }

    pub fn list(&self) -> Vec<PeerInfo> {
        self.peers.iter().map(|entry| entry.clone()).collect()
    }
}
