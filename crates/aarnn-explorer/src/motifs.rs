// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Recurring (analogous) subthreads: the second-order activity graph of
//! Theilman et al. (2023) Eq. 4-5.
//!
//! Vertices are pairs of events of the same neuron, one from activity graph A
//! and one from B. A second-order edge `(u_a, v_a) -> (u_b, v_b)` exists when
//! `u_a -> u_b` is an edge of A and `v_a -> v_b` is an edge of B. Analogous
//! subthreads are the weakly connected components. They need not be exactly
//! isomorphic subgraphs.
//!
//! Construction is sparse: B's edges are indexed by their (pre neuron, post
//! neuron) pair, so only edges with matching neuron pairs are paired, never
//! all spike pairs. The number of second-order edges is capped by a budget.
//! Exceeding it sets `approximate` and reports how many edges were seen.
//!
//! A fingerprint (hash of the sorted neuron-pair edge multiset) groups
//! candidate occurrences of one motif. Grouping is then verified by exact
//! multiset comparison, because hash equality alone is not proof.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::gnat::{Edge, Event};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Subthread {
    pub id: String,
    /// (event index in A, event index in B).
    pub pairs: Vec<(u32, u32)>,
    pub neurons: Vec<u32>,
    /// Sorted (pre neuron, post neuron) edge multiset.
    pub edge_pairs: Vec<(u32, u32)>,
    /// Mean and standard deviation of t_B - t_A over the pairs (ticks).
    pub mean_offset_ticks: f64,
    pub offset_std_ticks: f64,
    /// Spread of within-thread timing differences between the two copies.
    pub timing_jitter_ticks: f64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recurrence {
    pub subthreads: Vec<Subthread>,
    pub second_order_edges: u64,
    pub approximate: bool,
    pub budget: u64,
    pub min_size: usize,
}

fn fingerprint(edge_pairs: &[(u32, u32)]) -> String {
    let s: String = edge_pairs
        .iter()
        .map(|(a, b)| format!("{a}>{b};"))
        .collect();
    crate::store::sha256_hex(s.as_bytes())[..16].to_string()
}

/// `same_graph`: A and B are the same activity graph. Vertex pairs then need
/// a strictly later B copy (t_v > t_u), so a thread is not matched with
/// itself and each pair is counted once.
#[allow(clippy::too_many_arguments)]
pub fn analogous_subthreads(
    events_a: &[Event],
    edges_a: &[Edge],
    events_b: &[Event],
    edges_b: &[Edge],
    same_graph: bool,
    budget: u64,
    min_size: usize,
) -> Recurrence {
    let mut index_b: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::new();
    for e in edges_b {
        let key = (
            events_b[e.pre as usize].neuron,
            events_b[e.post as usize].neuron,
        );
        index_b.entry(key).or_default().push((e.pre, e.post));
    }
    let mut vid: HashMap<(u32, u32), u32> = HashMap::new();
    let mut verts: Vec<(u32, u32)> = Vec::new();
    let mut so_edges: Vec<(u32, u32)> = Vec::new();
    let mut seen = 0u64;
    let mut approximate = false;
    let mut intern = |p: (u32, u32), verts: &mut Vec<(u32, u32)>| -> u32 {
        *vid.entry(p).or_insert_with(|| {
            verts.push(p);
            (verts.len() - 1) as u32
        })
    };
    'outer: for ea in edges_a {
        let (ua, ub) = (ea.pre, ea.post);
        let key = (events_a[ua as usize].neuron, events_a[ub as usize].neuron);
        let Some(list) = index_b.get(&key) else {
            continue;
        };
        for &(va, vb) in list {
            if same_graph && events_b[va as usize].tick <= events_a[ua as usize].tick {
                continue;
            }
            seen += 1;
            if seen > budget {
                approximate = true;
                break 'outer;
            }
            let x = intern((ua, va), &mut verts);
            let y = intern((ub, vb), &mut verts);
            so_edges.push((x, y));
        }
    }
    let mut parent: Vec<u32> = (0..verts.len() as u32).collect();
    fn find(p: &mut [u32], x: u32) -> u32 {
        let mut r = x;
        while p[r as usize] != r {
            r = p[r as usize];
        }
        let mut c = x;
        while p[c as usize] != r {
            let n = p[c as usize];
            p[c as usize] = r;
            c = n;
        }
        r
    }
    for &(x, y) in &so_edges {
        let (a, b) = (find(&mut parent, x), find(&mut parent, y));
        if a != b {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            parent[hi as usize] = lo;
        }
    }
    let mut groups: BTreeMap<u32, (Vec<u32>, Vec<(u32, u32)>)> = BTreeMap::new();
    for (v, _) in verts.iter().enumerate() {
        let r = find(&mut parent, v as u32);
        groups.entry(r).or_default().0.push(v as u32);
    }
    for &(x, y) in &so_edges {
        let r = find(&mut parent, x);
        let (ua, _) = verts[x as usize];
        let (ub, _) = verts[y as usize];
        groups
            .get_mut(&r)
            .expect("root exists")
            .1
            .push((events_a[ua as usize].neuron, events_a[ub as usize].neuron));
    }
    let mut subthreads = Vec::new();
    for (_, (members, mut edge_pairs)) in groups {
        if members.len() < min_size {
            continue;
        }
        edge_pairs.sort_unstable();
        let mut pairs: Vec<(u32, u32)> = members.iter().map(|&v| verts[v as usize]).collect();
        pairs.sort_unstable();
        let offsets: Vec<f64> = pairs
            .iter()
            .map(|&(a, b)| events_b[b as usize].tick as f64 - events_a[a as usize].tick as f64)
            .collect();
        let mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
        let var =
            offsets.iter().map(|o| (o - mean) * (o - mean)).sum::<f64>() / offsets.len() as f64;
        let (lo, hi) = offsets
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &o| {
                (l.min(o), h.max(o))
            });
        let mut neurons: Vec<u32> = pairs
            .iter()
            .map(|&(a, _)| events_a[a as usize].neuron)
            .collect();
        neurons.sort_unstable();
        neurons.dedup();
        let first = pairs[0];
        let fp = fingerprint(&edge_pairs);
        subthreads.push(Subthread {
            id: format!(
                "sub-a{}-b{}",
                events_a[first.0 as usize].id, events_b[first.1 as usize].id
            ),
            pairs,
            neurons,
            edge_pairs,
            mean_offset_ticks: mean,
            offset_std_ticks: var.sqrt(),
            timing_jitter_ticks: hi - lo,
            fingerprint: fp,
        });
    }
    subthreads.sort_by(|a, b| b.pairs.len().cmp(&a.pairs.len()).then(a.id.cmp(&b.id)));
    Recurrence {
        subthreads,
        second_order_edges: seen.min(budget),
        approximate,
        budget,
        min_size,
    }
}

/// A motif: verified identical neuron-pair edge multisets across subthreads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Motif {
    pub fingerprint: String,
    pub edge_pairs: Vec<(u32, u32)>,
    pub neurons: Vec<u32>,
    pub occurrences: usize,
    pub subthread_ids: Vec<String>,
    pub mean_timing_jitter_ticks: f64,
    /// Fingerprint collisions found and separated by exact comparison.
    pub collisions_separated: usize,
}

pub fn group_motifs(subs: &[Subthread]) -> Vec<Motif> {
    let mut by_fp: BTreeMap<&str, Vec<&Subthread>> = BTreeMap::new();
    for s in subs {
        by_fp.entry(&s.fingerprint).or_default().push(s);
    }
    let mut out = Vec::new();
    for (fp, list) in by_fp {
        // Exact verification within a fingerprint bucket.
        let mut classes: Vec<Vec<&Subthread>> = Vec::new();
        for s in list {
            match classes.iter_mut().find(|c| c[0].edge_pairs == s.edge_pairs) {
                Some(c) => c.push(s),
                None => classes.push(vec![s]),
            }
        }
        let collisions = classes.len() - 1;
        for c in classes {
            out.push(Motif {
                fingerprint: fp.to_string(),
                edge_pairs: c[0].edge_pairs.clone(),
                neurons: c[0].neurons.clone(),
                occurrences: c.len(),
                subthread_ids: c.iter().map(|s| s.id.clone()).collect(),
                mean_timing_jitter_ticks: c.iter().map(|s| s.timing_jitter_ticks).sum::<f64>()
                    / c.len() as f64,
                collisions_separated: collisions,
            });
        }
    }
    out.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then(b.edge_pairs.len().cmp(&a.edge_pairs.len()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnat::{Adjacency, EventKind, GnatParams, Norm, indexed_edges};

    fn ev(id: u64, tick: u64, neuron: u32) -> Event {
        Event {
            id,
            tick,
            micro: 0,
            neuron,
            kind: EventKind::Spike,
        }
    }

    fn params() -> GnatParams {
        GnatParams {
            tau_ms: 5.0,
            dt_ms: 1.0,
            threshold: 0.0067,
        }
    }

    #[test]
    fn reused_subthread_with_timing_variation_is_found() {
        let adj = Adjacency::build(
            5,
            &[
                (0, 1, 1.0, 1),
                (1, 2, 1.0, 1),
                (2, 3, 1.0, 1),
                (3, 4, 1.0, 1),
            ],
            Norm::L1Abs,
        );
        // Trial A: exact chain timing. Trial B: the same chain, 2-tick jitter.
        let a: Vec<Event> = (0..5).map(|k| ev(k, 10 + k, k as u32)).collect();
        let b: Vec<Event> = vec![
            ev(10, 100, 0),
            ev(11, 101, 1),
            ev(12, 103, 2),
            ev(13, 104, 3),
            ev(14, 106, 4),
        ];
        let ea = indexed_edges(&adj, &a, &params()).unwrap();
        let eb = indexed_edges(&adj, &b, &params()).unwrap();
        let r = analogous_subthreads(&a, &ea, &b, &eb, false, 1000, 2);
        assert_eq!(r.subthreads.len(), 1);
        let s = &r.subthreads[0];
        assert_eq!(s.pairs.len(), 5);
        assert_eq!(s.neurons, vec![0, 1, 2, 3, 4]);
        assert!(
            s.timing_jitter_ticks >= 2.0,
            "jitter {}",
            s.timing_jitter_ticks
        );
        assert!(!r.approximate);
    }

    #[test]
    fn same_shape_on_different_neurons_is_not_a_match() {
        // Two chains with identical topology but disjoint neurons.
        let adj = Adjacency::build(
            6,
            &[
                (0, 1, 1.0, 1),
                (1, 2, 1.0, 1),
                (3, 4, 1.0, 1),
                (4, 5, 1.0, 1),
            ],
            Norm::L1Abs,
        );
        let a: Vec<Event> = (0..3).map(|k| ev(k, 10 + k, k as u32)).collect();
        let b: Vec<Event> = (0..3).map(|k| ev(10 + k, 50 + k, 3 + k as u32)).collect();
        let ea = indexed_edges(&adj, &a, &params()).unwrap();
        let eb = indexed_edges(&adj, &b, &params()).unwrap();
        assert!(
            analogous_subthreads(&a, &ea, &b, &eb, false, 1000, 2)
                .subthreads
                .is_empty()
        );
    }

    #[test]
    fn budget_overflow_is_flagged_approximate() {
        let adj = Adjacency::build(2, &[(0, 1, 1.0, 1)], Norm::L1Abs);
        let mut evs = Vec::new();
        for k in 0..50 {
            evs.push(ev(2 * k, 10 * k, 0));
            evs.push(ev(2 * k + 1, 10 * k + 1, 1));
        }
        let e = indexed_edges(&adj, &evs, &params()).unwrap();
        let r = analogous_subthreads(&evs, &e, &evs, &e, true, 10, 2);
        assert!(r.approximate);
        assert_eq!(r.second_order_edges, 10);
    }

    #[test]
    fn fingerprint_groups_are_verified_exactly() {
        let mk = |id: &str, fp: &str, pairs: Vec<(u32, u32)>| Subthread {
            id: id.into(),
            pairs: vec![(0, 0), (1, 1)],
            neurons: vec![0, 1],
            edge_pairs: pairs,
            mean_offset_ticks: 0.0,
            offset_std_ticks: 0.0,
            timing_jitter_ticks: 0.0,
            fingerprint: fp.into(),
        };
        // Forced collision: same fingerprint, different edge multisets.
        let subs = vec![
            mk("a", "x", vec![(0, 1)]),
            mk("b", "x", vec![(0, 1)]),
            mk("c", "x", vec![(1, 2)]),
        ];
        let m = group_motifs(&subs);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].occurrences, 2);
        assert_eq!(m[0].collisions_separated, 1);
    }
}
