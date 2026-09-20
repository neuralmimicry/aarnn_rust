# AARNN sensory worlds for Minecraft Java and Bedrock

Both editions have native adapters. This guide covers **Java/Fabric**; see
[Bedrock Dedicated Server installation](bedrock/README.md) (or `BEDROCK.md` in the
bundle) for the native behaviour/resource packs, server requirements and commands.
`--minecraft-edition auto|java|bedrock` controls detection or forces an edition.

The Java adapter supports isolated compatibility profiles. Each profile has its
own mod build and launcher game directory; never put jars from different rows in
the same `mods/` directory.

| Minecraft | Fabric Loader | Fabric API | Java | Build mode |
|---|---:|---:|---:|---|
| 1.21.1 | 0.17.3+ | 0.107.0+1.21.1 | 21+ | remapped official mappings |
| 26.2 | 0.19.5+ | 0.160.0+26.2 | 25+ | named Mojang classes, no remap |

AARNN supplies a profile-specific Fabric mod, a standalone companion JAR and a
saved lab world. The Rust runtime owns every neural network; neither JAR contains
a Java neural simulator.

For the installed 26.2 profile, build its artifacts with Java 25:

```sh
cd sim/minecraft
JAVA_HOME=/usr/lib/jvm/java-25-openjdk-amd64 ./gradlew --no-daemon build \
  -Pminecraft_version=26.2 -Ploader_version=0.19.5 \
  -Pfabric_version=0.160.0+26.2 -Pno_remap=true -Pjava_release=25
```

This produces `aarnn-minecraft-26.2-0.1.0.jar` and
`aarnn-minecraft-26.2-0.1.0-bridge.jar`. Copy the first jar into the installed
26.2 profile's `mods/` directory alongside its existing Fabric API jar. The
1.21.1 build produces correspondingly named `aarnn-minecraft-1.21.1-*` artifacts.

## Installation

The staged bundle is `dist/minecraft/`: mod JAR, bridge JAR, saved world,
matching Fabric API, official installer, notices and `SHA256SUMS`.
It also includes a native Bedrock saved world, offline/server add-ons and a BDS server overlay.
Recreate it after building and validating with `python3 scripts/package_minecraft.py`.


1. Install/open the selected Minecraft Java profile once using your licensed launcher.
2. Run the supplied official Fabric installer for the selected row above. For
   1.21.1 select Loader **0.17.3**; for 26.2 select Loader **0.19.5**. Alternatively
   use the installer from <https://fabricmc.net/use/installer/>. Install matching
   Fabric server software separately if you want a dedicated server; review and
   accept Mojang's terms yourself.
3. In the Minecraft launcher, create/select the Fabric installation and give each
   row a separate game directory, for example `~/Games/AARNN-Minecraft-1.21.1`
   and `~/Games/AARNN-Minecraft-26.2`. Configure Java 21 for 1.21.1 or Java 25
   for 26.2, and allocate 4 GiB.
4. Copy the matching `aarnn-minecraft-<minecraft>-0.1.0.jar` and Fabric API jar
   into that game directory's `mods/`.
   Install both on clients and the dedicated server when using multiplayer.
   The **bridge JAR is a separate executable**, not a Minecraft mod.
5. Extract **AARNN-Sensory-Lab.zip** into the game directory's `saves/`, so that
   `saves/AARNN-Sensory-Lab/level.dat` exists. Open **AARNN Sensory Lab**.
   In a different creative world with commands enabled, `/aarnn world` constructs
   the same plots in the separate `aarnn:lab` dimension.
6. Use `/aarnn visit celegans` to enter the lab. `/aarnn status` lists all profiles.
   Creative flight or spectator mode helps inspect the enlarged anatomy.

The world opens with all robots **disarmed**. It needs no Rust process to view.
World saves contain morphology, pose and cutaway state, not neural checkpoints,
credentials or a promise that a brain continued running while Minecraft closed.

## Detection and launching

From the repository root:

```sh
cargo xtask doctor --product minecraft
python3 scripts/minecraft.py doctor --minecraft-version 26.2 \
  --game-dir "$HOME/Games/AARNN-Minecraft-26.2"
```

Detection reads version manifests, Fabric mod metadata and launcher game-directory
settings. It does not read account files. Missing Java, launcher, compatible
Fabric/profile/API/mod produces a structured `unavailable` result and exit **3**.
Malformed mod metadata is reported. `NM_MINECRAFT_DIR` selects a non-default
launcher directory; `NM_MINECRAFT_JAVA_HOME` selects the Java installation.
`NM_MINECRAFT_GAME_DIR` selects the separate game directory for launcher scripts;
`NM_MINECRAFT_VERSION=26.2` selects the 26.2 compatibility profile. Without an
explicit version, detection selects the complete installed profile and reports
its game directory. The detector checks Java, Fabric Loader, Fabric API and the
AARNN mod as one profile, so a 1.21.1 jar is never accepted for 26.2 or vice versa.
Duplicate mod IDs, corrupt metadata and an occupied companion port are rejected.
Only a verified, authenticated companion readiness response permits client launch.

For the installed Java profile, the dual-brain hexapod runner bypasses launcher
profile selection and starts the detected Fabric Loader directly. It quick-plays
the configured single-player world, so no manual Fabric installation or world
selection is required:

```sh
scripts/run_minecraft_dual_aer_growth.sh --no-keep-alive --iterations 4
```

The default world is `New World`; set `AARNN_MINECRAFT_WORLD` to quick-play a
different existing world. The direct launcher writes client startup output to
`~/.minecraft/aarnn-direct-launch.log`. This path still requires the compatible
Fabric profile, Java runtime and AARNN mod/API jars reported by the detector.
The ordinary `scripts/minecraft.py launch` command keeps its launcher-GUI path
unless `--direct` is supplied.

The common launcher accepts `--sim minecraft`; its convenience wrapper is
`scripts/run_minecraft_sim.sh`. It checks prerequisites before starting brains.
Use `--no-engine` on a host serving brains/bridge without a Minecraft client.
Distributed worker nodes need the Rust runtime; they do not need Minecraft,
Fabric or a desktop. The Minecraft host uses the shared TCP-to-IPC adapter for
`--nodes N`. One lab robot per selected profile is supported, six maximum.

## Connecting the Rust brains

The companion accepts bounded HTTP requests on **127.0.0.1:62620**, validates a
shared token and routes each selected profile to its own loopback Rust TCP
endpoint. It exchanges the existing **AER1** protocol with the same
`nn_tcp_server`/TCP-to-IPC adapter used by Unity and Unreal. It does not call
the cluster-wide `web_ui /api/aer/infer` route: that route currently rejects
authenticated sessions. No server authorisation gate is changed by this mod.

When the unified launcher starts the Java launcher, it creates a private random
session token automatically if `AARNN_MINECRAFT_TOKEN` is unset, and passes the
same process-environment token to the companion and launched client. Do not put
the secret in a command, world save or Git file. For an already-running client,
or when starting the companion and client separately, create the token in the
shared private environment first:

```sh
export AARNN_MINECRAFT_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
./scripts/run_minecraft_sim.sh --robots 'celegans=1'
```

For an existing separately opened Minecraft client, restart it from this
environment so its integrated server inherits the token. Both processes must
receive the same secret. With a dedicated server, set the variable in that
server's private service environment instead.

The first Minecraft server start creates `config/aarnn.json` in its game
directory. Review it, set `allowLegacySandboxInference` to `true`, and set the
selected binding's `networkId` to its profile key, for example `celegans`.
Leave `nodeId` empty/null. Preserve the generated content digest and dimensions.
`firstStep` is the initial sandbox sequence; it is not a wall-clock timestamp.
Then run `/aarnn connect celegans`. `/aarnn stop` disarms all lab robots locally;
`/aarnn disconnect celegans` disarms only that robot.

If `/aarnn status` reports `content=review-required`, the saved lab was created
with an earlier catalogue digest. Inspect the updated catalogue, then run
`/aarnn review` once; this updates only the saved lab content marker and keeps
all entities disarmed. Run `/aarnn connect celegans` afterwards. The mod refuses
to connect a mismatched saved lab until that explicit review is completed.

Confirm the live exchange from Minecraft chat with `/aarnn status`. Each robot
reports `frames in/out`, the last input/output step, the number of spikes in the
last neural reply and the mean sensory input. A working exchange has increasing
input and output counts, usually equal counts, and advancing step numbers; a
zero spike count is valid and means that the last reply contained no output
spikes. The first request is shown as `pending: first neural frame`. After the
first reply, `active: legacy sandbox (frame pending)` is normal because the
adapter keeps one bounded request in flight; the counts and advancing steps
confirm the exchange. If input increases while output stays behind for the
response budget, the route has faulted and the robot disarms. `/aarnn senses
<profile>` only samples the Minecraft world locally; it does not prove that a
frame reached Rust.

To run all six local networks, use:

```sh
./scripts/run_minecraft_sim.sh --no-engine \
  --robots 'celegans=1,drosophila_banc=1,drosophila_fafb=1,hexapod=1,nao=1,zebrafish=1'
```

The launcher prints the TCP allocation. The companion also prints each profile's
route. For an independently managed Rust setup, run the companion directly:

```sh
java -jar sim/minecraft/build/libs/aarnn-minecraft-<minecraft>-0.1.0-bridge.jar \
  --profiles celegans --base-port 7890 --port 62620
```

The profile order must match the Rust TCP port order. Restart/reconnect is
deliberate: stop the robot, inspect the backend after an ambiguous failure, and
use a sequence beyond the last submitted sequence (or restart the isolated
experiment and its bridge/brain together). The companion rejects duplicate
sequences and faults a route after an ambiguous backend response. It never
automatically resubmits an input. The loopback bridge is not a public service.

## Worlds and robot equivalents

| Profile | Inputs / outputs | World and visible anatomy |
|---|---:|---|
| `celegans` | 24 / 96 | Agar, bacterial colonies, chemical/heat fields, touch ridges; cuticle, 95 body-wall muscles, pharynx, intestine and schematic nerve ring |
| `drosophila_banc` | 418 / 48 | Orchard, fruit/bruises, stems/leaves/veins, optic-flow panels; compound-eye facets, antennae, proboscis, six legs, wings/veins and halteres |
| `drosophila_fafb` | 418 / 48 | Same adult fly anatomy; separate imported network/output identities and separate plot |
| `hexapod` | 34 / 18 | Steps, gravel, slalom, traction mat; servo housings, joint links, feet, sonar and camera |
| `nao` | 250 / 40 | Reachable objects, table, doorway, steps and target ball; humanoid panels, cameras, hands and foot pads |
| `zebrafish` | 32 / 32 | Freshwater tank, plants, pebbles, refuge, prey and current proxy; submerged larval-inspired body, fins, eyes, myomeres and lateral-line landmarks |

`/aarnn visit <profile>` moves the player between plots.
`/aarnn anatomy <profile>` toggles the cutaway.
`/aarnn senses <profile>` samples and reports the sensory mean without a brain.
Drop apples, sugar, sweet berries or honey bottles to add chemical sources;
placed solid Minecraft blocks occlude the visual/proximity rays. Commands require
operator level 2 (or an authorised single-player cheats-enabled world).

Five shared habitats contain 602 authored objects. Six plots contain the orchard
twice, preserving BANC/FAFB isolation. Every catalogue primitive and robot part
comes from the same generated content used by Webots, Unity, Unreal and WebGL.
Spheres/ellipsoids, cylinders and boxes are drawn as detailed meshes, not reduced
to single Minecraft blocks. The solid floor supports players; fine habitat
scenery is visual and supplies analytic sensor/proximity geometry. It is not a
set of voxel collision blocks for player navigation.

The zebrafish tank contains actual Minecraft water, ten blocks deep, enclosed by
glass. The fish and its dorsal extent are submerged. Water renders through
Minecraft's native fluid system; it does not occlude the reference sensory rays.

Coordinates convert `(X,Y,Z)` to Minecraft `(X,Z,-Y)`; each habitat half extent
is **16 blocks**. Robot size is its catalogue body-length fraction times that
extent. This is educational magnification, not a claim that one block has the
same physical scale for worms, robots and fish. Colours, shape resolution and
diffuse shading match the WebGL reference; Minecraft's rendering differs from
native PBR engines.

## Rebuild and verify

```sh
python3 scripts/regenerate_simulator_assets.py
cargo xtask qa run --suite simulator-content
cargo xtask qa run --suite simulator-minecraft
cargo xtask qa run --suite simulator-minecraft-world
cargo xtask qa run --suite simulator-minecraft-visual
cargo build --locked --release --features ui,robot_io --example nn_tcp_server
cargo xtask qa run --suite simulator-minecraft-neural
cargo xtask qa run --suite simulator-minecraft-bedrock
cargo xtask qa run --suite simulator-minecraft-bedrock-native
```

The contract lane needs Java 21, Node, Python and network access for the first
Gradle/Fabric build; it does not require an installed licensed launcher. Built
JARs are under `sim/minecraft/build/libs/`. Native lanes are explicit, with
missing capabilities or timeouts reported as failures. Evidence bundles live
under `target/qa/minecraft/`. The neural lane requires all six local snapshots,
starts isolated Rust processes, validates dimensions before handshaking and
records snapshot/output hashes. It does not modify the snapshots.
For a focused real-network rerun, set `NM_MINECRAFT_TEST_PROFILES` to a comma-separated
list of canonical profile IDs; the result explicitly lists the selected profiles.

The development-only visual probe opens a copy of the exported world, checks
the lab dimension and captures each habitat and cutaway using Minecraft's own
framebuffer. GameTest/visual test code is excluded from release JARs. Rebuild a
world with the `simulator-minecraft-world` lane. The packer requires successful
GameTest assertions, a clean process exit, and matching report/source/save hashes.
It crops unrelated test terrain and exports only the lab and arrival platform.

## Fidelity and current limits

Sensory fields, conservative box rays, retinal ON/OFF history and movement
decoding follow the WebGL reference. Tests compare all sensory channels across
five poses per profile, movement and both model views. Native block/food input
is an explicitly Minecraft-specific extension. The body stays at its declared
habitat height; movement is a bounded kinematic preview, without articulated
rigid-body locomotion, aerodynamics or fluid dynamics. Fly readouts still lack a
validated muscle-to-joint map; aggregate activity is a labelled preview.

The 54 chunks covering the six plots stay loaded once the lab is requested.
World/visit commands wait for saved entity regions before creating missing bodies.
One successful response advances the visible movement once. At most one request
per robot is outstanding; stalled inference freezes its pose and then disarms.
Stop/unload/restart clears outputs. Handshakes have a 60-second startup limit;
response budgets are 60 seconds for flies, 10 seconds for fish and 2.5 seconds
for the smaller profiles. These are wall-clock budgets, not biological timesteps.
The visible body waits without moving while inference is pending. The companion thresholds inputs above 0.5
into AER1 events, negotiates 1 ms Rust steps, validates output addresses and
preserves the legacy reply timestamp. This is not the future AARNN-AER/1
peripheral protocol and does not certify production capture-clock mapping,
EffectId fencing, durable admission or committed-output semantics.

Java scene meshes use an opaque, depth-writing cutout layer. The white texture
only supplies the entity render pipeline; catalogue colours remain vertex
colours. This keeps clouds and terrain behind the hexapod while preserving
double-sided schematic geometry.

Content/transducer parity does not establish biological adequacy or identical
physics/trajectories across simulators. Prior Unity toolchain absence, native
fly/fish mapping differences and native physics warnings remain tracked in
`../content/README.md`. No production phase gate is promoted.

## NAO conversations and autonomous participants

See [the NAO guide](../nao/README.md) for a separately generated 282/49 social model,
local chat/voice window and isolated Java/Bedrock installation. Ordinary players
can use `/nao say <message>` on Java or `/aarnn:nao "message"` on Bedrock when NAO
is connected nearby. The server derives their identity/range; operator privileges
are only needed to configure/arm the brain. Neural encounter output can ask a
nearby player/NPC a question. All six profiles observe nearby bodies through
existing senses and expose their neural-driven movements to other participants.
The social vocabulary is engineered and finite; general understanding is not claimed.
