#!/usr/bin/env python3
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import minecraft


class DetectionTests(unittest.TestCase):
    def profile(self, root, version, loader):
        p = root / 'versions' / 'fabric' / 'fabric.json'
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(dict(id='fabric', inheritsFrom=version, libraries=[dict(name='net.fabricmc:fabric-loader:'+loader)])))

    def mod(self, root, name, version, game):
        p = root / 'mods' / (name+'.jar')
        p.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(p, 'w') as z:
            z.writestr('fabric.mod.json', json.dumps(dict(id=name, version=version, depends=dict(minecraft=game))))

    @patch.object(minecraft, 'java', return_value=dict(available=True, major=21, path='/test/java'))
    def test_missing_and_wrong_versions_are_explicit(self, _):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            report = minecraft.detect(root)
            self.assertFalse(report['compatible_profiles'])
            self.profile(root, '26.2', '0.19.5')
            self.mod(root, 'fabric-api', '0.160.0+26.2', '~26.2-')
            report = minecraft.detect(root)
            self.assertFalse(report['compatible_profiles'])
            self.assertFalse(report['compatible_fabric_api'])
            self.assertFalse(report['compatible_aarnn_mod'])

    @patch.object(minecraft, 'java', return_value=dict(available=True, major=21, path='/test/java'))
    def test_compatible_isolated_profile_and_corrupt_mod(self, _):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d); game = root / 'isolated'
            self.profile(root, '1.21.1', '0.17.3')
            (root / 'launcher_profiles.json').write_text(json.dumps(dict(profiles={'test':dict(lastVersionId='fabric', gameDir=str(game))})))
            self.mod(game, 'fabric-api', '0.107.0+1.21.1', '~1.21.1')
            self.mod(game, 'aarnn', '0.1.0', '1.21.1')
            report = minecraft.detect(root)
            self.assertTrue(report['compatible_profiles'])
            self.assertTrue(report['compatible_fabric_api'])
            self.assertTrue(report['compatible_aarnn_mod'])
            (game / 'mods/broken.jar').write_text('broken')
            self.assertTrue(minecraft.detect(root)['errors'])

    @patch.object(minecraft, 'java', return_value=dict(available=True, major=21, path='/test/java'))
    def test_conflicting_mods_are_rejected(self, _):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            self.profile(root, '1.21.1', '0.17.3')
            self.mod(root, 'fabric-api', '0.107.0+1.21.1', '~1.21.1')
            (root / 'mods/fabric-api.jar').rename(root / 'mods/compatible.jar')
            self.mod(root, 'fabric-api', '0.160.0+26.2', '~26.2-')
            self.assertTrue(any('Duplicate mod id' in e for e in minecraft.detect(root)['errors']))

    @patch.object(minecraft, 'java', return_value=dict(available=True, major=21, path='/test/java'))
    def test_malformed_metadata_is_reported_without_crashing(self, _):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            self.profile(root, '1.21.1', '0.17.3')
            version = root/'versions/fabric/fabric.json'
            for data in ([], {'libraries':[None]}, {'libraries':{'name':'invalid'}}):
                version.write_text(json.dumps(data))
                self.assertTrue(minecraft.detect(root)['errors'])
            self.profile(root, '1.21.1', '0.17.3')
            (root/'launcher_profiles.json').write_text('{"profiles":{"bad":null}}')
            self.assertTrue(minecraft.detect(root)['errors'])
            (root/'launcher_profiles.json').unlink()
            (root/'mods').mkdir()
            for data in ([], {'id':[], 'version':'1'}, {'id':'bad', 'version':{}}):
                with zipfile.ZipFile(root/'mods/bad.jar', 'w') as archive:
                    archive.writestr('fabric.mod.json',json.dumps(data))
                self.assertTrue(minecraft.detect(root)['errors'])


if __name__ == '__main__':
    unittest.main()
