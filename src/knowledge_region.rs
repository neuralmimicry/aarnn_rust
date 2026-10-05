//! Knowledge regions: transformer feed-forward layers executed as AARNN
//! neuron meshes (Evelyn stage 3b).
//!
//! Evelyn converts a layer `down(act(gate x) * (up x))` into a mesh
//! description ([`FfnMesh`]). This module instantiates the mesh with AARNN's
//! own neuron kernels and membrane noise, then runs it:
//!
//! 1. **Dendritic summation.** Input synapses (gate and up weight matrices)
//!    sum the presynaptic activity `x` into one gate current `z_i` and one up
//!    current `u_i` per hidden channel `i`.
//! 2. **Population coding.** Each channel drives two populations of real
//!    AARNN neurons. Every neuron receives `rheobase + gain * relu(dir *
//!    (input - knot))` plus noise, and is simulated for `steps` steps. Its
//!    firing rate, normalised and weighted by its output synapse, is decoded.
//!    The gate population encodes `act(z_i)`; the up population encodes the
//!    identity on `u_i`'s range.
//! 3. **Active dendritic multiplication.** The two decoded signals meet in an
//!    active dendrite that multiplies them (the `aarnn::dynamics` active
//!    dendritic shaping seam), giving `h_i`.
//! 4. **Readout.** The down synapses sum `h` into the layer output.
//!
//! Hidden channels are independent, so they are split into contiguous shards
//! and run in parallel. Each shard is the unit of placement when regions are
//! distributed across estate nodes. Noise streams are seeded per neuron, so a
//! run is reproducible on any node and in any shard layout.

use crate::knowledge::{steady_rate, KnowledgeNeuron};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

/// One population neuron's wiring: threshold `knot`, direction (+1 rising,
/// -1 falling), input gain, and output synaptic weight (sign = E/I).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct UnitIr {
    pub knot: f32,
    pub direction: f32,
    pub gain: f64,
    pub weight: f32,
}

/// A fitted population code (shared by every channel of a layer).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PopulationIr {
    pub tonic: f32,
    pub units: Vec<UnitIr>,
    /// Rheobase of the neuron model the code was fitted to.
    pub rheobase: f64,
    /// Peak firing rate used to normalise responses (spikes/step).
    pub max_rate: f64,
}

/// A dense synapse matrix stored as little-endian f32 in a side file,
/// row-major `[outputs][inputs]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DenseIr {
    pub inputs: usize,
    pub outputs: usize,
    pub weights_file: PathBuf,
}

/// A complete feed-forward knowledge region description.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FfnMesh {
    pub name: String,
    pub neuron: KnowledgeNeuron,
    pub noise_std: f64,
    pub steps: usize,
    pub warmup: usize,
    pub gate: DenseIr,
    pub up: DenseIr,
    pub down: DenseIr,
    pub gate_code: PopulationIr,
    pub up_code: PopulationIr,
    /// Optional per-channel synaptic scaling for the up path (f32 file, one
    /// value per hidden channel). Channel `i`'s up current is divided by
    /// `scale_i` before encoding and the decoded value multiplied back, so one
    /// shared identity code serves channels of very different magnitude.
    #[serde(default)]
    pub up_scale_file: Option<PathBuf>,
}

/// Read a little-endian f32 file of exactly `n` values.
pub fn read_f32_file(path: &Path, n: usize) -> io::Result<Vec<f32>> {
    let mut bytes = Vec::with_capacity(n * 4);
    BufReader::new(File::open(path)?).read_to_end(&mut bytes)?;
    if bytes.len() != n * 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: expected {} values, found {}", path.display(), n, bytes.len() / 4),
        ));
    }
    Ok(bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

struct Dense {
    inputs: usize,
    outputs: usize,
    w: Vec<f32>,
}

impl Dense {
    fn load(ir: &DenseIr, base: &Path) -> io::Result<Self> {
        let path = if ir.weights_file.is_absolute() { ir.weights_file.clone() } else { base.join(&ir.weights_file) };
        Ok(Self { inputs: ir.inputs, outputs: ir.outputs, w: read_f32_file(&path, ir.inputs * ir.outputs)? })
    }

    fn forward(&self, x: &[f32]) -> Vec<f32> {
        (0..self.outputs)
            .map(|o| self.w[o * self.inputs..(o + 1) * self.inputs].iter().zip(x).map(|(w, v)| w * v).sum())
            .collect()
    }
}

/// An instantiated, runnable knowledge region.
pub struct FfnRegion {
    mesh: FfnMesh,
    gate: Dense,
    up: Dense,
    down: Dense,
    up_scale: Option<Vec<f32>>,
}

/// Statistics of one region evaluation.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct RegionStats {
    /// AARNN neurons simulated.
    pub neurons: u64,
    /// Total neuron-steps simulated (compute cost).
    pub neuron_steps: u64,
    pub shards: usize,
}

impl FfnRegion {
    /// Load a mesh description (JSON) and its weight files, validating shapes.
    pub fn load(mesh_json: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(mesh_json)?;
        let mesh: FfnMesh = serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let base = mesh_json.parent().unwrap_or(Path::new("."));
        let (gate, up, down) = (Dense::load(&mesh.gate, base)?, Dense::load(&mesh.up, base)?, Dense::load(&mesh.down, base)?);
        if gate.outputs != up.outputs || down.inputs != gate.outputs || gate.inputs != up.inputs {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "inconsistent mesh shapes"));
        }
        let up_scale = match &mesh.up_scale_file {
            Some(f) => {
                let path = if f.is_absolute() { f.clone() } else { base.join(f) };
                let v = read_f32_file(&path, gate.outputs)?;
                if v.iter().any(|s| !s.is_finite() || *s <= 0.0) {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "up scales must be finite and positive"));
                }
                Some(v)
            }
            None => None,
        };
        Ok(Self { mesh, gate, up, down, up_scale })
    }

    pub fn inputs(&self) -> usize {
        self.gate.inputs
    }

    pub fn outputs(&self) -> usize {
        self.down.outputs
    }

    /// Decode one population for one channel: simulate every unit neuron and
    /// sum its normalised rate through its output synapse.
    fn decode(&self, code: &PopulationIr, input: f32, seed_base: u64) -> (f32, u64) {
        let m = &self.mesh;
        let mut value = code.tonic;
        let mut neurons = 0u64;
        for (k, u) in code.units.iter().enumerate() {
            let drive = (u.direction * (input - u.knot)).max(0.0) as f64;
            if drive <= 0.0 && m.noise_std == 0.0 {
                continue; // a silent, noiseless neuron contributes nothing
            }
            // Below threshold the unit still receives noise but no drive; its
            // contribution is the fitted (zero-drive) response, so skipping
            // inactive units is exact for the population code.
            if drive <= 0.0 {
                continue;
            }
            let current = code.rheobase + u.gain * drive;
            let rate = steady_rate(&m.neuron, current, m.steps, m.warmup, m.noise_std, seed_base ^ (k as u64 + 1));
            value += u.weight * (rate / code.max_rate.max(1e-12)) as f32;
            neurons += 1;
        }
        (value, neurons)
    }

    /// Evaluate the region on one input vector, split into `shards` parallel
    /// shards of hidden channels (0 = one per available core).
    pub fn run(&self, x: &[f32], shards: usize) -> io::Result<(Vec<f32>, RegionStats)> {
        if x.len() != self.inputs() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "input width mismatch"));
        }
        let z = self.gate.forward(x);
        let u = self.up.forward(x);
        let hidden = z.len();
        let shards = if shards == 0 { std::thread::available_parallelism().map_or(1, |n| n.get()) } else { shards }.clamp(1, hidden);
        let chunk = hidden.div_ceil(shards);
        let parts: Vec<(Vec<f32>, u64)> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..shards)
                .map(|sh| {
                    let (z, u) = (&z, &u);
                    s.spawn(move || {
                        let (start, end) = (sh * chunk, ((sh + 1) * chunk).min(hidden));
                        let mut h = Vec::with_capacity(end.saturating_sub(start));
                        let mut neurons = 0u64;
                        for i in start..end {
                            let seed = 0xA5A5_0000_0000_0000 ^ ((i as u64) << 20);
                            let (g, ng) = self.decode(&self.mesh.gate_code, z[i], seed);
                            let scale = self.up_scale.as_ref().map_or(1.0, |s| s[i]);
                            let (v, nu) = self.decode(&self.mesh.up_code, u[i] / scale, seed ^ 0x5A5A);
                            let v = v * scale; // synaptic scaling restores magnitude
                            h.push(g * v); // active dendritic multiplication
                            neurons += ng + nu;
                        }
                        (h, neurons)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("region shard panicked")).collect()
        });
        let neurons: u64 = parts.iter().map(|p| p.1).sum();
        let h: Vec<f32> = parts.into_iter().flat_map(|p| p.0).collect();
        let stats = RegionStats { neurons, neuron_steps: neurons * (self.mesh.steps + self.mesh.warmup) as u64, shards };
        Ok((self.down.forward(&h), stats))
    }
}
