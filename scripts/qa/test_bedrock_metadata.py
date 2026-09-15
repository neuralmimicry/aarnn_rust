#!/usr/bin/env python3
"""Metadata preservation and malformed-input rejection for the native QA bootstrap."""
import struct
import unittest
from bedrock_metadata import enable_script_experiments


def tag(kind,name,payload):
    name=name.encode();return bytes([kind])+struct.pack('<H',len(name))+name+payload


def metadata(fields):
    body=b'\x0a\0\0'+fields+b'\0'
    return struct.pack('<II',10,len(body))+body


class Metadata(unittest.TestCase):
    def test_native_fields_and_unrelated_experiments_preserved(self):
        version=tag(3,'StorageVersion',struct.pack('<i',10))
        # Opaque native list/string/array values must survive without rewriting.
        values=tag(9,'lastOpenedWithVersion',b'\3'+struct.pack('<i5i',5,1,26,45,1,0))
        seed=tag(4,'RandomSeed',struct.pack('<q',481516))
        other=tag(1,'data_driven_biomes',b'\1')
        raw=metadata(version+values+seed+tag(10,'experiments',other+tag(1,'gametest',b'\0')+b'\0'))
        changed=enable_script_experiments(raw)
        for field in (version,values,seed,other):self.assertIn(field,changed)
        self.assertNotIn(tag(1,'gametest',b'\0'),changed)
        for name in ('gametest','experiments_ever_used','saved_with_toggled_experiments'):
            self.assertEqual(changed.count(tag(1,name,b'\1')),1)
        self.assertEqual(changed,enable_script_experiments(changed))
        self.assertEqual(struct.unpack_from('<I',changed,4)[0],len(changed)-8)

    def test_bad_header_truncation_and_negative_array_rejected(self):
        for raw in (b'',metadata(b'')[:-1],metadata(tag(7,'bad',struct.pack('<i',-1))),metadata(b'\xff\0\0')):
            with self.subTest(raw=raw),self.assertRaises(ValueError):enable_script_experiments(raw)

    def test_nesting_bound(self):
        fields=b''
        for _ in range(40):fields=tag(10,'nested',fields+b'\0')
        with self.assertRaises(ValueError):enable_script_experiments(metadata(fields))


if __name__=='__main__':unittest.main()
