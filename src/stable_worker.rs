//! Stable executor worker registration and admission rules.
//!
//! A registration is a capability observation sent by a worker during join and
//! heartbeat. It describes the stable topology/partition the process has
//! opened and the local writer lease it currently observes. It does not grant
//! placement, quorum authority, or permission to accept neural events. Those
//! decisions remain in the orchestrator and fenced migration contracts.

use thiserror::Error;

/// Versioned command sent by an orchestrator to activate a partial stable
/// worker on one already enrolled node. The manifest is opaque to the
/// control-plane wire contract and is fully decoded and checkpoint-verified
/// by the target worker before registration. A digest binds retries and
/// prevents an altered manifest from being accepted as the same command.
pub const STABLE_WORKER_ACTIVATION_SCHEMA_VERSION: u32 = 1;
pub const MAX_STABLE_WORKER_ACTIVATION_BYTES: usize = 8 * 1024 * 1024;
/// Schema for the capability observation sent by an idle stable-worker
/// binary. This is separate from the registration schema because a worker can
/// be eligible to receive its first manifest without owning any shard yet.
pub const STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_STABLE_WORKER_MAX_INPUT_EVENTS: u32 = 4096;
pub const DEFAULT_STABLE_WORKER_MAX_STEPS_PER_POLL: u32 = 4096;
pub const MAX_STABLE_WORKER_POLL_BUDGET: u32 = 1_000_000;
pub const STABLE_WORKER_CHECKPOINT_TRANSFER_REFERENCE_SCHEMA_VERSION: u32 = 1;

/// A target-local checkpoint transfer receipt referenced by a worker
/// activation.  The reference contains no filesystem path: the target must
/// resolve the checkpoint below its own configured transfer root.  All
/// identity and digest fields are repeated so an activation cannot bind a
/// worker to a checkpoint from another brain, plan or lease generation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StableWorkerCheckpointTransferReference {
    pub schema_version: u32,
    pub transfer_id: u64,
    pub checkpoint_id: u64,
    pub brain_id: u64,
    pub lease_term: u64,
    pub partition_generation: u64,
    pub plan_digest: String,
    pub payload_digest: String,
    pub manifest_digest: String,
}

impl StableWorkerCheckpointTransferReference {
    pub fn validate(&self) -> Result<(), StableWorkerActivationError> {
        if self.schema_version != STABLE_WORKER_CHECKPOINT_TRANSFER_REFERENCE_SCHEMA_VERSION
            || self.transfer_id == 0
            || self.checkpoint_id == 0
            || self.brain_id == 0
            || self.lease_term == 0
            || self.partition_generation == 0
        {
            return Err(StableWorkerActivationError::InvalidField(
                "checkpoint_transfer_reference",
            ));
        }
        for (field, value) in [
            ("checkpoint_transfer_plan_digest", self.plan_digest.as_str()),
            (
                "checkpoint_transfer_payload_digest",
                self.payload_digest.as_str(),
            ),
            (
                "checkpoint_transfer_manifest_digest",
                self.manifest_digest.as_str(),
            ),
        ] {
            if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(StableWorkerActivationError::InvalidField(field));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StableExecutorCapability {
    pub schema_version: u32,
    pub profile: String,
    pub activation_schema_version: u32,
    pub max_input_events: u32,
    pub max_steps_per_poll: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StableExecutorCapabilityError {
    #[error("stable executor capability schema version is unsupported")]
    UnsupportedSchema,
    #[error("stable executor capability profile is unsupported")]
    UnsupportedProfile,
    #[error("stable executor capability activation schema version is unsupported")]
    UnsupportedActivationSchema,
    #[error("stable executor capability poll budgets are invalid")]
    InvalidPollBudget,
}

impl StableExecutorCapability {
    pub fn validate(&self) -> Result<(), StableExecutorCapabilityError> {
        if self.schema_version != STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION {
            return Err(StableExecutorCapabilityError::UnsupportedSchema);
        }
        if self.profile != STABLE_EXECUTOR_PROFILE {
            return Err(StableExecutorCapabilityError::UnsupportedProfile);
        }
        if self.activation_schema_version != STABLE_WORKER_ACTIVATION_SCHEMA_VERSION {
            return Err(StableExecutorCapabilityError::UnsupportedActivationSchema);
        }
        if self.max_input_events == 0
            || self.max_steps_per_poll == 0
            || self.max_input_events > MAX_STABLE_WORKER_POLL_BUDGET
            || self.max_steps_per_poll > MAX_STABLE_WORKER_POLL_BUDGET
        {
            return Err(StableExecutorCapabilityError::InvalidPollBudget);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StableWorkerActivationCommand {
    pub schema_version: u32,
    pub request_id: String,
    pub operation_id: u64,
    pub brain_id: u64,
    pub network_id: String,
    pub target_node: String,
    pub manifest_json: String,
    pub manifest_digest: String,
    /// Placement idempotency key supplied by the management boundary. Empty
    /// is retained for standalone/reference activation commands that are not
    /// paired with a placement registry record.
    #[serde(default)]
    pub placement_idempotency_key: String,
    /// Optional reference to a checkpoint already received by the target's
    /// bounded checkpoint-transfer service.  The reference is deliberately
    /// separate from the manifest so the target can rebase source-local
    /// paths before opening the worker.
    #[serde(default)]
    pub checkpoint_transfer: Option<StableWorkerCheckpointTransferReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StableWorkerActivationError {
    #[error("stable worker activation schema version is unsupported")]
    UnsupportedSchema,
    #[error("stable worker activation field {0} is invalid")]
    InvalidField(&'static str),
    #[error("stable worker activation manifest is too large")]
    ManifestTooLarge,
    #[error("stable worker activation manifest digest does not match its contents")]
    DigestMismatch,
    #[error("stable worker activation manifest is not valid JSON")]
    InvalidManifest,
}

impl StableWorkerActivationCommand {
    pub fn new(
        request_id: impl Into<String>,
        operation_id: u64,
        brain_id: u64,
        network_id: impl Into<String>,
        target_node: impl Into<String>,
        manifest_json: impl Into<String>,
    ) -> Result<Self, StableWorkerActivationError> {
        let manifest_json = manifest_json.into();
        let command = Self {
            schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
            request_id: request_id.into(),
            operation_id,
            brain_id,
            network_id: network_id.into(),
            target_node: target_node.into(),
            manifest_digest: sha256_hex(manifest_json.as_bytes()),
            manifest_json,
            placement_idempotency_key: String::new(),
            checkpoint_transfer: None,
        };
        command.verify()?;
        Ok(command)
    }

    pub fn verify(&self) -> Result<(), StableWorkerActivationError> {
        if self.schema_version != STABLE_WORKER_ACTIVATION_SCHEMA_VERSION {
            return Err(StableWorkerActivationError::UnsupportedSchema);
        }
        for (field, value) in [
            ("request_id", self.request_id.as_str()),
            ("network_id", self.network_id.as_str()),
            ("target_node", self.target_node.as_str()),
        ] {
            if value.trim().is_empty() || value.len() > 256 {
                return Err(StableWorkerActivationError::InvalidField(field));
            }
        }
        if self.placement_idempotency_key.len() > 256 {
            return Err(StableWorkerActivationError::InvalidField(
                "placement_idempotency_key",
            ));
        }
        if let Some(reference) = &self.checkpoint_transfer {
            reference.validate()?;
            if reference.brain_id != self.brain_id {
                return Err(StableWorkerActivationError::InvalidField(
                    "checkpoint_transfer_reference.brain_id",
                ));
            }
        }
        if self.operation_id == 0 || self.brain_id == 0 {
            return Err(StableWorkerActivationError::InvalidField("identity"));
        }
        if self.manifest_json.is_empty() {
            return Err(StableWorkerActivationError::InvalidField("manifest_json"));
        }
        if self.manifest_json.len() > MAX_STABLE_WORKER_ACTIVATION_BYTES {
            return Err(StableWorkerActivationError::ManifestTooLarge);
        }
        if serde_json::from_str::<serde_json::Value>(&self.manifest_json).is_err() {
            return Err(StableWorkerActivationError::InvalidManifest);
        }
        if self.manifest_digest != sha256_hex(self.manifest_json.as_bytes())
            || self.manifest_digest.len() != 64
            || !self
                .manifest_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(StableWorkerActivationError::DigestMismatch);
        }
        Ok(())
    }

    /// Bind a command to the immutable placement request that authorised it.
    /// The binding is control-plane metadata and does not alter the manifest
    /// digest, so retries still identify the same worker bootstrap payload.
    pub fn bind_placement_idempotency_key(
        &mut self,
        key: impl Into<String>,
    ) -> Result<(), StableWorkerActivationError> {
        self.placement_idempotency_key = key.into();
        self.verify()
    }

    /// Bind an already materialised target-local checkpoint to this
    /// activation.  The command remains safe to retry because the reference
    /// is immutable and validated together with the manifest envelope.
    pub fn bind_checkpoint_transfer(
        &mut self,
        reference: StableWorkerCheckpointTransferReference,
    ) -> Result<(), StableWorkerActivationError> {
        self.checkpoint_transfer = Some(reference);
        self.verify()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Wire profile name for the stable-ID executor worker contract.
pub const STABLE_EXECUTOR_PROFILE: &str = "stable_executor_v3";
/// Schema version for [`StableWorkerRegistration`]. Version 3 adds one
/// durable application acknowledgement for every materialised shard.
pub const STABLE_WORKER_REGISTRATION_SCHEMA_VERSION: u32 = 3;

/// Durable application evidence for one materialised virtual shard.
///
/// This is an observation carried by join/heartbeat. It does not grant a
/// writer lease. The orchestrator accepts it only when every identity and
/// fence field matches the enclosing registration and the acknowledgement set
/// exactly covers `owned_shard_ids`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableShardApplicationAck {
    pub shard_id: u64,
    pub brain_id: u64,
    pub topology_generation: u64,
    pub partition_generation: u64,
    pub plan_digest: String,
    pub lease_term: u64,
    pub fencing_token: u64,
    pub applied_tick: u64,
    pub applied_microstep: u32,
    pub state_digest: String,
    pub durable_wal_sequence: Option<u64>,
    pub committed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableWorkerRegistration {
    pub schema_version: u32,
    pub profile: String,
    pub network_id: String,
    pub brain_id: u64,
    pub topology_generation: u64,
    pub partition_generation: u64,
    pub topology_digest: String,
    pub plan_digest: String,
    /// Complete stable virtual-shard inventory for the immutable plan.
    /// This remains part of plan identity.
    pub shard_ids: Vec<u64>,
    /// Stable virtual-shard IDs currently materialised by this worker.
    /// This is ownership telemetry and may change at an authorised migration
    /// boundary without changing the biological plan identity.
    pub owned_shard_ids: Vec<u64>,
    /// One durable application acknowledgement for every owned shard.
    pub application_acks: Vec<StableShardApplicationAck>,
    pub lease_term: u64,
    pub fencing_token: u64,
    pub current_tick: u64,
    pub current_microstep: u32,
    pub state_digest: String,
    pub max_input_events: u32,
    pub max_steps_per_poll: u32,
    pub authoritative: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StableWorkerRegistrationError {
    #[error("stable worker registration schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("stable worker registration profile '{0}' is unsupported")]
    UnsupportedProfile(String),
    #[error("stable worker registration network ID is empty")]
    EmptyNetworkId,
    #[error("stable worker registration brain ID must be non-zero")]
    ZeroBrainId,
    #[error("stable worker registration topology generation must be non-zero")]
    ZeroTopologyGeneration,
    #[error("stable worker registration partition generation must be non-zero")]
    ZeroPartitionGeneration,
    #[error("stable worker registration {field} digest must be 32 hexadecimal characters")]
    InvalidDigest { field: &'static str },
    #[error("stable worker registration must advertise at least one shard")]
    EmptyShardSet,
    #[error("stable worker registration shard IDs must be sorted and unique")]
    UnorderedShardSet,
    #[error("stable worker registration owned shard IDs must be sorted and unique")]
    UnorderedOwnedShardSet,
    #[error("stable worker registration owned shard IDs must be a subset of the complete plan")]
    OwnedShardNotInPlan,
    #[error("stable worker registration application acknowledgements must be sorted and unique")]
    UnorderedApplicationAcks,
    #[error(
        "stable worker registration application acknowledgements must exactly cover owned shards"
    )]
    ApplicationAckSetMismatch,
    #[error("stable worker registration application acknowledgement has an invalid identity")]
    InvalidApplicationAck,
    #[error("stable worker registration application acknowledgement is not committed")]
    ApplicationAckNotCommitted,
    #[error("stable worker registration shard ID must be non-zero")]
    ZeroShardId,
    #[error("stable worker registration lease term must be non-zero")]
    ZeroLeaseTerm,
    #[error("stable worker registration fencing token must be non-zero")]
    ZeroFencingToken,
    #[error("stable worker registration input and poll budgets must be non-zero")]
    ZeroPollBudget,
    #[error("stable worker registration must hold local authority before execution")]
    NotAuthoritative,
    #[error("stable worker registration plan identity changed during the session")]
    PlanIdentityChanged,
    #[error("stable worker registration lease term regressed during the session")]
    LeaseTermRegressed,
    #[error("stable worker registration fencing token regressed during the session")]
    FencingTokenRegressed,
    #[error(
        "stable worker registration ownership changed without a newer fenced migration boundary"
    )]
    OwnershipChangedWithoutBoundary,
}

impl StableWorkerRegistration {
    /// Validate all fields before they are admitted to orchestrator state.
    pub fn validate(&self) -> Result<(), StableWorkerRegistrationError> {
        if self.schema_version != STABLE_WORKER_REGISTRATION_SCHEMA_VERSION {
            return Err(StableWorkerRegistrationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.profile != STABLE_EXECUTOR_PROFILE {
            return Err(StableWorkerRegistrationError::UnsupportedProfile(
                self.profile.clone(),
            ));
        }
        if self.network_id.trim().is_empty() {
            return Err(StableWorkerRegistrationError::EmptyNetworkId);
        }
        if self.brain_id == 0 {
            return Err(StableWorkerRegistrationError::ZeroBrainId);
        }
        if self.topology_generation == 0 {
            return Err(StableWorkerRegistrationError::ZeroTopologyGeneration);
        }
        if self.partition_generation == 0 {
            return Err(StableWorkerRegistrationError::ZeroPartitionGeneration);
        }
        validate_digest("topology", &self.topology_digest)?;
        validate_digest("plan", &self.plan_digest)?;
        validate_digest("state", &self.state_digest)?;
        if self.shard_ids.is_empty() {
            return Err(StableWorkerRegistrationError::EmptyShardSet);
        }
        if self.shard_ids.iter().any(|id| *id == 0) {
            return Err(StableWorkerRegistrationError::ZeroShardId);
        }
        if self
            .shard_ids
            .windows(2)
            .any(|window| window[0] >= window[1])
        {
            return Err(StableWorkerRegistrationError::UnorderedShardSet);
        }
        if self.owned_shard_ids.iter().any(|id| *id == 0) {
            return Err(StableWorkerRegistrationError::ZeroShardId);
        }
        if self
            .owned_shard_ids
            .windows(2)
            .any(|window| window[0] >= window[1])
        {
            return Err(StableWorkerRegistrationError::UnorderedOwnedShardSet);
        }
        if self
            .owned_shard_ids
            .iter()
            .any(|owned| self.shard_ids.binary_search(owned).is_err())
        {
            return Err(StableWorkerRegistrationError::OwnedShardNotInPlan);
        }
        if self.application_acks.len() != self.owned_shard_ids.len() {
            return Err(StableWorkerRegistrationError::ApplicationAckSetMismatch);
        }
        if self
            .application_acks
            .windows(2)
            .any(|window| window[0].shard_id >= window[1].shard_id)
        {
            return Err(StableWorkerRegistrationError::UnorderedApplicationAcks);
        }
        if self
            .application_acks
            .iter()
            .zip(&self.owned_shard_ids)
            .any(|(ack, shard)| ack.shard_id != *shard)
        {
            return Err(StableWorkerRegistrationError::ApplicationAckSetMismatch);
        }
        for ack in &self.application_acks {
            if ack.brain_id != self.brain_id
                || ack.topology_generation != self.topology_generation
                || ack.partition_generation != self.partition_generation
                || ack.plan_digest != self.plan_digest
                || ack.lease_term != self.lease_term
                || ack.fencing_token != self.fencing_token
                || ack.applied_tick > self.current_tick
                || (ack.applied_tick == self.current_tick
                    && ack.applied_microstep > self.current_microstep)
            {
                return Err(StableWorkerRegistrationError::InvalidApplicationAck);
            }
            validate_digest("application state", &ack.state_digest)?;
            if !ack.committed {
                return Err(StableWorkerRegistrationError::ApplicationAckNotCommitted);
            }
        }
        if self.lease_term == 0 {
            return Err(StableWorkerRegistrationError::ZeroLeaseTerm);
        }
        if self.fencing_token == 0 {
            return Err(StableWorkerRegistrationError::ZeroFencingToken);
        }
        if self.max_input_events == 0 || self.max_steps_per_poll == 0 {
            return Err(StableWorkerRegistrationError::ZeroPollBudget);
        }
        if !self.authoritative {
            return Err(StableWorkerRegistrationError::NotAuthoritative);
        }
        Ok(())
    }

    /// Compare immutable biological/partition identity while allowing live
    /// lease, logical-frontier, state-digest and budget telemetry to change.
    pub fn same_plan_identity(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.profile == other.profile
            && self.network_id == other.network_id
            && self.brain_id == other.brain_id
            && self.topology_generation == other.topology_generation
            && self.partition_generation == other.partition_generation
            && self.topology_digest == other.topology_digest
            && self.plan_digest == other.plan_digest
            && self.shard_ids == other.shard_ids
    }

    /// Validate an update against the last accepted observation for this
    /// worker and network. Ordinary heartbeats may change frontier, digest,
    /// budgets and lease telemetry, but changing materialised ownership needs
    /// a newer lease and fencing token. The orchestrator's migration journal
    /// remains the authority for the actual handoff; this check prevents an
    /// uncoordinated heartbeat from presenting one.
    pub fn validate_update_from(
        &self,
        previous: &Self,
    ) -> Result<(), StableWorkerRegistrationError> {
        self.validate()?;
        if !self.same_plan_identity(previous) {
            return Err(StableWorkerRegistrationError::PlanIdentityChanged);
        }
        if self.lease_term < previous.lease_term {
            return Err(StableWorkerRegistrationError::LeaseTermRegressed);
        }
        if self.fencing_token < previous.fencing_token {
            return Err(StableWorkerRegistrationError::FencingTokenRegressed);
        }
        if self.owned_shard_ids != previous.owned_shard_ids
            && (self.lease_term <= previous.lease_term
                || self.fencing_token <= previous.fencing_token)
        {
            return Err(StableWorkerRegistrationError::OwnershipChangedWithoutBoundary);
        }
        Ok(())
    }
}

fn validate_digest(field: &'static str, digest: &str) -> Result<(), StableWorkerRegistrationError> {
    if digest.len() != 32 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StableWorkerRegistrationError::InvalidDigest { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> StableWorkerRegistration {
        StableWorkerRegistration {
            schema_version: STABLE_WORKER_REGISTRATION_SCHEMA_VERSION,
            profile: STABLE_EXECUTOR_PROFILE.to_owned(),
            network_id: "brain-a".to_owned(),
            brain_id: 1,
            topology_generation: 1,
            partition_generation: 1,
            topology_digest: "00".repeat(16),
            plan_digest: "11".repeat(16),
            shard_ids: vec![1, 2],
            owned_shard_ids: vec![1, 2],
            application_acks: vec![ack(1), ack(2)],
            lease_term: 1,
            fencing_token: 1,
            current_tick: 0,
            current_microstep: 0,
            state_digest: "22".repeat(16),
            max_input_events: 1,
            max_steps_per_poll: 1,
            authoritative: true,
        }
    }

    fn ack(shard_id: u64) -> StableShardApplicationAck {
        StableShardApplicationAck {
            shard_id,
            brain_id: 1,
            topology_generation: 1,
            partition_generation: 1,
            plan_digest: "11".repeat(16),
            lease_term: 1,
            fencing_token: 1,
            applied_tick: 0,
            applied_microstep: 0,
            state_digest: "33".repeat(16),
            durable_wal_sequence: Some(0),
            committed: true,
        }
    }

    #[test]
    fn registration_validation_rejects_unsafe_shape() {
        let mut registration = valid();
        registration.shard_ids = vec![2, 1];
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::UnorderedShardSet)
        );

        let mut registration = valid();
        registration.authoritative = false;
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::NotAuthoritative)
        );
    }

    #[test]
    fn registration_plan_identity_ignores_live_telemetry() {
        let first = valid();
        let mut second = first.clone();
        second.current_tick = 5;
        second.state_digest = "33".repeat(16);
        second.lease_term = 2;
        assert!(first.same_plan_identity(&second));
        second.owned_shard_ids = vec![2];
        second.application_acks = vec![ack(2)];
        second.application_acks[0].lease_term = 2;
        assert!(first.validate().is_ok());
        assert!(first.same_plan_identity(&second));
        assert_eq!(
            second.validate_update_from(&first),
            Err(StableWorkerRegistrationError::OwnershipChangedWithoutBoundary)
        );
        second.lease_term = 2;
        second.fencing_token = 2;
        second.application_acks[0].lease_term = 2;
        second.application_acks[0].fencing_token = 2;
        assert!(second.validate_update_from(&first).is_ok());
        second.plan_digest = "44".repeat(16);
        assert!(!first.same_plan_identity(&second));
    }

    #[test]
    fn registration_rejects_malformed_owned_shard_sets() {
        let mut registration = valid();
        registration.owned_shard_ids = vec![2, 1];
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::UnorderedOwnedShardSet)
        );

        let mut registration = valid();
        registration.owned_shard_ids = vec![3];
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::OwnedShardNotInPlan)
        );
    }

    #[test]
    fn registration_requires_fenced_durable_ack_for_every_owned_shard() {
        let mut registration = valid();
        registration.application_acks.pop();
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::ApplicationAckSetMismatch)
        );

        let mut registration = valid();
        registration.application_acks[0].fencing_token = 2;
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::InvalidApplicationAck)
        );

        let mut registration = valid();
        registration.application_acks[0].committed = false;
        assert_eq!(
            registration.validate(),
            Err(StableWorkerRegistrationError::ApplicationAckNotCommitted)
        );

        let mut drained = valid();
        drained.owned_shard_ids.clear();
        drained.application_acks.clear();
        assert!(
            drained.validate().is_ok(),
            "a drained worker must be able to acknowledge zero owned shards"
        );
    }

    fn capability() -> StableExecutorCapability {
        StableExecutorCapability {
            schema_version: STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
            profile: STABLE_EXECUTOR_PROFILE.to_owned(),
            activation_schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
            max_input_events: DEFAULT_STABLE_WORKER_MAX_INPUT_EVENTS,
            max_steps_per_poll: DEFAULT_STABLE_WORKER_MAX_STEPS_PER_POLL,
        }
    }

    #[test]
    fn idle_capability_is_versioned_and_bounded() {
        assert!(capability().validate().is_ok());

        let mut malformed = capability();
        malformed.activation_schema_version += 1;
        assert_eq!(
            malformed.validate(),
            Err(StableExecutorCapabilityError::UnsupportedActivationSchema)
        );

        let mut oversized = capability();
        oversized.max_steps_per_poll = MAX_STABLE_WORKER_POLL_BUDGET + 1;
        assert_eq!(
            oversized.validate(),
            Err(StableExecutorCapabilityError::InvalidPollBudget)
        );
    }

    #[test]
    fn activation_command_binds_manifest_digest_and_is_retry_safe() {
        let mut command = StableWorkerActivationCommand::new(
            "activation-request",
            7,
            42,
            "brain-42",
            "worker-a",
            r#"{"schema_version":1,"node_id":"worker-a"}"#,
        )
        .expect("valid activation command");
        command
            .bind_placement_idempotency_key("placement-key")
            .expect("placement binding");
        assert!(command.verify().is_ok());
        let encoded = serde_json::to_string(&command).unwrap();
        let decoded: StableWorkerActivationCommand = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, command);
        assert_eq!(decoded.placement_idempotency_key, "placement-key");
    }

    #[test]
    fn activation_command_rejects_manifest_tampering_and_unbounded_input() {
        let mut command = StableWorkerActivationCommand::new(
            "activation-request",
            7,
            42,
            "brain-42",
            "worker-a",
            r#"{"schema_version":1}"#,
        )
        .unwrap();
        command.manifest_json.push(' ');
        assert_eq!(
            command.verify(),
            Err(StableWorkerActivationError::DigestMismatch)
        );

        let too_large = StableWorkerActivationCommand {
            schema_version: STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
            request_id: "request".to_owned(),
            operation_id: 1,
            brain_id: 42,
            network_id: "brain-42".to_owned(),
            target_node: "worker-a".to_owned(),
            manifest_json: "x".repeat(MAX_STABLE_WORKER_ACTIVATION_BYTES + 1),
            manifest_digest: String::new(),
            placement_idempotency_key: String::new(),
            checkpoint_transfer: None,
        };
        assert_eq!(
            too_large.verify(),
            Err(StableWorkerActivationError::ManifestTooLarge)
        );
    }
}
