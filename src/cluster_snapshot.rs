//! Deterministic assembly and validation of a cluster-wide shard snapshot.
//!
//! This is a read-only reference contract over the current distributed runner
//! seam.  It deliberately requires a complete, common runner frontier before
//! returning a cluster cut.  It is not a substitute for durable shard-owned
//! state, quorum fencing, or a distributed consistent-cut protocol; callers
//! must use the later durability/control-plane gates for those guarantees.

use crate::consistent_cut::{ChannelMarker, ConsistentCut, ParticipantReport};
use crate::deterministic::{LogicalTag, StateDigest, StateDigestBuilder};
use crate::runner::decode_snapshot_with_profile_backfill;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CLUSTER_SNAPSHOT_SCHEMA_VERSION: u32 = 2;
const LEGACY_CLUSTER_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const MAX_SHARD_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardSnapshotInput {
    pub node_id: String,
    pub layers: Vec<u32>,
    pub snapshot_json: String,
    #[serde(default)]
    pub channel_state_json: String,
    /// The complete sealed shard boundary, when the durable shard profile is
    /// active.  The legacy runner projection remains accepted only for
    /// compatibility/reference snapshots and is never mistaken for a full
    /// recovery point.
    #[serde(default)]
    pub authoritative_state_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterShardSnapshot {
    pub node_id: String,
    pub layers: Vec<u32>,
    pub snapshot_json: String,
    pub channel_state_json: String,
    pub step: u64,
    pub sim_time_ms_bits: u64,
    pub state_digest: StateDigest,
    /// Digest of the canonical in-transit/channel state captured with the
    /// runner snapshot.  It is part of the cluster cut, not presentation
    /// metadata, so the aggregate digest must cover it.
    pub channel_state_digest: StateDigest,
    /// Canonical JSON encoding of [`crate::authoritative_shard::ShardState`].
    /// Empty means this is an explicitly marked compatibility snapshot.
    #[serde(default)]
    pub authoritative_state_json: String,
    /// Digest of the complete sealed shard boundary, independent of the
    /// runner projection digest above.
    #[serde(default)]
    pub authoritative_state_digest: Option<StateDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterGlobalSnapshot {
    pub schema_version: u32,
    pub network_id: String,
    pub cut_tag: LogicalTag,
    pub cluster_digest: StateDigest,
    pub shards: Vec<ClusterShardSnapshot>,
    /// Evidence from the asynchronous GVT protocol. Compatibility snapshots
    /// produced by [`assemble`] leave this absent.
    #[serde(default)]
    pub consistent_cut: Option<ConsistentCut>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCutEvidence {
    pub participant: ParticipantReport,
    pub marker: ChannelMarker,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClusterSnapshotError {
    #[error("cluster snapshot has no assigned shards")]
    NoAssignedShards,
    #[error("cluster snapshot is missing shard(s): {0}")]
    MissingShards(String),
    #[error("cluster snapshot contains an unexpected shard '{0}'")]
    UnexpectedShard(String),
    #[error("cluster snapshot contains duplicate shard '{0}'")]
    DuplicateShard(String),
    #[error("shard '{node_id}' has an empty assignment")]
    EmptyAssignment { node_id: String },
    #[error("shard '{node_id}' repeats layer {layer}")]
    DuplicateLayer { node_id: String, layer: u32 },
    #[error("shard '{node_id}' snapshot is too large ({bytes} bytes)")]
    SnapshotTooLarge { node_id: String, bytes: usize },
    #[error("shard '{node_id}' snapshot is invalid: {reason}")]
    InvalidSnapshot { node_id: String, reason: String },
    #[error("shard '{node_id}' has frontier {actual}, expected {expected}")]
    MixedFrontier {
        node_id: String,
        actual: u64,
        expected: u64,
    },
    #[error("shard '{node_id}' has a mismatched simulation time")]
    MixedSimulationTime { node_id: String },
    #[error("shard '{node_id}' has a mismatched layer range")]
    MismatchedLayerRange { node_id: String },
    #[error("shard '{node_id}' has a mismatched network shape")]
    MismatchedNetworkShape { node_id: String },
    #[error("cluster snapshot schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("cluster snapshot digest verification failed")]
    DigestMismatch,
    #[error("cluster snapshot is invalid: {0}")]
    InvalidGlobalSnapshot(String),
    #[error("cluster snapshot I/O failed: {0}")]
    Io(String),
    #[error("cluster snapshot encoding failed: {0}")]
    Encoding(String),
    #[error("cluster snapshot with digest {0} is not available")]
    MissingSnapshot(StateDigest),
    #[error("cluster snapshot with digest {0} is already published")]
    AlreadyPublished(StateDigest),
    #[error("live consistent-cut evidence is invalid: {0}")]
    InvalidConsistentCut(String),
}

fn digest_snapshot(
    snapshot_json: &str,
    node_id: &str,
) -> Result<(crate::runner::Snapshot, StateDigest), ClusterSnapshotError> {
    if snapshot_json.len() > MAX_SHARD_SNAPSHOT_BYTES {
        return Err(ClusterSnapshotError::SnapshotTooLarge {
            node_id: node_id.to_owned(),
            bytes: snapshot_json.len(),
        });
    }
    let snapshot = decode_snapshot_with_profile_backfill(snapshot_json).map_err(|error| {
        ClusterSnapshotError::InvalidSnapshot {
            node_id: node_id.to_owned(),
            reason: error.to_string(),
        }
    })?;
    let canonical =
        serde_json::to_vec(&snapshot).map_err(|error| ClusterSnapshotError::InvalidSnapshot {
            node_id: node_id.to_owned(),
            reason: error.to_string(),
        })?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("runner-snapshot", canonical);
    Ok((snapshot, digest.finish()))
}

fn digest_channel_state(
    channel_state_json: &str,
    node_id: &str,
) -> Result<StateDigest, ClusterSnapshotError> {
    let canonical = if channel_state_json.trim().is_empty() {
        Vec::new()
    } else {
        let value =
            serde_json::from_str::<serde_json::Value>(channel_state_json).map_err(|error| {
                ClusterSnapshotError::InvalidSnapshot {
                    node_id: node_id.to_owned(),
                    reason: format!("invalid channel state: {error}"),
                }
            })?;
        serde_json::to_vec(&value).map_err(|error| ClusterSnapshotError::InvalidSnapshot {
            node_id: node_id.to_owned(),
            reason: format!("channel state encoding failed: {error}"),
        })?
    };
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("channel-state", canonical);
    Ok(digest.finish())
}

fn digest_authoritative_state(
    state_json: &str,
    node_id: &str,
) -> Result<Option<StateDigest>, ClusterSnapshotError> {
    if state_json.trim().is_empty() {
        return Ok(None);
    }
    let state: crate::authoritative_shard::ShardState =
        serde_json::from_str(state_json).map_err(|error| {
            ClusterSnapshotError::InvalidSnapshot {
                node_id: node_id.to_owned(),
                reason: format!("invalid authoritative shard state: {error}"),
            }
        })?;
    state
        .verify()
        .map_err(|error| ClusterSnapshotError::InvalidSnapshot {
            node_id: node_id.to_owned(),
            reason: format!("authoritative shard state verification failed: {error}"),
        })?;
    let canonical =
        serde_json::to_vec(&state).map_err(|error| ClusterSnapshotError::InvalidSnapshot {
            node_id: node_id.to_owned(),
            reason: format!("authoritative shard state encoding failed: {error}"),
        })?;
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("authoritative-shard-state:v1", canonical);
    Ok(Some(digest.finish()))
}

fn marker_for_channel_state(
    channel: &str,
    epoch: u64,
    first_in_transit: Option<LogicalTag>,
    channel_state_json: &str,
) -> Result<ChannelMarker, ClusterSnapshotError> {
    ChannelMarker::new(
        channel,
        epoch,
        first_in_transit,
        channel_state_json.as_bytes(),
    )
    .map_err(|error| ClusterSnapshotError::InvalidConsistentCut(error.to_string()))
}

fn cluster_digest(
    network_id: &str,
    cut_tag: LogicalTag,
    shards: &[ClusterShardSnapshot],
    consistent_cut: Option<&ConsistentCut>,
) -> StateDigest {
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("network-id", network_id.as_bytes());
    let mut tag_bytes = Vec::with_capacity(16);
    tag_bytes.extend_from_slice(&cut_tag.tick.to_be_bytes());
    tag_bytes.extend_from_slice(&cut_tag.microstep.to_be_bytes());
    digest.add_domain("cut-tag", tag_bytes);
    for shard in shards {
        digest.add_domain(
            format!("shard:{}:runner", shard.node_id),
            shard.state_digest.0,
        );
        digest.add_domain(
            format!("shard:{}:channels", shard.node_id),
            shard.channel_state_digest.0,
        );
        if let Some(authoritative) = shard.authoritative_state_digest {
            digest.add_domain(
                format!("shard:{}:authoritative", shard.node_id),
                authoritative.0,
            );
        }
    }
    if let Some(cut) = consistent_cut {
        digest.add_domain("consistent-cut", cut.cut_digest.0);
    }
    digest.finish()
}

impl ClusterGlobalSnapshot {
    /// Verify the self-contained integrity and common-frontier claims of a
    /// cluster cut loaded from durable storage or received over the network.
    pub fn verify(&self) -> Result<(), ClusterSnapshotError> {
        if self.schema_version != CLUSTER_SNAPSHOT_SCHEMA_VERSION
            && self.schema_version != LEGACY_CLUSTER_SNAPSHOT_SCHEMA_VERSION
        {
            return Err(ClusterSnapshotError::UnsupportedSchema(self.schema_version));
        }
        if self.network_id.trim().is_empty() || self.shards.is_empty() {
            return Err(ClusterSnapshotError::InvalidGlobalSnapshot(
                "network and at least one shard are required".to_owned(),
            ));
        }
        if let Some(cut) = &self.consistent_cut {
            cut.verify()
                .map_err(|error| ClusterSnapshotError::InvalidConsistentCut(error.to_string()))?;
            if self.cut_tag != cut.safe_tag {
                return Err(ClusterSnapshotError::InvalidConsistentCut(
                    "cluster cut tag does not equal the GVT safe tag".to_owned(),
                ));
            }
            if cut.participants.len() != self.shards.len()
                || cut.channels.len() != self.shards.len()
            {
                return Err(ClusterSnapshotError::InvalidConsistentCut(
                    "cut evidence must contain exactly one participant and channel marker per shard"
                        .to_owned(),
                ));
            }
        }

        let mut previous_node = None;
        let mut seen_layers = BTreeSet::new();
        let mut expected_sim_time = None;
        let mut expected_shape = None;
        let participants = self.consistent_cut.as_ref().map(|cut| {
            cut.participants
                .iter()
                .map(|report| (report.participant.as_str(), report))
                .collect::<BTreeMap<_, _>>()
        });
        let markers = self.consistent_cut.as_ref().map(|cut| {
            cut.channels
                .iter()
                .map(|marker| (marker.channel.as_str(), marker))
                .collect::<BTreeMap<_, _>>()
        });
        let authoritative_shards = self
            .shards
            .iter()
            .filter(|shard| !shard.authoritative_state_json.trim().is_empty())
            .count();
        if authoritative_shards != 0 && authoritative_shards != self.shards.len() {
            return Err(ClusterSnapshotError::InvalidGlobalSnapshot(
                "a cluster cut cannot mix complete shard states with compatibility projections"
                    .to_owned(),
            ));
        }
        if self.schema_version == LEGACY_CLUSTER_SNAPSHOT_SCHEMA_VERSION
            && authoritative_shards != 0
        {
            return Err(ClusterSnapshotError::UnsupportedSchema(self.schema_version));
        }
        for shard in &self.shards {
            if shard.node_id.trim().is_empty() {
                return Err(ClusterSnapshotError::InvalidGlobalSnapshot(
                    "shard node ID must not be empty".to_owned(),
                ));
            }
            if previous_node.is_some_and(|previous| previous >= shard.node_id.as_str()) {
                return Err(ClusterSnapshotError::InvalidGlobalSnapshot(
                    "shards must be sorted by unique node ID".to_owned(),
                ));
            }
            previous_node = Some(shard.node_id.as_str());
            if shard.layers.is_empty() {
                return Err(ClusterSnapshotError::EmptyAssignment {
                    node_id: shard.node_id.clone(),
                });
            }
            for layer in &shard.layers {
                if !seen_layers.insert(*layer) {
                    return Err(ClusterSnapshotError::InvalidGlobalSnapshot(format!(
                        "layer {layer} is assigned to more than one shard"
                    )));
                }
            }
            if shard.layers.windows(2).any(|layers| layers[0] >= layers[1]) {
                return Err(ClusterSnapshotError::InvalidGlobalSnapshot(format!(
                    "layers for shard '{}' must be sorted and unique",
                    shard.node_id
                )));
            }

            let (snapshot, state_digest) = digest_snapshot(&shard.snapshot_json, &shard.node_id)?;
            if state_digest != shard.state_digest {
                return Err(ClusterSnapshotError::DigestMismatch);
            }
            if shard.step != snapshot.t as u64 || shard.sim_time_ms_bits != snapshot.t_ms.to_bits()
            {
                return Err(ClusterSnapshotError::DigestMismatch);
            }
            if self.consistent_cut.is_none() && self.shards.len() > 1 {
                let expected_range = shard.layers.first().copied().map(|first| {
                    (
                        first as usize,
                        shard.layers.last().copied().unwrap_or(first) as usize + 1,
                    )
                });
                if snapshot.layer_range != expected_range {
                    return Err(ClusterSnapshotError::MismatchedLayerRange {
                        node_id: shard.node_id.clone(),
                    });
                }
            }
            if expected_shape
                .replace(snapshot.net.clone())
                .is_some_and(|shape| shape != snapshot.net)
            {
                return Err(ClusterSnapshotError::MismatchedNetworkShape {
                    node_id: shard.node_id.clone(),
                });
            }
            let channel_digest = digest_channel_state(&shard.channel_state_json, &shard.node_id)?;
            if channel_digest != shard.channel_state_digest {
                return Err(ClusterSnapshotError::DigestMismatch);
            }
            let authoritative_digest =
                digest_authoritative_state(&shard.authoritative_state_json, &shard.node_id)?;
            if authoritative_digest != shard.authoritative_state_digest {
                return Err(ClusterSnapshotError::DigestMismatch);
            }
            if let (Some(participants), Some(markers), Some(cut)) = (
                participants.as_ref(),
                markers.as_ref(),
                self.consistent_cut.as_ref(),
            ) {
                let report = participants.get(shard.node_id.as_str()).ok_or_else(|| {
                    ClusterSnapshotError::InvalidConsistentCut(format!(
                        "missing participant evidence for shard {}",
                        shard.node_id
                    ))
                })?;
                if report.local_frontier != LogicalTag::new(shard.step, 0) {
                    return Err(ClusterSnapshotError::InvalidConsistentCut(format!(
                        "participant frontier for shard {} does not match its captured state",
                        shard.node_id
                    )));
                }
                let channel = format!("{}/{}", self.network_id, shard.node_id);
                let marker = markers.get(channel.as_str()).ok_or_else(|| {
                    ClusterSnapshotError::InvalidConsistentCut(format!(
                        "missing channel marker for shard {}",
                        shard.node_id
                    ))
                })?;
                let expected_marker = marker_for_channel_state(
                    &channel,
                    cut.epoch,
                    marker.first_in_transit,
                    &shard.channel_state_json,
                )?;
                if **marker != expected_marker {
                    return Err(ClusterSnapshotError::InvalidConsistentCut(format!(
                        "channel marker for shard {} does not bind to captured channel state",
                        shard.node_id
                    )));
                }
            }
            if self.consistent_cut.is_none() && snapshot.t as u64 != self.cut_tag.tick {
                return Err(ClusterSnapshotError::MixedFrontier {
                    node_id: shard.node_id.clone(),
                    actual: snapshot.t as u64,
                    expected: self.cut_tag.tick,
                });
            }
            if self.consistent_cut.is_none()
                && expected_sim_time
                    .replace(snapshot.t_ms.to_bits())
                    .is_some_and(|time| time != snapshot.t_ms.to_bits())
            {
                return Err(ClusterSnapshotError::MixedSimulationTime {
                    node_id: shard.node_id.clone(),
                });
            }
        }
        if self.consistent_cut.as_ref().is_some_and(|cut| {
            self.shards
                .iter()
                .any(|shard| shard.step < cut.safe_tag.tick)
        }) {
            return Err(ClusterSnapshotError::InvalidConsistentCut(
                "a shard snapshot precedes the GVT safe tag".to_owned(),
            ));
        }
        if cluster_digest(
            &self.network_id,
            self.cut_tag,
            &self.shards,
            self.consistent_cut.as_ref(),
        ) != self.cluster_digest
        {
            return Err(ClusterSnapshotError::DigestMismatch);
        }
        Ok(())
    }
}

/// Immutable filesystem publication for a verified cluster-global snapshot.
/// The digest is the content address, and hard-link publication prevents a
/// concurrent writer from replacing an already published cut.
#[derive(Debug, Clone)]
pub struct FileClusterSnapshotStore {
    root: PathBuf,
}

impl FileClusterSnapshotStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ClusterSnapshotError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|error| ClusterSnapshotError::Io(error.to_string()))?;
        Ok(Self { root })
    }

    pub fn publish(
        &self,
        snapshot: &ClusterGlobalSnapshot,
    ) -> Result<PathBuf, ClusterSnapshotError> {
        snapshot.verify()?;
        let digest = snapshot.cluster_digest;
        let path = self.path_for(digest);
        let bytes = serde_json::to_vec(snapshot)
            .map_err(|error| ClusterSnapshotError::Encoding(error.to_string()))?;
        atomic_publish_no_replace(&path, &bytes, digest)?;
        Ok(path)
    }

    /// Publish a cut idempotently for retrying management requests. A retry
    /// with the same content-addressed digest is successful only when the
    /// already-published bytes still verify as the exact same snapshot. This
    /// keeps immutable publication while making client retries safe.
    pub fn publish_idempotent(
        &self,
        snapshot: &ClusterGlobalSnapshot,
    ) -> Result<PathBuf, ClusterSnapshotError> {
        match self.publish(snapshot) {
            Ok(path) => Ok(path),
            Err(ClusterSnapshotError::AlreadyPublished(digest)) => {
                let existing = self.load(digest)?;
                if existing == *snapshot {
                    Ok(self.path_for(digest))
                } else {
                    Err(ClusterSnapshotError::DigestMismatch)
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn load(&self, digest: StateDigest) -> Result<ClusterGlobalSnapshot, ClusterSnapshotError> {
        let path = self.path_for(digest);
        let bytes = fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ClusterSnapshotError::MissingSnapshot(digest)
            } else {
                ClusterSnapshotError::Io(format!("{}: {error}", path.display()))
            }
        })?;
        let snapshot: ClusterGlobalSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| ClusterSnapshotError::Encoding(error.to_string()))?;
        if snapshot.cluster_digest != digest {
            return Err(ClusterSnapshotError::DigestMismatch);
        }
        snapshot.verify()?;
        Ok(snapshot)
    }

    fn path_for(&self, digest: StateDigest) -> PathBuf {
        self.root.join(format!("cluster-snapshot-{digest}.json"))
    }
}

fn create_unique_temp(path: &Path) -> Result<(PathBuf, fs::File), ClusterSnapshotError> {
    for attempt in 0..32u32 {
        let temporary = path.with_extension(format!("tmp-{}-{attempt}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ClusterSnapshotError::Io(error.to_string())),
        }
    }
    Err(ClusterSnapshotError::Io(
        "unable to allocate a unique temporary snapshot path".to_owned(),
    ))
}

fn atomic_publish_no_replace(
    path: &Path,
    bytes: &[u8],
    digest: StateDigest,
) -> Result<(), ClusterSnapshotError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ClusterSnapshotError::Io(error.to_string()))?;
    }
    let (temporary, mut file) = create_unique_temp(path)?;
    let write_result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| ClusterSnapshotError::Io(error.to_string()));
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let result = fs::hard_link(&temporary, path);
    let _ = fs::remove_file(&temporary);
    match result {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ClusterSnapshotError::AlreadyPublished(digest));
        }
        Err(error) => return Err(ClusterSnapshotError::Io(error.to_string())),
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| ClusterSnapshotError::Io(error.to_string()))?;
    }
    Ok(())
}

/// Assemble a complete cluster cut from node-owned snapshot responses.
///
/// The expected assignment is supplied by the orchestrator's placement view,
/// not by the callers.  Every node must contribute once, all frontiers must
/// match, and multi-node snapshots must carry the generation-scoped layer
/// range emitted by the distributed runner.  The returned digest is stable
/// across response arrival order because shards are canonicalised by node ID.
pub fn assemble(
    network_id: impl Into<String>,
    expected_assignment: &BTreeMap<String, Vec<u32>>,
    inputs: Vec<ShardSnapshotInput>,
) -> Result<ClusterGlobalSnapshot, ClusterSnapshotError> {
    assemble_with_cut(network_id, expected_assignment, inputs, None)
}

/// Assemble a cluster snapshot from a completed asynchronous GVT/consistent
/// cut. Local shard frontiers may differ; the safe tag is the proven minimum.
pub fn assemble_live(
    network_id: impl Into<String>,
    expected_assignment: &BTreeMap<String, Vec<u32>>,
    inputs: Vec<ShardSnapshotInput>,
    evidence: Vec<LiveCutEvidence>,
    cut: ConsistentCut,
) -> Result<ClusterGlobalSnapshot, ClusterSnapshotError> {
    let network_id = network_id.into();
    cut.verify()
        .map_err(|error| ClusterSnapshotError::InvalidConsistentCut(error.to_string()))?;
    let expected_nodes = expected_assignment.keys().collect::<BTreeSet<_>>();
    let mut by_node = BTreeMap::new();
    for item in evidence {
        if !expected_nodes.contains(&item.participant.participant) {
            return Err(ClusterSnapshotError::InvalidConsistentCut(
                "evidence contains an unexpected participant".to_owned(),
            ));
        }
        if by_node
            .insert(item.participant.participant.clone(), item)
            .is_some()
        {
            return Err(ClusterSnapshotError::InvalidConsistentCut(
                "evidence contains a duplicate participant".to_owned(),
            ));
        }
    }
    for node_id in &expected_nodes {
        let item = by_node.get(*node_id).ok_or_else(|| {
            ClusterSnapshotError::InvalidConsistentCut(format!(
                "missing evidence for participant {node_id}"
            ))
        })?;
        if item.marker.epoch != cut.epoch
            || item.marker.channel != format!("{network_id}/{node_id}")
        {
            return Err(ClusterSnapshotError::InvalidConsistentCut(format!(
                "marker for participant {node_id} does not belong to this cut"
            )));
        }
        let input = inputs
            .iter()
            .find(|input| input.node_id == **node_id)
            .ok_or_else(|| {
                ClusterSnapshotError::InvalidConsistentCut(format!(
                    "missing shard input for participant {node_id}"
                ))
            })?;
        let snapshot =
            decode_snapshot_with_profile_backfill(&input.snapshot_json).map_err(|error| {
                ClusterSnapshotError::InvalidSnapshot {
                    node_id: (*node_id).clone(),
                    reason: error.to_string(),
                }
            })?;
        if item.participant.local_frontier != LogicalTag::new(snapshot.t as u64, 0) {
            return Err(ClusterSnapshotError::InvalidConsistentCut(format!(
                "participant evidence for {node_id} does not match captured runner frontier"
            )));
        }
        let expected_marker = marker_for_channel_state(
            &item.marker.channel,
            cut.epoch,
            item.marker.first_in_transit,
            &input.channel_state_json,
        )?;
        if item.marker != expected_marker {
            return Err(ClusterSnapshotError::InvalidConsistentCut(format!(
                "marker for participant {node_id} does not bind to captured channel state"
            )));
        }
    }
    assemble_with_cut(network_id, expected_assignment, inputs, Some(cut))
}

fn assemble_with_cut(
    network_id: impl Into<String>,
    expected_assignment: &BTreeMap<String, Vec<u32>>,
    inputs: Vec<ShardSnapshotInput>,
    consistent_cut: Option<ConsistentCut>,
) -> Result<ClusterGlobalSnapshot, ClusterSnapshotError> {
    let network_id = network_id.into();
    if expected_assignment.is_empty() {
        return Err(ClusterSnapshotError::NoAssignedShards);
    }

    let expected_nodes: BTreeSet<&str> = expected_assignment.keys().map(String::as_str).collect();
    let mut by_node = BTreeMap::new();
    for input in inputs {
        if !expected_nodes.contains(input.node_id.as_str()) {
            return Err(ClusterSnapshotError::UnexpectedShard(input.node_id));
        }
        if by_node.insert(input.node_id.clone(), input).is_some() {
            return Err(ClusterSnapshotError::DuplicateShard(
                by_node.keys().next_back().cloned().unwrap_or_default(),
            ));
        }
    }

    let missing = expected_nodes
        .iter()
        .filter(|node_id| !by_node.contains_key(**node_id))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(ClusterSnapshotError::MissingShards(missing.join(", ")));
    }

    let mut shards: Vec<ClusterShardSnapshot> = Vec::with_capacity(by_node.len());
    let mut expected_shape = None;
    let mut frontier = None;
    let authoritative_shards = by_node
        .values()
        .filter(|input| !input.authoritative_state_json.trim().is_empty())
        .count();
    if authoritative_shards != 0 && authoritative_shards != by_node.len() {
        return Err(ClusterSnapshotError::InvalidGlobalSnapshot(
            "a cluster cut cannot mix complete shard states with compatibility projections"
                .to_owned(),
        ));
    }
    for (node_id, input) in by_node {
        let mut assigned = input.layers;
        if assigned.is_empty() {
            return Err(ClusterSnapshotError::EmptyAssignment { node_id });
        }
        assigned.sort_unstable();
        let mut unique = BTreeSet::new();
        for layer in &assigned {
            if !unique.insert(*layer) {
                return Err(ClusterSnapshotError::DuplicateLayer {
                    node_id,
                    layer: *layer,
                });
            }
        }
        let expected_layers = expected_assignment
            .get(&node_id)
            .expect("assignment was checked above");
        let mut expected_layers = expected_layers.clone();
        expected_layers.sort_unstable();
        if assigned != expected_layers {
            return Err(ClusterSnapshotError::MismatchedLayerRange { node_id });
        }

        let (snapshot, state_digest) = digest_snapshot(&input.snapshot_json, &node_id)?;
        let combined_bytes = input
            .snapshot_json
            .len()
            .saturating_add(input.channel_state_json.len())
            .saturating_add(input.authoritative_state_json.len());
        if combined_bytes > MAX_SHARD_SNAPSHOT_BYTES {
            return Err(ClusterSnapshotError::SnapshotTooLarge {
                node_id,
                bytes: combined_bytes,
            });
        }
        let channel_state_digest = digest_channel_state(&input.channel_state_json, &node_id)?;
        if expected_assignment.len() > 1 {
            let expected_range = assigned.first().copied().map(|first| {
                (
                    first as usize,
                    assigned.last().copied().unwrap_or(first) as usize + 1,
                )
            });
            if snapshot.layer_range != expected_range {
                return Err(ClusterSnapshotError::MismatchedLayerRange { node_id });
            }
        }
        if let Some(shape) = expected_shape.as_ref() {
            if shape != &snapshot.net {
                return Err(ClusterSnapshotError::MismatchedNetworkShape { node_id });
            }
        } else {
            expected_shape = Some(snapshot.net.clone());
        }
        let authoritative_state_digest =
            digest_authoritative_state(&input.authoritative_state_json, &node_id)?;
        let step = snapshot.t as u64;
        if authoritative_state_digest.is_some() {
            let state: crate::authoritative_shard::ShardState =
                serde_json::from_str(&input.authoritative_state_json).map_err(|error| {
                    ClusterSnapshotError::InvalidSnapshot {
                        node_id: node_id.clone(),
                        reason: error.to_string(),
                    }
                })?;
            if state.applied_tag.tick != step
                || state.biological_state != input.snapshot_json.as_bytes()
            {
                return Err(ClusterSnapshotError::InvalidConsistentCut(format!(
                    "authoritative state for shard {} does not bind to its captured runner boundary",
                    node_id
                )));
            }
        }
        if consistent_cut.is_none() {
            if let Some(expected) = frontier {
                if expected != step {
                    return Err(ClusterSnapshotError::MixedFrontier {
                        node_id,
                        actual: step,
                        expected,
                    });
                }
            } else {
                frontier = Some(step);
            }
        } else {
            frontier = Some(frontier.map_or(step, |current| current.min(step)));
        }
        if consistent_cut.is_none() {
            if let Some(previous) = shards.first() {
                if previous.sim_time_ms_bits != snapshot.t_ms.to_bits() {
                    return Err(ClusterSnapshotError::MixedSimulationTime { node_id });
                }
            }
        }
        shards.push(ClusterShardSnapshot {
            node_id,
            layers: assigned,
            snapshot_json: input.snapshot_json,
            channel_state_json: input.channel_state_json,
            step,
            sim_time_ms_bits: snapshot.t_ms.to_bits(),
            state_digest,
            channel_state_digest,
            authoritative_state_digest,
            authoritative_state_json: input.authoritative_state_json,
        });
    }

    let cut_tag = consistent_cut
        .as_ref()
        .map(|cut| cut.safe_tag)
        .unwrap_or_else(|| LogicalTag::new(frontier.expect("non-empty assignment"), 0));
    let cluster_digest = cluster_digest(&network_id, cut_tag, &shards, consistent_cut.as_ref());
    Ok(ClusterGlobalSnapshot {
        schema_version: CLUSTER_SNAPSHOT_SCHEMA_VERSION,
        network_id,
        cut_tag,
        cluster_digest,
        shards,
        consistent_cut,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;
    use crate::runner::Runner;
    use crate::sim::{Learning, NeuronModel};

    fn snapshot(layer_range: Option<(usize, usize)>, step: usize) -> String {
        let mut config = NetworkConfig::default();
        config.num_hidden_layers = 1;
        let mut runner = Runner::new(
            Default::default(),
            Default::default(),
            config,
            NeuronModel::Lif,
            Learning::Stdp,
        );
        runner.layer_range = layer_range.map(|(start, end)| start..end);
        runner.t = step;
        runner.export_network_json().expect("snapshot")
    }

    #[test]
    fn assembly_is_order_independent_and_requires_one_common_frontier() {
        let expected = BTreeMap::from([
            ("node-a".to_owned(), vec![0]),
            ("node-b".to_owned(), vec![1]),
        ]);
        let first = ShardSnapshotInput {
            node_id: "node-a".to_owned(),
            layers: vec![0],
            snapshot_json: snapshot(Some((0, 1)), 7),
            channel_state_json: "{}".to_owned(),
            authoritative_state_json: String::new(),
        };
        let second = ShardSnapshotInput {
            node_id: "node-b".to_owned(),
            layers: vec![1],
            snapshot_json: snapshot(Some((1, 2)), 7),
            channel_state_json: "{}".to_owned(),
            authoritative_state_json: String::new(),
        };
        let left = assemble("brain", &expected, vec![first.clone(), second.clone()]).unwrap();
        let right = assemble("brain", &expected, vec![second, first]).unwrap();
        assert_eq!(left, right);
        assert_eq!(left.cut_tag, LogicalTag::new(7, 0));
        assert_eq!(left.shards.len(), 2);
        assert!(matches!(
            assemble(
                "brain",
                &expected,
                vec![
                    ShardSnapshotInput {
                        node_id: "node-a".to_owned(),
                        layers: vec![0],
                        snapshot_json: snapshot(Some((0, 1)), 7),
                        channel_state_json: "{}".to_owned(),
                        authoritative_state_json: String::new(),
                    },
                    ShardSnapshotInput {
                        node_id: "node-b".to_owned(),
                        layers: vec![1],
                        snapshot_json: snapshot(Some((1, 2)), 8),
                        channel_state_json: "{}".to_owned(),
                        authoritative_state_json: String::new(),
                    },
                ],
            ),
            Err(ClusterSnapshotError::MixedFrontier { .. })
        ));
    }

    #[test]
    fn assembly_rejects_missing_and_duplicate_shards() {
        let expected = BTreeMap::from([("node-a".to_owned(), vec![0])]);
        assert!(matches!(
            assemble("brain", &expected, Vec::new()),
            Err(ClusterSnapshotError::MissingShards(_))
        ));
        let input = ShardSnapshotInput {
            node_id: "node-a".to_owned(),
            layers: vec![0],
            snapshot_json: snapshot(None, 1),
            channel_state_json: String::new(),
            authoritative_state_json: String::new(),
        };
        assert!(matches!(
            assemble("brain", &expected, vec![input.clone(), input]),
            Err(ClusterSnapshotError::DuplicateShard(_))
        ));
    }

    #[test]
    fn channel_state_is_part_of_the_cluster_digest_and_cut_verification() {
        let expected = BTreeMap::from([
            ("node-a".to_owned(), vec![0]),
            ("node-b".to_owned(), vec![1]),
        ]);
        let inputs = vec![
            ShardSnapshotInput {
                node_id: "node-a".to_owned(),
                layers: vec![0],
                snapshot_json: snapshot(Some((0, 1)), 2),
                channel_state_json: r#"{"queued":[1]}"#.to_owned(),
                authoritative_state_json: String::new(),
            },
            ShardSnapshotInput {
                node_id: "node-b".to_owned(),
                layers: vec![1],
                snapshot_json: snapshot(Some((1, 2)), 2),
                channel_state_json: r#"{"queued":[2]}"#.to_owned(),
                authoritative_state_json: String::new(),
            },
        ];
        let mut cut = assemble("brain", &expected, inputs).expect("assemble cut");
        cut.verify().expect("verified cut");
        let original_digest = cut.cluster_digest;
        let mut changed_metadata = cut.clone();
        changed_metadata.network_id = "other-brain".to_owned();
        assert!(matches!(
            changed_metadata.verify(),
            Err(ClusterSnapshotError::DigestMismatch)
        ));
        cut.shards[0].channel_state_json = r#"{"queued":[99]}"#.to_owned();
        assert!(matches!(
            cut.verify(),
            Err(ClusterSnapshotError::DigestMismatch)
        ));
        let mut changed_digest = cut.clone();
        changed_digest.shards[0].channel_state_digest = StateDigest([7; 16]);
        assert_ne!(
            original_digest,
            cluster_digest(
                &changed_digest.network_id,
                changed_digest.cut_tag,
                &changed_digest.shards,
                changed_digest.consistent_cut.as_ref()
            )
        );
    }

    #[test]
    fn filesystem_cluster_cut_is_immutable_and_tamper_detecting() {
        let root =
            std::env::temp_dir().join(format!("aarnn-cluster-snapshot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let expected = BTreeMap::from([("node-a".to_owned(), vec![0])]);
        let cut = assemble(
            "brain",
            &expected,
            vec![ShardSnapshotInput {
                node_id: "node-a".to_owned(),
                layers: vec![0],
                snapshot_json: snapshot(None, 3),
                channel_state_json: "{}".to_owned(),
                authoritative_state_json: String::new(),
            }],
        )
        .expect("assemble cut");
        let store = FileClusterSnapshotStore::new(&root).expect("store");
        let path = store.publish(&cut).expect("publish");
        assert_eq!(store.load(cut.cluster_digest).expect("load"), cut);
        assert!(matches!(
            store.publish(&cut),
            Err(ClusterSnapshotError::AlreadyPublished(_))
        ));

        let mut tampered: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read cut")).expect("decode cut");
        tampered["shards"][0]["channel_state_json"] = serde_json::json!("{\"queued\":[7]}");
        fs::write(&path, serde_json::to_vec(&tampered).expect("encode tamper")).expect("tamper");
        assert!(matches!(
            store.load(cut.cluster_digest),
            Err(ClusterSnapshotError::DigestMismatch)
        ));
        fs::remove_dir_all(root).expect("remove test store");
    }

    #[test]
    fn filesystem_cluster_cut_retry_is_idempotent_but_changed_content_is_not() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-cluster-snapshot-retry-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let expected = BTreeMap::from([("node-a".to_owned(), vec![0])]);
        let cut = assemble(
            "brain",
            &expected,
            vec![ShardSnapshotInput {
                node_id: "node-a".to_owned(),
                layers: vec![0],
                snapshot_json: snapshot(None, 4),
                channel_state_json: "{}".to_owned(),
                authoritative_state_json: String::new(),
            }],
        )
        .expect("assemble cut");
        let store = FileClusterSnapshotStore::new(&root).expect("store");
        let first = store.publish_idempotent(&cut).expect("first publish");
        let retry = store.publish_idempotent(&cut).expect("idempotent retry");
        assert_eq!(first, retry);

        let mut changed = cut.clone();
        changed.shards[0].channel_state_json = r#"{"queued":[1]}"#.to_owned();
        changed.shards[0].channel_state_digest =
            digest_channel_state(&changed.shards[0].channel_state_json, "node-a")
                .expect("channel digest");
        changed.cluster_digest = cluster_digest(
            &changed.network_id,
            changed.cut_tag,
            &changed.shards,
            changed.consistent_cut.as_ref(),
        );
        assert_ne!(changed.cluster_digest, cut.cluster_digest);
        assert!(store.publish_idempotent(&changed).is_ok());
        fs::remove_dir_all(root).expect("remove test store");
    }

    #[test]
    fn complete_shard_state_is_verified_and_bound_to_the_projection() {
        use crate::authoritative_shard::ShardState;
        use crate::deterministic::{BrainId, LeaseTerm, ShardId, StreamId, TopologyGeneration};
        use crate::durability::DurableShard;

        let snapshot_json = snapshot(None, 0);
        let shard = DurableShard::new(
            BrainId::new(101).unwrap(),
            ShardId::new(102).unwrap(),
            TopologyGeneration::INITIAL,
            crate::deterministic::PartitionGeneration::INITIAL,
            LeaseTerm::INITIAL,
            StreamId::new(103).unwrap(),
            4096,
            snapshot_json.as_bytes().to_vec(),
            br#"{}"#.to_vec(),
        );
        let state: ShardState = shard.checkpoint_payload().unwrap().try_into().unwrap();
        let authoritative_state_json = serde_json::to_string(&state).unwrap();
        let expected = BTreeMap::from([("node-a".to_owned(), vec![0])]);
        let cut = assemble(
            "brain",
            &expected,
            vec![ShardSnapshotInput {
                node_id: "node-a".to_owned(),
                layers: vec![0],
                snapshot_json: snapshot_json.clone(),
                channel_state_json: "{}".to_owned(),
                authoritative_state_json: authoritative_state_json.clone(),
            }],
        )
        .unwrap();
        cut.verify().unwrap();
        assert_eq!(
            cut.shards[0].authoritative_state_json,
            authoritative_state_json
        );
        assert!(cut.shards[0].authoritative_state_digest.is_some());

        let mut invalid = cut.clone();
        invalid.shards[0].snapshot_json = snapshot(None, 1);
        assert!(matches!(
            invalid.verify(),
            Err(ClusterSnapshotError::DigestMismatch)
                | Err(ClusterSnapshotError::InvalidConsistentCut(_))
        ));
    }

    #[test]
    fn live_assembly_accepts_asynchronous_frontiers_and_retains_gvt_evidence() {
        let expected = BTreeMap::from([
            ("node-a".to_owned(), vec![0]),
            ("node-b".to_owned(), vec![1]),
        ]);
        let mut coordinator = crate::consistent_cut::ConsistentCutCoordinator::begin(
            9,
            ["node-a".to_owned(), "node-b".to_owned()],
            ["brain/node-a".to_owned(), "brain/node-b".to_owned()],
        )
        .unwrap();
        let reports = [
            ParticipantReport {
                participant: "node-a".to_owned(),
                local_frontier: LogicalTag::new(7, 0),
                queued_min: None,
                in_flight_min: None,
                activity_epoch: 1,
            },
            ParticipantReport {
                participant: "node-b".to_owned(),
                local_frontier: LogicalTag::new(9, 0),
                queued_min: None,
                in_flight_min: None,
                activity_epoch: 1,
            },
        ];
        let mut evidence = Vec::new();
        for report in reports {
            let marker =
                ChannelMarker::new(format!("brain/{}", report.participant), 9, None, b"{}")
                    .unwrap();
            coordinator.record_report(report.clone()).unwrap();
            coordinator.record_marker(marker.clone()).unwrap();
            evidence.push(LiveCutEvidence {
                participant: report,
                marker,
            });
        }
        let cut = coordinator.finalise().unwrap();
        let snapshot = assemble_live(
            "brain",
            &expected,
            vec![
                ShardSnapshotInput {
                    node_id: "node-a".to_owned(),
                    layers: vec![0],
                    snapshot_json: snapshot(Some((0, 1)), 7),
                    channel_state_json: "{}".to_owned(),
                    authoritative_state_json: String::new(),
                },
                ShardSnapshotInput {
                    node_id: "node-b".to_owned(),
                    layers: vec![1],
                    snapshot_json: snapshot(Some((1, 2)), 9),
                    channel_state_json: "{}".to_owned(),
                    authoritative_state_json: String::new(),
                },
            ],
            evidence,
            cut,
        )
        .unwrap();
        assert_eq!(snapshot.cut_tag, LogicalTag::new(7, 0));
        assert!(snapshot.consistent_cut.is_some());
        snapshot.verify().unwrap();
    }
}
