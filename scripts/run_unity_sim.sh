#!/usr/bin/env bash
# run_unity_sim.sh — AARNN TCP brain-server launcher for Unity
#
# Convenience wrapper around run_sim.sh that passes --sim unity automatically.
# All other arguments are forwarded unchanged.
#
# Usage:
#   ./run_unity_sim.sh [--robots <spec>] [--tcp-host <host>] [--tcp-base-port <n>] [--no-build]
#
# Quick-start:
#   # Launch one C. elegans brain for Unity (listens on 127.0.0.1:7890):
#   ./run_unity_sim.sh
#
#   # Launch two zebrafish brains and one hexapod brain:
#   ./run_unity_sim.sh --robots "zebrafish=2,hexapod=1"
#
# ─────────────────────────────────────────────────────────────────────────────
# Unity project setup
# ─────────────────────────────────────────────────────────────────────────────
#
# This script starts one TCP server per AARNN brain instance.  Each server
# accepts a single persistent connection from the corresponding robot prefab
# in Unity.
#
# 1. NmSimulationManager (MonoBehaviour on a scene-level GameObject):
#      AarnnHost     = "127.0.0.1"
#      AarnnBasePort = 7890          (match --tcp-base-port if you changed it)
#    NmSimulationManager owns the brain registry and assigns port offsets.
#
# 2. Brain IDs are printed to the terminal when this script starts, e.g.:
#      zebrafish_0   127.0.0.1:7890   sensory=32  output=32
#      zebrafish_1   127.0.0.1:7891   sensory=32  output=32
#      hexapod_0     127.0.0.1:7892   sensory=34  output=18
#   Port allocation follows brain index order (brain 0 → base port,
#   brain 1 → base+1, etc.) in the order types appear in --robots.
#
# 3. NmBrainConnector (ScriptableObject):
#    Create one NmBrainConnector asset per robot type in your project:
#      - Set brainId to the brain ID string printed by this script
#        (e.g. "zebrafish_0", "hexapod_0").
#      - sensoryCount and outputCount must match the values printed by this
#        script (see table below).
#    Reference each NmBrainConnector from the corresponding robot prefab's
#    NmRobotController component.
#
# 4. NmRobotController (MonoBehaviour on each robot prefab):
#    - Assign the NmBrainConnector ScriptableObject for this robot type.
#    - Call SendSensoryFrame(float[] sensors) each FixedUpdate with the
#      robot's current sensor readings (array length == sensoryCount).
#    - Read float[] outputs = ReceiveOutputFrame() to get motor commands
#      (array length == outputCount).
#
# 5. To use a non-loopback address (e.g. separate machine):
#      ./run_unity_sim.sh --tcp-host 0.0.0.0 --robots "celegans=1"
#    Then set AarnnHost in NmSimulationManager to the host's reachable IP.
#
# 6. Robot sensory/output counts by type (for NmBrainConnector reference):
#      celegans        sensory=24   output=96
#      drosophila_banc sensory=418  output=48
#      drosophila_fafb sensory=418  output=48
#      hexapod         sensory=34   output=18
#      nao             sensory=250  output=40
#      zebrafish       sensory=32   output=32
#
# ─────────────────────────────────────────────────────────────────────────────

exec "$(dirname "${BASH_SOURCE[0]}")/run_sim.sh" --sim unity "$@"
