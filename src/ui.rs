//! # Real-time Neuromorphic Visualization and Control Interface
//!
//! This module implements the graphical user interface (GUI) using `eframe` (egui).
//! It provides a comprehensive dashboard for observing and interacting with the
//! spiking neural network in real-time.
//!
//! ## Core Features:
//! - **2D/3D Network Visualization**: Interactive rendering of neurons, synapses,
//!   and morphological growth.
//! - **Real-time Probing**: Oscilloscope-style views of membrane potentials,
//!   spike rates, and synaptic weights.
//! - **Dynamic Configuration**: On-the-fly adjustment of neuron models, learning
//!   rules, and network parameters.
//! - **Input Management**: Switch between various sensory providers (Audio, Visual, IPC).
//! - **Distributed Monitoring**: Overview of the simulation cluster state and resource usage.
//! - **Tool Integration**: Direct access to Python export/analysis scripts.
//!
//! ## Implementation Details:
//! The `App` struct maintains the UI state and orchestrates the interaction
//! between the `Runner` (simulation) and the user. It uses an immediate-mode
//! rendering paradigm for high responsiveness.
#[cfg(feature = "ui")]
use eframe::{egui, egui::vec2};

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
use crate::aer::decode_events;
#[cfg(feature = "ui")]
use crate::config::{
    ClumpingDesign, FpaaKernelRoute, FpaaStartupMode, FpaaTransportPreference, IzhikevichParams,
    LIFParams, NetworkConfig, NeuromodSignal, STDPParams, apply_aarnn_human_biomimicry_defaults,
    apply_clumping_design, apply_clumping_layer_defaults,
};
#[cfg(feature = "ui")]
use crate::distributed::{
    DistributedNode, ManagedNetwork,
    proto::{
        ControlUpdate, NetworkSnapshotRequest, NetworkStatus, NetworkUpdateRequest, NodeStatus,
        StatusRequest, control_update,
        distributed_neuromorphic_client::DistributedNeuromorphicClient, network_update_request,
    },
};
#[cfg(feature = "ui")]
use crate::fpaa::{FpaaKernel, FpaaRuntimeStatus};
use crate::ga::GASearch;
#[cfg(all(feature = "ui", feature = "image_input"))]
use crate::providers::ImageFileProvider;
#[cfg(all(feature = "ui", feature = "video_input"))]
use crate::providers::VideoFileProvider;
#[cfg(all(feature = "ui", feature = "webcam_input"))]
use crate::providers::WebcamCaptureProvider;
use crate::providers::{
    AudioFileProvider, MicrophoneProvider, RandomProvider, SensoryProvider, ThetaProvider,
};
#[cfg(feature = "ui")]
use crate::runner::Runner;
#[cfg(feature = "ui")]
use crate::runtime_api::{
    RemoteWorkspaceBinding, TokenBalanceResponse, WorkspaceControlAction, WorkspaceImportRequest,
};
use crate::sim::{Learning, NeuronModel};
#[cfg(all(feature = "ui", feature = "robot_io", unix))]
use crate::spike_io::encoding::TemporalEncodingContext;
#[cfg(all(feature = "ui", feature = "robot_io", unix))]
use crate::spike_io::profiles::{
    NetworkIoProfile, NetworkIoProfileSelector, ProfileInputEncoding, ProfileOutputEncoding,
    SpikeInputEncodingStrategy, SpikeInputPrimitive, SpikeIoConfig, SpikeOutputDecodingStrategy,
    decode_network_outputs, decode_profile_outputs, encode_network_inputs_with,
    encode_profile_inputs_with, resolve_network_io_profile,
};
#[cfg(feature = "ui")]
use crate::spike_io::transport::{apply_hex_aer_payload, apply_usize_indices};
use crate::stimuli::{AerIoConfig, AerLink};
use rand::{RngExt, SeedableRng};
use std::collections::HashMap;
use std::io::BufRead;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
#[cfg(feature = "sysinfo")]
use sysinfo::{Components, ProcessRefreshKind, ProcessesToUpdate};
use tokio::sync::RwLock;
#[cfg(feature = "ui")]
use tonic::Request;

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
use crate::bridge::{IoMapping, PortKind, PortSpec, Quantizer};
#[cfg(all(feature = "ui", feature = "robot_io", unix))]
use std::os::unix::net::UnixDatagram;
#[cfg(all(feature = "ui", feature = "robot_io", unix))]
use std::path::PathBuf;

#[cfg(feature = "ui")]
#[derive(Clone)]
struct HttpAerInputStatus {
    connected: bool,
    frames_received: u64,
    status_text: String,
    last_error: Option<String>,
    last_frame_time: Option<std::time::Instant>,
    source_url: String,
}

#[cfg(feature = "ui")]
impl Default for HttpAerInputStatus {
    fn default() -> Self {
        Self {
            connected: false,
            frames_received: 0,
            status_text: "Disconnected".to_string(),
            last_error: None,
            last_frame_time: None,
            source_url: String::new(),
        }
    }
}

#[cfg(feature = "ui")]
#[derive(serde::Deserialize, Default)]
struct HttpAerNdjsonFrame {
    #[serde(default)]
    aer_payload_hex: Option<String>,
    #[serde(default)]
    spike_indices: Option<Vec<usize>>,
    #[serde(default)]
    aer_base: Option<u32>,
}

#[cfg(feature = "ui")]
struct HttpAerStreamProvider {
    num_sensory_neurons: Arc<AtomicUsize>,
    rx: std::sync::mpsc::Receiver<Vec<i8>>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    status: Arc<RwLock<HttpAerInputStatus>>,
}

#[cfg(feature = "ui")]
impl HttpAerStreamProvider {
    fn new(
        source_url: String,
        default_aer_base: u32,
        num_sensory_neurons: usize,
        status: Arc<RwLock<HttpAerInputStatus>>,
    ) -> Self {
        Self::update_status(&status, |s| {
            s.connected = false;
            s.frames_received = 0;
            s.last_error = None;
            s.last_frame_time = None;
            s.source_url = source_url.clone();
            s.status_text = "Connecting...".to_string();
        });
        let (tx, rx) = std::sync::mpsc::channel::<Vec<i8>>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let neuron_count = Arc::new(AtomicUsize::new(num_sensory_neurons));
        let stop_flag_worker = stop_flag.clone();
        let neuron_count_worker = neuron_count.clone();
        let status_worker = status.clone();
        let worker = std::thread::Builder::new()
            .name("http-aer-stream".to_string())
            .spawn(move || {
                Self::run_worker(
                    source_url,
                    default_aer_base,
                    neuron_count_worker,
                    stop_flag_worker,
                    tx,
                    status_worker,
                )
            })
            .ok();

        Self {
            num_sensory_neurons: neuron_count,
            rx,
            stop_flag,
            worker,
            status,
        }
    }

    fn update_status<F: FnOnce(&mut HttpAerInputStatus)>(
        status: &Arc<RwLock<HttpAerInputStatus>>,
        update: F,
    ) {
        if let Ok(mut guard) = status.try_write() {
            update(&mut guard);
        }
    }

    fn apply_hex_payload(
        hex_payload: &str,
        aer_base: u32,
        spikes: &mut [i8],
    ) -> Result<(), String> {
        apply_hex_aer_payload(hex_payload, aer_base, spikes)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn parse_line(
        line: &str,
        default_aer_base: u32,
        sensory_len: usize,
    ) -> Result<Option<Vec<i8>>, String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let frame: HttpAerNdjsonFrame = if trimmed.starts_with('{') {
            serde_json::from_str(trimmed).map_err(|e| format!("invalid NDJSON frame: {e}"))?
        } else {
            HttpAerNdjsonFrame {
                aer_payload_hex: Some(trimmed.to_string()),
                spike_indices: None,
                aer_base: None,
            }
        };

        let mut spikes = vec![0i8; sensory_len];
        if let Some(indices) = frame.spike_indices {
            apply_usize_indices(&indices, &mut spikes);
        }
        if let Some(payload_hex) = frame.aer_payload_hex.as_deref() {
            Self::apply_hex_payload(
                payload_hex,
                frame.aer_base.unwrap_or(default_aer_base),
                &mut spikes,
            )?;
        }
        if spikes.iter().any(|&v| v != 0) {
            Ok(Some(spikes))
        } else {
            Ok(None)
        }
    }

    fn run_worker(
        source_url: String,
        default_aer_base: u32,
        num_sensory_neurons: Arc<AtomicUsize>,
        stop_flag: Arc<AtomicBool>,
        tx: std::sync::mpsc::Sender<Vec<i8>>,
        status: Arc<RwLock<HttpAerInputStatus>>,
    ) {
        let client = match reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                Self::update_status(&status, |s| {
                    s.connected = false;
                    s.last_error = Some(format!("HTTP client init failed: {e}"));
                    s.status_text = "Source error".to_string();
                });
                return;
            }
        };

        while !stop_flag.load(Ordering::Relaxed) {
            Self::update_status(&status, |s| {
                s.connected = false;
                s.status_text = "Connecting...".to_string();
                s.last_error = None;
            });
            let response = match client
                .get(&source_url)
                .header(reqwest::header::ACCEPT, "application/x-ndjson")
                .send()
            {
                Ok(resp) => resp,
                Err(e) => {
                    Self::update_status(&status, |s| {
                        s.connected = false;
                        s.last_error = Some(format!("connect failed: {e}"));
                        s.status_text = "Source error".to_string();
                    });
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            if !response.status().is_success() {
                let code = response.status();
                Self::update_status(&status, |s| {
                    s.connected = false;
                    s.last_error = Some(format!("HTTP source returned {code}"));
                    s.status_text = "Source error".to_string();
                });
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }

            Self::update_status(&status, |s| {
                s.connected = true;
                s.status_text = "Streaming".to_string();
                s.last_error = None;
            });

            let mut reader = std::io::BufReader::new(response);
            let mut line = String::new();
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        Self::update_status(&status, |s| {
                            s.connected = false;
                            s.status_text = "Stream closed (reconnecting)".to_string();
                        });
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) {
                            continue;
                        }
                        Self::update_status(&status, |s| {
                            s.connected = false;
                            s.last_error = Some(format!("stream read failed: {e}"));
                            s.status_text = "Source error".to_string();
                        });
                        break;
                    }
                }

                let sensory_len = num_sensory_neurons.load(Ordering::Relaxed);
                match Self::parse_line(&line, default_aer_base, sensory_len) {
                    Ok(Some(spikes)) => {
                        if tx.send(spikes).is_err() {
                            return;
                        }
                        Self::update_status(&status, |s| {
                            s.connected = true;
                            s.frames_received = s.frames_received.saturating_add(1);
                            s.last_frame_time = Some(std::time::Instant::now());
                            s.status_text = "Streaming".to_string();
                            s.last_error = None;
                        });
                    }
                    Ok(None) => {}
                    Err(err) => {
                        Self::update_status(&status, |s| {
                            s.last_error = Some(err);
                            s.status_text = "Decode error".to_string();
                        });
                    }
                }
            }

            if !stop_flag.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        Self::update_status(&status, |s| {
            s.connected = false;
            s.status_text = "Disconnected".to_string();
        });
    }
}

#[cfg(feature = "ui")]
impl SensoryProvider for HttpAerStreamProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let mut latest: Option<Vec<i8>> = None;
        while let Ok(spikes) = self.rx.try_recv() {
            latest = Some(spikes);
        }
        if let Some(spikes) = latest {
            spikes
        } else {
            vec![0i8; self.num_sensory_neurons.load(Ordering::Relaxed)]
        }
    }

    fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.worker.take();
        Self::update_status(&self.status, |s| {
            s.connected = false;
            s.status_text = "Disconnected".to_string();
        });
    }

    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons.store(n_s, Ordering::Relaxed);
    }
}

#[cfg(feature = "ui")]
impl Drop for HttpAerStreamProvider {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
#[derive(serde::Deserialize, Clone, PartialEq)]
struct IpcHandshake {
    #[serde(default)]
    s_names: Vec<String>,
    #[serde(default)]
    o_names: Vec<String>,
    #[serde(default)]
    reward_name: Option<String>,
    #[serde(default)]
    sensory: Option<usize>,
    #[serde(default)]
    output: Option<usize>,
    #[serde(default)]
    expected_s: Option<usize>,
    #[serde(default)]
    expected_o: Option<usize>,
    #[serde(default)]
    num_sensory_neurons: Option<usize>,
    #[serde(default)]
    num_output_neurons: Option<usize>,
}

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
enum IpcEvent {
    Data(f32, f32, Option<String>),       // t_ms, reward, peer_path
    Config(IpcHandshake, Option<String>), // handshake, peer_path
}

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
struct IpcUdsServer {
    sock: UnixDatagram,
    #[allow(dead_code)]
    s: usize,
    #[allow(dead_code)]
    o: usize,
    aer_sensory_base: u32,
    aer_output_base: u32,
    last_peer: Option<PathBuf>,
    req_buf: Vec<u8>,
    truncated_packets: u64,
    #[allow(dead_code)]
    recent_drops: u64,
    #[allow(dead_code)]
    recent_mismatches: u64,
    total_received: u64,
}

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
impl IpcUdsServer {
    fn bind(
        path: &str,
        s: usize,
        o: usize,
        aer_sensory_base: u32,
        aer_output_base: u32,
    ) -> std::io::Result<Self> {
        let req_buf_bytes = std::env::var("NM_IPC_UDS_RECV_BUF_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map(|v| v.clamp(8_192, 4 * 1024 * 1024))
            .unwrap_or(262_144);
        let _ = std::fs::remove_file(path);
        let sock = UnixDatagram::bind(path)?;
        sock.set_nonblocking(true)?;
        nm_err!(
            "[IpcUdsServer] Bound to {} with S={}, O={}, recv_buf={}B",
            path,
            s,
            o,
            req_buf_bytes
        );
        Ok(Self {
            sock,
            s,
            o,
            aer_sensory_base,
            aer_output_base,
            last_peer: None,
            req_buf: vec![0u8; req_buf_bytes],
            truncated_packets: 0,
            recent_drops: 0,
            recent_mismatches: 0,
            total_received: 0,
        })
    }
    fn poll_next_event(&mut self, dst_inputs: &mut [f32]) -> Option<IpcEvent> {
        let need_data = (1 + self.s) * 4;
        let need_data_reward = (2 + self.s) * 4;
        loop {
            match self.sock.recv_from(&mut self.req_buf) {
                Ok((n, addr)) => {
                    if n == 0 {
                        continue;
                    }
                    if n == self.req_buf.len() {
                        self.truncated_packets = self.truncated_packets.saturating_add(1);
                        if self.truncated_packets == 1 || self.truncated_packets % 64 == 0 {
                            nm_err!(
                                "[IpcUdsServer] Received packet at recv buffer limit ({} bytes). Consider increasing NM_IPC_UDS_RECV_BUF_BYTES.",
                                self.req_buf.len()
                            );
                        }
                    }
                    if n > 0 && self.req_buf[0] == b'{' {
                        match serde_json::from_slice::<IpcHandshake>(&self.req_buf[..n]) {
                            Ok(hs) => {
                                self.last_peer = addr.as_pathname().map(|p| p.to_path_buf());
                                nm_err!(
                                    "[IpcUdsServer] Handshake received S_names={} O_names={}",
                                    hs.s_names.len(),
                                    hs.o_names.len()
                                );
                                self.send_size_hint_to_last_peer();
                                let peer_str = self
                                    .last_peer
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().to_string());
                                return Some(IpcEvent::Config(hs, peer_str));
                            }
                            Err(err) => {
                                nm_err!("[IpcUdsServer] Handshake parse failed: {}", err);
                                self.recent_mismatches = self.recent_mismatches.saturating_add(1);
                                continue;
                            }
                        }
                    }
                    if n >= 4 && &self.req_buf[..4] == b"AER1" {
                        self.last_peer = addr.as_pathname().map(|p| p.to_path_buf());
                        let peer_str = self
                            .last_peer
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string());
                        for v in dst_inputs.iter_mut() {
                            *v = 0.0;
                        }
                        match decode_events(&self.req_buf[..n]) {
                            Ok(events) => {
                                for ev in events {
                                    if ev.value == 0 {
                                        continue;
                                    }
                                    let idx = if ev.addr >= self.aer_sensory_base {
                                        (ev.addr - self.aer_sensory_base) as usize
                                    } else {
                                        ev.addr as usize
                                    };
                                    if idx < dst_inputs.len() {
                                        dst_inputs[idx] = 1.0;
                                    }
                                }
                                return Some(IpcEvent::Data(0.0, 0.0, peer_str));
                            }
                            Err(_) => {
                                self.recent_mismatches = self.recent_mismatches.saturating_add(1);
                                self.send_size_hint_to_addr(&addr);
                                continue;
                            }
                        }
                    }
                    if n == need_data || n == need_data_reward {
                        self.last_peer = addr.as_pathname().map(|p| p.to_path_buf());
                        let peer_str = self
                            .last_peer
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string());
                        self.total_received = self.total_received.saturating_add(1);
                        let mut rdr = &self.req_buf[..];
                        let read_f32 = |bytes: &mut &[u8]| -> f32 {
                            let (head, rest) = bytes.split_at(4);
                            *bytes = rest;
                            f32::from_le_bytes(head.try_into().unwrap())
                        };
                        let t_ms = read_f32(&mut rdr);
                        for i in 0..self.s.min(dst_inputs.len()) {
                            dst_inputs[i] = read_f32(&mut rdr);
                        }
                        let reward = if n == need_data_reward {
                            read_f32(&mut rdr)
                        } else {
                            0.0
                        };
                        return Some(IpcEvent::Data(t_ms, reward, peer_str));
                    } else {
                        self.recent_mismatches = self.recent_mismatches.saturating_add(1);
                        self.send_size_hint_to_addr(&addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return None;
                }
                Err(_) => {
                    return None;
                }
            }
        }
    }
    #[allow(dead_code)]
    fn send_outputs(&mut self, outputs: &[f32]) -> std::io::Result<()> {
        if let Some(ref peer) = self.last_peer {
            let mut buf = Vec::with_capacity(outputs.len() * 4);
            for &v in outputs {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            let _ = self.sock.send_to(&buf, peer);
        }
        Ok(())
    }
    fn send_size_hint_to_last_peer(&self) {
        if let Some(ref peer) = self.last_peer {
            let hint = format!("{{\"expected_s\":{},\"expected_o\":{}}}", self.s, self.o);
            if let Err(e) = self.sock.send_to(hint.as_bytes(), peer) {
                nm_err!(
                    "[IpcUdsServer] Failed to send size hint to peer {:?}: {}",
                    peer,
                    e
                );
            }
        }
    }
    fn send_size_hint_to_addr(&self, addr: &std::os::unix::net::SocketAddr) {
        if let Some(path) = addr.as_pathname() {
            let hint = format!("{{\"expected_s\":{},\"expected_o\":{}}}", self.s, self.o);
            if let Err(e) = self.sock.send_to(hint.as_bytes(), path) {
                nm_err!(
                    "[IpcUdsServer] Failed to send size hint to addr {:?}: {}",
                    path,
                    e
                );
            }
        }
    }
    #[allow(dead_code)]
    fn stop(&mut self) {}
    #[allow(dead_code)]
    fn take_recent_drops(&mut self) -> u64 {
        let d = self.recent_drops;
        self.recent_drops = 0;
        d
    }
    #[allow(dead_code)]
    fn take_recent_mismatches(&mut self) -> u64 {
        let m = self.recent_mismatches;
        self.recent_mismatches = 0;
        m
    }
}

#[cfg(all(feature = "ui", feature = "robot_io", unix))]
fn resolve_ipc_handshake_sizes(
    handshake: &IpcHandshake,
    fallback_s: usize,
    fallback_o: usize,
) -> (usize, usize) {
    let from_names_s = (!handshake.s_names.is_empty()).then_some(handshake.s_names.len());
    let from_names_o = (!handshake.o_names.is_empty()).then_some(handshake.o_names.len());

    let sensory = from_names_s
        .or(handshake.sensory)
        .or(handshake.expected_s)
        .or(handshake.num_sensory_neurons)
        .unwrap_or(fallback_s)
        .max(1);
    let output = from_names_o
        .or(handshake.output)
        .or(handshake.expected_o)
        .or(handshake.num_output_neurons)
        .unwrap_or(fallback_o)
        .max(1);
    (sensory, output)
}

#[cfg(all(feature = "ui", feature = "morpho", feature = "growth3d"))]
use crate::morphology::SynKind;

#[cfg(feature = "ui")]
struct EdgeVisual {
    p0: egui::Pos2,
    p1: egui::Pos2,
    from_label: String,
    to_label: String,
    weight: Option<f32>,
    kind: &'static str, // "fwd", "bwd", "rec", "in", "out", "overlay", "feedback"
    is_longterm: bool,
}

#[cfg(feature = "ui")]
struct CachedEdge {
    from_layer: i32,
    to_layer: i32,
    from_idx: usize,
    to_idx: usize,
    weight: f32,
    kind: &'static str,
    is_longterm: bool,
}

#[cfg(feature = "ui")]
/// Launch the egui/eframe application.
///
/// If `growth_enabled` is true and the binary is compiled with the
/// `growth3d` feature, the Runner starts in growth mode (1×1 bootstrap).
pub fn launch_ui(
    net_cfg: crate::config::NetworkConfig,
    brain_id: String,
    ipc_enabled: bool,
    distributed_node: Option<DistributedNode>,
    remote_only: bool,
    startup_snapshot_json: Option<String>,
    remote_workspace_binding: Option<RemoteWorkspaceBinding>,
    aer_cfg: Option<AerIoConfig>,
    runtime_handle: tokio::runtime::Handle,
) -> anyhow::Result<()> {
    let ui_hidden = std::env::var("NM_UI_HIDDEN")
        .ok()
        .map(|v| {
            matches!(
                v.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("Neuromorphic Network - {}", brain_id))
        .with_inner_size(vec2(1100.0, 700.0));
    if ui_hidden {
        viewport = viewport.with_visible(false);
    }
    let mut native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    #[cfg(target_arch = "aarch64")]
    {
        native_options.renderer = eframe::Renderer::Wgpu;
    }
    if let Ok(renderer_name) = std::env::var("NM_UI_RENDERER") {
        if let Ok(renderer) = renderer_name.parse::<eframe::Renderer>() {
            native_options.renderer = renderer;
        }
    }
    if let Err(e) = eframe::run_native(
        &format!("Neuromorphic Network - {}", brain_id),
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(App::new(
                net_cfg,
                brain_id,
                ipc_enabled,
                distributed_node,
                remote_only,
                startup_snapshot_json,
                remote_workspace_binding,
                aer_cfg,
                runtime_handle,
            )))
        }),
    ) {
        return Err(anyhow::anyhow!(e.to_string()));
    }
    Ok(())
}

#[cfg(feature = "ui")]
enum GAControl {
    Stop,
    Pause,
    Resume,
}

#[cfg(feature = "ui")]
#[allow(dead_code)]
struct IpcStats {
    connected: bool,
    frame_count: u64,
    drop_count: u64,
    size_mismatch_count: u64,
    last_peer: Option<String>,
    last_receive_time: Option<std::time::Instant>,
    last_steps: usize,
    #[cfg(all(feature = "robot_io", unix))]
    last_handshake: Option<IpcHandshake>,
}

#[cfg(feature = "sysinfo")]
#[derive(Clone, Default)]
struct SysSnapshot {
    cpu_usage: f32,
    ram_usage_mb: f32,
    cpu_temp_c: Option<f32>,
    os_threads: u32,
    runnable_threads: u32,
    cpu_core_count: u32,
    hot_core_count: u32,
    hot_core_top: Vec<(usize, f32)>,
}

#[cfg(feature = "sysinfo")]
fn ui_hot_core_threshold_pct() -> f32 {
    static THRESHOLD: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("NM_UI_HOT_CORE_PCT")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(70.0)
            .clamp(1.0, 100.0)
    })
}

#[cfg(all(feature = "sysinfo", target_os = "linux"))]
fn read_linux_thread_counts() -> (u32, u32) {
    let mut os_threads = 0u32;
    let mut runnable_threads = 0u32;
    let entries = match std::fs::read_dir("/proc/self/task") {
        Ok(v) => v,
        Err(_) => return (0, 0),
    };
    for entry in entries.flatten() {
        os_threads = os_threads.saturating_add(1);
        let stat_path = entry.path().join("stat");
        let stat = match std::fs::read_to_string(stat_path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(close_idx) = stat.rfind(')') {
            let rest = stat.get(close_idx + 2..).unwrap_or("");
            if rest.starts_with('R') {
                runnable_threads = runnable_threads.saturating_add(1);
            }
        }
    }
    (os_threads, runnable_threads)
}

#[cfg(all(feature = "sysinfo", not(target_os = "linux")))]
fn read_linux_thread_counts() -> (u32, u32) {
    (0, 0)
}

#[cfg(feature = "ui")]
fn ga_pacing_label(pacing: bool, reason: &str) -> String {
    if !pacing {
        return "GA Pacing: No".to_string();
    }
    if reason.is_empty() {
        "GA Pacing: Yes".to_string()
    } else {
        format!("GA Pacing: Yes ({})", reason)
    }
}

#[cfg(feature = "ui")]
fn ga_ramp_label(population_size: usize, worker_cap: usize, sim_time_ms: f64) -> String {
    format!(
        "GA Ramp: pop {} | workers {} | sim {:.0} ms",
        population_size.max(1),
        worker_cap.max(1),
        sim_time_ms.max(1.0)
    )
}

#[cfg(feature = "ui")]
#[allow(dead_code)]
enum SimControl {
    SetPlaying(bool),
    SetProvider(Box<dyn SensoryProvider + Send>),
    ApplyConfig(crate::config::NetworkConfig),
    SetModel(NeuronModel),
    SetLearning(Learning),
    Reset,
    SetDt(f64),
    ResizeSensory(usize),
    ResizeOutput(usize),
    RecreateRunner(
        crate::config::LIFParams,
        crate::config::STDPParams,
        crate::config::NetworkConfig,
        NeuronModel,
        Learning,
    ),
    ImportNetwork(String),
    ImportNetworkWithReply(String, std::sync::mpsc::Sender<Result<(), String>>),
    SetStdpEta(f64),
    BindIpc(String, usize, usize),
    SetIpcNeuronsPerValue(usize),
    SetIpcThreshold(f32),
    SetFeedback(bool),
    Shutdown,
}

#[cfg(feature = "ui")]
enum ToolTaskResult {
    TfliteImport {
        path: std::path::PathBuf,
        json: Option<String>,
        stdout: String,
        stderr: String,
        error: Option<String>,
    },
    TflitePickCanceled,
    PythonResolved {
        result: Result<String, String>,
    },
    FileWrite {
        kind: FileTaskKind,
        path: std::path::PathBuf,
        error: Option<String>,
    },
    FileRead {
        kind: FileTaskKind,
        path: std::path::PathBuf,
        data: Option<String>,
        error: Option<String>,
    },
    ToolExport {
        kind: ToolExportKind,
        path: std::path::PathBuf,
        stdout: String,
        stderr: String,
        error: Option<String>,
    },
    ToolImport {
        kind: ImportKind,
        path: std::path::PathBuf,
        json: Option<String>,
        stdout: String,
        stderr: String,
        error: Option<String>,
    },
    RemoteTokenBalance {
        result: Result<TokenBalanceResponse, String>,
    },
}

#[cfg(feature = "ui")]
enum ClusterSnapshotMsg {
    Ok {
        network_id: String,
        node_id: String,
        snap: Box<crate::runner::Snapshot>,
    },
    Err {
        network_id: String,
        node_id: String,
        error: String,
    },
}

#[cfg(feature = "ui")]
struct EdgeCacheResult {
    edges: Vec<CachedEdge>,
    sizes: Vec<usize>,
    counts: Vec<usize>,
    output_count: usize,
    #[cfg(feature = "growth3d")]
    topo: Option<crate::topology::Topology3D>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    skull_membrane: Option<crate::morphology::SkullMembrane>,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum ImportKind {
    Standard,
    Tflite,
    Onnx,
    NeuroML,
    PyNN,
    Nir,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, Debug)]
enum ToolExportKind {
    Onnx,
    PyNN,
    Nir,
    NeuroML,
    Tflite,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, Debug)]
enum FileTaskKind {
    SaveConfig,
    LoadConfig,
    SaveNetwork,
    LoadNetwork,
    SaveProbes,
    LoadProbes,
}

#[cfg(feature = "ui")]
struct PendingImport {
    path: std::path::PathBuf,
    kind: ImportKind,
    stdout: String,
    stderr: String,
    rx: std::sync::mpsc::Receiver<Result<(), String>>,
    result: Option<Result<(), String>>,
}

#[cfg(feature = "ui")]
struct RemoteStatusSnapshot {
    nodes: HashMap<String, NodeStatus>,
    networks: HashMap<String, NetworkStatus>,
    last_error: Option<String>,
    last_update: std::time::Instant,
}

#[cfg(feature = "ui")]
enum RemoteStatusMsg {
    Update {
        addr: String,
        nodes: HashMap<String, NodeStatus>,
        networks: HashMap<String, NetworkStatus>,
    },
    Error {
        addr: String,
        error: String,
    },
}

#[cfg(feature = "ui")]
struct RemoteConnection {
    addr: String,
    stop: Arc<AtomicBool>,
}

#[cfg(feature = "ui")]
/// Top‑level UI state and cached drawing buffers.
///
/// The `runner` field holds the live simulation engine. Most fields are UI
/// state (camera, overlays, probes) or short‑term activity buffers used for
/// rendering.
struct App {
    brain_id: String,
    playing: bool,
    loop_feedback: bool,
    input_source: InputSource,
    http_aer_source_url: String,
    http_aer_base: u32,
    http_aer_status: Arc<RwLock<HttpAerInputStatus>>,
    sensory_count: usize,
    neuron_model: NeuronModelSel,
    izh_preset: IzhPreset,
    learning: LearningSel,
    status: String,
    remote_only: bool,
    // Engine (shared with background simulation thread)
    runner: Arc<RwLock<Runner>>,
    // Simulation Control
    sim_tx: std::sync::mpsc::Sender<SimControl>,
    playing_atomic: Arc<AtomicBool>,
    sim_throttle_ms: Arc<AtomicU32>,
    spectral_bands: Arc<RwLock<Vec<f32>>>,
    #[allow(dead_code)]
    ipc_stats: Arc<RwLock<IpcStats>>,
    // Longterm connection stats
    longterm_conn: usize,
    total_conn: usize,
    // simple RNG state for random spikes
    random_spike_probability: f32,
    mic_running: bool,
    // GA Search
    ga_search: Option<GASearch>,
    ga_running: bool,
    ga_panel_visible: bool,
    ga_best_fitness: f64,
    ga_mutation_rate: f64,
    ga_crossover_rate: f64,
    ga_use_dk_bias: bool,
    ga_pop_size: usize,
    ga_generations: usize,
    ga_sim_time_ms: f64,
    ga_rx: Option<std::sync::mpsc::Receiver<GASearch>>,
    ga_paused: bool,
    ga_live_preview: bool,
    #[allow(dead_code)]
    ga_leaderboard_idx: Option<usize>,
    ga_control_tx: Option<std::sync::mpsc::Sender<GAControl>>,
    ga_thread: Option<std::thread::JoinHandle<()>>,
    ga_pacing_ack: bool,
    ga_abort_cleanup_done: bool,
    #[cfg(feature = "webcam_input")]
    cam_running: bool,
    // EQ state (smoothed)
    smoothed_equalizer_values: Vec<f32>,
    output_count: usize,
    // Network view cache
    last_rendered_panel_size: egui::Vec2,
    sensory_positions: Vec<egui::Pos2>,
    hidden_positions: Vec<Vec<egui::Pos2>>, // per hidden layer
    output_positions: Vec<egui::Pos2>,
    network_layout: NetworkLayout,
    layout_auto: bool,
    // Activity buffers (0..1, exponential decay)
    sensory_activity: Vec<f32>,
    hidden_activity: Vec<Vec<f32>>, // per layer
    output_activity: Vec<f32>,
    // Raster inset (recent output spikes)
    raster_cols: usize,
    raster_outputs: std::collections::VecDeque<Vec<i8>>, // time-major columns, each length = num_output_neurons
    // View (camera) controls
    camera_zoom: f32,
    camera_yaw_degrees: f32,
    camera_pitch_degrees: f32,
    cam_pan: egui::Vec2,
    // Edge highlight options/state
    show_highlights: bool,
    max_highlight_lines: usize,
    last_sensory_spikes: Vec<i8>,
    // previous step hidden spikes (for backward highlighting timing)
    previous_hidden_spikes: Vec<Vec<i8>>, // per hidden layer
    // Backward highlighting + static overlays
    show_backward_highlights: bool,
    show_static_overlays: bool,
    overlay_density: usize,
    overlay_opacity: f32,
    // Edge interaction
    show_feedback_overlays: bool,
    edge_shapes: Vec<EdgeVisual>,
    // Growth (3D topology) toggle
    #[cfg(feature = "growth3d")]
    growth_enabled: bool,
    #[cfg(feature = "growth3d")]
    show_region_labels: bool,
    #[cfg(feature = "growth3d")]
    region_label_positions: Vec<(String, egui::Pos2, egui::Pos2)>,
    // Camera rotation pivot in world space (computed as the average center of neurons)
    #[cfg(feature = "growth3d")]
    cam_pivot_world: (f32, f32, f32),
    #[cfg(feature = "growth3d")]
    cam_pivot_pid: UiPid3State,
    #[cfg(feature = "growth3d")]
    topo_pid_sensory: Vec<UiPid2State>,
    #[cfg(feature = "growth3d")]
    topo_pid_hidden: Vec<Vec<UiPid2State>>,
    #[cfg(feature = "growth3d")]
    topo_pid_output: Vec<UiPid2State>,
    #[cfg(feature = "growth3d")]
    region_label_states: crate::morphology::FastHashMap<String, egui::Pos2>,
    #[cfg(feature = "growth3d")]
    region_label_target_states: crate::morphology::FastHashMap<String, egui::Pos2>,
    // Morphology overlays (optional)
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    show_morpho_overlays: bool,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    morpho_opacity: f32,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    show_transmissions: bool,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    transmissions_opacity: f32,
    show_equalizer: bool,
    // Oscilloscope / probes
    probes: Vec<Probe>,
    next_probe_id: u32,
    scope_time_ms: f32,
    scope_gain: f32,
    scope_lanes: bool,
    scope_grid: bool,
    scope_paused: bool,
    // Python tools configuration (optional override)
    python_path: Option<String>,
    // UI: temporarily suppress consolidated tooltip after inline actions
    tooltip_suppression_counter: u8,
    // Tooltip pinning state
    tooltip_pinned: bool,
    tooltip_pinned_pos: egui::Pos2,
    tooltip_pinned_lines: Vec<String>,
    tooltip_pinned_target: Option<ContextPick>,
    // One-time gate to apply AARNN biologically plausible defaults when AARNN is first selected
    aarnn_defaults_applied: bool,
    // Track consecutive "Boost Connectivity" clicks without success
    #[allow(dead_code)]
    boost_connectivity_count: usize,
    // Local copy of config for UI editing
    local_net: NetworkConfig,
    // FPAA discovery / routing status for the local host
    fpaa_status: FpaaRuntimeStatus,
    fpaa_last_refresh: Instant,
    fpaa_last_signature: String,
    // External IPC (UDS) state (only when robot_io feature is on)
    #[cfg(all(feature = "robot_io", unix))]
    ipc_sock_path: String,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_connected: bool,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_last_peer: Option<String>,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_last_receive_time: Option<std::time::Instant>,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_packet_drop_count: u64,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_size_mismatch_count: u64,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_threshold: f32,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_neurons_per_value: usize,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_bias_last_sensory_input: bool,
    #[allow(dead_code)]
    #[cfg(all(feature = "robot_io", unix))]
    ipc_sync: crate::bridge::TimeSync,
    #[allow(dead_code)]
    #[cfg(all(feature = "robot_io", unix))]
    quantizer: Quantizer,
    #[allow(dead_code)]
    #[cfg(all(feature = "robot_io", unix))]
    last_sensory_inputs_f32: Vec<f32>,
    #[allow(dead_code)]
    #[cfg(all(feature = "robot_io", unix))]
    last_actuator_outputs_f32: Vec<f32>,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_mapping: Option<IoMapping>,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_frame_count: u64,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_last_steps: usize,
    #[cfg(all(feature = "robot_io", unix))]
    ipc_last_handshake: Option<IpcHandshake>,
    // Resource monitoring
    #[cfg(feature = "sysinfo")]
    sys_snapshot: Arc<RwLock<SysSnapshot>>,
    #[cfg(feature = "sysinfo")]
    sys_stop: Arc<AtomicBool>,
    #[cfg(feature = "sysinfo")]
    cpu_temp_c: Option<f32>,
    cpu_usage: f32,
    ram_usage_mb: f32,
    hot_core_threshold_pct: f32,
    os_threads: u32,
    runnable_threads: u32,
    cpu_core_count: u32,
    hot_core_count: u32,
    hot_core_top: Vec<(usize, f32)>,
    // Timing and responsiveness
    #[allow(dead_code)]
    last_step_duration: std::time::Duration,
    avg_step_time_ms: f32,
    auto_dt_enabled: bool,
    responsiveness_target_ms: f32,
    last_longterm_update: std::time::Instant,
    // Pop-out window state
    show_neuron_detail: bool,
    selected_neuron_pick: Option<ContextPick>,
    detail_camera_zoom: f32,
    detail_camera_yaw: f32,
    detail_camera_pitch: f32,
    detail_cam_pan: egui::Vec2,
    detail_camera_pos: [f32; 3],
    detail_bio_orient: DetailBioOrient,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_enabled: bool,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_kp: f32,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_ki: f32,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_kd: f32,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_axon: Vec<Pid3State>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_dend: Vec<Pid3State>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    detail_bouton_pid_pick: Option<ContextPick>,
    detail_timescale: f32,
    detail_time_offset: f32,
    detail_paused: bool,
    detail_last_neuron: Option<ContextPick>,
    detail_waiting_for_activation: bool,
    // UI rate control
    #[allow(dead_code)]
    last_ui_render_time: std::time::Instant,
    // Latest sensory spikes captured by sim thread (avoids read-lock contention).
    sensory_spikes_snapshot: Arc<RwLock<Vec<i8>>>,
    // Full UI snapshot to avoid runner lock contention.
    ui_snapshot: Arc<RwLock<UiSnapshot>>,
    // Sim diagnostics for input flow.
    sim_step_counter: Arc<AtomicU64>,
    sim_last_spike_count: Arc<AtomicU64>,
    sim_last_spike_len: Arc<AtomicU64>,
    runtime_handle: tokio::runtime::Handle,
    remote_workspace_binding: Option<RemoteWorkspaceBinding>,
    remote_token_balance: Option<TokenBalanceResponse>,
    remote_token_error: Option<String>,
    remote_token_last_refresh: Option<Instant>,
    remote_token_refresh_inflight: bool,
    // Distributed state
    distributed_node: Option<DistributedNode>,
    view_source: ViewSource,
    view_node_filter: Option<String>,
    remote_addr_input: String,
    remote_connections: Vec<RemoteConnection>,
    remote_status_tx: std::sync::mpsc::Sender<RemoteStatusMsg>,
    remote_status_rx: std::sync::mpsc::Receiver<RemoteStatusMsg>,
    remote_statuses: HashMap<String, RemoteStatusSnapshot>,
    tool_task_tx: std::sync::mpsc::Sender<ToolTaskResult>,
    tool_task_rx: std::sync::mpsc::Receiver<ToolTaskResult>,
    edge_cache_rx: std::sync::mpsc::Receiver<EdgeCacheResult>,
    edge_cache_res_tx: std::sync::mpsc::Sender<EdgeCacheResult>,
    edge_cache_inflight: bool,
    pending_import: Option<PendingImport>,
    last_import_report: Option<String>,
    tflite_import_mode: TfliteImportMode,
    tflite_allow_fallback: bool,
    tflite_allow_large: bool,
    tflite_max_layers: usize,
    tflite_max_params: usize,
    tflite_freeze_learning: bool,
    tflite_sim_throttle_ms: u32,
    force_show_connections: bool,
    pending_edge_cache: bool,
    edge_cache_refresh_ms: u64,
    last_edge_cache_refresh: std::time::Instant,
    cached_edges: Vec<CachedEdge>,
    cached_layer_sizes: Vec<usize>,
    cached_conn_counts: Vec<usize>,
    cached_output_conn_count: Option<usize>,
    #[cfg(feature = "growth3d")]
    cached_edge_topo: Option<crate::topology::Topology3D>,
    #[cfg(all(feature = "morpho", feature = "growth3d"))]
    cached_skull_membrane: Option<crate::morphology::SkullMembrane>,
    conn_stats_refresh_ms: u64,
    last_conn_stats_refresh: std::time::Instant,
    last_layout_recompute: std::time::Instant,
    cluster_snapshot_tx: std::sync::mpsc::Sender<ClusterSnapshotMsg>,
    cluster_snapshot_rx: std::sync::mpsc::Receiver<ClusterSnapshotMsg>,
    cluster_snapshot_inflight: bool,
    cluster_snapshot_last_fetch: Option<std::time::Instant>,
    cluster_snapshot_network_id: Option<String>,
    cluster_snapshot_node_id: Option<String>,
    cluster_snapshot_cache: Option<Box<crate::runner::Snapshot>>,
    #[cfg(feature = "growth3d")]
    cluster_topo_cache: Option<crate::topology::Topology3D>,
    // Distributed state cache to prevent UI flicker when locks are busy
    dist_is_orchestrator: bool,
    dist_node_id: String,
    dist_nodes: HashMap<String, NodeStatus>,
    dist_network_registry: HashMap<String, NetworkStatus>,
    dist_local_playing_cache: HashMap<String, bool>,
    dist_initial_view_selected: bool,
    // hull cache for UI rendering
    #[cfg(feature = "growth3d")]
    cached_skull_hull: Vec<egui::Pos2>,
    #[cfg(feature = "growth3d")]
    last_hull_update: std::time::Instant,
    // Serialization cache for distributed sync
    last_synced_config: Option<crate::config::NetworkConfig>,
    last_config_json: String,
    last_ga_best_config: Option<crate::config::NetworkConfig>,
    last_ga_best_config_json: String,
    // Startup defaults for Reset.
    initial_net_cfg: NetworkConfig,
    initial_lif: LIFParams,
    initial_stdp: STDPParams,
    initial_model: NeuronModel,
    initial_learning: Learning,
}

#[cfg(feature = "ui")]
impl App {
    #[allow(dead_code)]
    fn runner_try_r(&self) -> Option<tokio::sync::RwLockReadGuard<'_, Runner>> {
        self.runner.try_read().ok()
    }

    fn log_ui_memory_snapshot(&self, reason: &str) {
        let cached_edges_bytes =
            (self.cached_edges.len() * std::mem::size_of::<CachedEdge>()) as u64;
        let edge_shapes_bytes = (self.edge_shapes.len() * std::mem::size_of::<EdgeVisual>()) as u64;
        let raster_bytes: u64 = self.raster_outputs.iter().map(|v| v.len() as u64).sum();
        let sensory_bytes = self.sensory_activity.len() as u64 * std::mem::size_of::<f32>() as u64;
        let hidden_bytes: u64 = self
            .hidden_activity
            .iter()
            .map(|v| v.len() as u64)
            .sum::<u64>()
            * std::mem::size_of::<f32>() as u64;
        let output_bytes = self.output_activity.len() as u64 * std::mem::size_of::<f32>() as u64;
        let prev_hidden_bytes: u64 = self
            .previous_hidden_spikes
            .iter()
            .map(|v| v.len() as u64)
            .sum();
        let last_sensory_bytes = self.last_sensory_spikes.len() as u64;
        nm_err!(
            "[warn] UI mem snapshot ({}): cached_edges={} (~{}MB) edge_shapes={} (~{}MB) raster_cols={} raster_bytes~{}MB activity_bytes~{}MB prev_hidden_bytes~{}MB last_sensory_bytes~{}MB.",
            reason,
            self.cached_edges.len(),
            cached_edges_bytes / 1024 / 1024,
            self.edge_shapes.len(),
            edge_shapes_bytes / 1024 / 1024,
            self.raster_outputs.len(),
            raster_bytes / 1024 / 1024,
            (sensory_bytes + hidden_bytes + output_bytes) / 1024 / 1024,
            prev_hidden_bytes / 1024 / 1024,
            last_sensory_bytes / 1024 / 1024
        );
        if let Ok(r) = self.runner.try_read() {
            let total_neurons = r.total_neurons();
            let total_conn =
                r.connection_counts().iter().sum::<usize>() + r.output_connection_count();
            nm_err!(
                "[warn] UI mem snapshot ({}): runner neurons {} conns {} layers {} hidden_per_layer {}.",
                reason,
                total_neurons,
                total_conn,
                r.net.num_hidden_layers,
                r.net.num_hidden_per_layer_initial
            );
        }
    }

    #[cfg(feature = "growth3d")]
    fn reset_topology_pid_states(&mut self) {
        self.cam_pivot_pid = UiPid3State::default();
        self.topo_pid_sensory.clear();
        self.topo_pid_hidden.clear();
        self.topo_pid_output.clear();
        self.region_label_states.clear();
        self.region_label_target_states.clear();
        self.region_label_positions.clear();
    }

    fn clear_ui_caches(&mut self) {
        self.cached_edges.clear();
        self.cached_edges.shrink_to_fit();
        self.cached_layer_sizes.clear();
        self.cached_layer_sizes.shrink_to_fit();
        self.cached_conn_counts.clear();
        self.cached_conn_counts.shrink_to_fit();
        self.cached_output_conn_count = None;
        #[cfg(feature = "growth3d")]
        {
            self.cached_edge_topo = None;
            self.reset_topology_pid_states();
        }
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            self.cached_skull_membrane = None;
        }
        self.edge_shapes.clear();
        self.edge_shapes.shrink_to_fit();
        self.raster_outputs.clear();
        self.sensory_activity.clear();
        self.hidden_activity.clear();
        self.output_activity.clear();
        self.previous_hidden_spikes.clear();
        self.last_sensory_spikes.clear();
        if let Ok(mut bands) = self.spectral_bands.try_write() {
            bands.clear();
            bands.shrink_to_fit();
        }
        if let Ok(mut snap) = self.ui_snapshot.try_write() {
            *snap = UiSnapshot::default();
        }
    }

    fn reap_finished_ga_thread(&mut self) {
        let finished = self
            .ga_thread
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(false);
        if finished {
            if let Some(handle) = self.ga_thread.take() {
                let _ = handle.join();
            }
        }
    }
    /// Display layer/connection summary including longterm connections
    #[allow(dead_code)]
    fn show_layer_connection_summary(&self, ui: &mut egui::Ui) {
        ui.label(format!(
            "Longterm connections: {} / {} ({:.2}%)",
            self.longterm_conn,
            self.total_conn,
            if self.total_conn > 0 {
                100.0 * (self.longterm_conn as f64) / (self.total_conn as f64)
            } else {
                0.0
            }
        ));
        // TODO: Add more layer/connection summary info here as needed
    }
    #[allow(dead_code)]
    fn get_incoming_synapses(
        &self,
        runner: &Runner,
        pick: ContextPick,
    ) -> Vec<(isize, usize, usize)> {
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            let mut res = Vec::new();
            match pick {
                ContextPick::Hidden(l, j) => {
                    if l == 0 {
                        if j < runner.recv_in.len() {
                            for &(pre_id, syn_idx) in &runner.recv_in[j] {
                                res.push((-1, pre_id, syn_idx));
                            }
                        }
                    } else if l > 0 && (l - 1) < runner.recv_fwd.len() {
                        if j < runner.recv_fwd[l - 1].len() {
                            for &(pre_id, syn_idx) in &runner.recv_fwd[l - 1][j] {
                                res.push((l as isize - 1, pre_id, syn_idx));
                            }
                        }
                    }
                    if l < runner.recv_bwd.len() {
                        if j < runner.recv_bwd[l].len() {
                            for &(pre_id, syn_idx) in &runner.recv_bwd[l][j] {
                                res.push((l as isize + 1, pre_id, syn_idx));
                            }
                        }
                    }
                }
                ContextPick::Output(k) => {
                    let l_last = runner.net.num_hidden_layers as isize - 1;
                    if k < runner.recv_out.len() {
                        for &(pre_id, syn_idx) in &runner.recv_out[k] {
                            res.push((l_last, pre_id, syn_idx));
                        }
                    }
                }
                _ => {}
            }
            res
        }
        #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
        {
            let _ = runner;
            let _ = pick;
            Vec::new()
        }
    }

    fn find_activations(&self, runner: &Runner, pick: ContextPick) -> Vec<usize> {
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            let mut acts = Vec::new();
            let incoming = self.get_incoming_synapses(runner, pick);
            for (pre_l, pre_id, syn_idx) in incoming {
                let (steps_delay, _) = runner.syn_delay_and_atten(syn_idx);
                if pre_l < 0 {
                    for (offset, frame) in runner.spk_hist_s.iter().enumerate() {
                        if pre_id < frame.len() && frame[pre_id] != 0 {
                            if offset >= steps_delay {
                                acts.push(offset - steps_delay);
                            }
                        }
                    }
                } else {
                    if let Some(dq) = runner.spk_hist_h.get(pre_l as usize) {
                        for (offset, frame) in dq.iter().enumerate() {
                            if pre_id < frame.len() && frame[pre_id] != 0 {
                                if offset >= steps_delay {
                                    acts.push(offset - steps_delay);
                                }
                            }
                        }
                    }
                }
            }
            acts.sort_unstable();
            acts.dedup();
            acts
        }
        #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
        {
            let _ = runner;
            let _ = pick;
            Vec::new()
        }
    }

    fn new(
        net_cfg: crate::config::NetworkConfig,
        brain_id: String,
        _ipc_enabled: bool,
        distributed_node: Option<DistributedNode>,
        remote_only: bool,
        startup_snapshot_json: Option<String>,
        remote_workspace_binding: Option<RemoteWorkspaceBinding>,
        aer_cfg: Option<AerIoConfig>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        let lif = LIFParams::default();
        let stdp = STDPParams::default();
        let initial_lif = lif.clone();
        let initial_stdp = stdp.clone();
        let initial_model = NeuronModel::Aarnn;
        let initial_learning = Learning::Aarnn;
        let mut runner = Runner::new(lif, stdp, net_cfg.clone(), initial_model, initial_learning);
        if let Some(json) = startup_snapshot_json.as_ref() {
            if let Err(e) = runner.import_network_json(json) {
                nm_err!("[warn] Startup snapshot import failed: {}", e);
            }
        }
        let initial_net_cfg = runner.net.clone();
        let local_net = runner.net.clone();
        let fpaa_signature = serde_json::to_string(&local_net.fpaa).unwrap_or_default();
        let fpaa_status = crate::fpaa::startup_probe(&local_net.fpaa);
        let n_s = runner.net.num_sensory_neurons;
        // allocate activity buffers
        let hidden_layer_sizes: Vec<usize> = (0..runner.net.num_hidden_layers)
            .map(|li| runner.layer_size(li).max(1))
            .collect();
        let l = hidden_layer_sizes.len();
        let o = runner.net.num_output_neurons;
        let act_h = hidden_layer_sizes
            .iter()
            .map(|&h| vec![0.0f32; h])
            .collect();
        let prev_spk_h = hidden_layer_sizes.iter().map(|&h| vec![0i8; h]).collect();
        let runner = Arc::new(RwLock::new(runner));
        let (sim_tx, sim_rx) = std::sync::mpsc::channel::<SimControl>();
        let playing_atomic = Arc::new(AtomicBool::new(false));
        let spectral_bands = Arc::new(RwLock::new(Vec::new()));
        let sensory_spikes_snapshot = Arc::new(RwLock::new(Vec::new()));
        let ui_snapshot = Arc::new(RwLock::new(UiSnapshot::default()));
        let sim_step_counter = Arc::new(AtomicU64::new(0));
        let sim_last_spike_count = Arc::new(AtomicU64::new(0));
        let sim_last_spike_len = Arc::new(AtomicU64::new(0));
        let (remote_status_tx, remote_status_rx) = std::sync::mpsc::channel::<RemoteStatusMsg>();
        let sim_throttle_ms = Arc::new(AtomicU32::new(0));
        let (tool_task_tx, tool_task_rx) = std::sync::mpsc::channel::<ToolTaskResult>();
        let (edge_cache_res_tx, edge_cache_rx) = std::sync::mpsc::channel::<EdgeCacheResult>();
        let (cluster_snapshot_tx, cluster_snapshot_rx) =
            std::sync::mpsc::channel::<ClusterSnapshotMsg>();
        let ipc_stats = Arc::new(RwLock::new(IpcStats {
            connected: false,
            frame_count: 0,
            drop_count: 0,
            size_mismatch_count: 0,
            last_peer: None,
            last_receive_time: None,
            last_steps: 0,
            #[cfg(all(feature = "robot_io", unix))]
            last_handshake: None,
        }));

        let sim_runner = runner.clone();
        let sim_playing = playing_atomic.clone();
        let sim_spectral = spectral_bands.clone();
        let sim_sensory_snapshot = sensory_spikes_snapshot.clone();
        let sim_ui_snapshot = ui_snapshot.clone();
        let sim_step_counter_thread = sim_step_counter.clone();
        let sim_last_spike_count_thread = sim_last_spike_count.clone();
        let sim_last_spike_len_thread = sim_last_spike_len.clone();
        let sim_remote_only = remote_only;
        let sim_throttle = sim_throttle_ms.clone();
        let sim_idle_sleep_ms = std::env::var("NM_SIM_IDLE_SLEEP_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(16)
            .clamp(1, 1000);
        let sim_ipc_idle_sleep_ms = std::env::var("NM_SIM_IPC_IDLE_SLEEP_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1)
            .clamp(0, 100);
        let sim_remote_idle_sleep_ms = std::env::var("NM_SIM_REMOTE_IDLE_SLEEP_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(25)
            .clamp(1, 1000);
        let sim_batch_steps = std::env::var("NM_SIM_BATCH_STEPS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 16);
        let edge_cache_refresh_ms = std::env::var("NM_UI_EDGE_CACHE_REFRESH_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600)
            .clamp(100, 5_000);
        let conn_stats_refresh_ms = std::env::var("NM_UI_CONN_STATS_REFRESH_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(900)
            .clamp(100, 10_000);
        #[cfg(all(feature = "robot_io", unix))]
        let sim_ipc_stats = ipc_stats.clone();
        let mut sim_provider: Box<dyn SensoryProvider + Send> =
            Box::new(RandomProvider::new(n_s, 0.02));
        let ipc_aer_cfg = aer_cfg.clone().unwrap_or_default();
        let ipc_aer_sensory_base = ipc_aer_cfg.sensory_base;
        let ipc_aer_output_base = ipc_aer_cfg.output_base;
        let mut aer_link = aer_cfg.and_then(|cfg| AerLink::bind(cfg).ok());

        #[cfg(all(feature = "robot_io", unix))]
        let mut sim_ipc_server: Option<IpcUdsServer> = None;

        std::thread::Builder::new()
            .name("simulation".into())
            .spawn(move || {
                // Keep simulation controller thread unpinned so the scheduler can
                // spread its control-path load instead of concentrating it on one core.
                let mut ipc_seed_rng = rand::rng();
                let mut ipc_spike_rng = rand::rngs::StdRng::from_rng(&mut ipc_seed_rng);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_celegans_graded_output = std::env::var("NM_IPC_CELEGANS_GRADED_OUTPUT")
                    .ok()
                    .map(|v| {
                        matches!(
                            v.trim().to_ascii_lowercase().as_str(),
                            "1" | "true" | "yes" | "on"
                        )
                    })
                    .unwrap_or(true);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_celegans_output_gain = std::env::var("NM_IPC_CELEGANS_OUTPUT_GAIN")
                    .ok()
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.95)
                    .clamp(0.05, 8.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_celegans_output_current_gain =
                    std::env::var("NM_IPC_CELEGANS_OUTPUT_CURRENT_GAIN")
                        .ok()
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.35)
                        .clamp(0.01, 8.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_celegans_output_current_mix =
                    std::env::var("NM_IPC_CELEGANS_OUTPUT_CURRENT_MIX")
                        .ok()
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.35)
                        .clamp(0.0, 1.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_non_celegans_graded_output =
                    std::env::var("NM_IPC_NON_CELEGANS_GRADED_OUTPUT")
                        .ok()
                        .map(|v| {
                            matches!(
                                v.trim().to_ascii_lowercase().as_str(),
                                "1" | "true" | "yes" | "on"
                            )
                        })
                        .unwrap_or(true);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_dros_input_rate_gain = std::env::var("NM_IPC_DROS_INPUT_RATE_GAIN")
                    .ok()
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.34)
                    .clamp(0.0, 2.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_nao_input_rate_gain = std::env::var("NM_IPC_NAO_INPUT_RATE_GAIN")
                    .ok()
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.18)
                    .clamp(0.0, 2.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_dros_output_gain = std::env::var("NM_IPC_DROS_OUTPUT_GAIN")
                    .ok()
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.82)
                    .clamp(0.05, 8.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_dros_output_current_gain =
                    std::env::var("NM_IPC_DROS_OUTPUT_CURRENT_GAIN")
                        .ok()
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.22)
                        .clamp(0.01, 8.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_dros_output_current_mix =
                    std::env::var("NM_IPC_DROS_OUTPUT_CURRENT_MIX")
                        .ok()
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.18)
                        .clamp(0.0, 1.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_nao_output_gain = std::env::var("NM_IPC_NAO_OUTPUT_GAIN")
                    .ok()
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.92)
                    .clamp(0.05, 8.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_nao_output_current_gain =
                    std::env::var("NM_IPC_NAO_OUTPUT_CURRENT_GAIN")
                        .ok()
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.42)
                        .clamp(0.01, 8.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_nao_output_current_mix =
                    std::env::var("NM_IPC_NAO_OUTPUT_CURRENT_MIX")
                        .ok()
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.38)
                        .clamp(0.0, 1.0);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_celegans_debug_interval =
                    std::env::var("NM_IPC_CELEGANS_DEBUG_INTERVAL")
                        .ok()
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0)
                        .min(100_000);
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_input_encoding = ProfileInputEncoding {
                    drosophila_rate: crate::spike_io::encoding::RateEncoding {
                        low_gain: ipc_dros_input_rate_gain,
                        quiet_floor: 0.002,
                        ..crate::spike_io::encoding::RateEncoding::default()
                    },
                    nao_rate: crate::spike_io::encoding::RateEncoding {
                        low_gain: ipc_nao_input_rate_gain,
                        quiet_floor: 0.001,
                        ..crate::spike_io::encoding::RateEncoding::default()
                    },
                    ..ProfileInputEncoding::default()
                };
                #[cfg(all(feature = "robot_io", unix))]
                let ipc_output_encoding = ProfileOutputEncoding {
                    celegans_graded_output: ipc_celegans_graded_output,
                    non_celegans_graded_output: ipc_non_celegans_graded_output,
                    celegans_output_gain: ipc_celegans_output_gain,
                    celegans_output_current_gain: ipc_celegans_output_current_gain,
                    celegans_output_current_mix: ipc_celegans_output_current_mix,
                    drosophila_output_gain: ipc_dros_output_gain,
                    drosophila_output_current_gain: ipc_dros_output_current_gain,
                    drosophila_output_current_mix: ipc_dros_output_current_mix,
                    nao_output_gain: ipc_nao_output_gain,
                    nao_output_current_gain: ipc_nao_output_current_gain,
                    nao_output_current_mix: ipc_nao_output_current_mix,
                };
                #[cfg(all(feature = "robot_io", unix))]
                let mut ipc_celegans_debug_counter: usize = 0;
                #[cfg(all(feature = "robot_io", unix))]
                let mut ipc_last_reply_values: Vec<f32> = Vec::new();
                loop {
                    // 1. Process all pending control messages
                    while let Ok(msg) = sim_rx.try_recv() {
                        match msg {
                            SimControl::SetPlaying(p) => {
                                sim_playing.store(p, Ordering::SeqCst);
                            }
                            SimControl::SetProvider(p) => {
                                sim_provider.stop();
                                sim_provider = p;
                                if let Ok(r) = sim_runner.try_read() {
                                    sim_provider.set_dt(r.lif.dt as f32);
                                }
                            }
                            SimControl::ApplyConfig(cfg) => {
                                sim_runner.blocking_write().apply_config(cfg);
                            }
                            SimControl::SetModel(m) => {
                                sim_runner.blocking_write().set_model(m);
                            }
                            SimControl::SetLearning(l) => {
                                sim_runner.blocking_write().set_learning(l);
                            }
                            SimControl::Reset => {
                                sim_runner.blocking_write().reset();
                            }
                            SimControl::SetDt(dt) => {
                                sim_runner.blocking_write().set_dt(dt);
                                sim_provider.set_dt(dt as f32);
                            }
                            SimControl::ResizeSensory(n) => {
                                sim_runner.blocking_write().resize_sensory(n);
                                sim_provider.set_num_sensory_neurons(n);
                            }
                            SimControl::ResizeOutput(n) => {
                                sim_runner.blocking_write().resize_output(n);
                            }
                            SimControl::SetFeedback(f) => {
                                sim_runner.blocking_write().feedback_enabled = f;
                            }
                            SimControl::RecreateRunner(lif, stdp, net, model, learning) => {
                                *sim_runner.blocking_write() =
                                    Runner::new(lif, stdp, net, model, learning);
                            }
                            SimControl::ImportNetwork(json) => {
                                let _ = sim_runner.blocking_write().import_network_json(&json);
                                if let Ok(r) = sim_runner.try_read() {
                                    sim_provider.set_num_sensory_neurons(r.net.num_sensory_neurons);
                                }
                            }
                            SimControl::ImportNetworkWithReply(json, tx) => {
                                let res = sim_runner.blocking_write().import_network_json(&json);
                                if let Ok(ref r) = sim_runner.try_read() {
                                    sim_provider.set_num_sensory_neurons(r.net.num_sensory_neurons);
                                    nm_log!(
                                        "[import] network apply result: {:?} (S={} H={} O={})",
                                        res.as_ref().map(|_| ()).map_err(|e| e.to_string()),
                                        r.net.num_sensory_neurons,
                                        r.net.num_hidden_layers,
                                        r.net.num_output_neurons
                                    );
                                }
                                let _ = tx.send(res.map_err(|e| e.to_string()));
                            }
                            SimControl::SetStdpEta(eta) => {
                                sim_runner.blocking_write().stdp.eta = eta;
                            }
                            #[cfg(all(feature = "robot_io", unix))]
                            SimControl::BindIpc(path, s, o) => {
                                match IpcUdsServer::bind(
                                    &path,
                                    s,
                                    o,
                                    ipc_aer_sensory_base,
                                    ipc_aer_output_base,
                                ) {
                                    Ok(srv) => {
                                        sim_ipc_server = Some(srv);
                                    }
                                    Err(_) => {
                                        sim_ipc_server = None;
                                    }
                                }
                            }
                            SimControl::Shutdown => {
                                return;
                            }
                            _ => {}
                        }
                    }

                    if sim_remote_only {
                        std::thread::sleep(std::time::Duration::from_millis(
                            sim_remote_idle_sleep_ms,
                        ));
                        continue;
                    }

                    if let Some(link) = aer_link.as_mut() {
                        link.poll();
                    }

                    // 2. Perform simulation step if playing or IPC frame available
                    #[allow(unused_mut)]
                    let mut spikes_source = None;
                    #[allow(unused_mut)]
                    let mut ipc_dt = None;
                    #[allow(unused_mut)]
                    let mut reward_source: Option<f32> = None;

                    #[cfg(all(feature = "robot_io", unix))]
                    if let Some(ref mut srv) = sim_ipc_server {
                        let mut inputs = vec![0.0f32; srv.s];
                        if let Some(ev) = srv.poll_next_event(&mut inputs) {
                            match ev {
                                IpcEvent::Data(t_ms, reward, peer) => {
                                    let mut spk = vec![0i8; srv.s];
                                    let (io_cfg, io_profile, encode_ctx) =
                                        if let Ok(r) = sim_runner.try_read() {
                                            let io_cfg = r.net.spike_io.clone();
                                            let dt_ms = if t_ms > 0.0 {
                                                t_ms as f32
                                            } else {
                                                r.lif.dt.max(0.001) as f32
                                            };
                                            let step_index =
                                                (r.t_ms / r.lif.dt.max(0.001)).round().max(0.0)
                                                    as usize;
                                            let io_profile = resolve_network_io_profile(
                                                io_cfg.profile,
                                                srv.s,
                                                srv.o,
                                            );
                                            (
                                                io_cfg,
                                                io_profile,
                                                TemporalEncodingContext {
                                                    step_index,
                                                    time_ms: r.t_ms as f32,
                                                    dt_ms,
                                                },
                                            )
                                        } else {
                                            let io_cfg = SpikeIoConfig::default();
                                            let io_profile = resolve_network_io_profile(
                                                io_cfg.profile,
                                                srv.s,
                                                srv.o,
                                            );
                                            (
                                                io_cfg,
                                                io_profile,
                                                TemporalEncodingContext {
                                                    step_index: 0,
                                                    time_ms: 0.0,
                                                    dt_ms: if t_ms > 0.0 { t_ms as f32 } else { 1.0 },
                                                },
                                            )
                                        };
                                    if matches!(
                                        io_cfg.input_strategy,
                                        SpikeInputEncodingStrategy::ProfileDefault
                                    ) {
                                        encode_profile_inputs_with(
                                            io_profile,
                                            &inputs,
                                            &mut spk,
                                            || ipc_spike_rng.random::<f32>(),
                                            &ipc_input_encoding,
                                        );
                                    } else {
                                        encode_network_inputs_with(
                                            &io_cfg,
                                            srv.s,
                                            srv.o,
                                            &inputs,
                                            &mut spk,
                                            || ipc_spike_rng.random::<f32>(),
                                            encode_ctx,
                                        );
                                    }
                                    spikes_source = Some(spk);
                                    if t_ms > 0.0 {
                                        ipc_dt = Some(t_ms as f64);
                                    }
                                    reward_source = Some(reward);
                                    if let Ok(mut stats) = sim_ipc_stats.try_write() {
                                        stats.connected = true;
                                        stats.frame_count += 1;
                                        stats.last_peer = peer; // peer is already Option<String>
                                        stats.last_receive_time = Some(std::time::Instant::now());
                                    }
                                }
                                IpcEvent::Config(hs, peer) => {
                                    let (new_s, new_o) =
                                        resolve_ipc_handshake_sizes(&hs, srv.s, srv.o);
                                    let resized = new_s != srv.s || new_o != srv.o;
                                    if resized {
                                        nm_err!(
                                            "[IpcUdsServer] Applying handshake resize S={} O={}",
                                            new_s,
                                            new_o
                                        );
                                        srv.s = new_s;
                                        srv.o = new_o;
                                        sim_runner.blocking_write().resize_sensory(new_s);
                                        sim_runner.blocking_write().resize_output(new_o);
                                        sim_provider.set_num_sensory_neurons(new_s);
                                    }
                                    srv.send_size_hint_to_last_peer();
                                    if let Ok(mut stats) = sim_ipc_stats.try_write() {
                                        stats.connected = true;
                                        stats.last_peer = peer;
                                        stats.last_receive_time = Some(std::time::Instant::now());
                                        stats.last_handshake = Some(hs);
                                        if resized {
                                            stats.size_mismatch_count =
                                                stats.size_mismatch_count.saturating_add(1);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if !sim_playing.load(Ordering::SeqCst) && spikes_source.is_none() {
                        let mut idle_sleep_ms = sim_idle_sleep_ms;
                        #[cfg(all(feature = "robot_io", unix))]
                        {
                            if sim_ipc_server.is_some() {
                                // Keep IPC round-trip latency low when Webots is driving the
                                // simulation via UDS, even if playback is paused in the UI.
                                idle_sleep_ms = sim_ipc_idle_sleep_ms;
                            }
                        }
                        if idle_sleep_ms > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(idle_sleep_ms));
                        } else {
                            std::thread::yield_now();
                        }
                        continue;
                    }

                    if sim_playing.load(Ordering::SeqCst) || spikes_source.is_some() {
                        let batch_steps: usize = {
                            let r = sim_runner.blocking_read();
                            let many_layers = r.net.num_hidden_layers > 32;
                            let many_neurons = r.total_neurons() > 5000;
                            if many_layers || many_neurons {
                                1
                            } else {
                                sim_batch_steps
                            }
                        };
                        if spikes_source.is_some() || ipc_dt.is_some() {
                            #[cfg(all(feature = "robot_io", unix))]
                            let mut ipc_reply_values: Option<Vec<f32>> = None;
                            let spikes = if let Some(s) = spikes_source {
                                s
                            } else {
                                sim_provider.next_spikes()
                            };
                            let mut spikes = spikes;
                            sim_step_counter_thread.fetch_add(1, Ordering::Relaxed);
                            sim_last_spike_count_thread.store(
                                spikes.iter().filter(|&&v| v != 0).count() as u64,
                                Ordering::Relaxed,
                            );
                            sim_last_spike_len_thread.store(spikes.len() as u64, Ordering::Relaxed);
                            if let Ok(mut snap) = sim_sensory_snapshot.try_write() {
                                *snap = spikes.clone();
                            }
                            if let Some(bands) = sim_provider.last_bands() {
                                if let Ok(mut b) = sim_spectral.try_write() {
                                    *b = bands.to_vec();
                                }
                            }
                            if let Ok(mut r) = sim_runner.try_write() {
                                r.external_reward = reward_source.unwrap_or(0.0);
                                if let Some(link) = aer_link.as_mut() {
                                    let start_us = (r.t_ms * 1000.0) as u64;
                                    let end_us = ((r.t_ms + r.lif.dt) * 1000.0) as u64;
                                    let aer_spikes =
                                        link.sensory_spikes(start_us, end_us, spikes.len());
                                    for (dst, src) in spikes.iter_mut().zip(aer_spikes.iter()) {
                                        if *src != 0 {
                                            *dst = 1;
                                        }
                                    }
                                }
                                if let Some(dt) = ipc_dt {
                                    r.step_sync(dt, Some(&spikes));
                                } else {
                                    r.step(Some(&spikes));
                                }
                                if let Some(link) = aer_link.as_mut() {
                                    let ts_us = (r.t_ms * 1000.0) as u64;
                                    if let Some(out) = r.last_spk_o.as_slice() {
                                        link.send_output_spikes(ts_us, out);
                                    }
                                }
                                #[cfg(all(feature = "robot_io", unix))]
                                if let Some(ref srv) = sim_ipc_server {
                                    let mut out = vec![0.0f32; srv.o];
                                    let io_cfg = r.net.spike_io.clone();
                                    let io_profile =
                                        resolve_network_io_profile(io_cfg.profile, srv.s, srv.o);
                                    if matches!(
                                        io_cfg.output_strategy,
                                        SpikeOutputDecodingStrategy::ProfileDefault
                                    ) {
                                        decode_profile_outputs(
                                            io_profile,
                                            &r,
                                            &mut out,
                                            &ipc_output_encoding,
                                        );
                                    } else {
                                        decode_network_outputs(&io_cfg, &r, &mut out);
                                    }
                                    let celegans_debug_enabled =
                                        matches!(io_profile, NetworkIoProfile::Celegans)
                                            && match io_cfg.output_strategy {
                                                SpikeOutputDecodingStrategy::ProfileDefault => {
                                                    ipc_output_encoding.celegans_graded_output
                                                }
                                                SpikeOutputDecodingStrategy::Graded => true,
                                                _ => false,
                                            };
                                    if celegans_debug_enabled {
                                        if ipc_celegans_debug_interval > 0 {
                                            ipc_celegans_debug_counter =
                                                ipc_celegans_debug_counter.saturating_add(1);
                                            if ipc_celegans_debug_counter
                                                .is_multiple_of(ipc_celegans_debug_interval)
                                            {
                                                let out_min = out
                                                    .iter()
                                                    .fold(f32::INFINITY, |acc, &v| acc.min(v));
                                                let out_max = out.iter().fold(
                                                    f32::NEG_INFINITY,
                                                    |acc, &v| acc.max(v),
                                                );
                                                let out_mean = if out.is_empty() {
                                                    0.0
                                                } else {
                                                    out.iter().sum::<f32>() / out.len() as f32
                                                };
                                                let in_spk =
                                                    spikes.iter().filter(|&&v| v != 0).count();
                                                let h_spk = r
                                                    .last_spk_h
                                                    .first()
                                                    .map(|h| h.iter().filter(|&&v| v != 0).count())
                                                    .unwrap_or(0);
                                                let (recv_in_total, recv_in_w_sum, recv_in_w_max) =
                                                    r.w_in.iter().fold(
                                                        (0usize, 0.0f64, 0.0f64),
                                                        |(count, sum, max_w), &w| {
                                                            let abs_w = w.abs();
                                                            if abs_w > 0.0 {
                                                                (
                                                                    count + 1,
                                                                    sum + abs_w,
                                                                    max_w.max(abs_w),
                                                                )
                                                            } else {
                                                                (count, sum, max_w)
                                                            }
                                                        },
                                                    );
                                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                                let (in_delay_min, in_delay_max) = {
                                                    let mut min_steps = usize::MAX;
                                                    let mut max_steps = 0usize;
                                                    for syns in &r.recv_in {
                                                        for &(_, syn_idx) in syns {
                                                            let (steps, _) =
                                                                r.syn_delay_and_atten(syn_idx);
                                                            min_steps = min_steps.min(steps);
                                                            max_steps = max_steps.max(steps);
                                                        }
                                                    }
                                                    if min_steps == usize::MAX {
                                                        (0, 0)
                                                    } else {
                                                        (min_steps, max_steps)
                                                    }
                                                };
                                                #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
                                                let (in_delay_min, in_delay_max) = (0usize, 0usize);
                                                let (i_h0_min, i_h0_max) = r
                                                    .last_i_h0
                                                    .as_ref()
                                                    .map(|arr| {
                                                        arr.iter().fold(
                                                            (f64::INFINITY, f64::NEG_INFINITY),
                                                            |(mn, mx), &v| {
                                                                (mn.min(v), mx.max(v))
                                                            },
                                                        )
                                                    })
                                                    .unwrap_or((0.0, 0.0));
                                                let v_h0_max = r
                                                    .v_h
                                                    .first()
                                                    .map(|arr| {
                                                        arr.iter().fold(
                                                            f64::NEG_INFINITY,
                                                            |mx, &v| mx.max(v),
                                                        )
                                                    })
                                                    .unwrap_or(0.0);
                                                let o_spk = r
                                                    .last_spk_o
                                                    .iter()
                                                    .filter(|&&v| v != 0)
                                                    .count();
                                                nm_err!(
                                                    "[IPC celegans dbg] in_spk={} h0_spk={} out_spk={} recv_in={} w_sum={:.3} w_max={:.3} p_rel={:.3} delay=[{},{}] i_h0=[{:.3},{:.3}] v_h0_max={:.3} out_min={:.3} out_max={:.3} out_mean={:.3} O={}",
                                                    in_spk,
                                                    h_spk,
                                                    o_spk,
                                                    recv_in_total,
                                                    recv_in_w_sum,
                                                    recv_in_w_max,
                                                    r.net.p_release_default,
                                                    in_delay_min,
                                                    in_delay_max,
                                                    i_h0_min,
                                                    i_h0_max,
                                                    v_h0_max,
                                                    out_min,
                                                    out_max,
                                                    out_mean,
                                                    r.net.num_output_neurons
                                                );
                                            }
                                        }
                                    }
                                    ipc_reply_values = Some(out);
                                }
                                if let Ok(mut snap) = sim_ui_snapshot.try_write() {
                                    snap.sensory_spikes.clear();
                                    snap.sensory_spikes.extend_from_slice(&spikes);
                                    let layers = r.last_spk_h.len();
                                    if snap.hidden_spikes.len() != layers {
                                        snap.hidden_spikes.resize_with(layers, Vec::new);
                                    }
                                    for (dst, src) in
                                        snap.hidden_spikes.iter_mut().zip(r.last_spk_h.iter())
                                    {
                                        if let Some(src_slice) = src.as_slice() {
                                            if dst.len() != src_slice.len() {
                                                dst.resize(src_slice.len(), 0);
                                            }
                                            dst.copy_from_slice(src_slice);
                                        } else {
                                            dst.clear();
                                            dst.extend(src.iter().copied());
                                        }
                                    }
                                    if let Some(src_slice) = r.last_spk_o.as_slice() {
                                        if snap.output_spikes.len() != src_slice.len() {
                                            snap.output_spikes.resize(src_slice.len(), 0);
                                        }
                                        snap.output_spikes.copy_from_slice(src_slice);
                                    } else {
                                        snap.output_spikes.clear();
                                        snap.output_spikes.extend(r.last_spk_o.iter().copied());
                                    }
                                    snap.num_sensory = r.net.num_sensory_neurons;
                                    snap.num_hidden_layers = r.net.num_hidden_layers;
                                    snap.num_output = r.net.num_output_neurons;
                                    #[cfg(feature = "growth3d")]
                                    {
                                        if r.net.growth_enabled {
                                            snap.topo_sensory = r.topo.sensory_nodes.clone();
                                            snap.topo_hidden = r.topo.layers.clone();
                                            snap.topo_output = r.topo.output_nodes.clone();
                                        }
                                    }
                                }
                            } else {
                                // If the runner lock is briefly busy, still reply with the last
                                // known actuator frame so Webots controller IO doesn't stall.
                                #[cfg(all(feature = "robot_io", unix))]
                                if let Some(ref srv) = sim_ipc_server {
                                    if ipc_last_reply_values.len() != srv.o {
                                        ipc_last_reply_values.resize(srv.o, 0.5);
                                    }
                                    ipc_reply_values = Some(ipc_last_reply_values.clone());
                                }
                            }
                            #[cfg(all(feature = "robot_io", unix))]
                            if let Some(out_vals) = ipc_reply_values {
                                if let Some(ref mut srv) = sim_ipc_server {
                                    ipc_last_reply_values = out_vals.clone();
                                    let _ = srv.send_outputs(&out_vals);
                                    if let Ok(mut stats) = sim_ipc_stats.try_write() {
                                        stats.last_steps = 1;
                                    }
                                }
                            }
                        } else {
                            let mut last_bands: Option<Vec<f32>> = None;
                            let update_ui_snapshot = |r: &Runner| {
                                if let Ok(mut snap) = sim_ui_snapshot.try_write() {
                                    snap.sensory_spikes.clear();
                                    if let Some(front) =
                                        r.spk_hist_s.front().and_then(|v| v.as_slice())
                                    {
                                        snap.sensory_spikes.extend_from_slice(front);
                                    } else if let Some(front) = r.spk_hist_s.front() {
                                        snap.sensory_spikes.extend(front.iter().copied());
                                    }
                                    let layers = r.last_spk_h.len();
                                    if snap.hidden_spikes.len() != layers {
                                        snap.hidden_spikes.resize_with(layers, Vec::new);
                                    }
                                    for (dst, src) in
                                        snap.hidden_spikes.iter_mut().zip(r.last_spk_h.iter())
                                    {
                                        if let Some(src_slice) = src.as_slice() {
                                            if dst.len() != src_slice.len() {
                                                dst.resize(src_slice.len(), 0);
                                            }
                                            dst.copy_from_slice(src_slice);
                                        } else {
                                            dst.clear();
                                            dst.extend(src.iter().copied());
                                        }
                                    }
                                    if let Some(src_slice) = r.last_spk_o.as_slice() {
                                        if snap.output_spikes.len() != src_slice.len() {
                                            snap.output_spikes.resize(src_slice.len(), 0);
                                        }
                                        snap.output_spikes.copy_from_slice(src_slice);
                                    } else {
                                        snap.output_spikes.clear();
                                        snap.output_spikes.extend(r.last_spk_o.iter().copied());
                                    }
                                    snap.num_sensory = r.net.num_sensory_neurons;
                                    snap.num_hidden_layers = r.net.num_hidden_layers;
                                    snap.num_output = r.net.num_output_neurons;
                                    #[cfg(feature = "growth3d")]
                                    {
                                        if r.net.growth_enabled {
                                            snap.topo_sensory = r.topo.sensory_nodes.clone();
                                            snap.topo_hidden = r.topo.layers.clone();
                                            snap.topo_output = r.topo.output_nodes.clone();
                                        }
                                    }
                                }
                            };
                            for _ in 0..batch_steps {
                                let mut spikes = sim_provider.next_spikes();
                                sim_step_counter_thread.fetch_add(1, Ordering::Relaxed);
                                sim_last_spike_count_thread.store(
                                    spikes.iter().filter(|&&v| v != 0).count() as u64,
                                    Ordering::Relaxed,
                                );
                                sim_last_spike_len_thread
                                    .store(spikes.len() as u64, Ordering::Relaxed);
                                if let Ok(mut snap) = sim_sensory_snapshot.try_write() {
                                    *snap = spikes.clone();
                                }
                                if let Some(bands) = sim_provider.last_bands() {
                                    last_bands = Some(bands.to_vec());
                                }
                                if let Ok(mut r) = sim_runner.try_write() {
                                    r.external_reward = 0.0;
                                    if let Some(link) = aer_link.as_mut() {
                                        let start_us = (r.t_ms * 1000.0) as u64;
                                        let end_us = ((r.t_ms + r.lif.dt) * 1000.0) as u64;
                                        let aer_spikes =
                                            link.sensory_spikes(start_us, end_us, spikes.len());
                                        for (dst, src) in spikes.iter_mut().zip(aer_spikes.iter()) {
                                            if *src != 0 {
                                                *dst = 1;
                                            }
                                        }
                                    }
                                    r.step(Some(&spikes));
                                    if let Some(link) = aer_link.as_mut() {
                                        let ts_us = (r.t_ms * 1000.0) as u64;
                                        if let Some(out) = r.last_spk_o.as_slice() {
                                            link.send_output_spikes(ts_us, out);
                                        }
                                    }
                                    update_ui_snapshot(&r);
                                    // Write lock is dropped here at end of this block.
                                    // Yield between batch steps so the UI event loop and
                                    // any pending try_read() callers can proceed.
                                } else {
                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                    break;
                                }
                                // Explicitly yield after each step within a batch so the UI
                                // thread gets a window to acquire the read lock between steps.
                                std::thread::yield_now();
                            }
                            if let Some(bands) = last_bands {
                                if let Ok(mut b) = sim_spectral.try_write() {
                                    *b = bands;
                                }
                            }
                        }
                        // Yield to give UI and other threads a chance to acquire the runner lock
                        std::thread::yield_now();
                        let throttle = sim_throttle.load(Ordering::Relaxed);
                        if throttle > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(throttle as u64));
                        }
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            })
            .expect("Failed to spawn simulation thread");

        #[cfg(feature = "sysinfo")]
        let (sys_snapshot, sys_stop) = {
            let snapshot = Arc::new(RwLock::new(SysSnapshot::default()));
            let stop = Arc::new(AtomicBool::new(false));
            let snapshot_thread = snapshot.clone();
            let stop_thread = stop.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("sysinfo".into())
                .spawn(move || {
                    let _ = crate::affinity::apply_rotating_current_thread("sysinfo");
                    let mut sys = sysinfo::System::new_all();
                    let mut components = Components::new_with_refreshed_list();
                    let mut last_sys_update =
                        std::time::Instant::now() - std::time::Duration::from_secs(1);
                    let mut last_temp_update =
                        std::time::Instant::now() - std::time::Duration::from_secs(2);
                    let mut last_rss_log =
                        std::time::Instant::now() - std::time::Duration::from_secs(5);
                    let mut rss_baseline_mb: Option<u64> = None;
                    let mut rss_last_mb: Option<u64> = None;
                    let mut cpu_usage = 0.0f32;
                    let mut ram_usage_mb = 0.0f32;
                    let mut cpu_temp_c: Option<f32> = None;
                    let hot_core_threshold_pct = ui_hot_core_threshold_pct();
                    let mut os_threads = 0u32;
                    let mut runnable_threads = 0u32;
                    let mut cpu_core_count = 0u32;
                    let mut hot_core_count = 0u32;
                    let mut hot_core_top: Vec<(usize, f32)> = Vec::new();

                    loop {
                        if stop_thread.load(Ordering::SeqCst) {
                            break;
                        }
                        let now = std::time::Instant::now();
                        let mut touched = false;

                        if now.duration_since(last_sys_update)
                            >= std::time::Duration::from_millis(1000)
                        {
                            sys.refresh_cpu_usage();
                            sys.refresh_memory();
                            cpu_usage = sys.global_cpu_usage();
                            ram_usage_mb = sys.used_memory() as f32 / 1024.0 / 1024.0;
                            let (threads, runnable) = read_linux_thread_counts();
                            os_threads = threads;
                            runnable_threads = runnable;
                            let mut per_core: Vec<(usize, f32)> = sys
                                .cpus()
                                .iter()
                                .enumerate()
                                .map(|(idx, cpu)| (idx, cpu.cpu_usage()))
                                .collect();
                            cpu_core_count = per_core.len() as u32;
                            hot_core_count = per_core
                                .iter()
                                .filter(|(_, usage)| *usage >= hot_core_threshold_pct)
                                .count() as u32;
                            per_core.sort_by(|a, b| {
                                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            per_core.truncate(4);
                            hot_core_top = per_core;
                            last_sys_update = now;
                            touched = true;
                        }
                        if now.duration_since(last_temp_update)
                            >= std::time::Duration::from_millis(2000)
                        {
                            components.refresh(false);
                            let mut max_c = None;
                            for component in &components {
                                if let Some(temp) = component.temperature() {
                                    if temp.is_finite() {
                                        max_c =
                                            Some(max_c.map_or(temp, |prev: f32| prev.max(temp)));
                                    }
                                }
                            }
                            cpu_temp_c = max_c;
                            last_temp_update = now;
                            touched = true;
                        }
                        if now.duration_since(last_rss_log) >= std::time::Duration::from_secs(5) {
                            if let Ok(pid) = sysinfo::get_current_pid() {
                                sys.refresh_processes_specifics(
                                    ProcessesToUpdate::Some(&[pid]),
                                    true,
                                    ProcessRefreshKind::nothing().with_memory(),
                                );
                                if let Some(proc) = sys.process(pid) {
                                    let total_raw = sys.total_memory() as u64;
                                    let scale_is_bytes = total_raw > 1_000_000_000;
                                    let raw = proc.memory() as u64;
                                    let rss_mb = if scale_is_bytes {
                                        raw / 1024 / 1024
                                    } else {
                                        raw / 1024
                                    };
                                    if rss_baseline_mb.is_none() {
                                        rss_baseline_mb = Some(rss_mb);
                                    }
                                    let baseline = rss_baseline_mb.unwrap_or(rss_mb);
                                    let growth = rss_mb.saturating_sub(baseline);
                                    if rss_last_mb.map_or(true, |prev| rss_mb != prev) {
                                        nm_log!("[info] UI RSS: {}MB (+{}MB)", rss_mb, growth);
                                    }
                                    rss_last_mb = Some(rss_mb);
                                }
                            }
                            last_rss_log = now;
                        }

                        if touched {
                            if let Ok(mut snap) = snapshot_thread.try_write() {
                                snap.cpu_usage = cpu_usage;
                                snap.ram_usage_mb = ram_usage_mb;
                                snap.cpu_temp_c = cpu_temp_c;
                                snap.os_threads = os_threads;
                                snap.runnable_threads = runnable_threads;
                                snap.cpu_core_count = cpu_core_count;
                                snap.hot_core_count = hot_core_count;
                                snap.hot_core_top = hot_core_top.clone();
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                })
            {
                nm_err!("[warn] Sysinfo thread failed: {}", e);
            }
            (snapshot, stop)
        };

        let http_aer_source_url = std::env::var("NM_HTTP_AER_SOURCE_URL").unwrap_or_default();
        let http_aer_base = std::env::var("NM_HTTP_AER_BASE")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let http_aer_status = Arc::new(RwLock::new(HttpAerInputStatus::default()));

        let mut app = Self {
            brain_id: brain_id.clone(),
            playing: false,
            loop_feedback: false,
            #[cfg(all(feature = "robot_io", unix))]
            input_source: if _ipc_enabled {
                InputSource::ExternalIpc
            } else {
                InputSource::Random
            },
            #[cfg(not(all(feature = "robot_io", unix)))]
            input_source: InputSource::Random,
            http_aer_source_url,
            http_aer_base,
            http_aer_status,
            sensory_count: n_s,
            neuron_model: NeuronModelSel::Aarnn,
            izh_preset: IzhPreset::RS,
            learning: LearningSel::Aarnn,
            status: "Ready".to_string(),
            remote_only,
            runner,
            sim_tx,
            playing_atomic,
            sim_throttle_ms,
            spectral_bands,
            longterm_conn: 0,
            total_conn: 0,
            random_spike_probability: 0.02,
            mic_running: false,
            ga_search: None,
            ga_running: false,
            ga_panel_visible: false,
            ga_best_fitness: 0.0,
            ga_mutation_rate: 0.05,
            ga_crossover_rate: 0.7,
            ga_use_dk_bias: true,
            ga_pop_size: 20,
            ga_generations: 50,
            ga_sim_time_ms: 10000.0,
            ga_rx: None,
            ga_paused: false,
            ga_live_preview: false,
            ga_leaderboard_idx: None,
            ga_control_tx: None,
            ga_thread: None,
            ga_pacing_ack: false,
            ga_abort_cleanup_done: false,
            #[cfg(feature = "webcam_input")]
            cam_running: false,
            smoothed_equalizer_values: Vec::new(),
            output_count: o,
            last_rendered_panel_size: egui::vec2(0.0, 0.0),
            sensory_positions: Vec::new(),
            hidden_positions: vec![vec![]; l],
            output_positions: Vec::new(),
            network_layout: NetworkLayout::Aarnn,
            layout_auto: true,
            sensory_activity: vec![0.0; n_s],
            hidden_activity: act_h,
            output_activity: vec![0.0; o],
            raster_cols: 240,
            raster_outputs: std::collections::VecDeque::new(),
            camera_zoom: 1.0,
            camera_yaw_degrees: 0.0,
            camera_pitch_degrees: 0.0,
            cam_pan: egui::vec2(0.0, 0.0),
            show_highlights: true,
            max_highlight_lines: 8,
            last_sensory_spikes: vec![0; n_s],
            previous_hidden_spikes: prev_spk_h,
            show_backward_highlights: true,
            show_static_overlays: true,
            overlay_density: 6,
            overlay_opacity: 0.25,
            show_feedback_overlays: false,
            edge_shapes: Vec::new(),
            #[cfg(feature = "growth3d")]
            growth_enabled: local_net.growth_enabled,
            #[cfg(feature = "growth3d")]
            show_region_labels: true,
            #[cfg(feature = "growth3d")]
            region_label_positions: Vec::new(),
            #[cfg(feature = "growth3d")]
            region_label_states: crate::morphology::FastHashMap::default(),
            #[cfg(feature = "growth3d")]
            region_label_target_states: crate::morphology::FastHashMap::default(),
            #[cfg(feature = "growth3d")]
            cam_pivot_world: (0.0, 0.0, 0.0),
            #[cfg(feature = "growth3d")]
            cam_pivot_pid: UiPid3State::default(),
            #[cfg(feature = "growth3d")]
            topo_pid_sensory: Vec::new(),
            #[cfg(feature = "growth3d")]
            topo_pid_hidden: Vec::new(),
            #[cfg(feature = "growth3d")]
            topo_pid_output: Vec::new(),
            #[cfg(feature = "growth3d")]
            cached_skull_hull: Vec::new(),
            #[cfg(feature = "growth3d")]
            last_hull_update: std::time::Instant::now() - std::time::Duration::from_secs(60),
            last_synced_config: None,
            last_config_json: String::new(),
            last_ga_best_config: None,
            last_ga_best_config_json: String::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            show_morpho_overlays: false,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            morpho_opacity: 0.25,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            show_transmissions: false,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            transmissions_opacity: 0.8,
            show_equalizer: false,
            probes: Vec::new(),
            next_probe_id: 1,
            scope_time_ms: 2000.0,
            scope_gain: 1.0,
            scope_lanes: true,
            scope_grid: true,
            scope_paused: false,
            python_path: None,
            tooltip_suppression_counter: 0,
            tooltip_pinned: false,
            tooltip_pinned_pos: egui::Pos2::ZERO,
            tooltip_pinned_lines: Vec::new(),
            tooltip_pinned_target: None,
            aarnn_defaults_applied: false,
            boost_connectivity_count: 0,
            local_net: local_net.clone(),
            fpaa_status,
            fpaa_last_refresh: Instant::now(),
            fpaa_last_signature: fpaa_signature,
            #[cfg(feature = "sysinfo")]
            sys_snapshot,
            #[cfg(feature = "sysinfo")]
            sys_stop,
            #[cfg(feature = "sysinfo")]
            cpu_temp_c: None,
            cpu_usage: 0.0,
            ram_usage_mb: 0.0,
            hot_core_threshold_pct: ui_hot_core_threshold_pct(),
            os_threads: 0,
            runnable_threads: 0,
            cpu_core_count: 0,
            hot_core_count: 0,
            hot_core_top: Vec::new(),
            last_step_duration: std::time::Duration::from_secs(0),
            avg_step_time_ms: 0.0,
            auto_dt_enabled: true,
            responsiveness_target_ms: 12.0, // Aim for ~12ms calculation time to leave room for UI
            last_longterm_update: std::time::Instant::now(),
            show_neuron_detail: false,
            selected_neuron_pick: None,
            detail_camera_zoom: 1.0,
            detail_camera_yaw: 0.0,
            detail_camera_pitch: 0.0,
            detail_cam_pan: egui::vec2(0.0, 0.0),
            detail_camera_pos: [0.0, 0.0, 0.0],
            detail_bio_orient: DetailBioOrient::AsIs,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_enabled: true,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_kp: 0.35,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_ki: 0.0,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_kd: 0.08,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_axon: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_dend: Vec::new(),
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            detail_bouton_pid_pick: None,
            detail_timescale: 1.0,
            detail_time_offset: 0.0,
            detail_paused: false,
            detail_last_neuron: None,
            detail_waiting_for_activation: false,
            last_ui_render_time: std::time::Instant::now(),
            #[cfg(all(feature = "robot_io", unix))]
            ipc_sock_path: {
                if let Ok(home) = std::env::var("HOME") {
                    if brain_id == "default" {
                        format!("{}/aarnn_rust.nn", home)
                    } else {
                        format!("{}/aarnn_rust.{}.nn", home, brain_id)
                    }
                } else {
                    if brain_id == "default" {
                        "/tmp/aarnn_rust.nn".to_string()
                    } else {
                        format!("/tmp/aarnn_rust.{}.nn", brain_id)
                    }
                }
            },
            #[cfg(all(feature = "robot_io", unix))]
            ipc_connected: false,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_last_peer: None,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_last_receive_time: None,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_packet_drop_count: 0,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_size_mismatch_count: 0,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_threshold: 0.2,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_neurons_per_value: 1,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_bias_last_sensory_input: true,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_sync: crate::bridge::TimeSync::new(),
            #[cfg(all(feature = "robot_io", unix))]
            quantizer: Quantizer {
                threshold: 0.2,
                probabilistic: true,
                ..Quantizer::default()
            },
            #[cfg(all(feature = "robot_io", unix))]
            last_sensory_inputs_f32: vec![0.0; n_s],
            #[cfg(all(feature = "robot_io", unix))]
            last_actuator_outputs_f32: vec![0.0; o],
            #[cfg(all(feature = "robot_io", unix))]
            ipc_mapping: None,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_frame_count: 0,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_last_steps: 0,
            #[cfg(all(feature = "robot_io", unix))]
            ipc_last_handshake: None,
            ipc_stats: ipc_stats.clone(),
            distributed_node,
            view_source: ViewSource::Standalone,
            view_node_filter: None,
            remote_addr_input: String::new(),
            remote_connections: Vec::new(),
            remote_status_tx,
            remote_status_rx,
            remote_statuses: HashMap::new(),
            tool_task_tx,
            tool_task_rx,
            edge_cache_rx,
            edge_cache_res_tx,
            edge_cache_inflight: false,
            pending_import: None,
            last_import_report: None,
            tflite_import_mode: TfliteImportMode::Mlp,
            tflite_allow_fallback: false,
            tflite_allow_large: false,
            tflite_max_layers: 16,
            tflite_max_params: 2_000_000,
            tflite_freeze_learning: true,
            tflite_sim_throttle_ms: 2,
            force_show_connections: false,
            pending_edge_cache: false,
            edge_cache_refresh_ms,
            last_edge_cache_refresh: std::time::Instant::now()
                .checked_sub(std::time::Duration::from_millis(edge_cache_refresh_ms))
                .unwrap_or_else(std::time::Instant::now),
            cached_edges: Vec::new(),
            cached_layer_sizes: hidden_layer_sizes,
            cached_conn_counts: Vec::new(),
            cached_output_conn_count: None,
            #[cfg(feature = "growth3d")]
            cached_edge_topo: None,
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            cached_skull_membrane: None,
            conn_stats_refresh_ms,
            last_conn_stats_refresh: std::time::Instant::now()
                .checked_sub(std::time::Duration::from_millis(conn_stats_refresh_ms))
                .unwrap_or_else(std::time::Instant::now),
            last_layout_recompute: std::time::Instant::now(),
            cluster_snapshot_tx,
            cluster_snapshot_rx,
            cluster_snapshot_inflight: false,
            cluster_snapshot_last_fetch: None,
            cluster_snapshot_network_id: None,
            cluster_snapshot_node_id: None,
            cluster_snapshot_cache: None,
            #[cfg(feature = "growth3d")]
            cluster_topo_cache: None,
            dist_is_orchestrator: false,
            dist_node_id: String::new(),
            dist_nodes: HashMap::new(),
            dist_network_registry: HashMap::new(),
            dist_local_playing_cache: HashMap::new(),
            dist_initial_view_selected: false,
            sensory_spikes_snapshot,
            ui_snapshot,
            sim_step_counter,
            sim_last_spike_count,
            sim_last_spike_len,
            runtime_handle,
            remote_workspace_binding,
            remote_token_balance: None,
            remote_token_error: None,
            remote_token_last_refresh: None,
            remote_token_refresh_inflight: false,
            initial_net_cfg,
            initial_lif,
            initial_stdp,
            initial_model,
            initial_learning,
        };

        if remote_only {
            app.status = "Remote-only UI (local simulation disabled)".into();
        }
        if app.remote_workspace_binding.is_some() {
            app.queue_remote_token_refresh(true);
        }
        let auto_remote_count = app.add_remote_orchestrators_from_env();
        if auto_remote_count > 0 {
            app.status = format!(
                "Remote-only UI (auto-connected {} orchestrator{})",
                auto_remote_count,
                if auto_remote_count == 1 { "" } else { "s" }
            );
        }

        #[cfg(all(feature = "robot_io", unix))]
        if _ipc_enabled {
            let _ = app
                .sim_tx
                .send(SimControl::BindIpc(app.ipc_sock_path.clone(), n_s, o));
            app.status = format!("IPC requested bind: {}", app.ipc_sock_path);
        }

        // Load GA leaderboard if it exists
        let mut ga_seed_rng = rand::rng();
        let mut ga_rng = rand::rngs::StdRng::from_rng(&mut ga_seed_rng);
        let mut ga = GASearch::new(12, &app.local_net, &mut ga_rng, None, false, Vec::new());
        if let Ok(_) = ga.load_leaderboard("leaderboard.json") {
            if !ga.leaderboard.is_empty() {
                app.ga_search = Some(ga);
            }
        }

        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        if app.local_net.use_morphology {
            app.show_morpho_overlays = true;
            app.show_transmissions = true;
            app.morpho_opacity = 0.4;
            app.transmissions_opacity = 0.9;
        }

        app
    }
}

#[cfg(feature = "ui")]
impl App {
    fn normalize_remote_orchestrator_addr(addr: &str) -> Option<String> {
        let mut normalized = addr.trim().to_string();
        if normalized.is_empty() {
            return None;
        }
        if !normalized.starts_with("http://") && !normalized.starts_with("https://") {
            normalized = format!("http://{}", normalized);
        }
        Some(normalized)
    }

    fn remote_workspace_client(&self) -> Result<crate::runtime_api::BlockingRuntimeClient, String> {
        self.remote_workspace_binding
            .as_ref()
            .ok_or_else(|| "Remote workspace binding is not configured".to_string())?
            .client()
            .map_err(|err| err.to_string())
    }

    fn queue_remote_token_refresh(&mut self, force: bool) {
        if self.remote_workspace_binding.is_none() {
            self.remote_token_refresh_inflight = false;
            return;
        }
        if self.remote_token_refresh_inflight {
            return;
        }
        let stale = self
            .remote_token_last_refresh
            .map(|ts| ts.elapsed() >= Duration::from_secs(30))
            .unwrap_or(true);
        if !force && !stale {
            return;
        }
        let Some(binding) = self.remote_workspace_binding.clone() else {
            return;
        };
        let tx = self.tool_task_tx.clone();
        self.remote_token_refresh_inflight = true;
        std::thread::spawn(move || {
            let result = (|| -> Result<TokenBalanceResponse, String> {
                let mut client = binding.client().map_err(|err| err.to_string())?;
                client.token_balance().map_err(|err| err.to_string())
            })();
            let _ = tx.send(ToolTaskResult::RemoteTokenBalance { result });
        });
    }

    fn push_remote_workspace_snapshot(&mut self) -> Result<(), String> {
        let binding = self
            .remote_workspace_binding
            .as_ref()
            .ok_or_else(|| "Remote workspace binding is not configured".to_string())?;
        let snapshot_json = {
            let runner = self
                .runner
                .try_read()
                .map_err(|_| "Runner busy".to_string())?;
            runner
                .export_network_json()
                .map_err(|err| err.to_string())?
        };
        let mut client = self.remote_workspace_client()?;
        client
            .import_workspace(
                &binding.workspace_id,
                &WorkspaceImportRequest {
                    payload_json: snapshot_json,
                    kind: Some(crate::engine::EnginePayloadKind::Snapshot),
                    replace_baseline: Some(false),
                    auto_start: Some(false),
                    neuron_model: None,
                    learning_rule: None,
                },
            )
            .map_err(|err| err.to_string())?;
        self.queue_remote_token_refresh(true);
        Ok(())
    }

    fn pull_remote_workspace_snapshot(&mut self) -> Result<(), String> {
        let binding = self
            .remote_workspace_binding
            .as_ref()
            .ok_or_else(|| "Remote workspace binding is not configured".to_string())?
            .clone();
        let mut client = self.remote_workspace_client()?;
        let snapshot = client
            .workspace_snapshot(&binding.workspace_id)
            .map_err(|err| err.to_string())?;

        {
            let mut runner = self
                .runner
                .try_write()
                .map_err(|_| "Runner busy".to_string())?;
            runner
                .import_network_json(&snapshot.snapshot_json)
                .map_err(|err| err.to_string())?;
            self.initial_net_cfg = runner.net.clone();
            self.initial_model = runner.neuron_model;
            self.initial_learning = runner.learning;
        }

        self.set_standalone_playing(false);
        self.refresh_ui_buffers();
        self.status = format!("Pulled remote workspace '{}'", binding.workspace_id);
        Ok(())
    }

    fn control_remote_workspace_backend(
        &mut self,
        action: WorkspaceControlAction,
    ) -> Result<(), String> {
        let binding = self
            .remote_workspace_binding
            .as_ref()
            .ok_or_else(|| "Remote workspace binding is not configured".to_string())?
            .clone();
        let mut client = self.remote_workspace_client()?;
        client
            .control_workspace(&binding.workspace_id, action)
            .map_err(|err| err.to_string())?;
        self.queue_remote_token_refresh(true);
        self.status = format!("Remote workspace '{}' {:?}", binding.workspace_id, action);
        Ok(())
    }

    fn add_remote_orchestrator_connection(&mut self, addr: &str) -> bool {
        let Some(addr) = Self::normalize_remote_orchestrator_addr(addr) else {
            return false;
        };
        if self.remote_connections.iter().any(|c| c.addr == addr) {
            return false;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let tx = self.remote_status_tx.clone();
        let addr_clone = addr.clone();
        let rt = self.runtime_handle.clone();
        rt.spawn(async move {
            loop {
                if stop_clone.load(Ordering::SeqCst) {
                    break;
                }
                match DistributedNeuromorphicClient::connect(addr_clone.clone()).await {
                    Ok(mut client) => {
                        match client
                            .get_system_status(Request::new(StatusRequest {}))
                            .await
                        {
                            Ok(resp) => {
                                let status = resp.into_inner();
                                let nodes = status
                                    .nodes
                                    .into_iter()
                                    .map(|n| (n.node_id.clone(), n))
                                    .collect();
                                let networks = status
                                    .networks
                                    .into_iter()
                                    .map(|n| (n.network_id.clone(), n))
                                    .collect();
                                let _ = tx.send(RemoteStatusMsg::Update {
                                    addr: addr_clone.clone(),
                                    nodes,
                                    networks,
                                });
                            }
                            Err(e) => {
                                let _ = tx.send(RemoteStatusMsg::Error {
                                    addr: addr_clone.clone(),
                                    error: format!("Status error: {}", e),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(RemoteStatusMsg::Error {
                            addr: addr_clone.clone(),
                            error: format!("Connect error: {}", e),
                        });
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
        self.remote_connections
            .push(RemoteConnection { addr, stop });
        true
    }

    fn add_remote_orchestrators_from_env(&mut self) -> usize {
        let raw = std::env::var("NM_UI_REMOTE_ORCHESTRATORS")
            .ok()
            .or_else(|| std::env::var("NM_REMOTE_ORCHESTRATORS").ok())
            .unwrap_or_default();
        if raw.trim().is_empty() {
            return 0;
        }

        let mut added = 0usize;
        for token in raw.split([',', ';', ' ']) {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.add_remote_orchestrator_connection(trimmed) {
                added += 1;
            }
        }
        added
    }

    fn apply_aarnn_bio_defaults(&mut self) {
        let mut net = self.local_net.clone();
        // Canonical baseline: AARNN biomimicry profile with human-brain clumping.
        apply_aarnn_human_biomimicry_defaults(&mut net);

        // Optional UI visualizations default on for AARNN
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            self.show_morpho_overlays = true;
            self.morpho_opacity = 0.4;
            self.show_transmissions = true;
            self.transmissions_opacity = 0.9;
        }

        // Ensure learning is AARNN when model is AARNN
        self.learning = LearningSel::Aarnn;
        self.local_net = net.clone();
        let _ = self.sim_tx.send(SimControl::ApplyConfig(net));
        let _ = self.sim_tx.send(SimControl::SetLearning(Learning::Aarnn));

        self.refresh_ui_buffers();
        self.status = "Applied AARNN human-brain biomimicry defaults".into();
    }

    fn refresh_fpaa_status(&mut self, force: bool) {
        let signature = serde_json::to_string(&self.local_net.fpaa).unwrap_or_default();
        let stale = self.fpaa_last_refresh.elapsed() >= Duration::from_secs(5);
        if force || stale || signature != self.fpaa_last_signature {
            self.fpaa_status = crate::fpaa::startup_probe(&self.local_net.fpaa);
            self.fpaa_last_refresh = Instant::now();
            self.fpaa_last_signature = signature;
        }
    }

    fn fpaa_route_label(route: FpaaKernelRoute) -> &'static str {
        match route {
            FpaaKernelRoute::Software => "Software",
            FpaaKernelRoute::Fpaa => "FPAA",
        }
    }

    fn render_fpaa_controls(&mut self, ui: &mut egui::Ui) -> bool {
        self.refresh_fpaa_status(false);
        let mut changed = false;

        ui.group(|ui| {
            ui.label("FPAA Offload");
            ui.small(self.fpaa_status.summary.as_str());
            if let Some(transport) = self.fpaa_status.detected_transport.as_ref() {
                ui.small(format!(
                    "Transport: {} | {} | ready={}",
                    transport.kind.label(),
                    transport.path,
                    transport.ready
                ));
            } else {
                ui.small("Transport: not detected");
            }
            if let Some(err) = self.fpaa_status.startup_error.as_deref() {
                ui.colored_label(egui::Color32::LIGHT_RED, err);
            }
            ui.horizontal(|ui| {
                ui.label("Startup mode");
                egui::ComboBox::from_id_salt("fpaa_startup_mode")
                    .selected_text(match self.local_net.fpaa.startup_mode {
                        FpaaStartupMode::Auto => "Auto",
                        FpaaStartupMode::Disabled => "Disabled",
                        FpaaStartupMode::Required => "Required",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(
                                &mut self.local_net.fpaa.startup_mode,
                                FpaaStartupMode::Auto,
                                "Auto",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.local_net.fpaa.startup_mode,
                                FpaaStartupMode::Disabled,
                                "Disabled",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.local_net.fpaa.startup_mode,
                                FpaaStartupMode::Required,
                                "Required",
                            )
                            .changed();
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Transport");
                egui::ComboBox::from_id_salt("fpaa_transport_pref")
                    .selected_text(match self.local_net.fpaa.transport_preference {
                        FpaaTransportPreference::Auto => "Auto",
                        FpaaTransportPreference::PiHat => "Pi.HAT",
                        FpaaTransportPreference::Usb => "USB",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(
                                &mut self.local_net.fpaa.transport_preference,
                                FpaaTransportPreference::Auto,
                                "Auto",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.local_net.fpaa.transport_preference,
                                FpaaTransportPreference::PiHat,
                                "Pi.HAT",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.local_net.fpaa.transport_preference,
                                FpaaTransportPreference::Usb,
                                "USB",
                            )
                            .changed();
                    });
            });
            changed |= ui
                .checkbox(
                    &mut self.local_net.fpaa.run_self_test_on_startup,
                    "Run FPAA sample tests at startup",
                )
                .changed();
            ui.horizontal(|ui| {
                if ui.button("Refresh FPAA").clicked() {
                    self.refresh_fpaa_status(true);
                    self.status = self.fpaa_status.summary.clone();
                }
                if ui.button("Re-run tests").clicked() {
                    self.local_net.fpaa.run_self_test_on_startup = true;
                    self.refresh_fpaa_status(true);
                    self.status = format!("FPAA re-probed: {}", self.fpaa_status.summary);
                    changed = true;
                }
            });
            ui.separator();
            for kernel in FpaaKernel::ALL {
                let requested = kernel.route_mut(&mut self.local_net.fpaa.routing);
                let effective = self.fpaa_status.effective_route(kernel);
                let status = self
                    .fpaa_status
                    .kernels
                    .iter()
                    .find(|item| item.kernel == kernel);
                ui.horizontal(|ui| {
                    ui.label(kernel.label());
                    egui::ComboBox::from_id_salt(format!("fpaa_kernel_route_{}", kernel.id()))
                        .selected_text(Self::fpaa_route_label(*requested))
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(requested, FpaaKernelRoute::Software, "Software")
                                .changed();
                            changed |= ui
                                .selectable_value(requested, FpaaKernelRoute::Fpaa, "FPAA")
                                .changed();
                        });
                    let effective_color = if effective == FpaaKernelRoute::Fpaa {
                        egui::Color32::LIGHT_GREEN
                    } else {
                        egui::Color32::LIGHT_YELLOW
                    };
                    ui.colored_label(
                        effective_color,
                        format!("effective {}", Self::fpaa_route_label(effective)),
                    );
                });
                if let Some(status) = status {
                    ui.small(format!(
                        "verify={} | self-test={} | {}",
                        status.verification.label(),
                        status.sample_test.label(),
                        status.note
                    ));
                }
            }
        });

        if changed {
            self.refresh_fpaa_status(true);
        }
        changed
    }

    fn cache_sizes_counts(runner: &Runner) -> (Vec<usize>, Vec<usize>, usize) {
        let sizes = (0..runner.net.num_hidden_layers)
            .map(|l| runner.layer_size(l).max(1))
            .collect();
        let counts = runner.connection_counts();
        let out = runner.output_connection_count();
        (sizes, counts, out)
    }

    fn compact_usize_list(values: &[usize]) -> String {
        if values.len() <= 12 {
            return format!("{:?}", values);
        }
        let head = values
            .iter()
            .take(6)
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let mut tail_vals = values
            .iter()
            .rev()
            .take(3)
            .map(|v| v.to_string())
            .collect::<Vec<_>>();
        tail_vals.reverse();
        let tail = tail_vals.join(", ");
        format!("[{}, ..., {}] (n={})", head, tail, values.len())
    }

    fn hidden_summary(layer_sizes: &[usize]) -> String {
        let total_hidden: usize = layer_sizes.iter().sum();
        format!(
            "layers={} total={} sizes={}",
            layer_sizes.len(),
            total_hidden,
            Self::compact_usize_list(layer_sizes)
        )
    }

    fn should_build_static_edges(layer_sizes: &[usize]) -> bool {
        // Keep import/load responsive for very large models unless user explicitly asks.
        let total_hidden: usize = layer_sizes.iter().sum();
        total_hidden <= 8192
    }

    fn compute_edges_from_snapshot(
        overlay_density: usize,
        snap: &crate::runner::Snapshot,
    ) -> (Vec<CachedEdge>, Vec<usize>, Vec<usize>, usize) {
        let k = overlay_density.max(1);
        let mut edges = Vec::new();

        let count_nonzero = |m: &crate::runner::Matrix2| -> usize {
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                m.data.par_iter().filter(|&&x| x.abs() > 1e-8).count()
            }
            #[cfg(not(feature = "parallel"))]
            {
                m.data.iter().filter(|&&x| x.abs() > 1e-8).count()
            }
        };

        let mut max_presence = 0u32;
        let mut has_presence_data = false;
        let mut absorb_presence = |m: &crate::runner::Matrix2U32| {
            has_presence_data = true;
            if let Some(local_max) = m.data.iter().copied().max() {
                max_presence = max_presence.max(local_max);
            }
        };
        if let Some(m) = snap.p_in.as_ref() {
            absorb_presence(m);
        }
        if let Some(ms) = snap.p_fwd.as_ref() {
            for m in ms {
                absorb_presence(m);
            }
        }
        if let Some(ms) = snap.p_bwd.as_ref() {
            for m in ms {
                absorb_presence(m);
            }
        }
        if let Some(ms) = snap.p_rec.as_ref() {
            for m in ms {
                absorb_presence(m);
            }
        }
        if let Some(m) = snap.p_out.as_ref() {
            absorb_presence(m);
        }
        let longterm_min_presence = if !has_presence_data {
            None
        } else if max_presence == 0 {
            Some(0)
        } else {
            Some(((max_presence as f32) * 0.75).ceil() as u32)
        };

        let push_topk = |from_layer: i32,
                         to_layer: i32,
                         m: &crate::runner::Matrix2,
                         presence: Option<&crate::runner::Matrix2U32>,
                         kind: &'static str|
         -> Vec<CachedEdge> {
            let nr = m.rows;
            let nc = m.cols;
            if nr == 0 || nc == 0 || m.data.is_empty() {
                return Vec::new();
            }

            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                let rows: Vec<Vec<CachedEdge>> = (0..nr)
                    .into_par_iter()
                    .map(|r| {
                        let mut best: Vec<(usize, f32)> = Vec::new();
                        let row_base = r.saturating_mul(nc);
                        for i in 0..nc {
                            let idx = row_base + i;
                            let w = *m.data.get(idx).unwrap_or(&0.0) as f32;
                            if w.abs() <= 1e-8 {
                                continue;
                            }
                            if best.len() < k {
                                best.push((i, w));
                            } else {
                                let mut min_idx = 0usize;
                                let mut min_w = best[0].1.abs();
                                for (bi, &(_, bw)) in best.iter().enumerate().skip(1) {
                                    if bw.abs() < min_w {
                                        min_w = bw.abs();
                                        min_idx = bi;
                                    }
                                }
                                if w.abs() > min_w {
                                    best[min_idx] = (i, w);
                                }
                            }
                        }
                        best.into_iter()
                            .map(|(i, w)| CachedEdge {
                                from_layer,
                                to_layer,
                                from_idx: i,
                                to_idx: r,
                                weight: w,
                                kind,
                                is_longterm: {
                                    if longterm_min_presence == Some(0) {
                                        true
                                    } else if let Some(min_presence) = longterm_min_presence {
                                        if let Some(p) = presence {
                                            if p.rows == nr && p.cols == nc {
                                                let p_idx = row_base + i;
                                                p.data.get(p_idx).copied().unwrap_or(0)
                                                    >= min_presence
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                },
                            })
                            .collect()
                    })
                    .collect();
                rows.into_iter().flatten().collect()
            }
            #[cfg(not(feature = "parallel"))]
            {
                let mut out = Vec::new();
                for r in 0..nr {
                    let mut best: Vec<(usize, f32)> = Vec::new();
                    let row_base = r.saturating_mul(nc);
                    for i in 0..nc {
                        let idx = row_base + i;
                        let w = *m.data.get(idx).unwrap_or(&0.0) as f32;
                        if w.abs() <= 1e-8 {
                            continue;
                        }
                        if best.len() < k {
                            best.push((i, w));
                        } else {
                            let mut min_idx = 0usize;
                            let mut min_w = best[0].1.abs();
                            for (bi, &(_, bw)) in best.iter().enumerate().skip(1) {
                                if bw.abs() < min_w {
                                    min_w = bw.abs();
                                    min_idx = bi;
                                }
                            }
                            if w.abs() > min_w {
                                best[min_idx] = (i, w);
                            }
                        }
                    }
                    for (i, w) in best.into_iter() {
                        let is_longterm = if longterm_min_presence == Some(0) {
                            true
                        } else if let Some(min_presence) = longterm_min_presence {
                            if let Some(p) = presence {
                                if p.rows == nr && p.cols == nc {
                                    let p_idx = row_base + i;
                                    p.data.get(p_idx).copied().unwrap_or(0) >= min_presence
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        out.push(CachedEdge {
                            from_layer,
                            to_layer,
                            from_idx: i,
                            to_idx: r,
                            weight: w,
                            kind,
                            is_longterm,
                        });
                    }
                }
                out
            }
        };

        if snap.net.num_sensory_neurons > 0 && snap.net.num_hidden_layers > 0 {
            edges.extend(push_topk(-1, 0, &snap.w_in, snap.p_in.as_ref(), "overlay"));
        }
        for l in 1..snap.net.num_hidden_layers {
            if let Some(w) = snap.w_hh_fwd.get(l - 1) {
                let p = snap.p_fwd.as_ref().and_then(|v| v.get(l - 1));
                edges.extend(push_topk((l - 1) as i32, l as i32, w, p, "overlay"));
            }
        }
        for l in 0..snap.net.num_hidden_layers.saturating_sub(1) {
            if let Some(w) = snap.w_hh_bwd.get(l) {
                let p = snap.p_bwd.as_ref().and_then(|v| v.get(l));
                edges.extend(push_topk((l + 1) as i32, l as i32, w, p, "overlay"));
            }
        }
        for l in 0..snap.net.num_hidden_layers {
            if let Some(w) = snap.w_hh_rec.get(l) {
                let p = snap.p_rec.as_ref().and_then(|v| v.get(l));
                edges.extend(push_topk(l as i32, l as i32, w, p, "overlay"));
            }
        }
        if snap.net.num_hidden_layers > 0 {
            edges.extend(push_topk(
                (snap.net.num_hidden_layers - 1) as i32,
                -2,
                &snap.w_out,
                snap.p_out.as_ref(),
                "overlay",
            ));
        }

        let mut sizes = Vec::new();
        if snap.net.num_hidden_layers > 0 {
            sizes.push(snap.w_in.rows);
            for l in 1..snap.net.num_hidden_layers {
                if let Some(w) = snap.w_hh_fwd.get(l - 1) {
                    sizes.push(w.rows);
                }
            }
        }

        let mut counts = Vec::new();
        if snap.net.num_hidden_layers > 0 {
            counts.resize(snap.net.num_hidden_layers, 0);
            counts[0] += count_nonzero(&snap.w_in);
            for (l, w) in snap.w_hh_fwd.iter().enumerate() {
                if l + 1 < counts.len() {
                    counts[l + 1] += count_nonzero(w);
                }
            }
            for (l, w) in snap.w_hh_bwd.iter().enumerate() {
                if l < counts.len() {
                    counts[l] += count_nonzero(w);
                }
            }
            for (l, w) in snap.w_hh_rec.iter().enumerate() {
                if l < counts.len() {
                    counts[l] += count_nonzero(w);
                }
            }
        }
        let output_count = count_nonzero(&snap.w_out);

        (edges, sizes, counts, output_count)
    }

    fn compute_cached_edges(overlay_density: usize, runner: &Runner) -> Vec<CachedEdge> {
        let k = overlay_density.max(1);
        let mut edges = Vec::new();
        let push_topk = |from_layer: i32,
                         to_layer: i32,
                         weights: &ndarray::Array2<f64>,
                         kind: &'static str,
                         is_longterm: Box<dyn Fn(usize, usize) -> bool + Send + Sync>|
         -> Vec<CachedEdge> {
            let nr = weights.nrows();
            let nc = weights.ncols();
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                let rows: Vec<Vec<CachedEdge>> = (0..nr)
                    .into_par_iter()
                    .map(|r| {
                        let mut best: Vec<(usize, f32)> = Vec::new();
                        for i in 0..nc {
                            let w = *weights.get((r, i)).unwrap_or(&0.0) as f32;
                            if w.abs() <= 1e-8 {
                                continue;
                            }
                            if best.len() < k {
                                best.push((i, w));
                            } else {
                                let mut min_idx = 0usize;
                                let mut min_w = best[0].1.abs();
                                for (bi, &(_, bw)) in best.iter().enumerate().skip(1) {
                                    if bw.abs() < min_w {
                                        min_w = bw.abs();
                                        min_idx = bi;
                                    }
                                }
                                if w.abs() > min_w {
                                    best[min_idx] = (i, w);
                                }
                            }
                        }
                        best.into_iter()
                            .map(|(i, w)| CachedEdge {
                                from_layer,
                                to_layer,
                                from_idx: i,
                                to_idx: r,
                                weight: w,
                                kind,
                                is_longterm: is_longterm(r, i),
                            })
                            .collect()
                    })
                    .collect();
                rows.into_iter().flatten().collect()
            }
            #[cfg(not(feature = "parallel"))]
            {
                let mut out = Vec::new();
                for r in 0..nr {
                    let mut best: Vec<(usize, f32)> = Vec::new();
                    for i in 0..nc {
                        let w = *weights.get((r, i)).unwrap_or(&0.0) as f32;
                        if w.abs() <= 1e-8 {
                            continue;
                        }
                        if best.len() < k {
                            best.push((i, w));
                        } else {
                            let mut min_idx = 0usize;
                            let mut min_w = best[0].1.abs();
                            for (bi, &(_, bw)) in best.iter().enumerate().skip(1) {
                                if bw.abs() < min_w {
                                    min_w = bw.abs();
                                    min_idx = bi;
                                }
                            }
                            if w.abs() > min_w {
                                best[min_idx] = (i, w);
                            }
                        }
                    }
                    for (i, w) in best.into_iter() {
                        out.push(CachedEdge {
                            from_layer,
                            to_layer,
                            from_idx: i,
                            to_idx: r,
                            weight: w,
                            kind,
                            is_longterm: is_longterm(r, i),
                        });
                    }
                }
                out
            }
        };

        let (in_l, out_l) = runner.get_io_layers();
        // S -> H(in_l)
        if runner.net.num_sensory_neurons > 0 && in_l < runner.net.num_hidden_layers {
            edges.extend(push_topk(
                -1,
                in_l as i32,
                &runner.w_in,
                "overlay",
                Box::new(|r, i| runner.is_longterm_in(r, i)),
            ));
        }
        // H(l-1) -> H(l) fwd
        for l in 1..runner.net.num_hidden_layers {
            if let Some(w) = runner.w_hh_fwd.get(l - 1) {
                edges.extend(push_topk(
                    (l - 1) as i32,
                    l as i32,
                    w,
                    "overlay",
                    Box::new(move |r, i| runner.is_longterm_fwd(l - 1, r, i)),
                ));
            }
        }
        // H(l+1) -> H(l) bwd
        for l in 0..runner.net.num_hidden_layers.saturating_sub(1) {
            if let Some(w) = runner.w_hh_bwd.get(l) {
                edges.extend(push_topk(
                    (l + 1) as i32,
                    l as i32,
                    w,
                    "overlay",
                    Box::new(move |r, i| runner.is_longterm_bwd(l, r, i)),
                ));
            }
        }
        // H(l) -> H(l) rec
        for l in 0..runner.net.num_hidden_layers {
            if let Some(w) = runner.w_hh_rec.get(l) {
                edges.extend(push_topk(
                    l as i32,
                    l as i32,
                    w,
                    "overlay",
                    Box::new(move |r, i| runner.is_longterm_rec(l, r, i)),
                ));
            }
        }
        // H(out_l) -> O
        if out_l < runner.net.num_hidden_layers {
            edges.extend(push_topk(
                out_l as i32,
                -2,
                &runner.w_out,
                "overlay",
                Box::new(|r, i| runner.is_longterm_out(r, i)),
            ));
        }
        edges
    }

    /// Refresh all UI-side activity buffers and cached positions to match the current Runner topology.
    fn refresh_ui_buffers(&mut self) {
        // When using LocalManaged, we need to get the size from that runner.
        // For simplicity, we just clear everything and let the drawing logic re-initialize it.
        self.sensory_positions.clear();
        self.hidden_positions.clear();
        self.output_positions.clear();
        #[cfg(feature = "growth3d")]
        self.reset_topology_pid_states();

        let (n_s, n_l, n_o) = match &self.view_source {
            ViewSource::Standalone => {
                if let Ok(r) = self.runner.try_read() {
                    (
                        r.net.num_sensory_neurons,
                        r.net.num_hidden_layers,
                        r.net.num_output_neurons,
                    )
                } else {
                    (
                        self.local_net.num_sensory_neurons,
                        self.local_net.num_hidden_layers,
                        self.local_net.num_output_neurons,
                    )
                }
            }
            ViewSource::ClusterGlobal(id) => {
                let mut cfg_opt: Option<NetworkConfig> = None;
                if let Some(net_status) = self.dist_network_registry.get(id) {
                    if !net_status.config_json.is_empty() {
                        if let Ok(cfg) =
                            serde_json::from_str::<NetworkConfig>(&net_status.config_json)
                        {
                            cfg_opt = Some(cfg);
                        }
                    }
                    let mut layers = net_status.num_layers.max(1) as usize;
                    let mut layer_sizes = vec![0usize; layers];
                    for range in net_status.distribution.values() {
                        for (&layer_idx, &count) in &range.layer_neuron_counts {
                            let li = layer_idx as usize;
                            if li >= layer_sizes.len() {
                                layer_sizes.resize(li + 1, 0);
                                layers = layer_sizes.len();
                            }
                            layer_sizes[li] = layer_sizes[li].saturating_add(count as usize);
                        }
                    }
                    if layer_sizes.iter().all(|&v| v == 0) {
                        if let Some(cfg) = cfg_opt.as_ref() {
                            layers = cfg.num_hidden_layers.max(1);
                            layer_sizes = vec![cfg.num_hidden_per_layer_initial.max(1); layers];
                        }
                    }
                    self.cached_layer_sizes = layer_sizes;
                    if let Some(cfg) = cfg_opt {
                        (cfg.num_sensory_neurons, layers, cfg.num_output_neurons)
                    } else {
                        (
                            self.local_net.num_sensory_neurons,
                            layers,
                            self.local_net.num_output_neurons,
                        )
                    }
                } else {
                    (
                        self.local_net.num_sensory_neurons,
                        self.local_net.num_hidden_layers,
                        self.local_net.num_output_neurons,
                    )
                }
            }
            ViewSource::LocalManaged(id) => {
                if let Some(ref node) = self.distributed_node {
                    if let Ok(state) = node.state.try_read() {
                        if let Some(net_arc) = state.networks.get(id) {
                            if let Ok(net) = net_arc.try_read() {
                                (
                                    net.runner.net.num_sensory_neurons,
                                    net.runner.net.num_hidden_layers,
                                    net.runner.net.num_output_neurons,
                                )
                            } else {
                                (
                                    self.local_net.num_sensory_neurons,
                                    self.local_net.num_hidden_layers,
                                    self.local_net.num_output_neurons,
                                )
                            }
                        } else {
                            (
                                self.local_net.num_sensory_neurons,
                                self.local_net.num_hidden_layers,
                                self.local_net.num_output_neurons,
                            )
                        }
                    } else {
                        (
                            self.local_net.num_sensory_neurons,
                            self.local_net.num_hidden_layers,
                            self.local_net.num_output_neurons,
                        )
                    }
                } else {
                    (
                        self.local_net.num_sensory_neurons,
                        self.local_net.num_hidden_layers,
                        self.local_net.num_output_neurons,
                    )
                }
            }
        };

        self.sensory_count = n_s;
        self.output_count = n_o;
        self.hidden_positions = vec![vec![]; n_l];
        self.sensory_activity = vec![0.0; n_s];
        self.hidden_activity = (0..n_l).map(|_| Vec::new()).collect(); // will be resized in update_activity
        self.output_activity = vec![0.0; n_o];
        self.previous_hidden_spikes = (0..n_l).map(|_| Vec::new()).collect();
        self.last_sensory_spikes = vec![0; n_s];
        self.raster_outputs.clear();
    }

    fn preferred_layout_for_view(
        &self,
        model: &NeuronModel,
        network_registry: &HashMap<String, NetworkStatus>,
    ) -> NetworkLayout {
        match &self.view_source {
            ViewSource::Standalone => {
                if matches!(model, NeuronModel::Aarnn) {
                    NetworkLayout::Aarnn
                } else {
                    NetworkLayout::Conventional
                }
            }
            ViewSource::LocalManaged(id) => {
                if network_registry
                    .get(id)
                    .map(|net| net.desired_aarnn_depth > 0)
                    .unwrap_or(false)
                {
                    NetworkLayout::Aarnn
                } else {
                    NetworkLayout::Conventional
                }
            }
            ViewSource::ClusterGlobal(id) => {
                // Prefer Aarnn if the registry says the network uses it, or if we
                // have already received 3D topology data from the remote node.
                let from_registry = network_registry
                    .get(id)
                    .map(|net| {
                        net.desired_aarnn_depth > 0
                            || net.neuron_model.eq_ignore_ascii_case("aarnn")
                    })
                    .unwrap_or(false);
                #[cfg(feature = "growth3d")]
                let from_topo = self
                    .cluster_topo_cache
                    .as_ref()
                    .map(|topo| {
                        !topo.layers.is_empty()
                            || !topo.sensory_nodes.is_empty()
                            || !topo.output_nodes.is_empty()
                    })
                    .unwrap_or(false);
                #[cfg(not(feature = "growth3d"))]
                let from_topo = false;
                if from_registry || from_topo {
                    NetworkLayout::Aarnn
                } else {
                    NetworkLayout::Conventional
                }
            }
        }
    }

    fn set_network_layout(&mut self, layout: NetworkLayout, auto: bool) {
        let changed = self.network_layout != layout;
        self.network_layout = layout;
        self.layout_auto = auto;
        // Only reset camera parameters if explicitly switching TO conventional layout from something else.
        if changed && matches!(layout, NetworkLayout::Conventional) {
            self.camera_yaw_degrees = 0.0;
            self.camera_pitch_degrees = 0.0;
            #[cfg(feature = "growth3d")]
            {
                // Prevent stale AARNN topology data from being reused in conventional layout.
                self.cached_edge_topo = None;
                // In ClusterGlobal mode the topology is fetched from a remote node and must
                // survive layout transitions — clearing it here would prevent the Aarnn view
                // from ever recovering once the fetch completes.
                if !matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
                    self.cluster_topo_cache = None;
                }
                self.reset_topology_pid_states();
            }
            #[cfg(feature = "growth3d")]
            if let Ok(mut snap) = self.ui_snapshot.try_write() {
                snap.topo_sensory.clear();
                snap.topo_hidden.clear();
                snap.topo_output.clear();
            }
        }
        if changed {
            self.refresh_ui_buffers();
        }
    }

    fn set_view_source(&mut self, source: ViewSource) {
        if self.view_source != source {
            self.view_source = source;
            self.dist_initial_view_selected = true;
            self.view_node_filter = None;
            self.layout_auto = true;
            self.refresh_ui_buffers();
        }
    }

    fn managed_playing_from_state(
        state_arc: Option<&Arc<RwLock<crate::distributed::NodeState>>>,
        network_id: &str,
    ) -> Option<bool> {
        let state_guard = state_arc?.try_read().ok()?;
        let net_arc = state_guard.networks.get(network_id)?;
        let net_guard = net_arc.try_read().ok()?;
        Some(net_guard.playing)
    }

    fn resolve_view_playing(
        &self,
        state_arc: Option<&Arc<RwLock<crate::distributed::NodeState>>>,
    ) -> Option<bool> {
        let resolve_managed = |network_id: &str| {
            self.dist_network_registry
                .get(network_id)
                .map(|net| net.playing)
                .or_else(|| self.dist_local_playing_cache.get(network_id).copied())
                .or_else(|| Self::managed_playing_from_state(state_arc, network_id))
        };
        match &self.view_source {
            ViewSource::Standalone => Some(self.playing),
            ViewSource::LocalManaged(id) | ViewSource::ClusterGlobal(id) => resolve_managed(id),
        }
    }

    fn distributed_view_auto_select_enabled() -> bool {
        std::env::var("NM_UI_AUTO_SELECT_DISTRIBUTED_VIEW")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true)
    }

    fn preferred_cluster_network_id(&self) -> Option<String> {
        if let ViewSource::ClusterGlobal(id) = &self.view_source {
            if self.dist_network_registry.contains_key(id) {
                return Some(id.clone());
            }
        }

        let mut network_ids: Vec<String> = self.dist_network_registry.keys().cloned().collect();
        network_ids.sort();

        let preferred = |require_playing: bool, require_distribution: bool| {
            network_ids
                .iter()
                .filter(|id| {
                    self.dist_network_registry
                        .get(*id)
                        .map(|status| {
                            (!require_playing || status.playing)
                                && (!require_distribution || !status.distribution.is_empty())
                        })
                        .unwrap_or(false)
                })
                .max_by_key(|id| {
                    self.dist_network_registry
                        .get(*id)
                        .map(|status| status.total_neurons)
                        .unwrap_or(0)
                })
                .cloned()
        };

        preferred(true, true)
            .or_else(|| preferred(false, true))
            .or_else(|| preferred(true, false))
            .or_else(|| network_ids.first().cloned())
    }

    fn maybe_select_initial_distributed_view(&mut self) {
        if self.dist_initial_view_selected {
            return;
        }
        if self.distributed_node.is_none() {
            self.dist_initial_view_selected = true;
            return;
        }
        if !matches!(self.view_source, ViewSource::Standalone) {
            self.dist_initial_view_selected = true;
            return;
        }
        // Default distributed UIs to the managed/cluster view so Start/Stop
        // reflects the network actually hosted by the node. Set
        // NM_UI_AUTO_SELECT_DISTRIBUTED_VIEW=0 to keep the old standalone
        // default.
        if !Self::distributed_view_auto_select_enabled() {
            self.dist_initial_view_selected = true;
            return;
        }

        let preferred = if self.dist_is_orchestrator {
            self.preferred_cluster_network_id()
                .map(ViewSource::ClusterGlobal)
        } else {
            let mut network_ids: Vec<String> =
                self.dist_local_playing_cache.keys().cloned().collect();
            network_ids.sort();
            network_ids
                .iter()
                .find(|id| *id == &self.brain_id)
                .cloned()
                .or_else(|| {
                    network_ids
                        .iter()
                        .find(|id| {
                            self.dist_local_playing_cache
                                .get(*id)
                                .copied()
                                .unwrap_or(false)
                        })
                        .cloned()
                })
                .or_else(|| network_ids.first().cloned())
                .map(ViewSource::LocalManaged)
        };

        if let Some(source) = preferred {
            let source_label = match &source {
                ViewSource::Standalone => "standalone",
                ViewSource::LocalManaged(_) => "local managed",
                ViewSource::ClusterGlobal(_) => "cluster",
            };
            self.set_view_source(source);
            self.status = format!(
                "Auto-selected {} view so Start/Stop tracks active network state",
                source_label
            );
            self.dist_initial_view_selected = true;
        }
    }

    fn reset_to_network_state(
        &mut self,
        lif: LIFParams,
        stdp: STDPParams,
        net_cfg: NetworkConfig,
        model: NeuronModel,
        learning: Learning,
        status: &str,
    ) {
        self.set_standalone_playing(false);
        let _ = self.sim_tx.send(SimControl::RecreateRunner(
            lif.clone(),
            stdp.clone(),
            net_cfg.clone(),
            model,
            learning,
        ));
        let _ = self
            .sim_tx
            .send(SimControl::SetProvider(Box::new(RandomProvider::new(
                net_cfg.num_sensory_neurons,
                self.random_spike_probability,
            ))));
        let _ = self.sim_tx.send(SimControl::Reset);
        self.refresh_ui_buffers();
        self.local_net = net_cfg;
        self.neuron_model = match model {
            NeuronModel::Lif => NeuronModelSel::Lif,
            NeuronModel::Izh(_) => NeuronModelSel::Izh,
            NeuronModel::Aarnn => NeuronModelSel::Aarnn,
        };
        self.learning = match learning {
            Learning::Stdp => LearningSel::Stdp,
            Learning::Hebb => LearningSel::Hebb,
            Learning::Oja => LearningSel::Oja,
            Learning::Aarnn => LearningSel::Aarnn,
        };
        self.input_source = InputSource::Random;
        self.loop_feedback = false;
        self.view_source = ViewSource::Standalone;
        self.view_node_filter = None;
        self.pending_import = None;
        self.last_import_report = None;
        self.probes.clear();
        self.next_probe_id = 1;
        self.aarnn_defaults_applied = false;
        if matches!(self.neuron_model, NeuronModelSel::Aarnn) {
            self.apply_aarnn_bio_defaults();
        }
        self.force_show_connections = false;
        self.pending_edge_cache = false;
        self.edge_cache_inflight = false;
        self.last_edge_cache_refresh = std::time::Instant::now();
        self.cached_edges.clear();
        self.cached_layer_sizes.clear();
        self.cached_conn_counts.clear();
        self.cached_output_conn_count = None;
        #[cfg(feature = "growth3d")]
        {
            self.cached_edge_topo = None;
        }
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            self.cached_skull_membrane = None;
        }
        self.last_conn_stats_refresh = std::time::Instant::now();
        self.status = status.to_string();
    }

    fn set_http_aer_status_message(
        &self,
        connected: bool,
        status_text: String,
        last_error: Option<String>,
    ) {
        if let Ok(mut stats) = self.http_aer_status.try_write() {
            stats.connected = connected;
            stats.status_text = status_text;
            stats.last_error = last_error;
            if !connected {
                stats.last_frame_time = None;
            }
        }
    }

    fn http_aer_status_snapshot(&self) -> HttpAerInputStatus {
        if let Ok(stats) = self.http_aer_status.try_read() {
            stats.clone()
        } else {
            HttpAerInputStatus::default()
        }
    }

    fn connect_http_aer_source(&mut self, sensory_count: usize) {
        let source_url = self.http_aer_source_url.trim().to_string();
        if source_url.is_empty() {
            self.set_http_aer_status_message(
                false,
                "Enter a source URL".to_string(),
                Some("missing URL".to_string()),
            );
            self.status = "HTTP AER source URL is empty".to_string();
            return;
        }
        if !source_url.starts_with("http://") && !source_url.starts_with("https://") {
            self.set_http_aer_status_message(
                false,
                "Invalid URL".to_string(),
                Some("URL must start with http:// or https://".to_string()),
            );
            self.status = "HTTP AER source URL must start with http:// or https://".to_string();
            return;
        }
        self.http_aer_source_url = source_url.clone();
        self.set_http_aer_status_message(true, "Connecting...".to_string(), None);
        let provider = HttpAerStreamProvider::new(
            source_url.clone(),
            self.http_aer_base,
            sensory_count.max(1),
            self.http_aer_status.clone(),
        );
        let _ = self
            .sim_tx
            .send(SimControl::SetProvider(Box::new(provider)));
        self.status = format!("HTTP AER source selected: {}", source_url);
    }

    fn set_standalone_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.playing_atomic.store(playing, Ordering::SeqCst);
        let _ = self.sim_tx.send(SimControl::SetPlaying(playing));
    }

    fn reset_and_start_standalone(&mut self, status: &str) {
        let _ = self.sim_tx.send(SimControl::Reset);
        self.refresh_ui_buffers();
        self.status = status.to_string();
        self.set_standalone_playing(true);
    }

    fn apply_cluster_control(
        &mut self,
        network_id: &str,
        action: control_update::Action,
        status: &str,
    ) {
        let view_scope = match &self.view_source {
            ViewSource::LocalManaged(_) => "Local network",
            ViewSource::ClusterGlobal(_) => "Cluster network",
            ViewSource::Standalone => "Standalone",
        };

        let playing_after = matches!(
            action,
            control_update::Action::Start | control_update::Action::Repeat
        );

        if let Some(node) = &self.distributed_node {
            let queue_result = node.apply_network_control(network_id, action);
            // "Cluster state busy" means the write lock was contended — the
            // direct gRPC path below will still deliver the command immediately,
            // so treat it as a soft failure and still update the UI optimistically.
            let fatal = match &queue_result {
                Err(e) if e.contains("Cluster state busy") => false,
                Err(_) => true,
                Ok(()) => false,
            };
            if !fatal {
                // Optimistic UI update: reflect the intended state immediately.
                self.dist_local_playing_cache
                    .insert(network_id.to_string(), playing_after);
                if let Some(net_status) = self.dist_network_registry.get_mut(network_id) {
                    match action {
                        control_update::Action::Start | control_update::Action::Repeat => {
                            net_status.playing = true;
                        }
                        control_update::Action::Stop
                        | control_update::Action::Reset
                        | control_update::Action::New => {
                            net_status.playing = false;
                        }
                    }
                }
                self.status = format!("{} {} ({})", view_scope, status, network_id);
            } else {
                self.status = format!(
                    "{} {} failed: {}",
                    view_scope,
                    status,
                    queue_result.unwrap_err()
                );
            }
        } else {
            self.status = "Cluster control unavailable".into();
        }

        // For ClusterGlobal views also send a direct gRPC UpdateNetwork to
        // every worker node that manages this network.  Workers now accept
        // ControlUpdate directly (they no longer require is_orchestrator for
        // this RPC variant), so this gives immediate effect even when the
        // heartbeat-queue path was temporarily busy.
        if matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
            if let Some(net_status) = self.dist_network_registry.get(network_id) {
                let node_ids: Vec<String> = net_status.distribution.keys().cloned().collect();
                let rt = self.runtime_handle.clone();
                let net_id = network_id.to_string();
                let action_i32 = action as i32;
                for node_id in node_ids {
                    if let Some(node_info) = self.dist_nodes.get(&node_id) {
                        let mut addr = node_info.address.clone();
                        if addr.is_empty() {
                            continue;
                        }
                        if !addr.starts_with("http://") && !addr.starts_with("https://") {
                            addr = format!("http://{}", addr);
                        }
                        let net_id_c = net_id.clone();
                        rt.spawn(async move {
                            if let Ok(mut client) =
                                DistributedNeuromorphicClient::connect(addr).await
                            {
                                let req = NetworkUpdateRequest {
                                    network_id: net_id_c,
                                    update: Some(network_update_request::Update::Control(
                                        ControlUpdate { action: action_i32 },
                                    )),
                                };
                                let _ = client.update_network(Request::new(req)).await;
                            }
                        });
                    }
                }
            }
        }
        if matches!(
            action,
            control_update::Action::Repeat
                | control_update::Action::Reset
                | control_update::Action::New
        ) {
            self.refresh_ui_buffers();
        }
    }

    fn export_view_config_json(&self) -> Result<String, String> {
        match &self.view_source {
            ViewSource::Standalone => {
                let runner = self
                    .runner
                    .try_read()
                    .map_err(|_| "Runner busy".to_string())?;
                runner.export_config_json().map_err(|e| e.to_string())
            }
            ViewSource::LocalManaged(id) => {
                let node = self
                    .distributed_node
                    .as_ref()
                    .ok_or_else(|| "Local managed network unavailable".to_string())?;
                let state = node
                    .state
                    .try_read()
                    .map_err(|_| "Cluster state busy".to_string())?;
                let net_arc = state
                    .networks
                    .get(id)
                    .cloned()
                    .ok_or_else(|| "Local managed network not found".to_string())?;
                drop(state);
                let net = net_arc
                    .try_read()
                    .map_err(|_| "Local managed network busy".to_string())?;
                net.runner.export_config_json().map_err(|e| e.to_string())
            }
            ViewSource::ClusterGlobal(id) => {
                if let Some(snap) = &self.cluster_snapshot_cache {
                    if self.cluster_snapshot_network_id.as_deref() == Some(id) {
                        return serde_json::to_string_pretty(&snap.net).map_err(|e| e.to_string());
                    }
                }
                if let Some(net_status) = self.dist_network_registry.get(id) {
                    if !net_status.config_json.is_empty() {
                        return Ok(net_status.config_json.clone());
                    }
                }
                Err("Cluster config not available yet".to_string())
            }
        }
    }

    fn export_view_network_json(&self) -> Result<String, String> {
        match &self.view_source {
            ViewSource::Standalone => {
                let runner = self
                    .runner
                    .try_read()
                    .map_err(|_| "Runner busy".to_string())?;
                runner.export_network_json().map_err(|e| e.to_string())
            }
            ViewSource::LocalManaged(id) => {
                let node = self
                    .distributed_node
                    .as_ref()
                    .ok_or_else(|| "Local managed network unavailable".to_string())?;
                let state = node
                    .state
                    .try_read()
                    .map_err(|_| "Cluster state busy".to_string())?;
                let net_arc = state
                    .networks
                    .get(id)
                    .cloned()
                    .ok_or_else(|| "Local managed network not found".to_string())?;
                drop(state);
                let net = net_arc
                    .try_read()
                    .map_err(|_| "Local managed network busy".to_string())?;
                net.runner.export_network_json().map_err(|e| e.to_string())
            }
            ViewSource::ClusterGlobal(id) => {
                if let Some(snap) = &self.cluster_snapshot_cache {
                    if self.cluster_snapshot_network_id.as_deref() == Some(id) {
                        return serde_json::to_string_pretty(snap).map_err(|e| e.to_string());
                    }
                }
                Err("Cluster snapshot not available yet".to_string())
            }
        }
    }

    fn queue_import(
        &mut self,
        kind: ImportKind,
        path: std::path::PathBuf,
        json: String,
        stdout: String,
        stderr: String,
    ) -> Result<(), String> {
        let kind_str = match kind {
            ImportKind::Tflite => "TFLite",
            ImportKind::Onnx => "ONNX",
            ImportKind::NeuroML => "NeuroML",
            ImportKind::PyNN => "PyNN",
            ImportKind::Nir => "NIR",
            ImportKind::Standard => "Network",
        };
        match self.view_source.clone() {
            ViewSource::Standalone => {
                if self.pending_import.is_some() {
                    self.pending_import = None;
                }
                let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                match self
                    .sim_tx
                    .send(SimControl::ImportNetworkWithReply(json, reply_tx))
                {
                    Ok(()) => {
                        self.cached_layer_sizes.clear();
                        self.cached_conn_counts.clear();
                        self.cached_output_conn_count = None;
                        self.pending_import = Some(PendingImport {
                            path,
                            kind,
                            stdout,
                            stderr,
                            rx: reply_rx,
                            result: None,
                        });
                        let status = if kind == ImportKind::Standard {
                            "Loading network...".to_string()
                        } else {
                            format!("Applying {} import...", kind_str)
                        };
                        self.status = status.clone();
                        Ok(())
                    }
                    Err(_) => Err(format!(
                        "{} import failed: simulation channel closed",
                        kind_str
                    )),
                }
            }
            ViewSource::LocalManaged(id) => self
                .import_network_json_to_local_managed(&id, &json, kind, &path)
                .map_err(|e| e.to_string()),
            ViewSource::ClusterGlobal(_) => Err(format!(
                "{} import not supported for cluster view",
                kind_str
            )),
        }
    }

    fn apply_config_to_local_managed(
        &mut self,
        network_id: &str,
        net: NetworkConfig,
    ) -> Result<(), String> {
        let node = self
            .distributed_node
            .as_ref()
            .ok_or_else(|| "Local managed network unavailable".to_string())?;
        let state = node
            .state
            .try_read()
            .map_err(|_| "Cluster state busy".to_string())?;
        let net_arc = state
            .networks
            .get(network_id)
            .cloned()
            .ok_or_else(|| "Local managed network not found".to_string())?;
        drop(state);
        let mut net_guard = net_arc
            .try_write()
            .map_err(|_| "Local managed network busy".to_string())?;
        let requires_recreate = net.num_hidden_layers != net_guard.runner.net.num_hidden_layers
            || net.clumping_design != net_guard.runner.net.clumping_design;
        if requires_recreate {
            let lif = net_guard.runner.lif.clone();
            let stdp = net_guard.runner.stdp.clone();
            let model = net_guard.runner.neuron_model;
            let learning = net_guard.runner.learning;
            net_guard.runner = Runner::new(lif, stdp, net.clone(), model, learning);
        } else {
            net_guard.runner.apply_config(net.clone());
        }
        net_guard.initial_config = net;
        Ok(())
    }

    fn import_network_json_to_local_managed(
        &mut self,
        network_id: &str,
        json: &str,
        kind: ImportKind,
        path: &std::path::Path,
    ) -> Result<(), String> {
        let node = self
            .distributed_node
            .as_ref()
            .ok_or_else(|| "Local managed network unavailable".to_string())?;
        let state = node
            .state
            .try_read()
            .map_err(|_| "Cluster state busy".to_string())?;
        let net_arc = state
            .networks
            .get(network_id)
            .cloned()
            .ok_or_else(|| "Local managed network not found".to_string())?;
        drop(state);
        let mut net_guard = net_arc
            .try_write()
            .map_err(|_| "Local managed network busy".to_string())?;
        net_guard
            .runner
            .import_network_json(json)
            .map_err(|e| e.to_string())?;
        let imported_layers = (net_guard.runner.net.num_hidden_layers + 1) as u32;
        let imported_model = net_guard.runner.neuron_model.to_str().to_string();
        let imported_learning = net_guard.runner.learning.to_str().to_string();
        let imported_snapshot_json = json.to_string();
        if let Some(node) = self.distributed_node.clone() {
            let network_id = network_id.to_string();
            self.runtime_handle.spawn(async move {
                let mut should_rebalance = false;
                {
                    let mut state = node.state.write().await;
                    if state.is_orchestrator {
                        if let Some(net_status) = state.network_registry.get_mut(&network_id) {
                            net_status.config_json = imported_snapshot_json.clone();
                            net_status.num_layers = imported_layers;
                            net_status.neuron_model = imported_model;
                            net_status.learning_rule = imported_learning;
                            crate::distributed::sync_network_status_deployment_from_payload(
                                net_status,
                                &imported_snapshot_json,
                            );
                        }
                        state
                            .network_snapshots
                            .insert(network_id.clone(), imported_snapshot_json);
                        should_rebalance = true;
                    }
                }
                if should_rebalance {
                    node.rebalance_networks().await;
                }
            });
        }
        let kind_str = match kind {
            ImportKind::Tflite => "TFLite",
            ImportKind::Onnx => "ONNX",
            ImportKind::NeuroML => "NeuroML",
            ImportKind::PyNN => "PyNN",
            ImportKind::Nir => "NIR",
            ImportKind::Standard => "Network",
        };
        let layer_sizes: Vec<usize> = (0..net_guard.runner.net.num_hidden_layers)
            .map(|li| net_guard.runner.layer_size(li).max(1))
            .collect();
        let hidden_summary = Self::hidden_summary(&layer_sizes);
        let summary = format!(
            "Imported {} from {} (S={} {} O={})",
            kind_str,
            path.display(),
            net_guard.runner.net.num_sensory_neurons,
            hidden_summary,
            net_guard.runner.net.num_output_neurons
        );
        self.cached_layer_sizes.clear();
        self.cached_conn_counts.clear();
        self.cached_output_conn_count = None;
        self.cached_edges.clear();
        #[cfg(feature = "growth3d")]
        {
            self.cached_edge_topo = None;
        }
        #[cfg(all(feature = "morpho", feature = "growth3d"))]
        {
            self.cached_skull_membrane = None;
        }
        self.refresh_ui_buffers();
        self.status = summary.clone();
        self.last_import_report = Some(summary);
        Ok(())
    }

    fn pull_activity(&mut self) {
        let node_opt = self.distributed_node.clone();
        let _runner_arc = self.runner.clone();
        match &self.view_source {
            ViewSource::Standalone => {
                // Done in run_simulation_step if playing.
                // If not playing, decay slightly.
                if !self.playing {
                    let decay = 0.95f32;
                    for v in &mut self.sensory_activity {
                        *v *= decay;
                    }
                    for layer in &mut self.hidden_activity {
                        for v in layer {
                            *v *= decay;
                        }
                    }
                    for v in &mut self.output_activity {
                        *v *= decay;
                    }
                }
            }
            ViewSource::LocalManaged(id) | ViewSource::ClusterGlobal(id) => {
                if let Some(node) = node_opt {
                    if let Ok(state) = node.state.try_read() {
                        if let Some(net_arc) = state.networks.get(id) {
                            if let Ok(net) = net_arc.try_read() {
                                self.sync_activity_from_runner(&net.runner);
                            }
                        } else {
                            // Decay if no data
                            let decay = 0.95f32;
                            for v in &mut self.sensory_activity {
                                *v *= decay;
                            }
                            for layer in &mut self.hidden_activity {
                                for v in layer {
                                    *v *= decay;
                                }
                            }
                            for v in &mut self.output_activity {
                                *v *= decay;
                            }
                            if matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
                                self.status = "Watching Cluster".into();
                            }
                        }
                    } else {
                        // Locked, skip frame but decay to avoid stale activity
                        let decay = 0.95f32;
                        for v in &mut self.sensory_activity {
                            *v *= decay;
                        }
                        for layer in &mut self.hidden_activity {
                            for v in layer {
                                *v *= decay;
                            }
                        }
                        for v in &mut self.output_activity {
                            *v *= decay;
                        }
                        if matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
                            self.status = "Watching Cluster".into();
                        }
                    }
                }
            }
        }
    }

    fn sync_activity_from_runner(&mut self, runner: &Runner) {
        let decay = 0.90f32;

        // 1. Sensory
        if self.sensory_activity.len() != runner.net.num_sensory_neurons {
            self.sensory_activity
                .resize(runner.net.num_sensory_neurons, 0.0);
        }
        self.last_sensory_spikes = runner
            .spk_hist_s
            .front()
            .map(|v| v.to_vec())
            .filter(|v| v.len() == runner.net.num_sensory_neurons)
            .unwrap_or_else(|| vec![0; runner.net.num_sensory_neurons]);
        for v in &mut self.sensory_activity {
            *v *= decay;
        }
        for (i, &sv) in runner.x_pre_in.iter().enumerate() {
            if sv > 0.1 && i < self.sensory_activity.len() {
                self.sensory_activity[i] = 1.0;
            }
        }

        // 2. Hidden
        if self.hidden_activity.len() != runner.net.num_hidden_layers {
            self.hidden_activity = (0..runner.net.num_hidden_layers)
                .map(|_| Vec::new())
                .collect();
        }
        if self.previous_hidden_spikes.len() != runner.net.num_hidden_layers {
            self.previous_hidden_spikes = (0..runner.net.num_hidden_layers)
                .map(|_| Vec::new())
                .collect();
        }

        for (li, sp) in runner.last_spk_h.iter().enumerate() {
            if li < self.hidden_activity.len() {
                if self.hidden_activity[li].len() != sp.len() {
                    self.hidden_activity[li].resize(sp.len(), 0.0);
                }
                if self.previous_hidden_spikes[li].len() != sp.len() {
                    self.previous_hidden_spikes[li].resize(sp.len(), 0);
                }

                for j in 0..sp.len() {
                    self.hidden_activity[li][j] *= decay;
                    if sp[j] != 0 {
                        self.hidden_activity[li][j] = 1.0;
                        self.previous_hidden_spikes[li][j] = 1;
                    } else {
                        self.previous_hidden_spikes[li][j] = 0;
                    }
                }
            }
        }

        // 3. Output
        if self.output_activity.len() != runner.net.num_output_neurons {
            self.output_activity
                .resize(runner.net.num_output_neurons, 0.0);
        }
        let mut col = vec![0i8; runner.net.num_output_neurons];
        let mut any = false;
        for k in 0..runner.net.num_output_neurons {
            self.output_activity[k] *= decay;
            if runner.last_spk_o[k] != 0 {
                self.output_activity[k] = 1.0;
                col[k] = 1;
                any = true;
            }
        }

        if any || (runner.t % 5 == 0) {
            self.raster_outputs.push_back(col);
            if self.raster_outputs.len() > self.raster_cols {
                self.raster_outputs.pop_front();
            }
        }

        self.status = format!(
            "Watching {}: t={} ms",
            match &self.view_source {
                ViewSource::Standalone => "Standalone",
                ViewSource::LocalManaged(id) => id,
                ViewSource::ClusterGlobal(id) => id,
            },
            runner.t_ms as i64
        );
    }

    fn sync_activity_from_standalone(&mut self, runner: &Runner) {
        let (runner_t, num_sensory, num_hidden, num_output, runner_t_ms, x_pre_in) = (
            runner.t,
            runner.net.num_sensory_neurons,
            runner.net.num_hidden_layers,
            runner.net.num_output_neurons,
            runner.t_ms,
            runner.x_pre_in.to_vec(),
        );

        let decay = 0.90f32;
        if self.sensory_activity.len() != num_sensory {
            self.sensory_activity.resize(num_sensory, 0.0);
        }
        self.last_sensory_spikes = runner
            .spk_hist_s
            .front()
            .map(|v| v.to_vec())
            .filter(|v| v.len() == num_sensory)
            .unwrap_or_else(|| vec![0; num_sensory]);
        for v in &mut self.sensory_activity {
            *v *= decay;
        }
        for (i, &sv) in x_pre_in.iter().enumerate() {
            if sv > 0.1 && i < self.sensory_activity.len() {
                self.sensory_activity[i] = 1.0;
            }
        }

        if self.hidden_activity.len() != num_hidden {
            self.hidden_activity = (0..num_hidden).map(|_| Vec::new()).collect();
        }
        if self.previous_hidden_spikes.len() != num_hidden {
            self.previous_hidden_spikes = (0..num_hidden).map(|_| Vec::new()).collect();
        }

        for li in 0..num_hidden {
            let sp = runner.last_spk_h[li].clone();
            if self.hidden_activity[li].len() != sp.len() {
                self.hidden_activity[li].resize(sp.len(), 0.0);
            }
            if self.previous_hidden_spikes[li].len() != sp.len() {
                self.previous_hidden_spikes[li].resize(sp.len(), 0);
            }
            for j in 0..sp.len() {
                self.hidden_activity[li][j] *= decay;
                if sp[j] != 0 {
                    self.hidden_activity[li][j] = 1.0;
                    self.previous_hidden_spikes[li][j] = 1;
                } else {
                    self.previous_hidden_spikes[li][j] = 0;
                }
            }
        }

        if self.output_activity.len() != num_output {
            self.output_activity.resize(num_output, 0.0);
        }
        let last_spk_o = runner.last_spk_o.to_vec();
        let mut col = vec![0i8; num_output];
        let mut any = false;
        for k in 0..num_output {
            self.output_activity[k] *= decay;
            if last_spk_o[k] != 0 {
                self.output_activity[k] = 1.0;
                col[k] = 1;
                any = true;
            }
        }
        if any || (runner_t % 5 == 0) {
            self.raster_outputs.push_back(col);
            if self.raster_outputs.len() > self.raster_cols {
                self.raster_outputs.pop_front();
            }
        }
        self.status = format!("Watching Standalone: t={} ms", runner_t_ms as i64);
    }

    fn get_layer_visuals(
        view_source: &ViewSource,
        _brain_id: &String,
        view_node_filter: &Option<String>,
        l: isize,
        default_color: egui::Color32,
        network_registry: &HashMap<String, NetworkStatus>,
    ) -> (egui::Color32, bool) {
        let brain_id = match view_source {
            ViewSource::Standalone => return (default_color, true),
            ViewSource::LocalManaged(id) => id,
            ViewSource::ClusterGlobal(id) => id,
        };

        if let Some(net_status) = network_registry.get(brain_id) {
            if l < 0 {
                return (egui::Color32::from_rgb(60, 140, 255), true);
            }

            let mut owning_node: Option<&String> = None;
            for (nid, range) in &net_status.distribution {
                if range.layers.contains(&(l as u32)) {
                    owning_node = Some(nid);
                    break;
                }
            }

            if let Some(nid) = owning_node {
                if let Some(filter) = view_node_filter {
                    if nid != filter {
                        return (default_color.gamma_multiply(0.15), false);
                    }
                }

                if matches!(view_source, ViewSource::ClusterGlobal(_)) {
                    let mut h = 0u64;
                    for b in nid.bytes() {
                        h = h.wrapping_mul(31).wrapping_add(b as u64);
                    }
                    let color =
                        egui::epaint::Hsva::new((h % 360) as f32 / 360.0, 0.7, 0.8, 1.0).into();
                    return (color, true);
                }
            } else if view_node_filter.is_some() {
                return (default_color.gamma_multiply(0.15), false);
            }
        }

        (default_color, true)
    }

    fn update_probes_from_standalone(&mut self, runner: &Runner) {
        if self.scope_paused {
            return;
        }

        let dt = runner.lif.dt.max(0.001) as f32;
        let last_sensory_spikes = runner
            .spk_hist_s
            .front()
            .map(|v| v.to_vec())
            .filter(|v| v.len() == runner.net.num_sensory_neurons)
            .unwrap_or_else(|| {
                if self.last_sensory_spikes.len() == runner.net.num_sensory_neurons {
                    self.last_sensory_spikes.clone()
                } else {
                    vec![0; runner.net.num_sensory_neurons]
                }
            });
        let desired_cap = ((self.scope_time_ms / dt).ceil() as usize).clamp(100, 20000);

        let bands_guard = self.spectral_bands.try_read().ok();
        let samples: Vec<f32> = self
            .probes
            .iter()
            .map(|p| {
                if p.enabled {
                    sample_probe_value(
                        runner,
                        &last_sensory_spikes,
                        bands_guard.as_deref().map(|v| v.as_slice()),
                        p,
                    )
                    .unwrap_or(f32::NAN)
                } else {
                    f32::NAN
                }
            })
            .collect();

        for (idx, val) in samples.into_iter().enumerate() {
            let pr = &mut self.probes[idx];
            if !pr.enabled {
                continue;
            }
            if pr.capacity != desired_cap {
                pr.data.clear();
                pr.data.resize(desired_cap, 0.0);
                pr.capacity = desired_cap;
                pr.write_idx = 0;
            }
            pr.push(val);
        }
    }

    fn update_probes_from_snapshot(&mut self, dt: f32, sensory_spikes: &[i8]) {
        if self.scope_paused {
            return;
        }
        let dt = dt.max(0.001);
        let desired_cap = ((self.scope_time_ms / dt).ceil() as usize).clamp(100, 20000);
        let bands_guard = self.spectral_bands.try_read().ok();
        let samples: Vec<f32> = self
            .probes
            .iter()
            .map(|p| {
                if !p.enabled {
                    f32::NAN
                } else {
                    match (p.kind, p.target) {
                        (ProbeKind::Spike, ProbeTarget::Sensory(i)) => sensory_spikes
                            .get(i)
                            .copied()
                            .map(|v| v as f32)
                            .unwrap_or(f32::NAN),
                        (ProbeKind::Level, ProbeTarget::Band(b)) => bands_guard
                            .as_deref()
                            .and_then(|bands| bands.get(b).copied())
                            .unwrap_or(f32::NAN),
                        _ => f32::NAN,
                    }
                }
            })
            .collect();
        for (idx, val) in samples.into_iter().enumerate() {
            let pr = &mut self.probes[idx];
            if !pr.enabled {
                continue;
            }
            if pr.capacity != desired_cap {
                pr.data.clear();
                pr.data.resize(desired_cap, 0.0);
                pr.capacity = desired_cap;
                pr.write_idx = 0;
            }
            pr.push(val);
        }
    }

    #[cfg(all(feature = "robot_io", unix))]
    fn apply_ipc_config(&mut self, handshake: IpcHandshake) {
        self.ipc_last_handshake = Some(handshake.clone());
        let (n_s_raw, n_o_raw) = resolve_ipc_handshake_sizes(
            &handshake,
            self.local_net.num_sensory_neurons.max(1),
            self.local_net.num_output_neurons.max(1),
        );
        let k = self.ipc_neurons_per_value.max(1);

        let s_total = n_s_raw * k;
        let o_total = n_o_raw * k;

        let mut mapping = IoMapping::new(s_total, o_total);
        if handshake.s_names.is_empty() {
            for i in 0..n_s_raw {
                mapping.add_port(PortSpec::new(format!("S{}", i), PortKind::Sensor, i * k, k));
            }
        } else {
            for (i, name) in handshake.s_names.iter().enumerate() {
                mapping.add_port(PortSpec::new(name.clone(), PortKind::Sensor, i * k, k));
            }
        }
        if handshake.o_names.is_empty() {
            for i in 0..n_o_raw {
                mapping.add_port(PortSpec::new(
                    format!("O{}", i),
                    PortKind::Actuator,
                    i * k,
                    k,
                ));
            }
        } else {
            for (i, name) in handshake.o_names.iter().enumerate() {
                mapping.add_port(PortSpec::new(name.clone(), PortKind::Actuator, i * k, k));
            }
        }
        self.ipc_mapping = Some(mapping);

        // Keep UI fallback config in sync with negotiated IPC dimensions so
        // diagnostics and manual rebind actions don't regress to stale S/O.
        self.local_net.num_sensory_neurons = s_total;
        self.local_net.num_output_neurons = o_total;

        let _ = self.sim_tx.send(SimControl::ResizeSensory(s_total));
        let _ = self.sim_tx.send(SimControl::ResizeOutput(o_total));
        let reward_note = handshake.reward_name.as_deref().unwrap_or("none");
        self.status = format!(
            "IPC configured: S={}x{}, O={}x{} (reward: {})",
            n_s_raw, k, n_o_raw, k, reward_note
        );
    }
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum ProbeKind {
    Spike,
    Membrane,
    Current,
    Level,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum ProbeTarget {
    Sensory(usize),
    Hidden(usize, usize), // (layer, j)
    Output(usize),
    ConnIn(usize, usize),         // (i -> j) S→H0
    ConnFwd(usize, usize, usize), // (l, i -> j) H(l)→H(l+1)
    ConnBwd(usize, usize, usize), // (l, i <- j) H(l+1)→H(l)
    ConnOut(usize, usize),        // (j -> k) H_last→O
    ConnRec(usize, usize, usize), // (l, i -> j) H(l)→H(l)
    Band(usize),                  // Provider spectral band index
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum DetailBioOrient {
    AsIs,
    MirrorAxes,
    AlignAxonRight,
}

#[cfg(feature = "ui")]
impl DetailBioOrient {
    fn label(self) -> &'static str {
        match self {
            DetailBioOrient::AsIs => "As-is",
            DetailBioOrient::MirrorAxes => "Mirror dendrites left / axons right",
            DetailBioOrient::AlignAxonRight => "Auto-orient axon to right",
        }
    }
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
#[derive(Clone, Copy, Default)]
struct UiPid2State {
    pos: egui::Pos2,
    prev_err: egui::Vec2,
    integral: egui::Vec2,
    initialized: bool,
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
#[derive(Clone, Copy, Default)]
struct UiPid3State {
    pos: [f32; 3],
    prev_err: [f32; 3],
    integral: [f32; 3],
    initialized: bool,
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
fn pid_smooth_pos2(
    state: &mut UiPid2State,
    target: egui::Pos2,
    dt: f32,
    kp: f32,
    ki: f32,
    kd: f32,
) -> egui::Pos2 {
    if !state.initialized {
        state.pos = target;
        state.prev_err = egui::Vec2::ZERO;
        state.integral = egui::Vec2::ZERO;
        state.initialized = true;
        return target;
    }
    let err = target - state.pos;
    state.integral += err * dt;
    let integral_limit = 2000.0;
    state.integral.x = state.integral.x.clamp(-integral_limit, integral_limit);
    state.integral.y = state.integral.y.clamp(-integral_limit, integral_limit);
    let derivative = (err - state.prev_err) * (1.0 / dt.max(0.001));
    let mut delta = err * kp + state.integral * ki + derivative * kd;
    let max_step = (2400.0 * dt).max(1.0);
    let delta_sq = delta.length_sq();
    if delta_sq > max_step * max_step {
        delta *= max_step / delta_sq.sqrt();
    }
    state.pos += delta;
    state.prev_err = err;
    state.pos
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
fn pid_smooth_positions(
    states: &mut Vec<UiPid2State>,
    targets: &[egui::Pos2],
    dt: f32,
    kp: f32,
    ki: f32,
    kd: f32,
) -> Vec<egui::Pos2> {
    if states.len() != targets.len() {
        states.resize(targets.len(), UiPid2State::default());
    }
    let mut out = Vec::with_capacity(targets.len());
    for (idx, target) in targets.iter().enumerate() {
        out.push(pid_smooth_pos2(&mut states[idx], *target, dt, kp, ki, kd));
    }
    out
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
fn pid_smooth_layered_positions(
    states: &mut Vec<Vec<UiPid2State>>,
    targets: &[Vec<egui::Pos2>],
    dt: f32,
    kp: f32,
    ki: f32,
    kd: f32,
) -> Vec<Vec<egui::Pos2>> {
    if states.len() != targets.len() {
        states.resize_with(targets.len(), Vec::new);
    }
    let mut out = Vec::with_capacity(targets.len());
    for (layer_idx, layer_targets) in targets.iter().enumerate() {
        let layer_states = &mut states[layer_idx];
        if layer_states.len() != layer_targets.len() {
            layer_states.resize(layer_targets.len(), UiPid2State::default());
        }
        let mut layer_out = Vec::with_capacity(layer_targets.len());
        for (idx, target) in layer_targets.iter().enumerate() {
            layer_out.push(pid_smooth_pos2(
                &mut layer_states[idx],
                *target,
                dt,
                kp,
                ki,
                kd,
            ));
        }
        out.push(layer_out);
    }
    out
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
fn pid_smooth_vec3(
    state: &mut UiPid3State,
    target: [f32; 3],
    dt: f32,
    kp: f32,
    ki: f32,
    kd: f32,
) -> [f32; 3] {
    if !state.initialized {
        state.pos = target;
        state.prev_err = [0.0; 3];
        state.integral = [0.0; 3];
        state.initialized = true;
        return target;
    }
    let inv_dt = 1.0 / dt.max(0.001);
    let mut delta = [0.0f32; 3];
    for axis in 0..3 {
        let err = target[axis] - state.pos[axis];
        state.integral[axis] = (state.integral[axis] + err * dt).clamp(-2.0, 2.0);
        let derivative = (err - state.prev_err[axis]) * inv_dt;
        delta[axis] = err * kp + state.integral[axis] * ki + derivative * kd;
        state.prev_err[axis] = err;
    }
    let delta_mag_sq = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
    let max_step = (2.0 * dt).max(0.002);
    if delta_mag_sq > max_step * max_step {
        let scale = max_step / delta_mag_sq.sqrt();
        delta[0] *= scale;
        delta[1] *= scale;
        delta[2] *= scale;
    }
    state.pos[0] += delta[0];
    state.pos[1] += delta[1];
    state.pos[2] += delta[2];
    state.pos
}

#[cfg(all(feature = "ui", feature = "growth3d"))]
fn centroid_of_projected_positions(
    sensory: &[egui::Pos2],
    hidden: &[Vec<egui::Pos2>],
    output: &[egui::Pos2],
) -> Option<egui::Pos2> {
    let mut sum = egui::Vec2::ZERO;
    let mut count = 0usize;
    for p in sensory {
        sum += p.to_vec2();
        count += 1;
    }
    for layer in hidden {
        for p in layer {
            sum += p.to_vec2();
            count += 1;
        }
    }
    for p in output {
        sum += p.to_vec2();
        count += 1;
    }
    if count == 0 {
        None
    } else {
        Some(egui::pos2(sum.x / count as f32, sum.y / count as f32))
    }
}

#[cfg(all(feature = "ui", feature = "morpho"))]
#[derive(Clone, Copy, Default)]
struct Pid3State {
    pos: crate::morphology::Point3,
    prev_err: crate::morphology::Point3,
    integral: crate::morphology::Point3,
    initialized: bool,
}

#[cfg(all(feature = "ui", feature = "morpho"))]
fn pid_smooth_point(
    state: &mut Pid3State,
    target: crate::morphology::Point3,
    dt: f32,
    kp: f32,
    ki: f32,
    kd: f32,
) -> crate::morphology::Point3 {
    if !state.initialized {
        state.pos = target;
        state.prev_err = crate::morphology::Point3::default();
        state.integral = crate::morphology::Point3::default();
        state.initialized = true;
        return target;
    }
    let err = target.sub(state.pos);
    state.integral = state.integral.add(err.mul(dt));
    let derivative = err.sub(state.prev_err).mul(1.0 / dt.max(0.001));
    let delta = err
        .mul(kp)
        .add(state.integral.mul(ki))
        .add(derivative.mul(kd));
    state.pos = state.pos.add(delta);
    state.prev_err = err;
    state.pos
}

#[cfg(feature = "ui")]
#[derive(Clone)]
struct Probe {
    id: u32,
    name: String,
    color: egui::Color32,
    enabled: bool,
    target: ProbeTarget,
    kind: ProbeKind,
    data: Vec<f32>,
    write_idx: usize,
    capacity: usize,
}

#[cfg(feature = "ui")]
#[derive(Clone, Default)]
struct UiSnapshot {
    sensory_spikes: Vec<i8>,
    hidden_spikes: Vec<Vec<i8>>,
    output_spikes: Vec<i8>,
    num_sensory: usize,
    num_hidden_layers: usize,
    num_output: usize,
    #[cfg(feature = "growth3d")]
    topo_sensory: Vec<crate::topology::Node3D>,
    #[cfg(feature = "growth3d")]
    topo_hidden: Vec<Vec<crate::topology::Node3D>>,
    #[cfg(feature = "growth3d")]
    topo_output: Vec<crate::topology::Node3D>,
}

#[cfg(feature = "ui")]
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct SnapshotTopo {
    net: crate::config::NetworkConfig,
    #[cfg(feature = "growth3d")]
    topo: Option<crate::topology::Topology3D>,
}

#[cfg(feature = "ui")]
impl Probe {
    fn new(
        id: u32,
        name: String,
        color: egui::Color32,
        target: ProbeTarget,
        kind: ProbeKind,
        capacity: usize,
    ) -> Self {
        Self {
            id,
            name,
            color,
            enabled: true,
            target,
            kind,
            data: vec![0.0; capacity],
            write_idx: 0,
            capacity,
        }
    }
    fn push(&mut self, v: f32) {
        if self.capacity == 0 {
            return;
        }
        let idx = self.write_idx % self.capacity;
        self.data[idx] = v;
        self.write_idx = (self.write_idx + 1) % self.capacity;
    }
}

#[cfg(feature = "ui")]
fn sample_probe_value(
    runner: &Runner,
    last_sensory_spikes: &[i8],
    last_bands: Option<&[f32]>,
    p: &Probe,
) -> Option<f32> {
    let r = runner;
    match (p.kind, p.target) {
        (ProbeKind::Spike, ProbeTarget::Sensory(i)) => {
            last_sensory_spikes.get(i).copied().map(|v| v as f32)
        }
        (ProbeKind::Spike, ProbeTarget::Hidden(l, j)) => r
            .last_spk_h
            .get(l)
            .and_then(|a| a.get(j))
            .copied()
            .map(|v| v as f32),
        (ProbeKind::Spike, ProbeTarget::Output(k)) => {
            r.last_spk_o.get(k).copied().map(|v| v as f32)
        }
        (ProbeKind::Membrane, ProbeTarget::Hidden(l, j)) => r
            .v_h
            .get(l)
            .and_then(|a| a.get(j))
            .copied()
            .map(|v| v as f32),
        (ProbeKind::Membrane, ProbeTarget::Output(k)) => r.v_o.get(k).copied().map(|v| v as f32),
        (ProbeKind::Level, ProbeTarget::Band(b)) => {
            last_bands.and_then(|bands| bands.get(b).copied())
        }
        (ProbeKind::Current, ProbeTarget::ConnIn(i, j)) => {
            // Approx per-connection current: weight * pre_spike (S→H0)
            if j < r.v_h.get(0).map(|a| a.len()).unwrap_or(0) && i < r.net.num_sensory_neurons {
                let pre = last_sensory_spikes.get(i).copied().unwrap_or(0) as f64;
                let w = *r.w_in.get((j, i)).unwrap_or(&0.0);
                Some((pre * w) as f32)
            } else {
                None
            }
        }
        (ProbeKind::Current, ProbeTarget::ConnFwd(l, i, j)) => {
            // Approx per-connection current: w * pre_spike (H→H)
            if l < r.w_hh_fwd.len()
                && i < r.last_spk_h.get(l).map(|a| a.len()).unwrap_or(0)
                && j < r.last_spk_h.get(l + 1).map(|a| a.len()).unwrap_or(0)
            {
                let pre = r.last_spk_h[l].get(i).copied().unwrap_or(0) as f64;
                let w = *r.w_hh_fwd[l].get((j, i)).unwrap_or(&0.0);
                Some((pre * w) as f32)
            } else {
                None
            }
        }
        (ProbeKind::Current, ProbeTarget::ConnBwd(l, i, j)) => {
            // Backward matrix: w_bwd(l)[i,j] with pre from layer l+1 index j
            if l < r.w_hh_bwd.len()
                && j < r.last_spk_h.get(l + 1).map(|a| a.len()).unwrap_or(0)
                && i < r.last_spk_h.get(l).map(|a| a.len()).unwrap_or(0)
            {
                let pre = r.last_spk_h[l + 1].get(j).copied().unwrap_or(0) as f64;
                let w = *r.w_hh_bwd[l].get((i, j)).unwrap_or(&0.0);
                Some((pre * w) as f32)
            } else {
                None
            }
        }
        (ProbeKind::Current, ProbeTarget::ConnOut(j, k)) => {
            // H_last→O: w_out[k,j] * pre_spike
            let l_count_h = r.last_spk_h.len();
            if l_count_h == 0 {
                return None;
            }
            if j < r.last_spk_h[l_count_h - 1].len() && k < r.net.num_output_neurons {
                let pre = r.last_spk_h[l_count_h - 1].get(j).copied().unwrap_or(0) as f64;
                let w = *r.w_out.get((k, j)).unwrap_or(&0.0);
                Some((pre * w) as f32)
            } else {
                None
            }
        }
        (ProbeKind::Current, ProbeTarget::ConnRec(l, i, j)) => {
            if l < r.w_hh_rec.len()
                && i < r.last_spk_h.get(l).map(|a| a.len()).unwrap_or(0)
                && j < r.last_spk_h.get(l).map(|a| a.len()).unwrap_or(0)
            {
                let pre = r.last_spk_h[l].get(i).copied().unwrap_or(0) as f64;
                let w = *r.w_hh_rec[l].get((j, i)).unwrap_or(&0.0);
                Some((pre * w) as f32)
            } else {
                None
            }
        }
        // Fallback: try total currents if available (UI Runner caches)
        (ProbeKind::Current, ProbeTarget::Hidden(0, j)) => r
            .last_i_h0
            .as_ref()
            .and_then(|v| v.get(j))
            .copied()
            .map(|v| v as f32),
        (ProbeKind::Current, ProbeTarget::Hidden(l, j)) => {
            if l == 0 {
                return None;
            }
            r.last_i_f
                .get(l)
                .and_then(|v| v.get(j))
                .copied()
                .map(|v| v as f32)
        }
        (ProbeKind::Current, ProbeTarget::Output(k)) => r
            .last_i_o
            .as_ref()
            .and_then(|v| v.get(k))
            .copied()
            .map(|v| v as f32),
        _ => None,
    }
}

// -------- Probe persistence (UI only) --------
#[cfg(feature = "ui")]
#[derive(serde::Serialize, serde::Deserialize)]
struct ProbeMeta {
    id: u32,
    name: String,
    color_rgba: [u8; 4],
    enabled: bool,
    kind: String,
    target: String,
}

#[cfg(feature = "ui")]
impl App {
    // ---------------- Python tools helpers ----------------
    fn verify_python(p: &str) -> bool {
        std::process::Command::new(p)
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    fn resolve_python(&self) -> anyhow::Result<String> {
        Self::resolve_python_with_override(self.python_path.clone())
    }

    fn resolve_python_with_override(override_path: Option<String>) -> anyhow::Result<String> {
        // 1) User override
        if let Some(p) = &override_path {
            if !p.is_empty() && Self::verify_python(p) {
                return Ok(p.clone());
            }
        }
        // 2) Env var
        if let Ok(p) = std::env::var("NMD_PYTHON") {
            if Self::verify_python(&p) {
                return Ok(p);
            }
        }
        // 3) .venv in cwd
        if let Ok(cwd) = std::env::current_dir() {
            let cand = cwd.join(".venv").join("bin").join("python");
            if cand.exists() {
                let s = cand.to_string_lossy().to_string();
                if Self::verify_python(&s) {
                    return Ok(s);
                }
            }
            let cand_win = cwd.join(".venv").join("Scripts").join("python.exe");
            if cand_win.exists() {
                let s = cand_win.to_string_lossy().to_string();
                if Self::verify_python(&s) {
                    return Ok(s);
                }
            }
        }
        // 4) PATH
        for name in ["python3", "python"] {
            if Self::verify_python(name) {
                return Ok(name.to_string());
            }
        }
        Err(anyhow::anyhow!(
            "No working Python interpreter found. Set NMD_PYTHON or configure a venv."
        ))
    }

    fn resolve_tool(script_file: &str) -> anyhow::Result<std::path::PathBuf> {
        use std::path::{Path, PathBuf};
        let mut tried: Vec<PathBuf> = Vec::new();

        let candidates = [script_file.to_string(), format!("{}c", script_file)];

        // 0) Env override for tools dir
        if let Ok(tools_dir) = std::env::var("NMD_TOOLS_DIR") {
            for candidate in &candidates {
                let p = Path::new(&tools_dir).join(candidate);
                tried.push(p.clone());
                if p.exists() {
                    return Ok(p);
                }
            }
        }

        // 1) Relative to current working directory
        for candidate in &candidates {
            let rel = Path::new("tools").join(candidate);
            tried.push(rel.clone());
            if rel.exists() {
                return Ok(rel);
            }
        }

        // 2) CARGO_MANIFEST_DIR (project root) if available
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            for candidate in &candidates {
                let p = Path::new(&manifest_dir).join("tools").join(candidate);
                tried.push(p.clone());
                if p.exists() {
                    return Ok(p);
                }
            }
        }

        // 3) Next to current executable, and parents
        if let Ok(exe) = std::env::current_exe() {
            let mut cur = exe.parent().map(Path::to_path_buf);
            for _ in 0..4 {
                // check a few ancestors
                if let Some(dir) = cur {
                    for candidate in &candidates {
                        let p = dir.join("tools").join(candidate);
                        tried.push(p.clone());
                        if p.exists() {
                            return Ok(p);
                        }
                    }
                    cur = dir.parent().map(Path::to_path_buf);
                } else {
                    break;
                }
            }
        }

        // Build helpful error listing attempted paths
        let mut msg = format!("Tool script not found: {}\nTried:\n", script_file);
        for p in tried {
            msg.push_str(&format!(" - {}\n", p.display()));
        }
        msg.push_str("Tip: set NMD_TOOLS_DIR to the folder containing the scripts, or run from the project root.\n");
        Err(anyhow::anyhow!(msg))
    }

    fn run_tool_with_python(
        python_override: Option<String>,
        script_file: &str,
        args: &[std::ffi::OsString],
    ) -> anyhow::Result<std::process::Output> {
        let py = Self::resolve_python_with_override(python_override)
            .map_err(|e| anyhow::anyhow!(format!("Python resolve failed: {}", e)))?;
        let tool = Self::resolve_tool(script_file)?;
        let mut cmd = std::process::Command::new(&py);
        cmd.arg(tool.as_os_str());
        for a in args {
            cmd.arg(a);
        }
        match cmd.output() {
            Ok(o) => Ok(o),
            Err(e) => {
                if let Some(8) = e.raw_os_error() {
                    Err(anyhow::anyhow!(
                        "Exec format error launching Python. The configured interpreter may be invalid or wrong-architecture. Set NMD_PYTHON to a working python3, or install Python and required packages."
                    ))
                } else {
                    Err(anyhow::anyhow!(format!("Failed to run python: {}", e)))
                }
            }
        }
    }
    fn export_probes_json(&self) -> anyhow::Result<String> {
        let metas: Vec<ProbeMeta> = self
            .probes
            .iter()
            .map(|p| ProbeMeta {
                id: p.id,
                name: p.name.clone(),
                color_rgba: [p.color.r(), p.color.g(), p.color.b(), p.color.a()],
                enabled: p.enabled,
                kind: match p.kind {
                    ProbeKind::Spike => "Spike",
                    ProbeKind::Membrane => "Membrane",
                    ProbeKind::Current => "Current",
                    ProbeKind::Level => "Level",
                }
                .to_string(),
                target: match p.target {
                    ProbeTarget::Sensory(i) => format!("Sensory:{}", i),
                    ProbeTarget::Hidden(l, j) => format!("Hidden:{},{}", l, j),
                    ProbeTarget::Output(k) => format!("Output:{}", k),
                    ProbeTarget::ConnIn(i, j) => format!("ConnIn:{},{}", i, j),
                    ProbeTarget::ConnFwd(l, i, j) => format!("ConnFwd:{},{},{}", l, i, j),
                    ProbeTarget::ConnBwd(l, i, j) => format!("ConnBwd:{},{},{}", l, i, j),
                    ProbeTarget::ConnOut(j, k) => format!("ConnOut:{},{}", j, k),
                    ProbeTarget::ConnRec(l, i, j) => format!("ConnRec:{},{},{}", l, i, j),
                    ProbeTarget::Band(b) => format!("Band:{}", b),
                },
            })
            .collect();
        Ok(serde_json::to_string_pretty(&metas)?)
    }

    fn import_probes_json(&mut self, s: &str) -> anyhow::Result<()> {
        let metas: Vec<ProbeMeta> = serde_json::from_str(s)?;
        self.probes.clear();
        self.next_probe_id = 1;
        for m in metas {
            // parse kind
            let kind = match m.kind.as_str() {
                "Spike" => ProbeKind::Spike,
                "Membrane" => ProbeKind::Membrane,
                "Current" => ProbeKind::Current,
                "Level" => ProbeKind::Level,
                _ => ProbeKind::Spike,
            };
            // parse target
            let target = if let Some(rest) = m.target.strip_prefix("Sensory:") {
                ProbeTarget::Sensory(rest.parse::<usize>().unwrap_or(0))
            } else if let Some(rest) = m.target.strip_prefix("Hidden:") {
                let mut it = rest.split(',');
                let l = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let j = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                ProbeTarget::Hidden(l, j)
            } else if let Some(rest) = m.target.strip_prefix("Output:") {
                ProbeTarget::Output(rest.parse::<usize>().unwrap_or(0))
            } else if let Some(rest) = m.target.strip_prefix("ConnIn:") {
                let mut it = rest.split(',');
                let i = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let j = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                ProbeTarget::ConnIn(i, j)
            } else if let Some(rest) = m.target.strip_prefix("ConnFwd:") {
                let mut it = rest.split(',');
                let l = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let i = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let j = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                ProbeTarget::ConnFwd(l, i, j)
            } else if let Some(rest) = m.target.strip_prefix("ConnBwd:") {
                let mut it = rest.split(',');
                let l = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let i = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let j = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                ProbeTarget::ConnBwd(l, i, j)
            } else if let Some(rest) = m.target.strip_prefix("ConnOut:") {
                let mut it = rest.split(',');
                let j = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let k = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                ProbeTarget::ConnOut(j, k)
            } else if let Some(rest) = m.target.strip_prefix("ConnRec:") {
                let mut it = rest.split(',');
                let l = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let i = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                let j = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
                ProbeTarget::ConnRec(l, i, j)
            } else if let Some(rest) = m.target.strip_prefix("Band:") {
                ProbeTarget::Band(rest.parse::<usize>().unwrap_or(0))
            } else {
                ProbeTarget::Sensory(0)
            };
            let color = egui::Color32::from_rgba_unmultiplied(
                m.color_rgba[0],
                m.color_rgba[1],
                m.color_rgba[2],
                m.color_rgba[3],
            );
            let id = self.next_probe_id;
            self.next_probe_id += 1;
            let mut pr = Probe::new(id, m.name, color, target, kind, 10_000);
            pr.enabled = m.enabled;
            self.probes.push(pr);
        }
        Ok(())
    }
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum InputSource {
    Random,
    Theta,
    ExternalHttpAer,
    AudioFile,
    Microphone,
    #[cfg(feature = "image_input")]
    ImageFile,
    #[cfg(feature = "video_input")]
    VideoFile,
    #[cfg(feature = "webcam_input")]
    Webcam,
    #[cfg(feature = "robot_io")]
    ExternalIpc,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum NeuronModelSel {
    Lif,
    Izh,
    Aarnn,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq, Debug)]
enum IzhPreset {
    RS,
    FS,
    IB,
    CH,
    LTS,
    RZ,
    TC,
    P,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum LearningSel {
    Stdp,
    Hebb,
    Oja,
    Aarnn,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum NetworkLayout {
    Conventional,
    Aarnn,
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, PartialEq)]
enum TfliteImportMode {
    Mlp,
    Cnn,
}

#[cfg(feature = "ui")]
#[derive(PartialEq, Clone, Debug)]
enum ViewSource {
    Standalone,
    LocalManaged(String),
    ClusterGlobal(String),
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, Debug)]
enum ContextPick {
    Sensory(usize),
    Hidden(usize, usize),
    Output(usize),
    EdgeIn(usize, usize),         // S i -> H0 j
    EdgeFwd(usize, usize, usize), // H(l) i -> H(l+1) j
    EdgeBwd(usize, usize, usize), // H(l+1) j -> H(l) i
    EdgeOut(usize, usize),        // H_last j -> O k
    EdgeRec(usize, usize, usize), // H(l) i -> H(l) j
}

impl ContextPick {
    fn is_neuron(&self) -> bool {
        matches!(
            self,
            ContextPick::Sensory(_) | ContextPick::Hidden(_, _) | ContextPick::Output(_)
        )
    }
}

#[cfg(feature = "ui")]
fn dist_point_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ax = a.x;
    let ay = a.y;
    let bx = b.x;
    let by = b.y;
    let px = p.x;
    let py = p.y;
    let vx = bx - ax;
    let vy = by - ay;
    let wx = px - ax;
    let wy = py - ay;
    let vv = vx * vx + vy * vy;
    let t = if vv > 0.0 {
        (wx * vx + wy * vy) / vv
    } else {
        0.0
    };
    let t = t.clamp(0.0, 1.0);
    let cx = ax + t * vx;
    let cy = ay + t * vy;
    let dx = px - cx;
    let dy = py - cy;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(feature = "ui")]
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.sim_tx.send(SimControl::Shutdown);
        if self
            .remote_workspace_binding
            .as_ref()
            .map(|binding| binding.save_on_exit)
            .unwrap_or(false)
        {
            let _ = self.push_remote_workspace_snapshot();
        }
        if let Some(tx) = &self.ga_control_tx {
            let _ = tx.send(GAControl::Stop);
        }
        crate::ga::ga_request_stop("ui_shutdown");
        for conn in &self.remote_connections {
            conn.stop.store(true, Ordering::SeqCst);
        }
        #[cfg(feature = "sysinfo")]
        {
            self.sys_stop.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = self.ga_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(feature = "ui")]
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        observe_time!("App::update");
        observe_hit!("ui_frame");
        let ui_frame_ms = (ctx.input(|i| i.unstable_dt).max(0.0) * 1000.0) as f32;
        crate::ga::ga_update_ui_frame_ms(ui_frame_ms);
        #[cfg(feature = "sysinfo")]
        {
            if let Ok(snap) = self.sys_snapshot.try_read() {
                self.cpu_usage = snap.cpu_usage;
                self.ram_usage_mb = snap.ram_usage_mb;
                self.cpu_temp_c = snap.cpu_temp_c;
                self.os_threads = snap.os_threads;
                self.runnable_threads = snap.runnable_threads;
                self.cpu_core_count = snap.cpu_core_count;
                self.hot_core_count = snap.hot_core_count;
                self.hot_core_top = snap.hot_core_top.clone();
            }
        }
        self.refresh_fpaa_status(false);
        self.reap_finished_ga_thread();
        self.queue_remote_token_refresh(false);

        let runner_arc = self.runner.clone();
        let spectral_arc = self.spectral_bands.clone();
        #[cfg(all(feature = "robot_io", unix))]
        let ipc_stats_arc = self.ipc_stats.clone();

        let bands_guard = spectral_arc.try_read().ok();
        #[cfg(all(feature = "robot_io", unix))]
        let ipc_stats_guard = ipc_stats_arc.try_read().ok();

        let fallback_model = match self.neuron_model {
            NeuronModelSel::Lif => NeuronModel::Lif,
            NeuronModelSel::Izh => {
                let preset = match self.izh_preset {
                    IzhPreset::RS => "RS",
                    IzhPreset::FS => "FS",
                    IzhPreset::IB => "IB",
                    IzhPreset::CH => "CH",
                    IzhPreset::LTS => "LTS",
                    IzhPreset::RZ => "RZ",
                    IzhPreset::TC => "TC",
                    IzhPreset::P => "P",
                };
                NeuronModel::Izh(IzhikevichParams::from_preset(preset, self.initial_lif.dt))
            }
            NeuronModelSel::Aarnn => NeuronModel::Aarnn,
        };
        let fallback_learning = match self.learning {
            LearningSel::Stdp => Learning::Stdp,
            LearningSel::Hebb => Learning::Hebb,
            LearningSel::Oja => Learning::Oja,
            LearningSel::Aarnn => Learning::Aarnn,
        };

        let (
            net_cloned,
            lif_cloned,
            stdp_cloned,
            model_cloned,
            learning_cloned,
            _t_ms_cloned,
            total_neurons_cloned,
            runner_ready,
        ): (
            NetworkConfig,
            LIFParams,
            STDPParams,
            NeuronModel,
            Learning,
            f64,
            usize,
            bool,
        ) = if let Ok(r) = runner_arc.try_read() {
            (
                r.net.clone(),
                r.lif.clone(),
                r.stdp.clone(),
                r.neuron_model,
                r.learning,
                r.t_ms,
                r.total_neurons(),
                true,
            )
        } else {
            // Fallback to local values if simulation is writing (busy)
            (
                self.local_net.clone(),
                self.initial_lif.clone(),
                self.initial_stdp.clone(),
                fallback_model,
                fallback_learning,
                0.0,
                0,
                false,
            )
        };

        // --- 1. Distributed State Sync ---
        {
            observe_time!("App::update/dist_sync");
            if let Some(ref node) = self.distributed_node {
                if let Ok(mut state) = node.state.try_write() {
                    let sync_standalone_registry =
                        matches!(self.view_source, ViewSource::Standalone);
                    if state.is_orchestrator && sync_standalone_registry {
                        if !state.network_registry.contains_key(&self.brain_id) {
                            if state.network_registry.is_empty() {
                                state.network_registry.insert(self.brain_id.clone(), {
                                    let mut status = crate::distributed::proto::NetworkStatus {
                                        network_id: self.brain_id.clone(),
                                        distribution: std::collections::HashMap::new(),
                                        current_dt: lif_cloned.dt,
                                        total_neurons: total_neurons_cloned as u64,
                                        num_layers: (net_cloned.num_hidden_layers + 1) as u32,
                                        desired_aarnn_depth: net_cloned.aarnn_layer_depth as u32,
                                        config_json: serde_json::to_string(&net_cloned)
                                            .unwrap_or_default(),
                                        neuron_model: model_cloned.to_str().to_string(),
                                        learning_rule: learning_cloned.to_str().to_string(),
                                        playing: self.playing,
                                        ..Default::default()
                                    };
                                    let deployment_payload = status.config_json.clone();
                                    crate::distributed::sync_network_status_deployment_from_payload(
                                        &mut status,
                                        &deployment_payload,
                                    );
                                    status
                                });
                            }
                        } else {
                            let nodes_empty = state.nodes.is_empty();
                            if let Some(net_status) = state.network_registry.get_mut(&self.brain_id)
                            {
                                if runner_ready {
                                    // Only update total_neurons if it's non-zero or if we are the only node.
                                    // This prevents flickering to 0 or local-only count in a distributed setup.
                                    if total_neurons_cloned > 0 || nodes_empty {
                                        net_status.total_neurons = net_status
                                            .total_neurons
                                            .max(total_neurons_cloned as u64);
                                    }
                                    net_status.current_dt = lif_cloned.dt;
                                    net_status.num_layers =
                                        (net_cloned.num_hidden_layers + 1) as u32;
                                    net_status.desired_aarnn_depth =
                                        net_cloned.aarnn_layer_depth as u32;
                                    net_status.neuron_model = model_cloned.to_str().to_string();
                                    net_status.learning_rule = learning_cloned.to_str().to_string();
                                    if self.last_synced_config.as_ref() != Some(&net_cloned) {
                                        self.last_config_json =
                                            serde_json::to_string(&net_cloned).unwrap_or_default();
                                        self.last_synced_config = Some(net_cloned.clone());
                                    }
                                    net_status.config_json = self.last_config_json.clone();
                                    let deployment_payload = net_status.config_json.clone();
                                    crate::distributed::sync_network_status_deployment_from_payload(
                                        net_status,
                                        &deployment_payload,
                                    );
                                    if nodes_empty {
                                        net_status.playing = self.playing;
                                    }
                                }
                            }
                        }
                    }

                    // Sync GA status to distributed state for heartbeat reporting
                    state.ga_running = self.ga_running;
                    if let Some(ga) = &self.ga_search {
                        state.ga_generation = ga.generation as u32;
                        state.ga_best_fitness = ga.best_fitness;
                        if let Some(best) = &ga.best_config {
                            if self.last_ga_best_config.as_ref() != Some(best) {
                                self.last_ga_best_config_json =
                                    serde_json::to_string(best).unwrap_or_default();
                                self.last_ga_best_config = Some(best.clone());
                            }
                            state.ga_best_config_json = self.last_ga_best_config_json.clone();
                        }
                    }
                }
            }
        }

        if let Some(ref node) = self.distributed_node {
            if let Ok(state) = node.state.try_read() {
                self.dist_is_orchestrator = state.is_orchestrator;
                self.dist_node_id = state.node_id.clone();
                self.dist_nodes = state.nodes.clone();
                self.dist_network_registry = state.network_registry.clone();
                self.dist_local_playing_cache
                    .retain(|network_id, _| state.networks.contains_key(network_id));
                for (network_id, net_arc) in &state.networks {
                    if let Ok(net) = net_arc.try_read() {
                        self.dist_local_playing_cache
                            .insert(network_id.clone(), net.playing);
                    }
                }
            }
        }
        self.maybe_select_initial_distributed_view();

        while let Ok(msg) = self.cluster_snapshot_rx.try_recv() {
            match msg {
                ClusterSnapshotMsg::Ok {
                    network_id,
                    node_id,
                    snap,
                } => {
                    self.cluster_snapshot_inflight = false;
                    self.cluster_snapshot_last_fetch = Some(std::time::Instant::now());
                    self.cluster_snapshot_network_id = Some(network_id);
                    self.cluster_snapshot_node_id = Some(node_id);
                    #[cfg(feature = "growth3d")]
                    {
                        self.cluster_topo_cache = snap.topo.clone();
                    }

                    // Re-calculate edges from the remote snapshot
                    let density = self.overlay_density;
                    let (edges, sizes, counts, output_count) =
                        Self::compute_edges_from_snapshot(density, &snap);
                    self.cached_edges = edges;
                    self.cached_layer_sizes = sizes;
                    self.cached_conn_counts = counts;
                    self.cached_output_conn_count = Some(output_count);
                    #[cfg(feature = "growth3d")]
                    {
                        self.cached_edge_topo = snap.topo.clone();
                    }
                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                    {
                        self.cached_skull_membrane = snap.skull_membrane;
                    }
                    self.last_conn_stats_refresh = std::time::Instant::now();
                    self.pending_edge_cache = false;

                    self.cluster_snapshot_cache = Some(snap);
                    self.status = "Remote snapshot updated (with connections)".to_string();

                    // Trigger layout recompute to ensure nodes are aligned
                    self.refresh_ui_buffers();
                }
                ClusterSnapshotMsg::Err {
                    network_id,
                    node_id,
                    error,
                } => {
                    self.cluster_snapshot_inflight = false;
                    nm_err!(
                        "[warn] Cluster snapshot failed (net={}, node={}): {}",
                        network_id,
                        node_id,
                        error
                    );
                }
            }
        }

        if let ViewSource::ClusterGlobal(net_id) = &self.view_source {
            let target_node = if let Some(filter) = self.view_node_filter.as_ref() {
                Some(filter.clone())
            } else if let Some(net_status) = self.dist_network_registry.get(net_id) {
                net_status.distribution.keys().next().cloned()
            } else {
                None
            };
            if let Some(node_id) = target_node {
                let addr_opt = self.dist_nodes.get(&node_id).map(|n| n.address.clone());
                if let Some(mut addr) = addr_opt {
                    if !addr.is_empty() {
                        if !addr.starts_with("http://") && !addr.starts_with("https://") {
                            addr = format!("http://{}", addr);
                        }
                        let now = std::time::Instant::now();
                        let stale = self.cluster_snapshot_last_fetch.map_or(true, |t| {
                            now.duration_since(t) > std::time::Duration::from_secs(2)
                        });
                        let needs_refresh = self.cluster_snapshot_network_id.as_deref()
                            != Some(net_id)
                            || self.cluster_snapshot_node_id.as_deref() != Some(&node_id);
                        if !self.cluster_snapshot_inflight && (stale || needs_refresh) {
                            self.cluster_snapshot_inflight = true;
                            self.cluster_snapshot_network_id = Some(net_id.clone());
                            self.cluster_snapshot_node_id = Some(node_id.clone());
                            let tx = self.cluster_snapshot_tx.clone();
                            let net_id_clone = net_id.clone();
                            let node_id_clone = node_id.clone();
                            let rt = self.runtime_handle.clone();
                            rt.spawn(async move {
                                match DistributedNeuromorphicClient::connect(addr.clone()).await {
                                    Ok(mut client) => {
                                        match client.get_network_snapshot(Request::new(NetworkSnapshotRequest {
                                            network_id: net_id_clone.clone(),
                                        })).await {
                                            Ok(resp) => {
                                                let snap_resp = resp.into_inner();
                                                match crate::runner::decode_snapshot_with_profile_backfill(&snap_resp.snapshot_json) {
                                                    Ok(st) => {
                                                        let _ = tx.send(ClusterSnapshotMsg::Ok {
                                                            network_id: net_id_clone,
                                                            node_id: node_id_clone,
                                                            snap: Box::new(st),
                                                        });
                                                    }
                                                    Err(e) => {
                                                        let _ = tx.send(ClusterSnapshotMsg::Err {
                                                            network_id: net_id_clone,
                                                            node_id: node_id_clone,
                                                            error: format!("snapshot parse failed: {}", e),
                                                        });
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let _ = tx.send(ClusterSnapshotMsg::Err {
                                                    network_id: net_id_clone,
                                                    node_id: node_id_clone,
                                                    error: format!("snapshot request failed: {}", e),
                                                });
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx.send(ClusterSnapshotMsg::Err {
                                            network_id: net_id_clone,
                                            node_id: node_id_clone,
                                            error: format!("snapshot connect failed: {}", e),
                                        });
                                    }
                                }
                            });
                        }
                    }
                }
            } else if let Some(node) = self.distributed_node.clone() {
                let (local_node_id, net_arc) = {
                    if let Ok(state) = node.state.try_read() {
                        (
                            Some(state.node_id.clone()),
                            state.networks.get(net_id).cloned(),
                        )
                    } else {
                        (None, None)
                    }
                };
                if let (Some(local_node_id), Some(net_arc)) = (local_node_id, net_arc) {
                    let now = std::time::Instant::now();
                    let stale = self.cluster_snapshot_last_fetch.map_or(true, |t| {
                        now.duration_since(t) > std::time::Duration::from_secs(2)
                    });
                    let needs_refresh = self.cluster_snapshot_network_id.as_deref() != Some(net_id)
                        || self.cluster_snapshot_node_id.as_deref() != Some(local_node_id.as_str());
                    if !self.cluster_snapshot_inflight && (stale || needs_refresh) {
                        if let Ok(net) = net_arc.try_read() {
                            let snap = net.runner.snapshot();
                            self.cluster_snapshot_inflight = false;
                            self.cluster_snapshot_last_fetch = Some(now);
                            self.cluster_snapshot_network_id = Some(net_id.clone());
                            self.cluster_snapshot_node_id = Some(local_node_id.clone());
                            #[cfg(feature = "growth3d")]
                            {
                                self.cluster_topo_cache = snap.topo.clone();
                            }
                            let density = self.overlay_density;
                            let (edges, sizes, counts, output_count) =
                                Self::compute_edges_from_snapshot(density, &snap);
                            self.cached_edges = edges;
                            self.cached_layer_sizes = sizes;
                            self.cached_conn_counts = counts;
                            self.cached_output_conn_count = Some(output_count);
                            #[cfg(feature = "growth3d")]
                            {
                                self.cached_edge_topo = snap.topo.clone();
                            }
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            {
                                self.cached_skull_membrane = snap.skull_membrane;
                            }
                            self.last_conn_stats_refresh = std::time::Instant::now();
                            self.pending_edge_cache = false;
                            if self.cluster_snapshot_cache.is_none() || needs_refresh {
                                self.status = "Cluster snapshot updated (local)".to_string();
                                self.refresh_ui_buffers();
                            }
                            self.cluster_snapshot_cache = Some(Box::new(snap));
                        }
                    }
                }
            }
        }

        while let Ok(msg) = self.remote_status_rx.try_recv() {
            match msg {
                RemoteStatusMsg::Update {
                    addr,
                    nodes,
                    networks,
                } => {
                    self.remote_statuses.insert(
                        addr,
                        RemoteStatusSnapshot {
                            nodes,
                            networks,
                            last_error: None,
                            last_update: std::time::Instant::now(),
                        },
                    );
                }
                RemoteStatusMsg::Error { addr, error } => {
                    let entry = self
                        .remote_statuses
                        .entry(addr)
                        .or_insert(RemoteStatusSnapshot {
                            nodes: HashMap::new(),
                            networks: HashMap::new(),
                            last_error: None,
                            last_update: std::time::Instant::now(),
                        });
                    entry.last_error = Some(error);
                    entry.last_update = std::time::Instant::now();
                }
            }
        }

        while let Ok(msg) = self.tool_task_rx.try_recv() {
            match msg {
                ToolTaskResult::TfliteImport {
                    path,
                    json,
                    stdout,
                    stderr,
                    error,
                } => {
                    if let Some(err) = error {
                        let details = if stderr.trim().is_empty() {
                            stdout.trim()
                        } else {
                            stderr.trim()
                        };
                        if details.is_empty() {
                            self.status = format!("TFLite import failed: {}", err);
                            self.last_import_report =
                                Some(format!("TFLite import failed: {}", err));
                        } else {
                            self.status = format!("TFLite import failed: {} ({})", err, details);
                            self.last_import_report =
                                Some(format!("TFLite import failed: {} ({})", err, details));
                        }
                        nm_log!("[import] TFLite failed: {} {}", err, details);
                        continue;
                    }
                    let Some(json) = json else {
                        self.status = "TFLite import failed: missing output JSON".to_string();
                        self.last_import_report =
                            Some("TFLite import failed: missing output JSON".to_string());
                        nm_log!("[import] TFLite failed: missing output JSON");
                        continue;
                    };
                    let mut parsed = match serde_json::from_str::<serde_json::Value>(&json) {
                        Ok(v) => v,
                        Err(e) => {
                            let details = if stderr.trim().is_empty() {
                                stdout.trim()
                            } else {
                                stderr.trim()
                            };
                            if details.is_empty() {
                                self.status = format!("TFLite import failed: invalid JSON ({})", e);
                                self.last_import_report =
                                    Some(format!("TFLite import failed: invalid JSON ({})", e));
                            } else {
                                self.status = format!(
                                    "TFLite import failed: invalid JSON ({}) ({})",
                                    e, details
                                );
                                self.last_import_report = Some(format!(
                                    "TFLite import failed: invalid JSON ({}) ({})",
                                    e, details
                                ));
                            }
                            nm_log!("[import] TFLite failed: invalid JSON ({}) {}", e, details);
                            continue;
                        }
                    };
                    let mut missing = Vec::new();
                    if parsed.get("net").is_none() {
                        missing.push("net");
                    }
                    if parsed.get("w_in").is_none() {
                        missing.push("w_in");
                    }
                    if parsed.get("w_out").is_none() {
                        missing.push("w_out");
                    }
                    if !missing.is_empty() {
                        let details = if stderr.trim().is_empty() {
                            stdout.trim()
                        } else {
                            stderr.trim()
                        };
                        if details.is_empty() {
                            self.status = format!(
                                "TFLite import failed: missing fields {}",
                                missing.join(", ")
                            );
                            self.last_import_report = Some(format!(
                                "TFLite import failed: missing fields {}",
                                missing.join(", ")
                            ));
                        } else {
                            self.status = format!(
                                "TFLite import failed: missing fields {} ({})",
                                missing.join(", "),
                                details
                            );
                            self.last_import_report = Some(format!(
                                "TFLite import failed: missing fields {} ({})",
                                missing.join(", "),
                                details
                            ));
                        }
                        nm_log!(
                            "[import] TFLite failed: missing fields {}",
                            missing.join(", ")
                        );
                        continue;
                    }
                    let allow_large = std::env::var("NMD_TFLITE_ALLOW_LARGE")
                        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
                        .unwrap_or(false);
                    let max_layers = std::env::var("NMD_TFLITE_MAX_LAYERS")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(16);
                    let max_params = std::env::var("NMD_TFLITE_MAX_PARAMS")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(2_000_000);
                    let mut total_params = 0usize;
                    let mut hidden_layers = 0usize;
                    let mut oversize = None;
                    let matrix_info = |val: &serde_json::Value| -> Option<(usize, usize, usize)> {
                        let rows = val.get("rows")?.as_u64()? as usize;
                        let cols = val.get("cols")?.as_u64()? as usize;
                        let data_len = val
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        Some((rows, cols, data_len))
                    };
                    if let Some(w_in) = parsed.get("w_in").and_then(matrix_info) {
                        total_params = total_params.saturating_add(w_in.0.saturating_mul(w_in.1));
                    }
                    if let Some(w_out) = parsed.get("w_out").and_then(matrix_info) {
                        total_params = total_params.saturating_add(w_out.0.saturating_mul(w_out.1));
                    }
                    if let Some(arr) = parsed.get("w_hh_fwd").and_then(|v| v.as_array()) {
                        hidden_layers = arr.len() + 1;
                        for mat in arr {
                            if let Some((rows, cols, _)) = matrix_info(mat) {
                                total_params =
                                    total_params.saturating_add(rows.saturating_mul(cols));
                                if rows.saturating_mul(cols) > max_params {
                                    oversize = Some((rows, cols));
                                    break;
                                }
                            }
                        }
                    }
                    if !allow_large
                        && (hidden_layers > max_layers
                            || total_params > max_params
                            || oversize.is_some())
                    {
                        let msg = if let Some((r, c)) = oversize {
                            format!(
                                "TFLite import rejected: layer {}x{} too large (set NMD_TFLITE_ALLOW_LARGE=1 to override)",
                                r, c
                            )
                        } else if hidden_layers > max_layers {
                            format!(
                                "TFLite import rejected: {} hidden layers > {} (set NMD_TFLITE_ALLOW_LARGE=1 to override)",
                                hidden_layers, max_layers
                            )
                        } else {
                            format!(
                                "TFLite import rejected: {} params > {} (set NMD_TFLITE_ALLOW_LARGE=1 to override)",
                                total_params, max_params
                            )
                        };
                        self.status = msg.clone();
                        self.last_import_report = Some(msg.clone());
                        nm_log!("[import] {}", msg);
                        continue;
                    }
                    if let Some(obj) = parsed.as_object_mut() {
                        if !obj.contains_key("w_hh_fwd") {
                            obj.insert(
                                "w_hh_fwd".to_string(),
                                serde_json::Value::Array(Vec::new()),
                            );
                        }
                        if !obj.contains_key("w_hh_bwd") {
                            obj.insert(
                                "w_hh_bwd".to_string(),
                                serde_json::Value::Array(Vec::new()),
                            );
                        }
                        if !obj.contains_key("w_hh_rec") {
                            obj.insert(
                                "w_hh_rec".to_string(),
                                serde_json::Value::Array(Vec::new()),
                            );
                        }
                    }
                    let json = match serde_json::to_string(&parsed) {
                        Ok(s) => s,
                        Err(e) => {
                            self.status = format!("TFLite import failed: normalize JSON ({})", e);
                            self.last_import_report =
                                Some(format!("TFLite import failed: normalize JSON ({})", e));
                            nm_log!("[import] TFLite failed: normalize JSON ({})", e);
                            continue;
                        }
                    };
                    let path_for_pending = path.clone();
                    let json_for_sim = json.clone();
                    let view_source = self.view_source.clone();
                    match view_source {
                        ViewSource::Standalone => {
                            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                            match self
                                .sim_tx
                                .send(SimControl::ImportNetworkWithReply(json_for_sim, reply_tx))
                            {
                                Ok(()) => {
                                    self.force_show_connections = false;
                                    self.pending_edge_cache = false;
                                    self.edge_cache_inflight = false;
                                    self.cached_edges.clear();
                                    self.cached_layer_sizes.clear();
                                    self.cached_conn_counts.clear();
                                    self.cached_output_conn_count = None;
                                    #[cfg(feature = "growth3d")]
                                    {
                                        self.cached_edge_topo = None;
                                    }
                                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                    {
                                        self.cached_skull_membrane = None;
                                    }
                                    self.pending_import = Some(PendingImport {
                                        path: path_for_pending,
                                        kind: ImportKind::Tflite,
                                        stdout,
                                        stderr,
                                        rx: reply_rx,
                                        result: None,
                                    });
                                    self.status = "Applying TFLite import...".to_string();
                                    self.last_import_report =
                                        Some("Applying TFLite import...".to_string());
                                    nm_log!("[import] TFLite applying import");
                                }
                                Err(_) => {
                                    self.status = "TFLite import failed: simulation channel closed"
                                        .to_string();
                                    self.last_import_report = Some(
                                        "TFLite import failed: simulation channel closed"
                                            .to_string(),
                                    );
                                    nm_log!("[import] TFLite failed: simulation channel closed");
                                }
                            }
                        }
                        ViewSource::LocalManaged(id) => {
                            match self.import_network_json_to_local_managed(
                                &id,
                                &json,
                                ImportKind::Tflite,
                                &path,
                            ) {
                                Ok(()) => {}
                                Err(e) => {
                                    self.status = format!("TFLite import failed: {}", e);
                                    self.last_import_report =
                                        Some(format!("TFLite import failed: {}", e));
                                }
                            }
                        }
                        ViewSource::ClusterGlobal(_) => {
                            self.status =
                                "TFLite import not supported for cluster view".to_string();
                            self.last_import_report =
                                Some("TFLite import not supported for cluster view".to_string());
                        }
                    }
                }
                ToolTaskResult::TflitePickCanceled => {
                    self.status = "TFLite import canceled".to_string();
                    self.last_import_report = Some("TFLite import canceled".to_string());
                    nm_log!("[import] TFLite canceled");
                }
                ToolTaskResult::PythonResolved { result } => match result {
                    Ok(p) => {
                        self.python_path = Some(p.clone());
                        self.status = format!("Using Python: {}", p);
                    }
                    Err(e) => {
                        self.status = format!("Python not found: {}", e);
                    }
                },
                ToolTaskResult::FileWrite { kind, path, error } => {
                    if let Some(e) = error {
                        self.status = format!("Write failed: {}", e);
                    } else {
                        let label = match kind {
                            FileTaskKind::SaveConfig => "config",
                            FileTaskKind::SaveNetwork => "network snapshot",
                            FileTaskKind::SaveProbes => "probes",
                            _ => "file",
                        };
                        self.status = format!("Saved {} to {}", label, path.display());
                    }
                }
                ToolTaskResult::FileRead {
                    kind,
                    path,
                    data,
                    error,
                } => {
                    if let Some(e) = error {
                        self.status = format!("Read failed: {}", e);
                        continue;
                    }
                    let Some(data) = data else {
                        self.status = "Read failed: empty file".to_string();
                        continue;
                    };
                    match kind {
                        FileTaskKind::LoadConfig => {
                            match serde_json::from_str::<NetworkConfig>(&data) {
                                Ok(mut net) => {
                                    if net.clumping_design != ClumpingDesign::None
                                        && net.num_hidden_layers <= 1
                                    {
                                        apply_clumping_layer_defaults(&mut net);
                                    }
                                    let view_source = self.view_source.clone();
                                    match view_source {
                                        ViewSource::Standalone => {
                                            self.local_net = net.clone();
                                            let _ = self.sim_tx.send(SimControl::RecreateRunner(
                                                lif_cloned.clone(),
                                                stdp_cloned.clone(),
                                                net,
                                                model_cloned,
                                                learning_cloned,
                                            ));
                                            self.refresh_ui_buffers();
                                            self.status = format!(
                                                "Loaded and applied config from {}",
                                                path.display()
                                            );
                                        }
                                        ViewSource::LocalManaged(id) => {
                                            match self.apply_config_to_local_managed(&id, net) {
                                                Ok(()) => {
                                                    self.refresh_ui_buffers();
                                                    self.status = format!(
                                                        "Loaded config for local managed network from {}",
                                                        path.display()
                                                    );
                                                }
                                                Err(e) => {
                                                    self.status = format!("Load failed: {}", e);
                                                }
                                            }
                                        }
                                        ViewSource::ClusterGlobal(_) => {
                                            self.status =
                                                "Load Config not supported for cluster view"
                                                    .to_string();
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.status = format!("Load failed: {}", e);
                                }
                            }
                        }
                        FileTaskKind::LoadNetwork => {
                            let json = data;
                            let path_for_pending = path.clone();
                            let json_for_sim = json.clone();
                            let view_source = self.view_source.clone();
                            match view_source {
                                ViewSource::Standalone => {
                                    let res = self.queue_import(
                                        ImportKind::Standard,
                                        path_for_pending,
                                        json_for_sim,
                                        String::new(),
                                        String::new(),
                                    );
                                    if let Err(e) = res {
                                        self.status = format!("Load failed: {}", e);
                                    }
                                }
                                ViewSource::LocalManaged(id) => {
                                    match self.import_network_json_to_local_managed(
                                        &id,
                                        &json,
                                        ImportKind::Standard,
                                        &path,
                                    ) {
                                        Ok(()) => {}
                                        Err(e) => {
                                            self.status = format!("Load failed: {}", e);
                                        }
                                    }
                                }
                                ViewSource::ClusterGlobal(_) => {
                                    self.status =
                                        "Load Network not supported for cluster view".to_string();
                                }
                            }
                        }
                        FileTaskKind::LoadProbes => match self.import_probes_json(&data) {
                            Ok(()) => {
                                self.status = format!("Loaded probes from {}", path.display());
                            }
                            Err(e) => {
                                self.status = format!("Load failed: {}", e);
                            }
                        },
                        _ => {}
                    }
                }
                ToolTaskResult::ToolExport {
                    kind,
                    path,
                    stdout,
                    stderr,
                    error,
                } => {
                    let kind_str = match kind {
                        ToolExportKind::Onnx => "ONNX",
                        ToolExportKind::PyNN => "PyNN",
                        ToolExportKind::Nir => "NIR",
                        ToolExportKind::NeuroML => "NeuroML",
                        ToolExportKind::Tflite => "TFLite",
                    };
                    if let Some(e) = error {
                        let details = if stderr.trim().is_empty() {
                            stdout.trim()
                        } else {
                            stderr.trim()
                        };
                        if details.is_empty() {
                            self.status = format!("{} export failed: {}", kind_str, e);
                        } else {
                            self.status =
                                format!("{} export failed: {} ({})", kind_str, e, details);
                        }
                    } else {
                        self.status = format!("Exported {} to {}", kind_str, path.display());
                    }
                }
                ToolTaskResult::ToolImport {
                    kind,
                    path,
                    json,
                    stdout,
                    stderr,
                    error,
                } => {
                    let kind_str = match kind {
                        ImportKind::Tflite => "TFLite",
                        ImportKind::Onnx => "ONNX",
                        ImportKind::NeuroML => "NeuroML",
                        ImportKind::PyNN => "PyNN",
                        ImportKind::Nir => "NIR",
                        ImportKind::Standard => "Network",
                    };
                    if let Some(e) = error {
                        let details = if stderr.trim().is_empty() {
                            stdout.trim()
                        } else {
                            stderr.trim()
                        };
                        if details.is_empty() {
                            self.status = format!("{} import failed: {}", kind_str, e);
                        } else {
                            self.status =
                                format!("{} import failed: {} ({})", kind_str, e, details);
                        }
                        continue;
                    }
                    let Some(json) = json else {
                        self.status = format!("{} import failed: missing output JSON", kind_str);
                        continue;
                    };
                    if let Err(e) = self.queue_import(kind, path, json, stdout, stderr) {
                        self.status = format!("{} import failed: {}", kind_str, e);
                    }
                }
                ToolTaskResult::RemoteTokenBalance { result } => {
                    self.remote_token_refresh_inflight = false;
                    self.remote_token_last_refresh = Some(Instant::now());
                    match result {
                        Ok(balance) => {
                            self.remote_token_balance = Some(balance);
                            self.remote_token_error = None;
                        }
                        Err(error) => {
                            self.remote_token_error = Some(error);
                        }
                    }
                }
            }
        }

        if let Some(mut pending) = self.pending_import.take() {
            let mut keep_pending = false;

            if pending.result.is_none() {
                match pending.rx.try_recv() {
                    Ok(res) => {
                        pending.result = Some(res);
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.status = "Import failed: channel closed".to_string();
                        self.last_import_report = Some("Import failed: channel closed".to_string());
                        nm_log!("[import] failed: channel closed");
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        keep_pending = true;
                    }
                }
            }

            if !keep_pending {
                match pending.result.as_ref() {
                    Some(Ok(())) => {
                        let (net, model, learning, sizes, counts) =
                            if let Ok(r) = self.runner.try_read() {
                                let sizes = Self::cache_sizes_counts(&r);
                                let allow_static_edges = self.show_static_overlays
                                    && Self::should_build_static_edges(&sizes.0);
                                (
                                    r.net.clone(),
                                    r.neuron_model,
                                    r.learning,
                                    sizes,
                                    if allow_static_edges {
                                        Some(Self::compute_cached_edges(self.overlay_density, &r))
                                    } else {
                                        None
                                    },
                                )
                            } else {
                                keep_pending = true;
                                (
                                    self.local_net.clone(),
                                    NeuronModel::Lif,
                                    Learning::Stdp,
                                    (Vec::new(), Vec::new(), 0),
                                    None,
                                )
                            };

                        if !keep_pending {
                            self.local_net = net;

                            if pending.kind == ImportKind::Tflite {
                                self.neuron_model = NeuronModelSel::Lif;
                                self.learning = LearningSel::Stdp;
                                let _ = self.sim_tx.send(SimControl::SetModel(NeuronModel::Lif));
                                let _ = self.sim_tx.send(SimControl::SetLearning(Learning::Stdp));
                                if self.tflite_freeze_learning {
                                    let _ = self.sim_tx.send(SimControl::SetStdpEta(0.0));
                                }
                                self.sim_throttle_ms
                                    .store(self.tflite_sim_throttle_ms, Ordering::Relaxed);
                                self.set_network_layout(NetworkLayout::Conventional, true);
                            } else {
                                // Standard or ONNX: synchronize UI model selectors with the loaded model
                                match model {
                                    NeuronModel::Lif => self.neuron_model = NeuronModelSel::Lif,
                                    NeuronModel::Izh(_) => self.neuron_model = NeuronModelSel::Izh,
                                    NeuronModel::Aarnn => {
                                        self.neuron_model = NeuronModelSel::Aarnn;
                                        self.set_network_layout(NetworkLayout::Aarnn, true);
                                    }
                                }
                                match learning {
                                    Learning::Stdp => self.learning = LearningSel::Stdp,
                                    Learning::Hebb => self.learning = LearningSel::Hebb,
                                    Learning::Oja => self.learning = LearningSel::Oja,
                                    Learning::Aarnn => self.learning = LearningSel::Aarnn,
                                }
                            }

                            self.refresh_ui_buffers();
                            if matches!(self.input_source, InputSource::Random) {
                                let n = self.local_net.num_sensory_neurons;
                                let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(
                                    RandomProvider::new(n, self.random_spike_probability),
                                )));
                            } else if matches!(self.input_source, InputSource::Theta) {
                                let n = self.local_net.num_sensory_neurons;
                                let dt_ms = self.initial_lif.dt.max(0.001) as f32;
                                let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(
                                    ThetaProvider::new(
                                        n,
                                        self.local_net.theta_rhythm_hz,
                                        self.local_net.theta_rhythm_duty,
                                        self.local_net.theta_rhythm_phase_jitter,
                                        dt_ms,
                                    ),
                                )));
                            } else if matches!(self.input_source, InputSource::ExternalHttpAer) {
                                self.connect_http_aer_source(self.local_net.num_sensory_neurons);
                            }

                            self.cached_layer_sizes = sizes.0;
                            self.cached_conn_counts = sizes.1;
                            self.cached_output_conn_count = Some(sizes.2);
                            let hidden_summary = Self::hidden_summary(&self.cached_layer_sizes);
                            let skipped_static_edges = self.show_static_overlays
                                && !self.force_show_connections
                                && !Self::should_build_static_edges(&self.cached_layer_sizes);
                            if skipped_static_edges {
                                self.show_static_overlays = false;
                            }
                            if let Some(edges) = counts {
                                self.cached_edges = edges;
                                #[cfg(feature = "growth3d")]
                                {
                                    self.cached_edge_topo = None;
                                }
                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                {
                                    self.cached_skull_membrane = None;
                                }
                            }

                            let kind_str = match pending.kind {
                                ImportKind::Tflite => "TFLite",
                                ImportKind::Onnx => "ONNX",
                                ImportKind::NeuroML => "NeuroML",
                                ImportKind::PyNN => "PyNN",
                                ImportKind::Nir => "NIR",
                                ImportKind::Standard => "Network",
                            };
                            let summary = format!(
                                "Imported {} from {} (S={} {} O={})",
                                kind_str,
                                pending.path.display(),
                                self.local_net.num_sensory_neurons,
                                hidden_summary,
                                self.local_net.num_output_neurons
                            );
                            let summary = if skipped_static_edges {
                                format!(
                                    "{} (static connection overlays auto-disabled for large model)",
                                    summary
                                )
                            } else {
                                summary
                            };
                            self.status = summary.clone();
                            self.last_import_report = Some(summary.clone());
                            nm_log!("[import] {}", summary);
                            let has_zero_io = self.local_net.num_sensory_neurons == 0
                                || self.local_net.num_output_neurons == 0;
                            if has_zero_io {
                                let warn = format!(
                                    "{} import warning: zero-sized input/output layers",
                                    kind_str
                                );
                                self.last_import_report = Some(format!("{} ({})", summary, warn));
                                nm_log!("[import] {}", warn);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        let kind_str = match pending.kind {
                            ImportKind::Tflite => "TFLite",
                            ImportKind::Onnx => "ONNX",
                            ImportKind::NeuroML => "NeuroML",
                            ImportKind::PyNN => "PyNN",
                            ImportKind::Nir => "NIR",
                            ImportKind::Standard => "Network",
                        };
                        let details = if pending.stderr.trim().is_empty() {
                            pending.stdout.trim()
                        } else {
                            pending.stderr.trim()
                        };
                        if details.is_empty() {
                            self.status = format!("{} import failed: {}", kind_str, e);
                            self.last_import_report =
                                Some(format!("{} import failed: {}", kind_str, e));
                        } else {
                            self.status =
                                format!("{} import failed: {} ({})", kind_str, e, details);
                            self.last_import_report =
                                Some(format!("{} import failed: {} ({})", kind_str, e, details));
                        }
                        nm_log!("[import] {} failed: {} {}", kind_str, e, details);
                    }
                    None => {
                        keep_pending = true;
                    }
                }
            }
            if keep_pending {
                self.pending_import = Some(pending);
            }
        }
        if self.pending_edge_cache && !self.edge_cache_inflight {
            match self.view_source {
                ViewSource::Standalone => {
                    self.edge_cache_inflight = true;
                    self.pending_edge_cache = false;
                    let density = self.overlay_density;
                    let tx = self.edge_cache_res_tx.clone();
                    if let Ok(r) = self.runner.try_read() {
                        let edges = Self::compute_cached_edges(density, &r);
                        let (sizes, counts, output_count) = Self::cache_sizes_counts(&r);
                        #[cfg(feature = "growth3d")]
                        let topo = r.topo.clone();
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        let skull_membrane = r.morph.skull_membrane;
                        std::thread::spawn(move || {
                            let _ = tx.send(EdgeCacheResult {
                                edges,
                                sizes,
                                counts,
                                output_count,
                                #[cfg(feature = "growth3d")]
                                topo: Some(topo),
                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                skull_membrane,
                            });
                        });
                    } else {
                        let runner = self.runner.clone();
                        std::thread::spawn(move || {
                            let (edges, sizes, counts, output_count, topo_opt) = {
                                let r = runner.blocking_read();
                                let edges = Self::compute_cached_edges(density, &r);
                                let (sizes, counts, output_count) = Self::cache_sizes_counts(&r);
                                #[cfg(feature = "growth3d")]
                                let topo_opt = Some(r.topo.clone());
                                #[cfg(not(feature = "growth3d"))]
                                let topo_opt = ();
                                (edges, sizes, counts, output_count, topo_opt)
                            };
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            let skull_membrane = {
                                let r = runner.blocking_read();
                                r.morph.skull_membrane
                            };
                            let _ = tx.send(EdgeCacheResult {
                                edges,
                                sizes,
                                counts,
                                output_count,
                                #[cfg(feature = "growth3d")]
                                topo: topo_opt,
                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                skull_membrane,
                            });
                        });
                    }
                }
                ViewSource::LocalManaged(ref network_id) => {
                    let managed_net_arc = self
                        .distributed_node
                        .as_ref()
                        .and_then(|node| node.state.try_read().ok())
                        .and_then(|state| state.networks.get(network_id).cloned());
                    if let Some(net_arc) = managed_net_arc {
                        self.edge_cache_inflight = true;
                        self.pending_edge_cache = false;
                        let density = self.overlay_density;
                        let tx = self.edge_cache_res_tx.clone();
                        std::thread::spawn(move || {
                            let (edges, sizes, counts, output_count, topo_opt) = {
                                let net = net_arc.blocking_read();
                                let r = &net.runner;
                                let edges = Self::compute_cached_edges(density, r);
                                let (sizes, counts, output_count) = Self::cache_sizes_counts(r);
                                #[cfg(feature = "growth3d")]
                                let topo_opt = Some(r.topo.clone());
                                #[cfg(not(feature = "growth3d"))]
                                let topo_opt = ();
                                (edges, sizes, counts, output_count, topo_opt)
                            };
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            let skull_membrane = {
                                let net = net_arc.blocking_read();
                                net.runner.morph.skull_membrane
                            };
                            let _ = tx.send(EdgeCacheResult {
                                edges,
                                sizes,
                                counts,
                                output_count,
                                #[cfg(feature = "growth3d")]
                                topo: topo_opt,
                                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                                skull_membrane,
                            });
                        });
                    } else {
                        // Keep last good cache; retry next refresh period.
                        self.pending_edge_cache = false;
                        self.edge_cache_inflight = false;
                        self.last_edge_cache_refresh = std::time::Instant::now();
                    }
                }
                ViewSource::ClusterGlobal(_) => {
                    if let Some(snap) = &self.cluster_snapshot_cache {
                        let density = self.overlay_density;
                        let (edges, sizes, counts, output_count) =
                            Self::compute_edges_from_snapshot(density, snap);
                        self.cached_edges = edges;
                        self.cached_layer_sizes = sizes;
                        self.cached_conn_counts = counts;
                        self.cached_output_conn_count = Some(output_count);
                        #[cfg(feature = "growth3d")]
                        {
                            self.cached_edge_topo = snap.topo.clone();
                        }
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            self.cached_skull_membrane = snap.skull_membrane;
                        }
                        self.last_conn_stats_refresh = std::time::Instant::now();
                        self.pending_edge_cache = false;
                        self.last_edge_cache_refresh = std::time::Instant::now();
                    }
                }
            }
        }

        while let Ok(msg) = self.edge_cache_rx.try_recv() {
            let incoming_edges_empty = msg.edges.is_empty();
            let incoming_has_connections =
                msg.output_count > 0 || msg.counts.iter().any(|&v| v > 0);
            let preserve_cached_edges =
                incoming_edges_empty && incoming_has_connections && !self.cached_edges.is_empty();
            self.cached_layer_sizes = msg.sizes;
            self.cached_conn_counts = msg.counts;
            self.cached_output_conn_count = Some(msg.output_count);
            self.last_conn_stats_refresh = std::time::Instant::now();
            if !preserve_cached_edges {
                self.cached_edges = msg.edges;
            }
            #[cfg(feature = "growth3d")]
            {
                if let Some(topo) = msg.topo {
                    self.cached_edge_topo = Some(topo);
                } else if !preserve_cached_edges {
                    self.cached_edge_topo = None;
                }
            }
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            {
                self.cached_skull_membrane = msg.skull_membrane;
            }
            self.pending_edge_cache = false;
            self.edge_cache_inflight = false;
            self.last_edge_cache_refresh = std::time::Instant::now();
        }

        if self.cached_edges.is_empty() {
            if matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
                if let Some(snap) = &self.cluster_snapshot_cache {
                    let density = self.overlay_density;
                    let (edges, sizes, counts, output_count) =
                        Self::compute_edges_from_snapshot(density, snap);
                    self.cached_edges = edges;
                    self.cached_layer_sizes = sizes;
                    self.cached_conn_counts = counts;
                    self.cached_output_conn_count = Some(output_count);
                    #[cfg(feature = "growth3d")]
                    {
                        self.cached_edge_topo = snap.topo.clone();
                    }
                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                    {
                        self.cached_skull_membrane = snap.skull_membrane;
                    }
                    self.last_conn_stats_refresh = std::time::Instant::now();
                }
            } else if (self.show_static_overlays || self.force_show_connections)
                && self.overlay_density > 0
                && !self.pending_edge_cache
                && !self.edge_cache_inflight
            {
                self.pending_edge_cache = true;
                self.last_edge_cache_refresh = std::time::Instant::now();
            }
        }

        let is_orchestrator = self.dist_is_orchestrator;
        let node_id = self.dist_node_id.clone();
        let connected_nodes = self.dist_nodes.clone();
        let network_registry = self.dist_network_registry.clone();

        if self.layout_auto {
            let desired = self.preferred_layout_for_view(&model_cloned, &network_registry);
            if desired != self.network_layout {
                self.set_network_layout(desired, true);
            }
        }

        let mut ga_finished = false;
        let mut last_ga: Option<GASearch> = None;
        if let Some(rx) = &self.ga_rx {
            loop {
                match rx.try_recv() {
                    Ok(ga) => {
                        if ga.generation >= self.ga_generations {
                            ga_finished = true;
                        }
                        last_ga = Some(ga);
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if self.ga_running {
                            let reason = crate::ga::ga_abort_reason()
                                .unwrap_or_else(|| "worker_exit".to_string());
                            self.status = format!("GA Search stopped ({})", reason);
                        }
                        self.ga_running = false;
                        self.ga_rx = None;
                        self.ga_control_tx = None;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                }
            }
        }

        if let Some(ga) = last_ga {
            self.ga_search = Some(ga);
            if let Some(ga_ref) = &self.ga_search {
                self.ga_best_fitness = ga_ref.best_fitness;

                if self.ga_live_preview && self.ga_running {
                    // Update main network parameters with the "current" one from GA
                    // Pick the one that was just evaluated
                    if let Some(ind) = ga_ref
                        .population
                        .get(ga_ref.current_eval_idx.saturating_sub(1))
                    {
                        let _ = self
                            .sim_tx
                            .send(SimControl::ApplyConfig(ind.config.clone()));
                    }
                }
            }
        }

        if ga_finished {
            self.ga_running = false;
            self.ga_rx = None;
            self.ga_control_tx = None;
            self.status = "GA Search completed".into();
            if let Some(ga) = &self.ga_search {
                // Moving save_leaderboard to a separate thread to avoid blocking the UI
                let ga_clone = ga.clone();
                std::thread::spawn(move || {
                    let _ = ga_clone.save_leaderboard("leaderboard.json");
                });
            }
        }
        self.reap_finished_ga_thread();

        if crate::ga::ga_take_ui_cleanup_request() {
            self.log_ui_memory_snapshot("ga_abort_cleanup_request");
            self.clear_ui_caches();
            self.log_ui_memory_snapshot("ga_abort_cleanup_done");
            self.ga_abort_cleanup_done = true;
        }

        if self.ga_running {
            self.ga_abort_cleanup_done = false;
        } else if let Some(reason) = crate::ga::ga_abort_reason() {
            if !self.ga_abort_cleanup_done {
                self.log_ui_memory_snapshot(&reason);
                self.clear_ui_caches();
                self.ga_abort_cleanup_done = true;
            }
        }

        #[cfg(all(feature = "robot_io", unix))]
        if let Some(stats) = ipc_stats_guard.as_deref() {
            self.ipc_connected = stats.connected;
            self.ipc_last_peer = stats.last_peer.clone();
            self.ipc_last_receive_time = stats.last_receive_time;
            self.ipc_frame_count = stats.frame_count;
            self.ipc_packet_drop_count = stats.drop_count;
            self.ipc_size_mismatch_count = stats.size_mismatch_count;
            self.ipc_last_steps = stats.last_steps;
            if let Some(hs) = stats.last_handshake.clone() {
                let unchanged = self
                    .ipc_last_handshake
                    .as_ref()
                    .map(|old| old == &hs)
                    .unwrap_or(false);
                if !unchanged {
                    self.apply_ipc_config(hs);
                }
            }
        }

        // --- 1. Simulation Monitoring & Activity Pull ---
        if matches!(self.view_source, ViewSource::Standalone) {
            if let Ok(r) = runner_arc.try_read() {
                let (lt, tot) = r.calculate_longterm_connections();
                self.sync_activity_from_standalone(&r);
                self.update_probes_from_standalone(&r);
                self.longterm_conn = lt;
                self.total_conn = tot;
                self.last_longterm_update = std::time::Instant::now();
            } else {
                // If locked by simulation, just decay current UI activity to keep it smooth.
                let decay = 0.95f32;
                for v in &mut self.sensory_activity {
                    *v *= decay;
                }
                for layer in &mut self.hidden_activity {
                    for v in layer {
                        *v *= decay;
                    }
                }
                for v in &mut self.output_activity {
                    *v *= decay;
                }
                let snap_opt = self
                    .sensory_spikes_snapshot
                    .try_read()
                    .ok()
                    .map(|s| s.clone());
                if let Some(snap) = snap_opt {
                    if !snap.is_empty() {
                        if self.sensory_activity.len() != snap.len() {
                            self.sensory_activity.resize(snap.len(), 0.0);
                        }
                        self.last_sensory_spikes = snap.clone();
                        for (i, &sv) in snap.iter().enumerate() {
                            if sv != 0 {
                                self.sensory_activity[i] = 1.0;
                            }
                        }
                        self.update_probes_from_snapshot(lif_cloned.dt as f32, snap.as_slice());
                        self.status = format!(
                            "Input spikes: {}/{}",
                            snap.iter().filter(|&&v| v != 0).count(),
                            snap.len()
                        );
                    }
                }
                let ui_snap_opt = self.ui_snapshot.try_read().ok().map(|s| s.clone());
                if let Some(snap) = ui_snap_opt {
                    if !snap.hidden_spikes.is_empty() {
                        self.hidden_activity
                            .resize(snap.hidden_spikes.len(), Vec::new());
                        self.previous_hidden_spikes
                            .resize(snap.hidden_spikes.len(), Vec::new());
                        for (li, spk) in snap.hidden_spikes.iter().enumerate() {
                            if self.hidden_activity[li].len() != spk.len() {
                                self.hidden_activity[li] = vec![0.0; spk.len()];
                            }
                            if self.previous_hidden_spikes[li].len() != spk.len() {
                                self.previous_hidden_spikes[li] = vec![0; spk.len()];
                            }
                            for j in 0..spk.len() {
                                if spk[j] != 0 {
                                    self.hidden_activity[li][j] = 1.0;
                                    self.previous_hidden_spikes[li][j] = 1;
                                } else {
                                    self.previous_hidden_spikes[li][j] = 0;
                                }
                            }
                        }
                    }
                    if !snap.output_spikes.is_empty() {
                        if self.output_activity.len() != snap.output_spikes.len() {
                            self.output_activity.resize(snap.output_spikes.len(), 0.0);
                        }
                        let mut col = vec![0i8; snap.output_spikes.len()];
                        let mut any = false;
                        for (k, &sv) in snap.output_spikes.iter().enumerate() {
                            if sv != 0 {
                                self.output_activity[k] = 1.0;
                                col[k] = 1;
                                any = true;
                            }
                        }
                        let step = self.sim_step_counter.load(Ordering::Relaxed) as usize;
                        if any || (step % 5 == 0) {
                            self.raster_outputs.push_back(col);
                            if self.raster_outputs.len() > self.raster_cols {
                                self.raster_outputs.pop_front();
                            }
                        }
                    }
                }
            }
        } else {
            self.pull_activity();
        }
        #[cfg(all(feature = "robot_io", unix))]
        if matches!(self.view_source, ViewSource::Standalone)
            && self.ipc_connected
            && self.ipc_frame_count == 0
        {
            self.status = "IPC connected; waiting for sensory frames from Webots.".to_string();
        }

        let dist_node_arc = self.distributed_node.clone();
        let state_arc = dist_node_arc.as_ref().map(|n| n.state.clone());
        let state_arc_for_controls = state_arc.clone();

        // --- 2. Panels and Controls ---
        {
            observe_time!("App::update/render");
            egui::Panel::top("top").show_inside(ui, |ui| {
                ui.heading("Neuromorphic Network");
                ui.label("Comparison of conventional models with Auto-Asynchronous Recursive Neuromorphic Network (AARNN)");
            });

            egui::Panel::right("controls")
                .resizable(true)
                .default_size(260.0_f32)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.heading("Controls");
                    let sleep_label = if let Ok(r) = self.runner.try_read() {
                        if r.net.sleep_enabled {
                            if r.sleep_active { "sleeping" } else { "awake" }
                        } else {
                            "off"
                        }
                    } else {
                        "busy"
                    };
                    ui.label(format!("Status: {} | Sleep: {}", self.status, sleep_label));
                    ui.small(format!("FPAA: {}", self.fpaa_status.summary));
                    ui.separator();

                    if let Some(binding) = self.remote_workspace_binding.clone() {
                        ui.group(|ui| {
                            ui.label(format!(
                                "Remote workspace: {} @ {}",
                                binding.workspace_id, binding.base_url
                            ));
                            ui.horizontal(|ui| {
                                if ui.button("Pull").on_hover_text("Load the latest backend workspace snapshot into this UI session").clicked() {
                                    if let Err(err) = self.pull_remote_workspace_snapshot() {
                                        self.status = format!("Remote pull failed: {}", err);
                                    }
                                }
                                if ui.button("Push").on_hover_text("Save the current UI snapshot back into the backend workspace").clicked() {
                                    if let Err(err) = self.push_remote_workspace_snapshot() {
                                        self.status = format!("Remote push failed: {}", err);
                                    } else {
                                        self.status = format!("Pushed remote workspace '{}'", binding.workspace_id);
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Start backend").on_hover_text("Resume background stepping in the backend runtime").clicked() {
                                    if let Err(err) = self.control_remote_workspace_backend(WorkspaceControlAction::Start) {
                                        self.status = format!("Remote start failed: {}", err);
                                    }
                                }
                                if ui.button("Stop backend").on_hover_text("Pause background stepping in the backend runtime").clicked() {
                                    if let Err(err) = self.control_remote_workspace_backend(WorkspaceControlAction::Stop) {
                                        self.status = format!("Remote stop failed: {}", err);
                                    }
                                }
                            });
                            ui.separator();
                            let neuron_count = self
                                .runner
                                .try_read()
                                .ok()
                                .map(|runner| runner.total_neurons());
                            let token_balance = self.remote_token_balance.clone();
                            let token_balance_ref = token_balance.as_ref();
                            let neuron_daily_rate = token_balance_ref
                                .map(|balance| balance.neuron_daily_rate.max(0))
                                .unwrap_or(1);
                            let projected_burn = neuron_count
                                .map(|count| (count as i64).saturating_mul(neuron_daily_rate));
                            let balance_label = if self.remote_token_refresh_inflight
                                && token_balance_ref.is_none()
                            {
                                "Loading...".to_string()
                            } else {
                                token_balance_ref
                                    .map(|balance| format!("{} tok", balance.balance))
                                    .unwrap_or_else(|| "-".to_string())
                            };
                            ui.label(format!("Token balance: {}", balance_label));
                            if let Some(count) = neuron_count {
                                let burn = projected_burn.unwrap_or(0);
                                ui.small(format!(
                                    "Projected burn: {} tok/day ({} neurons x {} tok)",
                                    burn, count, neuron_daily_rate
                                ));
                            } else {
                                ui.small(format!(
                                    "Projected burn rate: {} tok per neuron per day.",
                                    neuron_daily_rate
                                ));
                            }
                            if let Some(balance) = token_balance_ref {
                                if let Some(updated_at) = balance.updated_at.as_deref() {
                                    ui.small(format!("Ledger snapshot: {}", updated_at));
                                }
                            }
                            if let Some(error) = self.remote_token_error.as_deref() {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 170, 120),
                                    format!("Token info unavailable: {}", error),
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Refresh tokens").clicked() {
                                    self.queue_remote_token_refresh(true);
                                }
                                if let Some(url) = token_balance_ref
                                    .and_then(|balance| balance.token_vault_url.as_deref())
                                {
                                    ui.hyperlink_to("Token Vault", url);
                                }
                                if let Some(url) = token_balance_ref
                                    .and_then(|balance| balance.buy_tokens_url.as_deref())
                                {
                                    ui.hyperlink_to("Buy tokens", url);
                                }
                                if let Some(url) = token_balance_ref
                                    .and_then(|balance| balance.billing_dashboard_url.as_deref())
                                {
                                    ui.hyperlink_to("Billing", url);
                                }
                                if let Some(url) = token_balance_ref
                                    .and_then(|balance| balance.billing_admin_url.as_deref())
                                {
                                    ui.hyperlink_to("Admin Billing", url);
                                }
                            });
                            if binding.save_on_exit {
                                ui.small("Remote workspace auto-push on exit is enabled.");
                            }
                        });
                        ui.separator();
                    }

                    let view_is_standalone = matches!(self.view_source, ViewSource::Standalone);
                    let view_playing_state =
                        self.resolve_view_playing(state_arc_for_controls.as_ref());
                    let view_playing = view_playing_state.unwrap_or(false);
                    let play_button_label = match view_playing_state {
                        Some(true) => "Stop",
                        Some(false) => "Start",
                        None => "Syncing...",
                    };
                    let play_button_hover = if view_playing_state.is_some() {
                        "Start/stop simulation stepping"
                    } else {
                        "Waiting for simulation state sync"
                    };

                    if self.remote_only && view_is_standalone {
                        ui.label("Remote-only mode: local simulation disabled.");
                    } else {
                        if view_is_standalone
                            && self.distributed_node.is_some()
                            && (self.dist_is_orchestrator
                                && !self.dist_network_registry.is_empty()
                                || !self.dist_local_playing_cache.is_empty())
                        {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 196, 128),
                                "Standalone view controls only this local runner.",
                            );
                            ui.small(
                                "Orchestrator Start/Stop/Repeat/Reset affects the selected cluster network instead. Switch View Selection to a local managed or cluster network to control the distributed run.",
                            );
                        }
                        let view_network_id = match &self.view_source {
                            ViewSource::Standalone => None,
                            ViewSource::LocalManaged(id) | ViewSource::ClusterGlobal(id) => Some(id.clone()),
                        };
                        ui.horizontal(|ui| {
                            let play_clicked = ui
                                .add_enabled(
                                    view_playing_state.is_some(),
                                    egui::Button::new(play_button_label),
                                )
                                .on_hover_text(play_button_hover)
                                .clicked();
                            if play_clicked {
                                if view_is_standalone {
                                    self.set_standalone_playing(!self.playing);
                                } else if let Some(net_id) = view_network_id.as_ref() {
                                    let action = if view_playing {
                                        control_update::Action::Stop
                                    } else {
                                        control_update::Action::Start
                                    };
                                    let label = if view_playing { "stopped" } else { "started" };
                                    self.apply_cluster_control(net_id, action, label);
                                } else {
                                    self.status = "Cluster control unavailable".into();
                                }
                            }
                            if ui.button("Repeat").on_hover_text("Reset state and start from t=0").clicked() {
                                if view_is_standalone {
                                    self.reset_and_start_standalone("Reset");
                                } else if let Some(net_id) = view_network_id.as_ref() {
                                    self.apply_cluster_control(net_id, control_update::Action::Repeat, "restarted");
                                } else {
                                    self.status = "Cluster control unavailable".into();
                                }
                            }
                            if ui.button("Reset").on_hover_text("Reset to startup state (stopped)").clicked() {
                                if view_is_standalone {
                                    self.reset_to_network_state(
                                        self.initial_lif.clone(),
                                        self.initial_stdp.clone(),
                                        self.initial_net_cfg.clone(),
                                        self.initial_model,
                                        self.initial_learning,
                                        "Reset to startup state",
                                    );
                                } else if let Some(net_id) = view_network_id.as_ref() {
                                    self.apply_cluster_control(net_id, control_update::Action::Reset, "reset");
                                } else {
                                    self.status = "Cluster control unavailable".into();
                                }
                            }
                            if ui.button("New").on_hover_text("Create a fresh single-neuron network (clears loaded snapshot)").clicked() {
                                if view_is_standalone {
                                    let (lif_cloned, stdp_cloned) = {
                                        let runner = match self.runner.try_read() {
                                            Ok(runner) => runner,
                                            Err(_) => {
                                                self.status = "Runner busy".to_string();
                                                return;
                                            }
                                        };
                                        (runner.lif.clone(), runner.stdp.clone())
                                    };
                                    let preset = match self.izh_preset {
                                        IzhPreset::RS => "RS",
                                        IzhPreset::FS => "FS",
                                        IzhPreset::IB => "IB",
                                        IzhPreset::CH => "CH",
                                        IzhPreset::LTS => "LTS",
                                        IzhPreset::RZ => "RZ",
                                        IzhPreset::TC => "TC",
                                        IzhPreset::P => "P",
                                    };
                                    let model = match self.neuron_model {
                                        NeuronModelSel::Lif => NeuronModel::Lif,
                                        NeuronModelSel::Izh => NeuronModel::Izh(IzhikevichParams::from_preset(preset, lif_cloned.dt)),
                                        NeuronModelSel::Aarnn => NeuronModel::Aarnn,
                                    };
                                    let learning = match self.learning {
                                        LearningSel::Stdp => Learning::Stdp,
                                        LearningSel::Hebb => Learning::Hebb,
                                        LearningSel::Oja => Learning::Oja,
                                        LearningSel::Aarnn => Learning::Aarnn,
                                    };
                                    let net = NetworkConfig::default();
                                    self.initial_lif = lif_cloned.clone();
                                    self.initial_stdp = stdp_cloned.clone();
                                    self.initial_net_cfg = net.clone();
                                    self.initial_model = model;
                                    self.initial_learning = learning;
                                    self.reset_to_network_state(
                                        lif_cloned,
                                        stdp_cloned,
                                        net,
                                        model,
                                        learning,
                                        "New network created",
                                    );
                                } else if let Some(net_id) = view_network_id.as_ref() {
                                    self.apply_cluster_control(net_id, control_update::Action::New, "new network");
                                } else {
                                    self.status = "Cluster control unavailable".into();
                                }
                            }
                        });
                    }
                    ui.separator();
                ui.collapsing("Resources & Performance", |ui| {
                    ui.label(format!("CPU: {:.1}%", self.cpu_usage));
                    ui.label(format!("RAM: {:.1} MB", self.ram_usage_mb));
                    #[cfg(feature = "sysinfo")]
                    {
                        if let Some(temp) = self.cpu_temp_c {
                            ui.label(format!("Temp: {:.1} C", temp));
                        } else {
                            ui.label("Temp: n/a");
                        }
                    }
                    #[cfg(feature = "opencl")]
                    {
                        let cl_status = self
                            .runner
                            .try_read()
                            .map(|r| {
                                r.cl.as_ref()
                                    .map(|cl| (cl.execution_target(), cl.is_cuda_backend()))
                            })
                            .ok();
                        match cl_status {
                            Some(Some((crate::cl_compute::OpenCLExecutionTarget::Gpu, true))) => {
                                ui.label("GPU: Detected (CUDA)");
                                if self.playing {
                                    let use_aarnn = matches!(model_cloned, NeuronModel::Aarnn);
                                    if !use_aarnn || !net_cloned.use_morphology {
                                        ui.label("GPU Status: Active (Dense CUDA path)");
                                    } else {
                                        ui.label("GPU Status: Active (Sparse CUDA path)");
                                    }
                                } else {
                                    ui.label("GPU Status: Inactive");
                                }
                            }
                            Some(Some((crate::cl_compute::OpenCLExecutionTarget::Gpu, false))) => {
                                ui.label("GPU: Detected (OpenCL)");
                                if self.playing {
                                    let use_aarnn = matches!(model_cloned, NeuronModel::Aarnn);
                                    if !use_aarnn || !net_cloned.use_morphology {
                                        ui.label("GPU Status: Active (Dense path)");
                                    } else {
                                        ui.label("GPU Status: Active (Sparse path)");
                                    }
                                } else {
                                    ui.label("GPU Status: Inactive");
                                }
                            }
                            Some(Some((crate::cl_compute::OpenCLExecutionTarget::Cpu, _))) => {
                                ui.label("GPU: Not Detected (OpenCL CPU fallback)");
                                if self.playing {
                                    ui.label("GPU Status: Active (OpenCL CPU path)");
                                } else {
                                    ui.label("GPU Status: Inactive");
                                }
                            }
                            Some(None) => {
                                ui.label("GPU: Not Detected");
                            }
                            None => {
                                ui.label("GPU: Busy");
                            }
                        }
                    }
                    ui.label(format!("Rayon Pool Threads: {}", rayon::current_num_threads()));
                    if let Ok(r) = self.runner.try_read() {
                        let sim_parallel = r.sim_parallel_status();
                        if sim_parallel.enabled {
                            ui.label(format!(
                                "Sim Parallel Workers: {}/{}",
                                sim_parallel.worker_budget,
                                sim_parallel.max_workers
                            ));
                            ui.label(format!(
                                "Sim Parallel Ramp/Health: {:.0}% / {:.0}%",
                                sim_parallel.ramp_ratio * 100.0,
                                sim_parallel.health_ratio * 100.0
                            ));
                            ui.label(format!(
                                "Sim Parallel Thresholds: light {} heavy {} matrix {} ops",
                                sim_parallel.light_neuron_threshold,
                                sim_parallel.heavy_neuron_threshold,
                                sim_parallel.matrix_ops_threshold
                            ));
                        } else {
                            ui.label("Sim Parallel: Single-worker");
                        }
                    }
                    ui.label(format!(
                        "Affinity Rotation: {}",
                        if crate::affinity::affinity_rotation_enabled() { "On" } else { "Off" }
                    ));
                    ui.label("Async Runtime: Shared");
                    if self.os_threads > 0 {
                        ui.label(format!("OS Threads: {}", self.os_threads));
                        ui.label(format!("Runnable Threads: {}", self.runnable_threads));
                    }
                    if self.cpu_core_count > 0 {
                        let spread_pct = if self.cpu_core_count > 0 {
                            (self.hot_core_count as f32 * 100.0) / self.cpu_core_count as f32
                        } else {
                            0.0
                        };
                        ui.label(format!(
                            "Hot Core Spread (>= {:.0}%): {}/{} ({:.0}%)",
                            self.hot_core_threshold_pct,
                            self.hot_core_count,
                            self.cpu_core_count,
                            spread_pct
                        ));
                        if !self.hot_core_top.is_empty() {
                            let top = self.hot_core_top.iter()
                                .map(|(idx, usage)| format!("C{}:{:.0}%", idx, usage))
                                .collect::<Vec<_>>()
                                .join("  ");
                            ui.label(format!("Top Cores: {}", top));
                        }
                    }
                    let (pacing, reason) = crate::ga::ga_pacing_status();
                    ui.label(ga_pacing_label(pacing, &reason));
                    if let Some(ramp) = crate::ga::ga_ramp_runtime_status() {
                        ui.label(ga_ramp_label(ramp.population_size, ramp.worker_cap, ramp.sim_time_ms));
                    } else {
                        ui.label("GA Ramp: No");
                    }
                    ui.separator();
                    ui.label(format!("Step time: {:.2} ms", self.avg_step_time_ms));
                    ui.checkbox(&mut self.auto_dt_enabled, "Auto-adjust dt (responsiveness)");
                    ui.add(egui::Slider::new(&mut self.responsiveness_target_ms, 1.0..=33.0).text("Target (ms)"));
                    {
                        ui.label(format!("UI Target FPS: {:.1}", net_cloned.ui_target_fps));
                        ui.label(format!("Current dt: {:.3} ms", lif_cloned.dt));
                    }
                });

                ui.collapsing("Remote Orchestrators", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Address");
                        ui.text_edit_singleline(&mut self.remote_addr_input);
                        if ui.button("Add").clicked() {
                            let addr_input = self.remote_addr_input.clone();
                            let _ = self.add_remote_orchestrator_connection(&addr_input);
                        }
                    });

                    if self.remote_connections.is_empty() {
                        ui.label("(none)");
                    }

                    let mut remove_idx = None;
                    for (idx, conn) in self.remote_connections.iter().enumerate() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(&conn.addr);
                            if ui.button("Remove").clicked() {
                                remove_idx = Some(idx);
                            }
                        });
                        if let Some(snapshot) = self.remote_statuses.get(&conn.addr) {
                            ui.label(format!("Nodes: {}", snapshot.nodes.len()));
                            ui.label(format!("Networks: {}", snapshot.networks.len()));
                            if let Some(err) = &snapshot.last_error {
                                ui.colored_label(egui::Color32::LIGHT_RED, err);
                            } else {
                                let age = snapshot.last_update.elapsed().as_secs();
                                ui.label(format!("Last update: {}s ago", age));
                            }
                        } else {
                            ui.label("Status: pending...");
                        }
                    }
                    if let Some(idx) = remove_idx {
                        if let Some(conn) = self.remote_connections.get(idx) {
                            conn.stop.store(true, Ordering::SeqCst);
                        }
                        if let Some(conn) = self.remote_connections.get(idx) {
                            self.remote_statuses.remove(&conn.addr);
                        }
                        self.remote_connections.remove(idx);
                    }
                });

                let mut filter_node: Option<String> = None;
                if self.distributed_node.is_some() {
                    let mut new_source = None;
                    let mut clear_filter = false;

                    ui.collapsing("View Selection & Cluster", |ui| {
                        let current_view = match &self.view_source {
                            ViewSource::Standalone => format!("Standalone: {}", self.brain_id),
                            ViewSource::LocalManaged(id) => format!("Local: {}", id),
                            ViewSource::ClusterGlobal(id) => format!("Cluster: {}", id),
                        };
                        ui.label(format!("Current view: {}", current_view));
                        if let Some(filter) = self.view_node_filter.as_ref() {
                            ui.label(format!("Node focus: {}", filter));
                        }

                        ui.separator();
                        ui.label("Combined Views:");
                        ui.horizontal(|ui| {
                            if ui.button("Standalone").clicked() {
                                new_source = Some(ViewSource::Standalone);
                                clear_filter = true;
                            }
                            if is_orchestrator {
                                if ui.button("Cluster Global (combined)").clicked() {
                                    // Use the network with the most neurons (has distribution
                                    // data) rather than the orchestrator's own brain_id, which
                                    // is a node ID and may not match any registered network.
                                    let net_id = network_registry
                                        .iter()
                                        .filter(|(_, s)| !s.distribution.is_empty())
                                        .max_by_key(|(_, s)| s.total_neurons)
                                        .map(|(id, _)| id.clone())
                                        .unwrap_or_else(|| self.brain_id.clone());
                                    new_source = Some(ViewSource::ClusterGlobal(net_id));
                                    clear_filter = true;
                                }
                            }
                        });

                        ui.separator();
                        ui.label("Local Managed Networks:");
                        let mut net_ids: Vec<_> = connected_nodes.values()
                            .filter(|n| n.node_id == node_id)
                            .flat_map(|n| n.active_networks.iter())
                            .cloned()
                            .collect();
                        net_ids.sort();
                        net_ids.dedup();

                        if net_ids.is_empty() {
                            ui.label("  (None)");
                        }
                        for id in net_ids {
                            if ui.selectable_label(self.view_source == ViewSource::LocalManaged(id.clone()), format!("Local: {}", id)).clicked() {
                                new_source = Some(ViewSource::LocalManaged(id));
                            }
                        }

                        if is_orchestrator {
                            ui.separator();
                            ui.label("Cluster Networks (combined):");
                            let mut reg_ids: Vec<_> = network_registry.keys().cloned().collect();
                            reg_ids.sort();
                            for id in reg_ids {
                                if ui.selectable_label(self.view_source == ViewSource::ClusterGlobal(id.clone()), format!("Cluster: {}", id)).clicked() {
                                    new_source = Some(ViewSource::ClusterGlobal(id));
                                    clear_filter = true;
                                }
                            }
                        }

                        if is_orchestrator {
                            ui.separator();
                            ui.label("Individual Node Focus (cluster):");
                            for (id, status) in &connected_nodes {
                                let label = format!("Focus {}", id);
                                if ui.button(label).clicked() {
                                    // Keep current ClusterGlobal network ID if already set,
                                    // otherwise pick the best registered network.
                                    let net_id = if let ViewSource::ClusterGlobal(cur) = &self.view_source {
                                        cur.clone()
                                    } else {
                                        network_registry
                                            .iter()
                                            .filter(|(_, s)| !s.distribution.is_empty())
                                            .max_by_key(|(_, s)| s.total_neurons)
                                            .map(|(nid, _)| nid.clone())
                                            .unwrap_or_else(|| self.brain_id.clone())
                                    };
                                    new_source = Some(ViewSource::ClusterGlobal(net_id));
                                    filter_node = Some(id.clone());
                                }
                                if let Some(res) = &status.resources {
                                    ui.label(format!(
                                        "  cap {:.2} cpu {:.1}% ram {}MB",
                                        res.capacity_score,
                                        res.cpu_usage,
                                        res.available_ram / 1024 / 1024
                                    ));
                                }
                            }
                        }

                        if let Some(filter) = self.view_node_filter.as_ref() {
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(format!("Filtering by: {}", filter));
                                if ui.button("Clear").clicked() {
                                    clear_filter = true;
                                }
                            });
                        }
                    });

                    if let Some(source) = new_source { self.set_view_source(source); }
                    if clear_filter { self.view_node_filter = None; }
                }

                if self.distributed_node.is_some() {
                    ui.collapsing("Cluster Dashboard", |ui| {
                        ui.label(format!("Node ID: {}", node_id));
                        ui.label(format!("Role: {}", if is_orchestrator { "Orchestrator" } else { "Worker Node" }));

                        if is_orchestrator {
                            ui.separator();
                            ui.label(format!("Active Networks: {}", network_registry.len()));
                            for (id, net) in &network_registry {
                                ui.collapsing(format!("Network: {}", id), |ui| {
                                    let deployment_modes = if net.deployment_modes.is_empty() {
                                        "auto".to_string()
                                    } else {
                                        net.deployment_modes.join(", ")
                                    };
                                    ui.label(format!("dt: {:.3} ms", net.current_dt));
                                    ui.label(format!("Total Neurons: {}", net.total_neurons));
                                    ui.label(format!(
                                        "Deployment: {} [{}]",
                                        deployment_modes, net.deployment_scope
                                    ));
                                    ui.label(format!(
                                        "Live Transition: {} | Autonomous: {}",
                                        if net.live_transition_allowed { "yes" } else { "no" },
                                        if net.autonomous_transition_enabled {
                                            "yes"
                                        } else {
                                            "no"
                                        }
                                    ));
                                    if !net.last_transition_reason.is_empty() {
                                        let source = if net.last_transition_source.is_empty() {
                                            "unknown"
                                        } else {
                                            &net.last_transition_source
                                        };
                                        ui.label(format!(
                                            "Last Transition: {} at {} ms ({})",
                                            source, net.last_transition_ts_ms, net.last_transition_reason
                                        ));
                                    }

                                    // Calculate estimated nodes for 1ms cycle
                                    let mut total_workload_ms = 0.0;
                                    let mut total_cluster_neurons = 0;
                                    for node_status in connected_nodes.values() {
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
                                    ui.label(format!("Est. nodes for 1ms cycle: {:.1}", est_nodes_1ms));

                                    ui.label("Distribution (node -> layers(neurons)):");
                                    for (node_id, range) in &net.distribution {
                                        let layer_info: Vec<String> = range.layers.iter().map(|&l| {
                                            let count = range.layer_neuron_counts.get(&l).cloned().unwrap_or(0);
                                            format!("{}({})", l, count)
                                        }).collect();
                                        ui.label(format!(" - {}: [{}]", node_id, layer_info.join(", ")));
                                    }
                                });
                            }

                            ui.separator();
                            let cluster_ga_evals: u64 = connected_nodes.values()
                                .filter_map(|s| s.resources.as_ref())
                                .map(|r| r.ga_total_evaluations)
                                .sum();
                            let local_ga_evals = crate::ga::ga_total_evaluations();
                            let total_cluster_ga_evals = cluster_ga_evals + local_ga_evals;

                            ui.label(format!("Connected Nodes: {}", connected_nodes.len()));
                            if total_cluster_ga_evals > 0 {
                                ui.label(format!("Cluster GA Evaluations: {}", total_cluster_ga_evals));
                            }

                            for (id, status) in &connected_nodes {
                                ui.collapsing(format!("Node: {}", id), |ui| {
                                    ui.label(format!("Address: {}", status.address));
                                    if let Some(res) = &status.resources {
                                        ui.label(format!("CPU: {:.1}%", res.cpu_usage));
                                        ui.label(format!("RAM: {}/{} MB", res.available_ram / 1024 / 1024, res.total_ram / 1024 / 1024));
                                        if res.temperature_c > 0.0 {
                                            ui.label(format!("Temp: {:.1} C", res.temperature_c));
                                        } else {
                                            ui.label("Temp: n/a");
                                        }

                                        ui.separator();
                                        ui.label(format!("GA Evaluations: {}", res.ga_total_evaluations));
                                        if total_cluster_ga_evals > 0 {
                                            let share = (res.ga_total_evaluations as f32 / total_cluster_ga_evals as f32) * 100.0;
                                            ui.label(format!("Cluster Contribution: {:.1}%", share));
                                        }

                                        ui.label(ga_pacing_label(res.ga_pacing, &res.ga_pacing_reason));
                                        if res.ga_ramp_active {
                                            ui.label(ga_ramp_label(
                                                res.ga_ramp_population as usize,
                                                res.ga_ramp_worker_cap as usize,
                                                res.ga_ramp_sim_time_ms,
                                            ));
                                        } else {
                                            ui.label("GA Ramp: No");
                                        }
                                        ui.label(format!("Neurons: {}", res.num_neurons));
                                        ui.label(format!("Redundant: {}", res.redundant_neurons));
                                        ui.label(format!("AARNN Depth: {}/{}", res.current_aarnn_depth, res.desired_aarnn_depth));
                                        ui.label(format!("Capacity Score: {:.2}", res.capacity_score));
                                        if !res.telemetry_source.is_empty() {
                                            ui.separator();
                                            ui.label(format!(
                                                "External Telemetry: {}",
                                                res.telemetry_source
                                            ));
                                            ui.label(format!(
                                                "Telemetry CPU/Mem: {:.1}% / {:.1}%",
                                                res.telemetry_cpu_usage_pct,
                                                res.telemetry_mem_used_pct
                                            ));
                                            ui.label(format!(
                                                "Telemetry Net RX/TX: {:.0} / {:.0} Bps",
                                                res.telemetry_net_rx_bps,
                                                res.telemetry_net_tx_bps
                                            ));
                                            ui.label(format!(
                                                "Telemetry Disk Used: {:.1}% | Actions: {}",
                                                res.telemetry_disk_used_pct,
                                                res.telemetry_recent_action_count
                                            ));
                                            if res.num_gpus > 0
                                                || res.telemetry_gpu_util_pct > 0.0
                                                || res.telemetry_gpu_temp_c > 0.0
                                                || res.telemetry_gpu_power_w > 0.0
                                            {
                                                ui.label(format!(
                                                    "Telemetry GPU Util/Temp/Power: {:.1}% / {:.1} C / {:.1} W",
                                                    res.telemetry_gpu_util_pct,
                                                    res.telemetry_gpu_temp_c,
                                                    res.telemetry_gpu_power_w
                                                ));
                                            }
                                        }

                                        if res.ga_running {
                                            ui.separator();
                                            ui.colored_label(egui::Color32::LIGHT_GREEN, "🧬 GA Search Running");
                                            ui.label(format!("Generation: {}", res.ga_generation));
                                            ui.label(format!("Best Fitness: {:.4}", res.ga_best_fitness));

                                            if !res.ga_best_config_json.is_empty() {
                                                if ui.button("Apply Node's Best GA Params").clicked() {
                                                    if let Ok(cfg) = serde_json::from_str(&res.ga_best_config_json) {
                                                        if let Ok(mut runner) = self.runner.try_write() {
                                                            runner.apply_config(cfg);
                                                            self.status = format!("Applied best GA params from node {}", id);
                                                        } else {
                                                            self.status = "Runner busy".to_string();
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if res.ga_evaluating {
                                            ui.separator();
                                            ui.colored_label(egui::Color32::GOLD, "🧪 Evaluating GA Individual");
                                            if res.ga_active_eval_seed > 0 {
                                                ui.label(format!("Seed: {}", res.ga_active_eval_seed));
                                            }
                                            if res.ga_eval_progress > 0.0 && res.ga_eval_progress < 1.0 {
                                                ui.add(egui::ProgressBar::new(res.ga_eval_progress).show_percentage());
                                            } else {
                                                ui.spinner();
                                            }
                                        }
                                    }
                                    ui.label(format!("Active Networks: {:?}", status.active_networks));
                                    ui.horizontal(|ui| {
                                        if ui.button("👁 View Node Part").clicked() {
                                            filter_node = Some(id.clone());
                                        }
                                        if ui.button("🎯 Select as Display Base").clicked() {
                                            filter_node = Some(id.clone());
                                            // Also sync parameters if possible
                                            if let Some(res) = &status.resources {
                                                if !res.ga_best_config_json.is_empty() {
                                                    if let Ok(cfg) = serde_json::from_str(&res.ga_best_config_json) {
                                                        if let Ok(mut runner) = self.runner.try_write() {
                                                            runner.apply_config(cfg);
                                                        } else {
                                                            self.status = "Runner busy".to_string();
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    });
                                });
                            }
                        } else {
                            if let Some(ref _node) = self.distributed_node {
                                // We can't re-lock easily here if we are inside a closure that captures self.
                                // But we already have orchestrator_addr if we extracted it.
                                // Let's add it to the extraction.
                            }
                        }
                    });
                    if let Some(nid) = filter_node {
                        if self.view_node_filter.as_ref() != Some(&nid) {
                            self.view_node_filter = Some(nid);
                            self.refresh_ui_buffers(); // Trigger layout recompute when switching node parts
                        }
                        if !matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
                            if let Some(net_id) = self.preferred_cluster_network_id() {
                                self.set_view_source(ViewSource::ClusterGlobal(net_id));
                            }
                        }
                    }
                }

                ui.collapsing("Network Architecture", |ui| {

                let prev_model_sel = self.neuron_model;
                let prev_learning_sel = self.learning;

                ui.label("Neuron model").on_hover_text("Choose membrane dynamics: LIF or Izhikevich");
                ui.horizontal(|ui|{
                    ui.radio_value(&mut self.neuron_model, NeuronModelSel::Lif, "LIF").on_hover_text("Leaky Integrate-and-Fire: simple threshold-and-reset model");
                    ui.radio_value(&mut self.neuron_model, NeuronModelSel::Izh, "Izh").on_hover_text("Izhikevich: rich spiking dynamics via quadratic integrate-and-fire");
                    ui.radio_value(&mut self.neuron_model, NeuronModelSel::Aarnn, "AARNN").on_hover_text("Axon–Axon–Recurrent Neural Network: includes connection length and transmission velocity in thresholding");
                });

                // AARNN specific parameters - always visible when AARNN is selected
                if matches!(self.neuron_model, NeuronModelSel::Aarnn) || matches!(self.learning, LearningSel::Aarnn) {
                    ui.add_space(4.0);
                    ui.label("AARNN Biological Realism").on_hover_text("Configure biological growth and transmission parameters");
                    let net = &mut self.local_net;
                    let mut theta_changed = false;
                    theta_changed |= ui.checkbox(&mut net.theta_rhythm_enabled, "Theta rhythm")
                        .on_hover_text("Use a global theta rhythm drive instead of random spiking")
                        .changed();
                    if net.theta_rhythm_enabled {
                        theta_changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_hz, 0.5..=12.0).text("Theta Hz"))
                            .on_hover_text("Theta oscillation frequency in Hz")
                            .changed();
                        theta_changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_duty, 0.05..=0.9).text("Theta duty"))
                            .on_hover_text("Fraction of the cycle that drives spikes")
                            .changed();
                        theta_changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_drive, 0.0..=20.0).text("Theta drive"))
                            .on_hover_text("Current injected during the active phase")
                            .changed();
                        theta_changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_phase_jitter, 0.0..=1.0).text("Theta jitter"))
                            .on_hover_text("Phase jitter across neurons (0 = synchronized)")
                            .changed();
                        if let Ok(r) = runner_arc.try_read() {
                            let phase = r.theta_phase % std::f32::consts::TAU;
                            let gate = phase.sin() * 0.5 + 0.5;
                            let duty = net.theta_rhythm_duty.clamp(0.01, 1.0);
                            let active = gate >= 1.0 - duty;
                            ui.add(egui::ProgressBar::new(gate).text(format!("Theta gate {:.2} {}", gate, if active { "on" } else { "off" })));
                            ui.label(format!("Theta phase {:.2} rad", phase));
                        } else {
                            ui.label("Theta phase: busy");
                        }
                    } else if ui.add(egui::Slider::new(&mut net.aarnn_synaptic_energy_randomness, 0.0..=1.0).text("Synaptic Energy Randomness"))
                        .on_hover_text("Initial random spiking probability for first layer neurons").changed() {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        }
                    if theta_changed {
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        if matches!(self.input_source, InputSource::Theta) {
                            let n = net.num_sensory_neurons;
                            let dt_ms = lif_cloned.dt.max(0.001) as f32;
                            let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(ThetaProvider::new(
                                n,
                                net.theta_rhythm_hz,
                                net.theta_rhythm_duty,
                                net.theta_rhythm_phase_jitter,
                                dt_ms,
                            ))));
                        }
                    }

                    ui.add_space(4.0);
                    ui.collapsing("Thalamic Gating", |ui| {
                        let mut gate_changed = false;
                        gate_changed |= ui.checkbox(&mut net.thalamic_gating_enabled, "Enable thalamic gating")
                            .on_hover_text("Rhythmic gating of sensory inputs (AARNN only)")
                            .changed();
                        if net.thalamic_gating_enabled {
                            gate_changed |= ui.add(egui::Slider::new(&mut net.thalamic_gate_hz, 0.5..=20.0).text("Gate Hz"))
                                .on_hover_text("Gating frequency in Hz")
                                .changed();
                            gate_changed |= ui.add(egui::Slider::new(&mut net.thalamic_gate_duty, 0.05..=0.95).text("Gate duty"))
                                .on_hover_text("Fraction of cycle that passes sensory spikes")
                                .changed();
                            gate_changed |= ui.add(egui::Slider::new(&mut net.thalamic_gate_floor, 0.0..=1.0).text("Gate floor"))
                                .on_hover_text("Minimum pass-through probability during closed phase")
                                .changed();
                            if let Ok(r) = runner_arc.try_read() {
                                let phase = r.thalamic_gate_phase % std::f32::consts::TAU;
                                let phase_gate = phase.sin() * 0.5 + 0.5;
                                let duty = net.thalamic_gate_duty.clamp(0.01, 1.0);
                                let open = phase_gate >= 1.0 - duty;
                                let pass = if open { 1.0 } else { net.thalamic_gate_floor.clamp(0.0, 1.0) };
                                ui.add(egui::ProgressBar::new(pass).text(format!("Gate {:.2} {}", pass, if open { "open" } else { "closed" })));
                                ui.label(format!("Thalamic phase {:.2} rad", phase));
                            } else {
                                ui.label("Thalamic phase: busy");
                            }
                        }
                        if gate_changed {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        }
                    });

                    ui.add_space(4.0);
                    ui.collapsing("Perceptual Loop", |ui| {
                        let mut loop_changed = false;
                        loop_changed |= ui.checkbox(&mut net.perceptual_loop_enabled, "Enable perceptual loop")
                            .on_hover_text("Predict sensory input and update a prediction state each step")
                            .changed();
                        if net.perceptual_loop_enabled {
                            loop_changed |= ui.add(egui::Slider::new(&mut net.perceptual_prediction_lr, 0.0..=1.0).text("Pred LR"))
                                .on_hover_text("Prediction state update rate")
                                .changed();
                            loop_changed |= ui.add(egui::Slider::new(&mut net.perceptual_prediction_decay, 0.0..=0.5).text("Pred decay"))
                                .on_hover_text("Per-step decay applied to prediction state")
                                .changed();
                            loop_changed |= ui.add(egui::Slider::new(&mut net.perceptual_prediction_threshold, 0.0..=1.0).text("Pred thresh"))
                                .on_hover_text("Threshold for predicted spikes")
                                .changed();
                            loop_changed |= ui.add(egui::Slider::new(&mut net.perceptual_error_gain, 0.0..=20.0).text("Error gain"))
                                .on_hover_text("Prediction error drive injected into hidden layer 0")
                                .changed();
                            loop_changed |= ui.add(egui::Slider::new(&mut net.perceptual_feedback_gain, 0.0..=1.0).text("Feedback gain"))
                                .on_hover_text("Blend output-driven predictions into sensory prediction")
                                .changed();
                        }
                        if loop_changed {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        }
                    });

                    ui.add_space(4.0);
                    ui.collapsing("World Model", |ui| {
                        let mut wm_changed = false;
                        wm_changed |= ui.checkbox(&mut net.world_model_enabled, "Enable world model")
                            .on_hover_text("Maintain a low-dimensional phase-space state from hidden activity")
                            .changed();
                        if net.world_model_enabled {
                            wm_changed |= ui.add(egui::Slider::new(&mut net.world_model_dim, 2..=32).text("Dim"))
                                .on_hover_text("World-model state dimension")
                                .changed();
                            wm_changed |= ui.add(egui::Slider::new(&mut net.world_model_decay, 0.0..=0.5).text("Decay"))
                                .on_hover_text("EMA decay applied to world-model state")
                                .changed();
                            if let Ok(r) = runner_arc.try_read() {
                                if !r.world_model_state.is_empty() {
                                    let mut line = String::new();
                                    for (i, v) in r.world_model_state.iter().enumerate() {
                                        if i > 0 { line.push_str(", "); }
                                        line.push_str(&format!("{:.2}", v));
                                    }
                                    ui.label(format!("State: [{}]", line));
                                } else {
                                    ui.label("State: (empty)");
                                }
                            } else {
                                ui.label("State: busy");
                            }
                        }
                        if wm_changed {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        }
                    });

                    ui.add_space(4.0);
                    ui.collapsing("Sleep / Dream", |ui| {
                        let mut sleep_changed = false;
                        sleep_changed |= ui.checkbox(&mut net.sleep_enabled, "Enable sleep/dream")
                            .on_hover_text("Cycle between wake and sleep with dream replay")
                            .changed();
                        if net.sleep_enabled {
                            sleep_changed |= ui.add(egui::Slider::new(&mut net.sleep_cycle_ms, 1000.0..=600000.0).text("Cycle ms"))
                                .on_hover_text("Length of the wake+sleep cycle")
                                .changed();
                            sleep_changed |= ui.add(egui::Slider::new(&mut net.sleep_duration_ms, 100.0..=120000.0).text("Sleep ms"))
                                .on_hover_text("Sleep duration within the cycle")
                                .changed();
                            sleep_changed |= ui.add(egui::Slider::new(&mut net.sleep_dream_replay_prob, 0.0..=1.0).text("Replay prob"))
                                .on_hover_text("Probability of replaying sensory history during sleep")
                                .changed();
                            sleep_changed |= ui.add(egui::Slider::new(&mut net.sleep_dream_threshold, 0.0..=1.0).text("Dream thresh"))
                                .on_hover_text("Threshold for dream spikes from predictions")
                                .changed();
                            sleep_changed |= ui.add(egui::Slider::new(&mut net.sleep_consolidation_gain, 0.0..=1.0).text("Consolidation gain"))
                                .on_hover_text("Boost consolidation during sleep")
                                .changed();
                        }
                    if let Ok(r) = runner_arc.try_read() {
                        ui.label(format!("Sleep state: {}", if r.sleep_active { "sleeping" } else { "awake" }));
                    }
                    if sleep_changed {
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                    }
                });
                    ui.add_space(4.0);
                    ui.collapsing("Neuromod / Resonance", |ui| {
                        let mut nm_changed = false;
                        let signal_label = |sig: NeuromodSignal| -> &'static str {
                            match sig {
                                NeuromodSignal::None => "None",
                                NeuromodSignal::RewardProxy => "Reward proxy",
                                NeuromodSignal::PerceptualError => "Perceptual error",
                                NeuromodSignal::WorldModelError => "World-model error",
                                NeuromodSignal::OutputSpikes => "Output spikes",
                                NeuromodSignal::SensorySpikes => "Sensory spikes",
                                NeuromodSignal::HiddenSpikes => "Hidden spikes",
                                NeuromodSignal::Stability => "Stability",
                            }
                        };
                        let pick_signal = |ui: &mut egui::Ui, label: &str, sig: &mut NeuromodSignal| -> bool {
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label(label);
                                egui::ComboBox::from_id_salt(label)
                                    .selected_text(signal_label(*sig))
                                    .show_ui(ui, |ui| {
                                        let options = [
                                            NeuromodSignal::None,
                                            NeuromodSignal::RewardProxy,
                                            NeuromodSignal::PerceptualError,
                                            NeuromodSignal::WorldModelError,
                                            NeuromodSignal::OutputSpikes,
                                            NeuromodSignal::SensorySpikes,
                                            NeuromodSignal::HiddenSpikes,
                                            NeuromodSignal::Stability,
                                        ];
                                        for opt in options {
                                            if ui.selectable_value(sig, opt, signal_label(opt)).changed() {
                                                changed = true;
                                            }
                                        }
                                    });
                            });
                            changed
                        };
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_resonance_gain, 0.0..=1.0).text("Resonance gain"))
                            .on_hover_text("Scale pseudo-spontaneous reverberation from recent spiking; higher values sustain oscillatory loops.")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_resonance_decay, 0.0..=1.0).text("Resonance decay"))
                            .on_hover_text("EMA decay for resonance readout")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_baseline_dopamine, 0.0..=3.0).text("DA baseline"))
                            .on_hover_text("Baseline dopamine level")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_baseline_ach, 0.0..=3.0).text("ACh baseline"))
                            .on_hover_text("Baseline acetylcholine level")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_baseline_serotonin, 0.0..=3.0).text("5-HT baseline"))
                            .on_hover_text("Baseline serotonin level")
                            .changed();
                        nm_changed |= pick_signal(ui, "DA signal", &mut net.aarnn_neuromod_dopamine_signal);
                        nm_changed |= pick_signal(ui, "ACh signal", &mut net.aarnn_neuromod_ach_signal);
                        nm_changed |= pick_signal(ui, "5-HT signal", &mut net.aarnn_neuromod_serotonin_signal);
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_reward_proxy, 0.0..=1.0).text("Reward proxy"))
                            .on_hover_text("External reward proxy used when RewardProxy is selected")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_decay, 0.0..=0.5).text("Neuromod decay"))
                            .on_hover_text("EMA decay for neuromodulator state")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_error_gain, 0.0..=3.0).text("DA gain"))
                            .on_hover_text("Gain applied to dopamine signal")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_activity_gain, 0.0..=3.0).text("ACh gain"))
                            .on_hover_text("Gain applied to acetylcholine signal")
                            .changed();
                        nm_changed |= ui.add(egui::Slider::new(&mut net.aarnn_neuromod_stability_gain, 0.0..=3.0).text("5-HT gain"))
                            .on_hover_text("Gain applied to serotonin signal")
                            .changed();
                        if let Ok(r) = runner_arc.try_read() {
                            ui.label(format!(
                                "State: DA {:.2}, ACh {:.2}, 5-HT {:.2} | Resonance {:.2} | Reward {:.2}",
                                r.neuromod_dopamine,
                                r.neuromod_ach,
                                r.neuromod_serotonin,
                                r.resonance_level,
                                r.external_reward
                            ));
                        } else {
                            ui.label("State: busy");
                        }
                        if nm_changed {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        }
                    });
                    if ui.add(egui::Slider::new(&mut net.aarnn_layer_depth, 0..=5).text("Algorithm Realism Depth"))
                        .on_hover_text("0: Base, 1: Synaptic filters, 2: Adaptive thresholds, 3: Homeostasis/neuromod, 4+: Reserved for micro-detail").changed() {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                        }
                    ui.add_space(4.0);
                }

                ui.label("Izh preset").on_hover_text("Choose a canonical Izhikevich neuron type");
                egui::ComboBox::from_label("")
                    .selected_text(format!("{:?}", self.izh_preset))
                    .show_ui(ui, |ui|{
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::RS, "RS"); r.on_hover_text("Regular Spiking: tonic spiking; typical cortical pyramidal neurons");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::FS, "FS"); r.on_hover_text("Fast Spiking: narrow spikes, high-frequency firing; inhibitory interneurons");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::IB, "IB"); r.on_hover_text("Intrinsically Bursting: bursts of spikes followed by silence");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::CH, "CH"); r.on_hover_text("Chattering: high-frequency bursts (chattering cells)");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::LTS, "LTS"); r.on_hover_text("Low-Threshold Spiking: delayed tonic spiking at low currents");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::RZ, "RZ"); r.on_hover_text("Resonator: subthreshold oscillations and rebound spikes");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::TC, "TC"); r.on_hover_text("Thalamo-cortical: burst/tonic modes depending on input");
                        let r = ui.selectable_value(&mut self.izh_preset, IzhPreset::P, "P"); r.on_hover_text("Phasic Spiking: single spike at stimulus onset");
                    });
                ui.separator();
                ui.label("Learning rule").on_hover_text("Select synaptic plasticity update rule");
                ui.horizontal(|ui|{
                    ui.radio_value(&mut self.learning, LearningSel::Stdp, "STDP").on_hover_text("Spike-Timing Dependent Plasticity: Δw depends on pre/post spike timing");
                    ui.radio_value(&mut self.learning, LearningSel::Hebb, "Hebb").on_hover_text("Hebbian: 'cells that fire together wire together'");
                    ui.radio_value(&mut self.learning, LearningSel::Oja, "Oja").on_hover_text("Oja's rule: Hebbian with weight normalization/stabilization");
                    ui.radio_value(&mut self.learning, LearningSel::Aarnn, "AARNN").on_hover_text("AARNN learning (currently mirrors STDP; reserved for future extensions)");
                });
                let model_changed = self.neuron_model != prev_model_sel;
                let learning_changed = self.learning != prev_learning_sel;
                if model_changed {
                    let desired = if matches!(self.neuron_model, NeuronModelSel::Aarnn) {
                        NetworkLayout::Aarnn
                    } else {
                        NetworkLayout::Conventional
                    };
                    self.set_network_layout(desired, true);
                }
                // Apply algorithm changes
                match self.neuron_model {
                    NeuronModelSel::Lif => {
                        if runner_ready && !matches!(model_cloned, NeuronModel::Lif) {
                            // Preserve imported/connectome wiring when switching models.
                            // RecreateRunner rebuilds random connectivity and breaks mapping.
                            let _ = self.sim_tx.send(SimControl::SetModel(NeuronModel::Lif));
                            self.refresh_ui_buffers();
                        }
                    }
                    NeuronModelSel::Izh => {
                        let preset = match self.izh_preset { IzhPreset::RS=>"RS", IzhPreset::FS=>"FS", IzhPreset::IB=>"IB", IzhPreset::CH=>"CH", IzhPreset::LTS=>"LTS", IzhPreset::RZ=>"RZ", IzhPreset::TC=>"TC", IzhPreset::P=>"P" };
                        let dt = lif_cloned.dt;
                        let izh = IzhikevichParams::from_preset(preset, dt);
                        if runner_ready && model_cloned != NeuronModel::Izh(izh.clone()) {
                            // Preserve imported/connectome wiring when switching models.
                            // RecreateRunner rebuilds random connectivity and breaks mapping.
                            let _ = self.sim_tx.send(SimControl::SetModel(NeuronModel::Izh(izh)));
                            self.refresh_ui_buffers();
                        }
                    }
                    NeuronModelSel::Aarnn => {
                        if runner_ready && !matches!(model_cloned, NeuronModel::Aarnn) {
                            // Preserve imported/connectome wiring when switching models.
                            // RecreateRunner rebuilds random connectivity and breaks mapping.
                            let _ = self.sim_tx.send(SimControl::SetModel(NeuronModel::Aarnn));
                            self.apply_aarnn_bio_defaults();
                            self.aarnn_defaults_applied = true;
                            // self.apply_aarnn_bio_defaults already calls refresh_ui_buffers
                        }
                        // When using AARNN model, force AARNN learning selection only on explicit model change
                        if runner_ready && model_changed && self.learning != LearningSel::Aarnn {
                            self.learning = LearningSel::Aarnn;
                            let _ = self.sim_tx.send(SimControl::SetLearning(Learning::Aarnn));
                        }
                        // AARNN inherent components: only auto-enable when user just switched to AARNN
                        if runner_ready && model_changed {
                            let mut changed_auto = false;
                            #[cfg(feature = "growth3d")]
                            {
                                if !self.local_net.growth_enabled {
                                    self.local_net.growth_enabled = true;
                                    changed_auto = true;
                                }
                            }
                            // Delays (AARNN path)
                            if !self.local_net.use_aarnn_delays {
                                self.local_net.use_aarnn_delays = true;
                                changed_auto = true;
                            }
                            // Provide sensible defaults without overriding user-changed values
                            if self.local_net.aarnn_velocity <= 0.0 {
                                self.local_net.aarnn_velocity = 10.0; // default reasonable velocity
                                changed_auto = true;
                            }
                            // Morphology (when compiled)
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            {
                                if !self.local_net.use_morphology {
                                    self.local_net.use_morphology = true;
                                    changed_auto = true;
                                }
                            }
                            if changed_auto {
                                let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                            }
                        }
                        // Expose Biological Realism parameters
                        ui.separator();
                        ui.label("Biological Realism");
                        // Offer a reset-to-defaults action
                        if ui.button("Reset Biological defaults").on_hover_text("Reapply human biologically plausible defaults for AARNN").clicked() {
                            self.apply_aarnn_bio_defaults();
                            self.aarnn_defaults_applied = true;
                        }
                        let mut need_apply = false;
                        let mut fpaa_changed = false;
                        ui.collapsing("FPAA Offload", |ui| {
                            fpaa_changed |= self.render_fpaa_controls(ui);
                        });
                        if fpaa_changed {
                            need_apply = true;
                        }
                        {
                            let net = &mut self.local_net;
                            let mut changed = false;
                            changed |= ui.add(egui::Slider::new(&mut net.aarnn_velocity, 0.1..=50.0).text("Default Velocity")).on_hover_text("Aggregate default velocity (used if per-segment velocities are zero)").changed();
                            changed |= ui.checkbox(&mut net.use_aarnn_delays, "Use Conduction Delays").on_hover_text("Apply velocity-based discrete delays proportional to connection length (growth3d)").changed();

                            // Per-segment controls (exact AARNN conduction)
                            ui.collapsing("Detailed Conduction Physics", |ui| {
                                changed |= ui.add(egui::Slider::new(&mut net.axon_velocity, 0.0..=100.0).text("Axon velocity")).on_hover_text("If > 0, overrides default velocity for axon segments").changed();
                                changed |= ui.add(egui::Slider::new(&mut net.dend_velocity, 0.0..=100.0).text("Dend velocity")).on_hover_text("If > 0, overrides default velocity for dendrite segments").changed();
                                changed |= ui.add(egui::Slider::new(&mut net.bouton_latency_ms, 0.0..=20.0).text("Bouton latency (ms)"))
                                    .on_hover_text("Fixed extra latency added at synaptic boutons").changed();
                                changed |= ui.add(egui::Slider::new(&mut net.bouton_jitter_ms, 0.0..=10.0).text("Bouton jitter (ms)"))
                                    .on_hover_text("Uniform +/- jitter applied to bouton latency per event").changed();
                            });
                            ui.collapsing("AARNN Bio Dynamics", |ui| {
                                ui.group(|ui| {
                                    ui.label("Short-Term Plasticity (STP)");
                                    changed |= ui.checkbox(&mut net.aarnn_bio.stp_enabled, "Enable STP")
                                        .on_hover_text("Short-term plasticity on presynaptic spikes").changed();
                                    ui.add_enabled_ui(net.aarnn_bio.stp_enabled, |ui| {
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.stp_u, 0.0..=1.0).text("STP U")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.stp_tau_rec_ms, 10.0..=5000.0).text("STP tau_rec (ms)")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.stp_tau_facil_ms, 10.0..=2000.0).text("STP tau_facil (ms)")).changed();
                                    });
                                });
                                ui.separator();
                                ui.group(|ui| {
                                    ui.label("Synaptic Filtering");
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.ampa_tau_ms, 1.0..=50.0).text("AMPA tau (ms)")).changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.nmda_tau_ms, 10.0..=300.0).text("NMDA tau (ms)")).changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.gaba_tau_ms, 1.0..=50.0).text("GABA tau (ms)")).changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.nmda_ratio, 0.0..=1.0).text("NMDA ratio")).changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.synaptic_gain, 0.1..=5.0).text("Synaptic gain")).changed();
                                });
                                ui.separator();
                                ui.group(|ui| {
                                    ui.label("Threshold & Refractory");
                                    changed |= ui.checkbox(&mut net.aarnn_bio.adaptive_threshold_enabled, "Adaptive threshold").changed();
                                    ui.add_enabled_ui(net.aarnn_bio.adaptive_threshold_enabled, |ui| {
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.adaptive_threshold_tau_ms, 10.0..=1000.0).text("Threshold tau (ms)")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.adaptive_threshold_increment, 0.0..=5.0).text("Threshold increment")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.adaptive_threshold_min, -5.0..=0.0).text("Threshold min")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.adaptive_threshold_max, 0.0..=10.0).text("Threshold max")).changed();
                                    });
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.izh_refractory_ms, 0.0..=10.0).text("Izh refractory (ms)")).changed();
                                });
                                ui.separator();
                                ui.group(|ui| {
                                    ui.label("Homeostasis");
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.homeostasis_target_rate_hz, 0.0..=20.0).text("Homeostasis target (Hz)")).changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.homeostasis_tau_ms, 100.0..=10000.0).text("Homeostasis tau (ms)")).changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.homeostasis_gain, 0.0..=5.0).text("Homeostasis gain")).changed();
                                });
                                ui.separator();
                                ui.group(|ui| {
                                    ui.label("Neuromodulation");
                                    changed |= ui.checkbox(&mut net.aarnn_bio.neuromodulation_enabled, "Enable neuromodulation").changed();
                                    ui.add_enabled_ui(net.aarnn_bio.neuromodulation_enabled, |ui| {
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.dopamine_gain, 0.1..=3.0).text("Dopamine gain")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.acetylcholine_gain, 0.1..=3.0).text("ACh gain")).changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.serotonin_gain, 0.1..=3.0).text("Serotonin gain")).changed();
                                    });
                                });
                                ui.separator();
                                ui.group(|ui| {
                                    ui.label("Dendritic Nonlinearity");
                                    changed |= ui.checkbox(&mut net.aarnn_bio.dendritic_active_enabled, "Enable active dendrites")
                                        .on_hover_text("Adds calcium/plateau-like dendritic compartment integration for hidden neurons").changed();
                                    ui.add_enabled_ui(net.aarnn_bio.dendritic_active_enabled, |ui| {
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.dendritic_ca_tau_ms, 10.0..=1000.0).text("Dendritic Ca tau (ms)"))
                                            .on_hover_text("Time constant for dendritic calcium integration").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.dendritic_plateau_tau_ms, 20.0..=2000.0).text("Plateau tau (ms)"))
                                            .on_hover_text("Decay time constant of dendritic plateau state").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.dendritic_ca_influx_gain, 0.0..=1.0).text("Ca influx gain"))
                                            .on_hover_text("How strongly excitatory drive enters dendritic calcium state").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.dendritic_plateau_threshold, 0.0..=5.0).text("Plateau threshold"))
                                            .on_hover_text("Calcium level needed to recruit nonlinear dendritic boost").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_bio.dendritic_plateau_gain, 0.0..=2.0).text("Plateau gain"))
                                            .on_hover_text("Maximum dendritic multiplicative gain contribution").changed();
                                    });
                                });
                                ui.separator();
                                ui.group(|ui| {
                                    ui.label("Advanced Bio Interactions");
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_inhibitory_fraction, 0.0..=0.8).text("Inhibitory fraction"))
                                        .on_hover_text("Fraction of presynaptic neurons treated as inhibitory for Dale-style sign constraints").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_dale_strictness, 0.0..=1.0).text("Dale strictness"))
                                        .on_hover_text("0 disables Dale enforcement; 1 enforces strict fixed-sign output per presynaptic neuron").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_gap_junction_strength, 0.0..=0.2).text("Gap junction strength"))
                                        .on_hover_text("Electrical coupling term that nudges same-layer membrane potentials toward their local mean").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_gap_junction_radius, 0.0..=0.5).text("Gap junction radius"))
                                        .on_hover_text("Locality radius for gap-junction coupling in normalized space; 0 falls back to global mean coupling").changed();
                                    changed |= ui.checkbox(&mut net.aarnn_gap_junction_inhibitory_only, "Gap junctions inhibitory-only")
                                        .on_hover_text("Restrict electrical coupling to inhibitory/interneuron-like neuron types").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_nmda_voltage_sensitivity, 0.0..=0.2).text("NMDA voltage sensitivity"))
                                        .on_hover_text("Voltage-dependent NMDA gating strength (0 disables voltage gating)").changed();
                                    changed |= ui.checkbox(&mut net.volume_transmission_enabled, "Enable volume transmission")
                                        .on_hover_text("Enable local neuromodulator diffusion-like gain fields around active neuromodulatory neurons").changed();
                                    ui.add_enabled_ui(net.volume_transmission_enabled, |ui| {
                                        changed |= ui.add(egui::Slider::new(&mut net.volume_transmission_radius, 0.05..=1.0).text("Volume radius"))
                                            .on_hover_text("Spatial radius of neuromodulator field spread").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.volume_transmission_strength, 0.0..=1.0).text("Volume strength"))
                                            .on_hover_text("Gain applied by local neuromodulator field to hidden-layer input current").changed();
                                    });
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_triplet_ltp_gain, 0.0..=2.0).text("Triplet LTP gain"))
                                        .on_hover_text("Scales an activity-based potentiation term in AARNN/STDP learning-rate modulation").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_triplet_ltd_gain, 0.0..=2.0).text("Triplet LTD gain"))
                                        .on_hover_text("Scales an activity-based depression term in AARNN/STDP learning-rate modulation").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_synaptic_scaling_strength, 0.0..=0.2).text("Synaptic scaling strength"))
                                        .on_hover_text("Homeostatic row-wise incoming-weight scaling strength applied after plastic updates").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_synaptic_scaling_target, 0.1..=5.0).text("Synaptic scaling target"))
                                        .on_hover_text("Target summed absolute incoming weight per postsynaptic neuron for scaling").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_distance_attenuation_per_unit, 0.0..=2.0).text("Distance attenuation"))
                                        .on_hover_text("Exponential attenuation of transmitted signals based on morphology path length").changed();
                                    changed |= ui.add(egui::Slider::new(&mut net.aarnn_release_prob_heterogeneity, 0.0..=1.0).text("Release heterogeneity"))
                                        .on_hover_text("Per-synapse variation around baseline release probability `p_release_default`").changed();
                                    changed |= ui.checkbox(&mut net.aarnn_myelination_enabled, "Enable myelination dynamics")
                                        .on_hover_text("Activity-dependent myelination/demyelination that modulates conduction delay").changed();
                                    ui.add_enabled_ui(net.aarnn_myelination_enabled, |ui| {
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_myelination_rate, 0.0..=0.02).text("Myelination rate"))
                                            .on_hover_text("Growth rate of myelin for sufficiently active synapses").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_demyelination_rate, 0.0..=0.02).text("Demyelination rate"))
                                            .on_hover_text("Decay rate of myelin for underused or metabolically stressed pathways").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_myelination_activity_target, 0.0..=1.0).text("Myelination target"))
                                            .on_hover_text("Activity threshold above which myelin tends to increase").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_myelin_min_conduction_gain, 0.2..=2.0).text("Myelin min gain"))
                                            .on_hover_text("Conduction factor at low myelin (values <1 slow conduction)").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_myelin_max_conduction_gain, 0.5..=4.0).text("Myelin max gain"))
                                            .on_hover_text("Conduction factor at high myelin (higher values speed conduction)").changed();
                                        changed |= ui.add(egui::Slider::new(&mut net.aarnn_myelin_initial, 0.0..=1.0).text("Initial myelin"))
                                            .on_hover_text("Initial myelin state assigned to newly formed synapses").changed();
                                    });
                                });
                            });

                            if changed { need_apply = true; }
                        }
                        if need_apply {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                            self.status = "Biological parameters updated".into();
                        }
                        #[cfg(feature = "morpho")]
                        {
                            ui.separator();
                            let net = &mut self.local_net;
                            let mut changed = false;
                            changed |= ui.checkbox(&mut net.use_morphology, "Use morphology")
                                .on_hover_text("Enable soma/axon/dendrite/synapse data model and overlays.").changed();
                            changed |= ui.add(egui::Slider::new(&mut net.p_release_default, 0.0..=1.0).text("Release p"))
                                .on_hover_text("Baseline synaptic release probability when morphology behavioral path is enabled").changed();
                            if changed {
                                let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                            }
                        }
                    }
                }
                let new_learning = match self.learning { LearningSel::Stdp=>Learning::Stdp, LearningSel::Hebb=>Learning::Hebb, LearningSel::Oja=>Learning::Oja, LearningSel::Aarnn=>Learning::Aarnn };
                if runner_ready {
                    if learning_cloned != new_learning {
                        let _ = self.sim_tx.send(SimControl::SetLearning(new_learning));
                    }
                    // If AARNN learning is selected, only auto-enable inherent components when the user changes the selection.
                    if matches!(self.learning, LearningSel::Aarnn) && learning_changed {
                        let mut changed_auto = false;
                        #[cfg(feature = "growth3d")]
                        {
                            if !self.local_net.growth_enabled {
                                self.local_net.growth_enabled = true;
                                changed_auto = true;
                            }
                        }
                        if !self.local_net.use_aarnn_delays {
                            self.local_net.use_aarnn_delays = true;
                            changed_auto = true;
                        }
                        if self.local_net.aarnn_velocity <= 0.0 {
                            self.local_net.aarnn_velocity = 10.0;
                            changed_auto = true;
                        }
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            if !self.local_net.use_morphology {
                                self.local_net.use_morphology = true;
                                changed_auto = true;
                            }
                        }
                        if changed_auto {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        }
                    }
                } else {
                    ui.label("Simulation busy; model changes paused.");
                }
                ui.separator();
                ui.separator();
                });
                ui.separator();
                ui.collapsing("Topology / Morphology", |ui| {

                if ui.add(egui::Slider::new(&mut self.sensory_count, 10..=200).text("Sensory neurons")).on_hover_text("Number of sensory input neurons").changed() {
                    let _ = self.sim_tx.send(SimControl::ResizeSensory(self.sensory_count));
                    self.status = format!("Sensory resized to {}", self.sensory_count);
                    self.smoothed_equalizer_values.clear();
                }
                if ui
                    .add(egui::Slider::new(&mut self.output_count, 1..=200).text("Output neurons"))
                    .on_hover_text("Number of output neurons")
                    .changed()
                {
                    let _ = self.sim_tx.send(SimControl::ResizeOutput(self.output_count));
                    self.status = format!("Output resized to {}", self.output_count);
                    self.raster_outputs.clear();
                }
                ui.horizontal(|ui| {
                    ui.label("Sensory target layer:").on_hover_text("Hidden layer index receiving sensory input. Index 0 is the first hidden layer.");
                    let mut has_s = self.local_net.sensory_target_layer.is_some();
                    if ui.checkbox(&mut has_s, "").on_hover_text("Override default mapping").changed() {
                        if has_s { self.local_net.sensory_target_layer = Some(0); }
                        else { self.local_net.sensory_target_layer = None; }
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                    }
                    if let Some(mut l) = self.local_net.sensory_target_layer {
                        if ui.add(egui::DragValue::new(&mut l).range(0..=self.local_net.num_hidden_layers.saturating_sub(1))).changed() {
                            self.local_net.sensory_target_layer = Some(l);
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        }
                    } else {
                        ui.label("(Default)").on_hover_text("Standard models use H0; AARNN uses H1");
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Output source layer:").on_hover_text("Hidden layer index feeding output neurons.");
                    let mut has_o = self.local_net.output_source_layer.is_some();
                    if ui.checkbox(&mut has_o, "").on_hover_text("Override default mapping").changed() {
                        if has_o { self.local_net.output_source_layer = Some(self.local_net.num_hidden_layers.saturating_sub(1)); }
                        else { self.local_net.output_source_layer = None; }
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                    }
                    if let Some(mut l) = self.local_net.output_source_layer {
                        if ui.add(egui::DragValue::new(&mut l).range(0..=self.local_net.num_hidden_layers.saturating_sub(1))).changed() {
                            self.local_net.output_source_layer = Some(l);
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        }
                    } else {
                        ui.label("(Default)").on_hover_text("Standard models use H_last; AARNN uses H4");
                    }
                });
                ui.separator();
                // Topology Growth controls
                #[cfg(feature = "growth3d")]
                ui.collapsing("Topology Evolution (3D)", |ui| {
                    ui.checkbox(&mut self.show_region_labels, "Show region labels").on_hover_text("Display name of clumping regions at their centers");
                    if ui.checkbox(&mut self.growth_enabled, "Enable growth 3D topology").on_hover_text("Start with 1×1 hidden layer and grow neurons/layers dynamically").changed() {
                        // Recreate runner with updated growth flag
                        let mut net = self.local_net.clone();
                        net.growth_enabled = self.growth_enabled;
                        let _ = self.sim_tx.send(SimControl::RecreateRunner(
                            lif_cloned.clone(),
                            stdp_cloned.clone(),
                            net.clone(),
                            model_cloned,
                            learning_cloned,
                        ));
                        self.local_net = net.clone();
                        // reset cached positions/activities to match new sizes
                        self.sensory_positions.clear();
                        let num_l = net.num_hidden_layers;
                        let num_s = net.num_sensory_neurons;
                        let num_h = net.num_hidden_per_layer_initial;
                        let num_o = net.num_output_neurons;
                        self.hidden_positions = vec![vec![]; num_l];
                        self.output_positions.clear();
                        self.reset_topology_pid_states();
                        self.sensory_activity = vec![0.0; num_s];
                        self.hidden_activity = (0..num_l).map(|_| vec![0.0; num_h]).collect();
                        self.output_activity = vec![0.0; num_o];

                        let num_l2 = net.num_hidden_layers;
                        let num_h2 = net.num_hidden_per_layer_initial;
                        let num_s2 = net.num_sensory_neurons;
                        self.previous_hidden_spikes = (0..num_l2).map(|_| vec![0; num_h2]).collect();
                        self.last_sensory_spikes = vec![0; num_s2];
                        self.status = if self.growth_enabled { "Growth enabled".into() } else { "Growth disabled".into() };
                    }

                    // Live parameters (apply immediately to runner.net)
                    ui.separator();
                    ui.label("Parameters").on_hover_text("Tune growth behavior at runtime");
                    let net = &mut self.local_net;
                    let mut changed = false;
                    changed |= ui.add(egui::Slider::new(&mut net.saturation_threshold, 0.0..=2.0).text("Saturation threshold")).on_hover_text("EMA spike rate needed to trigger growth").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.saturation_window_ms, 20.0..=10000.0).text("Window (ms)"))
                        .on_hover_text("Time constant for EMA firing rate").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.growth_cooldown_ms, 0.0..=10000.0).text("Cooldown (ms)"))
                        .on_hover_text("Minimum time between spawns for the same neuron").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.spawn_radius, 0.01..=2.0).text("Spawn radius"))
                        .on_hover_text("Distance for placing a new neuron near its parent (normalized units)").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.migrate_in_prob, 0.0..=1.0).text("Migrate inputs p"))
                        .on_hover_text("Probability to split an incoming weight to the new neuron").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.migrate_out_prob, 0.0..=1.0).text("Migrate outputs p"))
                        .on_hover_text("Probability to split an outgoing weight to the new neuron").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.new_edge_prob, 0.0..=1.0).text("New edge p"))
                        .on_hover_text("Chance to add extra proximity-based edges (future step)").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.layer_split_threshold, 1..=256).text("Layer split @ size"))
                        .on_hover_text("When a layer reaches this size, the next hidden layer may be created (future step)").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.max_layers, 1..=10).text("Max layers"))
                        .on_hover_text("Upper bound on total hidden layers").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.global_growth_cooldown_ms, 0.0..=5000.0).text("Global cooldown (ms)"))
                        .on_hover_text("Minimum time between any two growth events globally").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.proximity_degree_cap, 0..=64).text("Proximity degree cap"))
                        .on_hover_text("Maximum number of extra proximity-biased edges on spawn").changed();
                    changed |= ui.add(egui::Slider::new(&mut net.max_sensory_connections, 1..=128).text("Max sensory conn"))
                        .on_hover_text("Maximum number of connections allowed per sensory neuron").changed();

                    if changed {
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        self.status = "Growth params updated".into();
                    }

                    ui.separator();
                    // Readouts
                    #[cfg(feature = "growth3d")]
                    {
                        if let Ok(r) = self.runner.try_read() {
                            let counts: Vec<usize> = (0..r.net.num_hidden_layers).map(|l| r.layer_size(l)).collect();
                            let counts_str = if counts.is_empty() {
                                "0".to_string()
                            } else {
                                counts.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
                            };
                            ui.label(format!("Layer sizes: [{}]", counts_str));
                            let conn_counts = r.connection_counts();
                            let conn_str = if conn_counts.is_empty() {
                                "0".to_string()
                            } else {
                                conn_counts.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
                            };
                            ui.label(format!("Layer connections: [{}]", conn_str));
                            let out_conn = r.output_connection_count();
                            ui.label(format!("Output connections: {}", out_conn));
                        } else {
                            ui.label("Layer sizes: (busy)");
                            ui.label("Layer connections: (busy)");
                            ui.label("Output connections: (busy)");
                        }
                        // Longterm connection stats
                        ui.label(format!(
                            "Longterm connections: {} / {} ({:.2}%)",
                            self.longterm_conn,
                            self.total_conn,
                            if self.total_conn > 0 { 100.0 * (self.longterm_conn as f64) / (self.total_conn as f64) } else { 0.0 }
                        ));
                    }
                });

                // Morphological Growth controls
                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                ui.collapsing("Morphological Evolution (Physical)", |ui| {
                    let total_conn: usize = self.runner.try_read()
                        .map(|r| r.connection_counts().iter().sum::<usize>() + r.output_connection_count())
                        .unwrap_or(0);
                    let net = &mut self.local_net;
                    let mut changed = false;
                    let mut growth_params_changed = false;
                    let mut geom_changed = false;
                    let mut clumping_design_changed = false;

                    changed |= ui.checkbox(&mut net.morpho_growth_enabled, "Enable physical growth")
                        .on_hover_text("Allow dendrites to sprout and grow towards synaptic energy density, forming new connections on axon contact.").changed();

                    ui.add_enabled_ui(net.morpho_growth_enabled, |ui| {
                        // Intelligent Heuristic Section
                        ui.group(|ui| {
                            ui.label("Heuristic Growth Optimization");
                            if ui.button("🚀 Boost Connectivity").on_hover_text("Automatically adjust parameters to promote faster connection growth and better stability (Morphological + Topological).").clicked() {
                                // "Low" is defined as fewer than 5 connections total
                                if total_conn < 5 {
                                    self.boost_connectivity_count += 1;
                                } else {
                                    self.boost_connectivity_count = 1;
                                }

                                // Intelligent heuristic: progressively more aggressive if no/low growth
                                let phase = self.boost_connectivity_count as f32;

                                // --- Morphological (Physical) ---
                                net.energy_attraction_radius = (0.4 + 0.2 * phase).min(3.0);
                                net.energy_kernel_k = (2.0 - 0.3 * phase).max(0.01);
                                net.dendrite_sprout_prob = (0.02 + 0.05 * phase).min(1.0);
                                net.aarnn_ambient_energy_level = (0.05 + 0.05 * phase).min(1.0);
                                net.axon_contact_dist = (0.04 + 0.05 * phase).min(1.0);
                                net.component_decay_rate = (0.01 + 0.0005 * phase).min(1.0);
                                net.synaptic_stabilization_strength = (0.05 + 0.05 * phase).min(0.5);

                                net.trunk_growth_rate = (0.005 + 0.005 * phase).min(0.5);
                                net.branch_growth_rate = (0.02 + 0.02 * phase).min(1.0);
                                net.bouton_growth_rate = (0.1 + 0.05 * phase).min(2.0);
                                net.max_segment_length = (5.0 + 0.1 * phase).min(5.0);

                                net.spatial_clumping_strength = (0.005 * phase).min(0.2);
                                net.density_target = (0.05 + 0.05 * phase).min(1.0);

                                // --- Topological (Network Structure) ---
                                net.growth_enabled = true;
                                self.growth_enabled = true; // Sync UI state
                                net.saturation_threshold = (0.5 - 0.1 * phase).max(0.001);
                                net.growth_cooldown_ms = (1000.0 - 200.0 * phase).max(5.0);
                                net.global_growth_cooldown_ms = (500.0 - 100.0 * phase).max(0.0);
                                net.spawn_radius = (0.2 + 0.1 * phase).min(2.0);
                                net.new_edge_prob = (0.1 + 0.1 * phase).min(1.0);
                                net.proximity_degree_cap = (4 + phase as usize * 2).min(64);
                                net.migrate_in_prob = (0.1 * phase).min(1.0);
                                net.migrate_out_prob = (0.1 * phase).min(1.0);
                                net.max_sensory_connections = (12 + phase as usize * 6).min(128);
                                net.layer_split_threshold = (32.0 - phase * 4.0).max(2.0) as usize;

                                // --- Structural Plasticity ---
                                net.spontaneous_neuron_interval_ms = (600.0 - 150.0 * phase).max(20.0);
                                net.neuron_removal_delay_ms = (1500.0 + 1000.0 * phase).min(180000.0);
                                net.synaptic_energy_window_ms = (1000.0 + 1000.0 * phase).min(30000.0);

                                self.status = format!("Connectivity boosted (Phase {})", self.boost_connectivity_count);
                                changed = true;
                            }

                            // Indicators
                            let topo_pot = (2.0f32 - net.saturation_threshold).clamp(0.0, 2.0) / 2.0;
                            let morph_pot = (net.energy_attraction_radius * net.dendrite_sprout_prob * net.axon_contact_dist * (1.0 + net.aarnn_ambient_energy_level) * 200.0).min(1.0);
                            let growth_potential = (topo_pot * 0.3 + morph_pot * 0.7).min(1.0);
                            ui.label("Growth potential:");
                            ui.add(egui::ProgressBar::new(growth_potential).text(format!("{:.1}%", growth_potential * 100.0)));
                            if growth_potential < 0.2 {
                                ui.colored_label(egui::Color32::YELLOW, "⚠ Growth potential is low. Increase radius, prob, or contact distance.");
                            } else if total_conn >= 5 && growth_potential > 0.8 {
                                ui.colored_label(egui::Color32::GREEN, "✔ Optimal growth parameters identified.");
                            } else if total_conn > 0 {
                                ui.colored_label(egui::Color32::GREEN, "✔ Connections are forming. Parameters are effective.");
                            } else if growth_potential > 0.8 {
                                ui.colored_label(egui::Color32::LIGHT_BLUE, "ℹ High growth potential. Waiting for initial connections...");
                            } else {
                                ui.colored_label(egui::Color32::LIGHT_GRAY, "Exploring growth parameters...");
                            }
                        });

                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.synaptic_energy_window_ms, 100.0..=30000.0).text("Energy window (ms)")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.energy_attraction_radius, 0.05..=5.0).text("Attraction radius")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.energy_kernel_k, 0.01..=10.0).text("Energy kernel k")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.dendrite_sprout_prob, 0.0..=2.0).text("Sprout prob")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.aarnn_ambient_energy_level, 0.0..=1.0).text("Ambient energy level"))
                            .on_hover_text("Increases background synaptic energy within the skull, causing spontaneous spikes and promoting exploratory growth.")
                            .changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.axon_contact_dist, 0.005..=2.0).text("Contact dist")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.component_decay_rate, 0.01..=1.0).text("Component decay")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.synaptic_stabilization_strength, 0.0..=1.0).text("Synaptic stabilization"))
                            .on_hover_text("Amount by which a synapse's stimuli (structural stability) increases per spike. Higher values make active synapses harder to prune.")
                            .changed();

                        ui.separator();
                        ui.label("Synaptic Dynamics & Stability");
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.component_pruning_threshold, 0.001..=0.2).text("Pruning threshold"))
                            .on_hover_text("Threshold below which a synapse or segment is pruned. Lower values allow connections to survive longer with less activity.")
                            .changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.initial_synaptic_weight, 0.001..=0.5).text("Initial weight"))
                            .on_hover_text("Starting weight for newly formed synapses.")
                            .changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.synaptic_growth_threshold, 0.1..=0.9).text("Growth threshold"))
                            .on_hover_text("Stability level above which a component expands and below which it contracts.")
                            .changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.synaptic_consolidation_factor, 0.0..=1.0).text("Consolidation factor"))
                            .on_hover_text("Reduces the effective decay rate for established connections, rewarding stability.")
                            .changed();

                        ui.separator();
                        ui.label("Growth Rates & Constraints");
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.trunk_growth_rate, 0.0001..=0.5).text("Trunk growth rate")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.branch_growth_rate, 0.001..=1.0).text("Branch growth rate")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.bouton_growth_rate, 0.005..=2.0).text("Bouton growth rate")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.max_segment_length, 0.1..=2.0).text("Max segment length")).changed();

                        ui.separator();
                        ui.label("Geometry & Collision");
                        geom_changed |= ui.checkbox(&mut net.enforce_unique_geometry, "Enforce unique geometry")
                            .on_hover_text("Ensure no two morphology components share the same 3D coordinates and separate near-coincident segments.")
                            .changed();
                        geom_changed |= ui.add(egui::Slider::new(&mut net.seg_eps, 0.0005..=0.01).logarithmic(true).text("Segment epsilon"))
                            .on_hover_text("Minimum allowed 3D distance between any two connection segments (morphology post-process)")
                            .changed();
                        geom_changed |= ui.add(egui::Slider::new(&mut net.max_reroute_tries, 1..=16).text("Max reroute tries")).changed();
                        geom_changed |= ui.add(egui::Slider::new(&mut net.relax_iters, 0..=8).text("Relax iters"))
                            .on_hover_text("Micro-relaxation passes to spread very close points")
                            .changed();
                        geom_changed |= ui.add(egui::Slider::new(&mut net.relax_step, 0.0..=0.02).text("Relax step"))
                            .on_hover_text("Per-iteration maximum displacement for relaxation")
                            .changed();

                        ui.separator();
                        ui.label("Spatial Density & Skull smoothing");
                        ui.horizontal(|ui| {
                            ui.label("Clumping Design:").on_hover_text("Select a predefined brain layout and neuron type distribution.");
                            let prev_design = net.clumping_design;
                            egui::ComboBox::from_id_salt("clumping_design_combo")
                                .selected_text(net.clumping_design.to_str())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut net.clumping_design, ClumpingDesign::None, "None");
                                    ui.selectable_value(&mut net.clumping_design, ClumpingDesign::HumanBrain, "Human Brain");
                                    ui.selectable_value(&mut net.clumping_design, ClumpingDesign::FruitFly, "Fruit Fly (Adult)");
                                    ui.selectable_value(&mut net.clumping_design, ClumpingDesign::FruitFlyLarva, "Fruit Fly (Larva)");
                                    ui.selectable_value(&mut net.clumping_design, ClumpingDesign::ZebraFish, "Zebra Fish");
                                    ui.selectable_value(&mut net.clumping_design, ClumpingDesign::NematodeWorm, "Nematode Worm");
                                });
                            if net.clumping_design != prev_design {
                                apply_clumping_design(net, net.clumping_design);
                                growth_params_changed = true;
                                // We'll trigger a full reset for design changes below
                                geom_changed = true;
                                clumping_design_changed = true;
                            }
                        });
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.spatial_repulsion_strength, 0.0..=0.5).text("Spatial repulsion")).changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.spatial_clumping_strength, 0.0..=0.5).text("Spatial clumping"))
                            .on_hover_text("Gravitational force pulling somas towards the center of mass to encourage proximity and connection growth.")
                            .changed();
                        growth_params_changed |= ui.checkbox(&mut net.columnar_enabled, "Enable columnar organization")
                            .on_hover_text("Encourage lateral alignment into vertical columns across layers (AARNN).")
                            .changed();
                        if net.columnar_enabled {
                            growth_params_changed |= ui.add(egui::Slider::new(&mut net.columnar_spacing, 0.05..=1.0).text("Column spacing"))
                                .on_hover_text("Lateral spacing between column centers")
                                .changed();
                            growth_params_changed |= ui.add(egui::Slider::new(&mut net.columnar_strength, 0.0..=0.1).text("Column strength"))
                                .on_hover_text("Strength of column attraction per step")
                                .changed();
                            growth_params_changed |= ui.add(egui::Slider::new(&mut net.columnar_jitter, 0.0..=1.0).text("Column jitter"))
                                .on_hover_text("Randomized offset of column centers")
                                .changed();
                        }
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.density_target, 0.01..=1.0).text("Density target"))
                            .on_hover_text("Target average density of hidden somas within the skull volume; lower values expand the skull.")
                            .changed();

                        ui.add_space(4.0);
                        ui.label("Skull PID Smoothing:");
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.skull_pid_kp, 0.001..=0.5).text("Kp")).on_hover_text("Smoothing speed (Proportional)").changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.skull_pid_ki, 0.0..=0.05).text("Ki")).on_hover_text("Integral gain").changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.skull_pid_kd, 0.0..=0.1).text("Kd")).on_hover_text("Derivative gain").changed();

                        ui.separator();
                        ui.label("Structural Plasticity");
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.spontaneous_neuron_interval_ms, 20.0..=10000.0).text("Spontaneous addition (ms)"))
                            .on_hover_text("Interval for adding a new hidden neuron if no growth occurs spontaneously.")
                            .changed();
                        growth_params_changed |= ui.add(egui::Slider::new(&mut net.neuron_removal_delay_ms, 500.0..=180000.0).text("Removal delay (ms)"))
                            .on_hover_text("Time a neuron remains active after losing its last synaptic connection.")
                            .changed();
                    });

                    if clumping_design_changed {
                        let _ = self.sim_tx.send(SimControl::RecreateRunner(
                            lif_cloned.clone(),
                            stdp_cloned.clone(),
                            self.local_net.clone(),
                            model_cloned,
                            learning_cloned,
                        ));
                        self.refresh_ui_buffers();
                        self.status = format!(
                            "Clumping design updated: {} ({} layers)",
                            self.local_net.clumping_design.to_str(),
                            self.local_net.num_hidden_layers
                        );
                    } else if changed {
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        let _ = self.sim_tx.send(SimControl::Reset);
                        self.refresh_ui_buffers();
                        self.status = "Network reset with new growth mode".into();
                    } else if geom_changed {
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        let _ = self.sim_tx.send(SimControl::Reset);
                        self.refresh_ui_buffers();
                        self.status = "Morphology geometry constraints updated (reset)".into();
                    } else if growth_params_changed {
                        let _ = self.sim_tx.send(SimControl::ApplyConfig(self.local_net.clone()));
                        self.status = "Morphological parameters updated (live)".into();
                    }
                });

                ui.collapsing("GA Search", |ui| {
                    ui.label("Automatically optimize growth and morphology parameters to maximize stable connection formation.");

                    if self.remote_only {
                        ui.label("Remote-only mode: local GA search disabled.");
                        return;
                    }

                    ui.add(egui::Slider::new(&mut self.ga_pop_size, 4..=100).text("Population size"));
                    ui.add(egui::Slider::new(&mut self.ga_generations, 1..=100).text("Generations"));
                    ui.add(egui::Slider::new(&mut self.ga_mutation_rate, 0.001..=0.3).text("Initial mutation rate"));
                    ui.add(egui::Slider::new(&mut self.ga_crossover_rate, 0.1..=0.95).text("Initial crossover rate"));
                    ui.checkbox(&mut self.ga_use_dk_bias, "Simulated Dunning-Kruger Fitness Bias");
                    ui.add(egui::Slider::new(&mut self.ga_sim_time_ms, 1000.0..=180000.0).text("Sim time (ms)"));

                    ui.horizontal(|ui| {
                        if self.ga_running {
                            if ui.button("⏹ Stop GA Search").clicked() {
                                if let Some(tx) = &self.ga_control_tx {
                                    let _ = tx.send(GAControl::Stop);
                                }
                                if let Some(ga) = &self.ga_search {
                                    let _ = ga.save_leaderboard("leaderboard.json");
                                }
                                crate::ga::ga_request_stop("ui_stop");
                                crate::ga::ga_clear_ramp_runtime_status();
                                crate::ga::ga_mark_clean();
                                self.ga_running = false;
                                // We keep self.ga_search so the leaderboard stays visible
                                self.ga_rx = None;
                                self.ga_control_tx = None;
                                self.status = "GA Search stopped".into();
                            }
                            let pause_label = if self.ga_paused { "▶ Resume GA" } else { "⏸ Pause GA" };
                            if ui.button(pause_label).clicked() {
                                self.ga_paused = !self.ga_paused;
                                if let Some(tx) = &self.ga_control_tx {
                                    let _ = tx.send(if self.ga_paused { GAControl::Pause } else { GAControl::Resume });
                                }
                            }
                        } else {
                            if ui.button("🚀 Start GA Search").clicked() {
                                self.reap_finished_ga_thread();
                                if self.ga_thread.is_some() {
                                    self.status = "GA worker is still shutting down; please wait.".into();
                                    return;
                                }
                                self.ga_panel_visible = true;
                                self.ga_running = true;
                                self.ga_best_fitness = 0.0;
                                self.ga_paused = false;
                                crate::ga::ga_reset_abort_reason();
                                crate::ga::ga_clear_ramp_runtime_status();
                                crate::ga::ga_mark_dirty();

                                let (tx, rx) = std::sync::mpsc::channel();
                                self.ga_rx = Some(rx);

                                let (ctrl_tx, ctrl_rx) = std::sync::mpsc::channel();
                                self.ga_control_tx = Some(ctrl_tx);

                                // Determine if we are restarting from a previous search
                                let mut is_restart = false;
                                let mut existing_leaderboard = Vec::new();
                                let base_cfg = net_cloned.clone();
                                if let Some(ga) = &self.ga_search {
                                    existing_leaderboard = ga.leaderboard.clone();
                                    if !ga.leaderboard.is_empty() {
                                        is_restart = true;
                                    }
                                }

                            let pop_size = self.ga_pop_size;
                            let sim_time = self.ga_sim_time_ms;
                            let mutation_rate = self.ga_mutation_rate;
                            let crossover_rate = self.ga_crossover_rate;
                            let use_dk_bias = self.ga_use_dk_bias;
                            let n_elite = 2;
                            let dist_node = self.distributed_node.clone();
                            let gens = self.ga_generations;
                            let runtime_handle = self.runtime_handle.clone();
                            crate::ga::ga_set_stall_timeout_secs(base_cfg.ga_stall_timeout_secs);

                                match std::thread::Builder::new().name("ga-ui".into()).spawn(move || {
                                    // Keep GA controller thread unpinned so worker budgeting and child pools
                                    // can see/use the full CPU set.
                                    let mut seed_rng = rand::rng();
                                    let mut rng = rand::rngs::StdRng::from_rng(&mut seed_rng);
                                    let mut ga = GASearch::new(pop_size.max(1), &base_cfg, &mut rng, dist_node, is_restart, existing_leaderboard);

                                    // Set initial self-adaptive rates from UI
                                    for ind in &mut ga.population {
                                        ind.mutation_rate = mutation_rate;
                                        ind.crossover_rate = crossover_rate;
                                    }

                                    let mut paused = false;
                                    let mut stop_requested = false;
                                    let mut ramp = crate::ga::GARampController::new(pop_size.max(1), sim_time);
                                    crate::ga::ga_clear_ramp_runtime_status();

                                    for gen_iter in 0..gens {
                                        // Check for control signals
                                        while let Ok(ctrl) = ctrl_rx.try_recv() {
                                            match ctrl {
                                                GAControl::Stop => {
                                                    stop_requested = true;
                                                    break;
                                                }
                                                GAControl::Pause => paused = true,
                                                GAControl::Resume => paused = false,
                                            }
                                        }
                                        if stop_requested {
                                            break;
                                        }

                                        if paused {
                                            std::thread::sleep(std::time::Duration::from_millis(100));
                                            // Still send current state to UI to keep it responsive
                                            if tx.send(ga.clone()).is_err() { break; }
                                            // Wait until unpaused
                                            continue;
                                        }

                                        let plan = ramp.generation_plan();
                                        crate::ga::ga_set_ramp_runtime(&plan, gen_iter);
                                        crate::ga::GARampController::apply_plan_overrides(&plan);
                                        ga.resize_population(plan.population_size, &base_cfg, &mut rng);
                                        let gen_seed: u64 = rand::random();
                                        runtime_handle.block_on(ga.evaluate_population(plan.sim_time_ms, gen_seed, &tx));
                                        let success = crate::ga::ga_abort_reason().is_none();
                                        ramp.note_generation_result(success);

                                        if tx.send(ga.clone()).is_err() { break; }

                                        if !crate::ga::ga_wait_for_generation_headroom() {
                                            let _ = tx.send(ga.clone());
                                            break;
                                        }
                                        if success {
                                            ga.evolve(n_elite, use_dk_bias, &mut rng);
                                        }
                                    }
                                    // Send final state after all generations complete
                                    let _ = tx.send(ga);
                                    crate::ga::ga_clear_ramp_runtime_status();
                                    crate::ga::ga_clear_eval_limits_override();
                                    crate::ga::ga_set_worker_limit_override(None);
                                    crate::ga::ga_mark_clean();
                                }) {
                                    Ok(handle) => {
                                        self.ga_thread = Some(handle);
                                        self.status = "GA Search started".into();
                                    }
                                    Err(e) => {
                                        self.ga_running = false;
                                        self.ga_rx = None;
                                        self.ga_control_tx = None;
                                        crate::ga::ga_clear_ramp_runtime_status();
                                        crate::ga::ga_mark_clean();
                                        self.status = format!("Failed to start GA thread: {}", e);
                                    }
                                }
                            }
                        }
                    });

                    let show_ga_panel = self.ga_panel_visible || self.ga_search.is_some();
                    if show_ga_panel {
                        if let Some(ga) = &self.ga_search {
                            ui.separator();
                            ui.label(format!("Generation: {}", ga.generation));
                            ui.label(format!("Best Fitness: {:.4}", ga.best_fitness));

                            if !ga.population.is_empty() {
                                let avg_mut: f64 = ga.population.iter().map(|ind| ind.mutation_rate).sum::<f64>() / ga.population.len() as f64;
                                let avg_cross: f64 = ga.population.iter().map(|ind| ind.crossover_rate).sum::<f64>() / ga.population.len() as f64;
                                ui.label(format!("Avg Mutation Rate: {:.4}", avg_mut));
                                ui.label(format!("Avg Crossover Rate: {:.4}", avg_cross));
                            }

                            ui.label(format!("GA Backend: {}", crate::ga::ga_backend_label()));
                            if let Some(label) = crate::ga::ga_affinity_label() {
                                ui.label(format!("GA Affinity: {}", label));
                            }
                            ui.label(format!("GA Active Evals: {}", crate::ga::ga_active_evals()));
                            if let Some(ramp) = crate::ga::ga_ramp_runtime_status() {
                                ui.label(ga_ramp_label(ramp.population_size, ramp.worker_cap, ramp.sim_time_ms));
                            } else if self.ga_running {
                                ui.label("GA Ramp: initializing...");
                            } else {
                                ui.label("GA Ramp: No");
                            }

                            let (ga_pacing, ga_pacing_reason) = crate::ga::ga_pacing_status();
                            if !ga_pacing {
                                self.ga_pacing_ack = false;
                            }
                            let (ga_temp_opt, ga_temp_warn, ga_temp_hot) = crate::ga::ga_temperature_status();
                            let pop_total = ga.population.len().max(1);
                            let progress = if pop_total > 0 {
                                if ga.current_eval_idx < pop_total {
                                    ga.current_eval_idx as f32 / pop_total as f32
                                } else {
                                    1.0
                                }
                            } else {
                                0.0
                            };
                            ui.horizontal(|ui| {
                                if self.ga_running {
                                    if ga.current_eval_idx < pop_total {
                                        ui.label(format!("Evaluating population: {}/{}", ga.current_eval_idx, pop_total));
                                        ui.spinner();
                                    } else if ga_pacing {
                                        let label = if ga_pacing_reason.is_empty() {
                                            "GA pacing active; waiting for system headroom.".to_string()
                                        } else {
                                            format!("GA pacing active ({}); waiting for system headroom.", ga_pacing_reason)
                                        };
                                        ui.label(label);
                                        ui.spinner();
                                    } else {
                                        ui.label("Generation complete. Preparing next...");
                                    }
                                } else if let Some(reason) = crate::ga::ga_abort_reason() {
                                    ui.label(format!("GA stopped ({})", reason));
                                } else {
                                    ui.label("GA Search not running.");
                                }
                            });

                            if ga.current_eval_idx < pop_total {
                                egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                                    for (i, ind) in ga.population.iter().enumerate() {
                                        if let Some(node) = &ind.evaluating_node {
                                            ui.horizontal(|ui| {
                                                ui.small(format!(" #{} ", i + 1));
                                                if ind.fitness == 0.0 {
                                                    if ga.inflight.contains(&i) {
                                                        ui.small("evaluating on ");
                                                        ui.colored_label(egui::Color32::LIGHT_BLUE, node);
                                                        ui.spinner();
                                                    } else {
                                                        ui.small("queued on ");
                                                        ui.colored_label(egui::Color32::LIGHT_GRAY, node);
                                                    }
                                                } else {
                                                    ui.small("finished on ");
                                                    ui.label(node);
                                                    ui.small(format!(" (fit: {:.4})", ind.fitness));
                                                }
                                            });
                                        }
                                    }
                                });
                            }
                            let mut bar = egui::ProgressBar::new(progress).show_percentage();
                            if let Some(temp) = ga_temp_opt {
                                let warn = ga_temp_warn.max(1.0);
                                let hot = ga_temp_hot.max(warn + 1.0);
                                let t = if temp <= warn {
                                    0.0
                                } else if temp >= hot {
                                    1.0
                                } else {
                                    (temp - warn) / (hot - warn)
                                };
                                let cold = egui::Color32::from_rgb(80, 180, 120);
                                let warm = egui::Color32::from_rgb(240, 180, 60);
                                let hotc = egui::Color32::from_rgb(240, 80, 60);
                                let lerp_u8 = |a: u8, b: u8, t: f32| -> u8 {
                                    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
                                };
                                let base = if t <= 0.5 {
                                    let tt = t * 2.0;
                                    egui::Color32::from_rgb(
                                        lerp_u8(cold.r(), warm.r(), tt),
                                        lerp_u8(cold.g(), warm.g(), tt),
                                        lerp_u8(cold.b(), warm.b(), tt),
                                    )
                                } else {
                                    let tt = (t - 0.5) * 2.0;
                                    egui::Color32::from_rgb(
                                        lerp_u8(warm.r(), hotc.r(), tt),
                                        lerp_u8(warm.g(), hotc.g(), tt),
                                        lerp_u8(warm.b(), hotc.b(), tt),
                                    )
                                };
                                bar = bar.fill(base).text(format!("{:.1}C", temp));
                            }
                            if ga_pacing && !self.ga_pacing_ack {
                                let flash_on = (ui.input(|i| i.time) * 2.0).fract() < 0.5;
                                if flash_on {
                                    bar = bar.fill(egui::Color32::from_rgb(255, 220, 120));
                                }
                            }
                            let bar_resp = ui.add(bar);
                            let click_resp = ui.interact(
                                bar_resp.rect,
                                bar_resp.id.with("ga-progress-click"),
                                egui::Sense::click(),
                            );
                            if ga_pacing && (bar_resp.clicked() || click_resp.clicked()) {
                                self.ga_pacing_ack = true;
                            }
                            if let Some(temp) = ga_temp_opt {
                                ui.label(format!("GA Temp: {:.1}C", temp));
                            }

                            if self.ga_paused {
                                ui.colored_label(egui::Color32::YELLOW, "GA Search is paused.");
                            }

                            ui.checkbox(&mut self.ga_live_preview, "👁 Use parameters currently being tested")
                                .on_hover_text("Update the displayed network with the parameters of the individual currently being evaluated.");
                        } else {
                            ui.separator();
                            ui.label("GA Search initializing...");
                            if let Some(ramp) = crate::ga::ga_ramp_runtime_status() {
                                ui.label(ga_ramp_label(ramp.population_size, ramp.worker_cap, ramp.sim_time_ms));
                            }
                            if self.ga_running {
                                if self.ga_paused {
                                    ui.colored_label(egui::Color32::YELLOW, "GA Search is paused.");
                                } else {
                                    let (ga_pacing, ga_pacing_reason) = crate::ga::ga_pacing_status();
                                    if ga_pacing {
                                        let label = if ga_pacing_reason.is_empty() {
                                            "GA pacing active; waiting for system headroom.".to_string()
                                        } else {
                                            format!("GA pacing active ({}); waiting for system headroom.", ga_pacing_reason)
                                        };
                                        ui.label(label);
                                        ui.spinner();
                                    } else {
                                        ui.label("GA Search starting...");
                                        ui.spinner();
                                    }
                                }
                            } else if let Some(reason) = crate::ga::ga_abort_reason() {
                                ui.label(format!("GA stopped ({})", reason));
                            } else {
                                ui.label("GA Search not running.");
                            }
                            let (temp_opt, temp_warn, temp_hot) = crate::ga::ga_temperature_status();
                            if let Some(temp) = temp_opt {
                                if temp >= temp_hot {
                                    ui.colored_label(
                                        egui::Color32::YELLOW,
                                        format!("Thermal gate: {:.1}C >= {:.1}C hot threshold.", temp, temp_hot),
                                    );
                                } else if temp >= temp_warn {
                                    ui.label(format!(
                                        "Thermal nearing limit: {:.1}C >= {:.1}C warn threshold.",
                                        temp, temp_warn
                                    ));
                                }
                            }
                        }
                    }

                    // Display leaderboard and best configuration even if not running
                    if let Some(ga) = &self.ga_search {
                        if !self.ga_running {
                            ui.separator();
                            ui.label(format!("Last Search Best Fitness: {:.4}", ga.best_fitness));
                        }

                        if !ga.leaderboard.is_empty() {
                            ui.separator();
                            ui.label("🏆 Leaderboard (Top 10)");
                            for (i, ind) in ga.leaderboard.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}. Fitness: {:.4}", i + 1, ind.fitness));
                                    if ui.button("👁 Preview").on_hover_text("Use these parameters in the main network display.").clicked() {
                                        let _ = self.sim_tx.send(SimControl::ApplyConfig(ind.config.clone()));
                                        self.status = format!("Previewing Leaderboard #{}", i + 1);
                                    }
                                });
                            }
                        }
                    }

                    if let Some(best) = &self.ga_search.as_ref().and_then(|ga| ga.best_config.clone()) {
                        if ui.button("✅ Apply Best Configuration").clicked() {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(best.clone()));
                            self.status = "Applied best GA configuration".into();
                        }
                    }
                });
                });
                ui.separator();
                ui.collapsing("I/O", |ui| {

                if ui.checkbox(&mut self.loop_feedback, "Loop feedback").on_hover_text("Route output spikes back to sensory via the feedback map").changed() {
                    let _ = self.sim_tx.send(SimControl::SetFeedback(self.loop_feedback));
                }
                ui.separator();
                ui.label("Input source").on_hover_text("Choose the sensory spike source");
                let prev_input_source = self.input_source;
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.input_source, InputSource::Random, "Random").on_hover_text("Per-sensor Bernoulli spikes with tunable probability");
                    ui.radio_value(&mut self.input_source, InputSource::Theta, "Theta").on_hover_text("Deterministic theta rhythm spikes (global oscillation)");
                    ui.radio_value(&mut self.input_source, InputSource::ExternalHttpAer, "HTTP/HTTPS AER")
                        .on_hover_text("Pull NDJSON AER frames from an HTTP/HTTPS stream and feed them into sensory spikes.");
                    ui.radio_value(&mut self.input_source, InputSource::AudioFile, "Audio File").on_hover_text("Decode audio file → spectral bands → probabilistic spikes");
                    ui.radio_value(&mut self.input_source, InputSource::Microphone, "Microphone").on_hover_text("Live mic capture → spectral bands → probabilistic spikes");
                    #[cfg(feature = "image_input")]
                    ui.radio_value(&mut self.input_source, InputSource::ImageFile, "Image").on_hover_text("Static picture → grayscale → downsample to sensory → spikes");
                    #[cfg(feature = "video_input")]
                    ui.radio_value(&mut self.input_source, InputSource::VideoFile, "Video").on_hover_text("Video file (.mp4) → grayscale → downsample → spikes");
                    #[cfg(feature = "webcam_input")]
                    ui.radio_value(&mut self.input_source, InputSource::Webcam, "Webcam").on_hover_text("Live camera → grayscale → downsample → spikes");
                    #[cfg(feature = "robot_io")]
                    ui.radio_value(&mut self.input_source, InputSource::ExternalIpc, "External (IPC)")
                        .on_hover_text("Receive sensory spikes via AER (preferred) or legacy float frames over Unix Domain Socket; floats are thresholded to spikes.");
                });
                if self.input_source != prev_input_source {
                    match self.input_source {
                        InputSource::Random => {
                            let n = net_cloned.num_sensory_neurons;
                            let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(n, self.random_spike_probability))));
                            self.mic_running = false;
                            #[cfg(feature = "webcam_input")]
                            { self.cam_running = false; }
                            self.status = "Input source set to Random".to_string();
                        }
                        InputSource::Theta => {
                            let n = net_cloned.num_sensory_neurons;
                            let dt_ms = lif_cloned.dt.max(0.001) as f32;
                            let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(ThetaProvider::new(
                                n,
                                net_cloned.theta_rhythm_hz,
                                net_cloned.theta_rhythm_duty,
                                net_cloned.theta_rhythm_phase_jitter,
                                dt_ms,
                            ))));
                            self.mic_running = false;
                            #[cfg(feature = "webcam_input")]
                            { self.cam_running = false; }
                            self.status = "Input source set to Theta".to_string();
                        }
                        InputSource::ExternalHttpAer => {
                            self.mic_running = false;
                            #[cfg(feature = "webcam_input")]
                            { self.cam_running = false; }
                            self.connect_http_aer_source(net_cloned.num_sensory_neurons);
                        }
                        _ => {}
                    }
                }
                match self.input_source {
                    InputSource::ExternalHttpAer => {
                        ui.label("External AER stream over HTTP/HTTPS");
                        ui.horizontal(|ui| {
                            ui.label("Source URL");
                            ui.text_edit_singleline(&mut self.http_aer_source_url)
                                .on_hover_text("Remote NDJSON stream URL. Each line can be JSON with aer_payload_hex/spike_indices, or raw AER hex.");
                        });
                        ui.horizontal(|ui| {
                            ui.label("AER base");
                            ui.add(
                                egui::DragValue::new(&mut self.http_aer_base)
                                    .range(0..=u32::MAX)
                                    .speed(1.0),
                            )
                            .on_hover_text("Default base address used to decode incoming AER payloads.");
                            if ui.button("Connect / Restart")
                                .on_hover_text("Start or restart the HTTP AER source stream with current settings.")
                                .clicked()
                            {
                                self.connect_http_aer_source(net_cloned.num_sensory_neurons);
                            }
                        });
                        let stream_stats = self.http_aer_status_snapshot();
                        let status_color = if stream_stats.connected {
                            egui::Color32::GREEN
                        } else if stream_stats.last_error.is_some() {
                            egui::Color32::RED
                        } else {
                            egui::Color32::GRAY
                        };
                        ui.horizontal(|ui| {
                            ui.label("Status:");
                            ui.colored_label(status_color, stream_stats.status_text.as_str());
                        });
                        ui.label(format!("Frames received: {}", stream_stats.frames_received));
                        if let Some(last_frame) = stream_stats.last_frame_time {
                            ui.label(format!("Last frame: {} ms ago", last_frame.elapsed().as_millis()));
                        }
                        if let Some(err) = stream_stats.last_error {
                            ui.colored_label(egui::Color32::LIGHT_RED, format!("Last error: {}", err));
                        }
                    }
                    InputSource::AudioFile => {
                        if ui.button("Choose File...").on_hover_text("Open an audio file (wav, flac, ogg, mp3)").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Audio", &["wav","flac","ogg","mp3"]).pick_file() {
                                match AudioFileProvider::from_path(&path, net_cloned.num_sensory_neurons) {
                                    Ok(p) => {
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(p)));
                                        self.mic_running = false;
                                        #[cfg(feature = "webcam_input")]
                                        { self.cam_running = false; }
                                        self.status = format!("Loaded file: {}", path.display());
                                        self.smoothed_equalizer_values.clear();
                                        self.show_equalizer = true;
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to load audio: {}", e);
                                        self.input_source = InputSource::Random;
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(net_cloned.num_sensory_neurons, self.random_spike_probability))));
                                    }
                                }
                            }
                        }
                    }
                    #[cfg(feature = "robot_io")]
                    InputSource::ExternalIpc => {
                        ui.label(format!("External (IPC) via Unix Domain Socket - ID: {}", self.brain_id));
                        #[cfg(unix)]
                        {
                            let conn = if self.ipc_connected { "Active" } else { "Disconnected" };
                            let color = if self.ipc_connected { egui::Color32::GREEN } else { egui::Color32::RED };
                            ui.horizontal(|ui| {
                                ui.label("Status:");
                                ui.colored_label(color, conn);
                            });
                            ui.label("See 'Robot I/O (IPC)' panel for configuration and diagnostics.");
                        }
                        #[cfg(not(unix))]
                        {
                            ui.colored_label(egui::Color32::YELLOW, "IPC over UDS is only supported on Unix platforms");
                        }
                    }
                    #[cfg(feature = "image_input")]
                    InputSource::ImageFile => {
                        if ui.button("Choose Image...").on_hover_text("Open an image (png, jpg, bmp)").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png","jpg","jpeg","bmp"]).pick_file() {
                                match ImageFileProvider::from_path(&path, net_cloned.num_sensory_neurons) {
                                    Ok(p) => {
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(p)));
                                        self.mic_running = false;
                                        #[cfg(feature = "webcam_input")]
                                        { self.cam_running = false; }
                                        self.status = format!("Loaded image: {}", path.display());
                                        self.smoothed_equalizer_values.clear();
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to load image: {}", e);
                                        self.input_source = InputSource::Random;
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(net_cloned.num_sensory_neurons, self.random_spike_probability))));
                                    }
                                }
                            }
                        }
                    }
                    #[cfg(feature = "video_input")]
                    InputSource::VideoFile => {
                        if ui.button("Choose Video...").on_hover_text("Open a video file (mp4, avi)").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Video", &["mp4","avi","mov","mkv"]).pick_file() {
                                match VideoFileProvider::from_path(&path, net_cloned.num_sensory_neurons, true) {
                                    Ok(p) => {
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(p)));
                                        self.mic_running = false;
                                        #[cfg(feature = "webcam_input")]
                                        { self.cam_running = false; }
                                        self.status = format!("Loaded video: {}", path.display());
                                        self.smoothed_equalizer_values.clear();
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to open video: {}", e);
                                        self.input_source = InputSource::Random;
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(net_cloned.num_sensory_neurons, self.random_spike_probability))));
                                    }
                                }
                            }
                        }
                    }
                    #[cfg(feature = "webcam_input")]
                    InputSource::Webcam => {
                        let label = if self.cam_running { "Stop Cam" } else { "Start Cam" };
                        if ui.button(label).clicked() {
                            if self.cam_running {
                                let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(net_cloned.num_sensory_neurons, self.random_spike_probability))));
                                self.cam_running = false;
                                self.status = "Webcam stopped".to_string();
                            } else {
                                match WebcamCaptureProvider::new(0, net_cloned.num_sensory_neurons) {
                                    Ok(p) => {
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(p)));
                                        self.mic_running = false;
                                        self.cam_running = true;
                                        self.status = "Webcam started".to_string();
                                        self.smoothed_equalizer_values.clear();
                                    }
                                    Err(e) => {
                                        self.status = format!("Webcam unavailable: {} — falling back to Random", e);
                                        self.input_source = InputSource::Random;
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(net_cloned.num_sensory_neurons, self.random_spike_probability))));
                                    }
                                }
                            }
                        }
                    }
                    InputSource::Microphone => {
                        let label = if self.mic_running { "Stop Mic" } else { "Start Mic" };
                        if ui.button(label).clicked() {
                            if self.mic_running {
                                let n = net_cloned.num_sensory_neurons;
                                let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(n, self.random_spike_probability))));
                                self.mic_running = false;
                                self.status = "Microphone stopped (Random fallback)".to_string();
                            } else {
                                let n = net_cloned.num_sensory_neurons;
                                match MicrophoneProvider::new(n) {
                                    Ok(p) => {
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(p)));
                                        self.mic_running = true;
                                        self.status = "Microphone started".to_string();
                                        self.smoothed_equalizer_values.clear();
                                        self.show_equalizer = true;
                                    }
                                    Err(e) => {
                                        self.status = format!("Mic unavailable: {} — falling back to Random", e);
                                        self.input_source = InputSource::Random;
                                        let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(n, self.random_spike_probability))));
                                    }
                                }
                            }
                        }
                    }
                    InputSource::Theta => {
                        let net = &mut self.local_net;
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            ui.label("Theta Hz");
                            changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_hz, 0.5..=12.0).text("Hz")).changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Duty");
                            changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_duty, 0.05..=0.9).text("duty")).changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Phase jitter");
                            changed |= ui.add(egui::Slider::new(&mut net.theta_rhythm_phase_jitter, 0.0..=1.0).text("jitter")).changed();
                        });
                        if changed {
                            let _ = self.sim_tx.send(SimControl::ApplyConfig(net.clone()));
                            let n = net_cloned.num_sensory_neurons;
                            let dt_ms = lif_cloned.dt.max(0.001) as f32;
                            let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(ThetaProvider::new(
                                n,
                                net.theta_rhythm_hz,
                                net.theta_rhythm_duty,
                                net.theta_rhythm_phase_jitter,
                                dt_ms,
                            ))));
                            self.status = format!("Theta input f={:.2}Hz duty={:.2}", net.theta_rhythm_hz, net.theta_rhythm_duty);
                        }
                        let steps = self.sim_step_counter.load(Ordering::Relaxed);
                        let spikes = self.sim_last_spike_count.load(Ordering::Relaxed);
                        let total = self.sim_last_spike_len.load(Ordering::Relaxed);
                        ui.label(format!("Sim steps: {}  last spikes: {}/{}", steps, spikes, total));
                    }
                    InputSource::Random => {
                        ui.horizontal(|ui| {
                            ui.label("Spike prob");
                            let changed = ui.add(egui::Slider::new(&mut self.random_spike_probability, 0.0..=1.0).text("p")).changed();
                            if changed {
                                let n = net_cloned.num_sensory_neurons;
                                let _ = self.sim_tx.send(SimControl::SetProvider(Box::new(RandomProvider::new(n, self.random_spike_probability))));
                                self.status = format!("Random input p={:.3}", self.random_spike_probability);
                            }
                        });
                        let steps = self.sim_step_counter.load(Ordering::Relaxed);
                        let spikes = self.sim_last_spike_count.load(Ordering::Relaxed);
                        let total = self.sim_last_spike_len.load(Ordering::Relaxed);
                        ui.label(format!("Sim steps: {}  last spikes: {}/{}", steps, spikes, total));
                    }
                }
                #[cfg(all(feature = "robot_io", unix))]
                ui.collapsing("Robot I/O (IPC)", |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Brain ID:");
                            ui.label(egui::RichText::new(&self.brain_id).strong());
                        });

                        ui.collapsing("Connection", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Socket:");
                                ui.text_edit_singleline(&mut self.ipc_sock_path)
                                    .on_hover_text("UDS path to bind and receive frames: AER (AER1) preferred, or legacy [f32 t_ms] + [S f32] + optional [f32 reward].");
                            });
                            if ui.button("Bind / Restart").on_hover_text("Bind the IPC server socket with current sizes and path").clicked() {
                                let (s, o) = if let Ok(r) = self.runner.try_read() {
                                    (r.net.num_sensory_neurons, r.net.num_output_neurons)
                                } else {
                                    (
                                        self.local_net.num_sensory_neurons,
                                        self.local_net.num_output_neurons,
                                    )
                                };
                                let _ = self.sim_tx.send(SimControl::BindIpc(self.ipc_sock_path.clone(), s, o));
                                self.status = format!("Requested IPC bind: {}", self.ipc_sock_path);
                            }
                            ui.separator();
                            let conn = if self.ipc_connected { "Yes" } else { "No" };
                            let color = if self.ipc_connected { egui::Color32::GREEN } else { egui::Color32::RED };
                            let peer = self.ipc_last_peer.as_deref().unwrap_or("none");
                            let age_ms = self.ipc_last_receive_time.map(|t| t.elapsed().as_millis() as i64).unwrap_or(-1);

                            ui.horizontal(|ui| { ui.label("Connected:"); ui.colored_label(color, conn); });
                            ui.label(format!("Peer: {}", peer));
                            ui.label(format!("Last frame: {} ms ago", age_ms));
                        });

                        #[cfg(all(feature = "robot_io", unix))]
                        ui.collapsing("Encoding", |ui| {
                            let mut io_changed = false;
                            {
                                let spike_io = &mut self.local_net.spike_io;
                                let prev_profile = spike_io.profile;
                                let prev_input_strategy = spike_io.input_strategy;
                                let prev_output_strategy = spike_io.output_strategy;
                                ui.label("Network spike I/O policy");
                                ui.horizontal(|ui| {
                                    ui.label("Profile:");
                                    egui::ComboBox::from_id_salt("ipc_io_profile")
                                        .selected_text(spike_io.profile.as_str())
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut spike_io.profile,
                                                NetworkIoProfileSelector::Auto,
                                                "auto",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.profile,
                                                NetworkIoProfileSelector::Generic,
                                                "generic",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.profile,
                                                NetworkIoProfileSelector::Celegans,
                                                "celegans",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.profile,
                                                NetworkIoProfileSelector::Drosophila,
                                                "drosophila",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.profile,
                                                NetworkIoProfileSelector::Nao,
                                                "nao",
                                            );
                                        });
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Input:");
                                    egui::ComboBox::from_id_salt("ipc_input_strategy")
                                        .selected_text(spike_io.input_strategy.as_str())
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::ProfileDefault,
                                                "profile_default",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::Threshold,
                                                "threshold",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::Rate,
                                                "rate",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::PopulationThreshold,
                                                "population_threshold",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::PopulationRate,
                                                "population_rate",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::PopulationLevel,
                                                "population_level",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::Ttfs,
                                                "ttfs",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::Isi,
                                                "isi",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::Phase,
                                                "phase",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.input_strategy,
                                                SpikeInputEncodingStrategy::Multiplex,
                                                "multiplex",
                                            );
                                        });
                                    ui.label("Output:");
                                    egui::ComboBox::from_id_salt("ipc_output_strategy")
                                        .selected_text(spike_io.output_strategy.as_str())
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut spike_io.output_strategy,
                                                SpikeOutputDecodingStrategy::ProfileDefault,
                                                "profile_default",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.output_strategy,
                                                SpikeOutputDecodingStrategy::Binary,
                                                "binary",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.output_strategy,
                                                SpikeOutputDecodingStrategy::PopulationAverage,
                                                "population_average",
                                            );
                                            ui.selectable_value(
                                                &mut spike_io.output_strategy,
                                                SpikeOutputDecodingStrategy::Graded,
                                                "graded",
                                            );
                                        });
                                });
                                io_changed |= prev_profile != spike_io.profile
                                    || prev_input_strategy != spike_io.input_strategy
                                    || prev_output_strategy != spike_io.output_strategy;
                                io_changed |= ui
                                    .add(
                                        egui::Slider::new(&mut spike_io.threshold, 0.0..=1.0)
                                            .text("Threshold"),
                                    )
                                    .on_hover_text(
                                        "Threshold used by threshold/generic encoders and as a fallback for profile_default",
                                    )
                                    .changed();
                                match spike_io.input_strategy {
                                    SpikeInputEncodingStrategy::Rate => {
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.rate.low_gain,
                                                    0.0..=2.0,
                                                )
                                                .text("Rate low gain"),
                                            )
                                            .changed();
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.rate.high_value_bias,
                                                    0.0..=1.0,
                                                )
                                                .text("Rate high bias"),
                                            )
                                            .changed();
                                    }
                                    SpikeInputEncodingStrategy::PopulationThreshold
                                    | SpikeInputEncodingStrategy::PopulationRate
                                    | SpikeInputEncodingStrategy::PopulationLevel => {}
                                    SpikeInputEncodingStrategy::Ttfs => {
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.ttfs.window_steps,
                                                    1..=128,
                                                )
                                                .text("TTFS window"),
                                            )
                                            .changed();
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.ttfs.threshold,
                                                    0.0..=1.0,
                                                )
                                                .text("TTFS threshold"),
                                            )
                                            .changed();
                                    }
                                    SpikeInputEncodingStrategy::Isi => {
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.isi.min_interval_steps,
                                                    1..=64,
                                                )
                                                .text("ISI min"),
                                            )
                                            .changed();
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.isi.max_interval_steps,
                                                    1..=256,
                                                )
                                                .text("ISI max"),
                                            )
                                            .changed();
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.isi.threshold,
                                                    0.0..=1.0,
                                                )
                                                .text("ISI threshold"),
                                            )
                                            .changed();
                                    }
                                    SpikeInputEncodingStrategy::Phase => {
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.phase.frequency_hz,
                                                    0.1..=120.0,
                                                )
                                                .text("Phase Hz"),
                                            )
                                            .changed();
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.phase.phase_jitter,
                                                    0.0..=1.0,
                                                )
                                                .text("Phase jitter"),
                                            )
                                            .changed();
                                        io_changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut spike_io.phase.threshold,
                                                    0.0..=1.0,
                                                )
                                                .text("Phase gate"),
                                            )
                                            .changed();
                                    }
                                    SpikeInputEncodingStrategy::Multiplex => {
                                        ui.label("Multiplex components");
                                        let strategies = &mut spike_io.multiplex.strategies;
                                        let mut toggle =
                                            |ui: &mut egui::Ui,
                                             label: &str,
                                             primitive: SpikeInputPrimitive| {
                                                let mut enabled = strategies.contains(&primitive);
                                                if ui.checkbox(&mut enabled, label).changed() {
                                                    if enabled {
                                                        if !strategies.contains(&primitive) {
                                                            strategies.push(primitive);
                                                        }
                                                    } else {
                                                        strategies.retain(|s| *s != primitive);
                                                    }
                                                    io_changed = true;
                                                }
                                            };
                                        ui.horizontal(|ui| {
                                            toggle(ui, "threshold", SpikeInputPrimitive::Threshold);
                                            toggle(ui, "rate", SpikeInputPrimitive::Rate);
                                            toggle(ui, "ttfs", SpikeInputPrimitive::Ttfs);
                                            toggle(ui, "isi", SpikeInputPrimitive::Isi);
                                            toggle(ui, "phase", SpikeInputPrimitive::Phase);
                                        });
                                        ui.horizontal(|ui| {
                                            toggle(
                                                ui,
                                                "population_threshold",
                                                SpikeInputPrimitive::PopulationThreshold,
                                            );
                                            toggle(
                                                ui,
                                                "population_rate",
                                                SpikeInputPrimitive::PopulationRate,
                                            );
                                            toggle(
                                                ui,
                                                "population_level",
                                                SpikeInputPrimitive::PopulationLevel,
                                            );
                                        });
                                    }
                                    SpikeInputEncodingStrategy::ProfileDefault
                                    | SpikeInputEncodingStrategy::Threshold => {}
                                }
                                if matches!(
                                    spike_io.input_strategy,
                                    SpikeInputEncodingStrategy::PopulationThreshold
                                        | SpikeInputEncodingStrategy::PopulationRate
                                        | SpikeInputEncodingStrategy::PopulationLevel
                                        | SpikeInputEncodingStrategy::Multiplex
                                ) || matches!(
                                    spike_io.output_strategy,
                                    SpikeOutputDecodingStrategy::PopulationAverage
                                ) {
                                    io_changed |= ui
                                        .add(
                                            egui::DragValue::new(
                                                &mut spike_io.population.neurons_per_value,
                                            )
                                            .range(1..=128)
                                            .speed(1.0)
                                            .prefix("Population n/v "),
                                        )
                                        .changed();
                                    io_changed |= ui
                                        .add(
                                            egui::Slider::new(
                                                &mut spike_io.population.threshold,
                                                0.0..=1.0,
                                            )
                                            .text("Population threshold"),
                                        )
                                        .changed();
                                }
                            }
                            if io_changed {
                                self.ipc_threshold = self.local_net.spike_io.threshold;
                                self.quantizer.threshold = self.local_net.spike_io.threshold;
                                let _ = self
                                    .sim_tx
                                    .send(SimControl::ApplyConfig(self.local_net.clone()));
                            }

                            ui.separator();
                            ui.label("Legacy IPC mapping settings");
                            ui.horizontal(|ui| {
                                ui.label("Neurons/value:");
                                if ui.add(egui::DragValue::new(&mut self.ipc_neurons_per_value).range(1..=128))
                                    .on_hover_text("Spread each decimal value across multiple neurons (population encoding)").changed() {
                                    if let Some(hs) = self.ipc_last_handshake.clone() {
                                        self.apply_ipc_config(hs);
                                    }
                                }
                            });
                            ui.checkbox(
                                &mut self.quantizer.probabilistic,
                                "Legacy probabilistic quantizer",
                            )
                            .on_hover_text(
                                "Only used by the older port-mapping quantizer paths; the network spike_io policy above drives current IPC encoding",
                            );

                            let mut thr = self.ipc_threshold;
                            if ui.add(egui::Slider::new(&mut thr, 0.0..=1.0).text("Threshold"))
                                .on_hover_text("Float→spike threshold; lower to increase activity").changed() {
                                self.ipc_threshold = thr;
                                self.quantizer.threshold = thr;
                                self.local_net.spike_io.threshold = thr;
                            }
                            ui.checkbox(&mut self.ipc_bias_last_sensory_input, "Bias last input")
                                .on_hover_text("Keep last sensory channel ≥ 0.7 to encourage early spiking");
                        });

                        #[cfg(all(feature = "robot_io", unix))]
                        ui.collapsing("Diagnostics", |ui| {
                            let (runner_s, runner_o) = if let Ok(r) = self.runner.try_read() {
                                (r.net.num_sensory_neurons, r.net.num_output_neurons)
                            } else {
                                (
                                    net_cloned.num_sensory_neurons,
                                    net_cloned.num_output_neurons,
                                )
                            };
                            ui.label(format!("Frames processed: {}", self.ipc_frame_count));
                            ui.label(format!("Drops: {}", self.ipc_packet_drop_count));
                            ui.label(format!("Size errors: {}", self.ipc_size_mismatch_count));
                            ui.label(format!("Last frame steps: {}", self.ipc_last_steps));
                            ui.label(format!("Runner S={} O={}", runner_s, runner_o));
                            if let Some(hs) = self.ipc_last_handshake.as_ref() {
                                let (ipc_s, ipc_o) = resolve_ipc_handshake_sizes(hs, runner_s.max(1), runner_o.max(1));
                                ui.label(format!("IPC negotiated S={} O={}", ipc_s, ipc_o));
                            }
                            if ui.button("Clear Stats").clicked() {
                                self.ipc_frame_count = 0;
                                self.ipc_packet_drop_count = 0;
                                self.ipc_size_mismatch_count = 0;
                            }
                        });

                        #[cfg(all(feature = "robot_io", unix))]
                        if let Some(mapping) = &self.ipc_mapping {
                            ui.collapsing("Port Mapping", |ui| {
                                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                    egui::Grid::new("ipc_mapping_grid").striped(true).show(ui, |ui| {
                                        ui.label("Name"); ui.label("Range"); ui.end_row();
                                        for port in mapping.sensors() {
                                            ui.label(egui::RichText::new(&port.name).color(egui::Color32::from_rgb(120,170,255)));
                                            ui.label(format!("{}-{}", port.start, port.start + port.length_neurons - 1));
                                            ui.end_row();
                                        }
                                        for port in mapping.actuators() {
                                            ui.label(egui::RichText::new(&port.name).color(egui::Color32::from_rgb(160,240,120)));
                                            ui.label(format!("{}-{}", port.start, port.start + port.length_neurons - 1));
                                            ui.end_row();
                                        }
                                    });
                                });
                            });
                        }
                    });
                });
                ui.separator();
                });
                ui.collapsing("Visualization", |ui| {
                    if ui.checkbox(&mut self.force_show_connections, "Show live connections")
                        .on_hover_text("Enable heavy edge rendering for large models while simulation keeps running.").changed() {
                        if self.force_show_connections {
                            if !self.show_static_overlays {
                                self.show_static_overlays = true;
                            }
                            if self.overlay_density == 0 {
                                self.overlay_density = 4;
                            }
                            self.pending_edge_cache = true;
                            self.last_edge_cache_refresh = std::time::Instant::now();
                            self.status = "Preparing live connection cache...".to_string();
                        } else {
                            self.pending_edge_cache = false;
                            self.edge_cache_inflight = false;
                            self.cached_edges.clear();
                            #[cfg(feature = "growth3d")]
                            {
                                self.cached_edge_topo = None;
                            }
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            {
                                self.cached_skull_membrane = None;
                            }
                        }
                    }
                    if net_cloned.num_hidden_layers > 64 && !self.force_show_connections {
                        ui.label("Connections hidden for large models; enable 'Show live connections'.");
                    }
                    let mut counts: Vec<usize> = Vec::new();
                    #[cfg(feature = "growth3d")]
                    {
                        if let Ok(r) = self.runner.try_read() {
                            counts = r.topo.layers.iter().map(|l| l.len()).collect();
                        }
                    }
                    if counts.is_empty() {
                        if !self.cached_layer_sizes.is_empty() {
                            counts = self.cached_layer_sizes.clone();
                        } else {
                            counts = vec![net_cloned.num_hidden_per_layer_initial; net_cloned.num_hidden_layers];
                        }
                    }
                    let total_hidden: usize = counts.iter().sum();
                    ui.label(format!(
                        "Layers: {} — total hidden: {} — per-layer counts: {}",
                        counts.len(),
                        total_hidden,
                        Self::compact_usize_list(&counts)
                    ));
                    let refresh_due = self.last_conn_stats_refresh.elapsed().as_millis() as u64 >= self.conn_stats_refresh_ms;
                    let counts_from_edge_cache = self.show_static_overlays || self.force_show_connections;
                    if refresh_due && !counts_from_edge_cache {
                        if let Ok(r) = self.runner.try_read() {
                            self.cached_conn_counts = r.connection_counts();
                            self.cached_output_conn_count = Some(r.output_connection_count());
                            self.last_conn_stats_refresh = std::time::Instant::now();
                        }
                    }
                    let conn_counts = if self.cached_conn_counts.is_empty() {
                        None
                    } else {
                        Some(self.cached_conn_counts.clone())
                    };
                    if let Some(conn_counts) = conn_counts {
                        ui.label(format!(
                            "Per-layer connections: {}",
                            Self::compact_usize_list(&conn_counts)
                        ));
                    } else {
                        ui.label("Per-layer connections: (busy)");
                    }
                    if let Some(out_conn) = self.cached_output_conn_count {
                        ui.label(format!("Output connections: {}", out_conn));
                    } else {
                        ui.label("Output connections: (busy)");
                    }
                    ui.separator();
                    ui.collapsing("Oscilloscope", |ui| {
                        ui.checkbox(&mut self.scope_paused, "Pause");
                        ui.horizontal(|ui|{
                            ui.add(egui::Slider::new(&mut self.scope_time_ms, 250.0..=10000.0).logarithmic(true).text("Time (ms)"));
                            ui.add(egui::Slider::new(&mut self.scope_gain, 0.25..=8.0).logarithmic(true).text("Gain"));
                        });
                        ui.horizontal(|ui|{
                            ui.checkbox(&mut self.scope_lanes, "Lanes");
                            ui.checkbox(&mut self.scope_grid, "Grid");
                        });
                        ui.separator();
                        // Quick add probe helpers
                        ui.horizontal(|ui|{
                            ui.label("Sensory:");
                            let mut i = 0usize; ui.add(egui::DragValue::new(&mut i).range(0..=net_cloned.num_sensory_neurons.saturating_sub(1)));
                            if ui.button("+ Spike").clicked() {
                                let id = self.next_probe_id; self.next_probe_id += 1;
                                self.probes.push(Probe::new(id, format!("S{} spike", i), egui::Color32::from_rgb(120,170,255), ProbeTarget::Sensory(i), ProbeKind::Spike, 10_000));
                            }
                        });
                        ui.horizontal(|ui|{
                            ui.label("Hidden l,j:");
                            let mut l = 0usize; let mut j = 0usize;
                            ui.add(egui::DragValue::new(&mut l).range(0..=net_cloned.num_hidden_layers.saturating_sub(1)));
                            let hj = self.runner.try_read()
                                .map(|r| r.v_h.get(l).map(|a| a.len()).unwrap_or(1).saturating_sub(1))
                                .unwrap_or_else(|_| net_cloned.num_hidden_per_layer_initial.saturating_sub(1));
                            ui.add(egui::DragValue::new(&mut j).range(0..=hj));
                            if ui.button("+ Vm").clicked() {
                                let id = self.next_probe_id; self.next_probe_id += 1;
                                self.probes.push(Probe::new(id, format!("H{}:{} Vm", l+1, j), egui::Color32::from_rgb(255,190,80), ProbeTarget::Hidden(l, j), ProbeKind::Membrane, 10_000));
                            }
                        });
                        ui.horizontal(|ui|{
                            ui.label("Output k:");
                            let mut k = 0usize; ui.add(egui::DragValue::new(&mut k).range(0..=net_cloned.num_output_neurons.saturating_sub(1)));
                            if ui.button("+ Vm").clicked() {
                                let id = self.next_probe_id; self.next_probe_id += 1;
                                self.probes.push(Probe::new(id, format!("O{} Vm", k), egui::Color32::from_rgb(160,240,120), ProbeTarget::Output(k), ProbeKind::Membrane, 10_000));
                            }
                        });
                        ui.separator();
                        // Provider band probes (if available)
                        if let Some(bands) = bands_guard.as_deref() {
                            ui.horizontal(|ui|{
                                ui.label("Band b:");
                                let mut b = 0usize; ui.add(egui::DragValue::new(&mut b).range(0..=bands.len().saturating_sub(1)));
                                if ui.button("+ Level").clicked() {
                                    let id = self.next_probe_id; self.next_probe_id += 1;
                                    self.probes.push(Probe::new(id, format!("Band{} level", b), egui::Color32::from_rgb(200,120,255), ProbeTarget::Band(b), ProbeKind::Level, 10_000));
                                }
                            });
                        }
                        // Existing probes list
                        let mut remove_id: Option<u32> = None;
                        for pr in &mut self.probes {
                            ui.horizontal(|ui|{
                                ui.checkbox(&mut pr.enabled, "");
                                ui.color_edit_button_srgba(&mut pr.color);
                                ui.text_edit_singleline(&mut pr.name);
                                if ui.button("×").on_hover_text("Remove").clicked() { remove_id = Some(pr.id); }
                            });
                        }
                        if let Some(id) = remove_id { self.probes.retain(|pr| pr.id != id); }
                    });
                    ui.collapsing("View", |ui| {
                        ui.label("Pan / Zoom / Rotate");
                        let mut layout = self.network_layout;
                        ui.horizontal(|ui| {
                            ui.label("Layout");
                            ui.selectable_value(&mut layout, NetworkLayout::Conventional, "Conventional");
                            ui.selectable_value(&mut layout, NetworkLayout::Aarnn, "AARNN");
                            let auto_changed = ui.checkbox(&mut self.layout_auto, "Auto").changed();
                            if auto_changed && self.layout_auto {
                                let desired = self.preferred_layout_for_view(&model_cloned, &network_registry);
                                self.set_network_layout(desired, true);
                            }
                        });
                        if layout != self.network_layout {
                            self.set_network_layout(layout, false);
                        }
                        let mut changed = false;
                        changed |= ui.add(egui::Slider::new(&mut self.camera_zoom, 0.25..=4.0).text("Zoom")).changed();
                        #[cfg(feature = "growth3d")]
                        {
                            if matches!(self.network_layout, NetworkLayout::Aarnn) {
                                changed |= ui.add(egui::Slider::new(&mut self.camera_yaw_degrees, -45.0..=45.0).text("Yaw (deg)")).changed();
                                changed |= ui.add(egui::Slider::new(&mut self.camera_pitch_degrees, -30.0..=30.0).text("Pitch (deg)")).changed();
                            }
                        }
                        ui.horizontal(|ui|{
                            if ui.button("Pan Left").clicked() { self.cam_pan.x -= 20.0; }
                            if ui.button("Pan Right").clicked() { self.cam_pan.x += 20.0; }
                        });
                        ui.horizontal(|ui|{
                            if ui.button("Pan Up").clicked() { self.cam_pan.y -= 20.0; }
                            if ui.button("Pan Down").clicked() { self.cam_pan.y += 20.0; }
                        });
                        if ui.button("Reset View").clicked() { self.camera_zoom = 1.0; self.camera_yaw_degrees = 0.0; self.camera_pitch_degrees = 0.0; self.cam_pan = egui::vec2(0.0, 0.0); }
                        if changed { self.status = "View updated".into(); }
                    });
                    ui.collapsing("Visuals", |ui| {
                        ui.checkbox(&mut self.show_highlights, "Highlight active connections").on_hover_text("Draw bright edges between current spiking senders and receivers");
                        ui.add_enabled_ui(self.show_highlights, |ui| {
                            ui.add(egui::Slider::new(&mut self.max_highlight_lines, 1..=20).text("Max lines per neuron")).on_hover_text("Top-k strongest incoming active links to draw per receiver");
                        });
                        ui.separator();
                        ui.checkbox(&mut self.show_backward_highlights, "Highlight backward connections").on_hover_text("Show H(l+1 previous) → H(l current) links via backward matrix");
                        ui.separator();
                        if ui.checkbox(&mut self.show_static_overlays, "Show static connection overlays")
                            .on_hover_text("Always show faint strongest connections").changed() && self.show_static_overlays {
                            self.pending_edge_cache = true;
                            self.last_edge_cache_refresh = std::time::Instant::now();
                        }
                        ui.add_enabled_ui(self.show_static_overlays, |ui| {
                            if ui.add(egui::Slider::new(&mut self.overlay_density, 0..=20).text("Overlay density"))
                                .on_hover_text("Top-k incoming links per receiver to render in overlays").changed() {
                                self.pending_edge_cache = true;
                                self.last_edge_cache_refresh = std::time::Instant::now();
                            }
                            ui.add(egui::Slider::new(&mut self.overlay_opacity, 0.05..=1.0).text("Overlay opacity")).on_hover_text("Opacity multiplier for overlay edges");
                        });
                        ui.separator();
                        ui.checkbox(&mut self.show_feedback_overlays, "Show feedback map overlays").on_hover_text("Render Output→Sensory mapping used when Loop feedback is enabled");
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            ui.separator();
                            ui.checkbox(&mut self.show_morpho_overlays, "Show morphology synapse overlays")
                                .on_hover_text("Draw faint lines for synapses derived from morphology mapping (requires Morphology toggle under AARNN).");
                            ui.add_enabled_ui(self.show_morpho_overlays, |ui| {
                                ui.add(egui::Slider::new(&mut self.morpho_opacity, 0.05..=1.0).text("Morphology opacity"))
                                    .on_hover_text("Opacity multiplier for morphology synapse overlays");
                            });
                            ui.separator();
                            ui.checkbox(&mut self.show_transmissions, "Show transmissions")
                                .on_hover_text("Flash synapses that released this frame (AARNN Morphology behavioral path)");
                            ui.add_enabled_ui(self.show_transmissions, |ui| {
                                ui.add(egui::Slider::new(&mut self.transmissions_opacity, 0.1..=1.0).text("Transmissions opacity"))
                                    .on_hover_text("Opacity multiplier for transmission flashes");
                            });
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_equalizer, "Show Graphic EQ").on_hover_text("Show frequency bands for audio input sources (lower-left corner)");
                    });
                });
                ui.separator();
                ui.collapsing("Tools & Model I/O", |ui| {
                    ui.collapsing("Save / Load", |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Save Config…").on_hover_text("Save NetworkConfig to JSON").clicked() {
                                match self.export_view_config_json() {
                                    Ok(json) => {
                                        if let Some(path) = rfd::FileDialog::new().add_filter("json", &["json"]).set_file_name("config.json").save_file() {
                                            let tx = self.tool_task_tx.clone();
                                            self.status = format!("Saving config to {}", path.display());
                                            std::thread::spawn(move || {
                                                let error = std::fs::write(&path, json).err().map(|e| e.to_string());
                                                let _ = tx.send(ToolTaskResult::FileWrite {
                                                    kind: FileTaskKind::SaveConfig,
                                                    path,
                                                    error,
                                                });
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to export config: {}", e);
                                    }
                                }
                            }
                            if ui.button("Load Config…").on_hover_text("Load NetworkConfig from JSON").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("json", &["json"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    self.status = format!("Loading config from {}", path.display());
                                    std::thread::spawn(move || {
                                        let res = std::fs::read_to_string(&path);
                                        let (data, error) = match res {
                                            Ok(s) => (Some(s), None),
                                            Err(e) => (None, Some(e.to_string())),
                                        };
                                        let _ = tx.send(ToolTaskResult::FileRead {
                                            kind: FileTaskKind::LoadConfig,
                                            path,
                                            data,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Save Network…").on_hover_text("Save weights/topology snapshot to JSON").clicked() {
                                match self.export_view_network_json() {
                                    Ok(json) => {
                                        if let Some(path) = rfd::FileDialog::new().add_filter("json", &["json"]).set_file_name("network.json").save_file() {
                                            let tx = self.tool_task_tx.clone();
                                            self.status = format!("Saving network snapshot to {}", path.display());
                                            std::thread::spawn(move || {
                                                let error = std::fs::write(&path, json).err().map(|e| e.to_string());
                                                let _ = tx.send(ToolTaskResult::FileWrite {
                                                    kind: FileTaskKind::SaveNetwork,
                                                    path,
                                                    error,
                                                });
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to export network: {}", e);
                                    }
                                }
                            }
                            if ui.button("Load Network…").on_hover_text("Load weights/topology snapshot from JSON").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("json", &["json"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    self.status = format!("Loading network from {}", path.display());
                                    std::thread::spawn(move || {
                                        let res = std::fs::read_to_string(&path);
                                        let (data, error) = match res {
                                            Ok(s) => (Some(s), None),
                                            Err(e) => (None, Some(e.to_string())),
                                        };
                                        let _ = tx.send(ToolTaskResult::FileRead {
                                            kind: FileTaskKind::LoadNetwork,
                                            path,
                                            data,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Save Probes…").on_hover_text("Save oscilloscope probe metadata to JSON").clicked() {
                                match self.export_probes_json() {
                                    Ok(json) => {
                                        if let Some(path) = rfd::FileDialog::new().add_filter("json", &["json"]).set_file_name("probes.json").save_file() {
                                            let tx = self.tool_task_tx.clone();
                                            self.status = format!("Saving probes to {}", path.display());
                                            std::thread::spawn(move || {
                                                let error = std::fs::write(&path, json).err().map(|e| e.to_string());
                                                let _ = tx.send(ToolTaskResult::FileWrite {
                                                    kind: FileTaskKind::SaveProbes,
                                                    path,
                                                    error,
                                                });
                                            });
                                        }
                                    }
                                    Err(e) => { self.status = format!("Serialize probes failed: {}", e); }
                                }
                            }
                            if ui.button("Load Probes…").on_hover_text("Load oscilloscope probe metadata from JSON").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("json", &["json"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    self.status = format!("Loading probes from {}", path.display());
                                    std::thread::spawn(move || {
                                        let res = std::fs::read_to_string(&path);
                                        let (data, error) = match res {
                                            Ok(s) => (Some(s), None),
                                            Err(e) => (None, Some(e.to_string())),
                                        };
                                        let _ = tx.send(ToolTaskResult::FileRead {
                                            kind: FileTaskKind::LoadProbes,
                                            path,
                                            data,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                    });
                    ui.separator();
                    // Model export/import (ONNX / TFLite / NIR via Python tools)
                    ui.collapsing("ONNX / TFLite / NIR Export (Experimental)", |ui| {
                        if let Some(report) = &self.last_import_report {
                            ui.label(format!("Last import: {}", report));
                        }
                        ui.separator();
                        ui.label("TFLite import settings");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.tflite_import_mode, TfliteImportMode::Mlp, "MLP (Dense)");
                            ui.selectable_value(&mut self.tflite_import_mode, TfliteImportMode::Cnn, "CNN (Conv)");
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.tflite_allow_fallback, "Allow fallback (2D scan)");
                            ui.checkbox(&mut self.tflite_allow_large, "Allow large");
                        });
                        ui.checkbox(&mut self.tflite_freeze_learning, "Freeze learning (eta=0)");
                        ui.horizontal(|ui| {
                            ui.label("Sim throttle (ms)");
                            ui.add(egui::DragValue::new(&mut self.tflite_sim_throttle_ms).range(0..=50));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max layers");
                            ui.add(egui::DragValue::new(&mut self.tflite_max_layers).range(1..=256));
                            ui.label("Max params");
                            ui.add(egui::DragValue::new(&mut self.tflite_max_params).range(10_000..=50_000_000));
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Export ONNX…").on_hover_text("Export feedforward weights to an ONNX MLP (tools/export_onnx.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("ONNX", &["onnx"]).set_file_name("model.onnx").save_file() {
                                    let json_res = self.export_view_network_json();
                                    match json_res {
                                        Ok(json) => {
                                            let tx = self.tool_task_tx.clone();
                                            let python_override = self.python_path.clone();
                                            self.status = format!("Exporting ONNX to {}", path.display());
                                            std::thread::spawn(move || {
                                                let tmp = std::env::temp_dir().join(format!("network_export_{}.json", fastrand::u64(..)));
                                                let mut stdout = String::new();
                                                let mut stderr = String::new();
                                                let mut error = None;
                                                if let Err(e) = std::fs::write(&tmp, json) {
                                                    error = Some(format!("Write temp failed: {}", e));
                                                } else {
                                                    let args = vec![
                                                        std::ffi::OsString::from("--in"),
                                                        tmp.as_os_str().to_os_string(),
                                                        std::ffi::OsString::from("--out"),
                                                        path.as_os_str().to_os_string(),
                                                    ];
                                                    match Self::run_tool_with_python(python_override, "export_onnx.py", &args) {
                                                        Ok(o) => {
                                                            stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                            stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                            if !o.status.success() {
                                                                error = Some("Tool failed".to_string());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error = Some(format!("{}", e));
                                                        }
                                                    }
                                                }
                                                let _ = tx.send(ToolTaskResult::ToolExport {
                                                    kind: ToolExportKind::Onnx,
                                                    path,
                                                    stdout,
                                                    stderr,
                                                    error,
                                                });
                                            });
                                        }
                                        Err(e) => self.status = format!("Serialize failed: {}", e),
                                    }
                                }
                            }
                            if ui.button("Import ONNX…").on_hover_text("Import ONNX MLP into network weights (tools/import_onnx.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("ONNX", &["onnx"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    let python_override = self.python_path.clone();
                                    self.status = format!("Importing ONNX from {}", path.display());
                                    std::thread::spawn(move || {
                                        let tmp_out = std::env::temp_dir().join(format!("network_import_{}.json", fastrand::u64(..)));
                                        let mut stdout = String::new();
                                        let mut stderr = String::new();
                                        let mut error = None;
                                        let mut json = None;
                                        let args = vec![
                                            std::ffi::OsString::from("--in"),
                                            path.as_os_str().to_os_string(),
                                            std::ffi::OsString::from("--out-network"),
                                            tmp_out.as_os_str().to_os_string(),
                                        ];
                                        match Self::run_tool_with_python(python_override, "import_onnx.py", &args) {
                                            Ok(o) => {
                                                stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                if o.status.success() {
                                                    match std::fs::read_to_string(&tmp_out) {
                                                        Ok(j) => { json = Some(j); }
                                                        Err(e) => { error = Some(format!("Read temp failed: {}", e)); }
                                                    }
                                                } else {
                                                    error = Some("Tool failed".to_string());
                                                }
                                            }
                                            Err(e) => {
                                                error = Some(format!("{}", e));
                                            }
                                        }
                                        let _ = tx.send(ToolTaskResult::ToolImport {
                                            kind: ImportKind::Onnx,
                                            path,
                                            json,
                                            stdout,
                                            stderr,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Export TFLite…").on_hover_text("Export feedforward weights to TFLite (tools/export_tflite.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("TFLite", &["tflite"]).set_file_name("model.tflite").save_file() {
                                    let json_res = self.export_view_network_json();
                                    match json_res {
                                        Ok(json) => {
                                            let tx = self.tool_task_tx.clone();
                                            let python_override = self.python_path.clone();
                                            self.status = format!("Exporting TFLite to {}", path.display());
                                            std::thread::spawn(move || {
                                                let tmp = std::env::temp_dir().join(format!("network_export_{}.json", fastrand::u64(..)));
                                                let mut stdout = String::new();
                                                let mut stderr = String::new();
                                                let mut error = None;
                                                if let Err(e) = std::fs::write(&tmp, json) {
                                                    error = Some(format!("Write temp failed: {}", e));
                                                } else {
                                                    let args = vec![
                                                        std::ffi::OsString::from("--in"),
                                                        tmp.as_os_str().to_os_string(),
                                                        std::ffi::OsString::from("--out"),
                                                        path.as_os_str().to_os_string(),
                                                    ];
                                                    match Self::run_tool_with_python(python_override, "export_tflite.py", &args) {
                                                        Ok(o) => {
                                                            stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                            stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                            if !o.status.success() {
                                                                error = Some("Tool failed".to_string());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error = Some(format!("{}", e));
                                                        }
                                                    }
                                                }
                                                let _ = tx.send(ToolTaskResult::ToolExport {
                                                    kind: ToolExportKind::Tflite,
                                                    path,
                                                    stdout,
                                                    stderr,
                                                    error,
                                                });
                                            });
                                        }
                                        Err(e) => self.status = format!("Serialize failed: {}", e),
                                    }
                                }
                            }
                            if ui.button("Import TFLite…").on_hover_text("Import TFLite MLP into network weights (tools/import_tflite.py)").clicked() {
                                if self.pending_import.is_some() {
                                    self.pending_import = None;
                                    self.status = "Canceled previous import".to_string();
                                }
                                let tx = self.tool_task_tx.clone();
                                let mode = match self.tflite_import_mode {
                                    TfliteImportMode::Mlp => "mlp",
                                    TfliteImportMode::Cnn => "cnn",
                                };
                                let max_layers = self.tflite_max_layers;
                                let max_params = self.tflite_max_params;
                                let allow_fallback = self.tflite_allow_fallback;
                                let allow_large = self.tflite_allow_large;
                                let python_override = self.python_path.clone();
                                self.status = "Select a TFLite file...".to_string();
                                self.last_import_report = Some("Select a TFLite file...".to_string());
                                std::thread::spawn(move || {
                                    let py = match Self::resolve_python_with_override(python_override) {
                                        Ok(py) => py,
                                        Err(e) => {
                                            let _ = tx.send(ToolTaskResult::TfliteImport {
                                                path: std::path::PathBuf::new(),
                                                json: None,
                                                stdout: String::new(),
                                                stderr: String::new(),
                                                error: Some(format!("Python resolve failed: {}", e)),
                                            });
                                            return;
                                        }
                                    };
                                    let tool = match Self::resolve_tool("import_tflite.py") {
                                        Ok(tool) => tool,
                                        Err(e) => {
                                            let _ = tx.send(ToolTaskResult::TfliteImport {
                                                path: std::path::PathBuf::new(),
                                                json: None,
                                                stdout: String::new(),
                                                stderr: String::new(),
                                                error: Some(format!("{}", e)),
                                            });
                                            return;
                                        }
                                    };
                                    let path = rfd::FileDialog::new().add_filter("TFLite", &["tflite"]).pick_file();
                                    let Some(path) = path else {
                                        let _ = tx.send(ToolTaskResult::TflitePickCanceled);
                                        return;
                                    };
                                    nm_log!("[import] TFLite tool start: {}", path.display());
                                    let tmp_out = std::env::temp_dir().join(format!("network_import_{}.json", fastrand::u64(..)));
                                    let output = std::process::Command::new(py)
                                        .arg(tool)
                                        .arg("--in")
                                        .arg(&path)
                                        .arg("--out-network")
                                        .arg(&tmp_out)
                                        .arg("--mode")
                                        .arg(mode)
                                        .arg("--max-layers")
                                        .arg(format!("{}", max_layers))
                                        .arg("--max-params")
                                        .arg(format!("{}", max_params))
                                        .env("NMD_TFLITE_ALLOW_FALLBACK", if allow_fallback { "1" } else { "0" })
                                        .env("NMD_TFLITE_ALLOW_LARGE", if allow_large { "1" } else { "0" })
                                        .output();
                                    let msg = match output {
                                        Ok(o) => {
                                            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                            if o.status.success() {
                                                match std::fs::read_to_string(&tmp_out) {
                                                    Ok(json) => ToolTaskResult::TfliteImport {
                                                        path,
                                                        json: Some(json),
                                                        stdout,
                                                        stderr,
                                                        error: None,
                                                    },
                                                    Err(e) => ToolTaskResult::TfliteImport {
                                                        path,
                                                        json: None,
                                                        stdout,
                                                        stderr,
                                                        error: Some(format!("Read temp failed: {}", e)),
                                                    },
                                                }
                                            } else {
                                                ToolTaskResult::TfliteImport {
                                                    path,
                                                    json: None,
                                                    stdout,
                                                    stderr,
                                                    error: Some("Tool failed".to_string()),
                                                }
                                            }
                                        }
                                        Err(e) => ToolTaskResult::TfliteImport {
                                            path,
                                            json: None,
                                            stdout: String::new(),
                                            stderr: String::new(),
                                            error: Some(format!("Failed to run python: {}", e)),
                                        },
                                    };
                                    let _ = tx.send(msg);
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Export NeuroML…").on_hover_text("Export network to NeuroML 2.0 (tools/export_neuroml.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("NeuroML", &["nml"]).set_file_name("model.nml").save_file() {
                                    let export_res = self.export_view_network_json();
                                    match export_res {
                                        Ok(json) => {
                                            let tx = self.tool_task_tx.clone();
                                            let python_override = self.python_path.clone();
                                            self.status = format!("Exporting NeuroML to {}", path.display());
                                            std::thread::spawn(move || {
                                                let tmp = std::env::temp_dir().join(format!("network_export_{}.json", fastrand::u64(..)));
                                                let mut stdout = String::new();
                                                let mut stderr = String::new();
                                                let mut error = None;
                                                if let Err(e) = std::fs::write(&tmp, json) {
                                                    error = Some(format!("Write temp failed: {}", e));
                                                } else {
                                                    let args = vec![
                                                        std::ffi::OsString::from("--in-network"),
                                                        tmp.as_os_str().to_os_string(),
                                                        std::ffi::OsString::from("--out-neuroml"),
                                                        path.as_os_str().to_os_string(),
                                                    ];
                                                    match Self::run_tool_with_python(python_override, "export_neuroml.py", &args) {
                                                        Ok(o) => {
                                                            stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                            stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                            if !o.status.success() {
                                                                error = Some("Tool failed".to_string());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error = Some(format!("{}", e));
                                                        }
                                                    }
                                                }
                                                let _ = tx.send(ToolTaskResult::ToolExport {
                                                    kind: ToolExportKind::NeuroML,
                                                    path,
                                                    stdout,
                                                    stderr,
                                                    error,
                                                });
                                            });
                                        }
                                        Err(e) => self.status = format!("Serialize failed: {}", e),
                                    }
                                }
                            }
                            if ui.button("Import NeuroML…").on_hover_text("Import NeuroML 2.0 into network weights (tools/import_neuroml.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("NeuroML", &["nml"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    let python_override = self.python_path.clone();
                                    self.status = format!("Importing NeuroML from {}", path.display());
                                    std::thread::spawn(move || {
                                        let tmp_out = std::env::temp_dir().join(format!("network_import_{}.json", fastrand::u64(..)));
                                        let mut stdout = String::new();
                                        let mut stderr = String::new();
                                        let mut error = None;
                                        let mut json = None;
                                        let args = vec![
                                            std::ffi::OsString::from("--in"),
                                            path.as_os_str().to_os_string(),
                                            std::ffi::OsString::from("--out-network"),
                                            tmp_out.as_os_str().to_os_string(),
                                        ];
                                        match Self::run_tool_with_python(python_override, "import_neuroml.py", &args) {
                                            Ok(o) => {
                                                stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                if o.status.success() {
                                                    match std::fs::read_to_string(&tmp_out) {
                                                        Ok(j) => { json = Some(j); }
                                                        Err(e) => { error = Some(format!("Read temp failed: {}", e)); }
                                                    }
                                                } else {
                                                    error = Some("Tool failed".to_string());
                                                }
                                            }
                                            Err(e) => {
                                                error = Some(format!("{}", e));
                                            }
                                        }
                                        let _ = tx.send(ToolTaskResult::ToolImport {
                                            kind: ImportKind::NeuroML,
                                            path,
                                            json,
                                            stdout,
                                            stderr,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Export PyNN…").on_hover_text("Export network to a PyNN Python script (tools/export_pynn.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("Python", &["py"]).set_file_name("model_pynn.py").save_file() {
                                    let export_res = self.export_view_network_json();
                                    match export_res {
                                        Ok(json) => {
                                            let tx = self.tool_task_tx.clone();
                                            let python_override = self.python_path.clone();
                                            self.status = format!("Exporting PyNN to {}", path.display());
                                            std::thread::spawn(move || {
                                                let tmp = std::env::temp_dir().join(format!("network_export_{}.json", fastrand::u64(..)));
                                                let mut stdout = String::new();
                                                let mut stderr = String::new();
                                                let mut error = None;
                                                if let Err(e) = std::fs::write(&tmp, json) {
                                                    error = Some(format!("Write temp failed: {}", e));
                                                } else {
                                                    let args = vec![
                                                        std::ffi::OsString::from("--in-network"),
                                                        tmp.as_os_str().to_os_string(),
                                                        std::ffi::OsString::from("--out-pynn"),
                                                        path.as_os_str().to_os_string(),
                                                    ];
                                                    match Self::run_tool_with_python(python_override, "export_pynn.py", &args) {
                                                        Ok(o) => {
                                                            stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                            stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                            if !o.status.success() {
                                                                error = Some("Tool failed".to_string());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error = Some(format!("{}", e));
                                                        }
                                                    }
                                                }
                                                let _ = tx.send(ToolTaskResult::ToolExport {
                                                    kind: ToolExportKind::PyNN,
                                                    path,
                                                    stdout,
                                                    stderr,
                                                    error,
                                                });
                                            });
                                        }
                                        Err(e) => self.status = format!("Serialize failed: {}", e),
                                    }
                                }
                            }
                            if ui.button("Import PyNN…").on_hover_text("Import PyNN model into network weights (tools/import_pynn.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("Python", &["py"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    let python_override = self.python_path.clone();
                                    self.status = format!("Importing PyNN from {}", path.display());
                                    std::thread::spawn(move || {
                                        let tmp_out = std::env::temp_dir().join(format!("network_import_{}.json", fastrand::u64(..)));
                                        let mut stdout = String::new();
                                        let mut stderr = String::new();
                                        let mut error = None;
                                        let mut json = None;
                                        let args = vec![
                                            std::ffi::OsString::from("--in"),
                                            path.as_os_str().to_os_string(),
                                            std::ffi::OsString::from("--out-network"),
                                            tmp_out.as_os_str().to_os_string(),
                                        ];
                                        match Self::run_tool_with_python(python_override, "import_pynn.py", &args) {
                                            Ok(o) => {
                                                stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                if o.status.success() {
                                                    match std::fs::read_to_string(&tmp_out) {
                                                        Ok(j) => { json = Some(j); }
                                                        Err(e) => { error = Some(format!("Read temp failed: {}", e)); }
                                                    }
                                                } else {
                                                    error = Some("Tool failed".to_string());
                                                }
                                            }
                                            Err(e) => {
                                                error = Some(format!("{}", e));
                                            }
                                        }
                                        let _ = tx.send(ToolTaskResult::ToolImport {
                                            kind: ImportKind::PyNN,
                                            path,
                                            json,
                                            stdout,
                                            stderr,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Export NIR…").on_hover_text("Export network to Neuromorphic Intermediate Representation (tools/export_nir.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("NIR", &["nir"]).set_file_name("model.nir").save_file() {
                                    let export_res = self.export_view_network_json();
                                    match export_res {
                                        Ok(json) => {
                                            let tx = self.tool_task_tx.clone();
                                            let python_override = self.python_path.clone();
                                            self.status = format!("Exporting NIR to {}", path.display());
                                            std::thread::spawn(move || {
                                                let tmp = std::env::temp_dir().join(format!("network_export_{}.json", fastrand::u64(..)));
                                                let mut stdout = String::new();
                                                let mut stderr = String::new();
                                                let mut error = None;
                                                if let Err(e) = std::fs::write(&tmp, json) {
                                                    error = Some(format!("Write temp failed: {}", e));
                                                } else {
                                                    let args = vec![
                                                        std::ffi::OsString::from("--in"),
                                                        tmp.as_os_str().to_os_string(),
                                                        std::ffi::OsString::from("--out"),
                                                        path.as_os_str().to_os_string(),
                                                    ];
                                                    match Self::run_tool_with_python(python_override, "export_nir.py", &args) {
                                                        Ok(o) => {
                                                            stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                            stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                            if !o.status.success() {
                                                                error = Some("Tool failed".to_string());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error = Some(format!("{}", e));
                                                        }
                                                    }
                                                }
                                                let _ = tx.send(ToolTaskResult::ToolExport {
                                                    kind: ToolExportKind::Nir,
                                                    path,
                                                    stdout,
                                                    stderr,
                                                    error,
                                                });
                                            });
                                        }
                                        Err(e) => self.status = format!("Serialize failed: {}", e),
                                    }
                                }
                            }
                            if ui.button("Import NIR…").on_hover_text("Import NIR model into network weights (tools/import_nir.py)").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("NIR", &["nir"]).pick_file() {
                                    let tx = self.tool_task_tx.clone();
                                    let python_override = self.python_path.clone();
                                    self.status = format!("Importing NIR from {}", path.display());
                                    std::thread::spawn(move || {
                                        let tmp_out = std::env::temp_dir().join(format!("network_import_{}.json", fastrand::u64(..)));
                                        let mut stdout = String::new();
                                        let mut stderr = String::new();
                                        let mut error = None;
                                        let mut json = None;
                                        let args = vec![
                                            std::ffi::OsString::from("--in"),
                                            path.as_os_str().to_os_string(),
                                            std::ffi::OsString::from("--out-network"),
                                            tmp_out.as_os_str().to_os_string(),
                                        ];
                                        match Self::run_tool_with_python(python_override, "import_nir.py", &args) {
                                            Ok(o) => {
                                                stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                if o.status.success() {
                                                    match std::fs::read_to_string(&tmp_out) {
                                                        Ok(j) => { json = Some(j); }
                                                        Err(e) => { error = Some(format!("Read temp failed: {}", e)); }
                                                    }
                                                } else {
                                                    error = Some("Tool failed".to_string());
                                                }
                                            }
                                            Err(e) => {
                                                error = Some(format!("{}", e));
                                            }
                                        }
                                        let _ = tx.send(ToolTaskResult::ToolImport {
                                            kind: ImportKind::Nir,
                                            path,
                                            json,
                                            stdout,
                                            stderr,
                                            error,
                                        });
                                    });
                                }
                            }
                        });
                    });
                    ui.separator();
                    ui.collapsing("External tools", |ui| {
                        ui.label("Python interpreter override (optional)");
                        let mut override_str = self.python_path.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut override_str).changed() {
                            self.python_path = if override_str.trim().is_empty() { None } else { Some(override_str.clone()) };
                        }
                        if ui.button("Detect Python").clicked() {
                            let tx = self.tool_task_tx.clone();
                            let python_override = self.python_path.clone();
                            self.status = "Detecting Python...".to_string();
                            std::thread::spawn(move || {
                                let res = Self::resolve_python_with_override(python_override).map_err(|e| e.to_string());
                                let _ = tx.send(ToolTaskResult::PythonResolved { result: res });
                            });
                        }
                        ui.label("Tip: You can set the environment variable NMD_PYTHON to point to your python3 binary.");
                    });
                    ui.separator();
                    ui.collapsing("Companion Interfaces", |ui| {
                        ui.label("Browser surfaces now share a consistent session/runtime shell. Use each surface for the role it is best at instead of forcing one UI to do everything.");
                        ui.separator();
                        egui::Grid::new("ui_surface_matrix")
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Surface");
                                ui.strong("Role");
                                ui.strong("Best for");
                                ui.end_row();

                                ui.label("Native UI");
                                ui.label("Operational");
                                ui.label("Full simulation control, morphology, GA search, deep local import/export.");
                                ui.end_row();

                                ui.label("Web UI");
                                ui.label("Operational");
                                ui.label("Remote orchestration, browser topology view, probes, browser save/load, export.");
                                ui.end_row();

                                ui.label("Docs");
                                ui.label("Reference");
                                ui.label("Function matrix, auth flow, runtime notes, endpoint contract.");
                                ui.end_row();

                                ui.label("Swagger");
                                ui.label("Executable");
                                ui.label("Same-origin API request testing with the current browser session cookie.");
                                ui.end_row();
                            });
                        ui.separator();
                        ui.label("When the companion web server is running, the browser routes are:");
                        ui.label("/  -> Web UI");
                        ui.label("/docs  -> interface matrix + reference");
                        ui.label("/docs/swagger  -> interactive API explorer");
                    });
                });
            });
        });
    });

            let state_arc_for_layout = state_arc.clone();
            egui::CentralPanel::default().show_inside(ui, |ui| {
            // Main drawing area
            let avail = ui.available_size();
            let panel_rect = ui.allocate_space(avail).1;

            // Pre-compute Oscilloscope rect so we can gate network wheel-zoom when hovering scope
            let margin = 10.0f32;
            let scope_w = 520.0f32;
            let scope_h = 150.0f32;
            let scope_rect = egui::Rect::from_min_size(
                egui::pos2(
                    (panel_rect.center().x - scope_w * 0.5)
                        .max(panel_rect.left() + margin)
                        .min(panel_rect.right() - scope_w - margin),
                    panel_rect.bottom() - scope_h - margin,
                ),
                egui::vec2(scope_w, scope_h),
            );

            // Handle mouse gestures for pan / zoom / rotate on the central canvas
            // Interaction is only for this panel rect; we use click-drag and wheel.
            let canvas_id = egui::Id::new("network_canvas");
            let response = ui.interact(panel_rect, canvas_id, egui::Sense::click_and_drag());
            let mut cam_changed = false;
            ui.input(|i| {
                // Zoom: mouse wheel or trackpad pinch; focus around mouse position within panel
                let mut zoom_factor = 1.0f32;
                // Trackpad pinch zoom (egui provides a multiplicative delta)
                let pinch = i.zoom_delta();
                let pointer_pos = i.pointer.hover_pos();
                // Apply pinch to network only when NOT hovering the oscilloscope
                let mouse_over_scope = pointer_pos.map(|p| scope_rect.contains(p)).unwrap_or(false);
                let pointer_over_canvas = response.hovered()
                    && pointer_pos.map(|p| panel_rect.contains(p)).unwrap_or(false);
                if pointer_over_canvas && !mouse_over_scope {
                    if (pinch - 1.0).abs() > 0.002 { zoom_factor *= pinch as f32; }
                }
                // Mouse wheel Y scroll as zoom (use moderate sensitivity)
                if pointer_over_canvas && !mouse_over_scope {
                    let scroll_y = i.smooth_scroll_delta.y as f32;
                    if scroll_y.abs() > 0.5 {
                        // convert wheel delta to multiplicative factor (~ 120 per notch typical)
                        let f: f32 = 1.0 + (scroll_y / 480.0);
                        zoom_factor *= f.clamp(0.5, 1.5);
                    }
                }
                if (zoom_factor - 1.0).abs() > 0.0001 {
                    let old_zoom = self.camera_zoom;
                    let new_zoom = (self.camera_zoom * zoom_factor).clamp(0.25, 4.0);
                    if (new_zoom - old_zoom).abs() > 1e-6 {
                        // Focal point calculation: determine which point on screen should stay stationary.
                        // Reconstruct the reference center used in projection logic.
                        let top = panel_rect.top() + 30.0;
                        let bottom = panel_rect.bottom() - 150.0;
                        let height = (bottom - top).max(100.0);
                        let y_ref = top + height * 0.5;
                        let x_ref = panel_rect.center().x;

                        // AARNN layout uses (x_ref, y_ref) as the projection origin (pivot).
                        // Conventional Grid layout uses (0,0) as the projection origin.
                        let is_aarnn = matches!(self.network_layout, NetworkLayout::Aarnn);
                        let is_topo_active = is_aarnn && (!self.hidden_positions.is_empty() || self.local_net.growth_enabled);
                        let offset = if is_topo_active { egui::vec2(x_ref, y_ref) } else { egui::vec2(0.0, 0.0) };

                        // Focal point for zoom (screen coordinates).
                        // Priority: 1. Mouse position if inside the panel. 2. Current network center.
                        let current_network_center = if is_topo_active {
                            egui::pos2(x_ref + self.cam_pan.x, y_ref + self.cam_pan.y)
                        } else {
                            egui::pos2(x_ref * old_zoom + self.cam_pan.x, y_ref * old_zoom + self.cam_pan.y)
                        };

                        let focal_point = i.pointer.hover_pos()
                            .filter(|p| panel_rect.contains(*p))
                            .unwrap_or(current_network_center);

                        // Keep focal_point stationary under zoom by adjusting pan.
                        // Stationary point screen formula: s = offset + world * zoom + pan
                        // pan' = (s - offset) * (1 - f) + pan * f, where f = new_zoom / old_zoom
                        let f = new_zoom / old_zoom.max(1e-6);
                        self.cam_pan = (focal_point.to_vec2() - offset) * (1.0 - f) + self.cam_pan * f;

                        self.camera_zoom = new_zoom;
                        cam_changed = true;
                    }
                }

                // Pan and rotate with mouse drag
                if response.dragged() {
                    let delta = i.pointer.delta();
                    if i.modifiers.ctrl || i.modifiers.command {
                        // Pan: ctrl+drag
                        self.cam_pan += delta;
                        cam_changed = true;
                    } else {
                        // Rotate: left-drag (default)
                        #[cfg(feature = "growth3d")]
                        {
                            if matches!(self.network_layout, NetworkLayout::Aarnn) {
                                self.camera_yaw_degrees = (self.camera_yaw_degrees + delta.x * 0.15).clamp(-80.0, 80.0);
                                self.camera_pitch_degrees = (self.camera_pitch_degrees - delta.y * 0.10).clamp(-60.0, 60.0);
                                cam_changed = true;
                            } else {
                                self.cam_pan += delta;
                                cam_changed = true;
                            }
                        }
                        #[cfg(not(feature = "growth3d"))]
                        {
                            // If rotation unsupported, fall back to panning
                            self.cam_pan += delta;
                            cam_changed = true;
                        }
                    }
                }
            });
            // Double-click to reset view
            if response.double_clicked() {
                self.camera_zoom = 1.0; self.camera_yaw_degrees = 0.0; self.camera_pitch_degrees = 0.0; self.cam_pan = egui::vec2(0.0, 0.0);
                cam_changed = true;
            }
            if cam_changed { self.status = "View updated".into(); }
            let camera_interacting = response.dragged();


            // --- 2. Layout recomputation (mutates self positions) ---
            let runner_arc_for_guard = self.runner.clone();
            let mut managed_net_arc_for_guard = None;
            let mut managed_net_guard: Option<tokio::sync::RwLockReadGuard<'_, ManagedNetwork>> = None;
            let mut standalone_guard: Option<tokio::sync::RwLockReadGuard<'_, Runner>> = None;
            let mut runner_busy = false;
            let active_runner_opt: Option<&Runner> = match &self.view_source {
                ViewSource::Standalone => {
                    if let Ok(guard) = runner_arc_for_guard.try_read() {
                        standalone_guard = Some(guard);
                        Some(&*standalone_guard.as_ref().unwrap())
                    } else {
                        runner_busy = true;
                        None
                    }
                },
                ViewSource::ClusterGlobal(_) => None,
                ViewSource::LocalManaged(id) => {
                    if let Some(state_arc) = state_arc_for_layout.as_ref() {
                        if let Ok(s) = state_arc.try_read() {
                            if let Some(net_arc) = s.networks.get(id) {
                                managed_net_arc_for_guard = Some(net_arc.clone());
                                managed_net_guard = managed_net_arc_for_guard
                                    .as_ref()
                                    .and_then(|arc| arc.try_read().ok());
                                if let Some(ref m) = managed_net_guard {
                                    Some(&m.runner)
                                } else {
                                    runner_busy = true;
                                    None
                                }
                            } else {
                                runner_busy = true;
                                None
                            }
                        } else {
                            runner_busy = true;
                            None
                        }
                    } else {
                        runner_busy = true;
                        None
                    }
                }
            };
            if runner_busy {
                ui.label("Simulation busy...");
            }
            let active_net = active_runner_opt.map(|r| &r.net).unwrap_or(&net_cloned);
            let mut cluster_layout_ns = None;
            let mut cluster_layout_o = None;
            let mut cluster_layout_layers = None;
            let mut cluster_layer_sizes: Option<Vec<usize>> = None;
            let mut cluster_total_neurons = None;
            if let ViewSource::ClusterGlobal(id) = &self.view_source {
                if let Some(net_status) = network_registry.get(id) {
                    let mut cfg_opt: Option<NetworkConfig> = None;
                    if !net_status.config_json.is_empty() {
                        if let Ok(cfg) = serde_json::from_str::<NetworkConfig>(&net_status.config_json) {
                            cfg_opt = Some(cfg);
                        }
                    }
                    let mut layers = net_status.num_layers.max(1) as usize;
                    let mut layer_sizes = vec![0usize; layers];
                    for range in net_status.distribution.values() {
                        for (&layer_idx, &count) in &range.layer_neuron_counts {
                            let li = layer_idx as usize;
                            if li >= layer_sizes.len() {
                                layer_sizes.resize(li + 1, 0);
                                layers = layer_sizes.len();
                            }
                            layer_sizes[li] = layer_sizes[li].saturating_add(count as usize);
                        }
                    }
                    if layer_sizes.iter().all(|&v| v == 0) {
                        if let Some(cfg) = cfg_opt {
                            layers = cfg.num_hidden_layers.max(1);
                            layer_sizes = vec![cfg.num_hidden_per_layer_initial.max(1); layers];
                            cluster_layout_ns = Some(cfg.num_sensory_neurons);
                            cluster_layout_o = Some(cfg.num_output_neurons);
                        }
                    } else if let Some(cfg) = cfg_opt {
                        cluster_layout_ns = Some(cfg.num_sensory_neurons);
                        cluster_layout_o = Some(cfg.num_output_neurons);
                    }
                    cluster_layout_layers = Some(layers);
                    cluster_layer_sizes = Some(layer_sizes);
                    let total = if net_status.total_neurons > 0 {
                        net_status.total_neurons as usize
                    } else {
                        cluster_layer_sizes.as_ref().map(|v| v.iter().sum::<usize>()).unwrap_or(0)
                            + cluster_layout_ns.unwrap_or(0)
                            + cluster_layout_o.unwrap_or(0)
                    };
                    cluster_total_neurons = Some(total);
                }
            }
            let layout_ns = cluster_layout_ns.unwrap_or(active_net.num_sensory_neurons);
            let layout_o = cluster_layout_o.unwrap_or(active_net.num_output_neurons);
            let layout_layers = cluster_layout_layers.unwrap_or(active_net.num_hidden_layers.max(1));
            // In ClusterGlobal mode the snapshot comes from one node only, so
            // cached_layer_sizes reflects a single node's neuron counts rather than the
            // cluster aggregate.  Always bypass the cache path so cluster_layer_sizes
            // (which sums all nodes) is used for layout instead.
            let cache_layout_active = !matches!(self.view_source, ViewSource::ClusterGlobal(_))
                && (self.show_static_overlays || self.force_show_connections)
                && !self.cached_edges.is_empty()
                && !self.cached_layer_sizes.is_empty();
            #[cfg(feature = "growth3d")]
            let use_aarnn_layout = matches!(self.network_layout, NetworkLayout::Aarnn);
            #[cfg(feature = "growth3d")]
            let cache_topology_active =
                use_aarnn_layout && cache_layout_active && self.cached_edge_topo.is_some();
            #[cfg(feature = "growth3d")]
            let ui_snapshot_opt = self.ui_snapshot.try_read().ok().map(|s| s.clone());
            #[cfg(feature = "growth3d")]
            let snapshot_topology_allowed = matches!(self.view_source, ViewSource::Standalone);
            #[cfg(feature = "growth3d")]
            let snapshot_topology_available = ui_snapshot_opt
                .as_ref()
                .map(|snap| {
                    snapshot_topology_allowed
                        && use_aarnn_layout
                        && (!snap.topo_hidden.is_empty()
                            || !snap.topo_sensory.is_empty()
                            || !snap.topo_output.is_empty())
                })
                .unwrap_or(false);
            #[cfg(feature = "growth3d")]
            let prefer_snapshot_topology = use_aarnn_layout
                && snapshot_topology_allowed
                && !cache_topology_active
                && (self.playing || self.ga_running || runner_busy)
                && snapshot_topology_available;
            let mut layer_sizes: Vec<usize> = if cache_layout_active {
                self.cached_layer_sizes.clone()
            } else if let Some(active_runner) = active_runner_opt {
                (0..active_net.num_hidden_layers).map(|li| active_runner.layer_size(li).max(1)).collect()
            } else if let Some(sizes) = cluster_layer_sizes.as_ref() {
                sizes.clone()
            } else if !self.cached_layer_sizes.is_empty() {
                self.cached_layer_sizes.clone()
            } else {
                vec![active_net.num_hidden_per_layer_initial.max(1); layout_layers]
            };
            if layer_sizes.len() != layout_layers {
                layer_sizes.resize(layout_layers, active_net.num_hidden_per_layer_initial.max(1));
            }
            if !layer_sizes.is_empty() && !cache_layout_active {
                self.cached_layer_sizes = layer_sizes.clone();
            }

            // Recompute layout when size, counts, or (in growth mode) topology sizes change
            let mut need_recompute = cam_changed || self.last_rendered_panel_size != panel_rect.size()
                || self.sensory_positions.len() != layout_ns
                || self.hidden_positions.len() != layout_layers
                || self.output_positions.len() != layout_o;
            if !need_recompute {
                for (layer_pos, &target_size) in self.hidden_positions.iter().zip(layer_sizes.iter()) {
                    if layer_pos.len() != target_size {
                        need_recompute = true;
                        break;
                    }
                }
            }
            // Keep last known 3D layout if the runner is busy to avoid falling back to grid layout.
        #[cfg(feature = "growth3d")]
        let growth_check = active_net.growth_enabled;
        #[cfg(not(feature = "growth3d"))]
        let growth_check = false;

            if active_runner_opt.is_none()
                && !matches!(self.view_source, ViewSource::ClusterGlobal(_))
                && matches!(self.network_layout, NetworkLayout::Aarnn)
                && growth_check
            {
                #[cfg(feature = "growth3d")]
                let can_recompute_from_cache = cache_topology_active || snapshot_topology_available;
                #[cfg(not(feature = "growth3d"))]
                let can_recompute_from_cache = false;
                if !can_recompute_from_cache && !self.hidden_positions.is_empty() {
                    need_recompute = false;
                }
            }
            #[cfg(feature = "growth3d")]
            if self.growth_enabled && use_aarnn_layout {
                // Avoid recomputing full topology layout on every frame while dragging.
                if self.playing && !camera_interacting {
                    if self.last_layout_recompute.elapsed() >= std::time::Duration::from_millis(33) {
                        need_recompute = true;
                    }
                }

                // If any hidden layer size differs from topology, recompute
                if cache_topology_active {
                    if let Some(cached_topo) = self.cached_edge_topo.as_ref() {
                        let topo_layers_len = cached_topo.layers.len();
                        if topo_layers_len != self.hidden_positions.len() { need_recompute = true; }
                        else {
                            for li in 0..topo_layers_len {
                                if self.hidden_positions.get(li).map(|v| v.len()).unwrap_or(0) != cached_topo.layers[li].len() {
                                    need_recompute = true; break;
                                }
                            }
                        }
                    }
                } else if prefer_snapshot_topology {
                    if let Some(snap) = ui_snapshot_opt.as_ref() {
                        let topo_layers_len = snap.topo_hidden.len();
                        if topo_layers_len != self.hidden_positions.len() { need_recompute = true; }
                        else {
                            for li in 0..topo_layers_len {
                                if self.hidden_positions.get(li).map(|v| v.len()).unwrap_or(0) != snap.topo_hidden[li].len() {
                                    need_recompute = true; break;
                                }
                            }
                        }
                    }
                } else if let Some(active_runner) = active_runner_opt {
                    let topo_layers_len = active_runner.topo.layers.len();
                    if topo_layers_len != self.hidden_positions.len() { need_recompute = true; }
                    else {
                        for li in 0..topo_layers_len {
                            if self.hidden_positions.get(li).map(|v| v.len()).unwrap_or(0) != active_runner.topo.layers[li].len() {
                                need_recompute = true; break;
                            }
                        }
                    }
                }
            }
            if need_recompute {
                self.last_rendered_panel_size = panel_rect.size();
                self.last_layout_recompute = std::time::Instant::now();
                let ns = layout_ns.max(1);
                let o = layout_o.max(1);
                // compute positions
                let l = layout_layers.max(1);
                let cols = 2 + l; // S + H.. + O
                let gap_scale = if cols > 32 {
                    (32.0 / cols as f32).clamp(0.35, 1.0)
                } else {
                    1.0
                };
                let layout_width = panel_rect.width() * gap_scale;
                let left = panel_rect.center().x - layout_width * 0.5;
                let dx = layout_width / (cols as f32 + 1.0);
                let top = panel_rect.top() + 30.0;
                let bottom = panel_rect.bottom() - 150.0; // leave room for EQ
                let height = (bottom - top).max(100.0);

                // --- Node Layout Calculation ---
                #[cfg(feature = "growth3d")]
                let x_ref = panel_rect.center().x;
                #[cfg(feature = "growth3d")]
                let y_ref = top + height * 0.5;
                #[cfg(feature = "growth3d")]
                let yaw = self.camera_yaw_degrees.to_radians();
                #[cfg(feature = "growth3d")]
                let pitch = self.camera_pitch_degrees.to_radians();
                #[cfg(feature = "growth3d")]
                let (sy, cy) = (yaw.sin(), yaw.cos());
                #[cfg(feature = "growth3d")]
                let (sp, cp) = (pitch.sin(), pitch.cos());
                #[cfg(feature = "growth3d")]
                let zoom = self.camera_zoom;
                #[cfg(feature = "growth3d")]
                let scale_x = panel_rect.width() * 0.3 * zoom;
                #[cfg(feature = "growth3d")]
                let scale_y = height * 0.45 * zoom;
                #[cfg(feature = "growth3d")]
                let cluster_topo_opt = if matches!(self.view_source, ViewSource::ClusterGlobal(_)) {
                    self.cluster_topo_cache.as_ref()
                } else {
                    None
                };
                #[cfg(not(feature = "growth3d"))]
                let _cluster_topo_opt: Option<()> = None;
                #[cfg(feature = "growth3d")]
                let active_runner_topology_available = active_runner_opt
                    .map(|runner| {
                        !runner.topo.layers.is_empty()
                            || !runner.topo.sensory_nodes.is_empty()
                            || !runner.topo.output_nodes.is_empty()
                    })
                    .unwrap_or(false);
                #[cfg(feature = "growth3d")]
                let cluster_topology_available = cluster_topo_opt
                    .map(|topo| {
                        !topo.layers.is_empty()
                            || !topo.sensory_nodes.is_empty()
                            || !topo.output_nodes.is_empty()
                    })
                    .unwrap_or(false);
                #[cfg(feature = "growth3d")]
                let topo_enabled = cache_topology_active
                    || snapshot_topology_available
                    || active_runner_topology_available
                    || cluster_topology_available;

                #[cfg(feature = "growth3d")]
                if use_aarnn_layout {
                    if topo_enabled {
                        // Compute world-space centroid of currently visible neurons and smooth pivot with PID.
                        let mut sumx = 0.0f32;
                        let mut sumy = 0.0f32;
                        let mut sumz = 0.0f32;
                        let mut cnt = 0usize;
                        if cache_topology_active {
                            if let Some(topo) = self.cached_edge_topo.as_ref() {
                                for l in &topo.layers {
                                    for n in l {
                                        sumx += n.x;
                                        sumy += n.y;
                                        sumz += n.z;
                                        cnt += 1;
                                    }
                                }
                                for n in &topo.sensory_nodes {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                                for n in &topo.output_nodes {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                            }
                        } else if prefer_snapshot_topology {
                            if let Some(ref snap) = ui_snapshot_opt {
                                for l in &snap.topo_hidden {
                                    for n in l {
                                        sumx += n.x;
                                        sumy += n.y;
                                        sumz += n.z;
                                        cnt += 1;
                                    }
                                }
                                for n in &snap.topo_sensory {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                                for n in &snap.topo_output {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                            }
                        } else if let Some(active_runner) = active_runner_opt {
                            for l in &active_runner.topo.layers {
                                for n in l {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                            }
                            for n in &active_runner.topo.sensory_nodes {
                                sumx += n.x;
                                sumy += n.y;
                                sumz += n.z;
                                cnt += 1;
                            }
                            for n in &active_runner.topo.output_nodes {
                                sumx += n.x;
                                sumy += n.y;
                                sumz += n.z;
                                cnt += 1;
                            }
                        } else if let Some(topo) = cluster_topo_opt {
                            for l in &topo.layers {
                                for n in l {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                            }
                            for n in &topo.sensory_nodes {
                                sumx += n.x;
                                sumy += n.y;
                                sumz += n.z;
                                cnt += 1;
                            }
                            for n in &topo.output_nodes {
                                sumx += n.x;
                                sumy += n.y;
                                sumz += n.z;
                                cnt += 1;
                            }
                        } else if let Some(ref snap) = ui_snapshot_opt {
                            for l in &snap.topo_hidden {
                                for n in l {
                                    sumx += n.x;
                                    sumy += n.y;
                                    sumz += n.z;
                                    cnt += 1;
                                }
                            }
                            for n in &snap.topo_sensory {
                                sumx += n.x;
                                sumy += n.y;
                                sumz += n.z;
                                cnt += 1;
                            }
                            for n in &snap.topo_output {
                                sumx += n.x;
                                sumy += n.y;
                                sumz += n.z;
                                cnt += 1;
                            }
                        }

                        let dt_s = ctx.input(|i| i.unstable_dt).clamp(0.001, 0.1);
                        if cnt > 0 {
                            let pivot_target = [
                                sumx / (cnt as f32),
                                sumy / (cnt as f32),
                                sumz / (cnt as f32),
                            ];
                            let (pivot_kp, pivot_kd) = if camera_interacting {
                                (0.85, 0.02)
                            } else {
                                (0.24, 0.10)
                            };
                            let pivot_smoothed = pid_smooth_vec3(
                                &mut self.cam_pivot_pid,
                                pivot_target,
                                dt_s,
                                pivot_kp,
                                0.0,
                                pivot_kd,
                            );
                            self.cam_pivot_world =
                                (pivot_smoothed[0], pivot_smoothed[1], pivot_smoothed[2]);
                        }
                        let pivot = self.cam_pivot_world;
                        let cam_pan = self.cam_pan;

                        let project = |n: &crate::topology::Node3D| -> egui::Pos2 {
                            let (mut x3, mut y3, mut z3) =
                                (n.x - pivot.0, n.y - pivot.1, n.z - pivot.2);
                            // Rotate by yaw (around Y) then pitch (around X) around the world pivot.
                            let xz = x3 * cy + z3 * sy;
                            let zz = -x3 * sy + z3 * cy;
                            x3 = xz;
                            z3 = zz;
                            let yz = y3 * cp - z3 * sp;
                            y3 = yz;
                            egui::pos2(
                                x_ref + x3 * scale_x + cam_pan.x,
                                y_ref - y3 * scale_y + cam_pan.y,
                            )
                        };

                        let mut target_sensory_positions: Vec<egui::Pos2> = Vec::new();
                        let mut target_output_positions: Vec<egui::Pos2> = Vec::new();
                        let mut target_hidden_positions: Vec<Vec<egui::Pos2>> = Vec::new();

                        if cache_topology_active {
                            if let Some(topo) = self.cached_edge_topo.as_ref() {
                                target_sensory_positions =
                                    topo.sensory_nodes.iter().map(project).collect();
                                target_output_positions =
                                    topo.output_nodes.iter().map(project).collect();
                                target_hidden_positions = topo
                                    .layers
                                    .iter()
                                    .map(|layer| layer.iter().map(project).collect())
                                    .collect();
                            }
                        } else if prefer_snapshot_topology {
                            if let Some(ref snap) = ui_snapshot_opt {
                                target_sensory_positions =
                                    snap.topo_sensory.iter().map(project).collect();
                                target_output_positions =
                                    snap.topo_output.iter().map(project).collect();
                                target_hidden_positions = snap
                                    .topo_hidden
                                    .iter()
                                    .map(|layer| layer.iter().map(project).collect())
                                    .collect();
                            }
                        } else if let Some(active_runner) = active_runner_opt {
                            target_sensory_positions =
                                active_runner.topo.sensory_nodes.iter().map(project).collect();
                            target_output_positions =
                                active_runner.topo.output_nodes.iter().map(project).collect();
                            target_hidden_positions = active_runner
                                .topo
                                .layers
                                .iter()
                                .map(|layer: &Vec<crate::topology::Node3D>| {
                                    layer.iter().map(project).collect()
                                })
                                .collect();
                        } else if let Some(topo) = cluster_topo_opt {
                            target_sensory_positions =
                                topo.sensory_nodes.iter().map(project).collect();
                            target_output_positions =
                                topo.output_nodes.iter().map(project).collect();
                            target_hidden_positions = topo
                                .layers
                                .iter()
                                .map(|layer| layer.iter().map(project).collect())
                                .collect();
                        } else if let Some(ref snap) = ui_snapshot_opt {
                            target_sensory_positions =
                                snap.topo_sensory.iter().map(project).collect();
                            target_output_positions =
                                snap.topo_output.iter().map(project).collect();
                            target_hidden_positions = snap
                                .topo_hidden
                                .iter()
                                .map(|layer| layer.iter().map(project).collect())
                                .collect();
                        }

                        // Fallback if no hidden layers exist yet.
                        if target_hidden_positions.is_empty() {
                            target_hidden_positions
                                .push(vec![egui::pos2(x_ref + cam_pan.x, y_ref + cam_pan.y)]);
                        }

                        let (pos_kp, pos_kd) = if camera_interacting {
                            (0.9, 0.02)
                        } else {
                            (0.30, 0.10)
                        };
                        self.sensory_positions = pid_smooth_positions(
                            &mut self.topo_pid_sensory,
                            &target_sensory_positions,
                            dt_s,
                            pos_kp,
                            0.0,
                            pos_kd,
                        );
                        self.hidden_positions = pid_smooth_layered_positions(
                            &mut self.topo_pid_hidden,
                            &target_hidden_positions,
                            dt_s,
                            pos_kp,
                            0.0,
                            pos_kd,
                        );
                        self.output_positions = pid_smooth_positions(
                            &mut self.topo_pid_output,
                            &target_output_positions,
                            dt_s,
                            pos_kp,
                            0.0,
                            pos_kd,
                        );
                        let desired_center_2d = egui::pos2(x_ref + cam_pan.x, y_ref + cam_pan.y);
                        let mut centroid_correction = egui::Vec2::ZERO;
                        if let Some(curr_center_2d) = centroid_of_projected_positions(
                            &self.sensory_positions,
                            &self.hidden_positions,
                            &self.output_positions,
                        ) {
                            centroid_correction = desired_center_2d - curr_center_2d;
                            if centroid_correction.length_sq() > 1e-6 {
                                for p in &mut self.sensory_positions {
                                    *p += centroid_correction;
                                }
                                for layer in &mut self.hidden_positions {
                                    for p in layer {
                                        *p += centroid_correction;
                                    }
                                }
                                for p in &mut self.output_positions {
                                    *p += centroid_correction;
                                }
                            }
                        }

                        // Compute region label positions with smoothing
                        self.region_label_positions.clear();
                        if self.show_region_labels {
                            for region in &net_cloned.brain_regions {
                                let node = crate::topology::Node3D {
                                    x: region.center[0],
                                    y: region.center[1],
                                    z: region.center[2],
                                    ..Default::default()
                                };
                                let name = region.name.clone();
                                let target_raw = project(&node) + centroid_correction;
                                let target_entry = self
                                    .region_label_target_states
                                    .entry(name.clone())
                                    .or_insert(target_raw);
                                let target_tau_s = if camera_interacting { 0.10 } else { 0.26 };
                                let target_alpha = 1.0 - (-dt_s / target_tau_s).exp();
                                target_entry.x += (target_raw.x - target_entry.x) * target_alpha;
                                target_entry.y += (target_raw.y - target_entry.y) * target_alpha;
                                let target_pos = *target_entry;

                                // Keep the label on a stable side of its region target (sticky offset)
                                // and move it with a speed cap to prevent visual jitter.
                                let center_2d = desired_center_2d;
                                let mut center_dir = target_pos - center_2d;
                                if center_dir.length_sq() < 1.0 {
                                    center_dir = egui::vec2(1.0, 0.0);
                                }
                                let label_distance = 35.0f32;
                                let initial_label_pos =
                                    target_pos + center_dir.normalized() * label_distance;
                                let label_entry = self
                                    .region_label_states
                                    .entry(name.clone())
                                    .or_insert(initial_label_pos);
                                let mut sticky_dir = *label_entry - target_pos;
                                if sticky_dir.length_sq() < 64.0 {
                                    sticky_dir = center_dir;
                                }
                                if sticky_dir.length_sq() < 1.0 {
                                    sticky_dir = egui::vec2(1.0, 0.0);
                                }
                                let desired_label_pos =
                                    target_pos + sticky_dir.normalized() * label_distance;
                                let label_tau_s = if camera_interacting { 0.09 } else { 0.34 };
                                let label_alpha = 1.0 - (-dt_s / label_tau_s).exp();
                                let mut step = (desired_label_pos - *label_entry) * label_alpha;
                                let max_step = if camera_interacting {
                                    220.0 * dt_s
                                } else {
                                    85.0 * dt_s
                                };
                                let step_len_sq = step.length_sq();
                                if step_len_sq > max_step * max_step {
                                    step *= max_step / step_len_sq.sqrt();
                                }
                                *label_entry += step;

                                self.region_label_positions.push((name, *label_entry, target_pos));
                            }
                        }
                    } else {
                        // Grid layout when growth is disabled (apply camera transform)
                        self.reset_topology_pid_states();
                        self.sensory_positions = (0..ns).map(|i|{
                            let y = top + height * ((i as f32 + 1.0) / (ns as f32 + 1.0));
                            let px = (left + dx) * self.camera_zoom + self.cam_pan.x;
                            let py = y * self.camera_zoom + self.cam_pan.y;
                            egui::pos2(px, py)
                        }).collect();

                        self.hidden_positions = (0..layout_layers).map(|li|{
                            let x = left + dx * (2.0 + li as f32);
                            let x = x * self.camera_zoom + self.cam_pan.x;
                            let h = layer_sizes.get(li).copied().unwrap_or(1).max(1);
                            (0..h).map(|j|{
                                let y = top + height * ((j as f32 + 1.0) / (h as f32 + 1.0));
                                let y = y * self.camera_zoom + self.cam_pan.y;
                                egui::pos2(x, y)
                            }).collect::<Vec<_>>()
                        }).collect();

                        // Output
                        let x = left + dx * (2.0 + layout_layers as f32);
                        self.output_positions = (0..o).map(|k|{
                            let y = top + height * ((k as f32 + 1.0) / (o as f32 + 1.0));
                            egui::pos2(x * self.camera_zoom + self.cam_pan.x, y * self.camera_zoom + self.cam_pan.y)
                        }).collect();
                    }
                } else {
                    // Grid layout when growth is disabled (apply camera transform)
                    self.reset_topology_pid_states();
                    self.sensory_positions = (0..ns).map(|i|{
                        let y = top + height * ((i as f32 + 1.0) / (ns as f32 + 1.0));
                        let px = (left + dx) * self.camera_zoom + self.cam_pan.x;
                        let py = y * self.camera_zoom + self.cam_pan.y;
                        egui::pos2(px, py)
                    }).collect();

                    self.hidden_positions = (0..layout_layers).map(|li|{
                        let x = left + dx * (2.0 + li as f32);
                        let x = x * self.camera_zoom + self.cam_pan.x;
                        let h = layer_sizes.get(li).copied().unwrap_or(1).max(1);
                        (0..h).map(|j|{
                            let y = top + height * ((j as f32 + 1.0) / (h as f32 + 1.0));
                            let y = y * self.camera_zoom + self.cam_pan.y;
                            egui::pos2(x, y)
                        }).collect::<Vec<_>>()
                    }).collect();

                    // Output
                    let x = left + dx * (2.0 + layout_layers as f32);
                    self.output_positions = (0..o).map(|k|{
                        let y = top + height * ((k as f32 + 1.0) / (o as f32 + 1.0));
                        egui::pos2(x * self.camera_zoom + self.cam_pan.x, y * self.camera_zoom + self.cam_pan.y)
                    }).collect();
                }
                #[cfg(not(feature = "growth3d"))]
                {
                    // Grid layout when growth3d feature not compiled
                    self.sensory_positions = (0..ns).map(|i|{
                        let y = top + height * ((i as f32 + 1.0) / (ns as f32 + 1.0));
                        let px = (left + dx) * self.camera_zoom + self.cam_pan.x;
                        let py = y * self.camera_zoom + self.cam_pan.y;
                        egui::pos2(px, py)
                    }).collect();

                    self.hidden_positions = (0..layout_layers).map(|li|{
                        let x = left + dx * (2.0 + li as f32);
                        let h = layer_sizes.get(li).copied().unwrap_or(1).max(1);
                        (0..h).map(|j|{
                            let y = top + height * ((j as f32 + 1.0) / (h as f32 + 1.0));
                            egui::pos2(x, y)
                        }).collect::<Vec<_>>()
                    }).collect();

                    // Output
                    let x = left + dx * (2.0 + layout_layers as f32);
                    self.output_positions = (0..o).map(|k|{
                        let y = top + height * ((k as f32 + 1.0) / (o as f32 + 1.0));
                        egui::pos2(x * self.camera_zoom + self.cam_pan.x, y * self.camera_zoom + self.cam_pan.y)
                    }).collect();
                }

                // resize activity buffers if needed
                self.sensory_activity.resize(ns, 0.0);
                // Activity buffers for hidden layers
                #[cfg(feature = "growth3d")]
                if use_aarnn_layout && topo_enabled {
                    // Match per-layer sizes to current topology
                    let target_layers = self.hidden_positions.len();
                    self.hidden_activity.resize(target_layers, Vec::new());
                    for (v, layer_pos) in self.hidden_activity.iter_mut().zip(self.hidden_positions.iter()) {
                        let hlen = layer_pos.len();
                        if v.len() != hlen { *v = vec![0.0; hlen.max(1)]; }
                    }
                } else {
                self.hidden_activity.resize(layout_layers, Vec::new());
                    for (li, v) in self.hidden_activity.iter_mut().enumerate() {
                        let h = layer_sizes.get(li).copied().unwrap_or(1).max(1);
                        if v.len() != h { *v = vec![0.0; h]; }
                    }
                }
                self.output_activity.resize(o, 0.0);
                // resize raster history to match output size
                            for col in self.raster_outputs.iter_mut() { col.resize(o, 0); }
                        }

            let mut target_fps = net_cloned.ui_target_fps.max(1.0);
            let mut ui_idle = !self.playing
                && !self.ga_running
                && !self.pending_edge_cache
                && !self.edge_cache_inflight
                && !self.mic_running;
            #[cfg(feature = "webcam_input")]
            {
                ui_idle = ui_idle && !self.cam_running;
            }
            if ui_idle {
                target_fps = target_fps.min(20.0).max(5.0);
            }
            if camera_interacting {
                target_fps = target_fps.max(60.0);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(1.0 / target_fps as f32));

            // --- 3. State extraction for decoupled drawing ---
            let max_highlight_lines: usize = self.max_highlight_lines;
            let layout_total_neurons = cluster_total_neurons.unwrap_or(total_neurons_cloned);
            let large_model = layout_layers > 64 || layout_total_neurons > 5000;
            let allow_cached_edges = !self.cached_edges.is_empty();
            let allow_edges = !large_model || self.force_show_connections || allow_cached_edges;
            let show_highlights: bool = self.show_highlights && allow_edges && !camera_interacting;
            let show_backward_highlights: bool = self.show_backward_highlights && allow_edges && !camera_interacting;
            let show_static_overlays: bool = self.show_static_overlays && allow_edges;
            let live_edge_overlays = show_static_overlays || self.force_show_connections;
            let since_last_edge_refresh_ms = self.last_edge_cache_refresh.elapsed().as_millis() as u64;
            if live_edge_overlays
                && self.overlay_density > 0
                && since_last_edge_refresh_ms >= self.edge_cache_refresh_ms
                && !self.pending_edge_cache
                && !self.edge_cache_inflight
                && !camera_interacting
            {
                self.pending_edge_cache = true;
                self.last_edge_cache_refresh = std::time::Instant::now();
            }
            let overlay_density: usize = self.overlay_density;
            let overlay_opacity: f32 = self.overlay_opacity;
            let show_feedback_overlays: bool = self.show_feedback_overlays && allow_edges;
            let loop_feedback: bool = self.loop_feedback;
            let view_node_filter: Option<String> = self.view_node_filter.clone();
            let view_source: ViewSource = self.view_source.clone();
            let brain_id: String = self.brain_id.clone();

            #[cfg(feature = "growth3d")]
            let growth_enabled: bool = match &view_source {
                ViewSource::Standalone => self.growth_enabled,
                ViewSource::LocalManaged(_) => active_runner_opt
                    .map(|r| r.net.growth_enabled)
                    .unwrap_or(self.growth_enabled),
                ViewSource::ClusterGlobal(_) => self.cluster_snapshot_cache
                    .as_ref()
                    .map(|snap| snap.net.growth_enabled)
                    .unwrap_or(self.growth_enabled),
            };
            #[cfg(not(feature = "growth3d"))]
            let _growth_enabled: bool = false;
            let scope_gain: f32 = self.scope_gain;
            let scope_grid: bool = self.scope_grid;
            let scope_lanes: bool = self.scope_lanes;
            let raster_cols: usize = self.raster_cols;

            #[cfg(feature = "growth3d")]
            let (x_ref, y_ref) = (panel_rect.center().x, (panel_rect.top() + 30.0) + ((panel_rect.bottom() - 150.0).max(100.0) - (panel_rect.top() + 30.0)) * 0.5);
            #[cfg(feature = "growth3d")]
            let yaw = self.camera_yaw_degrees.to_radians();
            #[cfg(feature = "growth3d")]
            let pitch = self.camera_pitch_degrees.to_radians();
            #[cfg(feature = "growth3d")]
            let (sy, cy) = (yaw.sin(), yaw.cos());
            #[cfg(feature = "growth3d")]
            let (sp, cp) = (pitch.sin(), pitch.cos());
            #[cfg(feature = "growth3d")]
            let scale_x = panel_rect.width() * 0.3 * self.camera_zoom;
            #[cfg(feature = "growth3d")]
            let scale_y = ((panel_rect.bottom() - 150.0).max(100.0) - (panel_rect.top() + 30.0)) * 0.45 * self.camera_zoom;

            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let (show_morpho_overlays, morpho_opacity, show_transmissions, transmissions_opacity) = (
                self.show_morpho_overlays && allow_edges,
                self.morpho_opacity,
                self.show_transmissions && allow_edges,
                self.transmissions_opacity
            );
            #[cfg(not(all(feature = "morpho", feature = "growth3d")))]
            let (_show_morpho_overlays, _morpho_opacity, _show_transmissions, _transmissions_opacity) = (false, 0.0f32, false, 0.0f32);

            #[cfg(all(feature = "robot_io", unix))]
            let ipc_mapping = self.ipc_mapping.clone();

            let mut scope_time_ms = self.scope_time_ms;

            let mut tooltip_pinned = self.tooltip_pinned;
            let mut tooltip_pinned_pos = self.tooltip_pinned_pos;
            let mut tooltip_pinned_lines = self.tooltip_pinned_lines.clone();
            let mut tooltip_pinned_target = self.tooltip_pinned_target;
            let mut tooltip_suppression_counter = self.tooltip_suppression_counter;
            let mut next_probe_id = self.next_probe_id;
            let mut probes_local = self.probes.clone();
            let mut selected_neuron_pick = self.selected_neuron_pick;
            let mut show_neuron_detail = self.show_neuron_detail;
            let mut smoothed_equalizer_values = self.smoothed_equalizer_values.clone();

            // Draw network
            let painter = ui.painter_at(panel_rect);
            // reset edge cache for this frame
            let mut edge_shapes_vec = Vec::new();
            // Collect hover tooltip lines to consolidate into a single selectable tooltip
            let mut tooltip_lines: Vec<String> = Vec::new();
            let radius_s = 4.0f32;
            let radius_h = 6.0f32;
            let radius_o = 7.0f32;
            // labels
            painter.text(
                egui::pos2(panel_rect.left() + 8.0, panel_rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                "Network",
                egui::FontId::proportional(16.0),
                egui::Color32::LIGHT_GRAY,
            );

            #[cfg(feature = "growth3d")]
            if self.show_region_labels {
                for (name, label_pos, target_pos) in &self.region_label_positions {
                    // Draw a thin indicator line from smoothed label to actual target segment
                    let dist = label_pos.distance(*target_pos);
                    if dist > 5.0 {
                        painter.line_segment(
                            [*label_pos, *target_pos],
                            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(80)),
                        );
                    }

                    painter.text(
                        *label_pos,
                        egui::Align2::CENTER_CENTER,
                        name,
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_white_alpha(220),
                    );
                }
            }

            // Track hovered target for context menu
            let mut hovered_target: Option<ContextPick> = None;

            {
            let sensory_positions = &self.sensory_positions;
            let hidden_positions = &self.hidden_positions;
            let output_positions = &self.output_positions;
            let sensory_activity = &self.sensory_activity;
            let hidden_activity = &self.hidden_activity;
            let output_activity = &self.output_activity;
            let raster_outputs = &self.raster_outputs;
            let previous_hidden_spikes = &self.previous_hidden_spikes;
            let last_sensory_spikes = &self.last_sensory_spikes;

            // Draw skull membrane (semi-transparent bounding environment)
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let _hidden_layers_len = hidden_positions.len();
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            if growth_enabled {
            let skull_opt = if let Some(active_runner) = active_runner_opt {
                if active_runner.net.use_morphology {
                    active_runner.morph.skull_membrane
                } else {
                    #[cfg(all(feature = "morpho", feature = "growth3d"))]
                    {
                        self.cached_skull_membrane = None;
                    }
                    None
                    }
                } else if matches!(view_source, ViewSource::ClusterGlobal(_)) {
                    self.cluster_snapshot_cache.as_ref().and_then(|snap| {
                        if snap.net.use_morphology {
                            snap.skull_membrane
                        } else {
                            None
                        }
                    })
                } else {
                    None
                };
                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                let mut skull_opt = skull_opt;
                #[cfg(all(feature = "morpho", feature = "growth3d"))]
                {
                    if skull_opt.is_some() {
                        self.cached_skull_membrane = skull_opt;
                    } else if active_runner_opt.is_none() {
                        skull_opt = self.cached_skull_membrane;
                    }
                }

                if let Some(skull) = skull_opt {
                    let pivot = self.cam_pivot_world;

                    // Chaikin's algorithm for polygon smoothing
                    let smooth_polygon = |points: Vec<egui::Pos2>, iterations: usize| -> Vec<egui::Pos2> {
                        if points.len() < 3 { return points; }
                        let mut current = points;
                        for _ in 0..iterations {
                            let mut next = Vec::with_capacity(current.len() * 2);
                            for i in 0..current.len() {
                                let p1 = current[i];
                                let p2 = current[(i + 1) % current.len()];
                                next.push(egui::pos2(0.75 * p1.x + 0.25 * p2.x, 0.75 * p1.y + 0.25 * p2.y));
                                next.push(egui::pos2(0.25 * p1.x + 0.75 * p2.x, 0.25 * p1.y + 0.75 * p2.y));
                            }
                            current = next;
                        }
                        current
                    };

                    // k-NN concave hull (Moreira&Santos). Fallback to convex hull if it fails.
                    fn concave_hull_knn(mut points: Vec<egui::Pos2>, k: usize) -> Vec<egui::Pos2> {
                        if points.len() < 4 { return points; }
                        // Start at left-most (min x, then min y)
                        points.sort_by(|a,b| if a.x==b.x { a.y.partial_cmp(&b.y).unwrap() } else { a.x.partial_cmp(&b.x).unwrap() });
                        let start = points[0];
                        let mut hull: Vec<egui::Pos2> = vec![start];
                        let mut current = start;
                        let mut prev_angle = 0.0_f32; // pointing to +x
                        let mut used: Vec<usize> = Vec::new();

                        // Helper to compute angle from a->b
                        let angle = |a: egui::Pos2, b: egui::Pos2| -> f32 { (b.y - a.y).atan2(b.x - a.x) };
                        let dist2 = |a: egui::Pos2, b: egui::Pos2| -> f32 { let dx=b.x-a.x; let dy=b.y-a.y; dx*dx+dy*dy };

                        let mut remaining: Vec<(usize, egui::Pos2)> = points.iter().copied().enumerate().collect();
                        // Ensure start is removed from remaining
                        remaining.retain(|(_,p)| !(p.x==start.x && p.y==start.y));

                        let mut safe = 0usize;
                        while safe < 10000 {
                            safe += 1;
                            if remaining.is_empty() { break; }
                            // k nearest
                            if remaining.len() > k {
                                remaining.select_nth_unstable_by(k, |a,b| dist2(current, a.1).partial_cmp(&dist2(current, b.1)).unwrap_or(std::cmp::Ordering::Equal));
                            }
                            let take = &remaining[..k.min(remaining.len())];
                            // Pick by smallest right turn from prev_angle
                            let mut best: Option<(usize, egui::Pos2, f32)> = None;
                            for &(idx, p) in take {
                                // avoid last segment intersections rudimentarily by skipping already-used immediate neighbors
                                if used.contains(&idx) { continue; }
                                let ang = angle(current, p);
                                // Normalize turn angle to [0, 2pi)
                                let mut turn = ang - prev_angle;
                                while turn <= -std::f32::consts::PI { turn += 2.0*std::f32::consts::PI; }
                                while turn > std::f32::consts::PI { turn -= 2.0*std::f32::consts::PI; }
                                let score = if turn < 0.0 { turn + 2.0*std::f32::consts::PI } else { turn };
                                match &mut best {
                                    None => best = Some((idx, p, score)),
                                    Some((_,_,bs)) => if score < *bs { *bs = score; best = Some((idx, p, score)); }
                                }
                            }
                            if let Some((best_idx, best_p, best_score)) = best {
                                hull.push(best_p);
                                prev_angle = angle(current, best_p);
                                current = best_p;
                                used.push(best_idx);
                                remaining.retain(|(i,_)| *i != best_idx);
                                if hull.len() > 3 && (best_p.x - start.x).abs() < 1.0 && (best_p.y - start.y).abs() < 1.0 {
                                    break;
                                }
                                // If we are looping strangely, increase neighborhood lazily
                                if best_score > 3.0 { /* very large turn, might be stuck */ }
                            } else {
                                // Failed to find neighbor; bail
                                break;
                            }
                        }
                        if hull.len() >= 3 { hull } else { points }
                    }

                    // If alpha-shape mode is available, render a concave hull of the projected hidden neurons
                    if let Some(_alpha) = skull.alpha_radius {
                        // Gather all hidden positions (already in screen space) into a flat list
                        let mut pts2d: Vec<egui::Pos2> = Vec::new();
                        for layer in hidden_positions.iter() {
                            pts2d.extend(layer.iter().copied());
                        }
                        // Basic dedup and sanity
                        pts2d.sort_by(|a,b| if a.x==b.x { a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal) } else { a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal) });
                        pts2d.dedup_by(|a,b| (a.x-b.x).abs() < 0.5 && (a.y-b.y).abs() < 0.5);

                        let n = pts2d.len();
                        if n >= 3 {
                            // Always compute a convex hull for a faint transparent fill so the membrane is visible
                            let mut pts = pts2d.clone();
                            pts.sort_by(|a,b| if a.x==b.x { a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal) } else { a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal) });
                            let cross = |o: egui::Pos2, a: egui::Pos2, b: egui::Pos2| -> f32 { (a.x - o.x)*(b.y - o.y) - (a.y - o.y)*(b.x - o.x) };
                            let mut lower: Vec<egui::Pos2> = Vec::new();
                            for p in &pts { while lower.len() >= 2 && cross(lower[lower.len()-2], lower[lower.len()-1], *p) <= 0.0 { lower.pop(); } lower.push(*p); }
                            let mut upper: Vec<egui::Pos2> = Vec::new();
                            for p in pts.iter().rev() { while upper.len() >= 2 && cross(upper[upper.len()-2], upper[upper.len()-1], *p) <= 0.0 { upper.pop(); } upper.push(*p); }
                            if !lower.is_empty() { lower.pop(); }
                            if !upper.is_empty() { upper.pop(); }
                            let mut convex = lower; convex.extend_from_slice(&upper);
                            if convex.len() >= 3 {
                                // Inflate polygon outward by approximate node screen radius to avoid cutting through somas
                                let inflate = radius_h.max(4.0);
                                let mut convex_inflated = convex.clone();
                                if convex_inflated.len() >= 3 {
                                    let mut cx = 0.0f32; let mut cy = 0.0f32;
                                    for p in &convex_inflated { cx += p.x; cy += p.y; }
                                    cx /= convex_inflated.len() as f32; cy /= convex_inflated.len() as f32;
                                    for p in &mut convex_inflated {
                                        let vx = p.x - cx; let vy = p.y - cy;
                                        let len = (vx*vx + vy*vy).sqrt();
                                        if len > 1e-3 {
                                            let s = (len + inflate) / len;
                                            p.x = cx + vx * s; p.y = cy + vy * s;
                                        }
                                    }
                                }
                                let convex_smooth = smooth_polygon(convex_inflated, 3);
                                let membrane_fill = egui::Color32::from_rgba_unmultiplied(220, 230, 255, 18); // transparent fill
                                let membrane_stroke_bg = egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(180, 195, 255, 64));
                                painter.add(egui::Shape::convex_polygon(convex_smooth, membrane_fill, membrane_stroke_bg));
                            }

                            // Concave outline on top for shape fidelity; fall back to convex outline if needed
                            let k = ((n as f32).sqrt() as usize).clamp(3, 25);
                            let now = std::time::Instant::now();
                            let force_recompute = self.last_hull_update.elapsed() > std::time::Duration::from_millis(300) || self.cached_skull_hull.is_empty();

                            if force_recompute {
                                self.cached_skull_hull = concave_hull_knn(pts2d.clone(), k);
                                self.last_hull_update = now;
                            }
                            let hull = &self.cached_skull_hull;
                            let mut outline = if hull.len() >= 3 { hull.clone() } else { convex };
                            if outline.len() >= 3 {
                                // Inflate outline by the same amount
                                let inflate = radius_h.max(4.0);
                                let mut cx = 0.0f32; let mut cy = 0.0f32;
                                for p in &outline { cx += p.x; cy += p.y; }
                                cx /= outline.len() as f32; cy /= outline.len() as f32;
                                for p in &mut outline {
                                    let vx = p.x - cx; let vy = p.y - cy;
                                    let len = (vx*vx + vy*vy).sqrt();
                                    if len > 1e-3 {
                                        let s = (len + inflate) / len;
                                        p.x = cx + vx * s; p.y = cy + vy * s;
                                    }
                                }
                                let outline_smooth = smooth_polygon(outline, 3);
                                let membrane_stroke = egui::Stroke::new(1.6_f32, egui::Color32::from_rgba_unmultiplied(200, 210, 255, 140));
                                painter.add(egui::Shape::closed_line(outline_smooth, membrane_stroke));
                            }
                        }
                    } else if let Some((rx, ry, rz)) = skull.radii {
                        // Sample ellipsoid surface, project to 2D, compute convex hull and draw as vector polygon
                        let cx = skull.center.x - pivot.0;
                        let cyw = skull.center.y - pivot.1;
                        let cz = skull.center.z - pivot.2;
                        let mut pts2d: Vec<egui::Pos2> = Vec::with_capacity(24*12);
                        let n_phi = 24usize; // around
                        let n_theta = 12usize; // pole to pole
                        for ti in 0..=n_theta {
                            let theta = std::f32::consts::PI * (ti as f32) / (n_theta as f32);
                            let st = theta.sin();
                            let ct = theta.cos();
                            for pi in 0..n_phi {
                                let phi = 2.0 * std::f32::consts::PI * (pi as f32) / (n_phi as f32);
                                let sp = phi.sin();
                                let cp_ = phi.cos();
                                let mut x3 = cx + rx * st * cp_;
                                let mut y3 = cyw + ry * st * sp;
                                let mut z3 = cz + rz * ct;
                                // Apply yaw/pitch
                                let xz = x3*cy + z3*sy; let zz = -x3*sy + z3*cy; x3 = xz; z3 = zz;
                                let yz = y3*cp - z3*sp; y3 = yz;
                                pts2d.push(egui::pos2(
                                    x_ref + x3 * scale_x + self.cam_pan.x,
                                    y_ref - y3 * scale_y + self.cam_pan.y,
                                ));
                            }
                        }
                        // 2D monotonic chain convex hull
                        if pts2d.len() >= 3 {
                            pts2d.sort_by(|a,b| if a.x == b.x { a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal) } else { a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal) });
                            let cross = |o: egui::Pos2, a: egui::Pos2, b: egui::Pos2| -> f32 { (a.x - o.x)*(b.y - o.y) - (a.y - o.y)*(b.x - o.x) };
                            let mut lower: Vec<egui::Pos2> = Vec::new();
                            for p in &pts2d { while lower.len() >= 2 && cross(lower[lower.len()-2], lower[lower.len()-1], *p) <= 0.0 { lower.pop(); } lower.push(*p); }
                            let mut upper: Vec<egui::Pos2> = Vec::new();
                            for p in pts2d.iter().rev() { while upper.len() >= 2 && cross(upper[upper.len()-2], upper[upper.len()-1], *p) <= 0.0 { upper.pop(); } upper.push(*p); }
                            if !lower.is_empty() { lower.pop(); }
                            if !upper.is_empty() { upper.pop(); }
                            let mut hull = lower; hull.extend_from_slice(&upper);
                            if hull.len() >= 3 {
                                let hull_smooth = smooth_polygon(hull, 3);
                                let membrane_fill = egui::Color32::from_rgba_unmultiplied(220, 230, 255, 20);
                                let membrane_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(200, 210, 255, 40));
                                painter.add(egui::Shape::convex_polygon(hull_smooth, membrane_fill, membrane_stroke));
                            }
                        }
                    } else {
                        // Fallback: legacy sphere
                        let (mut x3, mut y3, mut z3) = (skull.center.x - pivot.0, skull.center.y - pivot.1, skull.center.z - pivot.2);
                        let xz = x3*cy + z3*sy; let zz = -x3*sy + z3*cy; x3 = xz; z3 = zz;
                        let yz = y3*cp - z3*sp; y3 = yz;
                        let center_proj = egui::pos2(x_ref + x3 * scale_x + self.cam_pan.x, y_ref - y3 * scale_y + self.cam_pan.y);
                        let radius_proj = skull.radius * scale_x.max(scale_y);
                        let membrane_col = egui::Color32::from_rgba_unmultiplied(220, 230, 255, 20);
                        painter.circle_filled(center_proj, radius_proj, membrane_col);
                        painter.circle_stroke(center_proj, radius_proj, egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(200, 210, 255, 40)));
                    }
                }
            }

            // draw sensory (use cached screen-space positions directly; NO extra camera transform here)
            let (col_s_base, vis_s) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, -1, egui::Color32::from_rgb(60, 140, 255), &network_registry);
            if vis_s || view_node_filter.is_none() {
                for (i, &p0) in sensory_positions.iter().enumerate() {
                    let a = sensory_activity.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                    let col = col_s_base.gamma_multiply(0.35 + 0.65 * a);
                    painter.circle_filled(p0, radius_s, col);
                    // tooltip area
                    let r = egui::Rect::from_center_size(p0, egui::vec2(radius_s*2.0, radius_s*2.0));
                    if response.hovered() && r.contains(ui.ctx().pointer_hover_pos().unwrap_or(egui::pos2(-1.0,-1.0))) {
                        hovered_target = Some(ContextPick::Sensory(i));
                        #[cfg_attr(not(all(feature = "robot_io", unix)), allow(unused_mut))]
                        let mut label = format!("S{}", i);
                        #[cfg(all(feature = "robot_io", unix))]
                        if let Some(ref m) = ipc_mapping { label = m.get_sensor_label(i); }
                        tooltip_lines.push(format!("{}  a={:.2}", label, a));
                    }
                }
            }

            if matches!(view_source, ViewSource::ClusterGlobal(_)) {
                if let Some(net_status) = network_registry.get(&brain_id) {
                    let mut node_ids: Vec<_> = net_status.distribution.keys().cloned().collect();
                    node_ids.sort();
                    if !node_ids.is_empty() {
                        let pad = 6.0f32;
                        let line_h = 14.0f32;
                        let box_w = 190.0f32;
                        let box_h = pad * 2.0 + line_h * (node_ids.len() as f32 + 1.0);
                        let x = panel_rect.right() - box_w - 8.0;
                        let y = panel_rect.top() + 28.0;
                        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(box_w, box_h));
                        painter.rect_filled(rect, 6.0, egui::Color32::from_rgba_unmultiplied(20, 20, 25, 200));
                        painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, egui::Color32::from_gray(60)), egui::StrokeKind::Outside);
                        painter.text(
                            egui::pos2(x + pad, y + pad - 1.0),
                            egui::Align2::LEFT_TOP,
                            "Nodes",
                            egui::FontId::proportional(12.0),
                            egui::Color32::from_gray(200),
                        );
                        for (idx, nid) in node_ids.iter().enumerate() {
                            let mut h = 0u64;
                            for b in nid.bytes() { h = h.wrapping_mul(31).wrapping_add(b as u64); }
                            let mut color: egui::Color32 = egui::epaint::Hsva::new((h % 360) as f32 / 360.0, 0.7, 0.85, 1.0).into();
                            let mut text_col = egui::Color32::from_gray(210);
                            if let Some(filter) = view_node_filter.as_ref() {
                                if nid != filter {
                                    color = color.gamma_multiply(0.35);
                                    text_col = egui::Color32::from_gray(120);
                                }
                            }
                            let row_y = y + pad + line_h * (idx as f32 + 1.0);
                            let swatch = egui::Rect::from_min_size(egui::pos2(x + pad, row_y + 2.0), egui::vec2(10.0, 10.0));
                            painter.rect_filled(swatch, 2.0, color);
                            painter.text(
                                egui::pos2(x + pad + 16.0, row_y),
                                egui::Align2::LEFT_TOP,
                                nid,
                                egui::FontId::proportional(11.0),
                                text_col,
                            );
                        }
                    }
                }
            }

            // draw hidden layers
            for (li, layer) in hidden_positions.iter().enumerate() {
                let (col_h_base, vis_h) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, li as isize, egui::Color32::from_rgb(255, 160, 60), &network_registry);
                if !vis_h && view_node_filter.is_some() { continue; }

                for (j, &p) in layer.iter().enumerate() {
                    // In ClusterGlobal mode activity data is only available for the local
                    // node; remote neurons have no entry in hidden_activity.  Use a 0.5
                    // baseline so all cluster neurons render at a visible brightness rather
                    // than the near-invisible 30% that a 0.0 default produces.
                    let activity_default = if matches!(view_source, ViewSource::ClusterGlobal(_)) { 0.5 } else { 0.0 };
                    let a = hidden_activity.get(li).and_then(|v| v.get(j)).copied().unwrap_or(activity_default).clamp(0.0, 1.0);
                    #[cfg_attr(not(feature = "growth3d"), allow(unused_mut))]
                    let mut col = col_h_base.gamma_multiply(0.30 + 0.70 * a);
                    #[cfg_attr(not(feature = "growth3d"), allow(unused_mut))]
                    let mut r_h = radius_h;
                    #[cfg(feature = "growth3d")]
                    if growth_enabled && use_aarnn_layout {
                        let depth_node_opt: Option<&crate::topology::Node3D> = if cache_topology_active {
                            self.cached_edge_topo
                                .as_ref()
                                .and_then(|topo| topo.layers.get(li))
                                .and_then(|nodes| nodes.get(j))
                        } else if prefer_snapshot_topology {
                            ui_snapshot_opt
                                .as_ref()
                                .and_then(|snap| snap.topo_hidden.get(li))
                                .and_then(|nodes| nodes.get(j))
                        } else if let Some(active_runner) = active_runner_opt {
                            active_runner.topo.layers.get(li).and_then(|nodes| nodes.get(j))
                        } else {
                            ui_snapshot_opt
                                .as_ref()
                                .and_then(|snap| snap.topo_hidden.get(li))
                                .and_then(|nodes| nodes.get(j))
                        };
                        if let Some(node) = depth_node_opt {
                            let depth = (1.0f32 - (node.z + 1.0f32) * 0.5f32).clamp(0.0, 1.0); // same as project_ortho
                            let scale = 0.85 + 0.30 * (1.0 - depth);
                            r_h *= scale;
                            col = col.gamma_multiply(0.85 + 0.30 * (1.0 - depth));
                        }
                    }
                    painter.circle_filled(p, r_h, col);
                    let r = egui::Rect::from_center_size(p, egui::vec2(radius_h*2.0, radius_h*2.0));
                    if response.hovered() && r.contains(ui.ctx().pointer_hover_pos().unwrap_or(egui::pos2(-1.0,-1.0))) {
                        hovered_target = Some(ContextPick::Hidden(li, j));
                        #[cfg(all(feature = "growth3d"))]
                        if growth_enabled {
                            // Show (x,y, z), EMA rate, cooldown
                            if let Some(active_runner) = active_runner_opt {
                                let (mut rate, mut cool) = (0.0f32, 0.0f32);
                                if li < active_runner.rate_h.len() && j < active_runner.rate_h[li].len() { rate = active_runner.rate_h[li][j]; }
                                if li < active_runner.since_growth_ms.len() && j < active_runner.since_growth_ms[li].len() { cool = active_runner.since_growth_ms[li][j]; }
                                if let Some(nodes) = active_runner.topo.layers.get(li) {
                                    if let Some(node) = nodes.get(j) {
                                        tooltip_lines.push(format!(
                                            "H{}:{}  a={:.2}\n(x,y,z)=({:.2},{:.2},{:.2})  rate={:.3}  since_growth={:.0}ms",
                                            li+1, j, a, node.x, node.y, node.z, rate, cool
                                        ));
                                    } else {
                                        tooltip_lines.push(format!("H{}:{}  a={:.2}", li+1, j, a));
                                    }
                                } else {
                                    tooltip_lines.push(format!("H{}:{}  a={:.2}", li+1, j, a));
                                }
                            } else {
                                tooltip_lines.push(format!("H{}:{}  a={:.2}", li+1, j, a));
                            }
                        } else {
                            tooltip_lines.push(format!("H{}:{}  a={:.2}", li+1, j, a));
                        }
                    }
                }
            }

            // Optional: morphology synapse overlays (uses current 2D positions)
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            if show_morpho_overlays {
                if let Some(active_runner) = active_runner_opt {
                    if !active_runner.net.use_morphology {
                        // No morphology data to draw.
                    } else {
                let mut drawn_count = 0usize;
                let draw_cap = 5000usize;
                let draw_syn = |edge_shapes: &mut Vec<EdgeVisual>, p0: egui::Pos2, bend: Option<egui::Pos2>, p1: egui::Pos2, _color: egui::Color32, label_from: String, label_to: String, w: f32, kind: &'static str, is_longterm: bool| {
                    let mut color = if is_longterm {
                        egui::Color32::from_rgb(0, 255, 128) // Greenish for longterm
                    } else {
                        egui::Color32::from_rgb(255, 128, 0) // Orangeish for new
                    };
                    color = color.gamma_multiply(morpho_opacity.clamp(0.05, 1.0));

                    if let Some(b) = bend {
                        // Use Quadratic Bezier as a B-spline approximation when morphology is selected.
                        // Solve for control point CP such that the curve passes through 'b' at t=0.5.
                        let cp = egui::pos2(2.0 * b.x - 0.5 * p0.x - 0.5 * p1.x, 2.0 * b.y - 0.5 * p0.y - 0.5 * p1.y);
                        painter.add(egui::Shape::QuadraticBezier(egui::epaint::QuadraticBezierShape {
                            points: [p0, cp, p1],
                            closed: false,
                            fill: egui::Color32::TRANSPARENT,
                            stroke: egui::epaint::PathStroke::new(if is_longterm { 1.5 } else { 1.0_f32 }, color),
                        }));
                    } else {
                        painter.line_segment([p0, p1], egui::Stroke { width: if is_longterm { 1.5 } else { 1.0 }, color });
                    }
                    edge_shapes.push(EdgeVisual { p0, p1, from_label: label_from, to_label: label_to, weight: Some(w), kind, is_longterm });
                };
                let l_count_vis = hidden_positions.len();
                let (in_l, out_l) = active_runner.get_io_layers();
                for syn in &active_runner.morph.synapses {
                    if drawn_count >= draw_cap { break; }
                    match syn.kind {
                        SynKind::In => {
                            // pre_layer: -1 (sensory), post_layer: 0
                            let i = syn.pre_id; let j = syn.post_id;
                            let l = if syn.post_layer >= 0 { syn.post_layer as usize } else { continue };
                            if l != in_l { continue; }
                            if l < hidden_positions.len()
                                && j < hidden_positions[l].len()
                                && i < sensory_positions.len()
                                && j < active_runner.w_in.nrows()
                                && i < active_runner.w_in.ncols() {
                                let w = active_runner.w_in[(j, i)];
                                if w.abs() <= 1.0e-8 { continue; }
                                let p0 = sensory_positions[i];
                                let p1 = hidden_positions[l][j];
                                let bend = syn.bend.as_ref().map(|_| {
                                    // screen-space midpoint offset (visual only)
                                    let mid = egui::pos2((p0.x + p1.x)*0.5, (p0.y + p1.y)*0.5);
                                    let off = egui::vec2( (p1.y - p0.y)*0.03, -(p1.x - p0.x)*0.03 );
                                    mid + off
                                });
                                let is_lt = active_runner.is_longterm_in(j, i);
                                draw_syn(&mut edge_shapes_vec, p0, bend, p1, egui::Color32::from_rgb(110, 170, 255), format!("S{}", i), format!("H{}:{}", l+1, j), w as f32, "morph_in", is_lt);
                                drawn_count += 1;
                            }
                        }
                        SynKind::HiddenFwd => {
                            let l = syn.pre_layer as usize; // pre: l, post: l+1
                            let i = syn.pre_id; let j = syn.post_id;
                            if l < l_count_vis && (l+1) < l_count_vis {
                                if i < hidden_positions[l].len()
                                    && j < hidden_positions[l+1].len()
                                    && l < active_runner.w_hh_fwd.len()
                                    && j < active_runner.w_hh_fwd[l].nrows()
                                    && i < active_runner.w_hh_fwd[l].ncols() {
                                    let w = active_runner.w_hh_fwd[l][(j, i)];
                                    if w.abs() <= 1.0e-8 { continue; }
                                    let p0 = hidden_positions[l][i]; let p1 = hidden_positions[l+1][j];
                                    let bend = syn.bend.as_ref().map(|_| {
                                        let mid = egui::pos2((p0.x + p1.x)*0.5, (p0.y + p1.y)*0.5);
                                        let off = egui::vec2( (p1.y - p0.y)*0.03, -(p1.x - p0.x)*0.03 );
                                        mid + off
                                    });
                                    let is_lt = active_runner.is_longterm_fwd(l, j, i);
                                    draw_syn(&mut edge_shapes_vec, p0, bend, p1, egui::Color32::from_rgb(255, 190, 80), format!("H{}:{}", l+1, i), format!("H{}:{}", l+2, j), w as f32, "morph_fwd", is_lt);
                                    drawn_count += 1;
                                }
                            }
                        }
                        SynKind::HiddenBwd => {
                            let l = syn.post_layer as usize; // post: l, pre: l+1
                            let i = syn.post_id; let j = syn.pre_id;
                            if l < l_count_vis && (l+1) < l_count_vis {
                                if i < hidden_positions[l].len()
                                    && j < hidden_positions[l+1].len()
                                    && l < active_runner.w_hh_bwd.len()
                                    && i < active_runner.w_hh_bwd[l].nrows()
                                    && j < active_runner.w_hh_bwd[l].ncols() {
                                    let w = active_runner.w_hh_bwd[l][(i, j)];
                                    if w.abs() <= 1.0e-8 { continue; }
                                    let p0 = hidden_positions[l+1][j]; let p1 = hidden_positions[l][i];
                                    let bend = syn.bend.as_ref().map(|_| {
                                        let mid = egui::pos2((p0.x + p1.x)*0.5, (p0.y + p1.y)*0.5);
                                        let off = egui::vec2( (p1.y - p0.y)*0.03, -(p1.x - p0.x)*0.03 );
                                        mid + off
                                    });
                                    let is_lt = active_runner.is_longterm_bwd(l, i, j);
                                    draw_syn(&mut edge_shapes_vec, p0, bend, p1, egui::Color32::from_rgb(255, 120, 160), format!("H{}:{}", l+2, j), format!("H{}:{}", l+1, i), w as f32, "morph_bwd", is_lt);
                                    drawn_count += 1;
                                }
                            }
                        }
                        SynKind::HiddenRec => {
                            let l = syn.pre_layer as usize;
                            let i = syn.pre_id; let j = syn.post_id;
                            if l < l_count_vis {
                                if i < hidden_positions[l].len()
                                    && j < hidden_positions[l].len()
                                    && l < active_runner.w_hh_rec.len()
                                    && j < active_runner.w_hh_rec[l].nrows()
                                    && i < active_runner.w_hh_rec[l].ncols() {
                                    let w = active_runner.w_hh_rec[l][(j, i)];
                                    if w.abs() <= 1.0e-8 { continue; }
                                    let p0 = hidden_positions[l][i]; let p1 = hidden_positions[l][j];
                                    let bend = syn.bend.as_ref().map(|_| {
                                        let mid = egui::pos2((p0.x + p1.x)*0.5, (p0.y + p1.y)*0.5);
                                        let off = egui::vec2( (p1.y - p0.y)*0.03, -(p1.x - p0.x)*0.03 );
                                        mid + off
                                    });
                                    let is_lt = active_runner.is_longterm_rec(l, j, i);
                                    draw_syn(&mut edge_shapes_vec, p0, bend, p1, egui::Color32::from_rgb(200, 200, 255), format!("H{}:{}", l+1, i), format!("H{}:{}", l+1, j), w as f32, "morph_rec", is_lt);
                                    drawn_count += 1;
                                }
                            }
                        }
                        SynKind::Out => {
                            // pre: last hidden, post: output
                            if l_count_vis > 0 {
                                let j = syn.pre_id; let k = syn.post_id;
                                let l = if syn.pre_layer >= 0 { syn.pre_layer as usize } else { continue };
                                if l != out_l { continue; }
                                if l < hidden_positions.len()
                                    && j < hidden_positions[l].len()
                                    && k < output_positions.len()
                                    && k < active_runner.w_out.nrows()
                                    && j < active_runner.w_out.ncols() {
                                    let w = active_runner.w_out[(k, j)];
                                    if w.abs() <= 1.0e-8 { continue; }
                                    let p0 = hidden_positions[l][j]; let p1 = output_positions[k];
                                    let bend = syn.bend.as_ref().map(|_| {
                                        let mid = egui::pos2((p0.x + p1.x)*0.5, (p0.y + p1.y)*0.5);
                                        let off = egui::vec2( (p1.y - p0.y)*0.03, -(p1.x - p0.x)*0.03 );
                                        mid + off
                                    });
                                    let is_lt = active_runner.is_longterm_out(k, j);
                                    draw_syn(&mut edge_shapes_vec, p0, bend, p1, egui::Color32::from_rgb(160, 240, 120), format!("H{}:{}", l+1, j), format!("O{}", k), w as f32, "morph_out", is_lt);
                                    drawn_count += 1;
                                }
                            }
                        }
                    }
                }
                if drawn_count >= draw_cap {
                    painter.text(
                        egui::pos2(panel_rect.left() + 8.0, panel_rect.top() + 26.0),
                        egui::Align2::LEFT_TOP,
                        format!("(Display capped at {}/{} synapses)", draw_cap, active_runner.morph.synapses.len()),
                        egui::FontId::proportional(12.0),
                        egui::Color32::YELLOW,
                    );
                }
                    }
                }
            }

            // Optional: transmission flashes (released synapses this frame)
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            if show_transmissions {
                if let Some(active_runner) = active_runner_opt {
                    if active_runner.net.use_morphology {
                use crate::morphology::ReleasedKind;
                let alpha = transmissions_opacity.clamp(0.1, 1.0);
                let col_in = egui::Color32::from_rgb(110, 170, 255).gamma_multiply(alpha);
                let col_fwd = egui::Color32::from_rgb(255, 190, 80).gamma_multiply(alpha);
                let col_out = egui::Color32::from_rgb(160, 240, 120).gamma_multiply(alpha);
                let mut drawn = 0usize;
                for evt in &active_runner.released_events {
                    if drawn >= 256 { break; }
                    match evt.kind {
                        ReleasedKind::In => {
                            let i = evt.pre_id; let j = evt.post_id;
                            if j < hidden_positions.get(0).map(|v| v.len()).unwrap_or(0) && i < sensory_positions.len() {
                                painter.line_segment([sensory_positions[i], hidden_positions[0][j]], egui::Stroke { width: 2.0, color: col_in });
                                drawn += 1;
                            }
                        }
                        ReleasedKind::Fwd { layer } => {
                            let l = layer;
                            if l < hidden_positions.len() && (l+1) < hidden_positions.len() {
                                let i = evt.pre_id; let j = evt.post_id;
                                if i < hidden_positions[l].len() && j < hidden_positions[l+1].len() {
                                    painter.line_segment([hidden_positions[l][i], hidden_positions[l+1][j]], egui::Stroke { width: 2.0, color: col_fwd });
                                    drawn += 1;
                                }
                            }
                        }
                        ReleasedKind::Bwd { layer } => {
                            let l = layer; // post: l, pre: l+1
                            if l < hidden_positions.len() && (l+1) < hidden_positions.len() {
                                let j = evt.pre_id; let i = evt.post_id; // pre: j in l+1, post: i in l
                                if j < hidden_positions[l+1].len() && i < hidden_positions[l].len() {
                                    painter.line_segment([hidden_positions[l+1][j], hidden_positions[l][i]], egui::Stroke { width: 2.0, color: egui::Color32::from_rgb(255, 120, 160).gamma_multiply(alpha) });
                                    drawn += 1;
                                }
                            }
                        }
                        ReleasedKind::HiddenRec { layer } => {
                            let l = layer;
                            if l < hidden_positions.len() {
                                let i = evt.pre_id; let j = evt.post_id;
                                if i < hidden_positions[l].len() && j < hidden_positions[l].len() {
                                    painter.line_segment([hidden_positions[l][i], hidden_positions[l][j]], egui::Stroke { width: 2.0, color: egui::Color32::from_rgb(200, 200, 255).gamma_multiply(alpha) });
                                    drawn += 1;
                                }
                            }
                        }
                        ReleasedKind::Out => {
                            let llast = hidden_positions.len().saturating_sub(1);
                            if !hidden_positions.is_empty() {
                                let j = evt.pre_id; let k = evt.post_id;
                                if j < hidden_positions[llast].len() && k < output_positions.len() {
                                    painter.line_segment([hidden_positions[llast][j], output_positions[k]], egui::Stroke { width: 2.0, color: col_out });
                                    drawn += 1;
                                }
                            }
                        }
                    }
                }
                    }
                }
            }

            // draw output
            let (col_o_base, vis_o) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, hidden_positions.len() as isize, egui::Color32::from_rgb(80, 220, 120), &network_registry);
            if vis_o || view_node_filter.is_none() {
                for (k, &p) in output_positions.iter().enumerate() {
                    let a = output_activity.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                    let col = col_o_base.gamma_multiply(0.30 + 0.70 * a);
                    painter.circle_filled(p, radius_o, col);
                    let r = egui::Rect::from_center_size(p, egui::vec2(radius_o*2.4, radius_o*2.4));
                    if response.hovered() && r.contains(ui.ctx().pointer_hover_pos().unwrap_or(egui::pos2(-1.0,-1.0))) {
                        hovered_target = Some(ContextPick::Output(k));
                        #[cfg_attr(not(all(feature = "robot_io", unix)), allow(unused_mut))]
                        let mut label = format!("O{}", k);
                        #[cfg(all(feature = "robot_io", unix))]
                        if let Some(ref m) = ipc_mapping { label = m.get_actuator_label(k); }
                        tooltip_lines.push(format!("{}  a={:.2}", label, a));
                    }
                }
            }

            // draw highlighted connections between active sender/receiver nodes
            if show_highlights {
                let use_smoothed_cached_highlights = (self.playing || self.ga_running || runner_busy) && allow_cached_edges;
                if use_smoothed_cached_highlights {
                    let get_pos = |layer: i32, idx: usize| -> Option<egui::Pos2> {
                        if layer == -1 {
                            sensory_positions.get(idx).copied()
                        } else if layer == -2 {
                            output_positions.get(idx).copied()
                        } else if layer >= 0 {
                            hidden_positions.get(layer as usize).and_then(|v| v.get(idx)).copied()
                        } else {
                            None
                        }
                    };
                    let get_act = |layer: i32, idx: usize| -> f32 {
                        if layer == -1 {
                            sensory_activity.get(idx).copied().unwrap_or(0.0).clamp(0.0, 1.0)
                        } else if layer == -2 {
                            output_activity.get(idx).copied().unwrap_or(0.0).clamp(0.0, 1.0)
                        } else if layer >= 0 {
                            hidden_activity
                                .get(layer as usize)
                                .and_then(|v| v.get(idx))
                                .copied()
                                .unwrap_or(0.0)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    };
                    let mut drawn = 0usize;
                    let draw_cap = max_highlight_lines.max(64) * 24;
                    for edge in &self.cached_edges {
                        if edge.kind == "bwd" && !show_backward_highlights {
                            continue;
                        }
                        let Some(p0) = get_pos(edge.from_layer, edge.from_idx) else { continue; };
                        let Some(p1) = get_pos(edge.to_layer, edge.to_idx) else { continue; };
                        let act = ((get_act(edge.from_layer, edge.from_idx) + get_act(edge.to_layer, edge.to_idx)) * 0.5).clamp(0.0, 1.0);
                        if act < 0.08 {
                            continue;
                        }
                        let abs_w = edge.weight.abs().min(1.0);
                        let ww = (0.8 + 2.6 * abs_w) * (0.45 + 0.55 * act);
                        let base_col = if edge.is_longterm {
                            egui::Color32::from_rgb(0, 255, 128)
                        } else {
                            egui::Color32::from_rgb(255, 128, 0)
                        };
                        let col_line = base_col.gamma_multiply(0.15 + 0.85 * act);
                        painter.line_segment([p0, p1], egui::Stroke { width: ww + 1.2, color: col_line.gamma_multiply(0.2) });
                        painter.line_segment([p0, p1], egui::Stroke { width: ww, color: col_line });
                        let from_label = match edge.from_layer {
                            -1 => format!("S{}", edge.from_idx),
                            -2 => format!("O{}", edge.from_idx),
                            l => format!("H{}:{}", l + 1, edge.from_idx),
                        };
                        let to_label = match edge.to_layer {
                            -1 => format!("S{}", edge.to_idx),
                            -2 => format!("O{}", edge.to_idx),
                            l => format!("H{}:{}", l + 1, edge.to_idx),
                        };
                        edge_shapes_vec.push(EdgeVisual {
                            p0,
                            p1,
                            from_label,
                            to_label,
                            weight: Some(edge.weight),
                            kind: edge.kind,
                            is_longterm: edge.is_longterm,
                        });
                        drawn += 1;
                        if drawn >= draw_cap {
                            break;
                        }
                    }
                } else if let Some(active_runner) = active_runner_opt {
                    // helper: draw up to K strongest incoming links from active senders to an active receiver set
                    let draw_links = |painter: &egui::Painter, edge_shapes: &mut Vec<EdgeVisual>, send_pos: &Vec<egui::Pos2>, recv_pos: &Vec<egui::Pos2>, send_mask: &[i8], recv_mask: &[i8], weights: &ndarray::Array2<f64>, _color: egui::Color32, kind: &'static str, label_from: &str, label_to: &str, vis_from: bool, vis_to: bool, check_lt: Box<dyn Fn(usize, usize) -> bool>| {
                        if !vis_from && !vis_to && view_node_filter.is_some() { return; }
                        let kmax = max_highlight_lines.max(1);
                        let ns = send_pos.len().min(send_mask.len());
                        let nr = recv_pos.len().min(recv_mask.len());
                        for r in 0..nr {
                            if recv_mask[r] == 0 { continue; }
                            // collect (i, w) for active senders; keep top-k in a small vector
                            let mut best: Vec<(usize, f32)> = Vec::new();
                            for i in 0..ns {
                                if send_mask[i] == 0 { continue; }
                                let w = *weights.get((r, i)).unwrap_or(&0.0) as f32;
                                if w.abs() <= 1e-8 { continue; }
                                if best.len() < kmax { best.push((i, w)); }
                                else {
                                    // replace smallest by ABSOLUTE weight
                                    let mut min_idx = 0usize; let mut min_w = best[0].1.abs();
                                    for (bi, &(_, bw)) in best.iter().enumerate().skip(1) { if bw.abs() < min_w { min_w = bw.abs(); min_idx = bi; } }
                                    if w.abs() > min_w { best[min_idx] = (i, w); }
                                }
                            }
                            // draw them
                            for (i, w) in best.into_iter() {
                                let p0 = send_pos[i];
                                let p1 = recv_pos[r];
                                let is_longterm = check_lt(r, i);

                                // width/alpha from weight (absolute)
                                let abs_w = w.abs();
                                let ww = (1.0 + 3.0 * abs_w).clamp(1.0, 4.0) * (if is_longterm { 1.3 } else { 1.0 });

                                let mut base_col = if is_longterm {
                                    egui::Color32::from_rgb(0, 255, 128) // Greenish for longterm
                                } else {
                                    egui::Color32::from_rgb(255, 128, 0) // Orangeish for new
                                };

                                if !vis_from || !vis_to { base_col = base_col.gamma_multiply(0.2); }
                                let col_line = base_col.gamma_multiply((0.3 + 0.7 * abs_w).clamp(0.3, 1.0));

                                // subtle glow
                                painter.line_segment([p0, p1], egui::Stroke { width: ww + 1.5, color: col_line.gamma_multiply(0.25) });
                                painter.line_segment([p0, p1], egui::Stroke { width: ww, color: col_line });
                                // record for tooltip
                                edge_shapes.push(EdgeVisual{
                                    p0, p1,
                                    from_label: format!("{}{}", label_from, i),
                                    to_label: format!("{}{}", label_to, r),
                                    weight: Some(w),
                                    kind,
                                    is_longterm,
                                });
                            }
                        }
                    };

                    let (in_l, out_l) = active_runner.get_io_layers();

                    // S -> H(in_l)
                    if in_l < hidden_positions.len() && !sensory_positions.is_empty() {
                        let (_, vis_h_in) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, in_l as isize, egui::Color32::TRANSPARENT, &network_registry);
                        let recv_mask = active_runner.last_spk_h.get(in_l).map(|a: &ndarray::Array1<i8>| a.as_slice().unwrap_or(&[])).unwrap_or(&[]);
                        draw_links(&painter, &mut edge_shapes_vec, sensory_positions, &hidden_positions[in_l], last_sensory_spikes, recv_mask, &active_runner.w_in, egui::Color32::from_rgb(120, 170, 255), "in", "S", &format!("H{}:", in_l+1), vis_s, vis_h_in, Box::new(|r, i| active_runner.is_longterm_in(r, i)));
                    }
                    // H(l-1) -> H(l)
                    for l in 1..hidden_positions.len() {
                        let (_, vis_prev) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize - 1, egui::Color32::TRANSPARENT, &network_registry);
                        let (_, vis_curr) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize, egui::Color32::TRANSPARENT, &network_registry);

                        let send_mask = active_runner.last_spk_h.get(l-1).map(|a: &ndarray::Array1<i8>| a.as_slice().unwrap_or(&[])).unwrap_or(&[]);
                        let recv_mask = active_runner.last_spk_h.get(l).map(|a: &ndarray::Array1<i8>| a.as_slice().unwrap_or(&[])).unwrap_or(&[]);
                        if let Some(w) = active_runner.w_hh_fwd.get(l-1) {
                            draw_links(&painter, &mut edge_shapes_vec, &hidden_positions[l-1], &hidden_positions[l], send_mask, recv_mask, w, egui::Color32::from_rgb(255, 190, 80), "fwd", &format!("H{}:", l), &format!("H{}:", l+1), vis_prev, vis_curr, Box::new(move |r, i| active_runner.is_longterm_fwd(l-1, r, i)));
                        }
                    }
                    // H(l) -> H(l) (recurrent)
                    for l in 0..hidden_positions.len() {
                        let (_, vis_h) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize, egui::Color32::TRANSPARENT, &network_registry);
                        let spk_mask = active_runner.last_spk_h.get(l).map(|a: &ndarray::Array1<i8>| a.as_slice().unwrap_or(&[])).unwrap_or(&[]);
                        if let Some(w) = active_runner.w_hh_rec.get(l) {
                            draw_links(&painter, &mut edge_shapes_vec, &hidden_positions[l], &hidden_positions[l], spk_mask, spk_mask, w, egui::Color32::from_rgb(200, 200, 255), "rec", &format!("H{}:", l+1), &format!("H{}:", l+1), vis_h, vis_h, Box::new(move |r, i| active_runner.is_longterm_rec(l, r, i)));
                        }
                    }
                    // H(out_l) -> O
                    if out_l < hidden_positions.len() {
                        let source_h = &hidden_positions[out_l];
                        let (_, vis_source) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, out_l as isize, egui::Color32::TRANSPARENT, &network_registry);

                        let send_mask = active_runner.last_spk_h.get(out_l).map(|a: &ndarray::Array1<i8>| a.as_slice().unwrap_or(&[])).unwrap_or(&[]);
                        let recv_mask = active_runner.last_spk_o.as_slice().unwrap_or(&[]);
                        draw_links(&painter, &mut edge_shapes_vec, source_h, output_positions, send_mask, recv_mask, &active_runner.w_out, egui::Color32::from_rgb(160, 240, 160), "out", &format!("H{}:", out_l+1), "O", vis_source, vis_o, Box::new(move |k, j| active_runner.is_longterm_out(k, j)));
                    }

                    // Backward connections highlighting: H(l+1 prev) -> H(l current)
                    if show_backward_highlights {
                        let layers = hidden_positions.len();
                        for l in 0..layers.saturating_sub(1) { // for each receiver layer l, senders are layer l+1 from previous step
                            let send_layer = l + 1;
                            let (_, vis_send) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, send_layer as isize, egui::Color32::TRANSPARENT, &network_registry);
                            let (_, vis_recv) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize, egui::Color32::TRANSPARENT, &network_registry);

                            let send_pos = &hidden_positions[send_layer];
                            let recv_pos = &hidden_positions[l];
                            let send_mask: &[i8] = previous_hidden_spikes.get(send_layer).map(|v| v.as_slice()).unwrap_or(&[]);
                            let recv_mask: &[i8] = active_runner.last_spk_h.get(l).map(|a: &ndarray::Array1<i8>| a.as_slice().unwrap_or(&[])).unwrap_or(&[]);
                            if let Some(w) = active_runner.w_hh_bwd.get(l) {
                                draw_links(&painter, &mut edge_shapes_vec, send_pos, recv_pos, send_mask, recv_mask, w, egui::Color32::from_rgb(255, 120, 160), "bwd", &format!("H{}:", l+2), &format!("H{}:", l+1), vis_send, vis_recv, Box::new(move |r, i| active_runner.is_longterm_bwd(l, r, i)));
                            }
                        }
                    }
                }
            }

            // Static connection overlays (faint network skeleton) under nodes.
            // Prefer cached edges so rendering remains stable while simulation threads mutate weights.
            if show_static_overlays {
                let alpha = overlay_opacity.clamp(0.05, 1.0);
                let get_pos = |layer: i32, idx: usize, sensory: &Vec<egui::Pos2>, hidden: &Vec<Vec<egui::Pos2>>, output: &Vec<egui::Pos2>| -> Option<egui::Pos2> {
                    if layer == -1 {
                        sensory.get(idx).copied()
                    } else if layer == -2 {
                        output.get(idx).copied()
                    } else if layer >= 0 {
                        let l = layer as usize;
                        hidden.get(l).and_then(|v| v.get(idx)).copied()
                    } else {
                        None
                    }
                };
                let allow_live_overlay_fallback = !self.playing
                    && !self.ga_running
                    && !runner_busy
                    && !camera_interacting;

                if allow_cached_edges {
                    for edge in &self.cached_edges {
                        let Some(p0) = get_pos(edge.from_layer, edge.from_idx, sensory_positions, hidden_positions, output_positions) else { continue; };
                        let Some(p1) = get_pos(edge.to_layer, edge.to_idx, sensory_positions, hidden_positions, output_positions) else { continue; };
                        let abs_w = edge.weight.abs();
                        let ww = (0.5 + 1.5 * abs_w).clamp(0.3, 1.8) * (if edge.is_longterm { 1.3 } else { 1.0 });
                        let base_col = if edge.is_longterm {
                            egui::Color32::from_rgb(0, 255, 128)
                        } else {
                            egui::Color32::from_rgb(255, 128, 0)
                        };
                        let col = base_col.gamma_multiply(alpha * (0.3 + 0.7 * abs_w.min(1.0)));
                        painter.line_segment([p0, p1], egui::Stroke { width: ww, color: col });
                        let from_label = match edge.from_layer {
                            -1 => format!("S{}", edge.from_idx),
                            -2 => format!("O{}", edge.from_idx),
                            l => format!("H{}:{}", l + 1, edge.from_idx),
                        };
                        let to_label = match edge.to_layer {
                            -1 => format!("S{}", edge.to_idx),
                            -2 => format!("O{}", edge.to_idx),
                            l => format!("H{}:{}", l + 1, edge.to_idx),
                        };
                        edge_shapes_vec.push(EdgeVisual {
                            p0,
                            p1,
                            from_label,
                            to_label,
                            weight: Some(edge.weight),
                            kind: edge.kind,
                            is_longterm: edge.is_longterm,
                        });
                    }
                } else if allow_live_overlay_fallback {
                    if let Some(active_runner) = active_runner_opt {
                    // Warm-up fallback before first cache result arrives.
                    let k = overlay_density;
                    if k > 0 {
                        let draw_overlay = |painter: &egui::Painter, edge_shapes: &mut Vec<EdgeVisual>, send_pos: &Vec<egui::Pos2>, recv_pos: &Vec<egui::Pos2>, weights: &ndarray::Array2<f64>, _color: egui::Color32, kind: &'static str, label_from: &str, label_to: &str, vis_from: bool, vis_to: bool, check_lt: Box<dyn Fn(usize, usize) -> bool>| {
                            if !vis_from && !vis_to && view_node_filter.is_some() { return; }

                            let ns = send_pos.len();
                            let nr = recv_pos.len();
                            for r in 0..nr.min(weights.shape()[0]) {
                                let mut best: Vec<(usize, f32)> = Vec::new();
                                for i in 0..ns.min(weights.shape()[1]) {
                                    let w = *weights.get((r, i)).unwrap_or(&0.0) as f32;
                                    if w.abs() <= 1e-8 { continue; }
                                    if best.len() < k { best.push((i, w)); }
                                    else {
                                        let mut min_idx = 0usize;
                                        let mut min_w = best[0].1.abs();
                                        for (bi, &(_, bw)) in best.iter().enumerate().skip(1) {
                                            if bw.abs() < min_w { min_w = bw.abs(); min_idx = bi; }
                                        }
                                        if w.abs() > min_w { best[min_idx] = (i, w); }
                                    }
                                }
                                for (i, w) in best.into_iter() {
                                    let p0 = send_pos[i];
                                    let p1 = recv_pos[r];
                                    let is_longterm = check_lt(r, i);
                                    let abs_w = w.abs();
                                    let ww = (0.5 + 1.5 * abs_w).clamp(0.3, 1.8) * (if is_longterm { 1.3 } else { 1.0 });
                                    let base_col = if is_longterm {
                                        egui::Color32::from_rgb(0, 255, 128)
                                    } else {
                                        egui::Color32::from_rgb(255, 128, 0)
                                    };
                                    let col = base_col.gamma_multiply(alpha * (0.3 + 0.7 * abs_w.min(1.0)));
                                    painter.line_segment([p0, p1], egui::Stroke { width: ww, color: col });
                                    edge_shapes.push(EdgeVisual {
                                        p0, p1,
                                        from_label: format!("{}{}", label_from, i),
                                        to_label: format!("{}{}", label_to, r),
                                        weight: Some(w),
                                        kind,
                                        is_longterm,
                                    });
                                }
                            }
                        };

                        if !sensory_positions.is_empty() && !hidden_positions.is_empty() {
                            let (_, vis_h0) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, 0, egui::Color32::TRANSPARENT, &network_registry);
                            draw_overlay(&painter, &mut edge_shapes_vec, sensory_positions, &hidden_positions[0], &active_runner.w_in, egui::Color32::from_rgb(120, 170, 255), "overlay", "S", "H1:", vis_s, vis_h0, Box::new(move |r, i| active_runner.is_longterm_in(r, i)));
                        }
                        for l in 1..hidden_positions.len() {
                            let (_, vis_prev) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize - 1, egui::Color32::TRANSPARENT, &network_registry);
                            let (_, vis_curr) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize, egui::Color32::TRANSPARENT, &network_registry);
                            if let Some(w) = active_runner.w_hh_fwd.get(l-1) {
                                draw_overlay(&painter, &mut edge_shapes_vec, &hidden_positions[l-1], &hidden_positions[l], w, egui::Color32::from_rgb(255, 190, 80), "overlay", &format!("H{}:", l), &format!("H{}:", l+1), vis_prev, vis_curr, Box::new(move |r, i| active_runner.is_longterm_fwd(l-1, r, i)));
                            }
                        }
                        for l in 0..hidden_positions.len().saturating_sub(1) {
                            let (_, vis_recv) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize, egui::Color32::TRANSPARENT, &network_registry);
                            let (_, vis_send) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize + 1, egui::Color32::TRANSPARENT, &network_registry);
                            if let Some(w) = active_runner.w_hh_bwd.get(l) {
                                draw_overlay(&painter, &mut edge_shapes_vec, &hidden_positions[l+1], &hidden_positions[l], w, egui::Color32::from_rgb(255, 120, 160), "overlay", &format!("H{}:", l+2), &format!("H{}:", l+1), vis_send, vis_recv, Box::new(move |r, i| active_runner.is_longterm_bwd(l, r, i)));
                            }
                        }
                        for l in 0..hidden_positions.len() {
                            let (_, vis) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l as isize, egui::Color32::TRANSPARENT, &network_registry);
                            if let Some(w) = active_runner.w_hh_rec.get(l) {
                                draw_overlay(&painter, &mut edge_shapes_vec, &hidden_positions[l], &hidden_positions[l], w, egui::Color32::from_rgb(200, 200, 255), "overlay", &format!("H{}:", l+1), &format!("H{}:", l+1), vis, vis, Box::new(move |r, i| active_runner.is_longterm_rec(l, r, i)));
                            }
                        }
                        if let Some(last_h) = hidden_positions.last() {
                            let l_last = hidden_positions.len() as isize - 1;
                            let (_, vis_last) = Self::get_layer_visuals(&view_source, &brain_id, &view_node_filter, l_last, egui::Color32::TRANSPARENT, &network_registry);
                            draw_overlay(&painter, &mut edge_shapes_vec, last_h, output_positions, &active_runner.w_out, egui::Color32::from_rgb(160, 240, 160), "overlay", &format!("H{}:", hidden_positions.len()), "O", vis_last, vis_o, Box::new(move |k, j| active_runner.is_longterm_out(k, j)));
                        }
                    }
                }
                }
            }

            // Feedback map overlays: O -> S
            if show_feedback_overlays && !output_positions.is_empty() && !sensory_positions.is_empty() {
                if let Some(active_runner) = active_runner_opt {
                let active_boost = if loop_feedback { 1.0 } else { 0.6 };
                for (k, &p_out) in output_positions.iter().enumerate() {
                    let idx = *active_runner.feedback_map.get(k).unwrap_or(&-1);
                    if idx >= 0 {
                        let i = idx as usize;
                        if i < sensory_positions.len() {
                            let p_in = sensory_positions[i];
                            let col = egui::Color32::from_rgb(200, 140, 255).gamma_multiply(0.25 * active_boost);
                            painter.line_segment([p_out, p_in], egui::Stroke { width: 1.2, color: col });
                            edge_shapes_vec.push(EdgeVisual{
                                p0: p_out, p1: p_in,
                                from_label: format!("O{}", k),
                                to_label: format!("S{}", i),
                                weight: None,
                                kind: "feedback",
                                is_longterm: true, // Feedback connections are static/permanent in this UI context
                            });
                        }
                    }
                }
                }
            }

            // Edge tooltips and context: find nearest edge
            if response.hovered() {
                if let Some(mouse_pos) = ui.ctx().pointer_hover_pos() {
                    if panel_rect.contains(mouse_pos) {
                    let mut best: Option<(usize, f32)> = None;
                    let thresh = 6.0f32; // pixels
                    for (idx, e) in edge_shapes_vec.iter().enumerate() {
                        let d = dist_point_to_segment(mouse_pos, e.p0, e.p1);
                        if d <= thresh {
                            match &mut best {
                                Some((_, bd)) => { if d < *bd { *bd = d; best = Some((idx, d)); } },
                                None => best = Some((idx, d)),
                            }
                        }
                    }
                    if let Some((idx, _)) = best {
                        // Prioritize neurons: only overwrite if we haven't picked a neuron yet
                        if hovered_target.as_ref().map(|t| !t.is_neuron()).unwrap_or(true) {
                            let e = &edge_shapes_vec[idx];
                            // Parse labels to indices for context menu
                            let parse_s = |s: &str| -> Option<usize> { s.strip_prefix('S')?.parse().ok() };
                            let parse_h = |s: &str| -> Option<(usize,usize)> {
                                // format H{layer}:{j}
                                if !s.starts_with('H') { return None; }
                                let rest = &s[1..];
                                let mut it = rest.split(':');
                                let l = it.next()?.parse::<usize>().ok()?;
                                let j = it.next()?.parse::<usize>().ok()?;
                                Some((l.saturating_sub(1), j))
                            };
                            let parse_o = |s: &str| -> Option<usize> { s.strip_prefix('O')?.parse().ok() };
                            match e.kind {
                                "in" | "morph_in" => {
                                    if let (Some(si), Some((_, hj))) = (parse_s(&e.from_label), parse_h(&e.to_label)) {
                                        hovered_target = Some(ContextPick::EdgeIn(si, hj));
                                    }
                                }
                                "fwd" | "morph_fwd" => {
                                    if let (Some((l1, hi)), Some((l2, hj))) = (parse_h(&e.from_label), parse_h(&e.to_label)) {
                                        if l2 == l1 + 1 { hovered_target = Some(ContextPick::EdgeFwd(l1, hi, hj)); }
                                    }
                                }
                                "bwd" | "morph_bwd" => {
                                    if let (Some((l2, hj)), Some((l1, hi))) = (parse_h(&e.from_label), parse_h(&e.to_label)) {
                                        if l2 == l1 + 1 { hovered_target = Some(ContextPick::EdgeBwd(l1, hi, hj)); }
                                    }
                                }
                                "out" | "morph_out" => {
                                    if let (Some((l, hj)), Some(ok)) = (parse_h(&e.from_label), parse_o(&e.to_label)) {
                                        let _ = l; // unused
                                        hovered_target = Some(ContextPick::EdgeOut(hj, ok));
                                    }
                                }
                                "rec" | "morph_rec" => {
                                    if let (Some((l1, hi)), Some((l2, hj))) = (parse_h(&e.from_label), parse_h(&e.to_label)) {
                                        if l1 == l2 { hovered_target = Some(ContextPick::EdgeRec(l1, hi, hj)); }
                                    }
                                }
                                "overlay" => {
                                    // Try all possible matches for overlays based on labels
                                    if let (Some(si), Some((_, hj))) = (parse_s(&e.from_label), parse_h(&e.to_label)) {
                                        hovered_target = Some(ContextPick::EdgeIn(si, hj));
                                    } else if let (Some((l1, hi)), Some((l2, hj))) = (parse_h(&e.from_label), parse_h(&e.to_label)) {
                                        if l1 == l2 {
                                            hovered_target = Some(ContextPick::EdgeRec(l1, hi, hj));
                                        } else if l2 == l1 + 1 {
                                            hovered_target = Some(ContextPick::EdgeFwd(l1, hi, hj));
                                        } else if l1 == l2 + 1 {
                                            hovered_target = Some(ContextPick::EdgeBwd(l2, hj, hi));
                                        }
                                    } else if let (Some((l, hj)), Some(ok)) = (parse_h(&e.from_label), parse_o(&e.to_label)) {
                                        let _ = l;
                                        hovered_target = Some(ContextPick::EdgeOut(hj, ok));
                                    }
                                }
                                _ => {}
                            }
                        }
                        let e = &edge_shapes_vec[idx];
                        let text = if let Some(w) = e.weight {
                            format!("{} → {}  kind={}  w={:.3}{}", e.from_label, e.to_label, e.kind, w, if e.is_longterm { " [Longterm]" } else { " [New]" })
                        } else {
                            format!("{} → {}  kind={}{}", e.from_label, e.to_label, e.kind, if e.is_longterm { " [Longterm]" } else { " [New]" })
                        };
                        tooltip_lines.push(text);
                    }
                }
            }
        }

            // Finally, if we collected any tooltip lines, show a single consolidated floating panel near the cursor.
            // Support pinning via right-click so the user can interact with the buttons.
            if response.secondary_clicked() {
                if !tooltip_lines.is_empty() {
                    tooltip_pinned = true;
                    tooltip_pinned_pos = ui.ctx().pointer_interact_pos().unwrap_or(egui::Pos2::ZERO);
                    tooltip_pinned_lines = tooltip_lines.clone();
                    tooltip_pinned_target = hovered_target;
                } else {
                    tooltip_pinned = false;
                }
            }

            let mut show_tooltip = false;
            let mut display_pos = egui::Pos2::ZERO;
            let mut display_lines = Vec::new();
            let mut display_target = None;

            if tooltip_pinned {
                show_tooltip = true;
                display_pos = tooltip_pinned_pos;
                display_lines = tooltip_pinned_lines.clone();
                display_target = tooltip_pinned_target;
            } else if !tooltip_lines.is_empty() {
                if tooltip_suppression_counter > 0 {
                    tooltip_suppression_counter = tooltip_suppression_counter.saturating_sub(1);
                } else if let Some(mouse_pos) = ui.ctx().pointer_hover_pos() {
                    show_tooltip = true;
                    display_pos = mouse_pos + egui::vec2(12.0, 16.0);
                    display_lines = tooltip_lines.clone();
                    display_target = hovered_target;
                }
            }

            if show_tooltip {
                egui::Area::new(egui::Id::new("consolidated-tooltip-area"))
                    .order(egui::Order::Tooltip)
                    .fixed_pos(display_pos)
                    .interactable(true)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            let mut text = display_lines.join("\n\n");
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .desired_width(300.0)
                                    .lock_focus(true)
                            );
                            if let Some(ref tgt) = display_target {
                                ui.add_space(6.0);
                                ui.separator();
                                ui.label("Add probe:");
                                let mut add = |name: String, color: egui::Color32, target: ProbeTarget, kind: ProbeKind| {
                                    let id = next_probe_id; next_probe_id += 1;
                                    probes_local.push(Probe::new(id, name, color, target, kind, 10_000));
                                };
                                match *tgt {
                                    ContextPick::Sensory(i) => {
                                        if ui.button("🔬 Detailed biological view").clicked() {
                                            selected_neuron_pick = Some(ContextPick::Sensory(i));
                                            show_neuron_detail = true;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Spike (S)").clicked() {
                                            add(format!("S{} spike", i), egui::Color32::from_rgb(120,170,255), ProbeTarget::Sensory(i), ProbeKind::Spike);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::Hidden(l, j) => {
                                        if ui.button("🔬 Detailed biological view").clicked() {
                                            selected_neuron_pick = Some(ContextPick::Hidden(l, j));
                                            show_neuron_detail = true;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Spike (H)").clicked() {
                                            add(format!("H{}:{} spike", l+1, j), egui::Color32::from_rgb(255,150,70), ProbeTarget::Hidden(l,j), ProbeKind::Spike);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Vm (H)").clicked() {
                                            add(format!("H{}:{} Vm", l+1, j), egui::Color32::from_rgb(255,190,80), ProbeTarget::Hidden(l,j), ProbeKind::Membrane);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Current (H)").clicked() {
                                            add(format!("H{}:{} I", l+1, j), egui::Color32::from_rgb(255,210,120), ProbeTarget::Hidden(l,j), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::Output(k) => {
                                        if ui.button("🔬 Detailed biological view").clicked() {
                                            selected_neuron_pick = Some(ContextPick::Output(k));
                                            show_neuron_detail = true;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Spike (O)").clicked() {
                                            add(format!("O{} spike", k), egui::Color32::from_rgb(160,240,120), ProbeTarget::Output(k), ProbeKind::Spike);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Vm (O)").clicked() {
                                            add(format!("O{} Vm", k), egui::Color32::from_rgb(160,240,120), ProbeTarget::Output(k), ProbeKind::Membrane);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                        if ui.button("Current (O)").clicked() {
                                            add(format!("O{} I", k), egui::Color32::from_rgb(160,240,160), ProbeTarget::Output(k), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::EdgeIn(i, j) => {
                                        if ui.button("Current (S→H0)").clicked() {
                                            add(format!("S{}→H1:{} I", i, j), egui::Color32::from_rgb(120,170,255), ProbeTarget::ConnIn(i,j), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::EdgeFwd(l, i, j) => {
                                        if ui.button("Current (H→H)").clicked() {
                                            add(format!("H{}:{}→H{}:{} I", l+1, i, l+2, j), egui::Color32::from_rgb(255,190,80), ProbeTarget::ConnFwd(l,i,j), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::EdgeBwd(l, i, j) => {
                                        if ui.button("Current (H←H)").clicked() {
                                            add(format!("H{}:{}←H{}:{} I", l+1, i, l+2, j), egui::Color32::from_rgb(255,120,160), ProbeTarget::ConnBwd(l,i,j), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::EdgeOut(j, k) => {
                                        if ui.button("Current (H→O)").clicked() {
                                            let l_last = hidden_positions.len();
                                            add(format!("H{}:{}→O{} I", l_last, j, k), egui::Color32::from_rgb(160, 240, 160), ProbeTarget::ConnOut(j, k), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                    ContextPick::EdgeRec(l, i, j) => {
                                        if ui.button("Current (Recurrent)").clicked() {
                                            add(format!("H{}:{}↺{} I", l+1, i, j), egui::Color32::from_rgb(200, 200, 255), ProbeTarget::ConnRec(l,i,j), ProbeKind::Current);
                                            tooltip_suppression_counter = 12;
                                            tooltip_pinned = false;
                                        }
                                    }
                                }
                            }
                        });
                    });
            }

            // Fixed-corner hint so it doesn't block right-clicking near the cursor
            if hovered_target.is_none() {
                let hint_pos = egui::pos2(panel_rect.left() + 8.0, panel_rect.top() + 24.0);
                painter.text(
                    hint_pos,
                    egui::Align2::LEFT_TOP,
                    "Right-click a node or edge to add a probe",
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_gray(160),
                );
            }

            // Draw Graphic EQ in lower-left corner of panel_rect (existing feature)
            if self.show_equalizer {
                let eq_width = 260.0;
                let eq_height = 120.0;
                let margin = 10.0;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.left() + margin, panel_rect.bottom() - eq_height - margin),
                    egui::vec2(eq_width, eq_height),
                );
                painter.rect_filled(rect, 6.0, egui::Color32::from_gray(20));
                painter.rect_stroke(rect, 6.0, egui::Stroke { width: 1.0, color: egui::Color32::from_gray(80) }, egui::StrokeKind::Outside);
                let title = String::from("Graphic EQ");
                painter.text(rect.left_top() + egui::vec2(8.0, 4.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(12.0), egui::Color32::WHITE);

                // Fetch bands and smooth
                if let Some(b) = bands_guard.as_deref() {
                    if smoothed_equalizer_values.len() != b.len() { smoothed_equalizer_values = vec![0.0f32; b.len()]; }
                    for i in 0..b.len() { smoothed_equalizer_values[i] = 0.7*smoothed_equalizer_values[i] + 0.3*b[i].clamp(0.0, 1.0); }
                } else {
                    for v in &mut smoothed_equalizer_values { *v *= 0.9; }
                }
                // Draw bars
                let n = smoothed_equalizer_values.len().max(8);
                if smoothed_equalizer_values.len() != n { smoothed_equalizer_values.resize(n, 0.0); }
                let bar_w = (eq_width - 16.0) / n as f32;
                let max_bar_h = eq_height - 28.0;
                for i in 0..n {
                    let val = smoothed_equalizer_values[i].clamp(0.0, 1.0) as f32;
                    let h = val * max_bar_h;
                    let x0 = rect.left() + 8.0 + i as f32 * bar_w;
                    let y0 = rect.bottom() - 8.0 - h;
                    let r = egui::Rect::from_min_size(egui::pos2(x0, y0), egui::vec2(bar_w*0.9, h));
                    let col = egui::Color32::from_rgb(100, 180, 240);
                    painter.rect_filled(r, 2.0, col);
                }
            }

            // Optional follow-up: small spike raster inset (outputs) at bottom-right
            let raster_w = 300.0f32;
            let raster_h = 130.0f32;
            let rrect = egui::Rect::from_min_size(
                egui::pos2(panel_rect.right() - raster_w - margin, panel_rect.bottom() - raster_h - margin),
                egui::vec2(raster_w, raster_h)
            );
            painter.rect_filled(rrect, 6.0, egui::Color32::from_gray(20));
            painter.rect_stroke(rrect, 6.0, egui::Stroke { width: 1.0, color: egui::Color32::from_gray(80) }, egui::StrokeKind::Outside);
            painter.text(rrect.left_top() + egui::vec2(8.0, 4.0), egui::Align2::LEFT_TOP, "Output raster", egui::FontId::proportional(12.0), egui::Color32::WHITE);
            // draw cells
            let cols = raster_cols.max(1) as f32;
            let rows = active_net.num_output_neurons.max(1) as f32;
            let left = rrect.left() + 8.0;
            let top = rrect.top() + 22.0;
            let w = rrect.width() - 16.0;
            let h = rrect.height() - 30.0;
            let cw = w / cols;
            let ch = h / rows;
            let first_col = raster_outputs.len().saturating_sub(raster_cols);
            for (ci, col) in raster_outputs.iter().skip(first_col).enumerate() {
                let x = left + ci as f32 * cw;
                for (k, &v) in col.iter().enumerate() {
                    if v != 0 {
                        let y = top + (rows - 1.0 - k as f32) * ch;
                        let rr = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw*0.9, ch*0.9));
                        painter.rect_filled(rr, 1.0, egui::Color32::from_rgb(180, 240, 120));
                    }
                }
            }

            // Oscilloscope window centered at bottom (multi-channel)
            // Make the oscilloscope react to mouse wheel zoom when hovered
            // Allocate an interactive region matching scope_rect to capture hover/scroll
            let scope_resp = ui.allocate_rect(scope_rect, egui::Sense::hover());
            // Handle zoom via mouse wheel when hovered
            if scope_resp.hovered() {
                let scroll_y = ui.input(|i| {
                    let d = i.smooth_scroll_delta.y;
                    d
                });
                if scroll_y.abs() > 0.0 {
                    // Map pixel delta to multiplicative scaling. Positive scroll → zoom in (shorter time window).
                    // Sensitivity tuned so a ~120 px wheel notch ≈ 15% zoom.
                    let sensitivity: f32 = 0.0012;
                    let factor = (1.0f32 - scroll_y * sensitivity).clamp(0.5f32, 1.5f32);
                    let new_time = (scope_time_ms * factor).clamp(250.0, 10000.0);
                    if (new_time - scope_time_ms).abs() > f32::EPSILON {
                        scope_time_ms = new_time;
                        ctx.request_repaint();
                    }
                }
            }

            // Draw scope background and frame
            painter.rect_filled(scope_rect, 6.0, egui::Color32::from_gray(20));
            painter.rect_stroke(scope_rect, 6.0, egui::Stroke { width: 1.0, color: egui::Color32::from_gray(80) }, egui::StrokeKind::Outside);
            painter.text(scope_rect.left_top() + egui::vec2(8.0, 4.0), egui::Align2::LEFT_TOP, "Oscilloscope", egui::FontId::proportional(12.0), egui::Color32::WHITE);
            // draw grid
            if scope_grid {
                let gx = 8; let gy = 4;
                for i in 1..gx {
                    let x = egui::lerp(scope_rect.left()..=scope_rect.right(), i as f32 / gx as f32);
                    painter.line_segment([egui::pos2(x, scope_rect.top()+18.0), egui::pos2(x, scope_rect.bottom()-6.0)], egui::Stroke{ width: 1.0, color: egui::Color32::from_gray(40)});
                }
                for j in 1..gy {
                    let y = egui::lerp((scope_rect.top()+18.0)..=(scope_rect.bottom()-6.0), j as f32 / gy as f32);
                    painter.line_segment([egui::pos2(scope_rect.left()+6.0, y), egui::pos2(scope_rect.right()-6.0, y)], egui::Stroke{ width: 1.0, color: egui::Color32::from_gray(40)});
                }
            }
            // plot traces
            let inner = egui::Rect::from_min_max(scope_rect.left_top() + egui::vec2(6.0, 18.0), scope_rect.right_bottom() - egui::vec2(6.0, 6.0));
            let enabled: Vec<&Probe> = probes_local.iter().filter(|p| p.enabled).collect();
            let lanes = if scope_lanes { enabled.len().max(1) } else { 1 };
            let lane_h = inner.height() / lanes as f32;
            let dt = lif_cloned.dt.max(0.001) as f32;
            let visible_samples = ((scope_time_ms / dt).ceil() as usize).clamp(10, 20000);
            for (idx, pr) in enabled.iter().enumerate() {
                let lane_top = inner.top() + idx as f32 * lane_h;
                let lane_rect = if scope_lanes { egui::Rect::from_min_max(egui::pos2(inner.left(), lane_top), egui::pos2(inner.right(), lane_top + lane_h)) } else { inner };
                // Stable per-pixel binning to avoid aliasing flicker: aggregate samples in each pixel column
                let px: usize = lane_rect.width().max(1.0) as usize;
                // Always span the full width: map the full visible time window across all pixel columns
                let mut last: Option<egui::Pos2> = None;
                let yrange: (f32, f32) = if matches!(pr.kind, ProbeKind::Spike) { (0.0, 1.0) } else { (-1.5, 1.5) };
                let cap = pr.capacity.max(1);
                let end = pr.write_idx % cap; // points to the index where next write will occur (newest is at end-1)
                for col in 0..px {
                    // Draw newest data at the RIGHT edge so time flows right -> left
                    let x = egui::lerp(lane_rect.left()..=lane_rect.right(), 1.0 - ((col as f32 + 0.5) / px as f32));
                    // Range [start_back, end_back) in samples back from newest that maps this pixel column.
                    // This distributes the entire visible window across all columns, ensuring full-width fill.
                    let start_back = (col * visible_samples) / px;
                    let mut end_back = ((col + 1) * visible_samples) / px;
                    if end_back <= start_back { end_back = (start_back + 1).min(visible_samples); }
                    let mut have = false;
                    let mut agg_val: f32 = if matches!(pr.kind, ProbeKind::Spike) { 0.0 } else { 0.0 };
                    let mut count: usize = 0;
                    for b in start_back..end_back {
                        let ridx = (cap + end + cap - 1 - (b % cap)) % cap; // newest at b=0
                        let v = pr.data.get(ridx).copied().unwrap_or(f32::NAN);
                        if v.is_nan() { continue; }
                        have = true;
                        match pr.kind {
                            ProbeKind::Spike => {
                                if v > 0.5 { agg_val = 1.0; break; } // max over bin
                            }
                            _ => {
                                agg_val += v;
                                count += 1;
                            }
                        }
                    }
                    if !have { last = None; continue; }
                    let v = match pr.kind {
                        ProbeKind::Spike => agg_val, // 0 or 1, ignore gain for spikes
                        _ => if count > 0 { (agg_val / count as f32) * scope_gain } else { f32::NAN },
                    };
                    if v.is_nan() { last = None; continue; }
                    let y_norm = ((v - yrange.0) / (yrange.1 - yrange.0)).clamp(0.0, 1.0);
                    let y = lane_rect.bottom() - y_norm * lane_rect.height();
                    let p = egui::pos2(x, y);
                    if let Some(p0) = last { painter.line_segment([p0, p], egui::Stroke{ width: 1.5, color: pr.color }); }
                    last = Some(p);
                }
            }
            }
            // --- 5. Sync back modified local state to self ---
            self.tooltip_pinned = tooltip_pinned;
            self.tooltip_pinned_pos = tooltip_pinned_pos;
            self.tooltip_pinned_lines = tooltip_pinned_lines;
            self.tooltip_pinned_target = tooltip_pinned_target;
            self.tooltip_suppression_counter = tooltip_suppression_counter;
            self.next_probe_id = next_probe_id;
            self.probes = probes_local;
            self.selected_neuron_pick = selected_neuron_pick;
            self.show_neuron_detail = show_neuron_detail;
            self.smoothed_equalizer_values = smoothed_equalizer_values;
            self.edge_shapes = edge_shapes_vec;
            self.scope_time_ms = scope_time_ms;
        });
        }

        // Pop-out detailed biological view window
        if self.show_neuron_detail {
            let detail_runner_arc = self.runner.clone();
            let mut detail_managed_arc = None;
            let mut detail_managed_guard = None;
            let mut detail_standalone_guard = None;
            let active_runner_opt: Option<&Runner> = match &self.view_source {
                ViewSource::Standalone | ViewSource::ClusterGlobal(_) => {
                    if let Ok(guard) = detail_runner_arc.try_read() {
                        detail_standalone_guard = Some(guard);
                        Some(&*detail_standalone_guard.as_ref().unwrap())
                    } else {
                        // Runner is busy with a simulation step; skip detail panel this frame
                        // rather than blocking the UI event loop.
                        None
                    }
                }
                ViewSource::LocalManaged(id) => {
                    if let Some(state_arc) = state_arc.as_ref() {
                        if let Ok(s) = state_arc.try_read() {
                            if let Some(net_arc) = s.networks.get(id) {
                                let arc = net_arc.clone();
                                detail_managed_arc = Some(arc);
                                if let Ok(guard) = detail_managed_arc.as_ref().unwrap().try_read() {
                                    detail_managed_guard = Some(guard);
                                    Some(&detail_managed_guard.as_ref().unwrap().runner)
                                } else {
                                    let guard =
                                        detail_managed_arc.as_ref().unwrap().blocking_read();
                                    detail_managed_guard = Some(guard);
                                    Some(&detail_managed_guard.as_ref().unwrap().runner)
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };
            let mut open = self.show_neuron_detail;
            let selected_neuron_pick = self.selected_neuron_pick;
            let mut detail_camera_zoom = self.detail_camera_zoom;
            let mut detail_camera_yaw = self.detail_camera_yaw;
            let mut detail_camera_pitch = self.detail_camera_pitch;
            let mut detail_cam_pan = self.detail_cam_pan;
            let mut detail_camera_pos = self.detail_camera_pos;
            let mut detail_bio_orient = self.detail_bio_orient;
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_enabled = self.detail_bouton_pid_enabled;
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_kp = self.detail_bouton_pid_kp;
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_ki = self.detail_bouton_pid_ki;
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_kd = self.detail_bouton_pid_kd;
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_axon = self.detail_bouton_pid_axon.clone();
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_dend = self.detail_bouton_pid_dend.clone();
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            let mut detail_bouton_pid_pick = self.detail_bouton_pid_pick;
            let mut detail_timescale = self.detail_timescale;
            let mut detail_time_offset = self.detail_time_offset;
            let mut detail_paused = self.detail_paused;
            let mut detail_last_neuron = self.detail_last_neuron;
            let mut detail_waiting_for_activation = self.detail_waiting_for_activation;
            let playing = self.playing;
            let hidden_layers_len = self.hidden_positions.len();
            let raster_outputs = self.raster_outputs.clone();

            egui::Window::new("🔬 Biological Detail View")
                .open(&mut open)
                .default_size(egui::vec2(600.0, 500.0))
                .show(ui, |ui| {
                    if active_runner_opt.is_none() {
                        ui.label("Simulation busy...");
                        return;
                    }
                    let active_runner = active_runner_opt.unwrap();
                    if let Some(pick) = selected_neuron_pick {
                        // Detect first selection or handle newly selected neuron
                        let is_new = detail_last_neuron.is_none() ||
                            format!("{:?}", detail_last_neuron.unwrap()) != format!("{:?}", pick);

                        if is_new {
                            detail_last_neuron = Some(pick);
                            let acts = self.find_activations(active_runner, pick);
                            if let Some(&recent) = acts.iter().min() {
                                detail_time_offset = recent as f32;
                                detail_waiting_for_activation = false;
                            } else {
                                detail_waiting_for_activation = true;
                            }
                            #[cfg(all(feature = "morpho", feature = "growth3d"))]
                            {
                                detail_bouton_pid_pick = Some(pick);
                                detail_bouton_pid_axon.clear();
                                detail_bouton_pid_dend.clear();
                            }
                        }

                        if detail_waiting_for_activation && playing {
                            let acts = self.find_activations(active_runner, pick);
                            if let Some(&recent) = acts.iter().min() {
                                detail_time_offset = recent as f32;
                                detail_waiting_for_activation = false;
                            }
                        }

                        ui.horizontal(|ui| {
                            ui.label(format!("Viewing: {:?}", pick));
                            if ui.button("Center View").clicked() {
                                detail_camera_zoom = 1.0;
                                detail_camera_yaw = 0.0;
                                detail_camera_pitch = 0.0;
                                detail_cam_pan = egui::Vec2::ZERO;
                                detail_camera_pos = [0.0, 0.0, 0.0];
                            }
                            ui.checkbox(&mut detail_paused, "Pause Playback");
                            ui.label("(Drag: Rotate, Ctrl+Drag: Pan, WASD/QE: Fly)");
                        });

                        ui.separator();

                        // Independent timescale/playback
                        ui.horizontal(|ui| {
                            ui.label("Timescale:");
                            ui.add(egui::Slider::new(&mut detail_timescale, 0.1..=5.0).text("Speed"));
                            ui.label("Time Sweep:");
                            let max_hist = active_runner.hist_len as f32;
                            if ui.add(egui::Slider::new(&mut detail_time_offset, 0.0..=max_hist).text("Steps back")).changed() {
                                detail_waiting_for_activation = false;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Dendrite Activations:");
                            if ui.button("⏮ Step Prev").on_hover_text("Jump to previous activation (further back in time)").clicked() {
                                let acts = self.find_activations(active_runner, pick);
                                let current = detail_time_offset as usize;
                                // Prev = earlier in time = larger offset.
                                if let Some(&prev) = acts.iter().filter(|&&a| a > current).min() {
                                    detail_time_offset = prev as f32;
                                    detail_waiting_for_activation = false;
                                }
                            }
                            if ui.button("Step Next ⏭").on_hover_text("Jump to next activation (closer to present)").clicked() {
                                let acts = self.find_activations(active_runner, pick);
                                let current = detail_time_offset as usize;
                                // Next = later in time = smaller offset.
                                if let Some(&next) = acts.iter().filter(|&&a| a < current).max() {
                                    detail_time_offset = next as f32;
                                    detail_waiting_for_activation = false;
                                }
                            }
                            if detail_waiting_for_activation {
                                ui.label(egui::RichText::new("⏳ Waiting for first activation...").color(egui::Color32::LIGHT_YELLOW));
                            }
                        });

                        if !detail_paused && playing {
                            detail_time_offset = (detail_time_offset + detail_timescale).min(active_runner.hist_len as f32);
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Orientation:");
                            egui::ComboBox::from_id_salt("detail_bio_orient")
                                .selected_text(detail_bio_orient.label())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut detail_bio_orient, DetailBioOrient::AsIs, DetailBioOrient::AsIs.label());
                                    ui.selectable_value(&mut detail_bio_orient, DetailBioOrient::MirrorAxes, DetailBioOrient::MirrorAxes.label());
                                    ui.selectable_value(&mut detail_bio_orient, DetailBioOrient::AlignAxonRight, DetailBioOrient::AlignAxonRight.label());
                                });
                        });
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut detail_bouton_pid_enabled, "Bouton PID smoothing");
                                ui.add_enabled_ui(detail_bouton_pid_enabled, |ui| {
                                    ui.add(egui::Slider::new(&mut detail_bouton_pid_kp, 0.0..=1.0).text("Kp"));
                                    ui.add(egui::Slider::new(&mut detail_bouton_pid_ki, 0.0..=0.2).text("Ki"));
                                    ui.add(egui::Slider::new(&mut detail_bouton_pid_kd, 0.0..=0.5).text("Kd"));
                                });
                            });
                        }

                        // 3D Visualizer Area
                        let (rect, response) = ui.allocate_at_least(ui.available_size() - egui::vec2(0.0, 20.0), egui::Sense::click_and_drag());

                        // Handle 3D rotation/pan/zoom/fly-through for detail view
                        ui.input(|i| {
                            if response.hovered() {
                                let zoom = i.zoom_delta();
                                if (zoom - 1.0).abs() > 0.001 {
                                    detail_camera_zoom = (detail_camera_zoom * zoom).clamp(0.1, 10.0);
                                }
                                if i.smooth_scroll_delta.y != 0.0 {
                                    let f = 1.0 + (i.smooth_scroll_delta.y / 480.0);
                                    detail_camera_zoom = (detail_camera_zoom * f).clamp(0.1, 10.0);
                                }

                                // Fly-through keyboard controls (WASD/QE)
                                let speed = 0.02f32;
                                if i.key_down(egui::Key::W) { detail_camera_pos[2] += speed; }
                                if i.key_down(egui::Key::S) { detail_camera_pos[2] -= speed; }
                                if i.key_down(egui::Key::A) { detail_camera_pos[0] += speed; }
                                if i.key_down(egui::Key::D) { detail_camera_pos[0] -= speed; }
                                if i.key_down(egui::Key::Q) { detail_camera_pos[1] += speed; }
                                if i.key_down(egui::Key::E) { detail_camera_pos[1] -= speed; }
                            }
                            if response.dragged() {
                                let delta = i.pointer.delta();
                                if i.modifiers.ctrl || i.modifiers.command {
                                    // Pan: ctrl+drag
                                    detail_cam_pan += delta;
                                } else {
                                    // Rotate: left-drag (default)
                                    detail_camera_yaw += delta.x * 0.01;
                                    detail_camera_pitch = (detail_camera_pitch - delta.y * 0.01).clamp(-1.5, 1.5);
                                }
                            }
                        });

                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(10));

                        // Render the neuron in 3D
                        // Find the neuron's morphology
                        #[cfg(all(feature = "morpho", feature = "growth3d"))]
                        {
                            let target_idx = match pick {
                                ContextPick::Hidden(_, j) => Some(j),
                                ContextPick::Sensory(i) => Some(i),
                                ContextPick::Output(k) => Some(k),
                                _ => None,
                            };
                            let _target_layer_idx = match pick {
                                ContextPick::Hidden(l, _) => Some(l),
                                ContextPick::Sensory(_) => None,
                                ContextPick::Output(_) => Some(hidden_layers_len),
                                _ => None,
                            };

                            if let Some(_j) = target_idx {
                                // Find biology components for this neuron
                                let opt_morph = match pick {
                                    ContextPick::Hidden(l, jj) => {
                                        if l < active_runner.morph.somas.len() && jj < active_runner.morph.somas[l].len() {
                                            Some((&active_runner.morph.somas[l][jj], &active_runner.morph.axons[l][jj], &active_runner.morph.dendrites[l][jj]))
                                        } else { None }
                                    }
                                    ContextPick::Sensory(ii) => {
                                        if ii < active_runner.morph.sensory_somas.len() {
                                            Some((&active_runner.morph.sensory_somas[ii], &active_runner.morph.sensory_axons[ii], &active_runner.morph.sensory_dendrites[ii]))
                                        } else { None }
                                    }
                                    ContextPick::Output(kk) => {
                                        if kk < active_runner.morph.output_somas.len() {
                                            Some((&active_runner.morph.output_somas[kk], &active_runner.morph.output_axons[kk], &active_runner.morph.output_dendrites[kk]))
                                        } else { None }
                                    }
                                    _ => None,
                                };

                                if let Some((soma, axon, dendrite)) = opt_morph {
                                    let mut detail_tooltip: Option<String> = None;
                                    let mouse_pos = if response.hovered() { ui.ctx().pointer_hover_pos() } else { None };

                                    let dt_s = ui.input(|i| i.unstable_dt).max(0.001);
                                    let center = rect.center() + detail_cam_pan;
                                    let scale = 200.0 * detail_camera_zoom;

                                    #[derive(Clone, Copy)]
                                    enum DetailPart { Soma, Organelle, Dendrite, Axon }

                                    let mut align_yaw: f32 = 0.0;
                                    if detail_bio_orient == DetailBioOrient::AlignAxonRight {
                                        let mut sum = (0.0f32, 0.0f32, 0.0f32);
                                        for seg in &axon.segments {
                                            let vx = seg.to.x - soma.pos.x;
                                            let vy = seg.to.y - soma.pos.y;
                                            let vz = seg.to.z - soma.pos.z;
                                            sum.0 += vx;
                                            sum.1 += vy;
                                            sum.2 += vz;
                                        }
                                        let mag = (sum.0 * sum.0 + sum.1 * sum.1 + sum.2 * sum.2).sqrt();
                                        if mag > 1e-6 {
                                            align_yaw = sum.2.atan2(sum.0);
                                        }
                                    }

                                    let project = |p: crate::morphology::Point3, part: DetailPart| {
                                        let x = p.x; let y = p.y; let z = p.z;
                                        // Rotate around soma center + camera translation
                                        let cx = soma.pos.x + detail_camera_pos[0];
                                        let cy = soma.pos.y + detail_camera_pos[1];
                                        let cz = soma.pos.z + detail_camera_pos[2];
                                        let dx = x - cx; let dy = y - cy; let dz = z - cz;

                                        let mut mx = dx;
                                        let my = dy;
                                        let mut mz = dz;

                                        if detail_bio_orient == DetailBioOrient::AlignAxonRight {
                                            let (sy_a, cy_a) = (-align_yaw).sin_cos();
                                            let rx = mx * cy_a - mz * sy_a;
                                            let rz = mx * sy_a + mz * cy_a;
                                            mx = rx;
                                            mz = rz;
                                        } else if detail_bio_orient == DetailBioOrient::MirrorAxes {
                                            match part {
                                                DetailPart::Dendrite => { mx = -mx.abs(); }
                                                DetailPart::Axon => { mx = mx.abs(); }
                                                _ => {}
                                            }
                                        }

                                        let (sy, cy_rot) = detail_camera_yaw.sin_cos();
                                        let (sp, cp) = detail_camera_pitch.sin_cos();

                                        // 3D Rotation
                                        let x1 = mx * cy_rot - mz * sy;
                                        let z1 = mx * sy + mz * cy_rot;

                                        let y2 = my * cp - z1 * sp;
                                        let z2 = my * sp + z1 * cp;

                                        // Perspective-like depth scaling
                                        let depth_scale = (1.5 + z2).max(0.1);
                                        let final_scale = scale * depth_scale;

                                        egui::pos2(center.x + x1 * final_scale, center.y + y2 * final_scale)
                                    };

                                    // Draw Soma (sphere approximation)
                                    let soma_p = project(soma.pos, DetailPart::Soma);
                                    let soma_r = 0.05 * scale; // default soma size

                                    if let Some(mpos) = mouse_pos {
                                        if mpos.distance(soma_p) <= soma_r {
                                            detail_tooltip = Some(format!("Soma (Layer {}, ID {}) - ATP: {:.1}%", soma.layer, soma.id, soma.atp * 100.0));
                                        }
                                    }

                                    // Activation flash
                                    let steps_back = detail_time_offset.floor() as usize;
                                    let active = match pick {
                                        ContextPick::Hidden(l, jj) => {
                                            if steps_back < active_runner.hist_len {
                                                active_runner.hist_h_at(l, steps_back, jj) != 0
                                            } else { false }
                                        }
                                        ContextPick::Sensory(ii) => {
                                            if steps_back < active_runner.hist_len {
                                                active_runner.hist_s_at(steps_back, ii) != 0
                                            } else { false }
                                        }
                                        ContextPick::Output(kk) => {
                                            // use raster_outputs history if available
                                            let n = raster_outputs.len();
                                            if steps_back < n {
                                                raster_outputs[n - 1 - steps_back][kk] != 0
                                            } else if steps_back == 0 {
                                                active_runner.last_spk_o[kk] != 0
                                            } else { false }
                                        }
                                        _ => false,
                                    };

                                    let soma_col = if active { egui::Color32::WHITE } else { egui::Color32::from_rgb(255, 160, 60) };
                                    let membrane_col = egui::Color32::from_rgb(255, 200, 150).gamma_multiply(0.8);

                                    // Draw Soma Membrane (close-fitting)
                                    painter.circle_filled(soma_p, soma_r, soma_col.gamma_multiply(0.3));

                                    // ATP indicator (outer ring)
                                    let atp = soma.atp;
                                    let atp_col = if atp > 0.5 { egui::Color32::from_rgb(50, 255, 50) } else { egui::Color32::from_rgb(255, 50, 50) };
                                    painter.circle_stroke(soma_p, soma_r + 4.0 * detail_camera_zoom, egui::Stroke::new(2.0 * detail_camera_zoom, atp_col.gamma_multiply(0.4)));

                                    painter.circle_stroke(soma_p, soma_r, egui::Stroke::new(3.0 * detail_camera_zoom, membrane_col));

                                    // Draw Organelles
                                    let time = ui.input(|i| i.time);
                                    for org in &soma.organelles {
                                        let p = project(org.pos, DetailPart::Organelle);
                                        let org_r = soma_r * 0.35;
                                        if let Some(mpos) = mouse_pos {
                                            if mpos.distance(p) <= org_r {
                                                detail_tooltip = Some(format!("{:?} - Activity: {:.1}%", org.kind, org.activity * 100.0));
                                            }
                                        }
                                        match org.kind {
                                            crate::morphology::OrganelleKind::Nucleus => {
                                                painter.circle_filled(p, soma_r * 0.35, egui::Color32::from_rgba_unmultiplied(180, 100, 255, 180));
                                                painter.circle_stroke(p, soma_r * 0.35, egui::Stroke::new(1.0_f32, egui::Color32::from_gray(200)));
                                            }
                                            crate::morphology::OrganelleKind::Mitochondria => {
                                                let pulse = (time * 5.0).sin().abs() as f32 * 0.2 * org.activity;
                                                let r = soma_r * (0.15 + pulse);
                                                painter.circle_filled(p, r, egui::Color32::from_rgba_unmultiplied(255, 150, 0, 200));
                                                painter.circle_stroke(p, r, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(200, 100, 0)));
                                            }
                                            crate::morphology::OrganelleKind::GolgiApparatus => {
                                                // Draw as folded ribbons/curves
                                                let r = soma_r * 0.2;
                                                for i in 0..3 {
                                                    let offset = (i as f32 - 1.0) * 2.0;
                                                    painter.add(egui::Shape::line(
                                                        vec![
                                                            p + egui::vec2(-r, offset),
                                                            p + egui::vec2(0.0, offset + 2.0),
                                                            p + egui::vec2(r, offset),
                                                        ],
                                                        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 100, 200))
                                                    ));
                                                }
                                            }
                                            crate::morphology::OrganelleKind::EndoplasmicReticulum => {
                                                // Draw as a textured/stippled area
                                                painter.circle_filled(p, soma_r * 0.25, egui::Color32::from_rgba_unmultiplied(100, 150, 255, 100));
                                                painter.circle_stroke(p, soma_r * 0.25, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(50, 100, 200)));
                                            }
                                            _ => {
                                                painter.circle_filled(p, soma_r * 0.1, egui::Color32::LIGHT_GRAY);
                                            }
                                        }
                                    }

                                    // Draw Dendrites with membrane and boutons
                                    for (si, seg) in dendrite.tree.branches.iter().enumerate() {
                                        let mut p0_3d = seg.from;
                                        let mut p1_3d = seg.to;
                                        if detail_bouton_pid_enabled {
                                            let idx0 = si.saturating_mul(2);
                                            let idx1 = idx0 + 1;
                                            if detail_bouton_pid_dend.len() <= idx1 {
                                                detail_bouton_pid_dend.resize(idx1 + 1, Pid3State::default());
                                            }
                                            p0_3d = pid_smooth_point(&mut detail_bouton_pid_dend[idx0], p0_3d, dt_s, detail_bouton_pid_kp, detail_bouton_pid_ki, detail_bouton_pid_kd);
                                            p1_3d = pid_smooth_point(&mut detail_bouton_pid_dend[idx1], p1_3d, dt_s, detail_bouton_pid_kp, detail_bouton_pid_ki, detail_bouton_pid_kd);
                                        }
                                        let p0 = project(p0_3d, DetailPart::Dendrite);
                                        let p1 = project(p1_3d, DetailPart::Dendrite);

                                        if let Some(mpos) = mouse_pos {
                                            if dist_point_to_segment(mpos, p0, p1) <= 5.0 {
                                                let mut msg = format!("Dendrite Segment - Length: {:.3}, Stimuli: {:.3}", seg.length, seg.stimuli);
                                                if let Some(sidx) = seg.syn_index {
                                                    if let Some(syn) = active_runner.morph.synapses.get(sidx) {
                                                        msg.push_str(&format!("\nSynapse: w={:.3}, delay={:.1}ms", syn.weight, syn.delay_ms));
                                                    }
                                                }
                                                detail_tooltip = Some(msg);
                                            }
                                        }

                                        let thickness = 0.01 * scale;
                                        let den_atp = dendrite.atp;
                                        // Membrane border
                                        painter.line_segment([p0, p1], egui::Stroke::new(thickness + 2.0 * self.detail_camera_zoom, egui::Color32::from_rgba_unmultiplied(150, 255, 150, (100.0 * den_atp) as u8)));
                                        // Internal core
                                        painter.line_segment([p0, p1], egui::Stroke::new(thickness, egui::Color32::from_rgb((100.0 * den_atp) as u8, 200, (100.0 * den_atp) as u8)));

                                        // Draw bouton at dendrite tip if it's a synapse site
                                        if seg.syn_index.is_some() {
                                            painter.circle_filled(p0, 0.015 * scale, egui::Color32::from_rgba_unmultiplied(180, 255, 180, 200));
                                            painter.circle_stroke(p0, 0.015 * scale, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(100, 200, 100)));
                                        }
                                    }

                                    // Draw Axon with membrane and terminal boutons
                                    let axon_segments = &axon.segments;
                                    for (si, seg) in axon_segments.iter().enumerate() {
                                        let mut p0_3d = seg.from;
                                        let mut p1_3d = seg.to;
                                        if detail_bouton_pid_enabled {
                                            let idx0 = si.saturating_mul(2);
                                            let idx1 = idx0 + 1;
                                            if detail_bouton_pid_axon.len() <= idx1 {
                                                detail_bouton_pid_axon.resize(idx1 + 1, Pid3State::default());
                                            }
                                            p0_3d = pid_smooth_point(&mut detail_bouton_pid_axon[idx0], p0_3d, dt_s, detail_bouton_pid_kp, detail_bouton_pid_ki, detail_bouton_pid_kd);
                                            p1_3d = pid_smooth_point(&mut detail_bouton_pid_axon[idx1], p1_3d, dt_s, detail_bouton_pid_kp, detail_bouton_pid_ki, detail_bouton_pid_kd);
                                        }
                                        let p0 = project(p0_3d, DetailPart::Axon);
                                        let p1 = project(p1_3d, DetailPart::Axon);

                                        if let Some(mpos) = mouse_pos {
                                            if dist_point_to_segment(mpos, p0, p1) <= 5.0 {
                                                let mut msg = format!("Axon Segment {} - Length: {:.3}, Stimuli: {:.3}", si, seg.length, seg.stimuli);
                                                if let Some(sidx) = seg.syn_index {
                                                    if let Some(syn) = active_runner.morph.synapses.get(sidx) {
                                                        msg.push_str(&format!("\nSynapse: w={:.3}, delay={:.1}ms", syn.weight, syn.delay_ms));
                                                    }
                                                }
                                                detail_tooltip = Some(msg);
                                            }
                                        }

                                        let mut thickness = 0.012 * scale;
                                        let ax_atp = axon.atp;

                                        // Axon hillock flare at the first segment of a trunk
                                        if seg.is_trunk && seg.parent_idx.is_none() {
                                            let flare_r = soma_r * 0.4;
                                            painter.add(egui::Shape::convex_polygon(
                                                vec![
                                                    soma_p + egui::vec2(0.0, -flare_r),
                                                    soma_p + egui::vec2(0.0, flare_r),
                                                    p1,
                                                ],
                                                egui::Color32::from_rgba_unmultiplied(255, 150, 150, 80),
                                                egui::Stroke::NONE
                                            ));
                                            thickness *= 1.5;
                                        }

                                        // Membrane border
                                        painter.line_segment([p0, p1], egui::Stroke::new(thickness + 2.0 * self.detail_camera_zoom, egui::Color32::from_rgba_unmultiplied(255, 150, 150, (100.0 * ax_atp) as u8)));
                                        // Internal core
                                        painter.line_segment([p0, p1], egui::Stroke::new(thickness, egui::Color32::from_rgb(200, (100.0 * ax_atp) as u8, (100.0 * ax_atp) as u8)));

                                        // Draw bouton at axon terminal if it's a synapse site
                                        if seg.syn_index.is_some() {
                                            painter.circle_filled(p1, 0.018 * scale, egui::Color32::from_rgba_unmultiplied(255, 180, 180, 220));
                                            painter.circle_stroke(p1, 0.018 * scale, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(200, 100, 100)));
                                        }
                                    }

                                    // Info overlay
                                    painter.text(rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, format!("ATP Level: {:.1}%", soma.atp * 100.0), egui::FontId::proportional(14.0), atp_col);

                                    if let (Some(text), Some(_mpos)) = (detail_tooltip, mouse_pos) {
                                        #[allow(deprecated)]
                                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("detail_tooltip"), |ui| {
                                            ui.label(text);
                                        });
                                    }
                                } else {
                                    // Placeholder for neurons without full morphology
                                    let center = rect.center() + detail_cam_pan;
                                    painter.circle_filled(center, 20.0 * detail_camera_zoom, egui::Color32::from_rgb(200, 200, 200));
                                    ui.label("Biological morphology data not available for this neuron type yet.");
                                }
                            }
                        }
                    } else {
                        ui.label("No neuron selected. Right-click a neuron in the main view and select 'Detailed biological view'.");
                    }
                });

            // Sync back mutations
            self.selected_neuron_pick = selected_neuron_pick;
            self.detail_camera_zoom = detail_camera_zoom;
            self.detail_camera_yaw = detail_camera_yaw;
            self.detail_camera_pitch = detail_camera_pitch;
            self.detail_cam_pan = detail_cam_pan;
            self.detail_camera_pos = detail_camera_pos;
            self.detail_bio_orient = detail_bio_orient;
            #[cfg(all(feature = "morpho", feature = "growth3d"))]
            {
                self.detail_bouton_pid_enabled = detail_bouton_pid_enabled;
                self.detail_bouton_pid_kp = detail_bouton_pid_kp;
                self.detail_bouton_pid_ki = detail_bouton_pid_ki;
                self.detail_bouton_pid_kd = detail_bouton_pid_kd;
                self.detail_bouton_pid_axon = detail_bouton_pid_axon;
                self.detail_bouton_pid_dend = detail_bouton_pid_dend;
                self.detail_bouton_pid_pick = detail_bouton_pid_pick;
            }
            self.detail_timescale = detail_timescale;
            self.detail_time_offset = detail_time_offset;
            self.detail_paused = detail_paused;
            self.detail_last_neuron = detail_last_neuron;
            self.detail_waiting_for_activation = detail_waiting_for_activation;
            self.show_neuron_detail = open;
        }
    }
}
