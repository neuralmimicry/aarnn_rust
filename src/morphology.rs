//! # Biological Morphology and Developmental Simulation
//!
//! This module implements the dynamic 3D physical growth and structural plasticity
//! core of the engine ( Adaptive Axonal-Relay Neural Network - AARNN).
//!
//! ## Overview
//! Unlike traditional SNNs with fixed connectivity, this module simulates:
//! 1. **Soma Placement**: Neurons (sensory, hidden, output) are placed in a 3D volume.
//! 2. **Morphological Growth**: Axons and dendrites grow as directed segments in 3D
//!    space, driven by activity-dependent "energy" and spatial gradients.
//! 3. **Synaptogenesis**: Synapses are formed dynamically when axons and dendritic
//!    boutons come into close physical proximity.
//! 4. **Pruning**: Weak or inactive connections and their associated morphology
//!    can retract or be removed over time.
//! 5. **Physical Conduction**: Signal propagation delays are explicitly calculated
//!    based on the length and conduction velocity of each axonal/dendritic segment.
//!
//! ## Key Structures
//! - `Point3`: Basic 3D coordinate and vector math.
//! - `Soma`: Represents the cell body of a neuron.
//! - `MorphoSegment`: A directed cylinder in 3D representing a piece of an axon or dendrite.
//! - `Morphology`: The top-level container for all physical structures in the simulation.
#![cfg(feature = "morpho")]

use fastrand;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default)]
pub(crate) struct NoHasher(u64);
impl Hasher for NoHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, _bytes: &[u8]) {}
    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }
}
pub(crate) type FastHashMap<K, V> = HashMap<K, V, BuildHasherDefault<NoHasher>>;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
#[cfg(feature = "opencl")]
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "opencl")]
use crate::cl_compute::OpenCLManager;
#[cfg(feature = "opencl")]
use crate::cl_compute::{
    Buffer, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE, CL_TRUE, ClError, ExecuteKernel,
};

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Point3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point3 {
    pub fn dist_sq(&self, other: Point3) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }
    pub fn dist(&self, other: Point3) -> f32 {
        self.dist_sq(other).sqrt()
    }
    pub fn add(&self, other: Point3) -> Point3 {
        Point3 {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }
    pub fn sub(&self, other: Point3) -> Point3 {
        Point3 {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
    pub fn mul(&self, s: f32) -> Point3 {
        Point3 {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
    pub fn dot(&self, other: Point3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }
    pub fn mag(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn normalize(&self) -> Point3 {
        let m = self.mag();
        if m > 1e-9 { self.mul(1.0 / m) } else { *self }
    }
    pub fn lerp(&self, other: Point3, t: f32) -> Point3 {
        Point3 {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            z: self.z + (other.z - self.z) * t,
        }
    }
}

#[inline(always)]
pub fn dist2_point_to_segment(p: Point3, a: Point3, b: Point3) -> (f32, Point3) {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dz = b.z - a.z;
    let len2 = dx * dx + dy * dy + dz * dz;
    if len2 < 1e-12 {
        return (p.dist_sq(a), a);
    }

    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy + (p.z - a.z) * dz) / len2;
    let t = t.clamp(0.0, 1.0);

    let proj = Point3 {
        x: a.x + t * dx,
        y: a.y + t * dy,
        z: a.z + t * dz,
    };
    (p.dist_sq(proj), proj)
}

#[inline(always)]
pub(crate) fn seg_seg_min_dist_sq(a0: Point3, a1: Point3, b0: Point3, b1: Point3) -> f32 {
    let ux = a1.x - a0.x;
    let uy = a1.y - a0.y;
    let uz = a1.z - a0.z;
    let vx = b1.x - b0.x;
    let vy = b1.y - b0.y;
    let vz = b1.z - b0.z;
    let wx0 = a0.x - b0.x;
    let wy0 = a0.y - b0.y;
    let wz0 = a0.z - b0.z;

    let a = ux * ux + uy * uy + uz * uz;
    let b = ux * vx + uy * vy + uz * vz;
    let c = vx * vx + vy * vy + vz * vz;
    let d = ux * wx0 + uy * wy0 + uz * wz0;
    let e = vx * wx0 + vy * wy0 + vz * wz0;

    let denom = a * c - b * b;

    // f32-friendly epsilon
    const EPS: f32 = 1e-6;

    let (mut s_n, mut s_d, mut t_n, mut t_d);

    if denom <= EPS {
        s_n = 0.0;
        s_d = 1.0;
        t_n = e;
        t_d = c;
    } else {
        s_n = b * e - c * d;
        t_n = a * e - b * d;
        s_d = denom;
        t_d = denom;

        if s_n < 0.0 {
            s_n = 0.0;
            t_n = e;
            t_d = c;
        } else if s_n > s_d {
            s_n = s_d;
            t_n = e + b;
            t_d = c;
        }
    }

    if t_n < 0.0 {
        t_n = 0.0;
        if -d < 0.0 {
            s_n = 0.0;
        } else if -d > a {
            s_n = s_d;
        } else {
            s_n = -d;
            s_d = a;
        }
    } else if t_n > t_d {
        t_n = t_d;
        let db = -d + b;
        if db < 0.0 {
            s_n = 0.0;
        } else if db > a {
            s_n = s_d;
        } else {
            s_n = db;
            s_d = a;
        }
    }

    let sc = if s_d.abs() <= EPS {
        0.0
    } else {
        s_n * (1.0 / s_d)
    };
    let tc = if t_d.abs() <= EPS {
        0.0
    } else {
        t_n * (1.0 / t_d)
    };

    let dx = wx0 + sc * ux - tc * vx;
    let dy = wy0 + sc * uy - tc * vy;
    let dz = wz0 + sc * uz - tc * vz;

    dx.mul_add(dx, dy.mul_add(dy, dz * dz))
}

fn aabb_overlap(a0: &Point3, a1: &Point3, b0: &Point3, b1: &Point3, pad: f32) -> bool {
    let (ax0, ay0, az0) = (
        a0.x.min(a1.x) - pad,
        a0.y.min(a1.y) - pad,
        a0.z.min(a1.z) - pad,
    );
    let (ax1, ay1, az1) = (
        a0.x.max(a1.x) + pad,
        a0.y.max(a1.y) + pad,
        a0.z.max(a1.z) + pad,
    );
    let (bx0, by0, bz0) = (
        b0.x.min(b1.x) - pad,
        b0.y.min(b1.y) - pad,
        b0.z.min(b1.z) - pad,
    );
    let (bx1, by1, bz1) = (
        b0.x.max(b1.x) + pad,
        b0.y.max(b1.y) + pad,
        b0.z.max(b1.z) + pad,
    );
    !(ax1 < bx0 || bx1 < ax0 || ay1 < by0 || by1 < ay0 || az1 < bz0 || bz1 < az0)
}

#[inline(always)]
fn near_colinear_overlap(a0: Point3, a1: Point3, b0: Point3, b1: Point3) -> bool {
    let ux = a1.x - a0.x;
    let uy = a1.y - a0.y;
    let uz = a1.z - a0.z;
    let vx = b1.x - b0.x;
    let vy = b1.y - b0.y;
    let vz = b1.z - b0.z;

    let du2 = ux * ux + uy * uy + uz * uz;
    let dv2 = vx * vx + vy * vy + vz * vz;
    if du2 < 1e-12 || dv2 < 1e-12 {
        return false;
    }

    let dot = ux * vx + uy * vy + uz * vz;
    // |cos(theta)| > 0.995  <=>  dot^2 > (0.995^2) * du2 * dv2
    const C2: f32 = 0.995 * 0.995;
    if dot * dot < C2 * du2 * dv2 {
        return false;
    }

    // Project b endpoints onto a using du2 directly (no sqrt)
    #[inline(always)]
    fn proj(p: Point3, a0: Point3, ux: f32, uy: f32, uz: f32, du2: f32) -> f32 {
        ((p.x - a0.x) * ux + (p.y - a0.y) * uy + (p.z - a0.z) * uz) / du2
    }

    let mut t0 = proj(b0, a0, ux, uy, uz, du2);
    let mut t1 = proj(b1, a0, ux, uy, uz, du2);
    if t0 > t1 {
        std::mem::swap(&mut t0, &mut t1);
    }

    let lo = t0.max(0.0);
    let hi = t1.min(1.0);
    hi - lo > 0.05
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SynKind {
    In,
    HiddenFwd,
    HiddenBwd,
    HiddenRec,
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DendriteType {
    Generic,
    Apical,
    Basal,
}

impl Default for DendriteType {
    fn default() -> Self {
        Self::Generic
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OrganelleKind {
    Mitochondria,
    Nucleus,
    #[allow(dead_code)]
    Ribosome,
    #[allow(dead_code)]
    Lysosome,
    GolgiApparatus,
    EndoplasmicReticulum,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkullMembrane {
    pub center: Point3,
    /// Legacy spherical radius kept for backward compatibility and quick UI fallbacks
    pub radius: f32,
    /// Optional per‑axis radii for an axis‑aligned ellipsoid that better fits the neuron cloud
    pub radii: Option<(f32, f32, f32)>,
    /// Optional alpha radius for concave hull / alpha-shape volume
    pub alpha_radius: Option<f32>,
    pub energy_fluctuation: f32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Organelle {
    pub kind: OrganelleKind,
    pub pos: Point3,
    pub activity: f32, // 0.0 to 1.0
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Soma {
    pub id: usize,    // neuron index within its hidden layer
    pub layer: usize, // hidden layer index
    pub pos: Point3,  // position in normalized coordinates
    pub stimuli: f32, // attractant release (neurotrophins)
    pub atp: f32,     // metabolic energy level
    pub organelles: Vec<Organelle>,
    pub prev_err: Point3,     // for PID smoothing
    pub integral_err: Point3, // for PID smoothing
    pub region_name: Option<String>,
    pub type_name: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AxonSeg {
    pub from: Point3,
    pub to: Point3,
    pub length: f32,
    pub stimuli: f32,
    pub parent_idx: Option<usize>,
    pub syn_index: Option<usize>,
    pub is_trunk: bool,
}

impl Default for AxonSeg {
    fn default() -> Self {
        Self {
            from: Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            to: Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            length: 0.0,
            stimuli: 0.0,
            parent_idx: None,
            syn_index: None,
            is_trunk: true,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Axon {
    pub neuron_layer: usize,
    pub neuron_id: usize,
    pub segments: Vec<AxonSeg>,
    pub stimuli: f32,
    pub atp: f32,
    #[allow(dead_code)]
    pub organelles: Vec<Organelle>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DendSeg {
    pub from: Point3,
    pub to: Point3,
    pub length: f32,
    /// Branch compartment type (apical, basal, or generic).
    pub dendrite_type: DendriteType,
    /// Initial trunk distance from soma for this branch lineage.
    pub trunk_len_from_soma: f32,
    pub stimuli: f32,
    pub parent_idx: Option<usize>,
    pub syn_index: Option<usize>,
    pub is_trunk: bool,
}

impl Default for DendSeg {
    fn default() -> Self {
        Self {
            from: Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            to: Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            length: 0.0,
            dendrite_type: DendriteType::Generic,
            trunk_len_from_soma: 0.0,
            stimuli: 0.0,
            parent_idx: None,
            syn_index: None,
            is_trunk: true,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DendriticTree {
    pub branches: Vec<DendSeg>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Dendrite {
    pub neuron_layer: usize,
    pub neuron_id: usize,
    pub tree: DendriticTree,
    pub stimuli: f32,
    pub atp: f32,
    #[allow(dead_code)]
    pub organelles: Vec<Organelle>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Synapse {
    pub kind: SynKind,
    pub pre_layer: isize, // -1 for sensory, L for output sinks
    pub pre_id: usize,
    pub post_layer: isize, // -1 for sensory sources, L for output layer
    pub post_id: usize,
    pub pre_site: Point3,
    pub post_site: Point3,
    pub axon_seg_idx: Option<usize>, // index of segment in axon.segments
    pub dend_seg_idx: Option<usize>, // index of segment in dendrite.tree.branches
    /// Optional mid-connection bend point used only for visualization.
    /// Bends are introduced when straight segments would nearly coincide.
    pub bend: Option<Point3>,
    pub weight: f64,
    #[allow(dead_code)]
    pub p_release: f32,
    #[allow(dead_code)]
    pub delay_ms: f32,
    pub stimuli: f32,
}

#[cfg(all(feature = "morpho", feature = "growth3d"))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum ReleasedKind {
    In,
    Fwd { layer: usize },
    Bwd { layer: usize },
    HiddenRec { layer: usize },
    Out,
}

#[cfg(all(feature = "morpho", feature = "growth3d"))]
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReleasedEvent {
    pub kind: ReleasedKind,
    #[allow(dead_code)]
    pub pre_layer: isize,
    #[allow(dead_code)]
    pub post_layer: isize,
    pub pre_id: usize,
    pub post_id: usize,
    pub syn_idx: Option<usize>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Morphology {
    pub somas: Vec<Vec<Soma>>,         // per hidden layer
    pub axons: Vec<Vec<Axon>>,         // per hidden layer
    pub dendrites: Vec<Vec<Dendrite>>, // per hidden layer

    pub sensory_somas: Vec<Soma>,
    pub sensory_axons: Vec<Axon>,
    pub sensory_dendrites: Vec<Dendrite>,

    pub output_somas: Vec<Soma>,
    pub output_axons: Vec<Axon>,
    pub output_dendrites: Vec<Dendrite>,

    pub synapses: Vec<Synapse>, // flat list of synapses
    /// Spatial index for fast proximity lookups (grid or octree, populated on demand)
    #[serde(skip, default)]
    pub(crate) spatial_index: Option<SpatialIndex>,
    pub skull_membrane: Option<SkullMembrane>,
    pub skull_center_integral: Point3,
    pub skull_radius_integral: f32,
    pub skull_center_prev_err: Point3,
    pub skull_radius_prev_err: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GridEntity {
    pub pos: Point3,
    pub stimuli: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct SpatialGrid {
    pub(crate) entities: Vec<GridEntity>,
    pub(crate) cell_starts: Vec<u32>,
    pub(crate) dim: usize,
    pub(crate) cell_size: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum SpatialIndex {
    Grid(SpatialGrid),
    Octree(OctreeIndex),
}

impl SpatialIndex {
    #[inline(always)]
    pub fn entities(&self) -> &[GridEntity] {
        match self {
            SpatialIndex::Grid(g) => &g.entities,
            SpatialIndex::Octree(o) => &o.entities,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OctreeIndex {
    pub(crate) entities: Vec<GridEntity>,
    root: OctreeNode,
    max_depth: u8,
    max_leaf: usize,
}

#[derive(Clone, Debug)]
struct OctreeNode {
    center: Point3,
    half: f32,
    indices: Vec<usize>,
    children: [Option<Box<OctreeNode>>; 8],
}

#[derive(Clone, Copy, Debug)]
struct SegRef {
    l: isize,
    j: usize,
    si: usize,
    min: Point3,
    max: Point3,
}

#[derive(Clone, Debug)]
enum AxonSegIndex {
    Grid {
        cell_size: f32,
        map: FastHashMap<u64, Vec<usize>>,
        segs: Vec<SegRef>,
    },
    Octree(OctreeSegIndex),
}

impl AxonSegIndex {
    fn build(segs: Vec<SegRef>, cell_size: f32, contact_dist: f32) -> Self {
        let cs = cell_size.max(0.01);
        let dim = (2.0 / cs).ceil() as usize;
        let num_cells = dim.saturating_mul(dim).saturating_mul(dim);
        let use_octree = num_cells > 2_000_000
            || (segs.len() > 512 && num_cells > segs.len().saturating_mul(64));
        if use_octree {
            return AxonSegIndex::Octree(OctreeSegIndex::build(segs, cs));
        }

        let mut map: FastHashMap<u64, Vec<usize>> = FastHashMap::default();
        for (idx, seg) in segs.iter().enumerate() {
            let pad = contact_dist;
            let min_gx = ((seg.min.x - pad) / cs).floor() as i64;
            let max_gx = ((seg.max.x + pad) / cs).floor() as i64;
            let min_gy = ((seg.min.y - pad) / cs).floor() as i64;
            let max_gy = ((seg.max.y + pad) / cs).floor() as i64;
            let min_gz = ((seg.min.z - pad) / cs).floor() as i64;
            let max_gz = ((seg.max.z + pad) / cs).floor() as i64;

            if (max_gx - min_gx + 1) * (max_gy - min_gy + 1) * (max_gz - min_gz + 1) > 512 {
                continue;
            }

            for gx in min_gx..=max_gx {
                for gy in min_gy..=max_gy {
                    for gz in min_gz..=max_gz {
                        let key = (((gx + 1048576) & 0x1FFFFF) as u64)
                            | ((((gy + 1048576) & 0x1FFFFF) as u64) << 21)
                            | ((((gz + 1048576) & 0x1FFFFF) as u64) << 42);
                        map.entry(key).or_default().push(idx);
                    }
                }
            }
        }

        AxonSegIndex::Grid {
            cell_size: cs,
            map,
            segs,
        }
    }

    fn for_each_candidate<F: FnMut(SegRef) -> bool>(&self, p: Point3, r: f32, mut f: F) {
        match self {
            AxonSegIndex::Grid {
                cell_size,
                map,
                segs,
            } => {
                let cs = *cell_size;
                let gx = (p.x / cs).floor() as i64;
                let gy = (p.y / cs).floor() as i64;
                let gz = (p.z / cs).floor() as i64;
                let r_cells = (r / cs).ceil().max(1.0) as i64;
                for dx in -r_cells..=r_cells {
                    for dy in -r_cells..=r_cells {
                        for dz in -r_cells..=r_cells {
                            let key = (((gx + dx + 1048576) & 0x1FFFFF) as u64)
                                | ((((gy + dy + 1048576) & 0x1FFFFF) as u64) << 21)
                                | ((((gz + dz + 1048576) & 0x1FFFFF) as u64) << 42);
                            if let Some(list) = map.get(&key) {
                                for &idx in list {
                                    if !f(segs[idx]) {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            AxonSegIndex::Octree(oct) => {
                let _ = oct.root.for_each_candidate(p, r * r, &oct.segs, &mut f);
            }
        }
    }

    fn collect_candidates(&self, p: Point3, r: f32, out: &mut Vec<SegRef>) {
        out.clear();
        self.for_each_candidate(p, r, |seg| {
            out.push(seg);
            true
        });
    }
}

#[derive(Clone, Debug)]
struct OctreeSegIndex {
    segs: Vec<SegRef>,
    root: OctreeSegNode,
    max_depth: u8,
    max_leaf: usize,
}

#[derive(Clone, Debug)]
struct OctreeSegNode {
    center: Point3,
    half: f32,
    indices: Vec<usize>,
    children: [Option<Box<OctreeSegNode>>; 8],
}

impl OctreeSegIndex {
    fn build(segs: Vec<SegRef>, target_leaf: f32) -> Self {
        let (center, half) = Self::compute_bounds(&segs);
        let max_depth = OctreeIndex::depth_for_leaf(half, target_leaf);
        let max_leaf = 24usize;
        let indices: Vec<usize> = (0..segs.len()).collect();
        let root = OctreeSegNode::build(center, half, indices, 0, max_depth, max_leaf, &segs);
        Self {
            segs,
            root,
            max_depth,
            max_leaf,
        }
    }

    fn compute_bounds(segs: &[SegRef]) -> (Point3, f32) {
        let mut min = Point3 {
            x: f32::INFINITY,
            y: f32::INFINITY,
            z: f32::INFINITY,
        };
        let mut max = Point3 {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
            z: f32::NEG_INFINITY,
        };
        if segs.is_empty() {
            min = Point3 {
                x: -1.0,
                y: -1.0,
                z: -1.0,
            };
            max = Point3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            };
        } else {
            for s in segs {
                min.x = min.x.min(s.min.x);
                min.y = min.y.min(s.min.y);
                min.z = min.z.min(s.min.z);
                max.x = max.x.max(s.max.x);
                max.y = max.y.max(s.max.y);
                max.z = max.z.max(s.max.z);
            }
            min.x = min.x.min(-1.0);
            min.y = min.y.min(-1.0);
            min.z = min.z.min(-1.0);
            max.x = max.x.max(1.0);
            max.y = max.y.max(1.0);
            max.z = max.z.max(1.0);
        }
        let center = Point3 {
            x: 0.5 * (min.x + max.x),
            y: 0.5 * (min.y + max.y),
            z: 0.5 * (min.z + max.z),
        };
        let span_x = (max.x - min.x).max(1e-6);
        let span_y = (max.y - min.y).max(1e-6);
        let span_z = (max.z - min.z).max(1e-6);
        let half = 0.5 * span_x.max(span_y).max(span_z) * 1.01 + 1e-3;
        (center, half)
    }
}

impl OctreeSegNode {
    fn build(
        center: Point3,
        half: f32,
        indices: Vec<usize>,
        depth: u8,
        max_depth: u8,
        max_leaf: usize,
        segs: &[SegRef],
    ) -> Self {
        if indices.len() <= max_leaf || depth >= max_depth {
            return OctreeSegNode {
                center,
                half,
                indices,
                children: std::array::from_fn(|_| None),
            };
        }

        let mut buckets: [Vec<usize>; 8] = std::array::from_fn(|_| Vec::new());
        let mut keep: Vec<usize> = Vec::new();
        let child_half = half * 0.5;

        for idx in indices {
            let seg = &segs[idx];
            let mut oct = 0usize;
            if seg.min.x >= center.x {
                oct |= 1;
            }
            if seg.min.y >= center.y {
                oct |= 2;
            }
            if seg.min.z >= center.z {
                oct |= 4;
            }

            let child_center = Point3 {
                x: center.x
                    + if (oct & 1) != 0 {
                        child_half
                    } else {
                        -child_half
                    },
                y: center.y
                    + if (oct & 2) != 0 {
                        child_half
                    } else {
                        -child_half
                    },
                z: center.z
                    + if (oct & 4) != 0 {
                        child_half
                    } else {
                        -child_half
                    },
            };
            let fits = seg.min.x >= child_center.x - child_half
                && seg.max.x <= child_center.x + child_half
                && seg.min.y >= child_center.y - child_half
                && seg.max.y <= child_center.y + child_half
                && seg.min.z >= child_center.z - child_half
                && seg.max.z <= child_center.z + child_half;

            if fits {
                buckets[oct].push(idx);
            } else {
                keep.push(idx);
            }
        }

        let mut children: [Option<Box<OctreeSegNode>>; 8] = std::array::from_fn(|_| None);
        for (oct, bucket) in buckets.into_iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }
            let child_center = Point3 {
                x: center.x
                    + if (oct & 1) != 0 {
                        child_half
                    } else {
                        -child_half
                    },
                y: center.y
                    + if (oct & 2) != 0 {
                        child_half
                    } else {
                        -child_half
                    },
                z: center.z
                    + if (oct & 4) != 0 {
                        child_half
                    } else {
                        -child_half
                    },
            };
            children[oct] = Some(Box::new(OctreeSegNode::build(
                child_center,
                child_half,
                bucket,
                depth + 1,
                max_depth,
                max_leaf,
                segs,
            )));
        }

        OctreeSegNode {
            center,
            half,
            indices: keep,
            children,
        }
    }

    #[inline(always)]
    fn intersects_sphere(&self, p: Point3, r2: f32) -> bool {
        let dx = (p.x - self.center.x).abs() - self.half;
        let dy = (p.y - self.center.y).abs() - self.half;
        let dz = (p.z - self.center.z).abs() - self.half;
        let dx = if dx > 0.0 { dx } else { 0.0 };
        let dy = if dy > 0.0 { dy } else { 0.0 };
        let dz = if dz > 0.0 { dz } else { 0.0 };
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= r2
    }

    #[inline(always)]
    fn point_aabb_dist2(p: Point3, min: Point3, max: Point3) -> f32 {
        let dx = if p.x < min.x {
            min.x - p.x
        } else if p.x > max.x {
            p.x - max.x
        } else {
            0.0
        };
        let dy = if p.y < min.y {
            min.y - p.y
        } else if p.y > max.y {
            p.y - max.y
        } else {
            0.0
        };
        let dz = if p.z < min.z {
            min.z - p.z
        } else if p.z > max.z {
            p.z - max.z
        } else {
            0.0
        };
        dx.mul_add(dx, dy.mul_add(dy, dz * dz))
    }

    fn for_each_candidate<F: FnMut(SegRef) -> bool>(
        &self,
        p: Point3,
        r2: f32,
        segs: &[SegRef],
        f: &mut F,
    ) -> bool {
        if !self.intersects_sphere(p, r2) {
            return true;
        }
        for &idx in &self.indices {
            let seg = segs[idx];
            if Self::point_aabb_dist2(p, seg.min, seg.max) <= r2 {
                if !f(seg) {
                    return false;
                }
            }
        }
        for child in &self.children {
            if let Some(c) = child {
                if !c.for_each_candidate(p, r2, segs, f) {
                    return false;
                }
            }
        }
        true
    }
}

impl OctreeIndex {
    fn build(entities: Vec<GridEntity>, target_leaf: f32) -> Self {
        let (center, half) = Self::compute_bounds(&entities);
        let max_depth = Self::depth_for_leaf(half, target_leaf);
        let max_leaf = 16usize;
        let indices: Vec<usize> = (0..entities.len()).collect();
        let root = OctreeNode::build(center, half, indices, 0, max_depth, max_leaf, &entities);
        Self {
            entities,
            root,
            max_depth,
            max_leaf,
        }
    }

    #[inline(always)]
    fn energy_at(&self, p: Point3, radius: f32, k: f32) -> f32 {
        let r2 = radius * radius;
        let mut total = 0.0;
        self.root
            .accumulate_energy(p, r2, k, &self.entities, &mut total);
        total
    }

    fn compute_bounds(entities: &[GridEntity]) -> (Point3, f32) {
        let mut min = Point3 {
            x: f32::INFINITY,
            y: f32::INFINITY,
            z: f32::INFINITY,
        };
        let mut max = Point3 {
            x: f32::NEG_INFINITY,
            y: f32::NEG_INFINITY,
            z: f32::NEG_INFINITY,
        };
        if entities.is_empty() {
            min = Point3 {
                x: -1.0,
                y: -1.0,
                z: -1.0,
            };
            max = Point3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            };
        } else {
            for e in entities {
                min.x = min.x.min(e.pos.x);
                min.y = min.y.min(e.pos.y);
                min.z = min.z.min(e.pos.z);
                max.x = max.x.max(e.pos.x);
                max.y = max.y.max(e.pos.y);
                max.z = max.z.max(e.pos.z);
            }
            // Ensure the bounds always cover the normalized space used by sampling.
            min.x = min.x.min(-1.0);
            min.y = min.y.min(-1.0);
            min.z = min.z.min(-1.0);
            max.x = max.x.max(1.0);
            max.y = max.y.max(1.0);
            max.z = max.z.max(1.0);
        }
        let center = Point3 {
            x: 0.5 * (min.x + max.x),
            y: 0.5 * (min.y + max.y),
            z: 0.5 * (min.z + max.z),
        };
        let span_x = (max.x - min.x).max(1e-6);
        let span_y = (max.y - min.y).max(1e-6);
        let span_z = (max.z - min.z).max(1e-6);
        let half = 0.5 * span_x.max(span_y).max(span_z) * 1.01 + 1e-3;
        (center, half)
    }

    fn depth_for_leaf(root_half: f32, target_leaf: f32) -> u8 {
        let target = target_leaf.max(0.01);
        if root_half <= target {
            return 1;
        }
        let ratio = (root_half / target).max(1.0);
        let depth = ratio.log2().ceil() as i32;
        depth.clamp(1, 10) as u8
    }
}

impl OctreeNode {
    fn build(
        center: Point3,
        half: f32,
        indices: Vec<usize>,
        depth: u8,
        max_depth: u8,
        max_leaf: usize,
        entities: &[GridEntity],
    ) -> Self {
        if indices.len() <= max_leaf || depth >= max_depth {
            return OctreeNode {
                center,
                half,
                indices,
                children: std::array::from_fn(|_| None),
            };
        }

        let mut buckets: [Vec<usize>; 8] = std::array::from_fn(|_| Vec::new());
        for idx in indices {
            let p = entities[idx].pos;
            let mut oct = 0usize;
            if p.x >= center.x {
                oct |= 1;
            }
            if p.y >= center.y {
                oct |= 2;
            }
            if p.z >= center.z {
                oct |= 4;
            }
            buckets[oct].push(idx);
        }

        let mut children: [Option<Box<OctreeNode>>; 8] = std::array::from_fn(|_| None);
        let child_half = half * 0.5;
        for (oct, bucket) in buckets.into_iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }
            let ox = if (oct & 1) != 0 {
                child_half
            } else {
                -child_half
            };
            let oy = if (oct & 2) != 0 {
                child_half
            } else {
                -child_half
            };
            let oz = if (oct & 4) != 0 {
                child_half
            } else {
                -child_half
            };
            let child_center = Point3 {
                x: center.x + ox,
                y: center.y + oy,
                z: center.z + oz,
            };
            children[oct] = Some(Box::new(OctreeNode::build(
                child_center,
                child_half,
                bucket,
                depth + 1,
                max_depth,
                max_leaf,
                entities,
            )));
        }

        OctreeNode {
            center,
            half,
            indices: Vec::new(),
            children,
        }
    }

    #[inline(always)]
    fn intersects_sphere(&self, p: Point3, r2: f32) -> bool {
        let dx = (p.x - self.center.x).abs() - self.half;
        let dy = (p.y - self.center.y).abs() - self.half;
        let dz = (p.z - self.center.z).abs() - self.half;
        let dx = if dx > 0.0 { dx } else { 0.0 };
        let dy = if dy > 0.0 { dy } else { 0.0 };
        let dz = if dz > 0.0 { dz } else { 0.0 };
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= r2
    }

    #[inline(always)]
    fn is_leaf(&self) -> bool {
        self.children.iter().all(|c| c.is_none())
    }

    fn accumulate_energy(
        &self,
        p: Point3,
        r2: f32,
        k: f32,
        entities: &[GridEntity],
        total: &mut f32,
    ) {
        if !self.intersects_sphere(p, r2) {
            return;
        }
        if self.is_leaf() {
            for &idx in &self.indices {
                let e = &entities[idx];
                let d2 = p.dist_sq(e.pos);
                if d2 < r2 {
                    *total += e.stimuli / (1.0 + k * d2);
                }
            }
        } else {
            for child in &self.children {
                if let Some(c) = child {
                    c.accumulate_energy(p, r2, k, entities, total);
                }
            }
        }
    }
}

impl SpatialGrid {
    #[inline(always)]
    #[allow(dead_code)]
    pub fn get_key(&self, p: Point3) -> Option<usize> {
        let gx = ((p.x + 1.0) / self.cell_size).floor() as isize;
        let gy = ((p.y + 1.0) / self.cell_size).floor() as isize;
        let gz = ((p.z + 1.0) / self.cell_size).floor() as isize;
        if gx < 0
            || gx >= self.dim as isize
            || gy < 0
            || gy >= self.dim as isize
            || gz < 0
            || gz >= self.dim as isize
        {
            return None;
        }
        Some((gx as usize * self.dim + gy as usize) * self.dim + gz as usize)
    }

    #[inline(always)]
    pub fn get_key_from_indices(&self, gx: isize, gy: isize, gz: isize) -> Option<usize> {
        if gx < 0
            || gx >= self.dim as isize
            || gy < 0
            || gy >= self.dim as isize
            || gz < 0
            || gz >= self.dim as isize
        {
            return None;
        }
        Some((gx as usize * self.dim + gy as usize) * self.dim + gz as usize)
    }

    #[inline(always)]
    pub fn cell_entities(&self, key: usize) -> &[GridEntity] {
        let start = self.cell_starts[key] as usize;
        let end = self.cell_starts[key + 1] as usize;
        &self.entities[start..end]
    }
}

#[derive(Default, Clone, Copy)]
struct MorphoStats {
    dendrite_sprout_attempts: usize,
    dendrite_sprout_successes: usize,
    dendrite_sprout_low_energy: usize,
    dendrite_sprout_too_near: usize,

    axon_sprout_attempts: usize,
    axon_sprout_successes: usize,

    contact_checks: usize,
    contact_candidates: usize,
    contact_incompatible: usize,
    contact_too_far: usize,
    contact_successes: usize,
    contact_rejected_cap: usize,
    contact_rejected_close: usize,
    contact_self_skips: usize,
    contact_existing_skips: usize,
    contact_post_cap_skips: usize,
    contact_probe_checks: usize,
    contact_skipped_low_energy: usize,
    contact_tip_cap_hits: usize,
    contact_tip_energy_sum: f32,
    contact_tip_energy_min: f32,
    contact_tip_energy_max: f32,
    contact_tip_energy_count: usize,
}

#[derive(Clone, Copy)]
struct MorphoEnergyTuning {
    ema: f32,
    dev: f32,
    cap_scale: f32,
    skip_bias: f32,
}

fn morpho_energy_tuning() -> &'static Mutex<MorphoEnergyTuning> {
    static TUNING: OnceLock<Mutex<MorphoEnergyTuning>> = OnceLock::new();
    TUNING.get_or_init(|| {
        Mutex::new(MorphoEnergyTuning {
            ema: 0.2,
            dev: 0.05,
            cap_scale: 1.0,
            skip_bias: 1.0,
        })
    })
}

#[derive(Clone, Copy, Debug)]
struct DendriteLayout {
    apical_trunks: usize,
    basal_trunks: usize,
}

impl DendriteLayout {
    #[inline]
    fn total_trunks(&self) -> usize {
        self.apical_trunks + self.basal_trunks
    }

    #[inline]
    fn trunk_type(&self, idx: usize) -> DendriteType {
        if idx < self.apical_trunks {
            DendriteType::Apical
        } else if idx < self.total_trunks() {
            DendriteType::Basal
        } else {
            DendriteType::Generic
        }
    }
}

#[inline]
fn is_apical_basal_cell_type(type_name: Option<&str>) -> bool {
    let Some(name) = type_name else {
        return false;
    };
    let n = name.to_ascii_lowercase();
    n.contains("pyramidal")
        || n.contains("corticothalamic")
        || n.contains("purkinje")
        || n.contains("projection_pn")
}

#[inline]
fn dendrite_layout_for_type(type_name: Option<&str>) -> DendriteLayout {
    if is_apical_basal_cell_type(type_name) {
        DendriteLayout {
            apical_trunks: 1,
            basal_trunks: 2,
        }
    } else {
        DendriteLayout {
            apical_trunks: 0,
            basal_trunks: 2,
        }
    }
}

#[inline]
fn trunk_scale_for(dend_type: DendriteType, config: &crate::config::NetworkConfig) -> f32 {
    match dend_type {
        DendriteType::Apical => config.aarnn_apical_trunk_scale.max(0.05),
        DendriteType::Basal => config.aarnn_basal_trunk_scale.max(0.05),
        DendriteType::Generic => 1.0,
    }
}

#[inline]
fn morphology_io_layers(
    config: &crate::config::NetworkConfig,
    is_aarnn: bool,
    num_hidden_layers: usize,
) -> (usize, usize) {
    if num_hidden_layers == 0 {
        return (0, 0);
    }

    let (default_in, default_out) = if is_aarnn {
        match crate::config::infer_biomimicry_profile(config) {
            crate::config::AarnnBiomimicryProfile::Human => (
                if num_hidden_layers > 1 { 1 } else { 0 },
                if num_hidden_layers > 2 {
                    2
                } else {
                    num_hidden_layers.saturating_sub(1)
                },
            ),
            crate::config::AarnnBiomimicryProfile::Celegans
            | crate::config::AarnnBiomimicryProfile::Drosophila
            | crate::config::AarnnBiomimicryProfile::Hexapod
            | crate::config::AarnnBiomimicryProfile::ZebraFish => {
                (0, num_hidden_layers.saturating_sub(1))
            }
        }
    } else {
        (0, num_hidden_layers.saturating_sub(1))
    };

    let mut in_l = config
        .sensory_target_layer
        .unwrap_or(default_in)
        .min(num_hidden_layers - 1);

    let mut out_l = config
        .output_source_layer
        .unwrap_or(default_out)
        .min(num_hidden_layers - 1);

    if is_aarnn && out_l < in_l {
        out_l = in_l;
    }

    in_l = in_l.min(num_hidden_layers - 1);
    out_l = out_l.min(num_hidden_layers - 1);

    (in_l, out_l)
}

#[inline]
fn morphology_output_connectivity_floor(
    config: &crate::config::NetworkConfig,
    hidden_out_count: usize,
) -> usize {
    if hidden_out_count == 0 {
        return 1;
    }
    let suggested = match crate::config::infer_biomimicry_profile(config) {
        crate::config::AarnnBiomimicryProfile::Human => 1usize,
        crate::config::AarnnBiomimicryProfile::Celegans
        | crate::config::AarnnBiomimicryProfile::Drosophila
        | crate::config::AarnnBiomimicryProfile::Hexapod
        | crate::config::AarnnBiomimicryProfile::ZebraFish => (hidden_out_count / 12).clamp(8, 32),
    };
    suggested.min(hidden_out_count.max(1))
}

impl Morphology {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_hidden_neuron(
        &mut self,
        l: usize,
        j: usize,
        pos: Point3,
        synapse_offset: f32,
        start_empty: bool,
        region_name: Option<String>,
        type_name: Option<String>,
    ) {
        if l >= self.somas.len() {
            self.somas.resize_with(l + 1, Vec::new);
            self.axons.resize_with(l + 1, Vec::new);
            self.dendrites.resize_with(l + 1, Vec::new);
        }

        let seed = ((l as u64) << 32) ^ (j as u64) ^ 0x01020304;
        let dend_layout = dendrite_layout_for_type(type_name.as_deref());
        let mut organelles = Vec::new();
        organelles.push(Organelle {
            kind: OrganelleKind::Nucleus,
            pos,
            activity: 1.0,
        });

        // Somas are added to layer l
        self.somas[l].push(Soma {
            id: j,
            layer: l,
            pos,
            stimuli: 0.1,
            atp: 1.0,
            organelles,
            prev_err: Point3::default(),
            integral_err: Point3::default(),
            region_name,
            type_name,
        });

        // Add Axon with an initial small trunk unless starting empty
        let mut axon_segments = Vec::new();
        if !start_empty {
            let (jx, jy) = (
                ((seed.wrapping_mul(6364136223846793005).rotate_left(13)) & 0xffff) as f32
                    / 32768.0
                    - 0.5,
                ((seed.wrapping_mul(1442695040888963407).rotate_left(7)) & 0xffff) as f32 / 32768.0
                    - 0.5,
            );
            let nz = ((seed >> 17) & 0xffff) as f32 / 32768.0 - 0.5;
            let mag = (synapse_offset * 0.8).max(0.004);
            let to = Point3 {
                x: (pos.x + jx * mag).clamp(-1.0, 1.0),
                y: (pos.y + jy * mag).clamp(-1.0, 1.0),
                z: (pos.z + nz * mag).clamp(-1.0, 1.0),
            };
            axon_segments.push(AxonSeg {
                from: pos,
                to,
                length: mag,
                stimuli: 1.0,
                parent_idx: None,
                syn_index: None,
                is_trunk: true,
            });
        }

        self.axons[l].push(Axon {
            neuron_layer: l,
            neuron_id: j,
            segments: axon_segments,
            stimuli: if start_empty { 0.0 } else { 1.0 },
            atp: 1.0,
            organelles: Vec::new(),
        });

        // Add Dendrite with initial trunks unless starting empty
        let mut dendrite_branches = Vec::new();
        if !start_empty {
            let total_trunks = dend_layout.total_trunks().max(1);
            for trunk_i in 0..total_trunks {
                let dend_type = dend_layout.trunk_type(trunk_i);
                let d_seed = (j as u64) ^ (0xD1D2D3D4 + trunk_i as u64);
                let (jx, jy) = (
                    ((d_seed.wrapping_mul(4101842887655102017).rotate_left(11)) & 0xffff) as f32
                        / 32768.0
                        - 0.5,
                    ((d_seed.wrapping_mul(2685821657736338717).rotate_left(5)) & 0xffff) as f32
                        / 32768.0
                        - 0.5,
                );
                let nz = ((d_seed >> 9) & 0xffff) as f32 / 32768.0 - 0.5;
                let dir = match dend_type {
                    DendriteType::Apical => Point3 {
                        x: 0.85 + 0.15 * jx,
                        y: 0.6 * jy,
                        z: 0.6 * nz,
                    }
                    .normalize(),
                    DendriteType::Basal => Point3 {
                        x: -0.2 + 0.5 * jx,
                        y: jy,
                        z: nz,
                    }
                    .normalize(),
                    DendriteType::Generic => Point3 {
                        x: jx,
                        y: jy,
                        z: nz,
                    }
                    .normalize(),
                };
                let scale = match dend_type {
                    DendriteType::Apical => 1.35,
                    DendriteType::Basal => 0.75,
                    DendriteType::Generic => 1.0,
                };
                let mag_d = (synapse_offset * 0.6 * scale).max(0.0035);
                let from = Point3 {
                    x: (pos.x + dir.x * mag_d).clamp(-1.0, 1.0),
                    y: (pos.y + dir.y * mag_d).clamp(-1.0, 1.0),
                    z: (pos.z + dir.z * mag_d).clamp(-1.0, 1.0),
                };
                dendrite_branches.push(DendSeg {
                    from,
                    to: pos,
                    length: mag_d,
                    dendrite_type: dend_type,
                    trunk_len_from_soma: mag_d,
                    stimuli: 1.0,
                    parent_idx: None,
                    syn_index: None,
                    is_trunk: true,
                });
            }
        }

        self.dendrites[l].push(Dendrite {
            neuron_layer: l,
            neuron_id: j,
            tree: DendriticTree {
                branches: dendrite_branches,
            },
            stimuli: if start_empty { 0.0 } else { 1.0 },
            atp: 1.0,
            organelles: Vec::new(),
        });
    }

    /// Build a morphology snapshot from topology and weight matrices.
    /// - `topo_layers`: hidden layer node positions.
    /// - `num_sensory_neurons`: sensory count; `num_output_neurons`: output count.
    /// - `w_in`: (H0 x S)
    /// - `w_hh_fwd[l]`: (H(l+1) x H(l)) ; `w_hh_bwd[l]`: (H(l) x H(l+1))
    /// - `w_out`: (O x H_last)
    pub fn from_weights(
        topo_layers: &Vec<Vec<crate::topology::Node3D>>,
        sensory_nodes: &Vec<crate::topology::Node3D>,
        output_nodes: &Vec<crate::topology::Node3D>,
        w_in: &ndarray::Array2<f64>,
        w_hh_fwd: &Vec<ndarray::Array2<f64>>,
        w_hh_bwd: &Vec<ndarray::Array2<f64>>,
        w_out: &ndarray::Array2<f64>,
        config: &crate::config::NetworkConfig,
        is_aarnn: bool,
    ) -> Self {
        let mut m = Morphology::new();
        let num_sensory_neurons = config.num_sensory_neurons;
        let num_output_neurons = config.num_output_neurons;
        let synapse_offset = config.synapse_offset;
        let velocity = config.aarnn_velocity;
        let max_tries = config.max_reroute_tries.max(1);
        let enforce_uniqueness = config.enforce_unique_geometry;
        let relax_iters = config.relax_iters;
        let relax_step = config.relax_step;
        let use_mid_bends = config.use_mid_bends;
        let eps = config.seg_eps.max(1e-4);
        // ------------------------------------------------------------------
        // Global uniqueness registry for sites (quantized grid)
        // Define before any calls to ensure uniqueness helpers are in scope.
        let mut used_sites: HashSet<(i32, i32, i32)> = HashSet::new();
        let quant = |p: &Point3| -> (i32, i32, i32) {
            // quantize to 1e-3 grid
            let q = 1000.0f32;
            (
                (p.x * q).round() as i32,
                (p.y * q).round() as i32,
                (p.z * q).round() as i32,
            )
        };
        // Small deterministic jitter generator in [-1,1]
        let jitter2 = |seed: u64| -> (f32, f32) {
            // simple hash → two floats
            let mut x = seed.wrapping_mul(0x9E3779B185EBCA87);
            x ^= x >> 33;
            x = x.wrapping_mul(0xff51afd7ed558ccd);
            let a = ((x as u128).wrapping_mul(48271) % 10_000) as f32 / 5000.0 - 1.0;
            let b = ((x as u128).wrapping_mul(69621) % 10_000) as f32 / 5000.0 - 1.0;
            (a.max(-1.0).min(1.0), b.max(-1.0).min(1.0))
        };
        // Ensure uniqueness of a point by jittering slightly if needed
        let mut ensure_unique_point = |p: Point3, seed: u64| -> Point3 {
            let base_key = quant(&p);
            if !used_sites.contains(&base_key) {
                used_sites.insert(base_key);
                return p;
            }
            // jitter attempts
            let tries = 8;
            for i in 1..=tries {
                let (jx, jy) = jitter2(seed.wrapping_add(i as u64));
                let mag = (synapse_offset * 0.35) * (i as f32 / tries as f32);
                let nx = (p.x + jx * mag).clamp(-1.0, 1.0);
                let ny = (p.y + jy * mag).clamp(-1.0, 1.0);
                let nz = (p.z + (jx * jy) * mag * 0.5).clamp(-1.0, 1.0);
                let cand = Point3 {
                    x: nx,
                    y: ny,
                    z: nz,
                };
                let key = quant(&cand);
                if !used_sites.contains(&key) {
                    used_sites.insert(key);
                    return cand;
                }
            }
            p
        };
        // Somas from topology
        m.somas = topo_layers
            .iter()
            .enumerate()
            .map(|(l, nodes)| {
                nodes
                    .iter()
                    .enumerate()
                    .map(|(j, n)| {
                        let p = Point3 {
                            x: n.x,
                            y: n.y,
                            z: n.z,
                        };
                        let seed = ((l as u64) << 32) ^ (j as u64) ^ 0x01020304;
                        let mut organelles = Vec::new();
                        // Nucleus at center
                        organelles.push(Organelle {
                            kind: OrganelleKind::Nucleus,
                            pos: p,
                            activity: 1.0,
                        });
                        // A few mitochondria nearby
                        for k in 0..3 {
                            let (jx, jy) = jitter2(seed.wrapping_add(k as u64));
                            organelles.push(Organelle {
                                kind: OrganelleKind::Mitochondria,
                                pos: Point3 {
                                    x: p.x + jx * 0.02,
                                    y: p.y + jy * 0.02,
                                    z: p.z + (jx * jy) * 0.01,
                                },
                                activity: 0.8,
                            });
                        }
                        // Golgi and ER
                        let (jx, jy) = jitter2(seed.wrapping_add(100));
                        organelles.push(Organelle {
                            kind: OrganelleKind::GolgiApparatus,
                            pos: Point3 {
                                x: p.x + jx * 0.015,
                                y: p.y + jy * 0.015,
                                z: p.z,
                            },
                            activity: 0.7,
                        });
                        let (jx, jy) = jitter2(seed.wrapping_add(200));
                        organelles.push(Organelle {
                            kind: OrganelleKind::EndoplasmicReticulum,
                            pos: Point3 {
                                x: p.x + jx * 0.01,
                                y: p.y + jy * 0.01,
                                z: p.z,
                            },
                            activity: 0.6,
                        });
                        Soma {
                            id: j,
                            layer: l,
                            pos: p,
                            stimuli: 0.1,
                            atp: 1.0,
                            organelles,
                            prev_err: Point3::default(),
                            integral_err: Point3::default(),
                            region_name: n.region_name.clone(),
                            type_name: n.type_name.clone(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        m.update_skull_membrane(config, 1.0);
        // Axons/dendrites with minimal non-zero distinct endpoints from soma to avoid coincident geometry.
        // For AARNN, start hidden neurons with empty morphology so connections form over time.
        if is_aarnn {
            m.axons = topo_layers
                .iter()
                .enumerate()
                .map(|(l, nodes)| {
                    nodes
                        .iter()
                        .enumerate()
                        .map(|(j, _n)| Axon {
                            neuron_layer: l,
                            neuron_id: j,
                            segments: Vec::new(),
                            stimuli: 0.0,
                            atp: 1.0,
                            organelles: Vec::new(),
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            m.dendrites = topo_layers
                .iter()
                .enumerate()
                .map(|(l, nodes)| {
                    nodes
                        .iter()
                        .enumerate()
                        .map(|(j, _n)| Dendrite {
                            neuron_layer: l,
                            neuron_id: j,
                            tree: DendriticTree {
                                branches: Vec::new(),
                            },
                            stimuli: 0.0,
                            atp: 1.0,
                            organelles: Vec::new(),
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
        } else {
            m.axons = topo_layers
                .iter()
                .enumerate()
                .map(|(l, nodes)| {
                    nodes
                        .iter()
                        .enumerate()
                        .map(|(j, n)| {
                            let p = Point3 {
                                x: n.x,
                                y: n.y,
                                z: n.z,
                            };
                            // Seed a tiny direction using hash(l,j)
                            let seed = ((l as u64) << 32) ^ (j as u64) ^ 0xA1A2A3A4;
                            let (jx, jy) = (
                                ((seed.wrapping_mul(6364136223846793005).rotate_left(13)) & 0xffff)
                                    as f32
                                    / 32768.0
                                    - 0.5,
                                ((seed.wrapping_mul(1442695040888963407).rotate_left(7)) & 0xffff)
                                    as f32
                                    / 32768.0
                                    - 0.5,
                            );
                            let nz = ((seed >> 17) & 0xffff) as f32 / 32768.0 - 0.5;
                            let mag = (synapse_offset * 0.8).max(0.004);
                            let mut to = Point3 {
                                x: (p.x + jx * mag).clamp(-1.0, 1.0),
                                y: (p.y + jy * mag).clamp(-1.0, 1.0),
                                z: (p.z + nz * mag).clamp(-1.0, 1.0),
                            };
                            // ensure uniqueness w.r.t. global used_sites
                            to = ensure_unique_point(to, seed);
                            Axon {
                                neuron_layer: l,
                                neuron_id: j,
                                segments: vec![AxonSeg {
                                    from: p,
                                    to,
                                    length: mag,
                                    stimuli: 1.0,
                                    parent_idx: None,
                                    syn_index: None,
                                    is_trunk: true,
                                }],
                                stimuli: 1.0,
                                atp: 1.0,
                                organelles: Vec::new(),
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            m.dendrites = topo_layers
                .iter()
                .enumerate()
                .map(|(l, nodes)| {
                    nodes
                        .iter()
                        .enumerate()
                        .map(|(j, n)| {
                            let p = Point3 {
                                x: n.x,
                                y: n.y,
                                z: n.z,
                            };
                            let layout = dendrite_layout_for_type(n.type_name.as_deref());
                            let mut branches = Vec::new();
                            let total_trunks = layout.total_trunks().max(1);
                            for trunk_i in 0..total_trunks {
                                let dend_type = layout.trunk_type(trunk_i);
                                let seed = ((l as u64) << 32)
                                    ^ (j as u64)
                                    ^ (0xB1B2B3B4u64 + trunk_i as u64);
                                let (jx, jy) = (
                                    ((seed.wrapping_mul(4101842887655102017).rotate_left(11))
                                        & 0xffff) as f32
                                        / 32768.0
                                        - 0.5,
                                    ((seed.wrapping_mul(2685821657736338717).rotate_left(5))
                                        & 0xffff) as f32
                                        / 32768.0
                                        - 0.5,
                                );
                                let nz = ((seed >> 9) & 0xffff) as f32 / 32768.0 - 0.5;
                                let dir = match dend_type {
                                    DendriteType::Apical => Point3 {
                                        x: 0.85 + 0.15 * jx,
                                        y: 0.6 * jy,
                                        z: 0.6 * nz,
                                    }
                                    .normalize(),
                                    DendriteType::Basal => Point3 {
                                        x: -0.2 + 0.5 * jx,
                                        y: jy,
                                        z: nz,
                                    }
                                    .normalize(),
                                    DendriteType::Generic => Point3 {
                                        x: jx,
                                        y: jy,
                                        z: nz,
                                    }
                                    .normalize(),
                                };
                                let mag =
                                    (synapse_offset * 0.6 * trunk_scale_for(dend_type, config))
                                        .max(0.0035);
                                let mut q = Point3 {
                                    x: (p.x + dir.x * mag).clamp(-1.0, 1.0),
                                    y: (p.y + dir.y * mag).clamp(-1.0, 1.0),
                                    z: (p.z + dir.z * mag).clamp(-1.0, 1.0),
                                };
                                q = ensure_unique_point(q, seed);
                                branches.push(DendSeg {
                                    from: q,
                                    to: p,
                                    length: mag,
                                    dendrite_type: dend_type,
                                    trunk_len_from_soma: mag,
                                    stimuli: 1.0,
                                    parent_idx: None,
                                    syn_index: None,
                                    is_trunk: true,
                                });
                            }
                            Dendrite {
                                neuron_layer: l,
                                neuron_id: j,
                                tree: DendriticTree { branches },
                                stimuli: 1.0,
                                atp: 1.0,
                                organelles: Vec::new(),
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
        }

        // Helper to compute delay (ms) from euclidean distance in normalized units
        let to_delay = |dist: f32| -> f32 {
            let v = velocity.max(1e-6);
            dist / v
        };

        // (unique helpers already defined above)

        let (rx, ry, rz, cx, cy, cz) = if let Some(sm) = &m.skull_membrane {
            let r = sm.radii.unwrap_or((sm.radius, sm.radius, sm.radius));
            (r.0, r.1, r.2, sm.center.x, sm.center.y, sm.center.z)
        } else {
            (0.5, 0.5, 0.5, 0.0, 0.0, 0.0)
        };

        let sensory_pos = if !sensory_nodes.is_empty() {
            sensory_nodes
                .iter()
                .map(|n| Point3 {
                    x: n.x,
                    y: n.y,
                    z: n.z,
                })
                .collect::<Vec<_>>()
        } else {
            (0..num_sensory_neurons)
                .map(|i| {
                    if is_aarnn {
                        let angle =
                            (i as f32) * 2.0 * std::f32::consts::PI / (num_sensory_neurons as f32);
                        let r_base = (rx.min(rz) * 0.4).max(0.05);
                        Point3 {
                            x: cx - rx * 0.4 + angle.cos() * r_base * 0.2,
                            y: cy - ry,
                            z: cz + angle.sin() * r_base,
                        }
                    } else {
                        let (y, z) = if num_sensory_neurons > 1 {
                            let angle = (i as f32) * 2.0 * std::f32::consts::PI
                                / (num_sensory_neurons as f32);
                            let radius = 0.65;
                            (radius * angle.cos(), radius * angle.sin())
                        } else {
                            (0.0, 0.0)
                        };
                        Point3 { x: -0.7, y, z }
                    }
                })
                .collect::<Vec<_>>()
        };

        m.sensory_somas = sensory_pos
            .iter()
            .enumerate()
            .map(|(i, &pos)| {
                let seed = (i as u64) ^ 0x05060708;
                let mut organelles = Vec::new();
                organelles.push(Organelle {
                    kind: OrganelleKind::Nucleus,
                    pos,
                    activity: 1.0,
                });
                for k in 0..2 {
                    let (jx, jy) = jitter2(seed.wrapping_add(k as u64));
                    organelles.push(Organelle {
                        kind: OrganelleKind::Mitochondria,
                        pos: Point3 {
                            x: pos.x + jx * 0.02,
                            y: pos.y + jy * 0.02,
                            z: pos.z + (jx * jy) * 0.01,
                        },
                        activity: 0.8,
                    });
                }
                // Golgi and ER
                let (jx, jy) = jitter2(seed.wrapping_add(100));
                organelles.push(Organelle {
                    kind: OrganelleKind::GolgiApparatus,
                    pos: Point3 {
                        x: pos.x + jx * 0.015,
                        y: pos.y + jy * 0.015,
                        z: pos.z,
                    },
                    activity: 0.7,
                });
                let (jx, jy) = jitter2(seed.wrapping_add(200));
                organelles.push(Organelle {
                    kind: OrganelleKind::EndoplasmicReticulum,
                    pos: Point3 {
                        x: pos.x + jx * 0.01,
                        y: pos.y + jy * 0.01,
                        z: pos.z,
                    },
                    activity: 0.6,
                });
                let (region_name, type_name) = if i < sensory_nodes.len() {
                    (
                        sensory_nodes[i].region_name.clone(),
                        sensory_nodes[i].type_name.clone(),
                    )
                } else {
                    (None, None)
                };
                Soma {
                    id: i,
                    layer: usize::MAX,
                    pos,
                    stimuli: 0.1,
                    atp: 1.0,
                    organelles,
                    prev_err: Point3::default(),
                    integral_err: Point3::default(),
                    region_name,
                    type_name,
                }
            })
            .collect();

        // Output positions from Topology3D
        let out_pos = if !output_nodes.is_empty() {
            output_nodes
                .iter()
                .map(|n| Point3 {
                    x: n.x,
                    y: n.y,
                    z: n.z,
                })
                .collect::<Vec<_>>()
        } else {
            (0..num_output_neurons)
                .map(|k| {
                    if is_aarnn {
                        let angle =
                            (k as f32) * 2.0 * std::f32::consts::PI / (num_output_neurons as f32);
                        let r_base = (rx.min(rz) * 0.4).max(0.05);
                        Point3 {
                            x: cx + rx * 0.4 + angle.cos() * r_base * 0.2,
                            y: cy - ry,
                            z: cz + angle.sin() * r_base,
                        }
                    } else {
                        let (y, z) = if num_output_neurons > 1 {
                            let angle = (k as f32) * 2.0 * std::f32::consts::PI
                                / (num_output_neurons as f32);
                            let radius = 0.65;
                            (radius * angle.cos(), radius * angle.sin())
                        } else {
                            (0.0, 0.0)
                        };
                        Point3 { x: 1.0, y, z }
                    }
                })
                .collect::<Vec<_>>()
        };

        m.output_somas = out_pos
            .iter()
            .enumerate()
            .map(|(k, &pos)| {
                let seed = (k as u64) ^ 0x090A0B0C;
                let mut organelles = Vec::new();
                organelles.push(Organelle {
                    kind: OrganelleKind::Nucleus,
                    pos,
                    activity: 1.0,
                });
                for i in 0..2 {
                    let (jx, jy) = jitter2(seed.wrapping_add(i as u64));
                    organelles.push(Organelle {
                        kind: OrganelleKind::Mitochondria,
                        pos: Point3 {
                            x: pos.x + jx * 0.02,
                            y: pos.y + jy * 0.02,
                            z: pos.z + (jx * jy) * 0.01,
                        },
                        activity: 0.8,
                    });
                }
                // Golgi and ER
                let (jx, jy) = jitter2(seed.wrapping_add(100));
                organelles.push(Organelle {
                    kind: OrganelleKind::GolgiApparatus,
                    pos: Point3 {
                        x: pos.x + jx * 0.015,
                        y: pos.y + jy * 0.015,
                        z: pos.z,
                    },
                    activity: 0.7,
                });
                let (jx, jy) = jitter2(seed.wrapping_add(200));
                organelles.push(Organelle {
                    kind: OrganelleKind::EndoplasmicReticulum,
                    pos: Point3 {
                        x: pos.x + jx * 0.01,
                        y: pos.y + jy * 0.01,
                        z: pos.z,
                    },
                    activity: 0.6,
                });
                let (region_name, type_name) = if k < output_nodes.len() {
                    (
                        output_nodes[k].region_name.clone(),
                        output_nodes[k].type_name.clone(),
                    )
                } else {
                    (None, None)
                };
                Soma {
                    id: k,
                    layer: usize::MAX - 1,
                    pos,
                    stimuli: 0.1,
                    atp: 1.0,
                    organelles,
                    prev_err: Point3::default(),
                    integral_err: Point3::default(),
                    region_name,
                    type_name,
                }
            })
            .collect();

        m.sensory_axons = m
            .sensory_somas
            .iter()
            .map(|s| {
                let p = s.pos;
                let seed = (s.id as u64) ^ 0xC1C2C3C4;
                let (jx, jy) = (
                    ((seed.wrapping_mul(6364136223846793005).rotate_left(13)) & 0xffff) as f32
                        / 32768.0
                        - 0.5,
                    ((seed.wrapping_mul(1442695040888963407).rotate_left(7)) & 0xffff) as f32
                        / 32768.0
                        - 0.5,
                );
                let nz = ((seed >> 17) & 0xffff) as f32 / 32768.0 - 0.5;
                let mag = (synapse_offset * 0.8).max(0.004);
                let mut to = Point3 {
                    x: (p.x + jx * mag).clamp(-1.0, 1.0),
                    y: (p.y + jy * mag).clamp(-1.0, 1.0),
                    z: (p.z + nz * mag).clamp(-1.0, 1.0),
                };
                to = ensure_unique_point(to, seed);
                Axon {
                    neuron_layer: usize::MAX,
                    neuron_id: s.id,
                    segments: vec![AxonSeg {
                        from: p,
                        to,
                        length: mag,
                        stimuli: 1.0,
                        parent_idx: None,
                        syn_index: None,
                        is_trunk: true,
                    }],
                    stimuli: 1.0,
                    atp: 1.0,
                    organelles: Vec::new(),
                }
            })
            .collect();

        m.sensory_dendrites = m
            .sensory_somas
            .iter()
            .map(|s| {
                let p = s.pos;
                let mut branches = Vec::new();
                for trunk_i in 0..2 {
                    let seed = (s.id as u64) ^ (0xD1D2D3D4 + trunk_i);
                    let (jx, jy) = (
                        ((seed.wrapping_mul(4101842887655102017).rotate_left(11)) & 0xffff) as f32
                            / 32768.0
                            - 0.5,
                        ((seed.wrapping_mul(2685821657736338717).rotate_left(5)) & 0xffff) as f32
                            / 32768.0
                            - 0.5,
                    );
                    let nz = ((seed >> 9) & 0xffff) as f32 / 32768.0 - 0.5;
                    let mag = (synapse_offset * 0.6).max(0.0035);
                    let mut q = Point3 {
                        x: (p.x + jx * mag).clamp(-1.0, 1.0),
                        y: (p.y + jy * mag).clamp(-1.0, 1.0),
                        z: (p.z + nz * mag).clamp(-1.0, 1.0),
                    };
                    q = ensure_unique_point(q, seed);
                    branches.push(DendSeg {
                        from: q,
                        to: p,
                        length: mag,
                        dendrite_type: DendriteType::Generic,
                        trunk_len_from_soma: mag,
                        stimuli: 1.0,
                        parent_idx: None,
                        syn_index: None,
                        is_trunk: true,
                    });
                }
                Dendrite {
                    neuron_layer: usize::MAX,
                    neuron_id: s.id,
                    tree: DendriticTree { branches },
                    stimuli: 1.0,
                    atp: 1.0,
                    organelles: Vec::new(),
                }
            })
            .collect();

        m.output_axons = m
            .output_somas
            .iter()
            .map(|s| {
                let p = s.pos;
                let seed = (s.id as u64) ^ 0xE1E2E3E4;
                let (jx, jy) = (
                    ((seed.wrapping_mul(6364136223846793005).rotate_left(13)) & 0xffff) as f32
                        / 32768.0
                        - 0.5,
                    ((seed.wrapping_mul(1442695040888963407).rotate_left(7)) & 0xffff) as f32
                        / 32768.0
                        - 0.5,
                );
                let nz = ((seed >> 17) & 0xffff) as f32 / 32768.0 - 0.5;
                let mag = (synapse_offset * 0.8).max(0.004);
                let mut to = Point3 {
                    x: (p.x + jx * mag).clamp(-1.0, 1.0),
                    y: (p.y + jy * mag).clamp(-1.0, 1.0),
                    z: (p.z + nz * mag).clamp(-1.0, 1.0),
                };
                to = ensure_unique_point(to, seed);
                Axon {
                    neuron_layer: usize::MAX - 1,
                    neuron_id: s.id,
                    segments: vec![AxonSeg {
                        from: p,
                        to,
                        length: mag,
                        stimuli: 1.0,
                        parent_idx: None,
                        syn_index: None,
                        is_trunk: true,
                    }],
                    stimuli: 1.0,
                    atp: 1.0,
                    organelles: Vec::new(),
                }
            })
            .collect();

        m.output_dendrites = m
            .output_somas
            .iter()
            .map(|s| {
                let p = s.pos;
                let mut branches = Vec::new();
                for trunk_i in 0..2 {
                    let seed = (s.id as u64) ^ (0xF1F2F3F4 + trunk_i);
                    let (jx, jy) = (
                        ((seed.wrapping_mul(4101842887655102017).rotate_left(11)) & 0xffff) as f32
                            / 32768.0
                            - 0.5,
                        ((seed.wrapping_mul(2685821657736338717).rotate_left(5)) & 0xffff) as f32
                            / 32768.0
                            - 0.5,
                    );
                    let nz = ((seed >> 9) & 0xffff) as f32 / 32768.0 - 0.5;
                    let mag = (synapse_offset * 0.6).max(0.0035);
                    let mut q = Point3 {
                        x: (p.x + jx * mag).clamp(-1.0, 1.0),
                        y: (p.y + jy * mag).clamp(-1.0, 1.0),
                        z: (p.z + nz * mag).clamp(-1.0, 1.0),
                    };
                    q = ensure_unique_point(q, seed);
                    branches.push(DendSeg {
                        from: q,
                        to: p,
                        length: mag,
                        dendrite_type: DendriteType::Generic,
                        trunk_len_from_soma: mag,
                        stimuli: 1.0,
                        parent_idx: None,
                        syn_index: None,
                        is_trunk: true,
                    });
                }
                Dendrite {
                    neuron_layer: usize::MAX - 1,
                    neuron_id: s.id,
                    tree: DendriticTree { branches },
                    stimuli: 1.0,
                    atp: 1.0,
                    organelles: Vec::new(),
                }
            })
            .collect();

        let num_layers = topo_layers.len();
        let in_l = if is_aarnn {
            if num_layers > 1 { 1 } else { 0 }
        } else {
            0
        };
        let out_l = if is_aarnn {
            if num_layers > 4 {
                4
            } else {
                num_layers.saturating_sub(1)
            }
        } else {
            num_layers.saturating_sub(1)
        };

        // Synapses: In (S -> target_in_layer)
        if !is_aarnn {
            if let Some(layer_in) = topo_layers.get(in_l) {
                let h_in_count = layer_in.len();
                for j in 0..h_in_count {
                    for i in 0..num_sensory_neurons.min(w_in.ncols()) {
                        let w = w_in[(j, i)];
                        if w == 0.0 {
                            continue;
                        }
                        let pre_soma = sensory_pos.get(i).cloned().unwrap_or(Point3 {
                            x: -1.2,
                            y: 0.0,
                            z: 0.0,
                        });
                        let post_soma = Point3 {
                            x: layer_in[j].x,
                            y: layer_in[j].y,
                            z: layer_in[j].z,
                        };
                        let dx = pre_soma.x - post_soma.x;
                        let dy = pre_soma.y - post_soma.y;
                        let dz = pre_soma.z - post_soma.z;
                        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                        // offset sites along connection direction: meet in the middle
                        let mut pre = Point3 {
                            x: pre_soma.x - dx * 0.5,
                            y: pre_soma.y - dy * 0.5,
                            z: pre_soma.z - dz * 0.5,
                        };
                        let mut post = Point3 {
                            x: post_soma.x + dx * 0.5,
                            y: post_soma.y + dy * 0.5,
                            z: post_soma.z + dz * 0.5,
                        };
                        // ensure uniqueness with small jitter
                        pre = ensure_unique_point(pre, ((i as u64) << 32) ^ (j as u64) ^ 0x11);
                        post = ensure_unique_point(post, ((i as u64) << 32) ^ (j as u64) ^ 0x12);
                        m.synapses.push(Synapse {
                            kind: SynKind::In,
                            pre_layer: -1,
                            pre_id: i,
                            post_layer: in_l as isize,
                            post_id: j,
                            pre_site: pre,
                            post_site: post,
                            axon_seg_idx: None,
                            dend_seg_idx: None,
                            bend: None,
                            weight: w,
                            p_release: 1.0,
                            delay_ms: to_delay(dist),
                            stimuli: 1.0,
                        });
                    }
                }
            }
        }

        // Synapses: Hidden forward/backward
        let l_count_topo = topo_layers.len();
        if !is_aarnn {
            for l in 0..l_count_topo.saturating_sub(1) {
                let rows = w_hh_fwd.get(l).map(|a| a.nrows()).unwrap_or(0);
                let cols = w_hh_fwd.get(l).map(|a| a.ncols()).unwrap_or(0);
                for j in 0..rows {
                    for i in 0..cols {
                        let w = w_hh_fwd[l][(j, i)];
                        if w == 0.0 {
                            continue;
                        }
                        let pre_node = topo_layers.get(l).and_then(|v| v.get(i));
                        let post_node = topo_layers.get(l + 1).and_then(|v| v.get(j));
                        if let (Some(a), Some(b)) = (pre_node, post_node) {
                            let pre_soma = Point3 {
                                x: a.x,
                                y: a.y,
                                z: a.z,
                            };
                            let post_soma = Point3 {
                                x: b.x,
                                y: b.y,
                                z: b.z,
                            };
                            let dx = pre_soma.x - post_soma.x;
                            let dy = pre_soma.y - post_soma.y;
                            let dz = pre_soma.z - post_soma.z;
                            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                            let mut pre = Point3 {
                                x: pre_soma.x - dx * 0.5,
                                y: pre_soma.y - dy * 0.5,
                                z: pre_soma.z - dz * 0.5,
                            };
                            let mut post = Point3 {
                                x: post_soma.x + dx * 0.5,
                                y: post_soma.y + dy * 0.5,
                                z: post_soma.z + dz * 0.5,
                            };
                            pre = ensure_unique_point(
                                pre,
                                ((l as u64) << 40) ^ ((i as u64) << 20) ^ (j as u64) ^ 0x21,
                            );
                            post = ensure_unique_point(
                                post,
                                ((l as u64) << 40) ^ ((i as u64) << 20) ^ (j as u64) ^ 0x22,
                            );
                            m.synapses.push(Synapse {
                                kind: SynKind::HiddenFwd,
                                pre_layer: l as isize,
                                pre_id: i,
                                post_layer: (l + 1) as isize,
                                post_id: j,
                                pre_site: pre,
                                post_site: post,
                                axon_seg_idx: None,
                                dend_seg_idx: None,
                                bend: None,
                                weight: w,
                                p_release: 1.0,
                                delay_ms: to_delay(dist),
                                stimuli: 1.0,
                            });
                        }
                    }
                }
                // Backward matrix
                let rows_b = w_hh_bwd.get(l).map(|a| a.nrows()).unwrap_or(0);
                let cols_b = w_hh_bwd.get(l).map(|a| a.ncols()).unwrap_or(0);
                for i in 0..rows_b {
                    for j in 0..cols_b {
                        let w = w_hh_bwd[l][(i, j)];
                        if w == 0.0 {
                            continue;
                        }
                        let pre_node = topo_layers.get(l + 1).and_then(|v| v.get(j));
                        let post_node = topo_layers.get(l).and_then(|v| v.get(i));
                        if let (Some(a), Some(b)) = (pre_node, post_node) {
                            let pre_soma = Point3 {
                                x: a.x,
                                y: a.y,
                                z: a.z,
                            };
                            let post_soma = Point3 {
                                x: b.x,
                                y: b.y,
                                z: b.z,
                            };
                            let dx = pre_soma.x - post_soma.x;
                            let dy = pre_soma.y - post_soma.y;
                            let dz = pre_soma.z - post_soma.z;
                            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                            let mut pre = Point3 {
                                x: pre_soma.x - dx * 0.5,
                                y: pre_soma.y - dy * 0.5,
                                z: pre_soma.z - dz * 0.5,
                            };
                            let mut post = Point3 {
                                x: post_soma.x + dx * 0.5,
                                y: post_soma.y + dy * 0.5,
                                z: post_soma.z + dz * 0.5,
                            };
                            pre = ensure_unique_point(
                                pre,
                                ((l as u64) << 40) ^ ((i as u64) << 20) ^ (j as u64) ^ 0x31,
                            );
                            post = ensure_unique_point(
                                post,
                                ((l as u64) << 40) ^ ((i as u64) << 20) ^ (j as u64) ^ 0x32,
                            );
                            m.synapses.push(Synapse {
                                kind: SynKind::HiddenBwd,
                                pre_layer: (l + 1) as isize,
                                pre_id: j,
                                post_layer: l as isize,
                                post_id: i,
                                pre_site: pre,
                                post_site: post,
                                axon_seg_idx: None,
                                dend_seg_idx: None,
                                bend: None,
                                weight: w,
                                p_release: 1.0,
                                delay_ms: to_delay(dist),
                                stimuli: 1.0,
                            });
                        }
                    }
                }
            }
        }

        // Synapses: Out (target_out_layer -> O)
        if !is_aarnn {
            if let Some(source_nodes) = topo_layers.get(out_l) {
                let h_out_count = source_nodes.len();
                for k in 0..num_output_neurons.min(w_out.nrows()) {
                    for j in 0..h_out_count.min(w_out.ncols()) {
                        let w = w_out[(k, j)];
                        if w == 0.0 {
                            continue;
                        }
                        let pre_node = &source_nodes[j];
                        let post_soma = out_pos.get(k).cloned().unwrap_or(Point3 {
                            x: 1.2,
                            y: 0.0,
                            z: 0.0,
                        });
                        let pre_soma = Point3 {
                            x: pre_node.x,
                            y: pre_node.y,
                            z: pre_node.z,
                        };
                        let dx = pre_soma.x - post_soma.x;
                        let dy = pre_soma.y - post_soma.y;
                        let dz = pre_soma.z - post_soma.z;
                        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                        let mut pre = Point3 {
                            x: pre_soma.x - dx * 0.5,
                            y: pre_soma.y - dy * 0.5,
                            z: pre_soma.z - dz * 0.5,
                        };
                        let mut post = Point3 {
                            x: post_soma.x + dx * 0.5,
                            y: post_soma.y + dy * 0.5,
                            z: post_soma.z + dz * 0.5,
                        };
                        pre = ensure_unique_point(pre, ((j as u64) << 20) ^ (k as u64) ^ 0x41);
                        post = ensure_unique_point(post, ((j as u64) << 20) ^ (k as u64) ^ 0x42);
                        m.synapses.push(Synapse {
                            kind: SynKind::Out,
                            pre_layer: out_l as isize,
                            pre_id: j,
                            post_layer: topo_layers.len() as isize,
                            post_id: k,
                            pre_site: pre,
                            post_site: post,
                            axon_seg_idx: None,
                            dend_seg_idx: None,
                            bend: None,
                            weight: w,
                            p_release: 1.0,
                            delay_ms: to_delay(dist),
                            stimuli: 1.0,
                        });
                    }
                }
            }
        }

        // ------------------------------------------------------------------
        // Build branched dendrites and single-axon-with-branches per neuron.
        // AARNN requirement:
        //  - Dendrites: multiple branches that consolidate to the soma.
        //  - Axon: exactly one axon per neuron (from hillock) that may branch.
        // We keep public types unchanged and only enrich `segments`/`branches`.
        {
            // Gather incoming and outgoing synapse sites per neuron
            let mut incoming: Vec<Vec<Vec<(Point3, usize)>>> = vec![Vec::new(); topo_layers.len()];
            let mut outgoing: Vec<Vec<Vec<(Point3, usize)>>> = vec![Vec::new(); topo_layers.len()];
            for (l, nodes) in topo_layers.iter().enumerate() {
                incoming[l] = vec![Vec::new(); nodes.len()];
                outgoing[l] = vec![Vec::new(); nodes.len()];
            }
            for (si, s) in m.synapses.iter().enumerate() {
                match s.kind {
                    SynKind::In => {
                        // post is in hidden layer 0
                        if s.post_layer >= 0 {
                            let l = s.post_layer as usize;
                            if l < incoming.len() && s.post_id < incoming[l].len() {
                                incoming[l][s.post_id].push((s.post_site, si));
                            }
                        }
                    }
                    SynKind::HiddenFwd => {
                        // pre: layer l, post: l+1
                        if s.pre_layer >= 0 {
                            let lpre = s.pre_layer as usize;
                            if lpre < outgoing.len() && s.pre_id < outgoing[lpre].len() {
                                outgoing[lpre][s.pre_id].push((s.pre_site, si));
                            }
                        }
                        if s.post_layer >= 0 {
                            let lpost = s.post_layer as usize;
                            if lpost < incoming.len() && s.post_id < incoming[lpost].len() {
                                incoming[lpost][s.post_id].push((s.post_site, si));
                            }
                        }
                    }
                    SynKind::HiddenBwd | SynKind::HiddenRec => {
                        // pre: layer l+1 (Bwd) or l (Rec), post: l
                        if s.pre_layer >= 0 {
                            let lpre = s.pre_layer as usize;
                            if lpre < outgoing.len() && s.pre_id < outgoing[lpre].len() {
                                outgoing[lpre][s.pre_id].push((s.pre_site, si));
                            }
                        }
                        if s.post_layer >= 0 {
                            let lpost = s.post_layer as usize;
                            if lpost < incoming.len() && s.post_id < incoming[lpost].len() {
                                incoming[lpost][s.post_id].push((s.post_site, si));
                            }
                        }
                    }
                    SynKind::Out => {
                        // pre is last hidden layer neuron
                        if s.pre_layer >= 0 {
                            let lpre = s.pre_layer as usize;
                            if lpre < outgoing.len() && s.pre_id < outgoing[lpre].len() {
                                outgoing[lpre][s.pre_id].push((s.pre_site, si));
                            }
                        }
                    }
                }
            }
            // Rebuild axon and dendrite structures using hubs near soma
            for l in 0..topo_layers.len() {
                let nodes = &topo_layers[l];
                for j in 0..nodes.len() {
                    let soma = Point3 {
                        x: nodes[j].x,
                        y: nodes[j].y,
                        z: nodes[j].z,
                    };
                    // Axon: keep first small segment as trunk to hillock if exists; else create a tiny one
                    let hillock = {
                        let mut base = if let Some(ax) = m.axons.get(l).and_then(|v| v.get(j)) {
                            if let Some(seg0) = ax.segments.get(0) {
                                seg0.to
                            } else {
                                soma
                            }
                        } else {
                            soma
                        };
                        // ensure uniqueness (reuse seed scheme)
                        let seed = ((l as u64) << 32) ^ (j as u64) ^ 0xA1A2A3A4;
                        base = ensure_unique_point(base, seed);
                        base
                    };
                    let mut ax_segments: Vec<AxonSeg> = Vec::new();
                    if hillock.x != soma.x || hillock.y != soma.y || hillock.z != soma.z {
                        let dx = hillock.x - soma.x;
                        let dy = hillock.y - soma.y;
                        let dz = hillock.z - soma.z;
                        let len = (dx * dx + dy * dy + dz * dz).sqrt();
                        ax_segments.push(AxonSeg {
                            from: soma,
                            to: hillock,
                            length: len,
                            stimuli: 1.0,
                            parent_idx: None,
                            syn_index: None,
                            is_trunk: true,
                        });
                    }
                    for (k, &(p, si)) in outgoing[l][j].iter().enumerate() {
                        // connect hillock to each pre_site
                        let mut endp = p;
                        let seed =
                            ((l as u64) << 40) ^ ((j as u64) << 20) ^ (k as u64) ^ 0xA55A5AA5;
                        endp = ensure_unique_point(endp, seed);
                        let dx = endp.x - hillock.x;
                        let dy = endp.y - hillock.y;
                        let dz = endp.z - hillock.z;
                        let len = (dx * dx + dy * dy + dz * dz).sqrt();
                        let asi = ax_segments.len();
                        ax_segments.push(AxonSeg {
                            from: hillock,
                            to: endp,
                            length: len,
                            stimuli: 1.0,
                            parent_idx: Some(0),
                            syn_index: Some(si),
                            is_trunk: false,
                        });
                        if si < m.synapses.len() {
                            m.synapses[si].axon_seg_idx = Some(asi);
                        }
                    }
                    if let Some(layer_axons) = m.axons.get_mut(l) {
                        if let Some(ax) = layer_axons.get_mut(j) {
                            ax.segments = ax_segments;
                        }
                    }

                    // Dendrites: pick a consolidation hub slightly away from soma toward centroid of incoming sites
                    let dend_branches = {
                        let pts = &incoming[l][j];
                        let mut branches: Vec<DendSeg> = Vec::new();
                        if !pts.is_empty() {
                            // Distribute incoming sites among trunks based on neuron cell structure.
                            let layout = dendrite_layout_for_type(nodes[j].type_name.as_deref());
                            let num_trunks = layout.total_trunks().max(1);
                            for trunk_i in 0..num_trunks {
                                let dend_type = layout.trunk_type(trunk_i);
                                let group: Vec<_> = pts
                                    .iter()
                                    .enumerate()
                                    .filter(|(k, _)| k % num_trunks == trunk_i)
                                    .map(|(_, p)| p)
                                    .collect();
                                if group.is_empty() {
                                    continue;
                                }

                                let mut cx = 0.0;
                                let mut cy = 0.0;
                                let mut cz = 0.0;
                                for &(p, _) in &group {
                                    cx += p.x;
                                    cy += p.y;
                                    cz += p.z;
                                }
                                let invn = 1.0f32 / (group.len() as f32);
                                cx *= invn;
                                cy *= invn;
                                cz *= invn;

                                // Hub between soma and centroid
                                let alpha =
                                    (0.35f32 * trunk_scale_for(dend_type, config)).clamp(0.1, 0.85);
                                let mut hub = Point3 {
                                    x: soma.x * (1.0 - alpha) + cx * alpha,
                                    y: soma.y * (1.0 - alpha) + cy * alpha,
                                    z: soma.z * (1.0 - alpha) + cz * alpha,
                                };
                                hub = ensure_unique_point(
                                    hub,
                                    ((l as u64) << 32) ^ (j as u64) ^ 0xBEEFABCD ^ (trunk_i as u64),
                                );

                                let trunk_idx = branches.len();
                                let dx = soma.x - hub.x;
                                let dy = soma.y - hub.y;
                                let dz = soma.z - hub.z;
                                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                                branches.push(DendSeg {
                                    from: hub,
                                    to: soma,
                                    length: len,
                                    dendrite_type: dend_type,
                                    trunk_len_from_soma: len,
                                    stimuli: 1.0,
                                    parent_idx: None,
                                    syn_index: None,
                                    is_trunk: true,
                                });

                                for (k, &&(p, si)) in group.iter().enumerate() {
                                    let mut start = p;
                                    let seed = ((l as u64) << 40)
                                        ^ ((j as u64) << 20)
                                        ^ (k as u64)
                                        ^ 0xB55B5BB5
                                        ^ (trunk_i as u64);
                                    start = ensure_unique_point(start, seed);
                                    let dx = hub.x - start.x;
                                    let dy = hub.y - start.y;
                                    let dz = hub.z - start.z;
                                    let len = (dx * dx + dy * dy + dz * dz).sqrt();
                                    let trunk_len = branches[trunk_idx].trunk_len_from_soma;
                                    let dsi = branches.len();
                                    branches.push(DendSeg {
                                        from: start,
                                        to: hub,
                                        length: len,
                                        dendrite_type: dend_type,
                                        trunk_len_from_soma: trunk_len,
                                        stimuli: 1.0,
                                        parent_idx: Some(trunk_idx),
                                        syn_index: Some(si),
                                        is_trunk: false,
                                    });
                                    if si < m.synapses.len() {
                                        m.synapses[si].dend_seg_idx = Some(dsi);
                                    }
                                }
                            }
                        } else {
                            // keep a minimal stub for visibility (existing first segment if present)
                            if let Some(d) = m.dendrites.get(l).and_then(|v| v.get(j)) {
                                branches.extend(d.tree.branches.iter().cloned());
                            }
                        }
                        branches
                    };
                    if let Some(layer_dends) = m.dendrites.get_mut(l) {
                        if let Some(d) = layer_dends.get_mut(j) {
                            d.tree.branches = dend_branches;
                        }
                    }
                }
            }
        }

        if enforce_uniqueness {
            // ------------------------------------------------------------------
            // Post-process to avoid connections occupying the same physical space.
            // Use existing synapse_offset as a base nudge scale; eps for distances
            let eps2 = eps * eps;
            let n_syn = m.synapses.len();

            // Parallel geometry enforcement: for each synapse i, check against all j.
            // Note: This is an approximation of the serial relaxation, but safe for parallel execution
            // if we only mutate synapse i.
            #[cfg(feature = "parallel")]
            if n_syn > 128 {
                let syn_ptr = m.synapses.as_ptr() as usize;
                m.synapses
                    .par_iter_mut()
                    .enumerate()
                    .for_each(|(i, syn_i)| {
                        // Safety: We access other synapses as read-only.
                        // This is safe because Rayon ensures syn_i is unique.
                        let other_syns =
                            unsafe { std::slice::from_raw_parts(syn_ptr as *const Synapse, n_syn) };

                        let a0 = syn_i.pre_site;
                        let a1 = syn_i.post_site;

                        for j in 0..n_syn {
                            if i == j {
                                continue;
                            }
                            let b0 = other_syns[j].pre_site;
                            let b1 = other_syns[j].post_site;

                            if !aabb_overlap(&a0, &a1, &b0, &b1, eps) {
                                continue;
                            }
                            let dist2 = seg_seg_min_dist_sq(a0, a1, b0, b1);
                            if dist2 < eps2 || near_colinear_overlap(a0, a1, b0, b1) {
                                let seed = ((syn_i.pre_id as u64) << 20)
                                    ^ (syn_i.post_id as u64)
                                    ^ (i as u64)
                                    ^ 0x55AACC55;
                                let (jx, jy) = jitter2(seed);
                                let mut fixed = false;
                                for t in 1..=max_tries {
                                    let dz = jy
                                        * (synapse_offset * 0.25)
                                        * (t as f32 / max_tries as f32);
                                    let dx = jx
                                        * (synapse_offset * 0.12)
                                        * (t as f32 / max_tries as f32);
                                    let dy = jy
                                        * (synapse_offset * 0.12)
                                        * (t as f32 / max_tries as f32);
                                    let mut new_post = syn_i.post_site;
                                    new_post.z = (new_post.z + dz).clamp(-1.0, 1.0);
                                    new_post.x = (new_post.x + dx).clamp(-1.0, 1.0);
                                    new_post.y = (new_post.y + dy).clamp(-1.0, 1.0);
                                    let nd2 = seg_seg_min_dist_sq(a0, new_post, b0, b1);
                                    if nd2 >= eps2 {
                                        syn_i.post_site = new_post;
                                        fixed = true;
                                        break;
                                    }
                                }
                                if !fixed && use_mid_bends {
                                    let seed2 = ((other_syns[j].pre_id as u64) << 20)
                                        ^ (other_syns[j].post_id as u64)
                                        ^ (j as u64)
                                        ^ 0xAA55AA55;
                                    let (jx2, jy2) = jitter2(seed2);
                                    let mid = Point3 {
                                        x: (a0.x + a1.x) * 0.5 + jx2 * synapse_offset * 0.25,
                                        y: (a0.y + a1.y) * 0.5 + jy2 * synapse_offset * 0.25,
                                        z: (a0.z + a1.z) * 0.5 + jy * synapse_offset * 0.2,
                                    };
                                    // accept bend if it improves separation against segment j
                                    let nd1 = seg_seg_min_dist_sq(a0, mid, b0, b1)
                                        .min(seg_seg_min_dist_sq(mid, a1, b0, b1));
                                    if nd1 > dist2 {
                                        syn_i.bend = Some(mid);
                                    }
                                }
                            }
                        }
                    });
            }

            #[cfg(not(feature = "parallel"))]
            for i in 0..n_syn {
                for j in 0..i {
                    let a0 = m.synapses[i].pre_site;
                    let a1 = m.synapses[i].post_site;
                    let b0 = m.synapses[j].pre_site;
                    let b1 = m.synapses[j].post_site;
                    if !aabb_overlap(&a0, &a1, &b0, &b1, eps) {
                        continue;
                    }
                    let dist2 = seg_seg_min_dist_sq(a0, a1, b0, b1);
                    if dist2 < eps2 || near_colinear_overlap(a0, a1, b0, b1) {
                        let seed = ((m.synapses[i].pre_id as u64) << 20)
                            ^ (m.synapses[i].post_id as u64)
                            ^ 0x55AACC55;
                        let (jx, jy) = jitter2(seed);
                        let mut fixed = false;
                        for t in 1..=max_tries {
                            let dz = jy * (synapse_offset * 0.25) * (t as f32 / max_tries as f32);
                            let dx = jx * (synapse_offset * 0.12) * (t as f32 / max_tries as f32);
                            let dy = jy * (synapse_offset * 0.12) * (t as f32 / max_tries as f32);
                            let mut new_post = m.synapses[i].post_site;
                            new_post.z = (new_post.z + dz).clamp(-1.0, 1.0);
                            new_post.x = (new_post.x + dx).clamp(-1.0, 1.0);
                            new_post.y = (new_post.y + dy).clamp(-1.0, 1.0);
                            let nd2 = seg_seg_min_dist_sq(a0, new_post, b0, b1);
                            if nd2 >= eps2 {
                                m.synapses[i].post_site = new_post;
                                fixed = true;
                                break;
                            }
                        }
                        if !fixed && use_mid_bends {
                            let seed2 = ((m.synapses[j].pre_id as u64) << 20)
                                ^ (m.synapses[j].post_id as u64)
                                ^ 0xAA55AA55;
                            let (jx2, jy2) = jitter2(seed2);
                            let mid = Point3 {
                                x: (a0.x + a1.x) * 0.5 + jx2 * synapse_offset * 0.25,
                                y: (a0.y + a1.y) * 0.5 + jy2 * synapse_offset * 0.25,
                                z: (a0.z + a1.z) * 0.5 + jy * synapse_offset * 0.2,
                            };
                            let nd1 = seg_seg_min_dist_sq(a0, mid, b0, b1)
                                .min(seg_seg_min_dist_sq(mid, a1, b0, b1));
                            if nd1 > dist2 {
                                m.synapses[i].bend = Some(mid);
                            }
                        }
                    }
                }
            }

            // Micro-relaxation: push very close synapse sites apart slightly
            if relax_iters > 0 {
                let step = relax_step.max(0.0);
                let eps_pt = (eps * 0.5).min(0.01).max(0.0001);
                for _ in 0..relax_iters {
                    #[cfg(feature = "parallel")]
                    if n_syn > 128 {
                        let syn_ptr = m.synapses.as_ptr() as usize;
                        let displacements: Vec<(Point3, Point3)> = m
                            .synapses
                            .par_iter()
                            .enumerate()
                            .map(|(a, syn_a)| {
                                let mut disp_pre = Point3::default();
                                let mut disp_post = Point3::default();
                                let other_syns = unsafe {
                                    std::slice::from_raw_parts(syn_ptr as *const Synapse, n_syn)
                                };

                                for b in 0..n_syn {
                                    if a == b {
                                        continue;
                                    }
                                    let syn_b = &other_syns[b];

                                    // Pre sites
                                    let dx = syn_a.pre_site.x - syn_b.pre_site.x;
                                    let dy = syn_a.pre_site.y - syn_b.pre_site.y;
                                    let dz = syn_a.pre_site.z - syn_b.pre_site.z;
                                    let d2 = dx * dx + dy * dy + dz * dz;
                                    if d2 < eps_pt * eps_pt {
                                        let d = d2.sqrt().max(1e-6);
                                        let disp = step * 0.5;
                                        disp_pre.x += (dx / d) * disp;
                                        disp_pre.y += (dy / d) * disp;
                                        disp_pre.z += (dz / d) * disp;
                                    }

                                    // Post sites
                                    let dx = syn_a.post_site.x - syn_b.post_site.x;
                                    let dy = syn_a.post_site.y - syn_b.post_site.y;
                                    let dz = syn_a.post_site.z - syn_b.post_site.z;
                                    let d2 = dx * dx + dy * dy + dz * dz;
                                    if d2 < eps_pt * eps_pt {
                                        let d = d2.sqrt().max(1e-6);
                                        let disp = step * 0.5;
                                        disp_post.x += (dx / d) * disp;
                                        disp_post.y += (dy / d) * disp;
                                        disp_post.z += (dz / d) * disp;
                                    }
                                }
                                (disp_pre, disp_post)
                            })
                            .collect();

                        m.synapses
                            .iter_mut()
                            .zip(displacements.into_iter())
                            .for_each(|(syn, disp)| {
                                syn.pre_site.x = (syn.pre_site.x + disp.0.x).clamp(-1.0, 1.0);
                                syn.pre_site.y = (syn.pre_site.y + disp.0.y).clamp(-1.0, 1.0);
                                syn.pre_site.z = (syn.pre_site.z + disp.0.z).clamp(-1.0, 1.0);
                                syn.post_site.x = (syn.post_site.x + disp.1.x).clamp(-1.0, 1.0);
                                syn.post_site.y = (syn.post_site.y + disp.1.y).clamp(-1.0, 1.0);
                                syn.post_site.z = (syn.post_site.z + disp.1.z).clamp(-1.0, 1.0);
                            });
                        continue;
                    }

                    for a in 0..m.synapses.len() {
                        for b in (a + 1)..m.synapses.len() {
                            let pairs = [
                                (m.synapses[a].pre_site, m.synapses[b].pre_site, true),
                                (m.synapses[a].post_site, m.synapses[b].post_site, false),
                            ];
                            for (pa, pb, is_pre) in pairs {
                                let dx = pa.x - pb.x;
                                let dy = pa.y - pb.y;
                                let dz = pa.z - pb.z;
                                let d2 = dx * dx + dy * dy + dz * dz;
                                if d2 > eps_pt * eps_pt {
                                    continue;
                                }
                                let d = d2.sqrt().max(1e-6);
                                let ux = dx / d;
                                let uy = dy / d;
                                let uz = dz / d;
                                let disp = step * 0.5;
                                if is_pre {
                                    let mut va = m.synapses[a].pre_site;
                                    va.x = (va.x + ux * disp).clamp(-1.0, 1.0);
                                    va.y = (va.y + uy * disp).clamp(-1.0, 1.0);
                                    va.z = (va.z + uz * disp).clamp(-1.0, 1.0);
                                    let mut vb = m.synapses[b].pre_site;
                                    vb.x = (vb.x - ux * disp).clamp(-1.0, 1.0);
                                    vb.y = (vb.y - uy * disp).clamp(-1.0, 1.0);
                                    vb.z = (vb.z - uz * disp).clamp(-1.0, 1.0);
                                    m.synapses[a].pre_site = va;
                                    m.synapses[b].pre_site = vb;
                                } else {
                                    let mut va = m.synapses[a].post_site;
                                    va.x = (va.x + ux * disp).clamp(-1.0, 1.0);
                                    va.y = (va.y + uy * disp).clamp(-1.0, 1.0);
                                    va.z = (va.z + uz * disp).clamp(-1.0, 1.0);
                                    let mut vb = m.synapses[b].post_site;
                                    vb.x = (vb.x - ux * disp).clamp(-1.0, 1.0);
                                    vb.y = (vb.y - uy * disp).clamp(-1.0, 1.0);
                                    vb.z = (vb.z - uz * disp).clamp(-1.0, 1.0);
                                    m.synapses[a].post_site = va;
                                    m.synapses[b].post_site = vb;
                                }
                            }
                        }
                    }
                }
            }
        }

        m
    }

    /// Lightweight consistency check (debug-only recommended in hot paths)
    pub fn assert_consistent(&self, topo_layers: &Vec<Vec<crate::topology::Node3D>>) {
        let l_topo = topo_layers.len();
        // Ensure soma layering matches topology
        assert_eq!(self.somas.len(), l_topo, "somas per layer mismatch");
        assert_eq!(self.axons.len(), l_topo, "axons per layer mismatch");
        assert_eq!(self.dendrites.len(), l_topo, "dendrites per layer mismatch");
        // Spot-check some synapse endpoints
        for s in self.synapses.iter().take(8) {
            match s.kind {
                SynKind::In => {
                    assert!(s.post_layer >= 0 && (s.post_layer as usize) < l_topo);
                }
                SynKind::HiddenFwd => {
                    assert!(s.pre_layer >= 0 && (s.pre_layer as usize) < l_topo);
                    assert!(s.post_layer >= 0 && (s.post_layer as usize) < l_topo);
                }
                SynKind::HiddenBwd | SynKind::HiddenRec => {
                    assert!(s.pre_layer >= 0 && (s.pre_layer as usize) < l_topo);
                    assert!(s.post_layer >= 0 && (s.post_layer as usize) < l_topo);
                }
                SynKind::Out => {
                    assert!(s.pre_layer >= 0 && (s.pre_layer as usize) < l_topo);
                }
            }
        }
    }

    /// Populates the spatial index for fast proximity lookups including synapses and boutons.
    pub fn populate_grid(&mut self, cell_size: f32) {
        let cs = cell_size.max(0.01);
        let dim = (2.0 / cs).ceil() as usize;
        let num_cells = dim * dim * dim;

        let mut entities: Vec<GridEntity> = Vec::with_capacity(self.synapses.len() * 2);

        // Helper to collect all points first (skip out-of-bounds)
        let mut add_entity = |p: Point3, s: f32| {
            if p.x < -1.0 || p.x > 1.0 || p.y < -1.0 || p.y > 1.0 || p.z < -1.0 || p.z > 1.0 {
                return;
            }
            entities.push(GridEntity { pos: p, stimuli: s });
        };

        // 1. Synapses
        for syn in &self.synapses {
            add_entity(syn.post_site, syn.stimuli as f32);
        }

        // 2. Axon Boutons
        for layer in &self.axons {
            for axon in layer {
                for seg in &axon.segments {
                    add_entity(seg.to, seg.stimuli);
                }
            }
        }
        for axon in &self.sensory_axons {
            for seg in &axon.segments {
                add_entity(seg.to, seg.stimuli);
            }
        }
        for axon in &self.output_axons {
            for seg in &axon.segments {
                add_entity(seg.to, seg.stimuli);
            }
        }

        // 3. Dendrite Boutons
        for layer in &self.dendrites {
            for dend in layer {
                for seg in &dend.tree.branches {
                    add_entity(seg.from, seg.stimuli);
                }
            }
        }
        for dend in &self.sensory_dendrites {
            for seg in &dend.tree.branches {
                add_entity(seg.from, seg.stimuli);
            }
        }
        for dend in &self.output_dendrites {
            for seg in &dend.tree.branches {
                add_entity(seg.from, seg.stimuli);
            }
        }

        // 4. Somas
        for layer in &self.somas {
            for soma in layer {
                add_entity(soma.pos, soma.stimuli);
            }
        }
        for soma in &self.sensory_somas {
            add_entity(soma.pos, soma.stimuli);
        }
        for soma in &self.output_somas {
            add_entity(soma.pos, soma.stimuli);
        }

        // Decide between uniform grid and octree based on density.
        let use_octree = num_cells > 2_000_000
            || (entities.len() > 512 && num_cells > entities.len().saturating_mul(64));

        if use_octree {
            self.spatial_index = Some(SpatialIndex::Octree(OctreeIndex::build(entities, cs)));
            return;
        }

        let mut raw_entities: Vec<(usize, GridEntity)> = Vec::with_capacity(entities.len());
        for entity in entities.into_iter() {
            let gx = ((entity.pos.x + 1.0) / cs).floor() as isize;
            let gy = ((entity.pos.y + 1.0) / cs).floor() as isize;
            let gz = ((entity.pos.z + 1.0) / cs).floor() as isize;
            if gx >= 0
                && gx < dim as isize
                && gy >= 0
                && gy < dim as isize
                && gz >= 0
                && gz < dim as isize
            {
                let key = (gx as usize * dim + gy as usize) * dim + gz as usize;
                raw_entities.push((key, entity));
            }
        }

        // Sort entities by key
        raw_entities.sort_by_key(|e| e.0);

        let mut sorted_entities = Vec::with_capacity(raw_entities.len());
        let mut cell_starts = vec![0u32; num_cells + 1];

        let mut current_key = 0usize;
        for (key, entity) in raw_entities {
            while current_key < key {
                current_key += 1;
                cell_starts[current_key] = sorted_entities.len() as u32;
            }
            sorted_entities.push(entity);
        }
        while current_key < num_cells {
            current_key += 1;
            cell_starts[current_key] = sorted_entities.len() as u32;
        }

        self.spatial_index = Some(SpatialIndex::Grid(SpatialGrid {
            entities: sorted_entities,
            cell_starts,
            dim,
            cell_size: cs,
        }));
    }

    /// Calculate synaptic energy density at point `p`.
    pub fn energy_at(&self, p: Point3, radius: f32, k: f32) -> f32 {
        let r2 = radius * radius;
        let mut total = 0.0;

        if let Some(ref index) = self.spatial_index {
            match index {
                SpatialIndex::Grid(grid) => {
                    let cs = grid.cell_size;
                    let gx = ((p.x + 1.0) / cs).floor() as isize;
                    let gy = ((p.y + 1.0) / cs).floor() as isize;
                    let gz = ((p.z + 1.0) / cs).floor() as isize;

                    let r_cells = (radius / cs).ceil() as isize;
                    for dx in -r_cells..=r_cells {
                        for dy in -r_cells..=r_cells {
                            for dz in -r_cells..=r_cells {
                                if let Some(key) =
                                    grid.get_key_from_indices(gx + dx, gy + dy, gz + dz)
                                {
                                    for entity in grid.cell_entities(key) {
                                        let d2 = p.dist_sq(entity.pos);
                                        if d2 < r2 {
                                            // Quadratic reducing effect: S / (1 + k * r^2)
                                            total += entity.stimuli / (1.0 + k * d2);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                SpatialIndex::Octree(octree) => {
                    total += octree.energy_at(p, radius, k);
                }
            }
        } else {
            // Unoptimized fallback if grid not present
            for syn in &self.synapses {
                let d2 = p.dist_sq(syn.post_site);
                if d2 < r2 {
                    total += syn.stimuli as f32 / (1.0 + k * d2);
                }
            }
        }

        // Add ambient energy from skull membrane to encourage growth
        if let Some(ref skull) = self.skull_membrane {
            let dx = p.x - skull.center.x;
            let dy = p.y - skull.center.y;
            let dz = p.z - skull.center.z;
            let (rx, ry, rz) = skull.radii.unwrap_or_else(|| {
                (
                    skull.radius.max(1e-4),
                    skull.radius.max(1e-4),
                    skull.radius.max(1e-4),
                )
            });
            let q2 = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) + (dz * dz) / (rz * rz);
            if q2 < 1.0 {
                // Higher ambient energy towards the center, fluctuating
                total += skull.energy_fluctuation * (1.0 - q2.sqrt());
            }
        }
        total
    }

    fn seek_energy_biased(
        &self,
        p: Point3,
        radius: f32,
        k: f32,
        max_dist: f32,
        preferred_dir: Point3,
        dir_weight: f32,
        anchor: Option<Point3>,
        anchor_weight: f32,
    ) -> Point3 {
        let pref = preferred_dir.normalize();
        let use_pref = pref.mag() > 1.0e-6 && dir_weight > 0.0;
        let use_anchor = anchor.is_some() && anchor_weight > 0.0;

        let mut best_p = p;
        let mut best_score = self.energy_at(p, radius, k);
        if use_anchor {
            if let Some(a) = anchor {
                best_score -= anchor_weight * p.dist(a);
            }
        }

        // Sample around the current endpoint with mild directional preference.
        for _ in 0..14 {
            let dx = (fastrand::f32() - 0.5) * max_dist * 2.0;
            let dy = (fastrand::f32() - 0.5) * max_dist * 2.0;
            let dz = (fastrand::f32() - 0.5) * max_dist * 2.0;
            let cand = Point3 {
                x: (p.x + dx).clamp(-1.0, 1.0),
                y: (p.y + dy).clamp(-1.0, 1.0),
                z: (p.z + dz).clamp(-1.0, 1.0),
            };
            let mut score = self.energy_at(cand, radius, k);

            if use_pref {
                let step_dir = cand.sub(p).normalize();
                score += dir_weight * step_dir.dot(pref);
            }
            if use_anchor {
                if let Some(a) = anchor {
                    score -= anchor_weight * cand.dist(a);
                }
            }
            if let Some(skull) = &self.skull_membrane {
                let (rx, ry, rz) = skull.radii.unwrap_or_else(|| {
                    (
                        skull.radius.max(1.0e-4),
                        skull.radius.max(1.0e-4),
                        skull.radius.max(1.0e-4),
                    )
                });
                let dx = cand.x - skull.center.x;
                let dy = cand.y - skull.center.y;
                let dz = cand.z - skull.center.z;
                let q =
                    ((dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) + (dz * dz) / (rz * rz)).sqrt();
                if q > 1.0 {
                    // Keep growth inside membrane; outside points are heavily disfavored.
                    score -= (q - 1.0) * 5.0;
                }
            }

            if score > best_score {
                best_score = score;
                best_p = cand;
            }
        }
        best_p
    }

    pub fn apply_spatial_forces(
        &mut self,
        config: &crate::config::NetworkConfig,
        is_aarnn: bool,
        dt: f32,
    ) {
        observe_time!("morphology/spatial_forces");
        let min_sep = config.min_node_sep.max(0.0);
        let repulsion_strength = if min_sep > 0.0 {
            config.spatial_repulsion_strength * dt
        } else {
            0.0
        };
        let clumping_strength = config.spatial_clumping_strength * dt;
        let column_strength = if is_aarnn && config.columnar_enabled {
            config.columnar_strength * dt
        } else {
            0.0
        };
        let region_scale = crate::config::brain_region_space_scale(&config.brain_regions);
        let column_spacing = config.columnar_spacing.max(0.01);
        let column_jitter = config.columnar_jitter.clamp(0.0, 1.0);
        if repulsion_strength <= 0.0 && clumping_strength <= 0.0 && column_strength <= 0.0 {
            return;
        }

        // Calculate center of mass for hidden neurons to pull everything together
        let mut hidden_center = Point3::default();
        let mut hidden_count = 0;
        for layer in &self.somas {
            for s in layer {
                hidden_center = hidden_center.add(s.pos);
                hidden_count += 1;
            }
        }
        let avg_hidden_center = if hidden_count > 0 {
            hidden_center.mul(1.0 / hidden_count as f32)
        } else {
            Point3::default()
        };

        // 1. Collect all somas for a global repulsion pass
        let mut all_soma_refs = Vec::new();
        for l in 0..self.somas.len() {
            for i in 0..self.somas[l].len() {
                all_soma_refs.push((l as isize, i));
            }
        }
        for i in 0..self.sensory_somas.len() {
            all_soma_refs.push((-1, i));
        }
        for i in 0..self.output_somas.len() {
            all_soma_refs.push((-2, i));
        }

        if all_soma_refs.is_empty() {
            return;
        }

        // Build a spatial grid for somas to optimize N^2 repulsion to O(N)
        let cs = min_sep.max(0.01);
        let mut grid: FastHashMap<u64, Vec<usize>> = FastHashMap::default();
        for (idx, &(l, i)) in all_soma_refs.iter().enumerate() {
            let p = self.get_soma_pos(l, i);
            let gx = (p.x / cs).floor() as i64;
            let gy = (p.y / cs).floor() as i64;
            let gz = (p.z / cs).floor() as i64;
            let key = (((gx + 1048576) & 0x1FFFFF) as u64)
                | ((((gy + 1048576) & 0x1FFFFF) as u64) << 21)
                | ((((gz + 1048576) & 0x1FFFFF) as u64) << 42);
            grid.entry(key).or_default().push(idx);
        }

        // Parallelize displacement calculation
        let displacements: Vec<Point3> = (0..all_soma_refs.len())
            .into_par_iter()
            .map(|idx| {
                let (l1, _i1) = all_soma_refs[idx];
                let mut total_disp = Point3::default();
                let p1 = self.get_soma_pos(all_soma_refs[idx].0, all_soma_refs[idx].1);
                let center = self
                    .skull_membrane
                    .as_ref()
                    .map(|m| m.center)
                    .unwrap_or_default();

                // A. Clumping:
                // - Hidden: pull towards assigned brain region or hidden center of mass
                // - AARNN Anchors: pull towards the base of the skull
                if clumping_strength > 0.0 {
                    let mut clump_target = None;
                    let (rx, ry, _rz) = if let Some(m) = &self.skull_membrane {
                        m.radii.unwrap_or((m.radius, m.radius, m.radius))
                    } else {
                        (0.5, 0.5, 0.5)
                    };

                    if l1 >= 0 {
                        let soma1 = &self.somas[l1 as usize][_i1];
                        let mut found_region = false;
                        if let Some(rname) = &soma1.region_name {
                            if let Some(region) =
                                config.brain_regions.iter().find(|r| &r.name == rname)
                            {
                                clump_target = Some(Point3 {
                                    x: region.center[0] * region_scale,
                                    y: region.center[1] * region_scale,
                                    z: region.center[2] * region_scale,
                                });
                                found_region = true;
                            }
                        }
                        if !found_region {
                            clump_target = Some(avg_hidden_center);
                        }
                        // Also add a small pull to center to keep the clump from drifting off-center
                        // and ensure single neurons don't stay "anchored" if they started far away.
                        // Now that center is tied to COM, this force primarily stabilizes outliers.
                        let to_center = center.sub(p1);
                        if to_center.mag() > 1e-6 {
                            total_disp = total_disp.add(to_center.mul(clumping_strength * 0.1));
                        }
                    } else if is_aarnn {
                        // Pull to base ports near the "base" of the hidden clump
                        let base_x = if hidden_count > 0 {
                            avg_hidden_center.x
                        } else {
                            center.x
                        };
                        let base_z = if hidden_count > 0 {
                            avg_hidden_center.z
                        } else {
                            center.z
                        };

                        if l1 == -1 {
                            // Sensory base: slightly to the left of the clump base, at the bottom
                            clump_target = Some(Point3 {
                                x: base_x - rx * 0.15,
                                y: center.y - ry,
                                z: base_z,
                            });
                        } else if l1 == -2 {
                            // Output base: slightly to the right of the clump base, at the bottom
                            clump_target = Some(Point3 {
                                x: base_x + rx * 0.15,
                                y: center.y - ry,
                                z: base_z,
                            });
                        }
                    }

                    if let Some(target) = clump_target {
                        let diff = target.sub(p1);
                        if diff.mag() > 1e-6 {
                            total_disp = total_disp.add(diff.mul(clumping_strength));
                        }
                    }
                }

                // B. Columnar organization (AARNN): pull hidden somas laterally toward nearest column center
                if column_strength > 0.0 && l1 >= 0 {
                    let origin = if hidden_count > 0 {
                        avg_hidden_center
                    } else {
                        center
                    };
                    let dy = p1.y - origin.y;
                    let dz = p1.z - origin.z;
                    let col_y = (dy / column_spacing).round() as i32;
                    let col_z = (dz / column_spacing).round() as i32;
                    let mut cy = origin.y + (col_y as f32) * column_spacing;
                    let mut cz = origin.z + (col_z as f32) * column_spacing;
                    if column_jitter > 0.0 {
                        let mut h = (col_y as i64).wrapping_mul(73856093)
                            ^ (col_z as i64).wrapping_mul(19349663);
                        if h == 0 {
                            h = 1;
                        }
                        let mut x = h as u64;
                        x ^= x >> 33;
                        x = x.wrapping_mul(0xff51afd7ed558ccd);
                        x ^= x >> 33;
                        x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
                        x ^= x >> 33;
                        let j1 = ((x & 0xFFFF) as f32) / 32767.5 - 1.0;
                        let j2 = (((x >> 16) & 0xFFFF) as f32) / 32767.5 - 1.0;
                        let jitter = column_jitter * column_spacing * 0.35;
                        cy = (cy + j1 * jitter).clamp(-1.0, 1.0);
                        cz = (cz + j2 * jitter).clamp(-1.0, 1.0);
                    }
                    let diff = Point3 {
                        x: 0.0,
                        y: cy - p1.y,
                        z: cz - p1.z,
                    };
                    if diff.mag() > 1e-6 {
                        total_disp = total_disp.add(diff.mul(column_strength));
                    }
                }

                let gx = (p1.x / cs).floor() as i64;
                let gy = (p1.y / cs).floor() as i64;
                let gz = (p1.z / cs).floor() as i64;

                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let key = (((gx + dx + 1048576) & 0x1FFFFF) as u64)
                                | ((((gy + dy + 1048576) & 0x1FFFFF) as u64) << 21)
                                | ((((gz + dz + 1048576) & 0x1FFFFF) as u64) << 42);
                            if let Some(indices) = grid.get(&key) {
                                for &j_idx in indices {
                                    if idx == j_idx {
                                        continue;
                                    }
                                    let (l2, _i2) = all_soma_refs[j_idx];
                                    let p2 = self.get_soma_pos(l2, _i2);
                                    let diff = p1.sub(p2);
                                    let d2 = diff.dist_sq(Point3::default());
                                    if d2 < min_sep * min_sep && d2 > 1e-9 {
                                        let dist = d2.sqrt();
                                        // Repulsion between all somas to maintain separation
                                        let force = diff
                                            .normalize()
                                            .mul(repulsion_strength * (min_sep - dist) / min_sep);
                                        total_disp = total_disp.add(force);
                                    }
                                }
                            }
                        }
                    }
                }
                total_disp
            })
            .collect();

        // 2. Apply displacements and update associated components using PID control for smoothing
        let kp = config.skull_pid_kp;
        let ki = config.skull_pid_ki;
        let kd = config.skull_pid_kd;

        for (idx, (l, i)) in all_soma_refs.into_iter().enumerate() {
            // Anchors (sensory/output) are static unless in AARNN mode
            if l < 0 && !is_aarnn {
                continue;
            }

            let error = displacements[idx]; // Desired displacement for this step

            let soma = match l {
                -1 => &mut self.sensory_somas[i],
                -2 => &mut self.output_somas[i],
                _ => &mut self.somas[l as usize][i],
            };

            // PID Control
            soma.integral_err = soma.integral_err.add(error.mul(dt));
            let max_integral = (min_sep * 4.0).clamp(0.02, 0.2);
            let integ_mag = soma.integral_err.mag();
            if integ_mag > max_integral {
                soma.integral_err = soma.integral_err.mul(max_integral / integ_mag);
            }
            let derivative = error.sub(soma.prev_err).mul(1.0 / dt.max(0.001));

            let mut smoothed_disp = error
                .mul(kp)
                .add(soma.integral_err.mul(ki))
                .add(derivative.mul(kd));

            // Clamp max displacement per step to avoid runaway drift
            let max_move = (min_sep * 0.25).clamp(0.001, 0.01);
            if smoothed_disp.mag() > max_move {
                smoothed_disp = smoothed_disp.normalize().mul(max_move);
            }

            soma.prev_err = error;

            if smoothed_disp.mag() > 1e-9 {
                observe_hit!("morphology/soma_moved");
                let old_pos = soma.pos;
                let mut new_pos = old_pos.add(smoothed_disp);

                // Enforce membrane containment
                if let Some(m) = &self.skull_membrane {
                    let center = m.center;
                    let (rx, ry, rz) = m.radii.unwrap_or((m.radius, m.radius, m.radius));
                    let dx = new_pos.x - center.x;
                    let dy = new_pos.y - center.y;
                    let dz = new_pos.z - center.z;

                    // Normalized distance squared (ellipsoid)
                    let q2 = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) + (dz * dz) / (rz * rz);

                    if l >= 0 {
                        // Hidden: keep inside
                        if q2 > 1.0 {
                            let q = q2.sqrt();
                            new_pos = Point3 {
                                x: center.x + dx / q,
                                y: center.y + dy / q,
                                z: center.z + dz / q,
                            };
                        }
                    } else if is_aarnn {
                        // AARNN Anchors: snap to bottom surface
                        let q = q2.sqrt().max(1e-6);
                        // Force to surface
                        let mut surf_pos = Point3 {
                            x: center.x + dx / q,
                            y: center.y + dy / q,
                            z: center.z + dz / q,
                        };
                        // And especially ensure they stay in the bottom hemisphere
                        if surf_pos.y > center.y - ry * 0.2 {
                            surf_pos.y = center.y - ry * 0.2;
                            // re-snap to surface?
                            let ndx = surf_pos.x - center.x;
                            let ndy = surf_pos.y - center.y;
                            let ndz = surf_pos.z - center.z;
                            let nq = ((ndx * ndx) / (rx * rx)
                                + (ndy * ndy) / (ry * ry)
                                + (ndz * ndz) / (rz * rz))
                                .sqrt()
                                .max(1e-6);
                            surf_pos = Point3 {
                                x: center.x + ndx / nq,
                                y: center.y + ndy / nq,
                                z: center.z + ndz / nq,
                            };
                        }
                        new_pos = surf_pos;
                    }
                }

                if new_pos.dist_sq(old_pos) > 1e-12 {
                    let actual_disp = new_pos.sub(old_pos);
                    soma.pos = new_pos;
                    self.move_neuron_components(l, i, old_pos, new_pos, actual_disp);
                }
            }
        }
    }

    fn get_soma_pos(&self, layer: isize, id: usize) -> Point3 {
        match layer {
            -1 => self.sensory_somas[id].pos,
            -2 => self.output_somas[id].pos,
            l if l >= 0 => self.somas[l as usize][id].pos,
            _ => Point3::default(),
        }
    }

    #[allow(dead_code)]
    fn set_soma_pos(&mut self, layer: isize, id: usize, pos: Point3) {
        match layer {
            -1 => {
                self.sensory_somas[id].pos = pos;
            }
            -2 => {
                self.output_somas[id].pos = pos;
            }
            l if l >= 0 => {
                self.somas[l as usize][id].pos = pos;
            }
            _ => {}
        }
    }

    fn move_neuron_components(
        &mut self,
        layer: isize,
        id: usize,
        old_soma_pos: Point3,
        new_soma_pos: Point3,
        disp: Point3,
    ) {
        // Move organelles
        let organelles = match layer {
            -1 => &mut self.sensory_somas[id].organelles,
            -2 => &mut self.output_somas[id].organelles,
            l if l >= 0 => &mut self.somas[l as usize][id].organelles,
            _ => return,
        };
        for org in organelles {
            org.pos = org.pos.add(disp);
        }

        // Move roots of axons and dendrites
        let (axons, dendrites) = match layer {
            -1 => (
                std::slice::from_mut(&mut self.sensory_axons[id]),
                std::slice::from_mut(&mut self.sensory_dendrites[id]),
            ),
            -2 => (
                std::slice::from_mut(&mut self.output_axons[id]),
                std::slice::from_mut(&mut self.output_dendrites[id]),
            ),
            l if l >= 0 => (
                std::slice::from_mut(&mut self.axons[l as usize][id]),
                std::slice::from_mut(&mut self.dendrites[l as usize][id]),
            ),
            _ => return,
        };

        for ax in axons {
            for seg in &mut ax.segments {
                if seg.from.dist(old_soma_pos) < 1e-5 {
                    seg.from = new_soma_pos;
                }
                if seg.to.dist(old_soma_pos) < 1e-5 {
                    seg.to = new_soma_pos;
                }
            }
        }
        for den in dendrites {
            for seg in &mut den.tree.branches {
                if seg.from.dist(old_soma_pos) < 1e-5 {
                    seg.from = new_soma_pos;
                }
                if seg.to.dist(old_soma_pos) < 1e-5 {
                    seg.to = new_soma_pos;
                }
            }
        }
    }

    fn update_synapse_pos(&mut self, syn_idx: usize, new_pos: Point3, is_pre: bool) {
        if syn_idx >= self.synapses.len() {
            return;
        }
        if is_pre {
            self.synapses[syn_idx].pre_site = new_pos;
        } else {
            self.synapses[syn_idx].post_site = new_pos;
        }
    }

    fn energies_at_cpu(&self, points: &[Point3], radius: f32, k: f32) -> Vec<f32> {
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            points
                .par_iter()
                .map(|&p| self.energy_at(p, radius, k))
                .collect()
        }
        #[cfg(not(feature = "parallel"))]
        {
            points
                .iter()
                .map(|&p| self.energy_at(p, radius, k))
                .collect()
        }
    }

    #[cfg(feature = "opencl")]
    fn energies_at_gpu(
        &self,
        points: &[Point3],
        sources: Option<&[GridEntity]>,
        radius: f32,
        k: f32,
        cl: &OpenCLManager,
    ) -> Vec<f32> {
        use std::sync::atomic::{AtomicBool, Ordering};
        static GPU_ENERGY_DISABLED: AtomicBool = AtomicBool::new(false);

        let n_pts = points.len();
        if n_pts == 0 {
            return Vec::new();
        }

        if GPU_ENERGY_DISABLED.load(Ordering::Relaxed) {
            return self.energies_at_cpu(points, radius, k);
        }

        let mut all_entities_fallback;
        let entities = if let Some(s) = sources {
            s
        } else {
            all_entities_fallback = Vec::new();
            if let Some(ref index) = self.spatial_index {
                all_entities_fallback.extend_from_slice(index.entities());
            } else {
                for syn in &self.synapses {
                    all_entities_fallback.push(GridEntity {
                        pos: syn.post_site,
                        stimuli: syn.stimuli as f32,
                    });
                }
            }
            &all_entities_fallback
        };

        let n_sources = entities.len();
        if n_sources == 0 {
            return vec![0.0; n_pts];
        }

        let points_f4: Vec<[f32; 4]> = points.iter().map(|p| [p.x, p.y, p.z, 0.0]).collect();
        let src_sites_f4: Vec<[f32; 4]> = entities
            .iter()
            .map(|e| [e.pos.x, e.pos.y, e.pos.z, 0.0])
            .collect();
        let src_stimuli: Vec<f32> = entities.iter().map(|e| e.stimuli).collect();

        let r2 = radius * radius;

        let try_gpu = || -> Result<Vec<f32>, ClError> {
            let mut pt_buf = unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_ONLY,
                    n_pts * std::mem::size_of::<[f32; 4]>(),
                    std::ptr::null_mut(),
                )?
            };
            let mut src_site_buf = unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_ONLY,
                    n_sources * std::mem::size_of::<[f32; 4]>(),
                    std::ptr::null_mut(),
                )?
            };
            let mut src_stim_buf = unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_ONLY,
                    n_sources * std::mem::size_of::<f32>(),
                    std::ptr::null_mut(),
                )?
            };
            let mut energy_buf = unsafe {
                Buffer::create(
                    &cl.context,
                    CL_MEM_READ_WRITE,
                    n_pts * std::mem::size_of::<f32>(),
                    std::ptr::null_mut(),
                )?
            };

            unsafe {
                cl.queue
                    .enqueue_write_buffer(&mut pt_buf, CL_TRUE, 0, &points_f4, &[])?;
                cl.queue
                    .enqueue_write_buffer(&mut src_site_buf, CL_TRUE, 0, &src_sites_f4, &[])?;
                cl.queue
                    .enqueue_write_buffer(&mut src_stim_buf, CL_TRUE, 0, &src_stimuli, &[])?;
            }

            unsafe {
                let kernel = cl.kernel_morpho_energy.lock().unwrap();
                let _ = ExecuteKernel::new(&kernel)
                    .set_arg(&pt_buf)
                    .set_arg(&src_site_buf)
                    .set_arg(&src_stim_buf)
                    .set_arg(&energy_buf)
                    .set_arg(&(n_sources as i32))
                    .set_arg(&r2)
                    .set_arg(&k)
                    .set_global_work_size(n_pts)
                    .enqueue_nd_range(&cl.queue)?;
            }

            let mut energies = vec![0.0f32; n_pts];
            unsafe {
                cl.queue
                    .enqueue_read_buffer(&mut energy_buf, CL_TRUE, 0, &mut energies, &[])?;
            }
            Ok(energies)
        };

        match try_gpu() {
            Ok(energies) => energies,
            Err(e) => {
                nm_log!(
                    "[warn] OpenCL morpho_energy failed: {:?}; falling back to CPU",
                    e
                );
                GPU_ENERGY_DISABLED.store(true, Ordering::Relaxed);
                self.energies_at_cpu(points, radius, k)
            }
        }
    }

    pub fn update_skull_membrane(&mut self, config: &crate::config::NetworkConfig, dt: f32) {
        let mut min_p = Point3 {
            x: f32::MAX,
            y: f32::MAX,
            z: f32::MAX,
        };
        let mut max_p = Point3 {
            x: f32::MIN,
            y: f32::MIN,
            z: f32::MIN,
        };
        let mut sum_p = Point3::default();
        let mut count = 0;

        let r_soma = 0.05f32;
        let mut process_soma = |s: &Soma| {
            // Expand AABB by soma radius so membrane wraps outer soma surface, not centers
            min_p.x = min_p.x.min(s.pos.x - r_soma);
            min_p.y = min_p.y.min(s.pos.y - r_soma);
            min_p.z = min_p.z.min(s.pos.z - r_soma);
            max_p.x = max_p.x.max(s.pos.x + r_soma);
            max_p.y = max_p.y.max(s.pos.y + r_soma);
            max_p.z = max_p.z.max(s.pos.z + r_soma);
            sum_p = sum_p.add(s.pos);
            count += 1;
        };

        // Tightly wrap around hidden neurons only.
        for layer in &self.somas {
            for s in layer {
                process_soma(s);
            }
        }

        let (target_center, target_radii, target_radius_scalar) = if count > 0 {
            let center = sum_p.mul(1.0 / count as f32);
            // Radii should cover both extremes relative to the center
            let hx = (max_p.x - center.x).max(center.x - min_p.x).max(0.001);
            let hy = (max_p.y - center.y).max(center.y - min_p.y).max(0.001);
            let hz = (max_p.z - center.z).max(center.z - min_p.z).max(0.001);

            // Looseness factor: start loose for tiny networks, tighten as neuron count grows
            // s(count): n<=16 -> ~1.6x slack; n>=2000 -> ~1.05x slack (near snug)
            let n = count as f32;
            let s_max = 1.6f32;
            let s_min = 1.05f32;
            let n0 = 16.0f32;
            let n1 = 2000.0f32;
            let t = ((n - n0) / (n1 - n0)).clamp(0.0, 1.0);
            let slack = s_max + (s_min - s_max) * t; // decreases with size

            // Density-based isotropic expansion (heuristic)
            let target_density = config.density_target.max(0.001);
            let volume_per_soma = (4.0f32 / 3.0) * std::f32::consts::PI * r_soma.powi(3);
            let desired_volume = (count as f32) * volume_per_soma / target_density;
            let iso_r = ((desired_volume / ((4.0f32 / 3.0) * std::f32::consts::PI)).max(1e-6))
                .cbrt()
                * slack;

            // Per-axis radii: cover half-extent plus margin and at least isotropic density radius
            let base_margin = 0.15f32 * slack;
            // AABB already includes soma radius; only add base margin and isotropic density radius
            let rx = (hx + base_margin).max(iso_r);
            let ry = (hy + base_margin).max(iso_r);
            let rz = (hz + base_margin).max(iso_r);
            let scalar = rx.max(ry).max(rz);
            (center, (rx, ry, rz), scalar)
        } else {
            (Point3::default(), (0.25, 0.25, 0.25), 0.25)
        };

        // Apply PID control to smooth center and radii by operating on scalar and center
        let kp = config.skull_pid_kp;
        let ki = config.skull_pid_ki;
        let kd = config.skull_pid_kd;

        let current = self.skull_membrane.get_or_insert(SkullMembrane {
            center: target_center,
            radius: target_radius_scalar,
            radii: Some(target_radii),
            alpha_radius: None,
            energy_fluctuation: 0.05,
        });

        // 1. Center PID
        let err_c = target_center.sub(current.center);
        self.skull_center_integral = self.skull_center_integral.add(err_c.mul(dt));
        let der_c = err_c
            .sub(self.skull_center_prev_err)
            .mul(1.0 / dt.max(0.001));
        let output_c = err_c
            .mul(kp)
            .add(self.skull_center_integral.mul(ki))
            .add(der_c.mul(kd));
        current.center = current.center.add(output_c);
        self.skull_center_prev_err = err_c;

        // 2. Scalar radius PID (kept for backward compatibility/UI sizing); radii follow target directly
        let err_r = target_radius_scalar - current.radius;
        self.skull_radius_integral += err_r * dt;
        let der_r = (err_r - self.skull_radius_prev_err) / dt.max(0.001);
        let output_r = err_r * kp + self.skull_radius_integral * ki + der_r * kd;
        current.radius += output_r;
        current.radii = Some(target_radii);
        current.alpha_radius = Some((0.15f32 + 0.05).max(target_radius_scalar * 0.25)); // Heuristic alpha
        self.skull_radius_prev_err = err_r;

        // Fluctuations
        let ambient = config.aarnn_ambient_energy_level;
        let fluctuation = (fastrand::f32() * 0.02) - 0.01;
        current.energy_fluctuation = ambient + fluctuation;
    }

    /// Evolve morphology: grow towards energy, detect axon contact, and shrink inactive components.
    pub fn evolve(
        &mut self,
        config: &crate::config::NetworkConfig,
        is_aarnn: bool,
        dt: f32,
        #[cfg(feature = "opencl")] _cl: Option<&Arc<OpenCLManager>>,
    ) -> EvolutionResult {
        static mut CALL_COUNT: u64 = 0;
        unsafe {
            CALL_COUNT += 1;
        }
        let should_log = unsafe { CALL_COUNT % 100 == 0 };
        let is_trace = std::env::var("NM_TRACE").is_ok();
        let mut stats = MorphoStats::default();

        observe_time!("morphology/evolve");
        let mut res = EvolutionResult::default();
        if !config.morpho_growth_enabled {
            return res;
        }

        #[inline(always)]
        fn pack_neuron_pair(l1: isize, i1: usize, l2: isize, i2: usize) -> u64 {
            let p1 = (((l1 + 1) as u64 & 0xF) << 28) | (i1 as u64 & 0x0FFFFFFF);
            let p2 = (((l2 + 1) as u64 & 0xF) << 28) | (i2 as u64 & 0x0FFFFFFF);
            (p1 << 32) | p2
        }

        let (t_ema, t_dev, t_skip, t_cap) = if is_aarnn {
            let tuning = morpho_energy_tuning()
                .lock()
                .expect("morpho tuning lock poisoned");
            (tuning.ema, tuning.dev, tuning.skip_bias, tuning.cap_scale)
        } else {
            (0.0, 0.0, 1.0, 1.0)
        };
        // Ambient energy baseline used throughout this function
        let ambient: f32 = config.aarnn_ambient_energy_level.max(0.001);

        let num_layers = self.dendrites.len();

        let (in_l, out_l) = morphology_io_layers(config, is_aarnn, num_layers);

        let mut total_neurons = 0usize;
        let mut per_neuron_seg_cap = 0usize;
        let mut max_sample_segments = 0usize;
        let mut small_net_axon_by_neuron: Vec<Vec<(isize, usize, usize)>> = Vec::new();
        let mut contact_check_budget: Option<usize> = None;

        // 0. Spatial Awareness & Density Control (moved to end of evolve to avoid biasing contact/migration decisions)
        // self.apply_spatial_forces(config, is_aarnn, dt);
        // self.update_skull_membrane(config, dt);

        let decay_base = if config.component_decay_rate < 0.1 {
            if is_trace && should_log {
                nm_log!(
                    "[trace] WARNING: component_decay_rate ({:.6}) is very low, connections will prune almost immediately. Consider values > 0.9",
                    config.component_decay_rate
                );
            }
            config.component_decay_rate
        } else {
            config.component_decay_rate
        };
        let decay = decay_base.powf(dt);
        if is_trace && should_log {
            nm_log!(
                "[trace] morphology evolve: dt={:.2}ms, decay_base={:.6}, decay_factor={:.6}, aarnn={}",
                dt,
                decay_base,
                decay,
                is_aarnn
            );
        }
        let sprout_p = config.dendrite_sprout_prob * dt;
        let attraction_r = config.energy_attraction_radius;
        let kernel_k = config.energy_kernel_k;
        let contact_dist = config.axon_contact_dist;

        // 1. Component Shrinkage & Pruning
        // First rebuild the grid to get accurate energy readings for stimuli update
        self.populate_grid(attraction_r);

        // Update stimuli based on quadratic energy field and decay
        let break_threshold = config.component_pruning_threshold;
        let consolidation = config.synaptic_consolidation_factor;

        // Attempt batched energy evaluation on GPU for stimuli updates
        #[cfg_attr(not(feature = "opencl"), allow(unused_mut))]
        let mut stimuli_updated_via_gpu = false;
        #[cfg(feature = "opencl")]
        if let Some(cl) = _cl {
            // Build list of points requiring energy evaluation in the same order we will apply updates
            let mut pts: Vec<Point3> = Vec::new();
            pts.reserve(
                self.synapses.len()
                    + self
                        .sensory_axons
                        .iter()
                        .map(|a| a.segments.len())
                        .sum::<usize>()
                    + self
                        .output_axons
                        .iter()
                        .map(|a| a.segments.len())
                        .sum::<usize>()
                    + self
                        .axons
                        .iter()
                        .map(|layer| layer.iter().map(|a| a.segments.len()).sum::<usize>())
                        .sum::<usize>()
                    + self
                        .sensory_dendrites
                        .iter()
                        .map(|d| d.tree.branches.len())
                        .sum::<usize>()
                    + self
                        .output_dendrites
                        .iter()
                        .map(|d| d.tree.branches.len())
                        .sum::<usize>()
                    + self
                        .dendrites
                        .iter()
                        .map(|layer| layer.iter().map(|d| d.tree.branches.len()).sum::<usize>())
                        .sum::<usize>(),
            );

            // Offsets
            let off_syn = pts.len();
            for s in &self.synapses {
                pts.push(s.post_site);
            }
            let off_ax_s = pts.len();
            for ax in &self.sensory_axons {
                for seg in &ax.segments {
                    pts.push(seg.to);
                }
            }
            let off_ax_o = pts.len();
            for ax in &self.output_axons {
                for seg in &ax.segments {
                    pts.push(seg.to);
                }
            }
            let mut off_ax_h: Vec<usize> = Vec::with_capacity(self.axons.len());
            for l in 0..self.axons.len() {
                off_ax_h.push(pts.len());
                for ax in &self.axons[l] {
                    for seg in &ax.segments {
                        pts.push(seg.to);
                    }
                }
            }
            let off_den_s = pts.len();
            for den in &self.sensory_dendrites {
                for seg in &den.tree.branches {
                    pts.push(seg.from);
                }
            }
            let off_den_o = pts.len();
            for den in &self.output_dendrites {
                for seg in &den.tree.branches {
                    pts.push(seg.from);
                }
            }
            let mut off_den_h: Vec<usize> = Vec::with_capacity(self.dendrites.len());
            for l in 0..self.dendrites.len() {
                off_den_h.push(pts.len());
                for den in &self.dendrites[l] {
                    for seg in &den.tree.branches {
                        pts.push(seg.from);
                    }
                }
            }

            // Build sources list from current spatial index (already populated)
            let mut sources_buf: Option<Vec<GridEntity>> = None;
            if let Some(ref index) = self.spatial_index {
                sources_buf = Some(index.entities().to_vec());
            }
            let src_slice = sources_buf.as_ref().map(|v| v.as_slice());

            // Compute
            let energies = self.energies_at_gpu(&pts, src_slice, attraction_r, kernel_k, cl);

            // Apply updates using energies
            let mut idx = off_syn;
            for syn in &mut self.synapses {
                let e = energies[idx];
                idx += 1;
                let energy_factor = (0.9 + e * 0.2).min(1.05);
                let eff_decay = decay + (1.0 - decay) * syn.stimuli * consolidation;
                syn.stimuli = (syn.stimuli * eff_decay * energy_factor).min(1.0);
            }
            let mut i = off_ax_s;
            for ax in &mut self.sensory_axons {
                for seg in &mut ax.segments {
                    let e = energies[i];
                    i += 1;
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            let mut i2 = off_ax_o;
            for ax in &mut self.output_axons {
                for seg in &mut ax.segments {
                    let e = energies[i2];
                    i2 += 1;
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            for (l, off) in off_ax_h.iter().enumerate() {
                let mut k = *off;
                for ax in &mut self.axons[l] {
                    for seg in &mut ax.segments {
                        let e = energies[k];
                        k += 1;
                        let energy_factor = (0.9 + e * 0.2).min(1.05);
                        let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                        seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                    }
                }
            }
            let mut j = off_den_s;
            for den in &mut self.sensory_dendrites {
                for seg in &mut den.tree.branches {
                    let e = energies[j];
                    j += 1;
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            let mut j2 = off_den_o;
            for den in &mut self.output_dendrites {
                for seg in &mut den.tree.branches {
                    let e = energies[j2];
                    j2 += 1;
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            for (l, off) in off_den_h.iter().enumerate() {
                let mut k = *off;
                for den in &mut self.dendrites[l] {
                    for seg in &mut den.tree.branches {
                        let e = energies[k];
                        k += 1;
                        let energy_factor = (0.9 + e * 0.2).min(1.05);
                        let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                        seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                    }
                }
            }

            stimuli_updated_via_gpu = true;
        }

        if !stimuli_updated_via_gpu {
            // A. Synapses
            #[cfg(feature = "parallel")]
            let syn_energies: Vec<f32> = self
                .synapses
                .par_iter()
                .map(|s| self.energy_at(s.post_site, attraction_r, kernel_k))
                .collect();
            #[cfg(not(feature = "parallel"))]
            let syn_energies: Vec<f32> = self
                .synapses
                .iter()
                .map(|s| self.energy_at(s.post_site, attraction_r, kernel_k))
                .collect();
            for (syn, e) in self.synapses.iter_mut().zip(syn_energies) {
                let energy_factor = (0.9 + e * 0.2).min(1.05);
                // Consolidation: slow decay for established components
                let eff_decay = decay + (1.0 - decay) * syn.stimuli * consolidation;
                syn.stimuli = (syn.stimuli * eff_decay * energy_factor).min(1.0);
            }

            // B. Axons (sensory, output, hidden)
            #[cfg(feature = "parallel")]
            let axon_energies_s: Vec<Vec<f32>> = self
                .sensory_axons
                .par_iter()
                .map(|ax| {
                    ax.segments
                        .iter()
                        .map(|seg| self.energy_at(seg.to, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            #[cfg(not(feature = "parallel"))]
            let axon_energies_s: Vec<Vec<f32>> = self
                .sensory_axons
                .iter()
                .map(|ax| {
                    ax.segments
                        .iter()
                        .map(|seg| self.energy_at(seg.to, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            for (ax, energies) in self.sensory_axons.iter_mut().zip(axon_energies_s) {
                for (seg, e) in ax.segments.iter_mut().zip(energies) {
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            #[cfg(feature = "parallel")]
            let axon_energies_o: Vec<Vec<f32>> = self
                .output_axons
                .par_iter()
                .map(|ax| {
                    ax.segments
                        .iter()
                        .map(|seg| self.energy_at(seg.to, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            #[cfg(not(feature = "parallel"))]
            let axon_energies_o: Vec<Vec<f32>> = self
                .output_axons
                .iter()
                .map(|ax| {
                    ax.segments
                        .iter()
                        .map(|seg| self.energy_at(seg.to, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            for (ax, energies) in self.output_axons.iter_mut().zip(axon_energies_o) {
                for (seg, e) in ax.segments.iter_mut().zip(energies) {
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            for l in 0..self.axons.len() {
                #[cfg(feature = "parallel")]
                let axon_energies_h: Vec<Vec<f32>> = self.axons[l]
                    .par_iter()
                    .map(|ax| {
                        ax.segments
                            .iter()
                            .map(|seg| self.energy_at(seg.to, attraction_r, kernel_k))
                            .collect()
                    })
                    .collect();
                #[cfg(not(feature = "parallel"))]
                let axon_energies_h: Vec<Vec<f32>> = self.axons[l]
                    .iter()
                    .map(|ax| {
                        ax.segments
                            .iter()
                            .map(|seg| self.energy_at(seg.to, attraction_r, kernel_k))
                            .collect()
                    })
                    .collect();
                for (ax, energies) in self.axons[l].iter_mut().zip(axon_energies_h) {
                    for (seg, e) in ax.segments.iter_mut().zip(energies) {
                        let energy_factor = (0.9 + e * 0.2).min(1.05);
                        let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                        seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                    }
                }
            }

            // C. Dendrites (sensory, output, hidden)
            #[cfg(feature = "parallel")]
            let dend_energies_s: Vec<Vec<f32>> = self
                .sensory_dendrites
                .par_iter()
                .map(|den| {
                    den.tree
                        .branches
                        .iter()
                        .map(|seg| self.energy_at(seg.from, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            #[cfg(not(feature = "parallel"))]
            let dend_energies_s: Vec<Vec<f32>> = self
                .sensory_dendrites
                .iter()
                .map(|den| {
                    den.tree
                        .branches
                        .iter()
                        .map(|seg| self.energy_at(seg.from, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            for (den, energies) in self.sensory_dendrites.iter_mut().zip(dend_energies_s) {
                for (seg, e) in den.tree.branches.iter_mut().zip(energies) {
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            #[cfg(feature = "parallel")]
            let dend_energies_o: Vec<Vec<f32>> = self
                .output_dendrites
                .par_iter()
                .map(|den| {
                    den.tree
                        .branches
                        .iter()
                        .map(|seg| self.energy_at(seg.from, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            #[cfg(not(feature = "parallel"))]
            let dend_energies_o: Vec<Vec<f32>> = self
                .output_dendrites
                .iter()
                .map(|den| {
                    den.tree
                        .branches
                        .iter()
                        .map(|seg| self.energy_at(seg.from, attraction_r, kernel_k))
                        .collect()
                })
                .collect();
            for (den, energies) in self.output_dendrites.iter_mut().zip(dend_energies_o) {
                for (seg, e) in den.tree.branches.iter_mut().zip(energies) {
                    let energy_factor = (0.9 + e * 0.2).min(1.05);
                    let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                    seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                }
            }
            for l in 0..self.dendrites.len() {
                #[cfg(feature = "parallel")]
                let dend_energies_h: Vec<Vec<f32>> = self.dendrites[l]
                    .par_iter()
                    .map(|den| {
                        den.tree
                            .branches
                            .iter()
                            .map(|seg| self.energy_at(seg.from, attraction_r, kernel_k))
                            .collect()
                    })
                    .collect();
                #[cfg(not(feature = "parallel"))]
                let dend_energies_h: Vec<Vec<f32>> = self.dendrites[l]
                    .iter()
                    .map(|den| {
                        den.tree
                            .branches
                            .iter()
                            .map(|seg| self.energy_at(seg.from, attraction_r, kernel_k))
                            .collect()
                    })
                    .collect();
                for (den, energies) in self.dendrites[l].iter_mut().zip(dend_energies_h) {
                    for (seg, e) in den.tree.branches.iter_mut().zip(energies) {
                        let energy_factor = (0.9 + e * 0.2).min(1.05);
                        let eff_decay = decay + (1.0 - decay) * seg.stimuli * consolidation;
                        seg.stimuli = (seg.stimuli * energy_factor * eff_decay).min(1.0);
                    }
                }
            }
        } // end CPU stimuli update fallback

        // D. Somas (sensory, output, hidden)
        for layer in &mut self.somas {
            for soma in layer {
                soma.stimuli = (soma.stimuli * decay).clamp(0.1, 1.0);
            }
        }
        for soma in &mut self.sensory_somas {
            soma.stimuli = (soma.stimuli * decay).clamp(0.1, 1.0);
        }
        for soma in &mut self.output_somas {
            soma.stimuli = (soma.stimuli * decay).clamp(0.1, 1.0);
        }

        // Identify broken synapses
        let mut old_idx = 0;
        let mut new_idx = 0;
        let mut old_to_new = vec![None; self.synapses.len()];

        self.synapses.retain(|syn| {
            let keep = syn.stimuli >= break_threshold;
            if keep {
                old_to_new[old_idx] = Some(new_idx);
                new_idx += 1;
            } else {
                if is_trace {
                    let pre_name = if syn.pre_layer == -1 {
                        "sensory".to_string()
                    } else {
                        format!("hidden {}", syn.pre_layer)
                    };
                    let post_name = if syn.post_layer == -1 {
                        "sensory".to_string()
                    } else if (syn.post_layer as usize) == num_layers {
                        "output".to_string()
                    } else {
                        format!("hidden {}", syn.post_layer)
                    };
                    nm_log!(
                        "[trace] synapse pruned: {}:{} -> {}:{} - stimuli {:.4} < {:.4}",
                        pre_name,
                        syn.pre_id,
                        post_name,
                        syn.post_id,
                        syn.stimuli,
                        break_threshold
                    );
                }
                res.broken_connections.push((
                    syn.pre_layer,
                    syn.pre_id,
                    syn.post_layer,
                    syn.post_id,
                ));
                observe_hit!("morphology/synapse_pruned");
            }
            old_idx += 1;
            keep
        });

        // Prune Axon segments (bottom-up)
        // A segment is pruned if it loses its bouton, or if it is a leaf with low stimuli.
        // Shrinkage continues to parent if parent also has no bouton.
        let mut layers_axons_prune = vec![&mut self.sensory_axons, &mut self.output_axons];
        layers_axons_prune.extend(self.axons.iter_mut());
        for layer in layers_axons_prune {
            for axon in layer {
                let mut to_remove = std::collections::HashSet::new();
                let mut changed = true;
                while changed {
                    changed = false;
                    for i in 0..axon.segments.len() {
                        if to_remove.contains(&i) {
                            continue;
                        }
                        let seg = &axon.segments[i];

                        // Root of trunk is protected
                        if seg.is_trunk && seg.parent_idx.is_none() {
                            continue;
                        }

                        // Check bouton status
                        let has_bouton = if let Some(syn_idx) = seg.syn_index {
                            old_to_new[syn_idx].is_some()
                        } else {
                            false
                        };
                        let lost_bouton = seg.syn_index.is_some() && !has_bouton;

                        if !has_bouton {
                            // Check if it has any active children
                            let mut has_active_children = false;
                            for (ci, cseg) in axon.segments.iter().enumerate() {
                                if cseg.parent_idx == Some(i) && !to_remove.contains(&ci) {
                                    has_active_children = true;
                                    break;
                                }
                            }

                            let is_original_leaf =
                                !axon.segments.iter().any(|s| s.parent_idx == Some(i));

                            // Pruning triggers:
                            // - Just lost bouton
                            // - Is a leaf with low stimuli (failed to find contact)
                            // - Was a parent but all children are pruned (upward propagation)
                            if lost_bouton
                                || (!has_active_children
                                    && (seg.stimuli < break_threshold || !is_original_leaf))
                            {
                                to_remove.insert(i);
                                changed = true;
                            }
                        }
                    }
                }
                if !to_remove.is_empty() {
                    let mut new_segs = Vec::new();
                    let mut seg_old_to_new = std::collections::HashMap::new();
                    for (i, seg) in axon.segments.drain(..).enumerate() {
                        if !to_remove.contains(&i) {
                            seg_old_to_new.insert(i, new_segs.len());
                            new_segs.push(seg);
                        }
                    }
                    for seg in &mut new_segs {
                        if let Some(pidx) = seg.parent_idx {
                            seg.parent_idx = seg_old_to_new.get(&pidx).copied();
                        }
                    }
                    axon.segments = new_segs;
                }
            }
        }

        // Prune Dendrite segments (bottom-up)
        let mut layers_dends_prune = vec![&mut self.sensory_dendrites, &mut self.output_dendrites];
        layers_dends_prune.extend(self.dendrites.iter_mut());
        for layer in layers_dends_prune {
            for dend in layer {
                let mut to_remove = std::collections::HashSet::new();
                let mut changed = true;
                while changed {
                    changed = false;
                    for i in 0..dend.tree.branches.len() {
                        if to_remove.contains(&i) {
                            continue;
                        }
                        let seg = &dend.tree.branches[i];
                        if seg.is_trunk && seg.parent_idx.is_none() {
                            continue;
                        }

                        let has_bouton = if let Some(syn_idx) = seg.syn_index {
                            old_to_new[syn_idx].is_some()
                        } else {
                            false
                        };
                        let lost_bouton = seg.syn_index.is_some() && !has_bouton;

                        if !has_bouton {
                            let mut has_active_children = false;
                            for (ci, cseg) in dend.tree.branches.iter().enumerate() {
                                if cseg.parent_idx == Some(i) && !to_remove.contains(&ci) {
                                    has_active_children = true;
                                    break;
                                }
                            }

                            let is_original_leaf =
                                !dend.tree.branches.iter().any(|s| s.parent_idx == Some(i));

                            if lost_bouton
                                || (!has_active_children
                                    && (seg.stimuli < break_threshold || !is_original_leaf))
                            {
                                to_remove.insert(i);
                                changed = true;
                            }
                        }
                    }
                }
                if !to_remove.is_empty() {
                    let mut new_segs = Vec::new();
                    let mut seg_old_to_new = std::collections::HashMap::new();
                    for (i, seg) in dend.tree.branches.drain(..).enumerate() {
                        if !to_remove.contains(&i) {
                            seg_old_to_new.insert(i, new_segs.len());
                            new_segs.push(seg);
                        }
                    }
                    for seg in &mut new_segs {
                        if let Some(pidx) = seg.parent_idx {
                            seg.parent_idx = seg_old_to_new.get(&pidx).copied();
                        }
                    }
                    dend.tree.branches = new_segs;
                }
            }
        }

        // Update syn_index in all remaining segments and reciprocal indices in synapses to avoid stale indices
        for layer_idx in -1..=(self.axons.len() as isize) {
            let layer = if layer_idx == -1 {
                &mut self.sensory_axons
            } else if layer_idx == self.axons.len() as isize {
                &mut self.output_axons
            } else {
                &mut self.axons[layer_idx as usize]
            };
            for (_j, axon) in layer.iter_mut().enumerate() {
                for (asi, seg) in axon.segments.iter_mut().enumerate() {
                    if let Some(idx) = seg.syn_index {
                        if let Some(real_si) = old_to_new[idx] {
                            seg.syn_index = Some(real_si);
                            self.synapses[real_si].axon_seg_idx = Some(asi);
                        } else {
                            seg.syn_index = None;
                        }
                    }
                }
            }
        }
        for layer_idx in -1..=(self.dendrites.len() as isize) {
            let layer = if layer_idx == -1 {
                &mut self.sensory_dendrites
            } else if layer_idx == self.dendrites.len() as isize {
                &mut self.output_dendrites
            } else {
                &mut self.dendrites[layer_idx as usize]
            };
            for (_j, dendrite) in layer.iter_mut().enumerate() {
                for (dsi, seg) in dendrite.tree.branches.iter_mut().enumerate() {
                    if let Some(idx) = seg.syn_index {
                        if let Some(real_si) = old_to_new[idx] {
                            seg.syn_index = Some(real_si);
                            self.synapses[real_si].dend_seg_idx = Some(dsi);
                        } else {
                            seg.syn_index = None;
                        }
                    }
                }
            }
        }

        // Re-rebuild spatial grid for energy_at queries in growth loop
        self.populate_grid(attraction_r);

        // Pre-build existence set for fast contact detection
        let mut pair_counts: FastHashMap<u64, usize> = FastHashMap::default();
        for syn in &self.synapses {
            let key = pack_neuron_pair(syn.pre_layer, syn.pre_id, syn.post_layer, syn.post_id);
            *pair_counts.entry(key).or_insert(0) += 1;
        }
        let mut pair_cap: usize = 1;

        // Track sensory connection counts to enforce the 6-connection limit
        let mut sensory_conn_counts = vec![0usize; config.num_sensory_neurons];
        let mut sensory_syn_indices = vec![Vec::<usize>::new(); config.num_sensory_neurons];
        let mut output_conn_counts = vec![0usize; config.num_output_neurons];
        let mut output_syn_indices = vec![Vec::<usize>::new(); config.num_output_neurons];

        for (si, syn) in self.synapses.iter().enumerate() {
            if syn.pre_layer == -1 && syn.pre_id < sensory_conn_counts.len() {
                sensory_conn_counts[syn.pre_id] += 1;
                sensory_syn_indices[syn.pre_id].push(si);
            }
            if syn.post_layer == num_layers as isize && syn.post_id < output_conn_counts.len() {
                output_conn_counts[syn.post_id] += 1;
                output_syn_indices[syn.post_id].push(si);
            }
        }

        // AARNN: cap connections per neuron and bias 75% toward closest neighbors.
        let mut max_conn_per_neuron = usize::MAX;
        let mut close_neighbor_target = 0usize;
        let mut close_neighbors: Vec<Vec<usize>> = Vec::new();
        let mut conn_counts: Vec<usize> = Vec::new();
        let mut close_conn_counts: Vec<usize> = Vec::new();
        let mut neuron_index_sensory: Vec<usize> = Vec::new();
        let mut neuron_index_hidden: Vec<Vec<usize>> = Vec::new();
        let mut neuron_index_output: Vec<usize> = Vec::new();
        let mut neuron_positions: Vec<Point3> = Vec::new();
        let mut neuron_ref_by_index: Vec<(isize, usize)> = Vec::new();
        let mut connected_pre_by_post: Vec<FastHashMap<usize, usize>> = Vec::new();
        let is_close_neighbor =
            |list: &[usize], other: usize| -> bool { list.binary_search(&other).is_ok() };

        if is_aarnn {
            neuron_index_sensory = vec![usize::MAX; self.sensory_somas.len()];

            for (i, soma) in self.sensory_somas.iter().enumerate() {
                neuron_index_sensory[i] = neuron_positions.len();
                neuron_positions.push(soma.pos);
                neuron_ref_by_index.push((-1, i));
            }
            neuron_index_hidden = Vec::with_capacity(self.somas.len());
            for layer in &self.somas {
                let mut idxs = Vec::with_capacity(layer.len());
                for soma in layer {
                    idxs.push(neuron_positions.len());
                    neuron_positions.push(soma.pos);
                    neuron_ref_by_index.push((soma.layer as isize, soma.id));
                }
                neuron_index_hidden.push(idxs);
            }
            neuron_index_output = vec![usize::MAX; self.output_somas.len()];
            for (i, soma) in self.output_somas.iter().enumerate() {
                neuron_index_output[i] = neuron_positions.len();
                neuron_positions.push(soma.pos);
                neuron_ref_by_index.push((num_layers as isize, i));
            }
        }

        let index_of = |layer: isize, id: usize| -> Option<usize> {
            if layer == -1 {
                neuron_index_sensory.get(id).copied()
            } else if layer == num_layers as isize {
                neuron_index_output.get(id).copied()
            } else {
                neuron_index_hidden
                    .get(layer as usize)
                    .and_then(|layer_ids| layer_ids.get(id).copied())
            }
        };

        if is_aarnn {
            total_neurons = neuron_positions.len();
            if total_neurons > 0 {
                let base = (total_neurons * total_neurons).max(1);
                contact_check_budget = Some((base * 40).clamp(200, 8000));
                per_neuron_seg_cap =
                    (8.0 + (total_neurons as f32).ln().max(1.0) * 6.0).round() as usize;
                per_neuron_seg_cap = per_neuron_seg_cap.clamp(8, 64);
                max_sample_segments = (total_neurons.saturating_mul(8)).clamp(256, 8192);
                pair_cap = (2.0 + (total_neurons as f32).ln().max(1.0))
                    .round()
                    .clamp(1.0, 4.0) as usize;
                small_net_axon_by_neuron = vec![Vec::new(); total_neurons.max(1)];
                connected_pre_by_post = vec![FastHashMap::default(); total_neurons.max(1)];
                for syn in &self.synapses {
                    if let (Some(pre_i), Some(post_i)) = (
                        index_of(syn.pre_layer, syn.pre_id),
                        index_of(syn.post_layer, syn.post_id),
                    ) {
                        let entry = connected_pre_by_post[post_i].entry(pre_i).or_insert(0);
                        *entry += 1;
                    }
                }
            }
            if total_neurons > 1 {
                // Use proximity_degree_cap if set, otherwise fallback to a percentage of neurons
                max_conn_per_neuron = if config.proximity_degree_cap > 0 {
                    config.proximity_degree_cap
                } else {
                    ((total_neurons as f32) * 0.80 + 2.0).ceil() as usize
                };
                max_conn_per_neuron = max_conn_per_neuron.max(2);
                let max_cap = total_neurons.saturating_sub(1).saturating_mul(2);
                if max_cap > 0 {
                    max_conn_per_neuron = max_conn_per_neuron.min(max_cap);
                }
                close_neighbor_target = ((max_conn_per_neuron as f32) * 0.75).ceil() as usize;
                close_neighbor_target = close_neighbor_target.min(total_neurons.saturating_sub(1));
            } else {
                max_conn_per_neuron = 0;
                close_neighbor_target = 0;
            }

            close_neighbors = vec![Vec::new(); total_neurons];
            if close_neighbor_target > 0 {
                for i in 0..total_neurons {
                    let mut distances: Vec<(usize, f32)> = (0..total_neurons)
                        .filter(|&j| j != i)
                        .map(|j| (j, neuron_positions[i].dist(neuron_positions[j])))
                        .collect();
                    distances
                        .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                    let mut neighbors: Vec<usize> = distances
                        .into_iter()
                        .take(close_neighbor_target)
                        .map(|(j, _)| j)
                        .collect();
                    neighbors.sort_unstable();
                    close_neighbors[i] = neighbors;
                }
            }

            conn_counts = vec![0usize; total_neurons];
            close_conn_counts = vec![0usize; total_neurons];

            for syn in &self.synapses {
                let pre_idx = if syn.pre_layer == -1 {
                    neuron_index_sensory.get(syn.pre_id).copied()
                } else if syn.pre_layer == num_layers as isize {
                    neuron_index_output.get(syn.pre_id).copied()
                } else {
                    neuron_index_hidden
                        .get(syn.pre_layer as usize)
                        .and_then(|layer| layer.get(syn.pre_id).copied())
                };
                let post_idx = if syn.post_layer == -1 {
                    neuron_index_sensory.get(syn.post_id).copied()
                } else if syn.post_layer == num_layers as isize {
                    neuron_index_output.get(syn.post_id).copied()
                } else {
                    neuron_index_hidden
                        .get(syn.post_layer as usize)
                        .and_then(|layer| layer.get(syn.post_id).copied())
                };
                if let (Some(pre_i), Some(post_i)) = (pre_idx, post_idx) {
                    conn_counts[pre_i] += 1;
                    conn_counts[post_i] += 1;
                    if close_neighbor_target > 0 {
                        if is_close_neighbor(&close_neighbors[pre_i], post_i) {
                            close_conn_counts[pre_i] += 1;
                        }
                        if is_close_neighbor(&close_neighbors[post_i], pre_i) {
                            close_conn_counts[post_i] += 1;
                        }
                    }
                }
            }
        }
        let mean_conn = if total_neurons > 0 {
            conn_counts.iter().sum::<usize>() as f32 / total_neurons as f32
        } else {
            0.0
        };
        let target_conn = if max_conn_per_neuron > 0 {
            mean_conn.clamp(1.0, (max_conn_per_neuron as f32) * 0.85)
        } else {
            mean_conn.max(1.0)
        };
        if is_aarnn && max_conn_per_neuron > 0 {
            let max_conn_f = max_conn_per_neuron as f32;
            let sparsity = ((max_conn_f - mean_conn) / max_conn_f).clamp(0.0, 1.0);
            pair_cap = ((pair_cap as f32) * (1.0 + 0.8 * sparsity))
                .round()
                .clamp(1.0, 6.0) as usize;
        }

        // 2. Growth & Seeking: Tiered rates and whole-length dendritic growth
        let num_layers = self.dendrites.len();
        let _trunk_rate = config.trunk_growth_rate * dt;
        let _branch_rate = config.branch_growth_rate * dt;
        let bouton_rate = config.bouton_growth_rate * dt;

        let mut synapse_pos_updates = Vec::new();
        let soma_centroid = |somas: &[Soma]| -> Point3 {
            if somas.is_empty() {
                return Point3::default();
            }
            let mut acc = Point3::default();
            for s in somas {
                acc = acc.add(s.pos);
            }
            acc.mul(1.0 / somas.len() as f32)
        };
        let layer_centers: Vec<Point3> = self
            .somas
            .iter()
            .map(|layer| soma_centroid(layer))
            .collect();
        let sensory_center = soma_centroid(&self.sensory_somas);
        let output_center = soma_centroid(&self.output_somas);
        let has_sensory = !self.sensory_somas.is_empty();
        let has_output = !self.output_somas.is_empty();
        let preferred_growth_dir =
            |layer_idx: isize, soma_pos: Point3, is_dendrite: bool| -> Point3 {
                if !is_aarnn {
                    return Point3::default();
                }
                let mut target = Point3::default();
                let mut weight = 0.0f32;

                if is_dendrite {
                    if layer_idx == num_layers as isize {
                        if out_l < layer_centers.len() {
                            target = target.add(layer_centers[out_l].mul(1.3));
                            weight += 1.3;
                        }
                    } else if layer_idx == -1 {
                        if in_l < layer_centers.len() {
                            target = target.add(layer_centers[in_l].mul(1.1));
                            weight += 1.1;
                        }
                    } else if layer_idx >= 0 {
                        let l = layer_idx as usize;
                        if l > 0 && (l - 1) < layer_centers.len() {
                            target = target.add(layer_centers[l - 1]);
                            weight += 1.0;
                        }
                        if l == in_l && has_sensory {
                            target = target.add(sensory_center.mul(1.4));
                            weight += 1.4;
                        }
                        if (l + 1) < layer_centers.len() {
                            target = target.add(layer_centers[l + 1].mul(0.35));
                            weight += 0.35;
                        }
                    }
                } else {
                    if layer_idx == -1 {
                        if in_l < layer_centers.len() {
                            target = target.add(layer_centers[in_l].mul(1.4));
                            weight += 1.4;
                        }
                    } else if layer_idx >= 0 {
                        let l = layer_idx as usize;
                        if (l + 1) < layer_centers.len() {
                            target = target.add(layer_centers[l + 1]);
                            weight += 1.0;
                        }
                        if l == out_l && has_output {
                            target = target.add(output_center.mul(1.3));
                            weight += 1.3;
                        }
                        if weight <= 0.0 && has_output {
                            target = target.add(output_center);
                            weight += 1.0;
                        }
                    }
                }

                if weight <= 0.0 {
                    return Point3::default();
                }
                target.mul(1.0 / weight).sub(soma_pos).normalize()
            };

        for l in 0..num_layers {
            let soma_layer = &self.somas[l];

            #[cfg(feature = "parallel")]
            let neuron_results: Vec<(
                usize,
                Vec<DendSeg>,
                Vec<AxonSeg>,
                Vec<(usize, Point3, bool)>,
            )> = self.dendrites[l]
                .par_iter()
                .zip(self.axons[l].par_iter())
                .enumerate()
                .map(|(j, (dendrite, axon))| {
                    let soma = &soma_layer[j];
                    let mut local_trunk_rate = config.trunk_growth_rate;
                    let mut local_branch_rate = config.branch_growth_rate;
                    if let Some(tname) = &soma.type_name {
                        if let Some(ntype) = config.neuron_types.iter().find(|t| &t.name == tname) {
                            let factor = ntype.bio_params.synaptic_gain as f32;
                            local_trunk_rate *= factor;
                            local_branch_rate *= factor;
                        }
                    }
                    let trunk_rate = local_trunk_rate * dt;
                    let branch_rate = local_branch_rate * dt;

                    let soma_pos = soma.pos;
                    let dend_pref = preferred_growth_dir(l as isize, soma_pos, true);
                    let axon_pref = preferred_growth_dir(l as isize, soma_pos, false);
                    let mut d_branches = dendrite.tree.branches.clone();
                    let mut a_segments = axon.segments.clone();
                    let mut updates = Vec::new();

                    // --- Dendrite Tree Evolution ---
                    if !d_branches.is_empty() {
                        let mut hub_pos = d_branches[0].from;
                        let mut moved = false;
                        let mut branch_updates = Vec::new();

                        for seg_idx in 0..d_branches.len() {
                            if d_branches[seg_idx].parent_idx.is_some() {
                                let old_p = d_branches[seg_idx].from;
                                let stimuli = d_branches[seg_idx].stimuli;
                                let syn_idx = d_branches[seg_idx].syn_index;

                                let best_p = self.seek_energy_biased(
                                    old_p,
                                    attraction_r,
                                    kernel_k,
                                    bouton_rate,
                                    dend_pref,
                                    if is_aarnn { 0.55 } else { 0.0 },
                                    if is_aarnn { Some(soma_pos) } else { None },
                                    if is_aarnn { 0.08 } else { 0.0 },
                                );
                                let hub_diff = hub_pos.sub(best_p);
                                let delta_l =
                                    (stimuli - config.synaptic_growth_threshold) * branch_rate;
                                let new_p = best_p.add(hub_diff.normalize().mul(-delta_l));

                                branch_updates.push((seg_idx, new_p, syn_idx));
                                moved = true;
                            }
                        }

                        if moved {
                            for &(seg_idx, new_p, syn_idx) in &branch_updates {
                                d_branches[seg_idx].from = new_p;
                                if let Some(si) = syn_idx {
                                    updates.push((si, new_p, false));
                                }
                            }

                            let mut target_hub = soma_pos;
                            let mut count = 1.0;
                            for seg in &d_branches {
                                if seg.parent_idx.is_some() {
                                    target_hub = target_hub.add(seg.from);
                                    count += 1.0;
                                }
                            }
                            target_hub = target_hub.mul(1.0 / count);
                            hub_pos = hub_pos.lerp(target_hub, trunk_rate);

                            d_branches[0].from = hub_pos;
                            d_branches[0].to = soma_pos;
                            for seg in &mut d_branches {
                                if seg.parent_idx.is_some() {
                                    seg.to = hub_pos;
                                }
                                seg.length = seg.from.dist(seg.to);
                                if seg.length > config.max_segment_length {
                                    let dir = seg.from.sub(seg.to).normalize();
                                    seg.from = seg.to.add(dir.mul(config.max_segment_length));
                                    seg.length = config.max_segment_length;
                                    if let Some(si) = seg.syn_index {
                                        updates.push((si, seg.from, false));
                                    }
                                }
                            }
                        }
                    }

                    // --- Axon Tree Evolution ---
                    if !a_segments.is_empty() {
                        let mut hillock_pos = a_segments[0].to;
                        let mut moved = false;
                        let mut terminal_updates = Vec::new();

                        for seg_idx in 0..a_segments.len() {
                            if a_segments[seg_idx].parent_idx.is_some() {
                                let old_p = a_segments[seg_idx].to;
                                let stimuli = a_segments[seg_idx].stimuli;
                                let syn_idx = a_segments[seg_idx].syn_index;

                                let best_p = self.seek_energy_biased(
                                    old_p,
                                    attraction_r,
                                    kernel_k,
                                    bouton_rate,
                                    axon_pref,
                                    if is_aarnn { 0.65 } else { 0.0 },
                                    if is_aarnn { Some(soma_pos) } else { None },
                                    if is_aarnn { 0.03 } else { 0.0 },
                                );
                                let h_diff = best_p.sub(hillock_pos);
                                let delta_l =
                                    (stimuli - config.synaptic_growth_threshold) * branch_rate;
                                let new_p = best_p.add(h_diff.normalize().mul(delta_l));

                                terminal_updates.push((seg_idx, new_p, syn_idx));
                                moved = true;
                            }
                        }

                        if moved {
                            for &(seg_idx, new_p, syn_idx) in &terminal_updates {
                                a_segments[seg_idx].to = new_p;
                                if let Some(si) = syn_idx {
                                    updates.push((si, new_p, true));
                                }
                            }

                            let mut target_hillock = soma_pos;
                            let mut count = 1.0;
                            for seg in &a_segments {
                                if seg.parent_idx.is_some() {
                                    target_hillock = target_hillock.add(seg.to);
                                    count += 1.0;
                                }
                            }
                            target_hillock = target_hillock.mul(1.0 / count);
                            hillock_pos = hillock_pos.lerp(target_hillock, trunk_rate);

                            a_segments[0].to = hillock_pos;
                            for seg in &mut a_segments {
                                if seg.parent_idx.is_some() {
                                    seg.from = hillock_pos;
                                }
                                seg.length = seg.from.dist(seg.to);
                                if seg.length > config.max_segment_length {
                                    let dir = seg.to.sub(seg.from).normalize();
                                    seg.to = seg.from.add(dir.mul(config.max_segment_length));
                                    seg.length = config.max_segment_length;
                                    if let Some(si) = seg.syn_index {
                                        updates.push((si, seg.to, true));
                                    }
                                }
                            }
                        }
                    }
                    (j, d_branches, a_segments, updates)
                })
                .collect();

            #[cfg(feature = "parallel")]
            for (j, d_branches, a_segments, updates) in neuron_results {
                self.dendrites[l][j].tree.branches = d_branches;
                self.axons[l][j].segments = a_segments;
                synapse_pos_updates.extend(updates);
            }

            #[cfg(not(feature = "parallel"))]
            for j in 0..self.dendrites[l].len() {
                let soma = &self.somas[l][j];
                let mut local_trunk_rate = config.trunk_growth_rate;
                let mut local_branch_rate = config.branch_growth_rate;
                if let Some(tname) = &soma.type_name {
                    if let Some(ntype) = config.neuron_types.iter().find(|t| &t.name == tname) {
                        let factor = ntype.bio_params.synaptic_gain as f32;
                        local_trunk_rate *= factor;
                        local_branch_rate *= factor;
                    }
                }
                let trunk_rate = local_trunk_rate * dt;
                let branch_rate = local_branch_rate * dt;

                let soma_pos = soma.pos;
                let dend_pref = preferred_growth_dir(l as isize, soma_pos, true);
                let axon_pref = preferred_growth_dir(l as isize, soma_pos, false);
                // --- Dendrite Tree Evolution ---
                if !self.dendrites[l][j].tree.branches.is_empty() {
                    let mut hub_pos = self.dendrites[l][j].tree.branches[0].from;
                    let mut moved = false;
                    let mut branch_updates = Vec::new();
                    for seg_idx in 0..self.dendrites[l][j].tree.branches.len() {
                        if self.dendrites[l][j].tree.branches[seg_idx]
                            .parent_idx
                            .is_some()
                        {
                            let old_p = self.dendrites[l][j].tree.branches[seg_idx].from;
                            let stimuli = self.dendrites[l][j].tree.branches[seg_idx].stimuli;
                            let syn_idx = self.dendrites[l][j].tree.branches[seg_idx].syn_index;
                            let best_p = self.seek_energy_biased(
                                old_p,
                                attraction_r,
                                kernel_k,
                                bouton_rate,
                                dend_pref,
                                if is_aarnn { 0.55 } else { 0.0 },
                                if is_aarnn { Some(soma_pos) } else { None },
                                if is_aarnn { 0.08 } else { 0.0 },
                            );
                            let hub_diff = hub_pos.sub(best_p);
                            let delta_l =
                                (stimuli - config.synaptic_growth_threshold) * branch_rate;
                            let new_p = best_p.add(hub_diff.normalize().mul(-delta_l));
                            branch_updates.push((seg_idx, new_p, syn_idx));
                            moved = true;
                        }
                    }
                    if moved {
                        let dend = &mut self.dendrites[l][j];
                        for (seg_idx, new_p, syn_idx) in branch_updates {
                            dend.tree.branches[seg_idx].from = new_p;
                            if let Some(si) = syn_idx {
                                synapse_pos_updates.push((si, new_p, false));
                            }
                        }
                        let mut target_hub = soma_pos;
                        let mut count = 1.0;
                        for seg in &dend.tree.branches {
                            if seg.parent_idx.is_some() {
                                target_hub = target_hub.add(seg.from);
                                count += 1.0;
                            }
                        }
                        target_hub = target_hub.mul(1.0 / count);
                        hub_pos = hub_pos.lerp(target_hub, trunk_rate);
                        dend.tree.branches[0].from = hub_pos;
                        dend.tree.branches[0].to = soma_pos;
                        for seg in &mut dend.tree.branches {
                            if seg.parent_idx.is_some() {
                                seg.to = hub_pos;
                            }
                            seg.length = seg.from.dist(seg.to);
                            if seg.length > config.max_segment_length {
                                let dir = seg.from.sub(seg.to).normalize();
                                seg.from = seg.to.add(dir.mul(config.max_segment_length));
                                seg.length = config.max_segment_length;
                                if let Some(si) = seg.syn_index {
                                    synapse_pos_updates.push((si, seg.from, false));
                                }
                            }
                        }
                    }
                }
                // --- Axon Tree Evolution ---
                if !self.axons[l][j].segments.is_empty() {
                    let mut hillock_pos = self.axons[l][j].segments[0].to;
                    let mut moved = false;
                    let mut terminal_updates = Vec::new();
                    for seg_idx in 0..self.axons[l][j].segments.len() {
                        if self.axons[l][j].segments[seg_idx].parent_idx.is_some() {
                            let old_p = self.axons[l][j].segments[seg_idx].to;
                            let stimuli = self.axons[l][j].segments[seg_idx].stimuli;
                            let syn_idx = self.axons[l][j].segments[seg_idx].syn_index;
                            let best_p = self.seek_energy_biased(
                                old_p,
                                attraction_r,
                                kernel_k,
                                bouton_rate,
                                axon_pref,
                                if is_aarnn { 0.65 } else { 0.0 },
                                if is_aarnn { Some(soma_pos) } else { None },
                                if is_aarnn { 0.03 } else { 0.0 },
                            );
                            let h_diff = best_p.sub(hillock_pos);
                            let delta_l =
                                (stimuli - config.synaptic_growth_threshold) * branch_rate;
                            let new_p = best_p.add(h_diff.normalize().mul(delta_l));
                            terminal_updates.push((seg_idx, new_p, syn_idx));
                            moved = true;
                        }
                    }
                    if moved {
                        let ax = &mut self.axons[l][j];
                        for (seg_idx, new_p, syn_idx) in terminal_updates {
                            ax.segments[seg_idx].to = new_p;
                            if let Some(si) = syn_idx {
                                synapse_pos_updates.push((si, new_p, true));
                            }
                        }
                        let mut target_hillock = soma_pos;
                        let mut count = 1.0;
                        for seg in &ax.segments {
                            if seg.parent_idx.is_some() {
                                target_hillock = target_hillock.add(seg.to);
                                count += 1.0;
                            }
                        }
                        target_hillock = target_hillock.mul(1.0 / count);
                        hillock_pos = hillock_pos.lerp(target_hillock, trunk_rate);
                        ax.segments[0].to = hillock_pos;
                        for seg in &mut ax.segments {
                            if seg.parent_idx.is_some() {
                                seg.from = hillock_pos;
                            }
                            seg.length = seg.from.dist(seg.to);
                            if seg.length > config.max_segment_length {
                                let dir = seg.to.sub(seg.from).normalize();
                                seg.to = seg.from.add(dir.mul(config.max_segment_length));
                                seg.length = config.max_segment_length;
                                if let Some(si) = seg.syn_index {
                                    synapse_pos_updates.push((si, seg.to, true));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Apply position updates to the central synapse repository
        for (si, pos, is_pre) in synapse_pos_updates {
            self.update_synapse_pos(si, pos, is_pre);
        }

        // 3. Sprouting & Contact Detection
        let mut new_dendrite_branches = Vec::new();
        let mut new_axon_branches = Vec::new();
        let mut new_syns = Vec::new();

        // Axon Sprouting (Refined: tree root style, space-aware)
        for l_idx in -1..=(num_layers as isize) {
            let axons = if l_idx == -1 {
                &self.sensory_axons
            } else if l_idx == num_layers as isize {
                &self.output_axons
            } else {
                &self.axons[l_idx as usize]
            };

            for j in 0..axons.len() {
                let soma = if l_idx == -1 {
                    &self.sensory_somas[j]
                } else if l_idx == num_layers as isize {
                    &self.output_somas[j]
                } else {
                    &self.somas[l_idx as usize][j]
                };
                let soma_pos = soma.pos;
                let axon_pref = preferred_growth_dir(l_idx, soma_pos, false);
                let local_e = self.energy_at(soma_pos, attraction_r, kernel_k);
                stats.axon_sprout_attempts += 1;

                let mut local_sprout_p = sprout_p;
                if let Some(tname) = &soma.type_name {
                    if let Some(ntype) = config.neuron_types.iter().find(|t| &t.name == tname) {
                        local_sprout_p *= ntype.bio_params.synaptic_gain as f32;
                    }
                }
                let effective_sprout_p = local_sprout_p * (0.4 + local_e * 2.0);

                if fastrand::f32() < effective_sprout_p {
                    stats.axon_sprout_successes += 1;
                    let axon = &axons[j];

                    // Sprout from end of a segment OR midway along a segment OR from soma if empty
                    let (p, parent_idx) = if axon.segments.is_empty() {
                        (soma_pos, None)
                    } else {
                        let pidx = fastrand::usize(..axon.segments.len());
                        let parent_seg = &axon.segments[pidx];
                        let pos = if fastrand::f32() < 0.5 {
                            parent_seg.to // end of axon branch
                        } else {
                            parent_seg.from.lerp(parent_seg.to, fastrand::f32())
                            // midway
                        };
                        (pos, Some(pidx))
                    };

                    // Space occupancy check: ensure no two branches sprout from same/near origin
                    let mut too_near = false;
                    for s in &axon.segments {
                        if s.from.dist(p) < 0.015 {
                            too_near = true;
                            break;
                        }
                    }

                    if !too_near {
                        let best_p = self.seek_energy_biased(
                            p,
                            attraction_r,
                            kernel_k,
                            attraction_r * 0.35,
                            axon_pref,
                            if is_aarnn { 0.70 } else { 0.0 },
                            if is_aarnn { Some(soma_pos) } else { None },
                            if is_aarnn { 0.02 } else { 0.0 },
                        );
                        if best_p.dist(p) > 0.005 {
                            if is_trace {
                                let name = if l_idx == -1 {
                                    "sensory".to_string()
                                } else if l_idx == num_layers as isize {
                                    "output".to_string()
                                } else {
                                    format!("hidden {}", l_idx)
                                };
                                nm_log!(
                                    "[trace] axon sprouted: {}:{} at {:?} -> {:?} - energy {:.4}",
                                    name,
                                    j,
                                    p,
                                    best_p,
                                    local_e
                                );
                            }
                            new_axon_branches.push((
                                l_idx,
                                j,
                                AxonSeg {
                                    from: p,
                                    to: best_p,
                                    length: p.dist(best_p),
                                    stimuli: 0.5,
                                    parent_idx,
                                    syn_index: None,
                                    is_trunk: axon.segments.is_empty(),
                                },
                            ));
                        }
                    }
                }
            }
        }

        // Build a spatial index for axon segments to optimize contact detection from O(N^2) to O(N)
        let axon_cs = contact_dist.max(0.01);
        let mut seg_refs: Vec<SegRef> = Vec::new();
        let mut small_net_segments: Vec<(isize, usize, usize)> = Vec::new();
        let mut segments_seen = 0usize;
        if max_sample_segments > 0 {
            small_net_segments.reserve(max_sample_segments.min(4096));
        }
        // Hidden axons
        for al in 0..self.axons.len() {
            for aj in 0..self.axons[al].len() {
                for (asi, aseg) in self.axons[al][aj].segments.iter().enumerate() {
                    if max_sample_segments > 0 {
                        segments_seen += 1;
                        if small_net_segments.len() < max_sample_segments {
                            small_net_segments.push((al as isize, aj, asi));
                        } else {
                            let j = fastrand::usize(..segments_seen);
                            if j < max_sample_segments {
                                small_net_segments[j] = (al as isize, aj, asi);
                            }
                        }
                        if let Some(layer) = neuron_index_hidden.get(al) {
                            if let Some(&idx) = layer.get(aj) {
                                if idx < small_net_axon_by_neuron.len() {
                                    let list = &mut small_net_axon_by_neuron[idx];
                                    if list.len() < per_neuron_seg_cap {
                                        list.push((al as isize, aj, asi));
                                    }
                                }
                            }
                        }
                    }
                    let min = Point3 {
                        x: aseg.from.x.min(aseg.to.x),
                        y: aseg.from.y.min(aseg.to.y),
                        z: aseg.from.z.min(aseg.to.z),
                    };
                    let max = Point3 {
                        x: aseg.from.x.max(aseg.to.x),
                        y: aseg.from.y.max(aseg.to.y),
                        z: aseg.from.z.max(aseg.to.z),
                    };
                    seg_refs.push(SegRef {
                        l: al as isize,
                        j: aj,
                        si: asi,
                        min,
                        max,
                    });
                }
            }
        }
        // Sensory axons
        for (aj, ax) in self.sensory_axons.iter().enumerate() {
            for (asi, aseg) in ax.segments.iter().enumerate() {
                if max_sample_segments > 0 {
                    segments_seen += 1;
                    if small_net_segments.len() < max_sample_segments {
                        small_net_segments.push((-1, aj, asi));
                    } else {
                        let j = fastrand::usize(..segments_seen);
                        if j < max_sample_segments {
                            small_net_segments[j] = (-1, aj, asi);
                        }
                    }
                    if let Some(&idx) = neuron_index_sensory.get(aj) {
                        if idx < small_net_axon_by_neuron.len() {
                            let list = &mut small_net_axon_by_neuron[idx];
                            if list.len() < per_neuron_seg_cap {
                                list.push((-1, aj, asi));
                            }
                        }
                    }
                }
                let min = Point3 {
                    x: aseg.from.x.min(aseg.to.x),
                    y: aseg.from.y.min(aseg.to.y),
                    z: aseg.from.z.min(aseg.to.z),
                };
                let max = Point3 {
                    x: aseg.from.x.max(aseg.to.x),
                    y: aseg.from.y.max(aseg.to.y),
                    z: aseg.from.z.max(aseg.to.z),
                };
                seg_refs.push(SegRef {
                    l: -1,
                    j: aj,
                    si: asi,
                    min,
                    max,
                });
            }
        }

        let axon_index = AxonSegIndex::build(seg_refs, axon_cs, contact_dist);

        let mut budget_exhausted = false;
        let mut probe_rr = 0usize;
        fn is_pair_full(pair_counts: &FastHashMap<u64, usize>, key: u64, pair_cap: usize) -> bool {
            pair_counts.get(&key).copied().unwrap_or(0) >= pair_cap
        }
        fn allow_conn_soft(count: usize, max_conn: usize) -> bool {
            if max_conn == 0 || count < max_conn {
                return true;
            }
            let over = count.saturating_sub(max_conn.saturating_sub(1)) as f32;
            let base = (max_conn as f32).max(1.0);
            let allow_p = (-over / base).exp();
            fastrand::f32() < allow_p
        }
        let mut pre_full_marks: Vec<u32> = Vec::new();
        let mut tip_mark: u32 = 1;
        if total_neurons > 0 {
            pre_full_marks = vec![0u32; total_neurons];
        }
        let mut trace_synapse_count = 0usize;
        let mut axon_candidates: Vec<SegRef> = Vec::new();
        'contact_all: for l_idx in -1..=(num_layers as isize) {
            let (skip_threshold, cap_base) = if is_aarnn {
                let dyn_skip_base = (t_ema - t_dev * 0.5).max(ambient * 1.1);
                let dyn_skip = (dyn_skip_base * t_skip).min(t_ema + t_dev * 1.5);
                let base =
                    (24.0 * (t_ema / (ambient + 0.05)).clamp(0.6, 1.6)).clamp(16.0, 48.0) * t_cap;
                (dyn_skip, base)
            } else {
                (ambient * 1.5, 32.0)
            };
            let is_output = l_idx == num_layers as isize;
            let is_sensory = l_idx == -1;
            let dendrites_layer = if is_sensory {
                &self.sensory_dendrites
            } else if is_output {
                &self.output_dendrites
            } else {
                &self.dendrites[l_idx as usize]
            };
            let somas_layer = if is_sensory {
                &self.sensory_somas
            } else if is_output {
                &self.output_somas
            } else {
                &self.somas[l_idx as usize]
            };

            for j in 0..dendrites_layer.len() {
                let soma_pos = somas_layer[j].pos;
                let local_e = self.energy_at(soma_pos, attraction_r, kernel_k);
                let post_idx = if l_idx == -1 {
                    neuron_index_sensory.get(j).copied()
                } else if l_idx == num_layers as isize {
                    neuron_index_output.get(j).copied()
                } else {
                    neuron_index_hidden
                        .get(l_idx as usize)
                        .and_then(|layer| layer.get(j).copied())
                };
                let mut homeo_boost = 1.0;
                if is_aarnn && target_conn > 0.0 {
                    let conn = post_idx
                        .and_then(|idx| conn_counts.get(idx).copied())
                        .unwrap_or(0);
                    let deficit = ((target_conn - conn as f32) / target_conn).clamp(0.0, 1.0);
                    homeo_boost = 1.0 + deficit * 0.4;
                }
                let mut local_sprout_p = sprout_p;
                let soma = &somas_layer[j];
                if let Some(tname) = &soma.type_name {
                    if let Some(ntype) = config.neuron_types.iter().find(|t| &t.name == tname) {
                        local_sprout_p *= ntype.bio_params.synaptic_gain as f32;
                    }
                }
                let effective_sprout_p = local_sprout_p * (1.0 + local_e * 5.0) * homeo_boost;

                // Sprouting: Triggered by nearby synaptic energy peaks (Refined: tree root style, space-aware)
                stats.dendrite_sprout_attempts += 1;
                if fastrand::f32() < effective_sprout_p {
                    let dend = &dendrites_layer[j];
                    let dend_pref = preferred_growth_dir(l_idx, soma_pos, true);
                    // Decide: new trunk from soma OR branch from existing segment
                    let (base_pos, parent_idx, is_trunk) =
                        if fastrand::f32() < 0.2 || dend.tree.branches.is_empty() {
                            (soma_pos, None, true)
                        } else {
                            let idx = fastrand::usize(..dend.tree.branches.len());
                            let pseg = &dend.tree.branches[idx];
                            let p = if fastrand::f32() < 0.5 {
                                pseg.from // end of dendrite branch
                            } else {
                                pseg.from.lerp(pseg.to, fastrand::f32()) // midway
                            };
                            (p, Some(idx), false)
                        };

                    // Space occupancy check: ensure no two branches sprout from same/near origin
                    let mut too_near = false;
                    for s in &dend.tree.branches {
                        // Allow multiple trunks to originate from soma, but check other origins
                        if base_pos.dist(soma_pos) > 1e-4 && s.to.dist(base_pos) < 0.015 {
                            too_near = true;
                            break;
                        }
                    }

                    if too_near {
                        stats.dendrite_sprout_too_near += 1;
                    } else {
                        let mut best_p = self.seek_energy_biased(
                            base_pos,
                            attraction_r,
                            kernel_k,
                            attraction_r * 0.85,
                            dend_pref,
                            if is_aarnn { 0.60 } else { 0.0 },
                            if is_aarnn { Some(soma_pos) } else { None },
                            if is_aarnn { 0.10 } else { 0.0 },
                        );
                        let max_e = self.energy_at(best_p, attraction_r, kernel_k);

                        if max_e > 0.1 {
                            let mut len = base_pos.dist(best_p);
                            if len <= 0.001 && is_aarnn {
                                let step = (attraction_r * 0.1).clamp(0.001, 0.01);
                                let step_dir = if dend_pref.mag() > 1.0e-6 {
                                    dend_pref
                                } else {
                                    let mut dx = fastrand::f32() - 0.5;
                                    let mut dy = fastrand::f32() - 0.5;
                                    let mut dz = fastrand::f32() - 0.5;
                                    let inv = 1.0 / (dx * dx + dy * dy + dz * dz).sqrt().max(1e-6);
                                    dx *= inv;
                                    dy *= inv;
                                    dz *= inv;
                                    Point3 {
                                        x: dx,
                                        y: dy,
                                        z: dz,
                                    }
                                };
                                best_p = Point3 {
                                    x: (base_pos.x + step_dir.x * step).clamp(-1.0, 1.0),
                                    y: (base_pos.y + step_dir.y * step).clamp(-1.0, 1.0),
                                    z: (base_pos.z + step_dir.z * step).clamp(-1.0, 1.0),
                                };
                                len = step;
                            }
                            if len > 0.001 {
                                let (dend_type, trunk_len_from_soma) = if is_trunk {
                                    let layout =
                                        dendrite_layout_for_type(soma.type_name.as_deref());
                                    let dtype = if layout.apical_trunks > 0 {
                                        if fastrand::f32() < 0.35 {
                                            DendriteType::Apical
                                        } else {
                                            DendriteType::Basal
                                        }
                                    } else if layout.basal_trunks > 0 {
                                        DendriteType::Basal
                                    } else {
                                        DendriteType::Generic
                                    };
                                    (dtype, len)
                                } else if let Some(pi) = parent_idx {
                                    let dtype = dend
                                        .tree
                                        .branches
                                        .get(pi)
                                        .map(|s| s.dendrite_type)
                                        .unwrap_or(DendriteType::Generic);
                                    let trunk_len = dend
                                        .tree
                                        .branches
                                        .get(pi)
                                        .map(|s| s.trunk_len_from_soma.max(1.0e-6))
                                        .unwrap_or(len);
                                    (dtype, trunk_len)
                                } else {
                                    (DendriteType::Generic, len)
                                };
                                stats.dendrite_sprout_successes += 1;
                                if is_trace {
                                    let name = if is_sensory {
                                        "sensory".to_string()
                                    } else if is_output {
                                        "output".to_string()
                                    } else {
                                        format!("hidden {}", l_idx)
                                    };
                                    nm_log!(
                                        "[trace] dendrite sprouted: {}:{} at {:?} -> {:?} - energy {:.4}",
                                        name,
                                        j,
                                        best_p,
                                        base_pos,
                                        local_e
                                    );
                                }
                                // Sprout a new branch/trunk towards the energy peak
                                new_dendrite_branches.push((
                                    l_idx,
                                    j,
                                    DendSeg {
                                        from: best_p,
                                        to: base_pos,
                                        length: len,
                                        dendrite_type: dend_type,
                                        trunk_len_from_soma,
                                        stimuli: 0.5,
                                        parent_idx,
                                        syn_index: None,
                                        is_trunk,
                                    },
                                ));
                                observe_hit!("morphology/dendrite_sprouted");
                            }
                        } else {
                            stats.dendrite_sprout_low_energy += 1;
                        }
                    }
                }

                // Contact Detection for all existing dendrite tips using the grid
                for seg_idx in 0..dendrites_layer[j].tree.branches.len() {
                    let tip = dendrites_layer[j].tree.branches[seg_idx].from;
                    let tip_energy = self.energy_at(tip, attraction_r, kernel_k);
                    stats.contact_tip_energy_sum += tip_energy;
                    stats.contact_tip_energy_count += 1;
                    if stats.contact_tip_energy_count == 1 {
                        stats.contact_tip_energy_min = tip_energy;
                        stats.contact_tip_energy_max = tip_energy;
                    } else {
                        stats.contact_tip_energy_min = stats.contact_tip_energy_min.min(tip_energy);
                        stats.contact_tip_energy_max = stats.contact_tip_energy_max.max(tip_energy);
                    }

                    let mut low_energy_probe = false;
                    if tip_energy < skip_threshold {
                        stats.contact_skipped_low_energy += 1;
                        if is_aarnn && tip_energy > ambient * 1.2 {
                            // Low-energy tips still do a small exploratory sample.
                            low_energy_probe = true;
                        } else {
                            continue;
                        }
                    }
                    let energy_scale = if is_aarnn {
                        (tip_energy / (t_ema + t_dev + 0.05)).clamp(0.2, 2.2)
                    } else {
                        (tip_energy / (ambient * 2.0 + 0.05)).clamp(0.2, 2.2)
                    };
                    if !pre_full_marks.is_empty() {
                        tip_mark = tip_mark.wrapping_add(1);
                        if tip_mark == 0 {
                            pre_full_marks.fill(0);
                            tip_mark = 1;
                        }
                    }
                    let mut made_connection = false;
                    if is_aarnn {
                        if let Some(post_i) = post_idx {
                            if max_conn_per_neuron > 0
                                && !allow_conn_soft(conn_counts[post_i], max_conn_per_neuron)
                            {
                                stats.contact_post_cap_skips += 1;
                                continue;
                            }
                        }
                    }
                    if !small_net_axon_by_neuron.is_empty() {
                        let total = small_net_axon_by_neuron.len();
                        let max_pre = 4usize.min(total.saturating_sub(1));
                        let mut picked_pre = 0usize;
                        let mut scan = 0usize;
                        while picked_pre < max_pre && scan < total {
                            let pre_idx = probe_rr % total;
                            probe_rr = probe_rr.wrapping_add(1);
                            scan += 1;
                            if Some(pre_idx) == post_idx {
                                continue;
                            }
                            if !pre_full_marks.is_empty() && pre_full_marks[pre_idx] == tip_mark {
                                continue;
                            }
                            if let Some(post_i) = post_idx {
                                if let Some(list) = connected_pre_by_post.get(post_i) {
                                    if list.get(&pre_idx).copied().unwrap_or(0) >= pair_cap {
                                        continue;
                                    }
                                }
                            }
                            picked_pre += 1;
                            let segs = &small_net_axon_by_neuron[pre_idx];
                            if segs.is_empty() {
                                if let Some(&(al, aj)) = neuron_ref_by_index.get(pre_idx) {
                                    if let Some(budget) = contact_check_budget {
                                        if stats.contact_checks >= budget {
                                            budget_exhausted = true;
                                            break;
                                        }
                                    }
                                    if !is_output && al == l_idx && aj == j {
                                        stats.contact_self_skips += 1;
                                        continue;
                                    }
                                    stats.contact_checks += 1;
                                    if let Some(post_i) = post_idx {
                                        if let Some(list) = connected_pre_by_post.get(post_i) {
                                            if list.get(&pre_idx).copied().unwrap_or(0) >= pair_cap
                                            {
                                                stats.contact_existing_skips += 1;
                                                continue;
                                            }
                                        }
                                    }
                                    if is_pair_full(
                                        &pair_counts,
                                        pack_neuron_pair(al, aj, l_idx, j),
                                        pair_cap,
                                    ) {
                                        stats.contact_existing_skips += 1;
                                        if !pre_full_marks.is_empty() {
                                            pre_full_marks[pre_idx] = tip_mark;
                                        }
                                        continue;
                                    }
                                    stats.contact_probe_checks += 1;
                                    stats.contact_candidates += 1;
                                    let mut migration_idx: Option<usize> = None;
                                    let compatible = if al == -1 {
                                        if l_idx != in_l as isize {
                                            false
                                        } else if aj < sensory_conn_counts.len()
                                            && sensory_conn_counts[aj]
                                                >= config.max_sensory_connections
                                        {
                                            let mut best_si = None;
                                            let mut max_d2 = -1.0;
                                            for &si in &sensory_syn_indices[aj] {
                                                let d2 = self.synapses[si]
                                                    .pre_site
                                                    .dist_sq(self.synapses[si].post_site);
                                                if d2 > max_d2 {
                                                    max_d2 = d2;
                                                    best_si = Some(si);
                                                }
                                            }
                                            if let Some(si) = best_si {
                                                let pre_pos = neuron_positions[pre_idx];
                                                let new_d2 = tip.dist_sq(pre_pos);
                                                let tip_e = tip_energy;
                                                if tip_e > 0.2 && new_d2 < max_d2 * 0.7 {
                                                    migration_idx = Some(si);
                                                    true
                                                } else {
                                                    false
                                                }
                                            } else {
                                                false
                                            }
                                        } else {
                                            l_idx == in_l as isize
                                        }
                                    } else if is_output {
                                        if al != out_l as isize {
                                            false
                                        } else if j < output_conn_counts.len()
                                            && output_conn_counts[j]
                                                >= config.max_output_connections
                                        {
                                            let mut best_si = None;
                                            let mut max_d2 = -1.0;
                                            for &si in &output_syn_indices[j] {
                                                let d2 = self.synapses[si]
                                                    .pre_site
                                                    .dist_sq(self.synapses[si].post_site);
                                                if d2 > max_d2 {
                                                    max_d2 = d2;
                                                    best_si = Some(si);
                                                }
                                            }
                                            if let Some(si) = best_si {
                                                let pre_pos = neuron_positions[pre_idx];
                                                let new_d2 = tip.dist_sq(pre_pos);
                                                let tip_e = tip_energy;
                                                if tip_e > 0.2 && new_d2 < max_d2 * 0.7 {
                                                    migration_idx = Some(si);
                                                    true
                                                } else {
                                                    false
                                                }
                                            } else {
                                                false
                                            }
                                        } else {
                                            true
                                        }
                                    } else {
                                        (l_idx == al + 1)
                                            || (al == l_idx + 1)
                                            || (is_aarnn && l_idx == al)
                                    };
                                    if !compatible {
                                        stats.contact_incompatible += 1;
                                        continue;
                                    }
                                    if let Some(pre_pos) = neuron_positions.get(pre_idx) {
                                        let contact2 = contact_dist * contact_dist;
                                        let d2 = tip.dist_sq(*pre_pos);
                                        if d2 < contact2 {
                                            let pre_i = pre_idx;
                                            let post_i = post_idx.unwrap_or(pre_idx);
                                            if is_aarnn && max_conn_per_neuron > 0 {
                                                let pre_ok = allow_conn_soft(
                                                    conn_counts[pre_i],
                                                    max_conn_per_neuron,
                                                );
                                                let post_ok = allow_conn_soft(
                                                    conn_counts[post_i],
                                                    max_conn_per_neuron,
                                                );
                                                if !(pre_ok && post_ok) && migration_idx.is_none() {
                                                    stats.contact_rejected_cap += 1;
                                                    continue;
                                                }

                                                if let Some(si) = migration_idx {
                                                    let old_pre_l = self.synapses[si].pre_layer;
                                                    let old_pre_id = self.synapses[si].pre_id;
                                                    let old_post_l = self.synapses[si].post_layer;
                                                    let old_post_id = self.synapses[si].post_id;

                                                    let old_pair_key = pack_neuron_pair(
                                                        old_pre_l,
                                                        old_pre_id,
                                                        old_post_l,
                                                        old_post_id,
                                                    );
                                                    if let Some(count) =
                                                        pair_counts.get_mut(&old_pair_key)
                                                    {
                                                        if *count > 0 {
                                                            *count -= 1;
                                                        }
                                                    }

                                                    if let (Some(pre_idx_old), Some(post_idx_old)) = (
                                                        index_of(old_pre_l, old_pre_id),
                                                        index_of(old_post_l, old_post_id),
                                                    ) {
                                                        conn_counts[pre_idx_old] = conn_counts
                                                            [pre_idx_old]
                                                            .saturating_sub(1);
                                                        conn_counts[post_idx_old] = conn_counts
                                                            [post_idx_old]
                                                            .saturating_sub(1);
                                                    }

                                                    res.migrations.push(MigrationInfo {
                                                        syn_idx: si,
                                                        new_pre_l: al,
                                                        new_pre_id: aj,
                                                        new_post_l: l_idx,
                                                        new_post_id: j,
                                                        new_dsi: seg_idx,
                                                        new_asi: 0, // Soma probe has no segment
                                                        new_pre_site: *pre_pos,
                                                        new_post_site: tip,
                                                    });

                                                    if al == -1 {
                                                        nm_log!(
                                                            "[info] sensory connection migration planned (soma): sensory:{} moves to hidden {}:{} (closer target)",
                                                            aj,
                                                            l_idx,
                                                            j
                                                        );
                                                    } else if is_output {
                                                        nm_log!(
                                                            "[info] output connection migration planned (soma): hidden {}:{} moves to output:{} (closer target)",
                                                            al,
                                                            aj,
                                                            j
                                                        );
                                                    } else {
                                                        nm_log!(
                                                            "[info] synapse migration planned (soma): {}:{} -> {}:{} (closer target)",
                                                            al,
                                                            aj,
                                                            l_idx,
                                                            j
                                                        );
                                                    }
                                                }

                                                conn_counts[pre_i] += 1;
                                                conn_counts[post_i] += 1;
                                            }
                                            stats.contact_successes += 1;
                                            let w = config.initial_synaptic_weight;

                                            if migration_idx.is_none() {
                                                res.new_connections.push((al, aj, l_idx, j, w));
                                                if al == -1 && aj < sensory_conn_counts.len() {
                                                    sensory_conn_counts[aj] += 1;
                                                }
                                                if is_output && j < output_conn_counts.len() {
                                                    output_conn_counts[j] += 1;
                                                }
                                                if is_trace && trace_synapse_count < 100 {
                                                    trace_synapse_count += 1;
                                                    let pre_name = if al == -1 {
                                                        "sensory".to_string()
                                                    } else {
                                                        format!("hidden {}", al)
                                                    };
                                                    let post_name = if is_output {
                                                        "output".to_string()
                                                    } else {
                                                        format!("hidden {}", l_idx)
                                                    };
                                                    nm_log!(
                                                        "[trace] synapse made (soma-probe): {}:{} -> {}:{} - soma proximity",
                                                        pre_name,
                                                        aj,
                                                        post_name,
                                                        j
                                                    );
                                                }
                                                observe_hit!("morphology/synapse_formed");
                                                new_syns.push(Synapse {
                                                    kind: if al == -1 {
                                                        SynKind::In
                                                    } else if is_output {
                                                        SynKind::Out
                                                    } else if al < l_idx {
                                                        SynKind::HiddenFwd
                                                    } else if al > l_idx {
                                                        SynKind::HiddenBwd
                                                    } else {
                                                        SynKind::HiddenRec
                                                    },
                                                    pre_layer: al,
                                                    pre_id: aj,
                                                    post_layer: l_idx,
                                                    post_id: j,
                                                    pre_site: *pre_pos,
                                                    post_site: tip,
                                                    axon_seg_idx: None,
                                                    dend_seg_idx: Some(seg_idx),
                                                    bend: None,
                                                    weight: w,
                                                    p_release: 1.0,
                                                    delay_ms: 1.0,
                                                    stimuli: 1.0,
                                                });
                                                *pair_counts
                                                    .entry(pack_neuron_pair(al, aj, l_idx, j))
                                                    .or_insert(0) += 1;
                                                if let Some(post_i) = post_idx {
                                                    if let Some(list) =
                                                        connected_pre_by_post.get_mut(post_i)
                                                    {
                                                        *list.entry(pre_i).or_insert(0) += 1;
                                                    }
                                                }
                                            }
                                            made_connection = true;
                                            break;
                                        } else {
                                            stats.contact_too_far += 1;
                                        }
                                    }
                                }
                                if made_connection || budget_exhausted {
                                    break;
                                }
                                continue;
                            }
                            let max_segs = 2usize.min(segs.len());
                            for _ in 0..max_segs {
                                if let Some(budget) = contact_check_budget {
                                    if stats.contact_checks >= budget {
                                        budget_exhausted = true;
                                        break;
                                    }
                                }
                                let seg_idx = fastrand::usize(..segs.len());
                                let (al, aj, asi) = segs[seg_idx];
                                if !is_output && al == l_idx && aj == j {
                                    stats.contact_self_skips += 1;
                                    continue;
                                }
                                stats.contact_checks += 1;
                                let pre_neuron_idx = if al == -1 {
                                    neuron_index_sensory.get(aj).copied()
                                } else if al == num_layers as isize {
                                    neuron_index_output.get(aj).copied()
                                } else {
                                    neuron_index_hidden
                                        .get(al as usize)
                                        .and_then(|layer| layer.get(aj).copied())
                                };
                                if let (Some(pre_i), Some(post_i)) = (pre_neuron_idx, post_idx) {
                                    if !pre_full_marks.is_empty()
                                        && pre_full_marks[pre_i] == tip_mark
                                    {
                                        continue;
                                    }
                                    if let Some(list) = connected_pre_by_post.get(post_i) {
                                        if list.get(&pre_i).copied().unwrap_or(0) >= pair_cap {
                                            stats.contact_existing_skips += 1;
                                            if !pre_full_marks.is_empty() {
                                                pre_full_marks[pre_i] = tip_mark;
                                            }
                                            continue;
                                        }
                                    }
                                }
                                if is_pair_full(
                                    &pair_counts,
                                    pack_neuron_pair(al, aj, l_idx, j),
                                    pair_cap,
                                ) {
                                    stats.contact_existing_skips += 1;
                                    if let Some(pre_i) = pre_neuron_idx {
                                        if !pre_full_marks.is_empty() {
                                            pre_full_marks[pre_i] = tip_mark;
                                        }
                                    }
                                    continue;
                                }
                                stats.contact_probe_checks += 1;
                                stats.contact_candidates += 1;

                                let mut migration_idx: Option<usize> = None;
                                let compatible = if al == -1 {
                                    if l_idx != in_l as isize {
                                        false
                                    } else if aj < sensory_conn_counts.len()
                                        && sensory_conn_counts[aj] >= config.max_sensory_connections
                                    {
                                        let mut best_si = None;
                                        let mut max_d2 = -1.0;
                                        for &si in &sensory_syn_indices[aj] {
                                            let d2 = self.synapses[si]
                                                .pre_site
                                                .dist_sq(self.synapses[si].post_site);
                                            if d2 > max_d2 {
                                                max_d2 = d2;
                                                best_si = Some(si);
                                            }
                                        }
                                        if let Some(si) = best_si {
                                            // Use the synapse's current pre_site as the reference for proximity (robust for degenerate segments)
                                            let new_d2 = tip.dist_sq(self.synapses[si].pre_site);
                                            let tip_e = tip_energy;
                                            let near_thresh = (max_d2 * 0.7)
                                                .max(contact_dist * contact_dist * 0.25);
                                            if tip_e > 0.2 && new_d2 < near_thresh {
                                                migration_idx = Some(si);
                                                true
                                            } else {
                                                nm_log!(
                                                    "[DEBUG] sensory migration rejected: aj={}, si={}, tip_e={:.4}, new_d2={:.4}, thresh={:.4} (max_d2*0.7={:.4})",
                                                    aj,
                                                    si,
                                                    tip_e,
                                                    new_d2,
                                                    near_thresh,
                                                    max_d2 * 0.7
                                                );
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    } else {
                                        true
                                    }
                                } else if is_output {
                                    if al != out_l as isize {
                                        false
                                    } else if j < output_conn_counts.len()
                                        && output_conn_counts[j] >= config.max_output_connections
                                    {
                                        let mut best_si = None;
                                        let mut max_d2 = -1.0;
                                        for &si in &output_syn_indices[j] {
                                            let d2 = self.synapses[si]
                                                .pre_site
                                                .dist_sq(self.synapses[si].post_site);
                                            if d2 > max_d2 {
                                                max_d2 = d2;
                                                best_si = Some(si);
                                            }
                                        }
                                        if let Some(si) = best_si {
                                            // Use the synapse's current pre_site as the reference for proximity (robust for degenerate segments)
                                            let new_d2 = tip.dist_sq(self.synapses[si].pre_site);
                                            let tip_e = tip_energy;
                                            let near_thresh = (max_d2 * 0.7)
                                                .max(contact_dist * contact_dist * 0.25);
                                            if tip_e > 0.2 && new_d2 < near_thresh {
                                                migration_idx = Some(si);
                                                true
                                            } else {
                                                nm_log!(
                                                    "[DEBUG] output migration rejected: j={}, si={}, tip_e={:.4}, new_d2={:.4}, thresh={:.4} (max_d2*0.7={:.4})",
                                                    j,
                                                    si,
                                                    tip_e,
                                                    new_d2,
                                                    near_thresh,
                                                    max_d2 * 0.7
                                                );
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    } else {
                                        true
                                    }
                                } else {
                                    (l_idx == al + 1)
                                        || (al == l_idx + 1)
                                        || (is_aarnn && l_idx == al)
                                };
                                if !compatible {
                                    stats.contact_incompatible += 1;
                                    continue;
                                }

                                let aseg = if al == -1 {
                                    &self.sensory_axons[aj].segments[asi]
                                } else {
                                    &self.axons[al as usize][aj].segments[asi]
                                };
                                let contact2 = contact_dist * contact_dist;
                                let (d2, proj) = dist2_point_to_segment(tip, aseg.from, aseg.to);
                                if d2 < contact2 {
                                    let pre_i = pre_idx;
                                    let post_i = post_idx.unwrap_or(pre_idx);
                                    if is_aarnn && max_conn_per_neuron > 0 {
                                        let pre_ok = allow_conn_soft(
                                            conn_counts[pre_i],
                                            max_conn_per_neuron,
                                        );
                                        let post_ok = allow_conn_soft(
                                            conn_counts[post_i],
                                            max_conn_per_neuron,
                                        );
                                        if !(pre_ok && post_ok) && migration_idx.is_none() {
                                            stats.contact_rejected_cap += 1;
                                            continue;
                                        }

                                        if let Some(si) = migration_idx {
                                            let old_pre_l = self.synapses[si].pre_layer;
                                            let old_pre_id = self.synapses[si].pre_id;
                                            let old_post_l = self.synapses[si].post_layer;
                                            let old_post_id = self.synapses[si].post_id;

                                            let old_pair_key = pack_neuron_pair(
                                                old_pre_l,
                                                old_pre_id,
                                                old_post_l,
                                                old_post_id,
                                            );
                                            if let Some(count) = pair_counts.get_mut(&old_pair_key)
                                            {
                                                if *count > 0 {
                                                    *count -= 1;
                                                }
                                            }

                                            if let (Some(pre_idx_old), Some(post_idx_old)) = (
                                                index_of(old_pre_l, old_pre_id),
                                                index_of(old_post_l, old_post_id),
                                            ) {
                                                conn_counts[pre_idx_old] =
                                                    conn_counts[pre_idx_old].saturating_sub(1);
                                                conn_counts[post_idx_old] =
                                                    conn_counts[post_idx_old].saturating_sub(1);
                                            }

                                            res.migrations.push(MigrationInfo {
                                                syn_idx: si,
                                                new_pre_l: al,
                                                new_pre_id: aj,
                                                new_post_l: l_idx,
                                                new_post_id: j,
                                                new_dsi: seg_idx,
                                                new_asi: asi,
                                                new_pre_site: proj,
                                                new_post_site: tip,
                                            });

                                            if al == -1 {
                                                nm_log!(
                                                    "[info] sensory connection migration planned: sensory:{} moves to hidden {}:{} (closer target)",
                                                    aj,
                                                    l_idx,
                                                    j
                                                );
                                            } else if is_output {
                                                nm_log!(
                                                    "[info] output connection migration planned: hidden {}:{} moves to output:{} (closer target)",
                                                    al,
                                                    aj,
                                                    j
                                                );
                                            } else {
                                                nm_log!(
                                                    "[info] synapse migration planned: {}:{} -> {}:{} (closer target)",
                                                    al,
                                                    aj,
                                                    l_idx,
                                                    j
                                                );
                                            }
                                        }

                                        conn_counts[pre_i] += 1;
                                        conn_counts[post_i] += 1;
                                    }
                                    stats.contact_successes += 1;
                                    let w = config.initial_synaptic_weight;

                                    if migration_idx.is_none() {
                                        res.new_connections.push((al, aj, l_idx, j, w));
                                        if al == -1 && aj < sensory_conn_counts.len() {
                                            sensory_conn_counts[aj] += 1;
                                        }
                                        if is_output && j < output_conn_counts.len() {
                                            output_conn_counts[j] += 1;
                                        }
                                        if is_trace && trace_synapse_count < 100 {
                                            trace_synapse_count += 1;
                                            let pre_name = if al == -1 {
                                                "sensory".to_string()
                                            } else {
                                                format!("hidden {}", al)
                                            };
                                            let post_name = if is_output {
                                                "output".to_string()
                                            } else {
                                                format!("hidden {}", l_idx)
                                            };
                                            nm_log!(
                                                "[trace] synapse made (distributed): {}:{} -> {}:{} - contact",
                                                pre_name,
                                                aj,
                                                post_name,
                                                j
                                            );
                                        }
                                        observe_hit!("morphology/synapse_formed");
                                        new_syns.push(Synapse {
                                            kind: if al == -1 {
                                                SynKind::In
                                            } else if is_output {
                                                SynKind::Out
                                            } else if al < l_idx {
                                                SynKind::HiddenFwd
                                            } else if al > l_idx {
                                                SynKind::HiddenBwd
                                            } else {
                                                SynKind::HiddenRec
                                            },
                                            pre_layer: al,
                                            pre_id: aj,
                                            post_layer: l_idx,
                                            post_id: j,
                                            pre_site: proj,
                                            post_site: tip,
                                            axon_seg_idx: Some(asi),
                                            dend_seg_idx: Some(seg_idx),
                                            bend: None,
                                            weight: w,
                                            p_release: 1.0,
                                            delay_ms: 1.0,
                                            stimuli: 1.0,
                                        });
                                        *pair_counts
                                            .entry(pack_neuron_pair(al, aj, l_idx, j))
                                            .or_insert(0) += 1;
                                        if let Some(post_i) = post_idx {
                                            if let Some(list) =
                                                connected_pre_by_post.get_mut(post_i)
                                            {
                                                *list.entry(pre_i).or_insert(0) += 1;
                                            }
                                        }
                                    }
                                    made_connection = true;
                                    break;
                                } else {
                                    stats.contact_too_far += 1;
                                }
                            }
                            if made_connection || budget_exhausted {
                                break;
                            }
                        }
                    }
                    if made_connection {
                        continue;
                    }

                    let max_checks_upper = if total_neurons > 0 {
                        (16.0 + (total_neurons as f32).ln().max(1.0) * 12.0).clamp(16.0, 96.0)
                    } else {
                        96.0
                    };
                    let cap_f: f32 = (cap_base * energy_scale).round();
                    let max_checks_per_tip = if low_energy_probe {
                        cap_f.clamp(2.0_f32, (max_checks_upper * 0.35).max(6.0_f32)) as usize
                    } else {
                        cap_f.clamp(8.0_f32, max_checks_upper) as usize
                    };
                    let mut checks_for_tip = 0usize;
                    let prev_candidates = stats.contact_candidates;
                    axon_index.collect_candidates(tip, contact_dist, &mut axon_candidates);

                    'contact_search: for seg in axon_candidates.iter() {
                        let al = seg.l;
                        let aj = seg.j;
                        let asi = seg.si;
                        if let Some(budget) = contact_check_budget {
                            if stats.contact_checks >= budget {
                                budget_exhausted = true;
                                break 'contact_search;
                            }
                        }
                        stats.contact_checks += 1;
                        checks_for_tip += 1;
                        if checks_for_tip >= max_checks_per_tip {
                            stats.contact_tip_cap_hits += 1;
                            break 'contact_search;
                        }
                        if !is_output && al == l_idx && aj == j {
                            stats.contact_self_skips += 1;
                            continue;
                        }
                        let pre_neuron_idx = if al == -1 {
                            neuron_index_sensory.get(aj).copied()
                        } else if al == num_layers as isize {
                            neuron_index_output.get(aj).copied()
                        } else {
                            neuron_index_hidden
                                .get(al as usize)
                                .and_then(|layer| layer.get(aj).copied())
                        };
                        if let (Some(pre_i), Some(post_i)) = (pre_neuron_idx, post_idx) {
                            if !pre_full_marks.is_empty() && pre_full_marks[pre_i] == tip_mark {
                                continue;
                            }
                            if let Some(list) = connected_pre_by_post.get(post_i) {
                                if list.get(&pre_i).copied().unwrap_or(0) >= pair_cap {
                                    stats.contact_existing_skips += 1;
                                    if !pre_full_marks.is_empty() {
                                        pre_full_marks[pre_i] = tip_mark;
                                    }
                                    continue;
                                }
                            }
                        }
                        if is_pair_full(&pair_counts, pack_neuron_pair(al, aj, l_idx, j), pair_cap)
                        {
                            stats.contact_existing_skips += 1;
                            if let Some(pre_i) = pre_neuron_idx {
                                if !pre_full_marks.is_empty() {
                                    pre_full_marks[pre_i] = tip_mark;
                                }
                            }
                            continue;
                        }
                        stats.contact_candidates += 1;

                        // Only allow connections between compatible layers based on current topology mapping
                        let mut migration_idx: Option<usize> = None;
                        let compatible = if al == -1 {
                            if l_idx != in_l as isize {
                                false
                            } else if aj < sensory_conn_counts.len()
                                && sensory_conn_counts[aj] >= config.max_sensory_connections
                            {
                                let mut best_si = None;
                                let mut max_d2 = -1.0;
                                for &si in &sensory_syn_indices[aj] {
                                    let d2 = self.synapses[si]
                                        .pre_site
                                        .dist_sq(self.synapses[si].post_site);
                                    if d2 > max_d2 {
                                        max_d2 = d2;
                                        best_si = Some(si);
                                    }
                                }
                                if let Some(si) = best_si {
                                    let aseg = &self.sensory_axons[aj].segments[asi];
                                    let (new_d2, _) =
                                        dist2_point_to_segment(tip, aseg.from, aseg.to);
                                    let tip_e = self.energy_at(tip, attraction_r, kernel_k);
                                    if tip_e > 0.2 && new_d2 < max_d2 * 0.7 {
                                        migration_idx = Some(si);
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                l_idx == in_l as isize
                            }
                        } else if is_output {
                            if al != out_l as isize {
                                false
                            } else if j < output_conn_counts.len()
                                && output_conn_counts[j] >= config.max_output_connections
                            {
                                let mut best_si = None;
                                let mut max_d2 = -1.0;
                                for &si in &output_syn_indices[j] {
                                    let d2 = self.synapses[si]
                                        .pre_site
                                        .dist_sq(self.synapses[si].post_site);
                                    if d2 > max_d2 {
                                        max_d2 = d2;
                                        best_si = Some(si);
                                    }
                                }
                                if let Some(si) = best_si {
                                    let aseg = &self.axons[al as usize][aj].segments[asi];
                                    let (new_d2, _) =
                                        dist2_point_to_segment(tip, aseg.from, aseg.to);
                                    let tip_e = self.energy_at(tip, attraction_r, kernel_k);
                                    if tip_e > 0.2 && new_d2 < max_d2 * 0.7 {
                                        migration_idx = Some(si);
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                true
                            }
                        } else {
                            (l_idx == al + 1) || (al == l_idx + 1) || (is_aarnn && l_idx == al)
                        };
                        if !compatible {
                            stats.contact_incompatible += 1;
                            continue;
                        }

                        let aseg = if al == -1 {
                            &self.sensory_axons[aj].segments[asi]
                        } else {
                            &self.axons[al as usize][aj].segments[asi]
                        };
                        let contact2 = contact_dist * contact_dist;
                        let (d2, proj) = dist2_point_to_segment(tip, aseg.from, aseg.to);
                        if d2 < contact2 {
                            let pre_neuron_idx = if al == -1 {
                                neuron_index_sensory.get(aj).copied()
                            } else if al == num_layers as isize {
                                neuron_index_output.get(aj).copied()
                            } else {
                                neuron_index_hidden
                                    .get(al as usize)
                                    .and_then(|layer| layer.get(aj).copied())
                            };
                            let post_neuron_idx = if l_idx == -1 {
                                neuron_index_sensory.get(j).copied()
                            } else if l_idx == num_layers as isize {
                                neuron_index_output.get(j).copied()
                            } else {
                                neuron_index_hidden
                                    .get(l_idx as usize)
                                    .and_then(|layer| layer.get(j).copied())
                            };
                            if is_aarnn && max_conn_per_neuron > 0 {
                                if let (Some(pre_i), Some(post_i)) =
                                    (pre_neuron_idx, post_neuron_idx)
                                {
                                    let pre_ok =
                                        allow_conn_soft(conn_counts[pre_i], max_conn_per_neuron);
                                    let post_ok =
                                        allow_conn_soft(conn_counts[post_i], max_conn_per_neuron);
                                    if !(pre_ok && post_ok) && migration_idx.is_none() {
                                        stats.contact_rejected_cap += 1;
                                        continue;
                                    }
                                    let pre_needs_close = close_neighbor_target > 0
                                        && close_conn_counts[pre_i] < close_neighbor_target;
                                    let post_needs_close = close_neighbor_target > 0
                                        && close_conn_counts[post_i] < close_neighbor_target;
                                    if !migration_idx.is_some()
                                        && ((pre_needs_close
                                            && !is_close_neighbor(&close_neighbors[pre_i], post_i))
                                            || (post_needs_close
                                                && !is_close_neighbor(
                                                    &close_neighbors[post_i],
                                                    pre_i,
                                                )))
                                    {
                                        stats.contact_rejected_close += 1;
                                        continue;
                                    }

                                    if let Some(si) = migration_idx {
                                        let old_pre_l = self.synapses[si].pre_layer;
                                        let old_pre_id = self.synapses[si].pre_id;
                                        let old_post_l = self.synapses[si].post_layer;
                                        let old_post_id = self.synapses[si].post_id;

                                        let old_pair_key = pack_neuron_pair(
                                            old_pre_l,
                                            old_pre_id,
                                            old_post_l,
                                            old_post_id,
                                        );
                                        if let Some(count) = pair_counts.get_mut(&old_pair_key) {
                                            if *count > 0 {
                                                *count -= 1;
                                            }
                                        }

                                        if let (Some(pre_idx_old), Some(post_idx_old)) = (
                                            index_of(old_pre_l, old_pre_id),
                                            index_of(old_post_l, old_post_id),
                                        ) {
                                            conn_counts[pre_idx_old] =
                                                conn_counts[pre_idx_old].saturating_sub(1);
                                            conn_counts[post_idx_old] =
                                                conn_counts[post_idx_old].saturating_sub(1);
                                        }

                                        res.migrations.push(MigrationInfo {
                                            syn_idx: si,
                                            new_pre_l: al,
                                            new_pre_id: aj,
                                            new_post_l: l_idx,
                                            new_post_id: j,
                                            new_dsi: seg_idx,
                                            new_asi: asi,
                                            new_pre_site: proj,
                                            new_post_site: tip,
                                        });

                                        if al == -1 {
                                            nm_log!(
                                                "[info] sensory connection migration planned (spatial): sensory:{} moves to hidden {}:{} (closer target)",
                                                aj,
                                                l_idx,
                                                j
                                            );
                                        } else if is_output {
                                            nm_log!(
                                                "[info] output connection migration planned (spatial): hidden {}:{} moves to output:{} (closer target)",
                                                al,
                                                aj,
                                                j
                                            );
                                        } else {
                                            nm_log!(
                                                "[info] synapse migration planned (spatial): {}:{} -> {}:{} (closer target)",
                                                al,
                                                aj,
                                                l_idx,
                                                j
                                            );
                                        }
                                    }

                                    conn_counts[pre_i] += 1;
                                    conn_counts[post_i] += 1;
                                    if close_neighbor_target > 0 {
                                        if is_close_neighbor(&close_neighbors[pre_i], post_i) {
                                            close_conn_counts[pre_i] += 1;
                                        }
                                        if is_close_neighbor(&close_neighbors[post_i], pre_i) {
                                            close_conn_counts[post_i] += 1;
                                        }
                                    }
                                }
                            }

                            stats.contact_successes += 1;
                            let w = config.initial_synaptic_weight;

                            if migration_idx.is_none() {
                                res.new_connections.push((al, aj, l_idx, j, w));

                                if al == -1 && aj < sensory_conn_counts.len() {
                                    sensory_conn_counts[aj] += 1;
                                }
                                if is_output && j < output_conn_counts.len() {
                                    output_conn_counts[j] += 1;
                                }

                                let pre_name = if al == -1 {
                                    "sensory".to_string()
                                } else {
                                    format!("hidden {}", al)
                                };
                                let post_name = if is_output {
                                    "output".to_string()
                                } else {
                                    format!("hidden {}", l_idx)
                                };
                                nm_log!(
                                    "[trace] synapse made: {}:{} -> {}:{} - physical contact detected",
                                    pre_name,
                                    aj,
                                    post_name,
                                    j
                                );

                                observe_hit!("morphology/synapse_formed");
                                new_syns.push(Synapse {
                                    kind: if al == -1 {
                                        SynKind::In
                                    } else if is_output {
                                        SynKind::Out
                                    } else if al < l_idx {
                                        SynKind::HiddenFwd
                                    } else if al > l_idx {
                                        SynKind::HiddenBwd
                                    } else {
                                        SynKind::HiddenRec
                                    },
                                    pre_layer: al,
                                    pre_id: aj,
                                    post_layer: l_idx,
                                    post_id: j,
                                    pre_site: proj,
                                    post_site: tip,
                                    axon_seg_idx: Some(asi),
                                    dend_seg_idx: Some(seg_idx),
                                    bend: None,
                                    weight: w,
                                    p_release: 1.0,
                                    delay_ms: 1.0,
                                    stimuli: 1.0,
                                });
                                *pair_counts
                                    .entry(pack_neuron_pair(al, aj, l_idx, j))
                                    .or_insert(0) += 1;
                                if let (Some(pre_i), Some(post_i)) =
                                    (pre_neuron_idx, post_neuron_idx)
                                {
                                    if let Some(list) = connected_pre_by_post.get_mut(post_i) {
                                        *list.entry(pre_i).or_insert(0) += 1;
                                    }
                                }
                            }
                            break 'contact_search;
                        } else {
                            stats.contact_too_far += 1;
                        }
                    }
                    if stats.contact_candidates == prev_candidates && !small_net_segments.is_empty()
                    {
                        let probe_limit = 4usize;
                        let probe_contact2 = contact_dist * contact_dist * 6.25;
                        let mut probes = 0usize;
                        while probes < probe_limit {
                            let mut picked = None;
                            let post_idx = if l_idx == -1 {
                                neuron_index_sensory.get(j).copied()
                            } else if l_idx == num_layers as isize {
                                neuron_index_output.get(j).copied()
                            } else {
                                neuron_index_hidden
                                    .get(l_idx as usize)
                                    .and_then(|layer| layer.get(j).copied())
                            };
                            if let Some(total) =
                                Some(small_net_axon_by_neuron.len()).filter(|v| *v > 1)
                            {
                                for _ in 0..total {
                                    let idx = probe_rr % total;
                                    probe_rr = probe_rr.wrapping_add(1);
                                    if Some(idx) == post_idx {
                                        continue;
                                    }
                                    if let Some(post_i) = post_idx {
                                        if let Some(list) = connected_pre_by_post.get(post_i) {
                                            if list.get(&idx).copied().unwrap_or(0) >= pair_cap {
                                                continue;
                                            }
                                        }
                                    }
                                    if small_net_axon_by_neuron[idx].is_empty() {
                                        continue;
                                    }
                                    let segs = &small_net_axon_by_neuron[idx];
                                    let seg_idx = fastrand::usize(..segs.len());
                                    picked = Some(segs[seg_idx]);
                                    break;
                                }
                            }
                            let (al, aj, asi) = picked.unwrap_or_else(|| {
                                let idx = fastrand::usize(..small_net_segments.len());
                                small_net_segments[idx]
                            });
                            if !is_output && al == l_idx && aj == j {
                                stats.contact_self_skips += 1;
                                probes += 1;
                                continue;
                            }
                            stats.contact_checks += 1;
                            stats.contact_probe_checks += 1;
                            let pre_neuron_idx = if al == -1 {
                                neuron_index_sensory.get(aj).copied()
                            } else if al == num_layers as isize {
                                neuron_index_output.get(aj).copied()
                            } else {
                                neuron_index_hidden
                                    .get(al as usize)
                                    .and_then(|layer| layer.get(aj).copied())
                            };
                            if let (Some(pre_i), Some(post_i)) = (pre_neuron_idx, post_idx) {
                                if !pre_full_marks.is_empty() && pre_full_marks[pre_i] == tip_mark {
                                    probes += 1;
                                    continue;
                                }
                                if let Some(list) = connected_pre_by_post.get(post_i) {
                                    if list.get(&pre_i).copied().unwrap_or(0) >= pair_cap {
                                        stats.contact_existing_skips += 1;
                                        if !pre_full_marks.is_empty() {
                                            pre_full_marks[pre_i] = tip_mark;
                                        }
                                        probes += 1;
                                        continue;
                                    }
                                }
                            }
                            if is_pair_full(
                                &pair_counts,
                                pack_neuron_pair(al, aj, l_idx, j),
                                pair_cap,
                            ) {
                                stats.contact_existing_skips += 1;
                                if let Some(pre_i) = pre_neuron_idx {
                                    if !pre_full_marks.is_empty() {
                                        pre_full_marks[pre_i] = tip_mark;
                                    }
                                }
                                probes += 1;
                                continue;
                            }
                            stats.contact_candidates += 1;

                            let mut migration_idx: Option<usize> = None;
                            let compatible = if al == -1 {
                                if l_idx != in_l as isize {
                                    false
                                } else if aj < sensory_conn_counts.len()
                                    && sensory_conn_counts[aj] >= config.max_sensory_connections
                                {
                                    let mut best_si = None;
                                    let mut max_d2 = -1.0;
                                    for &si in &sensory_syn_indices[aj] {
                                        let d2 = self.synapses[si]
                                            .pre_site
                                            .dist_sq(self.synapses[si].post_site);
                                        if d2 > max_d2 {
                                            max_d2 = d2;
                                            best_si = Some(si);
                                        }
                                    }
                                    if let Some(si) = best_si {
                                        let aseg = &self.sensory_axons[aj].segments[asi];
                                        let (new_d2, _) =
                                            dist2_point_to_segment(tip, aseg.from, aseg.to);
                                        let tip_e = self.energy_at(tip, attraction_r, kernel_k);
                                        if tip_e > 0.2 && new_d2 < max_d2 * 0.7 {
                                            migration_idx = Some(si);
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            } else if is_output {
                                if al != out_l as isize {
                                    false
                                } else if j < output_conn_counts.len()
                                    && output_conn_counts[j] >= config.max_output_connections
                                {
                                    let mut best_si = None;
                                    let mut max_d2 = -1.0;
                                    for &si in &output_syn_indices[j] {
                                        let d2 = self.synapses[si]
                                            .pre_site
                                            .dist_sq(self.synapses[si].post_site);
                                        if d2 > max_d2 {
                                            max_d2 = d2;
                                            best_si = Some(si);
                                        }
                                    }
                                    if let Some(si) = best_si {
                                        let aseg = &self.axons[al as usize][aj].segments[asi];
                                        let (new_d2, _) =
                                            dist2_point_to_segment(tip, aseg.from, aseg.to);
                                        let tip_e = self.energy_at(tip, attraction_r, kernel_k);
                                        if tip_e > 0.2 && new_d2 < max_d2 * 0.7 {
                                            migration_idx = Some(si);
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            } else {
                                (l_idx == al + 1) || (al == l_idx + 1) || (is_aarnn && l_idx == al)
                            };
                            if !compatible {
                                stats.contact_incompatible += 1;
                                probes += 1;
                                continue;
                            }

                            let aseg = if al == -1 {
                                &self.sensory_axons[aj].segments[asi]
                            } else {
                                &self.axons[al as usize][aj].segments[asi]
                            };
                            let (d2, proj) = dist2_point_to_segment(tip, aseg.from, aseg.to);
                            if d2 < probe_contact2 {
                                let pre_neuron_idx = if al == -1 {
                                    neuron_index_sensory.get(aj).copied()
                                } else if al == num_layers as isize {
                                    neuron_index_output.get(aj).copied()
                                } else {
                                    neuron_index_hidden
                                        .get(al as usize)
                                        .and_then(|layer| layer.get(aj).copied())
                                };
                                let post_neuron_idx = if l_idx == -1 {
                                    neuron_index_sensory.get(j).copied()
                                } else if l_idx == num_layers as isize {
                                    neuron_index_output.get(j).copied()
                                } else {
                                    neuron_index_hidden
                                        .get(l_idx as usize)
                                        .and_then(|layer| layer.get(j).copied())
                                };

                                if is_aarnn && max_conn_per_neuron > 0 {
                                    if let (Some(pre_i), Some(post_i)) =
                                        (pre_neuron_idx, post_neuron_idx)
                                    {
                                        let pre_ok = allow_conn_soft(
                                            conn_counts[pre_i],
                                            max_conn_per_neuron,
                                        );
                                        let post_ok = allow_conn_soft(
                                            conn_counts[post_i],
                                            max_conn_per_neuron,
                                        );
                                        if !(pre_ok && post_ok) && migration_idx.is_none() {
                                            // Reject
                                            probes += 1;
                                            continue;
                                        }

                                        if let Some(si) = migration_idx {
                                            let old_pre_l = self.synapses[si].pre_layer;
                                            let old_pre_id = self.synapses[si].pre_id;
                                            let old_post_l = self.synapses[si].post_layer;
                                            let old_post_id = self.synapses[si].post_id;

                                            let old_pair_key = pack_neuron_pair(
                                                old_pre_l,
                                                old_pre_id,
                                                old_post_l,
                                                old_post_id,
                                            );
                                            if let Some(count) = pair_counts.get_mut(&old_pair_key)
                                            {
                                                if *count > 0 {
                                                    *count -= 1;
                                                }
                                            }

                                            if let (Some(pre_idx_old), Some(post_idx_old)) = (
                                                index_of(old_pre_l, old_pre_id),
                                                index_of(old_post_l, old_post_id),
                                            ) {
                                                conn_counts[pre_idx_old] =
                                                    conn_counts[pre_idx_old].saturating_sub(1);
                                                conn_counts[post_idx_old] =
                                                    conn_counts[post_idx_old].saturating_sub(1);
                                            }

                                            res.migrations.push(MigrationInfo {
                                                syn_idx: si,
                                                new_pre_l: al,
                                                new_pre_id: aj,
                                                new_post_l: l_idx,
                                                new_post_id: j,
                                                new_dsi: seg_idx,
                                                new_asi: asi,
                                                new_pre_site: proj,
                                                new_post_site: tip,
                                            });

                                            if al == -1 {
                                                nm_log!(
                                                    "[info] sensory connection migration planned (exploratory): sensory:{} moves to hidden {}:{} (closer target)",
                                                    aj,
                                                    l_idx,
                                                    j
                                                );
                                            } else if is_output {
                                                nm_log!(
                                                    "[info] output connection migration planned (exploratory): hidden {}:{} moves to output:{} (closer target)",
                                                    al,
                                                    aj,
                                                    j
                                                );
                                            } else {
                                                nm_log!(
                                                    "[info] synapse migration planned (exploratory): {}:{} -> {}:{} (closer target)",
                                                    al,
                                                    aj,
                                                    l_idx,
                                                    j
                                                );
                                            }
                                        }

                                        conn_counts[pre_i] += 1;
                                        conn_counts[post_i] += 1;
                                    }
                                }

                                stats.contact_successes += 1;
                                let w = config.initial_synaptic_weight;

                                if migration_idx.is_none() {
                                    res.new_connections.push((al, aj, l_idx, j, w));
                                    if al == -1 && aj < sensory_conn_counts.len() {
                                        sensory_conn_counts[aj] += 1;
                                    }
                                    if is_output && j < output_conn_counts.len() {
                                        output_conn_counts[j] += 1;
                                    }
                                    if is_trace && trace_synapse_count < 100 {
                                        trace_synapse_count += 1;
                                        let pre_name = if al == -1 {
                                            "sensory".to_string()
                                        } else {
                                            format!("hidden {}", al)
                                        };
                                        let post_name = if is_output {
                                            "output".to_string()
                                        } else {
                                            format!("hidden {}", l_idx)
                                        };
                                        nm_log!(
                                            "[trace] synapse made (probe): {}:{} -> {}:{} - exploratory contact",
                                            pre_name,
                                            aj,
                                            post_name,
                                            j
                                        );
                                    }
                                    observe_hit!("morphology/synapse_formed");
                                    new_syns.push(Synapse {
                                        kind: if al == -1 {
                                            SynKind::In
                                        } else if is_output {
                                            SynKind::Out
                                        } else if al < l_idx {
                                            SynKind::HiddenFwd
                                        } else if al > l_idx {
                                            SynKind::HiddenBwd
                                        } else {
                                            SynKind::HiddenRec
                                        },
                                        pre_layer: al,
                                        pre_id: aj,
                                        post_layer: l_idx,
                                        post_id: j,
                                        pre_site: proj,
                                        post_site: tip,
                                        axon_seg_idx: Some(asi),
                                        dend_seg_idx: Some(seg_idx),
                                        bend: None,
                                        weight: w,
                                        p_release: 1.0,
                                        delay_ms: 1.0,
                                        stimuli: 1.0,
                                    });
                                    *pair_counts
                                        .entry(pack_neuron_pair(al, aj, l_idx, j))
                                        .or_insert(0) += 1;
                                    if let (Some(pre_i), Some(post_i)) =
                                        (pre_neuron_idx, post_neuron_idx)
                                    {
                                        if let Some(list) = connected_pre_by_post.get_mut(post_i) {
                                            *list.entry(pre_i).or_insert(0) += 1;
                                        }
                                    }
                                }
                                break;
                            } else {
                                stats.contact_too_far += 1;
                            }
                            probes += 1;
                        }
                    }
                    if budget_exhausted {
                        break;
                    }
                }
                if budget_exhausted {
                    break;
                }
            }
            if budget_exhausted {
                break 'contact_all;
            }
        }

        // Apply sprouting and new connections
        for (l, j, branch) in new_dendrite_branches {
            if l == -1 {
                self.sensory_dendrites[j].tree.branches.push(branch);
            } else if l == num_layers as isize {
                self.output_dendrites[j].tree.branches.push(branch);
            } else {
                self.dendrites[l as usize][j].tree.branches.push(branch);
            }
        }
        for (l, j, seg) in new_axon_branches {
            if l == -1 {
                self.sensory_axons[j].segments.push(seg);
            } else if l == num_layers as isize {
                self.output_axons[j].segments.push(seg);
            } else {
                self.axons[l as usize][j].segments.push(seg);
            }
        }
        for syn in new_syns {
            // Final deduplication before adding to flat vector
            if !self.synapses.iter().any(|s| {
                s.pre_layer == syn.pre_layer
                    && s.pre_id == syn.pre_id
                    && s.post_layer == syn.post_layer
                    && s.post_id == syn.post_id
            }) {
                let si = self.synapses.len();
                // Link segment to synapse
                if let Some(asi) = syn.axon_seg_idx {
                    if syn.pre_layer == -1 {
                        if syn.pre_id < self.sensory_axons.len()
                            && asi < self.sensory_axons[syn.pre_id].segments.len()
                        {
                            self.sensory_axons[syn.pre_id].segments[asi].syn_index = Some(si);
                        }
                    } else if syn.pre_layer == num_layers as isize {
                        if syn.pre_id < self.output_axons.len()
                            && asi < self.output_axons[syn.pre_id].segments.len()
                        {
                            self.output_axons[syn.pre_id].segments[asi].syn_index = Some(si);
                        }
                    } else if syn.pre_layer >= 0 && (syn.pre_layer as usize) < self.axons.len() {
                        let l = syn.pre_layer as usize;
                        if syn.pre_id < self.axons[l].len()
                            && asi < self.axons[l][syn.pre_id].segments.len()
                        {
                            self.axons[l][syn.pre_id].segments[asi].syn_index = Some(si);
                        }
                    }
                }
                if let Some(dsi) = syn.dend_seg_idx {
                    if syn.post_layer == -1 {
                        if syn.post_id < self.sensory_dendrites.len()
                            && dsi < self.sensory_dendrites[syn.post_id].tree.branches.len()
                        {
                            self.sensory_dendrites[syn.post_id].tree.branches[dsi].syn_index =
                                Some(si);
                        }
                    } else if syn.post_layer == num_layers as isize {
                        if syn.post_id < self.output_dendrites.len()
                            && dsi < self.output_dendrites[syn.post_id].tree.branches.len()
                        {
                            self.output_dendrites[syn.post_id].tree.branches[dsi].syn_index =
                                Some(si);
                        }
                    } else if syn.post_layer >= 0
                        && (syn.post_layer as usize) < self.dendrites.len()
                    {
                        let l = syn.post_layer as usize;
                        if syn.post_id < self.dendrites[l].len()
                            && dsi < self.dendrites[l][syn.post_id].tree.branches.len()
                        {
                            self.dendrites[l][syn.post_id].tree.branches[dsi].syn_index = Some(si);
                        }
                    }
                }
                self.synapses.push(syn);
            }
        }

        // Post-pass conservative migration repair for capped sensory/output connections
        // If an existing synapse has a much closer eligible partner, plan a migration.
        {
            let contact2 = contact_dist * contact_dist;
            // Sensory -> Hidden layer
            let mut syn_by_sens: FastHashMap<usize, Vec<usize>> = FastHashMap::default();
            for (si, s) in self.synapses.iter().enumerate() {
                if s.kind as i32 == 0 || s.pre_layer == -1 {
                    // SynKind::In or sensory pre-layer
                    syn_by_sens.entry(s.pre_id).or_default().push(si);
                }
            }

            // Pre-calculate dendrite candidate positions for in_l
            let dend_cands_in = if in_l < self.dendrites.len() {
                self.dendrites[in_l]
                    .iter()
                    .enumerate()
                    .map(|(j, d)| {
                        d.tree
                            .branches
                            .get(0)
                            .map(|b| b.from)
                            .unwrap_or(self.somas[in_l][j].pos)
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            for (sens_id, sis) in syn_by_sens {
                if sis.is_empty() {
                    continue;
                }
                // Only consider when at cap (or above)
                if config.max_sensory_connections > 0 && sis.len() >= config.max_sensory_connections
                {
                    for &si in &sis {
                        let s = &self.synapses[si];
                        if s.post_layer != in_l as isize {
                            continue;
                        }
                        let pre_p = s.pre_site;
                        let cur_post = s.post_id;
                        let cur_post_p = if cur_post < dend_cands_in.len() {
                            dend_cands_in[cur_post]
                        } else {
                            self.somas[in_l][cur_post].pos
                        };
                        let cur_d2 = pre_p.dist_sq(cur_post_p);
                        let mut best_j = cur_post;
                        let mut best_d2 = cur_d2;

                        for (j, &cand_p) in dend_cands_in.iter().enumerate() {
                            if j == cur_post {
                                continue;
                            }
                            let d2 = pre_p.dist_sq(cand_p);
                            if d2 < best_d2 {
                                best_d2 = d2;
                                best_j = j;
                            }
                        }
                        let tip_e = self.energy_at(pre_p, attraction_r, kernel_k);
                        // nm_log!("[DEBUG] post-fallback sensory eval: si={} cur_post={} cur_d2={:.6} best_j={} best_d2={:.6} contact2={:.6} tip_e={:.4}", si, cur_post, cur_d2, best_j, best_d2, contact2, tip_e);
                        if best_j != cur_post
                            && best_d2 < cur_d2 * 0.7
                            && best_d2 < contact2
                            && tip_e > 0.2
                        {
                            // Plan migration
                            let new_dsi = 0usize.min(
                                self.dendrites[in_l][best_j]
                                    .tree
                                    .branches
                                    .len()
                                    .saturating_sub(1),
                            );
                            let new_post_site = if let Some(seg) =
                                self.dendrites[in_l][best_j].tree.branches.get(new_dsi)
                            {
                                seg.from
                            } else {
                                self.somas[in_l][best_j].pos
                            };
                            nm_log!(
                                "[info] post-fallback planned sensory migration: si={} post {}->{} (d2 {:.6}->{:.6})",
                                si,
                                cur_post,
                                best_j,
                                cur_d2,
                                best_d2
                            );
                            res.migrations.push(MigrationInfo {
                                syn_idx: si,
                                new_pre_l: -1,
                                new_pre_id: sens_id,
                                new_post_l: in_l as isize,
                                new_post_id: best_j,
                                new_dsi,
                                new_asi: s.axon_seg_idx.unwrap_or(0),
                                new_pre_site: pre_p,
                                new_post_site,
                            });
                        }
                    }
                }
            }
            // Output <- Hidden layer
            let mut syn_by_out: FastHashMap<usize, Vec<usize>> = FastHashMap::default();
            for (si, s) in self.synapses.iter().enumerate() {
                if s.kind as i32 == 2 || s.post_layer == num_layers as isize {
                    // SynKind::Out or output post-layer
                    syn_by_out.entry(s.post_id).or_default().push(si);
                }
            }

            // Pre-calculate axon candidate positions for all hidden layers
            let axon_cands_by_layer: Vec<Vec<Point3>> = self
                .axons
                .iter()
                .enumerate()
                .map(|(l, layer)| {
                    layer
                        .iter()
                        .enumerate()
                        .map(|(i, ax)| {
                            ax.segments
                                .get(0)
                                .map(|seg| seg.to)
                                .unwrap_or(self.somas[l][i].pos)
                        })
                        .collect()
                })
                .collect();

            for (out_id, sis) in syn_by_out {
                if sis.is_empty() {
                    continue;
                }
                if config.max_output_connections > 0 && sis.len() >= config.max_output_connections {
                    for &si in &sis {
                        let s = &self.synapses[si];
                        if s.pre_layer < 0 {
                            continue;
                        }
                        let pre_l = s.pre_layer as usize;
                        let pre_id = s.pre_id;
                        let post_p = s.post_site;

                        let cur_pre_p = if pre_l < axon_cands_by_layer.len()
                            && pre_id < axon_cands_by_layer[pre_l].len()
                        {
                            axon_cands_by_layer[pre_l][pre_id]
                        } else {
                            self.somas[pre_l][pre_id].pos
                        };

                        let cur_d2 = post_p.dist_sq(cur_pre_p);
                        let mut best_i = pre_id;
                        let mut best_d2 = cur_d2;

                        if pre_l < axon_cands_by_layer.len() {
                            for (i, &cand_p) in axon_cands_by_layer[pre_l].iter().enumerate() {
                                if i == pre_id {
                                    continue;
                                }
                                let d2 = post_p.dist_sq(cand_p);
                                if d2 < best_d2 {
                                    best_d2 = d2;
                                    best_i = i;
                                }
                            }
                        }
                        let tip_e = self.energy_at(post_p, attraction_r, kernel_k);
                        // nm_log!("[DEBUG] post-fallback output eval: si={} cur_pre={} cur_d2={:.6} best_i={} best_d2={:.6} contact2={:.6} tip_e={:.4}", si, pre_id, cur_d2, best_i, best_d2, contact2, tip_e);
                        if best_i != pre_id
                            && best_d2 < cur_d2 * 0.7
                            && best_d2 < contact2
                            && tip_e > 0.2
                        {
                            let new_asi = 0usize
                                .min(self.axons[pre_l][best_i].segments.len().saturating_sub(1));
                            let new_pre_site = if let Some(seg) =
                                self.axons[pre_l][best_i].segments.get(new_asi)
                            {
                                seg.to
                            } else {
                                self.somas[pre_l][best_i].pos
                            };
                            nm_log!(
                                "[info] post-fallback planned output migration: si={} pre {}->{} (d2 {:.6}->{:.6})",
                                si,
                                pre_id,
                                best_i,
                                cur_d2,
                                best_d2
                            );
                            res.migrations.push(MigrationInfo {
                                syn_idx: si,
                                new_pre_l: pre_l as isize,
                                new_pre_id: best_i,
                                new_post_l: num_layers as isize,
                                new_post_id: out_id,
                                new_dsi: s.dend_seg_idx.unwrap_or(0),
                                new_asi,
                                new_pre_site,
                                new_post_site: post_p,
                            });
                        }
                    }
                }
            }
        }

        if should_log {
            let total_axons: usize = self.axons.iter().map(|l| l.len()).sum::<usize>()
                + self.sensory_axons.len()
                + self.output_axons.len();
            let total_dendrites: usize = self.dendrites.iter().map(|l| l.len()).sum::<usize>()
                + self.sensory_dendrites.len()
                + self.output_dendrites.len();
            let tip_avg = if stats.contact_tip_energy_count == 0 {
                0.0
            } else {
                stats.contact_tip_energy_sum / stats.contact_tip_energy_count as f32
            };
            if is_aarnn && stats.contact_tip_energy_count > 0 {
                let mut tuning = morpho_energy_tuning()
                    .lock()
                    .expect("morpho tuning lock poisoned");
                let alpha = 0.05;
                let prev = tuning.ema;
                tuning.ema = prev * (1.0 - alpha) + tip_avg * alpha;
                tuning.dev = tuning.dev * 0.9 + (tip_avg - prev).abs() * 0.1;
                let tip_count = stats.contact_tip_energy_count.max(1) as f32;
                let cap_pressure = (stats.contact_tip_cap_hits as f32 / tip_count).clamp(0.0, 1.0);
                let too_far_ratio = if stats.contact_candidates == 0 {
                    0.0
                } else {
                    stats.contact_too_far as f32 / stats.contact_candidates as f32
                };
                let no_success = stats.contact_successes == 0;
                let zero_checks = stats.contact_checks == 0 || stats.contact_candidates == 0;
                let cap_reject_ratio = if stats.contact_candidates == 0 {
                    0.0
                } else {
                    stats.contact_rejected_cap as f32 / stats.contact_candidates as f32
                };
                if no_success && cap_reject_ratio > 0.05 {
                    tuning.cap_scale = (tuning.cap_scale * 1.05).clamp(0.3, 1.2);
                    tuning.skip_bias = (tuning.skip_bias * 0.98).clamp(1.0, 2.0);
                }
                let high_cap_pressure = cap_pressure > 0.6;
                let high_too_far = too_far_ratio > 0.4;
                if no_success && high_cap_pressure && high_too_far && tip_avg > ambient * 1.2 {
                    // Under-connected: allow deeper probing and relax skip bias.
                    tuning.cap_scale = (tuning.cap_scale * 1.10).clamp(0.3, 1.2);
                    tuning.skip_bias = (tuning.skip_bias * 0.98).clamp(1.0, 2.0);
                } else if zero_checks && stats.contact_skipped_low_energy > 0 {
                    // Over-skipping: relax to allow some contact search.
                    tuning.cap_scale = (tuning.cap_scale * 1.25).clamp(0.3, 1.2);
                    tuning.skip_bias = (tuning.skip_bias * 0.90).clamp(1.0, 2.0);
                } else if no_success
                    && (cap_pressure > 0.4
                        || too_far_ratio > 0.6
                        || stats.dendrite_sprout_successes == 0)
                {
                    tuning.cap_scale = (tuning.cap_scale * 0.85).clamp(0.3, 1.2);
                    tuning.skip_bias = (tuning.skip_bias * 1.05).clamp(1.0, 2.0);
                } else {
                    tuning.cap_scale = (tuning.cap_scale * 1.02).clamp(0.3, 1.2);
                    tuning.skip_bias = (tuning.skip_bias * 0.99).clamp(1.0, 2.0);
                }
            }
            let (t_ema, t_dev, t_cap, t_skip) = if is_aarnn {
                let tuning = morpho_energy_tuning()
                    .lock()
                    .expect("morpho tuning lock poisoned");
                (tuning.ema, tuning.dev, tuning.cap_scale, tuning.skip_bias)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
            nm_log!(
                "[morpho] evolve {} - Axons: {} (sprouted {}/{}), Dendrites: {} (sprouted {}/{}, too_near {}, low_e {}), Synapses: {} (checks {}, candidates {}, incompatible {}, too_far {}, successes {}, rejected_cap {}, rejected_close {}, self_skips {}, exist_skips {}, post_cap_skips {}, probe_checks {}, skipped_low_e {}, cap_hits {}, tip_e avg {:.3} min {:.3} max {:.3}, tune_ema {:.3} tune_dev {:.3} cap_scale {:.2} skip_bias {:.2}, pair_cap {})",
                unsafe { CALL_COUNT },
                total_axons,
                stats.axon_sprout_successes,
                stats.axon_sprout_attempts,
                total_dendrites,
                stats.dendrite_sprout_successes,
                stats.dendrite_sprout_attempts,
                stats.dendrite_sprout_too_near,
                stats.dendrite_sprout_low_energy,
                self.synapses.len(),
                stats.contact_checks,
                stats.contact_candidates,
                stats.contact_incompatible,
                stats.contact_too_far,
                stats.contact_successes,
                stats.contact_rejected_cap,
                stats.contact_rejected_close,
                stats.contact_self_skips,
                stats.contact_existing_skips,
                stats.contact_post_cap_skips,
                stats.contact_probe_checks,
                stats.contact_skipped_low_energy,
                stats.contact_tip_cap_hits,
                tip_avg,
                stats.contact_tip_energy_min,
                stats.contact_tip_energy_max,
                t_ema,
                t_dev,
                t_cap,
                t_skip,
                pair_cap
            );

            if is_trace {
                let avg_stimuli = if self.synapses.is_empty() {
                    0.0
                } else {
                    self.synapses.iter().map(|s| s.stimuli).sum::<f32>()
                        / self.synapses.len() as f32
                };
                nm_log!(
                    "[trace] summary: synapses={}, avg_stimuli={:.4}, broken_this_step={}",
                    self.synapses.len(),
                    avg_stimuli,
                    res.broken_connections.len()
                );
            }
        }

        // Apply planned migrations
        for mig in &res.migrations {
            let si = mig.syn_idx;
            let old_pre_l = self.synapses[si].pre_layer;
            let old_pre_id = self.synapses[si].pre_id;
            let old_post_l = self.synapses[si].post_layer;
            let old_post_id = self.synapses[si].post_id;
            let old_asi = self.synapses[si].axon_seg_idx;
            let old_dsi = self.synapses[si].dend_seg_idx;

            // 1. Clear old indices
            if let Some(oasi) = old_asi {
                if old_pre_l == -1 {
                    if let Some(ax) = self.sensory_axons.get_mut(old_pre_id) {
                        if let Some(seg) = ax.segments.get_mut(oasi) {
                            seg.syn_index = None;
                        }
                    }
                } else if old_pre_l >= 0 && old_pre_l < self.axons.len() as isize {
                    if let Some(ax) = self.axons[old_pre_l as usize].get_mut(old_pre_id) {
                        if let Some(seg) = ax.segments.get_mut(oasi) {
                            seg.syn_index = None;
                        }
                    }
                }
            }
            if let Some(odsi) = old_dsi {
                if old_post_l == num_layers as isize {
                    if let Some(dend) = self.output_dendrites.get_mut(old_post_id) {
                        if let Some(seg) = dend.tree.branches.get_mut(odsi) {
                            seg.syn_index = None;
                        }
                    }
                } else if old_post_l >= 0 && old_post_l < self.dendrites.len() as isize {
                    if let Some(dend) = self.dendrites[old_post_l as usize].get_mut(old_post_id) {
                        if let Some(seg) = dend.tree.branches.get_mut(odsi) {
                            seg.syn_index = None;
                        }
                    }
                }
            }

            // 2. Update synapse in place
            let syn = &mut self.synapses[si];
            syn.pre_layer = mig.new_pre_l;
            syn.pre_id = mig.new_pre_id;
            syn.post_layer = mig.new_post_l;
            syn.post_id = mig.new_post_id;
            syn.pre_site = mig.new_pre_site;
            syn.post_site = mig.new_post_site;
            syn.axon_seg_idx = Some(mig.new_asi);
            syn.dend_seg_idx = Some(mig.new_dsi);
            syn.stimuli = 1.0;

            // 3. Set new indices
            if syn.pre_layer == -1 {
                if let Some(ax) = self.sensory_axons.get_mut(syn.pre_id) {
                    if let Some(seg) = ax.segments.get_mut(mig.new_asi) {
                        seg.syn_index = Some(si);
                    }
                }
            } else if syn.pre_layer >= 0 && syn.pre_layer < self.axons.len() as isize {
                if let Some(ax) = self.axons[syn.pre_layer as usize].get_mut(syn.pre_id) {
                    if let Some(seg) = ax.segments.get_mut(mig.new_asi) {
                        seg.syn_index = Some(si);
                    }
                }
            }

            if syn.post_layer == num_layers as isize {
                if let Some(dend) = self.output_dendrites.get_mut(syn.post_id) {
                    if let Some(seg) = dend.tree.branches.get_mut(mig.new_dsi) {
                        seg.syn_index = Some(si);
                    }
                }
            } else if syn.post_layer >= 0 && syn.post_layer < self.dendrites.len() as isize {
                if let Some(dend) = self.dendrites[syn.post_layer as usize].get_mut(syn.post_id) {
                    if let Some(seg) = dend.tree.branches.get_mut(mig.new_dsi) {
                        seg.syn_index = Some(si);
                    }
                }
            }
        }

        // Keep IO links alive during growth while remaining sparse:
        // each sensory/output keeps at least one connection when feasible.
        if is_aarnn && config.growth_enabled {
            let min_required_sensory = 1usize;
            let sensory_count = self.sensory_somas.len().min(config.num_sensory_neurons);
            let output_count = self.output_somas.len().min(config.num_output_neurons);
            let hidden_in_count = self.dendrites.get(in_l).map(|l| l.len()).unwrap_or(0);
            let hidden_out_count = self.axons.get(out_l).map(|l| l.len()).unwrap_or(0);
            let min_required_output =
                morphology_output_connectivity_floor(config, hidden_out_count);
            let sensory_cap = if config.max_sensory_connections == 0 {
                usize::MAX
            } else {
                config.max_sensory_connections
            };
            let output_cap_base = if config.max_output_connections == 0 {
                usize::MAX
            } else {
                config.max_output_connections
            };
            let output_cap = if output_cap_base == usize::MAX {
                usize::MAX
            } else {
                output_cap_base.max(min_required_output)
            };

            let mut sensory_conn_counts = vec![0usize; sensory_count];
            let mut sensory_targets = vec![HashSet::<usize>::new(); sensory_count];
            let mut output_conn_counts = vec![0usize; output_count];
            let mut output_sources = vec![HashSet::<usize>::new(); output_count];

            for syn in &self.synapses {
                if syn.pre_layer == -1
                    && syn.post_layer == in_l as isize
                    && syn.pre_id < sensory_count
                {
                    sensory_conn_counts[syn.pre_id] += 1;
                    sensory_targets[syn.pre_id].insert(syn.post_id);
                }
                if syn.post_layer == num_layers as isize
                    && syn.pre_layer == out_l as isize
                    && syn.post_id < output_count
                {
                    output_conn_counts[syn.post_id] += 1;
                    output_sources[syn.post_id].insert(syn.pre_id);
                }
            }

            for sens_id in 0..sensory_count {
                let current = sensory_conn_counts[sens_id];
                if hidden_in_count == 0 || current >= min_required_sensory || current >= sensory_cap
                {
                    continue;
                }
                let max_add = sensory_cap.saturating_sub(current);
                let needed = min_required_sensory.saturating_sub(current).min(max_add);
                if needed == 0 {
                    continue;
                }

                let sens_anchor = self
                    .sensory_somas
                    .get(sens_id)
                    .map(|s| s.pos)
                    .unwrap_or_default();
                let mut candidates: Vec<(usize, Point3, Point3, f32)> = Vec::new();
                for j in 0..hidden_in_count {
                    if sensory_targets[sens_id].contains(&j) {
                        continue;
                    }
                    let post_site = if in_l < self.dendrites.len() && j < self.dendrites[in_l].len()
                    {
                        let dend = &self.dendrites[in_l][j];
                        let mut best = self
                            .somas
                            .get(in_l)
                            .and_then(|l| l.get(j))
                            .map(|s| s.pos)
                            .unwrap_or_default();
                        let mut best_d2 = sens_anchor.dist_sq(best);
                        for seg in &dend.tree.branches {
                            let d2 = sens_anchor.dist_sq(seg.from);
                            if d2 < best_d2 {
                                best_d2 = d2;
                                best = seg.from;
                            }
                        }
                        best
                    } else {
                        self.somas
                            .get(in_l)
                            .and_then(|l| l.get(j))
                            .map(|s| s.pos)
                            .unwrap_or_default()
                    };
                    let pre_site = if sens_id < self.sensory_axons.len() {
                        let ax = &self.sensory_axons[sens_id];
                        let mut best = sens_anchor;
                        let mut best_d2 = post_site.dist_sq(best);
                        for seg in &ax.segments {
                            let d2 = post_site.dist_sq(seg.to);
                            if d2 < best_d2 {
                                best_d2 = d2;
                                best = seg.to;
                            }
                        }
                        best
                    } else {
                        sens_anchor
                    };
                    let d2 = pre_site.dist_sq(post_site);
                    candidates.push((j, pre_site, post_site, d2));
                }
                candidates
                    .sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));

                for (target_j, pre_site, post_site, _d2) in candidates.into_iter().take(needed) {
                    let w =
                        (config.initial_synaptic_weight * (0.8 + 0.4 * fastrand::f64())).max(0.01);
                    self.synapses.push(Synapse {
                        kind: SynKind::In,
                        pre_layer: -1,
                        pre_id: sens_id,
                        post_layer: in_l as isize,
                        post_id: target_j,
                        pre_site,
                        post_site,
                        axon_seg_idx: None,
                        dend_seg_idx: None,
                        bend: None,
                        weight: w,
                        p_release: 1.0,
                        delay_ms: 1.0,
                        stimuli: 1.0,
                    });
                    res.new_connections
                        .push((-1, sens_id, in_l as isize, target_j, w));
                    sensory_conn_counts[sens_id] += 1;
                    sensory_targets[sens_id].insert(target_j);
                }
            }

            for out_id in 0..output_count {
                let current = output_conn_counts[out_id];
                if hidden_out_count == 0 || current >= min_required_output || current >= output_cap
                {
                    continue;
                }
                let max_add = output_cap.saturating_sub(current);
                let needed = min_required_output.saturating_sub(current).min(max_add);
                if needed == 0 {
                    continue;
                }

                let out_anchor = self
                    .output_somas
                    .get(out_id)
                    .map(|s| s.pos)
                    .unwrap_or_default();
                let mut candidates: Vec<(usize, Point3, Point3, f32)> = Vec::new();
                for pre_j in 0..hidden_out_count {
                    if output_sources[out_id].contains(&pre_j) {
                        continue;
                    }
                    let pre_site = if out_l < self.axons.len() && pre_j < self.axons[out_l].len() {
                        let ax = &self.axons[out_l][pre_j];
                        let mut best = self
                            .somas
                            .get(out_l)
                            .and_then(|l| l.get(pre_j))
                            .map(|s| s.pos)
                            .unwrap_or_default();
                        let mut best_d2 = out_anchor.dist_sq(best);
                        for seg in &ax.segments {
                            let d2 = out_anchor.dist_sq(seg.to);
                            if d2 < best_d2 {
                                best_d2 = d2;
                                best = seg.to;
                            }
                        }
                        best
                    } else {
                        self.somas
                            .get(out_l)
                            .and_then(|l| l.get(pre_j))
                            .map(|s| s.pos)
                            .unwrap_or_default()
                    };
                    let post_site = if out_id < self.output_dendrites.len() {
                        let dend = &self.output_dendrites[out_id];
                        let mut best = out_anchor;
                        let mut best_d2 = pre_site.dist_sq(best);
                        for seg in &dend.tree.branches {
                            let d2 = pre_site.dist_sq(seg.from);
                            if d2 < best_d2 {
                                best_d2 = d2;
                                best = seg.from;
                            }
                        }
                        best
                    } else {
                        out_anchor
                    };
                    let d2 = pre_site.dist_sq(post_site);
                    candidates.push((pre_j, pre_site, post_site, d2));
                }
                candidates
                    .sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));

                for (pre_j, pre_site, post_site, _d2) in candidates.into_iter().take(needed) {
                    let w =
                        (config.initial_synaptic_weight * (0.8 + 0.4 * fastrand::f64())).max(0.01);
                    self.synapses.push(Synapse {
                        kind: SynKind::Out,
                        pre_layer: out_l as isize,
                        pre_id: pre_j,
                        post_layer: num_layers as isize,
                        post_id: out_id,
                        pre_site,
                        post_site,
                        axon_seg_idx: None,
                        dend_seg_idx: None,
                        bend: None,
                        weight: w,
                        p_release: 1.0,
                        delay_ms: 1.0,
                        stimuli: 1.0,
                    });
                    res.new_connections.push((
                        out_l as isize,
                        pre_j,
                        num_layers as isize,
                        out_id,
                        w,
                    ));
                    output_conn_counts[out_id] += 1;
                    output_sources[out_id].insert(pre_j);
                }
            }
        }

        // Apply spatial forces at the end so contact/migration are based on pre-move geometry
        self.apply_spatial_forces(config, is_aarnn, dt);
        self.update_skull_membrane(config, dt);

        res
    }
}

#[derive(Debug, Default)]
pub struct EvolutionResult {
    pub new_connections: Vec<(isize, usize, isize, usize, f64)>, // pre_l, pre_id, post_l, post_id, weight
    pub broken_connections: Vec<(isize, usize, isize, usize)>,
    pub migrations: Vec<MigrationInfo>,
}

#[derive(Debug)]
pub struct MigrationInfo {
    pub syn_idx: usize,
    pub new_pre_l: isize,
    pub new_pre_id: usize,
    pub new_post_l: isize,
    pub new_post_id: usize,
    pub new_dsi: usize,
    pub new_asi: usize,
    pub new_pre_site: Point3,
    pub new_post_site: Point3,
}

#[cfg(all(test, feature = "growth3d", feature = "morpho"))]
mod tests {
    use super::*;
    #[test]
    fn test_point3_ops() {
        let p1 = Point3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let p2 = Point3 {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        };

        let sum = p1.add(p2);
        assert_eq!(sum.x, 5.0);
        assert_eq!(sum.y, 7.0);
        assert_eq!(sum.z, 9.0);

        let dist = p1.dist(p2);
        let expected = ((3.0f32.powi(2) + 3.0f32.powi(2) + 3.0f32.powi(2)) as f32).sqrt();
        assert!((dist - expected).abs() < 1e-6);
    }

    #[test]
    fn test_spatial_grid() {
        let mut grid = SpatialGrid {
            entities: Vec::new(),
            cell_starts: vec![0; 1001],
            dim: 10,
            cell_size: 1.0,
        };
        let p = Point3 {
            x: 1.5,
            y: 2.5,
            z: 3.5,
        };
        let key = grid.get_key(p).unwrap();

        grid.entities.push(GridEntity {
            pos: p,
            stimuli: 1.0,
        });
        for i in (key + 1)..=1000 {
            grid.cell_starts[i] = 1;
        }

        let retrieved = grid.cell_entities(key);
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].pos.x, 1.5);
    }

    #[test]
    fn test_octree_energy_at_matches_bruteforce() {
        let entities = vec![
            GridEntity {
                pos: Point3 {
                    x: -0.5,
                    y: 0.2,
                    z: 0.1,
                },
                stimuli: 0.8,
            },
            GridEntity {
                pos: Point3 {
                    x: 0.3,
                    y: -0.1,
                    z: -0.2,
                },
                stimuli: 1.2,
            },
            GridEntity {
                pos: Point3 {
                    x: 0.9,
                    y: 0.9,
                    z: 0.9,
                },
                stimuli: 0.5,
            },
        ];
        let oct = OctreeIndex::build(entities.clone(), 0.2);
        let p = Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let radius = 0.8;
        let k = 0.4;
        let r2 = radius * radius;

        let brute: f32 = entities
            .iter()
            .map(|e| {
                let d2 = p.dist_sq(e.pos);
                if d2 < r2 {
                    e.stimuli / (1.0 + k * d2)
                } else {
                    0.0
                }
            })
            .sum();

        let octree_val = oct.energy_at(p, radius, k);
        assert!((octree_val - brute).abs() < 1e-5);
    }

    #[test]
    fn unique_points_and_min_dist() {
        // Build tiny topology with 2 layers, 2 neurons each
        use crate::topology::Node3D;
        let topo: Vec<Vec<Node3D>> = vec![
            vec![
                Node3D {
                    x: -0.2,
                    y: 0.0,
                    z: 0.1,
                    layer: 0,
                    ..Default::default()
                },
                Node3D {
                    x: 0.0,
                    y: 0.2,
                    z: -0.1,
                    layer: 0,
                    ..Default::default()
                },
            ],
            vec![
                Node3D {
                    x: 0.5,
                    y: 0.1,
                    z: 0.2,
                    layer: 1,
                    ..Default::default()
                },
                Node3D {
                    x: 0.6,
                    y: -0.1,
                    z: -0.2,
                    layer: 1,
                    ..Default::default()
                },
            ],
        ];
        let w_in = ndarray::Array2::<f64>::from_elem((2, 2), 0.2);
        let w_hh_fwd = vec![ndarray::Array2::<f64>::from_elem((2, 2), 0.3)];
        let w_hh_bwd = vec![ndarray::Array2::<f64>::from_elem((2, 2), 0.1)];
        let w_out = ndarray::Array2::<f64>::from_elem((2, 2), 0.4);
        let mut config = crate::config::NetworkConfig::default();
        config.num_sensory_neurons = 2;
        config.num_output_neurons = 2;
        config.seg_eps = 0.0015;
        config.max_reroute_tries = 12;
        config.enforce_unique_geometry = true;
        config.relax_iters = 8;
        config.relax_step = 0.005;
        config.use_mid_bends = true;
        config.synapse_offset = 0.05;
        config.aarnn_velocity = 10.0;

        let m = Morphology::from_weights(
            &topo,
            &Vec::new(),
            &Vec::new(),
            &w_in,
            &w_hh_fwd,
            &w_hh_bwd,
            &w_out,
            &config,
            false,
        );
        // Ensure points are unique (quantized)
        let mut set = HashSet::new();
        let q = |p: &Point3| -> (i32, i32, i32) {
            (
                ((p.x * 1000.0).round() as i32),
                ((p.y * 1000.0).round() as i32),
                ((p.z * 1000.0).round() as i32),
            )
        };
        for s in &m.synapses {
            assert!(set.insert(q(&s.pre_site)));
            assert!(set.insert(q(&s.post_site)));
        }
        // Ensure no identical segments
        for i in 0..m.synapses.len() {
            for j in 0..i {
                let s1 = &m.synapses[i];
                let s2 = &m.synapses[j];
                // Skip if they share a pre or post neuron, or are reciprocal
                if (s1.pre_layer == s2.pre_layer && s1.pre_id == s2.pre_id)
                    || (s1.post_layer == s2.post_layer && s1.post_id == s2.post_id)
                    || (s1.pre_layer == s2.post_layer
                        && s1.pre_id == s2.post_id
                        && s1.post_layer == s2.pre_layer
                        && s1.post_id == s2.pre_id)
                {
                    continue;
                }
                let d = seg_seg_min_dist_sq(s1.pre_site, s1.post_site, s2.pre_site, s2.post_site);
                assert!(
                    d >= (config.seg_eps * 0.5),
                    "Segments {} and {} are too close: {}",
                    i,
                    j,
                    d
                );
            }
        }
    }

    #[test]
    fn dendrite_compartments_reflect_cell_structure_types() {
        use crate::topology::Node3D;

        let topo: Vec<Vec<Node3D>> = vec![
            vec![Node3D {
                x: -0.2,
                y: 0.0,
                z: 0.0,
                layer: 0,
                type_name: Some("L5_Pyramidal".to_string()),
                ..Default::default()
            }],
            vec![Node3D {
                x: 0.2,
                y: 0.0,
                z: 0.0,
                layer: 1,
                type_name: Some("Interneuron".to_string()),
                ..Default::default()
            }],
        ];

        let w_in = ndarray::Array2::<f64>::from_elem((1, 3), 0.25);
        let w_hh_fwd = vec![ndarray::Array2::<f64>::from_elem((1, 1), 0.35)];
        let w_hh_bwd = vec![ndarray::Array2::<f64>::zeros((1, 1))];
        let w_out = ndarray::Array2::<f64>::from_elem((1, 1), 0.20);

        let mut config = crate::config::NetworkConfig::default();
        config.num_sensory_neurons = 3;
        config.num_output_neurons = 1;
        config.synapse_offset = 0.05;
        config.enforce_unique_geometry = false;

        let m = Morphology::from_weights(
            &topo,
            &Vec::new(),
            &Vec::new(),
            &w_in,
            &w_hh_fwd,
            &w_hh_bwd,
            &w_out,
            &config,
            false,
        );

        let pyr = &m.dendrites[0][0];
        let pyr_trunks: Vec<_> = pyr.tree.branches.iter().filter(|s| s.is_trunk).collect();
        assert!(
            pyr_trunks
                .iter()
                .any(|s| s.dendrite_type == DendriteType::Apical),
            "pyramidal neuron should include apical trunk"
        );
        assert!(
            pyr_trunks
                .iter()
                .any(|s| s.dendrite_type == DendriteType::Basal),
            "pyramidal neuron should include basal trunk"
        );

        let intn = &m.dendrites[1][0];
        let intn_trunks: Vec<_> = intn.tree.branches.iter().filter(|s| s.is_trunk).collect();
        assert!(
            intn_trunks
                .iter()
                .all(|s| s.dendrite_type != DendriteType::Apical),
            "interneuron should not force apical trunks"
        );
        assert!(
            intn_trunks.iter().all(|s| s.trunk_len_from_soma > 0.0),
            "initial trunk length from soma should be tracked"
        );
    }

    #[test]
    fn spatial_forces_limit_single_step_soma_motion() {
        use crate::config::BrainRegionConfig;
        use crate::topology::Node3D;

        let topo: Vec<Vec<Node3D>> = vec![vec![Node3D {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            layer: 0,
            region_name: Some("HugeRegion".to_string()),
            ..Default::default()
        }]];
        let w_in = ndarray::Array2::<f64>::zeros((1, 0));
        let w_hh_fwd: Vec<ndarray::Array2<f64>> = Vec::new();
        let w_hh_bwd: Vec<ndarray::Array2<f64>> = Vec::new();
        let w_out = ndarray::Array2::<f64>::zeros((0, 1));

        let mut config = crate::config::NetworkConfig::default();
        config.num_sensory_neurons = 0;
        config.num_output_neurons = 0;
        config.min_node_sep = 0.02;
        config.spatial_repulsion_strength = 0.0;
        config.spatial_clumping_strength = 1.0;
        config.columnar_enabled = false;
        config.brain_regions.push(BrainRegionConfig {
            name: "HugeRegion".to_string(),
            shape: None,
            center: [35.0, 0.0, 25.0],
            radii: [35.0, 55.0, 30.0],
            type_distribution: Vec::new(),
        });

        let mut m = Morphology::from_weights(
            &topo,
            &Vec::new(),
            &Vec::new(),
            &w_in,
            &w_hh_fwd,
            &w_hh_bwd,
            &w_out,
            &config,
            false,
        );
        let p0 = m.somas[0][0].pos;
        m.apply_spatial_forces(&config, true, 1.0);
        let p1 = m.somas[0][0].pos;

        let moved = p0.dist(p1);
        let max_move = (config.min_node_sep * 0.25).clamp(0.001, 0.01) + 1e-6;
        assert!(
            moved <= max_move,
            "Soma moved too far in one step: moved={moved:.6}, limit={max_move:.6}"
        );
    }
}
