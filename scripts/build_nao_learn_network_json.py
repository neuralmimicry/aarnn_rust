#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""NAO that learns obstacle avoidance by experience - same AARNN algorithms
as every other network, NAO-specific wiring and parameters only.

1. ``build_nao_walk_network_json.py --learn-groups G``: the NAO walk with
   the innate far-sonar turn replaced by distance-tuned sonar groups (the
   groups-of-groups rule: G cells per group, sparse inside, groups joined by
   a simple attachment) whose silent synapses onto learning-turn cells LT
   are the only plastic ones; the innate near reflex (back off and turn)
   teaches LT.
2. Plasticity on (graded cells take part), plastic-cell mask = LT cells,
   slow homeostatic counterweight (biological hours -> minutes here).
3. ``layout_designed_circuit.py --spatial``: human-equivalent 3D anatomy
   with morphology and conduction delays, so the generic dendritic-bouton
   plasticity (Hebbian + STDP/Oja mix by compartment) applies.

  python3 scripts/build_nao_learn_network_json.py [--plastic 1|0] [--eta E]
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_nao_learn.json")
    ap.add_argument("--config-out", default="webots_world/configs/config_nao_learn.json")
    ap.add_argument("--groups", type=int, default=20, help="cells per sonar group (group-size threshold)")
    ap.add_argument("--plastic", type=int, default=1, help="1 = learns, 0 = same network frozen (control)")
    ap.add_argument("--eta", type=float, default=0.0, help="learning rate (0 = runtime default)")
    ap.add_argument("--homeo-tau-ms", type=float, default=600000.0)
    ap.add_argument("--homeo-gain", type=float, default=0.0,
                    help="homeostatic counterweight gain (0 = off). Off by default with teacher gating: the gate "
                         "already stops exposure-driven potentiation, and the homeostat's 2 s warm-up set point "
                         "scaled the episodically active LT cells' inputs up (open loop: x2.1 in 16 approaches "
                         "with the teacher silent; x1.0 with the homeostat off)")
    ap.add_argument("--gated", type=int, default=1,
                    help="1: teacher-gated plasticity - the LC cell (driven by the innate near reflex) is the "
                         "modulatory cell; 0: ungated (co-activity alone learns)")
    ap.add_argument("--modulator-tau-ms", type=float, default=1000.0)
    ap.add_argument("--modulator-ref-hz", type=float, default=10.0)
    ap.add_argument("--spatial", type=int, default=1, help="3D anatomy + morphology + delays")
    ap.add_argument("--atten", type=float, default=0.04,
                    help="NAO distance attenuation per unit path at --spatial 1")
    ap.add_argument("--release-het", type=float, default=0.04,
                    help="NAO synaptic release-probability heterogeneity at --spatial 1")
    ap.add_argument("walk_args", nargs="*", help="extra build_nao_walk_network_json.py arguments (after --)")
    a = ap.parse_args()
    tmp = ROOT / "target/qa/nao_learn"
    tmp.mkdir(parents=True, exist_ok=True)
    raw = tmp / "network_nao_learn_raw.json"
    subprocess.run([sys.executable, str(ROOT / "scripts/build_nao_walk_network_json.py"), "--learn-groups",
                    str(a.groups), "--output", str(raw), "--config-out", str(ROOT / a.config_out)] + a.walk_args,
                   cwd=ROOT, check=True)
    snap = json.loads(raw.read_text())
    hidden = snap["connectome_labels"]["hidden_nodes"]
    outputs = snap["connectome_labels"]["output_nodes"]
    lt = {"LT_l", "LT_r"}
    assert lt <= set(hidden), "learning-turn cells missing"
    fields = dict(
        aarnn_plasticity_enabled=bool(a.plastic), aarnn_graded_plasticity_enabled=True,
        aarnn_plastic_cells=dict(enabled=True, hidden=[[h in lt for h in hidden]], outputs=[False] * len(outputs),
                                 newborns=True),
        aarnn_rate_homeostasis_tau_ms=a.homeo_tau_ms, aarnn_rate_homeostasis_gain=a.homeo_gain,
        aarnn_plasticity_eta=a.eta,
        aarnn_modulatory_cells=dict(enabled=bool(a.gated) and "LC" in hidden,
                                    hidden=[[h == "LC" for h in hidden]], outputs=[False] * len(outputs)),
        aarnn_modulator_tau_ms=a.modulator_tau_ms, aarnn_modulator_ref_hz=a.modulator_ref_hz,
    )
    snap["net"].update(fields)
    # Neuromodulation stays enabled (25 Nov; no effect at depth 0, see aarnn_circuit.apply_learner).
    labels = snap["connectome_labels"]
    labels["learning"] = dict(plastic_cells=sorted(lt), groups=a.groups, plastic=bool(a.plastic), eta=a.eta,
                              rule="generic AARNN plasticity (matrix STDP + dendritic-bouton Hebbian/STDP/Oja mix)",
                              teacher_gated=bool(a.gated) and "LC" in hidden,
                              modulatory_cells=["LC"] if a.gated and "LC" in hidden else [])
    raw.write_text(json.dumps(snap))
    out = ROOT / a.output
    cmd = [sys.executable, str(ROOT / "scripts/layout_designed_circuit.py"), str(raw), str(out), "--profile", "nao"]
    if a.spatial:
        cmd.append("--spatial")
    subprocess.run(cmd, cwd=ROOT, check=True)
    if a.spatial:
        # Designed circuits inherit the C. elegans template values (distance
        # attenuation 0.26, release heterogeneity 0.12), which silence the
        # NAO's threshold-tuned innate near reflex at S1 (relay 2.3 Hz vs
        # 13.7 Hz in point cells). Calibration rule (same for any animal):
        # the largest equal values at which the innate reflex still reaches
        # its point-cell level on both sides - 0.04/0.04 here (sweep in
        # target/qa/nao_learn/cal_*; 0.06 lost the right side). Both
        # features stay on; the NAO's connectome builder uses 0.08/0.08.
        laid = json.loads(out.read_text())
        laid["net"]["aarnn_distance_attenuation_per_unit"] = a.atten
        laid["net"]["aarnn_release_prob_heterogeneity"] = a.release_het
        out.write_text(json.dumps(laid))
    cfg = json.loads((ROOT / a.config_out).read_text())
    cfg.update(fields)
    if a.spatial:
        cfg.update(use_morphology=True, use_aarnn_delays=True, aarnn_distance_attenuation_per_unit=a.atten,
                   aarnn_release_prob_heterogeneity=a.release_het)
    (ROOT / a.config_out).write_text(json.dumps(cfg, indent=1))
    print(f"wrote {out} ({len(hidden)} hidden cells; plastic: {sorted(lt) if a.plastic else 'none (frozen)'})")


if __name__ == "__main__":
    main()
