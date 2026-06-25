use crate::aer::decode_packet;
use crate::metrics::Metrics;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tracing::warn;

pub async fn run_event_rx(
    socket: Arc<UdpSocket>,
    tx: mpsc::Sender<crate::aer::AerEvent>,
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            recv = socket.recv_from(&mut buf) => {
                let (size, _src) = match recv {
                    Ok(values) => values,
                    Err(err) => {
                        warn!("event rx socket error: {}", err);
                        continue;
                    }
                };
                let decoded = match decode_packet(&buf[..size]) {
                    Ok(packet) => packet,
                    Err(err) => {
                        warn!("invalid event packet: {}", err);
                        continue;
                    }
                };
                for event in decoded.events {
                    metrics.events_rx_udp.fetch_add(1, Ordering::Relaxed);
                    if tx.send(event).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
