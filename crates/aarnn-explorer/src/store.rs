// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Project directory: immutable content-addressed objects plus a small
//! crash-safe index (brief Sections 6 and 16).
//!
//! Layout:
//! ```text
//! <project>/project.json          schema, id, name, resource policy
//! <project>/index.json            named records -> object hashes (rebuildable)
//! <project>/objects/sha256/ab/<rest>   immutable published objects
//! <project>/tmp/                  private staging (same filesystem)
//! <project>/jobs.jsonl            job transition journal (append-only)
//! <project>/runs/<job>/           worker scratch, outputs
//! ```
//! Objects are written to `tmp/`, fsynced, verified and renamed into place, so
//! a crash leaves either the old state or the complete new object. An object
//! is never modified after publication (`INV-012`).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ExplorerError, Result, invalid};

pub const PROJECT_SCHEMA: u32 = 1;
/// Upper bound on a single object read back into memory (decompression-bomb
/// and oversized-input guard). Traces are segmented well below this.
pub const MAX_OBJECT_BYTES: u64 = 1 << 30;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// Validate a record name: no path separators, traversal or control bytes.
pub fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok {
        Ok(())
    } else {
        invalid(format!(
            "name {name:?}: use 1-128 of [A-Za-z0-9._-], not starting with '.'"
        ))
    }
}

/// Write `bytes` to `dest` atomically through `tmp_dir` (same filesystem).
pub fn atomic_write(tmp_dir: &Path, dest: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(tmp_dir)?;
    let tmp = tmp_dir.join(format!(".{}-{}.tmp", std::process::id(), fastrand::u64(..)));
    {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&tmp, dest)?;
    if let Some(parent) = dest.parent() {
        if let Ok(d) = File::open(parent) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub project_id: String,
    pub name: String,
    pub created_unix_s: u64,
    pub tool_version: String,
}

/// Named records. Each kind maps a record name to an object hash. Aliases
/// (`selected`) resolve to a recorded immutable candidate hash.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectIndex {
    pub schema_version: u32,
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub traces: BTreeMap<String, String>,
    #[serde(default)]
    pub analyses: BTreeMap<String, String>,
    #[serde(default)]
    pub results: BTreeMap<String, String>,
    #[serde(default)]
    pub candidates: BTreeMap<String, String>,
    #[serde(default)]
    pub reports: BTreeMap<String, String>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// Project defaults (e.g. `source`, `suite`), set by import/demo.
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Source,
    Trace,
    Analysis,
    Result,
    Candidate,
    Report,
}

impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Source => "source",
            RecordKind::Trace => "trace",
            RecordKind::Analysis => "analysis",
            RecordKind::Result => "result",
            RecordKind::Candidate => "candidate",
            RecordKind::Report => "report",
        }
    }
}

pub struct Project {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
}

impl Project {
    /// Create a new project. Refuses to overwrite an existing one.
    pub fn init(root: &Path, name: &str) -> Result<Project> {
        check_name(name)?;
        if root.join("project.json").exists() {
            return Err(ExplorerError::Conflict(format!(
                "{} already holds a project; choose another directory",
                root.display()
            )));
        }
        if root.exists() && fs::read_dir(root)?.next().is_some() {
            return Err(ExplorerError::Conflict(format!(
                "{} exists and is not empty",
                root.display()
            )));
        }
        fs::create_dir_all(root.join("objects/sha256"))?;
        fs::create_dir_all(root.join("tmp"))?;
        fs::create_dir_all(root.join("runs"))?;
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let manifest = ProjectManifest {
            schema_version: PROJECT_SCHEMA,
            project_id: format!(
                "prj-{}",
                &sha256_hex(format!("{name}:{created}:{}", fastrand::u64(..)).as_bytes())[..16]
            ),
            name: name.to_string(),
            created_unix_s: created,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        let p = Project {
            root: root.to_path_buf(),
            manifest,
        };
        atomic_write(
            &p.tmp_dir(),
            &root.join("project.json"),
            serde_json::to_string_pretty(&p.manifest)?.as_bytes(),
        )?;
        p.write_index(&ProjectIndex {
            schema_version: PROJECT_SCHEMA,
            ..Default::default()
        })?;
        Ok(p)
    }

    pub fn open(root: &Path) -> Result<Project> {
        let raw = fs::read_to_string(root.join("project.json")).map_err(|e| {
            ExplorerError::InvalidInput(format!(
                "{}: not an explorer project ({e}); run `aarnn-explorer project init`",
                root.display()
            ))
        })?;
        let manifest: ProjectManifest = serde_json::from_str(&raw)?;
        if manifest.schema_version != PROJECT_SCHEMA {
            return Err(ExplorerError::Unsupported(format!(
                "project schema {} (this build reads {PROJECT_SCHEMA})",
                manifest.schema_version
            )));
        }
        Ok(Project {
            root: root.to_path_buf(),
            manifest,
        })
    }

    /// Open, or initialise when the directory is absent or empty.
    pub fn open_or_init(root: &Path, name: &str) -> Result<Project> {
        if root.join("project.json").exists() {
            Project::open(root)
        } else {
            Project::init(root, name)
        }
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    fn object_path(&self, hash: &str) -> Result<PathBuf> {
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return invalid(format!("object hash {hash:?} is not sha256 hex"));
        }
        Ok(self
            .root
            .join("objects/sha256")
            .join(&hash[..2])
            .join(&hash[2..]))
    }

    /// Publish bytes as an immutable object; returns its sha256. Publishing
    /// identical content twice is idempotent.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let hash = sha256_hex(bytes);
        let path = self.object_path(&hash)?;
        if path.exists() {
            if sha256_file(&path)? != hash {
                return Err(ExplorerError::Internal(format!(
                    "object {hash} is corrupt on disk"
                )));
            }
            return Ok(hash);
        }
        atomic_write(&self.tmp_dir(), &path, bytes)?;
        let check = sha256_file(&path)?;
        if check != hash {
            return Err(ExplorerError::Io(format!(
                "object {hash} failed read-back verification"
            )));
        }
        Ok(hash)
    }

    pub fn put_json<T: Serialize>(&self, value: &T) -> Result<String> {
        self.put(serde_json::to_string_pretty(value)?.as_bytes())
    }

    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.object_path(hash)?;
        let meta = fs::metadata(&path)
            .map_err(|e| ExplorerError::InvalidInput(format!("object {hash}: {e}")))?;
        if meta.len() > MAX_OBJECT_BYTES {
            return Err(ExplorerError::Budget(format!(
                "object {hash} is {} bytes (limit {MAX_OBJECT_BYTES})",
                meta.len()
            )));
        }
        let bytes = fs::read(&path)?;
        if sha256_hex(&bytes) != hash {
            return Err(ExplorerError::Internal(format!(
                "object {hash} checksum mismatch"
            )));
        }
        Ok(bytes)
    }

    pub fn get_json<T: for<'de> Deserialize<'de>>(&self, hash: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.get(hash)?)?)
    }

    pub fn object_file(&self, hash: &str) -> Result<PathBuf> {
        let p = self.object_path(hash)?;
        if !p.exists() {
            return invalid(format!("object {hash} not found"));
        }
        Ok(p)
    }

    pub fn read_index(&self) -> Result<ProjectIndex> {
        let raw = fs::read_to_string(self.root.join("index.json"))?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn write_index(&self, idx: &ProjectIndex) -> Result<()> {
        atomic_write(
            &self.tmp_dir(),
            &self.root.join("index.json"),
            serde_json::to_string_pretty(idx)?.as_bytes(),
        )
    }

    /// Serialise index updates across processes with an exclusive lock file.
    fn with_index<R>(&self, f: impl FnOnce(&mut ProjectIndex) -> Result<R>) -> Result<R> {
        let lock = self.root.join("index.lock");
        let mut tries = 0u32;
        let guard = loop {
            match OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(g) => break g,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    tries += 1;
                    // A stale lock older than 60 s (crashed writer) is removed.
                    if let Ok(meta) = fs::metadata(&lock) {
                        if let Ok(age) = meta.modified().map(|m| m.elapsed().unwrap_or_default()) {
                            if age.as_secs() > 60 {
                                let _ = fs::remove_file(&lock);
                                continue;
                            }
                        }
                    }
                    if tries > 2000 {
                        return Err(ExplorerError::Conflict(
                            "project index is locked by another writer".into(),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => return Err(e.into()),
            }
        };
        drop(guard);
        let out = (|| {
            let mut idx = self.read_index()?;
            let r = f(&mut idx)?;
            self.write_index(&idx)?;
            Ok(r)
        })();
        let _ = fs::remove_file(&lock);
        out
    }

    fn map_mut(idx: &mut ProjectIndex, kind: RecordKind) -> &mut BTreeMap<String, String> {
        match kind {
            RecordKind::Source => &mut idx.sources,
            RecordKind::Trace => &mut idx.traces,
            RecordKind::Analysis => &mut idx.analyses,
            RecordKind::Result => &mut idx.results,
            RecordKind::Candidate => &mut idx.candidates,
            RecordKind::Report => &mut idx.reports,
        }
    }

    /// Bind a record name to an object. A name is immutable once bound to a
    /// different hash (a new semantic input needs a new name).
    pub fn bind(&self, kind: RecordKind, name: &str, hash: &str) -> Result<()> {
        check_name(name)?;
        self.object_file(hash)?;
        self.with_index(|idx| {
            let map = Self::map_mut(idx, kind);
            match map.get(name) {
                Some(h) if h != hash => Err(ExplorerError::Conflict(format!(
                    "{} {name:?} is already bound to {h}; use a new name",
                    kind.as_str()
                ))),
                _ => {
                    map.insert(name.to_string(), hash.to_string());
                    Ok(())
                }
            }
        })
    }

    pub fn lookup(&self, kind: RecordKind, name: &str) -> Result<String> {
        let mut idx = self.read_index()?;
        if kind == RecordKind::Candidate {
            if let Some(h) = idx.aliases.get(name) {
                return Ok(h.clone());
            }
            // A full object hash of a bound candidate also resolves.
            if name.len() == 64 && idx.candidates.values().any(|h| h == name) {
                return Ok(name.to_string());
            }
        }
        Self::map_mut(&mut idx, kind)
            .get(name)
            .cloned()
            .ok_or_else(|| {
                ExplorerError::InvalidInput(format!("no {} named {name:?}", kind.as_str()))
            })
    }

    /// Point an alias (e.g. `selected`) at a candidate hash. Aliases may move;
    /// every command prints the resolved hash.
    pub fn set_alias(&self, alias: &str, hash: &str) -> Result<()> {
        check_name(alias)?;
        self.object_file(hash)?;
        self.with_index(|idx| {
            idx.aliases.insert(alias.to_string(), hash.to_string());
            Ok(())
        })
    }

    pub fn set_default(&self, key: &str, value: &str) -> Result<()> {
        check_name(key)?;
        self.with_index(|idx| {
            idx.defaults.insert(key.to_string(), value.to_string());
            Ok(())
        })
    }

    pub fn default_value(&self, key: &str) -> Result<Option<String>> {
        Ok(self.read_index()?.defaults.get(key).cloned())
    }

    /// Append one line to the job journal (JSON object per line).
    pub fn journal(&self, line: &serde_json::Value) -> Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("jobs.jsonl"))?;
        // One write per line: concurrent appenders (worker threads) must not
        // interleave partial lines.
        f.write_all(format!("{line}\n").as_bytes())?;
        f.sync_data()?;
        Ok(())
    }

    pub fn journal_lines(&self) -> Result<Vec<serde_json::Value>> {
        let path = self.root.join("jobs.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        Ok(raw
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }
}
