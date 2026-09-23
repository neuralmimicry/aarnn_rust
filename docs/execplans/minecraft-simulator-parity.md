# Deliver Minecraft Java and native Bedrock sensory labs

This ExecPlan is a living document maintained under `.agent/PLANS.md`, owned by
Phase 8 and extending `simulator-content-parity.md`.

## Purpose and observable outcome

Deliver Java/Fabric and native Bedrock worlds for C. elegans, separate BANC/FAFB
flies, hexapod, NAO and zebrafish. Supply the mod and Rust companion JARs, Java
saved world, Bedrock behaviour/resource packs, installation instructions,
autodetection, managed launch and per-network evidence. Fish must be submerged.

## Specification authority and traceability

Sections 3.4, 16.15–16.24, 17.4, 19.3, 20.9–20.10 and 21.1/21.13–21.15 govern
the boundary. Preserve INV-002, INV-007, INV-008 and INV-015–017. Scenario
SIM-MINECRAFT-001 covers shared content, channel identity, bounded transduction,
cancellation, packaging and native scene construction. Production IO-E2E gates
are separate and remain open.

## Prerequisites and phase boundary

Phases 0–7 production gates and `workstation_io` remain unchanged. The shared
`/api/aer/infer` authorisation gate is intentionally closed and was preserved.
These are explicitly enabled, token-protected loopback reference sandboxes using
the existing Rust TCP **AER1** adapter, not the production **AARNN-AER/1** protocol.
No production capture mapping, durable admission, committed effect or EffectId
claim follows from these experiments. All released worlds start disarmed.

## Scope

Five shared habitats; six profiles; schematic anatomy/cutaways; reference sensing
and motion; native water; explicit connect/disconnect/stop; bounded asynchronous
Rust requests; edition detection; port collision management; packages and QA.

## Non-goals

No Java/JavaScript neural integrator or copied connectome dataset; no production
peripheral cutover or real actuator; no biological adequacy or cross-engine physics
claim. Bedrock is native BDS, not a Geyser proxy. Bedrock client rendering and Unity
Editor validation need their respective clients/toolchains.

## Repository orientation

Root `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`, branch
`codex/simulator-content-parity`. Discovery read root instructions, Phase 8,
referenced specification, Cargo workspace, CI/deploy/mobile/signing definitions
and ADRs. `cargo metadata --no-deps --format-version 1` is retained in
`/tmp/aarnn-minecraft-metadata.json`. Preserve the large unrelated dirty tree,
including runtime/UI/network/config edits. No commit or deployment requested.

Canonical sources:

- `sim/content/catalog.json`, `scripts/sim_content.py`,
  `scripts/robot_profiles.py`, Webots `*.io_alignment.json`: shared content,
  dimensions and available ordered names.
- `scripts/regenerate_simulator_assets.py`: derived visual assets only; it does
  not regenerate user network snapshots or configs.
- `web_ui/webgl-world.js`: bounded sensor/motion reference, generated verbatim
  into Bedrock. `src/bin/web_ui.rs` retains its authorisation gate.
- `examples/nn_tcp_server.rs`, `src/tcp_aer_ipc_bridge.rs` and the
  `tcp_aer_ipc_bridge` binary: existing Rust sandbox endpoints. No additional
  Rust workspace or neural implementation. The Python NAO reference proxy uses
  only the isolated `scripts/aer_legacy_codec.py` compatibility helpers.
- `sim/minecraft/src/`, Gradle build and `scripts/minecraft.py`: Java adapter,
  companion, detection and certified saved-world export.
- `scripts/build_minecraft_bedrock.py`, `sim/minecraft/bedrock/scripts/`,
  `scripts/bedrock.py`, `scripts/bedrock_ports.py`: generated native BDS adapter,
  scoped configuration, versioned-install discovery and safe managed launch.
- `scripts/qa/run_minecraft.py`, `probe_minecraft_{neural,bedrock}.py`,
  `bedrock_native_probe.js`, contract/detection/port fixtures and `tools/xtask`:
  bounded native/reference lanes and retained evidence.
- `scripts/run_minecraft_dual_aer_growth.sh` and
  `scripts/qa/minecraft_dual_aer_growth.py`: additive two-runner hexapod
  experiment. The proxy preserves the Java companion's legacy AER1 framing,
  inserts two explicit +1 ms causal hops, and records fixture control scores and
  growth evidence. It does not replace the production AARNN-AER/1 path.
- `scripts/package_minecraft.py`: isolated Bedrock regeneration comparison,
  Java acceptance/source hashes, pinned official prerequisites and SHA256SUMS.

Inspiration `/home/pbisaacs/Developer/fly-brain-minecraft` is MIT-licensed Fabric
1.21.1. Its README, licence, Gradle configuration, entity, sensing and rendering
were inspected. Its Java LIF/connectome integrator was not imported.

Java 21 is installed at `/usr/lib/jvm/java-21-openjdk-amd64`, while shell Java is
8. Builds autodetect Java 21. This laptop's installed Minecraft 26.2/Fabric
0.160.0+26.2 is incompatible with the pinned 1.21.1 adapter; install in a separate
game directory. No user launcher/accounts/mods/saves were changed.

The user later supplied native BDS at
`/home/pbisaacs/Developer/bedrock-server-1.26.45.1`. It is already running; its
process, settings and world are preserved. Native QA copies stock runtime assets
into a fresh evidence directory and authors a new flat, Beta-API-enabled lab.

## Architecture and safety constraints

Rust owns neural state. Minecraft owns sampling and bounded visual movement.
Worlds contain no credentials/checkpoints or automatic connection. Each profile
has one outstanding request; session generation rejects late output after stop.
Timeout or ambiguous response disarms without retrying input. Network identity
is bound to the exact profile, including BANC versus FAFB.

The companion uses AER1, threshold >0.5 and fixed 1 ms Rust steps. Capture metadata
identifies the sandbox sample but does not certify production clock admission.
Requests and returned legacy executor step indices are distinct; output must
advance beyond the previous output, matching the Java adapter. Reconnect requires
explicit sequence selection after inspection or a fresh isolated experiment.
Handshake limit is 60 s; response limits are 60 s for flies, 10 s for fish and
2.5 s for other profiles. These are wall-clock limits, not biological time.

The normalized X-forward/Y-left/Z-up catalogue converts to Minecraft (X,Z,-Y)
with half extent 16 blocks. Geometry is pedagogically magnified. Bedrock uses
bounded cuboid approximations, with worm/fish properties driving authored bones.
The glass fish tank contains real water and covers the entire dorsal extent.
Schematic anatomical landmarks follow the references/provenance in
`sim/content/README.md`; no fitted biological parameter is invented.

Java uses Fabric 1.21.1, Loader 0.17.3, API 0.107.0+1.21.1, Loom 1.12.7 and Gradle
9.1.0. Bedrock uses server Script API 2.9.0 and optional server-net/admin beta
modules. Network/admin APIs require BDS and Beta APIs; offline and connected packs
share identity, so install one variant. Private scoped SecretString configuration
contains the authorization header, never a world/client property or pack secret.

Managed BDS launches reserve actual TCP/UDP sockets, choose available replacements,
and check implicit RakNet LAN ports. NetherNet UDP ranges are checked too; a changed
external NAT mapping needs the displayed new port. Settings go into a per-run
folder with links to the same world/packs; original server.properties stays intact.
A managed lock, Linux process/directory checks and available database locks prevent
second writers. Never delete database locks, kill another server, or replay input
on retry. Only a proved startup bind race gets up to three fresh port attempts.

## Milestones

1. Generate matching six-profile content and build pinned Java/Bedrock adapters;
   contract oracles prove dimensions, bounded geometry and sensing tolerances.
2. Construct actual worlds and verify submerged fish, native entities, disarming
   and persistence; native rendering evidence is recorded separately.
3. Execute each real Rust snapshot through the companion and native BDS where
   available. Test unavailable/wrong-edition/stale config and occupied ports.
4. Package certified JARs/world and fresh Bedrock packs with notices, installation
   instructions and retained evidence. No production phase gate is promoted.

## Progress

- [x] `2026-09-15 20:00Z` User requested additive Java-version support for the
  installed Minecraft 26.2 profile. Discovery confirms the current detector,
  Fabric metadata and Gradle build are pinned to 1.21.1; the installed profile
  uses Fabric Loader 0.19.5, Fabric API 0.160.0+26.2 and Java 25. A parameterised
  26.2 compile currently reaches Loom but fails because Loom 1.12.7 cannot find
  official Mojang mappings for 26.2. The next milestone is a real versioned
  profile/build path, with the existing 1.21.1 path retained.
- [x] `2026-09-15` Versioned Java profiles are implemented. The 1.21.1 baseline
  builds with Java 21 and remapped Loom sources; 26.2 builds with Java 25, Loom
  no-remap sources and the new client render-state submission API. Artifacts are
  named with their Minecraft version. Detection selects isolated launcher game
  directories, enforces each profile's loader/API/mod/Java tuple and recognizes
  this laptop's 26.2 installation. Both Gradle builds and the six profile
  detector tests pass. The installed profile still needs the built AARNN mod jar
  copied into its `mods/` directory before engine launch.
- [x] `2026-09-15` Auto frontend selection now prefers an available Java
  launcher/profile over a detected Bedrock server. Java engine preflight remains
  authoritative and reports missing Fabric/API/AARNN artifacts instead of
  silently launching the Bedrock frontend.
- [x] `2026-09-15` Minecraft `/aarnn status` now exposes per-robot neural
  exchange telemetry: submitted and returned frame counts, logical steps, last
  output spike count and sensory mean. This makes a live Java-to-Rust exchange
  verifiable in Minecraft chat; local `/aarnn senses` remains explicitly
  separate from transport verification.
- [x] `2026-09-15 21:08Z` Live Minecraft launch verification found that
  `--node 3` reached `run_webot.sh` and started three workers, but the one-layer
  C. elegans placement retained only two active primary/backup targets. The
  rebalancer then unloaded the unnamed IPC owner after replacement readiness,
  leaving two connected workers and breaking the frontend's third-node
  expectation. The fix gives each IPC owner a stable `<brain>_ipc` identity,
  prefers it in target selection and protects it from the unload handoff. The
  bounded Rust placement test and shell checks pass against the updated source;
  the rebuilt live run is recorded below.
- [x] `2026-09-15 23:15Z` Rebuilt the Rust runtime and reran the live command
  `scripts/run_sim.sh --sim minecraft --robots "celegans=1" --node 3`. The
  fresh runtime reports three connected nodes, retains `celegans_0_ipc`, and
  keeps the IPC owner through startup placement. A bounded authenticated
  `celegans` request returned all 96 validated output addresses in under one
  second; the probe run was discarded and the final run was restarted so the
  frontend sequence begins at its configured step.
- [x] `2026-09-15 23:16Z` Rebuilt the installed 26.2 mod after correcting the
  status wording. `pending: first neural frame` now identifies the initial
  reply; `active: legacy sandbox (frame pending)` identifies normal one-credit
  operation after a successful reply. `/aarnn status` telemetry is present in
  the installed JAR. Java 26.2 Gradle tests/build and Python detection tests
  pass after regenerating Bedrock fixtures.
- [x] `2026-09-15 22:26Z` Resolved the saved-lab connection failure shown by the
  operator. A catalogue digest mismatch now reports the exact recovery command
  instead of combining configuration and saved-content failures. Added explicit
  `/aarnn review`, which requires a complete six-profile disarmed lab and
  acknowledges the current content digest without migrating neural state or
  arming a session. Status exposes `content=match` or `content=review-required`.
  Both 26.2 and 1.21.1 Java test/build lanes pass; the rebuilt 26.2 JAR is
  installed at the detected game directory.

- [x] `2026-09-15 11:42Z` Repository/inspiration/toolchain discovery complete.
- [x] `2026-09-15 12:40Z` Shared water catalogue reaches 602 objects, digest
  `4d0d506e80f146acf04e34ee781f67ab465c6d1f336b73c799bc3e3904a6b2d8`.
- [x] `2026-09-15 13:11Z` Java contract (13 JVM checks), native six-profile
  GameTest, clean world export, 12 native habitat/cutaway captures and six real
  Rust snapshot round trips pass. Final fish depth uses the authored water level.
- [x] `2026-09-15 13:13Z` Bedrock pack/API fixtures, six profiles, official API
  type checks and five initial detector cases pass; bundle staging succeeds.
- [x] `2026-09-15 13:24Z` Newly supplied native BDS 1.26.45.1 loads modules,
  constructs 12 entities/18,992 blocks and proves fish submersion. Native evidence
  exposed and corrected float-property defaults, stock JSONC detection and the
  legacy output/request sequence distinction.
- [x] `2026-09-15 13:24Z` Versioned Developer installation autodetection, six
  detector cases and four real-socket/lock tests pass. Active user server is
  explicitly reported as already running; its process/settings are untouched.
- [x] `2026-09-15 13:37Z` Native six-network/stop/reload/clean-exit acceptance
  passes in `target/qa/minecraft/bedrock-native-j68cl4mh/`. Each network completed
  four native frames; 12 entity IDs survive reload; fish remains submerged.
  Deliberately occupied TCP and both RakNet UDP ports are reassigned. The native
  `.mcworld` and receipt are built with offline packs and no observer/credentials.
- [x] `2026-09-15 13:37Z` Final Bedrock fixtures/type/detection/port suite passes
  in `target/qa/minecraft/bedrock-77tl8lpm/`; formatting, xtask unit test and strict
  xtask Clippy pass. Original user server remains running without changes.
- [x] `2026-09-15 13:40Z` Final exported `.mcworld` reload passes with native
  water, 12 disarmed entities and clean exit in
  `target/qa/minecraft/bedrock-export-n_eiqsf5/`. Package staging and all 19
  SHA256SUMS entries pass; final documentation reflects native BDS availability.
  Only the pre-existing user BDS process remains; all task-owned engines/brains
  and companions have stopped. No user Minecraft/server settings were modified.
- [x] `2026-09-15 13:34Z` All six native networks returned frames; the initial
  reload exposed asynchronous entity availability. The adapter now preloads the
  saved ticking area, waits without replacing bodies and disarms on entity load.
  Focused real-server reload passes in `target/qa/minecraft/bedrock-reload-jop9s1xa/`.
  Actual managed-launch/occupied-TCP/console/clean-exit smoke passes in
  `target/qa/minecraft/bedrock-launch-vcwu2c0c/`, preserving original properties.
- [ ] Bedrock client screenshots, Unity Editor build and calibrated cross-engine
  biological/physics parity remain distinct unavailable/unmet acceptance lanes.

- [x] `2026-09-20` Added the bounded dual-brain experiment. Two independently
  generated hexapod configs are routed through separate `aarnn_tcp_server`
  processes and the Java bridge's one-profile route. The proxy records
  A-to-B and B-to-A AER frame hops, returned logical timestamps, growth events
  and the fixture's isolated-versus-coupled motor score. The verified headless
  command was `./scripts/run_minecraft_dual_aer_growth.sh --no-engine
  --iterations 12`; its retained receipt recorded 12 frames and two growth
  paths. The bounded combiner now models B's delayed AER output as an
  inhibitory gate over A's final motor spikes; this avoids treating two
  saturated spike sets as a stronger command. The 24-frame rerun in
  `target/qa/minecraft-dual-aer-pKUiLJ/result.json` recorded both AER directions,
  195 growth log events and a fixture-score increase from `0.8263888889` for
  isolated A to `0.8333333333` for the coupled output. This remains a legacy
  AER1 fixture result and is not a claim of learned Minecraft physics control.

- [x] `2026-09-20` Corrected the launcher integration after the first operator
  run. Engine mode now selects a loopback HTTP port, writes that endpoint into
  the generated Minecraft config and keeps the bridge and client on the same
  route. The launcher prints readiness for brain A, brain B, the dual AER
  fabric and the Java bridge, and writes a failure receipt after any later
  control assertion so startup evidence survives cleanup. A headless one-frame check retained evidence in
  `target/qa/minecraft-dual-aer-VDNUX0/result.json`; both brain listeners and
  both growth paths were present, while the separate control-improvement
  assertion remained correctly failed.

- [x] `2026-09-20` Fixed the in-game connection failure shown by the operator.
  The installed Java config had an empty `hexapod.networkId`, so
  `/aarnn connect hexapod` rejected the binding before opening the bridge. Live
  launcher mode now atomically prepares the hexapod binding, records the prior
  config in the run receipt, and keeps both brain processes and the bridge alive
  until Ctrl-C. A bounded live launch with `--no-keep-alive --iterations 1`
  verified the endpoint, binding dimensions and fixture path; a keep-alive
  smoke was stopped after printing the interactive commands. The Minecraft
  process must be restarted from the launcher started by this run when its
  previous process did not inherit `AARNN_MINECRAFT_TOKEN`.

- [x] `2026-09-20` Fixed the follow-up operator failure where Minecraft opened
  only the ordinary last-played world with no visible neural body. The live
  launcher now writes an explicit `autoConnectOnStart` hexapod profile and the
  Fabric server builds the lab, arms the configured route and teleports the
  first player to the hexapod plot once chunks are ready. Ordinary configs
  remain disarmed because the automatic path is opt-in. The server emits a
  HUD indicator and logs the automatic connection; `/aarnn status` remains the
  authoritative frame and spike check.

- [x] `2026-09-20` Replaced repeated in-game status/chat output with a compact
  Fabric HUD indicator in the upper-left: grey is offline/disarmed, yellow is
  connecting, green is active and red is faulted. Automatic startup now makes
  one connection attempt per server session and leaves a fault visible for
  operator recovery instead of reconnecting every tick. The live launcher also
  starts Minecraft at the first logical step after its bounded fixture feed so
  the fixture route cannot reject the real hexapod as stale.

- [x] `2026-09-20` Reduced the connection HUD to a text-free traffic-light
  button in the upper-left. Automatic startup no longer writes success, failure
  or visit guidance into the player's scrolling chat; the three lamps provide
  the persistent visual state and `/aarnn status` remains available on demand.

- [x] `2026-09-20` Hardened repeated launcher runs after a stale bridge on the
  fixed default port caused a new token to receive HTTP 401 from the old
  process. Engine mode now selects a free loopback HTTP port and writes that
  endpoint into the generated Minecraft config. Startup waits for an
  authenticated bridge health response and fails immediately if the child
  bridge exits or another process owns the requested port.

- [x] `2026-09-20` Fixed live-session contention in engine mode. The bounded
  fixture driver is now restricted to `--no-engine` QA; engine mode waits for
  actual Minecraft hexapod frames and reports failure if they do not arrive.
  The traffic-light HUD now truly dims inactive lamps, and `/aarnn status`
  reports one compact hexapod line instead of flooding chat with every saved
  lab profile.

- [x] `2026-09-20 14:46Z` Removed the remaining manual launcher/world-selection
  step for the installed Java profile. `scripts/minecraft.py launch --direct`
  now resolves inherited Minecraft/Fabric library metadata, starts Fabric Loader
  with Java 25 and uses `--quickPlaySingleplayer`; the dual-brain runner invokes
  this path with `New World` by default and accepts `AARNN_MINECRAFT_WORLD` for
  another existing world. Direct launch was verified from a stopped client: the
  log loaded Fabric Loader 0.19.5, AARNN 0.1.0 and Fabric API 0.160.0+26.2.
  The bounded live run `scripts/run_minecraft_dual_aer_growth.sh
  --no-keep-alive --iterations 4` then passed with 19 frontend frames, A-to-B
  and B-to-A traffic, growth events from both brains, and control improvement
  from `0.8245614035` isolated to `0.8333333333` coupled in
  `target/qa/minecraft-dual-aer-wBwMci/result.json`.

- [x] `2026-09-20 14:57Z` Fixed the transparent hexapod reported in the 26.2
  screenshot. The custom mesh had been submitted through `debugQuads`, whose
  translucent pipeline does not write depth, so clouds could be drawn over the
  body. Both Java renderers now use an opaque, no-cull entity cutout layer with
  explicit texture, overlay, lightmap and normal attributes. The installed 26.2
  mod matches the rebuilt artifact SHA256
  `0c2802660972423c5e92862db3770afef3333a7a2d8f4a26c94514b7700784b8`.
  The installed 26.2 smoke run passed with 14 live frames and both AER
  directions; 26.2 and 1.21.1 Gradle test/parity suites also pass.

- [x] `2026-09-23 09:15Z` Cross-checked the local Minecraft failure logs and
  confirmed that distributed simulator startup intentionally sets
  `NM_DISTRIBUTED_AUTOSTART=0`, while the TCP bridge blocks binary AER frames
  until `neural_runtime_armed` exists. The Minecraft companion health handshake
  completed, but the launcher started the game without invoking the existing
  cluster-control `start` operation, so the bridge reported no neural traffic.
  Minecraft now calls `arm_distributed_networks` after `wait-bridge` and before
  either Java or Bedrock launch. The same missing barrier existed in the Unity
  editor path and was added there; Unreal already had the required ordering.
  Webots and WebGL retain their separate autostart-enabled startup paths.
  The shared readiness message is simulator-generic, and the launcher test now
  checks handshake-to-arm ordering for all three gated frontends.

- [x] `2026-09-23` Investigated the orchestrator screenshot and the newest
  `sim_cluster_85414` logs. The registry contains one network, `hexapod_0`,
  across the three expected node IDs; it does not contain two registered
  networks. The UI received one early snapshot (`1 shard`) and then retained
  that biological topology/edge cache while later cluster snapshot requests
  failed 489 times with `shard 'hexapod_0_ipc' has a mismatched layer range`.
  During the same interval the compatibility placement repeatedly changed
  between active layer `[1]` and `[0, 1, 2, 3, 4, 5, 6]` with redundant copies.
  Cluster layout counts are calculated from the placement distribution, while
  the cached topology and edge projection come from only the selected shard;
  missing or mismatched layer positions are filled with the ordered column
  fallback. The screenshot therefore combines one stale biological shard
  projection with synthetic placement/layout columns for the same network.
  The workers also imported the same 4,025,579-byte snapshot independently and
  reported divergent growth totals (worker 01: 628 -> 707; worker 02: 628 ->
  697), which confirms that this compatibility path is maintaining separate
  per-worker runner projections rather than a single merged biological graph.

- [x] `2026-09-15 15:12Z` Social/participant refresh: Java native all-six NPC sensing,
  clean save and JAR/world build pass in `world-9qrr1mf7/`. Bedrock timer-offset,
  ordinary-player command, API/detection/port suite passes in `bedrock-ezsbn3o3/`.
- [x] `2026-09-15 15:12Z` Native metadata correction passes all six Rust profiles,
  water, vanilla NPC identity before/after reload, saved robot IDs, occupied TCP/
  IPv4/IPv6 UDP resolution and clean shutdown in `bedrock-native-zbgkt3b4/` (450.7 s).
  The `.mcworld` is freshly certified. Earlier handmade-metadata exports are
  superseded; no user world is patched. See the NAO plan for the independent
  native inquiry test and the final package checksum record.

- [x] `2026-09-15 15:29Z` Final readiness-aware adapters: 13 JVM tests and native
  Java world/JAR export pass in `world-tlmb7e_5/`; controlled BDS/type/detection/
  port/metadata suite passes in `bedrock-p3dzqchb/`. Native BDS all-six Rust,
  water, vanilla NPC identity, port reassignment, stop/reload and clean-exit
  recertification passes in `bedrock-native-82l2875w/` (386.5 s). This supersedes
  the preceding source certificate. Actual NPC/Rust inquiry passes in the NAO
  plan; final package refresh follows the new world install check.

- [x] `2026-09-23 10:18Z` Cluster-global UI projection now assembles one biological
  snapshot from all active shard owners. Hidden layers, connection matrices and
  presence counters are selected from their receiving-layer owner; sensory/output
  populations and early cells are deduplicated; redundant/empty placement copies
  are not added to the biological aggregate. The aggregate clears `layer_range`
  so it cannot be rendered as a partial shard.

- [x] `2026-09-23 10:18Z` Cluster topology, edge caches and ordered layout counts
  are bound to a deterministic active-assignment digest. Placement changes and
  failed snapshot validation invalidate the biological cache, and the layout uses
  complete merged topology dimensions only when that digest matches the current
  registry. Focused owner-selection and assignment-generation regression tests
  were added.

- [x] `2026-09-23` Tightened cluster projection admission: every execution layer
  must have exactly one active owner, repeated or empty shard assignments are
  rejected, and biological output-stage ownership is selected from its assigned
  execution layer. Added a missing-owner regression test.

- [x] `2026-09-23 11:30Z` Rechecked `logs/sim_cluster_119969/` and the four
  newest `nm-17901622*.log` files. The current failure is earlier than UI
  projection: the orchestrator rejects every cluster cut because
  `hexapod_0_worker_01` has a mismatched network shape. The workers imported
  the same initial snapshot but continued local growth independently, so a
  strict multi-shard cut cannot claim one common biological state. The cluster
  UI now requests a complete topology witness from the preferred active IPC
  owner when that cut is unavailable, retains the current active-assignment
  digest, and refuses the witness when its biological layers are incomplete.

- [x] `2026-09-23` Cross-checked the local hexapod motor route. Rust and the
  companion preserve the authored 18-channel order (`lf/lm/lr/rf/rm/rr`, each
  `coxa/femur/tibia`), with output AER addresses based at `16384`; Java
  decoding returns zero-based indices without reordering. The visible-motion
  defect was in the Java mesh renderer, which animated only `segment_*`
  anchors and ignored the hexapod's `leg_*` hierarchy. Named Java/WebGL joint
  transforms and a per-channel geometry regression now cover all 18 endpoints.
- [x] `2026-09-23` Added Bedrock hexapod joint properties and hierarchical
  bones. The generated server entity exposes one client-synchronised float
  property per named motor endpoint, the resource animation rotates
  coxa/femur/tibia joint bones, and runtime output state resets on disarm and
  updates after each reply. The controlled Bedrock fixture verifies output
  index 0 changes `lf_coxa` while `lm_coxa` remains neutral.

## Validation and acceptance

- [x] `2026-09-23` `bash -n scripts/run_sim.sh`, `cargo fmt --all --check`,
  `git diff --check`, and `cargo test --locked --test run_examples_launcher`
  pass. The launcher suite reports 13 passed tests, including the new ordering
  assertion for Minecraft, Unity and Unreal. The Cargo build emits existing
  unused-code warnings only.

- [x] `2026-09-23` A rerun reached the new arm barrier but failed before the
  control RPC because `run_sim.sh` inherited local mTLS variables from
  `local_management_env.py`, while the selected Webots runtime profile removed
  them and served plaintext gRPC. The observed rustls error was
  `received corrupt message of type InvalidContentType`. The arm client now
  derives the shared Webots profile and clears TLS variables for plaintext
  profiles; `timeout 75s bash -c './scripts/run_sim.sh --sim minecraft
  --robots "hexapod=1" --nodes 3 --no-engine --no-build'` reached
  `Sent start to network 'hexapod_0' via http://127.0.0.1:38211`, confirmed
  Start on all three workers and created the arm marker. The no-engine process
  was stopped after that transport verification and cleaned up successfully.

- [x] `2026-09-23` `JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 ./gradlew
  --no-daemon test` passes all 14 Java 1.21.1 adapter tests, including the
  18-channel hexapod geometry regression. The 26.2 profile also passes with
  Java 25, Loom no-remap and its profile properties. Regenerated WebGL oracle
  fixtures match both Java profiles.
- [x] `2026-09-23` `python3 scripts/build_minecraft_bedrock.py`,
  `npm exec -- tsc -p tsconfig.json --noEmit`, and
  `node --experimental-vm-modules scripts/qa/test_minecraft_bedrock.cjs` pass.
  The controlled Bedrock suite reports 78 hexapod bones, 18 joint properties,
  and the expected endpoint response for output index 0.

- [x] `2026-09-15 15:31Z` Final exported-world NPC/Rust inquiry and clean shutdown
  pass in `target/qa/nao-social/bedrock-c3ua3jl_/`, with the exact exported-world
  hash. Bundle refresh, all 21 checksums and nine local documentation links pass;
  `target/nao-social/package-verification.json` records the delivered manifest.
  The installed user server remains untouched; all validation used fresh labs.

- [x] `2026-09-23 10:20Z` `cargo check --features 'ui,engine_runtime'` passes, and
  `cargo test --locked --features 'ui,engine_runtime'
  topology_presentation_tests --lib` passes all 3 focused tests. The narrower
  `ui,growth3d` feature combination is incomplete in this repository because it
  omits the `engine_runtime` feature's required OpenCL/morphology/parallel
  dependencies; the documented engine profile is the valid growth build.

- [x] `2026-09-23 11:01Z` Cluster snapshot and placement regressions pass with
  `cargo check --locked --features 'ui,engine_runtime'`, the active-owner range
  and assignment-stability tests, the cluster snapshot RPC test, and the
  `cluster_snapshot` test group. The RPC regression now includes a warm backup
  layer and confirms that only the active layer enters the biological runner
  range.

- [x] `2026-09-23 11:05Z` Final focused validation passes: all 52
  `distributed::tests`, all 4 `topology_presentation_tests`, all 13
  `run_examples_launcher` tests, `cargo fmt --all --check`, and `git diff --check`.
  The build retains pre-existing warning output only.

- [x] `2026-09-23 11:30Z` The topology-witness fallback compiles with
  `cargo check --locked --features 'ui,engine_runtime'`; `cargo fmt --all`
  and `git diff --check` pass. Runtime verification is pending a fresh
  `scripts/run_sim.sh --sim minecraft --robots "hexapod=1" --nodes 3` run.

All commands run at the repository root:

```sh
python3 scripts/regenerate_simulator_assets.py --check
cargo xtask qa run --suite simulator-content
cargo xtask qa run --suite simulator-minecraft
cargo xtask qa run --suite simulator-minecraft-world
cargo xtask qa run --suite simulator-minecraft-visual
cargo xtask qa run --suite simulator-minecraft-neural
cargo xtask qa run --suite simulator-minecraft-bedrock
cargo xtask qa run --suite simulator-minecraft-bedrock-native
cargo fmt --all --check
cargo test --locked -p xtask
cargo clippy --locked -p xtask -- -D warnings
python3 scripts/package_minecraft.py
(cd dist/minecraft && sha256sum -c SHA256SUMS)
```

Java sensing matches five poses per profile at absolute error 1e-10, mesh samples
at 1e-6 and movement at 1e-12. Tests distinguish reference/mocked gateways from
real snapshot transport. Native Java GameTest must pass and exit cleanly; source,
report and saved file hashes bind the ZIP acceptance. The development-only buffered
storage mixin preserves explicit flush/close and is excluded from release JARs.
Native QA observers are likewise excluded from Bedrock distributions.

`sim/minecraft/VALIDATION.md` is the concise evidence index, including per-profile
channels, spikes and measured round trips. The initial concurrent BANC run exceeded
its old 30 s budget and is retained as a failure; its focused rerun passed with
the final 60 s budget. Missing native engines fail requested lanes. A terminated
render probe is rendering evidence only, never clean-exit acceptance.

## Rollout, compatibility and rollback

Install Java in a separate compatible Fabric game directory. Install Bedrock packs
and scoped settings into a dedicated lab server/world; no Fabric JAR runs inside
Bedrock. Keep the companion beside Rust on the BDS host. Default auto detection
prefers an available Java launcher/profile, then BDS; Java engine preflight
reports missing Fabric/API/mod artifacts instead of silently switching frontends.
Forced editions never silently switch.
`--no-engine` permits brain/companion hosts without a game installation.
The dual-brain hexapod runner uses the direct Fabric path so the compatible
profile and configured world are selected automatically; ordinary launcher use
remains available through `scripts/minecraft.py launch` without `--direct`.

Generated artefacts are additive. Remove the adapter from a backed-up test instance
to roll back; do not open a modded save without its mod/backup. Per-run BDS settings
are disposable after shutdown, while linked canonical worlds persist normally.
No AARNN checkpoint or biological persistence schema is migrated.

## Risks and mitigations

Engine geometry/shading/physics differ: retain explicit fidelity limitations and
separate native evidence. Native fly/fish maps are legacy engineering proxies;
no guessed muscle assignment. Script beta APIs can change: pin declarations and
require actual startup readiness. Port checks have a spawn race: reserve until
spawn, retry only explicit bind failures, and report selected ports. World ownership
is independent of port availability and must never be bypassed.

## Surprises & Discoveries

- Initial stream content had surface markers but looked dry. User correction and
  native screenshots required actual water volumes across all exports.
- Native Java exposed asynchronous saved-entity duplication, support-floor
  z-fighting and unrelated test terrain; loading now waits for entities, support
  floor is separated, and saved region export is cropped.
- Vanilla GameTest shutdown used slow O_DSYNC writes. A test-only buffered storage
  profile fixes the test path; timeout was never accepted as clean shutdown.
- Large fly handshakes/steps need realistic bounded wall-clock budgets. A fixed
  1 ms AER1 timestep avoids the legacy raw-float header's set_dt behaviour.
- Bedrock stock manifests include JSONC, and native float properties require float
  JSON defaults. Static JSON/type fixtures alone did not catch those behaviours.
- Bedrock's native Rust replies may equal the request sequence while advancing
  beyond the prior output. Comparing them to the submitted sequence was incorrect.
- Native fish motion accessed a worm-only muscle map; branch selection now reads
  that map only for worms, and the API fixture drives connected motion for all six
  profiles. Native reload, like Java reload, also needs an explicit wait for saved
  entities before counting them. Preload/600-tick bounds avoid duplicate creation.
- Modern BDS can lack a LevelDB LOCK file. Linux process/world-directory checks
  supplement advisory locks; free listener ports never prove world availability.
- `run_sim.sh --node 3` was parsed and forwarded correctly. A one-layer network
  can have at most one active layer owner plus one distinct backup under the
  compatibility placement model, but the IPC owner is still a required live
  frontend service. Treating it as an ordinary removable source caused the
  observed connected-node count to fall from three to two after warm-copy
  handoff.
- The distributed simulator launcher deliberately disables runtime autostart so
  neural processing cannot begin before an environment has completed its
  handshake. The bridge's ready marker records the companion handshake, while
  the shared arm file gates binary AER traffic. Minecraft had stopped after the
  first marker and therefore needed the same explicit cluster-control start
  sequence already used by Unreal. Unity shared the defect; Webots and WebGL
  use autostart-enabled launchers and do not share this barrier.
- `local_management_env.py` provisions mTLS for the parent launcher, but the
  default Webots runtime profile intentionally unsets those variables because
  it excludes `management_v1`. The cluster-control client must apply the same
  profile boundary or it negotiates TLS against the plaintext orchestrator.
- The cluster UI previously accepted a successful one-shard biological snapshot
  as its topology/edge cache even after the placement registry expanded or
  changed. `decode_cluster_snapshot_projection` selected one shard for the UI,
  while layout dimensions came from the aggregate distribution. When a global
  snapshot then failed during placement churn, those two inputs remained visible
  together and produced the apparent biological-plus-ordered duplicate network.
  The decoder now assembles a single active-owner projection and suppresses it
  until the placement generation matches.
- The latest cluster logs show the deeper cause of the repeated snapshot failure:
  the compatibility planner alternated `hexapod_0` between active `[1]` with
  warm copies and active `[0..6]` on every telemetry cycle. The planner was
  evaluated before the already-valid assignment was considered, so noisy
  capacity/latency observations could enqueue competing `LoadNetwork` commands.
  In addition, a newly loaded worker derived its runner range from the hosted
  active-plus-backup union, while cluster validation expected only active
  ownership. This produced `hexapod_0_ipc has a mismatched layer range` and
  caused the UI to invalidate its biological cache and expose the ordered
  placement fallback.
- The newer `sim_cluster_119969` run confirms the same class of defect after
  placement stabilization: independent worker growth produces a mismatched
  network shape (`hexapod_0_worker_01`). Strict global cut assembly must remain
  closed in this state; the read-only UI now uses a complete active-node
  topology witness rather than presenting an ordered placement graph as
  biological topology.

## Decision Log

- `2026-09-15 MC-001`: Match inspiration's Fabric 1.21.1, but retain Rust-only neural
  authority and the shared catalogue. User request/specification boundaries govern.
- `2026-09-15 MC-002`: Keep the opt-in legacy sandbox explicit and production gates
  closed; preserve bounded credits, stop and ambiguous-failure behaviour.
- `2026-09-15 MC-003`: User requested water and native Bedrock. Supply native fluid
  content, BDS packs and a shared companion; no Geyser or Java-in-Bedrock claim.
- `2026-09-15 MC-004`: Validate supplied BDS in a new local world, preserving the
  running user installation. API fixtures supplement rather than replace native QA.
- `2026-09-15 MC-005`: User requires automatic port collision resolution each run.
  Reserve ports and isolate runtime settings; refuse a second writer on open worlds.
  Rollback consists of normal stop and selecting the original settings next run.
- `2026-09-15 MC-006`: Treat Java Minecraft targets as explicit compatibility
  profiles rather than accepting arbitrary version strings. A profile is usable
  only when its game version, loader/API/mod metadata, Java requirement and
  version-specific build artifact all agree. Keep 1.21.1 as the certified
  baseline while adding 26.2 only after the adapter compiles and its metadata is
  detected in the selected game directory.
- `2026-09-15 MC-007`: Preserve local IPC owners as named cluster workers during
  compatibility placement handoff. `--nodes` counts worker processes, while
  biological layer placement may use fewer active targets; the frontend owner
  must remain connected and addressable even when its topology cannot receive a
  distinct active layer. This changes lifecycle protection and observability,
  not logical time, event ordering or neural state semantics.
- `2026-09-15 MC-008`: Catalogue changes require explicit operator review before
  a saved lab can arm legacy sandbox inference. The review updates only the
  entity content marker after validating the complete profile set; it never
  silently retags a partial lab or restores neural state.
- `2026-09-20 MC-009`: Keep the dual-brain experiment in the legacy AER1
  compatibility lane. The Java companion has one profile route, so a local proxy
  is the narrowest way to exercise two independent Rust runners and preserve
  causal ordering. A fixture score may be reported as improved only when the
  measured coupled score is positive; no production protocol or biological
  control claim is promoted from this sandbox.
- `2026-09-23 MC-010`: Keep distributed simulator startup disarmed until each
  frontend handshake completes, then invoke the existing orchestrator `start`
  operation through `arm_distributed_networks`. Apply this barrier to every
  frontend that runs with `NM_DISTRIBUTED_AUTOSTART=0`; leave autostart-enabled
  Webots and WebGL launchers unchanged. This preserves the explicit simulator
  admission boundary and avoids retimestamping or dropping pre-arm AER traffic.
- `2026-09-23 MC-011`: Derive the arm client's gRPC transport from the same
  `webots_runtime_profile` used by `run_webot.sh`. Clear local mTLS variables
  for plaintext profiles and retain them for `management_v1`/`all-features`.
  This keeps the simulator control path protocol-compatible without weakening
  the production management TLS requirement.
- `2026-09-23 MC-012`: Treat the screenshot as a projection-consistency defect,
  not evidence of two logical networks. Cluster-global rendering must bind its
  biological topology, edge cache and ordered placement dimensions to the same
  placement/snapshot generation; a one-shard cache cannot silently remain the
  biological view after the cluster distribution changes.
- `2026-09-23 MC-013`: Preserve the existing cluster snapshot protocol and fail
  closed on duplicate active layer owners or incomplete biological topology.
  Build the UI aggregate at the presentation boundary from the authoritative
  active assignment, keeping warm redundant copies out of the biological graph.
  This changes only read-only rendering and cache lifetime; it does not alter
  runner ownership, logical time or distributed execution semantics.
- `2026-09-23 MC-014`: Treat a healthy published compatibility assignment as
  authoritative during telemetry churn. Replan only after a worker disappears,
  coverage becomes invalid or the eligible target set changes. Keep active layer
  ownership separate from hosted warm-copy layers in `ManagedNetwork` and the
  runner snapshot range so cluster cuts validate one biological topography.
- `2026-09-23 MC-015`: Keep strict cluster snapshot assembly fail-closed when
  independently evolved worker states cannot form one common cut. For the
  read-only cluster UI, use a complete biological topology witness from the
  stable IPC owner, bind it to the current assignment digest, and record the
  source in diagnostics. This preserves snapshot consistency while preventing
  the ordered placement fallback from masquerading as biological topology.
- `2026-09-23 MC-016`: Biological growth is scoped to the active area/layer/
  sub-shard owner. `BiologicalOwnershipMap` and its generation-fenced
  `BiologicalTopologyTransaction` publish local growth or a cross-area neuron
  migration as one commit, carrying state/synapse evidence and advancing both
  topology and partition generations at microstep zero. The active map contains
  one owner per stable neuron; warm copies remain durability metadata and cannot
  appear as a second biological graph. Growth into unoccupied volume expands
  the source area, while sustained pressure against the enclosing membrane
  commits bounded membrane expansion; overlap with another area's occupied
  volume is the condition that produces migration. The legacy runner path
  remains a compatibility path until it consumes this transaction boundary.
- `2026-09-23 MC-017`: Treat motor endpoint names as the cross-engine contract.
  Resolve Java/WebGL geometry through the generated output catalogue and expose
  the same names as Bedrock joint properties. Keep the 18-channel order and
  zero-based decoded spike indices unchanged; the adapters may differ in their
  rendering mechanism, but neither may invent a positional remap.

## Outcomes & Retrospective

Shared content, Java native world/rendering, all six real snapshot companion
checks and native BDS acceptance pass. Native BDS includes all six networks,
submerged fish, saved-entity reload, automatic port collision resolution and
clean shutdown. Java and Bedrock world exports and native adapter packages are
available, with the complete evidence index in `sim/minecraft/VALIDATION.md`.
Bedrock client rendering, Unity execution and calibrated cross-engine physics,
biological behaviour and remaining native channel migrations remain open.
No production phase gate is promoted.
