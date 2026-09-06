//! Rehearsal/canary/rollback evidence for the staged runtime migration.
//!
//! Legacy paths remain available until a successful, durable rehearsal and
//! rollback window are recorded. This module makes that decision explicit
//! rather than allowing a feature flag to silently remove the recovery path.

use crate::deterministic::StateDigest;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStage {
    Prepared,
    CatchingUp,
    Canary,
    Promoted,
    RolledBack,
    Cleaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEvidence {
    pub schema_version: u32,
    pub migration_id: String,
    pub source_digest: StateDigest,
    pub target_digest: Option<StateDigest>,
    pub rollback_digest: Option<StateDigest>,
    pub stage: MigrationStage,
    pub legacy_path_retained: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MigrationError {
    #[error("migration identifier is empty")]
    EmptyId,
    #[error("migration stage transition is invalid")]
    InvalidTransition,
    #[error("target digest does not match the source at the canary boundary")]
    DigestMismatch,
    #[error("legacy path cannot be removed before rollback evidence")]
    RollbackEvidenceRequired,
    #[error("migration evidence I/O failed: {0}")]
    Io(String),
    #[error("migration evidence encoding failed: {0}")]
    Encoding(String),
    #[error("migration evidence is invalid: {0}")]
    InvalidEvidence(String),
    #[error("migration evidence for {0} is already published")]
    AlreadyPublished(String),
}

#[derive(Debug, Clone)]
pub struct MigrationRehearsal {
    evidence: MigrationEvidence,
}

impl MigrationRehearsal {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn prepare(
        id: impl Into<String>,
        source_digest: StateDigest,
    ) -> Result<Self, MigrationError> {
        let migration_id = id.into();
        if migration_id.trim().is_empty() {
            return Err(MigrationError::EmptyId);
        }
        Ok(Self {
            evidence: MigrationEvidence {
                schema_version: Self::SCHEMA_VERSION,
                migration_id,
                source_digest,
                target_digest: None,
                rollback_digest: None,
                stage: MigrationStage::Prepared,
                legacy_path_retained: true,
            },
        })
    }

    pub fn begin_catchup(&mut self) -> Result<(), MigrationError> {
        if self.evidence.stage != MigrationStage::Prepared {
            return Err(MigrationError::InvalidTransition);
        }
        self.evidence.stage = MigrationStage::CatchingUp;
        Ok(())
    }

    pub fn begin_canary(&mut self, target_digest: StateDigest) -> Result<(), MigrationError> {
        if self.evidence.stage != MigrationStage::CatchingUp {
            return Err(MigrationError::InvalidTransition);
        }
        if target_digest != self.evidence.source_digest {
            return Err(MigrationError::DigestMismatch);
        }
        self.evidence.target_digest = Some(target_digest);
        self.evidence.stage = MigrationStage::Canary;
        Ok(())
    }

    pub fn promote(&mut self) -> Result<(), MigrationError> {
        if self.evidence.stage != MigrationStage::Canary || self.evidence.target_digest.is_none() {
            return Err(MigrationError::InvalidTransition);
        }
        self.evidence.stage = MigrationStage::Promoted;
        Ok(())
    }

    pub fn rollback(&mut self, rollback_digest: StateDigest) -> Result<(), MigrationError> {
        if !matches!(
            self.evidence.stage,
            MigrationStage::Canary | MigrationStage::Promoted
        ) {
            return Err(MigrationError::InvalidTransition);
        }
        if rollback_digest != self.evidence.source_digest {
            return Err(MigrationError::DigestMismatch);
        }
        self.evidence.rollback_digest = Some(rollback_digest);
        self.evidence.stage = MigrationStage::RolledBack;
        Ok(())
    }

    /// Record a successful rollback rehearsal while the promoted canary is
    /// still the selected path. This is the evidence required before cleanup;
    /// an actual rollback leaves the stage `RolledBack` and cannot be cleaned.
    pub fn record_rollback_evidence(
        &mut self,
        rollback_digest: StateDigest,
    ) -> Result<(), MigrationError> {
        if self.evidence.stage != MigrationStage::Promoted
            || rollback_digest != self.evidence.source_digest
        {
            return Err(MigrationError::DigestMismatch);
        }
        self.evidence.rollback_digest = Some(rollback_digest);
        Ok(())
    }

    pub fn cleanup_legacy(&mut self) -> Result<(), MigrationError> {
        if self.evidence.stage != MigrationStage::Promoted
            || self.evidence.rollback_digest.is_none()
        {
            return Err(MigrationError::RollbackEvidenceRequired);
        }
        self.evidence.legacy_path_retained = false;
        self.evidence.stage = MigrationStage::Cleaned;
        Ok(())
    }

    pub fn evidence(&self) -> &MigrationEvidence {
        &self.evidence
    }
}

impl MigrationEvidence {
    pub fn verify(&self) -> Result<(), MigrationError> {
        if self.schema_version != MigrationRehearsal::SCHEMA_VERSION
            || self.migration_id.trim().is_empty()
        {
            return Err(MigrationError::InvalidEvidence(
                "schema version and migration ID are required".to_owned(),
            ));
        }
        if matches!(
            self.stage,
            MigrationStage::Canary | MigrationStage::Promoted | MigrationStage::Cleaned
        ) && self.target_digest != Some(self.source_digest)
        {
            return Err(MigrationError::InvalidEvidence(
                "canary or promoted evidence lacks a matching target digest".to_owned(),
            ));
        }
        if self.stage == MigrationStage::Cleaned
            && (self.legacy_path_retained || self.rollback_digest != Some(self.source_digest))
        {
            return Err(MigrationError::InvalidEvidence(
                "cleaned migration lacks rollback evidence".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Immutable publication for migration/canary/rollback evidence.  Cleanup of
/// legacy paths is an operational decision and is never inferred from an
/// in-memory rehearsal object alone.
#[derive(Debug, Clone)]
pub struct FileMigrationEvidenceStore {
    root: PathBuf,
}

impl FileMigrationEvidenceStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, MigrationError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|error| MigrationError::Io(error.to_string()))?;
        Ok(Self { root })
    }

    pub fn publish(&self, evidence: &MigrationEvidence) -> Result<PathBuf, MigrationError> {
        evidence.verify()?;
        let path = self
            .root
            .join(format!("{}.json", safe_name(&evidence.migration_id)));
        let bytes = serde_json::to_vec(evidence)
            .map_err(|error| MigrationError::Encoding(error.to_string()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| MigrationError::Io(error.to_string()))?;
        if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&temporary);
            return Err(MigrationError::Io(error.to_string()));
        }
        match fs::hard_link(&temporary, &path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temporary);
                return Err(MigrationError::AlreadyPublished(
                    evidence.migration_id.clone(),
                ));
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(MigrationError::Io(error.to_string()));
            }
        }
        let _ = fs::remove_file(&temporary);
        if let Some(parent) = path.parent() {
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| MigrationError::Io(error.to_string()))?;
        }
        Ok(path)
    }

    pub fn load(&self, migration_id: &str) -> Result<MigrationEvidence, MigrationError> {
        let path = self.root.join(format!("{}.json", safe_name(migration_id)));
        let bytes = fs::read(&path).map_err(|error| MigrationError::Io(error.to_string()))?;
        let evidence: MigrationEvidence = serde_json::from_slice(&bytes)
            .map_err(|error| MigrationError::Encoding(error.to_string()))?;
        if evidence.migration_id != migration_id {
            return Err(MigrationError::InvalidEvidence(
                "migration ID does not match publication path".to_owned(),
            ));
        }
        evidence.verify()?;
        Ok(evidence)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canary_requires_digest_and_cleanup_requires_rollback_evidence() {
        let digest = StateDigest([4; 16]);
        let mut rehearsal = MigrationRehearsal::prepare("migration-1", digest).unwrap();
        rehearsal.begin_catchup().unwrap();
        assert!(matches!(
            rehearsal.begin_canary(StateDigest([5; 16])),
            Err(MigrationError::DigestMismatch)
        ));
        rehearsal.begin_canary(digest).unwrap();
        rehearsal.promote().unwrap();
        assert!(matches!(
            rehearsal.cleanup_legacy(),
            Err(MigrationError::RollbackEvidenceRequired)
        ));
        rehearsal.rollback(digest).unwrap();
        assert!(rehearsal.begin_catchup().is_err());
        // A rolled-back rehearsal is retained as evidence and cannot claim
        // that the new path was promoted.
        assert_eq!(rehearsal.evidence().stage, MigrationStage::RolledBack);

        let mut promoted = MigrationRehearsal::prepare("migration-2", digest).unwrap();
        promoted.begin_catchup().unwrap();
        promoted.begin_canary(digest).unwrap();
        promoted.promote().unwrap();
        promoted.record_rollback_evidence(digest).unwrap();
        promoted.cleanup_legacy().unwrap();
        assert!(!promoted.evidence().legacy_path_retained);
    }

    #[test]
    fn migration_evidence_is_immutable_and_reopenable() {
        let digest = StateDigest([7; 16]);
        let root =
            std::env::temp_dir().join(format!("aarnn-migration-evidence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut rehearsal = MigrationRehearsal::prepare("migration-persisted", digest).unwrap();
        rehearsal.begin_catchup().unwrap();
        rehearsal.begin_canary(digest).unwrap();
        rehearsal.promote().unwrap();
        rehearsal.record_rollback_evidence(digest).unwrap();
        rehearsal.cleanup_legacy().unwrap();
        let store = FileMigrationEvidenceStore::new(&root).unwrap();
        store.publish(rehearsal.evidence()).unwrap();
        assert!(matches!(
            store.publish(rehearsal.evidence()),
            Err(MigrationError::AlreadyPublished(_))
        ));
        assert_eq!(
            store.load("migration-persisted").unwrap(),
            *rehearsal.evidence()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
