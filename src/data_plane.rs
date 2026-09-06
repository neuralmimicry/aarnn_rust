//! Reliable, bounded causal data-plane reference types.

use crate::deterministic::{
    BrainId, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId, PartitionGeneration,
    PrimitiveError, RouteId, SchemaVersion, StreamId,
};
use std::collections::{BTreeMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvelopeKind {
    Unknown = 0,
    Event = 1,
    Acknowledgement = 2,
    Watermark = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalEnvelope {
    pub schema_version: SchemaVersion,
    pub brain: BrainId,
    pub stream: StreamId,
    pub sequence: u64,
    pub lease_term: LeaseTerm,
    pub route: RouteId,
    pub partition_generation: PartitionGeneration,
    /// Optional biological endpoints. Control and field events need not have
    /// neuron endpoints, so zero on the wire is represented as `None`.
    pub source: Option<NeuronId>,
    pub target: Option<NeuronId>,
    pub tag: LogicalTag,
    pub event: EventId,
    pub stage: EventStage,
    pub kind: EnvelopeKind,
    pub payload: Vec<u8>,
    pub deferred_from_nonconvergence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DataPlaneError {
    #[error("stream credit exhausted")]
    CreditExhausted,
    #[error("bounded transport capacity {capacity} exceeded")]
    TransportFull { capacity: usize },
    #[error("transport is faulted; envelope remains unsent")]
    TransportFault,
    #[error("stream sequence exhausted")]
    SequenceOverflow,
    #[error("sequence gap on stream: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("stale lease term: expected {expected}, received {received}")]
    StaleTerm {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("stale partition generation: expected {expected}, received {received}")]
    StaleGeneration {
        expected: PartitionGeneration,
        received: PartitionGeneration,
    },
    #[error("payload length {actual} exceeds maximum {maximum}")]
    PayloadTooLarge { actual: usize, maximum: usize },
    #[error("unsupported causal envelope schema version {0:?}")]
    UnsupportedSchema(SchemaVersion),
    #[error("unknown causal envelope kind")]
    UnknownEnvelopeKind,
    #[error("watermark moved backwards from {current} to {received}")]
    WatermarkRegression {
        current: LogicalTag,
        received: LogicalTag,
    },
    #[error(transparent)]
    Primitive(#[from] PrimitiveError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveResult {
    Accepted { sequence: u64 },
    Duplicate { sequence: u64 },
}

#[derive(Debug, Clone)]
pub struct ReliableSender {
    brain: BrainId,
    stream: StreamId,
    term: LeaseTerm,
    route: RouteId,
    generation: PartitionGeneration,
    next_sequence: u64,
    credits: usize,
    credit_limit: usize,
    max_payload: usize,
    inflight: BTreeMap<u64, CausalEnvelope>,
}

impl ReliableSender {
    pub fn new(
        brain: BrainId,
        stream: StreamId,
        term: LeaseTerm,
        route: RouteId,
        generation: PartitionGeneration,
        credits: usize,
        max_payload: usize,
    ) -> Self {
        Self {
            brain,
            stream,
            term,
            route,
            generation,
            next_sequence: 0,
            credits,
            credit_limit: credits,
            max_payload,
            inflight: BTreeMap::new(),
        }
    }

    pub fn send(
        &mut self,
        tag: LogicalTag,
        event: EventId,
        payload: Vec<u8>,
    ) -> Result<CausalEnvelope, DataPlaneError> {
        self.send_stage(tag, event, EventStage::SpikeDecision, payload, false)
    }

    pub fn send_stage(
        &mut self,
        tag: LogicalTag,
        event: EventId,
        stage: EventStage,
        payload: Vec<u8>,
        deferred_from_nonconvergence: bool,
    ) -> Result<CausalEnvelope, DataPlaneError> {
        if self.credits == 0 {
            return Err(DataPlaneError::CreditExhausted);
        }
        if payload.len() > self.max_payload {
            return Err(DataPlaneError::PayloadTooLarge {
                actual: payload.len(),
                maximum: self.max_payload,
            });
        }
        let envelope = CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain: self.brain,
            stream: self.stream,
            sequence: self.next_sequence,
            lease_term: self.term,
            route: self.route,
            partition_generation: self.generation,
            source: None,
            target: None,
            tag,
            event,
            stage,
            kind: EnvelopeKind::Event,
            payload,
            deferred_from_nonconvergence,
        };
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(DataPlaneError::SequenceOverflow)?;
        self.credits -= 1;
        self.inflight.insert(envelope.sequence, envelope.clone());
        Ok(envelope)
    }

    /// A cumulative acknowledgement releases exactly the acknowledged prefix.
    pub fn acknowledge(&mut self, sequence: u64, credit_return: usize) {
        let released = self
            .inflight
            .keys()
            .copied()
            .take_while(|candidate| *candidate <= sequence)
            .collect::<Vec<_>>();
        for candidate in released {
            self.inflight.remove(&candidate);
        }
        self.credits = self
            .credits
            .saturating_add(credit_return)
            .min(self.credit_limit);
    }

    pub fn resume_from(&self, sequence: u64) -> Vec<CausalEnvelope> {
        self.inflight
            .range(sequence..)
            .map(|(_, envelope)| envelope.clone())
            .collect()
    }

    pub fn inflight_len(&self) -> usize {
        self.inflight.len()
    }
}

#[derive(Debug, Clone)]
pub struct ReliableReceiver {
    brain: BrainId,
    stream: StreamId,
    expected_sequence: u64,
    term: LeaseTerm,
    generation: PartitionGeneration,
    max_payload: usize,
    watermark: Option<LogicalTag>,
}

impl ReliableReceiver {
    pub fn new(
        brain: BrainId,
        stream: StreamId,
        term: LeaseTerm,
        generation: PartitionGeneration,
        max_payload: usize,
    ) -> Self {
        Self {
            brain,
            stream,
            expected_sequence: 0,
            term,
            generation,
            max_payload,
            watermark: None,
        }
    }

    /// Reconstruct a receiver cursor from a verified checkpoint. The caller
    /// must validate the corresponding WAL and receipt state before using the
    /// returned receiver.
    pub fn from_progress(
        brain: BrainId,
        stream: StreamId,
        term: LeaseTerm,
        generation: PartitionGeneration,
        max_payload: usize,
        expected_sequence: u64,
        watermark: Option<LogicalTag>,
    ) -> Result<Self, DataPlaneError> {
        if max_payload == 0 {
            return Err(DataPlaneError::PayloadTooLarge {
                actual: 1,
                maximum: 0,
            });
        }
        Ok(Self {
            brain,
            stream,
            expected_sequence,
            term,
            generation,
            max_payload,
            watermark,
        })
    }

    pub fn accept(&mut self, envelope: &CausalEnvelope) -> Result<ReceiveResult, DataPlaneError> {
        if envelope.brain != self.brain {
            return Err(DataPlaneError::SequenceGap {
                expected: self.expected_sequence,
                received: envelope.sequence,
            });
        }
        if envelope.schema_version != SchemaVersion::CURRENT {
            return Err(DataPlaneError::UnsupportedSchema(envelope.schema_version));
        }
        if envelope.kind == EnvelopeKind::Unknown {
            return Err(DataPlaneError::UnknownEnvelopeKind);
        }
        if envelope.stream != self.stream {
            return Err(DataPlaneError::SequenceGap {
                expected: self.expected_sequence,
                received: envelope.sequence,
            });
        }
        if envelope.lease_term != self.term {
            return Err(DataPlaneError::StaleTerm {
                expected: self.term,
                received: envelope.lease_term,
            });
        }
        if envelope.partition_generation != self.generation {
            return Err(DataPlaneError::StaleGeneration {
                expected: self.generation,
                received: envelope.partition_generation,
            });
        }
        if envelope.payload.len() > self.max_payload {
            return Err(DataPlaneError::PayloadTooLarge {
                actual: envelope.payload.len(),
                maximum: self.max_payload,
            });
        }
        if envelope.sequence < self.expected_sequence {
            return Ok(ReceiveResult::Duplicate {
                sequence: envelope.sequence,
            });
        }
        if envelope.sequence > self.expected_sequence {
            return Err(DataPlaneError::SequenceGap {
                expected: self.expected_sequence,
                received: envelope.sequence,
            });
        }
        self.expected_sequence = self
            .expected_sequence
            .checked_add(1)
            .ok_or(DataPlaneError::SequenceOverflow)?;
        Ok(ReceiveResult::Accepted {
            sequence: envelope.sequence,
        })
    }

    pub fn observe_watermark(&mut self, tag: LogicalTag) -> Result<(), DataPlaneError> {
        if let Some(current) = self.watermark {
            if tag < current {
                return Err(DataPlaneError::WatermarkRegression {
                    current,
                    received: tag,
                });
            }
        }
        self.watermark = Some(tag);
        Ok(())
    }

    pub const fn expected_sequence(&self) -> u64 {
        self.expected_sequence
    }

    pub const fn watermark(&self) -> Option<LogicalTag> {
        self.watermark
    }
}

/// Deterministic bounded transport used by protocol and fault tests.
#[derive(Debug, Clone)]
pub struct FaultInjectingTransport {
    capacity: usize,
    fault_next: bool,
    queue: VecDeque<CausalEnvelope>,
}

impl FaultInjectingTransport {
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            fault_next: false,
            queue: VecDeque::new(),
        }
    }

    pub const fn fault_next_send(&mut self) {
        self.fault_next = true;
    }

    pub fn send(&mut self, envelope: CausalEnvelope) -> Result<(), DataPlaneError> {
        if self.fault_next {
            self.fault_next = false;
            return Err(DataPlaneError::TransportFault);
        }
        if self.queue.len() >= self.capacity {
            return Err(DataPlaneError::TransportFull {
                capacity: self.capacity,
            });
        }
        self.queue.push_back(envelope);
        Ok(())
    }

    pub fn receive(&mut self) -> Option<CausalEnvelope> {
        self.queue.pop_front()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationProof {
    pub component: crate::deterministic::ComponentId,
    pub tag: LogicalTag,
    pub membership_epoch: u64,
    pub local_queue_empty: bool,
    pub output_staging_empty: bool,
    pub send_balance: i64,
    pub activity_epoch: u64,
}

impl TerminationProof {
    /// A route watermark alone is deliberately not part of this proof.
    pub const fn proves_closure(&self) -> bool {
        self.local_queue_empty
            && self.output_staging_empty
            && self.send_balance == 0
            && self.membership_epoch > 0
            && self.activity_epoch > 0
    }
}
