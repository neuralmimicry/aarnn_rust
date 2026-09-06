//! Durable handoff for the stable multi-shard reference executor.
//!
//! [`StableExecutorAuthority`](crate::stable_executor_authority::StableExecutorAuthority)
//! commits a complete immutable fabric cut. This module connects that commit
//! point to one [`AuthoritativeShard`](crate::authoritative_shard::AuthoritativeShard)
//! per virtual shard. The executor remains the owner of deterministic neural
//! execution; the actors provide durable WAL, receipt and warm-replica
//! publication for the resulting shard checkpoints.
//!
//! Mirror publication is deliberately resumable rather than pretending that a
//! sequence of independent files is one consensus transaction. If a mirror
//! fails after the complete fabric cut has been published, the bridge retains
//! the pending operation and retries it with the same envelope and expected
//! pre-cut digests. Already published mirrors return durable duplicates. A
//! production implementation must replace this local coordination with the
//! networked quorum and complete-cut protocol required by the specification.

use crate::authoritative_shard::{AuthoritativeShard, ShardState};
use crate::checkpoint_transfer::CheckpointTransferSource;
use crate::consistent_cut::ConsistentCut;
use crate::data_plane::{CausalEnvelope, EnvelopeKind};
use crate::deterministic::{
    EventId, LeaseTerm, RouteId, SchemaVersion, ShardId, StateDigest, StreamId,
};
use crate::durability::{DurabilityError, DurableApplyOutcome};
use crate::migration_transfer::{MigrationTransferError, ShardTransferSource};
use crate::shard_executor::{
    RoutedCausalEvent, ShardExecutionError, ShardExecutionResult, StableShardCheckpoint,
    StableShardExecutor,
};
use crate::stable_executor_authority::{StableExecutorAuthority, StableExecutorAuthorityError};
use crate::stable_executor_store::StableExecutorCheckpointStore;
use crate::stable_worker::StableShardApplicationAck;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StableExecutorDurableError {
    #[error(transparent)]
    Authority(#[from] StableExecutorAuthorityError),
    #[error(transparent)]
    Durability(#[from] DurabilityError),
    #[error(transparent)]
    Execution(#[from] ShardExecutionError),
    #[error("stable executor durable bridge encoding failed: {0}")]
    Encoding(String),
    #[error("stable executor durable bridge has no actor for shard {0}")]
    MissingShard(ShardId),
    #[error("stable executor durable mirror update is pending for event {event}")]
    MirrorPending { event: EventId },
    #[error("stable executor durable mirror operation has no pending cut")]
    NoPendingMirror,
    #[error("stable executor source is draining for migration")]
    MigrationDraining,
    #[error("stable executor migration drain did not reach a bounded empty frontier")]
    MigrationDrainLimit,
    #[error(transparent)]
    Transfer(#[from] MigrationTransferError),
    #[error(transparent)]
    CheckpointTransfer(#[from] crate::checkpoint_transfer::CheckpointTransferError),
}

#[derive(Debug, Clone)]
struct PendingMirror {
    envelope: CausalEnvelope,
    expected_actor_digests: BTreeMap<ShardId, StateDigest>,
    checkpoints: BTreeMap<ShardId, StableShardCheckpoint>,
    channel_state: Vec<u8>,
    advance_internal_sequence: bool,
}

/// Durable progress reconstructed from the actor receipt ledgers for one
/// external producer stream. Every actor receives the same mirror envelope;
/// the bridge verifies that their receipt prefixes agree before exposing this
/// cursor to a reconnecting transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableStreamProgress {
    pub next_sequence: u64,
    pub entries: BTreeMap<u64, StableStreamReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableStreamReceipt {
    pub event_id: EventId,
    pub payload_digest: StateDigest,
}

/// A complete stable executor with durable per-shard mirror actors.
#[derive(Debug)]
pub struct StableExecutorDurableBridge {
    authority: StableExecutorAuthority,
    actors: BTreeMap<ShardId, AuthoritativeShard>,
    stream_id: StreamId,
    max_payload: usize,
    next_sequence: u64,
    pending_mirror: Option<PendingMirror>,
    owner_root: PathBuf,
    warm_root: PathBuf,
    migration_draining: bool,
}

impl StableExecutorDurableBridge {
    /// Create a bridge and publish its initial complete fabric cut before
    /// exposing any actor as a durable mirror.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        executor: StableShardExecutor,
        store: StableExecutorCheckpointStore,
        term: LeaseTerm,
        fencing_token: u64,
        initial_checkpoint_id: EventId,
        owner_root: impl Into<PathBuf>,
        warm_root: impl Into<PathBuf>,
        stream_id: StreamId,
        max_payload: usize,
        initial_channel_state: Vec<u8>,
    ) -> Result<Self, StableExecutorDurableError> {
        if max_payload == 0 {
            return Err(StableExecutorDurableError::Encoding(
                "durable actor payload bound must be positive".to_owned(),
            ));
        }
        let owner_root = owner_root.into();
        let warm_root = warm_root.into();
        let mut authority = StableExecutorAuthority::new(executor, store, term, fencing_token);
        authority.checkpoint(term, fencing_token, initial_checkpoint_id)?;
        let checkpoints = authority.executor().checkpoint_shards()?;
        let actors = Self::open_actors(
            &authority,
            checkpoints,
            &owner_root,
            &warm_root,
            term,
            stream_id,
            max_payload,
            &initial_channel_state,
        )?;
        Ok(Self {
            authority,
            actors,
            stream_id,
            max_payload,
            next_sequence: 0,
            pending_mirror: None,
            owner_root,
            warm_root,
            migration_draining: false,
        })
    }

    /// Reopen a bridge from an executor restored from a published complete
    /// cut. No checkpoint is republished and no biological transition is
    /// performed. Existing durable actor files must match the restored cut
    /// byte-for-byte, otherwise recovery fails closed.
    #[allow(clippy::too_many_arguments)]
    pub fn open_existing(
        executor: StableShardExecutor,
        store: StableExecutorCheckpointStore,
        term: LeaseTerm,
        fencing_token: u64,
        owner_root: impl Into<PathBuf>,
        warm_root: impl Into<PathBuf>,
        stream_id: StreamId,
        max_payload: usize,
        channel_state: Vec<u8>,
    ) -> Result<Self, StableExecutorDurableError> {
        if max_payload == 0 {
            return Err(StableExecutorDurableError::Encoding(
                "durable actor payload bound must be positive".to_owned(),
            ));
        }
        let owner_root = owner_root.into();
        let warm_root = warm_root.into();
        let authority = StableExecutorAuthority::new(executor, store, term, fencing_token);
        let checkpoints = authority.executor().checkpoint_shards()?;
        let actors = Self::open_actors(
            &authority,
            checkpoints,
            &owner_root,
            &warm_root,
            term,
            stream_id,
            max_payload,
            &channel_state,
        )?;
        let next_sequence = actors
            .values()
            .filter_map(|actor| {
                actor
                    .stream_receipts(stream_id)
                    .into_iter()
                    .map(|receipt| receipt.sequence)
                    .max()
            })
            .max()
            .map(|sequence| {
                sequence.checked_add(1).ok_or_else(|| {
                    StableExecutorDurableError::Encoding(
                        "durable mirror sequence exhausted".to_owned(),
                    )
                })
            })
            .transpose()?
            .unwrap_or(0);
        Ok(Self {
            authority,
            actors,
            stream_id,
            max_payload,
            next_sequence,
            pending_mirror: None,
            owner_root,
            warm_root,
            migration_draining: false,
        })
    }

    pub fn authority(&self) -> &StableExecutorAuthority {
        &self.authority
    }

    pub fn executor(&self) -> &StableShardExecutor {
        self.authority.executor()
    }

    pub fn actor(&self, shard: ShardId) -> Result<&AuthoritativeShard, StableExecutorDurableError> {
        self.actors
            .get(&shard)
            .ok_or(StableExecutorDurableError::MissingShard(shard))
    }

    pub fn pending_mirror_event(&self) -> Option<EventId> {
        self.pending_mirror
            .as_ref()
            .map(|pending| pending.envelope.event)
    }

    /// Drain all already-admitted work and then freeze new admission at one
    /// durable source boundary. The caller owns the bridge exclusively while
    /// this runs, so no biological transition can appear between the final
    /// drained step and the returned state set. A bounded step limit keeps a
    /// non-converging same-tick workload explicit instead of treating an
    /// unbounded loop as migration progress.
    pub fn drain_for_migration(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        checkpoint_start: EventId,
        max_steps: usize,
    ) -> Result<Vec<ShardState>, StableExecutorDurableError> {
        if self.migration_draining {
            return Err(StableExecutorDurableError::MigrationDraining);
        }
        if max_steps == 0 {
            return Err(StableExecutorDurableError::MigrationDrainLimit);
        }
        if self.pending_mirror.is_some() {
            self.retry_mirror()?;
        }
        let mut steps = 0usize;
        while self.executor().total_pending() > 0 {
            if steps >= max_steps {
                return Err(StableExecutorDurableError::MigrationDrainLimit);
            }
            let offset = u64::try_from(steps).map_err(|_| {
                StableExecutorDurableError::Encoding(
                    "migration drain checkpoint sequence overflowed".to_owned(),
                )
            })?;
            let checkpoint_raw = checkpoint_start.raw().checked_add(offset).ok_or_else(|| {
                StableExecutorDurableError::Encoding(
                    "migration drain checkpoint ID space exhausted".to_owned(),
                )
            })?;
            let checkpoint_id = EventId::new(checkpoint_raw).map_err(|_| {
                StableExecutorDurableError::Encoding(
                    "migration drain checkpoint ID is invalid".to_owned(),
                )
            })?;
            let result = self.step_pending(observed_term, observed_fencing_token, checkpoint_id)?;
            steps = steps.saturating_add(1);
            if result.is_none() {
                return Err(StableExecutorDurableError::MigrationDrainLimit);
            }
        }
        let states = self.durable_states()?;
        self.migration_draining = true;
        Ok(states)
    }

    /// Release a drain after a migration failed before source fencing. The
    /// managed runtime remains paused until its orchestrator deliberately
    /// resumes it; this method only removes the durable bridge admission gate
    /// for a safe retry or operator recovery path.
    pub fn abort_migration_drain(&mut self) {
        self.migration_draining = false;
    }

    pub fn migration_draining(&self) -> bool {
        self.migration_draining
    }

    /// Allocate the first checkpoint ID for a bounded migration drain after
    /// the last immutable checkpoint currently published by this bridge.
    pub fn next_checkpoint_id(&self) -> Result<EventId, StableExecutorDurableError> {
        let last = self
            .authority
            .last_checkpoint()
            .map(|manifest| manifest.checkpoint_id.raw())
            .ok_or_else(|| {
                StableExecutorDurableError::Encoding(
                    "stable executor has no published checkpoint".to_owned(),
                )
            })?;
        let next = last.checked_add(1).ok_or_else(|| {
            StableExecutorDurableError::Encoding(
                "stable executor checkpoint ID space exhausted".to_owned(),
            )
        })?;
        EventId::new(next).map_err(|_| {
            StableExecutorDurableError::Encoding(
                "stable executor next checkpoint ID is invalid".to_owned(),
            )
        })
    }

    /// Export the durable actor views in stable shard-ID order for a verified
    /// migration transfer. The nested stable checkpoint retains pending work,
    /// deduplication and the complete-fabric digest; the outer state retains
    /// the actor's WAL, receipts and channel projection.
    pub fn durable_states(&self) -> Result<Vec<ShardState>, StableExecutorDurableError> {
        self.actors
            .values()
            .map(|actor| actor.state().map_err(StableExecutorDurableError::from))
            .collect()
    }

    /// Prepare one bounded transfer source per durable shard from the same
    /// actor checkpoints that were published for the last complete cut.
    ///
    /// The returned sources are data-plane objects only: they do not grant a
    /// destination lease, alter the bridge's writer term, or publish a
    /// placement. The caller must submit the associated evidence through the
    /// fenced migration operation and placement registry before cutover.
    /// Transfer IDs are allocated in stable shard-ID order so retries can
    /// reproduce the same manifest identities from the same bridge boundary.
    pub fn prepare_transfer_sources(
        &self,
        first_transfer_id: EventId,
        source_node: impl Into<String>,
        cut: &ConsistentCut,
        source_plan_digest: StateDigest,
        frame_bytes: usize,
    ) -> Result<Vec<ShardTransferSource>, StableExecutorDurableError> {
        let source_node = source_node.into();
        let states = self.durable_states()?;
        states
            .into_iter()
            .enumerate()
            .map(|(index, state)| {
                let offset = u64::try_from(index).map_err(|_| {
                    StableExecutorDurableError::Encoding(
                        "stable executor transfer ID index overflow".to_owned(),
                    )
                })?;
                let transfer_id = first_transfer_id
                    .raw()
                    .checked_add(offset)
                    .and_then(|raw| EventId::new(raw).ok())
                    .ok_or_else(|| {
                        StableExecutorDurableError::Encoding(
                            "stable executor transfer ID space exhausted".to_owned(),
                        )
                    })?;
                ShardTransferSource::prepare(
                    transfer_id,
                    source_node.clone(),
                    &state,
                    cut,
                    source_plan_digest,
                    frame_bytes,
                )
                .map_err(StableExecutorDurableError::from)
            })
            .collect()
    }

    /// Expose the last complete immutable fabric cut through the bounded
    /// checkpoint-transfer protocol.  This is deliberately separate from the
    /// per-shard migration sources above: a whole-brain relocation needs one
    /// complete checkpoint that a destination can verify before activating a
    /// partial or consolidated runtime.
    pub fn prepare_checkpoint_transfer_source(
        &self,
        transfer_id: EventId,
        source_node: impl Into<String>,
        expected_plan_digest: StateDigest,
        frame_bytes: usize,
    ) -> Result<CheckpointTransferSource, StableExecutorDurableError> {
        let checkpoint_id = self
            .authority
            .last_checkpoint()
            .map(|manifest| manifest.checkpoint_id)
            .ok_or_else(|| {
                StableExecutorDurableError::Encoding(
                    "stable executor has no published complete checkpoint".to_owned(),
                )
            })?;
        CheckpointTransferSource::from_store(
            &self.authority.checkpoint_store(),
            transfer_id,
            source_node,
            checkpoint_id,
            self.authority.brain_id(),
            expected_plan_digest,
            frame_bytes,
        )
        .map_err(StableExecutorDurableError::from)
    }

    pub fn actor_roots(&self) -> (&Path, &Path) {
        (&self.owner_root, &self.warm_root)
    }

    /// Maximum payload accepted by the durable actor boundary. The managed
    /// network uses the same bound for its authenticated incoming stream
    /// cursor so transport admission cannot accept data the durable owner
    /// would later reject.
    pub fn max_payload(&self) -> usize {
        self.max_payload
    }

    /// Return one acknowledgement from each durable actor at one sealed
    /// state boundary. The caller uses this evidence for worker registration
    /// and migration admission; it is never a writer grant or a substitute
    /// for the orchestrator's fencing decision.
    pub fn application_acknowledgements(
        &self,
    ) -> Result<Vec<StableShardApplicationAck>, StableExecutorDurableError> {
        let plan_digest = self.authority.executor().plan().digest().to_string();
        self.actors
            .values()
            .map(|actor| {
                let state = actor.state()?;
                Ok(StableShardApplicationAck {
                    shard_id: state.shard_id.raw(),
                    brain_id: state.brain_id.raw(),
                    topology_generation: state.topology_generation.raw(),
                    partition_generation: state.partition_generation.raw(),
                    plan_digest: plan_digest.clone(),
                    lease_term: state.lease_term.raw(),
                    fencing_token: self.authority.fencing_token(),
                    applied_tick: state.applied_tag.tick,
                    applied_microstep: state.applied_tag.microstep,
                    state_digest: state.state_digest.to_string(),
                    durable_wal_sequence: state.durable_wal_sequence,
                    committed: true,
                })
            })
            .collect()
    }

    /// Reconstruct one external producer cursor from the durable actor
    /// receipts. A complete stable cut must expose the same prefix on every
    /// actor; disagreement means recovery cannot safely resume the stream.
    pub fn stream_progress(
        &self,
        stream_id: StreamId,
    ) -> Result<Option<StableStreamProgress>, StableExecutorDurableError> {
        let mut expected: Option<BTreeMap<u64, StableStreamReceipt>> = None;
        for actor in self.actors.values() {
            let entries = actor
                .stream_receipts(stream_id)
                .into_iter()
                .map(|receipt| {
                    (
                        receipt.sequence,
                        StableStreamReceipt {
                            event_id: receipt.event_id,
                            payload_digest: receipt.payload_digest,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if let Some(previous) = &expected {
                if previous != &entries {
                    return Err(StableExecutorDurableError::Encoding(
                        "durable external stream receipt prefixes disagree".to_owned(),
                    ));
                }
            } else {
                expected = Some(entries);
            }
        }
        let Some(entries) = expected else {
            return Ok(None);
        };
        let next_sequence = entries
            .keys()
            .next_back()
            .copied()
            .map(|sequence| {
                sequence.checked_add(1).ok_or_else(|| {
                    StableExecutorDurableError::Encoding(
                        "durable external stream sequence exhausted".to_owned(),
                    )
                })
            })
            .transpose()?
            .unwrap_or(0);
        Ok(Some(StableStreamProgress {
            next_sequence,
            entries,
        }))
    }

    /// Fence this source bridge after a brain-wide destination has been
    /// published.  The bridge retains its immutable source state for audit
    /// and recovery, but every subsequent admission must present the newer
    /// destination term and therefore fails closed at the source actor
    /// boundary.  A later reclaim registers the destination as a new bridge.
    pub fn fence_after_migration(
        &mut self,
        destination_term: LeaseTerm,
    ) -> Result<(), StableExecutorDurableError> {
        let current_term = self.authority.term();
        let current_token = self.authority.fencing_token();
        self.authority.reissue_term(
            current_term,
            current_token,
            destination_term,
            destination_term.raw(),
        )?;
        self.migration_draining = true;
        Ok(())
    }

    /// Execute and publish one complete fabric step, then mirror the resulting
    /// checkpoint to every durable actor. If mirroring fails, the immutable
    /// complete cut remains authoritative and [`Self::retry_mirror`] resumes
    /// the same operation without executing the neural event again.
    pub fn admit_and_step(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        routed: RoutedCausalEvent,
        checkpoint_id: EventId,
    ) -> Result<Option<ShardExecutionResult>, StableExecutorDurableError> {
        self.admit_and_step_inner(
            observed_term,
            observed_fencing_token,
            None,
            routed,
            checkpoint_id,
        )
    }

    /// Admit a transport envelope while preserving its producer stream and
    /// sequence in the durable actor receipts. The bridge's own mirror
    /// sequence remains separate for locally generated work.
    pub fn admit_and_step_with_transport(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        transport: &CausalEnvelope,
        routed: RoutedCausalEvent,
        checkpoint_id: EventId,
    ) -> Result<Option<ShardExecutionResult>, StableExecutorDurableError> {
        self.admit_and_step_inner(
            observed_term,
            observed_fencing_token,
            Some(transport),
            routed,
            checkpoint_id,
        )
    }

    fn admit_and_step_inner(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        transport: Option<&CausalEnvelope>,
        routed: RoutedCausalEvent,
        checkpoint_id: EventId,
    ) -> Result<Option<ShardExecutionResult>, StableExecutorDurableError> {
        if self.migration_draining {
            return Err(StableExecutorDurableError::MigrationDraining);
        }
        if let Some(pending) = &self.pending_mirror {
            return Err(StableExecutorDurableError::MirrorPending {
                event: pending.envelope.event,
            });
        }
        let before = self.actor_digests()?;
        let Some(result) = self.authority.admit_and_step(
            observed_term,
            observed_fencing_token,
            routed,
            checkpoint_id,
        )?
        else {
            return Ok(None);
        };
        self.prepare_mirror(&result, before, transport)?;
        self.retry_mirror()?;
        Ok(Some(result))
    }

    /// Process one event that was admitted by a previous committed cut.
    ///
    /// Keeping this operation on the durable bridge is important: generated
    /// same-tick work must cross the same term/fencing/checkpoint boundary as
    /// external input. Calling `StableShardExecutor::step` directly here
    /// would otherwise create a biological transition that is absent from the
    /// durable commit protocol.
    pub fn step_pending(
        &mut self,
        observed_term: LeaseTerm,
        observed_fencing_token: u64,
        checkpoint_id: EventId,
    ) -> Result<Option<ShardExecutionResult>, StableExecutorDurableError> {
        if self.migration_draining {
            return Err(StableExecutorDurableError::MigrationDraining);
        }
        if let Some(pending) = &self.pending_mirror {
            return Err(StableExecutorDurableError::MirrorPending {
                event: pending.envelope.event,
            });
        }
        let before = self.actor_digests()?;
        let Some(result) =
            self.authority
                .step_pending(observed_term, observed_fencing_token, checkpoint_id)?
        else {
            return Ok(None);
        };
        self.prepare_mirror(&result, before, None)?;
        self.retry_mirror()?;
        Ok(Some(result))
    }

    /// Retry the currently published cut. The same envelope is reused so the
    /// actor receiver's sequence and event deduplication remain stable.
    pub fn retry_mirror(&mut self) -> Result<(), StableExecutorDurableError> {
        let Some(pending) = self.pending_mirror.clone() else {
            return Err(StableExecutorDurableError::NoPendingMirror);
        };
        for (shard_id, checkpoint) in &pending.checkpoints {
            let actor = self
                .actors
                .get_mut(shard_id)
                .ok_or(StableExecutorDurableError::MissingShard(*shard_id))?;
            let expected = pending
                .expected_actor_digests
                .get(shard_id)
                .copied()
                .ok_or(StableExecutorDurableError::MissingShard(*shard_id))?;
            let outcome = actor.apply_stable_checkpoint(
                &pending.envelope,
                expected,
                encode_checkpoint(checkpoint)?,
                pending.channel_state.clone(),
            )?;
            if !matches!(
                outcome,
                DurableApplyOutcome::Applied { .. } | DurableApplyOutcome::Duplicate { .. }
            ) {
                return Err(StableExecutorDurableError::MirrorPending {
                    event: pending.envelope.event,
                });
            }
        }
        if pending.advance_internal_sequence {
            self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
                StableExecutorDurableError::Encoding("mirror sequence exhausted".to_owned())
            })?;
        }
        self.pending_mirror = None;
        Ok(())
    }

    fn prepare_mirror(
        &mut self,
        result: &ShardExecutionResult,
        expected_actor_digests: BTreeMap<ShardId, StateDigest>,
        transport: Option<&CausalEnvelope>,
    ) -> Result<(), StableExecutorDurableError> {
        let checkpoints = self
            .authority
            .executor()
            .checkpoint_shards()?
            .into_iter()
            .map(|checkpoint| (checkpoint.shard_id, checkpoint))
            .collect();
        let mut envelope = self.envelope_for(&result.consumed)?;
        let advance_internal_sequence = transport.is_none();
        if let Some(transport) = transport {
            if transport.event != result.consumed.event.id {
                return Err(StableExecutorDurableError::Encoding(
                    "transport event identity differs from the admitted event".to_owned(),
                ));
            }
            envelope.stream = transport.stream;
            envelope.sequence = transport.sequence;
            envelope.lease_term = transport.lease_term;
            envelope.route = transport.route;
        }
        self.pending_mirror = Some(PendingMirror {
            envelope,
            expected_actor_digests,
            checkpoints,
            channel_state: serde_json::to_vec(&result.emitted)
                .map_err(|error| StableExecutorDurableError::Encoding(error.to_string()))?,
            advance_internal_sequence,
        });
        Ok(())
    }

    fn open_actors(
        authority: &StableExecutorAuthority,
        checkpoints: Vec<StableShardCheckpoint>,
        owner_root: &Path,
        warm_root: &Path,
        term: LeaseTerm,
        stream_id: StreamId,
        max_payload: usize,
        channel_state: &[u8],
    ) -> Result<BTreeMap<ShardId, AuthoritativeShard>, StableExecutorDurableError> {
        let brain_id = authority.brain_id();
        let mut actors = BTreeMap::new();
        for checkpoint in checkpoints {
            let expected_biological_state = encode_checkpoint(&checkpoint)?;
            let actor = AuthoritativeShard::open(
                owner_path(owner_root, checkpoint.shard_id),
                Some(warm_path(warm_root, checkpoint.shard_id)),
                brain_id,
                checkpoint.shard_id,
                checkpoint.topology_generation,
                checkpoint.partition_generation,
                term,
                stream_id,
                max_payload,
                expected_biological_state.clone(),
                channel_state.to_vec(),
            )?;
            if actor.biological_state() != expected_biological_state.as_slice() {
                return Err(StableExecutorDurableError::Durability(
                    DurabilityError::Corrupt(
                        "existing durable actor does not match the stable cut".to_owned(),
                    ),
                ));
            }
            if actors.insert(checkpoint.shard_id, actor).is_some() {
                return Err(StableExecutorDurableError::MissingShard(
                    checkpoint.shard_id,
                ));
            }
        }
        Ok(actors)
    }

    fn actor_digests(&self) -> Result<BTreeMap<ShardId, StateDigest>, StableExecutorDurableError> {
        self.actors
            .iter()
            .map(|(shard, actor)| {
                actor
                    .checkpoint()
                    .map(|checkpoint| (*shard, checkpoint.state_digest))
                    .map_err(StableExecutorDurableError::from)
            })
            .collect()
    }

    fn envelope_for(
        &self,
        consumed: &RoutedCausalEvent,
    ) -> Result<CausalEnvelope, StableExecutorDurableError> {
        let route = consumed
            .route
            .or_else(|| RouteId::new(1).ok())
            .ok_or_else(|| StableExecutorDurableError::Encoding("invalid route".to_owned()))?;
        Ok(CausalEnvelope {
            schema_version: SchemaVersion::CURRENT,
            brain: self.authority.brain_id(),
            stream: self.stream_id,
            sequence: self.next_sequence,
            lease_term: self.authority.term(),
            route,
            partition_generation: self.authority.executor().plan().partition_generation(),
            source: (consumed.event.key.source != 0)
                .then(|| crate::deterministic::NeuronId::new(consumed.event.key.source))
                .transpose()
                .map_err(|_| {
                    StableExecutorDurableError::Encoding("invalid event source".to_owned())
                })?,
            target: crate::deterministic::NeuronId::new(consumed.event.key.target).ok(),
            tag: consumed.event.key.tag,
            event: consumed.event.id,
            stage: consumed.event.key.stage,
            kind: EnvelopeKind::Event,
            payload: consumed.event.payload.clone(),
            deferred_from_nonconvergence: consumed.event.deferred_from_nonconvergence,
        })
    }
}

fn encode_checkpoint(
    checkpoint: &StableShardCheckpoint,
) -> Result<Vec<u8>, StableExecutorDurableError> {
    serde_json::to_vec(checkpoint)
        .map_err(|error| StableExecutorDurableError::Encoding(error.to_string()))
}

fn owner_path(root: &Path, shard: ShardId) -> PathBuf {
    root.join(format!("shard-{}.owner.json", shard.raw()))
}

fn warm_path(root: &Path, shard: ShardId) -> PathBuf {
    root.join(format!("shard-{}.warm.json", shard.raw()))
}
