# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Canonical C. elegans hermaphrodite neuron anatomy used by the importer.

Every one of the 302 neurons is resolved from its *class* (the name without
the L/R/D/V/number suffix) to:

* ``modality``  - WormAtlas functional category used for AARNN ``type_name``:
  ``Sensory``, ``Interneuron``, ``Motor``, ``Neuromodulatory`` or
  ``Pharyngeal``;
* ``transmitter`` - principal classical transmitter. The GABAergic set is the
  26-cell McIntire et al. (1993) set (DD, VD, RME, AVL, DVB, RIS); cholinergic
  identity follows Pereira et al. (2015); glutamatergic identity follows
  Serrano-Saiz et al. (2013); monoamines follow Chase & Koelle (2007).
  Only the GABA flag changes synaptic polarity (see ``polarity``); the other
  labels are provenance;
* ``ganglion`` and ``ap`` - soma ganglion and anterior-posterior fraction of
  body length (0 = nose tip, 1 = tail tip) from WormAtlas soma maps. Ventral
  cord motor neurons are spaced by their index along their class range.

The table replaces prefix heuristics that, for example, matched the motor
prefix ``AS`` against the amphid sensory neurons ASE/ASG/ASH/ASI/ASJ/ASK and
placed them in the ventral nerve cord.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Dict, Optional, Tuple

SENSORY = "Sensory"
INTER = "Interneuron"
MOTOR = "Motor"
NEUROMOD = "Neuromodulatory"
PHARYNGEAL = "Pharyngeal"

ACH = "acetylcholine"
GLU = "glutamate"
GABA = "GABA"
DA = "dopamine"
SER = "serotonin"
TA = "tyramine"
OA = "octopamine"
PEP = "neuropeptide"
UNK = "unknown"


@dataclass(frozen=True)
class ClassInfo:
    modality: str
    transmitter: str
    ganglion: str
    ap: float  # soma anterior-posterior fraction for unnumbered classes
    note: str = ""


# fmt: off
CLASSES: Dict[str, ClassInfo] = {
    # --- Amphid sensory neurons (lateral ganglia, head) ---
    "ADF": ClassInfo(SENSORY, SER, "lateral", 0.105, "chemosensory, serotonergic"),
    "ADL": ClassInfo(SENSORY, GLU, "lateral", 0.105, "repellent chemosensory (ascarosides, metals)"),
    "AFD": ClassInfo(SENSORY, GLU, "lateral", 0.105, "primary thermosensor"),
    "ASE": ClassInfo(SENSORY, GLU, "lateral", 0.105, "water-soluble attractants, salt (ASEL ON / ASER OFF)"),
    "ASG": ClassInfo(SENSORY, GLU, "lateral", 0.105, "water-soluble chemosensory"),
    "ASH": ClassInfo(SENSORY, GLU, "lateral", 0.105, "polymodal nociceptor: nose touch, repellents, light"),
    "ASI": ClassInfo(SENSORY, PEP, "lateral", 0.105, "chemosensory, dauer, thermosensory modulation"),
    "ASJ": ClassInfo(SENSORY, ACH, "lateral", 0.105, "light (LITE-1), chemosensory"),
    "ASK": ClassInfo(SENSORY, GLU, "lateral", 0.105, "lysine/ascaroside chemosensory, light"),
    "AWA": ClassInfo(SENSORY, ACH, "lateral", 0.105, "volatile attractants (diacetyl)"),
    "AWB": ClassInfo(SENSORY, ACH, "lateral", 0.105, "volatile repellents, light"),
    "AWC": ClassInfo(SENSORY, GLU, "lateral", 0.105, "volatile attractants, thermosensory"),
    # --- Other head sensory neurons ---
    "AQR": ClassInfo(SENSORY, GLU, "anterior", 0.090, "O2 sensing"),
    "BAG": ClassInfo(SENSORY, GLU, "lateral", 0.110, "CO2 / low O2 sensing"),
    "CEP": ClassInfo(NEUROMOD, DA, "anterior", 0.070, "dopaminergic mechanosensory (texture, food lawn)"),
    "FLP": ClassInfo(SENSORY, GLU, "lateral", 0.110, "head harsh touch, noxious heat"),
    "IL1": ClassInfo(SENSORY, GLU, "anterior", 0.065, "head mechanosensory (nose touch, foraging)"),
    "IL2": ClassInfo(SENSORY, ACH, "anterior", 0.065, "chemo/mechanosensory, nictation"),
    "OLL": ClassInfo(SENSORY, GLU, "lateral", 0.075, "head mechanosensory"),
    "OLQ": ClassInfo(SENSORY, GLU, "anterior", 0.070, "nose touch, head oscillation"),
    "URX": ClassInfo(SENSORY, ACH, "anterior", 0.075, "O2 sensing"),
    "URY": ClassInfo(SENSORY, GLU, "anterior", 0.070, "head mechanosensory"),
    "ADE": ClassInfo(NEUROMOD, DA, "lateral", 0.120, "dopaminergic deirid mechanosensory"),
    # --- Body and tail sensory neurons ---
    "ALM": ClassInfo(SENSORY, GLU, "midbody", 0.420, "anterior gentle touch"),
    "AVM": ClassInfo(SENSORY, GLU, "midbody", 0.380, "anterior gentle touch (ventral)"),
    "PLM": ClassInfo(SENSORY, GLU, "lumbar", 0.950, "posterior gentle touch"),
    "PVM": ClassInfo(SENSORY, GLU, "midbody", 0.650, "posterior touch receptor (ventral)"),
    "PVD": ClassInfo(SENSORY, GLU, "midbody", 0.720, "harsh touch, cold, body proprioception"),
    "PDE": ClassInfo(NEUROMOD, DA, "postdeirid", 0.720, "dopaminergic postdeirid mechanosensory"),
    "PHA": ClassInfo(SENSORY, GLU, "lumbar", 0.935, "phasmid repellent chemosensory"),
    "PHB": ClassInfo(SENSORY, GLU, "lumbar", 0.935, "phasmid repellent chemosensory"),
    "PHC": ClassInfo(SENSORY, GLU, "lumbar", 0.960, "tail noxious heat / touch"),
    "PQR": ClassInfo(SENSORY, GLU, "lumbar", 0.950, "O2 sensing (tail)"),
    "SDQ": ClassInfo(INTER, ACH, "midbody", 0.500, "O2-sensing body interneuron (SDQL posterior, SDQR anterior)"),
    # --- Head interneurons ---
    "ADA": ClassInfo(INTER, GLU, "lateral", 0.120),
    "AIA": ClassInfo(INTER, ACH, "lateral", 0.110, "first-layer chemosensory integrator"),
    "AIB": ClassInfo(INTER, GLU, "lateral", 0.110, "first-layer interneuron, promotes reversals/turns"),
    "AIM": ClassInfo(INTER, GLU, "lateral", 0.115, "serotonin uptake"),
    "AIN": ClassInfo(INTER, ACH, "lateral", 0.110),
    "AIY": ClassInfo(INTER, ACH, "ventral", 0.110, "first-layer interneuron, suppresses turns"),
    "AIZ": ClassInfo(INTER, GLU, "lateral", 0.110, "first-layer interneuron, thermotaxis"),
    "ALA": ClassInfo(INTER, PEP, "dorsal", 0.110, "sleep-promoting"),
    "AUA": ClassInfo(INTER, GLU, "lateral", 0.105),
    "AVA": ClassInfo(INTER, ACH, "lateral", 0.125, "backward command interneuron"),
    "AVB": ClassInfo(INTER, ACH, "lateral", 0.120, "forward command interneuron"),
    "AVD": ClassInfo(INTER, ACH, "lateral", 0.125, "backward command interneuron"),
    "AVE": ClassInfo(INTER, ACH, "lateral", 0.120, "backward command interneuron"),
    "AVF": ClassInfo(INTER, UNK, "ventral", 0.135),
    "AVG": ClassInfo(INTER, ACH, "retrovesicular", 0.140),
    "AVH": ClassInfo(INTER, GLU, "lateral", 0.120),
    "AVJ": ClassInfo(INTER, UNK, "lateral", 0.120),
    "AVK": ClassInfo(INTER, PEP, "ventral", 0.120),
    "RIA": ClassInfo(INTER, GLU, "lateral", 0.105, "head-bending integrator"),
    "RIB": ClassInfo(INTER, ACH, "lateral", 0.110, "forward speed"),
    "RIC": ClassInfo(NEUROMOD, OA, "lateral", 0.110, "octopaminergic"),
    "RID": ClassInfo(INTER, PEP, "dorsal", 0.110, "dorsal-cord neuropeptidergic"),
    "RIF": ClassInfo(INTER, ACH, "ventral", 0.120),
    "RIG": ClassInfo(INTER, GLU, "ventral", 0.120),
    "RIH": ClassInfo(INTER, SER, "anterior", 0.090, "serotonin uptake"),
    "RIM": ClassInfo(NEUROMOD, TA, "lateral", 0.110, "tyraminergic, reversal/turn coupling"),
    "RIP": ClassInfo(INTER, UNK, "anterior", 0.075, "only somatic-pharyngeal bridge"),
    "RIR": ClassInfo(INTER, ACH, "ventral", 0.120),
    "RIS": ClassInfo(INTER, GABA, "ventral", 0.120, "GABAergic sleep-active interneuron"),
    "RIV": ClassInfo(MOTOR, ACH, "lateral", 0.115, "head/neck motor neuron (omega turns)"),
    "BDU": ClassInfo(INTER, GLU, "anterior_body", 0.250),
    "CAN": ClassInfo(INTER, UNK, "midbody", 0.500, "canal-associated, synaptically isolated"),
    # --- Tail interneurons ---
    "ALN": ClassInfo(INTER, ACH, "lumbar", 0.950),
    "PLN": ClassInfo(INTER, ACH, "lumbar", 0.950),
    "DVA": ClassInfo(INTER, ACH, "dorsorectal", 0.925, "stretch-sensitive proprioceptive interneuron"),
    "DVC": ClassInfo(INTER, GLU, "dorsorectal", 0.925),
    "LUA": ClassInfo(INTER, GLU, "lumbar", 0.935),
    "PVC": ClassInfo(INTER, ACH, "lumbar", 0.940, "forward command interneuron"),
    "PVN": ClassInfo(INTER, ACH, "lumbar", 0.940),
    "PVP": ClassInfo(INTER, ACH, "preanal", 0.900),
    "PVQ": ClassInfo(INTER, GLU, "lumbar", 0.940),
    "PVR": ClassInfo(INTER, GLU, "lumbar", 0.950),
    "PVT": ClassInfo(INTER, UNK, "preanal", 0.900),
    "PVW": ClassInfo(INTER, UNK, "lumbar", 0.945),
    "PDA": ClassInfo(MOTOR, ACH, "preanal", 0.905),
    "PDB": ClassInfo(MOTOR, ACH, "preanal", 0.905),
    "HSN": ClassInfo(NEUROMOD, SER, "midbody", 0.520, "serotonergic egg-laying motor neuron"),
    # --- Head motor neurons ---
    "RMD": ClassInfo(MOTOR, ACH, "lateral", 0.100, "head muscle motor neuron (head oscillation)"),
    "RME": ClassInfo(MOTOR, GABA, "anterior", 0.090, "GABAergic head motor neuron"),
    "RMF": ClassInfo(MOTOR, ACH, "ventral", 0.120),
    "RMG": ClassInfo(INTER, PEP, "lateral", 0.110, "hub for aggregation (gap junctions)"),
    "RMH": ClassInfo(MOTOR, ACH, "ventral", 0.115),
    "SAA": ClassInfo(INTER, ACH, "anterior", 0.090),
    "SAB": ClassInfo(MOTOR, ACH, "retrovesicular", 0.145),
    "SIA": ClassInfo(MOTOR, ACH, "ventral", 0.115),
    "SIB": ClassInfo(MOTOR, ACH, "ventral", 0.115),
    "SMB": ClassInfo(MOTOR, ACH, "lateral", 0.110, "head steering"),
    "SMD": ClassInfo(MOTOR, ACH, "ventral", 0.115, "head/neck motor, head-bend proprioception"),
    "URA": ClassInfo(MOTOR, ACH, "anterior", 0.070),
    "URB": ClassInfo(INTER, UNK, "anterior", 0.070),
    "AVL": ClassInfo(MOTOR, GABA, "ventral", 0.120, "GABAergic enteric/body motor neuron"),
    "DVB": ClassInfo(MOTOR, GABA, "dorsorectal", 0.920, "GABAergic enteric motor neuron"),
    # --- Ventral cord motor neurons (ap spread by index; see CORD_RANGES) ---
    "AS": ClassInfo(MOTOR, ACH, "ventral_cord", 0.5, "dorsal muscle excitatory (A-like)"),
    "DA": ClassInfo(MOTOR, ACH, "ventral_cord", 0.5, "dorsal muscle excitatory, backward"),
    "DB": ClassInfo(MOTOR, ACH, "ventral_cord", 0.5, "dorsal muscle excitatory, forward, proprioceptive"),
    "DD": ClassInfo(MOTOR, GABA, "ventral_cord", 0.5, "dorsal muscle inhibitory"),
    "VA": ClassInfo(MOTOR, ACH, "ventral_cord", 0.5, "ventral muscle excitatory, backward"),
    "VB": ClassInfo(MOTOR, ACH, "ventral_cord", 0.5, "ventral muscle excitatory, forward, proprioceptive"),
    "VC": ClassInfo(MOTOR, ACH, "ventral_cord", 0.5, "vulval / ventral muscle"),
    "VD": ClassInfo(MOTOR, GABA, "ventral_cord", 0.5, "ventral muscle inhibitory"),
    # --- Pharyngeal nervous system (20 neurons) ---
    "I1": ClassInfo(PHARYNGEAL, ACH, "pharynx", 0.040),
    "I2": ClassInfo(PHARYNGEAL, GLU, "pharynx", 0.040, "light/H2O2 sensing"),
    "I3": ClassInfo(PHARYNGEAL, UNK, "pharynx", 0.050),
    "I4": ClassInfo(PHARYNGEAL, UNK, "pharynx", 0.090),
    "I5": ClassInfo(PHARYNGEAL, GLU, "pharynx", 0.100),
    "I6": ClassInfo(PHARYNGEAL, UNK, "pharynx", 0.100),
    "M1": ClassInfo(PHARYNGEAL, ACH, "pharynx", 0.050),
    "M2": ClassInfo(PHARYNGEAL, ACH, "pharynx", 0.060),
    "M3": ClassInfo(PHARYNGEAL, GLU, "pharynx", 0.060),
    "M4": ClassInfo(PHARYNGEAL, ACH, "pharynx", 0.090),
    "M5": ClassInfo(PHARYNGEAL, ACH, "pharynx", 0.100),
    "MC": ClassInfo(PHARYNGEAL, ACH, "pharynx", 0.050, "pharyngeal pacemaker"),
    "MI": ClassInfo(PHARYNGEAL, GLU, "pharynx", 0.050),
    "NSM": ClassInfo(NEUROMOD, SER, "pharynx", 0.050, "serotonergic food sensing"),
}
# fmt: on

# Soma AP range (first index, last index) for numbered ventral cord classes.
CORD_RANGES: Dict[str, Tuple[int, float, float]] = {
    "AS": (11, 0.170, 0.830),
    "DA": (9, 0.160, 0.890),
    "DB": (7, 0.150, 0.840),
    "DD": (6, 0.160, 0.800),
    "VA": (12, 0.160, 0.885),
    "VB": (11, 0.145, 0.860),
    "VC": (6, 0.400, 0.620),
    "VD": (13, 0.160, 0.880),
}

# Numbered classes that are not ventral cord (I1..I6, M1..M5, VC handled above).
_NUMBERED_NON_CORD = {"I", "M"}

_CLASS_ORDER = sorted(CLASSES, key=len, reverse=True)

# GABAergic synapses that are excitatory through the EXP-1 cation channel
# (Beg & Jorgensen 2003): AVL/DVB onto enteric muscles.  Recorded for
# provenance; the robot has no enteric actuators.
EXCITATORY_GABA_TARGETS = {"MANAL"}


def neuron_class(name: str) -> str:
    """Return the class of a neuron name, e.g. ``ASEL -> ASE``, ``VD13 -> VD``."""
    n = name.upper()
    m = re.match(r"^([A-Z]+)(\d+)$", n)
    if m and m.group(1) in CORD_RANGES:
        return m.group(1)
    if m and n in CLASSES:  # I1, M4 ...
        return n
    if re.match(r"^(I|M)\d", n):
        base = re.match(r"^((?:I|M)\d)", n).group(1)
        if base in CLASSES:
            return base
    for cls in _CLASS_ORDER:
        if n.startswith(cls):
            return cls
    raise KeyError(f"unknown C. elegans neuron class for {name!r}")


def class_info(name: str) -> ClassInfo:
    return CLASSES[neuron_class(name)]


def cord_index(name: str) -> Optional[int]:
    m = re.match(r"^([A-Z]+)(\d+)$", name.upper())
    if m and m.group(1) in CORD_RANGES:
        return int(m.group(2))
    return None


def soma_ap(name: str) -> float:
    """Soma anterior-posterior body fraction (0 nose, 1 tail)."""
    cls = neuron_class(name)
    idx = cord_index(name)
    if idx is not None:
        count, first, last = CORD_RANGES[cls]
        if count <= 1:
            return first
        return first + (last - first) * (idx - 1) / float(count - 1)
    if cls == "SDQ":
        return 0.62 if name.upper().endswith("L") else 0.37
    return CLASSES[cls].ap


def side(name: str) -> str:
    """Left/right side from the suffix; dorsal/ventral pairs are midline."""
    n = name.upper()
    cls = neuron_class(n)
    suffix = n[len(cls):]
    if cord_index(n) is not None or re.match(r"^\d+$", suffix or "x"):
        return "midline"
    if suffix.endswith("L"):
        return "left"
    if suffix.endswith("R"):
        return "right"
    return "midline"


def dorsoventral(name: str) -> str:
    """Dorsal/ventral position of the soma or of the muscle quadrant it drives."""
    n = name.upper()
    cls = neuron_class(n)
    if cls in {"DA", "DB", "DD", "AS"}:
        return "dorsal"  # AS/DA/DB/DD innervate dorsal muscle; somata are in the ventral cord
    if cls in {"VA", "VB", "VC", "VD"}:
        return "ventral"
    suffix = n[len(cls):]
    if suffix.startswith("D"):
        return "dorsal"
    if suffix.startswith("V"):
        return "ventral"
    if cls in {"RMED", "RID", "ALA", "DVA", "DVB", "DVC"}:
        return "dorsal"
    return "none"


def is_gabaergic(name: str) -> bool:
    return class_info(name).transmitter == GABA


def type_name(name: str) -> str:
    """AARNN ``type_name``: modality qualified with the principal transmitter.

    The qualifier is explicit so the runtime never has to guess polarity from
    a generic word such as ``Interneuron`` (C. elegans interneurons are mostly
    cholinergic or glutamatergic, not inhibitory).
    """
    info = class_info(name)
    return f"{info.modality}:{info.transmitter}"


def muscle_quadrant(name: str) -> Optional[Tuple[str, str, int]]:
    """``MDL07 -> ('dorsal', 'left', 7)``; ``None`` for non body-wall targets."""
    m = re.match(r"^M([DV])([LR])(\d{2})$", name.upper())
    if not m:
        return None
    return ("dorsal" if m.group(1) == "D" else "ventral", "left" if m.group(2) == "L" else "right", int(m.group(3)))


def muscle_ap(name: str) -> float:
    """Body-wall muscle row AP fraction: row 1 at the head, row 24 at the tail."""
    q = muscle_quadrant(name)
    if q is None:
        return {"MVULVA": 0.50, "MANAL": 0.91}.get(name.upper(), 0.5)
    return 0.04 + 0.86 * (q[2] - 1) / 23.0


def validate(names) -> Dict[str, str]:
    """Resolve every name; returns name -> type_name, raising on unknowns."""
    out = {}
    for n in names:
        out[n] = type_name(n)
        soma_ap(n)
    return out
