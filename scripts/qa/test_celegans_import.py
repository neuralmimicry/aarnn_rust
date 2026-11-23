#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Independent checks of scripts/build_celegans_network_json.py.

The source is re-parsed here with its own pattern (signed counts, comment
lines skipped) and every synapse is compared with the generated matrices:
exact signed counts, no extra or missing entries, the documented polarity
rule, anatomical typing, biologically matched sensory projections and the
electrical-synapse matrix. Run: python3 -m unittest scripts/qa/test_celegans_import.py
"""
import json
import re
import subprocess
import sys
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import celegans_anatomy as A  # noqa: E402

GABA = {f'DD{i}' for i in range(1, 7)} | {f'VD{i}' for i in range(1, 14)} | {'RMED', 'RMEV', 'RMEL', 'RMER', 'AVL', 'DVB', 'RIS'}


def source_pairs():
    pairs = defaultdict(int)
    current = None
    for line in (ROOT / 'celegans.py').read_text().splitlines():
        if line.strip().startswith('#'):
            continue
        m = re.match(r'^def\s+([A-Za-z0-9_]+)\s*\(', line)
        if m:
            current = m.group(1) if m.group(1).isupper() and not re.search(r'[a-z_]', m.group(1)) else None
            continue
        if current:
            for post, w in re.findall(r"postsynaptic\['([^']+)'\]\s*\+=\s*(-?\s*\d+)", line):
                pairs[(current, post)] += int(w.replace(' ', ''))
    return {k: v for k, v in pairs.items() if v != 0}


def h_idx(name, snap):
    return snap['connectome_labels']['hidden_nodes'].index(name)


class CelegansImport(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        out = Path(cls.tmp.name)
        builds = (('v2', []), ('source', ['--polarity', 'source']), ('legacy24', ['--no-proprioception']),
                  ('graded', ['--motor-model', 'graded', '--muscle-model', 'graded', '--b-plateau-mv', '30']),
                  ('bbc', ['--b-proprioception', 'stretch']),
                  ('forward', ['--motor-model', 'graded', '--muscle-model', 'graded', '--b-plateau-mv', '22',
                               '--graded-classes', 'AVA,AVB', '--tonic', 'AVB=2.5', '--tonic', 'AVA=-1']),
                  ('isolated', ['--b-proprioception', 'stretch', '--isolate', 'bbc']),
                  ('isolated_avb', ['--b-proprioception', 'stretch', '--isolate', 'bbc', '--isolate-add', 'AVB']),
                  ('bbc_full', ['--b-proprioception', 'stretch']),
                  ('minimal', ['--b-proprioception', 'stretch', '--isolate', 'bbc-minimal']),
                  ('anterior', ['--b-proprioception', 'stretch', '--b-field', '0', '--b-field-anterior', '0.5']))
        for name, extra in builds:
            subprocess.run([sys.executable, str(ROOT / 'scripts/build_celegans_network_json.py'), '--output', str(out / f'{name}.json'),
                            '--config-out', str(out / f'{name}.config.json')] + extra, cwd=ROOT, check=True, capture_output=True)
        cls.snap = {n: json.loads((out / f'{n}.json').read_text()) for n, _ in builds}
        cls.pairs = source_pairs()

    def dense(self, m):
        return [m['data'][r * m['cols']:(r + 1) * m['cols']] for r in range(m['rows'])]

    def signed_counts(self, snap):
        L = snap['connectome_labels']
        unit = L['weights']['unit_per_synapse']
        h = {n: i for i, n in enumerate(L['hidden_nodes'])}
        o = {n: i for i, n in enumerate(L['output_nodes'])}
        rec, out = self.dense(snap['w_hh_rec'][0]), self.dense(snap['w_out'])
        got = {}
        for post, i in h.items():
            for pre, j in h.items():
                if rec[i][j]:
                    got[(pre, post)] = rec[i][j] / unit
        for post, k in o.items():
            for pre, j in h.items():
                if out[k][j]:
                    got[(pre, post)] = out[k][j] / unit
        return got, L

    def test_every_source_synapse_imported_with_exact_signed_count(self):
        got, L = self.signed_counts(self.snap['source'])
        self.assertEqual(len(self.pairs), 3689)
        self.assertEqual(sum(1 for v in self.pairs.values() if v < 0), 125)
        self.assertEqual(set(got), set(self.pairs), 'missing or extra synapses')
        for k, v in self.pairs.items():
            self.assertAlmostEqual(got[k], v, places=9, msg=k)
        # Presence counts equal |count|; |w| never exceeds the STDP bound.
        self.assertLessEqual(max(abs(x) for m in (self.snap['source']['w_hh_rec'][0], self.snap['source']['w_out']) for x in m['data']), 1.0 + 1e-12)

    def test_transmitter_polarity_rule(self):
        got, L = self.signed_counts(self.snap['v2'])
        hidden = set(L['hidden_nodes'])
        self.assertEqual(set(got), set(self.pairs))
        for (pre, post), v in self.pairs.items():
            expected = -abs(v) if (post in hidden and pre in GABA) else v
            self.assertAlmostEqual(got[(pre, post)], expected, places=9, msg=(pre, post))
        self.assertEqual(sorted(L['polarity']['gabaergic_neurons']), sorted(GABA))

    def test_anatomy_types_are_correct(self):
        L = self.snap['v2']['connectome_labels']
        topo = self.snap['v2']['topo']['layers'][0]
        types = {n: topo[i]['type_name'] for i, n in enumerate(L['hidden_nodes'])}
        for n in ('ASEL', 'ASHR', 'ASIL', 'ASJR', 'ASKL', 'ASGR', 'AWCL', 'AFDR', 'ALML', 'PLMR'):
            self.assertTrue(types[n].startswith('Sensory:'), n)
        for n in ('AVAL', 'AVBR', 'PVCL', 'AIYL', 'DVA'):
            self.assertTrue(types[n].startswith('Interneuron:'), n)
            self.assertNotIn('gaba', types[n].lower())
        for n in GABA:
            self.assertTrue(types[n].endswith(':GABA'), n)
        for n in ('DA1', 'DB7', 'VA12', 'VB11', 'AS11', 'SMDDL', 'RMDVR'):
            self.assertTrue(types[n].startswith('Motor:'), n)
        ap = {n: L['neuron_anatomy'][n]['soma_ap'] for n in L['hidden_nodes']}
        self.assertLess(ap['ASEL'], 0.15)
        self.assertGreater(ap['PLML'], 0.9)
        self.assertTrue(0.3 < ap['ALML'] < 0.5)
        self.assertLess(ap['VA1'], ap['VA12'])

    def test_sensory_channels_drive_matching_neuron_classes(self):
        L = self.snap['v2']['connectome_labels']
        proj = L['sensory_projection']
        cls = lambda ch: {A.neuron_class(n) for n in proj[ch]}
        self.assertEqual(len(L['sensory_nodes']), 48)
        self.assertLessEqual({'ASH', 'FLP', 'ALM', 'AVM'}, cls('celegans_s_06_touch_front'))
        self.assertIn('PLM', cls('celegans_s_07_touch_rear'))
        self.assertLessEqual({'ASJ', 'ASK', 'ASH', 'AWB', 'ASI'}, cls('celegans_s_08_light_left'))
        self.assertIn('AFD', cls('celegans_s_10_heat_left'))
        self.assertIn('ASE', cls('celegans_s_12_taste_front_center'))
        self.assertLessEqual({'AWA', 'AWC'}, cls('celegans_s_15_chem_left'))
        self.assertLessEqual({'PHA', 'PHB'}, cls('celegans_s_17_chem_rear_left'))
        self.assertLessEqual({'URX', 'AQR'}, cls('celegans_s_19_flow_front'))
        # Left/right channels only reach same-side cells where sided.
        for n in proj['celegans_s_15_chem_left']:
            self.assertNotEqual(A.side(n), 'right', n)
        # Every B-type motor neuron gets same-side (dorsal DB / ventral VB) curvature.
        db = {n for ch, t in proj.items() if '_bend_dorsal' in ch for n in t if A.neuron_class(n) == 'DB'}
        vb = {n for ch, t in proj.items() if '_bend_ventral' in ch for n in t if A.neuron_class(n) == 'VB'}
        self.assertEqual(db, {f'DB{i}' for i in range(1, 8)})
        self.assertEqual(vb, {f'VB{i}' for i in range(1, 12)})
        self.assertEqual(len(self.snap['legacy24']['connectome_labels']['sensory_nodes']), 24)
        # Head proprioception is a stretch reflex: a ventral head bend
        # stretches the dorsal side and excites SMDD.
        self.assertTrue(any(n.startswith('SMDD') for n in proj['celegans_s_25_bend_ventral01']))
        self.assertTrue(any(n.startswith('SMDV') for n in proj['celegans_s_24_bend_dorsal01']))
        topo = self.snap['v2']['topo']['sensory_nodes']
        self.assertEqual({t['type_name'] for t in topo[:24]}, {'Sensory:transducer'})
        self.assertEqual({t['type_name'] for t in topo[24:]}, {'Sensory:proprioceptor'})
        types = {t['name']: t['bio_params'] for t in self.snap['v2']['net']['neuron_types']}
        self.assertLess(types['Sensory:transducer']['stp_u'], 1.0, 'exteroceptors adapt')
        self.assertEqual(types['Sensory:proprioceptor']['stp_u'], 1.0, 'proprioceptors do not')
        self.assertEqual(self.snap['v2']['net']['spike_io']['input_strategy'], 'threshold')

    def test_outputs_keep_legacy_order(self):
        o = self.snap['v2']['connectome_labels']['output_nodes']
        self.assertEqual(len(o), 96)
        self.assertEqual(o[95], 'MVULVA')
        self.assertEqual(o[0], 'MDL01')
        self.assertNotIn('MVL24', o)

    def test_structure_is_frozen_and_config_matches(self):
        net = self.snap['v2']['net']
        self.assertFalse(net['growth_enabled'])
        self.assertFalse(net['aarnn_import_topology_rewire_enabled'])
        self.assertEqual(net['aarnn_synaptic_scaling_strength'], 0.0)
        self.assertEqual(net['aarnn_dale_strictness'], 0.0)
        self.assertFalse(net['aarnn_plasticity_enabled'])
        self.assertEqual(net['development_stage_mode'], 'manual')
        self.assertFalse(net['sleep_enabled'])
        cfg = json.loads((Path(self.tmp.name) / 'v2.config.json').read_text())
        self.assertEqual(cfg, net)
        # Every typed cell has a complete parameter set (no silent defaults).
        keys = set(net['aarnn_bio'])
        for t in net['neuron_types']:
            self.assertEqual(set(t['bio_params']), keys, t['name'])

    def test_electrical_synapses(self):
        snap = self.snap['v2']
        meta = snap['connectome_labels']['electrical_synapses']
        if meta.get('source') is None:
            self.skipTest('data/celegans/herm_full_edgelist.csv not present')
        g = self.dense(snap['w_gap'][0])
        n = len(g)
        for i in range(n):
            self.assertEqual(g[i][i], 0.0)
            for j in range(i):
                self.assertEqual(g[i][j], g[j][i])
        # The runtime integrates coupling implicitly; conductance follows contacts.
        unit = meta['unit_per_contact']
        self.assertIn('implicit', meta['rule'])
        self.assertAlmostEqual(g[h_idx('AVBL', snap)][h_idx('DB3', snap)], 9 * unit, places=12)
        h = snap['connectome_labels']['hidden_nodes']
        self.assertGreater(g[h.index('AVBL')][h.index('DB3')], 0.0)
        self.assertGreater(g[h.index('AVAL')][h.index('DA7')], 0.0)
        self.assertEqual(meta['pairs'], sum(1 for i in range(n) for j in range(i) if g[i][j] > 0))

    def test_graded_motor_neurons_and_muscles(self):
        snap = self.snap['graded']
        L = snap['connectome_labels']
        net = snap['net']
        types = {t['name']: t['bio_params'] for t in net['neuron_types']}
        cell = {n: snap['topo']['layers'][0][i]['type_name'] for i, n in enumerate(L['hidden_nodes'])}
        # The graded build changes cell dynamics only, never the wiring.
        self.assertEqual(snap['w_hh_rec'], self.snap['v2']['w_hh_rec'])
        self.assertEqual(snap['w_out'], self.snap['v2']['w_out'])
        self.assertEqual(snap['w_in'], self.snap['v2']['w_in'])
        graded = set(L['weights']['graded_motor_neurons'])
        for n in L['hidden_nodes']:
            motor = A.class_info(n).modality == A.MOTOR
            spiking_motor = A.neuron_class(n) in ('AVL', 'DVB')
            self.assertEqual(n in graded, motor and not spiking_motor, n)
            self.assertEqual(types[cell[n]]['graded_enabled'], n in graded, n)
        # B-type cells are bistable (plateau > 4 x slope); others are not.
        for n in graded:
            b = types[cell[n]]
            bistable = b['graded_plateau_mv'] > 4 * b['graded_plateau_slope_mv']
            self.assertEqual(bistable, A.neuron_class(n) in ('DB', 'VB'), n)
            self.assertEqual(b['graded_release_max_hz'], 20.0)
        # Transmitter polarity survives the type suffix.
        self.assertTrue(cell['DD1'].startswith('Motor:GABA:graded'))
        self.assertTrue(cell['AVL'] == 'Motor:GABA' and not types['Motor:GABA']['graded_enabled'])
        # Muscles are graded and send a 500 Hz (one event per 2 ms frame) pulse-density code.
        for name in ('Muscle:body_wall', 'Muscle:non_locomotor'):
            self.assertTrue(types[name]['graded_enabled'])
            self.assertEqual(types[name]['graded_release_max_hz'], 500.0)
        # Network default and the spiking build stay spiking, with complete keys.
        self.assertFalse(net['aarnn_bio']['graded_enabled'])
        v2_types = self.snap['v2']['net']['neuron_types']
        self.assertFalse(any(t['bio_params']['graded_enabled'] for t in v2_types))
        for t in net['neuron_types']:
            self.assertEqual(set(t['bio_params']), set(net['aarnn_bio']), t['name'])

    def test_b_type_stretch_receptor_mapping(self):
        # Boyle, Berri & Cohen (2012): DB integrates dorsal-side stretch
        # (ventral bend) locally and posteriorly over half the body.
        L = self.snap['bbc']['connectome_labels']
        proj = L['sensory_projection']
        H = L['hidden_nodes']
        w = self.dense(self.snap['bbc']['w_in'])
        names = L['sensory_nodes']
        for n in H:
            cls = A.neuron_class(n)
            if cls not in ('DB', 'VB'):
                continue
            drive = {}
            for ch, targets in proj.items():
                if '_bend_' in ch and n in targets:
                    drive[ch] = w[H.index(n)][names.index(ch)]
            want = '_bend_ventral' if cls == 'DB' else '_bend_dorsal'
            self.assertTrue(drive and all(want in ch for ch in drive), (n, sorted(drive)))
            aps = [0.06 + 0.88 * (int(ch[-2:]) - 1) / 11 for ch in drive]
            self.assertTrue(all(ap >= A.soma_ap(n) - 0.05 for ap in aps), n)
            self.assertLessEqual(max(aps), A.soma_ap(n) + 0.5 + 1e-9)
            # Field-normalised: the summed gain is b_gain for every neuron.
            unit_gain = sum(drive.values())
            self.assertAlmostEqual(unit_gain, next(iter(drive.values())) * len(drive), places=9)
        self.assertEqual(L['weights']['proprioception']['b_mode'], 'stretch')
        # Anterior field: samples from half a body anterior to the soma up to the soma.
        La = self.snap['anterior']['connectome_labels']
        spacing = 0.88 / 11
        for ch, targets in La['sensory_projection'].items():
            if '_bend_' not in ch:
                continue
            ap = 0.06 + 0.88 * (int(ch[-2:]) - 1) / 11
            for n in targets:
                if A.neuron_class(n) in ('DB', 'VB'):
                    self.assertTrue(A.soma_ap(n) - 0.5 - 1e-9 <= ap <= A.soma_ap(n) + 1e-9
                                    or abs(ap - A.soma_ap(n)) <= spacing, (n, ch))

    def test_graded_command_interneurons_and_tonic_drive(self):
        snap = self.snap['forward']
        L = snap['connectome_labels']
        types = {t['name']: t['bio_params'] for t in snap['net']['neuron_types']}
        cell = {n: snap['topo']['layers'][0][i]['type_name'] for i, n in enumerate(L['hidden_nodes'])}
        self.assertEqual(snap['w_hh_rec'], self.snap['v2']['w_hh_rec'])
        self.assertEqual(snap['w_gap'], self.snap['v2']['w_gap'])
        for n, tonic in (('AVBL', 2.5), ('AVBR', 2.5), ('AVAL', -1.0), ('AVAR', -1.0)):
            self.assertEqual(cell[n], f'Interneuron:acetylcholine:graded:{A.neuron_class(n)}')
            self.assertTrue(types[cell[n]]['graded_enabled'])
            self.assertEqual(types[cell[n]]['tonic_current'], tonic)
        # Other interneurons stay spiking without bias; B-type keep their plateau.
        for n in ('AVDL', 'PVCL', 'AIBL'):
            self.assertFalse(types[cell[n]]['graded_enabled'], n)
            self.assertEqual(types[cell[n]]['tonic_current'], 0.0)
        self.assertEqual(types[cell['DB3']]['graded_plateau_mv'], 22.0)
        self.assertEqual(types[cell['DA3']]['graded_plateau_mv'], 0.0)
        self.assertEqual(L['weights']['tonic_current'], {'AVB': 2.5, 'AVA': -1.0})
        self.assertEqual(snap['net']['aarnn_bio']['tonic_current'], 0.0)
        for t in snap['net']['neuron_types']:
            self.assertEqual(set(t['bio_params']), set(snap['net']['aarnn_bio']), t['name'])

    def test_isolated_bbc_circuit(self):
        circuit = {'DB', 'VB', 'DD', 'VD'}
        full, iso, avb = self.snap['bbc_full'], self.snap['isolated'], self.snap['isolated_avb']
        H = iso['connectome_labels']['hidden_nodes']
        O = iso['connectome_labels']['output_nodes']
        cls = [A.neuron_class(n) for n in H]
        rec_f, rec_i, rec_a = (self.dense(x['w_hh_rec'][0]) for x in (full, iso, avb))
        out_f, out_i = self.dense(full['w_out']), self.dense(iso['w_out'])
        gap_f, gap_i, gap_a = (self.dense(x['w_gap'][0]) for x in (full, iso, avb))
        n = len(H)
        for i in range(n):
            for j in range(n):
                onto_circuit = cls[i] in circuit
                if onto_circuit and cls[j] not in circuit:
                    self.assertEqual(rec_i[i][j], 0.0, (H[j], H[i]))
                else:
                    self.assertEqual(rec_i[i][j], rec_f[i][j], (H[j], H[i]))
                touches = (cls[i] in circuit) != (cls[j] in circuit)
                self.assertEqual(gap_i[i][j], 0.0 if touches else gap_f[i][j], (H[i], H[j]))
                # Add-back restores AVB's synapses and gap junctions onto the circuit.
                if onto_circuit and cls[j] == 'AVB':
                    self.assertEqual(rec_a[i][j], rec_f[i][j])
                    self.assertEqual(gap_a[i][j], gap_f[i][j])
        for k in range(len(O)):
            for j in range(n):
                self.assertEqual(out_i[k][j], out_f[k][j] if cls[j] in circuit else 0.0, (H[j], O[k]))
        self.assertTrue(any(out_i[k][j] for k in range(len(O)) for j in range(n)))
        # Stretch input to the circuit is unchanged; the result is a subset of the full wiring.
        self.assertEqual(iso['w_in'], full['w_in'])
        iso_meta = iso['connectome_labels']['weights']['isolation']
        self.assertEqual(iso_meta['mode'], 'bbc')
        self.assertGreater(iso_meta['removed']['neuromuscular'], 0)
        self.assertEqual(avb['connectome_labels']['weights']['isolation']['added_back'], ['AVB'])
        # bbc-minimal: only B -> contralateral D and D -> B links, no circuit gaps.
        mini = self.snap['minimal']
        rec_m, gap_m = self.dense(mini['w_hh_rec'][0]), self.dense(mini['w_gap'][0])
        allowed = {('VB', 'DD'), ('DB', 'VD'), ('VD', 'VB'), ('DD', 'DB')}
        kinds = set()
        for i in range(n):
            for j in range(n):
                if cls[i] in circuit:
                    if rec_m[i][j]:
                        kinds.add((cls[j], cls[i]))
                    self.assertEqual(rec_m[i][j], rec_f[i][j] if (cls[j], cls[i]) in allowed else 0.0)
                    self.assertEqual(gap_m[i][j], 0.0)
        self.assertLessEqual({('VB', 'DD'), ('DB', 'VD'), ('VD', 'VB')}, kinds)
        self.assertEqual(self.dense(mini['w_out']), out_i)


if __name__ == '__main__':
    unittest.main()
