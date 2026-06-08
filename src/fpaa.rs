//! FPAA detection, verification, and per-kernel routing support.
//!
//! The current implementation focuses on three concerns:
//! - host-side transport discovery (Pi.HAT GPIO/SPI and USB-style endpoints)
//! - verification that a known AARNN kernel image was most recently programmed
//! - operator-visible routing state so CLI/UI can request FPAA offload or software fallback
//!
//! The numerical AARNN kernels remain the software source of truth. When a kernel is
//! requested on FPAA but the transport/image verification is not good enough, the
//! effective route falls back to the Rust implementation.

use ndarray::{Array1, Array2, arr1};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::aarnn::dynamics::{
    ActiveDendriteSpec, DendriteStructureSignal, SpatialPoint3, SynapticDriveParams,
    apply_active_dendritic_compartment, apply_local_gap_junction_coupling, apply_synaptic_filter,
    volume_transmission_factors_for_layer,
};
use crate::aarnn::plasticity::{
    ShortTermPlasticityParams, ShortTermPlasticityState, apply_synaptic_scaling_matrix_rows,
    enforce_dale_matrix_cols_with_mask, stp_step, triplet_eta_scale,
};
use crate::aarnn::transmission::{
    CompartmentClass, DelayAttenuationSpec, DendriticTransmissionProfile, FatigueProfile,
    MyelinationProfile, compute_delay_and_attenuation,
};
use crate::config::{
    FpaaConfig, FpaaKernelRoute, FpaaRoutingConfig, FpaaStartupMode, FpaaTransportPreference,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpaaKernel {
    SynapticFilter,
    ShortTermPlasticity,
    AdaptiveThresholdHomeostasis,
    ActiveDendrite,
    GapJunctionField,
    MorphologyTransmission,
    TripletScalingDaleHybrid,
}

impl FpaaKernel {
    pub const ALL: [Self; 7] = [
        Self::SynapticFilter,
        Self::ShortTermPlasticity,
        Self::AdaptiveThresholdHomeostasis,
        Self::ActiveDendrite,
        Self::GapJunctionField,
        Self::MorphologyTransmission,
        Self::TripletScalingDaleHybrid,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::SynapticFilter => "synaptic_filter",
            Self::ShortTermPlasticity => "short_term_plasticity",
            Self::AdaptiveThresholdHomeostasis => "adaptive_threshold_homeostasis",
            Self::ActiveDendrite => "active_dendrite",
            Self::GapJunctionField => "gap_junction_field",
            Self::MorphologyTransmission => "morphology_transmission",
            Self::TripletScalingDaleHybrid => "triplet_scaling_dale_hybrid",
        }
    }

    #[cfg(feature = "ui")]
    pub fn label(self) -> &'static str {
        match self {
            Self::SynapticFilter => "Synaptic Filter",
            Self::ShortTermPlasticity => "Short-Term Plasticity",
            Self::AdaptiveThresholdHomeostasis => "Adaptive Threshold + Homeostasis",
            Self::ActiveDendrite => "Active Dendrite",
            Self::GapJunctionField => "Gap Junction + Field",
            Self::MorphologyTransmission => "Morphology Transmission",
            Self::TripletScalingDaleHybrid => "Triplet / Scaling / Dale Hybrid",
        }
    }

    pub fn manifest_file(self) -> &'static str {
        match self {
            Self::SynapticFilter => "01_synaptic_filter.okika.json",
            Self::ShortTermPlasticity => "02_short_term_plasticity.okika.json",
            Self::AdaptiveThresholdHomeostasis => "03_adaptive_threshold_homeostasis.okika.json",
            Self::ActiveDendrite => "04_active_dendrite.okika.json",
            Self::GapJunctionField => "05_gap_junction_field.okika.json",
            Self::MorphologyTransmission => "06_morphology_transmission.okika.json",
            Self::TripletScalingDaleHybrid => "07_triplet_scaling_dale_hybrid.okika.json",
        }
    }

    pub fn route(self, routing: &FpaaRoutingConfig) -> FpaaKernelRoute {
        match self {
            Self::SynapticFilter => routing.synaptic_filter,
            Self::ShortTermPlasticity => routing.short_term_plasticity,
            Self::AdaptiveThresholdHomeostasis => routing.adaptive_threshold_homeostasis,
            Self::ActiveDendrite => routing.active_dendrite,
            Self::GapJunctionField => routing.gap_junction_field,
            Self::MorphologyTransmission => routing.morphology_transmission,
            Self::TripletScalingDaleHybrid => routing.triplet_scaling_dale_hybrid,
        }
    }

    pub fn route_mut<'a>(self, routing: &'a mut FpaaRoutingConfig) -> &'a mut FpaaKernelRoute {
        match self {
            Self::SynapticFilter => &mut routing.synaptic_filter,
            Self::ShortTermPlasticity => &mut routing.short_term_plasticity,
            Self::AdaptiveThresholdHomeostasis => &mut routing.adaptive_threshold_homeostasis,
            Self::ActiveDendrite => &mut routing.active_dendrite,
            Self::GapJunctionField => &mut routing.gap_junction_field,
            Self::MorphologyTransmission => &mut routing.morphology_transmission,
            Self::TripletScalingDaleHybrid => &mut routing.triplet_scaling_dale_hybrid,
        }
    }

    pub fn parse_id(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "synaptic_filter" | "synaptic-filter" => Some(Self::SynapticFilter),
            "short_term_plasticity" | "short-term-plasticity" | "stp" => {
                Some(Self::ShortTermPlasticity)
            }
            "adaptive_threshold_homeostasis"
            | "adaptive-threshold-homeostasis"
            | "adaptive_threshold" => Some(Self::AdaptiveThresholdHomeostasis),
            "active_dendrite" | "active-dendrite" => Some(Self::ActiveDendrite),
            "gap_junction_field" | "gap-junction-field" | "gap_junction" => {
                Some(Self::GapJunctionField)
            }
            "morphology_transmission" | "morphology-transmission" => {
                Some(Self::MorphologyTransmission)
            }
            "triplet_scaling_dale_hybrid" | "triplet-scaling-dale-hybrid" | "triplet" => {
                Some(Self::TripletScalingDaleHybrid)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpaaTransportKind {
    PiHat,
    Usb,
}

impl FpaaTransportKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::PiHat => "Pi.HAT GPIO/SPI",
            Self::Usb => "USB",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpaaTransportInfo {
    pub kind: FpaaTransportKind,
    pub path: String,
    pub detail: String,
    pub ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpaaKernelVerification {
    Unknown,
    MissingManifest,
    MissingAhf,
    InvalidAhf,
    MissingProgramState,
    FingerprintMismatch,
    Loaded,
}

impl FpaaKernelVerification {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::MissingManifest => "manifest missing",
            Self::MissingAhf => "ahf missing",
            Self::InvalidAhf => "ahf invalid",
            Self::MissingProgramState => "not verified",
            Self::FingerprintMismatch => "image mismatch",
            Self::Loaded => "loaded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpaaSelfTestStatus {
    NotRun,
    Skipped,
    Passed,
    Failed,
}

impl FpaaSelfTestStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotRun => "not run",
            Self::Skipped => "skipped",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FpaaKernelStatus {
    pub kernel: FpaaKernel,
    pub requested_route: FpaaKernelRoute,
    pub effective_route: FpaaKernelRoute,
    pub manifest_path: String,
    pub ahf_path: Option<String>,
    pub manifest_present: bool,
    pub ahf_present: bool,
    pub verification: FpaaKernelVerification,
    pub sample_test: FpaaSelfTestStatus,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FpaaRuntimeStatus {
    pub startup_mode: FpaaStartupMode,
    pub transport_preference: FpaaTransportPreference,
    pub detected_transport: Option<FpaaTransportInfo>,
    pub available: bool,
    pub ready: bool,
    pub state_file_path: String,
    pub state_file_present: bool,
    pub startup_error: Option<String>,
    pub kernels: Vec<FpaaKernelStatus>,
    pub summary: String,
}

impl FpaaRuntimeStatus {
    #[cfg(feature = "ui")]
    pub fn effective_route(&self, kernel: FpaaKernel) -> FpaaKernelRoute {
        self.kernels
            .iter()
            .find(|status| status.kernel == kernel)
            .map(|status| status.effective_route)
            .unwrap_or(FpaaKernelRoute::Software)
    }

    pub fn unmet_requirement(&self) -> Option<String> {
        if self.startup_mode == FpaaStartupMode::Required && !self.ready {
            return Some(
                self.startup_error.clone().unwrap_or_else(|| {
                    "FPAA required but no ready transport was found".to_string()
                }),
            );
        }
        let unmet: Vec<&str> = self
            .kernels
            .iter()
            .filter(|status| {
                status.requested_route == FpaaKernelRoute::Fpaa
                    && status.effective_route != FpaaKernelRoute::Fpaa
            })
            .map(|status| status.kernel.id())
            .collect();
        if self.startup_mode == FpaaStartupMode::Required && !unmet.is_empty() {
            return Some(format!(
                "FPAA required but these requested kernels are not verified for hardware offload: {}",
                unmet.join(", ")
            ));
        }
        None
    }
}

#[derive(Debug, Deserialize)]
struct OkikaManifest {
    id: String,
    okika_design: OkikaDesign,
}

#[derive(Debug, Deserialize)]
struct OkikaDesign {
    expected_ahf: String,
}

#[derive(Clone, Debug, Deserialize)]
struct PersistedProgramState {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    transport: String,
    #[serde(default)]
    loaded_kernels: Vec<PersistedLoadedKernel>,
}

#[derive(Clone, Debug, Deserialize)]
struct PersistedLoadedKernel {
    kernel_id: String,
    #[serde(default)]
    ahf_fingerprint: String,
}

impl PersistedProgramState {
    fn schema_supported(&self) -> bool {
        matches!(self.schema_version, 0 | 1)
    }

    fn transport_matches(&self, transport: Option<&FpaaTransportInfo>) -> bool {
        let recorded = self.transport.trim().to_ascii_lowercase();
        if recorded.is_empty() {
            return true;
        }
        let Some(transport) = transport else {
            return false;
        };
        let expected_kind = if recorded.contains("pihat")
            || recorded.contains("gpio")
            || recorded.contains("spi")
        {
            Some(FpaaTransportKind::PiHat)
        } else if recorded.contains("usb")
            || recorded.contains("tty")
            || recorded.contains("serial")
        {
            Some(FpaaTransportKind::Usb)
        } else {
            None
        };
        expected_kind
            .map(|kind| kind == transport.kind)
            .unwrap_or(true)
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn ahf_fingerprint(bytes: &[u8]) -> String {
    format!("fnv1a64:{:016x}:{}", fnv1a64(bytes), bytes.len())
}

fn parse_ahf_file(path: &Path) -> Result<Vec<u8>, String> {
    let raw =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let token = line.trim();
        if token.is_empty() {
            continue;
        }
        if token.len() > 2 {
            return Err(format!(
                "{}:{} invalid hex byte {:?}",
                path.display(),
                index + 1,
                token
            ));
        }
        let byte = u8::from_str_radix(token, 16).map_err(|_| {
            format!(
                "{}:{} invalid hex byte {:?}",
                path.display(),
                index + 1,
                token
            )
        })?;
        bytes.push(byte);
    }
    if bytes.is_empty() {
        return Err(format!("{} contained no bytes", path.display()));
    }
    Ok(bytes)
}

fn validate_ahf_file(path: &Path) -> Result<String, String> {
    let bytes = parse_ahf_file(path)?;
    if bytes.len() < 12 {
        return Err(format!(
            "{} too short for a primary AHF header",
            path.display()
        ));
    }
    if bytes.iter().take(5).any(|&b| b != 0) {
        return Err(format!(
            "{} did not start with five sync-zero bytes",
            path.display()
        ));
    }
    if bytes[5] != 0xD5 {
        return Err(format!(
            "{} did not contain 0xD5 sync marker at byte 6",
            path.display()
        ));
    }
    Ok(ahf_fingerprint(&bytes))
}

fn read_trimmed(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let trimmed = value.trim_matches(char::from(0)).trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn detect_pihat(config: &FpaaConfig) -> Option<FpaaTransportInfo> {
    let spi_path = PathBuf::from(&config.spi_device);
    if !spi_path.exists() {
        return None;
    }
    let gpio_present = Path::new("/dev/gpiomem").exists() || Path::new("/sys/class/gpio").exists();
    let mut detail_parts = Vec::new();
    if let Some(product) = read_trimmed(Path::new("/proc/device-tree/hat/product"))
        .or_else(|| read_trimmed(Path::new("/sys/firmware/devicetree/base/hat/product")))
    {
        detail_parts.push(product);
    }
    if detail_parts.is_empty() {
        detail_parts.push("spidev present".to_string());
    }
    Some(FpaaTransportInfo {
        kind: FpaaTransportKind::PiHat,
        path: spi_path.display().to_string(),
        detail: detail_parts.join(" | "),
        ready: gpio_present,
    })
}

fn usb_matches_hint(path: &Path, hint: &str) -> bool {
    if hint.trim().is_empty() {
        return false;
    }
    let hint = hint.trim().to_ascii_lowercase();
    let path_str = path.display().to_string().to_ascii_lowercase();
    path_str.contains(&hint)
}

fn detect_usb(config: &FpaaConfig) -> Option<FpaaTransportInfo> {
    let mut candidates = Vec::new();
    for pattern in ["/dev/ttyUSB", "/dev/ttyACM"] {
        for idx in 0..16 {
            let path = PathBuf::from(format!("{}{}", pattern, idx));
            if path.exists() {
                candidates.push(path);
            }
        }
    }
    let serial_by_id = Path::new("/dev/serial/by-id");
    if let Ok(entries) = fs::read_dir(serial_by_id) {
        for entry in entries.flatten() {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    candidates.dedup();

    let hint = config.usb_device_hint.trim().to_ascii_lowercase();
    if !hint.is_empty() {
        if let Some(path) = candidates.iter().find(|path| usb_matches_hint(path, &hint)) {
            return Some(FpaaTransportInfo {
                kind: FpaaTransportKind::Usb,
                path: path.display().to_string(),
                detail: format!("matched usb hint '{}'", config.usb_device_hint),
                ready: true,
            });
        }
    }

    let keywords = ["okika", "fpaa", "anadigm", "pika"];
    for path in candidates {
        let path_lower = path.display().to_string().to_ascii_lowercase();
        if keywords.iter().any(|keyword| path_lower.contains(keyword)) {
            return Some(FpaaTransportInfo {
                kind: FpaaTransportKind::Usb,
                path: path.display().to_string(),
                detail: "matched usb device path".to_string(),
                ready: true,
            });
        }
        if let Some(file_name) = path.file_name().and_then(|value| value.to_str()) {
            let sysfs = Path::new("/sys/class/tty").join(file_name).join("device");
            let manufacturer = read_trimmed(&sysfs.join("../manufacturer")).unwrap_or_default();
            let product = read_trimmed(&sysfs.join("../product")).unwrap_or_default();
            let joined = format!("{} {}", manufacturer, product).to_ascii_lowercase();
            if keywords.iter().any(|keyword| joined.contains(keyword)) {
                return Some(FpaaTransportInfo {
                    kind: FpaaTransportKind::Usb,
                    path: path.display().to_string(),
                    detail: format!("{} {}", manufacturer, product).trim().to_string(),
                    ready: true,
                });
            }
        }
    }
    None
}

fn detect_transport(config: &FpaaConfig) -> Option<FpaaTransportInfo> {
    let order: &[FpaaTransportPreference] = match config.transport_preference {
        FpaaTransportPreference::Auto => {
            &[FpaaTransportPreference::PiHat, FpaaTransportPreference::Usb]
        }
        FpaaTransportPreference::PiHat => &[FpaaTransportPreference::PiHat],
        FpaaTransportPreference::Usb => &[FpaaTransportPreference::Usb],
    };
    for preference in order {
        let transport = match preference {
            FpaaTransportPreference::Auto => None,
            FpaaTransportPreference::PiHat => detect_pihat(config),
            FpaaTransportPreference::Usb => detect_usb(config),
        };
        if transport.is_some() {
            return transport;
        }
    }
    None
}

fn load_program_state(path: &Path) -> Option<PersistedProgramState> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn loaded_fingerprint_for_kernel<'a>(
    state: &'a PersistedProgramState,
    kernel: FpaaKernel,
) -> Option<&'a str> {
    state
        .loaded_kernels
        .iter()
        .find(|loaded| loaded.kernel_id == kernel.id())
        .map(|loaded| loaded.ahf_fingerprint.as_str())
}

fn sample_test_adaptive_threshold_homeostasis() -> Result<(), String> {
    let spikes = [0.0, 1.0, 0.0, 0.0, 1.0, 0.0];
    let dt_ms = 1.0f64;
    let thr_decay = (-(dt_ms / 200.0f64)).exp();
    let homeo_decay = (-(dt_ms / 2000.0f64)).exp();
    let base_target = 3.0 * dt_ms / 1000.0;
    let mut thr_state = 0.0f64;
    let mut rate_state = 0.0f64;
    for spike in spikes {
        thr_state *= thr_decay;
        rate_state *= homeo_decay;
        if spike != 0.0 {
            thr_state = (thr_state + 0.5).clamp(-2.0, 5.0);
            rate_state += 1.0 - homeo_decay;
        }
        let err = rate_state - base_target;
        thr_state = (thr_state + 0.25 * err).clamp(-2.0, 5.0);
    }
    if thr_state <= 0.0 {
        return Err("adaptive threshold did not rise after spikes".to_string());
    }
    if rate_state <= 0.0 {
        return Err("rate EMA did not accumulate spikes".to_string());
    }
    Ok(())
}

fn append_note(note: &mut Option<String>, extra: impl Into<String>) {
    let extra = extra.into();
    match note {
        Some(existing) if !existing.is_empty() => {
            existing.push_str(" | ");
            existing.push_str(&extra);
        }
        _ => {
            *note = Some(extra);
        }
    }
}

fn run_kernel_sample_test(kernel: FpaaKernel) -> Result<(), String> {
    match kernel {
        FpaaKernel::SynapticFilter => {
            let raw = arr1(&[0.0, 1.0, 0.5, -0.25]);
            let vmem = arr1(&[-65.0, -45.0, -55.0, -60.0]);
            let mut ampa = Array1::zeros(raw.len());
            let mut nmda = Array1::zeros(raw.len());
            let mut gaba = Array1::zeros(raw.len());
            let out = apply_synaptic_filter(
                &raw,
                &mut ampa,
                &mut nmda,
                &mut gaba,
                Some(&vmem),
                0.04,
                |_| SynapticDriveParams {
                    nmda_ratio: 0.25,
                    synaptic_gain: 1.0,
                    decay_ampa: 0.9,
                    decay_nmda: 0.99,
                    decay_gaba: 0.95,
                    neuromod_excitability_gain: 1.0,
                },
            );
            if !out.iter().all(|value| value.is_finite()) {
                return Err("non-finite synaptic filter output".to_string());
            }
            if out[1] <= 0.0 {
                return Err("excitatory drive did not produce positive output".to_string());
            }
            if out[3] >= 0.0 {
                return Err("inhibitory drive did not produce negative output".to_string());
            }
            let out_decay = apply_synaptic_filter(
                &Array1::zeros(raw.len()),
                &mut ampa,
                &mut nmda,
                &mut gaba,
                Some(&vmem),
                0.04,
                |_| SynapticDriveParams {
                    nmda_ratio: 0.25,
                    synaptic_gain: 1.0,
                    decay_ampa: 0.9,
                    decay_nmda: 0.99,
                    decay_gaba: 0.95,
                    neuromod_excitability_gain: 1.0,
                },
            );
            if out_decay[1] <= 0.0 || out_decay[1] >= out[1] {
                return Err("synaptic filter decay state did not evolve plausibly".to_string());
            }
            Ok(())
        }
        FpaaKernel::ShortTermPlasticity => {
            let mut state = ShortTermPlasticityState {
                utilization: 0.8,
                available_resources: 1.0,
            };
            let params = ShortTermPlasticityParams {
                baseline_utilization: 0.2,
                recovery_decay: 0.99,
                facilitation_decay: 0.9,
            };
            let released_first = stp_step(&mut state, true, params);
            let released_second = stp_step(&mut state, true, params);
            if released_first <= 0.0 {
                return Err("STP release was not positive on spike".to_string());
            }
            if released_second >= released_first {
                return Err("STP did not show resource depletion on repeated spikes".to_string());
            }
            let resources_after_second = state.available_resources;
            for _ in 0..200 {
                let _ = stp_step(&mut state, false, params);
            }
            if state.available_resources <= resources_after_second {
                return Err("STP resources did not recover during quiet period".to_string());
            }
            let released_recovered = stp_step(&mut state, true, params);
            if !released_recovered.is_finite() || released_recovered <= 0.0 {
                return Err("STP recovery release became invalid".to_string());
            }
            Ok(())
        }
        FpaaKernel::AdaptiveThresholdHomeostasis => sample_test_adaptive_threshold_homeostasis(),
        FpaaKernel::ActiveDendrite => {
            let mut curr = 1.2;
            let mut ca_state = 0.0;
            let mut plateau = 0.0;
            apply_active_dendritic_compartment(
                &mut curr,
                &mut ca_state,
                &mut plateau,
                1.0,
                ActiveDendriteSpec {
                    enabled: true,
                    calcium_tau_ms: 120.0,
                    plateau_tau_ms: 350.0,
                    calcium_influx_gain: 1.0,
                    plateau_threshold: 0.0,
                    plateau_gain: 1.0,
                },
                DendriteStructureSignal {
                    local_stimulus: 1.0,
                    branching_gain: 2.0,
                },
            );
            if curr <= 1.2 {
                return Err("active dendrite did not boost excitatory current".to_string());
            }
            let mut disabled_curr = 1.2;
            let mut disabled_ca = 0.0;
            let mut disabled_plateau = 0.0;
            apply_active_dendritic_compartment(
                &mut disabled_curr,
                &mut disabled_ca,
                &mut disabled_plateau,
                1.0,
                ActiveDendriteSpec {
                    enabled: false,
                    calcium_tau_ms: 120.0,
                    plateau_tau_ms: 350.0,
                    calcium_influx_gain: 1.0,
                    plateau_threshold: 0.0,
                    plateau_gain: 1.0,
                },
                DendriteStructureSignal {
                    local_stimulus: 1.0,
                    branching_gain: 2.0,
                },
            );
            if (disabled_curr - 1.2).abs() > 1.0e-9 {
                return Err("disabled active dendrite path should not modify current".to_string());
            }
            Ok(())
        }
        FpaaKernel::GapJunctionField => {
            let mut curr = Array1::zeros(3);
            let v = arr1(&[-40.0, -60.0, -70.0]);
            let coupled =
                apply_local_gap_junction_coupling(&mut curr, &v, 1.0, 0.2, None, |idx| match idx {
                    0 => SpatialPoint3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    1 => SpatialPoint3 {
                        x: 0.05,
                        y: 0.0,
                        z: 0.0,
                    },
                    _ => SpatialPoint3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                });
            if !coupled || curr[1].abs() <= 0.0 {
                return Err("gap junction coupling did not affect nearby node".to_string());
            }
            if curr[2].abs() > 1.0e-9 {
                return Err(
                    "gap junction coupling affected a node outside local radius".to_string()
                );
            }
            if curr.sum().abs() > 1.0e-9 {
                return Err("gap junction coupling did not preserve charge balance".to_string());
            }
            let factors = volume_transmission_factors_for_layer(
                2,
                0.2,
                0.4,
                1.5,
                &[SpatialPoint3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }],
                |idx| match idx {
                    0 => SpatialPoint3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    _ => SpatialPoint3 {
                        x: 0.1,
                        y: 0.0,
                        z: 0.0,
                    },
                },
            );
            if factors[0] <= 1.0 {
                return Err(
                    "volume transmission factor did not exceed baseline near source".to_string(),
                );
            }
            if factors[1] <= 1.0 || factors[1] >= factors[0] {
                return Err("volume transmission field did not decay with distance".to_string());
            }
            Ok(())
        }
        FpaaKernel::MorphologyTransmission => {
            let base_spec = DelayAttenuationSpec {
                depth: 3,
                dt_ms: 1.0,
                time_seed: 0,
                synapse_index: 0,
                axon_steps: 10,
                dendrite_steps: 5,
                bouton_latency_steps: 1,
                jitter_ms: 0.0,
                attenuation_per_unit: 0.15,
                axon_length: 1.0,
                dendrite_length: 1.0,
                path_length_scale: 1.0,
                dendritic_profile: Some(DendriticTransmissionProfile {
                    compartment: CompartmentClass::Apical,
                    trunk_length: 1.0,
                    forward_gain: 0.85,
                    backprop_gain: 1.25,
                    is_backward_path: false,
                }),
                myelination: None,
                fatigue: None,
            };
            let slow = compute_delay_and_attenuation(DelayAttenuationSpec {
                myelination: Some(MyelinationProfile {
                    level: 0.0,
                    min_gain: 1.0,
                    max_gain: 3.0,
                }),
                ..base_spec
            });
            let fast = compute_delay_and_attenuation(DelayAttenuationSpec {
                myelination: Some(MyelinationProfile {
                    level: 1.0,
                    min_gain: 1.0,
                    max_gain: 3.0,
                }),
                ..base_spec
            });
            if fast.steps >= slow.steps {
                return Err("myelination did not reduce delay".to_string());
            }
            if fast.attenuation < slow.attenuation {
                return Err("myelination unexpectedly reduced attenuation".to_string());
            }
            let fatigued = compute_delay_and_attenuation(DelayAttenuationSpec {
                myelination: Some(MyelinationProfile {
                    level: 1.0,
                    min_gain: 1.0,
                    max_gain: 3.0,
                }),
                fatigue: Some(FatigueProfile {
                    axon_atp: 0.2,
                    dendrite_atp: 0.2,
                }),
                ..base_spec
            });
            if fatigued.steps <= fast.steps {
                return Err("fatigue did not increase transmission delay".to_string());
            }
            Ok(())
        }
        FpaaKernel::TripletScalingDaleHybrid => {
            let eta = triplet_eta_scale(0.4, 0.5, 0.1, 0.25, 0.15);
            if eta <= 1.0 {
                return Err(
                    "triplet eta scale did not potentiate under positive correlation".to_string(),
                );
            }
            let mut weights: Array2<f64> =
                Array2::from_shape_vec((2, 3), vec![0.3, -0.2, 0.1, 0.5, 0.2, -0.4])
                    .map_err(|e| e.to_string())?;
            let pre_row_err: f64 = weights
                .rows()
                .into_iter()
                .map(|row| (row.iter().map(|w| (*w).abs()).sum::<f64>() - 1.0).abs())
                .sum();
            apply_synaptic_scaling_matrix_rows(&mut weights, 0.2, 1.0);
            enforce_dale_matrix_cols_with_mask(&mut weights, &[false, true, false], 1.0, 1.0);
            if weights[(0, 1)] > 0.0 || weights[(1, 0)] < 0.0 {
                return Err("Dale enforcement did not produce consistent column signs".to_string());
            }
            let post_row_err: f64 = weights
                .rows()
                .into_iter()
                .map(|row| (row.iter().map(|w| (*w).abs()).sum::<f64>() - 1.0).abs())
                .sum();
            if post_row_err >= pre_row_err {
                return Err("synaptic scaling did not move row norms toward target".to_string());
            }
            Ok(())
        }
    }
}

pub fn startup_probe(config: &FpaaConfig) -> FpaaRuntimeStatus {
    let state_path = PathBuf::from(&config.runtime_state_path);
    if config.startup_mode == FpaaStartupMode::Disabled {
        return FpaaRuntimeStatus {
            startup_mode: config.startup_mode,
            transport_preference: config.transport_preference,
            detected_transport: None,
            available: false,
            ready: false,
            state_file_path: state_path.display().to_string(),
            state_file_present: state_path.exists(),
            startup_error: None,
            kernels: FpaaKernel::ALL
                .into_iter()
                .map(|kernel| FpaaKernelStatus {
                    kernel,
                    requested_route: kernel.route(&config.routing),
                    effective_route: FpaaKernelRoute::Software,
                    manifest_path: Path::new(&config.manifest_root)
                        .join(kernel.manifest_file())
                        .display()
                        .to_string(),
                    ahf_path: None,
                    manifest_present: false,
                    ahf_present: false,
                    verification: FpaaKernelVerification::Unknown,
                    sample_test: FpaaSelfTestStatus::Skipped,
                    note: "FPAA startup mode disabled".to_string(),
                })
                .collect(),
            summary: "FPAA disabled; all kernels use software".to_string(),
        };
    }

    let detected_transport = detect_transport(config);
    let available = detected_transport.is_some();
    let ready = detected_transport
        .as_ref()
        .map(|transport| transport.ready)
        .unwrap_or(false);
    let program_state = load_program_state(&state_path);
    let state_file_present = program_state.is_some();
    let state_schema_supported = program_state
        .as_ref()
        .map(PersistedProgramState::schema_supported)
        .unwrap_or(true);
    let state_schema_note = program_state.as_ref().and_then(|state| {
        (!state.schema_supported()).then(|| {
            format!(
                "runtime state schema {} is unsupported by this build",
                state.schema_version
            )
        })
    });
    let state_transport_matches = program_state
        .as_ref()
        .map(|state| state.transport_matches(detected_transport.as_ref()))
        .unwrap_or(true);
    let state_transport_note = program_state.as_ref().and_then(|state| {
        if state.transport_matches(detected_transport.as_ref()) {
            None
        } else if let Some(transport) = detected_transport.as_ref() {
            Some(format!(
                "runtime state transport '{}' does not match detected {}",
                state.transport,
                transport.kind.label()
            ))
        } else {
            Some(format!(
                "runtime state transport '{}' is recorded but no FPAA transport is currently detected",
                state.transport
            ))
        }
    });

    let manifest_root = Path::new(&config.manifest_root);
    let mut fpaa_effective = 0usize;
    let mut kernels = Vec::new();
    for kernel in FpaaKernel::ALL {
        let requested_route = kernel.route(&config.routing);
        let manifest_path = manifest_root.join(kernel.manifest_file());
        let mut ahf_path = None;
        let mut manifest_present = false;
        let mut ahf_present = false;
        let mut verification = FpaaKernelVerification::MissingManifest;
        let mut note: Option<String> = None;
        let mut current_fingerprint = None;

        if let Ok(raw) = fs::read_to_string(&manifest_path) {
            match serde_json::from_str::<OkikaManifest>(&raw) {
                Ok(manifest) => {
                    manifest_present = manifest.id == kernel.id();
                    let expected_ahf_path =
                        manifest_path.with_file_name(&manifest.okika_design.expected_ahf);
                    ahf_present = expected_ahf_path.exists();
                    ahf_path = Some(expected_ahf_path.display().to_string());
                    if !manifest_present {
                        verification = FpaaKernelVerification::MissingManifest;
                        note = Some(format!(
                            "manifest id mismatch: expected {}, found {}",
                            kernel.id(),
                            manifest.id
                        ));
                    } else if !ahf_present {
                        verification = FpaaKernelVerification::MissingAhf;
                        note = Some("expected .ahf export is missing".to_string());
                    } else {
                        match validate_ahf_file(&expected_ahf_path) {
                            Ok(fingerprint) => {
                                current_fingerprint = Some(fingerprint.clone());
                                match program_state
                                    .as_ref()
                                    .and_then(|state| loaded_fingerprint_for_kernel(state, kernel))
                                {
                                    Some(_) if !state_schema_supported => {
                                        verification = FpaaKernelVerification::MissingProgramState;
                                        note =
                                            Some(state_schema_note.clone().unwrap_or_else(|| {
                                                "runtime state schema is unsupported".to_string()
                                            }));
                                    }
                                    Some(_) if !state_transport_matches => {
                                        verification = FpaaKernelVerification::MissingProgramState;
                                        note = Some(state_transport_note.clone().unwrap_or_else(|| {
                                            "runtime state transport does not match detected hardware"
                                                .to_string()
                                        }));
                                    }
                                    Some(persisted) if persisted == fingerprint => {
                                        verification = FpaaKernelVerification::Loaded;
                                    }
                                    Some(_) => {
                                        verification = FpaaKernelVerification::FingerprintMismatch;
                                        note = Some("runtime state fingerprint differs from local .ahf export".to_string());
                                    }
                                    None => {
                                        verification = FpaaKernelVerification::MissingProgramState;
                                        note = Some(
                                            "no persisted programming state for this kernel"
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                            Err(err) => {
                                verification = FpaaKernelVerification::InvalidAhf;
                                note = Some(err);
                            }
                        }
                    }
                }
                Err(err) => {
                    verification = FpaaKernelVerification::MissingManifest;
                    note = Some(format!("manifest parse failed: {err}"));
                }
            }
        } else {
            note = Some("manifest file not found".to_string());
        }

        let sample_test =
            if requested_route == FpaaKernelRoute::Fpaa && ready && config.run_self_test_on_startup
            {
                match run_kernel_sample_test(kernel) {
                    Ok(()) => FpaaSelfTestStatus::Passed,
                    Err(err) => {
                        append_note(&mut note, format!("sample test: {err}"));
                        FpaaSelfTestStatus::Failed
                    }
                }
            } else if requested_route == FpaaKernelRoute::Fpaa && available {
                FpaaSelfTestStatus::NotRun
            } else {
                FpaaSelfTestStatus::Skipped
            };

        let verified_for_fpaa = ready
            && verification == FpaaKernelVerification::Loaded
            && sample_test != FpaaSelfTestStatus::Failed;
        let effective_route = if requested_route == FpaaKernelRoute::Fpaa && verified_for_fpaa {
            fpaa_effective += 1;
            FpaaKernelRoute::Fpaa
        } else {
            FpaaKernelRoute::Software
        };

        if requested_route == FpaaKernelRoute::Fpaa
            && current_fingerprint.is_none()
            && note.is_none()
        {
            note = Some(
                "requested FPAA route will fall back to software until a verified image is present"
                    .to_string(),
            );
        }

        kernels.push(FpaaKernelStatus {
            kernel,
            requested_route,
            effective_route,
            manifest_path: manifest_path.display().to_string(),
            ahf_path,
            manifest_present,
            ahf_present,
            verification,
            sample_test,
            note: note.unwrap_or_default(),
        });
    }

    let startup_error = if !available {
        Some("No FPAA transport detected".to_string())
    } else if !ready {
        Some("FPAA transport detected but not ready".to_string())
    } else {
        None
    };

    let summary = if let Some(transport) = detected_transport.as_ref() {
        format!(
            "FPAA {} via {} at {} ({} kernel(s) verified for hardware offload)",
            if transport.ready { "ready" } else { "detected" },
            transport.kind.label(),
            transport.path,
            fpaa_effective
        )
    } else {
        "FPAA not detected; all kernels use software".to_string()
    };

    FpaaRuntimeStatus {
        startup_mode: config.startup_mode,
        transport_preference: config.transport_preference,
        detected_transport,
        available,
        ready,
        state_file_path: state_path.display().to_string(),
        state_file_present,
        startup_error,
        kernels,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_mut_updates_expected_field() {
        let mut routing = FpaaRoutingConfig::default();
        *FpaaKernel::SynapticFilter.route_mut(&mut routing) = FpaaKernelRoute::Fpaa;
        assert_eq!(routing.synaptic_filter, FpaaKernelRoute::Fpaa);
    }

    #[test]
    fn kernel_id_parser_accepts_aliases() {
        assert_eq!(
            FpaaKernel::parse_id("stp"),
            Some(FpaaKernel::ShortTermPlasticity)
        );
        assert_eq!(
            FpaaKernel::parse_id("gap-junction-field"),
            Some(FpaaKernel::GapJunctionField)
        );
    }

    #[test]
    fn software_disabled_mode_short_circuits_detection() {
        let mut cfg = FpaaConfig::default();
        cfg.startup_mode = FpaaStartupMode::Disabled;
        let status = startup_probe(&cfg);
        assert!(!status.ready);
        assert!(
            status
                .kernels
                .iter()
                .all(|kernel| kernel.effective_route == FpaaKernelRoute::Software)
        );
    }

    #[test]
    fn sample_tests_cover_every_kernel() {
        for kernel in FpaaKernel::ALL {
            assert!(run_kernel_sample_test(kernel).is_ok(), "{:?}", kernel);
        }
    }
}
