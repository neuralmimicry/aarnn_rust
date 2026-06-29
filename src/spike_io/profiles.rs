//! Network-specific spike I/O policies.

use serde::{Deserialize, Serialize};

use crate::config::{IzhikevichParams, LIFParams};
use crate::runner::Runner;
use crate::sim::NeuronModel;
use crate::spike_io::encoding::{
    IsiEncoding, PhaseEncoding, RateEncoding, SignalDomain, TemporalEncodingContext, TtfsEncoding,
    isi_encode, multiplex_or, phase_encode, population_decode_average, population_level_encode,
    population_rate_encode_with, population_threshold_encode, rate_encode_with,
    spikes_to_unit_interval, threshold_encode, ttfs_encode,
};

/// Profiles used by the UI/IPC adapters to match network-specific signal semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkIoProfile {
    #[serde(rename = "celegans", alias = "c_elegans")]
    Celegans,
    Drosophila,
    Hexapod,
    Nao,
    /// Larval/adult zebrafish (*Danio rerio*).  Lateral-line channels (indices 0-15)
    /// use spontaneous-discharge rate encoding; all other modalities use standard
    /// vertebrate rate encoding.  Motor output uses smooth graded decoding tuned
    /// for undulatory CPG-driven tail motion.
    #[serde(rename = "zebrafish")]
    ZebraFish,
    Generic,
}

impl NetworkIoProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Celegans => "celegans",
            Self::Drosophila => "drosophila",
            Self::Hexapod => "hexapod",
            Self::Nao => "nao",
            Self::ZebraFish => "zebrafish",
            Self::Generic => "generic",
        }
    }
}

/// Explicit profile selector stored in network/config JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkIoProfileSelector {
    Auto,
    #[serde(rename = "celegans", alias = "c_elegans")]
    Celegans,
    Drosophila,
    Hexapod,
    Nao,
    #[serde(rename = "zebrafish")]
    ZebraFish,
    Generic,
}

impl Default for NetworkIoProfileSelector {
    fn default() -> Self {
        Self::Auto
    }
}

impl NetworkIoProfileSelector {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Celegans => "celegans",
            Self::Drosophila => "drosophila",
            Self::Hexapod => "hexapod",
            Self::Nao => "nao",
            Self::ZebraFish => "zebrafish",
            Self::Generic => "generic",
        }
    }
}

/// Input encoder families that can be selected declaratively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpikeInputEncodingStrategy {
    ProfileDefault,
    Threshold,
    Rate,
    PopulationThreshold,
    PopulationRate,
    PopulationLevel,
    Ttfs,
    Isi,
    Phase,
    Multiplex,
}

impl Default for SpikeInputEncodingStrategy {
    fn default() -> Self {
        Self::ProfileDefault
    }
}

impl SpikeInputEncodingStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProfileDefault => "profile_default",
            Self::Threshold => "threshold",
            Self::Rate => "rate",
            Self::PopulationThreshold => "population_threshold",
            Self::PopulationRate => "population_rate",
            Self::PopulationLevel => "population_level",
            Self::Ttfs => "ttfs",
            Self::Isi => "isi",
            Self::Phase => "phase",
            Self::Multiplex => "multiplex",
        }
    }
}

/// Primitive strategies that can be OR-combined in multiplex mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpikeInputPrimitive {
    Threshold,
    Rate,
    PopulationThreshold,
    PopulationRate,
    PopulationLevel,
    Ttfs,
    Isi,
    Phase,
}

impl SpikeInputPrimitive {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Threshold => "threshold",
            Self::Rate => "rate",
            Self::PopulationThreshold => "population_threshold",
            Self::PopulationRate => "population_rate",
            Self::PopulationLevel => "population_level",
            Self::Ttfs => "ttfs",
            Self::Isi => "isi",
            Self::Phase => "phase",
        }
    }
}

/// Output decoder families that can be selected declaratively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpikeOutputDecodingStrategy {
    ProfileDefault,
    Binary,
    PopulationAverage,
    Graded,
}

impl Default for SpikeOutputDecodingStrategy {
    fn default() -> Self {
        Self::ProfileDefault
    }
}

impl SpikeOutputDecodingStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProfileDefault => "profile_default",
            Self::Binary => "binary",
            Self::PopulationAverage => "population_average",
            Self::Graded => "graded",
        }
    }
}

/// Population-coding parameters used by explicit strategies.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopulationEncodingConfig {
    pub neurons_per_value: usize,
    pub threshold: f32,
}

impl Default for PopulationEncodingConfig {
    fn default() -> Self {
        Self {
            neurons_per_value: 1,
            threshold: 0.5,
        }
    }
}

/// Multiplexed encoding config combining multiple primitive codes into one train.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiplexEncodingConfig {
    pub strategies: Vec<SpikeInputPrimitive>,
}

impl Default for MultiplexEncodingConfig {
    fn default() -> Self {
        Self {
            strategies: vec![SpikeInputPrimitive::Rate, SpikeInputPrimitive::Ttfs],
        }
    }
}

/// Declarative spike I/O policy stored in `NetworkConfig` and JSON payloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpikeIoConfig {
    pub input_domain: SignalDomain,
    pub output_domain: SignalDomain,
    pub profile: NetworkIoProfileSelector,
    pub input_strategy: SpikeInputEncodingStrategy,
    pub output_strategy: SpikeOutputDecodingStrategy,
    pub threshold: f32,
    pub rate: RateEncoding,
    pub population: PopulationEncodingConfig,
    pub ttfs: TtfsEncoding,
    pub isi: IsiEncoding,
    pub phase: PhaseEncoding,
    pub multiplex: MultiplexEncodingConfig,
}

impl Default for SpikeIoConfig {
    fn default() -> Self {
        Self {
            input_domain: SignalDomain::Hybrid,
            output_domain: SignalDomain::Hybrid,
            profile: NetworkIoProfileSelector::Auto,
            input_strategy: SpikeInputEncodingStrategy::ProfileDefault,
            output_strategy: SpikeOutputDecodingStrategy::ProfileDefault,
            threshold: 0.5,
            rate: RateEncoding::default(),
            population: PopulationEncodingConfig::default(),
            ttfs: TtfsEncoding::default(),
            isi: IsiEncoding::default(),
            phase: PhaseEncoding::default(),
            multiplex: MultiplexEncodingConfig::default(),
        }
    }
}

/// Tunables for profile-specific input encoding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileInputEncoding {
    pub default_threshold: f32,
    pub drosophila_rate: RateEncoding,
    pub hexapod_rate: RateEncoding,
    pub nao_rate: RateEncoding,
    /// Zebrafish lateral-line channels (indices 0-15): neuromast hair cells have a
    /// non-zero resting discharge even in still water.  `quiet_floor` encodes that
    /// spontaneous rate (~6%), `silence_threshold = -1.0` ensures every channel is
    /// evaluated rather than skipped at zero input.
    pub zebrafish_lateral_line_rate: RateEncoding,
    /// Zebrafish non-lateral-line modalities: visual (tectum), olfactory, flow,
    /// and inertial channels (indices 16-31).  Lower spontaneous floor; moderate
    /// gain reflecting vertebrate receptor range-to-spike transfer functions.
    pub zebrafish_rate: RateEncoding,
}

impl Default for ProfileInputEncoding {
    fn default() -> Self {
        Self {
            default_threshold: 0.5,
            drosophila_rate: RateEncoding {
                low_gain: 0.34,
                quiet_floor: 0.002,
                ..RateEncoding::default()
            },
            hexapod_rate: RateEncoding {
                low_gain: 0.24,
                quiet_floor: 0.0015,
                ..RateEncoding::default()
            },
            nao_rate: RateEncoding {
                low_gain: 0.18,
                quiet_floor: 0.001,
                ..RateEncoding::default()
            },
            // Lateral line: high quiet_floor models spontaneous hair-cell discharge;
            // silence_threshold = -1 means even zero-input channels are sampled so
            // background oscillations reach the network.
            zebrafish_lateral_line_rate: RateEncoding {
                quiet_floor: 0.062,
                quiet_floor_boost: 1.0,  // don't double the floor — 6% IS the resting rate
                low_gain: 0.32,
                high_value_threshold: 0.5,
                high_value_bias: 0.78,
                high_value_scale: 0.22,
                max_low_probability: 0.96,
                max_probability: 1.0,
                hard_fire_threshold: 0.999,
                silence_threshold: -1.0,  // never skip — spontaneous discharge at rest
            },
            // Non-LL modalities: standard vertebrate rate, low resting floor.
            zebrafish_rate: RateEncoding {
                quiet_floor: 0.003,
                quiet_floor_boost: 1.8,
                low_gain: 0.28,
                high_value_threshold: 0.5,
                high_value_bias: 0.82,
                high_value_scale: 0.18,
                max_low_probability: 0.95,
                max_probability: 1.0,
                hard_fire_threshold: 0.999,
                silence_threshold: 0.0,
            },
        }
    }
}

/// Tunables for profile-specific output decoding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileOutputEncoding {
    pub celegans_graded_output: bool,
    pub non_celegans_graded_output: bool,
    pub celegans_output_gain: f32,
    pub celegans_output_current_gain: f32,
    pub celegans_output_current_mix: f32,
    pub drosophila_output_gain: f32,
    pub drosophila_output_current_gain: f32,
    pub drosophila_output_current_mix: f32,
    pub hexapod_output_gain: f32,
    pub hexapod_output_current_gain: f32,
    pub hexapod_output_current_mix: f32,
    pub nao_output_gain: f32,
    pub nao_output_current_gain: f32,
    pub nao_output_current_mix: f32,
    /// Zebrafish motor output.  Tuned for CPG-driven undulatory tail-beating:
    /// - Moderate membrane gain (smooth sinusoidal curvature, not binary on/off).
    /// - Moderate current contribution (captures sub-threshold premotor drive).
    /// - Low mix ratio keeps membrane-potential grading dominant over current.
    pub zebrafish_output_gain: f32,
    pub zebrafish_output_current_gain: f32,
    pub zebrafish_output_current_mix: f32,
}

impl Default for ProfileOutputEncoding {
    fn default() -> Self {
        Self {
            celegans_graded_output: true,
            non_celegans_graded_output: true,
            celegans_output_gain: 0.95,
            celegans_output_current_gain: 0.35,
            celegans_output_current_mix: 0.35,
            drosophila_output_gain: 0.82,
            drosophila_output_current_gain: 0.22,
            drosophila_output_current_mix: 0.18,
            hexapod_output_gain: 0.88,
            hexapod_output_current_gain: 0.28,
            hexapod_output_current_mix: 0.24,
            nao_output_gain: 0.92,
            nao_output_current_gain: 0.42,
            nao_output_current_mix: 0.38,
            zebrafish_output_gain: 0.80,
            zebrafish_output_current_gain: 0.28,
            zebrafish_output_current_mix: 0.22,
        }
    }
}

/// Infer the most likely IO policy from network dimensions.
pub fn classify_network_io_profile(sensory_count: usize, output_count: usize) -> NetworkIoProfile {
    if sensory_count == 24 && output_count == 96 {
        return NetworkIoProfile::Celegans;
    }
    if output_count == 48 && (64..=4096).contains(&sensory_count) {
        return NetworkIoProfile::Drosophila;
    }
    if output_count == 18 && (24..=2048).contains(&sensory_count) {
        return NetworkIoProfile::Hexapod;
    }
    if output_count == 40 && sensory_count >= 1024 {
        return NetworkIoProfile::Nao;
    }
    // Zebrafish: 32 sensory (lateral line + visual + olfactory + IMU) × 32 motor
    // (8 tail segments × 2 sides + fins + jaw).
    if output_count == 32 && sensory_count == 32 {
        return NetworkIoProfile::ZebraFish;
    }
    NetworkIoProfile::Generic
}

/// Resolve the runtime I/O profile from an explicit selector, falling back to the
/// legacy dimension heuristic only when `Auto` is requested.
pub fn resolve_network_io_profile(
    selection: NetworkIoProfileSelector,
    sensory_count: usize,
    output_count: usize,
) -> NetworkIoProfile {
    match selection {
        NetworkIoProfileSelector::Auto => classify_network_io_profile(sensory_count, output_count),
        NetworkIoProfileSelector::Celegans => NetworkIoProfile::Celegans,
        NetworkIoProfileSelector::Drosophila => NetworkIoProfile::Drosophila,
        NetworkIoProfileSelector::Hexapod => NetworkIoProfile::Hexapod,
        NetworkIoProfileSelector::Nao => NetworkIoProfile::Nao,
        NetworkIoProfileSelector::ZebraFish => NetworkIoProfile::ZebraFish,
        NetworkIoProfileSelector::Generic => NetworkIoProfile::Generic,
    }
}

fn encode_primitive_inputs_with<F>(
    strategy: SpikeInputPrimitive,
    inputs: &[f32],
    dst: &mut [i8],
    sample: &mut F,
    ctx: TemporalEncodingContext,
    cfg: &SpikeIoConfig,
) where
    F: FnMut() -> f32,
{
    match strategy {
        SpikeInputPrimitive::Threshold => {
            threshold_encode(inputs, dst, cfg.threshold);
        }
        SpikeInputPrimitive::Rate => {
            rate_encode_with(inputs, dst, sample, cfg.rate);
        }
        SpikeInputPrimitive::PopulationThreshold => {
            population_threshold_encode(
                inputs,
                dst,
                cfg.population.neurons_per_value,
                cfg.population.threshold,
            );
        }
        SpikeInputPrimitive::PopulationRate => {
            population_rate_encode_with(inputs, dst, cfg.population.neurons_per_value, sample);
        }
        SpikeInputPrimitive::PopulationLevel => {
            population_level_encode(inputs, dst, cfg.population.neurons_per_value);
        }
        SpikeInputPrimitive::Ttfs => {
            ttfs_encode(inputs, dst, ctx, cfg.ttfs);
        }
        SpikeInputPrimitive::Isi => {
            isi_encode(inputs, dst, ctx, cfg.isi);
        }
        SpikeInputPrimitive::Phase => {
            phase_encode(inputs, dst, ctx, cfg.phase);
        }
    }
}

/// Encode external inputs for a specific network profile.
pub fn encode_profile_inputs_with<F>(
    profile: NetworkIoProfile,
    inputs: &[f32],
    dst: &mut [i8],
    mut sample: F,
    cfg: &ProfileInputEncoding,
) where
    F: FnMut() -> f32,
{
    dst.fill(0);
    match profile {
        NetworkIoProfile::Celegans => {
            let all_quiet = inputs.iter().all(|v| v.is_finite() && *v <= 1.0e-3);
            for i in 0..dst.len().min(inputs.len()) {
                let value = if inputs[i].is_finite() {
                    inputs[i].clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let base_probability = if all_quiet { 0.08 } else { 0.01 };
                let probability = if value >= 0.5 {
                    (0.30 + 0.70 * value).min(1.0)
                } else if value > 0.0 {
                    0.02 + 0.22 * value
                } else {
                    0.0
                }
                .max(base_probability);
                if probability > 0.0 && sample() < probability {
                    dst[i] = 1;
                }
            }
        }
        NetworkIoProfile::Drosophila => {
            rate_encode_with(inputs, dst, &mut sample, cfg.drosophila_rate);
        }
        NetworkIoProfile::Hexapod => {
            rate_encode_with(inputs, dst, &mut sample, cfg.hexapod_rate);
        }
        NetworkIoProfile::Nao => {
            rate_encode_with(inputs, dst, &mut sample, cfg.nao_rate);
        }
        NetworkIoProfile::ZebraFish => {
            // Zone 1 – lateral line (channels 0-15): neuromast hair cells fire
            // spontaneously even in still water.  Use a rate config with a non-zero
            // quiet_floor and silence_threshold = -1 so every channel is sampled.
            let ll_end = 16.min(dst.len());
            if ll_end > 0 {
                rate_encode_with(
                    &inputs[..ll_end.min(inputs.len())],
                    &mut dst[..ll_end],
                    &mut sample,
                    cfg.zebrafish_lateral_line_rate,
                );
            }
            // Zone 2 – visual, olfactory, flow, inertial (channels 16-31): standard
            // vertebrate rate encoding with a low resting floor.
            if ll_end < dst.len() {
                let rest_start = ll_end.min(inputs.len());
                rate_encode_with(
                    &inputs[rest_start..],
                    &mut dst[ll_end..],
                    &mut sample,
                    cfg.zebrafish_rate,
                );
            }
        }
        NetworkIoProfile::Generic => {
            for i in 0..dst.len().min(inputs.len()) {
                if inputs[i].is_finite() && inputs[i] >= cfg.default_threshold {
                    dst[i] = 1;
                }
            }
        }
    }
}

/// Encode external inputs using `fastrand`.
pub fn encode_profile_inputs(
    profile: NetworkIoProfile,
    inputs: &[f32],
    dst: &mut [i8],
    cfg: &ProfileInputEncoding,
) {
    encode_profile_inputs_with(profile, inputs, dst, fastrand::f32, cfg);
}

/// Encode external inputs using the declarative `SpikeIoConfig`.
pub fn encode_network_inputs_with<F>(
    io_cfg: &SpikeIoConfig,
    sensory_count: usize,
    output_count: usize,
    inputs: &[f32],
    dst: &mut [i8],
    mut sample: F,
    ctx: TemporalEncodingContext,
) where
    F: FnMut() -> f32,
{
    let profile = resolve_network_io_profile(io_cfg.profile, sensory_count, output_count);
    match io_cfg.input_strategy {
        SpikeInputEncodingStrategy::ProfileDefault => {
            let cfg = ProfileInputEncoding {
                default_threshold: io_cfg.threshold.clamp(0.0, 1.0),
                ..ProfileInputEncoding::default()
            };
            encode_profile_inputs_with(profile, inputs, dst, &mut sample, &cfg);
        }
        SpikeInputEncodingStrategy::Threshold => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::Threshold,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::Rate => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::Rate,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::PopulationThreshold => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::PopulationThreshold,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::PopulationRate => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::PopulationRate,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::PopulationLevel => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::PopulationLevel,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::Ttfs => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::Ttfs,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::Isi => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::Isi,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::Phase => {
            encode_primitive_inputs_with(
                SpikeInputPrimitive::Phase,
                inputs,
                dst,
                &mut sample,
                ctx,
                io_cfg,
            );
        }
        SpikeInputEncodingStrategy::Multiplex => {
            let strategies = if io_cfg.multiplex.strategies.is_empty() {
                &[SpikeInputPrimitive::Rate, SpikeInputPrimitive::Ttfs][..]
            } else {
                io_cfg.multiplex.strategies.as_slice()
            };
            let mut trains: Vec<Vec<i8>> = Vec::with_capacity(strategies.len());
            for strategy in strategies {
                let mut train = vec![0i8; dst.len()];
                encode_primitive_inputs_with(
                    *strategy,
                    inputs,
                    &mut train,
                    &mut sample,
                    ctx,
                    io_cfg,
                );
                trains.push(train);
            }
            let views: Vec<&[i8]> = trains.iter().map(Vec::as_slice).collect();
            multiplex_or(dst, &views);
        }
    }
}

/// Encode external inputs using the declarative config and `fastrand`.
pub fn encode_network_inputs(
    io_cfg: &SpikeIoConfig,
    sensory_count: usize,
    output_count: usize,
    inputs: &[f32],
    dst: &mut [i8],
    ctx: TemporalEncodingContext,
) {
    encode_network_inputs_with(
        io_cfg,
        sensory_count,
        output_count,
        inputs,
        dst,
        fastrand::f32,
        ctx,
    );
}

fn izh_membrane_to_unit(v: f64, p: IzhikevichParams, gain: f32) -> f32 {
    let span = (p.v_th - p.membrane_reset_potential_c).abs().max(1.0);
    let centered = ((v - p.membrane_reset_potential_c) / span) as f32;
    if !centered.is_finite() {
        return 0.5;
    }
    (0.5 + 0.5 * (gain * centered).tanh()).clamp(0.0, 1.0)
}

fn membrane_to_unit(
    v: f64,
    neuron_model: &NeuronModel,
    lif: &LIFParams,
    aarnn_izh_preset: &str,
    gain: f32,
) -> f32 {
    if !v.is_finite() {
        return 0.5;
    }
    match neuron_model {
        NeuronModel::Izh(p) => izh_membrane_to_unit(v, *p, gain),
        NeuronModel::Aarnn => {
            let p = IzhikevichParams::from_preset(aarnn_izh_preset, lif.dt);
            izh_membrane_to_unit(v, p, gain)
        }
        NeuronModel::Lif => (0.5 + 0.5 * (gain * v as f32).tanh()).clamp(0.0, 1.0),
    }
}

/// Decode output spikes directly to unit-interval actuator values.
pub fn copy_spike_outputs_to_unit(runner: &Runner, dst: &mut [f32]) {
    if let Some(spikes) = runner.last_spk_o.as_slice() {
        spikes_to_unit_interval(spikes, dst);
    } else {
        let spikes: Vec<i8> = runner.last_spk_o.iter().copied().collect();
        spikes_to_unit_interval(&spikes, dst);
    }
}

fn fill_graded_outputs(
    runner: &Runner,
    dst: &mut [f32],
    membrane_gain: f32,
    current_gain: f32,
    current_mix: f32,
) {
    let spike_slice = runner.last_spk_o.as_slice();
    let mix = current_mix.clamp(0.0, 1.0);
    let count = dst.len().min(runner.v_o.len());
    for i in 0..count {
        let membrane = membrane_to_unit(
            runner.v_o[i],
            &runner.neuron_model,
            &runner.lif,
            &runner.net.aarnn_bio.izh_preset,
            membrane_gain,
        );
        #[cfg(any(feature = "ui", feature = "growth3d"))]
        let current_drive = runner
            .last_i_o
            .as_ref()
            .and_then(|currents| currents.get(i).copied());
        #[cfg(not(any(feature = "ui", feature = "growth3d")))]
        let current_drive: Option<f64> = None;

        let blended = if let Some(current_drive) = current_drive {
            let current_drive = if !current_drive.is_finite() {
                0.5
            } else {
                (0.5 + 0.5 * (current_gain * current_drive as f32).tanh()).clamp(0.0, 1.0)
            };
            ((1.0 - mix) * membrane + mix * current_drive).clamp(0.0, 1.0)
        } else {
            membrane
        };
        let spike_gate = spike_slice
            .and_then(|s| s.get(i))
            .copied()
            .or_else(|| runner.last_spk_o.get(i).copied())
            .map(|spk| if spk != 0 { 1.0 } else { 0.0 })
            .unwrap_or(0.0);
        dst[i] = blended.max(spike_gate);
    }
}

/// Decode runner outputs using the network profile and profile-specific gains.
pub fn decode_profile_outputs(
    profile: NetworkIoProfile,
    runner: &Runner,
    dst: &mut [f32],
    cfg: &ProfileOutputEncoding,
) {
    match profile {
        NetworkIoProfile::Celegans if cfg.celegans_graded_output => fill_graded_outputs(
            runner,
            dst,
            cfg.celegans_output_gain,
            cfg.celegans_output_current_gain,
            cfg.celegans_output_current_mix,
        ),
        NetworkIoProfile::Drosophila if cfg.non_celegans_graded_output => fill_graded_outputs(
            runner,
            dst,
            cfg.drosophila_output_gain,
            cfg.drosophila_output_current_gain,
            cfg.drosophila_output_current_mix,
        ),
        NetworkIoProfile::Hexapod if cfg.non_celegans_graded_output => fill_graded_outputs(
            runner,
            dst,
            cfg.hexapod_output_gain,
            cfg.hexapod_output_current_gain,
            cfg.hexapod_output_current_mix,
        ),
        NetworkIoProfile::Nao if cfg.non_celegans_graded_output => fill_graded_outputs(
            runner,
            dst,
            cfg.nao_output_gain,
            cfg.nao_output_current_gain,
            cfg.nao_output_current_mix,
        ),
        NetworkIoProfile::ZebraFish if cfg.non_celegans_graded_output => fill_graded_outputs(
            runner,
            dst,
            cfg.zebrafish_output_gain,
            cfg.zebrafish_output_current_gain,
            cfg.zebrafish_output_current_mix,
        ),
        _ => copy_spike_outputs_to_unit(runner, dst),
    }
}

/// Decode runner outputs using the declarative `SpikeIoConfig`.
pub fn decode_network_outputs(io_cfg: &SpikeIoConfig, runner: &Runner, dst: &mut [f32]) {
    let profile = resolve_network_io_profile(
        io_cfg.profile,
        runner.net.num_sensory_neurons,
        runner.net.num_output_neurons,
    );
    match io_cfg.output_strategy {
        SpikeOutputDecodingStrategy::ProfileDefault => {
            decode_profile_outputs(profile, runner, dst, &ProfileOutputEncoding::default());
        }
        SpikeOutputDecodingStrategy::Binary => {
            copy_spike_outputs_to_unit(runner, dst);
        }
        SpikeOutputDecodingStrategy::PopulationAverage => {
            let neurons_per_value = io_cfg.population.neurons_per_value.max(1);
            if let Some(spikes) = runner.last_spk_o.as_slice() {
                population_decode_average(spikes, dst, neurons_per_value);
            } else {
                let spikes: Vec<i8> = runner.last_spk_o.iter().copied().collect();
                population_decode_average(&spikes, dst, neurons_per_value);
            }
        }
        SpikeOutputDecodingStrategy::Graded => {
            let cfg = ProfileOutputEncoding::default();
            match profile {
                NetworkIoProfile::Celegans => fill_graded_outputs(
                    runner,
                    dst,
                    cfg.celegans_output_gain,
                    cfg.celegans_output_current_gain,
                    cfg.celegans_output_current_mix,
                ),
                NetworkIoProfile::Drosophila => fill_graded_outputs(
                    runner,
                    dst,
                    cfg.drosophila_output_gain,
                    cfg.drosophila_output_current_gain,
                    cfg.drosophila_output_current_mix,
                ),
                NetworkIoProfile::Hexapod => fill_graded_outputs(
                    runner,
                    dst,
                    cfg.hexapod_output_gain,
                    cfg.hexapod_output_current_gain,
                    cfg.hexapod_output_current_mix,
                ),
                NetworkIoProfile::Nao => fill_graded_outputs(
                    runner,
                    dst,
                    cfg.nao_output_gain,
                    cfg.nao_output_current_gain,
                    cfg.nao_output_current_mix,
                ),
                NetworkIoProfile::ZebraFish => fill_graded_outputs(
                    runner,
                    dst,
                    cfg.zebrafish_output_gain,
                    cfg.zebrafish_output_current_gain,
                    cfg.zebrafish_output_current_mix,
                ),
                NetworkIoProfile::Generic => fill_graded_outputs(runner, dst, 1.0, 0.25, 0.0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_classification_matches_known_layouts() {
        assert_eq!(
            classify_network_io_profile(24, 96),
            NetworkIoProfile::Celegans
        );
        assert_eq!(classify_network_io_profile(1024, 40), NetworkIoProfile::Nao);
        assert_eq!(
            classify_network_io_profile(128, 48),
            NetworkIoProfile::Drosophila
        );
        assert_eq!(
            classify_network_io_profile(30, 18),
            NetworkIoProfile::Hexapod
        );
        assert_eq!(
            classify_network_io_profile(32, 32),
            NetworkIoProfile::ZebraFish
        );
        assert_eq!(
            classify_network_io_profile(16, 8),
            NetworkIoProfile::Generic
        );
    }

    #[test]
    fn generic_profile_thresholds_inputs() {
        let cfg = ProfileInputEncoding {
            default_threshold: 0.5,
            ..ProfileInputEncoding::default()
        };
        let mut spikes = vec![0i8; 3];
        encode_profile_inputs_with(
            NetworkIoProfile::Generic,
            &[0.2, 0.5, 0.9],
            &mut spikes,
            || 0.0,
            &cfg,
        );
        assert_eq!(spikes, vec![0, 1, 1]);
    }

    #[test]
    fn explicit_profile_selector_bypasses_dimension_heuristic() {
        assert_eq!(
            resolve_network_io_profile(NetworkIoProfileSelector::Celegans, 16, 8),
            NetworkIoProfile::Celegans
        );
        assert_eq!(
            resolve_network_io_profile(NetworkIoProfileSelector::Generic, 24, 96),
            NetworkIoProfile::Generic
        );
        assert_eq!(
            resolve_network_io_profile(NetworkIoProfileSelector::Hexapod, 8, 3),
            NetworkIoProfile::Hexapod
        );
    }

    #[test]
    fn celegans_profile_spelling_is_backward_compatible() {
        let legacy_selector: NetworkIoProfileSelector =
            serde_json::from_str("\"c_elegans\"").unwrap();
        let canonical_selector: NetworkIoProfileSelector =
            serde_json::from_str("\"celegans\"").unwrap();
        let legacy_profile: NetworkIoProfile = serde_json::from_str("\"c_elegans\"").unwrap();
        let canonical_profile: NetworkIoProfile = serde_json::from_str("\"celegans\"").unwrap();

        assert_eq!(legacy_selector, NetworkIoProfileSelector::Celegans);
        assert_eq!(canonical_selector, NetworkIoProfileSelector::Celegans);
        assert_eq!(legacy_profile, NetworkIoProfile::Celegans);
        assert_eq!(canonical_profile, NetworkIoProfile::Celegans);
        assert_eq!(
            serde_json::to_string(&NetworkIoProfileSelector::Celegans).unwrap(),
            "\"celegans\""
        );
        assert_eq!(
            serde_json::to_string(&NetworkIoProfile::Celegans).unwrap(),
            "\"celegans\""
        );
    }

    #[test]
    fn hexapod_profile_serialization_roundtrip() {
        let selector: NetworkIoProfileSelector = serde_json::from_str("\"hexapod\"").unwrap();
        let profile: NetworkIoProfile = serde_json::from_str("\"hexapod\"").unwrap();
        assert_eq!(selector, NetworkIoProfileSelector::Hexapod);
        assert_eq!(profile, NetworkIoProfile::Hexapod);
        assert_eq!(
            serde_json::to_string(&NetworkIoProfileSelector::Hexapod).unwrap(),
            "\"hexapod\""
        );
        assert_eq!(
            serde_json::to_string(&NetworkIoProfile::Hexapod).unwrap(),
            "\"hexapod\""
        );
    }

    #[test]
    fn zebrafish_profile_serialization_roundtrip() {
        let selector: NetworkIoProfileSelector =
            serde_json::from_str("\"zebrafish\"").unwrap();
        let profile: NetworkIoProfile = serde_json::from_str("\"zebrafish\"").unwrap();
        assert_eq!(selector, NetworkIoProfileSelector::ZebraFish);
        assert_eq!(profile, NetworkIoProfile::ZebraFish);
        assert_eq!(
            serde_json::to_string(&NetworkIoProfileSelector::ZebraFish).unwrap(),
            "\"zebrafish\""
        );
        assert_eq!(
            serde_json::to_string(&NetworkIoProfile::ZebraFish).unwrap(),
            "\"zebrafish\""
        );
    }

    #[test]
    fn zebrafish_lateral_line_fires_spontaneously_at_rest() {
        // All 16 lateral-line inputs at zero should still produce some spikes
        // (resting discharge), because silence_threshold = -1.0 means the floor
        // probability is always sampled.
        let cfg = ProfileInputEncoding::default();
        let inputs = vec![0.0f32; 32];
        let mut spike_counts = vec![0u32; 100];
        for count in spike_counts.iter_mut() {
            let mut spikes = vec![0i8; 32];
            // Use a deterministic "random" that always returns 0.05 (< quiet_floor=0.062)
            // so lateral-line channels always fire and others mostly don't.
            encode_profile_inputs_with(
                NetworkIoProfile::ZebraFish,
                &inputs,
                &mut spikes,
                || 0.05,
                &cfg,
            );
            *count = spikes[..16].iter().map(|&s| s as u32).sum();
        }
        // Every lateral-line channel fires (sample 0.05 < quiet_floor 0.062),
        // so sum per call should equal 16.
        assert!(
            spike_counts.iter().all(|&c| c == 16),
            "lateral line channels should all fire when sample < quiet_floor"
        );
        // Non-LL channels should NOT fire (sample 0.05 > typical low probability
        // at input=0.0 for standard zebrafish_rate with quiet_floor=0.003).
        let mut non_ll = vec![0i8; 32];
        encode_profile_inputs_with(
            NetworkIoProfile::ZebraFish,
            &inputs,
            &mut non_ll,
            || 0.05,
            &cfg,
        );
        let non_ll_fire: u32 = non_ll[16..].iter().map(|&s| s as u32).sum();
        assert_eq!(
            non_ll_fire, 0,
            "non-LL channels should not fire at rest when sample > quiet_floor*boost"
        );
    }

    #[test]
    fn zebrafish_explicit_selector_bypasses_dimension_heuristic() {
        assert_eq!(
            resolve_network_io_profile(NetworkIoProfileSelector::ZebraFish, 16, 8),
            NetworkIoProfile::ZebraFish
        );
        // Dimensions that don't match any profile still map correctly via selector.
        assert_eq!(
            resolve_network_io_profile(NetworkIoProfileSelector::ZebraFish, 100, 50),
            NetworkIoProfile::ZebraFish
        );
    }

    #[test]
    fn ttfs_strategy_is_available_through_declarative_config() {
        let mut spikes = vec![0i8; 1];
        let cfg = SpikeIoConfig {
            profile: NetworkIoProfileSelector::Generic,
            input_strategy: SpikeInputEncodingStrategy::Ttfs,
            ttfs: TtfsEncoding {
                threshold: 0.0,
                window_steps: 4,
            },
            ..SpikeIoConfig::default()
        };
        encode_network_inputs_with(
            &cfg,
            1,
            1,
            &[1.0],
            &mut spikes,
            || 0.0,
            TemporalEncodingContext {
                step_index: 0,
                ..Default::default()
            },
        );
        assert_eq!(spikes, vec![1]);
    }

    #[test]
    fn multiplex_strategy_combines_multiple_primitives() {
        let mut spikes = vec![0i8; 2];
        let cfg = SpikeIoConfig {
            profile: NetworkIoProfileSelector::Generic,
            input_strategy: SpikeInputEncodingStrategy::Multiplex,
            threshold: 0.8,
            ttfs: TtfsEncoding {
                threshold: 0.0,
                window_steps: 4,
            },
            multiplex: MultiplexEncodingConfig {
                strategies: vec![SpikeInputPrimitive::Threshold, SpikeInputPrimitive::Ttfs],
            },
            ..SpikeIoConfig::default()
        };
        encode_network_inputs_with(
            &cfg,
            2,
            2,
            &[0.4, 0.9],
            &mut spikes,
            || 0.0,
            TemporalEncodingContext {
                step_index: 2,
                ..Default::default()
            },
        );
        assert_eq!(spikes, vec![1, 1]);
    }
}
