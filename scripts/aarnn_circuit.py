#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Generic builder for designed AARNN circuits (snapshot + run config).

The C. elegans work established a common recipe for embodied locomotion:

1. graded (non-spiking) motor neurons and motor outputs with a conductance
   input, optionally bistable (plateau);
2. motor outputs sent as a pulse-density code (``output_code`` =
   ``pulse_density``) that the body decodes linearly (muscle activation, or a
   joint position over the full joint range);
3. antagonistic half-centres with reciprocal inhibition, switched by
   proprioception rather than by a central oscillator (Boyle, Berri & Cohen
   2012; Cruse's walking rules; Ekeberg 2004);
4. a tonic command drive (``tonic_current``) setting the behavioural state;
5. an operating point inside each cell's dynamic range, no common-mode
   coupling, and no plasticity, spontaneous drives or Dale rewiring, so the
   designed wiring is what runs at the start. Structural growth is on except
   for C. elegans (user, 24 Nov; ``Circuit.growth`` overrides).

``Circuit`` collects cells, per-type biology, signed synapses, electrical
synapses, sensory projections and outputs, and writes a snapshot in the
runtime's import format (one hidden layer, dense matrices, no morphology).
"""
from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Dict, List, Optional, Tuple

ROOT = Path(__file__).resolve().parents[1]

# Defaults mirror AarnnBioParams (src/config.rs) for the graded/tonic fields.
GRADED_OFF = {
    "graded_enabled": False, "graded_tau_ms": 30.0, "graded_v_rest_mv": -70.0,
    "graded_input_resistance": 5.0, "graded_release_v_half_mv": -50.0,
    "graded_release_slope_mv": 5.0, "graded_release_max_hz": 50.0,
    "graded_plateau_mv": 0.0, "graded_plateau_v_half_mv": -55.0,
    "graded_plateau_slope_mv": 3.0, "graded_e_exc_mv": 0.0, "graded_e_inh_mv": -80.0,
    "tonic_current": 0.0,
}


def graded(tau_ms: float = 30.0, release_hz: float = 20.0, plateau_mv: float = 0.0,
           resistance: float = 5.0, tonic: float = 0.0) -> dict:
    """Bio overrides of a graded cell type."""
    return {"graded_enabled": True, "graded_tau_ms": tau_ms, "graded_release_max_hz": release_hz,
            "graded_plateau_mv": plateau_mv, "graded_input_resistance": resistance,
            "tonic_current": tonic}


# Synaptic drive calibration shared by designed circuits. With NMDA off
# (``nmda_ratio`` 0) the runtime's AMPA/GABA kernels are first-order filters
# (tau 5 / 10 ms at dt 1 ms), whose step sums are these constants. A graded
# presynaptic cell at full release delivers ``release_hz`` spike-equivalents
# per second; a sensor value of 1.0 is one event per 2 ms frame (500 Hz).
KERNEL_EXC = 5.52
KERNEL_INH = 10.51
RELEASE_HZ = 20.0
SENSOR_HZ = 500.0


# Measured in the runtime with the template's active dendrites on (the
# default; see ``Circuit(exact_cells=...)``): excitatory paths deliver about
# 1.373 x the nominal current (the dendritic plateau gain at saturation, not
# a kernel constant), inhibitory ones ~0.9-1.0 x. ``exact=True`` applies
# these factors for dendrite-active circuits; with ``exact_cells=True`` the
# nominal weights are already exact and ``exact`` must stay False.
EXC_EFFECTIVE = 1.373
INH_EFFECTIVE = 0.9


# Newborn neurons (growth) start immature: a graded interneuron with no tonic
# drive, releasing ~0.4 Hz at rest (v_half -50 mV, slope 5 mV), so it joins
# the circuit nearly silent and gains influence only through experience.
NEWBORN_TYPE = "Growth:newborn"


def newborn_type(base: dict) -> dict:
    return {"name": NEWBORN_TYPE, "bio_params": {**base, **graded(tau_ms=30.0, release_hz=20.0),
                                                 "graded_release_v_half_mv": -50.0, "graded_release_slope_mv": 5.0}}


def drive_weight(mv: float, gain: float, release_hz: float = RELEASE_HZ, exact: bool = False) -> float:
    """Weight giving ``mv`` mV/ms of input current at full presynaptic release
    onto a cell type of ``synaptic_gain`` ``gain`` (nominal units unless
    ``exact``)."""
    k = KERNEL_EXC if mv >= 0 else KERNEL_INH
    if exact:
        k *= EXC_EFFECTIVE if mv >= 0 else INH_EFFECTIVE
    return mv / (release_hz / 1000.0 * k * gain)


def sensor_weight(mv: float, gain: float, exact: bool = False) -> float:
    """Weight giving ``mv`` mV/ms at sensor value 1.0 onto a type of ``gain``."""
    k = KERNEL_EXC if mv >= 0 else KERNEL_INH
    if exact:
        k *= EXC_EFFECTIVE if mv >= 0 else INH_EFFECTIVE
    return mv / (SENSOR_HZ / 1000.0 * k * gain)


def release_tonic(target: float, resistance: float = 5.0) -> float:
    """Tonic current (mV/ms) holding a graded cell at release ``target``
    (defaults: rest -70, half -50, slope 5, reversal 0 mV)."""
    base = 1.0 / (1.0 + math.exp(-(-70.0 + 50.0) / 5.0))
    s = base + target * (1.0 - base)
    v = -50.0 + 5.0 * math.log(s / (1.0 - s))
    g = (v + 70.0) / (0.0 - v)
    return g * 70.0 / resistance


class Circuit:
    def __init__(self, profile: str, sensory: List[str], outputs: List[str], exact_cells: bool = False):
        self.profile = profile
        self.growth: Optional[bool] = None     # None: on unless C. elegans (see _net)
        # Newborn synapse strength (x the runtime's 0.05-0.25 draw). 0.0025
        # gives ~0.1 mV/ms per sensory synapse at sensor value 1.0 and
        # ~0.004 mV/ms per synapse from a cell at full 20 Hz release: newborns
        # join silent (designed relays saturate at ~20 mV/ms). At 0.05 (2 mV/ms
        # per sensory synapse) closed-loop hexapod newborns saturated at
        # 20 Hz release, spawned in a cascade to the neuron cap and turned the
        # walk backwards (24 Nov).
        self.newborn_scale = 0.0025
        self.growth_placement = "high"         # new neurons at the highest-energy free location (or "low", "mid")
        # Morphological growth (neurite growth, contact synaptogenesis,
        # synapse migration, soma movement) is off for designed circuits until
        # it preserves their function: on the Freenove hexapod CPG it stalled
        # the walk in three closed-loop tests (0.48 m vs 1.10 m in 120 s) -
        # with full-strength contacts, with silent contacts and no migration,
        # and with contacts no longer overwriting existing synapses - while
        # neurogenesis alone did no harm (1.03-1.07 m; 24 Nov).
        self.morpho_growth = False
        # exact_cells: switch off the runtime's unmodelled cell nonlinearities
        # inherited from the template (active dendritic plateau: positive net
        # synaptic current x up to 1.4 with ~120-350 ms calcium state; adaptive
        # thresholds; network-wide short-term plasticity), so the cells follow
        # the graded equations the design assumes. Designs tuned before this
        # option (fish, fly, hexapod, archetype explorer) keep them on.
        self.exact_cells = exact_cells
        self.sensory = list(sensory)
        self.outputs = list(outputs)
        self.cells: List[Tuple[str, str, str, Tuple[float, float, float]]] = []
        self.index: Dict[str, int] = {}
        self.types: Dict[str, dict] = {}
        self.output_type: Dict[str, str] = {}
        self.syn: Dict[Tuple[str, str], float] = {}
        self.sense: Dict[Tuple[str, str], float] = {}
        self.gaps: Dict[Tuple[str, str], float] = {}
        # Sensory transmission is tonic: short-term depression at sensor
        # rates (up to 500 events/s) would erase proprioceptive signals, as
        # it did for the worm's bend channels.
        # STP is gated network-wide; per type only its parameters apply, so
        # U = 1 with instant recovery makes it inert (as for the worm's
        # proprioceptors).
        self.type("Sensory:tonic", stp_enabled=False, stp_u=1.0, stp_tau_rec_ms=0.001, stp_tau_facil_ms=0.001)

    @classmethod
    def for_embodiment(cls, profile: str, exact_cells: bool = False) -> "Circuit":
        """Circuit over a generated embodiment's ordered IO. Channels are named
        physically in the builder; the snapshot labels carry the neural names
        the binding pins (imported connectome IDs for the flies)."""
        io = json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]
        c = cls(profile, [s["name"] for s in io["sensors"]], list(io["output_names"]), exact_cells=exact_cells)
        c.sensory_modalities = [sensory_modality(s) for s in io["sensors"]]
        c.neural_sensory = io.get("neural_sensory_names")
        c.neural_outputs = io.get("neural_output_names")
        return c

    def type(self, name: str, **bio) -> str:
        self.types[name] = bio
        return name

    def cell(self, name: str, type_name: str, region: str = "core",
             pos: Tuple[float, float, float] = (0.0, 0.0, 0.0)) -> str:
        if name in self.index:
            raise ValueError(f"duplicate cell {name}")
        if type_name not in self.types:
            raise ValueError(f"unknown type {type_name}")
        self.index[name] = len(self.cells)
        self.cells.append((name, type_name, region, pos))
        return name

    def output(self, name: str, type_name: str) -> None:
        if name not in self.outputs:
            raise ValueError(f"unknown output {name}")
        self.output_type[name] = type_name

    def connect(self, pre: str, post: str, w: float) -> None:
        if pre not in self.index:
            raise ValueError(f"unknown presynaptic cell {pre}")
        if post not in self.index and post not in self.outputs:
            raise ValueError(f"unknown target {post}")
        self.syn[(pre, post)] = self.syn.get((pre, post), 0.0) + w

    def sensor(self, channel: str, post: str, w: float) -> None:
        if channel not in self.sensory:
            raise ValueError(f"unknown sensory channel {channel}")
        if post not in self.index:
            raise ValueError(f"unknown target {post}")
        self.sense[(channel, post)] = self.sense.get((channel, post), 0.0) + w

    def gap(self, a: str, b: str, g: float) -> None:
        key = tuple(sorted((a, b)))
        self.gaps[key] = self.gaps.get(key, 0.0) + g

    def _net(self, template: dict, synaptic_gain: float, depth: int, extra: dict) -> dict:
        net = dict(template.get("net", template))
        n = len(self.cells)
        # Structural growth (neurogenesis at energy peaks: a saturated cell
        # spawns a neighbour placed by the local activity-energy field, and
        # new cells connect with the template's new-edge probability) is on
        # for every body except C. elegans, whose network failed with more
        # than its 302 neurons (user, 24 Nov). It stays on until a body's own
        # tests show extra neurons do more harm than good; ``self.growth``
        # overrides for such comparisons.
        growth = self.growth if self.growth is not None else self.profile not in ("celegans", "c_elegans")
        edge_p = float(net.get("new_edge_prob", 0.05) or 0.05) if growth else 0.0
        # Newborns integrate the way new neurons do: nearly silent synapses
        # (growth_new_synapse_scale), the designed cells keep their own
        # synapses (no migration: splitting a threshold-tuned cell's inputs
        # and outputs with a daughter is not function-preserving), and a
        # sparse chance of input/output contacts. With the template's
        # full-strength, p = 1 wiring, 42 newborns saturated every motor
        # output of the hexapod CPG in 3 s (24 Nov).
        newborn = dict(growth_new_synapse_scale=self.newborn_scale, migrate_in_prob=0.0, migrate_out_prob=0.0,
                       p_in=0.25, p_out=0.25, growth_placement_mode=self.growth_placement,
                       growth_io_floor_enabled=False, growth_newborn_type=NEWBORN_TYPE,
                       # Morphological growth keeps the designed wiring: contacts
                       # made by growing neurites start as silent as newborn
                       # synapses (the template's 1.0 is ~11 mV/ms from a 20 Hz
                       # cell; 3417 such contacts stalled the hexapod walk), and
                       # existing synapses never migrate to "closer" targets.
                       initial_synaptic_weight=0.15 * self.newborn_scale,
                       morpho_synapse_migration_enabled=False) if growth else {}
        net.update(
            num_sensory_neurons=len(self.sensory), num_hidden_layers=1,
            num_hidden_per_layer_initial=n, num_output_neurons=len(self.outputs),
            io_channels_are_biological=False, sensory_target_layer=0, output_source_layer=0,
            max_total_neurons=max(n + len(self.outputs), int(net.get("max_total_neurons", 0) or 0)),
            growth_enabled=growth, morpho_growth_enabled=self.morpho_growth, use_morphology=False, use_aarnn_delays=False,
            aarnn_layer_depth=depth, max_layers=1, new_edge_prob=edge_p,
            development_stage_mode="manual", development_pruning_interval_ms=1.0e12,
            sleep_enabled=False, world_model_enabled=False, theta_rhythm_enabled=False,
            thalamic_gating_enabled=False, aarnn_dale_strictness=0.0,
            aarnn_gap_junction_strength=0.0, volume_transmission_enabled=False,
            aarnn_synaptic_scaling_strength=0.0, aarnn_myelination_enabled=False,
            aarnn_import_topology_rewire_enabled=False, aarnn_import_topology_rewire_keep_fraction=1.0,
            aarnn_plasticity_enabled=False, aarnn_output_gain_floor_enabled=False,
            aarnn_synaptic_energy_randomness=0.0, perceptual_loop_enabled=False,
            aarnn_resonance_gain=0.0, aarnn_ambient_energy_level=0.0,
            clumping_design="None", brain_regions=[],
        )
        net.update(newborn)
        spike_io = dict(net.get("spike_io") or {})
        # The runtime's spike-I/O profile enum knows the legacy bodies only;
        # every other embodiment uses the generic channel layout.
        known = {"celegans", "c_elegans", "drosophila", "hexapod", "nao", "zebrafish"}
        spike_io.update(profile=self.profile if self.profile in known else "generic",
                        input_strategy="threshold", threshold=0.5)
        net["spike_io"] = spike_io
        base = {**dict(net.get("aarnn_bio") or {}), **GRADED_OFF}
        base["synaptic_gain"] = synaptic_gain
        if self.exact_cells:
            base.update(dendritic_active_enabled=False, adaptive_threshold_enabled=False, stp_enabled=False)
        net["aarnn_bio"] = base
        net["neuron_types"] = [{"name": t, "bio_params": {**base, **bio}} for t, bio in sorted(self.types.items())]
        # What each sensory channel reports, for the runtime's neuromodulator
        # drive (food -> DA/5-HT, light -> 5-HT, heat/noxious -> NA, energy
        # reserve -> 5-HT); from the IO contract's kind and mode when known.
        net["aarnn_sensory_modalities"] = list(getattr(self, "sensory_modalities", None)
                                               or [sensory_modality({"name": n}) for n in self.sensory])
        if growth and NEWBORN_TYPE not in self.types:
            net["neuron_types"].append(newborn_type(base))
        net.update(extra)
        return net

    def write(self, snapshot: Path, config: Optional[Path] = None, synaptic_gain: float = 1.0,
              depth: int = 0, extra_net: Optional[dict] = None, labels: Optional[dict] = None) -> dict:
        template = json.loads((ROOT / "network.json").read_text())
        net = self._net(template, synaptic_gain, depth, extra_net or {})
        n, s, o = len(self.cells), len(self.sensory), len(self.outputs)
        missing = [x for x in self.outputs if x not in self.output_type]
        if missing:
            raise ValueError(f"outputs without a type: {missing[:4]}")

        def dense(rows: int, cols: int) -> List[List[float]]:
            return [[0.0] * cols for _ in range(rows)]
        w_in, w_rec, w_out, w_gap = dense(n, s), dense(n, n), dense(o, n), dense(n, n)
        for (ch, post), w in self.sense.items():
            w_in[self.index[post]][self.sensory.index(ch)] = w
        for (pre, post), w in self.syn.items():
            if post in self.index:
                w_rec[self.index[post]][self.index[pre]] = w
            else:
                w_out[self.outputs.index(post)][self.index[pre]] = w
        for (a, b), g in self.gaps.items():
            w_gap[self.index[a]][self.index[b]] = w_gap[self.index[b]][self.index[a]] = g

        def mat(m, rows, cols, u32=False):
            data = [x for r in m for x in r]
            if u32:
                data = [1 if x != 0 else 0 for x in data]
            return {"rows": rows, "cols": cols, "data": data}
        layer = [{"x": p[0], "y": p[1], "z": p[2], "layer": 0, "region_name": r, "type_name": t}
                 for (_, t, r, p) in self.cells]
        sens_nodes = [{"x": 0.0, "y": 0.0, "z": 0.0, "layer": 0, "region_name": "sensory", "type_name": "Sensory:tonic"}
                      for _ in self.sensory]
        out_nodes = [{"x": 0.0, "y": 0.0, "z": 0.0, "layer": 0, "region_name": "motor",
                      "type_name": self.output_type[x]} for x in self.outputs]
        snap = {
            "net": net,
            "topo": {"layers": [layer], "sensory_nodes": sens_nodes, "output_nodes": out_nodes},
            "w_in": mat(w_in, n, s), "w_hh_fwd": [], "w_hh_bwd": [], "w_hh_rec": [mat(w_rec, n, n)],
            "w_out": mat(w_out, o, n), "w_gap": [mat(w_gap, n, n)] if self.gaps else [],
            "p_in": mat(w_in, n, s, True), "p_fwd": [], "p_bwd": [], "p_rec": [mat(w_rec, n, n, True)],
            "p_out": mat(w_out, o, n, True), "layer_range": None, "rng_seed": 7,
            "connectome_labels": {
                "schema": "aarnn-designed-circuit-v1", "profile": self.profile,
                "sensory_nodes": getattr(self, "neural_sensory", None) or self.sensory,
                "hidden_nodes": [c[0] for c in self.cells],
                "output_nodes": getattr(self, "neural_outputs", None) or self.outputs,
                "physical_sensory_nodes": self.sensory, "physical_output_nodes": self.outputs, **(labels or {}),
            },
        }
        snapshot.write_text(json.dumps(snap))
        if config is not None:
            config.write_text(json.dumps(net, indent=1))
        return snap


# --- Design-time analysis of graded cells (mirrors src/neuron_kernels.rs) ---

def _logistic(x: float) -> float:
    return 1.0 / (1.0 + math.exp(-max(-60.0, min(60.0, x))))


def steady_release(current: float, plateau_mv: float = 0.0, start_mv: float = -70.0,
                   resistance: float = 5.0) -> float:
    """Steady-state release of a graded cell (default biology) under a
    constant net input ``current`` (mV/ms), relaxing from ``start_mv``: for a
    bistable cell the start selects the branch (rest = lower, high = upper)."""
    v_rest, e = -70.0, (0.0 if current >= 0 else -80.0)
    g = resistance * abs(current) / (e - v_rest if current >= 0 else v_rest - (-80.0))
    base_p = _logistic((v_rest + 55.0) / 3.0)
    v = start_mv
    for _ in range(4000):  # fixed point of tau dV/dt = 0 by damped iteration
        p = plateau_mv * (_logistic((v + 55.0) / 3.0) - base_p) if plateau_mv > 0 else 0.0
        target = (v_rest + p + g * e) / (1.0 + g)
        v += 0.2 * (target - v)
    base = _logistic(-4.0)
    return max(0.0, min(1.0, (_logistic((v + 50.0) / 5.0) - base) / (1.0 - base)))


def bistable_band(plateau_mv: float) -> Tuple[float, float]:
    """Input currents (lower, upper knee) between which a plateau cell is
    bistable: above ``upper`` it switches on from rest, below ``lower`` it
    falls back from the upper branch (e.g. 22 mV -> about 0.46, 1.56)."""
    grid = [i * 0.01 for i in range(0, 1001)]
    up = next(x for x in grid if steady_release(x, plateau_mv, -70.0) > 0.3)
    down = next(x for x in grid if steady_release(x, plateau_mv, -20.0) > 0.3)
    return down, up


# --- Shared hindbrain locomotor module ---

def add_hindbrain(c: "Circuit", gain: float, plateau_mv: float = 22.0, mlr_release: float = 0.5,
                  rs_plateau_mv: Optional[float] = None,
                  rs_mutual: float = 6.0, artr_drive: float = 2.0, artr_tau_a_ms: float = 5000.0,
                  artr_w_ea: float = 10.0, artr_w_ae: float = 9.0, artr_mutual: float = 4.0,
                  artr_rs: float = 1.0, region: str = "hindbrain",
                  pos: Tuple[float, float, float] = (0.0, 0.0, 0.0)) -> Dict[str, object]:
    """Command, steering and exploration cells common to every locomotor.

    * ``MLR`` - tonic locomotor command (release ``mlr_release``): forward drive;
    * ``RS_l``/``RS_r`` - bistable reticulospinal cells with mutual inhibition:
      a winner-take-all turn decision whose hysteresis carries a turn through.
      By convention ``RS_l`` responds to obstacles on the left and turns the
      body to the right (away), ``RS_r`` the reverse;
    * ``ARTR_l``/``ARTR_r`` (+ slow adaptation cells) - a half-centre
      oscillator alternating every ~10-20 s (larval zebrafish ARTR, Dunn et
      al. 2016). ``ARTR_s`` biases ``RS_s`` below threshold (picks the side
      of a head-on turn); callers add a gentle direct turning bias from it.

    Returns the cell names; the body-specific layer connects them.
    """
    def w(mv):
        return drive_weight(mv, gain)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    c.type("Command:MLR", **graded(tau_ms=20.0, tonic=release_tonic(mlr_release)), **common)
    c.type("Hindbrain:RS", **graded(tau_ms=20.0, plateau_mv=plateau_mv if rs_plateau_mv is None else rs_plateau_mv), **common)
    c.type("Hindbrain:ARTR", **graded(tau_ms=50.0, plateau_mv=plateau_mv, tonic=artr_drive), **common)
    # Deterministic asymmetry: the left ARTR population leads.
    c.type("Hindbrain:ARTR_lead", **graded(tau_ms=50.0, plateau_mv=plateau_mv, tonic=artr_drive + 0.3), **common)
    c.type("Hindbrain:ARTR_adapt", **graded(tau_ms=artr_tau_a_ms), **common)
    x, y, z = pos
    mlr = c.cell("MLR", "Command:MLR", region, (x, y, z))
    rs = {s: c.cell(f"RS_{s}", "Hindbrain:RS", region, (x - 0.005, y + (0.003 if s == "l" else -0.003), z)) for s in "lr"}
    artr = {s: c.cell(f"ARTR_{s}", "Hindbrain:ARTR_lead" if s == "l" else "Hindbrain:ARTR", region,
                      (x - 0.002, y + (0.002 if s == "l" else -0.002), z + 0.001)) for s in "lr"}
    artr_a = {s: c.cell(f"ARTR_A_{s}", "Hindbrain:ARTR_adapt", region,
                        (x - 0.002, y + (0.002 if s == "l" else -0.002), z - 0.001)) for s in "lr"}
    for s, o in (("l", "r"), ("r", "l")):
        c.connect(artr[s], artr_a[s], w(artr_w_ea))
        c.connect(artr_a[s], artr[s], w(-artr_w_ae))
        c.connect(artr[s], artr[o], w(-artr_mutual))
        c.connect(artr[s], rs[s], w(artr_rs))
        c.connect(rs[s], rs[o], w(-rs_mutual))
    return {"mlr": mlr, "rs": rs, "artr": artr, "artr_adapt": artr_a, "mlr_release": mlr_release}


def add_turn_bouts(c: "Circuit", gain: float, hb: Dict[str, object], on_ms: float = 1500.0,
                   off_ms: float = 3000.0, kick_mv: float = 1.2, drive_mv: float = 4.0,
                   plateau_mv: float = 30.0, region: str = "hindbrain") -> Dict[str, str]:
    """Spontaneous turn bouts (run-and-turn exploration), zebrafish-style:
    a bout clock times the turns, the ARTR picks their side.

    ``BOUT_on``/``BOUT_off`` is an escape half-centre (each side terminated
    by its own adaptation, ``on_ms``/``off_ms``); ``BOUT_on`` adds
    ``kick_mv`` to both ``RS`` cells, ARTR's standing bias adds to one, and
    only their sum crosses the bistable band, so a turn bout fires on the
    ARTR side while the clock is on (larval zebrafish: ARTR sets the
    direction of chains of turn bouts, Dunn et al. 2016). Between bouts the
    body runs straight; a body blocked by something its senses miss turns
    away within a few bouts.
    """
    def w(mv):
        return drive_weight(mv, gain)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    c.type("Bout:E", **graded(tau_ms=50.0, plateau_mv=plateau_mv, tonic=drive_mv), **common)
    c.type("Bout:E_lead", **graded(tau_ms=50.0, plateau_mv=plateau_mv, tonic=drive_mv + 0.3), **common)
    c.type("Bout:A_on", **graded(tau_ms=on_ms), **common)
    c.type("Bout:A_off", **graded(tau_ms=off_ms), **common)
    e = {"on": c.cell("BOUT_on", "Bout:E", region), "off": c.cell("BOUT_off", "Bout:E_lead", region)}
    a = {"on": c.cell("BOUT_A_on", "Bout:A_on", region), "off": c.cell("BOUT_A_off", "Bout:A_off", region)}
    for s, o in (("on", "off"), ("off", "on")):
        c.connect(e[s], a[s], w(10.0))
        c.connect(a[s], e[s], w(-9.0))
        c.connect(e[s], e[o], w(-4.0))
    for side in "lr":
        c.connect(e["on"], hb["rs"][side], w(kick_mv))
    return e


def add_touch_escape(c: "Circuit", gain: float, hb: Dict[str, object], touch: Dict[str, list],
                     s_ref: float = 0.5, relay_mv: float = 8.0, touch_mv: float = 8.0, escape_rs_mv: float = 12.0,
                     mutual_mv: float = 30.0, silence_mv: float = 30.0, adapt_ms: float = 1500.0,
                     latch_mv: float = 0.0, lead_mv: float = 0.1, adapt_mv: float = 1.0, touch_tau_ms: float = 400.0,
                     plateau_mv: float = 22.0, artr_mv: float = 0.5, suppress: Optional[List[str]] = None,
                     quiet: Optional[List[str]] = None, region: str = "hindbrain") -> Dict[str, object]:
    """Innate touch escape: contact on one side turns the body away, decisively.

    Why (26 Nov): with a wall ahead, the range rays and the learned
    anticipation cells drove both ``RS`` cells far past their mutual
    inhibition. RS_l and RS_r both ran at 19.4 Hz (fish) and 19.9 Hz (fly), so
    the two turns cancelled: the fish pressed its head against the tank wall
    and sank, and the fly shortened every stride, stalled and reared up the
    arena wall.

    ``touch``: {"l": [...], "r": [...], "f": [...]} channels reporting contact
    (or near contact) on the left, the right or head-on. An entry is a channel
    name or (channel, s_ref). Each passes through its own relay
    (``TCH_<side>_<k>``, ``add_sensor_relay`` centred on s_ref). The relays of
    one side converge on a saturating touch cell (``TOUCH_<side>``), so the
    escape's drive does not grow with the number of sensors. The fly's six
    relays per side, each wired straight to ESC, beat the mutual inhibition
    and both sides fired.

    ``ESC_l`` / ``ESC_r``: bistable escape cells with strong mutual inhibition,
    so only one side fires. This is like the paired Mauthner cells and their
    recurrent inhibition in fish, or the paired descending neurons of an
    insect's escape. Left contact excites ESC_l; frontal contact excites both,
    and ARTR's standing bias (plus a small lead on the left) picks the side.
    An escape outlasts the contact. The touch cells decay slowly
    (``touch_tau_ms``), an afterdischarge that carries the turn on for about
    1-3 s after the contact ends; adaptation (``ESC_A_<side>``, ``adapt_ms``,
    ``adapt_mv``) bounds it. A short escape was not enough: the fly let go as
    soon as its antennae cleared the wall, crept back and touched again,
    holding a fixed 37 deg angle to the wall (26 Nov).

    ``latch_mv`` is a tonic inside ESC's bistable band. A latch of 0.8 mV/ms
    (lead 0.2) made escapes self-sustaining, but the cells then leaked 1-2 Hz at
    rest, and the lead made that leak asymmetric. It held RS_l on in open water
    74 % of the time (fish) and 32 % (fly), against 17 % and 7 % without the
    block, so the body circled. Hence a latch of 0 and only a small lead.

    Effects:
    - ESC_s excites RS_s (by convention RS_l turns the body right, away from
      the left).
    - ESC_s silences RS_o and the ``suppress`` cells, e.g. the learned
      steering, which could otherwise turn the body back into the wall.
    - Builders add direct motor effects (a stronger tail bend, a pivot).
    - ``quiet`` cells (e.g. FEED) silence the escape: a feeding body tolerates
      touching its food.

    Returns dict(esc={l, r}, adapt={l, r}, touch={side}, relays=[...]).
    """
    def w(mv):
        return drive_weight(mv, gain)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    c.type("Escape:E", **graded(tau_ms=10.0, plateau_mv=plateau_mv, tonic=latch_mv), **common)
    c.type("Escape:E_lead", **graded(tau_ms=10.0, plateau_mv=plateau_mv, tonic=latch_mv + lead_mv), **common)
    c.type("Escape:A", **graded(tau_ms=adapt_ms), **common)
    c.type("Escape:touch", **graded(tau_ms=touch_tau_ms), **common)
    esc = {s: c.cell(f"ESC_{s}", "Escape:E_lead" if s == "l" else "Escape:E", region) for s in "lr"}
    adapt = {s: c.cell(f"ESC_A_{s}", "Escape:A", region) for s in "lr"}
    relays, sums = [], {}
    for side in ("l", "r", "f"):
        entries = touch.get(side, [])
        if not entries:
            continue
        sums[side] = c.cell(f"TOUCH_{side}", "Escape:touch", region)
        for s in ("l", "r") if side == "f" else (side,):
            c.connect(sums[side], esc[s], w(touch_mv))
        for k, entry in enumerate(entries):
            channel, ref = (entry, s_ref) if isinstance(entry, str) else entry
            cell, _, _ = add_sensor_relay(c, gain, channel, ref, name=f"TCH_{side}_{k}")
            relays.append(cell)
            c.connect(cell, sums[side], w(relay_mv))
    rs, artr = hb["rs"], hb["artr"]
    for s, o in (("l", "r"), ("r", "l")):
        c.connect(esc[s], adapt[s], w(10.0))
        c.connect(adapt[s], esc[s], w(-adapt_mv))
        c.connect(esc[s], esc[o], w(-mutual_mv))
        c.connect(artr[s], esc[s], w(artr_mv))
        c.connect(esc[s], rs[s], w(escape_rs_mv))
        c.connect(esc[s], rs[o], w(-silence_mv))
        for cell in suppress or []:
            c.connect(esc[s], cell, w(-silence_mv))
        for cell in quiet or []:
            c.connect(cell, esc[s], w(-silence_mv))
    return dict(esc=esc, adapt=adapt, touch=sums, relays=relays)


# --- Shared spinal blocks ---

def add_leg_oscillators(c: "Circuit", gain: float, legs, neighbours, drive_from: str, drive_mv: float,
                        drive_release: float = 0.5, tau_e_ms: float = 15.0, plateau_mv: Optional[float] = None,
                        tau_a_swing_ms: float = 30.0, tau_a_stance_ms: float = 45.0, w_ea: float = 10.0,
                        w_ae: float = 9.0, w_ec: float = 10.0, w_ce: float = 4.0, inter: float = 3.0,
                        inter_ex: float = 2.0, positions=None, lead=None) -> Tuple[dict, dict, dict]:
    """Per-leg swing/stance escape half-centres with neighbour coupling.

    For each leg: ``E_<leg>_sw``/``E_<leg>_st`` (bistable, terminated by their
    adaptation cells ``A``) crossed through inhibitory ``C`` cells; a
    swinging leg inhibits its neighbours' swing and excites their stance
    (Cruse rules 1-2). On a two-colourable leg graph the legs settle into
    two alternating groups: tripods for six legs, trot pairs for four.
    Returns (E, A, C) keyed by (leg, "sw"|"st").

    The half-centres need regenerative gain to escape: 22 mV plateaus with
    the template's active dendrites (which add up to ~1.4x on depolarising
    input), 30 mV with ``exact_cells`` (measured open-loop: 22 mV gives a
    silent, co-active pair; 26-35 mV a trot at 1.4-2.4 Hz set by the
    adaptation time constants and the MLR drive).
    """
    def w(mv):
        return drive_weight(mv, gain)
    if plateau_mv is None:
        plateau_mv = 30.0 if c.exact_cells else 22.0
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    c.type("Leg:E", **graded(tau_ms=tau_e_ms, plateau_mv=plateau_mv), **common)
    c.type("Leg:E_lead", **graded(tau_ms=tau_e_ms, plateau_mv=plateau_mv, tonic=0.3), **common)
    c.type("Leg:A_swing", **graded(tau_ms=tau_a_swing_ms), **common)
    c.type("Leg:A_stance", **graded(tau_ms=tau_a_stance_ms), **common)
    c.type("Leg:C", **graded(tau_ms=8.0), **common)
    lead = lead or legs[0]
    E, A, C = {}, {}, {}
    for leg in legs:
        x, y = (positions or {}).get(leg, (0.0, 0.0))
        for ph in ("sw", "st"):
            E[leg, ph] = c.cell(f"E_{leg}_{ph}", "Leg:E_lead" if (leg, ph) == (lead, "sw") else "Leg:E", leg,
                                (x, y, 0.002 if ph == "sw" else -0.002))
            A[leg, ph] = c.cell(f"A_{leg}_{ph}", "Leg:A_swing" if ph == "sw" else "Leg:A_stance", leg, (x, y, 0.0))
            C[leg, ph] = c.cell(f"C_{leg}_{ph}", "Leg:C", leg, (x, y * 0.5, 0.0))
        for ph, other in (("sw", "st"), ("st", "sw")):
            c.connect(drive_from, E[leg, ph], w(drive_mv / drive_release))
            c.connect(E[leg, ph], A[leg, ph], w(w_ea))
            c.connect(A[leg, ph], E[leg, ph], w(-w_ae))
            c.connect(E[leg, ph], C[leg, ph], w(w_ec))
            c.connect(C[leg, ph], E[leg, other], w(-w_ce))
    for a, b in neighbours:
        for p, q in ((a, b), (b, a)):
            c.connect(C[p, "sw"], E[q, "sw"], w(-inter))
            c.connect(E[p, "sw"], E[q, "st"], w(inter_ex))
    return E, A, C


def add_stretch_reflex(c: "Circuit", gain: float, joint: str, out_pos: str, out_neg: str,
                       angle_sensor: str, velocity_sensor: str, kp: float = 150.0, kd: float = 20.0,
                       out_mv: float = 8.0, s_ref: float = 0.5, region: str = "spinal") -> Tuple[str, str]:
    """Proportional-derivative joint servo from two rectified error cells.

    ``Q+`` = kp (ref - angle) - kd (velocity - 0.5) drives the positive output
    channel, ``Q-`` the mirror image the negative one (angle sensor
    (q + pi) / 2 pi, velocity (w + 40) / 80, i.e. the archetype encodings).
    Extra input onto ``Q+``/``Q-`` shifts the held angle (equilibrium-point
    control, as a central pattern generator drives a spindle set point).
    """
    def w(mv):
        return drive_weight(mv, gain)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    tonic = kp * s_ref + kd * 0.5
    c.type(f"Posture:{joint}:agonist", **graded(tau_ms=15.0, tonic=tonic), **common)
    c.type(f"Posture:{joint}:antagonist", **graded(tau_ms=15.0, tonic=-tonic), **common)
    qp = c.cell(f"Q+_{joint}", f"Posture:{joint}:agonist", region)
    qn = c.cell(f"Q-_{joint}", f"Posture:{joint}:antagonist", region)
    # The tonic must cancel the sensors' 0.5 baseline: in a dendrite-active
    # circuit apply the measured gains; exact cells need nominal weights.
    exact = not c.exact_cells
    c.sensor(angle_sensor, qp, sensor_weight(-kp, gain, exact=exact))
    c.sensor(velocity_sensor, qp, sensor_weight(-kd, gain, exact=exact))
    c.sensor(angle_sensor, qn, sensor_weight(kp, gain, exact=exact))
    c.sensor(velocity_sensor, qn, sensor_weight(kd, gain, exact=exact))
    c.connect(qp, out_pos, w(out_mv))
    c.connect(qn, out_neg, w(out_mv))
    return qp, qn


# --- Runtime calibration of sensory relays ---

def measure_sensor_relay(profile: str, channel: str, values, w_mv: float, tonic: float, tau_ms: float,
                         gain: float = 100.0, ms: int = 1500, exact_cells: bool = True, **bio) -> List[float]:
    """Steady release of a graded relay cell (input ``w_mv`` x sensor, tonic
    ``tonic``) for constant sensor ``values``, measured in the runtime itself.

    Sensors arrive as pulse trains (the body's phase-accumulator encoder), so a
    relay's mean response is below that of a steady current of the same mean;
    designs that need an absolute operating point (vestibular tilt, joint
    angle) measure it instead of assuming it. Uses the release build of the
    open-loop probe (``target/release/examples/celegans_depth_probe``).
    """
    import subprocess
    import tempfile
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    k = names.index(channel)
    out = []
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        c = Circuit.for_embodiment(profile, exact_cells=exact_cells)
        common = dict(nmda_ratio=0.0, synaptic_gain=gain)
        c.type("Relay", **graded(tau_ms=tau_ms, tonic=tonic), **common, **bio)
        cell = c.cell("R", "Relay")
        c.sensor(channel, cell, sensor_weight(w_mv, gain))
        c.type("Motor:x", **graded(tau_ms=15.0, release_hz=500.0), **common)
        for o in c.outputs:
            c.output(o, "Motor:x")
        c.write(tmp / "net.json", synaptic_gain=gain, depth=0)
        for v in values:
            header = {"profile": profile, "sensor_names": names, "output_names": c.outputs, "trace_every_steps": 10}
            with open(tmp / "in.jsonl", "w") as f:
                f.write(json.dumps(header) + "\n")
                for step in range(0, ms // 2 + 10, 10):
                    sv = [0.5] * len(names)
                    sv[k] = v
                    f.write(json.dumps({"time_s": step * 0.002, "step": step, "sensors": sv}) + "\n")
            subprocess.run([str(ROOT / "target/release/examples/celegans_depth_probe"), "--network", str(tmp / "net.json"),
                            "--ms", str(ms), "--input", str(tmp / "in.jsonl"), "--out", str(tmp / "o.json"),
                            "--series", str(tmp / "s.jsonl"), "--series-ms", "10", "--no-raster"],
                           check=True, capture_output=True)
            rows = [json.loads(x)["hidden_hz"][0] / 20.0 for x in open(tmp / "s.jsonl")]
            tail = rows[len(rows) // 2:]
            out.append(sum(tail) / len(tail))
    return out


_RELAY_CACHE: Dict[tuple, Tuple[float, float]] = {}


def add_sensor_relay(c: "Circuit", gain: float, channel: str, s_ref: float, drive_mv: float = 20.0,
                     slope_mv: float = 1.5, tau_ms: float = 10.0, region: str = "sensory",
                     name: Optional[str] = None) -> Tuple[str, float, float]:
    """Excitation-only relay of a pulse-coded sensor, centred on ``s_ref``.

    Returns (cell, release at s_ref, release per sensor unit), measured in the
    runtime (cached per channel kind, drive and operating point). The relay's
    input never turns inhibitory, so the pulse ripple cannot be rectified
    into a bias; its continuous release can be compared against a tonic
    downstream without the duty-cycle failure of direct cancellation.
    """
    g0 = 5.0 * drive_mv * s_ref / 70.0
    v0 = -70.0 / (1.0 + g0)
    bio = dict(graded_release_v_half_mv=v0, graded_release_slope_mv=slope_mv)
    key = (c.profile, round(s_ref, 4), drive_mv, slope_mv, tau_ms)
    if key not in _RELAY_CACHE:
        lo, mid, hi = measure_sensor_relay(c.profile, channel, [s_ref - 0.05, s_ref, s_ref + 0.05], drive_mv, 0.0,
                                           tau_ms, **bio)
        _RELAY_CACHE[key] = (mid, (hi - lo) / 0.1)
    r0, slope = _RELAY_CACHE[key]
    tname = f"Relay:{round(s_ref, 4)}" + (f":{drive_mv:g}:{slope_mv:g}:{tau_ms:g}" if (drive_mv, slope_mv, tau_ms) != (20.0, 1.5, 10.0) else "")
    c.type(tname, **graded(tau_ms=tau_ms), nmda_ratio=0.0, synaptic_gain=gain, **bio)
    cell = c.cell(name or f"R_{channel}", tname, region)
    c.sensor(channel, cell, sensor_weight(drive_mv, gain))
    return cell, r0, slope


def add_servo_reflex(c: "Circuit", gain: float, joint: str, out_pos: str, out_neg: str, angle_sensor: str,
                     velocity_sensor: str, angle_range=(-math.pi, math.pi), velocity_range=(-40.0, 40.0),
                     ref_rad: float = 0.0, kp_mv_per_rad: float = 40.0, kd_mv_per_rad_s: float = 2.0,
                     tone: float = 1.0, out_mv: float = 10.0, relay_tau_ms: float = 30.0,
                     region: str = "spinal") -> Tuple[str, str]:
    """Ripple-safe proportional-derivative joint servo (requires exact cells).

    Angle and velocity reach two push-pull cells Q+/Q- through calibrated
    excitation-only relays; Q+ = tone + kp (ref - angle) - kd velocity, Q- the
    mirror image, in mV/ms per rad and per rad/s. Extra input onto Q+/Q-
    shifts the held angle (equilibrium-point control).

    The relays must smooth the sensors' ~250 Hz pulse trains before the gain:
    with 10 ms relays the residual ripple (x kp) is half-wave rectified by Q+
    and Q-, which then co-contract (both ~0.4 at zero error), hold a 0.08 rad
    offset and give ~2 N m/rad on the UGOT knee; 30 ms relays remove the
    offset and give ~6.5 N m/rad (kp 100, out 20; measured open-loop).
    """
    if not c.exact_cells:
        raise ValueError("add_servo_reflex needs Circuit(exact_cells=True)")
    s_ref = (ref_rad - angle_range[0]) / (angle_range[1] - angle_range[0])
    ra, ra0, sa = add_sensor_relay(c, gain, angle_sensor, s_ref, tau_ms=relay_tau_ms)
    rv, rv0, sv = add_sensor_relay(c, gain, velocity_sensor, (0.0 - velocity_range[0]) / (velocity_range[1] - velocity_range[0]),
                                   tau_ms=relay_tau_ms)
    kp = kp_mv_per_rad / (sa / (angle_range[1] - angle_range[0]))
    kd = kd_mv_per_rad_s / (sv / (velocity_range[1] - velocity_range[0]))
    base = kp * ra0 + kd * rv0
    c.type(f"Servo:{joint}:+", **graded(tau_ms=8.0, tonic=tone + base), nmda_ratio=0.0, synaptic_gain=gain)
    c.type(f"Servo:{joint}:-", **graded(tau_ms=8.0, tonic=tone - base), nmda_ratio=0.0, synaptic_gain=gain)
    qp = c.cell(f"Q+_{joint}", f"Servo:{joint}:+", region)
    qn = c.cell(f"Q-_{joint}", f"Servo:{joint}:-", region)
    for relay, k in ((ra, kp), (rv, kd)):
        c.connect(relay, qp, drive_weight(-k, gain))
        c.connect(relay, qn, drive_weight(k, gain))
    c.connect(qp, out_pos, drive_weight(out_mv, gain))
    c.connect(qn, out_neg, drive_weight(out_mv, gain))
    return qp, qn


# --- Innate motor sequences (fall recovery and other fixed action patterns) ---

SEQ_TONIC = 1.0        # inside the 22 mV plateau band: off until kicked, then self-holding
SEQ_W_AE = 4.0         # A_i -> K_i: ends K_i (2.0 let a residual trigger drive hold K_0 on)
SEQ_ON = 0.95          # release of a held keyframe cell (22 mV plateau, measured ~18.6-19.3 Hz / 20)
SEQ_M_GAIN = 60.0      # K -> M (motor relay): saturated from K ~0.3 up
SEQ_M_THRESHOLD = 8.0  # M tonic inhibition: an idle K (~0.035) gives no output
SEQ_M_SOURCE = 34.0    # net drive of a saturated M (K ~0.7); the drive curve's source


def add_motor_sequence(c: "Circuit", gain: float, name: str, taus_ms: List[float], trigger: str,
                       trigger_mv: float = 12.0, gate_tau_ms: float = 1500.0, gate_mv: float = 40.0,
                       region: str = "spinal") -> Dict[str, object]:
    """A chain of keyframe cells: an innate motor program (fixed action pattern).

    ``K_<name>_<i>`` (22 mV plateau, tonic inside its bistable band) is off
    until kicked and then holds itself on; its adaptation cell ``A`` (time
    constant ``taus_ms[i]``) ends it. The handover is latched: ``S`` (a
    plateau cell set by ``K``) remembers that the keyframe ran, ``X`` = S and
    not K fires once ``K`` falls below ~0.3 and kicks ``K_{i+1}``, whose
    adaptation resets ``S``. (Kicking the next cell from ``A``'s level
    stalled the chain with long time constants; a slow copy of ``K``
    decayed with it.)
    The caller drives the body from each ``K`` (posture offsets, as the
    walk's phase cells do). ``trigger`` (a graded cell, e.g. a fall
    detector) starts ``K_0`` through ``T_<name>``; every ``K`` excites a slow
    gate ``G_<name>`` that inhibits ``T``, so a running program is not
    restarted and the trigger re-arms ~``gate_tau_ms`` after the program
    ends (a retry while still lying). Durations follow the adaptation time
    constants; calibrate them in the runtime (``measure_sequence``).
    Drive the body from ``M`` (the saturating motor relay of each ``K``),
    with weights from ``posture_drive``.
    Returns {"K": [...], "A": [...], "M": [...], "T": trigger cell, "G": gate}."""
    def w(mv):
        return drive_weight(mv, gain)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    c.type(f"Seq:{name}:K", **graded(tau_ms=50.0, plateau_mv=22.0, tonic=SEQ_TONIC), **common)
    # T: on (release ~0.5) when the trigger cell is well on (> ~0.4).
    c.type(f"Seq:{name}:T", **graded(tau_ms=20.0, tonic=-3.0), **common)
    # G: every K excites it (12 mV/ms); idle K cells leak ~0.035 each, which
    # with 12 keyframes held G at 0.4 and blocked the trigger before any
    # program ran, so G's tonic cancels the idle leak of all n keyframes.
    c.type(f"Seq:{name}:G", **graded(tau_ms=gate_tau_ms, tonic=-(len(taus_ms) * 0.035 * 12.0 + 1.0)), **common)
    c.type(f"Seq:{name}:S", **graded(tau_ms=50.0, plateau_mv=22.0, tonic=SEQ_TONIC), **common)
    c.type(f"Seq:{name}:X", **graded(tau_ms=20.0, tonic=-4.0), **common)
    # M: the keyframe's motor relay, saturated whenever K is above ~0.3 and
    # silent when it idles. K's own release sags from ~0.85 to ~0.5 as its
    # adaptation grows, and idle K cells leak; driving the joints from K
    # directly let every keyframe drift back toward the rest posture.
    c.type(f"Seq:{name}:M", **graded(tau_ms=10.0, tonic=-SEQ_M_THRESHOLD), **common)
    t = c.cell(f"T_{name}", f"Seq:{name}:T", region)
    g = c.cell(f"G_{name}", f"Seq:{name}:G", region)
    c.connect(trigger, t, w(trigger_mv))
    c.connect(g, t, w(-gate_mv))
    ks, as_, ms, xs, edge, latch = [], [], [], [], None, None
    for i, tau in enumerate(taus_ms):
        tname = f"Seq:{name}:A:{round(tau)}"
        if tname not in c.types:
            c.type(tname, **graded(tau_ms=tau), **common)
        k = c.cell(f"K_{name}_{i:02d}", f"Seq:{name}:K", region)
        a = c.cell(f"A_{name}_{i:02d}", tname, region)
        mo = c.cell(f"M_{name}_{i:02d}", f"Seq:{name}:M", region)
        c.connect(k, mo, w(SEQ_M_GAIN))
        ms.append(mo)
        c.connect(k, a, w(10.0))
        c.connect(a, k, w(-SEQ_W_AE))
        c.connect(k, g, w(12.0))
        c.connect(edge if edge else t, k, w(4.0))
        if not edge:
            # The first keyframe shuts its trigger off once established (from
            # its adaptation): the slow gate alone held T, and so K_0, on for
            # ~1.4 s, longer than a short first keyframe; cutting T from K_0
            # itself stopped K_0 before it latched.
            c.connect(a, t, w(-40.0))
        if edge:
            # Reset the previous keyframe's latch (which ends its edge cell)
            # from this keyframe's adaptation: it rises only once this one is
            # established. Resetting from K itself let K's first partial
            # activation cut its own starter off and the chain stalled.
            c.connect(a, latch, w(-20.0))
        if i + 1 < len(taus_ms):
            sl = c.cell(f"S_{name}_{i:02d}", f"Seq:{name}:S", region)
            x = c.cell(f"X_{name}_{i:02d}", f"Seq:{name}:X", region)
            c.connect(k, sl, w(4.0))
            c.connect(sl, x, w(30.0))
            c.connect(k, x, w(-80.0))
            edge, latch = x, sl
            xs.append(x)
        ks.append(k)
        as_.append(a)
    return {"K": ks, "A": as_, "M": ms, "X": xs, "T": t, "G": g}


def measure_sequence(c: "Circuit", seq: Dict[str, object], trigger_channel: str, trigger_value: float,
                     ms: int = 30000, rest_value: float = 0.5, start_s: float = 1.0) -> List[Tuple[float, float]]:
    """(on, off) times (s) of each keyframe cell of ``seq`` in the runtime,
    with ``trigger_channel`` held at ``trigger_value`` from ``start_s`` (the
    other channels at ``rest_value``): the open-loop probe, as
    ``measure_sensor_relay``."""
    import subprocess
    import tempfile
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{c.profile}.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    k = names.index(trigger_channel)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        c.write(tmp / "net.json", synaptic_gain=100.0, depth=0)
        hidden = json.loads((tmp / "net.json").read_text())["connectome_labels"]["hidden_nodes"]
        header = {"profile": c.profile, "sensor_names": names, "output_names": c.outputs, "trace_every_steps": 10}
        with open(tmp / "in.jsonl", "w") as f:
            f.write(json.dumps(header) + "\n")
            for step in range(0, ms // 2 + 10, 10):
                sv = [rest_value] * len(names)
                sv[k] = trigger_value if step * 0.002 >= start_s else rest_value
                f.write(json.dumps({"time_s": step * 0.002, "step": step, "sensors": sv}) + "\n")
        subprocess.run([str(ROOT / "target/release/examples/celegans_depth_probe"), "--network", str(tmp / "net.json"),
                        "--ms", str(ms), "--input", str(tmp / "in.jsonl"), "--out", str(tmp / "o.json"),
                        "--series", str(tmp / "s.jsonl"), "--series-ms", "10", "--no-raster"],
                       check=True, capture_output=True)
        rows = [json.loads(x)["hidden_hz"] for x in open(tmp / "s.jsonl")]
    # First pass only: a program that ends while the trigger still holds
    # (still lying) runs again once the gate decays - a retry.
    out, after = [], 0
    for kc in seq["K"]:
        i = hidden.index(kc)
        on = next((n for n in range(after, len(rows)) if rows[n][i] > 10.0), None)
        if on is None:
            out.append((float("nan"), float("nan")))
            continue
        off = next((n for n in range(on, len(rows)) if rows[n][i] < 5.0), len(rows) - 1)
        out.append(((on + 1) * 0.01, (off + 1) * 0.01))
        after = off
    return out


# --- Measured motor-cell drive curve (exact posture offsets) ---

DRIVE_CURVE_CACHE = ROOT / "target/qa/aarnn_drive_curve.json"


def measure_drive_curve(profile: str = "nao", tau_ms: float = 15.0, source_drive: float = SEQ_M_SOURCE,
                        gain: float = 100.0, ms: int = 3000) -> dict:
    """Steady release of a motor (posture) cell - graded, ``tau_ms``,
    release 500 Hz, tonic set for a resting release r0 - under one extra
    synapse from a source cell like a keyframe's motor relay (tau 10 ms,
    net drive ``source_drive``: saturated), measured in the
    runtime over a grid of r0 x drive (mV/ms nominal). The closed-form
    ``release_tonic`` difference is exact for excitation but not for strong
    inhibition (24 Nov: a keyframe asking an elbow for its joint limit got
    -0.93 of -1.54 rad), so keyframe drives are read from this curve."""
    import subprocess
    import tempfile
    key = f"{profile}:{tau_ms}:M{source_drive}:{gain}"
    store = json.loads(DRIVE_CURVE_CACHE.read_text()) if DRIVE_CURVE_CACHE.exists() else {}
    if key in store:
        return store[key]
    r0s = [round(0.05 * i, 2) for i in range(1, 20)]
    drives = [round(-20.0 + 0.5 * j, 2) for j in range(81)]
    c = Circuit.for_embodiment(profile, exact_cells=True)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    c.type("Src", **graded(tau_ms=10.0, tonic=source_drive), **common)
    src = c.cell("SRC", "Src")
    cells = {}
    for r0 in r0s:
        c.type(f"P:{r0}", **graded(tau_ms=tau_ms, release_hz=500.0, tonic=release_tonic(r0)), **common)
        for d in drives:
            cells[r0, d] = c.cell(f"P_{r0}_{d}", f"P:{r0}")
            if d:
                c.connect(src, cells[r0, d], drive_weight(d, gain))
    c.type("Motor:x", **graded(tau_ms=15.0, release_hz=500.0), **common)
    for o in c.outputs:
        c.output(o, "Motor:x")
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        c.write(tmp / "net.json", synaptic_gain=gain, depth=0)
        hidden = json.loads((tmp / "net.json").read_text())["connectome_labels"]["hidden_nodes"]
        with open(tmp / "in.jsonl", "w") as f:
            f.write(json.dumps({"profile": profile, "sensor_names": names, "output_names": c.outputs,
                                "trace_every_steps": 10}) + "\n")
            for step in range(0, ms // 2 + 10, 10):
                f.write(json.dumps({"time_s": step * 0.002, "step": step, "sensors": [0.5] * len(names)}) + "\n")
        subprocess.run([str(ROOT / "target/release/examples/celegans_depth_probe"), "--network", str(tmp / "net.json"),
                        "--ms", str(ms), "--input", str(tmp / "in.jsonl"), "--out", str(tmp / "o.json"),
                        "--series", str(tmp / "s.jsonl"), "--series-ms", "10", "--no-raster"],
                       check=True, capture_output=True)
        rows = [json.loads(x)["hidden_hz"] for x in open(tmp / "s.jsonl")]
    tail = rows[len(rows) // 2:]

    def rel(name):
        i = hidden.index(name)
        return sum(r[i] for r in tail) / len(tail) / 500.0
    curve = {"r0": r0s, "drive": drives, "release": [[rel(cells[r0, d]) for d in drives] for r0 in r0s]}
    store[key] = curve
    DRIVE_CURVE_CACHE.parent.mkdir(parents=True, exist_ok=True)
    DRIVE_CURVE_CACHE.write_text(json.dumps(store))
    return curve


def posture_drive(curve: dict, r0: float, target: float) -> float:
    """Drive (mV/ms nominal, from a source at the curve's release) that
    moves a motor cell resting at release ``r0`` to ``target``, by
    interpolation in the measured curve (clamped to its range)."""
    r0s, drives = curve["r0"], curve["drive"]

    def at(row):     # drive for `target` on one measured row (monotone in drive)
        rel = curve["release"][row]
        if target <= rel[0]:
            return drives[0]
        for j in range(1, len(drives)):
            if rel[j] >= target:
                lo, hi = rel[j - 1], rel[j]
                f = 0.0 if hi == lo else (target - lo) / (hi - lo)
                return drives[j - 1] + f * (drives[j] - drives[j - 1])
        return drives[-1]
    if r0 <= r0s[0]:
        return at(0)
    if r0 >= r0s[-1]:
        return at(len(r0s) - 1)
    k = next(i for i in range(1, len(r0s)) if r0s[i] >= r0)
    f = (r0 - r0s[k - 1]) / (r0s[k] - r0s[k - 1])
    return at(k - 1) + f * (at(k) - at(k - 1))


# --- Learning by experience: teacher-gated anticipation (any body) -----------
#
# The same block on every body (NAO pattern, 23-24 Nov): an innate near
# detector on each side's range channel is the teacher. It drives the
# modulatory cell LC (which gates plasticity) and a learning-turn cell LT_side,
# and LT_side drives that side's turn command exactly as the innate reflex
# does. LT's candidate inputs are distance-tuned range cells CS_side_k (a sparse
# group) whose synapses onto LT start silent and are the only plastic ones;
# when an echo at their distance predicts the near reflex, they strengthen and
# the body learns to turn earlier. Growth (neurogenesis in the CS niche, clonal
# newborns with jittered tunings, inputs fixed, outputs plastic) can extend
# the anticipation beyond the designed group's range (Freenove hexapod, 25 Nov).


def _range_cell(c: "Circuit", gain: float, channel: str, value: float, inverted: bool, name: str) -> str:
    """A cell on while the channel reads nearer than the distance coded by
    ``value``: a rising relay when the code grows with nearness, else a relay
    and an inverting cell (on while the code is below ``value``)."""
    if not inverted:
        cell, _, _ = add_sensor_relay(c, gain, channel, value, tau_ms=30.0, name=name)
        return cell
    # Direct proximity from the sensor (the wheeled explorer's proximity
    # cell): tonic w and sensor weight -w give w (1 - code), at half release
    # (5.68 mV/ms) where the code equals ``value``; a steep release slope makes
    # it a threshold. Relays on these small d / reach codes changed only
    # ~16.8 vs 20 Hz from far to near, so relay-and-invert cells barely
    # switched (UGOT quadruped, Keenon T8, 25 Nov).
    # The response is graded (the cell ramps over ~0.3-0.5 m), not the
    # threshold the formula suggests, and the switch-on point drifts nearer
    # as ``value`` grows. The sensor weight is divided by
    # _prox_inh_gain(value) so the cell reaches ~5 Hz at ``value``, the
    # point the UGOT quadruped's learner already met (gain 1.0 at code
    # 0.093). Measured on the Keenon T8 (code 0.22): gain 1.0 gave 1.4 Hz at
    # its nearest approach, so its teacher barely fired; gain 1.5 gives 5 Hz
    # at the teach distance (25 Nov).
    pv = max(1.0 - value, 0.05)
    w = 5.68 / pv
    tname = f"Learn:prox:{round(value, 4)}"
    if tname not in c.types:
        c.type(tname, **graded(tau_ms=20.0, tonic=w), nmda_ratio=0.0, synaptic_gain=gain,
               graded_release_slope_mv=1.5, graded_release_v_half_mv=-70.0 / (1.0 + 5.0 * 5.68 / 70.0))
    cell = c.cell(name, tname, "sensory")
    c.sensor(channel, cell, sensor_weight(-w / _prox_inh_gain(value), gain, exact=True))
    return cell


def _prox_inh_gain(value: float) -> float:
    """Sensor-weight divisor of a direct proximity cell; a two-point
    calibration (UGOT code 0.093 -> 1.0, Keenon T8 code 0.22 -> 1.5)."""
    return max(1.0, 1.0 + 3.9 * (value - 0.093))


def add_learned_anticipation(c: "Circuit", gain: float, sides: Dict[str, List[str]], encode, inverted: bool,
                             teach_m: float, near_m: float, far_m: float, groups: int,
                             targets: Dict[str, List[Tuple[str, float]]], w0_mv: float = 0.05, teach_mv: float = 12.0,
                             within_p: float = 0.5, within_mv: float = 0.3, seed: int = 7) -> Dict[str, object]:
    """Teacher-gated anticipation on every side in ``sides`` (side -> range
    channels; ``encode(d)`` -> sensor value at distance d). ``targets``:
    side -> [(cell, mV/ms at full LT release)] the learned turn drives."""
    import random
    rnd = random.Random(seed)
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    if "LC" not in c.index:
        c.type("Modulator:LC", **graded(tau_ms=50.0), **common)
        c.cell("LC", "Modulator:LC", "hindbrain")
    c.type("Learn:turn", **graded(tau_ms=20.0), **common)
    out = dict(lt={}, cs={}, teacher={}, channels=sides, teach_m=teach_m, near_m=near_m, far_m=far_m, inverted=inverted)
    for side, channels in sides.items():
        lt = c.cell(f"LT_{side}", "Learn:turn", "hindbrain")
        for cell, mv in targets.get(side, []):
            c.connect(lt, cell, drive_weight(mv, gain))
        teachers = [_range_cell(c, gain, ch, encode(teach_m), inverted, f"TN_{side}_{i}") for i, ch in enumerate(channels)]
        for t in teachers:
            c.connect(t, "LC", drive_weight(12.0, gain))
            c.connect(t, lt, drive_weight(teach_mv / len(teachers), gain))
        cells = []
        for k in range(groups):
            d = near_m + (far_m - near_m) * k / max(1, groups - 1)
            ch = channels[k % len(channels)]
            cs = _range_cell(c, gain, ch, encode(d), inverted, f"CS_{side}_{k:02d}")
            c.connect(cs, lt, drive_weight(w0_mv, gain))
            cells.append(cs)
        for a in cells:
            for b in cells:
                if a != b and rnd.random() < within_p:
                    c.connect(a, b, drive_weight(within_mv, gain))
        out["lt"][side], out["cs"][side], out["teacher"][side] = lt, cells, teachers
    return out


def add_learned_foraging(c: "Circuit", gain: float, walk: str, bout_on: Optional[str], levels: int = 6,
                         hunger_below: float = 0.5, w0_mv: float = 0.05, teach_mv: float = 12.0,
                         stop_mv: float = 30.0, run_mv: float = 20.0, turn_mv: float = 8.0,
                         slow_tau_ms: float = 3000.0, trend_threshold_mv: float = 2.0, hunger_gate_mv: float = 14.0,
                         hunger_tonic_mv: float = 14.0, hunger_inhibit_mv: float = 24.0,
                         hunger_tau_ms: float = 2000.0, halt: Optional[List[str]] = None,
                         steer: bool = False, steer_mv: float = 20.0,
                         steer_threshold_mv: float = 2.0) -> Dict[str, object]:
    """Learned foraging / recharging (needs-v1; user, 25 Nov: newborns feed when
    food is presented but must learn to seek it; robots are given power first).

    Innate (physiology, not the seeking):
    - FEED: on while energy flows in (``needs_consume``: food at the mouth,
      charge current; it flows only below a full reserve). It stops the walk
      command and the ``halt`` cells (turning) while feeding (consummatory
      reflex), teaches the value cell V and
      drives the reward cell DA_R (dopaminergic: opens the plasticity gate and
      feeds the dopamine pool).
    - HUNGER: tonically on, silenced by SATED (a relay of the reserve centred on
      ``hunger_below``).
    - Run-and-turn machinery: RISE / FALL compare V with a slow copy V_SLOW. A
      rising value suppresses the spontaneous turn bouts (``bout_on``: keep
      running); a falling value starts one (turn, as a worm's pirouette). Both
      need HUNGER: the motivational state gates whether a food memory is used,
      not whether it is formed (Drosophila: Krashes et al. 2009).

    Learned:
    - CF_<side>_<k>: cue-band cells (on while ``needs_cue_<side>`` exceeds band
      k), each onto V through a silent plastic synapse (``w0_mv``). While energy
      flows in, the cue is strong and V is taught, so the active bands
      strengthen: the body learns that the cue predicts energy.
    - Before learning V stays silent, RISE / FALL never fire and the body only
      explores. After learning, a hungry body climbs the cue's gradient; a
      sated one ignores it.

    Optional learned steering (``steer``; klinotaxis, as a worm's head-swing
    comparison or a fly's bilateral antennal comparison):
    - V_l / V_r: one value cell per side, taught by FEED like V, learning
      through silent plastic synapses from that side's cue bands.
    - STEER_l / STEER_r: the difference V_l - V_r (``steer_mv`` per full
      release), gated by HUNGER like RISE / FALL.
    - STEER_<side> turns the body toward that side; the builder wires it to
      the motors, e.g. a graded wheel push-pull.
    - Before learning both values are silent and nothing steers. After
      learning, a hungry body turns toward the side whose cue predicts food.
    Run 1 (26 Nov) showed why it is needed: a Keenon running at 0.3 m/s
    passed 0.59 m from the dock centre (reach 0.51 m) with its left cue at
    0.83. Bout timing alone could not bring it back.
    """
    common = dict(nmda_ratio=0.0, synaptic_gain=gain)
    sated, _, _ = add_sensor_relay(c, gain, "needs_energy", hunger_below, tau_ms=200.0, name="SATED")
    c.type("Need:hunger", **graded(tau_ms=hunger_tau_ms, tonic=hunger_tonic_mv), **common)
    hunger = c.cell("HUNGER", "Need:hunger", "hindbrain")
    c.connect(sated, hunger, drive_weight(-hunger_inhibit_mv, gain))
    feed, _, _ = add_sensor_relay(c, gain, "needs_consume", 0.5, tau_ms=30.0, name="FEED")
    c.type("Modulator:DA_reward", **graded(tau_ms=50.0), **common)
    da = c.cell("DA_R", "Modulator:DA_reward", "hindbrain")
    c.connect(feed, da, drive_weight(12.0, gain))
    c.connect(feed, walk, drive_weight(-stop_mv, gain))
    # Feeding halts locomotion as a whole (a worm dwelling on food): on wheeled
    # bases the turn cells drive the wheels directly, and a Keenon stopped only
    # in its forward command still rolled off the dock at 0.13-0.28 m/s,
    # grazing the pad's edge and charging for ~1 s per arrival (25 Nov).
    for cell in halt or []:
        c.connect(feed, cell, drive_weight(-stop_mv, gain))
    c.type("Learn:value", **graded(tau_ms=50.0), **common)
    v = c.cell("V", "Learn:value", "hindbrain")
    c.connect(feed, v, drive_weight(teach_mv, gain))
    cf = {}
    for side in ("left", "right"):
        cf[side] = []
        for k in range(levels):
            level = (k + 1) / (levels + 1)
            cell, _, _ = add_sensor_relay(c, gain, f"needs_cue_{side}", level, tau_ms=30.0, name=f"CF_{side[0]}_{k}")
            c.connect(cell, v, drive_weight(w0_mv, gain))
            cf[side].append(cell)
    c.type("Learn:value_slow", **graded(tau_ms=slow_tau_ms), **common)
    v_slow = c.cell("V_SLOW", "Learn:value_slow", "hindbrain")
    c.connect(v, v_slow, drive_weight(10.0, gain))
    # RISE ~ V - V_SLOW, FALL ~ V_SLOW - V (20 mV/ms per full-release
    # difference), each against its threshold plus the hunger gate, which a
    # hungry body's near-saturated HUNGER (tonic 14 mV/ms, ~0.9 release)
    # cancels. With tonic 8 the Keenon's HUNGER reached only 9.8 Hz (its
    # template keeps active dendrites) and the gate leaked when sated.
    c.type("Learn:trend", **graded(tau_ms=50.0, tonic=-(trend_threshold_mv + hunger_gate_mv)), **common)
    rise = c.cell("RISE", "Learn:trend", "hindbrain")
    fall = c.cell("FALL", "Learn:trend", "hindbrain")
    for cell, sign in ((rise, 1.0), (fall, -1.0)):
        c.connect(v, cell, drive_weight(20.0 * sign, gain))
        c.connect(v_slow, cell, drive_weight(-20.0 * sign, gain))
        c.connect(hunger, cell, drive_weight(hunger_gate_mv / 0.9, gain))
    if bout_on:
        c.connect(rise, bout_on, drive_weight(-run_mv, gain))
        c.connect(fall, bout_on, drive_weight(turn_mv, gain))
        c.connect(feed, bout_on, drive_weight(-run_mv, gain))
    steer_cells = {}
    if steer:
        side_v = {}
        for sd in "lr":
            side_v[sd] = c.cell(f"V_{sd}", "Learn:value", "hindbrain")
            c.connect(feed, side_v[sd], drive_weight(teach_mv, gain))
            for cell in cf["left" if sd == "l" else "right"]:
                c.connect(cell, side_v[sd], drive_weight(w0_mv, gain))
        c.type("Learn:steer", **graded(tau_ms=100.0, tonic=-(steer_threshold_mv + hunger_gate_mv)), **common)
        for sd, other in (("l", "r"), ("r", "l")):
            cell = c.cell(f"STEER_{sd}", "Learn:steer", "hindbrain")
            c.connect(side_v[sd], cell, drive_weight(steer_mv, gain))
            c.connect(side_v[other], cell, drive_weight(-steer_mv, gain))
            c.connect(hunger, cell, drive_weight(hunger_gate_mv / 0.9, gain))
            c.connect(feed, cell, drive_weight(-stop_mv, gain))
            steer_cells[sd] = cell
    return dict(hunger=hunger, sated=sated, feed=feed, da=da, v=v, v_slow=v_slow, rise=rise, fall=fall, cf=cf,
                steer=steer_cells)


def learner_fields(hidden: List[str], outputs: List[str], gain: float, rule: str = "oja",
                   w_max_mv: float = 1.5) -> dict:
    """Plasticity fields of the learners: anticipation (LT_* plastic, CS_*
    synapses, LC teacher) and foraging (V plastic, CF_* synapses, DA_R reward);
    the newborns' outputs learn, capped."""
    return dict(
        aarnn_plasticity_enabled=True, aarnn_graded_plasticity_enabled=True,
        aarnn_plastic_cells=dict(enabled=True, hidden=[[h.startswith("LT_") or h in ("V", "V_l", "V_r") for h in hidden]],
                                 outputs=[False] * len(outputs), newborns=False, newborn_outputs=True,
                                 pre_hidden=[[h.startswith(("CS_", "CF_")) for h in hidden]]),
        aarnn_matrix_plasticity_rule=rule, aarnn_plasticity_w_max=w_max_mv / (RELEASE_HZ / 1000.0 * KERNEL_EXC * gain),
        aarnn_rate_homeostasis_tau_ms=600000.0, aarnn_rate_homeostasis_gain=0.0, aarnn_plasticity_eta=0.0,
        aarnn_modulatory_cells=dict(enabled="LC" in hidden or "DA_R" in hidden, hidden=[[h in ("LC", "DA_R") for h in hidden]],
                                    outputs=[False] * len(outputs)),
        aarnn_modulator_tau_ms=1000.0, aarnn_modulator_ref_hz=10.0)


def growth_niche_fields(hidden: List[str], input_scale: float = 1.0, extra: int = 80) -> dict:
    """Neurogenesis in the learners' niche (CS_ distance cells, CF_ cue-band
    cells): clonal newborns (parent's type and jittered inputs), outputs
    silent and plastic."""
    return dict(growth_enabled=True, morpho_growth_enabled=False, new_edge_prob=0.05, growth_new_synapse_scale=0.0025,
                migrate_in_prob=0.0, migrate_out_prob=0.0, p_in=0.25, p_out=0.25, max_total_neurons=len(hidden) + extra,
                growth_placement_mode="high", growth_io_floor_enabled=False,
                growth_parent_cells=[[h.startswith(("CS_", "CF_")) for h in hidden]], growth_inherit_sensory_inputs=True,
                growth_new_input_synapse_scale=input_scale, growth_newborn_inherit_type=True)


def _place_learner_cells(snap: dict, profile: Optional[str]) -> None:
    """Put the learning cells (LC, LT_*, TN_*, CS_*; also the foraging and
    touch-escape cells) that the builder left at the origin where the profile's layout rules (layout_designed_circuit.py)
    place them. A newborn is born next to its parent: with the CS niche at
    the origin of a laid-out body, every zebrafish newborn fell where two
    real nuclei overlap and was refused (AmbiguousAreaOverlap, 37 of 39,
    25 Nov). Profiles without rules for these names are left unchanged."""
    import re
    try:
        from layout_designed_circuit import PROFILES, jitter
    except ImportError:
        return
    rule = PROFILES.get(profile or "")
    if rule is None:
        return
    names = snap["connectome_labels"]["hidden_nodes"]
    nodes = snap["topo"]["layers"][0]
    seen = {(round(n["x"], 4), round(n["y"], 4), round(n["z"], 4)) for n in nodes}
    for node, name in zip(nodes, names):
        if not re.match(r"^(LC|LT_[lr]|TN_[lr]_\d+|CS_[lr]_\d+|HUNGER|SATED|FEED|DA_R|V|V_SLOW|V_[lr]|STEER_[lr]|RISE|FALL|CF_[lr]_\d+|BOUT_(A_)?(on|off)|ESC_(A_)?[lr]|TOUCH_[lrf]|TCH_[lrf]_\d+)$", name):
            continue
        if abs(node["x"]) + abs(node["y"]) + abs(node["z"]) > 1e-9:
            continue
        hit = rule(name, "hidden")
        if hit is None:
            continue
        (x, y, z), region = hit
        key = (round(x, 4), round(y, 4), round(z, 4))
        jx, jy, jz = jitter(f"hidden:{name}") if key in seen else (0.0, 0.0, 0.0)
        seen.add(key)
        node.update(x=x + jx, y=y + jy, z=z + jz, region_name=region)


def sensory_modality(sensor: dict) -> str:
    """Neuromodulator-drive modality of one IO-contract sensor: light, heat,
    food, noxious, energy or other (see src/aarnn/neuromodulation.rs).
    Mechanical, range, acoustic and proprioceptive senses are "other"; the
    zebrafish "olfactory" channels are range rays. A hunger channel is not an
    energy-reserve reading (it is high when the reserve is low), so it stays
    "other"; so do camera OFF cells and water taste."""
    name = str(sensor.get("name", "")).lower()
    kind = str(sensor.get("kind", "")).lower()
    mode = str(sensor.get("mode", "")).lower()
    # Energy needs (needs-v1): intake (food at the mouth, charge current) is
    # food for the modulators (reward dopamine, satiety serotonin); the
    # reserve is energy; the cue is an odour for animals, a beacon (other)
    # for machines.
    if kind == "needs-energy":
        return "energy"
    if kind == "needs-consume":
        return "food"
    if kind == "needs-cue":
        return "food" if mode == "food" else "other"
    mechanical = ("range", "ray", "ultrasonic", "sonar", "proximity", "contact", "touch", "angle", "velocity", "gyro",
                  "accel", "gravity", "joint", "foot", "acoustic", "audio", "lateral", "bend", "stretch", "flow")
    if not mode and any(k in kind for k in mechanical):
        return "other"
    if ".off." in name or name.endswith("_off") or "water" in name or "hunger" in name or "fatigue" in name or "thirst" in name:
        return "other"
    text = " ".join((name, kind, mode))
    has = lambda *keys: any(k in text for k in keys)
    if has("energy", "satiet", "battery", "charge"):
        return "energy"
    if has("repel", "noxious", "nocicep"):
        return "noxious"
    if has("thermo", "heat", "temperat", "warm", "cold"):
        return "heat"
    if has("taste", "gustat", "attract", "odour", "odor", "food", "nutri", "sugar"):
        return "food"
    if has("light", "photo", "retina", "camera", "_lum", "lumin", "irradian", "ocell", "eye"):
        return "light"
    return "other"


def apply_learner(snapshot_path: Path, config_path: Optional[Path], gain: float, meta: dict) -> None:
    """Write the learner fields (and the anticipation metadata the open-loop
    protocol reads) into a built snapshot and its config."""
    snap = json.loads(Path(snapshot_path).read_text())
    hidden = snap["connectome_labels"]["hidden_nodes"]
    outs = snap["connectome_labels"]["output_nodes"]
    fields = learner_fields(hidden, outs, gain)
    snap["net"].update(fields)
    # Neuromodulation stays enabled (25 Nov): from depth 1 the global DA /
    # 5-HT levels scale the learning rate and transmission as in biology,
    # and the LC teaching cell releases into its pool (dopaminergic in the
    # insect mushroom body, noradrenergic in the vertebrate locus
    # coeruleus). At depth 0, where the designed learners run, it has no
    # effect, so earlier results are unchanged.
    snap["connectome_labels"]["learning"] = dict(meta, plastic_cells=[h for h in hidden if h.startswith("LT_") or h in ("V", "V_l", "V_r")],
                                                 modulatory_cells=[h for h in ("LC", "DA_R") if h in hidden],
                                                 rule="teacher-gated anticipation / reward-gated foraging (oja)")
    _place_learner_cells(snap, meta.get("profile"))
    Path(snapshot_path).write_text(json.dumps(snap))
    if config_path:
        cfg = json.loads(Path(config_path).read_text())
        cfg.update(fields)
        Path(config_path).write_text(json.dumps(cfg, indent=1))
