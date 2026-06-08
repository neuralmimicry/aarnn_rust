//! # 3D Topology and Spatial Layout Scaffolding
//!
//! This module provides the data structures for managing the spatial 3D layout
//! of neurons in the network. It is used primarily when the `growth3d` feature
//! is enabled to track the physical positions of somas across hidden, sensory,
//! and output layers.
//!
//! ## Key Structures:
//! - `Node3D`: Represents a single neuron's position and its layer assignment.
//! - `Topology3D`: A container that organizes nodes by their respective layers.
//!
//! This spatial information is essential for calculating distance-dependent
//! connection probabilities and for rendering the network in the 3D UI.

#![cfg(feature = "growth3d")]

#[cfg_attr(
    any(feature = "ui", feature = "growth3d"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, Default)]
pub struct Node3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub layer: usize,
    pub region_name: Option<String>,
    pub type_name: Option<String>,
}

#[cfg_attr(
    any(feature = "ui", feature = "growth3d"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(
    any(feature = "ui", feature = "growth3d"),
    serde(rename_all = "snake_case")
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EarlyCellPhase {
    #[default]
    Specification,
    Migration,
    Differentiation,
}

#[cfg_attr(
    any(feature = "ui", feature = "growth3d"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, Default)]
pub struct EarlyCell3D {
    pub id: u64,
    pub source_layer: usize,
    pub source_parent: usize,
    pub target_layer: usize,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub start_x: f32,
    pub start_y: f32,
    pub start_z: f32,
    pub target_x: f32,
    pub target_y: f32,
    pub target_z: f32,
    pub age_ms: f32,
    pub maturation_ms: f32,
    pub phase: EarlyCellPhase,
    pub region_name: Option<String>,
    pub target_type_name: Option<String>,
}

#[cfg_attr(
    any(feature = "ui", feature = "growth3d"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(any(feature = "ui", feature = "growth3d"), serde(default))]
#[derive(Clone, Debug, Default)]
pub struct Topology3D {
    pub layers: Vec<Vec<Node3D>>, // hidden layers only
    pub sensory_nodes: Vec<Node3D>,
    pub output_nodes: Vec<Node3D>,
    pub early_cells: Vec<EarlyCell3D>,
}

impl Topology3D {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            sensory_nodes: Vec::new(),
            output_nodes: Vec::new(),
            early_cells: Vec::new(),
        }
    }
    pub fn add_layer(&mut self) {
        self.layers.push(Vec::new());
    }
    pub fn add_neuron(&mut self, layer: usize, node: Node3D) {
        if layer >= self.layers.len() {
            self.layers.resize_with(layer + 1, Vec::new);
        }
        self.layers[layer].push(Node3D { layer, ..node });
    }
    pub fn add_early_cell(&mut self, cell: EarlyCell3D) {
        self.early_cells.push(cell);
    }
    #[allow(dead_code)]
    pub fn len(&self, layer: usize) -> usize {
        self.layers.get(layer).map(|v| v.len()).unwrap_or(0)
    }
}

// Simple orthographic projection helper (no rotation), with basic depth cue:
#[allow(dead_code)]
pub fn project_ortho(normalized_x: f32, normalized_y: f32, normalized_z: f32) -> (f32, f32, f32) {
    // Input coords assumed roughly in [-1, 1]. Return (x, y, depth_factor)
    let depth = (1.0 - (normalized_z + 1.0) * 0.5).clamp(0.0, 1.0); // near z=1 -> small; far z=-1 -> big
    (normalized_x, normalized_y, depth)
}
