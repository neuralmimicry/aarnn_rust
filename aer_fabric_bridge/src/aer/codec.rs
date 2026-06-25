use crate::aer::event::{AerEvent, AerFlags};
use crate::aer::packet::{
    AER_PACKET_MAGIC, AER_PACKET_VERSION, EVENT_WIRE_SIZE, HEADER_WIRE_SIZE, MAX_EVENTS_PER_PACKET,
};
use crate::aer::{AerPacketHeader, SynapseId};
use anyhow::{Context, anyhow, bail};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read};

#[derive(Debug, Clone)]
pub struct DecodedPacket {
    pub header: AerPacketHeader,
    pub events: Vec<AerEvent>,
}

pub fn encode_packet(
    source_node_slot: u16,
    sequence: u32,
    send_time_ns: u64,
    events: &[AerEvent],
) -> anyhow::Result<Vec<u8>> {
    if events.is_empty() {
        bail!("cannot encode packet with zero events");
    }
    if events.len() > MAX_EVENTS_PER_PACKET {
        bail!(
            "event count {} exceeds MAX_EVENTS_PER_PACKET {}",
            events.len(),
            MAX_EVENTS_PER_PACKET
        );
    }

    let mut out = Vec::with_capacity(HEADER_WIRE_SIZE + events.len() * EVENT_WIRE_SIZE);
    out.write_u32::<LittleEndian>(AER_PACKET_MAGIC)?;
    out.write_u16::<LittleEndian>(AER_PACKET_VERSION)?;
    out.write_u16::<LittleEndian>(events.len() as u16)?;
    out.write_u16::<LittleEndian>(source_node_slot)?;
    out.write_u16::<LittleEndian>(0)?;
    out.write_u32::<LittleEndian>(sequence)?;
    out.write_u64::<LittleEndian>(send_time_ns)?;

    for event in events {
        if event.ttl == 0 {
            bail!("ttl cannot be zero when encoding");
        }
        out.write_u64::<LittleEndian>(event.synapse_id.0)?;
        out.write_u32::<LittleEndian>(event.flags.bits())?;
        out.write_u32::<LittleEndian>(event.value)?;
        out.write_u64::<LittleEndian>(event.event_time_ns)?;
        out.write_u32::<LittleEndian>(event.pulse_width_ns)?;
        out.write_u16::<LittleEndian>(event.source_node_slot)?;
        out.write_u8(event.ttl)?;
        out.write_u8(0)?;
    }

    Ok(out)
}

pub fn decode_packet(payload: &[u8]) -> anyhow::Result<DecodedPacket> {
    if payload.len() < HEADER_WIRE_SIZE {
        bail!("packet shorter than header");
    }
    let mut cursor = Cursor::new(payload);
    let magic = cursor.read_u32::<LittleEndian>()?;
    if magic != AER_PACKET_MAGIC {
        bail!("invalid packet magic 0x{magic:08x}");
    }

    let version = cursor.read_u16::<LittleEndian>()?;
    if version != AER_PACKET_VERSION {
        bail!("unsupported packet version {}", version);
    }

    let count = cursor.read_u16::<LittleEndian>()? as usize;
    if count == 0 {
        bail!("packet has zero events");
    }
    if count > MAX_EVENTS_PER_PACKET {
        bail!("packet count {} exceeds {}", count, MAX_EVENTS_PER_PACKET);
    }

    let source_node_slot = cursor.read_u16::<LittleEndian>()?;
    let reserved = cursor.read_u16::<LittleEndian>()?;
    let sequence = cursor.read_u32::<LittleEndian>()?;
    let send_time_ns = cursor.read_u64::<LittleEndian>()?;

    let expected_len = HEADER_WIRE_SIZE + count * EVENT_WIRE_SIZE;
    if payload.len() != expected_len {
        bail!(
            "inconsistent packet length: got {}, expected {}",
            payload.len(),
            expected_len
        );
    }

    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        let synapse_id = cursor.read_u64::<LittleEndian>()?;
        let flags = cursor.read_u32::<LittleEndian>()?;
        let value = cursor.read_u32::<LittleEndian>()?;
        let event_time_ns = cursor.read_u64::<LittleEndian>()?;
        let pulse_width_ns = cursor.read_u32::<LittleEndian>()?;
        let event_source_slot = cursor.read_u16::<LittleEndian>()?;
        let ttl = cursor.read_u8()?;
        let _event_reserved = cursor.read_u8()?;
        if ttl == 0 {
            bail!("received ttl=0 event");
        }

        events.push(AerEvent {
            synapse_id: SynapseId(synapse_id),
            flags: AerFlags::from_bits(flags)
                .ok_or_else(|| anyhow!("invalid aer flags 0x{flags:08x}"))?,
            value,
            event_time_ns,
            pulse_width_ns,
            ttl,
            source_node_slot: event_source_slot,
            sequence,
        });
    }

    let mut trailing = Vec::new();
    cursor
        .read_to_end(&mut trailing)
        .context("failed to read trailing payload bytes")?;
    if !trailing.is_empty() {
        bail!("unexpected trailing payload bytes");
    }

    let header = AerPacketHeader {
        magic,
        version,
        count: count as u16,
        source_node_slot,
        reserved,
        sequence,
        send_time_ns,
    };
    Ok(DecodedPacket { header, events })
}

#[cfg(test)]
mod tests {
    use super::{decode_packet, encode_packet};
    use crate::aer::{AerEvent, AerFlags, SynapseId};

    #[test]
    fn packet_round_trip_encode_decode() {
        let events = vec![AerEvent {
            synapse_id: SynapseId(0x5001_0002_0000_1234),
            flags: AerFlags::REMOTE,
            value: 7,
            event_time_ns: 42,
            pulse_width_ns: 5_000,
            ttl: 3,
            source_node_slot: 4,
            sequence: 10,
        }];
        let payload = encode_packet(4, 10, 500, &events).expect("encode succeeds");
        let decoded = decode_packet(&payload).expect("decode succeeds");
        assert_eq!(decoded.header.count, 1);
        assert_eq!(decoded.events, events);
    }

    #[test]
    fn invalid_packets_are_rejected() {
        let mut payload = vec![0u8; 24];
        payload[..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert!(decode_packet(&payload).is_err());
    }
}
