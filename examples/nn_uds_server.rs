//! UDS-based NN server: hosts the Runner out-of-process and exchanges frames with a client
//! (e.g., a Webots controller) over a Unix datagram socket.
//!
//! Protocol:
//! - Preferred (AER): request/response payloads start with `AER1` and carry spike events.
//! - Legacy (floats, little-endian):
//!   - Request  frame: [f32 t_ms] + [S f32 sensory]
//!   - Response frame: [O f32 outputs]
//!
//! Run (release recommended):
//!   cargo run --release --features ui,robot_io --example nn_uds_server -- \
//!     --socket /tmp/aarnn_rust.nn --sensory 25 --output 11 --threshold 0.2 [--ui]
//!
//! The optional --ui flag opens a lightweight window (eframe) showing last inputs/outputs.

use std::io;
use std::os::unix::net::UnixDatagram;
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
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
#[derive(Debug, Clone)]
struct ServerArgs {
    socket_path: String,
    num_sensory_neurons: usize,
    num_output_neurons: usize,
    spike_threshold: f32,
    enable_ui: bool,
    aer_sensory_base: u32,
    aer_output_base: u32,
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn parse_server_args() -> ServerArgs {
    // Minimal, manual parsing to avoid pulling clap in examples
    let mut socket_path = "/tmp/aarnn_rust.nn".to_string();
    let mut num_sensory_neurons = 25usize;
    let mut num_output_neurons = 11usize;
    let mut spike_threshold = 0.5f32;
    let mut enable_ui = false;
    let mut aer_sensory_base = 4096u32;
    let mut aer_output_base = 16384u32;
    let mut args_iterator = std::env::args().skip(1);
    while let Some(arg) = args_iterator.next() {
        match arg.as_str() {
            "--socket" => {
                if let Some(value) = args_iterator.next() {
                    socket_path = value;
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
        socket_path,
        num_sensory_neurons,
        num_output_neurons,
        spike_threshold,
        enable_ui,
        aer_sensory_base,
        aer_output_base,
    }
}

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

fn unlink_if_exists(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn build_runner(num_sensory_neurons: usize, num_output_neurons: usize) -> Runner {
    let lif_params = LIFParams::default();
    let stdp_params = STDPParams::default();
    let network_config = NetworkConfig {
        num_sensory_neurons: num_sensory_neurons,
        num_hidden_layers: 2,
        num_hidden_per_layer_initial: 32,
        num_output_neurons: num_output_neurons,
        ..NetworkConfig::default()
    };
    Runner::new(
        lif_params,
        stdp_params,
        network_config,
        aarnn_rust::sim::NeuronModel::Lif,
        aarnn_rust::sim::Learning::Stdp,
    )
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn main() -> io::Result<()> {
    let server_args = parse_server_args();
    eprintln!(
        "[nn_uds_server] socket={}, S={}, O={}, thr={}, ui={}, aer_s_base={}, aer_o_base={}",
        server_args.socket_path,
        server_args.num_sensory_neurons,
        server_args.num_output_neurons,
        server_args.spike_threshold,
        server_args.enable_ui,
        server_args.aer_sensory_base,
        server_args.aer_output_base
    );

    let quantizer = Quantizer {
        threshold: server_args.spike_threshold,
        probabilistic: true,
    };

    // Shared state for optional visualization
    let last_inputs = Arc::new(Mutex::new(vec![0f32; server_args.num_sensory_neurons]));
    let last_outputs = Arc::new(Mutex::new(vec![0f32; server_args.num_output_neurons]));

    // Socket server thread
    let socket_path = server_args.socket_path.clone();
    let s_count = server_args.num_sensory_neurons;
    let o_count = server_args.num_output_neurons;
    let aer_s_base = server_args.aer_sensory_base;
    let aer_o_base = server_args.aer_output_base;
    let quantizer_srv = quantizer;
    let last_inputs_for_viz = last_inputs.clone();
    let last_outputs_for_viz = last_outputs.clone();
    std::thread::spawn(move || {
        unlink_if_exists(&socket_path);
        let server_socket = UnixDatagram::bind(&socket_path).expect("bind UDS");
        // Allow some backlog and avoid leftover non-response by setting read timeout (optional)
        let _ = server_socket.set_read_timeout(Some(Duration::from_millis(1000)));

        let mut active_s_names: Vec<String> = Vec::new();
        let mut active_o_names: Vec<String> = Vec::new();
        let mut io_mapping_srv = build_mapping(s_count.max(1), o_count.max(1));
        let mut runner = build_runner(io_mapping_srv.sensory_size, io_mapping_srv.output_size);
        let mut expected_bytes = (1 + io_mapping_srv.sensory_size) * 4;
        let mut request_buffer = vec![0u8; expected_bytes.max(8192)];
        let mut output_buffer = vec![0u8; io_mapping_srv.output_size * 4];
        let mut in_buf = vec![0f32; io_mapping_srv.total_sensor_values()];
        let mut spk_s = vec![0i8; io_mapping_srv.sensory_size];
        let mut out_buf = vec![0f32; io_mapping_srv.total_actuator_values()];

        loop {
            let (bytes_received, peer_address) = match server_socket.recv_from(&mut request_buffer)
            {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("[nn_uds_server] recv error: {error:?}");
                    continue;
                }
            };
            let payload = &request_buffer[..bytes_received];
            if payload.is_empty() {
                continue;
            }
            if payload[0] == b'{' {
                match serde_json::from_slice::<HandshakeFrame>(payload) {
                    Ok(handshake) => {
                        let (mut requested_s, mut requested_o) = resolve_handshake_sizes(
                            &handshake,
                            io_mapping_srv.sensory_size,
                            io_mapping_srv.output_size,
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
                            || (!requested_o_names.is_empty()
                                && requested_o_names != active_o_names);
                        let shape_changed = requested_s != io_mapping_srv.sensory_size
                            || requested_o != io_mapping_srv.output_size;

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

                            io_mapping_srv = build_mapping_with_names(
                                requested_s,
                                requested_o,
                                &active_s_names,
                                &active_o_names,
                            );
                            runner = build_runner(
                                io_mapping_srv.sensory_size,
                                io_mapping_srv.output_size,
                            );
                            expected_bytes = (1 + io_mapping_srv.sensory_size) * 4;
                            request_buffer.resize(expected_bytes.max(8192), 0);
                            output_buffer.resize(io_mapping_srv.output_size * 4, 0);
                            in_buf.resize(io_mapping_srv.total_sensor_values(), 0.0);
                            spk_s.resize(io_mapping_srv.sensory_size, 0);
                            out_buf.resize(io_mapping_srv.total_actuator_values(), 0.0);
                            if let Ok(mut li) = last_inputs_for_viz.lock() {
                                li.resize(in_buf.len(), 0.0);
                            }
                            if let Ok(mut lo) = last_outputs_for_viz.lock() {
                                lo.resize(out_buf.len(), 0.0);
                            }
                            eprintln!(
                                "[nn_uds_server] applied handshake mapping: S={} O={} (named_s={} named_o={})",
                                io_mapping_srv.sensory_size,
                                io_mapping_srv.output_size,
                                active_s_names.len(),
                                active_o_names.len()
                            );
                        }

                        if let Some(path) = peer_address.as_pathname() {
                            let hint = format!(
                                "{{\"expected_s\":{},\"expected_o\":{}}}",
                                io_mapping_srv.sensory_size, io_mapping_srv.output_size
                            );
                            let _ = server_socket.send_to(hint.as_bytes(), path);
                        }
                    }
                    Err(error) => {
                        eprintln!("[nn_uds_server] bad handshake JSON: {error}");
                    }
                }
                continue;
            }

            if payload.len() >= 4 && &payload[..4] == b"AER1" {
                spk_s.fill(0);
                if decode_spikes(payload, aer_s_base, &mut spk_s).is_err() {
                    eprintln!("[nn_uds_server] bad AER payload");
                    continue;
                }
                let _out = runner.step(Some(&spk_s));
                let out_vec: Vec<i8> = runner.last_spk_o.iter().copied().collect();
                let ts_us = (runner.t_ms * 1000.0) as u64;
                let mut aer_payload = encode_spikes(ts_us, aer_o_base, &out_vec);
                if aer_payload.is_empty() {
                    aer_payload.extend_from_slice(b"AER1");
                    aer_payload.extend_from_slice(&ts_us.to_le_bytes());
                }

                if let Ok(mut li) = last_inputs_for_viz.lock() {
                    if li.len() != spk_s.len() {
                        li.resize(spk_s.len(), 0.0);
                    }
                    for (i, v) in spk_s.iter().enumerate() {
                        li[i] = *v as f32;
                    }
                }
                if let Ok(mut lo) = last_outputs_for_viz.lock() {
                    if lo.len() != out_vec.len() {
                        lo.resize(out_vec.len(), 0.0);
                    }
                    for (i, v) in out_vec.iter().enumerate() {
                        lo[i] = *v as f32;
                    }
                }

                match peer_address.as_pathname() {
                    Some(path) => {
                        if let Err(error) = server_socket.send_to(&aer_payload, path) {
                            eprintln!("[nn_uds_server] send error: {error:?}");
                        }
                    }
                    None => {
                        eprintln!("[nn_uds_server] peer has no pathname; cannot reply");
                    }
                }
                continue;
            }

            if bytes_received != expected_bytes {
                eprintln!(
                    "[nn_uds_server] bad frame size: got {bytes_received}, want {expected_bytes}"
                );
                if let Some(path) = peer_address.as_pathname() {
                    let hint = format!(
                        "{{\"expected_s\":{},\"expected_o\":{}}}",
                        io_mapping_srv.sensory_size, io_mapping_srv.output_size
                    );
                    let _ = server_socket.send_to(hint.as_bytes(), path);
                }
                continue;
            }

            // Parse current_time_ms + inputs (legacy float path)
            let mut reader = payload;
            let float_from_le_bytes = |bytes: &mut &[u8]| -> f32 {
                let (head, rest) = bytes.split_at(4);
                *bytes = rest;
                f32::from_le_bytes(head.try_into().unwrap())
            };
            let current_time_ms = float_from_le_bytes(&mut reader) as f64;
            for i in 0..io_mapping_srv.sensory_size {
                in_buf[i] = float_from_le_bytes(&mut reader);
            }

            runner.set_dt(current_time_ms);
            quantizer_srv.to_spikes(&io_mapping_srv, &in_buf, &mut spk_s);
            let out = runner.step(Some(&spk_s));
            out_buf.fill(0.0);
            if let Some(spk_o) = out.spk_o.as_slice() {
                quantizer_srv.from_spikes(&io_mapping_srv, spk_o, &mut out_buf);
            }

            if let Ok(mut li) = last_inputs_for_viz.lock() {
                *li = in_buf.clone();
            }
            if let Ok(mut lo) = last_outputs_for_viz.lock() {
                *lo = out_buf.clone();
            }

            for (i, v) in out_buf.iter().enumerate() {
                output_buffer[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            match peer_address.as_pathname() {
                Some(path) => {
                    if let Err(error) = server_socket.send_to(&output_buffer, path) {
                        eprintln!("[nn_uds_server] send error: {error:?}");
                    }
                }
                None => {
                    eprintln!("[nn_uds_server] peer has no pathname; cannot reply");
                }
            }
        }
    });

    if server_args.enable_ui {
        // Minimal eframe window to show last inputs/outputs as bars
        {
            use eframe::{egui, NativeOptions};
            let li = last_inputs.clone();
            let lo = last_outputs.clone();
            let options = NativeOptions::default();
            let _ = eframe::run_native(
                "NN UDS Server",
                options,
                Box::new(move |_cc| {
                    struct Viz {
                        li: Arc<Mutex<Vec<f32>>>,
                        lo: Arc<Mutex<Vec<f32>>>,
                    }
                    impl eframe::App for Viz {
                        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.heading("Inputs (S) and Outputs (O)");
                                if let Ok(li) = self.li.lock() {
                                    draw_bars(ui, &li, "Inputs");
                                }
                                if let Ok(lo) = self.lo.lock() {
                                    draw_bars(ui, &lo, "Outputs");
                                }
                            });
                            ctx.request_repaint_after(Duration::from_millis(33));
                        }
                    }
                    fn draw_bars(ui: &mut egui::Ui, vals: &[f32], title: &str) {
                        ui.label(title);
                        let w = ui.available_width();
                        let h = 120.0;
                        let (rect, _resp) =
                            ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                        let painter = ui.painter_at(rect);
                        let n = vals.len().max(1) as f32;
                        for (i, v) in vals.iter().enumerate() {
                            let x0 = rect.left() + (i as f32) / n * rect.width();
                            let x1 = rect.left() + ((i as f32) + 1.0) / n * rect.width();
                            let y1 = rect.bottom();
                            let y0 = rect.bottom() - (v.clamp(0.0, 1.0)) * rect.height();
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(x0, y0),
                                    egui::pos2(x1 - 1.0, y1),
                                ),
                                0.0,
                                egui::Color32::LIGHT_BLUE,
                            );
                        }
                    }
                    Ok(Box::new(Viz { li, lo }))
                }),
            );
        }
    } else {
        // Park the main thread; the server thread runs indefinitely
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    // Unreachable
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(not(all(feature = "ui", feature = "robot_io")))]
fn main() {
    println!("nn_uds_server example requires the 'ui' and 'robot_io' features.");
}
