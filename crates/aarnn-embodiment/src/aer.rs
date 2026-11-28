// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
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
    /// `legacy-contact`: only contacts inside this body-local box
    /// `[[x0, y0, z0], [x1, y1, z1]]` count (a bumper switch on the shell,
    /// not the sole that stands on the floor).
    #[serde(default)]
    pub region_m: Option<[[f64; 3]; 2]>,
    /// `legacy-range`: explicit ray directions (mount-body frame) sampling
    /// the transducer's cone; the reading is the nearest echo. Every engine
    /// casts these same rays. Absent: the single `direction` ray.
    #[serde(default)]
    pub rays: Option<Vec<[f64; 3]>>,
    /// `legacy-range`: a ray whose angle of incidence on the surface it hits
    /// exceeds this (rad) returns no echo (sonar specular loss; Webots
    /// "sonar" uses pi/8). Absent: every hit echoes.
    #[serde(default)]
    pub incidence_max_rad: Option<f64>,
    /// `legacy-range`: nearest measurable distance (m); nearer objects read
    /// as this distance (presence without a distance).
    #[serde(default)]
    pub min_m: Option<f64>,
    /// Sample-and-hold period (s): the transducer measures once per period
    /// and holds its reading in between (a sonar's ping cycle).
    #[serde(default)]
    pub period_s: Option<f64>,
    /// `legacy-acoustic` (`acoustics`): band centre (Hz), directivity at and
    /// above 1 kHz (0 omni, 1 cardioid), level range mapped to [0, 1], and
    /// for `mode: "itd"` the paired receiver's sensor name. The receiver
    /// sits at `position_m` on `body`, its axis along `direction`.
    #[serde(default)]
    pub band_hz: Option<f64>,
    #[serde(default)]
    pub directivity: Option<f64>,
    #[serde(default)]
    pub floor_db: Option<f64>,
    #[serde(default)]
    pub ceil_db: Option<f64>,
    #[serde(default)]
    pub pair: Option<String>,
    /// Optional sensor group (absent: the body's core contract), e.g.
    /// "hearing": present on the body, bound only by networks that name it.
    #[serde(default)]
    pub group: Option<String>,
    /// Receptive field spanning several bodies (C. elegans touch cells: ALM/AVM
    /// the anterior half, PLM the posterior half); any of them counts.
    #[serde(default)]
    pub bodies: Option<Vec<String>>,
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
    /// Servo wake-up (graded position readouts): after arming, the target
    /// blends linearly from `from` to the decoded target over `seconds`, as a
    /// position-servo robot interpolates from its rest pose on wake-up. A
    /// graded brain's outputs start at zero density, which would otherwise
    /// command every joint to its lower limit for the first ~50 ms.
    #[serde(default)]
    pub wake: Option<Wake>,
    /// Graded decode time constant (s) for this actuator's readout; default
    /// motor::RATE_TAU_S (20 ms). A slower decode is the transducer's own
    /// low-pass (a muscle twitch, a servo's position loop): at 20 ms the
    /// pulse-density ripple (~+-5 % of the joint range) topples a biped.
    #[serde(default)]
    pub decode_tau_s: Option<f64>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Wake {
    pub from: f64,
    pub seconds: f64,
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
    /// Insect flight motor (`resonant-flight-v1`, Drosophila bodies).
    #[serde(default)]
    pub flight: Option<crate::flight::Flight>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    pub half_extent_m: f64,
    /// Analytic stimulus fields (C. elegans habitat v3); empty for others.
    #[serde(default)]
    pub stimuli: Vec<Stimulus>,
    /// Static sound sources of the habitat (`acoustics`); moving ones (an
    /// NPC, another robot) are supplied by the engine each step.
    #[serde(default)]
    pub sound_sources: Vec<crate::acoustics::SoundSource>,
    /// Periodic whole-plate tap (mechanical stimulus), if declared.
    #[serde(default)]
    pub taps: Option<Taps>,
    /// Resistive-force (agar groove) substrate drag per body segment.
    #[serde(default)]
    pub resistive_force: Option<ResistiveForce>,
    /// Engine-independent water model (zebrafish `resistive-ellipsoid-v1`).
    #[serde(default)]
    pub hydrodynamics: Option<Hydrodynamics>,
    /// Engine-independent air model (crow `quasi-steady-panel-v2`).
    #[serde(default)]
    pub aerodynamics: Option<Aerodynamics>,
    /// Initial free-root velocity applied at load and every reset (a bird
    /// released in flight), in world axes.
    #[serde(default)]
    pub launch: Option<Launch>,
    /// Energy needs (`needs-v1`): an energy store that drains with activity
    /// and refills at food (animals) or a charging dock (machines), with a
    /// caretaker that presents food or docks the body in early life.
    #[serde(default)]
    pub needs: Option<Needs>,
}
/// Energy needs of a body (`needs-v1`, engineering proxy). The store holds
/// `basal_w_per_kg x total mass x endurance_s` and drains at the basal rate
/// times `1 + effort` (effort = mechanical power / (`effort_ref_w_per_kg` x
/// mass), at most 1). It refills in `refill_s` from empty while the body is
/// at a source, and only below full. At zero the body is exhausted (disarmed).
/// What the body does about it is not wired: the network learns that from the
/// sensed reserve, the feeding events and the cue field.
#[derive(Debug, Clone, Deserialize)]
pub struct Needs {
    pub version: String,
    /// `food` (animals) or `power` (machines).
    pub resource: String,
    pub basal_w_per_kg: f64,
    pub endurance_s: f64,
    pub effort_ref_w_per_kg: f64,
    pub refill_s: f64,
    pub start_fraction: f64,
    /// Refill hysteresis: intake at a source starts only below this fraction
    /// and then continues to full (a charger's top-off threshold; a meal ends
    /// at satiety). Without it a full body topped up continuously and the
    /// feeding reflex held it on the dock for good (Keenon, 25 Nov).
    #[serde(default = "default_refill_start")]
    pub refill_start_fraction: f64,
    pub sources: Vec<NeedsSource>,
    #[serde(default)]
    pub caretaker: Option<Caretaker>,
}
fn default_refill_start() -> f64 {
    0.9
}
/// A food item or charging dock: consumed within `radius_m` of the body's
/// root (horizontal distance); sensed through a Gaussian cue field of width
/// `field_radius_m` (odour or dock beacon).
#[derive(Debug, Clone, Deserialize)]
pub struct NeedsSource {
    pub id: String,
    pub position_m: [f64; 3],
    pub radius_m: f64,
    pub field_radius_m: f64,
}
/// Early-life care: until `until_s`, whenever the reserve falls below
/// `threshold` the caretaker presents food at the mouth (or plugs the robot
/// in) for `feed_s`: energy flows in, the consume channel and the cue field
/// read as at a source.
#[derive(Debug, Clone, Deserialize)]
pub struct Caretaker {
    pub until_s: f64,
    pub threshold: f64,
    pub feed_s: f64,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Aerodynamics {
    pub model: String,
    pub density_kg_m3: f64,
    /// Named polars `[lift_slope_per_rad, lift_max, drag_zero, drag_induced,
    /// drag_stall]` (see `force::polar_panel_force`).
    pub polars: std::collections::BTreeMap<String, AeroPolar>,
    pub panels: Vec<AeroPanel>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct AeroPolar {
    pub lift_slope_per_rad: f64,
    pub lift_max: f64,
    pub drag_zero: f64,
    pub drag_induced: f64,
    pub drag_stall: f64,
}
impl AeroPolar {
    pub fn coefficients(&self) -> [f64; 5] {
        [
            self.lift_slope_per_rad,
            self.lift_max,
            self.drag_zero,
            self.drag_induced,
            self.drag_stall,
        ]
    }
}
#[derive(Debug, Clone, Deserialize)]
pub struct AeroPanel {
    pub body: String,
    /// Centre of pressure in the body frame.
    pub point_m: [f64; 3],
    pub area_m2: f64,
    pub polar: String,
    /// Panel frame (X chord, Y span, Z normal) relative to the body frame.
    #[serde(default = "identity_quaternion")]
    pub quaternion_wxyz: [f64; 4],
}
fn identity_quaternion() -> [f64; 4] {
    [1., 0., 0., 0.]
}
#[derive(Debug, Clone, Deserialize)]
pub struct Launch {
    pub root_linear_velocity_mps: [f64; 3],
}
#[derive(Debug, Clone, Deserialize)]
pub struct Hydrodynamics {
    pub model: String,
    pub density_kg_m3: f64,
    pub surface_z_m: f64,
    pub bottom_z_m: f64,
    pub submergence_band_m: f64,
    pub drag_coefficients: [f64; 3],
    pub rotational_coefficient: f64,
    /// Centre of buoyancy relative to each body's centre of mass, in body
    /// axes (a dorsal swim bladder gives a passive righting moment).
    #[serde(default)]
    pub buoyancy_offset_m: [f64; 3],
    pub bodies: Vec<HydroBody>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct HydroBody {
    pub body: String,
    pub half_size_m: [f64; 3],
    pub offset_m: [f64; 3],
}
impl Hydrodynamics {
    pub fn water(&self) -> crate::force::Water {
        crate::force::Water {
            density: self.density_kg_m3,
            surface_z: self.surface_z_m,
            bottom_z: self.bottom_z_m,
            band: self.submergence_band_m,
            drag: self.drag_coefficients,
            rotational: self.rotational_coefficient,
        }
    }
}
/// One additive contribution to a named normalised field.
#[derive(Debug, Clone, Deserialize)]
pub struct Stimulus {
    pub field: String,
    pub shape: String,
    #[serde(default)]
    pub center: [f64; 2],
    #[serde(default)]
    pub radii: [f64; 2],
    #[serde(default)]
    pub radius: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub edge: f64,
    #[serde(default)]
    pub strength: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub gradient: [f64; 2],
}
impl Stimulus {
    /// Contribution at the horizontal point `(x, y)`.
    pub fn value(&self, x: f64, y: f64) -> f64 {
        let dx = x - self.center[0];
        let dy = y - self.center[1];
        match self.shape.as_str() {
            "gaussian" => {
                let sx = self.radii[0].max(1.0e-6);
                let sy = self.radii[1].max(1.0e-6);
                self.strength * (-0.5 * ((dx / sx).powi(2) + (dy / sy).powi(2))).exp()
            }
            "ellipse" => {
                let ax = self.radii[0].max(1.0e-6);
                let ay = self.radii[1].max(1.0e-6);
                let r = ((dx / ax).powi(2) + (dy / ay).powi(2)).sqrt();
                // Soft edge in metres measured along the mean semi-axis.
                let edge = (self.edge / (0.5 * (ax + ay))).max(1.0e-6);
                self.strength / (1.0 + ((r - 1.0) / edge).exp())
            }
            "ring" => {
                let d = (dx * dx + dy * dy).sqrt() - self.radius;
                let w = self.width.max(1.0e-6);
                self.strength * (-0.5 * (d / (0.5 * w)).powi(2)).exp()
            }
            "linear" => self.offset + self.gradient[0] * x + self.gradient[1] * y,
            "constant" => self.strength,
            _ => 0.0,
        }
    }
}
/// Sum of every stimulus of `field` at `(x, y)`, clamped to [0, 1].
pub fn field_value(stimuli: &[Stimulus], field: &str, x: f64, y: f64) -> f64 {
    stimuli
        .iter()
        .filter(|s| s.field == field)
        .map(|s| s.value(x, y))
        .sum::<f64>()
        .clamp(0.0, 1.0)
}
#[derive(Debug, Clone, Deserialize)]
pub struct Taps {
    pub period_s: f64,
    pub first_s: f64,
    pub duration_s: f64,
    pub acceleration_mps2: f64,
}
impl Taps {
    /// Whether simulation time `t` (s) lies inside a tap pulse.
    pub fn active(&self, t: f64) -> bool {
        // A non-positive or NaN period never taps.
        if self.period_s.is_nan() || self.period_s <= 0.0 || t < self.first_s {
            return false;
        }
        (t - self.first_s).rem_euclid(self.period_s) < self.duration_s
    }
}
#[derive(Debug, Clone, Deserialize)]
pub struct ResistiveForce {
    #[serde(rename = "tangential_N_s_per_m")]
    pub tangential: f64,
    #[serde(rename = "normal_N_s_per_m")]
    pub normal: f64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Binding {
    pub brain_id: String,
    pub brain_generation: u64,
    pub model_digest: String,
    pub io_digest: String,
    pub host: String,
    pub port: u16,
    /// Optional sensor groups: the record channels this brain was built
    /// with, in its own order (by record sensor name). A body may carry more
    /// senses (e.g. hearing) than a preserved network knows; the network
    /// binds exactly its own channels. Absent: every record sensor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor_names: Option<Vec<String>>,
}
impl Binding {
    /// Record sensor indices sent to this brain, in its order.
    pub fn selection(&self, model: &Model) -> Result<Vec<usize>, String> {
        match &self.sensor_names {
            None => Ok((0..model.io.sensors.len()).collect()),
            Some(names) => {
                let mut seen = HashSet::new();
                names
                    .iter()
                    .map(|n| {
                        if !seen.insert(n.as_str()) {
                            return Err(format!("binding names sensor {n} twice"));
                        }
                        model
                            .io
                            .sensors
                            .iter()
                            .position(|s| &s.name == n)
                            .ok_or_else(|| format!("binding sensor {n} is not in the body record"))
                    })
                    .collect()
            }
        }
    }
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
        self.selection(model)?;
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
#[derive(Debug, Default)]
pub struct Reply {
    pub source_step: u64,
    pub timestamp_us: u64,
    pub spikes: Vec<u32>,
    /// Encoding of `spikes`, negotiated once in the handshake reply.
    pub output_code: OutputCode,
}
/// Encoding of the brain's output events, advertised by the brain in its
/// handshake reply (`output_code`). Both arrive as per-frame event counts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputCode {
    /// Spike trains of spiking output cells (no key, or `"spikes"`).
    #[default]
    Spikes,
    /// Pulse-density code of graded activations (`"pulse_density"`): the
    /// fraction of 2 ms frames carrying an event equals the activation.
    PulseDensity,
}
impl OutputCode {
    pub fn from_handshake(reply: &serde_json::Value) -> Result<Self, String> {
        match reply.get("output_code") {
            None | Some(serde_json::Value::Null) => Ok(Self::Spikes),
            Some(v) => match v.as_str() {
                Some("spikes") => Ok(Self::Spikes),
                Some("pulse_density") => Ok(Self::PulseDensity),
                _ => Err(format!("unsupported brain output_code {v}")),
            },
        }
    }
}
#[derive(Debug, Default, Serialize, Clone)]
pub struct Status {
    pub ready: bool,
    pub output_code: OutputCode,
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
    stream.read_exact(&mut length).map_err(read_error)?;
    let count = u32::from_le_bytes(length) as usize;
    if !(1..=4 * 1024 * 1024).contains(&count) {
        return Err("response budget".into());
    }
    let mut response = vec![0; count];
    stream.read_exact(&mut response).map_err(read_error)?;
    Ok(response)
}
/// A socket read timeout surfaces as `WouldBlock` (EAGAIN) on Unix and
/// `TimedOut` on Windows; report it as the brain-reply timeout it is.
fn read_error(error: std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            "no brain reply within the data timeout (NM_AER_REFERENCE_DATA_TIMEOUT_MS)".into()
        }
        _ => error.to_string(),
    }
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
                let selection = binding.selection(&model)?;
                let handshake = serde_json::json!({"brain_id":binding.brain_id,"brain_generation":binding.brain_generation,"model_digest":binding.model_digest,"io_digest":binding.io_digest,"sensory_base":model.io.sensory_base,"output_base":model.io.output_base,"s_names":selection.iter().map(|&i|&model.io.sensors[i].name).collect::<Vec<_>>(),"o_names":model.io.output_names,"sensory":selection.len(),"output":model.io.output_names.len(),"dt_ms":2.});
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
                    ("expected_s", serde_json::json!(selection.len())),
                    ("expected_o", serde_json::json!(model.io.output_names.len())),
                ] {
                    if reply.get(key) != Some(&value) {
                        return Err(format!("strict reference handshake mismatch: {key}"));
                    }
                }
                // Managed layout/name negotiation may take longer than one
                // steady-state exchange. Restore the 5s data budget, which an
                // explicitly slower-than-real-time lock-step experiment may
                // raise with NM_AER_REFERENCE_DATA_TIMEOUT_MS (1 s .. 10 min).
                let data_timeout_ms = std::env::var("NM_AER_REFERENCE_DATA_TIMEOUT_MS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|v| v.clamp(1_000, 600_000))
                    .unwrap_or(5_000);
                stream
                    .set_read_timeout(Some(Duration::from_millis(data_timeout_ms)))
                    .map_err(|e| e.to_string())?;
                let output_code = OutputCode::from_handshake(&reply)?;
                {
                    let mut status = progress.lock().unwrap();
                    status.output_code = output_code;
                    status.ready = true;
                }
                let mut phase = vec![0.; selection.len()];
                while !cancel.load(Ordering::Relaxed) {
                    let capture = match rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(c) => c,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    if capture.values.len() != model.io.sensors.len() {
                        return Err("capture shape mismatch".into());
                    }
                    let mut spikes = vec![0; phase.len()];
                    for (i, value) in selection.iter().map(|&k| capture.values[k]).enumerate() {
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
                        output_code,
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
    fn a_network_binds_exactly_its_own_named_channels() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sim/content/embodiments/generated/nao.json");
        let model: Model = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let names: Vec<String> = model.io.sensors.iter().map(|s| s.name.clone()).collect();
        let mut b = Binding {
            brain_id: "selection".into(),
            brain_generation: 1,
            model_digest: model.digest.clone(),
            io_digest: model.io.digest.clone(),
            host: "127.0.0.1".into(),
            port: 49200,
            sensor_names: None,
        };
        assert_eq!(b.selection(&model).unwrap().len(), names.len());
        // A network built with fewer channels, in its own order.
        b.sensor_names = Some(vec![names[3].clone(), names[0].clone()]);
        assert_eq!(b.selection(&model).unwrap(), vec![3, 0]);
        b.sensor_names = Some(vec!["no-such-ear".into()]);
        assert!(b.selection(&model).is_err());
        b.sensor_names = Some(vec![names[1].clone(), names[1].clone()]);
        assert!(b.selection(&model).is_err());
    }
    #[test]
    fn binding_lease_is_atomic_and_released_without_a_socket() {
        let b = Binding {
            brain_id: "lease-test-one".into(),
            brain_generation: 1,
            model_digest: String::new(),
            io_digest: String::new(),
            host: "127.0.0.1".into(),
            port: 49123,
            sensor_names: None,
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
