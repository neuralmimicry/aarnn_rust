use crate::aer::{AerEvent, AerFlags};
use crate::config::{ConfigBundle, FpaaMappingConfig, NodeHardwareConfig};
use crate::discovery::{
    NodeIdentity, PeerTable, SlotAllocator, TelemetrySnapshotMessage, TelemetrySnapshotProvider,
};
use crate::hardware::gpio::GpioBackend;
#[cfg(feature = "linux-gpio")]
use crate::hardware::gpio_linux::LinuxGpioBackend;
use crate::hardware::gpio_mock::MockGpioBackend;
use crate::hardware::pika::{PikaHat, StimulateResult};
use crate::hardware::spi::SpiBackend;
#[cfg(feature = "linux-spi")]
use crate::hardware::spi_linux::{LinuxSpiBackend, LinuxSpiConfig};
use crate::hardware::spi_mock::MockSpiBackend;
use crate::metrics::Metrics;
use crate::network::sockets::{bind_control_socket, bind_event_rx_socket, bind_event_tx_socket};
use crate::network::udp_control::run_control_discovery;
use crate::network::udp_event_rx::run_event_rx;
use crate::network::udp_event_tx::{OutboundDatagram, run_event_tx};
use crate::routing::{BridgeRouteTable, HostSubscriptionTable, RouteAction, Router};
use crate::runtime::queues::QueueSet;
use crate::runtime::shutdown;
use crate::time::unix_time_ns;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

pub async fn run_bridge(bundle: ConfigBundle) -> anyhow::Result<()> {
    let metrics = Arc::new(Metrics::default());
    let peer_table = Arc::new(PeerTable::default());
    let route_table = Arc::new(BridgeRouteTable::default());
    let host_subscriptions = Arc::new(HostSubscriptionTable::default());
    for addr in &bundle.cluster.host_subscribers {
        if let Ok(addr) = addr.parse::<SocketAddr>() {
            host_subscriptions.add(addr);
        }
    }

    let slot_allocator = Arc::new(SlotAllocator::load(&bundle.slot_registry_path)?);
    let local_slot = slot_allocator.assign_slot(bundle.node.node_uuid)?;
    let node_identity = NodeIdentity::from_config(&bundle.node);
    let synapse_entry_count = bundle.synapses.all().count();
    let synapse_config_loaded = bundle
        .node_config_path
        .parent()
        .map(|dir| dir.join("synapses.toml").exists())
        .unwrap_or(false);
    let fpaa_mapping_loaded = bundle
        .node_config_path
        .parent()
        .map(|dir| dir.join("fpaa_mapping.toml").exists())
        .unwrap_or(false);
    let fpaa_runtime_state_path = detect_fpaa_runtime_state_path();
    let fpaa_runtime_state_loaded = fpaa_runtime_state_path.is_some();
    let software_fallback_synapse_ids =
        collect_software_fallback_synapse_ids(&bundle.synapses, local_slot);
    let line_index = Arc::new(bundle.synapses.build_capture_line_index(local_slot));
    let capture_lines = line_index.lines();

    let control_socket = Arc::new(bind_control_socket(&bundle.node.network)?);
    let event_rx_socket = Arc::new(bind_event_rx_socket(&bundle.node.network)?);
    let event_tx_socket = Arc::new(bind_event_tx_socket(&bundle.node.network)?);

    let gpio = build_gpio_backend(&bundle.node.hardware, &bundle.fpaa_mapping, &capture_lines)?;
    let spi = build_spi_backend(&bundle.node.hardware)?;
    let pika = Arc::new(PikaHat::new(
        bundle.node.hardware.fpaa_count,
        Arc::clone(&gpio),
        spi,
        bundle.node.hardware.force_software_fallback,
    ));
    let fpaa_available = pika.detect_fpaa().await;
    if fpaa_available {
        info!("FPAA detection complete: hardware route active");
    } else {
        warn!("FPAA not detected: local actions will run software kernels");
    }

    let router = Arc::new(Router {
        local_node_slot: local_slot,
        synapse_table: Arc::new(bundle.synapses),
        bridge_routes: Arc::clone(&route_table),
        host_subscriptions: Arc::clone(&host_subscriptions),
        metrics: Arc::clone(&metrics),
    });

    let queues = QueueSet::new(4_096);
    let event_ingress_tx = queues.event_ingress_tx.clone();
    let route_action_tx = queues.route_action_tx.clone();
    let local_stimulus_tx = queues.local_stimulus_tx.clone();
    let outbound_tx = queues.outbound_tx.clone();

    let (shutdown_tx, shutdown_rx) = shutdown::channel();
    spawn_shutdown_signal_listener(shutdown_tx.clone());

    let control_shutdown = shutdown_rx.clone();
    let owned_ranges = bundle.cluster.owned_synapse_ranges.clone();
    let control_cfg = bundle.node.clone();
    let control_peer_table = Arc::clone(&peer_table);
    let control_slot_allocator = Arc::clone(&slot_allocator);
    let control_route_table = Arc::clone(&route_table);
    let telemetry_snapshot: TelemetrySnapshotProvider = {
        let telemetry_metrics = Arc::clone(&metrics);
        let telemetry_pika = Arc::clone(&pika);
        let node_uuid = bundle.node.node_uuid;
        let node_name = bundle.node.node_name.clone();
        let cluster_name = bundle.node.cluster_name.clone();
        let advertise_ip = bundle
            .node
            .network
            .advertise_ip
            .clone()
            .unwrap_or_else(|| bundle.node.network.bind_ip.clone());
        let control_port = bundle.node.network.control_port;
        let event_port = bundle.node.network.event_port;
        let diagnostics_port = bundle.node.network.diagnostics_port;
        let fpaa_force_software_fallback = bundle.node.hardware.force_software_fallback;
        let fpaa_runtime_state_path = fpaa_runtime_state_path.clone();
        let software_fallback_synapse_ids = software_fallback_synapse_ids.clone();
        Arc::new(move || TelemetrySnapshotMessage {
            node_uuid,
            node_name: node_name.clone(),
            cluster_name: cluster_name.clone(),
            node_slot: local_slot,
            ip: advertise_ip.clone(),
            control_port,
            event_port,
            diagnostics_port,
            fpaa_hardware_detected: telemetry_pika.fpaa_available(),
            fpaa_force_software_fallback,
            fpaa_runtime_state_path: fpaa_runtime_state_path.clone(),
            fpaa_runtime_state_loaded,
            synapse_config_loaded,
            synapse_entry_count,
            fpaa_mapping_loaded,
            software_fallback_synapse_ids: software_fallback_synapse_ids.clone(),
            software_kernel_fallback_events: telemetry_metrics
                .software_kernel_fallback_events
                .load(Ordering::Relaxed),
            gpio_emit_events: telemetry_metrics.gpio_emit_events.load(Ordering::Relaxed),
            events_rx_udp: telemetry_metrics.events_rx_udp.load(Ordering::Relaxed),
            events_tx_udp: telemetry_metrics.events_tx_udp.load(Ordering::Relaxed),
            timestamp_ns: unix_time_ns(),
        })
    };
    tokio::spawn(async move {
        if let Err(err) = run_control_discovery(
            control_socket,
            &control_cfg,
            node_identity,
            control_peer_table,
            control_slot_allocator,
            control_route_table,
            owned_ranges,
            telemetry_snapshot,
            control_shutdown,
        )
        .await
        {
            warn!("control discovery task stopped: {}", err);
        }
    });

    let rx_shutdown = shutdown_rx.clone();
    let rx_metrics = Arc::clone(&metrics);
    tokio::spawn(async move {
        if let Err(err) =
            run_event_rx(event_rx_socket, event_ingress_tx, rx_metrics, rx_shutdown).await
        {
            warn!("event rx task stopped: {}", err);
        }
    });

    let tx_shutdown = shutdown_rx.clone();
    let tx_metrics = Arc::clone(&metrics);
    tokio::spawn(async move {
        if let Err(err) = run_event_tx(
            event_tx_socket,
            local_slot,
            queues.outbound_rx,
            tx_metrics,
            tx_shutdown,
        )
        .await
        {
            warn!("event tx task stopped: {}", err);
        }
    });

    let router_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        if let Err(err) = run_router_task(
            router,
            queues.event_ingress_rx,
            route_action_tx,
            router_shutdown,
        )
        .await
        {
            warn!("router task stopped: {}", err);
        }
    });

    let dispatch_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        if let Err(err) = run_action_dispatch_task(
            queues.route_action_rx,
            local_stimulus_tx,
            outbound_tx,
            dispatch_shutdown,
        )
        .await
        {
            warn!("route action dispatch task stopped: {}", err);
        }
    });

    let output_shutdown = shutdown_rx.clone();
    let output_metrics = Arc::clone(&metrics);
    let output_pika = Arc::clone(&pika);
    tokio::spawn(async move {
        if let Err(err) = run_hardware_output_task(
            output_pika,
            queues.local_stimulus_rx,
            output_metrics,
            output_shutdown,
        )
        .await
        {
            warn!("hardware output task stopped: {}", err);
        }
    });

    let capture_shutdown = shutdown_rx.clone();
    let capture_metrics = Arc::clone(&metrics);
    let capture_tx = queues.event_ingress_tx.clone();
    let capture_line_index = Arc::clone(&line_index);
    tokio::spawn(async move {
        if let Err(err) = run_hardware_capture_task(
            gpio,
            capture_line_index,
            capture_tx,
            local_slot,
            capture_metrics,
            capture_shutdown,
        )
        .await
        {
            warn!("hardware capture task stopped: {}", err);
        }
    });

    let metrics_shutdown = shutdown_rx.clone();
    let metrics_clone = Arc::clone(&metrics);
    tokio::spawn(async move {
        if let Err(err) = run_metrics_task(metrics_clone, metrics_shutdown).await {
            warn!("metrics task stopped: {}", err);
        }
    });

    info!(
        node_name = bundle.node.node_name,
        node_slot = local_slot,
        event_port = bundle.node.network.event_port,
        pid = std::process::id(),
        "service lifecycle: aer_fabric_bridge started"
    );
    wait_for_shutdown(shutdown_rx).await;
    let _ = shutdown_tx.send(true);
    info!(pid = std::process::id(), "service lifecycle: aer_fabric_bridge stopped");
    Ok(())
}

fn detect_fpaa_runtime_state_path() -> Option<String> {
    if let Ok(explicit) = std::env::var("AER_BRIDGE_FPAA_RUNTIME_STATE") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() && Path::new(trimmed).exists() {
            return Some(trimmed.to_string());
        }
    }

    let candidates = [
        "/etc/aer-bridge/fpaa/runtime_state.json",
        "/etc/aer-bridge/fpaa_runtime_state.json",
        "/opt/continuum/src/aarnn_rust/fpaa/runtime_state.json",
        "./fpaa/runtime_state.json",
        "fpaa/runtime_state.json",
    ];
    for path in candidates {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

fn spawn_shutdown_signal_listener(shutdown_tx: watch::Sender<bool>) {
    #[cfg(unix)]
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sig_int = match signal(SignalKind::interrupt()) {
            Ok(stream) => stream,
            Err(err) => {
                warn!("failed to install SIGINT handler: {}", err);
                return;
            }
        };
        let mut sig_term = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(err) => {
                warn!("failed to install SIGTERM handler: {}", err);
                return;
            }
        };

        tokio::select! {
            _ = sig_int.recv() => {
                info!("received SIGINT; shutting down aer_fabric_bridge");
            }
            _ = sig_term.recv() => {
                info!("received SIGTERM; shutting down aer_fabric_bridge (service stop/restart)");
            }
        }

        let _ = shutdown_tx.send(true);
    });

    #[cfg(not(unix))]
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        info!("received shutdown signal; shutting down aer_fabric_bridge");
        let _ = shutdown_tx.send(true);
    });
}

async fn run_router_task(
    router: Arc<Router>,
    mut event_rx: mpsc::Receiver<AerEvent>,
    route_action_tx: mpsc::Sender<RouteAction>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            maybe_event = event_rx.recv() => {
                let Some(event) = maybe_event else {
                    return Ok(());
                };
                debug!(synapse_id = %event.synapse_id, "received AER event");
                let actions = router.route_event(event).await?;
                for action in actions {
                    if route_action_tx.send(action).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn run_action_dispatch_task(
    mut route_action_rx: mpsc::Receiver<RouteAction>,
    local_stimulus_tx: mpsc::Sender<RouteAction>,
    outbound_tx: mpsc::Sender<OutboundDatagram>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            maybe_action = route_action_rx.recv() => {
                let Some(action) = maybe_action else {
                    return Ok(());
                };
                match action {
                    RouteAction::LocalStimulus { .. } => {
                        if local_stimulus_tx.send(action).await.is_err() {
                            return Ok(());
                        }
                    }
                    RouteAction::RemoteUdp { target_addr, event, .. } => {
                        if outbound_tx.send(OutboundDatagram { target: target_addr, event }).await.is_err() {
                            return Ok(());
                        }
                    }
                    RouteAction::HostMirror { target_addr, event } => {
                        if outbound_tx.send(OutboundDatagram { target: target_addr, event }).await.is_err() {
                            return Ok(());
                        }
                    }
                    RouteAction::Drop { reason, event } => {
                        debug!(reason = reason, synapse_id = %event.synapse_id, "dropped event");
                    }
                }
            }
        }
    }
}

async fn run_hardware_output_task(
    pika: Arc<PikaHat>,
    mut local_stimulus_rx: mpsc::Receiver<RouteAction>,
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut software_fallback_announced = false;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            maybe_action = local_stimulus_rx.recv() => {
                let Some(action) = maybe_action else {
                    return Ok(());
                };
                let RouteAction::LocalStimulus { synapse_id, endpoint, pulse_width_ns, value } = action else {
                    continue;
                };
                let event = AerEvent {
                    synapse_id,
                    flags: AerFlags::empty(),
                    value,
                    event_time_ns: unix_time_ns(),
                    pulse_width_ns,
                    ttl: 1,
                    source_node_slot: endpoint.node_slot,
                    sequence: 0,
                };
                match pika.stimulate_endpoint(&endpoint, &event).await? {
                    StimulateResult::HardwarePulse { line, pulse_width_ns, value } => {
                        debug!(
                            line = line,
                            pulse_width_ns = pulse_width_ns,
                            value = value,
                            "local stimulus dispatched to hardware pulse output"
                        );
                        metrics.gpio_emit_events.fetch_add(1, Ordering::Relaxed);
                    }
                    StimulateResult::SoftwareKernel {
                        kernel,
                        output_value,
                        delay_ns,
                        reason,
                    } => {
                        debug!(
                            kernel = ?kernel,
                            output_value = output_value,
                            delay_ns = delay_ns,
                            reason = reason,
                            "local stimulus fulfilled by software kernel fallback"
                        );
                        let fallback_total = metrics
                            .software_kernel_fallback_events
                            .fetch_add(1, Ordering::Relaxed)
                            .saturating_add(1);
                        if !software_fallback_announced {
                            info!(
                                kernel = ?kernel,
                                reason = reason,
                                software_kernel_fallback_events = fallback_total,
                                "software-kernel fallback path active on bridge node"
                            );
                            software_fallback_announced = true;
                        }
                    }
                }
            }
        }
    }
}

async fn run_hardware_capture_task(
    gpio: Arc<dyn GpioBackend>,
    line_index: Arc<crate::routing::LineSynapseIndex>,
    event_ingress_tx: mpsc::Sender<AerEvent>,
    local_node_slot: u16,
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(5)) => {
                let edges = gpio.read_edges().await?;
                for edge in edges {
                    let Some(synapse_id) = line_index.resolve(&edge.line) else {
                        continue;
                    };
                    metrics.gpio_capture_events.fetch_add(1, Ordering::Relaxed);
                    let event = AerEvent {
                        synapse_id,
                        flags: AerFlags::CAPTURED,
                        value: if edge.rising { 1 } else { 0 },
                        event_time_ns: edge.timestamp_ns,
                        pulse_width_ns: 5_000,
                        ttl: 8,
                        source_node_slot: local_node_slot,
                        sequence: 0,
                    };
                    if event_ingress_tx.send(event).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn run_metrics_task(
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let interval_s = 5.0_f64;
    let mut last_events_rx_udp = 0_u64;
    let mut last_events_tx_udp = 0_u64;
    let mut last_local_routes = 0_u64;
    let mut last_remote_routes = 0_u64;
    let mut last_host_mirrors = 0_u64;
    let mut last_fallbacks = 0_u64;

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {
                let events_rx_udp = metrics.events_rx_udp.load(Ordering::Relaxed);
                let events_tx_udp = metrics.events_tx_udp.load(Ordering::Relaxed);
                let local_routes =
                    metrics.local_same_fpaa_routes.load(Ordering::Relaxed)
                    + metrics.local_same_pika_routes.load(Ordering::Relaxed)
                    + metrics.local_same_bridge_routes.load(Ordering::Relaxed);
                let remote_routes = metrics.remote_udp_forwards.load(Ordering::Relaxed);
                let host_mirrors = metrics.host_mirror_events.load(Ordering::Relaxed);
                let fallback_events = metrics.software_kernel_fallback_events.load(Ordering::Relaxed);

                let delta_events_rx_udp = events_rx_udp.saturating_sub(last_events_rx_udp);
                let delta_events_tx_udp = events_tx_udp.saturating_sub(last_events_tx_udp);
                let delta_local_routes = local_routes.saturating_sub(last_local_routes);
                let delta_remote_routes = remote_routes.saturating_sub(last_remote_routes);
                let delta_host_mirrors = host_mirrors.saturating_sub(last_host_mirrors);
                let delta_fallback_events = fallback_events.saturating_sub(last_fallbacks);

                last_events_rx_udp = events_rx_udp;
                last_events_tx_udp = events_tx_udp;
                last_local_routes = local_routes;
                last_remote_routes = remote_routes;
                last_host_mirrors = host_mirrors;
                last_fallbacks = fallback_events;

                info!(
                    aer_rx_events_s = (delta_events_rx_udp as f64 / interval_s),
                    aer_tx_events_s = (delta_events_tx_udp as f64 / interval_s),
                    aer_local_routes_s = (delta_local_routes as f64 / interval_s),
                    aer_remote_routes_s = (delta_remote_routes as f64 / interval_s),
                    aer_host_mirrors_s = (delta_host_mirrors as f64 / interval_s),
                    aer_software_fallback_s = (delta_fallback_events as f64 / interval_s),
                    "AER utilisation window"
                );
                metrics.log_snapshot();
            }
        }
    }
}

fn build_gpio_backend(
    hardware: &NodeHardwareConfig,
    fpaa_mapping: &FpaaMappingConfig,
    capture_lines: &[String],
) -> anyhow::Result<Arc<dyn GpioBackend>> {
    #[cfg(not(feature = "linux-gpio"))]
    let _ = (fpaa_mapping, capture_lines);

    match hardware.gpio_backend.trim().to_ascii_lowercase().as_str() {
        "mock" => Ok(Arc::new(MockGpioBackend::default())),
        "linux" => {
            #[cfg(feature = "linux-gpio")]
            {
                Ok(Arc::new(LinuxGpioBackend::new(
                    hardware.gpio_chip.clone(),
                    hardware.gpio_consumer.clone(),
                    fpaa_mapping.gpio_alias_map(),
                    capture_lines.to_vec(),
                )?))
            }
            #[cfg(not(feature = "linux-gpio"))]
            {
                anyhow::bail!("linux gpio backend requested without linux-gpio feature")
            }
        }
        other => anyhow::bail!("unsupported gpio backend '{}'", other),
    }
}

fn collect_software_fallback_synapse_ids(
    table: &crate::routing::LocalSynapseTable,
    local_slot: u16,
) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for (synapse_id, entry) in table.all() {
        let has_local_consumer = entry.consumers.iter().any(|consumer| {
            consumer.node_slot == local_slot
                && matches!(
                    consumer.route,
                    crate::routing::EndpointRoute::LocalSameFpaa
                        | crate::routing::EndpointRoute::LocalSamePika
                        | crate::routing::EndpointRoute::LocalSameBridge
                )
        });
        if has_local_consumer {
            ids.insert(synapse_id.to_string());
        }
    }
    ids.into_iter().collect()
}

fn build_spi_backend(hardware: &NodeHardwareConfig) -> anyhow::Result<Arc<dyn SpiBackend>> {
    match hardware.spi_backend.trim().to_ascii_lowercase().as_str() {
        "mock" => Ok(Arc::new(MockSpiBackend::default())),
        "linux" => {
            #[cfg(feature = "linux-spi")]
            {
                let probe_bytes = parse_probe_hex_bytes(&hardware.fpaa_probe_hex)?;
                Ok(Arc::new(LinuxSpiBackend::new(LinuxSpiConfig {
                    device: hardware.spi_device.clone(),
                    speed_hz: hardware.spi_speed_hz,
                    bits_per_word: hardware.spi_bits_per_word,
                    mode: hardware.spi_mode,
                    lsb_first: false,
                    probe_bytes,
                })))
            }
            #[cfg(not(feature = "linux-spi"))]
            {
                anyhow::bail!("linux spi backend requested without linux-spi feature")
            }
        }
        other => anyhow::bail!("unsupported spi backend '{}'", other),
    }
}

#[cfg(feature = "linux-spi")]
fn parse_probe_hex_bytes(raw: &str) -> anyhow::Result<Vec<u8>> {
    let compact: String = raw
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace() && *ch != ':' && *ch != '-')
        .collect();
    if compact.is_empty() {
        return Ok(vec![0x00]);
    }
    if !compact.len().is_multiple_of(2) {
        anyhow::bail!("fpaa_probe_hex must contain an even number of hex digits");
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    for idx in (0..compact.len()).step_by(2) {
        let byte = u8::from_str_radix(&compact[idx..idx + 2], 16).map_err(|err| {
            anyhow::anyhow!(
                "invalid fpaa_probe_hex byte '{}': {}",
                &compact[idx..idx + 2],
                err
            )
        })?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{run_action_dispatch_task, run_hardware_output_task, run_router_task};
    use crate::aer::{AerEvent, AerFlags, SynapseId};
    use crate::hardware::gpio_mock::MockGpioBackend;
    use crate::hardware::pika::PikaHat;
    use crate::hardware::software_kernel::SoftwareKernel;
    use crate::hardware::spi_mock::MockSpiBackend;
    use crate::metrics::Metrics;
    use crate::routing::Router;
    use crate::routing::bridge_route_table::BridgeRouteTable;
    use crate::routing::endpoint_table::{EndpointRoute, EndpointType};
    use crate::routing::host_subscription_table::HostSubscriptionTable;
    use crate::routing::synapse_table::{LocalSynapseTable, SynapseEndpoint, SynapseEntry};
    use crate::runtime::shutdown;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc;
    use tokio::time::{Duration, sleep, timeout};

    #[tokio::test]
    async fn bridge_pipeline_uses_software_kernel_fallback_when_fpaa_unavailable() {
        let metrics = Arc::new(Metrics::default());
        let synapse_id = SynapseId(0x5001_0002_0000_4321);
        let synapses = Arc::new(LocalSynapseTable::from_entries(vec![SynapseEntry {
            synapse_id,
            description: Some("integration fallback test".to_string()),
            weight: 1.0,
            delay_ns: 5_000,
            mirror_to_host: false,
            producers: vec![],
            consumers: vec![SynapseEndpoint {
                endpoint_id: None,
                endpoint_type: EndpointType::DendriteBouton,
                location: None,
                node_slot: 0,
                fpaa_index: Some(0),
                neuron_id: Some(17),
                bouton_id: Some(3),
                io_name: None,
                route: EndpointRoute::LocalSameFpaa,
                weight: Some(1.0),
                delay_ns: None,
                pulse_width_ns: Some(5_000),
                gpio_line: Some("FPAA0_IO5P".to_string()),
                gpio_mask: None,
                software_kernel: Some(SoftwareKernel::ShortTermPlasticity),
            }],
        }]));

        let router = Arc::new(Router {
            local_node_slot: 0,
            synapse_table: synapses,
            bridge_routes: Arc::new(BridgeRouteTable::default()),
            host_subscriptions: Arc::new(HostSubscriptionTable::default()),
            metrics: Arc::clone(&metrics),
        });

        let gpio = Arc::new(MockGpioBackend::default());
        let spi = Arc::new(MockSpiBackend::with_probe_ok(false));
        let pika = Arc::new(PikaHat::new(4, gpio.clone(), spi, false));
        assert!(!pika.detect_fpaa().await, "test expects unavailable FPAA");

        let (shutdown_tx, shutdown_rx) = shutdown::channel();
        let (event_ingress_tx, event_ingress_rx) = mpsc::channel(32);
        let (route_action_tx, route_action_rx) = mpsc::channel(32);
        let (local_stimulus_tx, local_stimulus_rx) = mpsc::channel(32);
        let (outbound_tx, mut outbound_rx) = mpsc::channel(32);

        let router_handle = tokio::spawn(run_router_task(
            Arc::clone(&router),
            event_ingress_rx,
            route_action_tx,
            shutdown_rx.clone(),
        ));
        let dispatch_handle = tokio::spawn(run_action_dispatch_task(
            route_action_rx,
            local_stimulus_tx,
            outbound_tx,
            shutdown_rx.clone(),
        ));
        let output_handle = tokio::spawn(run_hardware_output_task(
            pika,
            local_stimulus_rx,
            Arc::clone(&metrics),
            shutdown_rx,
        ));

        event_ingress_tx
            .send(AerEvent {
                synapse_id,
                flags: AerFlags::empty(),
                value: 1,
                event_time_ns: 1,
                pulse_width_ns: 5_000,
                ttl: 8,
                source_node_slot: 0,
                sequence: 1,
            })
            .await
            .expect("event ingress channel should be open");

        timeout(Duration::from_secs(2), async {
            loop {
                if metrics
                    .software_kernel_fallback_events
                    .load(Ordering::Relaxed)
                    >= 1
                {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("software fallback metric did not increment in time");

        assert_eq!(metrics.local_synapse_hits.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.gpio_emit_events.load(Ordering::Relaxed), 0);
        assert_eq!(gpio.pulses().len(), 0, "no GPIO pulse expected in fallback");
        assert!(
            timeout(Duration::from_millis(100), outbound_rx.recv())
                .await
                .is_err(),
            "no UDP outbound datagram expected for local route"
        );

        let _ = shutdown_tx.send(true);
        drop(event_ingress_tx);
        let _ = router_handle.await;
        let _ = dispatch_handle.await;
        let _ = output_handle.await;
    }
}
