# AARNN Simulation Bridges — Webots · Unreal Engine · Unity · WebGL · Minecraft

This directory holds the **Unreal Engine** (`sim/unreal/`) and **Unity** (`sim/unity/`)
integrations for the AARNN neuromorphic networks, mirroring the existing **Webots**
integration under `../webots_world/`.

All five environments use the shared Rust neural runtime and robot profiles.
[Minecraft installation and validation](minecraft/README.md) covers its Fabric
mod, Java saved world, native Bedrock packs, token-protected companion JAR and
edition/capability detection. Java 1.21.1 and Bedrock Script API 2.9.0 are separate
targets; use `--minecraft-edition auto|java|bedrock` to detect or force the adapter.
[Shared simulator content](content/README.md) describes the species habitats,
biological landmarks, regeneration commands, evaluation and remaining parity gaps.
Choose the simulator and robot population through the common launcher.

---

## 1. How a robot connects to a brain

Every robot instance drives one AARNN "brain" process through a sense → think → act loop:

```
robot sensors ──▶ [encode] ──▶ AARNN brain ──▶ [decode] ──▶ robot motors
   (floats 0..1)     AER1/raw     (Runner.step)   AER1/raw     (floats 0..1)
```

| Engine  | Transport                     | Brain process                                        |
|---------|-------------------------------|------------------------------------------------------|
| Webots  | Unix domain socket (local)    | `examples/nn_uds_server.rs` via `run_multi_robot_webots.sh` |
| Unreal  | TCP (`FNmAerClient`)          | standalone `nn_tcp_server`, or a TCP-to-IPC bridge into distributed nodes |
| Unity   | TCP (`NmAerClient`)           | standalone `nn_tcp_server`, or a TCP-to-IPC bridge into distributed nodes |
| WebGL | Authenticated browser HTTP gateway | `web_ui` → distributed AER admission |

The TCP and UDS servers share the same `Runner`, the same network/config JSON files, and
the same **AER1 wire protocol** (`src/aer.rs`). Shared transport preserves the neural
runtime boundary; equivalent behaviour also requires calibrated sensor/motor maps,
physics and timing. Several legacy native mappings still differ; see the content guide.

### Wire protocol (TCP)

Each message is length-prefixed: `[u32 LE payload_len][payload]`. The payload is one of:

- **JSON handshake** (first frame): `{"s_names":[...],"o_names":[...],"sensory":N,"output":M}`.
  The server replies `{"expected_s":N,"expected_o":M}`.
- **AER1 spike packet** (preferred): `"AER1"` + `u64 LE base_ts_us` + varint
  `(delta_ts, addr, value)` triplets. Sensory addresses are `sensory_base + index`
  (default base `4096`); output addresses are `output_base + index` (default base `16384`).
  Output spikes carry `value == 1` (binary) — any non-zero value is a spike.
- **Raw float packet** (fallback): `[f32 LE t_ms][S × f32 LE sensors]` → `[O × f32 LE outputs]`.

---

## 2. Launch matrix

The unified launcher is `../scripts/run_sim.sh`. Select the backend with `--sim` and the
robot population with `--robots`:

```bash
# Webots only — one C. elegans
./scripts/run_sim.sh --sim webots --robots "celegans=1"

# Unreal Engine — three C. elegans brains on TCP 7890,7891,7892.
# This ALSO launches the Unreal project; robots spawn and connect automatically.
./scripts/run_sim.sh --sim unreal --robots "celegans=3"
#   convenience wrapper:  ./scripts/run_unreal_sim.sh --robots "celegans=3"
#   distributed shards:   ./scripts/run_unreal_sim.sh --robots "celegans=1" --nodes 3
#   brains only (launch Unreal yourself):  ./scripts/run_sim.sh --sim unreal --no-engine ...
#   custom engine path:  ./scripts/run_sim.sh --sim unreal --engine /opt/UnrealEngine/Engine ...

# Unity — two zebrafish + one hexapod
./scripts/run_sim.sh --sim unity --robots "zebrafish=2,hexapod=1"
#   convenience wrapper:  ./scripts/run_unity_sim.sh --robots "zebrafish=2,hexapod=1"
#   distributed shards:   ./scripts/run_unity_sim.sh --robots "zebrafish=1" --node 3

# Combination — Webots AND TCP servers at once
./scripts/run_sim.sh --sim all --robots "celegans=1,hexapod=1,nao=1"
```

Useful flags: `--tcp-host <host>` (default `127.0.0.1`; use `0.0.0.0` for a remote engine),
`--tcp-base-port <n>` (default `7890`), `--no-build` (reuse an existing binary). Run
`./scripts/run_sim.sh --help` for the full list.

`--node <n>` (also accepted as `--nodes <n>`) sets the total worker-node count and
requires at least one worker per configured brain. Webots passes the count directly
to its cluster launcher. Unreal and Unity start the same distributed runtime and
put one bounded TCP bridge in front of each brain IPC socket, so their existing
AER1 and raw-float clients continue to use the same host/port assignments. The
bridge uses the AER timestamp plus the negotiated frame duration when it creates
an AER output timestamp; it never uses packet arrival time.

With `--sim all --node <n>`, Webots and Unreal use separate cluster IPC namespaces
because each IPC endpoint has one active peer. This keeps both simulations
independent while allowing both runtimes to use the requested worker count.

### Port allocation

For `unreal`/`unity`/`all`, one TCP brain endpoint is started per robot instance. Ports are
assigned **in spec order**, `base_port + index`:

```
celegans_0   127.0.0.1:7890   sensory=24  output=96
hexapod_0    127.0.0.1:7891   sensory=34  output=18
nao_0        127.0.0.1:7892   sensory=250 output=40
```

The launcher prints this exact `brainId → host:port → sensory/output` table on startup —
copy those ports into your engine's brain connectors (below).

---

## 3. Robot types

| Type              | Aliases                              | Sensory | Output | Network file                    |
|-------------------|--------------------------------------|--------:|-------:|---------------------------------|
| `celegans`        | worm, c_elegans                      |      24 |     96 | `network_celegans.json`         |
| `drosophila_banc` | drosophila, fly, fruitfly, banc      |     418 |     48 | `network_drosophila_banc.json`  |
| `drosophila_fafb` | fafb                                 |     418 |     48 | `network_drosophila_fafb.json`  |
| `hexapod`         | hex, freenove, six_legged            |      34 |     18 | `network_hexapod.json`          |
| `nao`             | naos                                 |     250 |     40 | `network_nao.json`              |
| `zebrafish`       | danio, danio_rerio, fish, zfish, zf  |      32 |     32 | `network_zebrafish.json`        |

The sensory/output channel counts **must** match on both ends. In Unreal they are the lengths of
the sensor/actuator name arrays passed to `SendHandshake`; in Unity they are set on the
`NmBrainConnector` and the robot component. The server adapts its network to the handshake
sizes, but keeping them aligned with the table above avoids silent truncation.

For the C. elegans import, the 24 sensory values are external input channels and the 96
outputs are muscle readout channels. They are connected to the 302 imported biological
neurons; they are not additional neurons and are excluded from biological growth counts.

---

## 4. Unreal Engine project (`sim/unreal/`)

`sim/unreal/` is a **complete, openable UE 5.8 project** (`NeuralMimicrySim.uproject`) with
two modules:

- `NeuralMimicrySim` — thin primary game module (project entry point only).
- `NmAerBridge` (`Source/NmAerBridge/`) — the reusable bridge, self-contained so the folder
  can be dropped verbatim into any other project's `Source/`:
  - `FNmAerClient` — TCP client speaking the AER1 protocol (`NmAerClient.h/.cpp`).
  - `UNmRobotBase` — `UActorComponent` base that owns a client and runs the sense/act loop
    each tick (`NmRobotBase.h/.cpp`).
  - `ANmSimManager` — level actor holding host/base-port and per-robot registration
    (`NmSimManager.h/.cpp`).
  - `Robots/` — illustrative per-phenotype components (`UNmCelegansComponent`,
    `UNmDrosophilaComponent`, `UNmHexapodComponent`, `UNmNaoComponent`,
    `UNmZebrafishComponent`) mapping engine bodies (static meshes + physics constraints +
    scene-capture "eyes") to each brain's sensory/motor channels.

**Build (verified against UE 5.8):**

```bash
ENGINE=/path/to/UnrealEngine/Engine   # this machine: /home/pbisaacs/Developer/Engine
"$ENGINE/Build/BatchFiles/Linux/Build.sh" \
    NeuralMimicrySimEditor Linux Development \
    -Project="$(pwd)/sim/unreal/NeuralMimicrySim.uproject" -WaitMutex
```

**Run (automatic, recommended):** `./scripts/run_sim.sh --sim unreal --robots "<spec>"`
starts the brain servers and launches the project as a standalone game. `ANmSimGameMode`
(forced via the launch URL) reads three environment variables the launcher exports —
`NM_UE_ROBOTS` (the spec), `NM_AARNN_HOST`, `NM_AARNN_BASE_PORT` — and on BeginPlay spawns one
robot actor per brain, wiring each to its port (base+index, same order as the launcher) so
they connect with no manual setup. Engine build/load evidence is recorded separately
from closed-loop neural validation. Override the engine
location with `--engine <UE>/Engine` (or `UE_ENGINE`), the project with `--uproject`, the boot
map with `--map`, or start brains only with `--no-engine`.

**Run (manual, in-editor):** open `NeuralMimicrySim.uproject` and press Play — the same
`ANmSimGameMode` (set as the project's default game mode in `Config/DefaultEngine.ini`) spawns
and connects the robots from those same env vars. Or place robot actors by hand and set each
component's `BrainId` / `TcpHost` / `TcpPort` in the Details panel. No socket plugin is needed
(plain `FSocket`; the `ProceduralMeshComponent` engine plugin is enabled in the uproject).

**Reuse in your own project:** copy just `Source/NmAerBridge/` into your project's `Source/`,
add `"NmAerBridge"` to your game module's dependencies, and regenerate project files.

## 5. Unity project (`sim/unity/`)

Assembly `NeuralMimicry` (`Assets/NeuralMimicry/`):

- `NmAerClient` — TCP client speaking the AER1 protocol (`Runtime/NmAerClient.cs`).
- `NmBrainConnector` — `ScriptableObject` holding one brain's host/port/threshold; creates
  clients and self-registers by `brainId` (`Runtime/NmBrainConnector.cs`).
- `NmRobotBase` — `MonoBehaviour` base that drives the client every `FixedUpdate`
  (`Runtime/NmRobotBase.cs`).
- `NmSimulationManager` — scene singleton tracking robots with a debug overlay
  (`Runtime/NmSimulationManager.cs`).
- `Runtime/Robots/` — illustrative per-phenotype components (`NmCelegansRobot`,
  `NmDrosophilaRobot`, `NmHexapodRobot`, `NmNaoRobot`, `NmZebrafishRobot`).

**Setup:** copy `Assets/NeuralMimicry` into your project. Create one `NmBrainConnector`
asset per robot (Assets ▸ Create ▸ NeuralMimicry ▸ Brain Connector), set its `brainId`,
`tcpHost`, and `tcpPort` to a row from the launcher output, then reference it from the
robot component. See `../scripts/run_unity_sim.sh` for the full checklist.

---

## 6. Notes

- The engines are **clients**; AARNN is the **server**. `--sim unreal` launches Unreal for
  you; for Unity, start `run_sim.sh` first and then press Play (clients reconnect
  automatically if started early).
- `ANmSimGameMode` builds the shared agar, orchard, terrain, room and freshwater
  habitats, then frames the scene. `NmSharedContent.cpp` consumes the generated
  catalogue; `GetTypeCfg` retains the existing native physical scale (worm ×1.8,
  fly ×15, fish ×12, hexapod ×3, NAO ×2.5). Scale affects physical bodies as well as
  presentation and is not a biological measurement.
- Shared visual skins add no collision bodies, masses or neural channels. Existing
  joint solvers, buoyancy/drag and drive gains remain engine-specific approximations.
  The content tests do not establish contact stability or identical trajectories.
- For an Unreal framebuffer capture after camera framing, set
  `NM_SIM_CONTENT_CAPTURE=/absolute/path/image.png` before launching. This captures
  the application's own render surface during the existing diagnostic callback.
- The Drosophila also uses a **world-space limb layout**: its 6 legs radiate from the thorax
  underside (4 segments each), the head and wings are kinematic and follow the body, and every
  leg joint is wired at its midpoint — otherwise the 24 tiny leg segments collapse inside the
  thorax and explode.
- Robots **auto-reconnect**: `UNmRobotBase` retries the brain connection ~once a second until
  the server is up, so a species whose brain loads a large network (Drosophila's is big and the
  server binds its port only after loading) still links up instead of staying idle.
- Current evidence: generated-content and browser checks pass, and native build/load
  probes are tracked in the content ExecPlan. Native warnings and channel-map gaps
  remain open; stable cross-engine neural behaviour is not certified.
- AER1 is preferred for large networks (Drosophila, NAO); the raw-float path is available
  for debugging (`useAerEncoding = false` in Unity, `bUseAER = false` in Unreal).
- To drive an engine on another machine, launch with `--tcp-host 0.0.0.0` and point the
  engine's host field at the server's reachable IP.
# Browser WebGL simulator

The browser simulator is served by the same authenticated `web_ui` gateway as
the control surface. It renders a WebGL robot preview for all six shared robot
profiles and sends bounded sensory frames to the selected brain through
`POST /api/aer/infer`. Returned output spike indices drive the browser actuator
adapter. The neural runtime remains authoritative; the browser body and transducers
are explicitly bounded kinematic/spatial proxies.

Start it with:

```bash
scripts/run_webgl_sim.sh --robots "celegans=1" --nodes 3
```

Open the printed `/sim/webgl` URL and select the matching brain ID, such as
`celegans_0`. `--nodes` has the same meaning as the other simulator launchers:
it starts the shared cluster with that many workers, while each configured
brain retains an IPC owner. Use `--web-port` to select the browser port and
`--orchestrator-port` to pin the cluster gRPC port. `--no-build` reuses the
release `aarnn_rust` and `web_ui` binaries.

The page reports WebGL as unavailable when the browser cannot create a WebGL
context and does not claim global keyboard, pointer, camera, microphone, or
USB access. Those capabilities remain explicit product adapters governed by
the web workstation requirements.

For focused player text/voice interaction with NAO and capability-limited autonomous
encounters, use `scripts/run_sim.sh --sim webgl --nao-social` (or select a native
engine). The [NAO guide](nao/README.md) covers its fresh model, scoped player
invitations, Java/Bedrock commands, installation and validation limits.
