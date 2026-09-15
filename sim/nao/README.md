# NAO conversation and autonomous interaction

NAO can receive focused text or a reviewed speech transcript, answer through its
Rust neural circuit, and display the answer in a conversation window and speech
bubble. On encountering a participant it can ask what that participant can teach
it. The initial model has **nine engineered speech acts**, not general language
understanding. No external language model or cloud API is required.

All six robot profiles can observe nearby robots, NPCs and players through their
existing scene senses. Their neural motor outputs remain visible/contact cues to
other participants. The [capability contract](../content/communication.json)
keeps fish, fly, worm and hexapod channels distinct: arbitrary text is never
injected into an animal without a text receptor. This provides opportunities for
interaction; it does not establish improved learning, curiosity or biological
understanding. Native rigid-body engines observe physical actors; Java and
Bedrock add bounded native actor bounds to their spatial sensor reference.

## Build and start

From the repository root:

```sh
cargo build --locked --release --features ui,robot_io --example nn_tcp_server
python3 scripts/run_nao_social.py --sim webgl
# Equivalent unified entry point:
scripts/run_sim.sh --sim webgl --nao-social
```

Open the printed WebGL URL. The new private run directory contains `session.json`
and `environment.sh`. Enter `adapter_token` in the local simulator credential
field and connect the body. Use `join_token` in the conversation's invitation
field. Only give other players the invitation; the adapter token controls the
local simulated body. The HTTP listener is loopback-only. Minecraft multiplayer
players use authenticated server commands and need neither token.

Each run creates a fresh model and credentials; occupied HTTP/body ports are
reassigned. Stop with Ctrl+C. An unavailable engine, Java runtime, bridge or
installation produces a managed diagnostic. It never edits your Minecraft
accounts, launcher profiles, installed mods, existing saves or running BDS.

| Environment | Start / player interface |
|---|---|
| WebGL | `--sim webgl`; shared window beside the rendered world, reply bubble. Focus the canvas and use Shift+arrows to move the blue participant; ordinary arrows orbit. |
| Webots R2025a | `--sim webots`; open NAO's `nao_chat` robot window. The launcher creates a temporary world with the existing NAO body/controller. Build that controller with `make -C webots_world/controllers/nao_nn_controller_uds` if needed. |
| Unity | `--sim unity --no-engine`; source the printed `environment.sh` before starting the Editor, open a NAO scene and press Play. The component clones/configures its connector and adds focused chat. `NmNaoChat.Submit` is the host-side player/NPC integration seam. |
| Unreal 5.8 | `--sim unreal`; set `UE_ENGINE` to the engine directory if autodetection's local default differs. Build `NeuralMimicrySimEditor` first. The sample shows a Slate window and world text; `SubmitText` is a Blueprint host-side integration seam. |
| Minecraft Java | `--sim minecraft --minecraft-edition java`; see the isolated installation below. Ordinary players within 24 blocks use `/nao say hello` and `/nao stop`. |
| Minecraft Bedrock | `--sim minecraft --minecraft-edition bedrock --bedrock-dir /path/to/stock/server`; ordinary players within 24 blocks use `/aarnn:nao "Hello NAO"` and `/aarnn:nao_stop`. The loaded NAO and player must share a dimension. |

Native Unity/Unreal sample scenes are local demos. Their components do not add an
authenticated multiplayer replication layer; a multiplayer game must call the
host-side seam using its authenticated player/NPC identity and scene position.
Webots users participate through its local robot window. Minecraft's commands
already derive identity, location and dimension from its server.

## Minecraft installation

Build and package the base Minecraft worlds first using
[the Minecraft guide](../minecraft/README.md). Java needs JDK 21 and the supplied
**Minecraft 1.21.1 / Fabric** mod and matching Fabric API; the laptop's existing
26.2 Fabric installation cannot load that mod. The launcher creates a separate
`minecraft-java` game directory with the mod, API, saved world and NAO binding.
Create a separate 1.21.1 Fabric launcher profile pointing at that directory.
Source the run's `environment.sh` **before starting the Minecraft launcher**, so
the game inherits its session credentials. Existing already-running launchers do
not gain new environment variables. Open the exported lab world, enter the lab
using `/aarnn visit nao`, then arm NAO with `/aarnn connect nao` as its operator.
Ordinary invited players only need `/nao say ...`.

The Bedrock path needs the native BDS executable and data, plus Java 21 for the
Rust companion JAR. It stages a separate server with the exported world and
current connected packs, scoped Script API configuration and private secrets.
It uses the existing managed launcher on **every run**, detecting and reallocating
occupied IPv4/IPv6 UDP or TCP ports and rejecting an already-open world. Read the
allocated ports in `bedrock.log`. In the new server console, run `scriptevent aarnn:world build` to preload the
saved lab (the operation is idempotent), wait for the existing-world message,
then arm only NAO with `scriptevent aarnn:connect nao`. Join with a compatible Bedrock client. No Fabric
or Java mod goes into BDS; the JAR is an external companion. For unattended
preparation use `--no-engine`, then source `environment.sh` and run
`python3 scripts/minecraft.py launch --edition bedrock --bedrock-dir <printed-directory>`.

`--minecraft-edition auto` uses the existing detector; forcing an unavailable
edition fails with a diagnostic. A social session owns one NAO brain. Other lab
bodies remain disarmed unless separately connected to their own neural sessions.

## Speech, stopping and bounds

Dictation is feature-probed in the common browser window. Explicitly enable it,
press Dictate, review/edit the transcript, then Send. The browser may use its
vendor's speech service; the consent text explains this. NAO speech presentation
has a separate checkbox. Hidden pages stop microphone/TTS; Stop and navigation
cancel the local conversation. Native game voice-chat audio is not intercepted;
use device keyboard dictation or the common browser window.

The reference service permits 16 registered participants, eight pending turns,
one pending turn per participant, 256 UTF-8 bytes per message, 128 retained turns,
64 KiB framed transport and eight HTTP workers. One simulation writer drives the
Rust instance. Replies require observed social output, a unique winner and at
least two spikes. Silence produces a status. Disconnect/cancellation suppresses
late presentation; ambiguous neural transport failures require a fresh run.
Autonomous questions occur once per registered encounter, and are not recursively
forwarded as new utterances. Native Minecraft scans retain at most 64 encounter
identities per run and wait for the first validated body response after connection.
A fixed 128-frame presentation guard separates turns; it is
not a proof of neural quiescence.

## Model and evidence

The source `network_nao.json` stays unchanged. `scripts/build_nao_social.py`
generates `nao_social_v1`: 282 sensory inputs, 49 outputs, nine additional relay
neurons in each hidden layer. All original 250/40 matrix entries and labels are
preserved. The new snapshot records H0 sensory entry, H3 output, disabled delays,
structural growth and import rewiring. These settings are required by the
existing `nn_tcp_server` LIF reference path; this is an explicit new model, not a
checkpoint conversion. It does not establish equivalent base-body trajectories.
UTF-8/lexical preprocessing is versioned; phrase selection requires Rust output.
Gesture labels describe presentation intent; no scripted gesture overrides the
40 original motor outputs.

```sh
cargo xtask qa run --suite simulator-nao
# Playwright + Chromium required; optionally set NODE_PATH and NM_CHROMIUM:
cargo xtask qa run --suite simulator-nao-browser
# Installed engine lanes (each fails explicitly if its engine is unavailable):
cargo xtask qa run --suite simulator-nao-webots
cargo xtask qa run --suite simulator-nao-unreal
cargo xtask qa run --suite simulator-nao-bedrock
```

The suites retain source/model/contract hashes, real output counts and steps,
contract denials, lifecycle tests and browser screenshots under
`target/qa/nao-social/`. Browser speech-device tests use controlled STT/TTS objects;
the neural runtime is real. Webots/Unreal lanes exercise native body sensing and
Rust replies through chat HTTP; native-window clicks and microphones are separate
checks. The Bedrock social lane installs the exported world in a fresh server,
spawns a native villager and requires a Rust-selected inquiry in NAO's name tag.
It does not verify a Bedrock client's rendering. Minecraft native-world and
all-network suites are separate. See [the living validation record](../minecraft/VALIDATION.md).

This remains an explicitly selected legacy reference sandbox. Production
`workstation_io`, governed capture-clock admission, durable output commits and
EffectId fencing remain gated by the repository's Phase 8 plan.
