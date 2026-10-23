#!/usr/bin/env python3
"""Exercise the actual shared-library ABI, independently of Rust unit calls."""
import ctypes as c
import json
from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[2]
class Abi(unittest.TestCase):
    def test_foreign_buffers_projection_replay_and_independent_handles(self):
        lib=c.CDLL(str(ROOT/'target/debug/libaarnn_embodiment_ffi.so'))
        lib.nm_embodiment_version.restype=c.c_uint32
        self.assertEqual(lib.nm_embodiment_version(),1)
        lib.nm_reference_open.argtypes=[c.c_char_p,c.c_uint32,c.c_char_p,c.c_uint32,c.POINTER(c.c_uint64)]
        lib.nm_reference_step.argtypes=[c.c_uint64,c.c_uint64,c.POINTER(c.c_double),c.c_uint32,c.POINTER(c.c_double),c.c_uint32]
        lib.nm_reference_status.argtypes=[c.c_uint64,c.c_void_p,c.c_uint32]
        lib.nm_reference_project.argtypes=[c.c_uint64,c.POINTER(c.c_double),c.c_uint32,c.POINTER(c.c_double),c.c_uint32]
        lib.nm_reference_close.argtypes=[c.c_uint64]
        lib.nm_reference_reset.argtypes=[c.c_uint64]
        lib.nm_control_mode.argtypes=[c.c_uint64,c.c_uint32]
        handles=[]
        try:
            path=str(ROOT/'sim/content/embodiments/generated/ugot_mecanum.xml').encode()
            for _ in range(2):
                h=c.c_uint64();self.assertEqual(lib.nm_reference_open(path,len(path),None,0,c.byref(h)),0);handles.append(h.value)
            self.assertNotEqual(*handles)
            raw=(c.c_double*19)();raw[11]=3;sensors=(c.c_double*19)();controls=(c.c_double*6)(*([1]*6))
            self.assertEqual(lib.nm_reference_project(handles[0],raw,19,sensors,19),19)
            self.assertEqual(sensors[8],1)
            self.assertEqual(lib.nm_reference_step(handles[0],1,sensors,19,controls,6),0)
            self.assertEqual(list(controls),[0]*6)
            controls[0]=1
            self.assertLess(lib.nm_reference_step(handles[0],1,sensors,19,controls,6),0)
            self.assertEqual(list(controls),[0]*6)
            def status(h):
                b=c.create_string_buffer(65536);n=lib.nm_reference_status(h,b,65536);self.assertGreater(n,0);return json.loads(b.raw[:n])
            self.assertTrue(status(handles[0])['inhibited'])
            self.assertFalse(status(handles[1])['inhibited'])
            self.assertEqual(status(handles[1])['source_step'],0)
            self.assertEqual(lib.nm_control_mode(handles[1],1),0)
            self.assertEqual(lib.nm_reference_step(handles[1],9,sensors,19,controls,6),2)
            self.assertGreater(controls[0],0)
            self.assertEqual(lib.nm_reference_reset(handles[1]),0)
            self.assertEqual(status(handles[1])['reset_epoch'],1)
            self.assertTrue(status(handles[1])['inhibited'])
            self.assertEqual(lib.nm_reference_step(handles[1],1,sensors,19,controls,6),0)
            self.assertEqual(list(controls),[0]*6)
        finally:
            for h in handles:self.assertEqual(lib.nm_reference_close(h),0)
        self.assertLess(lib.nm_reference_step(handles[0],2,sensors,19,controls,6),0)
        self.assertEqual(list(controls),[0]*6)
if __name__=='__main__':unittest.main()
