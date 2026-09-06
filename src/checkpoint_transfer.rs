//! Bounded network transfer of immutable stable-executor checkpoints.
//!
//! A checkpoint transfer is a data-plane operation.  It verifies the complete
//! stable checkpoint set and publishes it atomically on the target, but it
//! never issues a lease, fences a source, changes a placement generation or
//! starts a worker.  Those actions remain separate management operations.
//!
//! The wire adapter deliberately transfers sealed frames rather than putting
//! checkpoint bytes in heartbeat commands.  Every frame is bounded and
//! digest-bound, retransmission of an identical frame is idempotent, and a
//! target publishes the immutable checkpoint only after the complete payload
//! has been reconstructed and verified.

use crate::deterministic::{
    BrainId, EventId, LeaseTerm, PartitionGeneration, StateDigest, StateDigestBuilder,
};
use crate::distributed::proto;
use crate::node_auth::{
    certificate_sha256_der, live_causal_transport_enabled, validate_peer_metadata,
};
use crate::stable_executor_store::{
    MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES, StableExecutorCheckpointSet,
    StableExecutorCheckpointStore, StableExecutorStoreError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub const CHECKPOINT_TRANSFER_SCHEMA_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_TRANSFER_MANIFEST_BYTES: usize = 64 * 1024;
pub const DEFAULT_CHECKPOINT_TRANSFER_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_CHECKPOINT_TRANSFER_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const CHECKPOINT_TRANSFER_SOURCE_NODE_METADATA: &str = "x-aarnn-node-id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTransferManifest {
    pub schema_version: u32,
    pub transfer_id: EventId,
    pub source_node: String,
    pub brain_id: BrainId,
    pub checkpoint_id: EventId,
    pub lease_term: LeaseTerm,
    pub partition_generation: PartitionGeneration,
    pub plan_digest: StateDigest,
    pub payload_digest: StateDigest,
    pub total_bytes: u64,
    pub frame_bytes: u32,
    pub frame_count: u32,
    pub manifest_digest: StateDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ManifestMaterial<'a> {
    schema_version: u32,
    transfer_id: EventId,
    source_node: &'a str,
    brain_id: BrainId,
    checkpoint_id: EventId,
    lease_term: LeaseTerm,
    partition_generation: PartitionGeneration,
    plan_digest: StateDigest,
    payload_digest: StateDigest,
    total_bytes: u64,
    frame_bytes: u32,
    frame_count: u32,
}

impl CheckpointTransferManifest {
    fn seal(mut self) -> Result<Self, CheckpointTransferError> {
        let bytes = serde_json::to_vec(&ManifestMaterial {
            schema_version: self.schema_version,
            transfer_id: self.transfer_id,
            source_node: &self.source_node,
            brain_id: self.brain_id,
            checkpoint_id: self.checkpoint_id,
            lease_term: self.lease_term,
            partition_generation: self.partition_generation,
            plan_digest: self.plan_digest,
            payload_digest: self.payload_digest,
            total_bytes: self.total_bytes,
            frame_bytes: self.frame_bytes,
            frame_count: self.frame_count,
        })
        .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-checkpoint-transfer-manifest:v1", bytes);
        self.manifest_digest = digest.finish();
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), CheckpointTransferError> {
        if self.schema_version != CHECKPOINT_TRANSFER_SCHEMA_VERSION
            || self.source_node.trim().is_empty()
            || self.brain_id.raw() == 0
            || self.checkpoint_id.raw() == 0
            || self.lease_term.raw() == 0
            || self.plan_digest == StateDigest([0; 16])
            || self.payload_digest == StateDigest([0; 16])
            || self.total_bytes == 0
            || self.total_bytes > MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES as u64
            || self.frame_bytes == 0
            || self.frame_bytes as usize > MAX_CHECKPOINT_TRANSFER_FRAME_BYTES
            || self.frame_count == 0
        {
            return Err(CheckpointTransferError::InvalidManifest(
                "identity, digest or transfer bounds are invalid",
            ));
        }
        let expected_count = self
            .total_bytes
            .checked_add(self.frame_bytes as u64 - 1)
            .ok_or(CheckpointTransferError::SizeOverflow)?
            / self.frame_bytes as u64;
        if expected_count != u64::from(self.frame_count) {
            return Err(CheckpointTransferError::InvalidManifest(
                "frame count does not cover the declared payload",
            ));
        }
        let expected = self.clone().seal()?.manifest_digest;
        if expected != self.manifest_digest {
            return Err(CheckpointTransferError::DigestMismatch { kind: "manifest" });
        }
        Ok(())
    }

    /// Build the bounded activation reference that a target can resolve
    /// against its own transfer service root.
    pub fn activation_reference(
        &self,
    ) -> crate::stable_worker::StableWorkerCheckpointTransferReference {
        crate::stable_worker::StableWorkerCheckpointTransferReference {
            schema_version:
                crate::stable_worker::STABLE_WORKER_CHECKPOINT_TRANSFER_REFERENCE_SCHEMA_VERSION,
            transfer_id: self.transfer_id.raw(),
            checkpoint_id: self.checkpoint_id.raw(),
            brain_id: self.brain_id.raw(),
            lease_term: self.lease_term.raw(),
            partition_generation: self.partition_generation.raw(),
            plan_digest: self.plan_digest.to_string(),
            payload_digest: self.payload_digest.to_string(),
            manifest_digest: self.manifest_digest.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTransferFrame {
    pub schema_version: u32,
    pub transfer_id: EventId,
    pub manifest_digest: StateDigest,
    pub frame_index: u32,
    pub frame_count: u32,
    pub payload: Vec<u8>,
    pub frame_digest: StateDigest,
}

impl CheckpointTransferFrame {
    fn seal(mut self) -> Result<Self, CheckpointTransferError> {
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-checkpoint-transfer-frame:v1", frame_material(&self));
        self.frame_digest = digest.finish();
        Ok(self)
    }

    fn verify(&self, manifest: &CheckpointTransferManifest) -> Result<(), CheckpointTransferError> {
        if self.schema_version != CHECKPOINT_TRANSFER_SCHEMA_VERSION
            || self.transfer_id != manifest.transfer_id
            || self.manifest_digest != manifest.manifest_digest
            || self.frame_count != manifest.frame_count
            || self.frame_index >= self.frame_count
            || self.payload.is_empty()
            || self.payload.len() > manifest.frame_bytes as usize
        {
            return Err(CheckpointTransferError::InvalidFrame);
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-checkpoint-transfer-frame:v1", frame_material(self));
        if digest.finish() != self.frame_digest {
            return Err(CheckpointTransferError::DigestMismatch { kind: "frame" });
        }
        if self.frame_index + 1 < self.frame_count
            && self.payload.len() != manifest.frame_bytes as usize
        {
            return Err(CheckpointTransferError::InvalidFrame);
        }
        Ok(())
    }
}

fn frame_material(frame: &CheckpointTransferFrame) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + frame.payload.len());
    bytes.extend_from_slice(&frame.transfer_id.raw().to_be_bytes());
    bytes.extend_from_slice(&frame.manifest_digest.0);
    bytes.extend_from_slice(&frame.frame_index.to_be_bytes());
    bytes.extend_from_slice(&frame.frame_count.to_be_bytes());
    bytes.extend_from_slice(&(frame.payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&frame.payload);
    bytes
}

#[derive(Debug, Error)]
pub enum CheckpointTransferError {
    #[error("checkpoint transfer encoding failed: {0}")]
    Encoding(String),
    #[error("checkpoint transfer manifest is invalid: {0}")]
    InvalidManifest(&'static str),
    #[error("checkpoint transfer frame is invalid")]
    InvalidFrame,
    #[error("checkpoint transfer frame size is invalid")]
    InvalidFrameSize,
    #[error("checkpoint transfer payload is incomplete: received {received} of {expected} frames")]
    Incomplete { received: usize, expected: usize },
    #[error("checkpoint transfer frame {0} is missing")]
    MissingFrame(u32),
    #[error("checkpoint transfer payload length does not match its manifest")]
    PayloadLengthMismatch,
    #[error("checkpoint transfer digest mismatch for {kind}")]
    DigestMismatch { kind: &'static str },
    #[error("checkpoint transfer payload exceeds the configured bound ({0} bytes)")]
    PayloadTooLarge(usize),
    #[error("checkpoint transfer size arithmetic overflowed")]
    SizeOverflow,
    #[error("checkpoint transfer checkpoint identity does not match its manifest")]
    CheckpointMismatch,
    #[error("checkpoint transfer checkpoint store failed: {0}")]
    Store(#[from] StableExecutorStoreError),
    #[error("checkpoint transfer transport failed: {0}")]
    Transport(String),
    #[error("checkpoint transfer wire field is invalid: {0}")]
    InvalidWire(&'static str),
    #[error("checkpoint transfer was rejected by the target: {0}")]
    Rejected(String),
}

#[derive(Debug, Clone)]
pub struct CheckpointTransferSource {
    manifest: CheckpointTransferManifest,
    payload: Vec<u8>,
}

impl CheckpointTransferSource {
    pub fn from_store(
        store: &StableExecutorCheckpointStore,
        transfer_id: EventId,
        source_node: impl Into<String>,
        checkpoint_id: EventId,
        expected_brain: BrainId,
        expected_plan: StateDigest,
        frame_bytes: usize,
    ) -> Result<Self, CheckpointTransferError> {
        let checkpoint = store.verify(checkpoint_id)?;
        Self::from_payload(
            transfer_id,
            source_node,
            checkpoint_id,
            expected_brain,
            expected_plan,
            checkpoint.manifest.lease_term,
            checkpoint.manifest.partition_generation,
            checkpoint.payload,
            frame_bytes,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_payload(
        transfer_id: EventId,
        source_node: impl Into<String>,
        checkpoint_id: EventId,
        expected_brain: BrainId,
        expected_plan: StateDigest,
        lease_term: LeaseTerm,
        partition_generation: PartitionGeneration,
        payload: Vec<u8>,
        frame_bytes: usize,
    ) -> Result<Self, CheckpointTransferError> {
        if frame_bytes == 0 || frame_bytes > MAX_CHECKPOINT_TRANSFER_FRAME_BYTES {
            return Err(CheckpointTransferError::InvalidFrameSize);
        }
        if payload.is_empty() || payload.len() > MAX_STABLE_EXECUTOR_CHECKPOINT_BYTES {
            return Err(CheckpointTransferError::PayloadTooLarge(payload.len()));
        }
        let set: StableExecutorCheckpointSet = serde_json::from_slice(&payload)
            .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?;
        set.verify()?;
        if set.brain_id != expected_brain
            || set.plan_digest != expected_plan
            || set.lease_term != lease_term
            || set.partition_generation != partition_generation
        {
            return Err(CheckpointTransferError::CheckpointMismatch);
        }
        let mut payload_digest = StateDigestBuilder::default();
        payload_digest.add_domain("stable-checkpoint-transfer-payload:v1", &payload);
        let frame_count = payload
            .len()
            .checked_add(frame_bytes - 1)
            .ok_or(CheckpointTransferError::SizeOverflow)?
            / frame_bytes;
        let frame_count =
            u32::try_from(frame_count).map_err(|_| CheckpointTransferError::SizeOverflow)?;
        let manifest = CheckpointTransferManifest {
            schema_version: CHECKPOINT_TRANSFER_SCHEMA_VERSION,
            transfer_id,
            source_node: source_node.into(),
            brain_id: expected_brain,
            checkpoint_id,
            lease_term,
            partition_generation,
            plan_digest: expected_plan,
            payload_digest: payload_digest.finish(),
            total_bytes: payload.len() as u64,
            frame_bytes: u32::try_from(frame_bytes)
                .map_err(|_| CheckpointTransferError::SizeOverflow)?,
            frame_count,
            manifest_digest: StateDigest([0; 16]),
        }
        .seal()?;
        manifest.verify()?;
        Ok(Self { manifest, payload })
    }

    pub fn manifest(&self) -> &CheckpointTransferManifest {
        &self.manifest
    }

    pub fn frames(&self) -> Result<Vec<CheckpointTransferFrame>, CheckpointTransferError> {
        self.manifest.verify()?;
        self.payload
            .chunks(self.manifest.frame_bytes as usize)
            .enumerate()
            .map(|(index, payload)| {
                CheckpointTransferFrame {
                    schema_version: CHECKPOINT_TRANSFER_SCHEMA_VERSION,
                    transfer_id: self.manifest.transfer_id,
                    manifest_digest: self.manifest.manifest_digest,
                    frame_index: u32::try_from(index)
                        .map_err(|_| CheckpointTransferError::SizeOverflow)?,
                    frame_count: self.manifest.frame_count,
                    payload: payload.to_vec(),
                    frame_digest: StateDigest([0; 16]),
                }
                .seal()
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct CheckpointTransferReceiver {
    manifest: CheckpointTransferManifest,
    frames: BTreeMap<u32, CheckpointTransferFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedCheckpoint {
    pub manifest: CheckpointTransferManifest,
    pub payload: Vec<u8>,
}

impl CheckpointTransferReceiver {
    pub fn new(manifest: CheckpointTransferManifest) -> Result<Self, CheckpointTransferError> {
        manifest.verify()?;
        Ok(Self {
            manifest,
            frames: BTreeMap::new(),
        })
    }

    pub fn accept(
        &mut self,
        frame: CheckpointTransferFrame,
    ) -> Result<(), CheckpointTransferError> {
        frame.verify(&self.manifest)?;
        if let Some(existing) = self.frames.get(&frame.frame_index) {
            if existing != &frame {
                return Err(CheckpointTransferError::InvalidFrame);
            }
            return Ok(());
        }
        self.frames.insert(frame.frame_index, frame);
        Ok(())
    }

    pub fn received_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn finalize(self) -> Result<ReceivedCheckpoint, CheckpointTransferError> {
        if self.frames.len() != self.manifest.frame_count as usize {
            return Err(CheckpointTransferError::Incomplete {
                received: self.frames.len(),
                expected: self.manifest.frame_count as usize,
            });
        }
        let mut payload = Vec::with_capacity(self.manifest.total_bytes as usize);
        for index in 0..self.manifest.frame_count {
            let frame = self
                .frames
                .get(&index)
                .ok_or(CheckpointTransferError::MissingFrame(index))?;
            payload.extend_from_slice(&frame.payload);
        }
        if payload.len() as u64 != self.manifest.total_bytes {
            return Err(CheckpointTransferError::PayloadLengthMismatch);
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-checkpoint-transfer-payload:v1", &payload);
        if digest.finish() != self.manifest.payload_digest {
            return Err(CheckpointTransferError::DigestMismatch { kind: "payload" });
        }
        let set: StableExecutorCheckpointSet = serde_json::from_slice(&payload)
            .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?;
        set.verify()?;
        if set.brain_id != self.manifest.brain_id
            || set.plan_digest != self.manifest.plan_digest
            || set.lease_term != self.manifest.lease_term
            || set.partition_generation != self.manifest.partition_generation
        {
            return Err(CheckpointTransferError::CheckpointMismatch);
        }
        Ok(ReceivedCheckpoint {
            manifest: self.manifest,
            payload,
        })
    }
}

impl ReceivedCheckpoint {
    /// Publish the verified checkpoint below the target-controlled root. The
    /// store's no-replace publication keeps a retry from overwriting an
    /// immutable checkpoint; identical retries are accepted by the store.
    pub fn publish(self, root: impl AsRef<Path>) -> Result<PathBuf, CheckpointTransferError> {
        let root = root.as_ref().to_path_buf();
        let manifest = self.manifest;
        let payload = self.payload;
        let store = StableExecutorCheckpointStore::new(&root)?;
        store.publish_payload(
            manifest.checkpoint_id,
            manifest.lease_term,
            manifest.partition_generation,
            payload,
        )?;
        publish_transfer_receipt(&root, &manifest)?;
        Ok(root)
    }
}

fn transfer_receipt_path(root: &Path, transfer_id: EventId) -> PathBuf {
    root.join(format!("transfer-{}.json", transfer_id.raw()))
}

/// Publish the transfer manifest beside the immutable checkpoint.  This
/// receipt lets a later activation prove that its transfer reference came
/// from a completed target-side transfer; it never accepts a path supplied by
/// the source or the activation command.
fn publish_transfer_receipt(
    root: &Path,
    manifest: &CheckpointTransferManifest,
) -> Result<(), CheckpointTransferError> {
    let bytes = serde_json::to_vec(manifest)
        .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?;
    if bytes.len() > MAX_CHECKPOINT_TRANSFER_MANIFEST_BYTES {
        return Err(CheckpointTransferError::PayloadTooLarge(bytes.len()));
    }
    fs::create_dir_all(root)
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
    let path = transfer_receipt_path(root, manifest.transfer_id);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(&bytes)
                .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
            file.sync_all()
                .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read(&path)
                .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
            if existing != bytes {
                return Err(CheckpointTransferError::Rejected(
                    "transfer ID is already bound to a different manifest".to_owned(),
                ));
            }
        }
        Err(error) => return Err(CheckpointTransferError::Transport(error.to_string())),
    }
    Ok(())
}

fn digest_from_bytes(
    bytes: &[u8],
    field: &'static str,
) -> Result<StateDigest, CheckpointTransferError> {
    let value: [u8; 16] = bytes
        .try_into()
        .map_err(|_| CheckpointTransferError::InvalidWire(field))?;
    Ok(StateDigest(value))
}

fn valid_node_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && value.bytes().all(|byte| byte.is_ascii())
}

/// Target-side gRPC adapter. The root is target-local configuration and is
/// never accepted from the network. A successful response means only that an
/// immutable checkpoint was published; it is not an activation acknowledgement.
#[derive(Clone)]
pub struct StableCheckpointTransferService {
    node_id: String,
    root: Arc<PathBuf>,
}

impl StableCheckpointTransferService {
    pub fn new(node_id: impl Into<String>, root: impl Into<PathBuf>) -> Result<Self, String> {
        let node_id = node_id.into();
        if !valid_node_id(&node_id) {
            return Err("checkpoint transfer node identity is invalid".to_owned());
        }
        Ok(Self {
            node_id,
            root: Arc::new(root.into()),
        })
    }

    pub fn from_env(node_id: impl Into<String>) -> Result<Self, String> {
        let node_id_string = node_id.into();
        let root = std::env::var_os("NM_CHECKPOINT_TRANSFER_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/checkpoint-transfers").join(&node_id_string));
        Self::new(node_id_string, root)
    }

    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    /// Validate a worker activation reference against the target-local
    /// transfer receipt and immutable checkpoint.  This is synchronous so it
    /// can run inside the existing bounded bootstrap blocking task.
    pub fn verify_activation_reference(
        &self,
        reference: &crate::stable_worker::StableWorkerCheckpointTransferReference,
    ) -> Result<(), CheckpointTransferError> {
        reference
            .validate()
            .map_err(|_| CheckpointTransferError::InvalidWire("activation transfer reference"))?;
        let receipt_path = transfer_receipt_path(
            self.root(),
            EventId::new(reference.transfer_id)
                .map_err(|_| CheckpointTransferError::InvalidWire("transfer ID"))?,
        );
        let receipt_bytes = fs::read(&receipt_path)
            .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
        if receipt_bytes.len() > MAX_CHECKPOINT_TRANSFER_MANIFEST_BYTES {
            return Err(CheckpointTransferError::PayloadTooLarge(
                receipt_bytes.len(),
            ));
        }
        let manifest: CheckpointTransferManifest = serde_json::from_slice(&receipt_bytes)
            .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?;
        manifest.verify()?;
        let expected_manifest = manifest.manifest_digest.to_string();
        if manifest.transfer_id.raw() != reference.transfer_id
            || manifest.checkpoint_id.raw() != reference.checkpoint_id
            || manifest.brain_id.raw() != reference.brain_id
            || manifest.lease_term.raw() != reference.lease_term
            || manifest.partition_generation.raw() != reference.partition_generation
            || manifest.plan_digest.to_string() != reference.plan_digest
            || manifest.payload_digest.to_string() != reference.payload_digest
            || expected_manifest != reference.manifest_digest
        {
            return Err(CheckpointTransferError::CheckpointMismatch);
        }
        let checkpoint_id = EventId::new(reference.checkpoint_id)
            .map_err(|_| CheckpointTransferError::InvalidWire("checkpoint ID"))?;
        let store = StableExecutorCheckpointStore::new(self.root())?;
        let checkpoint = store.verify(checkpoint_id)?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("stable-checkpoint-transfer-payload:v1", &checkpoint.payload);
        if digest.finish() != manifest.payload_digest {
            return Err(CheckpointTransferError::DigestMismatch { kind: "payload" });
        }
        Ok(())
    }

    /// Produce the exact target-local checkpoint root used by activation.
    /// This method intentionally exposes only a configured local path.
    pub fn worker_state_root(node_id: &str) -> Result<PathBuf, String> {
        if !valid_node_id(node_id) {
            return Err("checkpoint transfer worker node identity is invalid".to_owned());
        }
        Ok(std::env::var_os("NM_STABLE_WORKER_STATE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/stable-workers"))
            .join(node_id))
    }
}

/// Send one immutable checkpoint to an enrolled target. The stream is
/// bounded independently from the checkpoint size, so a slow target applies
/// backpressure to the source instead of accumulating an unbounded command or
/// heartbeat payload in the orchestrator.
pub async fn send_checkpoint_transfer(
    address: &str,
    source_node: &str,
    destination_node: &str,
    source: CheckpointTransferSource,
) -> Result<proto::CheckpointTransferAcknowledgement, CheckpointTransferError> {
    if !valid_node_id(source_node) || !valid_node_id(destination_node) {
        return Err(CheckpointTransferError::InvalidWire("node identity"));
    }
    if address.len() > 2048 || !(address.starts_with("http://") || address.starts_with("https://"))
    {
        return Err(CheckpointTransferError::InvalidWire("target address"));
    }
    let manifest = source.manifest().clone();
    if manifest.source_node != source_node {
        return Err(CheckpointTransferError::InvalidManifest(
            "source node does not match the transfer session",
        ));
    }
    let frames = source.frames()?;
    let (tx, rx) = mpsc::channel(4);
    let manifest_chunk = proto::CheckpointTransferChunk {
        schema_version: CHECKPOINT_TRANSFER_SCHEMA_VERSION,
        source_node_id: source_node.to_owned(),
        destination_node_id: destination_node.to_owned(),
        manifest_json: serde_json::to_vec(&manifest)
            .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?,
        transfer_id: manifest.transfer_id.raw(),
        manifest_digest: manifest.manifest_digest.0.to_vec(),
        ..Default::default()
    };
    let source_node_for_sender = source_node.to_owned();
    let destination_node_for_sender = destination_node.to_owned();
    let producer = tokio::spawn(async move {
        tx.send(manifest_chunk)
            .await
            .map_err(|error| error.to_string())?;
        for frame in frames {
            tx.send(proto::CheckpointTransferChunk {
                schema_version: frame.schema_version,
                source_node_id: source_node_for_sender.clone(),
                destination_node_id: destination_node_for_sender.clone(),
                transfer_id: frame.transfer_id.raw(),
                manifest_digest: frame.manifest_digest.0.to_vec(),
                frame_index: frame.frame_index,
                frame_count: frame.frame_count,
                payload: frame.payload,
                frame_digest: frame.frame_digest.0.to_vec(),
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    });

    // Use the same deployment-managed endpoint policy as the other internal
    // gRPC clients.  In the live profile this upgrades an http:// address to
    // mTLS, loads the configured CA/client identity, and fails closed on a
    // partial TLS configuration.  Keeping endpoint construction centralised
    // prevents checkpoint transfer from becoming an unauthenticated escape
    // hatch beside the causal data plane.
    let endpoint = crate::management::grpc_client_endpoint(address)
        .map_err(CheckpointTransferError::Transport)?;
    let channel = endpoint
        .connect()
        .await
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
    let mut client =
        proto::stable_checkpoint_transfer_client::StableCheckpointTransferClient::new(channel);
    let mut request = Request::new(ReceiverStream::new(rx));
    let source_metadata = tonic::metadata::MetadataValue::try_from(source_node)
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
    request
        .metadata_mut()
        .insert(CHECKPOINT_TRANSFER_SOURCE_NODE_METADATA, source_metadata);
    if live_causal_transport_enabled() {
        let node_token = crate::node_auth::configured_token_for_node(source_node)
            .map_err(CheckpointTransferError::Transport)?;
        let token_metadata = tonic::metadata::MetadataValue::try_from(node_token)
            .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
        request
            .metadata_mut()
            .insert("x-aarnn-node-token", token_metadata);
    }
    let response = client
        .transfer(request)
        .await
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?;
    let mut acknowledgements = response.into_inner();
    let acknowledgement = acknowledgements
        .message()
        .await
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?
        .ok_or(CheckpointTransferError::Transport(
            "target closed checkpoint transfer without an acknowledgement".to_owned(),
        ))?;
    producer
        .await
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?
        .map_err(CheckpointTransferError::Transport)?;
    if !acknowledgement.accepted || !acknowledgement.durable {
        return Err(CheckpointTransferError::Rejected(
            if acknowledgement.error.is_empty() {
                "target did not durably publish the checkpoint".to_owned()
            } else {
                acknowledgement.error
            },
        ));
    }
    if acknowledgement.schema_version != CHECKPOINT_TRANSFER_SCHEMA_VERSION
        || acknowledgement.transfer_id != manifest.transfer_id.raw()
        || acknowledgement.manifest_digest != manifest.manifest_digest.0.to_vec()
        || acknowledgement.received_frames != manifest.frame_count
        || acknowledgement.checkpoint_id != manifest.checkpoint_id.raw()
        || acknowledgement.brain_id != manifest.brain_id.raw()
    {
        return Err(CheckpointTransferError::InvalidWire(
            "target acknowledgement identity",
        ));
    }
    Ok(acknowledgement)
}

#[tonic::async_trait]
impl proto::stable_checkpoint_transfer_server::StableCheckpointTransfer
    for StableCheckpointTransferService
{
    type TransferStream = ReceiverStream<Result<proto::CheckpointTransferAcknowledgement, Status>>;

    async fn transfer(
        &self,
        request: Request<tonic::Streaming<proto::CheckpointTransferChunk>>,
    ) -> Result<Response<Self::TransferStream>, Status> {
        let metadata = request.metadata().clone();
        let peer_certificate_sha256 = request.peer_certs().and_then(|certificates| {
            certificates
                .first()
                .map(|certificate| certificate_sha256_der(certificate.as_ref()))
        });
        let session_source_node = metadata
            .get(CHECKPOINT_TRANSFER_SOURCE_NODE_METADATA)
            .ok_or_else(|| {
                Status::unauthenticated("checkpoint transfer source identity is required")
            })?
            .to_str()
            .map_err(|_| Status::unauthenticated("checkpoint transfer source identity is invalid"))?
            .to_owned();
        if !valid_node_id(&session_source_node) {
            return Err(Status::unauthenticated(
                "checkpoint transfer source identity is invalid",
            ));
        }
        if live_causal_transport_enabled() {
            validate_peer_metadata(
                &metadata,
                &session_source_node,
                peer_certificate_sha256.as_deref(),
            )?;
        }
        let mut stream = request.into_inner();
        let service = self.clone();
        let (tx, rx) = mpsc::channel(4);
        tokio::spawn(async move {
            let result = service
                .receive_stream(&session_source_node, &mut stream)
                .await;
            let acknowledgement = match result {
                Ok(acknowledgement) => acknowledgement,
                Err(error) => error_acknowledgement(error),
            };
            let _ = tx.send(Ok(acknowledgement)).await;
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

impl StableCheckpointTransferService {
    async fn receive_stream(
        &self,
        session_source_node: &str,
        stream: &mut tonic::Streaming<proto::CheckpointTransferChunk>,
    ) -> Result<proto::CheckpointTransferAcknowledgement, CheckpointTransferError> {
        let first = stream
            .message()
            .await
            .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?
            .ok_or(CheckpointTransferError::InvalidManifest(
                "empty transfer stream",
            ))?;
        if first.schema_version != CHECKPOINT_TRANSFER_SCHEMA_VERSION
            || first.source_node_id != session_source_node
            || first.destination_node_id != self.node_id
            || first.manifest_json.len() > MAX_CHECKPOINT_TRANSFER_MANIFEST_BYTES
            || first.manifest_json.is_empty()
            || !first.payload.is_empty()
        {
            return Err(CheckpointTransferError::InvalidManifest(
                "first chunk must contain only the bounded manifest",
            ));
        }
        let manifest: CheckpointTransferManifest = serde_json::from_slice(&first.manifest_json)
            .map_err(|error| CheckpointTransferError::Encoding(error.to_string()))?;
        manifest.verify()?;
        if manifest.source_node != session_source_node
            || first.transfer_id != manifest.transfer_id.raw()
            || digest_from_bytes(&first.manifest_digest, "manifest")? != manifest.manifest_digest
        {
            return Err(CheckpointTransferError::InvalidManifest(
                "manifest and authenticated session identities differ",
            ));
        }
        let mut receiver = CheckpointTransferReceiver::new(manifest.clone())?;
        while let Some(chunk) = stream
            .message()
            .await
            .map_err(|error| CheckpointTransferError::Transport(error.to_string()))?
        {
            if chunk.schema_version != CHECKPOINT_TRANSFER_SCHEMA_VERSION
                || chunk.source_node_id != session_source_node
                || chunk.destination_node_id != self.node_id
                || !chunk.manifest_json.is_empty()
                || chunk.transfer_id != manifest.transfer_id.raw()
                || digest_from_bytes(&chunk.manifest_digest, "manifest")?
                    != manifest.manifest_digest
            {
                return Err(CheckpointTransferError::InvalidFrame);
            }
            let frame_digest = digest_from_bytes(&chunk.frame_digest, "frame")?;
            receiver.accept(CheckpointTransferFrame {
                schema_version: chunk.schema_version,
                transfer_id: manifest.transfer_id,
                manifest_digest: manifest.manifest_digest,
                frame_index: chunk.frame_index,
                frame_count: chunk.frame_count,
                payload: chunk.payload,
                frame_digest,
            })?;
        }
        let received_frames = receiver.received_frames();
        let _published_root = tokio::task::spawn_blocking({
            let root = self.root.clone();
            move || receiver.finalize()?.publish(root.as_ref())
        })
        .await
        .map_err(|error| CheckpointTransferError::Transport(error.to_string()))??;
        Ok(proto::CheckpointTransferAcknowledgement {
            schema_version: CHECKPOINT_TRANSFER_SCHEMA_VERSION,
            transfer_id: manifest.transfer_id.raw(),
            manifest_digest: manifest.manifest_digest.0.to_vec(),
            accepted: true,
            durable: true,
            received_frames: u32::try_from(received_frames)
                .map_err(|_| CheckpointTransferError::SizeOverflow)?,
            checkpoint_id: manifest.checkpoint_id.raw(),
            brain_id: manifest.brain_id.raw(),
            error: String::new(),
        })
    }
}

fn error_acknowledgement(
    error: CheckpointTransferError,
) -> proto::CheckpointTransferAcknowledgement {
    let text = error.to_string();
    proto::CheckpointTransferAcknowledgement {
        schema_version: CHECKPOINT_TRANSFER_SCHEMA_VERSION,
        accepted: false,
        durable: false,
        error: if text.len() > 1024 {
            text[..1024].to_owned()
        } else {
            text
        },
        ..Default::default()
    }
}
