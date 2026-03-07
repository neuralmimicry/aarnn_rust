//! Minimal AER <-> CAN frame helpers for robotic endpoints.
//!
//! This module defines a compact mapping that fits a single AER event into a
//! standard 8‑byte CAN data payload. Timestamps are not carried on the wire;
//! receivers should assign `ts_us` based on local arrival time if needed.

use crate::aer::AerEvent;

/// Generic CAN frame representation (no transport dependency).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AerCanFrame {
    /// CAN identifier (11-bit standard or 29-bit extended).
    pub id: u32,
    /// Data payload (up to 8 bytes).
    pub data: [u8; 8],
    /// Payload length.
    pub len: u8,
    /// Whether this uses the extended 29‑bit identifier.
    pub is_extended: bool,
}

/// Encode a single AER event into a CAN frame.
///
/// Layout (AER‑CAN v1):
/// - Bytes 0..4: addr (u32, little‑endian)
/// - Byte 4: value (u8)
/// - Bytes 5..8: unused (zero)
pub fn aer_event_to_can_frame(ev: &AerEvent, can_id: u32, is_extended: bool) -> AerCanFrame {
    let mut data = [0u8; 8];
    data[..4].copy_from_slice(&ev.addr.to_le_bytes());
    data[4] = ev.value;
    AerCanFrame {
        id: can_id,
        data,
        len: 5,
        is_extended,
    }
}

/// Decode a CAN frame into an AER event.
///
/// If `ts_us` is `None`, the event timestamp is set to 0 and should be
/// populated by the caller using the receive time.
pub fn can_frame_to_aer_event(frame: &AerCanFrame, ts_us: Option<u64>) -> Option<AerEvent> {
    if frame.len < 5 {
        return None;
    }
    let mut addr_bytes = [0u8; 4];
    addr_bytes.copy_from_slice(&frame.data[..4]);
    let addr = u32::from_le_bytes(addr_bytes);
    let value = frame.data[4];
    Some(AerEvent {
        ts_us: ts_us.unwrap_or(0),
        addr,
        value,
    })
}
