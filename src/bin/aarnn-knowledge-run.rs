//! `aarnn-knowledge-run <mesh.json> <inputs.f32> <count> <outputs.f32> [shards]`
//!
//! Executes an Evelyn knowledge region on AARNN neurons. It reads `count`
//! input vectors (little-endian f32, row-major), writes the region outputs in
//! the same format, and prints JSON statistics (cost, timing) to stdout.

use aarnn_rust::knowledge_region::{FfnRegion, read_f32_file};
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

fn run(a: &[String]) -> Result<(), String> {
    if a.len() < 5 {
        return Err(
            "usage: aarnn-knowledge-run <mesh.json> <inputs.f32> <count> <outputs.f32> [shards]"
                .into(),
        );
    }
    let count: usize = a[3].parse().map_err(|_| "invalid count")?;
    let shards: usize = a
        .get(5)
        .map_or(Ok(0), |s| s.parse())
        .map_err(|_| "invalid shards")?;
    let t0 = Instant::now();
    let region = FfnRegion::load(Path::new(&a[1])).map_err(|e| format!("load mesh: {e}"))?;
    let xs = read_f32_file(Path::new(&a[2]), count * region.inputs())
        .map_err(|e| format!("inputs: {e}"))?;
    let load_s = t0.elapsed().as_secs_f64();
    let mut out =
        std::io::BufWriter::new(std::fs::File::create(&a[4]).map_err(|e| format!("outputs: {e}"))?);
    let mut per_token = Vec::with_capacity(count);
    let mut neurons = 0u64;
    let mut neuron_steps = 0u64;
    for x in xs.chunks_exact(region.inputs()) {
        let t = Instant::now();
        let (y, stats) = region.run(x, shards).map_err(|e| format!("run: {e}"))?;
        per_token.push(t.elapsed().as_secs_f64());
        neurons = stats.neurons;
        neuron_steps += stats.neuron_steps;
        for v in y {
            out.write_all(&v.to_le_bytes())
                .map_err(|e| format!("write: {e}"))?;
        }
    }
    out.flush().map_err(|e| format!("flush: {e}"))?;
    let mean = per_token.iter().sum::<f64>() / per_token.len().max(1) as f64;
    println!(
        "{{\"load_s\":{load_s:.2},\"tokens\":{count},\"seconds_per_token\":{mean:.3},\"active_neurons_last_token\":{neurons},\"neuron_steps_total\":{neuron_steps}}}"
    );
    Ok(())
}

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().collect();
    match run(&a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aarnn-knowledge-run: {e}");
            ExitCode::from(2)
        }
    }
}
