//! Independently governed workstation/peripheral reference boundaries.
//!
//! Samples enter the neural system only through an admitted channel carrying
//! capture provenance. Effects enter hardware only after committed neural
//! output, actuator fencing and deduplication.

use crate::deterministic::{BrainId, EventId, LeaseTerm, LogicalTag, StreamId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

/// Maximum payload accepted by the reference peripheral admission boundary.
/// High-rate media must use a separately negotiated bounded transport.
pub const MAX_PERIPHERAL_PAYLOAD_BYTES: usize = 1024 * 1024;
pub const MAX_PERIPHERAL_QUEUE_SAMPLES: usize = 4096;
const CAPTURE_DEDUPE_WINDOW: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChannelKind {
    UsbAer,
    Audio,
    Video,
    Keyboard,
    Pointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockMapping {
    pub version: u64,
    pub capture_origin_ns: u64,
    pub biological_origin_tick: u64,
    pub nanos_per_tick: u64,
    pub uncertainty_ns: u64,
}

impl ClockMapping {
    pub fn map(&self, capture_time_ns: u64) -> Result<(LogicalTag, u64), PeripheralError> {
        if self.version == 0 || self.nanos_per_tick == 0 || capture_time_ns < self.capture_origin_ns
        {
            return Err(PeripheralError::InvalidClockMapping);
        }
        let delta = capture_time_ns - self.capture_origin_ns;
        let tick_delta = delta / self.nanos_per_tick;
        let tick = self
            .biological_origin_tick
            .checked_add(tick_delta)
            .ok_or(PeripheralError::ClockOverflow)?;
        Ok((LogicalTag::new(tick, 0), self.uncertainty_ns))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralSample {
    pub channel: StreamId,
    pub device_epoch: u64,
    pub capture_sequence: u64,
    pub capture_time_ns: u64,
    pub mapping_version: u64,
    pub uncertainty_ns: u64,
    pub biological_tag: LogicalTag,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelGrant {
    pub input: bool,
    pub output: bool,
}

#[derive(Debug, Clone)]
struct ChannelState {
    kind: ChannelKind,
    direction: Direction,
    grant: ChannelGrant,
    epoch: u64,
    mapping: ClockMapping,
    connected: bool,
    capacity: usize,
    queue: VecDeque<PeripheralSample>,
    seen_sequences: BTreeSet<u64>,
}

/// Versioned peripheral state captured at a migration/checkpoint boundary.
///
/// The neural checkpoint must carry the admission cursor, queued samples and
/// effect deduplication/fencing state explicitly.  A digest of an opaque
/// channel blob is insufficient to reconstruct a safe destination because it
/// cannot prove which capture sequences or effect IDs have already been
/// accepted.  The lists are bounded by the same windows used by the live
/// admission and actuator paths.
pub const PERIPHERAL_CURSOR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralAdmissionCursor {
    pub channel: StreamId,
    pub device_epoch: u64,
    pub mapping_version: u64,
    pub last_capture_sequence: Option<u64>,
    pub admitted_sequences: Vec<u64>,
    pub queued_samples: Vec<PeripheralSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralEffectCursor {
    pub channel: StreamId,
    pub device_epoch: u64,
    pub lease_term: LeaseTerm,
    pub armed: bool,
    pub accepted_effect_ids: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralCursorState {
    pub schema_version: u32,
    pub admissions: Vec<PeripheralAdmissionCursor>,
    pub effects: Vec<PeripheralEffectCursor>,
}

impl Default for PeripheralCursorState {
    fn default() -> Self {
        Self::empty()
    }
}

impl PeripheralCursorState {
    pub fn empty() -> Self {
        Self {
            schema_version: PERIPHERAL_CURSOR_SCHEMA_VERSION,
            admissions: Vec::new(),
            effects: Vec::new(),
        }
    }

    pub fn verify(&self) -> Result<(), PeripheralError> {
        if self.schema_version != PERIPHERAL_CURSOR_SCHEMA_VERSION
            || self.admissions.len() > MAX_PERIPHERAL_QUEUE_SAMPLES
            || self.effects.len() > MAX_PERIPHERAL_QUEUE_SAMPLES
        {
            return Err(PeripheralError::InvalidCursorState);
        }
        let mut channels = BTreeSet::new();
        for admission in &self.admissions {
            if admission.device_epoch == 0
                || admission.mapping_version == 0
                || !channels.insert(admission.channel)
                || admission.admitted_sequences.len() > CAPTURE_DEDUPE_WINDOW
                || admission.queued_samples.len() > MAX_PERIPHERAL_QUEUE_SAMPLES
                || admission
                    .admitted_sequences
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || admission.queued_samples.iter().any(|sample| {
                    sample.channel != admission.channel
                        || sample.device_epoch != admission.device_epoch
                        || sample.mapping_version != admission.mapping_version
                        || sample.payload.len() > MAX_PERIPHERAL_PAYLOAD_BYTES
                })
            {
                return Err(PeripheralError::InvalidCursorState);
            }
        }
        channels.clear();
        for effect in &self.effects {
            if effect.device_epoch == 0
                || effect.lease_term.raw() == 0
                || !channels.insert(effect.channel)
                || effect.accepted_effect_ids.len() > CAPTURE_DEDUPE_WINDOW
                || effect
                    .accepted_effect_ids
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
            {
                return Err(PeripheralError::InvalidCursorState);
            }
        }
        Ok(())
    }

    /// Re-fence effect state while retaining the dedupe set and admitted
    /// samples.  This is used only after a newer shard lease is issued.
    pub fn reterm(&mut self, term: LeaseTerm) -> Result<(), PeripheralError> {
        if term.raw() == 0 {
            return Err(PeripheralError::InvalidCursorState);
        }
        for effect in &mut self.effects {
            effect.lease_term = term;
        }
        self.verify()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PeripheralError {
    #[error("channel {0} is not present")]
    UnknownChannel(StreamId),
    #[error("channel {0} is already bound")]
    ChannelAlreadyBound(StreamId),
    #[error("channel is not authorised for this direction")]
    DirectionNotAuthorised,
    #[error("channel is disconnected")]
    Disconnected,
    #[error("channel queue capacity {capacity} exceeded")]
    QueueFull { capacity: usize },
    #[error("channel queue capacity {capacity} is outside 1..={maximum}")]
    InvalidCapacity { capacity: usize, maximum: usize },
    #[error("capture sequence {0} was already admitted in this device epoch")]
    DuplicateCaptureSequence(u64),
    #[error("peripheral payload length {actual} exceeds maximum {maximum}")]
    PayloadTooLarge { actual: usize, maximum: usize },
    #[error("device epoch {received} is stale; current epoch is {expected}")]
    StaleDeviceEpoch { expected: u64, received: u64 },
    #[error("clock mapping version {received} is stale; current version is {expected}")]
    StaleMapping { expected: u64, received: u64 },
    #[error("clock mapping is invalid")]
    InvalidClockMapping,
    #[error("clock mapping overflow")]
    ClockOverflow,
    #[error("USB AER frame is invalid: {0}")]
    InvalidAerFrame(String),
    #[error("effect must originate from committed neural output")]
    UncommittedEffect,
    #[error("effect lease term {received} is stale; current term is {expected}")]
    StaleActuatorTerm {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("actuator is disarmed")]
    ActuatorDisarmed,
    #[error("effect device epoch is stale")]
    StaleEffectEpoch,
    #[error("device epoch space is exhausted")]
    DeviceEpochOverflow,
    #[error("peripheral cursor state is invalid or exceeds its bounded schema")]
    InvalidCursorState,
}

#[derive(Debug, Clone)]
pub struct PeripheralSession {
    pub id: EventId,
    pub brain: BrainId,
    pub principal: String,
    channels: BTreeMap<StreamId, ChannelState>,
}

impl PeripheralSession {
    pub fn new(id: EventId, brain: BrainId, principal: impl Into<String>) -> Self {
        Self {
            id,
            brain,
            principal: principal.into(),
            channels: BTreeMap::new(),
        }
    }

    pub fn bind_channel(
        &mut self,
        channel: StreamId,
        kind: ChannelKind,
        direction: Direction,
        grant: ChannelGrant,
        mapping: ClockMapping,
        capacity: usize,
    ) -> Result<(), PeripheralError> {
        if self.channels.contains_key(&channel) {
            return Err(PeripheralError::ChannelAlreadyBound(channel));
        }
        if capacity == 0 || capacity > MAX_PERIPHERAL_QUEUE_SAMPLES {
            return Err(PeripheralError::InvalidCapacity {
                capacity,
                maximum: MAX_PERIPHERAL_QUEUE_SAMPLES,
            });
        }
        if mapping.version == 0 || mapping.nanos_per_tick == 0 {
            return Err(PeripheralError::InvalidClockMapping);
        }
        self.channels.insert(
            channel,
            ChannelState {
                kind,
                direction,
                grant,
                epoch: 1,
                mapping,
                connected: true,
                capacity,
                queue: VecDeque::new(),
                seen_sequences: BTreeSet::new(),
            },
        );
        Ok(())
    }

    pub fn admit_sample(
        &mut self,
        channel: StreamId,
        device_epoch: u64,
        capture_sequence: u64,
        capture_time_ns: u64,
        mapping_version: u64,
        payload: Vec<u8>,
    ) -> Result<PeripheralSample, PeripheralError> {
        let state = self
            .channels
            .get_mut(&channel)
            .ok_or(PeripheralError::UnknownChannel(channel))?;
        if state.direction != Direction::Input || !state.grant.input {
            return Err(PeripheralError::DirectionNotAuthorised);
        }
        if !state.connected {
            return Err(PeripheralError::Disconnected);
        }
        if device_epoch != state.epoch {
            return Err(PeripheralError::StaleDeviceEpoch {
                expected: state.epoch,
                received: device_epoch,
            });
        }
        if mapping_version != state.mapping.version {
            return Err(PeripheralError::StaleMapping {
                expected: state.mapping.version,
                received: mapping_version,
            });
        }
        if state.seen_sequences.contains(&capture_sequence) {
            return Err(PeripheralError::DuplicateCaptureSequence(capture_sequence));
        }
        if payload.len() > MAX_PERIPHERAL_PAYLOAD_BYTES {
            return Err(PeripheralError::PayloadTooLarge {
                actual: payload.len(),
                maximum: MAX_PERIPHERAL_PAYLOAD_BYTES,
            });
        }
        if state.queue.len() >= state.capacity {
            return Err(PeripheralError::QueueFull {
                capacity: state.capacity,
            });
        }
        let (biological_tag, uncertainty_ns) = state.mapping.map(capture_time_ns)?;
        let sample = PeripheralSample {
            channel,
            device_epoch,
            capture_sequence,
            capture_time_ns,
            mapping_version,
            uncertainty_ns,
            biological_tag,
            payload,
        };
        state.seen_sequences.insert(capture_sequence);
        if state.seen_sequences.len() > CAPTURE_DEDUPE_WINDOW {
            if let Some(oldest) = state.seen_sequences.first().copied() {
                state.seen_sequences.remove(&oldest);
            }
        }
        state.queue.push_back(sample.clone());
        Ok(sample)
    }

    pub fn drain(
        &mut self,
        channel: StreamId,
        limit: usize,
    ) -> Result<Vec<PeripheralSample>, PeripheralError> {
        let state = self
            .channels
            .get_mut(&channel)
            .ok_or(PeripheralError::UnknownChannel(channel))?;
        Ok(state.queue.drain(..limit.min(state.queue.len())).collect())
    }

    /// Disconnect only the selected channel. Other A/V/HID/AER channels retain
    /// their own state and queues.
    pub fn disconnect(&mut self, channel: StreamId) -> Result<(), PeripheralError> {
        let state = self
            .channels
            .get_mut(&channel)
            .ok_or(PeripheralError::UnknownChannel(channel))?;
        state.connected = false;
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or(PeripheralError::DeviceEpochOverflow)?;
        state.queue.clear();
        state.seen_sequences.clear();
        Ok(())
    }

    pub fn reconnect(
        &mut self,
        channel: StreamId,
        mapping: ClockMapping,
    ) -> Result<u64, PeripheralError> {
        let state = self
            .channels
            .get_mut(&channel)
            .ok_or(PeripheralError::UnknownChannel(channel))?;
        if mapping.version <= state.mapping.version {
            return Err(PeripheralError::StaleMapping {
                expected: state.mapping.version + 1,
                received: mapping.version,
            });
        }
        state.mapping = mapping;
        state.connected = true;
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or(PeripheralError::DeviceEpochOverflow)?;
        state.seen_sequences.clear();
        Ok(state.epoch)
    }

    pub fn channel_kind(&self, channel: StreamId) -> Result<ChannelKind, PeripheralError> {
        self.channels
            .get(&channel)
            .map(|state| state.kind)
            .ok_or(PeripheralError::UnknownChannel(channel))
    }

    /// Export admission cursors and in-flight samples for an immutable
    /// checkpoint.  The returned value is deterministic in channel order.
    pub fn cursor_state(&self) -> Result<PeripheralCursorState, PeripheralError> {
        let state = PeripheralCursorState {
            schema_version: PERIPHERAL_CURSOR_SCHEMA_VERSION,
            admissions: self
                .channels
                .iter()
                .map(|(channel, state)| PeripheralAdmissionCursor {
                    channel: *channel,
                    device_epoch: state.epoch,
                    mapping_version: state.mapping.version,
                    last_capture_sequence: state.seen_sequences.iter().next_back().copied(),
                    admitted_sequences: state.seen_sequences.iter().copied().collect(),
                    queued_samples: state.queue.iter().cloned().collect(),
                })
                .collect(),
            effects: Vec::new(),
        };
        state.verify()?;
        Ok(state)
    }

    /// Revoke the leased session and close every local device binding.
    pub fn revoke(&mut self) {
        for state in self.channels.values_mut() {
            state.connected = false;
            // Revocation closes the binding before any future admission. If
            // the epoch counter is exhausted it is intentionally left at its
            // terminal value; disconnected state still rejects samples and a
            // later reconnect reports exhaustion explicitly.
            if let Some(next_epoch) = state.epoch.checked_add(1) {
                state.epoch = next_epoch;
            }
            state.grant.input = false;
            state.grant.output = false;
            state.queue.clear();
            state.seen_sequences.clear();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbAerFrame {
    pub protocol_version: u16,
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub address: u32,
    pub polarity: bool,
    pub crc16: Option<u16>,
}

impl UsbAerFrame {
    pub fn validate(&self, max_address: u32) -> Result<(), PeripheralError> {
        if self.protocol_version == 0 {
            return Err(PeripheralError::InvalidAerFrame(
                "unknown protocol version".to_owned(),
            ));
        }
        if self.address > max_address {
            return Err(PeripheralError::InvalidAerFrame(
                "address outside allow-list".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectCommand {
    pub id: EventId,
    pub channel: StreamId,
    pub device_epoch: u64,
    pub lease_term: LeaseTerm,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ActuatorGate {
    term: LeaseTerm,
    armed: bool,
    accepted: BTreeSet<EventId>,
}

impl ActuatorGate {
    pub fn new(term: LeaseTerm) -> Self {
        Self {
            term,
            armed: false,
            accepted: BTreeSet::new(),
        }
    }

    pub fn arm(&mut self, term: LeaseTerm) -> Result<(), PeripheralError> {
        if term < self.term {
            return Err(PeripheralError::StaleActuatorTerm {
                expected: self.term,
                received: term,
            });
        }
        self.term = term;
        self.armed = true;
        Ok(())
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn commit(
        &mut self,
        command: EffectCommand,
        committed_neural_output: bool,
        current_epoch: u64,
    ) -> Result<bool, PeripheralError> {
        if !committed_neural_output {
            return Err(PeripheralError::UncommittedEffect);
        }
        if !self.armed {
            return Err(PeripheralError::ActuatorDisarmed);
        }
        if command.lease_term != self.term {
            return Err(PeripheralError::StaleActuatorTerm {
                expected: self.term,
                received: command.lease_term,
            });
        }
        if command.device_epoch != current_epoch {
            return Err(PeripheralError::StaleEffectEpoch);
        }
        Ok(self.accepted.insert(command.id))
    }

    /// Export the exact actuator term, epoch, armed state and effect dedupe
    /// set required for a safe handoff.
    pub fn cursor_state(
        &self,
        channel: StreamId,
        device_epoch: u64,
    ) -> Result<PeripheralEffectCursor, PeripheralError> {
        let mut accepted_effect_ids = self.accepted.iter().copied().collect::<Vec<_>>();
        accepted_effect_ids.sort_unstable();
        let cursor = PeripheralEffectCursor {
            channel,
            device_epoch,
            lease_term: self.term,
            armed: self.armed,
            accepted_effect_ids,
        };
        PeripheralCursorState {
            schema_version: PERIPHERAL_CURSOR_SCHEMA_VERSION,
            admissions: Vec::new(),
            effects: vec![cursor.clone()],
        }
        .verify()?;
        Ok(cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (PeripheralSession, StreamId) {
        let channel = StreamId::new(1).expect("channel");
        let mut session = PeripheralSession::new(
            EventId::new(1).expect("session"),
            BrainId::new(1).expect("brain"),
            "test",
        );
        session
            .bind_channel(
                channel,
                ChannelKind::UsbAer,
                Direction::Input,
                ChannelGrant {
                    input: true,
                    output: false,
                },
                ClockMapping {
                    version: 1,
                    capture_origin_ns: 0,
                    biological_origin_tick: 0,
                    nanos_per_tick: 1,
                    uncertainty_ns: 0,
                },
                4,
            )
            .expect("bind");
        (session, channel)
    }

    #[test]
    fn admission_rejects_duplicate_sequences_but_allows_reordered_unique_samples() {
        let (mut session, channel) = session();
        session
            .admit_sample(channel, 1, 4, 4, 1, vec![4])
            .expect("first sample");
        session
            .admit_sample(channel, 1, 2, 2, 1, vec![2])
            .expect("reordered unique sample");
        assert!(matches!(
            session.admit_sample(channel, 1, 4, 4, 1, vec![4]),
            Err(PeripheralError::DuplicateCaptureSequence(4))
        ));
    }

    #[test]
    fn admission_rejects_oversized_payload_before_queue_mutation() {
        let (mut session, channel) = session();
        let payload = vec![0; MAX_PERIPHERAL_PAYLOAD_BYTES + 1];
        assert!(matches!(
            session.admit_sample(channel, 1, 1, 1, 1, payload),
            Err(PeripheralError::PayloadTooLarge { .. })
        ));
        assert!(session.drain(channel, 1).expect("drain").is_empty());
    }

    #[test]
    fn channel_capacity_is_bounded_before_binding() {
        let channel = StreamId::new(1).expect("channel");
        let mut session = PeripheralSession::new(
            EventId::new(1).expect("session"),
            BrainId::new(1).expect("brain"),
            "test",
        );
        let result = session.bind_channel(
            channel,
            ChannelKind::Audio,
            Direction::Input,
            ChannelGrant {
                input: true,
                output: false,
            },
            ClockMapping {
                version: 1,
                capture_origin_ns: 0,
                biological_origin_tick: 0,
                nanos_per_tick: 1,
                uncertainty_ns: 0,
            },
            MAX_PERIPHERAL_QUEUE_SAMPLES + 1,
        );
        assert!(matches!(
            result,
            Err(PeripheralError::InvalidCapacity { .. })
        ));
    }
}
