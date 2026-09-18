//! # Sensory Input Providers for Interactive UI
//!
//! This module provides various real-time data sources that can drive the
//! sensory (input) layer of the neural network during interactive simulation.
//!
//! ## Core Interface: `SensoryProvider`
//! Every provider must implement this trait, which defines:
//! - `next_spikes()`: Called once per simulation frame to get new binary spikes.
//! - `last_bands()`: (Optional) Provides frequency or spatial magnitudes for UI visualization.
//!
//! ## Available Providers:
//! - **Random (`RandomProvider`)**: Generates stochastic noise.
//! - **Theta (`ThetaProvider`)**: Deterministic theta‑rhythm spikes.
//! - **Audio (`AudioFileProvider`, `MicrophoneProvider`)**: Performs FFT on audio
//!   to generate spikes based on frequency bands.
//! - **Visual (`ImageFileProvider`, `VideoFileProvider`, `WebcamCaptureProvider`)**:
//!   Resamples image/video frames to generate spikes based on pixel intensity.
//!
//! ## Feature Flags:
//! Use `image_input`, `video_input`, or `webcam_input` to enable specific providers
//! and their respective dependencies.
#[cfg(feature = "ui")]
use std::sync::Arc;
#[cfg(feature = "ui")]
use std::sync::Mutex;

#[cfg(feature = "ui")]
const MAX_VIDEO_PREVIEW_PIXELS: usize = 640 * 480;

/// Latest decoded frame retained for an input preview. This is deliberately
/// separate from sensory admission: pixels are display state and carry no
/// biological timestamp or causal authority.
#[cfg(feature = "ui")]
#[derive(Clone, Debug)]
pub struct VideoPreviewFrame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    pub sequence: u64,
}

#[cfg(feature = "ui")]
pub type VideoPreviewStore = Arc<Mutex<Option<VideoPreviewFrame>>>;

#[cfg(feature = "ui")]
pub fn new_video_preview_store() -> VideoPreviewStore {
    Arc::new(Mutex::new(None))
}

#[cfg(feature = "ui")]
fn publish_video_preview(store: &VideoPreviewStore, width: usize, height: usize, rgb: &[u8]) {
    let Some(source_pixels) = width.checked_mul(height) else {
        return;
    };
    let Some(source_bytes) = source_pixels.checked_mul(3) else {
        return;
    };
    if width == 0 || height == 0 || rgb.len() < source_bytes {
        return;
    }
    let scale = if source_pixels > MAX_VIDEO_PREVIEW_PIXELS {
        (source_pixels as f64 / MAX_VIDEO_PREVIEW_PIXELS as f64).sqrt()
    } else {
        1.0
    };
    let mut out_width = ((width as f64) / scale).floor().max(1.0) as usize;
    let mut out_height = ((height as f64) / scale).floor().max(1.0) as usize;
    while out_width.saturating_mul(out_height) > MAX_VIDEO_PREVIEW_PIXELS {
        if out_width >= out_height {
            out_width = out_width.saturating_sub(1).max(1);
        } else {
            out_height = out_height.saturating_sub(1).max(1);
        }
    }
    let mut out = vec![0u8; out_width.saturating_mul(out_height).saturating_mul(3)];
    for y in 0..out_height {
        let source_y = y.saturating_mul(height) / out_height;
        for x in 0..out_width {
            let source_x = x.saturating_mul(width) / out_width;
            let source = (source_y * width + source_x) * 3;
            let target = (y * out_width + x) * 3;
            out[target..target + 3].copy_from_slice(&rgb[source..source + 3]);
        }
    }
    if let Ok(mut current) = store.lock() {
        let sequence = current
            .as_ref()
            .map(|frame| frame.sequence.saturating_add(1))
            .unwrap_or(1);
        *current = Some(VideoPreviewFrame {
            width: out_width,
            height: out_height,
            rgb: out,
            sequence,
        });
    }
}

#[cfg(feature = "ui")]
use rustfft::{FftPlanner, num_complex::Complex32};

#[cfg(feature = "ui")]
pub trait SensoryProvider {
    fn next_spikes(&mut self) -> Vec<i8>;
    fn last_bands(&self) -> Option<&[f32]> {
        None
    }
    fn stop(&mut self) {}
    fn set_num_sensory_neurons(&mut self, _n_s: usize) {}
    fn set_dt(&mut self, _dt_ms: f32) {}
}

/// Stable, operator-facing description of an available microphone.
///
/// CPAL device handles are short-lived and may become invalid after a hot-plug
/// event.  The UI therefore stores the backend-provided `id` and resolves it
/// again when a stream is started instead of retaining a device handle across
/// refreshes.
#[cfg(feature = "ui")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicrophoneDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Enumerate the input devices currently exposed by the active CPAL host.
pub fn list_microphone_devices() -> anyhow::Result<Vec<MicrophoneDeviceInfo>> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host
        .input_devices()
        .map_err(|error| anyhow::anyhow!("failed to enumerate microphone devices: {error}"))?;

    let mut result = Vec::new();
    for device in devices {
        let id = device
            .id()
            .map_err(|error| anyhow::anyhow!("failed to identify a microphone device: {error}"))?
            .to_string();
        if result
            .iter()
            .any(|entry: &MicrophoneDeviceInfo| entry.id == id)
        {
            continue;
        }
        let name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| id.clone());
        result.push(MicrophoneDeviceInfo {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
        });
    }
    Ok(result)
}

/// Stable, operator-facing description of an available camera.
///
/// Nokhwa may expose a numeric or backend-specific string identity.  Keep the
/// identity as a string so a refresh never collapses two cameras that happen
/// to have the same display name and so macOS/Windows backends retain their
/// native device key.
#[cfg(feature = "webcam_input")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebcamDeviceInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub misc: String,
}

/// Enumerate all cameras currently visible to Nokhwa.
#[cfg(feature = "webcam_input")]
pub fn list_webcam_devices() -> anyhow::Result<Vec<WebcamDeviceInfo>> {
    use nokhwa::utils::ApiBackend;

    let cameras = nokhwa::query(ApiBackend::Auto)
        .map_err(|error| anyhow::anyhow!("failed to enumerate webcam devices: {error}"))?;
    let mut result = Vec::with_capacity(cameras.len());
    for camera in cameras {
        let id = camera.index().as_string();
        if result.iter().any(|entry: &WebcamDeviceInfo| entry.id == id) {
            continue;
        }
        result.push(WebcamDeviceInfo {
            name: camera.human_name(),
            description: camera.description().to_owned(),
            misc: camera.misc(),
            id,
        });
    }
    result.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(result)
}

/// Maximum sensory width accepted by the file-backed audio provider.
///
/// Audio input is an operator-facing boundary.  Keeping its width bounded
/// prevents an accidental environment value or malformed UI request from
/// allocating an unbounded spike raster and the corresponding input weights.
#[cfg(feature = "ui")]
pub const MAX_AUDIO_SENSORY_NEURONS: usize = 65_536;

#[cfg(feature = "ui")]
pub struct RandomProvider {
    num_sensory_neurons: usize,
    p: f32,
}

#[cfg(feature = "ui")]
impl RandomProvider {
    pub fn new(num_sensory_neurons: usize, p: f32) -> Self {
        Self {
            num_sensory_neurons,
            p,
        }
    }
}

#[cfg(feature = "ui")]
impl SensoryProvider for RandomProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        (0..self.num_sensory_neurons)
            .map(|_| if fastrand::f32() < self.p { 1 } else { 0 })
            .collect()
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
    }
}

#[cfg(feature = "ui")]
pub struct ThetaProvider {
    num_sensory_neurons: usize,
    freq_hz: f32,
    duty: f32,
    phase: f32,
    dt_ms: f32,
    phase_jitter: f32,
    phase_offsets: Vec<f32>,
}

#[cfg(feature = "ui")]
impl ThetaProvider {
    pub fn new(
        num_sensory_neurons: usize,
        freq_hz: f32,
        duty: f32,
        phase_jitter: f32,
        dt_ms: f32,
    ) -> Self {
        let mut p = Self {
            num_sensory_neurons,
            freq_hz,
            duty,
            phase: 0.0,
            dt_ms,
            phase_jitter: phase_jitter.clamp(0.0, 1.0),
            phase_offsets: Vec::new(),
        };
        p.rebuild_offsets();
        p
    }

    fn rebuild_offsets(&mut self) {
        self.phase_offsets = (0..self.num_sensory_neurons)
            .map(|i| {
                let h = (i as u32).wrapping_mul(2654435761) & 0xFFFF;
                let base = (h as f32) / 65535.0;
                base * std::f32::consts::TAU * self.phase_jitter
            })
            .collect();
    }
}

#[cfg(feature = "ui")]
impl SensoryProvider for ThetaProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let dt_s = (self.dt_ms.max(0.001)) / 1000.0;
        let freq = self.freq_hz.max(0.01);
        let step = std::f32::consts::TAU * freq * dt_s;
        self.phase = (self.phase + step) % std::f32::consts::TAU;
        let duty = self.duty.clamp(0.0, 1.0);
        let thresh = (1.0 - duty).clamp(0.0, 1.0);
        let mut out = vec![0i8; self.num_sensory_neurons];
        for i in 0..self.num_sensory_neurons {
            let phase = self.phase + self.phase_offsets[i];
            let gate = (phase.sin() * 0.5) + 0.5;
            if gate >= thresh {
                out[i] = 1;
            }
        }
        out
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
        self.rebuild_offsets();
    }
    fn set_dt(&mut self, dt_ms: f32) {
        self.dt_ms = dt_ms;
    }
}

#[cfg(feature = "ui")]
pub struct BandMapper {
    bands: usize,
    // mapping: for each band b, a range (start,end) of sensory indices [start, end)
    ranges: Vec<(usize, usize)>,
}

#[cfg(feature = "ui")]
impl BandMapper {
    pub fn new(num_sensory_neurons_target: usize, bands: usize) -> Self {
        let bands = bands.max(1);
        let mut ranges = Vec::with_capacity(bands);
        let per = (num_sensory_neurons_target as f32) / (bands as f32);
        let mut start = 0usize;
        for b in 0..bands {
            let mut end = (((b + 1) as f32) * per).round() as usize;
            if b == bands - 1 {
                end = num_sensory_neurons_target;
            }
            if end < start {
                end = start;
            }
            ranges.push((start, end));
            start = end;
        }
        Self { bands, ranges }
    }
    pub fn set_n_s(&mut self, n_s: usize) {
        *self = Self::new(n_s, self.bands);
    }
    pub fn ranges(&self) -> &[(usize, usize)] {
        &self.ranges
    }
}

#[cfg(feature = "ui")]
pub struct AudioFileProvider {
    num_sensory_neurons: usize,
    data: Vec<f32>,
    sample_rate: u32,
    cursor: usize,
    frame_index: u64,
    win: usize,
    hop: usize,
    fft: Arc<dyn rustfft::Fft<f32>>,
    scratch: Vec<Complex32>,
    buf: Vec<Complex32>,
    bands: usize,
    last_bands: Vec<f32>,
    mapper: BandMapper,
}

#[cfg(feature = "ui")]
impl AudioFileProvider {
    pub fn from_path(path: &std::path::Path, num_sensory_neurons: usize) -> anyhow::Result<Self> {
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        use symphonia::default::{get_codecs, get_probe};

        anyhow::ensure!(
            num_sensory_neurons > 0,
            "audio input requires at least one sensory neuron"
        );
        anyhow::ensure!(
            num_sensory_neurons <= MAX_AUDIO_SENSORY_NEURONS,
            "audio input supports at most {MAX_AUDIO_SENSORY_NEURONS} sensory neurons"
        );
        let file = std::fs::File::open(path)
            .map_err(|e| anyhow::anyhow!("failed to open audio file {}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let hint = Hint::new();
        let probed = get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;
        let mut format = probed.format;
        let codecs = get_codecs();
        // A video container commonly has a video default track and an audio
        // track. Prefer an audio-bearing track so the same provider can be
        // paired with VideoFileProvider without requiring a second file. The
        // codec registry check is essential: some containers expose audio-like
        // metadata on their video track, which must never be sent to an audio
        // decoder.
        let track = format
            .tracks()
            .iter()
            .find(|track| {
                track.codec_params.sample_rate.is_some()
                    && codecs.get_codec(track.codec_params.codec).is_some()
            })
            .ok_or_else(|| anyhow::anyhow!("No decodable audio track"))?;
        // Extract required fields to avoid holding an immutable borrow of `format` during the read loop.
        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let mut decoder = codecs.make(&codec_params, &DecoderOptions::default())?;
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| anyhow::anyhow!("Unknown sample rate"))?;

        let mut pcm: Vec<f32> = Vec::new();
        while let Ok(packet) = format.next_packet() {
            if packet.track_id() != track_id {
                continue;
            }
            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    use symphonia::core::audio::SampleBuffer;
                    let spec = *audio_buf.spec();
                    let chans = spec.channels.count();
                    let frames = audio_buf.frames();
                    let mut sbuf = SampleBuffer::<f32>::new(frames as u64, spec);
                    sbuf.copy_interleaved_ref(audio_buf);
                    let data = sbuf.samples();
                    if chans == 1 {
                        pcm.extend_from_slice(data);
                    } else {
                        for i in 0..frames {
                            let mut s = 0.0f32;
                            let base = i * chans;
                            for c in 0..chans {
                                s += data[base + c];
                            }
                            pcm.push(s / chans as f32);
                        }
                    }
                }
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "failed to decode audio packet in {}: {error}",
                        path.display()
                    ));
                }
            }
        }

        anyhow::ensure!(
            !pcm.is_empty(),
            "audio file {} contains no decodable samples",
            path.display()
        );
        anyhow::ensure!(
            pcm.iter().all(|sample| sample.is_finite()),
            "audio file {} contains non-finite samples",
            path.display()
        );

        // Normalize gently
        let maxv = pcm.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        if maxv > 0.0001 {
            for v in &mut pcm {
                *v /= maxv;
            }
        }

        // FFT setup
        let win = 1024usize;
        let hop = 512usize;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win);
        let buf = vec![Complex32::new(0.0, 0.0); win];
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        let bands = (num_sensory_neurons as f32).sqrt().round().max(8.0) as usize; // heuristic
        let last_bands = vec![0.0f32; bands];
        let mapper = BandMapper::new(num_sensory_neurons, bands);

        Ok(Self {
            num_sensory_neurons,
            data: pcm,
            sample_rate,
            cursor: 0,
            frame_index: 0,
            win,
            hop,
            fft,
            scratch,
            buf,
            bands,
            last_bands,
            mapper,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn sample_count(&self) -> usize {
        self.data.len()
    }

    pub fn band_count(&self) -> usize {
        self.bands
    }

    fn next_window(&mut self) -> Vec<f32> {
        if self.data.is_empty() {
            return Vec::new();
        }
        if self.cursor + self.win >= self.data.len() {
            self.cursor = 0;
        }
        let start = self.cursor;
        let end = (start + self.win).min(self.data.len());
        self.cursor = (start + self.hop).min(self.data.len());
        self.data[start..end].to_vec()
    }

    fn compute_bands(&mut self, win: &[f32]) {
        // Copy window into complex buffer with Hann window
        let n = self.win.min(win.len());
        for i in 0..self.win {
            self.buf[i] = Complex32::new(0.0, 0.0);
        }
        for i in 0..n {
            let w =
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (self.win as f32 - 1.0)).cos();
            self.buf[i].re = win[i] * w;
        }
        // SAFETY: rustfft requires scratch len query; we preallocated `scratch`
        self.fft
            .process_with_scratch(&mut self.buf, &mut self.scratch);
        // Magnitudes up to Nyquist
        let ny = self.win / 2;
        let mut mags = vec![0.0f32; ny];
        for k in 0..ny {
            mags[k] = self.buf[k].norm();
        }
        // Map to `bands` logarithmically
        let mut bands = vec![0.0f32; self.bands];
        for (bi, val) in bands.iter_mut().enumerate() {
            let f0 = (bi as f32) / (self.bands as f32);
            let f1 = ((bi + 1) as f32) / (self.bands as f32);
            let k0 = (f0.powf(2.0) * ny as f32).floor() as usize; // bias to low freqs
            let k1 = (f1.powf(2.0) * ny as f32).ceil() as usize;
            let k1 = k1.max(k0 + 1).min(ny);
            let mut acc = 0.0f32;
            let mut cnt = 0usize;
            for k in k0..k1 {
                acc += mags[k];
                cnt += 1;
            }
            *val = if cnt > 0 { acc / cnt as f32 } else { 0.0 };
        }
        // Normalize 0..1 and smooth with last_bands
        let maxv = bands.iter().cloned().fold(0.0f32, f32::max).max(1e-6);
        for i in 0..self.bands {
            bands[i] = (0.6 * self.last_bands[i] + 0.4 * (bands[i] / maxv)).min(1.0);
        }
        self.last_bands.copy_from_slice(&bands);
    }
}

#[cfg(feature = "ui")]
impl SensoryProvider for AudioFileProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let win = self.next_window();
        if win.len() < self.win / 2 {
            return vec![0i8; self.num_sensory_neurons];
        }
        self.compute_bands(&win);
        let frame_index = self.frame_index;
        self.frame_index = self.frame_index.wrapping_add(1);
        // Convert bands to spikes across sensory ranges
        let mut out = vec![0i8; self.num_sensory_neurons];
        for (b, &(start, end)) in self.mapper.ranges().iter().enumerate() {
            let p = (self.last_bands[b] * 0.8).min(0.95);
            for i in start..end {
                if deterministic_unit(frame_index, i) < p {
                    out[i] = 1;
                }
            }
        }
        out
    }
    fn last_bands(&self) -> Option<&[f32]> {
        Some(&self.last_bands)
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        // The trait cannot return a validation error.  Keep a provider that
        // was already admitted valid if a later resize request is invalid;
        // the owning UI reports/rejects the resize through its network path.
        if n_s == 0 || n_s > MAX_AUDIO_SENSORY_NEURONS {
            return;
        }
        self.num_sensory_neurons = n_s;
        self.mapper.set_n_s(n_s);
    }
}

/// Combines one visual provider and one audio provider at the same sensory
/// boundary.  Visual and audio spikes are merged deterministically and the
/// audio bands remain available to the Graphic EQ.  The preview frame stays
/// owned by the visual provider and is never used as audio or timing input.
#[cfg(feature = "ui")]
pub struct CombinedVideoAudioProvider {
    video: Box<dyn SensoryProvider + Send>,
    audio: Box<dyn SensoryProvider + Send>,
}

#[cfg(feature = "ui")]
impl CombinedVideoAudioProvider {
    pub fn new(
        video: Box<dyn SensoryProvider + Send>,
        audio: Box<dyn SensoryProvider + Send>,
    ) -> Self {
        Self { video, audio }
    }
}

#[cfg(feature = "ui")]
impl SensoryProvider for CombinedVideoAudioProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let mut visual = self.video.next_spikes();
        let audio = self.audio.next_spikes();
        if visual.len() < audio.len() {
            visual.resize(audio.len(), 0);
        }
        for (visual_spike, audio_spike) in visual.iter_mut().zip(audio.iter()) {
            *visual_spike = (*visual_spike).max(*audio_spike);
        }
        visual
    }

    fn last_bands(&self) -> Option<&[f32]> {
        self.audio.last_bands()
    }

    fn stop(&mut self) {
        self.video.stop();
        self.audio.stop();
    }

    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.video.set_num_sensory_neurons(n_s);
        self.audio.set_num_sensory_neurons(n_s);
    }

    fn set_dt(&mut self, dt_ms: f32) {
        self.video.set_dt(dt_ms);
        self.audio.set_dt(dt_ms);
    }
}

#[cfg(feature = "ui")]
fn deterministic_unit(frame_index: u64, neuron_index: usize) -> f32 {
    // Counter based input dithering keeps the rate-coded raster repeatable
    // across runs and independent of thread scheduling or unrelated RNG use.
    let mut x = frame_index
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .wrapping_add(neuron_index as u64 ^ 0xd1b5_4a32_d192_ed03);
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    ((x >> 40) as u32) as f32 / (1u32 << 24) as f32
}

#[cfg(feature = "ui")]
pub struct MicrophoneProvider {
    num_sensory_neurons: usize,
    device_id: String,
    device_name: String,
    // audio capture
    stream: Option<cpal::Stream>,
    buf: Arc<Mutex<Vec<f32>>>,
    // analysis
    win: usize,
    fft: Arc<dyn rustfft::Fft<f32>>,
    scratch: Vec<Complex32>,
    cbuf: Vec<Complex32>,
    bands: usize,
    last_bands: Vec<f32>,
    mapper: BandMapper,
}

#[cfg(feature = "ui")]
impl MicrophoneProvider {
    pub fn new(num_sensory_neurons: usize) -> anyhow::Result<Self> {
        use cpal::traits::HostTrait;
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No default input device"))?;
        Self::new_for_device(num_sensory_neurons, device)
    }

    /// Start capture from the microphone identified by `MicrophoneDeviceInfo::id`.
    /// The ID is resolved at start time so unplugging a device cannot leave a
    /// stale CPAL handle in the simulation thread.
    pub fn new_with_device_id(num_sensory_neurons: usize, device_id: &str) -> anyhow::Result<Self> {
        use cpal::traits::HostTrait;

        let parsed_id = device_id
            .parse::<cpal::DeviceId>()
            .map_err(|error| anyhow::anyhow!("invalid microphone device ID: {error}"))?;
        let host = cpal::default_host();
        let device = host.device_by_id(&parsed_id).ok_or_else(|| {
            anyhow::anyhow!("selected microphone is no longer available: {device_id}")
        })?;
        Self::new_for_device(num_sensory_neurons, device)
    }

    fn new_for_device(num_sensory_neurons: usize, device: cpal::Device) -> anyhow::Result<Self> {
        use cpal::traits::{DeviceTrait, StreamTrait};

        anyhow::ensure!(
            num_sensory_neurons > 0,
            "microphone input requires at least one sensory neuron"
        );
        let device_id = device
            .id()
            .map_err(|error| anyhow::anyhow!("failed to identify microphone: {error}"))?
            .to_string();
        let device_name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| device_id.clone());
        let mut supported_configs = device
            .supported_input_configs()
            .map_err(|e| anyhow::anyhow!("Failed to query input configs: {}", e))?;
        // Prefer 16k mono f32 if possible
        let mut chosen = None;
        while let Some(cfg) = supported_configs.next() {
            let sr = 16_000u32;
            if cfg.sample_format() == cpal::SampleFormat::F32 {
                let range = cfg.min_sample_rate()..=cfg.max_sample_rate();
                if range.contains(&sr) && cfg.channels() >= 1 {
                    chosen = Some(cfg.with_sample_rate(sr));
                    break;
                }
            }
        }
        // Resolve supported config and separate sample_format from stream config
        let supported = if let Some(c) = chosen {
            c
        } else {
            device
                .default_input_config()
                .map_err(|e| anyhow::anyhow!("Failed to get default input config: {}", e))?
        };
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();
        let _sample_rate = config.sample_rate;
        let channels = config.channels as usize;

        let win = 1024usize;
        let _hop = 512usize;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win);
        let cbuf = vec![Complex32::new(0.0, 0.0); win];
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        let bands = (num_sensory_neurons as f32).sqrt().round().max(8.0) as usize;
        let last_bands = vec![0.0f32; bands];
        let mapper = BandMapper::new(num_sensory_neurons, bands);

        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(win * 8)));
        let cap_limit = win * 8;
        let buf_clone = buf.clone();

        // Build and start a raw stream, converting every supported PCM format to
        // mono f32.  A typed f32 callback is only valid for an f32 device
        // configuration; using it as a generic fallback would reinterpret the
        // bytes of integer or 24-bit microphones.
        fn append_capture_samples<S>(
            data: &[S],
            channels: usize,
            buffer: &mut Vec<f32>,
            cap_limit: usize,
        ) where
            S: cpal::Sample,
            f32: cpal::FromSample<S>,
        {
            use cpal::Sample;

            if channels == 0 {
                return;
            }
            if channels == 1 {
                buffer.extend(data.iter().copied().map(f32::from_sample));
            } else {
                for frame in data.chunks_exact(channels) {
                    let sum: f32 = frame.iter().copied().map(f32::from_sample).sum();
                    buffer.push(sum / channels as f32);
                }
            }
            buffer.retain(|sample| sample.is_finite());
            if buffer.len() > cap_limit {
                let drop = buffer.len() - cap_limit;
                buffer.drain(0..drop);
            }
        }

        let err_fn = |e| nm_err!("Microphone stream error: {}", e);
        let stream = match sample_format {
            cpal::SampleFormat::I8 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<i8>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::I16 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<i16>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::I24 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<cpal::I24>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::I32 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<i32>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::I64 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<i64>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::U8 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<u8>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::U16 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<u16>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::U24 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<cpal::U24>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::U32 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<u32>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::U64 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<u64>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::F32 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<f32>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::F64 => {
                let input_data_fn = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                    if let Some(samples) = data.as_slice::<f64>() {
                        if let Ok(mut buffer) = buf_clone.lock() {
                            append_capture_samples(samples, channels, &mut buffer, cap_limit);
                        }
                    }
                };
                device.build_input_stream_raw(&config, sample_format, input_data_fn, err_fn, None)
            }
            other if other.is_dsd() => {
                return Err(anyhow::anyhow!(
                    "microphone format {other} is DSD and cannot be converted to PCM"
                ));
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unsupported microphone sample format {other}"
                ));
            }
        }
        .map_err(|e| anyhow::anyhow!("Failed to build input stream: {}", e))?;

        stream
            .play()
            .map_err(|e| anyhow::anyhow!("Failed to start stream: {}", e))?;

        Ok(Self {
            num_sensory_neurons,
            device_id,
            device_name,
            stream: Some(stream),
            buf,
            win,
            fft,
            scratch,
            cbuf,
            bands,
            last_bands,
            mapper,
        })
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    fn take_window(&mut self) -> Vec<f32> {
        // Copy last `win` samples from buffer
        if let Ok(b) = self.buf.lock() {
            let n = b.len();
            if n >= self.win {
                b[n - self.win..n].to_vec()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    fn compute_bands(&mut self, win: &[f32]) {
        // Prepare complex buffer with Hann window
        let n = self.win.min(win.len());
        for i in 0..self.win {
            self.cbuf[i] = Complex32::new(0.0, 0.0);
        }
        for i in 0..n {
            let w =
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (self.win as f32 - 1.0)).cos();
            self.cbuf[i].re = win[i] * w;
        }
        self.fft
            .process_with_scratch(&mut self.cbuf, &mut self.scratch);
        let ny = self.win / 2;
        let mut mags = vec![0.0f32; ny];
        for k in 0..ny {
            mags[k] = self.cbuf[k].norm();
        }
        // Map to bands (log-like bias to lows)
        let mut bands = vec![0.0f32; self.bands];
        for bi in 0..self.bands {
            let f0 = (bi as f32) / (self.bands as f32);
            let f1 = ((bi + 1) as f32) / (self.bands as f32);
            let k0 = (f0.powf(2.0) * ny as f32).floor() as usize;
            let k1 = (f1.powf(2.0) * ny as f32).ceil() as usize;
            let k1 = k1.max(k0 + 1).min(ny);
            let mut acc = 0.0f32;
            let mut cnt = 0usize;
            for k in k0..k1 {
                acc += mags[k];
                cnt += 1;
            }
            bands[bi] = if cnt > 0 { acc / cnt as f32 } else { 0.0 };
        }
        let maxv = bands.iter().cloned().fold(0.0f32, f32::max).max(1e-6);
        for i in 0..self.bands {
            bands[i] = (0.6 * self.last_bands[i] + 0.4 * (bands[i] / maxv)).min(1.0);
        }
        self.last_bands.copy_from_slice(&bands);
    }
}

#[cfg(feature = "ui")]
impl SensoryProvider for MicrophoneProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let win = self.take_window();
        if win.len() < self.win / 2 {
            // decay bands when insufficient data
            for v in &mut self.last_bands {
                *v *= 0.9;
            }
            return vec![0i8; self.num_sensory_neurons];
        }
        self.compute_bands(&win);
        let mut out = vec![0i8; self.num_sensory_neurons];
        for (b, &(start, end)) in self.mapper.ranges().iter().enumerate() {
            let p = (self.last_bands[b] * 0.8).min(0.95);
            for i in start..end {
                if fastrand::f32() < p {
                    out[i] = 1;
                }
            }
        }
        out
    }
    fn last_bands(&self) -> Option<&[f32]> {
        Some(&self.last_bands)
    }
    fn stop(&mut self) {
        self.stream = None;
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
        self.mapper.set_n_s(n_s);
    }
}

// ---------------- Image (static picture) provider ----------------
#[cfg(all(feature = "ui", feature = "image_input"))]
pub struct ImageFileProvider {
    num_sensory_neurons: usize,
    gray: Vec<f32>, // grayscale image, row-major, [0,1]
    w: usize,
    h: usize,
    threshold: f32,
    invert: bool,
    use_max: bool, // downsample using max (true) or mean (false)
}

#[cfg(all(feature = "ui", feature = "image_input"))]
impl ImageFileProvider {
    pub fn from_path(path: &std::path::Path, num_sensory_neurons: usize) -> anyhow::Result<Self> {
        let img = image::open(path)?;
        let gray_img = img.to_luma8();
        let (w, h) = gray_img.dimensions();
        let w = w as usize;
        let h = h as usize;
        let mut gray = Vec::with_capacity(w * h);
        for &px in gray_img.as_raw().iter() {
            gray.push((px as f32) / 255.0);
        }
        Ok(Self {
            num_sensory_neurons: num_sensory_neurons,
            gray,
            w,
            h,
            threshold: 0.5,
            invert: false,
            use_max: true,
        })
    }

    fn resample_columns(&self, target: usize) -> Vec<f32> {
        // Collapse rows by mean, then resample columns by mean or max
        if self.w == 0 || self.h == 0 || target == 0 {
            return vec![0.0; target];
        }
        let mut col_vals = vec![0.0f32; self.w];
        for x in 0..self.w {
            let mut acc = 0.0f32;
            for y in 0..self.h {
                acc += self.gray[y * self.w + x];
            }
            col_vals[x] = acc / (self.h as f32);
        }
        // Resample to target using bin aggregation
        let mut out = vec![0.0f32; target];
        for i in 0..target {
            let start = (i * self.w) / target;
            let mut end = ((i + 1) * self.w) / target;
            if end <= start {
                end = (start + 1).min(self.w);
            }
            if self.use_max {
                let mut m = 0.0f32;
                for x in start..end {
                    m = m.max(col_vals[x]);
                }
                out[i] = m;
            } else {
                let mut acc = 0.0f32;
                let mut cnt = 0usize;
                for x in start..end {
                    acc += col_vals[x];
                    cnt += 1;
                }
                out[i] = if cnt > 0 { acc / (cnt as f32) } else { 0.0 };
            }
        }
        out
    }
}

#[cfg(all(feature = "ui", feature = "image_input"))]
impl SensoryProvider for ImageFileProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let vals = self.resample_columns(self.num_sensory_neurons);
        let thr = self.threshold.clamp(0.0, 1.0);
        vals.into_iter()
            .map(|v| {
                let p = if self.invert { 1.0 - v } else { v };
                if p >= thr { 1 } else { 0 }
            })
            .collect()
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
    }
}

// ---------------- Video (mp4) provider via OpenCV ----------------
#[cfg(all(feature = "ui", feature = "video_input", not(target_arch = "aarch64")))]
pub struct VideoFileProvider {
    num_sensory_neurons: usize,
    cap: opencv::videoio::VideoCapture,
    loop_on_eof: bool,
    threshold: f32,
    invert: bool,
    use_max: bool,
    preview: Option<VideoPreviewStore>,
}

#[cfg(all(feature = "ui", feature = "video_input", not(target_arch = "aarch64")))]
impl VideoFileProvider {
    pub fn from_path(
        path: &std::path::Path,
        num_sensory_neurons: usize,
        loop_on_eof: bool,
    ) -> anyhow::Result<Self> {
        Self::from_path_with_preview(path, num_sensory_neurons, loop_on_eof, None)
    }

    pub fn from_path_with_preview(
        path: &std::path::Path,
        num_sensory_neurons: usize,
        loop_on_eof: bool,
        preview: Option<VideoPreviewStore>,
    ) -> anyhow::Result<Self> {
        use opencv::prelude::*;
        let cap = opencv::videoio::VideoCapture::from_file(
            path.to_string_lossy().as_ref(),
            opencv::videoio::CAP_ANY,
        )?;
        if !cap.is_opened()? {
            return Err(anyhow::anyhow!("Failed to open video"));
        }
        Ok(Self {
            num_sensory_neurons: num_sensory_neurons,
            cap,
            loop_on_eof,
            threshold: 0.5,
            invert: false,
            use_max: true,
            preview,
        })
    }

    fn read_gray(&mut self) -> anyhow::Result<(Vec<f32>, usize, usize, Vec<u8>)> {
        use opencv::prelude::VideoCaptureTrait;
        use opencv::prelude::*;
        let mut frame = opencv::core::Mat::default();
        if !self.cap.read(&mut frame)? || frame.empty() {
            if self.loop_on_eof {
                self.cap.set(opencv::videoio::CAP_PROP_POS_FRAMES, 0.0)?;
                let _ = self.cap.read(&mut frame)?;
            }
        }
        if frame.empty() {
            return Ok((Vec::new(), 0, 0, Vec::new()));
        }
        let size = frame.size()?;
        let w = size.width as usize;
        let h = size.height as usize;
        let channels = frame.channels();
        let data_u8 = frame.data_bytes()?;
        let mut out = vec![0.0f32; w * h];
        let mut rgb = vec![0u8; w.saturating_mul(h).saturating_mul(3)];
        if channels == 1 {
            for i in 0..(w * h) {
                out[i] = data_u8[i] as f32 / 255.0;
                rgb[i * 3..i * 3 + 3].fill(data_u8[i]);
            }
        } else {
            // Assume BGR
            for y in 0..h {
                for x in 0..w {
                    let idx = (y * w + x) * channels as usize;
                    // BGR order
                    let b = data_u8[idx] as f32;
                    let g = data_u8[idx + 1] as f32;
                    let r = data_u8[idx + 2] as f32;
                    out[y * w + x] = (0.114 * b + 0.587 * g + 0.299 * r) / 255.0;
                    let rgb_idx = (y * w + x) * 3;
                    rgb[rgb_idx..rgb_idx + 3].copy_from_slice(&[r as u8, g as u8, b as u8]);
                }
            }
        }
        Ok((out, w, h, rgb))
    }

    fn resample_cols(gray: &[f32], w: usize, h: usize, target: usize, use_max: bool) -> Vec<f32> {
        if w == 0 || h == 0 || target == 0 {
            return vec![0.0; target];
        }
        let mut col_vals = vec![0.0f32; w];
        for x in 0..w {
            let mut acc = 0.0f32;
            for y in 0..h {
                acc += gray[y * w + x];
            }
            col_vals[x] = acc / (h as f32);
        }
        let mut out = vec![0.0f32; target];
        for i in 0..target {
            let start = (i * w) / target;
            let mut end = ((i + 1) * w) / target;
            if end <= start {
                end = (start + 1).min(w);
            }
            if use_max {
                let mut m = 0.0f32;
                for x in start..end {
                    m = m.max(col_vals[x]);
                }
                out[i] = m;
            } else {
                let mut acc = 0.0f32;
                let mut cnt = 0usize;
                for x in start..end {
                    acc += col_vals[x];
                    cnt += 1;
                }
                out[i] = if cnt > 0 { acc / (cnt as f32) } else { 0.0 };
            }
        }
        out
    }
}

#[cfg(all(feature = "ui", feature = "video_input", not(target_arch = "aarch64")))]
impl SensoryProvider for VideoFileProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let (gray, w, h, rgb) = match self.read_gray() {
            Ok(t) => t,
            Err(_) => (Vec::new(), 0, 0, Vec::new()),
        };
        if gray.is_empty() {
            return vec![0; self.num_sensory_neurons];
        }
        if let Some(preview) = &self.preview {
            publish_video_preview(preview, w, h, &rgb);
        }
        let vals = Self::resample_cols(
            &gray,
            if w > 0 { w } else { 1 },
            if h > 0 { h } else { gray.len() },
            self.num_sensory_neurons,
            self.use_max,
        );
        let thr = self.threshold.clamp(0.0, 1.0);
        vals.into_iter()
            .map(|v| {
                let p = if self.invert { 1.0 - v } else { v };
                if p >= thr { 1 } else { 0 }
            })
            .collect()
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
    }
}

#[cfg(all(feature = "ui", feature = "video_input", target_arch = "aarch64"))]
pub struct VideoFileProvider {
    num_sensory_neurons: usize,
    preview: Option<VideoPreviewStore>,
}

#[cfg(all(feature = "ui", feature = "video_input", target_arch = "aarch64"))]
impl VideoFileProvider {
    pub fn from_path(
        _path: &std::path::Path,
        _num_sensory_neurons: usize,
        _loop_on_eof: bool,
    ) -> anyhow::Result<Self> {
        Err(anyhow::anyhow!(
            "video_input via OpenCV is not supported on arm64 in this build; use image_input or webcam_input"
        ))
    }

    pub fn from_path_with_preview(
        path: &std::path::Path,
        num_sensory_neurons: usize,
        loop_on_eof: bool,
        _preview: Option<VideoPreviewStore>,
    ) -> anyhow::Result<Self> {
        Self::from_path(path, num_sensory_neurons, loop_on_eof)
    }
}

#[cfg(all(feature = "ui", feature = "video_input", target_arch = "aarch64"))]
impl SensoryProvider for VideoFileProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        vec![0; self.num_sensory_neurons]
    }

    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
    }
}

// ---------------- Webcam provider via Nokhwa ----------------
#[cfg(all(feature = "ui", feature = "webcam_input"))]
pub struct WebcamCaptureProvider {
    num_sensory_neurons: usize,
    cam: nokhwa::Camera,
    threshold: f32,
    invert: bool,
    use_max: bool,
    preview: Option<VideoPreviewStore>,
}

#[cfg(all(feature = "ui", feature = "webcam_input"))]
unsafe impl Send for WebcamCaptureProvider {}
#[cfg(all(feature = "ui", feature = "webcam_input"))]
unsafe impl Sync for WebcamCaptureProvider {}

#[cfg(all(feature = "ui", feature = "webcam_input"))]
impl WebcamCaptureProvider {
    pub fn new(index: u32, num_sensory_neurons: usize) -> anyhow::Result<Self> {
        Self::new_with_device_id(&index.to_string(), num_sensory_neurons, None)
    }

    pub fn new_with_preview(
        index: u32,
        num_sensory_neurons: usize,
        preview: Option<VideoPreviewStore>,
    ) -> anyhow::Result<Self> {
        Self::new_with_device_id(&index.to_string(), num_sensory_neurons, preview)
    }

    pub fn new_with_device_id(
        device_id: &str,
        num_sensory_neurons: usize,
        preview: Option<VideoPreviewStore>,
    ) -> anyhow::Result<Self> {
        use nokhwa::{
            Camera,
            pixel_format::RgbFormat,
            utils::{CameraIndex, RequestedFormat, RequestedFormatType},
        };
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
        let camera_index = device_id
            .parse::<u32>()
            .map(CameraIndex::Index)
            .unwrap_or_else(|_| CameraIndex::String(device_id.to_owned()));
        let mut cam = Camera::new(camera_index, requested)?;
        cam.open_stream()?;
        Ok(Self {
            num_sensory_neurons: num_sensory_neurons,
            cam,
            threshold: 0.5,
            invert: false,
            use_max: true,
            preview,
        })
    }
}

#[cfg(all(feature = "ui", feature = "webcam_input"))]
impl SensoryProvider for WebcamCaptureProvider {
    fn next_spikes(&mut self) -> Vec<i8> {
        let frame = match self.cam.frame() {
            Ok(f) => f,
            Err(_) => return vec![0; self.num_sensory_neurons],
        };
        let res = frame.resolution();
        let w = res.width() as usize;
        let h = res.height() as usize;
        let buf = frame.buffer().to_vec();
        let len = buf.len();
        let pixels = w.saturating_mul(h);

        // Convert to grayscale safely depending on buffer layout
        let mut gray = vec![0.0f32; pixels];
        if pixels == 0 || len == 0 {
            // leave as zeros
        } else if len == pixels {
            // GRAY8
            for i in 0..pixels {
                gray[i] = buf[i] as f32 / 255.0;
            }
        } else if len == pixels * 2 {
            // Likely YUYV422: [Y0 U Y1 V] per two pixels
            for p in 0..pixels {
                let base = (p / 2) * 4; // bytes per pair
                let y_idx = if (p & 1) == 0 { base } else { base + 2 };
                if y_idx < len {
                    gray[p] = buf[y_idx] as f32 / 255.0;
                }
            }
        } else if len >= pixels * 3 {
            // Assume interleaved RGB/BGR with at least 3 bytes per pixel
            // Heuristic: many drivers deliver RGB; even if BGR, luminance formula is symmetric enough for demo
            let stride = len / pixels; // 3 or 4
            for p in 0..pixels {
                let i = p * stride;
                if i + 2 < len {
                    let r = buf[i] as f32;
                    let g = buf[i + 1] as f32;
                    let b = buf[i + 2] as f32;
                    gray[p] = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;
                }
            }
        } else {
            // Unknown format; fallback to zeros of correct length
        }
        if let Some(preview) = &self.preview {
            let mut rgb = vec![0u8; pixels.saturating_mul(3)];
            if len >= pixels.saturating_mul(3) && pixels > 0 {
                let stride = len / pixels;
                for index in 0..pixels {
                    let source = index * stride;
                    let target = index * 3;
                    rgb[target..target + 3].copy_from_slice(&buf[source..source + 3]);
                }
            } else {
                for (index, value) in gray.iter().copied().enumerate() {
                    let value = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                    rgb[index * 3..index * 3 + 3].fill(value);
                }
            }
            publish_video_preview(preview, w, h, &rgb);
        }
        // Downsample
        let mut col_vals = vec![0.0f32; w];
        if w > 0 {
            for x in 0..w {
                let mut acc = 0.0f32;
                for y in 0..h {
                    acc += gray[y * w + x];
                }
                col_vals[x] = if h > 0 { acc / (h as f32) } else { 0.0 };
            }
        }
        let mut vals = vec![0.0f32; self.num_sensory_neurons.max(0)];
        let target = self.num_sensory_neurons.max(1);
        for i in 0..target {
            let start = (i * w) / target;
            let mut end = ((i + 1) * w) / target;
            if end <= start {
                end = (start + 1).min(w);
            }
            if start >= end || w == 0 {
                vals[i] = 0.0;
                continue;
            }
            if self.use_max {
                let mut m: f32 = 0.0;
                for x in start..end {
                    m = m.max(col_vals[x]);
                }
                vals[i] = m;
            } else {
                let mut acc = 0.0f32;
                let mut cnt = 0usize;
                for x in start..end {
                    acc += col_vals[x];
                    cnt += 1;
                }
                vals[i] = if cnt > 0 { acc / (cnt as f32) } else { 0.0 };
            }
        }
        let thr = self.threshold.clamp(0.0, 1.0);
        vals.into_iter()
            .map(|v| {
                let p = if self.invert { 1.0 - v } else { v };
                if p >= thr { 1 } else { 0 }
            })
            .collect()
    }
    fn stop(&mut self) {
        let _ = self.cam.stop_stream();
    }
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
    }
}

#[cfg(all(test, feature = "ui"))]
mod tests {
    use super::{
        AudioFileProvider, CombinedVideoAudioProvider, SensoryProvider, list_microphone_devices,
        new_video_preview_store, publish_video_preview,
    };
    use std::io::Write;
    use std::path::PathBuf;

    fn write_pcm_wav(path: &std::path::Path, sample_rate: u32, samples: &[i16]) {
        let data_bytes = (samples.len() * std::mem::size_of::<i16>()) as u32;
        let byte_rate = sample_rate * 2;
        let mut file = std::fs::File::create(path).expect("create test wav");
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&sample_rate.to_le_bytes()).unwrap();
        file.write_all(&byte_rate.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for sample in samples {
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
        file.flush().unwrap();
    }

    fn temp_wav(name: &str, samples: &[i16]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aarnn-provider-{name}-{}-{}.wav",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        write_pcm_wav(&path, 8_000, samples);
        path
    }

    #[test]
    fn microphone_enumeration_is_stable_or_reports_an_explicit_error() {
        match list_microphone_devices() {
            Ok(devices) => {
                let mut ids = std::collections::HashSet::new();
                for device in &devices {
                    assert!(!device.id.is_empty());
                    assert!(!device.name.is_empty());
                    assert!(ids.insert(device.id.clone()), "duplicate microphone ID");
                }
                eprintln!(
                    "microphone devices: {}",
                    devices
                        .iter()
                        .map(|device| device.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            Err(error) => assert!(!error.to_string().trim().is_empty()),
        }
    }

    #[cfg(feature = "webcam_input")]
    #[test]
    fn webcam_enumeration_is_stable_or_reports_an_explicit_error() {
        match super::list_webcam_devices() {
            Ok(devices) => {
                let mut ids = std::collections::HashSet::new();
                for device in &devices {
                    assert!(!device.id.is_empty());
                    assert!(ids.insert(device.id.clone()), "duplicate webcam ID");
                }
                eprintln!("webcam devices: {}", devices.len());
            }
            Err(error) => assert!(!error.to_string().trim().is_empty()),
        }
    }

    #[test]
    fn audio_provider_decodes_eq_and_emits_repeatable_spikes() {
        let samples: Vec<i16> = (0..8_192)
            .map(|i| {
                let phase = (i as f32 * std::f32::consts::TAU * 440.0 / 8_000.0).sin();
                (phase * i16::MAX as f32 * 0.7) as i16
            })
            .collect();
        let path = temp_wav("sine", &samples);
        let mut first = AudioFileProvider::from_path(&path, 64).expect("decode sine wav");
        let mut second = AudioFileProvider::from_path(&path, 64).expect("decode sine wav");

        assert_eq!(first.sample_rate(), 8_000);
        assert_eq!(first.sample_count(), samples.len());
        assert!(first.band_count() >= 8);
        let mut saw_band_energy = false;
        let mut saw_spike = false;
        for _ in 0..4 {
            let first_spikes = first.next_spikes();
            let second_spikes = second.next_spikes();
            assert_eq!(first_spikes, second_spikes);
            assert_eq!(first_spikes.len(), 64);
            saw_spike |= first_spikes.iter().any(|&spike| spike != 0);
            saw_band_energy |= first
                .last_bands()
                .unwrap()
                .iter()
                .any(|&band| band.is_finite() && band > 0.0);
        }
        assert!(
            saw_band_energy,
            "the EQ must expose decoded spectral energy"
        );
        assert!(saw_spike, "non-silent audio must drive sensory spikes");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audio_provider_decodes_audio_track_from_mp4_video() {
        let path = std::env::temp_dir().join(format!(
            "aarnn-provider-video-audio-{}-{}.mp4",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(
            &path,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/video-with-audio.mp4"
            )),
        )
        .expect("write MP4 fixture");

        let mut provider =
            AudioFileProvider::from_path(&path, 64).expect("select and decode the MP4 audio track");
        assert_eq!(provider.sample_rate(), 8_000);
        assert!(provider.sample_count() > 0);
        let spikes = provider.next_spikes();
        assert_eq!(spikes.len(), 64);
        assert!(
            provider
                .last_bands()
                .unwrap()
                .iter()
                .any(|&band| band.is_finite() && band > 0.0)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audio_provider_maps_silence_to_zero_spikes() {
        let samples = vec![0i16; 2_048];
        let path = temp_wav("silence", &samples);
        let mut provider = AudioFileProvider::from_path(&path, 32).expect("decode silent wav");
        for _ in 0..2 {
            assert!(provider.next_spikes().iter().all(|&spike| spike == 0));
        }
        assert!(
            provider
                .last_bands()
                .unwrap()
                .iter()
                .all(|&band| band == 0.0)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audio_provider_rejects_zero_sensory_neurons() {
        let path = temp_wav("zero-sensory", &[1i16; 1_024]);
        let error = match AudioFileProvider::from_path(&path, 0) {
            Ok(_) => panic!("zero sensory input must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("at least one sensory neuron"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn audio_provider_rejects_unbounded_sensory_width() {
        let path = temp_wav("oversized-sensory", &[1i16; 1_024]);
        let error = match AudioFileProvider::from_path(&path, super::MAX_AUDIO_SENSORY_NEURONS + 1)
        {
            Ok(_) => panic!("oversized sensory input must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("supports at most"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn video_preview_store_keeps_one_bounded_latest_frame() {
        let store = new_video_preview_store();
        let rgb = vec![127u8; 1_000 * 1_000 * 3];
        publish_video_preview(&store, 1_000, 1_000, &rgb);
        let first = store.lock().unwrap().clone().expect("preview frame");
        assert_eq!(first.sequence, 1);
        assert_eq!(first.rgb.len(), first.width * first.height * 3);
        assert!(first.width * first.height <= super::MAX_VIDEO_PREVIEW_PIXELS);

        publish_video_preview(&store, 2, 1, &[1, 2, 3, 4, 5, 6]);
        let second = store.lock().unwrap().clone().expect("latest preview frame");
        assert_eq!(second.sequence, 2);
        assert_eq!((second.width, second.height), (2, 1));
        assert_eq!(second.rgb, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn combined_video_audio_provider_exposes_audio_bands_and_merges_spikes() {
        struct FixtureProvider {
            spikes: Vec<i8>,
            bands: Option<Vec<f32>>,
        }

        impl SensoryProvider for FixtureProvider {
            fn next_spikes(&mut self) -> Vec<i8> {
                self.spikes.clone()
            }

            fn last_bands(&self) -> Option<&[f32]> {
                self.bands.as_deref()
            }
        }

        let mut provider = CombinedVideoAudioProvider::new(
            Box::new(FixtureProvider {
                spikes: vec![1, 0, 0],
                bands: None,
            }),
            Box::new(FixtureProvider {
                spikes: vec![0, 1, 0],
                bands: Some(vec![0.25, 0.75]),
            }),
        );
        assert_eq!(provider.next_spikes(), vec![1, 1, 0]);
        assert_eq!(provider.last_bands(), Some(&[0.25, 0.75][..]));
    }
}
