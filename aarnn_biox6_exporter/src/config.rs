use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineProfile {
    pub id: String,
    pub model: String,
    pub build_volume_mm: Axis3,
    pub maximum_tools: u8,
    pub minimum_clearance_mm: f64,
    pub minimum_bend_radius_mm: f64,
    pub default_feed_mm_min: f64,
    pub travel_feed_mm_min: f64,
    pub tools: Vec<ToolProfile>,
    pub gcode: GcodeDialect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Axis3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProfile {
    pub tool: u8,
    pub material_id: String,
    pub nozzle_diameter_mm: f64,
    pub pressure_kpa: f64,
    #[serde(default)]
    pub temperature_c: Option<f64>,
    #[serde(default)]
    pub coaxial_partner_tool: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcodeDialect {
    pub header: Vec<String>,
    pub footer: Vec<String>,
    pub select_tool: String,
    pub set_pressure: String,
    pub set_temperature: String,
    pub move_linear: String,
    pub extrusion_start: String,
    pub extrusion_stop: String,
    pub dwell_ms: String,
    pub cure: String,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}
