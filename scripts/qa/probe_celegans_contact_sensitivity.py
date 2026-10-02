#!/usr/bin/env python3
"""Verify that a physical contact reaches both C. elegans bumper input ports.

This runs one Webots worm with a neutral UDS motor responder. A Supervisor
places one collision sphere against each bumper in turn; the responder records
the actual 24-channel sensor frames emitted by the production robot controller.
"""

import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "target/qa/celegans-contact-sensitivity"
SENSORY_COUNT = 24
OUTPUT_COUNT = 96

SUPERVISOR = """from controller import Supervisor
from pathlib import Path
import os

supervisor = Supervisor()
children = supervisor.getRoot().getField("children")
robot = next(children.getMFNode(i) for i in range(children.getCount())
             if children.getMFNode(i).getField("name") is not None
             and children.getMFNode(i).getField("name").getSFString() == "C_ELEGANS_01")
devices = robot.getBaseNodeField("children")
touch = {devices.getMFNode(i).getField("name").getSFString(): devices.getMFNode(i)
         for i in range(devices.getCount())
         if devices.getMFNode(i).getBaseTypeName() == "TouchSensor"}
front = touch["celegans_s_06_touch_front"]
rear = touch["celegans_s_07_touch_rear"]
children.importMFNodeFromString(-1, '''DEF CONTACT_PROBE Solid {
  translation 0 2 0
  children [ Shape { geometry Sphere { radius 0.015 } } ]
  boundingObject Sphere { radius 0.015 }
}''')
probe = supervisor.getFromDef("CONTACT_PROBE")
position = probe.getField("translation")
phase_path = Path(os.environ["NM_CONTACT_PHASE_FILE"])
step_ms = int(supervisor.getBasicTimeStep())
for step in range(100):
    if step == 20:
        position.setSFVec3f(front.getPosition())
    elif step == 50:
        position.setSFVec3f([0, 2, 0])
    elif step == 60:
        position.setSFVec3f(rear.getPosition())
    elif step == 90:
        position.setSFVec3f([0, 2, 0])
    phase = "baseline" if step < 20 else "front" if step < 50 else "gap" if step < 60 else "rear" if step < 90 else "done"
    phase_path.write_text(phase)
    if supervisor.step(step_ms) == -1:
        break
supervisor.simulationQuit(0)
"""


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="run-", dir=OUTPUT) as project_name, \
         tempfile.TemporaryDirectory(prefix="socket-") as socket_name:
        project = Path(project_name)
        (project / "worlds").mkdir()
        supervisor_dir = project / "controllers/contact_probe_supervisor"
        supervisor_dir.mkdir(parents=True)
        (supervisor_dir / "contact_probe_supervisor.py").write_text(SUPERVISOR)
        (project / "controllers/nao_nn_controller_uds").symlink_to(
            ROOT / "webots_world/controllers/nao_nn_controller_uds", target_is_directory=True)
        world = project / "worlds/contact.wbt"
        socket_path = Path(socket_name) / "contact_probe.nn"
        subprocess.run([sys.executable, str(ROOT / "scripts/build_webots_multi_world.py"),
                        "--world", str(world), "--celegans-brains", "contact_probe",
                        "--celegans-proto", str(ROOT / "webots_world/protos/CelegansRobot.proto")],
                       cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        content = world.read_text().replace('controller "nm_world_recorder"',
                                            'controller "contact_probe_supervisor"')
        content = content.replace('"NM_BRAINS=contact_probe"',
                                  f'"NM_BRAINS=contact_probe"\n    "NM_NAO_SOCKET={socket_path}"', 1)
        world.write_text(content)
        phase_path = project / "phase.txt"
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as sock:
            sock.bind(str(socket_path))
            sock.settimeout(0.3)
            env = dict(os.environ, NM_IPC_DISABLE_AER="1",
                       NM_CONTACT_PHASE_FILE=str(phase_path),
                       NM_CELEGANS_TWITCH_FALLBACK="0")
            with (OUTPUT / "webots.log").open("w") as log:
                proc = subprocess.Popen(["webots", "--batch", "--mode=fast", "--no-rendering",
                                         "--stdout", "--stderr", str(world)], cwd=ROOT,
                                        env=env, stdout=log, stderr=subprocess.STDOUT)
                samples = {phase: [] for phase in ("baseline", "front", "gap", "rear", "done")}
                current_phase = "baseline"
                deadline = time.monotonic() + 90
                try:
                    while time.monotonic() < deadline:
                        if proc.poll() is not None:
                            break
                        try:
                            packet, peer = sock.recvfrom(1 << 20)
                        except socket.timeout:
                            continue
                        if packet.startswith(b"{"):
                            continue
                        if len(packet) != 4 + 4 * SENSORY_COUNT:
                            raise AssertionError(f"unexpected sensory packet length: {len(packet)}")
                        values = struct.unpack_from("<" + "f" * SENSORY_COUNT, packet, 4)
                        phase = phase_path.read_text().strip() if phase_path.exists() else "baseline"
                        if phase in samples:
                            current_phase = phase
                        samples[current_phase].append(values)
                        sock.sendto(struct.pack("<" + "f" * OUTPUT_COUNT,
                                                *([0.5] * OUTPUT_COUNT)), peer)
                    else:
                        raise TimeoutError("single-worm Webots contact probe exceeded 90 seconds")
                finally:
                    if proc.poll() is None:
                        proc.terminate()
                    proc.wait(timeout=5)

    report = {phase: {"frames": len(frames),
                      "front_min": min((row[6] for row in frames), default=0.0),
                      "front_max": max((row[6] for row in frames), default=0.0),
                      "rear_min": min((row[7] for row in frames), default=0.0),
                      "rear_max": max((row[7] for row in frames), default=0.0),
                      "front_high": sum(row[6] > 0.5 for row in frames),
                      "rear_high": sum(row[7] > 0.5 for row in frames)}
              for phase, frames in samples.items()}
    (OUTPUT / "report.json").write_text(json.dumps(report, indent=2))
    assert report["baseline"]["frames"] >= 5, report
    assert report["front"]["frames"] >= 5, report
    assert report["rear"]["frames"] >= 5, report
    assert report["baseline"]["front_high"] == report["baseline"]["rear_high"] == 0, report
    assert report["front"]["front_max"] > report["baseline"]["front_max"] + 0.5, report
    assert report["front"]["rear_high"] == 0, report
    assert report["rear"]["rear_max"] > report["baseline"]["rear_max"] + 0.5, report
    assert report["rear"]["front_high"] == 0, report
    assert all(row[6] < 0.5 for row in samples["gap"][-5:]), report
    assert all(row[7] < 0.5 for row in samples["done"][-5:]), report
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
