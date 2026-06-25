use crate::aer::SynapseId;
use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub struct AerFlags: u32 {
        const REMOTE = 0x0001;
        const MIRROR = 0x0002;
        const CAPTURED = 0x0004;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AerEvent {
    pub synapse_id: SynapseId,
    pub flags: AerFlags,
    pub value: u32,
    pub event_time_ns: u64,
    pub pulse_width_ns: u32,
    pub ttl: u8,
    pub source_node_slot: u16,
    pub sequence: u32,
}

impl AerEvent {
    pub fn with_decremented_ttl(self) -> Option<Self> {
        if self.ttl <= 1 {
            return None;
        }
        let mut next = self;
        next.ttl -= 1;
        Some(next)
    }
}
