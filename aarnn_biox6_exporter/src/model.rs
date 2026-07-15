use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AarnnSnapshot {
    pub schema_version: String,
    pub network_id: String,
    pub captured_at_tick: u64,
    pub time_unit_us: f64,
    pub nodes: Vec<LogicalNode>,
    pub connections: Vec<LogicalConnection>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalNode {
    pub id: String,
    pub kind: NodeKind,
    pub threshold: f64,
    #[serde(default)]
    pub bias: f64,
    #[serde(default)]
    pub refractory_us: f64,
    #[serde(default)]
    pub preferred_position_mm: Option<Vec3>,
    #[serde(default)]
    pub source_component_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Sensory,
    Interneuron,
    Motor,
    Readout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalConnection {
    pub id: String,
    pub source_node: String,
    pub target_node: String,
    pub weight: f64,
    pub delay_us: f64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub source_component_ids: Vec<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub schema_version: String,
    pub source_network_id: String,
    pub source_tick: u64,
    pub wavelength_nm: f64,
    pub time_scale: TimeScale,
    pub nodes: Vec<PhysicalNode>,
    pub connections: Vec<PhysicalConnection>,
    pub warnings: Vec<PlanWarning>,
    pub metrics: PlanMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeScale {
    /// Physical seconds represented by one AARNN microsecond.
    pub physical_seconds_per_aarnn_us: f64,
    /// The exporter preserves relative delays; exact biological-scale delay
    /// is normally realised by material dynamics or external electronics.
    pub strategy: DelayStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelayStrategy {
    GeometryOnly,
    FluorescenceAndGeometry,
    HybridElectronic,
    TimeBinned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalNode {
    pub id: String,
    pub logical_kind: NodeKind,
    pub position_mm: Vec3,
    pub radius_mm: f64,
    pub realisation: NodeRealisation,
    pub logical_threshold: f64,
    pub realised_threshold_mw: f64,
    pub logical_refractory_us: f64,
    pub realised_refractory_us: f64,
    pub optical_threshold_mw: f64,
    pub refractory_physical_us: f64,
    pub material_id: String,
    pub source_component_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRealisation {
    PassiveFluorescent,
    SoftThresholdFluorescent,
    Photochromic,
    Thermoresponsive,
    OptoelectronicRegenerator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalConnection {
    pub id: String,
    pub source_node: String,
    pub target_node: String,
    pub polarity: Polarity,
    pub path_mm: Vec<Vec3>,
    pub path_length_mm: f64,
    pub core_material_id: String,
    pub cladding_material_id: String,
    pub core_diameter_mm: f64,
    pub target_delay_us: f64,
    pub geometric_delay_us: f64,
    pub residual_delay_us: f64,
    pub target_transmission: f64,
    pub estimated_transmission: f64,
    pub implementation: EdgeImplementation,
    pub source_component_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Excitatory,
    Inhibitory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeImplementation {
    DirectWaveguide,
    AttenuatedWaveguide,
    FluorescentDelayNode,
    ExternalDelayChannel,
    DualRailInhibitory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanWarning {
    pub code: String,
    pub entity_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanMetrics {
    pub total_path_length_mm: f64,
    pub estimated_core_volume_ml: f64,
    pub estimated_cladding_volume_ml: f64,
    pub maximum_delay_error_us: f64,
    pub minimum_estimated_transmission: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolpathBundle {
    pub operations: Vec<ToolpathOperation>,
    pub material_volumes_ml: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ToolpathOperation {
    Comment { text: String },
    SelectTool { tool: u8, material_id: String },
    SetPressure { tool: u8, pressure_kpa: f64 },
    SetTemperature { tool: u8, temperature_c: f64 },
    Move { position_mm: Vec3, feed_mm_min: f64 },
    ExtrusionStart { tool: u8 },
    ExtrusionStop { tool: u8 },
    DwellMs { milliseconds: u64 },
    Cure { wavelength_nm: u16, seconds: f64 },
    PauseForComponent { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBundle {
    pub plan: PhysicalPlan,
    pub toolpaths: ToolpathBundle,
    pub gcode: String,
}
