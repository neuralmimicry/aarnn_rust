// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Interventions on isolated copies, plus physical lowering of a selected
//! reduction (brief Sections 9 and 12).
//!
//! Every intervention applies to a copy of an immutable source snapshot at a
//! declared cold-start boundary (tick 0). No event is in flight, so the
//! pending-event policy is `cold-start-empty`. The source object is never
//! modified. Its hash is checked before and after in `experiment`.
//!
//! Classes:
//! - **mask** (`structural-mask`): the cell keeps its state and execution cost,
//!   but its synapses are zeroed. Used for causal tests.
//! - **lowering** (`physical-reduction`): rows and columns are removed from
//!   every matrix, presence pattern and per-cell configuration array. This
//!   actually shrinks storage and work, and the result is measured.
//! - **parameter change**: weight scaling or bounded perturbation through the
//!   native weights.
//! - **transient silencing**: the cell's outgoing weights are zeroed for a
//!   logical interval, then restored (frozen plasticity only; `exec`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ExplorerError, Result, invalid};
use crate::model::{Role, SynClass, Topology};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Selection {
    /// Explicit stable IDs (neurons).
    Neurons {
        ids: Vec<String>,
    },
    /// Explicit stable synapse IDs (`pre->post`).
    Synapses {
        ids: Vec<String>,
    },
    Region {
        region: String,
    },
    CellType {
        type_name: String,
    },
    /// Deterministic random fraction of the eligible hidden population. The
    /// count is round-half-up of fraction x population, minimum 1 when the
    /// fraction is > 0.
    Random {
        fraction: f64,
        seed: u64,
        /// Restrict the eligible population to these regions (empty: all).
        #[serde(default)]
        regions: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    NeuronAblation,
    SynapseAblation,
    WeightScale {
        factor: f64,
    },
    /// Set each selected synapse's weight magnitude to `value` (native
    /// units), keeping its sign; e.g. plastic synapses at the plasticity cap
    /// for a learned-state source (`derive --preset learned-cap`).
    WeightSet {
        value: f64,
    },
    /// Multiply each selected weight by (1 + sigma * U(-1, 1)), seeded.
    WeightPerturb {
        sigma: f64,
        seed: u64,
    },
    Silence {
        start_ms: f64,
        end_ms: f64,
    },
    /// No-op control with the same bookkeeping as a lesion.
    Sham,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Intervention {
    pub id: String,
    pub selection: Selection,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Applied {
    pub id: String,
    /// transient-mask | structural-mask | parameter-change | sham
    pub classification: String,
    pub neurons: Vec<String>,
    pub synapses: Vec<String>,
    pub effective_tick: u64,
    pub pending_event_policy: String,
    /// Hidden dense indices to silence (exec::Silence), when transient.
    pub silence: Vec<(u32, u64, u64)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Protection {
    /// Sensory and output neurons can never be removed (I/O mapping).
    pub protect_io: bool,
    /// Additional protected stable IDs.
    pub ids: BTreeSet<String>,
}

/// Resolve a selection to hidden neuron IDs and synapse IDs.
pub fn resolve(
    t: &Topology,
    sel: &Selection,
    prot: &Protection,
) -> Result<(Vec<String>, Vec<String>)> {
    let hidden = |pred: &dyn Fn(&crate::model::Neuron) -> bool| -> Vec<String> {
        t.neurons
            .iter()
            .filter(|n| n.role == Role::Hidden && pred(n))
            .map(|n| n.id.clone())
            .collect()
    };
    let (mut neurons, synapses) = match sel {
        Selection::Neurons { ids } => {
            for id in ids {
                let Some(i) = t.index_of(id) else {
                    return invalid(format!(
                        "selection: unknown neuron {id:?} (stable IDs, not display indices)"
                    ));
                };
                if t.neurons[i as usize].role != Role::Hidden && prot.protect_io {
                    return invalid(format!(
                        "selection: {id} is a protected input/output neuron"
                    ));
                }
            }
            (ids.clone(), Vec::new())
        }
        Selection::Synapses { ids } => {
            for id in ids {
                if !t.synapses.iter().any(|s| &s.id == id) {
                    return invalid(format!("selection: unknown synapse {id:?}"));
                }
            }
            (Vec::new(), ids.clone())
        }
        Selection::Region { region } => (hidden(&|n| &n.region == region), Vec::new()),
        Selection::CellType { type_name } => (hidden(&|n| &n.type_name == type_name), Vec::new()),
        Selection::Random {
            fraction,
            seed,
            regions,
        } => {
            if !(0.0..=1.0).contains(fraction) {
                return invalid("random fraction must be in [0, 1]");
            }
            let pop = hidden(&|n| {
                !prot.ids.contains(&n.id) && (regions.is_empty() || regions.contains(&n.region))
            });
            let k = if *fraction > 0.0 {
                ((fraction * pop.len() as f64) + 0.5).floor().max(1.0) as usize
            } else {
                0
            };
            let mut rng = fastrand::Rng::with_seed(*seed);
            let mut idx: Vec<usize> = (0..pop.len()).collect();
            rng.shuffle(&mut idx);
            let mut pick: Vec<String> = idx
                .into_iter()
                .take(k.min(pop.len()))
                .map(|i| pop[i].clone())
                .collect();
            pick.sort();
            (pick, Vec::new())
        }
    };
    if let Some(p) = neurons.iter().find(|n| prot.ids.contains(*n)) {
        return invalid(format!("selection includes protected neuron {p}"));
    }
    neurons.sort();
    neurons.dedup();
    Ok((neurons, synapses))
}

fn mat_mut<'a>(v: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    let m = v.get_mut(key)?;
    if m.is_array() {
        m.get_mut(0)
    } else if m.is_object() {
        Some(m)
    } else {
        None
    }
}

fn zero_entries(v: &mut Value, key: &str, f: &dyn Fn(usize, usize) -> bool) {
    if let Some(m) = mat_mut(v, key) {
        let cols = m["cols"].as_u64().unwrap_or(0) as usize;
        if let Some(d) = m.get_mut("data").and_then(|d| d.as_array_mut()) {
            for (k, x) in d.iter_mut().enumerate() {
                if cols > 0 && f(k / cols, k % cols) {
                    // Keep the element type: presence patterns are u32.
                    *x = if x.is_u64() || x.is_i64() {
                        Value::from(0u32)
                    } else {
                        Value::from(0.0)
                    };
                }
            }
        }
    }
}

fn scale_entries(v: &mut Value, key: &str, f: &mut dyn FnMut(usize, usize, f64) -> Option<f64>) {
    if let Some(m) = mat_mut(v, key) {
        let cols = m["cols"].as_u64().unwrap_or(0) as usize;
        if let Some(d) = m.get_mut("data").and_then(|d| d.as_array_mut()) {
            for (k, x) in d.iter_mut().enumerate() {
                if cols == 0 {
                    break;
                }
                let w = x.as_f64().unwrap_or(0.0);
                if let Some(nw) = f(k / cols, k % cols, w) {
                    *x = serde_json::Number::from_f64(nw)
                        .map(Value::Number)
                        .unwrap_or(Value::from(0.0));
                }
            }
        }
    }
}

/// Matrix slot of a synapse: (weight key, pattern key, row, col).
fn slot(
    t: &Topology,
    s: &crate::model::Synapse,
) -> Option<(&'static str, &'static str, usize, usize)> {
    let (a, b) = (&t.neurons[s.pre as usize], &t.neurons[s.post as usize]);
    match (s.class, a.role, b.role) {
        (SynClass::Chemical, Role::Sensory, Role::Hidden) => {
            Some(("w_in", "p_in", b.dense as usize, a.dense as usize))
        }
        (SynClass::Chemical, Role::Hidden, Role::Hidden) => {
            Some(("w_hh_rec", "p_rec", b.dense as usize, a.dense as usize))
        }
        (SynClass::Chemical, Role::Hidden, Role::Output) => {
            Some(("w_out", "p_out", b.dense as usize, a.dense as usize))
        }
        (SynClass::Electrical, Role::Hidden, Role::Hidden) => {
            Some(("w_gap", "", a.dense as usize, b.dense as usize))
        }
        _ => None,
    }
}

/// Apply interventions to a copy of `snapshot`.
pub fn apply(
    t: &Topology,
    snapshot: &Value,
    list: &[Intervention],
    prot: &Protection,
) -> Result<(Value, Vec<Applied>)> {
    let mut snap = snapshot.clone();
    let mut applied = Vec::new();
    for iv in list {
        let (neurons, mut synapses) = resolve(t, &iv.selection, prot)?;
        let dense: BTreeSet<usize> = neurons
            .iter()
            .filter_map(|id| t.index_of(id))
            .map(|i| t.neurons[i as usize].dense as usize)
            .collect();
        let mut silence = Vec::new();
        let classification = match &iv.action {
            Action::Sham => "sham",
            Action::NeuronAblation => {
                if neurons.is_empty() {
                    return invalid(format!(
                        "{}: neuron ablation selected no hidden neurons",
                        iv.id
                    ));
                }
                // Mask: every incident synapse of the selected cells is zeroed.
                zero_entries(&mut snap, "w_in", &|r, _| dense.contains(&r));
                zero_entries(&mut snap, "p_in", &|r, _| dense.contains(&r));
                zero_entries(&mut snap, "w_hh_rec", &|r, c| {
                    dense.contains(&r) || dense.contains(&c)
                });
                zero_entries(&mut snap, "p_rec", &|r, c| {
                    dense.contains(&r) || dense.contains(&c)
                });
                zero_entries(&mut snap, "w_out", &|_, c| dense.contains(&c));
                zero_entries(&mut snap, "p_out", &|_, c| dense.contains(&c));
                zero_entries(&mut snap, "w_gap", &|r, c| {
                    dense.contains(&r) || dense.contains(&c)
                });
                let global: BTreeSet<u32> =
                    neurons.iter().filter_map(|id| t.index_of(id)).collect();
                synapses = t
                    .synapses
                    .iter()
                    .filter(|s| global.contains(&s.pre) || global.contains(&s.post))
                    .map(|s| s.id.clone())
                    .collect();
                "structural-mask"
            }
            Action::SynapseAblation => {
                if synapses.is_empty() {
                    return invalid(format!(
                        "{}: synapse ablation needs a synapses selection",
                        iv.id
                    ));
                }
                for sid in &synapses {
                    let s = t.synapses.iter().find(|s| &s.id == sid).expect("resolved");
                    let (wk, pk, r, c) = slot(t, s).ok_or_else(|| {
                        ExplorerError::Unsupported(format!("synapse {sid} has no native slot"))
                    })?;
                    zero_entries(&mut snap, wk, &|rr, cc| rr == r && cc == c);
                    if wk == "w_gap" {
                        zero_entries(&mut snap, wk, &|rr, cc| rr == c && cc == r);
                    } else {
                        zero_entries(&mut snap, pk, &|rr, cc| rr == r && cc == c);
                    }
                }
                "structural-mask"
            }
            Action::WeightScale { factor } | Action::WeightPerturb { sigma: factor, .. } => {
                if !factor.is_finite() || factor.abs() > 1e3 {
                    return invalid(format!(
                        "{}: weight change must be finite and bounded",
                        iv.id
                    ));
                }
                let perturb = matches!(iv.action, Action::WeightPerturb { .. });
                let seed = if let Action::WeightPerturb { seed, .. } = iv.action {
                    seed
                } else {
                    0
                };
                let mut rng = fastrand::Rng::with_seed(seed);
                let targets: Vec<&crate::model::Synapse> = if !synapses.is_empty() {
                    t.synapses
                        .iter()
                        .filter(|s| synapses.contains(&s.id))
                        .collect()
                } else {
                    t.synapses
                        .iter()
                        .filter(|s| {
                            let (a, b) = (&t.neurons[s.pre as usize], &t.neurons[s.post as usize]);
                            (a.role == Role::Hidden && dense.contains(&(a.dense as usize)))
                                || (b.role == Role::Hidden && dense.contains(&(b.dense as usize)))
                        })
                        .collect()
                };
                for s in targets {
                    let (wk, _, r, c) = slot(t, s).ok_or_else(|| {
                        ExplorerError::Unsupported(format!("synapse {} has no native slot", s.id))
                    })?;
                    let m = if perturb {
                        1.0 + factor * (2.0 * rng.f64() - 1.0)
                    } else {
                        *factor
                    };
                    scale_entries(&mut snap, wk, &mut |rr, cc, w| {
                        if rr == r && cc == c {
                            Some(w * m)
                        } else {
                            None
                        }
                    });
                    synapses.push(s.id.clone());
                }
                synapses.sort();
                synapses.dedup();
                "parameter-change"
            }
            Action::WeightSet { value } => {
                if !value.is_finite() || *value < 0.0 {
                    return invalid(format!(
                        "{}: weight-set value must be finite and >= 0",
                        iv.id
                    ));
                }
                if synapses.is_empty() {
                    return invalid(format!("{}: weight-set needs a synapses selection", iv.id));
                }
                for sid in &synapses {
                    let s = t.synapses.iter().find(|s| &s.id == sid).expect("resolved");
                    let (wk, _, r, c) = slot(t, s).ok_or_else(|| {
                        ExplorerError::Unsupported(format!("synapse {sid} has no native slot"))
                    })?;
                    let sign = if s.weight < 0.0 { -1.0 } else { 1.0 };
                    scale_entries(&mut snap, wk, &mut |rr, cc, _| {
                        (rr == r && cc == c).then_some(sign * value)
                    });
                }
                "parameter-change"
            }
            Action::Silence { start_ms, end_ms } => {
                if !(start_ms.is_finite() && end_ms > start_ms && *start_ms >= 0.0) {
                    return invalid(format!("{}: silence interval", iv.id));
                }
                for &d in &dense {
                    silence.push((
                        d as u32,
                        (*start_ms / t.dt_ms) as u64,
                        (*end_ms / t.dt_ms) as u64,
                    ));
                }
                "transient-mask"
            }
        };
        applied.push(Applied {
            id: iv.id.clone(),
            classification: classification.into(),
            neurons,
            synapses,
            effective_tick: 0,
            pending_event_policy:
                "cold-start-empty: interventions apply before the first step; no event is in flight"
                    .into(),
            silence,
        });
    }
    Ok((snap, applied))
}

// ---------------------------------------------------------------------------
// Physical lowering
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mapping {
    pub schema_version: u32,
    /// Source hidden ID -> candidate dense index.
    pub hidden: BTreeMap<String, u32>,
    pub retired_neurons: Vec<String>,
    pub retired_synapses: Vec<String>,
    /// Sensory/output mapping is the identity (protected).
    pub io_identity: bool,
    /// Per-cell configuration arrays filtered with the hidden layer.
    pub filtered_config_arrays: Vec<String>,
    pub notes: Vec<String>,
}

fn filter_matrix(
    v: &mut Value,
    key: &str,
    keep_rows: Option<&[usize]>,
    keep_cols: Option<&[usize]>,
) -> Result<()> {
    let Some(m) = mat_mut(v, key) else {
        return Ok(());
    };
    let rows = m["rows"].as_u64().unwrap_or(0) as usize;
    let cols = m["cols"].as_u64().unwrap_or(0) as usize;
    let data: Vec<Value> = m["data"].as_array().cloned().unwrap_or_default();
    if !data.is_empty() && data.len() != rows * cols {
        return invalid(format!("{key}: corrupt matrix"));
    }
    let rr: Vec<usize> = keep_rows
        .map(|k| k.to_vec())
        .unwrap_or_else(|| (0..rows).collect());
    let cc: Vec<usize> = keep_cols
        .map(|k| k.to_vec())
        .unwrap_or_else(|| (0..cols).collect());
    let mut out = Vec::with_capacity(rr.len() * cc.len());
    if !data.is_empty() {
        for &r in &rr {
            for &c in &cc {
                out.push(data[r * cols + c].clone());
            }
        }
    }
    m["rows"] = Value::from(rr.len());
    m["cols"] = Value::from(cc.len());
    m["data"] = Value::Array(out);
    Ok(())
}

/// Filter every `[[...]]` per-hidden-cell array (one entry per hidden cell)
/// in the network settings, e.g. plastic-cell and growth-parent masks.
fn filter_cell_arrays(
    net: &mut Value,
    old_h: usize,
    keep: &[usize],
    path: &str,
    found: &mut Vec<String>,
) {
    match net {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                filter_cell_arrays(v, old_h, keep, &format!("{path}.{k}"), found);
            }
        }
        Value::Array(a) => {
            if a.len() == 1
                && a[0].as_array().is_some_and(|inner| {
                    inner.len() == old_h && inner.iter().all(|x| x.is_boolean() || x.is_number())
                })
            {
                let inner = a[0].as_array().cloned().unwrap_or_default();
                a[0] = Value::Array(keep.iter().map(|&i| inner[i].clone()).collect());
                found.push(path.to_string());
            }
        }
        _ => {}
    }
}

/// Physically remove hidden neurons and individual synapses.
pub fn lower(
    t: &Topology,
    snapshot: &Value,
    remove_neurons: &[String],
    remove_synapses: &[String],
    prot: &Protection,
) -> Result<(Value, Mapping)> {
    let mut snap = snapshot.clone();
    let h = t.count(Role::Hidden);
    let mut drop: BTreeSet<usize> = BTreeSet::new();
    for id in remove_neurons {
        let i = t
            .index_of(id)
            .ok_or_else(|| ExplorerError::InvalidInput(format!("unknown neuron {id}")))?;
        let n = &t.neurons[i as usize];
        if n.role != Role::Hidden {
            return invalid(format!(
                "{id}: only hidden neurons can be removed (I/O mapping is protected)"
            ));
        }
        if prot.ids.contains(id) {
            return invalid(format!("{id} is protected"));
        }
        drop.insert(n.dense as usize);
    }
    if drop.len() == h {
        return invalid("lowering would remove every hidden neuron");
    }
    let mut notes = Vec::new();
    if snap.get("runtime_state").is_some_and(|r| !r.is_null()) {
        snap["runtime_state"] = Value::Null;
        notes.push("runtime_state dropped: the lowered candidate is a declared cold start".into());
    }
    // Zero individually removed synapses first (in source indexing).
    for sid in remove_synapses {
        let s = t
            .synapses
            .iter()
            .find(|s| &s.id == sid)
            .ok_or_else(|| ExplorerError::InvalidInput(format!("unknown synapse {sid}")))?;
        let (wk, pk, r, c) = slot(t, s).ok_or_else(|| {
            ExplorerError::Unsupported(format!("synapse {sid} has no native slot"))
        })?;
        zero_entries(&mut snap, wk, &|rr, cc| rr == r && cc == c);
        if wk == "w_gap" {
            zero_entries(&mut snap, wk, &|rr, cc| rr == c && cc == r);
        } else {
            zero_entries(&mut snap, pk, &|rr, cc| rr == r && cc == c);
        }
    }
    let keep: Vec<usize> = (0..h).filter(|i| !drop.contains(i)).collect();
    filter_matrix(&mut snap, "w_in", Some(&keep), None)?;
    filter_matrix(&mut snap, "p_in", Some(&keep), None)?;
    filter_matrix(&mut snap, "w_hh_rec", Some(&keep), Some(&keep))?;
    filter_matrix(&mut snap, "p_rec", Some(&keep), Some(&keep))?;
    filter_matrix(&mut snap, "w_out", None, Some(&keep))?;
    filter_matrix(&mut snap, "p_out", None, Some(&keep))?;
    filter_matrix(&mut snap, "w_gap", Some(&keep), Some(&keep))?;
    if let Some(layer) = snap["topo"]["layers"]
        .get_mut(0)
        .and_then(|l| l.as_array_mut())
    {
        let old = layer.clone();
        *layer = keep.iter().map(|&i| old[i].clone()).collect();
    }
    let hid_names: Vec<Value> = snap["connectome_labels"]["hidden_nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if hid_names.len() == h {
        snap["connectome_labels"]["hidden_nodes"] =
            Value::Array(keep.iter().map(|&i| hid_names[i].clone()).collect());
    }
    let mut found = Vec::new();
    let net = snap
        .get_mut("net")
        .ok_or_else(|| ExplorerError::InvalidInput("snapshot has no net".into()))?;
    filter_cell_arrays(net, h, &keep, "net", &mut found);
    net["num_hidden_per_layer_initial"] = Value::from(keep.len());
    if let Some(m) = net["max_total_neurons"].as_u64() {
        net["max_total_neurons"] = Value::from(m.saturating_sub(drop.len() as u64));
    }
    let hidden_ids: Vec<&crate::model::Neuron> = t
        .neurons
        .iter()
        .filter(|n| n.role == Role::Hidden)
        .collect();
    let mut map = BTreeMap::new();
    for (new_i, &old_i) in keep.iter().enumerate() {
        map.insert(hidden_ids[old_i].id.clone(), new_i as u32);
    }
    let retired_neurons: Vec<String> = drop.iter().map(|&i| hidden_ids[i].id.clone()).collect();
    let mut retired_synapses: Vec<String> = t
        .synapses
        .iter()
        .filter(|s| {
            [s.pre, s.post].iter().any(|&g| {
                let n = &t.neurons[g as usize];
                n.role == Role::Hidden && drop.contains(&(n.dense as usize))
            })
        })
        .map(|s| s.id.clone())
        .chain(remove_synapses.iter().cloned())
        .collect();
    retired_synapses.sort();
    retired_synapses.dedup();
    Ok((
        snap,
        Mapping {
            schema_version: 1,
            hidden: map,
            retired_neurons,
            retired_synapses,
            io_identity: true,
            filtered_config_arrays: found,
            notes,
        },
    ))
}

/// Verified-inert hidden cells: no directed chemical path and no electrical
/// coupling path to any output, not a modulatory/plastic/growth-parent cell,
/// and the network declares no environmental neuromodulator drive (activity
/// of any cell could otherwise change the global modulator levels).
pub fn inert_cells(t: &Topology, snapshot: &Value) -> (Vec<String>, Vec<String>) {
    let net = &snapshot["net"];
    let mut reasons = Vec::new();
    if net["aarnn_neuromod_env_enabled"].as_bool().unwrap_or(true) {
        reasons.push("aarnn_neuromod_env_enabled: cell activity drives global modulator levels, so dead-end cells are not verified inert (task validation still decides)".into());
        return (Vec::new(), reasons);
    }
    if net["growth_enabled"].as_bool().unwrap_or(false) {
        reasons.push(
            "growth_enabled: newborns may connect dead-end cells, so they are not verified inert"
                .into(),
        );
        return (Vec::new(), reasons);
    }
    let n = t.neurons.len();
    let mut rev: Vec<Vec<u32>> = vec![Vec::new(); n];
    for s in &t.synapses {
        if s.weight != 0.0 {
            rev[s.post as usize].push(s.pre);
            if s.class == SynClass::Electrical {
                rev[s.pre as usize].push(s.post);
            }
        }
    }
    let mut reach = vec![false; n];
    let mut q: VecDeque<u32> = t
        .neurons
        .iter()
        .enumerate()
        .filter(|(_, x)| x.role == Role::Output)
        .map(|(i, _)| i as u32)
        .collect();
    for &o in &q {
        reach[o as usize] = true;
    }
    while let Some(x) = q.pop_front() {
        for &p in &rev[x as usize] {
            if !reach[p as usize] {
                reach[p as usize] = true;
                q.push_back(p);
            }
        }
    }
    let special = |key: &str, d: usize| -> bool {
        let v = &net[key];
        let arr = v.get("hidden").and_then(|h| h.get(0)).or_else(|| v.get(0));
        arr.and_then(|a| a.as_array())
            .and_then(|a| a.get(d))
            .and_then(|x| x.as_bool())
            .unwrap_or(false)
            && v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true)
    };
    let mut out = Vec::new();
    for (i, nrn) in t.neurons.iter().enumerate() {
        if nrn.role != Role::Hidden || reach[i] {
            continue;
        }
        let d = nrn.dense as usize;
        if special("aarnn_modulatory_cells", d)
            || special("aarnn_plastic_cells", d)
            || special("growth_parent_cells", d)
        {
            reasons.push(format!(
                "{}: no path to an output but modulatory/plastic/growth-parent",
                nrn.id
            ));
            continue;
        }
        out.push(nrn.id.clone());
    }
    (out, reasons)
}

/// Cost vector of an executable snapshot (brief Section 12).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Cost {
    pub neurons: u64,
    pub hidden: u64,
    pub synapses: u64,
    /// Mutable per-cell plus per-synapse state of the native runner
    /// (estimate: 12 f64 per hidden/output cell, 2 f64 per stored weight
    /// including the dense matrix slots the runner allocates).
    pub state_bytes: u64,
    /// Bytes of the dense matrices the runner allocates (w_in, w_hh_rec, w_out, w_gap).
    pub memory_bytes: u64,
    /// Measured median step time (ns), when a run measured it.
    pub latency_ns_p50: Option<u64>,
    pub event_rate_hz: Option<f64>,
    pub cost_basis: String,
}

pub fn cost(snapshot: &Value) -> Cost {
    let dims = |k: &str| -> (u64, u64, u64) {
        let m = &snapshot[k];
        let m = if m.is_array() { &m[0] } else { m };
        let r = m["rows"].as_u64().unwrap_or(0);
        let c = m["cols"].as_u64().unwrap_or(0);
        let nz = m["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|x| x.as_f64().unwrap_or(0.0) != 0.0)
                    .count() as u64
            })
            .unwrap_or(0);
        (r, c, nz)
    };
    let (hi, s, nz_in) = dims("w_in");
    let (_, _, nz_rec) = dims("w_hh_rec");
    let (o, _, nz_out) = dims("w_out");
    let (gr, gc, nz_gap) = dims("w_gap");
    let dense_slots = hi * s + hi * hi + o * hi + gr * gc;
    Cost {
        neurons: s + hi + o,
        hidden: hi,
        synapses: nz_in + nz_rec + nz_out + nz_gap / 2,
        state_bytes: (hi + o) * 12 * 8 + dense_slots * 2 * 8,
        memory_bytes: dense_slots * 8,
        latency_ns_p50: None,
        event_rate_hz: None,
        cost_basis: "proxy: native dense-matrix allocation plus 12 state words per cell; latency measured per run when available".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author::{DemoWeights, demo_circuit};

    #[test]
    fn lowering_removes_rows_columns_and_keeps_mapping() {
        let (t, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
        let prot = Protection {
            protect_io: true,
            ..Default::default()
        };
        let (inert, why) = inert_cells(&t, &snap);
        assert_eq!(inert, vec!["D1", "D2", "Z"], "{why:?}");
        let before = cost(&snap);
        let (low, map) = lower(&t, &snap, &inert, &[], &prot).unwrap();
        let after = cost(&low);
        assert_eq!(after.hidden, before.hidden - 3);
        assert!(after.memory_bytes < before.memory_bytes);
        assert_eq!(map.retired_neurons, vec!["D1", "D2", "Z"]);
        assert_eq!(map.retired_synapses, vec!["A->D1", "D1->D2"]);
        assert_eq!(after.hidden, 13);
        // Re-import: the lowered topology is consistent and keeps the I/O.
        let (t2, _) = crate::model::from_native(&low.to_string()).unwrap();
        assert_eq!(t2.count(Role::Sensory), t.count(Role::Sensory));
        assert_eq!(t2.count(Role::Output), t.count(Role::Output));
        assert!(t2.index_of("D1").is_none() && t2.index_of("M1").is_some());
    }

    #[test]
    fn protected_io_and_malformed_selections_are_rejected() {
        let (t, snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
        let prot = Protection {
            protect_io: true,
            ..Default::default()
        };
        let bad = Intervention {
            id: "x".into(),
            selection: Selection::Neurons {
                ids: vec!["O_go".into()],
            },
            action: Action::NeuronAblation,
        };
        assert!(apply(&t, &snap, &[bad], &prot).is_err());
        let unknown = Intervention {
            id: "y".into(),
            selection: Selection::Neurons {
                ids: vec!["nope".into()],
            },
            action: Action::NeuronAblation,
        };
        assert!(apply(&t, &snap, &[unknown], &prot).is_err());
        assert!(lower(&t, &snap, &["A".into()], &[], &prot).is_err());
    }

    #[test]
    fn weight_set_puts_plastic_synapses_at_the_cap() {
        let (t, mut snap) = demo_circuit(DemoWeights::default()).compile().unwrap();
        // A hidden cell with an excitatory hidden input becomes plastic, and
        // only one of its presynaptic cells is allowed to teach it.
        let s0 = t
            .synapses
            .iter()
            .find(|s| {
                s.weight > 0.0
                    && t.neurons[s.pre as usize].role == Role::Hidden
                    && t.neurons[s.post as usize].role == Role::Hidden
            })
            .expect("an excitatory hidden synapse")
            .clone();
        let h = t.count(Role::Hidden);
        let (pre, post) = (&t.neurons[s0.pre as usize], &t.neurons[s0.post as usize]);
        let mut m_post = vec![false; h];
        m_post[post.dense as usize] = true;
        let mut m_pre = vec![false; h];
        m_pre[pre.dense as usize] = true;
        // No mask: refused (every synapse would count as plastic).
        assert!(crate::optimise::plastic_synapses(&t, &snap).is_err());
        snap["net"]["aarnn_plastic_cells"] = serde_json::json!({"enabled": true, "hidden": [m_post], "pre_hidden": [m_pre], "outputs": []});
        let ids = crate::optimise::plastic_synapses(&t, &snap).unwrap();
        assert_eq!(ids, vec![s0.id.clone()]);
        let cap = 0.37;
        let iv = Intervention {
            id: "cap".into(),
            selection: Selection::Synapses { ids: ids.clone() },
            action: Action::WeightSet { value: cap },
        };
        let (out, applied) = apply(&t, &snap, &[iv], &Protection::default()).unwrap();
        assert_eq!(applied[0].classification, "parameter-change");
        let (t2, _) = crate::model::from_native(&out.to_string()).unwrap();
        for s in &t2.synapses {
            let before = t.synapses.iter().find(|x| x.id == s.id).unwrap();
            if s.id == s0.id {
                assert!(
                    (s.weight - cap).abs() < 1e-9,
                    "{} -> {}",
                    before.weight,
                    s.weight
                );
            } else {
                assert_eq!(s.weight, before.weight, "{} unchanged", s.id);
            }
        }
        // A negative magnitude is refused.
        let bad = Intervention {
            id: "bad".into(),
            selection: Selection::Synapses { ids },
            action: Action::WeightSet { value: -1.0 },
        };
        assert!(apply(&t, &snap, &[bad], &Protection::default()).is_err());
    }

    #[test]
    fn random_selection_is_deterministic_and_rounds_small_populations() {
        let (t, _) = demo_circuit(DemoWeights::default()).compile().unwrap();
        let prot = Protection::default();
        let a = resolve(
            &t,
            &Selection::Random {
                fraction: 0.01,
                seed: 4,
                regions: vec![],
            },
            &prot,
        )
        .unwrap()
        .0;
        let b = resolve(
            &t,
            &Selection::Random {
                fraction: 0.01,
                seed: 4,
                regions: vec![],
            },
            &prot,
        )
        .unwrap()
        .0;
        assert_eq!(a, b);
        assert_eq!(a.len(), 1, "a non-zero fraction removes at least one cell");
    }
}
