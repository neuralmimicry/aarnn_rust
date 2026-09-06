//! Authority-sensitive single-shard cutover coordination.
//!
//! Checkpoint transfer, quorum lease issuance and placement publication are
//! separate durable concerns. This coordinator provides the smallest safe
//! composition for one shard: it preflights the destination plan, promotes
//! through the majority-backed lease authority, verifies source fencing, then
//! probes and publishes the registry mutation with the returned cutover
//! evidence. A brain-wide operation can call this coordinator for each shard
//! while its journal owns the group barrier.

use crate::deterministic::StreamId;
use crate::deterministic::{LeaseTerm, LogicalTag, ShardId, StateDigest};
use crate::management::ReplicatedQuorumLeaseAuthority;
use crate::migration_group::{MigrationGroup, MigrationGroupError};
use crate::migration_transfer::{ImportedShardState, MigrationTransferError, QuorumPromotedShard};
use crate::placement_registry::{
    CutoverEvidence, PlacementApplyReceipt, PlacementApplyRequest, PlacementRegistry,
    PlacementRegistryError, ShardCutoverEvidence,
};
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuorumCutoverError {
    #[error(transparent)]
    Transfer(#[from] MigrationTransferError),
    #[error(transparent)]
    Placement(#[from] PlacementRegistryError),
    #[error("single-shard cutover request is invalid: {0}")]
    InvalidRequest(&'static str),
}

/// The promoted actor and the registry receipt form the durable handoff
/// evidence returned to the migration operation. The actor remains bound to
/// the same authority and therefore cannot silently continue under an old
/// lease after this value is dropped or persisted.
#[derive(Debug)]
pub struct QuorumCutoverOutcome {
    pub promoted: QuorumPromotedShard,
    pub receipt: PlacementApplyReceipt,
}

/// Result returned by one independent shard transfer. The transfer adapter is
/// responsible for checkpoint digest verification, WAL catch-up, route cursor
/// capture, effect deduplication cursor capture and destination fencing. The
/// brain coordinator only composes those facts into one publication barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainShardCutover {
    pub shard_id: ShardId,
    pub source_node: String,
    pub source_term: LeaseTerm,
    pub checkpoint_digest: StateDigest,
    pub cut_tag: LogicalTag,
    pub destination_term: LeaseTerm,
    pub route_cursor_digest: StateDigest,
    pub effect_cursor_digest: StateDigest,
}

impl BrainShardCutover {
    fn evidence(&self) -> ShardCutoverEvidence {
        ShardCutoverEvidence {
            source_node: self.source_node.clone(),
            source_term: self.source_term,
            checkpoint_digest: self.checkpoint_digest,
            caught_up: true,
            route_cursor_digest: self.route_cursor_digest,
            effect_cursor_digest: self.effect_cursor_digest,
        }
    }
}

#[derive(Debug, Error)]
pub enum BrainCutoverError {
    #[error(transparent)]
    Group(#[from] MigrationGroupError),
    #[error("parallel shard transfer failed for {shard}: {reason}")]
    Transfer { shard: ShardId, reason: String },
    #[error("parallel shard transfer returned duplicate or unexpected shard {0}")]
    UnexpectedShard(ShardId),
    #[error("brain cutover evidence is incomplete")]
    IncompleteEvidence,
    #[error("brain cutover evidence does not match the migration group")]
    EvidenceMismatch,
    #[error("brain cutover shards use different destination lease terms")]
    InconsistentDestinationTerm,
    #[error("brain cutover shards use different logical cut tags")]
    InconsistentCutTag,
    #[error(transparent)]
    Placement(#[from] PlacementRegistryError),
}

/// Evidence produced after all shard transfers have caught up and their old
/// writers have been fenced. It is intentionally not committed yet: the
/// caller must publish the complete placement generation atomically, then
/// call [`BrainMigrationCoordinator::finalize_after_publication`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBrainCutover {
    pub evidence: CutoverEvidence,
    pub shards: BTreeMap<ShardId, BrainShardCutover>,
}

/// Coordinates all affected shards without holding a control-plane lock over
/// checkpoint or network work. Transfer futures run concurrently; durable
/// group mutations are then applied in stable shard-ID order so audit replay
/// remains deterministic regardless of completion order.
pub struct BrainMigrationCoordinator;

impl BrainMigrationCoordinator {
    /// Compose already verified imported shard evidence into the same
    /// brain-wide barrier used by asynchronous transfers.  The import step is
    /// intentionally separate so a transport can receive frames in parallel
    /// and only this short deterministic section mutates the group.
    pub fn prepare_imported(
        group: &mut MigrationGroup,
        source_plan_digest: StateDigest,
        cutovers: BTreeMap<ShardId, BrainShardCutover>,
    ) -> Result<PreparedBrainCutover, BrainCutoverError> {
        if source_plan_digest == StateDigest([0; 16])
            || cutovers.len() != group.shards.len()
            || cutovers
                .keys()
                .any(|shard| !group.shards.contains_key(shard))
        {
            return Err(BrainCutoverError::EvidenceMismatch);
        }
        let leader_term = group.leader_term;
        let shard_ids = group.shards.keys().copied().collect::<Vec<_>>();
        for shard in shard_ids {
            group.begin_transfer(shard, leader_term)?;
        }
        let cut_tag = cutovers
            .values()
            .map(|cutover| cutover.cut_tag)
            .min()
            .ok_or(BrainCutoverError::IncompleteEvidence)?;
        let destination_term = cutovers
            .values()
            .map(|cutover| cutover.destination_term)
            .next()
            .ok_or(BrainCutoverError::IncompleteEvidence)?;
        if cutovers.values().any(|cutover| {
            cutover.cut_tag != cut_tag || cutover.destination_term != destination_term
        }) {
            let _ = group.abort(group.leader_term);
            return Err(BrainCutoverError::InconsistentDestinationTerm);
        }
        for (shard, cutover) in &cutovers {
            group.mark_caught_up(
                *shard,
                group.leader_term,
                cutover.checkpoint_digest,
                cutover.cut_tag,
                cutover.destination_term,
                cutover.route_cursor_digest,
                cutover.effect_cursor_digest,
            )?;
            group.mark_fenced(*shard, group.leader_term, cutover.destination_term)?;
        }
        let evidence = CutoverEvidence {
            operation_id: crate::deterministic::EventId::new(group.operation_id)
                .map_err(|_| BrainCutoverError::IncompleteEvidence)?,
            source_plan_digest,
            cut_tag,
            destination_term,
            shards: cutovers
                .iter()
                .map(|(shard, cutover)| (*shard, cutover.evidence()))
                .collect(),
        };
        evidence
            .verify()
            .map_err(|_| BrainCutoverError::EvidenceMismatch)?;
        Ok(PreparedBrainCutover {
            evidence,
            shards: cutovers,
        })
    }

    pub async fn prepare<I, F, Fut>(
        group: &mut MigrationGroup,
        source_plan_digest: StateDigest,
        shard_ids: I,
        transfer: F,
    ) -> Result<PreparedBrainCutover, BrainCutoverError>
    where
        I: IntoIterator<Item = ShardId>,
        F: Fn(ShardId) -> Fut + Send + Sync + Clone,
        Fut: Future<Output = Result<BrainShardCutover, String>> + Send,
    {
        if source_plan_digest == StateDigest([0; 16]) {
            return Err(BrainCutoverError::EvidenceMismatch);
        }
        let shard_ids = shard_ids.into_iter().collect::<BTreeSet<_>>();
        if shard_ids.is_empty()
            || shard_ids
                .iter()
                .any(|shard| !group.shards.contains_key(shard))
        {
            return Err(BrainCutoverError::IncompleteEvidence);
        }
        for shard in &shard_ids {
            group.begin_transfer(*shard, group.leader_term)?;
        }

        let mut futures = FuturesUnordered::new();
        for shard in shard_ids.iter().copied() {
            let transfer = transfer.clone();
            futures.push(async move { (shard, transfer(shard).await) });
        }
        let mut results = BTreeMap::new();
        while let Some((expected_shard, result)) = futures.next().await {
            let cutover = match result {
                Ok(cutover) => cutover,
                Err(reason) => {
                    let _ = group.abort(group.leader_term);
                    return Err(BrainCutoverError::Transfer {
                        shard: expected_shard,
                        reason,
                    });
                }
            };
            if cutover.shard_id != expected_shard
                || results.insert(cutover.shard_id, cutover).is_some()
            {
                let _ = group.abort(group.leader_term);
                return Err(BrainCutoverError::UnexpectedShard(expected_shard));
            }
        }
        if results.len() != shard_ids.len() {
            let _ = group.abort(group.leader_term);
            return Err(BrainCutoverError::IncompleteEvidence);
        }

        for shard in &shard_ids {
            let cutover = results.get(shard).expect("parallel results are complete");
            group.mark_caught_up(
                *shard,
                group.leader_term,
                cutover.checkpoint_digest,
                cutover.cut_tag,
                cutover.destination_term,
                cutover.route_cursor_digest,
                cutover.effect_cursor_digest,
            )?;
            group.mark_fenced(*shard, group.leader_term, cutover.destination_term)?;
        }

        let cut_tag = results
            .values()
            .map(|cutover| cutover.cut_tag)
            .min()
            .ok_or(BrainCutoverError::IncompleteEvidence)?;
        if results.values().any(|cutover| cutover.cut_tag != cut_tag) {
            let _ = group.abort(group.leader_term);
            return Err(BrainCutoverError::InconsistentCutTag);
        }
        let destination_term = results
            .values()
            .map(|cutover| cutover.destination_term)
            .next()
            .ok_or(BrainCutoverError::IncompleteEvidence)?;
        if results
            .values()
            .any(|cutover| cutover.destination_term != destination_term)
        {
            let _ = group.abort(group.leader_term);
            return Err(BrainCutoverError::InconsistentDestinationTerm);
        }
        let evidence = CutoverEvidence {
            operation_id: crate::deterministic::EventId::new(group.operation_id)
                .map_err(|_| BrainCutoverError::IncompleteEvidence)?,
            source_plan_digest,
            cut_tag,
            destination_term,
            shards: results
                .iter()
                .map(|(shard, cutover)| (*shard, cutover.evidence()))
                .collect(),
        };
        evidence
            .verify()
            .map_err(|_| BrainCutoverError::EvidenceMismatch)?;
        Ok(PreparedBrainCutover {
            evidence,
            shards: results,
        })
    }

    /// Publish the complete brain placement and then finish the durable group
    /// barrier. Both values are preflighted on clones first; a registry or
    /// barrier validation failure therefore leaves the caller's state intact.
    pub fn publish_and_finalize(
        group: &mut MigrationGroup,
        prepared: &PreparedBrainCutover,
        registry: &mut PlacementRegistry,
        mut placement: PlacementApplyRequest,
    ) -> Result<PlacementApplyReceipt, BrainCutoverError> {
        if placement.cutover.is_some() {
            return Err(BrainCutoverError::EvidenceMismatch);
        }
        placement.cutover = Some(prepared.evidence.clone());
        let mut group_probe = group.clone();
        Self::finalize_after_publication(&mut group_probe, prepared)?;
        let mut registry_probe = registry.clone();
        let _probe_receipt = registry_probe.apply(placement.clone())?;
        let plan_digest = placement.plan.digest();
        let receipt = registry.apply(placement)?;
        Self::finalize_after_publication(group, prepared)?;
        debug_assert_eq!(receipt.plan_digest, plan_digest);
        Ok(receipt)
    }

    /// Complete the group barrier only after the caller has successfully
    /// applied the complete placement generation and route redirection.
    pub fn finalize_after_publication(
        group: &mut MigrationGroup,
        prepared: &PreparedBrainCutover,
    ) -> Result<(), BrainCutoverError> {
        if prepared.shards.len() != group.shards.len()
            || prepared
                .shards
                .keys()
                .any(|shard| !group.shards.contains_key(shard))
        {
            return Err(BrainCutoverError::EvidenceMismatch);
        }
        for shard in prepared.shards.keys().copied() {
            group.mark_published(shard, group.leader_term)?;
        }
        group.commit(group.leader_term)?;
        Ok(())
    }
}

pub struct QuorumShardCutover;

impl QuorumShardCutover {
    #[allow(clippy::too_many_arguments)]
    pub fn promote_and_publish(
        imported: ImportedShardState,
        owner_path: impl Into<PathBuf>,
        warm_path: impl Into<PathBuf>,
        authority: &mut ReplicatedQuorumLeaseAuthority,
        registry: &mut PlacementRegistry,
        mut placement: PlacementApplyRequest,
        destination_node: impl Into<String>,
        operation_id: u64,
        source_fencing_token: u64,
        stream_id: StreamId,
        max_payload: usize,
    ) -> Result<QuorumCutoverOutcome, QuorumCutoverError> {
        let destination_node = destination_node.into();
        let shard_id = imported.manifest.shard_id;
        if imported.manifest.brain_id != placement.plan.brain_id {
            return Err(QuorumCutoverError::InvalidRequest(
                "transfer and placement brain IDs differ",
            ));
        }
        if placement.plan.placements.len() != 1 {
            return Err(QuorumCutoverError::InvalidRequest(
                "single-shard coordinator requires exactly one placement",
            ));
        }
        let Some(target) = placement
            .plan
            .placements
            .iter()
            .find(|placement| placement.shard_id == shard_id)
        else {
            return Err(QuorumCutoverError::InvalidRequest(
                "destination plan does not contain the transferred shard",
            ));
        };
        if target.active_node != destination_node {
            return Err(QuorumCutoverError::InvalidRequest(
                "destination plan owner differs from requested node",
            ));
        }
        if placement.cutover.is_some() {
            return Err(QuorumCutoverError::InvalidRequest(
                "coordinator owns cutover evidence construction",
            ));
        }

        // Validate the immutable placement/lease handoff before issuing a new
        // lease.  A placement plan is produced by the control plane and must
        // name the exact term that the quorum authority will issue for this
        // promotion.  Checking this first keeps a malformed plan from
        // fencing the source and then failing after the authority mutation.
        let expected_destination_term = authority
            .authority()
            .current_term()
            .raw()
            .checked_add(1)
            .and_then(|raw| LeaseTerm::new(raw).ok())
            .ok_or(QuorumCutoverError::InvalidRequest(
                "quorum lease term is exhausted",
            ))?;
        if placement.plan.lease_term != expected_destination_term
            || placement.plan.fencing_token != expected_destination_term.raw()
        {
            return Err(QuorumCutoverError::InvalidRequest(
                "destination plan term or fencing token does not match the next quorum lease",
            ));
        }

        let promoted = imported.promote_with_quorum(
            owner_path,
            warm_path,
            authority,
            destination_node,
            operation_id,
            source_fencing_token,
            stream_id,
            max_payload,
        )?;
        if placement.plan.lease_term != promoted.lease.term
            || placement.plan.fencing_token != promoted.lease.fencing_token
        {
            return Err(QuorumCutoverError::InvalidRequest(
                "destination plan term or fencing token differs from quorum lease",
            ));
        }
        placement.cutover = Some(promoted.cutover.clone());

        // Probe against a clone before mutating the caller's registry. This
        // catches stale plan/version/evidence conflicts after transfer while
        // the promoted actor is still available for journal recovery.
        let mut probe = registry.clone();
        probe.apply(placement.clone())?;
        let receipt = registry.apply(placement)?;
        Ok(QuorumCutoverOutcome { promoted, receipt })
    }
}

#[cfg(test)]
mod brain_tests {
    use super::*;
    use crate::migration_group::MigrationGroupSpec;

    fn group() -> MigrationGroup {
        MigrationGroupSpec {
            brain_id: crate::deterministic::BrainId::new(7).unwrap(),
            leader_term: LeaseTerm::INITIAL,
            topology_generation: crate::deterministic::TopologyGeneration::INITIAL,
            partition_generation: crate::deterministic::PartitionGeneration::INITIAL,
            shard_ids: vec![ShardId::new(1).unwrap(), ShardId::new(2).unwrap()],
        }
        .build(9)
        .unwrap()
    }

    fn cutover(shard_id: ShardId) -> BrainShardCutover {
        BrainShardCutover {
            shard_id,
            source_node: format!("source-{shard_id}"),
            source_term: LeaseTerm::INITIAL,
            checkpoint_digest: StateDigest([shard_id.raw() as u8; 16]),
            cut_tag: LogicalTag::ZERO,
            destination_term: LeaseTerm::new(2).unwrap(),
            route_cursor_digest: StateDigest([3; 16]),
            effect_cursor_digest: StateDigest([4; 16]),
        }
    }

    #[tokio::test]
    async fn transfers_run_in_parallel_and_finalize_only_after_publication() {
        let mut group = group();
        let prepared = BrainMigrationCoordinator::prepare(
            &mut group,
            StateDigest([8; 16]),
            [ShardId::new(1).unwrap(), ShardId::new(2).unwrap()],
            |shard| async move { Ok(cutover(shard)) },
        )
        .await
        .unwrap();
        assert_eq!(
            group.phase,
            crate::migration_group::MigrationGroupPhase::Transferring
        );
        assert!(
            BrainMigrationCoordinator::finalize_after_publication(&mut group, &prepared).is_ok()
        );
        assert_eq!(
            group.phase,
            crate::migration_group::MigrationGroupPhase::Committed
        );
    }

    #[tokio::test]
    async fn one_failed_transfer_aborts_without_partial_publication() {
        let mut group = group();
        let result = BrainMigrationCoordinator::prepare(
            &mut group,
            StateDigest([8; 16]),
            [ShardId::new(1).unwrap(), ShardId::new(2).unwrap()],
            |shard| async move {
                if shard == ShardId::new(2).unwrap() {
                    Err("destination unavailable".to_owned())
                } else {
                    Ok(cutover(shard))
                }
            },
        )
        .await;
        assert!(matches!(result, Err(BrainCutoverError::Transfer { .. })));
        assert_eq!(
            group.phase,
            crate::migration_group::MigrationGroupPhase::Aborted
        );
        assert!(
            group
                .shards
                .values()
                .all(|shard| shard.phase == crate::migration_group::ShardMigrationPhase::Aborted)
        );
    }

    #[tokio::test]
    async fn mixed_destination_terms_are_rejected_before_publication() {
        let mut group = group();
        let result = BrainMigrationCoordinator::prepare(
            &mut group,
            StateDigest([8; 16]),
            [ShardId::new(1).unwrap(), ShardId::new(2).unwrap()],
            |shard| async move {
                let mut result = cutover(shard);
                if shard == ShardId::new(2).unwrap() {
                    result.destination_term = LeaseTerm::new(3).unwrap();
                }
                Ok(result)
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(BrainCutoverError::InconsistentDestinationTerm)
        ));
        assert_eq!(
            group.phase,
            crate::migration_group::MigrationGroupPhase::Aborted
        );
    }
}
