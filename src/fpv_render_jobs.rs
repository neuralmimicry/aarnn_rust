//! Persistent, multi-worker FPV rendering from immutable display projections.
//!
//! Workers claim individual frames through process-shared file leases. A web
//! process may disappear without losing the job, its completed frames, or its
//! output; another replica can resume the remaining work from the same root.
//! Rendering reads only the captured display/activity projection and never
//! steps or mutates the live network.

use crate::morphology_contract::{AnatomicalId, DisplayRole, DisplaySnapshot, Vec3};
use crate::shared_fs::{atomic_write, read_json_if_exists, try_acquire_lease, write_json_pretty};
use crate::visualization::{
    VISUALIZATION_POLICY_VERSION, VisualizationStage, automatic_target, highest_supported_stage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const FPV_RENDER_JOB_SCHEMA_VERSION: u32 = 1;
pub const FPV_RENDER_MAX_NODES: usize = 65_536;
pub const FPV_RENDER_MAX_EDGES: usize = 131_072;
pub const FPV_RENDER_MAX_PATHS: usize = 65_536;
pub const FPV_RENDER_MAX_FRAMES: u32 = 9000;
pub const FPV_RENDER_FRAME_CHUNK_BYTES: usize = 1024 * 1024;
pub const FPV_RENDER_MAX_SCRATCH_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const FPV_REPLAY_MAX_SNAPSHOT_BYTES: usize = 512 * 1024 * 1024;
pub const FPV_REPLAY_MAX_EVENTS: usize = 2_000_000;
pub const FPV_REPLAY_MAX_STEPS: u64 = 2_000_000;
const FPV_REPLAY_ACTIVE_IDS_PER_FRAME: usize = 512;
const FPV_RENDER_MAX_GEOMETRY_POINTS: usize = 2_000_000;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(200);
static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FpvRenderRequest {
    #[serde(default = "default_job_schema_version")]
    pub schema_version: u32,
    pub network_id: String,
    pub scene: DisplaySnapshot,
    /// Stable anatomical identities selected on the low-detail planner map.
    pub waypoint_ids: Vec<AnatomicalId>,
    /// Activity observed at submission time. This is presentation metadata;
    /// it does not inject spikes or alter network state.
    #[serde(default)]
    pub active_node_ids: Vec<AnatomicalId>,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_frame_rate")]
    pub frame_rate: u32,
    #[serde(default = "default_frame_count")]
    pub frame_count: u32,
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    #[serde(default = "default_true")]
    pub focus_active_regions: bool,
    /// Manual or automatic detail selected independently at each route point.
    #[serde(default)]
    pub visualization_keyframes: Vec<FpvVisualizationKeyframe>,
    /// Resolver version captured so resumed workers remain reproducible.
    #[serde(default = "default_visualization_policy_version")]
    pub visualization_policy_version: u16,
    /// Planner-side p95 visualisation cost, captured so all render workers
    /// resolve automatic stages identically regardless of worker load/order.
    #[serde(default = "default_auto_visualization_latency_ms")]
    pub auto_visualization_latency_ms: f64,
    /// Optional isolated playback. The snapshot is removed from the render
    /// manifest and persisted separately when the job is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<FpvReplaySpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FpvVisualizationKeyframe {
    pub waypoint_id: AnatomicalId,
    pub automatic: bool,
    pub stage: u8,
    pub zoom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FpvReplaySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_json: Option<String>,
    pub duration_ms: u64,
    pub step_ms: f64,
    pub tracks: Vec<FpvReplayTrack>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpvReplaySource {
    Aer,
    Audio,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FpvReplayTrack {
    pub source: FpvReplaySource,
    #[serde(default)]
    pub sync_offset_us: i64,
    pub events: Vec<FpvReplayEvent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FpvReplayEvent {
    pub timestamp_us: u64,
    pub sensory_index: u32,
    pub value: i8,
}

fn default_width() -> u32 {
    1280
}
fn default_job_schema_version() -> u32 {
    FPV_RENDER_JOB_SCHEMA_VERSION
}
fn default_height() -> u32 {
    720
}
fn default_frame_rate() -> u32 {
    30
}
fn default_frame_count() -> u32 {
    300
}
fn default_zoom() -> f64 {
    1.0
}
fn default_auto_visualization_latency_ms() -> f64 {
    16.0
}
fn default_visualization_policy_version() -> u16 {
    VISUALIZATION_POLICY_VERSION
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpvRenderState {
    Queued,
    Replaying,
    Rendering,
    Encoding,
    Complete,
    Failed,
    Cancelled,
}

impl FpvRenderState {
    fn terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpvRenderJobStatus {
    pub schema_version: u32,
    pub job_id: String,
    pub owner_id: String,
    pub network_id: String,
    pub state: FpvRenderState,
    pub frame_count: u32,
    pub completed_frames: u32,
    pub frame_rate: u32,
    pub width: u32,
    pub height: u32,
    pub sampled_nodes: u32,
    pub sampled_edges: u32,
    pub projection_complete: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub error: Option<String>,
    pub video_ready: bool,
    #[serde(default)]
    pub replay_enabled: bool,
    #[serde(default)]
    pub replay_frames_total: u32,
    #[serde(default)]
    pub replay_frames_completed: u32,
    /// Immutable effective complexity stage for each rendered frame.
    #[serde(default)]
    pub visualization_stage_track: Vec<u8>,
    /// Resolver version used to produce the per-frame stage track.
    #[serde(default)]
    pub visualization_policy_version: u16,
}

#[derive(Debug, Error)]
pub enum FpvRenderError {
    #[error("FPV render job is invalid: {0}")]
    Invalid(String),
    #[error("FPV render job storage failed: {0}")]
    Storage(String),
    #[error("FPV render frame failed: {0}")]
    Render(String),
    #[error("FPV video encoding failed: {0}")]
    Encode(String),
}

impl FpvRenderRequest {
    pub fn validate(&self) -> Result<(), FpvRenderError> {
        self.validate_inner(true)
    }

    fn validate_stored_manifest(&self) -> Result<(), FpvRenderError> {
        self.validate_inner(false)
    }

    fn validate_inner(&self, require_replay_snapshot: bool) -> Result<(), FpvRenderError> {
        if self.visualization_policy_version != VISUALIZATION_POLICY_VERSION {
            return Err(FpvRenderError::Invalid(format!(
                "unsupported visualisation policy version {}",
                self.visualization_policy_version
            )));
        }
        if self.schema_version != FPV_RENDER_JOB_SCHEMA_VERSION {
            return Err(FpvRenderError::Invalid(format!(
                "unsupported FPV job schema version {}",
                self.schema_version
            )));
        }
        if self.network_id.trim().is_empty() || self.network_id.len() > 256 {
            return Err(FpvRenderError::Invalid(
                "network ID is empty or too long".to_owned(),
            ));
        }
        if self.scene.nodes.is_empty() || self.scene.nodes.len() > FPV_RENDER_MAX_NODES {
            return Err(FpvRenderError::Invalid(format!(
                "display projection must contain 1..={FPV_RENDER_MAX_NODES} neurons"
            )));
        }
        if self.scene.edges.len() > FPV_RENDER_MAX_EDGES
            || self.scene.paths.len() > FPV_RENDER_MAX_PATHS
            || self.scene.markers.len() > FPV_RENDER_MAX_EDGES
        {
            return Err(FpvRenderError::Invalid(
                "display projection exceeds its edge or path bounds".to_owned(),
            ));
        }
        if self.waypoint_ids.len() < 2 || self.waypoint_ids.len() > 512 {
            return Err(FpvRenderError::Invalid(
                "camera route must contain between 2 and 512 waypoints".to_owned(),
            ));
        }
        if self.width < 320
            || self.width > 3840
            || self.height < 180
            || self.height > 2160
            || self.width % 2 != 0
            || self.height % 2 != 0
        {
            return Err(FpvRenderError::Invalid(
                "video dimensions must be even and within 320x180..3840x2160".to_owned(),
            ));
        }
        if !(1..=60).contains(&self.frame_rate)
            || self.frame_count == 0
            || self.frame_count > FPV_RENDER_MAX_FRAMES
        {
            return Err(FpvRenderError::Invalid(
                "frame rate or frame count is outside the supported range".to_owned(),
            ));
        }
        let estimated_scratch = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .and_then(|pixels| pixels.checked_mul(3))
            .and_then(|bytes| bytes.checked_mul(u64::from(self.frame_count)))
            .ok_or_else(|| {
                FpvRenderError::Invalid("estimated frame storage overflows".to_owned())
            })?;
        if estimated_scratch > FPV_RENDER_MAX_SCRATCH_BYTES {
            return Err(FpvRenderError::Invalid(format!(
                "frame intermediates would exceed the {} GiB job storage limit",
                FPV_RENDER_MAX_SCRATCH_BYTES / 1024 / 1024 / 1024
            )));
        }
        if !self.zoom.is_finite() || !(0.2..=4.0).contains(&self.zoom) {
            return Err(FpvRenderError::Invalid(
                "camera zoom must be finite and within 0.2..=4.0".to_owned(),
            ));
        }
        if !self.auto_visualization_latency_ms.is_finite()
            || !(0.0..=10_000.0).contains(&self.auto_visualization_latency_ms)
        {
            return Err(FpvRenderError::Invalid(
                "automatic visualisation latency must be finite and bounded".to_owned(),
            ));
        }
        if !self.visualization_keyframes.is_empty()
            && self.visualization_keyframes.len() != self.waypoint_ids.len()
        {
            return Err(FpvRenderError::Invalid(
                "visualisation keyframes must align with camera waypoints".to_owned(),
            ));
        }
        for (index, keyframe) in self.visualization_keyframes.iter().enumerate() {
            if keyframe.waypoint_id != self.waypoint_ids[index]
                || VisualizationStage::from_number(keyframe.stage).is_none()
                || !keyframe.zoom.is_finite()
                || !(0.2..=4.0).contains(&keyframe.zoom)
            {
                return Err(FpvRenderError::Invalid(
                    "visualisation keyframe identity, stage or zoom is invalid".to_owned(),
                ));
            }
        }
        let nodes = self
            .scene
            .nodes
            .iter()
            .map(|node| (node.id, node.position_mm))
            .collect::<BTreeMap<_, _>>();
        if nodes.len() != self.scene.nodes.len() {
            return Err(FpvRenderError::Invalid(
                "display projection contains duplicate neuron identities".to_owned(),
            ));
        }
        if nodes.values().any(|point| !finite_vec(*point)) {
            return Err(FpvRenderError::Invalid(
                "display projection contains non-finite coordinates".to_owned(),
            ));
        }
        if self.waypoint_ids.iter().any(|id| !nodes.contains_key(id))
            || self
                .active_node_ids
                .iter()
                .any(|id| !nodes.contains_key(id))
        {
            return Err(FpvRenderError::Invalid(
                "camera route or activity references a neuron outside the projection".to_owned(),
            ));
        }
        let geometry_points = self
            .scene
            .edges
            .iter()
            .map(|edge| edge.points_mm.len())
            .chain(self.scene.paths.iter().map(|path| path.points_mm.len()))
            .fold(0usize, usize::saturating_add);
        if geometry_points > FPV_RENDER_MAX_GEOMETRY_POINTS {
            return Err(FpvRenderError::Invalid(
                "display projection exceeds its geometry point budget".to_owned(),
            ));
        }
        if self.scene.edges.iter().any(|edge| {
            !nodes.contains_key(&edge.source)
                || !nodes.contains_key(&edge.target)
                || edge.points_mm.iter().any(|point| !finite_vec(*point))
        }) || self.scene.paths.iter().any(|path| {
            !nodes.contains_key(&path.owner)
                || path.points_mm.iter().any(|point| !finite_vec(*point))
                || !path.radius_mm.is_finite()
        }) {
            return Err(FpvRenderError::Invalid(
                "display geometry has invalid owners or coordinates".to_owned(),
            ));
        }
        if self
            .scene
            .markers
            .iter()
            .any(|marker| !nodes.contains_key(&marker.owner) || !finite_vec(marker.position_mm))
        {
            return Err(FpvRenderError::Invalid(
                "display projection contains an invalid activity marker".to_owned(),
            ));
        }
        if let Some(replay) = &self.replay {
            if !replay.step_ms.is_finite() || !(0.01..=100.0).contains(&replay.step_ms) {
                return Err(FpvRenderError::Invalid(
                    "isolated replay step must be between 0.01 and 100 ms".to_owned(),
                ));
            }
            let render_duration_ms = u64::from(self.frame_count)
                .saturating_mul(1000)
                .div_ceil(u64::from(self.frame_rate));
            if replay.duration_ms == 0 || replay.duration_ms > render_duration_ms {
                return Err(FpvRenderError::Invalid(
                    "replay duration must fit within the rendered video".to_owned(),
                ));
            }
            let estimated_steps = (replay.duration_ms as f64 / replay.step_ms).ceil();
            if !estimated_steps.is_finite() || estimated_steps > FPV_REPLAY_MAX_STEPS as f64 {
                return Err(FpvRenderError::Invalid(format!(
                    "isolated replay exceeds the {FPV_REPLAY_MAX_STEPS} step budget"
                )));
            }
            let Some(snapshot_json) = replay.snapshot_json.as_deref() else {
                if require_replay_snapshot {
                    return Err(FpvRenderError::Invalid(
                        "isolated replay requires a saved network snapshot".to_owned(),
                    ));
                }
                return Ok(());
            };
            if snapshot_json.is_empty() || snapshot_json.len() > FPV_REPLAY_MAX_SNAPSHOT_BYTES {
                return Err(FpvRenderError::Invalid(
                    "isolated replay snapshot is empty or exceeds the 512 MiB transfer limit"
                        .to_owned(),
                ));
            }
            if replay.tracks.is_empty() || replay.tracks.len() > 8 {
                return Err(FpvRenderError::Invalid(
                    "isolated replay requires 1..=8 synchronized input tracks".to_owned(),
                ));
            }
            let mut event_count = 0usize;
            for track in &replay.tracks {
                event_count = event_count.saturating_add(track.events.len());
                if event_count > FPV_REPLAY_MAX_EVENTS {
                    return Err(FpvRenderError::Invalid(format!(
                        "synchronized replay exceeds the {FPV_REPLAY_MAX_EVENTS} event budget"
                    )));
                }
                for event in &track.events {
                    let aligned = i128::from(event.timestamp_us)
                        .saturating_add(i128::from(track.sync_offset_us));
                    if aligned < 0
                        || aligned > i128::from(replay.duration_ms).saturating_mul(1000)
                        || event.sensory_index >= 65_536
                        || !matches!(event.value, 0 | 1)
                    {
                        return Err(FpvRenderError::Invalid(
                            "synchronized replay event has an invalid timestamp, sensory index, or value".to_owned(),
                        ));
                    }
                }
            }
            if event_count == 0 {
                return Err(FpvRenderError::Invalid(
                    "synchronized replay tracks contain no sensory events".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

fn finite_vec(point: Vec3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FpvReplayCapture {
    schema_version: u32,
    snapshot_sha256: String,
    input_sha256: String,
    frame_count: u32,
}

fn run_isolated_replay(
    request: &FpvRenderRequest,
    replay: &FpvReplaySpec,
    snapshot_json: &str,
) -> Result<(FpvReplayCapture, Vec<Vec<AnatomicalId>>), FpvRenderError> {
    let mut input_bytes = serde_json::to_vec(&replay.tracks)
        .map_err(|error| FpvRenderError::Render(error.to_string()))?;
    let input_sha256 = hex::encode(Sha256::digest(&input_bytes));
    input_bytes.clear();
    let snapshot_sha256 = hex::encode(Sha256::digest(snapshot_json.as_bytes()));

    let mut spec = crate::engine::EngineSpec::default();
    spec.lif.dt = replay.step_ms;
    let mut isolated = crate::engine::RunnerEngine::new(spec)
        .map_err(|error| FpvRenderError::Render(format!("isolated engine init failed: {error}")))?;
    isolated
        .import_snapshot_json(snapshot_json)
        .map_err(|error| {
            FpvRenderError::Render(format!("isolated snapshot import failed: {error}"))
        })?;
    let status = isolated.status();
    let visible_ids = request
        .scene
        .nodes
        .iter()
        .map(|node| node.id)
        .collect::<BTreeSet<_>>();

    let mut events = replay
        .tracks
        .iter()
        .flat_map(|track| {
            track.events.iter().map(|event| {
                (
                    (i128::from(event.timestamp_us) + i128::from(track.sync_offset_us)) as u64,
                    event.sensory_index as usize,
                    event.value,
                )
            })
        })
        .collect::<Vec<_>>();
    events.sort_by_key(|event| (event.0, event.1));
    if events
        .iter()
        .any(|(_, index, _)| *index >= status.num_sensory_neurons)
    {
        return Err(FpvRenderError::Invalid(
            "replay sensory index is outside the imported network input layer".to_owned(),
        ));
    }

    let estimated_steps = (replay.duration_ms as f64 / replay.step_ms).ceil() as u64;
    if estimated_steps > FPV_REPLAY_MAX_STEPS {
        return Err(FpvRenderError::Invalid(
            "isolated replay exceeds the supported step budget".to_owned(),
        ));
    }
    let mut active_by_frame = Vec::with_capacity(request.frame_count as usize);
    let mut event_index = 0usize;
    let mut elapsed_ms = 0.0_f64;
    let mut total_steps = 0u64;
    for frame_index in 0..request.frame_count {
        let frame_time_ms = f64::from(frame_index) * 1000.0 / f64::from(request.frame_rate);
        while total_steps == 0
            || (elapsed_ms < frame_time_ms && elapsed_ms < replay.duration_ms as f64)
        {
            if total_steps >= FPV_REPLAY_MAX_STEPS {
                return Err(FpvRenderError::Render(
                    "isolated replay exceeded its step budget".to_owned(),
                ));
            }
            let next_time_ms = elapsed_ms + replay.step_ms;
            let boundary_us = (next_time_ms * 1000.0).ceil() as u64;
            let mut sensory = vec![0_i8; status.num_sensory_neurons];
            while event_index < events.len() && events[event_index].0 <= boundary_us {
                let (_, sensory_index, value) = events[event_index];
                sensory[sensory_index] = value;
                event_index += 1;
            }
            isolated.step(Some(&sensory));
            if let Some(error) = isolated.last_step_error() {
                return Err(FpvRenderError::Render(format!(
                    "isolated replay step failed: {error}"
                )));
            }
            elapsed_ms = next_time_ms;
            total_steps = total_steps.saturating_add(1);
            if elapsed_ms >= replay.duration_ms as f64 {
                break;
            }
            if elapsed_ms >= frame_time_ms {
                break;
            }
        }
        let activity = isolated.activity();
        let mut active_ids = Vec::new();
        let mut add_active = |role, layer, indices: &[usize]| {
            for index in indices {
                let id = crate::engine::display_neuron_id(role, layer, *index);
                if visible_ids.contains(&id) {
                    active_ids.push(id);
                }
            }
        };
        add_active(
            crate::morphology_contract::DisplayRole::Sensory,
            0,
            &activity.sensory,
        );
        for (layer, indices) in activity.hidden.iter().enumerate() {
            add_active(
                crate::morphology_contract::DisplayRole::Hidden,
                layer + 1,
                indices,
            );
        }
        add_active(
            crate::morphology_contract::DisplayRole::Output,
            status.num_hidden_layers + 1,
            &activity.output,
        );
        active_ids.sort();
        active_ids.dedup();
        active_ids.truncate(FPV_REPLAY_ACTIVE_IDS_PER_FRAME);
        active_by_frame.push(active_ids);
    }

    Ok((
        FpvReplayCapture {
            schema_version: 1,
            snapshot_sha256,
            input_sha256,
            frame_count: active_by_frame.len() as u32,
        },
        active_by_frame,
    ))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn safe_job_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn job_dir(root: &Path, job_id: &str) -> Result<PathBuf, FpvRenderError> {
    if !safe_job_id(job_id) {
        return Err(FpvRenderError::Invalid("invalid job ID".to_owned()));
    }
    Ok(root.join(job_id))
}

pub fn submit_job(
    root: &Path,
    owner_id: &str,
    mut request: FpvRenderRequest,
) -> Result<FpvRenderJobStatus, FpvRenderError> {
    request.validate()?;
    let owner_id = owner_id.trim();
    if owner_id.is_empty() || owner_id.len() > 256 {
        return Err(FpvRenderError::Invalid("invalid job owner".to_owned()));
    }
    std::fs::create_dir_all(root).map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let id = loop {
        let serial = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = format!("fpv-{:x}-{:x}", now_ms(), serial);
        let path = job_dir(root, &candidate)?;
        match std::fs::create_dir(&path) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(FpvRenderError::Storage(error.to_string())),
        }
    };
    let path = job_dir(root, &id)?;
    let replay_enabled = if let Some(replay) = request.replay.as_mut() {
        let snapshot = replay.snapshot_json.take().ok_or_else(|| {
            FpvRenderError::Invalid("isolated replay requires a saved network snapshot".to_owned())
        })?;
        atomic_write(&path.join("replay-snapshot.json"), snapshot.as_bytes())
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        true
    } else {
        false
    };
    let status = FpvRenderJobStatus {
        schema_version: FPV_RENDER_JOB_SCHEMA_VERSION,
        job_id: id,
        owner_id: owner_id.to_owned(),
        network_id: request.network_id.clone(),
        state: FpvRenderState::Queued,
        frame_count: request.frame_count,
        completed_frames: 0,
        frame_rate: request.frame_rate,
        width: request.width,
        height: request.height,
        sampled_nodes: request.scene.nodes.len() as u32,
        sampled_edges: request
            .scene
            .edges
            .len()
            .saturating_add(request.scene.paths.len()) as u32,
        projection_complete: request.scene.coverage.complete,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
        error: None,
        video_ready: false,
        replay_enabled,
        replay_frames_total: if replay_enabled {
            request.frame_count
        } else {
            0
        },
        replay_frames_completed: 0,
        visualization_stage_track: visualization_stage_track(&request),
        visualization_policy_version: request.visualization_policy_version,
    };
    write_json_pretty(&path.join("request.json"), &request)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    write_json_pretty(&path.join("status.json"), &status)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    Ok(status)
}

pub fn get_job_status(
    root: &Path,
    job_id: &str,
    owner_id: &str,
) -> Result<FpvRenderJobStatus, FpvRenderError> {
    let path = job_dir(root, job_id)?.join("status.json");
    let status: FpvRenderJobStatus = read_json_if_exists(&path)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Invalid("FPV job not found".to_owned()))?;
    if status.owner_id != owner_id {
        return Err(FpvRenderError::Invalid("FPV job not found".to_owned()));
    }
    Ok(status)
}

pub fn list_jobs(root: &Path, owner_id: &str) -> Result<Vec<FpvRenderJobStatus>, FpvRenderError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(FpvRenderError::Storage(error.to_string())),
    };
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        let Some(job_id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if !safe_job_id(&job_id) || !entry.path().is_dir() {
            continue;
        }
        if let Some(status) =
            read_json_if_exists::<FpvRenderJobStatus>(&entry.path().join("status.json"))
                .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            && status.owner_id == owner_id
        {
            jobs.push(status);
        }
    }
    jobs.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
    jobs.truncate(100);
    Ok(jobs)
}

pub fn cancel_job(
    root: &Path,
    job_id: &str,
    owner_id: &str,
) -> Result<FpvRenderJobStatus, FpvRenderError> {
    let path = job_dir(root, job_id)?;
    let _lease = try_acquire_lease(&path.join("status.lock"))
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Storage("job status is busy".to_owned()))?;
    let mut status: FpvRenderJobStatus = read_json_if_exists(&path.join("status.json"))
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Invalid("FPV job not found".to_owned()))?;
    if status.owner_id != owner_id {
        return Err(FpvRenderError::Invalid("FPV job not found".to_owned()));
    }
    if !status.state.terminal() {
        status.state = FpvRenderState::Cancelled;
        status.updated_at_ms = now_ms();
        write_json_pretty(&path.join("status.json"), &status)
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    }
    Ok(status)
}

pub fn retry_job(
    root: &Path,
    job_id: &str,
    owner_id: &str,
) -> Result<FpvRenderJobStatus, FpvRenderError> {
    let path = job_dir(root, job_id)?;
    let _lease = try_acquire_lease(&path.join("status.lock"))
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Storage("job status is busy".to_owned()))?;
    let mut status: FpvRenderJobStatus = read_json_if_exists(&path.join("status.json"))
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Invalid("FPV job not found".to_owned()))?;
    if status.owner_id != owner_id {
        return Err(FpvRenderError::Invalid("FPV job not found".to_owned()));
    }
    if status.state != FpvRenderState::Failed {
        return Err(FpvRenderError::Invalid(
            "only failed FPV jobs can be retried".to_owned(),
        ));
    }
    let _ = std::fs::remove_file(path.join("video.partial.mp4"));
    let _ = std::fs::remove_file(path.join("video.mp4"));
    status.state = FpvRenderState::Queued;
    status.video_ready = false;
    status.error = None;
    status.updated_at_ms = now_ms();
    write_json_pretty(&path.join("status.json"), &status)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    Ok(status)
}

pub fn video_path(root: &Path, job_id: &str, owner_id: &str) -> Result<PathBuf, FpvRenderError> {
    let path = job_dir(root, job_id)?;
    let status: FpvRenderJobStatus = read_json_if_exists(&path.join("status.json"))
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Invalid("FPV job not found".to_owned()))?;
    if status.owner_id != owner_id || !status.video_ready {
        return Err(FpvRenderError::Invalid("FPV video not found".to_owned()));
    }
    let video = path.join("video.mp4");
    if !video.is_file() {
        return Err(FpvRenderError::Storage(
            "completed FPV video is missing".to_owned(),
        ));
    }
    Ok(video)
}

/// Run one worker loop. Multiple web replicas can call this against a shared
/// root; frame leases divide work without making the UI connection the owner.
pub async fn worker_loop(root: PathBuf) {
    loop {
        if let Err(error) = process_available_frames(&root).await {
            eprintln!("FPV worker iteration failed: {error}");
        }
        tokio::time::sleep(WORKER_POLL_INTERVAL).await;
    }
}

async fn process_available_frames(root: &Path) -> Result<(), FpvRenderError> {
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(FpvRenderError::Storage(error.to_string())),
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
    {
        let Some(job_id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if !safe_job_id(&job_id)
            || !entry
                .file_type()
                .await
                .map(|kind| kind.is_dir())
                .unwrap_or(false)
        {
            continue;
        }
        if let Err(error) = process_one_frame(root, &job_id).await {
            mark_job_failed(root, &job_id, &error).await?;
            eprintln!("FPV render job {job_id} failed: {error}");
        }
    }
    Ok(())
}

async fn process_one_frame(root: &Path, job_id: &str) -> Result<(), FpvRenderError> {
    let directory = job_dir(root, job_id)?;
    let status_path = directory.join("status.json");
    let Some(status) = read_status(&status_path).await? else {
        return Ok(());
    };
    if status.state.terminal() {
        return Ok(());
    }
    let request_path = directory.join("request.json");
    let request_bytes = tokio::fs::read(&request_path)
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let request: FpvRenderRequest = serde_json::from_slice(&request_bytes)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    request.validate_stored_manifest()?;
    if request.replay.is_some()
        && ensure_replay_capture(&directory, &status_path, &request)
            .await?
            .is_none()
    {
        return Ok(());
    }
    let existing_frames = existing_frame_indices(&directory).await?;

    for frame_index in 0..request.frame_count {
        if read_status(&status_path)
            .await?
            .is_some_and(|current| current.state == FpvRenderState::Cancelled)
        {
            return Ok(());
        }
        let frame_path = directory.join(format!("frame-{frame_index:08}.ppm"));
        if existing_frames.contains(&frame_index) {
            continue;
        }
        let lease_path = directory.join(format!("frame-{frame_index:08}.lock"));
        let lease_path_for_task = lease_path.clone();
        let lease = tokio::task::spawn_blocking(move || try_acquire_lease(&lease_path_for_task))
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        let Some(lease) = lease else { continue };
        if tokio::fs::try_exists(&frame_path)
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        {
            drop(lease);
            continue;
        }
        let mut render_request = request.clone();
        if request.replay.is_some() {
            let active_path = replay_active_frame_path(&directory, frame_index);
            let active_bytes = tokio::fs::read(&active_path)
                .await
                .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
            let frame_active: Vec<AnatomicalId> = serde_json::from_slice(&active_bytes)
                .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
            let mut active = render_request
                .active_node_ids
                .into_iter()
                .collect::<BTreeSet<_>>();
            active.extend(frame_active);
            render_request.active_node_ids = active.into_iter().collect();
        }
        let ppm =
            tokio::task::spawn_blocking(move || render_frame_ppm(&render_request, frame_index))
                .await
                .map_err(|error| FpvRenderError::Render(error.to_string()))??;
        let frame_path_for_write = frame_path.clone();
        tokio::task::spawn_blocking(move || atomic_write(&frame_path_for_write, &ppm))
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        drop(lease);
        update_progress(&directory, &status_path, frame_index + 1).await?;
        return Ok(());
    }
    encode_when_ready(&directory, &status_path, &request).await
}

fn replay_active_frame_path(directory: &Path, frame_index: u32) -> PathBuf {
    directory
        .join("replay-active")
        .join(format!("frame-{frame_index:08}.json"))
}

async fn ensure_replay_capture(
    directory: &Path,
    status_path: &Path,
    request: &FpvRenderRequest,
) -> Result<Option<FpvReplayCapture>, FpvRenderError> {
    let Some(replay) = request.replay.as_ref() else {
        return Ok(None);
    };
    let capture_path = directory.join("replay-capture.json");
    if let Some(capture) = read_json_if_exists::<FpvReplayCapture>(&capture_path)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        && capture.schema_version == 1
        && capture.frame_count == request.frame_count
    {
        return Ok(Some(capture));
    }
    let lease_path = directory.join("replay.lock");
    let lease = tokio::task::spawn_blocking(move || try_acquire_lease(&lease_path))
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let Some(_lease) = lease else { return Ok(None) };
    if let Some(capture) = read_json_if_exists::<FpvReplayCapture>(&capture_path)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        && capture.schema_version == 1
        && capture.frame_count == request.frame_count
    {
        return Ok(Some(capture));
    }

    update_replay_status(status_path, request.frame_count, false).await?;
    let snapshot_path = directory.join("replay-snapshot.json");
    let snapshot_bytes = tokio::fs::read(&snapshot_path)
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    if snapshot_bytes.len() > FPV_REPLAY_MAX_SNAPSHOT_BYTES {
        return Err(FpvRenderError::Invalid(
            "isolated replay snapshot exceeds the 512 MiB transfer limit".to_owned(),
        ));
    }
    let snapshot_json = String::from_utf8(snapshot_bytes)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let request = request.clone();
    let replay = replay.clone();
    let replay_frame_count = request.frame_count;
    let snapshot_json_for_task = snapshot_json.clone();
    let replay_capture = tokio::task::spawn_blocking(move || {
        run_isolated_replay(&request, &replay, &snapshot_json_for_task)
    })
    .await
    .map_err(|error| FpvRenderError::Render(error.to_string()))??;
    let active_dir = directory.join("replay-active");
    tokio::fs::create_dir_all(&active_dir)
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let (capture, active_by_frame) = replay_capture;
    for (frame_index, active_ids) in active_by_frame.iter().enumerate() {
        let path = replay_active_frame_path(directory, frame_index as u32);
        let bytes = serde_json::to_vec(active_ids)
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        let path_for_task = path.clone();
        tokio::task::spawn_blocking(move || atomic_write(&path_for_task, &bytes))
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    }
    let capture_for_task = capture.clone();
    let capture_path_for_task = capture_path.clone();
    tokio::task::spawn_blocking(move || {
        write_json_pretty(&capture_path_for_task, &capture_for_task)
    })
    .await
    .map_err(|error| FpvRenderError::Storage(error.to_string()))?
    .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    update_replay_status(status_path, replay_frame_count, true).await?;
    Ok(Some(capture))
}

async fn update_replay_status(
    status_path: &Path,
    frame_count: u32,
    completed: bool,
) -> Result<(), FpvRenderError> {
    let status_path = status_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let lock_path = status_path.with_file_name("status.lock");
        let Some(_lease) = try_acquire_lease(&lock_path)
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        else {
            return Ok(());
        };
        let mut status: FpvRenderJobStatus = read_json_if_exists(&status_path)
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            .ok_or_else(|| FpvRenderError::Storage("job status disappeared".to_owned()))?;
        if status.state.terminal() {
            return Ok(());
        }
        status.state = if completed {
            FpvRenderState::Queued
        } else {
            FpvRenderState::Replaying
        };
        status.replay_frames_total = frame_count;
        status.replay_frames_completed = if completed { frame_count } else { 0 };
        status.updated_at_ms = now_ms();
        write_json_pretty(&status_path, &status)
            .map_err(|error| FpvRenderError::Storage(error.to_string()))
    })
    .await
    .map_err(|error| FpvRenderError::Storage(error.to_string()))?
}

async fn read_status(path: &Path) -> Result<Option<FpvRenderJobStatus>, FpvRenderError> {
    tokio::fs::read(path)
        .await
        .map(|bytes| serde_json::from_slice(&bytes).ok())
        .or_else(|error| {
            if error.kind() == ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(FpvRenderError::Storage(error.to_string()))
            }
        })
}

async fn existing_frame_indices(directory: &Path) -> Result<BTreeSet<u32>, FpvRenderError> {
    let mut entries = tokio::fs::read_dir(directory)
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let mut indices = BTreeSet::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(index) = name
            .strip_prefix("frame-")
            .and_then(|value| value.strip_suffix(".ppm"))
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        indices.insert(index);
    }
    Ok(indices)
}

async fn update_progress(
    directory: &Path,
    status_path: &Path,
    _rendered_index: u32,
) -> Result<(), FpvRenderError> {
    let directory = directory.to_path_buf();
    let lock_path = directory.join("status.lock");
    let lease = tokio::task::spawn_blocking(move || try_acquire_lease(&lock_path))
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let Some(_lease) = lease else { return Ok(()) };
    let status_path = status_path.to_path_buf();
    let mut status: FpvRenderJobStatus = tokio::fs::read(&status_path)
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|error| FpvRenderError::Storage(error.to_string()))
        })?;
    if status.state.terminal() {
        return Ok(());
    }
    status.state = FpvRenderState::Rendering;
    status.completed_frames = status
        .completed_frames
        .saturating_add(1)
        .min(status.frame_count);
    status.updated_at_ms = now_ms();
    write_json_pretty(&status_path, &status)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))
}

async fn encode_when_ready(
    directory: &Path,
    status_path: &Path,
    request: &FpvRenderRequest,
) -> Result<(), FpvRenderError> {
    let completed = tokio::fs::read_dir(directory)
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let mut completed = completed;
    let mut frames = 0u32;
    while let Some(entry) = completed
        .next_entry()
        .await
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
    {
        if entry.file_name().to_string_lossy().starts_with("frame-")
            && entry.file_name().to_string_lossy().ends_with(".ppm")
        {
            frames = frames.saturating_add(1);
        }
    }
    if frames < request.frame_count {
        return Ok(());
    }
    let directory_for_lock = directory.to_path_buf();
    let lease = tokio::task::spawn_blocking(move || {
        try_acquire_lease(&directory_for_lock.join("encoding.lock"))
    })
    .await
    .map_err(|error| FpvRenderError::Storage(error.to_string()))?
    .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let Some(_lease) = lease else { return Ok(()) };
    let status_path_owned = status_path.to_path_buf();
    let status_dir = directory.to_path_buf();
    let status_lease =
        tokio::task::spawn_blocking(move || try_acquire_lease(&status_dir.join("status.lock")))
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let Some(status_lease) = status_lease else {
        return Ok(());
    };
    let mut status: FpvRenderJobStatus = read_json_if_exists(&status_path_owned)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Storage("job status disappeared".to_owned()))?;
    if status.state.terminal() || status.video_ready {
        return Ok(());
    }
    status.state = FpvRenderState::Encoding;
    status.completed_frames = status.frame_count;
    status.updated_at_ms = now_ms();
    write_json_pretty(&status_path_owned, &status)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    drop(status_lease);

    let directory = directory.to_path_buf();
    let final_video_path = directory.join("video.mp4");
    let encoder_directory = directory.clone();
    let frame_rate = request.frame_rate;
    let frame_count = request.frame_count;
    let encoder = tokio::task::spawn_blocking(move || {
        encode_video(&encoder_directory, frame_rate, frame_count)
    })
    .await
    .map_err(|error| FpvRenderError::Encode(error.to_string()))?;
    let status_dir = directory.clone();
    let status_lease =
        tokio::task::spawn_blocking(move || try_acquire_lease(&status_dir.join("status.lock")))
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?
            .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    let Some(_status_lease) = status_lease else {
        return Ok(());
    };
    let final_status_path = status_path.to_path_buf();
    let mut final_status: FpvRenderJobStatus = read_json_if_exists(&final_status_path)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?
        .ok_or_else(|| FpvRenderError::Storage("job status disappeared".to_owned()))?;
    if final_status.state == FpvRenderState::Cancelled {
        let _ = std::fs::remove_file(final_video_path);
        let _ = std::fs::remove_file(directory.join("video.partial.mp4"));
    } else {
        match encoder {
            Ok(()) => {
                final_status.state = FpvRenderState::Complete;
                final_status.video_ready = true;
                final_status.error = None;
            }
            Err(error) => {
                final_status.state = FpvRenderState::Failed;
                final_status.error = Some(error.to_string());
            }
        }
    }
    final_status.updated_at_ms = now_ms();
    write_json_pretty(&final_status_path, &final_status)
        .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
    if final_status.state == FpvRenderState::Complete {
        let directory = directory.clone();
        tokio::task::spawn_blocking(move || remove_frame_intermediates(&directory))
            .await
            .map_err(|error| FpvRenderError::Storage(error.to_string()))??;
    }
    Ok(())
}

fn remove_frame_intermediates(directory: &Path) -> Result<(), FpvRenderError> {
    for entry in
        std::fs::read_dir(directory).map_err(|error| FpvRenderError::Storage(error.to_string()))?
    {
        let entry = entry.map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("frame-") && (name.ends_with(".ppm") || name.ends_with(".lock")) {
            std::fs::remove_file(entry.path())
                .map_err(|error| FpvRenderError::Storage(error.to_string()))?;
        }
    }
    Ok(())
}

async fn mark_job_failed(
    root: &Path,
    job_id: &str,
    error: &FpvRenderError,
) -> Result<(), FpvRenderError> {
    let directory = job_dir(root, job_id)?;
    let directory_for_lock = directory.clone();
    let lease = tokio::task::spawn_blocking(move || {
        try_acquire_lease(&directory_for_lock.join("status.lock"))
    })
    .await
    .map_err(|join| FpvRenderError::Storage(join.to_string()))?
    .map_err(|storage| FpvRenderError::Storage(storage.to_string()))?;
    let Some(_lease) = lease else { return Ok(()) };
    let status_path = directory.join("status.json");
    let Some(mut status) = read_status(&status_path).await? else {
        return Ok(());
    };
    if status.state.terminal() {
        return Ok(());
    }
    status.state = FpvRenderState::Failed;
    status.error = Some(error.to_string());
    status.updated_at_ms = now_ms();
    let status_path = status_path.clone();
    tokio::task::spawn_blocking(move || write_json_pretty(&status_path, &status))
        .await
        .map_err(|join| FpvRenderError::Storage(join.to_string()))?
        .map_err(|storage| FpvRenderError::Storage(storage.to_string()))
}

fn encode_video(directory: &Path, frame_rate: u32, frame_count: u32) -> Result<(), FpvRenderError> {
    let pattern = directory.join("frame-%08d.ppm");
    let temporary_video = directory.join("video.partial.mp4");
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-framerate")
        .arg(frame_rate.to_string())
        .arg("-i")
        .arg(pattern)
        .arg("-frames:v")
        .arg(frame_count.to_string())
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("fast")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-threads")
        .arg("2")
        .arg("-movflags")
        .arg("+faststart")
        .arg(&temporary_video)
        .output()
        .map_err(|error| {
            FpvRenderError::Encode(format!(
                "could not start ffmpeg; the web-ui image needs the ffmpeg runtime: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(FpvRenderError::Encode(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    std::fs::rename(&temporary_video, directory.join("video.mp4"))
        .map_err(|error| FpvRenderError::Encode(error.to_string()))?;
    Ok(())
}

#[derive(Clone, Copy)]
struct ProjectedPoint {
    x: f64,
    y: f64,
    depth: f64,
}

fn visualization_stage_track(request: &FpvRenderRequest) -> Vec<u8> {
    (0..request.frame_count)
        .map(|frame| visualization_for_frame(request, frame).0)
        .collect()
}

/// Resolve a route frame solely from the immutable request. Actual worker
/// timing is deliberately excluded, so parallel claims render the same image.
fn visualization_for_frame(request: &FpvRenderRequest, frame_index: u32) -> (u8, f64) {
    let highest = highest_supported_stage(None, Some(&request.scene))
        .unwrap_or(VisualizationStage::SyntheticPixels);
    let route_length = request.waypoint_ids.len().max(2);
    let progress = if request.frame_count <= 1 {
        0.0
    } else {
        f64::from(frame_index.min(request.frame_count - 1)) / f64::from(request.frame_count - 1)
    };
    let route_position = progress * (route_length - 1) as f64;
    let index = (route_position.floor() as usize).min(route_length - 2);
    let local = route_position - index as f64;
    let resolve = |keyframe: Option<&FpvVisualizationKeyframe>| {
        let Some(keyframe) = keyframe else {
            return (VisualizationStage::AnatomicalContacts, request.zoom);
        };
        let requested = VisualizationStage::from_number(keyframe.stage)
            .unwrap_or(VisualizationStage::AnatomicalContacts);
        let stage = if keyframe.automatic {
            automatic_target(
                keyframe.zoom,
                request.auto_visualization_latency_ms,
                highest,
            )
        } else {
            requested.min(highest)
        };
        (stage, keyframe.zoom)
    };
    let (left_stage, left_zoom) = resolve(request.visualization_keyframes.get(index));
    let (right_stage, right_zoom) = resolve(request.visualization_keyframes.get(index + 1));
    let interpolated = f64::from(left_stage.number())
        + (f64::from(right_stage.number()) - f64::from(left_stage.number())) * local;
    let stage = (interpolated.round() as u8).clamp(1, highest.number());
    let zoom = (left_zoom + (right_zoom - left_zoom) * local).clamp(0.2, 4.0);
    (stage, zoom)
}

fn synthetic_column_positions(scene: &DisplaySnapshot) -> BTreeMap<AnatomicalId, Vec3> {
    let hidden_layers = scene
        .nodes
        .iter()
        .filter(|node| node.role == DisplayRole::Hidden)
        .filter_map(|node| node.layer)
        .max()
        .map_or(0, |layer| layer + 1);
    let output_column = hidden_layers + 1;
    let columns = output_column + 1;
    let column_for = |node: &crate::morphology_contract::DisplayNode| match node.role {
        DisplayRole::Sensory => 0,
        DisplayRole::Hidden => node.layer.unwrap_or(0).min(hidden_layers.saturating_sub(1)) + 1,
        DisplayRole::Output | DisplayRole::Unassigned => output_column,
    };
    let mut rank = BTreeMap::<usize, usize>::new();
    let mut counts = BTreeMap::<usize, usize>::new();
    for node in &scene.nodes {
        *counts.entry(column_for(node)).or_default() += 1;
    }
    scene
        .nodes
        .iter()
        .map(|node| {
            let column = column_for(node);
            let row = rank.entry(column).or_default();
            let centred = *row as f64 - (counts[&column].saturating_sub(1) as f64 * 0.5);
            *row += 1;
            (
                node.id,
                Vec3 {
                    x: column as f64 - (columns.saturating_sub(1) as f64 * 0.5),
                    y: -centred * 0.04,
                    z: 0.0,
                },
            )
        })
        .collect()
}

fn render_frame_ppm(
    request: &FpvRenderRequest,
    frame_index: u32,
) -> Result<Vec<u8>, FpvRenderError> {
    let width = request.width as usize;
    let height = request.height as usize;
    let (visualization_stage, camera_zoom) = visualization_for_frame(request, frame_index);
    let pixels = width
        .checked_mul(height)
        .and_then(|count| count.checked_mul(3))
        .ok_or_else(|| FpvRenderError::Render("frame size overflow".to_owned()))?;
    let mut rgb = Vec::new();
    rgb.try_reserve_exact(pixels)
        .map_err(|error| FpvRenderError::Render(format!("frame allocation failed: {error}")))?;
    rgb.resize(pixels, 8);

    let node_positions = if visualization_stage <= 3 {
        synthetic_column_positions(&request.scene)
    } else {
        request
            .scene
            .nodes
            .iter()
            .map(|node| (node.id, node.position_mm))
            .collect::<BTreeMap<_, _>>()
    };
    let waypoints = request
        .waypoint_ids
        .iter()
        .filter_map(|id| node_positions.get(id).copied())
        .collect::<Vec<_>>();
    let target = route_target(&waypoints, frame_index, request.frame_count);
    let active = request
        .active_node_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let active_target = active
        .iter()
        .filter_map(|id| node_positions.get(id).copied())
        .reduce(|sum, point| add(sum, point))
        .map(|sum| scale(sum, 1.0 / active.len().max(1) as f64));
    let target = if request.focus_active_regions {
        active_target.map_or(target, |focus| mix(target, focus, 0.55))
    } else {
        target
    };
    let route_next = route_target(
        &waypoints,
        frame_index
            .saturating_add(1)
            .min(request.frame_count.saturating_sub(1)),
        request.frame_count,
    );
    let mut forward = normalize(sub(route_next, target));
    if length(forward) < 1.0e-9 {
        forward = normalize(sub(*waypoints.last().unwrap(), *waypoints.first().unwrap()));
    }
    let mut world_up = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    let mut right = normalize(cross(forward, world_up));
    if length(right) < 1.0e-9 {
        world_up = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        right = normalize(cross(forward, world_up));
    }
    let up = normalize(cross(right, forward));
    let mut extent = 1.0_f64;
    for position in node_positions.values() {
        extent = extent.max(length(sub(*position, target)));
    }
    let distance = (extent / camera_zoom).max(0.05);
    let focal = height as f64 * 0.72;
    let camera = add(
        sub(target, scale(forward, distance)),
        scale(up, distance * 0.14),
    );
    let project = |point: Vec3| -> Option<ProjectedPoint> {
        let relative = sub(point, camera);
        let depth = dot(relative, forward);
        if !depth.is_finite() || depth <= distance * 0.025 {
            return None;
        }
        let x = width as f64 * 0.5 + dot(relative, right) * focal / depth;
        let y = height as f64 * 0.5 - dot(relative, up) * focal / depth;
        (x.is_finite() && y.is_finite()).then_some(ProjectedPoint { x, y, depth })
    };
    let projected = node_positions
        .iter()
        .filter_map(|(id, point)| project(*point).map(|screen| (*id, screen)))
        .collect::<BTreeMap<_, _>>();
    let display_nodes = request
        .scene
        .nodes
        .iter()
        .map(|node| (node.id, node))
        .collect::<BTreeMap<_, _>>();

    if (2..=6).contains(&visualization_stage) {
        let mut used_sources = BTreeSet::new();
        let mut used_targets = BTreeSet::new();
        for edge in &request.scene.edges {
            if !node_positions.contains_key(&edge.source)
                || !node_positions.contains_key(&edge.target)
            {
                continue;
            }
            if visualization_stage == 5
                && (!used_sources.insert(edge.source) || !used_targets.insert(edge.target))
            {
                continue;
            }
            let (Some(from), Some(to)) = (
                project(node_positions[&edge.source]),
                project(node_positions[&edge.target]),
            ) else {
                continue;
            };
            draw_line(&mut rgb, width, height, from, to, [28, 67, 90], 1);
        }
    }
    if visualization_stage >= 7 && request.scene.coverage.volumetric_clearance_verified {
        for path in &request.scene.paths {
            for segment in path.points_mm.windows(2) {
                if let (Some(from), Some(to)) = (project(segment[0]), project(segment[1])) {
                    let stroke = if visualization_stage >= 8 {
                        let depth = (from.depth + to.depth) * 0.5;
                        2.0 * path.radius_mm * focal / depth.max(1.0e-9)
                    } else {
                        1.0
                    };
                    // Keep perspective-scaled physical diameters inside the
                    // frame budget; centre-line positions remain the exact
                    // committed 3D samples in either case.
                    let bounded_stroke =
                        stroke.round().clamp(1.0, width.min(height).max(1) as f64) as i32;
                    draw_line(
                        &mut rgb,
                        width,
                        height,
                        from,
                        to,
                        [24, 59, 76],
                        bounded_stroke,
                    );
                }
            }
        }
    }

    let mut ordered_nodes = projected.into_iter().collect::<Vec<_>>();
    ordered_nodes.sort_by(|left, right| right.1.depth.total_cmp(&left.1.depth));
    for (id, point) in ordered_nodes {
        let is_active = active.contains(&id);
        let colour = if is_active {
            [255, 255, 255]
        } else {
            [45, 92, 118]
        };
        let radius = if visualization_stage == 3 {
            1
        } else if visualization_stage >= 8 && request.scene.coverage.volumetric_clearance_verified {
            display_nodes
                .get(&id)
                .and_then(|node| node.soma_radius_mm)
                .map(|radius_mm| {
                    (radius_mm * focal / point.depth.max(1.0e-9))
                        .round()
                        .clamp(0.0, width.min(height).max(1) as f64) as i32
                })
                .unwrap_or(0)
        } else {
            0
        };
        draw_disc(
            &mut rgb,
            width,
            height,
            point.x.round() as i32,
            point.y.round() as i32,
            radius,
            colour,
        );
    }
    if visualization_stage >= 9 && request.scene.coverage.volumetric_clearance_verified {
        for marker in &request.scene.markers {
            if let Some(point) = project(marker.position_mm) {
                draw_disc(
                    &mut rgb,
                    width,
                    height,
                    point.x.round() as i32,
                    point.y.round() as i32,
                    2,
                    [255, 220, 130],
                );
            }
        }
    }
    let header = format!("P6\n{} {}\n255\n", request.width, request.height);
    let mut ppm = Vec::new();
    ppm.try_reserve_exact(header.len().saturating_add(rgb.len()))
        .map_err(|error| {
            FpvRenderError::Render(format!("frame output allocation failed: {error}"))
        })?;
    ppm.extend_from_slice(header.as_bytes());
    ppm.extend_from_slice(&rgb);
    Ok(ppm)
}

fn route_target(points: &[Vec3], frame: u32, frames: u32) -> Vec3 {
    if points.len() == 1 {
        return points[0];
    }
    let t = if frames <= 1 {
        0.0
    } else {
        f64::from(frame.min(frames - 1)) / f64::from(frames - 1)
    };
    let route_t = t * (points.len() - 1) as f64;
    let segment = (route_t.floor() as usize).min(points.len() - 2);
    mix(
        points[segment],
        points[segment + 1],
        route_t - segment as f64,
    )
}

fn draw_line(
    rgb: &mut [u8],
    width: usize,
    height: usize,
    from: ProjectedPoint,
    to: ProjectedPoint,
    colour: [u8; 3],
    stroke_width: i32,
) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let steps = dx.abs().max(dy.abs()).ceil().min((width + height) as f64) as usize;
    if steps == 0 {
        return;
    }
    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        draw_disc(
            rgb,
            width,
            height,
            (from.x + dx * t).round() as i32,
            (from.y + dy * t).round() as i32,
            stroke_width.saturating_sub(1) / 2,
            colour,
        );
    }
}

fn draw_disc(
    rgb: &mut [u8],
    width: usize,
    height: usize,
    cx: i32,
    cy: i32,
    radius: i32,
    colour: [u8; 3],
) {
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= radius * radius {
                put_pixel(rgb, width, height, cx + x, cy + y, colour);
            }
        }
    }
}

fn put_pixel(rgb: &mut [u8], width: usize, height: usize, x: i32, y: i32, colour: [u8; 3]) {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
        return;
    }
    let offset = (y as usize * width + x as usize) * 3;
    rgb[offset..offset + 3].copy_from_slice(&colour);
}

fn add(left: Vec3, right: Vec3) -> Vec3 {
    Vec3 {
        x: left.x + right.x,
        y: left.y + right.y,
        z: left.z + right.z,
    }
}
fn sub(left: Vec3, right: Vec3) -> Vec3 {
    Vec3 {
        x: left.x - right.x,
        y: left.y - right.y,
        z: left.z - right.z,
    }
}
fn scale(point: Vec3, value: f64) -> Vec3 {
    Vec3 {
        x: point.x * value,
        y: point.y * value,
        z: point.z * value,
    }
}
fn mix(left: Vec3, right: Vec3, t: f64) -> Vec3 {
    add(scale(left, 1.0 - t), scale(right, t))
}
fn dot(left: Vec3, right: Vec3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}
fn cross(left: Vec3, right: Vec3) -> Vec3 {
    Vec3 {
        x: left.y * right.z - left.z * right.y,
        y: left.z * right.x - left.x * right.z,
        z: left.x * right.y - left.y * right.x,
    }
}
fn length(point: Vec3) -> f64 {
    dot(point, point).sqrt()
}
fn normalize(point: Vec3) -> Vec3 {
    let norm = length(point);
    if norm <= 1.0e-12 || !norm.is_finite() {
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    } else {
        scale(point, 1.0 / norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deterministic::SchemaVersion;
    use crate::morphology_contract::{
        AnatomicalKind, DisplayCoverage, DisplayEdge, DisplayMembrane, DisplayMode, DisplayNode,
        DisplayProvenance, DisplayRole,
    };

    fn scene() -> DisplaySnapshot {
        let first = AnatomicalId::new(1, 1).unwrap();
        let second = AnatomicalId::new(2, 1).unwrap();
        DisplaySnapshot {
            schema_version: SchemaVersion::new(DisplaySnapshot::SCHEMA_VERSION).unwrap(),
            morphology_revision: 1,
            topology_epoch: 1,
            route_epoch: 1,
            sequence: 1,
            mode: DisplayMode::Anatomical,
            provenance: DisplayProvenance::ObservedAnatomy,
            coverage: DisplayCoverage {
                complete: true,
                truncated: false,
                volumetric_clearance_verified: false,
                contact_set_verified: false,
                unavailable_reason: None,
                region: None,
                membrane: Some(DisplayMembrane {
                    centre_mm: Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    radii_mm: Vec3 {
                        x: 10.0,
                        y: 10.0,
                        z: 10.0,
                    },
                }),
            },
            nodes: vec![
                DisplayNode {
                    id: first,
                    role: DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: Vec3 {
                        x: -1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: AnatomicalKind::Soma,
                    soma_radius_mm: Some(0.05),
                    colour_slot: 0,
                },
                DisplayNode {
                    id: second,
                    role: DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: AnatomicalKind::Soma,
                    soma_radius_mm: Some(0.05),
                    colour_slot: 1,
                },
            ],
            edges: vec![DisplayEdge {
                source: first,
                target: second,
                points_mm: vec![
                    Vec3 {
                        x: -1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                ],
                multiplicity: 1,
                kind: "axon".to_owned(),
            }],
            paths: vec![],
            markers: vec![],
        }
    }

    fn request() -> FpvRenderRequest {
        let scene = scene();
        FpvRenderRequest {
            schema_version: FPV_RENDER_JOB_SCHEMA_VERSION,
            network_id: "test-network".to_owned(),
            waypoint_ids: scene.nodes.iter().map(|node| node.id).collect(),
            active_node_ids: vec![scene.nodes[1].id],
            scene,
            width: 320,
            height: 180,
            frame_rate: 24,
            frame_count: 3,
            zoom: 1.0,
            focus_active_regions: true,
            visualization_keyframes: Vec::new(),
            visualization_policy_version: VISUALIZATION_POLICY_VERSION,
            auto_visualization_latency_ms: 16.0,
            replay: None,
        }
    }

    #[test]
    fn frame_renderer_is_deterministic_and_writes_one_bounded_ppm() {
        let request = request();
        request.validate().unwrap();
        let first = render_frame_ppm(&request, 0).unwrap();
        assert_eq!(first, render_frame_ppm(&request, 0).unwrap());
        assert!(first.starts_with(b"P6\n320 180\n255\n"));
        assert_eq!(first.len(), b"P6\n320 180\n255\n".len() + 320 * 180 * 3);
    }

    #[test]
    fn waypoint_stages_interpolate_deterministically_and_clamp_to_snapshot_geometry() {
        let mut request = request();
        request.visualization_keyframes = request
            .waypoint_ids
            .iter()
            .enumerate()
            .map(|(index, waypoint_id)| FpvVisualizationKeyframe {
                waypoint_id: *waypoint_id,
                automatic: false,
                stage: if index == 0 { 1 } else { 9 },
                zoom: 1.0,
            })
            .collect();
        assert_eq!(visualization_stage_track(&request), vec![1, 4, 6]);

        // The stage-eight/nine ceiling is available only when the immutable
        // scene carries physical radii and a producer clearance witness.
        request.scene.coverage.volumetric_clearance_verified = true;
        request.scene.coverage.contact_set_verified = true;
        request
            .scene
            .paths
            .push(crate::morphology_contract::DisplayPath {
                id: AnatomicalId::new(3, 1).unwrap(),
                owner: request.scene.nodes[0].id,
                kind: crate::morphology_contract::AnatomicalKind::Axon,
                points_mm: vec![
                    Vec3 {
                        x: -1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 0.0,
                        y: 0.25,
                        z: 0.0,
                    },
                    Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                ],
                radius_mm: 0.02,
            });
        request
            .scene
            .markers
            .push(crate::morphology_contract::DisplayMarker {
                id: AnatomicalId::new(4, 1).unwrap(),
                owner: request.scene.nodes[0].id,
                kind: crate::morphology_contract::AnatomicalKind::Synapse,
                position_mm: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                synapse_id: None,
            });
        request.visualization_keyframes[1].stage = 9;
        assert_eq!(visualization_stage_track(&request), vec![1, 5, 9]);
    }

    #[test]
    fn automatic_waypoints_use_captured_zoom_and_latency_not_worker_timing() {
        let mut request = request();
        request.visualization_keyframes = request
            .waypoint_ids
            .iter()
            .map(|waypoint_id| FpvVisualizationKeyframe {
                waypoint_id: *waypoint_id,
                automatic: true,
                stage: 9,
                zoom: 4.0,
            })
            .collect();
        request.auto_visualization_latency_ms = 90.0;
        assert_eq!(visualization_stage_track(&request), vec![1, 1, 1]);
    }

    #[test]
    fn stage_filtered_frame_renders_hide_connections_at_the_simplest_level() {
        let mut request = request();
        request.visualization_keyframes = request
            .waypoint_ids
            .iter()
            .map(|waypoint_id| FpvVisualizationKeyframe {
                waypoint_id: *waypoint_id,
                automatic: false,
                stage: 1,
                zoom: 1.0,
            })
            .collect();
        let simple = render_frame_ppm(&request, 1).unwrap();
        request
            .visualization_keyframes
            .iter_mut()
            .for_each(|keyframe| keyframe.stage = 6);
        let connected = render_frame_ppm(&request, 1).unwrap();
        assert_ne!(
            simple, connected,
            "stage 1 must omit graph connections drawn at stage 6"
        );
    }

    #[test]
    fn request_rejects_unbounded_projection_and_foreign_waypoints() {
        let mut request = request();
        request.schema_version = FPV_RENDER_JOB_SCHEMA_VERSION + 1;
        assert!(request.validate().is_err());
        request.schema_version = FPV_RENDER_JOB_SCHEMA_VERSION;
        request.waypoint_ids[0] = AnatomicalId::new(999, 1).unwrap();
        assert!(request.validate().is_err());
        request.waypoint_ids[0] = request.scene.nodes[0].id;
        request.frame_count = FPV_RENDER_MAX_FRAMES + 1;
        assert!(request.validate().is_err());
    }

    #[test]
    fn visualization_policy_version_is_captured_and_unknown_versions_fail_closed() {
        let mut invalid_request = request();
        invalid_request.visualization_policy_version += 1;
        assert!(matches!(
            invalid_request.validate(),
            Err(FpvRenderError::Invalid(message)) if message.contains("unsupported visualisation policy version")
        ));

        let mut stored = serde_json::to_value(request()).unwrap();
        stored
            .as_object_mut()
            .unwrap()
            .remove("visualization_policy_version");
        let restored: FpvRenderRequest = serde_json::from_value(stored).unwrap();
        assert_eq!(
            restored.visualization_policy_version,
            VISUALIZATION_POLICY_VERSION
        );
    }

    #[test]
    fn synchronized_tracks_replay_on_an_isolated_network_copy() {
        let mut spec = crate::engine::EngineSpec::default();
        spec.net.num_sensory_neurons = 2;
        let source = crate::engine::RunnerEngine::new(spec).expect("source engine");
        let live_before = source.export_snapshot_json().expect("live snapshot");
        let status = source.status();
        let sensory_a = crate::engine::display_neuron_id(
            crate::morphology_contract::DisplayRole::Sensory,
            0,
            0,
        );
        let sensory_b = crate::engine::display_neuron_id(
            crate::morphology_contract::DisplayRole::Sensory,
            0,
            1,
        );
        let mut render = request();
        render.scene.nodes[0].id = sensory_a;
        render.scene.nodes[0].role = crate::morphology_contract::DisplayRole::Sensory;
        render.scene.nodes[0].layer = None;
        render.scene.nodes[1].id = sensory_b;
        render.scene.nodes[1].role = crate::morphology_contract::DisplayRole::Sensory;
        render.scene.nodes[1].layer = None;
        render.scene.edges[0].source = sensory_a;
        render.scene.edges[0].target = sensory_b;
        render.waypoint_ids = vec![sensory_a, sensory_b];
        render.active_node_ids.clear();
        let tracks = vec![
            FpvReplayTrack {
                source: FpvReplaySource::Aer,
                sync_offset_us: 0,
                events: vec![FpvReplayEvent {
                    timestamp_us: 0,
                    sensory_index: 0,
                    value: 1,
                }],
            },
            FpvReplayTrack {
                source: FpvReplaySource::Audio,
                sync_offset_us: -1_000,
                events: vec![FpvReplayEvent {
                    timestamp_us: 1_000,
                    sensory_index: 1,
                    value: 1,
                }],
            },
            FpvReplayTrack {
                source: FpvReplaySource::Video,
                sync_offset_us: 0,
                events: vec![FpvReplayEvent {
                    timestamp_us: 2_000,
                    sensory_index: 0,
                    value: 1,
                }],
            },
        ];
        assert!(status.num_sensory_neurons >= 2);
        let replay = FpvReplaySpec {
            snapshot_json: Some(live_before.clone()),
            duration_ms: 3,
            step_ms: 1.0,
            tracks,
        };

        let (capture, active) = run_isolated_replay(&render, &replay, &live_before)
            .expect("isolated synchronized replay");

        assert_eq!(capture.frame_count, render.frame_count);
        assert!(active[0].contains(&sensory_a));
        assert!(active[0].contains(&sensory_b));
        assert_eq!(
            source.export_snapshot_json().expect("live after"),
            live_before
        );
    }

    #[test]
    fn request_rejects_frame_sets_that_exceed_shared_scratch_budget() {
        let mut request = request();
        request.width = 3840;
        request.height = 2160;
        request.frame_count = FPV_RENDER_MAX_FRAMES;
        assert!(request.validate().is_err());
    }

    #[test]
    fn durable_job_store_separates_owners_and_persists_progress_contract() {
        let root = std::env::temp_dir().join(format!("aarnn-fpv-test-{}", now_ms()));
        let job = submit_job(&root, "alice", request()).unwrap();
        assert_eq!(job.state, FpvRenderState::Queued);
        assert_eq!(
            job.visualization_policy_version,
            VISUALIZATION_POLICY_VERSION
        );
        assert_eq!(job.visualization_stage_track, vec![6, 6, 6]);
        assert_eq!(list_jobs(&root, "alice").unwrap(), vec![job.clone()]);
        assert!(list_jobs(&root, "bob").unwrap().is_empty());
        assert!(get_job_status(&root, &job.job_id, "bob").is_err());
        cancel_job(&root, &job.job_id, "alice").unwrap();
        assert_eq!(
            get_job_status(&root, &job.job_id, "alice").unwrap().state,
            FpvRenderState::Cancelled
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_jobs_can_be_retried_without_losing_the_captured_request() {
        let root = std::env::temp_dir().join(format!("aarnn-fpv-retry-{}", now_ms()));
        let job = submit_job(&root, "alice", request()).unwrap();
        let status_path = job_dir(&root, &job.job_id).unwrap().join("status.json");
        let mut failed = job.clone();
        failed.state = FpvRenderState::Failed;
        failed.error = Some("temporary encoder error".to_owned());
        write_json_pretty(&status_path, &failed).unwrap();
        let retried = retry_job(&root, &job.job_id, "alice").unwrap();
        assert_eq!(retried.state, FpvRenderState::Queued);
        assert_eq!(retried.error, None);
        assert!(
            job_dir(&root, &job.job_id)
                .unwrap()
                .join("request.json")
                .is_file()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn shared_worker_completes_job_and_publishes_downloadable_mp4() {
        if !Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "aarnn-fpv-worker-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let request = request();
        let expected_scene = serde_json::to_vec(&request.scene).unwrap();
        let job = submit_job(&root, "alice", request.clone()).unwrap();
        for _ in 0..8 {
            process_available_frames(&root).await.unwrap();
            if get_job_status(&root, &job.job_id, "alice")
                .is_ok_and(|status| status.state == FpvRenderState::Complete)
            {
                break;
            }
        }
        let status = get_job_status(&root, &job.job_id, "alice").unwrap();
        assert_eq!(status.state, FpvRenderState::Complete);
        assert_eq!(status.completed_frames, request.frame_count);
        assert!(
            video_path(&root, &job.job_id, "alice")
                .unwrap()
                .metadata()
                .unwrap()
                .len()
                > 0
        );
        let stored_request: FpvRenderRequest =
            read_json_if_exists(&job_dir(&root, &job.job_id).unwrap().join("request.json"))
                .unwrap()
                .unwrap();
        assert_eq!(
            serde_json::to_vec(&stored_request.scene).unwrap(),
            expected_scene
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
