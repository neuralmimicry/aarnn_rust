#!/usr/bin/env bash
# run_unreal_sim.sh — AARNN TCP brain-server launcher for Unreal Engine
#
# Convenience wrapper around run_sim.sh that passes --sim unreal automatically.
# All other arguments are forwarded unchanged.
#
# Usage:
#   ./run_unreal_sim.sh [--robots <spec>] [--tcp-host <host>] [--tcp-base-port <n>] [--no-build]
#
# Quick-start:
#   # Launch one C. elegans brain for Unreal (listens on 127.0.0.1:7890):
#   ./run_unreal_sim.sh
#
#   # Launch a hexapod brain and two NAO brains:
#   ./run_unreal_sim.sh --robots "hexapod=1,nao=2"
#
# ─────────────────────────────────────────────────────────────────────────────
# Unreal Engine project setup
# ─────────────────────────────────────────────────────────────────────────────
#
# This script starts one TCP server per AARNN brain instance.  Each server
# accepts a single persistent connection from the corresponding robot actor in
# Unreal.
#
# 1. No special socket plugin is required — the servers use plain TCP.
#    The default OS socket API (FSocket / ISocketSubsystem) is sufficient.
#
# 2. In your Level Blueprint or ANmSimManager actor:
#      AarnnHost     = "127.0.0.1"
#      AarnnBasePort = 7890          (override with --tcp-base-port if changed)
#
# 3. Brain IDs are printed to the terminal when this script starts, e.g.:
#      celegans_0   127.0.0.1:7890   sensory=24  output=96
#      hexapod_0    127.0.0.1:7891   sensory=34  output=18
#   Set each ANmRobotController's BrainId property to the matching brain ID
#   string.  Port allocation follows brain index order (brain 0 → base port,
#   brain 1 → base+1, etc.) in the order types appear in --robots.
#
# 4. Wire the NmAarnnSocketComponent to your robot's movement/perception
#    pipeline:
#      - Call SendSensoryFrame() each tick with the robot's sensor readings
#        packed as a float array (length == sensory count for that brain type).
#      - Read ReceiveOutputFrame() to get motor commands (length == output count).
#
# 5. To use a non-loopback address (e.g. separate machine or cloud VM):
#      ./run_unreal_sim.sh --tcp-host 0.0.0.0 --robots "celegans=1"
#    Then set AarnnHost in Unreal to the host's reachable IP address.
#
# 6. Robot sensory/output counts by type (for channel mapping reference):
#      celegans        sensory=24   output=96
#      drosophila_banc sensory=418  output=48
#      drosophila_fafb sensory=418  output=48
#      hexapod         sensory=34   output=18
#      nao             sensory=250  output=40
#      zebrafish       sensory=32   output=32
#
# ─────────────────────────────────────────────────────────────────────────────

exec "$(dirname "${BASH_SOURCE[0]}")/run_sim.sh" --sim unreal "$@"
