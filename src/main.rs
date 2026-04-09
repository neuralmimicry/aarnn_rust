//! Entry point and CLI for the AARNN.
//!
//! Modes
//! - Batch/CLI (this file): runs a fixed‑shape, matrix‑based simulation and
//!   writes several PNG visualizations to disk.
//! - UI Runner (feature `ui`): launches an interactive application with optional
//!   `growth3d` and `morpho` features. In that mode, detailed AARNN per‑segment
//!   conduction runs inside `runner.rs` and is not exercised here.
//!
//! Notes
//! - Selecting the AARNN neuron model in the CLI forces the AARNN learning rule
//!   and enables growth by default, but still uses the LIF dynamics for batch.
//! - The Python scripts in the repository generate similarly named images; the
//!   Rust batch path mirrors those outputs for convenience.
#[macro_use]
mod obs;
mod aarnn;
mod aer;
mod affinity;
#[cfg(feature = "robot_io")]
mod bridge;
#[cfg(feature = "opencl")]
mod cl_compute;
mod config;
mod distributed;
mod engine;
mod fpaa;
mod ga;
#[cfg(feature = "opencl")]
mod gpu_api;
mod monitor;
#[cfg(feature = "morpho")]
mod morphology;
mod network;
#[cfg(feature = "openmpi")]
mod openmpi_runtime;
#[cfg(feature = "ui")]
mod providers;
mod rdma;
mod runner;
mod runtime;
mod runtime_api;
mod shared_fs;
mod sim;
#[allow(dead_code)]
mod spike_io;
mod stimuli;
#[cfg(feature = "growth3d")]
mod topology;
#[cfg(feature = "ui")]
mod ui;
mod viz;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::Deserialize;

use crate::config::{
    apply_clumping_layer_defaults, FpaaKernelRoute, FpaaStartupMode, FpaaTransportPreference,
    IzhikevichParams, LIFParams, NetworkConfig, STDPParams,
};
use crate::monitor::MonitorHeuristics;
use crate::runner::Runner;
use crate::runtime_api::{
    BlockingRuntimeClient, WorkspaceControlAction, WorkspaceCreateRequest, WorkspaceImportRequest,
};
use crate::stimuli::{AerIoConfig, AerLink};
use std::sync::atomic::AtomicBool;

fn grpc_max_message_bytes() -> usize {
    const DEFAULT: usize = 512 * 1024 * 1024;
    const MIN: usize = 4 * 1024 * 1024;
    std::env::var("NM_GRPC_MAX_MESSAGE_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v >= MIN)
        .unwrap_or(DEFAULT)
}

/// Supported neuron models for simulation.
#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum NeuronModel {
    /// Leaky Integrate-and-Fire model. Simple and fast.
    Lif,
    /// Izhikevich model. Biologically plausible and computationally efficient.
    Izh,
    /// Adaptive Axonal-Relay Neural Network model. Supports detailed morphology and delays.
    Aarnn,
}

impl NeuronModel {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Lif => "lif",
            Self::Izh => "izh",
            Self::Aarnn => "aarnn",
        }
    }
}

/// Supported synaptic learning rules for weight adaptation.
#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum LearningRule {
    /// Spike-Timing-Dependent Plasticity. Weights change based on relative timing of pre/post spikes.
    Stdp,
    /// Hebbian learning. "Neurons that fire together, wire together."
    Hebb,
    /// Oja's rule. A normalized version of Hebbian learning to prevent weight explosion.
    Oja,
    /// AARNN-specific learning rule, often coupled with morphological growth.
    Aarnn,
}

impl LearningRule {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Stdp => "stdp",
            Self::Hebb => "hebb",
            Self::Oja => "oja",
            Self::Aarnn => "aarnn",
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum RuntimeAction {
    Status,
    Create,
    Delete,
    Load,
    Save,
    Snapshot,
    Start,
    Stop,
    Reset,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum RuntimePayloadKindArg {
    Auto,
    Config,
    Snapshot,
}

impl RuntimePayloadKindArg {
    fn to_payload_kind(self) -> crate::engine::EnginePayloadKind {
        match self {
            Self::Auto => crate::engine::EnginePayloadKind::Auto,
            Self::Config => crate::engine::EnginePayloadKind::Config,
            Self::Snapshot => crate::engine::EnginePayloadKind::Snapshot,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum FpaaModeArg {
    Auto,
    Disabled,
    Required,
}

impl From<FpaaModeArg> for FpaaStartupMode {
    fn from(value: FpaaModeArg) -> Self {
        match value {
            FpaaModeArg::Auto => FpaaStartupMode::Auto,
            FpaaModeArg::Disabled => FpaaStartupMode::Disabled,
            FpaaModeArg::Required => FpaaStartupMode::Required,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum FpaaTransportArg {
    Auto,
    Pihat,
    Usb,
}

impl From<FpaaTransportArg> for FpaaTransportPreference {
    fn from(value: FpaaTransportArg) -> Self {
        match value {
            FpaaTransportArg::Auto => FpaaTransportPreference::Auto,
            FpaaTransportArg::Pihat => FpaaTransportPreference::PiHat,
            FpaaTransportArg::Usb => FpaaTransportPreference::Usb,
        }
    }
}

/// Command-Line Interface (CLI) arguments for the AARNN.
#[derive(Parser, Debug)]
#[command(author, version, about = "Neuromorphic simulation and visualization engine", long_about = None)]
struct Cli {
    /// Total duration of the simulation in milliseconds.
    #[arg(long, default_value_t = 10000.0)]
    simulation_time_ms: f64,

    /// Time step size (Δt) for the simulation in milliseconds.
    /// Smaller values increase accuracy but require more computation.
    #[arg(long, default_value_t = 1.0)]
    dt_ms: f64,

    /// Enable verbose trace output to the console for debugging purposes.
    #[arg(long, default_value_t = false)]
    trace: bool,

    /// Run the simulation in a continuous loop, dynamically adjusting the time step
    /// based on available system resources (real-time mode).
    #[arg(long, default_value_t = false)]
    continuous: bool,

    /// Seed for the random number generator to ensure simulation reproducibility.
    #[arg(long, default_value_t = 42u64)]
    seed: u64,

    /// Select the mathematical model for individual neurons.
    #[arg(long, value_enum, default_value_t = NeuronModel::Aarnn)]
    neuron_model: NeuronModel,

    /// Specify a preset for the Izhikevich model (e.g., "RS", "FS", "IB").
    /// Only applicable if `--neuron-model izh` is used.
    #[arg(long, default_value = "RS")]
    izh_type: String,

    /// Select the learning rule to apply to synaptic weights.
    #[arg(long, value_enum, default_value_t = LearningRule::Aarnn)]
    learning: LearningRule,

    /// Force-enable sleep/dream cycle (overrides config).
    #[arg(long, default_value_t = false)]
    sleep: bool,

    /// Use theta rhythm input patterns instead of Poisson in batch mode.
    #[arg(long, default_value_t = false)]
    theta_input: bool,

    /// Number of sensory neurons (input layer).
    #[arg(long, default_value_t = 50)]
    num_sensory_neurons: usize,

    /// Number of hidden layers in the network.
    #[arg(long, default_value_t = 1)]
    num_hidden_layers: usize,

    /// Initial number of neurons in each hidden layer.
    #[arg(long, default_value_t = 1)]
    num_hidden_per_layer: usize,

    /// Number of output neurons (output layer).
    #[arg(long, default_value_t = 10)]
    num_output_neurons: usize,

    /// Launch the interactive graphical user interface.
    /// Requires the project to be built with the `ui` feature enabled.
    #[arg(long, default_value_t = false)]
    ui: bool,

    /// Launch UI in remote-only mode (no local simulation compute).
    #[arg(long, default_value_t = false)]
    ui_remote_only: bool,

    /// Run as a distributed orchestrator
    #[arg(long, default_value_t = false)]
    orchestrator: bool,

    /// Run as a distributed node
    #[arg(long, default_value_t = false)]
    node: bool,

    /// Address of the orchestrator (if running as a node)
    #[arg(long)]
    orchestrator_addr: Option<String>,

    /// Listen address for gRPC
    #[arg(long, default_value = "0.0.0.0:50051")]
    grpc_addr: String,

    /// Unique brain identifier for IPC and UI tagging
    #[arg(long, default_value = "default")]
    brain_id: String,

    /// Automatically start in IPC mode and bind the socket (requires --ui)
    #[arg(long, default_value_t = false)]
    ipc: bool,

    /// Enable 3D growth of hidden topology (requires --features growth3d)
    #[arg(long, default_value_t = true)]
    growth: bool,

    /// Path to config JSON file (default: config.json)
    #[arg(long, default_value = "config.json")]
    config: String,

    /// Path to network snapshot JSON file (weights + config)
    #[arg(long)]
    network: Option<String>,

    /// FPAA startup policy: auto-detect, disable, or require hardware readiness.
    #[arg(long, value_enum)]
    fpaa_mode: Option<FpaaModeArg>,

    /// Preferred FPAA host transport to probe first.
    #[arg(long, value_enum)]
    fpaa_transport: Option<FpaaTransportArg>,

    /// Override one kernel route, e.g. --fpaa-route synaptic_filter=fpaa
    #[arg(long = "fpaa-route")]
    fpaa_routes: Vec<String>,

    /// Force startup FPAA sample tests on when probing hardware.
    #[arg(long, default_value_t = false)]
    fpaa_self_test: bool,

    /// Override the SPI device used for Pi.HAT probing.
    #[arg(long)]
    fpaa_spi_device: Option<String>,

    /// Optional USB device path or product-name hint for USB probing.
    #[arg(long)]
    fpaa_usb_hint: Option<String>,

    /// Print full FPAA status JSON at startup and continue.
    #[arg(long, default_value_t = false)]
    fpaa_print_status: bool,

    /// Print full FPAA status JSON at startup and exit.
    #[arg(long, default_value_t = false)]
    fpaa_status_only: bool,

    /// Runtime middleware URL (for workspace-backed UI/CLI flows).
    #[arg(long)]
    runtime_url: Option<String>,

    /// Runtime user identity used for workspace isolation.
    #[arg(long)]
    runtime_user: Option<String>,

    /// Runtime password used when the runtime API is in local-auth mode.
    #[arg(long)]
    runtime_password: Option<String>,

    /// Runtime workspace identifier.
    #[arg(long)]
    runtime_workspace: Option<String>,

    /// Human-friendly runtime workspace name used during create.
    #[arg(long)]
    runtime_workspace_name: Option<String>,

    /// Runtime management action to execute against the middleware API.
    #[arg(long, value_enum)]
    runtime_action: Option<RuntimeAction>,

    /// Payload interpretation for runtime load operations.
    #[arg(long, value_enum, default_value_t = RuntimePayloadKindArg::Auto)]
    runtime_payload_kind: RuntimePayloadKindArg,

    /// Create the runtime workspace on demand if it does not already exist.
    #[arg(long, default_value_t = false)]
    runtime_create_if_missing: bool,

    /// Push the current UI snapshot back to the runtime workspace when the UI exits.
    #[arg(long, default_value_t = false)]
    runtime_save_on_exit: bool,

    /// Disable all console/logging output for maximum performance.
    #[arg(long, short, default_value_t = false)]
    quiet: bool,

    /// Enable Genetic Algorithm parameter search
    #[arg(long, default_value_t = false)]
    ga_search: bool,

    /// Automatically start GA search in both standalone and cluster modes
    #[arg(long, default_value_t = false)]
    auto_ga: bool,

    /// Listen address for AER stimuli (UDP)
    #[arg(long)]
    aer_listen: Option<String>,

    /// Peer address for AER stimuli (UDP)
    #[arg(long)]
    aer_peer: Option<String>,

    /// Base address for sensory mapping (decimal, default 4096)
    #[arg(long, default_value_t = 4096)]
    aer_sensory_base: u32,

    /// Base address for output spikes (decimal, default 16384)
    #[arg(long, default_value_t = 16384)]
    aer_output_base: u32,

    /// Max buffered AER events
    #[arg(long, default_value_t = 4096)]
    aer_max_events: usize,

    /// Max AER packet size
    #[arg(long, default_value_t = 8192)]
    aer_max_packet_bytes: usize,
}

fn configure_openmp_runtime_env() {
    let auto_enabled = std::env::var("NM_OPENMP_AUTO")
        .ok()
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(true);
    if !auto_enabled {
        return;
    }

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);

    if std::env::var_os("OMP_NUM_THREADS").is_none() {
        std::env::set_var("OMP_NUM_THREADS", threads.to_string());
    }
    if std::env::var_os("OMP_PROC_BIND").is_none() {
        std::env::set_var("OMP_PROC_BIND", "close");
    }
    if std::env::var_os("OMP_PLACES").is_none() {
        std::env::set_var("OMP_PLACES", "cores");
    }
}

#[derive(Debug, Deserialize)]
struct OrchestratorNetworkSpec {
    network_id: String,
    #[serde(default)]
    config_path: Option<String>,
    #[serde(default)]
    network_path: Option<String>,
    #[serde(default)]
    neuron_model: Option<String>,
    #[serde(default)]
    learning_rule: Option<String>,
}

#[derive(Debug, Clone)]
struct OrchestratorStartupNetwork {
    network_id: String,
    num_layers: u32,
    desired_aarnn_depth: u32,
    config_json: String,
    snapshot_json: Option<String>,
    neuron_model: String,
    learning_rule: String,
}

fn load_network_config_or_snapshot(path: &str) -> anyhow::Result<(NetworkConfig, Option<String>)> {
    let payload = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read startup network payload: {}", path))?;
    if let Ok(snapshot) = crate::runner::decode_snapshot_with_profile_backfill(&payload) {
        Ok((snapshot.net, Some(payload)))
    } else {
        let mut cfg: NetworkConfig = serde_json::from_str(&payload)
            .with_context(|| format!("Failed to parse network config JSON: {}", path))?;
        if cfg.clumping_design != crate::config::ClumpingDesign::None && cfg.num_hidden_layers <= 1
        {
            apply_clumping_layer_defaults(&mut cfg);
        }
        Ok((cfg, None))
    }
}

fn build_orchestrator_startup_networks(
    args: &Cli,
    fallback_cfg: &NetworkConfig,
    fallback_config_json: Option<&str>,
    fallback_snapshot_json: Option<&str>,
) -> anyhow::Result<Vec<OrchestratorStartupNetwork>> {
    let default_model = args.neuron_model.to_str().to_string();
    let default_learning = args.learning.to_str().to_string();

    let mut startup_networks = Vec::new();
    if let Ok(raw_specs) = std::env::var("NM_ORCHESTRATOR_NETWORK_SPECS") {
        let trimmed = raw_specs.trim();
        if !trimmed.is_empty() {
            let specs: Vec<OrchestratorNetworkSpec> = serde_json::from_str(trimmed)
                .context("Failed to parse NM_ORCHESTRATOR_NETWORK_SPECS JSON payload")?;
            let mut seen = std::collections::HashSet::new();
            for spec in specs {
                let network_id = spec.network_id.trim().to_string();
                if network_id.is_empty() {
                    continue;
                }
                if !seen.insert(network_id.clone()) {
                    return Err(anyhow::anyhow!(
                        "Duplicate network_id '{}' in NM_ORCHESTRATOR_NETWORK_SPECS",
                        network_id
                    ));
                }

                let mut loaded: Option<(NetworkConfig, Option<String>)> = None;
                if let Some(path) = spec.network_path.as_deref().map(str::trim) {
                    if !path.is_empty() {
                        loaded =
                            Some(load_network_config_or_snapshot(path).with_context(|| {
                                format!(
                                    "Failed loading network_path for startup network '{}': {}",
                                    network_id, path
                                )
                            })?);
                    }
                }
                if loaded.is_none() {
                    if let Some(path) = spec.config_path.as_deref().map(str::trim) {
                        if !path.is_empty() {
                            loaded =
                                Some(load_network_config_or_snapshot(path).with_context(|| {
                                    format!(
                                        "Failed loading config_path for startup network '{}': {}",
                                        network_id, path
                                    )
                                })?);
                        }
                    }
                }

                let (cfg, snapshot_json) = if let Some((cfg, snapshot_json)) = loaded {
                    (cfg, snapshot_json)
                } else {
                    (
                        fallback_cfg.clone(),
                        fallback_snapshot_json.map(ToOwned::to_owned),
                    )
                };

                let cfg_json = serde_json::to_string(&cfg).unwrap_or_default();
                startup_networks.push(OrchestratorStartupNetwork {
                    network_id,
                    num_layers: (cfg.num_hidden_layers + 1) as u32,
                    desired_aarnn_depth: cfg.aarnn_layer_depth as u32,
                    config_json: cfg_json,
                    snapshot_json,
                    neuron_model: spec.neuron_model.unwrap_or_else(|| default_model.clone()),
                    learning_rule: spec
                        .learning_rule
                        .unwrap_or_else(|| default_learning.clone()),
                });
            }
        }
    }

    if startup_networks.is_empty() {
        startup_networks.push(OrchestratorStartupNetwork {
            network_id: args.brain_id.clone(),
            num_layers: (fallback_cfg.num_hidden_layers + 1) as u32,
            desired_aarnn_depth: fallback_cfg.aarnn_layer_depth as u32,
            config_json: fallback_config_json
                .map(ToOwned::to_owned)
                .or_else(|| serde_json::to_string(fallback_cfg).ok())
                .unwrap_or_default(),
            snapshot_json: fallback_snapshot_json.map(ToOwned::to_owned),
            neuron_model: default_model,
            learning_rule: default_learning,
        });
    }

    Ok(startup_networks)
}

fn normalize_runtime_url(raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{}", raw)
    }
}

fn build_runtime_client(args: &Cli) -> anyhow::Result<BlockingRuntimeClient> {
    let url = args
        .runtime_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--runtime-url is required for runtime actions"))?;
    BlockingRuntimeClient::new(
        normalize_runtime_url(url),
        args.runtime_user.clone(),
        args.runtime_password.clone(),
    )
}

fn read_runtime_seed_file(path: &str) -> anyhow::Result<(Option<String>, Option<String>)> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read runtime payload file: {}", path))?;
    if crate::runner::decode_snapshot_with_profile_backfill(&raw).is_ok() {
        Ok((None, Some(raw)))
    } else {
        Ok((Some(raw), None))
    }
}

fn runtime_workspace_required(args: &Cli) -> anyhow::Result<String> {
    args.runtime_workspace
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--runtime-workspace is required for this action"))
}

fn print_json_pretty<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn handle_runtime_action(args: &Cli) -> anyhow::Result<()> {
    let action = match args.runtime_action {
        Some(action) => action,
        None => return Ok(()),
    };
    let mut client = build_runtime_client(args)?;

    match action {
        RuntimeAction::Status => {
            if let Some(workspace_id) = args.runtime_workspace.as_deref() {
                let detail = client.workspace_detail(workspace_id)?;
                print_json_pretty(&detail)?;
            } else {
                let status = client.runtime_status()?;
                print_json_pretty(&status)?;
            }
        }
        RuntimeAction::Create => {
            let mut req = WorkspaceCreateRequest {
                workspace_id: args.runtime_workspace.clone(),
                name: args.runtime_workspace_name.clone(),
                config_json: None,
                snapshot_json: None,
                neuron_model: Some(args.neuron_model.to_str().to_string()),
                learning_rule: Some(args.learning.to_str().to_string()),
                auto_start: Some(false),
            };
            if let Some(network_path) = args.network.as_deref() {
                req.snapshot_json =
                    Some(std::fs::read_to_string(network_path).with_context(|| {
                        format!("Failed to read runtime network payload: {}", network_path)
                    })?);
            } else if std::path::Path::new(&args.config).exists() {
                let (config_json, snapshot_json) = read_runtime_seed_file(&args.config)?;
                req.config_json = config_json;
                req.snapshot_json = snapshot_json;
            }
            let detail = client.create_workspace(&req)?;
            print_json_pretty(&detail)?;
        }
        RuntimeAction::Delete => {
            let workspace_id = runtime_workspace_required(args)?;
            let resp = client.delete_workspace(&workspace_id)?;
            print_json_pretty(&resp)?;
        }
        RuntimeAction::Load => {
            let workspace_id = runtime_workspace_required(args)?;
            let (payload_json, kind) = if let Some(network_path) = args.network.as_deref() {
                (
                    std::fs::read_to_string(network_path).with_context(|| {
                        format!("Failed to read runtime network payload: {}", network_path)
                    })?,
                    crate::engine::EnginePayloadKind::Snapshot,
                )
            } else {
                let path = args.config.as_str();
                let raw = std::fs::read_to_string(path)
                    .with_context(|| format!("Failed to read runtime config payload: {}", path))?;
                let kind = if args.runtime_payload_kind == RuntimePayloadKindArg::Auto {
                    if crate::runner::decode_snapshot_with_profile_backfill(&raw).is_ok() {
                        crate::engine::EnginePayloadKind::Snapshot
                    } else {
                        crate::engine::EnginePayloadKind::Config
                    }
                } else {
                    args.runtime_payload_kind.to_payload_kind()
                };
                (raw, kind)
            };
            let detail = client.import_workspace(
                &workspace_id,
                &WorkspaceImportRequest {
                    payload_json,
                    kind: Some(kind),
                    replace_baseline: Some(true),
                    auto_start: Some(false),
                    neuron_model: Some(args.neuron_model.to_str().to_string()),
                    learning_rule: Some(args.learning.to_str().to_string()),
                },
            )?;
            print_json_pretty(&detail)?;
        }
        RuntimeAction::Save => {
            let workspace_id = runtime_workspace_required(args)?;
            let detail = client.control_workspace(&workspace_id, WorkspaceControlAction::Save)?;
            print_json_pretty(&detail)?;
        }
        RuntimeAction::Snapshot => {
            let workspace_id = runtime_workspace_required(args)?;
            let snapshot = client.workspace_snapshot(&workspace_id)?;
            if let Some(path) = args.network.as_deref() {
                std::fs::write(path, snapshot.snapshot_json.as_bytes())
                    .with_context(|| format!("Failed to write runtime snapshot to {}", path))?;
                println!("wrote {}", path);
            } else {
                println!("{}", snapshot.snapshot_json);
            }
        }
        RuntimeAction::Start => {
            let workspace_id = runtime_workspace_required(args)?;
            let detail = client.control_workspace(&workspace_id, WorkspaceControlAction::Start)?;
            print_json_pretty(&detail)?;
        }
        RuntimeAction::Stop => {
            let workspace_id = runtime_workspace_required(args)?;
            let detail = client.control_workspace(&workspace_id, WorkspaceControlAction::Stop)?;
            print_json_pretty(&detail)?;
        }
        RuntimeAction::Reset => {
            let workspace_id = runtime_workspace_required(args)?;
            let detail = client.control_workspace(&workspace_id, WorkspaceControlAction::Reset)?;
            print_json_pretty(&detail)?;
        }
    }
    Ok(())
}

fn parse_fpaa_route_assignment(raw: &str) -> anyhow::Result<(crate::fpaa::FpaaKernel, FpaaKernelRoute)> {
    let (kernel_raw, route_raw) = raw
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("invalid --fpaa-route '{}', expected kernel=software|fpaa", raw))?;
    let kernel = crate::fpaa::FpaaKernel::parse_id(kernel_raw)
        .ok_or_else(|| anyhow::anyhow!("unknown FPAA kernel '{}'", kernel_raw))?;
    let route = match route_raw.trim().to_ascii_lowercase().as_str() {
        "software" | "sw" => FpaaKernelRoute::Software,
        "fpaa" => FpaaKernelRoute::Fpaa,
        other => {
            return Err(anyhow::anyhow!(
                "unknown FPAA route '{}', expected software or fpaa",
                other
            ))
        }
    };
    Ok((kernel, route))
}

fn apply_fpaa_cli_overrides(net_cfg: &mut NetworkConfig, args: &Cli) -> anyhow::Result<()> {
    if let Some(mode) = args.fpaa_mode {
        net_cfg.fpaa.startup_mode = mode.into();
    }
    if let Some(transport) = args.fpaa_transport {
        net_cfg.fpaa.transport_preference = transport.into();
    }
    if args.fpaa_self_test {
        net_cfg.fpaa.run_self_test_on_startup = true;
    }
    if let Some(spi_device) = args.fpaa_spi_device.as_deref() {
        net_cfg.fpaa.spi_device = spi_device.to_string();
    }
    if let Some(usb_hint) = args.fpaa_usb_hint.as_deref() {
        net_cfg.fpaa.usb_device_hint = usb_hint.to_string();
    }
    for assignment in &args.fpaa_routes {
        let (kernel, route) = parse_fpaa_route_assignment(assignment)?;
        *kernel.route_mut(&mut net_cfg.fpaa.routing) = route;
    }
    Ok(())
}

fn log_fpaa_status(status: &crate::fpaa::FpaaRuntimeStatus) {
    nm_log!("[info] {}", status.summary);
    if let Some(err) = status.startup_error.as_deref() {
        nm_log!("[warn] FPAA startup: {}", err);
    }
    for kernel in &status.kernels {
        if kernel.requested_route == FpaaKernelRoute::Fpaa
            || kernel.effective_route == FpaaKernelRoute::Fpaa
        {
            nm_log!(
                "[info] FPAA kernel {} requested={:?} effective={:?} verification={} self_test={} note={}",
                kernel.kernel.id(),
                kernel.requested_route,
                kernel.effective_route,
                kernel.verification.label(),
                kernel.sample_test.label(),
                kernel.note
            );
        }
    }
}

fn distributed_autostart_enabled() -> bool {
    std::env::var("NM_DISTRIBUTED_AUTOSTART")
        .ok()
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(true)
}

#[cfg(feature = "openmpi")]
fn maybe_apply_openmpi_bootstrap(args: &mut Cli) -> anyhow::Result<()> {
    let force_bootstrap = std::env::var("NM_MPI_FORCE_BOOTSTRAP")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mpi_env_present = [
        "OMPI_COMM_WORLD_SIZE",
        "OMPI_COMM_WORLD_RANK",
        "MPI_LOCALRANKID",
        "PMI_SIZE",
        "PMIX_RANK",
    ]
    .iter()
    .any(|k| std::env::var_os(k).is_some());
    if !force_bootstrap && !mpi_env_present {
        return Ok(());
    }

    let bootstrap =
        crate::openmpi_runtime::bootstrap(&args.grpc_addr, args.orchestrator_addr.as_deref())?;

    let Some(bootstrap) = bootstrap else {
        return Ok(());
    };

    let explicit_role = args.orchestrator || args.node;
    if !explicit_role {
        if bootstrap.rank == 0 {
            args.orchestrator = true;
        } else {
            args.node = true;
        }
    }

    if args.node && args.orchestrator_addr.is_none() {
        args.orchestrator_addr = Some(bootstrap.orchestrator_addr.clone());
    }

    nm_log!(
        "[info] OpenMPI bootstrap: rank={}/{}, local_rank={:?}, orchestrator={}",
        bootstrap.rank,
        bootstrap.size,
        bootstrap.local_rank,
        bootstrap.orchestrator_addr
    );
    if !explicit_role {
        nm_log!(
            "[info] OpenMPI auto-role selected: {}",
            if args.orchestrator {
                "orchestrator"
            } else if args.node {
                "node"
            } else {
                "standalone"
            }
        );
    }
    nm_log!(
        "[info] OpenMPI spike transport: {}",
        if crate::openmpi_runtime::spike_transport_available() {
            "enabled"
        } else {
            "disabled"
        }
    );

    Ok(())
}

#[cfg(not(feature = "openmpi"))]
fn maybe_apply_openmpi_bootstrap(_args: &mut Cli) -> anyhow::Result<()> {
    Ok(())
}

/// Main entry point for the AARNN.
///
/// This function coordinates configuration loading, network building, and simulation execution.
/// It supports several distinct execution paths:
/// 1. **Batch Mode**: Runs a fixed-duration simulation and generates visualizations.
/// 2. **UI Mode**: Launches an interactive GUI for real-time observation and manipulation.
/// 3. **Distributed Mode**: Participates in a multi-node simulation cluster (as orchestrator or node).
fn main() -> anyhow::Result<()> {
    let mut args = Cli::parse();

    if args.quiet {
        crate::obs::SILENT.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    if args.runtime_action.is_some() {
        return handle_runtime_action(&args);
    }

    maybe_apply_openmpi_bootstrap(&mut args)?;
    configure_openmp_runtime_env();
    if !crate::obs::is_silent() {
        let log_path = std::env::var("NM_LOG_PATH").ok();
        if let Some(path) = log_path.as_deref() {
            if !path.is_empty() && path != "off" && path != "none" {
                let _ = crate::obs::init_log_file(std::path::Path::new(path));
                nm_log!("[info] Logging to file: {}", path);
            }
        } else {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let path = format!("logs/nm-{}.log", ts);
            let _ = crate::obs::init_log_file(std::path::Path::new(&path));
            nm_log!("[info] Logging to file: {}", path);
        }
    }

    // Load network configuration. Priority: Config file > CLI arguments > Defaults.
    let mut startup_config_json: Option<String> = None;
    let mut startup_snapshot_json: Option<String> = None;
    let mut net_cfg: NetworkConfig = {
        use std::fs;
        use std::path::Path;
        if Path::new(&args.config).exists() {
            let s = fs::read_to_string(&args.config)?;
            if args.network.is_none() {
                if let Ok(snap) = crate::runner::decode_snapshot_with_profile_backfill(&s) {
                    startup_snapshot_json = Some(s);
                    snap.net
                } else {
                    serde_json::from_str(&s)?
                }
            } else {
                serde_json::from_str(&s)?
            }
        } else {
            NetworkConfig {
                num_sensory_neurons: args.num_sensory_neurons,
                num_hidden_layers: args.num_hidden_layers,
                num_hidden_per_layer_initial: args.num_hidden_per_layer,
                num_output_neurons: args.num_output_neurons,
                growth_enabled: args.growth,
                use_morphology: true,
                aarnn_layer_depth: 5,
                ..NetworkConfig::default()
            }
        }
    };

    if let Some(network_path) = args.network.as_deref() {
        let s = std::fs::read_to_string(network_path)?;
        let snap = crate::runner::decode_snapshot_with_profile_backfill(&s)?;
        startup_snapshot_json = Some(s);
        net_cfg = snap.net;
    }

    if args.theta_input {
        net_cfg.theta_rhythm_enabled = true;
    }
    if args.sleep {
        net_cfg.sleep_enabled = true;
    }

    let loaded_from_snapshot = startup_snapshot_json.is_some();

    // AARNN-specific bootstrap defaults are only forced when we are not loading
    // an explicit snapshot payload. Snapshot/config values should remain authoritative.
    if matches!(args.neuron_model, NeuronModel::Aarnn) && !loaded_from_snapshot {
        net_cfg.growth_enabled = true;
        net_cfg.use_morphology = true;
        net_cfg.aarnn_layer_depth = 5;
    }
    apply_fpaa_cli_overrides(&mut net_cfg, &args)?;
    startup_config_json = serde_json::to_string(&net_cfg).ok();

    let mut remote_workspace_binding: Option<crate::runtime_api::RemoteWorkspaceBinding> = None;
    if let (Some(runtime_url), Some(workspace_id)) = (
        args.runtime_url.as_deref(),
        args.runtime_workspace.as_deref(),
    ) {
        let binding = crate::runtime_api::RemoteWorkspaceBinding {
            base_url: normalize_runtime_url(runtime_url),
            user_id: args.runtime_user.clone(),
            password: args.runtime_password.clone(),
            workspace_id: workspace_id.to_string(),
            save_on_exit: args.runtime_save_on_exit,
        };
        let mut client = binding.client()?;
        if args.runtime_create_if_missing {
            if client.workspace_detail(workspace_id).is_err() {
                let _ = client.create_workspace(&WorkspaceCreateRequest {
                    workspace_id: Some(workspace_id.to_string()),
                    name: args.runtime_workspace_name.clone(),
                    config_json: startup_config_json.clone(),
                    snapshot_json: startup_snapshot_json.clone(),
                    neuron_model: Some(args.neuron_model.to_str().to_string()),
                    learning_rule: Some(args.learning.to_str().to_string()),
                    auto_start: Some(false),
                })?;
            }
        }
        let snapshot = client.workspace_snapshot(workspace_id).with_context(|| {
            format!(
                "failed to fetch runtime workspace '{}' from {}",
                workspace_id, binding.base_url
            )
        })?;
        let snap = crate::runner::decode_snapshot_with_profile_backfill(&snapshot.snapshot_json)?;
        net_cfg = snap.net;
        apply_fpaa_cli_overrides(&mut net_cfg, &args)?;
        startup_snapshot_json = Some(snapshot.snapshot_json);
        startup_config_json = serde_json::to_string(&net_cfg).ok();
        remote_workspace_binding = Some(binding);
    }

    let fpaa_status = crate::fpaa::startup_probe(&net_cfg.fpaa);
    if args.fpaa_print_status || args.fpaa_status_only {
        print_json_pretty(&fpaa_status)?;
    }
    log_fpaa_status(&fpaa_status);
    if let Some(err) = fpaa_status.unmet_requirement() {
        return Err(anyhow::anyhow!(err));
    }
    if args.fpaa_status_only {
        return Ok(());
    }

    // Initialize tracing/logging if requested via CLI.
    if args.trace {
        nm_log!("[trace] NetworkConfig initialized: {:#?}", net_cfg);
        std::env::set_var("NM_TRACE", "1");
    }

    // Initialize a shared Tokio runtime for async background tasks.
    // This runtime is reused by UI background tasks to avoid per-task runtimes.
    let mut rt_builder = tokio::runtime::Builder::new_multi_thread();
    rt_builder.enable_all();
    rt_builder.thread_name("nm-tokio");
    crate::affinity::configure_tokio_runtime_affinity(&mut rt_builder, "nm-tokio");
    let rt = rt_builder.build()?;
    let _guard = rt.enter();

    // Configure distributed simulation roles (Orchestrator or Compute Node).
    let mut distributed_node = None;
    if args.orchestrator || args.node {
        distributed_node = Some(rt.block_on(async { start_distributed(&args).await })?);
        if args.orchestrator {
            if let Some(node) = distributed_node.clone() {
                let startup_networks = build_orchestrator_startup_networks(
                    &args,
                    &net_cfg,
                    startup_config_json.as_deref(),
                    startup_snapshot_json.as_deref(),
                )?;
                let default_playing = distributed_autostart_enabled();
                let distribute_startup_snapshot = std::env::var("NM_DISTRIBUTE_STARTUP_SNAPSHOT")
                    .ok()
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(true);
                rt.block_on(async move {
                    {
                        let mut state = node.state.write().await;
                        state.network_registry.clear();
                        state.network_snapshots.clear();
                        for startup in startup_networks {
                            let network_id = startup.network_id.clone();
                            state.network_registry.insert(
                                network_id.clone(),
                                crate::distributed::proto::NetworkStatus {
                                    network_id: network_id.clone(),
                                    distribution: std::collections::HashMap::new(),
                                    current_dt: args.dt_ms,
                                    total_neurons: 0,
                                    num_layers: startup.num_layers,
                                    desired_aarnn_depth: startup.desired_aarnn_depth,
                                    config_json: startup.config_json,
                                    neuron_model: startup.neuron_model,
                                    learning_rule: startup.learning_rule,
                                    playing: default_playing,
                                },
                            );
                            if distribute_startup_snapshot {
                                if let Some(snapshot_json) = startup.snapshot_json {
                                    state.network_snapshots.insert(network_id, snapshot_json);
                                }
                            }
                        }
                    }
                    node.rebalance_networks().await;
                });
            }
        }
    }

    let aer_cfg = {
        let mut cfg = AerIoConfig::default();
        cfg.listen_addr = args.aer_listen.clone();
        cfg.peer_addr = args.aer_peer.clone();
        cfg.sensory_base = args.aer_sensory_base;
        cfg.output_base = args.aer_output_base;
        cfg.max_events = args.aer_max_events;
        cfg.max_packet_bytes = args.aer_max_packet_bytes;
        if cfg.enabled() {
            Some(cfg)
        } else {
            None
        }
    };

    let ga_search_enabled = args.ga_search || args.auto_ga;

    // Interactive Mode: Launch the real-time visualization interface.
    // This branches away from the standard batch execution path.
    #[cfg(feature = "ui")]
    if args.ui {
        let mut net_cfg = net_cfg; // Re-use or reload config for UI consistency
        if (matches!(args.neuron_model, NeuronModel::Aarnn)
            || matches!(args.learning, LearningRule::Aarnn))
            && startup_snapshot_json.is_none()
        {
            net_cfg.growth_enabled = true;
            net_cfg.use_morphology = true;
            net_cfg.aarnn_layer_depth = 5;
        }
        return ui::launch_ui(
            net_cfg,
            args.brain_id,
            args.ipc,
            distributed_node,
            args.ui_remote_only,
            startup_snapshot_json,
            remote_workspace_binding,
            aer_cfg.clone(),
            rt.handle().clone(),
        );
    }
    #[cfg(not(feature = "ui"))]
    if args.ui {
        nm_err!("UI requested, but the binary was built without `--features ui`. Falling back to batch mode.");
    }
    let mut rng = StdRng::seed_from_u64(args.seed);

    // Initialize neuron and learning parameters with their respective defaults.
    let mut lif = LIFParams::default();
    lif.dt = args.dt_ms;
    let stdp = STDPParams::default();
    let izh = IzhikevichParams::from_preset(&args.izh_type, lif.dt);

    // Step 1: Network Construction
    // Instantiate the neural network based on the configuration (layers, connectivity).
    let built = network::build_network(&net_cfg, &mut rng);

    // Step 2: Input Generation
    // Create sensory spike trains to drive the sensory layer.
    let use_theta_input = args.theta_input || net_cfg.theta_rhythm_enabled;
    let (sensory_spikes, _pattern_id, _thirds) = if use_theta_input {
        sim::theta_input_patterns(
            args.simulation_time_ms,
            net_cfg.num_sensory_neurons,
            lif.dt,
            net_cfg.theta_rhythm_hz,
            net_cfg.theta_rhythm_duty,
            net_cfg.theta_rhythm_phase_jitter,
        )
    } else {
        sim::poisson_input_patterns(
            args.simulation_time_ms,
            net_cfg.num_sensory_neurons,
            lif.dt,
            &mut rng,
        )
    };

    // Step 3: Simulation Selection
    // Choose the appropriate neuron and learning models for the execution.
    let neuron_model = match args.neuron_model {
        NeuronModel::Lif => sim::NeuronModel::Lif,
        NeuronModel::Izh => sim::NeuronModel::Izh(izh),
        NeuronModel::Aarnn => sim::NeuronModel::Aarnn,
    };
    let mut learning = match args.learning {
        LearningRule::Stdp => sim::Learning::Stdp,
        LearningRule::Hebb => sim::Learning::Hebb,
        LearningRule::Oja => sim::Learning::Oja,
        LearningRule::Aarnn => sim::Learning::Aarnn,
    };
    if matches!(args.neuron_model, NeuronModel::Aarnn) {
        learning = sim::Learning::Aarnn;
    }

    // Branch to real-time continuous mode if requested.
    if args.continuous {
        return run_continuous(
            net_cfg,
            lif,
            stdp,
            neuron_model,
            learning,
            args.seed,
            aer_cfg,
        );
    }

    if ga_search_enabled && (args.orchestrator || args.node) {
        // Run GA search in background for distributed nodes/orchestrators
        let net_cfg_clone = net_cfg.clone();
        let seed = args.seed;
        let sim_time = args.simulation_time_ms;
        let dist_node_clone = distributed_node.clone();
        let rt_handle = rt.handle().clone();
        std::thread::spawn(move || {
            // Keep GA controller thread unpinned so worker budgeting and child pools
            // can see/use the full CPU set.
            if let Err(e) = run_ga_search(net_cfg_clone, seed, sim_time, dist_node_clone, rt_handle)
            {
                nm_err!("[error] Background GA search failed: {}", e);
            }
        });
    } else if ga_search_enabled {
        return run_ga_search(
            net_cfg,
            args.seed,
            args.simulation_time_ms,
            distributed_node,
            rt.handle().clone(),
        );
    }

    // Distributed nodes enter a perpetual sleep loop, as their work is managed via gRPC.
    if args.orchestrator || args.node {
        return rt.block_on(async {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });
    }

    // Step 4: Batch Simulation Execution
    // Perform the time-stepping simulation of the SNN.
    let sim_out = sim::run_snn(
        args.simulation_time_ms,
        &lif,
        &stdp,
        &net_cfg,
        built,
        &sensory_spikes,
        neuron_model,
        learning,
    );

    // Step 5: Post-Simulation Analysis and Reporting
    let total_h_spikes: usize = sim_out
        .spikes_h
        .iter()
        .map(|s| s.iter().filter(|&&x| x != 0).count())
        .sum();
    let total_o_spikes: usize = sim_out.spikes_o.iter().filter(|&&x| x != 0).count();
    nm_log!(
        "[info] Simulation complete. Total spikes: hidden={}, output={}",
        total_h_spikes,
        total_o_spikes
    );

    nm_log!("[summary] Hidden layer neuron counts:");
    for (l, spikes) in sim_out.spikes_h.iter().enumerate() {
        let n = spikes.shape()[1];
        nm_log!("[summary]   Layer {}: {} neurons", l + 1, n);
    }
    nm_log!(
        "[summary] Output layer neuron count: {}",
        sim_out.spikes_o.shape()[1]
    );

    let w = &sim_out.weights;
    let count_nonzero =
        |arr: &ndarray::Array2<f64>| arr.iter().filter(|&&x| x.abs() > 1e-8).count();
    nm_log!("[summary] Connections:");
    nm_log!("[summary]   Sensory -> H1: {}", count_nonzero(&w.w_in));
    for (l, m) in w.w_hh_fwd.iter().enumerate() {
        nm_log!(
            "[summary]   H{} -> H{} (fwd): {}",
            l + 1,
            l + 2,
            count_nonzero(m)
        );
    }
    for (l, m) in w.w_hh_bwd.iter().enumerate() {
        nm_log!(
            "[summary]   H{} <- H{} (bwd): {}",
            l + 1,
            l + 2,
            count_nonzero(m)
        );
    }
    nm_log!("[summary]   H_last -> Output: {}", count_nonzero(&w.w_out));
    nm_log!(
        "[summary] Longterm connections: {} / {} ({:.2}%)",
        sim_out.longterm_conn,
        sim_out.total_conn,
        if sim_out.total_conn > 0 {
            100.0 * (sim_out.longterm_conn as f64) / (sim_out.total_conn as f64)
        } else {
            0.0
        }
    );

    // Step 6: Visualization
    // Export simulation results as various graphical diagrams.
    viz::draw_network_diagram(
        "neuromorphic_network_diagram.png",
        &net_cfg,
        &sim_out.weights,
    )?;
    viz::draw_spike_raster(
        "spike_raster.png",
        &sim_out.spikes_h,
        &sim_out.spikes_o,
        lif.dt,
    )?;
    viz::draw_weight_histograms("weight_histograms.png", &sim_out.weights, false)?;
    viz::draw_weight_histograms("weight_histograms_output.png", &sim_out.weights, true)?;
    viz::draw_final_weighted_network("final_weighted_network.png", &net_cfg, &sim_out.weights)?;

    nm_log!("Files generated:\n - neuromorphic_network_diagram.png\n - spike_raster.png\n - weight_histograms.png\n - final_weighted_network.png\n - weight_histograms_output.png");
    Ok(())
}

fn run_ga_search(
    base_cfg: NetworkConfig,
    seed: u64,
    sim_time_ms: f64,
    dist_node: Option<distributed::DistributedNode>,
    rt: tokio::runtime::Handle,
) -> anyhow::Result<()> {
    use crate::ga::{GARampController, GASearch, Individual};
    crate::ga::ga_mark_dirty();
    crate::ga::ga_set_stall_timeout_secs(base_cfg.ga_stall_timeout_secs);
    let mut rng = StdRng::seed_from_u64(seed);
    let pop_size = 20;
    let n_gen = 50;
    let _mutation_rate = 0.2;
    let n_elite = 2;

    let mut ga = GASearch::new(
        pop_size.max(1),
        &base_cfg,
        &mut rng,
        dist_node.clone(),
        false,
        Vec::new(),
    );
    let mut ramp = GARampController::new(pop_size.max(1), sim_time_ms);

    // Load existing leaderboard to pick up where we left off
    if let Err(e) = ga.load_leaderboard("leaderboard.json") {
        if std::path::Path::new("leaderboard.json").exists() {
            nm_err!("[warn] Failed to load leaderboard: {}", e);
        }
    } else if !ga.leaderboard.is_empty() {
        nm_log!(
            "[info] Loaded leaderboard with {} entries. Seeding population.",
            ga.leaderboard.len()
        );
        // Seed first individual with the best from leaderboard
        ga.population[0] = ga.leaderboard[0].clone();
    }

    let (status_tx, _status_rx) = std::sync::mpsc::channel();

    for gen in 0..n_gen {
        nm_log!("\n=== Generation {}/{} ===", gen + 1, n_gen);
        let gen_seed = rng.random::<u64>();
        let plan = ramp.generation_plan();
        crate::ga::ga_set_ramp_runtime(&plan, gen);
        GARampController::apply_plan_overrides(&plan);
        ga.resize_population(plan.population_size, &base_cfg, &mut rng);
        rt.block_on(ga.evaluate_population(plan.sim_time_ms, gen_seed, &status_tx));
        ramp.note_generation_result(crate::ga::ga_abort_reason().is_none());

        // Pull best results from cluster if we are the orchestrator
        if let Some(dist) = &dist_node {
            let state = rt.block_on(async { dist.state.read().await });
            if state.is_orchestrator {
                for (node_id, node_status) in &state.nodes {
                    if let Some(res) = &node_status.resources {
                        if !res.ga_best_config_json.is_empty() && res.ga_best_fitness > 0.0 {
                            if let Ok(config) = serde_json::from_str(&res.ga_best_config_json) {
                                nm_log!(
                                    "[info] Incorporating best config from node {}: fitness {:.4}",
                                    node_id,
                                    res.ga_best_fitness
                                );
                                ga.add_to_leaderboard(Individual::new(config, res.ga_best_fitness));
                            }
                        }
                    }
                }
            }

            // Update local state for reporting back to orchestrator (if we are a node)
            let mut state_mut = rt.block_on(async { dist.state.write().await });
            state_mut.ga_running = true;
            state_mut.ga_generation = (gen + 1) as u32;
            state_mut.ga_best_fitness = ga.best_fitness;
            if let Some(best) = &ga.best_config {
                state_mut.ga_best_config_json = serde_json::to_string(best).unwrap_or_default();
            }
        }

        // Sort and print results
        ga.population.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (i, ind) in ga.population.iter().enumerate().take(5) {
            nm_log!(
                "  [{}] Fitness: {:.4} (p_in: {:.2}, p_hidden: {:.2}, p_out: {:.2})",
                i + 1,
                ind.fitness,
                ind.config.p_in,
                ind.config.p_hidden,
                ind.config.p_out
            );
        }

        if gen < n_gen - 1 {
            if !crate::ga::ga_wait_for_generation_headroom() {
                break;
            }
            if crate::ga::ga_abort_reason().is_none() {
                // Main CLI search uses DK bias by default
                ga.evolve(n_elite, true, &mut rng);
            }
            // Optionally seed with best from leaderboard if evolved population is weak
            if !ga.leaderboard.is_empty() && ga.population[0].fitness < ga.leaderboard[0].fitness {
                ga.population[n_elite] = ga.leaderboard[0].clone();
            }
        }

        // Save leaderboard after each generation
        let _ = ga.save_leaderboard("leaderboard.json");
    }

    if let Some(best) = &ga.best_config {
        nm_log!("\n=== Search Complete ===");
        nm_log!("Best Fitness: {:.4}", ga.best_fitness);
        nm_log!("Best Params: {:#?}", best);

        // Save best config
        let s = serde_json::to_string_pretty(best)?;
        std::fs::write("best_config_ga.json", s)?;
        nm_log!("Best configuration saved to best_config_ga.json");
        let _ = ga.save_leaderboard("leaderboard.json");
    }
    crate::ga::ga_clear_eval_limits_override();
    crate::ga::ga_set_worker_limit_override(None);
    crate::ga::ga_mark_clean();

    Ok(())
}

/// Executes the simulation in an infinite loop, providing a real-time data stream.
///
/// This function is used by the UI and interactive modes to maintain a persistent
/// simulation state that can be observed and modified on-the-fly.
static CONT_ABORT: AtomicBool = AtomicBool::new(false);

fn run_continuous(
    net_cfg: NetworkConfig,
    lif: LIFParams,
    stdp: STDPParams,
    neuron_model: sim::NeuronModel,
    learning: sim::Learning,
    seed: u64,
    aer_cfg: Option<AerIoConfig>,
) -> anyhow::Result<()> {
    let mut runner = Runner::new(lif, stdp, net_cfg, neuron_model, learning);
    let mut rng = StdRng::seed_from_u64(seed);
    let mut aer_link = aer_cfg.and_then(|cfg| AerLink::bind(cfg).ok());

    // Monitoring heuristics (defaults). Could be made configurable via CLI/env.
    let monitor_h = MonitorHeuristics::default();

    let mut avg_step_time_ms = 0.0;
    // Aim for ~10ms calculation time per step to leave room for system responsiveness
    let target_step_time_ms = 10.0;

    nm_log!("[info] Starting continuous simulation. Press Ctrl+C to stop.");
    nm_log!("[info] Initial dt: {:.3}ms", runner.lif.dt);

    let mut last_report = std::time::Instant::now();
    let mut total_hidden_spikes = 0;
    let mut total_output_spikes = 0;
    let mut steps_since_report = 0;

    loop {
        // Thermal guard: if the system is hot, wait until it cools.
        let waited = monitor::thermal_wait_blocking("continuous", &monitor_h, &CONT_ABORT);
        if waited.as_millis() > 0 {
            nm_log!(
                "[info] Continuous run paused for cooling: {}ms",
                waited.as_millis()
            );
        }

        observe_time!("run_continuous/step");
        let step_start = std::time::Instant::now();

        if let Some(link) = aer_link.as_mut() {
            link.poll();
        }

        // Generate continuous Poisson spikes
        let mut spikes = vec![0i8; runner.net.num_sensory_neurons];
        let base_rate = 200.0; // Hz (increased for profiling)
        let p = base_rate * runner.lif.dt / 1000.0;
        for s in &mut spikes {
            if rng.random::<f64>() < p {
                *s = 1;
            }
        }

        if let Some(link) = aer_link.as_mut() {
            let start_us = (runner.t_ms * 1000.0) as u64;
            let end_us = ((runner.t_ms + runner.lif.dt) * 1000.0) as u64;
            let aer_spikes = link.sensory_spikes(start_us, end_us, spikes.len());
            for (dst, src) in spikes.iter_mut().zip(aer_spikes.iter()) {
                if *src != 0 {
                    *dst = 1;
                }
            }
        }

        let out = runner.step(Some(&spikes));
        if let Some(link) = aer_link.as_mut() {
            let ts_us = (runner.t_ms * 1000.0) as u64;
            if let Some(out_spikes) = out.spk_o.as_slice() {
                link.send_output_spikes(ts_us, out_spikes);
            }
        }

        total_hidden_spikes += out
            .spk_h
            .iter()
            .map(|s| s.iter().filter(|&&x| x != 0).count())
            .sum::<usize>();
        total_output_spikes += out.spk_o.iter().filter(|&&x| x != 0).count();
        steps_since_report += 1;

        let step_elapsed = step_start.elapsed().as_secs_f32() * 1000.0;
        if avg_step_time_ms == 0.0 {
            avg_step_time_ms = step_elapsed;
        } else {
            avg_step_time_ms = 0.95 * avg_step_time_ms + 0.05 * step_elapsed;
        }

        // Auto-adjust dt primarily based on compute cost and lightly with resource pressure
        let current_dt = runner.lif.dt;
        if avg_step_time_ms > target_step_time_ms * 1.1 {
            // Simulation is too heavy, increase dt (coarser but faster)
            runner.set_dt((current_dt * 1.05).min(10.0));
        } else if avg_step_time_ms < target_step_time_ms * 0.9 {
            // Simulation is light, decrease dt (finer precision)
            runner.set_dt((current_dt * 0.95).max(0.01));
        }
        // Light-touch resource backoff
        let snap = monitor::get_safety_snapshot(None);
        let dt_now = runner.lif.dt;
        if let Some(free) = snap.mem_free_mb {
            if free < monitor_h.mem_free_min_mb {
                runner.set_dt((dt_now * 1.05).min(10.0));
            }
        }
        if let Some(rss) = snap.proc_rss_mb {
            if rss >= monitor_h.mem_rss_warn_mb {
                runner.set_dt((dt_now * 1.02).min(10.0));
            }
        }
        if let Some(temp) = snap.temp_c {
            if temp >= monitor_h.temp_warn_c {
                // System is warm, reduce calculation frequency
                runner.set_dt((dt_now * 1.05).min(10.0));
            }
        }

        if last_report.elapsed().as_secs() >= 1 {
            let snap = monitor::get_safety_snapshot(None);
            let sim_time = runner.t_ms;
            let temp_s = snap
                .temp_c
                .map(|v| format!("{:.0}C", v))
                .unwrap_or_else(|| "-".into());
            let free_s = snap
                .mem_free_mb
                .map(|v| format!("{}MB", v))
                .unwrap_or_else(|| "-".into());
            nm_log!(
                "[info] t={:.2}ms, dt={:.3}ms, avg_calc={:.2}ms, steps/s={}, spikes: H={}, O={}, temp={}, free_mem={}", 
                sim_time, runner.lif.dt, avg_step_time_ms, steps_since_report, total_hidden_spikes, total_output_spikes, temp_s, free_s
            );
            last_report = std::time::Instant::now();
            steps_since_report = 0;
        }

        // Optional: brief yield if it's too fast, though "shortest interval" implies max speed
        if step_elapsed < 0.1 {
            std::thread::yield_now();
        }
    }
}

/// Initializes a node for distributed simulation.
///
/// Depending on the CLI arguments, the node will either act as:
/// - **Orchestrator**: Manages the cluster, assigns partitions, and aggregates results.
/// - **Compute Node**: Performs a subset of the network simulation.
async fn start_distributed(args: &Cli) -> anyhow::Result<crate::distributed::DistributedNode> {
    use crate::distributed::proto::distributed_neuromorphic_client::DistributedNeuromorphicClient;
    use crate::distributed::proto::{HeartbeatRequest, JoinRequest};
    use crate::distributed::{
        proto::distributed_neuromorphic_server::DistributedNeuromorphicServer, DistributedNode,
        ManagedNetwork,
    };
    use tonic::transport::{Channel, Server};

    let node_id = {
        #[cfg(feature = "openmpi")]
        {
            if let Ok(rank_s) = std::env::var("OMPI_COMM_WORLD_RANK") {
                if let Ok(rank) = rank_s.parse::<u32>() {
                    format!("{}_mpi{}", args.brain_id, rank)
                } else {
                    format!("{}_{}", args.brain_id, fastrand::u32(..))
                }
            } else {
                format!("{}_{}", args.brain_id, fastrand::u32(..))
            }
        }
        #[cfg(not(feature = "openmpi"))]
        {
            format!("{}_{}", args.brain_id, fastrand::u32(..))
        }
    };
    let node = DistributedNode::new(node_id.clone(), args.orchestrator);
    node.start_optional_mpi_spike_receiver().await;

    // In node role, pre-load the startup config/snapshot from local files so a
    // large snapshot doesn't have to be delivered in one heartbeat response.
    if args.node {
        let preload_enabled = std::env::var("NM_PRELOAD_NODE_NETWORK")
            .ok()
            .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
            .unwrap_or(true);
        if preload_enabled {
            let config_fingerprint = |bytes: &[u8]| -> u64 {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                bytes.hash(&mut hasher);
                hasher.finish()
            };
            let mut preload_snapshot_json: Option<String> = None;
            let mut preload_cfg: Option<NetworkConfig> = None;

            if let Some(network_path) = args.network.as_deref() {
                if let Ok(s) = std::fs::read_to_string(network_path) {
                    if let Ok(snap) = crate::runner::decode_snapshot_with_profile_backfill(&s) {
                        preload_snapshot_json = Some(s);
                        preload_cfg = Some(snap.net);
                    }
                }
            }

            if preload_cfg.is_none() {
                let cfg_path = std::path::Path::new(&args.config);
                if cfg_path.exists() {
                    if let Ok(s) = std::fs::read_to_string(cfg_path) {
                        if let Ok(snap) = crate::runner::decode_snapshot_with_profile_backfill(&s) {
                            preload_snapshot_json = Some(s);
                            preload_cfg = Some(snap.net);
                        } else if let Ok(cfg) = serde_json::from_str::<NetworkConfig>(&s) {
                            preload_cfg = Some(cfg);
                        }
                    }
                }
            }

            if let Some(mut cfg) = preload_cfg {
                let preload_config_fingerprint = preload_snapshot_json
                    .as_ref()
                    .map(|json| config_fingerprint(json.as_bytes()))
                    .or_else(|| {
                        serde_json::to_vec(&cfg)
                            .ok()
                            .map(|bytes| config_fingerprint(&bytes))
                    });
                if (matches!(args.neuron_model, NeuronModel::Aarnn)
                    || matches!(args.learning, LearningRule::Aarnn))
                    && preload_snapshot_json.is_none()
                {
                    cfg.growth_enabled = true;
                    cfg.use_morphology = true;
                    cfg.aarnn_layer_depth = 5;
                }

                let model = match args.neuron_model {
                    NeuronModel::Lif => sim::NeuronModel::Lif,
                    NeuronModel::Izh => {
                        sim::NeuronModel::Izh(IzhikevichParams::from_preset(&args.izh_type, 1.0))
                    }
                    NeuronModel::Aarnn => sim::NeuronModel::Aarnn,
                };
                let learning = match args.learning {
                    LearningRule::Stdp => sim::Learning::Stdp,
                    LearningRule::Hebb => sim::Learning::Hebb,
                    LearningRule::Oja => sim::Learning::Oja,
                    LearningRule::Aarnn => sim::Learning::Aarnn,
                };

                let lif = LIFParams::default();
                let stdp = STDPParams::default();
                let mut runner =
                    Runner::new(lif.clone(), stdp.clone(), cfg.clone(), model, learning);

                if let Some(json) = preload_snapshot_json.as_deref() {
                    if let Err(e) = runner.import_network_json(json) {
                        nm_err!(
                            "[warn] Failed to preload startup snapshot for node {}: {}",
                            args.brain_id,
                            e
                        );
                    }
                }

                let total_layers = (runner.net.num_hidden_layers + 1) as u32;
                let assigned_layers: Vec<u32> = (0..total_layers).collect();
                let desired_depth = runner.net.aarnn_layer_depth as u32;
                let initial_config = runner.net.clone();

                let mut state = node.state.write().await;
                let workspace_binding = state.workspace_bindings.get(&args.brain_id).cloned();
                state.networks.insert(
                    args.brain_id.clone(),
                    std::sync::Arc::new(tokio::sync::RwLock::new(ManagedNetwork {
                        id: args.brain_id.clone(),
                        runner,
                        assigned_layers,
                        redundant_layers: Vec::new(),
                        remote_spikes_fwd: std::collections::HashMap::new(),
                        remote_spikes_bwd: std::collections::HashMap::new(),
                        remote_spike_steps_fwd: std::collections::HashMap::new(),
                        remote_spike_steps_bwd: std::collections::HashMap::new(),
                        external_sensory_spikes: None,
                        avg_step_time_ms: 0.0,
                        desired_aarnn_depth: desired_depth,
                        playing: true,
                        initial_config,
                        initial_model: model,
                        initial_learning: learning,
                        initial_lif: lif,
                        initial_stdp: stdp,
                        last_config_fingerprint: preload_config_fingerprint,
                        workspace_binding,
                    })),
                );
            }
        }
    }

    async fn connect_and_join(
        orchestrator_addr: &str,
        node_id: &str,
        grpc_addr: &str,
        node: &DistributedNode,
    ) -> Result<DistributedNeuromorphicClient<Channel>, String> {
        let grpc_max_msg_bytes = grpc_max_message_bytes();
        let mut client = DistributedNeuromorphicClient::connect(orchestrator_addr.to_string())
            .await
            .map_err(|e| format!("connect: {e}"))?;
        client = client
            .max_decoding_message_size(grpc_max_msg_bytes)
            .max_encoding_message_size(grpc_max_msg_bytes);
        let resources = node.get_resources().await;
        let network_resources = node.get_network_resources().await;
        let join_req = JoinRequest {
            node_id: node_id.to_string(),
            address: grpc_addr.to_string(),
            resources: Some(resources),
            network_resources,
        };
        client
            .join(join_req)
            .await
            .map_err(|e| format!("join: {e}"))?;
        Ok(client)
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    if args.orchestrator {
        DistributedNode::start_discovery_beacon(args.grpc_addr.clone(), shutdown_rx.clone())
            .await?;

        // Register the brain network if we are an orchestrator
        let default_playing = distributed_autostart_enabled();
        let mut state = node.state.write().await;
        state.network_registry.insert(
            args.brain_id.clone(),
            crate::distributed::proto::NetworkStatus {
                network_id: args.brain_id.clone(),
                distribution: std::collections::HashMap::new(),
                current_dt: args.dt_ms,
                total_neurons: 0,
                num_layers: (args.num_hidden_layers + 1) as u32,
                desired_aarnn_depth: 5, // Default to max realism depth
                config_json: String::new(),
                neuron_model: NeuronModel::Aarnn.to_str().to_string(),
                learning_rule: LearningRule::Aarnn.to_str().to_string(),
                playing: default_playing,
            },
        );
    }

    let addr: std::net::SocketAddr = args.grpc_addr.parse()?;
    nm_log!("[info] Starting distributed node {} at {}", node_id, addr);

    let node_clone = node.clone();
    let mut shutdown_rx_server = shutdown_rx.clone();
    tokio::spawn(async move {
        let grpc_max_msg_bytes = grpc_max_message_bytes();
        let shutdown = async move {
            while !*shutdown_rx_server.borrow() {
                if shutdown_rx_server.changed().await.is_err() {
                    break;
                }
            }
        };
        if let Err(e) = Server::builder()
            .add_service(
                DistributedNeuromorphicServer::new(node_clone)
                    .max_decoding_message_size(grpc_max_msg_bytes)
                    .max_encoding_message_size(grpc_max_msg_bytes),
            )
            .serve_with_shutdown(addr, shutdown)
            .await
        {
            nm_err!("[error] gRPC server failed: {}", e);
        }
    });

    let node_sim = node.clone();
    let shutdown_rx_sim = shutdown_rx.clone();
    tokio::spawn(async move {
        node_sim.run_simulation(shutdown_rx_sim).await;
    });

    let shutdown_tx_ctrl = shutdown_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx_ctrl.send(true);
    });
    #[cfg(unix)]
    {
        let shutdown_tx_term = shutdown_tx.clone();
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::spawn(async move {
            sigterm.recv().await;
            let _ = shutdown_tx_term.send(true);
        });
    }

    if args.node {
        let node_id_inner = node_id.clone();
        let node_inner = node.clone();
        let grpc_addr = args.grpc_addr.clone();
        let orchestrator_addr_arg = args.orchestrator_addr.clone();
        let shutdown_tx_node = shutdown_tx.clone();

        tokio::spawn(async move {
            let reconnect_timeout = std::time::Duration::from_secs(5 * 60);
            let reconnect_interval = std::time::Duration::from_secs(2);
            let heartbeat_interval = std::time::Duration::from_secs(5);
            let orchestrator_addr = if let Some(addr) = orchestrator_addr_arg {
                addr
            } else {
                match DistributedNode::discover_orchestrator().await {
                    Ok(addr) => addr,
                    Err(e) => {
                        nm_err!("[error] Discovery failed: {}", e);
                        return;
                    }
                }
            };
            {
                let mut state = node_inner.state.write().await;
                state._orchestrator_addr = Some(orchestrator_addr.clone());
            }

            let mut client = loop {
                match connect_and_join(&orchestrator_addr, &node_id_inner, &grpc_addr, &node_inner)
                    .await
                {
                    Ok(c) => {
                        nm_log!("[info] Successfully joined orchestrator");
                        break c;
                    }
                    Err(e) => {
                        nm_err!(
                            "[info] Waiting for orchestrator at {}: {}",
                            orchestrator_addr,
                            e
                        );
                        tokio::time::sleep(reconnect_interval).await;
                    }
                }
            };

            let mut reconnect_started: Option<std::time::Instant> = None;
            loop {
                tokio::time::sleep(heartbeat_interval).await;
                {
                    let mut state = node_inner.state.write().await;
                    state.prune_peer_maps(
                        std::time::Instant::now(),
                        crate::distributed::PEER_STALE_AFTER,
                    );
                }
                let resources = node_inner.get_resources().await;
                let network_resources = node_inner.get_network_resources().await;
                let hb_req = HeartbeatRequest {
                    node_id: node_id_inner.clone(),
                    resources: Some(resources),
                    network_resources,
                };
                match client.heartbeat(hb_req).await {
                    Ok(resp) => {
                        let mut resp = resp.into_inner();
                        if !resp.peers.is_empty() || !resp.network_peers.is_empty() {
                            let mut state = node_inner.state.write().await;
                            if !resp.peers.is_empty() {
                                state.peers = resp.peers.drain().collect();
                                let now = std::time::Instant::now();
                                state.peer_last_seen.clear();
                                let peer_ids: Vec<String> = state.peers.keys().cloned().collect();
                                for node_id in peer_ids {
                                    state.peer_last_seen.insert(node_id, now);
                                }
                            }
                            if !resp.network_peers.is_empty() {
                                state.network_peers = resp
                                    .network_peers
                                    .drain()
                                    .map(|(k, v)| (k, v.node_ids))
                                    .collect();
                            }
                            state.prune_peer_maps(
                                std::time::Instant::now(),
                                crate::distributed::PEER_STALE_AFTER,
                            );
                        }
                        let commands = resp.commands;
                        for cmd in commands {
                            node_inner.handle_command(cmd).await;
                        }
                        reconnect_started = None;
                    }
                    Err(e) => {
                        nm_err!("[error] Heartbeat failed: {}", e);
                        let reconnect_start =
                            reconnect_started.get_or_insert_with(std::time::Instant::now);
                        let deadline = *reconnect_start + reconnect_timeout;
                        loop {
                            if std::time::Instant::now() >= deadline {
                                nm_err!("[error] Orchestrator unreachable for 5 minutes; shutting down node");
                                let _ = shutdown_tx_node.send(true);
                                return;
                            }
                            match connect_and_join(
                                &orchestrator_addr,
                                &node_id_inner,
                                &grpc_addr,
                                &node_inner,
                            )
                            .await
                            {
                                Ok(c) => {
                                    nm_log!("[info] Reconnected to orchestrator");
                                    client = c;
                                    reconnect_started = None;
                                    break;
                                }
                                Err(err) => {
                                    nm_err!("[warn] Reconnect attempt failed: {}", err);
                                    tokio::time::sleep(reconnect_interval).await;
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    if args.orchestrator {
        let node_inner = node.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

                // Periodically rebalance to account for new nodes or resource changes
                node_inner.rebalance_networks().await;

                let state = node_inner.state.read().await;
                nm_log!("\n--- DASHBOARD ---");
                nm_log!("Nodes connected: {}", state.nodes.len());
                for (id, status) in &state.nodes {
                    if let Some(res) = &status.resources {
                        nm_log!(
                            " - Node {}: CPU={:.1}%, RAM={}/{} MB, Neurons={}, Redundant={}, Depth={}/{}, Networks={:?}",
                            id,
                            res.cpu_usage,
                            res.available_ram / 1024 / 1024,
                            res.total_ram / 1024 / 1024,
                            res.num_neurons,
                            res.redundant_neurons,
                            res.current_aarnn_depth,
                            res.desired_aarnn_depth,
                            status.active_networks
                        );
                    }
                }
                nm_log!("Active Networks: {}", state.network_registry.len());
                for (id, net) in &state.network_registry {
                    // Calculate estimated nodes for 1ms cycle
                    let mut total_workload_ms = 0.0;
                    let mut total_cluster_neurons = 0;
                    for node_status in state.nodes.values() {
                        if let Some(res) = &node_status.resources {
                            total_workload_ms += res.avg_step_time_ms;
                            total_cluster_neurons += res.num_neurons;
                        }
                    }

                    let avg_ms_per_neuron = if total_cluster_neurons > 0 {
                        total_workload_ms / total_cluster_neurons as f32
                    } else {
                        0.0
                    };

                    let est_nodes_1ms = if avg_ms_per_neuron > 0.0 {
                        (net.total_neurons as f32 * avg_ms_per_neuron) / 1.0
                    } else {
                        0.0
                    };

                    nm_log!(" - Network {}: dt={:.3}ms, Total Neurons={}, Distributed across {} nodes, Est. nodes for 1ms: {:.1}", 
                        id, net.current_dt, net.total_neurons, net.distribution.len(), est_nodes_1ms);
                }
                nm_log!("-----------------\n");
            }
        });
    }

    Ok(node)
}

#[allow(dead_code)]
async fn run_distributed(args: Cli) -> anyhow::Result<()> {
    let _node = start_distributed(&args).await?;
    // Keep running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
