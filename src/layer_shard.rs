// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Hidden-layer sharding of the legacy [`Runner`] kernel.
//!
//! A distributed worker owns a contiguous range of whole hidden layers
//! (`Runner::layer_range`, assigned by the orchestrator's `LoadNetwork`
//! command) plus the output stage when it owns the output source layer (or,
//! with biological I/O, the extra execution layer `num_hidden_layers`).
//! Layers are never split, so recurrent drive and electrical synapses
//! (`w_gap`) are always shard-local. Across a shard boundary the kernel reads
//! the boundary layer's spikes (feed-forward: the current step of `l - 1`;
//! feedback: the previous step of `l + 1`), its delay history and STP/trace
//! state derived from those spikes and, for graded cells, its membrane
//! voltage. [`Runner::stage_remote_layer`] admits that state at the point of
//! the step where the single-node kernel computes the remote layer.
//!
//! [`LayerShardGroup`] is the in-process lock-step reference: it steps the
//! shards in ascending layer order and hands each one every remote layer's
//! newest committed state (and the output owner's previous-step output),
//! which reproduces single-node execution. The free functions below are the
//! wire helpers used by the gRPC transport in `distributed.rs`.

use crate::runner::{Runner, StepOut};
use std::ops::Range;

/// How boundary state crosses shards in [`LayerShardGroup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryExchange {
    /// Stage spikes (plus membrane voltages of layers with graded cells) and
    /// the output owner's previous output; admitted by the kernel at the
    /// single-node evaluation point. This is the distributed transport.
    Staged,
    /// The transport before staged admission: overwrite each remote layer's
    /// `last_spk_h` with the owner's spikes before the step. No voltages,
    /// delay history, STP/trace replay or output state cross the boundary.
    /// Kept to quantify that behaviour against single-node execution.
    LegacySpikeOverwrite,
    /// No boundary exchange: remote layers stay at their initial state.
    /// Shows that a network's shards depend on the boundary at all.
    Isolated,
}

/// Execution layers of a network: hidden layers plus the separate output
/// stage of a biological-I/O profile (as planned by the orchestrator).
pub fn execution_layer_count(runner: &Runner) -> usize {
    runner.net.num_hidden_layers + usize::from(runner.net.io_channels_are_biological)
}

/// Split `0..layers` into `shards` contiguous, non-empty ranges of near-equal
/// size (earlier ranges take the remainder).
pub fn even_layer_ranges(layers: usize, shards: usize) -> Vec<Range<usize>> {
    let shards = shards.clamp(1, layers.max(1));
    let base = layers / shards;
    let extra = layers % shards;
    let mut start = 0;
    (0..shards)
        .map(|i| {
            let len = base + usize::from(i < extra);
            let range = start..start + len;
            start += len;
            range
        })
        .collect()
}

/// Lock-step execution of one network split into hidden-layer shards.
pub struct LayerShardGroup {
    shards: Vec<Runner>,
    ranges: Vec<Range<usize>>,
    output_owner: usize,
    exchange: BoundaryExchange,
}

impl LayerShardGroup {
    /// Bind identical runners (same snapshot) to contiguous layer ranges
    /// that cover every execution layer in ascending order.
    pub fn new(
        mut shards: Vec<Runner>,
        ranges: Vec<Range<usize>>,
        exchange: BoundaryExchange,
    ) -> Result<Self, String> {
        if shards.is_empty() || shards.len() != ranges.len() {
            return Err(format!(
                "{} runners for {} layer ranges",
                shards.len(),
                ranges.len()
            ));
        }
        let layers = execution_layer_count(&shards[0]);
        let mut next = 0;
        for range in &ranges {
            if range.start != next || range.end <= range.start {
                return Err(format!(
                    "layer ranges must be contiguous and non-empty: {ranges:?}"
                ));
            }
            next = range.end;
        }
        if next != layers {
            return Err(format!(
                "layer ranges {ranges:?} do not cover {layers} execution layers"
            ));
        }
        for (runner, range) in shards.iter_mut().zip(&ranges) {
            runner.layer_range = Some(range.clone());
        }
        let owners: Vec<usize> = (0..shards.len())
            .filter(|&i| shards[i].owns_output_stage())
            .collect();
        let [output_owner] = owners[..] else {
            return Err(format!("output stage owned by shards {owners:?}"));
        };
        Ok(Self {
            shards,
            ranges,
            output_owner,
            exchange,
        })
    }

    pub fn shards(&self) -> &[Runner] {
        &self.shards
    }

    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    /// Index of the shard that owns hidden `layer`.
    pub fn layer_owner(&self, layer: usize) -> usize {
        self.ranges
            .iter()
            .position(|r| r.contains(&layer))
            .unwrap_or(0)
    }

    /// The shard that computes the output stage (and the embodiment output).
    pub fn output_runner(&self) -> &Runner {
        &self.shards[self.output_owner]
    }

    /// Advance the whole network by one step. Every shard receives the same
    /// sensory frame; only the owner of the sensory target layer uses it for
    /// drive. Returns the output owner's step result.
    pub fn step(&mut self, sensory: Option<&[i8]>) -> StepOut {
        let previous_output = self.shards[self.output_owner].last_spk_o.to_vec();
        let mut output = None;
        for i in 0..self.shards.len() {
            // Shards before `i` have committed this step, shards after it the
            // previous one: exactly the state the single-node kernel sees.
            let frames = if self.exchange == BoundaryExchange::Isolated {
                Vec::new()
            } else {
                self.remote_frames(i)
            };
            let shard = &mut self.shards[i];
            match self.exchange {
                BoundaryExchange::Staged => {
                    for (layer, spikes, membrane_v) in &frames {
                        shard.stage_remote_layer(*layer, spikes, membrane_v.as_deref());
                    }
                    shard.stage_remote_output(&previous_output);
                }
                BoundaryExchange::LegacySpikeOverwrite => {
                    for (layer, spikes, _) in &frames {
                        if let Some(dst) = shard.last_spk_h[*layer].as_slice_mut()
                            && dst.len() == spikes.len()
                        {
                            dst.copy_from_slice(spikes);
                        }
                    }
                }
                BoundaryExchange::Isolated => {}
            }
            let out = shard.step(sensory);
            if i == self.output_owner {
                output = Some(out);
            }
        }
        output.expect("output owner stepped")
    }

    /// Newest committed state of every hidden layer not owned by shard `i`.
    #[allow(clippy::type_complexity)]
    fn remote_frames(&self, i: usize) -> Vec<(usize, Vec<i8>, Option<Vec<f64>>)> {
        let hidden = self.shards[i].net.num_hidden_layers;
        let mut frames = Vec::new();
        for (j, owner) in self.shards.iter().enumerate() {
            if j == i {
                continue;
            }
            for layer in self.ranges[j].clone().filter(|l| *l < hidden) {
                let spikes = owner.last_spk_h[layer].to_vec();
                let membrane_v = owner
                    .layer_boundary_carries_membrane(layer)
                    .then(|| owner.v_h[layer].to_vec());
                frames.push((layer, spikes, membrane_v));
            }
        }
        frames
    }
}

/// Little-endian `f64` wire form of a boundary layer's membrane voltages.
pub fn encode_membrane_v(v: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 8);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Decode [`encode_membrane_v`]; `None` when empty or not `width` values.
pub fn decode_membrane_v(bytes: &[u8], width: usize) -> Option<Vec<f64>> {
    if bytes.is_empty() || bytes.len() != width.checked_mul(8)? {
        return None;
    }
    Some(
        bytes
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().expect("8-byte chunk")))
            .collect(),
    )
}

/// Lock-step admission rule for a layer shard about to run the step that
/// starts at tick `t` (`Runner::t`) and publishes `step_index = t + 1`.
///
/// Every remote hidden layer must have delivered the state the single-node
/// kernel would read: an upstream layer (`< range.start`) its step `t + 1`,
/// a downstream layer (`>= range.end`) its step `t`, which at the shared
/// starting tick `origin` is the common initial state. `received(layer)` is
/// the newest step index received for that layer.
pub fn lockstep_boundary_ready(
    range: &Range<usize>,
    num_hidden_layers: usize,
    t: i64,
    origin: i64,
    received: impl Fn(usize) -> Option<i64>,
) -> bool {
    (0..num_hidden_layers)
        .filter(|layer| !range.contains(layer))
        .all(|layer| {
            let have = received(layer);
            if layer < range.start {
                have.is_some_and(|s| s > t)
            } else {
                t <= origin || have.is_some_and(|s| s >= t)
            }
        })
}

#[cfg(all(test, feature = "growth3d"))]
pub(crate) mod tests {
    use super::*;
    use crate::config::{ClumpingDesign, LIFParams, NetworkConfig, NeuronTypeConfig, STDPParams};
    use crate::sim::{Learning, NeuronModel};

    /// Deterministic pseudo-random sequence in [-1, 1).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Features {
        pub(crate) graded: bool,
        pub(crate) tonic: bool,
        pub(crate) gap: bool,
        pub(crate) graded_output: bool,
        pub(crate) biological_io: bool,
        pub(crate) delays: bool,
        pub(crate) stochastic: bool,
    }

    pub(crate) const PLAIN: Features = Features {
        graded: false,
        tonic: false,
        gap: false,
        graded_output: false,
        biological_io: false,
        delays: false,
        stochastic: false,
    };

    const LAYERS: usize = 4;
    const WIDTH: usize = 8;
    const SENSORY: usize = 6;
    const OUTPUTS: usize = 5;
    const SPIKING_BIAS: f64 = 5.0;

    /// A frozen, deterministic multi-layer network exercising the requested
    /// features at every shard boundary, exported as a snapshot.
    fn snapshot(f: Features, depth: usize) -> String {
        snapshot_with_layers(f, depth, LAYERS)
    }

    /// [`snapshot`] with `layers` hidden layers.
    pub(crate) fn snapshot_with_layers(f: Features, depth: usize, layers: usize) -> String {
        let mut net = NetworkConfig::default();
        net.clumping_design = ClumpingDesign::None;
        net.brain_regions = Vec::new();
        net.num_sensory_neurons = SENSORY;
        net.num_hidden_layers = layers;
        net.num_hidden_per_layer_initial = WIDTH;
        net.num_output_neurons = OUTPUTS;
        // A frozen circuit, as written by `scripts/aarnn_circuit.py` and the
        // connectome importers: no growth, morphology evolution or learning.
        net.growth_enabled = false;
        net.morpho_growth_enabled = false;
        net.use_morphology = f.delays;
        net.use_aarnn_delays = f.delays;
        net.thalamic_gating_enabled = false;
        net.aarnn_synaptic_energy_randomness = 0.0;
        net.perceptual_loop_enabled = false;
        net.aarnn_resonance_gain = 0.0;
        net.aarnn_ambient_energy_level = 0.0;
        net.theta_rhythm_enabled = false;
        net.aarnn_layer_depth = depth;
        net.sensory_target_layer = Some(0);
        net.output_source_layer = Some(layers - 1);
        net.io_channels_are_biological = f.biological_io;
        if f.stochastic {
            // Brain-wide stochastic and output-feedback drives.
            net.aarnn_synaptic_energy_randomness = 0.02;
            net.aarnn_resonance_gain = 0.2;
            net.thalamic_gating_enabled = true;
            net.perceptual_loop_enabled = true;
        }
        let mut r = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            net,
            NeuronModel::Aarnn,
            Learning::Aarnn,
        );
        r.set_dt(1.0);
        r.stdp.eta = 0.0;
        r.net.aarnn_plasticity_enabled = false;
        let mut spiking = r.net.aarnn_bio.clone();
        let mut graded = spiking.clone();
        // A sub-threshold bias keeps spiking cells near threshold at depth
        // >= 1 (adaptation and kinetic synapses) so spikes cross every
        // boundary; graded cells take theirs from `graded_tonic`.
        if depth > 0 {
            spiking.tonic_current = SPIKING_BIAS;
        }
        graded.graded_enabled = true;
        graded.graded_release_max_hz = 500.0;
        let mut tonic = spiking.clone();
        tonic.tonic_current = 12.0;
        let mut inhibited = spiking.clone();
        inhibited.tonic_current = -20.0;
        let mut graded_tonic = graded.clone();
        graded_tonic.tonic_current = 1.5;
        r.net.neuron_types = vec![
            NeuronTypeConfig {
                name: "Spiking".into(),
                bio_params: spiking,
            },
            NeuronTypeConfig {
                name: "Graded".into(),
                bio_params: graded.clone(),
            },
            NeuronTypeConfig {
                name: "Tonic".into(),
                bio_params: tonic,
            },
            NeuronTypeConfig {
                name: "GradedTonic".into(),
                bio_params: graded_tonic,
            },
            NeuronTypeConfig {
                name: "Inhibited".into(),
                bio_params: inhibited,
            },
        ];
        for (l, layer) in r.topo.layers.iter_mut().enumerate() {
            for (j, node) in layer.iter_mut().enumerate() {
                let kind = match (j % 4, f.graded, f.tonic) {
                    (0, true, true) => "GradedTonic",
                    (0 | 1, true, _) => "Graded",
                    (2, _, true) => "Tonic",
                    (3, _, true) => "Inhibited",
                    _ => "Spiking",
                };
                // Keep one purely spiking cell per layer.
                node.type_name = Some(if j == WIDTH - 1 { "Spiking" } else { kind }.into());
                let _ = l;
            }
        }
        for node in r.topo.output_nodes.iter_mut() {
            node.type_name = Some(
                if f.graded_output {
                    "GradedTonic"
                } else {
                    "Spiking"
                }
                .into(),
            );
        }
        let mut rng = Lcg(0x5eed_0000 + depth as u64);
        // Kinetic synapses (depth >= 1) deliver less charge per spike.
        let k = if depth == 0 { 1.0 } else { 15.0 };
        let mut fill = |m: &mut ndarray::Array2<f64>, density: f64, gain: f64| {
            for w in m.iter_mut() {
                let keep = rng.next().abs() < density;
                let value = rng.next();
                *w = if keep {
                    gain * (0.25 + value.abs()) * value.signum()
                } else {
                    0.0
                };
            }
        };
        fill(&mut r.w_in, 0.7, 9.0 * k);
        for m in r.w_hh_fwd.iter_mut() {
            fill(m, 0.6, 8.0 * k);
        }
        for m in r.w_hh_bwd.iter_mut() {
            fill(m, 0.4, 3.0 * k);
        }
        for m in r.w_hh_rec.iter_mut() {
            fill(m, 0.3, 3.0 * k);
        }
        fill(&mut r.w_out, 0.7, 8.0 * k);
        // Excitatory-biased so activity crosses every boundary.
        for m in r.w_hh_fwd.iter_mut() {
            m.mapv_inplace(|w| if w < 0.0 { 0.3 * w } else { w });
        }
        r.w_in.mapv_inplace(f64::abs);
        if f.gap {
            r.w_gap = (0..layers)
                .map(|_| {
                    let mut g = ndarray::Array2::<f64>::zeros((WIDTH, WIDTH));
                    for j in 0..WIDTH {
                        let k = (j + 1) % WIDTH;
                        g[(j, k)] = 0.4;
                        g[(k, j)] = 0.4;
                    }
                    g
                })
                .collect();
        }
        r.sync_bio_from_topo();
        r.export_network_json().expect("export test network")
    }

    pub(crate) fn runner_from(json: &str) -> Runner {
        let mut r = Runner::new(
            LIFParams::default(),
            STDPParams::default(),
            NetworkConfig::default(),
            NeuronModel::Aarnn,
            Learning::Aarnn,
        );
        r.import_network_json(json).expect("import test network");
        r.set_dt(1.0);
        r.stdp.eta = 0.0;
        r
    }

    pub(crate) fn sensory_frame(step: usize) -> Vec<i8> {
        (0..SENSORY)
            .map(|i| i8::from((step * 7 + i * 3) % 5 < 2 || (step / 40 + i) % 6 == 0))
            .collect()
    }

    #[derive(Debug, Default)]
    struct Divergence {
        history_frames: usize,
        first_spike_mismatch: Option<usize>,
        spike_mismatches: usize,
        output_mismatches: usize,
        max_v_error: f64,
        max_v_o_error: f64,
        hidden_spikes: Vec<usize>,
        output_events: usize,
        graded_release: Vec<f64>,
    }

    impl Divergence {
        /// Every hidden layer spiked or released transmitter.
        fn all_layers_active(&self) -> bool {
            self.hidden_spikes
                .iter()
                .zip(&self.graded_release)
                .all(|(s, r)| *s > 0 || *r > 0.0)
        }

        fn exact(&self) -> bool {
            self.spike_mismatches == 0
                && self.output_mismatches == 0
                && self.max_v_error == 0.0
                && self.max_v_o_error == 0.0
        }
    }

    fn compare(
        f: Features,
        depth: usize,
        shards: usize,
        exchange: BoundaryExchange,
        steps: usize,
    ) -> Divergence {
        let json = snapshot(f, depth);
        let mut single = runner_from(&json);
        let layers = execution_layer_count(&single);
        let ranges = even_layer_ranges(layers, shards);
        let runners = ranges.iter().map(|_| runner_from(&json)).collect();
        let mut group = LayerShardGroup::new(runners, ranges, exchange).expect("valid shards");
        let hidden = single.net.num_hidden_layers;
        let mut d = Divergence {
            hidden_spikes: vec![0; hidden],
            graded_release: vec![0.0; hidden],
            ..Default::default()
        };
        for step in 0..steps {
            let s = sensory_frame(step);
            let a = single.step(Some(&s));
            let b = group.step(Some(&s));
            let mut mismatch = a.spk_o != b.spk_o;
            if mismatch {
                d.output_mismatches += 1;
            }
            d.output_events += a.spk_o.iter().filter(|x| **x != 0).count();
            for l in 0..single.net.num_hidden_layers {
                let owner = &group.shards()[group.layer_owner(l)];
                if single.last_spk_h[l] != owner.last_spk_h[l] {
                    d.spike_mismatches += 1;
                    mismatch = true;
                }
                d.hidden_spikes[l] += single.last_spk_h[l].iter().filter(|x| **x != 0).count();
                for (x, y) in single.v_h[l].iter().zip(owner.v_h[l].iter()) {
                    d.max_v_error = d.max_v_error.max((x - y).abs());
                }
                d.graded_release[l] += single
                    .graded_release_h(l)
                    .into_iter()
                    .flatten()
                    .sum::<f64>();
            }
            for (x, y) in single.v_o.iter().zip(group.output_runner().v_o.iter()) {
                d.max_v_o_error = d.max_v_o_error.max((x - y).abs());
            }
            if mismatch && d.first_spike_mismatch.is_none() {
                d.first_spike_mismatch = Some(step);
            }
        }
        assert_eq!(
            single.output_code(),
            group.output_runner().output_code(),
            "output code"
        );
        d.history_frames = single.hist_len;
        d
    }

    pub(crate) const CASES: [(&str, Features); 8] = [
        ("plain spiking", PLAIN),
        (
            "graded cells across boundaries",
            Features {
                graded: true,
                ..PLAIN
            },
        ),
        (
            "tonic currents",
            Features {
                tonic: true,
                ..PLAIN
            },
        ),
        (
            "gap junctions at boundary layers",
            Features { gap: true, ..PLAIN },
        ),
        (
            "graded pulse-density output",
            Features {
                graded: true,
                tonic: true,
                graded_output: true,
                ..PLAIN
            },
        ),
        (
            "biological I/O output-stage shard",
            Features {
                graded: true,
                biological_io: true,
                ..PLAIN
            },
        ),
        (
            "morphology axonal delays",
            Features {
                delays: true,
                graded: true,
                ..PLAIN
            },
        ),
        (
            "stochastic drives and perceptual output feedback",
            Features {
                stochastic: true,
                graded: true,
                ..PLAIN
            },
        ),
    ];

    /// Staged boundary admission reproduces single-node execution exactly
    /// (bit-identical spikes and membranes) for two, three and one-per-layer
    /// shards with graded cells, tonic currents, gap junctions, axonal
    /// delays, stochastic drives, a pulse-density output code and a
    /// biological-I/O output stage. Each case is active in every layer and
    /// diverges without the boundary.
    fn assert_staged_equivalence(depth: usize) {
        for (name, f) in CASES {
            let layers = LAYERS + usize::from(f.biological_io);
            for shards in [2usize, 3, layers] {
                let d = compare(f, depth, shards, BoundaryExchange::Staged, 200);
                assert!(
                    d.exact(),
                    "{name}, depth {depth}, {shards} shards diverged: {d:?}"
                );
                assert!(
                    d.all_layers_active(),
                    "{name}, depth {depth}: a layer is silent, test is vacuous: {d:?}"
                );
                assert!(
                    !f.delays || d.history_frames > 1,
                    "{name}: no axonal delay history, test is vacuous: {d:?}"
                );
            }
            let isolated = compare(f, depth, 2, BoundaryExchange::Isolated, 200);
            assert!(
                !isolated.exact(),
                "{name}, depth {depth}: shards do not depend on the boundary"
            );
        }
    }

    /// Instantaneous synaptic charge (depth 0).
    #[test]
    fn layer_shards_reproduce_single_node_execution_depth0() {
        assert_staged_equivalence(0);
    }

    /// Kinetic synapses, STP, adaptive thresholds and homeostasis (depth 3,
    /// the depth of the designed reflex circuits).
    #[test]
    fn layer_shards_reproduce_single_node_execution_depth3() {
        assert_staged_equivalence(3);
    }

    /// Quantify the transport before staged admission (spikes overwritten,
    /// no voltages/history/output state) against single-node execution.
    /// Graded release cannot cross that boundary, so those networks diverge.
    #[test]
    fn legacy_spike_overwrite_loses_graded_boundary_state() {
        let mut report = Vec::new();
        for (name, f) in CASES {
            for shards in [2usize, 3] {
                let d = compare(f, 3, shards, BoundaryExchange::LegacySpikeOverwrite, 200);
                report.push(format!(
                    "{name} x{shards}: exact={} first_mismatch={:?} hidden_mismatch_steps={} output_mismatch_steps={} max|dv|={:.3e} max|dv_o|={:.3e}",
                    d.exact(),
                    d.first_spike_mismatch,
                    d.spike_mismatches,
                    d.output_mismatches,
                    d.max_v_error,
                    d.max_v_o_error
                ));
                if f.graded {
                    assert!(!d.exact(), "{name}: graded boundary unexpectedly exact");
                }
            }
        }
        eprintln!("legacy boundary exchange:\n  {}", report.join("\n  "));
    }

    #[test]
    fn staged_output_code_matches_single_node() {
        let f = Features {
            graded: true,
            graded_output: true,
            ..PLAIN
        };
        let json = snapshot(f, 3);
        let single = runner_from(&json);
        assert_eq!(single.output_code(), Some("pulse_density"));
        let ranges = even_layer_ranges(LAYERS, 2);
        let runners = ranges.iter().map(|_| runner_from(&json)).collect();
        let group = LayerShardGroup::new(runners, ranges, BoundaryExchange::Staged).unwrap();
        for shard in group.shards() {
            assert_eq!(shard.output_code(), Some("pulse_density"));
        }
        assert!(group.output_runner().owns_output_stage());
    }

    #[test]
    fn even_ranges_are_contiguous() {
        assert_eq!(even_layer_ranges(4, 3), vec![0..2, 2..3, 3..4]);
        assert_eq!(even_layer_ranges(1, 3), vec![0..1]);
        assert_eq!(even_layer_ranges(5, 5).len(), 5);
    }

    #[test]
    fn membrane_wire_round_trip() {
        let v = [-70.0, -55.25, f64::MIN_POSITIVE, 12.5];
        assert_eq!(decode_membrane_v(&encode_membrane_v(&v), 4).unwrap(), v);
        assert!(decode_membrane_v(&encode_membrane_v(&v), 3).is_none());
        assert!(decode_membrane_v(&[], 0).is_none());
    }

    #[test]
    fn lockstep_rule_orders_upstream_and_downstream() {
        let steps = |table: &'static [(usize, i64)]| {
            move |layer: usize| table.iter().find(|(l, _)| *l == layer).map(|(_, s)| *s)
        };
        // Middle shard owning layer 1 of 0..3, about to run tick 5.
        let range = 1..2;
        assert!(!lockstep_boundary_ready(
            &range,
            3,
            5,
            0,
            steps(&[(0, 5), (2, 5)])
        ));
        assert!(lockstep_boundary_ready(
            &range,
            3,
            5,
            0,
            steps(&[(0, 6), (2, 5)])
        ));
        assert!(!lockstep_boundary_ready(
            &range,
            3,
            5,
            0,
            steps(&[(0, 6), (2, 4)])
        ));
        // At the shared origin a downstream layer needs nothing yet.
        assert!(lockstep_boundary_ready(&range, 3, 0, 0, steps(&[(0, 1)])));
        assert!(!lockstep_boundary_ready(&range, 3, 0, 0, steps(&[])));
        // The first shard only waits on downstream layers.
        assert!(lockstep_boundary_ready(&(0..1), 3, 0, 0, steps(&[])));
        assert!(!lockstep_boundary_ready(&(0..1), 3, 1, 0, steps(&[(2, 1)])));
        assert!(lockstep_boundary_ready(
            &(0..1),
            3,
            1,
            0,
            steps(&[(1, 1), (2, 1)])
        ));
    }
}
