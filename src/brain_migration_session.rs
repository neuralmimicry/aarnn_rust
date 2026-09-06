//! Whole-brain migration session for the stable reference executor.
//!
//! [`crate::stable_executor_durable::StableExecutorDurableBridge`] owns the
//! source cut and can expose one bounded [`ShardTransferSource`] per virtual
//! shard.  This module composes those sources into one brain-wide operation:
//! frames are verified and reassembled, every destination actor is restored
//! under a newer writer term, the existing group barrier records the per-shard
//! facts, and the placement registry is published only after all evidence is
//! complete.
//!
//! The session deliberately does not pretend that local files are a network
//! quorum.  A production adapter must issue the destination leases and bind
//! the actors to the replicated authority before exposing them to traffic.
//! The reference session nevertheless exercises the complete state-transfer,
//! digest, fencing-order and atomic-publication seam without a live network.

use crate::authoritative_shard::{AuthoritativeShard, ShardState};
use crate::deterministic::{LeaseTerm, ShardId, StateDigest, StreamId};
use crate::management::{LeasePromotionRequest, ReplicatedQuorumLeaseAuthority};
use crate::migration_coordinator::{
    BrainCutoverError, BrainMigrationCoordinator, BrainShardCutover, PreparedBrainCutover,
};
use crate::migration_group::MigrationGroup;
use crate::migration_operation::PersistedMigrationJournal;
use crate::migration_operation::{MigrationJournal, MigrationOperation, MigrationOperationError};
use crate::migration_transfer::{
    MigrationTransferError, ShardCatchUpBatch, ShardTransferReceiver, ShardTransferSource,
};
use crate::placement_registry::{
    PersistedPlacementRegistry, PlacementApplyReceipt, PlacementApplyRequest, PlacementRegistry,
    PlacementRegistryError,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Materialised destination actors and the complete evidence produced for the
/// brain-wide cut.  The actors are returned only after registry publication so
/// callers cannot accidentally serve an uncommitted placement.
#[derive(Debug)]
pub struct PreparedBrainMigration {
    pub cutover: PreparedBrainCutover,
    pub destinations: BTreeMap<ShardId, AuthoritativeShard>,
    pub transferred_bytes: u64,
}

#[derive(Debug)]
pub struct BrainMigrationOutcome {
    pub receipt: PlacementApplyReceipt,
    pub operation: Option<MigrationOperation>,
    /// Complete barrier returned by the data-plane cutover.  Management uses
    /// this immutable evidence to finalise its own journal after the registry
    /// publication succeeds.
    pub group: MigrationGroup,
    pub destinations: BTreeMap<ShardId, AuthoritativeShard>,
}

#[derive(Debug, Error)]
pub enum BrainMigrationSessionError {
    #[error(transparent)]
    Transfer(#[from] MigrationTransferError),
    #[error(transparent)]
    Cutover(#[from] BrainCutoverError),
    #[error(transparent)]
    Placement(#[from] PlacementRegistryError),
    #[error(transparent)]
    Journal(#[from] MigrationOperationError),
    #[error("transfer source set does not match the migration group")]
    SourceSetMismatch,
    #[error("transfer source {0} has a different source placement digest")]
    SourcePlanMismatch(ShardId),
    #[error("destination term must be newer than the migration group's leader term")]
    DestinationTermNotNewer,
    #[error("destination actor term does not match the requested destination term")]
    DestinationTermMismatch,
    #[error("destination shard path is invalid")]
    DestinationPath,
}

/// Reference whole-brain migration orchestrator.
pub struct BrainMigrationSession;

impl BrainMigrationSession {
    /// Receive and materialise all sources from one stable executor cut.
    ///
    /// Frames are accepted in reverse order to exercise the same bounded,
    /// digest-verified receiver used by a real transport adapter. The method
    /// performs no placement publication and therefore remains safe to retry
    /// or abandon before the caller commits the cutover.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_from_sources(
        group: &mut MigrationGroup,
        source_plan_digest: StateDigest,
        sources: impl IntoIterator<Item = ShardTransferSource>,
        destination_root: impl Into<PathBuf>,
        warm_root: impl Into<PathBuf>,
        destination_term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<PreparedBrainMigration, BrainMigrationSessionError> {
        if destination_term <= group.leader_term {
            return Err(BrainMigrationSessionError::DestinationTermNotNewer);
        }
        let expected_shards = group
            .shards
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let sources = sources.into_iter().collect::<Vec<_>>();
        let (imported, transferred_bytes) = receive_sources_parallel(sources, source_plan_digest)?;
        if imported
            .keys()
            .any(|shard| !expected_shards.contains(shard))
            || imported
                .values()
                .any(|state| state.manifest.brain_id != group.brain_id)
        {
            return Err(BrainMigrationSessionError::SourceSetMismatch);
        }
        if imported.len() != expected_shards.len() {
            return Err(BrainMigrationSessionError::SourceSetMismatch);
        }

        let destination_root = destination_root.into();
        let warm_root = warm_root.into();
        let mut destinations = BTreeMap::new();
        for (shard_id, state) in &imported {
            let owner_path = destination_path(&destination_root, *shard_id, "owner")?;
            let warm_path = destination_path(&warm_root, *shard_id, "warm")?;
            let actor = state.clone().promote_into_authoritative(
                owner_path,
                warm_path,
                destination_term,
                stream_id,
                max_payload,
            )?;
            if actor.term() != destination_term || actor.shard_id() != *shard_id {
                return Err(BrainMigrationSessionError::DestinationTermMismatch);
            }
            destinations.insert(*shard_id, actor);
        }

        let mut group_clone = group.clone();
        let mut cutovers = BTreeMap::new();
        for (shard_id, state) in &imported {
            let evidence = state.cutover_evidence(group.operation_id, destination_term)?;
            let shard_evidence = evidence
                .shards
                .get(shard_id)
                .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
            cutovers.insert(
                *shard_id,
                BrainShardCutover {
                    shard_id: *shard_id,
                    source_node: shard_evidence.source_node.clone(),
                    source_term: shard_evidence.source_term,
                    checkpoint_digest: shard_evidence.checkpoint_digest,
                    cut_tag: evidence.cut_tag,
                    destination_term,
                    route_cursor_digest: shard_evidence.route_cursor_digest,
                    effect_cursor_digest: shard_evidence.effect_cursor_digest,
                },
            );
        }
        let prepared = BrainMigrationCoordinator::prepare_imported(
            &mut group_clone,
            source_plan_digest,
            cutovers,
        )?;
        *group = group_clone;
        Ok(PreparedBrainMigration {
            cutover: prepared,
            destinations,
            transferred_bytes,
        })
    }

    /// Prepare a brain cut with one quorum-backed destination term and bind
    /// every materialised actor to the same replicated fencing authority.
    /// Lease issuance is a single authority transaction; a per-shard lease
    /// loop would create different terms and could expose a mixed generation.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_from_sources_with_quorum(
        group: &mut MigrationGroup,
        source_plan_digest: StateDigest,
        sources: impl IntoIterator<Item = ShardTransferSource>,
        destination_root: impl Into<PathBuf>,
        warm_root: impl Into<PathBuf>,
        authority: &mut ReplicatedQuorumLeaseAuthority,
        destination_nodes: BTreeMap<ShardId, String>,
        source_fencing_tokens: BTreeMap<ShardId, u64>,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<PreparedBrainMigration, BrainMigrationSessionError> {
        Self::prepare_from_sources_with_quorum_and_catch_up(
            group,
            source_plan_digest,
            sources,
            destination_root,
            warm_root,
            authority,
            destination_nodes,
            source_fencing_tokens,
            stream_id,
            max_payload,
            BTreeMap::new(),
        )
    }

    /// Prepare a quorum-backed destination from a checkpoint plus one
    /// source-drain WAL tail per shard. The initial source transfer is still
    /// verified as an immutable cut; the tail is applied only after the
    /// destination actor has been opened and before its cutover evidence is
    /// assembled. Publishing placement therefore cannot skip committed work
    /// that was drained after the first checkpoint was captured.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_from_sources_with_quorum_and_catch_up(
        group: &mut MigrationGroup,
        source_plan_digest: StateDigest,
        sources: impl IntoIterator<Item = ShardTransferSource>,
        destination_root: impl Into<PathBuf>,
        warm_root: impl Into<PathBuf>,
        authority: &mut ReplicatedQuorumLeaseAuthority,
        destination_nodes: BTreeMap<ShardId, String>,
        source_fencing_tokens: BTreeMap<ShardId, u64>,
        stream_id: StreamId,
        max_payload: usize,
        catch_up: BTreeMap<ShardId, (ShardCatchUpBatch, ShardState)>,
    ) -> Result<PreparedBrainMigration, BrainMigrationSessionError> {
        let expected_shards = group.shards.keys().copied().collect::<Vec<_>>();
        let (imported, transferred_bytes) =
            receive_sources_parallel(sources.into_iter().collect(), source_plan_digest)?;
        if imported.len() != expected_shards.len()
            || imported.keys().copied().collect::<Vec<_>>() != expected_shards
            || imported
                .values()
                .any(|state| state.manifest.brain_id != group.brain_id)
            || destination_nodes.len() != expected_shards.len()
            || source_fencing_tokens.len() != expected_shards.len()
        {
            return Err(BrainMigrationSessionError::SourceSetMismatch);
        }
        if !catch_up.is_empty() && catch_up.keys().copied().collect::<Vec<_>>() != expected_shards {
            return Err(BrainMigrationSessionError::SourceSetMismatch);
        }
        let promotion_requests = expected_shards
            .iter()
            .map(|shard_id| {
                let imported = imported
                    .get(shard_id)
                    .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
                let destination_node = destination_nodes
                    .get(shard_id)
                    .cloned()
                    .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
                let source_fencing_token = source_fencing_tokens
                    .get(shard_id)
                    .copied()
                    .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
                Ok(LeasePromotionRequest {
                    shard_id: *shard_id,
                    source_node: imported.manifest.source_node.clone(),
                    source_term: imported.manifest.source_term,
                    source_fencing_token,
                    destination_node,
                })
            })
            .collect::<Result<Vec<_>, BrainMigrationSessionError>>()?;
        let leases = authority
            .promote_leases(promotion_requests)
            .map_err(|error| {
                BrainMigrationSessionError::Transfer(MigrationTransferError::Authority(
                    error.to_string(),
                ))
            })?;
        let destination_root = destination_root.into();
        let warm_root = warm_root.into();
        let (replicas, members) = authority.replica_binding();
        let mut destinations = BTreeMap::new();
        let mut cutovers = BTreeMap::new();
        for (shard_id, imported) in imported {
            let lease = leases
                .get(&shard_id)
                .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
            let mut actor = match imported.clone().promote_into_authoritative(
                destination_path(&destination_root, shard_id, "owner")?,
                destination_path(&warm_root, shard_id, "warm")?,
                lease.term,
                stream_id,
                max_payload,
            ) {
                Ok(actor) => actor,
                Err(error) => {
                    let _ = authority.revoke_leases(leases.keys().copied().collect());
                    return Err(BrainMigrationSessionError::Transfer(error));
                }
            };
            let final_state = catch_up.get(&shard_id).map(|(_, state)| state);
            if let Some((batch, _)) = catch_up.get(&shard_id) {
                if let Err(error) = batch.apply_to_authoritative_with_final_state(
                    &imported.manifest,
                    &mut actor,
                    lease.term,
                    final_state.expect("catch-up state exists with its batch"),
                ) {
                    let _ = authority.revoke_leases(leases.keys().copied().collect());
                    return Err(BrainMigrationSessionError::Transfer(error));
                }
            }
            actor.bind_replicated_fencing(
                replicas.clone(),
                members.clone(),
                lease.node_id.clone(),
                lease.fencing_token,
            );
            let evidence = match final_state {
                Some(final_state) => imported.cutover_evidence_after_catch_up(
                    final_state,
                    group.operation_id,
                    lease.term,
                )?,
                None => imported.cutover_evidence(group.operation_id, lease.term)?,
            };
            let shard_evidence = evidence
                .shards
                .get(&shard_id)
                .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
            cutovers.insert(
                shard_id,
                BrainShardCutover {
                    shard_id,
                    source_node: shard_evidence.source_node.clone(),
                    source_term: shard_evidence.source_term,
                    checkpoint_digest: shard_evidence.checkpoint_digest,
                    cut_tag: evidence.cut_tag,
                    destination_term: lease.term,
                    route_cursor_digest: shard_evidence.route_cursor_digest,
                    effect_cursor_digest: shard_evidence.effect_cursor_digest,
                },
            );
            destinations.insert(shard_id, actor);
        }
        let mut group_clone = group.clone();
        let prepared = BrainMigrationCoordinator::prepare_imported(
            &mut group_clone,
            source_plan_digest,
            cutovers,
        )?;
        *group = group_clone;
        Ok(PreparedBrainMigration {
            cutover: prepared,
            destinations,
            transferred_bytes,
        })
    }

    /// Publish the complete placement and finish the group barrier. If either
    /// preflight fails, neither caller-owned object is changed.
    pub fn publish_and_finalize(
        group: &mut MigrationGroup,
        prepared: PreparedBrainMigration,
        registry: &mut PlacementRegistry,
        placement: PlacementApplyRequest,
        mut journal: Option<(&mut MigrationJournal, u64)>,
    ) -> Result<BrainMigrationOutcome, BrainMigrationSessionError> {
        let receipt = BrainMigrationCoordinator::publish_and_finalize(
            group,
            &prepared.cutover,
            registry,
            placement,
        )?;
        let operation = if let Some((journal, expected_resource_version)) = journal.as_mut() {
            Some(journal.commit_prepared_group(
                group,
                receipt.cut_tag,
                prepared.transferred_bytes,
                *expected_resource_version,
            )?)
        } else {
            None
        };
        Ok(BrainMigrationOutcome {
            receipt,
            operation,
            group: group.clone(),
            destinations: prepared.destinations,
        })
    }

    /// Persist the registry publication and journal commit using their
    /// crash-safe adapters. The registry is published first; if journal
    /// publication fails, reopening the journal leaves the operation in its
    /// recoverable cutover-ready state while the already published registry
    /// remains idempotently visible.
    pub fn publish_and_finalize_persisted(
        group: &mut MigrationGroup,
        prepared: PreparedBrainMigration,
        registry: &mut PersistedPlacementRegistry,
        mut placement: PlacementApplyRequest,
        mut journal: Option<(&mut PersistedMigrationJournal, u64)>,
    ) -> Result<BrainMigrationOutcome, BrainMigrationSessionError> {
        if placement.cutover.is_some() {
            return Err(BrainMigrationSessionError::SourceSetMismatch);
        }
        placement.cutover = Some(prepared.cutover.evidence.clone());
        let mut group_probe = group.clone();
        BrainMigrationCoordinator::finalize_after_publication(&mut group_probe, &prepared.cutover)?;
        let mut registry_probe = registry.state().clone();
        registry_probe.apply(placement.clone())?;
        let receipt = registry.apply(placement)?;
        BrainMigrationCoordinator::finalize_after_publication(group, &prepared.cutover)?;
        let operation = if let Some((journal, expected_resource_version)) = journal.as_mut() {
            Some(journal.commit_prepared_group(
                group,
                receipt.cut_tag,
                prepared.transferred_bytes,
                *expected_resource_version,
            )?)
        } else {
            None
        };
        Ok(BrainMigrationOutcome {
            receipt,
            operation,
            group: group.clone(),
            destinations: prepared.destinations,
        })
    }
}

/// Receive transfer frames concurrently, while retaining deterministic
/// insertion and validation order in the returned map. Transfer failures are
/// collected only after all bounded workers have joined, so no worker can be
/// left holding a source buffer or destination file handle.
fn receive_sources_parallel(
    sources: Vec<ShardTransferSource>,
    source_plan_digest: StateDigest,
) -> Result<
    (
        BTreeMap<ShardId, crate::migration_transfer::ImportedShardState>,
        u64,
    ),
    BrainMigrationSessionError,
> {
    let results = std::thread::scope(|scope| {
        let handles = sources
            .into_iter()
            .map(|source| {
                scope.spawn(move || {
                    let manifest = source.manifest().clone();
                    if manifest.source_plan_digest != source_plan_digest {
                        return Err(BrainMigrationSessionError::SourcePlanMismatch(
                            manifest.shard_id,
                        ));
                    }
                    let mut receiver = ShardTransferReceiver::new(manifest.clone())?;
                    let mut frames = source.frames()?;
                    frames.reverse();
                    for frame in frames {
                        receiver.accept(frame)?;
                    }
                    receiver
                        .finalize()
                        .map(|state| (manifest.shard_id, manifest.total_bytes, state))
                        .map_err(BrainMigrationSessionError::from)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| BrainMigrationSessionError::SourceSetMismatch)?
            })
            .collect::<Result<Vec<_>, BrainMigrationSessionError>>()
    })?;
    let mut imported = BTreeMap::new();
    let mut transferred_bytes = 0u64;
    for (shard_id, bytes, state) in results {
        if imported.insert(shard_id, state).is_some() {
            return Err(BrainMigrationSessionError::SourceSetMismatch);
        }
        transferred_bytes = transferred_bytes
            .checked_add(bytes)
            .ok_or(BrainMigrationSessionError::SourceSetMismatch)?;
    }
    Ok((imported, transferred_bytes))
}

fn destination_path(
    root: &Path,
    shard_id: ShardId,
    kind: &str,
) -> Result<PathBuf, BrainMigrationSessionError> {
    if kind != "owner" && kind != "warm" {
        return Err(BrainMigrationSessionError::DestinationPath);
    }
    Ok(root.join(format!("shard-{}.{}.json", shard_id.raw(), kind)))
}
