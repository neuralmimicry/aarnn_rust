#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Give a designed AARNN circuit a biologically equivalent 3D layout.

Designed circuits (``scripts/aarnn_circuit.py``) place almost every cell at
the origin, which makes spatial effects - conduction delays, activity-guided
growth, resonance between overlapping assemblies - meaningless. This writes a
copy of the network whose soma positions follow the animal's nervous system
(for a synthetic body: the equivalent animal's - a humanoid robot gets a
human brainstem and spinal cord), in the normalised body frame of the
C. elegans import: ``y`` rostral (+0.9) to caudal (-0.9), ``x`` left (+) /
right (-), ``z`` dorsal (+) / ventral (-).

Every node is placed at its *soma*: interneurons in their brain region or
spinal segment, motor neurons in the motor column of the segment that
innervates the joint, and sensory neurons where their cell bodies are -
peripheral in insects (leg sense organs), in cranial or dorsal-root ganglia
beside the CNS in vertebrates. Cells sharing a site get a small fixed offset
so no two somas coincide.

Morphology, delays and growth are left as they are unless ``--spatial`` turns
them on, so ``complexity_ladder.py --spatial`` can compare levels on one
layout.

  python3 scripts/layout_designed_circuit.py network_zebrafish_swim.json \
      network_zebrafish_layout.json [--spatial]
"""
import argparse
import json
import re
import zlib
from pathlib import Path


def lr(side: str, dx: float) -> float:
    return dx if side.lower() in ("l", "left") else -dx


def jitter(name: str, scale: float = 0.004):
    """Deterministic sub-site offset so co-located somas do not coincide."""
    h = zlib.crc32(name.encode())
    return tuple(scale * (((h >> s) & 0xFF) / 127.5 - 1.0) for s in (0, 8, 16))


def zebrafish(name: str, kind: str):
    """Larval zebrafish (5-7 dpf): nMLF/MLR in the midbrain, reticulospinal
    and ARTR in the hindbrain, an 8-segment spinal CPG (V2a-like E, adapting
    A, commissural C) with ventral motor columns; lateral-line afferents in
    the posterior/anterior lateral-line ganglia, vestibular afferents in the
    statoacoustic ganglion, retinal ganglion cells in the eyes."""
    if kind == "sensory":
        hit = needs_sensor_rule(name, "fish")
        if hit:
            return hit
    seg_y = lambda i: 0.35 - i * (1.1 / 7.0)
    if kind == "hidden":
        if name == "MLR":
            return (0.0, 0.62, 0.04), "midbrain_nMLF"
        m = re.match(r"RS_([lr])$", name)
        if m:
            return (lr(m[1], 0.03), 0.50, -0.01), "hindbrain_reticulospinal"
        m = re.match(r"ARTR(_A)?_([lr])$", name)
        if m:
            return (lr(m[2], 0.05 + (0.015 if m[1] else 0.0)), 0.57, 0.02), "hindbrain_ARTR_r2r3"
        m = re.match(r"([EAC])_([lr])(\d+)$", name)
        if m:
            role, side, i = m[1], m[2], int(m[3])
            dx = {"E": 0.03, "A": 0.035, "C": 0.012}[role]
            dz = {"E": 0.0, "A": -0.008, "C": 0.008}[role]
            return (lr(side, dx), seg_y(i), dz), f"spinal_{i}"
        # Learning by experience on the olfactory range channels
        # (aarnn_circuit.add_learned_anticipation): distance-tuned cells CS
        # in the dorsal pallium Dp (the olfactory-cortex homolog, sparse
        # odour coding like Kenyon cells, and an adult neurogenic niche), the
        # learned turn LT in Dm (the amygdala homolog, aversive learning), the
        # near-reflex teacher relays TN in the olfactory bulb and the
        # modulatory LC in the locus coeruleus (rhombomere 1).
        hit = forage_rule(name, "fish")
        if hit:
            return hit
        if name == "LC":
            return (0.012, 0.56, 0.045), "locus_coeruleus"
        m = re.match(r"LT_([lr])$", name)
        if m:
            return (lr(m[1], 0.015), 0.77, 0.05), "pallium_Dm"
        m = re.match(r"TN_([lr])_(\d+)$", name)
        if m:
            return (lr(m[1], 0.035 + 0.006 * int(m[2])), 0.85, 0.0), "olfactory_bulb"
        m = re.match(r"CS_([lr])_(\d+)$", name)
        if m:
            k = int(m[2])
            return (lr(m[1], 0.035 + 0.003 * (k % 5)), 0.75 + 0.006 * (k // 5), 0.025 + 0.003 * (k % 3)), "pallium_Dp"
    if kind == "output":
        m = re.search(r"tail_([lr])(\d+)", name)
        if m:
            return (lr(m[1], 0.045), seg_y(int(m[2])), -0.02), f"motor_column_{m[2]}"
        m = re.search(r"(pec|pelvic|dorsal|caudal)_fin_([lr])", name)
        if m:
            y = {"pec": 0.40, "pelvic": -0.10, "dorsal": -0.25, "caudal": -0.82}[m[1]]
            return (lr(m[2], 0.045), y, -0.02), f"{m[1]}_fin_motor_pool"
        if name.endswith("_jaw"):
            return (0.0, 0.60, -0.02), "trigeminal_motor_nucleus"
        m = re.search(r"operculum_([lr])", name)
        if m:
            return (lr(m[1], 0.04), 0.53, -0.02), "facial_motor_nucleus"
        if "trunk_stiffness" in name:
            return (0.0, 0.0, -0.02), "spinal_tonic_motor"
    if kind == "sensory":
        m = re.search(r"lateralline_([lr])(\d+)", name)
        if m:   # pLL ganglion behind the ear, somatotopic: rostral neuromasts dorsal
            i = int(m[2])
            return (lr(m[1], 0.09), 0.46 - 0.004 * i, 0.012 - 0.003 * i), "posterior_lateral_line_ganglion"
        m = re.search(r"eye_(left|right)", name)
        if m:
            return (lr(m[1], 0.15), 0.82, 0.03), "retina"
        m = re.search(r"olfactory_(l|r|front|rear)", name)
        if m:
            x = {"l": 0.05, "r": -0.05, "front": 0.0, "rear": 0.0}[m[1]]
            return (x, 0.9 if m[1] != "rear" else 0.86, 0.02), "olfactory_epithelium"
        if "flow_anterior" in name:
            return (0.08, 0.60, 0.01), "anterior_lateral_line_ganglion"
        if "flow_posterior" in name:
            return (-0.09, 0.46, 0.01), "posterior_lateral_line_ganglion"
        if "pressure" in name or "accel" in name or "gyro" in name:
            return (0.07 if name.endswith(("x", "depth")) else -0.07, 0.49, 0.0), "statoacoustic_ganglion"
        m = re.search(r"hear_inner_ear_(left|right)_(\d+)Hz", name)
        if m:   # saccular hearing afferents, also in the statoacoustic ganglion; tonotopic offset
            return (lr(m[1], 0.075), 0.495, -0.004 if int(m[2]) < 200 else -0.008), "statoacoustic_ganglion"
    return None


HEX_SEG = {"f": (0.35, "prothoracic"), "m": (0.0, "mesothoracic"), "r": (-0.35, "metathoracic")}
HEX_JOINT = {"coxa": 0.25, "femur": 0.45, "tibia": 0.65, "foot": 0.85}


def hexapod(name: str, kind: str):
    """Insect: brain and suboesophageal zone in the head, one thoracic
    hemiganglion per leg (swing/stance half-centre, levator/depressor
    premotor cells), motor neurons in the same hemiganglion; leg sense
    organs (campaniform sensilla, chordotonal organs, tarsal receptors) have
    their somas in the leg; eyes and tympanal-like ears on the head."""
    m = re.search(r"(^|_)([lr])([fmr])_(SW|ST|HI|LO|coxa|femur|tibia|foot)$", name)
    if m:
        side, leg, part = m[2], m[3], m[4]
        y, ganglion = HEX_SEG[leg]
        if kind == "hidden":
            dz = {"SW": 0.012, "ST": -0.012, "HI": 0.02, "LO": -0.02}[part]
            dy = {"SW": 0.02, "ST": 0.02, "HI": -0.02, "LO": -0.02}[part]
            return (lr(side, 0.05), y + dy, dz), f"{ganglion}_ganglion"
        if kind == "output":
            dy = {"coxa": 0.03, "femur": 0.0, "tibia": -0.03}[part]
            return (lr(side, 0.08), y + dy, -0.03), f"{ganglion}_ganglion_motor"
        if kind == "sensory":
            return (lr(side, HEX_JOINT[part]), y - 0.05, -0.05), f"{leg}{side}_leg_{part}_sensilla"
    if kind == "hidden":
        # Tripod CPG circuit (scripts/build_hexapod_cpg_network_json.py).
        # Insect walking: rhythm and premotor interneurons in the thoracic
        # ganglia, the descending command in the brain, steering through the
        # lateral accessory lobe flip-flop and descending neurons of the
        # suboesophageal zone; backward walking from the moonwalker
        # descending neurons (MDN, Bidaye et al. 2014).
        fixed = {"WALK": ((0.0, 0.92, 0.06), "central_complex_locomotor_command"),
                 "MLR": ((0.0, 0.90, 0.05), "central_complex"),
                 "P_sonar": ((0.0, 0.86, 0.02), "suboesophageal_auditory_neuropil"),
                 "BOUT": ((0.012, 0.84, 0.0), "suboesophageal_zone"),
                 "BOUT_A": ((-0.012, 0.84, 0.0), "suboesophageal_zone"),
                 "BACK": ((0.0, 0.82, 0.03), "moonwalker_descending_neurons"),
                 "BACK_I": ((0.006, 0.82, 0.03), "moonwalker_descending_neurons"),
                 # learning by experience: mushroom-body output neuron (LT)
                 # and its dopaminergic teaching neuron (LC, PPL1-like)
                 "LT": ((0.03, 0.93, 0.02), "mushroom_body_output_neuron"),
                 "LC": ((0.035, 0.94, 0.04), "mushroom_body_dopaminergic_neurons"),
                 # learned foraging / recharging (aarnn_circuit.add_learned_foraging):
                 # hunger in the pars intercerebralis (insulin / AKH cells),
                 # feeding from gustatory input in the suboesophageal zone,
                 # reward from the PAM dopaminergic cluster onto an appetitive
                 # mushroom-body output neuron, run / turn trend cells in the LAL.
                 "HUNGER": ((0.0, 0.97, 0.06), "pars_intercerebralis"),
                 "SATED": ((0.006, 0.97, 0.06), "pars_intercerebralis"),
                 "FEED": ((0.0, 0.84, -0.02), "suboesophageal_zone_gustatory"),
                 "DA_R": ((-0.035, 0.94, 0.04), "mushroom_body_PAM_dopaminergic_neurons"),
                 "V": ((-0.03, 0.93, 0.02), "mushroom_body_output_neuron_appetitive"),
                 "V_SLOW": ((-0.03, 0.93, 0.0), "mushroom_body_output_neuron_appetitive"),
                 "RISE": ((0.01, 0.90, 0.05), "lateral_accessory_lobe"),
                 "FALL": ((-0.01, 0.90, 0.05), "lateral_accessory_lobe")}
        if name in fixed:
            return fixed[name]
        m = re.match(r"CS_(\d+)$", name)
        if m:   # distance-tuned sonar cells: mushroom-body calyx (Kenyon-cell-like)
            k = int(m[1])
            return (0.02 + 0.002 * (k % 5), 0.95 + 0.003 * (k // 5), 0.03), "mushroom_body_calyx"
        m = re.match(r"RQ_([lr])_(\d+)$", name)
        if m:   # cue relays: antennal-lobe projection neurons
            return (lr(m[1], 0.025 + 0.003 * int(m[2])), 0.975, 0.03), "antennal_lobe"
        m = re.match(r"CF_([lr])_(\d+)$", name)
        if m:   # cue-band cells: Kenyon-cell-like, mushroom-body calyx
            k = int(m[2])
            return (lr(m[1], 0.012 + 0.002 * k), 0.958, 0.035), "mushroom_body_calyx"
        m = re.match(r"BOUT_(A_)?(on|off)$", name)
        if m:
            return (0.02 if m[2] == "on" else -0.02, 0.88, 0.03 if m[1] else 0.04), "lateral_accessory_lobe"
        m = re.match(r"ARTR(_A)?_([lr])$", name)
        if m:   # LAL flip-flop (bilateral, alternating)
            return (lr(m[2], 0.04 + (0.01 if m[1] else 0.0)), 0.88, 0.0), "lateral_accessory_lobe_flipflop"
        m = re.match(r"RS_([lr])$", name)
        if m:
            return (lr(m[1], 0.03), 0.80, -0.01), "suboesophageal_descending_neurons"
        m = re.match(r"([EAC])_gait_(sw|st)$", name)
        if m:
            return (0.02 if m[2] == "sw" else -0.02, 0.05, {"E": 0.0, "A": -0.008, "C": 0.008}[m[1]]), \
                "mesothoracic_ganglion_CPG"
        m = re.match(r"D(\d*)_([lr])_([01])$", name)
        if m:   # lagged stride interneurons, one chain per side and phase
            n = int(m[1] or 0)
            return (lr(m[2], 0.03 + 0.006 * n), 0.02 - 0.012 * int(m[3]), 0.01), "thoracic_premotor_interneurons"
        m = re.match(r"([XZ])_(?:([lr])_)?([01])$", name)
        if m:   # turn (X) and reversal (Z) gates
            x = lr(m[2], 0.015) if m[2] else 0.0
            return (x, -0.03 - 0.01 * int(m[3]), 0.015 if m[1] == "X" else -0.015), "thoracic_premotor_interneurons"
        m = re.match(r"R_hex_s_\d+_ultrasonic_(left|right)$", name)
        if m:
            return (lr(m[1], 0.06), 0.86, 0.0), "suboesophageal_auditory_neuropil"
    if kind == "output":
        m = re.match(r"hex_o_\d+_([lr])([fmr])_(coxa|femur|tibia)$", name)
        if m:
            y, ganglion = HEX_SEG[m[2]]
            dy = {"coxa": 0.03, "femur": 0.0, "tibia": -0.03}[m[3]]
            return (lr(m[1], 0.08), y + dy, -0.03), f"{ganglion}_ganglion_motor"
    if kind == "sensory":
        if "body_accel" in name or "body_gyro" in name:
            return (0.0, 0.0, 0.05), "mesothoracic_dorsal_sensilla"
        if "head_camera" in name:
            return (0.0, 0.85, 0.05), "compound_eye"
        m = re.search(r"ultrasonic_(left|right)", name)
        if m:
            return (lr(m[1], 0.12), 0.8, 0.0), "head_tympanal_organ"
        # Energy needs (optional group `needs`): reserve sensed internally,
        # feeding / charging contact at the mouthparts, cue receivers on the
        # antennae.
        if name == "needs_energy":
            return (0.0, 0.6, 0.04), "interoceptive_energy_sensors"
        if name == "needs_consume":
            return (0.0, 0.9, -0.04), "mouthpart_gustatory_sensilla"
        m = re.match(r"needs_cue_(left|right)$", name)
        if m:
            return (lr(m[1], 0.06), 1.0, 0.05), "antenna"
    return None


# Humanoid (NAO) - the vertebrate equivalent: brain and brainstem in the
# head, cervical enlargement for the arms, lumbosacral enlargement for the
# legs; somatic afferents in the dorsal-root ganglia of the same segments.
NAO_MOTOR = [  # joint prefix -> (cord y, pool)
    ("Head", 0.62, "cervical_C1_C4"), ("Shoulder", 0.52, "cervical_C5"), ("Elbow", 0.49, "cervical_C6_C7"),
    ("Wrist", 0.46, "cervical_C7_C8"), ("Phalanx", 0.44, "cervical_C8_T1"), ("HipYawPitch", 0.25, "lumbar_L1_L2"),
    ("HipRoll", 0.24, "lumbar_L2_L3"), ("HipPitch", 0.235, "lumbar_L2_L4"), ("Knee", 0.22, "lumbar_L3_L4"),
    ("Ankle", 0.19, "lumbosacral_L5_S1"),
]


def nao(name: str, kind: str):
    if kind == "hidden":
        fixed = {"WALK": ((0.0, 0.82, 0.03), "basal_ganglia_locomotor_command"),
                 "MLR": ((0.0, 0.72, 0.01), "midbrain_MLR"),
                 "BACK": ((0.0, 0.69, 0.0), "brainstem_backward_command"),
                 "BACK_A": ((0.012, 0.69, 0.0), "brainstem_backward_command"),
                 "REV": ((0.0, 0.66, -0.01), "medullary_reversal"),
                 "X_A": ((0.02, 0.26, 0.0), "lumbar_gate"), "X_B": ((-0.02, 0.26, 0.0), "lumbar_gate")}
        if name in fixed:
            return fixed[name]
        m = re.match(r"RS_([lr])$", name)
        if m:
            return (lr(m[1], 0.02), 0.64, -0.01), "medulla_reticulospinal"
        if name == "LC":   # noradrenergic modulatory cell (teacher-gated plasticity)
            return (0.006, 0.675, 0.02), "locus_coeruleus"
        m = re.match(r"Q_([lr])$", name)
        if m:   # steering during reversal (turn command x reversal level)
            return (lr(m[1], 0.012), 0.65, -0.012), "medullary_reversal"
        m = re.match(r"ARTR(_A)?_([lr])$", name)
        if m:
            return (lr(m[2], 0.03 + (0.01 if m[1] else 0.0)), 0.66, 0.01), "pontine_turn_oscillator"
        m = re.match(r"([EAC])_gait_(sw|st)$", name)
        if m:   # one flexor/extensor half-centre: left half sw, right half st
            role, phase = m[1], m[2]
            return (0.025 if phase == "sw" else -0.025, 0.23, {"E": 0.0, "A": -0.008, "C": 0.008}[role]), \
                "lumbar_CPG_L2"
        m = re.match(r"YD_([AB])$", name)
        if m:   # yaw-steering delay of the phase (lumbar CPG)
            return (0.012 if m[1] == "A" else -0.012, 0.215, 0.004), "lumbar_CPG_delay"
        if name == "FORCE_TURN":
            return (0.0, 0.66, 0.03), "test_command"
        m = re.match(r"Y_([lr])_([AB])$", name)
        if m:   # yaw AND cells (turn command x lagged phase)
            return (lr(m[1], 0.02), 0.245, 0.0 if m[2] == "A" else -0.004), "lumbar_gate"
        m = re.match(r"D_([LR])_([AB])$", name)
        if m:
            return (lr(m[1], 0.03), 0.22 - (0.01 if m[2] == "B" else 0.0), 0.006), "lumbar_CPG_delay"
        m = re.match(r"CS_([lr])_(\d+)$", name)
        if m:   # learned sonar distance map: inferior colliculus, one group per side
            k = int(m[2])
            return (lr(m[1], 0.032 + 0.0015 * (k % 5)), 0.755 - 0.002 * (k // 5), 0.015 + 0.002 * (k % 3)), \
                "inferior_colliculus_distance_map"
        m = re.match(r"LT_([lr])$", name)
        if m:   # learned orienting turn: superior colliculus
            return (lr(m[1], 0.03), 0.78, 0.035), "superior_colliculus_orienting"
        m = re.match(r"CN_(F|R)(L|R)_(\d+)$", name)
        if m:   # cochlear nucleus, tonotopic (low bands ventral)
            k = [250, 1000, 3000, 8000].index(int(m[3])) if int(m[3]) in (250, 1000, 3000, 8000) else 0
            return (lr(m[2], 0.045 + (0.004 if m[1] == "R" else 0.0)), 0.705, -0.01 + 0.004 * k), "cochlear_nucleus"
        m = re.match(r"ILD_([lr])_(\d+)$", name)
        if m:   # lateral superior olive
            return (lr(m[1], 0.025), 0.69, -0.02 + 0.002 * (int(m[2]) // 1000)), "superior_olive_lateral"
        m = re.match(r"(ITD_([lr])_(Front|Rear)|SOC_(Front|Rear))$", name)
        if m:   # medial superior olive (SOC relays on the midline)
            side = m[2] or ""
            return ((lr(side, 0.018) if side else 0.0), 0.69, -0.012 if (m[3] or m[4]) == "Front" else -0.016), \
                "superior_olive_medial"
        m = re.match(r"OR_([lr])$", name)
        if m:   # deep superior colliculus: orienting
            return (lr(m[1], 0.028), 0.78, 0.028), "superior_colliculus_deep"
        m = re.match(r"R_touch_([LR])Foot/Bumper/(Left|Right)$", name)
        if m:   # toe touch afferents enter the L5/S1 dorsal horn of their foot
            outer = (m[1] == "L") == (m[2] == "Left")
            return (lr(m[1], 0.034 + (0.006 if outer else 0.0)), 0.185, 0.03), "lumbosacral_L5_S1_dorsal_horn"
        m = re.match(r"(?:R_fall_[xz](?:_back)?|FALL_(front|back))$", name)
        if m:   # gravity (otolith) relay and lying detectors: vestibular nuclei
            return ((0.0 if m[1] is None else lr("l" if m[1] == "front" else "r", 0.015)), 0.68, -0.025), \
                "vestibular_nuclei"
        m = re.match(r"(?:R_state_(\w+?)_(\d+)|B_(\w+?)_(\d+))$", name)
        if m:   # body-state code: vestibular (acc) and proprioceptive (joint) bands, dorsal spinocerebellar
            ch = m[1] or m[3]
            k = int(m[2] or m[4])
            vest = ch.startswith("acc")
            return ({"accx": 0.012, "accy": 0.0}.get(ch, -0.012) if vest else lr("l", 0.02 + 0.004 * k),
                    0.68 if vest else 0.60 - 0.01 * k, 0.05 if m[3] else 0.045), \
                "vestibular_nuclei" if vest else "cerebellar_proprioceptive_relay"
        if name in ("RM", "MOVING", "STUCK", "NO_TRIGGER"):   # righting mode, movement and stuck: reticular formation
            return ({"RM": 0.0, "MOVING": 0.006, "STUCK": -0.006, "NO_TRIGGER": 0.012}[name], 0.665, 0.045), \
                "pontine_reticular_formation"
        if name in ("LYING", "R_lying_z") or name.startswith("R_prog_") or name in ("P_up", "P_ventral"):
            # gravity (otolith) relays, lying and progress: vestibular nuclei; progress -> LC
            x = {"LYING": 0.0, "R_lying_z": 0.004, "P_up": 0.008, "P_ventral": -0.008}.get(name, 0.012 if "_z_" in name else -0.012)
            return (x, 0.685, 0.03 if name.startswith("P_") else 0.02), "vestibular_nuclei"
        m = re.match(r"(?:(?:TR|BB)_(acc[xz])_(\d+)|PT_(acc[xz])_(\d+))$", name)
        if m:   # band traces and progress transitions: vestibular nuclei
            ch, k = (m[1], m[2]) if m[1] else (m[3], m[4])
            return ((0.016 if ch == "accx" else -0.016), 0.685 - 0.002 * int(k), 0.055 if m[1] else 0.06), "vestibular_nuclei"
        m = re.match(r"EXO_(A_)?(\d)_([ab])$", name)
        if m:   # exploration oscillators: brainstem
            return (0.01 * (int(m[2]) - 1), 0.655, (0.05 if m[3] == "a" else 0.054) + (0.003 if m[1] else 0.0)), \
                "brainstem_exploration_oscillators"
        m = re.match(r"(LR|LA|ST|SA|L)_(getup|rollm|roll)_(\d+)$", name)
        if m:   # learned sequencing: cerebellar-like context -> movement cells
            k = int(m[3]) + {"roll": 20, "rollm": 30}.get(m[2], 0)
            return (lr("l" if k % 2 == 0 else "r", 0.012), 0.62 - 0.002 * k, {"L": 0.03, "LR": 0.036, "LA": 0.024, "ST": 0.042, "SA": 0.048}[m[1]]), \
                "cerebellum_sequence_learning"
        m = re.match(r"([TG])_(getup|rollm|roll)$", name)
        if m:   # program trigger and its gate: pontine reticular formation
            return ({"roll": 0.006, "rollm": -0.006}.get(m[2], 0.0), 0.67, 0.035 if m[1] == "T" else 0.04), "pontine_reticular_formation"
        m = re.match(r"([KASXM])_(getup|rollm|roll)_(\d+)$", name)
        if m:   # righting sequence (fixed action pattern), mesencephalic locomotor/righting region
            k = int(m[3]) + {"roll": 20, "rollm": 30}.get(m[2], 0)
            return (lr("l" if k % 2 == 0 else "r", 0.008), 0.715 - 0.002 * k, {"K": 0.03, "A": 0.024, "S": 0.036, "X": 0.042, "M": 0.018}[m[1]]), \
                "midbrain_righting_sequence"
        m = re.match(r"T_([lr])$", name)
        if m:   # withdrawal integrator: deep dorsal horn (lamina V), spinoreticular
            return (lr(m[1], 0.025), 0.195, 0.025), "lumbosacral_dorsal_horn_lamina_V"
        m = re.match(r"(R|P|N|R_near)_(?:Sonar/)?(?:sonar_)?(Left|Right|l|r)$", name)
        if m:
            nucleus = {"R": "cochlear_nucleus", "R_near": "cochlear_nucleus", "P": "inferior_colliculus",
                       "N": "inferior_colliculus_near"}[m[1]]
            y = {"R": 0.70, "R_near": 0.70, "P": 0.76, "N": 0.76}[m[1]]
            return (lr(m[2][0], 0.04 if m[1] in ("R", "R_near") else 0.03), y, 0.02), nucleus
    if kind == "output":
        m = re.match(r"([LR]?)(\w+?)(\d*)$", name)
        side = name[0] if name[0] in "LR" else ""
        for prefix, y, pool in NAO_MOTOR:
            if prefix in name:
                x = lr(side, 0.035) if side else 0.0
                return (x, y, -0.02), f"{pool}_ventral_horn"
    if kind == "sensory":
        side = name[0] if name[0] in "LR" else ""
        if name.startswith("Camera"):
            top = name.startswith("CameraTop")
            return (0.0, 0.82 if top else 0.78, 0.06), "retina"
        m = re.match(r"Sonar/(Left|Right)", name)
        if m:
            return (lr(m[1][0], 0.08), 0.74, 0.0), "spiral_ganglion"
        m = re.match(r"Micro/(F|R)(L|R)/", name)
        if m:   # cochlea / spiral ganglion of the ear on that side
            return (lr(m[2], 0.075), 0.73 if m[1] == "R" else 0.745, -0.01), "spiral_ganglion"
        if name.startswith("Micro/ITD"):
            return (0.0, 0.70, -0.02), "spiral_ganglion"
        if name.startswith(("accelerometer", "gyro")):
            return (0.07 if name.endswith("x") else -0.07, 0.73, 0.0), "vestibular_ganglion"
        if "Fsr" in name or "Bumper" in name:
            return (lr(side, 0.06), 0.17, 0.02), "sacral_S1_drg"
        for prefix, y, pool in NAO_MOTOR:
            if prefix in name:
                x = lr(side, 0.06) if side else 0.0
                return (x, y, 0.02), f"{pool.split('_')[0]}_drg"
    return None


# --- adult Drosophila (BANC brain-and-cord connectome) -----------------------
# Imported BANC cells are placed at their top input/output neuropil from the
# BANC v626 annotation table (data/drosophila/BANC v626/neurons.csv.gz), on
# their soma side; designed cells at the neuropil of their functional
# equivalent. Head y 0.72-0.88 (brain), thorax y 0.12-0.42 (ventral nerve
# cord: T1/T2/T3 leg neuromeres ventral, neck/wing/haltere tectula dorsal).
def needs_sensor_rule(name: str, anatomy: str):
    """Energy-needs channels (optional group ``needs``): the reserve as an
    interoceptive signal, intake through gustatory / vagal afferents, the cue
    pair at the olfactory organs (antennae in insects)."""
    if not name.startswith("needs_"):
        return None
    if anatomy == "fly":
        n = FLY_NEUROPIL
        if name == "needs_energy":
            return n["SMP"][:2] + (n["SMP"][2] + 0.02,), "interoceptive_energy_sensors"
        if name == "needs_consume":
            return (0.0, n["GNG"][1] + 0.04, n["GNG"][2] - 0.02), "labellum_gustatory_sensilla"
        m = re.match(r"needs_cue_(left|right)$", name)
        if m:
            return (lr(m[1], n["AL"][0] + 0.02), n["AL"][1] + 0.05, n["AL"][2]), "antenna"
        return None
    y0 = 0.66 if anatomy == "fish" else 0.70
    if name == "needs_energy":
        return (0.0, y0, -0.03), "hypothalamic_interoceptors"
    if name == "needs_consume":
        return (0.0, y0 - 0.07, -0.04), "nodose_ganglion_vagal" if anatomy != "fish" else "gustatory_vagal_afferents"
    m = re.match(r"needs_cue_(left|right)$", name)
    if m:
        return (lr(m[1], 0.03), 0.92 if anatomy == "fish" else 0.90, 0.02), "olfactory_epithelium"
    return None


def forage_rule(name: str, anatomy: str):
    """Learned foraging / recharging cells (aarnn_circuit.add_learned_foraging)
    and spontaneous turn bouts, per anatomy ("fish", "fly", "vertebrate"):
    hunger / satiety in the hypothalamus (pars intercerebralis in insects),
    feeding from gustatory / vagal input, reward from the dopaminergic
    cluster (posterior tuberculum in fish, PAM in insects, VTA in mammals) onto
    a value cell in the ventral striatum (appetitive mushroom-body output
    neuron in insects), cue-band cells in the olfactory bulb / antennal lobe
    and the run / turn trend cells beside the locomotor command."""
    if anatomy == "fly":
        n = FLY_NEUROPIL
        at = {"HUNGER": (n["SMP"], (0.0, 0.02, 0.01), "pars_intercerebralis"),
              "SATED": (n["SMP"], (0.006, 0.02, 0.01), "pars_intercerebralis"),
              "FEED": (n["GNG"], (0.0, 0.0, -0.01), "suboesophageal_zone_gustatory"),
              "DA_R": (n["SMP"], (-0.005, 0.005, 0.005), "mushroom_body_PAM_dopaminergic_neurons"),
              "V": (n["SMP"], (-0.01, 0.0, 0.0), "mushroom_body_output_neuron_appetitive"),
              "V_SLOW": (n["SMP"], (-0.01, 0.0, -0.004), "mushroom_body_output_neuron_appetitive"),
              "RISE": (n["LAL"], (0.0, 0.006, 0.004), "lateral_accessory_lobe"),
              "FALL": (n["LAL"], (0.0, 0.006, -0.004), "lateral_accessory_lobe")}
        cue, bout = (n["AL"], "antennal_lobe"), (n["LAL"], "lateral_accessory_lobe")
    elif anatomy == "fish":
        at = {"HUNGER": ((0.0, 0.66, -0.01), (0, 0, 0), "hypothalamus_arcuate"),
              "SATED": ((0.006, 0.66, -0.01), (0, 0, 0), "hypothalamus_arcuate"),
              "FEED": ((0.0, 0.62, -0.03), (0, 0, 0), "vagal_lobe_gustatory"),
              "DA_R": ((0.008, 0.70, 0.0), (0, 0, 0), "posterior_tuberculum_dopaminergic"),
              "V": ((0.0, 0.74, 0.03), (0, 0, 0), "subpallium_ventral_striatum"),
              "V_SLOW": ((0.004, 0.74, 0.03), (0, 0, 0), "subpallium_ventral_striatum"),
              "RISE": ((0.01, 0.60, 0.02), (0, 0, 0), "midbrain_tegmentum"),
              "FALL": ((-0.01, 0.60, 0.02), (0, 0, 0), "midbrain_tegmentum")}
        cue, bout = ((0.03, 0.87, 0.01), "olfactory_bulb_glomeruli"), ((0.012, 0.60, 0.0), "hindbrain_bout_generator")
    else:
        at = {"HUNGER": ((0.0, 0.70, -0.01), (0, 0, 0), "hypothalamus_arcuate"),
              "SATED": ((0.006, 0.70, -0.01), (0, 0, 0), "hypothalamus_arcuate"),
              "FEED": ((0.0, 0.64, -0.02), (0, 0, 0), "nucleus_of_the_solitary_tract"),
              "DA_R": ((0.008, 0.69, 0.0), (0, 0, 0), "ventral_tegmental_area"),
              "V": ((0.012, 0.80, 0.0), (0, 0, 0), "nucleus_accumbens"),
              "V_SLOW": ((0.016, 0.80, 0.0), (0, 0, 0), "nucleus_accumbens"),
              "RISE": ((0.015, 0.79, 0.01), (0, 0, 0), "striatum"),
              "FALL": ((-0.015, 0.79, 0.01), (0, 0, 0), "striatum")}
        cue, bout = ((0.03, 0.86, 0.02), "olfactory_bulb"), ((0.012, 0.71, 0.01), "midbrain_MLR_bout")
    if name in at:
        (x, y, z), (dx, dy, dz), region = at[name]
        return (x + dx, y + dy, z + dz), region
    # Learned steering (add_learned_foraging(steer=True)): side value cells
    # beside V, steering cells beside the run / turn trend cells.
    m = re.match(r"(V|STEER)_([lr])$", name)
    if m:
        (x, y, z), (dx, dy, dz), region = at["V" if m[1] == "V" else "RISE"]
        return (lr(m[2], abs(x + dx) + 0.006), y + dy, z + dz + 0.004), region
    m = re.match(r"CF_([lr])_(\d+)$", name)
    if m:
        (x, y, z), region = cue
        return (lr(m[1], x + 0.003 * int(m[2])), y, z + 0.002 * int(m[2])), region
    m = re.match(r"BOUT_(A_)?(on|off)$", name)
    if m:
        (x, y, z), region = bout
        return (x if m[2] == "on" else -x, y, z + (0.004 if m[1] else 0.0)), region
    # Innate touch escape (aarnn_circuit.add_touch_escape): the escape cells
    # beside the reticulospinal / descending turn cells (Mauthner cells in
    # rhombomere 4; insect escape descending neurons in the GNG; medullary
    # reticular formation), the touch relays with the head's mechanosensory
    # afferents (trigeminal ganglion; antennal mechanosensory centre AMMC).
    if anatomy == "fly":
        esc = ((0.02, 0.745, -0.045), "descending_neurons_escape_gng")
        touch = (FLY_NEUROPIL["AMMC"], "ammc_antennal_touch")
    elif anatomy == "fish":
        esc, touch = ((0.02, 0.52, -0.02), "hindbrain_mauthner_r4"), ((0.06, 0.72, -0.03), "trigeminal_ganglion")
    else:
        esc, touch = ((0.02, 0.60, -0.02), "medullary_reticular_formation"), ((0.05, 0.75, -0.03), "trigeminal_ganglion")
    m = re.match(r"ESC_(A_)?([lr])$", name)
    if m:
        (x, y, z), region = esc
        return (lr(m[2], x + (0.006 if m[1] else 0.0)), y, z - (0.004 if m[1] else 0.0)), region
    m = re.match(r"TOUCH_([lrf])$", name) or re.match(r"TCH_([lrf])_(\d+)$", name)
    if m:
        (x, y, z), region = touch
        k = int(m[2]) + 1 if m.re.groups > 1 else 0
        if m[1] == "f":
            return (0.0, y + 0.004, z + 0.002 * k), region
        return (lr(m[1], x + 0.002 * k), y, z + 0.002 * k), region
    return None


FLY_NEUROPIL = {
    # brain (x lateral offset, y, z)
    "AL": (0.03, 0.85, -0.01), "SAD": (0.04, 0.82, -0.03), "AMMC": (0.06, 0.81, -0.02),
    "MB_CA": (0.05, 0.74, 0.05), "LH": (0.08, 0.75, 0.03), "SMP": (0.02, 0.79, 0.05),
    "CRE": (0.02, 0.80, 0.03), "LAL": (0.03, 0.79, 0.0), "AOTU": (0.07, 0.81, 0.03),
    "EB": (0.0, 0.79, 0.02), "FB": (0.0, 0.78, 0.03), "PB": (0.015, 0.75, 0.05), "NO": (0.01, 0.77, 0.02),
    "VES": (0.03, 0.77, -0.02), "ICL": (0.04, 0.76, 0.0), "IPS": (0.045, 0.75, -0.02), "SPS": (0.045, 0.75, 0.01),
    "AVLP": (0.07, 0.81, 0.0), "PVLP": (0.08, 0.78, 0.0), "PLP": (0.08, 0.75, 0.01),
    "LO": (0.12, 0.78, 0.0), "LOP": (0.13, 0.77, 0.0), "ME": (0.15, 0.78, 0.0),
    "GNG": (0.01, 0.74, -0.05), "FLA": (0.01, 0.79, -0.04),
    # ventral nerve cord
    "NTCT": (0.01, 0.42, 0.02), "T1_PRONM": (0.03, 0.38, -0.02), "T1_MVAC": (0.01, 0.38, -0.03),
    "LTCT": (0.01, 0.31, 0.0), "WTCT": (0.02, 0.28, 0.03), "INTTCT": (0.015, 0.28, 0.015), "AMNP": (0.03, 0.27, 0.03),
    "T2_PRONM": (0.03, 0.26, -0.02), "T2_MESONM": (0.03, 0.26, -0.02), "T2_MVAC": (0.01, 0.26, -0.03),
    "HTCT": (0.02, 0.17, 0.03), "T3_METANM": (0.03, 0.15, -0.02), "T3_MVAC": (0.01, 0.15, -0.03),
    "ANM": (0.01, 0.05, -0.01),
}
_BANC = {}


def _banc_table():
    if not _BANC:
        import csv
        import gzip
        src = Path(__file__).resolve().parents[1] / "data/drosophila/BANC v626/neurons.csv.gz"
        with gzip.open(src, "rt") as f:
            for row in csv.DictReader(f):
                _BANC[row["Root ID"]] = row
    return _BANC


def drosophila(name: str, kind: str):
    """Adult Drosophila: imported BANC cells at their annotated neuropil and
    soma side; designed flight/steering cells at their functional
    equivalents (descending neurons and the lateral accessory lobe in the
    brain, wing steering premotor cells in the wing tectulum, haltere
    interneurons in the haltere tectulum, antennal wind/gravity relays in
    the AMMC, looming relays in the lobula)."""
    if kind == "sensory":
        hit = needs_sensor_rule(name, "fly")
        if hit:
            return hit
    if name.isdigit():
        row = _banc_table().get(name)
        if row is None:
            return None
        hood = row["Top in/out region"].upper().split(".")
        labels = row["Community labels"]
        side = (row["Soma side"] or "").lower()
        # somas sit in the brain or the cord; take the first neuropil on the
        # soma's side of that division (descending/ascending cells span both)
        brain = "soma in brain" in labels or (not "soma in VNC" in labels and "innervates" not in labels
                                              and hood[0] in FLY_NEUROPIL and FLY_NEUROPIL[hood[0]][1] > 0.6)
        pick = next((h for h in hood if h in FLY_NEUROPIL and (FLY_NEUROPIL[h][1] > 0.6) == brain), None)
        pick = pick or next((h for h in hood if h in FLY_NEUROPIL), None)
        if pick is None:
            return None
        x, y, z = FLY_NEUROPIL[pick]
        x = lr(side[:1], x) if side[:1] in ("l", "r") else 0.0
        if "innervates" in labels:   # motor neurons: somas at the neuromere's ventral cortex
            z -= 0.01
        return (x, y, z), f"{pick.lower()}_banc"
    fixed = {"MLR": ("LAL", 0.0, "lal_cx_locomotor_command"), "TAKEOFF": ("GNG", 0.02, "giant_fibre_takeoff"),
             "FLIGHT": ("WTCT", 0.0, "wing_tectulum_flight_command"),
             "PITCH_b": ("WTCT", 0.006, "wing_tectulum_steering"), "PITCH_f": ("WTCT", -0.006, "wing_tectulum_steering"),
             "LIFT_u": ("WTCT", 0.012, "wing_tectulum_steering"), "LIFT_d": ("WTCT", -0.012, "wing_tectulum_steering"),
             "I_ref": ("HTCT", 0.0, "haltere_tectulum"), "I_roll": ("HTCT", 0.008, "haltere_tectulum"),
             "I_pitch_neg": ("HTCT", -0.008, "haltere_tectulum"),
             "H_slow": ("AMMC", 0.06, "ammc_antennal_wind"), "N_slow": ("AMMC", -0.06, "ammc_antennal_wind"),
             "D_lateral": ("LO", 0.12, "lobula_looming")}
    if name in fixed:
        hood, x, region = fixed[name]
        _, y, z = FLY_NEUROPIL[hood]
        return (x, y, z), region
    m = re.match(r"RS_([lr])$", name)
    if m:
        return (lr(m[1], 0.025), 0.76, -0.03), "descending_neurons_gng"
    m = re.match(r"ARTR(_A)?_([lr])$", name)
    if m:
        return (lr(m[2], 0.03 + (0.01 if m[1] else 0.0)), 0.79, 0.0), "lal_turning"
    m = re.match(r"(ROLL|YAW)_([lr])$", name)
    if m:
        x, y, z = FLY_NEUROPIL["WTCT"]
        return (lr(m[2], x + (0.01 if m[1] == "YAW" else 0.0)), y, z), "wing_tectulum_steering"
    if name.startswith("R_dros_s_03_gyro"):
        x, y, z = FLY_NEUROPIL["HTCT"]
        return ({"x": 0.02, "y": -0.02, "z": 0.0}[name[-1]], y, z + 0.005), "haltere_tectulum_afferent_relay"
    if name.startswith("R_dros_s_00_accel"):
        x, y, z = FLY_NEUROPIL["AMMC"]
        return (x if name.endswith("x") else -x, y, z), "ammc_johnston_organ_relay"
    if name.startswith("R_dros_s_22_prox"):
        x, y, z = FLY_NEUROPIL["LO"]
        return (x, y, z + 0.01), "lobula_looming"
    # Learning by experience (aarnn_circuit.add_learned_anticipation), as on
    # the hexapod: distance-tuned Kenyon-cell-like CS in the calyx (the adult
    # neurogenic niche of the insect brain), the learned turn LT as a
    # mushroom-body output neuron in the SMP, its PPL1-like dopaminergic
    # teacher LC, and the near-reflex relays TN with the lobula looming cells.
    hit = forage_rule(name, "fly")
    if hit:
        return hit
    if name == "LC":
        x, y, z = FLY_NEUROPIL["SMP"]
        return (x + 0.005, y + 0.005, z + 0.005), "mushroom_body_dopaminergic_neurons"
    m = re.match(r"LT_([lr])$", name)
    if m:
        x, y, z = FLY_NEUROPIL["SMP"]
        return (lr(m[1], x), y, z), "mushroom_body_output_neuron"
    m = re.match(r"TN_([lr])_(\d+)$", name)
    if m:
        x, y, z = FLY_NEUROPIL["LO"]
        return (lr(m[1], x), y, z + 0.003 * int(m[2])), "lobula_looming"
    m = re.match(r"CS_([lr])_(\d+)$", name)
    if m:
        k = int(m[2])
        x, y, z = FLY_NEUROPIL["MB_CA"]
        return (lr(m[1], x + 0.003 * (k % 5)), y + 0.004 * (k // 5), z + 0.002 * (k % 3)), "mushroom_body_calyx"
    return None


# --- vertebrate equivalents for synthetic bodies ------------------------------
# A body with limbs or wheels gets the vertebrate nervous system of the
# animal it resembles: brainstem command and turning cells, reticulospinal
# neurons, limb CPGs and motor pools in the cervical (fore) and lumbar
# (hind) enlargements, afferents in their dorsal-root, vestibular or
# trigeminal ganglia. Distance sensors stand in for vision: retina in the
# eye, relays in the superior colliculus.
CERVICAL_Y, LUMBAR_Y = 0.45, -0.05


def _vertebrate_core(name: str):
    fixed = {"MLR": ((0.0, 0.72, 0.01), "midbrain_MLR"),
             "BOUT_on": ((0.012, 0.71, 0.01), "midbrain_MLR_bout"), "BOUT_off": ((-0.012, 0.71, 0.01), "midbrain_MLR_bout"),
             "BOUT_A_on": ((0.02, 0.71, 0.0), "midbrain_MLR_bout"), "BOUT_A_off": ((-0.02, 0.71, 0.0), "midbrain_MLR_bout"),
             "BACKUP": ((0.0, 0.69, 0.0), "brainstem_backward_command"),
             "BACKUP_A": ((0.012, 0.69, 0.0), "brainstem_backward_command"),
             "REV": ((0.0, 0.66, -0.01), "medullary_reversal"), "REV_A": ((0.012, 0.66, -0.01), "medullary_reversal"),
             "STALL": ((0.0, 0.70, 0.03), "cerebellar_stall_detector"),
             "LAUNCH": ((0.0, 0.71, 0.02), "midbrain_launch_command"),
             "LAUNCH_T": ((0.012, 0.71, 0.02), "midbrain_launch_command")}
    if name in fixed:
        return fixed[name]
    m = re.match(r"RS_([lr])$", name)
    if m:
        return (lr(m[1], 0.02), 0.64, -0.01), "medulla_reticulospinal"
    m = re.match(r"ARTR(_A)?_([lr])$", name)
    if m:
        return (lr(m[2], 0.03 + (0.01 if m[1] else 0.0)), 0.66, 0.01), "pontine_turn_oscillator"
    m = re.match(r"([RP])_proximity_(\d+)$", name)
    if m:
        i = int(m[2])
        x = 0.03 * (1 if i % 2 == 0 else -1) * (1 + i // 2 * 0.2)
        return ((x, 0.76, 0.04) if m[1] == "P" else (x * 1.4, 0.80, 0.02)), \
            ("superior_colliculus" if m[1] == "P" else "lateral_geniculate_relay")
    # Learning by experience (aarnn_circuit.add_learned_anticipation) on the
    # range rays that stand in for vision, as on the NAO: the noradrenergic
    # LC, the learned orienting turn LT in the deep superior colliculus, the
    # distance-tuned CS map in its superficial layers and the near-reflex
    # (looming) teacher relays TN beside the innate proximity cells.
    hit = forage_rule(name, "vertebrate")
    if hit:
        return hit
    if name == "LC":
        return (0.006, 0.675, 0.02), "locus_coeruleus"
    m = re.match(r"LT_([lr])$", name)
    if m:
        return (lr(m[1], 0.03), 0.78, 0.035), "superior_colliculus_orienting"
    m = re.match(r"TN_([lr])_(\d+)$", name)
    if m:
        return (lr(m[1], 0.025 + 0.004 * int(m[2])), 0.755, 0.05), "superior_colliculus_looming"
    m = re.match(r"CS_([lr])_(\d+)$", name)
    if m:
        k = int(m[2])
        return (lr(m[1], 0.04 + 0.0015 * (k % 5)), 0.765 - 0.002 * (k // 5), 0.045 + 0.002 * (k % 3)), \
            "superior_colliculus_distance_map"
    return None


def _distance_eye(name: str):
    hit = needs_sensor_rule(name, "vertebrate")
    if hit:
        return hit
    m = re.match(r"proximity_(\d+)$", name)
    if m:
        i = int(m[1])
        return (0.12 * (1 if i % 2 == 0 else -1), 0.86, 0.03 - 0.004 * i), "retina"
    if name.startswith(("body_gyro", "body_gravity", "body_accel")):
        return (0.07 if name[-1] in "xz" else -0.07, 0.73, 0.0), "vestibular_ganglion"
    return None


def ugot_quadruped(name: str, kind: str):
    """Quadruped mammal equivalent: fore-limb CPGs and motor pools in the
    cervical enlargement, hind limbs in the lumbar enlargement; per-limb
    gating cells beside the CPG, proprioceptive relays in the dorsal horn,
    their afferents in the dorsal-root ganglia."""
    if kind == "hidden":
        hit = _vertebrate_core(name)
        if hit:
            return hit
        m = re.match(r"(E|A|C|G|D|Gr)_(-?1)_(-?1)_(sw|st)$", name)
        if m:
            role, fb, side, phase = m[1], int(m[2]), int(m[3]), m[4]
            y = (CERVICAL_Y if fb > 0 else LUMBAR_Y) + (0.01 if phase == "sw" else -0.01)
            dz = {"E": 0.0, "A": -0.008, "C": 0.008, "G": 0.016, "D": 0.02, "Gr": -0.016}[role]
            dx = {"C": 0.012}.get(role, 0.028)
            return (dx * side, y, dz), f"{'cervical' if fb > 0 else 'lumbar'}_limb_cpg"
        m = re.match(r"(R|Q\+|Q-)_leg_(-?1)_(-?1)_(\d)", name)
        if m:
            kindc, fb, side, j = m[1], int(m[2]), int(m[3]), int(m[4])
            y = (CERVICAL_Y if fb > 0 else LUMBAR_Y) - 0.02 * j
            if kindc == "R":
                return (0.02 * side, y, 0.025 + (0.004 if "velocity" in name else 0.0)), "dorsal_horn_relay"
            return (0.035 * side, y, -0.012 if kindc == "Q+" else -0.018), "ventral_horn_premotor"
    if kind == "output":
        m = re.match(r"leg_(-?1)_(-?1)_(\d)_(positive|negative)$", name)
        if m:
            fb, side, j = int(m[1]), int(m[2]), int(m[3])
            y = (CERVICAL_Y if fb > 0 else LUMBAR_Y) - 0.02 * j
            return (0.045 * side, y - (0.004 if m[4] == "negative" else 0.0), -0.025), \
                f"{'cervical' if fb > 0 else 'lumbar'}_ventral_horn_motor_pool"
    if kind == "sensory":
        m = re.match(r"leg_(-?1)_(-?1)_(\d)_", name)
        if m:
            fb, side, j = int(m[1]), int(m[2]), int(m[3])
            y = (CERVICAL_Y if fb > 0 else LUMBAR_Y) - 0.02 * j
            return (0.07 * side, y - (0.004 if "velocity" in name else 0.0), 0.02), "dorsal_root_ganglion"
        return _distance_eye(name)
    return None


def ugot_balance(name: str, kind: str):
    """Two-wheeled balancer as an upright biped's balance system: tilt and
    turn-rate relays in the vestibular nuclei, the balance premotor pair
    (B+/B-) in the lateral vestibular (Deiters') nucleus driving the
    vestibulospinal tract, wheel-speed relay in Clarke's column, the two
    wheels as the two legs' lumbar motor pools."""
    if kind == "hidden":
        hit = _vertebrate_core(name)
        if hit:
            return hit
        fixed = {"R_body_gravity_x": ((0.035, 0.67, 0.02), "vestibular_nuclei"),
                 "R_body_gyro_y": ((-0.035, 0.67, 0.02), "vestibular_nuclei"),
                 "B+": ((0.03, 0.655, 0.0), "lateral_vestibular_nucleus"),
                 "B-": ((-0.03, 0.655, 0.0), "lateral_vestibular_nucleus"),
                 "R_wheel_speed": ((0.015, 0.30, 0.02), "clarkes_column_spinocerebellar")}
        if name in fixed:
            return fixed[name]
    if kind == "output":
        m = re.match(r"wheel_(-?1)_(positive|negative)$", name)
        if m:
            side = int(m[1])   # +1 left, -1 right
            return (0.04 * side, 0.19 - (0.006 if m[2] == "negative" else 0.0), -0.025), "lumbar_L4_S1_motor_pool"
    if kind == "sensory":
        m = re.match(r"wheel_(-?1)_", name)
        if m:
            return (0.065 * int(m[1]), 0.19 - (0.004 if "velocity" in name else 0.0), 0.02), "lumbar_drg"
        return _distance_eye(name)
    return None


def keenon_t8(name: str, kind: str):
    """Four-wheeled delivery robot as a quadruped: front wheels in the
    cervical enlargement, rear wheels in the lumbar enlargement, wheel-speed
    relays in the dorsal horn of the same segments."""
    if kind == "hidden":
        hit = _vertebrate_core(name)
        if hit:
            return hit
        m = re.match(r"V_wheel_(front|rear)_(left|right)$", name)
        if m:
            return (lr(m[2][0], 0.02), CERVICAL_Y if m[1] == "front" else LUMBAR_Y, 0.025), "dorsal_horn_relay"
    if kind == "output":
        m = re.match(r"wheel_(front|rear)_(left|right)_(positive|negative)$", name)
        if m:
            y = (CERVICAL_Y if m[1] == "front" else LUMBAR_Y) - (0.006 if m[3] == "negative" else 0.0)
            return (lr(m[2][0], 0.045), y, -0.025), "ventral_horn_motor_pool"
    if kind == "sensory":
        m = re.match(r"wheel_(front|rear)_(left|right)_", name)
        if m:
            y = (CERVICAL_Y if m[1] == "front" else LUMBAR_Y) - (0.004 if "velocity" in name else 0.0)
            return (lr(m[2][0], 0.07), y, 0.02), "dorsal_root_ganglion"
        return _distance_eye(name)
    return None


# --- Corvus corone (carrion crow) ---------------------------------------------
# Avian layout: a large optic tectum, vestibular nuclei and cerebellum in the
# hindbrain, a long cervical cord (neck motor pools), wing CPG and motor
# pools in the brachial enlargement, leg pools in the lumbosacral
# enlargement, tail pools caudally; the syrinx from the tracheosyringeal
# hypoglossal nucleus, respiration from the ventral respiratory group.
CROW_SEG = {"neck": 0.58, "wing": 0.42, "tail": -0.35, "leg": 0.02}


def crow(name: str, kind: str):
    if kind == "hidden":
        hit = _vertebrate_core(name)
        if hit:
            return hit
        m = re.match(r"([EAC])_wing_(sw|st)$", name)
        if m:
            return (0.028 if m[2] == "sw" else -0.028, CROW_SEG["wing"], {"E": 0.0, "A": -0.008, "C": 0.008}[m[1]]), \
                "brachial_wing_cpg"
        m = re.match(r"(BOT|TOP)_([pn])$", name)
        if m:
            return (0.02 if m[2] == "p" else -0.02, CROW_SEG["wing"] + 0.015, 0.02 if m[1] == "TOP" else -0.005), \
                "brachial_stroke_phase"
        m = re.match(r"V_(\w+)$", name)
        if m:
            k = ["gx", "gy", "wy", "az", "ax", "fl", "fr", "ref"].index(m[1]) if m[1] in \
                ["gx", "gy", "wy", "az", "ax", "fl", "fr", "ref"] else 0
            return (0.035 * (1 if k % 2 == 0 else -1), 0.67, 0.015 + 0.003 * (k // 2)), "vestibular_nuclei"
        m = re.match(r"([RPG])_([pn])$", name)
        if m:
            return (0.02 if m[2] == "p" else -0.02, 0.70, 0.045), "vestibulocerebellum"
        m = re.match(r"S_(fast|slow)_([pn])$", name)
        if m:   # feather-follicle airflow afferents -> cuneate nucleus
            return (0.015 if m[2] == "p" else -0.015, 0.62, 0.03 if m[1] == "fast" else 0.02), "cuneate_nucleus"
        m = re.match(r"D_([lr])$", name)
        if m:
            return (lr(m[1], 0.08), 0.78, 0.02), "optic_tectum"
        m = re.match(r"Q([+-])_(neck|beak|left_leg|right_leg)_?(\w*)$", name)
        if m:
            part = m[2]
            if part == "neck":
                return (0.012 if m[1] == "+" else -0.012, CROW_SEG["neck"], -0.012), "cervical_premotor"
            if part == "beak":
                return (0.012 if m[1] == "+" else -0.012, 0.71, -0.012), "trigeminal_premotor"
            side = 1 if part.startswith("left") else -1
            return (0.03 * side, CROW_SEG["leg"] + 0.01 * (1 if m[1] == "+" else -1), -0.012), "lumbosacral_premotor"
    if kind == "output":
        side = 1 if name.startswith("left") else (-1 if name.startswith("right") else 0)
        sign = -0.004 if name.endswith("negative") else 0.0
        if name.startswith("neck"):
            return (0.0, CROW_SEG["neck"] + sign, -0.025), "cervical_motor_pool"
        if name.startswith("beak"):
            return (0.0, 0.71 + sign, -0.025), "trigeminal_motor_nucleus"
        if "_wing_" in name:
            return (0.045 * side, CROW_SEG["wing"] + sign, -0.025), "brachial_motor_pool"
        if "_leg_" in name:
            return (0.045 * side, CROW_SEG["leg"] + sign, -0.025), "lumbosacral_motor_pool"
        if name.startswith("tail"):
            return (0.0, CROW_SEG["tail"] + sign, -0.025), "caudal_motor_pool"
        if name.startswith("syrinx"):
            return (0.0, 0.66, -0.02), "tracheosyringeal_hypoglossal_nucleus"
        if name.startswith("respiration"):
            return (0.0, 0.645, -0.02), "ventral_respiratory_group"
    if kind == "sensory":
        m = re.match(r"retina_(left|right)_(\d+)_(\d+)_(on|off)$", name)
        if m:   # retinotopic: 12 x 12 grid on the back of each eye
            r_, c_ = int(m[2]), int(m[3])
            return (lr(m[1][0], 0.12 + 0.002 * (1 if m[4] == "on" else -1)), 0.86 + 0.002 * (c_ - 6),
                    0.03 + 0.002 * (6 - r_)), "retina"
        m = re.match(r"audio_(left|right)_band_(\d+)$", name)
        if m:   # tonotopic cochlear ganglion
            return (lr(m[1][0], 0.09), 0.72, -0.01 + 0.001 * int(m[2])), "cochlear_ganglion"
        if name.startswith(("airspeed", "left_wing_contact", "right_wing_contact")):
            side = 1 if "left" in name else (-1 if "right" in name else 0)
            return (0.06 * side if side else 0.05, CROW_SEG["wing"], 0.03), "brachial_drg"
        if name.startswith(("head_gravity", "head_angular", "head_accel", "body_gyro", "body_accel", "body_gravity",
                            "vestibular_")):
            return (0.07 if name[-1] in "xz" else -0.07, 0.73, 0.0), "vestibular_ganglion"
        if name.startswith("beak"):
            return (0.05 if "left" in name else (-0.05 if "right" in name else 0.0), 0.74, -0.02), "trigeminal_ganglion"
        if name.startswith("neck"):
            return (0.06, CROW_SEG["neck"], 0.02), "cervical_drg"
        side = 1 if name.startswith("left") else (-1 if name.startswith("right") else 0)
        if "_wing_" in name:
            return (0.065 * side, CROW_SEG["wing"] - 0.01, 0.02), "brachial_drg"
        if "_leg_" in name or "_foot_" in name:
            return (0.065 * side, CROW_SEG["leg"] - 0.01, 0.02), "lumbosacral_drg"
        if name.startswith("tail"):
            return (0.03, CROW_SEG["tail"], 0.02), "caudal_drg"
        if name in ("hunger", "thirst", "energy_reserve", "fatigue"):   # interoceptive drives
            return ({"hunger": 0.01, "thirst": -0.005, "energy_reserve": -0.01, "fatigue": 0.0}[name], 0.70, -0.06), \
                "hypothalamus"
    return None


PROFILES = {"zebrafish": zebrafish, "hexapod": hexapod, "nao": nao, "drosophila": drosophila,
            "drosophila_banc": drosophila, "ugot_quadruped": ugot_quadruped, "ugot_balance": ugot_balance,
            "keenon_t8": keenon_t8, "crow": crow, "crow_corvus_corone": crow}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("network", type=Path)
    ap.add_argument("output", type=Path)
    ap.add_argument("--profile", help="default: the network's connectome_labels.profile")
    ap.add_argument("--spatial", action="store_true", help="also switch on morphology and conduction delays")
    args = ap.parse_args()
    snap = json.loads(args.network.read_text())
    labels = snap.get("connectome_labels") or {}
    profile = args.profile or labels.get("profile")
    rule = PROFILES.get(profile)
    if rule is None:
        raise SystemExit(f"no layout rules for profile {profile!r} (have: {sorted(PROFILES)})")
    topo = snap["topo"]
    missing, seen = [], set()
    for kind, nodes, names in (("hidden", topo["layers"][0], labels.get("hidden_nodes") or []),
                               ("sensory", topo["sensory_nodes"], labels.get("sensory_nodes") or []),
                               ("output", topo["output_nodes"], labels.get("output_nodes") or [])):
        for node, name in zip(nodes, names):
            hit = rule(name, kind)
            if hit is None:
                missing.append(f"{kind}:{name}")
                continue
            (x, y, z), region = hit
            jx, jy, jz = jitter(f"{kind}:{name}") if (round(x, 4), round(y, 4), round(z, 4)) in seen else (0, 0, 0)
            seen.add((round(x, 4), round(y, 4), round(z, 4)))
            node["x"], node["y"], node["z"], node["region_name"] = x + jx, y + jy, z + jz, region
    if missing:
        raise SystemExit(f"{len(missing)} cells without a layout rule: {missing[:8]}")
    if args.spatial:
        snap["net"].update(use_morphology=True, use_aarnn_delays=True)
    labels["layout"] = {"source": "scripts/layout_designed_circuit.py", "profile": profile,
                        "frame": "normalised body: y rostral+, x left+, z dorsal+; nodes at their somas"}
    snap["connectome_labels"] = labels
    args.output.write_text(json.dumps(snap))
    print(f"wrote {args.output}: {profile} layout, spatial={'on' if args.spatial else 'unchanged'}")


if __name__ == "__main__":
    main()
