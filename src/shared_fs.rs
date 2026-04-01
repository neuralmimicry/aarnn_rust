use anyhow::{anyhow, Context};
use fs2::FileExt;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir '{}'", parent.display()))?;
    }
    let tmp_path = path.with_extension(format!("tmp-{}", fastrand::u32(..)));
    std::fs::write(&tmp_path, bytes)
        .with_context(|| format!("failed to write temp file '{}'", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename temp file '{}' to '{}'",
            tmp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

pub fn read_if_exists(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    Ok(Some(data))
}

pub fn file_modified_ms(path: &Path) -> anyhow::Result<Option<u64>> {
    if !path.exists() {
        return Ok(None);
    }
    let modified = std::fs::metadata(path)
        .with_context(|| format!("failed to stat '{}'", path.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for '{}'", path.display()))?;
    Ok(Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    ))
}

pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("failed to encode JSON for '{}'", path.display()))?;
    atomic_write(path, &bytes)
}

pub fn read_json_if_exists<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    let Some(raw) = read_if_exists(path)? else {
        return Ok(None);
    };
    let value = serde_json::from_str::<T>(&raw)
        .with_context(|| format!("failed to parse JSON from '{}'", path.display()))?;
    Ok(Some(value))
}

pub struct FileLease {
    file: File,
}

impl Drop for FileLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn try_acquire_lease(path: &Path) -> anyhow::Result<Option<FileLease>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create lease dir '{}'", parent.display()))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open lease '{}'", path.display()))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(FileLease { file })),
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(err) => Err(anyhow!(
            "failed to acquire lease '{}': {}",
            path.display(),
            err
        )),
    }
}

pub async fn acquire_lease_with_timeout(
    path: PathBuf,
    timeout: Duration,
    retry_interval: Duration,
) -> anyhow::Result<FileLease> {
    let started = Instant::now();
    loop {
        let lease_path = path.clone();
        let lease = tokio::task::spawn_blocking(move || try_acquire_lease(&lease_path))
            .await
            .context("lease acquisition task failed")??;
        if let Some(lease) = lease {
            return Ok(lease);
        }
        if started.elapsed() >= timeout {
            return Err(anyhow!("timed out waiting for lease '{}'", path.display()));
        }
        tokio::time::sleep(retry_interval).await;
    }
}
