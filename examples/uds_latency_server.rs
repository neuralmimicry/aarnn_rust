// Unix Domain Socket (UDS) latency demo - server side (echo).
//
// Usage:
//   cargo run --example uds_latency_server -- /tmp/aarnn_rust.rtt
// The server will bind the socket path and echo back any datagrams it receives.

use std::env;
use std::fs;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

fn main() -> io::Result<()> {
    let socket_path = env::args().nth(1).unwrap_or_else(|| "/tmp/aarnn_rust.rtt".to_string());
    let path_ref = Path::new(&socket_path);
    // Remove stale socket if present
    if path_ref.exists() { let _ = fs::remove_file(path_ref); }
    let server_socket = UnixDatagram::bind(path_ref)?;
    println!("uds_latency_server: bound {}", socket_path);
    let mut buffer = vec![0u8; 2048];
    loop {
        let (bytes_received, peer_address) = server_socket.recv_from(&mut buffer)?;
        if bytes_received == 0 { continue; }
        if let Some(peer_path) = peer_address.as_pathname() {
            let _ = server_socket.send_to(&buffer[..bytes_received], peer_path);
        }
    }
}
