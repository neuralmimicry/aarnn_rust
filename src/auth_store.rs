use crate::shared_fs::{read_json_if_exists, write_json_pretty};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionIdentityRecord {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub active_team: Option<Value>,
    #[serde(default)]
    pub team_count: Option<i64>,
    #[serde(default)]
    pub pending_invitation_count: Option<i64>,
    #[serde(default)]
    pub is_admin: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub username: String,
    pub expires_at: u64,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub identity: SessionIdentityRecord,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OidcPendingRecord {
    pub nonce: String,
    pub pkce_verifier: String,
}

#[derive(Clone, Debug)]
pub struct FileSessionStore {
    root: PathBuf,
}

impl FileSessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", hex::encode(key.as_bytes())))
    }

    pub async fn put(&self, key: &str, record: &SessionRecord) -> anyhow::Result<()> {
        let path = self.path_for(key);
        let record = record.clone();
        tokio::task::spawn_blocking(move || write_json_pretty(&path, &record))
            .await
            .context("session store write task failed")??;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> anyhow::Result<Option<SessionRecord>> {
        let path = self.path_for(key);
        tokio::task::spawn_blocking(move || read_json_if_exists(&path))
            .await
            .context("session store read task failed")?
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let path = self.path_for(key);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            if path.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("failed to delete '{}'", path.display()))?;
            }
            Ok(())
        })
        .await
        .context("session store delete task failed")??;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct FileOidcPendingStore {
    root: PathBuf,
}

impl FileOidcPendingStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", hex::encode(key.as_bytes())))
    }

    pub async fn put(&self, key: &str, record: &OidcPendingRecord) -> anyhow::Result<()> {
        let path = self.path_for(key);
        let record = record.clone();
        tokio::task::spawn_blocking(move || write_json_pretty(&path, &record))
            .await
            .context("oidc pending store write task failed")??;
        Ok(())
    }

    pub async fn take(&self, key: &str) -> anyhow::Result<Option<OidcPendingRecord>> {
        let path = self.path_for(key);
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<OidcPendingRecord>> {
            let record = read_json_if_exists(&path)?;
            if path.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("failed to delete '{}'", path.display()))?;
            }
            Ok(record)
        })
        .await
        .context("oidc pending store take task failed")?
    }
}
