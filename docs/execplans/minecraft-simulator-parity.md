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
- `examples/nn_tcp_server.rs`, `scripts/tcp_aer_ipc_bridge.py`: existing Rust
  sandbox endpoints. No additional Rust workspace or neural implementation.
- `sim/minecraft/src/`, Gradle build and `scripts/minecraft.py`: Java adapter,
  companion, detection and certified saved-world export.
- `scripts/build_minecraft_bedrock.py`, `sim/minecraft/bedrock/scripts/`,
  `scripts/bedrock.py`, `scripts/bedrock_ports.py`: generated native BDS adapter,
  scoped configuration, versioned-install discovery and safe managed launch.
- `scripts/qa/run_minecraft.py`, `probe_minecraft_{neural,bedrock}.py`,
  `bedrock_native_probe.js`, contract/detection/port fixtures and `tools/xtask`:
  bounded native/reference lanes and retained evidence.
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

## Validation and acceptance

- [x] `2026-09-15 15:31Z` Final exported-world NPC/Rust inquiry and clean shutdown
  pass in `target/qa/nao-social/bedrock-c3ua3jl_/`, with the exact exported-world
  hash. Bundle refresh, all 21 checksums and nine local documentation links pass;
  `target/nao-social/package-verification.json` records the delivered manifest.
  The installed user server remains untouched; all validation used fresh labs.

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
prefers compatible Java, then BDS; forced editions never silently switch.
`--no-engine` permits brain/companion hosts without a game installation.

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

## Outcomes & Retrospective

Shared content, Java native world/rendering, all six real snapshot companion
checks and native BDS acceptance pass. Native BDS includes all six networks,
submerged fish, saved-entity reload, automatic port collision resolution and
clean shutdown. Java and Bedrock world exports and native adapter packages are
available, with the complete evidence index in `sim/minecraft/VALIDATION.md`.
Bedrock client rendering, Unity execution and calibrated cross-engine physics,
biological behaviour and remaining native channel migrations remain open.
No production phase gate is promoted.
