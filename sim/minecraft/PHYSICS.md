# Authoritative physics mode

This opt-in route observes one private native MuJoCo service. The service owns
body dynamics, NPC/AARNN controller arbitration and sensory/AER production. Game
displays have no local robot physics, AI or sensory stream. All 28 canonical
models can be selected without six-slot/profile aliases. Existing six-profile
illustrative modes remain available separately.

Start a simulation service and prepare private pins:

```bash
scripts/run_minecraft_sim.sh --physics-service --robots ugot_mecanum=1 \
  --control auto --token-file /tmp/aarnn-physics.token --port 8092
# In another terminal:
python3 scripts/prepare_minecraft_physics.py --port 8092 \
  --token-file /tmp/aarnn-physics.token --origin 0,64,0 \
  --output target/qa/new-private-game-config
```

A matching ready AARNN binding takes control at a physics boundary. Without one,
auto mode selects the shared programmed NPC policy. A connected silent brain
retains ownership with zero NPC drive. `--control aarnn` excludes startup/fallback
NPC behavior; `silent` and `npc` require an unbound body. NPC patterns do not
establish locomotion/task/biological competence.

For Java, use the supported profile's installed Fabric mod:

```bash
export AARNN_PHYSICS_CONFIG="$PWD/target/qa/new-private-game-config/java-physics.json"
export AARNN_PHYSICS_TOKEN="$(cat /tmp/aarnn-physics.token)"
# Start the configured Fabric server normally with those environment variables.
```

It attaches on server startup; operator `/aarnn physics attach` reattaches and
`/aarnn physics stop` removes its display and requests disarm/pause. Credentials
remain server-side. The 1.21.1 access widener enables private display setters;
26.2 uses the official namespace generated from that same source. Thin
`PhysicsCompat` classes adapt renamed APIs; world/client logic is shared.

For Bedrock Dedicated Server, build the server behaviour/resource packs with
`python3 scripts/build_minecraft_bedrock.py`. Merge the prepared
`bedrock-physics-variables.json` fields into the server's existing private
variables, and `bedrock-physics-secrets.json` into its private secrets. Preserve
all other grants and secrets. Its server-admin/network modules must already be
permitted by the normal BDS installation. Physics uses a separate explicit
`AARNN_ALLOW_PHYSICS_SERVICE` and `AARNN_PHYSICS_AUTHORIZATION` secret; enabling
legacy inference/chat never enables physics. An operator or server command uses
`/scriptevent aarnn:physics_attach`; `aarnn:physics_stop` stops the observer.
Offline packs do not open this network route.

The coordinate mapping is `[x,y,z] -> [x,z,-y]` plus the configured game origin,
with one metre per block. Body quaternions compose with each geometry's local
pose. Box proxies render timestamped native states; their visual transforms
never become sensed robot motion or a physical grasp. Java/Bedrock orientation
rendering still needs native rendered acceptance.

The current collision tier is deliberately bounded: at most 32 full blocks
above a matching flat native floor, in a five-block-wide local neighborhood.
The adapter requests pause, atomically replaces the collision set using the
current environment/reset/model epoch, and resumes only after acknowledgement.
Ambiguous requests are not retried. Reset/model changes require explicit
reattachment. Partial blocks, doors, perches, nearby players, unloaded chunks or
overflow pause/disarm instead of being treated as empty space. These limitations
change collision experiments; full functional game acceptance remains open.
A detached game observer cannot certify that later game edits reached the
service. High-speed boundary/lag coverage and mixed-world services remain open.

Reproduction:

```bash
cargo xtask qa run --suite lec-crow-minecraft-service
cargo xtask qa run --suite simulator-minecraft-bedrock
cargo xtask qa run --suite simulator-minecraft-world
cargo xtask qa run --suite simulator-minecraft-bedrock-native
```

The first lane proves both client implementations against a real authenticated
Rust/MuJoCo service, including identity/reset/stale-write denial. It submits no
robot sensory/AER frame. Official Bedrock type/pack checks and actual native
Java/BDS regressions are separate from new rendered/full functional acceptance.
Missing runtimes/assets remain blocked/not-run in the 224-row expansion matrix.
