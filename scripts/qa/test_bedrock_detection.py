#!/usr/bin/env python3
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import bedrock
import minecraft


class BedrockDetectionTests(unittest.TestCase):
    @patch.dict(os.environ,{},clear=True)
    def test_versioned_developer_install_is_discovered_without_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            for version in ('1.26.9.1','1.26.45.1'):
                folder=root/'Developer'/('bedrock-server-'+version);folder.mkdir(parents=True)
                (folder/'bedrock_server').write_text('not an executable probe')
            with patch('pathlib.Path.home',return_value=root),patch('shutil.which',return_value=None):
                self.assertEqual(bedrock.server_directory(),root/'Developer/bedrock-server-1.26.45.1')
                self.assertEqual(bedrock.server_directory(root/'explicit'),root/'explicit')

    def configured(self,root):
        metadata=bedrock.read_json(bedrock.PACKS/'bedrock-content.json');ids=metadata['pack_ids']
        binary=root/'bedrock_server';binary.write_text('#!/usr/bin/env python3\nprint("unsupported script module")\n');binary.chmod(0o700)
        (root/'server.properties').write_text('level-name=AARNN\nallow-cheats=true\n')
        for kind,folder in [('behavior','server'),('resource','resource')]:
            target=root/(kind+'_packs')/('AARNN_Lab_BP' if kind=='behavior' else 'AARNN_Lab_RP')
            shutil.copytree(bedrock.PACKS/folder,target)
        world=root/'worlds/AARNN';world.mkdir(parents=True)
        for name in ('world_behavior_packs.json','world_resource_packs.json'):shutil.copy2(bedrock.PACKS/name,world/name)
        config=root/'config'/ids['script'];shutil.copytree(bedrock.PACKS/'server-config',config)
        (config/'variables.json').write_text(json.dumps(dict(AARNN_ALLOW_SANDBOX_INFERENCE=True,AARNN_CONTENT_DIGEST=metadata['content_digest'])))
        (config/'secrets.json').write_text('{"AARNN_AUTHORIZATION":"private test fixture"}')
        return config

    @patch.dict(os.environ,{},clear=True)
    def test_absent_and_forced_selection_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            report=bedrock.detect(Path(tmp));self.assertEqual(report['status'],'unavailable')
            self.assertEqual(minecraft.select_edition('bedrock',{'compatible_profiles':[1]},report),'bedrock')
            self.assertEqual(minecraft.select_edition('auto',{},report),'java')

    @patch.dict(os.environ,{},clear=True)
    def test_configured_native_and_conflicting_java(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);config=self.configured(root);report=bedrock.detect(root)
            self.assertFalse(report['errors']);self.assertFalse(report['native_runtime_verified'])
            self.assertEqual(minecraft.select_edition('auto',{},report),'bedrock')
            self.assertEqual(minecraft.select_edition('auto',dict(compatible_profiles=[1],compatible_fabric_api=True,compatible_aarnn_mod=True,errors=[]),report),'java')
            self.assertNotIn('private test fixture',json.dumps(report))
            vanilla=root/'behavior_packs/vanilla';vanilla.mkdir()
            (vanilla/'manifest.json').write_text('{"header": {/* stock JSONC */},}')
            self.assertFalse(bedrock.detect(root)['errors'])
            (config/'permissions.json').write_text('{"allowed_modules":[]}')
            self.assertEqual(bedrock.detect(root)['status'],'unavailable')

    @patch.dict(os.environ,{},clear=True)
    def test_malformed_and_stale_pack_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);self.configured(root)
            (root/'behavior_packs/AARNN_Lab_BP/scripts/main.js').write_text('stale')
            self.assertTrue(bedrock.detect(root)['errors'])
            (root/'worlds/AARNN/world_behavior_packs.json').write_text('null')
            self.assertTrue(bedrock.detect(root)['errors'])

    @patch.dict(os.environ,{},clear=True)
    def test_embedded_export_cannot_shadow_server_variant(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);self.configured(root)
            copy=root/'worlds/AARNN/behavior_packs/exported';copy.mkdir(parents=True)
            shutil.copy2(bedrock.PACKS/'offline/manifest.json',copy/'manifest.json')
            self.assertTrue(any('duplicate' in error for error in bedrock.detect(root)['errors']))

    def test_missing_runtime_readiness_is_managed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);self.configured(root);report=bedrock.detect(root)
            with contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(bedrock.launch(report,timeout=1),3)


if __name__=='__main__':unittest.main()
