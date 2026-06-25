use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

#[derive(Debug, Default)]
pub struct Metrics {
    pub events_rx_udp: AtomicU64,
    pub events_tx_udp: AtomicU64,
    pub local_synapse_hits: AtomicU64,
    pub local_same_fpaa_routes: AtomicU64,
    pub local_same_pika_routes: AtomicU64,
    pub local_same_bridge_routes: AtomicU64,
    pub remote_udp_forwards: AtomicU64,
    pub host_mirror_events: AtomicU64,
    pub dropped_unknown_synapse: AtomicU64,
    pub dropped_ttl_expired: AtomicU64,
    pub gpio_emit_events: AtomicU64,
    pub gpio_capture_events: AtomicU64,
    pub software_kernel_fallback_events: AtomicU64,
}

impl Metrics {
    pub fn log_snapshot(&self) {
        let events_rx_udp = self.events_rx_udp.load(Ordering::Relaxed);
        let events_tx_udp = self.events_tx_udp.load(Ordering::Relaxed);
        let local_same_fpaa_routes = self.local_same_fpaa_routes.load(Ordering::Relaxed);
        let local_same_pika_routes = self.local_same_pika_routes.load(Ordering::Relaxed);
        let local_same_bridge_routes = self.local_same_bridge_routes.load(Ordering::Relaxed);
        let remote_udp_forwards = self.remote_udp_forwards.load(Ordering::Relaxed);
        let host_mirror_events = self.host_mirror_events.load(Ordering::Relaxed);
        let dropped_unknown_synapse = self.dropped_unknown_synapse.load(Ordering::Relaxed);
        let dropped_ttl_expired = self.dropped_ttl_expired.load(Ordering::Relaxed);
        let software_kernel_fallback_events =
            self.software_kernel_fallback_events.load(Ordering::Relaxed);

        let local_routes_total =
            local_same_fpaa_routes + local_same_pika_routes + local_same_bridge_routes;
        let resolved_routes_total = local_routes_total + remote_udp_forwards;
        let local_route_share_pct = pct(local_routes_total, resolved_routes_total);
        let remote_route_share_pct = pct(remote_udp_forwards, resolved_routes_total);

        info!(
            events_rx_udp = events_rx_udp,
            events_tx_udp = events_tx_udp,
            local_synapse_hits = self.local_synapse_hits.load(Ordering::Relaxed),
            local_same_fpaa_routes = local_same_fpaa_routes,
            local_same_pika_routes = local_same_pika_routes,
            local_same_bridge_routes = local_same_bridge_routes,
            remote_udp_forwards = remote_udp_forwards,
            host_mirror_events = host_mirror_events,
            dropped_unknown_synapse = dropped_unknown_synapse,
            dropped_ttl_expired = dropped_ttl_expired,
            software_kernel_fallback_events = software_kernel_fallback_events,
            local_routes_total = local_routes_total,
            resolved_routes_total = resolved_routes_total,
            local_route_share_pct = local_route_share_pct,
            remote_route_share_pct = remote_route_share_pct,
            "metrics snapshot"
        );
    }
}

fn pct(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        (numerator as f64 * 100.0) / denominator as f64
    }
}
