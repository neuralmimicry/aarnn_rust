//! Orchestrator-authorised management reference contract.

use crate::deterministic::{BrainId, EventId, LeaseTerm, ShardId};
use crate::migration_executor::{MigrationDispatchHandler, MigrationDispatchReceipt};
use crate::migration_group::{MigrationGroupSpec, MigrationGroupUpdate};
use crate::migration_operation::{
    MigrationJournal, MigrationOperation, MigrationPhase, MigrationRequest, MigrationTransition,
    PersistedMigrationJournal,
};
use crate::placement::{PlacementCommand, PlacementCommandKind, PlacementPlan, ShardDemand};
use crate::placement_controller::{
    AutomaticPlacementPolicy, PlacementController, PlacementControllerError, PlacementReview,
};
use crate::placement_registry::{
    CutoverEvidence, PersistedPlacementRegistry, PlacementActivationState, PlacementApplyRequest,
    PlacementRegistry, PlacementRegistryError,
};
use crate::stable_worker::{StableWorkerActivationCommand, StableWorkerRegistration};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity, ServerTlsConfig};
use tonic::{Request, Response, Status};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    Read,
    Operate,
    Reset,
    Export,
    PeripheralInput,
    PeripheralOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    /// Cluster-wide grants retained for the compatibility profile. New
    /// managed resources should use `brain_grants` so permission on one brain
    /// cannot be used as permission on another.
    grants: BTreeMap<String, BTreeSet<Capability>>,
    #[serde(default)]
    brain_grants: BTreeMap<(String, String), BTreeSet<Capability>>,
}

impl Policy {
    pub fn grant(&mut self, principal: impl Into<String>, capability: Capability) {
        self.grants
            .entry(principal.into())
            .or_default()
            .insert(capability);
    }

    pub fn allows(&self, principal: &Principal, capability: &Capability) -> bool {
        self.grants
            .get(&principal.id)
            .is_some_and(|capabilities| capabilities.contains(capability))
    }

    /// Grant a capability for one explicit brain scope. This is additive to
    /// the existing cluster-wide grant API so persisted policies remain
    /// backwards-compatible while management cutover migrates callers.
    pub fn grant_for_brain(
        &mut self,
        principal: impl Into<String>,
        brain_id: impl Into<String>,
        capability: Capability,
    ) {
        self.brain_grants
            .entry((principal.into(), brain_id.into()))
            .or_default()
            .insert(capability);
    }

    pub fn allows_for_brain(
        &self,
        principal: &Principal,
        brain_id: &str,
        capability: &Capability,
    ) -> bool {
        self.allows(principal, capability)
            || self
                .brain_grants
                .get(&(principal.id.clone(), brain_id.to_owned()))
                .is_some_and(|capabilities| capabilities.contains(capability))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationKind {
    Start,
    Stop,
    Reset,
    Export,
}

/// Orchestrator-owned execution hook for the secured generated API. The
/// management service never receives a worker handle; it can only submit a
/// persisted operation and ask the orchestrator adapter to execute it.
pub type ManagementDispatchFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
pub type ManagementOperationDispatcher =
    Arc<dyn Fn(String, OperationKind) -> ManagementDispatchFuture + Send + Sync>;

/// Orchestrator-owned placement activation hook. The management service only
/// passes a verified command to this adapter; it never receives a worker
/// handle or opens a data-plane connection itself.
pub type PlacementActivationDispatchFuture =
    Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
pub type PlacementActivationDispatcher =
    Arc<dyn Fn(StableWorkerActivationCommand) -> PlacementActivationDispatchFuture + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationState {
    Pending,
    Running,
    Succeeded,
    Failed { code: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: EventId,
    #[serde(default)]
    pub brain_id: String,
    pub principal: Principal,
    pub idempotency_key: String,
    pub request_id: String,
    pub expected_version: u64,
    pub kind: OperationKind,
    pub state: OperationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    #[serde(default)]
    pub sequence: u64,
    pub request_id: String,
    pub principal: String,
    pub operation_id: EventId,
    pub outcome: String,
    pub leader_term: LeaseTerm,
    #[serde(default)]
    pub previous_digest: String,
    #[serde(default)]
    pub digest: String,
}

const AUDIT_GENESIS_DIGEST: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Serialize)]
struct AuditDigestMaterial<'a> {
    sequence: u64,
    request_id: &'a str,
    principal: &'a str,
    operation_id: EventId,
    outcome: &'a str,
    leader_term: LeaseTerm,
    previous_digest: &'a str,
}

fn audit_record_digest(record: &AuditRecord) -> String {
    let material = AuditDigestMaterial {
        sequence: record.sequence,
        request_id: &record.request_id,
        principal: &record.principal,
        operation_id: record.operation_id,
        outcome: &record.outcome,
        leader_term: record.leader_term,
        previous_digest: &record.previous_digest,
    };
    let encoded = serde_json::to_vec(&material).expect("audit digest material is serializable");
    hex::encode(Sha256::digest(encoded))
}

fn append_audit_record(audit: &mut Vec<AuditRecord>, mut record: AuditRecord) {
    record.sequence = audit
        .len()
        .checked_add(1)
        .expect("audit sequence exhausted") as u64;
    record.previous_digest = audit
        .last()
        .map(|previous| previous.digest.clone())
        .unwrap_or_else(|| AUDIT_GENESIS_DIGEST.to_owned());
    record.digest = audit_record_digest(&record);
    audit.push(record);
}

fn verify_audit_chain(audit: &[AuditRecord]) -> Result<(), String> {
    let mut previous = AUDIT_GENESIS_DIGEST.to_owned();
    for (index, record) in audit.iter().enumerate() {
        let expected_sequence = index
            .checked_add(1)
            .ok_or_else(|| "audit sequence exhausted".to_owned())?
            as u64;
        if record.sequence != expected_sequence
            || record.previous_digest != previous
            || record.digest.len() != 64
            || record.digest != audit_record_digest(record)
        {
            return Err(format!(
                "audit hash chain is invalid at sequence {expected_sequence}"
            ));
        }
        previous = record.digest.clone();
    }
    Ok(())
}

fn reseal_legacy_audit(audit: &mut Vec<AuditRecord>) -> Result<(), String> {
    if audit.is_empty() {
        return Ok(());
    }
    if audit.iter().any(|record| {
        record.sequence != 0 || !record.previous_digest.is_empty() || !record.digest.is_empty()
    }) {
        return Err("management audit contains a partially migrated hash chain".to_owned());
    }
    let legacy = std::mem::take(audit);
    for record in legacy {
        append_audit_record(audit, record);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ManagementError {
    #[error("principal {principal} lacks capability {capability:?}")]
    Forbidden {
        principal: String,
        capability: Capability,
    },
    #[error("request used stale leader term: expected {expected}, received {received}")]
    StaleLeader {
        expected: LeaseTerm,
        received: LeaseTerm,
    },
    #[error("expected resource version {expected}, current version is {current}")]
    VersionConflict { expected: u64, current: u64 },
    #[error("idempotency key {0} was reused for a different operation")]
    IdempotencyConflict(String),
    #[error("idempotency key is empty")]
    EmptyIdempotencyKey,
    #[error("request ID is empty")]
    EmptyRequestId,
    #[error("operation {0} is not present")]
    MissingOperation(EventId),
    #[error("operation identity space is exhausted")]
    OperationIdExhausted,
    #[error("resource version space is exhausted")]
    ResourceVersionOverflow,
    #[error("invalid operation state transition from {from:?} to {to:?}")]
    InvalidOperationTransition {
        from: OperationState,
        to: OperationState,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationContext {
    pub observed_leader_term: LeaseTerm,
    pub expected_version: u64,
    pub idempotency_key: String,
    pub request_id: String,
}

/// A lease and fencing token issued for one shard by a quorum-backed control
/// plane. The token is checked at every stateful data-plane boundary; node
/// reachability alone is never sufficient for issuance or promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardLease {
    pub shard_id: ShardId,
    pub node_id: String,
    pub term: LeaseTerm,
    pub fencing_token: u64,
}

/// Inputs for an atomic brain-wide destination lease decision.
///
/// A whole-brain cut must use one destination term so that the placement
/// generation and all shard actors become visible at the same logical
/// boundary. Issuing one lease per shard would advance the term for every
/// shard and make that invariant impossible to prove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasePromotionRequest {
    pub shard_id: ShardId,
    pub source_node: String,
    pub source_term: LeaseTerm,
    pub source_fencing_token: u64,
    pub destination_node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QuorumError {
    #[error("control plane has no quorum: {available} of {members} members available")]
    QuorumUnavailable { available: usize, members: usize },
    #[error("unknown control-plane member {0}")]
    UnknownMember(String),
    #[error("control-plane member ID is empty")]
    EmptyMember,
    #[error("control-plane member {0} is listed more than once")]
    DuplicateMember(String),
    #[error("shard lease target node {0} is not a control-plane member")]
    UnknownLeaseNode(String),
    #[error("lease term space is exhausted")]
    TermExhausted,
    #[error("shard {0} has no current lease")]
    MissingLease(ShardId),
    #[error("brain-wide lease promotion must contain at least one shard")]
    EmptyLeaseSet,
    #[error(
        "shard {shard} lease is fenced: expected term/token {expected_term}/{expected_token}, received {received_term}/{received_token}"
    )]
    Fenced {
        shard: ShardId,
        expected_term: LeaseTerm,
        expected_token: u64,
        received_term: LeaseTerm,
        received_token: u64,
    },
}

/// Deterministic quorum/fencing reference authority.
///
/// Membership and availability are deliberately explicit inputs so a test
/// harness can inject partitions without relying on wall-clock heartbeats.
/// A production implementation must persist these decisions through a mature
/// consensus system before this authority can replace the compatibility
/// control path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumLeaseAuthority {
    members: BTreeSet<String>,
    available: BTreeSet<String>,
    current_term: LeaseTerm,
    leases: BTreeMap<ShardId, ShardLease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PersistedAuthorityError {
    #[error(transparent)]
    Quorum(#[from] QuorumError),
    #[error("persisted authority I/O failed: {0}")]
    Io(String),
    #[error("persisted authority encoding failed: {0}")]
    Encoding(String),
    #[error("persisted authority state is invalid: {0}")]
    Invalid(String),
    #[error(transparent)]
    Management(#[from] ManagementError),
}

/// Process-safe persistence for the quorum/fencing decision log.
///
/// This is intentionally a small storage adapter around the reference
/// authority, not an invented consensus algorithm: callers must still supply
/// the member availability observed by their real consensus implementation.
/// The persisted term and lease record make restart/failover tests reject old
/// terms instead of resetting fencing state to the initial term.
#[derive(Debug)]
pub struct PersistedQuorumLeaseAuthority {
    path: PathBuf,
    lock_path: PathBuf,
    authority: QuorumLeaseAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAuthorityDocument {
    schema_version: u32,
    authority: QuorumLeaseAuthority,
}

impl PersistedQuorumLeaseAuthority {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn open(
        path: impl Into<PathBuf>,
        members: impl IntoIterator<Item = String>,
    ) -> Result<Self, PersistedAuthorityError> {
        let path = path.into();
        let lock_path = path.with_extension("lock");
        let expected = members.into_iter().collect::<BTreeSet<_>>();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let result = (|| {
            let authority = if path.exists() {
                let bytes = fs::read(&path)
                    .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
                let document: PersistedAuthorityDocument = serde_json::from_slice(&bytes)
                    .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
                if document.schema_version != Self::SCHEMA_VERSION {
                    return Err(PersistedAuthorityError::Invalid(
                        "unsupported authority schema version".to_owned(),
                    ));
                }
                let actual = document.authority.members.clone();
                if !expected.is_empty() && actual != expected {
                    return Err(PersistedAuthorityError::Invalid(
                        "persisted membership differs from configured membership".to_owned(),
                    ));
                }
                document.authority
            } else {
                let authority = QuorumLeaseAuthority::new(expected.iter().cloned())?;
                let document = PersistedAuthorityDocument {
                    schema_version: Self::SCHEMA_VERSION,
                    authority: authority.clone(),
                };
                let bytes = serde_json::to_vec(&document)
                    .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
                atomic_replace_authority(&path, &bytes)?;
                authority
            };
            authority.validate_loaded()?;
            Ok(authority)
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        let authority = match (result, unlock) {
            (Ok(authority), Ok(())) => authority,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        Ok(Self {
            path,
            lock_path,
            authority,
        })
    }

    pub fn authority(&self) -> &QuorumLeaseAuthority {
        &self.authority
    }

    pub fn set_member_available(
        &mut self,
        member: &str,
        is_available: bool,
    ) -> Result<(), PersistedAuthorityError> {
        self.with_locked_update(|authority| authority.set_member_available(member, is_available))
    }

    pub fn issue_lease(
        &mut self,
        shard_id: ShardId,
        node_id: impl Into<String>,
    ) -> Result<ShardLease, PersistedAuthorityError> {
        let node_id = node_id.into();
        self.with_locked_update(|authority| authority.issue_lease(shard_id, node_id))
    }

    pub fn revoke(&mut self, shard_id: ShardId) -> Result<(), PersistedAuthorityError> {
        self.with_locked_update(|authority| authority.revoke(shard_id))
    }

    pub fn validate(
        &self,
        shard_id: ShardId,
        node_id: &str,
        term: LeaseTerm,
        fencing_token: u64,
    ) -> Result<(), QuorumError> {
        self.authority
            .validate(shard_id, node_id, term, fencing_token)
    }

    /// Validate against the latest persisted decision, not merely this
    /// process's opening snapshot.  A long-lived worker must use this check
    /// before each stateful commit so a lease revoked by another control
    /// process fences it without requiring a process restart.
    pub fn validate_current(
        &self,
        shard_id: ShardId,
        node_id: &str,
        term: LeaseTerm,
        fencing_token: u64,
    ) -> Result<(), PersistedAuthorityError> {
        self.with_locked_read(|authority| {
            authority
                .validate(shard_id, node_id, term, fencing_token)
                .map_err(PersistedAuthorityError::from)
        })
    }

    /// Refresh the in-memory view after a control-plane process has changed
    /// membership or a lease.  Mutating operations already refresh while
    /// holding their write lock; this method is for read-only observers.
    pub fn refresh(&mut self) -> Result<(), PersistedAuthorityError> {
        let authority = self.with_locked_read(|authority| Ok(authority.clone()))?;
        self.authority = authority;
        Ok(())
    }

    fn read_latest_unlocked(&self) -> Result<QuorumLeaseAuthority, PersistedAuthorityError> {
        if !self.path.exists() {
            return Ok(self.authority.clone());
        }
        let bytes =
            fs::read(&self.path).map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let document: PersistedAuthorityDocument = serde_json::from_slice(&bytes)
            .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
        if document.schema_version != Self::SCHEMA_VERSION {
            return Err(PersistedAuthorityError::Invalid(
                "unsupported authority schema version".to_owned(),
            ));
        }
        if document.authority.members != self.authority.members {
            return Err(PersistedAuthorityError::Invalid(
                "persisted membership differs from configured membership".to_owned(),
            ));
        }
        document.authority.validate_loaded()?;
        Ok(document.authority)
    }

    fn with_locked_read<T>(
        &self,
        read: impl FnOnce(&QuorumLeaseAuthority) -> Result<T, PersistedAuthorityError>,
    ) -> Result<T, PersistedAuthorityError> {
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let result = (|| {
            let authority = self.read_latest_unlocked()?;
            read(&authority)
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn persist(&self, authority: &QuorumLeaseAuthority) -> Result<(), PersistedAuthorityError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let document = PersistedAuthorityDocument {
            schema_version: Self::SCHEMA_VERSION,
            authority: authority.clone(),
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
        let result = atomic_replace_authority(&self.path, &bytes);
        let unlock = fs2::FileExt::unlock(&lock);
        result.and(unlock.map_err(|error| PersistedAuthorityError::Io(error.to_string())))
    }

    /// Execute one authority mutation against the latest on-disk state while
    /// holding the process-shared lock for the complete read/modify/write
    /// transaction. Without this refresh, two control-plane processes could
    /// both issue the same next term from stale memory and one decision would
    /// disappear after restart.
    fn with_locked_update<T>(
        &mut self,
        update: impl FnOnce(&mut QuorumLeaseAuthority) -> Result<T, QuorumError>,
    ) -> Result<T, PersistedAuthorityError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;

        let result = (|| {
            let mut authority = if self.path.exists() {
                let bytes = fs::read(&self.path)
                    .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
                let document: PersistedAuthorityDocument = serde_json::from_slice(&bytes)
                    .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
                if document.schema_version != Self::SCHEMA_VERSION {
                    return Err(PersistedAuthorityError::Invalid(
                        "unsupported authority schema version".to_owned(),
                    ));
                }
                document.authority
            } else {
                self.authority.clone()
            };
            authority.validate_loaded()?;
            let value = update(&mut authority)?;
            let document = PersistedAuthorityDocument {
                schema_version: Self::SCHEMA_VERSION,
                authority: authority.clone(),
            };
            let bytes = serde_json::to_vec(&document)
                .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
            atomic_replace_authority(&self.path, &bytes)?;
            self.authority = authority;
            Ok(value)
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplicatedAuthorityDocument {
    schema_version: u32,
    revision: u64,
    authority: QuorumLeaseAuthority,
}

/// Majority-replicated authority adapter used by the authoritative shard and
/// recovery harnesses.
///
/// Each member has an independent durable document. A mutation is successful
/// only after a majority of configured members have atomically persisted the
/// same revision. Reads select a revision that is present on a majority and
/// reject split/divergent state. This gives the repository a real durable
/// quorum boundary for local multi-process testing; production still needs an
/// approved networked consensus implementation and its operational evidence
/// before this adapter can be promoted.
#[derive(Debug)]
pub struct ReplicatedQuorumLeaseAuthority {
    paths: BTreeMap<String, PathBuf>,
    available: BTreeSet<String>,
    authority: QuorumLeaseAuthority,
    revision: u64,
    lock_path: PathBuf,
}

impl ReplicatedQuorumLeaseAuthority {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn open(
        replicas: impl IntoIterator<Item = (String, PathBuf)>,
        members: impl IntoIterator<Item = String>,
    ) -> Result<Self, PersistedAuthorityError> {
        let paths = replicas.into_iter().collect::<BTreeMap<_, _>>();
        let expected = members.into_iter().collect::<BTreeSet<_>>();
        let available = paths.keys().cloned().collect::<BTreeSet<_>>();
        Self::open_with_available(paths, expected, available)
    }

    /// Open the local replicated adapter with an explicit availability view.
    ///
    /// Availability is deliberately supplied by the deployment/fault
    /// harness; it is not inferred from file existence. This prevents a
    /// restarted process from treating a previously failed member as live
    /// merely because its old document is still on disk.
    pub fn open_with_available(
        paths: BTreeMap<String, PathBuf>,
        expected: BTreeSet<String>,
        available: BTreeSet<String>,
    ) -> Result<Self, PersistedAuthorityError> {
        if paths.is_empty() || paths.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(PersistedAuthorityError::Invalid(
                "replica members must exactly match configured quorum members".to_owned(),
            ));
        }
        if !available.is_subset(&expected) {
            return Err(PersistedAuthorityError::Invalid(
                "available replicas must be configured quorum members".to_owned(),
            ));
        }
        let lock_path = paths
            .values()
            .next()
            .expect("non-empty replica paths")
            .with_extension("quorum.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let result = (|| {
            let documents = read_replicated_documents(&paths, &available)?;
            let (authority, revision) = if documents.is_empty() {
                (QuorumLeaseAuthority::new(expected.iter().cloned())?, 0)
            } else {
                select_committed_authority(&documents, expected.len())?
            };
            authority.validate_loaded()?;
            Ok((authority, revision))
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        let (authority, revision) = match (result, unlock) {
            (Ok(value), Ok(())) => value,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        let instance = Self {
            paths,
            available,
            authority,
            revision,
            lock_path,
        };
        if instance.revision == 0 {
            instance.persist_quorum(&instance.authority, 0)?;
        }
        Ok(instance)
    }

    pub fn authority(&self) -> &QuorumLeaseAuthority {
        &self.authority
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Return the immutable replica binding required by a shard worker when
    /// validating its lease. The paths are configuration, not authority
    /// state; the authority itself is reopened on every worker commit so a
    /// stale process observes the latest majority decision.
    pub fn replica_binding(&self) -> (Vec<(String, PathBuf)>, Vec<String>) {
        (
            self.paths
                .iter()
                .map(|(member, path)| (member.clone(), path.clone()))
                .collect(),
            self.paths.keys().cloned().collect(),
        )
    }

    pub fn set_member_available(
        &mut self,
        member: &str,
        is_available: bool,
    ) -> Result<(), PersistedAuthorityError> {
        if !self.paths.contains_key(member) {
            return Err(PersistedAuthorityError::Quorum(QuorumError::UnknownMember(
                member.to_owned(),
            )));
        }
        if is_available {
            self.available.insert(member.to_owned());
        } else {
            self.available.remove(member);
        }
        Ok(())
    }

    pub fn issue_lease(
        &mut self,
        shard_id: ShardId,
        node_id: impl Into<String>,
    ) -> Result<ShardLease, PersistedAuthorityError> {
        self.with_locked_update(|authority| authority.issue_lease(shard_id, node_id))
    }

    /// Issue one shared initial term for a complete brain lease set.  A
    /// per-shard loop would create mixed source terms and make a later
    /// brain-wide promotion impossible to prove atomic.
    pub fn issue_leases(
        &mut self,
        shard_ids: impl IntoIterator<Item = ShardId>,
        node_id: impl Into<String>,
    ) -> Result<BTreeMap<ShardId, ShardLease>, PersistedAuthorityError> {
        let shard_ids = shard_ids.into_iter().collect::<Vec<_>>();
        let node_id = node_id.into();
        self.with_locked_update(|authority| authority.issue_leases(&shard_ids, node_id))
    }

    /// Fence all source writers and issue one shared destination term for a
    /// brain-wide migration. Validation and publication happen in one quorum
    /// update, so a concurrent writer cannot pass source validation between
    /// individual shard lease updates.
    pub fn promote_leases(
        &mut self,
        requests: Vec<LeasePromotionRequest>,
    ) -> Result<BTreeMap<ShardId, ShardLease>, PersistedAuthorityError> {
        self.with_locked_update(|authority| authority.promote_leases(&requests))
    }

    /// Revoke a previously issued brain-wide lease set with one term update.
    /// This is used when destination materialisation fails after promotion.
    pub fn revoke_leases(
        &mut self,
        shard_ids: Vec<ShardId>,
    ) -> Result<(), PersistedAuthorityError> {
        self.with_locked_update(|authority| authority.revoke_leases(&shard_ids))
    }

    pub fn revoke(&mut self, shard_id: ShardId) -> Result<(), PersistedAuthorityError> {
        self.with_locked_update(|authority| authority.revoke(shard_id))
    }

    pub fn validate_current(
        &self,
        shard_id: ShardId,
        node_id: &str,
        term: LeaseTerm,
        fencing_token: u64,
    ) -> Result<(), PersistedAuthorityError> {
        let documents = read_replicated_documents(&self.paths, &self.available)?;
        let (authority, _) = select_committed_authority(&documents, self.paths.len())?;
        authority
            .validate(shard_id, node_id, term, fencing_token)
            .map_err(PersistedAuthorityError::from)
    }

    fn with_locked_update<T>(
        &mut self,
        update: impl FnOnce(&mut QuorumLeaseAuthority) -> Result<T, QuorumError>,
    ) -> Result<T, PersistedAuthorityError> {
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let result = (|| {
            let documents = read_replicated_documents(&self.paths, &self.available)?;
            let (current, revision) = if documents.is_empty() {
                (self.authority.clone(), self.revision)
            } else {
                select_committed_authority(&documents, self.paths.len())?
            };
            let mut candidate = current;
            let value = update(&mut candidate)?;
            let next_revision = revision.checked_add(1).ok_or_else(|| {
                PersistedAuthorityError::Invalid("authority revision exhausted".to_owned())
            })?;
            self.persist_quorum(&candidate, next_revision)?;
            self.authority = candidate;
            self.revision = next_revision;
            Ok(value)
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    fn persist_quorum(
        &self,
        authority: &QuorumLeaseAuthority,
        revision: u64,
    ) -> Result<(), PersistedAuthorityError> {
        let document = ReplicatedAuthorityDocument {
            schema_version: Self::SCHEMA_VERSION,
            revision,
            authority: authority.clone(),
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
        let required = self.paths.len() / 2 + 1;
        let mut successes = 0;
        let mut last_error = None;
        for member in &self.available {
            let Some(path) = self.paths.get(member) else {
                continue;
            };
            match atomic_replace_authority(path, &bytes) {
                Ok(()) => successes += 1,
                Err(error) => last_error = Some(error),
            }
        }
        if successes < required {
            return Err(last_error.unwrap_or(PersistedAuthorityError::Quorum(
                QuorumError::QuorumUnavailable {
                    available: successes,
                    members: self.paths.len(),
                },
            )));
        }
        Ok(())
    }
}

fn read_replicated_documents(
    paths: &BTreeMap<String, PathBuf>,
    available: &BTreeSet<String>,
) -> Result<Vec<ReplicatedAuthorityDocument>, PersistedAuthorityError> {
    let mut documents = Vec::new();
    for member in available {
        let Some(path) = paths.get(member) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let bytes =
            fs::read(path).map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let document: ReplicatedAuthorityDocument = serde_json::from_slice(&bytes)
            .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
        if document.schema_version != ReplicatedQuorumLeaseAuthority::SCHEMA_VERSION {
            return Err(PersistedAuthorityError::Invalid(
                "unsupported replicated authority schema version".to_owned(),
            ));
        }
        document.authority.validate_loaded()?;
        documents.push(document);
    }
    Ok(documents)
}

fn select_committed_authority(
    documents: &[ReplicatedAuthorityDocument],
    member_count: usize,
) -> Result<(QuorumLeaseAuthority, u64), PersistedAuthorityError> {
    let required = member_count / 2 + 1;
    let mut candidates = BTreeMap::<Vec<u8>, (usize, u64, QuorumLeaseAuthority)>::new();
    for document in documents {
        let encoded = serde_json::to_vec(&document.authority)
            .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
        let entry = candidates
            .entry(encoded)
            .or_insert_with(|| (0, document.revision, document.authority.clone()));
        entry.0 += 1;
        entry.1 = entry.1.max(document.revision);
    }
    candidates
        .into_values()
        .filter(|(count, _, _)| *count >= required)
        .max_by_key(|(_, revision, _)| *revision)
        .map(|(_, revision, authority)| (authority, revision))
        .ok_or(PersistedAuthorityError::Quorum(
            QuorumError::QuorumUnavailable {
                available: documents.len(),
                members: member_count,
            },
        ))
}

impl QuorumLeaseAuthority {
    pub fn new<I, S>(members: I) -> Result<Self, QuorumError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut member_set = BTreeSet::new();
        for member in members {
            let member = member.into();
            if member.trim().is_empty() {
                return Err(QuorumError::EmptyMember);
            }
            if !member_set.insert(member.clone()) {
                return Err(QuorumError::DuplicateMember(member));
            }
        }
        if member_set.is_empty() {
            return Err(QuorumError::QuorumUnavailable {
                available: 0,
                members: 0,
            });
        }
        let available = member_set.clone();
        Ok(Self {
            members: member_set,
            available,
            current_term: LeaseTerm::INITIAL,
            leases: BTreeMap::new(),
        })
    }

    pub fn set_member_available(
        &mut self,
        member: &str,
        is_available: bool,
    ) -> Result<(), QuorumError> {
        if !self.members.contains(member) {
            return Err(QuorumError::UnknownMember(member.to_owned()));
        }
        if is_available {
            self.available.insert(member.to_owned());
        } else {
            self.available.remove(member);
        }
        Ok(())
    }

    pub fn issue_lease(
        &mut self,
        shard_id: ShardId,
        node_id: impl Into<String>,
    ) -> Result<ShardLease, QuorumError> {
        self.require_quorum()?;
        let node_id = node_id.into();
        if !self.members.contains(&node_id) {
            return Err(QuorumError::UnknownLeaseNode(node_id));
        }
        // An empty authority is bootstrapping the initial generation.  Bind
        // all shards to the configured initial term so it matches the
        // initial durable bridge; later lease-set issuance advances the term.
        let term = if self.leases.is_empty() {
            self.current_term
        } else {
            self.next_term()?
        };
        let lease = ShardLease {
            shard_id,
            node_id,
            term,
            fencing_token: term.raw(),
        };
        self.current_term = term;
        self.leases.insert(shard_id, lease.clone());
        Ok(lease)
    }

    fn issue_leases(
        &mut self,
        shard_ids: &[ShardId],
        node_id: String,
    ) -> Result<BTreeMap<ShardId, ShardLease>, QuorumError> {
        self.require_quorum()?;
        if !self.members.contains(&node_id) {
            return Err(QuorumError::UnknownLeaseNode(node_id));
        }
        if shard_ids.is_empty() {
            return Err(QuorumError::EmptyLeaseSet);
        }
        let mut seen = BTreeSet::new();
        if shard_ids.iter().any(|shard| !seen.insert(*shard)) {
            return Err(QuorumError::EmptyLeaseSet);
        }
        let term = if self.leases.is_empty() {
            self.current_term
        } else {
            self.next_term()?
        };
        let leases = shard_ids
            .iter()
            .copied()
            .map(|shard_id| {
                let lease = ShardLease {
                    shard_id,
                    node_id: node_id.clone(),
                    term,
                    fencing_token: term.raw(),
                };
                self.leases.insert(shard_id, lease.clone());
                (shard_id, lease)
            })
            .collect::<BTreeMap<_, _>>();
        self.current_term = term;
        Ok(leases)
    }

    fn promote_leases(
        &mut self,
        requests: &[LeasePromotionRequest],
    ) -> Result<BTreeMap<ShardId, ShardLease>, QuorumError> {
        self.require_quorum()?;
        if requests.is_empty() {
            return Err(QuorumError::EmptyLeaseSet);
        }
        let mut seen = BTreeSet::new();
        for request in requests {
            if !seen.insert(request.shard_id) {
                return Err(QuorumError::Fenced {
                    shard: request.shard_id,
                    expected_term: self.current_term,
                    expected_token: self.current_term.raw(),
                    received_term: request.source_term,
                    received_token: request.source_fencing_token,
                });
            }
            self.validate(
                request.shard_id,
                &request.source_node,
                request.source_term,
                request.source_fencing_token,
            )?;
            if !self.members.contains(&request.destination_node) {
                return Err(QuorumError::UnknownLeaseNode(
                    request.destination_node.clone(),
                ));
            }
        }
        let term = self.next_term()?;
        let mut leases = BTreeMap::new();
        for request in requests {
            let lease = ShardLease {
                shard_id: request.shard_id,
                node_id: request.destination_node.clone(),
                term,
                fencing_token: term.raw(),
            };
            self.leases.insert(request.shard_id, lease.clone());
            leases.insert(request.shard_id, lease);
        }
        self.current_term = term;
        Ok(leases)
    }

    fn revoke_leases(&mut self, shard_ids: &[ShardId]) -> Result<(), QuorumError> {
        self.require_quorum()?;
        if shard_ids.is_empty() {
            return Err(QuorumError::EmptyLeaseSet);
        }
        let mut seen = BTreeSet::new();
        for shard_id in shard_ids {
            if !seen.insert(*shard_id) {
                return Err(QuorumError::MissingLease(*shard_id));
            }
            if !self.leases.contains_key(shard_id) {
                return Err(QuorumError::MissingLease(*shard_id));
            }
        }
        let term = self.next_term()?;
        for shard_id in shard_ids {
            self.leases.remove(shard_id);
        }
        self.current_term = term;
        Ok(())
    }

    pub fn revoke(&mut self, shard_id: ShardId) -> Result<(), QuorumError> {
        self.require_quorum()?;
        if !self.leases.contains_key(&shard_id) {
            return Err(QuorumError::MissingLease(shard_id));
        }
        let term = self.next_term()?;
        self.leases.remove(&shard_id);
        self.current_term = term;
        Ok(())
    }

    pub fn validate(
        &self,
        shard_id: ShardId,
        node_id: &str,
        term: LeaseTerm,
        fencing_token: u64,
    ) -> Result<(), QuorumError> {
        let lease = self
            .leases
            .get(&shard_id)
            .ok_or(QuorumError::MissingLease(shard_id))?;
        if lease.node_id != node_id || lease.term != term || lease.fencing_token != fencing_token {
            return Err(QuorumError::Fenced {
                shard: shard_id,
                expected_term: lease.term,
                expected_token: lease.fencing_token,
                received_term: term,
                received_token: fencing_token,
            });
        }
        Ok(())
    }

    pub const fn current_term(&self) -> LeaseTerm {
        self.current_term
    }

    pub fn lease(&self, shard_id: ShardId) -> Option<&ShardLease> {
        self.leases.get(&shard_id)
    }

    fn require_quorum(&self) -> Result<(), QuorumError> {
        let required = self.members.len() / 2 + 1;
        if self.available.len() < required {
            return Err(QuorumError::QuorumUnavailable {
                available: self.available.len(),
                members: self.members.len(),
            });
        }
        Ok(())
    }

    fn next_term(&self) -> Result<LeaseTerm, QuorumError> {
        self.current_term
            .raw()
            .checked_add(1)
            .and_then(|raw| LeaseTerm::new(raw).ok())
            .ok_or(QuorumError::TermExhausted)
    }

    fn validate_loaded(&self) -> Result<(), PersistedAuthorityError> {
        if self.members.is_empty() || !self.available.is_subset(&self.members) {
            return Err(PersistedAuthorityError::Invalid(
                "member availability is not a subset of membership".to_owned(),
            ));
        }
        if self.leases.values().any(|lease| {
            !self.members.contains(&lease.node_id)
                || lease.fencing_token != lease.term.raw()
                || lease.term > self.current_term
        }) {
            return Err(PersistedAuthorityError::Invalid(
                "persisted lease is outside the authority term or membership".to_owned(),
            ));
        }
        Ok(())
    }
}

fn atomic_replace_authority(path: &Path, bytes: &[u8]) -> Result<(), PersistedAuthorityError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
    let result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, path))
        .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementOrchestrator {
    leader_term: LeaseTerm,
    resource_version: u64,
    next_operation: u64,
    policy: Policy,
    operations: BTreeMap<EventId, Operation>,
    idempotency: BTreeMap<String, EventId>,
    audit: Vec<AuditRecord>,
}

impl ManagementOrchestrator {
    pub fn new(leader_term: LeaseTerm, policy: Policy) -> Self {
        Self {
            leader_term,
            resource_version: 0,
            next_operation: 1,
            policy,
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            audit: Vec::new(),
        }
    }

    pub fn replace_leader_term(&mut self, term: LeaseTerm) {
        if term > self.leader_term {
            self.leader_term = term;
        }
    }

    /// A new fenced leader may safely retry work that was only marked
    /// `Running` by the previous leader. The operation itself is not
    /// completed here; it is returned to the durable queue and claimed once
    /// by the new leader before dispatch.
    fn requeue_running_after_takeover(&mut self) {
        let running = self
            .operations
            .values_mut()
            .filter_map(|operation| {
                if matches!(operation.state, OperationState::Running) {
                    operation.state = OperationState::Pending;
                    Some((
                        operation.id,
                        operation.request_id.clone(),
                        operation.principal.id.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for (operation_id, request_id, principal) in running {
            append_audit_record(
                &mut self.audit,
                AuditRecord {
                    sequence: 0,
                    request_id,
                    principal,
                    operation_id,
                    outcome: "requeued-after-leader-takeover".to_owned(),
                    leader_term: self.leader_term,
                    previous_digest: String::new(),
                    digest: String::new(),
                },
            );
        }
    }

    pub fn submit(
        &mut self,
        principal: Principal,
        capability: Capability,
        context: MutationContext,
        kind: OperationKind,
    ) -> Result<Operation, ManagementError> {
        self.submit_for_brain(principal, capability, context, kind, String::new())
    }

    pub fn submit_for_brain(
        &mut self,
        principal: Principal,
        capability: Capability,
        context: MutationContext,
        kind: OperationKind,
        brain_id: String,
    ) -> Result<Operation, ManagementError> {
        let permitted = if brain_id.trim().is_empty() {
            self.policy.allows(&principal, &capability)
        } else {
            self.policy
                .allows_for_brain(&principal, &brain_id, &capability)
        };
        if !permitted {
            return Err(ManagementError::Forbidden {
                principal: principal.id,
                capability,
            });
        }
        if context.observed_leader_term != self.leader_term {
            return Err(ManagementError::StaleLeader {
                expected: self.leader_term,
                received: context.observed_leader_term,
            });
        }
        if context.idempotency_key.is_empty() {
            return Err(ManagementError::EmptyIdempotencyKey);
        }
        if context.request_id.is_empty() {
            return Err(ManagementError::EmptyRequestId);
        }
        if let Some(existing_id) = self.idempotency.get(&context.idempotency_key) {
            let existing = self
                .operations
                .get(existing_id)
                .expect("idempotency index is consistent");
            if existing.kind != kind
                || existing.principal != principal
                || existing.brain_id != brain_id
            {
                return Err(ManagementError::IdempotencyConflict(
                    context.idempotency_key,
                ));
            }
            return Ok(existing.clone());
        }
        if context.expected_version != self.resource_version {
            return Err(ManagementError::VersionConflict {
                expected: context.expected_version,
                current: self.resource_version,
            });
        }
        let next_operation = self
            .next_operation
            .checked_add(1)
            .ok_or(ManagementError::OperationIdExhausted)?;
        let next_resource_version = self
            .resource_version
            .checked_add(1)
            .ok_or(ManagementError::ResourceVersionOverflow)?;
        let id =
            EventId::new(self.next_operation).map_err(|_| ManagementError::OperationIdExhausted)?;
        let operation = Operation {
            id,
            brain_id,
            principal: principal.clone(),
            idempotency_key: context.idempotency_key.clone(),
            request_id: context.request_id.clone(),
            expected_version: context.expected_version,
            kind,
            state: OperationState::Pending,
        };
        self.idempotency.insert(context.idempotency_key, id);
        self.operations.insert(id, operation.clone());
        self.next_operation = next_operation;
        self.resource_version = next_resource_version;
        append_audit_record(
            &mut self.audit,
            AuditRecord {
                sequence: 0,
                request_id: context.request_id,
                principal: principal.id,
                operation_id: id,
                outcome: "accepted".to_owned(),
                leader_term: self.leader_term,
                previous_digest: String::new(),
                digest: String::new(),
            },
        );
        Ok(operation)
    }

    pub fn transition(
        &mut self,
        operation_id: EventId,
        observed_leader_term: LeaseTerm,
        state: OperationState,
    ) -> Result<(), ManagementError> {
        if observed_leader_term != self.leader_term {
            return Err(ManagementError::StaleLeader {
                expected: self.leader_term,
                received: observed_leader_term,
            });
        }
        let operation = self
            .operations
            .get_mut(&operation_id)
            .ok_or(ManagementError::MissingOperation(operation_id))?;
        let valid = matches!(
            (&operation.state, &state),
            (OperationState::Pending, OperationState::Running)
                | (OperationState::Pending, OperationState::Cancelled)
                | (OperationState::Pending, OperationState::Failed { .. })
                | (OperationState::Running, OperationState::Succeeded)
                | (OperationState::Running, OperationState::Cancelled)
                | (OperationState::Running, OperationState::Failed { .. })
        );
        if !valid {
            return Err(ManagementError::InvalidOperationTransition {
                from: operation.state.clone(),
                to: state,
            });
        }
        operation.state = state;
        append_audit_record(
            &mut self.audit,
            AuditRecord {
                sequence: 0,
                request_id: operation.request_id.clone(),
                principal: operation.principal.id.clone(),
                operation_id,
                outcome: "transitioned".to_owned(),
                leader_term: self.leader_term,
                previous_digest: String::new(),
                digest: String::new(),
            },
        );
        Ok(())
    }

    pub fn operation(&self, operation_id: EventId) -> Option<&Operation> {
        self.operations.get(&operation_id)
    }

    pub fn audit(&self) -> &[AuditRecord] {
        &self.audit
    }

    pub fn verify_audit_integrity(&self) -> Result<(), String> {
        verify_audit_chain(&self.audit)
    }

    pub const fn resource_version(&self) -> u64 {
        self.resource_version
    }

    pub const fn leader_term(&self) -> LeaseTerm {
        self.leader_term
    }

    pub fn allows(&self, principal: &Principal, capability: &Capability) -> bool {
        self.policy.allows(principal, capability)
    }

    pub fn allows_for_brain(
        &self,
        principal: &Principal,
        brain_id: &str,
        capability: &Capability,
    ) -> bool {
        self.policy
            .allows_for_brain(principal, brain_id, capability)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedManagementDocument {
    schema_version: u32,
    state: ManagementOrchestrator,
}

/// Crash-safe management state owner.  Every accepted operation and state
/// transition is written atomically before the caller receives success.  The
/// same lock/replace protocol is used by separate orchestrator processes, so
/// retries after a restart preserve idempotency and resource versions.
#[derive(Debug)]
pub struct PersistedManagementOrchestrator {
    path: PathBuf,
    lock_path: PathBuf,
    state: ManagementOrchestrator,
}

impl PersistedManagementOrchestrator {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn open(
        path: impl Into<PathBuf>,
        leader_term: LeaseTerm,
        policy: Policy,
    ) -> Result<Self, PersistedAuthorityError> {
        let path = path.into();
        let lock_path = path.with_extension("management.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let result = (|| {
            if path.exists() {
                let bytes = fs::read(&path)
                    .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
                let document: PersistedManagementDocument = serde_json::from_slice(&bytes)
                    .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
                if document.schema_version != Self::SCHEMA_VERSION {
                    return Err(PersistedAuthorityError::Invalid(
                        "unsupported management schema version".to_owned(),
                    ));
                }
                if leader_term < document.state.leader_term() {
                    return Err(PersistedAuthorityError::Quorum(QuorumError::Fenced {
                        shard: ShardId::new(1).expect("constant shard identity"),
                        expected_term: document.state.leader_term(),
                        expected_token: document.state.leader_term().raw(),
                        received_term: leader_term,
                        received_token: leader_term.raw(),
                    }));
                }
                let mut state = document.state;
                let was_legacy_audit = state.audit.iter().any(|record| record.sequence == 0);
                if was_legacy_audit {
                    reseal_legacy_audit(&mut state.audit)
                        .map_err(PersistedAuthorityError::Invalid)?;
                }
                verify_audit_chain(&state.audit).map_err(PersistedAuthorityError::Invalid)?;
                if leader_term > state.leader_term() {
                    state.replace_leader_term(leader_term);
                    state.requeue_running_after_takeover();
                    persist_management_state(&path, &state)?;
                }
                if was_legacy_audit && leader_term == state.leader_term() {
                    persist_management_state(&path, &state)?;
                }
                Ok(state)
            } else {
                let state = ManagementOrchestrator::new(leader_term, policy);
                persist_management_state(&path, &state)?;
                Ok(state)
            }
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        let state = match (result, unlock) {
            (Ok(state), Ok(())) => state,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        Ok(Self {
            path,
            lock_path,
            state,
        })
    }

    /// Atomically claim a pending operation for execution. A retry that sees
    /// an already-running or terminal operation must not issue a second
    /// command, preserving idempotency across management processes.
    pub fn claim_pending(
        &mut self,
        operation_id: EventId,
        observed_leader_term: LeaseTerm,
    ) -> Result<bool, PersistedAuthorityError> {
        self.with_locked_update(|orchestrator| {
            let Some(operation) = orchestrator.operation(operation_id) else {
                return Err(PersistedAuthorityError::Management(
                    ManagementError::MissingOperation(operation_id),
                ));
            };
            if !matches!(operation.state, OperationState::Pending) {
                return Ok(false);
            }
            orchestrator
                .transition(operation_id, observed_leader_term, OperationState::Running)
                .map_err(PersistedAuthorityError::from)?;
            Ok(true)
        })
    }

    pub fn state(&self) -> &ManagementOrchestrator {
        &self.state
    }

    pub fn submit(
        &mut self,
        principal: Principal,
        capability: Capability,
        context: MutationContext,
        kind: OperationKind,
    ) -> Result<Operation, PersistedAuthorityError> {
        self.submit_for_brain(principal, capability, context, kind, String::new())
    }

    pub fn submit_for_brain(
        &mut self,
        principal: Principal,
        capability: Capability,
        context: MutationContext,
        kind: OperationKind,
        brain_id: String,
    ) -> Result<Operation, PersistedAuthorityError> {
        self.with_locked_update(|state| {
            state
                .submit_for_brain(principal, capability, context, kind, brain_id)
                .map_err(PersistedAuthorityError::from)
        })
    }

    pub fn transition(
        &mut self,
        operation_id: EventId,
        observed_leader_term: LeaseTerm,
        state: OperationState,
    ) -> Result<(), PersistedAuthorityError> {
        self.with_locked_update(|orchestrator| {
            orchestrator
                .transition(operation_id, observed_leader_term, state)
                .map_err(PersistedAuthorityError::from)
        })
    }

    pub fn operation(&self, operation_id: EventId) -> Option<&Operation> {
        self.state.operation(operation_id)
    }

    pub fn refresh(&mut self) -> Result<(), PersistedAuthorityError> {
        self.state = read_management_state(&self.path)?;
        Ok(())
    }

    fn with_locked_update<T>(
        &mut self,
        update: impl FnOnce(&mut ManagementOrchestrator) -> Result<T, PersistedAuthorityError>,
    ) -> Result<T, PersistedAuthorityError> {
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
        let result = (|| {
            let mut state = if self.path.exists() {
                read_management_state(&self.path)?
            } else {
                self.state.clone()
            };
            let value = update(&mut state)?;
            persist_management_state(&self.path, &state)?;
            self.state = state;
            Ok(value)
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| PersistedAuthorityError::Io(error.to_string()));
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

fn read_management_state(path: &Path) -> Result<ManagementOrchestrator, PersistedAuthorityError> {
    let bytes = fs::read(path).map_err(|error| PersistedAuthorityError::Io(error.to_string()))?;
    let document: PersistedManagementDocument = serde_json::from_slice(&bytes)
        .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
    if document.schema_version != PersistedManagementOrchestrator::SCHEMA_VERSION {
        return Err(PersistedAuthorityError::Invalid(
            "unsupported management schema version".to_owned(),
        ));
    }
    verify_audit_chain(&document.state.audit).map_err(PersistedAuthorityError::Invalid)?;
    Ok(document.state)
}

fn persist_management_state(
    path: &Path,
    state: &ManagementOrchestrator,
) -> Result<(), PersistedAuthorityError> {
    let document = PersistedManagementDocument {
        schema_version: PersistedManagementOrchestrator::SCHEMA_VERSION,
        state: state.clone(),
    };
    let bytes = serde_json::to_vec(&document)
        .map_err(|error| PersistedAuthorityError::Encoding(error.to_string()))?;
    atomic_replace_authority(path, &bytes)
}

/// Generated-contract management service used by orchestrator processes and
/// contract tests. The service owns the policy-bearing orchestrator object;
/// it never exposes a worker handle or accepts an operation without the
/// persisted leader term, resource version and idempotency context.
#[derive(Clone)]
pub struct ManagementGrpcService {
    orchestrator: Arc<Mutex<ManagementOrchestrator>>,
    placement_registries: Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    placement_controllers: Arc<Mutex<BTreeMap<BrainId, PlacementController>>>,
    migration_journals: Arc<Mutex<BTreeMap<BrainId, MigrationJournal>>>,
    migration_dispatcher: Option<MigrationDispatchHandler>,
    placement_activation_dispatcher: Option<PlacementActivationDispatcher>,
    migration_in_flight: Arc<Mutex<BTreeSet<(BrainId, u64)>>>,
}

impl ManagementGrpcService {
    pub fn new(orchestrator: ManagementOrchestrator) -> Self {
        Self::with_migration_dispatcher(orchestrator, None)
    }

    pub fn with_migration_dispatcher(
        orchestrator: ManagementOrchestrator,
        migration_dispatcher: Option<MigrationDispatchHandler>,
    ) -> Self {
        Self::with_dispatchers_and_activation(orchestrator, migration_dispatcher, None)
    }

    pub fn with_dispatchers_and_activation(
        orchestrator: ManagementOrchestrator,
        migration_dispatcher: Option<MigrationDispatchHandler>,
        placement_activation_dispatcher: Option<PlacementActivationDispatcher>,
    ) -> Self {
        Self {
            orchestrator: Arc::new(Mutex::new(orchestrator)),
            placement_registries: Arc::new(Mutex::new(BTreeMap::new())),
            placement_controllers: Arc::new(Mutex::new(BTreeMap::new())),
            migration_journals: Arc::new(Mutex::new(BTreeMap::new())),
            migration_dispatcher,
            placement_activation_dispatcher,
            migration_in_flight: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    pub fn orchestrator(&self) -> Arc<Mutex<ManagementOrchestrator>> {
        Arc::clone(&self.orchestrator)
    }

    /// Accept validated worker registration evidence from the node adapter.
    /// This reference service uses the same registry matching rules as the
    /// secured service so tests cannot accidentally bless a weaker path.
    pub fn record_stable_worker_registration(
        &self,
        target_node: &str,
        registration: &StableWorkerRegistration,
    ) -> Result<usize, Status> {
        record_placement_worker_registration_in_stores(
            &self.placement_registries,
            target_node,
            registration,
        )
    }

    fn reserve_migration_dispatch(&self, operation: &MigrationOperation) -> Result<bool, Status> {
        let Some(_) = self.migration_dispatcher else {
            return Ok(false);
        };
        let mut in_flight = self
            .migration_in_flight
            .lock()
            .map_err(|_| Status::internal("migration dispatcher lock poisoned"))?;
        if in_flight
            .iter()
            .any(|(brain, _)| *brain == operation.brain_id)
        {
            return Err(Status::failed_precondition(
                "another migration for this brain is already executing",
            ));
        }
        in_flight.insert((operation.brain_id, operation.operation_id));
        Ok(true)
    }

    fn release_migration_dispatch(&self, brain_id: BrainId, operation_id: u64) {
        if let Ok(mut in_flight) = self.migration_in_flight.lock() {
            in_flight.remove(&(brain_id, operation_id));
        }
    }

    fn migration_still_dispatchable(&self, operation: &MigrationOperation) -> bool {
        self.migration_journals
            .lock()
            .ok()
            .and_then(|journals| {
                journals
                    .get(&operation.brain_id)
                    .and_then(|journal| journal.operation(operation.operation_id))
                    .map(|current| {
                        matches!(
                            current.phase,
                            MigrationPhase::Prepared | MigrationPhase::RecoveryRequired
                        )
                    })
            })
            .unwrap_or(false)
    }

    fn schedule_migration(&self, operation: MigrationOperation, group: MigrationGroupSpec) {
        let Some(dispatcher) = self.migration_dispatcher.as_ref().cloned() else {
            return;
        };
        let Ok(true) = self.reserve_migration_dispatch(&operation) else {
            return;
        };
        if !self.migration_still_dispatchable(&operation) {
            self.release_migration_dispatch(operation.brain_id, operation.operation_id);
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            let dispatch = dispatcher(operation.clone(), group).await;
            let result = match dispatch {
                Ok(receipt) => service.finalize_migration(operation.clone(), receipt),
                Err(error) => service.fail_migration(operation.clone(), error),
            };
            if let Err(error) = result {
                // The durable journal remains the recovery record.  A
                // completion callback must never panic or rewrite a fenced
                // operation after leadership changes.
                eprintln!(
                    "migration dispatch completion could not update journal: brain={} operation={} error={error}",
                    operation.brain_id.raw(),
                    operation.operation_id
                );
            }
            service.release_migration_dispatch(operation.brain_id, operation.operation_id);
        });
    }

    fn finalize_migration(
        &self,
        operation: MigrationOperation,
        receipt: MigrationDispatchReceipt,
    ) -> Result<(), String> {
        receipt.verify_against(&operation)?;
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| "migration journal lock poisoned".to_owned())?;
        let journal = journals
            .get_mut(&operation.brain_id)
            .ok_or_else(|| "migration journal disappeared during dispatch".to_owned())?;
        advance_journal_to_cutover_ready(journal, &operation, receipt.transferred_bytes)
            .map_err(|error| error.to_string())?;
        journal
            .commit_dispatched_group(
                &receipt.group,
                receipt.cut_tag,
                receipt.transferred_bytes,
                journal.resource_version,
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn fail_migration(&self, operation: MigrationOperation, error: String) -> Result<(), String> {
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| "migration journal lock poisoned".to_owned())?;
        let journal = journals
            .get_mut(&operation.brain_id)
            .ok_or_else(|| "migration journal disappeared during dispatch".to_owned())?;
        let current = journal
            .operation(operation.operation_id)
            .cloned()
            .ok_or_else(|| "migration operation disappeared during dispatch".to_owned())?;
        if current.phase.terminal() {
            return Ok(());
        }
        journal
            .transition(MigrationTransition {
                operation_id: current.operation_id,
                observed_leader_term: journal.leader_term,
                expected_resource_version: journal.resource_version,
                next_phase: MigrationPhase::Failed,
                progress: current.progress,
                error_code: Some(truncate_dispatch_error(error)),
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn validate_management_auth_values(
    production_cutover: bool,
    mode: &str,
    bearer_token: Option<&str>,
    principal: Option<&str>,
    oidc_issuer: Option<&str>,
    oidc_audience: Option<&str>,
    oidc_jwks_file: Option<&str>,
    oidc_revocation_file: Option<&str>,
) -> Result<(), String> {
    let mode = mode.trim().to_ascii_lowercase();
    if production_cutover && !matches!(mode.as_str(), "oidc" | "oidc-jwt") {
        return Err(
            "NM_PRODUCTION_CUTOVER requires NM_MANAGEMENT_AUTH_MODE=oidc-jwt; static bearer authentication is reference-only".to_owned(),
        );
    }
    match mode.as_str() {
        "oidc" | "oidc-jwt" => {
            for (value, name, message) in [
                (
                    oidc_issuer,
                    "NM_OIDC_ISSUER",
                    "OIDC issuer is not configured",
                ),
                (
                    oidc_audience,
                    "NM_OIDC_AUDIENCE",
                    "OIDC audience is not configured",
                ),
                (
                    oidc_jwks_file,
                    "NM_OIDC_JWKS_FILE",
                    "OIDC JWKS file is not configured",
                ),
            ] {
                let value = value.ok_or_else(|| message.to_owned())?;
                if value.trim().is_empty() {
                    return Err(format!("{name} must not be empty"));
                }
            }
            if production_cutover {
                let revocation = oidc_revocation_file.ok_or_else(|| {
                    "NM_PRODUCTION_CUTOVER requires NM_OIDC_REVOCATION_FILE".to_owned()
                })?;
                if revocation.trim().is_empty() {
                    return Err("NM_OIDC_REVOCATION_FILE must not be empty".to_owned());
                }
            }
            Ok(())
        }
        "static-reference" | "static" => {
            let token = bearer_token.ok_or_else(|| {
                "management endpoint requires NM_MANAGEMENT_BEARER_TOKEN in static-reference mode"
                    .to_owned()
            })?;
            if token.trim().is_empty() {
                return Err("NM_MANAGEMENT_BEARER_TOKEN must not be empty".to_owned());
            }
            let principal =
                principal.ok_or_else(|| "management principal is not configured".to_owned())?;
            if principal.trim().is_empty() {
                return Err("management principal is empty".to_owned());
            }
            Ok(())
        }
        mode => Err(format!(
            "unsupported NM_MANAGEMENT_AUTH_MODE '{mode}'; use oidc-jwt or static-reference"
        )),
    }
}

/// Transport-level authentication for the generated management endpoint.
///
/// The endpoint is disabled unless the deployment supplies a bearer token;
/// request `principal_id` remains an authorisation subject, never an identity
/// proof. Production deployments should put this interceptor behind the
/// existing OIDC/mTLS gateway and rotate the token through secret management.
pub fn validate_management_auth_config() -> Result<(), String> {
    let mode =
        std::env::var("NM_MANAGEMENT_AUTH_MODE").unwrap_or_else(|_| "static-reference".to_owned());
    validate_management_auth_values(
        production_cutover_enabled(),
        &mode,
        std::env::var("NM_MANAGEMENT_BEARER_TOKEN").ok().as_deref(),
        std::env::var("NM_MANAGEMENT_PRINCIPAL").ok().as_deref(),
        std::env::var("NM_OIDC_ISSUER").ok().as_deref(),
        std::env::var("NM_OIDC_AUDIENCE").ok().as_deref(),
        std::env::var("NM_OIDC_JWKS_FILE").ok().as_deref(),
        std::env::var("NM_OIDC_REVOCATION_FILE").ok().as_deref(),
    )
}

/// Return whether the deployment has explicitly requested production
/// cutover.  This is intentionally separate from the migration feature flags:
/// compiling a reference adapter does not make it safe to promote.
pub fn production_cutover_enabled() -> bool {
    matches!(
        std::env::var("NM_PRODUCTION_CUTOVER")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn validate_production_oidc_files(jwks_file: &str, revocation_file: &str) -> Result<(), String> {
    let jwks_metadata = fs::metadata(jwks_file)
        .map_err(|error| format!("cannot stat NM_OIDC_JWKS_FILE: {error}"))?;
    if !jwks_metadata.is_file() {
        return Err("NM_OIDC_JWKS_FILE must name a regular file".to_owned());
    }
    let jwks_bytes =
        fs::read(jwks_file).map_err(|error| format!("cannot read NM_OIDC_JWKS_FILE: {error}"))?;
    let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(&jwks_bytes)
        .map_err(|error| format!("cannot decode NM_OIDC_JWKS_FILE: {error}"))?;
    if jwks.keys.is_empty() {
        return Err("NM_OIDC_JWKS_FILE must contain at least one key".to_owned());
    }

    let revocation_metadata = fs::metadata(revocation_file)
        .map_err(|error| format!("cannot stat NM_OIDC_REVOCATION_FILE: {error}"))?;
    if !revocation_metadata.is_file() {
        return Err("NM_OIDC_REVOCATION_FILE must name a regular file".to_owned());
    }
    fs::read_to_string(revocation_file)
        .map_err(|error| format!("cannot read NM_OIDC_REVOCATION_FILE: {error}"))?;
    Ok(())
}

/// Validate the management prerequisites that must hold before an
/// orchestrator can expose a production control plane.  The generated
/// service and browser gateway both use this guard at startup; request-level
/// authentication remains fail-closed as a second boundary.
pub fn validate_production_management_config() -> Result<(), String> {
    if !production_cutover_enabled() {
        return Ok(());
    }
    validate_management_auth_config()?;
    required_management_grpc_tls()?;
    let state_path = std::env::var("NM_MANAGEMENT_STATE_PATH")
        .map_err(|_| "NM_PRODUCTION_CUTOVER requires NM_MANAGEMENT_STATE_PATH".to_owned())?;
    if state_path.trim().is_empty() {
        return Err("NM_MANAGEMENT_STATE_PATH must not be empty".to_owned());
    }
    let jwks_file = std::env::var("NM_OIDC_JWKS_FILE")
        .map_err(|_| "NM_PRODUCTION_CUTOVER requires NM_OIDC_JWKS_FILE".to_owned())?;
    let revocation_file = std::env::var("NM_OIDC_REVOCATION_FILE")
        .map_err(|_| "NM_PRODUCTION_CUTOVER requires NM_OIDC_REVOCATION_FILE".to_owned())?;
    validate_production_oidc_files(&jwks_file, &revocation_file)?;
    if std::env::var("NM_GRPC_TLS_DOMAIN")
        .ok()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err("NM_PRODUCTION_CUTOVER requires NM_GRPC_TLS_DOMAIN".to_owned());
    }
    Ok(())
}

/// Resolve the listener's mutual-TLS configuration from deployment-managed
/// PEM files. The gRPC socket carries both management and internal data-plane
/// services, so the identity policy applies to every service on that socket.
/// An incomplete configuration is an error rather than a downgrade to
/// plaintext.
pub fn configured_grpc_server_tls() -> Result<Option<ServerTlsConfig>, String> {
    let cert = std::env::var("NM_GRPC_TLS_CERT").ok();
    let key = std::env::var("NM_GRPC_TLS_KEY").ok();
    let ca = std::env::var("NM_GRPC_TLS_CA").ok();
    match (cert, key, ca) {
        (None, None, None) => Ok(None),
        (Some(cert), Some(key), Some(ca))
            if !cert.trim().is_empty() && !key.trim().is_empty() && !ca.trim().is_empty() =>
        {
            let cert = std::fs::read(&cert)
                .map_err(|error| format!("cannot read NM_GRPC_TLS_CERT: {error}"))?;
            let key = std::fs::read(&key)
                .map_err(|error| format!("cannot read NM_GRPC_TLS_KEY: {error}"))?;
            let ca = std::fs::read(&ca)
                .map_err(|error| format!("cannot read NM_GRPC_TLS_CA: {error}"))?;
            Ok(Some(
                ServerTlsConfig::new()
                    .identity(Identity::from_pem(cert, key))
                    .client_ca_root(Certificate::from_pem(ca)),
            ))
        }
        _ => Err(
            "NM_GRPC_TLS_CERT, NM_GRPC_TLS_KEY and NM_GRPC_TLS_CA must be configured together"
                .to_owned(),
        ),
    }
}

/// Management-enabled startup uses this strict form. The bearer token is an
/// authorisation hint only; authenticated transport identity is mandatory.
pub fn required_management_grpc_tls() -> Result<ServerTlsConfig, String> {
    configured_grpc_server_tls()?.ok_or_else(|| {
        "management_v1 requires NM_GRPC_TLS_CERT, NM_GRPC_TLS_KEY and NM_GRPC_TLS_CA".to_owned()
    })
}

/// Centralise internal client endpoint construction so distributed, causal
/// and management clients use one certificate policy. With no TLS variables
/// this preserves the explicit local/reference profile; a partial set fails
/// closed instead of silently selecting plaintext.
pub fn grpc_client_endpoint(target: &str) -> Result<Endpoint, String> {
    let tls_paths =
        match (
            std::env::var("NM_GRPC_TLS_CERT").ok(),
            std::env::var("NM_GRPC_TLS_KEY").ok(),
            std::env::var("NM_GRPC_TLS_CA").ok(),
        ) {
            (None, None, None) => None,
            (Some(cert), Some(key), Some(ca))
                if !cert.trim().is_empty() && !key.trim().is_empty() && !ca.trim().is_empty() =>
            {
                Some((cert, key, ca))
            }
            _ => return Err(
                "NM_GRPC_TLS_CERT, NM_GRPC_TLS_KEY and NM_GRPC_TLS_CA must be configured together"
                    .to_owned(),
            ),
        };

    let Some((cert_path, key_path, ca_path)) = tls_paths else {
        return Endpoint::from_shared(target.to_owned())
            .map_err(|error| format!("invalid gRPC endpoint: {error}"));
    };
    let cert = std::fs::read(cert_path)
        .map_err(|error| format!("cannot read gRPC client certificate: {error}"))?;
    let key =
        std::fs::read(key_path).map_err(|error| format!("cannot read gRPC client key: {error}"))?;
    let ca =
        std::fs::read(ca_path).map_err(|error| format!("cannot read gRPC client CA: {error}"))?;
    let secure_target = if let Some(rest) = target.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        target.to_owned()
    };
    let domain = std::env::var("NM_GRPC_TLS_DOMAIN")
        .map_err(|_| "NM_GRPC_TLS_DOMAIN is required when gRPC TLS is enabled".to_owned())?;
    Endpoint::from_shared(secure_target)
        .map_err(|error| format!("invalid secure gRPC endpoint: {error}"))?
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(ca))
                .identity(Identity::from_pem(cert, key))
                .domain_name(domain),
        )
        .map_err(|error| format!("invalid gRPC TLS configuration: {error}"))
}

pub fn management_auth_interceptor(request: Request<()>) -> Result<Request<()>, Status> {
    validate_management_auth_config().map_err(Status::unauthenticated)?;
    let supplied = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok());
    let supplied = supplied
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Status::unauthenticated("Bearer management credentials are required"))?;
    let mode =
        std::env::var("NM_MANAGEMENT_AUTH_MODE").unwrap_or_else(|_| "static-reference".to_owned());
    let principal = match mode.trim().to_ascii_lowercase().as_str() {
        "oidc" | "oidc-jwt" => verify_oidc_management_token(supplied).map_err(|error| {
            Status::unauthenticated(format!("invalid OIDC credentials: {error}"))
        })?,
        "static-reference" | "static" => {
            let expected = std::env::var("NM_MANAGEMENT_BEARER_TOKEN")
                .map_err(|error| Status::unauthenticated(error.to_string()))?;
            if supplied != expected {
                return Err(Status::unauthenticated("invalid management credentials"));
            }
            std::env::var("NM_MANAGEMENT_PRINCIPAL")
                .map_err(|error| Status::unauthenticated(error.to_string()))?
        }
        _ => return Err(Status::unauthenticated("unsupported management auth mode")),
    };
    let mut request = request;
    request
        .extensions_mut()
        .insert(AuthenticatedPrincipal(principal));
    Ok(request)
}

#[derive(Debug, Deserialize)]
struct OidcManagementClaims {
    sub: String,
    #[serde(rename = "exp")]
    _exp: usize,
    #[serde(rename = "iss")]
    _iss: String,
    #[serde(rename = "aud")]
    _aud: serde_json::Value,
    #[serde(default)]
    jti: Option<String>,
}

/// Verify a signed OIDC bearer token at the management transport boundary.
/// The JWK set is reread for every request so key rotation takes effect
/// without restarting the orchestrator. Only asymmetric provider keys are
/// accepted; a shared HMAC secret would make a verifier/operator credential
/// interchangeable with an issuer signing key.
fn verify_oidc_management_token(token: &str) -> Result<String, String> {
    let issuer = std::env::var("NM_OIDC_ISSUER").map_err(|_| "issuer is missing".to_owned())?;
    let audience =
        std::env::var("NM_OIDC_AUDIENCE").map_err(|_| "audience is missing".to_owned())?;
    let jwks_file =
        std::env::var("NM_OIDC_JWKS_FILE").map_err(|_| "JWK set file is missing".to_owned())?;
    let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(
        &fs::read(&jwks_file).map_err(|error| format!("cannot read JWK set: {error}"))?,
    )
    .map_err(|error| format!("cannot decode JWK set: {error}"))?;
    verify_oidc_management_token_with_jwks(token, &issuer, &audience, &jwks)
}

fn verify_oidc_management_token_with_jwks(
    token: &str,
    issuer: &str,
    audience: &str,
    jwks: &jsonwebtoken::jwk::JwkSet,
) -> Result<String, String> {
    let header = jsonwebtoken::decode_header(token)
        .map_err(|error| format!("cannot decode token header: {error}"))?;
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| "token key ID is missing".to_owned())?;
    let jwk = jwks
        .find(kid)
        .ok_or_else(|| "token key ID is not present in the configured JWK set".to_owned())?;
    let key_algorithm = jwk
        .common
        .key_algorithm
        .ok_or_else(|| "JWK algorithm is missing".to_owned())?;
    let algorithm = match key_algorithm {
        jsonwebtoken::jwk::KeyAlgorithm::ES256 => jsonwebtoken::Algorithm::ES256,
        jsonwebtoken::jwk::KeyAlgorithm::ES384 => jsonwebtoken::Algorithm::ES384,
        jsonwebtoken::jwk::KeyAlgorithm::RS256 => jsonwebtoken::Algorithm::RS256,
        jsonwebtoken::jwk::KeyAlgorithm::RS384 => jsonwebtoken::Algorithm::RS384,
        jsonwebtoken::jwk::KeyAlgorithm::RS512 => jsonwebtoken::Algorithm::RS512,
        jsonwebtoken::jwk::KeyAlgorithm::PS256 => jsonwebtoken::Algorithm::PS256,
        jsonwebtoken::jwk::KeyAlgorithm::PS384 => jsonwebtoken::Algorithm::PS384,
        jsonwebtoken::jwk::KeyAlgorithm::PS512 => jsonwebtoken::Algorithm::PS512,
        jsonwebtoken::jwk::KeyAlgorithm::EdDSA => jsonwebtoken::Algorithm::EdDSA,
        jsonwebtoken::jwk::KeyAlgorithm::HS256
        | jsonwebtoken::jwk::KeyAlgorithm::HS384
        | jsonwebtoken::jwk::KeyAlgorithm::HS512 => {
            return Err("symmetric JWKs are not accepted for OIDC management".to_owned());
        }
        _ => return Err("unsupported JWK algorithm".to_owned()),
    };
    if header.alg != algorithm {
        return Err("token and JWK algorithms do not match".to_owned());
    }
    let key = jsonwebtoken::DecodingKey::from_jwk(jwk)
        .map_err(|error| format!("invalid JWK: {error}"))?;
    let mut validation = jsonwebtoken::Validation::new(algorithm);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let claims = jsonwebtoken::decode::<OidcManagementClaims>(token, &key, &validation)
        .map_err(|error| format!("token verification failed: {error}"))?
        .claims;
    if claims.sub.trim().is_empty() {
        return Err("OIDC subject is empty".to_owned());
    }
    if let Some(path) = std::env::var_os("NM_OIDC_REVOCATION_FILE") {
        let contents = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read OIDC revocation file: {error}"))?;
        if oidc_credential_is_revoked(&contents, &claims.sub, claims.jti.as_deref()) {
            return Err("OIDC credential has been revoked".to_owned());
        }
    }
    Ok(claims.sub)
}

fn oidc_credential_is_revoked(contents: &str, subject: &str, jti: Option<&str>) -> bool {
    let revoked = contents
        .lines()
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !entry.starts_with('#'))
        .collect::<BTreeSet<_>>();
    revoked.contains(subject) || jti.is_some_and(|value| revoked.contains(value))
}

/// Principal established by the authenticated transport boundary.  A
/// management request's user-supplied `principal_id` is never accepted as
/// proof of identity by the secured service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal(pub String);

fn authenticated_principal<T>(request: &Request<T>) -> Result<&str, Status> {
    request
        .extensions()
        .get::<AuthenticatedPrincipal>()
        .map(|principal| principal.0.as_str())
        .filter(|principal| !principal.trim().is_empty())
        .ok_or_else(|| Status::unauthenticated("authenticated principal is required"))
}

fn operation_state_name(state: &OperationState) -> (String, String) {
    match state {
        OperationState::Pending => ("pending".to_owned(), String::new()),
        OperationState::Running => ("running".to_owned(), String::new()),
        OperationState::Succeeded => ("succeeded".to_owned(), String::new()),
        OperationState::Cancelled => ("cancelled".to_owned(), String::new()),
        OperationState::Failed { code } => ("failed".to_owned(), code.clone()),
    }
}

fn management_status(error: ManagementError) -> Status {
    match error {
        ManagementError::Forbidden { .. } => Status::permission_denied(error.to_string()),
        ManagementError::StaleLeader { .. } | ManagementError::VersionConflict { .. } => {
            Status::failed_precondition(error.to_string())
        }
        _ => Status::invalid_argument(error.to_string()),
    }
}

fn decode_placement_plan_command(
    command_json: &str,
) -> Result<
    (
        crate::placement::PlacementCommand,
        crate::placement::PlacementPlan,
    ),
    Status,
> {
    if command_json.len() > 2 * 1024 * 1024 {
        return Err(Status::resource_exhausted(
            "placement command exceeds the bounded request size",
        ));
    }
    let command: crate::placement::PlacementCommand = serde_json::from_str(command_json)
        .map_err(|error| Status::invalid_argument(format!("invalid placement command: {error}")))?;
    command
        .verify()
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    let crate::placement::PlacementCommandKind::PlanPlacement(request) = &command.kind else {
        return Err(Status::invalid_argument(
            "PlanPlacement accepts only a PlanPlacement command",
        ));
    };
    let plan = crate::placement::PlacementPlanner
        .plan(request.clone())
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    Ok((command, plan))
}

fn placement_plan_response(
    command: &crate::placement::PlacementCommand,
    plan: &crate::placement::PlacementPlan,
) -> Result<crate::generated_management::proto::PlacementCommandResponse, Status> {
    Ok(
        crate::generated_management::proto::PlacementCommandResponse {
            schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
            command_digest: command.digest().to_string(),
            plan_json: serde_json::to_string(plan).map_err(|error| {
                Status::internal(format!("placement plan encoding failed: {error}"))
            })?,
            error_code: String::new(),
        },
    )
}

/// Review an immutable planner result through the brain-scoped automatic
/// placement controller. The planner proves that a candidate fits; this
/// second boundary proves that automation may consider it now.
fn review_placement_plan(
    controllers: &mut BTreeMap<BrainId, PlacementController>,
    command: &PlacementCommand,
    plan: &PlacementPlan,
    current_plan: Option<PlacementPlan>,
    active_migrations: u16,
) -> Result<PlacementReview, Status> {
    let PlacementCommandKind::PlanPlacement(request) = &command.kind else {
        return Err(Status::invalid_argument(
            "placement controller accepts only planning commands",
        ));
    };
    let controller = placement_controller_entry(controllers, command.brain_id)?;
    if let Some(current_plan) = current_plan {
        if controller.active_plan.as_ref().map(PlacementPlan::digest) != Some(current_plan.digest())
        {
            controller
                .adopt(current_plan)
                .map_err(placement_controller_status)?;
        }
    }
    let demands = request
        .demands
        .iter()
        .cloned()
        .map(|demand| (demand.shard_id, demand))
        .collect::<BTreeMap<_, ShardDemand>>();
    controller
        .review(
            plan,
            &demands,
            &request.resources,
            plan.effective_tag,
            active_migrations,
        )
        .map_err(placement_controller_status)
}

fn placement_controller_entry(
    controllers: &mut BTreeMap<BrainId, PlacementController>,
    brain_id: BrainId,
) -> Result<&mut PlacementController, Status> {
    if !controllers.contains_key(&brain_id) {
        let controller = PlacementController::new(AutomaticPlacementPolicy::default())
            .map_err(placement_controller_status)?;
        controllers.insert(brain_id, controller);
    }
    controllers
        .get_mut(&brain_id)
        .ok_or_else(|| Status::internal("placement controller was not retained"))
}

fn placement_controller_status(error: PlacementControllerError) -> Status {
    match error {
        PlacementControllerError::Blocked(reason) => Status::failed_precondition(reason),
        e @ (PlacementControllerError::IdentityMismatch
        | PlacementControllerError::TagRegression
        | PlacementControllerError::RepartitionRequired) => {
            Status::failed_precondition(e.to_string())
        }
        PlacementControllerError::MissingDemand(shard) => {
            Status::invalid_argument(format!("placement demand is missing for shard {shard}"))
        }
        PlacementControllerError::InvalidState => {
            Status::internal("placement controller state is invalid")
        }
        other => Status::invalid_argument(other.to_string()),
    }
}

fn active_migration_count(in_flight: &BTreeSet<(BrainId, u64)>, brain_id: BrainId) -> u16 {
    in_flight
        .iter()
        .filter(|(brain, _)| *brain == brain_id)
        .count()
        .min(u16::MAX as usize) as u16
}

fn configured_placement_registry_path(brain_id: BrainId) -> Option<PathBuf> {
    std::env::var("NM_PLACEMENT_REGISTRY_DIR")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(|root| PathBuf::from(root).join(format!("brain-{}.json", brain_id.raw())))
}

/// Recover the last authoritative plan before reviewing a new automatic
/// proposal. An absent file means this process has not published a plan;
/// malformed or stale state fails closed instead of being treated as an
/// initial placement.
fn persisted_active_placement_plan(
    brain_id: BrainId,
    leader_term: LeaseTerm,
) -> Result<Option<PlacementPlan>, Status> {
    let Some(path) = configured_placement_registry_path(brain_id) else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let persisted = PersistedPlacementRegistry::open(path, brain_id, leader_term)
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    Ok(persisted.state().active_plan.clone())
}

fn decode_placement_apply_request(
    request: &crate::generated_management::proto::PlacementCommandRequest,
) -> Result<
    (
        crate::placement::PlacementCommand,
        PlacementApplyRequest,
        Option<StableWorkerActivationCommand>,
    ),
    Status,
> {
    if request.command_json.len() > 2 * 1024 * 1024
        || request.cutover_json.len() > 2 * 1024 * 1024
        || request.repartition_json.len() > 2 * 1024 * 1024
        || request.stable_worker_activation_json.len()
            > crate::stable_worker::MAX_STABLE_WORKER_ACTIVATION_BYTES
    {
        return Err(Status::resource_exhausted(
            "placement apply request exceeds the bounded request size",
        ));
    }
    let command: crate::placement::PlacementCommand = serde_json::from_str(&request.command_json)
        .map_err(|error| {
        Status::invalid_argument(format!("invalid placement command: {error}"))
    })?;
    command
        .verify()
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    let crate::placement::PlacementCommandKind::ApplyPlacement(plan) = &command.kind else {
        return Err(Status::invalid_argument(
            "ApplyPlacement accepts only an ApplyPlacement command",
        ));
    };
    let cutover = if request.cutover_json.trim().is_empty() {
        None
    } else {
        Some(
            serde_json::from_str::<CutoverEvidence>(&request.cutover_json).map_err(|error| {
                Status::invalid_argument(format!("invalid cutover evidence: {error}"))
            })?,
        )
    };
    let repartition = if request.repartition_json.trim().is_empty() {
        None
    } else {
        Some(
            serde_json::from_str(&request.repartition_json).map_err(|error| {
                Status::invalid_argument(format!("invalid repartition plan: {error}"))
            })?,
        )
    };
    let activation = if request.stable_worker_activation_json.trim().is_empty() {
        None
    } else {
        let mut activation = serde_json::from_str::<StableWorkerActivationCommand>(
            &request.stable_worker_activation_json,
        )
        .map_err(|error| {
            Status::invalid_argument(format!("invalid stable worker activation: {error}"))
        })?;
        activation
            .verify()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        if activation.brain_id != command.brain_id.raw() {
            return Err(Status::invalid_argument(
                "stable worker activation brain does not match placement command",
            ));
        }
        activation
            .bind_placement_idempotency_key(command.idempotency_key.clone())
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Some(activation)
    };
    Ok((
        command.clone(),
        PlacementApplyRequest {
            request_id: command.request_id,
            idempotency_key: command.idempotency_key,
            expected_resource_version: command.expected_resource_version,
            observed_leader_term: command.observed_leader_term,
            plan: plan.clone(),
            cutover,
            repartition,
        },
        activation,
    ))
}

fn validate_activation_target(
    activation: &StableWorkerActivationCommand,
    plan: &crate::placement::PlacementPlan,
) -> Result<(), Status> {
    if plan
        .placements
        .iter()
        .any(|placement| placement.active_node == activation.target_node)
    {
        return Ok(());
    }
    Err(Status::invalid_argument(
        "stable worker activation target is not an active placement in the applied plan",
    ))
}

fn apply_placement_registry(
    registries: &mut BTreeMap<BrainId, PlacementRegistry>,
    command: &crate::placement::PlacementCommand,
    request: PlacementApplyRequest,
    prepare: bool,
) -> Result<
    (
        crate::placement_registry::PlacementApplyReceipt,
        PlacementRegistry,
    ),
    Status,
> {
    let registry = registries
        .entry(command.brain_id)
        .or_insert_with(|| PlacementRegistry::new(command.brain_id, command.observed_leader_term));
    if command.observed_leader_term > registry.leader_term {
        registry
            .set_leader_term(command.observed_leader_term)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
    }
    let receipt = if prepare {
        registry.prepare(request)
    } else {
        registry.apply(request)
    }
    .map_err(|error: PlacementRegistryError| Status::failed_precondition(error.to_string()))?;
    Ok((receipt, registry.clone()))
}

fn abort_placement_prepared(
    placement_registries: &Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    brain_id: BrainId,
    leader_term: LeaseTerm,
) -> Result<(), Status> {
    if let Some(path) = configured_placement_registry_path(brain_id) {
        let mut persisted = PersistedPlacementRegistry::open(path, brain_id, leader_term)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        match persisted.abort_prepared() {
            Ok(()) | Err(PlacementRegistryError::NoPreparedPlacement) => Ok(()),
            Err(error) => Err(Status::failed_precondition(error.to_string())),
        }
    } else {
        let mut registries = placement_registries
            .lock()
            .map_err(|_| Status::internal("placement registry lock poisoned"))?;
        let Some(registry) = registries.get_mut(&brain_id) else {
            return Err(Status::failed_precondition(
                "placement registry is not published",
            ));
        };
        match registry.abort_prepared() {
            Ok(()) | Err(PlacementRegistryError::NoPreparedPlacement) => Ok(()),
            Err(error) => Err(Status::failed_precondition(error.to_string())),
        }
    }
}

/// Persist the lifecycle of the worker activation paired with a published
/// placement. Placement authority and worker process activation are separate
/// boundaries, so a transport failure must remain visible and retryable after
/// restart instead of being hidden behind an RPC error.
fn record_placement_activation_status(
    placement_registries: &Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    brain_id: BrainId,
    leader_term: LeaseTerm,
    idempotency_key: &str,
    request_id: &str,
    plan_digest: crate::deterministic::StateDigest,
    state: PlacementActivationState,
    error: &str,
) -> Result<PlacementRegistry, Status> {
    record_placement_activation_status_with_command(
        placement_registries,
        brain_id,
        leader_term,
        idempotency_key,
        request_id,
        plan_digest,
        state,
        error,
        "",
    )
}

fn record_placement_activation_status_with_command(
    placement_registries: &Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    brain_id: BrainId,
    leader_term: LeaseTerm,
    idempotency_key: &str,
    request_id: &str,
    plan_digest: crate::deterministic::StateDigest,
    state: PlacementActivationState,
    error: &str,
    activation_command_json: &str,
) -> Result<PlacementRegistry, Status> {
    if let Some(path) = configured_placement_registry_path(brain_id) {
        let mut persisted = PersistedPlacementRegistry::open(path, brain_id, leader_term)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        persisted
            .record_activation_status_with_command(
                idempotency_key,
                request_id,
                plan_digest,
                state,
                error,
                activation_command_json,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        return Ok(persisted.state().clone());
    }
    let mut registries = placement_registries
        .lock()
        .map_err(|_| Status::internal("placement registry lock poisoned"))?;
    let registry = registries
        .get_mut(&brain_id)
        .ok_or_else(|| Status::failed_precondition("placement registry is not published"))?;
    registry
        .record_activation_status_with_command(
            idempotency_key,
            request_id,
            plan_digest,
            state,
            error,
            activation_command_json,
        )
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    Ok(registry.clone())
}

/// Apply a digest-bound worker activation result to the same persisted
/// placement registry that recorded the original dispatch. A command created
/// outside the management placement API has no idempotency key and is left as
/// an explicitly local/reference activation.
fn record_placement_activation_result(
    placement_registries: &Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    result: &crate::distributed::proto::NetworkCommandResult,
) -> Result<(), Status> {
    if result.placement_idempotency_key.trim().is_empty() {
        return Ok(());
    }
    let brain_id = BrainId::new(result.brain_id)
        .map_err(|error| Status::invalid_argument(format!("invalid activation brain: {error}")))?;
    let state = if result.accepted {
        // Queued remains the public lifecycle state until the worker's full
        // registration and durable application acknowledgement are observed.
        PlacementActivationState::Queued
    } else {
        PlacementActivationState::Failed
    };
    if let Some(path) = configured_placement_registry_path(brain_id) {
        let mut persisted = PersistedPlacementRegistry::open_existing(path, brain_id)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        persisted
            .record_activation_outcome(
                result.placement_idempotency_key.clone(),
                state,
                result.error.clone(),
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if !result.accepted {
            let _ = persisted.abort_prepared();
        }
        return Ok(());
    }
    let mut registries = placement_registries
        .lock()
        .map_err(|_| Status::internal("placement registry lock poisoned"))?;
    let registry = registries
        .get_mut(&brain_id)
        .ok_or_else(|| Status::failed_precondition("placement registry is not published"))?;
    registry
        .record_activation_outcome(
            result.placement_idempotency_key.clone(),
            state,
            result.error.clone(),
        )
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    if !result.accepted {
        let _ = registry.abort_prepared();
    }
    Ok(())
}

/// Promote a queued activation only after the target has presented durable,
/// digest-bound worker evidence.  A registration is an observation, so every
/// field that can prove ownership is matched against the immutable placement
/// and activation command before the lifecycle state is changed.
fn record_placement_worker_registration(
    registry: &mut PlacementRegistry,
    target_node: &str,
    registration: &StableWorkerRegistration,
) -> Result<usize, Status> {
    registration.validate().map_err(|error| {
        Status::invalid_argument(format!("invalid stable worker registration: {error}"))
    })?;
    let Some(plan) = registry
        .prepared_plan()
        .or(registry.active_plan.as_ref())
        .cloned()
    else {
        return Ok(0);
    };
    if registration.brain_id != registry.brain_id.raw()
        || registration.plan_digest != plan.digest().to_string()
        || registration.lease_term != plan.lease_term.raw()
        || registration.fencing_token != plan.fencing_token
    {
        return Ok(0);
    }

    let mut expected_shards = plan
        .placements
        .iter()
        .filter(|placement| placement.active_node == target_node)
        .map(|placement| placement.shard_id.raw())
        .collect::<Vec<_>>();
    expected_shards.sort_unstable();
    let mut expected_plan_shards = plan
        .placements
        .iter()
        .map(|placement| placement.shard_id.raw())
        .collect::<Vec<_>>();
    expected_plan_shards.sort_unstable();
    if registration.shard_ids != expected_plan_shards
        || registration.owned_shard_ids != expected_shards
    {
        return Ok(0);
    }

    let mut activated = 0;
    let candidates = registry
        .activation_statuses
        .iter()
        .filter(|(_, status)| {
            matches!(
                status.state,
                PlacementActivationState::Pending | PlacementActivationState::Queued
            ) && status.plan_digest == plan.digest()
        })
        .map(|(key, status)| (key.clone(), status.clone()))
        .collect::<Vec<_>>();
    for (key, status) in candidates {
        let command =
            serde_json::from_str::<StableWorkerActivationCommand>(&status.activation_command_json)
                .map_err(|error| {
                    Status::failed_precondition(format!(
                        "activation {key} has an invalid durable command: {error}"
                    ))
                })?;
        command
            .verify()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if command.target_node != target_node
            || command.brain_id != registration.brain_id
            || command.network_id != registration.network_id
            || command.placement_idempotency_key
                != if status.placement_idempotency_key.trim().is_empty() {
                    key.clone()
                } else {
                    status.placement_idempotency_key.clone()
                }
        {
            continue;
        }
        registry
            .record_activation_outcome(&key, PlacementActivationState::Active, "")
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        activated += 1;
    }
    if registry.prepared_plan().is_some() {
        match registry.commit_prepared() {
            Ok(_) | Err(PlacementRegistryError::ActivationIncomplete) => {}
            Err(error) => return Err(Status::failed_precondition(error.to_string())),
        }
    }
    Ok(activated)
}

fn record_placement_worker_registration_in_stores(
    placement_registries: &Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    target_node: &str,
    registration: &StableWorkerRegistration,
) -> Result<usize, Status> {
    let brain_id = BrainId::new(registration.brain_id).map_err(|error| {
        Status::invalid_argument(format!("invalid registration brain: {error}"))
    })?;
    if let Some(path) = configured_placement_registry_path(brain_id) {
        let mut persisted = PersistedPlacementRegistry::open_existing(path, brain_id)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let mut state = persisted.state().clone();
        let activated =
            record_placement_worker_registration(&mut state, target_node, registration)?;
        if activated > 0 {
            // Persist through the same atomic registry boundary.  The in-memory
            // helper is used first so no durable write occurs for an unrelated
            // registration.
            for (key, status) in state.activation_statuses.clone() {
                if status.state == PlacementActivationState::Active
                    && persisted
                        .state()
                        .activation_statuses
                        .get(&key)
                        .is_some_and(|previous| previous.state != PlacementActivationState::Active)
                {
                    persisted
                        .record_activation_outcome(key, PlacementActivationState::Active, "")
                        .map_err(|error| Status::failed_precondition(error.to_string()))?;
                }
            }
            match persisted.commit_prepared() {
                Ok(_) | Err(PlacementRegistryError::ActivationIncomplete) => {}
                Err(PlacementRegistryError::NoPreparedPlacement) => {}
                Err(error) => return Err(Status::failed_precondition(error.to_string())),
            }
        }
        return Ok(activated);
    }
    let mut registries = placement_registries
        .lock()
        .map_err(|_| Status::internal("placement registry lock poisoned"))?;
    let registry = registries
        .get_mut(&brain_id)
        .ok_or_else(|| Status::failed_precondition("placement registry is not published"))?;
    record_placement_worker_registration(registry, target_node, registration)
}

fn placement_apply_response(
    receipt: &crate::placement_registry::PlacementApplyReceipt,
    registry: &PlacementRegistry,
) -> Result<crate::generated_management::proto::PlacementApplyResponse, Status> {
    Ok(crate::generated_management::proto::PlacementApplyResponse {
        schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
        receipt_json: serde_json::to_string(receipt).map_err(|error| {
            Status::internal(format!("placement receipt encoding failed: {error}"))
        })?,
        registry_json: serde_json::to_string(registry).map_err(|error| {
            Status::internal(format!("placement registry encoding failed: {error}"))
        })?,
        error_code: String::new(),
    })
}

const MAX_MIGRATION_JSON_BYTES: usize = 1024 * 1024;

fn persistent_migration_path(brain_id: BrainId) -> Option<PathBuf> {
    std::env::var("NM_MIGRATION_JOURNAL_DIR")
        .ok()
        .map(|root| root.trim().to_owned())
        .filter(|root| !root.is_empty())
        .map(|root| PathBuf::from(root).join(format!("brain-{}.json", brain_id.raw())))
}

fn decode_migration_request(raw: &str) -> Result<MigrationRequest, Status> {
    if raw.len() > MAX_MIGRATION_JSON_BYTES {
        return Err(Status::resource_exhausted("migration request is too large"));
    }
    serde_json::from_str(raw)
        .map_err(|error| Status::invalid_argument(format!("invalid migration request: {error}")))
}

fn decode_migration_transition(raw: &str) -> Result<MigrationTransition, Status> {
    if raw.len() > MAX_MIGRATION_JSON_BYTES {
        return Err(Status::resource_exhausted(
            "migration transition is too large",
        ));
    }
    serde_json::from_str(raw)
        .map_err(|error| Status::invalid_argument(format!("invalid migration transition: {error}")))
}

fn decode_migration_group_spec(raw: &str) -> Result<Option<MigrationGroupSpec>, Status> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_MIGRATION_JSON_BYTES {
        return Err(Status::resource_exhausted("migration group is too large"));
    }
    serde_json::from_str(raw)
        .map(Some)
        .map_err(|error| Status::invalid_argument(format!("invalid migration group: {error}")))
}

fn decode_migration_group_update(raw: &str) -> Result<Option<MigrationGroupUpdate>, Status> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_MIGRATION_JSON_BYTES {
        return Err(Status::resource_exhausted(
            "migration group update is too large",
        ));
    }
    serde_json::from_str(raw).map(Some).map_err(|error| {
        Status::invalid_argument(format!("invalid migration group update: {error}"))
    })
}

fn migration_operation_response(
    operation: &MigrationOperation,
    journal: &MigrationJournal,
) -> Result<crate::generated_management::proto::MigrationOperationResponse, Status> {
    Ok(
        crate::generated_management::proto::MigrationOperationResponse {
            schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
            operation_json: serde_json::to_string(operation).map_err(|error| {
                Status::internal(format!("migration operation encoding failed: {error}"))
            })?,
            journal_json: serde_json::to_string(journal).map_err(|error| {
                Status::internal(format!("migration journal encoding failed: {error}"))
            })?,
            error_code: String::new(),
        },
    )
}

fn advance_journal_to_cutover_ready(
    journal: &mut MigrationJournal,
    submitted: &MigrationOperation,
    transferred_bytes: u64,
) -> Result<MigrationOperation, crate::migration_operation::MigrationOperationError> {
    let mut current = journal.operation(submitted.operation_id).cloned().ok_or(
        crate::migration_operation::MigrationOperationError::MissingOperation(
            submitted.operation_id,
        ),
    )?;
    if current.phase.terminal() {
        return Err(crate::migration_operation::MigrationOperationError::TerminalOperation);
    }
    if transferred_bytes != current.progress.total_bytes {
        return Err(
            crate::migration_operation::MigrationOperationError::InvalidProgress(
                "executor transfer byte count does not match the journal",
            ),
        );
    }
    let mut complete = current.progress.clone();
    complete.completed_shards = complete.total_shards;
    complete.transferred_bytes = transferred_bytes;
    for next_phase in [
        MigrationPhase::Reserving,
        MigrationPhase::Transferring,
        MigrationPhase::CatchingUp,
        MigrationPhase::Draining,
        MigrationPhase::CutoverReady,
    ] {
        if current.phase == next_phase {
            continue;
        }
        let progress = if matches!(
            next_phase,
            MigrationPhase::CatchingUp | MigrationPhase::Draining | MigrationPhase::CutoverReady
        ) {
            complete.clone()
        } else {
            current.progress.clone()
        };
        current = journal.transition(MigrationTransition {
            operation_id: current.operation_id,
            observed_leader_term: journal.leader_term,
            expected_resource_version: journal.resource_version,
            next_phase,
            progress,
            error_code: None,
        })?;
    }
    Ok(current)
}

fn advance_journal_to_cutover_ready_persisted(
    journal: &mut PersistedMigrationJournal,
    submitted: &MigrationOperation,
    transferred_bytes: u64,
) -> Result<MigrationOperation, String> {
    let mut current = submitted.clone();
    if current.phase.terminal() {
        return Err("migration operation is already terminal".to_owned());
    }
    if transferred_bytes != current.progress.total_bytes {
        return Err("executor transfer byte count does not match the journal".to_owned());
    }
    let mut complete = current.progress.clone();
    complete.completed_shards = complete.total_shards;
    complete.transferred_bytes = transferred_bytes;
    for next_phase in [
        MigrationPhase::Reserving,
        MigrationPhase::Transferring,
        MigrationPhase::CatchingUp,
        MigrationPhase::Draining,
        MigrationPhase::CutoverReady,
    ] {
        if current.phase == next_phase {
            continue;
        }
        let progress = if matches!(
            next_phase,
            MigrationPhase::CatchingUp | MigrationPhase::Draining | MigrationPhase::CutoverReady
        ) {
            complete.clone()
        } else {
            current.progress.clone()
        };
        current = journal
            .transition(MigrationTransition {
                operation_id: current.operation_id,
                observed_leader_term: journal.journal().leader_term,
                expected_resource_version: journal.journal().resource_version,
                next_phase,
                progress,
                error_code: None,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(current)
}

fn truncate_dispatch_error(mut error: String) -> String {
    const MAX_ERROR_BYTES: usize = 1024;
    if error.len() > MAX_ERROR_BYTES {
        error.truncate(MAX_ERROR_BYTES);
    }
    if error.trim().is_empty() {
        "migration executor failed".to_owned()
    } else {
        error
    }
}

fn migration_brain_id(raw: u64) -> Result<BrainId, Status> {
    BrainId::new(raw).map_err(|error| Status::invalid_argument(error.to_string()))
}

#[tonic::async_trait]
impl crate::generated_management::proto::management_server::Management for ManagementGrpcService {
    async fn get_status(
        &self,
        request: Request<crate::generated_management::proto::StatusRequest>,
    ) -> Result<Response<crate::generated_management::proto::StatusResponse>, Status> {
        let brain_id = request.into_inner().brain_id;
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        Ok(Response::new(
            crate::generated_management::proto::StatusResponse {
                schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
                brain_id,
                resource_version: orchestrator.resource_version(),
                leader_term: orchestrator.leader_term().raw(),
                execution_state: "managed".to_owned(),
                durability_state: "policy-reported".to_owned(),
            },
        ))
    }

    async fn submit_operation(
        &self,
        request: Request<crate::generated_management::proto::OperationRequest>,
    ) -> Result<Response<crate::generated_management::proto::OperationResponse>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .ok_or_else(|| Status::invalid_argument("request context is required"))?;
        let kind = match request.kind {
            1 => OperationKind::Start,
            2 => OperationKind::Stop,
            3 => OperationKind::Reset,
            4 => OperationKind::Export,
            _ => return Err(Status::invalid_argument("operation kind is invalid")),
        };
        let capability = if kind == OperationKind::Reset {
            Capability::Reset
        } else if kind == OperationKind::Export {
            Capability::Export
        } else {
            Capability::Operate
        };
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        let operation = orchestrator
            .submit(
                Principal {
                    id: request.principal_id,
                },
                capability,
                MutationContext {
                    observed_leader_term: LeaseTerm::new(context.observed_leader_term)
                        .map_err(|error| Status::invalid_argument(error.to_string()))?,
                    expected_version: context.expected_resource_version,
                    idempotency_key: context.idempotency_key,
                    request_id: context.request_id,
                },
                kind,
            )
            .map_err(management_status)?;
        let (state, error_code) = operation_state_name(&operation.state);
        Ok(Response::new(
            crate::generated_management::proto::OperationResponse {
                schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
                operation_id: operation.id.raw(),
                state,
                resource_version: orchestrator.resource_version(),
                leader_term: orchestrator.leader_term().raw(),
                error_code,
            },
        ))
    }

    async fn plan_placement(
        &self,
        request: Request<crate::generated_management::proto::PlacementCommandRequest>,
    ) -> Result<Response<crate::generated_management::proto::PlacementCommandResponse>, Status>
    {
        let (command, plan) = decode_placement_plan_command(&request.into_inner().command_json)?;
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        if !orchestrator.allows_for_brain(
            &Principal {
                id: command.principal_id.clone(),
            },
            &command.brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot plan placement for this brain",
            ));
        }
        let leader_term = orchestrator.leader_term();
        drop(orchestrator);
        let current_plan = self
            .placement_registries
            .lock()
            .map_err(|_| Status::internal("placement registry lock poisoned"))?
            .get(&command.brain_id)
            .and_then(|registry| registry.active_plan.clone())
            .or(persisted_active_placement_plan(
                command.brain_id,
                leader_term,
            )?);
        let migration_in_flight = self
            .migration_in_flight
            .lock()
            .map_err(|_| Status::internal("migration dispatcher lock poisoned"))?;
        let active_migrations = active_migration_count(&migration_in_flight, command.brain_id);
        let mut placement_controllers = self
            .placement_controllers
            .lock()
            .map_err(|_| Status::internal("placement controller lock poisoned"))?;
        let review = review_placement_plan(
            &mut placement_controllers,
            &command,
            &plan,
            current_plan,
            active_migrations,
        )?;
        if !review.approved {
            return Err(Status::failed_precondition(review.reason));
        }
        Ok(Response::new(placement_plan_response(&command, &plan)?))
    }

    async fn apply_placement(
        &self,
        request: Request<crate::generated_management::proto::PlacementCommandRequest>,
    ) -> Result<Response<crate::generated_management::proto::PlacementApplyResponse>, Status> {
        let request = request.into_inner();
        let (command, apply_request, activation) = decode_placement_apply_request(&request)?;
        if let Some(activation) = activation.as_ref() {
            validate_activation_target(activation, &apply_request.plan)?;
            if self.placement_activation_dispatcher.is_none() {
                return Err(Status::failed_precondition(
                    "stable worker activation dispatcher is not configured",
                ));
            }
        }
        {
            let orchestrator = self
                .orchestrator
                .lock()
                .map_err(|_| Status::internal("management lock poisoned"))?;
            if command.observed_leader_term != orchestrator.leader_term() {
                return Err(Status::failed_precondition("stale leader term"));
            }
            if !orchestrator.allows_for_brain(
                &Principal {
                    id: command.principal_id.clone(),
                },
                &command.brain_id.to_string(),
                &Capability::Operate,
            ) {
                return Err(Status::permission_denied(
                    "principal cannot apply placement for this brain",
                ));
            }
        }
        let persisted_root = std::env::var("NM_PLACEMENT_REGISTRY_DIR")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let (receipt, mut registry) = if let Some(root) = persisted_root {
            // The brain ID is a validated numeric stable identity, so this
            // path cannot introduce a caller-controlled traversal component.
            let path = std::path::PathBuf::from(root)
                .join(format!("brain-{}.json", command.brain_id.raw()));
            let mut persisted = PersistedPlacementRegistry::open(
                path,
                command.brain_id,
                command.observed_leader_term,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let receipt = if activation.is_some() {
                persisted.prepare(apply_request)
            } else {
                persisted.apply(apply_request)
            }
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
            (receipt, persisted.state().clone())
        } else {
            let mut registries = self
                .placement_registries
                .lock()
                .map_err(|_| Status::internal("placement registry lock poisoned"))?;
            apply_placement_registry(
                &mut registries,
                &command,
                apply_request,
                activation.is_some(),
            )?
        };
        if let Some(activation) = activation.as_ref() {
            let activation_command_json = serde_json::to_string(activation).map_err(|error| {
                Status::internal(format!("stable worker activation encoding failed: {error}"))
            })?;
            let _ = record_placement_activation_status_with_command(
                &self.placement_registries,
                command.brain_id,
                receipt.leader_term,
                &receipt.idempotency_key,
                &receipt.request_id,
                receipt.plan_digest,
                PlacementActivationState::Pending,
                "",
                &activation_command_json,
            )?;
            let dispatcher = self
                .placement_activation_dispatcher
                .as_ref()
                .expect("activation dispatcher checked before placement publication")
                .clone();
            if let Err(error) = dispatcher(activation.clone()).await {
                let dispatch_error = error.clone();
                if let Err(status_error) = record_placement_activation_status(
                    &self.placement_registries,
                    command.brain_id,
                    receipt.leader_term,
                    &receipt.idempotency_key,
                    &receipt.request_id,
                    receipt.plan_digest,
                    PlacementActivationState::Failed,
                    &dispatch_error,
                ) {
                    return Err(Status::internal(format!(
                        "activation dispatch failed ({error}) and its failure status could not be persisted: {status_error}"
                    )));
                }
                abort_placement_prepared(
                    &self.placement_registries,
                    command.brain_id,
                    receipt.leader_term,
                )?;
                return Err(Status::unavailable(format!(
                    "placement activation was not queued; prepared placement was aborted: {error}"
                )));
            }
            registry = record_placement_activation_status(
                &self.placement_registries,
                command.brain_id,
                receipt.leader_term,
                &receipt.idempotency_key,
                &receipt.request_id,
                receipt.plan_digest,
                PlacementActivationState::Queued,
                "",
            )?;
        }
        if receipt.committed {
            if let Some(plan) = registry.active_plan.clone() {
                let mut placement_controllers = self
                    .placement_controllers
                    .lock()
                    .map_err(|_| Status::internal("placement controller lock poisoned"))?;
                placement_controller_entry(&mut placement_controllers, command.brain_id)?
                    .record_committed(plan, receipt.cut_tag)
                    .map_err(placement_controller_status)?;
            }
        }
        Ok(Response::new(placement_apply_response(
            &receipt, &registry,
        )?))
    }

    async fn submit_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationCommandRequest>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let request = request.into_inner();
        let migration = decode_migration_request(&request.command_json)?;
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        if !orchestrator.allows_for_brain(
            &Principal {
                id: request.principal_id,
            },
            &migration.brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot submit migration for this brain",
            ));
        }
        if migration.observed_leader_term != orchestrator.leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        drop(orchestrator);
        let group_spec = decode_migration_group_spec(&request.group_json)?;
        let dispatch_group = group_spec.clone();
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals.entry(migration.brain_id).or_insert_with(|| {
            MigrationJournal::new(migration.brain_id, migration.observed_leader_term)
        });
        let operation = journal
            .submit_with_group(migration, group_spec)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let response = migration_operation_response(&operation, journal)?;
        drop(journals);
        if let Some(dispatch_group) = dispatch_group {
            self.schedule_migration(operation.clone(), dispatch_group);
        }
        Ok(Response::new(response))
    }

    async fn advance_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationAdvanceRequest>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let request = request.into_inner();
        let transition = decode_migration_transition(&request.transition_json)?;
        let group_update = decode_migration_group_update(&request.group_update_json)?;
        let requested_brain = migration_brain_id(request.brain_id)?;
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let brain_id = requested_brain;
        let operation = journals
            .get(&brain_id)
            .and_then(|journal| journal.operation(transition.operation_id))
            .ok_or_else(|| Status::not_found("migration operation not found"))?;
        if operation.brain_id != brain_id {
            return Err(Status::not_found("migration operation not found"));
        }
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        if !orchestrator.allows_for_brain(
            &Principal {
                id: request.principal_id,
            },
            &brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot advance migration for this brain",
            ));
        }
        drop(orchestrator);
        let journal = journals
            .get_mut(&brain_id)
            .expect("operation lookup and journal mutation use the same map");
        if let Some(mut group_update) = group_update {
            if group_update.operation_id != transition.operation_id {
                return Err(Status::invalid_argument(
                    "migration group update operation does not match transition",
                ));
            }
            group_update.expected_resource_version = transition.expected_resource_version;
            journal
                .apply_group_update(group_update)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
        }
        let mut transition = transition;
        if journal.resource_version != transition.expected_resource_version {
            transition.expected_resource_version = journal.resource_version;
        }
        let operation = journal
            .transition(transition)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(migration_operation_response(
            &operation, journal,
        )?))
    }

    async fn get_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationLookup>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let request = request.into_inner();
        let brain_id = migration_brain_id(request.brain_id)?;
        let term = LeaseTerm::new(request.observed_leader_term)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        if term != orchestrator.leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        drop(orchestrator);
        let journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals
            .get(&brain_id)
            .ok_or_else(|| Status::not_found("migration brain not found"))?;
        if journal.leader_term != term {
            return Err(Status::failed_precondition("stale leader term"));
        }
        let operation = journal
            .operation(request.operation_id)
            .ok_or_else(|| Status::not_found("migration operation not found"))?;
        Ok(Response::new(migration_operation_response(
            operation, journal,
        )?))
    }

    async fn get_migration_status(
        &self,
        request: Request<crate::generated_management::proto::MigrationLookup>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        self.get_migration(request).await
    }

    async fn cancel_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationCancelRequest>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let request = request.into_inner();
        let brain_id = migration_brain_id(request.brain_id)?;
        if self
            .migration_in_flight
            .lock()
            .map_err(|_| Status::internal("migration dispatcher lock poisoned"))?
            .iter()
            .any(|(brain, operation)| *brain == brain_id && *operation == request.operation_id)
        {
            return Err(Status::failed_precondition(
                "migration cancellation is blocked while the registered executor is active",
            ));
        }
        let term = LeaseTerm::new(request.observed_leader_term)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        if term != orchestrator.leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        if !orchestrator.allows_for_brain(
            &Principal {
                id: request.principal_id,
            },
            &brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot cancel migration for this brain",
            ));
        }
        drop(orchestrator);
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals
            .get_mut(&brain_id)
            .ok_or_else(|| Status::not_found("migration brain not found"))?;
        let operation = journal
            .cancel(
                request.operation_id,
                term,
                request.expected_resource_version,
                request.reason,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(migration_operation_response(
            &operation, journal,
        )?))
    }

    async fn get_operation(
        &self,
        request: Request<crate::generated_management::proto::OperationLookup>,
    ) -> Result<Response<crate::generated_management::proto::OperationResponse>, Status> {
        let request = request.into_inner();
        let term = LeaseTerm::new(request.observed_leader_term)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        if term != orchestrator.leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        let id = EventId::new(request.operation_id)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let operation = orchestrator
            .operation(id)
            .ok_or_else(|| Status::not_found("operation not found"))?;
        let (state, error_code) = operation_state_name(&operation.state);
        Ok(Response::new(
            crate::generated_management::proto::OperationResponse {
                schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
                operation_id: operation.id.raw(),
                state,
                resource_version: orchestrator.resource_version(),
                leader_term: orchestrator.leader_term().raw(),
                error_code,
            },
        ))
    }
}

/// Secured, restart-safe generated management endpoint.  It consumes only an
/// [`AuthenticatedPrincipal`] installed by the transport interceptor and
/// persists operation state before acknowledging mutations.
#[derive(Clone)]
pub struct SecuredManagementGrpcService {
    orchestrator: Arc<Mutex<PersistedManagementOrchestrator>>,
    placement_registries: Arc<Mutex<BTreeMap<BrainId, PlacementRegistry>>>,
    placement_controllers: Arc<Mutex<BTreeMap<BrainId, PlacementController>>>,
    migration_journals: Arc<Mutex<BTreeMap<BrainId, MigrationJournal>>>,
    dispatcher: Option<ManagementOperationDispatcher>,
    migration_dispatcher: Option<MigrationDispatchHandler>,
    placement_activation_dispatcher: Option<PlacementActivationDispatcher>,
    /// Process-local suppression for commands successfully requeued by the
    /// restart recovery loop. Durable registry state remains authoritative.
    activation_recovery_attempts: Arc<Mutex<BTreeSet<String>>>,
    migration_in_flight: Arc<Mutex<BTreeSet<(BrainId, u64)>>>,
}

impl SecuredManagementGrpcService {
    pub fn new(orchestrator: PersistedManagementOrchestrator) -> Self {
        Self::with_dispatchers(orchestrator, None, None)
    }

    pub fn with_dispatcher(
        orchestrator: PersistedManagementOrchestrator,
        dispatcher: Option<ManagementOperationDispatcher>,
    ) -> Self {
        Self::with_dispatchers(orchestrator, dispatcher, None)
    }

    pub fn with_migration_dispatcher(
        orchestrator: PersistedManagementOrchestrator,
        migration_dispatcher: Option<MigrationDispatchHandler>,
    ) -> Self {
        Self::with_dispatchers(orchestrator, None, migration_dispatcher)
    }

    pub fn with_dispatchers(
        orchestrator: PersistedManagementOrchestrator,
        dispatcher: Option<ManagementOperationDispatcher>,
        migration_dispatcher: Option<MigrationDispatchHandler>,
    ) -> Self {
        Self::with_dispatchers_and_activation(orchestrator, dispatcher, migration_dispatcher, None)
    }

    pub fn with_dispatchers_and_activation(
        orchestrator: PersistedManagementOrchestrator,
        dispatcher: Option<ManagementOperationDispatcher>,
        migration_dispatcher: Option<MigrationDispatchHandler>,
        placement_activation_dispatcher: Option<PlacementActivationDispatcher>,
    ) -> Self {
        Self {
            orchestrator: Arc::new(Mutex::new(orchestrator)),
            placement_registries: Arc::new(Mutex::new(BTreeMap::new())),
            placement_controllers: Arc::new(Mutex::new(BTreeMap::new())),
            migration_journals: Arc::new(Mutex::new(BTreeMap::new())),
            dispatcher,
            migration_dispatcher,
            placement_activation_dispatcher,
            activation_recovery_attempts: Arc::new(Mutex::new(BTreeSet::new())),
            migration_in_flight: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    pub fn orchestrator(&self) -> Arc<Mutex<PersistedManagementOrchestrator>> {
        Arc::clone(&self.orchestrator)
    }

    /// Receive an authenticated heartbeat outcome from the orchestrator's
    /// node adapter. This is intentionally a one-way callback: workers never
    /// obtain this service or a placement registry handle.
    pub fn record_stable_activation_result(
        &self,
        result: &crate::distributed::proto::NetworkCommandResult,
    ) -> Result<(), Status> {
        record_placement_activation_result(&self.placement_registries, result)
    }

    /// Accept validated worker registration evidence from the node adapter.
    /// The callback is deliberately narrow and carries no registry handle to
    /// the worker or data plane.
    pub fn record_stable_worker_registration(
        &self,
        target_node: &str,
        registration: &StableWorkerRegistration,
    ) -> Result<usize, Status> {
        record_placement_worker_registration_in_stores(
            &self.placement_registries,
            target_node,
            registration,
        )
    }

    /// Requeue durable activation commands after an orchestrator restart.
    /// Admission is still delegated to the orchestrator node adapter, which
    /// requires enrollment, the activation capability and a live session.
    /// Failed activations are terminal and are intentionally excluded by the
    /// placement registry; a new management request is required to retry one.
    pub async fn recover_pending_placement_activations(&self) -> Result<usize, Status> {
        let Some(dispatcher) = self.placement_activation_dispatcher.clone() else {
            return Ok(0);
        };
        let Some(root) = std::env::var("NM_PLACEMENT_REGISTRY_DIR")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            return Ok(0);
        };
        let root = PathBuf::from(root);
        if !root.exists() {
            return Ok(0);
        }
        let entries = fs::read_dir(root).map_err(|error| {
            Status::failed_precondition(format!("placement recovery scan failed: {error}"))
        })?;
        let mut recovered = 0usize;
        for entry in entries.take(1024) {
            let entry = entry.map_err(|error| {
                Status::failed_precondition(format!("placement recovery entry failed: {error}"))
            })?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(raw_brain) = name
                .strip_prefix("brain-")
                .and_then(|value| value.strip_suffix(".json"))
            else {
                continue;
            };
            let Ok(raw_brain) = raw_brain.parse::<u64>() else {
                continue;
            };
            let brain_id = BrainId::new(raw_brain)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let persisted = PersistedPlacementRegistry::open_existing(entry.path(), brain_id)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let Some(plan) = persisted.state().active_plan.as_ref() else {
                continue;
            };
            for (idempotency_key, status) in persisted.state().retryable_activation_commands() {
                if self
                    .activation_recovery_attempts
                    .lock()
                    .map_err(|_| Status::internal("activation recovery lock poisoned"))?
                    .contains(&idempotency_key)
                {
                    continue;
                }
                if status.plan_digest != plan.digest() {
                    continue;
                }
                let command = serde_json::from_str::<StableWorkerActivationCommand>(
                    &status.activation_command_json,
                )
                .map_err(|error| {
                    Status::failed_precondition(format!(
                        "durable activation command {idempotency_key} is invalid: {error}"
                    ))
                })?;
                command
                    .verify()
                    .map_err(|error| Status::failed_precondition(error.to_string()))?;
                if command.brain_id != brain_id.raw()
                    || command.placement_idempotency_key != idempotency_key
                    || !plan
                        .placements
                        .iter()
                        .any(|placement| placement.active_node == command.target_node)
                {
                    continue;
                }
                if dispatcher(command).await.is_ok() {
                    self.activation_recovery_attempts
                        .lock()
                        .map_err(|_| Status::internal("activation recovery lock poisoned"))?
                        .insert(idempotency_key);
                    recovered += 1;
                }
            }
        }
        Ok(recovered)
    }

    fn reserve_migration_dispatch(&self, operation: &MigrationOperation) -> Result<bool, Status> {
        let Some(_) = self.migration_dispatcher else {
            return Ok(false);
        };
        let mut in_flight = self
            .migration_in_flight
            .lock()
            .map_err(|_| Status::internal("migration dispatcher lock poisoned"))?;
        if in_flight
            .iter()
            .any(|(brain, _)| *brain == operation.brain_id)
        {
            return Err(Status::failed_precondition(
                "another migration for this brain is already executing",
            ));
        }
        in_flight.insert((operation.brain_id, operation.operation_id));
        Ok(true)
    }

    fn release_migration_dispatch(&self, brain_id: BrainId, operation_id: u64) {
        if let Ok(mut in_flight) = self.migration_in_flight.lock() {
            in_flight.remove(&(brain_id, operation_id));
        }
    }

    fn migration_still_dispatchable(&self, operation: &MigrationOperation) -> bool {
        if let Some(path) = persistent_migration_path(operation.brain_id) {
            return PersistedMigrationJournal::open_existing(path, operation.brain_id)
                .ok()
                .and_then(|journal| journal.operation(operation.operation_id).cloned())
                .is_some_and(|current| {
                    matches!(
                        current.phase,
                        MigrationPhase::Prepared | MigrationPhase::RecoveryRequired
                    )
                });
        }
        self.migration_journals
            .lock()
            .ok()
            .and_then(|journals| {
                journals
                    .get(&operation.brain_id)
                    .and_then(|journal| journal.operation(operation.operation_id))
                    .map(|current| {
                        matches!(
                            current.phase,
                            MigrationPhase::Prepared | MigrationPhase::RecoveryRequired
                        )
                    })
            })
            .unwrap_or(false)
    }

    fn schedule_migration(&self, operation: MigrationOperation, group: MigrationGroupSpec) {
        let Some(dispatcher) = self.migration_dispatcher.as_ref().cloned() else {
            return;
        };
        let Ok(true) = self.reserve_migration_dispatch(&operation) else {
            return;
        };
        if !self.migration_still_dispatchable(&operation) {
            self.release_migration_dispatch(operation.brain_id, operation.operation_id);
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            let dispatch = dispatcher(operation.clone(), group).await;
            let result = match dispatch {
                Ok(receipt) => service.finalize_migration(operation.clone(), receipt),
                Err(error) => service.fail_migration(operation.clone(), error),
            };
            if let Err(error) = result {
                eprintln!(
                    "secured migration dispatch completion could not update journal: brain={} operation={} error={error}",
                    operation.brain_id.raw(),
                    operation.operation_id
                );
            }
            service.release_migration_dispatch(operation.brain_id, operation.operation_id);
        });
    }

    fn finalize_migration(
        &self,
        operation: MigrationOperation,
        receipt: MigrationDispatchReceipt,
    ) -> Result<(), String> {
        receipt.verify_against(&operation)?;
        if let Some(path) = persistent_migration_path(operation.brain_id) {
            let mut journal = PersistedMigrationJournal::open_existing(path, operation.brain_id)
                .map_err(|error| error.to_string())?;
            let submitted = journal
                .operation(operation.operation_id)
                .cloned()
                .ok_or_else(|| "migration operation disappeared during dispatch".to_owned())?;
            advance_journal_to_cutover_ready_persisted(
                &mut journal,
                &submitted,
                receipt.transferred_bytes,
            )?;
            journal
                .commit_dispatched_group(
                    &receipt.group,
                    receipt.cut_tag,
                    receipt.transferred_bytes,
                    journal.journal().resource_version,
                )
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| "migration journal lock poisoned".to_owned())?;
        let journal = journals
            .get_mut(&operation.brain_id)
            .ok_or_else(|| "migration journal disappeared during dispatch".to_owned())?;
        advance_journal_to_cutover_ready(journal, &operation, receipt.transferred_bytes)
            .map_err(|error| error.to_string())?;
        journal
            .commit_dispatched_group(
                &receipt.group,
                receipt.cut_tag,
                receipt.transferred_bytes,
                journal.resource_version,
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn fail_migration(&self, operation: MigrationOperation, error: String) -> Result<(), String> {
        if let Some(path) = persistent_migration_path(operation.brain_id) {
            let mut journal = PersistedMigrationJournal::open_existing(path, operation.brain_id)
                .map_err(|error| error.to_string())?;
            let current = journal
                .operation(operation.operation_id)
                .cloned()
                .ok_or_else(|| "migration operation disappeared during dispatch".to_owned())?;
            if current.phase.terminal() {
                return Ok(());
            }
            journal
                .transition(MigrationTransition {
                    operation_id: current.operation_id,
                    observed_leader_term: journal.journal().leader_term,
                    expected_resource_version: journal.journal().resource_version,
                    next_phase: MigrationPhase::Failed,
                    progress: current.progress,
                    error_code: Some(truncate_dispatch_error(error)),
                })
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| "migration journal lock poisoned".to_owned())?;
        let journal = journals
            .get_mut(&operation.brain_id)
            .ok_or_else(|| "migration journal disappeared during dispatch".to_owned())?;
        let current = journal
            .operation(operation.operation_id)
            .cloned()
            .ok_or_else(|| "migration operation disappeared during dispatch".to_owned())?;
        if current.phase.terminal() {
            return Ok(());
        }
        journal
            .transition(MigrationTransition {
                operation_id: current.operation_id,
                observed_leader_term: journal.leader_term,
                expected_resource_version: journal.resource_version,
                next_phase: MigrationPhase::Failed,
                progress: current.progress,
                error_code: Some(truncate_dispatch_error(error)),
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn schedule_operation(&self, operation: Operation) {
        let Some(dispatcher) = self.dispatcher.as_ref().cloned() else {
            return;
        };
        let orchestrator = Arc::clone(&self.orchestrator);
        tokio::spawn(async move {
            let term = match orchestrator.lock() {
                Ok(orchestrator) => orchestrator.state().leader_term(),
                Err(_) => return,
            };
            let claimed = match orchestrator.lock() {
                Ok(mut orchestrator) => orchestrator.claim_pending(operation.id, term),
                Err(_) => return,
            };
            let Ok(true) = claimed else {
                // Another retry or leader has already claimed/finished it.
                // In particular, never execute a second side effect.
                return;
            };

            let final_state = match dispatcher(operation.brain_id, operation.kind).await {
                Ok(()) => OperationState::Succeeded,
                Err(code) => OperationState::Failed { code },
            };
            let Ok(mut orchestrator) = orchestrator.lock() else {
                return;
            };
            // A leader change fences the old task. Its Running record remains
            // recoverable by the new leader rather than being rewritten by a
            // stale completion callback.
            let _ = orchestrator.transition(operation.id, term, final_state);
        });
    }
}

#[tonic::async_trait]
impl crate::generated_management::proto::management_server::Management
    for SecuredManagementGrpcService
{
    async fn get_status(
        &self,
        request: Request<crate::generated_management::proto::StatusRequest>,
    ) -> Result<Response<crate::generated_management::proto::StatusResponse>, Status> {
        let principal = authenticated_principal(&request)?.to_owned();
        let brain_id = request.into_inner().brain_id;
        if brain_id.trim().is_empty() {
            return Err(Status::invalid_argument("brain_id is required"));
        }
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if !orchestrator.state().policy.allows_for_brain(
            &Principal { id: principal },
            &brain_id,
            &Capability::Read,
        ) {
            return Err(Status::permission_denied(
                "principal cannot read this brain",
            ));
        }
        Ok(Response::new(
            crate::generated_management::proto::StatusResponse {
                schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
                brain_id,
                resource_version: orchestrator.state().resource_version(),
                leader_term: orchestrator.state().leader_term().raw(),
                execution_state: "managed".to_owned(),
                durability_state: "persisted".to_owned(),
            },
        ))
    }

    async fn submit_operation(
        &self,
        request: Request<crate::generated_management::proto::OperationRequest>,
    ) -> Result<Response<crate::generated_management::proto::OperationResponse>, Status> {
        let authenticated = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        if request.brain_id.trim().is_empty() {
            return Err(Status::invalid_argument("brain_id is required"));
        }
        if request.principal_id != authenticated {
            return Err(Status::permission_denied(
                "request principal does not match authenticated identity",
            ));
        }
        let context = request
            .context
            .ok_or_else(|| Status::invalid_argument("request context is required"))?;
        let kind = match request.kind {
            1 => OperationKind::Start,
            2 => OperationKind::Stop,
            3 => OperationKind::Reset,
            4 => OperationKind::Export,
            _ => return Err(Status::invalid_argument("operation kind is invalid")),
        };
        let capability = if kind == OperationKind::Reset {
            Capability::Reset
        } else if kind == OperationKind::Export {
            Capability::Export
        } else {
            Capability::Operate
        };
        let operation = {
            let mut orchestrator = self
                .orchestrator
                .lock()
                .map_err(|_| Status::internal("management lock poisoned"))?;
            orchestrator
                .submit_for_brain(
                    Principal { id: authenticated },
                    capability,
                    MutationContext {
                        observed_leader_term: LeaseTerm::new(context.observed_leader_term)
                            .map_err(|error| Status::invalid_argument(error.to_string()))?,
                        expected_version: context.expected_resource_version,
                        idempotency_key: context.idempotency_key,
                        request_id: context.request_id,
                    },
                    kind,
                    request.brain_id,
                )
                .map_err(|error| match error {
                    PersistedAuthorityError::Management(error) => management_status(error),
                    other => Status::failed_precondition(other.to_string()),
                })?
        };
        self.schedule_operation(operation.clone());
        let orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        let operation = orchestrator
            .operation(operation.id)
            .cloned()
            .ok_or_else(|| Status::not_found("operation not found"))?;
        let (state, error_code) = operation_state_name(&operation.state);
        Ok(Response::new(
            crate::generated_management::proto::OperationResponse {
                schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
                operation_id: operation.id.raw(),
                state,
                resource_version: orchestrator.state().resource_version(),
                leader_term: orchestrator.state().leader_term().raw(),
                error_code,
            },
        ))
    }

    async fn plan_placement(
        &self,
        request: Request<crate::generated_management::proto::PlacementCommandRequest>,
    ) -> Result<Response<crate::generated_management::proto::PlacementCommandResponse>, Status>
    {
        let principal = authenticated_principal(&request)?.to_owned();
        let (command, plan) = decode_placement_plan_command(&request.into_inner().command_json)?;
        if command.principal_id != principal {
            return Err(Status::permission_denied(
                "placement command principal does not match authenticated identity",
            ));
        }
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if !orchestrator.state().policy.allows_for_brain(
            &Principal { id: principal },
            &command.brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot plan placement for this brain",
            ));
        }
        let leader_term = orchestrator.state().leader_term();
        drop(orchestrator);
        let current_plan = self
            .placement_registries
            .lock()
            .map_err(|_| Status::internal("placement registry lock poisoned"))?
            .get(&command.brain_id)
            .and_then(|registry| registry.active_plan.clone())
            .or(persisted_active_placement_plan(
                command.brain_id,
                leader_term,
            )?);
        let migration_in_flight = self
            .migration_in_flight
            .lock()
            .map_err(|_| Status::internal("migration dispatcher lock poisoned"))?;
        let active_migrations = active_migration_count(&migration_in_flight, command.brain_id);
        let mut placement_controllers = self
            .placement_controllers
            .lock()
            .map_err(|_| Status::internal("placement controller lock poisoned"))?;
        let review = review_placement_plan(
            &mut placement_controllers,
            &command,
            &plan,
            current_plan,
            active_migrations,
        )?;
        if !review.approved {
            return Err(Status::failed_precondition(review.reason));
        }
        Ok(Response::new(placement_plan_response(&command, &plan)?))
    }

    async fn apply_placement(
        &self,
        request: Request<crate::generated_management::proto::PlacementCommandRequest>,
    ) -> Result<Response<crate::generated_management::proto::PlacementApplyResponse>, Status> {
        let principal = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        let (command, apply_request, activation) = decode_placement_apply_request(&request)?;
        if let Some(activation) = activation.as_ref() {
            validate_activation_target(activation, &apply_request.plan)?;
            if self.placement_activation_dispatcher.is_none() {
                return Err(Status::failed_precondition(
                    "stable worker activation dispatcher is not configured",
                ));
            }
        }
        if command.principal_id != principal {
            return Err(Status::permission_denied(
                "placement command principal does not match authenticated identity",
            ));
        }
        {
            let mut orchestrator = self
                .orchestrator
                .lock()
                .map_err(|_| Status::internal("management lock poisoned"))?;
            orchestrator
                .refresh()
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            if command.observed_leader_term != orchestrator.state().leader_term() {
                return Err(Status::failed_precondition("stale leader term"));
            }
            if !orchestrator.state().policy.allows_for_brain(
                &Principal { id: principal },
                &command.brain_id.to_string(),
                &Capability::Operate,
            ) {
                return Err(Status::permission_denied(
                    "principal cannot apply placement for this brain",
                ));
            }
        }
        let (receipt, mut registry) =
            if let Some(path) = configured_placement_registry_path(command.brain_id) {
                let mut persisted = PersistedPlacementRegistry::open(
                    path,
                    command.brain_id,
                    command.observed_leader_term,
                )
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
                let receipt = if activation.is_some() {
                    persisted.prepare(apply_request)
                } else {
                    persisted.apply(apply_request)
                }
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
                (receipt, persisted.state().clone())
            } else {
                let mut registries = self
                    .placement_registries
                    .lock()
                    .map_err(|_| Status::internal("placement registry lock poisoned"))?;
                apply_placement_registry(
                    &mut registries,
                    &command,
                    apply_request,
                    activation.is_some(),
                )?
            };
        if let Some(activation) = activation.as_ref() {
            let activation_command_json = serde_json::to_string(activation).map_err(|error| {
                Status::internal(format!("stable worker activation encoding failed: {error}"))
            })?;
            let _ = record_placement_activation_status_with_command(
                &self.placement_registries,
                command.brain_id,
                receipt.leader_term,
                &receipt.idempotency_key,
                &receipt.request_id,
                receipt.plan_digest,
                PlacementActivationState::Pending,
                "",
                &activation_command_json,
            )?;
            let dispatcher = self
                .placement_activation_dispatcher
                .as_ref()
                .expect("activation dispatcher checked before placement publication")
                .clone();
            if let Err(error) = dispatcher(activation.clone()).await {
                let dispatch_error = error.clone();
                if let Err(status_error) = record_placement_activation_status(
                    &self.placement_registries,
                    command.brain_id,
                    receipt.leader_term,
                    &receipt.idempotency_key,
                    &receipt.request_id,
                    receipt.plan_digest,
                    PlacementActivationState::Failed,
                    &dispatch_error,
                ) {
                    return Err(Status::internal(format!(
                        "activation dispatch failed ({error}) and its failure status could not be persisted: {status_error}"
                    )));
                }
                abort_placement_prepared(
                    &self.placement_registries,
                    command.brain_id,
                    receipt.leader_term,
                )?;
                return Err(Status::unavailable(format!(
                    "placement activation was not queued; prepared placement was aborted: {error}"
                )));
            }
            registry = record_placement_activation_status(
                &self.placement_registries,
                command.brain_id,
                receipt.leader_term,
                &receipt.idempotency_key,
                &receipt.request_id,
                receipt.plan_digest,
                PlacementActivationState::Queued,
                "",
            )?;
        }
        if receipt.committed {
            if let Some(plan) = registry.active_plan.clone() {
                let mut placement_controllers = self
                    .placement_controllers
                    .lock()
                    .map_err(|_| Status::internal("placement controller lock poisoned"))?;
                placement_controller_entry(&mut placement_controllers, command.brain_id)?
                    .record_committed(plan, receipt.cut_tag)
                    .map_err(placement_controller_status)?;
            }
        }
        Ok(Response::new(placement_apply_response(
            &receipt, &registry,
        )?))
    }

    async fn submit_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationCommandRequest>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let principal = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        if request.principal_id != principal {
            return Err(Status::permission_denied(
                "migration command principal does not match authenticated identity",
            ));
        }
        let migration = decode_migration_request(&request.command_json)?;
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if migration.observed_leader_term != orchestrator.state().leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        if !orchestrator.state().policy.allows_for_brain(
            &Principal { id: principal },
            &migration.brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot submit migration for this brain",
            ));
        }
        drop(orchestrator);
        let group_spec = decode_migration_group_spec(&request.group_json)?;
        let dispatch_group = group_spec.clone();
        if let Some(path) = persistent_migration_path(migration.brain_id) {
            let mut journal = PersistedMigrationJournal::open(
                path,
                migration.brain_id,
                migration.observed_leader_term,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let operation = journal
                .submit_with_group(migration, group_spec)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let response = migration_operation_response(&operation, journal.journal())?;
            drop(journal);
            if let Some(dispatch_group) = dispatch_group {
                self.schedule_migration(operation.clone(), dispatch_group);
            }
            return Ok(Response::new(response));
        }
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals.entry(migration.brain_id).or_insert_with(|| {
            MigrationJournal::new(migration.brain_id, migration.observed_leader_term)
        });
        let operation = journal
            .submit_with_group(migration, group_spec)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let response = migration_operation_response(&operation, journal)?;
        drop(journals);
        if let Some(dispatch_group) = dispatch_group {
            self.schedule_migration(operation.clone(), dispatch_group);
        }
        Ok(Response::new(response))
    }

    async fn advance_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationAdvanceRequest>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let principal = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        if request.principal_id != principal {
            return Err(Status::permission_denied(
                "migration transition principal does not match authenticated identity",
            ));
        }
        let brain_id = migration_brain_id(request.brain_id)?;
        let transition = decode_migration_transition(&request.transition_json)?;
        let group_update = decode_migration_group_update(&request.group_update_json)?;
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if transition.observed_leader_term != orchestrator.state().leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        if !orchestrator.state().policy.allows_for_brain(
            &Principal { id: principal },
            &brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot advance migration for this brain",
            ));
        }
        drop(orchestrator);
        if let Some(path) = persistent_migration_path(brain_id) {
            let mut journal =
                PersistedMigrationJournal::open(path, brain_id, transition.observed_leader_term)
                    .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let mut transition = transition;
            if let Some(mut group_update) = group_update {
                if group_update.operation_id != transition.operation_id {
                    return Err(Status::invalid_argument(
                        "migration group update operation does not match transition",
                    ));
                }
                group_update.expected_resource_version = transition.expected_resource_version;
                journal
                    .apply_group_update(group_update)
                    .map_err(|error| Status::failed_precondition(error.to_string()))?;
                transition.expected_resource_version = journal.journal().resource_version;
            }
            let operation = journal
                .transition(transition)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            return Ok(Response::new(migration_operation_response(
                &operation,
                journal.journal(),
            )?));
        }
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals
            .get_mut(&brain_id)
            .ok_or_else(|| Status::not_found("migration brain not found"))?;
        let mut transition = transition;
        if let Some(mut group_update) = group_update {
            if group_update.operation_id != transition.operation_id {
                return Err(Status::invalid_argument(
                    "migration group update operation does not match transition",
                ));
            }
            group_update.expected_resource_version = transition.expected_resource_version;
            journal
                .apply_group_update(group_update)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            transition.expected_resource_version = journal.resource_version;
        }
        let operation = journal
            .transition(transition)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(migration_operation_response(
            &operation, journal,
        )?))
    }

    async fn get_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationLookup>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let principal = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        let brain_id = migration_brain_id(request.brain_id)?;
        let term = LeaseTerm::new(request.observed_leader_term)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let can_read = orchestrator.state().policy.allows_for_brain(
            &Principal { id: principal },
            &brain_id.to_string(),
            &Capability::Read,
        );
        if term != orchestrator.state().leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        if !can_read {
            return Err(Status::permission_denied(
                "principal cannot read migration state for this brain",
            ));
        }
        drop(orchestrator);
        if let Some(path) = persistent_migration_path(brain_id) {
            let journal = PersistedMigrationJournal::open(path, brain_id, term)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let operation = journal
                .journal()
                .operation(request.operation_id)
                .ok_or_else(|| Status::not_found("migration operation not found"))?;
            return Ok(Response::new(migration_operation_response(
                operation,
                journal.journal(),
            )?));
        }
        let journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals
            .get(&brain_id)
            .ok_or_else(|| Status::not_found("migration brain not found"))?;
        let operation = journal
            .operation(request.operation_id)
            .ok_or_else(|| Status::not_found("migration operation not found"))?;
        Ok(Response::new(migration_operation_response(
            operation, journal,
        )?))
    }

    async fn get_migration_status(
        &self,
        request: Request<crate::generated_management::proto::MigrationLookup>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        self.get_migration(request).await
    }

    async fn cancel_migration(
        &self,
        request: Request<crate::generated_management::proto::MigrationCancelRequest>,
    ) -> Result<Response<crate::generated_management::proto::MigrationOperationResponse>, Status>
    {
        let principal = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        if request.principal_id != principal {
            return Err(Status::permission_denied(
                "migration cancel principal does not match authenticated identity",
            ));
        }
        let brain_id = migration_brain_id(request.brain_id)?;
        if self
            .migration_in_flight
            .lock()
            .map_err(|_| Status::internal("migration dispatcher lock poisoned"))?
            .iter()
            .any(|(brain, operation)| *brain == brain_id && *operation == request.operation_id)
        {
            return Err(Status::failed_precondition(
                "migration cancellation is blocked while the registered executor is active",
            ));
        }
        let term = LeaseTerm::new(request.observed_leader_term)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if term != orchestrator.state().leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        if !orchestrator.state().policy.allows_for_brain(
            &Principal { id: principal },
            &brain_id.to_string(),
            &Capability::Operate,
        ) {
            return Err(Status::permission_denied(
                "principal cannot cancel migration for this brain",
            ));
        }
        drop(orchestrator);
        if let Some(path) = persistent_migration_path(brain_id) {
            let mut journal = PersistedMigrationJournal::open(path, brain_id, term)
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            let operation = journal
                .cancel(
                    request.operation_id,
                    term,
                    request.expected_resource_version,
                    request.reason,
                )
                .map_err(|error| Status::failed_precondition(error.to_string()))?;
            return Ok(Response::new(migration_operation_response(
                &operation,
                journal.journal(),
            )?));
        }
        let mut journals = self
            .migration_journals
            .lock()
            .map_err(|_| Status::internal("migration journal lock poisoned"))?;
        let journal = journals
            .get_mut(&brain_id)
            .ok_or_else(|| Status::not_found("migration brain not found"))?;
        let operation = journal
            .cancel(
                request.operation_id,
                term,
                request.expected_resource_version,
                request.reason,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(migration_operation_response(
            &operation, journal,
        )?))
    }

    async fn get_operation(
        &self,
        request: Request<crate::generated_management::proto::OperationLookup>,
    ) -> Result<Response<crate::generated_management::proto::OperationResponse>, Status> {
        let principal = authenticated_principal(&request)?.to_owned();
        let request = request.into_inner();
        if request.brain_id.trim().is_empty() {
            return Err(Status::invalid_argument("brain_id is required"));
        }
        let term = LeaseTerm::new(request.observed_leader_term)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let mut orchestrator = self
            .orchestrator
            .lock()
            .map_err(|_| Status::internal("management lock poisoned"))?;
        orchestrator
            .refresh()
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        if term != orchestrator.state().leader_term() {
            return Err(Status::failed_precondition("stale leader term"));
        }
        let id = EventId::new(request.operation_id)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let operation = orchestrator
            .operation(id)
            .filter(|operation| operation.brain_id == request.brain_id)
            .ok_or_else(|| Status::not_found("operation not found"))?;
        let can_read = operation.principal.id == principal
            || orchestrator.state().policy.allows_for_brain(
                &Principal { id: principal },
                &request.brain_id,
                &Capability::Read,
            );
        if !can_read {
            return Err(Status::permission_denied(
                "principal cannot read this operation",
            ));
        }
        let (state, error_code) = operation_state_name(&operation.state);
        Ok(Response::new(
            crate::generated_management::proto::OperationResponse {
                schema_version: crate::generated_management::MANAGEMENT_SCHEMA_VERSION,
                operation_id: operation.id.raw(),
                state,
                resource_version: orchestrator.state().resource_version(),
                leader_term: orchestrator.state().leader_term().raw(),
                error_code,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(term: LeaseTerm) -> MutationContext {
        MutationContext {
            observed_leader_term: term,
            expected_version: 0,
            idempotency_key: "operation-1".to_owned(),
            request_id: "request-1".to_owned(),
        }
    }

    #[test]
    fn stale_worker_cannot_transition_operation_after_fencing() {
        let mut policy = Policy::default();
        policy.grant("operator", Capability::Operate);
        let mut manager = ManagementOrchestrator::new(LeaseTerm::INITIAL, policy);
        let operation = manager
            .submit(
                Principal {
                    id: "operator".to_owned(),
                },
                Capability::Operate,
                context(LeaseTerm::INITIAL),
                OperationKind::Start,
            )
            .expect("operation is accepted by the current leader");

        let next_term = LeaseTerm::new(2).expect("non-zero term");
        manager.replace_leader_term(next_term);
        let result =
            manager.transition(operation.id, LeaseTerm::INITIAL, OperationState::Succeeded);

        assert!(matches!(
            result,
            Err(ManagementError::StaleLeader {
                expected,
                received
            }) if expected == next_term && received == LeaseTerm::INITIAL
        ));
        assert_eq!(
            manager
                .operation(operation.id)
                .map(|operation| &operation.state),
            Some(&OperationState::Pending)
        );
        assert_eq!(manager.audit().len(), 1);
    }

    #[test]
    fn operation_state_machine_rejects_terminal_rewrites() {
        let mut policy = Policy::default();
        policy.grant("operator", Capability::Operate);
        let mut manager = ManagementOrchestrator::new(LeaseTerm::INITIAL, policy);
        let operation = manager
            .submit(
                Principal {
                    id: "operator".to_owned(),
                },
                Capability::Operate,
                context(LeaseTerm::INITIAL),
                OperationKind::Start,
            )
            .unwrap();
        manager
            .transition(operation.id, LeaseTerm::INITIAL, OperationState::Running)
            .unwrap();
        manager
            .transition(operation.id, LeaseTerm::INITIAL, OperationState::Succeeded)
            .unwrap();
        assert!(matches!(
            manager.transition(operation.id, LeaseTerm::INITIAL, OperationState::Running,),
            Err(ManagementError::InvalidOperationTransition {
                from: OperationState::Succeeded,
                to: OperationState::Running,
            })
        ));
        assert_eq!(manager.audit().len(), 3);
    }

    #[test]
    fn brain_scoped_grants_do_not_escape_their_brain() {
        let mut policy = Policy::default();
        policy.grant_for_brain("operator", "brain-a", Capability::Read);
        let principal = Principal {
            id: "operator".to_owned(),
        };
        assert!(policy.allows_for_brain(&principal, "brain-a", &Capability::Read));
        assert!(!policy.allows_for_brain(&principal, "brain-b", &Capability::Read));
    }

    #[test]
    fn lease_issue_requires_quorum_and_replaces_old_fencing_token() {
        let mut authority =
            QuorumLeaseAuthority::new(["cp-a", "cp-b", "cp-c"]).expect("valid quorum membership");
        let shard = ShardId::new(7).expect("shard");
        let first = authority.issue_lease(shard, "cp-a").expect("initial lease");
        authority
            .validate(shard, "cp-a", first.term, first.fencing_token)
            .expect("current lease validates");

        authority
            .set_member_available("cp-a", false)
            .expect("member state");
        authority
            .set_member_available("cp-b", false)
            .expect("member state");
        assert!(matches!(
            authority.issue_lease(shard, "cp-c"),
            Err(QuorumError::QuorumUnavailable { .. })
        ));
        authority
            .set_member_available("cp-a", true)
            .expect("quorum restored");

        let second = authority
            .issue_lease(shard, "cp-c")
            .expect("replacement lease");
        assert!(second.term > first.term);
        assert!(matches!(
            authority.validate(shard, "cp-a", first.term, first.fencing_token),
            Err(QuorumError::Fenced { .. })
        ));
        authority
            .validate(shard, "cp-c", second.term, second.fencing_token)
            .expect("replacement validates");
    }

    #[test]
    fn brain_wide_promotion_fences_all_sources_at_one_destination_term() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-brain-promotion-{}-{}",
            std::process::id(),
            EventId::new(33).unwrap()
        ));
        let _ = fs::remove_dir_all(&root);
        let replicas = ["cp-a", "cp-b", "cp-c"]
            .into_iter()
            .map(|member| (member.to_owned(), root.join(format!("{member}.json"))))
            .collect::<Vec<_>>();
        let mut authority = ReplicatedQuorumLeaseAuthority::open(
            replicas.clone(),
            ["cp-a".to_owned(), "cp-b".to_owned(), "cp-c".to_owned()],
        )
        .unwrap();
        let first = authority
            .issue_lease(ShardId::new(41).unwrap(), "cp-a")
            .unwrap();
        let second = authority
            .issue_lease(ShardId::new(42).unwrap(), "cp-a")
            .unwrap();
        let promoted = authority
            .promote_leases(vec![
                LeasePromotionRequest {
                    shard_id: first.shard_id,
                    source_node: first.node_id.clone(),
                    source_term: first.term,
                    source_fencing_token: first.fencing_token,
                    destination_node: "cp-c".to_owned(),
                },
                LeasePromotionRequest {
                    shard_id: second.shard_id,
                    source_node: second.node_id.clone(),
                    source_term: second.term,
                    source_fencing_token: second.fencing_token,
                    destination_node: "cp-c".to_owned(),
                },
            ])
            .unwrap();
        assert_eq!(
            promoted[&first.shard_id].term,
            promoted[&second.shard_id].term
        );
        assert!(matches!(
            authority.validate_current(
                first.shard_id,
                &first.node_id,
                first.term,
                first.fencing_token
            ),
            Err(PersistedAuthorityError::Quorum(QuorumError::Fenced { .. }))
        ));
        for lease in promoted.values() {
            authority
                .validate_current(
                    lease.shard_id,
                    &lease.node_id,
                    lease.term,
                    lease.fencing_token,
                )
                .unwrap();
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn quorum_membership_rejects_duplicate_and_unknown_nodes() {
        assert!(matches!(
            QuorumLeaseAuthority::new(["cp-a", "cp-a"]),
            Err(QuorumError::DuplicateMember(_))
        ));
        let mut authority =
            QuorumLeaseAuthority::new(["cp-a", "cp-b", "cp-c"]).expect("valid quorum membership");
        assert!(matches!(
            authority.set_member_available("unknown", false),
            Err(QuorumError::UnknownMember(_))
        ));
        assert!(matches!(
            authority.issue_lease(ShardId::new(8).unwrap(), "unknown"),
            Err(QuorumError::UnknownLeaseNode(_))
        ));
    }

    #[test]
    fn persisted_authority_keeps_terms_and_fences_after_reopen() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-authority-{}-{}.json",
            std::process::id(),
            EventId::new(31).unwrap()
        ));
        let _ = fs::remove_file(&path);
        let mut authority = PersistedQuorumLeaseAuthority::open(
            &path,
            vec!["cp-a".to_owned(), "cp-b".to_owned(), "cp-c".to_owned()],
        )
        .unwrap();
        let shard = ShardId::new(31).unwrap();
        let first = authority.issue_lease(shard, "cp-a").unwrap();
        drop(authority);
        let mut reopened = PersistedQuorumLeaseAuthority::open(
            &path,
            vec!["cp-a".to_owned(), "cp-b".to_owned(), "cp-c".to_owned()],
        )
        .unwrap();
        reopened
            .validate(shard, "cp-a", first.term, first.fencing_token)
            .unwrap();
        let replacement = reopened.issue_lease(shard, "cp-b").unwrap();
        assert!(replacement.term > first.term);
        assert!(matches!(
            reopened.validate(shard, "cp-a", first.term, first.fencing_token),
            Err(QuorumError::Fenced { .. })
        ));
        fs::remove_file(&path).unwrap();
        let _ = fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn persisted_authority_readers_observe_a_fence_written_by_another_process() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-authority-live-fence-{}-{}.json",
            std::process::id(),
            EventId::new(32).unwrap()
        ));
        let _ = fs::remove_file(&path);
        let members = vec!["cp-a".to_owned(), "cp-b".to_owned(), "cp-c".to_owned()];
        let mut first = PersistedQuorumLeaseAuthority::open(&path, members.clone()).unwrap();
        let shard = ShardId::new(32).unwrap();
        let lease = first.issue_lease(shard, "cp-a").unwrap();
        let mut second = PersistedQuorumLeaseAuthority::open(&path, members).unwrap();
        let replacement = second.issue_lease(shard, "cp-b").unwrap();
        assert!(replacement.term > lease.term);
        assert!(matches!(
            first.validate_current(shard, "cp-a", lease.term, lease.fencing_token),
            Err(PersistedAuthorityError::Quorum(QuorumError::Fenced { .. }))
        ));
        fs::remove_file(&path).unwrap();
        let _ = fs::remove_file(path.with_extension("lock"));
    }

    #[test]
    fn replicated_authority_requires_majority_and_survives_reopen() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-replicated-authority-{}-{}",
            std::process::id(),
            EventId::new(33).unwrap()
        ));
        let _ = fs::remove_dir_all(&root);
        let members = ["cp-a", "cp-b", "cp-c"];
        let replicas = members
            .iter()
            .map(|member| ((*member).to_owned(), root.join(format!("{member}.json"))))
            .collect::<Vec<_>>();
        let mut authority = ReplicatedQuorumLeaseAuthority::open(
            replicas.clone(),
            members.iter().map(|member| (*member).to_owned()),
        )
        .unwrap();
        let shard = ShardId::new(33).unwrap();
        let first = authority.issue_lease(shard, "cp-a").unwrap();
        assert_eq!(authority.revision(), 1);
        drop(authority);

        let mut reopened = ReplicatedQuorumLeaseAuthority::open(
            replicas.clone(),
            members.iter().map(|member| (*member).to_owned()),
        )
        .unwrap();
        reopened
            .validate_current(shard, "cp-a", first.term, first.fencing_token)
            .unwrap();
        reopened.set_member_available("cp-a", false).unwrap();
        reopened.set_member_available("cp-b", false).unwrap();
        assert!(matches!(
            reopened.issue_lease(shard, "cp-c"),
            Err(PersistedAuthorityError::Quorum(
                QuorumError::QuorumUnavailable { .. }
            ))
        ));
        reopened.set_member_available("cp-a", true).unwrap();
        let replacement = reopened.issue_lease(shard, "cp-c").unwrap();
        assert!(replacement.term > first.term);

        let observer = ReplicatedQuorumLeaseAuthority::open(
            replicas,
            members.iter().map(|member| (*member).to_owned()),
        )
        .unwrap();
        assert!(matches!(
            observer.validate_current(shard, "cp-a", first.term, first.fencing_token),
            Err(PersistedAuthorityError::Quorum(QuorumError::Fenced { .. }))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replicated_authority_reopen_does_not_infer_failed_members_from_old_files() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-replicated-authority-availability-{}-{}",
            std::process::id(),
            EventId::new(35).unwrap()
        ));
        let _ = fs::remove_dir_all(&root);
        let members = ["cp-a", "cp-b", "cp-c"];
        let paths = members
            .iter()
            .map(|member| ((*member).to_owned(), root.join(format!("{member}.json"))))
            .collect::<BTreeMap<_, _>>();
        let member_set = members
            .iter()
            .map(|member| (*member).to_owned())
            .collect::<BTreeSet<_>>();
        let mut authority = ReplicatedQuorumLeaseAuthority::open_with_available(
            paths.clone(),
            member_set.clone(),
            ["cp-a", "cp-b"]
                .iter()
                .map(|member| (*member).to_owned())
                .collect(),
        )
        .unwrap();
        let shard = ShardId::new(35).unwrap();
        authority.issue_lease(shard, "cp-a").unwrap();
        drop(authority);

        let unavailable = ReplicatedQuorumLeaseAuthority::open_with_available(
            paths,
            member_set,
            ["cp-a"].iter().map(|member| (*member).to_owned()).collect(),
        );
        assert!(matches!(
            unavailable,
            Err(PersistedAuthorityError::Quorum(
                QuorumError::QuorumUnavailable { .. }
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persisted_management_keeps_idempotency_and_brain_scope_after_restart() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-management-state-{}-{}.json",
            std::process::id(),
            EventId::new(34).unwrap()
        ));
        let _ = fs::remove_file(&path);
        let mut policy = Policy::default();
        policy.grant("operator", Capability::Operate);
        let mut first =
            PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy.clone())
                .unwrap();
        let context = |key: &str| MutationContext {
            observed_leader_term: LeaseTerm::INITIAL,
            expected_version: 0,
            idempotency_key: key.to_owned(),
            request_id: format!("request-{key}"),
        };
        let accepted = first
            .submit_for_brain(
                Principal {
                    id: "operator".to_owned(),
                },
                Capability::Operate,
                context("same-operation"),
                OperationKind::Start,
                "brain-a".to_owned(),
            )
            .unwrap();
        assert_eq!(accepted.brain_id, "brain-a");
        drop(first);

        let mut reopened =
            PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy).unwrap();
        let duplicate = reopened
            .submit_for_brain(
                Principal {
                    id: "operator".to_owned(),
                },
                Capability::Operate,
                context("same-operation"),
                OperationKind::Start,
                "brain-a".to_owned(),
            )
            .unwrap();
        assert_eq!(duplicate.id, accepted.id);
        assert!(matches!(
            reopened.submit_for_brain(
                Principal {
                    id: "operator".to_owned()
                },
                Capability::Operate,
                context("same-operation"),
                OperationKind::Start,
                "brain-b".to_owned(),
            ),
            Err(PersistedAuthorityError::Management(
                ManagementError::IdempotencyConflict(_)
            ))
        ));
        fs::remove_file(&path).unwrap();
        let _ = fs::remove_file(path.with_extension("management.lock"));
    }

    #[test]
    fn oidc_verifier_rejects_missing_key_id_and_unknown_keys() {
        let jwks: jsonwebtoken::jwk::JwkSet =
            serde_json::from_str(r#"{"keys":[]}"#).expect("empty JWK set");
        let missing_kid = "eyJhbGciOiJSUzI1NiJ9.e30.x";
        let error = verify_oidc_management_token_with_jwks(
            missing_kid,
            "https://issuer.example",
            "aarnn-management",
            &jwks,
        )
        .expect_err("a token without kid must be rejected");
        assert!(error.contains("key ID is missing"));

        let unknown_kid = "eyJhbGciOiJSUzI1NiIsImtpZCI6Im1pc3NpbmcifQ.e30.x";
        let error = verify_oidc_management_token_with_jwks(
            unknown_kid,
            "https://issuer.example",
            "aarnn-management",
            &jwks,
        )
        .expect_err("a token with an unknown key must be rejected");
        assert!(error.contains("not present"));
    }

    #[test]
    fn oidc_verifier_rejects_symmetric_jwks_before_signature_validation() {
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_str(
            r#"{"keys":[{"kty":"oct","kid":"shared","alg":"HS256","k":"c2VjcmV0"}]}"#,
        )
        .expect("symmetric JWK set");
        let token = "eyJhbGciOiJIUzI1NiIsImtpZCI6InNoYXJlZCJ9.e30.x";
        let error = verify_oidc_management_token_with_jwks(
            token,
            "https://issuer.example",
            "aarnn-management",
            &jwks,
        )
        .expect_err("symmetric issuer keys must be rejected");
        assert!(error.contains("symmetric JWKs are not accepted"));
    }

    #[test]
    fn oidc_revocation_matches_subject_or_jwt_id_and_ignores_comments() {
        let list = "# deployed revocations\nuser-17\nrequest-jti-9\n";
        assert!(oidc_credential_is_revoked(list, "user-17", None));
        assert!(oidc_credential_is_revoked(
            list,
            "other-user",
            Some("request-jti-9")
        ));
        assert!(!oidc_credential_is_revoked(
            list,
            "other-user",
            Some("request-jti-10")
        ));
    }

    #[test]
    fn management_cutover_validator_is_pure_and_fails_closed() {
        let valid_oidc = || {
            validate_management_auth_values(
                true,
                "oidc-jwt",
                None,
                None,
                Some("https://issuer.example"),
                Some("aarnn-management"),
                Some("/run/aarnn/jwks.json"),
                Some("/run/aarnn/revoked.txt"),
            )
        };
        assert!(valid_oidc().is_ok());

        let missing_revocation = validate_management_auth_values(
            true,
            "oidc-jwt",
            None,
            None,
            Some("https://issuer.example"),
            Some("aarnn-management"),
            Some("/run/aarnn/jwks.json"),
            None,
        )
        .expect_err("production OIDC must have revocation state");
        assert!(missing_revocation.contains("NM_OIDC_REVOCATION_FILE"));

        let static_production = validate_management_auth_values(
            true,
            "static-reference",
            Some("token"),
            Some("operator"),
            None,
            None,
            None,
            None,
        )
        .expect_err("static bearer auth is reference-only");
        assert!(static_production.contains("oidc-jwt"));

        let static_reference = validate_management_auth_values(
            false,
            "static-reference",
            Some("token"),
            Some("operator"),
            None,
            None,
            None,
            None,
        );
        assert!(static_reference.is_ok());

        for (mode, expected) in [
            ("oidc-jwt", "OIDC issuer"),
            ("static-reference", "NM_MANAGEMENT_BEARER_TOKEN"),
            ("unknown", "unsupported NM_MANAGEMENT_AUTH_MODE"),
        ] {
            let result =
                validate_management_auth_values(false, mode, None, None, None, None, None, None)
                    .expect_err("an incomplete auth profile must be rejected");
            assert!(result.contains(expected), "{mode}: {result}");
        }

        let empty_oidc = validate_management_auth_values(
            false,
            "oidc",
            None,
            None,
            Some("  "),
            Some("audience"),
            Some("jwks"),
            None,
        )
        .expect_err("empty issuer must not be accepted");
        assert!(empty_oidc.contains("NM_OIDC_ISSUER"));
    }

    #[test]
    fn production_oidc_file_validation_checks_readable_nonempty_sources() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-management-oidc-files-{}-{}",
            std::process::id(),
            EventId::new(36).unwrap()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let jwks = root.join("jwks.json");
        let revocation = root.join("revoked.txt");
        fs::write(
            &jwks,
            r#"{"keys":[{"kty":"EC","crv":"P-256","x":"w7JAoU_gJbZJvV-zCOvU9yFJq0FNC_edCMRM78P8eQQ","y":"wQg1EytcsEmGrM70Gb53oluoDbVhCZ3Uq3hHMslHVb4","kid":"configuration-check","alg":"ES256","use":"sig"}]}"#,
        )
        .unwrap();
        fs::write(&revocation, "# no revoked credentials\n").unwrap();
        assert!(
            validate_production_oidc_files(jwks.to_str().unwrap(), revocation.to_str().unwrap())
                .is_ok()
        );

        fs::write(&jwks, b"not-json").unwrap();
        let malformed =
            validate_production_oidc_files(jwks.to_str().unwrap(), revocation.to_str().unwrap())
                .expect_err("malformed JWKS must be rejected before cutover");
        assert!(malformed.contains("cannot decode NM_OIDC_JWKS_FILE"));

        fs::write(&jwks, r#"{"keys":[]}"#).unwrap();
        let empty =
            validate_production_oidc_files(jwks.to_str().unwrap(), revocation.to_str().unwrap())
                .expect_err("an empty JWK set cannot verify production credentials");
        assert!(empty.contains("at least one key"));

        fs::write(
            &jwks,
            r#"{"keys":[{"kty":"EC","crv":"P-256","x":"w7JAoU_gJbZJvV-zCOvU9yFJq0FNC_edCMRM78P8eQQ","y":"wQg1EytcsEmGrM70Gb53oluoDbVhCZ3Uq3hHMslHVb4","kid":"configuration-check","alg":"ES256","use":"sig"}]}"#,
        )
        .unwrap();
        fs::remove_file(&revocation).unwrap();
        let missing =
            validate_production_oidc_files(jwks.to_str().unwrap(), revocation.to_str().unwrap())
                .expect_err("production cutover cannot use a missing revocation source");
        assert!(missing.contains("NM_OIDC_REVOCATION_FILE"));

        fs::create_dir(&revocation).unwrap();
        let non_file =
            validate_production_oidc_files(jwks.to_str().unwrap(), revocation.to_str().unwrap())
                .expect_err("a revocation directory is not a valid source");
        assert!(non_file.contains("must name a regular file"));
        fs::remove_dir_all(root).unwrap();
    }
}
