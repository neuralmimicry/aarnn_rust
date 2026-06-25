pub const AER_PACKET_MAGIC: u32 = 0x4145_5231;
pub const AER_PACKET_VERSION: u16 = 1;
pub const MAX_EVENTS_PER_PACKET: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AerPacketHeader {
    pub magic: u32,
    pub version: u16,
    pub count: u16,
    pub source_node_slot: u16,
    pub reserved: u16,
    pub sequence: u32,
    pub send_time_ns: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AerEventWire {
    pub synapse_id: u64,
    pub flags: u32,
    pub value: u32,
    pub event_time_ns: u64,
    pub pulse_width_ns: u32,
    pub source_node_slot: u16,
    pub ttl: u8,
    pub reserved: u8,
}

pub const HEADER_WIRE_SIZE: usize = 24;
pub const EVENT_WIRE_SIZE: usize = 32;
