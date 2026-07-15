use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationSet {
    pub id: String,
    pub measured_at_wavelength_nm: f64,
    pub environment: Environment,
    pub materials: BTreeMap<String, MaterialCalibration>,
    pub node_transfer_curves: BTreeMap<String, NodeTransferCurve>,
    pub splitter: SplitterCalibration,
    pub detector: DetectorCalibration,
    pub mapping: MappingPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub temperature_c: f64,
    pub hydration_medium: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialCalibration {
    pub role: MaterialRole,
    pub refractive_index: f64,
    pub propagation_loss_db_per_cm: f64,
    pub minimum_feature_mm: f64,
    pub preferred_nozzle_mm: f64,
    pub pressure_kpa: f64,
    pub feed_mm_min: f64,
    #[serde(default)]
    pub fluorescence_lifetime_us: Option<f64>,
    #[serde(default)]
    pub fluorescence_gain: Option<f64>,
    #[serde(default)]
    pub cure: Option<CureProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialRole {
    Core,
    Cladding,
    FluorescentNode,
    Attenuator,
    OpaqueSeparator,
    Support,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CureProfile {
    pub wavelength_nm: u16,
    pub seconds_per_layer: f64,
    pub every_n_layers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTransferCurve {
    pub realisation: String,
    pub material_id: String,
    pub input_mw: Vec<f64>,
    pub output_mw: Vec<f64>,
    pub effective_delay_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitterCalibration {
    pub nominal_two_way_ratio: f64,
    pub excess_loss_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorCalibration {
    pub minimum_detectable_mw: f64,
    pub saturation_mw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingPolicy {
    pub core_material_id: String,
    pub cladding_material_id: String,
    pub fluorescent_node_material_id: String,
    pub inhibitory_encoding: InhibitoryEncoding,
    pub node_realisation: String,
    pub delay_strategy: String,
    pub physical_seconds_per_aarnn_us: f64,
    pub nominal_input_power_mw: f64,
    pub minimum_target_transmission: f64,
    pub default_core_diameter_mm: f64,
    pub default_node_radius_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InhibitoryEncoding {
    DualRail,
    SeparateWavelength,
    ElectronicSubtraction,
}
