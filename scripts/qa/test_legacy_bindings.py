#!/usr/bin/env python3
"""Named preflight contract tests; real Runner evidence is a separate lane."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
import subprocess
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from mujoco_bindings import prepare


class Bindings(unittest.TestCase):
    def test_mixed_population_selects_each_brain_and_leaves_new_unbound_bodies_npc(self):
        command=['python3',str(ROOT/'scripts/robot_profiles.py'),'managed-brains','hexapod=2,ugot_mecanum=1,nao=1']
        output=subprocess.check_output(command,text=True).splitlines()
        self.assertEqual(output,['hexapod_0 hexapod','hexapod_1 hexapod','nao_0 nao'])
        output=subprocess.check_output(command+['--network-map','ugot_mecanum_0=/tmp/selected.json'],text=True).splitlines()
        self.assertIn('ugot_mecanum_0 ugot_mecanum',output)
    def fixture(self,profile,root):
        record=json.loads((ROOT/f'sim/content/embodiments/generated/{profile}.json').read_text());io=record['io']
        sensory=io.get('neural_sensory_names',[s['name'] for s in io['sensors']])
        output=io.get('neural_output_names',[n.split('_',3)[-1] for n in io['output_names']] if profile=='celegans' else io['output_names'])
        snapshot=dict(net=dict(num_sensory_neurons=len(sensory),num_output_neurons=len(output)),connectome_labels=dict(sensory_nodes=sensory,output_nodes=output))
        path=root/(profile+'.json');path.write_text(json.dumps(snapshot));return record,snapshot,path

    def test_all_twenty_eight_named_contracts_and_distinct_brains(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);out=root/'bindings'
            records=sorted((ROOT/'sim/content/embodiments/generated').glob('*.xml'))
            self.assertEqual(len(records),28)
            for index,model in enumerate(records):
                profile=model.stem;record,snapshot,path=self.fixture(profile,root)
                before=path.read_bytes()
                prepare(profile,profile+'_0',path,'127.0.0.1',40000+index,out)
                self.assertEqual(path.read_bytes(),before)
                contract=json.loads((out/(profile+'_0.contract.json')).read_text())
                self.assertEqual(contract['s_names'],[s['name'] for s in record['io']['sensors']])
                self.assertEqual(contract['o_names'],record['io']['output_names'])
            mapping=json.loads((out/'mujoco-bindings.json').read_text())
            self.assertEqual(len(mapping),28);self.assertEqual(len({b['port'] for b in mapping.values()}),28)

    def test_mismatch_and_dataset_alias_rejected_before_contract_creation(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);record,snapshot,path=self.fixture('hexapod',root)
            snapshot['connectome_labels']['output_nodes'].reverse();path.write_text(json.dumps(snapshot))
            with self.assertRaisesRegex(ValueError,'named sensory/output'):prepare('hexapod','bad',path,'127.0.0.1',40000,root/'bad')
            self.assertFalse((root/'bad').exists())
            _,_,banc=self.fixture('drosophila_banc',root)
            with self.assertRaisesRegex(ValueError,'named sensory/output'):prepare('drosophila_fafb','bad',banc,'127.0.0.1',40000,root/'bad')
            with self.assertRaisesRegex(ValueError,'loopback'):prepare('hexapod','bad',path,'192.168.1.66',40000,root/'bad')
            _,_,valid=self.fixture('hexapod',root);cfg=root/'wrong-config.json';cfg.write_text(json.dumps(dict(num_sensory_neurons=1,num_output_neurons=18)))
            with self.assertRaisesRegex(ValueError,'no runtime resize'):prepare('hexapod','bad',valid,'127.0.0.1',40000,root/'bad',cfg)


if __name__=='__main__':unittest.main()
