# LEC and Corvus corone embodiments

The repository has **28 physical source profiles and 224 visible target rows**.
All 28 run bounded native mechanics/NPC checks in Rust MuJoCo, Rust Webots and
Unreal Chaos. Golf, UGOT mecanum and crow also run actual AARNN reference loops.
Unity has a native factory and generated Rust ABI bindings but no installed
Editor evidence. All 28 have offline USD articulations; an Isaac runtime is
still required. [Current evidence](../../../qa/fixtures/lec-crow/CURRENT.md),
[living plan](../../../docs/execplans/lec-crow-expansion.md) and
[source licences](NOTICE.md) distinguish these results from full acceptance.
The latest Chaos passive-energy checks fail 20 profiles; those ports require
contact/constraint calibration before physical task experiments. A later mixed
Chaos run also records a crow reference-channel timeout while both UGOT brains
continue. See the [delivery record](../../../docs/lec-crow-delivery.md).

**Full robot/task acceptance is incomplete.** NPC patterns are programmed motor
intents, not learned skills, a verified gait or biological cognition. Crow is
an original reduced Corvus corone model with a proposed sensory/motor mapping.
Flight, walking, perching, feeding and commercial process tasks remain open.
The six existing network IDs/files and backend routes remain preserved; BANC
and FAFB are separate. MuJoCo C. elegans now supports its preserved network
through a versioned engineering projection and named muscle-response v2. Native
Rust also implements hexapod 34/18, distinct BANC/FAFB 418/48, NAO 250/40 and
zebrafish 32/32 adapters. Biological calibration and learned task transfer remain open.
All profiles can run programmed NPC mechanics without neural transfer.

One shared Rust crate owns controller arbitration, causal motor filters, bounded
asynchronous AER, proposed sensory projections and panel forces. One physical
record produces rigid clusters, named IO, native assets and engine maps. Engine
adapters own only their bodies, measurements and native force application.

```bash
export AARNN_MUJOCO_DIR=/home/pbisaacs/Developer/mujoco-3.17.0
export LD_LIBRARY_PATH="$AARNN_MUJOCO_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# Persistent native Rust viewer and existing brain/orchestrator workflow.
scripts/run_sim.sh --sim mujoco --robots celegans=1
scripts/run_sim.sh --sim mujoco --robots celegans=1 --nodes 3
scripts/run_sim.sh --sim mujoco --robots hexapod=1 --nodes 3
scripts/run_sim.sh --sim mujoco --robots nao=1
scripts/run_sim.sh --sim mujoco --robots zebrafish=1
scripts/run_sim.sh --sim mujoco --robots drosophila_banc=1
scripts/run_sim.sh --sim mujoco --robots drosophila_fafb=1
# F switches habitat/body framing; --view body starts close to the worm.
scripts/run_sim.sh --sim mujoco --robots celegans=1 --view body
# Close/Esc stops its private cluster; Space pauses and R resets/disconnects.

# Root launcher: unbound bodies use the programmed NPC controller.
./run_sim.sh --sim mujoco --robots galbot_g1_golf=1,ugot_mecanum=1,crow_corvus_corone=1 \
  --control auto --headless --steps 100 --output target/qa/npc-fleet.json

# Both entry points select the same native Webots controller.
./run_webot.sh --robots ugot_mecanum=1 --control npc --headless --steps 100 \
  --output target/qa/new-empty-webots-run
scripts/run_multi_robot_webots.sh --robots hexapod=1,crow_corvus_corone=1 \
  --control auto --headless --steps 100 --output target/qa/new-empty-mixed-run

# Native Chaos factory; UE_ENGINE defaults to the installed Engine directory.
scripts/run_unreal_sim.sh --robots crow_corvus_corone=1,ugot_mecanum=2 \
  --control auto --headless --steps 100 --output target/qa/new-empty-chaos-run
```

`--control auto` uses NPC while no verified ready brain owns that body. At a
physics boundary it clears the former drive before selecting AARNN, and clears
neural drive before NPC fallback after disconnect. A connected but silent brain
retains control with no NPC drive. Stop/reset inhibition overrides both.
`npc` and `silent` require an unbound body; `aarnn` requires an explicit binding
and excludes NPC startup/fallback. Use `silent` for free-fall/silence diagnostics
and `aarnn` for neural evaluation. Diagnostic efforts are separately labelled
and cannot run with a neural binding.

A per-instance `--reference-bindings MAP.json` pins `brain_id`, positive
`brain_generation`, `model_digest`, `io_digest`, loopback host and port. Each
body requires a distinct brain and endpoint. The strict reference listener is
`examples/nn_tcp_server.rs`, with `NM_AER_REFERENCE_CONTRACT` selecting the exact
ordered IO names. QA creates temporary untrained reference brains; it never
resizes existing networks. AER1 reference exchange provides no production
AARNN-AER/1 commit, EffectId or fence proof; FPAA is another separate protocol.
The managed C. elegans MuJoCo route supplies a strict per-instance contract to
the existing TCP/UDS bridge and loads the preserved snapshot instead. Explicit
NPC/silent/reference bindings opt out. See [simulation assumptions](celegans-simulation-v2.json)
and the [reproduction/evidence record](../../../docs/lec-crow-delivery.md).

Managed MuJoCo selects legacy snapshots independently; a new unbound robot in
the same population remains NPC. Matching explicit `--network-map` entries can
select brains for new profiles. Strict preflight checks ordered names/imported
IDs and config shape before launching workers. The bounded startup budget now
reaches the inner launcher/handshake; 5s data faults and 100ms source-age expiry
remain unchanged.
Startup preserves raw snapshot state when only I/O encoding differs. In a
one-layer model, unspecified routing and explicit H0 both resolve to H0; this
case avoids rebuilding the biological network merely to change metadata.
Actual structural routing/dimension changes retain the existing migration.

See [native legacy v1 assumptions](legacy-native-v1.json) for sensor frames,
servo/readout maps, camera event cells, force models and limits. Fly antagonistic
readouts and fish fluid/pressure sensors change experiments relative to the old
illustrative bridges and need task calibration. No oscillator, twitch or generic
spike lift is injected. Rust owns these adapters; `--reference-python` rejects
the unsupported native legacy sensory path explicitly.

Native habitats reuse the existing shared catalogue for all 28 profiles. Worms
have tapered capsule cuticle with schematic annuli/internal landmarks, nearby
range/contact food proxies, ridges, refuge and light/heat cues. The 0.34 m body
and inherited mass are engineering scale, not measured worm anatomy. The exact
24/96 neural contract is preserved. Shared fly/fish anatomy and crow feather
details are massless visual geometry. Local body checkpoint digests change;
incompatible snapshots are rejected, while neural snapshots stay untouched.

The habitat lane audits all 28 profiles and retains 224 target rows, native
renders, actual browser/service evidence and representative Webots/Chaos NPC
runs. This is mechanics/appearance evidence. Native Rust fish water has assumed
displacement buoyancy and quadratic drag; fluid calibration remains open. Other
native factories have no equivalent fluid adapter; Unreal omits translucent water. Unity
and Isaac runtime absence remains blocked; Minecraft uses voxel bounds.

MuJoCo uses one owner per independent world. Webots/Chaos support mixed contact
scenes with separate body/controller/brain identities. Unity requires
`NM_UNITY_EDITOR` pointing to a supported 2022.3 LTS+ Editor; its launcher stages
links to the canonical Assets and Rust library in ignored output. Headless and
interactive factories are separate from the old six-profile preview routes.
Use an explicit `--control` to select physical ports of those six. Isaac needs
its own supported runtime and Python, separate from ordinary MuJoCo/Python.

For authoritative WebGL or Minecraft viewing:

```bash
scripts/run_webgl_sim.sh --physics-service --robots crow_corvus_corone=1 \
  --control auto --token-file /tmp/aarnn-physics.token --port 8092
# Equivalent native service for Java/BDS observers:
# scripts/run_minecraft_sim.sh --physics-service ...
python3 scripts/prepare_minecraft_physics.py --port 8092 \
  --token-file /tmp/aarnn-physics.token --origin 0,64,0 \
  --output target/qa/new-private-game-config
```

The token stays in a private 0600 file. Browser viewers use the same-origin
private service page; they produce no sensory/AER data. Java uses
`AARNN_PHYSICS_CONFIG` and `AARNN_PHYSICS_TOKEN`; `/aarnn physics attach|stop`
controls its observer. Bedrock uses the prepared independent server-admin
variables/secret and `/scriptevent aarnn:physics_attach` or
`aarnn:physics_stop`. Merge those fields with existing private server settings;
never replace other grants/secrets or copy secrets into a world/resource pack.

Minecraft renders inert bounding-box proxies at native body poses, one metre
per block. Its current collision tier mirrors at most 32 full blocks above a
matching flat native floor. Partial blocks, doors/perches, nearby players,
unloaded chunks and overflow stop/pause the experiment. Java and the
Bedrock-independent client have real native HTTP evidence; native/rendered game
functional acceptance is still open. The six existing illustrative game modes
remain separate. See [game guidance](../../minecraft/PHYSICS.md).

Recreate/check generated sources in the two distinct conversion environments:

```bash
# MuJoCo/reference: Python 3.14, pinned installed dependency set.
python3.14 -m venv target/robot-mujoco-venv
target/robot-mujoco-venv/bin/python -m pip install -r sim/mujoco/reference-requirements.lock
# Offline USD only: Python 3.12; this is not Isaac.
python3.12 -m venv target/robot-usd-venv
target/robot-usd-venv/bin/python -m pip install -r sim/isaacsim/usd-conversion-requirements.lock
target/robot-mujoco-venv/bin/python scripts/embodiment.py --check
target/robot-mujoco-venv/bin/python scripts/embodiment_graph.py --check
target/robot-mujoco-venv/bin/python scripts/embodiment_webots.py --check
target/robot-usd-venv/bin/python scripts/embodiment_usd.py --check
python3 scripts/generate_embodiment_ffi.py --check
```

Exact QA commands (each retains a unique bundle and all 224 rows):

```bash
cargo xtask qa run --suite lec-crow
cargo xtask qa run --suite lec-crow-habitats
cargo xtask qa run --suite lec-crow-legacy-neural
cargo xtask qa run --suite lec-crow-mujoco-population
cargo xtask qa run --suite lec-crow-shared
cargo xtask qa run --suite lec-crow-launchers
cargo xtask qa run --suite lec-crow-npc
cargo xtask qa run --suite lec-crow-rust-service
cargo xtask qa run --suite lec-crow-rust-neural
cargo xtask qa run --suite lec-crow-webots-neural
cargo xtask qa run --suite lec-crow-unreal-neural
cargo xtask qa run --suite lec-crow-fleet
cargo xtask qa run --suite lec-crow-unreal-passive
cargo xtask qa run --suite lec-crow-unreal-lifecycle
cargo xtask qa run --suite lec-crow-unreal-render
cargo xtask qa run --suite lec-crow-capabilities
cargo xtask qa run --suite lec-crow-minecraft-service
cargo xtask qa run --suite lec-crow-usd
python3 scripts/qa/consolidate_embodiments.py
```

For a targeted legacy repair, set `NM_LEGACY_NEURAL_PROFILES=zebrafish` on the
legacy-neural QA command. It still runs native/contract/startup and mixed
population checks. Consolidation retains the latest separate result for each
other profile, including unresolved failures. The population probe uses the
launcher's 600s startup budget; `--startup-timeout` makes other budgets explicit.
The focused `mujoco-population` suite verifies the current launcher with three
preserved brains plus an NPC. Initial load and current placement precede Start;
bounded retries repeat only the existing idempotent Start operation for missing
acknowledgements. Reset/repeat and active source-time expiry remain unchanged.

Calibration limits include assumed surrogate mass/torque/noise; UGOT roller
contact; tiny/oblong Golf inertias and compliant gripper approximations; game
engine convex collision tessellation and solver response; crow self-collision
suppression, quasi-steady panels and proposed 25 W/20 kJ physiology. Native
checkpoint/restore disconnects and inhibits stale output. Local embodiment
snapshots exclude a remote brain/in-flight AER and do not prove deterministic
closed-loop replay. Unity/Chaos complete lifecycle/parity/rendering and all
trained/held-out tasks remain open. Chaos body reset is tested separately from
checkpoint/replay: `--reset-at 50 --steps 100` disconnects the reference stream,
advances the local epoch and leaves both controllers inhibited. No physical
device is connected or actuated.
