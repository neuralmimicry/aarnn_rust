# Enable player interaction with NAO in simulator reference worlds

This ExecPlan is a living document maintained under `.agent/PLANS.md`.

## Purpose and observable outcome

Players can address NAO using focused text or explicitly enabled speech transcription, see their message's delivery state and a neural-selected reply, and stop their conversation. Minecraft Java and Bedrock use ordinary player chat commands and an in-world bubble; native engines and WebGL expose a conversation window. One versioned transducer and decoder serves all six environments. The initial social repertoire is an engineered neural reflex vocabulary, not a trained general language model.

## Specification authority and traceability

Sections 16.15–16.24, 17.4, 19.3, 20.9–20.10 and 21.11–21.15 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md`; INV-002, INV-007, INV-008 and INV-015–017. Reference scenarios cover bounds/provenance (UT-IOSAMPLE-001), deduplication and stop (UT-EFFECT-001), two-player isolation (IO-E2E-007), failure (CT-013), transduction and sandbox presentation (IO-E2E-001/004), and browser permissions/accessibility (21.11). These mappings do not certify production acceptance.

## Prerequisites and phase boundary

Phase 8's production gate remains unmet. The existing `nn_tcp_server`, simulator adapters and Minecraft companion are explicitly opted-in legacy AER1 reference sandboxes. This change remains within that compatibility boundary. It does not enable `workstation_io`, change `/api/aer/infer` authorisation, control a live brain, or add a second neural executor. All neural stepping remains in the existing Rust Runner, in a fresh task-owned instance.

## Scope

Add a separately generated NAO social snapshot with additive I/O and labelled neural pathways; a bounded local interaction/transducer proxy; focused chat, optional capability-detected speech transcription and speech presentation; scoped ordinary-player interfaces for Java/Bedrock; launch/install instructions, contract/browser/native/real-neural verification and refreshed affected packages.

## Non-goals

No external LLM, ambient microphone interception, Minecraft voice-chat interception, general language understanding, physical-robot actuation, global HID, public unauthenticated listener or production commit/EffectId claim. Native single-player demo scenes do not become authenticated multiplayer engines through this change; their reusable player-facing component must state that integration boundary.

## Repository orientation

Root `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`, branch `codex/simulator-content-parity`; root Rust package plus `tools/xtask`, no new workspace. Root AGENTS, `.agent/PLANS.md`, Phase 8 and relevant specification sections were read. Prior discovery covered manifests, CI, deployments, mobile/signing and ADRs. The dirty tree contains completed simulator/Minecraft work and unrelated changes in Rust runtime/UI/scripts; preserve all of them.

Canonical paths: `scripts/build_nao_network_json.py`, `network_nao.json`, `webots_world/configs/config_nao_webots.json`, `scripts/robot_profiles.py` (NAO 250/40); `examples/nn_tcp_server.rs` (length-prefixed AER1/raw floats, Rust Runner); `scripts/run_sim.sh`; `web_ui/webgl-sim.{html,js}`; Unity `Runtime/Robots/NmNaoRobot.cs`; Unreal `Robots/NmNaoActor.{h,cpp}`; Webots `controllers/nao_nn_controller_uds/nao_nn_controller_uds.cpp`; Java `AarnnMod.java`, `SceneEntity.java`, `Gateway.java`, `LabWorld.java`; Bedrock `scripts/{runtime,server,session}.js`. Generated Bedrock packs come from `scripts/build_minecraft_bedrock.py`; Minecraft packages from `scripts/package_minecraft.py`. Existing QA: `cargo xtask qa run --suite simulator-minecraft[-world|-bedrock|-bedrock-native]` and simulator-content.

## Architecture and safety constraints

NAO's existing 250 inputs and 40 outputs remain intact in the source snapshot. Generate a separate schema-labelled social profile; reject mismatched dimensions and stale content. A transport/transducer proxy adds communication receptors and consumes communication effectors while keeping the original body interface available. No neural implementation in Python/Java/JS/C#/C++.

Communication inputs retain source/session identity, monotonic source sequence, capture timestamp, modality, transducer version and explicit reference-frame admission. The legacy transport cannot prove production capture-clock mapping/durable admission; never label reference metadata as such. Queues and payloads are bounded before admission. One active turn at a time, fair FIFO across bounded players, no ambiguous retry, separate per-player stop, all sessions revoked on simulation disconnect. No user text goes to ordinary logs or persistent world data. Replies require actual social output activity; silence yields a status, never fabricated NAO speech.

Operator opt-in selects one world/NAO instance. A separate player credential grants chat only, not model management. Minecraft server code derives player identity, dimension and distance from engine state. Browser invitations are scoped to the selected running sandbox; no admin credential is shared. Capture and TTS require independent local choices and stop locally on navigation/disconnect. Browser speech recognition may use the browser vendor's service; disclose that before opt-in and retain editable transcripts.

## Milestones

1. Generate/verify additive social model and contract. Preserve every old label and matrix entry, append isolated social relay neurons and speech-act outputs. Run the real Rust snapshot to establish response behaviour before promising it.
2. Implement bounded interaction service with source provenance, player sessions, queueing, deduplication, timeout/cancellation and transport dimension enforcement. Test two players, malicious input, stale output and disconnects.
3. Integrate chat windows and ordinary-player Java/Bedrock commands with proximity and lifecycle checks. Share the contract/decoder and expose accurate voice capability. Test available engines and browser; unavailable Unity execution remains `not-run`.
4. Add launch/installation guidance, QA scenario/xtask lane; rebuild affected Java/Bedrock artefacts, update verification evidence and exports where required.

## Progress

- [x] `2026-09-15 13:49Z` Discovery: existing NAO I/O 250/40 fully assigned; default snapshot has 4×64 hidden neurons, sensory target layer 1, output source layer 3. Existing generator is heuristic, with no established language training. Native engine output labels disagree with the actual snapshot's 40 Webots motor names; none may be reused as communication channels. (Corrected an erroneous initial 18:00 timestamp.)
- [x] `2026-09-15 14:14Z` Resumed verification: real Rust evidence `target/nao-social/h0-neural.json` proves eight finite speech acts. The example's LIF path requires H0 sensory entry; H1 probes were silent. The generated model explicitly records H0 entry and disabled AARNN delays/growth/import rewiring, retaining the original matrices and source snapshot. It is a new LIF reference model, not calibrated AARNN language biology.
- [x] `2026-09-15 14:14Z` Rust release example, Java main/client, Bedrock TypeScript and Webots controller built. Unreal chat compiled with UE 5.2 but the project requires the installed UE 5.8 at `/home/pbisaacs/Developer/Engine`; rerun the whole project with that engine. Unity remains unavailable.
- [~] `2026-09-15 14:14Z` Latest user scope: autonomous same-world robot/NPC/player communication using existing capabilities. Add bounded observable participant cues and a neural encounter/question circuit for NAO, preserving species modality limits. Service lifecycle/auth tests, native interaction evidence and refreshed packages remain required.

- [~] `2026-09-15 15:03Z` Resumed from retained evidence. Full native BDS metadata restores `minecraft:villager_v2` identity (`bedrock-social-vbzehrzb/native.log`); the handmade 169-byte metadata omitted native version/storage defaults. Replace fresh QA world bootstrap with native creation and byte-preserving experiment activation; never patch installed user worlds. Inquiry still absent: the four-tick interval's `currentTick % 20` condition can permanently miss its cadence after non-zero startup. Add offset-cadence regression and real native NPC assertion before recertifying packs.
- [x] `2026-09-15 15:07Z` Ten Python behaviour/model tests, three new metadata preservation/rejection tests, offset-cadence native-script fixture, official API types, detection and port tests pass (`bedrock-ezsbn3o3/`). `cargo fmt --all --check` and the xtask unit test pass after formatting only the new native-suite dispatch. Fresh BDS bootstrap initially omitted the world directory needed by the managed mount; corrected before rerun. All-six native rerun `bedrock-native-zbgkt3b4/` is in progress and already proves native water and vanilla NPC identity.

- [~] `2026-09-15 15:16Z` Native inquiry trace proves a startup race: `/api/encounter`
  returned 409 because the eagerly negotiated body had been idle for over ten
  seconds, before the first real body frame refreshed it (`bedrock-o9c2xsv4/`).
  Bedrock and Java now wait for the first validated neural output before admitting
  encounters. Controlled delayed-first-frame and JVM lifecycle assertions cover
  this; rerun native interaction before the final all-six source certification.

- [x] `2026-09-15 15:21Z` Real native inquiry passes in `bedrock-elazvas5/`: newly
  installed exported world, actual vanilla villager, ordinary native encounter
  scan, companion JAR, social proxy, real Rust question and native name-tag bubble.
  The probe adds only a QA NPC/bubble observer; `--trace` is optional and was off.
  Clean shutdown passed. Java all-six world/JVM tests and refreshed JAR/world
  export pass in `world-tlmb7e_5/`; final Bedrock controlled/API/detection/port and
  metadata tests pass in `bedrock-p3dzqchb/`. Final all-six BDS source recertification
  is running before package refresh.

- [x] `2026-09-15 15:29Z` Final all-six BDS certification passes with the readiness
  fix in `target/qa/minecraft/bedrock-native-82l2875w/` (386.5 s), including native
  water, NPC identity, stop/reload, port collisions and clean exit. The previous
  receipt was deliberately rejected after the source change (`stale-package-rejection.log`).
  Java `world-tlmb7e_5/` includes 13 passing JVM tests with no skips. Source
  regeneration, Python syntax, Rust formatting, xtask unit/strict Clippy and diff
  checks pass. The exact new export is undergoing its final social install check.

## Validation and acceptance

- [x] `2026-09-15 15:31Z` Final exported-world install/inquiry passes in
  `target/qa/nao-social/bedrock-c3ua3jl_/` (BDS 1.26.45.1, 16.57 s), including clean
  shutdown and a recorded world SHA matching the native certificate. Package
  staging passes; all 21 SHA256SUMS entries and nine local documentation links
  pass. Evidence: `target/nao-social/package-verification.json`. Installation
  guides distinguish Java 1.21.1 from the installed 26.2 profile and preserve
  isolated Bedrock installation and port allocation on every launch.

SIM-NAO-INTERACTION-001 exercises unchanged base weights/labels, neural response versus disconnected silence, UTF-8 text/transcript equivalence, malformed/oversized text, bounded per-player queues, scoped credentials, cancellation, stale replies, simulation disconnect, origin checks and visible chat. Real Rust inference is mandatory for the neural lane. Minecraft native-world certificates/packs become stale when their sources change and must be regenerated before repackaging. Native Webots/Unreal suites prove body/Rust/chat HTTP integration; the Bedrock suite proves an actual NPC inquiry/name-tag exchange. Native-window clicks, physical microphones, Unity execution and Bedrock client rendering remain distinct unverified capabilities.

## Rollout, compatibility and rollback

Opt-in only. Generate to build output; do not overwrite `network_nao.json` or any checkpoint. Stop task-owned processes to revert, then use existing NAO launch commands. Old 250/40 snapshots remain usable. Enhanced snapshots require the matching social interface/proxy; never silently resize a saved brain. Do not modify the user's Minecraft profile, mods, saves or running BDS installation. Installation is into an isolated instance; existing port collision/world-writer protection remains in force.

## Risks and mitigations

Engineered reflexes could be mistaken for fluent language: disclose the finite repertoire and model provenance beside chat. Browser STT may be unavailable or remote: feature-probe, explain data handling and retain typed input. Delayed neural output can be assigned to a later speaker: isolate turns, drain between them, and reject old generations. Payloads may contain markup/commands: render plain text only, with no command execution. Legacy native mappings are approximate: preserve the original channels and document the discrepancy rather than claiming calibrated sensory equivalence.

## Surprises & Discoveries

The Unity/Unreal comments call some outputs LEDs/reserved, but `network_nao.json` labels all 40 outputs as motors, including 16 finger phalanges. Raw-float `nn_tcp_server` treats its leading float as dt despite naming it current time; the new proxy will use AER1 for its Rust connection and a pinned dt, avoiding that ambiguity.

Native BDS accepted the earlier minimal `level.dat` and passed robot-only checks,
but a spawned vanilla villager reported a habitat identity. Complete BDS-generated
metadata fixed that independent defect. The new exporter requires vanilla NPC
identity before and after reload. The first native inquiry then exposed an eager
connection that was idle before any body frame: the service correctly rejected
the encounter. Both Minecraft adapters now wait for validated neural output
before scanning or accepting social input. A four-tick script callback also need not align
with `currentTick % 20 == 0`; elapsed-tick scheduling and an offset fixture replace
that assumption. The companion's eager connection must survive idle game startup;
the proxy waits for readability before applying its incomplete-frame deadline.

## Decision Log

- 2026-09-15 D1: Use an additive, separately generated reference social profile and shared proxy rather than stealing channels or resizing existing snapshots. This preserves compatibility and one Rust neural authority (19.3).
- 2026-09-15 D2: Use the existing Rust neural runtime for every reply. No external LLM dependency or credential is introduced.
- 2026-09-15 D3: Autonomous nonverbal exchange remains physical scene input/output through each robot's existing modalities. NAO's new encounter receptor and question output are an explicitly engineered curiosity reflex, driven by actual Rust output; encounter messages are never recursively forwarded or translated into unsupported species channels. No trained understanding or learning improvement is inferred from this mechanism.
- 2026-09-15 D4: Bootstrap only fresh QA worlds through BDS, then preserve all native metadata while enabling the required experiments. Replace the development world export after native acceptance; do not transplant metadata into an existing world. This preserves the user-world rollback boundary and avoids an unsupported world-format migration.

## Outcomes & Retrospective

The six adapters, shared participant contract, additive social model and bounded
interaction service are implemented. Nine real Rust speech acts, browser flow,
native Webots/Unreal body exchange and a native Bedrock NPC inquiry pass. Final
Minecraft source certification, exported-world interaction, package checksums
and installation links pass. Unity and
Bedrock client visual acceptance remain unavailable; physical/biological parity
and general language understanding are not established. Production Phase 8 is
unchanged.

### Historical verification update — 2026-09-15 14:33Z

- `cargo xtask qa run --suite simulator-nao`: pass, `target/qa/nao-social/run-mh9lcfm_/`; nine actual Rust-selected acts including encounter inquiry, with input/frame/output evidence.
- `simulator-nao-browser`: pass, `target/qa/nao-social/run-nt9o9l_0/`; real Chromium and Rust, focused text, inquiry, reply bubble, edited STT fixture, independent TTS fixture, stop/rejoin and mobile screenshot. A first attempt lacked the matching Chromium binary; rerun used `NODE_PATH=/tmp/aarnn-sim-browser/node_modules` and `NM_CHROMIUM=/home/pbisaacs/.cache/ms-playwright/chromium-1223/chrome-linux64/chrome`. The screenshot prompted a side-by-side chat/world layout improvement, to be rechecked.
- Nine behavioural Python tests pass: model preservation, per-player identity, duplicate/conflict, queue/UTF-8/frame bounds, encounter modality gates, cancellation during inference, stalled body, disconnect and credential/origin separation.
- `simulator-minecraft-bedrock`: pass, `target/qa/minecraft/bedrock-jouk9aq5/`; controlled ordinary-player commands, world/identity/lifecycle, type declarations, detector and real-socket port tests.
- `simulator-minecraft-world`: pass, `target/qa/minecraft/world-t5byie3w/`; actual Java world, all six robot NPC observations, water, reload/disarm, clean exit, rebuilt mod/companion and saved-world certificate.
- Native BDS refresh initially failed safely on an occupied block during tank creation (`bedrock-native-ehbe378d/`). Inspection showed interleaved wall/water construction could let fluid updates run between job yields under load. The builder now completes the glass shell first and accepts flowing water as the equivalent requested fluid while preserving occupied-area rejection. Rerun underway. This is a timing-independent construction fix, not a waiver of the world-preservation rule.
- The original running BDS PID 265970 remains running; no user installation/account/save has been modified.
