//! TCP-based NN server: hosts the Runner out-of-process and exchanges length-prefixed frames
//! with a client over TCP — enabling cross-platform clients (Unity C#, Unreal Engine C++,
//! Python, etc.) that cannot use Unix domain sockets.
//!
//! Protocol (per-connection, full-duplex, synchronous request/response):
//!
//! Each message is framed as:
//!   [u32 LE: payload_byte_count][payload_bytes...]
//!
//! The payload is one of:
//! - JSON handshake  (payload[0] == b'{'):
//!     { "s_names": [...], "o_names": [...], "sensory": N, "output": M, ... }
//!     Server echoes: {"expected_s": N, "expected_o": M}
//! - AER1 packet  (payload[0..4] == b"AER1"):
//!     Preferred spike exchange.  Decode → step runner → encode AER response.
//! - Raw float packet (anything else):
//!     [f32 LE t_ms] + [S × f32 LE sensory values]  →  [O × f32 LE outputs]
//!
//! Multiple clients can connect simultaneously; each connection gets its own Runner clone.
//! A shared Arc<Mutex<Runner>> is the authoritative model; each client thread locks it only
//! for the step() call, so clients serialize on the neural simulation but not on I/O.
//!
//! Run (release recommended):
//!   cargo run --release --features ui,robot_io --example nn_tcp_server -- \
//!     --tcp 127.0.0.1:7890 --sensory 25 --output 11 --threshold 0.2 \
//!     [--config config.json] [--network network_celegans.json] [--ui]
//!
//! The optional --ui flag prints a periodic status digest to stdout (no eframe window is
//! launched here; cross-platform TCP clients typically run headless).

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(all(feature = "ui", feature = "robot_io"))]
use aarnn_rust::aer::{decode_spikes, encode_spikes};
#[cfg(all(feature = "ui", feature = "robot_io"))]
use aarnn_rust::bridge::{IoMapping, PortKind, PortSpec, Quantizer};
#[cfg(feature = "ui")]
use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
#[cfg(feature = "ui")]
use aarnn_rust::runner::Runner;

// ---------------------------------------------------------------------------
// Handshake frame (same schema as nn_uds_server)
// ---------------------------------------------------------------------------

#[cfg(all(feature = "ui", feature = "robot_io"))]
#[derive(Debug, Default, serde::Deserialize)]
struct HandshakeFrame {
    #[serde(default)]
    s_names: Vec<String>,
    #[serde(default)]
    o_names: Vec<String>,
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
    #[serde(default)]
    dt_ms: Option<f32>,
}

// ---------------------------------------------------------------------------
// Server arguments
// ---------------------------------------------------------------------------

#[cfg(all(feature = "ui", feature = "robot_io"))]
#[derive(Debug, Clone)]
struct ServerArgs {
    tcp_addr: String,
    num_sensory_neurons: usize,
    num_output_neurons: usize,
    config_path: Option<String>,
    network_path: Option<String>,
    spike_threshold: f32,
    enable_ui: bool,
    aer_sensory_base: u32,
    aer_output_base: u32,
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn parse_server_args() -> ServerArgs {
    // Minimal, manual parsing — avoids pulling clap into examples.
    let mut tcp_addr = "127.0.0.1:7890".to_string();
    let mut num_sensory_neurons = 25usize;
    let mut num_output_neurons = 11usize;
    let mut config_path: Option<String> = None;
    let mut network_path: Option<String> = None;
    let mut spike_threshold = 0.5f32;
    let mut enable_ui = false;
    let mut aer_sensory_base = 4096u32;
    let mut aer_output_base = 16384u32;
    let mut args_iterator = std::env::args().skip(1);
    while let Some(arg) = args_iterator.next() {
        match arg.as_str() {
            "--tcp" => {
                if let Some(value) = args_iterator.next() {
                    tcp_addr = value;
                }
            }
            "--sensory" => {
                if let Some(value) = args_iterator.next() {
                    num_sensory_neurons = value.parse().unwrap_or(num_sensory_neurons);
                }
            }
            "--output" => {
                if let Some(value) = args_iterator.next() {
                    num_output_neurons = value.parse().unwrap_or(num_output_neurons);
                }
            }
            "--threshold" => {
                if let Some(value) = args_iterator.next() {
                    spike_threshold = value.parse().unwrap_or(spike_threshold);
                }
            }
            "--config" => {
                if let Some(value) = args_iterator.next() {
                    config_path = Some(value);
                }
            }
            "--network" => {
                if let Some(value) = args_iterator.next() {
                    network_path = Some(value);
                }
            }
            "--aer-sensory-base" => {
                if let Some(value) = args_iterator.next() {
                    aer_sensory_base = value.parse().unwrap_or(aer_sensory_base);
                }
            }
            "--aer-output-base" => {
                if let Some(value) = args_iterator.next() {
                    aer_output_base = value.parse().unwrap_or(aer_output_base);
                }
            }
            "--ui" => enable_ui = true,
            _ => {}
        }
    }
    ServerArgs {
        tcp_addr,
        num_sensory_neurons,
        num_output_neurons,
        config_path,
        network_path,
        spike_threshold,
        enable_ui,
        aer_sensory_base,
        aer_output_base,
    }
}

// ---------------------------------------------------------------------------
// I/O mapping helpers (mirrors nn_uds_server exactly)
// ---------------------------------------------------------------------------

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn build_mapping(num_sensory_neurons: usize, num_output_neurons: usize) -> IoMapping {
    build_mapping_with_names(num_sensory_neurons, num_output_neurons, &[], &[])
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn build_mapping_with_names(
    num_sensory_neurons: usize,
    num_output_neurons: usize,
    sensory_names: &[String],
    output_names: &[String],
) -> IoMapping {
    let sensory_size = if sensory_names.is_empty() {
        num_sensory_neurons.max(1)
    } else {
        sensory_names.len().max(1)
    };
    let output_size = if output_names.is_empty() {
        num_output_neurons.max(1)
    } else {
        output_names.len().max(1)
    };

    let mut io_mapping = IoMapping::new(sensory_size, output_size);

    if sensory_names.is_empty() {
        io_mapping.add_port(PortSpec::new(
            "__S_ALL__",
            PortKind::Sensor,
            0,
            sensory_size,
        ));
    } else {
        for (idx, name) in sensory_names.iter().enumerate() {
            io_mapping.add_port(PortSpec::new(name.clone(), PortKind::Sensor, idx, 1));
        }
    }

    if output_names.is_empty() {
        io_mapping.add_port(PortSpec::new(
            "__O_ALL__",
            PortKind::Actuator,
            0,
            output_size,
        ));
    } else {
        for (idx, name) in output_names.iter().enumerate() {
            io_mapping.add_port(PortSpec::new(name.clone(), PortKind::Actuator, idx, 1));
        }
    }

    io_mapping
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn pick_nonzero(candidates: &[Option<usize>], fallback: usize) -> usize {
    for candidate in candidates {
        if let Some(value) = candidate {
            if *value > 0 {
                return *value;
            }
        }
    }
    fallback.max(1)
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn resolve_handshake_sizes(
    handshake: &HandshakeFrame,
    fallback_s: usize,
    fallback_o: usize,
) -> (usize, usize) {
    let sensory_count = pick_nonzero(
        &[
            (!handshake.s_names.is_empty()).then_some(handshake.s_names.len()),
            handshake.sensory,
            handshake.expected_s,
            handshake.num_sensory_neurons,
        ],
        fallback_s,
    );
    let output_count = pick_nonzero(
        &[
            (!handshake.o_names.is_empty()).then_some(handshake.o_names.len()),
            handshake.output,
            handshake.expected_o,
            handshake.num_output_neurons,
        ],
        fallback_o,
    );
    (sensory_count, output_count)
}

// ---------------------------------------------------------------------------
// Model loading (mirrors nn_uds_server, log prefix updated)
// ---------------------------------------------------------------------------

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn load_startup_model(server_args: &ServerArgs) -> (NetworkConfig, Option<String>, usize, usize) {
    let mut startup_snapshot_json: Option<String> = None;
    let mut net_cfg = NetworkConfig {
        num_sensory_neurons: server_args.num_sensory_neurons,
        num_hidden_layers: 2,
        num_hidden_per_layer_initial: 32,
        num_output_neurons: server_args.num_output_neurons,
        ..NetworkConfig::default()
    };

    if let Some(config_path) = server_args.config_path.as_deref() {
        if std::path::Path::new(config_path).exists() {
            match std::fs::read_to_string(config_path) {
                Ok(raw) => {
                    if server_args.network_path.is_none() {
                        if let Ok(snapshot) =
                            serde_json::from_str::<aarnn_rust::runner::Snapshot>(&raw)
                        {
                            startup_snapshot_json = Some(raw);
                            net_cfg = snapshot.net;
                        } else if let Ok(parsed) = serde_json::from_str::<NetworkConfig>(&raw) {
                            net_cfg = parsed;
                        } else {
                            eprintln!(
                                "[nn_tcp_server] unable to parse config JSON from {}",
                                config_path
                            );
                        }
                    } else if let Ok(parsed) = serde_json::from_str::<NetworkConfig>(&raw) {
                        net_cfg = parsed;
                    } else {
                        eprintln!(
                            "[nn_tcp_server] unable to parse config JSON from {}",
                            config_path
                        );
                    }
                }
                Err(err) => {
                    eprintln!(
                        "[nn_tcp_server] failed reading config {}: {}",
                        config_path, err
                    );
                }
            }
        } else {
            eprintln!(
                "[nn_tcp_server] config file not found (continuing with defaults): {}",
                config_path
            );
        }
    }

    if let Some(network_path) = server_args.network_path.as_deref() {
        match std::fs::read_to_string(network_path) {
            Ok(raw) => match serde_json::from_str::<aarnn_rust::runner::Snapshot>(&raw) {
                Ok(snapshot) => {
                    startup_snapshot_json = Some(raw);
                    net_cfg = snapshot.net;
                }
                Err(err) => {
                    eprintln!(
                        "[nn_tcp_server] failed parsing snapshot {}: {}",
                        network_path, err
                    );
                }
            },
            Err(err) => {
                eprintln!(
                    "[nn_tcp_server] failed reading snapshot {}: {}",
                    network_path, err
                );
            }
        }
    }

    let use_model_dims = server_args.config_path.is_some() || server_args.network_path.is_some();
    let initial_s = if use_model_dims {
        net_cfg.num_sensory_neurons
    } else {
        server_args.num_sensory_neurons
    };
    let initial_o = if use_model_dims {
        net_cfg.num_output_neurons
    } else {
        server_args.num_output_neurons
    };

    (
        net_cfg,
        startup_snapshot_json,
        initial_s.max(1),
        initial_o.max(1),
    )
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn build_runner(base_config: &NetworkConfig, snapshot_json: Option<&str>) -> Runner {
    let lif_params = LIFParams::default();
    let stdp_params = STDPParams::default();
    let mut runner = Runner::new(
        lif_params,
        stdp_params,
        base_config.clone(),
        aarnn_rust::sim::NeuronModel::Lif,
        aarnn_rust::sim::Learning::Stdp,
    );
    if let Some(json) = snapshot_json {
        if let Err(err) = runner.import_network_json(json) {
            eprintln!("[nn_tcp_server] startup snapshot import failed: {}", err);
        }
    }
    runner
}

// ---------------------------------------------------------------------------
// TCP framing helpers
// ---------------------------------------------------------------------------

/// Read exactly `n` bytes from a TCP stream into `buf[..n]`.
#[cfg(all(feature = "ui", feature = "robot_io"))]
fn read_exact_tcp(stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        match stream.read(&mut buf[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed",
                ));
            }
            Ok(n) => offset += n,
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Read a length-prefixed frame: [u32 LE length][payload].
/// Returns the payload bytes, or an error if the connection closed cleanly (EOF on length read)
/// or a real I/O error occurred.
#[cfg(all(feature = "ui", feature = "robot_io"))]
fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    // Try to read the 4-byte length prefix.  A clean EOF here means the client disconnected.
    match stream.read(&mut len_buf[..1]) {
        Ok(0) => {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client disconnected",
            ));
        }
        Ok(_) => {}
        Err(err) => return Err(err),
    }
    // Read the remaining 3 bytes of the length field.
    read_exact_tcp(stream, &mut len_buf[1..])?;
    let payload_len = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0u8; payload_len];
    read_exact_tcp(stream, &mut payload)?;
    Ok(payload)
}

/// Write a length-prefixed frame: [u32 LE length][payload].
#[cfg(all(feature = "ui", feature = "robot_io"))]
fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> io::Result<()> {
    let len = payload.len() as u32;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

// ---------------------------------------------------------------------------
// Per-connection handler
// ---------------------------------------------------------------------------

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn handle_client(
    mut stream: TcpStream,
    shared_runner: Arc<Mutex<Runner>>,
    _startup_cfg: NetworkConfig,
    _startup_snapshot_json: Option<String>,
    initial_s: usize,
    initial_o: usize,
    quantizer: Quantizer,
    aer_s_base: u32,
    aer_o_base: u32,
    last_inputs: Arc<Mutex<Vec<f32>>>,
    last_outputs: Arc<Mutex<Vec<f32>>>,
) {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    eprintln!("[nn_tcp_server] client connected: {}", peer);

    // Each connection keeps its own I/O mapping. The neural state lives in the
    // already-loaded shared runner and is locked only while resizing or stepping.
    let mut io_mapping = build_mapping(initial_s, initial_o);
    let mut active_s_names: Vec<String> = Vec::new();
    let mut active_o_names: Vec<String> = Vec::new();

    // Use the already-loaded runner owned by the server. Large profiles such as
    // Drosophila BANC carry snapshots large enough that re-importing per client can
    // exceed engine-side handshake timeouts.
    {
        let mut runner = match shared_runner.lock() {
            Ok(runner) => runner,
            Err(err) => {
                eprintln!("[nn_tcp_server] runner lock poisoned for {}: {}", peer, err);
                return;
            }
        };
        if runner.net.num_sensory_neurons != io_mapping.sensory_size {
            runner.resize_sensory(io_mapping.sensory_size);
        }
        if runner.net.num_output_neurons != io_mapping.output_size {
            runner.resize_output(io_mapping.output_size);
        }
    }

    let mut in_buf = vec![0f32; io_mapping.total_sensor_values()];
    let mut spk_s = vec![0i8; io_mapping.sensory_size];
    let mut out_buf = vec![0f32; io_mapping.total_actuator_values()];
    let mut ipc_dt_ms: f32 = std::env::var("NM_IPC_DT_MS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= 1000.0)
        .unwrap_or(1.0);

    // Set a generous read timeout so a stalled client doesn't park a thread forever,
    // but keep it long enough to survive engine-side hitches (e.g. first-run shader
    // compilation in Unreal/Unity can freeze the game thread for many seconds).
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));

    loop {
        let payload = match read_frame(&mut stream) {
            Ok(p) => p,
            Err(err) => {
                if err.kind() != io::ErrorKind::UnexpectedEof {
                    eprintln!("[nn_tcp_server] read error from {}: {}", peer, err);
                }
                break;
            }
        };

        if payload.is_empty() {
            continue;
        }

        // ---- JSON handshake ------------------------------------------------
        if payload[0] == b'{' {
            match serde_json::from_slice::<HandshakeFrame>(&payload) {
                Ok(handshake) => {
                    if let Some(handshake_dt_ms) = handshake
                        .dt_ms
                        .filter(|v| v.is_finite() && *v > 0.0 && *v <= 1000.0)
                    {
                        if (ipc_dt_ms - handshake_dt_ms).abs() > f32::EPSILON {
                            eprintln!(
                                "[nn_tcp_server] {} applied handshake dt_ms={:.3}",
                                peer, handshake_dt_ms
                            );
                        }
                        ipc_dt_ms = handshake_dt_ms;
                    }

                    let (mut requested_s, mut requested_o) = resolve_handshake_sizes(
                        &handshake,
                        io_mapping.sensory_size,
                        io_mapping.output_size,
                    );

                    let requested_s_names = handshake.s_names;
                    let requested_o_names = handshake.o_names;
                    if !requested_s_names.is_empty() {
                        requested_s = requested_s_names.len();
                    }
                    if !requested_o_names.is_empty() {
                        requested_o = requested_o_names.len();
                    }

                    let names_changed = (!requested_s_names.is_empty()
                        && requested_s_names != active_s_names)
                        || (!requested_o_names.is_empty() && requested_o_names != active_o_names);
                    let shape_changed = requested_s != io_mapping.sensory_size
                        || requested_o != io_mapping.output_size;

                    if names_changed || shape_changed {
                        if requested_s_names.is_empty() {
                            active_s_names.clear();
                        } else {
                            active_s_names = requested_s_names;
                        }
                        if requested_o_names.is_empty() {
                            active_o_names.clear();
                        } else {
                            active_o_names = requested_o_names;
                        }

                        io_mapping = build_mapping_with_names(
                            requested_s,
                            requested_o,
                            &active_s_names,
                            &active_o_names,
                        );
                        {
                            let mut runner = match shared_runner.lock() {
                                Ok(runner) => runner,
                                Err(err) => {
                                    eprintln!(
                                        "[nn_tcp_server] runner lock poisoned for {}: {}",
                                        peer, err
                                    );
                                    break;
                                }
                            };
                            if runner.net.num_sensory_neurons != io_mapping.sensory_size {
                                runner.resize_sensory(io_mapping.sensory_size);
                            }
                            if runner.net.num_output_neurons != io_mapping.output_size {
                                runner.resize_output(io_mapping.output_size);
                            }
                        }
                        in_buf.resize(io_mapping.total_sensor_values(), 0.0);
                        spk_s.resize(io_mapping.sensory_size, 0);
                        out_buf.resize(io_mapping.total_actuator_values(), 0.0);
                        if let Ok(mut li) = last_inputs.lock() {
                            li.resize(in_buf.len(), 0.0);
                        }
                        if let Ok(mut lo) = last_outputs.lock() {
                            lo.resize(out_buf.len(), 0.0);
                        }
                        eprintln!(
                            "[nn_tcp_server] {} applied handshake mapping: S={} O={} \
                             (named_s={} named_o={})",
                            peer,
                            io_mapping.sensory_size,
                            io_mapping.output_size,
                            active_s_names.len(),
                            active_o_names.len()
                        );
                    }

                    let hint = format!(
                        "{{\"expected_s\":{},\"expected_o\":{}}}",
                        io_mapping.sensory_size, io_mapping.output_size
                    );
                    if let Err(err) = write_frame(&mut stream, hint.as_bytes()) {
                        eprintln!("[nn_tcp_server] handshake reply error to {}: {}", peer, err);
                        break;
                    }
                }
                Err(err) => {
                    eprintln!("[nn_tcp_server] {} bad handshake JSON: {}", peer, err);
                }
            }
            continue;
        }

        // ---- AER1 spike packet --------------------------------------------
        if payload.len() >= 4 && &payload[..4] == b"AER1" {
            spk_s.fill(0);
            if decode_spikes(&payload, aer_s_base, &mut spk_s).is_err() {
                eprintln!("[nn_tcp_server] {} bad AER payload", peer);
                continue;
            }
            let (out_vec, ts_us) = {
                let mut runner = match shared_runner.lock() {
                    Ok(runner) => runner,
                    Err(err) => {
                        eprintln!("[nn_tcp_server] runner lock poisoned for {}: {}", peer, err);
                        break;
                    }
                };
                runner.set_dt(ipc_dt_ms as f64);
                let _out = runner.step(Some(&spk_s));
                (
                    runner.last_spk_o.iter().copied().collect::<Vec<i8>>(),
                    (runner.t_ms * 1000.0) as u64,
                )
            };
            let mut aer_response = encode_spikes(ts_us, aer_o_base, &out_vec);
            // If no spikes fired, still send a valid AER1 header so the client can parse it.
            if aer_response.is_empty() {
                aer_response.extend_from_slice(b"AER1");
                aer_response.extend_from_slice(&ts_us.to_le_bytes());
            }

            // Update shared visualization state.
            if let Ok(mut li) = last_inputs.lock() {
                if li.len() != spk_s.len() {
                    li.resize(spk_s.len(), 0.0);
                }
                for (i, v) in spk_s.iter().enumerate() {
                    li[i] = *v as f32;
                }
            }
            if let Ok(mut lo) = last_outputs.lock() {
                if lo.len() != out_vec.len() {
                    lo.resize(out_vec.len(), 0.0);
                }
                for (i, v) in out_vec.iter().enumerate() {
                    lo[i] = *v as f32;
                }
            }

            if let Err(err) = write_frame(&mut stream, &aer_response) {
                eprintln!("[nn_tcp_server] AER reply error to {}: {}", peer, err);
                break;
            }
            continue;
        }

        // ---- Legacy raw-float packet ---------------------------------------
        let expected_bytes = (1 + io_mapping.sensory_size) * 4;
        if payload.len() != expected_bytes {
            eprintln!(
                "[nn_tcp_server] {} bad frame size: got {}, want {}",
                peer,
                payload.len(),
                expected_bytes
            );
            // Send a size hint so the client can self-correct.
            let hint = format!(
                "{{\"expected_s\":{},\"expected_o\":{}}}",
                io_mapping.sensory_size, io_mapping.output_size
            );
            if let Err(err) = write_frame(&mut stream, hint.as_bytes()) {
                eprintln!("[nn_tcp_server] size-hint reply error to {}: {}", peer, err);
                break;
            }
            continue;
        }

        let mut reader: &[u8] = &payload;
        let float_from_le_bytes = |bytes: &mut &[u8]| -> f32 {
            let (head, rest) = bytes.split_at(4);
            *bytes = rest;
            f32::from_le_bytes(head.try_into().unwrap())
        };
        let current_time_ms = float_from_le_bytes(&mut reader) as f64;
        for i in 0..io_mapping.sensory_size {
            in_buf[i] = float_from_le_bytes(&mut reader);
        }

        quantizer.to_spikes(&io_mapping, &in_buf, &mut spk_s);
        let out_spikes = {
            let mut runner = match shared_runner.lock() {
                Ok(runner) => runner,
                Err(err) => {
                    eprintln!("[nn_tcp_server] runner lock poisoned for {}: {}", peer, err);
                    break;
                }
            };
            runner.set_dt(current_time_ms);
            let out = runner.step(Some(&spk_s));
            out.spk_o.iter().copied().collect::<Vec<i8>>()
        };
        out_buf.fill(0.0);
        quantizer.from_spikes(&io_mapping, &out_spikes, &mut out_buf);

        if let Ok(mut li) = last_inputs.lock() {
            *li = in_buf.clone();
        }
        if let Ok(mut lo) = last_outputs.lock() {
            *lo = out_buf.clone();
        }

        let mut output_bytes = vec![0u8; io_mapping.output_size * 4];
        for (i, v) in out_buf.iter().enumerate() {
            output_bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        if let Err(err) = write_frame(&mut stream, &output_bytes) {
            eprintln!("[nn_tcp_server] float reply error to {}: {}", peer, err);
            break;
        }
    }

    eprintln!("[nn_tcp_server] client disconnected: {}", peer);
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn main() -> io::Result<()> {
    let server_args = parse_server_args();
    let (startup_cfg, startup_snapshot_json, initial_s, initial_o) =
        load_startup_model(&server_args);
    eprintln!(
        "[nn_tcp_server] addr={}, S={} O={} thr={} ui={} \
         aer_s_base={} aer_o_base={} config={} network={}",
        server_args.tcp_addr,
        initial_s,
        initial_o,
        server_args.spike_threshold,
        server_args.enable_ui,
        server_args.aer_sensory_base,
        server_args.aer_output_base,
        server_args.config_path.as_deref().unwrap_or("none"),
        server_args.network_path.as_deref().unwrap_or("none"),
    );

    let quantizer = Quantizer {
        threshold: server_args.spike_threshold,
        probabilistic: true,
        ..Quantizer::default()
    };

    // Shared visualization state (updated by each client handler thread).
    let last_inputs = Arc::new(Mutex::new(vec![0f32; initial_s]));
    let last_outputs = Arc::new(Mutex::new(vec![0f32; initial_o]));

    // Shared authoritative Runner (each connection also gets its own local Runner; this
    // is kept for future cross-client synchronisation hooks).
    let shared_runner = Arc::new(Mutex::new(build_runner(
        &startup_cfg,
        startup_snapshot_json.as_deref(),
    )));

    let listener = TcpListener::bind(&server_args.tcp_addr)?;
    eprintln!(
        "[nn_tcp_server] listening on {}",
        listener
            .local_addr()
            .unwrap_or_else(|_| server_args.tcp_addr.parse().expect("valid addr"))
    );

    // Accept loop in a background thread so the main thread can run the UI (or park).
    let accept_runner = Arc::clone(&shared_runner);
    let accept_inputs = Arc::clone(&last_inputs);
    let accept_outputs = Arc::clone(&last_outputs);
    let accept_cfg = startup_cfg.clone();
    let accept_snapshot = startup_snapshot_json.clone();
    let accept_quantizer = quantizer;
    let accept_aer_s = server_args.aer_sensory_base;
    let accept_aer_o = server_args.aer_output_base;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(tcp_stream) => {
                    let runner_clone = Arc::clone(&accept_runner);
                    let inputs_clone = Arc::clone(&accept_inputs);
                    let outputs_clone = Arc::clone(&accept_outputs);
                    let cfg_clone = accept_cfg.clone();
                    let snap_clone = accept_snapshot.clone();
                    let q_clone = accept_quantizer.clone();
                    std::thread::spawn(move || {
                        handle_client(
                            tcp_stream,
                            runner_clone,
                            cfg_clone,
                            snap_clone,
                            initial_s,
                            initial_o,
                            q_clone,
                            accept_aer_s,
                            accept_aer_o,
                            inputs_clone,
                            outputs_clone,
                        );
                    });
                }
                Err(err) => {
                    eprintln!("[nn_tcp_server] accept error: {}", err);
                }
            }
        }
    });

    if server_args.enable_ui {
        // TCP server targets headless / cross-platform clients; launching a full eframe
        // window from the same process is optional and complex.  Print a periodic status
        // digest to stdout so operators can confirm the server is alive and active.
        #[cfg(feature = "ui")]
        {
            eprintln!("[nn_tcp_server] --ui active: printing status every 5 s (no eframe window)");
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let inputs_snap = last_inputs.lock().map(|g| g.clone()).unwrap_or_default();
                let outputs_snap = last_outputs.lock().map(|g| g.clone()).unwrap_or_default();
                let in_sum: f32 = inputs_snap.iter().sum();
                let out_sum: f32 = outputs_snap.iter().sum();
                println!(
                    "[nn_tcp_server] status  S={} sum={:.3}  O={} sum={:.3}",
                    inputs_snap.len(),
                    in_sum,
                    outputs_snap.len(),
                    out_sum,
                );
            }
        }
        #[cfg(not(feature = "ui"))]
        {
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
    } else {
        // Park the main thread; all work happens in the accept/handler threads.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    // Unreachable.
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(not(all(feature = "ui", feature = "robot_io")))]
fn main() {
    println!("nn_tcp_server example requires the 'ui' and 'robot_io' features.");
    println!(
        "Run with:  cargo run --release --features ui,robot_io --example nn_tcp_server -- \
         --tcp 127.0.0.1:7890 --sensory 25 --output 11"
    );
}
