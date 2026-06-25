use crate::aer::{AerEvent, SynapseId};
use crate::metrics::Metrics;
use crate::routing::bridge_route_table::BridgeRouteTable;
use crate::routing::endpoint_table::EndpointRoute;
use crate::routing::host_subscription_table::HostSubscriptionTable;
use crate::routing::synapse_table::{LocalSynapseTable, SynapseEndpoint};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub struct Router {
    pub local_node_slot: u16,
    pub synapse_table: Arc<LocalSynapseTable>,
    pub bridge_routes: Arc<BridgeRouteTable>,
    pub host_subscriptions: Arc<HostSubscriptionTable>,
    pub metrics: Arc<Metrics>,
}

#[derive(Debug, Clone)]
pub enum RouteAction {
    LocalStimulus {
        synapse_id: SynapseId,
        endpoint: SynapseEndpoint,
        pulse_width_ns: u32,
        value: u32,
    },
    RemoteUdp {
        target_node_slot: u16,
        target_addr: SocketAddr,
        event: AerEvent,
    },
    HostMirror {
        target_addr: SocketAddr,
        event: AerEvent,
    },
    Drop {
        reason: String,
        event: AerEvent,
    },
}

impl Router {
    pub async fn route_event(&self, event: AerEvent) -> anyhow::Result<Vec<RouteAction>> {
        if event.ttl == 0 {
            self.metrics
                .dropped_ttl_expired
                .fetch_add(1, Ordering::Relaxed);
            return Ok(vec![RouteAction::Drop {
                reason: "ttl expired".to_string(),
                event,
            }]);
        }

        if let Some(entry) = self.synapse_table.lookup(event.synapse_id) {
            self.metrics
                .local_synapse_hits
                .fetch_add(1, Ordering::Relaxed);
            let mut actions = Vec::new();

            for endpoint in &entry.consumers {
                match endpoint.route {
                    EndpointRoute::LocalSameFpaa => {
                        self.metrics
                            .local_same_fpaa_routes
                            .fetch_add(1, Ordering::Relaxed);
                        actions.push(RouteAction::LocalStimulus {
                            synapse_id: event.synapse_id,
                            endpoint: endpoint.clone(),
                            pulse_width_ns: endpoint
                                .pulse_width_ns
                                .or(entry.delay_ns.into())
                                .unwrap_or(5_000),
                            value: event.value,
                        });
                    }
                    EndpointRoute::LocalSamePika => {
                        self.metrics
                            .local_same_pika_routes
                            .fetch_add(1, Ordering::Relaxed);
                        actions.push(RouteAction::LocalStimulus {
                            synapse_id: event.synapse_id,
                            endpoint: endpoint.clone(),
                            pulse_width_ns: endpoint
                                .pulse_width_ns
                                .or(entry.delay_ns.into())
                                .unwrap_or(5_000),
                            value: event.value,
                        });
                    }
                    EndpointRoute::LocalSameBridge => {
                        self.metrics
                            .local_same_bridge_routes
                            .fetch_add(1, Ordering::Relaxed);
                        actions.push(RouteAction::LocalStimulus {
                            synapse_id: event.synapse_id,
                            endpoint: endpoint.clone(),
                            pulse_width_ns: endpoint
                                .pulse_width_ns
                                .or(entry.delay_ns.into())
                                .unwrap_or(5_000),
                            value: event.value,
                        });
                    }
                    EndpointRoute::RemoteBridge => {
                        let Some(next_event) = event.with_decremented_ttl() else {
                            self.metrics
                                .dropped_ttl_expired
                                .fetch_add(1, Ordering::Relaxed);
                            actions.push(RouteAction::Drop {
                                reason: "ttl expired".to_string(),
                                event,
                            });
                            continue;
                        };
                        let route = self
                            .bridge_routes
                            .route_for_node_slot(endpoint.node_slot)
                            .or_else(|| self.bridge_routes.owner_for_synapse(event.synapse_id));
                        if let Some(route) = route {
                            self.metrics
                                .remote_udp_forwards
                                .fetch_add(1, Ordering::Relaxed);
                            actions.push(RouteAction::RemoteUdp {
                                target_node_slot: route.node_slot,
                                target_addr: route.socket_addr(),
                                event: next_event,
                            });
                        } else {
                            self.metrics
                                .dropped_unknown_synapse
                                .fetch_add(1, Ordering::Relaxed);
                            actions.push(RouteAction::Drop {
                                reason: format!(
                                    "remote route missing for node_slot={}",
                                    endpoint.node_slot
                                ),
                                event,
                            });
                        }
                    }
                    EndpointRoute::Host => {
                        for addr in self.host_subscriptions.all() {
                            self.metrics
                                .host_mirror_events
                                .fetch_add(1, Ordering::Relaxed);
                            actions.push(RouteAction::HostMirror {
                                target_addr: addr,
                                event,
                            });
                        }
                    }
                }
            }

            if entry.mirror_to_host {
                for addr in self.host_subscriptions.all() {
                    self.metrics
                        .host_mirror_events
                        .fetch_add(1, Ordering::Relaxed);
                    actions.push(RouteAction::HostMirror {
                        target_addr: addr,
                        event,
                    });
                }
            }

            if actions.is_empty() {
                actions.push(RouteAction::Drop {
                    reason: "synapse has no route actions".to_string(),
                    event,
                });
            }
            return Ok(actions);
        }

        let Some(next_event) = event.with_decremented_ttl() else {
            self.metrics
                .dropped_ttl_expired
                .fetch_add(1, Ordering::Relaxed);
            return Ok(vec![RouteAction::Drop {
                reason: "ttl expired".to_string(),
                event,
            }]);
        };

        if let Some(owner) = self.bridge_routes.owner_for_synapse(event.synapse_id) {
            self.metrics
                .remote_udp_forwards
                .fetch_add(1, Ordering::Relaxed);
            return Ok(vec![RouteAction::RemoteUdp {
                target_node_slot: owner.node_slot,
                target_addr: owner.socket_addr(),
                event: next_event,
            }]);
        }

        self.metrics
            .dropped_unknown_synapse
            .fetch_add(1, Ordering::Relaxed);
        Ok(vec![RouteAction::Drop {
            reason: "unknown synapse".to_string(),
            event,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::{RouteAction, Router};
    use crate::aer::{AerEvent, AerFlags, SynapseId};
    use crate::metrics::Metrics;
    use crate::routing::bridge_route_table::{BridgeRoute, BridgeRouteTable, SynapseRange};
    use crate::routing::endpoint_table::{EndpointRoute, EndpointType};
    use crate::routing::host_subscription_table::HostSubscriptionTable;
    use crate::routing::synapse_table::{LocalSynapseTable, SynapseEndpoint, SynapseEntry};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use uuid::Uuid;

    fn event_for(synapse_id: SynapseId) -> AerEvent {
        AerEvent {
            synapse_id,
            flags: AerFlags::empty(),
            value: 1,
            event_time_ns: 10,
            pulse_width_ns: 5_000,
            ttl: 3,
            source_node_slot: 0,
            sequence: 1,
        }
    }

    fn router_with_entry(entry: SynapseEntry) -> Router {
        let table = Arc::new(LocalSynapseTable::from_entries(vec![entry]));
        let routes = Arc::new(BridgeRouteTable::default());
        routes.upsert(BridgeRoute {
            node_slot: 2,
            node_uuid: Uuid::new_v4(),
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            event_port: 45_881,
            synapse_ranges: vec![SynapseRange {
                start: SynapseId(0x5001_0002_0000_0000),
                end: SynapseId(0x5001_0002_ffff_ffff),
            }],
            last_seen_ns: 0,
            ready: true,
        });
        let hosts = Arc::new(HostSubscriptionTable::default());
        hosts.add(SocketAddr::from(([127, 0, 0, 1], 45_882)));
        Router {
            local_node_slot: 0,
            synapse_table: table,
            bridge_routes: routes,
            host_subscriptions: hosts,
            metrics: Arc::new(Metrics::default()),
        }
    }

    #[tokio::test]
    async fn same_fpaa_synapse_routes_locally_without_udp() {
        let synapse_id = SynapseId(0x5001_0002_0000_1234);
        let router = router_with_entry(SynapseEntry {
            synapse_id,
            description: None,
            weight: 1.0,
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
        });

        let actions = router.route_event(event_for(synapse_id)).await.unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::LocalStimulus { .. }))
        );
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, RouteAction::RemoteUdp { .. }))
        );
    }

    #[tokio::test]
    async fn same_pika_synapse_routes_locally_without_udp() {
        let synapse_id = SynapseId(0x5001_0002_0000_1235);
        let router = router_with_entry(SynapseEntry {
            synapse_id,
            description: None,
            weight: 1.0,
            delay_ns: 0,
            mirror_to_host: false,
            producers: vec![],
            consumers: vec![SynapseEndpoint {
                endpoint_id: None,
                endpoint_type: EndpointType::DendriteBouton,
                location: None,
                node_slot: 0,
                fpaa_index: Some(2),
                neuron_id: Some(31),
                bouton_id: Some(4),
                io_name: None,
                route: EndpointRoute::LocalSamePika,
                weight: None,
                delay_ns: None,
                pulse_width_ns: Some(5_000),
                gpio_line: Some("FPAA2_IO5N".to_string()),
                gpio_mask: None,
                software_kernel: None,
            }],
        });

        let actions = router.route_event(event_for(synapse_id)).await.unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::LocalStimulus { .. }))
        );
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, RouteAction::RemoteUdp { .. }))
        );
    }

    #[tokio::test]
    async fn remote_synapse_consumer_uses_udp() {
        let synapse_id = SynapseId(0x5001_0002_0000_1236);
        let router = router_with_entry(SynapseEntry {
            synapse_id,
            description: None,
            weight: 1.0,
            delay_ns: 0,
            mirror_to_host: false,
            producers: vec![],
            consumers: vec![SynapseEndpoint {
                endpoint_id: None,
                endpoint_type: EndpointType::DendriteBouton,
                location: None,
                node_slot: 2,
                fpaa_index: Some(1),
                neuron_id: Some(8),
                bouton_id: Some(2),
                io_name: None,
                route: EndpointRoute::RemoteBridge,
                weight: None,
                delay_ns: None,
                pulse_width_ns: None,
                gpio_line: None,
                gpio_mask: None,
                software_kernel: None,
            }],
        });

        let actions = router.route_event(event_for(synapse_id)).await.unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::RemoteUdp { .. }))
        );
    }

    #[tokio::test]
    async fn mirror_to_host_creates_host_mirror() {
        let synapse_id = SynapseId(0x5001_0002_0000_1237);
        let router = router_with_entry(SynapseEntry {
            synapse_id,
            description: None,
            weight: 1.0,
            delay_ns: 0,
            mirror_to_host: true,
            producers: vec![],
            consumers: vec![],
        });

        let actions = router.route_event(event_for(synapse_id)).await.unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::HostMirror { .. }))
        );
    }

    #[tokio::test]
    async fn ttl_expiry_drops_event() {
        let synapse_id = SynapseId(0x5001_0002_0000_1238);
        let router = router_with_entry(SynapseEntry {
            synapse_id,
            description: None,
            weight: 1.0,
            delay_ns: 0,
            mirror_to_host: false,
            producers: vec![],
            consumers: vec![SynapseEndpoint {
                endpoint_id: None,
                endpoint_type: EndpointType::DendriteBouton,
                location: None,
                node_slot: 2,
                fpaa_index: None,
                neuron_id: None,
                bouton_id: None,
                io_name: None,
                route: EndpointRoute::RemoteBridge,
                weight: None,
                delay_ns: None,
                pulse_width_ns: None,
                gpio_line: None,
                gpio_mask: None,
                software_kernel: None,
            }],
        });
        let mut event = event_for(synapse_id);
        event.ttl = 1;
        let actions = router.route_event(event).await.unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::Drop { .. }))
        );
    }

    #[tokio::test]
    async fn unknown_synapse_drops_without_route() {
        let table = Arc::new(LocalSynapseTable::default());
        let routes = Arc::new(BridgeRouteTable::default());
        let hosts = Arc::new(HostSubscriptionTable::default());
        let router = Router {
            local_node_slot: 0,
            synapse_table: table,
            bridge_routes: routes,
            host_subscriptions: hosts,
            metrics: Arc::new(Metrics::default()),
        };

        let actions = router
            .route_event(event_for(SynapseId(0x5009_0002_0000_1234)))
            .await
            .unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::Drop { .. }))
        );
    }

    #[tokio::test]
    async fn unknown_synapse_forwards_when_route_table_has_owner() {
        let table = Arc::new(LocalSynapseTable::default());
        let routes = Arc::new(BridgeRouteTable::default());
        routes.upsert(BridgeRoute {
            node_slot: 4,
            node_uuid: Uuid::new_v4(),
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            event_port: 45_881,
            synapse_ranges: vec![SynapseRange {
                start: SynapseId(0x5001_0002_0000_0000),
                end: SynapseId(0x5001_0002_ffff_ffff),
            }],
            last_seen_ns: 0,
            ready: true,
        });
        let hosts = Arc::new(HostSubscriptionTable::default());
        let router = Router {
            local_node_slot: 0,
            synapse_table: table,
            bridge_routes: routes,
            host_subscriptions: hosts,
            metrics: Arc::new(Metrics::default()),
        };
        let actions = router
            .route_event(event_for(SynapseId(0x5001_0002_0000_9999)))
            .await
            .unwrap();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, RouteAction::RemoteUdp { .. }))
        );
    }
}
