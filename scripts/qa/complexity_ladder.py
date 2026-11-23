#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Complexity ladder: the minimum AARNN complexity that keeps a robot's
behaviour AND its capacity to adapt and learn.

For each robot the same network runs at every tier of the ladder:

* T0  depth 0 - spikes/graded release only (no synaptic filter, STP,
      thresholds, metabolism; the closest to 0/1 spikes);
* T1  depth 1 - synaptic filtering, short-term plasticity, bouton latency;
* T2  depth 2 - + adaptive thresholds, homeostasis, refractoriness;
* T3  depth 3 - + metabolic/fatigue state at 20 ms;
* T4  depth 3 - + active dendrites and NMDA voltage gating;
* native - the network exactly as built (the behaviour reference);

each with learning off and on (``+L``: STDP/triplet plasticity with
neuromodulation). Every tier switches its mechanisms on explicitly in the
network and in every typed neuron, so a circuit built with
``exact_cells`` is not silently held at its own feature set.

Measured per tier:

* behaviour (closed loop, MuJoCo, lock-step): C. elegans against
  literature ranges (``celegans_depth_study.mimicry``), the other bodies by
  ``probe_embodiment_motion`` metrics against the robot's reference tier;
* adaptation (open loop, ``celegans_depth_probe`` on the robot's own
  baseline senses taken from its reference run):
  - habituation: response to 15 repeated pulses on a channel (index
    1 - late/early response) and its spontaneous recovery;
  - adaptation to a sustained step on a channel (index (peak - late)/peak);
  - conditioning: a neutral channel (CS) paired with an effective one (US),
    against an explicitly unpaired control; learning index = (CS change
    after paired - after unpaired training) / US response. A CS with no
    pathway into the network cannot be learned without growth: reported as
    ``unwired``;
* cost: open-loop milliseconds per 2 ms frame (single thread) and the
  closed-loop brain latency.

The minimum sufficient tier is the lowest one whose behaviour is preserved
and whose adaptation indices pass; that tier's circuit is the candidate
FPAA kernel for the robot.

  python3 scripts/qa/complexity_ladder.py celegans [--skip-closed] [--tiers T0 T1]
"""
import argparse
import concurrent.futures as cf
import importlib.util
import json
import math
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"
OUT = ROOT / "target/qa/complexity"
SDK = Path.home() / "Developer/mujoco-3.17.0"

# --- tiers -----------------------------------------------------------------
BASE_FLAGS = {"aarnn_bio.stp_enabled": True, "aarnn_bio.adaptive_threshold_enabled": True,
              "aarnn_bio.dendritic_active_enabled": False, "aarnn_nmda_voltage_sensitivity": 0.0}
TIERS = {
    "T0": (0, {}),
    "T1": (1, {}),
    "T2": (2, {}),
    "T3": (3, {}),
    "T4": (3, {"aarnn_bio.dendritic_active_enabled": True, "aarnn_nmda_voltage_sensitivity": 0.02}),
}
# Learning: long-term plasticity with neuromodulation; graded (analogue)
# cells take part through their expected spikes (runtime flag added for the
# FPAA-candidate circuits; spiking networks are unaffected by it).
LEARN = {"aarnn_plasticity_enabled": True, "aarnn_bio.neuromodulation_enabled": True,
         "aarnn_graded_plasticity_enabled": True}
# Learning in a specialised subset (``+Ls``): only synapses onto the robot's
# ``plastic`` cells learn; the innate rhythm generators and motor pools keep
# their built ("genetic") weights. C. elegans locomotion runs on motor-circuit
# rhythm generators with proprioceptive coupling (Wen et al. 2012; Fouad et
# al. 2018) while its learning sits at sensory-interneuron synapses; the
# amphid interneurons AIA/AIB/AIY/AIZ are the first integration layer of its
# navigation circuit (Gray, Hill & Bargmann 2005).
PLASTIC = {
    "celegans": ["AIAL", "AIAR", "AIBL", "AIBR", "AIYL", "AIYR", "AIZL", "AIZR"],
    # hindbrain command and direction-integrating cells; spinal CPG innate
    "zebrafish": ["MLR", "RS_l", "RS_r", "ARTR_l", "ARTR_r", "ARTR_A_l", "ARTR_A_r"],
    # sonar relays and the avoidance/back-up decision cells; the gait CPG,
    # delay cells and motor pools innate
    "nao": ["R_Sonar/Left", "P_sonar_l", "R_Sonar/Right", "P_sonar_r", "R_near_Sonar/Left", "N_sonar_l",
            "R_near_Sonar/Right", "N_sonar_r", "BACK", "BACK_A", "REV"],
}
# Spatial / resonant axis (the user's hypothesis: phase interplay of
# overlapping 3D assemblies and energy-guided bouton growth drive learning):
#   S0 point cells, no morphology or conduction delays;
#   S1 3D anatomy with axon/dendrite routes and conduction delays (frozen);
#   S2 + morphology growth: tips seek activity-released energy, synapses form
#      on contact, idle structure is pruned;
#   S3 + resonance (activity re-seeds oscillation) and ambient energy.
SPATIAL = {
    "S0": {"use_morphology": False, "use_aarnn_delays": False, "morpho_growth_enabled": False,
           "aarnn_resonance_gain": 0.0, "aarnn_ambient_energy_level": 0.0},
    "S1": {"use_morphology": True, "use_aarnn_delays": True, "morpho_growth_enabled": False,
           "aarnn_resonance_gain": 0.0, "aarnn_ambient_energy_level": 0.0},
    # Maintenance-level structural turnover: a component idle for ~70 s
    # (half-life) is pruned; activity re-energises it. The developmental
    # constant (0.99 per ms, S2dev) prunes any synapse idle for ~0.5 s and
    # erased the whole frozen C. elegans connectome within 40 s.
    "S2": {"use_morphology": True, "use_aarnn_delays": True, "morpho_growth_enabled": True,
           "aarnn_resonance_gain": 0.0, "aarnn_ambient_energy_level": 0.0, "component_decay_rate": 0.99999},
    "S2dev": {"use_morphology": True, "use_aarnn_delays": True, "morpho_growth_enabled": True,
              "aarnn_resonance_gain": 0.0, "aarnn_ambient_energy_level": 0.0, "component_decay_rate": 0.99},
    "S3": {"use_morphology": True, "use_aarnn_delays": True, "morpho_growth_enabled": True,
           "aarnn_resonance_gain": 0.5, "aarnn_ambient_energy_level": 0.2, "component_decay_rate": 0.99999},
    # Mechanisms in isolation on the frozen anatomy (S1): the global
    # resonance drive without growth, and local proximity coupling - somas
    # within `aarnn_gap_junction_radius` (body units) pull towards each
    # other's membrane potential (linear distance kernel), so overlapping
    # assemblies interact through their phases.
    "S1r": {"use_morphology": True, "use_aarnn_delays": True, "morpho_growth_enabled": False,
            "aarnn_resonance_gain": 0.5, "aarnn_ambient_energy_level": 0.2},
    "S1g": {"use_morphology": True, "use_aarnn_delays": True, "morpho_growth_enabled": False,
            "aarnn_resonance_gain": 0.0, "aarnn_ambient_energy_level": 0.0,
            "aarnn_gap_junction_strength": 0.04, "aarnn_gap_junction_radius": 0.05,
            "aarnn_gap_junction_inhibitory_only": False},
}
# Numeric mechanisms a circuit may have zeroed: give them the depth study's
# values so depth 2 actually has homeostasis and refractoriness.
NUMERIC_DEFAULTS = {"aarnn_bio.homeostasis_gain": 0.25, "aarnn_bio.izh_refractory_ms": 2.0}

# --- robots ----------------------------------------------------------------
# channels by index in the body's sensor list; readout "hidden:<names>" or
# "outputs" (all output channels: graded activation + spikes per window).
ROBOTS = {
    "celegans": dict(
        network="target/qa/celegans-depth-study-forward-crawl/network_grafted.json",
        config="target/qa/celegans-depth-study-forward-crawl/config_grafted.json",
        steps=5000, reference="native",
        habituation=dict(channels=[0, 1, 2], on=1.0, readout="hidden:AVAL,AVAR"),
        step=dict(channels=[15, 16], on=0.6, readout="targets"),
        conditioning=dict(cs=[12], cs_on=0.7, us=[6], us_on=1.0, readout="hidden:AVAL,AVAR"),
        groups=dict(kind="anatomy", key="ganglion", min_size=10),
        capacity=dict(on=0.7, patterns=[[6], [7], [15, 16], [8, 9], [10, 11], [6, 15], [7, 10], [8, 16]]),
    ),
    "hexapod": dict(
        build=["python3", "scripts/build_hexapod_reflex_network_json.py"],
        network="network_hexapod_reflex.json", config="webots_world/configs/config_hexapod_reflex.json",
        steps=5000, reference="native",
        habituation=dict(channels=[18], on=1.0, readout="outputs"),
        step=dict(channels=[0], on_delta=0.05, readout="outputs"),
        conditioning=dict(cs=[32], cs_on=0.8, us=[18], us_on=1.0, readout="outputs"),
        groups=dict(kind="region", min_size=5),   # thoracic ganglia (layout)
        capacity=dict(on=0.9, patterns=[[18], [19], [20], [21], [22], [23], [32], [33]]),
    ),
    "zebrafish": dict(
        build=["python3", "scripts/build_zebrafish_swim_network_json.py"],
        network="network_zebrafish_swim.json", config="webots_world/configs/config_zebrafish_swim.json",
        steps=5000, reference="native",
        habituation=dict(channels=[20], on=0.9, readout="outputs"),
        step=dict(channels=[21], on=0.9, readout="outputs"),
        conditioning=dict(cs=[16], cs_on=0.9, us=[20], us_on=0.9, readout="outputs"),
        # Spatial study (layout from scripts/layout_designed_circuit.py):
        # each spinal segment's E/A/C cells are one assembly.
        groups=dict(kind="region", min_size=5),
        capacity=dict(on=0.8, patterns=[[0, 1], [8, 9], [16], [18], [20], [21], [24], [3, 11]]),
    ),
    "nao": dict(
        build=["python3", "scripts/build_nao_walk_network_json.py"],
        network="network_nao_walk.json", config="webots_world/configs/config_nao_walk.json",
        steps=15000, reference="native",
        habituation=dict(channels=[242], on=0.9, readout="outputs"),
        step=dict(channels=[243], on=0.85, readout="outputs"),
        conditioning=dict(cs=[198], cs_on=1.0, us=[242], us_on=0.9, readout="outputs"),
        groups=dict(kind="region", min_size=3),   # lumbar CPG, brainstem nuclei (layout)
        capacity=dict(on=0.9, patterns=[[242], [243], [198], [222], [200, 201], [224, 225], [247], [244]]),
    ),
}


def tier_overrides(tier, learn):
    depth, extra = TIERS[tier]
    o = dict(BASE_FLAGS)
    o.update(extra)
    o["aarnn_layer_depth"] = depth
    o["aarnn_plasticity_enabled"] = False
    o["aarnn_bio.neuromodulation_enabled"] = False
    o["aarnn_graded_plasticity_enabled"] = False
    if learn:
        o.update(LEARN)
    if learn == "subset":
        o["_plastic_subset"] = True
    return o


def variant_overrides(tier, learn, spatial=None):
    o = tier_overrides(tier, learn)
    if spatial:
        o.update(SPATIAL[spatial])
    return o


def variant_file(name):
    return name.replace("/", "_").replace("+Ls", "_learnsub").replace("+L", "_learn")


def write_variant(src, dst, overrides):
    """Patch net.<field>, net.aarnn_bio.<field> and every typed neuron's
    bio_params.<field> (typed cells carry their own copy)."""
    snap = json.loads(Path(src).read_text())
    net = snap["net"]
    overrides = dict(overrides)
    if overrides.pop("_plastic_subset", False):
        labels = snap.get("connectome_labels", {})
        hidden, outputs = labels.get("hidden_nodes", []), labels.get("output_nodes", [])
        names = set(PLASTIC[overrides.pop("_robot")])
        if net.get("num_hidden_layers", 1) != 1:
            raise SystemExit("plastic subset: only single-layer circuits are mapped")
        mask = dict(enabled=True, hidden=[[h in names for h in hidden]], outputs=[o in names for o in outputs])
        missing = names - set(hidden) - set(outputs)
        if missing:
            raise SystemExit(f"plastic subset: unknown cells {sorted(missing)}")
        net["aarnn_plastic_cells"] = mask
    overrides.pop("_robot", None)
    for key, value in overrides.items():
        if key.startswith("aarnn_bio."):
            field = key.split(".", 1)[1]
            net.setdefault("aarnn_bio", {})[field] = value
            for t in net.get("neuron_types") or []:
                t.setdefault("bio_params", {})[field] = value
        else:
            net[key] = value
    for key, value in NUMERIC_DEFAULTS.items():
        field = key.split(".", 1)[1]
        if not net.get("aarnn_bio", {}).get(field):
            net["aarnn_bio"][field] = value
        for t in net.get("neuron_types") or []:
            if not t.get("bio_params", {}).get(field):
                t["bio_params"][field] = value
    Path(dst).write_text(json.dumps(snap))


# --- closed loop -------------------------------------------------------------
def closed_loop(profile, network, config, out, steps, every=10):
    out.mkdir(parents=True, exist_ok=True)
    s = socket.socket()
    s.bind(("", 0))
    port = s.getsockname()[1]
    s.close()
    env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2", NM_AUTO_AARNN_DEPTH="0",
               NM_AER_REFERENCE_DATA_TIMEOUT_MS="60000", NM_CLUSTER_NODES="1", AARNN_MUJOCO_DIR=str(SDK),
               LD_LIBRARY_PATH=str(SDK / "lib"))
    # Without morphology (S0) the runtime would still rebuild one from the
    # weights at node start-up; for the 302-cell worm that never finishes.
    if not json.loads(Path(network).read_text())["net"].get("use_morphology", True):
        env["NM_REALTIME_DISABLE_MORPHO"] = "1"
    cmd = ["bash", "scripts/run_sim.sh", "--sim", "mujoco", "--robots", f"{profile}=1", "--no-build",
           "--tcp-base-port", str(port), "--tcp-ready-timeout", "300", "--headless", "--steps", str(steps),
           "--lockstep", "--output", str(out / "run.json"), "--trace", str(out / "trace"), "--trace-every", str(every),
           "--network-map", f"{profile}_0={network}", "--config-map", f"{profile}_0={config}"]
    t0 = time.time()
    with open(out / "run.log", "w") as log:
        rc = subprocess.run(cmd, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=7200).returncode
    res = dict(rc=rc, wall_s=round(time.time() - t0, 1))
    try:
        w = json.loads((out / "run.json").read_text())["worlds"][0]
        lat = sorted(w["transport"]["latency_us"])
        res.update(status=w["status"], sim_s=w["time_s"], latency_ms=lat[len(lat) // 2] / 1000 if lat else None,
                   latency_p95_ms=lat[int(0.95 * len(lat))] / 1000 if lat else None,
                   wall_per_sim_s=round(res["wall_s"] / max(w["time_s"], 1e-6), 2))
    except Exception as e:
        res["error"] = str(e)
    return res


def behaviour(profile, out):
    trace = next((out / "trace").glob("*.jsonl"), None)
    if trace is None:
        return dict(error="no trace")
    if profile == "celegans":
        spec = importlib.util.spec_from_file_location("cds", ROOT / "scripts/qa/celegans_depth_study.py")
        cds = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cds)
        m = cds.mimicry(trace, cds.probe.analyse(trace))
        res = {k: v for k, v in m.items() if isinstance(v, (int, float, str, bool))}
        res["mimicry_n"] = sum(bool(v) for v in m.get("within_biological_range", {}).values())
        return res
    p = subprocess.run(["python3", "scripts/qa/probe_embodiment_motion.py", str(trace), "--json"], cwd=ROOT,
                       capture_output=True, text=True)
    try:
        d = json.loads(p.stdout)
    except ValueError:
        return dict(error=p.stderr[-300:])
    return {k: v for k, v in d.items() if isinstance(v, (int, float, bool))}


def baseline_senses(profile, ref_dir, n_sensors):
    trace = next((ref_dir / "trace").glob("*.jsonl"), None) if ref_dir else None
    if trace is None:
        return [0.0 if profile == "celegans" else 0.5] * n_sensors
    rows = [json.loads(l) for l in open(trace)][1:]
    rows = [r for r in rows if r.get("sensors")]
    S = np.array([r["sensors"] for r in rows[len(rows) // 4:]], dtype=float)
    return [float(v) for v in np.median(S, axis=0)]


# --- open loop ----------------------------------------------------------------
def run_probe(network, profile, schedule, ms, seed=1, tag="p", series_ms=50, full=False):
    """schedule(t_s) -> sensor vector. Returns (times_ms, hidden_hz, outputs,
    hidden_names, ms_per_frame[, probe output])."""
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        with open(tmp / "in.jsonl", "w") as f:
            f.write(json.dumps({"profile": profile, "sensor_names": names, "output_names": io["output_names"],
                                "trace_every_steps": 10}) + "\n")
            for step in range(0, ms // 2 + 10, 10):
                t = step * 0.002
                f.write(json.dumps({"time_s": t, "step": step, "sensors": schedule(t)}) + "\n")
        cmd = [str(PROBE), "--network", str(network), "--ms", str(ms), "--input", str(tmp / "in.jsonl"),
               "--out", str(tmp / "o.json"), "--series", str(tmp / "s.jsonl"), "--series-ms", str(series_ms),
               "--no-raster", "--seed", str(seed)]
        env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="1")
        p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
        if p.returncode != 0:
            raise RuntimeError(f"probe failed ({tag}): {p.stderr[-400:]}")
        o = json.loads((tmp / "o.json").read_text())
        rows = [json.loads(l) for l in open(tmp / "s.jsonl")]
    t = np.array([r["t_ms"] for r in rows], dtype=float)
    H = np.array([r["hidden_hz"] for r in rows], dtype=float)
    act = np.array([r.get("output_activation", []) for r in rows], dtype=float)
    spk = np.array([r.get("output_spikes", []) for r in rows], dtype=float)
    outs = (act if act.size else 0) + (spk / float(series_ms) if spk.size else 0)  # spikes per ms-equivalent
    if full:
        return t, H, np.atleast_2d(outs), o["hidden_names"], o.get("ms_per_frame"), o
    return t, H, np.atleast_2d(outs), o["hidden_names"], o.get("ms_per_frame")


def readout(spec, t, H, outs, hidden_names, network, channels):
    if spec.startswith("hidden:"):
        idx = [hidden_names.index(n) for n in spec.split(":", 1)[1].split(",") if n in hidden_names]
        return H[:, idx].sum(axis=1)
    if spec == "targets":
        snap = json.loads(Path(network).read_text())
        w = snap["w_in"][0] if isinstance(snap["w_in"], list) else snap["w_in"]
        W = np.array(w["data"], dtype=float).reshape(w["rows"], w["cols"])
        idx = sorted({int(i) for c in channels for i in np.where(np.abs(W[:, c]) > 0)[0]})
        return H[:, idx].sum(axis=1) if idx else np.zeros(len(t))
    return np.abs(outs).sum(axis=1) if outs.size else np.zeros(len(t))


def evoked(r, t_ms, at_s, win_s=1.0, pre_s=0.5):
    t = t_ms / 1000.0
    post = r[(t > at_s) & (t <= at_s + win_s)]
    pre = r[(t > at_s - pre_s) & (t <= at_s)]
    if not len(post) or not len(pre):
        return 0.0
    return float(np.abs(post - pre.mean()).mean())


def wired(network, channels):
    snap = json.loads(Path(network).read_text())
    w = snap["w_in"][0] if isinstance(snap["w_in"], list) else snap["w_in"]
    W = np.array(w["data"], dtype=float).reshape(w["rows"], w["cols"])
    return all(np.abs(W[:, c]).sum() > 0 for c in channels)


def habituation(robot, profile, network, base, seed=1):
    h = robot["habituation"]
    pulses = [5.0 + 3.0 * k for k in range(15)]
    recovery = pulses[-1] + 23.0

    def sched(t):
        v = list(base)
        on = any(p <= t < p + 0.3 for p in pulses + [recovery])
        for c in h["channels"]:
            if on:
                v[c] = h.get("on", min(1.0, base[c] + h.get("on_delta", 0.0)))
        return v
    t, H, outs, hn, mpf = run_probe(network, profile, sched, int((recovery + 3) * 1000), seed=seed, tag="hab")
    r = readout(h["readout"], t, H, outs, hn, network, h["channels"])
    resp = [evoked(r, t, p) for p in pulses]
    rec = evoked(r, t, recovery)
    first = float(np.mean(resp[:3]))
    last = float(np.mean(resp[-3:]))
    return dict(responses=[round(x, 3) for x in resp], first=round(first, 3), last=round(last, 3),
                index=round(1 - last / first, 3) if first > 1e-6 else None,
                recovery=round(rec / first, 3) if first > 1e-6 else None, ms_per_frame=mpf,
                wired=wired(network, h["channels"]))


def step_adaptation(robot, profile, network, base, seed=1):
    s = robot["step"]
    on_at, off_at = 5.0, 35.0

    def sched(t):
        v = list(base)
        if on_at <= t < off_at:
            for c in s["channels"]:
                v[c] = s.get("on", min(1.0, base[c] + s.get("on_delta", 0.0)))
        return v
    t, H, outs, hn, mpf = run_probe(network, profile, sched, int((off_at + 5) * 1000), seed=seed, tag="step")
    r = readout(s["readout"], t, H, outs, hn, network, s["channels"])
    ts = t / 1000.0
    pre = r[(ts > 2) & (ts <= on_at)].mean()
    dev = np.abs(r - pre)
    win = (ts > on_at) & (ts <= off_at)
    # peak of the 0.5 s smoothed deviation in the first 5 s; late = last 5 s
    k = max(1, int(500 / 50))
    sm = np.convolve(dev, np.ones(k) / k, mode="same")
    early = sm[(ts > on_at) & (ts <= on_at + 5)]
    late = dev[(ts > off_at - 5) & (ts <= off_at)]
    peak = float(early.max()) if len(early) else 0.0
    lt = float(late.mean()) if len(late) else 0.0
    return dict(peak=round(peak, 3), late=round(lt, 3), index=round((peak - lt) / peak, 3) if peak > 1e-6 else None,
                ms_per_frame=mpf, wired=wired(network, s["channels"]), active_fraction=round(float(win.mean()), 2))


def conditioning(robot, profile, network, base, seed=1):
    """Paired CS-US training against an explicitly unpaired control (same
    stimuli, US 2 s after each CS): learning index = (CS change after paired
    training - CS change after unpaired training) / US response. Without the
    control, sensitisation and slow state changes read as learning."""
    cfg = robot["conditioning"]
    pre = [5.0, 9.0, 13.0]
    train = [17.0 + 4.0 * k for k in range(20)]
    post = [train[-1] + 6.0 + 4.0 * k for k in range(3)]
    us_probe = post[-1] + 6.0

    def protocol(paired):
        def sched(t):
            v = list(base)
            cs = any(p <= t < p + 1.0 for p in pre + train + post)
            if paired:
                us = any(p + 0.6 <= t < p + 1.0 for p in train)
            else:
                us = any(p + 2.6 <= t < p + 3.0 for p in train)
            us = us or (us_probe <= t < us_probe + 0.4)
            if cs:
                for c in cfg["cs"]:
                    v[c] = cfg["cs_on"]
            if us:
                for c in cfg["us"]:
                    v[c] = cfg["us_on"]
            return v
        t, H, outs, hn, mpf = run_probe(network, profile, sched, int((us_probe + 3) * 1000), seed=seed,
                                        tag="cond" if paired else "unpaired")
        r = readout(cfg["readout"], t, H, outs, hn, network, cfg["us"])
        cs_pre = float(np.mean([evoked(r, t, p, win_s=0.6) for p in pre]))
        cs_post = float(np.mean([evoked(r, t, p, win_s=0.6) for p in post]))
        return cs_pre, cs_post, evoked(r, t, us_probe, win_s=0.4), mpf
    a_pre, a_post, us, mpf = protocol(True)
    b_pre, b_post, us_b, _ = protocol(False)
    us_ref = max(us, us_b)
    delta = (a_post - a_pre) - (b_post - b_pre)
    return dict(cs_pre=round(a_pre, 3), cs_post=round(a_post, 3), unpaired_pre=round(b_pre, 3),
                unpaired_post=round(b_post, 3), us=round(us_ref, 3),
                index=round(delta / us_ref, 3) if us_ref > 1e-6 else None,
                paired_only_index=round((a_post - a_pre) / us_ref, 3) if us_ref > 1e-6 else None,
                cs_wired=wired(network, cfg["cs"]), us_wired=wired(network, cfg["us"]), ms_per_frame=mpf)


def neuron_groups(robot, network, hidden_names):
    """Assemblies for the resonance test: anatomical groups (C. elegans
    ganglia) or the circuit's own region labels."""
    g = robot.get("groups") or {}
    snap = json.loads(Path(network).read_text())
    groups = {}
    if g.get("kind") == "anatomy":
        anatomy = snap.get("connectome_labels", {}).get("neuron_anatomy", {})
        for j, n in enumerate(hidden_names):
            key = anatomy.get(n, {}).get(g.get("key", "ganglion"))
            if key:
                groups.setdefault(key, []).append(j)
    elif g.get("kind") == "region":
        # Designed circuits: the soma's region label (needs a layout).
        layer = snap["topo"]["layers"][0]
        index = {n: j for j, n in enumerate(snap.get("connectome_labels", {}).get("hidden_nodes") or [])}
        for j, n in enumerate(hidden_names):
            k = index.get(n, j)
            key = layer[k].get("region_name") if k < len(layer) else None
            if key:
                groups.setdefault(key, []).append(j)
    return {k: v for k, v in groups.items() if len(v) >= g.get("min_size", 5)}


def welch(x, fs, seg):
    n = int(seg * fs)
    step = n // 2
    win = np.hanning(n)
    segs = [x[i:i + n] for i in range(0, len(x) - n + 1, step)]
    F = [np.fft.rfft((s_ - s_.mean()) * win) for s_ in segs]
    return np.fft.rfftfreq(n, 1 / fs), F


def resonance(robot, profile, network, base, ref_trace, seed=1):
    """Replay the reference run's sensory stream (crawling, swimming,
    walking) and measure, per assembly, the dominant frequency and how much
    power sits in its peak, and between assemblies the coherence and the
    spread of their phases at the network's dominant frequency."""
    rows = [json.loads(l) for l in open(ref_trace)][1:] if ref_trace else []
    rows = [r for r in rows if r.get("sensors")]
    period = (rows[-1]["time_s"] - rows[0]["time_s"]) if len(rows) > 1 else 0.0

    def sched(t):
        if not rows or period <= 0:
            return list(base)
        tt = rows[0]["time_s"] + (t % period)
        k = min(len(rows) - 1, max(0, int((tt - rows[0]["time_s"]) / period * (len(rows) - 1))))
        return rows[k]["sensors"]
    ms = 40000
    t, H, outs, hn, mpf, o = run_probe(network, profile, sched, ms, seed=seed, tag="res", series_ms=10, full=True)
    routes_end = o.get("routes", {})
    groups = neuron_groups(robot, network, hn)
    fs = 100.0
    keep = t > 5000
    band = None
    spectra, series = {}, {}
    for name, idx in groups.items():
        x = H[keep][:, idx].mean(axis=1)
        series[name] = x
        f, F = welch(x, fs, 4.0)
        P = np.mean([np.abs(z) ** 2 for z in F], axis=0)
        band = (f >= 0.3) & (f <= 20.0)
        if P[band].sum() <= 0:
            continue
        k = int(np.argmax(np.where(band, P, 0)))
        near = band & (np.abs(f - f[k]) <= 0.5)
        spectra[name] = dict(peak_hz=round(float(f[k]), 2), peak_fraction=round(float(P[near].sum() / P[band].sum()), 3))
    names = list(spectra)
    coh, phases = [], []
    if len(names) >= 2:
        # network frequency: median of the assemblies' peaks
        f0 = float(np.median([spectra[n]["peak_hz"] for n in names]))
        for a in range(len(names)):
            for b in range(a + 1, len(names)):
                f, Fa = welch(series[names[a]], fs, 4.0)
                _, Fb = welch(series[names[b]], fs, 4.0)
                Sab = np.mean([x * np.conj(y) for x, y in zip(Fa, Fb)], axis=0)
                Saa = np.mean([np.abs(x) ** 2 for x in Fa], axis=0)
                Sbb = np.mean([np.abs(y) ** 2 for y in Fb], axis=0)
                C = np.abs(Sab) ** 2 / np.maximum(Saa * Sbb, 1e-30)
                k0 = int(np.argmin(np.abs(f - f0)))
                coh.append(float(C[k0]))
                phases.append(float(np.angle(Sab[k0])))
    spread = float(1 - abs(np.mean(np.exp(1j * np.array(phases))))) if phases else None
    return dict(assemblies={n: len(groups[n]) for n in names}, spectra=spectra,
                network_hz=round(float(np.median([spectra[n]["peak_hz"] for n in names])), 2) if names else None,
                peak_fraction=round(float(np.mean([spectra[n]["peak_fraction"] for n in names])), 3) if names else None,
                coherence=round(float(np.mean(coh)), 3) if coh else None,
                phase_spread=round(spread, 3) if spread is not None else None, ms_per_frame=mpf,
                routes_end=routes_end)


def capacity(robot, profile, network, base, seed=1):
    """Dense packing: 8 stimulus patterns x 6 presentations in a fixed
    shuffled order; each response is the vector of every cell's evoked
    activity. Leave-one-out nearest-centroid accuracy (chance 1/8), the
    patterns' effective dimensionality, and with learning the change from
    the first to the second half."""
    cfg = robot.get("capacity")
    if not cfg:
        return dict(error="no capacity patterns for this robot")
    pats = cfg["patterns"]
    order = [i for _ in range(6) for i in range(len(pats))]
    rng = np.random.default_rng(7)
    rng.shuffle(order)
    onsets = [5.0 + 3.0 * k for k in range(len(order))]
    end = onsets[-1] + 3.0

    def sched(t):
        v = list(base)
        for on, pi in zip(onsets, order):
            if on <= t < on + 1.0:
                for c in pats[pi]:
                    v[c] = cfg["on"]
                break
        return v
    t, H, outs, hn, mpf = run_probe(network, profile, sched, int(end * 1000), seed=seed, tag="cap", series_ms=50)
    ts = t / 1000.0
    X = []
    for on in onsets:
        post = H[(ts > on + 0.2) & (ts <= on + 1.0)].mean(axis=0)
        pre = H[(ts > on - 0.5) & (ts <= on)].mean(axis=0)
        X.append(post - pre)
    X = np.array(X)
    y = np.array(order)

    def loo(idx):
        ok = 0
        for i in idx:
            cents = []
            for c in range(len(pats)):
                m = [j for j in idx if y[j] == c and j != i]
                cents.append(X[m].mean(axis=0) if m else np.full(X.shape[1], np.nan))
            d = []
            for cvec in cents:
                if np.isnan(cvec).any() or X[i].std() < 1e-9 or cvec.std() < 1e-9:
                    d.append(np.inf)
                else:
                    d.append(1 - np.corrcoef(X[i], cvec)[0, 1])
            ok += int(np.argmin(d) == y[i])
        return ok / max(len(idx), 1)
    half = len(order) // 2
    cents = np.array([X[y == c].mean(axis=0) for c in range(len(pats))])
    ev = np.linalg.eigvalsh(np.cov(cents.T) if cents.shape[1] > 1 else np.eye(1))
    ev = np.clip(ev, 0, None)
    pr = float(ev.sum() ** 2 / max((ev ** 2).sum(), 1e-30)) if ev.sum() > 0 else 0.0
    return dict(accuracy=round(loo(range(len(order))), 3), accuracy_first_half=round(loo(range(half)), 3),
                accuracy_second_half=round(loo(range(half, len(order))), 3), dimensionality=round(pr, 2),
                chance=round(1 / len(pats), 3), ms_per_frame=mpf)


def growth_counts(network, profile, ms, seed=1):
    """Routes and nonzero synapses after ``ms`` of quiet input (start: 4 ms)."""
    n_s = len(json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]["sensors"])
    *_, o = run_probe(network, profile, lambda t: [0.0] * n_s, ms, seed=seed, tag="grow", full=True)
    return o.get("routes", {})


# --- report ------------------------------------------------------------------
# Pass marks (engineering thresholds, stated in the report).
PASS = dict(habituation=0.3, recovery=0.5, step=0.3, conditioning=0.1, behaviour_tol=0.3)
KEY_METRICS = ("net_displacement_m", "travel_speed_L_s", "dominant_joint_hz")


def behaviour_ok(profile, b, ref):
    if not b or "error" in b:
        return False, "no behaviour"
    if profile == "celegans":
        n, nr = b.get("mimicry_n", 0), (ref or {}).get("mimicry_n", 0)
        return n >= max(nr - 1, 1), f"{n}/7 biology ranges (reference {nr}/7)"
    if b.get("fell"):
        return False, "fell"
    bad = []
    for k in KEY_METRICS:
        v, r = b.get(k), (ref or {}).get(k)
        if v is None or r in (None, 0):
            continue
        if abs(v - r) > PASS["behaviour_tol"] * abs(r):
            bad.append(f"{k} {v:.3g} vs {r:.3g}")
    return not bad, "; ".join(bad) or "within 30 % of reference"


def render(report, robot):
    profile = report["robot"]
    tiers = report["tiers"]
    order = (["native"] if "native" in tiers else []) + [t + l for t in TIERS for l in ("", "+L") if t + l in tiers]
    ref = tiers.get(robot["reference"], {}).get("closed", {}).get("behaviour")
    rows, minimum = [], {}
    for name in order:
        d = tiers[name]
        b = d.get("closed", {}).get("behaviour")
        ok, why = behaviour_ok(profile, b, ref) if b is not None else (None, "not run")
        o = d.get("open", {})
        hab, st, co = o.get("habituation", {}), o.get("step", {}), o.get("conditioning", {})
        caps = dict(
            behaviour=ok,
            habituation=(hab.get("index") or -9) >= PASS["habituation"] and (hab.get("recovery") or 0) >= PASS["recovery"],
            adaptation=(st.get("index") or -9) >= PASS["step"],
            conditioning=co.get("cs_wired") and (co.get("index") or -9) >= PASS["conditioning"],
        )
        for cap, v in caps.items():
            if v and cap not in minimum and name != "native":
                minimum[cap] = name
        cost = hab.get("ms_per_frame") or st.get("ms_per_frame")
        lat = d.get("closed", {}).get("latency_ms")
        rows.append((name, ok, why, hab.get("index"), hab.get("recovery"), st.get("index"),
                     co.get("index") if co.get("cs_wired") else "unwired", cost, lat))
    md = [f"# Complexity ladder: {profile}", "",
          f"Network: `{report.get('source_network')}`; open-loop baseline senses from `{report.get('baseline_from')}`.", "",
          "| tier | behaviour | detail | habituation | recovery | step adaptation | conditioning | open-loop ms/frame | closed-loop latency ms |",
          "|---|---|---|---|---|---|---|---|---|"]
    fmt = lambda x: "-" if x is None else (f"{x:.3g}" if isinstance(x, float) else str(x))
    for r in rows:
        md.append("| " + " | ".join([r[0], {True: "yes", False: "no", None: "-"}[r[1]], r[2]] + [fmt(x) for x in r[3:]]) + " |")
    md += ["", "Pass marks: habituation index >= %.1f with recovery >= %.1f; step adaptation index >= %.1f; "
           "conditioning index >= %.1f (CS must be wired); behaviour within %d %% of the reference tier (%s) on %s, "
           "or for C. elegans within one biology range of it." % (PASS["habituation"], PASS["recovery"], PASS["step"],
                                                                  PASS["conditioning"], PASS["behaviour_tol"] * 100,
                                                                  robot["reference"], ", ".join(KEY_METRICS)), "",
           "Lowest tier with each capacity: " + ", ".join(f"{k} {minimum.get(k, 'none')}" for k in
                                                         ("behaviour", "habituation", "adaptation", "conditioning")) + "."]
    return "\n".join(md) + "\n", minimum


def aggregate(per_seed):
    """Mean of numeric fields over seeds (and the index's spread); other
    fields from the first seed."""
    ok = [r for r in per_seed if "error" not in r]
    if not ok:
        return per_seed[0] if per_seed else {}
    out = dict(ok[0])
    for k, v in ok[0].items():
        vals = [r.get(k) for r in ok if isinstance(r.get(k), (int, float)) and not isinstance(r.get(k), bool)]
        if len(vals) == len(ok) and isinstance(v, (int, float)) and not isinstance(v, bool):
            out[k] = round(float(np.mean(vals)), 4)
            if k == "index" and len(vals) > 1:
                out["index_spread"] = round(float(max(vals) - min(vals)), 4)
    out["seeds"] = len(ok)
    return out


def render_spatial(report, robot):
    profile = report["robot"]
    tiers = report["tiers"]
    ref = tiers.get(robot["reference"], {}).get("closed", {}).get("behaviour")
    fmt = lambda x: "-" if x is None else (f"{x:.3g}" if isinstance(x, float) else str(x))
    md = [f"# Spatial / resonance axis: {profile}", "",
          f"Network: `{report.get('source_network')}`; levels {', '.join(report.get('spatial', []))}. "
          "S0 point cells; S1 3D anatomy + conduction delays (frozen); S2 + energy-guided growth and pruning; "
          "S3 + resonance and ambient energy. Open-loop measures are means over seeds.", "",
          "| variant | behaviour | conditioning (controlled) | habituation | step | network Hz | peak power fraction | coherence | phase spread | capacity accuracy (1st -> 2nd half) | dimensionality | w_rec nonzero start -> end | routes start -> end | ms/frame |",
          "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for name, d in tiers.items():
        if "/" not in name:
            continue
        b = d.get("closed", {}).get("behaviour")
        ok, why = behaviour_ok(profile, b, ref) if b is not None else (None, "not run")
        o = d.get("open", {})
        co, hab, st = o.get("conditioning", {}), o.get("habituation", {}), o.get("step", {})
        rs, cap = o.get("resonance", {}), o.get("capacity", {})
        r0, r1 = o.get("routes_start", {}) or {}, rs.get("routes_end", {}) or {}
        cond = co.get("index")
        if co and cond is not None and co.get("index_spread") is not None:
            cond = f"{cond:.3g} (+-{co['index_spread'] / 2:.2g})"
        md.append("| " + " | ".join([
            name, {True: "yes", False: "no", None: "-"}[ok] + ("" if ok is None else f" ({why})"),
            fmt(cond) if not isinstance(cond, str) else cond, fmt(hab.get("index")), fmt(st.get("index")),
            fmt(rs.get("network_hz")), fmt(rs.get("peak_fraction")), fmt(rs.get("coherence")), fmt(rs.get("phase_spread")),
            (f"{fmt(cap.get('accuracy'))} ({fmt(cap.get('accuracy_first_half'))} -> {fmt(cap.get('accuracy_second_half'))})" if cap else "-"),
            fmt(cap.get("dimensionality")),
            f"{r0.get('w_rec_nonzero', '-')} -> {r1.get('w_rec_nonzero', '-')}",
            f"{r0.get('rec0', '-')} -> {r1.get('rec0', '-')}",
            fmt(rs.get("ms_per_frame") or co.get("ms_per_frame"))]) + " |")
    md += ["", "Capacity chance level = 1/8 = 0.125. Coherence = mean magnitude-squared coherence between assemblies at "
           "the network frequency; phase spread = 1 - |mean phase vector| of their pairwise lags (0 = all in phase)."]
    return "\n".join(md) + "\n"


# --- driver --------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("robot", choices=sorted(ROBOTS))
    ap.add_argument("--tiers", nargs="+", default=["native"] + list(TIERS))
    ap.add_argument("--learn", choices=("both", "off", "on", "subset", "all3", "learn2"), default="both",
                    help="all3 = off, +L (every cell learns) and +Ls (only the PLASTIC subset learns); "
                         "learn2 = +L and +Ls")
    ap.add_argument("--skip-closed", action="store_true")
    ap.add_argument("--skip-open", action="store_true")
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--steps", type=int)
    ap.add_argument("--report-only", action="store_true")
    ap.add_argument("--spatial", nargs="+", choices=list(SPATIAL), help="cross the tiers with these spatial levels")
    ap.add_argument("--seeds", nargs="+", type=int, default=[1])
    ap.add_argument("--tests", nargs="+", default=["habituation", "step", "conditioning"],
                    choices=["habituation", "step", "conditioning", "resonance", "capacity"])
    ap.add_argument("--report-name", default="report", help="report file stem (report.json / REPORT.md)")
    ap.add_argument("--network", type=Path, help="source network instead of the robot's (e.g. a 3D layout)")
    ap.add_argument("--resume", action="store_true",
                    help="skip closed/open runs of variants already in the report")
    args = ap.parse_args()
    robot = ROBOTS[args.robot]
    profile = args.robot
    out = OUT / profile
    out.mkdir(parents=True, exist_ok=True)
    rjson = out / f"{args.report_name}.json"
    rmd = out / ("REPORT.md" if args.report_name == "report" else f"{args.report_name.upper()}.md")
    if args.report_only:
        rep = json.loads(rjson.read_text())
        md = render_spatial(rep, robot) if rep.get("spatial") else render(rep, robot)[0]
        rmd.write_text(md)
        print(md)
        return
    if robot.get("build") and not (ROOT / robot["network"]).exists():
        subprocess.run(robot["build"], cwd=ROOT, check=True)
    src = (args.network.resolve() if args.network else ROOT / robot["network"])
    config = ROOT / robot["config"]
    variants = []
    for tier in args.tiers:
        if tier == "native":   # the network exactly as built and validated
            path = out / "network_native.json"
            shutil.copyfile(src, path)
            variants.append(("native", path))
            continue
        for sp in (args.spatial or [None]):
            learns = {"both": (False, True), "off": (False,), "on": (True,), "subset": ("subset",),
                      "all3": (False, True, "subset"), "learn2": (True, "subset")}[args.learn]
            for learn in learns:
                name = tier + (f"/{sp}" if sp else "") + {False: "", True: "+L", "subset": "+Ls"}[learn]
                path = out / f"network_{variant_file(name)}.json"
                ov = variant_overrides(tier, learn, sp)
                if learn == "subset":
                    ov["_robot"] = profile
                write_variant(src, path, ov)
                variants.append((name, path))
    report_path = rjson
    report = json.loads(report_path.read_text()) if report_path.exists() else {}
    report.setdefault("robot", profile)
    report.setdefault("tiers", {})
    report["source_network"] = str(src)
    if args.spatial:
        report["spatial"] = args.spatial
    steps = args.steps or robot["steps"]
    if not args.skip_closed:
        def job(v):
            name, path = v
            d = out / f"closed_{variant_file(name)}"
            shutil.rmtree(d, ignore_errors=True)
            res = closed_loop(profile, path, config, d, steps)
            res["behaviour"] = behaviour(profile, d)
            return name, res
        todo = [v for v in variants
                if not (args.resume and report["tiers"].get(v[0], {}).get("closed", {}).get("behaviour"))]
        with cf.ThreadPoolExecutor(max_workers=args.jobs) as ex:
            futures = []
            for i, v in enumerate(todo):
                futures.append(ex.submit(job, v))
                time.sleep(20 if i < args.jobs else 0)   # stagger port allocation
            for fu in cf.as_completed(futures):
                name, res = fu.result()
                report["tiers"].setdefault(name, {})["closed"] = res
                report_path.write_text(json.dumps(report, indent=1, default=str))
                print(name, "closed", json.dumps(res.get("behaviour", {}))[:300], flush=True)
    if not args.skip_open:
        ref = robot["reference"]
        ref_dir = out / f"closed_{ref}"
        n_sensors = len(json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]["sensors"])
        base = baseline_senses(profile, ref_dir if ref_dir.exists() else None, n_sensors)
        report["baseline_from"] = str(ref_dir) if ref_dir.exists() else "default"

        ref_trace = next((ref_dir / "trace").glob("*.jsonl"), None) if ref_dir.exists() else None
        fns = dict(habituation=habituation, step=step_adaptation, conditioning=conditioning,
                   resonance=lambda *a, seed=1: resonance(*a, ref_trace, seed=seed), capacity=capacity)

        def ojob(v):
            name, path = v
            res = {}
            for label in args.tests:
                per_seed = []
                for seed in args.seeds:
                    try:
                        fn = fns[label]
                        per_seed.append(fn(robot, profile, path, base, seed=seed))
                    except Exception as e:  # keep the ladder going
                        per_seed.append(dict(error=str(e)[:300]))
                res[label] = aggregate(per_seed)
                if len(per_seed) > 1:
                    res[label + "_seeds"] = per_seed
            if args.spatial:
                try:
                    res["routes_start"] = growth_counts(path, profile, 4)
                except Exception as e:
                    res["routes_start"] = dict(error=str(e)[:200])
            return name, res
        todo = [v for v in variants if not (args.resume and report["tiers"].get(v[0], {}).get("open"))]
        with cf.ThreadPoolExecutor(max_workers=max(1, args.jobs * 2)) as ex:
            for name, res in ex.map(ojob, todo):
                report["tiers"].setdefault(name, {})["open"] = res
                report_path.write_text(json.dumps(report, indent=1, default=str))
                keep = ('index', 'first', 'us', 'cs_wired', 'error', 'ms_per_frame', 'accuracy', 'coherence')
                print(name, "open", json.dumps({k: {kk: vv for kk, vv in v.items() if kk in keep}
                                                for k, v in res.items() if isinstance(v, dict)}), flush=True)
    report_path.write_text(json.dumps(report, indent=1, default=str))
    md = render_spatial(report, robot) if report.get("spatial") else render(report, robot)[0]
    rmd.write_text(md)
    print(md)
    print("wrote", report_path)


if __name__ == "__main__":
    main()
