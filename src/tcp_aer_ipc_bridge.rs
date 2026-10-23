//! Asynchronous TCP-to-Unix-datagram bridge for simulator brain endpoints.
//!
//! The simulator adapters speak the existing length-prefixed TCP/AER1
//! compatibility protocol. The distributed runtime exposes one Unix datagram
//! endpoint per brain. This module joins those two bounded transports without
//! introducing a second neural executor or using packet arrival time as
//! biological time.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixDatagram};
use tokio::sync::{Semaphore, mpsc, oneshot, watch};

pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DATAGRAM_BYTES: usize = 4 * 1024 * 1024;
pub const AER_MAGIC: &[u8; 4] = b"AER1";
const DEFAULT_MAX_CLIENTS: usize = 64;
const DEFAULT_MAX_PENDING_REQUESTS: usize = 256;

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
    pub embodiment: Option<ReferenceContract>,
}

/// Pinned local simulation contract. This identifies a compatibility session,
/// never a production writer term or committed-effect authority.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceContract {
    pub brain_id: String,
    pub brain_generation: u64,
    pub model_digest: String,
    pub io_digest: String,
    pub s_names: Vec<String>,
    pub o_names: Vec<String>,
    pub sensory_base: u32,
    pub output_base: u32,
}
impl ReferenceContract {
    fn validate(&self, config: &BridgeConfig) -> Result<(), BridgeError> {
        if !matches!(config.listen_host.as_str(), "127.0.0.1" | "::1")
            || self.brain_id.is_empty()
            || self.brain_generation == 0
            || self.s_names.len() != config.sensory
            || self.o_names.len() != config.output
            || self.s_names.is_empty()
            || self.s_names.len() > 4096
            || self.o_names.is_empty()
            || self.o_names.len() > 4096
            || self.sensory_base != 4096
            || self.output_base != config.aer_output_base
            || [&self.model_digest, &self.io_digest]
                .iter()
                .any(|d| d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(BridgeError::Protocol(
                "invalid loopback embodiment contract".into(),
            ));
        }
        for names in [&self.s_names, &self.o_names] {
            let unique: std::collections::HashSet<_> = names.iter().collect();
            if unique.len() != names.len() || names.iter().any(|n| n.is_empty() || n.len() > 256) {
                return Err(BridgeError::Protocol("invalid/duplicate IO names".into()));
            }
        }
        Ok(())
    }
    fn handshake(&self, payload: &[u8]) -> Result<(), BridgeError> {
        let value: Value =
            serde_json::from_slice(payload).map_err(|e| BridgeError::Protocol(e.to_string()))?;
        for (key, expected) in [
            ("brain_id", serde_json::json!(self.brain_id)),
            ("brain_generation", serde_json::json!(self.brain_generation)),
            ("model_digest", serde_json::json!(self.model_digest)),
            ("io_digest", serde_json::json!(self.io_digest)),
            ("s_names", serde_json::json!(self.s_names)),
            ("o_names", serde_json::json!(self.o_names)),
            ("sensory", serde_json::json!(self.s_names.len())),
            ("output", serde_json::json!(self.o_names.len())),
            ("sensory_base", serde_json::json!(self.sensory_base)),
            ("output_base", serde_json::json!(self.output_base)),
            ("dt_ms", serde_json::json!(2.0)),
        ] {
            if value.get(key) != Some(&expected) {
                return Err(BridgeError::Protocol(format!(
                    "embodiment handshake mismatch: {key}"
                )));
            }
        }
        Ok(())
    }
    fn response(&self, response: &[u8]) -> Result<Vec<u8>, BridgeError> {
        if response_dimensions(response, 0, 0)? != (self.s_names.len(), self.o_names.len()) {
            return Err(BridgeError::Protocol(
                "IPC dimensions differ from pinned embodiment; resize rejected".into(),
            ));
        }
        let mut value: Value =
            serde_json::from_slice(response).map_err(|e| BridgeError::Protocol(e.to_string()))?;
        for (key, v) in [
            ("brain_id", serde_json::json!(self.brain_id)),
            ("brain_generation", serde_json::json!(self.brain_generation)),
            ("model_digest", serde_json::json!(self.model_digest)),
            ("io_digest", serde_json::json!(self.io_digest)),
            ("expected_s", serde_json::json!(self.s_names.len())),
            ("expected_o", serde_json::json!(self.o_names.len())),
        ] {
            value[key] = v;
        }
        serde_json::to_vec(&value).map_err(|e| BridgeError::Protocol(e.to_string()))
    }
    fn frame(&self, payload: &[u8]) -> Result<(), BridgeError> {
        let events = aarnn_aer_legacy::decode_events_bounded(
            payload,
            self.sensory_base,
            self.s_names.len() as u32,
            self.s_names.len(),
        )
        .map_err(|e| BridgeError::Protocol(format!("embodiment AER1 input: {e}")))?;
        if events.iter().any(|e| e.value != 1) {
            return Err(BridgeError::Protocol("embodiment input polarity".into()));
        }
        Ok(())
    }
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
    if let Some(contract) = &config.embodiment {
        contract.validate(&config)?;
    }
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

    // The Unix IPC endpoint has one reply owner (`last_peer`) and therefore
    // cannot safely service multiple request/reply exchanges concurrently.
    // Keep TCP sessions concurrent and put only the bounded biological
    // exchanges through this FIFO arbiter. Waiting for queue admission is
    // asynchronous; it never blocks the listener or another client task.
    let queue_capacity = std::env::var("NM_TCP_AER_BRIDGE_QUEUE_CAPACITY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_PENDING_REQUESTS)
        .clamp(1, 4096);
    let max_clients = if config.embodiment.is_some() {
        1
    } else {
        std::env::var("NM_TCP_AER_BRIDGE_MAX_CLIENTS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_CLIENTS)
            .clamp(1, 256)
    };
    println!(
        "[tcp_aer_ipc_bridge] bounded client capacity={} pending IPC requests={}",
        max_clients, queue_capacity
    );
    let (ipc_tx, ipc_rx) = mpsc::channel(queue_capacity);
    let arbiter_shutdown = shutdown.clone();
    let arbiter = tokio::spawn(run_ipc_arbiter(
        ipc_rx,
        config.ipc_path.clone(),
        config.timeout,
        arbiter_shutdown,
    ));
    let client_limit = Arc::new(Semaphore::new(max_clients));
    let mut clients = tokio::task::JoinSet::new();

    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let permit = match client_limit.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        eprintln!("[tcp_aer_ipc_bridge] rejecting client {peer}: active client limit reached");
                        continue;
                    }
                };
                println!("[tcp_aer_ipc_bridge] client connected: {peer}");
                let client_config = config.clone();
                let client_shutdown = shutdown.clone();
                let client_ipc_tx = ipc_tx.clone();
                clients.spawn(async move {
                    let _permit = permit;
                    if let Err(error) = handle_client(
                        stream,
                        &client_config,
                        client_ipc_tx,
                        client_shutdown,
                    ).await {
                        if !matches!(error, BridgeError::Shutdown) {
                            eprintln!("[tcp_aer_ipc_bridge] client stopped: {error}");
                        }
                    }
                });
            }
            Some(result) = clients.join_next() => {
                if let Err(error) = result {
                    eprintln!("[tcp_aer_ipc_bridge] client task failed: {error}");
                }
            }
            _ = wait_for_shutdown(&mut shutdown) => break,
        }
    }

    clients.abort_all();
    while clients.join_next().await.is_some() {}
    drop(ipc_tx);
    let _ = arbiter.await;
    Ok(())
}

struct IpcRequest {
    socket: Arc<UnixDatagram>,
    payload: Vec<u8>,
    reply: oneshot::Sender<Result<Vec<u8>, BridgeError>>,
}

async fn handle_client(
    mut stream: TcpStream,
    config: &BridgeConfig,
    ipc_tx: mpsc::Sender<IpcRequest>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    stream.set_nodelay(true)?;
    let (local_dir, local_path) = create_ipc_peer_path().await?;
    let ipc = UnixDatagram::bind(&local_path)?;
    let result = handle_client_io(&mut stream, Arc::new(ipc), config, ipc_tx, &mut shutdown).await;
    let _ = fs::remove_file(&local_path).await;
    let _ = fs::remove_dir(&local_dir).await;
    result
}

async fn handle_client_io(
    stream: &mut TcpStream,
    ipc: Arc<UnixDatagram>,
    config: &BridgeConfig,
    ipc_tx: mpsc::Sender<IpcRequest>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), BridgeError> {
    let (initial_sensory, initial_output) = config.bounded_dimensions();
    let mut negotiated = Negotiated {
        sensory: initial_sensory,
        output: initial_output,
        dt_ms: 1.0,
    };
    let mut handshaken = false;

    loop {
        let payload = read_frame(stream, config.timeout, shutdown).await?;
        let is_json = payload.first() == Some(&b'{');
        if let Some(contract) = &config.embodiment {
            if is_json {
                if handshaken {
                    return Err(BridgeError::Protocol("duplicate handshake".into()));
                }
                contract.handshake(&payload)?;
            } else {
                if !handshaken {
                    return Err(BridgeError::Protocol("handshake required".into()));
                }
                contract.frame(&payload)?;
            }
        }
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

        let response = exchange_ipc(&ipc, &ipc_tx, &payload, config.timeout, shutdown).await?;

        if response.first() == Some(&b'{') {
            let response = if let Some(contract) = &config.embodiment {
                contract.response(&response)?
            } else {
                response
            };
            let (sensory, output) =
                response_dimensions(&response, negotiated.sensory, negotiated.output)?;
            negotiated.sensory = sensory;
            negotiated.output = output;
            if is_json {
                handshaken = true;
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

async fn run_ipc_arbiter(
    mut requests: mpsc::Receiver<IpcRequest>,
    destination: PathBuf,
    timeout: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        let request = tokio::select! {
            request = requests.recv() => match request {
                Some(request) => request,
                None => return,
            },
            _ = wait_for_shutdown(&mut shutdown) => return,
        };
        let IpcRequest {
            socket,
            payload,
            reply,
        } = request;
        let result =
            exchange_ipc_serialized(&socket, &destination, &payload, timeout, &mut shutdown).await;
        let _ = reply.send(result);
        if *shutdown.borrow() {
            return;
        }
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
    ipc: &Arc<UnixDatagram>,
    queue: &mpsc::Sender<IpcRequest>,
    payload: &[u8],
    timeout: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Vec<u8>, BridgeError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    let request = IpcRequest {
        socket: Arc::clone(ipc),
        payload: payload.to_vec(),
        reply: reply_tx,
    };
    let enqueue = async { queue.send(request).await.map_err(|_| BridgeError::Shutdown) };
    select_with_shutdown(enqueue, timeout, shutdown).await?;
    let response = async { reply_rx.await.map_err(|_| BridgeError::Shutdown) };
    select_with_shutdown(response, timeout, shutdown).await?
}

async fn exchange_ipc_serialized(
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
    fn pinned_embodiment_rejects_aliases_resizes_remote_hosts_and_wrong_aer_window() {
        let contract = ReferenceContract {
            brain_id: "worm_0".into(),
            brain_generation: 1,
            model_digest: "a".repeat(64),
            io_digest: "b".repeat(64),
            s_names: vec!["touch".into()],
            o_names: vec!["MDL01".into()],
            sensory_base: 4096,
            output_base: 16384,
        };
        let mut config = BridgeConfig {
            listen_host: "127.0.0.1".into(),
            listen_port: 1234,
            ipc_path: "/tmp/test.sock".into(),
            sensory: 1,
            output: 1,
            aer_output_base: 16384,
            aer_threshold: 0.5,
            timeout: Duration::from_secs(2),
            ready_file: None,
            arm_file: None,
            embodiment: Some(contract.clone()),
        };
        contract.validate(&config).unwrap();
        config.listen_host = "0.0.0.0".into();
        assert!(contract.validate(&config).is_err());
        let mut handshake = serde_json::json!({"brain_id":"worm_0","brain_generation":1,"model_digest":"a".repeat(64),"io_digest":"b".repeat(64),"s_names":["touch"],"o_names":["MDL01"],"sensory":1,"output":1,"sensory_base":4096,"output_base":16384,"dt_ms":2.0});
        contract
            .handshake(handshake.to_string().as_bytes())
            .unwrap();
        for (key, value) in [
            ("brain_id", serde_json::json!("other")),
            ("s_names", serde_json::json!(["alias"])),
            ("sensory", serde_json::json!(2)),
            ("dt_ms", serde_json::json!(32.)),
            ("model_digest", serde_json::json!("c".repeat(64))),
        ] {
            let saved = handshake[key].clone();
            handshake[key] = value;
            assert!(
                contract
                    .handshake(handshake.to_string().as_bytes())
                    .is_err(),
                "{key}"
            );
            handshake[key] = saved;
        }
        assert!(
            contract
                .response(br#"{"expected_s":2,"expected_o":1}"#)
                .is_err()
        );
        let response: Value = serde_json::from_slice(
            &contract
                .response(br#"{"expected_s":1,"expected_o":1}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(response["brain_id"], "worm_0");
        let packet = aarnn_aer_legacy::encode_spikes(42_000, 4096, &[1]);
        contract.frame(&packet).unwrap();
        assert_eq!(aer_timestamp(&packet), 42_000);
        assert!(
            contract
                .frame(&aarnn_aer_legacy::encode_spikes(42_000, 0, &[1]))
                .is_err()
        );
        assert!(contract.frame(&packet[..12 + 1]).is_err());
        // AER1 is a compatibility protocol, not AARNN-AER/1 or FPAA UDP.
        assert!(contract.frame(b"AARNN-AER/1").is_err());
    }

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

    #[tokio::test]
    async fn concurrent_clients_are_queued_and_keep_their_ipc_reply_owner() {
        let root = std::env::temp_dir().join(format!(
            "aarnn-bridge-test-{}-{}",
            std::process::id(),
            SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).await.expect("create test directory");
        let destination_path = root.join("server.sock");
        let client_one_path = root.join("client-one.sock");
        let client_two_path = root.join("client-two.sock");
        let destination = UnixDatagram::bind(&destination_path).expect("bind destination");
        let client_one = Arc::new(UnixDatagram::bind(&client_one_path).expect("bind client one"));
        let client_two = Arc::new(UnixDatagram::bind(&client_two_path).expect("bind client two"));
        let (queue, requests) = mpsc::channel(1);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let arbiter = tokio::spawn(run_ipc_arbiter(
            requests,
            destination_path.clone(),
            Duration::from_secs(2),
            shutdown_rx,
        ));

        let responder = tokio::spawn(async move {
            for _ in 0..2 {
                let mut buffer = [0u8; 32];
                let (length, peer) = destination
                    .recv_from(&mut buffer)
                    .await
                    .expect("receive request");
                let peer_path = peer.as_pathname().expect("path based client");
                let mut response = b"reply:".to_vec();
                response.extend_from_slice(&buffer[..length]);
                destination
                    .send_to(&response, peer_path)
                    .await
                    .expect("send response");
            }
        });

        let (_first_shutdown_tx, first_shutdown_rx) = watch::channel(false);
        let (_second_shutdown_tx, second_shutdown_rx) = watch::channel(false);
        let mut first_shutdown = first_shutdown_rx;
        let mut second_shutdown = second_shutdown_rx;
        let first = exchange_ipc(
            &client_one,
            &queue,
            b"one",
            Duration::from_secs(2),
            &mut first_shutdown,
        );
        let second = exchange_ipc(
            &client_two,
            &queue,
            b"two",
            Duration::from_secs(2),
            &mut second_shutdown,
        );
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.expect("first response"), b"reply:one");
        assert_eq!(second.expect("second response"), b"reply:two");

        responder.await.expect("responder task");
        shutdown_tx.send(true).expect("shutdown arbiter");
        arbiter.await.expect("arbiter task");
        let _ = fs::remove_dir_all(root).await;
    }
}
