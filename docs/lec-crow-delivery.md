# LEC and crow expansion: implementation and remaining acceptance

This is a **partial delivery**, not a completed robot expansion. The repository
contains 28 canonical physical profiles and 224 explicit profile/target rows.
The [matrix](../qa/fixtures/lec-crow/CURRENT.md) retains all 224 rows with current
counts and evidence; there are **zero full passes**. Narrow passing checks remain
separate from full gates. Historical runs retain their exact source witnesses;
`source_matches_worktree` identifies differences from current source.

The [living ExecPlan](execplans/lec-crow-expansion.md) records phase ownership,
decisions, failed attempts and rollback. Existing network files and the supplied
task packs remain unchanged. No production phase gate is promoted.

The canonical `.agent/robot-expansion/` pack is tracked, including all 467
checksum-pinned licensed assets needed for fresh-checkout generation and native
loads. The duplicate installer pack, user-saved Webots world and runtime log
are excluded from the implementation commits.

MuJoCo now has named sensory/motor adapters for all six preserved profiles,
plus strict per-instance snapshot selection for all 28 physical profiles.
The original worm-only selection predicate is removed. Imported neuron IDs
remain separate from physical port names; BANC and FAFB cannot substitute for
one another. Unselected new profiles remain NPCs, including in mixed runs.

The latest per-profile probe results are:

| Profile | Sensory/output | Preserved-network result |
| --- | --- | --- |
| C. elegans | 24/96 | Pass: 47 exchanges, 98 steps with motor effort |
| Hexapod | 34/18 | Pass: 35 exchanges, 57 steps with motor effort |
| NAO | 250/40 | Pass: 17 exchanges, 92 steps with motor effort |
| Zebrafish | 32/32 | Correct endpoint and metadata alignment; neural placement exceeds 300s budget |
| Drosophila BANC | 418/48 | Correct endpoint; large preserved-snapshot placement exceeds 300s budget |
| Drosophila FAFB | 418/48 | Separate correct endpoint; large preserved-snapshot placement exceeds 300s budget |

Passing rows ran 1500 native physics steps with zero NPC steps and verified
private cluster/port cleanup. Evidence is retained in
`target/qa/lec-crow/legacy-neural-n7zg3gcc/legacy-neural.json`; the two separate
fly failures remain in `legacy-neural-dipz4mka/legacy-neural.json`. Those runs
predate the final launcher retry repair; their exact source differences remain
visible. Native/shared tests, all-28 ordered binding preflight and all eight
startup regressions pass. These checks do not validate learned tasks.

The final mixed test **passes** on matching source witnesses in
`target/qa/lec-crow/mujoco-population-kma0wd1c/`: two separate hexapod brains and
one worm brain each run across three workers, alongside an unbound UGOT NPC.
All four bodies complete 1500 native steps. Neural exchanges are 22/17/18 and
active motor steps are 92/92/67, with zero NPC steps on the three bound bodies.
UGOT completes 1500 NPC steps. Native rendering, independent step budgets,
original file hashes and private-port cleanup pass. Initial-load/current
placement checks avoid premature Start; missing acknowledgements permit bounded
idempotent Start retries through the same orchestrator. This changes startup
delivery, not neural clocks, reset, active expiry or production authority.
The inspected native framebuffer is `population/capture.png` in that bundle.
`target/qa/lec-crow/legacy-neural-preservation-final.json` verifies all six
original snapshots/configurations, the user's saved world, continued project-file
absence and shell syntax for ten launchers. No task-owned runtime remains.

The reconciled matrix has **144 not-run, 56 blocked and 24 failed rows**, with
zero full passes. Three MuJoCo preserved-brain imports remain failed; 20 Chaos
mechanics failures, the crow channel fault and Bedrock BANC failure remain
visible. Passing exchange tests do not close those separate gates.

Earlier persistent-worm viewer evidence remains in
`target/qa/lec-crow/mujoco-viewer-u0xgv_wt/managed.json` and native framebuffers:
111 exchanges/739 active steps in the default viewer and 81/370 with three
workers, both beyond 2500 physics steps. It is historical for the later source
changes. Known Chaos/BANC/channel failures remain unresolved when source
digests change; current row counts are in the matrix linked above.

## Implemented scope

The habitat/morphology repair replaces the native worm's boxes with 24 tapered,
overlapping capsules and an opaque cuticle, retaining its 23 named spine joints,
24 sensory inputs, 96 named muscle outputs and existing network. Annuli,
muscle-quadrant landmarks, pharyngeal bulbs and intestine are schematic visual
details; the opaque skin can hide the internal landmarks. The model deliberately
has no eyes. The inherited 0.34 m engineering body and mass are magnified
simulation assumptions, not literal C. elegans measurements.

All 28 native profiles now use habitats from the existing shared content
catalogue: agar, orchard, terrain, room, freshwater or crow woodland edge.
The agar scene adds nearby collidable food proxies, lawns, colonies, ridges,
refuge, contrast and warm/light cues. The front range measurement changes from
0.04066 m to 0.459875 m when its nearby food proxy is moved. Food and heat remain
ideal proximity/irradiance proxies; nutrition, chemistry and temperature are not
implemented. These objects provide actual geometry for supported range/contact
channels without inventing sensory channels or prescribing movement.

Fly/fish bodies and fly limbs also gain rounded shapes and shared schematic
anatomy; crow gains primary/tail feather and eye details. Static habitat geometry
propagates through MJCF, rigid graphs, Webots, USD, Unity/Unreal factories and
authoritative browser/game views. The browser now rounds capsule ends and draws
the floor flat. Native MuJoCo's **F** key switches between habitat and body
framing; Unreal's higher camera and fill light expose enclosure contents.
The improved geometry also exposed a real acceleration-frame error: MuJoCo's
inertial principal axes can change with shape. Rust now rotates world proper
acceleration into the named body frame explicitly, covered by the existing
rest/free-fall regression.

Cross-target limits remain explicit: Unity source is unexecuted and Isaac lacks
its runtime/client; Minecraft displays voxel bounds; browser imported meshes
use labelled bounds. The later native Rust fish adapter adds assumed buoyancy/drag;
other native factories still have display-only water and fluid calibration is open.
Unreal omits that translucent volume until a suitable material is implemented.
The preserved legacy Webots world is user-edited; earlier canonical-content
checks retained two population mismatches, and the user's subsequently saved
hexapod world is preserved. Other generated legacy assets match, and that saved
world is left intact. Richer scenery and
rounded bodies do not establish learned behaviour or biological calibration.

Current habitat evidence is retained in
`target/qa/lec-crow/habitats-cl08pgnw/`: `habitat-audit.json` covers all 28
profiles and 224 target rows; `rendered/all-28-bodies.png` and
`rendered/celegans-habitat.png` show actual MuJoCo framebuffers;
`worm-webgl.png` comes from the authenticated native service;
`webots/result.json` and `unreal/engine-result.json` retain representative
NPC mechanics, with `unreal/capture.png` showing the native scene. All nine
catalogued habitat checks pass. Final formatting, native-feature Clippy and
Minecraft Java/Bedrock client/build logs are in
`target/qa/lec-crow/habitat-repair/`. The latter client/build checks were run
against the same Java/JavaScript source; they do not certify rendered game
acceptance. The [changed-file manifest](../qa/fixtures/lec-crow/changes.current.json)
excludes the immutable input packs, user world/project and MuJoCo runtime log.

One compiler/registry produces all 28 MJCF/physical records, ordered IO contracts,
fresh untrained network recipes and USD articulations. A shared rigid-cluster
graph preserves fixed-link mass, COM, inertia and source collision masks. There
are seven distinct UGOT forms and distinct BANC/FAFB IDs.

The Rust crates `aarnn-embodiment`, `aarnn-embodiment-ffi` and
`aarnn-aer-legacy` share bounded asynchronous reference transport, projections,
actuation filters, NPC arbitration and aerodynamic panel forces. Native adapters
own physics and measurements. C/C# bindings derive from the Rust signatures.
Unity/Unreal use their required engine APIs around that common ABI; conversion
tools and the Python comparison runtime remain separate from Rust execution.

| Target | Observable implementation | Acceptance limit |
| --- | --- | --- |
| MuJoCo 3.17.0 | Rust owner/viewer, 28 native loads, named maps, sensors/forces, local snapshots/reset, private HTTP service, first-three real AARNN reference loops, named adapters/managed selection for all six preserved networks | Native transducers are versioned engineering models; calibration, learned tasks and cross-engine trajectories remain unvalidated. Original network startup/exchange is measured separately for every profile. Earlier ARM64 evidence predates these repairs |
| Webots R2025a | Generated 28 native worlds/maps and Rust controller; mixed contact worlds; all-28 NPC checks; first-three and mixed-fleet real reference exchange | Contact, mimic-compliance, biological transfer and task calibration incomplete |
| Unreal 5.8.0 CL55116800 | Native Chaos factory, shared Rust ABI, real constraints/sensors/forces, all-28 NPC and body-reset checks, first-three/reference and rendered evidence | Passive-energy falsification fails 20 profiles in the latest run; full lifecycle/checkpoint/parity and trained tasks open |
| Unity | Native Rigidbody/ConfigurableJoint factory and generated Rust ABI bindings | No Editor discovered; source has not compiled/executed in Unity |
| WebGL | Token-authenticated native authoritative physics service and viewer; independent viewer attachment, pause/step/disarm and local snapshot lifecycle | Per-profile browser/neural/task acceptance open; proxy rendering is labelled |
| Minecraft Java/Bedrock | Native-service observer clients, inert displays, atomic full-block collision mirrors; Java 1.21.1/26.2 compile; actual Java/Node client exchange with native HTTP | Partial blocks, players, doors, perches, capacity/unloaded chunks pause/disarm; full game/render/task acceptance open. Preserved Bedrock BANC startup regression fails |
| Isaac Sim | 28 offline native USD articulations; explicit separate-runtime boundary and capability failure | No supported installation discovered in configured/standard roots locally or on the four authorised hosts; native Kit client/lifecycle unimplemented. USD parsing is not Isaac execution |

`auto` control uses versioned programmed NPC motor intents until that body's
strict reference client is ready. A ready silent brain owns the body without NPC
movement. Handoffs clear previous drive; stop/reset inhibition wins. `npc`,
`silent` and `aarnn` select explicit experimental controls. NPC patterns are not
verified gait, navigation, task competence or learned neural performance.

Each reference binding pins a distinct brain ID, generation, loopback endpoint,
model and ordered IO digest. AER1 reference exchange is separate from
AARNN-AER/1 and FPAA UDP. The strict codec/reference adapter is tested; the
production protocol/committed-output/fencing integration remains gated by the
existing Phase 7/8 prerequisites. No physical robot, peripheral, lift or payment
service is connected.

## Reproduction

From the repository root, with the installed SDK:

```bash
export AARNN_MUJOCO_DIR=/home/pbisaacs/Developer/mujoco-3.17.0
export LD_LIBRARY_PATH="$AARNN_MUJOCO_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

./run_sim.sh --sim mujoco --robots galbot_g1_golf=1,ugot_mecanum=1,crow_corvus_corone=1 \
  --control auto --headless --steps 100 --output target/qa/npc-fleet.json
./run_webot.sh --robots ugot_mecanum=2,crow_corvus_corone=1 \
  --control auto --headless --steps 100 --output target/qa/new-empty-webots-run
scripts/run_unreal_sim.sh --robots ugot_mecanum=1 --control npc --headless \
  --steps 100 --reset-at 50 --output target/qa/new-empty-chaos-reset
scripts/run_webgl_sim.sh --physics-service --robots crow_corvus_corone=1 \
  --control silent --token-file /tmp/private-aarnn-physics.token --web-port 8092
```

Each engine output directory must be new/empty. Native Rust MuJoCo opens an
interactive viewer by default; `--headless --steps N` selects bounded diagnostics.

The reported C. elegans launcher mismatch is repaired. Both commands open the
native viewer and existing orchestrator dashboard, using the preserved snapshot:

```bash
scripts/run_sim.sh --sim mujoco --robots "celegans=1"
scripts/run_sim.sh --sim mujoco --robots "celegans=1" --nodes 3
# Start with a close body view; F switches back to the habitat.
scripts/run_sim.sh --sim mujoco --robots celegans=1 --view body
```

The second command distributes the same brain across three real workers.
Startup waits for placement, exact named 24/96 handshake and existing Start
control before sampling. Each body waits for its initial response before its
physics source clock advances. Ongoing packet latency still produces explicit
pre-admission gaps/expired output; the 100 ms stale-output rule is unchanged.
No programmed twitch is injected into neural control. Close the viewer or press
Esc to stop its owned cluster. Space pauses physics, R resets/disconnects and
inhibits output, left drag rotates and right drag or +/- zooms. `--control npc`
selects the separate programmed controller; `--control silent` selects passive
mechanics. The same launch path now selects hexapod, NAO, zebrafish and distinct
BANC/FAFB networks, with named contract checks before workers start. New profiles
remain NPC unless a matching per-instance `--network-map` selects a snapshot.
Mixed populations make that choice independently for each body.

```bash
scripts/run_sim.sh --sim mujoco --robots hexapod=1 --nodes 3
scripts/run_sim.sh --sim mujoco --robots nao=1
scripts/run_sim.sh --sim mujoco --robots zebrafish=1
scripts/run_sim.sh --sim mujoco --robots drosophila_banc=1
scripts/run_sim.sh --sim mujoco --robots drosophila_fafb=1
# Worm uses its preserved brain; the unbound UGOT remains NPC.
scripts/run_sim.sh --sim mujoco --robots celegans=1,ugot_mecanum=1
cargo xtask qa run --suite lec-crow-legacy-neural
cargo xtask qa run --suite lec-crow-mujoco-population
```

The focused population suite builds the canonical simulation profile and runs
the interactive three-brain/one-NPC check without repeating known large-import
failures. For a selected adapter repair, use for example:

```bash
NM_LEGACY_NEURAL_PROFILES='hexapod nao zebrafish celegans' \
  cargo xtask qa run --suite lec-crow-legacy-neural
```

That lane retains per-profile failures and continues; consolidation keeps the
latest measured result separately for every legacy profile. Its latest run
fails on zebrafish import, as listed above. The matching-source population
reproduction used the existing 600s startup budget and CPU reference selection:

```bash
NM_DISABLE_OPENCL=1 RAYON_NUM_THREADS=2 scripts/run_sim.sh --sim mujoco \
  --robots hexapod=2,celegans=1,ugot_mecanum=1 --nodes 3 \
  --tcp-ready-timeout 600 --steps 1500 --realtime \
  --output target/qa/new-population.json --screenshot target/qa/new-population.ppm
```

Startup uses the existing bounded `--tcp-ready-timeout` budget (default 600s),
now forwarded to the nested worker launcher and handshake. Active exchange keeps
its 5s network-fault and 100ms source-time output-expiry rules. Large imported
fly snapshots can take substantially longer to initialise than the worm.
Named preflight checks cover all 28 profiles and reject swapped channel order,
BANC/FAFB identity substitution and configuration shape mismatch before launch.
The QA builder resolves its feature arguments through the same
`scripts/webots_runtime_profile.sh` as the launcher. An all-features executable
requires its separate management configuration and cannot be reused under the
default simulation profile. The failed exploratory run is retained in
`target/qa/lec-crow/legacy-neural-m9kmaarc/`; no security check was disabled.

When snapshot dimensions and routing semantics already match, startup corrects
I/O encoding in a transient payload using the canonical metadata decoder. Equal
explicit routing changes only `net.spike_io`; a one-layer `None`/`Some(0)`
encoding difference may also update the two routing fields. Both encodings
resolve to H0 in the existing runtime.
All other raw biological/runtime fields and existing RNG state remain intact;
an absent seed follows the existing deterministic compatibility rule. Structural
changes retain the existing migration path. Regression tests cover this
path. Original snapshot files are never rewritten.

The [legacy adapter assumptions](../sim/content/embodiments/legacy-native-v1.json)
describe every changed experiment: flies use a proposed 48-channel antagonistic
leg readout plus declared wing seed pairs, with physical panel forces and no
oscillator/generic spike lift. Fish uses real local motion/depth/pitch, paired
muscles/fins, jaw/opercula, co-contraction, displaced-volume buoyancy and passive
drag. Those differ from old illustrative sensor/motor bridges; they do not prove
that a Webots-trained task transfers. Hexapod preserves named servo ranges,
torques, foot contacts, inertial and optical/sonar channels. NAO preserves 250/40
names on an original primitive surrogate whose OEM revision remains unresolved.
Silent initial output supplies no active drive. Old local-body snapshots are
rejected by digest; original neural files are unchanged. Rust owns these native
adapters; `--reference-python` explicitly rejects the unsupported legacy sensor
path. Other engines keep their existing legacy neural routes and cannot claim
this new native mapping from an exported mesh/graph alone.

```bash
# Bounded real three-worker exchange, with retained state and concise console.
scripts/run_sim.sh --sim mujoco --robots celegans=1 --nodes 3 \
  --headless --steps 1500 --realtime --output target/qa/celegans-mujoco.json
# Deterministic CPU-reference test selection, native rendering and cleanup:
cargo xtask qa run --suite lec-crow-mujoco-managed
cargo xtask qa run --suite lec-crow-mujoco-viewer
```

The viewer suite exercises both default interactive commands beyond the old
1000-step limit, captures native PPM images and checks private port/process
cleanup. Its tests preserve hashes of the network, configuration and unrelated
Webots world. SDK-only environments can run the headless suite; interactive
mode additionally needs system GLFW 3, an OpenGL driver and a display. A local
driver emitted `OpenGL error 0x502 in or before mjr_makeContext`; native images
rendered successfully, retained separately from mechanics/task evidence.

The named muscle readout is a Rust port of the repository's response v2, with
MDL/MDR/MVL/MVR and MVULVA kept distinct. Its versioned assumptions and limits
are in [celegans-simulation-v2.json](../sim/content/embodiments/celegans-simulation-v2.json).
Tapered capsule geometry, motor/traction calibration and ideal proximity/light/heat
proxies differ from Webots. This repairs neural connectivity and interactive
launching; learned locomotion, trajectory parity and measured biology remain
unvalidated. Model digest changes intentionally reject incompatible local body
checkpoints. Existing neural snapshots/checkpoints are untouched.
Webots/Unreal have native interactive launch without `--headless`. Isaac's
`NM_ISAACSIM_PYTHON` must point to its own supported installation; never use the
MuJoCo venv. Unity requires `NM_UNITY_EDITOR`. See
[embodiment guidance](../sim/content/embodiments/README.md) and
[Minecraft service configuration](../sim/minecraft/PHYSICS.md).

The following exact commands retain unique bundles under `target/qa/lec-crow/`
and preserve all 224 rows:

```bash
cargo xtask qa run --suite lec-crow
cargo xtask qa run --suite lec-crow-habitats
cargo xtask qa run --suite lec-crow-shared
cargo xtask qa run --suite lec-crow-rust-service
cargo xtask qa run --suite lec-crow-rust-neural
cargo xtask qa run --suite lec-crow-webots-neural
cargo xtask qa run --suite lec-crow-unreal-neural
cargo xtask qa run --suite lec-crow-npc
cargo xtask qa run --suite lec-crow-fleet
cargo xtask qa run --suite lec-crow-unreal-passive
cargo xtask qa run --suite lec-crow-unreal-lifecycle
cargo xtask qa run --suite lec-crow-unreal-render
cargo xtask qa run --suite lec-crow-minecraft-service
cargo xtask qa run --suite lec-crow-launchers
cargo xtask qa run --suite lec-crow-usd
NM_EMBODIMENT_QA_SSH=pbisaacs@192.168.1.66,pbisaacs@192.168.1.68,pbisaacs@192.168.1.70,pbisaacs@192.168.1.72 \
  cargo xtask qa run --suite lec-crow-capabilities
cargo xtask qa run --suite simulator-minecraft-bedrock-native
python3 scripts/qa/consolidate_embodiments.py
python3 scripts/qa/report_embodiment_changes.py
```

The passive, fleet and preserved Bedrock native lanes may fail for the recorded
reasons below. Failure is retained, not converted to success. The rendered lane
captures native geometry after one 2 ms step; its screenshot is appearance
evidence only. The separate 100-step silent lane measures contact energy.

Formatting/lint commands used:

```bash
cargo fmt --all --check
cargo clippy --locked -p aarnn-aer-legacy -p aarnn-embodiment \
  -p aarnn-embodiment-ffi -p aarnn-mujoco -p aarnn-webots --all-targets \
  --features aarnn-mujoco/service,aarnn-webots/native -- -D warnings
git diff --check
```

All five root/convenience launcher tests pass, including real root MuJoCo,
both Webots entry points, both WebGL/Minecraft service wrappers, asynchronous
stop inhibition and unavailable-engine/unsupported-remapping errors. Both
root and `scripts/` launch paths share the canonical dispatch.

Source regeneration commands and pinned conversion environments are documented
in the embodiment README. All 467 bundled asset hashes and all generated source
freshness checks pass. Unity/Isaac environment absence is not a passed lane.

ARM64 native testing ran in the isolated authorised `.72` directory
`/tmp/aarnn-robot-qa.rZrZtzID/project` with:

```bash
export RUSTUP_HOME=/tmp/aarnn-robot-qa.rZrZtzID/rustup
export CARGO_HOME=/tmp/aarnn-robot-qa.rZrZtzID/cargo
export PATH="$CARGO_HOME/bin:$PATH"
export AARNN_MUJOCO_DIR=/tmp/aarnn-robot-qa.rZrZtzID/mujoco-3.17.0
export LD_LIBRARY_PATH="$AARNN_MUJOCO_DIR/lib"
export LIBCLANG_PATH=/lib/aarch64-linux-gnu
cargo test --locked -p aarnn-embodiment -p aarnn-embodiment-ffi \
  -p aarnn-mujoco --features aarnn-mujoco/service
```

Rust 1.92.0 was installed only in that task directory; no login PATH or existing
GPU workload was changed. Retained ARM evidence is
`target/qa/lec-crow/arm64-current/{native-test.log,result.json,source-verification.json}`.
The source verification distinguishes 22 compiled files that matched the
workspace at that run from three unused older task-staging files. This evidence
predates the habitat/acceleration repair; the current matrix marks its source
differences and does not certify the new code on ARM64. `.72` reports ARM64 and
RTX 2000 Ada 16 GB; these tests use CPU, not GPU/Isaac.

## Evidence and unresolved work

The machine-readable [matrix](../qa/fixtures/lec-crow/acceptance-matrix.current.json)
contains exact latest bundle paths, commands, model/IO hashes, narrow subchecks,
failures and source-currency information. Representative retained evidence:

- `target/qa/lec-crow/shared-3pj_z0zh/`: actual shared ABI/projection/graph checks.
- `target/qa/lec-crow/rust-neural-pgys3_7l/`: first-three native MuJoCo/real Runner
  exchange with the actual `NM_DISABLE_OPENCL=1` CPU selector verified in logs.
- `target/qa/lec-crow/npc-ftay8qu1/`: all-28 programmed NPC mechanisms in
  MuJoCo/Webots/Chaos. This does not override failing Chaos mechanics.
- `target/qa/lec-crow/unreal-lifecycle-kj_v7u_l/`: all-28 body resets, stream
  invalidation and inhibited output after exactly 50 NPC steps.
- `target/qa/lec-crow/unreal-render-5o8t_i49/`: inspected actual framebuffer,
  source geometry only.
- `target/qa/lec-crow/unreal-passive-wrv8sjrz/`: 20 latest-recorded Chaos failures of
  the passive-energy tolerance `max(0.01 J, 5% of initial energy)`. No neural/NPC
  drive is active. Solver/contact/coupling/rotor-inertia equivalence needs work;
  source/model changes do not erase these unresolved failures pending retest.
  mass/drive inflation and invisible supports are not used to hide this.
  A removed premature low-level collision-manager experiment crashed before
  solver registration. The installed engine automatically uploaded that crash
  report; subsequent QA launches explicitly override crash/usage reporting off
  without changing the user's global settings.
- `target/qa/lec-crow/fleet-mvrkmcrn/`: latest mixed-world Webots succeeds;
  Chaos UGOTs retain independent progress while crow's reference channel times
  out (`Resource temporarily unavailable`, 5 s socket budget). Earlier
  `fleet-2v3zomg5` completed both targets; the later failure is preserved and
  prevents a healthy-fleet completion claim.
- `target/qa/minecraft/bedrock-native-tqx5_icc/`: real C. elegans BDS exchange
  passes; unchanged 661 MB BANC snapshot does not reach listener readiness in
  180 s, with CPU mode verified. FAFB and remaining snapshots are not promoted
  from the earlier partial run.

Still required: native Isaac Kit client and lifecycle; real Unity execution;
calibrated native contacts/constraints, legacy task-transfer calibration and
large preserved-brain import performance;
complete Minecraft collisions/rendered/native interaction lanes; stateful
delivery, cleaning and kiosk processes; crow feeding/perching/ground/flight
skills and conservation; trained curricula and held-out/silent/random controls;
full fault, performance, parity and production-governed protocol/effect gates.

Crow is an original reduced **Corvus corone** model (0.510 kg, 31 controlled
joints) with proposed 492-input/64-output projections, not a measured crow
brain. Wing panels, self-contact suppression, ideal transducers and assumed
25 W/20 kJ physiology require calibration. Local snapshots exclude remote brain
state and in-flight AER; they do not prove deterministic closed-loop replay.
Exact OEM/LEC variants stay open where source data is unavailable. Original
functional surrogates and explicit assumptions do not close replica gates.

## Changed files and licences

[changes.current.json](../qa/fixtures/lec-crow/changes.current.json) lists every
task-owned changed path, size, mode and SHA-256, excluding the immutable input
packs and itself. Main changes are the three shared Rust crates; native
MuJoCo/Webots adapters; canonical compilers/assets; Unity/Chaos factories;
WebGL/game clients; root/convenience launchers; xtask/QA; and this documentation.
No neural runtime, original network or production schema is replaced.

[NOTICE.md](../sim/content/embodiments/NOTICE.md) and copied licence texts retain:
official Galbot Golf Apache-2.0 revision
`2d496b053f0d4e9e2688f59fac66022f447226be`; community UGOT MIT revision
`ef935a3f22a179ae815e1d0c5108a082de1d0432`. MuJoCo, Webots, OpenUSD, Unreal,
Unity and Minecraft retain their separate upstream/runtime licences. No new
repository-wide licence, gated crow mesh, unlicensed NAO geometry or vendor
permission is invented.
