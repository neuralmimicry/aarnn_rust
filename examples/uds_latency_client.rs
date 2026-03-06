// Unix Domain Socket (UDS) latency demo - client side.
//
// Usage:
//   cargo run --example uds_latency_client -- /tmp/aarnn_rust.rtt 1000 256
// Where args are: <socket_path> [iterations=1000] [payload_bytes=256]
// The client will send timestamped pings and print RTT statistics.

use std::env;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let socket_path = env::args().nth(1).unwrap_or_else(|| "/tmp/aarnn_rust.rtt".to_string());
    let num_iterations: usize = env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let payload_size_bytes: usize = env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(256);

    let server_path = Path::new(&socket_path);
    // Bind to a unique client path so the server can reply
    let client_socket_path = format!("/tmp/aarnn_rust.client.{}.sock", std::process::id());
    let client_path = Path::new(&client_socket_path);
    if client_path.exists() { let _ = std::fs::remove_file(client_path); }
    let client_socket = UnixDatagram::bind(client_path)?;
    client_socket.connect(server_path)?;
    println!("uds_latency_client: connected to {} (iters={}, payload={} B)", socket_path, num_iterations, payload_size_bytes);

    let mut send_buffer = vec![0u8; payload_size_bytes.max(16)];
    let mut receive_buffer = vec![0u8; send_buffer.len()];
    let mut round_trip_times = Vec::with_capacity(num_iterations);

    for _ in 0..num_iterations {
        let start_time = Instant::now();
        // Write the timestamp (nanos since start) into the first 16 bytes
        let nanos_since_start = start_time.elapsed().as_nanos() as u128; // relative to program start
        send_buffer[..16].copy_from_slice(&nanos_since_start.to_le_bytes());
        client_socket.send(&send_buffer)?;
        let _bytes_received = client_socket.recv(&mut receive_buffer)?;
        let elapsed = start_time.elapsed();
        round_trip_times.push(elapsed);
    }

    // Compute simple stats
    round_trip_times.sort_by_key(|d| d.as_nanos());
    let to_ms = |d: &Duration| d.as_nanos() as f64 / 1e6;
    let mean_ms = round_trip_times.iter().map(to_ms).sum::<f64>() / round_trip_times.len() as f64;
    let p50_ms = to_ms(&round_trip_times[round_trip_times.len() / 2]);
    let p95_ms = to_ms(&round_trip_times[((round_trip_times.len() as f64 * 0.95) as usize).min(round_trip_times.len()-1)]);
    let p99_ms = to_ms(&round_trip_times[((round_trip_times.len() as f64 * 0.99) as usize).min(round_trip_times.len()-1)]);
    println!("RTT ms: mean={:.3} p50={:.3} p95={:.3} p99={:.3}", mean_ms, p50_ms, p95_ms, p99_ms);

    Ok(())
}
