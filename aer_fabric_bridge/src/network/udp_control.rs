use crate::config::NodeConfig;
use crate::discovery::beacon::{
    HelloMessage, TelemetrySnapshotProvider, run_control_receiver, run_hello_sender,
};
use crate::discovery::{NodeIdentity, PeerTable, SlotAllocator};
use crate::routing::{BridgeRouteTable, SynapseRange};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tracing::info;

pub async fn run_control_discovery(
    socket: Arc<UdpSocket>,
    cfg: &NodeConfig,
    identity: NodeIdentity,
    peer_table: Arc<PeerTable>,
    slot_allocator: Arc<SlotAllocator>,
    route_table: Arc<BridgeRouteTable>,
    owned_ranges: Vec<SynapseRange>,
    telemetry_snapshot: TelemetrySnapshotProvider,
    shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let local_ip: IpAddr = cfg
        .network
        .advertise_ip
        .as_deref()
        .unwrap_or(&cfg.network.bind_ip)
        .parse()?;
    let multicast_addr: SocketAddr = cfg.network.multicast_addr.parse()?;
    info!(
        node_uuid = %identity.node_uuid,
        node_name = %identity.node_name,
        cluster_name = %identity.cluster_name,
        advertise_ip = %local_ip,
        bind_ip = cfg.network.bind_ip,
        control_port = cfg.network.control_port,
        event_port = cfg.network.event_port,
        multicast_addr = %multicast_addr,
        "network discovery control plane initialised"
    );
    let hello = HelloMessage {
        node_uuid: identity.node_uuid,
        node_name: identity.node_name.clone(),
        cluster_name: identity.cluster_name.clone(),
        ip: local_ip.to_string(),
        control_port: cfg.network.control_port,
        event_port: cfg.network.event_port,
        fpaa_count: cfg.hardware.fpaa_count,
        chip_type: cfg.hardware.chip_type.clone(),
        hat_type: cfg.hardware.hat_type.clone(),
        boot_id: identity.boot_id,
        uptime_ms: identity.uptime_ms(),
    };

    let sender_socket = Arc::clone(&socket);
    let receiver_socket = Arc::clone(&socket);
    let sender_shutdown = shutdown.clone();
    let receiver_shutdown = shutdown.clone();
    let event_port = cfg.network.event_port;
    let sender = run_hello_sender(sender_socket, multicast_addr, hello, sender_shutdown);
    let receiver = run_control_receiver(
        receiver_socket,
        peer_table,
        slot_allocator,
        route_table,
        identity,
        event_port,
        owned_ranges,
        telemetry_snapshot,
        receiver_shutdown,
    );

    let _ = tokio::join!(sender, receiver);
    info!("network discovery control plane stopped");
    Ok(())
}
