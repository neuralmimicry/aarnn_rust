//! Governed, bounded AARNN-AER/1 application protocol.
//!
//! This layer is transport-neutral: QUIC, WSS, WebRTC data channels and
//! supported USB/IP adapters carry the same frames. Arrival time is never
//! used as biological time. Device/session/path epochs and capture mapping
//! provenance remain stable across path migration.

use crate::deterministic::{EventId, LogicalTag, StreamId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};
use thiserror::Error;

pub const AER_PROTOCOL_VERSION: u16 = 1;
pub const MAX_AER_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_AER_PAYLOAD_BYTES: usize = MAX_AER_FRAME_BYTES - 256;
pub const MAX_AER_RECEIVED_SEQUENCES: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AerDirection {
    Producer,
    Consumer,
    Duplex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AerPayloadType {
    Events,
    SpikeIndices,
    Gap,
    Effect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AerGap {
    pub first_missing_sequence: u64,
    pub count: u32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AerFrame {
    pub protocol_version: u16,
    pub session_id: StreamId,
    pub endpoint_epoch: u64,
    pub device_epoch: u64,
    pub source_sequence: u64,
    pub capture_timestamp_ns: u64,
    pub clock_mapping_version: u64,
    pub clock_uncertainty_ns: u64,
    pub direction: AerDirection,
    pub address_space_version: u64,
    pub polarity: bool,
    pub payload_type: AerPayloadType,
    pub frame_sequence: u64,
    pub gap: Option<AerGap>,
    pub effect_id: Option<EventId>,
    pub payload: Vec<u8>,
    pub crc16: u16,
}

impl AerFrame {
    pub fn seal(mut self) -> Result<Self, AerTransportError> {
        self.validate_shape()?;
        self.crc16 = crc16(&self.canonical_bytes_without_crc());
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), AerTransportError> {
        self.validate_shape()?;
        if self.crc16 != crc16(&self.canonical_bytes_without_crc()) {
            return Err(AerTransportError::CrcMismatch);
        }
        Ok(())
    }

    /// Capture provenance maps to a declared logical tag outside this
    /// protocol; this helper intentionally does not inspect arrival time.
    pub fn mapped_tag(
        &self,
        mapping_origin_ns: u64,
        biological_origin: LogicalTag,
        nanos_per_tick: u64,
    ) -> Result<LogicalTag, AerTransportError> {
        if self.capture_timestamp_ns < mapping_origin_ns || nanos_per_tick == 0 {
            return Err(AerTransportError::InvalidMapping);
        }
        let delta = self.capture_timestamp_ns - mapping_origin_ns;
        let tick_delta = delta / nanos_per_tick;
        let tick = biological_origin
            .tick
            .checked_add(tick_delta)
            .ok_or(AerTransportError::InvalidMapping)?;
        Ok(LogicalTag::new(tick, biological_origin.microstep))
    }

    fn validate_shape(&self) -> Result<(), AerTransportError> {
        if self.protocol_version != AER_PROTOCOL_VERSION
            || self.endpoint_epoch == 0
            || self.device_epoch == 0
            || self.clock_mapping_version == 0
            || self.address_space_version == 0
        {
            return Err(AerTransportError::InvalidFrame(
                "version or epoch is invalid".to_owned(),
            ));
        }
        if self.payload.len() > MAX_AER_PAYLOAD_BYTES {
            return Err(AerTransportError::PayloadTooLarge(self.payload.len()));
        }
        if self
            .gap
            .as_ref()
            .is_some_and(|gap| gap.count == 0 || gap.reason.len() > 256)
        {
            return Err(AerTransportError::InvalidFrame(
                "invalid gap record".to_owned(),
            ));
        }
        if self.payload_type == AerPayloadType::Gap && self.gap.is_none() {
            return Err(AerTransportError::InvalidFrame(
                "gap payload lacks gap record".to_owned(),
            ));
        }
        Ok(())
    }

    fn canonical_bytes_without_crc(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.payload.len() + 128);
        bytes.extend_from_slice(&self.protocol_version.to_be_bytes());
        bytes.extend_from_slice(&self.session_id.raw().to_be_bytes());
        for value in [
            self.endpoint_epoch,
            self.device_epoch,
            self.source_sequence,
            self.capture_timestamp_ns,
            self.clock_mapping_version,
            self.clock_uncertainty_ns,
            self.address_space_version,
            self.frame_sequence,
        ] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.push(match self.direction {
            AerDirection::Producer => 0,
            AerDirection::Consumer => 1,
            AerDirection::Duplex => 2,
        });
        bytes.push(self.polarity as u8);
        bytes.push(self.payload_type as u8);
        bytes.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        if let Some(gap) = &self.gap {
            bytes.extend_from_slice(&gap.first_missing_sequence.to_be_bytes());
            bytes.extend_from_slice(&gap.count.to_be_bytes());
            bytes.extend_from_slice(gap.reason.as_bytes());
        }
        if let Some(effect_id) = self.effect_id {
            bytes.extend_from_slice(&effect_id.raw().to_be_bytes());
        }
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AerTransportError {
    #[error("AER frame is invalid: {0}")]
    InvalidFrame(String),
    #[error("AER payload is {0} bytes and exceeds the bound")]
    PayloadTooLarge(usize),
    #[error("AER frame CRC does not match its contents")]
    CrcMismatch,
    #[error("AER capture mapping is invalid")]
    InvalidMapping,
    #[error("AER frame belongs to another endpoint epoch")]
    StaleEndpoint,
    #[error("AER frame sequence gap: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("AER credit window exhausted")]
    CreditExhausted,
    #[error("AER receive window is full")]
    ReceiveWindowFull,
    #[error("AER {direction:?} queue is full")]
    QueueFull { direction: AerDirection },
    #[error("AER {direction:?} queue is closed")]
    QueueClosed { direction: AerDirection },
}

/// Fair bounded scheduler for independently governed AER directions.
///
/// The scheduler is deliberately transport-neutral and does not merge
/// producer, consumer or duplex queues. Round-robin service prevents a high
/// rate direction from starving the other direction; each queue has its own
/// bound and close state. Management/stop traffic remains outside this data
/// queue and must have its own reserved control path.
#[derive(Debug, Clone)]
pub struct AerChannelScheduler {
    queues: [VecDeque<AerFrame>; 3],
    closed: [bool; 3],
    capacity: usize,
    cursor: usize,
}

impl AerChannelScheduler {
    pub fn new(capacity_per_direction: usize) -> Result<Self, AerTransportError> {
        if capacity_per_direction == 0 {
            return Err(AerTransportError::InvalidFrame(
                "AER scheduler capacity must be non-zero".to_owned(),
            ));
        }
        Ok(Self {
            queues: std::array::from_fn(|_| VecDeque::new()),
            closed: [false; 3],
            capacity: capacity_per_direction,
            cursor: 0,
        })
    }

    pub fn enqueue(&mut self, frame: AerFrame) -> Result<(), AerTransportError> {
        frame.validate()?;
        let index = queue_index(frame.direction);
        if self.closed[index] {
            return Err(AerTransportError::QueueClosed {
                direction: frame.direction,
            });
        }
        if self.queues[index].len() >= self.capacity {
            return Err(AerTransportError::QueueFull {
                direction: frame.direction,
            });
        }
        self.queues[index].push_back(frame);
        Ok(())
    }

    pub fn pop_next(&mut self) -> Option<AerFrame> {
        for offset in 0..self.queues.len() {
            let index = (self.cursor + offset) % self.queues.len();
            if let Some(frame) = self.queues[index].pop_front() {
                self.cursor = (index + 1) % self.queues.len();
                return Some(frame);
            }
        }
        None
    }

    pub fn close(&mut self, direction: AerDirection) {
        self.closed[queue_index(direction)] = true;
    }

    pub fn pending(&self, direction: AerDirection) -> usize {
        self.queues[queue_index(direction)].len()
    }
}

const fn queue_index(direction: AerDirection) -> usize {
    match direction {
        AerDirection::Producer => 0,
        AerDirection::Consumer => 1,
        AerDirection::Duplex => 2,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AerAck {
    pub endpoint_epoch: u64,
    pub acknowledged_frame: u64,
    pub credit_window: u32,
    pub path_epoch: u64,
}

#[derive(Debug, Clone)]
pub struct AerSession {
    session_id: StreamId,
    endpoint_epoch: u64,
    device_epoch: u64,
    path_epoch: u64,
    next_frame: u64,
    acknowledged_frame: Option<u64>,
    credits: u32,
    credit_limit: u32,
    received: BTreeSet<u64>,
}

impl AerSession {
    pub fn new(
        session_id: StreamId,
        endpoint_epoch: u64,
        device_epoch: u64,
        credits: u32,
    ) -> Result<Self, AerTransportError> {
        if endpoint_epoch == 0 || device_epoch == 0 || credits == 0 {
            return Err(AerTransportError::InvalidFrame(
                "invalid session epoch or credit window".to_owned(),
            ));
        }
        Ok(Self {
            session_id,
            endpoint_epoch,
            device_epoch,
            path_epoch: 1,
            next_frame: 0,
            acknowledged_frame: None,
            credits,
            credit_limit: credits,
            received: BTreeSet::new(),
        })
    }

    pub fn next_frame(&mut self, mut frame: AerFrame) -> Result<AerFrame, AerTransportError> {
        if self.credits == 0 {
            return Err(AerTransportError::CreditExhausted);
        }
        frame.session_id = self.session_id;
        frame.endpoint_epoch = self.endpoint_epoch;
        frame.device_epoch = self.device_epoch;
        frame.frame_sequence = self.next_frame;
        frame = frame.seal()?;
        self.next_frame = self
            .next_frame
            .checked_add(1)
            .ok_or(AerTransportError::InvalidFrame(
                "frame sequence exhausted".to_owned(),
            ))?;
        self.credits -= 1;
        Ok(frame)
    }

    pub fn receive(&mut self, frame: AerFrame) -> Result<bool, AerTransportError> {
        frame.validate()?;
        if frame.session_id != self.session_id
            || frame.endpoint_epoch != self.endpoint_epoch
            || frame.device_epoch != self.device_epoch
        {
            return Err(AerTransportError::StaleEndpoint);
        }
        if self.received.contains(&frame.frame_sequence) {
            return Ok(false);
        }
        if self.received.len() >= MAX_AER_RECEIVED_SEQUENCES {
            return Err(AerTransportError::ReceiveWindowFull);
        }
        self.received.insert(frame.frame_sequence);
        Ok(true)
    }

    pub fn acknowledge(&mut self, frame: u64, credit_return: u32) -> AerAck {
        self.acknowledged_frame = Some(self.acknowledged_frame.map_or(frame, |old| old.max(frame)));
        self.credits = self
            .credits
            .saturating_add(credit_return)
            .min(self.credit_limit);
        AerAck {
            endpoint_epoch: self.endpoint_epoch,
            acknowledged_frame: self.acknowledged_frame.unwrap_or(frame),
            credit_window: self.credits,
            path_epoch: self.path_epoch,
        }
    }

    pub fn migrate_path(&mut self) -> Result<u64, AerTransportError> {
        self.path_epoch = self
            .path_epoch
            .checked_add(1)
            .ok_or(AerTransportError::InvalidFrame(
                "path epoch exhausted".to_owned(),
            ))?;
        Ok(self.path_epoch)
    }

    pub fn path_epoch(&self) -> u64 {
        self.path_epoch
    }
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(session: StreamId) -> AerFrame {
        AerFrame {
            protocol_version: AER_PROTOCOL_VERSION,
            session_id: session,
            endpoint_epoch: 1,
            device_epoch: 1,
            source_sequence: 4,
            capture_timestamp_ns: 100,
            clock_mapping_version: 1,
            clock_uncertainty_ns: 2,
            direction: AerDirection::Duplex,
            address_space_version: 1,
            polarity: true,
            payload_type: AerPayloadType::Events,
            frame_sequence: 0,
            gap: None,
            effect_id: None,
            payload: vec![1, 2],
            crc16: 0,
        }
    }

    #[test]
    fn bounded_frames_are_integrity_checked_and_path_migration_preserves_epoch() {
        let session_id = StreamId::new(9).unwrap();
        let mut session = AerSession::new(session_id, 1, 1, 2).unwrap();
        let sealed = session.next_frame(frame(session_id)).unwrap();
        assert!(session.receive(sealed.clone()).unwrap());
        assert!(!session.receive(sealed).unwrap());
        let old_path = session.path_epoch();
        assert_eq!(session.migrate_path().unwrap(), old_path + 1);
        assert_eq!(session.acknowledge(0, 1).path_epoch, old_path + 1);
    }

    #[test]
    fn arrival_time_does_not_enter_mapping() {
        let id = StreamId::new(1).unwrap();
        let frame = frame(id).seal().unwrap();
        assert_eq!(
            frame.mapped_tag(0, LogicalTag::ZERO, 100).unwrap(),
            LogicalTag::new(1, 0)
        );
    }

    #[test]
    fn fair_scheduler_bounds_each_direction_and_prevents_starvation() {
        let session = StreamId::new(3).unwrap();
        let mut scheduler = AerChannelScheduler::new(2).unwrap();
        let mut producer = frame(session);
        producer.direction = AerDirection::Producer;
        let mut consumer = frame(session);
        consumer.direction = AerDirection::Consumer;
        scheduler.enqueue(producer.clone().seal().unwrap()).unwrap();
        scheduler.enqueue(producer.clone().seal().unwrap()).unwrap();
        assert!(matches!(
            scheduler.enqueue(producer.clone().seal().unwrap()),
            Err(AerTransportError::QueueFull {
                direction: AerDirection::Producer
            })
        ));
        // The saturated producer queue cannot block a consumer frame or alter
        // producer ordering.
        scheduler.enqueue(consumer.clone().seal().unwrap()).unwrap();
        scheduler.enqueue(consumer.seal().unwrap()).unwrap();
        assert_eq!(
            scheduler.pop_next().unwrap().direction,
            AerDirection::Producer
        );
        assert_eq!(
            scheduler.pop_next().unwrap().direction,
            AerDirection::Consumer
        );
        scheduler.close(AerDirection::Consumer);
        let mut closed = frame(session);
        closed.direction = AerDirection::Consumer;
        assert!(matches!(
            scheduler.enqueue(closed.seal().unwrap()),
            Err(AerTransportError::QueueClosed {
                direction: AerDirection::Consumer
            })
        ));
    }
}
