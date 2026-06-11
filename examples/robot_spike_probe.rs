//! Headless snapshot probe to verify output spike activity for robot networks.
//!
//! Usage:
//!   cargo run --example robot_spike_probe -- \
//!     --network network_celegans.json --steps 12 --dt-ms 1.0

use std::path::PathBuf;

use aarnn_rust::config::{LIFParams, NetworkConfig, STDPParams};
use aarnn_rust::runner::Runner;
use aarnn_rust::sim::{Learning, NeuronModel};

#[derive(Clone)]
struct ProbeArgs {
    network_path: PathBuf,
    steps: usize,
    dt_ms: f64,
}

fn parse_args() -> Result<ProbeArgs, String> {
    let mut network_path: Option<PathBuf> = None;
    let mut steps: usize = 12;
    let mut dt_ms: f64 = 1.0;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--network" => {
                let value = it
                    .next()
                    .ok_or_else(|| "--network requires a path".to_string())?;
                network_path = Some(PathBuf::from(value));
            }
            "--steps" => {
                let value = it
                    .next()
                    .ok_or_else(|| "--steps requires a positive integer".to_string())?;
                steps = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --steps value: {value}"))?;
                if steps == 0 {
                    return Err("--steps must be > 0".to_string());
                }
            }
            "--dt-ms" => {
                let value = it
                    .next()
                    .ok_or_else(|| "--dt-ms requires a positive number".to_string())?;
                dt_ms = value
                    .parse::<f64>()
                    .map_err(|_| format!("invalid --dt-ms value: {value}"))?;
                if !dt_ms.is_finite() || dt_ms <= 0.0 {
                    return Err("--dt-ms must be finite and > 0".to_string());
                }
            }
            "--help" | "-h" => {
                return Err(
                    "Usage: --network <snapshot.json> [--steps <n>] [--dt-ms <ms>]".to_string(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let network_path = network_path.ok_or_else(|| "--network is required".to_string())?;
    Ok(ProbeArgs {
        network_path,
        steps,
        dt_ms,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let snapshot_raw = match std::fs::read_to_string(&args.network_path) {
        Ok(v) => v,
        Err(err) => {
            eprintln!(
                "failed to read snapshot {}: {}",
                args.network_path.display(),
                err
            );
            std::process::exit(2);
        }
    };

    let mut runner = Runner::new(
        LIFParams::default(),
        STDPParams::default(),
        NetworkConfig::default(),
        NeuronModel::Lif,
        Learning::Stdp,
    );

    if let Err(err) = runner.import_network_json(&snapshot_raw) {
        eprintln!(
            "failed to import snapshot {}: {}",
            args.network_path.display(),
            err
        );
        std::process::exit(2);
    }

    let sensory_count = runner.net.num_sensory_neurons.max(1);
    let output_count = runner.net.num_output_neurons.max(1);
    let mut max_active = 0usize;
    let drive = vec![1i8; sensory_count];

    println!(
        "loaded={} S={} O={} steps={} dt_ms={}",
        args.network_path.display(),
        sensory_count,
        output_count,
        args.steps,
        args.dt_ms
    );

    for step_idx in 0..args.steps {
        runner.set_dt(args.dt_ms);
        let _ = runner.step(Some(&drive));
        let active = runner.last_spk_o.iter().filter(|&&v| v != 0).count();
        if active > max_active {
            max_active = active;
        }
        println!("step={} active_output_spikes={}", step_idx + 1, active);
        if max_active > 0 {
            break;
        }
    }

    println!("max_active_output_spikes={max_active}");
    if max_active == 0 {
        std::process::exit(1);
    }
}
