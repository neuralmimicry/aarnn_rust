//! Versioned, bounded global-field events for the local causal executor.
//!
//! A field event is an explicit causal input.  It carries the effective
//! logical tag, scope, cadence and reduction rule rather than relying on a
//! runner-wide mutable value or a wall-clock callback.  The queue is bounded
//! and stores admitted future events in the executor, so an event cannot be
//! silently lost when a biological tick is being settled.

use crate::causal::CausalEvent;
use crate::deterministic::{
    CanonicalEvent, CanonicalEventKey, ComponentId, EventId, EventStage, LogicalTag, SchemaVersion,
    ShardId, StateDigest, canonical_event_digest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MAX_FIELD_QUEUE: usize = 4096;
pub const MAX_FIELD_ABS_VALUE: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FieldScope {
    WholeBrain,
    Component(ComponentId),
    Shard(ShardId),
    Layer(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FieldCadence {
    /// Apply this event once at its effective tag. This is used for derived
    /// next-tick fields whose value depends on the committed preceding tick.
    Once,
    EveryQuantum,
    Every {
        period_ticks: u64,
    },
}

impl FieldCadence {
    fn validate(self) -> Result<(), FieldEventError> {
        if matches!(self, Self::Every { period_ticks: 0 }) {
            return Err(FieldEventError::InvalidCadence);
        }
        Ok(())
    }

    pub fn next_tag(self, current: LogicalTag) -> Result<LogicalTag, FieldEventError> {
        match self {
            Self::Once => Err(FieldEventError::OneShotHasNoNextOccurrence),
            Self::EveryQuantum => current
                .next_quantum()
                .map_err(|_| FieldEventError::LogicalTimeOverflow),
            Self::Every { period_ticks } => current
                .positive_delay(period_ticks)
                .map_err(|_| FieldEventError::LogicalTimeOverflow),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FieldReduction {
    Replace,
    Sum,
    Mean,
    Maximum,
    ExponentialMovingAverage { alpha_millionths: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FieldKind {
    HomeostaticThresholdDelta,
    ResonanceLevel,
    AmbientDrive,
    PerceptualErrorDrive,
    Dopamine,
    Acetylcholine,
    Serotonin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldEvent {
    pub schema_version: SchemaVersion,
    pub id: EventId,
    pub effective_tag: LogicalTag,
    pub scope: FieldScope,
    pub cadence: FieldCadence,
    pub reduction: FieldReduction,
    pub kind: FieldKind,
    pub value: f64,
    /// Zero for the first occurrence. Recurring occurrences increment this
    /// value and derive a deterministic identity from the preceding event.
    #[serde(default)]
    pub occurrence: u64,
    /// Maximum declared age of an asynchronous snapshot used to derive this
    /// event.  It is observable metadata, not a licence to use arrival time.
    pub staleness_ticks: u64,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum FieldEventError {
    #[error("field event schema version {0} is unsupported")]
    UnsupportedSchema(SchemaVersion),
    #[error("field event value must be finite")]
    NonFiniteValue,
    #[error("field event value exceeds the configured bound")]
    ValueOutOfRange,
    #[error("field cadence period must be positive")]
    InvalidCadence,
    #[error("field logical time overflowed while scheduling the next occurrence")]
    LogicalTimeOverflow,
    #[error("field event identifier space is exhausted")]
    EventIdOverflow,
    #[error("one-shot field events do not have a next occurrence")]
    OneShotHasNoNextOccurrence,
    #[error("field occurrence sequence is exhausted")]
    OccurrenceOverflow,
    #[error("field reduction alpha must be between zero and one")]
    InvalidReductionAlpha,
    #[error("field reduction {reduction:?} is unsupported for field kind {kind:?}")]
    UnsupportedReduction {
        kind: FieldKind,
        reduction: FieldReduction,
    },
    #[error("field event queue capacity {capacity} exceeded")]
    QueueFull { capacity: usize },
    #[error("duplicate field event id {0}")]
    DuplicateEvent(EventId),
    #[error("field event payload is invalid: {0}")]
    InvalidPayload(String),
}

impl FieldEvent {
    pub fn new(
        id: EventId,
        effective_tag: LogicalTag,
        scope: FieldScope,
        cadence: FieldCadence,
        reduction: FieldReduction,
        kind: FieldKind,
        value: f64,
    ) -> Result<Self, FieldEventError> {
        let event = Self {
            schema_version: SchemaVersion::CURRENT,
            id,
            effective_tag,
            scope,
            cadence,
            reduction,
            kind,
            value,
            staleness_ticks: 0,
            occurrence: 0,
        };
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), FieldEventError> {
        if self.schema_version != SchemaVersion::CURRENT {
            return Err(FieldEventError::UnsupportedSchema(self.schema_version));
        }
        if !self.value.is_finite() {
            return Err(FieldEventError::NonFiniteValue);
        }
        if self.value.abs() > MAX_FIELD_ABS_VALUE {
            return Err(FieldEventError::ValueOutOfRange);
        }
        self.cadence.validate()?;
        if let FieldReduction::ExponentialMovingAverage { alpha_millionths } = self.reduction {
            if alpha_millionths > 1_000_000 {
                return Err(FieldEventError::InvalidReductionAlpha);
            }
        }
        if matches!(self.kind, FieldKind::HomeostaticThresholdDelta)
            && matches!(
                self.reduction,
                FieldReduction::ExponentialMovingAverage { .. }
            )
        {
            return Err(FieldEventError::UnsupportedReduction {
                kind: self.kind,
                reduction: self.reduction,
            });
        }
        Ok(())
    }

    /// Construct the next occurrence from the event's execution tag. The
    /// high-bit namespace keeps recurring field IDs separate from the local
    /// controller's low-bit input sequence; the derivation is deterministic
    /// and independent of transport or thread order.
    pub fn next_occurrence(
        &self,
        effective_tag: LogicalTag,
    ) -> Result<Option<Self>, FieldEventError> {
        if matches!(self.cadence, FieldCadence::Once) {
            return Ok(None);
        }
        let occurrence = self
            .occurrence
            .checked_add(1)
            .ok_or(FieldEventError::OccurrenceOverflow)?;
        let mut next = self.clone();
        next.effective_tag = self.cadence.next_tag(effective_tag)?;
        next.occurrence = occurrence;
        next.id = EventId::new(derive_recurring_event_id(self.id.raw(), occurrence))
            .map_err(|_| FieldEventError::OccurrenceOverflow)?;
        next.validate()?;
        Ok(Some(next))
    }

    pub fn payload(&self) -> Result<Vec<u8>, FieldEventError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|error| FieldEventError::InvalidPayload(error.to_string()))
    }

    pub fn from_payload(payload: &[u8]) -> Result<Self, FieldEventError> {
        let event: Self = serde_json::from_slice(payload)
            .map_err(|error| FieldEventError::InvalidPayload(error.to_string()))?;
        event.validate()?;
        Ok(event)
    }

    pub fn canonical_event(&self) -> Result<CanonicalEvent, FieldEventError> {
        let payload = self.payload()?;
        let source = match self.scope {
            FieldScope::WholeBrain => 0,
            FieldScope::Component(component) => component.raw(),
            FieldScope::Shard(shard) => shard.raw(),
            FieldScope::Layer(layer) => u64::from(layer).saturating_add(1),
        };
        Ok(CanonicalEvent {
            key: CanonicalEventKey::new(
                self.effective_tag,
                EventStage::FieldUpdate,
                source,
                0,
                self.id.raw(),
            ),
            payload,
        })
    }

    pub fn causal_event(&self) -> Result<CausalEvent, FieldEventError> {
        let canonical = self.canonical_event()?;
        Ok(CausalEvent::new(self.id, canonical.key, canonical.payload))
    }
}

fn derive_recurring_event_id(source: u64, occurrence: u64) -> u64 {
    let mut value = source
        .wrapping_add(occurrence.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0xD1B5_4A32_D192_ED03);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value = (value ^ (value >> 31)) | (1u64 << 63);
    value
}

/// Bounded admission helper used by the superdense adapter.  Once admitted,
/// events are owned by `LocalExecutor`; this queue only provides the exact
/// canonical digest for a batch and does not create a second pending store.
#[derive(Debug, Default)]
pub struct FieldEventBatch {
    events: BTreeMap<LogicalTag, Vec<FieldEvent>>,
}

impl FieldEventBatch {
    pub fn push(&mut self, event: FieldEvent) -> Result<(), FieldEventError> {
        event.validate()?;
        let count = self.events.values().map(Vec::len).sum::<usize>();
        if count >= MAX_FIELD_QUEUE {
            return Err(FieldEventError::QueueFull {
                capacity: MAX_FIELD_QUEUE,
            });
        }
        if self
            .events
            .values()
            .flatten()
            .any(|existing| existing.id == event.id)
        {
            return Err(FieldEventError::DuplicateEvent(event.id));
        }
        self.events
            .entry(event.effective_tag)
            .or_default()
            .push(event);
        Ok(())
    }

    pub fn digest(&self) -> Result<StateDigest, FieldEventError> {
        let events = self
            .events
            .values()
            .flatten()
            .map(FieldEvent::canonical_event)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(canonical_event_digest(&events))
    }

    pub fn into_causal_events(self) -> Result<Vec<CausalEvent>, FieldEventError> {
        let mut events = self
            .events
            .into_values()
            .flatten()
            .map(|event| event.causal_event())
            .collect::<Result<Vec<_>, _>>()?;
        events.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.id.cmp(&right.id))
                .then_with(|| left.payload.cmp(&right.payload))
        });
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: u64, tick: u64) -> FieldEvent {
        FieldEvent::new(
            EventId::new(id).unwrap(),
            LogicalTag::new(tick, 0),
            FieldScope::WholeBrain,
            FieldCadence::Every { period_ticks: 4 },
            FieldReduction::Replace,
            FieldKind::ResonanceLevel,
            0.25,
        )
        .unwrap()
    }

    #[test]
    fn field_payload_round_trips_and_digest_is_order_independent() {
        let first = sample(1, 8);
        let second = sample(2, 4);
        let decoded = FieldEvent::from_payload(&first.payload().unwrap()).unwrap();
        assert_eq!(decoded, first);

        let mut left = FieldEventBatch::default();
        left.push(first.clone()).unwrap();
        left.push(second.clone()).unwrap();
        let mut right = FieldEventBatch::default();
        right.push(second).unwrap();
        right.push(first).unwrap();
        assert_eq!(left.digest().unwrap(), right.digest().unwrap());

        let ordered = left.into_causal_events().unwrap();
        assert_eq!(ordered[0].key.tag, LogicalTag::new(4, 0));
        assert_eq!(ordered[1].key.tag, LogicalTag::new(8, 0));
    }

    #[test]
    fn field_validation_rejects_unbounded_and_zero_period_updates() {
        assert!(matches!(
            FieldEvent::new(
                EventId::new(1).unwrap(),
                LogicalTag::ZERO,
                FieldScope::WholeBrain,
                FieldCadence::Every { period_ticks: 0 },
                FieldReduction::Replace,
                FieldKind::AmbientDrive,
                0.0,
            ),
            Err(FieldEventError::InvalidCadence)
        ));
        assert!(matches!(
            FieldEvent::new(
                EventId::new(2).unwrap(),
                LogicalTag::ZERO,
                FieldScope::WholeBrain,
                FieldCadence::EveryQuantum,
                FieldReduction::Replace,
                FieldKind::AmbientDrive,
                f64::INFINITY,
            ),
            Err(FieldEventError::NonFiniteValue)
        ));
        assert!(matches!(
            FieldEvent::new(
                EventId::new(3).unwrap(),
                LogicalTag::ZERO,
                FieldScope::WholeBrain,
                FieldCadence::EveryQuantum,
                FieldReduction::ExponentialMovingAverage {
                    alpha_millionths: 1_000_001,
                },
                FieldKind::ResonanceLevel,
                0.0,
            ),
            Err(FieldEventError::InvalidReductionAlpha)
        ));
        assert!(matches!(
            FieldEvent::new(
                EventId::new(4).unwrap(),
                LogicalTag::ZERO,
                FieldScope::WholeBrain,
                FieldCadence::EveryQuantum,
                FieldReduction::ExponentialMovingAverage {
                    alpha_millionths: 500_000,
                },
                FieldKind::HomeostaticThresholdDelta,
                0.0,
            ),
            Err(FieldEventError::UnsupportedReduction { .. })
        ));
    }

    #[test]
    fn recurring_occurrences_advance_declared_cadence_and_use_stable_ids() {
        let first = sample(17, 3);
        let second = first
            .next_occurrence(first.effective_tag)
            .unwrap()
            .expect("recurring field has a next occurrence");
        assert_eq!(second.effective_tag, LogicalTag::new(7, 0));
        assert_eq!(second.occurrence, 1);
        assert_ne!(second.id, first.id);
        let replay = first
            .next_occurrence(first.effective_tag)
            .unwrap()
            .expect("recurring field has a next occurrence");
        assert_eq!(second, replay);
    }

    #[test]
    fn one_shot_field_has_no_recurring_output() {
        let mut event = sample(3, 8);
        event.cadence = FieldCadence::Once;
        event.validate().unwrap();
        assert_eq!(event.next_occurrence(event.effective_tag).unwrap(), None);
        assert!(matches!(
            FieldCadence::Once.next_tag(event.effective_tag),
            Err(FieldEventError::OneShotHasNoNextOccurrence)
        ));
    }
}
