//! Additive generated-gRPC seam for causal event transport.
//!
//! This module deliberately stops at wire conversion and bounded receiver
//! validation.  It does not select shard ownership or replace the legacy
//! `StreamSpikes` service.  Those actions require the Phase 3/6 integration
//! and migration gates.

use crate::data_plane::{CausalEnvelope, DataPlaneError, ReceiveResult, ReliableReceiver};
use crate::deterministic::{EventStage, LogicalTag, NeuronId, SchemaVersion};
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
        }
    }
}

/// Typed boundary used by the generated gRPC validation service.
/// Conversion and receiver checks happen before application-owned queue state
/// can be mutated.
#[derive(Debug)]
pub struct CausalStreamAdapter {
    receiver: ReliableReceiver,
}

impl CausalStreamAdapter {
    pub fn new(receiver: ReliableReceiver) -> Self {
        Self { receiver }
    }

    pub fn accept(
        &mut self,
        frame: proto::CausalEventEnvelope,
    ) -> Result<(CausalEnvelope, ReceiveResult), CausalTransportError> {
        let envelope = CausalEnvelope::try_from(frame)?;
        let result = self.receiver.accept(&envelope)?;
        Ok((envelope, result))
    }

    pub fn receiver(&self) -> &ReliableReceiver {
        &self.receiver
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
                let mut adapter = self
                    .adapter
                    .lock()
                    .map_err(|_| Status::internal("causal adapter lock poisoned"))?;
                adapter.accept(frame).map_err(status_for)?.0
            };
            sender
                .send(Ok(proto::CausalEventEnvelope::from(&accepted)))
                .await
                .map_err(|_| Status::cancelled("causal response stream closed"))?;
        }

        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::{
        BrainId, EventId, LeaseTerm, PartitionGeneration, RouteId, StreamId,
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
}
