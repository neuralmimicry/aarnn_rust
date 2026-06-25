use crate::aer::AerEvent;
use crate::network::OutboundDatagram;
use crate::routing::RouteAction;
use tokio::sync::mpsc;

pub struct QueueSet {
    pub event_ingress_tx: mpsc::Sender<AerEvent>,
    pub event_ingress_rx: mpsc::Receiver<AerEvent>,
    pub route_action_tx: mpsc::Sender<RouteAction>,
    pub route_action_rx: mpsc::Receiver<RouteAction>,
    pub local_stimulus_tx: mpsc::Sender<RouteAction>,
    pub local_stimulus_rx: mpsc::Receiver<RouteAction>,
    pub outbound_tx: mpsc::Sender<OutboundDatagram>,
    pub outbound_rx: mpsc::Receiver<OutboundDatagram>,
}

impl QueueSet {
    pub fn new(buffer: usize) -> Self {
        let (event_ingress_tx, event_ingress_rx) = mpsc::channel(buffer);
        let (route_action_tx, route_action_rx) = mpsc::channel(buffer);
        let (local_stimulus_tx, local_stimulus_rx) = mpsc::channel(buffer);
        let (outbound_tx, outbound_rx) = mpsc::channel(buffer);
        Self {
            event_ingress_tx,
            event_ingress_rx,
            route_action_tx,
            route_action_rx,
            local_stimulus_tx,
            local_stimulus_rx,
            outbound_tx,
            outbound_rx,
        }
    }
}
