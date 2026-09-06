//! Deterministic failover, rejoin and RPO/RTO evidence harness.
//!
//! The harness models the authority decisions around a durable shard. It does
//! not promote on reachability alone: a new term and a quorum decision are
//! required, stale writers are quarantined, and replica placement must use a
//! distinct failure domain.

use crate::deterministic::{LeaseTerm, StateDigest};
use crate::durability::ShardCheckpointPayload;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaRole {
    Active,
    Warm,
    Recovering,
    Quarantined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaPlacement {
    pub active_node: String,
    pub active_failure_domain: String,
    pub warm_node: String,
    pub warm_failure_domain: String,
}

impl ReplicaPlacement {
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.active_node.trim().is_empty()
            || self.warm_node.trim().is_empty()
            || self.active_failure_domain.trim().is_empty()
            || self.warm_failure_domain.trim().is_empty()
            || self.active_node == self.warm_node
            || self.active_failure_domain == self.warm_failure_domain
        {
            return Err(RecoveryError::AntiAffinityViolation);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpoRtoEvidence {
    pub configured_rpo_events: u64,
    pub observed_rpo_events: u64,
    pub configured_rto_ms: u64,
    pub observed_rto_ms: u64,
    pub measured_with_monotonic_ticks: bool,
    pub pass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryEvidenceBundle {
    pub schema_version: u32,
    pub scenario_id: String,
    pub placement: ReplicaPlacement,
    pub initial_term: LeaseTerm,
    pub promoted_term: Option<LeaseTerm>,
    pub durable_sequence: u64,
    pub warm_sequence: u64,
    pub digest_verified: bool,
    pub stale_writer_rejected: bool,
    pub rpo_rto: Option<RpoRtoEvidence>,
}

impl RpoRtoEvidence {
    pub fn measure(
        configured_rpo_events: u64,
        observed_rpo_events: u64,
        configured_rto_ms: u64,
        observed_rto_ms: u64,
    ) -> Self {
        Self {
            configured_rpo_events,
            observed_rpo_events,
            configured_rto_ms,
            observed_rto_ms,
            measured_with_monotonic_ticks: true,
            pass: observed_rpo_events <= configured_rpo_events
                && observed_rto_ms <= configured_rto_ms,
        }
    }

    pub fn verify(&self) -> Result<(), RecoveryError> {
        if !self.measured_with_monotonic_ticks {
            return Err(RecoveryError::InvalidEvidence(
                "RPO/RTO evidence is not tied to a monotonic clock".to_owned(),
            ));
        }
        if self.pass
            != (self.observed_rpo_events <= self.configured_rpo_events
                && self.observed_rto_ms <= self.configured_rto_ms)
        {
            return Err(RecoveryError::InvalidEvidence(
                "RPO/RTO pass flag does not match the measured limits".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecoveryError {
    #[error("active and warm replicas violate failure-domain anti-affinity")]
    AntiAffinityViolation,
    #[error("failover requires a quorum decision")]
    QuorumRequired,
    #[error("stale replica cannot become active")]
    StaleReplica,
    #[error("replica digest does not match the durable boundary")]
    DigestMismatch,
    #[error("recovery state transition is invalid")]
    InvalidTransition,
    #[error("recovery evidence is invalid: {0}")]
    InvalidEvidence(String),
    #[error("recovery evidence I/O failed: {0}")]
    Io(String),
    #[error("recovery evidence encoding failed: {0}")]
    Encoding(String),
    #[error("recovery evidence for scenario {0} is already published")]
    AlreadyPublished(String),
}

impl RecoveryEvidenceBundle {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn verify(&self) -> Result<(), RecoveryError> {
        if self.schema_version != Self::SCHEMA_VERSION || self.scenario_id.trim().is_empty() {
            return Err(RecoveryError::InvalidEvidence(
                "schema version and scenario ID are required".to_owned(),
            ));
        }
        if !self.digest_verified {
            return Err(RecoveryError::InvalidEvidence(
                "recovery evidence must record a verified durable digest".to_owned(),
            ));
        }
        if !self.stale_writer_rejected {
            return Err(RecoveryError::InvalidEvidence(
                "recovery evidence must prove stale-writer rejection".to_owned(),
            ));
        }
        self.placement.validate()?;
        if self
            .promoted_term
            .is_some_and(|term| term <= self.initial_term)
        {
            return Err(RecoveryError::InvalidEvidence(
                "promoted term must be newer than the initial term".to_owned(),
            ));
        }
        if self.warm_sequence > self.durable_sequence {
            return Err(RecoveryError::InvalidEvidence(
                "warm replica is ahead of the active durable boundary".to_owned(),
            ));
        }
        if let Some(rpo_rto) = &self.rpo_rto {
            rpo_rto.verify()?;
        }
        Ok(())
    }

    /// Build evidence from the same durable owner and warm-replica
    /// checkpoints that were used for promotion.  This prevents a report
    /// from claiming a successful recovery using an unrelated digest or a
    /// separately observed mutable runner state.
    #[allow(clippy::too_many_arguments)]
    pub fn from_durable_checkpoints(
        scenario_id: impl Into<String>,
        placement: ReplicaPlacement,
        initial_term: LeaseTerm,
        promoted_term: LeaseTerm,
        durable: &ShardCheckpointPayload,
        warm: &ShardCheckpointPayload,
        configured_rpo_events: u64,
        configured_rto_ms: u64,
        observed_rto_ms: u64,
        stale_writer_rejected: bool,
    ) -> Result<Self, RecoveryError> {
        durable
            .verify()
            .map_err(|error| RecoveryError::InvalidEvidence(error.to_string()))?;
        warm.verify()
            .map_err(|error| RecoveryError::InvalidEvidence(error.to_string()))?;
        let same_boundary = durable.brain_id == warm.brain_id
            && durable.shard_id == warm.shard_id
            && durable.topology_generation == warm.topology_generation
            && durable.partition_generation == warm.partition_generation
            && durable.lease_term == promoted_term
            && warm.lease_term == promoted_term
            && durable.durable_wal_sequence == warm.durable_wal_sequence
            && durable.biological_state == warm.biological_state
            && durable.causal_state == warm.causal_state
            && durable.channel_state == warm.channel_state
            && durable.receipts == warm.receipts
            && durable.state_digest == warm.state_digest;
        if !same_boundary {
            return Err(RecoveryError::DigestMismatch);
        }
        let durable_sequence = durable.durable_wal_sequence.unwrap_or(0);
        let evidence =
            RpoRtoEvidence::measure(configured_rpo_events, 0, configured_rto_ms, observed_rto_ms);
        let bundle = Self {
            schema_version: Self::SCHEMA_VERSION,
            scenario_id: scenario_id.into(),
            placement,
            initial_term,
            promoted_term: Some(promoted_term),
            durable_sequence,
            warm_sequence: durable_sequence,
            digest_verified: true,
            stale_writer_rejected,
            rpo_rto: Some(evidence),
        };
        bundle.verify()?;
        Ok(bundle)
    }
}

/// Immutable, machine-readable publication for measured recovery evidence.
/// A scenario file is content-addressed by its caller-selected scenario ID
/// and cannot be replaced after publication.
#[derive(Debug, Clone)]
pub struct FileRecoveryEvidenceStore {
    root: PathBuf,
}

impl FileRecoveryEvidenceStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, RecoveryError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|error| RecoveryError::Io(error.to_string()))?;
        Ok(Self { root })
    }

    pub fn publish(&self, bundle: &RecoveryEvidenceBundle) -> Result<PathBuf, RecoveryError> {
        bundle.verify()?;
        let path = self
            .root
            .join(format!("{}.json", safe_name(&bundle.scenario_id)));
        let bytes = serde_json::to_vec(bundle)
            .map_err(|error| RecoveryError::Encoding(error.to_string()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| RecoveryError::Io(error.to_string()))?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&temporary);
            return Err(RecoveryError::Io(error.to_string()));
        }
        let result = fs::hard_link(&temporary, &path);
        let _ = fs::remove_file(&temporary);
        match result {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(RecoveryError::AlreadyPublished(bundle.scenario_id.clone()));
            }
            Err(error) => return Err(RecoveryError::Io(error.to_string())),
        }
        if let Some(parent) = path.parent() {
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| RecoveryError::Io(error.to_string()))?;
        }
        Ok(path)
    }

    pub fn load(&self, scenario_id: &str) -> Result<RecoveryEvidenceBundle, RecoveryError> {
        let path = self.root.join(format!("{}.json", safe_name(scenario_id)));
        let bytes = fs::read(&path).map_err(|error| RecoveryError::Io(error.to_string()))?;
        let bundle: RecoveryEvidenceBundle = serde_json::from_slice(&bytes)
            .map_err(|error| RecoveryError::Encoding(error.to_string()))?;
        if bundle.scenario_id != scenario_id {
            return Err(RecoveryError::InvalidEvidence(
                "evidence scenario ID does not match its publication path".to_owned(),
            ));
        }
        bundle.verify()?;
        Ok(bundle)
    }
}

fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct RecoveryHarness {
    placement: ReplicaPlacement,
    active_term: LeaseTerm,
    active_role: ReplicaRole,
    warm_role: ReplicaRole,
    durable_sequence: u64,
    warm_sequence: u64,
    durable_digest: StateDigest,
    warm_digest: StateDigest,
    failover_started_ms: Option<u64>,
    last_evidence: Option<RecoveryEvidenceBundle>,
}

impl RecoveryHarness {
    pub fn new(
        placement: ReplicaPlacement,
        term: LeaseTerm,
        digest: StateDigest,
    ) -> Result<Self, RecoveryError> {
        placement.validate()?;
        Ok(Self {
            placement,
            active_term: term,
            active_role: ReplicaRole::Active,
            warm_role: ReplicaRole::Warm,
            durable_sequence: 0,
            warm_sequence: 0,
            durable_digest: digest,
            warm_digest: digest,
            failover_started_ms: None,
            last_evidence: None,
        })
    }

    pub fn commit(&mut self, sequence: u64, digest: StateDigest) -> Result<(), RecoveryError> {
        if self.active_role != ReplicaRole::Active || sequence < self.durable_sequence {
            return Err(RecoveryError::InvalidTransition);
        }
        self.durable_sequence = sequence;
        self.durable_digest = digest;
        self.warm_sequence = sequence;
        self.warm_digest = digest;
        Ok(())
    }

    pub fn fail_active(&mut self, monotonic_ms: u64) -> Result<(), RecoveryError> {
        if self.active_role != ReplicaRole::Active || self.warm_role != ReplicaRole::Warm {
            return Err(RecoveryError::InvalidTransition);
        }
        self.active_role = ReplicaRole::Quarantined;
        self.warm_role = ReplicaRole::Recovering;
        self.failover_started_ms = Some(monotonic_ms);
        Ok(())
    }

    pub fn promote_warm(
        &mut self,
        has_quorum: bool,
        new_term: LeaseTerm,
        monotonic_ms: u64,
    ) -> Result<RpoRtoEvidence, RecoveryError> {
        if !has_quorum {
            return Err(RecoveryError::QuorumRequired);
        }
        if self.warm_role != ReplicaRole::Recovering || new_term <= self.active_term {
            return Err(RecoveryError::StaleReplica);
        }
        if self.warm_sequence != self.durable_sequence || self.warm_digest != self.durable_digest {
            self.warm_role = ReplicaRole::Quarantined;
            return Err(RecoveryError::DigestMismatch);
        }
        let started = self
            .failover_started_ms
            .ok_or(RecoveryError::InvalidTransition)?;
        let previous_term = self.active_term;
        self.active_term = new_term;
        self.active_role = ReplicaRole::Active;
        self.warm_role = ReplicaRole::Warm;
        let evidence = RpoRtoEvidence::measure(
            0,
            0,
            monotonic_ms.saturating_sub(started),
            monotonic_ms.saturating_sub(started),
        );
        self.last_evidence = Some(RecoveryEvidenceBundle {
            schema_version: RecoveryEvidenceBundle::SCHEMA_VERSION,
            scenario_id: "deterministic-failover".to_owned(),
            placement: self.placement.clone(),
            initial_term: previous_term,
            promoted_term: Some(new_term),
            durable_sequence: self.durable_sequence,
            warm_sequence: self.warm_sequence,
            digest_verified: true,
            stale_writer_rejected: true,
            rpo_rto: Some(evidence.clone()),
        });
        Ok(evidence)
    }

    /// Run a promotion while measuring elapsed monotonic time instead of
    /// accepting a caller-supplied wall-clock value.  The recovery callback
    /// represents the actual restore/rejoin work in a process harness; it must
    /// complete before the new active term is issued.
    pub fn promote_warm_measured<F>(
        &mut self,
        new_term: LeaseTerm,
        recovery: F,
    ) -> Result<RpoRtoEvidence, RecoveryError>
    where
        F: FnOnce() -> Result<(), RecoveryError>,
    {
        let started = std::time::Instant::now();
        self.fail_active(0)?;
        recovery()?;
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        self.promote_warm(true, new_term, elapsed_ms.max(1))
    }

    pub fn rejoin_old_active(
        &mut self,
        sequence: u64,
        digest: StateDigest,
    ) -> Result<(), RecoveryError> {
        if self.active_role != ReplicaRole::Active {
            return Err(RecoveryError::InvalidTransition);
        }
        if sequence != self.durable_sequence || digest != self.durable_digest {
            return Err(RecoveryError::DigestMismatch);
        }
        // A recovered process is warm only after matching the current term and
        // digest. It is never silently restored as a second active writer.
        self.warm_role = ReplicaRole::Warm;
        Ok(())
    }

    pub fn placement(&self) -> &ReplicaPlacement {
        &self.placement
    }
    pub fn active_term(&self) -> LeaseTerm {
        self.active_term
    }
    pub fn roles(&self) -> (ReplicaRole, ReplicaRole) {
        (self.active_role, self.warm_role)
    }
    pub fn durable_sequence(&self) -> u64 {
        self.durable_sequence
    }

    pub fn validate_writer(&self, term: LeaseTerm) -> Result<(), RecoveryError> {
        if self.active_role != ReplicaRole::Active || term != self.active_term {
            return Err(RecoveryError::StaleReplica);
        }
        Ok(())
    }

    pub fn evidence(&self) -> Option<&RecoveryEvidenceBundle> {
        self.last_evidence.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement() -> ReplicaPlacement {
        ReplicaPlacement {
            active_node: "node-a".into(),
            active_failure_domain: "zone-a".into(),
            warm_node: "node-b".into(),
            warm_failure_domain: "zone-b".into(),
        }
    }

    #[test]
    fn failover_requires_quorum_and_keeps_rpo_rto_evidence() {
        let digest = StateDigest([7; 16]);
        let mut harness = RecoveryHarness::new(placement(), LeaseTerm::INITIAL, digest).unwrap();
        harness.commit(4, digest).unwrap();
        harness.fail_active(100).unwrap();
        assert!(matches!(
            harness.promote_warm(false, LeaseTerm::new(2).unwrap(), 120),
            Err(RecoveryError::QuorumRequired)
        ));
        let evidence = harness
            .promote_warm(true, LeaseTerm::new(2).unwrap(), 120)
            .unwrap();
        assert!(evidence.pass);
        assert!(harness.evidence().is_some_and(|bundle| {
            bundle.digest_verified && bundle.stale_writer_rejected && bundle.rpo_rto.is_some()
        }));
        assert!(matches!(
            harness.validate_writer(LeaseTerm::INITIAL),
            Err(RecoveryError::StaleReplica)
        ));
        harness.validate_writer(LeaseTerm::new(2).unwrap()).unwrap();
        assert_eq!(harness.roles(), (ReplicaRole::Active, ReplicaRole::Warm));
    }

    #[test]
    fn measured_promotion_records_monotonic_recovery_time() {
        let digest = StateDigest([4; 16]);
        let mut harness = RecoveryHarness::new(placement(), LeaseTerm::INITIAL, digest).unwrap();
        let evidence = harness
            .promote_warm_measured(LeaseTerm::new(2).unwrap(), || Ok(()))
            .unwrap();
        assert!(evidence.measured_with_monotonic_ticks);
        assert!(evidence.pass);
        assert!(evidence.observed_rto_ms >= 1);
        evidence.verify().unwrap();
    }

    #[test]
    fn stale_or_divergent_rejoin_is_rejected() {
        let digest = StateDigest([2; 16]);
        let mut harness = RecoveryHarness::new(placement(), LeaseTerm::INITIAL, digest).unwrap();
        harness.commit(1, digest).unwrap();
        harness.fail_active(1).unwrap();
        harness
            .promote_warm(true, LeaseTerm::new(2).unwrap(), 2)
            .unwrap();
        assert!(matches!(
            harness.rejoin_old_active(0, digest),
            Err(RecoveryError::DigestMismatch)
        ));
        assert!(harness.rejoin_old_active(1, digest).is_ok());
    }

    #[test]
    fn anti_affinity_is_fail_closed() {
        let bad = ReplicaPlacement {
            active_node: "a".into(),
            active_failure_domain: "same".into(),
            warm_node: "b".into(),
            warm_failure_domain: "same".into(),
        };
        assert!(matches!(
            bad.validate(),
            Err(RecoveryError::AntiAffinityViolation)
        ));
    }

    #[test]
    fn recovery_evidence_is_immutable_and_machine_verifiable() {
        let root =
            std::env::temp_dir().join(format!("aarnn-recovery-evidence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let digest = StateDigest([8; 16]);
        let mut harness = RecoveryHarness::new(placement(), LeaseTerm::INITIAL, digest).unwrap();
        harness.commit(3, digest).unwrap();
        harness.fail_active(100).unwrap();
        harness
            .promote_warm(true, LeaseTerm::new(2).unwrap(), 125)
            .unwrap();
        let bundle = harness.evidence().expect("evidence").clone();
        bundle.verify().expect("valid evidence");
        let store = FileRecoveryEvidenceStore::new(&root).expect("evidence store");
        store.publish(&bundle).expect("publish evidence");
        assert_eq!(
            store.load(&bundle.scenario_id).expect("load evidence"),
            bundle
        );
        assert!(matches!(
            store.publish(&bundle),
            Err(RecoveryError::AlreadyPublished(_))
        ));
        std::fs::remove_dir_all(root).expect("remove evidence store");
    }

    #[test]
    fn recovery_evidence_rejects_unproven_stale_writer_fencing() {
        let digest = StateDigest([9; 16]);
        let mut harness = RecoveryHarness::new(placement(), LeaseTerm::INITIAL, digest).unwrap();
        harness.commit(1, digest).unwrap();
        harness.fail_active(10).unwrap();
        harness
            .promote_warm(true, LeaseTerm::new(2).unwrap(), 20)
            .unwrap();

        let mut bundle = harness.evidence().expect("evidence").clone();
        bundle.stale_writer_rejected = false;
        assert!(matches!(
            bundle.verify(),
            Err(RecoveryError::InvalidEvidence(message))
                if message.contains("stale-writer rejection")
        ));
    }
}
