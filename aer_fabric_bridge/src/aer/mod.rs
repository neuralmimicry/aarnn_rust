pub mod address;
pub mod codec;
pub mod event;
pub mod packet;

pub use address::SynapseId;
pub use codec::{DecodedPacket, decode_packet, encode_packet};
pub use event::{AerEvent, AerFlags};
pub use packet::{
    AER_PACKET_MAGIC, AER_PACKET_VERSION, AerEventWire, AerPacketHeader, MAX_EVENTS_PER_PACKET,
};
