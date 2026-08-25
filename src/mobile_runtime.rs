//! Platform-neutral mobile runtime and capability contracts.
//!
//! This module is the Rust-facing seam for the future iOS and Android shells.
//! It deliberately contains no Swift, Kotlin, UIKit, Android, USB or network
//! implementation.  Platform adapters must report unavailable capabilities
//! explicitly and may not change biological, logical-time or persistence
//! semantics.  The current implementation uses [`crate::engine::RunnerEngine`]
//! as a compatibility adapter; it is not a claim that native mobile products
//! or production distributed execution are complete.

use crate::deterministic::BrainId;
use crate::engine::{EngineActivity, EnginePayloadKind, EngineSpec, RunnerEngine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MOBILE_SCHEMA_VERSION: u32 = 1;
pub const MAX_MOBILE_CHECKPOINT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DISCOVERY_ENDPOINT_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileProduct {
    Ios,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileExecutionMode {
    RemoteClient,
    ForegroundEdgeWorker,
    StandaloneBrain,
    OfflineDemonstrator,
}

impl MobileExecutionMode {
    pub const fn executes_locally(self) -> bool {
        matches!(
            self,
            Self::ForegroundEdgeWorker | Self::StandaloneBrain | Self::OfflineDemonstrator
        )
    }

    pub const fn is_single_device_authority(self) -> bool {
        matches!(self, Self::StandaloneBrain | Self::OfflineDemonstrator)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileLifecycle {
    Created,
    Ready,
    Running,
    Paused,
    Backgrounded,
    Reconnecting,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileLifecycleAction {
    Initialise,
    Start,
    Pause,
    EnterBackground,
    EnterForeground,
    Disconnect,
    Reconnect,
    Terminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MobileRuntimeError {
    #[error("mobile lifecycle transition {action:?} is invalid from {state:?}")]
    InvalidTransition {
        state: MobileLifecycle,
        action: MobileLifecycleAction,
    },
    #[error("local execution is unavailable in remote-client mode")]
    RemoteExecutionUnavailable,
    #[error("mobile execution requires Running state, current state is {0:?}")]
    NotRunning(MobileLifecycle),
    #[error("mobile runtime is already terminated")]
    Terminated,
    #[error("mobile checkpoint payload is {actual} bytes; maximum is {maximum}")]
    CheckpointTooLarge { actual: usize, maximum: usize },
    #[error("unsupported mobile checkpoint schema version {0}")]
    UnsupportedCheckpointVersion(u32),
    #[error("mobile checkpoint is malformed: {0}")]
    MalformedCheckpoint(String),
    #[error("mobile engine operation failed: {0}")]
    Engine(String),
    #[error("discovery observation is invalid: {0}")]
    InvalidDiscovery(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileCapability {
    LocalStandaloneBrain,
    RemoteManagement,
    ForegroundEdgeExecution,
    CameraCapture,
    MicrophoneCapture,
    LocalNetworkDiscovery,
    UsbAerInput,
    UsbAerOutput,
    BackgroundExecution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum CapabilityAvailability {
    Available,
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileCapabilityReport {
    pub schema_version: u32,
    pub product: MobileProduct,
    pub capabilities: BTreeMap<MobileCapability, CapabilityAvailability>,
}

impl MobileCapabilityReport {
    /// Construct a conservative report for a platform adapter that has not
    /// been installed.  Unknown capabilities are never advertised as ready.
    pub fn safe_unavailable(product: MobileProduct, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let capabilities = [
            MobileCapability::LocalStandaloneBrain,
            MobileCapability::RemoteManagement,
            MobileCapability::ForegroundEdgeExecution,
            MobileCapability::CameraCapture,
            MobileCapability::MicrophoneCapture,
            MobileCapability::LocalNetworkDiscovery,
            MobileCapability::UsbAerInput,
            MobileCapability::UsbAerOutput,
            MobileCapability::BackgroundExecution,
        ]
        .into_iter()
        .map(|capability| {
            (
                capability,
                CapabilityAvailability::Unavailable(reason.clone()),
            )
        })
        .collect();
        Self {
            schema_version: MOBILE_SCHEMA_VERSION,
            product,
            capabilities,
        }
    }

    pub fn availability(&self, capability: MobileCapability) -> CapabilityAvailability {
        self.capabilities
            .get(&capability)
            .cloned()
            .unwrap_or_else(|| CapabilityAvailability::Unavailable("not reported".to_owned()))
    }
}

/// An untrusted service-discovery observation.  It has no authority to enrol,
/// grant access, start execution or create a federation link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryObservation {
    pub schema_version: u32,
    pub observation_id: u64,
    pub service_type: String,
    pub endpoint_hint: String,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub expires_at_ms: u64,
}

impl DiscoveryObservation {
    pub fn validate(&self, now_ms: u64) -> Result<(), MobileRuntimeError> {
        if self.schema_version != MOBILE_SCHEMA_VERSION {
            return Err(MobileRuntimeError::InvalidDiscovery(format!(
                "unsupported schema version {}",
                self.schema_version
            )));
        }
        if self.observation_id == 0 {
            return Err(MobileRuntimeError::InvalidDiscovery(
                "observation ID must be non-zero".to_owned(),
            ));
        }
        if self.service_type.is_empty() || self.service_type.len() > 256 {
            return Err(MobileRuntimeError::InvalidDiscovery(
                "service type is empty or too long".to_owned(),
            ));
        }
        if self.endpoint_hint.is_empty() || self.endpoint_hint.len() > MAX_DISCOVERY_ENDPOINT_BYTES
        {
            return Err(MobileRuntimeError::InvalidDiscovery(
                "endpoint hint is empty or too long".to_owned(),
            ));
        }
        if self.protocol_min == 0 || self.protocol_min > self.protocol_max {
            return Err(MobileRuntimeError::InvalidDiscovery(
                "protocol range is invalid".to_owned(),
            ));
        }
        if self.expires_at_ms <= now_ms {
            return Err(MobileRuntimeError::InvalidDiscovery(
                "observation has expired".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobileCheckpoint {
    pub schema_version: u32,
    pub brain: BrainId,
    pub mode: MobileExecutionMode,
    pub logical_step: u64,
    pub engine_spec: EngineSpec,
    pub engine_snapshot_json: Vec<u8>,
}

impl MobileCheckpoint {
    pub fn encode(&self) -> Result<Vec<u8>, MobileRuntimeError> {
        if self.schema_version != MOBILE_SCHEMA_VERSION {
            return Err(MobileRuntimeError::UnsupportedCheckpointVersion(
                self.schema_version,
            ));
        }
        if self.engine_snapshot_json.len() > MAX_MOBILE_CHECKPOINT_BYTES {
            return Err(MobileRuntimeError::CheckpointTooLarge {
                actual: self.engine_snapshot_json.len(),
                maximum: MAX_MOBILE_CHECKPOINT_BYTES,
            });
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|error| MobileRuntimeError::MalformedCheckpoint(error.to_string()))?;
        if encoded.len() > MAX_MOBILE_CHECKPOINT_BYTES {
            return Err(MobileRuntimeError::CheckpointTooLarge {
                actual: encoded.len(),
                maximum: MAX_MOBILE_CHECKPOINT_BYTES,
            });
        }
        Ok(encoded)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, MobileRuntimeError> {
        if bytes.len() > MAX_MOBILE_CHECKPOINT_BYTES {
            return Err(MobileRuntimeError::CheckpointTooLarge {
                actual: bytes.len(),
                maximum: MAX_MOBILE_CHECKPOINT_BYTES,
            });
        }
        let checkpoint: Self = serde_json::from_slice(bytes)
            .map_err(|error| MobileRuntimeError::MalformedCheckpoint(error.to_string()))?;
        if checkpoint.schema_version != MOBILE_SCHEMA_VERSION {
            return Err(MobileRuntimeError::UnsupportedCheckpointVersion(
                checkpoint.schema_version,
            ));
        }
        if checkpoint.engine_snapshot_json.len() > MAX_MOBILE_CHECKPOINT_BYTES {
            return Err(MobileRuntimeError::CheckpointTooLarge {
                actual: checkpoint.engine_snapshot_json.len(),
                maximum: MAX_MOBILE_CHECKPOINT_BYTES,
            });
        }
        Ok(checkpoint)
    }
}

pub struct MobileRuntime {
    brain: BrainId,
    mode: MobileExecutionMode,
    lifecycle: MobileLifecycle,
    engine: Option<RunnerEngine>,
}

impl MobileRuntime {
    pub fn new(
        brain: BrainId,
        mode: MobileExecutionMode,
        spec: EngineSpec,
    ) -> Result<Self, MobileRuntimeError> {
        let engine = mode
            .executes_locally()
            .then(|| RunnerEngine::new(spec))
            .transpose()
            .map_err(|error| MobileRuntimeError::Engine(error.to_string()))?;
        Ok(Self {
            brain,
            mode,
            lifecycle: MobileLifecycle::Created,
            engine,
        })
    }

    pub fn brain(&self) -> BrainId {
        self.brain
    }

    pub fn mode(&self) -> MobileExecutionMode {
        self.mode
    }

    pub fn lifecycle(&self) -> MobileLifecycle {
        self.lifecycle
    }

    pub fn initialise(&mut self) -> Result<(), MobileRuntimeError> {
        self.transition(MobileLifecycleAction::Initialise)
    }

    pub fn start(&mut self) -> Result<(), MobileRuntimeError> {
        if !self.mode.executes_locally() {
            return Err(MobileRuntimeError::RemoteExecutionUnavailable);
        }
        self.transition(MobileLifecycleAction::Start)
    }

    pub fn pause(&mut self) -> Result<(), MobileRuntimeError> {
        self.transition(MobileLifecycleAction::Pause)
    }

    /// Checkpoint before entering background.  No biological step is performed
    /// while backgrounded; edge-worker leases are handled by the owning control
    /// plane rather than fabricated locally.
    pub fn enter_background(&mut self) -> Result<MobileCheckpoint, MobileRuntimeError> {
        let checkpoint = self.checkpoint()?;
        self.transition(MobileLifecycleAction::EnterBackground)?;
        Ok(checkpoint)
    }

    pub fn enter_foreground(&mut self) -> Result<(), MobileRuntimeError> {
        self.transition(MobileLifecycleAction::EnterForeground)
    }

    pub fn disconnect(&mut self) -> Result<(), MobileRuntimeError> {
        self.transition(MobileLifecycleAction::Disconnect)
    }

    pub fn reconnect(&mut self) -> Result<(), MobileRuntimeError> {
        self.transition(MobileLifecycleAction::Reconnect)
    }

    pub fn terminate(&mut self) -> Result<(), MobileRuntimeError> {
        self.transition(MobileLifecycleAction::Terminate)
    }

    pub fn step(&mut self, sensory: Option<&[i8]>) -> Result<EngineActivity, MobileRuntimeError> {
        if !self.mode.executes_locally() {
            return Err(MobileRuntimeError::RemoteExecutionUnavailable);
        }
        if self.lifecycle != MobileLifecycle::Running {
            return Err(MobileRuntimeError::NotRunning(self.lifecycle));
        }
        let engine = self
            .engine
            .as_mut()
            .ok_or(MobileRuntimeError::RemoteExecutionUnavailable)?;
        let activity = engine.step(sensory);
        if let Some(error) = engine.last_step_error() {
            return Err(MobileRuntimeError::Engine(error.to_owned()));
        }
        Ok(activity)
    }

    pub fn checkpoint(&self) -> Result<MobileCheckpoint, MobileRuntimeError> {
        let engine = self
            .engine
            .as_ref()
            .ok_or(MobileRuntimeError::RemoteExecutionUnavailable)?;
        let snapshot = engine
            .export_snapshot_json()
            .map_err(|error| MobileRuntimeError::Engine(error.to_string()))?
            .into_bytes();
        let checkpoint = MobileCheckpoint {
            schema_version: MOBILE_SCHEMA_VERSION,
            brain: self.brain,
            mode: self.mode,
            logical_step: engine.status().step,
            engine_spec: engine.spec().clone(),
            engine_snapshot_json: snapshot,
        };
        checkpoint.encode()?;
        Ok(checkpoint)
    }

    pub fn restore(checkpoint: MobileCheckpoint) -> Result<Self, MobileRuntimeError> {
        if checkpoint.mode == MobileExecutionMode::RemoteClient {
            return Err(MobileRuntimeError::RemoteExecutionUnavailable);
        }
        let mut runtime = Self::new(
            checkpoint.brain,
            checkpoint.mode,
            checkpoint.engine_spec.clone(),
        )?;
        let engine = runtime
            .engine
            .as_mut()
            .ok_or(MobileRuntimeError::RemoteExecutionUnavailable)?;
        let snapshot = std::str::from_utf8(&checkpoint.engine_snapshot_json)
            .map_err(|error| MobileRuntimeError::MalformedCheckpoint(error.to_string()))?;
        engine
            .import_payload_json(snapshot, EnginePayloadKind::Snapshot)
            .map_err(|error| MobileRuntimeError::MalformedCheckpoint(error.to_string()))?;
        runtime.initialise()?;
        Ok(runtime)
    }

    fn transition(&mut self, action: MobileLifecycleAction) -> Result<(), MobileRuntimeError> {
        let next = match (self.lifecycle, action) {
            (MobileLifecycle::Created, MobileLifecycleAction::Initialise) => MobileLifecycle::Ready,
            (MobileLifecycle::Ready, MobileLifecycleAction::Start)
            | (MobileLifecycle::Paused, MobileLifecycleAction::Start) => MobileLifecycle::Running,
            (MobileLifecycle::Ready, MobileLifecycleAction::Pause)
            | (MobileLifecycle::Running, MobileLifecycleAction::Pause) => MobileLifecycle::Paused,
            (MobileLifecycle::Ready, MobileLifecycleAction::EnterBackground)
            | (MobileLifecycle::Paused, MobileLifecycleAction::EnterBackground)
            | (MobileLifecycle::Running, MobileLifecycleAction::EnterBackground) => {
                MobileLifecycle::Backgrounded
            }
            (MobileLifecycle::Backgrounded, MobileLifecycleAction::EnterForeground) => {
                MobileLifecycle::Paused
            }
            (MobileLifecycle::Running, MobileLifecycleAction::Disconnect)
            | (MobileLifecycle::Backgrounded, MobileLifecycleAction::Disconnect) => {
                MobileLifecycle::Reconnecting
            }
            (MobileLifecycle::Reconnecting, MobileLifecycleAction::Reconnect) => {
                MobileLifecycle::Paused
            }
            (_, MobileLifecycleAction::Terminate) => MobileLifecycle::Terminated,
            (state, action) => {
                return Err(if state == MobileLifecycle::Terminated {
                    MobileRuntimeError::Terminated
                } else {
                    MobileRuntimeError::InvalidTransition { state, action }
                });
            }
        };
        self.lifecycle = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;

    fn small_spec() -> EngineSpec {
        let mut net = NetworkConfig::default();
        net.num_sensory_neurons = 2;
        net.num_hidden_layers = 1;
        net.num_hidden_per_layer_initial = 2;
        net.num_output_neurons = 1;
        EngineSpec {
            net,
            ..EngineSpec::default()
        }
    }

    #[test]
    fn lifecycle_background_checkpoints_without_advancing_time() {
        let brain = BrainId::new(1).expect("brain");
        let mut runtime =
            MobileRuntime::new(brain, MobileExecutionMode::StandaloneBrain, small_spec())
                .expect("runtime");
        runtime.initialise().expect("initialise");
        runtime.start().expect("start");
        runtime.step(Some(&[1, 0])).expect("step");
        let before = runtime.checkpoint().expect("checkpoint").logical_step;
        let checkpoint = runtime.enter_background().expect("background");
        assert_eq!(checkpoint.logical_step, before);
        assert_eq!(runtime.lifecycle(), MobileLifecycle::Backgrounded);
        assert!(matches!(
            runtime.step(None),
            Err(MobileRuntimeError::NotRunning(
                MobileLifecycle::Backgrounded
            ))
        ));
    }

    #[test]
    fn checkpoint_round_trip_preserves_brain_and_step() {
        let brain = BrainId::new(2).expect("brain");
        let mut runtime = MobileRuntime::new(
            brain,
            MobileExecutionMode::OfflineDemonstrator,
            small_spec(),
        )
        .expect("runtime");
        runtime.initialise().expect("initialise");
        runtime.start().expect("start");
        runtime.step(Some(&[0, 1])).expect("step");
        let checkpoint = runtime.checkpoint().expect("checkpoint");
        let bytes = checkpoint.encode().expect("encode");
        let restored = MobileRuntime::restore(MobileCheckpoint::decode(&bytes).expect("decode"))
            .expect("restore");
        assert_eq!(restored.brain(), brain);
        assert_eq!(restored.lifecycle(), MobileLifecycle::Ready);
        assert_eq!(restored.checkpoint().expect("checkpoint").logical_step, 1);
    }

    #[test]
    fn discovery_is_observation_only_and_expires() {
        let observation = DiscoveryObservation {
            schema_version: MOBILE_SCHEMA_VERSION,
            observation_id: 1,
            service_type: "_aarnn._tcp".to_owned(),
            endpoint_hint: "https://example.invalid".to_owned(),
            protocol_min: 1,
            protocol_max: 1,
            expires_at_ms: 10,
        };
        observation.validate(9).expect("valid observation");
        assert!(observation.validate(10).is_err());
    }

    #[test]
    fn unavailable_capabilities_default_deny() {
        let report = MobileCapabilityReport::safe_unavailable(
            MobileProduct::Ios,
            "native adapter not built",
        );
        assert_eq!(
            report.availability(MobileCapability::UsbAerOutput),
            CapabilityAvailability::Unavailable("native adapter not built".to_owned())
        );
    }
}
