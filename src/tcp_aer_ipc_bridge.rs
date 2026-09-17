//! Asynchronous TCP-to-Unix-datagram bridge for simulator brain endpoints.
//!
//! The simulator adapters speak the existing length-prefixed TCP/AER1
//! compatibility protocol. The distributed runtime exposes one Unix datagram
//! endpoint per brain. This module joins those two bounded transports without
//! introducing a second neural executor or using packet arrival time as
//! biological time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixDatagram};
use tokio::sync::watch;

pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DATAGRAM_BYTES: usize = 4 * 1024 * 1024;
pub const AER_MAGIC: &[u8; 4] = b"AER1";

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("bridge operation timed out")]
    Timeout,
    #[error("bridge shutdown requested")]
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct BridgeConfig {
    pub listen_host: String,
    pub listen_port: u16,
    pub ipc_path: PathBuf,
    pub sensory: usize,
    pub output: usize,
    pub aer_output_base: u32,
    pub aer_threshold: f32,
    pub timeout: Duration,
    pub ready_file: Option<PathBuf>,
    pub arm_file: Option<PathBuf>,
}

impl BridgeConfig {
    pub fn bounded_dimensions(&self) -> (usize, usize) {
        (self.sensory.max(1), self.output.max(1))
    }
}

#[derive(Clone, Copy, Debug)]
struct Negotiated {
    sensory: usize,
    output: usize,
    dt_ms: f64,
}

static SOCKET_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Run the bridge until the supplied shutdown signal is set.
pub async fn run(
    config: BridgeConfig,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    let bind_address = if config.listen_host.contains(':') {
        format!("[{}]:{}", config.listen_host, config.listen_port)
    } else {
        format!("{}:{}", config.listen_host, config.listen_port)
    };
    let listener = TcpListener::bind(&bind_address).await?;
    println!(
        "[tcp_aer_ipc_bridge] listening on {}:{} -> {}",
        config.listen_host,
        config.listen_port,
        config.ipc_path.display()
    );

    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                println!("[tcp_aer_ipc_bridge] client connected: {peer}");
                if let Err(error) = handle_client(stream, &config, shutdown.clone()).await {
                    if !matches!(error, BridgeError::Shutdown) {
                        eprintln!("[tcp_aer_ipc_bridge] client stopped: {error}");
                    }
                }
            }
            _ = wait_for_shutdown(&mut shutdown) => return Ok(()),
        }
    }
}

async fn handle_client(
    mut stream: TcpStream,
    config: &BridgeConfig,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    stream.set_nodelay(true)?;
    let (local_dir, local_path) = create_ipc_peer_path().await?;
    let ipc = UnixDatagram::bind(&local_path)?;
    let result = handle_client_io(&mut stream, &ipc, config, &mut shutdown).await;
    drop(ipc);
    let _ = fs::remove_file(&local_path).await;
    let _ = fs::remove_dir(&local_dir).await;
    result
}

async fn handle_client_io(
    stream: &mut TcpStream,
    ipc: &UnixDatagram,
    config: &BridgeConfig,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    let (initial_sensory, initial_output) = config.bounded_dimensions();
    let mut negotiated = Negotiated {
        sensory: initial_sensory,
        output: initial_output,
        dt_ms: 1.0,
    };

    loop {
        let payload = read_frame(stream, config.timeout, shutdown).await?;
        let is_json = payload.first() == Some(&b'{');
        if is_json {
            let (sensory, output, dt_ms) =
                handshake_dimensions(&payload, negotiated.sensory, negotiated.output);
            negotiated = Negotiated {
                sensory,
                output,
                dt_ms,
            };
        }
        if payload.len() > MAX_DATAGRAM_BYTES {
            return Err(BridgeError::Protocol(format!(
                "payload exceeds IPC datagram bound: {}",
                payload.len()
            )));
        }
        if !is_json {
            wait_until_armed(config.arm_file.as_deref(), shutdown).await?;
        }

        let response =
            exchange_ipc(ipc, &config.ipc_path, &payload, config.timeout, shutdown).await?;

        if response.first() == Some(&b'{') {
            let (sensory, output) =
                response_dimensions(&response, negotiated.sensory, negotiated.output)?;
            negotiated.sensory = sensory;
            negotiated.output = output;
            if is_json {
                mark_ready(config.ready_file.as_deref()).await?;
            }
            write_frame(stream, &response, config.timeout, shutdown).await?;
            continue;
        }

        let values = output_values(&response, negotiated.output)?;
        let response = if payload.starts_with(AER_MAGIC) {
            let timestamp = aer_timestamp(&payload)
                .checked_add((negotiated.dt_ms * 1000.0).round().max(1.0) as u64)
                .ok_or_else(|| BridgeError::Protocol("AER timestamp overflow".into()))?;
            aer_frame(
                timestamp,
                config.aer_output_base,
                &values,
                config.aer_threshold,
            )?
        } else {
            response
        };
        write_frame(stream, &response, config.timeout, shutdown).await?;
    }
}

async fn create_ipc_peer_path() -> Result<(PathBuf, PathBuf), BridgeError> {
    let root = std::env::temp_dir().join(format!(
        "aarnn-tcp-bridge-{}-{}",
        std::process::id(),
        SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).await?;
    let path = root.join("peer.sock");
    Ok((root, path))
}

async fn read_frame(
    stream: &mut TcpStream,
    timeout: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Vec<u8>, BridgeError> {
    let operation = async {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;
        let size = u32::from_le_bytes(header) as usize;
        if size == 0 || size > MAX_FRAME_BYTES {
            return Err(BridgeError::Protocol(format!(
                "invalid TCP frame length {size}"
            )));
        }
        let mut payload = vec![0u8; size];
        stream.read_exact(&mut payload).await?;
        Ok(payload)
    };
    select_with_shutdown(operation, timeout, shutdown).await
}

async fn write_frame(
    stream: &mut TcpStream,
    payload: &[u8],
    timeout: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(BridgeError::Protocol(format!(
            "invalid TCP response length {}",
            payload.len()
        )));
    }
    let operation = async {
        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .await
            .map_err(BridgeError::Io)?;
        stream.write_all(payload).await.map_err(BridgeError::Io)
    };
    select_with_shutdown(operation, timeout, shutdown).await
}

async fn exchange_ipc(
    ipc: &UnixDatagram,
    destination: &Path,
    payload: &[u8],
    timeout: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Vec<u8>, BridgeError> {
    let operation = async {
        ipc.send_to(payload, destination).await?;
        let mut response = vec![0u8; MAX_DATAGRAM_BYTES + 1];
        let (length, _) = ipc.recv_from(&mut response).await?;
        if length > MAX_DATAGRAM_BYTES {
            return Err(BridgeError::Protocol(format!(
                "IPC response exceeds datagram bound: {length}"
            )));
        }
        response.truncate(length);
        if response.is_empty() {
            return Err(BridgeError::Protocol("empty IPC response".into()));
        }
        Ok(response)
    };
    select_with_shutdown(operation, timeout, shutdown).await
}

async fn select_with_shutdown<F, T>(
    operation: F,
    timeout: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<T, BridgeError>
where
    F: std::future::Future<Output = Result<T, BridgeError>>,
{
    if *shutdown.borrow() {
        return Err(BridgeError::Shutdown);
    }
    tokio::select! {
        result = tokio::time::timeout(timeout, operation) => result.map_err(|_| BridgeError::Timeout)?,
        _ = wait_for_shutdown(shutdown) => Err(BridgeError::Shutdown),
    }
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    if !*shutdown.borrow() {
        let _ = shutdown.changed().await;
    }
}

async fn wait_until_armed(
    arm_file: Option<&Path>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    let Some(arm_file) = arm_file else {
        return Ok(());
    };
    loop {
        if fs::metadata(arm_file).await.is_ok() {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
            _ = wait_for_shutdown(shutdown) => return Err(BridgeError::Shutdown),
        }
    }
}

async fn mark_ready(path: Option<&Path>) -> Result<(), BridgeError> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_file_name(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ready"),
        std::process::id(),
        SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, b"ready\n").await?;
    fs::rename(temporary, path).await?;
    Ok(())
}

fn handshake_dimensions(payload: &[u8], sensory: usize, output: usize) -> (usize, usize, f64) {
    let Ok(document) = serde_json::from_slice::<Value>(payload) else {
        return (sensory, output, 1.0);
    };
    let sensory = json_dimension(&document, "s_names", "sensory", "expected_s")
        .unwrap_or(sensory)
        .max(1);
    let output = json_dimension(&document, "o_names", "output", "expected_o")
        .unwrap_or(output)
        .max(1);
    let dt_ms = document
        .get("dt_ms")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 1000.0)
        .unwrap_or(1.0);
    (sensory, output, dt_ms)
}

fn json_dimension(document: &Value, names: &str, primary: &str, fallback: &str) -> Option<usize> {
    if let Some(length) = document.get(names).and_then(Value::as_array).map(Vec::len) {
        if length > 0 {
            return Some(length);
        }
    }
    document
        .get(primary)
        .or_else(|| document.get(fallback))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn response_dimensions(
    response: &[u8],
    sensory: usize,
    output: usize,
) -> Result<(usize, usize), BridgeError> {
    let document: Value = serde_json::from_slice(response)
        .map_err(|error| BridgeError::Protocol(format!("invalid IPC size hint: {error}")))?;
    let sensory = json_dimension(&document, "s_names", "expected_s", "sensory")
        .unwrap_or(sensory)
        .max(1);
    let output = json_dimension(&document, "o_names", "expected_o", "output")
        .unwrap_or(output)
        .max(1);
    Ok((sensory, output))
}

fn output_values(response: &[u8], output: usize) -> Result<Vec<f32>, BridgeError> {
    let expected = output
        .checked_mul(4)
        .ok_or_else(|| BridgeError::Protocol("output dimension overflow".into()))?;
    if response.len() != expected {
        return Err(BridgeError::Protocol(format!(
            "IPC output length {} does not match O={output}",
            response.len()
        )));
    }
    Ok(response
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("chunks_exact")))
        .collect())
}

pub fn aer_frame(
    timestamp_us: u64,
    output_base: u32,
    values: &[f32],
    threshold: f32,
) -> Result<Vec<u8>, BridgeError> {
    let mut result = Vec::with_capacity(12 + values.len() * 6);
    result.extend_from_slice(AER_MAGIC);
    result.extend_from_slice(&timestamp_us.to_le_bytes());
    for (index, value) in values.iter().enumerate() {
        if *value > threshold {
            let address = output_base
                .checked_add(index as u32)
                .ok_or_else(|| BridgeError::Protocol("AER output address overflow".into()))?;
            put_varint(0, &mut result);
            put_varint(address as u64, &mut result);
            put_varint(1, &mut result);
        }
    }
    Ok(result)
}

fn put_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn aer_timestamp(payload: &[u8]) -> u64 {
    if payload.len() >= 12 && payload.starts_with(AER_MAGIC) {
        let mut timestamp = [0u8; 8];
        timestamp.copy_from_slice(&payload[4..12]);
        u64::from_le_bytes(timestamp)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_bounds_and_handshake_dimensions_are_bounded() {
        let payload = br#"{"s_names":["a","b"],"o_names":["x"],"dt_ms":2.5}"#;
        assert_eq!(handshake_dimensions(payload, 24, 96), (2, 1, 2.5));
        assert_eq!(handshake_dimensions(b"not-json", 24, 96), (24, 96, 1.0));
    }

    #[test]
    fn aer_output_uses_logical_timestamp_and_strict_threshold() {
        let frame = aer_frame(7_000, 16_384, &[0.5, 0.5001, 0.1], 0.5).unwrap();
        assert_eq!(&frame[..12], b"AER1\x58\x1b\x00\x00\x00\x00\x00\x00");
        assert_eq!(&frame[12..], &[0, 0x81, 0x80, 1, 1]);
    }

    #[test]
    fn raw_output_requires_exact_negotiated_dimension() {
        assert!(output_values(&[0; 7], 2).is_err());
        assert_eq!(output_values(&[0, 0, 0, 0], 1).unwrap(), vec![0.0]);
    }
}
