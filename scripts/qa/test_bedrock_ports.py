#!/usr/bin/env python3
"""Real socket collision and world-ownership checks for every managed BDS launch."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from bedrock_ports import Ports,launch_directory,properties


class BedrockPortTests(unittest.TestCase):
    def test_occupied_ipv4_and_ipv6_udp_are_reassigned(self):
        with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as busy:
            busy.bind(('0.0.0.0',0));number=busy.getsockname()[1]
            reservation=Ports()
            try:
                reservation.select({'server-port':str(number),'server-portv6':'0','enable-lan-visibility':'false'})
                self.assertNotEqual(int(reservation.changes['server-port']),number)
                with socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as collision:
                    with self.assertRaises(OSError):collision.bind(('0.0.0.0',int(reservation.changes['server-port'])))
            finally:reservation.close()
        if socket.has_ipv6:
            with socket.socket(socket.AF_INET6,socket.SOCK_DGRAM) as busy:
                busy.setsockopt(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,1)
                busy.bind(('::',0));number=busy.getsockname()[1]
                reservation=Ports()
                try:
                    reservation.select({'server-port':'0','server-portv6':str(number),'enable-lan-visibility':'false'})
                    self.assertNotEqual(int(reservation.changes['server-portv6']),number)
                finally:reservation.close()

    def test_occupied_nethernet_tcp_and_udp_range_are_reassigned(self):
        with socket.socket() as tcp,socket.socket(socket.AF_INET,socket.SOCK_DGRAM) as udp:
            tcp.bind(('127.0.0.1',0));tcp.listen();udp.bind(('127.0.0.1',0))
            reservation=Ports()
            try:
                reservation.select(dict(transport='nethernet',**{'server-ip':'127.0.0.1','server-port':str(tcp.getsockname()[1]),'server-udp-ports':str(udp.getsockname()[1])}))
                self.assertNotEqual(int(reservation.changes['server-port']),tcp.getsockname()[1])
                self.assertNotEqual(int(reservation.changes['server-udp-ports']),udp.getsockname()[1])
            finally:reservation.close()

    def test_launch_preserves_properties_and_rechecks_each_run(self):
        with tempfile.TemporaryDirectory() as tmp,socket.socket() as busy:
            root=Path(tmp);(root/'worlds/Lab/db').mkdir(parents=True)
            (root/'worlds/Lab/db/LOCK').touch();(root/'payload').write_text('worlds are shared, not copied')
            busy.bind(('127.0.0.1',0));busy.listen()
            original=f'# operator settings\ntransport=nethernet\nserver-ip=127.0.0.1\nserver-port={busy.getsockname()[1]}\n'
            (root/'server.properties').write_text(original)
            report=dict(directory=str(root),world='Lab')
            for _ in range(2):
                with launch_directory(report) as (runtime,reservation):
                    directory=Path(runtime['directory'])
                    self.assertEqual((root/'server.properties').read_text(),original)
                    self.assertTrue((directory/'worlds').is_symlink())
                    self.assertNotEqual(int(properties((directory/'server.properties').read_text())['server-port']),busy.getsockname()[1])
                    self.assertTrue(reservation.sockets)
                self.assertEqual((root/'server.properties').read_text(),original)

    @unittest.skipIf(os.name=='nt','POSIX LevelDB lock fixture')
    def test_live_world_lock_prevents_second_server(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);db=root/'worlds/Lab/db';db.mkdir(parents=True)
            code="import fcntl,sys; f=open(sys.argv[1],'a+b'); fcntl.lockf(f,fcntl.LOCK_EX); print('locked',flush=True); sys.stdin.read()"
            child=subprocess.Popen([sys.executable,'-c',code,str(db/'LOCK')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
            try:
                self.assertEqual(child.stdout.readline().strip(),'locked')
                with self.assertRaisesRegex(RuntimeError,'already open'):
                    with launch_directory(dict(directory=str(root),world='Lab')):pass
            finally:child.communicate('');self.assertEqual(child.returncode,0)


if __name__=='__main__':unittest.main()
