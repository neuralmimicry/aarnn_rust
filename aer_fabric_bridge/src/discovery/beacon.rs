use crate::aer::SynapseId;
use crate::discovery::{NodeIdentity, PeerTable, PeerUpsertOutcome, SlotAllocator};
use crate::routing::{BridgeRoute, BridgeRouteTable, SynapseRange};
use crate::time::unix_time_ns;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub type TelemetrySnapshotProvider = Arc<dyn Fn() -> TelemetrySnapshotMessage + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ControlMessage {
    Hello(HelloMessage),
    Ready(ReadyMessage),
    ClusterState(ClusterStateMessage),
    TelemetryRequest(TelemetryRequestMessage),
    TelemetrySnapshot(TelemetrySnapshotMessage),
    Goodbye(GoodbyeMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMessage {
    pub node_uuid: Uuid,
    pub node_name: String,
    pub cluster_name: String,
    pub ip: String,
    pub control_port: u16,
    pub event_port: u16,
    pub fpaa_count: u8,
    pub chip_type: String,
    pub hat_type: String,
    pub boot_id: u64,
    pub uptime_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyMessage {
    pub node_uuid: Uuid,
    pub node_slot: u16,
    pub event_port: u16,
    pub owned_synapse_ranges: Vec<SynapseRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryRequestMessage {
    #[serde(default)]
    pub requester: String,
    #[serde(default)]
    pub request_time_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshotMessage {
    pub node_uuid: Uuid,
    pub node_name: String,
    pub cluster_name: String,
    pub node_slot: u16,
    pub ip: String,
    pub control_port: u16,
    pub event_port: u16,
    pub diagnostics_port: u16,
    pub fpaa_hardware_detected: bool,
    pub fpaa_force_software_fallback: bool,
    pub fpaa_runtime_state_path: Option<String>,
    pub fpaa_runtime_state_loaded: bool,
    pub synapse_config_loaded: bool,
    pub synapse_entry_count: usize,
    pub fpaa_mapping_loaded: bool,
    #[serde(default)]
    pub software_fallback_synapse_ids: Vec<String>,
    pub software_kernel_fallback_events: u64,
    pub gpio_emit_events: u64,
    pub events_rx_udp: u64,
    pub events_tx_udp: u64,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStateMessage {
    pub cluster_name: String,
    pub coordinator_uuid: Option<Uuid>,
    pub nodes: Vec<NodeSummary>,
    pub routes: Vec<BridgeRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSummary {
    pub node_uuid: Uuid,
    pub node_name: String,
    pub node_slot: Option<u16>,
    pub ip: String,
    pub event_port: u16,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoodbyeMessage {
    pub node_uuid: Uuid,
}

pub async fn run_hello_sender(
    socket: Arc<UdpSocket>,
    multicast_addr: SocketAddr,
    hello: HelloMessage,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(&ControlMessage::Hello(hello))?;
    debug!(target = %multicast_addr, "control hello sender started");
    loop {
        if *shutdown.borrow() {
            debug!("control hello sender stopping");
            return Ok(());
        }
        if let Err(err) = socket.send_to(&payload, multicast_addr).await {
            warn!("failed to send hello beacon: {}", err);
        }
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {}
        }
    }
}

pub async fn run_control_receiver(
    socket: Arc<UdpSocket>,
    peer_table: Arc<PeerTable>,
    slot_allocator: Arc<SlotAllocator>,
    bridge_routes: Arc<BridgeRouteTable>,
    local_identity: NodeIdentity,
    local_event_port: u16,
    local_ranges: Vec<SynapseRange>,
    telemetry_snapshot: TelemetrySnapshotProvider,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 16 * 1024];
    debug!("control receiver started");
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    debug!("control receiver stopping");
                    return Ok(());
                }
            }
            recv = socket.recv_from(&mut buf) => {
                let (size, src) = match recv {
                    Ok(values) => values,
                    Err(err) => {
                        warn!("control recv error: {}", err);
                        continue;
                    }
                };
                let parsed = serde_json::from_slice::<ControlMessage>(&buf[..size]);
                let msg = match parsed {
                    Ok(msg) => msg,
                    Err(_) => {
                        continue;
                    }
                };
                match msg {
                    ControlMessage::Hello(hello) => {
                        if hello.node_uuid == local_identity.node_uuid {
                            continue;
                        }
                        if hello.cluster_name != local_identity.cluster_name {
                            debug!(
                                peer_uuid = %hello.node_uuid,
                                peer_cluster = hello.cluster_name,
                                local_cluster = local_identity.cluster_name,
                                "ignoring hello from different cluster"
                            );
                            continue;
                        }
                        let ip = parse_ip(&hello.ip).unwrap_or(src.ip());
                        let upsert = peer_table.upsert_hello(
                            hello.node_uuid,
                            hello.node_name.clone(),
                            hello.cluster_name.clone(),
                            ip,
                            hello.control_port,
                            hello.event_port,
                            hello.fpaa_count,
                        );
                        let slot = slot_allocator.assign_slot(hello.node_uuid)?;
                        let ready_changed = peer_table.mark_ready(hello.node_uuid, slot);
                        bridge_routes.upsert(BridgeRoute {
                            node_slot: slot,
                            node_uuid: hello.node_uuid,
                            ip,
                            event_port: hello.event_port,
                            synapse_ranges: vec![],
                            last_seen_ns: unix_time_ns(),
                            ready: true,
                        });
                        if upsert == PeerUpsertOutcome::Inserted {
                            info!(
                                peer_uuid = %hello.node_uuid,
                                peer_name = hello.node_name,
                                peer_ip = %ip,
                                peer_event_port = hello.event_port,
                                peer_slot = slot,
                                peer_fpaa_count = hello.fpaa_count,
                                "network discovery: peer detected via hello"
                            );
                        } else if ready_changed {
                            info!(
                                peer_uuid = %hello.node_uuid,
                                peer_slot = slot,
                                "network discovery: peer readiness updated from hello"
                            );
                        }
                        let ready = ControlMessage::Ready(ReadyMessage {
                            node_uuid: local_identity.node_uuid,
                            node_slot: slot_allocator.assign_slot(local_identity.node_uuid)?,
                            event_port: local_event_port,
                            owned_synapse_ranges: local_ranges.clone(),
                        });
                        let ready_payload = serde_json::to_vec(&ready)?;
                        let target = SocketAddr::new(ip, hello.control_port);
                        if let Err(err) = socket.send_to(&ready_payload, target).await {
                            warn!(
                                peer_uuid = %hello.node_uuid,
                                target = %target,
                                error = %err,
                                "failed to send ready response"
                            );
                        }
                    }
                    ControlMessage::Ready(ready) => {
                        if ready.node_uuid == local_identity.node_uuid {
                            continue;
                        }
                        let _ = slot_allocator.assign_slot(ready.node_uuid)?;
                        let ready_changed = peer_table.mark_ready(ready.node_uuid, ready.node_slot);
                        if let Some(existing) = peer_table
                            .list()
                            .into_iter()
                            .find(|peer| peer.node_uuid == ready.node_uuid)
                        {
                            bridge_routes.upsert(BridgeRoute {
                                node_slot: ready.node_slot,
                                node_uuid: ready.node_uuid,
                                ip: existing.ip,
                                event_port: ready.event_port,
                                synapse_ranges: ready.owned_synapse_ranges.clone(),
                                last_seen_ns: unix_time_ns(),
                                ready: true,
                            });
                            if ready_changed {
                                info!(
                                    peer_uuid = %ready.node_uuid,
                                    peer_slot = ready.node_slot,
                                    peer_ip = %existing.ip,
                                    peer_event_port = ready.event_port,
                                    "network discovery: peer ready received and route updated"
                                );
                            }
                        }
                    }
                    ControlMessage::ClusterState(_state) => {}
                    ControlMessage::TelemetryRequest(request) => {
                        let snapshot = (telemetry_snapshot)();
                        info!(
                            requester = %request.requester,
                            requester_addr = %src,
                            requester_time_ns = request.request_time_ns,
                            node_slot = snapshot.node_slot,
                            fpaa_hardware_detected = snapshot.fpaa_hardware_detected,
                            fpaa_force_software_fallback = snapshot.fpaa_force_software_fallback,
                            software_kernel_fallback_events = snapshot.software_kernel_fallback_events,
                            events_rx_udp = snapshot.events_rx_udp,
                            events_tx_udp = snapshot.events_tx_udp,
                            "telemetry request served"
                        );
                        let payload =
                            serde_json::to_vec(&ControlMessage::TelemetrySnapshot(snapshot))?;
                        if let Err(err) = socket.send_to(&payload, src).await {
                            warn!(
                                target = %src,
                                error = %err,
                                "failed to send telemetry snapshot"
                            );
                        }
                    }
                    ControlMessage::TelemetrySnapshot(_snapshot) => {}
                    ControlMessage::Goodbye(goodbye) => {
                        debug!("peer goodbye received for {}", goodbye.node_uuid);
                    }
                }
            }
        }
    }
}

fn parse_ip(raw: &str) -> Option<IpAddr> {
    raw.parse::<IpAddr>().ok()
}

#[allow(dead_code)]
fn full_synapse_space() -> Vec<SynapseRange> {
    vec![SynapseRange {
        start: SynapseId(0x5000_0000_0000_0000),
        end: SynapseId(0x5fff_ffff_ffff_ffff),
    }]
}
