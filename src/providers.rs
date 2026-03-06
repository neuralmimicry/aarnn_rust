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
use rustfft::{num_complex::Complex32, FftPlanner};

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
    cursor: usize,
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
        use symphonia::default::get_probe;

        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let hint = Hint::new();
        let probed = get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;
        let mut format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| anyhow::anyhow!("No default track"))?;
        // Extract required fields to avoid holding an immutable borrow of `format` during the read loop.
        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let mut decoder =
            symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;
        let _sample_rate = codec_params
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
                Err(_) => { /* ignore decode errors */ }
            }
        }

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
            cursor: 0,
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
        // Convert bands to spikes across sensory ranges
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
    fn set_num_sensory_neurons(&mut self, n_s: usize) {
        self.num_sensory_neurons = n_s;
        self.mapper.set_n_s(n_s);
    }
}

#[cfg(feature = "ui")]
pub struct MicrophoneProvider {
    num_sensory_neurons: usize,
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
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No default input device"))?;
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

        // Build and start stream, converting to mono f32
        let err_fn = |e| nm_err!("Microphone stream error: {}", e);
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut b) = buf_clone.lock() {
                        if channels == 1 {
                            b.extend_from_slice(data);
                        } else {
                            for frame in data.chunks_exact(channels) {
                                let mut acc = 0.0f32;
                                for &s in frame { acc += s; }
                                b.push(acc / channels as f32);
                            }
                        }
                        if b.len() > cap_limit { let drop = b.len() - cap_limit; b.drain(0..drop); }
                    }
                };
                device.build_input_stream(&config, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::I16 => {
                let input_data_fn = move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut b) = buf_clone.lock() {
                        let scale = 1.0f32 / i16::MAX as f32;
                        if channels == 1 {
                            b.extend(data.iter().map(|&v| v as f32 * scale));
                        } else {
                            for frame in data.chunks_exact(channels) {
                                let mut acc = 0.0f32; for &s in frame { acc += s as f32 * scale; } b.push(acc / channels as f32);
                            }
                        }
                        if b.len() > cap_limit { let drop = b.len() - cap_limit; b.drain(0..drop); }
                    }
                };
                device.build_input_stream(&config, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::I32 => {
                let input_data_fn = move |data: &[i32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut b) = buf_clone.lock() {
                        let scale = 1.0f32 / i32::MAX as f32;
                        if channels == 1 {
                            b.extend(data.iter().map(|&v| v as f32 * scale));
                        } else {
                            for frame in data.chunks_exact(channels) {
                                let mut acc = 0.0f32; for &s in frame { acc += s as f32 * scale; } b.push(acc / channels as f32);
                            }
                        }
                        if b.len() > cap_limit { let drop = b.len() - cap_limit; b.drain(0..drop); }
                    }
                };
                device.build_input_stream(&config, input_data_fn, err_fn, None)
            }
            cpal::SampleFormat::U16 => {
                let input_data_fn = move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut b) = buf_clone.lock() {
                        if channels == 1 {
                            b.extend(data.iter().map(|&v| (v as f32 / 32767.5) - 1.0));
                        } else {
                            for frame in data.chunks_exact(channels) {
                                let mut acc = 0.0f32; for &s in frame { acc += (s as f32 / 32767.5) - 1.0; } b.push(acc / channels as f32);
                            }
                        }
                        if b.len() > cap_limit { let drop = b.len() - cap_limit; b.drain(0..drop); }
                    }
                };
                device.build_input_stream(&config, input_data_fn, err_fn, None)
            }
            other => {
                // Fallback: try to build as f32 using cpal internal conversion if supported
                nm_err!("Unsupported input sample format {:?}, attempting f32 stream with internal conversion", other);
                let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut b) = buf_clone.lock() {
                        if channels == 1 { b.extend_from_slice(data); }
                        else {
                            for frame in data.chunks_exact(channels) {
                                let mut acc = 0.0f32; for &s in frame { acc += s; } b.push(acc / channels as f32);
                            }
                        }
                        if b.len() > cap_limit { let drop = b.len() - cap_limit; b.drain(0..drop); }
                    }
                };
                device.build_input_stream(&config, input_data_fn, err_fn, None)
            }
        }.map_err(|e| anyhow::anyhow!("Failed to build input stream: {}", e))?;

        stream
            .play()
            .map_err(|e| anyhow::anyhow!("Failed to start stream: {}", e))?;

        Ok(Self {
            num_sensory_neurons,
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
                if p >= thr {
                    1
                } else {
                    0
                }
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
}

#[cfg(all(feature = "ui", feature = "video_input", not(target_arch = "aarch64")))]
impl VideoFileProvider {
    pub fn from_path(
        path: &std::path::Path,
        num_sensory_neurons: usize,
        loop_on_eof: bool,
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
        })
    }

    fn read_gray(&mut self) -> anyhow::Result<(Vec<f32>, usize, usize)> {
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
            return Ok((Vec::new(), 0, 0));
        }
        let size = frame.size()?;
        let w = size.width as usize;
        let h = size.height as usize;
        let channels = frame.channels();
        let data_u8 = frame.data_bytes()?;
        let mut out = vec![0.0f32; w * h];
        if channels == 1 {
            for i in 0..(w * h) {
                out[i] = data_u8[i] as f32 / 255.0;
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
                }
            }
        }
        Ok((out, w, h))
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
        let (gray, w, h) = match self.read_gray() {
            Ok(t) => t,
            Err(_) => (Vec::new(), 0, 0),
        };
        if gray.is_empty() {
            return vec![0; self.num_sensory_neurons];
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
                if p >= thr {
                    1
                } else {
                    0
                }
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
}

#[cfg(all(feature = "ui", feature = "webcam_input"))]
unsafe impl Send for WebcamCaptureProvider {}
#[cfg(all(feature = "ui", feature = "webcam_input"))]
unsafe impl Sync for WebcamCaptureProvider {}

#[cfg(all(feature = "ui", feature = "webcam_input"))]
impl WebcamCaptureProvider {
    pub fn new(index: u32, num_sensory_neurons: usize) -> anyhow::Result<Self> {
        use nokhwa::{
            pixel_format::RgbFormat,
            utils::{CameraIndex, RequestedFormat, RequestedFormatType},
            Camera,
        };
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
        let mut cam = Camera::new(CameraIndex::Index(index), requested)?;
        cam.open_stream()?;
        Ok(Self {
            num_sensory_neurons: num_sensory_neurons,
            cam,
            threshold: 0.5,
            invert: false,
            use_max: true,
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
                if p >= thr {
                    1
                } else {
                    0
                }
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
