#!/usr/bin/env python3
"""SIM-CONTENT-001: real catalogue generation, adapter assets and spatial oracles."""
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import sim_content
from robot_profiles import PROFILES

ROOT = sim_content.ROOT


class ContentParity(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.catalogue = sim_content.compile_catalog()

    def test_exports_match_the_authoritative_compiler(self):
        from regenerate_simulator_assets import generated_assets
        for path, expected in generated_assets():
            with self.subTest(path=path):
                self.assertTrue(path.exists(), str(path))
                self.assertEqual(path.read_text(), expected)
        raw = json.dumps({k:v for k,v in self.catalogue.items() if k!='digest'},sort_keys=True,separators=(',',':')).encode()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), self.catalogue['digest'])

    def test_habitats_have_bounded_spatially_distinct_content(self):
        habitats = self.catalogue['habitats']
        self.assertEqual({h['id'] for h in habitats}, {'agar','orchard','terrain','room','stream'})
        self.assertEqual(len({json.dumps(h['objects']) for h in habitats}),5)
        for h in habitats:
            self.assertTrue(50 < len(h['objects']) <= 256)
            self.assertTrue(any(o['cue'] for o in h['objects']))
            self.assertTrue(any(o['collision'] for o in h['objects']))
            ids=[]
            for o in h['objects']:
                ids.append(o['id'])
                self.assertIn(o['shape'], ('box','sphere','cylinder'))
                self.assertTrue(all(math.isfinite(v) for v in o['position']+o['size']+o['colour']))
                self.assertTrue(all(v>0 for v in o['size']))
                self.assertTrue(all(0<=v<=1 for v in o['colour']))
            self.assertEqual(len(ids),len(set(ids)))

    def test_profiles_preserve_channel_identity_and_biology_landmarks(self):
        profiles=self.catalogue['profiles']
        self.assertEqual({p['id'] for p in profiles},set(PROFILES))
        for p in profiles:
            ref=PROFILES[p['id']]
            self.assertEqual((p['sensory'],p['output']),(ref.sensory,ref.output))
            self.assertTrue(20<len(p['parts'])<512)
            self.assertFalse(any(o['collision'] for o in p['parts']))
            if p['id']!='nao':
                self.assertEqual(len(p['sensor_names']),p['sensory'])
                self.assertEqual(len(p['output_names']),p['output'])
                self.assertEqual(len(set(p['sensor_names'])),p['sensory'])
        worm=next(p for p in profiles if p['id']=='celegans')
        self.assertFalse(any('eye' in o['id'] for o in worm['parts']))
        self.assertTrue(any(o['id'].startswith('pharynx') for o in worm['parts']))
        self.assertEqual(sum(o['id'].startswith('muscle_') for o in worm['parts']), 95)
        names=worm['output_names']
        for segment,row in enumerate(worm['muscle_channels']):
            for group,index in zip(('MDL','MDR','MVL','MVR'),row):
                if index>=0: self.assertTrue(names[index].endswith(f'_{group}{segment+1:02}'))
        self.assertNotIn(names.index('celegans_o_095_MVULVA'),sum(worm['muscle_channels'],[]))
        nao=next(p for p in profiles if p['id']=='nao')
        room=next(h for h in self.catalogue['habitats'] if h['id']=='room')
        table=next(o for o in room['objects'] if o['id']=='table_top')
        self.assertLess(table['position'][2]+table['size'][2]/2,nao['body_length']*.75,
                        'table must be below shoulder reach in the shared humanoid reference')
        banc=next(p for p in profiles if p['id']=='drosophila_banc')
        fafb=next(p for p in profiles if p['id']=='drosophila_fafb')
        self.assertEqual(banc['parts'],fafb['parts'])
        self.assertNotEqual(banc['output_names'],fafb['output_names'])

    def test_spatial_sensor_behaviour_and_retina_history(self):
        subprocess.run(['node', str(ROOT/'scripts/qa/test_simulator_sensors.cjs')],cwd=ROOT,check=True)

    def test_webots_generators_use_shared_habitats_and_do_not_mutate_configs(self):
        import build_webots_celegans_assets as worm
        import build_webots_drosophila_assets as fly
        import build_webots_zebrafish_assets as fish
        import build_webots_multi_world as multi
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'worm.wbt'; worm.generate_world(path)
            self.assertIn('nm_agar_bacterial_lawn_0', path.read_text())
            path=Path(folder)/'fly.wbt'
            fly.generate_world(path,ROOT/'webots_world/protos/DrosophilaBancRobot.proto',ROOT/'webots_world/protos/DrosophilaFafbRobot.proto','DrosophilaBancRobot','DrosophilaFafbRobot','banc_test','fafb_test')
            text=path.read_text()
            self.assertIn('NM_BRAINS=banc_test',text);self.assertIn('NM_BRAINS=fafb_test',text)
            self.assertIn('nm_orchard_leaf_0_0',text)
            self.assertIn('nm_stream_prey_0',fish.build_world())
            self.assertIn('nm_terrain_graded_step_0',multi.webots_habitats([('HexapodRobot','hex','hex_0',.19,'hexapod')],[(0,0)]))

    def test_regenerated_robot_definitions_match_sources(self):
        import build_webots_celegans_assets as worm
        import build_webots_drosophila_assets as fly
        import build_webots_zebrafish_assets as fish
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'worm.proto'
            p=next(p for p in self.catalogue['profiles'] if p['id']=='celegans')
            labels=[name.split('_',3)[-1] for name in p['output_names']]
            worm.generate_proto(labels,path,'../../network_celegans.json','../configs/config_celegans_webots.json')
            self.assertEqual(path.read_text(),(ROOT/'webots_world/protos/CelegansRobot.proto').read_text())
            for name,kind in [('DrosophilaBancRobot','drosophila_banc'),('DrosophilaFafbRobot','drosophila_fafb'),('DrosophilaRobot','drosophila')]:
                data=json.loads((ROOT/f'webots_world/configs/config_{kind}_webots.io_alignment.json').read_text())
                labels=[v['connectome_node_id'] for v in data['output_channels']]
                path=Path(folder)/(name+'.proto')
                fly.generate_proto(labels,path,f'../../network_{kind}.json',f'../configs/config_{kind}_webots.json',proto_name=name,brain_id='default',default_robot_name=kind+'_robot',include_compound_eyes=True,eye_camera_width=32,eye_camera_height=24)
                self.assertEqual(path.read_text(),(ROOT/f'webots_world/protos/{name}.proto').read_text())
            self.assertEqual(fish.build_proto(),(ROOT/'webots_world/protos/ZebrafishRobot.proto').read_text())

if __name__=='__main__': unittest.main()
