#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""NAO get-up probe: find a stand-up motor program for a fallen NAO.

The generated NAO (MuJoCo) starts lying on its back or front. A get-up
program is a short sequence of keyframes; each keyframe holds targets for ten
symmetric joint groups (left/right mirrored where the joint mirrors) for a
duration. Targets pass the adapter's servo path (200 ms decode, per-joint
speed limit), as a network's outputs would. The program ends in the walk's
StandInit posture.

``evaluate`` scores a program: uprightness x torso height, held at the end.
``search`` refines a seed program with a (mu, lambda) evolution strategy
(multiprocessing), the "training" of the innate program before it is wired
into the network (scripts/build_nao_walk_network_json.py --getup).

  target/robot-mujoco-venv/bin/python scripts/qa/nao_getup_probe.py --pose back --search 400 --out target/qa/nao_getup/back.json
"""
import argparse
import json
import math
import sys
from multiprocessing import Pool
from pathlib import Path

import mujoco
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from build_nao_stand_network_json import posture_angle  # noqa: E402

# Keyframe groups, the squat tail and its timing: shared with the network
# builder (scripts/nao_getup_program.py), which replays the same targets.
from nao_getup_program import GROUPS, RISE_S, SQUAT, SQUAT_HOLD_S, STAND_S, SUCCESS_HEIGHT  # noqa: E402,F401

# Hand-written seeds (rad), one row per keyframe: duration, then GROUPS.
SEEDS = {
    # Back: prop on the arms, sit up with the trunk flexed, pull the feet in
    # under the hips, lean forward over them, rise.
    "back": [
        [0.8, 2.0, 0.3, 0.0, -0.3, 0.0, 0.0, -0.3, 0.6, -0.4, 0.0],
        [0.8, 2.0, 0.2, 0.0, -1.2, -0.6, 0.3, -1.5, 1.2, -0.6, 0.0],
        [0.8, 1.6, 0.1, 0.0, -0.4, -0.8, 0.4, -1.5, 2.1, -1.0, 0.0],
        [1.0, 0.5, 0.2, 0.0, -0.3, -0.4, 0.1, -1.3, 2.1, -1.1, 0.0],
        [1.2, 1.4, 0.2, -1.4, -0.4, 0.0, 0.0, -0.8, 1.6, -0.8, 0.0],
    ],
    # Front, wide (how the NAO itself gets up from its belly): push up on the
    # arms, open the hips (hip yaw-pitch) and roll the thighs out so the knees
    # come forward beside the trunk, push back onto the feet into a wide frog
    # squat, close the legs into the balanced squat (appended), rise.
    "front_wide": [
        [0.8, 0.2, 0.25, 0.0, -1.3, 0.0, 0.0, -0.3, 0.3, 0.0, 0.0],
        [0.8, 0.1, 0.25, 0.0, -0.1, 0.0, 0.0, -0.3, 0.3, 0.0, 0.0],
        [1.0, 0.1, 0.25, 0.0, -0.1, -0.9, 0.35, -1.45, 2.0, -0.6, -0.2],
        [1.0, 0.7, 0.2, 0.0, -0.2, -0.9, 0.3, -1.5, 2.1, -1.15, -0.2],
        [1.0, 1.2, 0.2, -1.0, -0.4, -0.5, 0.1, -1.3, 2.1, -1.1, 0.0],
    ],
    # Front: push up on the arms, draw the knees under the hips, sit back onto
    # the feet in a squat, rise.
    "front": [
        [0.8, 0.0, 0.3, 0.0, -1.2, 0.0, 0.0, -0.4, 0.4, 0.0, 0.0],
        [0.8, 0.0, 0.2, 0.0, -0.1, 0.0, 0.0, -1.4, 2.0, -0.2, 0.0],
        [0.8, 0.3, 0.2, 0.0, -0.1, -0.3, 0.0, -1.5, 2.1, -1.1, 0.0],
        [1.0, 1.0, 0.2, 0.0, -0.4, -0.2, 0.0, -1.3, 2.1, -1.1, 0.0],
        [1.2, 1.4, 0.2, -1.4, -0.4, 0.0, 0.0, -0.8, 1.6, -0.8, 0.0],
    ],
}


class Body:
    def __init__(self, open_floor=True):
        self.m = mujoco.MjModel.from_xml_path(str(ROOT / "sim/content/embodiments/generated/nao.xml"))
        m = self.m
        if open_floor:
            # Only the floor collides: an evolved program found the room's
            # step behind the robot and pushed on it (24 Nov), which fails
            # wherever a real fall happens. A get-up must work on open floor.
            for g in range(m.ngeom):
                if m.geom_bodyid[g] == 0 and (mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_GEOM, g) or "") != "ground":
                    m.geom_contype[g] = 0
                    m.geom_conaffinity[g] = 0
        self.acts = {mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_ACTUATOR, i): i for i in range(m.nu)}
        self.lo, self.hi = m.actuator_ctrlrange[:, 0].copy(), m.actuator_ctrlrange[:, 1].copy()
        self.vmax = m.actuator_user[:, 0].copy()
        self.qadr = np.array([m.jnt_qposadr[m.actuator_trnid[i, 0]] for i in range(m.nu)])
        self.posture = np.array([min(max(posture_angle(mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_ACTUATOR, i)), self.lo[i]),
                                         self.hi[i]) for i in range(m.nu)])

    def targets(self, frame):
        """10 values: symmetric (right mirrored); 20 values: left then right,
        both in the mirrored convention (a symmetric pose repeats its numbers),
        for asymmetric moves such as rolling over."""
        want = self.posture.copy()
        left = list(frame[:len(GROUPS)])
        right = list(frame[len(GROUPS):2 * len(GROUPS)]) if len(frame) >= 2 * len(GROUPS) else left
        for (joint, mirror), lv, rv in zip(GROUPS, left, right):
            for side, sgn, value in (("L", 1.0, lv), ("R", float(mirror), rv)):
                i = self.acts.get(side + joint)
                if i is not None:
                    want[i] = value * sgn
        return np.clip(want, self.lo, self.hi)

    def fallen(self, pose, tilt=0.0, roll=0.0, noise=0.0, seed=0, phi=None, x=0.0, y=0.0, yaw=0.0):
        """Lying on the back or front; `tilt`/`roll` (rad) and joint `noise`
        vary the start, as no two falls land alike. `phi` (rad) instead
        rotates the body about its own long axis before laying it down (0
        chest-down, pi chest-up, +-pi/2 on a side); `x`, `y`, `yaw` place it
        in the room (heading of the head)."""
        d = mujoco.MjData(self.m)
        rng = np.random.default_rng(seed)
        d.qpos[self.qadr] = np.clip(self.posture + rng.normal(0.0, noise, len(self.qadr)), self.lo, self.hi) \
            if noise else self.posture
        if phi is None:
            pitch = (-math.pi / 2 if pose == "back" else math.pi / 2) + tilt   # back: chest up
            qp = np.array([math.cos(pitch / 2), 0.0, math.sin(pitch / 2), 0.0])
            qr = np.array([math.cos(roll / 2), math.sin(roll / 2), 0.0, 0.0])
            q = np.zeros(4)
            mujoco.mju_mulQuat(q, qp, qr)
        else:
            pitch = math.pi / 2 + tilt
            qy = np.array([math.cos(yaw / 2), 0.0, 0.0, math.sin(yaw / 2)])
            qp = np.array([math.cos(pitch / 2), 0.0, math.sin(pitch / 2), 0.0])
            qz = np.array([math.cos(phi / 2), 0.0, 0.0, math.sin(phi / 2)])
            q1, q = np.zeros(4), np.zeros(4)
            mujoco.mju_mulQuat(q1, qy, qp)
            mujoco.mju_mulQuat(q, q1, qz)
        d.qpos[3:7] = q
        d.qpos[0], d.qpos[1], d.qpos[2] = x, y, 0.11 if phi is None else 0.14
        mujoco.mj_forward(self.m, d)
        return d


# Starts a robust program must handle: (tilt, roll, joint noise, seed).
STARTS = [(0.0, 0.0, 0.0, 0), (0.15, 0.1, 0.05, 1), (-0.15, -0.1, 0.05, 2)]


def evaluate_robust(program, pose):
    """Worst score over STARTS (a program must work from every one)."""
    runs = [evaluate(program, pose, start=s) for s in STARTS]
    worst = min(runs, key=lambda r: r[0])
    info = dict(worst[1], successes=sum(1 for r in runs if r[1]["success"]), starts=len(STARTS))
    return worst[0], info


def evaluate(program, pose, record=False, start=(0.0, 0.0, 0.0, 0), initial=None, open_floor=True):
    """program: list of [duration, 10 group targets]; returns (score, info).
    `initial`: (qpos, qvel) to continue from (after a roll) instead of a fall.
    `open_floor` False keeps the room's fixtures."""
    body = Body(open_floor=open_floor)
    m = body.m
    d = body.fallen(pose, *start)
    if initial is not None:
        d.qpos[:], d.qvel[:] = initial
        mujoco.mj_forward(m, d)
    dt = m.opt.timestep
    decoded = d.qpos[body.qadr].copy()
    squat = body.targets(SQUAT)
    rise = [(RISE_S / 4, squat + (body.posture - squat) * k / 4) for k in range(1, 5)]
    frames = ([(max(0.2, float(f[0])), body.targets(f[1:])) for f in program] + [(SQUAT_HOLD_S, squat)]
              + rise + [(STAND_S, body.posture)])
    squat_end = 1.0 + sum(f[0] for f in frames[:len(program) + 1])
    squat_score = 0.0
    trace, t = [], 0.0
    heights, best_pose, balance = [], 0.0, []
    feet = [mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, n) for n in ("LAnkleRoll_link", "RAnkleRoll_link")]
    total = len(frames) and sum(f[0] for f in frames)
    settle = 1.0
    schedule = [(settle, decoded.copy())] + frames
    for duration, want in schedule:
        for _ in range(int(duration / dt)):
            alpha = dt / (0.2 + dt)
            target = decoded + alpha * (want - decoded)
            decoded = decoded + np.clip(target - decoded, -body.vmax * dt, body.vmax * dt)
            d.ctrl[:] = decoded
            mujoco.mj_step(m, d)
            t += dt
            if abs(t - squat_end) < dt / 2:
                # How well the squat was reached: upright trunk, centre of mass
                # over the feet, trunk at squat height.
                mid = 0.5 * (d.xpos[feet[0]][:2] + d.xpos[feet[1]][:2])
                off = float(np.linalg.norm(d.subtree_com[1][:2] - mid))
                squat_score = (max(float(d.xmat[1][8]), 0.0) * math.exp(-off / 0.04)
                               * min(1.0, float(d.xpos[1][2]) / 0.24))
            if int(t / dt) % 25 == 0:
                heights.append(float(d.xpos[1][2]))
                # Balance over the feet: centre of mass above the feet's
                # midpoint while the torso is off the floor (late in the program).
                if t > settle + 0.5 * total:
                    mid = 0.5 * (d.xpos[feet[0]][:2] + d.xpos[feet[1]][:2])
                    off = float(np.linalg.norm(d.subtree_com[1][:2] - mid))
                    balance.append(math.exp(-off / 0.05) * min(1.0, float(d.xpos[1][2]) / 0.25))
                best_pose = max(best_pose, max(float(d.xmat[1][8]), 0.0) * float(d.xpos[1][2]))
                if record:
                    trace.append([round(t, 3)] + [round(float(v), 5) for v in d.qpos])
    up = float(d.xmat[1][8])            # torso z-axis . world z
    height = float(d.xpos[1][2])
    # Shaped score: final uprightness x height, the best such pose reached on
    # the way (credit for getting onto knees and feet before the final rise)
    # and the mean torso height (credit for leaving the floor at all).
    score = (max(up, 0.0) * height + 0.5 * best_pose + 0.5 * float(np.mean(heights))
             + 0.2 * (float(np.mean(balance)) if balance else 0.0) + 0.5 * squat_score)
    success = up > 0.9 and height > SUCCESS_HEIGHT
    info = dict(pose=pose, upright=round(up, 3), height=round(height, 3), squat=round(squat_score, 3), success=success,
                seconds=round(t, 2))
    if record:
        info["trace"] = trace
    return score + (0.5 if success else 0.0), info


# Back: roll onto the front, then the front get-up. A roll seed: reach the
# left arm across the chest, bend and lift the left leg over the right, turn
# the hips and push off with the right arm (asymmetric: left then right).
ROLL_SEED = [
    [0.8, 0.5, -0.3, -1.0, -1.2, -0.4, 0.4, -1.0, 1.2, -0.5, 0.0,   1.8, 0.8, 0.0, -0.3, -0.4, 0.0, -0.3, 0.3, 0.0, 0.0],
    [0.8, -0.5, -0.3, -1.0, -1.2, -0.6, 0.6, -1.4, 1.5, -0.5, 0.2,  2.0, 1.2, 0.0, -0.3, -0.6, -0.2, -0.1, 0.2, 0.0, 0.0],
    [0.8, 0.0, 0.3, 0.0, -1.0, 0.0, 0.0, -0.4, 0.4, 0.0, 0.0,       0.0, 0.3, 0.0, -1.0, 0.0, 0.0, -0.4, 0.4, 0.0, 0.0],
]


def evaluate_roll(roll, front_program, start=(0.0, 0.0, 0.0, 0)):
    """Roll keyframes from the back, then the front program. Score: how prone
    the body is when the roll ends (torso x-axis pointing down) plus the
    get-up's own score once prone."""
    body = Body()
    m = body.m
    d = body.fallen("back", *start)
    dt = m.opt.timestep
    decoded = d.qpos[body.qadr].copy()
    for duration, want in [(1.0, decoded.copy())] + [(max(0.2, float(f[0])), body.targets(f[1:])) for f in roll]:
        for _ in range(int(duration / dt)):
            decoded = decoded + np.clip(decoded + dt / (0.2 + dt) * (want - decoded) - decoded, -body.vmax * dt, body.vmax * dt)
            d.ctrl[:] = decoded
            mujoco.mj_step(m, d)
    prone = (1.0 - float(d.xmat[1][6])) / 2.0      # x-axis z: -1 prone -> 1
    state = (d.qpos.copy(), d.qvel.copy())
    score, info = evaluate(front_program, "front", start=start, initial=state)
    return prone + (score if prone > 0.8 else 0.0), dict(info, prone=round(prone, 3))


def _eval_roll(args):
    vec, shape, front_program = args
    runs = [evaluate_roll(vec.reshape(shape).tolist(), front_program, s) for s in STARTS]
    worst = min(runs, key=lambda r: r[0])
    return worst[0], dict(worst[1], successes=sum(1 for r in runs if r[1]["success"]))


def _eval(args):
    vec, pose, shape = args
    return evaluate_robust(vec.reshape(shape).tolist(), pose)


def search_roll(seed, front_program, budget, workers=4, sigma=0.35, seed_rng=1):
    rng = np.random.default_rng(seed_rng)
    shape = np.array(seed).shape
    mean = np.array(seed, dtype=float).ravel()
    scale = np.ones_like(mean) * sigma
    scale[::shape[1]] = 0.15
    lam, mu = 16, 4
    best = (-1.0, mean.copy())
    with Pool(workers) as pool:
        for g in range(max(1, budget // lam)):
            pop = [mean + scale * rng.standard_normal(mean.size) for _ in range(lam)]
            for p in pop:
                p[::shape[1]] = np.clip(p[::shape[1]], 0.2, 2.5)
            res = pool.map(_eval_roll, [(p, shape, front_program) for p in pop])
            order = np.argsort([-r[0] for r in res])
            mean = np.mean([pop[i] for i in order[:mu]], axis=0)
            if res[order[0]][0] > best[0]:
                best = (res[order[0]][0], pop[order[0]].copy())
            print(json.dumps(dict(gen=g, best=round(best[0], 3), gen_best=res[order[0]][1])), flush=True)
            scale *= 0.97
    return best[1].reshape(shape).tolist(), best[0]


def search(seed, pose, budget, workers=4, sigma=0.35, seed_rng=1):
    rng = np.random.default_rng(seed_rng)
    shape = np.array(seed).shape
    mean = np.array(seed, dtype=float).ravel()
    scale = np.ones_like(mean) * sigma
    scale[::shape[1]] = 0.15             # durations vary less
    best = (evaluate_robust(seed, pose)[0], mean.copy())
    lam, mu = 16, 4
    gens = max(1, budget // lam)
    with Pool(workers) as pool:
        for g in range(gens):
            pop = [mean + scale * rng.standard_normal(mean.size) for _ in range(lam)]
            for p in pop:
                p[::shape[1]] = np.clip(p[::shape[1]], 0.2, 2.5)
            res = pool.map(_eval, [(p, pose, shape) for p in pop])
            order = np.argsort([-r[0] for r in res])
            elite = [pop[i] for i in order[:mu]]
            mean = np.mean(elite, axis=0)
            if res[order[0]][0] > best[0]:
                best = (res[order[0]][0], pop[order[0]].copy())
            print(json.dumps(dict(gen=g, best=round(best[0], 3), gen_best=res[order[0]][1])), flush=True)
            scale *= 0.97
    return best[1].reshape(shape).tolist(), best[0]


# --- Staged search (curriculum) -------------------------------------------
# One end-of-program score is deceptive for a get-up: every program that
# fails leaves the body on the floor with about the same score, so the
# search had no slope (24 Nov: 80 generations at upright -0.18). A child
# learns the get-up in stages, each with its own goal: onto hands and knees,
# then back onto the feet. Each stage is searched from where the best
# previous stage left the body (all STARTS), and the stages are then joined
# into one program that ``evaluate`` checks from the fall.
STAGE_HOLD_S = 0.6     # each stage ends holding its last keyframe this long


def run_frames(body, d, decoded, frames):
    """Step through (duration, targets) keyframes on the servo path."""
    m = body.m
    dt = m.opt.timestep
    for duration, want in frames:
        for _ in range(int(duration / dt)):
            target = decoded + dt / (0.2 + dt) * (want - decoded)
            decoded = decoded + np.clip(target - decoded, -body.vmax * dt, body.vmax * dt)
            d.ctrl[:] = decoded
            mujoco.mj_step(m, d)
    return decoded


def settled_states(pose):
    """The fallen body after the 1 s settle, for each of STARTS."""
    body = Body()
    out = []
    for start in STARTS:
        d = body.fallen(pose, *start)
        decoded = d.qpos[body.qadr].copy()
        decoded = run_frames(body, d, decoded, [(1.0, decoded.copy())])
        out.append((d.qpos.copy(), d.qvel.copy(), decoded.copy()))
    return out


def body_metrics(body, d):
    m = body.m

    def bz(names):
        return float(np.mean([d.xpos[mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, n)][2] for n in names]))
    touching = set()
    for c in range(d.ncon):
        for g in (d.contact[c].geom1, d.contact[c].geom2):
            if m.geom_bodyid[g]:
                touching.add(mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_BODY, m.geom_bodyid[g]))
    feet = [mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, n) for n in ("LAnkleRoll_link", "RAnkleRoll_link")]
    mid = 0.5 * (d.xpos[feet[0]][:2] + d.xpos[feet[1]][:2])
    off = float(np.linalg.norm(d.subtree_com[1][:2] - mid))
    up, z = float(d.xmat[1][8]), float(d.xpos[1][2])
    sole = float(np.mean([d.xmat[f][8] for f in feet]))     # foot z-axis . world z: 1 = flat
    return dict(z=z, up=up, com_feet=off, sole_flat=sole, fwd_z=float(d.xmat[1][6]), hip_z=bz(("LHipPitch_link", "RHipPitch_link")),
                knee_z=bz(("LKneePitch_link", "RKneePitch_link")), head_contact="HeadPitch_link" in touching,
                torso_contact="torso" in touching, angvel=float(np.linalg.norm(d.qvel[3:6])),
                squat=max(up, 0.0) * math.exp(-off / 0.04) * min(1.0, z / 0.24))


def stage_rollout(program, state, tail=False):
    """Run a stage's keyframes (plus the hold) from `state`; with `tail`,
    continue through the fixed squat, rise and stand. Returns (metrics, end)."""
    body = Body()
    m = body.m
    d = mujoco.MjData(m)
    d.qpos[:], d.qvel[:] = state[0], state[1]
    mujoco.mj_forward(m, d)
    frames = [(max(0.2, float(f[0])), body.targets(f[1:])) for f in program]
    decoded = run_frames(body, d, state[2].copy(), frames + [(STAGE_HOLD_S, frames[-1][1])])
    met = body_metrics(body, d)
    end = (d.qpos.copy(), d.qvel.copy(), decoded.copy())
    if tail:
        squat = body.targets(SQUAT)
        rise = [(RISE_S / 4, squat + (body.posture - squat) * k / 4) for k in range(1, 5)]
        run_frames(body, d, decoded, [(SQUAT_HOLD_S, squat)] + rise + [(STAND_S, body.posture)])
        met["final_up"], met["final_z"] = float(d.xmat[1][8]), float(d.xpos[1][2])
        met["success"] = met["final_up"] > 0.9 and met["final_z"] > SUCCESS_HEIGHT
    return met, end


def score_kneel(met):
    """Hands and knees: hips raised, knees down, head and chest off the
    floor, still facing the floor, and still (statically held)."""
    if met["fwd_z"] > -0.3:          # rolled onto the side or back
        return 0.1 * met["hip_z"]
    return (min(met["hip_z"] / 0.16, 1.0) + 1.0 - min(max(met["knee_z"] - 0.03, 0.0) / 0.08, 1.0)
            + 0.5 * (not met["head_contact"]) + 0.5 * (not met["torso_contact"])
            + 0.5 * math.exp(-met["angvel"]) + 0.5 * min(max(met["up"] + 0.5, 0.0), 1.0))


def score_crouch(met):
    """From hands and feet back over the feet: centre of mass over the
    feet, soles flatter, knees and chest off the floor, held still. The
    squat score alone had no slope here (it is zero until the trunk is
    upright over the feet; 24 Nov: the squat stage fell forward every time)."""
    if met["head_contact"] or met["torso_contact"] or met["up"] < -0.2:
        return 0.3 * math.exp(-met["com_feet"] / 0.1)
    return (2.0 * math.exp(-met["com_feet"] / 0.05) + max(met["sole_flat"], 0.0)
            + 0.5 * min(max(met["knee_z"] - 0.03, 0.0) / 0.05, 1.0) + min(met["hip_z"] / 0.2, 1.0)
            + 0.5 * math.exp(-met["angvel"]) + 0.5 * max(met["up"], 0.0))


def score_squat(met):
    """Onto the feet: the balanced squat (upright trunk over the feet), held
    still, and the fixed rise from it succeeding."""
    return (2.0 * met["squat"] + 0.3 * math.exp(-met["angvel"])
            + max(met.get("final_up", 0.0), 0.0) * met.get("final_z", 0.0) / 0.3 + (2.0 if met.get("success") else 0.0))


def score_roll(met):
    """Back -> front: chest down (torso x-axis pointing at the floor), lying
    low and still, ready for the front get-up."""
    prone = (1.0 - met["fwd_z"]) / 2.0
    return 2.0 * prone + 0.5 * math.exp(-met["angvel"]) + 0.5 * (prone > 0.9) * (1.0 - min(met["z"] / 0.2, 1.0))


STAGES = {"kneel": (score_kneel, False), "crouch": (score_crouch, False), "squat": (score_squat, True),
          "roll": (score_roll, False)}

# Stage seeds (rad; duration then GROUPS). Kneel: spread the hips and roll the
# thighs out (frog), prop on the arms, draw the knees forward beside the trunk.
STAGE_SEEDS = {
    "kneel": [
        [0.8, 0.3, 0.4, 0.0, -1.0, -1.0, 0.6, -0.3, 0.5, 0.0, 0.0],
        [1.0, 0.3, 0.4, 0.0, -1.0, -1.0, 0.6, -1.5, 2.1, -1.0, 0.0],
        [1.0, 0.2, 0.4, 0.0, -0.3, -1.0, 0.5, -1.5, 2.1, -1.0, 0.0],
    ],
    # Crouch: from hands and feet, turn the thighs out (hip yaw-pitch, frog)
    # so the pelvis can drop back between the feet, flex the hips fully and
    # open the shoulders to push the trunk back over the feet.
    "crouch": [
        [1.0, 0.0, 0.3, -0.4, -0.1, -0.6, 0.79, -1.5, 2.12, -1.18, -0.4],
        [1.0, -0.5, 0.3, -0.4, -0.1, -0.9, 0.5, -1.5, 2.1, -1.0, -0.2],
    ],
    # Roll (back -> front; left then right, mirrored convention): the roll
    # seed defined with the roll search below.
    "roll": None,
    "squat": [
        [1.0, 0.7, 0.2, 0.0, -0.2, -0.9, 0.3, -1.5, 2.1, -1.15, -0.2],
        [1.0, 1.2, 0.2, -1.0, -0.4, -0.5, 0.1, -1.3, 2.1, -1.1, 0.0],
    ],
}


STAGE_SEEDS["roll"] = ROLL_SEED


def _eval_stage(args):
    vec, shape, stage, states = args
    fn, tail = STAGES[stage]
    runs = [stage_rollout(vec.reshape(shape).tolist(), st, tail)[0] for st in states]
    scores = [fn(r) for r in runs]
    k = int(np.argmin(scores))
    info = {key: (round(v, 3) if isinstance(v, float) else v) for key, v in runs[k].items()}
    info["successes"] = sum(1 for r in runs if r.get("success"))
    return scores[k], info


def search_stage(seed, stage, states, budget, workers=4, sigma=0.35, seed_rng=1):
    rng = np.random.default_rng(seed_rng)
    shape = np.array(seed).shape
    mean = np.array(seed, dtype=float).ravel()
    scale = np.ones_like(mean) * sigma
    scale[::shape[1]] = 0.15
    best = (_eval_stage((mean, shape, stage, states))[0], mean.copy())
    lam, mu = 16, 4
    with Pool(workers) as pool:
        for g in range(max(1, budget // lam)):
            pop = [mean + scale * rng.standard_normal(mean.size) for _ in range(lam)]
            for p in pop:
                p[::shape[1]] = np.clip(p[::shape[1]], 0.2, 2.5)
            res = pool.map(_eval_stage, [(p, shape, stage, states) for p in pop])
            order = np.argsort([-r[0] for r in res])
            mean = np.mean([pop[i] for i in order[:mu]], axis=0)
            if res[order[0]][0] > best[0]:
                best = (res[order[0]][0], pop[order[0]].copy())
            print(json.dumps(dict(stage=stage, gen=g, best=round(best[0], 3), gen_best=res[order[0]][1])), flush=True)
            scale *= 0.97
    return best[1].reshape(shape).tolist(), best[0]


def staged(pose, budget, workers, out, stages=("kneel", "crouch", "squat"), prefix=None, then=None):
    """Each stage searched from the previous stage's end states; the joined
    program is checked from the fall with ``evaluate_robust``. ``prefix``:
    keyframes already found (earlier stages, holds included) to start from."""
    states = settled_states(pose)
    program = list(prefix or [])
    if program:
        states = [stage_rollout(program[:-1], st)[1] for st in states]
    for stage in stages:
        seg, score = search_stage(STAGE_SEEDS[stage], stage, states, budget, workers)
        # The joined program repeats the stage hold, so it reproduces the
        # states the next stage was searched from.
        seg = seg + [[STAGE_HOLD_S] + list(seg[-1][1:])]
        program += seg
        states = [stage_rollout(seg[:-1], st)[1] for st in states]
        if out:
            Path(out).parent.mkdir(parents=True, exist_ok=True)
            Path(out).write_text(json.dumps(dict(stage=stage, stage_score=round(score, 4), program=program), indent=1))
    if then:
        # A fixed continuation (e.g. the front get-up after the roll). The
        # stage keyframes may carry 20 values (asymmetric); the front
        # program's 10-value keyframes are symmetric.
        program += then
    score, info = evaluate_robust(program, pose)
    result = dict(score=round(score, 4), groups=[g for g, _ in GROUPS], program=program, **info)
    print(json.dumps({k: v for k, v in result.items() if k != "program"}), flush=True)
    if out:
        Path(out).write_text(json.dumps(result, indent=1))


# The learning test (user, 24 Nov): stand up from any fall angle and position,
# on or near the room's fixtures. Positions (x, y, head heading) in the NAO room.
BATTERY_PLACES = {
    "open": (0.0, 0.0, 0.0),
    "head_to_wall": (1.2, 0.0, 0.0),
    "feet_to_wall": (1.2, 0.0, math.pi),
    "side_to_wall": (0.0, -1.36, 0.0),
    "head_on_step": (-0.72, 0.30, math.pi / 2),
    "under_table": (0.87, 0.62, 0.0),
    "head_to_ball": (0.42, -0.645, 0.0),
    "by_table_leg": (0.57, 0.18, math.pi / 2),
}
BATTERY_PHI = [0.0, math.pi / 4, math.pi / 2, 3 * math.pi / 4, math.pi, -3 * math.pi / 4, -math.pi / 2, -math.pi / 4]


def _battery_one(args):
    place, phi, front, back = args
    x, y, yaw = BATTERY_PLACES[place]
    body = Body(open_floor=False)
    d = body.fallen("front", phi=phi, x=x, y=y, yaw=yaw)
    # The fixed controller picks its program from the gravity sense at the
    # start: chest down -> front get-up, otherwise the back program (roll).
    chest_down = float(d.xmat[1][6]) < 0.0
    program = front if chest_down else back
    state = (d.qpos.copy(), d.qvel.copy())
    _, info = evaluate(program, "front", initial=state, open_floor=False)
    return dict(place=place, phi_deg=round(math.degrees(phi)), program="front" if chest_down else "back", **info)


def battery(front, back, workers):
    jobs = [(p, phi, front, back) for p in BATTERY_PLACES for phi in BATTERY_PHI]
    with Pool(workers) as pool:
        res = pool.map(_battery_one, jobs)
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pose", choices=("back", "front"), default="back")
    ap.add_argument("--seed-name", help="SEEDS entry to start from (default: the pose's)")
    ap.add_argument("--search", type=int, default=0, help="evaluation budget (0: evaluate the seed only)")
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--program", help="JSON file with a program to evaluate instead of the seed")
    ap.add_argument("--extend", action="store_true", help="append a squat keyframe before the final stand")
    ap.add_argument("--roll", action="store_true", help="back: search the roll onto the front (asymmetric keyframes)")
    ap.add_argument("--front-program", help="front get-up program used after the roll")
    ap.add_argument("--staged", action="store_true", help="curriculum search: kneel, crouch, squat (front)")
    ap.add_argument("--stages", default="kneel,crouch,squat", help="staged: the stages to search, in order")
    ap.add_argument("--prefix", help="staged: JSON file whose program's first --prefix-frames keyframes are kept")
    ap.add_argument("--prefix-frames", type=int, default=0)
    ap.add_argument("--then", help="staged: JSON file whose program follows the searched stages (e.g. the front get-up)")
    ap.add_argument("--battery", action="store_true",
                    help="the learning test: the front/back programs from 8 places (fixtures) x 8 body rotations, room on")
    ap.add_argument("--out")
    a = ap.parse_args()
    if a.battery:
        front = json.loads((ROOT / "target/qa/nao_getup/front_getup.json").read_text())["program"]
        back = json.loads((ROOT / "target/qa/nao_getup/back_getup.json").read_text())["program"]
        res = battery(front, back, a.workers)
        for r in res:
            print(f"{r['place']:13s} phi {r['phi_deg']:+5d}  {r['program']:5s}  {'STANDS' if r['success'] else 'fails '}  up {r['upright']:+.2f} h {r['height']:.2f}")
        by = {}
        for r in res:
            by.setdefault(r["place"], []).append(r["success"])
        print("success by place:", {k: f"{sum(v)}/{len(v)}" for k, v in by.items()})
        print(f"TOTAL {sum(r['success'] for r in res)}/{len(res)}")
        if a.out:
            Path(a.out).write_text(json.dumps(res, indent=1))
        return
    if a.staged:
        prefix = json.loads(Path(a.prefix).read_text())["program"][:a.prefix_frames] if a.prefix else None
        then = json.loads(Path(a.then).read_text())["program"] if a.then else None
        staged(a.pose, a.search or 800, a.workers, a.out, tuple(a.stages.split(",")), prefix, then)
        return
    if a.roll:
        front = json.loads(Path(a.front_program).read_text())["program"]
        roll = json.loads(Path(a.program).read_text())["roll"] if a.program else ROLL_SEED
        if a.search:
            roll, _ = search_roll(roll, front, a.search, a.workers)
        runs = [evaluate_roll(roll, front, s) for s in STARTS]
        result = dict(roll=roll, front_program=front, successes=sum(1 for r in runs if r[1]["success"]),
                      starts=len(STARTS), runs=[r[1] for r in runs])
        print(json.dumps({k: v for k, v in result.items() if k not in ("roll", "front_program")}))
        if a.out:
            Path(a.out).parent.mkdir(parents=True, exist_ok=True)
            Path(a.out).write_text(json.dumps(result, indent=1))
        return
    program = json.loads(Path(a.program).read_text())["program"] if a.program else SEEDS[a.seed_name or a.pose]
    if a.extend:
        # One more keyframe before the final StandInit: a squat over the feet.
        program = program + [[1.0, 1.4, 0.2, -1.4, -0.4, -0.3, 0.0, -1.2, 2.0, -1.0, 0.0]]
    if a.search:
        program, score = search(program, a.pose, a.search, a.workers)
    score, info = evaluate_robust(program, a.pose)
    result = dict(score=round(score, 4), groups=[g for g, _ in GROUPS], program=program, **info)
    print(json.dumps({k: v for k, v in result.items() if k != "program"}))
    if a.out:
        Path(a.out).parent.mkdir(parents=True, exist_ok=True)
        Path(a.out).write_text(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
