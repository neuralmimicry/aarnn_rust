// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Authoring small circuits for fixtures and examples. A circuit is written
//! as an `aarnn-explorer-sparse-v1` document with declared dynamics and
//! compiled by the same importer users run (`model::from_sparse`), so the
//! fixtures exercise the real import path and the real AARNN runner.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::error::Result;
use crate::model::{self, Topology};

/// Spiking cell with the runtime's unmodelled nonlinearities off (active
/// dendrites, adaptive threshold, short-term plasticity, NMDA, homeostasis),
/// so responses are hand-checkable. Izhikevich regular spiking.
pub fn exact_spiking() -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    for (k, v) in [
        ("graded_enabled", json!(false)),
        ("dendritic_active_enabled", json!(false)),
        ("adaptive_threshold_enabled", json!(false)),
        ("stp_enabled", json!(false)),
        ("nmda_ratio", json!(0.0)),
        ("homeostasis_gain", json!(0.0)),
        ("tonic_current", json!(0.0)),
    ] {
        m.insert(k.to_string(), v);
    }
    m
}

#[derive(Debug, Clone, Default)]
pub struct Circuit {
    pub name: String,
    types: BTreeMap<String, BTreeMap<String, Value>>,
    neurons: Vec<Value>,
    edges: Vec<Value>,
}

impl Circuit {
    pub fn new(name: &str) -> Circuit {
        Circuit {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn ty(&mut self, name: &str, params: BTreeMap<String, Value>) -> &mut Self {
        self.types.insert(name.to_string(), params);
        self
    }

    pub fn sensory(&mut self, id: &str) -> &mut Self {
        self.neurons
            .push(json!({"id": id, "role": "sensory", "type": "", "region": "sensory"}));
        self
    }

    pub fn hidden(&mut self, id: &str, ty: &str, region: &str) -> &mut Self {
        self.neurons
            .push(json!({"id": id, "role": "hidden", "type": ty, "region": region}));
        self
    }

    pub fn output(&mut self, id: &str, ty: &str) -> &mut Self {
        self.neurons
            .push(json!({"id": id, "role": "output", "type": ty, "region": "motor"}));
        self
    }

    pub fn syn(&mut self, pre: &str, post: &str, w: f64) -> &mut Self {
        self.edges.push(
            json!({"pre": pre, "post": post, "weight": w, "class": "chemical", "delay_ticks": 1}),
        );
        self
    }

    pub fn sparse(&self) -> Value {
        json!({
            "schema": "aarnn-explorer-sparse-v1",
            "dataset": {
                "name": self.name, "version": "authored-1", "specimen": "synthetic",
                "coverage": "complete (authored circuit)", "licence": "MIT (project-authored)",
            },
            "units": {"weight": "aarnn-native", "delay": "ticks"},
            "neurons": self.neurons,
            "edges": self.edges,
            "dynamics": {
                "cell_types": self.types,
                "weight_conversion": "explicit-weight",
                "assumption_version": "authored-exact-spiking-v1",
            },
        })
    }

    pub fn compile(&self) -> Result<(Topology, Value)> {
        let (t, snap) = model::from_sparse(&self.sparse().to_string(), 1_000_000, 10_000_000)?;
        Ok((t, snap.expect("authored circuits declare dynamics")))
    }
}

/// Weights of the authored demonstration circuit (see `demo_circuit`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemoWeights {
    pub relay: f64,
    pub inhibit: f64,
    pub loop_w: f64,
    pub mem_to_q: f64,
    pub probe_to_q: f64,
}

impl Default for DemoWeights {
    fn default() -> Self {
        // Calibrated on the real runner (tests/calibrate_demo.rs, 26 Nov):
        // relay 16 gives one spike per input spike; FS inhibition -16 blocks
        // the go response (-30 causes rebound bursts); with the FS loop
        // (~6 ms period) Q fires only on probe + memory at every probe phase.
        DemoWeights {
            relay: 16.0,
            inhibit: -16.0,
            loop_w: 16.0,
            mem_to_q: 1.5,
            probe_to_q: 2.5,
        }
    }
}

/// `explorer-synthetic-sensorimotor-v1`: the authored fixtures of brief
/// Section 21 in one executable circuit.
/// - Redundant go pathway: A -> G1a -> G2a -> O_go and A -> G1b -> G2b ->
///   O_go; either chain alone suffices, removing both fails.
/// - No-go inhibition: B -> I, and I inhibits O_go, G2a and G2b. I is
///   essential to the no-go task although inhibition creates no
///   positive-edge GNAT edge.
/// - Memory: C starts a reverberating loop M1 -> M2 -> M3 -> M1. Q fires only
///   when the probe P coincides with the loop, and drives O_mem. Pruning or
///   collapsing the loop destroys the delayed response.
/// - Steering (closed loop): SL -> TL -> OL and SR -> TR -> OR, each side
///   with a second relay (TL2, TR2) for redundancy.
/// - Dispensable: the dead-end branch A -> D1 -> D2 (no path to an output)
///   and the isolated cell Z.
pub fn demo_circuit(w: DemoWeights) -> Circuit {
    demo_circuit_with(w, "FS", "FS")
}

/// `loop_preset` / `inh_preset`: Izhikevich presets of the memory loop and
/// the inhibitory interneuron (development sweeps).
pub fn demo_circuit_with(w: DemoWeights, loop_preset: &str, inh_preset: &str) -> Circuit {
    let mut c = Circuit::new("explorer-synthetic-sensorimotor-v1");
    c.ty("Exc", exact_spiking());
    let mut lp = exact_spiking();
    lp.insert("izh_preset".into(), json!(loop_preset));
    c.ty("Loop", lp);
    let mut ip = exact_spiking();
    ip.insert("izh_preset".into(), json!(inh_preset));
    c.ty("Inh", ip);
    for s in ["A", "B", "C", "P", "SL", "SR"] {
        c.sensory(s);
    }
    for (id, region) in [
        ("G1a", "go"),
        ("G2a", "go"),
        ("G1b", "go"),
        ("G2b", "go"),
        ("I", "nogo"),
        ("M1", "memory"),
        ("M2", "memory"),
        ("M3", "memory"),
        ("Q", "memory"),
        ("TL", "steer"),
        ("TL2", "steer"),
        ("TR", "steer"),
        ("TR2", "steer"),
        ("D1", "deadend"),
        ("D2", "deadend"),
        ("Z", "isolated"),
    ] {
        let ty = match id {
            "M1" | "M2" | "M3" => "Loop",
            "I" => "Inh",
            _ => "Exc",
        };
        c.hidden(id, ty, region);
    }
    for o in ["O_go", "O_mem", "OL", "OR"] {
        c.output(o, "Exc");
    }
    let r = w.relay;
    c.syn("A", "G1a", r)
        .syn("G1a", "G2a", r)
        .syn("G2a", "O_go", r);
    c.syn("A", "G1b", r)
        .syn("G1b", "G2b", r)
        .syn("G2b", "O_go", r);
    c.syn("B", "I", r)
        .syn("I", "O_go", w.inhibit)
        .syn("I", "G2a", w.inhibit)
        .syn("I", "G2b", w.inhibit);
    c.syn("C", "M1", r)
        .syn("M1", "M2", w.loop_w)
        .syn("M2", "M3", w.loop_w)
        .syn("M3", "M1", w.loop_w);
    c.syn("M3", "Q", w.mem_to_q)
        .syn("P", "Q", w.probe_to_q)
        .syn("Q", "O_mem", r);
    c.syn("SL", "TL", r)
        .syn("TL", "OL", r)
        .syn("SL", "TL2", r)
        .syn("TL2", "OL", r);
    c.syn("SR", "TR", r)
        .syn("TR", "OR", r)
        .syn("SR", "TR2", r)
        .syn("TR2", "OR", r);
    c.syn("A", "D1", r).syn("D1", "D2", r);
    c
}
