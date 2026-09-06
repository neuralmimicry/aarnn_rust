//! Additive generated-gRPC seam for causal event transport.
//!
//! This module owns wire conversion, bounded receiver validation and the
//! durable authoritative-shard service. The validation-only service remains
//! available for compatibility tests; the live node uses the authoritative
//! service path when its explicit migration profile is enabled.

use crate::authoritative_shard::AuthoritativeShard;
use crate::data_plane::{CausalEnvelope, DataPlaneError, ReceiveResult, ReliableReceiver};
use crate::deterministic::{EventStage, LogicalTag, NeuronId, SchemaVersion};
use crate::durability::{DurableApplyOutcome, DurableShard, ReceiptLedger, ReceiptOutcome};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

/// Rust bindings generated from `proto/distributed.proto` by `build.rs`.
pub mod proto {
    tonic::include_proto!("distributed");
}

#[derive(Debug, Error)]
pub enum CausalTransportError {
    #[error("causal envelope is missing its logical tag")]
    MissingTag,
    #[error("causal envelope contains unsupported event stage {0}")]
    UnsupportedStage(i32),
    #[error("causal envelope schema version {0} is not representable")]
    SchemaOutOfRange(u32),
    #[error("causal envelope field {0} is zero or invalid")]
    InvalidIdentity(&'static str),
    #[error(transparent)]
    DataPlane(#[from] DataPlaneError),
    #[error(transparent)]
    Durability(#[from] crate::durability::DurabilityError),
    #[error(transparent)]
    Primitive(#[from] crate::deterministic::PrimitiveError),
}

fn stage_from_wire(stage: i32) -> Result<EventStage, CausalTransportError> {
    match stage {
        1 => Ok(EventStage::SpikeDecision),
        2 => Ok(EventStage::AxonalDeparture),
        3 => Ok(EventStage::AxonalArrival),
        4 => Ok(EventStage::SynapticTransition),
        5 => Ok(EventStage::PostsynapticEffect),
        6 => Ok(EventStage::PlasticityUpdate),
        7 => Ok(EventStage::FieldUpdate),
        other => Err(CausalTransportError::UnsupportedStage(other)),
    }
}

fn stage_to_wire(stage: EventStage) -> i32 {
    match stage {
        EventStage::SpikeDecision => 1,
        EventStage::AxonalDeparture => 2,
        EventStage::AxonalArrival => 3,
        EventStage::SynapticTransition => 4,
        EventStage::PostsynapticEffect => 5,
        EventStage::PlasticityUpdate => 6,
        EventStage::FieldUpdate => 7,
    }
}

fn identity<T>(value: u64, name: &'static str) -> Result<T, CausalTransportError>
where
    T: TryFrom<u64, Error = crate::deterministic::PrimitiveError>,
{
    T::try_from(value).map_err(|_| CausalTransportError::InvalidIdentity(name))
}

impl TryFrom<proto::CausalEventEnvelope> for CausalEnvelope {
    type Error = CausalTransportError;

    fn try_from(value: proto::CausalEventEnvelope) -> Result<Self, Self::Error> {
        let schema = u16::try_from(value.schema_version)
            .map_err(|_| CausalTransportError::SchemaOutOfRange(value.schema_version))
            .and_then(|version| SchemaVersion::new(version).map_err(CausalTransportError::from))?;
        let tag = value.tag.ok_or(CausalTransportError::MissingTag)?;
        Ok(Self {
            schema_version: schema,
            brain: identity(value.brain_id, "brain_id")?,
            stream: identity(value.stream_id, "stream_id")?,
            sequence: value.sequence,
            lease_term: identity(value.lease_term, "lease_term")?,
            route: identity(value.route_id, "route_id")?,
            partition_generation: identity(value.partition_generation, "partition_generation")?,
            source: (value.source_id != 0)
                .then(|| identity(value.source_id, "source_id"))
                .transpose()?,
            target: (value.target_id != 0)
                .then(|| identity(value.target_id, "target_id"))
                .transpose()?,
            tag: LogicalTag::new(tag.tick, tag.microstep),
            event: identity(value.event_id, "event_id")?,
            stage: stage_from_wire(value.stage)?,
            kind: crate::data_plane::EnvelopeKind::Event,
            payload: value.payload,
            deferred_from_nonconvergence: value.deferred_from_nonconvergence,
        })
    }
}

impl From<&CausalEnvelope> for proto::CausalEventEnvelope {
    fn from(value: &CausalEnvelope) -> Self {
        Self {
            schema_version: u32::from(value.schema_version.raw()),
            brain_id: value.brain.raw(),
            stream_id: value.stream.raw(),
            sequence: value.sequence,
            lease_term: value.lease_term.raw(),
            route_id: value.route.raw(),
            partition_generation: value.partition_generation.raw(),
            tag: Some(proto::LogicalTag {
                tick: value.tag.tick,
                microstep: value.tag.microstep,
            }),
            event_id: value.event.raw(),
            stage: stage_to_wire(value.stage),
            source_id: value.source.map(NeuronId::raw).unwrap_or(0),
            target_id: value.target.map(NeuronId::raw).unwrap_or(0),
            payload: value.payload.clone(),
            deferred_from_nonconvergence: value.deferred_from_nonconvergence,
            sender_node_id: String::new(),
        }
    }
}

fn response_frame(envelope: &CausalEnvelope, sender_node_id: String) -> proto::CausalEventEnvelope {
    let mut frame = proto::CausalEventEnvelope::from(envelope);
    // The sender identity is transport-session metadata rather than part of
    // the biological envelope.  Preserve the authenticated wire value on an
    // acknowledgement without making it part of the model/event schema.
    frame.sender_node_id = sender_node_id;
    frame
}

/// Typed boundary used by the generated gRPC validation service.
/// Conversion and receiver checks happen before application-owned queue state
/// can be mutated.
#[derive(Debug)]
pub struct CausalStreamAdapter {
    receiver: ReliableReceiver,
    receipts: ReceiptLedger,
}

impl CausalStreamAdapter {
    pub fn new(receiver: ReliableReceiver) -> Self {
        Self {
            receiver,
            receipts: ReceiptLedger::default(),
        }
    }

    pub fn accept(
        &mut self,
        frame: proto::CausalEventEnvelope,
    ) -> Result<(CausalEnvelope, ReceiveResult), CausalTransportError> {
        let envelope = CausalEnvelope::try_from(frame)?;
        // Stage receiver and receipt mutations together.  If receipt
        // validation fails, the sequence cursor must not move either.
        let mut receiver = self.receiver.clone();
        let result = receiver.accept(&envelope)?;
        let mut event = crate::causal::CausalEvent::new(
            envelope.event,
            crate::deterministic::CanonicalEventKey::new(
                envelope.tag,
                envelope.stage,
                envelope.source.map(NeuronId::raw).unwrap_or(0),
                envelope.target.map(NeuronId::raw).unwrap_or(0),
                envelope.event.raw(),
            ),
            envelope.payload.clone(),
        );
        event.deferred_from_nonconvergence = envelope.deferred_from_nonconvergence;
        let mut receipts = self.receipts.clone();
        let receipt_outcome = receipts.record_event(
            envelope.stream,
            envelope.sequence,
            &event,
            envelope.lease_term,
            envelope.partition_generation,
        )?;
        if matches!(&result, ReceiveResult::Accepted { .. })
            && !matches!(receipt_outcome, ReceiptOutcome::New)
        {
            return Err(CausalTransportError::DataPlane(
                DataPlaneError::SequenceGap {
                    expected: receiver.expected_sequence(),
                    received: envelope.sequence,
                },
            ));
        }
        self.receiver = receiver;
        self.receipts = receipts;
        Ok((envelope, result))
    }

    pub fn receiver(&self) -> &ReliableReceiver {
        &self.receiver
    }

    pub fn receipts(&self) -> &ReceiptLedger {
        &self.receipts
    }
}

/// Causal transport adapter for an authoritative shard commit boundary.
/// Unlike [`CausalStreamAdapter`], this adapter does not acknowledge a frame
/// until the supplied pure biological transition, WAL append, warm replica
/// acknowledgement and durable receipt have all staged successfully.
#[derive(Debug)]
pub struct DurableCausalStreamAdapter {
    shard: DurableShard,
}

/// Generated gRPC data-plane service backed by a durable shard owner. A
/// response is emitted only after biological application, WAL append, warm
/// replication and receipt publication have all committed.
#[derive(Debug, Clone)]
pub struct DurableCausalService {
    adapter: Arc<Mutex<DurableCausalStreamAdapter>>,
    output_capacity: usize,
}

impl DurableCausalService {
    pub fn new(shard: DurableShard, output_capacity: usize) -> Self {
        Self {
            adapter: Arc::new(Mutex::new(DurableCausalStreamAdapter::new(shard))),
            output_capacity: output_capacity.max(1),
        }
    }

    pub fn adapter(&self) -> Arc<Mutex<DurableCausalStreamAdapter>> {
        Arc::clone(&self.adapter)
    }
}

/// Causal service backed by a single durable shard owner.  Unlike the
/// compatibility service below, this service has no in-memory receipt ledger
/// and no echo acknowledgement: the shard validates, applies, replicates and
/// publishes each event before returning it to the caller.
#[derive(Debug, Clone)]
pub struct AuthoritativeCausalService {
    shard: Arc<Mutex<AuthoritativeShard>>,
    output_capacity: usize,
    stable_biology: bool,
}

impl AuthoritativeCausalService {
    pub fn new(shard: AuthoritativeShard, output_capacity: usize) -> Self {
        Self {
            shard: Arc::new(Mutex::new(shard)),
            output_capacity: output_capacity.max(1),
            stable_biology: false,
        }
    }

    /// Construct an authoritative service using the stable-ID biological
    /// kernel. The shard's biological bytes must contain an encoded
    /// [`crate::authoritative_shard::StableBiologicalState`]; malformed state
    /// fails before the first acknowledgement. This constructor is an
    /// explicit migration profile and does not alter the compatibility
    /// constructor above.
    pub fn new_with_stable_biology(shard: AuthoritativeShard, output_capacity: usize) -> Self {
        Self {
            shard: Arc::new(Mutex::new(shard)),
            output_capacity: output_capacity.max(1),
            stable_biology: true,
        }
    }

    pub fn shard(&self) -> Arc<Mutex<AuthoritativeShard>> {
        Arc::clone(&self.shard)
    }
}

impl DurableCausalStreamAdapter {
    pub fn new(shard: DurableShard) -> Self {
        Self { shard }
    }

    pub fn shard(&self) -> &DurableShard {
        &self.shard
    }

    pub fn accept_and_apply<F, E>(
        &mut self,
        frame: proto::CausalEventEnvelope,
        transition: F,
    ) -> Result<(CausalEnvelope, DurableApplyOutcome), CausalTransportError>
    where
        F: FnOnce(&[u8], &crate::causal::CausalEvent) -> Result<Vec<u8>, E>,
        E: std::fmt::Display,
    {
        let envelope = CausalEnvelope::try_from(frame)?;
        let outcome = self.shard.apply_once(&envelope, transition)?;
        Ok((envelope, outcome))
    }
}

/// Bounded generated-gRPC validation service.
///
/// This service deliberately echoes only frames accepted by the typed
/// [`CausalStreamAdapter`]. It is a transport/contract test seam, not a shard
/// executor: no biological state is mutated and no frame is acknowledged as
/// durably applied. The production cutover must replace the echo with the
/// shard-owned apply/commit path and durable receipt boundary.
#[derive(Debug, Clone)]
pub struct CausalValidationService {
    adapter: Arc<Mutex<CausalStreamAdapter>>,
    output_capacity: usize,
}

impl CausalValidationService {
    pub fn new(receiver: ReliableReceiver, output_capacity: usize) -> Self {
        Self {
            adapter: Arc::new(Mutex::new(CausalStreamAdapter::new(receiver))),
            output_capacity: output_capacity.max(1),
        }
    }

    pub fn adapter(&self) -> Arc<Mutex<CausalStreamAdapter>> {
        Arc::clone(&self.adapter)
    }
}

fn status_for(error: CausalTransportError) -> Status {
    match error {
        CausalTransportError::DataPlane(DataPlaneError::StaleTerm { .. })
        | CausalTransportError::DataPlane(DataPlaneError::StaleGeneration { .. }) => {
            Status::failed_precondition(error.to_string())
        }
        CausalTransportError::Durability(crate::durability::DurabilityError::StaleTerm {
            ..
        })
        | CausalTransportError::Durability(crate::durability::DurabilityError::DataPlane(
            DataPlaneError::StaleTerm { .. } | DataPlaneError::StaleGeneration { .. },
        ))
        | CausalTransportError::Durability(crate::durability::DurabilityError::Corrupt(_)) => {
            Status::failed_precondition(error.to_string())
        }
        CausalTransportError::Durability(crate::durability::DurabilityError::Authority(_)) => {
            Status::failed_precondition(error.to_string())
        }
        _ => Status::invalid_argument(error.to_string()),
    }
}

#[tonic::async_trait]
impl proto::causal_data_plane_server::CausalDataPlane for CausalValidationService {
    type StreamEventsStream = ReceiverStream<Result<proto::CausalEventEnvelope, Status>>;

    async fn stream_events(
        &self,
        request: Request<tonic::Streaming<proto::CausalEventEnvelope>>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let mut inbound = request.into_inner();
        let (sender, receiver) = mpsc::channel(self.output_capacity);

        while let Some(frame) = inbound.message().await? {
            let accepted = {
                let sender_node_id = frame.sender_node_id.clone();
                let mut adapter = self
                    .adapter
                    .lock()
                    .map_err(|_| Status::internal("causal adapter lock poisoned"))?;
                let envelope = adapter.accept(frame).map_err(status_for)?.0;
                (envelope, sender_node_id)
            };
            sender
                .send(Ok(response_frame(&accepted.0, accepted.1)))
                .await
                .map_err(|_| Status::cancelled("causal response stream closed"))?;
        }

        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

#[tonic::async_trait]
impl proto::causal_data_plane_server::CausalDataPlane for DurableCausalService {
    type StreamEventsStream = ReceiverStream<Result<proto::CausalEventEnvelope, Status>>;

    async fn stream_events(
        &self,
        request: Request<tonic::Streaming<proto::CausalEventEnvelope>>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let mut inbound = request.into_inner();
        let (sender, receiver) = mpsc::channel(self.output_capacity);
        while let Some(frame) = inbound.message().await? {
            let accepted = {
                let sender_node_id = frame.sender_node_id.clone();
                let mut adapter = self
                    .adapter
                    .lock()
                    .map_err(|_| Status::internal("durable causal adapter lock poisoned"))?;
                let envelope = adapter
                    .accept_and_apply(frame, |_, event| {
                        Ok::<Vec<u8>, crate::durability::DurabilityError>(event.payload.clone())
                    })
                    .map_err(status_for)?
                    .0;
                (envelope, sender_node_id)
            };
            sender
                .send(Ok(response_frame(&accepted.0, accepted.1)))
                .await
                .map_err(|_| Status::cancelled("durable response stream closed"))?;
        }
        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

#[tonic::async_trait]
impl proto::causal_data_plane_server::CausalDataPlane for AuthoritativeCausalService {
    type StreamEventsStream = ReceiverStream<Result<proto::CausalEventEnvelope, Status>>;

    async fn stream_events(
        &self,
        request: Request<tonic::Streaming<proto::CausalEventEnvelope>>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let mut inbound = request.into_inner();
        let (sender, receiver) = mpsc::channel(self.output_capacity);
        while let Some(frame) = inbound.message().await? {
            let accepted = {
                let sender_node_id = frame.sender_node_id.clone();
                let envelope = CausalEnvelope::try_from(frame).map_err(status_for)?;
                let mut shard = self
                    .shard
                    .lock()
                    .map_err(|_| Status::internal("authoritative shard lock poisoned"))?;
                let channel_state = shard.channel_state().to_vec();
                if self.stable_biology {
                    shard
                        .apply_stable_event(&envelope, channel_state)
                        .map_err(|error| status_for(CausalTransportError::Durability(error)))?;
                } else {
                    shard
                        .apply(&envelope, channel_state, |_, event| {
                            Ok::<Vec<u8>, crate::durability::DurabilityError>(event.payload.clone())
                        })
                        .map_err(|error| status_for(CausalTransportError::Durability(error)))?;
                }
                (envelope, sender_node_id)
            };
            sender
                .send(Ok(response_frame(&accepted.0, accepted.1)))
                .await
                .map_err(|_| Status::cancelled("authoritative response stream closed"))?;
        }
        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::{
        BrainId, EventId, LeaseTerm, PartitionGeneration, RouteId, ShardId, StreamId,
        TopologyGeneration,
    };

    fn envelope() -> CausalEnvelope {
        CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain: BrainId::new(1).unwrap(),
            stream: StreamId::new(2).unwrap(),
            sequence: 0,
            lease_term: LeaseTerm::INITIAL,
            route: RouteId::new(3).unwrap(),
            partition_generation: PartitionGeneration::INITIAL,
            source: Some(NeuronId::new(6).unwrap()),
            target: Some(NeuronId::new(7).unwrap()),
            tag: LogicalTag::new(4, 0),
            event: EventId::new(5).unwrap(),
            stage: EventStage::FieldUpdate,
            kind: crate::data_plane::EnvelopeKind::Event,
            payload: vec![9, 8, 7],
            deferred_from_nonconvergence: true,
        }
    }

    #[test]
    fn generated_causal_frame_round_trips_stage_and_provenance() {
        let original = envelope();
        let wire = proto::CausalEventEnvelope::from(&original);
        let decoded = CausalEnvelope::try_from(wire).expect("valid causal frame");
        assert_eq!(decoded, original);
    }

    #[test]
    fn generated_causal_frame_preserves_optional_endpoints() {
        let original = envelope();
        let wire = proto::CausalEventEnvelope::from(&original);
        assert_eq!(wire.source_id, 6);
        assert_eq!(wire.target_id, 7);
        assert_eq!(CausalEnvelope::try_from(wire).unwrap(), original);
    }

    #[test]
    fn causal_adapter_validates_generated_frames_before_acceptance() {
        let original = envelope();
        let receiver = ReliableReceiver::new(
            original.brain,
            original.stream,
            original.lease_term,
            original.partition_generation,
            16,
        );
        let mut adapter = CausalStreamAdapter::new(receiver);
        let (accepted, result) = adapter
            .accept(proto::CausalEventEnvelope::from(&original))
            .expect("frame accepted");
        assert_eq!(accepted, original);
        assert_eq!(result, ReceiveResult::Accepted { sequence: 0 });
        let adapter_receipts = adapter.receipts();
        assert_eq!(adapter_receipts.len(), 1);
        assert!(adapter_receipts.contains(original.stream, original.sequence));

        let mut stale = proto::CausalEventEnvelope::from(&original);
        stale.lease_term = 2;
        assert!(matches!(
            adapter.accept(stale),
            Err(CausalTransportError::DataPlane(
                DataPlaneError::StaleTerm { .. }
            ))
        ));
    }

    #[test]
    fn causal_adapter_replays_exact_duplicates_without_creating_receipts() {
        let original = envelope();
        let receiver = ReliableReceiver::new(
            original.brain,
            original.stream,
            original.lease_term,
            original.partition_generation,
            16,
        );
        let mut adapter = CausalStreamAdapter::new(receiver);
        let wire = proto::CausalEventEnvelope::from(&original);
        assert_eq!(
            adapter.accept(wire.clone()).expect("first frame").1,
            ReceiveResult::Accepted { sequence: 0 }
        );
        assert_eq!(
            adapter.accept(wire).expect("replayed frame").1,
            ReceiveResult::Duplicate { sequence: 0 }
        );
        assert_eq!(adapter.receipts().len(), 1);
    }

    #[test]
    fn unknown_stage_and_missing_tag_fail_closed() {
        let original = envelope();
        let mut unknown = proto::CausalEventEnvelope::from(&original);
        unknown.stage = 99;
        assert!(matches!(
            CausalEnvelope::try_from(unknown),
            Err(CausalTransportError::UnsupportedStage(99))
        ));

        let mut missing = proto::CausalEventEnvelope::from(&original);
        missing.tag = None;
        assert!(matches!(
            CausalEnvelope::try_from(missing),
            Err(CausalTransportError::MissingTag)
        ));
    }

    #[test]
    fn durable_causal_adapter_commits_only_after_the_staged_transition() {
        let original = envelope();
        let shard = DurableShard::new(
            original.brain,
            ShardId::new(8).unwrap(),
            TopologyGeneration::INITIAL,
            original.partition_generation,
            original.lease_term,
            original.stream,
            16,
            vec![0],
            Vec::new(),
        );
        let mut adapter = DurableCausalStreamAdapter::new(shard);
        let wire = proto::CausalEventEnvelope::from(&original);
        assert!(matches!(
            adapter.accept_and_apply(wire.clone(), |_, _| {
                Err::<Vec<u8>, _>("biological transition rejected")
            }),
            Err(CausalTransportError::Durability(
                crate::durability::DurabilityError::Transition(_)
            ))
        ));
        assert_eq!(adapter.shard().durable_log_sequence(), None);

        let (_, result) = adapter
            .accept_and_apply(wire.clone(), |current, event| {
                Ok::<_, &'static str>(vec![current[0] + event.payload[0]])
            })
            .expect("durable apply");
        assert_eq!(result, DurableApplyOutcome::Applied { sequence: 0 });
        assert_eq!(adapter.shard().biological_state(), &[9]);

        let (_, result) = adapter
            .accept_and_apply(wire, |_, _| -> Result<Vec<u8>, &'static str> {
                panic!("duplicate must not enter the transition")
            })
            .expect("duplicate");
        assert_eq!(result, DurableApplyOutcome::Duplicate { sequence: 0 });
    }
}
