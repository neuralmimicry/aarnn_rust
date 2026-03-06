//! Global configuration types for the simulator, UI Runner, and optional AARNN/morphology features.
//!
//! This module defines parameter structs used across both batch (matrix‑based) and
//! interactive (Runner/UI) modes. All values are expressed in SI‑like units unless
//! noted, with time in milliseconds and positions/lengths in normalized scene units
//! (roughly in the range [−1, +1]).
//!
//! Notes
//! - When the `growth3d` feature is enabled, some fields gate dynamic topology
//!   growth of hidden layers (spawn, cooldowns, proximity caps, etc.).
//! - When the `morpho` feature is enabled in combination with `growth3d`, the
//!   AARNN path can use per‑segment conduction (axon + bouton + dendrite) with
//!   distances measured from the morphology snapshot.
//! - Batch/CLI mode ignores morphology data and uses the classic matrix path.
// (Removed unused clap::ValueEnum import to silence warning)
use serde::{Deserialize, Serialize};

/// Parameters for a Leaky Integrate-and-Fire (LIF) neuron model.
///
/// The LIF model is a standard simplified model of a biological neuron. It integrates
/// incoming current into a membrane potential, which decays over time towards a reset
/// value. If the potential exceeds a threshold, the neuron fires a spike and enters
/// a refractory period.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LIFParams {
    /// Membrane time constant τ_m (ms). Determines how fast the membrane potential
    /// decays back to the reset potential. A larger τ_m means slower decay.
    pub tau_m: f64,
    /// Reset potential (V_reset). The value to which the membrane potential is set
    /// immediately after a spike occurs.
    pub v_reset: f64,
    /// Firing threshold (V_th). If the membrane potential (V_m) crosses this value,
    /// the neuron generates an action potential (spike).
    pub v_th: f64,
    /// Refractory period duration (in simulation steps). After firing, the neuron
    /// is "paralyzed" and cannot fire again until this period has elapsed.
    pub refractory: usize,
    /// Simulation time step Δt (ms). This is the fundamental unit of time for the
    /// Euler integration of the membrane potential and other time-dependent variables.
    pub dt: f64,
}

impl Default for LIFParams {
    /// Returns default parameters for a typical LIF neuron.
    ///
    /// * `tau_m`: 20.0 ms
    /// * `v_reset`: 0.0
    /// * `v_th`: 1.0
    /// * `refractory`: 5 steps
    /// * `dt`: 1.0 ms
    fn default() -> Self {
        Self {
            tau_m: 20.0,
            v_reset: 0.0,
            v_th: 1.0,
            refractory: 5,
            dt: 1.0,
        }
    }
}

/// Parameters for the Izhikevich neuron model.
///
/// The Izhikevich model is more computationally efficient than Hodgkin-Huxley but
/// more biologically plausible than LIF. It can reproduce many firing patterns
/// seen in cortical neurons (e.g., regular spiking, bursting, chattering).
///
/// Ref: Izhikevich, E. M. (2003). Simple model of spiking neurons.
/// IEEE Transactions on Neural Networks, 14(6), 1569-1572.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IzhikevichParams {
    /// Time scale of the recovery variable 'u' (parameter 'a').
    /// Smaller values result in slower recovery.
    pub recovery_time_constant_a: f64,
    /// Sensitivity of 'u' to the subthreshold fluctuations of membrane potential 'v' (parameter 'b').
    pub recovery_sensitivity_b: f64,
    /// After-spike reset value of membrane potential 'v' (parameter 'c').
    pub membrane_reset_potential_c: f64,
    /// After-spike reset increment of recovery variable 'u' (parameter 'd').
    pub recovery_increment_d: f64,
    /// Firing threshold (V_th). Typically 30 mV in the original model.
    pub v_th: f64,
    /// Simulation time step Δt (ms).
    pub dt: f64,
}

impl IzhikevichParams {
    /// Constructs an Izhikevich model using a named preset.
    ///
    /// Available presets:
    /// - "RS": Regular Spiking
    /// - "FS": Fast Spiking
    /// - "IB": Intrinsically Bursting
    /// - "CH": Chattering
    /// - "LTS": Low-Threshold Spiking
    /// - "RZ": Resonator
    /// - "TC": Thalamo-Cortical
    /// - "P": Persistent
    pub fn from_preset(name: &str, dt: f64) -> Self {
        // Construct a commonly used Izhikevich neuron variant by short code:
        // "RS", "FS", "IB", "CH", "LTS", "RZ", "TC", "P".
        //
        // The returned struct embeds the same `dt` value you pass here so that
        // step integration stays consistent with the Runner.
        let n = name.to_uppercase();
        let (a_val, b_val, c_val, d_val) = match n.as_str() {
            "RS" => (0.02, 0.2, -65.0, 8.0),
            "IB" => (0.02, 0.2, -55.0, 4.0),
            "CH" => (0.02, 0.2, -50.0, 2.0),
            "FS" => (0.1, 0.2, -65.0, 2.0),
            "LTS" => (0.02, 0.25, -65.0, 2.0),
            "RZ" => (0.1, 0.26, -65.0, 2.0),
            "TC" => (0.02, 0.25, -65.0, 0.05),
            "P" => (0.02, 1.0, -60.0, 0.0),
            _ => (0.02, 0.2, -65.0, 8.0),
        };
        Self {
            recovery_time_constant_a: a_val,
            recovery_sensitivity_b: b_val,
            membrane_reset_potential_c: c_val,
            recovery_increment_d: d_val,
            v_th: 30.0,
            dt,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct STDPParams {
    /// Pre‑synaptic trace time constant τ_pre (ms)
    pub tau_pre: f64,
    /// Post‑synaptic trace time constant τ_post (ms)
    pub tau_post: f64,
    /// Learning rate (η). Scales all weight updates.
    pub eta: f64,
    /// Lower clamp for synaptic weights
    pub w_min: f64,
    /// Upper clamp for synaptic weights
    pub w_max: f64,
}

impl Default for STDPParams {
    fn default() -> Self {
        Self {
            tau_pre: 20.0,
            tau_post: 20.0,
            eta: 0.002,
            w_min: 0.0,
            w_max: 1.0,
        }
    }
}

/// Biologically-motivated parameters for AARNN dynamics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AarnnBioParams {
    /// Enable short-term plasticity (STP) on presynaptic spikes.
    pub stp_enabled: bool,
    /// STP utilization (U) applied on each presynaptic spike.
    pub stp_u: f64,
    /// STP recovery time constant τ_rec (ms).
    pub stp_tau_rec_ms: f64,
    /// STP facilitation time constant τ_facil (ms).
    pub stp_tau_facil_ms: f64,
    /// Synaptic filtering: AMPA decay time constant (ms).
    pub ampa_tau_ms: f64,
    /// Synaptic filtering: NMDA decay time constant (ms).
    pub nmda_tau_ms: f64,
    /// Synaptic filtering: GABA decay time constant (ms).
    pub gaba_tau_ms: f64,
    /// Fraction of excitatory drive routed to NMDA (0-1).
    pub nmda_ratio: f64,
    /// Global synaptic gain applied to filtered currents.
    pub synaptic_gain: f64,

    /// Izhikevich preset name (e.g. "RS", "FS", "IB").
    pub izh_preset: String,
    /// Adaptive threshold enabled for AARNN/Izh neurons.
    pub adaptive_threshold_enabled: bool,
    /// Adaptive threshold decay time constant (ms).
    pub adaptive_threshold_tau_ms: f64,
    /// Threshold increment added on spike.
    pub adaptive_threshold_increment: f64,
    /// Clamp for adaptive threshold offset (min).
    pub adaptive_threshold_min: f64,
    /// Clamp for adaptive threshold offset (max).
    pub adaptive_threshold_max: f64,
    /// Additional refractory period for AARNN/Izh neurons (ms).
    pub izh_refractory_ms: f64,
    /// Homeostatic firing rate target (Hz).
    pub homeostasis_target_rate_hz: f64,
    /// Homeostasis decay time constant (ms).
    pub homeostasis_tau_ms: f64,
    /// Homeostasis gain applied to threshold offsets.
    pub homeostasis_gain: f64,
    /// Neuromodulation enabled (affects plasticity/excitability).
    pub neuromodulation_enabled: bool,
    /// Dopaminergic gain multiplier for plasticity.
    pub dopamine_gain: f64,
    /// Acetylcholine gain multiplier for excitability.
    pub acetylcholine_gain: f64,
    /// Serotonin gain multiplier for plasticity damping.
    pub serotonin_gain: f64,
}

/// Selectable signal sources for neuromodulator dynamics (AARNN).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NeuromodSignal {
    None,
    RewardProxy,
    PerceptualError,
    WorldModelError,
    OutputSpikes,
    SensorySpikes,
    HiddenSpikes,
    Stability,
}

impl Default for AarnnBioParams {
    fn default() -> Self {
        Self {
            stp_enabled: true,
            stp_u: 0.2,
            stp_tau_rec_ms: 800.0,
            stp_tau_facil_ms: 200.0,
            ampa_tau_ms: 5.0,
            nmda_tau_ms: 100.0,
            gaba_tau_ms: 10.0,
            nmda_ratio: 0.25,
            synaptic_gain: 1.0,
            izh_preset: "RS".to_string(),
            adaptive_threshold_enabled: true,
            adaptive_threshold_tau_ms: 200.0,
            adaptive_threshold_increment: 0.5,
            adaptive_threshold_min: -2.0,
            adaptive_threshold_max: 5.0,
            izh_refractory_ms: 2.0,
            homeostasis_target_rate_hz: 5.0,
            homeostasis_tau_ms: 2000.0,
            homeostasis_gain: 0.25,
            neuromodulation_enabled: true,
            dopamine_gain: 1.0,
            acetylcholine_gain: 1.0,
            serotonin_gain: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NeuronTypeConfig {
    pub name: String,
    pub bio_params: AarnnBioParams,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum RegionShape {
    Ellipsoid {
        center: [f32; 3],
        radii: [f32; 3],
    },
    #[allow(non_snake_case)]
    Torus {
        center: [f32; 3],
        R: f32,
        r: f32,
        plane: String,
    },
    Tube {
        line_from: [f32; 3],
        line_to: [f32; 3],
        radius: f32,
    },
    RepeatedEllipsoids {
        count: usize,
        center_start: [f32; 3],
        step: [f32; 3],
        radii: [f32; 3],
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BrainRegionConfig {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<RegionShape>,
    // Backward-compatible fields for legacy configs that assume ellipsoids
    #[serde(default)]
    pub center: [f32; 3],
    #[serde(default)]
    pub radii: [f32; 3],
    /// Distribution of neuron types in this region: (type_name, probability)
    /// The f32 is the relative weight in the distribution.
    pub type_distribution: Vec<(String, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClumpingDesign {
    None,
    HumanBrain,
    FruitFly,
    FruitFlyLarva,
    ZebraFish,
    NematodeWorm,
}

impl ClumpingDesign {
    #[allow(dead_code)]
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::HumanBrain => "Human Brain",
            Self::FruitFly => "Fruit Fly (Adult)",
            Self::FruitFlyLarva => "Fruit Fly (Larva)",
            Self::ZebraFish => "Zebra Fish",
            Self::NematodeWorm => "Nematode Worm",
        }
    }
}

fn apply_human_brain_design(cfg: &mut NetworkConfig) {
    cfg.max_total_neurons = 86_000_000_000;
    cfg.brain_regions.push(BrainRegionConfig {
        name: "Left Cortex".to_string(),
        shape: None,
        center: [-35.0, 0.0, 25.0],
        radii: [35.0, 55.0, 30.0],
        type_distribution: vec![
            ("Pyramidal".to_string(), 0.8),
            ("Interneuron".to_string(), 0.2),
        ],
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "Right Cortex".to_string(),
        shape: None,
        center: [35.0, 0.0, 25.0],
        radii: [35.0, 55.0, 30.0],
        type_distribution: vec![
            ("Pyramidal".to_string(), 0.8),
            ("Interneuron".to_string(), 0.2),
        ],
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "Thalamus".to_string(),
        shape: None,
        center: [0.0, -5.0, 10.0],
        radii: [12.0, 10.0, 8.0],
        type_distribution: vec![
            ("Pyramidal".to_string(), 0.85),
            ("Interneuron".to_string(), 0.15),
        ],
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "Hippocampus".to_string(),
        shape: None,
        center: [0.0, -25.0, 5.0],
        radii: [18.0, 8.0, 6.0],
        type_distribution: vec![
            ("Pyramidal".to_string(), 0.9),
            ("Interneuron".to_string(), 0.1),
        ],
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "Cerebellum".to_string(),
        shape: None,
        center: [0.0, -55.0, 0.0],
        radii: [35.0, 20.0, 15.0],
        type_distribution: vec![
            ("Pyramidal".to_string(), 0.2),
            ("Interneuron".to_string(), 0.8),
        ],
    });
}

fn apply_fruit_fly_adult_design(cfg: &mut NetworkConfig) {
    cfg.max_total_neurons = 139_255;

    let optic_types = vec![
        ("sensory_spn".to_string(), 0.55),
        ("local_interneuron".to_string(), 0.30),
        ("projection_pn".to_string(), 0.10),
        ("neuromod".to_string(), 0.05),
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "optic_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-28.0, 0.0, 0.0],
            radii: [14.0, 18.0, 14.0],
        }),
        center: [-28.0, 0.0, 0.0],
        radii: [14.0, 18.0, 14.0],
        type_distribution: optic_types.clone(),
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "optic_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [28.0, 0.0, 0.0],
            radii: [14.0, 18.0, 14.0],
        }),
        center: [28.0, 0.0, 0.0],
        radii: [14.0, 18.0, 14.0],
        type_distribution: optic_types,
    });

    let antennal_types = vec![
        ("local_interneuron".to_string(), 0.45),
        ("projection_pn".to_string(), 0.35),
        ("sensory_spn".to_string(), 0.15),
        ("neuromod".to_string(), 0.05),
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "antennal_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-12.0, 18.0, -2.0],
            radii: [7.0, 6.0, 6.0],
        }),
        center: [-12.0, 18.0, -2.0],
        radii: [7.0, 6.0, 6.0],
        type_distribution: antennal_types.clone(),
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "antennal_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [12.0, 18.0, -2.0],
            radii: [7.0, 6.0, 6.0],
        }),
        center: [12.0, 18.0, -2.0],
        radii: [7.0, 6.0, 6.0],
        type_distribution: antennal_types,
    });

    let lateral_horn_types = vec![
        ("projection_pn".to_string(), 0.55),
        ("local_interneuron".to_string(), 0.30),
        ("neuromod".to_string(), 0.10),
        ("feedback_pn".to_string(), 0.05),
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "lateral_horn_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-14.0, 10.0, 6.0],
            radii: [6.0, 8.0, 6.0],
        }),
        center: [-14.0, 10.0, 6.0],
        radii: [6.0, 8.0, 6.0],
        type_distribution: lateral_horn_types.clone(),
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "lateral_horn_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [14.0, 10.0, 6.0],
            radii: [6.0, 8.0, 6.0],
        }),
        center: [14.0, 10.0, 6.0],
        radii: [6.0, 8.0, 6.0],
        type_distribution: lateral_horn_types,
    });

    let mushroom_body_types = vec![
        ("kenyon_cell".to_string(), 0.70),
        ("mb_input_pn".to_string(), 0.15),
        ("local_interneuron".to_string(), 0.10),
        ("neuromod".to_string(), 0.05),
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "mushroom_body_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-8.0, 5.0, 10.0],
            radii: [6.0, 10.0, 8.0],
        }),
        center: [-8.0, 5.0, 10.0],
        radii: [6.0, 10.0, 8.0],
        type_distribution: mushroom_body_types.clone(),
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "mushroom_body_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [8.0, 5.0, 10.0],
            radii: [6.0, 10.0, 8.0],
        }),
        center: [8.0, 5.0, 10.0],
        radii: [6.0, 10.0, 8.0],
        type_distribution: mushroom_body_types,
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "central_complex".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 0.0, 8.0],
            radii: [10.0, 10.0, 6.0],
        }),
        center: [0.0, 0.0, 8.0],
        radii: [10.0, 10.0, 6.0],
        type_distribution: vec![
            ("local_interneuron".to_string(), 0.45),
            ("projection_pn".to_string(), 0.35),
            ("neuromod".to_string(), 0.10),
            ("descending_dn".to_string(), 0.10),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "SEZ".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 15.0, -10.0],
            radii: [12.0, 10.0, 8.0],
        }),
        center: [0.0, 15.0, -10.0],
        radii: [12.0, 10.0, 8.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.35),
            ("local_interneuron".to_string(), 0.35),
            ("projection_pn".to_string(), 0.15),
            ("descending_dn".to_string(), 0.10),
            ("neuromod".to_string(), 0.05),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "VNC".to_string(),
        shape: Some(RegionShape::Tube {
            line_from: [0.0, -120.0, -8.0],
            line_to: [0.0, -40.0, -8.0],
            radius: 10.0,
        }),
        center: [0.0, -80.0, -8.0],
        radii: [10.0, 40.0, 10.0],
        type_distribution: vec![
            ("motor_premotor".to_string(), 0.45),
            ("local_interneuron".to_string(), 0.25),
            ("sensory_spn".to_string(), 0.20),
            ("descending_target".to_string(), 0.10),
        ],
    });
}

fn apply_fruit_fly_larva_design(cfg: &mut NetworkConfig) {
    cfg.max_total_neurons = 3016;

    let brain_types = vec![
        ("sensory_spn".to_string(), 0.20),
        ("local_interneuron".to_string(), 0.45),
        ("projection_pn".to_string(), 0.20),
        ("neuromod".to_string(), 0.10),
        ("descending_dn".to_string(), 0.05),
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "brain_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-8.0, 28.0, 4.0],
            radii: [7.0, 10.0, 7.0],
        }),
        center: [-8.0, 28.0, 4.0],
        radii: [7.0, 10.0, 7.0],
        type_distribution: brain_types.clone(),
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "brain_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [8.0, 28.0, 4.0],
            radii: [7.0, 10.0, 7.0],
        }),
        center: [8.0, 28.0, 4.0],
        radii: [7.0, 10.0, 7.0],
        type_distribution: brain_types,
    });

    let mb_types = vec![
        ("kenyon_cell".to_string(), 0.75),
        ("mb_input_pn".to_string(), 0.10),
        ("local_interneuron".to_string(), 0.10),
        ("neuromod".to_string(), 0.05),
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "MB_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-6.0, 30.0, 8.0],
            radii: [4.0, 6.0, 4.0],
        }),
        center: [-6.0, 30.0, 8.0],
        radii: [4.0, 6.0, 4.0],
        type_distribution: mb_types.clone(),
    });
    cfg.brain_regions.push(BrainRegionConfig {
        name: "MB_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [6.0, 30.0, 8.0],
            radii: [4.0, 6.0, 4.0],
        }),
        center: [6.0, 30.0, 8.0],
        radii: [4.0, 6.0, 4.0],
        type_distribution: mb_types,
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "SEZ".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 18.0, -4.0],
            radii: [10.0, 8.0, 7.0],
        }),
        center: [0.0, 18.0, -4.0],
        radii: [10.0, 8.0, 7.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.35),
            ("local_interneuron".to_string(), 0.35),
            ("projection_pn".to_string(), 0.15),
            ("descending_dn".to_string(), 0.10),
            ("neuromod".to_string(), 0.05),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "VNC".to_string(),
        shape: Some(RegionShape::Tube {
            line_from: [0.0, -110.0, -3.0],
            line_to: [0.0, 10.0, -3.0],
            radius: 8.0,
        }),
        center: [0.0, -50.0, -3.0],
        radii: [8.0, 60.0, 8.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.20),
            ("local_interneuron".to_string(), 0.30),
            ("motor_premotor".to_string(), 0.40),
            ("relay".to_string(), 0.10),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "segments".to_string(),
        shape: Some(RegionShape::RepeatedEllipsoids {
            count: 10,
            center_start: [0.0, -100.0, -3.0],
            step: [0.0, 12.0, 0.0],
            radii: [6.0, 4.0, 5.0],
        }),
        center: [0.0, -46.0, -3.0],
        radii: [6.0, 54.0, 5.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.15),
            ("local_interneuron".to_string(), 0.35),
            ("motor_premotor".to_string(), 0.45),
            ("neuromod".to_string(), 0.05),
        ],
    });
}

fn apply_zebra_fish_design(cfg: &mut NetworkConfig) {
    cfg.max_total_neurons = 100_000;

    cfg.brain_regions.push(BrainRegionConfig {
        name: "olfactory_bulbs".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 58.0, 0.0],
            radii: [6.0, 4.0, 5.0],
        }),
        center: [0.0, 58.0, 0.0],
        radii: [6.0, 4.0, 5.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.40),
            ("local_interneuron".to_string(), 0.40),
            ("projection_pn".to_string(), 0.15),
            ("neuromod".to_string(), 0.05),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "telencephalon".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 50.0, 4.0],
            radii: [12.0, 10.0, 10.0],
        }),
        center: [0.0, 50.0, 4.0],
        radii: [12.0, 10.0, 10.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.20),
            ("local_interneuron".to_string(), 0.45),
            ("projection_pn".to_string(), 0.25),
            ("neuromod".to_string(), 0.10),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "diencephalon".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 40.0, 0.0],
            radii: [12.0, 10.0, 10.0],
        }),
        center: [0.0, 40.0, 0.0],
        radii: [12.0, 10.0, 10.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.15),
            ("local_interneuron".to_string(), 0.35),
            ("projection_pn".to_string(), 0.30),
            ("neuromod".to_string(), 0.20),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "pretectum".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 34.0, 2.0],
            radii: [8.0, 6.0, 6.0],
        }),
        center: [0.0, 34.0, 2.0],
        radii: [8.0, 6.0, 6.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.25),
            ("local_interneuron".to_string(), 0.45),
            ("projection_pn".to_string(), 0.20),
            ("command_neuron".to_string(), 0.10),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "tectum_L".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [-10.0, 26.0, 8.0],
            radii: [10.0, 10.0, 8.0],
        }),
        center: [-10.0, 26.0, 8.0],
        radii: [10.0, 10.0, 8.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.45),
            ("local_interneuron".to_string(), 0.35),
            ("projection_pn".to_string(), 0.15),
            ("command_neuron".to_string(), 0.05),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "tectum_R".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [10.0, 26.0, 8.0],
            radii: [10.0, 10.0, 8.0],
        }),
        center: [10.0, 26.0, 8.0],
        radii: [10.0, 10.0, 8.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.45),
            ("local_interneuron".to_string(), 0.35),
            ("projection_pn".to_string(), 0.15),
            ("command_neuron".to_string(), 0.05),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "cerebellum".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 18.0, 10.0],
            radii: [12.0, 8.0, 8.0],
        }),
        center: [0.0, 18.0, 10.0],
        radii: [12.0, 8.0, 8.0],
        type_distribution: vec![
            ("granule_like".to_string(), 0.55),
            ("purkinje_like".to_string(), 0.05),
            ("local_interneuron".to_string(), 0.30),
            ("projection_pn".to_string(), 0.10),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "hindbrain".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 8.0, 0.0],
            radii: [14.0, 12.0, 12.0],
        }),
        center: [0.0, 8.0, 0.0],
        radii: [14.0, 12.0, 12.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.15),
            ("local_interneuron".to_string(), 0.35),
            ("projection_pn".to_string(), 0.25),
            ("motor_premotor".to_string(), 0.20),
            ("command_neuron".to_string(), 0.05),
        ],
    });

    cfg.brain_regions.push(BrainRegionConfig {
        name: "spinal_cord".to_string(),
        shape: Some(RegionShape::Tube {
            line_from: [0.0, -200.0, -2.0],
            line_to: [0.0, 0.0, -2.0],
            radius: 8.0,
        }),
        center: [0.0, -100.0, -2.0],
        radii: [8.0, 100.0, 8.0],
        type_distribution: vec![
            ("sensory_spn".to_string(), 0.20),
            ("local_interneuron".to_string(), 0.30),
            ("motor_premotor".to_string(), 0.45),
            ("relay".to_string(), 0.05),
        ],
    });
}

fn apply_nematode_worm_design(cfg: &mut NetworkConfig) {
    cfg.max_total_neurons = 302;
    // Head ganglia (ellipsoid): center (0,7,0), radii (6,8,6)
    cfg.brain_regions.push(BrainRegionConfig {
        name: "head_ganglia".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 7.0, 0.0],
            radii: [6.0, 8.0, 6.0],
        }),
        center: [0.0, 7.0, 0.0],
        radii: [6.0, 8.0, 6.0],
        type_distribution: vec![
            ("Sensory".to_string(), 0.55),
            ("Interneuron".to_string(), 0.30),
            ("Neuromodulatory".to_string(), 0.10),
            ("Motor".to_string(), 0.05),
        ],
    });
    // Nerve ring (torus) around y ≈ 10, major R=6, minor r=1.5 in x–z plane
    cfg.brain_regions.push(BrainRegionConfig {
        name: "nerve_ring".to_string(),
        shape: Some(RegionShape::Torus {
            center: [0.0, 10.0, 0.0],
            R: 6.0,
            r: 1.5,
            plane: "x-z".to_string(),
        }),
        center: [0.0, 10.0, 0.0], // representative center
        radii: [7.5, 1.5, 7.5],   // approximate for legacy ellipsoid logic
        type_distribution: vec![
            ("Sensory".to_string(), 0.15),
            ("Interneuron".to_string(), 0.75),
            ("Neuromodulatory".to_string(), 0.10),
        ],
    });
    // Ventral nerve cord (tube) along y:15–95 at z≈−6, radius 1.2
    let v_from = [0.0, 15.0, -6.0];
    let v_to = [0.0, 95.0, -6.0];
    let v_mid = [
        (v_from[0] + v_to[0]) * 0.5,
        (v_from[1] + v_to[1]) * 0.5,
        (v_from[2] + v_to[2]) * 0.5,
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "ventral_nerve_cord".to_string(),
        shape: Some(RegionShape::Tube {
            line_from: v_from,
            line_to: v_to,
            radius: 1.2,
        }),
        center: v_mid,
        radii: [1.2, ((v_to[1] - v_from[1]).abs()) * 0.5, 1.2],
        type_distribution: vec![
            ("Sensory".to_string(), 0.10),
            ("Interneuron".to_string(), 0.25),
            ("Motor".to_string(), 0.60),
            ("Neuromodulatory".to_string(), 0.05),
        ],
    });
    // Dorsal nerve cord (tube) along y:15–95 at z≈+6, radius 1.0
    let d_from = [0.0, 15.0, 6.0];
    let d_to = [0.0, 95.0, 6.0];
    let d_mid = [
        (d_from[0] + d_to[0]) * 0.5,
        (d_from[1] + d_to[1]) * 0.5,
        (d_from[2] + d_to[2]) * 0.5,
    ];
    cfg.brain_regions.push(BrainRegionConfig {
        name: "dorsal_nerve_cord".to_string(),
        shape: Some(RegionShape::Tube {
            line_from: d_from,
            line_to: d_to,
            radius: 1.0,
        }),
        center: d_mid,
        radii: [1.0, ((d_to[1] - d_from[1]).abs()) * 0.5, 1.0],
        type_distribution: vec![
            ("Motor".to_string(), 0.95),
            ("Interneuron".to_string(), 0.05),
        ],
    });
    // Tail ganglia (ellipsoid) center (0,95,0), radii (5,6,5)
    cfg.brain_regions.push(BrainRegionConfig {
        name: "tail_ganglia".to_string(),
        shape: Some(RegionShape::Ellipsoid {
            center: [0.0, 95.0, 0.0],
            radii: [5.0, 6.0, 5.0],
        }),
        center: [0.0, 95.0, 0.0],
        radii: [5.0, 6.0, 5.0],
        type_distribution: vec![
            ("Sensory".to_string(), 0.60),
            ("Interneuron".to_string(), 0.20),
            ("Motor".to_string(), 0.15),
            ("Neuromodulatory".to_string(), 0.05),
        ],
    });
}

pub fn apply_clumping_design(cfg: &mut NetworkConfig, design: ClumpingDesign) {
    cfg.brain_regions.clear();
    cfg.max_total_neurons = 0; // Default: no limit
    ensure_default_neuron_types(cfg);

    match design {
        ClumpingDesign::None => {}
        ClumpingDesign::HumanBrain => apply_human_brain_design(cfg),
        ClumpingDesign::FruitFly => apply_fruit_fly_adult_design(cfg),
        ClumpingDesign::FruitFlyLarva => apply_fruit_fly_larva_design(cfg),
        ClumpingDesign::ZebraFish => apply_zebra_fish_design(cfg),
        ClumpingDesign::NematodeWorm => apply_nematode_worm_design(cfg),
    }
    cfg.clumping_design = design;
}

/// Apply the baseline AARNN biomimicry profile used by UI defaults:
/// human-brain clumping + core AARNN growth/morphology/delay settings.
pub fn apply_aarnn_human_biomimicry_defaults(cfg: &mut NetworkConfig) {
    cfg.growth_enabled = true;
    cfg.use_morphology = true;
    cfg.morpho_growth_enabled = true;

    cfg.use_aarnn_delays = true;
    cfg.aarnn_layer_depth = 5;
    cfg.aarnn_bio = AarnnBioParams::default();
    cfg.aarnn_bio.stp_enabled = true;
    cfg.aarnn_bio.neuromodulation_enabled = true;

    cfg.aarnn_velocity = 10.0;
    cfg.axon_velocity = 20.0;
    cfg.dend_velocity = 5.0;
    cfg.p_release_default = 0.7;
    cfg.bouton_latency_ms = 0.5;
    cfg.bouton_jitter_ms = 0.1;

    cfg.enforce_unique_geometry = true;
    cfg.use_mid_bends = true;

    cfg.aarnn_synaptic_energy_randomness = 0.1;
    cfg.aarnn_resonance_gain = 0.2;
    cfg.aarnn_resonance_decay = 0.1;
    cfg.aarnn_neuromod_baseline_dopamine = 1.0;
    cfg.aarnn_neuromod_baseline_ach = 1.0;
    cfg.aarnn_neuromod_baseline_serotonin = 1.0;
    cfg.aarnn_neuromod_dopamine_signal = NeuromodSignal::PerceptualError;
    cfg.aarnn_neuromod_ach_signal = NeuromodSignal::SensorySpikes;
    cfg.aarnn_neuromod_serotonin_signal = NeuromodSignal::Stability;
    cfg.aarnn_reward_proxy = 0.0;
    cfg.aarnn_neuromod_decay = 0.05;
    cfg.aarnn_neuromod_error_gain = 0.0;
    cfg.aarnn_neuromod_activity_gain = 0.0;
    cfg.aarnn_neuromod_stability_gain = 0.0;

    cfg.aarnn_inhibitory_fraction = 0.2;
    cfg.aarnn_dale_strictness = 0.75;
    cfg.aarnn_gap_junction_strength = 0.02;
    cfg.aarnn_nmda_voltage_sensitivity = 0.04;
    cfg.aarnn_triplet_ltp_gain = 0.25;
    cfg.aarnn_triplet_ltd_gain = 0.15;
    cfg.aarnn_synaptic_scaling_strength = 0.02;
    cfg.aarnn_synaptic_scaling_target = 1.0;
    cfg.aarnn_distance_attenuation_per_unit = 0.15;
    cfg.aarnn_release_prob_heterogeneity = 0.1;

    apply_clumping_design(cfg, ClumpingDesign::HumanBrain);
}

fn ensure_default_neuron_types(cfg: &mut NetworkConfig) {
    if !cfg.neuron_types.iter().any(|t| t.name == "Pyramidal") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "Pyramidal".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                synaptic_gain: 1.0,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "Interneuron") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "Interneuron".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "FS".to_string(),
                synaptic_gain: 1.0,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "Sensory") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "Sensory".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(), // adapting RS for tonic + bursts
                synaptic_gain: 1.0,
                adaptive_threshold_enabled: true,
                ..AarnnBioParams::default()
            },
        });
    }
    // Fly specific types
    if !cfg.neuron_types.iter().any(|t| t.name == "sensory_spn") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "sensory_spn".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                adaptive_threshold_enabled: true,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg
        .neuron_types
        .iter()
        .any(|t| t.name == "local_interneuron")
    {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "local_interneuron".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "FS".to_string(),
                synaptic_gain: 0.8,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "projection_pn") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "projection_pn".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "kenyon_cell") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "kenyon_cell".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                adaptive_threshold_increment: 1.0, // higher threshold for sparse firing
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "mb_input_pn") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "mb_input_pn".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "feedback_pn") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "feedback_pn".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "neuromod") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "neuromod".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "IB".to_string(),
                neuromodulation_enabled: true,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "motor_premotor") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "motor_premotor".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "CH".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "descending_dn") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "descending_dn".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "IB".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg
        .neuron_types
        .iter()
        .any(|t| t.name == "descending_target")
    {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "descending_target".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "Motor") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "Motor".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "CH".to_string(), // rhythmic/oscillator-like
                synaptic_gain: 1.1,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "Neuromodulatory") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "Neuromodulatory".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "IB".to_string(), // state/persistent activity
                neuromodulation_enabled: true,
                synaptic_gain: 0.9,
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "command_neuron") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "command_neuron".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "IB".to_string(),
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "granule_like") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "granule_like".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "RS".to_string(),
                adaptive_threshold_increment: 1.5, // higher for sparse firing
                ..AarnnBioParams::default()
            },
        });
    }
    if !cfg.neuron_types.iter().any(|t| t.name == "purkinje_like") {
        cfg.neuron_types.push(NeuronTypeConfig {
            name: "purkinje_like".to_string(),
            bio_params: AarnnBioParams {
                izh_preset: "FS".to_string(),
                synaptic_gain: 2.0, // strong influence
                ..AarnnBioParams::default()
            },
        });
    }
}

/// Configuration for the entire neural network and its simulation environment.
///
/// This struct covers everything from layer sizes and connection probabilities to
/// advanced 3D growth and morphological developmental parameters.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NetworkConfig {
    /// Number of sensory (input) neurons. These neurons receive external stimuli.
    pub num_sensory_neurons: usize,
    /// Number of hidden layers in the network. In the classic matrix path, this is fixed.
    /// With `growth3d` enabled, this can be an initial value.
    pub num_hidden_layers: usize,
    /// Initial number of neurons per hidden layer.
    pub num_hidden_per_layer_initial: usize,
    /// Number of output neurons. These neurons provide the network's final response.
    pub num_output_neurons: usize,
    /// Maximum total number of neurons allowed in the network (sensory + hidden + output).
    /// If 0, no limit is enforced.
    pub max_total_neurons: u64,
    /// Probability of a synapse being created between an input neuron and a neuron
    /// in the first hidden layer during initialization.
    pub p_in: f64,
    /// Probability of a synapse being created between neurons in adjacent hidden layers.
    pub p_hidden: f64,
    /// Probability of a synapse being created between a neuron in the last hidden layer
    /// and an output neuron.
    pub p_out: f64,
    /// GA local evaluation stall timeout in seconds.
    pub ga_stall_timeout_secs: u64,

    /// Target hidden layer index for sensory inputs. If None, uses model-specific defaults.
    pub sensory_target_layer: Option<usize>,
    /// Source hidden layer index for output neurons. If None, uses model-specific defaults.
    pub output_source_layer: Option<usize>,

    /// Defined brain regions for multi-area clumping and type allocation.
    pub brain_regions: Vec<BrainRegionConfig>,
    /// Selected clumping design preset.
    pub clumping_design: ClumpingDesign,
    /// Available neuron types and their biological parameters.
    pub neuron_types: Vec<NeuronTypeConfig>,

    // --- Growth (3D topology) Parameters ---
    // These are effective when the project is built with the "growth3d" feature.
    /// Master toggle for dynamic 3D growth in the Runner.
    pub growth_enabled: bool,
    /// Maximum number of hidden layers allowed when growing the network dynamically.
    pub max_layers: usize,
    /// Firing rate threshold (Exponential Moving Average) that triggers a neuron to
    /// "spawn" a new child neuron if it remains saturated.
    pub saturation_threshold: f32,
    /// Time window (ms) for calculating the EMA of the firing rate.
    pub saturation_window_ms: f32,
    /// Cooldown period (ms) for a specific neuron after it has successfully spawned.
    pub growth_cooldown_ms: f32,
    /// Radial distance from the parent neuron within which the new neuron is placed.
    pub spawn_radius: f32,
    /// Probability that a portion of the parent's incoming synaptic weights are
    /// migrated to the new child neuron.
    pub migrate_in_prob: f32,
    /// Probability that a portion of the parent's outgoing synaptic weights are
    /// migrated to the new child neuron.
    pub migrate_out_prob: f32,
    /// Probability of creating additional edges between the new neuron and its neighbors.
    pub new_edge_prob: f32,
    /// Number of neurons in a layer that triggers a potential layer split.
    pub layer_split_threshold: usize,
    /// Global cooldown (ms) to prevent a burst of growth events across the entire network.
    pub global_growth_cooldown_ms: f32,
    /// Maximum number of proximity-biased edges created during a single spawn event.
    pub proximity_degree_cap: usize,

    // --- Morphology & AARNN (Adaptive Axonal-Relay Neural Network) Parameters ---
    /// Toggle for using the detailed morphological data model and AARNN conduction.
    pub use_morphology: bool,
    /// Default signal propagation velocity (units/ms) for axons/dendrites.
    pub aarnn_velocity: f32,
    /// Specific axonal conduction velocity. Falls back to `aarnn_velocity` if ≤ 0.
    pub axon_velocity: f32,
    /// Specific dendritic conduction velocity. Falls back to `aarnn_velocity` if ≤ 0.
    pub dend_velocity: f32,
    /// Baseline probability of neurotransmitter release at a synaptic bouton.
    pub p_release_default: f32,
    /// Whether to simulate discrete time delays based on physical connection lengths.
    pub use_aarnn_delays: bool,
    /// Fixed latency (ms) added at each synaptic bouton (synaptic cleft delay).
    pub bouton_latency_ms: f32,
    /// Magnitude of random jitter (± ms) added to the bouton latency.
    pub bouton_jitter_ms: f32,

    // --- Geometry & Physics Constraints ---
    /// If true, the system ensures neurons and segments do not occupy the same 3D space.
    pub enforce_unique_geometry: bool,
    /// Minimum allowed 3D distance between neurons within the same layer.
    pub min_node_sep: f32,
    /// Minimum allowed 3D distance between segment endpoints within a single neuron.
    pub min_segment_sep: f32,
    /// Distance along the connection vector to offset the synapse site from the soma.
    pub synapse_offset: f32,
    /// Maximum number of attempts to find a valid, non-colliding position for a new node.
    pub max_place_tries: usize,
    /// Number of iterations for the iterative relaxation pass to resolve geometric conflicts.
    pub relax_iters: usize,
    /// Maximum displacement per relaxation iteration.
    pub relax_step: f32,
    /// Minimum 3D distance between any two connection segments to prevent intersection.
    pub seg_eps: f32,
    /// Attempts to reroute or "bend" a path to avoid collisions with other segments.
    pub max_reroute_tries: usize,
    /// If true, use mid-point bends in connections to avoid obstacle occupancy.
    pub use_mid_bends: bool,

    // --- Morphological Development (AARNN-specific) ---
    /// Enable dynamic growth and retraction of axons and dendrites during simulation.
    pub morpho_growth_enabled: bool,
    /// EMA window (ms) for tracking synaptic activity, which drives morphological changes.
    pub synaptic_energy_window_ms: f32,
    /// Radius within which "synaptic energy" from active synapses attracts dendrite growth.
    pub energy_attraction_radius: f32,
    /// Scaling factor for the spatial decay of the energy attraction field.
    pub energy_kernel_k: f32,
    /// Probability of a neuron sprouting a new dendritic branch per simulation step.
    pub dendrite_sprout_prob: f32,
    /// Degree of randomness in the initial synaptic energy levels.
    pub aarnn_synaptic_energy_randomness: f32,
    /// Enable cyclic perceptual loop with prediction and update (AARNN).
    pub perceptual_loop_enabled: bool,
    /// Learning rate for sensory prediction update (0..1).
    pub perceptual_prediction_lr: f32,
    /// Per-step decay applied to prediction state (0..1).
    pub perceptual_prediction_decay: f32,
    /// Threshold for predicted sensory spikes (0..1).
    pub perceptual_prediction_threshold: f32,
    /// Gain applied to prediction error to drive hidden layer 0.
    pub perceptual_error_gain: f32,
    /// Blend factor for output-driven prediction (0..1).
    pub perceptual_feedback_gain: f32,
    /// Enable low-dimensional world-model phase-space state (AARNN).
    pub world_model_enabled: bool,
    /// Dimension of the world-model state vector.
    pub world_model_dim: usize,
    /// EMA decay applied to the world-model state (0..1).
    pub world_model_decay: f32,
    /// Enable sleep/dream cycles (AARNN).
    pub sleep_enabled: bool,
    /// Sleep cycle length in milliseconds.
    pub sleep_cycle_ms: f32,
    /// Sleep duration per cycle in milliseconds.
    pub sleep_duration_ms: f32,
    /// Probability of replaying sensory history during sleep (0..1).
    pub sleep_dream_replay_prob: f32,
    /// Threshold for dream spikes from predictions (0..1).
    pub sleep_dream_threshold: f32,
    /// Consolidation gain applied during sleep (0..1).
    pub sleep_consolidation_gain: f32,
    /// Enable a global theta rhythm drive as a deterministic alternative to random spiking.
    pub theta_rhythm_enabled: bool,
    /// Theta rhythm frequency in Hz.
    pub theta_rhythm_hz: f32,
    /// Fraction of the theta cycle that is "active" (0..1).
    pub theta_rhythm_duty: f32,
    /// Current injected into hidden layer 0 during the active theta phase.
    pub theta_rhythm_drive: f32,
    /// Phase jitter across neurons (0..1), 0 = fully synchronized.
    pub theta_rhythm_phase_jitter: f32,
    /// Enable thalamic gating of sensory inputs (AARNN).
    pub thalamic_gating_enabled: bool,
    /// Thalamic gating frequency in Hz.
    pub thalamic_gate_hz: f32,
    /// Fraction of the gating cycle that is open (0..1).
    pub thalamic_gate_duty: f32,
    /// Minimum pass-through probability during the closed phase (0..1).
    pub thalamic_gate_floor: f32,
    /// Ambient energy level in the environment that influences growth.
    pub aarnn_ambient_energy_level: f32,
    /// Resonance gain for pseudo-spontaneous spiking driven by recent activity.
    pub aarnn_resonance_gain: f32,
    /// EMA decay for resonance state readout (0..1).
    pub aarnn_resonance_decay: f32,
    /// Neuromodulator baseline for dopamine (0..1+).
    pub aarnn_neuromod_baseline_dopamine: f32,
    /// Neuromodulator baseline for acetylcholine (0..1+).
    pub aarnn_neuromod_baseline_ach: f32,
    /// Neuromodulator baseline for serotonin (0..1+).
    pub aarnn_neuromod_baseline_serotonin: f32,
    /// Dopamine signal source for neuromodulation.
    pub aarnn_neuromod_dopamine_signal: NeuromodSignal,
    /// Acetylcholine signal source for neuromodulation.
    pub aarnn_neuromod_ach_signal: NeuromodSignal,
    /// Serotonin signal source for neuromodulation.
    pub aarnn_neuromod_serotonin_signal: NeuromodSignal,
    /// Reward proxy value (0..1) used when RewardProxy is selected (bias added to external reward).
    pub aarnn_reward_proxy: f32,
    /// EMA decay for neuromodulator state (0..1).
    pub aarnn_neuromod_decay: f32,
    /// Gain applied to the selected dopamine signal (0..1+).
    pub aarnn_neuromod_error_gain: f32,
    /// Gain applied to the selected acetylcholine signal (0..1+).
    pub aarnn_neuromod_activity_gain: f32,
    /// Gain applied to the selected serotonin signal (0..1+).
    pub aarnn_neuromod_stability_gain: f32,
    /// Fraction of presynaptic neurons treated as inhibitory for Dale-style sign constraints (0..1).
    pub aarnn_inhibitory_fraction: f32,
    /// Strength of Dale-style sign enforcement on synapses (0 = disabled, 1 = strict).
    pub aarnn_dale_strictness: f32,
    /// Electrical coupling strength between neurons in the same hidden layer (gap-junction-like).
    pub aarnn_gap_junction_strength: f32,
    /// Voltage sensitivity of NMDA gating (0 disables voltage dependence).
    pub aarnn_nmda_voltage_sensitivity: f32,
    /// Additional potentiation gain for triplet-like STDP modulation.
    pub aarnn_triplet_ltp_gain: f32,
    /// Additional depression gain for triplet-like STDP modulation.
    pub aarnn_triplet_ltd_gain: f32,
    /// Strength of per-neuron synaptic scaling after plastic updates.
    pub aarnn_synaptic_scaling_strength: f32,
    /// Target summed absolute incoming synaptic strength used by synaptic scaling.
    pub aarnn_synaptic_scaling_target: f32,
    /// Per-unit-length attenuation factor for morphology-aware transmission.
    pub aarnn_distance_attenuation_per_unit: f32,
    /// Per-synapse release-probability heterogeneity around `p_release_default` (0..1).
    pub aarnn_release_prob_heterogeneity: f32,
    /// Factor by which local activity stabilizes and strengthens a synapse.
    pub synaptic_stabilization_strength: f32,
    /// Maximum distance between a dendrite bouton and an axon for synapse formation.
    pub axon_contact_dist: f32,
    /// Rate at which component activity (and physical size) decays when inactive.
    pub component_decay_rate: f32,
    /// Growth rate of the main dendritic trunk.
    pub trunk_growth_rate: f32,
    /// Growth rate of dendritic branches.
    pub branch_growth_rate: f32,
    /// Growth rate of synaptic boutons.
    pub bouton_growth_rate: f32,
    /// Maximum physical length for a single segment (axon or dendrite).
    pub max_segment_length: f32,
    /// Strength of repulsion forces between different neurons/components.
    pub spatial_repulsion_strength: f32,
    /// Strength of attractive "clumping" force between neurons.
    pub spatial_clumping_strength: f32,
    /// Enable columnar organization forces (AARNN).
    pub columnar_enabled: bool,
    /// Column spacing in lateral plane (y/z).
    pub columnar_spacing: f32,
    /// Column attraction strength.
    pub columnar_strength: f32,
    /// Column center jitter (0..1) as a fraction of spacing.
    pub columnar_jitter: f32,
    /// Target neuron density the network tries to maintain through spatial forces.
    pub density_target: f32,
    /// Proportional gain for the skull-membrane PID controller.
    pub skull_pid_kp: f32,
    /// Integral gain for the skull-membrane PID controller.
    pub skull_pid_ki: f32,
    /// Derivative gain for the skull-membrane PID controller.
    pub skull_pid_kd: f32,
    /// Frequency of spontaneous neuron generation in the hidden layers (ms).
    pub spontaneous_neuron_interval_ms: f32,
    /// Delay (ms) before a "dead" neuron (no connections) is removed from the simulation.
    pub neuron_removal_delay_ms: f32,
    /// Capacity limit for the number of connections a sensory neuron can maintain.
    pub max_sensory_connections: usize,
    /// Capacity limit for the number of connections an output neuron can receive.
    pub max_output_connections: usize,

    // --- AARNN Synaptic Dynamics & Search Optimization ---
    /// Threshold below which a synapse or segment is pruned.
    pub component_pruning_threshold: f32,
    /// Initial weight for newly formed synapses.
    pub initial_synaptic_weight: f64,
    /// Center point for growth vs shrinkage (stimuli - growth_threshold).
    pub synaptic_growth_threshold: f32,
    /// Factor for synaptic consolidation (slows decay for active synapses).
    pub synaptic_consolidation_factor: f32,

    // --- AARNN Multi-scale Detail ---
    /// Depth level for AARNN simulation detail (0 = macro, higher = more micro-detail).
    pub aarnn_layer_depth: usize,
    /// Biologically motivated AARNN parameters (gated by `aarnn_layer_depth`).
    pub aarnn_bio: AarnnBioParams,

    // --- UI Configuration ---
    /// Target frame rate for the visualization engine.
    pub ui_target_fps: f32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        let mut cfg = Self {
            num_sensory_neurons: 0,
            num_hidden_layers: 1,
            num_hidden_per_layer_initial: 1,
            num_output_neurons: 0,
            max_total_neurons: 0,
            p_in: 0.15,
            p_hidden: 0.10,
            p_out: 0.15,
            ga_stall_timeout_secs: 60,
            sensory_target_layer: None,
            output_source_layer: None,
            brain_regions: Vec::new(),
            clumping_design: ClumpingDesign::None,
            neuron_types: Vec::new(),
            growth_enabled: true,
            max_layers: 6,
            saturation_threshold: 0.5,
            saturation_window_ms: 200.0,
            growth_cooldown_ms: 500.0,
            spawn_radius: 0.1,
            migrate_in_prob: 0.5,
            migrate_out_prob: 0.5,
            new_edge_prob: 0.05,
            layer_split_threshold: 32,
            global_growth_cooldown_ms: 150.0,
            proximity_degree_cap: 4,
            use_morphology: true,
            aarnn_velocity: 10.0, // fast default → ~0-1 step delay at unit length
            axon_velocity: 20.0,
            dend_velocity: 5.0,
            p_release_default: 0.7,
            use_aarnn_delays: true,
            bouton_latency_ms: 0.5,
            bouton_jitter_ms: 0.1,
            // Geometry defaults (conservative; normalized scene units ~ [-1,1])
            enforce_unique_geometry: true,
            min_node_sep: 0.02,
            min_segment_sep: 0.01,
            synapse_offset: 0.0125,
            max_place_tries: 16,
            relax_iters: 2,
            relax_step: 0.004,
            // Geometry collision defaults
            seg_eps: 0.0015,
            max_reroute_tries: 6,
            use_mid_bends: true,
            morpho_growth_enabled: true,
            synaptic_energy_window_ms: 1000.0,
            energy_attraction_radius: 0.4,
            energy_kernel_k: 2.0,
            dendrite_sprout_prob: 0.01,
            aarnn_synaptic_energy_randomness: 0.1,
            perceptual_loop_enabled: false,
            perceptual_prediction_lr: 0.05,
            perceptual_prediction_decay: 0.0,
            perceptual_prediction_threshold: 0.5,
            perceptual_error_gain: 5.0,
            perceptual_feedback_gain: 0.5,
            world_model_enabled: false,
            world_model_dim: 8,
            world_model_decay: 0.05,
            sleep_enabled: false,
            sleep_cycle_ms: 60000.0,
            sleep_duration_ms: 5000.0,
            sleep_dream_replay_prob: 0.7,
            sleep_dream_threshold: 0.5,
            sleep_consolidation_gain: 0.5,
            theta_rhythm_enabled: false,
            theta_rhythm_hz: 6.0,
            theta_rhythm_duty: 0.2,
            theta_rhythm_drive: 10.0,
            theta_rhythm_phase_jitter: 0.0,
            thalamic_gating_enabled: false,
            thalamic_gate_hz: 6.0,
            thalamic_gate_duty: 0.3,
            thalamic_gate_floor: 0.1,
            aarnn_ambient_energy_level: 0.05,
            aarnn_resonance_gain: 0.0,
            aarnn_resonance_decay: 0.1,
            aarnn_neuromod_baseline_dopamine: 1.0,
            aarnn_neuromod_baseline_ach: 1.0,
            aarnn_neuromod_baseline_serotonin: 1.0,
            aarnn_neuromod_dopamine_signal: NeuromodSignal::PerceptualError,
            aarnn_neuromod_ach_signal: NeuromodSignal::SensorySpikes,
            aarnn_neuromod_serotonin_signal: NeuromodSignal::Stability,
            aarnn_reward_proxy: 0.0,
            aarnn_neuromod_decay: 0.05,
            aarnn_neuromod_error_gain: 0.0,
            aarnn_neuromod_activity_gain: 0.0,
            aarnn_neuromod_stability_gain: 0.0,
            aarnn_inhibitory_fraction: 0.2,
            aarnn_dale_strictness: 0.0,
            aarnn_gap_junction_strength: 0.0,
            aarnn_nmda_voltage_sensitivity: 0.0,
            aarnn_triplet_ltp_gain: 0.0,
            aarnn_triplet_ltd_gain: 0.0,
            aarnn_synaptic_scaling_strength: 0.0,
            aarnn_synaptic_scaling_target: 1.0,
            aarnn_distance_attenuation_per_unit: 0.0,
            aarnn_release_prob_heterogeneity: 0.0,
            synaptic_stabilization_strength: 0.05,
            axon_contact_dist: 0.03,
            component_decay_rate: 0.99,
            trunk_growth_rate: 0.005,
            branch_growth_rate: 0.02,
            bouton_growth_rate: 0.1,
            max_segment_length: 5.0,
            spatial_repulsion_strength: 0.01,
            spatial_clumping_strength: 0.005,
            columnar_enabled: false,
            columnar_spacing: 0.2,
            columnar_strength: 0.02,
            columnar_jitter: 0.15,
            density_target: 0.05,
            skull_pid_kp: 0.05,
            skull_pid_ki: 0.001,
            skull_pid_kd: 0.01,
            spontaneous_neuron_interval_ms: 1000.0,
            neuron_removal_delay_ms: 180000.0,
            max_sensory_connections: 4,
            max_output_connections: 4,
            component_pruning_threshold: 0.01,
            initial_synaptic_weight: 0.05,
            synaptic_growth_threshold: 0.6,
            synaptic_consolidation_factor: 0.1,
            aarnn_layer_depth: 5,
            aarnn_bio: AarnnBioParams::default(),
            ui_target_fps: 60.0,
        };
        apply_aarnn_human_biomimicry_defaults(&mut cfg);
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lif_params_default() {
        let p = LIFParams::default();
        assert_eq!(p.tau_m, 20.0);
        assert_eq!(p.v_th, 1.0);
        assert_eq!(p.v_reset, 0.0);
    }

    #[test]
    fn test_izhikevich_presets() {
        let p_rs = IzhikevichParams::from_preset("RS", 1.0);
        assert_eq!(p_rs.recovery_time_constant_a, 0.02);
        assert_eq!(p_rs.recovery_sensitivity_b, 0.2);
        assert_eq!(p_rs.membrane_reset_potential_c, -65.0);
        assert_eq!(p_rs.recovery_increment_d, 8.0);

        let p_fs = IzhikevichParams::from_preset("FS", 1.0);
        assert_eq!(p_fs.recovery_time_constant_a, 0.1);
        assert_eq!(p_fs.recovery_sensitivity_b, 0.2);
        assert_eq!(p_fs.membrane_reset_potential_c, -65.0);
        assert_eq!(p_fs.recovery_increment_d, 2.0);

        let p_default = IzhikevichParams::from_preset("UNKNOWN", 1.0);
        assert_eq!(p_default.recovery_time_constant_a, 0.02);
    }

    #[test]
    fn test_stdp_params_default() {
        let p = STDPParams::default();
        assert_eq!(p.tau_pre, 20.0);
        assert_eq!(p.tau_post, 20.0);
        assert!(p.eta > 0.0);
    }

    #[test]
    fn test_network_config_default() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.num_sensory_neurons, 0);
        assert!(cfg.num_hidden_layers > 0);
        assert!(cfg.num_hidden_per_layer_initial > 0);
        assert_eq!(cfg.num_output_neurons, 0);
        assert_eq!(cfg.clumping_design, ClumpingDesign::HumanBrain);
        assert!(!cfg.brain_regions.is_empty());
        assert!(!cfg.neuron_types.is_empty());
        assert!(cfg.growth_enabled);
        assert!(cfg.use_morphology);
        assert!(cfg.use_aarnn_delays);
        assert_eq!(cfg.aarnn_layer_depth, 5);
        assert!(cfg.aarnn_bio.stp_enabled);
        assert!(cfg.aarnn_bio.neuromodulation_enabled);
        assert_eq!(cfg.aarnn_dale_strictness, 0.75);
        assert_eq!(cfg.aarnn_gap_junction_strength, 0.02);
        assert_eq!(cfg.aarnn_nmda_voltage_sensitivity, 0.04);
    }
}
