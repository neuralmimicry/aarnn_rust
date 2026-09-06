//! Placement-authorised dispatch for stable partial workers.
//!
//! This adapter is the control/data-plane join between the authoritative
//! placement registry, the durable per-destination outbox and the typed stable
//! shard transport. It never treats a resource observation as authority: a
//! record is dispatched only when the current registry still names the same
//! destination, fence, placement digest and topology generations that sealed
//! the record.
//!
//! Network flushes for independent destinations run concurrently. Filesystem
//! operations remain inside the bounded outbox API and are never held across a
//! network await. A failed destination leaves its records pending for a later
//! reconnect retry; successful destinations may already have been durably
//! acknowledged.

use crate::partial_shard_executor::PartialShardOutbound;
use crate::placement_registry::PlacementRegistry;
use crate::stable_outbound::{StableOutboundError, StableOutboundLog, StableOutboundRecord};
use crate::stable_shard_transport::{StableShardFlushError, flush_pending};
use futures_util::future::join_all;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

const MAX_ENDPOINTS: usize = 1024;
const MAX_BATCH_MESSAGES: usize = 4096;
const MAX_NODE_ID_BYTES: usize = 256;
const MAX_ENDPOINT_BYTES: usize = 2048;

#[derive(Debug, Error)]
pub enum StableShardDispatchError {
    #[error("stable-shard dispatcher source node identity is invalid")]
    InvalidSourceNode,
    #[error("stable-shard dispatcher endpoint identity is invalid")]
    InvalidEndpoint,
    #[error("stable-shard dispatcher placement lock is poisoned")]
    PlacementLock,
    #[error("stable-shard dispatcher endpoint lock is poisoned")]
    EndpointLock,
    #[error("stable-shard destination {0} has no configured endpoint")]
    MissingEndpoint(String),
    #[error(
        "stable-shard destination {destination} is not the current authority for shard {shard}"
    )]
    DestinationAuthorityMismatch { destination: String, shard: u64 },
    #[error("stable-shard record for shard {shard} has a stale physical placement digest")]
    PlacementDigestMismatch { shard: u64 },
    #[error("stable-shard record for shard {shard} has stale topology or partition generations")]
    GenerationMismatch { shard: u64 },
    #[error("stable-shard placement has no active plan for shard {0}")]
    MissingPlacement(u64),
    #[error("stable-shard outbound batch reached its bound {0}")]
    BatchTooLarge(usize),
    #[error(transparent)]
    Outbound(#[from] StableOutboundError),
    #[error(transparent)]
    Transport(#[from] StableShardFlushError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StableShardDispatchReport {
    pub attempted_destinations: usize,
    pub acknowledged_records: usize,
}

/// Async-safe dispatcher shared by an orchestrator worker loop and management
/// operations. Placement updates replace the registry value atomically under
/// the caller's control; this type only observes a cloned immutable snapshot.
#[derive(Clone)]
pub struct StableShardDispatcher {
    source_node_id: String,
    placement: Arc<RwLock<PlacementRegistry>>,
    outbox: Arc<tokio::sync::Mutex<StableOutboundLog>>,
    endpoints: Arc<RwLock<BTreeMap<String, String>>>,
}

impl std::fmt::Debug for StableShardDispatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StableShardDispatcher")
            .field("source_node_id", &self.source_node_id)
            .finish_non_exhaustive()
    }
}

impl StableShardDispatcher {
    pub fn new(
        source_node_id: impl Into<String>,
        placement: Arc<RwLock<PlacementRegistry>>,
        outbox: Arc<tokio::sync::Mutex<StableOutboundLog>>,
    ) -> Result<Self, StableShardDispatchError> {
        let source_node_id = source_node_id.into();
        validate_node_id(&source_node_id)?;
        // Fail early if the registry is already poisoned or malformed. The
        // registry's own verify method remains the authority for its contents.
        placement
            .read()
            .map_err(|_| StableShardDispatchError::PlacementLock)?
            .verify()
            .map_err(|_| StableShardDispatchError::PlacementLock)?;
        Ok(Self {
            source_node_id,
            placement,
            outbox,
            endpoints: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    /// Return the brain identity of the placement registry this dispatcher
    /// observes. Binding this identity at receiver registration prevents an
    /// otherwise valid queue for another brain from being attached to a
    /// receiver by configuration error.
    pub fn brain_id(&self) -> Result<crate::deterministic::BrainId, StableShardDispatchError> {
        Ok(self
            .placement
            .read()
            .map_err(|_| StableShardDispatchError::PlacementLock)?
            .brain_id)
    }

    pub fn register_endpoint(
        &self,
        node_id: impl Into<String>,
        address: impl Into<String>,
    ) -> Result<(), StableShardDispatchError> {
        let node_id = node_id.into();
        let address = address.into();
        validate_node_id(&node_id)?;
        if address.len() > MAX_ENDPOINT_BYTES
            || !(address.starts_with("http://") || address.starts_with("https://"))
        {
            return Err(StableShardDispatchError::InvalidEndpoint);
        }
        let mut endpoints = self
            .endpoints
            .write()
            .map_err(|_| StableShardDispatchError::EndpointLock)?;
        if !endpoints.contains_key(&node_id) && endpoints.len() >= MAX_ENDPOINTS {
            return Err(StableShardDispatchError::BatchTooLarge(MAX_ENDPOINTS));
        }
        endpoints.insert(node_id, address);
        Ok(())
    }

    pub fn remove_endpoint(&self, node_id: &str) -> Result<bool, StableShardDispatchError> {
        Ok(self
            .endpoints
            .write()
            .map_err(|_| StableShardDispatchError::EndpointLock)?
            .remove(node_id)
            .is_some())
    }

    /// Seal one typed outbound message against the current physical authority.
    pub async fn enqueue(
        &self,
        message: PartialShardOutbound,
    ) -> Result<StableOutboundRecord, StableShardDispatchError> {
        self.enqueue_batch([message])
            .await
            .map(|mut records| records.remove(0))
    }

    /// Seal a bounded set of messages. Earlier records remain durable if a
    /// later message fails validation; callers can inspect and retry them.
    pub async fn enqueue_batch<I>(
        &self,
        messages: I,
    ) -> Result<Vec<StableOutboundRecord>, StableShardDispatchError>
    where
        I: IntoIterator<Item = PartialShardOutbound>,
    {
        let messages = messages
            .into_iter()
            .take(MAX_BATCH_MESSAGES.saturating_add(1))
            .collect::<Vec<_>>();
        if messages.len() > MAX_BATCH_MESSAGES {
            return Err(StableShardDispatchError::BatchTooLarge(MAX_BATCH_MESSAGES));
        }
        let placement = self.placement_snapshot()?;
        let mut outbox = self.outbox.lock().await;
        Ok(outbox.append_for_shard_generation_bound_batch(&placement, messages)?)
    }

    /// Dispatch all currently pending destination streams concurrently.
    pub async fn dispatch_pending(
        &self,
    ) -> Result<StableShardDispatchReport, StableShardDispatchError> {
        let destinations = self.outbox.lock().await.destinations()?;
        let placement = self.placement_snapshot()?;
        let endpoints = self
            .endpoints
            .read()
            .map_err(|_| StableShardDispatchError::EndpointLock)?
            .clone();
        let mut jobs = Vec::new();
        for destination in destinations {
            let records = self.outbox.lock().await.pending(&destination)?;
            if records.is_empty() {
                continue;
            }
            validate_records(&placement, &destination, &records)?;
            let address = endpoints
                .get(&destination)
                .cloned()
                .ok_or_else(|| StableShardDispatchError::MissingEndpoint(destination.clone()))?;
            jobs.push((destination, address));
        }
        let attempted_destinations = jobs.len();
        let results = join_all(jobs.into_iter().map(|(destination, address)| {
            let outbox = Arc::clone(&self.outbox);
            let source = self.source_node_id.clone();
            async move {
                flush_pending(outbox, &destination, &source, &address)
                    .await
                    .map_err(StableShardDispatchError::from)
            }
        }))
        .await;
        let mut acknowledged_records = 0usize;
        for result in results {
            acknowledged_records = acknowledged_records.saturating_add(result?);
        }
        Ok(StableShardDispatchReport {
            attempted_destinations,
            acknowledged_records,
        })
    }

    /// Dispatch the current authority's destination for one shard. The
    /// destination stream may include sibling shards, so all records on that
    /// stream are validated before any network frame is sent.
    pub async fn dispatch_shard(
        &self,
        shard: crate::deterministic::ShardId,
    ) -> Result<usize, StableShardDispatchError> {
        let placement = self.placement_snapshot()?;
        let authority = placement
            .authority(shard)
            .ok_or(StableShardDispatchError::MissingPlacement(shard.raw()))?;
        let destination = authority.node_id.clone();
        let records = self.outbox.lock().await.pending(&destination)?;
        if records.is_empty() {
            return Ok(0);
        }
        validate_records(&placement, &destination, &records)?;
        let address = self
            .endpoints
            .read()
            .map_err(|_| StableShardDispatchError::EndpointLock)?
            .get(&destination)
            .cloned()
            .ok_or_else(|| StableShardDispatchError::MissingEndpoint(destination.clone()))?;
        Ok(flush_pending(
            Arc::clone(&self.outbox),
            &destination,
            &self.source_node_id,
            &address,
        )
        .await?)
    }

    fn placement_snapshot(&self) -> Result<PlacementRegistry, StableShardDispatchError> {
        Ok(self
            .placement
            .read()
            .map_err(|_| StableShardDispatchError::PlacementLock)?
            .clone())
    }
}

fn validate_node_id(value: &str) -> Result<(), StableShardDispatchError> {
    if value.trim().is_empty()
        || value.len() > MAX_NODE_ID_BYTES
        || value.contains(['/', '\\', '\0'])
    {
        return Err(StableShardDispatchError::InvalidSourceNode);
    }
    Ok(())
}

fn validate_records(
    placement: &PlacementRegistry,
    destination: &str,
    records: &[StableOutboundRecord],
) -> Result<(), StableShardDispatchError> {
    let Some(plan) = placement.active_plan.as_ref() else {
        return Err(StableShardDispatchError::MissingPlacement(
            records[0].destination_shard.raw(),
        ));
    };
    for record in records {
        let authority = placement.authority(record.destination_shard).ok_or(
            StableShardDispatchError::MissingPlacement(record.destination_shard.raw()),
        )?;
        if authority.node_id != destination || authority.node_id != record.destination_node {
            return Err(StableShardDispatchError::DestinationAuthorityMismatch {
                destination: destination.to_owned(),
                shard: record.destination_shard.raw(),
            });
        }
        if record.placement_plan_digest != plan.digest()
            || authority.plan_digest != record.placement_plan_digest
            || record.lease_term != authority.lease_term
            || record.fencing_token != authority.fencing_token
        {
            return Err(StableShardDispatchError::PlacementDigestMismatch {
                shard: record.destination_shard.raw(),
            });
        }
        if record.topology_generation != plan.topology_generation
            || record.partition_generation != plan.partition_generation
        {
            return Err(StableShardDispatchError::GenerationMismatch {
                shard: record.destination_shard.raw(),
            });
        }
    }
    Ok(())
}
