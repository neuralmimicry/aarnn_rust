use crate::aer::{AerEvent, encode_packet};
use crate::metrics::Metrics;
use crate::time::unix_time_ns;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tracing::warn;

#[derive(Debug, Clone)]
pub struct OutboundDatagram {
    pub target: SocketAddr,
    pub event: AerEvent,
}

pub async fn run_event_tx(
    socket: Arc<UdpSocket>,
    local_node_slot: u16,
    mut rx: mpsc::Receiver<OutboundDatagram>,
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let sequence = AtomicU32::new(1);
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            maybe_datagram = rx.recv() => {
                let Some(datagram) = maybe_datagram else {
                    return Ok(());
                };
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                let payload = match encode_packet(local_node_slot, seq, unix_time_ns(), &[datagram.event]) {
                    Ok(payload) => payload,
                    Err(err) => {
                        warn!("failed to encode outbound event: {}", err);
                        continue;
                    }
                };
                if let Err(err) = socket.send_to(&payload, datagram.target).await {
                    warn!("failed to send event datagram to {}: {}", datagram.target, err);
                    continue;
                }
                metrics.events_tx_udp.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
