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
use std::path::Path;
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
            "--socket" => if let Some(value) = args_iterator.next() { socket_path = value; },
            "--sensory" => if let Some(value) = args_iterator.next() { num_sensory_neurons = value.parse().unwrap_or(num_sensory_neurons); },
            "--output" => if let Some(value) = args_iterator.next() { num_output_neurons = value.parse().unwrap_or(num_output_neurons); },
            "--threshold" => if let Some(value) = args_iterator.next() { spike_threshold = value.parse().unwrap_or(spike_threshold); },
            "--aer-sensory-base" => if let Some(value) = args_iterator.next() { aer_sensory_base = value.parse().unwrap_or(aer_sensory_base); },
            "--aer-output-base" => if let Some(value) = args_iterator.next() { aer_output_base = value.parse().unwrap_or(aer_output_base); },
            "--ui" => enable_ui = true,
            _ => {}
        }
    }
    ServerArgs { socket_path, num_sensory_neurons, num_output_neurons, spike_threshold, enable_ui, aer_sensory_base, aer_output_base }
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn build_mapping(num_sensory_neurons: usize, num_output_neurons: usize) -> IoMapping {
    let mut io_mapping = IoMapping::new(num_sensory_neurons, num_output_neurons);
    io_mapping.add_port(PortSpec::new("__S_ALL__", PortKind::Sensor, 0, num_sensory_neurons));
    io_mapping.add_port(PortSpec::new("__O_ALL__", PortKind::Actuator, 0, num_output_neurons));
    io_mapping
}

fn unlink_if_exists(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(all(feature = "ui", feature = "robot_io"))]
fn build_runner(num_sensory_neurons: usize, num_output_neurons: usize) -> Runner {
    let lif_params = LIFParams::default();
    let stdp_params = STDPParams::default();
    let network_config = NetworkConfig { num_sensory_neurons: num_sensory_neurons, num_hidden_layers: 2, num_hidden_per_layer_initial: 32, num_output_neurons: num_output_neurons, ..NetworkConfig::default() };
    Runner::new(lif_params, stdp_params, network_config, aarnn_rust::sim::NeuronModel::Lif, aarnn_rust::sim::Learning::Stdp)
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

    // Build engine
    let io_mapping = build_mapping(server_args.num_sensory_neurons, server_args.num_output_neurons);
    let quantizer = Quantizer { threshold: server_args.spike_threshold, probabilistic: true };

    // Shared state for optional visualization
    let last_inputs = Arc::new(Mutex::new(vec![0f32; server_args.num_sensory_neurons]));
    let last_outputs = Arc::new(Mutex::new(vec![0f32; server_args.num_output_neurons]));

    // Socket server thread
    let socket_path = server_args.socket_path.clone();
    let s_count = server_args.num_sensory_neurons;
    let o_count = server_args.num_output_neurons;
    let aer_s_base = server_args.aer_sensory_base;
    let aer_o_base = server_args.aer_output_base;
    let io_mapping_srv = io_mapping.clone();
    let quantizer_srv = quantizer;
    let last_inputs_for_viz = last_inputs.clone();
    let last_outputs_for_viz = last_outputs.clone();
    std::thread::spawn(move || {
        unlink_if_exists(&socket_path);
        let server_socket = UnixDatagram::bind(&socket_path).expect("bind UDS");
        // Allow some backlog and avoid leftover non-response by setting read timeout (optional)
        let _ = server_socket.set_read_timeout(Some(Duration::from_millis(1000)));

        let expected_bytes = (1 + io_mapping_srv.sensory_size) * 4;
        let mut request_buffer = vec![0u8; expected_bytes.max(8192)];
        let mut output_buffer = vec![0u8; io_mapping_srv.output_size * 4];
        let mut runner = build_runner(s_count, o_count);
        let mut in_buf = vec![0f32; io_mapping_srv.total_sensor_values()];
        let mut spk_s = vec![0i8; io_mapping_srv.sensory_size];
        let mut out_buf = vec![0f32; io_mapping_srv.total_actuator_values()];

        loop {
            let (bytes_received, peer_address) = match server_socket.recv_from(&mut request_buffer) {
                Ok(result) => result,
                Err(error) => { eprintln!("[nn_uds_server] recv error: {error:?}"); continue; }
            };
            let payload = &request_buffer[..bytes_received];
            if payload.is_empty() {
                continue;
            }
            if payload[0] == b'{' {
                // Handshake JSON (optional); ignore for this example.
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
                    for (i, v) in spk_s.iter().enumerate() { li[i] = *v as f32; }
                }
                if let Ok(mut lo) = last_outputs_for_viz.lock() {
                    for (i, v) in out_vec.iter().enumerate() { lo[i] = *v as f32; }
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
                eprintln!("[nn_uds_server] bad frame size: got {bytes_received}, want {expected_bytes}");
                continue;
            }

            // Parse current_time_ms + inputs (legacy float path)
            let mut reader = payload;
            let mut float_from_le_bytes = |bytes: &mut &[u8]| -> f32 {
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
            if let Some(spk_o) = out.spk_o.as_slice() {
                quantizer_srv.from_spikes(&io_mapping_srv, spk_o, &mut out_buf);
            }

            if let Ok(mut li) = last_inputs_for_viz.lock() { *li = in_buf.clone(); }
            if let Ok(mut lo) = last_outputs_for_viz.lock() { *lo = out_buf.clone(); }

            for (i, v) in out_buf.iter().enumerate() {
                output_buffer[i*4..i*4+4].copy_from_slice(&v.to_le_bytes());
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
                    struct Viz { li: Arc<Mutex<Vec<f32>>>, lo: Arc<Mutex<Vec<f32>>> }
                    impl eframe::App for Viz {
                        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.heading("Inputs (S) and Outputs (O)");
                                if let Ok(li) = self.li.lock() { draw_bars(ui, &li, "Inputs"); }
                                if let Ok(lo) = self.lo.lock() { draw_bars(ui, &lo, "Outputs"); }
                            });
                            ctx.request_repaint_after(Duration::from_millis(33));
                        }
                    }
                    fn draw_bars(ui: &mut egui::Ui, vals: &[f32], title: &str) {
                        ui.label(title);
                        let w = ui.available_width();
                        let h = 120.0;
                        let (rect, _resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                        let painter = ui.painter_at(rect);
                        let n = vals.len().max(1) as f32;
                        for (i, v) in vals.iter().enumerate() {
                            let x0 = rect.left() + (i as f32)/n * rect.width();
                            let x1 = rect.left() + ((i as f32)+1.0)/n * rect.width();
                            let y1 = rect.bottom();
                            let y0 = rect.bottom() - (v.clamp(0.0, 1.0)) * rect.height();
                            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1-1.0, y1)), 0.0, egui::Color32::LIGHT_BLUE);
                        }
                    }
                    Ok(Box::new(Viz { li, lo }))
                })
            );
        }
    } else {
        // Park the main thread; the server thread runs indefinitely
        loop { std::thread::sleep(Duration::from_secs(3600)); }
    }

    // Unreachable
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(not(all(feature = "ui", feature = "robot_io")))]
fn main() {
    println!("nn_uds_server example requires the 'ui' and 'robot_io' features.");
}
