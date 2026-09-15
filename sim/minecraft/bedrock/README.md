# Native Bedrock Dedicated Server adapter

AARNN supplies native Bedrock behaviour/resource packs as well as its Java/Fabric
JARs. This adapter runs on **Bedrock Dedicated Server (BDS)**; it is not a Geyser
proxy, and Fabric JARs must not be installed as Bedrock add-ons. The same standalone
AARNN companion JAR connects either edition to the authoritative Rust brains.

The packs require Bedrock **1.26.30 or newer with Script API 2.9.0 support**.
Connected operation additionally requires the **Beta APIs** world experiment,
`@minecraft/server-net` and `@minecraft/server-admin` version `1.0.0-beta`.
These experimental modules are available only in BDS. The launcher requires a
successful runtime script probe because experimental APIs may change between
server releases. Unsupported modules cause an explicit failure and cleanup.

Native server verification uses the supplied **BDS 1.26.45.1** in an isolated lab.
The API types used for verification are pinned in `package-lock.json`: stable
server 2.9.0 and network/admin declarations from 1.26.60-preview.23. This is
API/type evidence, not a claim that that preview server ran on this laptop.

## Supplied software and world

`dist/minecraft/` contains:

- `AARNN-Bedrock-Sensory-Lab.mcworld`: the saved, native-verified six-profile lab,
  including offline packs. Import it into a compatible Bedrock client, or extract
  it as a ZIP into a dedicated BDS world directory.
- `AARNN-Bedrock-offline.mcaddon`: offline behaviour and resource packs. Import
  this into a compatible Bedrock client to inspect the worlds without a brain.
- `AARNN-Bedrock-server.mcaddon`: the BDS connector variant and resource pack.
- `AARNN-Bedrock-server-overlay.zip`: the same server variant arranged into BDS
  `behavior_packs/`, `resource_packs/` and scoped `config/` directories, with
  activation manifests for `worlds/AARNN-Sensory-Lab/`.
- `aarnn-minecraft-0.1.0-bridge.jar`: the shared executable Rust companion.

The offline and server variants have the same pack identity. Install **one
variant per world**. The server variant cannot load on an ordinary client or
Realms because those environments do not provide its networking/admin modules.
Bedrock clients joining BDS receive the resource pack; server scripts stay on BDS.

The add-on constructs the reproducible six-plot lab using native Bedrock APIs.
It includes C. elegans, BANC and FAFB flies, hexapod, NAO and zebrafish, with the
shared 602-object catalogue, cutaway anatomy and neural channel identities.
The saved `.mcworld` contains the newly generated lab database and offline packs;
it includes no player/account data, credentials or proprietary game binary.
The add-ons can also construct the lab explicitly in a new flat creative world.
Construction is not automatic on server startup.

## Prepare a dedicated lab

1. Install a compatible [official Bedrock Dedicated Server](https://www.minecraft.net/en-us/download/server/bedrock)
   in its own directory and follow Mojang's installation and licence instructions.
2. Extract **AARNN-Bedrock-Sensory-Lab.mcworld** as a ZIP into the BDS directory
   `worlds/AARNN-Sensory-Lab/`. It already contains the flat lab and Beta API
   setting. Alternatively create a new flat creative world in a Bedrock client,
   enable cheats and **Beta APIs**, activate the offline add-on, then export it
   into that directory. Preserve `level.dat` and `db/`; they carry the world and
   experiment settings. Connected packs require Beta APIs as well as module grants.
3. Stop BDS. Extract `AARNN-Bedrock-server-overlay.zip` into that dedicated server
   directory. This selects the server variant for the exported world. Use a
   separate lab directory so the overlay does not replace another installation.
   Some world exports embed copies under the world's own `behavior_packs/` and
   `resource_packs/`. Keep the original export as a backup and move only those
   embedded AARNN copies aside, so they cannot shadow the server variant. Detection
   rejects duplicate AARNN pack UUIDs across the root and selected world.
4. Set these entries in that server's `server.properties`:

   ```properties
   level-name=AARNN-Sensory-Lab
   level-type=FLAT
   gamemode=creative
   allow-cheats=true
   online-mode=true
   allow-list=true
   texturepack-required=true
   ```

   Add your licensed account to the server allow-list and grant operator access
   using BDS's normal administration commands. The world can also be constructed
   through the server console.
5. The overlay includes `config/<script-UUID>/permissions.json` with only the three
   required script modules. Do not grant these modules globally. In that same
   directory's `variables.json`, set `AARNN_ALLOW_SANDBOX_INFERENCE` to `true`
   after reviewing the sandbox boundary. Preserve its content digest.
6. Generate one random `AARNN_MINECRAFT_TOKEN` for the companion, as described in
   the Java installation guide. In the scoped BDS `secrets.json`, store the entire
   header value under `AARNN_AUTHORIZATION`: `Bearer ` followed by that token.
   BDS does not expand environment variables inside this JSON file. Restrict file
   access to the server account. No secret belongs in a pack, world, entity or Git.

To create that private file from your environment without printing its contents,
run this from the repository root after setting `NM_BEDROCK_DIR` and the token:

```sh
python3 - <<'PY'
import json, os
from pathlib import Path
metadata = json.loads(Path('sim/minecraft/build/bedrock/bedrock-content.json').read_text())
token = os.environ['AARNN_MINECRAFT_TOKEN']
assert 24 <= len(token) <= 4096
folder = Path(os.environ['NM_BEDROCK_DIR']) / 'config' / metadata['pack_ids']['script']
path = folder / 'secrets.json'
with path.open('x') as stream:
    path.chmod(0o600)
    json.dump({'AARNN_AUTHORIZATION': 'Bearer ' + token}, stream)
PY
```

This deliberately refuses to replace an existing secret file. Review an existing
server's secret configuration through its normal administration procedure.

## Detect, force and launch

From the repository root:

```sh
export NM_BEDROCK_DIR="$HOME/Games/AARNN-Bedrock-Server"
python3 scripts/minecraft.py doctor --edition bedrock
./scripts/run_minecraft_sim.sh --minecraft-edition bedrock --robots 'zebrafish=1'
```

`--minecraft-edition auto` is the default. It prefers a compatible installed
Java/Fabric/AARNN client; otherwise it selects a detected BDS executable.
`--minecraft-edition java` and `bedrock` force the edition without suppressing
capability checks. `NM_MINECRAFT_EDITION` supplies the same default in automation.
`--bedrock-dir`, `NM_BEDROCK_DIR`, `NM_BEDROCK_SERVER` and a `bedrock_server` on PATH
support non-default installations. Versioned `~/Developer/bedrock-server-*` folders
are detected too, newest numeric version first. Detection never launches an
executable just to infer its version.

Preflight verifies the selected world, installed pack bytes, activation lists,
module permissions, variables and presence of the private secret file. It does
not read or display the secret's contents. Missing requirements return exit **3**
before starting brains. At launch, BDS must emit a matching script readiness
record within 180 seconds. Module/experiment/version failures stop the managed
server and trigger brain/bridge cleanup; a forced edition never silently changes.
The console is forwarded while the managed server runs. Stop normally with `stop`
or Ctrl+C. The launcher requests a server save/stop before bounded termination.

**Ports are checked and resolved on every managed launch.** Occupied RakNet UDP
IPv4/IPv6 listeners or NetherNet TCP/UDP listeners receive free replacement ports.
The launcher prints the selected endpoints and reserves sockets until the spawn
boundary. It retries a proved startup bind race up to three times. If RakNet's
implicit LAN discovery ports are occupied, LAN broadcast is disabled for that run;
connect using the displayed port. A reassigned externally mapped NetherNet UDP
range requires its firewall/NAT forwarding to target the newly displayed port.

Per-run settings and an endpoint report live under `.aarnn-runs/run-*/` in the
selected server installation. Packs and worlds are linked to their canonical
locations; the original `server.properties` is preserved. This requires filesystem
symlink support, which is checked by the launch operation. An already-running
server using the same installation/world is rejected; choosing another port never
permits two writers to one world. Stop that server normally or use a separate lab
directory. No unrelated listener or server is terminated.

`--no-engine` starts only Rust brains and the companion; BDS can run separately.
Hosts supplying Rust workers need no Minecraft installation. The companion and
BDS networking script use loopback, so run them on the same host (Rust workers
can still be distributed through the existing launcher adapters).

## Lab commands

Run these as an operator player, or omit the slash in the server console:

```text
/scriptevent aarnn:world build
/scriptevent aarnn:visit zebrafish
/scriptevent aarnn:anatomy zebrafish
/scriptevent aarnn:senses zebrafish
/scriptevent aarnn:connect zebrafish
/scriptevent aarnn:disconnect zebrafish
/scriptevent aarnn:status
/scriptevent aarnn:stop
```

Replace `zebrafish` with any canonical profile ID. `visit` requires a player.
A reconnect may specify its initial sequence, for example
`/scriptevent aarnn:connect zebrafish 100`; inspect any ambiguous backend result
first. Neither a timeout nor reconnect automatically resubmits input.

Construction uses one bounded ticking area and 12 persistent entities. Plots are
at Y=128, with 16-block half extents and 48-block spacing. The zebrafish tank has
native water ten blocks deep and a glass enclosure. Existing blocks with another
identity are rejected instead of being cleared. The substrate supports players;
fine scenery is procedural entity geometry. An incomplete build reports its
failure and leaves existing content in place for inspection.

Every server restart disarms bodies. Stop/revocation/unload neutralises local
outputs. One request per profile may remain pending; a late reply cannot re-arm
a stopped body. Wall-clock HTTP budgets are 60 seconds for flies, 10 for fish
and 2.5 for other profiles. Each accepted reply advances the bounded kinematic
preview once; pending neural work advances no visible movement.
Saved entities load asynchronously. `aarnn:world build` requests ticking-area
preloading and waits up to 600 server ticks when the saved lab exists, without
creating replacement bodies. Each loaded robot is explicitly disarmed. Enable
cheats in the imported world's settings when using these commands in a client.

## Verification and limits

```sh
cargo xtask qa run --suite simulator-minecraft-bedrock
cargo xtask qa run --suite simulator-minecraft-bedrock-native
```

The lane builds both pack variants, checks all six profiles against the WebGL
sensor oracle, exercises malformed replies/cancellation, checks world construction
and water through controlled API fixtures, tests missing/stale/forced server
detection, and type-checks against pinned Microsoft API declarations. The native
lane requires Linux BDS, Java 21 and the six local Rust snapshots. It creates a
fresh flat lab, checks native water/entities, exchanges real neural frames,
disarms, saves/reloads and verifies clean shutdown. Occupied TCP and UDP listeners
exercise the same allocator used by the launcher. The saved-world packer rejects
stale native evidence or mismatched packs. BDS generates the complete native
`level.dat` before the QA bootstrap enables Script API experiments. Native vanilla
NPC identity must survive both initial construction and reload; incomplete
handwritten world metadata is not used. Use the refreshed export for a new lab;
do not transplant its `level.dat` into an existing world database.
See [the validation record](../VALIDATION.md)
(or `VALIDATION.md` in the bundle) for exact results. Bedrock client screenshots
remain unverified; Java native screenshots and GameTests are separate evidence.

Bedrock renders bounded cuboid approximations of shared spheres and cylinders.
Worm/fish segment activity drives generated bones; the motion and sensing reference
is generated verbatim from WebGL. Native placed blocks constrain movement. Fine
scenery, chemical fields, currents and receptors are engineering proxies. These
checks establish content and contract agreement, not identical physics,
photorealism, calibrated biology or production governed peripheral semantics.
The companion still uses legacy AER1, with fixed 1 ms steps and input threshold
0.5; production clock mappings, durable admission and EffectId fences remain a
separate gated track.

Official references: [Dedicated Server scripting](https://learn.microsoft.com/en-us/minecraft/creator/documents/scriptingservers),
[server-net](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server-net/minecraft-server-net),
[server-admin](https://learn.microsoft.com/en-us/minecraft/creator/scriptapi/minecraft/server-admin/minecraft-server-admin).

## NAO interaction

The [NAO social launcher](../../nao/README.md) prepares an isolated BDS instance
with scoped `AARNN_NAO_AUTHORIZATION` and the actual allocated local HTTP port.
Ordinary players use `/aarnn:nao "hello"` and `/aarnn:nao_stop`; no operator status
is required for chat. Only the operator arms NAO using `scriptevent aarnn:connect nao`.
A connected NAO can ask a nearby player, villager or NPC a neural-selected question.
Nearby actors' native bounding boxes enter all six profiles' visual/contact/proximity
senses; unsupported species do not gain text receptors. Replies are temporary
name-tag bubbles. Device keyboard dictation or the common browser window provides
optional speech input. The offline pack remains disarmed and has no HTTP capability.
