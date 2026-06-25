use parking_lot::RwLock;
use std::collections::BTreeSet;
use std::net::SocketAddr;

#[derive(Default)]
pub struct HostSubscriptionTable {
    addrs: RwLock<BTreeSet<SocketAddr>>,
}

impl HostSubscriptionTable {
    pub fn add(&self, addr: SocketAddr) {
        self.addrs.write().insert(addr);
    }

    pub fn remove(&self, addr: &SocketAddr) {
        self.addrs.write().remove(addr);
    }

    pub fn all(&self) -> Vec<SocketAddr> {
        self.addrs.read().iter().copied().collect()
    }
}
