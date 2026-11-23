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
   coupling, and a frozen structure (no growth, plasticity, spontaneous
   drives or Dale rewiring) so the designed wiring is what runs.

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
        net.update(
            num_sensory_neurons=len(self.sensory), num_hidden_layers=1,
            num_hidden_per_layer_initial=n, num_output_neurons=len(self.outputs),
            io_channels_are_biological=False, sensory_target_layer=0, output_source_layer=0,
            max_total_neurons=max(n + len(self.outputs), int(net.get("max_total_neurons", 0) or 0)),
            growth_enabled=False, morpho_growth_enabled=False, use_morphology=False, use_aarnn_delays=False,
            aarnn_layer_depth=depth, max_layers=1, new_edge_prob=0.0,
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
