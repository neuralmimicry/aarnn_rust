#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""
Build a Runner-compatible network snapshot JSON from celegans.py connectome code.

The generated file follows the `Snapshot` schema used by `Runner::import_network_json`:
  - net
  - w_in / w_hh_fwd / w_hh_bwd / w_hh_rec / w_out
  - p_in / p_fwd / p_bwd / p_rec / p_out

Mapping strategy:
  - Presynaptic neuron functions (uppercase names) -> the 302 biological neurons
    in a single hidden layer
  - Postsynaptic targets that are never presynaptic functions -> external muscle
    readout channels; they are not additional biological neurons
  - Webots Celegans sensory channels (24) -> label-aware projections into known
    C. elegans sensory/interneuronal hidden nodes (not index-only remapping).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from collections import Counter
from pathlib import Path
from typing import Dict, List, Tuple

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import celegans_anatomy as anatomy  # noqa: E402

DEF_RE = re.compile(r"^def\s+([A-Za-z0-9_]+)\s*\(")
# Signed synapse counts. celegans.py encodes inhibitory (GABAergic and RID/RIV)
# neuromuscular synapses as negative integers, e.g. ``+= -5``; the previous
# pattern ``([0-9]+)`` silently dropped all 126 of them.
EDGE_RE = re.compile(r"postsynaptic\['([^']+)'\]\s*\+=\s*(-?\s*[0-9]+)")

# Body-wall readout order is preserved for downstream asset/device names
# (``celegans_o_095_MVULVA``); non-locomotor targets that were previously lost
# with the negative edges are appended after it, never sorted into it.
APPENDED_OUTPUTS: Tuple[str, ...] = ("MANAL",)

TARGET_CELEGANS_NEURONS = 302
# celegans.py encodes 300 synaptically connected neurons; CANL/CANR are part of
# the canonical 302-neuron soma set but are typically omitted in synapse-only
# reconstructions due sparse/absent synaptic entries.
CANONICAL_ISOLATED_NEURONS: List[str] = ["CANL", "CANR"]

# Webots controller sensor channel order for Celegans (must match DeviceMapper order).
CELEGANS_SENSOR_CHANNELS: List[str] = [
    "celegans_s_00_vibration_accel.x",
    "celegans_s_00_vibration_accel.y",
    "celegans_s_00_vibration_accel.z",
    "celegans_s_03_vibration_gyro.x",
    "celegans_s_03_vibration_gyro.y",
    "celegans_s_03_vibration_gyro.z",
    "celegans_s_06_touch_front",
    "celegans_s_07_touch_rear",
    "celegans_s_08_light_left",
    "celegans_s_09_light_right",
    "celegans_s_10_heat_left",
    "celegans_s_11_heat_right",
    "celegans_s_12_taste_front_center",
    "celegans_s_13_taste_front_left",
    "celegans_s_14_taste_front_right",
    "celegans_s_15_chem_left",
    "celegans_s_16_chem_right",
    "celegans_s_17_chem_rear_left",
    "celegans_s_18_chem_rear_right",
    "celegans_s_19_flow_front",
    "celegans_s_20_flow_rear",
    "celegans_s_21_far_front_left",
    "celegans_s_22_far_front_right",
    "celegans_s_23_far_rear",
]

# Proprioceptive body-bend channels (MuJoCo celegans-sensors-v2). Twelve
# curvature samples from head (01) to tail (12), each rectified into a dorsal
# and a ventral channel, so straight posture is silent.
BEND_REGIONS = 12
BEND_CHANNELS: List[str] = [
    f"celegans_s_{24 + 2 * (k - 1) + j:02d}_bend_{side}{k:02d}"
    for k in range(1, BEND_REGIONS + 1)
    for j, side in enumerate(("dorsal", "ventral"))
]


def bend_region_ap(k: int) -> float:
    """AP fraction (0 nose .. 1 tail) of bend sample ``k`` (1-based)."""
    return 0.06 + 0.88 * (k - 1) / float(BEND_REGIONS - 1)


# Biologically equivalent sensory projection (celegans-sensory-map-v2).
# Each robot channel drives the neuron classes that transduce the matching
# stimulus in the worm, on the matching side.  Gains are relative.
#   touch:  gentle touch ALM/AVM (anterior), PLM (posterior); nose touch
#           ASH/FLP/OLQ/IL1 (Chalfie 1985; Kaplan & Horvitz 1993)
#   tap/vibration: the six touch-receptor neurons (Wicks & Rankin 1995)
#   light:  LITE-1 cells ASJ, ASK, ASH, AWB, ASI (Ward 2008; Edwards 2008)
#   heat:   AFD primary, AWC and ASI secondary; FLP noxious (Mori 1995;
#           Kuhara 2008; Liu 2012)
#   taste:  centre = water-soluble attractant (ASE, ASG, ASI, ADF);
#           left/right = water-soluble repellent (ASH, ADL) (Bargmann 2006)
#   chem:   head = volatile attractant (AWA, AWC); rear = phasmid repellent
#           (PHA, PHB) (Hilliard 2002)
#   flow:   ambient O2 tension, O2-ON cells URX/AQR (head) and PQR (tail)
#           (Zimmer 2009); BAG (O2-OFF/CO2-ON) has no robot equivalent
#   far:    ray proximity has no worm equivalent; mapped weakly to the
#           anticipatory nose/texture mechanoreceptors OLQ/IL1/CEP and the
#           posterior PVD/PLM so that only near contact is supra-threshold
#   inertial rotation: whole-body proprioception DVA/PVD (Li 2006; Tao 2019)
#   bend:   B-type motor neurons sense curvature of the body anterior to
#           their soma (Wen 2012); SMD/RMD head proprioception (Yeon 2018);
#           DVA integrates body stretch
CELEGANS_SENSOR_TO_NEURONS_V2: Dict[str, List[Tuple[str, float]]] = {
    "celegans_s_00_vibration_accel.x": [("ALML", 0.5), ("ALMR", 0.5), ("AVM", 0.5), ("PLML", 0.5), ("PLMR", 0.5), ("PVM", 0.4)],
    "celegans_s_00_vibration_accel.y": [("ALML", 0.5), ("ALMR", 0.5), ("AVM", 0.5), ("PLML", 0.5), ("PLMR", 0.5), ("PVM", 0.4)],
    "celegans_s_00_vibration_accel.z": [("ALML", 0.5), ("ALMR", 0.5), ("AVM", 0.5), ("PLML", 0.5), ("PLMR", 0.5), ("PVM", 0.4)],
    "celegans_s_03_vibration_gyro.x": [("DVA", 0.5), ("PVDL", 0.4), ("PVDR", 0.4)],
    "celegans_s_03_vibration_gyro.y": [("DVA", 0.5), ("PVDL", 0.4), ("PVDR", 0.4)],
    "celegans_s_03_vibration_gyro.z": [("DVA", 0.5), ("PVDL", 0.4), ("PVDR", 0.4)],
    "celegans_s_06_touch_front": [
        ("ASHL", 1.2), ("ASHR", 1.2), ("FLPL", 1.0), ("FLPR", 1.0), ("OLQDL", 0.7), ("OLQDR", 0.7),
        ("OLQVL", 0.7), ("OLQVR", 0.7), ("IL1L", 0.5), ("IL1R", 0.5), ("ALML", 0.9), ("ALMR", 0.9), ("AVM", 0.9),
    ],
    "celegans_s_07_touch_rear": [("PLML", 1.3), ("PLMR", 1.3), ("PVM", 0.6), ("PHCL", 0.6), ("PHCR", 0.6), ("PVDL", 0.5), ("PVDR", 0.5)],
    "celegans_s_08_light_left": [("ASJL", 1.2), ("ASKL", 0.9), ("ASHL", 0.8), ("AWBL", 0.7), ("ASIL", 0.6)],
    "celegans_s_09_light_right": [("ASJR", 1.2), ("ASKR", 0.9), ("ASHR", 0.8), ("AWBR", 0.7), ("ASIR", 0.6)],
    "celegans_s_10_heat_left": [("AFDL", 1.4), ("AWCL", 0.6), ("ASIL", 0.4), ("FLPL", 0.3)],
    "celegans_s_11_heat_right": [("AFDR", 1.4), ("AWCR", 0.6), ("ASIR", 0.4), ("FLPR", 0.3)],
    "celegans_s_12_taste_front_center": [
        ("ASEL", 1.2), ("ASER", 1.2), ("ASGL", 0.5), ("ASGR", 0.5), ("ASIL", 0.4), ("ASIR", 0.4), ("ADFL", 0.4), ("ADFR", 0.4),
    ],
    "celegans_s_13_taste_front_left": [("ASHL", 1.2), ("ADLL", 0.9)],
    "celegans_s_14_taste_front_right": [("ASHR", 1.2), ("ADLR", 0.9)],
    "celegans_s_15_chem_left": [("AWAL", 1.2), ("AWCL", 1.0)],
    "celegans_s_16_chem_right": [("AWAR", 1.2), ("AWCR", 1.0)],
    "celegans_s_17_chem_rear_left": [("PHAL", 1.1), ("PHBL", 1.0)],
    "celegans_s_18_chem_rear_right": [("PHAR", 1.1), ("PHBR", 1.0)],
    "celegans_s_19_flow_front": [("URXL", 1.0), ("URXR", 1.0), ("AQR", 1.0)],
    "celegans_s_20_flow_rear": [("PQR", 1.2)],
    "celegans_s_21_far_front_left": [("OLQDL", 0.35), ("OLQVL", 0.35), ("IL1L", 0.3), ("CEPDL", 0.3), ("CEPVL", 0.3)],
    "celegans_s_22_far_front_right": [("OLQDR", 0.35), ("OLQVR", 0.35), ("IL1R", 0.3), ("CEPDR", 0.3), ("CEPVR", 0.3)],
    "celegans_s_23_far_rear": [("PVDL", 0.3), ("PVDR", 0.3), ("PLML", 0.3), ("PLMR", 0.3)],
}


# Proprioceptive projection parameters (tunable from the command line).
PROPRIO = {"b_gain": 1.2, "head_gain": 0.9, "head_mode": "stretch", "head_extent": 0.15, "rmd_gain": 0.0,
           "b_mode": "curvature", "b_field": 0.5, "b_field_anterior": 0.0,
           "b_curv_gain": 0.0, "b_curv_field": 0.3, "b_origin": "soma"}
# Body position (AP fraction) where each motor neuron acts: the weighted
# centroid of its neuromuscular junctions (filled from the connectome).
ACTUATION_AP: Dict[str, float] = {}


def b_stretch_field(neuron: str) -> List[int]:
    """Bend samples in a B-type neuron's stretch-receptor field (``b_mode``
    ``stretch``): local, posterior over ``b_field`` of the body (Boyle, Berri
    & Cohen 2012 use half the body, N_SR = M/2) and anterior over
    ``b_field_anterior`` (0 by default; an anterior field is the arrangement
    Wen et al. 2012 measured)."""
    soma = anatomy.soma_ap(neuron)
    if PROPRIO["b_origin"] == "muscle":
        # Measure the field from where the neuron acts. B-type junctions lie
        # 0.05-0.15 body lengths posterior to the soma, so a soma-relative
        # "posterior" field half-overlaps the neuron's own muscles.
        soma = ACTUATION_AP.get(neuron, soma)
    half = 0.5 * (bend_region_ap(2) - bend_region_ap(1))
    lo = soma - max(half, PROPRIO["b_field_anterior"])
    field = [k for k in range(1, BEND_REGIONS + 1) if lo <= bend_region_ap(k) <= soma + PROPRIO["b_field"]]
    if not field:
        field = [min(range(1, BEND_REGIONS + 1), key=lambda k: abs(bend_region_ap(k) - soma))]
    return field


def b_curvature_field(neuron: str) -> List[int]:
    """Bend samples anterior to a B-type soma (excluding the local sample)
    whose same-side curvature drives it when ``b_curv_gain`` > 0."""
    soma = anatomy.soma_ap(neuron)
    half = 0.5 * (bend_region_ap(2) - bend_region_ap(1))
    field = [k for k in range(1, BEND_REGIONS + 1)
             if soma - PROPRIO["b_curv_field"] <= bend_region_ap(k) < soma - half]
    return field or [1]


def bend_targets(channel: str, hidden_nodes: List[str]) -> List[Tuple[str, float]]:
    """Proprioceptive targets for one rectified bend channel.

    Body (``b_mode``): ``curvature`` (default) - B-type motor neurons read
    the curvature of the body just anterior to their soma, same side (DB
    dorsal, VB ventral; Wen et al. 2012). This positive feedback carries a
    bend posteriorly but, without a rhythm source, holds it (latch-up).
    ``stretch`` - Boyle, Berri & Cohen (2012): DB (VB) integrates stretch of
    the dorsal (ventral) side, i.e. ventral (dorsal) bend, locally and
    posteriorly over ``b_field`` of the body; the weight is ``b_gain``
    divided by the field size, so a uniform full-scale bend gives every
    neuron the same total drive. This delayed negative feedback lets
    bistable B-type units oscillate and propagate a wave without a CPG.

    Head (``head_mode``): ``stretch`` (default) follows Yeon et al. (2018):
    SMD neurons are stretch receptors of their own side, so a ventral head
    bend (dorsal side stretched) excites SMDD and a dorsal bend excites SMDV.
    That is delayed negative feedback, the head-oscillator reflex. ``bend``
    reproduces the earlier same-side (positive) mapping. Every sample also
    drives DVA weakly (body stretch).
    """
    m = re.match(r"^celegans_s_\d{2}_bend_(dorsal|ventral)(\d{2})$", channel)
    if not m:
        return []
    side_dv, k = m.group(1), int(m.group(2))
    ap = bend_region_ap(k)
    out: List[Tuple[str, float]] = []
    if PROPRIO["b_mode"] == "stretch":
        # A ventral bend stretches the dorsal side: it excites DB.
        cls = "DB" if side_dv == "ventral" else "VB"
        for n in hidden_nodes:
            if anatomy.neuron_class(n) == cls:
                field = b_stretch_field(n)
                if k in field:
                    out.append((n, PROPRIO["b_gain"] / len(field)))
        # Optional same-side curvature from the anterior body (Wen et al.
        # 2012): a dorsal bend ahead excites DB, carrying bends tailward.
        if PROPRIO["b_curv_gain"] > 0:
            same = "DB" if side_dv == "dorsal" else "VB"
            for n in hidden_nodes:
                if anatomy.neuron_class(n) == same:
                    field = b_curvature_field(n)
                    if k in field:
                        out.append((n, PROPRIO["b_curv_gain"] / len(field)))
    else:
        cls = "DB" if side_dv == "dorsal" else "VB"
        for n in hidden_nodes:
            if anatomy.neuron_class(n) != cls:
                continue
            # Each B-type neuron reads the sample nearest its receptive field.
            receptive = max(bend_region_ap(1), anatomy.soma_ap(n) - 0.10)
            nearest = min(range(1, BEND_REGIONS + 1), key=lambda j: abs(bend_region_ap(j) - receptive))
            if nearest == k:
                out.append((n, PROPRIO["b_gain"]))
    if ap <= PROPRIO["head_extent"]:
        stretched = side_dv == "ventral" if PROPRIO["head_mode"] == "stretch" else side_dv == "dorsal"
        prefix = "SMDD" if stretched else "SMDV"
        out += [(n, PROPRIO["head_gain"]) for n in hidden_nodes if n.startswith(prefix)]
        if PROPRIO["rmd_gain"] > 0:
            rmd = "RMDD" if stretched else "RMDV"
            out += [(n, PROPRIO["rmd_gain"]) for n in hidden_nodes if n.startswith(rmd)]
    out.append(("DVA", 0.15))
    return out


# Legacy label-aware projection retained for --sensor-map v1 reproduction.
CELEGANS_SENSOR_TO_NEURONS: Dict[str, List[Tuple[str, float]]] = {
    # IMU / vibration equivalents
    "celegans_s_00_vibration_accel.x": [
        ("IL1DL", 0.9), ("OLQDL", 0.8), ("CEPDL", 0.8), ("ALML", 1.0), ("FLPL", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_00_vibration_accel.y": [
        ("IL1L", 0.9), ("OLQVL", 0.8), ("CEPVL", 0.8), ("ALML", 0.9), ("PVDL", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_00_vibration_accel.z": [
        ("IL1R", 0.9), ("OLQVR", 0.8), ("CEPVR", 0.8), ("ALMR", 0.9), ("PVDR", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_03_vibration_gyro.x": [
        ("IL1DR", 0.8), ("OLQDR", 0.8), ("CEPDR", 0.8), ("ALMR", 0.9), ("FLPR", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_03_vibration_gyro.y": [
        ("IL1VL", 0.8), ("CEPVL", 0.7), ("PVDL", 0.9), ("AQR", 0.7), ("DVA", 1.0),
    ],
    "celegans_s_03_vibration_gyro.z": [
        ("IL1VR", 0.8), ("CEPVR", 0.7), ("PVDR", 0.9), ("PQR", 0.7), ("DVA", 1.0),
    ],
    # Touch / mechanosensation
    "celegans_s_06_touch_front": [
        ("ALML", 1.3), ("ALMR", 1.3), ("AVM", 1.4), ("FLPL", 1.0), ("FLPR", 1.0), ("IL1L", 0.8), ("IL1R", 0.8),
    ],
    "celegans_s_07_touch_rear": [
        ("PLML", 1.3), ("PLMR", 1.3), ("PVM", 1.2), ("PVDL", 1.0), ("PVDR", 1.0), ("DVA", 0.9),
    ],
    # Light / heat equivalents
    "celegans_s_08_light_left": [
        ("ASJL", 1.0), ("AWCL", 0.9), ("AWBL", 0.8), ("ASIL", 0.8),
    ],
    "celegans_s_09_light_right": [
        ("ASJR", 1.0), ("AWCR", 0.9), ("AWBR", 0.8), ("ASIR", 0.8),
    ],
    "celegans_s_10_heat_left": [
        ("AFDL", 1.4), ("ASIL", 0.8), ("ASEL", 0.7),
    ],
    "celegans_s_11_heat_right": [
        ("AFDR", 1.4), ("ASIR", 0.8), ("ASER", 0.7),
    ],
    # Taste / chemical channels
    "celegans_s_12_taste_front_center": [
        ("ASEL", 1.1), ("ASER", 1.1), ("ASGL", 0.8), ("ASGR", 0.8), ("ADFL", 0.8), ("ADFR", 0.8),
    ],
    "celegans_s_13_taste_front_left": [
        ("ASEL", 1.2), ("AWAL", 1.0), ("ASKL", 0.9), ("ASGL", 0.9), ("ADLL", 0.8),
    ],
    "celegans_s_14_taste_front_right": [
        ("ASER", 1.2), ("AWAR", 1.0), ("ASKR", 0.9), ("ASGR", 0.9), ("ADLR", 0.8),
    ],
    "celegans_s_15_chem_left": [
        ("ASHL", 1.1), ("ADLL", 1.0), ("AWCL", 0.9), ("ASEL", 0.9), ("ASKL", 0.8),
    ],
    "celegans_s_16_chem_right": [
        ("ASHR", 1.1), ("ADLR", 1.0), ("AWCR", 0.9), ("ASER", 0.9), ("ASKR", 0.8),
    ],
    "celegans_s_17_chem_rear_left": [
        ("PHAL", 1.0), ("PHBL", 1.0), ("PVDL", 0.8), ("PLML", 0.9),
    ],
    "celegans_s_18_chem_rear_right": [
        ("PHAR", 1.0), ("PHBR", 1.0), ("PVDR", 0.8), ("PLMR", 0.9),
    ],
    # Flow / far-range channels (gas/flow equivalents)
    "celegans_s_19_flow_front": [
        ("URXL", 1.0), ("URXR", 1.0), ("AQR", 0.9), ("BAGL", 0.8), ("BAGR", 0.8),
    ],
    "celegans_s_20_flow_rear": [
        ("PQR", 1.0), ("PHAL", 0.8), ("PHAR", 0.8), ("BAGL", 0.7), ("BAGR", 0.7),
    ],
    "celegans_s_21_far_front_left": [
        ("URXL", 0.9), ("AWCL", 0.8), ("ASJL", 0.8), ("AQR", 0.7),
    ],
    "celegans_s_22_far_front_right": [
        ("URXR", 0.9), ("AWCR", 0.8), ("ASJR", 0.8), ("AQR", 0.7),
    ],
    "celegans_s_23_far_rear": [
        ("PQR", 1.0), ("PHBL", 0.8), ("PHBR", 0.8), ("BAGL", 0.7), ("BAGR", 0.7),
    ],
}

# Normalized region anchors used for explicit per-neuron topology (`topo`) export.
# `net["brain_regions"]` remains in canonical worm-space units from config.rs.
CELEGANS_TOPO_REGIONS: Dict[str, dict] = {
    "head_ganglia": {"shape": "ellipsoid", "center": (0.0, 0.86, 0.0), "radii": (0.34, 0.18, 0.26)},
    "nerve_ring": {"shape": "torus", "center": (0.0, 0.73, 0.0), "R": 0.24, "r": 0.06},
    "ventral_nerve_cord": {
        "shape": "tube",
        "line_from": (0.0, 0.58, -0.18),
        "line_to": (0.0, -0.95, -0.18),
        "radius": 0.050,
    },
    "dorsal_nerve_cord": {
        "shape": "tube",
        "line_from": (0.0, 0.58, 0.18),
        "line_to": (0.0, -0.95, 0.18),
        "radius": 0.045,
    },
    "tail_ganglia": {"shape": "ellipsoid", "center": (0.0, -0.98, 0.0), "radii": (0.22, 0.12, 0.18)},
}

SENSORY_PREFIXES: Tuple[str, ...] = (
    "ADF",
    "ADL",
    "AFD",
    "ALM",
    "ASE",
    "ASG",
    "ASH",
    "ASI",
    "ASJ",
    "ASK",
    "AWA",
    "AWB",
    "AWC",
    "BAG",
    "CEP",
    "FLP",
    "IL",
    "OLQ",
    "PHA",
    "PHB",
    "PHC",
    "PLM",
    "PQR",
    "PVD",
    "PVM",
    "UR",
    "AQR",
    "AVM",
)
MOTOR_PREFIXES: Tuple[str, ...] = (
    "AS",
    "DA",
    "DB",
    "DD",
    "RMD",
    "RME",
    "RMF",
    "RMH",
    "SAB",
    "SIA",
    "SIB",
    "SMB",
    "SMD",
    "VA",
    "VB",
    "VC",
    "VD",
    "DVA",
    "DVB",
    "DVC",
    "AVL",
)
NEUROMOD_PREFIXES: Tuple[str, ...] = ("ADE", "AIM", "NSM", "PDE", "RIM", "RIS")
TAIL_PREFIXES: Tuple[str, ...] = (
    "PH",
    "PLM",
    "PLN",
    "PQR",
    "PVQ",
    "PVT",
    "PVW",
    "LUA",
    "PDA",
    "PDB",
    "PDE",
)
CORD_PREFIXES: Tuple[str, ...] = (
    "AS",
    "DA",
    "DB",
    "DD",
    "DVA",
    "DVB",
    "DVC",
    "VA",
    "VB",
    "VC",
    "VD",
    "PVC",
    "PVP",
    "PVR",
    "PVN",
)


def stable_u01(node_key: str, salt: str) -> float:
    digest = hashlib.blake2b(f"{node_key}|{salt}".encode("utf-8"), digest_size=8).digest()
    return int.from_bytes(digest, "big") / float((1 << 64) - 1)


def stable_gauss(node_key: str, salt: str) -> float:
    u1 = max(1e-12, stable_u01(node_key, f"{salt}:u1"))
    u2 = stable_u01(node_key, f"{salt}:u2")
    return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def pick_one(node_key: str, salt: str, values: List[float]) -> float:
    if not values:
        return 0.0
    idx = int(stable_u01(node_key, salt) * len(values)) % len(values)
    return values[idx]


def sample_clustered_ellipsoid(
    center: Tuple[float, float, float],
    radii: Tuple[float, float, float],
    cluster_center_norm: Tuple[float, float, float],
    cluster_spread_norm: Tuple[float, float, float],
    node_key: str,
    salt: str,
) -> Tuple[float, float, float]:
    nx = cluster_center_norm[0] + stable_gauss(node_key, f"{salt}:x") * cluster_spread_norm[0]
    ny = cluster_center_norm[1] + stable_gauss(node_key, f"{salt}:y") * cluster_spread_norm[1]
    nz = cluster_center_norm[2] + stable_gauss(node_key, f"{salt}:z") * cluster_spread_norm[2]
    norm_sq = nx * nx + ny * ny + nz * nz
    if norm_sq > 1.0:
        scale = 0.98 / math.sqrt(norm_sq)
        nx *= scale
        ny *= scale
        nz *= scale
    return (
        center[0] + radii[0] * nx,
        center[1] + radii[1] * ny,
        center[2] + radii[2] * nz,
    )


def sample_point_in_ellipsoid(
    center: Tuple[float, float, float],
    radii: Tuple[float, float, float],
    node_key: str,
    salt: str,
) -> Tuple[float, float, float]:
    u = stable_u01(node_key, f"{salt}:u")
    v = stable_u01(node_key, f"{salt}:v")
    w = stable_u01(node_key, f"{salt}:w")
    theta = 2.0 * math.pi * u
    cos_phi = max(-1.0, min(1.0, 2.0 * v - 1.0))
    phi = math.acos(cos_phi)
    rr = w ** (1.0 / 3.0)
    sin_phi = math.sin(phi)
    dx = rr * sin_phi * math.cos(theta)
    dy = rr * sin_phi * math.sin(theta)
    dz = rr * math.cos(phi)
    return (
        center[0] + radii[0] * dx,
        center[1] + radii[1] * dy,
        center[2] + radii[2] * dz,
    )


def sample_point_in_torus(
    center: Tuple[float, float, float],
    major_radius: float,
    minor_radius: float,
    node_key: str,
    salt: str,
    theta_override: float | None = None,
) -> Tuple[float, float, float]:
    theta = theta_override if theta_override is not None else (2.0 * math.pi * stable_u01(node_key, f"{salt}:t"))
    phi = 2.0 * math.pi * stable_u01(node_key, f"{salt}:p")
    rr = math.sqrt(stable_u01(node_key, f"{salt}:r")) * minor_radius
    ring = major_radius + rr * math.cos(phi)
    x = center[0] + ring * math.cos(theta)
    y = center[1] + rr * math.sin(phi)
    z = center[2] + ring * math.sin(theta)
    return (x, y, z)


def sample_point_in_tube(
    line_from: Tuple[float, float, float],
    line_to: Tuple[float, float, float],
    radius: float,
    node_key: str,
    salt: str,
    t_override: float | None = None,
) -> Tuple[float, float, float]:
    t = t_override if t_override is not None else stable_u01(node_key, f"{salt}:t")
    angle = 2.0 * math.pi * stable_u01(node_key, f"{salt}:a")
    rr = math.sqrt(stable_u01(node_key, f"{salt}:r")) * radius
    px = line_from[0] + (line_to[0] - line_from[0]) * t
    py = line_from[1] + (line_to[1] - line_from[1]) * t
    pz = line_from[2] + (line_to[2] - line_from[2]) * t
    return (px + rr * math.cos(angle), py, pz + rr * math.sin(angle))


def neuron_prefix(name: str) -> str:
    m = re.match(r"([A-Z]+)", name.upper())
    return m.group(1) if m else name.upper()


def parse_terminal_index(name: str) -> int | None:
    m = re.search(r"(\d+)$", name)
    if not m:
        return None
    try:
        return int(m.group(1))
    except ValueError:
        return None


def infer_side(name: str) -> str:
    n = name.upper()
    if n.endswith("L"):
        return "left"
    if n.endswith("R"):
        return "right"
    if "LEFT" in n:
        return "left"
    if "RIGHT" in n:
        return "right"
    return "midline"


def infer_dorsoventral(name: str) -> str:
    n = name.upper()
    p = neuron_prefix(n)
    if n.startswith("MD") or p in {"DA", "DB", "DD", "DVB", "DVC"}:
        return "dorsal"
    if n.startswith("MV") or p in {"VA", "VB", "VC", "VD", "AS"}:
        return "ventral"
    return "none"


def infer_hidden_type(name: str) -> str:
    if name.startswith(NEUROMOD_PREFIXES):
        return "Neuromodulatory"
    if name.startswith(MOTOR_PREFIXES):
        return "Motor"
    if name.startswith(SENSORY_PREFIXES):
        return "Sensory"
    return "Interneuron"


def infer_hidden_region(name: str, type_name: str) -> str:
    if name.startswith(TAIL_PREFIXES):
        return "tail_ganglia"
    if name.startswith(CORD_PREFIXES):
        return "dorsal_nerve_cord" if infer_dorsoventral(name) == "dorsal" else "ventral_nerve_cord"
    if type_name == "Motor":
        return "dorsal_nerve_cord" if infer_dorsoventral(name) == "dorsal" else "ventral_nerve_cord"
    if type_name == "Sensory":
        if name.startswith(("PHA", "PHB", "PHC", "PLM", "PQR")):
            return "tail_ganglia"
        return "head_ganglia"
    return "nerve_ring"


def infer_hidden_ap_fraction(name: str, region_name: str) -> float:
    jitter = stable_u01(name, "ap")
    if region_name == "head_ganglia":
        return 0.06 + 0.12 * jitter
    if region_name == "nerve_ring":
        return 0.13 + 0.05 * jitter
    if region_name == "tail_ganglia":
        return 0.86 + 0.12 * jitter

    idx = parse_terminal_index(name)
    p = neuron_prefix(name)
    if idx is not None:
        if p == "AS":
            max_idx = 11
        elif p in {"VA", "VB", "VC", "VD", "DA", "DB", "DD"}:
            max_idx = 12
        else:
            max_idx = max(12, idx)
        frac = (max(1, idx) - 1) / float(max(1, max_idx - 1))
    else:
        frac = jitter
    return 0.20 + 0.74 * frac


def infer_sensor_region(channel: str) -> str:
    c = channel.lower()
    if any(k in c for k in ("touch_rear", "chem_rear", "far_rear", "flow_rear")):
        return "tail_ganglia"
    if "vibration" in c:
        return "nerve_ring"
    return "head_ganglia"


def infer_sensor_side(channel: str) -> str:
    c = channel.lower()
    if "left" in c:
        return "left"
    if "right" in c:
        return "right"
    if "rear" in c:
        return "midline"
    return "midline"


def infer_sensor_ap_fraction(channel: str) -> float:
    c = channel.lower()
    jitter = stable_u01(channel, "sensor_ap")
    if "rear" in c:
        return 0.84 + 0.12 * jitter
    if "vibration" in c:
        return 0.12 + 0.08 * jitter
    if "flow_front" in c:
        return 0.17 + 0.05 * jitter
    return 0.07 + 0.10 * jitter


def infer_output_region(name: str) -> str:
    if name.startswith("MD"):
        return "dorsal_nerve_cord"
    return "ventral_nerve_cord"


def infer_output_side(name: str) -> str:
    if name.startswith(("MDL", "MVL")):
        return "left"
    if name.startswith(("MDR", "MVR")):
        return "right"
    return "midline"


def infer_output_ap_fraction(name: str) -> float:
    if name == "MVULVA":
        return 0.56
    idx = parse_terminal_index(name)
    if idx is None:
        return 0.55
    bounded = max(1, min(24, idx))
    return 0.22 + 0.73 * ((bounded - 1) / 23.0)


def sample_celegans_position(
    node_key: str,
    region_name: str,
    side: str,
    ap_fraction: float,
) -> Tuple[float, float, float]:
    region = CELEGANS_TOPO_REGIONS[region_name]
    side_push = -1.0 if side == "left" else 1.0 if side == "right" else 0.0

    if region["shape"] == "ellipsoid":
        if region_name == "head_ganglia":
            if side == "left":
                centers = [(-0.55, 0.20, 0.34), (-0.40, -0.18, 0.06), (-0.34, 0.02, -0.30), (-0.22, 0.24, -0.02)]
            elif side == "right":
                centers = [(0.55, 0.20, 0.34), (0.40, -0.18, 0.06), (0.34, 0.02, -0.30), (0.22, 0.24, -0.02)]
            else:
                centers = [(0.0, 0.18, 0.25), (0.0, -0.12, -0.16), (0.0, 0.02, 0.02)]
            c = centers[int(stable_u01(node_key, "head_cluster") * len(centers)) % len(centers)]
            spread = (0.16, 0.14, 0.14)
            x, y, z = sample_clustered_ellipsoid(region["center"], region["radii"], c, spread, node_key, "head")
        elif region_name == "tail_ganglia":
            if side == "left":
                centers = [(-0.42, 0.14, 0.24), (-0.28, -0.20, 0.02), (-0.20, -0.02, -0.24)]
            elif side == "right":
                centers = [(0.42, 0.14, 0.24), (0.28, -0.20, 0.02), (0.20, -0.02, -0.24)]
            else:
                centers = [(0.0, 0.14, 0.20), (0.0, -0.16, -0.08)]
            c = centers[int(stable_u01(node_key, "tail_cluster") * len(centers)) % len(centers)]
            spread = (0.18, 0.18, 0.18)
            x, y, z = sample_clustered_ellipsoid(region["center"], region["radii"], c, spread, node_key, "tail")
        else:
            x, y, z = sample_point_in_ellipsoid(region["center"], region["radii"], node_key, region_name)
        y += (0.5 - ap_fraction) * 0.06
    elif region["shape"] == "torus":
        if side == "left":
            theta_anchors = [math.pi - 0.55, math.pi - 0.10, math.pi + 0.35]
        elif side == "right":
            theta_anchors = [-0.35, 0.10, 0.55]
        else:
            theta_anchors = [0.5 * math.pi - 0.25, 0.5 * math.pi + 0.25, -0.5 * math.pi]
        theta_base = pick_one(node_key, "ring_theta_anchor", theta_anchors)
        theta = theta_base + stable_gauss(node_key, "ring_theta_jitter") * 0.14
        phi_base = pick_one(node_key, "ring_phi_anchor", [-0.7, -0.2, 0.35, 0.8])
        phi = phi_base + stable_gauss(node_key, "ring_phi_jitter") * 0.16
        rr = math.sqrt(stable_u01(node_key, "ring_rr")) * region["r"]
        ring = region["R"] + rr * math.cos(phi)
        x = region["center"][0] + ring * math.cos(theta)
        y = region["center"][1] + rr * math.sin(phi)
        z = region["center"][2] + ring * math.sin(theta)
        y += (0.5 - ap_fraction) * 0.03
    else:
        base_t = clamp((ap_fraction - 0.20) / 0.74, 0.0, 1.0)
        segment_anchors = [0.06, 0.14, 0.23, 0.32, 0.41, 0.50, 0.61, 0.72, 0.84, 0.94]
        nearest = sorted(segment_anchors, key=lambda s: abs(s - base_t))[:3]
        seg = pick_one(node_key, "tube_segment", nearest)
        t = clamp(seg + stable_gauss(node_key, "tube_t_jitter") * 0.020 + (base_t - seg) * 0.45, 0.0, 1.0)
        x, y, z = sample_point_in_tube(region["line_from"], region["line_to"], region["radius"], node_key, region_name, t_override=t)
        # Segmental dorso-ventral fasciculation.
        z += stable_gauss(node_key, "tube_dv") * 0.010

    x += 0.07 * side_push

    return (
        max(-1.35, min(1.35, x)),
        max(-1.35, min(1.35, y)),
        max(-1.35, min(1.35, z)),
    )


def anatomical_region(name: str) -> str:
    """Map a neuron's soma ganglion onto the five NematodeWorm regions."""
    info = anatomy.class_info(name)
    ap = anatomy.soma_ap(name)
    if ap >= 0.885:
        return "tail_ganglia"
    if info.ganglion == "ventral_cord":
        return "ventral_nerve_cord"
    if ap < 0.17:
        if info.modality in (anatomy.INTER, anatomy.MOTOR):
            return "nerve_ring"
        return "head_ganglia"
    if info.ganglion == "dorsal":
        return "dorsal_nerve_cord"
    return "ventral_nerve_cord"


def anatomical_position(name: str) -> Tuple[float, float, float]:
    """Topology coordinates from the soma map: +y anterior, +x right, +z dorsal.

    ``y`` is linear in body fraction (nose 0.92, tail -0.98, the extent the
    previous heuristic used).  Ventral cord somata sit on the ventral cord
    (z=-0.18) whichever muscle row they drive; bilateral pairs are offset on x.
    A deterministic jitter of at most 0.01 separates otherwise coincident cells.
    """
    ap = anatomy.soma_ap(name)
    info = anatomy.class_info(name)
    side = anatomy.side(name)
    y = 0.92 - 1.90 * ap
    x = {"left": -0.07, "right": 0.07}.get(side, 0.0)
    dv = anatomy.dorsoventral(name)
    if info.ganglion == "ventral_cord":
        z = -0.18
    elif dv == "dorsal":
        z = 0.06
    elif dv == "ventral":
        z = -0.06
    else:
        z = 0.0
    if info.ganglion in ("midbody", "anterior_body", "postdeirid") and side != "midline":
        x *= 2.0  # lateral body-wall somata (ALM, PVD, PDE)
    jx = (stable_u01(name, "jx") - 0.5) * 0.02
    jy = (stable_u01(name, "jy") - 0.5) * 0.02
    jz = (stable_u01(name, "jz") - 0.5) * 0.02
    return (x + jx, y + jy, z + jz)


def build_celegans_topology(hidden_nodes: List[str], output_nodes: List[str],
                            sensory_channels: List[str] | None = None) -> Tuple[dict, Dict[str, int]]:
    region_counts: Counter[str] = Counter()
    sensory_channels = list(CELEGANS_SENSOR_CHANNELS) if sensory_channels is None else sensory_channels

    hidden_layer: List[dict] = []
    for node_id in hidden_nodes:
        type_name = cell_type_name(node_id)
        region_name = anatomical_region(node_id)
        x, y, z = anatomical_position(node_id)
        hidden_layer.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": type_name,
            }
        )
        region_counts[f"hidden:{region_name}"] += 1

    sensory_out: List[dict] = []
    for channel in sensory_channels:
        m = re.match(r"^celegans_s_\d{2}_bend_(dorsal|ventral)(\d{2})$", channel)
        if m:
            ap_fraction = bend_region_ap(int(m.group(2)))
            region_name = "ventral_nerve_cord" if ap_fraction > 0.17 else "head_ganglia"
            if ap_fraction >= 0.885:
                region_name = "tail_ganglia"
            x, y, z = 0.0, 0.92 - 1.90 * ap_fraction, 0.12 if m.group(1) == "dorsal" else -0.12
        else:
            region_name = infer_sensor_region(channel)
            side = infer_sensor_side(channel)
            ap_fraction = infer_sensor_ap_fraction(channel)
            x, y, z = sample_celegans_position(channel, region_name, side, ap_fraction)
        sensory_out.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                # External transducer channels: explicitly excitatory input;
                # bend channels are non-adapting proprioceptors.
                "type_name": "Sensory:proprioceptor" if m else "Sensory:transducer",
            }
        )
        region_counts[f"sensory:{region_name}"] += 1

    output_out: List[dict] = []
    for node_id in output_nodes:
        quadrant = anatomy.muscle_quadrant(node_id)
        region_name = infer_output_region(node_id)
        ap_fraction = anatomy.muscle_ap(node_id)
        if quadrant:
            dv, lr, _row = quadrant
            x = -0.10 if lr == "left" else 0.10
            z = 0.20 if dv == "dorsal" else -0.20
        else:
            x, z = 0.0, -0.20
        y = 0.92 - 1.90 * ap_fraction
        output_out.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": "Muscle:body_wall" if quadrant else "Muscle:non_locomotor",
            }
        )
        region_counts[f"output:{region_name}"] += 1

    topo = {"layers": [hidden_layer], "sensory_nodes": sensory_out, "output_nodes": output_out}
    return topo, {k: int(v) for k, v in sorted(region_counts.items())}


def is_connectome_neuron_fn(name: str) -> bool:
    # Keep original neuron naming convention (uppercase + digits), reject helpers.
    return name.upper() == name and not re.search(r"[a-z_]", name)


def parse_connectome_edges(celegans_path: Path) -> Tuple[List[str], List[str], Counter, List[str]]:
    lines = celegans_path.read_text(encoding="utf-8").splitlines()

    defs: List[Tuple[str, int]] = []
    for idx, line in enumerate(lines):
        m = DEF_RE.match(line)
        if m:
            defs.append((m.group(1), idx))

    defs = sorted(defs, key=lambda x: x[1])
    blocks: Dict[str, List[str]] = {}
    for i, (name, start) in enumerate(defs):
        end = defs[i + 1][1] if i + 1 < len(defs) else len(lines)
        blocks[name] = lines[start:end]

    hidden_nodes = sorted([name for name, _ in defs if is_connectome_neuron_fn(name)])
    added_hidden_nodes: List[str] = []
    for neuron in CANONICAL_ISOLATED_NEURONS:
        if neuron not in hidden_nodes:
            hidden_nodes.append(neuron)
            added_hidden_nodes.append(neuron)
    hidden_nodes = sorted(hidden_nodes)
    if len(hidden_nodes) != TARGET_CELEGANS_NEURONS:
        raise SystemExit(
            f"Expected {TARGET_CELEGANS_NEURONS} hidden neurons after canonical completion; got {len(hidden_nodes)}."
        )

    edges: Counter = Counter()
    post_nodes = set()
    for src in hidden_nodes:
        for line in blocks.get(src, []):
            if line.strip().startswith("#"):
                continue
            for dst, raw in EDGE_RE.findall(line):
                edges[(src, dst)] += int(raw.replace(" ", ""))
                post_nodes.add(dst)
    # A pair whose signed entries cancel is not a synapse.
    for key in [k for k, v in edges.items() if v == 0]:
        del edges[key]

    targets = post_nodes.difference(hidden_nodes)
    output_nodes = sorted(t for t in targets if t not in APPENDED_OUTPUTS)
    output_nodes += [t for t in APPENDED_OUTPUTS if t in targets]
    unknown = sorted(t for t in targets if not t.startswith("M"))
    if unknown:
        raise SystemExit(f"Non-muscle postsynaptic targets outside the neuron set: {unknown}")
    return hidden_nodes, output_nodes, edges, added_hidden_nodes


def apply_polarity(edges: Counter, hidden_nodes: List[str], mode: str) -> Tuple[Dict[Tuple[str, str], int], List[list]]:
    """Signed synapse counts used for the matrices.

    ``source``: celegans.py signs verbatim.  ``transmitter`` (default):
    source magnitudes and source signs for every neuromuscular synapse (the
    source already marks GABAergic and RID/RIV junctions negative); chemical
    neuron-to-neuron synapses of the 26 GABAergic cells (McIntire 1993) are
    inhibitory, since celegans.py only signs neuromuscular junctions.
    Receptor-level exceptions (GluCl, ACC, EXP-1) are not modelled.
    """
    hidden = set(hidden_nodes)
    signed: Dict[Tuple[str, str], int] = {}
    overrides: List[list] = []
    for (pre, post), w in sorted(edges.items()):
        out = w
        if mode == "transmitter" and post in hidden and anatomy.is_gabaergic(pre) and w > 0:
            out = -w
            overrides.append([pre, post, w, out, "GABAergic presynaptic neuron (Dale)"])
        signed[(pre, post)] = out
    return signed, overrides


def build_nematode_regions() -> List[dict]:
    # Mirrors apply_nematode_worm_design() in src/config.rs.
    return [
        {
            "name": "head_ganglia",
            "shape": {
                "shape": "ellipsoid",
                "center": [0.0, 7.0, 0.0],
                "radii": [6.0, 8.0, 6.0],
            },
            "center": [0.0, 7.0, 0.0],
            "radii": [6.0, 8.0, 6.0],
            "type_distribution": [
                ["Sensory", 0.55],
                ["Interneuron", 0.30],
                ["Neuromodulatory", 0.10],
                ["Motor", 0.05],
            ],
        },
        {
            "name": "nerve_ring",
            "shape": {
                "shape": "torus",
                "center": [0.0, 10.0, 0.0],
                "R": 6.0,
                "r": 1.5,
                "plane": "x-z",
            },
            "center": [0.0, 10.0, 0.0],
            "radii": [7.5, 1.5, 7.5],
            "type_distribution": [
                ["Sensory", 0.15],
                ["Interneuron", 0.75],
                ["Neuromodulatory", 0.10],
            ],
        },
        {
            "name": "ventral_nerve_cord",
            "shape": {
                "shape": "tube",
                "line_from": [0.0, 15.0, -6.0],
                "line_to": [0.0, 95.0, -6.0],
                "radius": 1.2,
            },
            "center": [0.0, 55.0, -6.0],
            "radii": [1.2, 40.0, 1.2],
            "type_distribution": [
                ["Sensory", 0.10],
                ["Interneuron", 0.25],
                ["Motor", 0.60],
                ["Neuromodulatory", 0.05],
            ],
        },
        {
            "name": "dorsal_nerve_cord",
            "shape": {
                "shape": "tube",
                "line_from": [0.0, 15.0, 6.0],
                "line_to": [0.0, 95.0, 6.0],
                "radius": 1.0,
            },
            "center": [0.0, 55.0, 6.0],
            "radii": [1.0, 40.0, 1.0],
            "type_distribution": [
                ["Motor", 0.95],
                ["Interneuron", 0.05],
            ],
        },
        {
            "name": "tail_ganglia",
            "shape": {
                "shape": "ellipsoid",
                "center": [0.0, 95.0, 0.0],
                "radii": [5.0, 6.0, 5.0],
            },
            "center": [0.0, 95.0, 0.0],
            "radii": [5.0, 6.0, 5.0],
            "type_distribution": [
                ["Sensory", 0.60],
                ["Interneuron", 0.20],
                ["Motor", 0.15],
                ["Neuromodulatory", 0.05],
            ],
        },
    ]


def make_matrix(rows: int, cols: int) -> List[List[float]]:
    return [[0.0 for _ in range(cols)] for _ in range(rows)]


def flatten_row_major(mat: List[List[float]]) -> List[float]:
    out: List[float] = []
    for row in mat:
        out.extend(row)
    return out


def flatten_row_major_u32(mat: List[List[int]]) -> List[int]:
    out: List[int] = []
    for row in mat:
        out.extend(row)
    return out


def aarnn_laminar_io_layers(hidden_layer_count: int) -> Tuple[int, int]:
    """Map hidden-layer count to canonical AARNN laminar IO layers."""
    if hidden_layer_count <= 0:
        return 0, 0
    sensory_target_layer = 1 if hidden_layer_count > 1 else 0  # L4
    if hidden_layer_count > 4:
        output_source_layer = 4  # L5
    else:
        output_source_layer = hidden_layer_count - 1
    return sensory_target_layer, output_source_layer


def build_sensory_projection(
    hidden_nodes: List[str],
    channels: List[str] | None = None,
    sensor_map: str = "v2",
    input_scale: float = 1.0,
) -> Tuple[List[List[float]], List[List[int]], Dict[str, List[str]]]:
    channels = list(CELEGANS_SENSOR_CHANNELS) if channels is None else channels
    hidden_index = {name: i for i, name in enumerate(hidden_nodes)}
    h_count = len(hidden_nodes)
    s_count = len(channels)
    w_in = make_matrix(h_count, s_count)
    p_in = [[0 for _ in range(s_count)] for _ in range(h_count)]
    used_targets: Dict[str, List[str]] = {}
    table = CELEGANS_SENSOR_TO_NEURONS_V2 if sensor_map == "v2" else CELEGANS_SENSOR_TO_NEURONS

    for s_idx, s_name in enumerate(channels):
        specs = table.get(s_name) or bend_targets(s_name, hidden_nodes)
        picked: List[str] = []
        for n_name, gain in specs:
            h_idx = hidden_index.get(n_name)
            if h_idx is None:
                raise SystemExit(f"sensory map {s_name} names unknown neuron {n_name}")
            w_in[h_idx][s_idx] += float(gain) * input_scale
            p_in[h_idx][s_idx] = max(p_in[h_idx][s_idx], 1)
            picked.append(n_name)

        if not picked and sensor_map == "v2":
            raise SystemExit(f"sensory channel {s_name} has no biological target")
        # Legacy v1 safety fallback: if a channel lost all named targets,
        # connect to a compact sensory integration set.
        if not picked:
            for fallback_name in ("ASEL", "ASER", "AIAL", "AIAR"):
                h_idx = hidden_index.get(fallback_name)
                if h_idx is None:
                    continue
                w_in[h_idx][s_idx] += 0.75
                p_in[h_idx][s_idx] = max(p_in[h_idx][s_idx], 1)
                picked.append(fallback_name)

        used_targets[s_name] = picked

    return w_in, p_in, used_targets


# Izhikevich preset per modality for the C. elegans profile.  Most worm
# neurons are graded rather than spiking; the regular-spiking preset is the
# neutral AARNN choice.  GABAergic motor neurons use fast-spiking so their
# inhibition tracks the B/A-type excitation that drives them.
IZH_PRESET_BY_TYPE = {"GABA": "FS"}
# Body-wall muscle cells integrate many small cholinergic/GABAergic junction
# currents and are electrically coupled within a quadrant; their spiking
# surrogate needs a larger per-synapse efficacy than neuron-neuron synapses.
NMJ_GAIN_DEFAULT = 30.0
# With w = count / 37, a gain of 10 gives a single-synapse charge of ~3.5 mV
# at rest (AMPA+NMDA kernel integral ~13 step-currents): sensory cells reach
# 20-60 Hz under stimulus and the network stays asynchronous (open-loop
# depth probe, examples/celegans_depth_probe.rs, 2026-11-21).
SYNAPTIC_GAIN_DEFAULT = 14.0
# Fast-spiking (weakly adapting) muscle readout cells follow motor-neuron
# drive; regular-spiking cells adapted to 1-5 Hz (closed-loop sweep
# target/qa/celegans-calib*, 2026-11-21).
MUSCLE_PRESET_DEFAULT = "FS"
SYNAPTIC_GAIN_BY_MODALITY = {
    anatomy.SENSORY: 1.0,
    anatomy.INTER: 1.0,
    anatomy.MOTOR: 1.0,
    anatomy.NEUROMOD: 1.0,
    anatomy.PHARYNGEAL: 1.0,
}


GAP_EDGELIST_DEFAULT = "data/celegans/herm_full_edgelist.csv"
GAP_EDGELIST_URL = "https://raw.githubusercontent.com/openworm/c302/master/c302/data/herm_full_edgelist.csv"
# Conductance per gap-junction contact (Izhikevich current units per mV).
# The runtime integrates electrical coupling implicitly, so any value is
# stable. Near rest an Izhikevich cell's input conductance is ~0.6/ms, so a
# 10-contact pair (g = 0.25) has a coupling coefficient g / (g + 0.6) ~ 0.3,
# in the range measured for coupled C. elegans neurons.
GAP_UNIT_DEFAULT = 0.025


def load_electrical_synapses(path: Path, hidden_nodes: List[str]) -> Tuple[Dict[Tuple[str, str], int], dict]:
    """Electrical synapses (gap junctions) from the Cook et al. (2019)
    hermaphrodite edge list distributed with OpenWorm c302.

    Each pair is listed in both directions; the contact count of a pair is
    the larger listing. Neuron names are normalised (``DA01`` -> ``DA1``).
    Muscle-muscle and neuron-muscle electrical contacts are excluded: the
    muscles are external readout channels, not simulated cells.
    """
    import csv

    hidden = set(hidden_nodes)
    pairs: Dict[Tuple[str, str], int] = {}
    excluded = Counter()
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            if row["Type"].strip() != "electrical":
                continue
            a, b = (re.sub(r"^([A-Z]+)0(\d)$", r"\1\2", row[k].strip()) for k in ("Source", "Target"))
            if a not in hidden or b not in hidden:
                excluded["non_neuron_endpoint"] += 1
                continue
            if a == b:
                excluded["self"] += 1
                continue
            key = tuple(sorted((a, b)))
            pairs[key] = max(pairs.get(key, 0), int(row["Weight"]))
    meta = {
        "source": str(path),
        "url": GAP_EDGELIST_URL,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "dataset": "Cook et al. 2019 hermaphrodite connectome (electrical), via OpenWorm c302",
        "pairs": len(pairs),
        "contacts": int(sum(pairs.values())),
        "excluded_records": dict(excluded),
    }
    return pairs, meta


TRANSDUCER_ADAPTATION = {"stp_u": 0.3, "stp_tau_rec_ms": 1500.0, "stp_tau_facil_ms": 50.0}
HEAD_OSCILLATOR_CLASSES = ("SMD", "RMD")
HEAD = {"preset": "RS"}


# Graded (non-spiking) motor neurons and muscles (--motor-model/--muscle-model
# graded). C. elegans ventral-cord and head motor neurons signal with graded
# potentials and tonic transmitter release (Lockery & Goodman 2009; Liu et al.
# 2014; Kato et al. 2015); AVL and DVB fire action potentials (Jiang et al.
# 2022) and stay spiking. B-type motor neurons can be made bistable (plateau,
# Boyle, Berri & Cohen 2012) and RMD/SMD given plateau potentials (Mellem et
# al. 2008). Body-wall muscles fire Ca2+ action potentials whose rate sets a
# graded Ca2+/force level (Gao & Zhen 2011); the graded muscle surrogate models
# that level and the body decodes its pulse-density code linearly.
# These defaults mirror `AarnnBioParams::default()` (graded fields).
GRADED_BIO_OFF = {
    "graded_enabled": False,
    "graded_tau_ms": 30.0,
    "graded_v_rest_mv": -70.0,
    "graded_input_resistance": 5.0,
    "graded_release_v_half_mv": -50.0,
    "graded_release_slope_mv": 5.0,
    "graded_release_max_hz": 50.0,
    "graded_plateau_mv": 0.0,
    "graded_plateau_v_half_mv": -55.0,
    "graded_plateau_slope_mv": 3.0,
    "graded_e_exc_mv": 0.0,
    "graded_e_inh_mv": -80.0,
}
GRADED = {
    "motor": False,
    "muscle": False,
    "neuron_tau_ms": 30.0,
    "input_resistance": 5.0,
    "muscle_input_resistance": 5.0,
    "release_v_half_mv": -50.0,
    # Spike-equivalent rate of full graded release. 50 Hz saturated every
    # graded stage of the connectome (motor neurons, D-type, muscles) under
    # the synaptic gain calibrated for spiking cells; 20 Hz keeps them in
    # range (open-loop replay calibration, target/qa/celegans-graded-*).
    "release_hz": 20.0,
    "muscle_tau_ms": 20.0,
    # One event per 2 ms body frame at full activation.
    "muscle_pulse_hz": 500.0,
    "b_plateau_mv": 0.0,
    "head_plateau_mv": 0.0,
    "plateau_v_half_mv": -55.0,
    "plateau_slope_mv": 3.0,
}
SPIKING_MOTOR_CLASSES = ("AVL", "DVB")
B_CLASSES = ("DB", "VB")
# Command interneurons, likewise graded in the animal (AVA, AVB: Kato et al.
# 2015; Lockery & Goodman 2009); --graded-classes makes any listed class
# graded. --tonic gives a class a constant input current (mV/ms): a
# behavioural-state drive such as the forward command to AVB (Boyle, Berri &
# Cohen 2012 drive B-class neurons with a constant AVB current).
GRADED_CLASSES: List[str] = []
TONIC: Dict[str, float] = {}


# Isolated locomotor circuit test (--isolate bbc): the Boyle, Berri & Cohen
# (2012) forward circuit inside the connectome. DB, VB, DD and VD keep the
# connectome synapses and gap junctions among themselves; every other input
# onto them and every neuromuscular junction from other classes is removed.
# AVB drive is then a tonic current on DB/VB (--tonic DB=.. --tonic VB=..).
# --isolate-add CLASS,... restores the listed classes' connections onto the
# circuit and their junctions onto muscles (add-back test).
BBC_CIRCUIT = ("DB", "VB", "DD", "VD")
# ``bbc-minimal`` keeps only the model's own links inside the circuit: B-type
# excitation of the contralateral-muscle D-type cells and D-type inhibition of
# B-type cells (Boyle, Berri & Cohen 2012 VD -> VB reset). Within-pool
# coupling (DB<->DB, VB<->VB chemical and gap) and all circuit gap junctions
# are removed; restore them with --isolate-add DB,VB etc.
BBC_MINIMAL_EDGES = {("VB", "DD"), ("DB", "VD"), ("VD", "VB"), ("DD", "DB")}
ISOLATE: Dict[str, object] = {"mode": None, "add": []}


def isolation_keeps_edge(pre: str, post: str, to_muscle: bool) -> bool:
    if ISOLATE["mode"] is None:
        return True
    added = set(ISOLATE["add"])
    cpre = anatomy.neuron_class(pre)
    if to_muscle:
        return cpre in set(BBC_CIRCUIT) | added
    cpost = anatomy.neuron_class(post)
    if cpost not in BBC_CIRCUIT:
        return True
    if cpre in added:
        return True
    if ISOLATE["mode"] == "bbc-minimal":
        return (cpre, cpost) in BBC_MINIMAL_EDGES
    return cpre in BBC_CIRCUIT


def isolation_keeps_gap(a: str, b: str) -> bool:
    if ISOLATE["mode"] is None:
        return True
    added = set(ISOLATE["add"])
    ca, cb = anatomy.neuron_class(a), anatomy.neuron_class(b)
    if ca not in BBC_CIRCUIT and cb not in BBC_CIRCUIT:
        return True
    if ca in added or cb in added:
        return True
    if ISOLATE["mode"] == "bbc-minimal":
        return False
    return ca in BBC_CIRCUIT and cb in BBC_CIRCUIT


def is_graded_motor(name: str) -> bool:
    return (GRADED["motor"] and anatomy.class_info(name).modality == anatomy.MOTOR
            and anatomy.neuron_class(name) not in SPIKING_MOTOR_CLASSES)


def is_graded(name: str) -> bool:
    return is_graded_motor(name) or anatomy.neuron_class(name) in GRADED_CLASSES


def cell_type_name(name: str) -> str:
    """AARNN type name. Graded motor neurons are ``<type>:graded`` (B-type
    ``:graded_b`` and SMD/RMD ``:graded_head`` when given a plateau); spiking
    head-oscillator motor neurons (SMD, RMD) get ``:head_oscillator`` so their
    Izhikevich preset can be set independently."""
    base = anatomy.type_name(name)
    cls = anatomy.neuron_class(name)
    if cls in GRADED_CLASSES or cls in TONIC:
        # A class with its own settings gets its own type: <type>:graded:AVB.
        return f"{base}:{'graded' if is_graded(name) else 'spiking'}:{cls}"
    if is_graded_motor(name):
        if GRADED["b_plateau_mv"] > 0 and cls in B_CLASSES:
            return base + ":graded_b"
        if GRADED["head_plateau_mv"] > 0 and cls in HEAD_OSCILLATOR_CLASSES:
            return base + ":graded_head"
        return base + ":graded"
    if HEAD["preset"] != "RS" and cls in HEAD_OSCILLATOR_CLASSES:
        return base + ":head_oscillator"
    return base


def graded_bio(tau_ms: float, release_hz: float, plateau_mv: float = 0.0, resistance: Optional[float] = None) -> dict:
    return {
        "graded_enabled": True,
        "graded_tau_ms": tau_ms,
        "graded_v_rest_mv": GRADED_BIO_OFF["graded_v_rest_mv"],
        "graded_input_resistance": GRADED["input_resistance"] if resistance is None else resistance,
        "graded_release_v_half_mv": GRADED["release_v_half_mv"],
        "graded_release_max_hz": release_hz,
        "graded_plateau_mv": plateau_mv,
        "graded_plateau_v_half_mv": GRADED["plateau_v_half_mv"],
        "graded_plateau_slope_mv": GRADED["plateau_slope_mv"],
    }


def neuron_type_table(net: dict, hidden_nodes: List[str], gain: float, muscle_gain: float = 1.0,
                      muscle_preset: str = "RS") -> List[dict]:
    """Complete per-type bio parameters.

    ``bio_params`` replaces the network default for a typed neuron, so every
    field is written: a partial object would silently fall back to code
    defaults (e.g. homeostasis 3 Hz instead of the network's value).
    """
    base = dict(net.get("aarnn_bio") or {})
    # Every member of a type shares the class-derived parameters below, so
    # one representative cell defines the type.
    representative: Dict[str, str] = {}
    for n in hidden_nodes:
        representative.setdefault(cell_type_name(n), n)
    out = []
    for name in sorted(representative):
        cell = representative[name]
        cls = anatomy.neuron_class(cell)
        modality, transmitter = name.split(":")[:2]
        bio = dict(base)
        bio["izh_preset"] = IZH_PRESET_BY_TYPE.get(transmitter, "RS")
        if name.endswith(":head_oscillator"):
            bio["izh_preset"] = HEAD["preset"]
        bio["synaptic_gain"] = SYNAPTIC_GAIN_BY_MODALITY.get(modality, 1.0) * gain
        if is_graded(cell):
            plateau = 0.0
            if cls in B_CLASSES:
                plateau = GRADED["b_plateau_mv"]
            elif cls in HEAD_OSCILLATOR_CLASSES:
                plateau = GRADED["head_plateau_mv"]
            bio.update(graded_bio(GRADED["neuron_tau_ms"], GRADED["release_hz"], plateau))
        bio["tonic_current"] = TONIC.get(cls, 0.0)
        out.append({"name": name, "bio_params": bio})
    for name in ("Sensory:transducer", "Sensory:proprioceptor", "Muscle:body_wall", "Muscle:non_locomotor"):
        bio = dict(base)
        bio["izh_preset"] = muscle_preset if name.startswith("Muscle:") else "RS"
        bio["synaptic_gain"] = gain * (muscle_gain if name.startswith("Muscle:") else 1.0)
        if name == "Sensory:transducer":
            # Exteroceptive transduction adapts (receptor desensitisation):
            # a steady odour/temperature/O2 level must not become a steady
            # several-hundred-Hz drive. Depressing transmission (U 0.3,
            # 1.5 s recovery) leaves a few Hz at steady state and transients
            # at stimulus changes, like AWC/AFD/ASE calcium responses.
            bio.update(TRANSDUCER_ADAPTATION)
        if name == "Sensory:proprioceptor":
            # Body-bend proprioception is tonic: no transmission adaptation.
            bio["stp_u"] = 1.0
            bio["stp_tau_rec_ms"] = 0.001
            bio["stp_tau_facil_ms"] = 0.001
        if name.startswith("Muscle:") and GRADED["muscle"]:
            bio.update(graded_bio(GRADED["muscle_tau_ms"], GRADED["muscle_pulse_hz"],
                                  resistance=GRADED["muscle_input_resistance"]))
        out.append({"name": name, "bio_params": bio})
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate C. elegans connectome network JSON.")
    parser.add_argument("--connectome", default="celegans.py",
                        help="Path to celegans.py connectome source (default: celegans.py)")
    parser.add_argument("--template", default="network.json",
                        help="Template snapshot JSON used for baseline net defaults (default: network.json)")
    parser.add_argument("--output", default="network_celegans.json",
                        help="Output snapshot JSON path (default: network_celegans.json)")
    parser.add_argument("--polarity", choices=("transmitter", "source"), default="transmitter",
                        help="Synapse sign rule (default: transmitter; see apply_polarity)")
    parser.add_argument("--sensor-map", choices=("v2", "v1"), default="v2",
                        help="Sensory channel projection (v2 biological equivalents, v1 legacy)")
    parser.add_argument("--no-proprioception", action="store_true",
                        help="Omit the 24 rectified body-bend channels (legacy 24-input contract)")
    parser.add_argument("--weight-per-synapse", type=float, default=None,
                        help="Matrix weight of one synapse (default 1/max |count|: all weights in [-1,1])")
    parser.add_argument("--synaptic-gain", type=float, default=None,
                        help="Per-neuron synaptic gain compensating the weight unit (default: SYNAPTIC_GAIN_DEFAULT)")
    parser.add_argument("--nmj-gain", type=float, default=NMJ_GAIN_DEFAULT,
                        help="Neuromuscular junction efficacy relative to neuron-neuron synapses")
    parser.add_argument("--muscle-preset", default=MUSCLE_PRESET_DEFAULT,
                        help="Izhikevich preset of the muscle readout cells")
    parser.add_argument("--gap-junctions", default=GAP_EDGELIST_DEFAULT,
                        help="Electrical synapse edge list (Cook 2019 CSV); 'none' to omit")
    parser.add_argument("--gap-unit", type=float, default=None,
                        help="Conductance per gap-junction contact (default GAP_UNIT_DEFAULT)")
    parser.add_argument("--proprio-gain", type=float, default=PROPRIO["b_gain"],
                        help="B-type motor neuron curvature gain")
    parser.add_argument("--b-proprioception", choices=("curvature", "stretch"), default=PROPRIO["b_mode"],
                        help="B-type proprioception: same-side anterior curvature (Wen et al. 2012) or own-side "
                             "stretch integrated locally and posteriorly (Boyle, Berri & Cohen 2012)")
    parser.add_argument("--b-field", type=float, default=PROPRIO["b_field"],
                        help="Posterior extent of the B-type stretch-receptor field (body fraction; stretch mode)")
    parser.add_argument("--b-field-origin", choices=("soma", "muscle"), default=PROPRIO["b_origin"],
                        help="Origin of B-type stretch fields: soma, or the neuron's neuromuscular centroid")
    parser.add_argument("--b-field-anterior", type=float, default=PROPRIO["b_field_anterior"],
                        help="Anterior extent of the B-type stretch-receptor field (body fraction; stretch mode)")
    parser.add_argument("--b-curvature-gain", type=float, default=PROPRIO["b_curv_gain"],
                        help="Stretch mode: add same-side curvature sensed anteriorly (Wen et al. 2012) with this "
                             "total gain per neuron (0 = off)")
    parser.add_argument("--b-curvature-field", type=float, default=PROPRIO["b_curv_field"],
                        help="Anterior extent of the same-side curvature field (body fraction)")
    parser.add_argument("--head-proprio-gain", type=float, default=PROPRIO["head_gain"],
                        help="SMD head stretch-receptor gain")
    parser.add_argument("--head-proprioception", choices=("stretch", "bend"), default=PROPRIO["head_mode"],
                        help="SMD head proprioception: stretch receptor (negative feedback) or same-side bend")
    parser.add_argument("--head-extent", type=float, default=PROPRIO["head_extent"],
                        help="Body fraction from the nose whose bend samples reach SMD")
    parser.add_argument("--input-encoding", choices=("threshold", "profile"), default="threshold",
                        help="App-side sensory encoding: threshold passes the client's rate-coded AER spikes "
                             "through unchanged; profile adds the celegans profile's 1 %%/8 %% random floor")
    parser.add_argument("--transducer-tau-rec", type=float, default=TRANSDUCER_ADAPTATION["stp_tau_rec_ms"],
                        help="Exteroceptive transducer adaptation recovery time (ms); <= 0.01 disables adaptation")
    parser.add_argument("--head-motor-preset", default=HEAD["preset"],
                        help="Izhikevich preset of the SMD/RMD head-oscillator motor neurons (e.g. RS, CH, IB)")
    parser.add_argument("--rmd-proprio-gain", type=float, default=PROPRIO["rmd_gain"],
                        help="Optional RMD head proprioceptive gain (0 = connectome input only)")
    parser.add_argument("--motor-model", choices=("spiking", "graded"), default="spiking",
                        help="Motor neurons: Izhikevich spiking (default) or graded membrane with tonic release "
                             "(AVL/DVB always spike)")
    parser.add_argument("--muscle-model", choices=("spiking", "graded"), default="spiking",
                        help="Body-wall muscle cells: spiking (twitch-decoded by the body) or graded "
                             "(pulse-density code, decoded linearly)")
    parser.add_argument("--graded-tau", type=float, default=GRADED["neuron_tau_ms"],
                        help="Graded motor neuron membrane time constant (ms)")
    parser.add_argument("--graded-resistance", type=float, default=GRADED["input_resistance"],
                        help="Graded steady-state depolarisation per unit current (mV per mV/ms)")
    parser.add_argument("--muscle-resistance", type=float, default=GRADED["muscle_input_resistance"],
                        help="Graded muscle steady-state depolarisation per unit current (mV per mV/ms)")
    parser.add_argument("--graded-v-half", type=float, default=GRADED["release_v_half_mv"],
                        help="Graded release half-activation voltage (mV; rest is -70)")
    parser.add_argument("--graded-release-hz", type=float, default=GRADED["release_hz"],
                        help="Spike-equivalent event rate of a graded motor neuron at full release (Hz)")
    parser.add_argument("--muscle-tau", type=float, default=GRADED["muscle_tau_ms"],
                        help="Graded muscle membrane time constant (ms); the body adds the 55 ms Ca2+/force filter")
    parser.add_argument("--b-plateau-mv", type=float, default=GRADED["b_plateau_mv"],
                        help="Plateau depolarisation of graded DB/VB (mV); > 4 x 3 mV makes them bistable")
    parser.add_argument("--head-plateau-mv", type=float, default=GRADED["head_plateau_mv"],
                        help="Plateau depolarisation of graded SMD/RMD (mV)")
    parser.add_argument("--graded-classes", default="",
                        help="Comma-separated neuron classes made graded in addition to motor neurons, "
                             "e.g. AVA,AVB (command interneurons)")
    parser.add_argument("--tonic", action="append", default=[], metavar="CLASS=MV_PER_MS",
                        help="Constant input current of a neuron class (repeatable), e.g. AVB=2 AVA=-1")
    parser.add_argument("--isolate", choices=("bbc", "bbc-minimal"), default=None,
                        help="Isolated locomotor circuit test: DB/VB/DD/VD keep only their mutual connections and "
                             "are the only neuromuscular input (Boyle, Berri & Cohen 2012 forward circuit)")
    parser.add_argument("--isolate-add", default="",
                        help="Comma-separated classes whose connections onto the isolated circuit and muscles are restored")
    parser.add_argument("--config-out", default=None,
                        help="Also write the matching run configuration (the snapshot's net block)")
    parser.add_argument("--input-scale", type=float, default=1.0,
                        help="Scale applied to the sensory projection gains")
    args = parser.parse_args()

    HEAD.update(preset=args.head_motor_preset)
    GRADED.update(motor=args.motor_model == "graded", muscle=args.muscle_model == "graded",
                  neuron_tau_ms=args.graded_tau, input_resistance=args.graded_resistance,
                  muscle_input_resistance=args.muscle_resistance,
                  release_v_half_mv=args.graded_v_half, release_hz=args.graded_release_hz,
                  muscle_tau_ms=args.muscle_tau, b_plateau_mv=args.b_plateau_mv,
                  head_plateau_mv=args.head_plateau_mv)
    known = set(anatomy.CLASSES)
    GRADED_CLASSES[:] = [c.strip() for c in args.graded_classes.split(",") if c.strip()]
    TONIC.clear()
    for item in args.tonic:
        cls, _, value = item.partition("=")
        TONIC[cls.strip()] = float(value)
    ISOLATE.update(mode=args.isolate, add=[c.strip() for c in args.isolate_add.split(",") if c.strip()])
    if ISOLATE["add"] and ISOLATE["mode"] is None:
        raise SystemExit("--isolate-add needs --isolate")
    for cls in list(GRADED_CLASSES) + list(TONIC) + list(ISOLATE["add"]):
        if cls not in known:
            raise SystemExit(f"unknown neuron class {cls}")
    if not GRADED["motor"] and (args.b_plateau_mv > 0 or args.head_plateau_mv > 0):
        raise SystemExit("--b-plateau-mv/--head-plateau-mv need --motor-model graded")
    TRANSDUCER_ADAPTATION["stp_tau_rec_ms"] = args.transducer_tau_rec
    if args.transducer_tau_rec <= 0.01:
        TRANSDUCER_ADAPTATION.update(stp_u=1.0, stp_tau_facil_ms=0.001)
    PROPRIO.update(b_gain=args.proprio_gain, head_gain=args.head_proprio_gain, head_mode=args.head_proprioception,
                   head_extent=args.head_extent, rmd_gain=args.rmd_proprio_gain,
                   b_mode=args.b_proprioception, b_field=args.b_field, b_field_anterior=args.b_field_anterior,
                   b_curv_gain=args.b_curvature_gain, b_curv_field=args.b_curvature_field,
                   b_origin=args.b_field_origin)
    connectome_path = Path(args.connectome)
    template_path = Path(args.template)
    output_path = Path(args.output)

    if not connectome_path.exists():
        raise SystemExit(f"Missing connectome source: {connectome_path}")
    if not template_path.exists():
        raise SystemExit(f"Missing template JSON: {template_path}")

    hidden_nodes, output_nodes, edges, added_hidden_nodes = parse_connectome_edges(connectome_path)
    if not hidden_nodes:
        raise SystemExit("No connectome neuron functions detected.")
    anatomy.validate(hidden_nodes)
    signed, polarity_overrides = apply_polarity(edges, hidden_nodes, args.polarity)

    hidden_index = {name: i for i, name in enumerate(hidden_nodes)}
    output_index = {name: i for i, name in enumerate(output_nodes)}
    sensory_channels = list(CELEGANS_SENSOR_CHANNELS) + ([] if args.no_proprioception else BEND_CHANNELS)

    hidden_count = len(hidden_nodes)
    output_count = len(output_nodes)
    sensory_count = len(sensory_channels)

    body_wall = [n for n in output_nodes if anatomy.muscle_quadrant(n)]
    if len(body_wall) != 95 or output_nodes[95] != "MVULVA":
        raise SystemExit(f"Expected 95 body-wall muscles then MVULVA at index 95, got {output_nodes[:3]}... {output_nodes[94:]}")

    max_count = max(abs(w) for w in signed.values())
    unit = args.weight_per_synapse if args.weight_per_synapse is not None else 1.0 / float(max_count)
    gain = args.synaptic_gain if args.synaptic_gain is not None else SYNAPTIC_GAIN_DEFAULT

    ACTUATION_AP.clear()
    nmj: Dict[str, List[Tuple[float, float]]] = {}
    for (src, dst), count in signed.items():
        if dst in output_index and anatomy.muscle_quadrant(dst):
            nmj.setdefault(src, []).append((anatomy.muscle_ap(dst), abs(float(count))))
    for src, pts in nmj.items():
        ACTUATION_AP[src] = sum(a * w for a, w in pts) / sum(w for _, w in pts)
    w_in, p_in, sensory_target_map = build_sensory_projection(
        hidden_nodes, sensory_channels, args.sensor_map, args.input_scale)
    topo, topo_region_counts = build_celegans_topology(hidden_nodes, output_nodes, sensory_channels)
    w_rec = make_matrix(hidden_count, hidden_count)
    p_rec = [[0 for _ in range(hidden_count)] for _ in range(hidden_count)]
    w_out = make_matrix(output_count, hidden_count)
    p_out = [[0 for _ in range(hidden_count)] for _ in range(output_count)]

    removed = Counter()
    if ISOLATE["mode"] is not None:
        kept = {}
        for (src, dst), count in signed.items():
            if isolation_keeps_edge(src, dst, dst in output_index):
                kept[(src, dst)] = count
            else:
                removed["neuromuscular" if dst in output_index else "chemical"] += 1
        signed = kept
    for (src, dst), count in signed.items():
        s_idx = hidden_index[src]
        if dst in hidden_index:
            d_idx = hidden_index[dst]
            w_rec[d_idx][s_idx] = float(count) * unit
            p_rec[d_idx][s_idx] = abs(int(count))
        elif dst in output_index:
            d_idx = output_index[dst]
            w_out[d_idx][s_idx] = float(count) * unit
            p_out[d_idx][s_idx] = abs(int(count))
        else:
            raise SystemExit(f"unrouted synapse {src}->{dst}")

    gap_pairs: Dict[Tuple[str, str], int] = {}
    gap_meta: dict = {"source": None, "note": "no electrical synapses imported"}
    gap_path = Path(args.gap_junctions) if args.gap_junctions and args.gap_junctions != "none" else None
    if gap_path is not None:
        if not gap_path.exists():
            raise SystemExit(f"Missing electrical synapse edge list {gap_path}; fetch {GAP_EDGELIST_URL} or pass --gap-junctions none")
        gap_pairs, gap_meta = load_electrical_synapses(gap_path, hidden_nodes)
        if ISOLATE["mode"] is not None:
            before = len(gap_pairs)
            gap_pairs = {k: c for k, c in gap_pairs.items() if isolation_keeps_gap(*k)}
            removed["electrical_pairs"] = before - len(gap_pairs)
            gap_meta["pairs"] = len(gap_pairs)
            gap_meta["contacts"] = int(sum(gap_pairs.values()))
    w_gap = make_matrix(hidden_count, hidden_count)
    gap_unit = 0.0
    if gap_pairs:
        row_sum: Counter = Counter()
        for (a, b), c in gap_pairs.items():
            row_sum[a] += c
            row_sum[b] += c
        max_row = max(row_sum.values())
        gap_unit = args.gap_unit if args.gap_unit is not None else GAP_UNIT_DEFAULT
        for (a, b), c in gap_pairs.items():
            ia, ib = hidden_index[a], hidden_index[b]
            w_gap[ia][ib] = w_gap[ib][ia] = c * gap_unit
        gap_meta.update(unit_per_contact=gap_unit, max_summed_contacts=max_row,
                        max_summed_conductance_per_ms=gap_unit * max_row,
                        rule="g_ij = contacts * unit; runtime integrates the coupling implicitly (backward Euler), stable for any g")

    template = json.loads(template_path.read_text(encoding="utf-8"))
    net = dict(template.get("net", {}))

    net["num_sensory_neurons"] = sensory_count
    net["num_hidden_layers"] = 1
    net["num_hidden_per_layer_initial"] = hidden_count
    net["num_output_neurons"] = output_count
    # Sensory inputs and muscle targets are external adapter/readout channels.
    # The biological population is exactly the imported 302-neuron connectome.
    net["io_channels_are_biological"] = False
    sensory_target_layer, output_source_layer = aarnn_laminar_io_layers(net["num_hidden_layers"])
    net["sensory_target_layer"] = sensory_target_layer
    net["output_source_layer"] = output_source_layer
    # Adult connectome: active morphology (delays/attenuation from soma
    # positions) but no structural development.  Growth, spontaneous neuron
    # addition, pruning and import rewiring would each change the imported
    # wiring, so all are disabled; the stage is pinned to the mature stage.
    net["growth_enabled"] = False
    net["morpho_growth_enabled"] = False
    net["use_morphology"] = True
    net["use_aarnn_delays"] = True
    net["aarnn_layer_depth"] = max(3, int(net.get("aarnn_layer_depth", 0) or 0))
    net["max_layers"] = 1
    net["layer_split_threshold"] = 4096
    net["spawn_radius"] = 0.045
    net["new_edge_prob"] = 0.0
    net["proximity_degree_cap"] = 3
    net["development_stage_mode"] = "manual"
    net["development_stage"] = "myelination_stabilization"
    net["development_pruning_interval_ms"] = 1.0e12
    # The AARNN sleep phase replaces sensory input with replayed frames for
    # the first 1.2 s of every 3 min cycle (including the start of every
    # run). Adult C. elegans has no such periodic sleep (lethargus is a
    # larval moult state), so it is disabled for the adult connectome.
    net["sleep_enabled"] = False
    net["sleep_cycle_ms"] = 180000.0
    net["sleep_duration_ms"] = 1200.0
    net["world_model_enabled"] = False
    net["theta_rhythm_enabled"] = False
    net["thalamic_gating_enabled"] = False
    net["aarnn_velocity"] = 5.5
    net["axon_velocity"] = 6.8
    net["dend_velocity"] = 3.6
    net["p_release_default"] = 0.72
    net["bouton_latency_ms"] = 0.4
    net["bouton_jitter_ms"] = 0.05
    # Polarity is carried by the signed matrices (transmitter identity applied
    # above).  Soft Dale enforcement would re-sign every column from a type
    # heuristic and clamp |w| to the STDP bound every step, so it is off.
    net["aarnn_dale_strictness"] = 0.0
    net["aarnn_inhibitory_fraction"] = round(
        sum(1 for n in hidden_nodes if anatomy.is_gabaergic(n)) / float(hidden_count), 4)
    # Measured electrical synapses replace the proximity-based mean-field
    # gap coupling, which would couple every cell within 0.28 of another
    # regardless of the connectome.
    net["aarnn_gap_junction_strength"] = 0.0 if gap_pairs else 0.06
    net["aarnn_gap_junction_radius"] = 0.28
    net["aarnn_gap_junction_inhibitory_only"] = False
    net["aarnn_nmda_voltage_sensitivity"] = 0.02
    net["aarnn_distance_attenuation_per_unit"] = 0.26
    net["aarnn_release_prob_heterogeneity"] = 0.12
    net["volume_transmission_enabled"] = True
    net["volume_transmission_radius"] = 0.18
    net["volume_transmission_strength"] = 0.08
    net["aarnn_triplet_ltp_gain"] = 0.12
    net["aarnn_triplet_ltd_gain"] = 0.08
    # Per-step row rescaling (0.03 per 2 ms step) normalises every neuron's
    # summed input to the same target within ~0.2 s, erasing the synapse-count
    # differences between neurons; homeostatic scaling acts over hours in vivo.
    net["aarnn_synaptic_scaling_strength"] = 0.0
    net["aarnn_synaptic_scaling_target"] = 0.85
    net["aarnn_myelination_enabled"] = False
    net["aarnn_myelination_rate"] = 0.0
    net["aarnn_demyelination_rate"] = 0.0
    net["aarnn_myelin_min_conduction_gain"] = 1.0
    net["aarnn_myelin_max_conduction_gain"] = 1.0
    net["aarnn_myelin_initial"] = 0.0
    net["aarnn_import_topology_rewire_enabled"] = False
    net["aarnn_import_topology_rewire_keep_fraction"] = 1.0
    net["aarnn_import_topology_rewire_region_bias"] = 0.0
    # Connectome fidelity switches (see NetworkConfig docs): no online
    # rewriting of the measured wiring and no automatic output gain control.
    net["aarnn_plasticity_enabled"] = False
    net["aarnn_output_gain_floor_enabled"] = False
    # Spontaneous drives. The template's growth-network defaults inject a
    # +10 current kick into every layer-0 neuron with probability 0.1 per ms
    # (~100 Hz per cell) plus a global perceptual-error drive and resonance
    # kicks, which swamp a 302-cell connectome. Keep a ~1 Hz spontaneous
    # kick (stochastic channel opening) and nothing else.
    net["aarnn_synaptic_energy_randomness"] = 0.001
    net["perceptual_loop_enabled"] = False
    net["aarnn_resonance_gain"] = 0.0
    net["aarnn_ambient_energy_level"] = 0.0
    net["clumping_design"] = "NematodeWorm"
    # The embodiment client already rate-codes every channel into AER spikes.
    # Re-encoding them with the celegans profile adds a random floor (1 % of
    # channels per 2 ms frame, 8 % when the frame is silent): ~240 spurious
    # sensory spikes/s that drive sensory neurons at random.
    spike_io = dict(net.get("spike_io") or {})
    if args.input_encoding == "threshold":
        spike_io["input_strategy"] = "threshold"
        spike_io["threshold"] = 0.5
    net["spike_io"] = spike_io
    net["max_total_neurons"] = max(hidden_count + output_count, int(net.get("max_total_neurons", 0) or 0))
    net["brain_regions"] = build_nematode_regions()
    # Network default biology is spiking; graded cells are opted in per type.
    net["aarnn_bio"] = {**dict(net.get("aarnn_bio") or {}), **GRADED_BIO_OFF, "tonic_current": 0.0}
    net["neuron_types"] = neuron_type_table(net, hidden_nodes, gain, args.nmj_gain, args.muscle_preset)

    nm_pairs = [(s, d, w) for (s, d), w in signed.items() if d in output_index]
    snapshot = {
        "net": net,
        "topo": topo,
        "w_in": {"rows": hidden_count, "cols": sensory_count, "data": flatten_row_major(w_in)},
        "w_hh_fwd": [],
        "w_hh_bwd": [],
        "w_hh_rec": [
            {"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major(w_rec)}
        ],
        "w_out": {"rows": output_count, "cols": hidden_count, "data": flatten_row_major(w_out)},
        "w_gap": [{"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major(w_gap)}] if gap_pairs else [],
        "p_in": {"rows": hidden_count, "cols": sensory_count, "data": flatten_row_major_u32(p_in)},
        "p_fwd": [],
        "p_bwd": [],
        "p_rec": [
            {"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major_u32(p_rec)}
        ],
        "p_out": {"rows": output_count, "cols": hidden_count, "data": flatten_row_major_u32(p_out)},
        "layer_range": None,
        # Fixed seed: the runtime seeds morphology reconstruction from it, so
        # every load (and every depth variant) has identical anatomy.
        "rng_seed": 302,
        # Extra metadata is ignored by loader but useful for post-load interpretation.
        "connectome_labels": {
            "schema": "celegans-import-v2",
            "sensory_nodes": sensory_channels,
            "hidden_nodes": hidden_nodes,
            "output_nodes": output_nodes,
            "sensory_projection": sensory_target_map,
            "sensory_map_version": "celegans-sensory-map-" + args.sensor_map,
            "sensory_neuron_nodes": sorted(
                {node for targets in sensory_target_map.values() for node in targets}
            ),
            "motor_neuron_nodes": sorted({s for s, d, w in nm_pairs}),
            "motor_output_channels": output_nodes,
            "neuron_anatomy": {
                n: {
                    "class": anatomy.neuron_class(n),
                    "type_name": anatomy.type_name(n),
                    "soma_ap": round(anatomy.soma_ap(n), 4),
                    "side": anatomy.side(n),
                    "dorsoventral": anatomy.dorsoventral(n),
                    "ganglion": anatomy.class_info(n).ganglion,
                }
                for n in hidden_nodes
            },
            "weights": {
                "unit_per_synapse": unit,
                "synaptic_gain": gain,
                "max_abs_synapse_count": max_count,
                "rule": "w = signed synapse count * unit_per_synapse; |w| <= 1 so STDP bounds never clip imported values",
                "nmj_gain": args.nmj_gain,
                "proprioception": dict(PROPRIO),
                "head_oscillator_preset": HEAD["preset"],
                "motor_model": args.motor_model,
                "muscle_model": args.muscle_model,
                "graded": dict(GRADED),
                "graded_motor_neurons": sorted(n for n in hidden_nodes if is_graded_motor(n)),
                "graded_classes": list(GRADED_CLASSES),
                "graded_neurons": sorted(n for n in hidden_nodes if is_graded(n)),
                "tonic_current": dict(TONIC),
                "isolation": {"mode": ISOLATE["mode"], "circuit": list(BBC_CIRCUIT) if ISOLATE["mode"] else [],
                              "added_back": list(ISOLATE["add"]), "removed": dict(removed)},
            },
            "electrical_synapses": gap_meta,
            "polarity": {
                "mode": args.polarity,
                "source_negative_pairs": sum(1 for w in edges.values() if w < 0),
                "inhibitory_pairs": sum(1 for w in signed.values() if w < 0),
                "overrides": polarity_overrides,
                "gabaergic_neurons": sorted(n for n in hidden_nodes if anatomy.is_gabaergic(n)),
                "excitatory_gaba_targets_note": "DVB->MANAL is EXP-1 excitatory in vivo but kept with its source sign; no robot actuator",
            },
            "io_semantics": {
                "biological_neuron_count": hidden_count,
                "sensory_input_channel_count": sensory_count,
                "proprioceptive_channel_count": 0 if args.no_proprioception else len(BEND_CHANNELS),
                "motor_readout_channel_count": output_count,
                "io_channels_are_biological": False,
                "output_targets_are": "muscle_endpoints",
                "appended_outputs": [n for n in output_nodes if n in APPENDED_OUTPUTS],
            },
            "laminar_mapping": {
                "hidden_layer_count": int(net.get("num_hidden_layers", 1)),
                "sensory_target_layer": sensory_target_layer,
                "output_source_layer": output_source_layer,
                "sensory_layer_name": "L4" if sensory_target_layer == 1 else "fallback",
                "output_layer_name": "L5" if output_source_layer == 4 else "fallback",
            },
            "source_file": str(connectome_path),
            "source_sha256": hashlib.sha256(connectome_path.read_bytes()).hexdigest(),
            "edge_count": len(signed),
            "total_weight": int(sum(abs(w) for w in signed.values())),
            "signed_total_weight": int(sum(signed.values())),
            "isolated_hidden_nodes_added": added_hidden_nodes,
            "topology_projection": {
                "mode": "anatomical_soma_map_v1",
                "topo_region_counts": topo_region_counts,
                "uses_name_based_regioning": False,
            },
            "bio_profile": {
                "species": "caenorhabditis_elegans",
                "dataset": "openworm_celegans_connectome",
                "growth3d": bool(net.get("growth_enabled", False)),
                "morphology": bool(net.get("use_morphology", False)),
                "morphological_growth": bool(net.get("morpho_growth_enabled", False)),
                "aarnn_layer_depth": int(net.get("aarnn_layer_depth", 0) or 0),
                "rewire_on_import": bool(net.get("aarnn_import_topology_rewire_enabled", False)),
                "rewire_keep_fraction": float(net.get("aarnn_import_topology_rewire_keep_fraction", 1.0)),
                "structure": "frozen adult connectome (no growth, pruning, rewiring or synaptic scaling)",
            },
        },
    }

    output_path.write_text(json.dumps(snapshot, indent=2), encoding="utf-8")
    if args.config_out:
        # The pinned snapshot is authoritative; the configuration is its I/O
        # contract, so it is the same net block (no dimension can drift).
        Path(args.config_out).write_text(json.dumps(net, indent=2) + "\n", encoding="utf-8")

    print(
        f"Wrote {output_path} | sensory={sensory_count} hidden={hidden_count} output={output_count} "
        f"edges={len(signed)} inhibitory={sum(1 for w in signed.values() if w < 0)} gap_pairs={len(gap_pairs)} gap_unit={gap_unit:.5f} "
        f"total_|weight|={sum(abs(w) for w in signed.values())} unit={unit:.5f} gain={gain:.3f} "
        f"isolated_added={len(added_hidden_nodes)}"
    )


if __name__ == "__main__":
    main()
