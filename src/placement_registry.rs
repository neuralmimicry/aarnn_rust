//! Authoritative virtual-shard placement state.
//!
//! [`crate::placement`] deliberately stops at a proposal.  This module is the
//! next control-plane boundary: it records the placement generation and the
//! single active writer for every stable shard.  It does not transfer model
//! bytes or open worker connections.  A migration adapter must first produce
//! [`CutoverEvidence`] and then submit one atomic apply request here.
//!
//! The reference implementation is intentionally synchronous and deterministic
//! so it can be used by an orchestrator, a CLI rehearsal, or a fault-injection
//! test.  The persisted wrapper uses a process-shared lock and an atomic,
//! fsync-backed replace.  A production deployment must put this state behind
//! the phase-07 replicated control-plane authority before enabling automatic
//! failover.

use crate::deterministic::{BrainId, EventId, LeaseTerm, LogicalTag, ShardId, StateDigest};
use crate::placement::{PlacementError, PlacementPlan, RepartitionPlan, ShardPlacement};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Version of the authoritative registry document.
pub const PLACEMENT_REGISTRY_SCHEMA_VERSION: u32 = 2;
/// Maximum serialized activation command retained in the registry journal.
/// The worker manifest has the same bounded input contract; this larger bound
/// leaves room for the envelope while preventing a control-plane restart from
/// replaying unbounded caller data.
pub const MAX_PLACEMENT_ACTIVATION_COMMAND_BYTES: usize = 2 * 1024 * 1024;

fn zero_state_digest() -> StateDigest {
    StateDigest([0; 16])
}

/// The one active writer recorded for a stable virtual shard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardAuthority {
    pub shard_id: ShardId,
    pub node_id: String,
    pub device_id: String,
    pub failure_domain: String,
    pub lease_term: LeaseTerm,
    pub fencing_token: u64,
    pub plan_digest: StateDigest,
}

impl ShardAuthority {
    fn from_placement(placement: &ShardPlacement, plan: &PlacementPlan) -> Self {
        Self {
            shard_id: placement.shard_id,
            node_id: placement.active_node.clone(),
            device_id: placement.active_device.clone(),
            failure_domain: placement.active_failure_domain.clone(),
            lease_term: plan.lease_term,
            fencing_token: plan.fencing_token,
            plan_digest: plan.digest(),
        }
    }
}

/// Evidence emitted by a bounded checkpoint/WAL transfer before a writer cut.
///
/// The registry checks identity, generation, source term, destination term and
/// the committed logical boundary.  It cannot verify the bytes itself; the
/// checkpoint store and migration adapter own that cryptographic verification
/// and must only submit evidence after it succeeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardCutoverEvidence {
    pub source_node: String,
    pub source_term: LeaseTerm,
    pub checkpoint_digest: StateDigest,
    pub caught_up: bool,
    /// Digest of the transferred causal route/channel cursor boundary.
    #[serde(default = "zero_state_digest")]
    pub route_cursor_digest: StateDigest,
    /// Digest of the committed peripheral/actuator effect cursor boundary.
    #[serde(default = "zero_state_digest")]
    pub effect_cursor_digest: StateDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CutoverEvidence {
    pub operation_id: EventId,
    pub source_plan_digest: StateDigest,
    pub cut_tag: LogicalTag,
    pub destination_term: LeaseTerm,
    pub shards: BTreeMap<ShardId, ShardCutoverEvidence>,
}

impl CutoverEvidence {
    pub fn verify(&self) -> Result<(), PlacementRegistryError> {
        if self.cut_tag.microstep != 0 || self.destination_term.raw() == 0 {
            return Err(PlacementRegistryError::InvalidEvidence(
                "cutover must use a committed microstep and non-zero destination term",
            ));
        }
        if self.shards.is_empty() {
            return Err(PlacementRegistryError::InvalidEvidence(
                "cutover must cover at least one shard",
            ));
        }
        if self.shards.values().any(|shard| {
            shard.source_node.trim().is_empty()
                || shard.checkpoint_digest == StateDigest([0; 16])
                || shard.route_cursor_digest == StateDigest([0; 16])
                || shard.effect_cursor_digest == StateDigest([0; 16])
                || !shard.caught_up
        }) {
            return Err(PlacementRegistryError::InvalidEvidence(
                "cutover evidence contains an unverified source or checkpoint",
            ));
        }
        Ok(())
    }
}

/// A placement mutation submitted by an already-authenticated orchestrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementApplyRequest {
    pub request_id: String,
    pub idempotency_key: String,
    pub expected_resource_version: u64,
    pub observed_leader_term: LeaseTerm,
    pub plan: PlacementPlan,
    #[serde(default)]
    pub cutover: Option<CutoverEvidence>,
    #[serde(default)]
    pub repartition: Option<RepartitionPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementApplyReceipt {
    pub request_id: String,
    pub idempotency_key: String,
    pub plan_digest: StateDigest,
    pub resource_version: u64,
    pub leader_term: LeaseTerm,
    pub cut_tag: LogicalTag,
    /// `false` while the placement is durably prepared and waiting for target
    /// worker activation evidence.  The field defaults to `true` so registry
    /// documents written before the prepare/commit boundary remain readable.
    #[serde(default = "default_committed_receipt")]
    pub committed: bool,
}

fn default_committed_receipt() -> bool {
    true
}

/// Outcome of the asynchronous worker activation paired with a published
/// placement. The registry records this separately from shard authority so a
/// dispatch failure cannot be mistaken for a committed worker lifecycle step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementActivationState {
    Pending,
    Queued,
    /// The target worker has presented a validated registration whose plan,
    /// ownership, lease, fencing token and committed shard acknowledgements
    /// match the activation command and published placement.
    Active,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementActivationStatus {
    /// Idempotency key of the placement publication that authorised this
    /// activation. Older registry documents used the map key for both
    /// values; the empty default preserves that representation on reopen.
    #[serde(default)]
    pub placement_idempotency_key: String,
    pub request_id: String,
    pub plan_digest: StateDigest,
    pub state: PlacementActivationState,
    #[serde(default)]
    pub error: String,
    /// The verified command envelope is retained so a restarted orchestrator
    /// can retry a non-terminal dispatch. It is immutable for an idempotency
    /// key and is deliberately stored as bounded JSON rather than a raw
    /// in-memory worker object.
    #[serde(default)]
    pub activation_command_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AppliedRequest {
    request_id: String,
    plan_digest: StateDigest,
    receipt: PlacementApplyReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PreparedPlacement {
    request: PlacementApplyRequest,
    receipt: PlacementApplyReceipt,
    authorities: BTreeMap<ShardId, ShardAuthority>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementRegistry {
    pub schema_version: u32,
    pub brain_id: BrainId,
    pub leader_term: LeaseTerm,
    pub resource_version: u64,
    pub active_plan: Option<PlacementPlan>,
    pub authorities: BTreeMap<ShardId, ShardAuthority>,
    /// Idempotency key -> activation lifecycle status. This is additive
    /// state and defaults empty when opening an older registry document.
    #[serde(default)]
    pub activation_statuses: BTreeMap<String, PlacementActivationStatus>,
    /// At most one placement may be in the prepare phase for a brain.  The
    /// record is durable so a control-plane restart cannot publish a plan
    /// without rechecking the target activation evidence.
    #[serde(default)]
    prepared_placement: Option<PreparedPlacement>,
    applied_requests: BTreeMap<String, AppliedRequest>,
    #[serde(default)]
    aborted_requests: BTreeMap<String, AppliedRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlacementRegistryError {
    #[error("placement registry schema version is unsupported")]
    UnsupportedSchema,
    #[error("placement registry brain identity does not match")]
    BrainMismatch,
    #[error("request ID and idempotency key are required")]
    EmptyRequestIdentity,
    #[error("request used stale leader term: expected {expected}, received {received}")]
    StaleLeader {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("expected resource version {expected}, current version is {current}")]
    VersionConflict { expected: u64, current: u64 },
    #[error("idempotency key {key} was reused for a different placement")]
    IdempotencyConflict { key: String },
    #[error("placement plan is invalid: {0}")]
    Plan(#[from] PlacementError),
    #[error("placement plan lease term/token does not match the control-plane term")]
    PlanTermMismatch,
    #[error("placement generations moved backwards")]
    GenerationRegression,
    #[error("placement changed shard identities without a valid repartition transaction")]
    MissingRepartition,
    #[error("repartition transaction does not match the placement cutover")]
    RepartitionMismatch,
    #[error("active writer changed without cutover evidence")]
    CutoverRequired,
    #[error("cutover evidence is invalid: {0}")]
    InvalidEvidence(&'static str),
    #[error("cutover evidence does not match shard {shard}")]
    EvidenceMismatch { shard: ShardId },
    #[error("persisted registry I/O failed: {0}")]
    Io(String),
    #[error("persisted registry encoding failed: {0}")]
    Encoding(String),
    #[error("persisted registry state is invalid: {0}")]
    InvalidPersisted(String),
    #[error("another placement is already prepared for activation")]
    PreparedPlacementExists,
    #[error("no placement is prepared for activation")]
    NoPreparedPlacement,
    #[error("prepared placement activation is incomplete")]
    ActivationIncomplete,
    #[error("prepared placement activation failed: {0}")]
    ActivationFailed(String),
}

impl PlacementRegistry {
    pub fn new(brain_id: BrainId, leader_term: LeaseTerm) -> Self {
        Self {
            schema_version: PLACEMENT_REGISTRY_SCHEMA_VERSION,
            brain_id,
            leader_term,
            resource_version: 0,
            active_plan: None,
            authorities: BTreeMap::new(),
            activation_statuses: BTreeMap::new(),
            prepared_placement: None,
            applied_requests: BTreeMap::new(),
            aborted_requests: BTreeMap::new(),
        }
    }

    pub fn verify(&self) -> Result<(), PlacementRegistryError> {
        if self.schema_version != PLACEMENT_REGISTRY_SCHEMA_VERSION || self.leader_term.raw() == 0 {
            return Err(PlacementRegistryError::UnsupportedSchema);
        }
        if let Some(plan) = &self.active_plan {
            plan.verify()?;
            if plan.brain_id != self.brain_id || plan.lease_term > self.leader_term {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "active plan identity or term does not match registry".to_owned(),
                ));
            }
            let expected = plan
                .placements
                .iter()
                .map(|placement| {
                    (
                        placement.shard_id,
                        ShardAuthority::from_placement(placement, plan),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if expected != self.authorities {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "active shard authorities do not match active plan".to_owned(),
                ));
            }
        } else if !self.authorities.is_empty() {
            return Err(PlacementRegistryError::InvalidPersisted(
                "authorities exist without an active plan".to_owned(),
            ));
        }
        if let Some(prepared) = &self.prepared_placement {
            prepared.request.plan.verify()?;
            self.validate_request(&prepared.request)?;
            if prepared.request.plan.brain_id != self.brain_id
                || prepared.receipt.committed
                || prepared.receipt.plan_digest != prepared.request.plan.digest()
                || prepared.receipt.idempotency_key != prepared.request.idempotency_key
                || prepared.receipt.request_id != prepared.request.request_id
                || prepared.receipt.resource_version != self.resource_version.saturating_add(1)
                || prepared.authorities
                    != prepared
                        .request
                        .plan
                        .placements
                        .iter()
                        .map(|placement| {
                            (
                                placement.shard_id,
                                ShardAuthority::from_placement(placement, &prepared.request.plan),
                            )
                        })
                        .collect::<BTreeMap<_, _>>()
            {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "prepared placement record is inconsistent".to_owned(),
                ));
            }
        }
        for applied in self.applied_requests.values() {
            if !applied.receipt.committed
                || applied.receipt.plan_digest != applied.plan_digest
                || applied.receipt.leader_term > self.leader_term
            {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "applied request receipt is inconsistent".to_owned(),
                ));
            }
        }
        for (idempotency_key, status) in &self.activation_statuses {
            let placement_key = if status.placement_idempotency_key.trim().is_empty() {
                idempotency_key
            } else {
                status.placement_idempotency_key.as_str()
            };
            let applied = self.applied_requests.get(placement_key);
            let prepared = self
                .prepared_placement
                .as_ref()
                .filter(|prepared| prepared.request.idempotency_key == placement_key)
                .map(|prepared| &prepared.receipt);
            let aborted = self.aborted_requests.get(placement_key);
            let Some((expected_request_id, expected_digest)) = applied
                .map(|applied| (applied.request_id.as_str(), applied.plan_digest))
                .or_else(|| {
                    prepared.map(|receipt| (receipt.request_id.as_str(), receipt.plan_digest))
                })
                .or_else(|| {
                    aborted.map(|aborted| (aborted.request_id.as_str(), aborted.plan_digest))
                })
            else {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "activation status has no known placement".to_owned(),
                ));
            };
            if status.request_id != expected_request_id || status.plan_digest != expected_digest {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "activation status does not match its placement".to_owned(),
                ));
            }
            if matches!(status.state, PlacementActivationState::Failed)
                && status.error.trim().is_empty()
            {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "failed activation status has no error".to_owned(),
                ));
            }
            if !matches!(status.state, PlacementActivationState::Failed) && !status.error.is_empty()
            {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "non-failed activation status contains an error".to_owned(),
                ));
            }
            if status.activation_command_json.len() > MAX_PLACEMENT_ACTIVATION_COMMAND_BYTES {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "activation command exceeds the bounded journal size".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Atomically publish one plan after all active owner changes have
    /// evidence. No authority is changed until every validation succeeds.
    pub fn apply(
        &mut self,
        request: PlacementApplyRequest,
    ) -> Result<PlacementApplyReceipt, PlacementRegistryError> {
        if let Some(applied) = self.applied_requests.get(&request.idempotency_key) {
            if applied.request_id != request.request_id
                || applied.plan_digest != request.plan.digest()
            {
                return Err(PlacementRegistryError::IdempotencyConflict {
                    key: request.idempotency_key,
                });
            }
            return Ok(applied.receipt.clone());
        }
        if self.prepared_placement.is_some() {
            return Err(PlacementRegistryError::PreparedPlacementExists);
        }
        self.validate_request(&request)?;

        // Build all new records before mutating self. This keeps the method
        // transaction-like even when a future validation is added.
        let authorities = request
            .plan
            .placements
            .iter()
            .map(|placement| {
                (
                    placement.shard_id,
                    ShardAuthority::from_placement(placement, &request.plan),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let next_version = self.resource_version.checked_add(1).ok_or(
            PlacementRegistryError::InvalidPersisted("resource version exhausted".to_owned()),
        )?;
        let cut_tag = request
            .cutover
            .as_ref()
            .map(|evidence| evidence.cut_tag)
            .unwrap_or(request.plan.effective_tag);
        let receipt = PlacementApplyReceipt {
            request_id: request.request_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            plan_digest: request.plan.digest(),
            resource_version: next_version,
            leader_term: self.leader_term,
            cut_tag,
            committed: true,
        };
        self.resource_version = next_version;
        self.active_plan = Some(request.plan);
        self.authorities = authorities;
        self.applied_requests.insert(
            request.idempotency_key,
            AppliedRequest {
                request_id: request.request_id,
                plan_digest: receipt.plan_digest,
                receipt: receipt.clone(),
            },
        );
        self.verify()?;
        Ok(receipt)
    }

    /// Durably validate and stage a placement without changing the
    /// authoritative plan or shard writers.  Activation status records may be
    /// attached to the returned receipt and `commit_prepared` is the only path
    /// that publishes the staged authorities.
    pub fn prepare(
        &mut self,
        request: PlacementApplyRequest,
    ) -> Result<PlacementApplyReceipt, PlacementRegistryError> {
        if let Some(applied) = self.applied_requests.get(&request.idempotency_key) {
            if applied.request_id != request.request_id
                || applied.plan_digest != request.plan.digest()
            {
                return Err(PlacementRegistryError::IdempotencyConflict {
                    key: request.idempotency_key,
                });
            }
            return Ok(applied.receipt.clone());
        }
        if let Some(aborted) = self.aborted_requests.get(&request.idempotency_key) {
            if aborted.request_id == request.request_id
                && aborted.plan_digest == request.plan.digest()
            {
                return Err(PlacementRegistryError::IdempotencyConflict {
                    key: request.idempotency_key,
                });
            }
            return Err(PlacementRegistryError::IdempotencyConflict {
                key: request.idempotency_key,
            });
        }
        if let Some(prepared) = &self.prepared_placement {
            if prepared.request.idempotency_key == request.idempotency_key
                && prepared.request.request_id == request.request_id
                && prepared.receipt.plan_digest == request.plan.digest()
            {
                return Ok(prepared.receipt.clone());
            }
            return Err(PlacementRegistryError::PreparedPlacementExists);
        }
        self.validate_request(&request)?;
        let authorities = request
            .plan
            .placements
            .iter()
            .map(|placement| {
                (
                    placement.shard_id,
                    ShardAuthority::from_placement(placement, &request.plan),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let resource_version = self.resource_version.checked_add(1).ok_or(
            PlacementRegistryError::InvalidPersisted("resource version exhausted".to_owned()),
        )?;
        let cut_tag = request
            .cutover
            .as_ref()
            .map(|evidence| evidence.cut_tag)
            .unwrap_or(request.plan.effective_tag);
        let receipt = PlacementApplyReceipt {
            request_id: request.request_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            plan_digest: request.plan.digest(),
            resource_version,
            leader_term: self.leader_term,
            cut_tag,
            committed: false,
        };
        self.prepared_placement = Some(PreparedPlacement {
            request,
            receipt: receipt.clone(),
            authorities,
        });
        self.verify()?;
        Ok(receipt)
    }

    pub fn prepared_plan(&self) -> Option<&PlacementPlan> {
        self.prepared_placement
            .as_ref()
            .map(|prepared| &prepared.request.plan)
    }

    /// Publish the prepared plan only after all of its activation records are
    /// terminally active.  The state transition is one in-memory mutation and
    /// the persisted wrapper writes it with the same atomic replace as other
    /// registry mutations.
    pub fn commit_prepared(&mut self) -> Result<PlacementApplyReceipt, PlacementRegistryError> {
        let Some(prepared) = self.prepared_placement.clone() else {
            return Err(PlacementRegistryError::NoPreparedPlacement);
        };
        let placement_key = prepared.request.idempotency_key.as_str();
        let statuses = self
            .activation_statuses
            .values()
            .filter(|status| {
                let key = if status.placement_idempotency_key.trim().is_empty() {
                    placement_key
                } else {
                    status.placement_idempotency_key.as_str()
                };
                key == placement_key
            })
            .collect::<Vec<_>>();
        if statuses
            .iter()
            .any(|status| matches!(status.state, PlacementActivationState::Failed))
        {
            return Err(PlacementRegistryError::ActivationFailed(
                "one or more target activations failed".to_owned(),
            ));
        }
        if statuses
            .iter()
            .any(|status| !matches!(status.state, PlacementActivationState::Active))
        {
            return Err(PlacementRegistryError::ActivationIncomplete);
        }
        let mut receipt = prepared.receipt.clone();
        receipt.committed = true;
        self.resource_version = receipt.resource_version;
        self.active_plan = Some(prepared.request.plan.clone());
        self.authorities = prepared.authorities;
        self.applied_requests.insert(
            placement_key.to_owned(),
            AppliedRequest {
                request_id: prepared.request.request_id,
                plan_digest: receipt.plan_digest,
                receipt: receipt.clone(),
            },
        );
        self.prepared_placement = None;
        self.verify()?;
        Ok(receipt)
    }

    /// Abort an uncommitted placement while retaining its activation statuses
    /// and an audit binding that prevents accidental idempotency-key reuse.
    pub fn abort_prepared(&mut self) -> Result<(), PlacementRegistryError> {
        let Some(prepared) = self.prepared_placement.take() else {
            return Err(PlacementRegistryError::NoPreparedPlacement);
        };
        self.aborted_requests.insert(
            prepared.request.idempotency_key,
            AppliedRequest {
                request_id: prepared.request.request_id,
                plan_digest: prepared.receipt.plan_digest,
                receipt: prepared.receipt,
            },
        );
        self.verify()
    }

    pub fn authority(&self, shard_id: ShardId) -> Option<&ShardAuthority> {
        self.authorities.get(&shard_id)
    }

    /// Record the result of dispatching the activation paired with an applied
    /// placement. The status is bound to the immutable applied request digest
    /// and is safe to retry after a process or transport failure.
    pub fn record_activation_status(
        &mut self,
        idempotency_key: impl Into<String>,
        request_id: impl Into<String>,
        plan_digest: StateDigest,
        state: PlacementActivationState,
        error: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        self.record_activation_status_with_command(
            idempotency_key,
            request_id,
            plan_digest,
            state,
            error,
            "",
        )
    }

    /// Record an activation state and, when supplied, its verified command in
    /// one in-memory mutation. The persisted wrapper publishes this mutation
    /// atomically, so a crash cannot leave a retryable status without its
    /// command payload.
    pub fn record_activation_status_with_command(
        &mut self,
        idempotency_key: impl Into<String>,
        request_id: impl Into<String>,
        plan_digest: StateDigest,
        state: PlacementActivationState,
        error: impl Into<String>,
        activation_command_json: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        let idempotency_key = idempotency_key.into();
        self.record_activation_dispatch_status(
            idempotency_key.clone(),
            idempotency_key,
            request_id,
            plan_digest,
            state,
            error,
            activation_command_json,
        )
    }

    /// Record one independently retryable worker activation for an applied
    /// placement. A placement may contain shards on many nodes, so each
    /// target gets its own activation key while all keys remain bound to the
    /// same immutable placement publication.
    pub fn record_activation_dispatch_status(
        &mut self,
        placement_idempotency_key: impl Into<String>,
        activation_idempotency_key: impl Into<String>,
        request_id: impl Into<String>,
        plan_digest: StateDigest,
        state: PlacementActivationState,
        error: impl Into<String>,
        activation_command_json: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        let placement_idempotency_key = placement_idempotency_key.into();
        let activation_idempotency_key = activation_idempotency_key.into();
        let request_id = request_id.into();
        let error = error.into();
        let activation_command_json = activation_command_json.into();
        if placement_idempotency_key.trim().is_empty()
            || activation_idempotency_key.trim().is_empty()
            || request_id.trim().is_empty()
        {
            return Err(PlacementRegistryError::EmptyRequestIdentity);
        }
        let placement = self
            .applied_requests
            .get(&placement_idempotency_key)
            .map(|applied| (applied.request_id.clone(), applied.plan_digest))
            .or_else(|| {
                self.prepared_placement
                    .as_ref()
                    .filter(|prepared| {
                        prepared.request.idempotency_key == placement_idempotency_key
                    })
                    .map(|prepared| {
                        (
                            prepared.request.request_id.clone(),
                            prepared.receipt.plan_digest,
                        )
                    })
            })
            .or_else(|| {
                self.aborted_requests
                    .get(&placement_idempotency_key)
                    .map(|aborted| (aborted.request_id.clone(), aborted.plan_digest))
            });
        let Some((applied_request_id, applied_plan_digest)) = placement else {
            return Err(PlacementRegistryError::InvalidPersisted(
                "activation status references an unknown placement".to_owned(),
            ));
        };
        if applied_request_id != request_id || applied_plan_digest != plan_digest {
            return Err(PlacementRegistryError::IdempotencyConflict {
                key: activation_idempotency_key,
            });
        }
        if matches!(state, PlacementActivationState::Failed) && error.trim().is_empty() {
            return Err(PlacementRegistryError::InvalidPersisted(
                "failed activation status requires an error".to_owned(),
            ));
        }
        if !matches!(state, PlacementActivationState::Failed) && !error.is_empty() {
            return Err(PlacementRegistryError::InvalidPersisted(
                "non-failed activation status cannot contain an error".to_owned(),
            ));
        }
        if activation_command_json.len() > MAX_PLACEMENT_ACTIVATION_COMMAND_BYTES {
            return Err(PlacementRegistryError::InvalidPersisted(
                "activation command exceeds the bounded journal size".to_owned(),
            ));
        }
        let previous_command = self
            .activation_statuses
            .get(&activation_idempotency_key)
            .map(|status| status.activation_command_json.as_str())
            .unwrap_or_default();
        if let Some(previous) = self.activation_statuses.get(&activation_idempotency_key) {
            if previous.state == PlacementActivationState::Active
                && state != PlacementActivationState::Active
            {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "an active worker activation cannot regress".to_owned(),
                ));
            }
            if previous.state == PlacementActivationState::Failed
                && state != PlacementActivationState::Failed
            {
                return Err(PlacementRegistryError::InvalidPersisted(
                    "a failed worker activation cannot be resurrected".to_owned(),
                ));
            }
        }
        if !previous_command.is_empty()
            && !activation_command_json.is_empty()
            && previous_command != activation_command_json
        {
            return Err(PlacementRegistryError::IdempotencyConflict {
                key: activation_idempotency_key,
            });
        }
        let command_json = if activation_command_json.is_empty() {
            previous_command.to_owned()
        } else {
            activation_command_json
        };
        self.activation_statuses.insert(
            activation_idempotency_key,
            PlacementActivationStatus {
                placement_idempotency_key,
                request_id,
                plan_digest,
                state,
                error,
                activation_command_json: command_json,
            },
        );
        self.verify()
    }

    /// Return durable activation commands that may be retried after a process
    /// restart. Terminal failures are intentionally excluded: retrying one
    /// requires a new management request and idempotency key.
    pub fn retryable_activation_commands(&self) -> Vec<(String, PlacementActivationStatus)> {
        self.activation_statuses
            .iter()
            .filter(|(_, status)| {
                matches!(
                    status.state,
                    PlacementActivationState::Pending | PlacementActivationState::Queued
                ) && !status.activation_command_json.is_empty()
            })
            .map(|(key, status)| (key.clone(), status.clone()))
            .collect()
    }

    /// Record a later worker outcome without requiring the data-plane result
    /// handler to reconstruct the original request or plan digest. The
    /// immutable applied-request table is the source of those bindings.
    pub fn record_activation_outcome(
        &mut self,
        idempotency_key: impl Into<String>,
        state: PlacementActivationState,
        error: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        let idempotency_key = idempotency_key.into();
        let error = error.into();
        let Some(previous) = self.activation_statuses.get(&idempotency_key).cloned() else {
            return Err(PlacementRegistryError::InvalidPersisted(
                "activation outcome references an unknown activation".to_owned(),
            ));
        };
        if previous.state == state && previous.error == error {
            return Ok(());
        }
        if matches!(previous.state, PlacementActivationState::Failed) {
            return Err(PlacementRegistryError::InvalidPersisted(
                "a worker outcome cannot resurrect a failed activation; submit a new retry"
                    .to_owned(),
            ));
        }
        if matches!(previous.state, PlacementActivationState::Active)
            && state != PlacementActivationState::Active
        {
            return Err(PlacementRegistryError::InvalidPersisted(
                "an active worker outcome cannot regress".to_owned(),
            ));
        }
        let placement_key = if previous.placement_idempotency_key.trim().is_empty() {
            idempotency_key.clone()
        } else {
            previous.placement_idempotency_key.clone()
        };
        let (request_id, plan_digest) = self
            .applied_requests
            .get(&placement_key)
            .map(|applied| (applied.request_id.clone(), applied.plan_digest))
            .or_else(|| {
                self.prepared_placement
                    .as_ref()
                    .filter(|prepared| prepared.request.idempotency_key == placement_key)
                    .map(|prepared| {
                        (
                            prepared.request.request_id.clone(),
                            prepared.receipt.plan_digest,
                        )
                    })
            })
            .or_else(|| {
                self.aborted_requests
                    .get(&placement_key)
                    .map(|aborted| (aborted.request_id.clone(), aborted.plan_digest))
            })
            .ok_or_else(|| {
                PlacementRegistryError::InvalidPersisted(
                    "activation outcome references an unknown placement".to_owned(),
                )
            })?;
        self.record_activation_dispatch_status(
            placement_key,
            idempotency_key,
            request_id,
            plan_digest,
            state,
            error,
            "",
        )
    }

    pub fn set_leader_term(&mut self, term: LeaseTerm) -> Result<(), PlacementRegistryError> {
        if term < self.leader_term {
            return Err(PlacementRegistryError::StaleLeader {
                expected: self.leader_term,
                received: term,
            });
        }
        if term == self.leader_term {
            return Ok(());
        }
        self.leader_term = term;
        Ok(())
    }

    fn validate_request(
        &self,
        request: &PlacementApplyRequest,
    ) -> Result<(), PlacementRegistryError> {
        request.plan.verify()?;
        if request.plan.brain_id != self.brain_id {
            return Err(PlacementRegistryError::BrainMismatch);
        }
        if request.request_id.trim().is_empty() || request.idempotency_key.trim().is_empty() {
            return Err(PlacementRegistryError::EmptyRequestIdentity);
        }
        if request.observed_leader_term != self.leader_term {
            return Err(PlacementRegistryError::StaleLeader {
                expected: self.leader_term,
                received: request.observed_leader_term,
            });
        }
        if request.plan.lease_term != self.leader_term
            || request.plan.fencing_token != self.leader_term.raw()
        {
            return Err(PlacementRegistryError::PlanTermMismatch);
        }
        if request.expected_resource_version != self.resource_version {
            return Err(PlacementRegistryError::VersionConflict {
                expected: request.expected_resource_version,
                current: self.resource_version,
            });
        }
        self.validate_generations(&request.plan)?;
        self.validate_repartition(request)?;
        let changed = self.changed_shards(&request.plan);
        if !changed.is_empty() {
            let evidence = request
                .cutover
                .as_ref()
                .ok_or(PlacementRegistryError::CutoverRequired)?;
            evidence.verify()?;
            if evidence.source_plan_digest
                != self
                    .active_plan
                    .as_ref()
                    .map(PlacementPlan::digest)
                    .unwrap_or(StateDigest([0; 16]))
                || evidence.destination_term != request.plan.lease_term
            {
                return Err(PlacementRegistryError::InvalidEvidence(
                    "source plan or destination term does not match",
                ));
            }
            for shard in changed.iter().copied() {
                let Some(current) = self.authorities.get(&shard) else {
                    return Err(PlacementRegistryError::EvidenceMismatch { shard });
                };
                let Some(proof) = evidence.shards.get(&shard) else {
                    return Err(PlacementRegistryError::EvidenceMismatch { shard });
                };
                if proof.source_node != current.node_id || proof.source_term != current.lease_term {
                    return Err(PlacementRegistryError::EvidenceMismatch { shard });
                }
            }
        }
        Ok(())
    }

    fn validate_generations(&self, plan: &PlacementPlan) -> Result<(), PlacementRegistryError> {
        let Some(current) = &self.active_plan else {
            return Ok(());
        };
        if plan.topology_generation < current.topology_generation
            || plan.partition_generation < current.partition_generation
        {
            return Err(PlacementRegistryError::GenerationRegression);
        }
        if plan.partition_generation == current.partition_generation {
            let current_shards = current
                .placements
                .iter()
                .map(|placement| placement.shard_id)
                .collect::<BTreeSet<_>>();
            let next_shards = plan
                .placements
                .iter()
                .map(|placement| placement.shard_id)
                .collect::<BTreeSet<_>>();
            if current_shards != next_shards {
                return Err(PlacementRegistryError::MissingRepartition);
            }
        }
        Ok(())
    }

    fn validate_repartition(
        &self,
        request: &PlacementApplyRequest,
    ) -> Result<(), PlacementRegistryError> {
        let Some(current) = &self.active_plan else {
            if request.repartition.is_some() {
                return Err(PlacementRegistryError::RepartitionMismatch);
            }
            return Ok(());
        };
        let current_shards = current
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        let next_shards = request
            .plan
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        if current_shards == next_shards {
            if request.repartition.is_some() {
                return Err(PlacementRegistryError::RepartitionMismatch);
            }
            return Ok(());
        }
        let Some(repartition) = request.repartition.as_ref() else {
            return Err(PlacementRegistryError::MissingRepartition);
        };
        repartition.verify().map_err(PlacementRegistryError::Plan)?;
        if repartition.brain_id != self.brain_id
            || repartition.source_partition_generation != current.partition_generation
            || repartition.target_partition_generation != request.plan.partition_generation
            || repartition.topology_generation != request.plan.topology_generation
            || repartition
                .source_shards
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                != current_shards
            || repartition
                .target_shards
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                != next_shards
        {
            return Err(PlacementRegistryError::RepartitionMismatch);
        }
        Ok(())
    }

    fn changed_shards(&self, plan: &PlacementPlan) -> BTreeSet<ShardId> {
        let mut changed = BTreeSet::new();
        let next_ids = plan
            .placements
            .iter()
            .map(|placement| placement.shard_id)
            .collect::<BTreeSet<_>>();
        // Removed source shards also need a checkpoint/cut proof: their
        // authority is being retired at the same atomic boundary.
        changed.extend(
            self.authorities
                .keys()
                .filter(|shard| !next_ids.contains(shard))
                .copied(),
        );
        for placement in &plan.placements {
            let Some(current) = self.authorities.get(&placement.shard_id) else {
                continue;
            };
            if current.node_id != placement.active_node
                || current.device_id != placement.active_device
                || current.failure_domain != placement.active_failure_domain
            {
                changed.insert(placement.shard_id);
            }
        }
        changed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRegistryDocument {
    schema_version: u32,
    state: PlacementRegistry,
}

/// Crash-safe storage adapter for [`PlacementRegistry`].
#[derive(Debug)]
pub struct PersistedPlacementRegistry {
    path: PathBuf,
    lock_path: PathBuf,
    state: PlacementRegistry,
}

impl PersistedPlacementRegistry {
    pub fn open(
        path: impl Into<PathBuf>,
        brain_id: BrainId,
        leader_term: LeaseTerm,
    ) -> Result<Self, PlacementRegistryError> {
        let path = path.into();
        let lock_path = path.with_extension("placement.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
        }
        let lock = lock_file(&lock_path)?;
        let result = (|| {
            if path.exists() {
                let bytes = fs::read(&path)
                    .map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
                let document: PersistedRegistryDocument = serde_json::from_slice(&bytes)
                    .map_err(|error| PlacementRegistryError::Encoding(error.to_string()))?;
                if document.schema_version != PLACEMENT_REGISTRY_SCHEMA_VERSION {
                    return Err(PlacementRegistryError::UnsupportedSchema);
                }
                if document.state.brain_id != brain_id {
                    return Err(PlacementRegistryError::BrainMismatch);
                }
                if leader_term < document.state.leader_term {
                    return Err(PlacementRegistryError::StaleLeader {
                        expected: document.state.leader_term,
                        received: leader_term,
                    });
                }
                let mut state = document.state;
                if leader_term > state.leader_term {
                    state.set_leader_term(leader_term)?;
                    persist_registry(&path, &state)?;
                }
                state.verify()?;
                Ok(state)
            } else {
                let state = PlacementRegistry::new(brain_id, leader_term);
                persist_registry(&path, &state)?;
                Ok(state)
            }
        })();
        unlock_file(lock)?;
        Ok(Self {
            path,
            lock_path,
            state: result?,
        })
    }

    pub fn state(&self) -> &PlacementRegistry {
        &self.state
    }

    /// Reopen an already published registry for an outcome update without
    /// inventing or regressing a leader term. Result callbacks carry the
    /// immutable placement key but intentionally do not grant a new term.
    pub fn open_existing(
        path: impl Into<PathBuf>,
        brain_id: BrainId,
    ) -> Result<Self, PlacementRegistryError> {
        let path = path.into();
        let lock_path = path.with_extension("placement.lock");
        let lock = lock_file(&lock_path)?;
        let result = (|| {
            let state = read_registry(&path)?;
            if state.brain_id != brain_id {
                return Err(PlacementRegistryError::BrainMismatch);
            }
            state.verify()?;
            Ok(state)
        })();
        unlock_file(lock)?;
        Ok(Self {
            path,
            lock_path,
            state: result?,
        })
    }

    pub fn refresh(&mut self) -> Result<(), PlacementRegistryError> {
        let bytes =
            fs::read(&self.path).map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
        let document: PersistedRegistryDocument = serde_json::from_slice(&bytes)
            .map_err(|error| PlacementRegistryError::Encoding(error.to_string()))?;
        if document.schema_version != PLACEMENT_REGISTRY_SCHEMA_VERSION {
            return Err(PlacementRegistryError::UnsupportedSchema);
        }
        document.state.verify()?;
        self.state = document.state;
        Ok(())
    }

    /// Advance the persisted control-plane term before a destination cut is
    /// applied. This uses the same lock and atomic publication boundary as a
    /// placement mutation, so a restarted process cannot observe a stale
    /// registry term after promotion.
    pub fn set_leader_term(&mut self, term: LeaseTerm) -> Result<(), PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut state = read_registry(&self.path)?;
            state.set_leader_term(term)?;
            persist_registry(&self.path, &state)?;
            self.state = state;
            Ok(())
        })();
        unlock_file(lock)?;
        result
    }

    pub fn prepare(
        &mut self,
        request: PlacementApplyRequest,
    ) -> Result<PlacementApplyReceipt, PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut state = read_registry(&self.path)?;
            let receipt = state.prepare(request)?;
            persist_registry(&self.path, &state)?;
            self.state = state;
            Ok(receipt)
        })();
        unlock_file(lock)?;
        result
    }

    pub fn commit_prepared(&mut self) -> Result<PlacementApplyReceipt, PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut state = read_registry(&self.path)?;
            let receipt = state.commit_prepared()?;
            persist_registry(&self.path, &state)?;
            self.state = state;
            Ok(receipt)
        })();
        unlock_file(lock)?;
        result
    }

    pub fn abort_prepared(&mut self) -> Result<(), PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut state = read_registry(&self.path)?;
            state.abort_prepared()?;
            persist_registry(&self.path, &state)?;
            self.state = state;
            Ok(())
        })();
        unlock_file(lock)?;
        result
    }

    pub fn apply(
        &mut self,
        request: PlacementApplyRequest,
    ) -> Result<PlacementApplyReceipt, PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut state = read_registry(&self.path)?;
            let receipt = state.apply(request)?;
            persist_registry(&self.path, &state)?;
            self.state = state;
            Ok(receipt)
        })();
        unlock_file(lock)?;
        result
    }

    pub fn record_activation_status(
        &mut self,
        idempotency_key: impl Into<String>,
        request_id: impl Into<String>,
        plan_digest: StateDigest,
        state: PlacementActivationState,
        error: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        self.record_activation_status_with_command(
            idempotency_key,
            request_id,
            plan_digest,
            state,
            error,
            "",
        )
    }

    pub fn record_activation_status_with_command(
        &mut self,
        idempotency_key: impl Into<String>,
        request_id: impl Into<String>,
        plan_digest: StateDigest,
        state: PlacementActivationState,
        error: impl Into<String>,
        activation_command_json: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut registry = read_registry(&self.path)?;
            registry.record_activation_status_with_command(
                idempotency_key,
                request_id,
                plan_digest,
                state,
                error,
                activation_command_json,
            )?;
            persist_registry(&self.path, &registry)?;
            self.state = registry;
            Ok(())
        })();
        unlock_file(lock)?;
        result
    }

    /// Persist one independently retryable activation dispatch while keeping
    /// it bound to the placement publication that authorised it.
    pub fn record_activation_dispatch_status(
        &mut self,
        placement_idempotency_key: impl Into<String>,
        activation_idempotency_key: impl Into<String>,
        request_id: impl Into<String>,
        plan_digest: StateDigest,
        state: PlacementActivationState,
        error: impl Into<String>,
        activation_command_json: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut registry = read_registry(&self.path)?;
            registry.record_activation_dispatch_status(
                placement_idempotency_key,
                activation_idempotency_key,
                request_id,
                plan_digest,
                state,
                error,
                activation_command_json,
            )?;
            persist_registry(&self.path, &registry)?;
            self.state = registry;
            Ok(())
        })();
        unlock_file(lock)?;
        result
    }

    pub fn record_activation_outcome(
        &mut self,
        idempotency_key: impl Into<String>,
        state: PlacementActivationState,
        error: impl Into<String>,
    ) -> Result<(), PlacementRegistryError> {
        let lock = lock_file(&self.lock_path)?;
        let result = (|| {
            let mut registry = read_registry(&self.path)?;
            registry.record_activation_outcome(idempotency_key, state, error)?;
            persist_registry(&self.path, &registry)?;
            self.state = registry;
            Ok(())
        })();
        unlock_file(lock)?;
        result
    }
}

fn lock_file(path: &Path) -> Result<std::fs::File, PlacementRegistryError> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
    file.lock_exclusive()
        .map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
    Ok(file)
}

fn unlock_file(file: std::fs::File) -> Result<(), PlacementRegistryError> {
    file.unlock()
        .map_err(|error| PlacementRegistryError::Io(error.to_string()))
}

fn read_registry(path: &Path) -> Result<PlacementRegistry, PlacementRegistryError> {
    let bytes = fs::read(path).map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
    let document: PersistedRegistryDocument = serde_json::from_slice(&bytes)
        .map_err(|error| PlacementRegistryError::Encoding(error.to_string()))?;
    if document.schema_version != PLACEMENT_REGISTRY_SCHEMA_VERSION {
        return Err(PlacementRegistryError::UnsupportedSchema);
    }
    document.state.verify()?;
    Ok(document.state)
}

fn persist_registry(path: &Path, state: &PlacementRegistry) -> Result<(), PlacementRegistryError> {
    state.verify()?;
    let document = PersistedRegistryDocument {
        schema_version: PLACEMENT_REGISTRY_SCHEMA_VERSION,
        state: state.clone(),
    };
    let bytes = serde_json::to_vec(&document)
        .map_err(|error| PlacementRegistryError::Encoding(error.to_string()))?;
    let temporary = path.with_extension(format!("placement.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
    let result = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, path))
        .map_err(|error| PlacementRegistryError::Io(error.to_string()));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlacementRegistryError::Io(error.to_string()))?;
    }
    Ok(())
}
