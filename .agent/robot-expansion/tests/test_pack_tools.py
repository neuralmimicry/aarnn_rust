from __future__ import annotations
import hashlib
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

TOOLS=Path(__file__).resolve().parents[1]/'tools'
sys.path.insert(0,str(TOOLS))
import aer1_reference as aer
from install import install
from fetch_assets import fetch_one
from pack_common import safe_path

class AerFixtures(unittest.TestCase):
    def test_golden_sensor_frame_and_empty_packet(self):
        golden=bytes.fromhex('41455231e80300000000000000802001')
        self.assertEqual(aer.encode([(1000,4096,1)]),golden)
        self.assertEqual(aer.frame(golden),bytes.fromhex('10000000')+golden)
        self.assertEqual(aer.decode(golden,4096,492),[(1000,4096,1)])
        empty=bytes.fromhex('41455231e803000000000000')
        self.assertEqual(aer.encode([],1000),empty)
        self.assertEqual(aer.decode(empty),[])

    def test_address_overflow_truncation_and_negative_values(self):
        golden=bytes.fromhex('41455231e80300000000000000802001')
        with self.assertRaises(ValueError):aer.decode(golden,16384,64)
        with self.assertRaises(ValueError):aer.decode(golden[:-1])
        with self.assertRaises(ValueError):aer.read_varint(b'\xff'*9+b'\x02',0)
        with self.assertRaises(ValueError):aer.encode([(1,4096,-1)])
        with self.assertRaises(ValueError):aer.encode([(1,2**32,1)])

class InstallationAndIntegrity(unittest.TestCase):
    def test_preview_install_idempotence_and_conflict_refusal(self):
        with tempfile.TemporaryDirectory() as temporary:
            base=Path(temporary);src=base/'pack';repo=base/'repo'
            src.mkdir();repo.mkdir();(repo/'.git').mkdir()
            (repo/'Cargo.toml').write_text('[package]\nname="test"\n')
            (repo/'AGENTS.md').write_text('retain user instructions')
            (src/'AGENTS.md').write_text('task instructions')
            (src/'START_HERE.md').write_text('load task')
            preview=install(src,repo)
            self.assertEqual(preview['mode'],'preview')
            self.assertFalse((repo/'.agent').exists())
            result=install(src,repo,True)
            self.assertEqual(result['created'],2)
            self.assertEqual((repo/'AGENTS.md').read_text(),'retain user instructions')
            self.assertEqual(install(src,repo,True)['created'],0)
            (repo/'.agent/robot-expansion/AGENTS.md').write_text('user edits')
            (src/'additional.md').write_text('should not be written after conflict')
            with self.assertRaises(ValueError):install(src,repo,True)
            self.assertFalse((repo/'.agent/robot-expansion/additional.md').exists())

    def test_asset_integrity_no_overwrite_and_path_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);data=b'known source';p=root/'source.bin';p.write_bytes(data)
            a=dict(path='source.bin',sha256=hashlib.sha256(data).hexdigest(),size_bytes=len(data),url='https://raw.githubusercontent.com/unused')
            self.assertEqual(fetch_one(a,root),'verified_existing')
            p.write_bytes(b'user changes')
            with self.assertRaises(ValueError):fetch_one(a,root)
            self.assertEqual(p.read_bytes(),b'user changes')
            with self.assertRaises(ValueError):safe_path(root,'../escape')
            outside=root.parent/'outside'
            (root/'link').symlink_to(outside)
            with self.assertRaises(ValueError):safe_path(root,'link/file')

if __name__=='__main__':unittest.main()
