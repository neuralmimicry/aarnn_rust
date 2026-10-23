//! Bounded, asynchronous, loopback-only AER1 reference client. AARNN-AER/1
//! authorisation/commit and FPAA UDP are deliberately different protocols.
use aarnn_aer_legacy::{decode_events_bounded, encode_spikes};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Sensor {
    pub name: String,
    pub kind: String,
    pub range: [f64; 2],
    pub joint: Option<String>,
    pub angle_rad: Option<f64>,
    pub axis: Option<usize>,
    pub body: Option<String>,
    pub position_m: Option<[f64; 3]>,
    pub camera: Option<String>,
    pub cell: Option<[usize; 2]>,
    pub retina_shape: Option<[usize; 2]>,
    pub direction: Option<[f64; 3]>,
    pub lookup: Option<Vec<[f64; 2]>>,
    pub mode: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Io {
    pub digest: String,
    pub sensors: Vec<Sensor>,
    pub output_names: Vec<String>,
    pub sensory_base: u32,
    pub output_base: u32,
    pub dt_us: u64,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Actuator {
    pub name: String,
    pub joint: String,
    pub control_range: [f64; 2],
    #[serde(default)]
    pub readout: Vec<WeightedChannel>,
    #[serde(default)]
    pub position_readout: bool,
    pub velocity_limit: Option<f64>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct WeightedChannel {
    pub channel: usize,
    pub coefficient: f64,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub profile_id: String,
    pub digest: String,
    pub native_mjcf_sha256: String,
    pub io: Io,
    pub actuators: Vec<Actuator>,
    pub controller_transfer: Option<String>,
    pub scene: Option<Scene>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    pub half_extent_m: f64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Binding {
    pub brain_id: String,
    pub brain_generation: u64,
    pub model_digest: String,
    pub io_digest: String,
    pub host: String,
    pub port: u16,
}
impl Binding {
    pub fn validate(&self, model: &Model) -> Result<(), String> {
        if self.brain_id.is_empty()
            || self.brain_generation == 0
            || self.model_digest != model.digest
            || self.io_digest != model.io.digest
        {
            return Err("brain/model/IO binding mismatch".into());
        }
        if model
            .controller_transfer
            .as_deref()
            .is_some_and(|s| s.starts_with("blocked"))
        {
            return Err("legacy projection transfer requires calibration".into());
        }
        if self.host != "127.0.0.1" && self.host != "::1" {
            return Err("AER1 reference endpoints must be loopback literals".into());
        }
        if model.io.sensors.len() > 4096
            || model.io.output_names.len() > 4096
            || model.io.dt_us != 2000
        {
            return Err("unsupported IO/clock contract".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Capture {
    pub step: u64,
    pub timestamp_us: u64,
    pub values: Vec<f64>,
}
#[derive(Debug)]
pub struct Reply {
    pub source_step: u64,
    pub timestamp_us: u64,
    pub spikes: Vec<u32>,
}
#[derive(Debug, Default, Serialize, Clone)]
pub struct Status {
    pub ready: bool,
    pub exchanges: u64,
    pub output_spikes: u64,
    pub pre_admission_gaps: u64,
    pub fault: Option<String>,
    pub latency_us: Vec<u64>,
}
pub struct Client {
    input: SyncSender<Capture>,
    output: Option<Receiver<Reply>>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<Status>>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    worker: Option<thread::JoinHandle<()>>,
    _binding: BindingLease,
}
// Process-local uniqueness complements the strict server's producer lease.
// Separate processes are still checked by the server, never by this cache.
static BINDINGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
struct BindingLease([String; 2]);
impl BindingLease {
    fn reserve(binding: &Binding) -> Result<Self, String> {
        let keys = [
            format!("brain:{}", binding.brain_id),
            format!("endpoint:{}:{}", binding.host, binding.port),
        ];
        let mut used = BINDINGS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .map_err(|_| "binding registry unavailable")?;
        if keys.iter().any(|k| used.contains(k)) {
            return Err("brain or reference endpoint already bound in this process".into());
        }
        used.extend(keys.iter().cloned());
        Ok(Self(keys))
    }
}
impl Drop for BindingLease {
    fn drop(&mut self) {
        if let Ok(mut used) = BINDINGS.get_or_init(|| Mutex::new(HashSet::new())).lock() {
            for k in &self.0 {
                used.remove(k);
            }
        }
    }
}
fn exchange(stream: &mut TcpStream, payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.is_empty() || payload.len() > 4 * 1024 * 1024 {
        return Err("frame budget".into());
    }
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|_| stream.write_all(payload))
        .map_err(|e| e.to_string())?;
    let mut length = [0; 4];
    stream.read_exact(&mut length).map_err(|e| e.to_string())?;
    let count = u32::from_le_bytes(length) as usize;
    if !(1..=4 * 1024 * 1024).contains(&count) {
        return Err("response budget".into());
    }
    let mut response = vec![0; count];
    stream
        .read_exact(&mut response)
        .map_err(|e| e.to_string())?;
    Ok(response)
}
impl Client {
    pub fn start(model: Model, binding: Binding) -> Result<Self, String> {
        binding.validate(&model)?;
        let startup_ms = match std::env::var("NM_AER_REFERENCE_STARTUP_TIMEOUT_MS") {
            Ok(v) => v
                .parse::<u64>()
                .ok()
                .filter(|n| (1000..=3_600_000).contains(n))
                .ok_or("reference startup timeout must be 1000..3600000 ms")?,
            Err(_) => 5000,
        };
        let lease = BindingLease::reserve(&binding)?;
        let (input, rx) = mpsc::sync_channel::<Capture>(2);
        let (tx, output) = mpsc::sync_channel(2);
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(Status::default()));
        let socket = Arc::new(Mutex::new(None));
        let (cancel, progress, handle) = (stop.clone(), status.clone(), socket.clone());
        let worker = thread::spawn(move || {
            let run = || -> Result<(), String> {
                let address: SocketAddr = format!(
                    "{}:{}",
                    if binding.host == "::1" {
                        "[::1]"
                    } else {
                        &binding.host
                    },
                    binding.port
                )
                .parse()
                .map_err(|_| "invalid endpoint")?;
                let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_millis(startup_ms)))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                stream.set_nodelay(true).map_err(|e| e.to_string())?;
                *handle.lock().unwrap() = Some(stream.try_clone().map_err(|e| e.to_string())?);
                let handshake = serde_json::json!({"brain_id":binding.brain_id,"brain_generation":binding.brain_generation,"model_digest":binding.model_digest,"io_digest":binding.io_digest,"sensory_base":model.io.sensory_base,"output_base":model.io.output_base,"s_names":model.io.sensors.iter().map(|s|&s.name).collect::<Vec<_>>(),"o_names":model.io.output_names,"sensory":model.io.sensors.len(),"output":model.io.output_names.len(),"dt_ms":2.});
                let reply: serde_json::Value = serde_json::from_slice(&exchange(
                    &mut stream,
                    handshake.to_string().as_bytes(),
                )?)
                .map_err(|e| e.to_string())?;
                for (key, value) in [
                    ("brain_id", serde_json::json!(binding.brain_id)),
                    (
                        "brain_generation",
                        serde_json::json!(binding.brain_generation),
                    ),
                    ("model_digest", serde_json::json!(binding.model_digest)),
                    ("io_digest", serde_json::json!(binding.io_digest)),
                    ("expected_s", serde_json::json!(model.io.sensors.len())),
                    ("expected_o", serde_json::json!(model.io.output_names.len())),
                ] {
                    if reply.get(key) != Some(&value) {
                        return Err(format!("strict reference handshake mismatch: {key}"));
                    }
                }
                // Managed layout/name negotiation may take longer than one
                // steady-state exchange. Restore the unchanged 5s data budget.
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                progress.lock().unwrap().ready = true;
                let mut phase = vec![0.; model.io.sensors.len()];
                while !cancel.load(Ordering::Relaxed) {
                    let capture = match rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(c) => c,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    if capture.values.len() != phase.len() {
                        return Err("capture shape mismatch".into());
                    }
                    let mut spikes = vec![0; phase.len()];
                    for (i, value) in capture.values.into_iter().enumerate() {
                        if !value.is_finite() || !(0. ..=1.).contains(&value) {
                            return Err("invalid normalised sensor".into());
                        }
                        phase[i] += value;
                        if phase[i] >= 1. {
                            spikes[i] = 1;
                            phase[i] -= 1.;
                        }
                    }
                    let mut packet =
                        encode_spikes(capture.timestamp_us, model.io.sensory_base, &spikes);
                    if packet.is_empty() {
                        packet.extend_from_slice(b"AER1");
                        packet.extend_from_slice(&capture.timestamp_us.to_le_bytes());
                    }
                    let sent = Instant::now();
                    let response = exchange(&mut stream, &packet)?;
                    let latency_us = sent.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
                    let events = decode_events_bounded(
                        &response,
                        model.io.output_base,
                        model.io.output_names.len() as u32,
                        65536,
                    )
                    .map_err(str::to_string)?;
                    let timestamp_us = u64::from_le_bytes(response[4..12].try_into().unwrap());
                    let mut spikes = vec![0u32; model.io.output_names.len()];
                    for event in events {
                        spikes[(event.addr - model.io.output_base) as usize] +=
                            u32::from(event.value);
                    }
                    let spike_count: u64 = spikes.iter().map(|v| u64::from(*v)).sum();
                    tx.send(Reply {
                        source_step: capture.step,
                        timestamp_us,
                        spikes,
                    })
                    .map_err(|_| "output consumer closed")?;
                    let mut status = progress.lock().unwrap();
                    status.exchanges += 1;
                    status.output_spikes += spike_count;
                    if status.latency_us.len() == 256 {
                        status.latency_us.remove(0);
                    }
                    status.latency_us.push(latency_us);
                }
                Ok(())
            };
            if let Err(error) = run()
                && !cancel.load(Ordering::Relaxed)
            {
                progress.lock().unwrap().fault = Some(error);
            }
        });
        Ok(Self {
            input,
            output: Some(output),
            stop,
            status,
            socket,
            worker: Some(worker),
            _binding: lease,
        })
    }
    pub fn submit(&self, capture: Capture) -> bool {
        match self.input.try_send(capture) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.status.lock().unwrap().pre_admission_gaps += 1;
                false
            }
            Err(_) => false,
        }
    }
    pub fn receive(&self) -> Option<Reply> {
        self.output.as_ref()?.try_recv().ok()
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.output.take();
        if let Some(stream) = self.socket.lock().unwrap().as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_lease_is_atomic_and_released_without_a_socket() {
        let b = Binding {
            brain_id: "lease-test-one".into(),
            brain_generation: 1,
            model_digest: String::new(),
            io_digest: String::new(),
            host: "127.0.0.1".into(),
            port: 49123,
        };
        let lease = BindingLease::reserve(&b).unwrap();
        assert!(BindingLease::reserve(&b).is_err());
        let mut other = b.clone();
        other.port += 1;
        assert!(BindingLease::reserve(&other).is_err());
        other.brain_id = "lease-test-two".into();
        let separate = BindingLease::reserve(&other).unwrap();
        drop(lease);
        let resumed = BindingLease::reserve(&b).unwrap();
        drop(resumed);
        drop(separate);
    }
}
