// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Graphical Neural Activity Threads (Theilman, Wang, Rothganger and Aimone
//! 2023, arXiv:2306.16684 v1).
//!
//! Method `effective-delivery-v1` (see docs/connectome-explorer/methods.md):
//!
//! ```text
//! delta = t_post - (t_pre + d_ab)                       (ticks; 1 tick = dt_ms)
//! score = (w_ab / norm_a) * exp(-delta * dt_ms / tau_ms)  for w_ab > 0, delta >= 0
//! edge pre -> post  iff  score >= threshold              (ties are edges)
//! ```
//!
//! - `norm_a` is the L1 (default) or L2 norm of the absolute weights of every
//!   chemical input onto `a`, sensory inputs included. A zero norm means no
//!   edges onto `a`. The paper says only "the norm of all synaptic weights
//!   onto neuron alpha".
//! - Only positive weights create edges, as in the paper's excitatory case.
//!   Inhibition and gap junctions remain in the simulation and in overlays;
//!   their absence from the graph is never evidence that they are dispensable.
//! - `d_ab` is the effective synaptic delay in ticks. A zero-delay synapse
//!   (the legacy runner's sensory->hidden and hidden->output stages) creates
//!   an edge only when the pre event's microstep precedes the post event's:
//!   events carry their within-step stage as the microstep (sensory 0, hidden
//!   1, output 2; D-003). Microsteps order events; they add no duration to
//!   `delta`.
//! - GNATs are the weakly connected components of the event graph. Single
//!   events without edges are counted as isolated, not reported as GNATs.
//!
//! Eligibility horizon: score <= (w/norm) * exp(-delta/tau) and w/norm <= 1,
//! so an edge needs delta <= tau * ln((w/norm) / threshold). That bound is
//! exact, not an approximation.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

pub const METHOD: &str = "effective-delivery-v1";
pub const METHOD_GRADED: &str = "effective-delivery-graded-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    /// A spike of a spiking cell.
    Spike,
    /// A spike-equivalent release event of a graded cell (D-005).
    GradedEquivalent,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Source-scoped event identity (sequence number in the trace).
    pub id: u64,
    pub tick: u64,
    pub micro: u32,
    pub neuron: u32,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InSyn {
    pub pre: u32,
    pub weight: f64,
    pub delay_ticks: u32,
}

/// Incoming chemical adjacency per neuron plus per-neuron norms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adjacency {
    pub incoming: Vec<Vec<InSyn>>,
    pub norm: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Norm {
    L1Abs,
    L2Abs,
}

impl Norm {
    pub fn parse(s: &str) -> Option<Norm> {
        match s {
            "l1-abs" => Some(Norm::L1Abs),
            "l2-abs" => Some(Norm::L2Abs),
            _ => None,
        }
    }
}

impl Adjacency {
    /// `edges`: (pre, post, weight, delay_ticks) over all chemical synapses.
    pub fn build(n_neurons: usize, edges: &[(u32, u32, f64, u32)], norm: Norm) -> Adjacency {
        let mut incoming = vec![Vec::new(); n_neurons];
        for &(pre, post, w, d) in edges {
            if w != 0.0 && (post as usize) < n_neurons {
                incoming[post as usize].push(InSyn {
                    pre,
                    weight: w,
                    delay_ticks: d,
                });
            }
        }
        for v in incoming.iter_mut() {
            v.sort_by(|a, b| a.pre.cmp(&b.pre).then(a.delay_ticks.cmp(&b.delay_ticks)));
        }
        let norm_v = incoming
            .iter()
            .map(|v| match norm {
                Norm::L1Abs => v.iter().map(|s| s.weight.abs()).sum::<f64>(),
                Norm::L2Abs => v.iter().map(|s| s.weight * s.weight).sum::<f64>().sqrt(),
            })
            .collect();
        Adjacency {
            incoming,
            norm: norm_v,
        }
    }

    pub fn max_delay(&self) -> u32 {
        self.incoming
            .iter()
            .flat_map(|v| v.iter().map(|s| s.delay_ticks))
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GnatParams {
    pub tau_ms: f64,
    pub dt_ms: f64,
    pub threshold: f64,
}

impl GnatParams {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.tau_ms.is_finite() && self.tau_ms > 0.0) {
            return Err("tau_ms must be finite and > 0".into());
        }
        if !(self.dt_ms.is_finite() && self.dt_ms > 0.0) {
            return Err("dt_ms must be finite and > 0".into());
        }
        if !(self.threshold > 0.0 && self.threshold <= 1.0) {
            return Err("threshold must be in (0, 1]".into());
        }
        Ok(())
    }

    /// Largest eligible delta (ticks) for a synapse with ratio w/norm.
    pub fn horizon_ticks(&self, ratio: f64) -> Option<u64> {
        if ratio < self.threshold {
            return None;
        }
        Some(((self.tau_ms / self.dt_ms) * (ratio / self.threshold).ln()).floor() as u64)
    }

    fn score(&self, ratio: f64, delta: u64) -> f64 {
        ratio * (-(delta as f64) * self.dt_ms / self.tau_ms).exp()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Indices into the analysed event slice.
    pub pre: u32,
    pub post: u32,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    /// Deterministic: from the earliest member (tick, neuron, event id).
    pub id: String,
    pub events: Vec<u32>,
    pub neurons: Vec<u32>,
    pub start_tick: u64,
    pub end_tick: u64,
    /// Longest causal chain inside the component, in edges.
    pub depth: u32,
    /// May have unseen ancestors before the window.
    pub open_start: bool,
    /// May have unseen descendants after the window.
    pub open_end: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnatResult {
    pub method: String,
    pub params: GnatParams,
    pub window: Bounds,
    pub events: usize,
    pub edges: Vec<Edge>,
    pub components: Vec<Component>,
    pub isolated_events: usize,
    /// Ticks within which a boundary may hide a parent/child.
    pub boundary_reach_ticks: u64,
    pub approximations: Vec<String>,
}

#[derive(Debug)]
pub enum GnatError {
    Params(String),
    Unsorted,
    UnknownNeuron(u32),
    TooLarge(usize),
}

impl std::fmt::Display for GnatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GnatError::Params(m) => write!(f, "{m}"),
            GnatError::Unsorted => write!(f, "events must be sorted by (tick, micro, neuron, id)"),
            GnatError::UnknownNeuron(n) => {
                write!(f, "event names neuron {n}, outside the topology generation")
            }
            GnatError::TooLarge(n) => write!(
                f,
                "reference algorithm is bounded to 20000 events (got {n})"
            ),
        }
    }
}

fn check(adj: &Adjacency, events: &[Event], params: &GnatParams) -> Result<(), GnatError> {
    params.validate().map_err(GnatError::Params)?;
    if let Some(e) = events
        .iter()
        .find(|e| e.neuron as usize >= adj.incoming.len())
    {
        return Err(GnatError::UnknownNeuron(e.neuron));
    }
    if events.windows(2).any(|w| {
        (w[0].tick, w[0].micro, w[0].neuron, w[0].id)
            > (w[1].tick, w[1].micro, w[1].neuron, w[1].id)
    }) {
        return Err(GnatError::Unsorted);
    }
    Ok(())
}

/// Transparent reference: every ordered event pair is tested against every
/// synapse between their neurons. O(events^2); bounded fixtures only.
pub fn reference_edges(
    adj: &Adjacency,
    events: &[Event],
    params: &GnatParams,
) -> Result<Vec<Edge>, GnatError> {
    check(adj, events, params)?;
    if events.len() > 20_000 {
        return Err(GnatError::TooLarge(events.len()));
    }
    let mut syn: HashMap<(u32, u32), Vec<(f64, u32)>> = HashMap::new();
    for (post, v) in adj.incoming.iter().enumerate() {
        for s in v {
            syn.entry((s.pre, post as u32))
                .or_default()
                .push((s.weight, s.delay_ticks));
        }
    }
    let mut out = Vec::new();
    for (j, e) in events.iter().enumerate() {
        let norm = adj.norm.get(e.neuron as usize).copied().unwrap_or(0.0);
        if norm <= 0.0 {
            continue;
        }
        for (i, p) in events.iter().enumerate() {
            let Some(list) = syn.get(&(p.neuron, e.neuron)) else {
                continue;
            };
            // Parallel synapses: the best-scoring one defines the edge.
            let mut best: Option<f64> = None;
            for &(w, d) in list {
                if w <= 0.0 {
                    continue;
                }
                let arrival = p.tick + d as u64;
                if e.tick < arrival || (d == 0 && e.tick == arrival && p.micro >= e.micro) {
                    continue;
                }
                let sc = params.score(w / norm, e.tick - arrival);
                if sc >= params.threshold && best.is_none_or(|b| sc > b) {
                    best = Some(sc);
                }
            }
            if let Some(score) = best {
                out.push(Edge {
                    pre: i as u32,
                    post: j as u32,
                    score,
                });
            }
        }
    }
    out.sort_by(|a, b| (a.post, a.pre).cmp(&(b.post, b.pre)));
    Ok(out)
}

/// Indexed algorithm: per-neuron sorted event ticks, binary search within the
/// exact eligibility horizon of each incoming synapse.
pub fn indexed_edges(
    adj: &Adjacency,
    events: &[Event],
    params: &GnatParams,
) -> Result<Vec<Edge>, GnatError> {
    check(adj, events, params)?;
    let n = adj.incoming.len();
    let mut by_neuron: Vec<Vec<(u64, u32)>> = vec![Vec::new(); n];
    for (i, e) in events.iter().enumerate() {
        if (e.neuron as usize) < n {
            by_neuron[e.neuron as usize].push((e.tick, i as u32));
        }
    }
    let mut out = Vec::new();
    let mut best: BTreeMap<u32, f64> = BTreeMap::new();
    for (j, e) in events.iter().enumerate() {
        let a = e.neuron as usize;
        if a >= n {
            continue;
        }
        let norm = adj.norm[a];
        if norm <= 0.0 {
            continue;
        }
        best.clear();
        for s in &adj.incoming[a] {
            if s.weight <= 0.0 {
                continue;
            }
            let ratio = s.weight / norm;
            let Some(h) = params.horizon_ticks(ratio) else {
                continue;
            };
            let d = s.delay_ticks as u64;
            if e.tick < d {
                continue;
            }
            let hi = e.tick - d; // latest eligible pre tick
            let lo = hi.saturating_sub(h);
            let list = &by_neuron[s.pre as usize];
            let start = list.partition_point(|&(t, _)| t < lo);
            for &(t, i) in &list[start..] {
                if t > hi {
                    break;
                }
                if d == 0 && t == hi && events[i as usize].micro >= e.micro {
                    continue;
                }
                let sc = params.score(ratio, hi - t);
                if sc >= params.threshold {
                    let entry = best.entry(i).or_insert(sc);
                    if sc > *entry {
                        *entry = sc;
                    }
                }
            }
        }
        for (&i, &score) in &best {
            out.push(Edge {
                pre: i,
                post: j as u32,
                score,
            });
        }
    }
    out.sort_by(|a, b| (a.post, a.pre).cmp(&(b.post, b.pre)));
    Ok(out)
}

struct Dsu(Vec<u32>);

impl Dsu {
    fn find(&mut self, x: u32) -> u32 {
        let mut r = x;
        while self.0[r as usize] != r {
            r = self.0[r as usize];
        }
        let mut c = x;
        while self.0[c as usize] != r {
            let next = self.0[c as usize];
            self.0[c as usize] = r;
            c = next;
        }
        r
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Deterministic: the smaller root wins.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.0[hi as usize] = lo;
        }
    }
}

/// Weakly connected components plus causal depth and boundary flags.
/// Recording window. `start_is_origin`: the window starts at the run's cold
/// start, so no event can have unseen ancestors. `end_is_final`: the run ends
/// with the window, so no event can have unseen descendants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub start: u64,
    pub end: u64,
    pub start_is_origin: bool,
    pub end_is_final: bool,
}

impl Bounds {
    pub fn open(start: u64, end: u64) -> Bounds {
        Bounds {
            start,
            end,
            start_is_origin: false,
            end_is_final: false,
        }
    }
    pub fn whole_run(end: u64) -> Bounds {
        Bounds {
            start: 0,
            end,
            start_is_origin: true,
            end_is_final: true,
        }
    }
}

pub fn components(
    adj: &Adjacency,
    events: &[Event],
    edges: &[Edge],
    params: &GnatParams,
    window: Bounds,
) -> (Vec<Component>, usize, u64) {
    let n = events.len();
    let mut dsu = Dsu((0..n as u32).collect());
    let mut has_edge = vec![false; n];
    for e in edges {
        dsu.union(e.pre, e.post);
        has_edge[e.pre as usize] = true;
        has_edge[e.post as usize] = true;
    }
    // Longest path (edges) ending at each event: events are sorted by
    // (tick, micro) and every edge points to a later (tick, micro), so the
    // pre index is always below the post index and one pass in post order
    // suffices.
    let mut depth = vec![0u32; n];
    let mut by_post: Vec<&Edge> = edges.iter().collect();
    by_post.sort_by_key(|e| e.post);
    for e in by_post {
        let d = depth[e.pre as usize] + 1;
        if d > depth[e.post as usize] {
            depth[e.post as usize] = d;
        }
    }
    let ratio_max = adj
        .incoming
        .iter()
        .enumerate()
        .flat_map(|(a, v)| {
            let norm = adj.norm[a];
            v.iter()
                .filter(move |s| s.weight > 0.0 && norm > 0.0)
                .map(move |s| s.weight / norm)
        })
        .fold(0.0f64, f64::max);
    let reach = adj.max_delay() as u64 + params.horizon_ticks(ratio_max).unwrap_or(0);
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut isolated = 0usize;
    for i in 0..n as u32 {
        if has_edge[i as usize] {
            let r = dsu.find(i);
            groups.entry(r).or_default().push(i);
        } else {
            isolated += 1;
        }
    }
    let mut comps: Vec<Component> = groups
        .into_values()
        .map(|mut members| {
            members.sort_by_key(|&i| {
                let e = &events[i as usize];
                (e.tick, e.micro, e.neuron, e.id)
            });
            let first = &events[members[0] as usize];
            let start = first.tick;
            let end = members
                .iter()
                .map(|&i| events[i as usize].tick)
                .max()
                .unwrap_or(start);
            let mut neurons: Vec<u32> =
                members.iter().map(|&i| events[i as usize].neuron).collect();
            neurons.sort_unstable();
            neurons.dedup();
            Component {
                id: format!("gnat-t{}-n{}-e{}", first.tick, first.neuron, first.id),
                depth: members
                    .iter()
                    .map(|&i| depth[i as usize])
                    .max()
                    .unwrap_or(0),
                open_start: !window.start_is_origin && start < window.start + reach,
                open_end: !window.end_is_final && end + reach >= window.end,
                events: members,
                neurons,
                start_tick: start,
                end_tick: end,
            }
        })
        .collect();
    comps.sort_by(|a, b| (a.start_tick, &a.id).cmp(&(b.start_tick, &b.id)));
    (comps, isolated, reach)
}

pub fn analyse(
    adj: &Adjacency,
    events: &[Event],
    params: &GnatParams,
    window: Bounds,
    method: &str,
) -> Result<GnatResult, GnatError> {
    let edges = indexed_edges(adj, events, params)?;
    let (components, isolated, reach) = components(adj, events, &edges, params, window);
    Ok(GnatResult {
        method: method.to_string(),
        params: *params,
        window,
        events: events.len(),
        edges,
        components,
        isolated_events: isolated,
        boundary_reach_ticks: reach,
        approximations: Vec::new(),
    })
}

/// Summary statistics used by reports and by VX-001 (computation depth).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DepthSummary {
    pub gnats: usize,
    pub closed_gnats: usize,
    pub events_in_gnats: usize,
    pub isolated_events: usize,
    pub max_depth: u32,
    pub mean_depth: f64,
    pub p95_depth: u32,
    pub mean_size: f64,
    /// Mean of depth / distinct neurons per GNAT: causal hops per cell.
    pub mean_depth_per_neuron: f64,
    pub participating_neurons: usize,
}

pub fn depth_summary(r: &GnatResult) -> DepthSummary {
    let mut depths: Vec<u32> = r.components.iter().map(|c| c.depth).collect();
    depths.sort_unstable();
    let g = r.components.len();
    let mut neurons: Vec<u32> = r
        .components
        .iter()
        .flat_map(|c| c.neurons.iter().copied())
        .collect();
    neurons.sort_unstable();
    neurons.dedup();
    DepthSummary {
        gnats: g,
        closed_gnats: r
            .components
            .iter()
            .filter(|c| !c.open_start && !c.open_end)
            .count(),
        events_in_gnats: r.components.iter().map(|c| c.events.len()).sum(),
        isolated_events: r.isolated_events,
        max_depth: depths.last().copied().unwrap_or(0),
        mean_depth: if g > 0 {
            depths.iter().map(|&d| d as f64).sum::<f64>() / g as f64
        } else {
            0.0
        },
        p95_depth: if g > 0 {
            depths[((g - 1) as f64 * 0.95).round() as usize]
        } else {
            0
        },
        mean_size: if g > 0 {
            r.components
                .iter()
                .map(|c| c.events.len() as f64)
                .sum::<f64>()
                / g as f64
        } else {
            0.0
        },
        mean_depth_per_neuron: if g > 0 {
            r.components
                .iter()
                .map(|c| c.depth as f64 / c.neurons.len().max(1) as f64)
                .sum::<f64>()
                / g as f64
        } else {
            0.0
        },
        participating_neurons: neurons.len(),
    }
}

/// Per-neuron participation: events of the neuron inside GNATs, and the
/// number of GNATs it appears in. Used to rank removal hypotheses.
pub fn participation(r: &GnatResult, events: &[Event], n_neurons: usize) -> Vec<(u64, u32)> {
    let mut out = vec![(0u64, 0u32); n_neurons];
    for c in &r.components {
        for &i in &c.events {
            let nrn = events[i as usize].neuron as usize;
            if nrn < n_neurons {
                out[nrn].0 += 1;
            }
        }
        for &nrn in &c.neurons {
            if (nrn as usize) < n_neurons {
                out[nrn as usize].1 += 1;
            }
        }
    }
    out
}

/// Threshold calibration against circularly time-shifted controls, which
/// keep each neuron's rate and inter-event intervals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Calibration {
    /// Histogram bins of -ln(score), width `bin`, from 0.
    pub bin: f64,
    pub real: Vec<u64>,
    pub shuffled: Vec<u64>,
    pub suggested_threshold: f64,
    /// Fraction of edges above the suggested threshold expected by chance.
    pub false_edge_fraction: f64,
    /// `calibrated`, `no-transition` (every bin passes) or `uncertain`.
    pub status: String,
    pub shuffles: u32,
    pub seed: u64,
}

pub fn calibrate(
    adj: &Adjacency,
    events: &[Event],
    params: &GnatParams,
    window: Bounds,
    shuffles: u32,
    seed: u64,
    max_false_fraction: f64,
) -> Result<Calibration, GnatError> {
    let floor = GnatParams {
        threshold: 1e-6,
        ..*params
    };
    let bin = 0.25;
    let nb = ((-(floor.threshold.ln())) / bin).ceil() as usize + 1;
    let hist = |edges: &[Edge]| {
        let mut h = vec![0u64; nb];
        for e in edges {
            let k = ((-e.score.ln()) / bin).floor().max(0.0) as usize;
            h[k.min(nb - 1)] += 1;
        }
        h
    };
    let real = hist(&indexed_edges(adj, events, &floor)?);
    let span = (window.end - window.start).max(1);
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut shuffled = vec![0u64; nb];
    let n = adj.incoming.len();
    for _ in 0..shuffles.max(1) {
        let offsets: Vec<u64> = (0..n).map(|_| rng.u64(0..span)).collect();
        let mut sh: Vec<Event> = events
            .iter()
            .map(|e| {
                let off = offsets.get(e.neuron as usize).copied().unwrap_or(0);
                Event {
                    tick: window.start + (e.tick - window.start + off) % span,
                    ..*e
                }
            })
            .collect();
        sh.sort_by_key(|e| (e.tick, e.micro, e.neuron, e.id));
        for (k, v) in hist(&indexed_edges(adj, &sh, &floor)?)
            .into_iter()
            .enumerate()
        {
            shuffled[k] += v;
        }
    }
    let s = shuffles.max(1) as f64;
    // Walk outwards from strong scores; keep the deepest bin boundary where
    // the cumulative chance fraction stays within max_false_fraction.
    let (mut cr, mut cs) = (0.0f64, 0.0f64);
    let mut best: Option<(usize, f64)> = None;
    for k in 0..nb {
        cr += real[k] as f64;
        cs += shuffled[k] as f64 / s;
        if cr > 0.0 {
            let frac = cs / cr;
            if frac <= max_false_fraction {
                best = Some((k, frac));
            } else {
                break;
            }
        }
    }
    let (suggested, frac, status) = match best {
        Some((k, f)) if k + 1 >= nb => ((-((k + 1) as f64 * bin)).exp(), f, "no-transition"),
        Some((k, f)) => ((-((k + 1) as f64 * bin)).exp(), f, "calibrated"),
        None => (params.threshold, f64::NAN, "uncertain"),
    };
    Ok(Calibration {
        bin,
        real,
        shuffled,
        suggested_threshold: suggested,
        false_edge_fraction: frac,
        status: status.into(),
        shuffles: shuffles.max(1),
        seed,
    })
}

/// GNAT counts over a tau x threshold grid (analysis sensitivity; kept
/// independent of resilience settings).
pub fn sensitivity(
    adj: &Adjacency,
    events: &[Event],
    params: &GnatParams,
    window: Bounds,
) -> Result<Vec<serde_json::Value>, GnatError> {
    let mut out = Vec::new();
    for tf in [0.5, 1.0, 2.0] {
        for thf in [0.5, 1.0, 2.0] {
            let p = GnatParams {
                tau_ms: params.tau_ms * tf,
                threshold: (params.threshold * thf).min(1.0),
                ..*params
            };
            let r = analyse(adj, events, &p, window, METHOD)?;
            let d = depth_summary(&r);
            out.push(serde_json::json!({
                "tau_ms": p.tau_ms, "threshold": p.threshold,
                "gnats": d.gnats, "edges": r.edges.len(), "max_depth": d.max_depth,
                "mean_size": d.mean_size, "isolated_events": d.isolated_events,
            }));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: u64, tick: u64, neuron: u32) -> Event {
        Event {
            id,
            tick,
            micro: 0,
            neuron,
            kind: EventKind::Spike,
        }
    }

    fn p(threshold: f64) -> GnatParams {
        GnatParams {
            tau_ms: 5.0,
            dt_ms: 1.0,
            threshold,
        }
    }

    fn sorted(mut v: Vec<Event>) -> Vec<Event> {
        v.sort_by_key(|e| (e.tick, e.micro, e.neuron, e.id));
        v
    }

    /// Chain 0 -> 1 -> 2 with delay 1, a branch 1 -> 3, reconvergence
    /// 2 -> 4 and 3 -> 4, and an unrelated spike on neuron 5.
    fn fixture() -> (Adjacency, Vec<Event>) {
        let syn = vec![
            (0, 1, 1.0, 1),
            (1, 2, 1.0, 1),
            (1, 3, 1.0, 2),
            (2, 4, 1.0, 1),
            (3, 4, 1.0, 1),
            (5, 6, 1.0, 1),
        ];
        let adj = Adjacency::build(7, &syn, Norm::L1Abs);
        let events = sorted(vec![
            ev(0, 10, 0),
            ev(1, 11, 1),
            ev(2, 12, 2),
            ev(3, 13, 3),
            ev(4, 14, 4),
            ev(5, 30, 5),
        ]);
        (adj, events)
    }

    #[test]
    fn chain_branch_reconvergence_form_one_gnat() {
        let (adj, events) = fixture();
        let r = analyse(&adj, &events, &p(0.0067), Bounds::whole_run(100), METHOD).unwrap();
        assert_eq!(r.components.len(), 1, "{:?}", r.components);
        let c = &r.components[0];
        assert_eq!(c.neurons, vec![0, 1, 2, 3, 4]);
        // Longest chain 0 -> 1 -> 2 -> 4 (or 0 -> 1 -> 3 -> 4): 3 edges.
        assert_eq!(c.depth, 3);
        assert_eq!(r.isolated_events, 1, "neuron 5's spike has no partner");
        // Norm onto 4 is 2: each reconverging edge has ratio 0.5; 3 -> 4
        // arrives exactly (delta 0), 2 -> 4 one tick early (delta 1).
        let e4: Vec<_> = r
            .edges
            .iter()
            .filter(|e| events[e.post as usize].neuron == 4)
            .collect();
        assert_eq!(e4.len(), 2);
        let mut sc: Vec<f64> = e4.iter().map(|e| e.score).collect();
        sc.sort_by(f64::total_cmp);
        assert!((sc[1] - 0.5).abs() < 1e-12 && (sc[0] - 0.5 * (-0.2f64).exp()).abs() < 1e-12);
    }

    #[test]
    fn reference_and_indexed_agree_on_adversarial_fixtures() {
        let mut rng = fastrand::Rng::with_seed(11);
        for case in 0..40 {
            let n = 3 + rng.usize(0..8);
            let mut syn = Vec::new();
            for a in 0..n as u32 {
                for b in 0..n as u32 {
                    if rng.f64() < 0.35 {
                        // Mixed signs, parallel synapses, delays 1..6.
                        let w = if rng.f64() < 0.25 {
                            -rng.f64()
                        } else {
                            rng.f64()
                        };
                        syn.push((a, b, w, rng.u32(0..6)));
                        if rng.f64() < 0.1 {
                            syn.push((a, b, rng.f64(), 1 + rng.u32(0..6)));
                        }
                    }
                }
            }
            let adj = Adjacency::build(
                n,
                &syn,
                if case % 2 == 0 {
                    Norm::L1Abs
                } else {
                    Norm::L2Abs
                },
            );
            let mut events = Vec::new();
            for id in 0..(20 + rng.u64(0..120)) {
                // Recurrent firing: the same neuron fires repeatedly.
                let mut e = ev(id, rng.u64(0..60), rng.u32(0..n as u32));
                e.micro = rng.u32(0..3);
                events.push(e);
            }
            let events = sorted(events);
            let th = [0.001, 0.0067, 0.05, 0.3][case % 4];
            let a = reference_edges(&adj, &events, &p(th)).unwrap();
            let b = indexed_edges(&adj, &events, &p(th)).unwrap();
            assert_eq!(a.len(), b.len(), "case {case}");
            for (x, y) in a.iter().zip(&b) {
                assert_eq!((x.pre, x.post), (y.pre, y.post), "case {case}");
                assert!((x.score - y.score).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn false_coincidence_without_synapse_is_not_an_edge() {
        // 0 and 2 fire 1 tick apart but are not connected.
        let adj = Adjacency::build(3, &[(0, 1, 1.0, 1)], Norm::L1Abs);
        let events = sorted(vec![ev(0, 5, 0), ev(1, 6, 2)]);
        let r = analyse(&adj, &events, &p(0.0067), Bounds::whole_run(50), METHOD).unwrap();
        assert!(r.edges.is_empty() && r.components.is_empty());
    }

    #[test]
    fn spike_before_arrival_and_weak_or_zero_influence_are_excluded() {
        let adj = Adjacency::build(3, &[(0, 1, 1.0, 3), (2, 1, 1000.0, 1)], Norm::L1Abs);
        // Post spike at 11: arrival from 0 is at 13 -> no edge; from 2 at 11
        // -> ratio 1000/1001.
        let events = sorted(vec![ev(0, 10, 0), ev(1, 10, 2), ev(2, 11, 1)]);
        let r = indexed_edges(&adj, &events, &p(0.0067)).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(events[r[0].pre as usize].neuron, 2);
        // The 0 -> 1 synapse has ratio 1/1001 < threshold 0.0067: never an edge.
        let late = sorted(vec![ev(0, 10, 0), ev(1, 13, 1)]);
        assert!(indexed_edges(&adj, &late, &p(0.0067)).unwrap().is_empty());
    }

    #[test]
    fn threshold_ties_are_edges() {
        let adj = Adjacency::build(2, &[(0, 1, 1.0, 1)], Norm::L1Abs);
        let events = sorted(vec![ev(0, 0, 0), ev(1, 3, 1)]);
        let exact = (-2.0f64 / 5.0).exp();
        assert_eq!(indexed_edges(&adj, &events, &p(exact)).unwrap().len(), 1);
        assert_eq!(
            indexed_edges(&adj, &events, &p(exact * 1.000001))
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn inhibition_never_creates_edges_but_counts_in_the_norm() {
        let adj = Adjacency::build(3, &[(0, 2, 1.0, 1), (1, 2, -3.0, 1)], Norm::L1Abs);
        let events = sorted(vec![ev(0, 0, 0), ev(1, 0, 1), ev(2, 1, 2)]);
        let r = indexed_edges(&adj, &events, &p(0.0067)).unwrap();
        assert_eq!(r.len(), 1);
        assert!((r[0].score - 0.25).abs() < 1e-12, "norm |1| + |-3| = 4");
    }

    #[test]
    fn overlapping_independent_threads_stay_separate() {
        let adj = Adjacency::build(4, &[(0, 1, 1.0, 1), (2, 3, 1.0, 1)], Norm::L1Abs);
        let events = sorted(vec![ev(0, 5, 0), ev(1, 5, 2), ev(2, 6, 1), ev(3, 6, 3)]);
        let r = analyse(&adj, &events, &p(0.0067), Bounds::whole_run(50), METHOD).unwrap();
        assert_eq!(r.components.len(), 2);
    }

    #[test]
    fn later_edge_merges_provisional_components() {
        // 0 -> 2 and 1 -> 2: two parents first look separate, merge at 2.
        let adj = Adjacency::build(3, &[(0, 2, 1.0, 2), (1, 2, 1.0, 1)], Norm::L1Abs);
        let events = sorted(vec![ev(0, 0, 0), ev(1, 1, 1), ev(2, 2, 2)]);
        let r = analyse(&adj, &events, &p(0.0067), Bounds::whole_run(50), METHOD).unwrap();
        assert_eq!(r.components.len(), 1);
        assert_eq!(r.components[0].events.len(), 3);
    }

    #[test]
    fn window_boundaries_mark_open_components() {
        let (adj, events) = fixture();
        // A chunk of a longer run: ancestors/descendants may lie outside.
        let r = analyse(&adj, &events, &p(0.0067), Bounds::open(9, 15), METHOD).unwrap();
        assert!(r.components[0].open_start && r.components[0].open_end);
        // Far from both edges of a chunk: closed.
        let r = analyse(&adj, &events, &p(0.0067), Bounds::open(0, 200), METHOD).unwrap();
        assert!(
            r.components[0].open_start,
            "tick 10 is within reach of the chunk start"
        );
        assert!(!r.components[0].open_end);
        // The whole run from its cold start: no unknown prehistory.
        let r = analyse(&adj, &events, &p(0.0067), Bounds::whole_run(200), METHOD).unwrap();
        assert!(!r.components[0].open_start && !r.components[0].open_end);
    }

    #[test]
    fn zero_delay_edges_need_an_earlier_stage() {
        // Sensory (micro 0) -> hidden (micro 1) in the same step: an edge.
        let adj = Adjacency::build(3, &[(0, 1, 1.0, 0), (1, 2, 1.0, 0)], Norm::L1Abs);
        let mk = |id, tick, micro, neuron| Event {
            id,
            tick,
            micro,
            neuron,
            kind: EventKind::Spike,
        };
        let evs = vec![mk(0, 4, 0, 0), mk(1, 4, 1, 1), mk(2, 4, 1, 2)];
        let a = indexed_edges(&adj, &evs, &p(0.01)).unwrap();
        let b = reference_edges(&adj, &evs, &p(0.01)).unwrap();
        assert_eq!(a, b);
        // 0 -> 1 is an edge; 1 -> 2 is not (same stage, no order).
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].pre, a[0].post), (0, 1));
    }

    #[test]
    fn calibration_finds_a_transition_on_causal_data() {
        // A strongly driven chain repeated 30 times plus background noise.
        let syn: Vec<_> = (0..5)
            .map(|i| (i, i + 1, 1.0, 1))
            .chain((0..6).map(|i| (6 + i, i, 0.05, 1)))
            .collect();
        let adj = Adjacency::build(12, &syn, Norm::L1Abs);
        let mut events = Vec::new();
        let mut id = 0;
        let mut rng = fastrand::Rng::with_seed(3);
        for rep in 0..30u64 {
            for k in 0..6u64 {
                events.push(ev(id, rep * 40 + k, k as u32));
                id += 1;
            }
        }
        for _ in 0..200 {
            events.push(ev(id, rng.u64(0..1200), 6 + rng.u32(0..6)));
            id += 1;
        }
        let events = sorted(events);
        let c = calibrate(
            &adj,
            &events,
            &p(0.0067),
            Bounds::whole_run(1200),
            4,
            9,
            0.25,
        )
        .unwrap();
        assert_eq!(c.status, "calibrated", "{c:?}");
        assert!(c.suggested_threshold > 1e-6 && c.suggested_threshold < 1.0);
    }
}
