// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Scientific topology: stable neuron/synapse identities, classes, signs,
//! weights and delays, with their provenance (brief Section 6).
//!
//! Neuron order (generation-bound dense index used by traces): sensory
//! channels, then hidden cells, then outputs. Stable identities are the
//! source's names (`connectome_labels`), carried as strings so large dataset
//! IDs never pass through a float.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ExplorerError, Result, invalid};
use crate::gnat::{Adjacency, Norm};

pub const TOPOLOGY_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Sensory,
    Hidden,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SynClass {
    Chemical,
    Electrical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neuron {
    pub id: String,
    pub role: Role,
    /// Index within its role (row/column in the native matrices).
    pub dense: u32,
    pub type_name: String,
    pub region: String,
    pub graded: bool,
    pub position: Option<[f64; 3]>,
    /// `procedural` (designed layout), `measured`, `inferred` or `unknown`.
    pub position_kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Synapse {
    pub id: String,
    /// Global neuron indices.
    pub pre: u32,
    pub post: u32,
    pub class: SynClass,
    pub weight: f64,
    pub delay_ticks: u32,
}

/// What the source supports (brief Section 6: never silently equated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Exact continuation of a running state (not available for the legacy
    /// runner; D-002).
    ExactResume,
    /// Executable from a declared initial state.
    DeclaredColdStart,
    /// Anatomy only: no dynamics are declared.
    StructuralOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topology {
    pub schema_version: u32,
    pub source_format: String,
    pub generation: u64,
    pub dt_ms: f64,
    pub neurons: Vec<Neuron>,
    pub synapses: Vec<Synapse>,
    pub capability: Capability,
    /// Versioned modelling assumptions (delay rule, sign rule, ...).
    pub assumptions: Vec<String>,
    /// Model facts that matter for interpretation (plasticity, growth,
    /// neuromodulation switches as declared by the source).
    pub model_flags: BTreeMap<String, Value>,
    pub limitations: Vec<String>,
}

impl Topology {
    pub fn count(&self, role: Role) -> usize {
        self.neurons.iter().filter(|n| n.role == role).count()
    }

    pub fn index_of(&self, id: &str) -> Option<u32> {
        self.neurons
            .iter()
            .position(|n| n.id == id)
            .map(|i| i as u32)
    }

    /// Global index of (role, dense).
    pub fn global(&self, role: Role, dense: u32) -> u32 {
        let s = self.count(Role::Sensory) as u32;
        let h = self.count(Role::Hidden) as u32;
        match role {
            Role::Sensory => dense,
            Role::Hidden => s + dense,
            Role::Output => s + h + dense,
        }
    }

    pub fn adjacency(&self, norm: Norm) -> Adjacency {
        let chem: Vec<(u32, u32, f64, u32)> = self
            .synapses
            .iter()
            .filter(|s| s.class == SynClass::Chemical)
            .map(|s| (s.pre, s.post, s.weight, s.delay_ticks))
            .collect();
        Adjacency::build(self.neurons.len(), &chem, norm)
    }

    pub fn summary(&self) -> Value {
        let chem = self
            .synapses
            .iter()
            .filter(|s| s.class == SynClass::Chemical);
        serde_json::json!({
            "source_format": self.source_format,
            "capability": self.capability,
            "sensory": self.count(Role::Sensory),
            "hidden": self.count(Role::Hidden),
            "outputs": self.count(Role::Output),
            "graded_cells": self.neurons.iter().filter(|n| n.graded).count(),
            "chemical_synapses": chem.clone().count(),
            "excitatory": chem.clone().filter(|s| s.weight > 0.0).count(),
            "inhibitory": chem.filter(|s| s.weight < 0.0).count(),
            "electrical": self.synapses.iter().filter(|s| s.class == SynClass::Electrical).count(),
            "assumptions": self.assumptions,
            "limitations": self.limitations,
        })
    }
}

pub fn mat(v: &Value, key: &str) -> Result<(usize, usize, Vec<f64>)> {
    let m = &v[key];
    let m = if m.is_array() { &m[0] } else { m };
    if m.is_null() {
        return Ok((0, 0, Vec::new()));
    }
    let rows = m["rows"]
        .as_u64()
        .ok_or_else(|| ExplorerError::InvalidInput(format!("{key}.rows missing")))?
        as usize;
    let cols = m["cols"]
        .as_u64()
        .ok_or_else(|| ExplorerError::InvalidInput(format!("{key}.cols missing")))?
        as usize;
    let data: Vec<f64> = m["data"]
        .as_array()
        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
        .unwrap_or_default();
    if !data.is_empty() && data.len() != rows * cols {
        return invalid(format!(
            "{key}: {} values for {rows}x{cols} (truncated or corrupt)",
            data.len()
        ));
    }
    Ok((rows, cols, data))
}

fn names(labels: &Value, key: &str, n: usize, prefix: &str) -> Vec<String> {
    let given: Vec<String> = labels[key]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out = Vec::with_capacity(n);
    let mut seen: HashMap<String, u32> = HashMap::new();
    for i in 0..n {
        let base = given
            .get(i)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{prefix}:{i}"));
        let k = seen.entry(base.clone()).or_insert(0);
        out.push(if *k == 0 {
            base.clone()
        } else {
            format!("{base}#{k}")
        });
        *k += 1;
    }
    out
}

/// Bio parameters of a type name (graded flag and so on).
fn type_bio<'a>(net: &'a Value, type_name: &str) -> Option<&'a Value> {
    net["neuron_types"]
        .as_array()?
        .iter()
        .find(|t| t["name"].as_str() == Some(type_name))
        .map(|t| &t["bio_params"])
}

/// Import a native AARNN snapshot (designed circuit or imported connectome).
pub fn from_native(raw: &str) -> Result<(Topology, Value)> {
    let v: Value = serde_json::from_str(raw)?;
    let net = &v["net"];
    if !net.is_object() || !v["topo"].is_object() {
        return invalid("not an AARNN network snapshot: missing `net` or `topo`");
    }
    let layers = v["topo"]["layers"].as_array().map(|a| a.len()).unwrap_or(0);
    if layers != 1 || v["w_hh_fwd"].as_array().is_some_and(|a| !a.is_empty()) {
        return Err(ExplorerError::Unsupported(format!(
            "{layers} hidden layers: the explorer lowers single-layer recurrent networks (designed circuits, imported connectomes); multi-layer snapshots are structural-only in this version"
        )));
    }
    let (h_in, s, w_in) = mat(&v, "w_in")?;
    let (h_r, h_c, w_rec) = mat(&v, "w_hh_rec")?;
    let (o, h_out, w_out) = mat(&v, "w_out")?;
    let (gr, _, w_gap) = mat(&v, "w_gap")?;
    let cells = v["topo"]["layers"][0]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let h = cells.len();
    if h_in != h || h_r != h || h_c != h || h_out != h || (gr != 0 && gr != h) {
        return invalid(format!(
            "inconsistent hidden size: topo {h}, w_in rows {h_in}, w_hh_rec {h_r}x{h_c}, w_out cols {h_out}, w_gap {gr}"
        ));
    }
    let labels = &v["connectome_labels"];
    let sn = names(labels, "sensory_nodes", s, "sensory");
    let hn = names(labels, "hidden_nodes", h, "hidden");
    let on = names(labels, "output_nodes", o, "output");
    let mut neurons = Vec::with_capacity(s + h + o);
    let sens_nodes = v["topo"]["sensory_nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let out_nodes = v["topo"]["output_nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let base_graded = net["aarnn_bio"]["graded_enabled"]
        .as_bool()
        .unwrap_or(false);
    let mut push = |role: Role, dense: usize, id: &str, node: Option<&Value>| {
        let type_name = node
            .and_then(|n| n["type_name"].as_str())
            .unwrap_or("")
            .to_string();
        let graded = type_bio(net, &type_name)
            .and_then(|b| b["graded_enabled"].as_bool())
            .unwrap_or(base_graded)
            && role != Role::Sensory;
        let pos = node.and_then(|n| Some([n["x"].as_f64()?, n["y"].as_f64()?, n["z"].as_f64()?]));
        neurons.push(Neuron {
            id: id.to_string(),
            role,
            dense: dense as u32,
            type_name,
            region: node
                .and_then(|n| n["region_name"].as_str())
                .unwrap_or("")
                .to_string(),
            graded,
            position: pos,
            position_kind: if labels["schema"].as_str() == Some("aarnn-designed-circuit-v1") {
                "procedural".into()
            } else if pos.is_some() {
                "inferred".into()
            } else {
                "unknown".into()
            },
        });
    };
    for (i, id) in sn.iter().enumerate() {
        push(Role::Sensory, i, id, sens_nodes.get(i));
    }
    for (i, id) in hn.iter().enumerate() {
        push(Role::Hidden, i, id, cells.get(i));
    }
    for (i, id) in on.iter().enumerate() {
        push(Role::Output, i, id, out_nodes.get(i));
    }
    let use_delays = net["use_aarnn_delays"].as_bool().unwrap_or(false);
    let use_morph = net["use_morphology"].as_bool().unwrap_or(false);
    let mut limitations = Vec::new();
    if use_delays {
        limitations.push(
            "use_aarnn_delays = true: per-synapse distance delays are not reproduced by the explorer's delay rule; GNAT timing for this source is approximate".to_string(),
        );
    }
    let (s32, h32) = (s as u32, h as u32);
    let mut synapses = Vec::new();
    let mut add = |pre: u32, post: u32, w: f64, d: u32, class: SynClass, ids: (&str, &str)| {
        synapses.push(Synapse {
            id: format!(
                "{}->{}{}",
                ids.0,
                ids.1,
                if class == SynClass::Electrical {
                    ":gap"
                } else {
                    ""
                }
            ),
            pre,
            post,
            class,
            weight: w,
            delay_ticks: d,
        });
    };
    // Delay rule `legacy-runner-stage-delay-v1` (runner.rs: hist_len = 1
    // without use_aarnn_delays): sensory->hidden 0 ticks (same step, earlier
    // stage), hidden->hidden 1 tick, hidden->output 0 ticks.
    for j in 0..h {
        for i in 0..s {
            let w = w_in.get(j * s + i).copied().unwrap_or(0.0);
            if w != 0.0 {
                add(
                    i as u32,
                    s32 + j as u32,
                    w,
                    0,
                    SynClass::Chemical,
                    (&sn[i], &hn[j]),
                );
            }
        }
    }
    for post in 0..h {
        for pre in 0..h {
            let w = w_rec.get(post * h + pre).copied().unwrap_or(0.0);
            if w != 0.0 {
                add(
                    s32 + pre as u32,
                    s32 + post as u32,
                    w,
                    1,
                    SynClass::Chemical,
                    (&hn[pre], &hn[post]),
                );
            }
        }
    }
    for k in 0..o {
        for j in 0..h {
            let w = w_out.get(k * h + j).copied().unwrap_or(0.0);
            if w != 0.0 {
                add(
                    s32 + j as u32,
                    s32 + h32 + k as u32,
                    w,
                    0,
                    SynClass::Chemical,
                    (&hn[j], &on[k]),
                );
            }
        }
    }
    if gr == h {
        for a in 0..h {
            for b in (a + 1)..h {
                let g = w_gap.get(a * h + b).copied().unwrap_or(0.0);
                if g != 0.0 {
                    add(
                        s32 + a as u32,
                        s32 + b as u32,
                        g,
                        0,
                        SynClass::Electrical,
                        (&hn[a], &hn[b]),
                    );
                }
            }
        }
    }
    let mut model_flags = BTreeMap::new();
    for k in [
        "aarnn_plasticity_enabled",
        "growth_enabled",
        "morpho_growth_enabled",
        "use_morphology",
        "use_aarnn_delays",
        "aarnn_neuromod_transmission_enabled",
        "aarnn_neuromod_env_enabled",
        "neuromodulation_enabled",
        "max_total_neurons",
        "aarnn_layer_depth",
    ] {
        model_flags.insert(
            k.to_string(),
            net.get(k)
                .cloned()
                .unwrap_or(Value::String("default".into())),
        );
    }
    if v["runtime_state"].is_object() {
        limitations.push(
            "runtime_state present: the legacy runner does not persist every transient (graded output phase, STP release history, neuromodulator EMAs), so continuation is declared-cold-start, not exact-resume".into(),
        );
    }
    let topo = Topology {
        schema_version: TOPOLOGY_SCHEMA,
        source_format: "aarnn-native-snapshot".into(),
        generation: 1,
        dt_ms: 1.0,
        neurons,
        synapses,
        capability: Capability::DeclaredColdStart,
        assumptions: vec![
            "delay rule legacy-runner-stage-delay-v1: sensory->hidden 0, hidden->hidden 1, hidden->output 0 ticks; same-step order by stage microstep (sensory 0, hidden 1, output 2)".into(),
            format!("use_morphology = {use_morph}, use_aarnn_delays = {use_delays} as declared by the source"),
            "weights are the source's native units (runner current scale)".into(),
        ],
        model_flags,
        limitations,
    };
    Ok((topo, v))
}

// ---------------------------------------------------------------------------
// Sparse interchange `aarnn-explorer-sparse-v1` (docs/connectome-explorer/data-formats.md)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SparseDataset {
    pub name: String,
    pub version: String,
    pub specimen: String,
    pub coverage: String,
    pub licence: String,
    #[serde(default)]
    pub source_files: Vec<SparseFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SparseFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SparseDynamics {
    /// Bio-parameter overrides per cell type (AARNN `bio_params` keys).
    pub cell_types: BTreeMap<String, BTreeMap<String, Value>>,
    /// `explicit-weight` (edges carry `weight`) or `count-sign-scale`
    /// (weight = count x sign x scale; sign must be known).
    pub weight_conversion: String,
    #[serde(default)]
    pub scale: Option<f64>,
    /// Version label of the assumption set, stored in every result.
    pub assumption_version: String,
    /// Network-level settings (NetworkConfig fields). Unless overridden, a
    /// compiled network has neuromodulator transmission and environmental
    /// drive off: an imported wiring diagram declares no modulator biology.
    #[serde(default)]
    pub net_overrides: BTreeMap<String, Value>,
}

/// Parse and validate a sparse interchange document. Returns the topology
/// (structural-only unless `dynamics` is present) and, with dynamics, an
/// executable native snapshot compiled from it.
pub fn from_sparse(
    raw: &str,
    max_neurons: usize,
    max_edges: usize,
) -> Result<(Topology, Option<Value>)> {
    let v: Value = serde_json::from_str(raw)?;
    if v["schema"].as_str() != Some("aarnn-explorer-sparse-v1") {
        return Err(ExplorerError::Unsupported(format!(
            "sparse schema {:?} (expected \"aarnn-explorer-sparse-v1\")",
            v["schema"]
        )));
    }
    let allowed = ["schema", "dataset", "units", "neurons", "edges", "dynamics"];
    if let Some(obj) = v.as_object() {
        if let Some(k) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
            return invalid(format!("sparse: unknown top-level field {k:?}"));
        }
    }
    let dataset: SparseDataset = serde_json::from_value(v["dataset"].clone())
        .map_err(|e| ExplorerError::InvalidInput(format!("sparse dataset manifest: {e}")))?;
    let units = &v["units"];
    let wu = units["weight"]
        .as_str()
        .ok_or_else(|| ExplorerError::InvalidInput("units.weight is required".into()))?;
    let du = units["delay"]
        .as_str()
        .ok_or_else(|| ExplorerError::InvalidInput("units.delay is required".into()))?;
    if du != "ticks" {
        return invalid(format!(
            "units.delay = {du:?}: only \"ticks\" (1 ms) is supported"
        ));
    }
    let nlist = v["neurons"]
        .as_array()
        .ok_or_else(|| ExplorerError::InvalidInput("neurons[] is required".into()))?;
    let elist = v["edges"]
        .as_array()
        .ok_or_else(|| ExplorerError::InvalidInput("edges[] is required".into()))?;
    // Size estimate before allocation.
    if nlist.len() > max_neurons || elist.len() > max_edges {
        return Err(ExplorerError::Budget(format!(
            "{} neurons / {} edges exceed the import budget ({max_neurons} / {max_edges})",
            nlist.len(),
            elist.len()
        )));
    }
    let id_of = |x: &Value| -> Option<String> {
        match x {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) if n.is_u64() || n.is_i64() => Some(n.to_string()),
            _ => None,
        }
    };
    let mut rows: Vec<(Role, String, &Value)> = Vec::with_capacity(nlist.len());
    let mut seen = HashMap::new();
    for (k, n) in nlist.iter().enumerate() {
        let id = id_of(&n["id"]).ok_or_else(|| {
            ExplorerError::InvalidInput(format!("neurons[{k}].id must be a string or integer"))
        })?;
        if n["id"].is_f64() {
            return invalid(format!(
                "neurons[{k}].id is a float: IDs must be integers or strings"
            ));
        }
        let role = match n["role"].as_str() {
            Some("sensory") => Role::Sensory,
            Some("hidden") => Role::Hidden,
            Some("output") => Role::Output,
            r => {
                return invalid(format!(
                    "neurons[{k}].role = {r:?}: sensory, hidden or output"
                ));
            }
        };
        if seen.insert(id.clone(), k).is_some() {
            return invalid(format!("duplicate neuron id {id}"));
        }
        rows.push((role, id, n));
    }
    rows.sort_by_key(|r| r.0);
    let mut dense_count: BTreeMap<Role, u32> = BTreeMap::new();
    let mut neurons = Vec::with_capacity(rows.len());
    let mut index: HashMap<String, u32> = HashMap::new();
    for (role, id, n) in &rows {
        let d = dense_count.entry(*role).or_insert(0);
        let pos = n["position"].as_array().and_then(|a| {
            Some([
                a.first()?.as_f64()?,
                a.get(1)?.as_f64()?,
                a.get(2)?.as_f64()?,
            ])
        });
        index.insert(id.clone(), neurons.len() as u32);
        neurons.push(Neuron {
            id: id.clone(),
            role: *role,
            dense: *d,
            type_name: n["type"].as_str().unwrap_or("").to_string(),
            region: n["region"].as_str().unwrap_or("").to_string(),
            graded: false,
            position: pos,
            position_kind: n["position_kind"]
                .as_str()
                .unwrap_or(if pos.is_some() { "measured" } else { "unknown" })
                .to_string(),
        });
        *d += 1;
    }
    let dynamics: Option<SparseDynamics> = if v["dynamics"].is_null() {
        None
    } else {
        Some(
            serde_json::from_value(v["dynamics"].clone())
                .map_err(|e| ExplorerError::InvalidInput(format!("dynamics: {e}")))?,
        )
    };
    let mut synapses = Vec::with_capacity(elist.len());
    let mut unknown_weights = 0usize;
    let mut unknown_signs = 0usize;
    for (k, e) in elist.iter().enumerate() {
        let pre = id_of(&e["pre"]).and_then(|i| index.get(&i).copied());
        let post = id_of(&e["post"]).and_then(|i| index.get(&i).copied());
        let (Some(pre), Some(post)) = (pre, post) else {
            return invalid(format!("edges[{k}]: unknown pre/post neuron"));
        };
        let class = match e["class"].as_str().unwrap_or("chemical") {
            "chemical" => SynClass::Chemical,
            "electrical" => SynClass::Electrical,
            c => return invalid(format!("edges[{k}].class = {c:?}")),
        };
        let sign = e["sign"].as_i64();
        let weight = match (
            e["weight"].as_f64(),
            dynamics.as_ref().map(|d| d.weight_conversion.as_str()),
        ) {
            (Some(w), _) => Some(w),
            (None, Some("count-sign-scale")) => match (e["count"].as_f64(), sign) {
                (Some(c), Some(sg)) if sg == 1 || sg == -1 => {
                    Some(c * sg as f64 * dynamics.as_ref().and_then(|d| d.scale).unwrap_or(1.0))
                }
                (Some(_), _) => {
                    unknown_signs += 1;
                    None
                }
                _ => None,
            },
            _ => None,
        };
        if weight.is_none() {
            unknown_weights += 1;
        }
        let delay = match e["delay_ticks"].as_u64() {
            Some(d) => d as u32,
            None => 1,
        };
        let (pr, po) = (neurons[pre as usize].role, neurons[post as usize].role);
        if pr == Role::Output || po == Role::Sensory {
            return invalid(format!(
                "edges[{k}]: {pr:?} -> {po:?} is not a feed-forward/recurrent edge"
            ));
        }
        synapses.push(Synapse {
            id: format!(
                "{}->{}",
                neurons[pre as usize].id, neurons[post as usize].id
            ),
            pre,
            post,
            class,
            weight: weight.unwrap_or(f64::NAN),
            delay_ticks: delay,
        });
    }
    let mut limitations = vec![format!(
        "dataset {} {} ({}, coverage {}), licence {}",
        dataset.name, dataset.version, dataset.specimen, dataset.coverage, dataset.licence
    )];
    if unknown_weights > 0 {
        limitations.push(format!("{unknown_weights} edges have unknown weights ({unknown_signs} with unknown sign): kept as unknown (NaN), excluded from GNAT edges"));
    }
    let executable = dynamics.is_some() && unknown_weights == 0;
    let mut topo = Topology {
        schema_version: TOPOLOGY_SCHEMA,
        source_format: "aarnn-explorer-sparse-v1".into(),
        generation: 1,
        dt_ms: 1.0,
        neurons,
        synapses,
        capability: if executable {
            Capability::DeclaredColdStart
        } else {
            Capability::StructuralOnly
        },
        assumptions: vec![
            format!("weight units: {wu}"),
            "delay units: ticks (1 ms)".into(),
        ],
        model_flags: BTreeMap::new(),
        limitations,
    };
    let snapshot = match (&dynamics, executable) {
        (Some(d), true) => {
            topo.assumptions.push(format!(
                "dynamics {}: weight conversion {} (scale {:?}); declared cold start",
                d.assumption_version, d.weight_conversion, d.scale
            ));
            Some(compile_native(&topo, d)?)
        }
        (Some(_), false) => {
            topo.limitations
                .push("dynamics declared but some weights are unknown: structural-only".into());
            None
        }
        _ => None,
    };
    if let Some(snap) = &snapshot {
        // Re-derive the topology from the compiled snapshot so the executed
        // stage delays (not the dataset's declared ones) are what analyses use.
        let (mut compiled, _) = from_native(&snap.to_string())?;
        compiled.source_format = topo.source_format.clone();
        compiled.assumptions.extend(topo.assumptions.clone());
        compiled.limitations.extend(topo.limitations.clone());
        compiled.limitations.push(
            "compiled to the legacy runner: declared per-edge delays are replaced by the stage delay rule (hidden->hidden 1 tick)".into(),
        );
        for n in compiled.neurons.iter_mut() {
            if let Some(src) = topo.neurons.iter().find(|m| m.id == n.id) {
                n.position_kind = src.position_kind.clone();
                n.position = src.position;
            }
        }
        return Ok((compiled, snapshot));
    }
    Ok((topo, None))
}

/// Compile a topology with declared dynamics into a native single-layer
/// AARNN snapshot (the designed-circuit layout of `scripts/aarnn_circuit.py`).
pub fn compile_native(t: &Topology, d: &SparseDynamics) -> Result<Value> {
    let base = serde_json::to_value(aarnn_rust::config::NetworkConfig::default())
        .map_err(|e| ExplorerError::Internal(format!("default config: {e}")))?;
    let by_role = |r: Role| {
        t.neurons
            .iter()
            .filter(move |n| n.role == r)
            .collect::<Vec<_>>()
    };
    let (sens, hid, out) = (
        by_role(Role::Sensory),
        by_role(Role::Hidden),
        by_role(Role::Output),
    );
    let (s, h, o) = (sens.len(), hid.len(), out.len());
    if h == 0 || s == 0 || o == 0 {
        return invalid(
            "an executable network needs at least one sensory, hidden and output neuron",
        );
    }
    let mut net = base;
    let set = |net: &mut Value, k: &str, v: Value| {
        net[k] = v;
    };
    set(&mut net, "num_sensory_neurons", s.into());
    set(&mut net, "num_hidden_layers", 1.into());
    set(&mut net, "num_hidden_per_layer_initial", h.into());
    set(&mut net, "num_output_neurons", o.into());
    set(&mut net, "max_total_neurons", (h + o).into());
    for (k, v) in [
        ("growth_enabled", false),
        ("morpho_growth_enabled", false),
        ("use_morphology", false),
        ("use_aarnn_delays", false),
        ("aarnn_plasticity_enabled", false),
        ("sleep_enabled", false),
        ("io_channels_are_biological", false),
    ] {
        set(&mut net, k, v.into());
    }
    set(&mut net, "max_layers", 1.into());
    set(&mut net, "development_stage_mode", "manual".into());
    // The designed-circuit overrides of scripts/aarnn_circuit.py::_net: whole-
    // network subsystems that would otherwise rewire, rescale or drive the
    // declared circuit (import rewiring, synaptic-energy randomness, ambient
    // energy, rhythms, sleep, volume transmission, scaling, myelination).
    for (k, v) in [
        ("development_pruning_interval_ms", json_f(1.0e12)),
        ("new_edge_prob", json_f(0.0)),
        ("aarnn_layer_depth", Value::from(0)),
        ("world_model_enabled", Value::Bool(false)),
        ("theta_rhythm_enabled", Value::Bool(false)),
        ("thalamic_gating_enabled", Value::Bool(false)),
        ("aarnn_dale_strictness", json_f(0.0)),
        ("aarnn_gap_junction_strength", json_f(0.0)),
        ("volume_transmission_enabled", Value::Bool(false)),
        ("aarnn_synaptic_scaling_strength", json_f(0.0)),
        ("aarnn_myelination_enabled", Value::Bool(false)),
        ("aarnn_import_topology_rewire_enabled", Value::Bool(false)),
        ("aarnn_import_topology_rewire_keep_fraction", json_f(1.0)),
        ("aarnn_output_gain_floor_enabled", Value::Bool(false)),
        ("aarnn_synaptic_energy_randomness", json_f(0.0)),
        ("perceptual_loop_enabled", Value::Bool(false)),
        ("aarnn_resonance_gain", json_f(0.0)),
        ("aarnn_ambient_energy_level", json_f(0.0)),
        // Reliable release as in the designed-circuit template (network.json:
        // 1.0; the NetworkConfig default 0.7 makes 30 % of releases fail).
        // Transmission failure is a separately declared fault, never implicit.
        ("p_release_default", json_f(1.0)),
        ("clumping_design", Value::from("None")),
        ("brain_regions", Value::Array(vec![])),
    ] {
        if net.get(k).is_none() {
            return Err(ExplorerError::Internal(format!(
                "designed-circuit override {k} is not a NetworkConfig field in this runtime"
            )));
        }
        net[k] = v;
    }
    net["aarnn_neuromod_transmission_enabled"] = Value::Bool(false);
    net["aarnn_neuromod_env_enabled"] = Value::Bool(false);
    for (k, val) in &d.net_overrides {
        if net.get(k).is_none() {
            return invalid(format!(
                "dynamics.net_overrides.{k}: not a NetworkConfig field"
            ));
        }
        net[k] = val.clone();
    }
    let mut spike_io = net["spike_io"].clone();
    spike_io["profile"] = "generic".into();
    spike_io["input_strategy"] = "threshold".into();
    spike_io["threshold"] = 0.5.into();
    net["spike_io"] = spike_io;
    let bio_base = net["aarnn_bio"].clone();
    let mut types: Vec<Value> = Vec::new();
    for (name, over) in &d.cell_types {
        let mut b = bio_base.clone();
        for (k, v) in over {
            if b.get(k).is_none() {
                return invalid(format!(
                    "dynamics.cell_types.{name}.{k}: not an AARNN bio parameter"
                ));
            }
            b[k] = v.clone();
        }
        types.push(serde_json::json!({"name": name, "bio_params": b}));
    }
    for n in t.neurons.iter().filter(|n| n.role != Role::Sensory) {
        if !d.cell_types.contains_key(&n.type_name) {
            return invalid(format!(
                "neuron {} has type {:?} without declared dynamics",
                n.id, n.type_name
            ));
        }
    }
    net["neuron_types"] = Value::Array(types);
    let mut w_in = vec![0.0; h * s];
    let mut w_rec = vec![0.0; h * h];
    let mut w_out = vec![0.0; o * h];
    let mut w_gap = vec![0.0; h * h];
    let mut has_gap = false;
    for syn in &t.synapses {
        let (a, b) = (&t.neurons[syn.pre as usize], &t.neurons[syn.post as usize]);
        let (ad, bd) = (a.dense as usize, b.dense as usize);
        match (syn.class, a.role, b.role) {
            (SynClass::Chemical, Role::Sensory, Role::Hidden) => w_in[bd * s + ad] += syn.weight,
            (SynClass::Chemical, Role::Hidden, Role::Hidden) => w_rec[bd * h + ad] += syn.weight,
            (SynClass::Chemical, Role::Hidden, Role::Output) => w_out[bd * h + ad] += syn.weight,
            (SynClass::Electrical, Role::Hidden, Role::Hidden) => {
                w_gap[ad * h + bd] += syn.weight.abs();
                w_gap[bd * h + ad] += syn.weight.abs();
                has_gap = true;
            }
            (c, ra, rb) => {
                return invalid(format!(
                    "edge {} ({c:?} {ra:?}->{rb:?}) has no native slot",
                    syn.id
                ));
            }
        }
    }
    let matj = |rows: usize, cols: usize, data: &[f64], bin: bool| -> Value {
        let d: Vec<Value> = data
            .iter()
            .map(|&x| {
                if bin {
                    Value::from(u32::from(x != 0.0))
                } else {
                    Value::from(x)
                }
            })
            .collect();
        serde_json::json!({"rows": rows, "cols": cols, "data": d})
    };
    let node = |n: &Neuron| {
        let p = n.position.unwrap_or([0.0, 0.0, 0.0]);
        serde_json::json!({"x": p[0], "y": p[1], "z": p[2], "layer": 0, "region_name": n.region, "type_name": n.type_name})
    };
    Ok(serde_json::json!({
        "net": net,
        "topo": {
            "layers": [hid.iter().map(|n| node(n)).collect::<Vec<_>>()],
            "sensory_nodes": sens.iter().map(|n| node(n)).collect::<Vec<_>>(),
            "output_nodes": out.iter().map(|n| node(n)).collect::<Vec<_>>(),
        },
        "w_in": matj(h, s, &w_in, false), "w_hh_fwd": [], "w_hh_bwd": [], "w_hh_rec": [matj(h, h, &w_rec, false)],
        "w_out": matj(o, h, &w_out, false), "w_gap": if has_gap { vec![matj(h, h, &w_gap, false)] } else { vec![] },
        "p_in": matj(h, s, &w_in, true), "p_fwd": [], "p_bwd": [], "p_rec": [matj(h, h, &w_rec, true)],
        "p_out": matj(o, h, &w_out, true), "layer_range": null, "rng_seed": 7,
        "connectome_labels": {
            "schema": "aarnn-explorer-compiled-v1",
            "sensory_nodes": sens.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "hidden_nodes": hid.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "output_nodes": out.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "assumption_version": d.assumption_version,
        },
    }))
}

fn json_f(x: f64) -> Value {
    serde_json::Number::from_f64(x)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}
